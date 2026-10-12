package dev.mcpd

import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.delay
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlin.time.Duration.Companion.milliseconds
import org.junit.jupiter.api.Assumptions
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue
import kotlin.test.fail
import kotlin.time.Duration.Companion.hours
import kotlin.time.Duration.Companion.seconds

/**
 * End-to-end tests against a running mcpd.
 *
 * These are skipped unless an endpoint is configured, because they need a
 * server. Start one and point them at it:
 *
 *     mcpd demo --data /tmp/mcpd-live.db --listen 127.0.0.1:4317
 *     MCPD_TEST_ENDPOINT=http://127.0.0.1:4317 ./gradlew test
 *
 * What they prove that [FakeMcpd] cannot: that mcpd accepts the exact shapes
 * this SDK sends, and that what comes back out is what went in. Every other
 * test in this package talks to a server this repository also wrote, so the two
 * can agree with each other and both be wrong about the daemon.
 *
 * `MCPD_REQUIRE_LIVE` turns a missing endpoint into a failure rather than a
 * skip. CI sets it in the job that starts a daemon, so if the endpoint variable
 * is ever renamed or dropped the job goes red instead of quietly skipping. The
 * Rust SDK's live suite spent a long time reporting three passes having
 * contacted no server at all, and this is the guard against a repeat.
 */
class LiveTest {

    @Test
    fun `a whole run survives the round trip`() = runBlocking {
        val mcpd = liveClient("sdk-live-kotlin")
        try {
            val health = mcpd.health()
            assertEquals("ok", health.status, "the server should be healthy")
            // A compatibility check, not a presence check. A daemon built from
            // a different spec fails here, at connect, rather than downstream
            // on a field that silently is not there.
            assertEquals(
                specVersion,
                health.specVersion,
                "the daemon reports spec ${health.specVersion}; this SDK was generated from $specVersion",
            )

            val run = mcpd.trace(
                "support agent",
                input = "where is my order?",
                tags = listOf("live-test"),
                sessionId = SESSION,
            )
            val traceId = run.traceId

            val tool = run.child("lookup_order", SpanKind.TOOL_CALL)
            tool.setOutput(mapOf("status" to "shipped"))
            tool.end()

            val call = run.child("answer", SpanKind.MODEL_GENERATION)
            call.setModel("claude-sonnet-4", "anthropic")
            call.setUsage(inputTokens = 1000, outputTokens = 500)
            call.end()

            val failed = run.child("notify_customer", SpanKind.TOOL_CALL)
            failed.setError(GATEWAY_FAILURE)
            failed.end()

            run.setOutput("It shipped on Tuesday.")
            run.end()

            mcpd.score(traceId, "answer_relevance", value = 0.9, min = 0.0, max = 1.0)
            mcpd.feedback(traceId, FeedbackKind.THUMBS, "up")
            run.log(LogLevel.INFO, "run finished")
            run.metric("orders.looked_up", 1.0, MetricKind.COUNTER)

            // flush never throws, by contract B1, so a rejected batch shows up
            // as a read that finds nothing rather than as an exception here.
            // Every assertion below is therefore also the assertion that the
            // server accepted what was sent.
            mcpd.flush()

            // Spans always come back. `include` is what asks for the signals
            // attached to the run; the server ignores a name it does not know.
            val detail = mcpd.getTrace(traceId, include = listOf("logs", "scores", "feedback"))

            assertEquals("support agent", detail.trace.name)
            assertEquals(4, detail.spans.size, "four spans, each written once and updated once")
            assertEquals(1, detail.trace.errorCount)
            assertEquals(2, detail.trace.toolCount)
            assertEquals(1, detail.trace.modelCount)
            assertEquals(
                SESSION,
                detail.trace.correlation.sessionId,
                "the session id did not survive the round trip",
            )

            // mcpd priced the model call from its own price book at ingest. The
            // SDK sent tokens and no cost, so a cost here can only have come
            // from the daemon, which is the point of asserting it.
            val cost = assertNotNull(detail.trace.cost?.totalCost, "a priced model call should have a cost")
            assertTrue(cost > 0.0, "a priced model call should have cost more than nothing, got $cost")

            val errSpan = assertNotNull(
                detail.spans.firstOrNull { it.name == "notify_customer" },
                "the failed tool span should be stored",
            )
            assertEquals(SpanStatus.ERROR, errSpan.status, "a recorded error should make the span an error")
            assertEquals(
                GATEWAY_FAILURE,
                assertNotNull(errSpan.error).message,
                "the error message did not survive",
            )

            assertEquals(1, assertNotNull(detail.scores).size)
            assertEquals(1, assertNotNull(detail.feedback).size)
        } finally {
            mcpd.shutdown()
        }
    }

    @Test
    fun `the read surface answers`() = runBlocking {
        val mcpd = liveClient(READ_PROJECT)
        try {
            val run = mcpd.trace("readable run", input = "q")
            val traceId = run.traceId
            run.setOutput("a")
            run.end()

            mcpd.flush()

            val page = mcpd.listTraces(project = READ_PROJECT, limit = 50)
            assertTrue(
                page.traces.any { it.traceId == traceId },
                "the run just written was not in its own project's listing (${page.traces.size} traces)",
            )
        } finally {
            mcpd.shutdown()
        }
    }

    @Test
    fun `a trace continues across a service boundary`() = runBlocking {
        val mcpd = liveClient("sdk-live-kotlin-ctx")
        try {
            // The caller opens a run and hands the header over the wire.
            val caller = mcpd.trace("caller")
            val traceId = caller.traceId
            val header = caller.traceparent()

            // The callee is a different process in real life. It joins the
            // trace from the header alone, which is the whole contract of
            // traceparent.
            //
            // `trace` with a traceparent, not `span` with one. `span` takes the
            // active span of the calling coroutine as its parent and only falls
            // back to the header when there is none, which is right for a
            // request handler and wrong for proving a header carried the trace:
            // run it inside the caller's scope and it would pass on the
            // in-process parent no matter what the header said.
            val callee = mcpd.trace("callee", traceparent = header)
            assertEquals(traceId, callee.traceId, "the callee should have joined the caller's trace")
            assertEquals(caller.spanId, callee.parentSpanId, "the caller's span should be the parent")
            callee.end()

            caller.end()

            mcpd.flush()

            val detail = mcpd.getTrace(traceId)
            assertEquals(
                2,
                detail.spans.size,
                "both sides of the boundary should be in one trace",
            )
        } finally {
            mcpd.shutdown()
        }
    }

    private companion object {
        const val SESSION = "session-live-kotlin"
        const val READ_PROJECT = "sdk-live-kotlin-read"

        /** Fixed, so the recording and the assertion cannot drift apart. */
        const val GATEWAY_FAILURE = "sms gateway timed out after 5s"
    }

    @Test
    fun `a workflow is served and the run and its trace read back`() = runBlocking {
        val mcpd = liveClient("sdk-live-kotlin-runs-" + Ids.newTraceId().take(8))
        try {
            val wf = Workflow("live_support")
                .step("classify") { ctx ->
                    val input = ctx.input<JsonObject>()
                    val ticket = input["ticket"]!!.jsonPrimitive.content
                    mapOf("kind" to if (ticket.startsWith("Q")) "question" else "bug")
                }
                .agent("draft", StepOptions(input = mapOf("kind" to "\${steps.classify.output.kind}"))) { ctx ->
                    val kind = ctx.input<JsonObject>()["kind"]!!.jsonPrimitive.content
                    ctx.setTokens(120, 40)
                    ctx.setState(mapOf("drafted" to true))
                    ctx.span("live_model", SpanKind.MODEL_GENERATION) { span ->
                        span.setModel("claude-sonnet-4", "anthropic")
                        span.setUsage(inputTokens = 120, outputTokens = 40)
                    }
                    "an answer to a $kind"
                }
                .output(mapOf("answer" to "\${steps.draft.output}"))
            val worker = Worker(mcpd, workerId = "live-worker", wait = 1.seconds, onError = { m, e -> fail("$m: $e") }).register(wf)
            worker.registerDefinitions()

            val run = mcpd.startRun("live_support", input = mapOf("ticket" to "Q-42"))
            assertEquals(RunStatus.RUNNING, run.status)
            assertEquals("live_support", run.definitionId)

            var detail = mcpd.getRun(run.runId)
            repeat(10) {
                if (detail.run.status != RunStatus.RUNNING) return@repeat
                worker.claim(4, run.runId)
                worker.drain()
                detail = mcpd.getRun(run.runId)
            }
            assertEquals(RunStatus.SUCCEEDED, detail.run.status, detail.run.error ?: "")
            assertEquals(Json.parseToJsonElement("""{"answer": "an answer to a question"}"""), detail.run.output)
            assertEquals(Json.parseToJsonElement("""{"drafted": true}"""), detail.run.state)
            val draft = detail.attempts.first { it.stepKey == "draft" }
            assertEquals(AttemptStatus.SUCCEEDED, draft.status)
            assertEquals(StepKind.AGENT, draft.kind)
            assertEquals(Json.parseToJsonElement("""{"kind": "question"}"""), draft.input)
            assertEquals(120L, draft.usage?.inputTokens)

            val kinds = mutableListOf<RunEventKind>()
            mcpd.watchRun(run.runId, poll = 50.milliseconds) { kinds.add(it.kind) }
            assertEquals(RunEventKind.RUN_STARTED, kinds.first())
            assertEquals(RunEventKind.RUN_ENDED, kinds.last())

            mcpd.flush()
            // The daemon writes the run's own spans through an outbox drained on
            // its own clock, so give it a moment before reading the trace.
            var trace: TraceDetail? = null
            repeat(40) {
                val candidate = runCatching { mcpd.getTrace(run.traceId) }.getOrNull()
                if (candidate != null && candidate.spans.size >= 4 && candidate.spans.any { it.name == "live_model" }) {
                    trace = candidate
                    return@repeat
                }
                delay(250)
            }
            val model = assertNotNull(trace, "the run's trace").spans.first { it.name == "live_model" }
            assertEquals(draft.spanId, model.parentSpanId)
        } finally {
            mcpd.shutdown()
        }
    }

    @Test
    fun `a wait resumes on publish and another run ends on cancel`() = runBlocking {
        val mcpd = liveClient("sdk-live-kotlin-runs-" + Ids.newTraceId().take(8))
        try {
            val wf = Workflow("live_approval")
                .waitFor("approval", "approved", "\${input.ticket}", 60.seconds)
                .step("close", StepOptions(input = mapOf("by" to "\${steps.approval.output.by}"))) { ctx ->
                    mapOf("closed_by" to ctx.input<JsonObject>()["by"]!!.jsonPrimitive.content)
                }
                .output("\${steps.close.output}")
            val worker = Worker(mcpd, workerId = "live-approver", wait = 500.milliseconds, onError = { m, e -> fail("$m: $e") }).register(wf)
            worker.registerDefinitions()

            val waiting = mcpd.startRun("live_approval", input = mapOf("ticket" to "T-7"))
            var detail = mcpd.getRun(waiting.runId)
            repeat(10) {
                if (detail.waits.isNotEmpty()) return@repeat
                delay(100)
                detail = mcpd.getRun(waiting.runId)
            }
            assertEquals(listOf("approved" to "T-7"), detail.waits.map { it.event to it.key })

            val receipt = mcpd.publishEvent("approved", "T-7", payload = mapOf("by" to "ops"), eventId = "live-approval-kotlin-1")
            assertEquals(EventOutcome.DELIVERED, receipt.outcome)
            val again = mcpd.publishEvent("approved", "T-7", eventId = "live-approval-kotlin-1")
            assertEquals(EventOutcome.DUPLICATE, again.outcome)

            repeat(10) {
                if (detail.run.status != RunStatus.RUNNING) return@repeat
                worker.claim(1, waiting.runId)
                worker.drain()
                detail = mcpd.getRun(waiting.runId)
            }
            assertEquals(RunStatus.SUCCEEDED, detail.run.status, detail.run.error ?: "")
            assertEquals(Json.parseToJsonElement("""{"closed_by": "ops"}"""), detail.run.output)

            val doomed = mcpd.startRun("live_approval", input = mapOf("ticket" to "T-8"))
            val cancelled = mcpd.cancelRun(doomed.runId)
            assertEquals(RunStatus.CANCELLED, cancelled.status)
            var last: RunEventKind? = null
            mcpd.watchRun(doomed.runId, poll = 50.milliseconds) { last = it.kind }
            assertEquals(RunEventKind.RUN_ENDED, last)

            val listed = mcpd.listRuns(definitionId = "live_approval")
            assertEquals(2, listed.runs.size)
        } finally {
            mcpd.shutdown()
        }
    }

}

/**
 * A client pointed at a real daemon, or a skipped test.
 *
 * This is a helper rather than a fixture so each test reports its own skip, and
 * so a run with no server says which tests did not happen instead of silently
 * reporting a green suite.
 */
private fun liveClient(project: String): Mcpd = Mcpd(
    McpdConfig(
        endpoint = liveEndpoint(),
        project = project,
        environment = "test",
        serviceName = "mcpd-kotlin-live",
        serviceVersion = "0.1.0",
        // Big and slow, so a test that forgets to flush fails on its assertions
        // rather than on a timer that had not fired yet.
        batchSize = 1000,
        flushInterval = 1.hours,
        timeout = 10.seconds,
    )
)

private fun liveEndpoint(): String {
    liveEnv("MCPD_TEST_ENDPOINT")?.let { return it }
    if (liveEnv("MCPD_REQUIRE_LIVE") != null) {
        fail(
            "MCPD_REQUIRE_LIVE is set but MCPD_TEST_ENDPOINT is not, so this suite would " +
                "have skipped without reaching a server. That is the exact silence this " +
                "variable exists to break."
        )
    }
    return Assumptions.abort(
        "no MCPD_TEST_ENDPOINT; set it to a running daemon to run the live suite"
    )
}

private fun liveEnv(name: String): String? = System.getenv(name)?.trim()?.takeIf { it.isNotEmpty() }
