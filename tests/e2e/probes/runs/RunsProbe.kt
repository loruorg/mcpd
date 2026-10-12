// The canonical run, served in Kotlin. Prints one JSON line at the end.
import dev.mcpd.Correlation
import dev.mcpd.Effect
import dev.mcpd.Backoff
import dev.mcpd.Mcpd
import dev.mcpd.McpdConfig
import dev.mcpd.RetryPolicy
import dev.mcpd.RetryableException
import dev.mcpd.RunStatus
import dev.mcpd.SpanKind
import dev.mcpd.StepOptions
import dev.mcpd.Worker
import dev.mcpd.Workflow
import dev.mcpd.getRun
import dev.mcpd.publishEvent
import dev.mcpd.startRun
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.File
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds

fun main() = runBlocking {
    val canonical = Json.parseToJsonElement(File("tests/e2e/probes/runs/canonical.json").readText()).jsonObject
    val definitionId = canonical["definition_id"]!!.jsonPrimitive.content
    val mcpd = Mcpd(McpdConfig(endpoint = System.getenv("MCPD_ENDPOINT"), project = System.getenv("MCPD_PROJECT")))

    val wf = Workflow(definitionId)
        .step("classify") { ctx ->
            val ticket = ctx.input<JsonObject>()["ticket"]!!.jsonPrimitive.content
            mapOf("kind" to if (ticket.startsWith("Q")) "question" else "bug")
        }
        .agent("draft", StepOptions(input = mapOf("kind" to "\${steps.classify.output.kind}"))) { ctx ->
            val kind = ctx.input<JsonObject>()["kind"]!!.jsonPrimitive.content
            ctx.setState(mapOf("drafted" to true))
            ctx.span("model_call", SpanKind.MODEL_GENERATION) { span ->
                span.setModel("claude-sonnet-4", "anthropic")
                span.setUsage(inputTokens = 120, outputTokens = 40)
            }
            "an answer to a $kind"
        }
        .waitFor("approval", "approved", "\${input.ticket}", 60.seconds, StepOptions(acceptParked = true))
        .step(
            "send",
            StepOptions(
                input = mapOf("text" to "\${steps.draft.output}", "by" to "\${steps.approval.output.by}"),
                effect = Effect.AT_MOST_ONCE,
                retry = RetryPolicy(maxAttempts = 2, backoff = Backoff.FIXED, initial = 100.milliseconds),
            ),
        ) { ctx ->
            if (ctx.attempt == 0) throw RetryableException("relay busy", retryAfter = 100.milliseconds)
            mapOf("sent" to true, "by" to ctx.input<JsonObject>()["by"]!!.jsonPrimitive.content)
        }
        .output(mapOf("answer" to "\${steps.draft.output}", "sent" to "\${steps.send.output.sent}", "by" to "\${steps.send.output.by}"))

    val worker = Worker(mcpd, workerId = "probe-kotlin", wait = 1.seconds, onError = { m, e -> throw IllegalStateException("$m: $e") }).register(wf)
    worker.registerDefinitions()

    val correlation = canonical["correlation"]!!.jsonObject
    val run = mcpd.startRun(
        definitionId,
        input = canonical["input"],
        correlation = Correlation(project = "", sessionId = correlation["session_id"]!!.jsonPrimitive.content, userId = correlation["user_id"]!!.jsonPrimitive.content),
    )
    val event = canonical["event"]!!.jsonObject
    val first = mcpd.publishEvent(event["name"]!!.jsonPrimitive.content, event["key"]!!.jsonPrimitive.content, payload = event["payload"], eventId = event["event_id"]!!.jsonPrimitive.content)
    val second = mcpd.publishEvent(event["name"]!!.jsonPrimitive.content, event["key"]!!.jsonPrimitive.content, payload = event["payload"], eventId = event["event_id"]!!.jsonPrimitive.content)

    var detail = mcpd.getRun(run.runId)
    repeat(60) {
        if (detail.run.status != RunStatus.RUNNING) return@repeat
        worker.claim(4, run.runId)
        worker.drain()
        detail = mcpd.getRun(run.runId)
        if (detail.run.status == RunStatus.RUNNING) delay(100)
    }
    mcpd.shutdown()
    println("""{"run_id":"${run.runId}","trace_id":"${run.traceId}","status":"${detail.run.status.wire}","publish":["${first.outcome.wire}","${second.outcome.wire}"]}""")
}
