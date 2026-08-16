//! What crosses the wire.

use serde::{Deserialize, Serialize};

/// One server, without its credential.
///
/// Mirrors the record each platform already stores, minus the secret — which is not omitted by
/// oversight but is the point: the receiving device asks for it once and keeps it in its own
/// keystore, so a transfer never puts a password anywhere it was not already.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncHost {
    pub address: String,
    pub port: u16,
    pub user: String,
    /// `agent`, `key`, `key_text` or `password` — which credential to ask for.
    pub auth_kind: String,
    /// Only meaningful for `key`, and only on a device where that path exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
    pub host_key_policy: String,
    pub refresh_ms: u64,
}

impl SyncHost {
    /// What makes two records the same server.
    ///
    /// Not the identifier: those are generated per device, so matching on them would duplicate
    /// every host on every transfer.
    pub fn identity(&self) -> (String, u16, String) {
        (self.address.clone(), self.port, self.user.clone())
    }
}

/// The transferable half of one device's state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payload {
    pub hosts: Vec<SyncHost>,
    /// `known_hosts` lines, verbatim. Public keys — no secrecy, but the integrity of these is what
    /// makes a later impersonation detectable.
    #[serde(default)]
    pub known_hosts: Vec<String>,
}

impl Payload {
    pub fn to_json(&self) -> Result<Vec<u8>, crate::SyncError> {
        serde_json::to_vec(self).map_err(|e| crate::SyncError::Malformed(e.to_string()))
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, crate::SyncError> {
        serde_json::from_slice(bytes).map_err(|e| crate::SyncError::Malformed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> SyncHost {
        SyncHost {
            address: "10.0.0.9".into(),
            port: 2222,
            user: "root".into(),
            auth_kind: "password".into(),
            key_path: None,
            host_key_policy: "accept_new".into(),
            refresh_ms: 1000,
        }
    }

    #[test]
    fn a_payload_survives_the_round_trip() {
        let payload = Payload {
            hosts: vec![host()],
            known_hosts: vec!["[10.0.0.9]:2222 ssh-ed25519 AAAA".into()],
        };
        assert_eq!(
            Payload::from_json(&payload.to_json().unwrap()).unwrap(),
            payload
        );
    }

    /// The whole design rests on credentials never travelling, and the serialised form is where
    /// that is either true or not.
    ///
    /// Asserts the exact field set rather than searching for words: `auth_kind` legitimately *has*
    /// the value "password", so a substring search reports a secret that is not there — and, worse,
    /// would still pass if someone added a field called `pw`. Adding a field to `SyncHost` fails
    /// this test on purpose, so that the person adding it has to think about whether it should
    /// leave the device.
    #[test]
    fn the_wire_format_carries_exactly_the_fields_it_should() {
        let json = Payload {
            hosts: vec![host()],
            known_hosts: vec![],
        }
        .to_json()
        .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&json).unwrap();

        let mut fields: Vec<&str> = value["hosts"][0]
            .as_object()
            .expect("a host object")
            .keys()
            .map(String::as_str)
            .collect();
        fields.sort_unstable();

        assert_eq!(
            fields,
            [
                "address",
                "auth_kind",
                "host_key_policy",
                "port",
                "refresh_ms",
                "user"
            ],
            "the set of transferred fields changed — is the new one a secret?"
        );
    }

    /// A key path is a path, not a key: it names a file that exists on one device and probably not
    /// on the other. It travels so the receiving device can show what the original was set to.
    #[test]
    fn a_key_path_travels_but_a_key_never_does() {
        let mut with_path = host();
        with_path.auth_kind = "key".into();
        with_path.key_path = Some("/home/me/.ssh/id_ed25519".into());

        let json = String::from_utf8(
            Payload {
                hosts: vec![with_path],
                known_hosts: vec![],
            }
            .to_json()
            .unwrap(),
        )
        .unwrap();

        assert!(json.contains("/home/me/.ssh/id_ed25519"));
        // The key itself is not a field of SyncHost at all, and there is no way to put one there.
        assert!(!json.contains("BEGIN"));
    }

    /// A record written by a newer version must not stop the transfer.
    #[test]
    fn unknown_fields_are_ignored_rather_than_fatal() {
        let json = br#"{"hosts":[{"address":"a","port":22,"user":"u","auth_kind":"password","host_key_policy":"strict","refresh_ms":1000,"future_field":true}],"known_hosts":[]}"#;
        assert_eq!(Payload::from_json(json).unwrap().hosts.len(), 1);
    }
}
