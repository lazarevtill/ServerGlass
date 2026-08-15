import ServerGlassFFI
import SwiftUI

// MARK: - Percentages

/// A ring, used **only** for metrics with a real maximum.
struct RingGauge: View {
    let fraction: Double
    let color: Color
    var lineWidth: CGFloat = 6
    var caption: String
    var sub: String?

    var body: some View {
        ZStack {
            Circle().stroke(Theme.track, lineWidth: lineWidth)
            Circle()
                .trim(from: 0, to: fraction)
                .stroke(color, style: StrokeStyle(lineWidth: lineWidth, lineCap: .round))
                .rotationEffect(.degrees(-90))
                .animation(.easeOut(duration: 0.4), value: fraction)

            VStack(spacing: 1) {
                Text(caption)
                    .font(Theme.value(15, weight: .semibold))
                    .foregroundStyle(Theme.primary)
                if let sub {
                    Text(sub)
                        .font(Theme.value(8.5, weight: .regular))
                        .foregroundStyle(Theme.tertiary)
                }
            }
            .minimumScaleFactor(0.6)
            .lineLimit(1)
            .padding(.horizontal, lineWidth + 4)
        }
    }
}

/// The four headline percentages, each with the underlying quantity spelled out beneath it.
/// "79.2 %" alone is a number; "79.2 % · 49.4 / 62.4 GiB" is an answer.
struct HeadlineRing: View {
    let gauge: MetricGauge
    let caption: String
    let detail: String?

    var body: some View {
        VStack(spacing: 7) {
            RingGauge(
                fraction: gauge.fraction ?? 0,
                color: gauge.color,
                lineWidth: 6,
                caption: caption,
                sub: nil
            )
            .frame(width: 76, height: 76)

            VStack(spacing: 1) {
                Text(gauge.label)
                    .font(Theme.label(10))
                    .foregroundStyle(Theme.primary)
                if let detail {
                    Text(detail)
                        .font(Theme.value(9, weight: .regular))
                        .foregroundStyle(Theme.secondary)
                        .lineLimit(1)
                        .minimumScaleFactor(0.7)
                }
            }
        }
        .frame(maxWidth: .infinity)
    }
}

// MARK: - Capacities

/// Used-of-total, as a bar. Filesystems and memory are capacities, not proportions of an abstract
/// whole, and a bar shows how much room is left far better than a ring does.
struct CapacityBar: View {
    let fraction: Double
    let color: Color
    var height: CGFloat = 6

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                Capsule().fill(Theme.track)
                Capsule()
                    .fill(color)
                    .frame(width: max(2, geometry.size.width * fraction))
                    .animation(.easeOut(duration: 0.4), value: fraction)
            }
        }
        .frame(height: height)
    }
}

/// One capacity line: name on the left, used/total on the right, bar underneath.
struct CapacityRow: View {
    let name: String
    let usage: MetricGauge
    let used: MetricGauge?
    let total: MetricGauge?
    let format: (MetricGauge) -> String

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(name)
                    .font(Theme.value(11, weight: .medium))
                    .foregroundStyle(Theme.primary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer(minLength: 8)
                if let used, let total {
                    Text("\(format(used)) / \(format(total))")
                        .font(Theme.value(10, weight: .regular))
                        .foregroundStyle(Theme.secondary)
                }
                Text(format(usage))
                    .font(Theme.value(10, weight: .semibold))
                    .foregroundStyle(usage.color)
                    .frame(width: 46, alignment: .trailing)
            }
            CapacityBar(fraction: usage.fraction ?? 0, color: usage.color)
        }
    }
}

// MARK: - Rates

/// A rate or a raw quantity: a large monospaced number with its own sparkline. No ring, because
/// there is no maximum for it to be a fraction of.
struct StatCell: View {
    let label: String
    let value: String
    var color: Color = Theme.primary
    var history: [Double] = []
    var icon: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 3) {
                if let icon {
                    Image(systemName: icon).font(.system(size: 8)).foregroundStyle(color)
                }
                Text(label.uppercased())
                    .font(Theme.label(8.5))
                    .tracking(0.5)
                    .foregroundStyle(Theme.secondary)
                    .lineLimit(1)
            }
            Text(value)
                .font(Theme.value(13, weight: .semibold))
                .foregroundStyle(color)
                .lineLimit(1)
                .minimumScaleFactor(0.6)

            if history.count > 1 {
                Sparkline(values: history, color: color).frame(height: 14)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// Scaled to the observed range, not to zero: a byte rate hovering between 4.0 and 4.2 MiB/s draws
/// as a flat line against a zero baseline and tells the reader nothing.
///
/// But range-scaling alone lies in the opposite direction. Storage sitting at 5.19% and ticking to
/// 5.20% has a span of 0.01, which stretched to full height draws a cliff — the chart screams that
/// the disk just filled up. So the span is floored at a fraction of the magnitude: genuinely flat
/// series draw flat, and only real movement gets amplified.
struct Sparkline: View {
    let values: [Double]
    let color: Color
    /// Minimum span as a fraction of the largest value. 5% keeps noise flat without flattening
    /// changes a person would care about.
    var noiseFloor: Double = 0.05

    var body: some View {
        GeometryReader { geometry in
            let lowest = values.min() ?? 0
            let highest = values.max() ?? 1
            let magnitude = Swift.max(abs(highest), abs(lowest))
            let span = Swift.max(highest - lowest, magnitude * noiseFloor)
            let width = geometry.size.width
            let height = geometry.size.height

            let points: [CGPoint] = values.enumerated().map { index, value in
                let x = width * Double(index) / Double(Swift.max(values.count - 1, 1))
                let normalised = span > 0 ? (value - lowest) / span : 0.5
                return CGPoint(x: x, y: height - normalised * height)
            }

            ZStack {
                // A faint fill under the line gives the eye a shape to read at a glance, which a
                // 1px stroke at this size does not.
                Path { path in
                    guard let first = points.first, let last = points.last else { return }
                    path.move(to: CGPoint(x: first.x, y: height))
                    path.addLine(to: first)
                    for point in points.dropFirst() { path.addLine(to: point) }
                    path.addLine(to: CGPoint(x: last.x, y: height))
                    path.closeSubpath()
                }
                .fill(color.opacity(0.14))

                Path { path in
                    guard let first = points.first else { return }
                    path.move(to: first)
                    for point in points.dropFirst() { path.addLine(to: point) }
                }
                .stroke(color.opacity(0.9), style: StrokeStyle(lineWidth: 1.3, lineJoin: .round))
            }
        }
    }
}

// MARK: - Counts

/// A label-value line. Socket counts and segment rates are numbers to be read, not gauges to be
/// interpreted, and twenty of them belong in a compact grid.
struct KeyValueRow: View {
    let label: String
    let value: String
    var emphasis: Color = Theme.primary

    var body: some View {
        HStack(spacing: 6) {
            Text(label)
                .font(Theme.label(10))
                .foregroundStyle(Theme.secondary)
                .lineLimit(1)
            Spacer(minLength: 4)
            Text(value)
                .font(Theme.value(10.5, weight: .medium))
                .foregroundStyle(emphasis)
        }
    }
}

// MARK: - Per-core CPU

/// One logical CPU as a thin bar. Twenty of these read at a glance — twenty rings do not, and
/// twenty rings is what a 20-core Proxmox host produced before this existed.
struct CoreBar: View {
    let index: String
    let percent: Double

    var body: some View {
        HStack(spacing: 5) {
            Text(index)
                .font(Theme.value(8.5, weight: .regular))
                .foregroundStyle(Theme.tertiary)
                .frame(width: 16, alignment: .trailing)
            CapacityBar(fraction: percent / 100, color: Theme.severity(percent / 100), height: 5)
            Text("\(Int(percent.rounded()))")
                .font(Theme.value(8.5, weight: .regular))
                .foregroundStyle(Theme.secondary)
                .frame(width: 18, alignment: .trailing)
        }
    }
}

/// A stacked breakdown bar — user / system / iowait / steal as proportions of one whole.
struct StackedBar: View {
    /// `(label, percent, colour)`, summing to at most 100.
    let segments: [(String, Double, Color)]

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            GeometryReader { geometry in
                HStack(spacing: 1) {
                    ForEach(Array(segments.enumerated()), id: \.offset) { _, segment in
                        Rectangle()
                            .fill(segment.2)
                            .frame(width: geometry.size.width * (segment.1 / 100))
                    }
                    Rectangle().fill(Theme.track)
                }
                .clipShape(Capsule())
            }
            .frame(height: 8)

            HStack(spacing: 12) {
                ForEach(Array(segments.enumerated()), id: \.offset) { _, segment in
                    HStack(spacing: 4) {
                        Circle().fill(segment.2).frame(width: 5, height: 5)
                        Text(segment.0)
                            .font(Theme.label(9))
                            .foregroundStyle(Theme.secondary)
                        Text(String(format: "%.1f%%", segment.1))
                            .font(Theme.value(9, weight: .medium))
                            .foregroundStyle(Theme.primary)
                    }
                }
                Spacer(minLength: 0)
            }
        }
    }
}
