//! Connection parameters: where to connect, as whom, and how to prove it.

use std::path::PathBuf;

/// How to authenticate.
///
/// v0 deliberately has no credential vault. Keys are referenced by path or fetched from the
/// running `ssh-agent`, so ServerGlass never holds long-lived secrets — the passphrase field is
/// short-lived and used only to decrypt a key file in memory.
#[derive(Clone, Debug)]
pub enum Auth {
    /// Delegate to the running `ssh-agent` (or Pageant on Windows). The best default: the app
    /// never sees key material at all.
    Agent,
    /// A private key file, optionally passphrase-protected.
    KeyFile {
        path: PathBuf,
        passphrase: Option<String>,
    },
    /// Password authentication, for hosts that allow nothing else.
    Password(String),
}

impl Auth {
    /// Redacted description for logs and the UI. Never renders secret material.
    pub fn describe(&self) -> String {
        match self {
            Auth::Agent => "ssh-agent".into(),
            Auth::KeyFile { path, .. } => format!("key {}", path.display()),
            Auth::Password(_) => "password".into(),
        }
    }
}

/// Host key verification policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HostKeyPolicy {
    /// Require the key to be present and matching in `~/.ssh/known_hosts`. An unknown host is an
    /// error the user must resolve explicitly, and a changed key always is.
    #[default]
    Strict,
    /// Accept and record an unknown host on first sight, but still reject a *changed* key.
    /// Equivalent to OpenSSH's `StrictHostKeyChecking=accept-new`.
    AcceptNew,
    /// Accept anything. Only for the throwaway containers in `fixtures/`, whose keys are
    /// regenerated on every `docker compose up`.
    AcceptAny,
}

/// Everything needed to open a session.
#[derive(Clone, Debug)]
pub struct ConnectionSpec {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: Auth,
    pub host_key_policy: HostKeyPolicy,
    /// How long to wait for the TCP connect and handshake.
    pub connect_timeout_ms: u64,
    /// How long a single collection batch may take before the tick is abandoned.
    pub batch_timeout_ms: u64,
    /// Idle interval after which a keepalive is sent. This is what stops a NAT or a bastion from
    /// silently dropping an idle monitoring session.
    pub keepalive_secs: u64,
}

impl ConnectionSpec {
    pub fn new(host: impl Into<String>, user: impl Into<String>) -> Self {
        ConnectionSpec {
            host: host.into(),
            port: 22,
            user: user.into(),
            auth: Auth::Agent,
            host_key_policy: HostKeyPolicy::Strict,
            connect_timeout_ms: 15_000,
            batch_timeout_ms: 10_000,
            keepalive_secs: 30,
        }
    }

    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn auth(mut self, auth: Auth) -> Self {
        self.auth = auth;
        self
    }

    pub fn host_key_policy(mut self, policy: HostKeyPolicy) -> Self {
        self.host_key_policy = policy;
        self
    }

    /// `user@host:port`, for logs and window titles.
    pub fn endpoint(&self) -> String {
        if self.port == 22 {
            format!("{}@{}", self.user, self.host)
        } else {
            format!("{}@{}:{}", self.user, self.host, self.port)
        }
    }
}
