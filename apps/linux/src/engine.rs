//! The bridge between the saved inventory and the running core.
//!
//! `sg_ffi::ServerGlass` keys targets by an identifier it hands out; the inventory keys hosts by
//! one that survives restarts. This owns the mapping between them, and nothing else — every
//! decision about scheduling, connecting, parsing and reconnecting stays behind that object.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use sg_ffi::{ServerGlass, TargetSnapshot};

use crate::store::{Paths, SavedHost};

/// One process-wide core, and the targets registered against it.
#[derive(Clone)]
pub struct Engine {
    core: Arc<ServerGlass>,
    /// Saved-host id to the core's target id.
    targets: Rc<RefCell<HashMap<String, String>>>,
    known_hosts: PathBuf,
}

impl Engine {
    pub fn new(paths: &Paths) -> Self {
        Engine {
            core: ServerGlass::new(),
            targets: Rc::new(RefCell::new(HashMap::new())),
            known_hosts: paths.known_hosts.clone(),
        }
    }

    /// Register a host and begin refreshing it.
    ///
    /// Idempotent: starting an already-running host is what happens every time the sidebar
    /// selection changes, and must not open a second connection.
    pub fn start(&self, host: &SavedHost, secret: Option<String>) -> Result<(), String> {
        if self.targets.borrow().contains_key(&host.id) {
            return Ok(());
        }
        let config = host.config(secret, &self.known_hosts);
        let target_id = self.core.add_target(config);
        self.targets
            .borrow_mut()
            .insert(host.id.clone(), target_id.clone());
        self.core.start(target_id).map_err(|e| e.to_string())
    }

    /// Drop a host's connection and forget it.
    pub fn forget(&self, host_id: &str) {
        let Some(target_id) = self.targets.borrow_mut().remove(host_id) else {
            return;
        };
        // A failure here means the core has already forgotten the target, which is the state being
        // asked for. Nothing is hidden by carrying on.
        let _ = self.core.remove_target(target_id);
    }

    /// Apply an edited host by rebuilding its target.
    ///
    /// The address or the sign-in method may have changed, and `TargetConfig` is consumed when the
    /// target is created — so the honest way to change one is to replace it.
    pub fn restart(&self, host: &SavedHost, secret: Option<String>) -> Result<(), String> {
        self.forget(&host.id);
        self.start(host, secret)
    }

    /// The last completed refresh, or `None` for a host that has not been started.
    pub fn snapshot(&self, host_id: &str) -> Option<TargetSnapshot> {
        let target_id = self.targets.borrow().get(host_id).cloned()?;
        self.core.snapshot(target_id).ok()
    }

    /// Whether a host is registered with the core at all.
    pub fn is_running(&self, host_id: &str) -> bool {
        self.targets.borrow().contains_key(host_id)
    }

    /// The core's target id for a host, for the command runner.
    pub fn target_id(&self, host_id: &str) -> Option<String> {
        self.targets.borrow().get(host_id).cloned()
    }

    /// A handle that can be moved to a worker thread.
    ///
    /// `run_command` blocks until the host answers, so it must never be called on the thread
    /// driving the interface. Only the `Arc` crosses; the id map stays on the main thread where it
    /// is read and written.
    pub fn core(&self) -> Arc<ServerGlass> {
        Arc::clone(&self.core)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().expect("temp dir");
        let paths = Paths::under(dir.path());
        (dir, paths)
    }

    #[test]
    fn starting_the_same_host_twice_does_not_open_a_second_connection() {
        let (_dir, paths) = paths();
        let engine = Engine::new(&paths);
        let host = SavedHost::new("198.51.100.1");

        engine.start(&host, None).unwrap();
        let first = engine.target_id(&host.id);
        engine.start(&host, None).unwrap();

        assert_eq!(engine.target_id(&host.id), first);
    }

    #[test]
    fn a_forgotten_host_has_no_snapshot_and_no_target() {
        let (_dir, paths) = paths();
        let engine = Engine::new(&paths);
        let host = SavedHost::new("198.51.100.1");

        engine.start(&host, None).unwrap();
        engine.forget(&host.id);

        assert!(!engine.is_running(&host.id));
        assert!(engine.snapshot(&host.id).is_none());
    }

    #[test]
    fn forgetting_a_host_that_was_never_started_is_harmless() {
        let (_dir, paths) = paths();
        let engine = Engine::new(&paths);
        engine.forget("never-existed");
    }

    #[test]
    fn restarting_replaces_the_target_so_an_edited_address_takes_effect() {
        let (_dir, paths) = paths();
        let engine = Engine::new(&paths);
        let mut host = SavedHost::new("198.51.100.1");

        engine.start(&host, None).unwrap();
        let before = engine.target_id(&host.id).unwrap();

        host.address = "198.51.100.2".into();
        engine.restart(&host, None).unwrap();

        assert_ne!(engine.target_id(&host.id).unwrap(), before);
    }

    #[test]
    fn a_host_that_has_never_connected_still_reports_a_snapshot_to_render() {
        // Rather than the UI special-casing "no data yet" everywhere, the core hands back a
        // placeholder that renders.
        let (_dir, paths) = paths();
        let engine = Engine::new(&paths);
        let host = SavedHost::new("198.51.100.1");
        engine.start(&host, None).unwrap();

        let snapshot = engine.snapshot(&host.id).expect("placeholder snapshot");
        assert_eq!(snapshot.display_name, "198.51.100.1");
    }
}
