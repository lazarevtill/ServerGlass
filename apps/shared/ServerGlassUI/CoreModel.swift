import Foundation
import ServerGlassFFI
import SwiftUI

/// A host the user has added, plus its most recent snapshot.
public struct Host: Identifiable, Equatable {
    public let id: String
    public var address: String
    public var snapshot: TargetSnapshot
    /// Identifier of the persisted record, so removing a host can also forget its stored secret.
    public var savedId: String = ""

    public var isOnline: Bool {
        if case .online = snapshot.state { return true }
        return false
    }

    /// Short status text for the sidebar.
    public var statusText: String {
        switch snapshot.state {
        case .idle: return "Idle"
        case .connecting: return "Connecting…"
        case .online: return snapshot.distro.isEmpty ? "Online" : snapshot.distro
        case .reconnecting(let attempt, _): return "Reconnecting (\(attempt))"
        case .failed(let message, _): return message
        }
    }

    public var statusColor: Color {
        switch snapshot.state {
        case .online: return .green
        case .connecting, .reconnecting: return .orange
        case .failed: return .red
        case .idle: return .secondary
        }
    }
}

/// The bridge between the Rust core and SwiftUI.
///
/// The core runs its own refresh loop on its own threads and publishes a finished snapshot per
/// tick; this polls that snapshot on a display timer. Nothing here parses, schedules, or decides
/// anything — that is all on the Rust side, shared with the other three platforms.
@MainActor
public final class CoreModel: ObservableObject {
    private let core = ServerGlass()

    @Published public private(set) var hosts: [Host] = []
    @Published public var selection: String?
    @Published public var lastError: String?

    /// `nonisolated(unsafe)` so `deinit`, which is not actor-isolated, may cancel it. `Task` is
    /// `Sendable` and cancellation is atomic, so the unsafety is nominal.
    private nonisolated(unsafe) var pollTask: Task<Void, Never>?

    public init() {
        // 2 Hz against an in-process snapshot that is only rebuilt once per tick. Polling faster
        // than the core refreshes would just redraw identical values.
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(500))
                self?.poll()
            }
        }

        restore()
        addDemoHostIfRequested()
    }

    /// Development convenience: `SG_DEMO_HOST=user@host:port` adds and starts a host at launch,
    /// with `SG_DEMO_KEY` as the private key. Used to drive the app against `fixtures/`.
    private func addDemoHostIfRequested() {
        let environment = ProcessInfo.processInfo.environment
        guard let target = environment["SG_DEMO_HOST"] else { return }

        let (user, rest) = target.contains("@")
            ? (String(target.split(separator: "@")[0]), String(target.split(separator: "@")[1]))
            : (NSUserName(), target)
        let parts = rest.split(separator: ":")
        let address = String(parts[0])
        let port = parts.count > 1 ? UInt16(parts[1]) ?? 22 : 22
        let keyPath = environment["SG_DEMO_KEY"]

        addHost(
            address: address,
            port: port,
            user: user,
            authKind: keyPath == nil ? "agent" : "key",
            keyPath: keyPath,
            secret: nil,
            // The fixture containers regenerate their host keys on every build.
            hostKeyPolicy: environment["SG_DEMO_KEY"] == nil ? "strict" : "accept_any",
            refreshMs: 1000,
            persist: false
        )
    }

    deinit {
        pollTask?.cancel()
    }

    /// Register a host and start polling it.
    public func addHost(
        address: String,
        port: UInt16,
        user: String,
        authKind: String,
        keyPath: String?,
        keyText: String? = nil,
        secret: String?,
        hostKeyPolicy: String,
        refreshMs: UInt64,
        /// False for the development demo host, which would otherwise accumulate a duplicate
        /// saved record on every launch.
        persist: Bool = true
    ) {
        let saved = HostStore.SavedHost(
            address: address, port: port, user: user, authKind: authKind,
            keyPath: keyPath?.isEmpty == true ? nil : keyPath,
            hostKeyPolicy: hostKeyPolicy, refreshMs: refreshMs)

        if persist {
            // Secrets go to the Keychain and nowhere else; the saved record never carries them.
            // A refusal is reported here rather than becoming a confusing connection error later.
            if !Keychain.setSecret(secret, for: saved.id)
                || !Keychain.setSecret(keyText, for: saved.id, kind: .keyText)
            {
                lastError = Self.keychainRefused
            }
            var stored = HostStore.load()
            stored.append(saved)
            HostStore.save(stored)
        } else {
            // Not persisted, so the secret has to travel with the config rather than the Keychain.
            ephemeralSecrets[saved.id] = secret
        }

        start(saved)
    }

    /// Change a saved host and reconnect it with the new settings.
    ///
    /// Reconnects rather than editing in place because every field here is a connection parameter:
    /// a new address, port or credential cannot apply to a session already established with the
    /// old ones. The saved record keeps its identifier, so the Keychain entry is *updated* rather
    /// than orphaned — which is what would happen if editing were implemented as remove-then-add.
    ///
    /// `secret` and `keyText` are `nil` when the field was left untouched, which is different from
    /// an empty string meaning "clear it". An edit sheet cannot show an existing password, so
    /// treating a blank field as a deliberate erasure would silently discard the credential of
    /// anyone who edited a port number.
    public func updateHost(
        id: String,
        address: String,
        port: UInt16,
        user: String,
        authKind: String,
        keyPath: String?,
        keyText: String?,
        secret: String?,
        hostKeyPolicy: String,
        refreshMs: UInt64
    ) {
        guard let savedId = hosts.first(where: { $0.id == id })?.savedId,
            let index = HostStore.load().firstIndex(where: { $0.id == savedId })
        else { return }

        var stored = HostStore.load()
        stored[index] = HostStore.SavedHost(
            id: savedId, address: address, port: port, user: user, authKind: authKind,
            keyPath: keyPath?.isEmpty == true ? nil : keyPath,
            hostKeyPolicy: hostKeyPolicy, refreshMs: refreshMs)
        HostStore.save(stored)

        if let secret, !Keychain.setSecret(secret, for: savedId) {
            lastError = Self.keychainRefused
        }
        if let keyText, !Keychain.setSecret(keyText, for: savedId, kind: .keyText) {
            lastError = Self.keychainRefused
        }

        // Drop the live target and bring the record back up. Not `removeHost`, which would also
        // erase the record and its secrets — the very things being kept.
        try? core.removeTarget(targetId: id)
        hosts.removeAll { $0.id == id }
        let wasSelected = selection == id
        if wasSelected { selection = nil }

        let newId = start(stored[index])
        if wasSelected { selection = newId }
    }

    /// The saved record behind a live host, for populating an edit form.
    public func saved(for id: String) -> HostStore.SavedHost? {
        guard let savedId = hosts.first(where: { $0.id == id })?.savedId else { return nil }
        return HostStore.load().first { $0.id == savedId }
    }

    static let keychainRefused =
        "This device's Keychain refused to store the password or key, so this server cannot "
        + "sign in. On an unsigned build — one installed without a developer signature — the "
        + "Keychain is unavailable; use an SSH agent or a key file instead."

    /// Secrets for hosts that were deliberately not saved.
    private var ephemeralSecrets: [String: String?] = [:]

    /// Bring a saved host up: hand its config to the core, start polling, and show it.
    @discardableResult
    private func start(_ saved: HostStore.SavedHost) -> String {
        var config = HostStore.config(for: saved)
        if let ephemeral = ephemeralSecrets[saved.id] {
            config.secret = ephemeral
        }
        let id = core.addTarget(config: config)
        do {
            try core.start(targetId: id)
        } catch {
            lastError = "\(error)"
        }
        if let snapshot = try? core.snapshot(targetId: id) {
            hosts.append(
                Host(
                    id: id, address: "\(saved.user)@\(saved.address)", snapshot: snapshot,
                    savedId: saved.id))
        }
        if selection == nil { selection = id }
        return id
    }

    /// Reconnect everything that was added in a previous session.
    private func restore() {
        for saved in HostStore.load() {
            start(saved)
        }
    }

    public func removeHost(id: String) {
        try? core.removeTarget(targetId: id)

        // Forget the stored record and its Keychain entry too, or removing a host from the list
        // would leave its password behind and the host itself would return on next launch.
        if let savedId = hosts.first(where: { $0.id == id })?.savedId {
            var stored = HostStore.load()
            if let doomed = stored.first(where: { $0.id == savedId }) {
                HostStore.forget(doomed)
            }
            stored.removeAll { $0.id == savedId }
            HostStore.save(stored)
        }

        hosts.removeAll { $0.id == id }
        if selection == id { selection = hosts.first?.id }
    }

    /// Run a command on a host, off the main thread.
    ///
    /// The core call blocks until the host answers; doing that on the main actor would freeze the
    /// UI for the length of the command. Failures come back as output rather than as a thrown
    /// error, because from the reader's point of view "could not run it" and "it printed an
    /// error" belong in the same place — the transcript.
    public func runCommand(hostId: String, command: String) async -> CommandResult {
        let core = core
        return await Task.detached(priority: .userInitiated) {
            do {
                return try core.runCommand(targetId: hostId, command: command)
            } catch {
                return CommandResult(output: "\(error)", exitCode: -1, elapsedMs: 0)
            }
        }.value
    }

    public func host(id: String?) -> Host? {
        guard let id else { return nil }
        return hosts.first { $0.id == id }
    }

    /// Format a value exactly the way the Rust core does, so the four UIs never drift apart.
    public func format(_ gauge: MetricGauge) -> String {
        if gauge.metric == "uptime" {
            return core.formatDuration(seconds: gauge.value)
        }
        return core.format(
            value: gauge.value, unitSuffix: gauge.unitSuffix, binaryScaled: gauge.binaryScaled)
    }

    private func poll() {
        for index in hosts.indices {
            if let snapshot = try? core.snapshot(targetId: hosts[index].id) {
                hosts[index].snapshot = snapshot
            }
        }
    }
}
