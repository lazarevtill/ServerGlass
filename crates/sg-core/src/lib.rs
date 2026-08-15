//! The ServerGlass core.
//!
//! Everything the app knows how to do lives here; the four UIs are view layers that send commands
//! and render events. That split is what makes a native SwiftUI app, a WinUI 3 app, a GTK4 app and
//! a Jetpack Compose app viable at once — they share every line of parsing, scheduling and
//! connection handling, and differ only where they should.
//!
//! A refresh is one round trip:
//!
//! ```text
//!   Collector::requests   every enabled source declares what it needs, merged and deduplicated
//!          |
//!   SshSession::batch     one script over one long-lived channel, one framed reply
//!          |
//!   Collector::collect    every source parses the same responses
//!          |
//!   RateEngine            counters become rates using measured elapsed time
//!          |
//!   LiveStore             bounded in-memory window; nothing is ever written to disk
//! ```
//!
//! ServerGlass is live-only by design. The store holds a rolling window so charts have something
//! to draw, and samples leave through a sink if one is configured; retention and alerting belong
//! to whatever system consumes them.

pub mod collector;
pub mod event;
pub mod rate;
pub mod runtime;
pub mod store;

pub use collector::{Collector, Tick};
pub use event::{Event, TargetState};
pub use rate::RateEngine;
pub use runtime::{backoff_for, TargetRuntime, TickSummary};
pub use store::{LiveStore, Point, DEFAULT_WINDOW};

/// Every built-in collector, ready to hand to a [`TargetRuntime`].
pub fn default_sources() -> Vec<Box<dyn sg_model::Source>> {
    sg_collect::builtin_sources()
}
