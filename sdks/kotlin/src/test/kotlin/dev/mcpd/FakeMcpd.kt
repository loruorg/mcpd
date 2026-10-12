package dev.mcpd

import com.sun.net.httpserver.HttpExchange
import com.sun.net.httpserver.HttpServer
import kotlinx.serialization.json.Json
import java.net.InetSocketAddress
import java.util.concurrent.CopyOnWriteArrayList
import java.util.zip.GZIPInputStream
import kotlin.time.Duration.Companion.hours
import kotlin.time.Duration.Companion.seconds

/** One request the fake server received, decoded far enough to assert on. */
class Received(
    val path: String,
    val method: String,
    val headers: Map<String, List<String>>,
    val body: String,
) {
    fun header(name: String): String? =
        headers.entries.firstOrNull { it.key.equals(name, ignoreCase = true) }?.value?.firstOrNull()

    fun batch(): IngestBatch = testJson.decodeFromString(IngestBatch.serializer(), body)
}

internal val testJson = Json {
    ignoreUnknownKeys = true
    explicitNulls = false
}

/**
 * A stand-in for mcpd on a loopback port.
 *
 * The tests assert on what actually crossed the wire rather than on the SDK's
 * own idea of what it sent, which is the only way to catch a serialisation bug.
 * [reply] is given the request and the number received so far, so a test can
 * answer by path, or fail the first two attempts and succeed on the third.
 */
class FakeMcpd(
    private val reply: (Received, Int) -> Pair<Int, String> = { _, _ -> 200 to ACCEPTED },
) : AutoCloseable {
    val requests = CopyOnWriteArrayList<Received>()

    private val server: HttpServer = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)

    /** Where to point a client. */
    val endpoint: String get() = "http://127.0.0.1:" + server.address.port

    init {
        server.createContext("/") { exchange -> handle(exchange) }
        server.start()
    }

    private fun handle(exchange: HttpExchange) {
        val raw = exchange.requestBody.use { it.readBytes() }
        val body = if (exchange.requestHeaders.getFirst("Content-Encoding") == "gzip") {
            GZIPInputStream(raw.inputStream()).use { it.readBytes() }.decodeToString()
        } else {
            raw.decodeToString()
        }
        val received = Received(
            path = exchange.requestURI.toString(),
            method = exchange.requestMethod,
            headers = exchange.requestHeaders.entries.associate { it.key to it.value.toList() },
            body = body,
        )
        requests.add(received)
        val (status, payload) = reply(received, requests.size)
        val bytes = payload.toByteArray()
        exchange.responseHeaders.add("Content-Type", "application/json")
        exchange.sendResponseHeaders(status, bytes.size.toLong())
        exchange.responseBody.use { it.write(bytes) }
    }

    override fun close() {
        server.stop(0)
    }

    companion object {
        const val ACCEPTED = """{"accepted":1,"rejected":0,"duplicate":0}"""
    }
}

/** A client pointed at [server], with the worker asleep so tests flush explicitly. */
internal fun client(
    server: FakeMcpd? = null,
    endpoint: String? = null,
    build: (McpdConfig) -> McpdConfig = { it },
): Mcpd = Mcpd(
    build(
        McpdConfig(
            endpoint = endpoint ?: server?.endpoint ?: "http://127.0.0.1:1",
            project = "test",
            environment = "test",
            serviceName = "sdk-test",
            // Long enough that nothing exports until a test asks it to.
            flushInterval = 1.hours,
            timeout = 5.seconds,
        )
    )
)
