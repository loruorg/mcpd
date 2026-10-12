// The canonical run, in Kotlin. Prints its trace id on the last line.
import dev.mcpd.FeedbackKind
import dev.mcpd.LogLevel
import dev.mcpd.Mcpd
import dev.mcpd.McpdConfig
import dev.mcpd.MetricKind
import dev.mcpd.ScoreSource
import dev.mcpd.SpanKind
import kotlinx.coroutines.runBlocking

fun main() = runBlocking {
    val mcpd = Mcpd(
        McpdConfig(
            endpoint = System.getenv("MCPD_ENDPOINT"),
            project = System.getenv("MCPD_PROJECT"),
        )
    )

    val traceId = mcpd.span(
        "support_agent",
        SpanKind.AGENT_RUN,
        input = "where is my order 41?",
        sessionId = "sess-conformance",
        threadId = "thread-7",
        userId = "user-41",
    ) { run ->
        // `span` takes no requestId or release, so they are set on the span
        // itself. It has to happen before the children start: a child copies
        // its parent's correlation once, at the moment it is created.
        run.setCorrelation(requestId = "req-9001", release = "2026.08.1")

        mcpd.span("plan_answer", SpanKind.MODEL_GENERATION) { gen ->
            gen.setModel("gpt-4o", provider = "openai")
            gen.setUsage(inputTokens = 1180, outputTokens = 96)
        }
        mcpd.span("lookup_order", SpanKind.TOOL_CALL) { }
        try {
            mcpd.span("estimate_delivery", SpanKind.TOOL_CALL) {
                throw IllegalStateException("shipping service returned 503")
            }
        } catch (e: IllegalStateException) {
            // recorded on the span; the run continues
        }

        mcpd.log(
            LogLevel.WARN,
            "estimate_delivery failed, answering from the cached estimate",
            mapOf("step" to "estimate_delivery"),
        )
        mcpd.metric(
            "conformance.run_steps",
            3.0,
            MetricKind.COUNTER,
            labels = mapOf("outcome" to "degraded", "lang" to "kotlin"),
            unit = "{step}",
        )

        run.traceId
    }

    mcpd.score(
        traceId = traceId, scorerId = "answer_relevance", value = 0.86,
        min = 0.0, max = 1.0, source = ScoreSource.LIVE,
    )
    // No source: the suite compares what each SDK defaults to.
    mcpd.score(traceId = traceId, scorerId = "scale_check", value = 3.0, min = 1.0, max = 5.0)
    mcpd.feedback(traceId = traceId, kind = FeedbackKind.THUMBS, value = "up")
    mcpd.shutdown()
    println(traceId)
}
