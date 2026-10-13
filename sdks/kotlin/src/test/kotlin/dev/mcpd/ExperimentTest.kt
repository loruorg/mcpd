package dev.mcpd

import dev.mcpd.testing.TestClient
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.Test
import kotlin.test.assertContains
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Experiments (contract section 5.2).
 *
 * Reads and writes go to a fake daemon, so each assertion is on what crossed
 * the wire. The runs go to a [TestClient]'s recording sink, so the spans are
 * read back as they would have been sent.
 */
class ExperimentTest {

    private fun body(request: Received): JsonObject = testJson.parseToJsonElement(request.body).jsonObject

    private fun pathOf(request: Received): String = request.path.substringBefore('?')

    /** A daemon that answers the experiment endpoints, with [items] as the dataset. */
    private fun daemon(items: String, itemsStatus: Int = 200): FakeMcpd = FakeMcpd { request, _ ->
        when ("${request.method} ${pathOf(request)}") {
            "POST /v1/api/experiments" -> {
                val posted = body(request)
                200 to JsonObject(posted + ("experiment_id" to JsonPrimitive("exp1"))).toString()
            }
            "GET /v1/api/datasets/ds1/items" -> itemsStatus to items
            "POST /v1/api/experiments/exp1/score" ->
                200 to """{"traces_scored":3,"scores_written":3,"skipped":[],"scores":[]}"""
            "GET /v1/api/experiments/exp1/results" -> 200 to RESULTS
            "GET /v1/api/traces/t%2F1/trajectory" -> 200 to TRAJECTORY
            else -> 404 to """{"error":"no route"}"""
        }
    }

    private fun testClient(server: FakeMcpd, sample: Double = 1.0): TestClient =
        TestClient(McpdConfig(endpoint = server.endpoint, project = "p"), sample = sample)

    private fun experimentPosts(server: FakeMcpd): List<JsonObject> =
        server.requests.filter { it.method == "POST" && it.path == "/v1/api/experiments" }.map(::body)

    @Test
    fun `each read and write goes to its path with its body`() = runBlocking {
        daemon(ITEMS).use { server ->
            val mcpd = client(server) { it.copy(project = "p") }

            val trajectory = mcpd.getTrajectory("t/1")
            assertEquals(listOf("search"), trajectory.toolSequence)
            val items = mcpd.listDatasetItems("ds1", limit = 10)
            assertEquals(
                testJson.parseToJsonElement("""{"question":"capital of France","userName":"ada"}"""),
                items[0].input,
                "a case's input arrives with its keys as they were stored",
            )
            mcpd.createExperiment(
                Experiment(
                    experimentId = "",
                    name = "n",
                    datasetId = "ds1",
                    status = ExperimentStatus.PENDING,
                    itemCount = 0,
                    correlation = Correlation(project = ""),
                    metadata = testJson.parseToJsonElement("""{"promptVersion":3}"""),
                )
            )
            val schema = testJson.parseToJsonElement("""{"required":["userName"]}""")
            mcpd.scoreExperiment("exp1", listOf("json_schema"), scorerConfig(jsonSchema = schema))
            val results = mcpd.experimentResults("exp1")
            assertEquals(testJson.parseToJsonElement("""{"exact_match":1}"""), results.items[0].scores)
            assertEquals("exact_match", results.scorers[0].scorerId)

            assertEquals(
                listOf(
                    "GET /v1/api/traces/t%2F1/trajectory?project=p",
                    "GET /v1/api/datasets/ds1/items?limit=10",
                    "POST /v1/api/experiments",
                    "POST /v1/api/experiments/exp1/score?project=p",
                    "GET /v1/api/experiments/exp1/results?project=p",
                ),
                server.requests.map { "${it.method} ${it.path}" },
            )
            assertEquals(
                testJson.parseToJsonElement(
                    """{"experiment_id":"","name":"n","dataset_id":"ds1","status":"pending","item_count":0,
                       "correlation":{"project":"p"},"metadata":{"promptVersion":3}}"""
                ),
                body(server.requests[2]),
                "the client's project fills an empty correlation, and metadata keeps its keys",
            )
            val score = body(server.requests[3])
            assertEquals(setOf("scorers", "config"), score.keys)
            assertEquals("json_schema", score.getValue("scorers").toString().trim('[', ']', '"'))
            assertEquals(schema, score.getValue("config").jsonObject["json_schema"])

            mcpd.scoreExperiment("exp1")
            assertEquals("""{"scorers":[]}""", server.requests.last().body, "no config, no config key")
            mcpd.shutdown()
        }
    }

    @Test
    fun `a run gives every case its own span, records a failure, carries on and scores`() = runBlocking {
        daemon(ITEMS).use { server ->
            // Sampled at zero: the case spans are kept regardless.
            val test = testClient(server, sample = 0.0)
            val mcpd = test.client

            val results = mcpd.runExperiment(
                "ds1",
                name = "capitals-v2",
                scorers = listOf("exact_match"),
                metadata = mapOf("prompt" to "v2"),
            ) { input ->
                val question = input.jsonObject.getValue("question").jsonPrimitive.content
                if (question == "boom") throw IllegalStateException("the model refused")
                mcpd.span("lookup") { }
                if (question.endsWith("France")) "Paris" else "Madrid"
            }
            assertEquals(3, results.runs)

            val roots = test.spans(name = "capitals-v2")
            assertEquals(3, roots.size)
            val byItem = roots.associateBy {
                it.attributes?.jsonObject?.get(DATASET_ITEM_ATTRIBUTE)?.jsonPrimitive?.content
            }
            for (span in roots) {
                assertEquals(SpanKind.AGENT_RUN, span.kind)
                assertEquals("exp1", span.correlation.experimentId)
                assertNull(span.parentSpanId)
            }
            val a = assertNotNull(byItem["a"])
            val b = assertNotNull(byItem["b"])
            val c = assertNotNull(byItem["c"])
            assertEquals(testJson.parseToJsonElement("""{"question":"capital of France","userName":"ada"}"""), a.input)
            assertEquals(JsonPrimitive("Paris"), a.output)
            assertEquals(SpanStatus.OK, a.status)
            assertEquals(SpanStatus.ERROR, b.status)
            assertEquals("the model refused", b.error?.message)
            assertEquals(JsonPrimitive("Madrid"), c.output, "the case after the failing one still ran")

            val lookups = test.spans(name = "lookup")
            assertEquals(setOf(a.spanId, c.spanId), lookups.map { it.parentSpanId }.toSet())
            assertTrue(lookups.all { it.traceId == a.traceId || it.traceId == c.traceId })

            val posts = experimentPosts(server)
            assertEquals(2, posts.size)
            val running = posts[0]
            assertEquals(JsonPrimitive(""), running["experiment_id"])
            assertEquals(JsonPrimitive("capitals-v2"), running["name"])
            assertEquals(JsonPrimitive("ds1"), running["dataset_id"])
            assertEquals(JsonPrimitive("running"), running["status"])
            assertEquals(testJson.parseToJsonElement("""{"project":"p"}"""), running["correlation"])
            assertEquals(testJson.parseToJsonElement("""{"prompt":"v2"}"""), running["metadata"])
            assertTrue(running.getValue("started_at").jsonPrimitive.content.matches(Regex("""^\d{4}-.*Z$""")))
            val succeeded = posts[1]
            assertEquals(JsonPrimitive("exp1"), succeeded["experiment_id"])
            assertEquals(JsonPrimitive("succeeded"), succeeded["status"])
            assertEquals(JsonPrimitive(3), succeeded["item_count"])
            assertEquals(testJson.parseToJsonElement("""{"exact_match":0.5}"""), succeeded["summary"])
            assertEquals(running["started_at"], succeeded["started_at"], "the second post keeps the first's fields")
            assertTrue(succeeded.getValue("finished_at").jsonPrimitive.content.matches(Regex("""^\d{4}-.*Z$""")))

            val score = server.requests.single { it.path.contains("/score") }
            assertEquals("""{"scorers":["exact_match"]}""", score.body)
            assertEquals(
                "/v1/api/datasets/ds1/items?limit=5000",
                server.requests.single { it.path.contains("/items") }.path,
            )
            test.shutdown()
        }
    }

    @Test
    fun `never more than concurrency targets are in flight`() = runBlocking {
        val many = (0 until 12).joinToString(",", "[", "]") { """{"item_id":"i$it","dataset_id":"ds1","input":$it}""" }
        daemon(many).use { server ->
            val test = testClient(server)
            val inFlight = AtomicInteger()
            val most = AtomicInteger()
            test.client.runExperiment("ds1", concurrency = 3) { input ->
                most.accumulateAndGet(inFlight.incrementAndGet(), ::maxOf)
                delay(5)
                inFlight.decrementAndGet()
                input
            }
            assertEquals(3, most.get())
            assertEquals(12, test.spans(name = "ds1").size)
            test.shutdown()
        }
    }

    @Test
    fun `a failed read marks the experiment failed and throws, and no target runs`() = runBlocking {
        daemon("""{"error":"dataset belongs to another project"}""", itemsStatus = 403).use { server ->
            val test = testClient(server)
            var called = false
            val failure = assertFailsWith<McpdException> {
                test.client.runExperiment("ds1") { called = true }
            }
            assertEquals(403, failure.status)
            assertFalse(called)
            val posts = experimentPosts(server)
            assertEquals(listOf("running", "failed"), posts.map { it.getValue("status").jsonPrimitive.content })
            assertEquals(JsonPrimitive("exp1"), posts[1]["experiment_id"])
            assertContains(posts[1].getValue("error").jsonPrimitive.content, "403")
            assertNotNull(posts[1]["finished_at"])
            test.shutdown()
        }
    }

    @Test
    fun `a concurrency below 1 and a disabled client are refused before any request`() = runBlocking {
        daemon(ITEMS).use { server ->
            val test = testClient(server)
            val low = assertFailsWith<IllegalArgumentException> {
                test.client.runExperiment("ds1", concurrency = 0) { 1 }
            }
            assertContains(assertNotNull(low.message), "at least 1")
            val off = TestClient(McpdConfig(endpoint = server.endpoint, project = "p"), enabled = false)
            val disabled = assertFailsWith<IllegalStateException> { off.client.runExperiment("ds1") { 1 } }
            assertContains(assertNotNull(disabled.message), "enabled")
            assertEquals(0, server.requests.size)
            off.shutdown()
            test.shutdown()
        }
    }

    private fun scorerConfig(jsonSchema: JsonElement): ScorerConfig = ScorerConfig(
        forbidden = emptyList(),
        keywords = emptyList(),
        expectedTools = emptyList(),
        forbiddenTools = emptyList(),
        context = emptyList(),
        jsonSchema = jsonSchema,
    )

    private companion object {
        const val ITEMS = """[
            {"item_id":"a","dataset_id":"ds1","input":{"question":"capital of France","userName":"ada"}},
            {"item_id":"b","dataset_id":"ds1","input":{"question":"boom"}},
            {"item_id":"c","dataset_id":"ds1","input":{"question":"capital of Spain"}}
        ]"""

        const val RESULTS = """{"experiment_id":"exp1",
            "items":[{"trace_id":"t1","item_id":"a","status":"ok","scores":{"exact_match":1}}],
            "scorers":[{"scorer_id":"exact_match","n":2,"mean":0.5,"min":0,"max":1,
                        "direction":"higher_is_better"}],
            "runs":3,"failed":1}"""

        const val TRAJECTORY = """{"trace_id":"t/1","steps":[],"tool_sequence":["search"],"step_count":1,
            "tool_count":1,"error_count":0,"goal_reached":true}"""
    }
}
