import SwiftUI

#if os(macOS)
    import AppKit
#endif

/// Adding a server, and changing one.
///
/// The same sheet does both, because "edit" that is not exactly the add form with the values
/// filled in is how two forms drift into disagreeing about what a valid host is.
///
/// One asymmetry is deliberate: an edit cannot show the existing password or key, because the
/// Keychain hands them out per connection and nothing here should hold them. A blank credential
/// field in edit mode therefore means *unchanged*, not *erase* — see `CoreModel.updateHost`.
public struct AddHostSheet: View {
    /// The saved host being changed, or `nil` to add a new one.
    private let editing: HostStore.SavedHost?
    /// The live target id behind `editing`, which is what the core keys on.
    private let editingTargetId: String?

    public init() {
        editing = nil
        editingTargetId = nil
    }

    public init(editing host: HostStore.SavedHost, targetId: String) {
        editing = host
        editingTargetId = targetId
    }

    @EnvironmentObject private var model: CoreModel
    @Environment(\.dismiss) private var dismiss

    @State private var address = ""
    @State private var port = "22"
    @State private var user = NSUserName()
    @State private var authKind = "agent"
    @State private var keyPath = ""
    @State private var keyText = ""
    @State private var secret = ""
    @State private var acceptNewHostKey = false
    @State private var refreshSeconds = 1.0
    @State private var loaded = false

    private var isEditing: Bool { editing != nil }

    private var isValid: Bool {
        !address.trimmingCharacters(in: .whitespaces).isEmpty
            && !user.trimmingCharacters(in: .whitespaces).isEmpty
            && UInt16(port) != nil
            && (authKind != "key" || !keyPath.isEmpty)
            // An edit keeps the key it already has, so an empty box is not an empty key.
            && (authKind != "key_text" || !keyText.isEmpty || isEditing)
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(isEditing ? "Edit Host" : "Add Host")
                .font(.headline)
                .padding(.bottom, 12)

            Form {
                // Autocapitalisation turns `root` into `Root`, which a case-sensitive Linux
                // account rejects — and the resulting "authentication failed" points at the
                // password rather than at the one character the keyboard changed. The same
                // applies to a hostname and a key path.
                TextField("Address", text: $address, prompt: Text("hostname or IP"))
                    .plainTextEntry()
                TextField("Port", text: $port)
                    .plainTextEntry()
                TextField("User", text: $user)
                    .plainTextEntry()

                Picker("Authentication", selection: $authKind) {
                    // Agent first and default: with it, ServerGlass never holds key material.
                    // It is also the only option a phone cannot use, which is why the paste
                    // option exists directly below it.
                    Text("SSH agent").tag("agent")
                    Text("Private key").tag("key")
                    Text("Paste a key").tag("key_text")
                    Text("Password").tag("password")
                }

                switch authKind {
                case "key":
                    HStack {
                        TextField("Key file", text: $keyPath, prompt: Text("~/.ssh/id_ed25519"))
                            .plainTextEntry()
                        #if os(macOS)
                            Button("Choose…") { chooseKey() }
                        #endif
                    }
                    SecureField(
                        isEditing ? "Passphrase (unchanged if blank)" : "Passphrase (optional)",
                        text: $secret)

                case "key_text":
                    keyEditor
                    SecureField(
                        isEditing ? "Passphrase (unchanged if blank)" : "Passphrase (optional)",
                        text: $secret)

                case "password":
                    SecureField(
                        isEditing ? "Password (unchanged if blank)" : "Password", text: $secret)

                default:
                    EmptyView()
                }

                Toggle("Trust this host key on first connection", isOn: $acceptNewHostKey)

                VStack(alignment: .leading) {
                    Text("Refresh every \(refreshSeconds, specifier: "%.1f")s")
                    Slider(value: $refreshSeconds, in: 0.5...10, step: 0.5)
                }
            }
            .formStyle(.grouped)

            Text(footnote)
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
                .padding(.top, 8)

            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button(isEditing ? "Save" : "Add") { submit() }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
                    .disabled(!isValid)
            }
            .padding(.top, 12)
        }
        .padding(18)
        #if os(macOS)
            .frame(width: 420)
        #endif
        .onAppear(perform: loadForEditing)
    }

    private var footnote: String {
        switch authKind {
        case "agent": return "Keys stay in your ssh-agent; ServerGlass never sees them."
        case "key_text":
            return "The key is stored in the Keychain on this device and sent to nothing but the "
                + "server you are connecting to."
        default: return "ServerGlass installs nothing on the server and only reads from it."
        }
    }

    /// The paste target.
    ///
    /// A private key is twenty-odd lines, so a single-line field would show about four percent of
    /// it and give no way to tell a truncated paste from a complete one. `TextEditor` in a
    /// monospaced font shows the `BEGIN`/`END` lines, which is exactly how someone checks that
    /// what they pasted is whole.
    private var keyEditor: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text("Private key")
                Spacer()
                Button("Paste") { pasteKey() }
                    .buttonStyle(.borderless)
                    .font(.system(size: 11))
            }
            TextEditor(text: $keyText)
                .font(.system(size: 11, design: .monospaced))
                .frame(minHeight: 108)
                .scrollContentBackground(.hidden)
                .background(Theme.panel, in: RoundedRectangle(cornerRadius: 6))
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Theme.panelBorder))
                // Autocorrect turning `-----BEGIN` into an em dash silently corrupts the key, and
                // the resulting failure looks like a wrong key rather than a mangled one.
                .autocorrectionDisabled()
                #if os(iOS)
                    .textInputAutocapitalization(.never)
                #endif
            if isEditing && keyText.isEmpty {
                Text("Leave empty to keep the key already stored.")
                    .font(.system(size: 10.5))
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func pasteKey() {
        #if os(macOS)
            if let text = NSPasteboard.general.string(forType: .string) { keyText = text }
        #else
            if let text = UIPasteboard.general.string { keyText = text }
        #endif
    }

    /// Fill the form from the saved record, once.
    ///
    /// `onAppear` can fire more than once for the same sheet; refilling would discard whatever the
    /// user had already typed.
    private func loadForEditing() {
        guard let editing, !loaded else { return }
        loaded = true
        address = editing.address
        port = String(editing.port)
        user = editing.user
        authKind = editing.authKind
        keyPath = editing.keyPath ?? ""
        acceptNewHostKey = editing.hostKeyPolicy != "strict"
        refreshSeconds = Double(editing.refreshMs) / 1000
    }

    #if os(macOS)
        private func chooseKey() {
            let panel = NSOpenPanel()
            panel.canChooseFiles = true
            panel.canChooseDirectories = false
            panel.allowsMultipleSelection = false
            panel.showsHiddenFiles = true
            panel.directoryURL = URL(fileURLWithPath: NSHomeDirectory())
                .appendingPathComponent(".ssh")
            if panel.runModal() == .OK, let url = panel.url {
                keyPath = url.path
            }
        }
    #endif

    private func submit() {
        let trimmedAddress = address.trimmingCharacters(in: .whitespaces)
        let trimmedUser = user.trimmingCharacters(in: .whitespaces)
        let policy = acceptNewHostKey ? "accept_new" : "strict"
        let path = authKind == "key" ? keyPath : nil
        let key = authKind == "key_text" ? keyText : nil

        if let targetId = editingTargetId {
            model.updateHost(
                id: targetId,
                address: trimmedAddress,
                port: UInt16(port) ?? 22,
                user: trimmedUser,
                authKind: authKind,
                keyPath: path,
                // nil means "leave what is stored alone"; an empty box in edit mode is not an
                // instruction to erase a credential the form could never show in the first place.
                keyText: key?.isEmpty == true ? nil : key,
                secret: secret.isEmpty ? nil : secret,
                hostKeyPolicy: policy,
                refreshMs: UInt64(refreshSeconds * 1000)
            )
        } else {
            model.addHost(
                address: trimmedAddress,
                port: UInt16(port) ?? 22,
                user: trimmedUser,
                authKind: authKind,
                keyPath: path,
                keyText: key,
                secret: authKind == "agent" ? nil : secret,
                hostKeyPolicy: policy,
                refreshMs: UInt64(refreshSeconds * 1000)
            )
        }
        dismiss()
    }
}


/// Text that is not prose: no autocapitalisation, no autocorrection.
///
/// Hostnames, usernames, paths and keys are all case- and character-exact, and a keyboard that
/// "helps" with them produces failures that look like wrong credentials rather than like a typo
/// nobody typed.
extension View {
    func plainTextEntry() -> some View {
        autocorrectionDisabled()
            #if os(iOS)
                .textInputAutocapitalization(.never)
            #endif
    }
}
