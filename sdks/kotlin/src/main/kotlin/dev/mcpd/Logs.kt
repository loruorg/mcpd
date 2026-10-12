package dev.mcpd

import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * The logging signal: what a field map may hold, and how a level relates to an
 * OpenTelemetry severity number.
 *
 * Nothing here throws. A field the caller handed us is data, and data that
 * cannot be encoded is worth less than the batch it travels in.
 */

/**
 * A throwable as `type`, `message` and `stack`, the shape every mcpd SDK sends.
 *
 * Serialising it as its `toString` keeps the type and the message and loses the
 * stack, which is the part a reader of the log line actually needs.
 */
fun describeError(error: Throwable): JsonObject = JsonObject(
    buildMap {
        put("type", JsonPrimitive(error::class.qualifiedName?.substringAfterLast('.')
            ?: error::class.simpleName ?: "Throwable"))
        put("message", JsonPrimitive(error.message ?: error::class.simpleName ?: "Throwable"))
        val stack = runCatching { error.stackTraceToString() }.getOrNull()
        if (!stack.isNullOrBlank()) put("stack", JsonPrimitive(stack.trim()))
    },
)

/**
 * The level an OpenTelemetry severity number lands on.
 *
 * By range, the way the daemon reads an inbound OTLP `LogRecord`: 1 to 4 is
 * trace, 5 to 8 debug, 9 to 12 info, 13 to 16 warn, 17 to 20 error, and 21 and
 * above fatal. A number outside the range is clamped rather than refused,
 * because a sender that invents a 40 still meant something serious.
 */
fun logLevelFromSeverity(severity: Int): LogLevel {
    var found = LogLevel.entries.first()
    for (candidate in LogLevel.entries) {
        if (severity >= candidate.severity) found = candidate
    }
    return found
}

/** The OpenTelemetry severity number the spec gives a level. */
fun severityNumber(level: LogLevel): Int = level.severity

/** Read a level by name, case-insensitively. Anything else is null. */
fun parseLogLevel(name: String?): LogLevel? {
    val wanted = name?.trim()?.lowercase() ?: return null
    return LogLevel.fromWire(wanted)
}

/** Whether [level] is at or above [floor]. */
fun meetsLevel(level: LogLevel, floor: LogLevel): Boolean = level.severity >= floor.severity

/**
 * A caller's field map, in a shape the JSON encoder is certain to accept.
 *
 * One value nothing can encode costs its own key and nothing else, and a
 * throwable under a key keeps its stack instead of flattening to one line.
 */
internal fun encodeFields(values: Map<String, Any?>?): JsonObject? =
    values?.takeIf { it.isNotEmpty() }?.let { map ->
        JsonObject(map.entries.associate { (key, value) -> key to encodeFieldValue(value, 0) })
    }

private const val MAX_FIELD_DEPTH = 12

private fun encodeFieldValue(value: Any?, depth: Int): JsonElement = when {
    value is Throwable -> describeError(value)
    depth >= MAX_FIELD_DEPTH -> JsonPrimitive("[too deep]")
    value is Map<*, *> -> JsonObject(
        value.entries.associate { (key, item) ->
            key.toString() to encodeFieldValue(item, depth + 1)
        },
    )
    value is Iterable<*> -> kotlinx.serialization.json.JsonArray(
        value.map { encodeFieldValue(it, depth + 1) },
    )
    value is Array<*> -> kotlinx.serialization.json.JsonArray(
        value.map { encodeFieldValue(it, depth + 1) },
    )
    else -> toJsonElement(value)
}
