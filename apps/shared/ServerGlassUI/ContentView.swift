import ServerGlassFFI
import SwiftUI

public struct ContentView: View {
    public init() {}

    @EnvironmentObject private var model: CoreModel
    @State private var showingAddHost = false
    /// The host being edited, if any. Identified by its live target id.
    @State private var editingHost: HostTarget?
    /// Simple by default. Someone who needs the technical view will find it and it is remembered;
    /// someone who does not should never be shown a load average.
    @AppStorage("sg.showTechnicalDetails") private var showTechnical = false
    /// Which host is showing its command runner, keyed by target id.
    @State private var commandHost: HostTarget?
    #if os(iOS)
        @Environment(\.horizontalSizeClass) private var sizeClass
    #endif

    public var body: some View {
        layout
            .sheet(isPresented: $showingAddHost) { AddHostSheet() }
            .sheet(item: $editingHost) { target in
                if let saved = model.saved(for: target.id) {
                    AddHostSheet(editing: saved, targetId: target.id)
                }
            }
            .environment(\.editHost, { editingHost = HostTarget(id: $0) })
            // Errors that belong to the app rather than to one host — a Keychain refusal, a
            // target that would not start. Without this they were set and never shown.
            .alert(
                "Something went wrong",
                isPresented: Binding(
                    get: { model.lastError != nil },
                    set: { if !$0 { model.lastError = nil } })
            ) {
                Button("OK", role: .cancel) { model.lastError = nil }
            } message: {
                Text(model.lastError ?? "")
            }
            .sheet(item: $commandHost) { target in
                if let host = model.host(id: target.id) {
                    NavigationStack {
                        CommandView(host: host)
                            .navigationTitle("Run a command")
                            #if os(iOS)
                                .navigationBarTitleDisplayMode(.inline)
                            #endif
                            .toolbar {
                                ToolbarItem(placement: .cancellationAction) {
                                    Button("Done") { commandHost = nil }
                                }
                            }
                    }
                    #if os(macOS)
                        .frame(minWidth: 640, minHeight: 460)
                    #endif
                }
            }
    }

    /// A phone gets a navigation stack; anything with room gets the two-column split.
    ///
    /// `NavigationSplitView` does collapse on a compact width, but it collapses to the sidebar and
    /// never pushes — selection-based navigation has nowhere to go. On a phone the list has to
    /// push its detail, which is a different structure, not a different width.
    @ViewBuilder
    private var layout: some View {
        #if os(iOS)
            if sizeClass == .compact {
                stackLayout
            } else {
                splitLayout
            }
        #else
            splitLayout
        #endif
    }

    private var splitLayout: some View {
        NavigationSplitView {
            Sidebar(showingAddHost: $showingAddHost)
                // Also on iPad: the default sidebar claims ~40% of an 11-inch screen, which
                // squeezes the readings into two columns when three fit comfortably. The host
                // list is short names and a dot; it does not need that much room.
                .navigationSplitViewColumnWidth(min: 196, ideal: 220, max: 300)
        } detail: {
            detail(for: model.selection)
        }
        // Balanced keeps the sidebar visible on a landscape iPad instead of overlaying it.
        .navigationSplitViewStyle(.balanced)
        #if os(macOS)
            .frame(minWidth: 900, minHeight: 600)
        #endif
    }

    #if os(iOS)
        private var stackLayout: some View {
            NavigationStack {
                Group {
                    if model.hosts.isEmpty {
                        // The split layout puts this in its detail column. A stack has no detail
                        // column, so without this a phone opens on an empty "Hosts" header over
                        // black — a list of nothing, with no indication that anything is missing
                        // or what to do about it.
                        EmptyState(showingAddHost: $showingAddHost)
                    } else {
                        hostList
                    }
                }
                .navigationTitle("ServerGlass")
                .navigationDestination(for: String.self) { detail(for: $0) }
                .toolbar {
                    ToolbarItem(placement: .primaryAction) {
                        Button {
                            showingAddHost = true
                        } label: {
                            Label("Add Host", systemImage: "plus")
                        }
                    }
                }
            }
        }

        private var hostList: some View {
            List {
                if model.hosts.count > 1 {
                    NavigationLink(value: Selection.statusID) {
                        Label("All hosts", systemImage: "square.grid.2x2")
                    }
                }
                Section("Hosts") {
                    ForEach(model.hosts) { host in
                        NavigationLink(value: host.id) { SidebarRow(host: host) }
                            // Swiping is the iOS idiom but it is invisible; the context menu is
                            // how someone who has never swiped a row finds these at all.
                            .contextMenu {
                                Button("Edit…") { editingHost = HostTarget(id: host.id) }
                                Button("Remove", role: .destructive) {
                                    model.removeHost(id: host.id)
                                }
                            }
                            .swipeActions(edge: .leading) {
                                Button("Edit") { editingHost = HostTarget(id: host.id) }.tint(Theme.info)
                            }
                    }
                    .onDelete { offsets in
                        for index in offsets { model.removeHost(id: model.hosts[index].id) }
                    }
                }
            }
        }
    #endif

    @ViewBuilder
    private func detail(for selection: String?) -> some View {
        switch selection {
        case .some(Selection.statusID):
            StatusOverview(showingAddHost: $showingAddHost)
        case .some(let id):
            if let host = model.host(id: id) {
                Group {
                    if showTechnical {
                        HostDetailView(host: host)
                    } else {
                        SimpleHostView(host: host, showTechnical: $showTechnical)
                    }
                }
                .toolbar {
                    ToolbarItem(placement: .automatic) {
                        Button {
                            commandHost = HostTarget(id: host.id)
                        } label: {
                            Label("Run a command", systemImage: "terminal")
                        }
                        .help("Run a command on this server")
                        .disabled(!host.isOnline)
                    }
                    ToolbarItem(placement: .automatic) {
                        Button {
                            withAnimation { showTechnical.toggle() }
                        } label: {
                            Label(
                                showTechnical ? "Simple view" : "Technical view",
                                systemImage: showTechnical ? "gauge.low" : "chart.bar.doc.horizontal")
                        }
                        .help(showTechnical
                            ? "Show the plain-language summary"
                            : "Show every reading")
                    }
                }
                    #if os(iOS)
                        .navigationTitle(host.snapshot.displayName.isEmpty
                            ? host.address : host.snapshot.displayName)
                        .navigationBarTitleDisplayMode(.inline)
                    #endif
            } else {
                EmptyState(showingAddHost: $showingAddHost)
            }
        case nil:
            EmptyState(showingAddHost: $showingAddHost)
        }
    }
}

enum Selection {
    /// Sentinel for the all-hosts overview, so it can share the sidebar's selection binding.
    static let statusID = "__status__"
}

struct Sidebar: View {
    @EnvironmentObject private var model: CoreModel
    @Binding var showingAddHost: Bool
    @Environment(\.editHost) private var editHost

    var body: some View {
        List(selection: $model.selection) {
            // The all-hosts grid answers "is anything on fire across the fleet". With one server
            // that question is the same as the one the detail page already answers, so the entry
            // only appears once it means something.
            if model.hosts.count > 1 {
                Label("All hosts", systemImage: "square.grid.2x2")
                    .font(.system(size: 12, weight: .medium))
                    .tag(Selection.statusID)
            }

            Section("Hosts") {
                ForEach(model.hosts) { host in
                    SidebarRow(host: host)
                        .tag(host.id)
                        .contextMenu {
                            Button("Edit…") { editHost(host.id) }
                            Button("Remove", role: .destructive) { model.removeHost(id: host.id) }
                        }
                }
            }
        }
        .listStyle(.sidebar)
        #if os(macOS)
            .safeAreaInset(edge: .bottom) {
                Button {
                    showingAddHost = true
                } label: {
                    Label("Add Host", systemImage: "plus").frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderless)
                .padding(9)
            }
        #endif
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
                color: gauge.color,
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


/// How a row deep in the view tree asks for the edit sheet.
///
/// The sheet is presented once at the root — two sheets for the same host, one per list, is how
/// they end up disagreeing — so the request has to travel down rather than the presentation up.
struct EditHostAction: EnvironmentKey {
    static let defaultValue: @MainActor (String) -> Void = { _ in }
}

extension EnvironmentValues {
    var editHost: @MainActor (String) -> Void {
        get { self[EditHostAction.self] }
        set { self[EditHostAction.self] = newValue }
    }
}

/// The host a sheet is about.
///
/// `sheet(item:)` needs an `Identifiable`, which used to be supplied by conforming `String` itself
/// — a conformance on every string in the app, retroactive to a type nobody here owns, to serve
/// two sheets. A named box costs one line and claims nothing.
struct HostTarget: Identifiable, Equatable {
    let id: String
}
