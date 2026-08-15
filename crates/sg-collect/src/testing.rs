//! Corpus-driven test helpers.
//!
//! Parsers are tested against real `/proc` text captured from the containers in `fixtures/` rather
//! than against hand-written strings. Hand-written fixtures encode what the author *believed* the
//! format was, which is precisely the belief a parser bug consists of.
//!
//! Refresh the corpora with `fixtures/capture.sh` after changing the fixture images.

use std::collections::BTreeSet;
use std::path::PathBuf;

use sg_model::{
    Capabilities, CgroupVersion, Coreutils, Entity, Request, Response, Responses, SampleSink,
    Source, TargetCtx, TargetId,
};

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/proc-corpus")
}

/// Map a remote path to its captured filename: `/proc/net/dev` -> `net-dev`.
fn corpus_name(path: &str) -> String {
    path.trim_start_matches("/proc/").trim_start_matches('/').replace('/', "-")
}

fn read(host: &str, name: &str) -> String {
    let path = corpus_root().join(host).join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("missing corpus {}: {e}\nrun fixtures/capture.sh to regenerate", path.display())
    })
}

/// Assemble a [`TargetCtx`] and [`Responses`] from captured host output.
pub struct CorpusBuilder {
    host: String,
    responses: Responses,
    paths: BTreeSet<String>,
    binaries: BTreeSet<String>,
    coreutils: Coreutils,
}

/// Start building against one captured host: `"debian"` (GNU) or `"alpine"` (BusyBox).
pub fn corpus(host: &str) -> CorpusBuilder {
    CorpusBuilder {
        host: host.to_string(),
        responses: Responses::default(),
        paths: BTreeSet::new(),
        binaries: BTreeSet::new(),
        coreutils: if host == "alpine" { Coreutils::Busybox } else { Coreutils::Gnu },
        }
}

impl CorpusBuilder {
    /// Serve a captured file for `path`.
    pub fn file(mut self, path: &str) -> Self {
        let body = read(&self.host, &corpus_name(path));
        self.responses.insert(Request::read(path).id(), Response::ok(body));
        self.paths.insert(path.to_string());
        self
    }

    /// Serve captured output for a command.
    pub fn exec(mut self, argv: &[&str], corpus_file: &str) -> Self {
        let body = read(&self.host, corpus_file);
        self.responses.insert(Request::exec(argv.iter().copied()).id(), Response::ok(body));
        if let Some(program) = argv.first() {
            self.binaries.insert((*program).to_string());
        }
        self
    }

    /// Serve a non-zero exit for `path`, as a host lacking that file would.
    pub fn missing(mut self, path: &str) -> Self {
        self.responses.insert(Request::read(path).id(), Response::failed(1));
        self
    }

    /// Serve literal text for `path`, for edge cases no real host produces on demand.
    pub fn literal(mut self, path: &str, body: &str) -> Self {
        self.responses.insert(Request::read(path).id(), Response::ok(body));
        self.paths.insert(path.to_string());
        self
    }

    pub fn build(self) -> (TargetCtx, Responses) {
        // Derive the core count from the captured /proc/stat when it is present, so CPU scaling
        // in tests matches the machine the corpus came from.
        let cpu_count = std::fs::read_to_string(corpus_root().join(&self.host).join("stat"))
            .map(|s| crate::cpu::parse_proc_stat(&s).cores.len() as u32)
            .unwrap_or(0);

        let caps = Capabilities {
            kernel: "6.1.0-test".into(),
            distro: self.host.clone(),
            arch: "aarch64".into(),
            hostname: format!("{}-fixture", self.host),
            coreutils: self.coreutils,
            cgroup: CgroupVersion::V2,
            cpu_count,
            clock_ticks: 100,
            page_size: 4096,
            binaries: self.binaries,
            paths: self.paths,
        };

        let ctx = TargetCtx {
            target: TargetId::new(self.host.clone()),
            host: Entity::host(format!("{}-fixture", self.host)),
            caps,
            interval_ms: 1000,
        };

        (ctx, self.responses)
    }
}

/// Run a source's parser and return what it produced.
pub fn sink_for(source: &dyn Source, ctx: &TargetCtx, responses: &Responses) -> SampleSink {
    let mut out = SampleSink::new(1_700_000_000_000);
    source.parse(ctx, responses, &mut out).expect("parser should not error on captured output");
    out
}

/// The numeric value of one sample, looked up by metric name.
pub fn value_of(out: &SampleSink, metric: &str) -> Option<f64> {
    let descriptor = out.descriptors.iter().find(|d| d.metric == metric)?;
    out.samples.iter().find(|s| s.series == descriptor.id)?.value.as_f64()
}

/// Every metric name the source emitted, sorted.
pub fn metrics(out: &SampleSink) -> Vec<String> {
    let mut names: Vec<_> = out.descriptors.iter().map(|d| d.metric.clone()).collect();
    names.sort();
    names.dedup();
    names
}

/// Both captured hosts, so a test can assert a parser works on GNU *and* BusyBox output.
pub const HOSTS: [&str; 2] = ["debian", "alpine"];
