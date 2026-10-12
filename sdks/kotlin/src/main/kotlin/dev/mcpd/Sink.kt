package dev.mcpd

import kotlinx.serialization.SerializationStrategy
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject

/**
 * Where a drained batch goes.
 *
 * The exporter turns the queue into an [IngestBatch] and hands it here with
 * the batch's idempotency key. The HTTP transport is the default sink and the
 * only one that talks to a daemon; [RecordingSink] keeps batches in memory so
 * a test can assert what was recorded, and [ConsoleSink] prints them.
 *
 * A sink runs on the exporter's worker inside the same guard as the transport.
 * One that throws is counted as a failed export and reported once per
 * interval; the caller recording the signal never hears about it.
 */
fun interface Sink {
    /** Take one batch. Throw to refuse it. */
    suspend fun export(batch: IngestBatch, ingestKey: String)
}

/**
 * A sink that keeps every batch in memory.
 *
 * The five lists hold what arrived, in arrival order, and are read as copies
 * so a test can assert while the exporter is still running. This is how a
 * test proves its instrumentation recorded the span, the score and the usage
 * it meant to, without standing up a server.
 */
class RecordingSink : Sink {
    private val lock = Any()
    private val spanList = ArrayList<Span>()
    private val logList = ArrayList<LogRecord>()
    private val metricList = ArrayList<MetricPoint>()
    private val scoreList = ArrayList<Score>()
    private val feedbackList = ArrayList<Feedback>()
    private var batchCount = 0

    /** Every span seen, in arrival order. */
    val spans: List<Span> get() = synchronized(lock) { spanList.toList() }

    /** Every log line seen. */
    val logs: List<LogRecord> get() = synchronized(lock) { logList.toList() }

    /** Every metric point seen. */
    val metrics: List<MetricPoint> get() = synchronized(lock) { metricList.toList() }

    /** Every score seen. */
    val scores: List<Score> get() = synchronized(lock) { scoreList.toList() }

    /** Every piece of feedback seen. */
    val feedback: List<Feedback> get() = synchronized(lock) { feedbackList.toList() }

    /** How many batches have arrived. */
    val batches: Int get() = synchronized(lock) { batchCount }

    override suspend fun export(batch: IngestBatch, ingestKey: String) {
        synchronized(lock) {
            batchCount++
            batch.spans?.let { spanList.addAll(it) }
            batch.logs?.let { logList.addAll(it) }
            batch.metrics?.let { metricList.addAll(it) }
            batch.scores?.let { scoreList.addAll(it) }
            batch.feedback?.let { feedbackList.addAll(it) }
        }
    }

    /** Forget everything seen so far. */
    fun clear() {
        synchronized(lock) {
            spanList.clear()
            logList.clear()
            metricList.clear()
            scoreList.clear()
            feedbackList.clear()
            batchCount = 0
        }
    }
}

/**
 * A sink that writes one JSON line per signal.
 *
 * Each line is the wire record with a `signal` field in front naming its
 * kind: `{"signal":"span",...}`. Standard error by default, so a local dump
 * while debugging does not mix with the process's own output; pass [write]
 * to send the lines somewhere else.
 */
class ConsoleSink(
    private val write: (String) -> Unit = { System.err.println(it) },
) : Sink {
    @OptIn(kotlinx.serialization.ExperimentalSerializationApi::class)
    private val json = Json {
        explicitNulls = false
        encodeDefaults = true
    }

    override suspend fun export(batch: IngestBatch, ingestKey: String) {
        batch.spans?.forEach { line("span", Span.serializer(), it) }
        batch.logs?.forEach { line("log", LogRecord.serializer(), it) }
        batch.metrics?.forEach { line("metric", MetricPoint.serializer(), it) }
        batch.scores?.forEach { line("score", Score.serializer(), it) }
        batch.feedback?.forEach { line("feedback", Feedback.serializer(), it) }
    }

    private fun <T> line(signal: String, serializer: SerializationStrategy<T>, record: T) {
        val fields = LinkedHashMap<String, kotlinx.serialization.json.JsonElement>()
        fields["signal"] = JsonPrimitive(signal)
        fields.putAll(json.encodeToJsonElement(serializer, record).jsonObject)
        write(JsonObject(fields).toString())
    }
}
