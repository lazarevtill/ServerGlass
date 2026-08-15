import ServerGlassFFI
import SwiftUI

/// One host, as a sequence of answers.
///
/// The page is ordered by how people actually triage a server: is it busy, is it out of memory, is
/// it out of disk, what is the network doing, then the detail you only want when something looks
/// wrong. Each section chooses the widget that fits its metric rather than reusing one everywhere.
struct HostDetailView: View {
    @EnvironmentObject private var model: CoreModel
    let host: Host

    private var snapshot: TargetSnapshot { host.snapshot }

    /// Below this width the two-column panels stack.
    ///
    /// Driven by measured width rather than by a size class, because the case that matters most is
    /// a device whose width changes while the app is running — an unfolding phone, a resized
    /// window, an iPad entering Split View. A `GeometryReader` re-evaluates on every one of those;
    /// a size class does not always change, and stored state would go stale.
    private static let wideThreshold: CGFloat = 680

    var body: some View {
        GeometryReader { geometry in
            content(wide: geometry.size.width >= Self.wideThreshold)
        }
        .background(Theme.background)
    }

    /// Two panels side by side when there is room, stacked when there is not.
    @ViewBuilder
    private func pair<A: View, B: View>(
        _ wide: Bool,
        @ViewBuilder _ first: () -> A,
        @ViewBuilder _ second: () -> B
    ) -> some View {
        if wide {
            HStack(alignment: .top, spacing: 12) {
                first()
                second()
            }
        } else {
            VStack(spacing: 12) {
                first()
                second()
            }
        }
    }

    private func content(wide: Bool) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                HostHeader(host: host)

                if case .failed(let message, let recoverable) = snapshot.state {
                    Banner(
                        text: message,
                        detail: recoverable
                            ? "ServerGlass will keep retrying."
                            : "This will not resolve on its own.",
                        color: Theme.bad)
                }
                if !snapshot.sourceErrors.isEmpty {
                    Banner(
                        text: "\(snapshot.sourceErrors.count) collector(s) reported a problem",
                        detail: snapshot.sourceErrors.joined(separator: "\n"),
                        color: Theme.warn)
                }

                if snapshot.gauges.isEmpty {
                    ProgressView("Collecting…")
                        .controlSize(.small)
                        .frame(maxWidth: .infinity, minHeight: 220)
                } else {
                    overview
                    pair(wide) { cpuPanel } _: { memoryPanel }
                    pair(wide) { networkPanel } _: { diskPanel }
                    processPanel
                    filesystemPanel
                    socketsPanel
                }
            }
            .padding(14)
        }
    }

    // MARK: Overview — the four questions asked first

    private var overview: some View {
        Panel(title: "Overview") {
            // A grid, not a row: six rings do not fit across a phone, and on a foldable the same
            // view has to work at both widths without a second layout existing.
            LazyVGrid(
                columns: [GridItem(.adaptive(minimum: 92, maximum: 170), spacing: 4)],
                spacing: 12
            ) {
                if let cpu = snapshot.gauge("cpu_usage") {
                    HeadlineRing(
                        gauge: cpu,
                        caption: model.format(cpu),
                        detail: "\(snapshot.cpuCount) cores")
                }
                if let memory = snapshot.gauge("mem_usage") {
                    HeadlineRing(
                        gauge: memory,
                        caption: model.format(memory),
                        detail: pair("mem_used", "mem_total"))
                }
                if let disk = snapshot.gauge("disk_usage") {
                    HeadlineRing(gauge: disk, caption: model.format(disk), detail: "root")
                }
                if let swap = snapshot.gauge("swap_usage") {
                    HeadlineRing(
                        gauge: swap,
                        caption: model.format(swap),
                        detail: pair("swap_used", "swap_total"))
                }
                if let load = snapshot.gauge("load1") {
                    HeadlineRing(
                        gauge: load,
                        caption: String(format: "%.2f", load.value),
                        detail: loadDetail)
                }
                if let uptime = snapshot.gauge("uptime") {
                    VStack(spacing: 7) {
                        VStack(spacing: 1) {
                            Text(model.format(uptime))
                                .font(Theme.value(15, weight: .semibold))
                                .foregroundStyle(Theme.primary)
                                .lineLimit(1)
                                .minimumScaleFactor(0.6)
                        }
                        .frame(width: 76, height: 76)
                        Text("Uptime")
                            .font(Theme.label(10))
                            .foregroundStyle(Theme.primary)
                    }
                    .frame(maxWidth: .infinity)
                }
            }
        }
    }

    /// "6 of 23" when the list is capped, "4 devices" when it is not. Silent truncation reads as
    /// "this is everything", which on a Proxmox host with two dozen block devices is a lie.
    static func shown(_ count: Int, cap: Int) -> String {
        count > cap ? "\(cap) of \(count)" : "\(count)"
    }

    private var loadDetail: String? {
        guard let five = snapshot.gauge("load5"), let fifteen = snapshot.gauge("load15") else {
            return nil
        }
        return String(format: "%.2f · %.2f", five.value, fifteen.value)
    }

    /// `used / total`, e.g. `49.4 GiB / 62.4 GiB`.
    private func pair(_ used: String, _ total: String) -> String? {
        guard let used = snapshot.gauge(used), let total = snapshot.gauge(total) else { return nil }
        return "\(model.format(used)) / \(model.format(total))"
    }

    // MARK: CPU

    private var cpuPanel: some View {
        let cores = snapshot.entities(ofKind: "cpu").sorted {
            (Int($0.display) ?? 0) < (Int($1.display) ?? 0)
        }

        return Panel(title: "CPU", subtitle: "\(snapshot.cpuCount) logical") {
            VStack(alignment: .leading, spacing: 12) {
                StackedBar(segments: [
                    ("User", snapshot.gauge("cpu_user")?.value ?? 0, Theme.info),
                    ("System", snapshot.gauge("cpu_system")?.value ?? 0, Theme.warn),
                    ("I/O wait", snapshot.gauge("cpu_iowait")?.value ?? 0, Theme.bad),
                    ("Steal", snapshot.gauge("cpu_steal")?.value ?? 0, Color.purple),
                ])

                if !cores.isEmpty {
                    LazyVGrid(
                        columns: [GridItem(.adaptive(minimum: 108, maximum: 200), spacing: 10)],
                        spacing: 6
                    ) {
                        ForEach(cores, id: \.id) { core in
                            CoreBar(index: core.display, percent: core.gauge("usage")?.value ?? 0)
                        }
                    }
                }

                HStack(spacing: 14) {
                    ForEach(["procs_running", "procs_blocked", "ctx_switches"], id: \.self) { metric in
                        if let gauge = snapshot.gauge(metric) {
                            StatCell(
                                label: gauge.label,
                                value: model.format(gauge),
                                color: Theme.primary,
                                history: gauge.history)
                        }
                    }
                }
            }
        }
    }

    // MARK: Memory

    private var memoryPanel: some View {
        Panel(title: "Memory") {
            VStack(alignment: .leading, spacing: 12) {
                if let usage = snapshot.gauge("mem_usage") {
                    CapacityRow(
                        name: "Physical",
                        usage: usage,
                        used: snapshot.gauge("mem_used"),
                        total: snapshot.gauge("mem_total"),
                        format: model.format)
                }
                if let swap = snapshot.gauge("swap_usage") {
                    CapacityRow(
                        name: "Swap",
                        usage: swap,
                        used: snapshot.gauge("swap_used"),
                        total: snapshot.gauge("swap_total"),
                        format: model.format)
                }

                // The breakdown is deliberately a plain list of quantities: on a host running ZFS
                // these do not sum to the total (ARC is neither free nor counted as cached), so
                // rendering them as a stacked bar would draw a picture that is simply untrue.
                LazyVGrid(
                    columns: [GridItem(.adaptive(minimum: 118), spacing: 12)], spacing: 5
                ) {
                    ForEach(["mem_available", "mem_free", "mem_cached", "mem_buffers"], id: \.self) {
                        metric in
                        if let gauge = snapshot.gauge(metric) {
                            KeyValueRow(label: gauge.label, value: model.format(gauge))
                        }
                    }
                }
            }
        }
    }

    // MARK: Network

    private var networkPanel: some View {
        let interfaces = snapshot.entities(ofKind: "net")
            .filter { ($0.gauge("rx_bytes")?.value ?? 0) > 0 || ($0.gauge("tx_bytes")?.value ?? 0) > 0 }
            // Rank by combined traffic. Sorting on receive alone buries a send-heavy uplink
            // below idle interfaces and then truncation hides it entirely.
            .sorted { $0.throughput("rx_bytes", "tx_bytes") > $1.throughput("rx_bytes", "tx_bytes") }

        return Panel(title: "Network", subtitle: Self.shown(interfaces.count, cap: 6)) {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 14) {
                    if let rx = snapshot.gauge("net_rx") {
                        StatCell(
                            label: "Download", value: model.format(rx),
                            color: Theme.good, history: rx.history, icon: "arrow.down")
                    }
                    if let tx = snapshot.gauge("net_tx") {
                        StatCell(
                            label: "Upload", value: model.format(tx),
                            color: Theme.info, history: tx.history, icon: "arrow.up")
                    }
                }

                if !interfaces.isEmpty {
                    Divider().overlay(Theme.panelBorder)
                    ForEach(interfaces.prefix(6), id: \.id) { interface in
                        InterfaceRow(entity: interface, format: model.format)
                    }
                }
            }
        }
    }

    // MARK: Disk I/O

    private var diskPanel: some View {
        let disks = snapshot.entities(ofKind: "disk")
            // Combined, for the same reason: reads served from ZFS ARC leave a write-heavy pool
            // reading zero, which would sort the busiest device on the box last and then cut it.
            .sorted { $0.throughput("read_bytes", "write_bytes") > $1.throughput("read_bytes", "write_bytes") }

        return Panel(title: "Disk I/O", subtitle: Self.shown(disks.count, cap: 6)) {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 14) {
                    if let read = snapshot.gauge("disk_read") {
                        StatCell(
                            label: "Read", value: model.format(read),
                            color: Theme.good, history: read.history, icon: "arrow.down")
                    }
                    if let write = snapshot.gauge("disk_write") {
                        StatCell(
                            label: "Write", value: model.format(write),
                            color: Theme.warn, history: write.history, icon: "arrow.up")
                    }
                }

                if !disks.isEmpty {
                    Divider().overlay(Theme.panelBorder)
                    ForEach(disks.prefix(6), id: \.id) { disk in
                        DeviceRow(entity: disk, format: model.format)
                    }
                }
            }
        }
    }

    // MARK: Processes

    /// What is actually using the machine.
    ///
    /// "CPU 79 %" only raises a question; this is where it gets answered. Placed directly under the
    /// CPU and memory panels for that reason, and rendered as a table because a process list is
    /// something you read down, not something you gauge.
    private var processPanel: some View {
        Group {
            if !snapshot.topProcesses.isEmpty {
                Panel(title: "Top processes", subtitle: "by CPU") {
                    VStack(spacing: 0) {
                        ProcessHeaderRow()
                        ForEach(snapshot.topProcesses, id: \.pid) { process in
                            ProcessRow(
                                process: process,
                                cores: Double(snapshot.cpuCount),
                                format: model.format)
                        }
                    }
                }
            }
        }
    }

    // MARK: Filesystems

    private var filesystemPanel: some View {
        let mounts = snapshot.entities(ofKind: "fs")
            .sorted { ($0.gauge("usage")?.value ?? 0) > ($1.gauge("usage")?.value ?? 0) }

        return Group {
            if !mounts.isEmpty {
                Panel(title: "Filesystems", subtitle: "\(mounts.count) mounted") {
                    LazyVGrid(
                        columns: [GridItem(.adaptive(minimum: 300, maximum: 560), spacing: 18)],
                        spacing: 10
                    ) {
                        ForEach(mounts, id: \.id) { mount in
                            if let usage = mount.gauge("usage") {
                                CapacityRow(
                                    name: mount.display,
                                    usage: usage,
                                    used: mount.gauge("used"),
                                    total: mount.gauge("total"),
                                    format: model.format)
                            }
                        }
                    }
                }
            }
        }
    }

    // MARK: Sockets

    private var socketsPanel: some View {
        Group {
            if let group = snapshot.group("Sockets & TCP"), !group.gauges.isEmpty {
                Panel(title: group.title) {
                    // Twenty socket counters are numbers to read, not gauges to interpret.
                    LazyVGrid(
                        columns: [GridItem(.adaptive(minimum: 150), spacing: 18)], spacing: 5
                    ) {
                        ForEach(group.gauges, id: \.seriesId) { gauge in
                            KeyValueRow(
                                label: gauge.label,
                                value: model.format(gauge),
                                emphasis: gauge.metric == "tcp_retrans" && gauge.value > 0
                                    ? Theme.warn : Theme.primary)
                        }
                    }
                }
            }
        }
    }
}

struct ProcessHeaderRow: View {
    var body: some View {
        HStack(spacing: 10) {
            Text("PID").frame(width: 52, alignment: .trailing)
            Text("COMMAND").frame(maxWidth: .infinity, alignment: .leading)
            Text("CPU").frame(width: 100, alignment: .trailing)
            Text("MEMORY").frame(width: 72, alignment: .trailing)
        }
        .font(Theme.label(8.5))
        .tracking(0.5)
        .foregroundStyle(Theme.tertiary)
        .padding(.bottom, 5)
    }
}

struct ProcessRow: View {
    let process: ProcessView
    /// Used to scale the inline bar: on a 20-core host a process at 400 % is busy, not impossible.
    let cores: Double
    let format: (MetricGauge) -> String

    /// Share of the whole machine, which is what the bar should represent — 100 % of one core on a
    /// 20-core box is 5 % of the host, and drawing it as a full bar would be alarming nonsense.
    private var machineFraction: Double {
        let total = Swift.max(cores, 1) * 100
        return Swift.min(Swift.max(process.cpuPercent / total, 0), 1)
    }

    var body: some View {
        HStack(spacing: 10) {
            Text(process.pid)
                .font(Theme.value(10, weight: .regular))
                .foregroundStyle(Theme.tertiary)
                .frame(width: 52, alignment: .trailing)

            HStack(spacing: 5) {
                Text(process.command)
                    .font(Theme.value(10.5, weight: .medium))
                    .foregroundStyle(Theme.primary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                // Uninterruptible sleep and zombies are worth flagging; sleeping and running are
                // the normal states and adding a badge for them would be pure noise.
                if process.state == "D" || process.state == "Z" {
                    Text(process.state)
                        .font(Theme.label(8))
                        .foregroundStyle(Theme.warn)
                        .padding(.horizontal, 3)
                        .padding(.vertical, 1)
                        .background(Theme.warn.opacity(0.15), in: RoundedRectangle(cornerRadius: 3))
                }
                Spacer(minLength: 0)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            HStack(spacing: 6) {
                CapacityBar(
                    fraction: machineFraction,
                    color: Theme.severity(machineFraction),
                    height: 4)
                .frame(width: 46)
                Text(String(format: "%.1f%%", process.cpuPercent))
                    .font(Theme.value(10, weight: .medium))
                    .foregroundStyle(Theme.primary)
                    .frame(width: 48, alignment: .trailing)
            }
            .frame(width: 100, alignment: .trailing)

            Text(format(memoryGauge))
                .font(Theme.value(10, weight: .regular))
                .foregroundStyle(Theme.secondary)
                .frame(width: 72, alignment: .trailing)
        }
        .padding(.vertical, 2.5)
    }

    /// Reuse the core's byte formatter so process memory reads the same as memory everywhere else.
    private var memoryGauge: MetricGauge {
        MetricGauge(
            seriesId: "", metric: "rss", label: "Memory", value: process.memoryBytes,
            max: nil, unitSuffix: "B", binaryScaled: true, history: [])
    }
}

/// `eth0   ↓ 246 KiB/s   ↑ 240 KiB/s`, with the receive sparkline behind it.
struct InterfaceRow: View {
    let entity: EntityView
    let format: (MetricGauge) -> String

    var body: some View {
        HStack(spacing: 10) {
            Text(entity.display)
                .font(Theme.value(10.5, weight: .medium))
                .foregroundStyle(Theme.primary)
                .frame(width: 78, alignment: .leading)
                .lineLimit(1)
                .truncationMode(.middle)

            if let rx = entity.gauge("rx_bytes") {
                Sparkline(values: rx.history, color: Theme.good)
                    .frame(height: 14)
                Text(format(rx))
                    .font(Theme.value(10, weight: .regular))
                    .foregroundStyle(Theme.good)
                    .frame(width: 74, alignment: .trailing)
            }
            if let tx = entity.gauge("tx_bytes") {
                Text(format(tx))
                    .font(Theme.value(10, weight: .regular))
                    .foregroundStyle(Theme.info)
                    .frame(width: 74, alignment: .trailing)
            }
        }
    }
}

/// The same shape for block devices.
struct DeviceRow: View {
    let entity: EntityView
    let format: (MetricGauge) -> String

    var body: some View {
        HStack(spacing: 10) {
            Text(entity.display)
                .font(Theme.value(10.5, weight: .medium))
                .foregroundStyle(Theme.primary)
                .frame(width: 78, alignment: .leading)
                .lineLimit(1)
                .truncationMode(.middle)

            if let read = entity.gauge("read_bytes") {
                Sparkline(values: read.history, color: Theme.good)
                    .frame(height: 14)
                Text(format(read))
                    .font(Theme.value(10, weight: .regular))
                    .foregroundStyle(Theme.good)
                    .frame(width: 74, alignment: .trailing)
            }
            if let write = entity.gauge("write_bytes") {
                Text(format(write))
                    .font(Theme.value(10, weight: .regular))
                    .foregroundStyle(Theme.warn)
                    .frame(width: 74, alignment: .trailing)
            }
        }
    }
}

struct HostHeader: View {
    let host: Host

    var body: some View {
        HStack(alignment: .center) {
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 7) {
                    Circle().fill(host.statusColor).frame(width: 8, height: 8)
                    Text(host.snapshot.displayName.isEmpty ? host.address : host.snapshot.displayName)
                        .font(.system(size: 19, weight: .semibold))
                        .foregroundStyle(Theme.primary)
                }
                Text(subtitle)
                    .font(Theme.value(10, weight: .regular))
                    .foregroundStyle(Theme.secondary)
            }
            Spacer()
            // Making the round-trip count visible keeps the app's central design claim honest: it
            // rises by exactly one per refresh, however many collectors are enabled.
            VStack(alignment: .trailing, spacing: 1) {
                Text("\(host.snapshot.roundTrips)")
                    .font(Theme.value(11, weight: .medium))
                    .foregroundStyle(Theme.secondary)
                Text("round trips")
                    .font(Theme.label(8.5))
                    .foregroundStyle(Theme.tertiary)
            }
        }
    }

    private var subtitle: String {
        let snapshot = host.snapshot
        let parts = [
            snapshot.distro,
            snapshot.kernel.isEmpty ? nil : snapshot.kernel,
            snapshot.arch.isEmpty ? nil : snapshot.arch,
        ].compactMap { $0 }.filter { !$0.isEmpty }
        return parts.isEmpty ? host.address : parts.joined(separator: "  ·  ")
    }
}

struct Banner: View {
    let text: String
    let detail: String
    let color: Color

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Label(text, systemImage: "exclamationmark.triangle.fill")
                .font(.system(size: 11.5, weight: .medium))
            Text(detail).font(.system(size: 10.5)).foregroundStyle(Theme.secondary)
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(color.opacity(0.12), in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(color.opacity(0.35)))
    }
}
