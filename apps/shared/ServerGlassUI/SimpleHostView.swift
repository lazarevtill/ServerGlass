import ServerGlassFFI
import SwiftUI

/// The default screen: what a person who has never heard of SSH needs to know.
///
/// One sentence about whether the server is fine, four readings that mean something without
/// training, and the names of whatever is working hardest. Everything else — load averages, socket
/// counts, per-core breakdowns, I/O wait — is real and still one tap away, but it is not what this
/// screen is for.
///
/// The wording and the thresholds come from the Rust core, so the Mac, the iPhone, the iPad and
/// eventually Windows, Linux and Android all say the same thing in the same words.
struct SimpleHostView: View {
    @EnvironmentObject private var model: CoreModel
    let host: Host
    @Binding var showTechnical: Bool

    private var snapshot: TargetSnapshot { host.snapshot }

    var body: some View {
        GeometryReader { geometry in
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    HealthCard(health: snapshot.health, name: displayName)

                    if !snapshot.simpleTiles.isEmpty {
                        LazyVGrid(
                            columns: [GridItem(.adaptive(minimum: 152, maximum: 260), spacing: 12)],
                            spacing: 12
                        ) {
                            ForEach(snapshot.simpleTiles, id: \.metric) { tile in
                                SimpleTileCard(tile: tile)
                            }
                        }
                    }

                    if !snapshot.topProcesses.isEmpty {
                        busiestSection
                    }

                    technicalToggle
                }
                .padding(geometry.size.width > 700 ? 20 : 14)
                .frame(maxWidth: 900, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
        }
        .background(Theme.background)
    }

    private var displayName: String {
        snapshot.displayName.isEmpty ? host.address : snapshot.displayName
    }

    /// Named for the question rather than the mechanism: nobody asks "what are the top processes
    /// by CPU", they ask what is making the machine busy.
    private var busiestSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("What's keeping it busy")
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(Theme.primary)

            VStack(spacing: 0) {
                ForEach(snapshot.topProcesses.prefix(5), id: \.pid) { process in
                    HStack(spacing: 10) {
                        Text(process.command)
                            .font(.system(size: 13))
                            .foregroundStyle(Theme.primary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        Spacer(minLength: 8)
                        Text(String(format: "%.0f%%", process.cpuPercent))
                            .font(Theme.value(13, weight: .medium))
                            .foregroundStyle(Theme.secondary)
                    }
                    .padding(.vertical, 7)
                    if process.pid != snapshot.topProcesses.prefix(5).last?.pid {
                        Divider().overlay(Theme.panelBorder)
                    }
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 2)
            .background(Theme.panel, in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Theme.panelBorder))

            Text("These are the programs using the most processing power right now.")
                .font(.system(size: 11))
                .foregroundStyle(Theme.tertiary)
        }
    }

    private var technicalToggle: some View {
        Button {
            withAnimation { showTechnical = true }
        } label: {
            HStack(spacing: 6) {
                Image(systemName: "chart.bar.doc.horizontal")
                Text("Show technical details")
                Spacer(minLength: 0)
                Image(systemName: "chevron.right").font(.system(size: 10))
            }
            .font(.system(size: 13))
            .foregroundStyle(Theme.secondary)
            .padding(12)
            .frame(maxWidth: .infinity)
            .background(Theme.panel, in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Theme.panelBorder))
        }
        .buttonStyle(.plain)
    }
}

/// Colour and icon for a health level, kept in one place so "problem" always looks like a problem.
enum HealthStyle {
    static func color(_ level: String) -> Color {
        switch level {
        case "ok": return Theme.good
        case "busy": return Theme.warn
        case "problem": return Theme.bad
        case "offline": return Theme.bad
        default: return Theme.secondary
        }
    }

    static func icon(_ level: String) -> String {
        switch level {
        case "ok": return "checkmark.circle.fill"
        case "busy": return "exclamationmark.circle.fill"
        case "problem": return "exclamationmark.triangle.fill"
        case "offline": return "wifi.slash"
        default: return "clock.fill"
        }
    }
}

/// The answer to "is my server OK?", in the largest type on the screen.
struct HealthCard: View {
    let health: HostHealth
    let name: String

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: HealthStyle.icon(health.level))
                .font(.system(size: 26))
                .foregroundStyle(HealthStyle.color(health.level))

            VStack(alignment: .leading, spacing: 4) {
                Text(health.headline)
                    .font(.system(size: 19, weight: .semibold))
                    .foregroundStyle(Theme.primary)
                if !health.detail.isEmpty {
                    Text(health.detail)
                        .font(.system(size: 13))
                        .foregroundStyle(Theme.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Text(name)
                    .font(.system(size: 11))
                    .foregroundStyle(Theme.tertiary)
                    .padding(.top, 2)
            }
            Spacer(minLength: 0)
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            HealthStyle.color(health.level).opacity(0.10),
            in: RoundedRectangle(cornerRadius: 14))
        .overlay(
            RoundedRectangle(cornerRadius: 14)
                .strokeBorder(HealthStyle.color(health.level).opacity(0.30)))
    }
}

/// One reading, named and explained. Deliberately large — this is a glanceable screen.
struct SimpleTileCard: View {
    let tile: SimpleTile

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(tile.name)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(Theme.secondary)

            HStack(alignment: .center, spacing: 10) {
                if let fraction = tile.fraction {
                    ZStack {
                        Circle().stroke(Theme.track, lineWidth: 5)
                        Circle()
                            .trim(from: 0, to: fraction)
                            .stroke(
                                HealthStyle.color(tile.level),
                                style: StrokeStyle(lineWidth: 5, lineCap: .round))
                            .rotationEffect(.degrees(-90))
                            .animation(.easeOut(duration: 0.4), value: fraction)
                    }
                    .frame(width: 42, height: 42)
                }

                VStack(alignment: .leading, spacing: 1) {
                    Text(tile.valueText)
                        .font(Theme.value(20, weight: .semibold))
                        .foregroundStyle(Theme.primary)
                        .lineLimit(1)
                        .minimumScaleFactor(0.6)
                }
                Spacer(minLength: 0)
            }

            if !tile.summary.isEmpty {
                Text(tile.summary)
                    .font(.system(size: 11))
                    .foregroundStyle(Theme.tertiary)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(13)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.panel, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Theme.panelBorder))
    }
}
