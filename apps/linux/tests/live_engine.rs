//! The Linux app against a real host.
//!
//! Everything below the window: the inventory record, the config it builds, the core it links
//! directly, and the snapshot the views are handed. If this passes, the only thing between it and
//! a working app is the drawing — and the drawing is what a person can see is wrong.
//!
//! Needs the fixtures: `./fixtures/up.sh`. Without them the test skips itself, and under
//! `SG_REQUIRE_FIXTURES=1` a missing fixture is a failure instead — a skipped test reports `ok`,
//! which is how a suite quietly stops testing anything.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use sg_ffi::ConnectionState;
use sg_linux::engine::Engine;
use sg_linux::store::{Paths, SavedHost};
use sg_linux::widgets::Shape;

/// The Debian fixture's port.
///
/// Overridable because Windows reserves TCP port ranges for Hyper-V — 2180-2279 on at least one
/// machine, which swallows both fixture ports and makes Docker's publish fail outright. Check with
/// `netsh interface ipv4 show excludedportrange protocol=tcp` before concluding the fixture is
/// broken. See docs/WINDOWS.md.
fn fixture_port() -> u16 {
    std::env::var("SG_FIXTURE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(2222)
}

fn key_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/id_test")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("../../fixtures/id_test"))
}

/// Whether the fixture is answering, and what to do when it is not.
fn fixture_available(port: u16) -> bool {
    let reachable = std::net::TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().expect("address"),
        Duration::from_millis(500),
    )
    .is_ok();

    if !reachable && std::env::var("SG_REQUIRE_FIXTURES").is_ok() {
        panic!("no fixture on 127.0.0.1:{port} — run ./fixtures/up.sh");
    }
    reachable && key_path().exists()
}

fn fixture_host(port: u16) -> SavedHost {
    let mut host = SavedHost::new("127.0.0.1");
    host.port = port;
    host.user = "root".into();
    host.auth_kind = "key".into();
    host.key_path = Some(key_path().to_string_lossy().into_owned());
    // The fixture's host key is generated at image build time and thrown away with it, so there is
    // nothing to have pinned beforehand.
    host.host_key_policy = "accept_new".into();
    host.refresh_ms = 500;
    host
}

/// Poll the way the display timer does, until the core has something real to show.
fn wait_for_readings(engine: &Engine, host_id: &str) -> sg_ffi::TargetSnapshot {
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut last = String::new();

    while Instant::now() < deadline {
        if let Some(snapshot) = engine.snapshot(host_id) {
            if let ConnectionState::Failed { message, .. } = &snapshot.state {
                panic!("the fixture refused the connection: {message}");
            }
            // The first tick is withheld on purpose — a rate needs two readings — so waiting for
            // gauges rather than for Online is what the window is really waiting for.
            if !snapshot.gauges.is_empty() {
                return snapshot;
            }
            last = format!("{:?}", snapshot.state);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    panic!("no readings within 45s; last state was {last}");
}

#[test]
fn the_app_connects_to_a_real_host_and_is_handed_readings_to_draw() {
    let port = fixture_port();
    if !fixture_available(port) {
        eprintln!("skipping: no fixture on 127.0.0.1:{port}");
        return;
    }

    let directory = tempfile::tempdir().expect("temp dir");
    let paths = Paths::under(directory.path());
    let engine = Engine::new(&paths);
    let host = fixture_host(port);

    engine.start(&host, None).expect("start");
    let snapshot = wait_for_readings(&engine, &host.id);

    // A name the host called itself, not the address it was reached at.
    assert!(!snapshot.display_name.is_empty());
    assert!(!snapshot.distro.is_empty(), "no distro reported");
    assert!(snapshot.cpu_count > 0, "no cores reported");

    // The plain screen has something to say, and it is not still deciding.
    assert!(!snapshot.health.headline.is_empty());
    assert_ne!(snapshot.health.level, "checking");
    assert!(
        !snapshot.simple_tiles.is_empty(),
        "the default screen would be empty"
    );

    // The app's own known_hosts path was used, rather than the user's ~/.ssh.
    assert!(
        paths.known_hosts.exists(),
        "trusting the fixture recorded nothing at {}",
        paths.known_hosts.display()
    );
}

#[test]
fn a_refresh_costs_one_round_trip_however_many_collectors_ran() {
    let port = fixture_port();
    if !fixture_available(port) {
        eprintln!("skipping: no fixture on 127.0.0.1:{port}");
        return;
    }

    let directory = tempfile::tempdir().expect("temp dir");
    let paths = Paths::under(directory.path());
    let engine = Engine::new(&paths);
    let host = fixture_host(port);

    engine.start(&host, None).expect("start");
    let first = wait_for_readings(&engine, &host.id);

    // Let several more refreshes go by and check the count climbs by refreshes, not by collectors.
    std::thread::sleep(Duration::from_secs(3));
    let later = engine.snapshot(&host.id).expect("snapshot");

    let refreshes = later.round_trips.saturating_sub(first.round_trips);
    let collectors = later.gauges.len() + later.detail_groups.len();
    assert!(
        refreshes > 0,
        "the connection stopped refreshing after the first reading"
    );
    assert!(
        refreshes < collectors as u64,
        "{refreshes} round trips for {collectors} collector groups — the batching is gone"
    );
}

#[test]
fn the_widget_matches_the_metric_on_a_real_host() {
    let port = fixture_port();
    if !fixture_available(port) {
        eprintln!("skipping: no fixture on 127.0.0.1:{port}");
        return;
    }

    let directory = tempfile::tempdir().expect("temp dir");
    let paths = Paths::under(directory.path());
    let engine = Engine::new(&paths);
    let host = fixture_host(port);

    engine.start(&host, None).expect("start");
    let snapshot = wait_for_readings(&engine, &host.id);

    // Invariant 4, against readings a real kernel produced rather than a hand-written gauge.
    for gauge in &snapshot.gauges {
        match Shape::of(gauge) {
            Shape::Proportion => {
                assert_eq!(gauge.unit_suffix, "%");
                assert!(gauge.max.is_some());
            }
            Shape::Capacity => assert!(gauge.max.is_some()),
            Shape::Rate => assert!(
                gauge.max.is_none(),
                "{} would be drawn as a rate but has a maximum",
                gauge.metric
            ),
        }
    }

    // Something on this host is a percentage, or the check above proved nothing.
    assert!(
        snapshot
            .gauges
            .iter()
            .any(|g| matches!(Shape::of(g), Shape::Proportion)),
        "no proportional reading came back at all"
    );
}

#[test]
fn a_command_typed_by_the_user_runs_on_the_reading_connection() {
    let port = fixture_port();
    if !fixture_available(port) {
        eprintln!("skipping: no fixture on 127.0.0.1:{port}");
        return;
    }

    let directory = tempfile::tempdir().expect("temp dir");
    let paths = Paths::under(directory.path());
    let engine = Engine::new(&paths);
    let host = fixture_host(port);

    engine.start(&host, None).expect("start");
    wait_for_readings(&engine, &host.id);

    let target = engine.target_id(&host.id).expect("target");
    let result = engine
        .core()
        .run_command(target, "echo serverglass-linux".into())
        .expect("the command ran");

    assert!(result.output.contains("serverglass-linux"));
    assert_eq!(result.exit_code, 0);
}
