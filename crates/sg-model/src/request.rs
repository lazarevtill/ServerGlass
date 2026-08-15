//! What a source asks for, and what comes back.
//!
//! Sources never perform I/O. They *declare* requests; the scheduler merges every active source's
//! requests into one batch, the transport executes that batch in a single round trip, and the
//! results are handed back for parsing. Three things fall out of that:
//!
//! - `/proc/stat` requested by three sources is fetched once.
//! - A refresh costs one round trip no matter how many sources are enabled.
//! - A WebAssembly plugin can implement the same trait as a built-in without being granted any
//!   I/O capability at all — it can only ask, and the host decides.

use std::collections::HashMap;

/// A unit of data a source needs this tick.
#[derive(Clone, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Request {
    /// Read a file whole. The overwhelmingly common case: `/proc/stat`, `/proc/meminfo`.
    ReadFile { path: String },
    /// List a directory's entries, one per line.
    ReadDir { path: String },
    /// Run a program already present on the host.
    ///
    /// `argv` is quoted by the transport before it reaches the remote shell, so values
    /// interpolated from container names or user input cannot break out into shell syntax.
    Exec { argv: Vec<String> },
    /// An HTTP request made *from the app*, not from the monitored host. Used by external checks
    /// and Prometheus scraping.
    Http {
        url: String,
        #[serde(default)]
        method: String,
        #[serde(default)]
        headers: Vec<(String, String)>,
    },
}

impl Request {
    pub fn read(path: impl Into<String>) -> Self {
        Request::ReadFile { path: path.into() }
    }

    pub fn read_dir(path: impl Into<String>) -> Self {
        Request::ReadDir { path: path.into() }
    }

    pub fn exec<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Request::Exec { argv: argv.into_iter().map(Into::into).collect() }
    }

    pub fn get(url: impl Into<String>) -> Self {
        Request::Http { url: url.into(), method: "GET".into(), headers: Vec::new() }
    }

    /// Whether this request is executed against the monitored host (versus from the app itself).
    pub fn is_remote(&self) -> bool {
        !matches!(self, Request::Http { .. })
    }

    /// Short, stable, collision-resistant id. Doubles as the dedup key and as the frame marker in
    /// the wire protocol, so it must contain no whitespace or shell metacharacters.
    ///
    /// ```
    /// # use sg_model::Request;
    /// assert_eq!(Request::read("/proc/stat").id(), Request::read("/proc/stat").id());
    /// assert_ne!(Request::read("/proc/stat").id(), Request::read("/proc/meminfo").id());
    /// ```
    pub fn id(&self) -> String {
        // FNV-1a over a canonical encoding. Not cryptographic — this only has to avoid accidental
        // collisions within a single tick's request set, which numbers in the dozens.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |bytes: &[u8]| {
            for &b in bytes {
                hash ^= b as u64;
                hash = hash.wrapping_mul(0x1000_0000_01b3);
            }
        };
        match self {
            Request::ReadFile { path } => {
                feed(b"f\0");
                feed(path.as_bytes());
            }
            Request::ReadDir { path } => {
                feed(b"d\0");
                feed(path.as_bytes());
            }
            Request::Exec { argv } => {
                feed(b"x\0");
                for arg in argv {
                    feed(arg.as_bytes());
                    feed(b"\0");
                }
            }
            Request::Http { url, method, headers } => {
                feed(b"h\0");
                feed(method.as_bytes());
                feed(url.as_bytes());
                for (k, v) in headers {
                    feed(k.as_bytes());
                    feed(v.as_bytes());
                }
            }
        }
        format!("{hash:016x}")
    }
}

/// The result of executing one [`Request`].
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Response {
    /// 0 on success. Missing `/proc` files and absent binaries surface here rather than as errors,
    /// because "this host has no `/proc/pressure`" is normal, not exceptional.
    pub exit_code: i32,
    pub body: String,
}

impl Response {
    pub fn ok(body: impl Into<String>) -> Self {
        Response { exit_code: 0, body: body.into() }
    }

    pub fn failed(exit_code: i32) -> Self {
        Response { exit_code, body: String::new() }
    }

    pub fn is_ok(&self) -> bool {
        self.exit_code == 0
    }
}

/// Everything the batch returned, keyed by [`Request::id`].
#[derive(Clone, Default, Debug, serde::Serialize, serde::Deserialize)]
pub struct Responses(HashMap<String, Response>);

impl Responses {
    pub fn insert(&mut self, request_id: impl Into<String>, response: Response) {
        self.0.insert(request_id.into(), response);
    }

    pub fn get(&self, request: &Request) -> Option<&Response> {
        self.0.get(&request.id())
    }

    /// The body of a *successful* request.
    ///
    /// Returning `None` for a non-zero exit is what lets parsers be written as a chain of
    /// `let Some(text) = r.text(&req) else { return Ok(()) };` — a source silently produces
    /// nothing on hosts where its data does not exist, which is the desired behaviour.
    pub fn text(&self, request: &Request) -> Option<&str> {
        self.0.get(&request.id()).filter(|r| r.is_ok()).map(|r| r.body.as_str())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl FromIterator<(String, Response)> for Responses {
    fn from_iter<T: IntoIterator<Item = (String, Response)>>(iter: T) -> Self {
        Responses(iter.into_iter().collect())
    }
}
