import ServerGlassFFI
import Testing

/// Exercise the real boundary used during restore, not just the Swift configuration record.
struct CoreBridgeTests {
    @Test("saved configurations cross the native boundary without trapping")
    func restoresConfigurations() throws {
        let core = ServerGlass()
        for keyText: String? in [nil, "test key with Unicode: λ"] {
            let config = TargetConfig(
                host: "example.invalid", port: 2222, user: "test", authKind: "key_text",
                keyPath: nil, keyText: keyText, secret: "test passphrase",
                hostKeyPolicy: "strict", knownHostsPath: "/tmp/serverglass-bridge-known-hosts",
                refreshMs: 1500)
            let id = core.addTarget(config: config)
            let snapshot = try core.snapshot(targetId: id)
            #expect(snapshot.displayName == config.host)
            #expect(snapshot.state == .idle)
            try core.removeTarget(targetId: id)
        }
        #expect(core.targetIds().isEmpty)
    }
}
