//! Request merging and tick assembly.
//!
//! This is the piece that turns a pile of independent collectors into one network round trip. It
//! asks every applicable source what it needs, merges the lot into a deduplicated request set,
//! and — once the transport has answered — hands the same responses to every source in turn.

use std::collections::{HashMap, HashSet};

use sg_model::{
    Capabilities, Entity, EntityId, ParseError, Request, Responses, Sample, SampleSink,
    SeriesDescriptor, Source, SourceId, TargetCtx,
};

use crate::rate::RateEngine;

/// The processed result of one refresh.
#[derive(Debug, Default)]
pub struct Tick {
    pub entities: Vec<Entity>,
    pub descriptors: Vec<SeriesDescriptor>,
    /// Rates already derived; gauges untouched.
    pub samples: Vec<Sample>,
    /// Parsers that failed. A tick reports these rather than aborting, so one broken collector
    /// cannot blank the dashboard.
    pub errors: Vec<ParseError>,
}

impl Tick {
    /// Every entity id seen this tick, for pruning things that have gone away.
    pub fn entity_ids(&self) -> Vec<EntityId> {
        self.entities.iter().map(|e| e.id.clone()).collect()
    }
}

/// Owns the source set and the rate state for one target.
pub struct Collector {
    sources: Vec<Box<dyn Source>>,
    disabled: HashSet<SourceId>,
    rate: RateEngine,
}

impl Collector {
    pub fn new(sources: Vec<Box<dyn Source>>) -> Self {
        let disabled = sources
            .iter()
            .filter(|s| !s.descriptor().default_enabled)
            .map(|s| s.descriptor().id.clone())
            .collect();
        Collector {
            sources,
            disabled,
            rate: RateEngine::new(),
        }
    }

    /// Every registered source, whether or not it applies to the connected host.
    pub fn sources(&self) -> impl Iterator<Item = &dyn Source> {
        self.sources.iter().map(AsRef::as_ref)
    }

    pub fn set_enabled(&mut self, source: &SourceId, enabled: bool) {
        if enabled {
            self.disabled.remove(source);
        } else {
            self.disabled.insert(source.clone());
        }
    }

    pub fn is_enabled(&self, source: &SourceId) -> bool {
        !self.disabled.contains(source)
    }

    /// Sources that are switched on *and* whose requirements this host satisfies.
    ///
    /// Capability gating is what stops a BusyBox container from showing a grid of empty gauges for
    /// hardware it has no way to report on.
    pub fn applicable<'a>(
        &'a self,
        caps: &'a Capabilities,
    ) -> impl Iterator<Item = &'a dyn Source> {
        self.sources
            .iter()
            .map(AsRef::as_ref)
            .filter(move |s| self.is_enabled(&s.descriptor().id))
            .filter(move |s| s.descriptor().requires.satisfied_by(caps))
    }

    /// The merged request set for one refresh.
    ///
    /// Deduplicated by [`Request::id`], preserving first-seen order so the generated script is
    /// stable between ticks and diffable when debugging. `/proc/stat` wanted by three sources is
    /// fetched once.
    pub fn requests(&self, ctx: &TargetCtx) -> Vec<Request> {
        let mut seen = HashSet::new();
        let mut merged = Vec::new();

        for source in self.applicable(&ctx.caps) {
            for request in source.requests(ctx) {
                if seen.insert(request.id()) {
                    merged.push(request);
                }
            }
        }

        merged
    }

    /// Run every applicable parser over the batch's responses and derive rates.
    pub fn collect(&mut self, ctx: &TargetCtx, responses: &Responses, at_ms: i64) -> Tick {
        let mut sink = SampleSink::new(at_ms);
        let mut errors = Vec::new();

        for source in self
            .sources
            .iter()
            .filter(|s| !self.disabled.contains(&s.descriptor().id))
            .filter(|s| s.descriptor().requires.satisfied_by(&ctx.caps))
        {
            let mut per_source = SampleSink::new(at_ms);
            match source.parse(ctx, responses, &mut per_source) {
                Ok(()) => sink.absorb(per_source),
                // Keep the tick: a source whose parser broke should cost only its own metrics.
                Err(error) => errors.push(error),
            }
        }

        let SampleSink {
            entities,
            descriptors,
            samples,
            ..
        } = sink;
        let samples = self.rate.process(&descriptors, samples);

        Tick {
            entities: dedup_entities(entities),
            descriptors,
            samples,
            errors,
        }
    }

    /// Drop remembered counter readings. Called on reconnect, where the host may have rebooted.
    pub fn reset_rates(&mut self) {
        self.rate.reset();
    }
}

/// Collapse entities re-declared by several sources, keeping the richest set of labels.
///
/// A disk shows up from both the I/O collector and the SMART collector, each knowing different
/// things about it; the UI should see one disk carrying both.
fn dedup_entities(entities: Vec<Entity>) -> Vec<Entity> {
    let mut merged: HashMap<EntityId, Entity> = HashMap::new();
    let mut order = Vec::new();

    for entity in entities {
        match merged.get_mut(&entity.id) {
            Some(existing) => existing.labels.extend(entity.labels),
            None => {
                order.push(entity.id.clone());
                merged.insert(entity.id.clone(), entity);
            }
        }
    }

    order
        .into_iter()
        .filter_map(|id| merged.remove(&id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sg_model::{
        EntityKind, ParseResult, Requirements, SampleSink, SeriesDescriptor, SourceDescriptor,
        TargetId, Unit,
    };

    /// A source that asks for a fixed set of files and emits one gauge.
    struct Fake {
        descriptor: SourceDescriptor,
        wants: Vec<String>,
        explode: bool,
    }

    impl Fake {
        fn new(id: &str, wants: &[&str]) -> Self {
            Fake {
                descriptor: SourceDescriptor {
                    id: SourceId::new(id),
                    display: id.into(),
                    description: String::new(),
                    produces: vec![EntityKind::Host],
                    requires: Requirements::NONE,
                    default_enabled: true,
                },
                wants: wants.iter().map(|s| s.to_string()).collect(),
                explode: false,
            }
        }

        fn requiring(mut self, requirements: Requirements) -> Self {
            self.descriptor.requires = requirements;
            self
        }

        fn disabled_by_default(mut self) -> Self {
            self.descriptor.default_enabled = false;
            self
        }

        fn failing(mut self) -> Self {
            self.explode = true;
            self
        }
    }

    impl Source for Fake {
        fn descriptor(&self) -> &SourceDescriptor {
            &self.descriptor
        }

        fn requests(&self, _ctx: &TargetCtx) -> Vec<Request> {
            self.wants.iter().map(Request::read).collect()
        }

        fn parse(&self, ctx: &TargetCtx, _r: &Responses, out: &mut SampleSink) -> ParseResult {
            if self.explode {
                return Err(ParseError::new(
                    &self.descriptor.id,
                    "deliberate test failure",
                ));
            }
            out.emit(
                SeriesDescriptor::gauge(
                    &self.descriptor.id,
                    &ctx.host.id,
                    self.descriptor.id.as_str(),
                    "Fake",
                    Unit::Count,
                ),
                1.0_f64,
            );
            Ok(())
        }
    }

    fn ctx() -> TargetCtx {
        let mut caps = Capabilities {
            clock_ticks: 100,
            cpu_count: 4,
            ..Default::default()
        };
        caps.binaries.insert("docker".into());
        caps.paths.insert("/proc/stat".into());
        TargetCtx {
            target: TargetId::new("t"),
            host: Entity::host("test-host"),
            caps,
            interval_ms: 1000,
        }
    }

    /// The reason the requests/parse split exists.
    #[test]
    fn overlapping_requests_are_fetched_once() {
        let collector = Collector::new(vec![
            Box::new(Fake::new("a", &["/proc/stat", "/proc/meminfo"])),
            Box::new(Fake::new("b", &["/proc/stat"])),
            Box::new(Fake::new("c", &["/proc/stat", "/proc/uptime"])),
        ]);

        let requests = collector.requests(&ctx());
        assert_eq!(
            requests.len(),
            3,
            "expected /proc/stat to be merged: {requests:#?}"
        );
        assert_eq!(requests[0], Request::read("/proc/stat"));
    }

    #[test]
    fn request_order_is_stable_between_ticks() {
        let collector = Collector::new(vec![
            Box::new(Fake::new("a", &["/proc/stat", "/proc/meminfo"])),
            Box::new(Fake::new("b", &["/proc/uptime"])),
        ]);

        let ctx = ctx();
        assert_eq!(collector.requests(&ctx), collector.requests(&ctx));
    }

    #[test]
    fn sources_the_host_cannot_support_are_never_scheduled() {
        let collector = Collector::new(vec![
            Box::new(Fake::new("present", &["/proc/stat"])),
            Box::new(
                Fake::new("absent", &["/proc/gpu"]).requiring(Requirements::binary("nvidia-smi")),
            ),
        ]);

        let ctx = ctx();
        let ids: Vec<_> = collector
            .applicable(&ctx.caps)
            .map(|s| s.descriptor().id.to_string())
            .collect();
        assert_eq!(ids, vec!["present"]);
        assert_eq!(collector.requests(&ctx), vec![Request::read("/proc/stat")]);
    }

    #[test]
    fn expensive_sources_stay_off_until_asked_for() {
        let mut collector = Collector::new(vec![
            Box::new(Fake::new("cheap", &["/proc/stat"])),
            Box::new(Fake::new("expensive", &["/proc/everything"]).disabled_by_default()),
        ]);

        assert!(!collector.is_enabled(&SourceId::new("expensive")));
        assert_eq!(collector.requests(&ctx()).len(), 1);

        collector.set_enabled(&SourceId::new("expensive"), true);
        assert_eq!(collector.requests(&ctx()).len(), 2);

        collector.set_enabled(&SourceId::new("cheap"), false);
        assert_eq!(
            collector.requests(&ctx()),
            vec![Request::read("/proc/everything")]
        );
    }

    /// One broken parser must cost only its own metrics.
    #[test]
    fn a_failing_source_does_not_blank_the_others() {
        let mut collector = Collector::new(vec![
            Box::new(Fake::new("good", &["/proc/stat"])),
            Box::new(Fake::new("broken", &["/proc/stat"]).failing()),
            Box::new(Fake::new("also-good", &["/proc/stat"])),
        ]);

        let tick = collector.collect(&ctx(), &Responses::default(), 1_000);

        assert_eq!(
            tick.samples.len(),
            2,
            "a failing parser took its neighbours down with it"
        );
        assert_eq!(tick.errors.len(), 1);
        assert_eq!(tick.errors[0].source, SourceId::new("broken"));
    }

    #[test]
    fn entities_declared_by_several_sources_merge_into_one() {
        let host = Entity::host("h");
        let from_io = Entity::child(&host, EntityKind::Disk, "sda").with_label("bus", "nvme");
        let from_smart = Entity::child(&host, EntityKind::Disk, "sda").with_label("model", "X1");

        let merged = dedup_entities(vec![host.clone(), from_io, from_smart, host]);

        assert_eq!(merged.len(), 2, "the same disk was reported twice");
        let disk = merged.iter().find(|e| e.display == "sda").unwrap();
        assert_eq!(disk.labels.get("bus").map(String::as_str), Some("nvme"));
        assert_eq!(disk.labels.get("model").map(String::as_str), Some("X1"));
    }

    #[test]
    fn reports_the_entity_ids_present_this_tick() {
        let mut collector = Collector::new(vec![Box::new(Fake::new("a", &["/proc/stat"]))]);
        let tick = collector.collect(&ctx(), &Responses::default(), 1);
        // The fake declares no entities of its own, so only what it emitted appears.
        assert!(tick.entity_ids().is_empty());
        assert_eq!(tick.descriptors.len(), 1);
    }
}
