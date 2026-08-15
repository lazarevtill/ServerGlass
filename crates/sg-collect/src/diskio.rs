//! Block device throughput from `/proc/diskstats`.

use sg_model::{
    Entity, EntityKind, ParseResult, Request, Requirements, Responses, SampleSink,
    SeriesDescriptor, Source, SourceDescriptor, SourceId, TargetCtx, Unit,
};

/// `/proc/diskstats` reports in 512-byte sectors regardless of the device's real sector size.
/// This is a property of the kernel interface, not of the hardware, so it is a constant.
const SECTOR_BYTES: u64 = 512;

/// Cumulative counters for one block device.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiskStats {
    pub name: String,
    pub reads: u64,
    pub read_sectors: u64,
    pub read_ms: u64,
    pub writes: u64,
    pub write_sectors: u64,
    pub write_ms: u64,
    pub io_in_progress: u64,
    pub io_ms: u64,
}

impl DiskStats {
    pub fn read_bytes(&self) -> u64 {
        self.read_sectors.saturating_mul(SECTOR_BYTES)
    }

    pub fn write_bytes(&self) -> u64 {
        self.write_sectors.saturating_mul(SECTOR_BYTES)
    }

    /// Virtual devices that exist on nearly every host and interest nobody: snap mounts, ramdisks,
    /// and the floppy driver that still enumerates on some kernels.
    pub fn is_virtual(&self) -> bool {
        ["loop", "ram", "fd", "sr", "zram"]
            .iter()
            .any(|p| self.name.starts_with(p))
    }

    pub fn is_idle(&self) -> bool {
        self.reads == 0 && self.writes == 0
    }
}

/// Parse `/proc/diskstats`.
///
/// Modern kernels append discard and flush fields; only the first eleven after the device name are
/// read, so both old and new layouts parse identically.
pub fn parse_diskstats(text: &str) -> Vec<DiskStats> {
    let mut out = Vec::new();

    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // major, minor, name, then at least the eleven classic counters.
        if fields.len() < 14 {
            continue;
        }
        let n = |i: usize| {
            fields
                .get(i)
                .and_then(|f| f.parse::<u64>().ok())
                .unwrap_or(0)
        };

        out.push(DiskStats {
            name: fields[2].to_string(),
            reads: n(3),
            read_sectors: n(5),
            read_ms: n(6),
            writes: n(7),
            write_sectors: n(9),
            write_ms: n(10),
            io_in_progress: n(11),
            io_ms: n(12),
        });
    }

    out
}

/// Whether `name` is a partition of some other device in `all`.
///
/// Used to group `nvme0n1p1` under `nvme0n1` in the UI. Checking against the actual device list
/// beats a name-shape heuristic, which cannot tell the partition `sda1` from the whole device
/// `md0` or `dm-1`.
fn parent_device<'a>(name: &str, all: &'a [DiskStats]) -> Option<&'a str> {
    all.iter()
        .map(|d| d.name.as_str())
        .filter(|candidate| *candidate != name && name.starts_with(*candidate))
        .filter(|candidate| {
            // `sda` -> `sda1`, and `nvme0n1` -> `nvme0n1p1`.
            let suffix = &name[candidate.len()..];
            let digits = suffix.strip_prefix('p').unwrap_or(suffix);
            !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
        })
        // Longest match wins, so `nvme0n1p1` attaches to `nvme0n1` rather than to `nvme0n`.
        .max_by_key(|candidate| candidate.len())
}

pub struct DiskIoSource {
    descriptor: SourceDescriptor,
}

impl Default for DiskIoSource {
    fn default() -> Self {
        DiskIoSource {
            descriptor: SourceDescriptor {
                id: SourceId::new("proc.diskio"),
                display: "Disk I/O".into(),
                description: "Per-device read/write throughput and operation rates".into(),
                produces: vec![EntityKind::Disk],
                requires: Requirements::path("/proc/diskstats"),
                default_enabled: true,
            },
        }
    }
}

impl DiskIoSource {
    fn request() -> Request {
        Request::read("/proc/diskstats")
    }
}

impl Source for DiskIoSource {
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
        let all = parse_diskstats(text);
        let id = &self.descriptor.id;

        let mut host_read = 0u64;
        let mut host_write = 0u64;

        for disk in &all {
            if disk.is_virtual() || disk.is_idle() {
                continue;
            }

            let is_partition = parent_device(&disk.name, &all).is_some();
            // Only whole devices contribute to the host total; adding partitions as well would
            // double-count every byte.
            if !is_partition {
                host_read += disk.read_bytes();
                host_write += disk.write_bytes();
            }

            let mut entity = Entity::child(&ctx.host, EntityKind::Disk, &disk.name);
            if let Some(parent) = parent_device(&disk.name, &all) {
                entity = entity.with_label("partition_of", parent);
            }

            for (metric, display, value, unit) in [
                ("read_bytes", "Read", disk.read_bytes(), Unit::Bytes),
                ("write_bytes", "Written", disk.write_bytes(), Unit::Bytes),
                ("reads", "Read ops", disk.reads, Unit::Operations),
                ("writes", "Write ops", disk.writes, Unit::Operations),
            ] {
                out.emit(
                    SeriesDescriptor::counter(id, &entity.id, metric, display, unit),
                    value,
                );
            }
            out.emit(
                SeriesDescriptor::gauge(id, &entity.id, "in_flight", "In flight", Unit::Count),
                disk.io_in_progress,
            );

            out.entity(entity);
        }

        let host = &ctx.host.id;
        out.emit(
            SeriesDescriptor::counter(id, host, "disk_read", "Disk read", Unit::Bytes),
            host_read,
        );
        out.emit(
            SeriesDescriptor::counter(id, host, "disk_write", "Disk write", Unit::Bytes),
            host_write,
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{corpus, sink_for, value_of, HOSTS};

    // major minor name reads rd_merged rd_sectors rd_ms writes wr_merged wr_sectors wr_ms inflight io_ms weighted
    const SAMPLE: &str = "\
   7       0 loop0 0 0 0 0 0 0 0 0 0 0 0
 259       0 nvme0n1 100 0 2000 50 200 0 4000 90 1 140 0
 259       1 nvme0n1p1 40 0 800 20 60 0 1200 30 0 50 0
   8       0 sda 5 0 10 1 0 0 0 0 0 1 0
   8      16 sdb 0 0 0 0 0 0 0 0 0 0 0
";

    #[test]
    fn parses_the_classic_field_layout() {
        let disks = parse_diskstats(SAMPLE);
        let nvme = disks.iter().find(|d| d.name == "nvme0n1").unwrap();

        assert_eq!(nvme.reads, 100);
        assert_eq!(nvme.read_sectors, 2000);
        assert_eq!(nvme.writes, 200);
        assert_eq!(nvme.write_sectors, 4000);
        assert_eq!(nvme.io_in_progress, 1);
    }

    /// diskstats always reports 512-byte sectors, whatever the device's physical sector size.
    #[test]
    fn converts_sectors_to_bytes_at_512() {
        let disks = parse_diskstats(SAMPLE);
        let nvme = disks.iter().find(|d| d.name == "nvme0n1").unwrap();
        assert_eq!(nvme.read_bytes(), 2000 * 512);
        assert_eq!(nvme.write_bytes(), 4000 * 512);
    }

    #[test]
    fn tolerates_the_extra_discard_and_flush_fields() {
        let modern = " 259 0 nvme0n1 100 0 2000 50 200 0 4000 90 1 140 0 7 0 8 9 1 2\n";
        let disks = parse_diskstats(modern);
        assert_eq!(disks.len(), 1);
        assert_eq!(disks[0].read_sectors, 2000);
        assert_eq!(disks[0].write_sectors, 4000);
    }

    #[test]
    fn ignores_truncated_lines() {
        assert!(parse_diskstats("8 0 sda 1 2 3\n").is_empty());
    }

    #[test]
    fn recognises_partitions_by_matching_real_devices() {
        let disks = parse_diskstats(SAMPLE);
        assert_eq!(parent_device("nvme0n1p1", &disks), Some("nvme0n1"));
        assert_eq!(parent_device("nvme0n1", &disks), None);
        assert_eq!(parent_device("sda", &disks), None);
        // `sdb` is not a partition of `sda` despite the shared prefix length.
        assert_eq!(parent_device("sdb", &disks), None);
    }

    #[test]
    fn skips_virtual_and_never_used_devices() {
        let (ctx, responses) = corpus("debian").literal("/proc/diskstats", SAMPLE).build();
        let out = sink_for(&DiskIoSource::default(), &ctx, &responses);

        let names: Vec<_> = out.entities.iter().map(|e| e.display.as_str()).collect();
        assert!(
            !names.contains(&"loop0"),
            "loop devices are noise on every host"
        );
        assert!(
            !names.contains(&"sdb"),
            "a device with no I/O has nothing to show"
        );
        assert!(names.contains(&"nvme0n1"));
        assert!(names.contains(&"sda"));
    }

    /// Partitions report the same bytes as their parent device. Summing both inflates every
    /// host-level figure.
    #[test]
    fn host_totals_do_not_double_count_partitions() {
        let (ctx, responses) = corpus("debian").literal("/proc/diskstats", SAMPLE).build();
        let out = sink_for(&DiskIoSource::default(), &ctx, &responses);

        // nvme0n1 (2000 sectors) + sda (10 sectors), excluding nvme0n1p1's 800.
        assert_eq!(
            value_of(&out, "disk_read"),
            Some((2000 + 10) as f64 * 512.0)
        );
    }

    #[test]
    fn labels_partitions_with_their_parent() {
        let (ctx, responses) = corpus("debian").literal("/proc/diskstats", SAMPLE).build();
        let out = sink_for(&DiskIoSource::default(), &ctx, &responses);

        let part = out
            .entities
            .iter()
            .find(|e| e.display == "nvme0n1p1")
            .unwrap();
        assert_eq!(
            part.labels.get("partition_of").map(String::as_str),
            Some("nvme0n1")
        );
    }

    #[test]
    fn parses_both_corpora_without_panicking() {
        for host in HOSTS {
            let (ctx, responses) = corpus(host).file("/proc/diskstats").build();
            let out = sink_for(&DiskIoSource::default(), &ctx, &responses);
            assert!(
                value_of(&out, "disk_read").is_some(),
                "{host}: no host disk totals"
            );
        }
    }
}
