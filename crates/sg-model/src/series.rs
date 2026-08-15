//! Series descriptors and sample values.

use std::collections::BTreeMap;

use crate::{EntityId, SeriesId, SourceId, Unit};

/// How a series' raw readings should be interpreted over time.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeriesKind {
    /// Reading stands on its own: temperature, free memory, load average.
    Gauge,
    /// Monotonically increasing tally. The scheduler differentiates it into a rate and applies
    /// [`Unit::per_second`]; sources never compute rates themselves.
    Counter,
    /// A discrete condition — `running`, `degraded`, `unreachable`.
    State,
    /// Constant descriptive text: kernel version, container image, disk model.
    Info,
}

/// A measured value.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "t", content = "v")]
pub enum Value {
    Float(f64),
    Int(i64),
    /// Counters are unsigned and frequently exceed `i64` on long-uptime hosts.
    Uint(u64),
    Bool(bool),
    Text(String),
}

impl Value {
    /// Numeric view of the value, for charting and rate computation.
    ///
    /// `Bool` maps to 1.0/0.0; `Text` has no numeric meaning and yields `None`.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(v) => Some(*v),
            Value::Int(v) => Some(*v as f64),
            Value::Uint(v) => Some(*v as f64),
            Value::Bool(v) => Some(if *v { 1.0 } else { 0.0 }),
            Value::Text(_) => None,
        }
    }

    /// Counter view, for rate computation. Only unsigned readings qualify.
    pub fn as_counter(&self) -> Option<u64> {
        match self {
            Value::Uint(v) => Some(*v),
            Value::Int(v) if *v >= 0 => Some(*v as u64),
            _ => None,
        }
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}

impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Value::Uint(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Text(v)
    }
}

/// Everything the UI needs to render a series without consulting the source that produced it.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct SeriesDescriptor {
    pub id: SeriesId,
    pub source: SourceId,
    pub entity: EntityId,
    /// Stable machine name within the entity: `usage`, `rx_bytes`, `temp`.
    pub metric: String,
    /// Human-readable label: "Usage", "Received", "Temperature".
    pub display: String,
    pub unit: Unit,
    pub kind: SeriesKind,
    /// Upper bound for gauge rendering when known — total bytes for a filesystem, core count for
    /// a load average. Falls back to [`Unit::natural_max`], then to window-relative scaling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Multiplier applied to the rate of a [`SeriesKind::Counter`], after differentiation.
    ///
    /// This is what keeps CPU collection stateless. CPU usage is not a time rate but a ratio of
    /// two deltas — busy jiffies over elapsed jiffies — which a source could only compute by
    /// remembering its previous reading. Instead the source emits raw busy jiffies as a counter
    /// and sets `scale = 100 / clock_ticks`: the scheduler differentiates to jiffies-per-second,
    /// the scale turns that into percent of one core, and the source stays a pure function.
    ///
    /// Ignored for every other [`SeriesKind`].
    #[serde(default = "unit_scale")]
    pub scale: f64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

fn unit_scale() -> f64 {
    1.0
}

impl SeriesDescriptor {
    /// A gauge series on an entity. `metric` doubles as the default display name, title-cased by
    /// the UI, so most sources need only this constructor.
    pub fn gauge(
        source: &SourceId,
        entity: &EntityId,
        metric: impl Into<String>,
        display: impl Into<String>,
        unit: Unit,
    ) -> Self {
        let metric = metric.into();
        SeriesDescriptor {
            id: SeriesId::of(entity, &metric),
            source: source.clone(),
            entity: entity.clone(),
            metric,
            display: display.into(),
            unit,
            kind: SeriesKind::Gauge,
            max: None,
            scale: 1.0,
            labels: BTreeMap::new(),
        }
    }

    /// A counter series. The scheduler will differentiate it; `unit` is the unit of the raw
    /// reading, not of the resulting rate.
    pub fn counter(
        source: &SourceId,
        entity: &EntityId,
        metric: impl Into<String>,
        display: impl Into<String>,
        unit: Unit,
    ) -> Self {
        SeriesDescriptor {
            kind: SeriesKind::Counter,
            ..Self::gauge(source, entity, metric, display, unit)
        }
    }

    /// Descriptive text that does not change between ticks.
    pub fn info(
        source: &SourceId,
        entity: &EntityId,
        metric: impl Into<String>,
        display: impl Into<String>,
    ) -> Self {
        SeriesDescriptor {
            kind: SeriesKind::Info,
            ..Self::gauge(source, entity, metric, display, Unit::None)
        }
    }

    /// Builder-style upper bound.
    pub fn with_max(mut self, max: f64) -> Self {
        self.max = Some(max);
        self
    }

    /// Builder-style rate scale. See [`SeriesDescriptor::scale`].
    pub fn with_scale(mut self, scale: f64) -> Self {
        self.scale = scale;
        self
    }

    /// Builder-style label.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// The unit values carry *after* the scheduler has processed them. A counter's readings are
    /// raw, but what reaches the UI is a rate, so the UI must format against this.
    ///
    /// A counter that declares an explicit `scale` has already been converted into its declared
    /// unit by that scale — CPU jiffies scaled into percent stay percent, they do not become
    /// "percent per second".
    pub fn effective_unit(&self) -> Unit {
        match self.kind {
            SeriesKind::Counter if self.scale == 1.0 => self.unit.per_second(),
            _ => self.unit,
        }
    }

    /// The bound a gauge should be drawn against, if any.
    pub fn display_max(&self) -> Option<f64> {
        self.max.or_else(|| self.unit.natural_max())
    }
}
