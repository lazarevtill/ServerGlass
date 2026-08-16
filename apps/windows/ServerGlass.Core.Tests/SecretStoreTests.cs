using ServerGlass.Core;
using Xunit;

namespace ServerGlass.Core.Tests;

/// <summary>
/// The real DPAPI-backed store.
/// </summary>
/// <remarks>
/// <see cref="HostStoreTests"/> uses an in-memory double so it can run anywhere; this exercises the
/// platform calls for real, because a store that agrees with its own interface proves nothing about
/// whether it agrees with Windows. Every test gets its own directory and cleans up after itself.
/// </remarks>
public sealed class SecretStoreTests : IDisposable
{
    private readonly string _directory =
        Path.Combine(Path.GetTempPath(), "sg-secrets-" + Guid.NewGuid().ToString("N"));

    private readonly DpapiSecretStore _store;

    public SecretStoreTests() => _store = new DpapiSecretStore(_directory);

    [Fact]
    public void A_secret_survives_a_write_and_a_read()
    {
        Assert.True(_store.Set("round-trip", SecretKind.Password, "hunter2"));
        Assert.Equal("hunter2", _store.Get("round-trip", SecretKind.Password));
    }

    /// <summary>
    /// A host can have both a pasted key and the passphrase protecting it. On Apple these were
    /// originally stored under the same account and silently overwrote each other.
    /// </summary>
    [Fact]
    public void A_key_and_its_passphrase_do_not_overwrite_each_other()
    {
        Assert.True(_store.Set("both", SecretKind.Password, "passphrase"));
        Assert.True(_store.Set("both", SecretKind.KeyText, "-----BEGIN OPENSSH PRIVATE KEY-----"));

        Assert.Equal("passphrase", _store.Get("both", SecretKind.Password));
        Assert.StartsWith("-----BEGIN", _store.Get("both", SecretKind.KeyText));
    }

    [Fact]
    public void An_absent_secret_reads_as_null_rather_than_throwing()
    {
        Assert.Null(_store.Get("never-written", SecretKind.Password));
    }

    /// <summary>
    /// An empty secret means "there is none", not "store an empty string" — a stored empty
    /// passphrase would be handed to the transport and fail differently from no passphrase.
    /// </summary>
    [Fact]
    public void An_empty_secret_removes_rather_than_stores()
    {
        _store.Set("empty", SecretKind.Password, "something");
        _store.Set("empty", SecretKind.Password, "");
        Assert.Null(_store.Get("empty", SecretKind.Password));
    }

    [Fact]
    public void Forgetting_erases_both_kinds()
    {
        _store.Set("forget", SecretKind.Password, "hunter2");
        _store.Set("forget", SecretKind.KeyText, "key-material");

        _store.Forget("forget");

        Assert.Null(_store.Get("forget", SecretKind.Password));
        Assert.Null(_store.Get("forget", SecretKind.KeyText));
    }

    /// <summary>
    /// The test that chose this implementation.
    /// </summary>
    /// <remarks>
    /// The first version of this store used the Windows Credential Manager, which is the closer
    /// analogue to the Keychain. <c>CredWriteW</c> caps a blob at 2560 bytes, and a pasted private
    /// key — the entire point of the <c>key_text</c> sign-in method — is larger than that for
    /// anything but a short ed25519 key. It returned false, and the app would then have handed the
    /// transport an empty key and blamed the key. DPAPI has no such cap.
    /// </remarks>
    [Fact]
    public void A_multi_kilobyte_key_survives_intact()
    {
        var key = "-----BEGIN OPENSSH PRIVATE KEY-----\n"
                  + string.Concat(Enumerable.Repeat("b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAAB\n", 60))
                  + "-----END OPENSSH PRIVATE KEY-----\n";

        Assert.True(key.Length > 2560, "the point of this test is a key past the old 2560-byte cap");
        Assert.True(_store.Set("large", SecretKind.KeyText, key));
        Assert.Equal(key, _store.Get("large", SecretKind.KeyText));
    }

    /// <summary>Passphrases are not ASCII-only, and encoding round-trips are where that shows.</summary>
    [Fact]
    public void A_non_ascii_secret_survives_intact()
    {
        const string secret = "пароль-🔐-Ünïcödé";
        Assert.True(_store.Set("unicode", SecretKind.Password, secret));
        Assert.Equal(secret, _store.Get("unicode", SecretKind.Password));
    }

    [Fact]
    public void Rewriting_a_secret_replaces_it()
    {
        _store.Set("rewrite", SecretKind.Password, "first");
        _store.Set("rewrite", SecretKind.Password, "second");
        Assert.Equal("second", _store.Get("rewrite", SecretKind.Password));
    }

    /// <summary>Nothing readable may sit on disk — that is the whole reason this store exists.</summary>
    [Fact]
    public void The_secret_is_not_stored_in_the_clear()
    {
        _store.Set("plaintext-check", SecretKind.Password, "hunter2-in-the-clear");

        foreach (var file in Directory.GetFiles(_directory))
        {
            var bytes = File.ReadAllBytes(file);
            Assert.DoesNotContain("hunter2-in-the-clear", System.Text.Encoding.UTF8.GetString(bytes), StringComparison.Ordinal);
            Assert.DoesNotContain("hunter2-in-the-clear", System.Text.Encoding.Unicode.GetString(bytes), StringComparison.Ordinal);
        }
    }

    /// <summary>
    /// The host id must not reach the filesystem as a path. It is ours today; a filename built by
    /// concatenation is a traversal waiting for the day it is not.
    /// </summary>
    [Fact]
    public void A_hostile_host_id_cannot_escape_the_directory()
    {
        const string hostile = @"..\..\..\escaped";
        Assert.True(_store.Set(hostile, SecretKind.Password, "hunter2"));
        Assert.Equal("hunter2", _store.Get(hostile, SecretKind.Password));

        // Everything written stayed in the directory we own.
        Assert.All(
            Directory.GetFiles(_directory),
            file => Assert.Equal(
                Path.GetFullPath(_directory),
                Path.GetFullPath(Path.GetDirectoryName(file)!)));
    }

    /// <summary>
    /// A blob that cannot be decrypted is reported, not silently treated as "no secret" — which
    /// would surface one screen later as an authentication failure and point the user at their
    /// password instead of at the storage that actually broke.
    /// </summary>
    [Fact]
    public void An_undecryptable_secret_is_reported_rather_than_read_as_absent()
    {
        _store.Set("corrupt", SecretKind.Password, "hunter2");
        var file = Directory.GetFiles(_directory).Single();
        File.WriteAllBytes(file, [0x00, 0x01, 0x02, 0x03, 0x04]);

        Exception? reported = null;
        _store.Unreadable += (_, error) => reported = error;

        Assert.Null(_store.Get("corrupt", SecretKind.Password));
        Assert.NotNull(reported);
    }

    public void Dispose()
    {
        if (Directory.Exists(_directory))
        {
            Directory.Delete(_directory, recursive: true);
        }
    }
}
