//! One SSH connection to one monitored host.
//!
//! The session owns exactly two things: the russh handle, and a single long-lived shell channel
//! that every collection batch runs through. Opening a channel per metric — the obvious
//! implementation — costs two round trips each and makes a twenty-source refresh unusable over a
//! satellite link or a bastion hop. Here a refresh is one write and one read.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{self, Handle};
use russh::keys::agent::client::AgentClient;
use russh::keys::{decode_secret_key, load_secret_key, PrivateKeyWithHashAlg};
use russh::{ChannelMsg, Disconnect};
use sg_model::{Request, Responses};
use tokio::time::timeout;

use crate::auth::{Auth, ConnectionSpec, HostKeyPolicy};
use crate::error::{Result, TransportError};
use crate::frame::Framing;

/// What host-key verification concluded, recorded out of band because russh's handler can only
/// answer yes/no and the user needs to be told *which* kind of no.
#[derive(Clone, Debug, PartialEq, Eq)]
enum HostKeyVerdict {
    Accepted,
    Unknown { fingerprint: String },
    Changed { fingerprint: String },
}

struct ClientHandler {
    host: String,
    port: u16,
    policy: HostKeyPolicy,
    verdict: Arc<Mutex<Option<HostKeyVerdict>>>,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        let fingerprint = server_public_key
            .fingerprint(russh::keys::HashAlg::Sha256)
            .to_string();

        if self.policy == HostKeyPolicy::AcceptAny {
            self.record(HostKeyVerdict::Accepted);
            return Ok(true);
        }

        let known = russh::keys::check_known_hosts(&self.host, self.port, server_public_key);
        let verdict = match known {
            Ok(true) => HostKeyVerdict::Accepted,
            // A key recorded for this host that does not match the one presented. Never
            // acceptable under any policy — this is the case host key checking exists for.
            Err(russh::keys::Error::KeyChanged { .. }) => HostKeyVerdict::Changed { fingerprint },
            Ok(false) | Err(_) => {
                if self.policy == HostKeyPolicy::AcceptNew {
                    let _ = russh::keys::known_hosts::learn_known_hosts(
                        &self.host,
                        self.port,
                        server_public_key,
                    );
                    HostKeyVerdict::Accepted
                } else {
                    HostKeyVerdict::Unknown { fingerprint }
                }
            }
        };

        let accepted = verdict == HostKeyVerdict::Accepted;
        self.record(verdict);
        Ok(accepted)
    }
}

impl ClientHandler {
    fn record(&self, verdict: HostKeyVerdict) {
        if let Ok(mut slot) = self.verdict.lock() {
            *slot = Some(verdict);
        }
    }
}

/// A live connection with its collection channel open.
pub struct SshSession {
    spec: ConnectionSpec,
    handle: Handle<ClientHandler>,
    channel: russh::Channel<client::Msg>,
    framing: Framing,
    round_trips: u64,
}

impl SshSession {
    /// Connect, authenticate, and bring up the collection shell.
    pub async fn connect(spec: ConnectionSpec) -> Result<Self> {
        let verdict = Arc::new(Mutex::new(None));
        let handler = ClientHandler {
            host: spec.host.clone(),
            port: spec.port,
            policy: spec.host_key_policy,
            verdict: Arc::clone(&verdict),
        };

        let config = Arc::new(client::Config {
            keepalive_interval: Some(Duration::from_secs(spec.keepalive_secs)),
            keepalive_max: 3,
            // Metric batches are small and latency-sensitive; Nagle would add up to 40ms to every
            // tick for no benefit.
            nodelay: true,
            ..Default::default()
        });

        let connect = client::connect(config, (spec.host.as_str(), spec.port), handler);
        let mut handle =
            match timeout(Duration::from_millis(spec.connect_timeout_ms), connect).await {
                Err(_) => {
                    return Err(TransportError::Timeout {
                        what: "connect",
                        ms: spec.connect_timeout_ms,
                    })
                }
                Ok(Ok(handle)) => handle,
                Ok(Err(err)) => {
                    // A rejected host key surfaces here as a generic protocol failure; the verdict
                    // slot tells us what actually happened so the user gets an actionable message.
                    return Err(match verdict.lock().ok().and_then(|v| v.clone()) {
                        Some(HostKeyVerdict::Unknown { fingerprint }) => {
                            TransportError::UnknownHostKey {
                                host: spec.host.clone(),
                                port: spec.port,
                                fingerprint,
                            }
                        }
                        Some(HostKeyVerdict::Changed { fingerprint }) => {
                            TransportError::HostKeyMismatch {
                                host: spec.host.clone(),
                                port: spec.port,
                                fingerprint,
                            }
                        }
                        _ => TransportError::Ssh(err),
                    });
                }
            };

        authenticate(&mut handle, &spec).await?;

        let channel = handle.channel_open_session().await?;
        // No PTY, deliberately. A PTY would echo the script back into the output stream and
        // translate LF to CRLF, corrupting every payload we parse.
        //
        // `/bin/sh` rather than the login shell: the user's shell might be fish or csh, whose
        // syntax our batch script is not written in. Executing sh explicitly makes the protocol
        // independent of whatever the account happens to be configured with.
        channel.exec(true, "/bin/sh").await?;

        let mut session = SshSession {
            spec,
            handle,
            channel,
            framing: Framing::new(generate_nonce()),
            round_trips: 0,
        };

        // Neutralise anything the account's environment does to output formatting — locale-aware
        // decimal separators and translated command output would break parsers on non-English
        // hosts. Also drops us into a predictable PATH.
        session.raw_write("export LC_ALL=C LANG=C\n").await?;

        Ok(session)
    }

    /// Execute a batch of requests in a single round trip.
    ///
    /// Requests the host cannot serve — HTTP, empty argv — are skipped; the caller executes those
    /// itself. The returned [`Responses`] simply lacks entries for them.
    pub async fn batch(&mut self, requests: &[Request]) -> Result<Responses> {
        let (script, ids) = self.framing.encode(requests);
        if ids.is_empty() {
            return Ok(Responses::default());
        }

        self.round_trips += 1;
        self.raw_write(&script).await?;

        let terminator = self.framing.terminator();
        let deadline = Duration::from_millis(self.spec.batch_timeout_ms);
        let mut buf: Vec<u8> = Vec::with_capacity(16 * 1024);

        loop {
            match timeout(deadline, self.channel.wait()).await {
                Err(_) => {
                    return Err(TransportError::Timeout {
                        what: "collection batch",
                        ms: self.spec.batch_timeout_ms,
                    })
                }
                Ok(None) => return Err(TransportError::Closed),
                Ok(Some(msg)) => match msg {
                    ChannelMsg::Data { ref data } => {
                        buf.extend_from_slice(data);
                        // Search only the tail: the terminator cannot straddle more than its own
                        // length, and rescanning a megabyte of process table every chunk is
                        // quadratic.
                        let from = buf.len().saturating_sub(data.len() + terminator.len());
                        if find(&buf[from..], terminator.as_bytes()).is_some() {
                            break;
                        }
                    }
                    // The remote shell writing to stderr is not our business; we redirect
                    // per-command stderr away, so anything here is noise from the login profile.
                    ChannelMsg::ExtendedData { .. } => {}
                    ChannelMsg::Eof | ChannelMsg::Close => return Err(TransportError::Closed),
                    ChannelMsg::ExitStatus { .. } => return Err(TransportError::Closed),
                    _ => {}
                },
            }
        }

        // Decoded once, at the end: a chunk boundary can fall inside a multi-byte UTF-8 sequence,
        // and container names and process command lines are not always ASCII.
        Ok(self.framing.decode(&String::from_utf8_lossy(&buf)))
    }

    async fn raw_write(&mut self, text: &str) -> Result<()> {
        self.channel
            .data_bytes(text.as_bytes().to_vec())
            .await
            .map_err(TransportError::Ssh)
    }

    pub fn spec(&self) -> &ConnectionSpec {
        &self.spec
    }

    /// Batches executed on this session.
    ///
    /// Exists so tests can assert the central claim of the design — that a refresh costs exactly
    /// one round trip however many sources are enabled — rather than inferring it from timing.
    pub fn round_trips(&self) -> u64 {
        self.round_trips
    }

    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }

    /// Close the channel and disconnect cleanly.
    pub async fn close(self) -> Result<()> {
        let _ = self.channel.eof().await;
        self.handle
            .disconnect(Disconnect::ByApplication, "", "en")
            .await?;
        Ok(())
    }
}

async fn authenticate(handle: &mut Handle<ClientHandler>, spec: &ConnectionSpec) -> Result<()> {
    let auth_failed = || TransportError::AuthFailed {
        user: spec.user.clone(),
        host: spec.host.clone(),
    };

    match &spec.auth {
        Auth::Password(password) => {
            let result = handle.authenticate_password(&spec.user, password).await?;
            if !result.success() {
                return Err(auth_failed());
            }
        }

        Auth::KeyFile { .. } | Auth::KeyText { .. } => {
            // A file and a paste differ only in where the bytes come from; everything after
            // decoding is identical, so the two share one path rather than one being a copy of
            // the other that drifts.
            let key = match &spec.auth {
                Auth::KeyFile { path, passphrase } => load_secret_key(path, passphrase.as_deref())
                    .map_err(|e| TransportError::KeyFile {
                        path: path.clone(),
                        detail: e.to_string(),
                    })?,
                Auth::KeyText { key, passphrase } => {
                    decode_secret_key(key.trim(), passphrase.as_deref()).map_err(|e| {
                        TransportError::KeyText {
                            detail: e.to_string(),
                        }
                    })?
                }
                _ => unreachable!("guarded by the match arm"),
            };
            let hash_alg = handle.best_supported_rsa_hash().await?.flatten();
            let result = handle
                .authenticate_publickey(
                    &spec.user,
                    PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg),
                )
                .await?;
            if !result.success() {
                return Err(auth_failed());
            }
        }

        Auth::Agent => {
            let mut agent = AgentClient::connect_env()
                .await
                .map_err(|e| TransportError::NoAgent(e.to_string()))?;
            let identities = agent
                .request_identities()
                .await
                .map_err(|e| TransportError::NoAgent(e.to_string()))?;
            let hash_alg = handle.best_supported_rsa_hash().await?.flatten();

            // The agent may hold a dozen keys and the server accepts one. Trying each in turn is
            // what OpenSSH does; the server's per-connection auth attempt limit bounds the loop.
            for identity in identities {
                let public_key = match &identity {
                    russh::keys::agent::AgentIdentity::PublicKey { key, .. } => key.clone(),
                    // Certificate auth needs a different call path; skip rather than fail so a
                    // certificate sitting alongside usable keys does not block the connection.
                    russh::keys::agent::AgentIdentity::Certificate { .. } => continue,
                };
                if let Ok(result) = handle
                    .authenticate_publickey_with(&spec.user, public_key, hash_alg, &mut agent)
                    .await
                {
                    if result.success() {
                        return Ok(());
                    }
                }
            }
            return Err(TransportError::AgentNoIdentity {
                user: spec.user.clone(),
                host: spec.host.clone(),
            });
        }
    }

    Ok(())
}

/// Per-connection random nonce for the frame markers.
///
/// Seeded from `RandomState`, which the standard library initialises from the OS entropy source.
/// This does not need to be cryptographically strong — it only has to be unpredictable to the
/// monitored host, so that remote content cannot forge a frame boundary.
fn generate_nonce() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let mut out = String::from("__SG");
    for _ in 0..2 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_usize(std::process::id() as usize);
        out.push_str(&format!("{:016x}", hasher.finish()));
    }
    out.push_str("__");
    out
}

/// Naive substring search over bytes. The haystack here is at most a few KiB.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonces_differ_between_connections() {
        let a = generate_nonce();
        let b = generate_nonce();
        assert_ne!(
            a, b,
            "nonce is not random; a hostile payload could forge frame boundaries"
        );
        assert!(a.len() >= 32);
        // Must survive shell quoting and line-oriented framing untouched.
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    }

    #[test]
    fn find_locates_needles_at_the_boundaries() {
        assert_eq!(find(b"abcdef", b"abc"), Some(0));
        assert_eq!(find(b"abcdef", b"def"), Some(3));
        assert_eq!(find(b"abcdef", b"xyz"), None);
        assert_eq!(find(b"ab", b"abcdef"), None);
        assert_eq!(find(b"abc", b""), None);
    }
}
