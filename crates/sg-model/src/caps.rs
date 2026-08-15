//! What a given host can actually tell us.
//!
//! Detected once per connection and cached. Sources declare requirements; anything unsatisfied is
//! never scheduled and never appears in the UI, so a BusyBox container and a Proxmox node each
//! show exactly the metrics they can support.

use std::collections::BTreeSet;

/// Which coreutils flavour the host ships. Output formats differ enough (`df`, `ps`, `ss`) that
/// several parsers branch on this.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coreutils {
    Gnu,
    Busybox,
    /// BSD-ish or something else entirely; parsers should prefer `/proc` over command output.
    #[default]
    Unknown,
}

/// cgroup hierarchy in use. Determines where container resource accounting lives.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CgroupVersion {
    V1,
    V2,
    #[default]
    Unknown,
}

/// The capability profile of one connected host.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Capabilities {
    /// `uname -r`.
    pub kernel: String,
    /// Pretty name from `/etc/os-release`, e.g. `Debian GNU/Linux 12 (bookworm)`.
    pub distro: String,
    /// `uname -m`.
    pub arch: String,
    pub hostname: String,
    pub coreutils: Coreutils,
    pub cgroup: CgroupVersion,
    /// Logical CPUs, from `/proc/stat`.
    pub cpu_count: u32,
    /// `getconf CLK_TCK`, almost always 100.
    ///
    /// Required to turn `/proc/stat` and `/proc/<pid>/stat` jiffies into seconds. Assuming 100
    /// is right nearly everywhere and silently wrong on the hosts where it isn't, so it is
    /// measured rather than assumed.
    pub clock_ticks: u64,
    /// `getconf PAGESIZE`, for `/proc/<pid>/statm` accounting.
    pub page_size: u64,
    /// Programs found on `PATH`: `docker`, `podman`, `kubectl`, `nvidia-smi`, `smartctl`,
    /// `zpool`, `systemctl`, `ss`, `ip`.
    pub binaries: BTreeSet<String>,
    /// Readable paths detected at connect time, e.g. `/proc/pressure/cpu`, `/sys/class/hwmon`.
    pub paths: BTreeSet<String>,
}

impl Capabilities {
    pub fn has(&self, binary: &str) -> bool {
        self.binaries.contains(binary)
    }

    pub fn has_path(&self, path: &str) -> bool {
        self.paths.contains(path)
    }

    /// Seconds per clock tick. Guards against a bogus or unread `CLK_TCK`.
    pub fn tick_seconds(&self) -> f64 {
        if self.clock_ticks == 0 {
            0.01
        } else {
            1.0 / self.clock_ticks as f64
        }
    }
}

/// What a source needs before it may be scheduled.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Requirements {
    /// All of these programs must be present.
    #[serde(default)]
    pub binaries: Vec<String>,
    /// All of these paths must be readable.
    #[serde(default)]
    pub paths: Vec<String>,
}

impl Requirements {
    pub const NONE: Requirements = Requirements {
        binaries: Vec::new(),
        paths: Vec::new(),
    };

    pub fn binary(name: impl Into<String>) -> Self {
        Requirements {
            binaries: vec![name.into()],
            paths: Vec::new(),
        }
    }

    pub fn path(path: impl Into<String>) -> Self {
        Requirements {
            binaries: Vec::new(),
            paths: vec![path.into()],
        }
    }

    pub fn satisfied_by(&self, caps: &Capabilities) -> bool {
        self.binaries.iter().all(|b| caps.has(b)) && self.paths.iter().all(|p| caps.has_path(p))
    }
}
