package dev.mcpd

import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.util.concurrent.CopyOnWriteArrayList
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
import kotlin.time.Duration
import kotlin.time.Instant

/** Prompts (contract section 5.1) and the query filters that read them back. */
class PromptTest {

    private fun prompt(
        name: String = "support/answer",
        version: Int = 3,
        template: String? = "Hello {{ city }}",
        labels: String = """["production"]""",
    ): String = """
        {"project":"test","name":"$name","version":$version,"kind":"text",
        "variables":["city"],"labels":$labels,"created_at":"2026-05-01T00:00:00Z",
        "template":${Json.encodeToString(JsonPrimitive.serializer(), JsonPrimitive(template))}}
    """

    private fun textPrompt(template: String) = Prompt(
        project = "test", name = "p", version = 1, kind = PromptKind.TEXT, variables = emptyList(),
        labels = emptyList(), createdAt = Instant.parse("2026-05-01T00:00:00Z"), template = template,
    )

    // ----------------------------------------------------------- fetching ---

    @Test
    fun `the default label falls back to the latest version on a 404`() = runBlocking {
        FakeMcpd { req, _ ->
            if ("label=production" in req.path) 404 to """{"error":"not_found"}""" else 200 to prompt()
        }.use { server ->
            val mcpd = client(server)
            val got = mcpd.getPrompt("support/answer")
            assertEquals(3, got.version)
            assertEquals(
                listOf(
                    "/v1/api/prompts/support%2Fanswer?project=test&label=production",
                    "/v1/api/prompts/support%2Fanswer?project=test",
                ),
                server.requests.map { it.path },
                "a name with / is one path segment, and the retry carries no selector",
            )
            mcpd.shutdown()
        }
    }

    @Test
    fun `a 404 for a named label is not retried`() = runBlocking {
        FakeMcpd { _, _ -> 404 to "{}" }.use { server ->
            val mcpd = client(server)
            val failure = assertFailsWith<McpdException> { mcpd.getPrompt("p", label = "staging") }
            assertEquals(404, failure.status)
            assertEquals(listOf("/v1/api/prompts/p?project=test&label=staging"), server.requests.map { it.path })
            mcpd.shutdown()
        }
    }

    @Test
    fun `a fresh copy is served with no request`() = runBlocking {
        FakeMcpd { _, _ -> 200 to prompt() }.use { server ->
            val mcpd = client(server)
            val first = mcpd.getPrompt("p", version = 3)
            val second = mcpd.getPrompt("p", version = 3)
            assertEquals(first, second)
            assertEquals(1, server.requests.size, "the second read was served from the cache")
            mcpd.getPrompt("p", label = "production")
            assertEquals(2, server.requests.size, "another selector is another cache entry")
            assertContains(server.requests.last().path, "label=production")
            mcpd.shutdown()
        }
    }

    @Test
    fun `a stale copy is served when the refetch fails, and reported once`() = runBlocking {
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
            FakeMcpd { _, n -> if (n == 1) 200 to prompt() else 500 to """{"error":"down"}""" }.use { server ->
                val mcpd = client(server)
                val fresh = mcpd.getPrompt("p", cacheTtl = Duration.ZERO)
                val stale = mcpd.getPrompt("p", cacheTtl = Duration.ZERO)
                val again = mcpd.getPrompt("p", cacheTtl = Duration.ZERO)
                assertEquals(fresh, stale)
                assertEquals(fresh, again)
                assertEquals(3, server.requests.size, "cacheTtl = 0 fetches every time")
                assertEquals(1, captured.count { "could not refresh prompt p" in it }, "$captured")
                mcpd.shutdown()
            }
        } finally {
            logger.removeHandler(handler)
        }
    }

    @Test
    fun `a fallback is returned when nothing was fetched, and never cached`() = runBlocking {
        FakeMcpd { _, _ -> 500 to "{}" }.use { server ->
            val mcpd = client(server)
            val fallback = PromptFallback.Template("Hi {{name}}, from {{ city }} and {{name}}")
            val got = mcpd.getPrompt("greeting", label = "production", fallback = fallback)
            assertEquals("greeting", got.name)
            assertEquals(0, got.version)
            assertEquals(PromptKind.TEXT, got.kind)
            assertEquals(emptyList(), got.labels)
            assertEquals(listOf("name", "city"), got.variables)
            assertEquals("test", got.project)
            assertEquals("Hi Ann, from Oslo and Ann", got.renderText(mapOf("name" to "Ann", "city" to "Oslo")))

            mcpd.getPrompt("greeting", label = "production", fallback = fallback)
            assertEquals(2, server.requests.size, "the second call tried the daemon again")

            val chat = mcpd.getPrompt(
                "chat",
                version = 1,
                fallback = PromptFallback.Messages(
                    listOf(
                        mapOf("role" to "system", "content" to "Be {{tone}}"),
                        mapOf("role" to "user", "content" to "{{question}} {{tone}}"),
                    ),
                ),
            )
            assertEquals(PromptKind.CHAT, chat.kind)
            assertEquals(listOf("tone", "question"), chat.variables)

            val failure = assertFailsWith<McpdException> { mcpd.getPrompt("greeting", version = 2) }
            assertEquals(500, failure.status, "no copy and no fallback raises the fetch error")
            mcpd.shutdown()
        }
    }

    @Test
    fun `an unreachable daemon with a fallback still answers`() = runBlocking {
        val mcpd = client(endpoint = "http://127.0.0.1:1")
        val got = mcpd.getPrompt("p", fallback = PromptFallback.Template("x"))
        assertEquals("x", got.template)
        mcpd.shutdown()
    }

    @Test
    fun `both selectors are refused before the wire`() = runBlocking {
        FakeMcpd { _, _ -> 200 to prompt() }.use { server ->
            val mcpd = client(server)
            assertFailsWith<IllegalArgumentException> { mcpd.getPrompt("p", label = "production", version = 2) }
            assertEquals(0, server.requests.size)
            mcpd.shutdown()
        }
    }

    @Test
    fun `a disabled client still fetches`() = runBlocking {
        FakeMcpd { _, _ -> 200 to prompt() }.use { server ->
            val mcpd = client(server) { it.copy(enabled = false) }
            assertEquals(3, mcpd.getPrompt("p").version)
            assertEquals(1, server.requests.size)
            mcpd.shutdown()
        }
    }

    @Test
    fun `the cache holds at most 256 prompts`() = runBlocking {
        FakeMcpd { _, _ -> 200 to prompt() }.use { server ->
            val mcpd = client(server)
            repeat(PROMPT_CACHE_SIZE + 1) { i -> mcpd.getPrompt("p", version = i + 1) }
            mcpd.getPrompt("p", version = PROMPT_CACHE_SIZE + 1)
            assertEquals(PROMPT_CACHE_SIZE + 1, server.requests.size, "the newest is still cached")
            mcpd.getPrompt("p", version = 1)
            assertEquals(PROMPT_CACHE_SIZE + 2, server.requests.size, "the oldest was evicted")
            mcpd.shutdown()
        }
    }

    // ---------------------------------------------------------- rendering ---

    @Test
    fun `rendering allows whitespace in braces`() {
        assertEquals("Oslo, Oslo, Oslo", textPrompt("{{city}}, {{ city }}, {{\tcity  }}").renderText(mapOf("city" to "Oslo")))
    }

    @Test
    fun `a placeholder that is not a name is left as written`() {
        val template = "{{ not a name }} {{1x}} {{}} {\"a\": {{ x.y_2 }}} {{x"
        assertEquals("{{ not a name }} {{1x}} {{}} {\"a\": 7} {{x", textPrompt(template).renderText(mapOf("x.y_2" to 7)))
    }

    @Test
    fun `substitution is one pass`() {
        assertEquals("{{b}} and 2", textPrompt("{{a}} and {{b}}").renderText(mapOf("a" to "{{b}}", "b" to 2)))
    }

    @Test
    fun `every missing variable is named, in order of first use`() {
        val failure = assertFailsWith<PromptRenderException> {
            textPrompt("{{zeta}} {{alpha}} {{given}} {{zeta}} {{mid}}").renderText(mapOf("given" to "x", "extra" to 1))
        }
        assertEquals(listOf("zeta", "alpha", "mid"), failure.missing)
        assertContains(assertNotNull(failure.message), "zeta, alpha, mid")
    }

    @Test
    fun `chat content is rendered and every other field kept`() {
        val messages = Json.parseToJsonElement(
            """
            [{"role":"system","content":"You help in {{ city }}.","name":"{{city}}"},
             {"role":"user","content":[{"type":"text","text":"{{city}}"}]},
             "not an object"]
            """
        )
        val chat = textPrompt("").copy(kind = PromptKind.CHAT, template = null, messages = messages)
        val rendered = chat.renderMessages(mapOf("city" to "Oslo"))
        assertEquals(
            JsonObject(
                mapOf(
                    "role" to JsonPrimitive("system"),
                    "content" to JsonPrimitive("You help in Oslo."),
                    "name" to JsonPrimitive("{{city}}"),
                ),
            ),
            rendered[0],
        )
        assertEquals(messages.let { it as JsonArray }[1], rendered[1], "a content that is not a string is copied")
        assertEquals(JsonPrimitive("not an object"), rendered[2])
        assertFailsWith<PromptRenderException> { chat.renderMessages(emptyMap()) }
        assertFailsWith<IllegalStateException> { chat.renderText(emptyMap()) }
    }

    // ------------------------------------------------------------ linking ---

    @Test
    fun `setPrompt sets both attributes and never throws on a bad prompt`() = runBlocking {
        val recorder = RecordingSink()
        val mcpd = client { it.copy(sink = recorder) }
        val stored = testJson.decodeFromString(Prompt.serializer(), prompt())
        mcpd.trace("linked").setPrompt(stored).end()
        mcpd.trace("fallback").setPrompt(stored.copy(version = 0)).end()
        mcpd.trace("none").setPrompt(null).setPrompt(stored.copy(name = "")).end()
        mcpd.flush()

        val linked = assertNotNull(recorder.spans.last { it.name == "linked" }.attributes).jsonObject
        assertEquals("support/answer", linked["mcpd.prompt.name"]?.jsonPrimitive?.content)
        val version = assertNotNull(linked["mcpd.prompt.version"]).jsonPrimitive
        assertTrue(!version.isString, "the version is an integer, not a string")
        assertEquals(3, version.int)

        val fallback = assertNotNull(recorder.spans.last { it.name == "fallback" }.attributes).jsonObject
        assertEquals("support/answer", fallback["mcpd.prompt.name"]?.jsonPrimitive?.content)
        assertNull(fallback["mcpd.prompt.version"], "a fallback sets the name only")

        assertNull(recorder.spans.last { it.name == "none" }.attributes)
        mcpd.shutdown()
    }

    // ------------------------------------------------------------ writing ---

    @Test
    fun `createPrompt and setPromptLabel send the documented requests`() = runBlocking {
        FakeMcpd { _, _ -> 200 to prompt() }.use { server ->
            val mcpd = client(server)
            mcpd.createPrompt(
                "support/answer",
                messages = listOf(mapOf("role" to "user", "content" to "{{q}}")),
                labels = listOf("production"),
            )
            mcpd.setPromptLabel("support/answer", "production", 2)

            val (create, label) = server.requests
            assertEquals("POST", create.method)
            assertEquals("/v1/api/prompts?project=test", create.path)
            assertEquals(
                Json.parseToJsonElement(
                    """{"name":"support/answer","messages":[{"role":"user","content":"{{q}}"}],"labels":["production"]}""",
                ),
                Json.parseToJsonElement(create.body),
            )
            assertEquals("PUT", label.method)
            assertEquals("/v1/api/prompts/support%2Fanswer/labels/production?project=test", label.path)
            assertEquals(Json.parseToJsonElement("""{"version":2}"""), Json.parseToJsonElement(label.body))
            mcpd.shutdown()
        }
    }

    @Test
    fun `a refused write raises`() = runBlocking {
        FakeMcpd { _, _ -> 400 to """{"error":"bad_param"}""" }.use { server ->
            val mcpd = client(server)
            val failure = assertFailsWith<McpdException> { mcpd.createPrompt("p", template = " ") }
            assertEquals(400, failure.status)
            assertFailsWith<McpdException> { mcpd.setPromptLabel("p", "production", 9) }
            mcpd.shutdown()
        }
    }

    // ------------------------------------------------------------ filters ---

    @Test
    fun `listTraces sends the new filters and nothing else`() = runBlocking {
        FakeMcpd { _, _ -> 200 to """{"traces":[],"page":{"has_more":false}}""" }.use { server ->
            val mcpd = client(server)
            mcpd.listTraces(
                maxDurationMs = 900,
                maxCost = 0.5,
                minTokens = 1000,
                hasScore = "relevance",
                scoreAbove = 0.8,
                hasFeedback = false,
            )
            assertEquals(
                "/v1/api/traces?project=test&max_duration_ms=900&max_cost=0.5&min_tokens=1000" +
                    "&has_score=relevance&score_above=0.8&has_feedback=false",
                server.requests.single().path,
            )
            mcpd.listTraces(hasFeedback = true)
            assertEquals("/v1/api/traces?project=test&has_feedback=true", server.requests.last().path)
            mcpd.shutdown()
        }
    }

    @Test
    fun `listSpans sends the prompt filters and nothing else`() = runBlocking {
        FakeMcpd { _, _ -> 200 to """{"spans":[],"page":{"has_more":false}}""" }.use { server ->
            val mcpd = client(server)
            mcpd.listSpans(prompt = "support/answer", promptVersion = 3)
            assertEquals(
                "/v1/api/spans?project=test&prompt=support%2Fanswer&prompt_version=3",
                server.requests.single().path,
            )
            mcpd.shutdown()
        }
    }
}
