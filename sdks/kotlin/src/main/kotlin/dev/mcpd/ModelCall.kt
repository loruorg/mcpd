package dev.mcpd

import kotlinx.coroutines.CancellationException
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.longOrNull
import kotlin.math.floor

/*
 * Model calls: what an OpenAI- or Anthropic-shaped response says about the
 * call that produced it, mapped onto mcpd's usage record (SDK contract 3.1).
 *
 * Nothing here imports a provider SDK. The response is read as a JsonElement
 * or a Map, so a parsed JSON body, a decoded map and a hand-built test fixture
 * all work, and a field that is missing or of the wrong type is left absent
 * rather than raised on.
 */

/**
 * The response as a JSON object, or null when it is not one.
 *
 * A Map is converted on the way in so one reader serves both forms. The
 * conversion can fail on a cyclic map, and these functions must never throw
 * (B1), so any failure reads as "not an object".
 */
private fun asObject(response: Any?): JsonObject? = try {
    when (response) {
        is JsonObject -> response
        is Map<*, *> -> toJsonElement(response) as? JsonObject
        else -> null
    }
} catch (e: CancellationException) {
    throw e
} catch (t: Throwable) {
    if (t is ThreadDeath) throw t
    null
}

private fun obj(v: JsonElement?): JsonObject? = v as? JsonObject

private fun int(v: JsonElement?): Long? {
    val p = v as? JsonPrimitive ?: return null
    // A quoted number is a string in JSON, and a token count sent as one is a
    // wrong type, so it is left absent like any other.
    if (p.isString) return null
    p.longOrNull?.let { return it.takeIf { n -> n >= 0 } }
    val d = p.doubleOrNull ?: return null
    return if (d.isFinite() && d >= 0) floor(d).toLong() else null
}

private fun str(v: JsonElement?): String? =
    (v as? JsonPrimitive)?.takeIf { it.isString }?.content?.takeIf { it.isNotEmpty() }

private fun InputTokenDetails.orNull(): InputTokenDetails? = takeIf { it != InputTokenDetails() }

private fun OutputTokenDetails.orNull(): OutputTokenDetails? = takeIf { it != OutputTokenDetails() }

private fun TokenUsage.orNull(): TokenUsage? = takeIf { it != TokenUsage() }

/**
 * Token usage from an OpenAI chat-completions or responses-API response,
 * given as a [JsonElement] or a `Map<String, Any?>`.
 *
 * The prompt total is gross, so the plain text share is the total less the
 * cached and audio tokens. Returns null when nothing can be read.
 */
fun usageFromOpenAI(response: Any?): TokenUsage? {
    val usage = obj(asObject(response)?.get("usage")) ?: return null
    val input = int(usage["prompt_tokens"]) ?: int(usage["input_tokens"])
    val output = int(usage["completion_tokens"]) ?: int(usage["output_tokens"])
    val inDetail = obj(usage["prompt_tokens_details"]) ?: obj(usage["input_tokens_details"])
    val outDetail = obj(usage["completion_tokens_details"]) ?: obj(usage["output_tokens_details"])
    val cached = int(inDetail?.get("cached_tokens"))
    val audioIn = int(inDetail?.get("audio_tokens"))
    val text = if (input != null && (cached != null || audioIn != null)) {
        maxOf(0L, input - (cached ?: 0) - (audioIn ?: 0))
    } else {
        null
    }
    return TokenUsage(
        inputTokens = input,
        outputTokens = output,
        totalTokens = int(usage["total_tokens"]) ?: if (input != null && output != null) input + output else null,
        inputDetails = InputTokenDetails(text = text, cacheRead = cached, audio = audioIn).orNull(),
        outputDetails = OutputTokenDetails(
            reasoning = int(outDetail?.get("reasoning_tokens")),
            audio = int(outDetail?.get("audio_tokens")),
        ).orNull(),
    ).orNull()
}

/**
 * Token usage from an Anthropic messages response, given as a [JsonElement]
 * or a `Map<String, Any?>`.
 *
 * `input_tokens` there is net of the cache, so it is the text share, and the
 * prompt's size is the sum of it and both cache meters. Returns null when
 * nothing can be read.
 */
fun usageFromAnthropic(response: Any?): TokenUsage? {
    val usage = obj(asObject(response)?.get("usage")) ?: return null
    val text = int(usage["input_tokens"])
    val read = int(usage["cache_read_input_tokens"])
    val write = int(usage["cache_creation_input_tokens"])
    val split = obj(usage["cache_creation"])
    val output = int(usage["output_tokens"])
    val input = if (text == null && read == null && write == null) null else (text ?: 0) + (read ?: 0) + (write ?: 0)
    return TokenUsage(
        inputTokens = input,
        outputTokens = output,
        totalTokens = if (input != null && output != null) input + output else null,
        inputDetails = InputTokenDetails(
            text = text,
            cacheRead = read,
            cacheWrite = write,
            cacheWrite5m = int(split?.get("ephemeral_5m_input_tokens")),
            cacheWrite1h = int(split?.get("ephemeral_1h_input_tokens")),
        ).orNull(),
    ).orNull()
}

/** Which provider shaped this response: `openai`, `anthropic`, or null when neither did. */
fun responseShape(response: Any?): String? {
    val r = asObject(response) ?: return null
    val usage = obj(r["usage"])
    val type = str(r["type"])
    val objectName = str(r["object"])
    return when {
        type == "message" || usage?.containsKey("cache_read_input_tokens") == true -> "anthropic"
        usage != null && ("prompt_tokens" in usage || "input_tokens_details" in usage) -> "openai"
        r["choices"] is JsonArray || objectName == "response" -> "openai"
        usage != null && "input_tokens" in usage -> "anthropic"
        else -> null
    }
}

/** What [ActiveSpan.recordResponse] sets on a span. */
data class ResponseFacts(
    val model: String? = null,
    val provider: String? = null,
    val usage: TokenUsage? = null,
    val attributes: Map<String, JsonElement> = emptyMap(),
)

/**
 * Everything a response says about its call: the model, the provider, the
 * usage, and the id, finish reasons and service tier as `gen_ai.response.*`
 * attributes. Never the text: that stays the caller's to record or not.
 */
fun responseFacts(response: Any?, provider: String? = null): ResponseFacts {
    val r = asObject(response) ?: JsonObject(emptyMap())
    val shape = responseShape(r)
    val attributes = LinkedHashMap<String, JsonElement>()
    str(r["id"])?.let { attributes["gen_ai.response.id"] = JsonPrimitive(it) }
    val reasons = ArrayList<String>()
    (r["choices"] as? JsonArray)?.forEach { choice -> str(obj(choice)?.get("finish_reason"))?.let(reasons::add) }
    str(r["stop_reason"])?.let(reasons::add)
    if (reasons.isEmpty() && str(r["object"]) == "response") str(r["status"])?.let(reasons::add)
    if (reasons.isNotEmpty()) attributes["gen_ai.response.finish_reasons"] = JsonArray(reasons.map(::JsonPrimitive))
    (str(r["service_tier"]) ?: str(obj(r["usage"])?.get("service_tier")))?.let {
        attributes["gen_ai.response.service_tier"] = JsonPrimitive(it)
    }
    val usage = when (shape) {
        "anthropic" -> usageFromAnthropic(r)
        "openai" -> usageFromOpenAI(r)
        else -> null
    }
    return ResponseFacts(model = str(r["model"]), provider = provider ?: shape, usage = usage, attributes = attributes)
}
