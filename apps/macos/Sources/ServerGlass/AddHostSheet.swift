import SwiftUI

struct AddHostSheet: View {
    @EnvironmentObject private var model: CoreModel
    @Environment(\.dismiss) private var dismiss

    @State private var address = ""
    @State private var port = "22"
    @State private var user = NSUserName()
    @State private var authKind = "agent"
    @State private var keyPath = ""
    @State private var secret = ""
    @State private var acceptNewHostKey = false
    @State private var refreshSeconds = 1.0

    private var isValid: Bool {
        !address.trimmingCharacters(in: .whitespaces).isEmpty
            && !user.trimmingCharacters(in: .whitespaces).isEmpty
            && UInt16(port) != nil
            && (authKind != "key" || !keyPath.isEmpty)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Add Host")
                .font(.headline)
                .padding(.bottom, 12)

            Form {
                TextField("Address", text: $address, prompt: Text("hostname or IP"))
                TextField("Port", text: $port)
                TextField("User", text: $user)

                Picker("Authentication", selection: $authKind) {
                    // Agent first and default: with it, ServerGlass never holds key material.
                    Text("SSH agent").tag("agent")
                    Text("Private key").tag("key")
                    Text("Password").tag("password")
                }

                if authKind == "key" {
                    HStack {
                        TextField("Key file", text: $keyPath, prompt: Text("~/.ssh/id_ed25519"))
                        Button("Choose…") { chooseKey() }
                    }
                    SecureField("Passphrase (optional)", text: $secret)
                } else if authKind == "password" {
                    SecureField("Password", text: $secret)
                }

                Toggle("Trust this host key on first connection", isOn: $acceptNewHostKey)

                VStack(alignment: .leading) {
                    Text("Refresh every \(refreshSeconds, specifier: "%.1f")s")
                    Slider(value: $refreshSeconds, in: 0.5...10, step: 0.5)
                }
            }
            .formStyle(.grouped)

            Text(
                authKind == "agent"
                    ? "Keys stay in your ssh-agent; ServerGlass never sees them."
                    : "ServerGlass installs nothing on the server and only reads from it."
            )
            .font(.system(size: 11))
            .foregroundStyle(.secondary)
            .padding(.top, 8)

            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Add") { add() }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
                    .disabled(!isValid)
            }
            .padding(.top, 12)
        }
        .padding(18)
        .frame(width: 420)
    }

    private func chooseKey() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.showsHiddenFiles = true
        panel.directoryURL = URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent(".ssh")
        if panel.runModal() == .OK, let url = panel.url {
            keyPath = url.path
        }
    }

    private func add() {
        model.addHost(
            address: address.trimmingCharacters(in: .whitespaces),
            port: UInt16(port) ?? 22,
            user: user.trimmingCharacters(in: .whitespaces),
            authKind: authKind,
            keyPath: authKind == "key" ? keyPath : nil,
            secret: authKind == "agent" ? nil : secret,
            hostKeyPolicy: acceptNewHostKey ? "accept_new" : "strict",
            refreshMs: UInt64(refreshSeconds * 1000)
        )
        dismiss()
    }
}
