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

    fun load(): List<SavedHost> {
        val raw = config.getString(KEY, null) ?: return emptyList()
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

    fun save(hosts: List<SavedHost>) {
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
        config.edit().putString(KEY, array.toString()).apply()
    }

    fun secret(id: String): String? = secrets.getString(id, null)?.ifEmpty { null }

    fun setSecret(id: String, secret: String?) {
        secrets.edit().apply {
            if (secret.isNullOrEmpty()) remove(id) else putString(id, secret)
        }.apply()
    }

    /** Remove a host's stored record and its secret together. */
    fun forget(id: String) {
        save(load().filterNot { it.id == id })
        secrets.edit().remove(id).apply()
    }

    private companion object {
        const val KEY = "hosts.v1"
    }
}
