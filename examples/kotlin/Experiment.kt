// An experiment in Kotlin: run every case of a dataset through your own code,
// score each answer against its case, and read the results.
//
// The regions are read into the documentation at build time.

// Its own package, so its `main` sits beside the quickstart's and the workflow's.
package examples.experiment

import dev.mcpd.Mcpd
import dev.mcpd.McpdConfig
import dev.mcpd.SpanKind
import dev.mcpd.SpanStatus
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

val mcpd = Mcpd(McpdConfig(project = "support"))

// The code under test. Anything it records nests under the case's run.
suspend fun answer(input: JsonElement): String {
    val question = input.jsonObject.getValue("question").jsonPrimitive.content
    return mcpd.span("model_call", SpanKind.MODEL_GENERATION) { span ->
        span.setModel("claude-sonnet-4", "anthropic")
        if ("refund" in question) "Refunds take five days." else "It ships tomorrow."
    }
}

fun main() = runBlocking {
    // #region experiment
    // Each case runs in its own root span, four at a time. A case that throws is
    // recorded as a failed run, and the others still run.
    val results = mcpd.runExperiment(
        "support-questions",
        name = "answers-v2",
        scorers = listOf("exact_match", "similarity"),
        metadata = mapOf("prompt" to "answer@4"),
        target = ::answer,
    )

    for (scorer in results.scorers) {
        println("${scorer.scorerId}: mean ${"%.2f".format(scorer.mean)} over ${scorer.n}")
    }
    for (item in results.items) {
        if (item.status == SpanStatus.ERROR) println("case ${item.itemId} failed: ${item.traceId}")
    }

    // Read one run back as the steps its agent took.
    results.items.firstOrNull()?.let { first ->
        val trajectory = mcpd.getTrajectory(first.traceId)
        val tools = trajectory.toolSequence.joinToString(" > ")
        println("${trajectory.stepCount} steps, tools: $tools")
    }
    // #endregion experiment

    mcpd.shutdown()
}
