import ServerGlassFFI
import SwiftUI

/// The default screen: what a person who has never heard of SSH needs to know.
///
/// Three things, in descending order of what someone actually wants:
///
/// 1. **Is it OK?** One sentence, in the largest type on the screen, in a colour that answers the
///    question before the words are read.
/// 2. **Three readings** — processor, memory, storage — each with the quantity spelled out and a
///    trend line, because "84%" and "84%, climbing all afternoon" are different facts.
/// 3. **What is using it**, named.
///
/// Everything else — load averages, socket counts, per-core breakdowns — is real, still collected,
/// and one tap away. It is simply not what this screen is for.
///
/// Wording and thresholds come from the Rust core, so every platform says the same thing.
struct SimpleHostView: View {
    @EnvironmentObject private var model: CoreModel
    let host: Host
    @Binding var showTechnical: Bool

    private var snapshot: TargetSnapshot { host.snapshot }

    var body: some View {
        GeometryReader { geometry in
            let wide = geometry.size.width > 640

            ScrollView {
                VStack(alignment: .leading, spacing: wide ? 18 : 14) {
                    HealthCard(health: snapshot.health, name: displayName)

                    if snapshot.simpleTiles.isEmpty {
                        loading
                    } else {
                        // Always one row. An adaptive grid wrapped 3 tiles as 2 + 1, leaving a
                        // hole beside the last one; a fixed three columns with a ring sized to
                        // the available width reads as a set at every size.
                        HStack(alignment: .top, spacing: wide ? 12 : 9) {
                            ForEach(snapshot.simpleTiles, id: \.metric) { tile in
                                SimpleTileCard(tile: tile, ring: wide ? 104 : 78)
                            }
                        }
                    }

                    if !snapshot.topProcesses.isEmpty {
                        busiestSection
                    }

                    technicalToggle
                }
                .padding(wide ? 20 : 15)
                // Capped and centred: on a 27-inch display an uncapped column stretches a
                // three-tile summary across two feet of glass.
                .frame(maxWidth: 940, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
        }
        .background(Theme.background)
    }

    private var displayName: String {
        snapshot.displayName.isEmpty ? host.address : snapshot.displayName
    }

    private var loading: some View {
        HStack(spacing: 10) {
            ProgressView().controlSize(.small)
            Text("Taking the first readings…")
                .font(.system(size: 13))
                .foregroundStyle(Theme.secondary)
        }
        .frame(maxWidth: .infinity, minHeight: 150)
    }

    /// Named for the question rather than the mechanism: nobody asks "what are the top processes by
    /// CPU", they ask what is making the machine busy.
    private var busiestSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("What's keeping it busy")
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(Theme.primary)

            VStack(spacing: 0) {
                let shown = Array(snapshot.topProcesses.prefix(5))
                ForEach(shown, id: \.pid) { process in
                    HStack(spacing: 10) {
                        Text(process.command)
                            .font(.system(size: 13))
                            .foregroundStyle(Theme.primary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        Spacer(minLength: 8)
                        Text(String(format: "%.0f%%", process.cpuPercent))
                            .font(Theme.value(13, weight: .medium))
                            .foregroundStyle(
                                process.cpuPercent >= 50 ? Theme.warn : Theme.secondary)
                    }
                    .padding(.vertical, 9)
                    if process.pid != shown.last?.pid {
                        Divider().overlay(Theme.panelBorder)
                    }
                }
            }
            .padding(.horizontal, 14)
            .background(Theme.card, in: RoundedRectangle(cornerRadius: 14))
            .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(Theme.panelBorder))
        }
    }

    /// The way to everything else.
    ///
    /// It used to be grey secondary text on a grey card, which reads as a footnote rather than a
    /// control — on a phone, where it sits below the fold, that was indistinguishable from the
    /// technical view not existing. Tinted and captioned, it now looks like the door it is.
    private var technicalToggle: some View {
        Button {
            withAnimation(.easeInOut(duration: 0.2)) { showTechnical = true }
        } label: {
            HStack(spacing: 11) {
                Image(systemName: "chart.bar.doc.horizontal")
                    .font(.system(size: 15))
                    .foregroundStyle(Theme.info)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Show every reading")
                        .font(.system(size: 13.5, weight: .medium))
                        .foregroundStyle(Theme.primary)
                    Text("Per-core CPU, network, disks, temperatures")
                        .font(.system(size: 11))
                        .foregroundStyle(Theme.secondary)
                }
                Spacer(minLength: 0)
                Image(systemName: "chevron.right")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(Theme.tertiary)
            }
            .padding(13)
            .frame(maxWidth: .infinity)
            .background(Theme.card, in: RoundedRectangle(cornerRadius: 14))
            .overlay(
                RoundedRectangle(cornerRadius: 14)
                    .strokeBorder(Theme.info.opacity(0.35)))
        }
        .buttonStyle(.plain)
    }
}

/// Colour and icon for a health level, in one place so "problem" always looks like a problem.
enum HealthStyle {
    static func color(_ level: String) -> Color {
        switch level {
        case "ok": return Theme.good
        case "busy": return Theme.warn
        case "problem", "offline": return Theme.bad
        default: return Theme.secondary
        }
    }

    static func icon(_ level: String) -> String {
        switch level {
        case "ok": return "checkmark.circle.fill"
        case "busy": return "exclamationmark.circle.fill"
        case "problem": return "exclamationmark.triangle.fill"
        case "offline": return "wifi.slash"
        default: return "clock.arrow.circlepath"
        }
    }
}

/// The answer to "is my server OK?", as the hero of the screen.
///
/// A tinted gradient rather than a flat wash: the card has to read as *the* answer at a glance
/// from across a room, and a solid block of colour at this size looks like an error banner even
/// when it is green.
struct HealthCard: View {
    let health: HostHealth
    let name: String

    private var tint: Color { HealthStyle.color(health.level) }

    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            Image(systemName: HealthStyle.icon(health.level))
                .font(.system(size: 30, weight: .medium))
                .foregroundStyle(tint)
                .symbolRenderingMode(.hierarchical)

            VStack(alignment: .leading, spacing: 5) {
                Text(health.headline)
                    .font(.system(size: 22, weight: .semibold, design: .rounded))
                    .foregroundStyle(Theme.primary)
                    .fixedSize(horizontal: false, vertical: true)

                if !health.detail.isEmpty {
                    Text(health.detail)
                        .font(.system(size: 13))
                        .foregroundStyle(Theme.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }

                Text(name)
                    .font(Theme.value(11, weight: .regular))
                    .foregroundStyle(Theme.tertiary)
                    .padding(.top, 3)
            }
            Spacer(minLength: 0)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            LinearGradient(
                colors: [tint.opacity(0.16), tint.opacity(0.05)],
                startPoint: .topLeading,
                endPoint: .bottomTrailing),
            in: RoundedRectangle(cornerRadius: 16))
        .overlay(RoundedRectangle(cornerRadius: 16).strokeBorder(tint.opacity(0.28)))
    }
}

/// One reading: a large ring, the number inside it, the quantity beneath, and a trend line.
///
/// The ring is centred and big on purpose. This is a glanceable screen, and a 42-point ring tucked
/// beside a number reads as decoration; at this size it reads as the measurement.
struct SimpleTileCard: View {
    let tile: SimpleTile
    /// Diameter of the ring; the card sizes itself around it.
    var ring: CGFloat = 104

    private var tint: Color { HealthStyle.color(tile.level) }

    var body: some View {
        VStack(spacing: 11) {
            HStack {
                Text(tile.name)
                    .font(.system(size: 12.5, weight: .medium))
                    .foregroundStyle(Theme.secondary)
                Spacer(minLength: 0)
            }

            ZStack {
                let stroke = ring > 90 ? 8.0 : 6.0
                Circle().stroke(Theme.track, lineWidth: stroke)
                if let fraction = tile.fraction {
                    Circle()
                        .trim(from: 0, to: fraction)
                        .stroke(tint, style: StrokeStyle(lineWidth: stroke, lineCap: .round))
                        .rotationEffect(.degrees(-90))
                        .animation(.easeOut(duration: 0.45), value: fraction)
                }
                Text(tile.valueText)
                    .font(Theme.value(ring > 90 ? 21 : 16, weight: .semibold))
                    .foregroundStyle(Theme.primary)
                    .minimumScaleFactor(0.5)
                    .lineLimit(1)
                    .padding(.horizontal, ring > 90 ? 14 : 9)
            }
            .frame(width: ring, height: ring)

            // Two lines are always reserved, whether or not the text needs both. "Barely working"
            // is one line and "240.9 GiB free of 254.2 GiB" is two, and without a reservation the
            // three cards end at three different heights.
            Text(tile.summary)
                .font(.system(size: ring > 90 ? 11.5 : 10))
                .foregroundStyle(Theme.tertiary)
                .multilineTextAlignment(.center)
                .lineLimit(2, reservesSpace: true)
                .frame(maxWidth: .infinity)

            // A number answers "how much"; the trend answers "is this getting worse", which is the
            // question someone glancing at a dashboard is really asking.
            if tile.history.count > 1 {
                Sparkline(values: tile.history, color: tint)
                    .frame(height: 22)
            }
        }
        .padding(ring > 90 ? 14 : 11)
        .frame(maxWidth: .infinity)
        .background(Theme.card, in: RoundedRectangle(cornerRadius: 16))
        .overlay(RoundedRectangle(cornerRadius: 16).strokeBorder(Theme.panelBorder))
    }
}
