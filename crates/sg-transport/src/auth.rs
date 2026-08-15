//! Connection parameters: where to connect, as whom, and how to prove it.

use std::path::PathBuf;

/// How to authenticate.
///
/// Key material never lives in the transport for longer than a connection attempt. Whatever is
/// held between attempts is held by the platform's own keystore, and handed here per connection.
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
    /// A private key as text — what someone pastes on a phone.
    ///
    /// A phone has no user-visible filesystem and no ssh-agent, so a path is not a usable way to
    /// name a key there. Pasting the key body is, and it is the same key: this decodes the exact
    /// bytes `KeyFile` would have read.
    KeyText {
        key: String,
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
            // Deliberately says nothing about the key itself.
            Auth::KeyText { .. } => "pasted key".into(),
            Auth::Password(_) => "password".into(),
        }
    }
}

/// Where the record of trusted host keys lives.
///
/// A path rather than "wherever `~/.ssh` is", because on a phone there is no `~/.ssh` and no
/// `HOME` in the app's environment. The apps offered "remember its identity the first time you
/// connect", the write failed, and every later connection was another first connection — which
/// would have accepted a substituted key. The platform knows its own writable directory; the core
/// should be told, not guess.
pub type KnownHostsPath = Option<std::path::PathBuf>;

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
    /// Where trusted host keys are recorded. `None` means `~/.ssh/known_hosts`, which is right on
    /// a desktop and wrong in an app sandbox — see [`KnownHostsPath`].
    pub known_hosts: KnownHostsPath,
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
            known_hosts: None,
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

    /// Record trusted host keys here instead of `~/.ssh/known_hosts`.
    pub fn known_hosts(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.known_hosts = Some(path.into());
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
