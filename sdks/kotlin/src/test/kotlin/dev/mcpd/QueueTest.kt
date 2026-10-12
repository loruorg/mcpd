package dev.mcpd

import kotlin.time.Clock
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class QueueTest {

    private fun span(name: String): Span {
        val now = Clock.System.now()
        return Span(
            traceId = Ids.newTraceId(),
            spanId = Ids.newSpanId(),
            name = name,
            kind = SpanKind.GENERIC,
            status = SpanStatus.UNSET,
            startTime = now,
            isRoot = true,
            isEvent = false,
            correlation = Correlation(project = "test"),
            receivedAt = now,
        )
    }

    @Test
    fun `the queue is bounded and drops the oldest`() {
        val queue = SignalQueue(4)
        var evicted = 0
        repeat(10) { i -> evicted += queue.offer(Signal.SpanSlot(span("span-$i"))) }

        assertEquals(4, queue.size, "the bound must hold")
        assertEquals(6, evicted, "six offers had to make room")
        assertEquals(6, queue.dropped)

        val kept = queue.drain(10).map { (it as Signal.SpanSlot).record.name }
        assertEquals(listOf("span-6", "span-7", "span-8", "span-9"), kept, "the newest survive")
    }

    @Test
    fun `draining takes at most the batch size, oldest first`() {
        val queue = SignalQueue(100)
        repeat(5) { i -> queue.offer(Signal.SpanSlot(span("span-$i"))) }

        val first = queue.drain(2)
        assertEquals(listOf("span-0", "span-1"), first.map { (it as Signal.SpanSlot).record.name })
        assertEquals(3, queue.size)
        assertEquals(3, queue.drain(50).size)
        assertTrue(queue.drain(50).isEmpty())
    }

    @Test
    fun `a queued span record can be updated in place`() {
        val queue = SignalQueue(10)
        val slot = Signal.SpanSlot(span("still-running"))
        queue.offer(slot)

        val finished = slot.record.copy(status = SpanStatus.OK, durationNs = 1_000L)
        assertTrue(queue.update(slot, finished), "a queued slot updates")
        assertEquals(1, queue.size, "updating must not enqueue a second record")

        val drained = queue.drain(10).single() as Signal.SpanSlot
        assertEquals(SpanStatus.OK, drained.record.status)
        assertFalse(queue.update(slot, finished), "a drained slot cannot be updated")
    }

    @Test
    fun `an evicted slot cannot be updated`() {
        val queue = SignalQueue(1)
        val first = Signal.SpanSlot(span("first"))
        queue.offer(first)
        queue.offer(Signal.SpanSlot(span("second")))

        assertFalse(queue.update(first, first.record), "the evicted slot is gone")
    }

    @Test
    fun `a batch carries every signal kind and its idempotency key`() {
        val now = Clock.System.now()
        val signals = listOf(
            Signal.SpanSlot(span("a")),
            Signal.LogSlot(
                LogRecord(
                    logId = "l", timestamp = now, level = LogLevel.INFO, message = "hello",
                    correlation = Correlation(project = "test"), receivedAt = now,
                )
            ),
            Signal.MetricSlot(
                MetricPoint(
                    metricId = "m", name = "x", kind = MetricKind.COUNTER, value = 1.0,
                    timestamp = now, correlation = Correlation(project = "test"), receivedAt = now,
                )
            ),
            Signal.ScoreSlot(
                Score(
                    scoreId = "s", scorerId = "helpfulness", source = ScoreSource.LIVE, value = 0.5,
                    timestamp = now, correlation = Correlation(project = "test"),
                )
            ),
            Signal.FeedbackSlot(
                Feedback(
                    feedbackId = "f", kind = FeedbackKind.THUMBS,
                    value = toJsonElement("up"), timestamp = now,
                    correlation = Correlation(project = "test"),
                )
            ),
        )

        val batch = ingestBatch(signals, "key-1")
        assertEquals(1, batch.spans?.size)
        assertEquals(1, batch.logs?.size)
        assertEquals(1, batch.metrics?.size)
        assertEquals(1, batch.scores?.size)
        assertEquals(1, batch.feedback?.size)
        assertEquals("key-1", batch.ingestKey)
    }

    @Test
    fun `an empty signal kind is left out of the batch`() {
        val batch = ingestBatch(listOf(Signal.SpanSlot(span("only-a-span"))), "key-2")
        assertEquals(1, batch.spans?.size)
        assertEquals(null, batch.logs, "an empty array would claim logs were sent")
    }
}
