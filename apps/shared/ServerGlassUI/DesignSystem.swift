import ServerGlassFFI
import SwiftUI

/// The visual language.
///
/// The guiding rule is that **the widget must match the metric**. A ring implies a proportion of
/// something; drawing one for "context switches per second" or "TCP orphaned: 0" tells the reader
/// nothing and, worse, implies a fullness that does not exist. So:
///
/// | Metric shape                  | Widget                                  |
/// |-------------------------------|-----------------------------------------|
/// | Percentage (has a maximum)    | ring gauge                              |
/// | Capacity (used of total)      | horizontal bar with used / total text   |
/// | Rate (bytes/s, ops/s)         | large monospaced number + sparkline     |
/// | Count / state                 | plain label-value row                   |
///
/// Density comes from small type and tight spacing, not from cramming identical widgets together.
enum Theme {
    static let background = Color(red: 0.043, green: 0.043, blue: 0.051)
    static let panel = Color(red: 0.082, green: 0.082, blue: 0.094)
    /// Slightly lifted from `panel`, for the simple view's larger cards. A single flat surface
    /// colour across every size makes big cards look like empty space.
    static let card = Color(red: 0.098, green: 0.098, blue: 0.114)
    static let panelBorder = Color.white.opacity(0.06)
    static let inset = Color.black.opacity(0.25)
    static let track = Color.white.opacity(0.08)

    static let primary = Color.white.opacity(0.92)
    static let secondary = Color.white.opacity(0.45)
    static let tertiary = Color.white.opacity(0.28)

    static let good = Color(red: 0.35, green: 0.84, blue: 0.55)
    static let warn = Color(red: 0.98, green: 0.75, blue: 0.28)
    static let bad = Color(red: 0.97, green: 0.44, blue: 0.44)
    static let info = Color(red: 0.42, green: 0.66, blue: 0.97)

    /// Green below 60 %, amber to 85 %, red above — applied only where a fraction is real.
    /// Colour for a level the core assigned — to a host's health, a reading, or a process.
    ///
    /// The thresholds behind these levels used to live here *and* in Compose, and had already
    /// drifted apart. Deciding what counts as "busy" is the core's job; this maps its answer.
    static func level(_ level: String) -> Color {
        switch level {
        case "ok": return good
        case "busy": return warn
        case "problem", "offline": return bad
        case "none": return info
        default: return secondary
        }
    }

    // Numbers are monospaced everywhere so columns line up and a changing value does not make the
    // layout twitch on every refresh.
    static func value(_ size: CGFloat, weight: Font.Weight = .medium) -> Font {
        .system(size: size, weight: weight, design: .rounded).monospacedDigit()
    }

    static func label(_ size: CGFloat = 9) -> Font {
        .system(size: size, weight: .medium)
    }
}

extension MetricGauge {
    /// Position within the metric's range — `nil` when it has no meaningful maximum.
    var fraction: Double? {
        guard let max, max > 0 else { return nil }
        return Swift.min(Swift.max(value / max, 0), 1)
    }

    /// The colour for this reading, from the level the core assigned it.
    ///
    /// The thresholds used to live here and in Compose, and they had already drifted apart. A view
    /// layer maps a level onto a colour; deciding what counts as "busy" is the core's job.
    var color: Color { Theme.level(severity) }
}

extension TargetSnapshot {
    /// Headline and grouped metrics together, for lookup by name.
    var allGauges: [MetricGauge] { gauges + detailGroups.flatMap(\.gauges) }

    func gauge(_ metric: String) -> MetricGauge? {
        allGauges.first { $0.metric == metric }
    }

    func entities(ofKind kind: String) -> [EntityView] {
        entities.filter { $0.kind == kind }
    }

    func group(_ title: String) -> DetailGroup? {
        detailGroups.first { $0.title == title }
    }
}

extension EntityView {
    func gauge(_ metric: String) -> MetricGauge? {
        gauges.first { $0.metric == metric }
    }

    /// Combined rate across two directions, for ranking. Both values are already per-second.
    func throughput(_ first: String, _ second: String) -> Double {
        (gauge(first)?.value ?? 0) + (gauge(second)?.value ?? 0)
    }
}

/// A titled section. Everything on the detail page lives in one of these, so the page reads as a
/// sequence of answers rather than an undifferentiated field of widgets.
struct Panel<Content: View>: View {
    let title: String
    var subtitle: String?
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(title.uppercased())
                    .font(Theme.label(9.5))
                    .tracking(0.8)
                    .foregroundStyle(Theme.secondary)
                if let subtitle {
                    Text(subtitle)
                        .font(Theme.value(9.5, weight: .regular))
                        .foregroundStyle(Theme.tertiary)
                }
                Spacer(minLength: 0)
            }
            content
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.panel, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Theme.panelBorder))
    }
}
