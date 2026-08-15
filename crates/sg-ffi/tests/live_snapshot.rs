//! Drives the FFI exactly as a UI does — add a target, start it, poll snapshots — against the
//! containers in `fixtures/`.
//!
//!     ./fixtures/up.sh
//!     SG_REQUIRE_FIXTURES=1 cargo test -p sg-ffi
//!
//! This is the layer the macOS, Windows, Linux and Android apps all sit on, so it is worth
//! covering with the real stack rather than trusting that four UIs each got it right.

use std::time::Duration;

use sg_ffi::{ConnectionState, ServerGlass, TargetConfig};

const DEBIAN_PORT: u16 = 2222;

fn fixture_config(port: u16, refresh_ms: u64) -> TargetConfig {
    TargetConfig {
        host: "127.0.0.1".into(),
        port,
        user: "root".into(),
        auth_kind: "key".into(),
        key_path: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/id_test").to_string()),
        secret: None,
        // Fixture host keys are regenerated on every image build.
        host_key_policy: "accept_any".into(),
        refresh_ms,
    }
}

fn fixture_up(port: u16) -> bool {
    let up = std::net::TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_millis(500),
    )
    .is_ok();

    if !up {
        assert!(
            std::env::var("SG_REQUIRE_FIXTURES").is_err(),
            "SG_REQUIRE_FIXTURES is set but nothing is listening on 127.0.0.1:{port}"
        );
        eprintln!("SKIP: no fixture on 127.0.0.1:{port} (run ./fixtures/up.sh)");
    }
    up
}

/// Poll the way the UI's display timer does, until `predicate` holds or we give up.
fn poll_until(
    core: &ServerGlass,
    id: &str,
    timeout: Duration,
    predicate: impl Fn(&sg_ffi::TargetSnapshot) -> bool,
) -> sg_ffi::TargetSnapshot {
    let deadline = std::time::Instant::now() + timeout;
    let mut snapshot = core.snapshot(id.to_string()).expect("snapshot");
    while std::time::Instant::now() < deadline {
        snapshot = core.snapshot(id.to_string()).expect("snapshot");
        if predicate(&snapshot) {
            return snapshot;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    snapshot
}

#[test]
fn a_ui_polling_snapshots_sees_a_host_come_online_and_fill_in() {
    if !fixture_up(DEBIAN_PORT) {
        return;
    }

    let core = ServerGlass::new();
    let id = core.add_target(config_for_test());
    core.start(id.clone()).expect("start");

    let snapshot = poll_until(&core, &id, Duration::from_secs(20), |s| {
        s.state == ConnectionState::Online && !s.gauges.is_empty()
    });

    assert_eq!(
        snapshot.state,
        ConnectionState::Online,
        "never came online: {:?}",
        snapshot.state
    );
    assert!(
        snapshot.distro.contains("Debian"),
        "distro was {:?}",
        snapshot.distro
    );
    assert!(snapshot.cpu_count >= 1);
    assert!(!snapshot.display_name.is_empty());
    assert!(
        snapshot.source_errors.is_empty(),
        "collector errors: {:?}",
        snapshot.source_errors
    );

    // The very first thing the user sees must already be the complete grid, in final order.
    // Counter-derived tiles (CPU, network) need two readings to exist; if a partial grid were
    // published, CPU would appear one refresh later and shove every other tile sideways.
    let metrics: Vec<&str> = snapshot.gauges.iter().map(|g| g.metric.as_str()).collect();
    assert_eq!(
        metrics.first(),
        Some(&"cpu_usage"),
        "grid order changed: {metrics:?}"
    );
    for expected in [
        "cpu_usage",
        "mem_usage",
        "disk_usage",
        "uptime",
        "net_rx",
        "net_tx",
    ] {
        assert!(
            metrics.contains(&expected),
            "the first published grid is missing {expected}, so tiles will shift when it arrives: {metrics:?}"
        );
    }

    // Every gauge is renderable: a finite value, a usable label, and a suffix.
    for gauge in &snapshot.gauges {
        assert!(gauge.value.is_finite(), "{} is not finite", gauge.metric);
        assert!(!gauge.label.is_empty(), "{} has no label", gauge.metric);
        if let Some(max) = gauge.max {
            assert!(max > 0.0, "{} has a non-positive maximum", gauge.metric);
        }
    }

    // The entity tree the sidebar and cards navigate.
    let kinds: Vec<&str> = snapshot.entities.iter().map(|e| e.kind.as_str()).collect();
    assert!(kinds.contains(&"cpu"), "no CPU cores: {kinds:?}");
    assert!(kinds.contains(&"net"), "no interfaces: {kinds:?}");
    assert!(kinds.contains(&"fs"), "no filesystems: {kinds:?}");

    core.remove_target(id).expect("remove");
}

fn config_for_test() -> TargetConfig {
    fixture_config(DEBIAN_PORT, 500)
}

/// Sparklines need history, which only accumulates across refreshes.
#[test]
fn history_accumulates_so_sparklines_have_something_to_draw() {
    if !fixture_up(DEBIAN_PORT) {
        return;
    }

    let core = ServerGlass::new();
    let id = core.add_target(config_for_test());
    core.start(id.clone()).expect("start");

    let snapshot = poll_until(&core, &id, Duration::from_secs(25), |s| {
        s.gauges
            .iter()
            .any(|g| g.metric == "cpu_usage" && g.history.len() >= 3)
    });

    let cpu = snapshot
        .gauges
        .iter()
        .find(|g| g.metric == "cpu_usage")
        .expect("cpu_usage gauge");

    assert!(
        cpu.history.len() >= 3,
        "history did not accumulate: {:?}",
        cpu.history
    );
    assert_eq!(
        cpu.history.last().copied(),
        Some(cpu.value),
        "the gauge shows the newest point"
    );
    assert!(
        cpu.history.iter().all(|v| (0.0..=100.0).contains(v)),
        "CPU history out of range: {:?}",
        cpu.history
    );

    core.remove_target(id).expect("remove");
}

/// The batching guarantee, observed through the same surface the app's header displays.
#[test]
fn round_trips_grow_by_one_per_refresh() {
    if !fixture_up(DEBIAN_PORT) {
        return;
    }

    let core = ServerGlass::new();
    let id = core.add_target(fixture_config(DEBIAN_PORT, 500));
    core.start(id.clone()).expect("start");

    let first = poll_until(&core, &id, Duration::from_secs(20), |s| s.round_trips >= 3);
    assert!(first.round_trips >= 3, "no refreshes completed");

    let started = std::time::Instant::now();
    let baseline = first.round_trips;
    std::thread::sleep(Duration::from_secs(3));
    let later = core.snapshot(id.clone()).expect("snapshot");

    let refreshes = later.round_trips - baseline;
    let elapsed = started.elapsed().as_secs_f64();
    // At a 500ms refresh, ~6 in 3 seconds. One round trip each, not one per collector — with
    // seven collectors a per-request implementation would be seven times this.
    assert!(
        refreshes as f64 <= elapsed / 0.5 + 2.0,
        "{refreshes} round trips in {elapsed:.1}s is more than one per refresh"
    );
    assert!(
        refreshes >= 2,
        "only {refreshes} refreshes in {elapsed:.1}s"
    );

    core.remove_target(id).expect("remove");
}

/// Bad credentials must stop, not retry forever.
#[test]
fn unrecoverable_failures_are_reported_and_not_retried() {
    if !fixture_up(DEBIAN_PORT) {
        return;
    }

    let core = ServerGlass::new();
    let mut config = fixture_config(DEBIAN_PORT, 500);
    config.auth_kind = "password".into();
    config.secret = Some("definitely-not-the-password".into());

    let id = core.add_target(config);
    core.start(id.clone()).expect("start");

    let snapshot = poll_until(&core, &id, Duration::from_secs(15), |s| {
        matches!(s.state, ConnectionState::Failed { .. })
    });

    match snapshot.state {
        ConnectionState::Failed { recoverable, .. } => {
            assert!(
                !recoverable,
                "auth failure marked retryable would loop forever"
            );
        }
        other => panic!("expected a failure state, got {other:?}"),
    }

    core.remove_target(id).expect("remove");
}
