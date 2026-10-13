package dev.mcpd.testing

import dev.mcpd.Feedback
import dev.mcpd.LogRecord
import dev.mcpd.Mcpd
import dev.mcpd.McpdConfig
import dev.mcpd.MetricPoint
import dev.mcpd.RecordingSink
import dev.mcpd.Score
import dev.mcpd.Span
import dev.mcpd.SpanKind
import kotlin.time.Duration

/** One finished span and the finished spans under it, ordered by start time. */
data class SpanTree(
    val span: Span,
    val children: List<SpanTree>,
)

/**
 * A client for your own unit tests (SDK contract 2.5).
 *
 * ```kotlin
 * val test = TestClient()
 * answerQuestion(test.client)
 * val call = test.span("model_call")
 * assertEquals(37, call.usage?.outputTokens)
 * ```
 *
 * [client] is a real [Mcpd] whose sink keeps every batch in memory, so what
 * you read back went through sampling, payload capture, redaction and the
 * queue as it would on the way to a daemon. Nothing leaves the process. Every
 * accessor flushes first, so you never call `flush()` yourself.
 *
 * [config] is any client configuration; its `sink` is replaced. [enabled] and
 * [sample] are separate so they hold whatever the environment says: a CI job
 * that sets `MCPD_ENABLED=false` does not turn the test client off.
 */
class TestClient(
    config: McpdConfig = McpdConfig(),
    enabled: Boolean = true,
    sample: Double = 1.0,
) : AutoCloseable {
    /** The sink underneath, for a test that wants the raw batches. */
    val recorder: RecordingSink = RecordingSink()

    /** The client to pass wherever your code takes an [Mcpd]. */
    val client: Mcpd = Mcpd(config.copy(sink = recorder, enabled = enabled, sample = sample))

    /** The finished spans, one per span id, in the order each id first arrived. */
    suspend fun spans(name: String? = null, kind: SpanKind? = null): List<Span> {
        client.flush()
        return finished(recorder.spans).filter { (name == null || it.name == name) && (kind == null || it.kind == kind) }
    }

    /**
     * The one finished span called [name].
     *
     * Throws [AssertionError] when there is none or more than one, with the
     * count and the names that were recorded, because a failed assertion is
     * what a test wants from it.
     */
    suspend fun span(name: String): Span {
        val all = spans()
        val found = all.filter { it.name == name }
        if (found.size != 1) {
            val seen = all.map { it.name }.distinct().joinToString(", ").ifEmpty { "none" }
            throw AssertionError(
                "expected one finished span named \"$name\", found ${found.size}; spans recorded: $seen"
            )
        }
        return found.single()
    }

    /** Every log line, in arrival order. */
    suspend fun logs(): List<LogRecord> {
        client.flush()
        return recorder.logs
    }

    /** Every metric point, in arrival order. */
    suspend fun metrics(): List<MetricPoint> {
        client.flush()
        return recorder.metrics
    }

    /** Every score, in arrival order. */
    suspend fun scores(): List<Score> {
        client.flush()
        return recorder.scores
    }

    /** Every piece of feedback, in arrival order. Not `feedback()`, which the client uses to record one. */
    suspend fun recordedFeedback(): List<Feedback> {
        client.flush()
        return recorder.feedback
    }

    /** Every trace as a tree of finished spans. A span whose parent was not recorded is a root. */
    suspend fun trees(): List<SpanTree> {
        val all = spans()
        val byId = all.associateBy { it.spanId }
        val children = HashMap<String, MutableList<Span>>()
        val roots = ArrayList<Span>()
        for (span in all) {
            val parent = span.parentSpanId?.takeIf { it != span.spanId && it in byId }
            if (parent != null) children.getOrPut(parent) { ArrayList() }.add(span) else roots.add(span)
        }
        // Each span has one parent, so a walk down from the roots reaches each
        // span once and cannot loop.
        fun build(span: Span): SpanTree =
            SpanTree(span, children[span.spanId].orEmpty().sortedBy { it.startTime }.map(::build))
        return roots.sortedBy { it.startTime }.map(::build)
    }

    /** Flush, then forget everything recorded so far. */
    suspend fun reset() {
        client.flush()
        recorder.clear()
    }

    /** Stop the client's worker. */
    suspend fun shutdown(timeout: Duration = Mcpd.DEFAULT_DEADLINE) {
        client.shutdown(timeout)
    }

    /** Blocking [shutdown], for `use { }`. */
    override fun close() {
        client.close()
    }
}

/** The last record per span id, in first-arrival order, ended spans only. */
private fun finished(records: List<Span>): List<Span> {
    // Putting a key a LinkedHashMap already holds keeps its position, which is
    // the order the contract asks for.
    val last = LinkedHashMap<String, Span>()
    for (span in records) last[span.spanId] = span
    return last.values.filter { it.endTime != null }
}
