//! SSH transport for ServerGlass.
//!
//! Agentless monitoring means every reading is the text output of a command run over SSH. Done
//! naively — one `exec` channel per metric — a refresh costs two round trips per source, and a
//! twenty-source dashboard becomes unusable the moment the host is not on the local network.
//!
//! This crate does it in one round trip instead:
//!
//! 1. [`SshSession::connect`] opens **one** channel and runs `/bin/sh` on it, keeping it alive for
//!    the life of the connection.
//! 2. [`SshSession::batch`] compiles a whole tick's [`Request`](sg_model::Request)s into a single
//!    script, writes it, and reads back one framed reply.
//! 3. [`frame::Framing`] wraps each request's output in nonce-bearing markers so payloads cannot
//!    forge frame boundaries, and duplicate requests collapse to one execution.
//!
//! Nothing is ever installed or written on the monitored host. Requests read files, list
//! directories, or run programs that are already present — a constraint enforced by the shape of
//! [`Request`](sg_model::Request) itself, which offers no way to express a write.

pub mod auth;
pub mod error;
pub mod frame;
pub mod probe;
pub mod quote;
pub mod session;

pub use auth::{Auth, ConnectionSpec, HostKeyPolicy};
pub use error::{Result, TransportError};
pub use frame::Framing;
pub use quote::{shell_join, shell_quote};
pub use session::SshSession;
