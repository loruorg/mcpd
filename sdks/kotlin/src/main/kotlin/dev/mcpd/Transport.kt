package dev.mcpd

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import kotlinx.serialization.DeserializationStrategy
import kotlinx.serialization.json.Json
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.net.URI
import java.net.URLEncoder
import java.net.http.HttpClient
import java.net.http.HttpRequest
import java.net.http.HttpResponse
import java.nio.charset.StandardCharsets
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ThreadLocalRandom
import java.util.logging.Logger
import java.util.zip.GZIPOutputStream
import kotlin.time.Duration
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds
import kotlin.time.toJavaDuration

/** Bodies larger than this are gzipped. Below it the compression costs more than it saves. */
private const val GZIP_THRESHOLD = 1024

/** Total attempts for one request, the first included. */
private const val MAX_ATTEMPTS = 3

private val BASE_BACKOFF = 250.milliseconds
private val MAX_BACKOFF = 10.seconds

private const val INGEST_PATH = "/v1/ingest"

/**
 * Rate-limited reporting of the SDK's own failures.
 *
 * An exporter that cannot reach the server would otherwise print one line per
 * batch forever, which is how an observability tool ends up being the thing
 * that fills the disk. Each distinct problem is reported at most once per
 * interval and swallowed the rest of the time.
 */
internal class Diagnostics(private val interval: Duration) {
    private val logger: Logger = Logger.getLogger("dev.mcpd")
    private val lastReported = ConcurrentHashMap<String, Long>()

    fun warn(key: String, message: () -> String) {
        val now = System.nanoTime()
        var emit = false
        lastReported.compute(key) { _, previous ->
            if (previous == null || now - previous >= interval.inWholeNanoseconds) {
                emit = true
                now
            } else {
                previous
            }
        }
        // Building the message is deferred so the suppressed case costs nothing.
        if (emit) logger.warning("mcpd: " + message())
    }
}

/**
 * Thrown by the transport's [Sink] face when a batch did not land.
 *
 * The reason was reported by [Transport.sendBatch] already, so the exporter
 * counts the failure and says nothing more. No stack: nothing here is a bug.
 */
internal class BatchRefused : RuntimeException("the batch was not accepted") {
    override fun fillInStackTrace(): Throwable = this
}

/** What one HTTP attempt sequence ended in. */
internal sealed class Outcome {
    /** The server answered, whatever it said. */
    class Received(val response: HttpResponse<String>) : Outcome()

    /** No answer at all: connection refused, DNS failure, timeout. */
    class Unreachable(val cause: Throwable) : Outcome()
}

/**
 * The HTTP layer.
 *
 * Uses the JDK's own client so the SDK adds no HTTP dependency to an agent that
 * already has enough of them. The client's blocking `send` runs on
 * [Dispatchers.IO], which is what that dispatcher exists for and what keeps the
 * call cancellable from the caller's coroutine.
 */
internal class Transport(
    private val config: McpdConfig,
    private val json: Json,
    private val diagnostics: Diagnostics,
) : Sink {
    private val http: HttpClient = HttpClient.newBuilder()
        .connectTimeout(config.timeout.toJavaDuration())
        // Cleartext HTTP/2 would cost an upgrade round trip on every batch and
        // buy nothing for a handful of small POSTs.
        .version(HttpClient.Version.HTTP_1_1)
        .followRedirects(HttpClient.Redirect.NORMAL)
        .build()

    /** The default [Sink]: [sendBatch], with a refusal turned into the throw the exporter counts. */
    override suspend fun export(batch: IngestBatch, ingestKey: String) {
        if (!sendBatch(batch)) throw BatchRefused()
    }

    /**
     * Send one batch. Never throws: a failed export is reported once per
     * interval and the batch is dropped.
     *
     * Returns whether the server accepted it.
     */
    suspend fun sendBatch(batch: IngestBatch): Boolean {
        val request = try {
            buildIngestRequest(batch)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            diagnostics.warn("encode") { "could not encode a batch, dropping it: $e" }
            return false
        }
        return when (val outcome = execute(request)) {
            is Outcome.Received -> {
                val code = outcome.response.statusCode()
                if (code in 200..299) {
                    true
                } else {
                    diagnostics.warn("ingest-$code") {
                        "mcpd refused a batch with HTTP $code: ${excerpt(outcome.response.body())}"
                    }
                    false
                }
            }

            is Outcome.Unreachable -> {
                diagnostics.warn("unreachable") {
                    "cannot reach mcpd at ${config.endpoint}, dropping the batch: ${outcome.cause}"
                }
                false
            }
        }
    }

    /**
     * Read an endpoint and decode the response.
     *
     * Reads are the one part of this SDK that throws. A query is something the
     * caller asked for and is waiting on, so an empty result where there should
     * have been an error would be a lie; instrumentation, which the caller is
     * not waiting on, stays silent instead.
     */
    suspend fun <T> getJson(
        path: String,
        params: List<Pair<String, String?>>,
        deserializer: DeserializationStrategy<T>,
    ): T {
        val request = HttpRequest.newBuilder(URI.create(url(path, params)))
            .timeout(config.timeout.toJavaDuration())
            .header("accept", "application/json")
            .header("user-agent", USER_AGENT)
            .also { builder -> config.apiKey?.let { builder.header("authorization", "Bearer $it") } }
            .GET()
            .build()
        return when (val outcome = execute(request)) {
            is Outcome.Received -> {
                val response = outcome.response
                if (response.statusCode() !in 200..299) {
                    throw McpdException(
                        "mcpd answered HTTP ${response.statusCode()} for $path: ${excerpt(response.body())}"
                    )
                }
                try {
                    json.decodeFromString(deserializer, response.body())
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    throw McpdException("could not read the response from $path: ${e.message}", e)
                }
            }

            is Outcome.Unreachable ->
                throw McpdException("cannot reach mcpd at ${config.endpoint}: ${outcome.cause}", outcome.cause)
        }
    }

    /**
     * POST a JSON body to a run endpoint and decode the answer.
     *
     * No retry: a worker call that is retried blindly can land twice, and the
     * daemon's own 409 table is how the caller learns what happened. A 409
     * carrying one of the four closed reasons is thrown as
     * [StepConflictException]; anything else that is not 2xx is an
     * [McpdException].
     *
     * [wait] is how long the daemon may hold the request open before it
     * answers, as it does for a long poll. The request's timeout is that plus
     * the configured timeout; otherwise the client would cancel a poll longer
     * than its own timeout, which is every idle claim.
     */
    suspend fun <T> postJson(
        path: String,
        params: List<Pair<String, String?>>,
        body: String,
        deserializer: DeserializationStrategy<T>,
        wait: Duration = Duration.ZERO,
    ): T {
        val request = HttpRequest.newBuilder(URI.create(url(path, params)))
            .timeout((config.timeout + wait).toJavaDuration())
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .header("user-agent", USER_AGENT)
            .also { builder -> config.apiKey?.let { builder.header("authorization", "Bearer $it") } }
            .POST(HttpRequest.BodyPublishers.ofString(body))
            .build()
        val response = try {
            withContext(Dispatchers.IO) { http.send(request, HttpResponse.BodyHandlers.ofString()) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            throw McpdException("cannot reach mcpd at ${config.endpoint}: $e", e)
        }
        val code = response.statusCode()
        if (code == 409) {
            val conflict = try {
                json.decodeFromString(StepConflict.serializer(), response.body())
            } catch (e: Exception) {
                null
            }
            if (conflict != null) throw StepConflictException(conflict)
        }
        if (code !in 200..299) {
            throw McpdException("mcpd answered HTTP $code for $path: ${excerpt(response.body())}")
        }
        return try {
            json.decodeFromString(deserializer, response.body())
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            throw McpdException("could not read the response from $path: ${e.message}", e)
        }
    }

    /**
     * Run a request, retrying the failures that are worth retrying.
     *
     * 429 and the three gateway statuses mean "try again"; a connection error
     * means the same. Any other 4xx means the batch is malformed and will stay
     * malformed, so retrying it only wastes the server's time.
     */
    private suspend fun execute(request: HttpRequest): Outcome {
        var attempt = 1
        while (true) {
            var response: HttpResponse<String>? = null
            var failure: Exception? = null
            try {
                response = withContext(Dispatchers.IO) {
                    http.send(request, HttpResponse.BodyHandlers.ofString())
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                failure = e
            }

            val last = attempt >= MAX_ATTEMPTS
            if (response != null && (!isRetryable(response.statusCode()) || last)) {
                return Outcome.Received(response)
            }
            if (response == null && last) {
                return Outcome.Unreachable(failure ?: IOException("request failed"))
            }
            delay(backoff(attempt, response))
            attempt++
        }
    }

    private fun isRetryable(status: Int): Boolean =
        status == 429 || status == 502 || status == 503 || status == 504

    /**
     * How long to wait before the next attempt.
     *
     * `Retry-After` wins when the server sent one: it knows when it will be
     * ready and we do not. Otherwise exponential backoff with jitter, so a
     * fleet that lost the server at the same moment does not return in lockstep.
     */
    private fun backoff(attempt: Int, response: HttpResponse<String>?): Duration {
        val advised = response?.headers()?.firstValue("retry-after")?.orElse(null)?.trim()?.toLongOrNull()
        if (advised != null && advised >= 0) return minOf(advised.seconds, MAX_BACKOFF)
        val exponential = BASE_BACKOFF * (1 shl (attempt - 1))
        val jitter = 0.5 + ThreadLocalRandom.current().nextDouble()
        return minOf(exponential * jitter, MAX_BACKOFF)
    }

    private fun buildIngestRequest(batch: IngestBatch): HttpRequest {
        val payload = json.encodeToString(IngestBatch.serializer(), batch).toByteArray(StandardCharsets.UTF_8)
        val compress = payload.size > GZIP_THRESHOLD
        val body = if (compress) gzip(payload) else payload
        val builder = HttpRequest.newBuilder(URI.create(config.endpoint.trimEnd('/') + INGEST_PATH))
            .timeout(config.timeout.toJavaDuration())
            .header("content-type", "application/json")
            .header("user-agent", USER_AGENT)
        if (compress) builder.header("content-encoding", "gzip")
        config.apiKey?.let { builder.header("authorization", "Bearer $it") }
        return builder.POST(HttpRequest.BodyPublishers.ofByteArray(body)).build()
    }

    private fun gzip(data: ByteArray): ByteArray {
        val out = ByteArrayOutputStream(data.size / 2)
        GZIPOutputStream(out).use { it.write(data) }
        return out.toByteArray()
    }

    private fun url(path: String, params: List<Pair<String, String?>>): String {
        val base = config.endpoint.trimEnd('/') + path
        val query = params
            .mapNotNull { (key, value) -> if (value.isNullOrEmpty()) null else encode(key) + "=" + encode(value) }
            .joinToString("&")
        return if (query.isEmpty()) base else "$base?$query"
    }

    private fun encode(value: String): String =
        // URLEncoder is form encoding, where a space is `+`. In a query string
        // that is legal but ambiguous, and some servers read it literally.
        URLEncoder.encode(value, StandardCharsets.UTF_8).replace("+", "%20")

    /** Enough of a response body to diagnose it, never enough to fill a log file. */
    private fun excerpt(body: String): String =
        if (body.length <= 200) body else body.take(200) + "..."
}
