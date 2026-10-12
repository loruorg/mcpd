package dev.mcpd

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertSame
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds

/** The workflow builder: the document it produces and the handlers it keeps (contract section 7.1). */
class WorkflowTest {
    private val body: StepHandler = { "done" }

    private fun parse(text: String): JsonElement = Json.parseToJsonElement(text)

    @Test
    fun `the document spells every construct the way the daemon reads it`() {
        val wf = Workflow(
            "support",
            version = 2,
            inputSchema = mapOf("type" to "object", "required" to listOf("ticket")),
            limits = Limits(maxSteps = 50, runTimeout = 60.seconds, maxCostUsd = 2.0),
        )
            .step(
                "classify",
                StepOptions(
                    input = mapOf("ticket" to "\${input.ticket}"),
                    effect = Effect.AT_MOST_ONCE,
                    timeout = 5.seconds,
                    retry = RetryPolicy(maxAttempts = 3, backoff = Backoff.EXPONENTIAL, initial = 100.milliseconds, max = 1.seconds),
                ),
                body,
            )
            .agent("draft", StepOptions(guard = predicate("steps.classify.output.kind", "eq", "question"), onError = OnError.CONTINUE), body)
            .sleep("cool", 10.milliseconds)
            .sleepUntil("bill", "\${input.billing_date}")
            .waitFor("approval", "approved", "\${input.ticket}", 1.seconds, StepOptions(onTimeout = WaitTimeout.CONTINUE, acceptParked = true))
            .parallel("checks") { step("fraud", body = body).agent("policy", body = body) }
            .forEach("notify", "input.recipients", 10, StepOptions(concurrency = 2)) {
                step("send", StepOptions(input = mapOf("to" to "\${item}", "position" to "\${index}")), body)
            }
            .loop("refine", 3, StepOptions(until = predicate("steps.rewrite.output.done", "eq", true))) { step("rewrite", body = body) }
            .subworkflow(
                "escalate", "escalation",
                StepOptions(version = 1, input = mapOf("ticket" to "\${input.ticket}"), guard = not(exists("steps.checks.fraud.output.clean"))),
            )
            .output(mapOf("answer" to "\${steps.draft.output}"))

        val doc = wf.document()
        assertEquals("support", doc.id)
        assertEquals(2, doc.version)
        assertEquals(DefinitionKind.WORKFLOW, doc.kind)
        val want = parse(
            """
            {"input_schema": {"type": "object", "required": ["ticket"]},
             "limits": {"max_steps": 50, "run_timeout_ms": 60000, "max_cost_usd": 2.0},
             "output": {"answer": "${'$'}{steps.draft.output}"},
             "steps": [
               {"kind": "task", "id": "classify", "input": {"ticket": "${'$'}{input.ticket}"},
                "effect": "at_most_once", "step_timeout_ms": 5000,
                "retry": {"max_attempts": 3, "backoff": "exponential", "initial_ms": 100, "max_ms": 1000}},
               {"kind": "agent", "id": "draft",
                "when": {"path": "steps.classify.output.kind", "op": "eq", "value": "question"}, "on_error": "continue"},
               {"kind": "sleep", "id": "cool", "ms": 10},
               {"kind": "sleep_until", "id": "bill", "at": "${'$'}{input.billing_date}"},
               {"kind": "wait_event", "id": "approval", "event": "approved", "key": "${'$'}{input.ticket}",
                "timeout_ms": 1000, "accept_parked": true, "on_timeout": "continue"},
               {"kind": "parallel", "id": "checks",
                "branches": [{"kind": "task", "id": "fraud"}, {"kind": "agent", "id": "policy"}]},
               {"kind": "foreach", "id": "notify", "over": "input.recipients",
                "body": {"kind": "task", "id": "send", "input": {"to": "${'$'}{item}", "position": "${'$'}{index}"}},
                "max_items": 10, "concurrency": 2},
               {"kind": "loop", "id": "refine", "body": {"kind": "task", "id": "rewrite"}, "max_iterations": 3,
                "until": {"path": "steps.rewrite.output.done", "op": "eq", "value": true}},
               {"kind": "subworkflow", "id": "escalate", "definition": "escalation", "version": 1,
                "input": {"ticket": "${'$'}{input.ticket}"},
                "when": {"not": {"path": "steps.checks.fraud.output.clean", "op": "exists"}}}
             ]}
            """.trimIndent(),
        )
        assertEquals(want, doc.body)
    }

    @Test
    fun `what was not said is left out so the daemon applies its defaults`() {
        val doc = Workflow("plain").step("one", body = body).document()
        assertNull(doc.version)
        assertEquals(parse("""{"steps": [{"kind": "task", "id": "one"}]}"""), doc.body)
    }

    @Test
    fun `predicates combine the way the contract spells them`() {
        val got: JsonObject = allOf(exists("a"), anyOf(predicate("b", "gt", 1), not(predicate("c", "in", listOf(1, 2)))))
        assertEquals(
            parse("""{"all": [{"path": "a", "op": "exists"}, {"any": [{"path": "b", "op": "gt", "value": 1}, {"not": {"path": "c", "op": "in", "value": [1, 2]}}]}]}"""),
            got,
        )
    }

    @Test
    fun `an id the daemon would refuse is refused here`() {
        assertFailsWith<IllegalArgumentException> { Workflow("Support") }
        assertFailsWith<IllegalArgumentException> { Workflow("ok").step("9lives", body = body) }
        assertFailsWith<IllegalArgumentException> { Workflow("ok").sleep("has-dash", 1.seconds) }
    }

    @Test
    fun `a foreach or loop body is one claimable step`() {
        assertFailsWith<IllegalArgumentException> {
            Workflow("ok").forEach("each", "input.items", 1) { step("a", body = body).step("b", body = body) }
        }
        assertFailsWith<IllegalArgumentException> { Workflow("ok").loop("again", 1) { } }
    }

    @Test
    fun `handlerFor resolves a plain step a branch an item and an iteration`() {
        val classify: StepHandler = { "classify" }
        val fraud: StepHandler = { "fraud" }
        val send: StepHandler = { "send" }
        val rewrite: StepHandler = { "rewrite" }
        val wf = Workflow("support")
            .step("classify", body = classify)
            .parallel("checks") { step("fraud", body = fraud) }
            .forEach("notify", "input.to", 5) { step("send", body = send) }
            .loop("refine", 2) { step("rewrite", body = rewrite) }

        assertSame(classify, wf.handlerFor("classify"))
        assertSame(fraud, wf.handlerFor("checks/fraud"))
        assertSame(send, wf.handlerFor("notify[3]"))
        assertSame(send, wf.handlerFor("notify[3]/send"))
        assertSame(rewrite, wf.handlerFor("refine[1]"))
        assertSame(rewrite, wf.handlerFor("refine[1]/rewrite"))
        assertNotNull(wf.handlerFor("classify"))
        assertNull(wf.handlerFor("checks"))
        assertNull(wf.handlerFor("unknown"))
        assertNull(wf.handlerFor("checks/policy"))
    }
}
