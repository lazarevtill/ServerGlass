//! Temperatures, fan speeds and power draw, from `/sys/class/hwmon` and `/sys/class/thermal`.
//!
//! Temperature is the reading people actually go looking for and the one every other tool makes
//! hard to get. `lm-sensors` is the usual answer, and it is the wrong one here: it means installing
//! a package on the monitored host, which the first invariant forbids outright. The kernel already
//! exports every reading `sensors` prints, as plain files:
//!
//! ```text
//! /sys/class/hwmon/hwmon2/name          coretemp
//! /sys/class/hwmon/hwmon2/temp1_input   45000       millidegrees C
//! /sys/class/hwmon/hwmon2/temp1_label   Package id 0
//! /sys/class/hwmon/hwmon2/temp1_crit    100000      the manufacturer's limit
//! /sys/class/hwmon/hwmon2/fan1_input    1183        RPM
//! /sys/class/hwmon/hwmon2/power1_input  8500000     microwatts
//! ```
//!
//! `/sys/class/thermal/thermal_zone*` is read as well, and is not redundant: virtual machines and
//! most ARM boards expose a thermal zone and no hwmon chip at all, so a host that would otherwise
//! report no temperature still reports one.
//!
//! What is deliberately *not* here: voltages (`in*_input`) and current (`curr*_input`). A modern
//! board reports a few dozen of them, they are meaningless without the board's own tolerances, and
//! burying two useful temperatures under thirty rail voltages is how a dashboard stops being read.

use sg_model::{
    Entity, EntityKind, ParseResult, Request, Requirements, Responses, SampleSink,
    SeriesDescriptor, Source, SourceDescriptor, SourceId, TargetCtx, Unit,
};

/// One reading from one chip.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    /// `coretemp`, `nvme`, `x86_pkg_temp` — the chip or thermal zone it came from.
    pub chip: String,
    /// `temp1`, `fan2`, `power1`.
    pub key: String,
    /// The chip's own name for it: `Package id 0`, `Composite`. Empty when it has none.
    pub label: String,
    /// Already converted: degrees Celsius, RPM, or watts.
    pub value: f64,
    /// The manufacturer's limit, in the same unit. Only ever present for temperatures.
    pub critical: Option<f64>,
}

impl Reading {
    /// What kind of reading this is, from the sysfs attribute name.
    pub fn quantity(&self) -> Quantity {
        if self.key.starts_with("temp") {
            Quantity::Temperature
        } else if self.key.starts_with("fan") {
            Quantity::Fan
        } else {
            Quantity::Power
        }
    }

    /// A name a person would recognise.
    ///
    /// The chip's own label when it has one — `Package id 0` says more than `temp1` ever will —
    /// and the chip name with the attribute otherwise, because bare `temp1` on a host with four
    /// chips identifies nothing.
    pub fn display(&self) -> String {
        if self.label.is_empty() {
            format!("{} {}", self.chip, self.key)
        } else {
            self.label.clone()
        }
    }

    /// Whether this is a CPU package or core temperature.
    ///
    /// Named chips rather than a guess at labels: `coretemp` is Intel, `k10temp` and `zenpower`
    /// are AMD, `cpu_thermal` is the Raspberry Pi and friends, and `x86_pkg_temp` is the thermal
    /// zone the kernel exposes for the package. A `nvme` or `acpitz` reading is a real temperature
    /// but it is not the processor's, and reporting it as such would be worse than reporting none.
    pub fn is_cpu_temperature(&self) -> bool {
        const CPU_CHIPS: [&str; 6] = [
            "coretemp",
            "k10temp",
            "k8temp",
            "zenpower",
            "cpu_thermal",
            "x86_pkg_temp",
        ];
        self.quantity() == Quantity::Temperature && CPU_CHIPS.contains(&self.chip.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quantity {
    Temperature,
    Fan,
    Power,
}

impl Quantity {
    pub fn unit(self) -> Unit {
        match self {
            Quantity::Temperature => Unit::Celsius,
            Quantity::Fan => Unit::Rpm,
            Quantity::Power => Unit::Watts,
        }
    }

    /// The metric name, and so the series id suffix.
    pub fn metric(self) -> &'static str {
        match self {
            Quantity::Temperature => "temp",
            Quantity::Fan => "fan",
            Quantity::Power => "power",
        }
    }
}

/// Argv for the sensor sweep. A constant; nothing here comes from a host or from the user.
///
/// Pipe-separated rather than space-separated because labels contain spaces (`Package id 0`) and
/// `crit` is frequently absent — splitting on whitespace would shift every field left on exactly
/// the hosts whose readings are worth having.
///
/// The trailing `exit 0` is the usual guard: the loop's status is that of its last iteration, so a
/// chip without the last-listed attribute would otherwise discard the whole sweep.
pub const SENSORS_ARGV: [&str; 3] = [
    "sh",
    "-c",
    "for d in /sys/class/hwmon/hwmon*; do \
     [ -d \"$d\" ] || continue; \
     printf '#|%s\\n' \"$(cat \"$d/name\" 2>/dev/null)\"; \
     for f in \"$d\"/temp*_input \"$d\"/fan*_input \"$d\"/power*_input; do \
     [ -f \"$f\" ] || continue; \
     b=${f##*/}; b=${b%_input}; \
     printf '%s|%s|%s|%s\\n' \"$b\" \"$(cat \"$f\" 2>/dev/null)\" \
     \"$(cat \"$d/${b}_crit\" 2>/dev/null)\" \"$(cat \"$d/${b}_label\" 2>/dev/null)\"; \
     done; done; \
     for z in /sys/class/thermal/thermal_zone*; do \
     [ -d \"$z\" ] || continue; \
     printf '#|%s\\n' \"$(cat \"$z/type\" 2>/dev/null)\"; \
     printf 'temp|%s||\\n' \"$(cat \"$z/temp\" 2>/dev/null)\"; \
     done; exit 0",
];

/// Parse the framed sweep. Each chip starts with `#|<name>`.
pub fn parse_sensors(text: &str) -> Vec<Reading> {
    let mut out = Vec::new();
    let mut chip = String::new();

    for line in text.lines() {
        if let Some(name) = line.strip_prefix("#|") {
            chip = name.trim().to_string();
            continue;
        }
        // A chip whose `name` file is unreadable would otherwise attach its readings to whichever
        // chip came before it.
        if chip.is_empty() {
            continue;
        }

        let mut fields = line.split('|');
        let (Some(key), Some(raw)) = (fields.next(), fields.next()) else {
            continue;
        };
        let Ok(raw) = raw.trim().parse::<f64>() else {
            continue;
        };
        let critical = fields.next().and_then(|c| c.trim().parse::<f64>().ok());
        let label = fields.next().unwrap_or_default().trim().to_string();

        let mut reading = Reading {
            chip: chip.clone(),
            key: key.trim().to_string(),
            label,
            value: raw,
            critical,
        };

        // Convert once, here, so nothing downstream has to know what unit sysfs chose.
        match reading.quantity() {
            Quantity::Temperature => {
                reading.value = raw / 1000.0;
                reading.critical = critical.map(|c| c / 1000.0);
            }
            Quantity::Power => {
                reading.value = raw / 1_000_000.0;
                reading.critical = None;
            }
            // Fans are already RPM, and have no meaningful ceiling.
            Quantity::Fan => reading.critical = None,
        }

        // A disconnected fan header reads 0 RPM forever and a disabled sensor reads exactly 0°C;
        // both are absence of a reading rather than a reading of zero.
        if reading.value > 0.0 {
            out.push(reading);
        }
    }

    out
}

/// The processor's temperature, when the host reports one.
///
/// The hottest CPU reading rather than the average: a package running at 95°C while three cores
/// idle at 40 is a machine about to throttle, and an average of 54 says it is fine.
pub fn cpu_temperature(readings: &[Reading]) -> Option<f64> {
    readings
        .iter()
        .filter(|r| r.is_cpu_temperature())
        .map(|r| r.value)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
}

pub struct SensorSource {
    descriptor: SourceDescriptor,
}

impl Default for SensorSource {
    fn default() -> Self {
        SensorSource {
            descriptor: SourceDescriptor {
                id: SourceId::new("sys.sensors"),
                display: "Sensors".into(),
                description: "Temperatures, fan speeds and power draw, straight from sysfs".into(),
                produces: vec![EntityKind::Sensor],
                // hwmon is the primary source; a host with only thermal zones still has the
                // directory, empty. Capability detection already probes for it.
                requires: Requirements::path("/sys/class/hwmon"),
                default_enabled: true,
            },
        }
    }
}

impl SensorSource {
    fn request() -> Request {
        Request::exec(SENSORS_ARGV)
    }
}

impl Source for SensorSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    fn requests(&self, _ctx: &TargetCtx) -> Vec<Request> {
        vec![Self::request()]
    }

    fn parse(&self, ctx: &TargetCtx, responses: &Responses, out: &mut SampleSink) -> ParseResult {
        let Some(text) = responses.text(&Self::request()) else {
            return Ok(());
        };
        let id = &self.descriptor.id;
        let readings = parse_sensors(text);

        // One host-level reading, so "how hot is it" is answerable without opening the sensor list
        // — and so the plain-language view has something to say about heat at all.
        if let Some(temperature) = cpu_temperature(&readings) {
            out.emit(
                SeriesDescriptor::gauge(
                    id,
                    &ctx.host.id,
                    "cpu_temp",
                    "CPU temperature",
                    Unit::Celsius,
                ),
                temperature,
            );
        }

        for reading in readings {
            let quantity = reading.quantity();
            let entity = Entity::child(&ctx.host, EntityKind::Sensor, &reading.display())
                .with_label("chip", &reading.chip);

            let mut gauge = SeriesDescriptor::gauge(
                id,
                &entity.id,
                quantity.metric(),
                &reading.display(),
                quantity.unit(),
            );
            // The manufacturer's limit is the only honest maximum for a temperature. Without one
            // the UI must scale against what it has seen, because there is no such thing as 100%
            // of a degree.
            if let Some(critical) = reading.critical {
                gauge = gauge.with_max(critical);
            }
            out.emit(gauge, reading.value);
            out.entity(entity);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{corpus, sink_for, value_of};

    /// A Proxmox host: an Intel package with four cores, an NVMe drive, a fan, and the thermal
    /// zone the same package also appears as.
    const SWEEP: &str = "\
#|coretemp
temp1|45000|100000|Package id 0
temp2|43000|100000|Core 0
temp3|47000|100000|Core 1
#|nvme
temp1|38850||Composite
#|nct6798
fan1|1183||
fan2|0||
power1|8500000||
#|x86_pkg_temp
temp|45000||
";

    #[test]
    fn parses_each_chip_with_its_own_labels() {
        let readings = parse_sensors(SWEEP);

        let package = &readings[0];
        assert_eq!(package.chip, "coretemp");
        assert_eq!(package.label, "Package id 0");
        assert_eq!(package.value, 45.0, "millidegrees become degrees");
        assert_eq!(package.critical, Some(100.0));

        let nvme = readings.iter().find(|r| r.chip == "nvme").unwrap();
        assert_eq!(nvme.value, 38.85);
        assert_eq!(nvme.critical, None, "no crit attribute, no ceiling");
    }

    /// Labels contain spaces, which is why the sweep is pipe-separated.
    #[test]
    fn a_label_with_spaces_survives() {
        let readings = parse_sensors(SWEEP);
        assert_eq!(readings[0].display(), "Package id 0");
    }

    /// An absent `crit` leaves an empty field, and must not shift the label into its place.
    #[test]
    fn a_missing_critical_does_not_shift_the_label() {
        let readings = parse_sensors("#|nvme\ntemp1|38850||Composite\n");
        assert_eq!(readings[0].critical, None);
        assert_eq!(readings[0].label, "Composite");
    }

    #[test]
    fn converts_each_quantity_to_the_unit_people_read() {
        let readings = parse_sensors(SWEEP);

        let fan = readings.iter().find(|r| r.key == "fan1").unwrap();
        assert_eq!(fan.quantity(), Quantity::Fan);
        assert_eq!(fan.value, 1183.0, "RPM is already RPM");

        let power = readings.iter().find(|r| r.key == "power1").unwrap();
        assert_eq!(power.quantity(), Quantity::Power);
        assert_eq!(power.value, 8.5, "microwatts become watts");
    }

    /// A header with nothing plugged into it reads 0 RPM forever. That is not a fan at rest, it is
    /// no fan; listing it invites someone to worry about a stopped fan that does not exist.
    #[test]
    fn a_disconnected_fan_is_not_listed() {
        let readings = parse_sensors(SWEEP);
        assert!(readings.iter().all(|r| r.key != "fan2"));
    }

    /// The hottest CPU reading, not the average and not the hottest thing in the machine.
    #[test]
    fn cpu_temperature_ignores_everything_that_is_not_the_processor() {
        let readings = parse_sensors(
            "#|coretemp\ntemp1|45000|100000|Package id 0\ntemp2|61000|100000|Core 0\n\
             #|nvme\ntemp1|72000||Composite\n",
        );
        assert_eq!(
            cpu_temperature(&readings),
            Some(61.0),
            "a hot NVMe drive is not the CPU, and the hottest core is the one that throttles"
        );
    }

    #[test]
    fn a_host_with_no_sensors_produces_nothing() {
        let (ctx, responses) = corpus("debian").exec_literal(&SENSORS_ARGV, "").build();
        let out = sink_for(&SensorSource::default(), &ctx, &responses);
        assert!(out.is_empty());
    }

    /// A VM has no hwmon chip at all and only a thermal zone. Reporting no temperature there was
    /// the reason this reads both.
    #[test]
    fn a_thermal_zone_alone_still_reports_a_temperature() {
        let (ctx, responses) = corpus("debian")
            .exec_literal(&SENSORS_ARGV, "#|x86_pkg_temp\ntemp|52000||\n")
            .build();
        let out = sink_for(&SensorSource::default(), &ctx, &responses);
        assert_eq!(value_of(&out, "cpu_temp"), Some(52.0));
    }

    #[test]
    fn emits_an_entity_per_reading_with_its_chip_recorded() {
        let (ctx, responses) = corpus("debian").exec_literal(&SENSORS_ARGV, SWEEP).build();
        let out = sink_for(&SensorSource::default(), &ctx, &responses);

        let composite = out
            .entities
            .iter()
            .find(|e| e.display == "Composite")
            .expect("the NVMe reading is listed");
        assert_eq!(composite.kind, EntityKind::Sensor);
        assert_eq!(composite.labels.get("chip").map(String::as_str), Some("nvme"));

        assert_eq!(value_of(&out, "cpu_temp"), Some(47.0));
    }

    /// A fan with no label would be listed as a bare `fan1` on a host with three chips.
    #[test]
    fn an_unlabelled_reading_is_named_after_its_chip() {
        let readings = parse_sensors(SWEEP);
        let fan = readings.iter().find(|r| r.key == "fan1").unwrap();
        assert_eq!(fan.display(), "nct6798 fan1");
    }

    #[test]
    fn the_sweep_normalises_its_exit_status() {
        let script = SENSORS_ARGV[2];
        assert!(script.trim_end().ends_with("exit 0"), "{script}");
    }

    #[test]
    fn is_gated_on_hwmon() {
        let source = SensorSource::default();
        let mut caps = sg_model::Capabilities::default();
        assert!(!source.descriptor().requires.satisfied_by(&caps));
        caps.paths.insert("/sys/class/hwmon".into());
        assert!(source.descriptor().requires.satisfied_by(&caps));
    }
}
