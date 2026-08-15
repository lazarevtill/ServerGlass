//! UniFFI bindings for the ServerGlass core.
//!
//! The surface is deliberately narrow: add a target, start it, read a snapshot. All parsing,
//! scheduling, rate derivation and connection handling stay in Rust, so the macOS, Windows, Linux
//! and Android front-ends share every line of it.
//!
//! # Snapshot polling, not an event stream
//!
//! Each target runs a background task that ticks on its own interval and publishes a finished
//! [`TargetSnapshot`]. The UI calls [`ServerGlass::snapshot`] on a display timer and renders what
//! it gets. At a one-second refresh this is indistinguishable from a push stream, needs no
//! callback interface on four platforms, and cannot deadlock the tick loop behind a slow UI
//! thread. A push-based event stream is the natural next step once the terminal lands, which does
//! need one — a terminal cannot be polled.

pub mod plain;
mod view;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use sg_core::{default_sources, TargetRuntime, TargetState};
use sg_model::{now_ms, TargetId};

pub use plain::HostHealth;
use view::{
    connection_spec, entity_view, host_details, host_gauges, simple_tiles, top_processes,
    PROCESS_KIND,
};
pub use view::{
    format_uptime, format_value, ConnectionState, DetailGroup, EntityView, MetricGauge,
    ProcessView, SimpleTile, TargetConfig, TargetSnapshot,
};

uniffi::setup_scaffolding!();

/// Anything the UI can be told about a failure.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum SgError {
    #[error("no such target: {id}")]
    UnknownTarget { id: String },
    #[error("{detail}")]
    Connection { detail: String, recoverable: bool },
    #[error("internal error: {detail}")]
    Internal { detail: String },
}

// The field is `detail`, not `message`, on purpose: UniFFI maps an error enum onto a Kotlin
// `Exception` subclass, which already has a `message` property, and the duplicate makes every
// reference to it an "overload resolution ambiguity" that fails the Android build.

/// One command the user asked to run, and where to send the answer.
type CommandJob = (String, tokio::sync::oneshot::Sender<Result<CommandResult, String>>);

/// One monitored host and its background poller.
struct Target {
    config: TargetConfig,
    snapshot: Arc<RwLock<TargetSnapshot>>,
    /// Dropping this aborts the poll loop.
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Commands waiting to run on the poll loop's session.
    ///
    /// A queue rather than a second connection: the poll loop owns the session, and opening
    /// another one per command would authenticate again, double the connections a host sees, and
    /// give the command a different environment from the one the readings come from.
    commands: tokio::sync::mpsc::UnboundedSender<CommandJob>,
    /// Held so the receiver survives being handed to each new poll-loop attempt.
    command_inbox: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<CommandJob>>,
}

/// What one command printed, and how it ended.
#[derive(Clone, Debug, uniffi::Record)]
pub struct CommandResult {
    /// Everything it wrote, standard error included and in order.
    pub output: String,
    /// -1 when the host did not report one.
    pub exit_code: i32,
    pub elapsed_ms: u64,
}

/// The core, as the UIs see it.
#[derive(uniffi::Object)]
pub struct ServerGlass {
    runtime: tokio::runtime::Runtime,
    targets: RwLock<HashMap<String, Arc<Target>>>,
    next_id: Mutex<u64>,
}

#[uniffi::export]
impl ServerGlass {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        // A dedicated multi-threaded runtime: the UI thread must never be the thing driving SSH.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("serverglass")
            .build()
            .expect("tokio runtime");

        Arc::new(ServerGlass {
            runtime,
            targets: RwLock::new(HashMap::new()),
            next_id: Mutex::new(0),
        })
    }

    /// Register a host. Does not connect; call [`ServerGlass::start`] for that.
    pub fn add_target(&self, config: TargetConfig) -> String {
        let id = {
            let mut next = self.next_id.lock().expect("id lock");
            *next += 1;
            format!("t{}", *next)
        };

        let snapshot = Arc::new(RwLock::new(TargetSnapshot::placeholder(
            &id,
            &config.host,
            ConnectionState::Idle,
        )));

        let (commands, command_inbox) = tokio::sync::mpsc::unbounded_channel();

        self.targets.write().expect("targets lock").insert(
            id.clone(),
            Arc::new(Target {
                config,
                snapshot,
                task: Mutex::new(None),
                commands,
                command_inbox: tokio::sync::Mutex::new(command_inbox),
            }),
        );
        id
    }

    /// Connect and begin refreshing. Returns immediately; watch the snapshot for progress.
    pub fn start(&self, target_id: String) -> Result<(), SgError> {
        let target = self.target(&target_id)?;

        let mut slot = target.task.lock().expect("task lock");
        if slot.as_ref().is_some_and(|t| !t.is_finished()) {
            return Ok(());
        }

        let handle = self
            .runtime
            .spawn(poll_loop(target_id, Arc::clone(&target)));
        *slot = Some(handle);
        Ok(())
    }

    /// Run one command on the host and wait for what it printed.
    ///
    /// Blocks until the command finishes, so call it off the UI thread. It runs on the connection
    /// the readings already use — one round trip, no second sign-in — which also means the host
    /// must be online: there is no queueing a command for a machine that is not answering.
    ///
    /// **Not a terminal.** No PTY is allocated, so anything interactive (`top`, `vim`, a `sudo`
    /// password prompt) will produce nothing useful or hang until the timeout. That limit is the
    /// honest shape of the existing transport, and the UIs say so rather than hiding it.
    pub fn run_command(&self, target_id: String, command: String) -> Result<CommandResult, SgError> {
        let target = self.target(&target_id)?;
        let command = command.trim().to_string();
        if command.is_empty() {
            return Ok(CommandResult {
                output: String::new(),
                exit_code: 0,
                elapsed_ms: 0,
            });
        }

        let (reply, answer) = tokio::sync::oneshot::channel();
        target
            .commands
            .send((command, reply))
            .map_err(|_| SgError::Connection {
                detail: "This server is not connected.".into(),
                recoverable: true,
            })?;

        // A bounded wait: a command that never returns must not leave the caller's thread parked
        // for the life of the app. Sixty seconds is long enough for a package list and short
        // enough to notice.
        self.runtime.block_on(async {
            match tokio::time::timeout(std::time::Duration::from_secs(60), answer).await {
                Ok(Ok(Ok(result))) => Ok(result),
                Ok(Ok(Err(detail))) => Err(SgError::Connection {
                    detail,
                    recoverable: true,
                }),
                // The poll loop dropped the reply channel: the connection went down under it.
                Ok(Err(_)) => Err(SgError::Connection {
                    detail: "The connection dropped before the command finished.".into(),
                    recoverable: true,
                }),
                Err(_) => Err(SgError::Connection {
                    detail: "The command did not finish within 60 seconds. Interactive programs \
                             such as top or vim cannot run here."
                        .into(),
                    recoverable: true,
                }),
            }
        })
    }

    /// Stop refreshing and drop the connection.
    pub fn stop(&self, target_id: String) -> Result<(), SgError> {
        let target = self.target(&target_id)?;
        if let Some(handle) = target.task.lock().expect("task lock").take() {
            handle.abort();
        }
        let mut snapshot = target.snapshot.write().expect("snapshot lock");
        snapshot.state = ConnectionState::Idle;
        Ok(())
    }

    pub fn remove_target(&self, target_id: String) -> Result<(), SgError> {
        self.stop(target_id.clone())?;
        self.targets
            .write()
            .expect("targets lock")
            .remove(&target_id);
        Ok(())
    }

    /// The most recent completed refresh. Cheap enough to call on a display timer.
    pub fn snapshot(&self, target_id: String) -> Result<TargetSnapshot, SgError> {
        let target = self.target(&target_id)?;
        let snapshot = target.snapshot.read().expect("snapshot lock").clone();
        Ok(snapshot)
    }

    pub fn target_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .targets
            .read()
            .expect("targets lock")
            .keys()
            .cloned()
            .collect();
        ids.sort();
        ids
    }

    /// Format a value the way every ServerGlass UI formats it.
    pub fn format(&self, value: f64, unit_suffix: String, binary_scaled: bool) -> String {
        format_value(value, &unit_suffix, binary_scaled)
    }

    pub fn format_duration(&self, seconds: f64) -> String {
        format_uptime(seconds)
    }
}

impl ServerGlass {
    fn target(&self, id: &str) -> Result<Arc<Target>, SgError> {
        self.targets
            .read()
            .expect("targets lock")
            .get(id)
            .cloned()
            .ok_or_else(|| SgError::UnknownTarget { id: id.to_string() })
    }
}

/// Connect, then refresh forever, republishing a snapshot after each tick.
async fn poll_loop(target_id: String, target: Arc<Target>) {
    let publish = |state: ConnectionState| {
        let mut snapshot = target.snapshot.write().expect("snapshot lock");
        snapshot.state = state;
    };

    let interval = std::time::Duration::from_millis(target.config.refresh_ms.clamp(250, 60_000));

    loop {
        publish(ConnectionState::Connecting);

        let mut runtime = TargetRuntime::new(
            TargetId::new(target_id.clone()),
            connection_spec(&target.config),
            default_sources(),
        );

        if let Err(error) = runtime.connect().await {
            let recoverable = error.is_transient();
            publish(ConnectionState::Failed {
                message: error.to_string(),
                recoverable,
            });
            if !recoverable {
                // Bad credentials or a changed host key: retrying cannot fix it, and hammering
                // the host is how accounts get locked out.
                return;
            }
            tokio::time::sleep(sg_core::backoff_for(1)).await;
            continue;
        }

        // The first tick after connecting carries no counter-derived series: a rate needs two
        // readings. Publishing it would render a status grid without its CPU and network tiles,
        // which then appear one refresh later and shove every other tile sideways. Holding the
        // first tick back costs one refresh interval of "Collecting…" and means the grid is
        // complete and stable the moment it appears.
        let mut published_first = false;

        loop {
            let tick_started = tokio::time::Instant::now();
            match runtime.tick().await {
                Ok(tick) => {
                    if published_first {
                        let snapshot = build_snapshot(&target_id, &runtime, &tick);
                        *target.snapshot.write().expect("snapshot lock") = snapshot;
                    } else {
                        published_first = true;
                        publish(ConnectionState::Online);
                    }
                }
                Err(_) => {
                    // `tick` has already moved the runtime into Reconnecting or Failed; publish
                    // that and fall out to re-establish the session.
                    publish(ConnectionState::from(runtime.state()));
                    break;
                }
            }

            // Subtract the time the tick took, so a slow host produces a steady cadence rather
            // than drifting further behind on every refresh.
            let elapsed = tick_started.elapsed();
            let until_next = tokio::time::sleep(interval.saturating_sub(elapsed));
            tokio::pin!(until_next);

            // Commands run in the gap between refreshes rather than waiting for one. Someone who
            // just pressed Return should not wait out a ten-second refresh interval, and a slow
            // command must not delay the readings any longer than it has to.
            loop {
                let mut inbox = target.command_inbox.lock().await;
                tokio::select! {
                    _ = &mut until_next => break,
                    job = inbox.recv() => {
                        let Some((command, reply)) = job else { break };
                        drop(inbox);
                        let answer = runtime
                            .run_command(&command)
                            .await
                            .map(|out| CommandResult {
                                output: out.output,
                                exit_code: out.exit_code,
                                elapsed_ms: out.elapsed_ms,
                            })
                            .map_err(|e| e.to_string());
                        let failed = answer.is_err();
                        // The receiver may be gone if the caller timed out; its answer is simply
                        // discarded.
                        let _ = reply.send(answer);
                        if failed {
                            // `run_command` has already moved the runtime into Reconnecting or
                            // Failed; surface it and rebuild the session.
                            publish(ConnectionState::from(runtime.state()));
                            break;
                        }
                    }
                }
            }
        }

        if let TargetState::Failed {
            recoverable: false, ..
        } = runtime.state()
        {
            return;
        }
        tokio::time::sleep(sg_core::backoff_for(1)).await;
    }
}

fn build_snapshot(
    target_id: &str,
    runtime: &TargetRuntime,
    tick: &sg_core::Tick,
) -> TargetSnapshot {
    let store = runtime.store();
    let caps = runtime.capabilities();
    let host = runtime.host_entity();

    let (gauges, detail_groups, entities, processes) = match host {
        Some(host) => (
            host_gauges(store, &host.id),
            host_details(store, &host.id),
            store
                .children_of(&host.id)
                .into_iter()
                // Processes are excluded here on purpose. There are hundreds, each with two series
                // and a 300-point window; serialising them across the FFI twice a second would
                // cost far more than the whole rest of the snapshot. The ranked few go in
                // `top_processes` instead.
                .filter(|e| e.kind.slug() != PROCESS_KIND)
                .map(|e| entity_view(e, store))
                .collect(),
            top_processes(store, &host.id, 12),
        ),
        None => (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
    };

    let state = ConnectionState::from(runtime.state());
    TargetSnapshot {
        target_id: target_id.to_string(),
        state: state.clone(),
        display_name: host.map(|h| h.display.clone()).unwrap_or_default(),
        distro: caps.map(|c| c.distro.clone()).unwrap_or_default(),
        kernel: caps.map(|c| c.kernel.clone()).unwrap_or_default(),
        arch: caps.map(|c| c.arch.clone()).unwrap_or_default(),
        cpu_count: caps.map(|c| c.cpu_count).unwrap_or(0),
        health: plain::assess(&state, &gauges, !gauges.is_empty()),
        simple_tiles: {
            let mut all = gauges.clone();
            all.extend(detail_groups.iter().flat_map(|g| g.gauges.iter().cloned()));
            simple_tiles(&gauges, &all, &entities)
        },
        gauges,
        detail_groups,
        entities,
        top_processes: processes,
        source_errors: tick.errors.iter().map(ToString::to_string).collect(),
        last_update_ms: now_ms(),
        round_trips: runtime.round_trips(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(host: &str) -> TargetConfig {
        TargetConfig {
            host: host.into(),
            port: 22,
            user: "root".into(),
            auth_kind: "agent".into(),
            key_path: None,
            key_text: None,
            secret: None,
            host_key_policy: "strict".into(),
            refresh_ms: 1000,
        }
    }

    #[test]
    fn targets_get_distinct_ids_and_a_placeholder_snapshot() {
        let core = ServerGlass::new();
        let a = core.add_target(config("host-a"));
        let b = core.add_target(config("host-b"));

        assert_ne!(a, b);
        assert_eq!(core.target_ids(), vec![a.clone(), b.clone()]);

        // A target that has never connected still renders, rather than the UI special-casing nil.
        let snapshot = core.snapshot(a).unwrap();
        assert_eq!(snapshot.state, ConnectionState::Idle);
        assert_eq!(snapshot.display_name, "host-a");
        assert!(snapshot.gauges.is_empty());
    }

    #[test]
    fn unknown_targets_are_reported_rather_than_panicking() {
        let core = ServerGlass::new();
        assert!(matches!(
            core.snapshot("nope".into()),
            Err(SgError::UnknownTarget { .. })
        ));
        assert!(matches!(
            core.start("nope".into()),
            Err(SgError::UnknownTarget { .. })
        ));
        assert!(matches!(
            core.stop("nope".into()),
            Err(SgError::UnknownTarget { .. })
        ));
    }

    #[test]
    fn removing_a_target_stops_it_and_forgets_it() {
        let core = ServerGlass::new();
        let id = core.add_target(config("host-a"));

        core.remove_target(id.clone()).unwrap();
        assert!(core.target_ids().is_empty());
        assert!(matches!(
            core.snapshot(id),
            Err(SgError::UnknownTarget { .. })
        ));
    }

    #[test]
    fn stopping_an_idle_target_is_harmless() {
        let core = ServerGlass::new();
        let id = core.add_target(config("host-a"));
        core.stop(id.clone()).unwrap();
        core.stop(id.clone()).unwrap();
        assert_eq!(core.snapshot(id).unwrap().state, ConnectionState::Idle);
    }

    #[test]
    fn formatting_is_exposed_so_every_ui_agrees() {
        let core = ServerGlass::new();
        assert_eq!(core.format(1536.0, "B".into(), true), "1.5 KiB");
        assert_eq!(core.format(42.0, "%".into(), false), "42.0%");
        assert_eq!(core.format_duration(90_000.0), "1d 1h");
    }
}
