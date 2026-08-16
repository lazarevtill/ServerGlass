using System.Text.Json;
using ServerGlass.Core;
using Xunit;

namespace ServerGlass.Core.Tests;

/// <summary>
/// What the Windows layer owns that is not a view: how a host is stored, and what is kept in the
/// credential store rather than beside it.
/// </summary>
/// <remarks>
/// A direct translation of <c>HostStoreTests.swift</c>, case for case, because the same bugs are
/// available on every platform and "the same code shape on two platforms is not the same behaviour"
/// is on this project's list of lessons. Each test gets its own directory and an in-memory secret
/// store, so nothing here depends on DPAPI — that is covered separately in
/// <see cref="SecretStoreTests"/>.
/// </remarks>
public sealed class HostStoreTests : IDisposable
{
    private readonly string _directory =
        Path.Combine(Path.GetTempPath(), "sg-hoststore-" + Guid.NewGuid().ToString("N"));

    private readonly InMemorySecretStore _secrets = new();

    private HostStore Store() => new(_secrets, _directory);

    private static SavedHost Sample(string address = "10.0.0.9") => new()
    {
        Address = address,
        Port = 2222,
        User = "root",
        AuthKind = "password",
        KeyPath = null,
        HostKeyPolicy = "accept_new",
        RefreshMs = 1500,
    };

    [Fact]
    public void A_saved_host_survives_being_written_and_read_back()
    {
        var store = Store();
        var host = Sample();
        store.Save([host]);

        var loaded = store.Load();
        Assert.Single(loaded);
        Assert.Equal(host, loaded[0]);
    }

    /// <summary>
    /// The bug that shipped elsewhere: the list lived only in memory, so adding a server and
    /// closing the app threw it away. Nothing about the record may depend on the process that
    /// wrote it.
    /// </summary>
    [Fact]
    public void The_identifier_is_stable_across_a_save_and_load()
    {
        var store = Store();
        var host = Sample();
        store.Save([host]);
        Assert.Equal(host.Id, store.Load()[0].Id);
    }

    /// <summary>
    /// The record is what gets backed up and inspected. A password in it would be a password on
    /// disk in the clear.
    /// </summary>
    [Fact]
    public void No_secret_is_ever_written_into_the_saved_record()
    {
        var store = Store();
        var host = Sample();
        store.SetSecret(host.Id, "hunter2");
        store.SetSecret(host.Id, "-----BEGIN OPENSSH PRIVATE KEY-----", SecretKind.KeyText);
        store.Save([host]);

        var onDisk = File.ReadAllText(store.HostsPath);
        Assert.DoesNotContain("hunter2", onDisk, StringComparison.Ordinal);
        Assert.DoesNotContain("BEGIN OPENSSH", onDisk, StringComparison.Ordinal);

        // And the type itself has nowhere to put one, which is the stronger guarantee: this cannot
        // regress by someone adding a field, only by someone changing the record on purpose.
        var serialised = JsonSerializer.Serialize(host);
        Assert.DoesNotContain("secret", serialised, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("keyText", serialised, StringComparison.OrdinalIgnoreCase);
    }

    /// <summary>
    /// A host can have both a pasted key <em>and</em> the passphrase protecting it, and one must
    /// not overwrite the other — they were originally stored under the same account on Apple.
    /// </summary>
    [Fact]
    public void A_key_and_its_passphrase_are_stored_separately()
    {
        var store = Store();
        var host = Sample();

        Assert.True(store.SetSecret(host.Id, "passphrase"));
        Assert.True(store.SetSecret(host.Id, "-----BEGIN OPENSSH PRIVATE KEY-----", SecretKind.KeyText));

        Assert.Equal("passphrase", store.Secret(host.Id));
        Assert.StartsWith("-----BEGIN", store.Secret(host.Id, SecretKind.KeyText));
    }

    /// <summary>
    /// Removing a host must take its secrets with it. Leaving them behind means a password for a
    /// server the user believes they deleted stays on the device.
    /// </summary>
    [Fact]
    public void Forgetting_a_host_erases_both_of_its_secrets()
    {
        var store = Store();
        var host = Sample();
        store.SetSecret(host.Id, "hunter2");
        store.SetSecret(host.Id, "key-material", SecretKind.KeyText);

        store.Forget(host);

        Assert.Null(store.Secret(host.Id));
        Assert.Null(store.Secret(host.Id, SecretKind.KeyText));
    }

    /// <summary>The config handed to the core carries the secret; the record it was built from does not.</summary>
    [Fact]
    public void The_config_is_assembled_with_the_secret_fetched_at_the_last_moment()
    {
        var store = Store();
        var host = Sample("192.0.2.5");
        store.SetSecret(host.Id, "hunter2");

        var config = store.Config(host);
        Assert.Equal("192.0.2.5", config.Host);
        Assert.Equal(2222, config.Port);
        Assert.Equal(1500ul, config.RefreshMs);
        Assert.Equal("hunter2", config.Secret);
        Assert.Equal("accept_new", config.HostKeyPolicy);
    }

    /// <summary>
    /// An empty secret means "there is none", not "store an empty string" — a stored empty
    /// passphrase would be handed to the transport and fail differently from no passphrase.
    /// </summary>
    [Fact]
    public void An_empty_secret_removes_rather_than_stores()
    {
        var store = Store();
        store.SetSecret("empty-secret-test", "something");
        store.SetSecret("empty-secret-test", "");
        Assert.Null(store.Secret("empty-secret-test"));
    }

    /// <summary>
    /// Windows has no <c>$HOME</c>. The core is handed an explicit path because guessing wrong
    /// means "remember this server's identity" records nothing, and a substituted key is then
    /// accepted on every later connection.
    /// </summary>
    [Fact]
    public void The_known_hosts_path_is_explicit_and_inside_the_data_directory()
    {
        var store = Store();
        Assert.Equal(Path.Combine(_directory, "known_hosts"), store.KnownHostsPath);
        Assert.Equal(store.KnownHostsPath, store.Config(Sample()).KnownHostsPath);
    }

    /// <summary>An unreadable list must not stop the app starting — and must not fail silently either.</summary>
    [Fact]
    public void A_corrupt_host_list_is_reported_rather_than_thrown_or_swallowed()
    {
        var store = Store();
        Directory.CreateDirectory(_directory);
        File.WriteAllText(store.HostsPath, "{ this is not the list }");

        Exception? reported = null;
        store.LoadFailed += (_, error) => reported = error;

        Assert.Empty(store.Load());
        Assert.NotNull(reported);
    }

    /// <summary>An empty store is an empty list, not a crash on first launch.</summary>
    [Fact]
    public void A_first_launch_with_no_file_loads_nothing()
    {
        Assert.Empty(Store().Load());
    }

    public void Dispose()
    {
        if (Directory.Exists(_directory))
        {
            Directory.Delete(_directory, recursive: true);
        }
    }
}
