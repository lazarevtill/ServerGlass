//! Load average and uptime.

use sg_model::{
    EntityKind, ParseResult, Request, Requirements, Responses, SampleSink, SeriesDescriptor, Source,
    SourceDescriptor, SourceId, TargetCtx, Unit,
};

/// `/proc/loadavg`: `0.15 0.09 0.03 2/431 12345`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LoadAvg {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
    pub runnable: u64,
    pub total_procs: u64,
}

pub fn parse_loadavg(text: &str) -> Option<LoadAvg> {
    let mut fields = text.split_whitespace();
    let load = LoadAvg {
        one: fields.next()?.parse().ok()?,
        five: fields.next()?.parse().ok()?,
        fifteen: fields.next()?.parse().ok()?,
        ..Default::default()
    };

    // The `runnable/total` field is absent on some minimal kernels; the averages are the point.
    let Some(procs) = fields.next().and_then(|f| f.split_once('/')) else { return Some(load) };
    Some(LoadAvg {
        runnable: procs.0.parse().unwrap_or(0),
        total_procs: procs.1.parse().unwrap_or(0),
        ..load
    })
}

/// `/proc/uptime`: seconds since boot, then aggregate idle seconds across all cores.
pub fn parse_uptime(text: &str) -> Option<f64> {
    text.split_whitespace().next()?.parse().ok()
}

pub struct LoadSource {
    descriptor: SourceDescriptor,
}

impl Default for LoadSource {
    fn default() -> Self {
        LoadSource {
            descriptor: SourceDescriptor {
                id: SourceId::new("proc.load"),
                display: "Load & uptime".into(),
                description: "Load averages, process counts and uptime".into(),
                produces: vec![EntityKind::Host],
                requires: Requirements::path("/proc/loadavg"),
                default_enabled: true,
            },
        }
    }
}

impl LoadSource {
    fn loadavg() -> Request {
        Request::read("/proc/loadavg")
    }

    fn uptime() -> Request {
        Request::read("/proc/uptime")
    }
}

impl Source for LoadSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    fn requests(&self, _ctx: &TargetCtx) -> Vec<Request> {
        vec![Self::loadavg(), Self::uptime()]
    }

    fn parse(&self, ctx: &TargetCtx, responses: &Responses, out: &mut SampleSink) -> ParseResult {
        let id = &self.descriptor.id;
        let host = &ctx.host.id;

        if let Some(load) = responses.text(&Self::loadavg()).and_then(parse_loadavg) {
            // A load average is only interpretable against the core count: 4.0 is saturation on a
            // 4-core box and idle on a 64-core one. Carrying the core count as the gauge maximum
            // lets every UI render it correctly without knowing anything about load averages.
            let cores = ctx.caps.cpu_count.max(1) as f64;
            for (metric, display, value) in [
                ("load1", "Load 1m", load.one),
                ("load5", "Load 5m", load.five),
                ("load15", "Load 15m", load.fifteen),
            ] {
                out.emit(
                    SeriesDescriptor::gauge(id, host, metric, display, Unit::Count).with_max(cores),
                    value,
                );
            }

            if load.total_procs > 0 {
                out.emit(
                    SeriesDescriptor::gauge(id, host, "procs_total", "Processes", Unit::Count),
                    load.total_procs,
                );
            }
        }

        if let Some(seconds) = responses.text(&Self::uptime()).and_then(parse_uptime) {
            out.emit(
                SeriesDescriptor::gauge(id, host, "uptime", "Uptime", Unit::Seconds),
                seconds,
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{corpus, sink_for, value_of, HOSTS};

    #[test]
    fn parses_the_standard_format() {
        let load = parse_loadavg("0.15 0.09 0.03 2/431 12345\n").unwrap();
        assert_eq!(load.one, 0.15);
        assert_eq!(load.five, 0.09);
        assert_eq!(load.fifteen, 0.03);
        assert_eq!(load.runnable, 2);
        assert_eq!(load.total_procs, 431);
    }

    #[test]
    fn tolerates_a_missing_process_field() {
        let load = parse_loadavg("1.0 2.0 3.0\n").unwrap();
        assert_eq!(load.fifteen, 3.0);
        assert_eq!(load.total_procs, 0);
    }

    #[test]
    fn rejects_unparseable_input_rather_than_reporting_zero_load() {
        assert!(parse_loadavg("").is_none());
        assert!(parse_loadavg("not a load average\n").is_none());
        assert!(parse_loadavg("1.0 2.0\n").is_none());
    }

    #[test]
    fn parses_uptime_ignoring_the_idle_column() {
        assert_eq!(parse_uptime("12345.67 98765.43\n"), Some(12345.67));
        assert_eq!(parse_uptime(""), None);
    }

    #[test]
    fn reads_both_corpora() {
        for host in HOSTS {
            let (ctx, responses) =
                corpus(host).file("/proc/loadavg").file("/proc/uptime").build();
            let out = sink_for(&LoadSource::default(), &ctx, &responses);

            assert!(value_of(&out, "load1").is_some(), "{host}: no load1");
            assert!(value_of(&out, "uptime").unwrap() > 0.0, "{host}: no uptime");
        }
    }

    /// Load is meaningless without the core count, so the gauge maximum must carry it.
    #[test]
    fn load_gauges_are_bounded_by_the_core_count() {
        let (ctx, responses) = corpus("debian")
            .file("/proc/stat")
            .literal("/proc/loadavg", "1.0 2.0 3.0 1/10 99\n")
            .build();
        let out = sink_for(&LoadSource::default(), &ctx, &responses);

        let load1 = out.descriptors.iter().find(|d| d.metric == "load1").unwrap();
        assert_eq!(load1.max, Some(ctx.caps.cpu_count as f64));
        assert!(ctx.caps.cpu_count > 0, "corpus should have reported cores");
    }

    #[test]
    fn one_missing_file_does_not_suppress_the_other() {
        let (ctx, responses) =
            corpus("debian").file("/proc/uptime").missing("/proc/loadavg").build();
        let out = sink_for(&LoadSource::default(), &ctx, &responses);

        assert!(value_of(&out, "uptime").is_some());
        assert!(value_of(&out, "load1").is_none());
    }
}
