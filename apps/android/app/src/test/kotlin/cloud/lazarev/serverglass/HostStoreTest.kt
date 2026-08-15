package cloud.lazarev.serverglass

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The stored form of a host list.
 *
 * This is the format that has to survive an app upgrade — a field silently dropped here loses
 * somebody's servers — and the Kotlin layer had no tests at all. The bugs that reached a device
 * (hosts vanishing on relaunch, an edit form presenting itself as an add form) all lived here or
 * next to it, and all were found by a person driving an emulator.
 *
 * Runs on the JVM: a test that needs an emulator is a test that does not run.
 */
class HostStoreTest {

    private fun sample(id: String = "abc-123") = HostStore.SavedHost(
        id = id,
        address = "10.0.0.9",
        port = 2222u,
        user = "root",
        authKind = "key_text",
        keyPath = null,
        hostKeyPolicy = "accept_new",
        refreshMs = 1500u,
    )

    @Test
    fun `a saved host survives being written and read back`() {
        val host = sample()
        val back = HostStore.decode(HostStore.encode(listOf(host)))

        assertEquals(1, back.size)
        // Every field, not just the address: a port or a refresh interval quietly reset to its
        // default is the kind of loss nobody notices until the host stops answering.
        assertEquals(host, back.first())
    }

    @Test
    fun `an absent key path stays absent rather than becoming an empty string`() {
        // It is stored as "" because JSON has no gap, but an empty path handed to the transport
        // would be a path — and would fail as "could not read key ''" rather than as no key.
        val back = HostStore.decode(HostStore.encode(listOf(sample())))
        assertNull(back.first().keyPath)
    }

    @Test
    fun `a key path is preserved when there is one`() {
        val host = sample().copy(authKind = "key", keyPath = "/data/local/tmp/id_test")
        assertEquals("/data/local/tmp/id_test", HostStore.decode(HostStore.encode(listOf(host))).first().keyPath)
    }

    @Test
    fun `no secret is written into the record`() {
        // The record is what a backup copies. A password in it is a password in the clear.
        val json = HostStore.encode(listOf(sample()))
        assertFalse(json.contains("secret"))
        assertFalse(json.contains("password"))
    }

    @Test
    fun `unreadable storage costs the list rather than the app`() {
        // A record written by a future version, or a file truncated by a device losing power
        // mid-write, must not stop the app opening.
        assertTrue(HostStore.decode("this is not json").isEmpty())
        assertTrue(HostStore.decode("").isEmpty())
        assertTrue(HostStore.decode(null).isEmpty())
        assertTrue(HostStore.decode("[{\"id\":\"only-an-id\"}]").isEmpty())
    }

    @Test
    fun `several hosts keep their order`() {
        // The list is the sidebar's order; shuffling it on every launch would be its own bug.
        val hosts = listOf(sample("one"), sample("two"), sample("three"))
        assertEquals(listOf("one", "two", "three"), HostStore.decode(HostStore.encode(hosts)).map { it.id })
    }

    @Test
    fun `a high port survives the trip through a signed integer`() {
        // Ports are unsigned and JSON's number is not: anything above 32767 is where a naive
        // round trip turns 60000 into a negative number.
        val host = sample().copy(port = 60000u)
        assertEquals(60000u.toUShort(), HostStore.decode(HostStore.encode(listOf(host))).first().port)
    }
}
