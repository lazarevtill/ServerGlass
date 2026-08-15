package cloud.lazarev.serverglass

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch

/** One command and what came back. */
data class CommandEntry(
    val command: String,
    val output: String,
    val exitCode: Int,
    val elapsedMs: Long,
) {
    val failed: Boolean get() = exitCode != 0
}

/**
 * Running a command on the server.
 *
 * Honest about what it is: a command runner, not a terminal. There is no PTY behind it, so `top`,
 * `vim` and anything that prompts will hang rather than work — and the screen says so instead of
 * leaving someone to find out by waiting sixty seconds. What it does do is the thing people
 * actually reach for on a phone: `systemctl restart nginx`, `df -h`, `docker ps`, `tail -n 50
 * /var/log/syslog`.
 *
 * It runs on the same connection the readings use, so there is no second sign-in and no second
 * session for the host to log.
 */
@Composable
fun CommandScreen(
    host: Host,
    model: CoreModel,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var command by remember { mutableStateOf("") }
    var running by remember { mutableStateOf(false) }
    val entries = remember { mutableStateListOf<CommandEntry>() }
    val scope = rememberCoroutineScope()
    val listState = rememberLazyListState()
    val online = host.snapshot.state is uniffi.sg_ffi.ConnectionState.Online

    // Follow the output the way a terminal does, rather than leaving the newest answer below the
    // fold.
    LaunchedEffect(entries.size) {
        if (entries.isNotEmpty()) listState.animateScrollToItem(entries.size - 1)
    }

    fun run() {
        val typed = command.trim()
        if (typed.isEmpty() || !online || running) return
        command = ""
        running = true
        scope.launch {
            // `runCommand` blocks until the host answers; the model puts it on an IO thread so
            // the UI keeps repainting.
            val result = model.runCommand(host.id, typed)
            entries.add(result)
            running = false
        }
    }

    Column(modifier.fillMaxSize().background(Theme.background).imePadding()) {
        Row(
            Modifier.fillMaxWidth().padding(start = 4.dp, end = 14.dp, top = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onBack) {
                Icon(
                    Icons.AutoMirrored.Filled.ArrowBack,
                    contentDescription = "Back",
                    tint = Theme.primary,
                )
            }
            Column(Modifier.weight(1f)) {
                Text(
                    "Run a command",
                    color = Theme.primary,
                    fontSize = 16.sp,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    host.snapshot.displayName.ifEmpty { host.address },
                    color = Theme.tertiary,
                    fontSize = 11.sp,
                    maxLines = 1,
                )
            }
        }

        LazyColumn(
            state = listState,
            modifier = Modifier.weight(1f).fillMaxWidth().padding(horizontal = 14.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            if (entries.isEmpty()) {
                item { Placeholder { command = it } }
            }
            items(entries) { entry ->
                Column(Modifier.fillMaxWidth()) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            "$",
                            color = Theme.good,
                            fontSize = 12.sp,
                            fontFamily = FontFamily.Monospace,
                        )
                        Spacer(Modifier.width(7.dp))
                        Text(
                            entry.command,
                            color = Theme.primary,
                            fontSize = 12.sp,
                            fontFamily = FontFamily.Monospace,
                            modifier = Modifier.weight(1f),
                        )
                        Text(
                            if (entry.failed) "exit ${entry.exitCode}" else "${entry.elapsedMs} ms",
                            color = if (entry.failed) Theme.bad else Theme.tertiary,
                            fontSize = 11.sp,
                            fontFamily = FontFamily.Monospace,
                        )
                    }
                    if (entry.output.isNotEmpty()) {
                        Spacer(Modifier.height(5.dp))
                        Text(
                            entry.output,
                            color = Theme.secondary,
                            fontSize = 11.5.sp,
                            fontFamily = FontFamily.Monospace,
                        )
                    }
                }
            }
            item { Spacer(Modifier.height(8.dp)) }
        }

        HorizontalDivider(color = Theme.border)
        Row(
            Modifier.fillMaxWidth().background(Theme.panel).padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedTextField(
                value = command,
                onValueChange = { command = it },
                placeholder = {
                    Text(
                        if (online) "command" else "not connected",
                        color = Theme.tertiary,
                        fontSize = 13.sp,
                        fontFamily = FontFamily.Monospace,
                    )
                },
                singleLine = true,
                enabled = online && !running,
                textStyle = androidx.compose.ui.text.TextStyle(
                    fontFamily = FontFamily.Monospace,
                    fontSize = 13.sp,
                    color = Theme.primary,
                ),
                keyboardOptions = KeyboardOptions(
                    autoCorrectEnabled = false,
                    capitalization = KeyboardCapitalization.None,
                    imeAction = ImeAction.Go,
                ),
                keyboardActions = KeyboardActions(onGo = { run() }),
                colors = TextFieldDefaults.colors(
                    focusedContainerColor = Theme.card,
                    unfocusedContainerColor = Theme.card,
                    disabledContainerColor = Theme.card,
                    focusedTextColor = Theme.primary,
                    unfocusedTextColor = Theme.primary,
                ),
                modifier = Modifier.weight(1f),
            )
            Spacer(Modifier.width(10.dp))
            if (running) {
                CircularProgressIndicator(
                    color = Theme.good,
                    modifier = Modifier.size(22.dp),
                )
            } else {
                Button(onClick = { run() }, enabled = online && command.isNotBlank()) {
                    Text("Run")
                }
            }
        }
    }
}

@Composable
private fun Placeholder(onPick: (String) -> Unit) {
    Column(Modifier.fillMaxWidth().padding(top = 10.dp)) {
        Text(
            "Commands run on the connection ServerGlass already has open.",
            color = Theme.secondary,
            fontSize = 12.5.sp,
        )
        Spacer(Modifier.height(6.dp))
        Text(
            "Programs that need a terminal of their own — top, vim, anything that asks a " +
                "question — will not work here.",
            color = Theme.tertiary,
            fontSize = 11.5.sp,
        )
        Spacer(Modifier.height(12.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            listOf("df -h", "docker ps", "uptime").forEach { suggestion ->
                Box(
                    Modifier
                        .clip(RoundedCornerShape(20.dp))
                        .background(Theme.card)
                        .border(1.dp, Theme.border, RoundedCornerShape(20.dp))
                        .clickable { onPick(suggestion) }
                        .padding(horizontal = 11.dp, vertical = 6.dp),
                ) {
                    Text(
                        suggestion,
                        color = Theme.secondary,
                        fontSize = 11.5.sp,
                        fontFamily = FontFamily.Monospace,
                    )
                }
            }
        }
    }
}
