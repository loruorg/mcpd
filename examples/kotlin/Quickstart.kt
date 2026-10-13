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
import dev.mcpd.PromptFallback
import dev.mcpd.SpanKind
import dev.mcpd.renderText
import dev.mcpd.recordStream
import dev.mcpd.testing.TestClient
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
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

    // A deploy script stores the prompt; posting the same body again stores nothing.
    mcpd.createPrompt(
        "support/answer",
        template = "Answer in a {{ tone }} tone: {{question}}",
        labels = listOf("production"),
    )

    // #region prompt
    // The version labelled production. The client caches it for a minute and
    // serves its last copy if mcpd is down; the fallback covers a first start
    // with no daemon at all.
    val prompt = mcpd.getPrompt(
        "support/answer",
        label = "production",
        fallback = PromptFallback.Template("Answer in a {{ tone }} tone: {{question}}"),
    )
    val text = prompt.renderText(mapOf("tone" to "friendly", "question" to "where is order 41?"))

    mcpd.span("draft_reply", SpanKind.MODEL_GENERATION) { generation ->
        // Links the call to the prompt version, so the trace shows which
        // version wrote the answer and listSpans(prompt = ...) finds it.
        generation.setPrompt(prompt)
        generation.setInput(listOf(mapOf("role" to "user", "content" to text)))
        generation.setOutput(callModel(text))
    }
    // #endregion prompt

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

    // A streamed call, and the unit test that checks what it records.
    println("streamed: ${streamAnswer(mcpd, "Is order 41 on its way?")}")
    checkStreamAnswer()

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
        generation.setInput(listOf(mapOf("role" to "user", "content" to question)))
        // Where a real provider call goes. This is the shape Anthropic answers in.
        val completion = callModel(question)
        generation.markFirstToken()
        val response = mapOf(
            "id" to "msg_01",
            "type" to "message",
            "model" to "claude-sonnet-4-5",
            "stop_reason" to "end_turn",
            "content" to listOf(mapOf("type" to "text", "text" to completion)),
            "usage" to mapOf(
                "input_tokens" to 540,
                "cache_read_input_tokens" to 640,
                "output_tokens" to 96,
            ),
        )
        generation.setOutput(completion)
        // The model, the token counts mcpd prices, the response id and the
        // finish reason, read from the response in one call.
        generation.recordResponse(response)
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

// #region stream
/**
 * A streamed model call. Collect the wrapper instead of the provider's flow:
 * every chunk comes through unchanged, and when the flow ends the span gets
 * the time to first token, the text, the usage and the finish reason.
 */
suspend fun streamAnswer(client: Mcpd, question: String): String =
    client.span("stream_answer", SpanKind.MODEL_GENERATION) { generation ->
        generation.setInput(listOf(mapOf("role" to "user", "content" to question)))
        // Where your provider's streaming call goes.
        val chunks = fakeChatStream("Your order is in transit.")
        val answer = StringBuilder()
        generation.recordStream(chunks).collect { chunk ->
            answer.append(chunk.deltaText())
        }
        answer.toString()
    }
// #endregion stream

// #region testing
/**
 * A unit test. Pass `test.client` where your code takes an `Mcpd`, then read
 * back what it recorded: no daemon, no port, no flush.
 */
suspend fun checkStreamAnswer() {
    val test = TestClient()
    val answer = streamAnswer(test.client, "Where is my order 41?")

    val call = test.span("stream_answer")
    check(call.output?.jsonPrimitive?.content == answer)
    check(call.model == "gpt-4o-mini")
    check(call.usage?.outputTokens == 6L)

    val run = test.trees().single()
    check(run.span.name == "stream_answer")
    test.shutdown()
}
// #endregion testing

/** Stand-in for a provider's stream, in the shape OpenAI streams chat completions. */
private fun fakeChatStream(text: String): Flow<JsonObject> = flow {
    val base = """"id": "chatcmpl-01", "model": "gpt-4o-mini""""
    for (word in text.split(Regex("(?= )"))) {
        delay(5)
        val content = JsonPrimitive(word)
        emit(chunk("""{$base, "choices": [{"index": 0, "delta": {"content": $content}}]}"""))
    }
    emit(chunk("""{$base, "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}"""))
    val usage = """{"prompt_tokens": 24, "completion_tokens": 6, "total_tokens": 30}"""
    emit(chunk("""{$base, "choices": [], "usage": $usage}"""))
}

private fun chunk(json: String): JsonObject = Json.parseToJsonElement(json).jsonObject

/** The text a chat-completion chunk adds, or nothing. */
private fun JsonObject.deltaText(): String {
    val choice = this["choices"]?.jsonArray?.firstOrNull()?.jsonObject
    return choice?.get("delta")?.jsonObject?.get("content")?.jsonPrimitive?.contentOrNull ?: ""
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
