//! ServerGlass for Linux.
//!
//! A GTK4 front-end that links the Rust core directly — no UniFFI, no generated bindings, no
//! second copy of anything. The parsing, scheduling, rate derivation, health verdicts, number
//! formatting and plain-language wording are the same code the Mac and the phone run; what is
//! written here is a window.
//!
//! The modules are a library as well as a binary so the parts that do not need a display — the
//! inventory, and the bridge to the core — can be driven by a test against a real host.

pub mod command;
pub mod dialogs;
pub mod engine;
pub mod pairing;
pub mod palette;
pub mod store;
pub mod views;
pub mod widgets;
pub mod window;
