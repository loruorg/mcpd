package dev.mcpd

import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.jsonPrimitive
import kotlin.test.Test
import kotlin.test.assertContains
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * What actually reaches the server.
 *
 * Every assertion here reads the bytes the SDK sent, decoded with the generated
 * types, so a field that never made it onto the wire fails the test.
 */
class ExportTest {

    @Test
    fun `spans are batched into one request`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            repeat(3) { i ->
                val span = mcpd.trace("run-$i", input = mapOf("question" to "where is my order"))
                span.setOutput("it ships tomorrow")
                span.end()
            }
            mcpd.flush()

            assertEquals(1, server.requests.size, "three spans, one request")
            val request = server.requests.first()
            assertEquals("/v1/ingest", request.path)
            assertEquals("POST", request.method)

            val batch = request.batch()
            val spans = assertNotNull(batch.spans)
            assertEquals(3, spans.size)
            assertEquals(listOf("run-0", "run-1", "run-2"), spans.map { it.name })
            spans.forEach {
                assertEquals(SpanStatus.OK, it.status)
                assertNotNull(it.endTime, "an ended span carries its end time")
                assertTrue((it.durationNs ?: -1) >= 0, "duration is measured, not guessed")
                assertTrue(it.isRoot)
                assertEquals("test", it.correlation.project)
            }
            assertNotNull(batch.ingestKey, "a batch without a key cannot be retried safely")
            mcpd.shutdown()
        }
    }

    @Test
    fun `an unfinished span is still exported`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            // Started and never ended: the run that crashed mid-flight is
            // exactly the one you need to see.
            mcpd.trace("long_running_agent")
            mcpd.flush()

            val span = assertNotNull(server.requests.single().batch().spans).single()
            assertEquals("long_running_agent", span.name)
            assertEquals(SpanStatus.UNSET, span.status)
            assertNull(span.endTime)
            mcpd.shutdown()
        }
    }

    @Test
    fun `an ended span replaces its own start record`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            val span = mcpd.trace("quick")
            span.end()
            mcpd.flush()

            val spans = assertNotNull(server.requests.single().batch().spans)
            assertEquals(1, spans.size, "a short span costs one record, not two")
            assertEquals(SpanStatus.OK, spans.single().status)
            mcpd.shutdown()
        }
    }

    @Test
    fun `a span that ends after its start was sent is sent again`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            val span = mcpd.trace("slow")
            mcpd.flush()
            span.end()
            mcpd.flush()

            assertEquals(2, server.requests.size)
            val opened = assertNotNull(server.requests[0].batch().spans).single()
            val closed = assertNotNull(server.requests[1].batch().spans).single()
            assertEquals(opened.spanId, closed.spanId, "the update must address the same span")
            assertNull(opened.endTime)
            assertNotNull(closed.endTime)
            mcpd.shutdown()
        }
    }

    @Test
    fun `retries are backed off and reuse one idempotency key`() = runBlocking {
        FakeMcpd { _, attempt -> if (attempt <= 2) 503 to "busy" else 200 to FakeMcpd.ACCEPTED }
            .use { server ->
                val mcpd = client(server)
                mcpd.trace("retried").end()
                mcpd.flush()

                assertEquals(3, server.requests.size, "two failures, then the retry that worked")
                val keys = server.requests.map { it.batch().ingestKey }.toSet()
                assertEquals(1, keys.size, "a retry must not be able to double-count")
                mcpd.shutdown()
            }
    }

    @Test
    fun `a malformed batch is never retried`() = runBlocking {
        FakeMcpd { _, _ -> 400 to """{"error":"bad_request"}""" }.use { server ->
            val mcpd = client(server)
            mcpd.trace("rejected").end()
            mcpd.flush()

            assertEquals(1, server.requests.size, "a 4xx will stay a 4xx")
            mcpd.shutdown()
        }
    }

    @Test
    fun `a dead server never raises`() = runBlocking {
        // Nothing is listening on this port. Every call below must still work.
        val mcpd = client(endpoint = "http://127.0.0.1:1")
        val span = mcpd.trace("unreachable", input = "hello")
        span.setUsage(inputTokens = 10, outputTokens = 2)
        span.log(LogLevel.INFO, "still fine")
        span.metric("tokens", 12.0, MetricKind.COUNTER)
        span.score("helpfulness", 1.0, 0.0, 1.0)
        span.end()
        mcpd.flush()
        mcpd.shutdown()
        mcpd.shutdown()
    }

    @Test
    fun `a client that is disabled sends nothing and still returns spans`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server) { it.copy(enabled = false) }
            val span = mcpd.trace("invisible")
            assertTrue(Ids.isValidTraceId(span.traceId), "the handle is still valid")
            assertFalse(span.recorded)
            span.setInput("anything")
            span.end()
            mcpd.feedback(span.traceId, FeedbackKind.THUMBS, "up")
            mcpd.flush()
            mcpd.shutdown()

            assertEquals(0, server.requests.size, "disabled means disabled")
            assertEquals(0, mcpd.queueDepth)
        }
    }

    @Test
    fun `the queue is bounded and the drop is counted`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server) { it.copy(maxQueue = 4, batchSize = 1000) }
            repeat(10) { i -> mcpd.trace("run-$i") }

            assertEquals(4, mcpd.queueDepth, "memory is bounded whatever happens")
            assertEquals(6L, mcpd.droppedSignals)

            mcpd.flush()
            val spans = assertNotNull(server.requests.single().batch().spans)
            assertEquals(listOf("run-6", "run-7", "run-8", "run-9"), spans.map { it.name })
            mcpd.shutdown()
        }
    }

    @Test
    fun `an incoming traceparent is continued and emitted again`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            val incoming = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
            val span = mcpd.trace("checkout", traceparent = incoming)

            assertEquals("4bf92f3577b34da6a3ce929d0e0e4736", span.traceId)
            assertEquals("00f067aa0ba902b7", span.parentSpanId)
            assertFalse(span.isRoot, "it has a parent, in another service")
            assertTrue(span.sampled, "the upstream decision is inherited")

            val outgoing = assertNotNull(TraceParent.parse(span.traceparent()))
            assertEquals(span.traceId, outgoing.traceId)
            assertEquals(span.spanId, outgoing.parentSpanId, "our span becomes their parent")
            assertTrue(outgoing.sampled)

            span.end()
            mcpd.flush()
            val sent = assertNotNull(server.requests.single().batch().spans).single()
            assertEquals("4bf92f3577b34da6a3ce929d0e0e4736", sent.traceId)
            assertEquals("00f067aa0ba902b7", sent.parentSpanId)
            mcpd.shutdown()
        }
    }

    @Test
    fun `an unsampled traceparent stops the trace being recorded`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            val span = mcpd.trace(
                "checkout",
                traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00",
            )
            assertFalse(span.sampled)
            assertFalse(span.recorded)
            span.child("step").end()
            span.end()
            mcpd.flush()

            assertEquals(0, server.requests.size, "half a trace is worse than none")
            mcpd.shutdown()
        }
    }

    @Test
    fun `children inherit the trace and the sampling decision`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            val root = mcpd.trace("support_agent", sessionId = "session-7", userId = "user-3")
            val child = root.child("call_model", SpanKind.MODEL_GENERATION, model = "gpt-4o")
            child.setUsage(inputTokens = 900, outputTokens = 120)
            child.end()
            root.end()
            mcpd.flush()

            val spans = assertNotNull(server.requests.single().batch().spans)
            val model = spans.single { it.name == "call_model" }
            assertEquals(root.traceId, model.traceId)
            assertEquals(root.spanId, model.parentSpanId)
            assertFalse(model.isRoot)
            assertEquals("gpt-4o", model.model)
            assertEquals(1020L, model.usage?.totalTokens, "a missing total is summed, not dropped")
            assertEquals("session-7", model.correlation.sessionId, "correlation is inherited")
            assertEquals("user-3", model.correlation.userId)
            mcpd.shutdown()
        }
    }

    @Test
    fun `an error sets the status and carries a message`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            val span = mcpd.trace("lookup_order")
            span.setError(IllegalStateException("order 41 is not in the database"))
            span.end()
            mcpd.flush()

            val sent = assertNotNull(server.requests.single().batch().spans).single()
            assertEquals(SpanStatus.ERROR, sent.status)
            assertEquals("order 41 is not in the database", sent.error?.message)
            assertEquals("java.lang.IllegalStateException", sent.error?.kind)
            assertNotNull(sent.error?.stack)
            mcpd.shutdown()
        }
    }

    @Test
    fun `a model span carries the settings the call was made with`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            val span = mcpd.trace("answer", kind = SpanKind.MODEL_GENERATION)
            span.setModelParameters(
                temperature = 0.2,
                topP = 0.95,
                topK = 40,
                maxTokens = 1024,
                seed = 7,
                stopSequences = listOf("\n\nHuman:"),
                hasResponseFormat = true,
            )
            span.end()
            mcpd.flush()

            val sent = assertNotNull(server.requests.single().batch().spans).single()
            val params = assertNotNull(sent.modelParameters)
            assertEquals(0.2, params.temperature)
            assertEquals(0.95, params.topP)
            assertEquals(40, params.topK)
            assertEquals(1024L, params.maxTokens)
            assertEquals(7L, params.seed)
            assertEquals(listOf("\n\nHuman:"), params.stopSequences)
            assertTrue(params.hasResponseFormat)
            mcpd.shutdown()
        }
    }

    /**
     * The classic bug this guards: reading zero as "not given". A zero seed is
     * what a reproducible run is pinned to and a zero temperature is the
     * setting most worth recording, so neither may be dropped on the way out.
     * The raw body is read as well as the decoded record, because a serialiser
     * that omits a zero would leave the field absent rather than wrong.
     */
    @Test
    fun `a zero seed is a seed`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            val span = mcpd.trace("greedy")
            span.setModelParameters(temperature = 0.0, topP = 0.0, seed = 0)
            span.end()
            mcpd.flush()

            val request = server.requests.single()
            val params = assertNotNull(assertNotNull(request.batch().spans).single().modelParameters)
            assertEquals(0L, params.seed)
            assertEquals(0.0, params.temperature)
            assertEquals(0.0, params.topP)
            assertFalse(params.hasResponseFormat)
            assertNull(params.maxTokens, "a field nobody set must stay absent")
            assertContains(request.body, "\"seed\":0")
            mcpd.shutdown()
        }
    }

    @Test
    fun `a scoped span ends itself and records what the block threw`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            var caught: String? = null
            try {
                mcpd.span("support_agent") { span ->
                    span.setInput("where is my order")
                    // The child finds its parent through the coroutine context.
                    mcpd.span("call_model", SpanKind.MODEL_GENERATION) { inner ->
                        inner.setModel("gpt-4o", "openai")
                    }
                    throw IllegalArgumentException("the tool refused")
                }
            } catch (e: IllegalArgumentException) {
                caught = e.message
            }
            assertEquals("the tool refused", caught, "the exception still reaches the caller")
            mcpd.flush()

            val spans = assertNotNull(server.requests.single().batch().spans)
            assertEquals(2, spans.size)
            val root = spans.single { it.name == "support_agent" }
            val child = spans.single { it.name == "call_model" }
            assertEquals(SpanKind.AGENT_RUN, root.kind, "a scoped root is an agent run")
            assertEquals(SpanStatus.ERROR, root.status)
            assertEquals("the tool refused", root.error?.message)
            assertEquals(SpanStatus.OK, child.status)
            assertEquals(root.spanId, child.parentSpanId, "the coroutine context carried the parent")
            mcpd.shutdown()
        }
    }

    @Test
    fun `scores carry their scale and feedback carries its value`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            val span = mcpd.trace("answer")
            span.score("answer_relevance", 0.82, 0.0, 1.0, reason = "answered the question asked")
            mcpd.feedback(span.traceId, FeedbackKind.THUMBS, "up", comment = "helpful")
            span.end()
            mcpd.flush()

            val batch = server.requests.single().batch()
            val score = assertNotNull(batch.scores).single()
            assertEquals("answer_relevance", score.scorerId)
            assertEquals(0.82, score.value, 1e-9)
            assertEquals(0.0, assertNotNull(score.min), 1e-9, "a number with no scale cannot be compared")
            assertEquals(1.0, assertNotNull(score.max), 1e-9)
            assertEquals(span.spanId, score.spanId)
            assertContains(server.requests.single().body, "\"min\"", message = "the scale must be on the wire")

            val feedback = assertNotNull(batch.feedback).single()
            assertEquals(FeedbackKind.THUMBS, feedback.kind)
            assertEquals("up", feedback.value.jsonPrimitive.content)
            assertEquals("helpful", feedback.comment)
            mcpd.shutdown()
        }
    }

    @Test
    fun `a score with no source is live, like every other SDK`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            // No `source`: the default is what the daemon files it under, and
            // one SDK once defaulted to offline, so the same call filtered
            // differently depending on the language that made it.
            mcpd.score(Ids.newTraceId(), "scale_check", value = 3.0, min = 1.0, max = 5.0)
            mcpd.flush()

            val score = assertNotNull(server.requests.single().batch().scores).single()
            assertEquals(ScoreSource.LIVE, score.source)
            assertContains(server.requests.single().body, "\"source\":\"live\"", message = "the default must be on the wire, not implied")
            mcpd.shutdown()
        }
    }

    @Test
    fun `logs and metrics are correlated to the active span`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            mcpd.span("support_agent") { span ->
                mcpd.log(LogLevel.INFO, "looked up the order", mapOf("order_id" to 41))
                mcpd.metric("orders.looked_up", 1.0, MetricKind.COUNTER, unit = "{order}")
                span.setOutput("done")
            }
            mcpd.flush()

            val batch = server.requests.single().batch()
            val log = assertNotNull(batch.logs).single()
            val metric = assertNotNull(batch.metrics).single()
            val span = assertNotNull(batch.spans).single()
            assertEquals(span.traceId, log.traceId)
            assertEquals(span.spanId, log.spanId)
            assertEquals(LogLevel.INFO, log.level)
            assertEquals(span.spanId, metric.spanId, "an exemplar turns a spike into a run")
            assertEquals(MetricKind.COUNTER, metric.kind)
            mcpd.shutdown()
        }
    }

    @Test
    fun `a large payload is gzipped`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            mcpd.trace("big_prompt", input = "x".repeat(4096)).end()
            mcpd.flush()

            val request = server.requests.single()
            assertEquals("gzip", request.header("Content-Encoding"))
            // FakeMcpd decompressed it, so a readable batch proves the round trip.
            assertEquals(4096, assertNotNull(request.batch().spans).single().input?.jsonPrimitive?.content?.length)
            mcpd.shutdown()
        }
    }

    @Test
    fun `a small payload is not gzipped`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            mcpd.trace("small").end()
            mcpd.flush()

            assertNull(server.requests.single().header("Content-Encoding"))
            mcpd.shutdown()
        }
    }

    @Test
    fun `an api key is presented as a bearer token`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server) { it.copy(apiKey = "secret-key") }
            mcpd.trace("authorised").end()
            mcpd.flush()

            assertEquals("Bearer secret-key", server.requests.single().header("Authorization"))
            mcpd.shutdown()
        }
    }

    @Test
    fun `shutdown flushes, is idempotent, and stops accepting work`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            mcpd.trace("last").end()
            mcpd.shutdown()
            assertEquals(1, server.requests.size, "the final batch is sent")

            mcpd.shutdown()
            mcpd.trace("after").end()
            mcpd.flush()
            assertEquals(1, server.requests.size, "nothing is accepted after shutdown")
        }
    }

    @Test
    fun `sampling at zero records nothing but still returns handles`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server) { it.copy(sample = 0.0) }
            val span = mcpd.trace("dropped")
            assertFalse(span.recorded)
            assertTrue(Ids.isValidSpanId(span.spanId))
            span.end()
            mcpd.flush()
            assertEquals(0, server.requests.size)
            mcpd.shutdown()
        }
    }

    @Test
    fun `an event is exported immediately and has no duration`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            val root = mcpd.trace("stream")
            root.event("first_chunk", mapOf("index" to 0))
            mcpd.flush()

            val event = assertNotNull(server.requests.single().batch().spans)
                .single { it.name == "first_chunk" }
            assertTrue(event.isEvent)
            assertNull(event.endTime)
            assertEquals(root.spanId, event.parentSpanId)
            mcpd.shutdown()
        }
    }

    @Test
    fun `a client built with no arguments points at a local mcpd`() {
        val config = McpdConfig()
        assertEquals("http://localhost:4317", config.endpoint)
        assertEquals("default", config.project)
        assertEquals(1.0, config.sample, 1e-9)
        assertTrue(config.enabled)
        assertEquals(128, config.batchSize)
        assertEquals(8192, config.maxQueue)
    }
}
