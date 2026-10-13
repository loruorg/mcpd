package dev.mcpd

import dev.mcpd.testing.TestClient
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

/** The test client (contract 2.5). */
class TestClientTest {

    private suspend fun answer(mcpd: Mcpd) {
        mcpd.span("answer") { run ->
            mcpd.span("lookup", SpanKind.TOOL_CALL) { "ok" }
            mcpd.span("generate", SpanKind.MODEL_GENERATION) { it.setOutput("it ships tomorrow") }
            mcpd.log(LogLevel.INFO, "answered", mapOf("order" to "A-1"))
            mcpd.metric("answers", 1.0, MetricKind.COUNTER)
            mcpd.score(traceId = run.traceId, scorerId = "relevance", value = 0.9, min = 0.0, max = 1.0)
            mcpd.feedback(traceId = run.traceId, kind = FeedbackKind.THUMBS, value = 1)
            run.setOutput("done")
        }
    }

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
    fun `reads back every signal with no flush`() = runBlocking {
        TestClient().use { test ->
            answer(test.client)
            assertEquals(listOf("answer", "lookup", "generate"), test.spans().map { it.name })
            assertEquals(listOf("answered"), test.logs().map { it.message })
            assertEquals(listOf("answers"), test.metrics().map { it.name })
            assertEquals(listOf("relevance"), test.scores().map { it.scorerId })
            assertEquals(listOf(FeedbackKind.THUMBS), test.recordedFeedback().map { it.kind })
        }
    }

    @Test
    fun `keeps one record per span id, finished spans only`() = runBlocking {
        TestClient(McpdConfig(batchSize = 1)).use { test ->
            val open = test.client.trace("still_running")
            // Flushed while open, so its start record reaches the sink before its end record.
            test.spans()
            answer(test.client)
            open.end()
            val spans = test.spans()
            assertTrue(test.recorder.spans.size > spans.size, "the sink saw the start and the end of still_running")
            assertEquals(spans.size, spans.map { it.spanId }.toSet().size)
            assertEquals(listOf("still_running", "answer", "lookup", "generate"), spans.map { it.name })
        }
    }

    @Test
    fun `leaves out a span that has not ended`() = runBlocking {
        TestClient().use { test ->
            val open = test.client.trace("still_running")
            answer(test.client)
            assertTrue("still_running" !in test.spans().map { it.name })
            open.end()
            assertTrue("still_running" in test.spans().map { it.name })
        }
    }

    @Test
    fun `filters by name and kind`() = runBlocking {
        TestClient().use { test ->
            answer(test.client)
            assertEquals(listOf("lookup"), test.spans(kind = SpanKind.TOOL_CALL).map { it.name })
            assertEquals(JsonPrimitive("it ships tomorrow"), test.spans(name = "generate").single().output)
        }
    }

    @Test
    fun `span fails on none and on two, with the count`() = runBlocking {
        TestClient().use { test ->
            answer(test.client)
            val none = assertFailsWith<AssertionError> { test.span("missing") }
            assertTrue("found 0" in none.message!!, none.message)
            assertTrue("answer, lookup, generate" in none.message!!, none.message)
            answer(test.client)
            val two = assertFailsWith<AssertionError> { test.span("lookup") }
            assertTrue("found 2" in two.message!!, two.message)
        }
    }

    @Test
    fun `builds the tree, children by start time`() = runBlocking {
        TestClient().use { test ->
            answer(test.client)
            val roots = test.trees()
            assertEquals(1, roots.size)
            assertEquals("answer", roots[0].span.name)
            assertEquals(listOf("lookup", "generate"), roots[0].children.map { it.span.name })
            assertTrue(roots[0].children.all { it.children.isEmpty() })
        }
    }

    @Test
    fun `orders children by start time, not by arrival`() = runBlocking {
        val trace = "0123456789abcdef0123456789abcdef"
        fun at(seconds: Int, name: String, id: String, parent: String?) = Span(
            traceId = trace,
            spanId = id,
            name = name,
            kind = SpanKind.GENERIC,
            status = SpanStatus.OK,
            startTime = kotlin.time.Instant.fromEpochSeconds(1_800_000_000L + seconds),
            isRoot = parent == null,
            isEvent = false,
            correlation = Correlation(project = ""),
            receivedAt = kotlin.time.Instant.fromEpochSeconds(1_800_000_100L),
            parentSpanId = parent,
            endTime = kotlin.time.Instant.fromEpochSeconds(1_800_000_050L),
        )
        TestClient().use { test ->
            test.client.record(at(3, "late", "0000000000000003", "0000000000000001"))
            test.client.record(at(2, "early", "0000000000000002", "0000000000000001"))
            test.client.record(at(1, "root", "0000000000000001", null))
            val roots = test.trees()
            assertEquals(listOf("root"), roots.map { it.span.name })
            assertEquals(listOf("early", "late"), roots[0].children.map { it.span.name })
        }
    }

    @Test
    fun `a span whose parent was not recorded is a root`() = runBlocking {
        TestClient().use { test ->
            val parent = test.client.trace("parent")
            parent.child("orphan").end()
            assertEquals(listOf("orphan"), test.trees().map { it.span.name })
            parent.end()
            val roots = test.trees()
            assertEquals(listOf("parent"), roots.map { it.span.name })
            assertEquals(listOf("orphan"), roots[0].children.map { it.span.name })
        }
    }

    @Test
    fun `reset forgets everything`() = runBlocking {
        TestClient().use { test ->
            answer(test.client)
            test.reset()
            assertEquals(emptyList(), test.spans())
            assertEquals(emptyList(), test.logs())
            assertEquals(emptyList(), test.scores())
        }
    }

    @Test
    fun `goes through the same redaction as a real client`() = runBlocking {
        val redact = { v: kotlinx.serialization.json.JsonElement ->
            if (v is JsonPrimitive && v.isString) JsonPrimitive(v.content.replace("tomorrow", "[date]")) else v
        }
        TestClient(McpdConfig(redact = redact)).use { test ->
            answer(test.client)
            assertEquals("it ships [date]", test.span("generate").output!!.jsonPrimitive.content)
        }
    }

    @Test
    fun `stays on when the environment turns mcpd off`() = runBlocking {
        val test = withEnvironment(mapOf("MCPD_ENABLED" to "false", "MCPD_SAMPLE" to "0", "MCPD_PROJECT" to "from-env")) {
            TestClient()
        }
        test.use {
            assertTrue(test.client.config.enabled)
            assertEquals(1.0, test.client.config.sample)
            // Every other option still takes the client's own default.
            assertEquals("from-env", test.client.config.project)
            answer(test.client)
            assertEquals(3, test.spans().size)
        }
    }

    @Test
    fun `ignores a sink in the configuration`() = runBlocking {
        val other = RecordingSink()
        TestClient(McpdConfig(sink = other)).use { test ->
            answer(test.client)
            assertEquals(3, test.spans().size)
            assertEquals(0, other.batches)
        }
    }
}
