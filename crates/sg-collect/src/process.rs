//! The process table, from `/proc/<pid>/stat`.
//!
//! This is the collector that answers the question a dashboard exists to raise. "CPU is at 79 %"
//! is not actionable; "79 %, and `kvm` is 61 % of it" is.
//!
//! Per-process CPU is a rate, not a reading — it is the derivative of the process's cumulative
//! jiffy counter. That falls straight out of the existing machinery: each process becomes an
//! entity, its `utime + stime` is emitted as a counter with the same `100 / clock_ticks` scale the
//! CPU collector uses, and [`crate::super::rate`] differentiates it. The collector stays a pure
//! function and no process bookkeeping is needed anywhere.

use sg_model::{
    Entity, EntityKind, ParseResult, Request, Requirements, Responses, SampleSink,
    SeriesDescriptor, Source, SourceDescriptor, SourceId, TargetCtx, Unit,
};

/// Ceiling on how many processes are turned into entities per tick.
///
/// A busy host runs thousands. Each becomes an entity with two series held in the live store, and
/// the whole set is rebuilt every tick, so this is the difference between a bounded few megabytes
/// and unbounded growth. Selection is by resident memory, which — unlike CPU — is readable from a
/// single sample, so the cut can be made on the very first tick.
const MAX_PROCESSES: usize = 256;

/// One row of the process table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessStat {
    pub pid: u32,
    /// Executable name, without the kernel's surrounding parentheses.
    pub comm: String,
    /// `R`, `S`, `D`, `Z`, …
    pub state: char,
    /// Cumulative user + system jiffies. A counter; the scheduler turns it into a percentage.
    pub cpu_ticks: u64,
    /// Resident set, in pages. Multiplied by the host's page size at emit time.
    pub rss_pages: u64,
}

/// Parse the concatenated contents of every `/proc/<pid>/stat`.
///
/// The format's notorious trap is field 2: the executable name is wrapped in parentheses and may
/// itself contain spaces *and* parentheses — `(Web Content)`, `(foo (bar))`. Splitting the line on
/// whitespace therefore shifts every subsequent field for those processes, silently attributing
/// one process's CPU to another's column. Scanning to the **last** `)` is the only correct way to
/// find where the fixed-width fields begin.
pub fn parse_process_stats(text: &str) -> Vec<ProcessStat> {
    let mut out = Vec::new();

    for line in text.lines() {
        let Some(open) = line.find('(') else { continue };
        let Some(close) = line.rfind(')') else {
            continue;
        };
        if close < open {
            continue;
        }

        let Ok(pid) = line[..open].trim().parse::<u32>() else {
            continue;
        };
        let comm = line[open + 1..close].to_string();

        // After the closing paren the fields are positional again. Numbering here is relative to
        // that point: index 0 is `state` (field 3), so field N is at index N - 3.
        let rest: Vec<&str> = line[close + 1..].split_whitespace().collect();
        let at = |field: usize| rest.get(field - 3).copied().unwrap_or("0");

        let utime = at(14).parse::<u64>().unwrap_or(0);
        let stime = at(15).parse::<u64>().unwrap_or(0);

        out.push(ProcessStat {
            pid,
            comm,
            state: at(3).chars().next().unwrap_or('?'),
            cpu_ticks: utime.saturating_add(stime),
            rss_pages: at(24).parse::<u64>().unwrap_or(0),
        });
    }

    out
}

/// Argv for the process-table read. Shared with the tests so they cannot drift from the collector.
///
/// One command for the whole table: the shell expands the glob and `cat` concatenates, so several
/// hundred processes cost one entry in the batch rather than several hundred.
///
/// Both trailing guards matter. A process that exits between the glob expanding and `cat` opening
/// its file makes `cat` fail, and a shell reports the *last* failure as the command's status — so
/// without `exit 0` a single exiting process discards the entire process table. `2>/dev/null`
/// keeps that same race from injecting error text into the payload.
pub const PROCESS_ARGV: [&str; 3] = ["sh", "-c", "cat /proc/[0-9]*/stat 2>/dev/null; exit 0"];

pub struct ProcessSource {
    descriptor: SourceDescriptor,
}

impl Default for ProcessSource {
    fn default() -> Self {
        ProcessSource {
            descriptor: SourceDescriptor {
                id: SourceId::new("proc.process"),
                display: "Processes".into(),
                description: "Per-process CPU and memory, so a busy host can be explained".into(),
                produces: vec![EntityKind::Process],
                requires: Requirements::path("/proc/stat"),
                default_enabled: true,
            },
        }
    }
}

impl ProcessSource {
    fn request() -> Request {
        Request::exec(PROCESS_ARGV)
    }
}

impl Source for ProcessSource {
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

        let mut processes = parse_process_stats(text);
        // Kernel threads have no resident memory and no user-space work to explain; they would
        // fill the table with `kworker` entries that never tell anyone anything.
        processes.retain(|p| p.rss_pages > 0);
        processes.sort_by(|a, b| b.rss_pages.cmp(&a.rss_pages));
        processes.truncate(MAX_PROCESSES);

        // Identical to the per-core CPU scale: a rate of `clock_ticks` per second is one core fully
        // occupied, which is 100 %. A process using four cores therefore reads 400 %, exactly as
        // `top` reports it.
        let cpu_scale = 100.0 / ctx.caps.clock_ticks as f64;
        let page_size = ctx.caps.page_size.max(1);

        for process in processes {
            // The pid is in the entity id so a recycled pid never inherits the previous process's
            // counter, and the name is a label so the UI can show something readable.
            let entity = Entity::child(&ctx.host, EntityKind::Process, process.pid.to_string())
                .with_label("command", &process.comm)
                .with_label("state", process.state.to_string());

            out.emit(
                SeriesDescriptor::counter(id, &entity.id, "cpu", "CPU", Unit::Percent)
                    .with_scale(cpu_scale),
                process.cpu_ticks,
            );
            out.emit(
                SeriesDescriptor::gauge(id, &entity.id, "rss", "Memory", Unit::Bytes),
                process.rss_pages.saturating_mul(page_size),
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

    /// pid (comm) state ppid pgrp session tty tpgid flags min_flt cmin_flt maj_flt cmaj_flt
    /// utime stime cutime cstime priority nice num_threads itrealvalue starttime vsize rss
    fn stat_line(pid: u32, comm: &str, utime: u64, stime: u64, rss: u64) -> String {
        let mut fields = vec![pid.to_string(), format!("({comm})"), "S".into()];
        // Fields 4..13 inclusive.
        fields.extend((4..=13).map(|_| "0".to_string()));
        fields.push(utime.to_string()); // 14
        fields.push(stime.to_string()); // 15
                                        // 16..23.
        fields.extend((16..=23).map(|_| "0".to_string()));
        fields.push(rss.to_string()); // 24
        fields.join(" ")
    }

    #[test]
    fn parses_the_positional_fields() {
        let stats = parse_process_stats(&stat_line(4242, "nginx", 100, 50, 1024));
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].pid, 4242);
        assert_eq!(stats[0].comm, "nginx");
        assert_eq!(stats[0].state, 'S');
        assert_eq!(stats[0].cpu_ticks, 150, "utime and stime should be summed");
        assert_eq!(stats[0].rss_pages, 1024);
    }

    /// The classic `/proc/<pid>/stat` trap. Splitting on whitespace shifts every field for these
    /// processes, so one process's CPU lands in another's column.
    #[test]
    fn handles_command_names_containing_spaces_and_parentheses() {
        for name in ["Web Content", "foo (bar)", "a)b(c", "((("] {
            let stats = parse_process_stats(&stat_line(7, name, 10, 5, 99));
            assert_eq!(stats.len(), 1, "{name:?} was dropped");
            assert_eq!(stats[0].comm, name, "{name:?} parsed wrongly");
            assert_eq!(
                stats[0].cpu_ticks, 15,
                "{name:?} shifted the numeric fields"
            );
            assert_eq!(stats[0].rss_pages, 99);
        }
    }

    #[test]
    fn skips_malformed_and_truncated_lines() {
        assert!(parse_process_stats("not a process line").is_empty());
        assert!(parse_process_stats("(noleadingpid) S 0").is_empty());
        assert!(parse_process_stats(")backwards( S 0").is_empty());
        // A truncated line yields zeroes rather than a panic.
        let stats = parse_process_stats("5 (short) R");
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].cpu_ticks, 0);
    }

    #[test]
    fn parses_a_whole_table() {
        let table = [
            stat_line(1, "systemd", 500, 100, 2048),
            stat_line(2, "kthreadd", 0, 0, 0),
            stat_line(999, "kvm", 100_000, 20_000, 8_000_000),
        ]
        .join("\n");

        let stats = parse_process_stats(&table);
        assert_eq!(stats.len(), 3);
        assert_eq!(stats[2].comm, "kvm");
        assert_eq!(stats[2].cpu_ticks, 120_000);
    }

    #[test]
    fn emits_an_entity_per_process_with_a_cpu_counter_and_memory_gauge() {
        let table = [
            stat_line(1, "systemd", 500, 100, 2048),
            stat_line(999, "kvm", 100_000, 20_000, 8_000_000),
        ]
        .join("\n");
        let (ctx, responses) = corpus("debian").exec_literal(&PROCESS_ARGV, &table).build();
        let out = sink_for(&ProcessSource::default(), &ctx, &responses);

        assert_eq!(out.entities.len(), 2);
        assert!(out.entities.iter().all(|e| e.kind == EntityKind::Process));

        // Largest resident set first, so the cap keeps what matters.
        assert_eq!(out.entities[0].display, "999");
        assert_eq!(
            out.entities[0].labels.get("command").map(String::as_str),
            Some("kvm")
        );

        // Memory is pages times the host page size.
        let rss = out
            .descriptors
            .iter()
            .find(|d| d.entity == out.entities[0].id && d.metric == "rss")
            .expect("rss series");
        let sample = out.samples.iter().find(|s| s.series == rss.id).unwrap();
        assert_eq!(sample.value.as_f64(), Some(8_000_000.0 * 4096.0));
    }

    /// A process pinning one core must read 100 %, and one using four cores 400 %, exactly as
    /// `top` reports it — the scale is what makes the stateless counter produce that.
    #[test]
    fn cpu_scale_matches_one_core_at_one_hundred_percent() {
        let (ctx, responses) = corpus("debian")
            .exec_literal(&PROCESS_ARGV, &stat_line(1, "busy", 0, 0, 100))
            .build();
        let out = sink_for(&ProcessSource::default(), &ctx, &responses);

        let cpu = out.descriptors.iter().find(|d| d.metric == "cpu").unwrap();
        let one_core_rate = ctx.caps.clock_ticks as f64;
        assert!((one_core_rate * cpu.scale - 100.0).abs() < 1e-9);
        assert_eq!(cpu.effective_unit(), Unit::Percent);
    }

    /// Kernel threads have no resident memory and nothing to explain; they would crowd out the
    /// processes a person is actually looking for.
    #[test]
    fn kernel_threads_are_excluded() {
        let table = [
            stat_line(2, "kthreadd", 900, 900, 0),
            stat_line(3, "kworker/0:1", 900, 900, 0),
            stat_line(500, "postgres", 10, 10, 4096),
        ]
        .join("\n");
        let (ctx, responses) = corpus("debian").exec_literal(&PROCESS_ARGV, &table).build();
        let out = sink_for(&ProcessSource::default(), &ctx, &responses);

        let names: Vec<_> = out
            .entities
            .iter()
            .filter_map(|e| e.labels.get("command").cloned())
            .collect();
        assert_eq!(names, vec!["postgres"]);
    }

    /// The process table is unbounded on a real host; the store is not.
    #[test]
    fn the_table_is_capped() {
        let table: String = (1..=MAX_PROCESSES as u32 + 200)
            .map(|pid| stat_line(pid, "proc", 1, 1, pid as u64))
            .collect::<Vec<_>>()
            .join("\n");
        let (ctx, responses) = corpus("debian").exec_literal(&PROCESS_ARGV, &table).build();
        let out = sink_for(&ProcessSource::default(), &ctx, &responses);

        assert_eq!(out.entities.len(), MAX_PROCESSES);
        // The cut keeps the largest, not the first N encountered.
        assert_eq!(out.entities[0].display, (MAX_PROCESSES + 200).to_string());
    }

    #[test]
    fn a_silent_host_produces_nothing() {
        let (ctx, _) = corpus("debian").build();
        let out = sink_for(
            &ProcessSource::default(),
            &ctx,
            &sg_model::Responses::default(),
        );
        assert!(out.is_empty());
    }

    /// The `exit 0` is load-bearing: a process exiting mid-glob makes `cat` fail, and a non-zero
    /// status makes the transport discard the whole payload.
    #[test]
    fn the_probe_normalises_its_exit_status() {
        let script = PROCESS_ARGV[2];
        assert!(script.trim_end().ends_with("exit 0"), "{script}");
        assert!(script.contains("2>/dev/null"));
    }
}
