package cloud.lazarev.serverglass

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties

/**
 * Adding a server, and changing one.
 *
 * The same form does both, because an edit form that is not exactly the add form with the values
 * filled in is how two forms drift into disagreeing about what a valid host is.
 *
 * Three ways in, in the order a phone can actually use them: a password, a pasted key, or a key
 * file for the rare case where one has been put on the device. The Apple apps default to the SSH
 * agent; a phone has none, and no user-visible filesystem to browse either — which is exactly why
 * pasting a key had to exist here.
 *
 * One asymmetry is deliberate: an edit cannot show the existing password or key, because the
 * Keystore hands them out per connection and nothing here should hold them. A blank credential
 * field in edit mode therefore means *unchanged*, not *erase*.
 */
@Composable
fun AddServerDialog(
    model: CoreModel,
    onDismiss: () -> Unit,
    /** The live target id being edited, or null to add a new server. */
    editing: String? = null,
) {
    val saved = remember(editing) { editing?.let { model.saved(it) } }
    val isEditing = saved != null

    var address by remember { mutableStateOf(saved?.address ?: "") }
    var port by remember { mutableStateOf(saved?.port?.toString() ?: "22") }
    var user by remember { mutableStateOf(saved?.user ?: "root") }
    var authKind by remember { mutableStateOf(saved?.authKind ?: "password") }
    var secret by remember { mutableStateOf("") }
    var keyPath by remember { mutableStateOf(saved?.keyPath ?: "") }
    var keyText by remember { mutableStateOf("") }
    var trustOnFirstUse by remember { mutableStateOf(saved?.hostKeyPolicy != "strict") }
    val clipboard = LocalClipboardManager.current

    val credentialGiven = when (authKind) {
        "key" -> keyPath.isNotBlank()
        "key_text" -> keyText.isNotBlank()
        else -> secret.isNotBlank()
    }
    val valid = address.isNotBlank() && user.isNotBlank() && port.toUShortOrNull() != null &&
        (credentialGiven || isEditing)

    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Column(
            Modifier
                .fillMaxSize()
                .background(Theme.background)
                .verticalScroll(rememberScrollState())
                .padding(20.dp),
        ) {
            Text(
                if (isEditing) "Edit server" else "Add a server",
                color = Theme.primary,
                fontSize = 24.sp,
                fontWeight = FontWeight.Bold,
            )
            Spacer(Modifier.height(6.dp))
            Text(
                "ServerGlass reads how your server is doing. It installs nothing on it.",
                color = Theme.secondary,
                fontSize = 13.sp,
            )
            Spacer(Modifier.height(20.dp))

            Field(address, { address = it }, "Address", "hostname or IP address")
            Spacer(Modifier.height(12.dp))

            Row {
                Column(Modifier.weight(2f)) {
                    Field(user, { user = it }, "Username", "the account to sign in as")
                }
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Field(port, { port = it }, "Port", "22", numeric = true)
                }
            }
            Spacer(Modifier.height(16.dp))

            Text("Sign in with", color = Theme.secondary, fontSize = 13.sp)
            Spacer(Modifier.height(8.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                AuthChip("Password", authKind == "password") { authKind = "password" }
                AuthChip("Paste a key", authKind == "key_text") { authKind = "key_text" }
                AuthChip("Key file", authKind == "key") { authKind = "key" }
            }
            Spacer(Modifier.height(12.dp))

            when (authKind) {
                "key" -> {
                    Field(keyPath, { keyPath = it }, "Key file", "/sdcard/Download/id_ed25519")
                    Spacer(Modifier.height(12.dp))
                    Field(
                        secret, { secret = it }, "Passphrase",
                        if (isEditing) "unchanged if left empty" else "leave empty if none",
                        secret = true,
                    )
                }

                "key_text" -> {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            "Private key",
                            color = Theme.secondary,
                            fontSize = 13.sp,
                            modifier = Modifier.weight(1f),
                        )
                        TextButton(onClick = {
                            clipboard.getText()?.text?.let { keyText = it }
                        }) {
                            Text("Paste", color = Theme.good, fontSize = 13.sp)
                        }
                    }
                    // Multi-line and monospaced: a private key is twenty-odd lines, and a
                    // single-line box would show four percent of it with no way to tell a
                    // truncated paste from a whole one. Seeing the BEGIN and END lines is
                    // exactly how someone checks.
                    OutlinedTextField(
                        value = keyText,
                        onValueChange = { keyText = it },
                        placeholder = {
                            Text(
                                "-----BEGIN OPENSSH PRIVATE KEY-----",
                                color = Theme.tertiary,
                                fontSize = 11.sp,
                                fontFamily = FontFamily.Monospace,
                            )
                        },
                        singleLine = false,
                        textStyle = androidx.compose.ui.text.TextStyle(
                            fontFamily = FontFamily.Monospace,
                            fontSize = 11.sp,
                            color = Theme.primary,
                        ),
                        // Autocorrect turning `-----BEGIN` into an em dash silently corrupts the
                        // key, and the failure then looks like a wrong key rather than a mangled
                        // one.
                        keyboardOptions = KeyboardOptions(
                            autoCorrectEnabled = false,
                            capitalization = KeyboardCapitalization.None,
                        ),
                        colors = TextFieldDefaults.colors(
                            focusedContainerColor = Theme.panel,
                            unfocusedContainerColor = Theme.panel,
                            focusedTextColor = Theme.primary,
                            unfocusedTextColor = Theme.primary,
                        ),
                        modifier = Modifier.fillMaxWidth().heightIn(min = 130.dp),
                    )
                    if (isEditing && keyText.isBlank()) {
                        Spacer(Modifier.height(4.dp))
                        Text(
                            "Leave empty to keep the key already stored.",
                            color = Theme.tertiary,
                            fontSize = 11.sp,
                        )
                    }
                    Spacer(Modifier.height(12.dp))
                    Field(
                        secret, { secret = it }, "Passphrase",
                        if (isEditing) "unchanged if left empty" else "leave empty if none",
                        secret = true,
                    )
                }

                else -> Field(
                    secret, { secret = it }, "Password",
                    if (isEditing) "unchanged if left empty" else "your password",
                    secret = true,
                )
            }

            Spacer(Modifier.height(18.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Switch(checked = trustOnFirstUse, onCheckedChange = { trustOnFirstUse = it })
                Spacer(Modifier.width(12.dp))
                Column {
                    Text("Trust this server", color = Theme.primary, fontSize = 14.sp)
                    Text(
                        "Remember its identity the first time you connect.",
                        color = Theme.tertiary,
                        fontSize = 11.5.sp,
                    )
                }
            }

            Spacer(Modifier.height(26.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                TextButton(onClick = onDismiss, modifier = Modifier.weight(1f)) {
                    Text("Cancel", color = Theme.secondary)
                }
                Button(
                    onClick = {
                        val path = if (authKind == "key") keyPath.trim() else null
                        val key = if (authKind == "key_text") keyText.trim() else null

                        if (editing != null) {
                            model.updateHost(
                                id = editing,
                                address = address.trim(),
                                port = port.toUShortOrNull() ?: 22U,
                                user = user.trim(),
                                authKind = authKind,
                                keyPath = path,
                                // null means "leave what is stored alone"; an empty box in edit
                                // mode is not an instruction to erase a credential the form
                                // could never show in the first place.
                                keyText = key?.takeIf { it.isNotBlank() },
                                secret = secret.takeIf { it.isNotBlank() },
                                trustOnFirstUse = trustOnFirstUse,
                            )
                        } else {
                            model.addHost(
                                address = address.trim(),
                                port = port.toUShortOrNull() ?: 22U,
                                user = user.trim(),
                                authKind = authKind,
                                keyPath = path,
                                keyText = key,
                                secret = secret,
                                trustOnFirstUse = trustOnFirstUse,
                            )
                        }
                        onDismiss()
                    },
                    enabled = valid,
                    modifier = Modifier.weight(1f),
                ) {
                    Text(if (isEditing) "Save" else "Add")
                }
            }
            Spacer(Modifier.height(40.dp))
        }
    }
}

@Composable
private fun Field(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    hint: String,
    numeric: Boolean = false,
    secret: Boolean = false,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onValueChange,
        label = { Text(label) },
        placeholder = { Text(hint, color = Theme.tertiary) },
        singleLine = true,
        keyboardOptions = KeyboardOptions(
            keyboardType = when {
                numeric -> KeyboardType.Number
                secret -> KeyboardType.Password
                else -> KeyboardType.Uri
            },
        ),
        visualTransformation =
            if (secret) {
                PasswordVisualTransformation()
            } else {
                androidx.compose.ui.text.input.VisualTransformation.None
            },
        colors = TextFieldDefaults.colors(
            focusedContainerColor = Theme.panel,
            unfocusedContainerColor = Theme.panel,
            focusedTextColor = Theme.primary,
            unfocusedTextColor = Theme.primary,
        ),
        modifier = Modifier.fillMaxWidth(),
    )
}

@Composable
private fun AuthChip(label: String, selected: Boolean, onClick: () -> Unit) {
    FilterChip(
        selected = selected,
        onClick = onClick,
        label = { Text(label) },
        colors = FilterChipDefaults.filterChipColors(
            containerColor = Theme.panel,
            labelColor = Theme.secondary,
            selectedContainerColor = Theme.good.copy(alpha = 0.20f),
            selectedLabelColor = Theme.primary,
        ),
    )
}
