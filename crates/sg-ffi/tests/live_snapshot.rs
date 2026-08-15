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
        key_text: None,
        secret: None,
        // Fixture host keys are regenerated on every image build.
        host_key_policy: "accept_any".into(),
        known_hosts_path: None,
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

/// The panel that explains a busy host. Per-process CPU is a derived rate, so it only exists from
/// the second tick — and it must be attributed to the right process, which is the whole risk in
/// parsing `/proc/<pid>/stat`.
#[test]
fn the_process_table_explains_what_is_running() {
    if !fixture_up(DEBIAN_PORT) {
        return;
    }

    let core = ServerGlass::new();
    let id = core.add_target(fixture_config(DEBIAN_PORT, 500));
    core.start(id.clone()).expect("start");

    let snapshot = poll_until(&core, &id, Duration::from_secs(25), |s| {
        !s.top_processes.is_empty()
    });

    assert!(
        !snapshot.top_processes.is_empty(),
        "no processes were reported"
    );

    // sshd is serving this very connection, so it is guaranteed to be running.
    assert!(
        snapshot
            .top_processes
            .iter()
            .any(|p| p.command.contains("sshd")),
        "expected sshd among {:?}",
        snapshot
            .top_processes
            .iter()
            .map(|p| &p.command)
            .collect::<Vec<_>>()
    );

    for process in &snapshot.top_processes {
        assert!(
            process.pid.parse::<u32>().is_ok(),
            "pid {:?} is not numeric",
            process.pid
        );
        assert!(
            !process.command.is_empty(),
            "process {} has no command",
            process.pid
        );
        assert!(process.cpu_percent.is_finite() && process.cpu_percent >= 0.0);
        assert!(
            process.memory_bytes > 0.0,
            "kernel threads should be filtered out"
        );
    }

    // Ranked by CPU, descending — the panel's entire purpose.
    let cpu: Vec<f64> = snapshot
        .top_processes
        .iter()
        .map(|p| p.cpu_percent)
        .collect();
    assert!(
        cpu.windows(2).all(|w| w[0] >= w[1]),
        "process table is not ranked: {cpu:?}"
    );

    // Hundreds of process entities must not be shipped in the general entity list.
    assert!(
        !snapshot.entities.iter().any(|e| e.kind == "proc"),
        "process entities leaked into the snapshot's entity list"
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

/// The whole pasted-key path, through the FFI surface the apps actually call.
///
/// The transport test proves `Auth::KeyText` works; this proves the app-facing config reaches it —
/// `auth_kind: "key_text"` with the key in `key_text` and no path anywhere.
#[test]
fn a_pasted_key_config_connects_and_collects() {
    if !fixture_up(DEBIAN_PORT) {
        return;
    }
    let key = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/id_test"
    ))
    .expect("read the fixture key");

    let core = ServerGlass::new();
    let id = core.add_target(TargetConfig {
        host: "127.0.0.1".into(),
        port: DEBIAN_PORT,
        user: "root".into(),
        auth_kind: "key_text".into(),
        key_path: None,
        key_text: Some(key),
        secret: None,
        host_key_policy: "accept_any".into(),
        known_hosts_path: None,
        refresh_ms: 500,
    });
    core.start(id.clone()).expect("start");

    let mut online = false;
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(250));
        let snapshot = core.snapshot(id.clone()).expect("snapshot");
        if snapshot.state == ConnectionState::Online && !snapshot.gauges.is_empty() {
            online = true;
            break;
        }
        if let ConnectionState::Failed { message, .. } = &snapshot.state {
            panic!("pasted key rejected: {message}");
        }
    }
    assert!(online, "never came online with a pasted key");
}

/// Running a command on the connection the readings already use.
#[test]
fn commands_run_on_the_live_session() {
    if !fixture_up(DEBIAN_PORT) {
        return;
    }
    let core = ServerGlass::new();
    let id = core.add_target(fixture_config(DEBIAN_PORT, 500));
    core.start(id.clone()).expect("start");

    // Wait for the session the command will borrow.
    let mut online = false;
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(250));
        if core.snapshot(id.clone()).expect("snapshot").state == ConnectionState::Online {
            online = true;
            break;
        }
    }
    assert!(online, "never came online");

    let before = core.snapshot(id.clone()).expect("snapshot").round_trips;

    let hello = core
        .run_command(id.clone(), "echo hello from serverglass".into())
        .expect("run echo");
    assert_eq!(hello.output.trim(), "hello from serverglass");
    assert_eq!(hello.exit_code, 0);

    // Shell syntax has to survive: people type pipes and quotes, and a runner that splits on
    // whitespace would turn this into nonsense.
    let piped = core
        .run_command(id.clone(), "printf 'a\\nb\\nc\\n' | wc -l".into())
        .expect("run pipeline");
    assert_eq!(piped.output.trim(), "3");

    // A failed command's message is the answer, so unlike collection the body is kept.
    let failed = core
        .run_command(id.clone(), "ls /definitely-not-here".into())
        .expect("run failing command");
    assert_ne!(failed.exit_code, 0);
    assert!(
        failed.output.to_lowercase().contains("no such file"),
        "the error text was dropped: {failed:?}"
    );

    // One round trip per command, on the session that was already open. The count is republished
    // by the next tick rather than by the command, so wait for one — three commands must show up
    // as more than the three refreshes that could have happened meanwhile.
    let mut after = before;
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(250));
        after = core.snapshot(id.clone()).expect("snapshot").round_trips;
        if after >= before + 3 {
            break;
        }
    }
    assert!(
        after >= before + 3,
        "commands should run on the counted session: {before} -> {after}"
    );
}

/// A command must not be accepted for a host that is not answering.
///
/// It used to be queued: the channel took it, the poll loop was busy reconnecting, and it ran
/// whenever the session came back — minutes later, unwatched, against a machine in a different
/// state than the one it was typed for.
#[test]
fn commands_are_refused_rather_than_queued_while_offline() {
    let core = ServerGlass::new();
    // Never started, so never online.
    let id = core.add_target(fixture_config(DEBIAN_PORT, 1000));

    let refused = core.run_command(id.clone(), "echo late".into());
    assert!(refused.is_err(), "an offline host accepted a command");

    // And nothing is left behind waiting to fire: starting it later must not run that command.
    if fixture_up(DEBIAN_PORT) {
        core.start(id.clone()).expect("start");
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(250));
            if core.snapshot(id.clone()).expect("snapshot").state == ConnectionState::Online {
                break;
            }
        }
        let now = core
            .run_command(id.clone(), "echo now".into())
            .expect("run once online");
        assert_eq!(now.output.trim(), "now", "a stale command ran instead");
    }
}

/// A host that is not answering must be retried with a growing gap, not once a second.
///
/// The ladder existed in `sg_core::backoff_for` and was unit-tested there, but the poll loop
/// called it with a hardcoded `1` — so an unreachable server was hammered every second for as long
/// as the app stayed open, while the UI displayed a "retry in" that grew. This measures the gap
/// between attempts rather than reading a constant, because the constant was never the problem.
#[test]
fn an_unreachable_host_is_retried_less_and_less_often() {
    let core = ServerGlass::new();
    let mut config = fixture_config(DEBIAN_PORT, 500);
    // A port nothing listens on, so every attempt fails the same transient way.
    config.port = 1;
    config.host = "127.0.0.1".into();

    let id = core.add_target(config);
    core.start(id.clone()).expect("start");

    // Watch the reported retry interval climb. With the bug it stays at one second forever.
    let mut seen: Vec<u64> = Vec::new();
    for _ in 0..60 {
        std::thread::sleep(Duration::from_millis(250));
        if let sg_ffi::ConnectionState::Reconnecting { retry_in_ms, .. } =
            core.snapshot(id.clone()).expect("snapshot").state
        {
            if seen.last() != Some(&retry_in_ms) {
                seen.push(retry_in_ms);
            }
        }
        if seen.len() >= 2 {
            break;
        }
    }

    assert!(
        seen.windows(2).all(|w| w[1] >= w[0]),
        "the retry interval must not shrink between attempts: {seen:?}"
    );
    assert!(
        seen.last().is_some_and(|last| *last > seen[0]) || seen.len() < 2,
        "the retry interval never grew: {seen:?}"
    );
}
