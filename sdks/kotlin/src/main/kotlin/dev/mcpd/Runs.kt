package dev.mcpd

import kotlinx.coroutines.delay
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlin.time.Duration
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.nanoseconds

/*
 * The run surface of the daemon, as a client: definitions, runs, events, and
 * the worker protocol [Worker] drives.
 *
 * These calls throw on failure, like the read API, because a run you asked
 * for and did not get is news. A worker call refused with a 409 throws
 * [StepConflictException] carrying the daemon's closed reason, which is what
 * lets a worker tell a fence from its own landed call.
 */

/** A worker call the daemon refused with one of the four closed reasons. */
class StepConflictException internal constructor(
    /** The daemon's answer in full. */
    val conflict: StepConflict,
) : McpdException("${conflict.reason.wire}: ${conflict.message}")

/** A run that ended failed, timed out or cancelled, thrown by [waitRun] with the run attached. */
class RunFailedException internal constructor(
    /** The run as it ended. */
    val run: Run,
) : McpdException("run ${run.runId} ${run.status.wire}" + (run.error?.let { ": $it" } ?: "")) {
    /** `FAILED`, `TIMED_OUT` or `CANCELLED`. */
    val runStatus: RunStatus get() = run.status
}

/** What [waitRun] answers: the run as it ended, with its output to hand. */
data class RunResult(
    /** Always `SUCCEEDED`; any other ending throws. */
    val status: RunStatus,
    /** What the definition's output template rendered. */
    val output: JsonElement?,
    /** The priced usage the steps reported, in USD. */
    val costUsd: Double,
    /** How many attempts the run spent. */
    val stepsUsed: Int,
    /** The trace every step of the run landed in. */
    val traceId: String,
    /** The run in full. */
    val run: Run,
)

private fun projectParam(project: String): List<Pair<String, String?>> = listOf("project" to project)

/** Store one version of a definition, and read the version back. */
suspend fun Mcpd.putDefinition(doc: DefinitionDocument, project: String = config.project): DefinitionVersion =
    transport.postJson(
        "/v1/api/definitions", projectParam(project),
        json.encodeToString(DefinitionDocument.serializer(), doc), DefinitionVersion.serializer(),
    )

/** One definition: the latest version, unless [version] names one. */
suspend fun Mcpd.getDefinition(id: String, version: Int? = null, project: String = config.project): Definition =
    transport.getJson(
        "/v1/api/definitions/$id",
        listOf("project" to project, "version" to version?.toString()),
        Definition.serializer(),
    )

/** Every stored version in the project, or every version of one id. */
suspend fun Mcpd.listDefinitions(id: String? = null, project: String = config.project): List<Definition> =
    transport.getJson("/v1/api/definitions", listOf("project" to project, "id" to id), ListSerializer(Definition.serializer()))

/**
 * Start a run. The response is the run as stored, with its trace id.
 *
 * [idempotencyKey] is unique per project while held: a retry of this call
 * returns the same run.
 */
suspend fun Mcpd.startRun(
    definition: String,
    input: Any? = null,
    version: Int? = null,
    correlation: Correlation? = null,
    idempotencyKey: String? = null,
    project: String = config.project,
): Run {
    val body = StartRunRequest(
        definitionId = definition,
        project = project,
        version = version,
        input = input?.let { toJsonElement(it) },
        correlation = correlation?.let { if (it.project.isEmpty()) it.copy(project = project) else it },
        idempotencyKey = idempotencyKey,
    )
    return transport.postJson("/v1/api/runs", emptyList(), json.encodeToString(StartRunRequest.serializer(), body), Run.serializer())
}

/** One run and everything durable about it: its journal, deadlines and waits. */
suspend fun Mcpd.getRun(runId: String): RunDetail =
    transport.getJson("/v1/api/runs/$runId", emptyList(), RunDetail.serializer())

/** Runs in the project, most recently updated first. */
suspend fun Mcpd.listRuns(
    status: RunStatus? = null,
    definitionId: String? = null,
    limit: Int? = null,
    cursor: String? = null,
    project: String = config.project,
): RunListResponse = transport.getJson(
    "/v1/api/runs",
    listOf(
        "project" to project, "status" to status?.wire, "definition_id" to definitionId,
        "limit" to limit?.toString(), "cursor" to cursor,
    ),
    RunListResponse.serializer(),
)

/** End a run and every child run under it. Cancelling an ended run changes nothing. */
suspend fun Mcpd.cancelRun(runId: String): Run =
    transport.postJson("/v1/api/runs/$runId/cancel", emptyList(), "{}", Run.serializer())

/** A page of the run's journal, from [after] exclusive. Pass -1 to start at the beginning. */
suspend fun Mcpd.runEvents(runId: String, after: Long = -1, limit: Int? = null): RunEventPage =
    transport.getJson(
        "/v1/api/runs/$runId/events",
        listOf("after" to after.toString(), "limit" to limit?.toString()),
        RunEventPage.serializer(),
    )

/**
 * Hand every event of a run to [block] as it happens, from [after] exclusive,
 * until the run ends. A client that stops and starts again passes the last
 * `seq` it saw and misses nothing that was committed.
 */
suspend fun Mcpd.watchRun(
    runId: String,
    after: Long = -1,
    poll: Duration = 500.milliseconds,
    block: suspend (RunEvent) -> Unit,
) {
    var last = after
    while (true) {
        val page = runEvents(runId, last, 500)
        var ended = false
        for (event in page.events) {
            if (event.seq > last) last = event.seq
            block(event)
            if (event.kind == RunEventKind.RUN_ENDED) ended = true
        }
        if (ended) return
        if (page.hasMore) continue
        delay(poll)
    }
}

/**
 * Wait for a run to end and return it with its output. A run that ended
 * failed, timed out or cancelled throws [RunFailedException] carrying the run;
 * one still going after [timeout] throws [McpdException].
 */
suspend fun Mcpd.waitRun(
    runId: String,
    after: Long = -1,
    timeout: Duration? = null,
    poll: Duration = 500.milliseconds,
): RunResult {
    val deadline = timeout?.let { System.nanoTime() + it.inWholeNanoseconds }
    var last = after
    while (true) {
        val page = runEvents(runId, last, 500)
        var ended = false
        for (event in page.events) {
            if (event.seq > last) last = event.seq
            if (event.kind == RunEventKind.RUN_ENDED) ended = true
        }
        if (ended) break
        if (page.hasMore) continue
        if (deadline != null && System.nanoTime() >= deadline) {
            throw McpdException("run $runId is still running after $timeout")
        }
        delay(if (deadline == null) poll else minOf(poll, (deadline - System.nanoTime()).coerceAtLeast(0).nanoseconds))
    }
    val run = getRun(runId).run
    if (run.status != RunStatus.SUCCEEDED) throw RunFailedException(run)
    return RunResult(run.status, run.output, run.costUsd, run.stepsUsed, run.traceId, run)
}

/** [startRun], then [waitRun]. */
suspend fun Mcpd.startRunAndWait(
    definition: String,
    input: Any? = null,
    version: Int? = null,
    correlation: Correlation? = null,
    idempotencyKey: String? = null,
    project: String = config.project,
    timeout: Duration? = null,
    poll: Duration = 500.milliseconds,
): RunResult {
    val run = startRun(definition, input, version, correlation, idempotencyKey, project)
    return waitRun(run.runId, timeout = timeout, poll = poll)
}

/** Deliver an event to one run through its own open wait. */
suspend fun Mcpd.signalRun(runId: String, event: String, payload: Any? = null, eventId: String? = null): EventReceipt {
    val body = JsonObject(
        buildMap {
            put("event", JsonPrimitive(event))
            payload?.let { put("payload", toJsonElement(it)) }
            eventId?.let { put("event_id", JsonPrimitive(it)) }
        },
    )
    return transport.postJson("/v1/api/runs/$runId/signal", emptyList(), body.toString(), EventReceipt.serializer())
}

/**
 * Route an event by name and key to the run waiting for it. Send your own
 * [eventId] from any handler that can be retried, such as a webhook: a repeat
 * of a supplied id is answered `duplicate` and writes nothing.
 */
suspend fun Mcpd.publishEvent(
    event: String,
    key: String,
    payload: Any? = null,
    eventId: String? = null,
    project: String = config.project,
): EventReceipt {
    val body = PublishEventRequest(event = event, key = key, project = project, payload = payload?.let { toJsonElement(it) }, eventId = eventId)
    return transport.postJson("/v1/api/events", emptyList(), json.encodeToString(PublishEventRequest.serializer(), body), EventReceipt.serializer())
}

/** Announce a worker and store what it serves. */
suspend fun Mcpd.registerWorker(workerId: String, definitions: List<DefinitionDocument>, project: String = config.project): WorkerRegistered {
    val body = WorkerRegistration(workerId = workerId, definitions = definitions, project = project)
    return transport.postJson("/v1/api/workers/register", emptyList(), json.encodeToString(WorkerRegistration.serializer(), body), WorkerRegistered.serializer())
}

/** Offer up to [max] runnable steps, waiting up to [wait] for one. */
suspend fun Mcpd.claimSteps(workerId: String, max: Int, wait: Duration, runId: String? = null, project: String = config.project): ClaimResponse {
    val body = ClaimRequest(workerId = workerId, max = max, waitMs = wait.inWholeMilliseconds.toInt(), project = project, runId = runId)
    return transport.postJson("/v1/api/steps/claim", emptyList(), json.encodeToString(ClaimRequest.serializer(), body), ClaimResponse.serializer(), wait = wait)
}

/** Declare that the body is running. Required under `at_most_once`. */
suspend fun Mcpd.stepStarted(leaseId: String): LeaseState =
    transport.postJson("/v1/api/steps/$leaseId/started", emptyList(), "{}", LeaseState.serializer())

/** Extend the lease. The only evidence the daemon has that a worker is alive. */
suspend fun Mcpd.stepHeartbeat(leaseId: String): LeaseState =
    transport.postJson("/v1/api/steps/$leaseId/heartbeat", emptyList(), "{}", LeaseState.serializer())

/** Finish the step. */
suspend fun Mcpd.completeStep(leaseId: String, result: StepResult): StepAck =
    transport.postJson("/v1/api/steps/$leaseId/complete", emptyList(), json.encodeToString(StepResult.serializer(), result), StepAck.serializer())

/** Fail the step, with the class that decides whether it is retried. */
suspend fun Mcpd.failStep(leaseId: String, error: StepError): StepAck =
    transport.postJson("/v1/api/steps/$leaseId/fail", emptyList(), json.encodeToString(StepFailure.serializer(), StepFailure(error)), StepAck.serializer())

