//! View models handed to the UIs.
//!
//! These are deliberately UI-shaped: a gauge already knows its label, its formatted range and its
//! sparkline. Building them here rather than in Swift/Kotlin/C# is what keeps the third invariant
//! — the core owns all logic — from eroding one convenience method at a time in four codebases.

use sg_core::{LiveStore, TargetState};
use sg_model::{Entity, EntityId, EntityKind, SeriesDescriptor, SeriesKind, Unit};

/// How to reach a host.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TargetConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    /// `"agent"`, `"key"`, `"key_text"` or `"password"`.
    pub auth_kind: String,
    /// Path to a private key, when `auth_kind` is `"key"`.
    pub key_path: Option<String>,
    /// The private key itself, when `auth_kind` is `"key_text"`.
    ///
    /// How a key is given on a phone: there is no user-visible filesystem to point a path at and
    /// no ssh-agent to defer to, so the key body is pasted. Secret material, handled exactly like
    /// `secret` — kept in the platform keystore and passed here per connection.
    pub key_text: Option<String>,
    /// Key passphrase or account password. Held only for the life of the connection attempt.
    pub secret: Option<String>,
    /// `"strict"`, `"accept_new"` or `"accept_any"`.
    pub host_key_policy: String,
    /// Where to record trusted host keys. Empty means `~/.ssh/known_hosts`.
    ///
    /// A phone has no `~/.ssh` and no `HOME` in the app's environment, so "remember this server's
    /// identity" wrote nothing and every later connection was another first connection. Each app
    /// passes its own writable directory; the core does not guess.
    pub known_hosts_path: Option<String>,
    pub refresh_ms: u64,
}

/// Connection lifecycle, flattened for the FFI.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ConnectionState {
    Idle,
    Connecting,
    Online,
    Reconnecting { attempt: u32, retry_in_ms: u64 },
    Failed { message: String, recoverable: bool },
}

impl From<&TargetState> for ConnectionState {
    fn from(state: &TargetState) -> Self {
        match state {
            TargetState::Idle => ConnectionState::Idle,
            TargetState::Connecting => ConnectionState::Connecting,
            TargetState::Online => ConnectionState::Online,
            TargetState::Reconnecting {
                attempt,
                retry_in_ms,
            } => ConnectionState::Reconnecting {
                attempt: *attempt,
                retry_in_ms: *retry_in_ms,
            },
            TargetState::Failed {
                message,
                recoverable,
            } => ConnectionState::Failed {
                message: message.clone(),
                recoverable: *recoverable,
            },
        }
    }
}

/// One reading, ready to draw.
///
/// Named `MetricGauge` rather than `Gauge` because SwiftUI exports a `Gauge` view, and an
/// ambiguous type lookup in every UI file is a worse tax than four extra characters here.
#[derive(Clone, Debug, uniffi::Record)]
pub struct MetricGauge {
    pub series_id: String,
    pub metric: String,
    pub label: String,
    pub value: f64,
    /// Upper bound when one is known — 100 for a percentage, total bytes for a filesystem.
    /// `None` means the UI should scale against the observed window.
    pub max: Option<f64>,
    /// Display suffix: `%`, `B/s`, `°C`.
    pub unit_suffix: String,
    /// True when values scale by 1024 rather than 1000.
    pub binary_scaled: bool,
    /// Recent values, oldest first, for the sparkline.
    pub history: Vec<f64>,
    /// `ok`, `busy`, `problem`, or `none` when the reading is not a proportion of anything.
    ///
    /// Decided here rather than in each UI. It had drifted out into all four front-ends — Android
    /// coloured at 0.75/0.90 where Apple used 0.60/0.85, so the same host was amber on a phone and
    /// green on a desk — and the temperature rule was written twice, by hand, in two languages.
    /// The UIs map a level onto a colour, which is the whole of what a view layer should decide.
    pub severity: String,
}

/// A titled group of secondary metrics, shown below the headline grid.
#[derive(Clone, Debug, uniffi::Record)]
pub struct DetailGroup {
    pub title: String,
    pub gauges: Vec<MetricGauge>,
}

/// One tile of the simple view: a name a person recognises, a number, and a sentence.
///
/// Assembled here rather than in each UI so that "Storage · 4.6% · 142 GiB free of 150 GiB" reads
/// identically on every platform, and so the decision about *which* readings a non-technical
/// person should see lives in one place.
#[derive(Clone, Debug, uniffi::Record)]
pub struct SimpleTile {
    pub metric: String,
    /// "Processor", "Memory", "Storage", "Running for".
    pub name: String,
    /// The headline number, already formatted.
    pub value_text: String,
    /// A sentence: "142.3 GiB free of 150.0 GiB", "Barely working".
    pub summary: String,
    /// 0-1 for the ring, absent for things with no proportion.
    pub fraction: Option<f64>,
    /// `ok`, `busy`, `problem` — drives colour without the UI re-deriving thresholds.
    pub level: String,
    /// Recent values, oldest first.
    ///
    /// The simple view used to be the only screen without any trend, which had it backwards: a
    /// number with no history cannot answer "is this getting worse", which is the question someone
    /// glancing at a dashboard is actually asking.
    pub history: Vec<f64>,
}

/// One row of the process table.
///
/// Flattened rather than reusing [`EntityView`]: a host runs hundreds of processes, and shipping
/// each one's full gauge set and sparkline history across the FFI twice a second would dominate
/// the cost of a refresh. Only the handful worth showing crosses, already sorted.
#[derive(Clone, Debug, uniffi::Record)]
pub struct ProcessView {
    pub pid: String,
    pub command: String,
    /// Percent of one core, so a process spanning four cores reads 400 — the same convention
    /// `top` uses.
    pub cpu_percent: f64,
    pub memory_bytes: f64,
    /// `R`, `S`, `D`, `Z`, …
    pub state: String,
    /// Share of the *whole machine*, 0-1 — which is what a bar beside this row should draw.
    ///
    /// 100% of one core on a twenty-core host is 5% of the machine, and drawing it as a full bar
    /// would be alarming nonsense. Computed here because the core knows the core count and because
    /// both UIs were doing the arithmetic and the colouring themselves.
    pub machine_fraction: f64,
    /// `ok`, `busy` or `problem` for that share.
    pub severity: String,
}

/// A node in the entity tree.
#[derive(Clone, Debug, uniffi::Record)]
pub struct EntityView {
    pub id: String,
    pub kind: String,
    pub display: String,
    pub parent: Option<String>,
    pub gauges: Vec<MetricGauge>,
}

/// Everything the UI needs to render one host, as of the last completed refresh.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TargetSnapshot {
    pub target_id: String,
    pub state: ConnectionState,
    /// What the host calls itself; falls back to the configured address.
    pub display_name: String,
    pub distro: String,
    pub kernel: String,
    pub arch: String,
    pub cpu_count: u32,
    /// The headline status grid.
    pub gauges: Vec<MetricGauge>,
    /// Everything else the host reports, grouped by collector.
    pub detail_groups: Vec<DetailGroup>,
    /// Child entities — cores, interfaces, disks, filesystems. Processes are deliberately absent;
    /// see `top_processes`.
    pub entities: Vec<EntityView>,
    /// The busiest processes, already ranked. What explains a busy host.
    pub top_processes: Vec<ProcessView>,
    /// One-line plain-language assessment of the host.
    pub health: crate::plain::HostHealth,
    /// The readings a non-technical person should see, in the order they should see them.
    pub simple_tiles: Vec<SimpleTile>,
    /// Collectors that failed to parse this tick. Non-fatal.
    pub source_errors: Vec<String>,
    pub last_update_ms: i64,
    /// Round trips spent since connecting, surfaced so the batching guarantee is observable in
    /// the running app rather than only in tests.
    pub round_trips: u64,
}

impl TargetSnapshot {
    /// A snapshot for a target that has produced nothing yet.
    pub fn placeholder(target_id: &str, host: &str, state: ConnectionState) -> Self {
        let state_for_placeholder = state.clone();
        TargetSnapshot {
            target_id: target_id.to_string(),
            state,
            display_name: host.to_string(),
            distro: String::new(),
            kernel: String::new(),
            arch: String::new(),
            cpu_count: 0,
            gauges: Vec::new(),
            detail_groups: Vec::new(),
            entities: Vec::new(),
            top_processes: Vec::new(),
            health: crate::plain::assess(&state_for_placeholder, &[], false),
            simple_tiles: Vec::new(),
            source_errors: Vec::new(),
            last_update_ms: 0,
            round_trips: 0,
        }
    }
}

/// The headline tiles, in the order the status grid shows them.
///
/// This list **curates**, not merely orders. A busy host publishes on the order of forty
/// host-level series, and promoting all of them to top-level tiles gives `tcp_orphaned` the same
/// visual weight as CPU — which is not a dashboard, it is a data dump. Everything not named here
/// is still collected and still shown, grouped under [`host_details`].
///
/// A fixed order also beats sorting by name: these are the tiles people actually look at, and they
/// must not move when a host gains a swap partition.
const HEADLINE: [&str; 9] = [
    "cpu_usage",
    "mem_usage",
    "disk_usage",
    "swap_usage",
    "cpu_temp",
    "load1",
    "net_rx",
    "net_tx",
    "uptime",
];

/// Display order for everything that is not a headline tile.
///
/// Explicit because sorting by name is actively wrong for the reader: it renders load averages as
/// `load1, load15, load5`, and scatters the memory breakdown alphabetically instead of running
/// total → used → available. Anything unlisted sorts after these, by name.
const DETAIL_ORDER: &[&str] = &[
    // CPU breakdown, in the order the parts add up.
    "cpu_user",
    "cpu_system",
    "cpu_iowait",
    "cpu_steal",
    "ctx_switches",
    "procs_running",
    "procs_blocked",
    // Memory, largest envelope first.
    "mem_total",
    "mem_used",
    "mem_available",
    "mem_free",
    "mem_cached",
    "mem_buffers",
    "swap_total",
    "swap_used",
    // Load averages in time order, not alphabetical order.
    "load1",
    "load5",
    "load15",
    "procs_total",
    "uptime",
    // Heat sits with the CPU it belongs to rather than at the end with the rates.
    "cpu_temp",
    // Throughput.
    "disk_read",
    "disk_write",
    "net_rx",
    "net_tx",
    // Sockets, current state before cumulative rates.
    "sockets",
    "tcp_established",
    "tcp_inuse",
    "tcp_timewait",
    "tcp_orphan",
    "udp_inuse",
    "tcp_in_segs",
    "tcp_out_segs",
    "tcp_retrans",
    "tcp_active_opens",
    "tcp_passive_opens",
];

/// Section headings for the detail groups, keyed by the collector that produced them.
fn group_title(source: &str) -> Option<&'static str> {
    match source {
        "proc.cpu" => Some("CPU"),
        "proc.memory" => Some("Memory"),
        "proc.load" => Some("Load & processes"),
        "proc.diskio" => Some("Disk I/O"),
        "proc.network" => Some("Network totals"),
        "proc.tcp" => Some("Sockets & TCP"),
        // Individual readings get their own per-sensor cards; the host-level temperature is
        // already a headline, so a flat duplicate list would say everything twice.
        "sys.sensors" => None,
        // Filesystems get their own per-mount cards; a duplicate flat list would be noise.
        "proc.filesystem" => None,
        _ => Some("Other"),
    }
}

/// Rank within [`DETAIL_ORDER`], with unlisted metrics after the listed ones.
fn detail_rank(metric: &str) -> usize {
    DETAIL_ORDER
        .iter()
        .position(|m| *m == metric)
        .unwrap_or(DETAIL_ORDER.len())
}

fn gauge_from(descriptor: &SeriesDescriptor, store: &LiveStore) -> Option<MetricGauge> {
    // Info series carry text and have no numeric history to draw.
    if descriptor.kind == SeriesKind::Info {
        return None;
    }
    let latest = store.latest(&descriptor.id)?;
    let unit = descriptor.effective_unit();

    Some(MetricGauge {
        series_id: descriptor.id.to_string(),
        metric: descriptor.metric.clone(),
        label: descriptor.display.clone(),
        value: latest.value,
        max: descriptor.display_max(),
        unit_suffix: unit.suffix().to_string(),
        binary_scaled: unit.is_binary_scaled(),
        history: store
            .history_vec(&descriptor.id)
            .into_iter()
            .map(|p| p.value)
            .collect(),
        severity: severity_of(unit, latest.value, descriptor.display_max()),
    })
}

/// How worrying a reading is.
///
/// Temperature is judged differently from everything else, and has to be: an NVMe drive is
/// specified to 70°C and a CPU package to 100, so a fixed number is wrong in both directions.
/// Where the chip publishes its own critical point the reading is judged against that; where it
/// does not, the fallback is the same pair of numbers the plain-language verdict uses.
///
/// Anything without a maximum is not a proportion of anything, so it gets no level at all rather
/// than a green that would imply "plenty left".
fn severity_of(unit: Unit, value: f64, max: Option<f64>) -> String {
    let level = |fraction: f64, warn: f64, bad: f64| {
        if fraction >= bad {
            "problem"
        } else if fraction >= warn {
            "busy"
        } else {
            "ok"
        }
    };

    if unit == Unit::Celsius {
        return match max.filter(|m| *m > 0.0) {
            Some(critical) => level(value / critical, 0.85, 0.95),
            None => level(value, 80.0, 90.0),
        }
        .to_string();
    }

    match max.filter(|m| *m > 0.0) {
        Some(max) => level((value / max).clamp(0.0, 1.0), 0.60, 0.85).to_string(),
        None => "none".to_string(),
    }
}

/// The headline status grid: only the metrics in [`HEADLINE`], in that order.
pub fn host_gauges(store: &LiveStore, host: &EntityId) -> Vec<MetricGauge> {
    let mut gauges: Vec<MetricGauge> = store
        .series_for(host)
        .into_iter()
        .filter(|d| HEADLINE.contains(&d.metric.as_str()))
        .filter_map(|d| gauge_from(d, store))
        .collect();

    gauges.sort_by_key(|g| {
        HEADLINE
            .iter()
            .position(|m| *m == g.metric)
            .unwrap_or(usize::MAX)
    });
    gauges
}

/// Everything else the host reports, grouped by collector.
///
/// Nothing is discarded — the headline grid is a summary, and this is where the rest lives. Groups
/// come back in collector order with a stable heading, so the section a reader learned to look in
/// stays where they left it.
pub fn host_details(store: &LiveStore, host: &EntityId) -> Vec<DetailGroup> {
    let mut groups: Vec<DetailGroup> = Vec::new();

    for descriptor in store.series_for(host) {
        if HEADLINE.contains(&descriptor.metric.as_str()) {
            continue;
        }
        let Some(title) = group_title(descriptor.source.as_str()) else {
            continue;
        };
        let Some(gauge) = gauge_from(descriptor, store) else {
            continue;
        };

        match groups.iter_mut().find(|g| g.title == title) {
            Some(group) => group.gauges.push(gauge),
            None => groups.push(DetailGroup {
                title: title.to_string(),
                gauges: vec![gauge],
            }),
        }
    }

    for group in &mut groups {
        group.gauges.sort_by(|a, b| {
            detail_rank(&a.metric)
                .cmp(&detail_rank(&b.metric))
                .then(a.metric.cmp(&b.metric))
        });
    }
    // Same ordering rule for the sections themselves: by their first metric's rank.
    groups.sort_by_key(|g| {
        g.gauges
            .first()
            .map(|x| detail_rank(&x.metric))
            .unwrap_or(usize::MAX)
    });
    groups
}

/// Entity kind slug for a process, used to keep the table out of the general entity list.
pub const PROCESS_KIND: &str = "proc";

/// The busiest processes, ranked by CPU and then by memory.
///
/// CPU is a derived rate, so it is absent on the first tick after connecting and every process
/// ties at zero; falling back to resident memory means the panel shows something plausible
/// immediately rather than an arbitrary ordering that then reshuffles.
pub fn top_processes(
    store: &LiveStore,
    host: &EntityId,
    limit: usize,
    cpu_count: u32,
) -> Vec<ProcessView> {
    let machine = f64::from(cpu_count.max(1)) * 100.0;
    let mut rows: Vec<ProcessView> = store
        .children_of(host)
        .into_iter()
        .filter(|e| e.kind == EntityKind::Process)
        .map(|entity| {
            let value = |metric: &str| {
                store
                    .series_for(&entity.id)
                    .into_iter()
                    .find(|d| d.metric == metric)
                    .and_then(|d| store.latest(&d.id))
                    .map(|p| p.value)
                    .unwrap_or(0.0)
            };
            ProcessView {
                pid: entity.display.clone(),
                command: entity
                    .labels
                    .get("command")
                    .cloned()
                    .unwrap_or_else(|| entity.display.clone()),
                cpu_percent: value("cpu"),
                memory_bytes: value("rss"),
                state: entity.labels.get("state").cloned().unwrap_or_default(),
                machine_fraction: (value("cpu") / machine).clamp(0.0, 1.0),
                severity: severity_of(Unit::Percent, value("cpu") / machine * 100.0, Some(100.0)),
            }
        })
        .collect();

    rows.sort_by(|a, b| {
        b.cpu_percent
            .partial_cmp(&a.cpu_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                b.memory_bytes
                    .partial_cmp(&a.memory_bytes)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    rows.truncate(limit);
    rows
}

pub fn entity_view(entity: &Entity, store: &LiveStore) -> EntityView {
    EntityView {
        id: entity.id.to_string(),
        kind: entity.kind.slug().to_string(),
        display: entity.display.clone(),
        parent: entity.parent.as_ref().map(ToString::to_string),
        gauges: store
            .series_for(&entity.id)
            .into_iter()
            .filter_map(|d| gauge_from(d, store))
            .collect(),
    }
}

/// Format a value for display. Lives here so all four UIs agree byte for byte.
pub fn format_value(value: f64, unit_suffix: &str, binary_scaled: bool) -> String {
    if binary_scaled {
        let base = 1024.0;
        const PREFIXES: [&str; 6] = ["", "Ki", "Mi", "Gi", "Ti", "Pi"];
        let mut scaled = value.abs();
        let mut index = 0;
        while scaled >= base && index < PREFIXES.len() - 1 {
            scaled /= base;
            index += 1;
        }
        let signed = if value < 0.0 { -scaled } else { scaled };
        let digits = if index == 0 { 0 } else { 1 };
        // `B` and `B/s` both scale, but only the leading `B` takes the prefix.
        let tail = unit_suffix.strip_prefix('B').unwrap_or("");
        format!("{signed:.digits$} {}B{tail}", PREFIXES[index])
    } else if unit_suffix == "%" {
        format!("{value:.1}%")
    } else if value.abs() >= 1000.0 || value.fract() == 0.0 {
        // Large values and whole numbers both read better without a decimal tail: "12345 " not
        // "12345.00 ", and "7" not "7.00".
        format!("{value:.0} {unit_suffix}").trim_end().to_string()
    } else {
        format!("{value:.2} {unit_suffix}").trim_end().to_string()
    }
}

/// Minimum sparkline span, as a fraction of the largest value in the window.
///
/// Range-scaling a chart lies in one direction and a zero baseline lies in the other. Storage
/// sitting at 5.19% and ticking to 5.20% has a span of 0.01; stretched to full height it draws a
/// cliff, and the chart screams that the disk just filled up. Flooring the span keeps genuinely
/// flat series flat and amplifies only real movement. See DESIGN.md, "Charts must not lie in
/// either direction".
const NOISE_FLOOR: f64 = 0.05;

/// Normalise a series to 0-1 for a sparkline, oldest first.
///
/// Here rather than in the front-ends because it is a rule about *what the chart claims*, not
/// about how to draw a line — and because it was already written by hand in Swift and again in
/// Kotlin, which is the shape invariant 3 exists to stop. The Linux app calls this; the Apple and
/// Android layers still carry their own copies and should be moved onto it by someone who can
/// build and run them.
///
/// A series that has not moved draws along the bottom of the box, which is what the Swift and
/// Kotlin implementations already do — the point of the floor is that it draws *level*, not where
/// that level sits. The midline is reserved for the one case with no range to scale against at
/// all: every reading exactly zero.
pub fn sparkline_points(history: &[f64]) -> Vec<f64> {
    if history.is_empty() {
        return Vec::new();
    }
    let highest = history.iter().copied().fold(f64::MIN, f64::max);
    let lowest = history.iter().copied().fold(f64::MAX, f64::min);
    let magnitude = highest.abs().max(lowest.abs());
    let span = (highest - lowest).max(magnitude * NOISE_FLOOR);

    history
        .iter()
        .map(|value| {
            if span > 0.0 {
                ((value - lowest) / span).clamp(0.0, 1.0)
            } else {
                // Every point identical and zero: there is no range to scale against at all.
                0.5
            }
        })
        .collect()
}

/// Format a duration in seconds as `12d 4h`, for uptime.
pub fn format_uptime(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let (days, hours, minutes) = (
        total / 86_400,
        (total % 86_400) / 3_600,
        (total % 3_600) / 60,
    );
    match (days, hours) {
        (0, 0) => format!("{minutes}m"),
        (0, _) => format!("{hours}h {minutes}m"),
        _ => format!("{days}d {hours}h"),
    }
}

/// Parse the string forms used across the FFI boundary into transport types.
pub fn connection_spec(config: &TargetConfig) -> sg_transport::ConnectionSpec {
    use sg_transport::{Auth, HostKeyPolicy};

    let auth = match config.auth_kind.as_str() {
        "key" => Auth::KeyFile {
            path: config.key_path.clone().unwrap_or_default().into(),
            passphrase: config.secret.clone().filter(|s| !s.is_empty()),
        },
        "key_text" => Auth::KeyText {
            key: config.key_text.clone().unwrap_or_default(),
            passphrase: config.secret.clone().filter(|s| !s.is_empty()),
        },
        "password" => Auth::Password(config.secret.clone().unwrap_or_default()),
        // Agent is the safe default: the app never holds key material at all.
        _ => Auth::Agent,
    };

    let policy = match config.host_key_policy.as_str() {
        "accept_new" => HostKeyPolicy::AcceptNew,
        "accept_any" => HostKeyPolicy::AcceptAny,
        _ => HostKeyPolicy::Strict,
    };

    let mut spec = sg_transport::ConnectionSpec::new(&config.host, &config.user)
        .port(if config.port == 0 { 22 } else { config.port })
        .auth(auth)
        .host_key_policy(policy);
    if let Some(path) = config.known_hosts_path.as_deref().filter(|p| !p.is_empty()) {
        spec = spec.known_hosts(path);
    }
    spec
}

/// The unit a suffix came from, for tests.
#[allow(dead_code)]
fn unit_of(suffix: &str) -> Option<Unit> {
    [
        Unit::Percent,
        Unit::Bytes,
        Unit::BytesPerSecond,
        Unit::Celsius,
        Unit::Seconds,
        Unit::Count,
    ]
    .into_iter()
    .find(|u| u.suffix() == suffix)
}

/// Build the simple view's tiles from the headline gauges.
///
/// Deliberately only four things. A non-technical person asked to watch six numbers watches none;
/// processor, memory, storage and "has it been up" are the four that mean something without
/// training, and everything else stays one tap away under the technical view.
pub fn simple_tiles(
    headline: &[MetricGauge],
    all: &[MetricGauge],
    entities: &[EntityView],
) -> Vec<SimpleTile> {
    // Three, not four. Uptime was the fourth, but the health card's own sentence already reads
    // "Running for 12h 36m" — the tile repeated it, carried no ring because it has no proportion,
    // and unbalanced the grid. Saying a thing once and saying it well beats saying it twice.
    const ORDER: [&str; 3] = ["cpu_usage", "mem_usage", "disk_usage"];

    let find = |metric: &str| headline.iter().find(|g| g.metric == metric);
    // Sizes live in the detail groups, not the headline set, so the lookup has to span both. A
    // tile that says "6% used" instead of "7.4 GiB free of 7.8 GiB" is the exact failure this
    // whole layer exists to prevent.
    let anywhere = |metric: &str| all.iter().find(|g| g.metric == metric).map(|g| g.value);

    // Storage sizes belong to the root filesystem entity rather than to any host-level series.
    let root = entities
        .iter()
        .find(|e| e.kind == "fs" && e.display == "/")
        .map(|fs| {
            let of = |metric: &str| {
                fs.gauges
                    .iter()
                    .find(|g| g.metric == metric)
                    .map(|g| g.value)
            };
            (of("used"), of("total"))
        })
        .unwrap_or((None, None));

    ORDER
        .iter()
        .filter_map(|metric| {
            let gauge = find(metric)?;
            let name = crate::plain::plain_name(metric)?;

            let (used, total) = match *metric {
                "mem_usage" => (anywhere("mem_used"), anywhere("mem_total")),
                "disk_usage" => root,
                _ => (None, None),
            };

            let value_text = format_value(gauge.value, &gauge.unit_suffix, gauge.binary_scaled);

            let level = match gauge.fraction_percent() {
                Some(p) if p >= 90.0 => "problem",
                Some(p) if p >= 80.0 => "busy",
                _ => "ok",
            };

            Some(SimpleTile {
                metric: (*metric).to_string(),
                name: name.to_string(),
                value_text,
                summary: crate::plain::plain_summary(gauge, used, total),
                fraction: gauge.display_fraction(),
                level: level.to_string(),
                history: gauge.history.clone(),
            })
        })
        .collect()
}

impl MetricGauge {
    /// Percentage position within the metric's range, when it has one.
    fn fraction_percent(&self) -> Option<f64> {
        self.max
            .filter(|m| *m > 0.0)
            .map(|m| self.value / m * 100.0)
    }

    /// 0-1 position, for a ring.
    fn display_fraction(&self) -> Option<f64> {
        self.fraction_percent().map(|p| (p / 100.0).clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sg_model::{Sample, SourceId, Value};

    #[test]
    fn a_sparkline_spans_the_box_when_the_reading_really_moved() {
        let points = sparkline_points(&[0.0, 50.0, 100.0]);
        assert_eq!(points, vec![0.0, 0.5, 1.0]);
    }

    #[test]
    fn a_nearly_flat_series_draws_nearly_flat_rather_than_as_a_cliff() {
        // Storage at 5.19% ticking to 5.20%. Scaled to the observed range alone this draws a
        // full-height climb and reads as a disk about to fill up.
        let points = sparkline_points(&[5.19, 5.19, 5.20]);
        let highest = points.iter().copied().fold(f64::MIN, f64::max);
        let lowest = points.iter().copied().fold(f64::MAX, f64::min);
        assert!(
            highest - lowest < 0.05,
            "0.01 of movement on a 5.2 magnitude drew a span of {}",
            highest - lowest
        );
    }

    #[test]
    fn a_series_that_has_not_moved_draws_level() {
        // Level, not centred — matching the Swift and Kotlin implementations exactly. What matters
        // is that there is no vertical variation to misread as movement.
        assert_eq!(sparkline_points(&[4.0, 4.0, 4.0]), vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn an_all_zero_window_has_no_range_to_scale_against_and_uses_the_midline() {
        assert_eq!(sparkline_points(&[0.0, 0.0]), vec![0.5, 0.5]);
    }

    #[test]
    fn an_empty_window_produces_no_points_rather_than_a_panic() {
        assert!(sparkline_points(&[]).is_empty());
        assert_eq!(sparkline_points(&[7.0]), vec![0.0]);
    }

    #[test]
    fn formats_bytes_with_binary_prefixes() {
        assert_eq!(format_value(512.0, "B", true), "512 B");
        assert_eq!(format_value(1024.0, "B", true), "1.0 KiB");
        assert_eq!(format_value(1_536.0, "B", true), "1.5 KiB");
        assert_eq!(format_value(1_073_741_824.0, "B", true), "1.0 GiB");
    }

    /// Only the leading `B` takes the prefix: a rate is `MiB/s`, not `MiB/sMi`.
    #[test]
    fn formats_byte_rates_keeping_the_per_second_tail() {
        assert_eq!(format_value(2_097_152.0, "B/s", true), "2.0 MiB/s");
        assert_eq!(format_value(100.0, "B/s", true), "100 B/s");
    }

    #[test]
    fn formats_percentages_to_one_decimal() {
        assert_eq!(format_value(42.0, "%", false), "42.0%");
        assert_eq!(format_value(3.16159, "%", false), "3.2%");
        assert_eq!(format_value(100.0, "%", false), "100.0%");
    }

    #[test]
    fn formats_plain_counts_without_a_trailing_space() {
        assert_eq!(format_value(7.0, "", false), "7");
        assert_eq!(format_value(1.5, "", false), "1.50");
        assert_eq!(format_value(12_345.0, "", false), "12345");
    }

    #[test]
    fn formats_uptime_at_a_sensible_granularity() {
        assert_eq!(format_uptime(45.0), "0m");
        assert_eq!(format_uptime(3_600.0), "1h 0m");
        assert_eq!(format_uptime(3_660.0), "1h 1m");
        assert_eq!(format_uptime(90_000.0), "1d 1h");
        assert_eq!(
            format_uptime(-5.0),
            "0m",
            "a negative uptime must not underflow"
        );
    }

    /// Build a store holding the host-level series a real Proxmox host produced.
    fn proxmox_like_store() -> (LiveStore, Entity) {
        let mut store = LiveStore::default();
        let host = Entity::host("polly");

        // (source, metric) pairs taken from an actual 20-core Proxmox run.
        let series = [
            ("proc.cpu", "cpu_usage"),
            ("proc.cpu", "cpu_user"),
            ("proc.cpu", "cpu_system"),
            ("proc.cpu", "cpu_iowait"),
            ("proc.cpu", "cpu_steal"),
            ("proc.cpu", "ctx_switches"),
            ("proc.cpu", "procs_running"),
            ("proc.cpu", "procs_blocked"),
            ("proc.memory", "mem_usage"),
            ("proc.memory", "mem_total"),
            ("proc.memory", "mem_used"),
            ("proc.memory", "mem_available"),
            ("proc.memory", "mem_free"),
            ("proc.memory", "mem_cached"),
            ("proc.memory", "mem_buffers"),
            ("proc.memory", "swap_usage"),
            ("proc.memory", "swap_total"),
            ("proc.memory", "swap_used"),
            ("proc.load", "load1"),
            ("proc.load", "load5"),
            ("proc.load", "load15"),
            ("proc.load", "procs_total"),
            ("proc.load", "uptime"),
            ("proc.diskio", "disk_read"),
            ("proc.diskio", "disk_write"),
            ("proc.network", "net_rx"),
            ("proc.network", "net_tx"),
            ("proc.filesystem", "disk_usage"),
            ("proc.tcp", "sockets"),
            ("proc.tcp", "tcp_established"),
            ("proc.tcp", "tcp_inuse"),
            ("proc.tcp", "tcp_timewait"),
            ("proc.tcp", "tcp_orphan"),
            ("proc.tcp", "udp_inuse"),
            ("proc.tcp", "tcp_in_segs"),
            ("proc.tcp", "tcp_out_segs"),
            ("proc.tcp", "tcp_retrans"),
            ("proc.tcp", "tcp_active_opens"),
            ("proc.tcp", "tcp_passive_opens"),
        ];

        for (source, metric) in series {
            let d = SeriesDescriptor::gauge(
                &SourceId::new(source),
                &host.id,
                metric,
                metric,
                Unit::Count,
            );
            store.ingest(
                vec![host.clone()],
                vec![d.clone()],
                &[Sample::new(d.id.clone(), 1, Value::Float(1.0))],
            );
        }
        (store, host)
    }

    /// The defect a first run against a real Proxmox host exposed: every host-level series became
    /// a top-level tile, so a 20-core box rendered forty of them and `tcp_orphaned` carried the
    /// same visual weight as CPU.
    #[test]
    fn the_headline_grid_is_curated_not_merely_ordered() {
        let (store, host) = proxmox_like_store();
        let headline: Vec<_> = host_gauges(&store, &host.id)
            .into_iter()
            .map(|g| g.metric)
            .collect();

        assert_eq!(
            headline,
            vec![
                "cpu_usage",
                "mem_usage",
                "disk_usage",
                "swap_usage",
                "load1",
                "net_rx",
                "net_tx",
                "uptime"
            ],
            "the status grid must be a summary, not every series the host publishes"
        );
    }

    /// Curating must not lose anything — the rest moves into groups, it does not disappear.
    #[test]
    fn every_host_series_is_either_headline_or_grouped() {
        let (store, host) = proxmox_like_store();

        let mut shown: Vec<String> = host_gauges(&store, &host.id)
            .into_iter()
            .map(|g| g.metric)
            .collect();
        for group in host_details(&store, &host.id) {
            shown.extend(group.gauges.into_iter().map(|g| g.metric));
        }
        shown.sort();

        let mut published: Vec<String> = store
            .series_for(&host.id)
            .into_iter()
            .map(|d| d.metric.clone())
            .collect();
        published.sort();

        assert_eq!(
            shown, published,
            "curation dropped metrics instead of relocating them"
        );
    }

    /// Sorting detail metrics by name renders load averages as 1, 15, 5 — which is the order the
    /// first real screenshot showed, and is simply wrong to read.
    #[test]
    fn load_averages_are_ordered_by_time_not_by_name() {
        let (store, host) = proxmox_like_store();
        let groups = host_details(&store, &host.id);

        let load = groups
            .iter()
            .find(|g| g.title == "Load & processes")
            .expect("load group");
        let metrics: Vec<_> = load.gauges.iter().map(|g| g.metric.as_str()).collect();
        let position = |m: &str| metrics.iter().position(|x| *x == m).unwrap();

        assert!(position("load5") < position("load15"), "got {metrics:?}");
        // `load1` is a headline tile, so only the other two appear here.
        assert!(!metrics.contains(&"load1"));
    }

    #[test]
    fn detail_groups_are_titled_by_collector_and_memory_reads_top_down() {
        let (store, host) = proxmox_like_store();
        let groups = host_details(&store, &host.id);

        let titles: Vec<_> = groups.iter().map(|g| g.title.as_str()).collect();
        for expected in ["CPU", "Memory", "Load & processes", "Sockets & TCP"] {
            assert!(
                titles.contains(&expected),
                "missing group {expected}: {titles:?}"
            );
        }
        // Filesystems have their own per-mount cards; a flat duplicate list would be noise.
        assert!(
            !titles.contains(&"Other"),
            "an ungrouped collector leaked into the UI"
        );

        let memory = groups.iter().find(|g| g.title == "Memory").unwrap();
        let metrics: Vec<_> = memory.gauges.iter().map(|g| g.metric.as_str()).collect();
        assert_eq!(
            metrics.first(),
            Some(&"mem_total"),
            "memory should read total first: {metrics:?}"
        );
        assert!(
            metrics.iter().position(|m| *m == "mem_used").unwrap()
                < metrics.iter().position(|m| *m == "mem_free").unwrap()
        );
    }

    #[test]
    fn status_gauges_keep_a_fixed_order_regardless_of_arrival() {
        let mut store = LiveStore::default();
        let host = Entity::host("web-01");
        let source = SourceId::new("test");

        // Declared in an order deliberately unlike the display order.
        for metric in ["uptime", "net_rx", "cpu_usage", "zz_custom", "mem_usage"] {
            let d = SeriesDescriptor::gauge(&source, &host.id, metric, metric, Unit::Percent);
            store.ingest(
                vec![host.clone()],
                vec![d.clone()],
                &[Sample::new(d.id.clone(), 1, Value::Float(1.0))],
            );
        }

        let order: Vec<_> = host_gauges(&store, &host.id)
            .into_iter()
            .map(|g| g.metric)
            .collect();
        assert_eq!(order, vec!["cpu_usage", "mem_usage", "net_rx", "uptime"]);

        // A metric outside the headline set is relocated, not discarded.
        let grouped: Vec<_> = host_details(&store, &host.id)
            .into_iter()
            .flat_map(|g| g.gauges)
            .map(|g| g.metric)
            .collect();
        assert!(grouped.contains(&"zz_custom".to_string()));
    }

    #[test]
    fn a_gauge_carries_its_sparkline() {
        let mut store = LiveStore::default();
        let host = Entity::host("web-01");
        let d = SeriesDescriptor::gauge(
            &SourceId::new("test"),
            &host.id,
            "cpu_usage",
            "CPU",
            Unit::Percent,
        );

        for (at, value) in [(1, 10.0), (2, 20.0), (3, 30.0)] {
            store.ingest(
                vec![host.clone()],
                vec![d.clone()],
                &[Sample::new(d.id.clone(), at, Value::Float(value))],
            );
        }

        let gauge = host_gauges(&store, &host.id).remove(0);
        assert_eq!(gauge.value, 30.0, "the gauge shows the latest value");
        assert_eq!(
            gauge.history,
            vec![10.0, 20.0, 30.0],
            "oldest first, for the sparkline"
        );
        assert_eq!(
            gauge.max,
            Some(100.0),
            "a percentage has an inherent maximum"
        );
        assert_eq!(gauge.unit_suffix, "%");
    }

    #[test]
    fn agent_is_the_default_for_unrecognised_auth_kinds() {
        let config = TargetConfig {
            host: "h".into(),
            port: 0,
            user: "u".into(),
            auth_kind: "nonsense".into(),
            key_path: None,
            key_text: None,
            secret: None,
            host_key_policy: "nonsense".into(),
            known_hosts_path: None,
            refresh_ms: 1000,
        };
        let spec = connection_spec(&config);

        assert_eq!(spec.port, 22, "port 0 should fall back to the SSH default");
        assert_eq!(
            spec.auth.describe(),
            "ssh-agent",
            "unknown auth must not silently weaken"
        );
        assert_eq!(
            spec.host_key_policy,
            sg_transport::HostKeyPolicy::Strict,
            "an unrecognised policy must be the strict one, never the permissive one"
        );
    }

    #[test]
    fn maps_the_explicit_auth_and_policy_forms() {
        let config = TargetConfig {
            host: "h".into(),
            port: 2222,
            user: "root".into(),
            auth_kind: "key".into(),
            key_path: Some("/tmp/id".into()),
            key_text: None,
            secret: Some(String::new()),
            host_key_policy: "accept_new".into(),
            known_hosts_path: None,
            refresh_ms: 1000,
        };
        let spec = connection_spec(&config);

        assert_eq!(spec.port, 2222);
        assert_eq!(spec.auth.describe(), "key /tmp/id");
        assert_eq!(spec.host_key_policy, sg_transport::HostKeyPolicy::AcceptNew);
        // An empty passphrase must become None, not Some(""), or key loading fails.
        assert!(matches!(
            spec.auth,
            sg_transport::Auth::KeyFile {
                passphrase: None,
                ..
            }
        ));
    }
}

#[cfg(test)]
mod severity_tests {
    use super::*;

    /// The rule the UIs used to each implement for themselves.
    #[test]
    fn utilisation_is_judged_against_its_own_maximum() {
        assert_eq!(severity_of(Unit::Percent, 42.0, Some(100.0)), "ok");
        assert_eq!(severity_of(Unit::Percent, 71.0, Some(100.0)), "busy");
        assert_eq!(severity_of(Unit::Percent, 91.0, Some(100.0)), "problem");
    }

    /// A rate has no maximum, so it is not a proportion of anything — and colouring it green
    /// would say "plenty of room left" about a number with no ceiling.
    #[test]
    fn a_reading_without_a_maximum_has_no_level() {
        assert_eq!(severity_of(Unit::BytesPerSecond, 9_000_000.0, None), "none");
    }

    /// An NVMe drive is specified to 70°C and a CPU package to 100. Judging both by one number is
    /// wrong in both directions, so the chip's own critical point wins where it publishes one.
    #[test]
    fn temperature_is_judged_against_the_chips_own_limit() {
        // 68 °C on a drive whose limit is 70 is nearly at it.
        assert_eq!(severity_of(Unit::Celsius, 68.0, Some(70.0)), "problem");
        // The same 68 °C on a package specified to 100 is unremarkable.
        assert_eq!(severity_of(Unit::Celsius, 68.0, Some(100.0)), "ok");
    }

    /// Most thermal zones publish no critical point, so there has to be a fallback — and it has to
    /// be the same pair of numbers the spoken verdict uses, or the colour and the sentence
    /// disagree on the same screen.
    #[test]
    fn temperature_without_a_limit_falls_back_to_the_spoken_thresholds() {
        assert_eq!(severity_of(Unit::Celsius, 72.0, None), "ok");
        assert_eq!(severity_of(Unit::Celsius, 84.0, None), "busy");
        assert_eq!(severity_of(Unit::Celsius, 95.0, None), "problem");
    }
}

#[cfg(test)]
mod process_share_tests {
    use super::*;

    /// A process pinning one core is busy on a laptop and barely noticeable on a Proxmox host.
    /// Both UIs were doing this arithmetic themselves, which is how the two came to disagree.
    #[test]
    fn a_processes_share_is_of_the_machine_not_of_one_core() {
        let one_core_fully_busy = 100.0;

        let on_a_single_core_box = (one_core_fully_busy / (1.0 * 100.0_f64)).clamp(0.0, 1.0);
        assert_eq!(on_a_single_core_box, 1.0);
        assert_eq!(
            severity_of(Unit::Percent, on_a_single_core_box * 100.0, Some(100.0)),
            "problem"
        );

        let on_a_twenty_core_box = (one_core_fully_busy / (20.0 * 100.0_f64)).clamp(0.0, 1.0);
        assert_eq!(on_a_twenty_core_box, 0.05);
        assert_eq!(
            severity_of(Unit::Percent, on_a_twenty_core_box * 100.0, Some(100.0)),
            "ok"
        );
    }
}
