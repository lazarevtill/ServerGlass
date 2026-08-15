//! CPU utilisation from `/proc/stat`.

use sg_model::{
    Entity, EntityKind, ParseResult, Request, Requirements, Responses, SampleSink,
    SeriesDescriptor, Source, SourceDescriptor, SourceId, TargetCtx, Unit,
};

/// One `cpu`/`cpuN` row of `/proc/stat`, in clock ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
}

impl CpuTimes {
    /// Ticks spent doing anything other than waiting.
    ///
    /// `iowait` counts as idle: the CPU was available, the disk was not. Counting it as busy is a
    /// common mistake that makes an I/O-bound host look CPU-saturated.
    pub fn busy(&self) -> u64 {
        self.user + self.nice + self.system + self.irq + self.softirq + self.steal
    }

    pub fn total(&self) -> u64 {
        self.busy() + self.idle + self.iowait
    }

    /// Parse the numeric tail of a `cpu...` line.
    ///
    /// Kernels have gained fields over time (`steal` in 2.6.11, `guest` in 2.6.24) and BusyBox
    /// hosts may report fewer, so missing trailing fields default to zero rather than failing.
    fn parse(fields: &str) -> CpuTimes {
        let mut n = fields
            .split_whitespace()
            .filter_map(|f| f.parse::<u64>().ok());
        CpuTimes {
            user: n.next().unwrap_or(0),
            nice: n.next().unwrap_or(0),
            system: n.next().unwrap_or(0),
            idle: n.next().unwrap_or(0),
            iowait: n.next().unwrap_or(0),
            irq: n.next().unwrap_or(0),
            softirq: n.next().unwrap_or(0),
            steal: n.next().unwrap_or(0),
        }
    }
}

/// The aggregate row plus one row per logical CPU.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProcStat {
    pub total: CpuTimes,
    /// `(core index, times)`, in the order the kernel listed them.
    pub cores: Vec<(u32, CpuTimes)>,
    /// Context switches since boot, if reported.
    pub context_switches: Option<u64>,
    /// Processes forked since boot, if reported.
    pub processes: Option<u64>,
    /// Runnable processes right now, if reported.
    pub procs_running: Option<u64>,
    /// Processes blocked on I/O right now, if reported.
    pub procs_blocked: Option<u64>,
}

/// Parse `/proc/stat`.
pub fn parse_proc_stat(text: &str) -> ProcStat {
    let mut out = ProcStat::default();

    for line in text.lines() {
        let Some((key, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        match key {
            "cpu" => out.total = CpuTimes::parse(rest),
            "ctxt" => out.context_switches = rest.trim().parse().ok(),
            "processes" => out.processes = rest.trim().parse().ok(),
            "procs_running" => out.procs_running = rest.trim().parse().ok(),
            "procs_blocked" => out.procs_blocked = rest.trim().parse().ok(),
            _ => {
                if let Some(index) = key.strip_prefix("cpu").and_then(|i| i.parse::<u32>().ok()) {
                    out.cores.push((index, CpuTimes::parse(rest)));
                }
            }
        }
    }

    out
}

pub struct CpuSource {
    descriptor: SourceDescriptor,
}

impl Default for CpuSource {
    fn default() -> Self {
        CpuSource {
            descriptor: SourceDescriptor {
                id: SourceId::new("proc.cpu"),
                display: "CPU".into(),
                description: "Per-core and aggregate CPU utilisation from /proc/stat".into(),
                produces: vec![EntityKind::CpuCore],
                requires: Requirements::path("/proc/stat"),
                default_enabled: true,
            },
        }
    }
}

impl CpuSource {
    fn request() -> Request {
        Request::read("/proc/stat")
    }
}

impl Source for CpuSource {
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
        let stat = parse_proc_stat(text);
        let id = &self.descriptor.id;

        // Ticks accrue at `clock_ticks` per second per core, so dividing a tick *rate* by
        // clock_ticks yields the fraction of one core in use. Across the whole machine the
        // denominator is that times the core count, which normalises the aggregate to 0-100%
        // however many cores the host has.
        let cores = stat.cores.len().max(1) as f64;
        let per_core_scale = 100.0 / ctx.caps.clock_ticks as f64;
        let aggregate_scale = per_core_scale / cores;

        let host = &ctx.host.id;
        out.emit(
            SeriesDescriptor::counter(id, host, "cpu_usage", "CPU", Unit::Percent)
                .with_scale(aggregate_scale),
            stat.total.busy(),
        );

        // The breakdown uses the same scale, so the parts sum to the whole.
        for (metric, display, ticks) in [
            ("cpu_user", "User", stat.total.user + stat.total.nice),
            (
                "cpu_system",
                "System",
                stat.total.system + stat.total.irq + stat.total.softirq,
            ),
            ("cpu_iowait", "I/O wait", stat.total.iowait),
            ("cpu_steal", "Steal", stat.total.steal),
        ] {
            out.emit(
                SeriesDescriptor::counter(id, host, metric, display, Unit::Percent)
                    .with_scale(aggregate_scale),
                ticks,
            );
        }

        if let Some(running) = stat.procs_running {
            out.emit(
                SeriesDescriptor::gauge(id, host, "procs_running", "Running", Unit::Count),
                running,
            );
        }
        if let Some(blocked) = stat.procs_blocked {
            out.emit(
                SeriesDescriptor::gauge(id, host, "procs_blocked", "Blocked", Unit::Count),
                blocked,
            );
        }
        if let Some(switches) = stat.context_switches {
            out.emit(
                SeriesDescriptor::counter(
                    id,
                    host,
                    "ctx_switches",
                    "Context switches",
                    Unit::Count,
                ),
                switches,
            );
        }

        for (index, times) in &stat.cores {
            let core = Entity::child(&ctx.host, EntityKind::CpuCore, index.to_string());
            out.emit(
                SeriesDescriptor::counter(id, &core.id, "usage", "Usage", Unit::Percent)
                    .with_scale(per_core_scale),
                times.busy(),
            );
            out.entity(core);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{corpus, sink_for, HOSTS};

    #[test]
    fn parses_the_aggregate_row() {
        let stat = parse_proc_stat("cpu  100 20 30 4000 5 6 7 8 0 0\nintr 999\n");
        assert_eq!(stat.total.user, 100);
        assert_eq!(stat.total.nice, 20);
        assert_eq!(stat.total.system, 30);
        assert_eq!(stat.total.idle, 4000);
        assert_eq!(stat.total.iowait, 5);
        assert_eq!(stat.total.steal, 8);
        assert_eq!(stat.total.busy(), 100 + 20 + 30 + 6 + 7 + 8);
        assert_eq!(stat.total.total(), stat.total.busy() + 4000 + 5);
    }

    /// A CPU waiting on disk is available, not saturated. Counting iowait as busy would report an
    /// idle-but-IO-bound host as pegged.
    #[test]
    fn iowait_counts_as_idle_not_busy() {
        let stat = parse_proc_stat("cpu 0 0 0 0 5000 0 0 0\n");
        assert_eq!(stat.total.busy(), 0);
        assert_eq!(stat.total.total(), 5000);
    }

    #[test]
    fn tolerates_short_rows_from_older_kernels() {
        let stat = parse_proc_stat("cpu 10 0 5 100\n");
        assert_eq!(stat.total.user, 10);
        assert_eq!(stat.total.idle, 100);
        assert_eq!(stat.total.steal, 0);
    }

    #[test]
    fn collects_cores_in_order_without_counting_the_aggregate() {
        let stat = parse_proc_stat(
            "cpu  9 9 9 9\ncpu0 1 0 0 10\ncpu1 2 0 0 20\ncpu2 3 0 0 30\nctxt 42\nprocs_running 3\n",
        );
        assert_eq!(stat.cores.len(), 3);
        assert_eq!(
            stat.cores.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(stat.cores[1].1.user, 2);
        assert_eq!(stat.context_switches, Some(42));
        assert_eq!(stat.procs_running, Some(3));
        assert_eq!(stat.procs_blocked, None);
    }

    #[test]
    fn ignores_lines_it_does_not_understand() {
        let stat = parse_proc_stat("garbage\n\ncpu\nsoftirq 1 2 3\ncpu0 1 2 3 4\n");
        assert_eq!(stat.cores.len(), 1);
    }

    /// The scale is what lets the source stay stateless, so it has to be exactly right: a rate of
    /// `clock_ticks` per second on one core must render as 100%.
    #[test]
    fn scales_convert_tick_rates_into_percent() {
        for host in HOSTS {
            let (ctx, responses) = corpus(host).file("/proc/stat").build();
            let out = sink_for(&CpuSource::default(), &ctx, &responses);

            let aggregate = out
                .descriptors
                .iter()
                .find(|d| d.metric == "cpu_usage")
                .expect("aggregate CPU series");

            // One core fully busy for one second = clock_ticks ticks/s.
            let one_core_rate = ctx.caps.clock_ticks as f64;
            let cores = ctx.caps.cpu_count as f64;
            assert!(
                (one_core_rate * aggregate.scale - 100.0 / cores).abs() < 1e-9,
                "{host}: one busy core should read as {}%, got {}",
                100.0 / cores,
                one_core_rate * aggregate.scale
            );

            let per_core = out
                .descriptors
                .iter()
                .find(|d| d.metric == "usage")
                .expect("per-core series");
            assert!((one_core_rate * per_core.scale - 100.0).abs() < 1e-9);

            // Percent must survive differentiation as percent, not become percent-per-second.
            assert_eq!(aggregate.effective_unit(), Unit::Percent);
        }
    }

    #[test]
    fn emits_one_entity_per_core_from_real_corpora() {
        let (ctx, responses) = corpus("debian").file("/proc/stat").build();
        let out = sink_for(&CpuSource::default(), &ctx, &responses);

        assert_eq!(out.entities.len(), ctx.caps.cpu_count as usize);
        assert!(out.entities.iter().all(|e| e.kind == EntityKind::CpuCore));
        assert!(out
            .entities
            .iter()
            .all(|e| e.parent.as_ref() == Some(&ctx.host.id)));
    }

    #[test]
    fn produces_nothing_when_the_host_did_not_answer() {
        let (ctx, _) = corpus("debian").build();
        let out = sink_for(&CpuSource::default(), &ctx, &Responses::default());
        assert!(
            out.is_empty(),
            "a missing response should yield no samples, not zeroes"
        );
    }
}
