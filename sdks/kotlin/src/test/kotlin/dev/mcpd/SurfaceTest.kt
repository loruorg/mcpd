package dev.mcpd

import kotlinx.coroutines.runBlocking
import kotlin.time.Instant
import kotlin.time.Duration.Companion.milliseconds
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Payload capture, recorded spans, the external parent, sinks and stats
 * (contract sections 2.1 to 2.3 and 3).
 *
 * Every assertion reads what reached the wire or the sink, decoded with the
 * generated types, so a field that never left the process fails the test.
 */
class SurfaceTest {

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
    fun `capture off keeps input and output in the process and the rest of the span survives`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server) { it.copy(capturePayloads = false) }
            val span = mcpd.trace("run", input = "the prompt", model = "gpt-4o", provider = "openai", tags = listOf("t"))
            span.setInput("the prompt again")
            span.setOutput("the completion")
            span.setUsage(inputTokens = 3, outputTokens = 1)
            span.setModelParameters(temperature = 0.0, seed = 0, stopSequences = listOf("STOP"))
            span.event("chunk", input = "delta in", output = "delta out", attributes = mapOf("index" to 1))
            span.end()
            mcpd.flush()

            val spans = assertNotNull(server.requests.single().batch().spans)
            assertEquals(2, spans.size)
            for (s in spans) {
                assertNull(s.input, "${s.name} carried its input off the process")
                assertNull(s.output, "${s.name} carried its output off the process")
            }
            val run = spans.first { it.name == "run" }
            assertEquals("gpt-4o", run.model)
            assertEquals("openai", run.provider)
            assertEquals(3, run.usage?.inputTokens)
            // Sampling settings are metadata, not payload: no prompt is in
            // them, and they are what the run has to be reproduced from.
            // Capture off must not take them, and a zero seed must survive.
            assertEquals(0L, run.modelParameters?.seed)
            assertEquals(0.0, run.modelParameters?.temperature)
            assertEquals(listOf("STOP"), run.modelParameters?.stopSequences)
            assertFalse(assertNotNull(run.modelParameters).hasResponseFormat)
            assertEquals(listOf("t"), run.tags)
            assertEquals(SpanStatus.OK, run.status)
            assertNotNull(run.endTime)
            val chunk = spans.first { it.name == "chunk" }
            assertTrue(chunk.isEvent)
            assertEquals("1", chunk.attributes?.jsonObject?.get("index")?.jsonPrimitive?.content)
            mcpd.shutdown()
        }
    }

    @Test
    fun `MCPD_CAPTURE_PAYLOADS is read and code wins over it`() {
        withEnvironment(mapOf("MCPD_CAPTURE_PAYLOADS" to "false")) {
            assertFalse(McpdConfig().capturePayloads)
            assertTrue(McpdConfig(capturePayloads = true).capturePayloads)
        }
        withEnvironment(mapOf("MCPD_CAPTURE_PAYLOADS" to "off")) { assertFalse(McpdConfig().capturePayloads) }
        withEnvironment(mapOf("MCPD_CAPTURE_PAYLOADS" to "0")) { assertFalse(McpdConfig().capturePayloads) }
        withEnvironment(emptyMap()) { assertTrue(McpdConfig().capturePayloads) }
    }

    @Test
    fun `a span overrides the client for itself in either direction`() = runBlocking {
        FakeMcpd().use { server ->
            val quiet = client(server) { it.copy(capturePayloads = false) }
            val kept = quiet.trace("kept", input = "shown", capturePayloads = true)
            kept.setOutput("shown too")
            // The override is the span's own: a child follows the client again.
            kept.child("child", input = "hidden").end()
            kept.end()
            quiet.flush()
            quiet.shutdown()

            val loud = client(server)
            val hidden = loud.trace("hidden", input = "hidden", capturePayloads = false)
            hidden.setOutput("hidden too")
            hidden.child("child", input = "shown", capturePayloads = null).end()
            hidden.end()
            loud.flush()
            loud.shutdown()

            val spans = server.requests.flatMap { it.batch().spans ?: emptyList() }.associateBy { "${it.correlation.project}/${it.name}/${it.input?.jsonPrimitive?.content}" }
            val byName = server.requests.flatMap { it.batch().spans ?: emptyList() }
            assertEquals(4, byName.size)
            val keptRoot = byName.first { it.name == "kept" }
            assertEquals("shown", keptRoot.input?.jsonPrimitive?.content)
            assertEquals("shown too", keptRoot.output?.jsonPrimitive?.content)
            assertNull(byName.first { it.parentSpanId == keptRoot.spanId }.input)
            val hiddenRoot = byName.first { it.name == "hidden" }
            assertNull(hiddenRoot.input)
            assertNull(hiddenRoot.output)
            assertEquals("shown", byName.first { it.parentSpanId == hiddenRoot.spanId }.input?.jsonPrimitive?.content)
            assertTrue(spans.isNotEmpty())
        }
    }

    @Test
    fun `an external parent reaches the wire and never parents the tree`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            val root = mcpd.trace("root", externalParentSpanId = "0011223344556677")
            root.child("child", externalParentSpanId = "8899aabbccddeeff").end()
            mcpd.span("nested", externalParentSpanId = "1122334455667788") { }
            root.end()
            mcpd.flush()

            val spans = assertNotNull(server.requests.single().batch().spans).associateBy { it.name }
            assertEquals("0011223344556677", spans["root"]?.externalParentSpanId)
            assertNull(spans["root"]?.parentSpanId)
            assertTrue(spans["root"]!!.isRoot)
            assertEquals("8899aabbccddeeff", spans["child"]?.externalParentSpanId)
            assertEquals(root.spanId, spans["child"]?.parentSpanId)
            assertEquals("1122334455667788", spans["nested"]?.externalParentSpanId)
            mcpd.shutdown()
        }
    }

    private val traceId = "0123456789abcdef0123456789abcdef"

    private fun finished(
        name: String = "imported",
        trace: String = traceId,
        spanId: String = "0123456789abcdef",
        parent: String? = "fedcba9876543210",
        correlation: Correlation = Correlation(project = ""),
    ) = Span(
        traceId = trace,
        spanId = spanId,
        name = name,
        kind = SpanKind.TOOL_CALL,
        status = SpanStatus.OK,
        startTime = Instant.parse("2026-09-01T10:00:00Z"),
        isRoot = parent == null,
        isEvent = false,
        correlation = correlation,
        receivedAt = Instant.parse("2026-09-01T10:00:02Z"),
        parentSpanId = parent,
        endTime = Instant.parse("2026-09-01T10:00:01.5Z"),
        durationNs = 1_500_000_000,
        input = kotlinx.serialization.json.JsonPrimitive("the prompt"),
        output = kotlinx.serialization.json.JsonPrimitive("the answer"),
    )

    @Test
    fun `record queues a finished span as given and fills only the empty correlation`() = runBlocking {
        FakeMcpd().use { server ->
            // sample = 0.0: a recorded span is never re-sampled, so it still lands.
            val mcpd = client(server) { it.copy(sample = 0.0) }
            mcpd.record(finished(correlation = Correlation(project = "", sessionId = "s-9")))
            mcpd.flush()

            val span = assertNotNull(server.requests.single().batch().spans).single()
            assertEquals(traceId, span.traceId)
            assertEquals("0123456789abcdef", span.spanId)
            assertEquals("fedcba9876543210", span.parentSpanId)
            assertEquals(Instant.parse("2026-09-01T10:00:00Z"), span.startTime)
            assertEquals(Instant.parse("2026-09-01T10:00:01.5Z"), span.endTime)
            assertEquals(1_500_000_000, span.durationNs)
            assertEquals("the prompt", span.input?.jsonPrimitive?.content)
            assertEquals("test", span.correlation.project, "an empty project is filled from the client")
            assertEquals("test", span.correlation.environment)
            assertEquals("sdk-test", span.correlation.serviceName)
            assertEquals("s-9", span.correlation.sessionId, "a field the record set is kept")
            val stats = mcpd.stats()
            assertEquals(1, stats.exported)
            assertEquals(0, stats.rejected)
            mcpd.shutdown()
        }
    }

    @Test
    fun `record refuses each malformed shape by counting rather than raising`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            mcpd.record(finished(trace = "0123456789ABCDEF0123456789abcdef"))
            mcpd.record(finished(trace = "0".repeat(32)))
            mcpd.record(finished(spanId = "0123456789abcde"))
            mcpd.record(finished(spanId = "0".repeat(16)))
            mcpd.record(finished(parent = "not-a-span-id"))
            mcpd.record(finished(name = "   "))
            mcpd.flush()

            assertTrue(server.requests.isEmpty(), "nothing malformed reached the wire")
            val stats = mcpd.stats()
            assertEquals(6, stats.rejected)
            assertEquals(0, stats.exported)
            assertEquals(0, stats.queued)

            // A well-formed one still goes, stripped when capture is off.
            mcpd.record(finished(parent = null))
            mcpd.flush()
            assertEquals(1, server.requests.size)
            mcpd.shutdown()

            val quiet = client(server) { it.copy(capturePayloads = false) }
            quiet.record(finished())
            quiet.flush()
            val stripped = assertNotNull(server.requests.last().batch().spans).single()
            assertNull(stripped.input)
            assertNull(stripped.output)
            assertEquals("imported", stripped.name)
            quiet.shutdown()

            val off = client(server) { it.copy(enabled = false) }
            off.record(finished())
            off.flush()
            assertEquals(2, server.requests.size, "a disabled client drops the record")
            assertEquals(0, off.stats().rejected)
            off.shutdown()
        }
    }

    @Test
    fun `a RecordingSink sees exactly the signals recorded`() = runBlocking {
        val recorder = RecordingSink()
        val mcpd = client { it.copy(sink = recorder) }
        val traceId = mcpd.span("run", input = "q") { span ->
            span.setOutput("a")
            mcpd.log(LogLevel.INFO, "hello", mapOf("n" to 1))
            mcpd.metric("latency_ms", 12.0, MetricKind.GAUGE)
            span.traceId
        }
        mcpd.score(traceId, scorerId = "relevance", value = 0.9, min = 0.0, max = 1.0)
        mcpd.feedback(traceId, FeedbackKind.THUMBS, "up")
        mcpd.flush()

        assertEquals(listOf("run"), recorder.spans.map { it.name })
        assertEquals("a", recorder.spans.single().output?.jsonPrimitive?.content)
        assertEquals(listOf("hello"), recorder.logs.map { it.message })
        assertEquals(traceId, recorder.logs.single().traceId)
        assertEquals(listOf("latency_ms"), recorder.metrics.map { it.name })
        assertEquals(listOf("relevance"), recorder.scores.map { it.scorerId })
        assertEquals(listOf(FeedbackKind.THUMBS), recorder.feedback.map { it.kind })
        assertEquals(1, recorder.batches)

        val stats = mcpd.stats()
        assertEquals(5, stats.exported)
        assertEquals(0, stats.failed)
        assertEquals(0, stats.queued)

        recorder.clear()
        assertEquals(0, recorder.batches)
        assertTrue(recorder.spans.isEmpty() && recorder.logs.isEmpty())
        mcpd.shutdown()
    }

    @Test
    fun `a sink that throws is a failed batch the caller never sees`() = runBlocking {
        val refusing = Sink { _, _ -> throw IllegalStateException("no room") }
        val mcpd = client { it.copy(sink = refusing) }
        mcpd.trace("doomed").end()
        mcpd.flush()
        mcpd.trace("doomed again").end()
        mcpd.shutdown()

        val stats = mcpd.stats()
        assertEquals(2, stats.failed)
        assertEquals(0, stats.exported)
        assertEquals(0, stats.queued)
    }

    @Test
    fun `flush and shutdown return by their deadline when the sink never answers`() = runBlocking {
        // Against a black-holed daemon every batch costs three attempts of the
        // full request timeout. A bound on batches is no bound on time.
        val calls = java.util.concurrent.atomic.AtomicInteger()
        val slow = Sink { _, _ ->
            calls.incrementAndGet()
            kotlinx.coroutines.delay(400)
        }
        val mcpd = client { it.copy(sink = slow, batchSize = 1) }
        repeat(5) { mcpd.log(LogLevel.INFO, "line $it") }

        val started = System.nanoTime()
        assertFalse(mcpd.flush(50.milliseconds), "a flush that ran out of time says so")
        mcpd.shutdown(50.milliseconds)
        val took = (System.nanoTime() - started) / 1_000_000
        assertTrue(took < 300, "flush and shutdown took ${took}ms")

        // The batch in flight finishes and is counted; no further batch starts.
        kotlinx.coroutines.delay(1_000)
        assertEquals(1, calls.get())
        assertEquals(1, mcpd.stats().exported)
        assertEquals(4, mcpd.stats().queued, "what was given up stays visible as queued")
    }

    @Test
    fun `a ConsoleSink writes one JSON line per signal with its kind in front`() = runBlocking {
        val lines = CopyOnWriteArrayList<String>()
        val mcpd = client { it.copy(sink = ConsoleSink { lines.add(it) }) }
        mcpd.span("run") { mcpd.log(LogLevel.WARN, "careful") }
        mcpd.flush()
        mcpd.shutdown()

        assertEquals(2, lines.size)
        val span = Json.parseToJsonElement(lines[0]).jsonObject
        assertEquals("span", span["signal"]?.jsonPrimitive?.content)
        assertEquals("run", span["name"]?.jsonPrimitive?.content)
        assertNotNull(span["trace_id"], "the wire spelling, not the Kotlin one")
        val log = Json.parseToJsonElement(lines[1]).jsonObject
        assertEquals("log", log["signal"]?.jsonPrimitive?.content)
        assertEquals("careful", log["message"]?.jsonPrimitive?.content)
    }

    @Test
    fun `stats answer the queue the sink and record`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server) { it.copy(maxQueue = 2) }
            val stats0 = mcpd.stats()
            assertEquals(Stats(queued = 0, dropped = 0, capacity = 2, exported = 0, failed = 0, rejected = 0, redactionFailures = 0), stats0)
            repeat(3) { mcpd.trace("t$it").end() }
            assertEquals(2, mcpd.stats().queued)
            assertEquals(1, mcpd.stats().dropped)
            mcpd.flush()
            assertEquals(2, mcpd.stats().exported)
            assertEquals(0, mcpd.stats().queued)
            mcpd.shutdown()
        }
    }
}
