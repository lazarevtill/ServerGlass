namespace ServerGlass.Core;

/// <summary>
/// The view models, exactly as <c>crates/sg-ffi</c> serialises them.
/// </summary>
/// <remarks>
/// Every one of these is a mirror of a Rust record, and nothing in the compiler connects the two.
/// The guard is <c>field_set_is_asserted_so_a_new_field_fails_here</c> in <c>cabi.rs</c>, which
/// pins the exact key set on the wire — so a field added or renamed in Rust fails there rather
/// than arriving here as a property that silently never populates.
///
/// These types carry no behaviour beyond what is pure geometry. Thresholds, wording, units and
/// number formatting all belong to the core; see the third invariant in CLAUDE.md, which is the
/// one this project has already broken twice.
/// </remarks>
public sealed record ConnectionState
{
    /// <summary><c>idle</c>, <c>connecting</c>, <c>online</c>, <c>reconnecting</c> or <c>failed</c>.</summary>
    public string Kind { get; init; } = "idle";

    /// <summary>Present when <see cref="Kind"/> is <c>failed</c>. Already plain language.</summary>
    public string? Message { get; init; }

    /// <summary>
    /// Whether retrying could ever help. Bad credentials and a changed host key cannot, and the UI
    /// says so rather than promising a retry that will not come.
    /// </summary>
    public bool? Recoverable { get; init; }

    public uint? Attempt { get; init; }

    public ulong? RetryInMs { get; init; }

    public bool IsOnline => Kind == "online";
}

/// <summary>One reading, ready to draw.</summary>
public sealed record MetricGauge
{
    public string SeriesId { get; init; } = "";
    public string Metric { get; init; } = "";
    public string Label { get; init; } = "";
    public double Value { get; init; }

    /// <summary>
    /// Upper bound when one is known. Null means the reading has no maximum — which is precisely
    /// what decides between a ring and a number with a sparkline.
    /// </summary>
    public double? Max { get; init; }

    public string UnitSuffix { get; init; } = "";
    public bool BinaryScaled { get; init; }

    /// <summary>Recent values, oldest first.</summary>
    public IReadOnlyList<double> History { get; init; } = [];

    /// <summary>
    /// <c>ok</c>, <c>busy</c>, <c>problem</c>, or <c>none</c> where the reading is not a proportion
    /// of anything. Assigned by the core: Android once coloured at 0.75/0.90 where Apple used
    /// 0.60/0.85, so the same host read amber on a phone and green on a desk.
    /// </summary>
    public string Severity { get; init; } = "none";

    /// <summary>Position within the range, or null when there is no real maximum.</summary>
    public double? Fraction => Max is > 0 ? Math.Clamp(Value / Max.Value, 0, 1) : null;
}

/// <summary>A titled group of secondary metrics, shown below the headline grid.</summary>
public sealed record DetailGroup
{
    public string Title { get; init; } = "";
    public IReadOnlyList<MetricGauge> Gauges { get; init; } = [];
}

/// <summary>One tile of the plain view: a name a person recognises, a number, and a sentence.</summary>
public sealed record SimpleTile
{
    public string Metric { get; init; } = "";

    /// <summary>"Processor", "Memory", "Storage".</summary>
    public string Name { get; init; } = "";

    /// <summary>The headline number, already formatted by the core.</summary>
    public string ValueText { get; init; } = "";

    /// <summary>A sentence: "142.3 GiB free of 150.0 GiB", "Barely working".</summary>
    public string Summary { get; init; } = "";

    /// <summary>0-1 for the ring, null for things with no proportion.</summary>
    public double? Fraction { get; init; }

    /// <summary><c>ok</c>, <c>busy</c>, <c>problem</c>.</summary>
    public string Level { get; init; } = "ok";

    public IReadOnlyList<double> History { get; init; } = [];
}

/// <summary>One row of the process table.</summary>
public sealed record ProcessView
{
    public string Pid { get; init; } = "";
    public string Command { get; init; } = "";

    /// <summary>Percent of one core, so a process spanning four cores reads 400 — as <c>top</c> reports it.</summary>
    public double CpuPercent { get; init; }

    public double MemoryBytes { get; init; }

    /// <summary><c>R</c>, <c>S</c>, <c>D</c>, <c>Z</c>, …</summary>
    public string State { get; init; } = "";

    /// <summary>
    /// Share of the whole machine, 0-1 — which is what the bar beside the row draws. 100% of one
    /// core on a twenty-core host is 5% of the machine, and a full bar for it would be nonsense.
    /// </summary>
    public double MachineFraction { get; init; }

    public string Severity { get; init; } = "ok";
}

/// <summary>A node in the entity tree: a core, an interface, a disk, a filesystem, a sensor.</summary>
public sealed record EntityView
{
    public string Id { get; init; } = "";
    public string Kind { get; init; } = "";
    public string Display { get; init; } = "";
    public string? Parent { get; init; }
    public IReadOnlyList<MetricGauge> Gauges { get; init; } = [];

    public MetricGauge? Gauge(string metric) =>
        Gauges.FirstOrDefault(g => g.Metric == metric);

    /// <summary>Combined rate across two directions, for ranking. Both are already per-second.</summary>
    public double Throughput(string first, string second) =>
        (Gauge(first)?.Value ?? 0) + (Gauge(second)?.Value ?? 0);
}

/// <summary>How a host is doing, in one word and one sentence — both written by the core.</summary>
public sealed record HostHealth
{
    /// <summary><c>ok</c>, <c>busy</c>, <c>problem</c>, <c>offline</c>, <c>checking</c>.</summary>
    public string Level { get; init; } = "checking";

    public string Headline { get; init; } = "";

    public string Detail { get; init; } = "";
}

/// <summary>Everything needed to render one host, as of the last completed refresh.</summary>
public sealed record TargetSnapshot
{
    public string TargetId { get; init; } = "";
    public ConnectionState State { get; init; } = new();
    public string DisplayName { get; init; } = "";
    public string Distro { get; init; } = "";
    public string Kernel { get; init; } = "";
    public string Arch { get; init; } = "";
    public uint CpuCount { get; init; }
    public IReadOnlyList<MetricGauge> Gauges { get; init; } = [];
    public IReadOnlyList<DetailGroup> DetailGroups { get; init; } = [];
    public IReadOnlyList<EntityView> Entities { get; init; } = [];
    public IReadOnlyList<ProcessView> TopProcesses { get; init; } = [];
    public HostHealth Health { get; init; } = new();
    public IReadOnlyList<SimpleTile> SimpleTiles { get; init; } = [];
    public IReadOnlyList<string> SourceErrors { get; init; } = [];
    public long LastUpdateMs { get; init; }

    /// <summary>
    /// Round trips spent since connecting. Shown in the app so the batching guarantee — one round
    /// trip per refresh however many collectors are enabled — is observable rather than only
    /// asserted in a test.
    /// </summary>
    public ulong RoundTrips { get; init; }

    /// <summary>Headline and grouped gauges together, for lookup by metric name.</summary>
    public IEnumerable<MetricGauge> AllGauges =>
        Gauges.Concat(DetailGroups.SelectMany(g => g.Gauges));

    public MetricGauge? Gauge(string metric) =>
        AllGauges.FirstOrDefault(g => g.Metric == metric);

    public IEnumerable<EntityView> EntitiesOfKind(string kind) =>
        Entities.Where(e => e.Kind == kind);

    public DetailGroup? Group(string title) =>
        DetailGroups.FirstOrDefault(g => g.Title == title);
}

/// <summary>How to reach a host. The one record that travels <em>into</em> the core.</summary>
public sealed record TargetConfig
{
    public string Host { get; init; } = "";
    public ushort Port { get; init; } = 22;
    public string User { get; init; } = "";

    /// <summary><c>agent</c>, <c>key</c>, <c>key_text</c> or <c>password</c>.</summary>
    public string AuthKind { get; init; } = "agent";

    public string? KeyPath { get; init; }

    /// <summary>The private key itself. Secret material — it comes from DPAPI, never from the host record.</summary>
    public string? KeyText { get; init; }

    /// <summary>Key passphrase or account password. Held only for the life of the connection attempt.</summary>
    public string? Secret { get; init; }

    /// <summary><c>strict</c>, <c>accept_new</c> or <c>accept_any</c>.</summary>
    public string HostKeyPolicy { get; init; } = "strict";

    /// <summary>
    /// Where to record trusted host keys. The core does not guess: an app with no writable
    /// <c>~/.ssh</c> recorded nothing, and every later connection was another first connection.
    /// </summary>
    public string? KnownHostsPath { get; init; }

    public ulong RefreshMs { get; init; } = 1000;
}

/// <summary>What one command printed, and how it ended.</summary>
public sealed record CommandResult
{
    /// <summary>Everything it wrote, standard error included and in order.</summary>
    public string Output { get; init; } = "";

    /// <summary>-1 when the host did not report one.</summary>
    public int ExitCode { get; init; }

    public ulong ElapsedMs { get; init; }
}

// ---------------------------------------------------------------------------------------------
// Pairing
// ---------------------------------------------------------------------------------------------

/// <summary>A host as it crosses between devices — no credential, by construction.</summary>
public sealed record SyncHostView
{
    public string Address { get; init; } = "";
    public ushort Port { get; init; } = 22;
    public string User { get; init; } = "";
    public string AuthKind { get; init; } = "agent";
    public string? KeyPath { get; init; }
    public string HostKeyPolicy { get; init; } = "strict";
    public ulong RefreshMs { get; init; } = 1000;
}

/// <summary>A pin the two devices disagree about. Never applied; shown to the user.</summary>
public sealed record PinConflictView
{
    public string Host { get; init; } = "";
    public string Existing { get; init; } = "";
    public string Incoming { get; init; } = "";
}

/// <summary>What a device is offering to send, or has received.</summary>
public sealed record SyncBundle
{
    public IReadOnlyList<SyncHostView> Hosts { get; init; } = [];
    public IReadOnlyList<string> KnownHosts { get; init; } = [];
}

/// <summary>The result of applying a received bundle to what this device already had.</summary>
public sealed record MergeResult
{
    public IReadOnlyList<SyncHostView> Hosts { get; init; } = [];
    public IReadOnlyList<string> KnownHosts { get; init; } = [];
    public uint AddedHosts { get; init; }
    public uint KeptHosts { get; init; }
    public uint AddedPins { get; init; }

    /// <summary>Each of these needs a person. None were applied.</summary>
    public IReadOnlyList<PinConflictView> Conflicts { get; init; } = [];
}

/// <summary>A listening pairing session, and the text to render as a QR.</summary>
public sealed record ReceiverStarted
{
    public ulong Id { get; init; }
    public string PairingCode { get; init; } = "";
}

/// <summary>A connected pairing session, and the code the user compares with the other screen.</summary>
public sealed record SenderConnected
{
    public ulong Id { get; init; }
    public string VerificationCode { get; init; } = "";
}
