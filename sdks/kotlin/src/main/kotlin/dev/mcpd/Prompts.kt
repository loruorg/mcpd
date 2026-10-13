package dev.mcpd

import kotlinx.coroutines.CancellationException
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import java.net.URLEncoder
import java.nio.charset.StandardCharsets
import kotlin.time.Clock
import kotlin.time.Duration

/** How many prompts one client keeps in memory before it evicts the oldest (B3). */
internal const val PROMPT_CACHE_SIZE = 256

/** The label read when a caller names neither a label nor a version. */
private const val DEFAULT_LABEL = "production"

/**
 * What [Mcpd.getPrompt] returns when the daemon cannot be read and nothing was
 * fetched before. It becomes a [Prompt] with version `0` and no labels, and it
 * is never cached, so the next call tries the daemon again.
 */
sealed class PromptFallback {
    /** A `text` prompt. */
    data class Template(val template: String) : PromptFallback()

    /**
     * A `chat` prompt. Each message is a map or a JSON object with `role` and
     * `content`, the same shape the daemon stores.
     */
    class Messages(messages: List<Any?>) : PromptFallback() {
        val messages: JsonArray = JsonArray(messages.map(::toJsonElement))
    }
}

/**
 * Rendering named variables the prompt uses and the caller did not give.
 * [missing] lists them in order of first use.
 */
class PromptRenderException(val missing: List<String>) :
    IllegalArgumentException("missing prompt variables: " + missing.joinToString(", "))

/**
 * Render a `text` prompt to the string to send.
 *
 * `{{name}}` and `{{ name }}` are replaced by `variables[name].toString()`, in
 * one pass, so a value that contains `{{x}}` is sent as written. Braces around
 * anything that is not a name, and a `{{` with no closing `}}`, are left as
 * written. A name the template uses with no value (or a null one) throws
 * [PromptRenderException]; extra variables are ignored.
 *
 * Throws [IllegalStateException] for a `chat` prompt: use [renderMessages].
 */
fun Prompt.renderText(variables: Map<String, Any?> = emptyMap()): String {
    val text = checkNotNull(template) {
        "prompt $name is a ${kind.wire} prompt with no template; use renderMessages"
    }
    val missing = ArrayList<String>()
    val out = substitute(text, variables, missing)
    if (missing.isNotEmpty()) throw PromptRenderException(missing)
    return out
}

/**
 * Render a `chat` prompt to a copy of its messages.
 *
 * Every `content` that is a string is rendered by the rules of [renderText];
 * every other field, and a `content` that is not a string, is copied unchanged.
 *
 * Throws [IllegalStateException] for a `text` prompt: use [renderText].
 */
fun Prompt.renderMessages(variables: Map<String, Any?> = emptyMap()): JsonArray {
    val list = messages as? JsonArray
        ?: throw IllegalStateException("prompt $name is a ${kind.wire} prompt with no messages; use renderText")
    val missing = ArrayList<String>()
    val out = list.map { message ->
        val content = (message as? JsonObject)?.get("content") as? JsonPrimitive
        if (content == null || !content.isString) return@map message
        val rendered = substitute(content.content, variables, missing)
        JsonObject((message as JsonObject) + ("content" to JsonPrimitive(rendered)))
    }
    if (missing.isNotEmpty()) throw PromptRenderException(missing)
    return JsonArray(out)
}

/** The daemon's rule for a variable name: a letter or `_`, then letters, digits, `_` and `.`. */
private fun isVariableName(name: String): Boolean {
    if (name.isEmpty()) return false
    fun letter(c: Char) = c in 'a'..'z' || c in 'A'..'Z'
    val first = name[0]
    return (letter(first) || first == '_') &&
        name.all { letter(it) || it in '0'..'9' || it == '_' || it == '.' }
}

/**
 * Walk the placeholders of [template] exactly as the daemon's `variables_in`
 * does, so the variables a prompt reports and the ones rendering asks for are
 * the same list.
 */
private inline fun forEachPlaceholder(
    template: String,
    onText: (start: Int, end: Int) -> Unit,
    onPlaceholder: (start: Int, end: Int, name: String?) -> Unit,
) {
    var i = 0
    while (true) {
        val open = template.indexOf("{{", i)
        if (open < 0) break
        val close = template.indexOf("}}", open + 2)
        if (close < 0) break
        onText(i, open)
        val name = template.substring(open + 2, close).trim()
        onPlaceholder(open, close + 2, name.takeIf(::isVariableName))
        i = close + 2
    }
    onText(i, template.length)
}

private fun substitute(template: String, variables: Map<String, Any?>, missing: MutableList<String>): String {
    val out = StringBuilder(template.length)
    forEachPlaceholder(
        template,
        onText = { start, end -> out.append(template, start, end) },
        onPlaceholder = { start, end, name ->
            val value = name?.let { variables[it] }
            when {
                value != null -> out.append(value.toString())
                else -> {
                    if (name != null && name !in missing) missing.add(name)
                    out.append(template, start, end)
                }
            }
        },
    )
    return out.toString()
}

internal fun variablesIn(template: String, out: MutableList<String>) {
    forEachPlaceholder(template, onText = { _, _ -> }) { _, _, name ->
        if (name != null && name !in out) out.add(name)
    }
}

private fun variablesInValue(value: JsonElement, out: MutableList<String>) {
    when (value) {
        is JsonPrimitive -> if (value.isString) variablesIn(value.content, out)
        is JsonArray -> value.forEach { variablesInValue(it, out) }
        // The daemon's JSON objects keep their keys sorted, so it walks the
        // values in key order; doing the same keeps the two lists identical.
        is JsonObject -> value.keys.sorted().forEach { variablesInValue(value.getValue(it), out) }
    }
}

private fun PromptFallback.toPrompt(name: String, project: String): Prompt {
    val variables = ArrayList<String>()
    return when (this) {
        is PromptFallback.Template -> {
            variablesIn(template, variables)
            Prompt(project, name, 0, PromptKind.TEXT, variables, emptyList(), Clock.System.now(), template = template)
        }
        is PromptFallback.Messages -> {
            variablesInValue(messages, variables)
            Prompt(project, name, 0, PromptKind.CHAT, variables, emptyList(), Clock.System.now(), messages = messages)
        }
    }
}

/** A path segment, percent-encoded: a prompt name may contain `/`. */
internal fun segment(value: String): String =
    URLEncoder.encode(value, StandardCharsets.UTF_8).replace("+", "%20")

/**
 * The prompt surface behind [Mcpd.getPrompt], with its cache.
 *
 * The cache is per client and bounded, because a client that reads prompts by
 * computed names would otherwise grow without limit (B3).
 */
internal class PromptStore(private val client: Mcpd) {
    private class Entry(val prompt: Prompt, val fetchedAt: Long)

    private val cache = object : LinkedHashMap<String, Entry>() {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, Entry>?): Boolean =
            size > PROMPT_CACHE_SIZE
    }

    internal val cacheSize: Int get() = synchronized(cache) { cache.size }

    suspend fun get(
        name: String,
        label: String?,
        version: Int?,
        cacheTtl: Duration,
        fallback: PromptFallback?,
        project: String,
    ): Prompt {
        require(label == null || version == null) {
            "getPrompt takes a label or a version, not both"
        }
        val selector = when {
            label != null -> "label=$label"
            version != null -> "version=$version"
            else -> "default"
        }
        val key = "$project\u0000$name\u0000$selector"
        val cached = synchronized(cache) { cache[key] }
        if (cached != null && System.nanoTime() - cached.fetchedAt < cacheTtl.inWholeNanoseconds) {
            return cached.prompt
        }
        val fetched = try {
            fetch(name, label, version, project)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            if (cached != null) {
                client.diagnostics.warn("prompt-stale-$name") {
                    "could not refresh prompt $name, using the copy fetched earlier: ${e.message}"
                }
                return cached.prompt
            }
            if (fallback != null) return fallback.toPrompt(name, project)
            throw e
        }
        synchronized(cache) {
            // Removed first so a refreshed copy counts as the newest, not as
            // the oldest it was when first stored.
            cache.remove(key)
            cache[key] = Entry(fetched, System.nanoTime())
        }
        return fetched
    }

    private suspend fun fetch(name: String, label: String?, version: Int?, project: String): Prompt {
        if (label != null || version != null) return read(name, label, version, project)
        return try {
            read(name, DEFAULT_LABEL, null, project)
        } catch (e: McpdException) {
            // A prompt nobody has labelled yet still works: the latest version.
            if (e.status != 404) throw e
            read(name, null, null, project)
        }
    }

    private suspend fun read(name: String, label: String?, version: Int?, project: String): Prompt =
        client.transport.getJson(
            "/v1/api/prompts/${segment(name)}",
            client.query("project" to project, "label" to label, "version" to version),
            Prompt.serializer(),
        )

    suspend fun create(request: CreatePromptRequest, project: String): Prompt =
        client.transport.postJson(
            "/v1/api/prompts",
            client.query("project" to project),
            client.json.encodeToString(CreatePromptRequest.serializer(), request),
            Prompt.serializer(),
        )

    suspend fun setLabel(name: String, label: String, version: Int, project: String): Prompt =
        client.transport.putJson(
            "/v1/api/prompts/${segment(name)}/labels/${segment(label)}",
            client.query("project" to project),
            client.json.encodeToString(PromptLabelRequest.serializer(), PromptLabelRequest(version)),
            Prompt.serializer(),
        )
}
