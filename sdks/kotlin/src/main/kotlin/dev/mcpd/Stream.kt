package dev.mcpd

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.longOrNull
import java.util.concurrent.atomic.AtomicBoolean

/*
 * Streamed model calls (SDK contract 3.2).
 *
 * A streamed response spreads the facts recordResponse reads across its
 * chunks: the id and model in the first, the text in the middle, the finish
 * reason and the usage at the end. This collects them one chunk at a time and
 * builds the response the provider would have sent without streaming, so the
 * span records what it would have recorded for the same call made the other
 * way.
 *
 * Nothing here throws on a chunk it does not understand, and nothing grows
 * without bound: text and tool-call arguments share one budget, and tool calls
 * are capped by count.
 */

/** Unicode code points of text and tool-call arguments kept, together. */
const val STREAM_TEXT_BUDGET: Int = 32_768

/** Tool calls kept. Later ones are ignored. */
const val STREAM_MAX_TOOL_CALLS: Int = 64

private val ANTHROPIC_EVENTS = setOf("message_start", "content_block_start", "content_block_delta", "message_delta")

private fun streamObj(v: JsonElement?): JsonObject? = v as? JsonObject

private fun streamStr(v: JsonElement?): String? =
    (v as? JsonPrimitive)?.takeIf { it.isString }?.content?.takeIf { it.isNotEmpty() }

private fun streamIndex(v: JsonElement?, fallback: Int): Int {
    val p = v as? JsonPrimitive ?: return fallback
    if (p.isString) return fallback
    val n = p.longOrNull ?: p.doubleOrNull?.takeIf { it.isFinite() && it == Math.floor(it) }?.toLong()
    return if (n != null && n in 0..Int.MAX_VALUE) n.toInt() else fallback
}

/**
 * A chunk as a JSON object, or null when it is not one.
 *
 * A Map is converted so one reader serves both forms. The conversion can fail
 * on a cyclic map, which reads as "not a chunk" like any other unknown shape.
 */
private fun chunkObject(chunk: Any?): JsonObject? = try {
    when (chunk) {
        is JsonObject -> chunk
        is Map<*, *> -> toJsonElement(chunk) as? JsonObject
        else -> null
    }
} catch (e: CancellationException) {
    throw e
} catch (t: Throwable) {
    if (t is ThreadDeath) throw t
    null
}

private class StreamToolCall {
    var id: String? = null
    var name: String? = null
    val arguments = StringBuilder()
}

/** What one stream has said so far. Read from one collector at a time. */
internal class StreamAccumulator {
    /** `openai` or `anthropic` once a chunk was recognised. */
    var shape: String? = null
        private set

    /** Whether text or a tool call was dropped to keep memory bounded. */
    var truncated: Boolean = false
        private set

    private var id: String? = null
    private var model: String? = null
    private var tier: String? = null
    private val text = StringBuilder()
    private var budget = STREAM_TEXT_BUDGET
    private val tools = HashMap<Int, StreamToolCall>()
    private val finish = HashMap<Int, String>()
    private var stop: String? = null
    private var usage: JsonObject? = null

    /** Fold one chunk in. Answers whether it carried text or a tool-call delta. */
    fun add(chunk: Any?): Boolean {
        val c = chunkObject(chunk) ?: return false
        // A typed chunk with no choices can serialise them as null.
        if (c["choices"] is JsonArray || c["choices"] is JsonNull) return openai(c)
        val type = streamStr(c["type"])
        if (type != null && type in ANTHROPIC_EVENTS) return anthropic(c, type)
        return false
    }

    private fun openai(c: JsonObject): Boolean {
        if (shape == null) shape = "openai"
        if (id == null) id = streamStr(c["id"])
        if (model == null) model = streamStr(c["model"])
        if (tier == null) tier = streamStr(c["service_tier"])
        streamObj(c["usage"])?.let { usage = it }
        var content = false
        ((c["choices"] as? JsonArray) ?: JsonArray(emptyList())).forEachIndexed { position, raw ->
            val choice = streamObj(raw) ?: return@forEachIndexed
            val at = streamIndex(choice["index"], position)
            streamStr(choice["finish_reason"])?.let { finish[at] = it }
            if (at != 0) return@forEachIndexed
            val delta = streamObj(choice["delta"]) ?: return@forEachIndexed
            streamStr(delta["content"])?.let {
                content = true
                text.append(take(it))
            }
            (delta["tool_calls"] as? JsonArray)?.forEachIndexed { i, rawCall ->
                val call = streamObj(rawCall) ?: return@forEachIndexed
                content = true
                val tool = tool(streamIndex(call["index"], i)) ?: return@forEachIndexed
                val fn = streamObj(call["function"])
                if (tool.id == null) tool.id = streamStr(call["id"])
                if (tool.name == null) tool.name = streamStr(fn?.get("name"))
                streamStr(fn?.get("arguments"))?.let { tool.arguments.append(take(it)) }
            }
        }
        return content
    }

    private fun anthropic(c: JsonObject, type: String): Boolean {
        if (shape == null) shape = "anthropic"
        when (type) {
            "message_start" -> {
                val message = streamObj(c["message"])
                if (id == null) id = streamStr(message?.get("id"))
                if (model == null) model = streamStr(message?.get("model"))
                streamObj(message?.get("usage"))?.let { usage = it }
                return false
            }
            "content_block_start" -> {
                val block = streamObj(c["content_block"])
                if (streamStr(block?.get("type")) == "tool_use") {
                    tool(streamIndex(c["index"], tools.size))?.let { tool ->
                        if (tool.id == null) tool.id = streamStr(block?.get("id"))
                        if (tool.name == null) tool.name = streamStr(block?.get("name"))
                    }
                    return true
                }
                val start = streamStr(block?.get("text")) ?: return false
                text.append(take(start))
                return true
            }
            "content_block_delta" -> {
                val delta = streamObj(c["delta"])
                when (streamStr(delta?.get("type"))) {
                    "text_delta" -> {
                        val more = streamStr(delta?.get("text")) ?: return false
                        text.append(take(more))
                        return true
                    }
                    "input_json_delta" -> {
                        val args = streamStr(delta?.get("partial_json")) ?: return false
                        tools[streamIndex(c["index"], -1)]?.arguments?.append(take(args))
                        return true
                    }
                }
                return false
            }
            "message_delta" -> {
                streamStr(streamObj(c["delta"])?.get("stop_reason"))?.let { stop = it }
                streamObj(c["usage"])?.let { delta ->
                    // A field message_delta reports replaces the one from
                    // message_start; one it leaves out keeps its first value.
                    val merged = LinkedHashMap(usage ?: emptyMap())
                    delta.forEach { (key, value) -> if (value !is JsonNull) merged[key] = value }
                    usage = JsonObject(merged)
                }
                return false
            }
        }
        return false
    }

    /** The tool call at [at], made on first sight, or null past the cap. */
    private fun tool(at: Int): StreamToolCall? {
        tools[at]?.let { return it }
        if (tools.size >= STREAM_MAX_TOOL_CALLS) {
            truncated = true
            return null
        }
        return StreamToolCall().also { tools[at] = it }
    }

    /** As much of [s] as the budget still allows, counted in code points. */
    private fun take(s: String): String {
        if (budget <= 0) {
            truncated = true
            return ""
        }
        val n = s.codePointCount(0, s.length)
        if (n <= budget) {
            budget -= n
            return s
        }
        val cut = s.offsetByCodePoints(0, budget)
        truncated = true
        budget = 0
        return s.substring(0, cut)
    }

    /** The response the provider would have sent, or null when no chunk was recognised. */
    fun response(): JsonObject? {
        val out = LinkedHashMap<String, JsonElement>()
        fun put(key: String, value: String?) {
            if (value != null) out[key] = JsonPrimitive(value)
        }
        when (shape) {
            "openai" -> {
                out["object"] = JsonPrimitive("chat.completion")
                put("id", id)
                put("model", model)
                put("service_tier", tier)
                out["choices"] = JsonArray(
                    finish.entries.sortedBy { it.key }.map { (i, reason) ->
                        JsonObject(mapOf("index" to JsonPrimitive(i), "finish_reason" to JsonPrimitive(reason)))
                    }
                )
            }
            "anthropic" -> {
                out["type"] = JsonPrimitive("message")
                put("id", id)
                put("model", model)
                put("stop_reason", stop)
            }
            else -> return null
        }
        usage?.let { out["usage"] = it }
        return JsonObject(out)
    }

    /** The text, or the text and the tool calls, or null when neither arrived. */
    fun output(): JsonElement? {
        if (tools.isEmpty()) return if (text.isNotEmpty()) JsonPrimitive(text.toString()) else null
        val calls = tools.entries.sortedBy { it.key }.map { (_, t) ->
            val call = LinkedHashMap<String, JsonElement>()
            t.id?.let { call["id"] = JsonPrimitive(it) }
            t.name?.let { call["name"] = JsonPrimitive(it) }
            call["arguments"] = JsonPrimitive(t.arguments.toString())
            JsonObject(call)
        }
        return JsonObject(mapOf("content" to JsonPrimitive(text.toString()), "tool_calls" to JsonArray(calls)))
    }
}

/** Folds chunks into one span and finishes it once. */
private class StreamRecorder(private val span: ActiveSpan, private val provider: String?) {
    private val acc = StreamAccumulator()
    private val done = AtomicBoolean(false)

    fun observe(chunk: Any?) {
        // An unsampled span records nothing, so reading its chunks would be wasted work.
        if (!span.recorded) return
        span.client.guard("span-record-stream") { if (acc.add(chunk)) span.markFirstToken() }
    }

    fun finish(failure: Throwable?) {
        if (!done.compareAndSet(false, true)) return
        span.client.guard("span-record-stream") {
            acc.response()?.let { span.recordResponse(it, provider) }
            acc.output()?.let { span.setOutput(it) }
            if (acc.truncated) span.setAttribute("mcpd.stream.truncated", true)
            if (failure != null) span.setError(failure)
        }
        span.end()
    }
}

/**
 * Record a streamed model call: read the returned flow in place of [flow].
 *
 * Every chunk comes through unchanged and in order. A chunk is an
 * OpenAI chat-completion chunk or an Anthropic message event, as a
 * [JsonElement] or a `Map<String, Any?>`; anything else passes through and is
 * not read. The first chunk with text or a tool call marks the time to first
 * token. When the flow completes, throws, or the collector stops early (with
 * `take`, `first`, an exception of its own or cancellation), the span gets
 * what [ActiveSpan.recordResponse] would record for the same call unstreamed,
 * the text as its output, and ends. An exception from [flow] also marks the
 * span failed and reaches you unchanged.
 *
 * The span is ended here, so do not end it yourself. Inside `Mcpd.span { }`
 * the block ends it too, which is harmless: a span ends once.
 */
fun <T> ActiveSpan.recordStream(flow: Flow<T>, provider: String? = null): Flow<T> = flow {
    val recorder = StreamRecorder(this@recordStream, provider)
    // Which side an exception came from decides whether the span failed: one
    // from the provider's flow did, one from the collector (take, first, its
    // own code, cancellation) means the caller stopped reading.
    var downstream: Throwable? = null
    try {
        flow.collect { chunk ->
            recorder.observe(chunk)
            try {
                emit(chunk)
            } catch (t: Throwable) {
                downstream = t
                throw t
            }
        }
    } catch (t: Throwable) {
        val fromCollector = downstream === t
        recorder.finish(if (fromCollector || t is CancellationException) null else t)
        throw t
    } finally {
        recorder.finish(null)
    }
}
