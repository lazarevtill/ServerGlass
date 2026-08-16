//! Moving a host inventory from one device to another.
//!
//! What travels: the list of servers, and the record of host keys already trusted. What never
//! travels: passwords, key passphrases, pasted private keys. That asymmetry is the whole design.
//!
//! - **Syncing credentials makes a person less safe.** Every device and every hop is another place
//!   the key can leak from, and the blast radius is every server on the list.
//! - **Syncing host key pins makes them safer.** A pin learned on a laptop protects the phone,
//!   which would otherwise trust-on-first-use again — and a phone on a hostile network is the
//!   likeliest place to meet an impostor.
//!
//! So a paired device receives the inventory and asks for each credential once, storing it in its
//! own keystore. See `docs/SYNC.md` for the research this follows.

pub mod merge;
pub mod pairing;
pub mod payload;
pub mod transfer;

pub use merge::{merge, Merge, PinConflict};
pub use pairing::{Handshake, Offer, Session, OFFER_TTL_SECS, VERSION};
pub use payload::{Payload, SyncHost};
pub use transfer::{accept_transfer, send_transfer, Listener};

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("{0}")]
    Malformed(String),
    #[error("this pairing code is from a newer version of ServerGlass (v{0})")]
    Version(String),
    #[error("{0}")]
    Handshake(String),
    #[error("{0}")]
    Crypto(String),
    #[error("{0}")]
    Transfer(String),
}
