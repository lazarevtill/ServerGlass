//! Filesystem capacity via `df -P -k`.
//!
//! `-P` selects the POSIX output format, which pins the column layout and stops `df` from wrapping
//! long device names onto a second line; `-k` pins the block size to 1024 bytes. Both GNU coreutils
//! and BusyBox honour them identically, which is why this one command works on every host instead
//! of needing a per-distribution branch.

use sg_model::{
    Entity, EntityKind, ParseResult, Request, Requirements, Responses, SampleSink,
    SeriesDescriptor, Source, SourceDescriptor, SourceId, TargetCtx, Unit,
};

/// One row of `df -P -k`, in bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filesystem {
    pub device: String,
    pub mount: String,
    pub total: u64,
    pub used: u64,
    pub available: u64,
}

impl Filesystem {
    pub fn usage_percent(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.used as f64 / self.total as f64 * 100.0
        }
    }

    /// Kernel bookkeeping mounts, not storage anyone provisions or runs out of.
    pub fn is_pseudo(&self) -> bool {
        const PSEUDO_MOUNTS: [&str; 4] = ["/proc", "/sys", "/dev", "/run"];
        const PSEUDO_DEVICES: [&str; 5] = ["devtmpfs", "udev", "none", "cgroup", "cgroup2"];

        // FUSE control filesystems report a capacity, so they survive the zero-size filter and
        // show up as a real mount. Proxmox's `/etc/pve` is the one everyone meets: a 72 KiB
        // cluster config filesystem rendered alongside a 1.3 TiB pool.
        let is_fuse = self.device.starts_with("/dev/fuse") || self.device == "fusectl";

        is_fuse
            || PSEUDO_DEVICES.contains(&self.device.as_str())
            || PSEUDO_MOUNTS
                .iter()
                .any(|p| self.mount == *p || self.mount.starts_with(&format!("{p}/")))
    }
}

/// Parse `df -P -k` output.
pub fn parse_df(text: &str) -> Vec<Filesystem> {
    let mut out = Vec::new();

    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 6 {
            continue;
        }
        let kib = |i: usize| {
            fields[i]
                .parse::<u64>()
                .ok()
                .map(|v| v.saturating_mul(1024))
        };
        let (Some(total), Some(used), Some(available)) = (kib(1), kib(2), kib(3)) else {
            continue;
        };

        out.push(Filesystem {
            device: fields[0].to_string(),
            // POSIX puts the mount point last and permits spaces in it, so everything from the
            // sixth field onward is one path, not several.
            mount: fields[5..].join(" "),
            total,
            used,
            available,
        });
    }

    out
}

pub struct FilesystemSource {
    descriptor: SourceDescriptor,
}

impl Default for FilesystemSource {
    fn default() -> Self {
        FilesystemSource {
            descriptor: SourceDescriptor {
                id: SourceId::new("proc.filesystem"),
                display: "Filesystems".into(),
                description: "Mounted filesystem capacity and usage".into(),
                produces: vec![EntityKind::Filesystem],
                requires: Requirements::NONE,
                default_enabled: true,
            },
        }
    }
}

impl FilesystemSource {
    fn request() -> Request {
        Request::exec(["df", "-P", "-k"])
    }
}

impl Source for FilesystemSource {
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

        let mut root_usage = None;

        for fs in parse_df(text) {
            if fs.is_pseudo() || fs.total == 0 {
                continue;
            }

            if fs.mount == "/" {
                root_usage = Some(fs.usage_percent());
            }

            let entity = Entity::child(&ctx.host, EntityKind::Filesystem, &fs.mount)
                .with_label("device", &fs.device);

            out.emit(
                SeriesDescriptor::gauge(id, &entity.id, "usage", "Usage", Unit::Percent),
                fs.usage_percent(),
            );
            out.emit(
                SeriesDescriptor::gauge(id, &entity.id, "used", "Used", Unit::Bytes)
                    .with_max(fs.total as f64),
                fs.used,
            );
            out.emit(
                SeriesDescriptor::gauge(id, &entity.id, "total", "Total", Unit::Bytes),
                fs.total,
            );
            out.emit(
                SeriesDescriptor::gauge(id, &entity.id, "available", "Available", Unit::Bytes),
                fs.available,
            );

            out.entity(entity);
        }

        // The status grid needs one disk figure per host, and the root filesystem is the one that
        // takes the host down when it fills.
        if let Some(usage) = root_usage {
            out.emit(
                SeriesDescriptor::gauge(id, &ctx.host.id, "disk_usage", "Disk", Unit::Percent),
                usage,
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{corpus, sink_for, value_of, HOSTS};

    const SAMPLE: &str = "\
Filesystem     1024-blocks     Used Available Capacity Mounted on
overlay          311492608 13976112 297516496       5% /
tmpfs                65536        0     65536       0% /dev
/dev/nvme0n1p2   103080224 41930312  55876916      43% /home
none                     0        0         0       0% /proc/sys
/dev/sdb1          1024000   512000    512000      50% /mnt/my backup
";

    #[test]
    fn parses_the_posix_column_layout() {
        let mounts = parse_df(SAMPLE);
        let home = mounts.iter().find(|f| f.mount == "/home").unwrap();

        assert_eq!(home.device, "/dev/nvme0n1p2");
        assert_eq!(home.total, 103_080_224 * 1024);
        assert_eq!(home.used, 41_930_312 * 1024);
        assert_eq!(home.available, 55_876_916 * 1024);
        assert!((home.usage_percent() - 40.68).abs() < 0.1);
    }

    /// POSIX allows spaces in the mount point, and it is the last field precisely so they can be
    /// rejoined unambiguously.
    #[test]
    fn keeps_mount_points_containing_spaces_intact() {
        let mounts = parse_df(SAMPLE);
        assert!(
            mounts.iter().any(|f| f.mount == "/mnt/my backup"),
            "mount points were truncated at the first space"
        );
    }

    #[test]
    fn filters_pseudo_filesystems() {
        let (ctx, responses) = corpus("debian").exec(&["df", "-P", "-k"], "df-P-k").build();
        let out = sink_for(&FilesystemSource::default(), &ctx, &responses);

        for entity in &out.entities {
            assert!(
                !entity.display.starts_with("/proc") && !entity.display.starts_with("/sys"),
                "pseudo filesystem {} was reported as storage",
                entity.display
            );
        }
    }

    #[test]
    fn ignores_zero_sized_and_malformed_rows() {
        let (ctx, responses) = corpus("debian").exec(&["df", "-P", "-k"], "df-P-k").build();
        let _ = sink_for(&FilesystemSource::default(), &ctx, &responses);

        assert!(parse_df(
            "Filesystem 1024-blocks Used Available Capacity Mounted on\nbroken row\n"
        )
        .is_empty());
        assert!(parse_df("").is_empty());
    }

    #[test]
    fn exposes_a_single_host_level_disk_figure_from_the_root_mount() {
        let (ctx, responses) = corpus("debian").build();
        let mut responses = responses;
        responses.insert(
            Request::exec(["df", "-P", "-k"]).id(),
            sg_model::Response::ok(SAMPLE),
        );
        let out = sink_for(&FilesystemSource::default(), &ctx, &responses);

        let usage = value_of(&out, "disk_usage").expect("host-level disk usage");
        assert!((usage - 13_976_112.0 / 311_492_608.0 * 100.0).abs() < 1e-6);
    }

    #[test]
    fn works_on_both_gnu_and_busybox_df() {
        for host in HOSTS {
            let (ctx, responses) = corpus(host).exec(&["df", "-P", "-k"], "df-P-k").build();
            let out = sink_for(&FilesystemSource::default(), &ctx, &responses);

            assert!(
                out.entities.iter().any(|e| e.display == "/"),
                "{host}: root filesystem missing from {:?}",
                out.entities.iter().map(|e| &e.display).collect::<Vec<_>>()
            );
            assert!(
                value_of(&out, "disk_usage").is_some(),
                "{host}: no host disk usage"
            );
        }
    }
}
