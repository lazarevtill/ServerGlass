//! Capability detection.
//!
//! Run once per connection, as an ordinary batch through the same one-round-trip machinery the
//! collectors use. The result decides which sources are ever scheduled, so a BusyBox container and
//! a Proxmox node each end up showing exactly what they can actually support instead of a grid of
//! empty gauges.

use sg_model::{Capabilities, CgroupVersion, Coreutils, Request, Responses};

/// Programs worth knowing about. Presence of each gates one or more sources.
pub const PROBED_BINARIES: &[&str] = &[
    "busybox", "docker", "podman", "kubectl", "nvidia-smi", "rocm-smi", "smartctl", "zpool",
    "mdadm", "systemctl", "journalctl", "ss", "ip", "lsblk", "virsh", "qm", "upsc", "sensors",
    "fail2ban-client", "nft", "iptables",
];

/// Paths whose readability gates one or more sources.
pub const PROBED_PATHS: &[&str] = &[
    "/proc/stat",
    "/proc/meminfo",
    "/proc/loadavg",
    "/proc/uptime",
    "/proc/net/dev",
    "/proc/net/snmp",
    "/proc/net/sockstat",
    "/proc/diskstats",
    "/proc/mounts",
    "/proc/pressure/cpu",
    "/sys/class/hwmon",
    "/sys/class/thermal",
    "/sys/block",
    "/sys/fs/cgroup/cgroup.controllers",
    "/sys/fs/cgroup/cpu",
];

fn kernel_request() -> Request {
    Request::exec(["uname", "-r"])
}

fn arch_request() -> Request {
    Request::exec(["uname", "-m"])
}

fn hostname_request() -> Request {
    Request::exec(["uname", "-n"])
}

fn os_release_request() -> Request {
    Request::read("/etc/os-release")
}

fn clock_ticks_request() -> Request {
    Request::exec(["getconf", "CLK_TCK"])
}

fn page_size_request() -> Request {
    Request::exec(["getconf", "PAGESIZE"])
}

fn stat_request() -> Request {
    Request::read("/proc/stat")
}

fn coreutils_request() -> Request {
    Request::exec(["ls", "--version"])
}

/// One request that reports every present binary, rather than one request per binary.
///
/// The shell fragment is assembled from [`PROBED_BINARIES`], which is a compile-time constant —
/// no value reaching this string ever originates from a host, a config file or the user. That is
/// asserted by a test below, because the day someone makes this list dynamic is the day it becomes
/// a command injection.
///
/// The trailing `exit 0` is load-bearing. A shell loop exits with the status of its *last*
/// iteration, so probing a host that lacks the last-listed binary would report failure for the
/// whole probe and [`Responses::text`] would discard a body that was entirely correct. For a probe
/// whose output is the whole point, the final iteration's status is noise.
fn binaries_request() -> Request {
    let list = PROBED_BINARIES.join(" ");
    Request::exec([
        "sh",
        "-c",
        &format!(
            "for b in {list}; do command -v \"$b\" >/dev/null 2>&1 && echo \"$b\"; done; exit 0"
        ),
    ])
}

/// The same trick for readable paths, with the same status normalisation.
fn paths_request() -> Request {
    let list = PROBED_PATHS.join(" ");
    Request::exec([
        "sh",
        "-c",
        &format!("for p in {list}; do [ -r \"$p\" ] && echo \"$p\"; done; exit 0"),
    ])
}

/// Every request capability detection needs. Issued as one batch, so detection costs one round
/// trip just like a refresh does.
pub fn requests() -> Vec<Request> {
    vec![
        kernel_request(),
        arch_request(),
        hostname_request(),
        os_release_request(),
        clock_ticks_request(),
        page_size_request(),
        stat_request(),
        coreutils_request(),
        binaries_request(),
        paths_request(),
    ]
}

/// Interpret the batch.
///
/// Everything is optional. A host that answers nothing yields a default [`Capabilities`], which
/// gates every source off rather than producing a screen of zeroes.
pub fn parse(responses: &Responses) -> Capabilities {
    let line = |req: Request| {
        responses.text(&req).map(|t| t.trim().to_string()).filter(|s| !s.is_empty())
    };

    let mut caps = Capabilities {
        kernel: line(kernel_request()).unwrap_or_default(),
        arch: line(arch_request()).unwrap_or_default(),
        hostname: line(hostname_request()).unwrap_or_default(),
        distro: responses
            .text(&os_release_request())
            .and_then(parse_pretty_name)
            .unwrap_or_default(),
        clock_ticks: line(clock_ticks_request())
            .and_then(|s| s.parse().ok())
            // 100 is right on essentially every Linux build; falling back to it beats emitting
            // zeroed CPU percentages on a host whose getconf is missing.
            .unwrap_or(100),
        page_size: line(page_size_request()).and_then(|s| s.parse().ok()).unwrap_or(4096),
        cpu_count: responses.text(&stat_request()).map(count_cpus).unwrap_or(0),
        binaries: responses
            .text(&binaries_request())
            .map(|t| t.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
            .unwrap_or_default(),
        paths: responses
            .text(&paths_request())
            .map(|t| t.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
            .unwrap_or_default(),
        coreutils: Coreutils::Unknown,
        cgroup: CgroupVersion::Unknown,
    };

    caps.coreutils = match responses.text(&coreutils_request()) {
        Some(text) if text.contains("GNU coreutils") => Coreutils::Gnu,
        // BusyBox's `ls` rejects `--version`, so absence of a GNU banner plus a busybox binary is
        // the reliable signal.
        _ if caps.has("busybox") => Coreutils::Busybox,
        _ => Coreutils::Unknown,
    };

    caps.cgroup = if caps.has_path("/sys/fs/cgroup/cgroup.controllers") {
        CgroupVersion::V2
    } else if caps.has_path("/sys/fs/cgroup/cpu") {
        CgroupVersion::V1
    } else {
        CgroupVersion::Unknown
    };

    caps
}

/// `PRETTY_NAME="Debian GNU/Linux 12 (bookworm)"` -> `Debian GNU/Linux 12 (bookworm)`.
fn parse_pretty_name(os_release: &str) -> Option<String> {
    os_release.lines().find_map(|line| {
        let value = line.strip_prefix("PRETTY_NAME=")?;
        Some(value.trim().trim_matches('"').to_string())
    })
}

/// Count `cpuN` lines in `/proc/stat`, excluding the leading aggregate `cpu ` line.
fn count_cpus(stat: &str) -> u32 {
    stat.lines()
        .filter(|l| {
            l.starts_with("cpu") && l.as_bytes().get(3).is_some_and(u8::is_ascii_digit)
        })
        .count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use sg_model::Response;

    /// The `sh -c` fragments are only safe because their inputs are constants. If someone makes
    /// these lists configurable, this test is the tripwire.
    #[test]
    fn probe_lists_contain_no_shell_metacharacters() {
        for item in PROBED_BINARIES.iter().chain(PROBED_PATHS.iter()) {
            assert!(
                item.chars().all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c)),
                "{item:?} would need quoting inside the sh -c probe"
            );
        }
    }

    /// Regression: a shell loop exits with its last iteration's status, so probing a host that
    /// lacks the last-listed binary reported 127 for the whole probe and the body was discarded.
    #[test]
    fn list_probes_normalise_their_exit_status() {
        for request in [binaries_request(), paths_request()] {
            let Request::Exec { argv } = &request else { panic!("expected an exec request") };
            let script = argv.last().expect("script argument");
            assert!(
                script.trim_end().ends_with("exit 0"),
                "probe would report the last iteration's status: {script}"
            );
        }
    }

    #[test]
    fn detection_is_a_single_round_trip() {
        let reqs = requests();
        // Ten frames, but one batch — the whole point of the design.
        assert_eq!(reqs.len(), 10);
        assert!(reqs.iter().all(|r| r.is_remote()));
    }

    fn responses_from(pairs: Vec<(Request, &str)>) -> Responses {
        let mut r = Responses::default();
        for (req, body) in pairs {
            r.insert(req.id(), Response::ok(body));
        }
        r
    }

    #[test]
    fn parses_a_debian_host() {
        let caps = parse(&responses_from(vec![
            (kernel_request(), "6.1.0-18-arm64\n"),
            (arch_request(), "aarch64\n"),
            (hostname_request(), "web-01\n"),
            (
                os_release_request(),
                "PRETTY_NAME=\"Debian GNU/Linux 12 (bookworm)\"\nID=debian\n",
            ),
            (clock_ticks_request(), "100\n"),
            (page_size_request(), "4096\n"),
            (stat_request(), "cpu  1 2 3\ncpu0 1 2 3\ncpu1 1 2 3\nintr 5\n"),
            (coreutils_request(), "ls (GNU coreutils) 9.1\n"),
            (binaries_request(), "docker\nsystemctl\nss\nip\n"),
            (paths_request(), "/proc/stat\n/sys/fs/cgroup/cgroup.controllers\n"),
        ]));

        assert_eq!(caps.kernel, "6.1.0-18-arm64");
        assert_eq!(caps.arch, "aarch64");
        assert_eq!(caps.hostname, "web-01");
        assert_eq!(caps.distro, "Debian GNU/Linux 12 (bookworm)");
        assert_eq!(caps.clock_ticks, 100);
        assert_eq!(caps.page_size, 4096);
        assert_eq!(caps.cpu_count, 2, "aggregate 'cpu' line must not be counted as a core");
        assert_eq!(caps.coreutils, Coreutils::Gnu);
        assert_eq!(caps.cgroup, CgroupVersion::V2);
        assert!(caps.has("docker") && caps.has("ss"));
        assert!(!caps.has("kubectl"));
        assert!(caps.has_path("/proc/stat"));
    }

    #[test]
    fn parses_a_busybox_host_whose_ls_rejects_version() {
        let mut responses = responses_from(vec![
            (binaries_request(), "busybox\n"),
            (paths_request(), "/proc/stat\n/sys/fs/cgroup/cpu\n"),
            (stat_request(), "cpu  1 2 3\ncpu0 1 2 3\n"),
        ]);
        responses.insert(coreutils_request().id(), Response::failed(1));

        let caps = parse(&responses);
        assert_eq!(caps.coreutils, Coreutils::Busybox);
        assert_eq!(caps.cgroup, CgroupVersion::V1);
        assert_eq!(caps.cpu_count, 1);
    }

    #[test]
    fn a_host_that_answers_nothing_yields_safe_defaults() {
        let caps = parse(&Responses::default());
        assert_eq!(caps.cpu_count, 0);
        assert_eq!(caps.coreutils, Coreutils::Unknown);
        assert_eq!(caps.cgroup, CgroupVersion::Unknown);
        // Falls back rather than dividing by zero later.
        assert_eq!(caps.clock_ticks, 100);
        assert!((caps.tick_seconds() - 0.01).abs() < f64::EPSILON);
        assert!(caps.binaries.is_empty());
    }

    #[test]
    fn os_release_without_pretty_name_is_tolerated() {
        assert_eq!(parse_pretty_name("ID=alpine\nVERSION_ID=3.19\n"), None);
        assert_eq!(parse_pretty_name("PRETTY_NAME=\"Alpine Linux v3.19\"\n").as_deref(), Some("Alpine Linux v3.19"));
        // Unquoted form is legal in os-release.
        assert_eq!(parse_pretty_name("PRETTY_NAME=Gentoo\n").as_deref(), Some("Gentoo"));
    }

    #[test]
    fn counts_double_digit_cpu_ids() {
        let stat = (0..12).map(|i| format!("cpu{i} 1 2 3")).collect::<Vec<_>>().join("\n");
        assert_eq!(count_cpus(&format!("cpu  0 0 0\n{stat}\n")), 12);
    }
}
