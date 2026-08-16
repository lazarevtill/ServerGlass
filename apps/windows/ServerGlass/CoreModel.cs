using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using Microsoft.UI.Dispatching;
using ServerGlass.Core;

namespace ServerGlass;

/// <summary>One monitored host, as the window sees it.</summary>
internal sealed class Host : INotifyPropertyChanged
{
    private TargetSnapshot _snapshot;

    public Host(SavedHost saved, string targetId)
    {
        Saved = saved;
        TargetId = targetId;
        _snapshot = new TargetSnapshot { DisplayName = saved.Address };
    }

    public SavedHost Saved { get; set; }

    public string TargetId { get; }

    public TargetSnapshot Snapshot
    {
        get => _snapshot;
        set
        {
            _snapshot = value;
            Raise();
            Raise(nameof(Title));
            Raise(nameof(Subtitle));
        }
    }

    public string Title =>
        string.IsNullOrEmpty(Snapshot.DisplayName) ? Saved.Address : Snapshot.DisplayName;

    /// <summary>What the sidebar shows under the name: the plain-language verdict, not a state enum.</summary>
    public string Subtitle => Snapshot.State.Kind switch
    {
        "online" => Snapshot.Health.Headline,
        "connecting" => "Connecting…",
        "reconnecting" => "Reconnecting…",
        "failed" => "Can't reach this server",
        _ => "Not connected",
    };

    /// <summary>The dot beside the name. Offline is a health level, so the core still decides it.</summary>
    public string StatusLevel => Snapshot.State.Kind switch
    {
        "online" => Snapshot.Health.Level,
        "failed" => "problem",
        "idle" => "checking",
        _ => "busy",
    };

    public event PropertyChangedEventHandler? PropertyChanged;

    private void Raise([CallerMemberName] string? name = null) =>
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}

/// <summary>
/// The app's state: the core, the saved hosts, and the poll loop that feeds the views.
/// </summary>
/// <remarks>
/// <para>
/// Each target runs its own tick loop inside Rust and publishes a finished snapshot; this polls
/// them on a display timer. At a one-second refresh that is indistinguishable from a push stream,
/// needs no callback interface across the FFI, and cannot deadlock the tick loop behind a slow UI
/// thread.
/// </para>
/// <para>
/// The polling happens on a background task rather than a <c>DispatcherQueueTimer</c>. A snapshot
/// for a busy host is a few hundred kilobytes of JSON, and deserialising that on the UI thread
/// twice a second is exactly the kind of jank a monitoring dashboard should not have.
/// </para>
/// </remarks>
internal sealed class CoreModel : IDisposable
{
    private readonly ServerGlassCore _core = new();
    private readonly DispatcherQueue _dispatcher;
    private readonly CancellationTokenSource _stopping = new();
    private readonly DpapiSecretStore _secrets;

    public CoreModel(DispatcherQueue dispatcher)
    {
        _dispatcher = dispatcher;

        var directory = Environment.GetEnvironmentVariable(HostStore.DirectoryVariable)
                        ?? Path.Combine(
                            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                            "ServerGlass");

        _secrets = new DpapiSecretStore(Path.Combine(directory, "secrets"));
        Store = new HostStore(_secrets, directory);

        // Storage problems are surfaced, never swallowed: "a discarded error is a silent failure"
        // is on this project's list, and it was a real bug on mobile for weeks.
        Store.LoadFailed += (_, error) => Report($"The saved server list could not be read: {error.Message}");
        _secrets.Unreadable += (_, error) => Report($"A saved sign-in could not be read: {error.Message}");

        foreach (var saved in Store.Load())
        {
            Adopt(saved);
        }

        _ = PollForever(_stopping.Token);
    }

    public HostStore Store { get; }

    public ObservableCollection<Host> Hosts { get; } = [];

    /// <summary>Raised for anything the user should be told about that is not a host's own state.</summary>
    public event EventHandler<string>? Problem;

    private void Report(string message) =>
        _dispatcher.TryEnqueue(() => Problem?.Invoke(this, message));

    /// <summary>Register a saved host with the core and start it.</summary>
    private Host Adopt(SavedHost saved)
    {
        var id = _core.AddTarget(Store.Config(saved));
        var host = new Host(saved, id);
        Hosts.Add(host);
        _core.Start(id);
        return host;
    }

    public Host Add(SavedHost saved, string? secret, string? keyText)
    {
        // Secrets first: the config handed to the core is built from them, and a storage refusal
        // must be reported as a storage refusal rather than as a sign-in failure a screen later.
        if (!string.IsNullOrEmpty(secret) && !Store.SetSecret(saved.Id, secret))
        {
            Report("The password could not be saved, so this server was not added.");
            throw new InvalidOperationException("the secret could not be stored");
        }

        if (!string.IsNullOrEmpty(keyText)
            && !Store.SetSecret(saved.Id, keyText, SecretKind.KeyText))
        {
            Report("The private key could not be saved, so this server was not added.");
            throw new InvalidOperationException("the key could not be stored");
        }

        var host = Adopt(saved);
        Store.Save([.. Hosts.Select(h => h.Saved)]);
        return host;
    }

    public void Remove(Host host)
    {
        _core.RemoveTarget(host.TargetId);
        Store.Forget(host.Saved);
        Hosts.Remove(host);
        Store.Save([.. Hosts.Select(h => h.Saved)]);
    }

    /// <summary>Format a value the way every ServerGlass UI formats it.</summary>
    public string Format(MetricGauge gauge) => _core.Format(gauge);

    public string Format(double value, string unit, bool binary) => _core.Format(value, unit, binary);

    public string FormatDuration(double seconds) => _core.FormatDuration(seconds);

    /// <summary>Normalise a series for a sparkline, using the core's rule rather than one of ours.</summary>
    public IReadOnlyList<double> Sparkline(IReadOnlyList<double> history) =>
        _core.SparklinePoints(history);

    /// <summary>Run a command off the UI thread. It blocks in the core until the host answers.</summary>
    public Task<CommandResult> RunCommand(Host host, string command) =>
        Task.Run(() => _core.RunCommand(host.TargetId, command));

    public Task<ReceiverStarted> StartReceiving(IReadOnlyList<string> addresses) =>
        Task.Run(() => _core.StartReceiving(addresses));

    public Task<string> AwaitPairing(ulong id) => Task.Run(() => _core.AwaitPairingConnection(id));

    public Task<SyncBundle> ReceivePairing(ulong id) => Task.Run(() => _core.ReceivePairing(id));

    public Task<SenderConnected> ScanPairingCode(string code) => Task.Run(() => _core.ScanPairingCode(code));

    public Task SendPairing(ulong id, SyncBundle bundle) => Task.Run(() => _core.SendPairing(id, bundle));

    public void ForgetPairing(ulong id) => _core.ForgetPairing(id);

    public MergeResult MergeBundle(SyncBundle existing, SyncBundle incoming) =>
        _core.MergeBundle(existing, incoming);

    /// <summary>Everything this device is worth advertising at, for pairing.</summary>
    /// <remarks>
    /// Every address, not the "best" one. Which address reaches depends on where the other device
    /// is — over WireGuard or Tailscale the tunnel address is often the only one that works — and
    /// the scanner tries each in turn, so a spare costs one failed connection while a missing one
    /// costs the pairing. This needs no permission on Windows; nothing here asks for location.
    /// </remarks>
    public static IReadOnlyList<string> LocalAddresses() =>
    [
        .. System.Net.NetworkInformation.NetworkInterface.GetAllNetworkInterfaces()
            .Where(n => n.OperationalStatus == System.Net.NetworkInformation.OperationalStatus.Up)
            .SelectMany(n => n.GetIPProperties().UnicastAddresses)
            .Select(a => a.Address)
            .Where(a => !System.Net.IPAddress.IsLoopback(a))
            .Where(a => a.AddressFamily is System.Net.Sockets.AddressFamily.InterNetwork
                                        or System.Net.Sockets.AddressFamily.InterNetworkV6)
            .Select(a => a.ToString())
            .Distinct(),
    ];

    private async Task PollForever(CancellationToken token)
    {
        using var timer = new PeriodicTimer(TimeSpan.FromMilliseconds(1000));
        while (await timer.WaitForNextTickAsync(token).ConfigureAwait(false))
        {
            // The list is only mutated on the UI thread, so a copy is enough to iterate safely.
            var hosts = Hosts.ToArray();
            foreach (var host in hosts)
            {
                TargetSnapshot snapshot;
                try
                {
                    snapshot = _core.Snapshot(host.TargetId);
                }
                catch (SgException)
                {
                    // The target was removed between the copy and the call. Nothing to draw.
                    continue;
                }

                _dispatcher.TryEnqueue(() => host.Snapshot = snapshot);
            }
        }
    }

    public void Dispose()
    {
        _stopping.Cancel();
        _stopping.Dispose();
        _core.Dispose();
    }
}
