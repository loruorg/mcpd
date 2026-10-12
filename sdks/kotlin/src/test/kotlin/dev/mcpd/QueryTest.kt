package dev.mcpd

import kotlinx.coroutines.runBlocking
import kotlin.time.Instant
import kotlin.test.Test
import kotlin.test.assertContains
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * The read surface.
 *
 * The canned responses here are real answers from a running mcpd, so decoding
 * them proves the generated types match the server this SDK was built against.
 */
class QueryTest {

    private val from = Instant.parse("2026-01-01T00:00:00Z")
    private val to = Instant.parse("2026-02-01T00:00:00Z")

    @Test
    fun `listing traces sends the filters and decodes the page`() = runBlocking {
        FakeMcpd { _, _ -> 200 to TRACE_LIST }.use { server ->
            val mcpd = client(server)
            val page = mcpd.listTraces(
                status = SpanStatus.ERROR,
                tags = listOf("checkout", "beta"),
                from = from,
                to = to,
                limit = 25,
            )

            val path = server.requests.single().path
            assertContains(path, "project=test", message = "the project is the tenancy key")
            assertContains(path, "status=error", message = "enums go on the wire in their wire form")
            assertContains(path, "tags=checkout%2Cbeta", message = "list filters are comma joined")
            assertContains(path, "from=2026-01-01T00%3A00%3A00Z")
            assertContains(path, "limit=25")

            val trace = page.traces.single()
            assertEquals("b3d9e71a4c0f428eb6a5d2317f90c684", trace.traceId)
            assertEquals("support_agent", trace.name)
            assertEquals(SpanStatus.ERROR, trace.status)
            assertEquals(7, trace.spanCount)
            assertEquals(3300L, trace.usage?.inputTokens)
            assertEquals(0.0005454, assertNotNull(trace.cost?.totalCost), 1e-12)
            assertEquals(false, page.page.hasMore)
            mcpd.shutdown()
        }
    }

    @Test
    fun `fetching one trace asks for the attached signals`() = runBlocking {
        FakeMcpd { _, _ -> 200 to TRACE_DETAIL }.use { server ->
            val mcpd = client(server)
            val detail = mcpd.getTrace("b3d9e71a4c0f428eb6a5d2317f90c684", include = listOf("scores", "logs"))

            val path = server.requests.single().path
            assertContains(path, "/v1/api/traces/b3d9e71a4c0f428eb6a5d2317f90c684")
            assertContains(path, "include=scores%2Clogs")

            assertEquals(1, detail.spans.size)
            assertEquals(SpanKind.AGENT_RUN, detail.spans.single().kind)
            assertEquals("answer_relevance", assertNotNull(detail.scores).single().scorerId)
            mcpd.shutdown()
        }
    }

    @Test
    fun `analytics decode`() = runBlocking {
        FakeMcpd { _, _ -> 200 to SUMMARY }.use { server ->
            val mcpd = client(server)
            val summary = mcpd.analyticsSummary(from, to, groupBy = "model")

            assertContains(server.requests.single().path, "group_by=model")
            assertEquals(3L, summary.traceCount)
            assertEquals(0.3333333333333333, summary.errorRate, 1e-12)
            assertEquals(3180.0, assertNotNull(summary.latency.p50), 1e-9)
            assertEquals("USD", summary.currency)
            mcpd.shutdown()
        }
    }

    @Test
    fun `the feedback list sends only the parameters its endpoint documents`() = runBlocking {
        FakeMcpd { _, _ -> 200 to FEEDBACK_LIST }.use { server ->
            val mcpd = client(server)
            val page = mcpd.listFeedback("b3d9e71a4c0f428eb6a5d2317f90c684", limit = 5)

            // The daemon answers 400 to any other key, so these three are all.
            assertEquals(
                "/v1/api/feedback?project=test&trace_id=b3d9e71a4c0f428eb6a5d2317f90c684&limit=5",
                server.requests.single().path,
            )
            assertEquals(FeedbackKind.THUMBS, page.feedback.single().kind)
            assertFailsWith<IllegalArgumentException> { mcpd.listFeedback("") }
            assertEquals(1, server.requests.size, "a refused call never reaches the wire")
            mcpd.shutdown()
        }
    }

    @Test
    fun `a metric filter goes out as key value pairs`() = runBlocking {
        FakeMcpd { _, _ -> 200 to METRIC_SERIES }.use { server ->
            val mcpd = client(server)
            mcpd.queryMetrics("calls", from, to, filter = linkedMapOf("model" to "gpt-4o", "status" to "ok"))

            // The daemon reads key:value pairs joined by commas.
            assertContains(server.requests.single().path, "filter=model%3Agpt-4o%2Cstatus%3Aok")
            for (bad in listOf(mapOf("a:b" to "x"), mapOf("a,b" to "x"), mapOf("" to "x"), mapOf("model" to "a,b"))) {
                assertFailsWith<IllegalArgumentException>("$bad cannot be written as key:value pairs") {
                    mcpd.queryMetrics("calls", from, to, filter = bad)
                }
            }
            assertEquals(1, server.requests.size, "a refused filter never reaches the wire")
            mcpd.shutdown()
        }
    }

    @Test
    fun `health decodes and needs no parameters`() = runBlocking {
        FakeMcpd { _, _ -> 200 to HEALTH }.use { server ->
            val mcpd = client(server)
            val health = mcpd.health()

            assertEquals("/healthz", server.requests.single().path)
            assertEquals("ok", health.status)
            assertEquals("1.0.0", health.specVersion)
            mcpd.shutdown()
        }
    }

    @Test
    fun `a read that fails says so instead of returning nothing`() = runBlocking {
        FakeMcpd { _, _ -> 500 to """{"error":"internal"}""" }.use { server ->
            val mcpd = client(server)
            val failure = assertFailsWith<McpdException> { mcpd.health() }
            assertContains(assertNotNull(failure.message), "500")
            mcpd.shutdown()
        }
    }

    @Test
    fun `a read against an unreachable server fails loudly`() = runBlocking {
        val mcpd = client(endpoint = "http://127.0.0.1:1")
        val failure = assertFailsWith<McpdException> { mcpd.listTraces() }
        assertTrue(assertNotNull(failure.message).contains("cannot reach mcpd"))
        mcpd.shutdown()
    }

    private companion object {
        const val TRACE_LIST = """
        {"traces":[{"trace_id":"b3d9e71a4c0f428eb6a5d2317f90c684",
        "root_span_id":"1122334455667788","name":"support_agent","kind":"agent_run",
        "status":"error","start_time":"2026-01-27T22:33:00.842306347Z",
        "end_time":"2026-01-27T22:33:09.262306347Z","duration_ns":8420000000,
        "span_count":7,"error_count":1,"model_count":2,"tool_count":3,
        "usage":{"input_tokens":3300,"output_tokens":84},
        "cost":{"currency":"USD","total_cost":0.0005454,"is_estimated":true},
        "correlation":{"project":"test","environment":"production"},
        "is_complete":true,"received_at":"2026-01-27T22:33:09.842306347Z"}],
        "page":{"has_more":false}}
        """

        const val TRACE_DETAIL = """
        {"trace":{"trace_id":"b3d9e71a4c0f428eb6a5d2317f90c684","name":"support_agent",
        "kind":"agent_run","status":"error","start_time":"2026-01-27T22:33:00.842306347Z",
        "span_count":1,"error_count":1,"model_count":0,"tool_count":0,
        "correlation":{"project":"test"},"is_complete":true,
        "received_at":"2026-01-27T22:33:09.842306347Z"},
        "spans":[{"trace_id":"b3d9e71a4c0f428eb6a5d2317f90c684","span_id":"1122334455667788",
        "name":"support_agent","kind":"agent_run","status":"error",
        "start_time":"2026-01-27T22:33:00.842306347Z","is_root":true,"is_event":false,
        "correlation":{"project":"test"},"received_at":"2026-01-27T22:33:09.842306347Z",
        "error":{"message":"the tool refused"}}],
        "scores":[{"score_id":"demo-score-2","trace_id":"b3d9e71a4c0f428eb6a5d2317f90c684",
        "scorer_id":"answer_relevance","source":"live","value":0.08,"min":0.0,"max":1.0,
        "timestamp":"2026-01-27T22:33:09.842306347Z","correlation":{"project":"test"}}]}
        """

        const val SUMMARY = """
        {"from":"2026-01-01T00:00:00Z","to":"2026-02-01T00:00:00Z","trace_count":3,
        "span_count":16,"error_count":1,"error_rate":0.3333333333333333,
        "latency":{"p50":3180.0,"p90":8420.0,"p95":8420.0,"p99":8420.0,"mean":4200.0,"max":8420.0},
        "input_tokens":6186,"output_tokens":314,"total_cost":0.0073104,"currency":"USD"}
        """

        const val FEEDBACK_LIST = """
        {"feedback":[{"feedback_id":"f-1","trace_id":"b3d9e71a4c0f428eb6a5d2317f90c684",
        "kind":"thumbs","value":"up","timestamp":"2026-01-27T22:33:09.842306347Z",
        "correlation":{"project":"test"}}],"page":{"has_more":false}}
        """

        const val METRIC_SERIES = """
        {"name":"calls","bucket":"1h","series":[]}
        """

        const val HEALTH = """
        {"status":"ok","version":"0.1.0","spec_version":"1.0.0","uptime_seconds":2,"storage":"sqlite"}
        """
    }
}
