import ServerGlassFFI
import SwiftUI

struct ContentView: View {
    @EnvironmentObject private var model: CoreModel
    @State private var showingAddHost = false

    var body: some View {
        NavigationSplitView {
            Sidebar(showingAddHost: $showingAddHost)
                .navigationSplitViewColumnWidth(min: 196, ideal: 214, max: 300)
        } detail: {
            switch model.selection {
            case .some(Selection.statusID):
                StatusOverview(showingAddHost: $showingAddHost)
            case .some(let id):
                if let host = model.host(id: id) {
                    HostDetailView(host: host)
                } else {
                    EmptyState(showingAddHost: $showingAddHost)
                }
            case nil:
                EmptyState(showingAddHost: $showingAddHost)
            }
        }
        .sheet(isPresented: $showingAddHost) { AddHostSheet() }
        .frame(minWidth: 900, minHeight: 600)
    }
}

enum Selection {
    /// Sentinel for the all-hosts overview, so it can share the sidebar's selection binding.
    static let statusID = "__status__"
}

struct Sidebar: View {
    @EnvironmentObject private var model: CoreModel
    @Binding var showingAddHost: Bool

    var body: some View {
        List(selection: $model.selection) {
            Label("Status", systemImage: "square.grid.2x2")
                .font(.system(size: 12, weight: .medium))
                .tag(Selection.statusID)

            Section("Hosts") {
                ForEach(model.hosts) { host in
                    SidebarRow(host: host)
                        .tag(host.id)
                        .contextMenu {
                            Button("Remove", role: .destructive) { model.removeHost(id: host.id) }
                        }
                }
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom) {
            Button {
                showingAddHost = true
            } label: {
                Label("Add Host", systemImage: "plus").frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderless)
            .padding(9)
        }
    }
}

/// Name, reachability, and the one number worth seeing without opening the host.
struct SidebarRow: View {
    let host: Host

    var body: some View {
        HStack(spacing: 8) {
            Circle().fill(host.statusColor).frame(width: 7, height: 7)
            VStack(alignment: .leading, spacing: 1) {
                Text(host.snapshot.displayName.isEmpty ? host.address : host.snapshot.displayName)
                    .font(.system(size: 12, weight: .medium))
                    .lineLimit(1)
                Text(host.statusText)
                    .font(.system(size: 9.5))
                    .foregroundStyle(Theme.secondary)
                    .lineLimit(1)
            }
            Spacer(minLength: 4)
            if let cpu = host.snapshot.gauge("cpu_usage") {
                Text(String(format: "%.0f%%", cpu.value))
                    .font(Theme.value(9.5, weight: .regular))
                    .foregroundStyle(cpu.color)
            }
        }
        .padding(.vertical, 2)
    }
}

/// The fleet at a glance: one card per host, the way the reference app's Status page works.
///
/// This is the view that answers "is anything on fire" without clicking into anything. It exists
/// because a per-host detail page, however good, cannot answer that question for eight servers.
struct StatusOverview: View {
    @EnvironmentObject private var model: CoreModel
    @Binding var showingAddHost: Bool

    var body: some View {
        ScrollView {
            if model.hosts.isEmpty {
                EmptyState(showingAddHost: $showingAddHost)
                    .frame(minHeight: 400)
            } else {
                LazyVGrid(
                    columns: [GridItem(.adaptive(minimum: 268, maximum: 380), spacing: 12)],
                    spacing: 12
                ) {
                    ForEach(model.hosts) { host in
                        HostCard(host: host)
                            .onTapGesture { model.selection = host.id }
                    }
                }
                .padding(14)
            }
        }
        .background(Theme.background)
        .navigationTitle("Status")
    }
}

/// One host as a card: the headline ring, then the three numbers that decide whether to look
/// closer. Deliberately not a shrunken copy of the detail page.
struct HostCard: View {
    @EnvironmentObject private var model: CoreModel
    let host: Host

    private var snapshot: TargetSnapshot { host.snapshot }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 6) {
                Circle().fill(host.statusColor).frame(width: 7, height: 7)
                Text(snapshot.displayName.isEmpty ? host.address : snapshot.displayName)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(Theme.primary)
                    .lineLimit(1)
                Spacer(minLength: 4)
                if let uptime = snapshot.gauge("uptime") {
                    Text(model.format(uptime))
                        .font(Theme.value(9.5, weight: .regular))
                        .foregroundStyle(Theme.tertiary)
                }
            }

            if snapshot.gauges.isEmpty {
                HStack {
                    ProgressView().controlSize(.small)
                    Text(host.statusText)
                        .font(.system(size: 10.5))
                        .foregroundStyle(Theme.secondary)
                        .lineLimit(2)
                }
                .frame(maxWidth: .infinity, minHeight: 88, alignment: .leading)
            } else {
                HStack(spacing: 14) {
                    if let cpu = snapshot.gauge("cpu_usage") {
                        RingGauge(
                            fraction: cpu.fraction ?? 0,
                            color: cpu.color,
                            lineWidth: 6,
                            caption: model.format(cpu),
                            sub: "CPU")
                        .frame(width: 78, height: 78)
                    }

                    VStack(alignment: .leading, spacing: 7) {
                        ForEach(["mem_usage", "disk_usage", "load1"], id: \.self) { metric in
                            if let gauge = snapshot.gauge(metric) {
                                MiniStat(gauge: gauge, text: model.format(gauge))
                            }
                        }
                    }
                }

                HStack(spacing: 12) {
                    if let rx = snapshot.gauge("net_rx") {
                        Label(model.format(rx), systemImage: "arrow.down")
                            .font(Theme.value(9.5, weight: .regular))
                            .foregroundStyle(Theme.good)
                    }
                    if let tx = snapshot.gauge("net_tx") {
                        Label(model.format(tx), systemImage: "arrow.up")
                            .font(Theme.value(9.5, weight: .regular))
                            .foregroundStyle(Theme.info)
                    }
                    Spacer(minLength: 0)
                    Text("\(snapshot.cpuCount) cores")
                        .font(Theme.label(9))
                        .foregroundStyle(Theme.tertiary)
                }
            }
        }
        .padding(12)
        .background(Theme.panel, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Theme.panelBorder))
        .contentShape(Rectangle())
    }
}

/// A labelled bar, for the card's secondary metrics.
struct MiniStat: View {
    let gauge: MetricGauge
    let text: String

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 4) {
                Text(gauge.label)
                    .font(Theme.label(9))
                    .foregroundStyle(Theme.secondary)
                Spacer(minLength: 6)
                Text(text)
                    .font(Theme.value(9.5, weight: .medium))
                    .foregroundStyle(Theme.primary)
            }
            CapacityBar(
                fraction: gauge.fraction ?? 0,
                color: gauge.fraction.map(Theme.severity) ?? Theme.info,
                height: 4)
        }
    }
}

struct EmptyState: View {
    @Binding var showingAddHost: Bool

    var body: some View {
        VStack(spacing: 11) {
            Image(systemName: "server.rack")
                .font(.system(size: 38))
                .foregroundStyle(Theme.tertiary)
            Text("No hosts yet").font(.system(size: 15, weight: .medium))
            Text("ServerGlass installs nothing on your servers — it only needs SSH access.")
                .font(.system(size: 11.5))
                .foregroundStyle(Theme.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 320)
            Button("Add Host") { showingAddHost = true }
                .buttonStyle(.borderedProminent)
                .padding(.top, 2)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.background)
    }
}
