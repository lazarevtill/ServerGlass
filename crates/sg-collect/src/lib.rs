//! Built-in ServerGlass collectors.
//!
//! Every collector here is a [`Source`](sg_model::Source): it declares which files or commands it
//! needs, and separately parses whatever came back. None of them performs I/O, holds state between
//! ticks, or knows that SSH exists — which is what lets the scheduler merge them all into one
//! round trip, and what lets a WebAssembly plugin be the same kind of thing as a built-in.
//!
//! Two conventions run through all of them:
//!
//! - **Missing data is not an error.** A host without `/proc/pressure` simply produces no pressure
//!   samples. Parsers return `Ok(())` on absent responses rather than reporting a failure the user
//!   can do nothing about.
//! - **Counters stay raw.** A source emits the cumulative number the kernel gave it and declares
//!   [`SeriesKind::Counter`](sg_model::SeriesKind::Counter); the scheduler differentiates. Sources
//!   that computed their own rates would need to remember the previous tick, and statelessness is
//!   what keeps them testable and portable to the plugin ABI.

pub mod cgroup;
pub mod cpu;
pub mod diskio;
pub mod filesystem;
pub mod load;
pub mod memory;
pub mod network;
pub mod pressure;
pub mod process;
pub mod tcp;

#[cfg(test)]
pub mod testing;

use sg_model::Source;

pub use cgroup::CgroupSource;
pub use cpu::CpuSource;
pub use diskio::DiskIoSource;
pub use filesystem::FilesystemSource;
pub use load::LoadSource;
pub use memory::MemorySource;
pub use network::NetworkSource;
pub use pressure::PressureSource;
pub use process::ProcessSource;
pub use tcp::TcpSource;

/// Every built-in collector, in the order the status page shows them.
pub fn builtin_sources() -> Vec<Box<dyn Source>> {
    vec![
        Box::new(CpuSource::default()),
        Box::new(MemorySource::default()),
        Box::new(LoadSource::default()),
        Box::new(FilesystemSource::default()),
        Box::new(DiskIoSource::default()),
        Box::new(NetworkSource::default()),
        Box::new(TcpSource::default()),
        Box::new(PressureSource::default()),
        Box::new(CgroupSource::default()),
        Box::new(ProcessSource::default()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::corpus;
    use std::collections::HashSet;

    #[test]
    fn source_ids_are_unique() {
        let mut seen = HashSet::new();
        for source in builtin_sources() {
            let id = source.descriptor().id.clone();
            assert!(seen.insert(id.clone()), "duplicate source id {id}");
        }
    }

    /// Series ids must not collide across sources, or one collector's samples would overwrite
    /// another's in the scheduler's store.
    #[test]
    fn series_ids_are_unique_across_all_sources() {
        let (ctx, responses) = corpus("debian")
            .file("/proc/stat")
            .file("/proc/meminfo")
            .file("/proc/loadavg")
            .file("/proc/uptime")
            .file("/proc/net/dev")
            .file("/proc/net/snmp")
            .file("/proc/net/sockstat")
            .file("/proc/diskstats")
            .exec(&["df", "-P", "-k"], "df-P-k")
            .build();

        let mut owners: std::collections::HashMap<String, String> = Default::default();
        for source in builtin_sources() {
            let out = testing::sink_for(source.as_ref(), &ctx, &responses);
            for descriptor in &out.descriptors {
                let previous = owners.insert(
                    descriptor.id.to_string(),
                    source.descriptor().id.to_string(),
                );
                if let Some(previous) = previous {
                    assert_eq!(
                        previous,
                        source.descriptor().id.to_string(),
                        "series {} is claimed by two different sources",
                        descriptor.id
                    );
                }
            }
        }
        assert!(
            !owners.is_empty(),
            "no series were produced from the corpus"
        );
    }

    /// Every sample must have a descriptor, or the UI receives a value it cannot label or scale.
    #[test]
    fn every_sample_has_a_descriptor() {
        for host in testing::HOSTS {
            let (ctx, responses) = corpus(host)
                .file("/proc/stat")
                .file("/proc/meminfo")
                .file("/proc/loadavg")
                .file("/proc/uptime")
                .file("/proc/net/dev")
                .file("/proc/net/snmp")
                .file("/proc/net/sockstat")
                .file("/proc/diskstats")
                .exec(&["df", "-P", "-k"], "df-P-k")
                .build();

            for source in builtin_sources() {
                let out = testing::sink_for(source.as_ref(), &ctx, &responses);
                let described: HashSet<_> = out.descriptors.iter().map(|d| &d.id).collect();
                for sample in &out.samples {
                    assert!(
                        described.contains(&sample.series),
                        "{host}/{}: sample {} has no descriptor",
                        source.descriptor().id,
                        sample.series
                    );
                }
            }
        }
    }

    /// Every entity referenced by a descriptor must be either the host or an entity the source
    /// declared, or the UI has a series belonging to a node that does not exist in its tree.
    #[test]
    fn every_series_belongs_to_a_declared_entity() {
        let (ctx, responses) = corpus("debian")
            .file("/proc/stat")
            .file("/proc/net/dev")
            .file("/proc/diskstats")
            .exec(&["df", "-P", "-k"], "df-P-k")
            .build();

        for source in builtin_sources() {
            let out = testing::sink_for(source.as_ref(), &ctx, &responses);
            let mut known: HashSet<_> = out.entities.iter().map(|e| e.id.clone()).collect();
            known.insert(ctx.host.id.clone());

            for descriptor in &out.descriptors {
                assert!(
                    known.contains(&descriptor.entity),
                    "{}: series {} references undeclared entity {}",
                    source.descriptor().id,
                    descriptor.id,
                    descriptor.entity
                );
            }
        }
    }

    /// A host that answers nothing must produce nothing — not a screen of zeroes that looks like
    /// a healthy idle server.
    #[test]
    fn silent_host_produces_no_samples() {
        let (ctx, _) = corpus("debian").build();
        let empty = sg_model::Responses::default();

        for source in builtin_sources() {
            let out = testing::sink_for(source.as_ref(), &ctx, &empty);
            assert!(
                out.samples.is_empty(),
                "{} invented {} samples from no data",
                source.descriptor().id,
                out.samples.len()
            );
        }
    }

    /// The whole design rests on a refresh being one round trip, which requires the request set to
    /// be small and fully declared up front.
    #[test]
    fn a_full_refresh_declares_a_modest_deduplicated_request_set() {
        let (ctx, _) = corpus("debian").build();

        let mut all = Vec::new();
        for source in builtin_sources() {
            all.extend(source.requests(&ctx));
        }

        let unique: HashSet<_> = all.iter().map(|r| r.id()).collect();
        // A ceiling, not a target. Every entry is one more thing concatenated into the same
        // single round trip, so the cost of crossing this is bytes rather than latency — but an
        // unbounded request set is how a one-round-trip design quietly turns into a slow one.
        // Raise it deliberately when a collector earns its place, never to make a test pass.
        assert!(
            unique.len() <= 16,
            "a refresh would fetch {} distinct things: {all:#?}",
            unique.len()
        );
        assert!(
            all.iter().all(|r| r.is_remote()),
            "no built-in should need app-side HTTP"
        );
    }
}
