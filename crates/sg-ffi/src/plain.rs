//! Plain language.
//!
//! A person who does not know what SSH is also does not know what a load average, a context switch
//! or swap is — and telling them "Load 1m: 0.26" is not information, it is noise wearing a number.
//!
//! Everything here turns readings into sentences a non-technical person can act on. It lives in
//! the core rather than in each UI for the same reason the formatting does: four front-ends that
//! each invent their own wording will drift, and the wording *is* the product for this audience.
//!
//! Two rules held throughout:
//!
//! - **Never state a problem without stating the size of it.** "Storage almost full" is an alarm;
//!   "Storage is almost full — 6 GB free of 150 GB" is something to act on.
//! - **Never blame the user for a failure the app cannot diagnose.** "Can't reach this server"
//!   with a plausible next step beats surfacing `Ssh(Disconnect)`.

use crate::view::MetricGauge;
use crate::ConnectionState;

/// How a host is doing, in one word and one sentence.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct HostHealth {
    /// `ok`, `busy`, `problem`, `offline`, `checking`.
    pub level: String,
    /// Four or five words: "Everything looks good".
    pub headline: String,
    /// One sentence with the numbers in it, or empty when there is nothing to add.
    pub detail: String,
}

/// Thresholds, named so the numbers are not scattered through the logic.
const NEARLY_FULL: f64 = 90.0;
const FILLING_UP: f64 = 80.0;
const WORKING_HARD: f64 = 85.0;

/// Assess a host from its headline gauges.
///
/// Ordered worst-first: a host that is both busy and nearly out of disk should say the thing that
/// will take it down, not the thing that will pass.
pub fn assess(state: &ConnectionState, gauges: &[MetricGauge], has_data: bool) -> HostHealth {
    let health = |level: &str, headline: &str, detail: String| HostHealth {
        level: level.into(),
        headline: headline.into(),
        detail,
    };

    match state {
        ConnectionState::Idle => return health("checking", "Not connected", String::new()),
        ConnectionState::Connecting => {
            return health(
                "checking",
                "Connecting…",
                "Getting the first reading.".into(),
            )
        }
        ConnectionState::Reconnecting { .. } => {
            return health(
                "offline",
                "Lost connection",
                "Trying again automatically.".into(),
            )
        }
        ConnectionState::Failed { message, .. } => {
            return health(
                "offline",
                "Can't reach this server",
                friendly_failure(message),
            )
        }
        ConnectionState::Online => {}
    }

    if !has_data {
        return health("checking", "Getting readings…", String::new());
    }

    let value = |metric: &str| gauges.iter().find(|g| g.metric == metric).map(|g| g.value);
    let pair = |used: &str, total: &str| {
        let used = gauges.iter().find(|g| g.metric == used)?.value;
        let total = gauges.iter().find(|g| g.metric == total)?.value;
        Some(format!(
            "{} free of {}",
            crate::format_value(total - used, "B", true),
            crate::format_value(total, "B", true)
        ))
    };

    if let Some(disk) = value("disk_usage") {
        if disk >= NEARLY_FULL {
            return health(
                "problem",
                "Storage is almost full",
                format!(
                    "The main drive is {disk:.0}% full. Things stop working when it fills up \
                     completely, so it is worth clearing space soon."
                ),
            );
        }
    }
    if let Some(memory) = value("mem_usage") {
        if memory >= NEARLY_FULL {
            let sizes = pair("mem_used", "mem_total").unwrap_or_default();
            return health(
                "problem",
                "Running out of memory",
                format!("Memory is {memory:.0}% used. {sizes}.")
                    .trim_end_matches(" .")
                    .to_string(),
            );
        }
    }

    if let Some(disk) = value("disk_usage") {
        if disk >= FILLING_UP {
            return health(
                "busy",
                "Storage is filling up",
                format!("The main drive is {disk:.0}% full — worth keeping an eye on."),
            );
        }
    }
    if let Some(memory) = value("mem_usage") {
        if memory >= WORKING_HARD {
            return health(
                "busy",
                "Memory is under pressure",
                format!("Memory is {memory:.0}% used."),
            );
        }
    }
    if let Some(cpu) = value("cpu_usage") {
        if cpu >= WORKING_HARD {
            return health(
                "busy",
                "Working very hard",
                format!("The processor is {cpu:.0}% busy. That is fine for a while, but slow if it stays there."),
            );
        }
    }

    let uptime = value("uptime")
        .map(|s| format!("Running for {}.", crate::format_uptime(s)))
        .unwrap_or_default();
    health("ok", "Everything looks good", uptime)
}

/// Turn a transport failure into something a person can act on.
///
/// The raw errors are accurate and useless to this audience: `Ssh(Disconnect)`,
/// `authentication failed for root@10.0.0.4`. Each of these says what to try next instead.
pub fn friendly_failure(message: &str) -> String {
    let lower = message.to_lowercase();

    if lower.contains("authentication failed") || lower.contains("no usable identity") {
        "The username or key was not accepted. Check the sign-in details are the ones this \
         server expects."
            .into()
    } else if lower.contains("not in known_hosts") {
        "This is the first time connecting to this server, so its identity has not been \
         confirmed yet. Turn on \"Trust this server\" when adding it if you know it is the right \
         one."
            .into()
    } else if lower.contains("changed") {
        "This server's identity is different from last time. That can mean it was rebuilt — or \
         that something is impersonating it. Do not reconnect until you know which."
            .into()
    } else if lower.contains("could not reach") || lower.contains("timed out") {
        "Nothing answered at that address. Check the server is switched on and that you are on \
         the same network or VPN."
            .into()
    } else if lower.contains("no ssh-agent") {
        "No key agent is running, so there is no key to sign in with. Choose a key file instead, \
         or start your key agent."
            .into()
    } else if lower.contains("could not read private key") {
        "That key file could not be read. Check the path is right and the passphrase matches."
            .into()
    } else if lower.contains("closed") {
        "The server closed the connection. It may be restarting.".into()
    } else {
        // Better an honest fallback than a confidently wrong guess.
        format!("Something went wrong connecting. The technical detail was: {message}")
    }
}

/// A person-facing name for a metric, and whether it belongs on a simple screen.
///
/// Returns `None` for anything with no non-technical meaning — swap, load average, context
/// switches, socket counts. Those are real and stay available under the technical view; they just
/// have no business being the first thing someone sees.
pub fn plain_name(metric: &str) -> Option<&'static str> {
    match metric {
        "cpu_usage" => Some("Processor"),
        "mem_usage" => Some("Memory"),
        "disk_usage" => Some("Storage"),
        "uptime" => Some("Running for"),
        "net_rx" => Some("Downloading"),
        "net_tx" => Some("Uploading"),
        _ => None,
    }
}

/// A sentence describing one reading, for the simple view's tiles.
pub fn plain_summary(gauge: &MetricGauge, used: Option<f64>, total: Option<f64>) -> String {
    let bytes = |v: f64| crate::format_value(v, "B", true);

    match gauge.metric.as_str() {
        "cpu_usage" => match gauge.value {
            v if v < 25.0 => "Barely working".into(),
            v if v < 60.0 => "Working normally".into(),
            v if v < 85.0 => "Working hard".into(),
            _ => "Very busy".into(),
        },
        "mem_usage" | "disk_usage" => match (used, total) {
            (Some(used), Some(total)) if total > 0.0 => {
                format!("{} free of {}", bytes(total - used), bytes(total))
            }
            _ => format!("{:.0}% used", gauge.value),
        },
        "uptime" => "Without a restart".into(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sg_model::Unit;

    fn gauge(metric: &str, value: f64) -> MetricGauge {
        MetricGauge {
            series_id: metric.into(),
            metric: metric.into(),
            label: metric.into(),
            value,
            max: Some(100.0),
            unit_suffix: Unit::Percent.suffix().into(),
            binary_scaled: false,
            history: vec![],
        }
    }

    fn bytes(metric: &str, value: f64) -> MetricGauge {
        MetricGauge {
            max: None,
            unit_suffix: "B".into(),
            binary_scaled: true,
            ..gauge(metric, value)
        }
    }

    #[test]
    fn a_healthy_host_says_so_plainly() {
        let health = assess(
            &ConnectionState::Online,
            &[
                gauge("cpu_usage", 4.0),
                gauge("mem_usage", 20.0),
                gauge("disk_usage", 30.0),
            ],
            true,
        );
        assert_eq!(health.level, "ok");
        assert_eq!(health.headline, "Everything looks good");
    }

    /// The worst thing wins: a host that is both busy and nearly out of disk should be told about
    /// the disk, because that is what takes it down.
    #[test]
    fn the_most_serious_problem_is_the_one_reported() {
        let health = assess(
            &ConnectionState::Online,
            &[
                gauge("cpu_usage", 99.0),
                gauge("mem_usage", 50.0),
                gauge("disk_usage", 95.0),
            ],
            true,
        );
        assert_eq!(health.level, "problem");
        assert_eq!(health.headline, "Storage is almost full");
        assert!(health.detail.contains("95%"));
    }

    /// A warning with no magnitude is an alarm, not information.
    #[test]
    fn problems_always_carry_their_size() {
        let health = assess(
            &ConnectionState::Online,
            &[
                gauge("mem_usage", 94.0),
                bytes("mem_used", 60.0 * 1024.0 * 1024.0 * 1024.0),
                bytes("mem_total", 64.0 * 1024.0 * 1024.0 * 1024.0),
            ],
            true,
        );
        assert_eq!(health.headline, "Running out of memory");
        assert!(health.detail.contains("94%"), "{}", health.detail);
        assert!(
            health.detail.contains("GiB"),
            "no sizes in {:?}",
            health.detail
        );
    }

    #[test]
    fn intermediate_pressure_is_a_nudge_not_an_alarm() {
        let busy = assess(&ConnectionState::Online, &[gauge("disk_usage", 84.0)], true);
        assert_eq!(busy.level, "busy");
        assert_eq!(busy.headline, "Storage is filling up");

        let fine = assess(&ConnectionState::Online, &[gauge("disk_usage", 60.0)], true);
        assert_eq!(fine.level, "ok");
    }

    #[test]
    fn connection_states_are_described_without_jargon() {
        for (state, level) in [
            (ConnectionState::Idle, "checking"),
            (ConnectionState::Connecting, "checking"),
            (
                ConnectionState::Reconnecting {
                    attempt: 2,
                    retry_in_ms: 1000,
                },
                "offline",
            ),
        ] {
            let health = assess(&state, &[], false);
            assert_eq!(health.level, level);
            assert!(!health.headline.is_empty());
            // No acronyms or raw identifiers in anything a person reads.
            assert!(!health.headline.contains("SSH") && !health.headline.contains('_'));
        }
    }

    /// The raw errors are accurate and useless: nobody can act on `Ssh(Disconnect)`.
    #[test]
    fn failures_are_translated_into_next_steps() {
        let auth = friendly_failure("authentication failed for root@10.0.0.4");
        assert!(auth.contains("not accepted"), "{auth}");
        assert!(!auth.contains("root@"), "raw error leaked: {auth}");

        let unknown = friendly_failure("host key for h:22 is not in known_hosts");
        assert!(unknown.contains("first time"), "{unknown}");

        let changed = friendly_failure("host key for h:22 CHANGED — possible interception");
        assert!(changed.contains("impersonating"), "{changed}");

        let unreachable = friendly_failure("could not reach 10.0.0.4:22: connection refused");
        assert!(unreachable.contains("switched on"), "{unreachable}");
    }

    /// An unrecognised failure must not be dressed up as something understood.
    #[test]
    fn unknown_failures_keep_the_technical_detail() {
        let odd = friendly_failure("kex algorithm negotiation failed");
        assert!(
            odd.contains("kex algorithm"),
            "detail was thrown away: {odd}"
        );
    }

    #[test]
    fn only_metrics_with_a_lay_meaning_get_a_plain_name() {
        assert_eq!(plain_name("cpu_usage"), Some("Processor"));
        assert_eq!(plain_name("disk_usage"), Some("Storage"));
        // Real, useful, and meaningless to this audience.
        assert_eq!(plain_name("load1"), None);
        assert_eq!(plain_name("swap_usage"), None);
        assert_eq!(plain_name("ctx_switches"), None);
        assert_eq!(plain_name("tcp_timewait"), None);
    }

    #[test]
    fn summaries_describe_rather_than_restate() {
        assert_eq!(
            plain_summary(&gauge("cpu_usage", 3.0), None, None),
            "Barely working"
        );
        assert_eq!(
            plain_summary(&gauge("cpu_usage", 95.0), None, None),
            "Very busy"
        );

        let free = plain_summary(
            &gauge("disk_usage", 50.0),
            Some(50.0 * 1024.0 * 1024.0 * 1024.0),
            Some(100.0 * 1024.0 * 1024.0 * 1024.0),
        );
        assert_eq!(free, "50.0 GiB free of 100.0 GiB");
    }
}
