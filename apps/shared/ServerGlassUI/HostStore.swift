import Foundation
import ServerGlassFFI

/// Persistence for the servers a person has added.
///
/// Until this existed, adding a server was pointless: the list lived only in memory, so closing the
/// app threw it away and every launch started from the empty state.
///
/// Two stores, deliberately:
///
/// - **Configuration** — address, port, username, which sign-in method, key path — is not secret.
///   It goes in `UserDefaults` as JSON, where it can be inspected and backed up like any other
///   preference.
/// - **Secrets** — passwords and key passphrases — go in the Keychain, and only ever there.
///
/// Secret storage is the one place the "core owns all logic" rule is deliberately broken. The
/// Keychain and the Android Keystore are operating-system facilities backed by hardware the app
/// cannot reach from Rust; reimplementing them in the core would mean inventing key management
/// instead of using the one the platform already audits. The core stays stateless about secrets and
/// is handed one per connection.
public enum HostStore {
    private static let key = "sg.hosts.v1"

    /// Everything needed to reconnect, minus the secret.
    public struct SavedHost: Codable, Identifiable, Equatable {
        public var id: String
        public var address: String
        public var port: UInt16
        public var user: String
        public var authKind: String
        public var keyPath: String?
        public var hostKeyPolicy: String
        public var refreshMs: UInt64

        public init(
            id: String = UUID().uuidString,
            address: String, port: UInt16, user: String, authKind: String,
            keyPath: String?, hostKeyPolicy: String, refreshMs: UInt64
        ) {
            self.id = id
            self.address = address
            self.port = port
            self.user = user
            self.authKind = authKind
            self.keyPath = keyPath
            self.hostKeyPolicy = hostKeyPolicy
            self.refreshMs = refreshMs
        }
    }

    public static func load() -> [SavedHost] {
        guard let data = UserDefaults.standard.data(forKey: key) else { return [] }
        return (try? JSONDecoder().decode([SavedHost].self, from: data)) ?? []
    }

    public static func save(_ hosts: [SavedHost]) {
        guard let data = try? JSONEncoder().encode(hosts) else { return }
        UserDefaults.standard.set(data, forKey: key)
    }

    /// Build the config the core wants, pulling the secret out of the Keychain at the last moment.
    ///
    /// The secret is fetched per connection rather than held alongside the rest of the host, so it
    /// exists in memory for as short a time as the language allows.
    public static func config(for host: SavedHost) -> TargetConfig {
        TargetConfig(
            host: host.address,
            port: host.port,
            user: host.user,
            authKind: host.authKind,
            keyPath: host.keyPath,
            secret: Keychain.secret(for: host.id),
            hostKeyPolicy: host.hostKeyPolicy,
            refreshMs: host.refreshMs
        )
    }

    public static func forget(_ host: SavedHost) {
        Keychain.removeSecret(for: host.id)
    }
}

/// The system Keychain, used for exactly one thing: the secret belonging to a saved host.
enum Keychain {
    private static let service = "cloud.lazarev.serverglass"

    static func setSecret(_ secret: String?, for id: String) {
        removeSecret(for: id)
        guard let secret, !secret.isEmpty, let data = secret.data(using: .utf8) else { return }

        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: id,
            kSecValueData as String: data,
            // Available once the device has been unlocked, and never synced to another device or
            // into a backup: a server password should not travel with an iCloud restore.
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        SecItemAdd(query as CFDictionary, nil)
    }

    static func secret(for id: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: id,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess,
            let data = item as? Data
        else { return nil }
        return String(data: data, encoding: .utf8)
    }

    static func removeSecret(for id: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: id,
        ]
        SecItemDelete(query as CFDictionary)
    }
}
