import ServerGlassFFI
import SwiftUI

/// Running a command on the server.
///
/// Honest about what it is: a command runner, not a terminal. There is no PTY behind it, so
/// `top`, `vim` and anything that prompts will hang rather than work — and the screen says so
/// instead of leaving someone to discover it by waiting sixty seconds. What it does do is the
/// thing people actually reach for on a phone: `systemctl restart nginx`, `df -h`, `docker ps`,
/// `tail -n 50 /var/log/syslog`.
///
/// It runs on the same connection the readings use, so there is no second sign-in and no second
/// session for the host to log.
struct CommandView: View {
    @EnvironmentObject private var model: CoreModel
    let host: Host

    @State private var command = ""
    @State private var entries: [Entry] = []
    @State private var running = false
    @FocusState private var inputFocused: Bool

    /// One command and what came back.
    struct Entry: Identifiable {
        let id = UUID()
        let command: String
        var output: String
        var exitCode: Int32
        var elapsedMs: UInt64
        var failed: Bool { exitCode != 0 }
    }

    private var isOnline: Bool { host.isOnline }

    var body: some View {
        VStack(spacing: 0) {
            transcript
            Divider().overlay(Theme.panelBorder)
            input
        }
        .background(Theme.background)
    }

    private var transcript: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 14) {
                    if entries.isEmpty {
                        placeholder
                    }
                    ForEach(entries) { entry in
                        VStack(alignment: .leading, spacing: 5) {
                            HStack(spacing: 7) {
                                Text("$").foregroundStyle(Theme.good)
                                Text(entry.command)
                                    .foregroundStyle(Theme.primary)
                                    .textSelection(.enabled)
                                Spacer(minLength: 0)
                                Text(
                                    entry.failed
                                        ? "exit \(entry.exitCode)" : "\(entry.elapsedMs) ms"
                                )
                                .foregroundStyle(entry.failed ? Theme.bad : Theme.tertiary)
                            }
                            .font(Theme.value(12, weight: .medium))

                            if !entry.output.isEmpty {
                                Text(entry.output)
                                    .font(Theme.value(11.5))
                                    .foregroundStyle(Theme.secondary)
                                    .textSelection(.enabled)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                        }
                        .id(entry.id)
                    }
                }
                .padding(14)
            }
            // Follow the output the way a terminal does, rather than leaving the newest answer
            // below the fold.
            .onChange(of: entries.count) {
                if let last = entries.last { proxy.scrollTo(last.id, anchor: .bottom) }
            }
        }
    }

    private var placeholder: some View {
        VStack(alignment: .leading, spacing: 7) {
            Text("Run a command on \(host.snapshot.displayName.isEmpty ? host.address : host.snapshot.displayName)")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(Theme.primary)
            Text(
                "It runs on the connection ServerGlass already has open. Programs that need a "
                    + "terminal of their own — top, vim, anything that asks a question — will not "
                    + "work here."
            )
            .font(.system(size: 11.5))
            .foregroundStyle(Theme.secondary)

            HStack(spacing: 8) {
                ForEach(["df -h", "docker ps", "uptime"], id: \.self) { suggestion in
                    Button(suggestion) { command = suggestion }
                        .font(Theme.value(11))
                        .buttonStyle(.plain)
                        .padding(.horizontal, 9)
                        .padding(.vertical, 5)
                        .background(Theme.card, in: Capsule())
                        .overlay(Capsule().strokeBorder(Theme.panelBorder))
                        .foregroundStyle(Theme.secondary)
                }
            }
            .padding(.top, 4)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var input: some View {
        HStack(spacing: 9) {
            Text("$").font(Theme.value(13)).foregroundStyle(Theme.good)
            TextField("command", text: $command)
                .textFieldStyle(.plain)
                .font(Theme.value(13))
                .focused($inputFocused)
                .autocorrectionDisabled()
                #if os(iOS)
                    .textInputAutocapitalization(.never)
                #endif
                .onSubmit(run)
                .disabled(!isOnline || running)

            if running {
                ProgressView().controlSize(.small)
            } else {
                Button("Run", action: run)
                    .buttonStyle(.borderedProminent)
                    .controlSize(.small)
                    .disabled(!isOnline || command.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(12)
        .background(Theme.panel)
        .overlay(alignment: .top) {
            if !isOnline {
                Text("Not connected — commands need a live connection.")
                    .font(.system(size: 10.5))
                    .foregroundStyle(Theme.warn)
                    .padding(.top, 2)
            }
        }
    }

    private func run() {
        let typed = command.trimmingCharacters(in: .whitespaces)
        guard !typed.isEmpty, isOnline, !running else { return }

        command = ""
        running = true
        entries.append(Entry(command: typed, output: "", exitCode: 0, elapsedMs: 0))
        let index = entries.count - 1

        // Off the main thread: the call blocks until the host answers, and blocking the UI thread
        // on a network round trip is how an app stops repainting mid-command.
        Task {
            let result = await model.runCommand(hostId: host.id, command: typed)
            entries[index].output = result.output
            entries[index].exitCode = result.exitCode
            entries[index].elapsedMs = result.elapsedMs
            running = false
            inputFocused = true
        }
    }
}
