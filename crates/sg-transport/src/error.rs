use std::path::PathBuf;

/// Anything that can go wrong reaching or talking to a monitored host.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("could not reach {host}:{port}: {source}")]
    Connect {
        host: String,
        port: u16,
        #[source]
        source: std::io::Error,
    },

    #[error("host key for {host}:{port} is not in known_hosts")]
    UnknownHostKey {
        host: String,
        port: u16,
        fingerprint: String,
    },

    #[error("host key for {host}:{port} CHANGED — possible interception (now {fingerprint})")]
    HostKeyMismatch {
        host: String,
        port: u16,
        fingerprint: String,
    },

    #[error("authentication failed for {user}@{host}")]
    AuthFailed { user: String, host: String },

    #[error("could not read private key {path}: {detail}")]
    KeyFile { path: PathBuf, detail: String },

    #[error("could not read the pasted private key: {detail}")]
    KeyText { detail: String },

    #[error("no ssh-agent available: {0}")]
    NoAgent(String),

    #[error("ssh-agent holds no usable identity for {user}@{host}")]
    AgentNoIdentity { user: String, host: String },

    #[error("the remote shell did not start")]
    ShellNotStarted,

    #[error("connection closed by the remote host")]
    Closed,

    #[error("timed out after {ms}ms waiting for {what}")]
    Timeout { what: &'static str, ms: u64 },

    #[error("ssh protocol error: {0}")]
    Ssh(#[from] russh::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl TransportError {
    /// Whether reconnecting could plausibly succeed.
    ///
    /// The scheduler retries with backoff on transient faults but stops on the rest, because
    /// hammering a host that rejected our credentials is how accounts get locked out — and a
    /// changed host key must reach the user rather than being retried past.
    pub fn is_transient(&self) -> bool {
        match self {
            TransportError::Connect { .. }
            | TransportError::Closed
            | TransportError::Timeout { .. }
            | TransportError::ShellNotStarted
            | TransportError::Io(_)
            | TransportError::Ssh(_) => true,

            TransportError::AuthFailed { .. }
            | TransportError::UnknownHostKey { .. }
            | TransportError::HostKeyMismatch { .. }
            | TransportError::KeyFile { .. }
            | TransportError::KeyText { .. }
            | TransportError::NoAgent(_)
            | TransportError::AgentNoIdentity { .. } => false,
        }
    }
}

pub type Result<T> = std::result::Result<T, TransportError>;
