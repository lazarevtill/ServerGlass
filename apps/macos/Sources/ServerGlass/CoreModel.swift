import Foundation
import ServerGlassFFI
import SwiftUI

/// A host the user has added, plus its most recent snapshot.
struct Host: Identifiable, Equatable {
    let id: String
    var address: String
    var snapshot: TargetSnapshot

    var isOnline: Bool {
        if case .online = snapshot.state { return true }
        return false
    }

    /// Short status text for the sidebar.
    var statusText: String {
        switch snapshot.state {
        case .idle: return "Idle"
        case .connecting: return "Connecting…"
        case .online: return snapshot.distro.isEmpty ? "Online" : snapshot.distro
        case .reconnecting(let attempt, _): return "Reconnecting (\(attempt))"
        case .failed(let message, _): return message
        }
    }

    var statusColor: Color {
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
final class CoreModel: ObservableObject {
    private let core = ServerGlass()

    @Published private(set) var hosts: [Host] = []
    @Published var selection: String?
    @Published var lastError: String?

    /// `nonisolated(unsafe)` so `deinit`, which is not actor-isolated, may cancel it. `Task` is
    /// `Sendable` and cancellation is atomic, so the unsafety is nominal.
    private nonisolated(unsafe) var pollTask: Task<Void, Never>?

    init() {
        // 2 Hz against an in-process snapshot that is only rebuilt once per tick. Polling faster
        // than the core refreshes would just redraw identical values.
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(500))
                self?.poll()
            }
        }

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
            refreshMs: 1000
        )
    }

    deinit {
        pollTask?.cancel()
    }

    /// Register a host and start polling it.
    func addHost(
        address: String,
        port: UInt16,
        user: String,
        authKind: String,
        keyPath: String?,
        secret: String?,
        hostKeyPolicy: String,
        refreshMs: UInt64
    ) {
        let config = TargetConfig(
            host: address,
            port: port,
            user: user,
            authKind: authKind,
            keyPath: keyPath?.isEmpty == true ? nil : keyPath,
            secret: secret?.isEmpty == true ? nil : secret,
            hostKeyPolicy: hostKeyPolicy,
            refreshMs: refreshMs
        )

        let id = core.addTarget(config: config)
        do {
            try core.start(targetId: id)
        } catch {
            lastError = "\(error)"
        }

        if let snapshot = try? core.snapshot(targetId: id) {
            hosts.append(Host(id: id, address: "\(user)@\(address)", snapshot: snapshot))
        }
        if selection == nil { selection = id }
    }

    func removeHost(id: String) {
        try? core.removeTarget(targetId: id)
        hosts.removeAll { $0.id == id }
        if selection == id { selection = hosts.first?.id }
    }

    func host(id: String?) -> Host? {
        guard let id else { return nil }
        return hosts.first { $0.id == id }
    }

    /// Format a value exactly the way the Rust core does, so the four UIs never drift apart.
    func format(_ gauge: MetricGauge) -> String {
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
