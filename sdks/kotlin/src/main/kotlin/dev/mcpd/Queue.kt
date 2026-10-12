package dev.mcpd

import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock

/**
 * One thing waiting to be exported.
 *
 * The five signals share a queue so that a burst of logs cannot starve spans,
 * and so the bound in [SignalQueue] is a bound on total memory rather than on
 * five separate ones.
 */
internal sealed class Signal {
    /**
     * A span record.
     *
     * The record is mutable in place because a span is exported when it starts
     * and again when it ends. If the start record has not left the queue yet
     * there is no reason to send both: updating it costs one send instead of
     * two for the common case of a span shorter than a flush interval.
     */
    class SpanSlot(@Volatile var record: Span) : Signal() {
        /** Whether this slot is still in the queue. Written under the queue lock. */
        @Volatile
        var queued: Boolean = false
    }

    class LogSlot(val record: LogRecord) : Signal()

    class MetricSlot(val record: MetricPoint) : Signal()

    class ScoreSlot(val record: Score) : Signal()

    class FeedbackSlot(val record: Feedback) : Signal()
}

/**
 * A bounded FIFO of signals waiting for the exporter.
 *
 * The bound is the whole point. An unbounded buffer turns a network outage into
 * an out-of-memory kill of the process being observed, which is the worst
 * possible failure for an observability tool. When the queue is full the oldest
 * signals are dropped: the newest are the ones most likely to explain what is
 * happening right now.
 *
 * Safe to call from any thread. [offer] is taken by application threads on the
 * hot path, so it does no work beyond a lock, a list append and a counter.
 */
internal class SignalQueue(private val capacity: Int) {
    private val lock = ReentrantLock()
    private val items = ArrayDeque<Signal>()
    private var droppedTotal = 0L

    /** How many signals are waiting. */
    val size: Int get() = lock.withLock { items.size }

    /** How many signals have been dropped since this client was created. */
    val dropped: Long get() = lock.withLock { droppedTotal }

    /**
     * Append a signal, evicting the oldest when the queue is full.
     *
     * Returns how many signals were dropped to make room, so the caller can
     * report it once per interval rather than once per drop.
     */
    fun offer(signal: Signal): Int = lock.withLock {
        var evicted = 0
        while (items.size >= capacity) {
            val old = items.removeFirst()
            if (old is Signal.SpanSlot) old.queued = false
            droppedTotal++
            evicted++
        }
        if (signal is Signal.SpanSlot) signal.queued = true
        items.addLast(signal)
        evicted
    }

    /**
     * Replace the record in a slot that is still queued.
     *
     * Returns false when the slot has already been drained or dropped, in which
     * case the caller must enqueue the new record instead.
     */
    fun update(slot: Signal.SpanSlot, record: Span): Boolean = lock.withLock {
        if (!slot.queued) {
            false
        } else {
            slot.record = record
            true
        }
    }

    /** Remove and return up to [max] signals, oldest first. */
    fun drain(max: Int): List<Signal> = lock.withLock {
        if (items.isEmpty()) {
            emptyList()
        } else {
            val count = minOf(max, items.size)
            val out = ArrayList<Signal>(count)
            repeat(count) {
                val signal = items.removeFirst()
                if (signal is Signal.SpanSlot) signal.queued = false
                out.add(signal)
            }
            out
        }
    }
}

/**
 * Group drained signals into the batch shape the ingest endpoint accepts.
 *
 * `ingestKey` is generated once per batch and reused across retries, so a
 * request that timed out after the server had already stored it is recognised
 * as a replay rather than counted twice.
 */
internal fun ingestBatch(signals: List<Signal>, ingestKey: String): IngestBatch {
    val spans = ArrayList<Span>()
    val logs = ArrayList<LogRecord>()
    val metrics = ArrayList<MetricPoint>()
    val scores = ArrayList<Score>()
    val feedback = ArrayList<Feedback>()
    for (signal in signals) {
        when (signal) {
            is Signal.SpanSlot -> spans.add(signal.record)
            is Signal.LogSlot -> logs.add(signal.record)
            is Signal.MetricSlot -> metrics.add(signal.record)
            is Signal.ScoreSlot -> scores.add(signal.record)
            is Signal.FeedbackSlot -> feedback.add(signal.record)
        }
    }
    return IngestBatch(
        spans = spans.takeIf { it.isNotEmpty() },
        logs = logs.takeIf { it.isNotEmpty() },
        metrics = metrics.takeIf { it.isNotEmpty() },
        scores = scores.takeIf { it.isNotEmpty() },
        feedback = feedback.takeIf { it.isNotEmpty() },
        ingestKey = ingestKey,
    )
}
