using System.Security.Cryptography;
using System.Text;

namespace ServerGlass.Core;

/// <summary>Which secret. A host can have both — a pasted key <em>and</em> the passphrase protecting it.</summary>
public enum SecretKind
{
    /// <summary>Account password or key passphrase.</summary>
    Password,

    /// <summary>A pasted private key body.</summary>
    KeyText,
}

/// <summary>
/// Where a host's secrets live.
/// </summary>
/// <remarks>
/// An interface so <see cref="HostStore"/> can be tested without touching the machine's real
/// secret storage, and so the one place that handles credentials stays small enough to read in
/// full.
/// </remarks>
public interface ISecretStore
{
    string? Get(string hostId, SecretKind kind);

    /// <returns><c>false</c> when the store refused to keep the secret.</returns>
    bool Set(string hostId, SecretKind kind, string? secret);

    void Forget(string hostId);
}

/// <summary>
/// Secrets held under DPAPI, scoped to the current user.
/// </summary>
/// <remarks>
/// <para>
/// This is the one place the "core owns all logic" rule is deliberately broken, and it is broken
/// the same way on every platform. DPAPI — like the Keychain and the Android Keystore — is an
/// operating-system facility keyed to the signed-in user and unavailable to any other account on
/// the machine. Reimplementing it in the core would mean inventing key management instead of using
/// the one the platform already audits. The core stays stateless about secrets and is handed one
/// per connection.
/// </para>
/// <para>
/// <b>Why DPAPI and not the Credential Manager.</b> The Credential Manager is the closer analogue
/// to the Keychain and was the first implementation here, but <c>CredWriteW</c> caps a credential
/// blob at <c>CRED_MAX_CREDENTIAL_BLOB_SIZE</c> — 2560 bytes. A pasted key is the whole point of
/// the <c>key_text</c> sign-in method, and an RSA-2048 private key in PEM is about 1.7 KB of text,
/// which is 3.4 KB once stored as UTF-16; an RSA-4096 key is twice that. So <c>CredWriteW</c>
/// returned false for exactly the secret this store exists to hold, and the app would then have
/// handed the transport an empty key and reported "could not read the pasted private key" — the
/// person re-pastes a perfectly good key, because nothing told them the saving was what failed.
/// A test covers a multi-kilobyte key so this cannot regress. The Credential Manager is itself
/// built on DPAPI, so nothing is given up in protection by going straight to it.
/// </para>
/// </remarks>
public sealed class DpapiSecretStore : ISecretStore
{
    /// <summary>
    /// Bound into every blob so a file cannot be renamed from one host or kind to another and
    /// still decrypt. It is a namespace, not a secret — it is in the source, and DPAPI's protection
    /// comes from the user's key rather than from this.
    /// </summary>
    private static readonly byte[] Entropy = "cloud.lazarev.serverglass/v1"u8.ToArray();

    private readonly string _directory;

    public DpapiSecretStore(string directory)
    {
        _directory = directory;
        Directory.CreateDirectory(_directory);
    }

    /// <summary>Raised when a stored secret exists but could not be decrypted.</summary>
    /// <remarks>
    /// Surfaced rather than swallowed. A secret that silently reads as absent turns into an
    /// authentication failure one screen later, which points the user at their password instead of
    /// at the storage that actually broke.
    /// </remarks>
    public event EventHandler<Exception>? Unreadable;

    /// <summary>
    /// A stable filename for a host and kind.
    /// </summary>
    /// <remarks>
    /// Hashed rather than used directly: a host id is ours today, but a filename built by
    /// concatenation is a path traversal waiting for the day it is not.
    /// </remarks>
    private string PathFor(string hostId, SecretKind kind)
    {
        var name = Convert.ToHexString(
            SHA256.HashData(Encoding.UTF8.GetBytes($"{hostId}:{kind}")));
        return Path.Combine(_directory, name + ".bin");
    }

    public string? Get(string hostId, SecretKind kind)
    {
        var path = PathFor(hostId, kind);
        if (!File.Exists(path))
        {
            return null;
        }

        byte[]? plain = null;
        try
        {
            plain = ProtectedData.Unprotect(
                File.ReadAllBytes(path), Entropy, DataProtectionScope.CurrentUser);
            return Encoding.UTF8.GetString(plain);
        }
        catch (Exception error) when (error is CryptographicException or IOException)
        {
            Unreadable?.Invoke(this, error);
            return null;
        }
        finally
        {
            if (plain is not null)
            {
                Array.Clear(plain);
            }
        }
    }

    public bool Set(string hostId, SecretKind kind, string? secret)
    {
        var path = PathFor(hostId, kind);

        // An empty secret means "there is none", not "store an empty string". A stored empty
        // passphrase would be handed to the transport and fail differently from no passphrase.
        if (string.IsNullOrEmpty(secret))
        {
            File.Delete(path);
            return true;
        }

        var plain = Encoding.UTF8.GetBytes(secret);
        try
        {
            var sealed_ = ProtectedData.Protect(plain, Entropy, DataProtectionScope.CurrentUser);
            var temporary = path + ".tmp";
            File.WriteAllBytes(temporary, sealed_);
            File.Move(temporary, path, overwrite: true);
            return true;
        }
        catch (Exception error) when (error is CryptographicException or IOException)
        {
            Unreadable?.Invoke(this, error);
            return false;
        }
        finally
        {
            // The plaintext should not linger in a pooled array any longer than the call needs it.
            Array.Clear(plain);
        }
    }

    /// <summary>Erase everything secret belonging to a host.</summary>
    /// <remarks>
    /// Both kinds, because leaving one behind means a password for a server the user believes they
    /// deleted stays on the device.
    /// </remarks>
    public void Forget(string hostId)
    {
        File.Delete(PathFor(hostId, SecretKind.Password));
        File.Delete(PathFor(hostId, SecretKind.KeyText));
    }
}

/// <summary>A secret store held in memory, for tests.</summary>
public sealed class InMemorySecretStore : ISecretStore
{
    private readonly Dictionary<(string, SecretKind), string> _secrets = [];

    public string? Get(string hostId, SecretKind kind) =>
        _secrets.TryGetValue((hostId, kind), out var secret) ? secret : null;

    public bool Set(string hostId, SecretKind kind, string? secret)
    {
        if (string.IsNullOrEmpty(secret))
        {
            _secrets.Remove((hostId, kind));
        }
        else
        {
            _secrets[(hostId, kind)] = secret;
        }

        return true;
    }

    public void Forget(string hostId)
    {
        _secrets.Remove((hostId, SecretKind.Password));
        _secrets.Remove((hostId, SecretKind.KeyText));
    }
}
