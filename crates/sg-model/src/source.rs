//! The one trait every collector implements.

use crate::{Capabilities, Entity, EntityKind, Requirements, Responses, SampleSink, SourceId, TargetId};

/// Static description of a collector.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SourceDescriptor {
    pub id: SourceId,
    /// Shown in the source-toggle list: "CPU", "Docker containers", "TLS certificate expiry".
    pub display: String,
    /// One line of explanation for the same list.
    pub description: String,
    /// Which entity kinds this source can bring into existence. Lets the UI decide whether a
    /// "Containers" tab should exist before the first tick has landed.
    pub produces: Vec<EntityKind>,
    pub requires: Requirements,
    /// Enabled on first connect without the user opting in. False for anything expensive or
    /// intrusive — a full `smartctl` sweep, a process table on a 5000-process host.
    pub default_enabled: bool,
}

/// Everything a source is told about the host it is collecting from.
#[derive(Clone, Debug)]
pub struct TargetCtx {
    pub target: TargetId,
    /// Root of the entity tree; sources build their entities as children of this.
    pub host: Entity,
    pub caps: Capabilities,
    /// Nominal gap between ticks. Sources should not use this to compute rates — the scheduler
    /// differentiates counters using measured elapsed time — but it is useful for deciding how
    /// much history to ask a command for.
    pub interval_ms: u64,
}

/// A collector.
///
/// Deliberately **synchronous**: neither method performs I/O, so there is no async machinery, no
/// `async_trait` allocation per call, and — the reason it matters — a WebAssembly plugin's
/// synchronous exported functions map onto this trait exactly. Built-ins, declarative probes and
/// plugins are genuinely the same kind of thing.
pub trait Source: Send + Sync {
    fn descriptor(&self) -> &SourceDescriptor;

    /// What this source needs fetched this tick.
    ///
    /// Called every tick, so the set may vary with what the previous tick discovered (a container
    /// source asks for per-container stats only for containers it has seen).
    fn requests(&self, ctx: &TargetCtx) -> Vec<crate::Request>;

    /// Turn the batch's responses into entities, descriptors and samples.
    ///
    /// A response being absent or non-zero is not an error — it means the host does not have that
    /// data, and the source should quietly emit nothing for it. Reserve `Err` for genuinely
    /// malformed input that indicates a parser bug.
    fn parse(&self, ctx: &TargetCtx, responses: &Responses, out: &mut SampleSink) -> ParseResult;
}

pub type ParseResult = Result<(), ParseError>;

/// A parser could not make sense of data the host did return.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub source: SourceId,
    pub detail: String,
}

impl ParseError {
    pub fn new(source: &SourceId, detail: impl Into<String>) -> Self {
        ParseError { source: source.clone(), detail: detail.into() }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "source {} failed to parse: {}", self.source, self.detail)
    }
}

impl std::error::Error for ParseError {}
