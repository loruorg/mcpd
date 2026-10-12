package dev.mcpd

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds

/**
 * Every field reads its variable, in the unit the name says, and code wins.
 *
 * The names are fixed by the contract and identical in every SDK, so a value
 * copied from a service written in another language keeps working here. The
 * JVM cannot set its own environment, so the lookup is answered by the test.
 */
class EnvironmentTest {

    private val variables = mapOf(
        "MCPD_ENDPOINT" to "http://from-env:4317",
        "MCPD_PROJECT" to "from-env",
        "MCPD_SAMPLE" to "0.25",
        "MCPD_ENABLED" to "false",
        "MCPD_BATCH_SIZE" to "7",
        "MCPD_FLUSH_INTERVAL_MS" to "150",
        "MCPD_MAX_QUEUE" to "33",
        "MCPD_TIMEOUT_MS" to "900",
        "MCPD_CAPTURE_PAYLOADS" to "false",
    )

    private fun <T> withEnvironment(values: Map<String, String>, body: () -> T): T {
        val previous = Environment.lookup
        Environment.lookup = { values[it] }
        try {
            return body()
        } finally {
            Environment.lookup = previous
        }
    }

    @Test
    fun `the environment configures the client`() = withEnvironment(variables) {
        val config = McpdConfig()
        assertEquals("http://from-env:4317", config.endpoint)
        assertEquals("from-env", config.project)
        assertEquals(0.25, config.sample)
        assertFalse(config.enabled)
        assertEquals(7, config.batchSize)
        // Whole milliseconds in the environment, so no SDK has to parse a
        // duration string; a Duration in code.
        assertEquals(150.milliseconds, config.flushInterval)
        assertEquals(33, config.maxQueue)
        assertEquals(900.milliseconds, config.timeout)
        assertFalse(config.capturePayloads)
    }

    @Test
    fun `code wins over the environment`() = withEnvironment(variables) {
        val config = McpdConfig(
            endpoint = "http://from-code:4317",
            project = "from-code",
            sample = 0.5,
            enabled = true,
            batchSize = 8,
            flushInterval = 160.milliseconds,
            maxQueue = 34,
            timeout = 901.milliseconds,
            capturePayloads = true,
        )
        assertEquals("http://from-code:4317", config.endpoint)
        assertEquals("from-code", config.project)
        assertEquals(0.5, config.sample)
        assertEquals(true, config.enabled)
        assertEquals(8, config.batchSize)
        assertEquals(160.milliseconds, config.flushInterval)
        assertEquals(34, config.maxQueue)
        assertEquals(901.milliseconds, config.timeout)
        assertEquals(true, config.capturePayloads)
    }

    @Test
    fun `a tuning variable that is not a whole positive number is ignored`() =
        withEnvironment(mapOf("MCPD_BATCH_SIZE" to "lots", "MCPD_MAX_QUEUE" to "0", "MCPD_TIMEOUT_MS" to "2.5")) {
            val config = McpdConfig()
            assertEquals(128, config.batchSize)
            assertEquals(8192, config.maxQueue)
            assertEquals(10.seconds, config.timeout)
        }
}
