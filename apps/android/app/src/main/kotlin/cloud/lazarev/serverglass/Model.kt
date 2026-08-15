package cloud.lazarev.serverglass

import android.app.Application
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.util.UUID
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
    /** Identifier of the persisted record, so removing a host also forgets its stored secret. */
    val savedId: String = "",
)

/**
 * The bridge between the Rust core and Compose.
 *
 * Identical in shape to the Swift `CoreModel`: the core runs its own refresh loop on its own
 * threads and publishes a finished snapshot per tick, and this polls it. Nothing here parses,
 * schedules or decides anything — the health verdicts, the plain-language wording and the number
 * formatting all come from Rust, so this app says exactly what the Mac and iPhone say.
 */
class CoreModel(application: Application) : AndroidViewModel(application) {
    private val core = ServerGlass()
    private val store = HostStore(application)

    var hosts by mutableStateOf<List<Host>>(emptyList())
        private set
    var selection by mutableStateOf<String?>(null)
    var showTechnical by mutableStateOf(false)
        private set

    /** False when the Keystore was unavailable and secrets fell back to plain preferences. */
    val secretsAreHardwareBacked: Boolean get() = store.secretsAreHardwareBacked

    /** Secrets for hosts that were deliberately not saved. */
    private val ephemeralSecrets = mutableMapOf<String, String?>()

    init {
        viewModelScope.launch {
            while (true) {
                delay(500)
                poll()
            }
        }
        showTechnical = store.showTechnical()
        restore()
    }

    /** Reconnect everything added in a previous session. */
    private fun restore() {
        store.load().forEach { start(it) }
    }

    /**
     * Bring a saved host up: hand its config to the core, start polling, and show it.
     *
     * The secret is read here rather than held beside the rest of the host, so it exists in memory
     * only for as long as building the config takes.
     */
    private fun start(saved: HostStore.SavedHost, selectWhenReady: Boolean = false) {
        val config = TargetConfig(
            host = saved.address,
            port = saved.port,
            user = saved.user,
            authKind = saved.authKind,
            keyPath = saved.keyPath,
            // A pasted key is key material, so it comes from the encrypted store rather than the
            // record, exactly like the passphrase beside it.
            keyText = store.secret(saved.id, HostStore.Kind.KEY_TEXT),
            secret = if (ephemeralSecrets.containsKey(saved.id)) {
                ephemeralSecrets[saved.id]
            } else {
                store.secret(saved.id)
            },
            hostKeyPolicy = saved.hostKeyPolicy,
            refreshMs = saved.refreshMs,
        )

        viewModelScope.launch {
            val id = withContext(Dispatchers.IO) {
                val id = core.addTarget(config)
                core.start(id)
                id
            }
            // Deliberately does not select the new host. On a phone `selection` means "the user
            // navigated into a server" and the detail screen replaces the list, so selecting
            // automatically would drop someone into detail on launch with the list — and the Add
            // button with it — out of reach. Two-pane layouts, where the list never leaves the
            // screen, opt into a default selection themselves.
            hosts = hosts + Host(id, "${saved.user}@${saved.address}", core.snapshot(id), saved.id)
            // Only after an edit, so someone watching a host stays on it rather than being sent
            // back to the list by their own change.
            if (selectWhenReady) selection = id
        }
    }

    fun addHost(
        address: String,
        port: UShort,
        user: String,
        authKind: String,
        keyPath: String?,
        keyText: String? = null,
        secret: String?,
        trustOnFirstUse: Boolean,
        refreshMs: ULong = 1000UL,
        /** False for the development demo host, which would otherwise be saved on every launch. */
        persist: Boolean = true,
    ) {
        val saved = HostStore.SavedHost(
            id = UUID.randomUUID().toString(),
            address = address,
            port = port,
            user = user,
            authKind = authKind,
            keyPath = keyPath?.takeIf { it.isNotBlank() },
            hostKeyPolicy = if (trustOnFirstUse) "accept_new" else "strict",
            refreshMs = refreshMs,
        )

        if (persist) {
            // The secret goes to the encrypted store and nowhere else; the record never carries it.
            store.setSecret(saved.id, secret?.takeIf { it.isNotBlank() })
            store.setSecret(saved.id, keyText?.takeIf { it.isNotBlank() }, HostStore.Kind.KEY_TEXT)
            store.save(store.load() + saved)
        } else {
            // Not saved, so the secret has to travel in memory rather than through the Keystore.
            ephemeralSecrets[saved.id] = secret?.takeIf { it.isNotBlank() }
        }

        start(saved)
    }

    fun removeHost(id: String) {
        viewModelScope.launch {
            withContext(Dispatchers.IO) { core.removeTarget(id) }
            // Forget the stored record and its secret too, or the host returns on the next launch
            // and its password is left behind in the Keystore.
            hosts.firstOrNull { it.id == id }?.savedId?.takeIf { it.isNotEmpty() }
                ?.let { savedId ->
                    store.forget(savedId)
                    ephemeralSecrets.remove(savedId)
                }
            hosts = hosts.filterNot { it.id == id }
            if (selection == id) selection = hosts.firstOrNull()?.id
        }
    }

    /**
     * Change a saved host and reconnect it with the new settings.
     *
     * Reconnects rather than editing in place because every field here is a connection parameter:
     * a new address, port or credential cannot apply to a session already established with the old
     * ones. The record keeps its identifier, so its stored secret is updated rather than orphaned
     * — which is what remove-then-add would do.
     *
     * `secret` and `keyText` are null when the field was left untouched, which is not the same as
     * an empty string meaning "clear it". An edit form cannot show an existing password, so
     * treating a blank box as a deliberate erasure would silently discard the credential of
     * anyone who edited a port number.
     */
    fun updateHost(
        id: String,
        address: String,
        port: UShort,
        user: String,
        authKind: String,
        keyPath: String?,
        keyText: String?,
        secret: String?,
        trustOnFirstUse: Boolean,
        refreshMs: ULong = 1000UL,
    ) {
        val savedId = hosts.firstOrNull { it.id == id }?.savedId ?: return
        val stored = store.load().toMutableList()
        val index = stored.indexOfFirst { it.id == savedId }
        if (index < 0) return

        stored[index] = HostStore.SavedHost(
            id = savedId,
            address = address,
            port = port,
            user = user,
            authKind = authKind,
            keyPath = keyPath?.takeIf { it.isNotBlank() },
            hostKeyPolicy = if (trustOnFirstUse) "accept_new" else "strict",
            refreshMs = refreshMs,
        )
        store.save(stored)
        secret?.let { store.setSecret(savedId, it.takeIf(String::isNotBlank)) }
        keyText?.let { store.setSecret(savedId, it.takeIf(String::isNotBlank), HostStore.Kind.KEY_TEXT) }

        viewModelScope.launch {
            // Not removeHost: that would also erase the record and its secrets, the very things
            // being kept.
            withContext(Dispatchers.IO) { core.removeTarget(id) }
            val wasSelected = selection == id
            hosts = hosts.filterNot { it.id == id }
            if (wasSelected) selection = null
            start(stored[index], selectWhenReady = wasSelected)
        }
    }

    /** The saved record behind a live host, for populating an edit form. */
    fun saved(id: String): HostStore.SavedHost? {
        val savedId = hosts.firstOrNull { it.id == id }?.savedId ?: return null
        return store.load().firstOrNull { it.id == savedId }
    }

    /**
     * Run a command on a host.
     *
     * The core call blocks until the host answers, so it goes to an IO thread. Failures come back
     * as output rather than as a thrown exception, because from the reader's point of view "could
     * not run it" and "it printed an error" belong in the same place — the transcript.
     */
    suspend fun runCommand(hostId: String, command: String): CommandEntry =
        withContext(Dispatchers.IO) {
            try {
                val result = core.runCommand(hostId, command)
                CommandEntry(
                    command = command,
                    output = result.output,
                    exitCode = result.exitCode,
                    elapsedMs = result.elapsedMs.toLong(),
                )
            } catch (e: Exception) {
                CommandEntry(
                    command = command,
                    output = e.message ?: "The command could not be run.",
                    exitCode = -1,
                    elapsedMs = 0,
                )
            }
        }

    fun host(id: String?): Host? = hosts.firstOrNull { it.id == id }

    /** Format a value exactly as the core does, so the platforms never drift apart. */
    fun format(value: Double, unitSuffix: String, binaryScaled: Boolean): String =
        core.format(value, unitSuffix, binaryScaled)

    fun formatDuration(seconds: Double): String = core.formatDuration(seconds)

    /** Format a gauge the way the core would, uptime included. */
    fun formatGauge(gauge: uniffi.sg_ffi.MetricGauge): String =
        if (gauge.metric == "uptime") {
            core.formatDuration(gauge.value)
        } else {
            core.format(gauge.value, gauge.unitSuffix, gauge.binaryScaled)
        }

    /**
     * Switch between the plain-language summary and every reading, and remember the choice.
     *
     * Named `showTechnical(…)` rather than `setShowTechnical` because Kotlin already generates
     * that name for the property itself, and the two would collide on the JVM.
     */
    fun showTechnical(value: Boolean) {
        showTechnical = value
        store.setShowTechnical(value)
    }

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
            persist = false,
        )
    }
}
