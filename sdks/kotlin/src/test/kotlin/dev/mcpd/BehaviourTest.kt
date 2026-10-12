package dev.mcpd

import com.sun.net.httpserver.HttpServer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.joinAll
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.yield
import kotlin.time.Instant
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.atomic.AtomicInteger
import java.util.logging.Handler
import java.util.logging.LogRecord
import java.util.logging.Logger
import kotlin.test.Test
import kotlin.test.assertContains
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds

/**
 * The promises that hold whatever the server, the clock or the caller does.
 *
 * `ExportTest` proves the happy path reaches the wire intact. This file is the
 * other half: what happens when mcpd answers 500, answers nothing at all,
 * answers with HTML, when eight threads record at once, and when a caller ends
 * a span twice or ends one after the client has gone away. Contract behaviours
 * B1, B2, B3, B5, B7, B9 and B10 all have an edge here that the existing suite
 * does not reach.
 */
class BehaviourTest {

    // ------------------------------------------------------- B3, the bound ---

    @Test
    fun `the queue bound counts every kind of signal, not only spans`() = runBlocking {
        FakeMcpd().use { server ->
            // One shared bound over all five signals is the point of the shared
            // queue: five separate bounds would be five ways to run out of
            // memory. Nothing in the suite so far puts anything but spans in it.
            val mcpd = client(server) { it.copy(maxQueue = 5, batchSize = 10_000) }
            val root = mcpd.trace("run")
            root.log(LogLevel.INFO, "first")
            root.metric("tokens", 12.0, MetricKind.COUNTER)
            root.score("helpfulness", 0.5, 0.0, 1.0)
            root.feedback(FeedbackKind.THUMBS, "up")
            assertEquals(5, mcpd.queueDepth, "a span, a log, a metric, a score and a feedback")
            assertEquals(0L, mcpd.droppedSignals, "nothing has overflowed yet")

            root.log(LogLevel.INFO, "sixth")

            assertEquals(5, mcpd.queueDepth, "the bound holds across kinds")
            assertEquals(1L, mcpd.droppedSignals)

            mcpd.flush()
            val batch = server.requests.single().batch()
            assertNull(batch.spans, "the span was the oldest, so the span is what went")
            assertEquals(2, assertNotNull(batch.logs).size)
            assertEquals(1, assertNotNull(batch.metrics).size)
            assertEquals(1, assertNotNull(batch.scores).size)
            assertEquals(1, assertNotNull(batch.feedback).size)
            mcpd.shutdown()
        }
    }

    // ---------------------------------------------- B1, the server misbehaves ---

    @Test
    fun `a 500 is not retried and the caller never hears about it`() = runBlocking {
        FakeMcpd { _, _ -> 500 to """{"error":"internal"}""" }.use { server ->
            val mcpd = client(server)
            mcpd.trace("run").end()
            mcpd.flush()

            // 500 is not in the retryable set: it is not a load-shedding
            // status, so a second identical batch is unlikely to fare better.
            assertEquals(1, server.requests.size)
            assertEquals(0, mcpd.queueDepth, "the batch is dropped, not held forever")
            mcpd.shutdown()
        }
    }

    @Test
    fun `a server that stays busy is given three attempts and then let go`() = runBlocking {
        FakeMcpd { _, _ -> 503 to "busy" }.use { server ->
            val mcpd = client(server)
            mcpd.trace("run").end()
            mcpd.flush()

            assertEquals(3, server.requests.size, "the first attempt and two retries")
            assertEquals(0, mcpd.queueDepth, "a batch that cannot land is dropped")
            mcpd.shutdown()
        }
    }

    @Test
    fun `each status the contract names is retried, and the retry carries the same key`() = runBlocking {
        // 503 has its own test above. These are the other three, and a run
        // that proves one of the four proves nothing about the rest.
        for (status in listOf(429, 502, 504)) {
            FakeMcpd { _, attempt -> if (attempt <= 2) status to "busy" else 200 to FakeMcpd.ACCEPTED }
                .use { server ->
                    val mcpd = client(server)
                    mcpd.trace("retried after $status").end()
                    mcpd.flush()

                    assertEquals(3, server.requests.size, "$status: two failures, then the attempt that landed")
                    assertEquals(1, server.requests.map { it.batch().ingestKey }.toSet().size, "$status: the key changed between attempts")
                    assertEquals(0, mcpd.queueDepth)
                    mcpd.shutdown()
                }
        }
    }

    @Test
    fun `a body that is not JSON is ignored on the ingest path`() = runBlocking {
        FakeMcpd { _, attempt ->
            if (attempt == 1) 200 to "<html>fronted by a proxy</html>" else 500 to "<html>gone</html>"
        }.use { server ->
            val mcpd = client(server)
            mcpd.trace("accepted").end()
            mcpd.flush()
            mcpd.trace("refused").end()
            mcpd.flush()

            // The exporter reads the status and nothing else. A proxy that
            // answers HTML must not be able to raise inside an agent.
            assertEquals(2, server.requests.size)
            assertEquals(0, mcpd.queueDepth)
            mcpd.shutdown()
        }
    }

    @Test
    fun `a body that is not JSON fails a read, because a read is being waited on`() = runBlocking {
        FakeMcpd { _, _ -> 200 to "<html>fronted by a proxy</html>" }.use { server ->
            val mcpd = client(server)
            val failure = assertFailsWith<McpdException> { mcpd.health() }
            assertContains(assertNotNull(failure.message), "could not read the response")
            mcpd.shutdown()
        }
    }

    @Test
    fun `a server that accepts the connection and never answers only costs a timeout`() = runBlocking {
        SilentServer().use { silent ->
            val mcpd = client(endpoint = silent.endpoint) { it.copy(timeout = 200.milliseconds) }
            mcpd.trace("run").end()

            val started = System.nanoTime()
            mcpd.flush()
            val elapsed = (System.nanoTime() - started) / 1_000_000

            assertEquals(0, mcpd.queueDepth, "the batch was given up on")
            assertTrue(elapsed < 10_000, "three 200ms attempts and their backoff, not a hang: ${elapsed}ms")
            mcpd.shutdown()
        }
    }

    // ------------------------------------------------- B2, nothing blocks ---

    @Test
    fun `recording does not wait for a server that is not answering`() = runBlocking {
        SilentServer().use { silent ->
            // The worker is stuck in a request that can only end in a timeout.
            // Recording happens on this thread and must not notice.
            val mcpd = client(endpoint = silent.endpoint) {
                it.copy(timeout = 400.milliseconds, flushInterval = 20.milliseconds, batchSize = 1, maxQueue = 64)
            }
            mcpd.trace("wake the worker").end()
            delay(60)

            val started = System.nanoTime()
            repeat(200) { i -> mcpd.trace("run-$i").end() }
            val elapsed = (System.nanoTime() - started) / 1_000_000

            // One blocking send would already cost more than this, and there
            // would have been many.
            assertTrue(elapsed < 400, "200 spans took ${elapsed}ms; recording is waiting on the network")
        }
    }

    // ---------------------------------------------------- B4, concurrency ---

    @Test
    fun `eight threads recording at once produce eight correct trees`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server) { it.copy(batchSize = 10_000) }
            val gate = CountDownLatch(1)
            val threads = (0 until 8).map { i ->
                Thread {
                    gate.await()
                    val root = mcpd.trace("run-$i")
                    repeat(5) { j -> root.child("step-$i-$j").end() }
                    root.end()
                }
            }
            threads.forEach { it.start() }
            // Released together, so the threads genuinely overlap rather than
            // running one after another and proving nothing.
            gate.countDown()
            threads.forEach { it.join() }
            mcpd.flush()

            val spans = server.requests.flatMap { it.batch().spans ?: emptyList() }
            assertEquals(48, spans.size, "eight roots and forty children, each recorded once")
            assertEquals(48, spans.map { it.spanId }.toSet().size, "a shared id generator handed out a duplicate")

            val roots = spans.filter { it.isRoot }
            assertEquals(8, roots.size)
            assertEquals(8, roots.map { it.traceId }.toSet().size, "each thread opened its own trace")
            for (root in roots) {
                val children = spans.filter { it.parentSpanId == root.spanId }
                assertEquals(5, children.size, "${root.name} lost a child")
                assertTrue(children.all { it.traceId == root.traceId }, "a child landed in another trace")
            }
            mcpd.shutdown()
        }
    }

    @Test
    fun `sibling coroutines cannot see each other's active span`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server) { it.copy(batchSize = 10_000) }
            coroutineScope {
                (0 until 8).map { i ->
                    launch(Dispatchers.Default) {
                        mcpd.span("root-$i") { root ->
                            // A suspension point in the middle: the coroutine
                            // can resume on a different thread, and the span
                            // has to follow it there.
                            yield()
                            assertEquals(root.spanId, mcpd.currentSpan()?.spanId, "root-$i lost its own span")
                            mcpd.span("child-$i") { child ->
                                assertEquals(root.spanId, child.parentSpanId, "child-$i attached to a sibling")
                            }
                        }
                    }
                }.joinAll()
            }
            mcpd.flush()

            val spans = server.requests.flatMap { it.batch().spans ?: emptyList() }
            assertEquals(16, spans.size)
            for (i in 0 until 8) {
                val root = spans.single { it.name == "root-$i" }
                val child = spans.single { it.name == "child-$i" }
                assertEquals(root.spanId, child.parentSpanId)
                assertEquals(root.traceId, child.traceId)
            }
            mcpd.shutdown()
        }
    }

    // ------------------------------------------------- the span's lifetime ---

    @Test
    fun `ending a span twice records it once and keeps the first outcome`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            val span = mcpd.trace("once")
            span.end()
            assertTrue(span.isEnded)
            // A caller with a `finally` and an explicit end has ended it twice.
            // The second one must not reopen it, restate it or duplicate it.
            span.end(SpanStatus.ERROR)
            mcpd.flush()

            val sent = assertNotNull(server.requests.single().batch().spans).single()
            assertEquals(SpanStatus.OK, sent.status, "the second end overwrote the first")
            mcpd.shutdown()
        }
    }

    @Test
    fun `a span that ends after shutdown is dropped rather than queued forever`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            val span = mcpd.trace("straggler")
            mcpd.shutdown()
            assertEquals(1, server.requests.size, "shutdown sent the start record")

            span.end()
            mcpd.flush()

            assertEquals(1, server.requests.size, "there is no worker left to send an end record")
            assertEquals(0, mcpd.queueDepth, "and nothing is accumulating in memory")
            val sent = assertNotNull(server.requests.single().batch().spans).single()
            assertNull(sent.endTime, "what landed is the unfinished span, which is the truth")
        }
    }

    // ------------------------------------------------ B7, a broken header ---

    @Test
    fun `a malformed, empty or all-zero traceparent starts a fresh trace`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server) { it.copy(batchSize = 10_000) }
            val headers = listOf(
                "",
                "   ",
                "garbage",
                "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
                "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
                "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7",
                "ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            )
            for (header in headers) {
                val span = mcpd.trace("inbound", traceparent = header)
                assertTrue(span.isRoot, "a broken header must not leave a parent behind: $header")
                assertNull(span.parentSpanId)
                assertTrue(Ids.isValidTraceId(span.traceId), "$header produced ${span.traceId}")
                assertTrue(span.recorded, "a broken header is not a reason to stop recording")
                span.end()
            }
            mcpd.flush()

            val spans = server.requests.flatMap { it.batch().spans ?: emptyList() }
            assertEquals(headers.size, spans.size)
            assertEquals(headers.size, spans.map { it.traceId }.toSet().size, "each got its own trace")
            assertTrue(spans.none { it.traceId == "0".repeat(32) }, "the reserved id reached the wire")
            mcpd.shutdown()
        }
    }

    // ------------------------------------------------------- B10, errors ---

    @Test
    fun `an error with a blank message still says something a reader can act on`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server) { it.copy(batchSize = 10_000) }
            mcpd.trace("blank").setError("   ").end()
            mcpd.trace("nameless").setError(RuntimeException()).end()
            mcpd.flush()

            val spans = server.requests.flatMap { it.batch().spans ?: emptyList() }
            val blank = spans.single { it.name == "blank" }
            assertEquals(SpanStatus.ERROR, blank.status)
            assertEquals("no message given", blank.error?.message, "an empty string is not an answer")

            val nameless = spans.single { it.name == "nameless" }
            assertEquals(SpanStatus.ERROR, nameless.status)
            assertEquals("java.lang.RuntimeException", nameless.error?.message, "the class is all there was")
            mcpd.shutdown()
        }
    }

    // ---------------------------------------------------- B9, timestamps ---

    @Test
    fun `timestamps go on the wire as UTC and the duration is in nanoseconds`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            val span = mcpd.trace("timed")
            delay(20)
            span.end()
            mcpd.flush()

            val body = server.requests.single().body
            val start = assertNotNull(
                Regex(""""start_time":"([^"]+)"""").find(body)?.groupValues?.get(1),
                "no start_time on the wire",
            )
            assertTrue(
                Regex("""^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z$""").matches(start),
                "a local timestamp makes a distributed trace unreadable: $start",
            )

            val sent = assertNotNull(server.requests.single().batch().spans).single()
            assertEquals(Instant.parse(start), sent.startTime)
            assertTrue(assertNotNull(sent.endTime) >= sent.startTime, "a span cannot end before it starts")
            val duration = assertNotNull(sent.durationNs)
            // Twenty milliseconds of sleep is 20,000,000ns. Milliseconds or
            // microseconds in this field would be three or six orders out.
            assertTrue(duration in 15_000_000..5_000_000_000, "duration_ns is not nanoseconds: $duration")
            mcpd.shutdown()
        }
    }

    @Test
    fun `timestamps carry nanosecond digits on the wire`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            repeat(5) { i ->
                val span = mcpd.trace("timed-$i")
                delay(1)
                span.end()
            }
            mcpd.flush()

            val body = server.requests.single().body
            val ends = Regex(""""end_time":"([^"]+)"""").findAll(body).map { it.groupValues[1] }.toList()
            assertEquals(5, ends.size, "five ended spans, five end times")
            // ISO 8601 prints the fraction in groups of three, so a value with
            // sub-microsecond digits shows nine. One in a thousand end times
            // lands on a whole microsecond by chance; five in a row do not.
            val fractions = ends.map { assertNotNull(Regex("""\.(\d+)Z$""").find(it)?.groupValues?.get(1), "no fraction in $it").length }
            assertTrue(fractions.any { it == 9 }, "no end time carried nanoseconds: $ends")
            assertTrue(fractions.all { it == 3 || it == 6 || it == 9 }, "an end time with a fraction of another width: $ends")

            // What went out is what the receiver reads back, to the nanosecond.
            val sent = assertNotNull(server.requests.single().batch().spans)
            sent.zip(ends).forEach { (span, wire) ->
                assertEquals(Instant.parse(wire), span.endTime, "the wire and the record disagree")
            }
            mcpd.shutdown()
        }
    }

    // ------------------------------------------------- correlation ---

    @Test
    fun `a log inside a run carries the run's correlation, and so does a grandchild`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server) { it.copy(batchSize = 10_000) }
            mcpd.span("run", sessionId = "sess-1", userId = "user-41", threadId = "thread-7") { run ->
                mcpd.log(LogLevel.WARN, "inside the run")
                mcpd.metric("steps", 1.0, MetricKind.COUNTER)
                mcpd.span("child") { child ->
                    mcpd.span("grandchild") { grandchild ->
                        assertEquals("sess-1", grandchild.correlation.sessionId)
                        assertEquals(run.traceId, grandchild.traceId)
                        assertEquals(child.spanId, grandchild.parentSpanId)
                    }
                }
            }
            mcpd.flush()

            val batch = server.requests.single().batch()
            val log = assertNotNull(batch.logs).single()
            assertEquals("sess-1", log.correlation.sessionId, "a log inside a run took the client's correlation, not the run's")
            assertEquals("user-41", log.correlation.userId)
            assertEquals("thread-7", log.correlation.threadId)

            val metric = assertNotNull(batch.metrics).single()
            assertEquals("sess-1", metric.correlation.sessionId)

            val spans = assertNotNull(batch.spans).associateBy { it.name }
            // The grandchild as well as the child. Inheritance that stops one
            // level down is the same defect wearing a different depth.
            for (name in listOf("run", "child", "grandchild")) {
                val span = assertNotNull(spans[name])
                assertEquals("sess-1", span.correlation.sessionId, "$name lost the session")
                assertEquals("user-41", span.correlation.userId, "$name lost the user")
                assertEquals("test", span.correlation.project, "$name lost the deployment")
            }
            mcpd.shutdown()
        }
    }

    @Test
    fun `a field set on the run after it started reaches a child opened later`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server) { it.copy(batchSize = 10_000) }
            val run = mcpd.trace("run", sessionId = "sess-1")
            val early = run.child("early")
            early.end()

            // Set after the run started, and after one child was already
            // opened. A child that copied the run's options at start would
            // miss it; one that reads the run's current state sees it.
            run.setCorrelation(userId = "user-41", release = "2026.08.1")
            val late = run.child("late")
            late.log(LogLevel.INFO, "late log")
            late.end()
            run.end()
            mcpd.flush()

            val batch = server.requests.single().batch()
            val spans = assertNotNull(batch.spans).associateBy { it.name }
            assertEquals("user-41", assertNotNull(spans["late"]).correlation.userId, "the late child did not see the field set on the run")
            assertEquals("2026.08.1", assertNotNull(spans["late"]).correlation.release)
            assertEquals("sess-1", assertNotNull(spans["late"]).correlation.sessionId, "inheriting the new field lost the old one")
            assertNull(assertNotNull(spans["early"]).correlation.userId, "a child that ended before the field was set is not rewritten")
            assertEquals("user-41", assertNotNull(batch.logs).single().correlation.userId, "a log under the late child did not carry the field")
            mcpd.shutdown()
        }
    }

    // ------------------------------------------------- B3, the warning ---

    @Test
    fun `the overflow warning is logged once per interval, not once per drop`() = runBlocking {
        val previous = diagnosticInterval
        diagnosticInterval = 300.milliseconds
        val captured = CopyOnWriteArrayList<String>()
        val handler = object : Handler() {
            override fun publish(record: LogRecord) {
                captured.add(record.message)
            }

            override fun flush() {}
            override fun close() {}
        }
        val logger = Logger.getLogger("dev.mcpd")
        logger.addHandler(handler)
        try {
            FakeMcpd().use { server ->
                val mcpd = client(server) { it.copy(maxQueue = 2, batchSize = 10_000) }
                repeat(50) { i -> mcpd.trace("run-$i") }
                assertEquals(48L, mcpd.droppedSignals, "the drops happened")
                assertEquals(1, captured.count { "queue is full" in it }, "48 drops in one interval produced more than one warning: $captured")

                delay(400)
                repeat(10) { i -> mcpd.trace("again-$i") }
                assertEquals(2, captured.count { "queue is full" in it }, "a new interval is allowed one more warning")
                mcpd.shutdown()
            }
        } finally {
            logger.removeHandler(handler)
            diagnosticInterval = previous
        }
    }

    // ------------------------------------------------- B5, flush and stop ---

    @Test
    fun `a flush with nothing to send makes no request`() = runBlocking {
        FakeMcpd().use { server ->
            val mcpd = client(server)
            mcpd.flush()
            assertEquals(0, server.requests.size, "an empty batch costs the server a request for nothing")

            mcpd.trace("something").end()
            mcpd.flush()
            mcpd.flush()
            assertEquals(1, server.requests.size, "the second flush had nothing left")
            mcpd.shutdown()
            assertEquals(1, server.requests.size, "and neither did shutdown")
        }
    }

    @Test
    fun `Retry-After decides when the retry happens`() = runBlocking {
        RetryAfterServer(seconds = 1).use { server ->
            val mcpd = client(endpoint = server.endpoint)
            mcpd.trace("held off").end()

            val started = System.nanoTime()
            mcpd.flush()
            val elapsed = (System.nanoTime() - started) / 1_000_000

            assertEquals(2, server.received.get(), "the retry landed")
            // Backoff on its own would be 250ms with jitter, so anything past
            // 850ms can only have come from reading the header.
            assertTrue(elapsed >= 850, "the server said one second and waited ${elapsed}ms")
            mcpd.shutdown()
        }
    }
}

/**
 * A socket that accepts a connection and then says nothing at all.
 *
 * Worse than a refused connection, and more common: a load balancer that has
 * lost its backend accepts the TCP connection and holds it. The only way out is
 * the client's own timeout.
 */
private class SilentServer : AutoCloseable {
    private val socket = ServerSocket(0, 16, InetAddress.getByName("127.0.0.1"))
    private val held = CopyOnWriteArrayList<Socket>()

    private val accepter = Thread {
        try {
            while (true) held.add(socket.accept())
        } catch (e: Exception) {
            // The socket was closed by close(). Nothing else to do.
        }
    }

    init {
        accepter.isDaemon = true
        accepter.start()
    }

    val endpoint: String get() = "http://127.0.0.1:" + socket.localPort

    override fun close() {
        // Closing the held connections first makes any in-flight request fail
        // immediately, so a test does not pay the timeout twice.
        held.forEach { runCatching { it.close() } }
        socket.close()
    }
}

/**
 * Answers 503 with a `Retry-After` once, then accepts.
 *
 * [FakeMcpd] can choose a status and a body but not a header, and the header is
 * the whole subject here.
 */
private class RetryAfterServer(private val seconds: Int) : AutoCloseable {
    private val server: HttpServer = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)

    /** How many requests have arrived. */
    val received = AtomicInteger(0)

    val endpoint: String get() = "http://127.0.0.1:" + server.address.port

    init {
        server.createContext("/") { exchange ->
            exchange.requestBody.use { it.readBytes() }
            val first = received.incrementAndGet() == 1
            val payload = (if (first) "busy" else FakeMcpd.ACCEPTED).toByteArray()
            if (first) exchange.responseHeaders.add("Retry-After", seconds.toString())
            exchange.sendResponseHeaders(if (first) 503 else 200, payload.size.toLong())
            exchange.responseBody.use { it.write(payload) }
        }
        server.start()
    }

    override fun close() {
        server.stop(0)
    }
}
