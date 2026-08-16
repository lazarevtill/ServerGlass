using System.Text.Json;

namespace ServerGlass.Core;

/// <summary>Everything needed to reconnect to a host, minus the secret.</summary>
public sealed record SavedHost
{
    public string Id { get; init; } = Guid.NewGuid().ToString();
    public string Address { get; init; } = "";
    public ushort Port { get; init; } = 22;
    public string User { get; init; } = "";

    /// <summary><c>agent</c>, <c>key</c>, <c>key_text</c> or <c>password</c>.</summary>
    public string AuthKind { get; init; } = "agent";

    public string? KeyPath { get; init; }
    public string HostKeyPolicy { get; init; } = "strict";
    public ulong RefreshMs { get; init; } = 1000;
}

/// <summary>
/// Persistence for the servers a person has added.
/// </summary>
/// <remarks>
/// <para>
/// Two stores, deliberately, exactly as on the Apple and Android sides:
/// </para>
/// <list type="bullet">
/// <item><b>Configuration</b> — address, port, username, which sign-in method, key path — is not
/// secret. It goes in <c>hosts.json</c>, where it can be inspected and backed up like any other
/// preference.</item>
/// <item><b>Secrets</b> — passwords, key passphrases and pasted key bodies — go to DPAPI through
/// <see cref="ISecretStore"/>, and only ever there. <see cref="SavedHost"/> has no field to put one
/// in, which is the point: it cannot be written to disk in the clear by accident.</item>
/// </list>
/// <para>
/// Until a store existed on the other platforms, adding a server was pointless — the list lived
/// only in memory, so closing the app threw it away and every launch started from the empty state.
/// </para>
/// </remarks>
public sealed class HostStore
{
    /// <summary>Overrides where the store keeps its files. Set by tests; honoured everywhere.</summary>
    public const string DirectoryVariable = "SERVERGLASS_DATA_DIR";

    private static readonly JsonSerializerOptions FileJson = new()
    {
        WriteIndented = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        PropertyNameCaseInsensitive = true,
    };

    private readonly ISecretStore _secrets;

    public HostStore(ISecretStore secrets, string? directory = null)
    {
        _secrets = secrets;
        Directory = directory
            ?? Environment.GetEnvironmentVariable(DirectoryVariable)
            ?? Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "ServerGlass");
    }

    /// <summary>Where the host list and the trusted host keys live.</summary>
    public string Directory { get; }

    public string HostsPath => Path.Combine(Directory, "hosts.json");

    /// <summary>
    /// Where trusted host keys are recorded.
    /// </summary>
    /// <remarks>
    /// Passed to the core rather than left to it. Windows has no <c>$HOME</c> — it has
    /// <c>USERPROFILE</c> — and an app that guesses wrong here records nothing, which means "remember
    /// this server's identity" silently does nothing and a substituted key is accepted on every
    /// later connection. The core creates the containing directory and reports a write it could
    /// not make, rather than discarding it.
    /// </remarks>
    public string KnownHostsPath => Path.Combine(Directory, "known_hosts");

    public IReadOnlyList<SavedHost> Load()
    {
        if (!File.Exists(HostsPath))
        {
            return [];
        }

        try
        {
            return JsonSerializer.Deserialize<List<SavedHost>>(File.ReadAllText(HostsPath), FileJson)
                   ?? [];
        }
        catch (Exception error) when (error is JsonException or IOException)
        {
            // A corrupt or unreadable list must not stop the app from starting. It is reported
            // rather than swallowed, because a discarded error is a silent failure.
            LoadFailed?.Invoke(this, error);
            return [];
        }
    }

    /// <summary>Raised when the saved list could not be read. The app shows it; it is not fatal.</summary>
    public event EventHandler<Exception>? LoadFailed;

    public void Save(IReadOnlyList<SavedHost> hosts)
    {
        System.IO.Directory.CreateDirectory(Directory);
        // Written to a temporary file and moved into place, so a crash mid-write cannot leave a
        // half-written list where a complete one used to be.
        var temporary = HostsPath + ".tmp";
        File.WriteAllText(temporary, JsonSerializer.Serialize(hosts, FileJson));
        File.Move(temporary, HostsPath, overwrite: true);
    }

    public string? Secret(string hostId, SecretKind kind = SecretKind.Password) =>
        _secrets.Get(hostId, kind);

    /// <returns><c>false</c> when the credential store refused to keep the secret.</returns>
    public bool SetSecret(string hostId, string? secret, SecretKind kind = SecretKind.Password) =>
        _secrets.Set(hostId, kind, secret);

    /// <summary>Erase everything secret belonging to a host.</summary>
    public void Forget(SavedHost host) => _secrets.Forget(host.Id);

    /// <summary>
    /// Build the config the core wants, pulling the secret out of the credential store at the last
    /// moment.
    /// </summary>
    /// <remarks>
    /// Fetched per connection rather than held alongside the rest of the host, so it exists in
    /// managed memory for as short a time as the language allows.
    /// </remarks>
    public TargetConfig Config(SavedHost host) => new()
    {
        Host = host.Address,
        Port = host.Port,
        User = host.User,
        AuthKind = host.AuthKind,
        KeyPath = host.KeyPath,
        // A pasted key is key material, so it lives beside the passphrase in the credential store
        // rather than in the saved record. Stored under its own name so a key and its passphrase
        // can both exist for the same host.
        KeyText = _secrets.Get(host.Id, SecretKind.KeyText),
        Secret = _secrets.Get(host.Id, SecretKind.Password),
        HostKeyPolicy = host.HostKeyPolicy,
        KnownHostsPath = KnownHostsPath,
        RefreshMs = host.RefreshMs,
    };
}
