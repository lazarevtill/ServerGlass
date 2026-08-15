//! Memory and swap from `/proc/meminfo`.

use std::collections::HashMap;

use sg_model::{
    EntityKind, ParseResult, Request, Requirements, Responses, SampleSink, SeriesDescriptor, Source,
    SourceDescriptor, SourceId, TargetCtx, Unit,
};

/// Parse `/proc/meminfo` into bytes, keyed by field name without the colon.
///
/// The kernel reports kibibytes with an explicit `kB` suffix on most fields and no suffix on a
/// few (`HugePages_Total`). Converting on the suffix rather than assuming keeps the unsuffixed
/// counts from being inflated by 1024.
pub fn parse_meminfo(text: &str) -> HashMap<String, u64> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else { continue };
        let mut parts = rest.split_whitespace();
        let Some(value) = parts.next().and_then(|v| v.parse::<u64>().ok()) else { continue };
        let bytes = match parts.next() {
            Some("kB") | Some("KB") => value.saturating_mul(1024),
            _ => value,
        };
        out.insert(key.to_string(), bytes);
    }
    out
}

pub struct MemorySource {
    descriptor: SourceDescriptor,
}

impl Default for MemorySource {
    fn default() -> Self {
        MemorySource {
            descriptor: SourceDescriptor {
                id: SourceId::new("proc.memory"),
                display: "Memory".into(),
                description: "Memory and swap usage from /proc/meminfo".into(),
                produces: vec![EntityKind::Host],
                requires: Requirements::path("/proc/meminfo"),
                default_enabled: true,
            },
        }
    }
}

impl MemorySource {
    fn request() -> Request {
        Request::read("/proc/meminfo")
    }
}

impl Source for MemorySource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    fn requests(&self, _ctx: &TargetCtx) -> Vec<Request> {
        vec![Self::request()]
    }

    fn parse(&self, ctx: &TargetCtx, responses: &Responses, out: &mut SampleSink) -> ParseResult {
        let Some(text) = responses.text(&Self::request()) else { return Ok(()) };
        let mem = parse_meminfo(text);
        let id = &self.descriptor.id;
        let host = &ctx.host.id;

        let Some(&total) = mem.get("MemTotal") else { return Ok(()) };
        if total == 0 {
            return Ok(());
        }

        // `MemAvailable` is the kernel's own estimate of what a new allocation could obtain, and
        // is the only correct basis for "used" on Linux. The older `total - free - buffers -
        // cached` arithmetic is the reason so many tools report a healthy host as out of memory;
        // it is used here only on pre-3.14 kernels that do not publish MemAvailable.
        let available = mem.get("MemAvailable").copied().unwrap_or_else(|| {
            mem.get("MemFree").copied().unwrap_or(0)
                + mem.get("Buffers").copied().unwrap_or(0)
                + mem.get("Cached").copied().unwrap_or(0)
        });
        let available = available.min(total);
        let used = total - available;

        out.emit(
            SeriesDescriptor::gauge(id, host, "mem_usage", "Memory", Unit::Percent),
            used as f64 / total as f64 * 100.0,
        );
        out.emit(
            SeriesDescriptor::gauge(id, host, "mem_used", "Used", Unit::Bytes).with_max(total as f64),
            used,
        );
        out.emit(SeriesDescriptor::gauge(id, host, "mem_total", "Total", Unit::Bytes), total);
        out.emit(
            SeriesDescriptor::gauge(id, host, "mem_available", "Available", Unit::Bytes),
            available,
        );

        for (field, metric, display) in [
            ("MemFree", "mem_free", "Free"),
            ("Cached", "mem_cached", "Cached"),
            ("Buffers", "mem_buffers", "Buffers"),
        ] {
            if let Some(&value) = mem.get(field) {
                out.emit(SeriesDescriptor::gauge(id, host, metric, display, Unit::Bytes), value);
            }
        }

        // A host with swap disabled reports SwapTotal 0; emitting a 0/0 percentage there would
        // render as a permanently full gauge.
        if let Some(&swap_total) = mem.get("SwapTotal").filter(|t| **t > 0) {
            let swap_free = mem.get("SwapFree").copied().unwrap_or(0).min(swap_total);
            let swap_used = swap_total - swap_free;
            out.emit(
                SeriesDescriptor::gauge(id, host, "swap_usage", "Swap", Unit::Percent),
                swap_used as f64 / swap_total as f64 * 100.0,
            );
            out.emit(
                SeriesDescriptor::gauge(id, host, "swap_used", "Swap used", Unit::Bytes)
                    .with_max(swap_total as f64),
                swap_used,
            );
            out.emit(
                SeriesDescriptor::gauge(id, host, "swap_total", "Swap total", Unit::Bytes),
                swap_total,
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{corpus, metrics, sink_for, value_of, HOSTS};

    #[test]
    fn converts_kilobytes_to_bytes_and_leaves_counts_alone() {
        let mem = parse_meminfo("MemTotal:  1024 kB\nHugePages_Total:  7\nBogus: x kB\n");
        assert_eq!(mem.get("MemTotal"), Some(&1_048_576));
        assert_eq!(mem.get("HugePages_Total"), Some(&7));
        assert_eq!(mem.get("Bogus"), None);
    }

    #[test]
    fn parses_real_corpora_from_both_distributions() {
        for host in HOSTS {
            let (ctx, responses) = corpus(host).file("/proc/meminfo").build();
            let out = sink_for(&MemorySource::default(), &ctx, &responses);

            let total = value_of(&out, "mem_total").expect("mem_total");
            let used = value_of(&out, "mem_used").expect("mem_used");
            let available = value_of(&out, "mem_available").expect("mem_available");
            let usage = value_of(&out, "mem_usage").expect("mem_usage");

            assert!(total > 0.0, "{host}: no total memory");
            assert!((used + available - total).abs() < 1.0, "{host}: used + available != total");
            assert!((0.0..=100.0).contains(&usage), "{host}: usage {usage} out of range");
            assert!((usage - used / total * 100.0).abs() < 1e-6, "{host}: percentage disagrees");
        }
    }

    /// `MemAvailable` is the correct basis for "used"; the free+buffers+cached fallback exists
    /// only for kernels that predate it.
    #[test]
    fn prefers_mem_available_over_the_legacy_estimate() {
        let (ctx, responses) = corpus("debian")
            .literal(
                "/proc/meminfo",
                "MemTotal: 1000 kB\nMemFree: 100 kB\nBuffers: 50 kB\nCached: 150 kB\nMemAvailable: 700 kB\n",
            )
            .build();
        let out = sink_for(&MemorySource::default(), &ctx, &responses);

        // MemAvailable says 700 used=300. The legacy estimate would have said 100+50+150=300
        // available, i.e. 700 used — more than double.
        assert_eq!(value_of(&out, "mem_used"), Some(300.0 * 1024.0));
        assert_eq!(value_of(&out, "mem_usage"), Some(30.0));
    }

    #[test]
    fn falls_back_when_mem_available_is_absent() {
        let (ctx, responses) = corpus("debian")
            .literal("/proc/meminfo", "MemTotal: 1000 kB\nMemFree: 100 kB\nBuffers: 50 kB\nCached: 150 kB\n")
            .build();
        let out = sink_for(&MemorySource::default(), &ctx, &responses);
        assert_eq!(value_of(&out, "mem_used"), Some(700.0 * 1024.0));
    }

    #[test]
    fn omits_swap_series_entirely_when_swap_is_disabled() {
        let (ctx, responses) = corpus("debian")
            .literal("/proc/meminfo", "MemTotal: 1000 kB\nMemAvailable: 500 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n")
            .build();
        let out = sink_for(&MemorySource::default(), &ctx, &responses);

        // A 0/0 swap gauge would render as permanently full.
        assert!(!metrics(&out).iter().any(|m| m.starts_with("swap")));
    }

    #[test]
    fn reports_swap_when_present() {
        let (ctx, responses) = corpus("debian")
            .literal("/proc/meminfo", "MemTotal: 1000 kB\nMemAvailable: 500 kB\nSwapTotal: 400 kB\nSwapFree: 100 kB\n")
            .build();
        let out = sink_for(&MemorySource::default(), &ctx, &responses);
        assert_eq!(value_of(&out, "swap_used"), Some(300.0 * 1024.0));
        assert_eq!(value_of(&out, "swap_usage"), Some(75.0));
    }

    #[test]
    fn a_host_reporting_zero_total_produces_nothing_rather_than_dividing_by_zero() {
        let (ctx, responses) =
            corpus("debian").literal("/proc/meminfo", "MemTotal: 0 kB\n").build();
        let out = sink_for(&MemorySource::default(), &ctx, &responses);
        assert!(out.is_empty());
    }
}
