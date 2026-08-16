using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;

namespace ServerGlass.Core;

/// <summary>A failure the core reported, with the plain-language detail it wrote.</summary>
/// <remarks>
/// <see cref="Recoverable"/> is carried across on purpose: it is what lets the UI say "ServerGlass
/// will keep retrying" rather than guessing. Bad credentials and a changed host key are not
/// recoverable, and telling someone to wait for a retry that will never come is worse than saying
/// nothing.
/// </remarks>
public sealed class SgException(string kind, string detail, bool recoverable)
    : Exception(detail)
{
    /// <summary><c>unknownTarget</c>, <c>connection</c>, <c>pairing</c> or <c>internal</c>.</summary>
    public string Kind { get; } = kind;

    public bool Recoverable { get; } = recoverable;
}

/// <summary>
/// The Rust core, as the Windows app sees it.
/// </summary>
/// <remarks>
/// <para>
/// Everything crosses the boundary as UTF-8 JSON in an <c>{ok|err}</c> envelope; see the module
/// header of <c>crates/sg-ffi/src/cabi.rs</c> for why that beats describing a nested snapshot to a
/// C compiler twice. This class is the only place in the app that touches a pointer.
/// </para>
/// <para>
/// <see cref="Snapshot"/> is cheap and meant for a display timer. <see cref="RunCommand"/>,
/// <see cref="AwaitPairingConnection"/> and <see cref="ScanPairingCode"/> block — call them off the
/// UI thread.
/// </para>
/// </remarks>
public sealed unsafe class ServerGlassCore : IDisposable
{
    /// <summary>
    /// camelCase both ways, matching what UniFFI already produces for Swift and Kotlin — so one
    /// property name is right on all four platforms. Case-insensitive on read so a casing slip is
    /// a mismatch that still works rather than a field that silently stays null.
    /// </summary>
    internal static readonly JsonSerializerOptions Json = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        PropertyNameCaseInsensitive = true,
    };

    private void* _handle;
    private bool _disposed;

    public ServerGlassCore()
    {
        _handle = Native.sg_core_new();
        if (_handle is null)
        {
            throw new SgException("internal", "The ServerGlass core could not be started.", false);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Targets
    // -----------------------------------------------------------------------------------------

    /// <summary>Register a host. Does not connect; call <see cref="Start"/> for that.</summary>
    public string AddTarget(TargetConfig config)
    {
        var json = Utf8(JsonSerializer.Serialize(config, Json));
        fixed (byte* p = json)
        {
            return Read<string>(Native.sg_add_target(Handle, p));
        }
    }

    /// <summary>Connect and begin refreshing. Returns immediately; watch the snapshot for progress.</summary>
    public void Start(string targetId)
    {
        fixed (byte* p = Utf8(targetId))
        {
            ReadVoid(Native.sg_start(Handle, p));
        }
    }

    /// <summary>Stop refreshing and drop the connection.</summary>
    public void Stop(string targetId)
    {
        fixed (byte* p = Utf8(targetId))
        {
            ReadVoid(Native.sg_stop(Handle, p));
        }
    }

    public void RemoveTarget(string targetId)
    {
        fixed (byte* p = Utf8(targetId))
        {
            ReadVoid(Native.sg_remove_target(Handle, p));
        }
    }

    /// <summary>The most recent completed refresh. Cheap enough to call on a display timer.</summary>
    public TargetSnapshot Snapshot(string targetId)
    {
        fixed (byte* p = Utf8(targetId))
        {
            return Read<TargetSnapshot>(Native.sg_snapshot(Handle, p));
        }
    }

    public IReadOnlyList<string> TargetIds() =>
        Read<List<string>>(Native.sg_target_ids(Handle));

    /// <summary>
    /// Run one command on the host and wait for what it printed.
    /// </summary>
    /// <remarks>
    /// <b>Blocks.</b> It runs on the connection the readings already use — one round trip, no
    /// second sign-in — which also means the host must be online. There is no PTY, so anything
    /// interactive produces nothing useful until the core's sixty-second timeout.
    /// </remarks>
    public CommandResult RunCommand(string targetId, string command)
    {
        fixed (byte* id = Utf8(targetId))
        fixed (byte* cmd = Utf8(command))
        {
            return Read<CommandResult>(Native.sg_run_command(Handle, id, cmd));
        }
    }

    // -----------------------------------------------------------------------------------------
    // Formatting
    //
    // Exported by the core so this app cannot re-implement it. Number formatting drifting between
    // front-ends is how the same host came to read differently on a phone and a desk.
    // -----------------------------------------------------------------------------------------

    public string Format(double value, string unitSuffix, bool binaryScaled)
    {
        fixed (byte* unit = Utf8(unitSuffix))
        {
            return Read<string>(Native.sg_format(Handle, value, unit, binaryScaled));
        }
    }

    /// <summary>Format a gauge the way every ServerGlass UI formats it, uptime included.</summary>
    /// <remarks>
    /// Uptime is the one metric whose gauge value is not formatted by <see cref="Format(double,
    /// string, bool)"/>: it is seconds, and "11324 s" is not something anybody reads. Both the
    /// Apple and Android layers special-case it to the duration formatter, so this does too — the
    /// alternative is a Windows dashboard that says something different from the phone about the
    /// same host, which is exactly the divergence this project keeps having to fix.
    /// </remarks>
    public string Format(MetricGauge gauge) =>
        gauge.Metric == "uptime"
            ? FormatDuration(gauge.Value)
            : Format(gauge.Value, gauge.UnitSuffix, gauge.BinaryScaled);

    public string FormatDuration(double seconds) =>
        Read<string>(Native.sg_format_duration(Handle, seconds));

    /// <summary>
    /// Normalise a series to 0-1 for a sparkline, oldest first.
    /// </summary>
    /// <remarks>
    /// The noise floor — the rule that a disk creeping from 5.19% to 5.20% must not draw a cliff —
    /// belongs to the core. It was written by hand in Swift and again in Kotlin before it moved
    /// there, and CLAUDE.md now says in as many words that the fix for a rule living in the
    /// front-ends is to move it into <c>sg-ffi</c> and call it, "not to write it a third time".
    /// </remarks>
    public IReadOnlyList<double> SparklinePoints(IReadOnlyList<double> history)
    {
        if (history.Count == 0)
        {
            return [];
        }

        fixed (byte* p = Utf8(JsonSerializer.Serialize(history, Json)))
        {
            return Read<List<double>>(Native.sg_sparkline_points(Handle, p));
        }
    }

    // -----------------------------------------------------------------------------------------
    // Pairing
    // -----------------------------------------------------------------------------------------

    /// <summary>
    /// Offer this device as the destination for a transfer.
    /// </summary>
    /// <param name="advertiseHosts">
    /// Every address this device might be reachable at. Pass all of them — over WireGuard or
    /// Tailscale the tunnel address is often the only one that reaches, and a missing address costs
    /// the pairing while a spare one costs a single failed connection.
    /// </param>
    public ReceiverStarted StartReceiving(IReadOnlyList<string> advertiseHosts)
    {
        fixed (byte* p = Utf8(JsonSerializer.Serialize(advertiseHosts, Json)))
        {
            return Read<ReceiverStarted>(Native.sg_start_receiving(Handle, p));
        }
    }

    /// <summary>
    /// Block until the other device connects, then return the code to show.
    /// </summary>
    /// <remarks>
    /// Nothing has been received when this returns. The caller shows this code, the user compares
    /// it with the other screen, and only then calls <see cref="ReceivePairing"/>. The whole
    /// security of the exchange rests on that comparison happening first, which is why it is two
    /// calls and not one.
    /// </remarks>
    public string AwaitPairingConnection(ulong id) =>
        Read<string>(Native.sg_receiver_await_connection(Handle, id));

    /// <summary>Take the transfer. Call only after the user confirmed the codes match.</summary>
    public SyncBundle ReceivePairing(ulong id) =>
        Read<SyncBundle>(Native.sg_receiver_receive(Handle, id));

    /// <summary>Connect to a pairing code from the other device. Nothing is sent yet.</summary>
    public SenderConnected ScanPairingCode(string code)
    {
        fixed (byte* p = Utf8(code))
        {
            return Read<SenderConnected>(Native.sg_scan_pairing_code(Handle, p));
        }
    }

    /// <summary>Send the bundle. Call only after the user confirmed the codes match.</summary>
    public void SendPairing(ulong id, SyncBundle bundle)
    {
        fixed (byte* p = Utf8(JsonSerializer.Serialize(bundle, Json)))
        {
            ReadVoid(Native.sg_sender_send(Handle, id, p));
        }
    }

    public void ForgetPairing(ulong id) => Native.sg_pairing_forget(Handle, id);

    /// <summary>
    /// Apply a received bundle to what this device already has.
    /// </summary>
    /// <remarks>
    /// Pure: it decides, it does not store. The caller writes the result to its own keystore and
    /// shows the conflicts — which are never applied, because a sync channel that can quietly
    /// rewrite a host key pin is a machine-in-the-middle with extra steps.
    /// </remarks>
    public MergeResult MergeBundle(SyncBundle existing, SyncBundle incoming)
    {
        fixed (byte* a = Utf8(JsonSerializer.Serialize(existing, Json)))
        fixed (byte* b = Utf8(JsonSerializer.Serialize(incoming, Json)))
        {
            return Read<MergeResult>(Native.sg_merge_bundle(Handle, a, b));
        }
    }

    // -----------------------------------------------------------------------------------------
    // Plumbing
    // -----------------------------------------------------------------------------------------

    private void* Handle =>
        _disposed
            ? throw new ObjectDisposedException(nameof(ServerGlassCore))
            : _handle;

    /// <summary>A string as NUL-terminated UTF-8, ready to pin.</summary>
    private static byte[] Utf8(string text)
    {
        var bytes = new byte[Encoding.UTF8.GetByteCount(text) + 1];
        Encoding.UTF8.GetBytes(text, bytes);
        // The last byte stays zero: the Rust side reads these with CStr::from_ptr.
        return bytes;
    }

    /// <summary>Read a reply, free it, and unwrap the envelope.</summary>
    private static T Read<T>(byte* reply)
    {
        var json = Consume(reply);
        using var document = JsonDocument.Parse(json);
        ThrowIfError(document);
        var ok = document.RootElement.GetProperty("ok");
        return ok.Deserialize<T>(Json)
               ?? throw new SgException("internal", "The core replied with nothing.", false);
    }

    private static void ReadVoid(byte* reply)
    {
        using var document = JsonDocument.Parse(Consume(reply));
        ThrowIfError(document);
    }

    /// <summary>
    /// Take ownership of a reply string and hand the allocation back to Rust.
    /// </summary>
    /// <remarks>
    /// The free happens in a <c>finally</c>: a reply that cannot be decoded is still a reply that
    /// was allocated, and leaking one per failure would be a slow leak nobody notices — the kind
    /// this project has been bitten by before.
    /// </remarks>
    private static string Consume(byte* reply)
    {
        if (reply is null)
        {
            throw new SgException("internal", "The core returned nothing at all.", false);
        }

        try
        {
            return Marshal.PtrToStringUTF8((IntPtr)reply) ?? "";
        }
        finally
        {
            Native.sg_string_free(reply);
        }
    }

    private static void ThrowIfError(JsonDocument document)
    {
        if (!document.RootElement.TryGetProperty("err", out var error))
        {
            return;
        }

        throw new SgException(
            error.TryGetProperty("kind", out var kind) ? kind.GetString() ?? "internal" : "internal",
            error.TryGetProperty("detail", out var detail) ? detail.GetString() ?? "" : "",
            error.TryGetProperty("recoverable", out var recoverable) && recoverable.GetBoolean());
    }

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }

        _disposed = true;
        var handle = _handle;
        _handle = null;
        // Drops every target's poll loop with it.
        Native.sg_core_free(handle);
    }
}
