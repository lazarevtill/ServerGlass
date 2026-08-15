//! Per-target runtime: connect, detect, refresh.
//!
//! One [`TargetRuntime`] owns everything about one monitored host — its session, its source set,
//! its rate state and its live store. It exposes `connect` and `tick` rather than running its own
//! loop, so the caller decides the cadence and the tests can drive it deterministically.

use std::time::Duration;

use sg_model::{now_ms, Capabilities, Entity, Request, Responses, Source, TargetCtx, TargetId};
use sg_transport::{ConnectionSpec, Result as TransportResult, SshSession, TransportError};

use crate::collector::{Collector, Tick};
use crate::event::TargetState;
use crate::store::LiveStore;

/// Backoff schedule for transient reconnects, in milliseconds.
///
/// Capped rather than unbounded: a laptop that closed its lid should reconnect within half a
/// minute of waking, not an hour later because the backoff had doubled all night.
const BACKOFF_MS: [u64; 6] = [1_000, 2_000, 5_000, 10_000, 20_000, 30_000];

pub fn backoff_for(attempt: u32) -> Duration {
    let index = (attempt.saturating_sub(1) as usize).min(BACKOFF_MS.len() - 1);
    Duration::from_millis(BACKOFF_MS[index])
}

/// What one refresh did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TickSummary {
    pub entities: usize,
    pub series: usize,
    pub samples: usize,
    pub errors: usize,
    /// Requests issued this tick, before deduplication removed overlaps.
    pub requests: usize,
}

pub struct TargetRuntime {
    target: TargetId,
    spec: ConnectionSpec,
    session: Option<SshSession>,
    ctx: Option<TargetCtx>,
    collector: Collector,
    store: LiveStore,
    state: TargetState,
    attempt: u32,
}

impl TargetRuntime {
    pub fn new(target: TargetId, spec: ConnectionSpec, sources: Vec<Box<dyn Source>>) -> Self {
        TargetRuntime {
            target,
            spec,
            session: None,
            ctx: None,
            collector: Collector::new(sources),
            store: LiveStore::default(),
            state: TargetState::Idle,
            attempt: 0,
        }
    }

    /// Every registered collector, so the UI can list and toggle them.
    pub fn all_sources(&self) -> Vec<&dyn Source> {
        self.collector.sources().collect()
    }

    /// Collectors this host can actually support. Empty until [`TargetRuntime::connect`] has run.
    pub fn applicable_sources(&self) -> Vec<&dyn Source> {
        match &self.ctx {
            Some(ctx) => self.collector.applicable(&ctx.caps).collect(),
            None => Vec::new(),
        }
    }

    pub fn set_source_enabled(&mut self, source: &sg_model::SourceId, enabled: bool) {
        self.collector.set_enabled(source, enabled);
    }

    pub fn state(&self) -> &TargetState {
        &self.state
    }

    pub fn store(&self) -> &LiveStore {
        &self.store
    }

    pub fn capabilities(&self) -> Option<&Capabilities> {
        self.ctx.as_ref().map(|c| &c.caps)
    }

    pub fn target(&self) -> &TargetId {
        &self.target
    }

    /// Round trips this runtime has spent since connecting.
    pub fn round_trips(&self) -> u64 {
        self.session
            .as_ref()
            .map(SshSession::round_trips)
            .unwrap_or(0)
    }

    /// Connect, detect capabilities, and build the collection context.
    ///
    /// Capability detection is itself one batch, so coming online costs two round trips total:
    /// one to learn what the host is, one for the first refresh.
    pub async fn connect(&mut self) -> TransportResult<()> {
        self.state = TargetState::Connecting;

        let mut session = match SshSession::connect(self.spec.clone()).await {
            Ok(session) => session,
            Err(error) => {
                self.state = TargetState::Failed {
                    message: error.to_string(),
                    recoverable: error.is_transient(),
                };
                return Err(error);
            }
        };

        let responses = session.batch(&sg_transport::probe::requests()).await?;
        let caps = sg_transport::probe::parse(&responses);

        // Prefer the host's own idea of its name over whatever the user typed into the address
        // field, which is often an IP or a jump-host alias.
        let display = if caps.hostname.is_empty() {
            self.spec.host.clone()
        } else {
            caps.hostname.clone()
        };

        self.ctx = Some(TargetCtx {
            target: self.target.clone(),
            host: Entity::host(display)
                .with_label("kernel", &caps.kernel)
                .with_label("distro", &caps.distro)
                .with_label("arch", &caps.arch),
            caps,
            interval_ms: 1_000,
        });

        // A reconnect must not derive a rate across the gap: the host may have rebooted, and the
        // counters on the far side would have restarted.
        self.collector.reset_rates();
        self.store.clear();

        self.session = Some(session);
        self.state = TargetState::Online;
        self.attempt = 0;
        Ok(())
    }

    /// One refresh.
    ///
    /// The whole point of the architecture: however many collectors are enabled, this issues
    /// exactly one batch. [`TargetRuntime::round_trips`] is asserted against that in the tests.
    pub async fn tick(&mut self) -> TransportResult<Tick> {
        let (Some(session), Some(ctx)) = (self.session.as_mut(), self.ctx.as_ref()) else {
            return Err(TransportError::Closed);
        };

        let requests = self.collector.requests(ctx);
        let responses = if requests.is_empty() {
            Responses::default()
        } else {
            match session.batch(&requests).await {
                Ok(responses) => responses,
                Err(error) => {
                    self.on_disconnect(&error);
                    return Err(error);
                }
            }
        };

        let tick = self.collector.collect(ctx, &responses, now_ms());

        self.store.ingest(
            tick.entities.clone(),
            tick.descriptors.clone(),
            &tick.samples,
        );
        // Prune anything that stopped being reported — a stopped container must leave the
        // dashboard rather than sitting there with its last reading frozen.
        let mut present = tick.entity_ids();
        present.push(ctx.host.id.clone());
        self.store.retain_entities(&present);

        Ok(tick)
    }

    /// The host entity, once connected.
    pub fn host_entity(&self) -> Option<&Entity> {
        self.ctx.as_ref().map(|c| &c.host)
    }

    /// The merged request set a refresh would issue right now, for diagnostics.
    pub fn planned_requests(&self) -> Vec<Request> {
        self.ctx
            .as_ref()
            .map(|ctx| self.collector.requests(ctx))
            .unwrap_or_default()
    }

    fn on_disconnect(&mut self, error: &TransportError) {
        self.session = None;
        self.collector.reset_rates();

        if error.is_transient() {
            self.attempt += 1;
            self.state = TargetState::Reconnecting {
                attempt: self.attempt,
                retry_in_ms: backoff_for(self.attempt).as_millis() as u64,
            };
        } else {
            self.state = TargetState::Failed {
                message: error.to_string(),
                recoverable: false,
            };
        }
    }

    /// Summarise a tick for logging and tests.
    pub fn summarise(&self, tick: &Tick) -> TickSummary {
        TickSummary {
            entities: tick.entities.len(),
            series: tick.descriptors.len(),
            samples: tick.samples.len(),
            errors: tick.errors.len(),
            requests: self.planned_requests().len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_stops_growing() {
        assert_eq!(backoff_for(1), Duration::from_secs(1));
        assert_eq!(backoff_for(2), Duration::from_secs(2));
        assert_eq!(backoff_for(3), Duration::from_secs(5));

        // A laptop waking from a closed lid should reconnect within half a minute, not after an
        // hour of doubling.
        assert_eq!(backoff_for(50), Duration::from_secs(30));
        assert_eq!(backoff_for(u32::MAX), Duration::from_secs(30));
    }

    #[test]
    fn attempt_zero_does_not_underflow() {
        assert_eq!(backoff_for(0), Duration::from_secs(1));
    }
}
