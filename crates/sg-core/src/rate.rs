//! Turning counters into rates.
//!
//! Sources emit the raw cumulative numbers the kernel gave them. Everything time-derived happens
//! here, in one place, using measured elapsed time rather than the nominal refresh interval — a
//! tick that arrived 1.4 seconds after the last one must not be divided by 1.0.
//!
//! This is the only component in the collection path that remembers anything between ticks, and it
//! remembers exactly one reading per counter series. That is the irreducible minimum: a rate is
//! defined by two samples. It is not history, and nothing here is ever written to disk.

use std::collections::HashMap;

use sg_model::{Sample, SeriesDescriptor, SeriesId, SeriesKind, Value};

/// Last raw reading of one counter.
#[derive(Clone, Copy, Debug)]
struct Previous {
    at_ms: i64,
    raw: u64,
}

/// Differentiates counter series and passes everything else through untouched.
#[derive(Default, Debug)]
pub struct RateEngine {
    previous: HashMap<SeriesId, Previous>,
}

impl RateEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert a tick's samples into what the UI should see.
    ///
    /// Counter samples are replaced by their rate; gauges, states and info pass straight through.
    /// A counter yields nothing at all when it cannot honestly produce a rate:
    ///
    /// - **First sighting.** One reading is not a rate. Emitting the raw counter, or zero, would
    ///   put a spike or a trough on every chart at connect time.
    /// - **Counter went backwards.** The device was reset, the interface was recreated, or the
    ///   host rebooted. The correct rate is unknown, and `(huge - small)` would render as a
    ///   spike of gigabytes per second.
    /// - **No time passed.** Two ticks in the same millisecond would divide by zero.
    pub fn process(
        &mut self,
        descriptors: &[SeriesDescriptor],
        samples: Vec<Sample>,
    ) -> Vec<Sample> {
        let by_id: HashMap<&SeriesId, &SeriesDescriptor> =
            descriptors.iter().map(|d| (&d.id, d)).collect();

        let mut out = Vec::with_capacity(samples.len());
        for sample in samples {
            let Some(descriptor) = by_id.get(&sample.series) else {
                // A sample with no descriptor cannot be interpreted; the collector tests forbid
                // this, so reaching it means a source is misbehaving.
                continue;
            };
            if descriptor.kind != SeriesKind::Counter {
                out.push(sample);
                continue;
            }
            let Some(raw) = sample.value.as_counter() else {
                continue;
            };

            let previous = self.previous.insert(
                sample.series.clone(),
                Previous {
                    at_ms: sample.at_ms,
                    raw,
                },
            );

            let Some(previous) = previous else { continue };
            let elapsed_ms = sample.at_ms - previous.at_ms;
            if elapsed_ms <= 0 || raw < previous.raw {
                continue;
            }

            let per_second = (raw - previous.raw) as f64 / (elapsed_ms as f64 / 1000.0);
            out.push(Sample {
                series: sample.series,
                at_ms: sample.at_ms,
                value: Value::Float(per_second * descriptor.scale),
            });
        }

        out
    }

    /// Forget everything. Called on reconnect: counters on the other side may have restarted, and
    /// carrying a pre-disconnect reading across the gap produces one enormous false spike.
    pub fn reset(&mut self) {
        self.previous.clear();
    }

    /// Number of counters currently being tracked.
    pub fn tracked(&self) -> usize {
        self.previous.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sg_model::{EntityId, SourceId, Unit};

    fn ids() -> (SourceId, EntityId) {
        (SourceId::new("test"), EntityId::new("host"))
    }

    fn counter(metric: &str, unit: Unit, scale: f64) -> SeriesDescriptor {
        let (source, entity) = ids();
        SeriesDescriptor::counter(&source, &entity, metric, metric, unit).with_scale(scale)
    }

    fn gauge(metric: &str) -> SeriesDescriptor {
        let (source, entity) = ids();
        SeriesDescriptor::gauge(&source, &entity, metric, metric, Unit::Percent)
    }

    fn sample(descriptor: &SeriesDescriptor, at_ms: i64, value: u64) -> Sample {
        Sample::new(descriptor.id.clone(), at_ms, value)
    }

    #[test]
    fn a_single_reading_is_not_a_rate() {
        let rx = counter("rx", Unit::Bytes, 1.0);
        let mut engine = RateEngine::new();

        let out = engine.process(std::slice::from_ref(&rx), vec![sample(&rx, 1_000, 500)]);
        assert!(
            out.is_empty(),
            "first sighting produced a value out of thin air"
        );
        assert_eq!(
            engine.tracked(),
            1,
            "the reading should still be remembered"
        );
    }

    #[test]
    fn derives_a_rate_from_the_second_reading() {
        let rx = counter("rx", Unit::Bytes, 1.0);
        let mut engine = RateEngine::new();

        engine.process(std::slice::from_ref(&rx), vec![sample(&rx, 1_000, 1_000)]);
        let out = engine.process(std::slice::from_ref(&rx), vec![sample(&rx, 3_000, 5_000)]);

        // 4000 bytes over 2 seconds.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].value.as_f64(), Some(2_000.0));
    }

    /// The tick that arrives late must be divided by the time that actually elapsed, not by the
    /// interval we hoped for.
    #[test]
    fn uses_measured_elapsed_time_not_the_nominal_interval() {
        let rx = counter("rx", Unit::Bytes, 1.0);
        let mut engine = RateEngine::new();

        engine.process(std::slice::from_ref(&rx), vec![sample(&rx, 0, 0)]);
        // A 3.5-second gap after a 1-second refresh was requested.
        let out = engine.process(std::slice::from_ref(&rx), vec![sample(&rx, 3_500, 7_000)]);
        assert_eq!(out[0].value.as_f64(), Some(2_000.0));
    }

    /// A rebooted host or a recreated interface restarts its counters. `(new - old)` underflows,
    /// and even saturating it would draw a spike that never happened.
    #[test]
    fn a_counter_reset_yields_no_sample_rather_than_a_spike() {
        let rx = counter("rx", Unit::Bytes, 1.0);
        let mut engine = RateEngine::new();

        engine.process(std::slice::from_ref(&rx), vec![sample(&rx, 0, 9_000_000)]);
        let out = engine.process(std::slice::from_ref(&rx), vec![sample(&rx, 1_000, 42)]);
        assert!(out.is_empty(), "counter reset produced a phantom spike");

        // The new baseline is adopted, so the tick after the reset works normally again.
        let out = engine.process(std::slice::from_ref(&rx), vec![sample(&rx, 2_000, 1_042)]);
        assert_eq!(out[0].value.as_f64(), Some(1_000.0));
    }

    #[test]
    fn two_ticks_in_the_same_millisecond_do_not_divide_by_zero() {
        let rx = counter("rx", Unit::Bytes, 1.0);
        let mut engine = RateEngine::new();

        engine.process(std::slice::from_ref(&rx), vec![sample(&rx, 5_000, 1)]);
        let out = engine.process(std::slice::from_ref(&rx), vec![sample(&rx, 5_000, 99)]);
        assert!(out.is_empty());
    }

    /// CPU is the reason `scale` exists: jiffies per second scaled into percent of a core.
    #[test]
    fn applies_the_declared_scale_to_the_rate() {
        // 100 ticks/second == one core fully busy == 100%.
        let cpu = counter("cpu", Unit::Percent, 100.0 / 100.0);
        let mut engine = RateEngine::new();

        engine.process(std::slice::from_ref(&cpu), vec![sample(&cpu, 0, 0)]);
        let out = engine.process(std::slice::from_ref(&cpu), vec![sample(&cpu, 1_000, 100)]);
        assert_eq!(
            out[0].value.as_f64(),
            Some(100.0),
            "a fully busy core should read 100%"
        );

        // Half a core over two seconds.
        let out = engine.process(std::slice::from_ref(&cpu), vec![sample(&cpu, 3_000, 200)]);
        assert_eq!(out[0].value.as_f64(), Some(50.0));
    }

    #[test]
    fn gauges_pass_through_untouched() {
        let usage = gauge("mem_usage");
        let mut engine = RateEngine::new();

        let out = engine.process(
            std::slice::from_ref(&usage),
            vec![Sample::new(usage.id.clone(), 1_000, 42.5_f64)],
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].value.as_f64(), Some(42.5));

        // And they are not tracked, so gauges cost no memory here.
        assert_eq!(engine.tracked(), 0);
    }

    #[test]
    fn samples_without_a_descriptor_are_dropped() {
        let mut engine = RateEngine::new();
        let out = engine.process(&[], vec![Sample::new(SeriesId::new("ghost"), 1, 1_u64)]);
        assert!(out.is_empty());
    }

    /// Carrying a pre-disconnect reading across a reconnect produces one enormous false spike,
    /// because the host may have rebooted in the gap.
    #[test]
    fn reset_discards_state_so_reconnects_do_not_spike() {
        let rx = counter("rx", Unit::Bytes, 1.0);
        let mut engine = RateEngine::new();

        engine.process(std::slice::from_ref(&rx), vec![sample(&rx, 0, 1_000_000)]);
        engine.reset();
        assert_eq!(engine.tracked(), 0);

        let out = engine.process(std::slice::from_ref(&rx), vec![sample(&rx, 60_000, 5_000)]);
        assert!(
            out.is_empty(),
            "the first reading after a reconnect must re-baseline"
        );
    }

    #[test]
    fn tracks_each_series_independently() {
        let rx = counter("rx", Unit::Bytes, 1.0);
        let tx = counter("tx", Unit::Bytes, 1.0);
        let mut engine = RateEngine::new();

        let descriptors = [rx.clone(), tx.clone()];
        engine.process(&descriptors, vec![sample(&rx, 0, 0), sample(&tx, 0, 0)]);
        let out = engine.process(
            &descriptors,
            vec![sample(&rx, 1_000, 10), sample(&tx, 1_000, 30)],
        );

        assert_eq!(out.len(), 2);
        let value = |metric: &SeriesDescriptor| {
            out.iter()
                .find(|s| s.series == metric.id)
                .unwrap()
                .value
                .as_f64()
        };
        assert_eq!(value(&rx), Some(10.0));
        assert_eq!(value(&tx), Some(30.0));
        assert_eq!(engine.tracked(), 2);
    }
}
