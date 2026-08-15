//! Samples and the sink sources write them into.

use crate::{Entity, SeriesDescriptor, SeriesId, Value};

/// One reading of one series at one instant.
///
/// Timestamps are Unix milliseconds rather than `std::time::Instant` because samples cross the FFI
/// boundary and are handed to sinks; a monotonic in-process clock would not survive either trip.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Sample {
    pub series: SeriesId,
    pub at_ms: i64,
    pub value: Value,
}

impl Sample {
    pub fn new(series: SeriesId, at_ms: i64, value: impl Into<Value>) -> Self {
        Sample { series, at_ms, value: value.into() }
    }
}

/// What a source writes its results into during [`crate::Source::parse`].
///
/// Sources push entities and descriptors on every tick without checking whether they already
/// exist — the scheduler deduplicates. That keeps parsers stateless, which matters because
/// container and pod sets change between ticks and a stateful parser would have to diff them.
#[derive(Default, Debug)]
pub struct SampleSink {
    pub entities: Vec<Entity>,
    pub descriptors: Vec<SeriesDescriptor>,
    pub samples: Vec<Sample>,
    at_ms: i64,
}

impl SampleSink {
    /// Create a sink stamped with the tick's collection time. Every sample written through
    /// [`SampleSink::push`] shares this timestamp, so a whole tick lines up on the time axis
    /// even though its readings were parsed sequentially.
    pub fn new(at_ms: i64) -> Self {
        SampleSink { at_ms, ..Default::default() }
    }

    /// The tick timestamp, for sources that need to emit off-cycle samples.
    pub fn at_ms(&self) -> i64 {
        self.at_ms
    }

    pub fn entity(&mut self, entity: Entity) -> &mut Self {
        self.entities.push(entity);
        self
    }

    pub fn describe(&mut self, descriptor: SeriesDescriptor) -> &mut Self {
        self.descriptors.push(descriptor);
        self
    }

    /// Record a reading against an already-described series.
    pub fn push(&mut self, series: &SeriesId, value: impl Into<Value>) -> &mut Self {
        self.samples.push(Sample::new(series.clone(), self.at_ms, value));
        self
    }

    /// Describe a series and record its first reading in one call — the common case, since most
    /// sources re-describe their series every tick.
    pub fn emit(&mut self, descriptor: SeriesDescriptor, value: impl Into<Value>) -> &mut Self {
        self.samples.push(Sample::new(descriptor.id.clone(), self.at_ms, value));
        self.descriptors.push(descriptor);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty() && self.descriptors.is_empty() && self.samples.is_empty()
    }

    /// Fold another sink's contents into this one. Used by the scheduler to merge per-source
    /// results into a single tick batch.
    pub fn absorb(&mut self, other: SampleSink) {
        self.entities.extend(other.entities);
        self.descriptors.extend(other.descriptors);
        self.samples.extend(other.samples);
    }
}
