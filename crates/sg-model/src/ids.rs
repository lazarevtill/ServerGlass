//! Stable string identifiers.
//!
//! Every id is a plain string newtype. They cross the FFI boundary constantly and end up as
//! dictionary keys in four different languages, so cleverness here (interning, integer handles)
//! would buy microseconds and cost portability.

use std::fmt;

macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({:?})", stringify!($name), self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
    };
}

string_id! {
    /// A monitored target — one host, cluster or endpoint the user added.
    TargetId
}

string_id! {
    /// A thing that has metrics: the host itself, a container, a disk, an interface, a GPU.
    ///
    /// Ids are scoped to their target and built by [`crate::Entity::child_id`], giving paths like
    /// `host/disk:nvme0n1` or `host/container:a1b2c3`.
    EntityId
}

string_id! {
    /// One measured quantity on one entity, e.g. `host/cpu:3` + `usage`.
    SeriesId
}

string_id! {
    /// A collector. Built-ins use stable slugs (`proc.cpu`); probes and plugins are namespaced
    /// (`probe.my-queue-depth`, `plugin.acme.widgets`).
    SourceId
}

impl EntityId {
    /// Build a child id underneath this entity.
    ///
    /// ```
    /// # use sg_model::EntityId;
    /// let host = EntityId::new("host");
    /// assert_eq!(host.child("disk", "nvme0n1").as_str(), "host/disk:nvme0n1");
    /// ```
    pub fn child(&self, kind: &str, name: &str) -> EntityId {
        EntityId::new(format!("{}/{}:{}", self.0, kind, name))
    }
}

impl SeriesId {
    /// Build the series id for a metric on an entity.
    ///
    /// ```
    /// # use sg_model::{EntityId, SeriesId};
    /// let core = EntityId::new("host").child("cpu", "3");
    /// assert_eq!(SeriesId::of(&core, "usage").as_str(), "host/cpu:3#usage");
    /// ```
    pub fn of(entity: &EntityId, metric: &str) -> SeriesId {
        SeriesId::new(format!("{}#{}", entity.as_str(), metric))
    }
}
