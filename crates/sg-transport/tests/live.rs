//! Live transport tests against the containers in `fixtures/`.
//!
//!     docker compose -f fixtures/compose.yml up -d --build
//!     cargo test -p sg-transport
//!
//! Each test skips itself when the fixture is not listening, so `cargo test` stays green on a
//! machine without Docker instead of failing for an unrelated reason.

use std::path::PathBuf;
use std::time::Duration;

use sg_model::Request;
use sg_transport::auth::{Auth, ConnectionSpec, HostKeyPolicy};
use sg_transport::{probe, SshSession};

const DEBIAN_PORT: u16 = 2222;
const ALPINE_PORT: u16 = 2223;

fn fixture_key() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/id_test")
}

fn spec(port: u16) -> ConnectionSpec {
    ConnectionSpec::new("127.0.0.1", "root")
        .port(port)
        .auth(Auth::KeyFile {
            path: fixture_key(),
            passphrase: None,
        })
        // Fixture host keys are regenerated on every image build.
        .host_key_policy(HostKeyPolicy::AcceptAny)
}

/// Whether a fixture is listening.
///
/// Set `SG_REQUIRE_FIXTURES=1` to turn "fixture missing" into a failure instead of a skip. CI sets
/// it, because a skipped test reports as `ok` and a whole suite can quietly stop testing anything
/// — which is exactly what happened when the Debian container died on a missing `/run/sshd`.
async fn fixture_up(port: u16) -> bool {
    let reachable = tokio::time::timeout(
        Duration::from_millis(500),
        tokio::net::TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .is_some();

    if !reachable {
        assert!(
            std::env::var("SG_REQUIRE_FIXTURES").is_err(),
            "SG_REQUIRE_FIXTURES is set but nothing is listening on 127.0.0.1:{port}"
        );
        eprintln!(
            "SKIP: no fixture on 127.0.0.1:{port} (docker compose -f fixtures/compose.yml up -d)"
        );
    }
    reachable
}

/// `None` when the fixture is not up, so callers can skip rather than fail.
async fn session(port: u16) -> Option<SshSession> {
    if !fixture_up(port).await {
        return None;
    }
    Some(
        SshSession::connect(spec(port))
            .await
            .expect("fixture connect"),
    )
}

macro_rules! session_or_skip {
    ($port:expr) => {
        match session($port).await {
            Some(s) => s,
            None => return,
        }
    };
}

#[tokio::test]
async fn reads_proc_files_over_one_channel() {
    let mut session = session_or_skip!(DEBIAN_PORT);

    let stat = Request::read("/proc/stat");
    let meminfo = Request::read("/proc/meminfo");
    let responses = session
        .batch(&[stat.clone(), meminfo.clone()])
        .await
        .unwrap();

    let stat_body = responses.text(&stat).expect("/proc/stat readable");
    assert!(
        stat_body.starts_with("cpu "),
        "unexpected /proc/stat: {stat_body:.60}"
    );
    assert!(responses.text(&meminfo).unwrap().contains("MemTotal:"));

    session.close().await.unwrap();
}

/// The central claim of the design: N requests, one round trip. Verified by timing — a per-request
/// channel would multiply the loopback handshake cost by the request count.
#[tokio::test]
async fn a_large_batch_costs_the_same_as_a_small_one() {
    let mut session = session_or_skip!(DEBIAN_PORT);

    let one = vec![Request::read("/proc/stat")];
    let many: Vec<_> = (0..40)
        .map(|i| Request::exec(["echo", &format!("payload-{i}")]))
        .chain([Request::read("/proc/stat")])
        .collect();

    // Warm the channel so neither measurement pays for lazily-negotiated state.
    session.batch(&one).await.unwrap();

    let t0 = std::time::Instant::now();
    session.batch(&one).await.unwrap();
    let small = t0.elapsed();

    let t1 = std::time::Instant::now();
    let responses = session.batch(&many).await.unwrap();
    let large = t1.elapsed();

    assert_eq!(
        responses.len(),
        41,
        "every request should have produced a frame"
    );
    assert_eq!(
        responses.text(&Request::exec(["echo", "payload-17"])),
        Some("payload-17\n")
    );

    // 41 sequential channel opens on loopback would be an order of magnitude worse than this.
    assert!(
        large < small + Duration::from_millis(250),
        "41-request batch took {large:?} vs {small:?} for one — batching is not working"
    );

    session.close().await.unwrap();
}

#[tokio::test]
async fn missing_files_report_exit_codes_rather_than_failing_the_batch() {
    let mut session = session_or_skip!(DEBIAN_PORT);

    let missing = Request::read("/proc/does-not-exist");
    let present = Request::read("/proc/uptime");
    let responses = session
        .batch(&[missing.clone(), present.clone()])
        .await
        .unwrap();

    assert_ne!(responses.get(&missing).unwrap().exit_code, 0);
    assert_eq!(responses.text(&missing), None);
    // The neighbouring request is unaffected — this is why a per-frame exit code exists.
    assert!(responses.text(&present).unwrap().contains('.'));

    session.close().await.unwrap();
}

/// Quoting is the boundary between a container name and a command injection.
#[tokio::test]
async fn hostile_arguments_reach_the_program_as_literal_text() {
    let mut session = session_or_skip!(DEBIAN_PORT);

    let hostile = "web-01; touch /tmp/pwned; echo $(id)";
    let echo = Request::exec(["echo", hostile]);
    let canary = Request::read("/tmp/pwned");

    let responses = session
        .batch(&[echo.clone(), canary.clone()])
        .await
        .unwrap();

    assert_eq!(responses.text(&echo), Some(format!("{hostile}\n").as_str()));
    assert_eq!(
        responses.text(&canary),
        None,
        "injected command executed — quoting is broken"
    );

    session.close().await.unwrap();
}

/// Payloads that mimic the frame protocol must not be able to truncate or forge a frame.
#[tokio::test]
async fn payloads_impersonating_the_protocol_are_harmless() {
    let mut session = session_or_skip!(DEBIAN_PORT);

    let forged = "__SG0000000000000000__E0000 0";
    let attack = Request::exec(["printf", "%s\\n%s\\n", forged, "still mine"]);
    let after = Request::read("/proc/uptime");

    let responses = session
        .batch(&[attack.clone(), after.clone()])
        .await
        .unwrap();

    let body = responses.text(&attack).expect("frame survived");
    assert!(body.contains(forged) && body.contains("still mine"));
    assert!(
        responses.text(&after).is_some(),
        "forged marker swallowed the following frame"
    );

    session.close().await.unwrap();
}

#[tokio::test]
async fn detects_capabilities_of_a_gnu_host() {
    let mut session = session_or_skip!(DEBIAN_PORT);

    let responses = session.batch(&probe::requests()).await.unwrap();
    let caps = probe::parse(&responses);

    assert!(!caps.kernel.is_empty(), "kernel not detected");
    assert!(
        caps.distro.contains("Debian"),
        "distro was {:?}",
        caps.distro
    );
    assert!(caps.cpu_count >= 1, "no CPUs detected");
    assert_eq!(caps.clock_ticks, 100);
    assert_eq!(caps.coreutils, sg_model::Coreutils::Gnu);
    assert!(
        caps.has("ss") && caps.has("ip"),
        "iproute2 not detected: {:?}",
        caps.binaries
    );
    assert!(
        !caps.has("nvidia-smi"),
        "detected a binary the image does not have"
    );
    assert!(caps.has_path("/proc/stat") && caps.has_path("/proc/diskstats"));

    session.close().await.unwrap();
}

#[tokio::test]
async fn detects_capabilities_of_a_busybox_host() {
    let mut session = session_or_skip!(ALPINE_PORT);

    let responses = session.batch(&probe::requests()).await.unwrap();
    let caps = probe::parse(&responses);

    assert!(
        caps.distro.contains("Alpine"),
        "distro was {:?}",
        caps.distro
    );
    assert_eq!(
        caps.coreutils,
        sg_model::Coreutils::Busybox,
        "BusyBox not detected; parsers would take GNU code paths"
    );
    assert!(caps.cpu_count >= 1);

    session.close().await.unwrap();
}

/// A wrong key must fail fast and permanently rather than being retried forever.
#[tokio::test]
async fn authentication_failure_is_not_transient() {
    if !fixture_up(DEBIAN_PORT).await {
        return;
    }

    let spec = spec(DEBIAN_PORT).auth(Auth::Password("definitely-not-the-password".into()));
    let err = match SshSession::connect(spec).await {
        Err(err) => err,
        Ok(_) => panic!("password auth should have been refused"),
    };

    assert!(
        !err.is_transient(),
        "auth failure would be retried in a loop: {err}"
    );
}

/// A pasted key must reach the same host the same key file reaches.
///
/// This is the whole claim of `Auth::KeyText`: on a phone there is no path to point at and no
/// agent to defer to, so the key arrives as text — and it has to be the *same* key, not a
/// second, subtly different code path that works on a laptop and fails on a phone.
#[tokio::test]
async fn a_pasted_key_authenticates_exactly_like_the_key_file() {
    if !fixture_up(DEBIAN_PORT).await {
        return;
    }
    let key = std::fs::read_to_string(fixture_key()).expect("read the fixture key");

    let spec = ConnectionSpec::new("127.0.0.1", "root")
        .port(DEBIAN_PORT)
        .auth(Auth::KeyText {
            key,
            passphrase: None,
        })
        .host_key_policy(HostKeyPolicy::AcceptAny);

    let mut session = SshSession::connect(spec).await.expect("pasted-key connect");
    let uptime = Request::read("/proc/uptime");
    let responses = session
        .batch(std::slice::from_ref(&uptime))
        .await
        .expect("read over the pasted-key session");
    assert!(responses
        .text(&uptime)
        .is_some_and(|t| !t.trim().is_empty()));
    session.close().await.unwrap();
}

/// Whitespace a paste picks up must not stop the key decoding.
///
/// Pasting from a chat client, a password manager or a terminal routinely adds a trailing newline
/// or leading spaces, and a key that "does not work when pasted" is indistinguishable from a
/// broken feature.
#[tokio::test]
async fn a_pasted_key_survives_the_whitespace_pasting_adds() {
    if !fixture_up(DEBIAN_PORT).await {
        return;
    }
    let key = std::fs::read_to_string(fixture_key()).expect("read the fixture key");
    let padded = format!("\n  \n{}\n\n  ", key.trim());

    let spec = ConnectionSpec::new("127.0.0.1", "root")
        .port(DEBIAN_PORT)
        .auth(Auth::KeyText {
            key: padded,
            passphrase: None,
        })
        .host_key_policy(HostKeyPolicy::AcceptAny);

    assert!(SshSession::connect(spec).await.is_ok());
}
