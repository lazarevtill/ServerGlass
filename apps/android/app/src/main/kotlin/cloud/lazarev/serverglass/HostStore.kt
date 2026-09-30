package cloud.lazarev.serverglass

import android.content.Context
import android.content.SharedPreferences
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import org.json.JSONArray
import org.json.JSONObject

/**
 * Persistence for the servers a person has added.
 *
 * Mirrors the Apple side exactly, using the platform's own facilities:
 *
 * - **Configuration** — address, port, username, sign-in method, key path — is not secret and lives
 *   in ordinary preferences as JSON.
 * - **Secrets** — passwords and key passphrases — live in `EncryptedSharedPreferences`, whose keys
 *   are held in the Android Keystore and, on hardware that has one, never leave the secure element.
 *
 * Secret storage is the one place the "core owns all logic" rule is deliberately broken. The
 * Keystore and the Keychain are operating-system facilities backed by hardware the app cannot reach
 * from Rust; reimplementing them in the core would mean inventing key management rather than using
 * the one the platform already audits.
 */
class HostStore(context: Context) {

    data class SavedHost(
        val id: String,
        val address: String,
        val port: UShort,
        val user: String,
        val authKind: String,
        val keyPath: String?,
        val hostKeyPolicy: String,
        val refreshMs: ULong,
    )

    private val config: SharedPreferences =
        context.getSharedPreferences("sg.hosts", Context.MODE_PRIVATE)

    // A broken vault must never silently turn passwords and private keys into plain preferences.
    private val secrets: SharedPreferences? = try {
        val masterKey = MasterKey.Builder(context)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()
        val encrypted = EncryptedSharedPreferences.create(
            context,
            "sg.secrets",
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM,
        )
        // Upgrade the old fallback only after a durable encrypted write. Until then the original
        // remains recoverable; an unavailable Keystore must not erase someone's credentials.
        val legacy = context.getSharedPreferences("sg.secrets.plain", Context.MODE_PRIVATE)
        if (legacy.all.isNotEmpty()) {
            val edit = encrypted.edit()
            legacy.all.forEach { (key, value) ->
                if (value is String && !encrypted.contains(key)) edit.putString(key, value)
            }
            check(edit.commit()) { "The sign-in details could not be moved to secure storage." }
            check(legacy.edit().clear().commit()) { "The old sign-in storage could not be cleared." }
        }
        encrypted
    } catch (_: Exception) {
        null
    }

    val secureStorageAvailable: Boolean get() = secrets != null

    fun load(): List<SavedHost> = decodeChecked(config.getString(KEY, null))

    fun save(hosts: List<SavedHost>) {
        check(config.edit().putString(KEY, encode(hosts)).commit()) { "The server list could not be saved." }
    }

    /** Which secret. A host can have both — a pasted key *and* the passphrase protecting it. */
    enum class Kind(val suffix: String) {
        PASSWORD(""),
        KEY_TEXT(".key"),
    }

    fun secret(id: String, kind: Kind = Kind.PASSWORD): String? =
        secrets?.getString(id + kind.suffix, null)?.ifEmpty { null }

    fun setSecrets(id: String, secret: String?, keyText: String?) {
        if (secret == null && keyText == null) return
        val vault = checkNotNull(secrets) { "This device's secure storage is unavailable. The sign-in details were not saved." }
        val edit = vault.edit()
        fun put(account: String, value: String?) {
            if (value != null) {
                if (value.isEmpty()) edit.remove(account) else edit.putString(account, value)
            }
        }
        put(id, secret)
        put(id + Kind.KEY_TEXT.suffix, keyText)
        check(edit.commit()) { "The sign-in details could not be saved." }
    }

    /**
     * Whether the technical view was the last one showing.
     *
     * A view preference rather than a host record, but it belongs to the same store: two
     * preference files for one app is two things to keep in step for no benefit.
     */
    fun showTechnical(): Boolean = config.getBoolean(SHOW_TECHNICAL, false)

    fun setShowTechnical(value: Boolean) {
        config.edit().putBoolean(SHOW_TECHNICAL, value).apply()
    }

    /** Remove a host's stored record and its secret together. */
    fun forget(id: String) {
        save(load().filterNot { it.id == id })
        secrets?.edit()?.remove(id)?.remove(id + Kind.KEY_TEXT.suffix)?.apply()
    }

    companion object {
        private const val KEY = "hosts.v1"
        private const val SHOW_TECHNICAL = "show_technical"

        /**
         * The stored form of the host list.
         *
         * Split out from the preferences plumbing so it can be tested without an Android runtime.
         * This is the format that has to survive an app upgrade — a field silently dropped here
         * loses somebody's servers — and it had no test at all.
         */
        fun encode(hosts: List<SavedHost>): String {
            val array = JSONArray()
            hosts.forEach { host ->
                array.put(
                    JSONObject().apply {
                        put("id", host.id)
                        put("address", host.address)
                        put("port", host.port.toInt())
                        put("user", host.user)
                        put("authKind", host.authKind)
                        put("keyPath", host.keyPath ?: "")
                        put("hostKeyPolicy", host.hostKeyPolicy)
                        put("refreshMs", host.refreshMs.toLong())
                    },
                )
            }
            return array.toString()
        }

        /**
         * Anything unreadable yields an empty list rather than a crash.
         *
         * For previews and callers that only need a best-effort list. Persistence uses
         * decodeChecked so a damaged inventory is reported and cannot be silently overwritten.
         */
        fun decode(raw: String?): List<SavedHost> = runCatching { decodeChecked(raw) }.getOrDefault(emptyList())

        internal fun decodeChecked(raw: String?): List<SavedHost> {
            if (raw.isNullOrEmpty()) return emptyList()
            val array = JSONArray(raw)
            return (0 until array.length()).map { index ->
                val o = array.getJSONObject(index)
                val port = o.getInt("port")
                val refresh = o.getLong("refreshMs")
                require(port in 1..65535 && refresh > 0) { "Invalid saved connection settings" }
                SavedHost(
                    id = o.getString("id"),
                    address = o.getString("address"),
                    port = port.toUShort(),
                    user = o.getString("user"),
                    authKind = o.getString("authKind"),
                    keyPath = o.optString("keyPath").ifEmpty { null },
                    hostKeyPolicy = o.getString("hostKeyPolicy"),
                    refreshMs = refresh.toULong(),
                )
            }
        }
    }
}
