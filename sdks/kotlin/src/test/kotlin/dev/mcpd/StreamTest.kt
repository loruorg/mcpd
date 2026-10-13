package dev.mcpd

import dev.mcpd.testing.TestClient
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.onEach
import kotlinx.coroutines.flow.take
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.yield
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull
import kotlin.test.assertSame
import kotlin.test.assertTrue

/** Streamed model calls (contract 3.2). */
class StreamTest {

    private fun json(text: String): JsonElement = Json.parseToJsonElement(text)

    private val openaiChunks: List<Any?> = listOf(
        json("""{"id":"chatcmpl-1","object":"chat.completion.chunk","model":"gpt-4o","choices":[{"index":0,"delta":{"role":"assistant"}}]}"""),
        json("""{"id":"chatcmpl-1","object":"chat.completion.chunk","model":"gpt-4o","choices":[{"index":0,"delta":{"content":"Hel"}}]}"""),
        json("""{"id":"chatcmpl-1","object":"chat.completion.chunk","model":"gpt-4o","choices":[{"index":0,"delta":{"content":"lo"}}]}"""),
        json("""{"id":"chatcmpl-1","object":"chat.completion.chunk","model":"gpt-4o","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"""),
        json(
            """{"id":"chatcmpl-1","object":"chat.completion.chunk","model":"gpt-4o","service_tier":"flex","choices":[],
            "usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12,"prompt_tokens_details":{"cached_tokens":4}}}"""
        ),
    )

    // Maps, to cover the second accepted form.
    private val anthropicEvents: List<Any?> = listOf(
        mapOf(
            "type" to "message_start",
            "message" to mapOf(
                "id" to "msg_1",
                "type" to "message",
                "model" to "claude-sonnet-4-5",
                "usage" to mapOf("input_tokens" to 20, "cache_read_input_tokens" to 80, "output_tokens" to 1),
            ),
        ),
        mapOf("type" to "content_block_start", "index" to 0, "content_block" to mapOf("type" to "text", "text" to "")),
        mapOf("type" to "content_block_delta", "index" to 0, "delta" to mapOf("type" to "text_delta", "text" to "Checking")),
        mapOf("type" to "content_block_stop", "index" to 0),
        mapOf(
            "type" to "content_block_start",
            "index" to 1,
            "content_block" to mapOf("type" to "tool_use", "id" to "toolu_1", "name" to "lookup", "input" to emptyMap<String, Any>()),
        ),
        mapOf("type" to "content_block_delta", "index" to 1, "delta" to mapOf("type" to "input_json_delta", "partial_json" to "{\"id\":")),
        mapOf("type" to "content_block_delta", "index" to 1, "delta" to mapOf("type" to "input_json_delta", "partial_json" to "\"A-1\"}")),
        mapOf("type" to "message_delta", "delta" to mapOf("stop_reason" to "tool_use"), "usage" to mapOf("output_tokens" to 15)),
        mapOf("type" to "message_stop"),
    )

    private class ConnectionReset : RuntimeException("connection reset")

    private fun <T> from(items: List<T>, failAfter: Int? = null): Flow<T> = flow {
        items.forEachIndexed { i, item ->
            if (failAfter != null && i == failAfter) throw ConnectionReset()
            emit(item)
        }
    }

    private fun attrs(span: Span): JsonObject = span.attributes as? JsonObject ?: JsonObject(emptyMap())

    @Test
    fun `an OpenAI stream records what recordResponse would, and every chunk passes through`() = runBlocking {
        val test = TestClient()
        val span = test.client.trace("call", SpanKind.MODEL_GENERATION)
        val input = openaiChunks + "not a chunk" + 42
        val seen = span.recordStream(from(input)).toList()
        assertEquals(input, seen)
        assertSame(input[1], seen[1])

        val s = test.span("call")
        assertEquals("gpt-4o", s.model)
        assertEquals("openai", s.provider)
        assertEquals(JsonPrimitive("Hello"), s.output)
        assertEquals(
            TokenUsage(
                inputTokens = 10,
                outputTokens = 2,
                totalTokens = 12,
                inputDetails = InputTokenDetails(text = 6, cacheRead = 4),
            ),
            s.usage,
        )
        val a = attrs(s)
        assertEquals(JsonPrimitive("chatcmpl-1"), a["gen_ai.response.id"])
        assertEquals(JsonArray(listOf(JsonPrimitive("stop"))), a["gen_ai.response.finish_reasons"])
        assertEquals(JsonPrimitive("flex"), a["gen_ai.response.service_tier"])
        assertTrue(a["mcpd.time_to_first_token_ms"]!!.jsonPrimitive.content.toDouble() >= 0)
        assertNull(a["mcpd.stream.truncated"])
        assertEquals(SpanStatus.OK, s.status)
        test.shutdown()
    }

    @Test
    fun `Anthropic tool-call deltas are joined and message_delta usage is merged`() = runBlocking {
        val test = TestClient()
        val span = test.client.trace("call", SpanKind.MODEL_GENERATION)
        span.recordStream(from(anthropicEvents)).toList()
        val s = test.span("call")
        assertEquals("claude-sonnet-4-5", s.model)
        assertEquals("anthropic", s.provider)
        assertEquals(
            json("""{"content":"Checking","tool_calls":[{"id":"toolu_1","name":"lookup","arguments":"{\"id\":\"A-1\"}"}]}"""),
            s.output,
        )
        assertEquals(100L, s.usage?.inputTokens)
        assertEquals(15L, s.usage?.outputTokens)
        assertEquals(JsonPrimitive("msg_1"), attrs(s)["gen_ai.response.id"])
        assertEquals(JsonArray(listOf(JsonPrimitive("tool_use"))), attrs(s)["gen_ai.response.finish_reasons"])
        test.shutdown()
    }

    @Test
    fun `OpenAI tool-call deltas are joined by index`() = runBlocking {
        val test = TestClient()
        val span = test.client.trace("call")
        fun call(delta: String) = json("""{"choices":[{"index":0,"delta":{"tool_calls":[$delta]}}]}""")
        span.recordStream(
            from(
                listOf(
                    call("""{"index":0,"id":"call_1","function":{"name":"lookup","arguments":"{\"a\""}}"""),
                    call("""{"index":0,"function":{"arguments":":1}"}}"""),
                    json("""{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"""),
                )
            )
        ).toList()
        assertEquals(
            json("""{"content":"","tool_calls":[{"id":"call_1","name":"lookup","arguments":"{\"a\":1}"}]}"""),
            test.span("call").output,
        )
        test.shutdown()
    }

    @Test
    fun `the first token is marked once, at the first content chunk`() = runBlocking {
        val test = TestClient()
        val span = test.client.trace("call")
        val gate = Channel<Unit>()
        val marks = mutableListOf<JsonElement?>()
        val upstream = flow {
            for (chunk in openaiChunks) {
                emit(chunk)
                gate.receive()
            }
        }
        val reader = launch {
            span.recordStream(upstream).collect { }
        }
        // After the role-only chunk nothing is marked; after the first text it is.
        yield()
        marks += readFirstToken(span)
        gate.send(Unit)
        yield()
        marks += readFirstToken(span)
        val first = marks[1]
        repeat(openaiChunks.size - 1) { gate.send(Unit) }
        reader.join()
        assertNull(marks[0])
        assertTrue(first != null)
        assertEquals(first, attrs(test.span("call"))["mcpd.time_to_first_token_ms"])
        test.shutdown()
    }

    /** The live span's first-token attribute, read without ending it. */
    private fun readFirstToken(span: ActiveSpan): JsonElement? =
        (span.toRecord(kotlin.time.Clock.System.now()).attributes as? JsonObject)?.get("mcpd.time_to_first_token_ms")

    @Test
    fun `a stream that throws ends the span with the error, and the error reaches the caller unchanged`() = runBlocking {
        val test = TestClient()
        val span = test.client.trace("call")
        val thrown = assertFailsWith<ConnectionReset> { span.recordStream(from(openaiChunks, failAfter = 3)).toList() }
        assertEquals("connection reset", thrown.message)
        val s = test.span("call")
        assertEquals(SpanStatus.ERROR, s.status)
        assertEquals("connection reset", s.error?.message)
        assertEquals(JsonPrimitive("Hello"), s.output)
        test.shutdown()
    }

    @Test
    fun `a caller that stops early with take still ends the span, as ok`() = runBlocking {
        val test = TestClient()
        val span = test.client.trace("call")
        val read = span.recordStream(from(openaiChunks)).take(2).toList()
        assertEquals(openaiChunks.take(2), read)
        val s = test.span("call")
        assertEquals(JsonPrimitive("Hel"), s.output)
        assertEquals(SpanStatus.OK, s.status)
        assertNull(s.error)
        test.shutdown()
    }

    @Test
    fun `first ends the span as ok`() = runBlocking {
        val test = TestClient()
        val span = test.client.trace("call")
        assertSame(openaiChunks[0], span.recordStream(from(openaiChunks)).first())
        assertEquals(SpanStatus.OK, test.span("call").status)
        test.shutdown()
    }

    @Test
    fun `an exception in the collector ends the span as ok and reaches the caller`() = runBlocking {
        val test = TestClient()
        val span = test.client.trace("call")
        assertFailsWith<IllegalStateException> {
            span.recordStream(from(openaiChunks)).onEach { error("the caller's own failure") }.toList()
        }
        val s = test.span("call")
        assertEquals(SpanStatus.OK, s.status)
        assertNull(s.error)
        test.shutdown()
    }

    @Test
    fun `a cancelled collector ends the span as ok`() = runBlocking {
        val test = TestClient()
        val span = test.client.trace("call")
        val upstream = flow {
            emit(openaiChunks[1])
            awaitCancellation()
        }
        val started = Channel<Unit>(1)
        val reader = launch { span.recordStream(upstream).collect { started.send(Unit) } }
        started.receive()
        reader.cancelAndJoin()
        val s = test.span("call")
        assertEquals(SpanStatus.OK, s.status)
        assertEquals(JsonPrimitive("Hel"), s.output)
        test.shutdown()
    }

    @Test
    fun `text past the budget is cut and the span says so`() = runBlocking {
        val test = TestClient()
        val span = test.client.trace("call")
        val big = "😀".repeat(STREAM_TEXT_BUDGET - 1) + "ab"
        span.recordStream(from(listOf(mapOf("choices" to listOf(mapOf("index" to 0, "delta" to mapOf("content" to big))))))).toList()
        val s = test.span("call")
        val out = s.output!!.jsonPrimitive.content
        assertEquals(STREAM_TEXT_BUDGET, out.codePointCount(0, out.length))
        assertTrue(out.endsWith("😀a"))
        assertEquals(JsonPrimitive(true), attrs(s)["mcpd.stream.truncated"])
        test.shutdown()
    }

    @Test
    fun `tool calls past the cap are ignored and the span says so`() = runBlocking {
        val test = TestClient()
        val span = test.client.trace("call")
        val chunks = (0..STREAM_MAX_TOOL_CALLS).map { i ->
            json("""{"choices":[{"index":0,"delta":{"tool_calls":[{"index":$i,"id":"c$i","function":{"name":"f","arguments":"{}"}}]}}]}""")
        }
        span.recordStream(from(chunks)).toList()
        val s = test.span("call")
        assertEquals(STREAM_MAX_TOOL_CALLS, ((s.output as JsonObject)["tool_calls"] as JsonArray).size)
        assertEquals(JsonPrimitive(true), attrs(s)["mcpd.stream.truncated"])
        test.shutdown()
    }

    @Test
    fun `payload capture off keeps the output in the process`() = runBlocking {
        val test = TestClient(McpdConfig(capturePayloads = false))
        val span = test.client.trace("call")
        span.recordStream(from(openaiChunks)).toList()
        val s = test.span("call")
        assertNull(s.output)
        assertEquals("gpt-4o", s.model)
        test.shutdown()
    }

    @Test
    fun `inside span the stream ends the span once`() = runBlocking {
        val test = TestClient()
        val count = test.client.span("call", SpanKind.MODEL_GENERATION) { generation ->
            generation.recordStream(from(openaiChunks)).toList().size
        }
        assertEquals(openaiChunks.size, count)
        assertEquals(1, test.spans("call").size)
        assertEquals(JsonPrimitive("Hello"), test.span("call").output)
        test.shutdown()
    }
}
