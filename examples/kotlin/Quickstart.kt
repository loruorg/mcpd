// A small support agent, instrumented with mcpd.
//
// Nothing here calls a real model or a real database: the point is the shape of
// the instrumentation, not the agent. Run it against a local mcpd:
//
//     mcpd serve --data ./mcpd.db
//     ./gradlew run
//
// Then open http://localhost:4317 and read the run.

package examples

import dev.mcpd.FeedbackKind
import dev.mcpd.LogLevel
import dev.mcpd.Mcpd
import dev.mcpd.McpdConfig
import dev.mcpd.MetricKind
import dev.mcpd.SpanKind
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlin.time.Clock
import kotlin.time.Duration.Companion.hours

fun main() = runBlocking {
    // #region init
    // Every field also reads an environment variable (MCPD_ENDPOINT,
    // MCPD_PROJECT, MCPD_API_KEY, ...), so `Mcpd()` on its own works too.
    val mcpd = Mcpd(
        McpdConfig(
            endpoint = "http://localhost:4317",
            project = "support-desk",
            environment = "development",
            serviceName = "support-agent",
            serviceVersion = "1.4.0",
        )
    )
    // #endregion init

    // #region trace
    val question = "where is my order 41?"

    // `span` starts the operation, ends it, and records anything it throws.
    // Inside the block the span is the active one, so the helpers below find
    // their parent through the coroutine context rather than being handed it.
    val traceId = mcpd.span(
        "support_agent",
        SpanKind.AGENT_RUN,
        input = question,
        sessionId = "session-8f21",
        userId = "customer-4477",
        tags = listOf("support", "email"),
    ) { run ->
        mcpd.log(LogLevel.INFO, "handling a question", mapOf("channel" to "email"))

        val plan = planAnswer(mcpd, question)
        val order = lookUpOrder(mcpd, 41)
        val estimate = estimateDelivery(mcpd, 41)

        val answer = buildString {
            append(plan).append(' ').append(order)
            if (estimate != null) append(" It should arrive ").append(estimate).append('.')
        }
        run.setOutput(answer)
        mcpd.metric("support.answers", 1.0, MetricKind.COUNTER, unit = "{answer}")

        // The block's value is the value of `span`.
        run.traceId
    }
    // #endregion trace

    // #region score
    // A machine evaluation. `min` and `max` are required: 7 is excellent out of
    // 10 and poor out of 100, and a score with no scale cannot be compared.
    mcpd.score(
        traceId = traceId,
        scorerId = "answer_relevance",
        value = 0.86,
        min = 0.0,
        max = 1.0,
        reason = "answered the question asked, without the delivery estimate",
    )

    // A person's judgement, from the thumbs-up control in your own product.
    mcpd.feedback(
        traceId = traceId,
        kind = FeedbackKind.THUMBS,
        value = "up",
        comment = "fast and clear",
        authorId = "customer-4477",
    )

    // Nothing above touched the network. This is where it goes.
    mcpd.flush()
    // #endregion score

    // #region query
    val detail = mcpd.getTrace(traceId, include = listOf("scores", "feedback"))
    println("${detail.trace.name}: ${detail.spans.size} spans, ${detail.trace.status}")
    for (span in detail.spans) {
        val ms = (span.durationNs ?: 0) / 1_000_000
        println("  ${span.kind} ${span.name} ${ms}ms ${span.error?.message ?: ""}")
    }

    val summary = mcpd.analyticsSummary(
        from = Clock.System.now() - 1.hours,
        to = Clock.System.now(),
    )
    println(
        "last hour: ${summary.traceCount} runs, " +
            "${summary.errorCount} failed, ${summary.totalCost} USD",
    )
    // #endregion query

    // Send anything still queued and stop the worker. Without this, a process
    // that exits quickly loses its last batch.
    mcpd.shutdown()
}

// ---------------------------------------------------------------------------
// Each step opens its own span. None of them is passed a parent: the enclosing
// run is on the coroutine context, so the tree assembles itself.

/** A model call. Token usage is what mcpd turns into cost. */
private suspend fun planAnswer(mcpd: Mcpd, question: String): String =
    mcpd.span("plan_answer", SpanKind.MODEL_GENERATION) { generation ->
        generation.setModel("gpt-4o", provider = "openai")
        generation.setInput(listOf(mapOf("role" to "user", "content" to question)))
        val completion = callModel(question)
        generation.setOutput(completion)
        generation.setUsage(inputTokens = 1_180, outputTokens = 96)
        completion
    }

/** A tool call that works. */
private suspend fun lookUpOrder(mcpd: Mcpd, id: Int): String =
    mcpd.span("lookup_order", SpanKind.TOOL_CALL, input = mapOf("order_id" to id)) { tool ->
        tool.setAttribute("tool.name", "orders.lookup")
        val found = lookupOrder(id)
        tool.setOutput(found)
        found
    }

/**
 * A tool call that fails. The span is recorded as an error with the exception
 * on it, and the exception still reaches the caller, so the agent decides what
 * to do about it rather than mcpd deciding for it.
 */
private suspend fun estimateDelivery(mcpd: Mcpd, id: Int): String? =
    try {
        mcpd.span(
            "estimate_delivery",
            SpanKind.TOOL_CALL,
            input = mapOf("order_id" to id),
        ) { tool ->
            tool.setAttribute("tool.name", "shipping.estimate")
            estimateDelivery(id)
        }
    } catch (e: IllegalStateException) {
        mcpd.log(LogLevel.WARN, "the shipping service is down, answering without an estimate")
        null
    }

// ---------------------------------------------------------------------------
// The fake agent. Stand-ins for a model, a database and a flaky HTTP service.

private suspend fun callModel(question: String): String {
    delay(40)
    return "Let me check on \"$question\"."
}

private suspend fun lookupOrder(id: Int): String {
    delay(15)
    return "Order $id shipped on Tuesday."
}

private suspend fun estimateDelivery(id: Int): String {
    delay(10)
    throw IllegalStateException("shipping service returned 503 for order $id")
}
