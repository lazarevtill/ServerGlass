import AppKit
import SwiftUI

/// Promotes the process to a regular, window-owning application.
///
/// A SwiftPM executable has no bundle and no `Info.plist`, so macOS launches it as an accessory
/// process: it runs, but it owns no windows and never comes to the front. Setting the activation
/// policy explicitly makes `swift run` usable during development. The packaged `.app` produced by
/// `scripts/build-macos.sh` gets this from its `Info.plist` instead, and setting it twice is
/// harmless.
final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApplication.shared.setActivationPolicy(.regular)
        NSApplication.shared.activate(ignoringOtherApps: true)

        // Bring the window forward explicitly. Ordering happens during launch, before the
        // activation policy above takes effect, so without this the window exists but stays
        // behind everything.
        DispatchQueue.main.async {
            for window in NSApplication.shared.windows {
                window.makeKeyAndOrderFront(nil)
            }
            if ProcessInfo.processInfo.environment["SG_DIAGNOSE"] != nil {
                let windows = NSApplication.shared.windows
                FileHandle.standardError.write(
                    "windows=\(windows.count) frames=\(windows.map { $0.frame })\n"
                        .data(using: .utf8)!)
            }
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }
}

@main
struct ServerGlassApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @StateObject private var model = CoreModel()

    var body: some Scene {
        WindowGroup("ServerGlass") {
            ContentView()
                .environmentObject(model)
                // The reference dashboards are dark, and so is every terminal this sits next to.
                .preferredColorScheme(.dark)
        }
        .defaultSize(width: 1000, height: 680)
        .commands {
            CommandGroup(replacing: .newItem) {}
        }
    }
}
