package dev.mcpd

import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlin.test.Test
import kotlin.test.assertContains
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * The logging signal of contract section 4: correlation that survives an
 * absent override, a throwable serialised rather than flattened, the level
 * floor, and the OpenTelemetry severity mapping.
 */
class LoggingTest {

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
    fun `a null correlation override keeps the run's own`() = runBlocking {
        val recorder = RecordingSink()
        val mcpd = client { it.copy(sink = recorder) }
        val run = mcpd.trace("run", sessionId = "session-7", userId = "u-1")
        // Named with the absent value. Not an instruction to erase the run's.
        val step = mcpd.newSpan(
            name = "step",
            kind = SpanKind.GENERIC,
            parent = run,
            remote = null,
            isEvent = false,
            input = null,
            attributes = null,
            metadata = null,
            tags = null,
            model = null,
            provider = null,
            entityKind = null,
            entityId = null,
            entityName = null,
            sessionId = null,
        )
        mcpd.recordLog(LogLevel.INFO, "inside", null, step)
        mcpd.recordMetric("latency_ms", 4.0, MetricKind.GAUGE, null, null, step)
        step.end()
        run.end()
        mcpd.flush()

        assertEquals("session-7", recorder.logs.single().correlation.sessionId)
        assertEquals("u-1", recorder.logs.single().correlation.userId)
        assertEquals("session-7", recorder.metrics.single().correlation.sessionId)
        mcpd.shutdown()
    }

    @Test
    fun `a throwable under a field key arrives as type message and stack`() = runBlocking {
        val recorder = RecordingSink()
        val mcpd = client { it.copy(sink = recorder) }
        val thrown = runCatching { error("boom") }.exceptionOrNull()!!
        mcpd.log(LogLevel.ERROR, "the call failed", mapOf("error" to thrown, "attempt" to 2))
        mcpd.metric(
            "failures",
            1.0,
            MetricKind.COUNTER,
            labels = mapOf("cause" to IllegalArgumentException("label")),
        )
        mcpd.flush()

        val fields = assertNotNull(recorder.logs.single().fields).jsonObject
        assertEquals(2, fields["attempt"]?.jsonPrimitive?.content?.toInt())
        val described = assertNotNull(fields["error"]).jsonObject
        assertEquals("IllegalStateException", described["type"]?.jsonPrimitive?.content)
        assertEquals("boom", described["message"]?.jsonPrimitive?.content)
        assertContains(
            assertNotNull(described["stack"]).jsonPrimitive.content,
            "IllegalStateException",
        )
        val labels = assertNotNull(recorder.metrics.single().labels).jsonObject
        assertEquals("label", labels["cause"]?.jsonObject?.get("message")?.jsonPrimitive?.content)
        mcpd.shutdown()
    }

    @Test
    fun `a number JSON cannot carry costs itself and never the batch`() = runBlocking {
        // The encoder refuses special floats, and one of them anywhere used to
        // lose every signal in the batch it travelled in.
        FakeMcpd().use { server ->
            val mcpd = client(server)
            mcpd.span("run") { span ->
                span.setInput(Double.NaN)
                span.setMetadata(mapOf("ratio" to Double.POSITIVE_INFINITY))
                span.setModelParameters(temperature = Double.NaN, topP = 0.9)
                mcpd.log(LogLevel.INFO, "measured", mapOf("ratio" to Double.NEGATIVE_INFINITY))
                mcpd.metric("ratio", Double.NaN)
                mcpd.metric("calls", 1.0)
                span.score("judge", Double.NaN, 0.0, 1.0)
            }
            assertTrue(mcpd.flush())

            val batches = server.requests.map { it.batch() }
            val run = batches.flatMap { it.spans.orEmpty() }.last { it.name == "run" }
            assertEquals("NaN", run.input?.jsonPrimitive?.content)
            assertEquals("Infinity", run.metadata?.jsonObject?.get("ratio")?.jsonPrimitive?.content)
            assertNull(run.modelParameters?.temperature, "a setting JSON cannot carry is left out")
            assertEquals(0.9, run.modelParameters?.topP)
            val fields = assertNotNull(batches.flatMap { it.logs.orEmpty() }.single().fields).jsonObject
            assertEquals("-Infinity", fields["ratio"]?.jsonPrimitive?.content)
            assertEquals(listOf("calls"), batches.flatMap { it.metrics.orEmpty() }.map { it.name })
            assertTrue(batches.flatMap { it.scores.orEmpty() }.isEmpty(), "the score is what cannot be sent")
            assertEquals(0, mcpd.stats().failed)
            mcpd.shutdown()
        }
    }

    @Test
    fun `a throwable nested in a list or a map is serialised too`() = runBlocking {
        val recorder = RecordingSink()
        val mcpd = client { it.copy(sink = recorder) }
        mcpd.log(
            LogLevel.WARN,
            "retries exhausted",
            mapOf("attempts" to listOf(IllegalStateException("first"))),
        )
        mcpd.flush()

        val fields = assertNotNull(recorder.logs.single().fields).jsonObject
        val first = assertNotNull(fields["attempts"]).jsonArray.first().jsonObject
        assertEquals("first", first["message"]?.jsonPrimitive?.content)
        assertEquals("IllegalStateException", first["type"]?.jsonPrimitive?.content)
        mcpd.shutdown()
    }

    @Test
    fun `the floor drops trace by default and keeps debug`() = runBlocking {
        val recorder = RecordingSink()
        val mcpd = client { it.copy(sink = recorder) }
        assertEquals(LogLevel.DEBUG, mcpd.config.logLevel)
        mcpd.log(LogLevel.TRACE, "finer than debug")
        mcpd.log(LogLevel.DEBUG, "kept")
        mcpd.flush()

        assertEquals(listOf("kept"), recorder.logs.map { it.message })
        mcpd.shutdown()
    }

    @Test
    fun `the floor drops everything below it and queues nothing`() = runBlocking {
        val recorder = RecordingSink()
        val mcpd = client { it.copy(sink = recorder, logLevel = LogLevel.WARN) }
        LogLevel.entries.forEach { mcpd.log(it, it.wire) }
        mcpd.flush()

        assertEquals(listOf("warn", "error", "fatal"), recorder.logs.map { it.message })
        assertEquals(0, mcpd.stats().dropped)
        mcpd.shutdown()
    }

    @Test
    fun `the floor comes from the environment and code wins`() {
        withEnvironment(mapOf("MCPD_LOG_LEVEL" to "ERROR")) {
            assertEquals(LogLevel.ERROR, McpdConfig().logLevel)
            assertEquals(LogLevel.TRACE, McpdConfig(logLevel = LogLevel.TRACE).logLevel)
        }
        withEnvironment(mapOf("MCPD_LOG_LEVEL" to "verbose")) {
            assertEquals(LogLevel.DEBUG, McpdConfig().logLevel)
        }
    }

    @Test
    fun `the floor never filters the other four signals`() = runBlocking {
        val recorder = RecordingSink()
        val mcpd = client { it.copy(sink = recorder, logLevel = LogLevel.FATAL) }
        val traceId = mcpd.span("run") { span ->
            mcpd.metric("latency_ms", 3.0, MetricKind.GAUGE)
            span.traceId
        }
        mcpd.score(traceId, scorerId = "judge", value = 1.0, min = 0.0, max = 1.0)
        mcpd.feedback(traceId, FeedbackKind.THUMBS, "up")
        mcpd.flush()

        assertEquals(1, recorder.spans.size)
        assertEquals(1, recorder.metrics.size)
        assertEquals(1, recorder.scores.size)
        assertEquals(1, recorder.feedback.size)
        assertTrue(recorder.logs.isEmpty())
        mcpd.shutdown()
    }

    @Test
    fun `severity numbers map both ways`() {
        assertEquals(listOf(1, 5, 9, 13, 17, 21), LogLevel.entries.map { severityNumber(it) })
        val ranges = listOf(
            Triple(1, 4, LogLevel.TRACE),
            Triple(5, 8, LogLevel.DEBUG),
            Triple(9, 12, LogLevel.INFO),
            Triple(13, 16, LogLevel.WARN),
            Triple(17, 20, LogLevel.ERROR),
            Triple(21, 24, LogLevel.FATAL),
        )
        for ((from, to, level) in ranges) {
            for (n in from..to) assertEquals(level, logLevelFromSeverity(n), "severity $n")
        }
        LogLevel.entries.forEach {
            assertEquals(it, logLevelFromSeverity(severityNumber(it)))
        }
        assertEquals(LogLevel.TRACE, logLevelFromSeverity(0))
        assertEquals(LogLevel.TRACE, logLevelFromSeverity(-3))
        assertEquals(LogLevel.FATAL, logLevelFromSeverity(40))
    }

    @Test
    fun `a name outside the enum parses to null`() {
        assertEquals(LogLevel.WARN, parseLogLevel("WARN"))
        assertEquals(LogLevel.ERROR, parseLogLevel(" error "))
        assertNull(parseLogLevel("verbose"))
        assertNull(parseLogLevel(null))
    }
}
