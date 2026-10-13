package dev.mcpd

import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.double
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** Model calls (contract 3.1) and redaction in the process (contract 2.1.1). */
class ModelCallTest {

    private val openaiChat = Json.parseToJsonElement(
        """
        {
          "id": "chatcmpl-1",
          "object": "chat.completion",
          "model": "gpt-4o-2024-08-06",
          "service_tier": "flex",
          "choices": [{"index": 0, "finish_reason": "stop", "message": {"role": "assistant", "content": "hi"}}],
          "usage": {
            "prompt_tokens": 1000,
            "completion_tokens": 200,
            "total_tokens": 1200,
            "prompt_tokens_details": {"cached_tokens": 600, "audio_tokens": 0},
            "completion_tokens_details": {"reasoning_tokens": 50}
          }
        }
        """
    )

    // A Map, to cover the second accepted form.
    private val anthropicMessage: Map<String, Any?> = mapOf(
        "id" to "msg_1",
        "type" to "message",
        "model" to "claude-sonnet-4-5",
        "stop_reason" to "end_turn",
        "usage" to mapOf(
            "input_tokens" to 100,
            "cache_read_input_tokens" to 900,
            "cache_creation_input_tokens" to 50,
            "cache_creation" to mapOf("ephemeral_5m_input_tokens" to 20, "ephemeral_1h_input_tokens" to 30),
            "output_tokens" to 40,
            "service_tier" to "batch",
        ),
    )

    @Test
    fun `maps OpenAI usage with the gross prompt split into text and cache`() {
        assertEquals(
            TokenUsage(
                inputTokens = 1000,
                outputTokens = 200,
                totalTokens = 1200,
                inputDetails = InputTokenDetails(text = 400, cacheRead = 600, audio = 0),
                outputDetails = OutputTokenDetails(reasoning = 50),
            ),
            usageFromOpenAI(openaiChat),
        )
        val responses = mapOf(
            "object" to "response",
            "usage" to mapOf("input_tokens" to 10, "output_tokens" to 5, "input_tokens_details" to mapOf("cached_tokens" to 4)),
        )
        assertEquals(
            TokenUsage(
                inputTokens = 10,
                outputTokens = 5,
                totalTokens = 15,
                inputDetails = InputTokenDetails(text = 6, cacheRead = 4),
            ),
            usageFromOpenAI(responses),
        )
    }

    @Test
    fun `maps Anthropic usage, whose input is net of the cache`() {
        assertEquals(
            TokenUsage(
                inputTokens = 1050,
                outputTokens = 40,
                totalTokens = 1090,
                inputDetails = InputTokenDetails(
                    text = 100,
                    cacheRead = 900,
                    cacheWrite = 50,
                    cacheWrite5m = 20,
                    cacheWrite1h = 30,
                ),
            ),
            usageFromAnthropic(anthropicMessage),
        )
    }

    @Test
    fun `reads nothing from a shape it does not know, and never throws`() {
        assertNull(usageFromOpenAI(null))
        assertNull(usageFromAnthropic(mapOf("usage" to mapOf("input_tokens" to "many"))))
        // A cyclic map cannot be converted; it reads as no usage, not a crash.
        val cyclic = HashMap<String, Any?>()
        cyclic["usage"] = cyclic
        assertNull(usageFromOpenAI(cyclic))
        val mcpd = client()
        val span = mcpd.trace("run")
        span.recordResponse(42).recordResponse(mapOf("usage" to "x")).recordResponse(cyclic)
        span.end()
    }

    @Test
    fun `records the model, usage, id, finish reasons and tier, and never the text`() = runBlocking {
        val recorder = RecordingSink()
        val mcpd = client { it.copy(sink = recorder) }
        mcpd.trace("a", SpanKind.MODEL_GENERATION).recordResponse(openaiChat).end()
        mcpd.trace("b", SpanKind.MODEL_GENERATION).recordResponse(anthropicMessage, "aws.bedrock").end()
        mcpd.flush()
        val a = recorder.spans.last { it.name == "a" }
        val b = recorder.spans.last { it.name == "b" }
        assertEquals("gpt-4o-2024-08-06", a.model)
        assertEquals("openai", a.provider)
        assertNull(a.output)
        assertEquals(
            JsonObject(
                mapOf(
                    "gen_ai.response.id" to JsonPrimitive("chatcmpl-1"),
                    "gen_ai.response.finish_reasons" to JsonArray(listOf(JsonPrimitive("stop"))),
                    "gen_ai.response.service_tier" to JsonPrimitive("flex"),
                )
            ),
            a.attributes,
        )
        assertEquals(1000L, a.usage?.inputTokens)
        assertEquals("aws.bedrock", b.provider)
        assertEquals("batch", b.attributes?.jsonObject?.get("gen_ai.response.service_tier")?.jsonPrimitive?.content)
        mcpd.shutdown()
    }

    @Test
    fun `records the first token once, and a guardrail outcome`() = runBlocking {
        val recorder = RecordingSink()
        val mcpd = client { it.copy(sink = recorder) }
        val s = mcpd.trace("stream")
        s.markFirstToken()
        Thread.sleep(20)
        s.markFirstToken()
        s.setGuardrail(true, "pii in output").end()
        mcpd.flush()
        val attrs = assertNotNull(recorder.spans.last { it.name == "stream" }.attributes).jsonObject
        val ttft = attrs.getValue("mcpd.time_to_first_token_ms").jsonPrimitive.double
        assertTrue(ttft < 20, "the second call must not overwrite the first: $ttft")
        assertEquals(JsonPrimitive(true), attrs["mcpd.guardrail.tripped"])
        assertEquals("pii in output", attrs["mcpd.guardrail.reason"]?.jsonPrimitive?.content)
        mcpd.shutdown()
    }

    @Test
    fun `redact rewrites payloads before they are queued, and drops what it throws on`() = runBlocking {
        val recorder = RecordingSink()
        val card = Regex("\\d{4}-\\d{4}")
        val redact: (JsonElement) -> JsonElement? = { value ->
            if (value is JsonPrimitive && value.content.contains("boom")) error("redactor broke")
            Json.parseToJsonElement(value.toString().replace(card, "****"))
        }
        val mcpd = client { it.copy(sink = recorder, redact = redact) }
        val span = mcpd.trace("run", input = mapOf("card" to "1234-5678"))
        span.setOutput("boom").end()
        mcpd.log(LogLevel.INFO, "paid", mapOf("card" to "1234-5678"))
        mcpd.flush()
        val record = recorder.spans.last { it.name == "run" }
        assertEquals(JsonObject(mapOf("card" to JsonPrimitive("****"))), record.input)
        assertNull(record.output)
        assertEquals(JsonObject(mapOf("card" to JsonPrimitive("****"))), recorder.logs.single().fields)
        assertEquals(1L, mcpd.stats().redactionFailures)
        mcpd.shutdown()
    }

    @Test
    fun `redact drops a map field it turns into a non-object, and a null drops without counting`() = runBlocking {
        val recorder = RecordingSink()
        val redact: (JsonElement) -> JsonElement? = { value ->
            when {
                value is JsonObject && "secret" in value -> JsonPrimitive("gone")
                value is JsonPrimitive && value.content == "drop me" -> null
                else -> value
            }
        }
        val mcpd = client { it.copy(sink = recorder, redact = redact) }
        mcpd.trace("run", input = "drop me", attributes = mapOf("secret" to 1), metadata = mapOf("k" to "v"))
            .setOutput("drop me")
            .end()
        mcpd.log(LogLevel.INFO, "x", mapOf("secret" to 2))
        mcpd.flush()
        val record = recorder.spans.last { it.name == "run" }
        assertNull(record.input)
        assertNull(record.output)
        assertNull(record.attributes)
        assertEquals(JsonObject(mapOf("k" to JsonPrimitive("v"))), record.metadata)
        assertNull(recorder.logs.single().fields)
        // The attributes once on the start record and once on the end record,
        // which updates it in the queue, and the log's fields once.
        assertEquals(3L, mcpd.stats().redactionFailures)
        mcpd.shutdown()
    }
}
