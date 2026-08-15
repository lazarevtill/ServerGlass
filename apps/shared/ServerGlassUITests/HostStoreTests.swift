import Foundation
import Testing

@testable import ServerGlassUI

/// What the Swift layer owns that is not a view: how a host is stored, and what is kept in the
/// Keychain rather than beside it.
///
/// None of this had a test. The bugs that reached a device — a host that vanished on relaunch, an
/// edit form that silently became an add form, a Keychain refusal reported as a connection failure
/// — were all in this layer, and all found by hand.
///
/// `UserDefaults` and the Keychain are process-wide, so each test uses its own identifiers and
/// removes what it wrote.
@Suite(.serialized)
struct HostStoreTests {
    /// A host nobody else's test will collide with.
    private func sample(_ address: String = "10.0.0.9") -> HostStore.SavedHost {
        HostStore.SavedHost(
            address: address, port: 2222, user: "root", authKind: "password",
            keyPath: nil, hostKeyPolicy: "accept_new", refreshMs: 1500)
    }

    private func clear() {
        for host in HostStore.load() { HostStore.forget(host) }
        HostStore.save([])
    }

    @Test("a saved host survives being written and read back")
    func roundTrip() throws {
        clear()
        defer { clear() }

        let host = sample()
        HostStore.save([host])

        let loaded = HostStore.load()
        #expect(loaded.count == 1)
        #expect(loaded.first == host, "every field must survive, not just the address")
    }

    /// The bug that shipped: the list lived only in memory, so adding a server and closing the app
    /// threw it away. Nothing about the record may depend on the process that wrote it.
    @Test("the identifier is stable across a save and load")
    func identifierIsStable() throws {
        clear()
        defer { clear() }

        let host = sample()
        HostStore.save([host])
        #expect(HostStore.load().first?.id == host.id)
    }

    /// The record is what gets backed up and inspected. A password in it would be a password on
    /// disk in the clear.
    @Test("no secret is ever written into the saved record")
    func secretsStayOutOfTheRecord() throws {
        clear()
        defer { clear() }

        let host = sample()
        Keychain.setSecret("hunter2", for: host.id)
        HostStore.save([host])
        defer { Keychain.setSecret(nil, for: host.id) }

        let encoded = try JSONEncoder().encode(HostStore.load())
        let text = String(decoding: encoded, as: UTF8.self)
        #expect(!text.contains("hunter2"), "the password was written into the record: \(text)")
    }

    /// A host can have both a pasted key *and* the passphrase protecting it, and one must not
    /// overwrite the other — they were originally stored under the same account.
    @Test("a key and its passphrase are stored separately")
    func keyAndPassphraseCoexist() throws {
        clear()
        defer { clear() }

        let host = sample()
        defer {
            Keychain.setSecret(nil, for: host.id)
            Keychain.setSecret(nil, for: host.id, kind: .keyText)
        }

        // Skipped rather than failed where the Keychain is unavailable — an unsigned build has no
        // keychain-access group, and that is a property of the build, not of this code.
        guard Keychain.setSecret("passphrase", for: host.id) else { return }
        #expect(Keychain.setSecret("-----BEGIN OPENSSH PRIVATE KEY-----", for: host.id, kind: .keyText))

        #expect(Keychain.secret(for: host.id) == "passphrase")
        #expect(Keychain.secret(for: host.id, kind: .keyText)?.hasPrefix("-----BEGIN") == true)
    }

    /// Removing a host must take its secrets with it. Leaving them behind means a password for a
    /// server the user believes they deleted stays on the device.
    @Test("forgetting a host erases both of its secrets")
    func forgettingErasesSecrets() throws {
        clear()
        defer { clear() }

        let host = sample()
        guard Keychain.setSecret("hunter2", for: host.id) else { return }
        _ = Keychain.setSecret("key-material", for: host.id, kind: .keyText)

        HostStore.forget(host)

        #expect(Keychain.secret(for: host.id) == nil)
        #expect(Keychain.secret(for: host.id, kind: .keyText) == nil)
    }

    /// The config handed to the core carries the secret; the record it was built from does not.
    @Test("the config is assembled with the secret fetched at the last moment")
    func configPullsTheSecret() throws {
        clear()
        defer { clear() }

        let host = sample("192.0.2.5")
        guard Keychain.setSecret("hunter2", for: host.id) else { return }
        defer { Keychain.setSecret(nil, for: host.id) }

        let config = HostStore.config(for: host)
        #expect(config.host == "192.0.2.5")
        #expect(config.port == 2222)
        #expect(config.refreshMs == 1500)
        #expect(config.secret == "hunter2")
    }

    /// An empty secret means "there is none", not "store an empty string" — a stored empty
    /// passphrase would be handed to the transport and fail differently from no passphrase.
    @Test("an empty secret removes rather than stores")
    func emptySecretRemoves() throws {
        let id = "empty-secret-test"
        _ = Keychain.setSecret("something", for: id)
        _ = Keychain.setSecret("", for: id)
        #expect(Keychain.secret(for: id) == nil)
        Keychain.setSecret(nil, for: id)
    }
}
