package dev.mcpd

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import java.util.concurrent.atomic.AtomicInteger
import kotlin.time.Clock

/** The attribute the daemon reads to find the case a run answered. */
const val DATASET_ITEM_ATTRIBUTE: String = "mcpd.dataset_item_id"

/** The most cases the daemon answers in one page. */
private const val MAX_CASES = 5000

/**
 * The code under test in [Mcpd.runExperiment]: called with each case's input,
 * and what it returns is the answer, recorded as the case span's output.
 */
typealias ExperimentTarget = suspend (input: JsonElement) -> Any?

/**
 * The experiment runner (SDK contract 5.2).
 *
 * The daemon never calls the agent under test. This does, in your process,
 * once per dataset case inside a root span that names the experiment and the
 * case, and then asks the daemon to score what was recorded.
 */
internal class ExperimentRunner(private val client: Mcpd) {

    suspend fun run(
        datasetId: String,
        name: String,
        scorers: List<String>,
        config: ScorerConfig?,
        concurrency: Int,
        metadata: Map<String, Any?>?,
        target: ExperimentTarget,
    ): ExperimentResults {
        check(client.config.enabled) {
            "runExperiment needs an enabled client: no run would reach the daemon to be scored. " +
                "Build the client with enabled = true, or unset MCPD_ENABLED"
        }
        require(concurrency >= 1) { "runExperiment concurrency must be at least 1, got $concurrency" }

        val draft = Experiment(
            experimentId = "",
            name = name,
            datasetId = datasetId,
            status = ExperimentStatus.RUNNING,
            itemCount = 0,
            correlation = Correlation(project = client.config.project),
            startedAt = Clock.System.now(),
            metadata = metadata?.let(::toJsonElement),
        )
        var created: Experiment? = null
        try {
            val experiment = client.createExperiment(draft)
            created = experiment
            val items = client.listDatasetItems(datasetId, MAX_CASES)

            // A fixed set of workers pulls the next case, so at most
            // `concurrency` targets are in flight however long the dataset is.
            val next = AtomicInteger()
            coroutineScope {
                repeat(minOf(concurrency, items.size)) {
                    launch {
                        while (true) {
                            val index = next.getAndIncrement()
                            if (index >= items.size) break
                            runCase(experiment, items[index], target)
                        }
                    }
                }
            }

            if (!client.flush()) {
                throw McpdException("the runs did not reach the daemon before the flush deadline, so none was scored")
            }
            client.scoreExperiment(experiment.experimentId, scorers, config)
            val results = client.experimentResults(experiment.experimentId)
            val summary = JsonObject(results.scorers.associate { it.scorerId to JsonPrimitive(it.mean) })
            client.createExperiment(
                experiment.copy(
                    status = ExperimentStatus.SUCCEEDED,
                    finishedAt = Clock.System.now(),
                    itemCount = items.size,
                    summary = summary,
                )
            )
            return results
        } catch (e: Throwable) {
            val experiment = created
            if (experiment != null) recordFailure(experiment, e)
            throw e
        }
    }

    /** One case: a root span around the target. A target that throws fails its own span only. */
    private suspend fun runCase(experiment: Experiment, item: DatasetItem, target: ExperimentTarget) {
        val span = client.newSpan(
            name = experiment.name,
            kind = SpanKind.AGENT_RUN,
            parent = null,
            remote = null,
            isEvent = false,
            input = item.input,
            attributes = mapOf(DATASET_ITEM_ATTRIBUTE to item.itemId),
            metadata = null,
            tags = null,
            model = null,
            provider = null,
            entityKind = null,
            entityId = null,
            entityName = null,
            experimentId = experiment.experimentId,
            // A sampled-out case would be a case with no answer.
            alwaysSampled = true,
        )
        try {
            val output = withContext(McpdContext(span)) { target(item.input) }
            // A target whose last line returns nothing hands back Unit, which is no answer.
            if (output != null && output != Unit) span.setOutput(output)
            span.end()
        } catch (e: Throwable) {
            // A cancelled runner stops here. A target's own timeout is also a
            // CancellationException, but the runner is still active, so that
            // is a failed case like any other.
            if (e is CancellationException && !currentCoroutineContext().isActive) {
                span.end(SpanStatus.CANCELLED)
                throw e
            }
            span.setError(e)
            span.end(SpanStatus.ERROR)
        }
    }

    /** Mark the experiment failed. A failure here is reported and never hides the original error. */
    private suspend fun recordFailure(experiment: Experiment, cause: Throwable) {
        try {
            client.createExperiment(
                experiment.copy(
                    status = ExperimentStatus.FAILED,
                    finishedAt = Clock.System.now(),
                    error = cause.message ?: cause.toString(),
                )
            )
        } catch (e: Exception) {
            client.diagnostics.warn("experiment-failed-status") {
                "runExperiment could not record that experiment ${experiment.experimentId} failed: $e"
            }
        }
    }
}
