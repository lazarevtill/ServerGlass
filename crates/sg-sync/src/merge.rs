//! Applying a received payload to what a device already has.
//!
//! The rule that matters is about host key pins. A new pin for a host nobody has met merges
//! silently. A pin that *differs* from one already held is never applied — it is reported, and the
//! user decides. A sync channel that can quietly rewrite a pin is a machine-in-the-middle with
//! extra steps, and it would defeat the exact protection the pin exists to provide.

use crate::payload::{Payload, SyncHost};

/// A pin that disagrees with one already trusted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinConflict {
    /// The `[host]:port` the two lines disagree about.
    pub host: String,
    pub existing: String,
    pub incoming: String,
}

/// What a merge did, and what it refused to do.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Merge {
    pub hosts: Vec<SyncHost>,
    pub known_hosts: Vec<String>,
    /// Servers that were not already present.
    pub added_hosts: usize,
    /// Servers already known, left untouched.
    pub kept_hosts: usize,
    pub added_pins: usize,
    /// Never applied. Each one needs a person to decide.
    pub conflicts: Vec<PinConflict>,
}

/// The `[host]:port` field of a known_hosts line.
fn pin_host(line: &str) -> Option<&str> {
    line.split_whitespace().next()
}

/// Everything after the host field: the key type and the key itself.
fn pin_key(line: &str) -> Option<&str> {
    line.split_once(char::is_whitespace)
        .map(|(_, rest)| rest.trim())
}

pub fn merge(existing: &Payload, incoming: &Payload) -> Merge {
    let mut out = Merge {
        hosts: existing.hosts.clone(),
        known_hosts: existing.known_hosts.clone(),
        ..Merge::default()
    };

    for host in &incoming.hosts {
        // Local settings win: a refresh interval or auth method chosen on *this* device is a
        // deliberate choice, and a transfer is not a reason to overwrite it.
        if out.hosts.iter().any(|h| h.identity() == host.identity()) {
            out.kept_hosts += 1;
        } else {
            out.hosts.push(host.clone());
            out.added_hosts += 1;
        }
    }

    for line in &incoming.known_hosts {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(host) = pin_host(line) else { continue };

        match out.known_hosts.iter().find(|l| pin_host(l) == Some(host)) {
            Some(held) if pin_key(held) == pin_key(line) => {}
            Some(held) => out.conflicts.push(PinConflict {
                host: host.to_string(),
                existing: held.clone(),
                incoming: line.to_string(),
            }),
            None => {
                out.known_hosts.push(line.to_string());
                out.added_pins += 1;
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(address: &str) -> SyncHost {
        SyncHost {
            address: address.into(),
            port: 22,
            user: "root".into(),
            auth_kind: "password".into(),
            key_path: None,
            host_key_policy: "accept_new".into(),
            refresh_ms: 1000,
        }
    }

    fn pin(host: &str, key: &str) -> String {
        format!("[{host}]:22 ssh-ed25519 {key}")
    }

    #[test]
    fn new_servers_are_added_and_known_ones_left_alone() {
        let mut mine = host("a");
        mine.refresh_ms = 5000;
        let existing = Payload {
            hosts: vec![mine],
            known_hosts: vec![],
        };
        let incoming = Payload {
            hosts: vec![host("a"), host("b")],
            known_hosts: vec![],
        };

        let merged = merge(&existing, &incoming);
        assert_eq!(merged.added_hosts, 1);
        assert_eq!(merged.kept_hosts, 1);
        assert_eq!(merged.hosts.len(), 2);
        // The local choice survived the transfer.
        assert_eq!(merged.hosts[0].refresh_ms, 5000);
    }

    #[test]
    fn a_pin_for_an_unknown_host_merges_silently() {
        let existing = Payload::default();
        let incoming = Payload {
            hosts: vec![],
            known_hosts: vec![pin("a", "AAAA")],
        };

        let merged = merge(&existing, &incoming);
        assert_eq!(merged.added_pins, 1);
        assert!(merged.conflicts.is_empty());
    }

    /// The one that matters: a *different* key for a host already trusted is never applied.
    #[test]
    fn a_conflicting_pin_is_reported_and_never_applied() {
        let existing = Payload {
            hosts: vec![],
            known_hosts: vec![pin("a", "HONEST")],
        };
        let incoming = Payload {
            hosts: vec![],
            known_hosts: vec![pin("a", "IMPOSTOR")],
        };

        let merged = merge(&existing, &incoming);
        assert_eq!(merged.added_pins, 0);
        assert_eq!(merged.conflicts.len(), 1);
        assert_eq!(merged.conflicts[0].host, "[a]:22");
        assert!(
            merged.known_hosts.iter().all(|l| l.contains("HONEST")),
            "the trusted pin was replaced: {:?}",
            merged.known_hosts
        );
    }

    #[test]
    fn an_identical_pin_is_neither_duplicated_nor_a_conflict() {
        let existing = Payload {
            hosts: vec![],
            known_hosts: vec![pin("a", "SAME")],
        };
        let incoming = existing.clone();

        let merged = merge(&existing, &incoming);
        assert_eq!(merged.known_hosts.len(), 1);
        assert_eq!(merged.added_pins, 0);
        assert!(merged.conflicts.is_empty());
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let incoming = Payload {
            hosts: vec![],
            known_hosts: vec![
                "".into(),
                "   ".into(),
                "# a comment".into(),
                pin("a", "AAAA"),
            ],
        };
        assert_eq!(merge(&Payload::default(), &incoming).added_pins, 1);
    }
}
