package cloud.lazarev.serverglass

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
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
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties

/**
 * Adding a server.
 *
 * The Apple apps default to the SSH agent, because on a Mac there almost always is one. A phone has
 * none, and no user-visible filesystem to browse for a key file either, so the honest default here
 * is a password — with a key file available for anyone who has managed to get one onto the device.
 *
 * The wording avoids naming the protocol. Someone who does not know what SSH is still knows what
 * "the address of the server" and "your password" mean.
 */
@Composable
fun AddServerDialog(model: CoreModel, onDismiss: () -> Unit) {
    var address by remember { mutableStateOf("") }
    var port by remember { mutableStateOf("22") }
    var user by remember { mutableStateOf("root") }
    var usesKeyFile by remember { mutableStateOf(false) }
    var secret by remember { mutableStateOf("") }
    var keyPath by remember { mutableStateOf("") }
    var trustOnFirstUse by remember { mutableStateOf(true) }

    val valid = address.isNotBlank() && user.isNotBlank() && port.toUShortOrNull() != null &&
        (if (usesKeyFile) keyPath.isNotBlank() else secret.isNotBlank())

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
                "Add a server",
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
                AuthChip("Password", !usesKeyFile) { usesKeyFile = false }
                AuthChip("Key file", usesKeyFile) { usesKeyFile = true }
            }
            Spacer(Modifier.height(12.dp))

            if (usesKeyFile) {
                Field(keyPath, { keyPath = it }, "Key file", "/sdcard/Download/id_ed25519")
                Spacer(Modifier.height(12.dp))
                Field(secret, { secret = it }, "Passphrase", "leave empty if none", secret = true)
            } else {
                Field(secret, { secret = it }, "Password", "your password", secret = true)
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
                        model.addHost(
                            address = address.trim(),
                            port = port.toUShortOrNull() ?: 22U,
                            user = user.trim(),
                            authKind = if (usesKeyFile) "key" else "password",
                            keyPath = if (usesKeyFile) keyPath.trim() else null,
                            secret = secret,
                            trustOnFirstUse = trustOnFirstUse,
                        )
                        onDismiss()
                    },
                    enabled = valid,
                    modifier = Modifier.weight(1f),
                ) {
                    Text("Add")
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
            if (secret) PasswordVisualTransformation() else androidx.compose.ui.text.input.VisualTransformation.None,
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
