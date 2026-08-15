import ServerGlassUI
import SwiftUI

/// ServerGlass for iOS and iPadOS.
///
/// The entry point is all that differs from the Mac app — every view comes from `ServerGlassUI`,
/// shared verbatim. The detail page measures its own width and reflows, so a phone, a foldable
/// mid-unfold and an iPad in Split View are the same layout at different widths rather than three
/// layouts to keep in sync.
@main
struct ServerGlassApp: App {
    @StateObject private var model = CoreModel()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(model)
                .preferredColorScheme(.dark)
        }
    }
}
