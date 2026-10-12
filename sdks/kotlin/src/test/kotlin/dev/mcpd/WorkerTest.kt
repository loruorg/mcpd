package dev.mcpd

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.Test
import kotlin.test.assertContains
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNotNull
import kotlin.test.assertTrue
import kotlin.time.Duration
import kotlin.time.Duration.Companion.hours
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds

/** The worker loop and the run API against a scripted daemon (contract sections 7.2 to 7.4). */
class WorkerTest {
    private val trace = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"
    private val stepSpan = "b2b2b2b2b2b2b2b2"

    private fun lease(vararg overrides: Pair<String, String>): String {
        val fields = linkedMapOf(
            "lease_id" to "\"lease-1\"", "run_id" to "\"run-1\"", "project" to "\"test\"", "definition_id" to "\"support\"",
            "definition_version" to "1", "step_key" to "\"classify\"", "kind" to "\"task\"", "attempt" to "0",
            "effect" to "\"at_least_once\"", "expires_at" to "\"2026-09-01T10:00:00Z\"", "idempotency_key" to "\"run-1:classify:0\"",
            "input" to """{"ticket_id": "T-1"}""",
            "prior" to """{"triage": {"status": "succeeded", "output": {"queue_name": "billing"}}}""",
            "state" to """{"seen_count": 1}""",
            "correlation" to """{"project": "test", "session_id": "s-1"}""",
            "trace_id" to "\"$trace\"", "span_id" to "\"$stepSpan\"", "parent_span_id" to "\"c3c3c3c3c3c3c3c3\"",
        )
        for ((k, v) in overrides) fields[k] = v
        return fields.entries.joinToString(",", "{", "}") { "\"${it.key}\": ${it.value}" }
    }

    private fun claimed(vararg leases: String) = 200 to """{"leases": [${leases.joinToString(",")}]}"""
    private val idle = 200 to """{"leases": []}"""
    private val started = 200 to """{"expires_at": "2026-09-01T10:00:30Z", "reclaimed": false}"""
    private val acked = 200 to """{"run_status": "running", "next": "claim"}"""
    private val registered = 200 to """{"worker_id": "w-1", "registered": [{"id": "support", "version": 1, "assigned": true}], "lease_ttl_ms": 3000, "heartbeat_ms": 1000}"""

    private fun conflict(reason: String, extra: String = "") =
        409 to """{"error": "conflict", "reason": "$reason", "message": "the daemon says $reason", "attempt": 0, "run_status": "running"$extra}"""

    /** Answers by path suffix from per-route queues, repeating the last reply; everything else from the script. */
    private class Routes(val script: MutableList<Pair<Int, String>>, val routes: Map<String, MutableList<Pair<Int, String>>>) {
        fun answer(received: Received): Pair<Int, String> {
            for ((suffix, queue) in routes) {
                if (received.path.substringBefore('?').endsWith(suffix) && queue.isNotEmpty()) {
                    return if (queue.size > 1) queue.removeAt(0) else queue[0]
                }
            }
            synchronized(script) {
                return if (script.isEmpty()) 200 to "{}" else script.removeAt(0)
            }
        }
    }

    private class Harness(script: List<Pair<Int, String>>, routes: Map<String, List<Pair<Int, String>>> = emptyMap(), concurrency: Int = 3) : AutoCloseable {
        private val plan = Routes(script.toMutableList(), routes.mapValues { it.value.toMutableList() })
        val server = FakeMcpd { received, _ -> plan.answer(received) }
        val mcpd = Mcpd(McpdConfig(endpoint = server.endpoint, project = "test", flushInterval = 1.hours))
        val errors = CopyOnWriteArrayList<String>()
        val worker = Worker(mcpd, workerId = "w-1", concurrency = concurrency, wait = Duration.ZERO, onError = { m, e -> errors.add("$m: $e") })

        fun paths(): List<String> = server.requests.map { "${it.method} ${it.path.substringBefore('?')}" }
        fun body(i: Int): JsonElement = Json.parseToJsonElement(server.requests[i].body)
        fun json(text: String): JsonElement = Json.parseToJsonElement(text)

        override fun close() {
            runBlocking { mcpd.shutdown() }
            server.close()
        }
    }

    private suspend fun Harness.claimAndDrain(max: Int): Int {
        val n = worker.claim(max)
        worker.drain()
        return n
    }

    @Test
    fun `an idle claim outlives the client timeout by the wait`() = runBlocking {
        // The daemon holds an idle claim open for the whole of the wait, so the
        // answer arrives after the client's own timeout and within the wait. A
        // claim cut off by that timeout is what made a healthy idle worker
        // report a failure every ten seconds.
        FakeMcpd { _, _ ->
            Thread.sleep(300)
            200 to """{"leases": []}"""
        }.use { server ->
            val mcpd = Mcpd(McpdConfig(endpoint = server.endpoint, project = "test", flushInterval = 1.hours, timeout = 100.milliseconds))
            try {
                val response = mcpd.claimSteps("w-1", 1, 2.seconds)
                assertEquals(emptyList(), response.leases)
            } finally {
                mcpd.shutdown()
            }
        }
    }

    @Test
    fun `one lease runs started the body and complete`() = runBlocking {
        Harness(listOf(claimed(lease()), started, acked)).use { h ->
            val seen = mutableListOf<Any?>()
            h.worker.register(
                Workflow("support").step("classify") { ctx ->
                    seen.addAll(listOf(ctx.input<JsonObject>(), ctx.prior<JsonObject>(), ctx.state<JsonObject>(), ctx.idempotencyKey, ctx.attempt, ctx.runId, ctx.stepKey))
                    ctx.setState(mapOf("seen_count" to 2, "last_ticket" to "T-1"))
                    ctx.setTokens(10, 5)
                    mapOf("kind" to "question")
                },
            )
            assertEquals(1, h.claimAndDrain(1))
            assertEquals(listOf("POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started", "POST /v1/api/steps/lease-1/complete"), h.paths())
            assertEquals(h.json("""{"project": "test", "worker_id": "w-1", "max": 1, "wait_ms": 0}"""), h.body(0))
            // Opaque objects reach the body spelled as the daemon sent them.
            assertEquals(h.json("""{"ticket_id": "T-1"}"""), seen[0])
            assertEquals(h.json("""{"triage": {"status": "succeeded", "output": {"queue_name": "billing"}}}"""), seen[1])
            assertEquals(h.json("""{"seen_count": 1}"""), seen[2])
            assertEquals(listOf<Any?>("run-1:classify:0", 0, "run-1", "classify"), seen.drop(3))
            assertEquals(
                h.json("""{"output": {"kind": "question"}, "state": {"seen_count": 2, "last_ticket": "T-1"}, "usage": {"input_tokens": 10, "output_tokens": 5}}"""),
                h.body(2),
            )
            assertEquals(emptyList(), h.errors)
            assertEquals(0, h.worker.inflight)
        }
    }

    @Test
    fun `a span from the body lands under the step span the daemon opened`() = runBlocking {
        Harness(listOf(claimed(lease()), started, acked, 200 to FakeMcpd.ACCEPTED)).use { h ->
            h.worker.register(
                Workflow("support").agent("classify") { ctx ->
                    ctx.span("model_call", SpanKind.MODEL_GENERATION) { span ->
                        span.setModel("claude-sonnet-4", "anthropic")
                        "ok"
                    }
                },
            )
            h.claimAndDrain(1)
            h.mcpd.flush()
            val spans = h.server.requests.first { it.path.endsWith("/v1/ingest") }.batch().spans ?: emptyList()
            assertTrue(spans.isNotEmpty())
            for (span in spans) {
                assertEquals("model_call", span.name)
                assertEquals(trace, span.traceId)
                assertEquals(stepSpan, span.parentSpanId)
                assertEquals("s-1", span.correlation.sessionId)
            }
        }
    }

    @Test
    fun `a branch an item and an iteration key find their handlers`() = runBlocking {
        val script = listOf(
            claimed(
                lease("lease_id" to "\"l-a\"", "step_key" to "\"checks/fraud\""),
                lease("lease_id" to "\"l-b\"", "step_key" to "\"notify[2]\""),
                lease("lease_id" to "\"l-c\"", "step_key" to "\"refine[1]\""),
            ),
        )
        Harness(script, routes = mapOf("/started" to listOf(started), "/complete" to listOf(acked))).use { h ->
            val ran = CopyOnWriteArrayList<String>()
            fun record(name: String): StepHandler = { ran.add(name); name }
            h.worker.register(
                Workflow("support")
                    .parallel("checks") { step("fraud", body = record("fraud")) }
                    .forEach("notify", "input.to", 3) { step("send", body = record("send")) }
                    .loop("refine", 2) { step("rewrite", body = record("rewrite")) },
            )
            assertEquals(3, h.claimAndDrain(3))
            assertEquals(listOf("fraud", "rewrite", "send"), ran.sorted())
            assertEquals(3, h.paths().count { it.endsWith("/complete") })
        }
    }

    @Test
    fun `a retryable error is reported with its class and delay`() = runBlocking {
        Harness(listOf(claimed(lease()), started, 200 to """{"run_status": "running", "next": "idle", "attempts_left": 2}""")).use { h ->
            h.worker.register(
                Workflow("support").step("classify") {
                    throw RetryableException("rate limited", retryAfter = 250.milliseconds, details = mapOf("status_code" to 429))
                },
            )
            h.claimAndDrain(1)
            assertEquals("POST /v1/api/steps/lease-1/fail", h.paths()[2])
            assertEquals(h.json("""{"error": {"class": "retryable", "message": "rate limited", "details": {"status_code": 429}, "retry_after_ms": 250}}"""), h.body(2))
        }
    }

    @Test
    fun `any other throw is fatal and a failure names its class`() = runBlocking {
        val script = listOf(claimed(lease(), lease("lease_id" to "\"lease-2\"", "step_key" to "\"draft\"")))
        Harness(script, routes = mapOf("/started" to listOf(started), "/fail" to listOf(acked))).use { h ->
            h.worker.register(
                Workflow("support")
                    .step("classify") { throw IllegalStateException("the model returned nothing") }
                    .step("draft") { throw StepFailureException("no such ticket", StepErrorClass.INVALID_INPUT) },
            )
            h.claimAndDrain(2)
            val fails = h.server.requests.filter { it.path.endsWith("/fail") }.associate { it.path to Json.parseToJsonElement(it.body) }
            assertEquals(h.json("""{"error": {"class": "fatal", "message": "the model returned nothing"}}"""), fails["/v1/api/steps/lease-1/fail"])
            assertEquals(h.json("""{"error": {"class": "invalid_input", "message": "no such ticket"}}"""), fails["/v1/api/steps/lease-2/fail"])
        }
    }

    @Test
    fun `a lease it cannot serve is handed back without running anything`() = runBlocking {
        val script = listOf(claimed(lease("definition_id" to "\"billing\""), lease("lease_id" to "\"lease-2\"", "definition_version" to "7")))
        Harness(script, routes = mapOf("/fail" to listOf(acked))).use { h ->
            var ran = false
            h.worker.register(Workflow("support", version = 1).step("classify") { ran = true; null })
            h.claimAndDrain(2)
            assertFalse(ran)
            for (request in h.server.requests.drop(1)) {
                assertTrue(request.path.endsWith("/fail"), request.path)
                val error = Json.parseToJsonElement(request.body).jsonObject["error"]!!.jsonObject
                assertEquals("unservable", error["class"]!!.jsonPrimitive.content)
                assertContains(error["message"]!!.jsonPrimitive.content, "w-1 does not serve")
            }
        }
    }

    @Test
    fun `already_applied is done and says nothing`() = runBlocking {
        Harness(listOf(claimed(lease()), started, conflict("already_applied", """, "attempt_status": "succeeded""""))).use { h ->
            h.worker.register(Workflow("support").step("classify") { 1 })
            h.claimAndDrain(1)
            assertEquals(3, h.paths().size)
            assertEquals(emptyList(), h.errors)
        }
    }

    @Test
    fun `a body whose started was fenced never runs`() = runBlocking {
        Harness(listOf(claimed(lease()), conflict("fenced", """, "attempt_status": "orphaned""""))).use { h ->
            var ran = false
            h.worker.register(Workflow("support").step("classify") { ran = true; null })
            h.claimAndDrain(1)
            assertFalse(ran)
            assertEquals(listOf("POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started"), h.paths())
            assertEquals(emptyList(), h.errors)
        }
    }

    @Test
    fun `a lease whose run ended is abandoned with nothing more posted`() = runBlocking {
        Harness(listOf(claimed(lease()), started, conflict("run_terminal", """, "run_status": "cancelled""""))).use { h ->
            h.worker.register(Workflow("support").step("classify") { 1 })
            h.claimAndDrain(1)
            assertEquals(3, h.paths().size)
            assertEquals(emptyList(), h.errors)
        }
    }

    @Test
    fun `not_started is logged loudly because it is a bug in the worker`() = runBlocking {
        Harness(listOf(claimed(lease("effect" to "\"at_most_once\"")), started, conflict("not_started"))).use { h ->
            h.worker.register(Workflow("support").step("classify") { 1 })
            h.claimAndDrain(1)
            assertEquals(1, h.errors.size, h.errors.toString())
            assertContains(h.errors[0], "a bug in this worker")
        }
    }

    @Test
    fun `a fenced heartbeat cancels the body and posts no complete`() = runBlocking {
        Harness(listOf(registered, claimed(lease()), started, conflict("fenced", """, "attempt_status": "orphaned""""))).use { h ->
            val reason = CompletableDeferred<String>()
            h.worker.register(
                Workflow("support").step("classify") { ctx ->
                    reason.complete(ctx.cancelled.await())
                    "too late"
                },
            )
            h.worker.registerDefinitions()
            h.claimAndDrain(1)
            assertContains(withTimeout(5.seconds) { reason.await() }, "fenced")
            assertEquals(
                listOf("POST /v1/api/workers/register", "POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started", "POST /v1/api/steps/lease-1/heartbeat"),
                h.paths(),
            )
            assertEquals(emptyList(), h.errors)
        }
    }

    @Test
    fun `a heartbeat that fails for any other reason is reported and the body goes on`() = runBlocking {
        // A 400 is not retried; a 5xx would be, and would land.
        Harness(listOf(registered, claimed(lease()), started, 400 to """{"error": "bad_request", "message": "no"}""", acked)).use { h ->
            h.worker.register(Workflow("support").step("classify") { delay(1150); "done" })
            h.worker.registerDefinitions()
            h.claimAndDrain(1)
            assertEquals("POST /v1/api/steps/lease-1/complete", h.paths().last())
            assertTrue(h.errors.any { it.contains("heartbeat failed") }, h.errors.toString())
        }
    }

    @Test
    fun `serve registers claims until stopped and returns once stopped`() = runBlocking {
        Harness(listOf(registered), routes = mapOf("/steps/claim" to listOf(idle)), concurrency = 2).use { h ->
            h.worker.register(Workflow("support").step("classify") { 1 })
            val serving = launch { h.worker.serve() }
            withTimeout(5.seconds) { while (h.server.requests.size < 3) delay(10) }
            h.worker.stop()
            withTimeout(5.seconds) { serving.join() }
            val paths = h.paths()
            assertEquals("POST /v1/api/workers/register", paths[0])
            assertEquals(
                h.json("""{"project": "test", "worker_id": "w-1", "definitions": [{"id": "support", "kind": "workflow", "body": {"steps": [{"kind": "task", "id": "classify"}]}}]}"""),
                h.body(0),
            )
            assertTrue(paths.size > 2, paths.toString())
            assertTrue(paths.drop(1).all { it == "POST /v1/api/steps/claim" })
            assertEquals(h.json("""{"project": "test", "worker_id": "w-1", "max": 2, "wait_ms": 0}"""), h.body(1))
            assertEquals(emptyList(), h.errors)
        }
    }

    @Test
    fun `a claim that failed is reported and tried again`() = runBlocking {
        Harness(listOf(registered, 400 to """{"error": "bad_request", "message": "no"}""", idle)).use { h ->
            h.worker.register(Workflow("support").step("classify") { 1 })
            val serving = launch { h.worker.serve() }
            withTimeout(5.seconds) { while (h.errors.isEmpty()) delay(10) }
            h.worker.stop()
            withTimeout(5.seconds) { serving.join() }
            assertTrue(h.errors[0].contains("claim failed"), h.errors.toString())
        }
    }

    @Test
    fun `a run starts with the fields the daemon expects`() = runBlocking {
        val run = """{"run_id": "run-1", "project": "test", "definition_id": "support", "definition_version": 2, "status": "running",
            "cursor": {"index": 0, "construct": "task"}, "correlation": {"project": "test"}, "trace_id": "$trace", "steps_used": 0,
            "cost_usd": 0.0, "root_run_id": "run-1", "depth": 0, "created_at": "2026-09-01T10:00:00Z", "updated_at": "2026-09-01T10:00:00Z"}"""
        Harness(listOf(200 to run)).use { h ->
            val started = h.mcpd.startRun(
                "support", version = 2, input = mapOf("ticket_id" to "T-1"), idempotencyKey = "ticket:T-1",
                correlation = Correlation(project = "", sessionId = "s-1"),
            )
            assertEquals("run-1", started.runId)
            assertEquals(RunStatus.RUNNING, started.status)
            assertEquals(
                h.json("""{"project": "test", "definition_id": "support", "version": 2, "input": {"ticket_id": "T-1"}, "idempotency_key": "ticket:T-1", "correlation": {"project": "test", "session_id": "s-1"}}"""),
                h.body(0),
            )
        }
    }

    @Test
    fun `a definition is stored with its body untouched`() = runBlocking {
        Harness(listOf(200 to """{"id": "support", "version": 1, "assigned": true}""")).use { h ->
            val doc = Workflow("support").step("classify", StepOptions(timeout = 100.milliseconds)) { 1 }.document()
            val stored = h.mcpd.putDefinition(doc)
            assertEquals(Triple("support", 1, true), Triple(stored.id, stored.version, stored.assigned))
            assertTrue(h.server.requests[0].path.endsWith("/v1/api/definitions?project=test"), h.server.requests[0].path)
            assertEquals(h.json("""{"id": "support", "kind": "workflow", "body": {"steps": [{"kind": "task", "id": "classify", "step_timeout_ms": 100}]}}"""), h.body(0))
        }
    }

    @Test
    fun `watchRun walks the pages until the run ends passing the last seq it saw`() = runBlocking {
        val script = listOf(
            200 to """{"events": [{"run_id": "run-1", "seq": 0, "kind": "run_started", "payload": {"definition_id": "support"}, "at": "2026-09-01T10:00:00Z"},
                {"run_id": "run-1", "seq": 1, "kind": "step_leased", "payload": {"step_key": "classify"}, "at": "2026-09-01T10:00:01Z"}], "has_more": true}""",
            200 to """{"events": [], "has_more": false}""",
            200 to """{"events": [{"run_id": "run-1", "seq": 2, "kind": "run_ended", "payload": {"status": "succeeded"}, "at": "2026-09-01T10:00:02Z"}], "has_more": false}""",
        )
        Harness(script).use { h ->
            val kinds = mutableListOf<RunEventKind>()
            h.mcpd.watchRun("run-1", poll = 1.milliseconds) { kinds.add(it.kind) }
            assertEquals(listOf(RunEventKind.RUN_STARTED, RunEventKind.STEP_LEASED, RunEventKind.RUN_ENDED), kinds)
            assertEquals(
                listOf("after=-1&limit=500", "after=1&limit=500", "after=1&limit=500"),
                h.server.requests.map { it.path.substringAfter('?') },
            )
        }
    }

    @Test
    fun `publish and signal carry the ids the caller supplied`() = runBlocking {
        val receipt = 200 to """{"outcome": "delivered", "event_id": "evt-1"}"""
        Harness(listOf(receipt, receipt)).use { h ->
            h.mcpd.publishEvent("approved", "T-1", payload = mapOf("by" to "ops"), eventId = "evt-1")
            h.mcpd.signalRun("run-1", "approved", payload = mapOf("by" to "ops"), eventId = "evt-2")
            assertEquals("POST /v1/api/events", h.paths()[0])
            assertEquals(h.json("""{"project": "test", "event": "approved", "key": "T-1", "payload": {"by": "ops"}, "event_id": "evt-1"}"""), h.body(0))
            assertEquals("POST /v1/api/runs/run-1/signal", h.paths()[1])
            assertEquals(h.json("""{"event": "approved", "payload": {"by": "ops"}, "event_id": "evt-2"}"""), h.body(1))
        }
    }

    @Test
    fun `a 409 from a worker call is a typed conflict and any other 409 is itself`() = runBlocking {
        val script = listOf(
            conflict("fenced", """, "attempt_status": "orphaned""""),
            409 to """{"error": "version_taken", "message": "support@1 exists with a different body"}""",
        )
        Harness(script).use { h ->
            val typed = assertFailsWith<StepConflictException> { h.mcpd.completeStep("lease-1", StepResult()) }
            assertEquals(ConflictReason.FENCED, typed.conflict.reason)
            assertEquals(AttemptStatus.ORPHANED, typed.conflict.attemptStatus)
            val plain = assertFailsWith<McpdException> { h.mcpd.putDefinition(Workflow("support", version = 1).document()) }
            assertFalse(plain is StepConflictException)
            assertNotNull(plain.message)
            assertContains(plain.message!!, "version_taken")
            assertIs<McpdException>(typed)
        }
    }

    @Test
    fun `a body that runs past the lease deadline is cancelled and failed retryable`() = runBlocking {
        Harness(listOf(claimed(lease("deadline_ms" to "150")), started, acked)).use { h ->
            val reason = CompletableDeferred<String>()
            h.worker.register(
                Workflow("support").step("classify") { ctx ->
                    reason.complete(ctx.cancelled.await())
                    "too late"
                },
            )
            h.claimAndDrain(1)
            assertEquals("step deadline of 150ms passed", withTimeout(5.seconds) { reason.await() })
            assertEquals(
                listOf("POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started", "POST /v1/api/steps/lease-1/fail"),
                h.paths(),
            )
            assertEquals(h.json("""{"error": {"class": "retryable", "message": "step deadline of 150ms passed"}}"""), h.body(2))
            assertEquals(emptyList(), h.errors)
        }
    }

    @Test
    fun `the slot is freed at the deadline even when the body ignores cancelled`() = runBlocking {
        Harness(listOf(claimed(lease("deadline_ms" to "100")), started, acked)).use { h ->
            h.worker.register(
                Workflow("support").step("classify") {
                    // A body that never looks at ctx.cancelled.
                    kotlinx.coroutines.delay(3_000)
                    "too late"
                },
            )
            val began = System.nanoTime()
            h.claimAndDrain(1)
            val took = (System.nanoTime() - began) / 1_000_000
            assertTrue(took < 1_000, "the slot was held for ${took}ms, past the deadline")
            assertEquals(0, h.worker.stats().inflight)
            assertEquals("POST /v1/api/steps/lease-1/fail", h.paths().last())
        }
    }

    @Test
    fun `without an onError the worker logs one line per kind of failure per interval, whatever the step`() = runBlocking {
        // Three leases this worker cannot serve, each with its own step key,
        // and a daemon that refuses each `fail`. The step key used to be part
        // of the limit's key, so every step got a line of its own.
        val leases = (1..3).map { lease("lease_id" to "\"lease-$it\"", "step_key" to "\"unknown_$it\"") }
        val plan = Routes(mutableListOf(claimed(*leases.toTypedArray())), mapOf("/fail" to mutableListOf(400 to """{"error":"bad"}""")))
        val lines = CopyOnWriteArrayList<String>()
        val capture = object : java.util.logging.Handler() {
            override fun publish(record: java.util.logging.LogRecord) { lines.add(record.message) }
            override fun flush() {}
            override fun close() {}
        }
        val logger = java.util.logging.Logger.getLogger("dev.mcpd")
        logger.addHandler(capture)
        FakeMcpd { received, _ -> plan.answer(received) }.use { server ->
            val mcpd = Mcpd(McpdConfig(endpoint = server.endpoint, project = "test", flushInterval = 1.hours))
            try {
                val worker = Worker(mcpd, workerId = "w-1", concurrency = 3, wait = Duration.ZERO)
                worker.claim(3)
                worker.drain()
                assertEquals(3, server.requests.count { it.path.endsWith("/fail") })
                assertEquals(1, lines.count { it.contains("call failed for") }, lines.toString())
            } finally {
                logger.removeHandler(capture)
                mcpd.shutdown()
            }
        }
    }

    @Test
    fun `a body that finishes in time is left alone`() = runBlocking {
        Harness(listOf(claimed(lease("deadline_ms" to "5000")), started, acked)).use { h ->
            h.worker.register(Workflow("support").step("classify") { "quick" })
            h.claimAndDrain(1)
            assertEquals("POST /v1/api/steps/lease-1/complete", h.paths().last())
        }
    }

    @Test
    fun `input that does not fit fails the step invalid_input`() = runBlocking {
        Harness(listOf(claimed(lease()), started, acked)).use { h ->
            @kotlinx.serialization.Serializable
            data class Ticket(val ticket_id: Int)
            h.worker.register(Workflow("support").step("classify") { ctx -> ctx.input<Ticket>().ticket_id })
            h.claimAndDrain(1)
            assertEquals("POST /v1/api/steps/lease-1/fail", h.paths()[2])
            val error = h.body(2).jsonObject["error"]!!.jsonObject
            assertEquals("invalid_input", error["class"]!!.jsonPrimitive.content)
            assertContains(error["message"]!!.jsonPrimitive.content, "the step's data could not be read")
        }
    }

    @Test
    fun `usage set on spans under the context is summed into the step and the last model wins`() = runBlocking {
        Harness(listOf(claimed(lease()), started, acked, 200 to FakeMcpd.ACCEPTED)).use { h ->
            h.worker.register(
                Workflow("support").agent("classify") { ctx ->
                    ctx.span("first", SpanKind.MODEL_GENERATION) { span ->
                        span.setModel("claude-sonnet-4", "anthropic")
                        span.setUsage(inputTokens = 100, outputTokens = 30)
                    }
                    ctx.span("second", SpanKind.MODEL_GENERATION) { span ->
                        span.setModel("claude-haiku-4")
                        span.setUsage(inputTokens = 20, outputTokens = 10)
                        // Three deep, through the ambient span rather than the context.
                        h.mcpd.span("embed", SpanKind.GENERIC) { inner ->
                            inner.setUsage(inputTokens = 5, outputTokens = 0)
                        }
                    }
                    "ok"
                },
            )
            h.claimAndDrain(1)
            assertEquals("POST /v1/api/steps/lease-1/complete", h.paths()[2])
            assertEquals(
                h.json("""{"output": "ok", "usage": {"input_tokens": 125, "output_tokens": 40, "total_tokens": 165, "model": "claude-haiku-4", "provider": "anthropic"}}"""),
                h.body(2),
            )
            assertEquals(emptyList(), h.errors)
        }
    }

    @Test
    fun `setUsage overrides what the spans summed`() = runBlocking {
        Harness(listOf(claimed(lease()), started, acked, 200 to FakeMcpd.ACCEPTED)).use { h ->
            h.worker.register(
                Workflow("support").agent("classify") { ctx ->
                    ctx.span("first", SpanKind.MODEL_GENERATION) { span ->
                        span.setModel("claude-sonnet-4", "anthropic")
                        span.setUsage(inputTokens = 100, outputTokens = 30)
                    }
                    ctx.setTokens(10, 5)
                    "ok"
                },
            )
            h.claimAndDrain(1)
            assertEquals(
                h.json("""{"output": "ok", "usage": {"input_tokens": 10, "output_tokens": 5, "model": "claude-sonnet-4", "provider": "anthropic"}}"""),
                h.body(2),
            )
        }
    }

    @Test
    fun `a body that records no spans reports what it set and nothing else`() = runBlocking {
        Harness(listOf(claimed(lease()), started, acked)).use { h ->
            h.worker.register(Workflow("support").step("classify") { "ok" })
            h.claimAndDrain(1)
            assertEquals(h.json("""{"output": "ok"}"""), h.body(2))
        }
    }

    private class Hooks {
        val leases = CopyOnWriteArrayList<String>()
        val starts = CopyOnWriteArrayList<String>()
        val ends = CopyOnWriteArrayList<Pair<String, StepOutcome>>()
        val conflicts = CopyOnWriteArrayList<Pair<String, String>>()
    }

    private fun hooked(script: List<Pair<Int, String>>, hooks: Hooks, onStart: ((StepContext) -> Unit)? = null): Pair<Harness, Worker> {
        val h = Harness(script)
        val worker = Worker(
            h.mcpd, workerId = "w-1", concurrency = 3, wait = Duration.ZERO,
            onError = { m, e -> h.errors.add("$m: $e") },
            onLease = { hooks.leases.add(it.leaseId) },
            onStepStart = { ctx -> hooks.starts.add(ctx.stepKey); onStart?.invoke(ctx) },
            onStepEnd = { ctx, outcome -> hooks.ends.add(ctx.stepKey to outcome) },
            onConflict = { lease, reason -> hooks.conflicts.add(lease.leaseId to reason) },
        )
        return h to worker
    }

    @Test
    fun `the four hooks are each called once with what the contract says`() = runBlocking {
        val hooks = Hooks()
        val (h, worker) = hooked(listOf(claimed(lease()), started, conflict("already_applied", """, "attempt_status": "succeeded"""")), hooks)
        h.use {
            worker.register(Workflow("support").step("classify") { 1 })
            worker.claim(1)
            worker.drain()
            assertEquals(listOf("lease-1"), hooks.leases)
            assertEquals(listOf("classify"), hooks.starts)
            assertEquals(listOf("classify" to StepOutcome.Completed), hooks.ends.toList())
            assertEquals(listOf("lease-1" to "already_applied"), hooks.conflicts.toList())
            assertEquals(emptyList(), h.errors)
        }
    }

    @Test
    fun `a failed body reaches onStepEnd with its class`() = runBlocking {
        val hooks = Hooks()
        val (h, worker) = hooked(listOf(claimed(lease()), started, acked), hooks)
        h.use {
            worker.register(Workflow("support").step("classify") { throw RetryableException("rate limited") })
            worker.claim(1)
            worker.drain()
            assertEquals(listOf("classify" to StepOutcome.Failed(StepErrorClass.RETRYABLE, "rate limited")), hooks.ends.toList())
        }
    }

    @Test
    fun `a hook that throws is reported and the lease still completes`() = runBlocking {
        val hooks = Hooks()
        val (h, worker) = hooked(listOf(claimed(lease()), started, acked), hooks) { throw IllegalStateException("hook broke") }
        h.use {
            worker.register(Workflow("support").step("classify") { 1 })
            worker.claim(1)
            worker.drain()
            assertEquals("POST /v1/api/steps/lease-1/complete", h.paths().last())
            assertEquals(1, h.errors.size, h.errors.toString())
            assertContains(h.errors[0], "onStepStart hook threw")
            assertEquals(listOf("classify" to StepOutcome.Completed), hooks.ends.toList())
        }
    }

    @Test
    fun `stats count a completion a fatal failure a fenced conflict and an unservable lease`() = runBlocking {
        val script = listOf(
            claimed(lease()), started, acked,
            claimed(lease("lease_id" to "\"lease-2\"", "step_key" to "\"draft\"")), started, acked,
            claimed(lease("lease_id" to "\"lease-3\"", "step_key" to "\"send\"")), conflict("fenced", """, "attempt_status": "orphaned""""),
            claimed(lease("lease_id" to "\"lease-4\"", "definition_id" to "\"billing\"")), acked,
        )
        Harness(script).use { h ->
            h.worker.register(
                Workflow("support")
                    .step("classify") { 1 }
                    .step("draft") { throw IllegalStateException("nothing came back") }
                    .step("send") { 1 },
            )
            repeat(4) { h.claimAndDrain(1) }
            assertEquals(
                WorkerStats(claimed = 4, inflight = 0, completed = 1, failed = mapOf("fatal" to 1), conflicts = mapOf("fenced" to 1), unservable = 1),
                h.worker.stats(),
            )
        }
    }

    private fun runBody(status: String, extra: String = "") =
        """{"run": {"run_id": "run-1", "project": "test", "definition_id": "support", "definition_version": 1, "status": "$status",
            "cursor": {"index": 2, "construct": "end"}, "correlation": {"project": "test"}, "trace_id": "$trace", "steps_used": 2,
            "cost_usd": 0.25, "root_run_id": "run-1", "depth": 0, "created_at": "2026-09-01T10:00:00Z", "updated_at": "2026-09-01T10:00:05Z"$extra},
            "attempts": [], "timers": [], "waits": []}"""
    private val ended = 200 to """{"events": [{"run_id": "run-1", "seq": 0, "kind": "run_ended", "at": "2026-09-01T10:00:05Z"}], "has_more": false}"""

    @Test
    fun `waitRun returns the output once the run ends`() = runBlocking {
        Harness(listOf(ended, 200 to runBody("succeeded", """, "output": {"answer": 42}"""))).use { h ->
            val result = h.mcpd.waitRun("run-1", poll = 1.milliseconds)
            assertEquals(RunStatus.SUCCEEDED, result.status)
            assertEquals(h.json("""{"answer": 42}"""), result.output)
            assertEquals(0.25, result.costUsd)
            assertEquals(trace, result.traceId)
        }
    }

    @Test
    fun `waitRun throws a typed exception carrying the run when it did not succeed`() = runBlocking {
        Harness(listOf(ended, 200 to runBody("failed", """, "error": "send: relay down""""))).use { h ->
            val e = assertFailsWith<RunFailedException> { h.mcpd.waitRun("run-1", poll = 1.milliseconds) }
            assertEquals(RunStatus.FAILED, e.runStatus)
            assertEquals("send: relay down", e.run.error)
            assertEquals("run run-1 failed: send: relay down", e.message)
        }
    }

    @Test
    fun `waitRun gives up after the timeout while the run is still going`() = runBlocking {
        Harness(listOf(), routes = mapOf("/events" to listOf(200 to """{"events": [], "has_more": false}"""))).use { h ->
            val e = assertFailsWith<McpdException> { h.mcpd.waitRun("run-1", timeout = 40.milliseconds, poll = 5.milliseconds) }
            assertFalse(e is RunFailedException)
            assertContains(e.message!!, "still running")
        }
    }

    @Test
    fun `startRunAndWait does both`() = runBlocking {
        val running = runBody("running")
        Harness(listOf(200 to Json.parseToJsonElement(running).jsonObject["run"].toString(), ended, 200 to runBody("succeeded", """, "output": "done""""))).use { h ->
            val result = h.mcpd.startRunAndWait("support", input = mapOf("a" to 1), poll = 1.milliseconds)
            assertEquals(h.json("\"done\""), result.output)
            assertEquals(h.json("""{"project": "test", "definition_id": "support", "input": {"a": 1}}"""), h.body(0))
        }
    }
}
