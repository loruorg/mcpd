package dev.mcpd

import java.security.SecureRandom
import java.util.concurrent.ThreadLocalRandom
import kotlin.coroutines.AbstractCoroutineContextElement
import kotlin.coroutines.CoroutineContext

/**
 * Trace and span identifiers, in OpenTelemetry's shapes.
 *
 * A trace id is 128 bits rendered as 32 lowercase hex characters, a span id is
 * 64 bits as 16. Keeping the wire format identical to OpenTelemetry's is what
 * lets an mcpd trace correlate with every other tool the process already talks
 * to; anything else quietly breaks that correlation.
 *
 * The bits come from [SecureRandom]. An id is a capability as much as a label:
 * it names a run in every query, header and log line it travels through, so a
 * generator whose next output can be worked out from the ones already seen
 * hands an observer ids it was never shown. The other five SDKs draw from their
 * platform's cryptographic source for the same reason.
 *
 * The fallback, if the JVM has no usable [SecureRandom] provider, is
 * [ThreadLocalRandom]. That id is weaker but it still correlates, and
 * instrumentation must never throw (B1) or block (B2), so degrading beats
 * failing the work being observed.
 */
object Ids {
    /** Characters in a hex-encoded trace id. */
    const val TRACE_ID_LENGTH: Int = 32

    /** Characters in a hex-encoded span id. */
    const val SPAN_ID_LENGTH: Int = 16

    private const val HEX_DIGITS = "0123456789abcdef"

    /**
     * The generator, built once for the life of the process, or null if this
     * JVM could not supply one.
     *
     * One instance rather than one per id: [SecureRandom] is thread-safe and
     * seeds itself on construction, so building one per span pays for entropy
     * the process already has. The no-argument constructor takes the platform's
     * default source, which does not wait for the entropy pool the way
     * `getInstanceStrong` can; an id must never cost the caller a block (B2).
     */
    private val source: SecureRandom? =
        try {
            SecureRandom()
        } catch (error: Throwable) {
            // A JVM with no random provider at all is the fallback's whole
            // reason to exist. Throwing out of this initialiser would take the
            // observed process down on the first span.
            null
        }

    /**
     * A new random trace id.
     *
     * OpenTelemetry reserves the all-zero id to mean "no trace", so on the
     * vanishingly rare occasion the generator produces it we draw again rather
     * than emit an id every downstream system will discard.
     */
    fun newTraceId(): String = randomHex(TRACE_ID_LENGTH / 2)

    /** A new random span id, with the same all-zero guarantee. */
    fun newSpanId(): String = randomHex(SPAN_ID_LENGTH / 2)

    /** Whether [value] is 32 lowercase hex characters and not the reserved all-zero id. */
    fun isValidTraceId(value: String): Boolean = isHexId(value, TRACE_ID_LENGTH)

    /** Whether [value] is 16 lowercase hex characters and not the reserved all-zero id. */
    fun isValidSpanId(value: String): Boolean = isHexId(value, SPAN_ID_LENGTH)

    private fun isHexId(value: String, length: Int): Boolean {
        if (value.length != length) return false
        var nonZero = false
        for (c in value) {
            if (c !in '0'..'9' && c !in 'a'..'f') return false
            if (c != '0') nonZero = true
        }
        return nonZero
    }

    /** `bytes` random bytes as lowercase hex, never the reserved all-zero id. */
    private fun randomHex(bytes: Int): String {
        val raw = ByteArray(bytes)
        var attempt = 0
        while (true) {
            fill(raw)
            if (raw.any { it.toInt() != 0 }) break
            // A working generator will not produce all zeroes twice, but a
            // broken one would spin here forever inside the caller's thread.
            // One byte set by hand is a valid id; a hang is not survivable.
            if (++attempt >= 2) {
                raw[0] = 1
                break
            }
        }
        val out = StringBuilder(bytes * 2)
        for (byte in raw) {
            val value = byte.toInt() and 0xff
            out.append(HEX_DIGITS[value ushr 4])
            out.append(HEX_DIGITS[value and 0x0f])
        }
        return out.toString()
    }

    /**
     * Fill [out] with random bytes, from [secure] where there is one.
     *
     * Internal rather than private so a test can drive the fallback with a null
     * source: a JVM with no working provider cannot be conjured in-process, and
     * a fallback no test has ever run is one that fails on the day it is needed.
     */
    internal fun fill(out: ByteArray, secure: SecureRandom? = source) {
        if (secure != null) {
            try {
                secure.nextBytes(out)
                return
            } catch (error: Throwable) {
                // A provider can also fail at draw time, not only at
                // construction: a native source whose device has gone throws
                // here. An id from a weaker generator is worth more to the
                // caller than an exception out of their instrumentation (B1).
            }
        }
        ThreadLocalRandom.current().nextBytes(out)
    }
}

/**
 * A parsed W3C `traceparent` header.
 *
 * Reading this on the way in and writing it on the way out is what makes one
 * trace survive a service boundary: the receiving process continues the trace
 * it was handed instead of starting an unrelated one.
 */
data class TraceParent(
    /** The trace this request belongs to. */
    val traceId: String,
    /** The caller's span, which becomes the parent of whatever we create. */
    val parentSpanId: String,
    /** The trace-flags byte. Bit 0 is the sampled flag. */
    val flags: Int,
) {
    /** Whether the upstream caller sampled this trace. */
    val sampled: Boolean get() = flags and 0x01 != 0

    /** Render back to the header form. */
    fun toHeader(): String = "00-$traceId-$parentSpanId-" + flags.toString(16).padStart(2, '0')

    companion object {
        /**
         * Parse a `traceparent` header value, or return null for anything
         * malformed.
         *
         * A bad header is never a reason to fail the caller's work: we start a
         * new trace instead and the run is still recorded.
         */
        fun parse(value: String?): TraceParent? {
            val raw = value?.trim()
            if (raw.isNullOrEmpty()) return null
            val parts = raw.split('-')
            if (parts.size < 4) return null
            val version = parts[0]
            val traceId = parts[1]
            val parentSpanId = parts[2]
            val flags = parts[3]
            // Version 00 has exactly four fields. Later versions may add more,
            // and are still usable as long as the ids parse.
            if (version == "00" && parts.size != 4) return null
            if (version.length != 2 || version.equals("ff", ignoreCase = true)) return null
            if (!version.all { it.isHexDigit() }) return null
            if (!Ids.isValidTraceId(traceId) || !Ids.isValidSpanId(parentSpanId)) return null
            if (flags.length != 2 || !flags.all { it.isHexDigit() }) return null
            val parsedFlags = flags.toIntOrNull(16) ?: return null
            return TraceParent(traceId, parentSpanId, parsedFlags)
        }

        private fun Char.isHexDigit(): Boolean =
            this in '0'..'9' || this in 'a'..'f' || this in 'A'..'F'
    }
}

/**
 * The active span, carried in the coroutine context.
 *
 * Coroutine context is Kotlin's own propagation mechanism, so a span started in
 * one suspend function is visible to every function it calls without either of
 * them passing it as an argument, and without a thread-local that a dispatcher
 * switch would lose.
 */
class McpdContext(
    /** The span every child operation should attach to. */
    val span: ActiveSpan,
) : AbstractCoroutineContextElement(Key) {
    companion object Key : CoroutineContext.Key<McpdContext>
}
