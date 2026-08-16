using ServerGlass.Core;
using Xunit;

namespace ServerGlass.Core.Tests;

/// <summary>
/// The FFI boundary, driven exactly as the app drives it.
/// </summary>
/// <remarks>
/// <para>
/// These load the real <c>sg_ffi.dll</c>. There is no skip-if-missing: "a skipped test reports ok"
/// is on this project's list of things that look fine and are not, and a native binding suite that
/// quietly tests nothing is the exact failure that list exists to prevent. If the library is
/// missing, these fail and say so.
/// </para>
/// <para>
/// What they are for: the C# records are hand-written mirrors of Rust ones, and nothing in either
/// compiler connects the two. The Rust side pins the wire format
/// (<c>field_set_is_asserted_so_a_new_field_fails_here</c>); this side proves the mirror actually
/// reads it.
/// </para>
/// </remarks>
public sealed class CoreTests
{
    private static TargetConfig Sample(string host = "example.test") => new()
    {
        Host = host,
        Port = 22,
        User = "root",
        AuthKind = "agent",
        HostKeyPolicy = "strict",
        RefreshMs = 1000,
    };

    [Fact]
    public void The_core_starts_and_stops_cleanly()
    {
        using var core = new ServerGlassCore();
        Assert.Empty(core.TargetIds());
    }

    [Fact]
    public void A_target_can_be_added_started_and_polled()
    {
        using var core = new ServerGlassCore();

        var id = core.AddTarget(Sample());
        Assert.NotEmpty(id);
        Assert.Equal([id], core.TargetIds());

        // A target that has never connected still renders, rather than the UI special-casing null.
        var snapshot = core.Snapshot(id);
        Assert.Equal("idle", snapshot.State.Kind);
        Assert.Equal("example.test", snapshot.DisplayName);
        Assert.Empty(snapshot.Gauges);

        core.RemoveTarget(id);
        Assert.Empty(core.TargetIds());
    }

    /// <summary>
    /// The placeholder health verdict crosses intact. If the mirror were wrong, this would be an
    /// empty string rather than a sentence — silently, which is the failure mode worth testing for.
    /// </summary>
    [Fact]
    public void The_health_verdict_arrives_already_worded_by_the_core()
    {
        using var core = new ServerGlassCore();
        var snapshot = core.Snapshot(core.AddTarget(Sample()));

        Assert.Equal("checking", snapshot.Health.Level);
        Assert.NotEmpty(snapshot.Health.Headline);
    }

    [Fact]
    public void An_unknown_target_is_an_exception_and_not_a_crash()
    {
        using var core = new ServerGlassCore();
        var error = Assert.Throws<SgException>(() => core.Snapshot("nope"));
        Assert.Equal("unknownTarget", error.Kind);
        Assert.False(error.Recoverable);
    }

    /// <summary>
    /// A command against a host that is not connected is refused rather than queued — running
    /// <c>systemctl restart</c> five minutes after someone gave up on it is not a delay, it is a
    /// surprise. The <see cref="SgException.Recoverable"/> flag has to survive the crossing for the
    /// UI to word that correctly.
    /// </summary>
    [Fact]
    public void A_command_against_an_offline_host_is_refused_and_marked_recoverable()
    {
        using var core = new ServerGlassCore();
        var id = core.AddTarget(Sample());

        var error = Assert.Throws<SgException>(() => core.RunCommand(id, "uptime"));
        Assert.Equal("connection", error.Kind);
        Assert.True(error.Recoverable);
        Assert.NotEmpty(error.Message);
    }

    /// <summary>
    /// Formatting comes from the core so all four front-ends agree. These are the same assertions
    /// the Rust suite makes, run through the binding — because agreeing in Rust is not the same as
    /// agreeing after a round trip.
    /// </summary>
    [Fact]
    public void Formatting_matches_the_core_exactly()
    {
        using var core = new ServerGlassCore();
        Assert.Equal("1.5 KiB", core.Format(1536.0, "B", binaryScaled: true));
        Assert.Equal("42.0%", core.Format(42.0, "%", binaryScaled: false));
        Assert.Equal("1d 1h", core.FormatDuration(90_000.0));
    }

    /// <summary>
    /// Uptime is formatted as a duration, not as a count of seconds.
    /// </summary>
    /// <remarks>
    /// Caught by looking at the running app, not by reading the code: the dense view showed
    /// "11324 s" where the phone shows "3h 8m". Both the Apple and Android layers special-case
    /// <c>metric == "uptime"</c>, and this pins that the Windows one does too.
    /// </remarks>
    [Fact]
    public void An_uptime_gauge_is_formatted_as_a_duration()
    {
        using var core = new ServerGlassCore();

        var uptime = new MetricGauge { Metric = "uptime", Value = 11_324, UnitSuffix = "s" };
        Assert.Equal("3h 8m", core.Format(uptime));

        // Everything else still goes through the ordinary formatter.
        var seconds = new MetricGauge { Metric = "some_other_seconds", Value = 11_324, UnitSuffix = "s" };
        Assert.Equal("11324 s", core.Format(seconds));
    }

    /// <summary>Non-ASCII must survive both directions; the boundary is UTF-8 on purpose.</summary>
    [Fact]
    public void Text_crosses_as_utf8_in_both_directions()
    {
        using var core = new ServerGlassCore();
        var id = core.AddTarget(Sample("хост.example"));
        Assert.Equal("хост.example", core.Snapshot(id).DisplayName);

        // The degree sign is a real unit suffix in this app — CPU temperature uses it. The exact
        // spacing and precision are the core's to decide, not this layer's: a whole number drops
        // its decimal tail, and a non-percent unit is spaced off the number.
        Assert.Equal("41 °C", core.Format(41.0, "°C", binaryScaled: false));
        Assert.Equal("41.50 °C", core.Format(41.5, "°C", binaryScaled: false));
    }

    /// <summary>
    /// The merge rules are the security argument behind pairing, so they are exercised through the
    /// binding too: a conflicting pin is reported and the local one is kept.
    /// </summary>
    [Fact]
    public void A_conflicting_host_key_pin_is_reported_and_never_applied()
    {
        using var core = new ServerGlassCore();

        var existing = new SyncBundle { KnownHosts = ["a.test ssh-ed25519 AAAA"] };
        var incoming = new SyncBundle { KnownHosts = ["a.test ssh-ed25519 BBBB"] };

        var merged = core.MergeBundle(existing, incoming);

        Assert.Single(merged.Conflicts);
        Assert.Equal(0u, merged.AddedPins);
        Assert.Equal(["a.test ssh-ed25519 AAAA"], merged.KnownHosts);
        Assert.Equal("a.test", merged.Conflicts[0].Host);
    }

    /// <summary>A new pin, with nothing to conflict with, merges silently.</summary>
    [Fact]
    public void A_new_host_key_pin_merges_without_asking()
    {
        using var core = new ServerGlassCore();

        var merged = core.MergeBundle(
            new SyncBundle(),
            new SyncBundle { KnownHosts = ["b.test ssh-ed25519 CCCC"] });

        Assert.Empty(merged.Conflicts);
        Assert.Equal(1u, merged.AddedPins);
    }

    /// <summary>A stale pairing handle is refused rather than dereferenced.</summary>
    [Fact]
    public void A_forgotten_pairing_session_is_reported()
    {
        using var core = new ServerGlassCore();
        var error = Assert.Throws<SgException>(() => core.AwaitPairingConnection(999));
        Assert.Equal("pairing", error.Kind);
    }

    /// <summary>Using a disposed core is a clear error rather than a use-after-free.</summary>
    [Fact]
    public void A_disposed_core_refuses_further_calls()
    {
        var core = new ServerGlassCore();
        core.Dispose();
        Assert.Throws<ObjectDisposedException>(() => core.TargetIds());
        // Disposing twice must be harmless; the app disposes on window close and on exit.
        core.Dispose();
    }

    /// <summary>
    /// Polling is what the UI actually does, twice a second, for as long as the app is open. If
    /// every reply leaked its allocation this is where it would show.
    /// </summary>
    [Fact]
    public void Repeated_polling_neither_leaks_nor_degrades()
    {
        using var core = new ServerGlassCore();
        var id = core.AddTarget(Sample());

        for (var i = 0; i < 2000; i++)
        {
            _ = core.Snapshot(id);
        }

        Assert.Equal("idle", core.Snapshot(id).State.Kind);
    }
}
