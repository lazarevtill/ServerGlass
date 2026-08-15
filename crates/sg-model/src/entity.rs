//! The entity tree.
//!
//! Everything that can carry metrics is an entity: the host, each CPU core, each disk, each
//! container, each Kubernetes pod, each UPS. Entities form a tree rooted at the host.
//!
//! The point of the generic tree is that the UI renders *entities*, not hardcoded pages. A source
//! that introduces [`EntityKind::Custom`] gets drill-down navigation and grouping with no UI change
//! on any of the four platforms — which is what makes the plugin SDK viable.

use std::collections::BTreeMap;

use crate::EntityId;

/// What kind of thing an entity is. Drives grouping, iconography and default sort in the UI.
#[derive(Clone, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    /// The monitored machine itself. Root of every tree.
    Host,
    /// One logical CPU.
    CpuCore,
    /// A physical or virtual block device.
    Disk,
    /// A mounted filesystem.
    Filesystem,
    NetworkInterface,
    Gpu,
    Process,
    /// Docker or Podman container.
    Container,
    /// Kubernetes pod.
    Pod,
    /// Kubernetes node.
    Node,
    /// A systemd unit or equivalent long-running service.
    Service,
    /// A guest under Proxmox, libvirt or similar.
    VirtualMachine,
    /// A ZFS pool, mdraid array or hardware RAID volume.
    StoragePool,
    /// A database instance being introspected.
    Database,
    /// An HTTP/TCP/ICMP check target.
    Endpoint,
    /// An uninterruptible power supply.
    Ups,
    /// A temperature, fan or voltage sensor.
    Sensor,
    /// Anything a probe or plugin invents. The string is shown as the group heading.
    Custom(String),
}

impl EntityKind {
    /// Slug used when composing child ids, and as the icon lookup key in each UI.
    pub fn slug(&self) -> &str {
        match self {
            EntityKind::Host => "host",
            EntityKind::CpuCore => "cpu",
            EntityKind::Disk => "disk",
            EntityKind::Filesystem => "fs",
            EntityKind::NetworkInterface => "net",
            EntityKind::Gpu => "gpu",
            EntityKind::Process => "proc",
            EntityKind::Container => "container",
            EntityKind::Pod => "pod",
            EntityKind::Node => "node",
            EntityKind::Service => "service",
            EntityKind::VirtualMachine => "vm",
            EntityKind::StoragePool => "pool",
            EntityKind::Database => "db",
            EntityKind::Endpoint => "endpoint",
            EntityKind::Ups => "ups",
            EntityKind::Sensor => "sensor",
            EntityKind::Custom(s) => s,
        }
    }
}

/// A node in the entity tree.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Entity {
    pub id: EntityId,
    pub kind: EntityKind,
    /// Human-readable name: `nvme0n1`, `eth0`, `postgres-primary`.
    pub display: String,
    /// `None` only for the host root.
    pub parent: Option<EntityId>,
    /// Free-form metadata shown on the detail page: model, serial, image, IP.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

impl Entity {
    /// The root host entity for a target.
    pub fn host(display: impl Into<String>) -> Self {
        Entity {
            id: EntityId::new("host"),
            kind: EntityKind::Host,
            display: display.into(),
            parent: None,
            labels: BTreeMap::new(),
        }
    }

    /// A child of `parent`, with its id derived from the parent's.
    ///
    /// ```
    /// # use sg_model::{Entity, EntityKind};
    /// let host = Entity::host("web-01");
    /// let disk = Entity::child(&host, EntityKind::Disk, "nvme0n1");
    /// assert_eq!(disk.id.as_str(), "host/disk:nvme0n1");
    /// assert_eq!(disk.parent.as_ref().unwrap(), &host.id);
    /// ```
    pub fn child(parent: &Entity, kind: EntityKind, name: impl Into<String>) -> Self {
        let name = name.into();
        Entity {
            id: parent.id.child(kind.slug(), &name),
            kind,
            display: name,
            parent: Some(parent.id.clone()),
            labels: BTreeMap::new(),
        }
    }

    /// Builder-style label attachment.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }
}
