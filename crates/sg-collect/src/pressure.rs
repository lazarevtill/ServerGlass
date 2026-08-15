//! Pressure Stall Information, from `/proc/pressure/{cpu,memory,io}`.
//!
//! PSI answers the question load average only gestures at: *is this machine actually struggling?*
//!
//! A load average of 4.0 means nothing without knowing the core count, says nothing about which
//! resource is scarce, and counts uninterruptible sleep as load even when the machine is idle. PSI
//! instead reports, directly, the share of wall-clock time that tasks spent stalled waiting for
//! CPU, memory or I/O. It needs no denominator and no interpretation: 30 means 30% of the time
//! something was waiting.
//!
//! Two lines per resource:
//!
//! - `some` — at least one task was stalled. Latency is being felt.
//! - `full` — *every* non-idle task was stalled, so the resource was wasted outright. Not reported
//!   for CPU, where by definition something can always run.
//!
//! Added in kernel 4.20 and absent on hosts without `CONFIG_PSI`, so the whole source is gated on
//! the file being readable — capability detection already probes for it.

use sg_model::{
    EntityKind, ParseResult, Request, Requirements, Responses, SampleSink, SeriesDescriptor,
    Source, SourceDescriptor, SourceId, TargetCtx, Unit,
};

/// One `some`/`full` line: the share of time stalled over three windows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stall {
    pub avg10: f64,
    pub avg60: f64,
    pub avg300: f64,
}

/// Both lines for one resource.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pressure {
    pub some: Stall,
    /// Absent for CPU, where the kernel does not report it.
    pub full: Option<Stall>,
}

/// Parse one `/proc/pressure/*` file.
///
/// ```text
/// some avg10=0.00 avg60=0.13 avg300=0.05 total=12345
/// full avg10=0.00 avg60=0.00 avg300=0.00 total=0
/// ```
///
/// `total` is a cumulative microsecond counter and is deliberately ignored: the averages are
/// already the answer, and differentiating the total would only reproduce them less accurately.
pub fn parse_pressure(text: &str) -> Pressure {
    let mut pressure = Pressure::default();

    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let Some(kind) = fields.next() else { continue };

        let mut stall = Stall::default();
        for field in fields {
            let Some((key, value)) = field.split_once('=') else {
                continue;
            };
            let Ok(value) = value.parse::<f64>() else {
                continue;
            };
            match key {
                "avg10" => stall.avg10 = value,
                "avg60" => stall.avg60 = value,
                "avg300" => stall.avg300 = value,
                _ => {}
            }
        }

        match kind {
            "some" => pressure.some = stall,
            "full" => pressure.full = Some(stall),
            _ => {}
        }
    }

    pressure
}

/// The three resources PSI reports on, and how each is named for a reader.
const RESOURCES: [(&str, &str, &str); 3] = [
    ("cpu", "/proc/pressure/cpu", "CPU"),
    ("memory", "/proc/pressure/memory", "Memory"),
    ("io", "/proc/pressure/io", "I/O"),
];

pub struct PressureSource {
    descriptor: SourceDescriptor,
}

impl Default for PressureSource {
    fn default() -> Self {
        PressureSource {
            descriptor: SourceDescriptor {
                id: SourceId::new("proc.pressure"),
                display: "Pressure".into(),
                description: "Time spent stalled waiting for CPU, memory or I/O".into(),
                produces: vec![EntityKind::Host],
                // Kernel 4.20+ with CONFIG_PSI. Gating here means hosts without it simply never
                // show the section rather than showing zeroes.
                requires: Requirements::path("/proc/pressure/cpu"),
                default_enabled: true,
            },
        }
    }
}

impl Source for PressureSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    fn requests(&self, _ctx: &TargetCtx) -> Vec<Request> {
        RESOURCES
            .iter()
            .map(|(_, path, _)| Request::read(*path))
            .collect()
    }

    fn parse(&self, ctx: &TargetCtx, responses: &Responses, out: &mut SampleSink) -> ParseResult {
        let id = &self.descriptor.id;
        let host = &ctx.host.id;

        for (key, path, display) in RESOURCES {
            let Some(text) = responses.text(&Request::read(path)) else {
                continue;
            };
            let pressure = parse_pressure(text);

            // avg10 is the headline: recent enough to be actionable, smoothed enough not to
            // flicker. The 60-second window is kept alongside it for the trend.
            out.emit(
                SeriesDescriptor::gauge(
                    id,
                    host,
                    format!("pressure_{key}"),
                    format!("{display} pressure"),
                    Unit::Percent,
                ),
                pressure.some.avg10,
            );
            out.emit(
                SeriesDescriptor::gauge(
                    id,
                    host,
                    format!("pressure_{key}_60s"),
                    format!("{display} pressure, 1 min"),
                    Unit::Percent,
                ),
                pressure.some.avg60,
            );

            // `full` means every runnable task was blocked — the resource was not merely
            // contended, it was wasted. Only meaningful for memory and I/O.
            if let Some(full) = pressure.full {
                out.emit(
                    SeriesDescriptor::gauge(
                        id,
                        host,
                        format!("pressure_{key}_full"),
                        format!("{display} fully stalled"),
                        Unit::Percent,
                    ),
                    full.avg10,
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{corpus, sink_for, value_of};

    const MEMORY: &str = "\
some avg10=1.50 avg60=0.75 avg300=0.20 total=123456789
full avg10=0.90 avg60=0.40 avg300=0.10 total=98765432
";

    #[test]
    fn parses_both_lines() {
        let pressure = parse_pressure(MEMORY);
        assert_eq!(pressure.some.avg10, 1.50);
        assert_eq!(pressure.some.avg60, 0.75);
        assert_eq!(pressure.some.avg300, 0.20);
        assert_eq!(pressure.full.map(|f| f.avg10), Some(0.90));
    }

    /// The CPU file has no `full` line — by definition something can always run.
    #[test]
    fn cpu_has_no_full_line() {
        let pressure = parse_pressure("some avg10=12.00 avg60=6.00 avg300=1.00 total=42\n");
        assert_eq!(pressure.some.avg10, 12.0);
        assert_eq!(pressure.full, None);
    }

    #[test]
    fn tolerates_unfamiliar_fields_and_ordering() {
        let pressure = parse_pressure("some total=1 avg60=2.5 unknown=9 avg10=1.5 avg300=0.5\n");
        assert_eq!(pressure.some.avg10, 1.5);
        assert_eq!(pressure.some.avg60, 2.5);
        assert_eq!(pressure.some.avg300, 0.5);
    }

    #[test]
    fn ignores_malformed_input_rather_than_reporting_zero_pressure() {
        // A host that answered garbage must not read as "nothing is stalled".
        let pressure = parse_pressure("not a pressure file\n");
        assert_eq!(pressure, Pressure::default());
        assert_eq!(parse_pressure(""), Pressure::default());
    }

    #[test]
    fn emits_a_gauge_per_resource_and_window() {
        let (ctx, responses) = corpus("debian")
            .literal(
                "/proc/pressure/cpu",
                "some avg10=3.00 avg60=1.00 avg300=0.50 total=1\n",
            )
            .literal("/proc/pressure/memory", MEMORY)
            .literal("/proc/pressure/io", MEMORY)
            .build();
        let out = sink_for(&PressureSource::default(), &ctx, &responses);

        assert_eq!(value_of(&out, "pressure_cpu"), Some(3.0));
        assert_eq!(value_of(&out, "pressure_cpu_60s"), Some(1.0));
        // CPU reports no `full`, so no series should be invented for it.
        assert_eq!(value_of(&out, "pressure_cpu_full"), None);

        assert_eq!(value_of(&out, "pressure_memory"), Some(1.5));
        assert_eq!(value_of(&out, "pressure_memory_full"), Some(0.9));
        assert_eq!(value_of(&out, "pressure_io"), Some(1.5));
    }

    /// Pressure is already a percentage of wall-clock time; it needs no denominator, which is the
    /// entire reason it beats a load average for this.
    #[test]
    fn gauges_are_percentages_bounded_at_one_hundred() {
        let (ctx, responses) = corpus("debian")
            .literal(
                "/proc/pressure/cpu",
                "some avg10=99.99 avg60=0 avg300=0 total=1\n",
            )
            .build();
        let out = sink_for(&PressureSource::default(), &ctx, &responses);

        let gauge = out
            .descriptors
            .iter()
            .find(|d| d.metric == "pressure_cpu")
            .unwrap();
        assert_eq!(gauge.unit, Unit::Percent);
        assert_eq!(gauge.display_max(), Some(100.0));
    }

    /// A host without CONFIG_PSI shows nothing rather than a row of zeroes.
    #[test]
    fn a_host_without_psi_produces_nothing() {
        let (ctx, responses) = corpus("debian").missing("/proc/pressure/cpu").build();
        let out = sink_for(&PressureSource::default(), &ctx, &responses);
        assert!(out.is_empty());
    }

    #[test]
    fn is_gated_on_the_kernel_supporting_it() {
        let source = PressureSource::default();
        let mut caps = sg_model::Capabilities::default();
        assert!(!source.descriptor().requires.satisfied_by(&caps));
        caps.paths.insert("/proc/pressure/cpu".into());
        assert!(source.descriptor().requires.satisfied_by(&caps));
    }
}
