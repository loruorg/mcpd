package dev.mcpd

import java.security.SecureRandom
import java.util.Collections
import java.util.Random
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertSame
import kotlin.test.assertTrue

class IdsTest {

    @Test
    fun `generated ids have the OpenTelemetry shape`() {
        repeat(200) {
            val traceId = Ids.newTraceId()
            val spanId = Ids.newSpanId()
            assertEquals(32, traceId.length, "trace id length")
            assertEquals(16, spanId.length, "span id length")
            assertTrue(Ids.isValidTraceId(traceId), "$traceId should be valid")
            assertTrue(Ids.isValidSpanId(spanId), "$spanId should be valid")
        }
    }

    @Test
    fun `generated ids do not collide`() {
        val ids = (1..1000).map { Ids.newTraceId() }.toSet()
        assertEquals(1000, ids.size, "trace ids collided")
    }

    @Test
    fun `ids are drawn from one cryptographic source`() {
        // B6 ids identify a trace across systems, so the generator has to be a
        // CSPRNG: a predictable one lets an unrelated caller guess the ids of a
        // run it cannot see. The field is asserted rather than the output
        // because no sample of hex can tell the two sources apart.
        val sources = Ids::class.java.declaredFields.filter {
            Random::class.java.isAssignableFrom(it.type)
        }
        assertEquals(1, sources.size, "Ids should hold exactly one random source, found $sources")
        val field = sources.single()
        field.isAccessible = true
        val before = field.get(Ids)
        assertTrue(
            before is SecureRandom,
            "the id source must be a java.security.SecureRandom, was ${before?.javaClass?.name}",
        )
        repeat(50) { Ids.newTraceId(); Ids.newSpanId() }
        assertSame(before, field.get(Ids), "the source is held once, not built per id")
    }

    @Test
    fun `generated ids are lowercase hex and never all zero`() {
        val hex = Regex("\\A[0-9a-f]+\\z")
        repeat(500) {
            val traceId = Ids.newTraceId()
            val spanId = Ids.newSpanId()
            assertEquals(32, traceId.length, "trace id length")
            assertEquals(16, spanId.length, "span id length")
            assertTrue(hex.matches(traceId), "$traceId is not lowercase hex")
            assertTrue(hex.matches(spanId), "$spanId is not lowercase hex")
            assertFalse(traceId == "0".repeat(32), "the all-zero trace id is reserved")
            assertFalse(spanId == "0".repeat(16), "the all-zero span id is reserved")
        }
    }

    @Test
    fun `two ids drawn in a row differ`() {
        assertTrue(Ids.newTraceId() != Ids.newTraceId(), "two trace ids were identical")
        assertTrue(Ids.newSpanId() != Ids.newSpanId(), "two span ids were identical")
    }

    @Test
    fun `a shared source still gives every thread a distinct id`() {
        // One SecureRandom is now shared by every caller. If that sharing were
        // unsafe, two threads would see the same draw, which is the failure this
        // catches.
        val threads = 8
        val perThread = 500
        val ids = Collections.synchronizedList(mutableListOf<String>())
        val start = CountDownLatch(1)
        val done = CountDownLatch(threads)
        repeat(threads) {
            Thread {
                start.await()
                repeat(perThread) { ids.add(Ids.newSpanId()) }
                done.countDown()
            }.start()
        }
        start.countDown()
        assertTrue(done.await(30, TimeUnit.SECONDS), "id generation did not finish")
        assertEquals(threads * perThread, ids.size, "lost ids")
        assertTrue(ids.all { Ids.isValidSpanId(it) }, "a thread produced a malformed span id")
        assertEquals(ids.size, ids.toSet().size, "two threads drew the same span id")
    }

    @Test
    fun `the documented fallback still produces bytes with no secure source`() {
        // P011 asks for a CSPRNG with a documented fallback. This is the
        // fallback: a JVM with no usable provider degrades to a weaker id
        // rather than throwing out of the caller's instrumentation (B1).
        val draws = (1..4).map {
            val out = ByteArray(16)
            Ids.fill(out, null)
            out.toList()
        }
        assertTrue(
            draws.any { bytes -> bytes.any { it.toInt() != 0 } },
            "the fallback filled nothing",
        )
        assertTrue(draws.toSet().size > 1, "the fallback returned the same bytes every time")
    }

    @Test
    fun `the reserved all-zero ids are rejected`() {
        assertFalse(Ids.isValidTraceId("0".repeat(32)))
        assertFalse(Ids.isValidSpanId("0".repeat(16)))
    }

    @Test
    fun `wrong length, wrong case and non-hex are rejected`() {
        assertFalse(Ids.isValidTraceId("abc"))
        assertFalse(Ids.isValidTraceId("A".repeat(32)), "uppercase is not the wire form")
        assertFalse(Ids.isValidTraceId("g".repeat(32)))
        assertFalse(Ids.isValidSpanId("a".repeat(32)))
    }

    @Test
    fun `a traceparent round-trips`() {
        val header = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
        val parsed = assertNotNull(TraceParent.parse(header))
        assertEquals("4bf92f3577b34da6a3ce929d0e0e4736", parsed.traceId)
        assertEquals("00f067aa0ba902b7", parsed.parentSpanId)
        assertTrue(parsed.sampled)
        assertEquals(header, parsed.toHeader())
    }

    @Test
    fun `the sampled flag is read`() {
        val parsed = assertNotNull(
            TraceParent.parse("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00")
        )
        assertFalse(parsed.sampled)
    }

    @Test
    fun `malformed headers parse to null rather than throwing`() {
        val bad = listOf(
            "ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
            "00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
            "",
            "garbage",
            null,
        )
        for (header in bad) {
            assertNull(TraceParent.parse(header), "should have rejected ${header ?: "null"}")
        }
    }

    @Test
    fun `a later version may carry extra fields`() {
        // Forward compatibility: a version we have never seen is still usable
        // as long as the ids parse.
        assertNotNull(
            TraceParent.parse("01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-something")
        )
    }
}
