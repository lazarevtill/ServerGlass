//! The vertical slice, end to end, against the containers in `fixtures/`.
//!
//!     ./fixtures/up.sh
//!     SG_REQUIRE_FIXTURES=1 cargo test -p sg-core
//!
//! Everything below runs the real stack: a real SSH connection, the real batching protocol, the
//! real `/proc` parsers, the real rate engine and the real store. Nothing is mocked.

use std::path::PathBuf;
use std::time::Duration;

use sg_core::{default_sources, TargetRuntime, TargetState};
use sg_model::{EntityKind, SeriesKind, TargetId, Unit};
use sg_transport::auth::{Auth, ConnectionSpec, HostKeyPolicy};

const DEBIAN_PORT: u16 = 2222;
const ALPINE_PORT: u16 = 2223;

fn spec(port: u16) -> ConnectionSpec {
    ConnectionSpec::new("127.0.0.1", "root")
        .port(port)
        .auth(Auth::KeyFile {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/id_test"),
            passphrase: None,
        })
        .host_key_policy(HostKeyPolicy::AcceptAny)
}

async fn fixture_up(port: u16) -> bool {
    let up = tokio::time::timeout(
        Duration::from_millis(500),
        tokio::net::TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .is_some();

    if !up {
        assert!(
            std::env::var("SG_REQUIRE_FIXTURES").is_err(),
            "SG_REQUIRE_FIXTURES is set but nothing is listening on 127.0.0.1:{port}"
        );
        eprintln!("SKIP: no fixture on 127.0.0.1:{port} (run ./fixtures/up.sh)");
    }
    up
}

async fn online(port: u16) -> Option<TargetRuntime> {
    if !fixture_up(port).await {
        return None;
    }
    let mut runtime =
        TargetRuntime::new(TargetId::new("fixture"), spec(port), default_sources());
    runtime.connect().await.expect("connect to fixture");
    Some(runtime)
}

macro_rules! online_or_skip {
    ($port:expr) => {
        match online($port).await {
            Some(runtime) => runtime,
            None => return,
        }
    };
}

#[tokio::test]
async fn connecting_detects_the_host_and_costs_one_round_trip() {
    let runtime = online_or_skip!(DEBIAN_PORT);

    assert_eq!(runtime.state(), &TargetState::Online);
    let caps = runtime.capabilities().expect("capabilities detected");
    assert!(caps.distro.contains("Debian"), "distro was {:?}", caps.distro);
    assert!(caps.cpu_count >= 1);

    // Capability detection is itself one batch, not one request per probe.
    assert_eq!(runtime.round_trips(), 1, "detection should be a single batch");

    // The host entity is named after what the host calls itself, not what we typed.
    let host = runtime.host_entity().expect("host entity");
    assert_eq!(host.kind, EntityKind::Host);
    assert_eq!(host.display, caps.hostname);
    assert_eq!(host.labels.get("distro"), Some(&caps.distro));
}

/// The central claim of the whole design.
#[tokio::test]
async fn a_refresh_costs_exactly_one_round_trip_however_many_sources_run() {
    let mut runtime = online_or_skip!(DEBIAN_PORT);

    let sources = runtime.applicable_sources().len();
    let requests = runtime.planned_requests().len();
    assert!(sources >= 5, "expected several collectors to apply, got {sources}");
    assert!(requests >= sources, "each source should be asking for something");

    let before = runtime.round_trips();
    runtime.tick().await.expect("first tick");
    assert_eq!(
        runtime.round_trips() - before,
        1,
        "{sources} sources issuing {requests} requests cost more than one round trip"
    );

    for _ in 0..3 {
        let before = runtime.round_trips();
        runtime.tick().await.expect("tick");
        assert_eq!(runtime.round_trips() - before, 1);
    }
}

/// The first tick of a counter cannot be a rate; the second can.
#[tokio::test]
async fn rates_appear_on_the_second_tick_not_the_first() {
    let mut runtime = online_or_skip!(DEBIAN_PORT);

    let first = runtime.tick().await.expect("first tick");
    let counters: Vec<_> =
        first.descriptors.iter().filter(|d| d.kind == SeriesKind::Counter).collect();
    assert!(!counters.is_empty(), "no counter series were declared");

    let first_counter_samples = first
        .samples
        .iter()
        .filter(|s| counters.iter().any(|d| d.id == s.series))
        .count();
    assert_eq!(
        first_counter_samples, 0,
        "a counter produced a value from a single reading, which would spike every chart"
    );

    tokio::time::sleep(Duration::from_millis(1_200)).await;

    let second = runtime.tick().await.expect("second tick");
    let second_counter_samples = second
        .samples
        .iter()
        .filter(|s| counters.iter().any(|d| d.id == s.series))
        .count();
    assert!(second_counter_samples > 0, "no rates were derived on the second tick");
}

#[tokio::test]
async fn produces_plausible_readings_for_a_real_host() {
    let mut runtime = online_or_skip!(DEBIAN_PORT);

    runtime.tick().await.expect("first tick");
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    let tick = runtime.tick().await.expect("second tick");

    assert!(tick.errors.is_empty(), "collectors reported errors: {:?}", tick.errors);

    let store = runtime.store();
    let host = runtime.host_entity().unwrap().id.clone();

    let latest = |metric: &str| -> Option<f64> {
        let descriptor = store.series_for(&host).into_iter().find(|d| d.metric == metric)?;
        store.latest(&descriptor.id).map(|p| p.value)
    };

    let cpu = latest("cpu_usage").expect("cpu_usage");
    assert!(
        (0.0..=100.0).contains(&cpu),
        "aggregate CPU should be normalised to 0-100% across all cores, got {cpu}"
    );

    let memory = latest("mem_usage").expect("mem_usage");
    assert!((0.0..=100.0).contains(&memory), "memory usage {memory} out of range");
    assert!(memory > 0.0, "a running host is using some memory");

    let disk = latest("disk_usage").expect("disk_usage");
    assert!((0.0..=100.0).contains(&disk), "disk usage {disk} out of range");

    let uptime = latest("uptime").expect("uptime");
    assert!(uptime > 0.0, "uptime should be positive");

    // Rates exist and are non-negative. SSH traffic alone guarantees the receive counter moves.
    let rx = latest("net_rx").expect("net_rx rate");
    assert!(rx >= 0.0, "negative byte rate {rx}");
    assert!(latest("load1").is_some());
    assert!(latest("tcp_established").is_some());
}

#[tokio::test]
async fn builds_an_entity_tree_the_ui_can_navigate() {
    let mut runtime = online_or_skip!(DEBIAN_PORT);
    runtime.tick().await.expect("tick");

    let store = runtime.store();
    let host = runtime.host_entity().unwrap().id.clone();
    let children = store.children_of(&host);
    assert!(!children.is_empty(), "host has no child entities");

    let kinds: Vec<_> = children.iter().map(|e| e.kind.clone()).collect();
    assert!(kinds.contains(&EntityKind::CpuCore), "no CPU cores in the tree");
    assert!(kinds.contains(&EntityKind::NetworkInterface), "no interfaces in the tree");
    assert!(kinds.contains(&EntityKind::Filesystem), "no filesystems in the tree");

    let cores = children.iter().filter(|e| e.kind == EntityKind::CpuCore).count();
    assert_eq!(cores, runtime.capabilities().unwrap().cpu_count as usize);

    // Every child is reachable from the host, and every series hangs off a real entity.
    for child in &children {
        assert_eq!(child.parent.as_ref(), Some(&host));
        for descriptor in store.series_for(&child.id) {
            assert!(store.entity(&descriptor.entity).is_some());
        }
    }
}

/// Units must survive differentiation correctly, or every UI formats rates wrongly.
#[tokio::test]
async fn declared_units_match_what_the_scheduler_actually_produces() {
    let mut runtime = online_or_skip!(DEBIAN_PORT);
    runtime.tick().await.expect("tick");

    let store = runtime.store();
    let host = runtime.host_entity().unwrap().id.clone();
    let by_metric = |metric: &str| {
        store.series_for(&host).into_iter().find(|d| d.metric == metric).cloned()
    };

    // Byte counters become byte rates.
    assert_eq!(by_metric("net_rx").unwrap().effective_unit(), Unit::BytesPerSecond);
    assert_eq!(by_metric("disk_read").unwrap().effective_unit(), Unit::BytesPerSecond);
    // CPU jiffies scaled into percent stay percent — not "percent per second".
    assert_eq!(by_metric("cpu_usage").unwrap().effective_unit(), Unit::Percent);
    // Gauges are unchanged.
    assert_eq!(by_metric("mem_usage").unwrap().effective_unit(), Unit::Percent);
}

/// BusyBox output differs from GNU in `df`, `ls` and `ps`. The same collectors must work on both.
#[tokio::test]
async fn the_same_collectors_work_on_busybox() {
    let mut runtime = online_or_skip!(ALPINE_PORT);

    let caps = runtime.capabilities().unwrap();
    assert_eq!(caps.coreutils, sg_model::Coreutils::Busybox);

    runtime.tick().await.expect("first tick");
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    let tick = runtime.tick().await.expect("second tick");

    assert!(tick.errors.is_empty(), "collectors failed on BusyBox: {:?}", tick.errors);

    let store = runtime.store();
    let host = runtime.host_entity().unwrap().id.clone();
    let metrics: Vec<_> =
        store.series_for(&host).into_iter().map(|d| d.metric.clone()).collect();

    for expected in ["cpu_usage", "mem_usage", "disk_usage", "load1", "net_rx", "uptime"] {
        assert!(metrics.contains(&expected.to_string()), "BusyBox host is missing {expected}");
    }
}

/// The store is a rolling window, not a database. Left running, it must not grow.
#[tokio::test]
async fn the_live_store_stays_bounded_across_many_ticks() {
    let mut runtime = online_or_skip!(DEBIAN_PORT);

    for _ in 0..6 {
        runtime.tick().await.expect("tick");
    }
    let after_six = runtime.store().point_count();
    let series = runtime.store().series_count();

    for _ in 0..6 {
        runtime.tick().await.expect("tick");
    }
    let after_twelve = runtime.store().point_count();

    assert_eq!(runtime.store().series_count(), series, "series set should have stabilised");
    assert!(
        after_twelve <= series * sg_core::DEFAULT_WINDOW,
        "store exceeded its window: {after_twelve} points over {series} series"
    );
    assert!(after_twelve > after_six, "history should still be accumulating within the window");
}
