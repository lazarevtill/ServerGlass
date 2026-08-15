package cloud.lazarev.serverglass

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.sg_ffi.ServerGlass
import uniffi.sg_ffi.TargetConfig
import uniffi.sg_ffi.TargetSnapshot

/** A host the user has added, plus its latest snapshot. */
data class Host(
    val id: String,
    val address: String,
    val snapshot: TargetSnapshot,
)

/**
 * The bridge between the Rust core and Compose.
 *
 * Identical in shape to the Swift `CoreModel`: the core runs its own refresh loop on its own
 * threads and publishes a finished snapshot per tick, and this polls it. Nothing here parses,
 * schedules or decides anything — the health verdicts, the plain-language wording and the number
 * formatting all come from Rust, so this app says exactly what the Mac and iPhone say.
 */
class CoreModel : ViewModel() {
    private val core = ServerGlass()

    var hosts by mutableStateOf<List<Host>>(emptyList())
        private set
    var selection by mutableStateOf<String?>(null)
    var showTechnical by mutableStateOf(false)

    init {
        viewModelScope.launch {
            while (true) {
                delay(500)
                poll()
            }
        }
    }

    fun addHost(
        address: String,
        port: UShort,
        user: String,
        authKind: String,
        keyPath: String?,
        secret: String?,
        trustOnFirstUse: Boolean,
        refreshMs: ULong = 1000UL,
    ) {
        val config = TargetConfig(
            host = address,
            port = port,
            user = user,
            authKind = authKind,
            keyPath = keyPath?.takeIf { it.isNotBlank() },
            secret = secret?.takeIf { it.isNotBlank() },
            hostKeyPolicy = if (trustOnFirstUse) "accept_new" else "strict",
            refreshMs = refreshMs,
        )

        viewModelScope.launch {
            val id = withContext(Dispatchers.IO) {
                val id = core.addTarget(config)
                core.start(id)
                id
            }
            hosts = hosts + Host(id, "$user@$address", core.snapshot(id))
            if (selection == null) selection = id
        }
    }

    fun removeHost(id: String) {
        viewModelScope.launch {
            withContext(Dispatchers.IO) { core.removeTarget(id) }
            hosts = hosts.filterNot { it.id == id }
            if (selection == id) selection = hosts.firstOrNull()?.id
        }
    }

    fun host(id: String?): Host? = hosts.firstOrNull { it.id == id }

    /** Format a value exactly as the core does, so the platforms never drift apart. */
    fun format(value: Double, unitSuffix: String, binaryScaled: Boolean): String =
        core.format(value, unitSuffix, binaryScaled)

    fun formatDuration(seconds: Double): String = core.formatDuration(seconds)

    private fun poll() {
        if (hosts.isEmpty()) return
        hosts = hosts.map { host ->
            runCatching { host.copy(snapshot = core.snapshot(host.id)) }.getOrDefault(host)
        }
    }

    /**
     * Development convenience, mirroring the Apple apps' `SG_DEMO_HOST`.
     *
     * Driven by an Intent extra rather than an environment variable, because `adb shell am start`
     * cannot set the latter:
     *
     *     adb shell am start -n cloud.lazarev.serverglass/.MainActivity \
     *         -e host root@10.0.2.2:2222 -e key /data/local/tmp/id_test
     *
     * Note `10.0.2.2` — from inside the emulator that is the host machine, where 127.0.0.1 is the
     * emulated device itself.
     */
    fun addDemoHost(target: String, key: String?) {
        if (hosts.isNotEmpty()) return
        val user = if (target.contains('@')) target.substringBefore('@') else "root"
        val rest = target.substringAfter('@')
        val address = rest.substringBefore(':')
        val port = rest.substringAfter(':', "22").toUShortOrNull() ?: 22U

        addHost(
            address = address,
            port = port,
            user = user,
            authKind = if (key == null) "agent" else "key",
            keyPath = key,
            secret = null,
            trustOnFirstUse = true,
        )
    }
}
