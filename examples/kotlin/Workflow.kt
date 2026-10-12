// A durable workflow in Kotlin: the graph goes to the daemon as a definition,
// the step bodies stay in this process for a Worker to serve.
//
// The regions are read into the documentation at build time.
import dev.mcpd.Backoff
import dev.mcpd.Effect
import dev.mcpd.Mcpd
import dev.mcpd.McpdConfig
import dev.mcpd.RetryPolicy
import dev.mcpd.RetryableException
import dev.mcpd.SpanKind
import dev.mcpd.StepOptions
import dev.mcpd.Worker
import dev.mcpd.Workflow
import dev.mcpd.getRun
import dev.mcpd.predicate
import dev.mcpd.publishEvent
import dev.mcpd.startRun
import dev.mcpd.watchRun
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlin.time.Duration.Companion.days
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds

fun deliver(text: String, idempotencyKey: String): Boolean =
    text.isNotEmpty() && idempotencyKey.isNotEmpty()

fun main() = runBlocking {
    val mcpd = Mcpd(McpdConfig(project = "support"))

    // #region workflow
    val support = Workflow("support_ticket")
        .step("classify") { ctx ->
            val ticket = ctx.input<JsonObject>()["ticket"]!!.jsonPrimitive.content
            mapOf("kind" to if (ticket.startsWith("Q")) "question" else "bug")
        }
        .agent("draft", StepOptions(input = mapOf("kind" to "\${steps.classify.output.kind}"))) {
            ctx ->
            // Your own model call, recorded under this step in the run's trace.
            // Its usage is summed into the step, so the daemon prices the run.
            ctx.span("model_call", SpanKind.MODEL_GENERATION) { span ->
                span.setModel("claude-sonnet-4", "anthropic")
                span.setUsage(inputTokens = 812, outputTokens = 37)
            }
            "an answer to a " + ctx.input<JsonObject>()["kind"]!!.jsonPrimitive.content
        }
        .waitFor(
            "approval", "approved", "\${input.ticket}", 7.days,
            StepOptions(guard = predicate("steps.classify.output.kind", "eq", "bug")),
        )
        .step(
            "send",
            StepOptions(
                input = mapOf("text" to "\${steps.draft.output}"),
                effect = Effect.AT_MOST_ONCE,
                retry = RetryPolicy(
                    maxAttempts = 3, backoff = Backoff.EXPONENTIAL, initial = 500.milliseconds,
                ),
            ),
        ) { ctx ->
            val text = ctx.input<JsonObject>()["text"]!!.jsonPrimitive.content
            if (!deliver(text, ctx.idempotencyKey)) {
                throw RetryableException("mail relay busy", retryAfter = 2.seconds)
            }
            mapOf("sent" to true)
        }
        .output(mapOf("sent" to "\${steps.send.output.sent}"))
    // #endregion workflow

    // #region worker
    // Serve step bodies for as long as this process lives: register, long-poll
    // for leases, heartbeat while a body runs, answer every 409 by its reason.
    val worker = Worker(mcpd, concurrency = 8).register(support)
    launch { worker.serve() }
    // #endregion worker

    // #region run
    // Start a run from anywhere that holds an execute key, answer the wait from
    // a webhook, then follow the journal to the end and read the run.
    val run = mcpd.startRun("support_ticket", input = mapOf("ticket" to "Q-42"))

    // A repeat of this event id is a duplicate, not a second event.
    mcpd.publishEvent("approved", "Q-42", payload = mapOf("by" to "ops"), eventId = "hook-1")

    mcpd.watchRun(run.runId) { event -> println("${event.seq} ${event.kind}") }
    val detail = mcpd.getRun(run.runId)
    println("${detail.run.status} ${detail.run.output}")
    // #endregion run

    worker.stop()
    mcpd.shutdown()
}
