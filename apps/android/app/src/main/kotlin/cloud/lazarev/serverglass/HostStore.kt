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

    /**
     * Falls back to plain preferences if the Keystore is unavailable.
     *
     * `EncryptedSharedPreferences` can fail on devices with a broken or wiped Keystore, and on some
     * emulators. Refusing to run at all would be worse than storing a secret the way any ordinary
     * app setting is stored — but the distinction is recorded so the UI can say which happened
     * rather than implying protection it does not have.
     */
    var secretsAreHardwareBacked: Boolean = true
        private set

    private val secrets: SharedPreferences = try {
        val masterKey = MasterKey.Builder(context)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()
        EncryptedSharedPreferences.create(
            context,
            "sg.secrets",
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM,
        )
    } catch (_: Exception) {
        secretsAreHardwareBacked = false
        context.getSharedPreferences("sg.secrets.plain", Context.MODE_PRIVATE)
    }

    fun load(): List<SavedHost> = decode(config.getString(KEY, null))

    fun save(hosts: List<SavedHost>) {
        config.edit().putString(KEY, encode(hosts)).apply()
    }

    /** Which secret. A host can have both — a pasted key *and* the passphrase protecting it. */
    enum class Kind(val suffix: String) {
        PASSWORD(""),
        KEY_TEXT(".key"),
    }

    fun secret(id: String, kind: Kind = Kind.PASSWORD): String? =
        secrets.getString(id + kind.suffix, null)?.ifEmpty { null }

    fun setSecret(id: String, secret: String?, kind: Kind = Kind.PASSWORD) {
        val account = id + kind.suffix
        secrets.edit().apply {
            if (secret.isNullOrEmpty()) remove(account) else putString(account, secret)
        }.apply()
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
        secrets.edit().remove(id).remove(id + Kind.KEY_TEXT.suffix).apply()
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
         * A record written by a future version, or a file truncated by a device losing power
         * mid-write, must cost the user their list — not the ability to open the app at all.
         */
        fun decode(raw: String?): List<SavedHost> {
            if (raw.isNullOrEmpty()) return emptyList()
            return runCatching {
                val array = JSONArray(raw)
                (0 until array.length()).map { index ->
                    val o = array.getJSONObject(index)
                    SavedHost(
                        id = o.getString("id"),
                        address = o.getString("address"),
                        port = o.getInt("port").toUShort(),
                        user = o.getString("user"),
                        authKind = o.getString("authKind"),
                        keyPath = o.optString("keyPath").ifEmpty { null },
                        hostKeyPolicy = o.getString("hostKeyPolicy"),
                        refreshMs = o.getLong("refreshMs").toULong(),
                    )
                }
            }.getOrDefault(emptyList())
        }
    }
}
