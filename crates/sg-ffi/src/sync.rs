//! Pairing, as the apps see it.
//!
//! Deliberately a small state machine with the user's confirmation in the middle. Every method
//! that moves data is separate from the method that produces the verification code, because the
//! whole security of the exchange rests on a person comparing two screens *before* anything is
//! sent. An API where transferring is one call cannot express that.

use std::sync::{Arc, Mutex};

use crate::ServerGlass;
use sg_sync::{merge, Offer, Payload, Session, SyncHost};
use tokio::net::TcpStream;

/// A host as it crosses between devices — no credential, by construction.
#[derive(Clone, Debug, uniffi::Record, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncHostView {
    pub address: String,
    pub port: u16,
    pub user: String,
    pub auth_kind: String,
    pub key_path: Option<String>,
    pub host_key_policy: String,
    pub refresh_ms: u64,
}

impl From<SyncHostView> for SyncHost {
    fn from(v: SyncHostView) -> Self {
        SyncHost {
            address: v.address,
            port: v.port,
            user: v.user,
            auth_kind: v.auth_kind,
            key_path: v.key_path,
            host_key_policy: v.host_key_policy,
            refresh_ms: v.refresh_ms,
        }
    }
}

impl From<SyncHost> for SyncHostView {
    fn from(h: SyncHost) -> Self {
        SyncHostView {
            address: h.address,
            port: h.port,
            user: h.user,
            auth_kind: h.auth_kind,
            key_path: h.key_path,
            host_key_policy: h.host_key_policy,
            refresh_ms: h.refresh_ms,
        }
    }
}

/// A pin the two devices disagree about. Never applied; shown to the user.
#[derive(Clone, Debug, uniffi::Record, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinConflictView {
    pub host: String,
    pub existing: String,
    pub incoming: String,
}

/// What a device is offering to send, or has received.
#[derive(Clone, Debug, uniffi::Record, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncBundle {
    pub hosts: Vec<SyncHostView>,
    pub known_hosts: Vec<String>,
}

impl From<SyncBundle> for Payload {
    fn from(b: SyncBundle) -> Self {
        Payload {
            hosts: b.hosts.into_iter().map(Into::into).collect(),
            known_hosts: b.known_hosts,
        }
    }
}

impl From<Payload> for SyncBundle {
    fn from(p: Payload) -> Self {
        SyncBundle {
            hosts: p.hosts.into_iter().map(Into::into).collect(),
            known_hosts: p.known_hosts,
        }
    }
}

/// The result of applying a received bundle to what this device already had.
#[derive(Clone, Debug, uniffi::Record, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeResult {
    pub hosts: Vec<SyncHostView>,
    pub known_hosts: Vec<String>,
    pub added_hosts: u32,
    pub kept_hosts: u32,
    pub added_pins: u32,
    /// Each of these needs a person. None were applied.
    pub conflicts: Vec<PinConflictView>,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum SyncError {
    #[error("{detail}")]
    Pairing { detail: String },
}

impl From<sg_sync::SyncError> for SyncError {
    fn from(e: sg_sync::SyncError) -> Self {
        SyncError::Pairing {
            detail: e.to_string(),
        }
    }
}

/// The device being set up: shows a code, waits, receives.
#[derive(uniffi::Object)]
pub struct SyncReceiver {
    runtime: tokio::runtime::Handle,
    offer_text: String,
    listener: Mutex<Option<sg_sync::Listener>>,
    connected: Mutex<Option<(Session, TcpStream)>>,
}

/// The device that already has the servers: scans a code, sends.
#[derive(uniffi::Object)]
pub struct SyncSender {
    runtime: tokio::runtime::Handle,
    connected: Mutex<Option<(Session, TcpStream)>>,
    code: String,
}

#[uniffi::export]
impl SyncReceiver {
    /// The text to render as a QR.
    pub fn pairing_code(&self) -> String {
        self.offer_text.clone()
    }

    /// Block until the other device connects, then return the code to show.
    ///
    /// Nothing has been received at this point — the caller shows this code, the user compares it
    /// with the other screen, and only then calls [`SyncReceiver::receive`].
    pub fn await_connection(&self) -> Result<String, SyncError> {
        let listener = self
            .listener
            .lock()
            .expect("listener lock")
            .take()
            .ok_or_else(|| SyncError::Pairing {
                detail: "this pairing has already been used".into(),
            })?;

        let (session, stream) = self.runtime.block_on(listener.accept())?;
        let code = session.verification_code();
        *self.connected.lock().expect("connection lock") = Some((session, stream));
        Ok(code)
    }

    /// Take the transfer. Call only after the user confirmed the codes match.
    pub fn receive(&self) -> Result<SyncBundle, SyncError> {
        let (session, mut stream) = self
            .connected
            .lock()
            .expect("connection lock")
            .take()
            .ok_or_else(|| SyncError::Pairing {
                detail: "no device is connected".into(),
            })?;

        let bytes = self
            .runtime
            .block_on(sg_sync::accept_transfer(&session, &mut stream))?;
        Ok(Payload::from_json(&bytes)?.into())
    }
}

#[uniffi::export]
impl SyncSender {
    /// The code to show. The user compares it with the other device's.
    pub fn verification_code(&self) -> String {
        self.code.clone()
    }

    /// Send the bundle. Call only after the user confirmed the codes match.
    pub fn send(&self, bundle: SyncBundle) -> Result<(), SyncError> {
        let (session, mut stream) = self
            .connected
            .lock()
            .expect("connection lock")
            .take()
            .ok_or_else(|| SyncError::Pairing {
                detail: "this transfer has already been sent".into(),
            })?;

        let payload: Payload = bundle.into();
        let bytes = payload.to_json()?;
        self.runtime.block_on(sg_sync::transfer::write_payload(
            &session,
            &mut stream,
            &bytes,
        ))?;
        Ok(())
    }
}

#[uniffi::export]
impl ServerGlass {
    /// Start offering this device as the destination for a transfer.
    ///
    /// `advertise_hosts` are every address this device might be reachable at. Pass all of them —
    /// the Wi-Fi address *and* the VPN address if a tunnel is up. Only the platform can enumerate
    /// its interfaces, and which address works depends on where the other device is: over
    /// WireGuard or Tailscale, the tunnel address is often the only one that reaches. The scanner
    /// tries each in turn, so an extra costs one failed connection and a missing one costs the
    /// pairing.
    pub fn start_receiving(
        &self,
        advertise_hosts: Vec<String>,
    ) -> Result<Arc<SyncReceiver>, SyncError> {
        let listener = self
            .runtime
            .block_on(sg_sync::Listener::bind(&advertise_hosts))?;
        let offer_text = listener.offer().encode();

        Ok(Arc::new(SyncReceiver {
            runtime: self.runtime.handle().clone(),
            offer_text,
            listener: Mutex::new(Some(listener)),
            connected: Mutex::new(None),
        }))
    }

    /// Connect to a scanned pairing code.
    ///
    /// Returns once the handshake is done and a code is available to compare. Nothing has been
    /// sent yet.
    pub fn scan_pairing_code(&self, code: String) -> Result<Arc<SyncSender>, SyncError> {
        let offer = Offer::decode(&code)?;
        let (session, stream) = self.runtime.block_on(sg_sync::send_transfer(&offer))?;
        let verification = session.verification_code();

        Ok(Arc::new(SyncSender {
            runtime: self.runtime.handle().clone(),
            connected: Mutex::new(Some((session, stream))),
            code: verification,
        }))
    }

    /// Apply a received bundle to what this device already has.
    ///
    /// Pure: it decides, it does not store. The caller writes the result to its own keystore and
    /// settings, and shows the conflicts.
    pub fn merge_bundle(&self, existing: SyncBundle, incoming: SyncBundle) -> MergeResult {
        let merged = merge(&existing.into(), &incoming.into());
        MergeResult {
            hosts: merged.hosts.into_iter().map(Into::into).collect(),
            known_hosts: merged.known_hosts,
            added_hosts: merged.added_hosts as u32,
            kept_hosts: merged.kept_hosts as u32,
            added_pins: merged.added_pins as u32,
            conflicts: merged
                .conflicts
                .into_iter()
                .map(|c| PinConflictView {
                    host: c.host,
                    existing: c.existing,
                    incoming: c.incoming,
                })
                .collect(),
        }
    }
}
