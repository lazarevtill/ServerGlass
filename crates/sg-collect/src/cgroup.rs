//! Per-container and per-VM metrics from the cgroup v2 hierarchy.
//!
//! This is the agentless answer to "what are my containers doing". The usual approach talks to the
//! Docker socket, which means the socket must be exposed, the user must be in the `docker` group,
//! and the whole thing only works for Docker. The kernel already accounts every container, VM and
//! service in `/sys/fs/cgroup`, in files any user can read.
//!
//! That covers more than Docker for the same effort:
//!
//! | Runtime | Path |
//! |---|---|
//! | Docker (systemd driver) | `/sys/fs/cgroup/system.slice/docker-<id>.scope` |
//! | Docker (cgroupfs driver) | `/sys/fs/cgroup/docker/<id>` |
//! | Podman | `/sys/fs/cgroup/machine.slice/libpod-<id>.scope` |
//! | LXC, including Proxmox containers | `/sys/fs/cgroup/lxc/<name>` |
//! | QEMU, including Proxmox VMs | `/sys/fs/cgroup/qemu.slice/<vmid>.scope` |
//!
//! cgroup v2 only. v1 splits the same numbers across a different file per controller, and the
//! hosts that still run it are old enough not to be the ones this is aimed at.

use sg_model::{
    Entity, EntityKind, ParseResult, Request, Requirements, Responses, SampleSink,
    SeriesDescriptor, Source, SourceDescriptor, SourceId, TargetCtx, Unit,
};

/// One cgroup's readings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CgroupStats {
    /// Full cgroup path, as the kernel reports it.
    pub path: String,
    /// Cumulative CPU time in microseconds. A counter.
    pub usage_usec: u64,
    pub memory_bytes: u64,
    /// `memory.max`, absent when the cgroup is unlimited (the file reads `max`).
    pub memory_limit: Option<u64>,
    pub pids: u64,
}

impl CgroupStats {
    /// A name a person would recognise, and what kind of thing this is.
    ///
    /// Container ids are 64 hex characters; the first twelve are what every Docker and Podman
    /// command line shows, so that is what is displayed. LXC and QEMU cgroups are already named
    /// after the guest.
    pub fn identify(&self) -> (EntityKind, String) {
        let leaf = self.path.rsplit('/').next().unwrap_or(&self.path);
        let parent = {
            let mut parts = self.path.rsplit('/');
            parts.next();
            parts.next().unwrap_or("")
        };

        let short = |id: &str| id.chars().take(12).collect::<String>();

        if let Some(id) = leaf
            .strip_prefix("docker-")
            .and_then(|s| s.strip_suffix(".scope"))
        {
            (EntityKind::Container, short(id))
        } else if let Some(id) = leaf
            .strip_prefix("libpod-")
            .and_then(|s| s.strip_suffix(".scope"))
        {
            (EntityKind::Container, short(id))
        } else if parent == "docker" {
            (EntityKind::Container, short(leaf))
        } else if parent == "lxc" || parent == "lxc.payload" {
            (EntityKind::Container, leaf.to_string())
        } else if parent == "qemu.slice" {
            let vmid = leaf.strip_suffix(".scope").unwrap_or(leaf);
            (EntityKind::VirtualMachine, format!("VM {vmid}"))
        } else {
            (EntityKind::Container, leaf.to_string())
        }
    }

    /// Whether this cgroup is running anything.
    ///
    /// The hierarchy contains scaffolding as well as workloads — `docker/buildkit` is a real
    /// cgroup with real CPU time but no processes and no memory, and listing it beside actual
    /// containers is noise. Anything holding neither a process nor a byte is not a workload.
    pub fn is_running(&self) -> bool {
        self.pids > 0 || self.memory_bytes > 0
    }

    /// Share of its memory limit in use, when it has one. Unlimited cgroups have no proportion to
    /// show, and inventing one against host RAM would be a different number wearing the same name.
    pub fn memory_fraction(&self) -> Option<f64> {
        let limit = self.memory_limit.filter(|l| *l > 0)?;
        Some(self.memory_bytes as f64 / limit as f64 * 100.0)
    }
}

/// Argv for the cgroup sweep. A constant; nothing here comes from a host or from the user.
///
/// One command for the whole hierarchy — the shell expands the globs and the loop emits a framed
/// blob, so a host running forty containers still costs one entry in the batch.
///
/// The trailing `exit 0` is the usual guard: the loop's status is that of its last iteration, and
/// a cgroup disappearing mid-sweep (a container exiting) would otherwise discard everything.
pub const CGROUP_ARGV: [&str; 3] = [
    "sh",
    "-c",
    "for d in /sys/fs/cgroup/system.slice/docker-*.scope /sys/fs/cgroup/docker/* \
     /sys/fs/cgroup/machine.slice/libpod-*.scope /sys/fs/cgroup/lxc/* \
     /sys/fs/cgroup/lxc.payload/* /sys/fs/cgroup/qemu.slice/*.scope; do \
     [ -f \"$d/cpu.stat\" ] || continue; \
     printf '#%s\\n' \"$d\"; \
     cat \"$d/cpu.stat\" 2>/dev/null; \
     printf 'memory.current %s\\n' \"$(cat \"$d/memory.current\" 2>/dev/null)\"; \
     printf 'memory.max %s\\n' \"$(cat \"$d/memory.max\" 2>/dev/null)\"; \
     printf 'pids.current %s\\n' \"$(cat \"$d/pids.current\" 2>/dev/null)\"; \
     done; exit 0",
];

/// Parse the framed sweep. Each cgroup starts with `#<path>`.
pub fn parse_cgroups(text: &str) -> Vec<CgroupStats> {
    let mut out: Vec<CgroupStats> = Vec::new();

    for line in text.lines() {
        if let Some(path) = line.strip_prefix('#') {
            out.push(CgroupStats {
                path: path.trim().to_string(),
                ..Default::default()
            });
            continue;
        }
        let Some(current) = out.last_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let value = value.trim();

        match key {
            "usage_usec" => current.usage_usec = value.parse().unwrap_or(0),
            "memory.current" => current.memory_bytes = value.parse().unwrap_or(0),
            // `max` means unlimited, and must not become a limit of zero.
            "memory.max" => current.memory_limit = value.parse().ok(),
            "pids.current" => current.pids = value.parse().unwrap_or(0),
            _ => {}
        }
    }

    out
}

pub struct CgroupSource {
    descriptor: SourceDescriptor,
}

impl Default for CgroupSource {
    fn default() -> Self {
        CgroupSource {
            descriptor: SourceDescriptor {
                id: SourceId::new("cgroup.containers"),
                display: "Containers & VMs".into(),
                description:
                    "Per-container and per-VM CPU, memory and process counts, straight from cgroup v2"
                        .into(),
                produces: vec![EntityKind::Container, EntityKind::VirtualMachine],
                // Gated on cgroup v2. Capability detection already probes for this file.
                requires: Requirements::path("/sys/fs/cgroup/cgroup.controllers"),
                default_enabled: true,
            },
        }
    }
}

impl CgroupSource {
    fn request() -> Request {
        Request::exec(CGROUP_ARGV)
    }
}

impl Source for CgroupSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    fn requests(&self, _ctx: &TargetCtx) -> Vec<Request> {
        vec![Self::request()]
    }

    fn parse(&self, ctx: &TargetCtx, responses: &Responses, out: &mut SampleSink) -> ParseResult {
        let Some(text) = responses.text(&Self::request()) else {
            return Ok(());
        };
        let id = &self.descriptor.id;

        for cgroup in parse_cgroups(text) {
            if !cgroup.is_running() {
                continue;
            }
            let (kind, name) = cgroup.identify();
            let entity = Entity::child(&ctx.host, kind, &name).with_label("cgroup", &cgroup.path);

            // `usage_usec` is a microsecond counter, so its rate is microseconds of CPU per second
            // of wall clock. One core fully occupied is 1,000,000 — hence 100/1e6, which puts this
            // on exactly the same footing as the host and per-process CPU figures.
            out.emit(
                SeriesDescriptor::counter(id, &entity.id, "cpu", "CPU", Unit::Percent)
                    .with_scale(100.0 / 1_000_000.0),
                cgroup.usage_usec,
            );

            let mut memory =
                SeriesDescriptor::gauge(id, &entity.id, "memory", "Memory", Unit::Bytes);
            if let Some(limit) = cgroup.memory_limit {
                memory = memory.with_max(limit as f64);
            }
            out.emit(memory, cgroup.memory_bytes);

            if let Some(fraction) = cgroup.memory_fraction() {
                out.emit(
                    SeriesDescriptor::gauge(
                        id,
                        &entity.id,
                        "memory_usage",
                        "Memory used",
                        Unit::Percent,
                    ),
                    fraction,
                );
            }

            out.emit(
                SeriesDescriptor::gauge(id, &entity.id, "pids", "Processes", Unit::Count),
                cgroup.pids,
            );

            out.entity(entity);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{corpus, sink_for};

    /// A Proxmox host: a Docker container, an LXC guest and a QEMU VM, as the kernel lays them out.
    const SWEEP: &str = "\
#/sys/fs/cgroup/system.slice/docker-3f2b1c9d8e7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d1e0f9a8b7c6d5e4f3a2b1c.scope
usage_usec 12500000
user_usec 9000000
system_usec 3500000
memory.current 268435456
memory.max 536870912
pids.current 14
#/sys/fs/cgroup/lxc/101
usage_usec 88000000
memory.current 1073741824
memory.max max
pids.current 63
#/sys/fs/cgroup/qemu.slice/102.scope
usage_usec 450000000
memory.current 4294967296
memory.max max
pids.current 7
";

    #[test]
    fn parses_a_framed_sweep() {
        let cgroups = parse_cgroups(SWEEP);
        assert_eq!(cgroups.len(), 3);
        assert_eq!(cgroups[0].usage_usec, 12_500_000);
        assert_eq!(cgroups[0].memory_bytes, 268_435_456);
        assert_eq!(cgroups[0].memory_limit, Some(536_870_912));
        assert_eq!(cgroups[0].pids, 14);
    }

    /// `memory.max` reads `max` when unlimited. Parsing that as a number would give a limit of
    /// zero and a memory usage of infinity.
    #[test]
    fn unlimited_memory_is_absent_rather_than_zero() {
        let cgroups = parse_cgroups(SWEEP);
        assert_eq!(cgroups[1].memory_limit, None);
        assert_eq!(cgroups[1].memory_fraction(), None);
        // The limited one still reports a proportion.
        assert_eq!(cgroups[0].memory_fraction(), Some(50.0));
    }

    #[test]
    fn names_each_runtime_the_way_its_own_tooling_does() {
        let cgroups = parse_cgroups(SWEEP);

        let (kind, name) = cgroups[0].identify();
        assert_eq!(kind, EntityKind::Container);
        assert_eq!(
            name, "3f2b1c9d8e7a",
            "docker ids show as the same 12 chars `docker ps` shows"
        );

        assert_eq!(cgroups[1].identify(), (EntityKind::Container, "101".into()));
        assert_eq!(
            cgroups[2].identify(),
            (EntityKind::VirtualMachine, "VM 102".into())
        );
    }

    #[test]
    fn recognises_the_other_layouts() {
        let cases = [
            (
                "/sys/fs/cgroup/docker/abcdef0123456789",
                EntityKind::Container,
                "abcdef012345",
            ),
            (
                "/sys/fs/cgroup/machine.slice/libpod-fedcba9876543210.scope",
                EntityKind::Container,
                "fedcba987654",
            ),
            (
                "/sys/fs/cgroup/lxc.payload/200",
                EntityKind::Container,
                "200",
            ),
        ];
        for (path, kind, name) in cases {
            let stats = CgroupStats {
                path: path.into(),
                ..Default::default()
            };
            assert_eq!(stats.identify(), (kind, name.to_string()), "{path}");
        }
    }

    #[test]
    fn ignores_junk_before_the_first_marker() {
        let cgroups = parse_cgroups(
            "usage_usec 999\nmemory.current 1\n#/sys/fs/cgroup/lxc/7\nusage_usec 5\n",
        );
        assert_eq!(cgroups.len(), 1);
        assert_eq!(cgroups[0].usage_usec, 5);
    }

    /// Found by running the sweep against a real Docker host: `docker/buildkit` is a cgroup with
    /// CPU time but no processes and no memory. It is scaffolding, not a workload.
    #[test]
    fn empty_scaffolding_cgroups_are_not_listed_as_containers() {
        let text = format!(
            "{SWEEP}#/sys/fs/cgroup/docker/buildkit\nusage_usec 3979168\nmemory.current 0\n\
             memory.max max\npids.current 0\n"
        );
        let (ctx, responses) = corpus("debian").exec_literal(&CGROUP_ARGV, &text).build();
        let out = sink_for(&CgroupSource::default(), &ctx, &responses);

        let names: Vec<_> = out.entities.iter().map(|e| e.display.as_str()).collect();
        assert!(
            !names.contains(&"buildkit"),
            "empty cgroup listed as a container: {names:?}"
        );
        assert_eq!(out.entities.len(), 3);
    }

    #[test]
    fn a_host_running_nothing_produces_nothing() {
        let (ctx, responses) = corpus("debian").exec_literal(&CGROUP_ARGV, "").build();
        let out = sink_for(&CgroupSource::default(), &ctx, &responses);
        assert!(out.is_empty());
    }

    /// The same scale as host and per-process CPU: one core fully occupied reads 100%.
    #[test]
    fn cpu_scale_matches_the_rest_of_the_app() {
        let (ctx, responses) = corpus("debian").exec_literal(&CGROUP_ARGV, SWEEP).build();
        let out = sink_for(&CgroupSource::default(), &ctx, &responses);

        let cpu = out.descriptors.iter().find(|d| d.metric == "cpu").unwrap();
        // A cgroup burning one core accrues 1,000,000 microseconds per second.
        assert!((1_000_000.0 * cpu.scale - 100.0).abs() < 1e-9);
        assert_eq!(cpu.effective_unit(), Unit::Percent);
    }

    #[test]
    fn emits_an_entity_per_guest_with_its_cgroup_recorded() {
        let (ctx, responses) = corpus("debian").exec_literal(&CGROUP_ARGV, SWEEP).build();
        let out = sink_for(&CgroupSource::default(), &ctx, &responses);

        assert_eq!(out.entities.len(), 3);
        let vm = out.entities.iter().find(|e| e.display == "VM 102").unwrap();
        assert_eq!(vm.kind, EntityKind::VirtualMachine);
        assert!(vm
            .labels
            .get("cgroup")
            .is_some_and(|c| c.contains("qemu.slice")));
    }

    #[test]
    fn the_sweep_normalises_its_exit_status() {
        let script = CGROUP_ARGV[2];
        assert!(script.trim_end().ends_with("exit 0"), "{script}");
    }

    #[test]
    fn is_gated_on_cgroup_v2() {
        let source = CgroupSource::default();
        let mut caps = sg_model::Capabilities::default();
        assert!(!source.descriptor().requires.satisfied_by(&caps));
        caps.paths
            .insert("/sys/fs/cgroup/cgroup.controllers".into());
        assert!(source.descriptor().requires.satisfied_by(&caps));
    }
}
