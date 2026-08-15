//! Domain types for ServerGlass.
//!
//! This crate is deliberately inert: no I/O, no async runtime, no dependency beyond `serde`. It
//! defines the vocabulary that the transport, the collectors, the scheduler, the plugin host and
//! all four UIs agree on.
//!
//! The shape of the whole system follows from one trait, [`Source`], and one split inside it:
//! a source *declares* what it needs ([`Source::requests`]) and separately *parses* what came back
//! ([`Source::parse`]). Because no source performs its own I/O, the scheduler can merge every
//! active source's requests into a single batch and satisfy a full refresh in one network round
//! trip — which is what makes agentless monitoring feel responsive over a high-latency link.
//!
//! ```
//! use sg_model::*;
//!
//! struct Uptime(SourceDescriptor);
//!
//! impl Source for Uptime {
//!     fn descriptor(&self) -> &SourceDescriptor { &self.0 }
//!
//!     fn requests(&self, _ctx: &TargetCtx) -> Vec<Request> {
//!         vec![Request::read("/proc/uptime")]
//!     }
//!
//!     fn parse(&self, ctx: &TargetCtx, r: &Responses, out: &mut SampleSink) -> ParseResult {
//!         let Some(text) = r.text(&Request::read("/proc/uptime")) else { return Ok(()) };
//!         let secs: f64 = text.split_whitespace().next().unwrap_or("0").parse().unwrap_or(0.0);
//!         out.emit(
//!             SeriesDescriptor::gauge(
//!                 &self.0.id, &ctx.host.id, "uptime", "Uptime", Unit::Seconds,
//!             ),
//!             secs,
//!         );
//!         Ok(())
//!     }
//! }
//! ```

mod caps;
mod entity;
mod ids;
mod request;
mod sample;
mod series;
mod source;
mod unit;

pub use caps::{Capabilities, CgroupVersion, Coreutils, Requirements};
pub use entity::{Entity, EntityKind};
pub use ids::{EntityId, SeriesId, SourceId, TargetId};
pub use request::{Request, Response, Responses};
pub use sample::{Sample, SampleSink};
pub use series::{SeriesDescriptor, SeriesKind, Value};
pub use source::{ParseError, ParseResult, Source, SourceDescriptor, TargetCtx};
pub use unit::Unit;

/// Current wall-clock time in Unix milliseconds.
///
/// The single place the model reaches for a clock. Sources receive the tick timestamp through
/// [`SampleSink::at_ms`] and should not call this themselves — a tick's samples must share one
/// timestamp to line up on the time axis.
pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
