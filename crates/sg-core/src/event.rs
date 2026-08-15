//! What the core tells the UI.
//!
//! The core is a state machine: the UI sends commands and subscribes to this event stream. Keeping
//! the surface narrow and free of generics is what lets Swift, Kotlin, C# and Rust bindings all be
//! generated from it — and keeps every UI a view layer with no business logic of its own.

use sg_model::{Capabilities, Entity, Sample, SeriesDescriptor, TargetId};

/// Where a target is in its connection lifecycle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetState {
    /// Configured but not being polled.
    Idle,
    Connecting,
    /// Connected and collecting.
    Online,
    /// Lost the connection; the scheduler will retry after `retry_in_ms`.
    Reconnecting {
        attempt: u32,
        retry_in_ms: u64,
    },
    /// Stopped for a reason retrying cannot fix — bad credentials, a changed host key.
    Failed {
        message: String,
        recoverable: bool,
    },
}

impl TargetState {
    pub fn is_online(&self) -> bool {
        matches!(self, TargetState::Online)
    }
}

/// Emitted by the core, consumed by whichever UI is attached.
#[derive(Clone, Debug)]
pub enum Event {
    StateChanged {
        target: TargetId,
        state: TargetState,
    },
    /// Capability detection finished. The UI can now show which sources apply to this host.
    CapabilitiesDetected {
        target: TargetId,
        capabilities: Box<Capabilities>,
    },
    /// The entity tree changed — a container started, a disk appeared.
    EntitiesChanged {
        target: TargetId,
        entities: Vec<Entity>,
    },
    /// One refresh's worth of readings, with rates already derived.
    Samples {
        target: TargetId,
        descriptors: Vec<SeriesDescriptor>,
        samples: Vec<Sample>,
    },
    /// A collector failed to parse something the host returned. Surfaced rather than swallowed,
    /// but never fatal.
    SourceError {
        target: TargetId,
        source: String,
        message: String,
    },
}

impl Event {
    pub fn target(&self) -> &TargetId {
        match self {
            Event::StateChanged { target, .. }
            | Event::CapabilitiesDetected { target, .. }
            | Event::EntitiesChanged { target, .. }
            | Event::Samples { target, .. }
            | Event::SourceError { target, .. } => target,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_online_counts_as_online() {
        assert!(TargetState::Online.is_online());
        assert!(!TargetState::Connecting.is_online());
        assert!(!TargetState::Reconnecting {
            attempt: 1,
            retry_in_ms: 1000
        }
        .is_online());
        assert!(!TargetState::Idle.is_online());
    }

    #[test]
    fn every_event_carries_its_target() {
        let target = TargetId::new("t1");
        let events = [
            Event::StateChanged {
                target: target.clone(),
                state: TargetState::Online,
            },
            Event::CapabilitiesDetected {
                target: target.clone(),
                capabilities: Box::default(),
            },
            Event::EntitiesChanged {
                target: target.clone(),
                entities: vec![],
            },
            Event::Samples {
                target: target.clone(),
                descriptors: vec![],
                samples: vec![],
            },
            Event::SourceError {
                target: target.clone(),
                source: "proc.cpu".into(),
                message: "boom".into(),
            },
        ];

        // The UI routes on this, so a variant that forgot it would silently update the wrong host.
        for event in events {
            assert_eq!(event.target(), &target);
        }
    }
}
