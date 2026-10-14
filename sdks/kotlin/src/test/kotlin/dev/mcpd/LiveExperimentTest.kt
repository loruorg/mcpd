package dev.mcpd

import java.net.URI
import java.net.http.HttpClient
import java.net.http.HttpRequest
import java.net.http.HttpResponse
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import org.junit.jupiter.api.Assumptions
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlin.test.fail

/**
 * The experiment runner against a running mcpd, with every empty shape it
 * sends. Skipped unless `MCPD_TEST_ENDPOINT` is set, like [LiveTest].
 */
class LiveExperimentTest {

    @Test
    fun `an experiment runs over nulls and empty lists`(): Unit = runBlocking {
        val endpoint = endpoint()
        val project = "sdk-live-kotlin-experiment"
        val mcpd = Mcpd(McpdConfig(endpoint = endpoint, project = project))
        try {
            val nested = Json.parseToJsonElement("""{"l":[],"n":null,"q":"hi"}""")
            val datasetId = seedDataset(endpoint, project, listOf(nested to "\"hi\"", JsonNull to "\"x\""))
            val seen = java.util.Collections.synchronizedList(mutableListOf<JsonElement>())
            val config = ScorerConfig(
                forbidden = emptyList(),
                keywords = emptyList(),
                expectedTools = emptyList(),
                forbiddenTools = emptyList(),
                context = emptyList(),
                needle = "hi",
            )
            val results = mcpd.runExperiment(datasetId, config = config) { input ->
                seen.add(input)
                "hi"
            }
            assertTrue(seen.contains(nested) && seen.contains(JsonNull), "inputs: $seen")
            assertEquals(listOf(2, 0, 2), listOf(results.runs, results.failed, results.items.size))

            val recorded = results.items.associate { item ->
                val root = mcpd.getTrace(item.traceId).spans.first { it.isRoot }
                item.itemId to root.input
            }
            assertEquals(nested, recorded["$datasetId-0"])
            assertTrue(recorded["$datasetId-1"] == null || recorded["$datasetId-1"] == JsonNull)

            val experiment = mcpd.createExperiment(
                Experiment(
                    experimentId = "",
                    name = "kt-live-empty",
                    datasetId = datasetId,
                    status = ExperimentStatus.RUNNING,
                    itemCount = 0,
                    correlation = Correlation(project = ""),
                )
            )
            mcpd.scoreExperiment(experiment.experimentId)
            mcpd.scoreExperiment(experiment.experimentId, emptyList(), config.copy(needle = null))
        } finally {
            mcpd.shutdown()
        }
    }

    /** A dataset holding the cases, written with plain HTTP so only the runner is under test. */
    private fun seedDataset(endpoint: String, project: String, cases: List<Pair<JsonElement, String>>): String {
        val id = "kt-live-${System.nanoTime()}"
        post(
            endpoint, "/v1/api/datasets",
            """{"dataset_id":"$id","name":"$id","item_count":0,""" +
                """"created_at":"2026-01-01T00:00:00Z","correlation":{"project":"$project"}}""",
        )
        val items = cases.mapIndexed { i, (input, expected) ->
            """{"item_id":"$id-$i","dataset_id":"$id","input":$input,"expected":$expected}"""
        }
        post(endpoint, "/v1/api/datasets/$id/items", items.joinToString(",", "[", "]"))
        return id
    }

    private fun post(endpoint: String, path: String, body: String) {
        val request = HttpRequest.newBuilder(URI.create(endpoint.trimEnd('/') + path))
            .header("content-type", "application/json")
            .POST(HttpRequest.BodyPublishers.ofString(body))
            .build()
        val answer = HttpClient.newHttpClient().send(request, HttpResponse.BodyHandlers.ofString())
        assertTrue(answer.statusCode() in 200..299, "$path: ${answer.statusCode()} ${answer.body()}")
    }

    private fun endpoint(): String {
        System.getenv("MCPD_TEST_ENDPOINT")?.trim()?.takeIf { it.isNotEmpty() }?.let { return it }
        if (System.getenv("MCPD_REQUIRE_LIVE") != null) {
            fail("MCPD_REQUIRE_LIVE is set but MCPD_TEST_ENDPOINT is not, so this suite would have skipped")
        }
        return Assumptions.abort("no MCPD_TEST_ENDPOINT; set it to a running daemon to run the live suite")
    }
}
