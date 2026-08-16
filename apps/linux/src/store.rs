//! Persistence for the servers a person has added.
//!
//! Two stores, the same split the Apple and Android apps make:
//!
//! - **Configuration** — address, port, username, which sign-in method, key path — is not secret.
//!   It goes in `$XDG_CONFIG_HOME/serverglass/hosts.json`, where it can be read, edited and backed
//!   up like any other dotfile.
//! - **Secrets** — passwords and key passphrases — never appear in that file. See `secrets.rs`.
//!
//! The record is field-for-field the one `HostStore.SavedHost` writes on Apple and Android, so a
//! host that crosses by pairing lands in the same shape on every platform.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sg_ffi::TargetConfig;

/// Everything needed to reconnect, minus the secret.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedHost {
    pub id: String,
    pub address: String,
    pub port: u16,
    pub user: String,
    /// `agent`, `key` or `password`.
    pub auth_kind: String,
    pub key_path: Option<String>,
    /// `strict`, `accept_new` or `accept_any`.
    pub host_key_policy: String,
    pub refresh_ms: u64,
}

impl SavedHost {
    /// A host with the defaults the add dialog opens on.
    pub fn new(address: &str) -> Self {
        SavedHost {
            id: new_id(),
            address: address.to_string(),
            port: 22,
            user: String::new(),
            auth_kind: "agent".into(),
            key_path: None,
            host_key_policy: "accept_new".into(),
            refresh_ms: 1000,
        }
    }

    /// What to call this host in the sidebar before it has told us its own name.
    pub fn label(&self) -> String {
        if self.user.is_empty() {
            self.address.clone()
        } else {
            format!("{}@{}", self.user, self.address)
        }
    }

    /// Build the config the core wants, taking the secret at the last moment.
    ///
    /// The secret is passed in rather than stored on the record so it exists in memory for as
    /// short a time as possible, and so that nothing which touches disk can ever carry one.
    pub fn config(&self, secret: Option<String>, known_hosts: &Path) -> TargetConfig {
        TargetConfig {
            host: self.address.clone(),
            port: self.port,
            user: self.user.clone(),
            auth_kind: self.auth_kind.clone(),
            key_path: self.key_path.clone(),
            // Desktop Linux has a real filesystem and an ssh-agent, so a key is pointed at rather
            // than pasted. `key_text` is how a phone supplies one, and has no use here.
            key_text: None,
            secret,
            host_key_policy: self.host_key_policy.clone(),
            known_hosts_path: Some(known_hosts.to_string_lossy().into_owned()),
            refresh_ms: self.refresh_ms,
        }
    }
}

/// Where the inventory lives, and where trusted host keys are recorded.
///
/// XDG rather than `~/.ssh`: the host key pins ServerGlass records are the app's, and writing into
/// the user's own `known_hosts` would mean a monitoring tool editing the file their `ssh` client
/// depends on. `sg-transport` creates the containing directory — on mobile that step was missing
/// and every "remember this server" wrote nothing at all.
#[derive(Clone, Debug)]
pub struct Paths {
    pub config: PathBuf,
    pub known_hosts: PathBuf,
}

impl Paths {
    /// The real ones, from the environment.
    pub fn from_env() -> Self {
        let config_home = env_path("XDG_CONFIG_HOME").unwrap_or_else(|| home().join(".config"));
        let data_home =
            env_path("XDG_DATA_HOME").unwrap_or_else(|| home().join(".local").join("share"));
        Paths {
            config: config_home.join("serverglass").join("hosts.json"),
            known_hosts: data_home.join("serverglass").join("known_hosts"),
        }
    }

    /// Everything under one directory, for tests.
    pub fn under(root: &Path) -> Self {
        Paths {
            config: root.join("hosts.json"),
            known_hosts: root.join("known_hosts"),
        }
    }
}

/// Read the inventory.
///
/// A missing file is an empty list — the first launch, not a failure. A file that will not parse
/// is reported, because silently starting from empty would look exactly like "the app forgot every
/// server I added", and the difference matters when the cause is a half-written file.
pub fn load(paths: &Paths) -> Result<Vec<SavedHost>, String> {
    let text = match std::fs::read_to_string(&paths.config) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Could not read {}: {e}", paths.config.display())),
    };
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&text)
        .map_err(|e| format!("{} is not readable: {e}", paths.config.display()))
}

/// Write the inventory, creating the directory the first time.
///
/// Written to a temporary file and renamed over the target: a crash or a full disk midway through
/// must not leave a half-written inventory where a complete one used to be.
pub fn save(paths: &Paths, hosts: &[SavedHost]) -> Result<(), String> {
    if let Some(dir) = paths.config.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(hosts).map_err(|e| e.to_string())?;
    let temporary = paths.config.with_extension("json.tmp");
    std::fs::write(&temporary, text.as_bytes())
        .map_err(|e| format!("Could not write {}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, &paths.config)
        .map_err(|e| format!("Could not save {}: {e}", paths.config.display()))
}

fn env_path(key: &str) -> Option<PathBuf> {
    match std::env::var_os(key) {
        Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
        _ => None,
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// An identifier that does not need a UUID dependency for what it is used for.
///
/// It only has to be unique within one person's inventory and stable across restarts, which the
/// clock plus a counter satisfies. It is never shown and never leaves the device.
fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("h{millis:x}-{n:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    #[test]
    fn a_first_launch_has_no_hosts_rather_than_an_error() {
        let dir = temp();
        let paths = Paths::under(dir.path());
        assert_eq!(load(&paths).unwrap(), Vec::new());
    }

    #[test]
    fn hosts_survive_a_save_and_load() {
        let dir = temp();
        let paths = Paths::under(dir.path());
        let mut host = SavedHost::new("10.0.0.4");
        host.user = "root".into();
        host.port = 2222;
        host.key_path = Some("/home/a/.ssh/id_ed25519".into());

        save(&paths, std::slice::from_ref(&host)).unwrap();
        assert_eq!(load(&paths).unwrap(), vec![host]);
    }

    #[test]
    fn a_damaged_inventory_is_reported_rather_than_silently_emptied() {
        let dir = temp();
        let paths = Paths::under(dir.path());
        std::fs::write(&paths.config, b"{ this is not json").unwrap();

        // Returning Ok(vec![]) here would be indistinguishable from "the app forgot every server
        // you added", which is the one outcome a person must never be left to guess at.
        assert!(load(&paths).is_err());
    }

    #[test]
    fn saving_replaces_the_file_atomically_and_leaves_no_temporary_behind() {
        let dir = temp();
        let paths = Paths::under(dir.path());
        save(&paths, &[SavedHost::new("a")]).unwrap();
        save(&paths, &[SavedHost::new("b")]).unwrap();

        let hosts = load(&paths).unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].address, "b");
        assert!(!paths.config.with_extension("json.tmp").exists());
    }

    #[test]
    fn no_secret_field_can_reach_the_inventory_file() {
        // The record has no field to hold one, and this test fails the moment somebody adds one.
        // The same guarantee `crates/sg-sync` asserts for the pairing wire format, for the same
        // reason: invariant 5 is only real if something breaks when it is weakened.
        let dir = temp();
        let paths = Paths::under(dir.path());
        let host = SavedHost::new("10.0.0.4");
        save(&paths, &[host]).unwrap();

        let written = std::fs::read_to_string(&paths.config).unwrap();
        let value: serde_json::Value = serde_json::from_str(&written).unwrap();
        // The set, not the order: serde_json hands object keys back sorted, and the guarantee
        // being asserted is that nothing extra is present.
        let mut fields: Vec<String> = value[0].as_object().unwrap().keys().cloned().collect();
        fields.sort();
        let mut expected = vec![
            "id",
            "address",
            "port",
            "user",
            "auth_kind",
            "key_path",
            "host_key_policy",
            "refresh_ms",
        ];
        expected.sort_unstable();
        assert_eq!(fields, expected);
    }

    #[test]
    fn ids_are_distinct_even_within_the_same_millisecond() {
        let a = SavedHost::new("a");
        let b = SavedHost::new("b");
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn the_config_carries_the_apps_own_known_hosts_path() {
        let host = SavedHost::new("10.0.0.4");
        let config = host.config(None, Path::new("/data/serverglass/known_hosts"));

        // Empty would mean ~/.ssh/known_hosts: a monitoring tool editing the file the user's own
        // ssh client depends on.
        assert_eq!(
            config.known_hosts_path.as_deref(),
            Some("/data/serverglass/known_hosts")
        );
        assert!(config.key_text.is_none());
    }

    #[test]
    fn a_host_with_no_username_still_has_something_to_call_it() {
        assert_eq!(SavedHost::new("10.0.0.4").label(), "10.0.0.4");
        let mut named = SavedHost::new("10.0.0.4");
        named.user = "root".into();
        assert_eq!(named.label(), "root@10.0.0.4");
    }
}
