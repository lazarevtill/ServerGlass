//! Units of measure.
//!
//! Units live in the model rather than the UI so that all four front-ends format a byte rate the
//! same way, and so a plugin author can declare `BytesPerSecond` and get correct axis scaling for
//! free.

/// The physical unit of a series' values.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    /// Dimensionless 0–100.
    Percent,
    /// Dimensionless 0–1.
    Ratio,
    Bytes,
    BytesPerSecond,
    Bits,
    BitsPerSecond,
    Packets,
    PacketsPerSecond,
    /// A plain tally.
    Count,
    CountPerSecond,
    /// Completed I/O operations.
    Operations,
    OperationsPerSecond,
    Seconds,
    Milliseconds,
    Celsius,
    Watts,
    Volts,
    Amperes,
    Hertz,
    /// Revolutions per minute — fans, spindles.
    Rpm,
    /// No unit at all (state, info, opaque text).
    None,
}

impl Unit {
    /// The unit a counter turns into once differentiated with respect to time.
    ///
    /// The scheduler uses this when it converts a [`crate::SeriesKind::Counter`] into a rate, so a
    /// source only ever declares the unit of the raw reading it took.
    ///
    /// ```
    /// # use sg_model::Unit;
    /// assert_eq!(Unit::Bytes.per_second(), Unit::BytesPerSecond);
    /// assert_eq!(Unit::Celsius.per_second(), Unit::Celsius);
    /// ```
    pub fn per_second(self) -> Unit {
        match self {
            Unit::Bytes => Unit::BytesPerSecond,
            Unit::Bits => Unit::BitsPerSecond,
            Unit::Packets => Unit::PacketsPerSecond,
            Unit::Count => Unit::CountPerSecond,
            Unit::Operations => Unit::OperationsPerSecond,
            // Already a rate, or not meaningfully differentiable.
            other => other,
        }
    }

    /// Whether values scale by 1024 rather than 1000.
    pub fn is_binary_scaled(self) -> bool {
        matches!(self, Unit::Bytes | Unit::BytesPerSecond)
    }

    /// Suffix for display, e.g. `%`, `B/s`, `°C`. Empty for [`Unit::None`].
    pub fn suffix(self) -> &'static str {
        match self {
            Unit::Percent => "%",
            Unit::Ratio => "",
            Unit::Bytes => "B",
            Unit::BytesPerSecond => "B/s",
            Unit::Bits => "b",
            Unit::BitsPerSecond => "b/s",
            Unit::Packets => "pkt",
            Unit::PacketsPerSecond => "pkt/s",
            Unit::Count => "",
            Unit::CountPerSecond => "/s",
            Unit::Operations => "IO",
            Unit::OperationsPerSecond => "IO/s",
            Unit::Seconds => "s",
            Unit::Milliseconds => "ms",
            Unit::Celsius => "°C",
            Unit::Watts => "W",
            Unit::Volts => "V",
            Unit::Amperes => "A",
            Unit::Hertz => "Hz",
            Unit::Rpm => "rpm",
            Unit::None => "",
        }
    }

    /// Upper bound for gauge rendering, when one is inherent to the unit.
    ///
    /// Series without an inherent maximum (a byte rate, a temperature) return `None` and the UI
    /// scales them against the observed window instead.
    pub fn natural_max(self) -> Option<f64> {
        match self {
            Unit::Percent => Some(100.0),
            Unit::Ratio => Some(1.0),
            _ => None,
        }
    }
}
