package dev.mcpd

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineName
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import kotlin.time.Clock
import kotlin.time.Instant
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import java.util.UUID
import java.util.concurrent.ThreadLocalRandom
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import kotlin.coroutines.coroutineContext
import kotlin.time.Duration
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds

/** Sent on every request, so a server operator can tell which client produced a batch. */
internal const val USER_AGENT = "mcpd-kotlin/0.1.0"

/**
 * How often the SDK is willing to complain about the same problem.
 *
 * A variable rather than a constant so a test can shorten it and watch the
 * second warning arrive; nothing outside this module should set it.
 */
internal var diagnosticInterval: Duration = 30.seconds

/**
 * Where configuration defaults read the environment from.
 *
 * `System.getenv` cannot be changed from inside a JVM, so a test that wants to
 * prove `MCPD_BATCH_SIZE` is read has to be able to answer the lookup itself.
 * Nothing outside this module should set it.
 */
internal object Environment {
    @Volatile
    var lookup: (String) -> String? = { System.getenv(it) }
}

/**
 * A read failed.
 *
 * Only the query methods throw this. Instrumentation never does.
 */
open class McpdException internal constructor(
    message: String,
    cause: Throwable? = null,
    /** The HTTP status mcpd answered with, or null when no answer arrived. */
    val status: Int? = null,
) : RuntimeException(message, cause)

private fun env(name: String): String? = try {
    Environment.lookup(name)?.trim()?.takeIf { it.isNotEmpty() }
} catch (e: SecurityException) {
    // A sandbox that forbids reading the environment is not a reason to fail
    // construction; the in-code defaults are still perfectly usable.
    null
}

private fun envDouble(name: String): Double? = env(name)?.toDoubleOrNull()

/**
 * A whole positive number, or nothing.
 *
 * The four tuning variables carry their unit in their name, so a value is a
 * plain integer and never a duration string. Zero and below are ignored along
 * with anything unparseable: a batch of zero signals or a queue of zero
 * entries would stop the exporter, and a typo in a deployment variable must
 * not.
 */
private fun envInt(name: String): Int? = env(name)?.toIntOrNull()?.takeIf { it > 0 }

/** A duration given in whole milliseconds, which is how every SDK reads these two names. */
private fun envMillis(name: String): Duration? = envInt(name)?.milliseconds

private fun envBool(name: String): Boolean? = when (env(name)?.lowercase()) {
    "1", "true", "yes", "on" -> true
    "0", "false", "no", "off" -> false
    else -> null
}

/**
 * How a client is configured.
 *
 * Every field reads an environment variable as its default and is settable in
 * code, so code always wins over the environment. A configuration built with no
 * arguments works against a local mcpd.
 */
data class McpdConfig(
    /** Where mcpd is listening. */
    val endpoint: String = env("MCPD_ENDPOINT") ?: "http://localhost:4317",
    /** Sent as `Authorization: Bearer`. Absent when mcpd runs without keys. */
    val apiKey: String? = env("MCPD_API_KEY"),
    /** The tenancy key every signal belongs to. */
    val project: String = env("MCPD_PROJECT") ?: "default",
    /** Deployment environment, such as `production`. */
    val environment: String? = env("MCPD_ENVIRONMENT"),
    /** The emitting service. */
    val serviceName: String? = env("MCPD_SERVICE_NAME"),
    /** The emitting service's version, so a regression can be pinned to a release. */
    val serviceVersion: String? = env("MCPD_SERVICE_VERSION"),
    /** Fraction of traces to keep, 0.0 to 1.0. Decided once, at the root. */
    val sample: Double = envDouble("MCPD_SAMPLE") ?: 1.0,
    /** How many signals go in one request. `MCPD_BATCH_SIZE`. */
    val batchSize: Int = envInt("MCPD_BATCH_SIZE") ?: 128,
    /** How long the exporter waits before sending a partial batch. `MCPD_FLUSH_INTERVAL_MS`. */
    val flushInterval: Duration = envMillis("MCPD_FLUSH_INTERVAL_MS") ?: 2.seconds,
    /** How many signals may wait in memory before the oldest are dropped. `MCPD_MAX_QUEUE`. */
    val maxQueue: Int = envInt("MCPD_MAX_QUEUE") ?: 8192,
    /** Per-request timeout. `MCPD_TIMEOUT_MS`. */
    val timeout: Duration = envMillis("MCPD_TIMEOUT_MS") ?: 10.seconds,
    /**
     * When false every recording call is a no-op that still returns valid
     * objects, so mcpd can be turned off in tests without touching the code
     * under test. Reads still go to the server: a query is something the caller
     * is waiting on, and an empty answer would be a lie.
     */
    val enabled: Boolean = envBool("MCPD_ENABLED") ?: true,
    /**
     * Whether `input` and `output` may leave the process. When false a span is
     * still recorded with its timing, status, usage, model, correlation and
     * errors, and the payloads are dropped before they reach the queue, so
     * nothing sensitive is queued and nothing sensitive can leave on a flush.
     * A span can override it for itself. `MCPD_CAPTURE_PAYLOADS`.
     */
    val capturePayloads: Boolean = envBool("MCPD_CAPTURE_PAYLOADS") ?: true,
    /**
     * The lowest level [Mcpd.log] records. A line below it is dropped at the
     * call, before the record is built. The default is `debug`, which keeps
     * every level the spec defines except `trace`. A name outside the enum is
     * ignored rather than refused, because a typo in a deployment variable must
     * not silence a service's logs. `MCPD_LOG_LEVEL`.
     */
    val logLevel: LogLevel = parseLogLevel(env("MCPD_LOG_LEVEL")) ?: LogLevel.DEBUG,
    /**
     * Where drained batches go. The HTTP transport when absent. A
     * [RecordingSink] keeps them in memory for a test; a [ConsoleSink] prints
     * them. Chosen in code only: there is no environment variable for it.
     */
    val sink: Sink? = null,
    /**
     * Applied to every span's `input`, `output`, `attributes` and `metadata`,
     * and to every log line's `fields`, before the record is queued. Return the
     * value to keep, or null to drop it. A redactor that throws, or returns
     * something other than an object for `attributes`, `metadata` or `fields`,
     * drops that value and counts it in [Stats.redactionFailures]: a broken redactor never
     * lets the original through (contract section 2.1.1). It runs after
     * [capturePayloads], so a payload that capture stripped is never handed to it.
     * Chosen in code only: no environment variable can carry a function.
     */
    val redact: ((JsonElement) -> JsonElement?)? = null,
)

/** What the client has done since it was built. */
data class Stats(
    /** Signals waiting to be sent. */
    val queued: Int,
    /** Signals evicted because the queue was full. */
    val dropped: Long,
    /** How many signals the queue can hold. */
    val capacity: Int,
    /** Signals the sink accepted. */
    val exported: Long,
    /** Batches the sink refused. */
    val failed: Long,
    /** Spans [Mcpd.record] refused. */
    val rejected: Long,
    /** Values [McpdConfig.redact] threw on, dropped rather than sent. */
    val redactionFailures: Long,
)

/**
 * The mcpd client.
 *
 * Recording a signal appends to a bounded in-memory queue and returns; a
 * background worker batches, compresses and sends. An agent's latency therefore
 * never depends on mcpd being fast, or up, or reachable at all.
 *
 * One instance per process is the intended shape. Call [shutdown] before the
 * process exits, or the last batch never leaves.
 */
class Mcpd(
    /** How this client is configured. */
    val config: McpdConfig = McpdConfig(),
) : AutoCloseable {

    companion object {
        /** How long [flush] and [shutdown] wait before they return anyway (contract section 2.4). */
        val DEFAULT_DEADLINE: Duration = 10.seconds
    }

    // Declaration order matters: the worker at the bottom starts immediately and
    // reads everything above it.
    @OptIn(kotlinx.serialization.ExperimentalSerializationApi::class)
    internal val json: Json = Json {
        // Omitting nulls roughly halves a span payload, and mcpd reads an absent
        // field and an explicit null the same way. `explicitNulls` is still
        // marked experimental upstream; the opt-in is deliberate and the
        // behaviour it controls has been stable for several releases.
        explicitNulls = false
        // A score without its scale is uninterpretable, so defaults are written
        // even when they match.
        encodeDefaults = true
        // A newer server may answer with fields this build has never heard of.
        // Refusing to read the rest of the response over that helps nobody.
        ignoreUnknownKeys = true
    }

    internal val diagnostics = Diagnostics(diagnosticInterval)
    private val queue = SignalQueue(config.maxQueue.coerceAtLeast(1))
    internal val transport = Transport(config, json, diagnostics)
    private val prompts = PromptStore(this)
    private val sink: Sink = config.sink ?: transport
    private val exportLock = Mutex()

    // Set when a shutdown's deadline passed: no further batch starts.
    private val abandoned = AtomicBoolean(false)

    // The batch being sent, so shutdown can let it finish before the scope goes.
    @Volatile private var inflight: Job? = null
    private val closed = AtomicBoolean(false)
    private val exportedCount = AtomicLong()
    private val failedCount = AtomicLong()
    private val rejectedCount = AtomicLong()
    private val redactionFailureCount = AtomicLong()

    /** Nudges the worker when a full batch is ready, rather than waiting out the interval. */
    private val wake = Channel<Unit>(Channel.CONFLATED)

    private val baseCorrelation = Correlation(
        project = config.project,
        environment = config.environment,
        serviceName = config.serviceName,
        serviceVersion = config.serviceVersion,
    )

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO + CoroutineName("mcpd-exporter"))

    private val worker: Job? = if (config.enabled) scope.launch { runWorker() } else null

    /** How many signals are waiting to be sent. */
    val queueDepth: Int get() = queue.size

    /** How many signals have been dropped because the queue was full. */
    val droppedSignals: Long get() = queue.dropped

    /** Queue depth, drops, capacity, what the sink took and refused, and what [record] refused. */
    fun stats(): Stats = Stats(
        queued = queue.size,
        dropped = queue.dropped,
        capacity = config.maxQueue.coerceAtLeast(1),
        exported = exportedCount.get(),
        failed = failedCount.get(),
        rejected = rejectedCount.get(),
        redactionFailures = redactionFailureCount.get(),
    )

    // ------------------------------------------------------------- tracing ---

    /**
     * Start a root span: one agent run, one workflow, one request.
     *
     * Pass `traceparent` to continue a trace that began in another service.
     * The returned span must be ended by the caller; prefer [span], which does
     * it for you.
     */
    fun trace(
        name: String,
        kind: SpanKind = SpanKind.AGENT_RUN,
        input: Any? = null,
        tags: List<String>? = null,
        attributes: Map<String, Any?>? = null,
        metadata: Map<String, Any?>? = null,
        model: String? = null,
        provider: String? = null,
        entityKind: EntityKind? = null,
        entityId: String? = null,
        entityName: String? = null,
        sessionId: String? = null,
        threadId: String? = null,
        userId: String? = null,
        runId: String? = null,
        requestId: String? = null,
        traceparent: String? = null,
        externalParentSpanId: String? = null,
        capturePayloads: Boolean? = null,
    ): ActiveSpan = newSpan(
        name = name,
        kind = kind,
        parent = null,
        remote = TraceParent.parse(traceparent),
        isEvent = false,
        input = input,
        attributes = attributes,
        metadata = metadata,
        tags = tags,
        model = model,
        provider = provider,
        entityKind = entityKind,
        entityId = entityId,
        entityName = entityName,
        sessionId = sessionId,
        threadId = threadId,
        userId = userId,
        runId = runId,
        requestId = requestId,
        externalParentSpanId = externalParentSpanId,
        capturePayloads = capturePayloads,
    )

    /**
     * Run [block] inside a span that ends itself.
     *
     * The span becomes the active span for the duration of the block, so
     * anything the block calls can start children without being handed the
     * parent. A block that throws leaves an `error` span carrying the exception
     * and rethrows; a cancelled block leaves a `cancelled` one.
     *
     * With no [kind] the span is an `agent_run` at the root of a trace and
     * `generic` inside one, which is what [trace] and [ActiveSpan.child] do.
     */
    suspend fun <T> span(
        name: String,
        kind: SpanKind? = null,
        input: Any? = null,
        tags: List<String>? = null,
        attributes: Map<String, Any?>? = null,
        metadata: Map<String, Any?>? = null,
        model: String? = null,
        provider: String? = null,
        entityKind: EntityKind? = null,
        entityId: String? = null,
        entityName: String? = null,
        sessionId: String? = null,
        threadId: String? = null,
        userId: String? = null,
        traceparent: String? = null,
        externalParentSpanId: String? = null,
        capturePayloads: Boolean? = null,
        block: suspend (ActiveSpan) -> T,
    ): T {
        val parent = currentSpan()
        val span = newSpan(
            name = name,
            kind = kind ?: if (parent == null) SpanKind.AGENT_RUN else SpanKind.GENERIC,
            parent = parent,
            remote = TraceParent.parse(traceparent),
            isEvent = false,
            input = input,
            attributes = attributes,
            metadata = metadata,
            tags = tags,
            model = model,
            provider = provider,
            entityKind = entityKind,
            entityId = entityId,
            entityName = entityName,
            sessionId = sessionId,
            threadId = threadId,
            userId = userId,
            runId = null,
            requestId = null,
            externalParentSpanId = externalParentSpanId,
            capturePayloads = capturePayloads,
        )
        return withContext(McpdContext(span)) {
            try {
                val result = block(span)
                span.end()
                result
            } catch (e: CancellationException) {
                // Abandoned deliberately, by a caller or a timeout. That is not
                // a failure of the operation and must not read as one.
                span.end(SpanStatus.CANCELLED)
                throw e
            } catch (e: Throwable) {
                span.setError(e)
                span.end(SpanStatus.ERROR)
                throw e
            }
        }
    }

    /**
     * Queue a span that already ended, as given.
     *
     * For an adapter over another tracing system, a batch import or a replay:
     * the caller holds the ids, the parent, the start and the end, and the
     * record is queued with all of them untouched. It is never re-parented,
     * never re-timed and never re-sampled. Correlation fields the record
     * leaves empty are filled from this client's; fields it sets are kept.
     *
     * A record with an id that is not OpenTelemetry-shaped, a blank name or
     * an all-zero id is refused: dropped, counted in [stats] as `rejected` and
     * reported once per interval. Refusing never throws. With
     * `capturePayloads = false` the payloads are stripped like any other
     * span's, and a disabled client drops the record like any other signal.
     */
    fun record(span: Span) {
        guard("record") {
            if (!config.enabled || closed.get()) return@guard
            val problem = when {
                !Ids.isValidTraceId(span.traceId) -> "trace_id is not 32 lowercase hex, or is all zero"
                !Ids.isValidSpanId(span.spanId) -> "span_id is not 16 lowercase hex, or is all zero"
                span.parentSpanId != null && !Ids.isValidSpanId(span.parentSpanId) ->
                    "parent_span_id is not 16 lowercase hex, or is all zero"
                span.name.isBlank() -> "name is empty"
                else -> null
            }
            if (problem != null) {
                rejectedCount.incrementAndGet()
                diagnostics.warn("record") { "a recorded span was refused and dropped: $problem" }
                return@guard
            }
            val given = span.correlation
            val correlation = given.copy(
                project = given.project.ifBlank { baseCorrelation.project },
                environment = given.environment ?: baseCorrelation.environment,
                serviceName = given.serviceName ?: baseCorrelation.serviceName,
                serviceVersion = given.serviceVersion ?: baseCorrelation.serviceVersion,
            )
            val record = if (config.capturePayloads) {
                span.copy(correlation = correlation)
            } else {
                span.copy(correlation = correlation, input = null, output = null)
            }
            enqueue(Signal.SpanSlot(redacted(record)))
        }
    }

    /** The span the calling coroutine is inside, if it belongs to this client. */
    suspend fun currentSpan(): ActiveSpan? =
        coroutineContext[McpdContext]?.span?.takeIf { it.client === this }

    /** The `traceparent` header for the active span, to send with an outgoing request. */
    suspend fun traceparent(): String? = currentSpan()?.traceparent()

    // ------------------------------------------------------ other signals ---

    /** A log line, correlated to the active span. */
    suspend fun log(level: LogLevel, message: String, fields: Map<String, Any?>? = null) {
        recordLog(level, message, fields, currentSpan())
    }

    /** A numeric observation, with the active span as its exemplar. */
    suspend fun metric(
        name: String,
        value: Double,
        kind: MetricKind = MetricKind.GAUGE,
        labels: Map<String, Any?>? = null,
        unit: String? = null,
    ) {
        recordMetric(name, value, kind, labels, unit, currentSpan())
    }

    /**
     * Attach a machine evaluation to a run, or to one step of it.
     *
     * [min] and [max] are required because a bare number with no declared scale
     * cannot be compared to anything: 7 is excellent out of 10 and poor out of
     * 100.
     */
    fun score(
        traceId: String,
        scorerId: String,
        value: Double,
        min: Double,
        max: Double,
        spanId: String? = null,
        reason: String? = null,
        label: String? = null,
        source: ScoreSource = ScoreSource.LIVE,
        scorerVersion: String? = null,
        metadata: Map<String, Any?>? = null,
    ) {
        recordScore(
            traceId = traceId,
            spanId = spanId,
            scorerId = scorerId,
            value = value,
            min = min,
            max = max,
            reason = reason,
            label = label,
            source = source,
            scorerVersion = scorerVersion,
            metadata = metadata,
            correlation = baseCorrelation,
        )
    }

    /** Attach a person's judgement to a run, or to one step of it. */
    fun feedback(
        traceId: String,
        kind: FeedbackKind,
        value: Any?,
        spanId: String? = null,
        comment: String? = null,
        authorId: String? = null,
        authorKind: String? = null,
        metadata: Map<String, Any?>? = null,
    ) {
        recordFeedback(
            traceId = traceId,
            spanId = spanId,
            kind = kind,
            value = value,
            comment = comment,
            authorId = authorId,
            authorKind = authorKind,
            metadata = metadata,
            correlation = baseCorrelation,
        )
    }

    // ------------------------------------------------------------- reads ----

    /**
     * A page of runs. Every filter is optional except the project.
     *
     * [experimentId] keeps the runs an experiment recorded. [preview] fills
     * each run's `input_preview` and `output_preview`.
     */
    suspend fun listTraces(
        q: String? = null,
        status: SpanStatus? = null,
        kind: SpanKind? = null,
        name: String? = null,
        tags: List<String>? = null,
        sessionId: String? = null,
        threadId: String? = null,
        userId: String? = null,
        environment: String? = null,
        model: String? = null,
        from: Instant? = null,
        to: Instant? = null,
        minDurationMs: Long? = null,
        maxDurationMs: Long? = null,
        minCost: Double? = null,
        maxCost: Double? = null,
        minTokens: Long? = null,
        hasScore: String? = null,
        scoreBelow: Double? = null,
        scoreAbove: Double? = null,
        hasFeedback: Boolean? = null,
        experimentId: String? = null,
        preview: Boolean? = null,
        sort: String? = null,
        order: String? = null,
        limit: Int? = null,
        cursor: String? = null,
        project: String = config.project,
    ): TraceListResponse = transport.getJson(
        "/v1/api/traces",
        query(
            "project" to project, "q" to q, "status" to status, "kind" to kind, "name" to name,
            "tags" to tags, "session_id" to sessionId, "thread_id" to threadId, "user_id" to userId,
            "environment" to environment, "model" to model, "from" to from, "to" to to,
            "min_duration_ms" to minDurationMs, "max_duration_ms" to maxDurationMs,
            "min_cost" to minCost, "max_cost" to maxCost, "min_tokens" to minTokens,
            "has_score" to hasScore, "score_below" to scoreBelow, "score_above" to scoreAbove,
            "has_feedback" to hasFeedback, "experiment_id" to experimentId, "preview" to preview,
            "sort" to sort, "order" to order, "limit" to limit, "cursor" to cursor,
        ),
        TraceListResponse.serializer(),
    )

    /**
     * One run and every span in it, ordered so a waterfall renders without a
     * second request.
     *
     * `include` asks for the attached signals: `logs`, `scores`, `feedback`.
     */
    suspend fun getTrace(
        traceId: String,
        include: List<String>? = null,
        project: String = config.project,
    ): TraceDetail = transport.getJson(
        "/v1/api/traces/$traceId",
        // A trace id is only unique within its project, which is the tenancy
        // key. Without it the server reads its own default project and answers
        // 404 for a trace that is stored and that listTraces returns. The other
        // four SDKs all send it; this one did not, and the fake server the unit
        // tests use answers 200 to any path, so nothing caught it until the
        // live suite ran against a real daemon.
        query("project" to project, "include" to include),
        TraceDetail.serializer(),
    )

    /**
     * Search individual steps across every run.
     *
     * [prompt] and [promptVersion] find the spans [ActiveSpan.setPrompt] linked
     * to a prompt, or to one version of it.
     */
    suspend fun listSpans(
        q: String? = null,
        kind: SpanKind? = null,
        status: SpanStatus? = null,
        name: String? = null,
        model: String? = null,
        traceId: String? = null,
        prompt: String? = null,
        promptVersion: Int? = null,
        from: Instant? = null,
        to: Instant? = null,
        minDurationMs: Long? = null,
        sort: String? = null,
        order: String? = null,
        limit: Int? = null,
        cursor: String? = null,
        project: String = config.project,
    ): SpanListResponse = transport.getJson(
        "/v1/api/spans",
        query(
            "project" to project, "q" to q, "kind" to kind, "status" to status, "name" to name,
            "model" to model, "trace_id" to traceId, "prompt" to prompt,
            "prompt_version" to promptVersion, "from" to from, "to" to to,
            "min_duration_ms" to minDurationMs, "sort" to sort, "order" to order,
            "limit" to limit, "cursor" to cursor,
        ),
        SpanListResponse.serializer(),
    )

    /** Search logs. `level` is a minimum severity, inclusive. */
    suspend fun listLogs(
        q: String? = null,
        level: LogLevel? = null,
        traceId: String? = null,
        spanId: String? = null,
        from: Instant? = null,
        to: Instant? = null,
        limit: Int? = null,
        cursor: String? = null,
        project: String = config.project,
    ): LogListResponse = transport.getJson(
        "/v1/api/logs",
        query(
            "project" to project, "q" to q, "level" to level, "trace_id" to traceId,
            "span_id" to spanId, "from" to from, "to" to to, "limit" to limit, "cursor" to cursor,
        ),
        LogListResponse.serializer(),
    )

    /**
     * A metric aggregated into time buckets, one series per label combination.
     *
     * [filter] keeps points whose labels have these values. It is sent as
     * `key:value` pairs joined by commas, so a key cannot contain `:` or `,`
     * and a value cannot contain `,`; either throws `IllegalArgumentException`
     * before a request is made.
     */
    suspend fun queryMetrics(
        name: String,
        from: Instant,
        to: Instant,
        bucket: String? = null,
        agg: String? = null,
        groupBy: List<String>? = null,
        project: String = config.project,
        filter: Map<String, String>? = null,
    ): MetricSeriesResponse = transport.getJson(
        "/v1/api/metrics",
        query(
            "project" to project, "name" to name, "from" to from, "to" to to,
            "bucket" to bucket, "agg" to agg, "group_by" to groupBy,
            "filter" to metricFilter(filter),
        ),
        MetricSeriesResponse.serializer(),
    )

    /** Machine evaluations, filterable by scorer, source, value range and time. */
    suspend fun listScores(
        scorerId: String? = null,
        source: ScoreSource? = null,
        traceId: String? = null,
        minValue: Double? = null,
        maxValue: Double? = null,
        from: Instant? = null,
        to: Instant? = null,
        limit: Int? = null,
        cursor: String? = null,
        project: String = config.project,
    ): ScoreListResponse = transport.getJson(
        "/v1/api/scores",
        query(
            "project" to project, "scorer_id" to scorerId, "source" to source,
            "trace_id" to traceId, "min_value" to minValue, "max_value" to maxValue,
            "from" to from, "to" to to, "limit" to limit, "cursor" to cursor,
        ),
        ScoreListResponse.serializer(),
    )

    /**
     * The human judgements attached to one run.
     *
     * The daemon reads feedback one run at a time and refuses any other filter
     * with a 400, so [traceId] is required and nothing else is sent.
     */
    suspend fun listFeedback(
        traceId: String,
        limit: Int? = null,
        project: String = config.project,
    ): FeedbackListResponse {
        require(traceId.isNotEmpty()) {
            "listFeedback needs a traceId: the daemon reads feedback one run at a time"
        }
        return transport.getJson(
            "/v1/api/feedback",
            query("project" to project, "trace_id" to traceId, "limit" to limit),
            FeedbackListResponse.serializer(),
        )
    }

    /** Run count, error rate, latency, tokens and cost for a window. */
    suspend fun analyticsSummary(
        from: Instant,
        to: Instant,
        groupBy: String? = null,
        project: String = config.project,
    ): AnalyticsSummary = transport.getJson(
        "/v1/api/analytics/summary",
        query("project" to project, "from" to from, "to" to to, "group_by" to groupBy),
        AnalyticsSummary.serializer(),
    )

    /** Where the money went, split by model, provider, agent, environment or user. */
    suspend fun analyticsCost(
        from: Instant,
        to: Instant,
        groupBy: String? = null,
        bucket: String? = null,
        project: String = config.project,
    ): CostBreakdown = transport.getJson(
        "/v1/api/analytics/cost",
        query("project" to project, "from" to from, "to" to to, "group_by" to groupBy, "bucket" to bucket),
        CostBreakdown.serializer(),
    )

    /** Whether the server is alive, and which spec version it implements. */
    suspend fun health(): HealthStatus =
        transport.getJson("/healthz", emptyList(), HealthStatus.serializer())

    // ----------------------------------------------------------- prompts ----

    /**
     * One version of a prompt, from the daemon or from this client's cache.
     *
     * Give [label] or [version], not both; with neither, the version labelled
     * `production` is read, or the latest when no version carries that label.
     * A copy younger than [cacheTtl] is returned with no request. When a fetch
     * fails, an older copy is returned however old it is, then [fallback] if
     * nothing was ever fetched, and only then is the failure thrown as an
     * [McpdException]. `enabled = false` does not stop a fetch: a prompt is the
     * agent's input, not telemetry.
     */
    suspend fun getPrompt(
        name: String,
        label: String? = null,
        version: Int? = null,
        cacheTtl: Duration = 60.seconds,
        fallback: PromptFallback? = null,
        project: String = config.project,
    ): Prompt = prompts.get(name, label, version, cacheTtl, fallback, project)

    /**
     * Store a new version of a prompt and return it. A body identical to the
     * latest version stores nothing and returns that version, so a deploy can
     * post its prompts on every start.
     */
    suspend fun createPrompt(request: CreatePromptRequest, project: String = config.project): Prompt =
        prompts.create(request, project)

    /**
     * Store a new version of a prompt, built from its parts. [messages] are
     * maps or JSON objects with `role` and `content`; [config] is model
     * settings the daemon stores and does not read.
     */
    suspend fun createPrompt(
        name: String,
        kind: PromptKind? = null,
        template: String? = null,
        messages: List<Any?>? = null,
        config: Map<String, Any?>? = null,
        note: String? = null,
        labels: List<String>? = null,
        project: String = this.config.project,
    ): Prompt = prompts.create(
        CreatePromptRequest(
            name = name,
            kind = kind,
            template = template,
            messages = messages?.let(::toJsonElement),
            config = config?.let(::toJsonElement),
            note = note,
            labels = labels,
        ),
        project,
    )

    /**
     * Point [label] at [version] of a prompt. Moving `production` releases a
     * version and moving it back rolls one back. The read cache is not
     * cleared, so a running agent sees the move within its `cacheTtl`.
     */
    suspend fun setPromptLabel(
        name: String,
        label: String,
        version: Int,
        project: String = config.project,
    ): Prompt = prompts.setLabel(name, label, version, project)

    // ------------------------------------------------------- experiments ----

    /** A run as the tool calls, model generations and handoffs its agent took. */
    suspend fun getTrajectory(traceId: String, project: String = config.project): Trajectory =
        transport.getJson(
            "/v1/api/traces/${segment(traceId)}/trajectory",
            query("project" to project),
            Trajectory.serializer(),
        )

    /**
     * The cases of a dataset. Sends [limit] only: the daemon finds the project
     * from the dataset.
     */
    suspend fun listDatasetItems(datasetId: String, limit: Int? = null): List<DatasetItem> =
        transport.getJson(
            "/v1/api/datasets/${segment(datasetId)}/items",
            query("limit" to limit),
            ListSerializer(DatasetItem.serializer()),
        )

    /**
     * Record an experiment, or replace one: the daemon upserts on
     * `experiment_id`, and an empty id gets a new one. This client's project
     * is used when the correlation names none.
     */
    suspend fun createExperiment(experiment: Experiment): Experiment {
        val correlation = experiment.correlation
        val scoped = experiment.copy(correlation = correlation.copy(project = correlation.project.ifBlank { config.project }))
        return transport.postJson(
            "/v1/api/experiments",
            emptyList(),
            json.encodeToString(Experiment.serializer(), scoped),
            Experiment.serializer(),
        )
    }

    /**
     * Score every run of an experiment, each against the expected answer of
     * the dataset case it names. No [scorers] runs the code scorers that need
     * no settings.
     */
    suspend fun scoreExperiment(
        experimentId: String,
        scorers: List<String> = emptyList(),
        config: ScorerConfig? = null,
        project: String = this.config.project,
    ): ScoreRunResult {
        // Built by hand so the body carries what was given and nothing else:
        // the generated request would add `dry_run` to every call.
        val body = buildMap<String, JsonElement> {
            put("scorers", JsonArray(scorers.map(::JsonPrimitive)))
            if (config != null) put("config", json.encodeToJsonElement(ScorerConfig.serializer(), config))
        }
        return transport.postJson(
            "/v1/api/experiments/${segment(experimentId)}/score",
            query("project" to project),
            json.encodeToString(JsonObject.serializer(), JsonObject(body)),
            ScoreRunResult.serializer(),
        )
    }

    /** An experiment case by case, with a summary per scorer. */
    suspend fun experimentResults(experimentId: String, project: String = config.project): ExperimentResults =
        transport.getJson(
            "/v1/api/experiments/${segment(experimentId)}/results",
            query("project" to project),
            ExperimentResults.serializer(),
        )

    /**
     * Run [target] over every case of a dataset and score the answers.
     *
     * Each case runs inside a root span named after the experiment, carrying
     * the experiment id and the case id, with at most [concurrency] cases at
     * once. The span is in the coroutine context, so spans [target] opens are
     * its children. A target that throws fails its own run and the next case
     * still runs. Then the runs are flushed and scored, the experiment is
     * marked `succeeded` with each scorer's mean, and the results are
     * returned. When the runner itself fails, the experiment is marked
     * `failed` and the error is thrown.
     *
     * [name] defaults to the dataset id. [metadata] is kept with the
     * experiment, such as the prompt version under test.
     */
    suspend fun runExperiment(
        datasetId: String,
        name: String = datasetId,
        scorers: List<String> = emptyList(),
        config: ScorerConfig? = null,
        concurrency: Int = 4,
        metadata: Map<String, Any?>? = null,
        target: ExperimentTarget,
    ): ExperimentResults = ExperimentRunner(this).run(datasetId, name, scorers, config, concurrency, metadata, target)

    // ------------------------------------------------------------ lifecycle --

    /**
     * Send everything queued and wait for it to land, for at most [timeout].
     *
     * Idempotent, and safe to call at any time. It does not throw: a flush that
     * could not reach the server has already been reported. Returns `false`
     * when [timeout] passed first; what was not sent by then stays queued for
     * the worker, and a batch already in flight finishes on its own.
     */
    suspend fun flush(timeout: Duration = DEFAULT_DEADLINE): Boolean {
        if (!config.enabled) return true
        return try {
            withTimeoutOrNull(timeout) {
                exportPending()
                true
            } ?: false.also {
                diagnostics.warn("flush") { "flush did not finish within $timeout; ${queue.size} signals are still queued" }
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            diagnostics.warn("flush") { "flush did not complete: $e" }
            false
        }
    }

    /**
     * Stop the worker and send what is left, within [timeout].
     *
     * Safe to call twice; the second call returns immediately. The final export
     * runs uncancellable, because shutdown is usually reached from a `finally`
     * during cancellation and the last batch is the one that explains why the
     * process is going away. It is not unbounded: a daemon that accepts the
     * connection and never answers costs every batch three attempts of the
     * full request timeout, which for a full queue is more than half an hour.
     * When [timeout] passes, no further batch starts and what is still queued
     * is given up; [stats] still counts it as queued.
     */
    suspend fun shutdown(timeout: Duration = DEFAULT_DEADLINE) {
        if (!closed.compareAndSet(false, true)) return
        try {
            withContext(NonCancellable) {
                val finished = withTimeoutOrNull(timeout) {
                    worker?.cancelAndJoin()
                    exportPending()
                    true
                }
                if (finished == null) {
                    abandoned.set(true)
                    diagnostics.warn("shutdown") {
                        "shutdown did not finish within $timeout; ${queue.size} queued signals were given up"
                    }
                }
            }
        } catch (e: Exception) {
            diagnostics.warn("shutdown") { "shutdown did not complete cleanly: $e" }
        } finally {
            // A batch still in flight finishes before the scope goes, rather
            // than being cut off halfway through its request.
            val sending = inflight
            if (sending != null && sending.isActive) {
                sending.invokeOnCompletion { scope.cancel() }
            } else {
                scope.cancel()
            }
        }
    }

    /** Blocking [shutdown], so a client can be used in `use { }` or a shutdown hook. */
    override fun close() {
        runBlocking { shutdown() }
    }

    // -------------------------------------------------------------- internals -

    /**
     * Create a span and hand its opening record to the exporter.
     *
     * A span is exported when it starts and again when it ends, so a run that
     * crashes mid-flight is still visible: an unfinished span with no end time
     * is exactly the run you need to see.
     */
    internal fun newSpan(
        name: String,
        kind: SpanKind,
        parent: ActiveSpan?,
        remote: TraceParent?,
        isEvent: Boolean,
        input: Any?,
        attributes: Map<String, Any?>?,
        metadata: Map<String, Any?>?,
        tags: List<String>?,
        model: String?,
        provider: String?,
        entityKind: EntityKind?,
        entityId: String?,
        entityName: String?,
        sessionId: String? = null,
        threadId: String? = null,
        userId: String? = null,
        runId: String? = null,
        requestId: String? = null,
        output: Any? = null,
        externalParentSpanId: String? = null,
        capturePayloads: Boolean? = null,
        experimentId: String? = null,
        alwaysSampled: Boolean = false,
    ): ActiveSpan {
        val created = guard("span-start") {
            // The sampling decision is made once, at the root, and inherited.
            // A half-recorded trace reads as a bug in the user's own system.
            val sampled = when {
                parent != null -> parent.sampled
                remote != null -> remote.sampled
                alwaysSampled -> true
                else -> decideSampling()
            }
            val parentSpanId = parent?.spanId ?: remote?.parentSpanId
            val inherited = parent?.correlation ?: baseCorrelation
            val span = ActiveSpan(
                client = this,
                traceId = parent?.traceId ?: remote?.traceId ?: Ids.newTraceId(),
                spanId = Ids.newSpanId(),
                parentSpanId = parentSpanId,
                name = name,
                kind = kind,
                isRoot = parentSpanId == null,
                isEvent = isEvent,
                sampled = sampled,
                recorded = config.enabled && sampled,
                startTime = Clock.System.now(),
                initialCorrelation = inherited.copy(
                    sessionId = sessionId ?: inherited.sessionId,
                    threadId = threadId ?: inherited.threadId,
                    userId = userId ?: inherited.userId,
                    runId = runId ?: inherited.runId,
                    requestId = requestId ?: inherited.requestId,
                    experimentId = experimentId ?: inherited.experimentId,
                ),
                capturesPayloads = capturePayloads ?: config.capturePayloads,
                externalParentSpanId = externalParentSpanId,
            )
            // A step context's usage sink travels down the tree, so a model
            // call three spans deep still counts toward the step.
            span.usageSink = parent?.usageSink
            span.initialise(
                input = input,
                attributes = attributes,
                metadata = metadata,
                tags = tags,
                model = model,
                provider = provider,
                entityKind = entityKind,
                entityId = entityId,
                entityName = entityName,
                output = output,
            )
            recordSpanStart(span)
            span
        }
        // Even a client that could not build a span hands back a usable handle,
        // so calling code never has to null-check its own instrumentation.
        return created ?: ActiveSpan(
            client = this,
            traceId = parent?.traceId ?: Ids.newTraceId(),
            spanId = Ids.newSpanId(),
            parentSpanId = parent?.spanId,
            name = name,
            kind = kind,
            isRoot = parent == null,
            isEvent = isEvent,
            sampled = false,
            recorded = false,
            startTime = Clock.System.now(),
            initialCorrelation = baseCorrelation,
        )
    }

    private fun recordSpanStart(span: ActiveSpan) {
        if (!span.recorded || closed.get()) return
        val slot = Signal.SpanSlot(redacted(span.toRecord(Clock.System.now())))
        span.slot = slot
        enqueue(slot)
    }

    internal fun recordSpanEnd(span: ActiveSpan) {
        if (!span.recorded || closed.get()) return
        // Build the record before touching the queue: the span's lock is never
        // held while the queue's is, so the two can never deadlock.
        val record = redacted(span.toRecord(Clock.System.now()))
        val slot = span.slot
        span.slot = null
        if (slot != null && queue.update(slot, record)) return
        enqueue(Signal.SpanSlot(record))
    }

    internal fun recordLog(
        level: LogLevel,
        message: String,
        fields: Map<String, Any?>?,
        span: ActiveSpan?,
    ) {
        // Below the floor costs one comparison: no record, no queue, no
        // correlation lookup.
        if (!meetsLevel(level, config.logLevel)) return
        if (!emitting(span)) return
        guard("log") {
            val now = Clock.System.now()
            enqueue(
                Signal.LogSlot(
                    redacted(LogRecord(
                        logId = newId(),
                        timestamp = now,
                        level = level,
                        message = message,
                        correlation = span?.correlation ?: baseCorrelation,
                        receivedAt = now,
                        traceId = span?.traceId,
                        spanId = span?.spanId,
                        fields = encodeFields(fields),
                    ))
                )
            )
        }
    }

    internal fun recordMetric(
        name: String,
        value: Double,
        kind: MetricKind,
        labels: Map<String, Any?>?,
        unit: String?,
        span: ActiveSpan?,
    ) {
        if (!emitting(span)) return
        // A value JSON cannot carry would cost the whole batch this point
        // travels in. The point itself is what cannot be sent, so it is the
        // one thing dropped (contract section 4.1).
        if (!value.isFinite()) {
            diagnostics.warn("metric-value") { "metric $name has the value $value, which JSON cannot carry; the point was dropped" }
            return
        }
        guard("metric") {
            val now = Clock.System.now()
            enqueue(
                Signal.MetricSlot(
                    MetricPoint(
                        metricId = newId(),
                        name = name,
                        kind = kind,
                        value = value,
                        timestamp = now,
                        correlation = span?.correlation ?: baseCorrelation,
                        receivedAt = now,
                        unit = unit,
                        labels = encodeFields(labels),
                        traceId = span?.traceId,
                        spanId = span?.spanId,
                    )
                )
            )
        }
    }

    internal fun recordScore(
        traceId: String,
        spanId: String?,
        scorerId: String,
        value: Double,
        min: Double,
        max: Double,
        reason: String?,
        label: String?,
        source: ScoreSource,
        scorerVersion: String?,
        metadata: Map<String, Any?>?,
        correlation: Correlation,
    ) {
        if (!config.enabled || closed.get()) return
        if (!value.isFinite() || !min.isFinite() || !max.isFinite()) {
            diagnostics.warn("score-value") {
                "score $scorerId has value $value on [$min, $max], which JSON cannot carry; the score was dropped"
            }
            return
        }
        guard("score") {
            enqueue(
                Signal.ScoreSlot(
                    Score(
                        scoreId = newId(),
                        scorerId = scorerId,
                        source = source,
                        value = value,
                        timestamp = Clock.System.now(),
                        correlation = correlation,
                        traceId = traceId,
                        spanId = spanId,
                        scorerVersion = scorerVersion,
                        min = min,
                        max = max,
                        label = label,
                        reason = reason,
                        metadata = toJsonObject(metadata),
                    )
                )
            )
        }
    }

    internal fun recordFeedback(
        traceId: String,
        spanId: String?,
        kind: FeedbackKind,
        value: Any?,
        comment: String?,
        authorId: String?,
        authorKind: String?,
        metadata: Map<String, Any?>?,
        correlation: Correlation,
    ) {
        if (!config.enabled || closed.get()) return
        guard("feedback") {
            enqueue(
                Signal.FeedbackSlot(
                    Feedback(
                        feedbackId = newId(),
                        kind = kind,
                        value = toJsonElement(value),
                        timestamp = Clock.System.now(),
                        correlation = correlation,
                        traceId = traceId,
                        spanId = spanId,
                        comment = comment,
                        authorId = authorId,
                        authorKind = authorKind,
                        metadata = toJsonObject(metadata),
                    )
                )
            )
        }
    }

    /**
     * Run instrumentation work, swallowing anything it throws.
     *
     * This is the single most important rule in the SDK: observability must
     * never take down the thing it observes. Cancellation is rethrown, because
     * that belongs to the caller's coroutine and is not our failure to absorb.
     */
    internal fun <T> guard(operation: String, body: () -> T): T? = try {
        body()
    } catch (e: CancellationException) {
        throw e
    } catch (t: Throwable) {
        // Throwable, not Exception. A StackOverflowError from a deeply nested
        // or cyclic payload is an Error, not an Exception, and catching only
        // Exception would let it escape into the caller: exactly the failure
        // B1 exists to prevent. The two Throwables that must not be absorbed
        // are rethrown above (cancellation) and below (thread death).
        if (t is ThreadDeath) throw t
        diagnostics.warn(operation) { "$operation failed and was ignored: $t" }
        null
    }

    /**
     * The span with [McpdConfig.redact] applied to its payload fields. The
     * record is immutable, so the caller's live span keeps what it was given.
     */
    private fun redacted(record: Span): Span {
        if (config.redact == null) return record
        return record.copy(
            input = redactField(record.input),
            output = redactField(record.output),
            attributes = redactField(record.attributes, objectOnly = true),
            metadata = redactField(record.metadata, objectOnly = true),
        )
    }

    /** The log line with [McpdConfig.redact] applied to its fields. */
    private fun redacted(record: LogRecord): LogRecord {
        if (config.redact == null) return record
        return record.copy(fields = redactField(record.fields, objectOnly = true))
    }

    /**
     * One value through the redactor, failing closed.
     *
     * Throwable, as in [guard], because a redactor that recurses through a deep
     * payload can overflow the stack, and that must cost the field, not the
     * caller. The field is dropped rather than sent as it was: a broken
     * redactor is the case where the original does the most harm.
     *
     * With [objectOnly], a result that is not a JSON object is unusable too,
     * because the field is a key-value map on the wire, and is dropped and
     * counted the same way. A null result is a deliberate drop and is not
     * counted.
     */
    private fun redactField(value: JsonElement?, objectOnly: Boolean = false): JsonElement? {
        val redact = config.redact ?: return value
        if (value == null) return null
        return try {
            val kept = redact(value)
            if (kept != null && objectOnly && kept !is JsonObject) {
                redactionFailureCount.incrementAndGet()
                diagnostics.warn("redact") { "redact returned a value that is not an object; it was dropped" }
                null
            } else {
                kept
            }
        } catch (e: CancellationException) {
            throw e
        } catch (t: Throwable) {
            if (t is ThreadDeath) throw t
            redactionFailureCount.incrementAndGet()
            diagnostics.warn("redact") { "redact failed and the value was dropped: $t" }
            null
        }
    }

    /** A signal is worth queueing only when the client is on and its trace was kept. */
    private fun emitting(span: ActiveSpan?): Boolean =
        config.enabled && !closed.get() && (span == null || span.recorded)

    private fun enqueue(signal: Signal) {
        val evicted = queue.offer(signal)
        if (evicted > 0) {
            diagnostics.warn("queue-full") {
                "the export queue is full at ${config.maxQueue} signals and is dropping the oldest " +
                    "(${queue.dropped} so far); mcpd is unreachable, or slower than this process " +
                    "is producing signals"
            }
        }
        if (queue.size >= config.batchSize) wake.trySend(Unit)
    }

    private fun decideSampling(): Boolean = when {
        config.sample >= 1.0 -> true
        config.sample <= 0.0 -> false
        else -> ThreadLocalRandom.current().nextDouble() < config.sample
    }

    private suspend fun runWorker() {
        while (coroutineContext.isActive) {
            // Wake on a full batch, or on the interval, whichever comes first.
            withTimeoutOrNull(config.flushInterval) { wake.receive() }
            try {
                exportPending()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                diagnostics.warn("export") { "export failed: $e" }
            }
        }
    }

    /**
     * Drain and send until the queue is empty.
     *
     * The lock makes concurrent flushes serial, so [flush] genuinely waits for
     * everything, including a batch the worker was already sending.
     */
    private suspend fun exportPending() {
        while (true) {
            exportLock.lock()
            if (abandoned.get()) {
                exportLock.unlock()
                break
            }
            val signals = queue.drain(config.batchSize)
            if (signals.isEmpty()) {
                exportLock.unlock()
                break
            }
            // Sent and counted by a job of the client's own scope, which holds
            // the lock until the batch is done. A caller whose deadline passes,
            // or a worker being stopped, stops waiting and starts nothing more,
            // while the batch in flight finishes, is counted, and keeps the
            // next one from starting beside it.
            val job = scope.launch(start = CoroutineStart.ATOMIC) {
                try {
                    sendBatch(signals)
                } finally {
                    exportLock.unlock()
                }
            }
            inflight = job
            job.join()
        }
    }

    /** One batch through the sink, counted either way. Never throws but for cancellation. */
    private suspend fun sendBatch(signals: List<Signal>) {
        // One key per batch, reused across retries, so a request that timed
        // out after the server stored it cannot double-count.
        val key = newId()
        val accepted = try {
            sink.export(ingestBatch(signals, key), key)
            true
        } catch (e: CancellationException) {
            throw e
        } catch (t: Throwable) {
            if (t is ThreadDeath) throw t
            // The transport reports its own refusals with the reason; any
            // other sink is reported here, once per interval.
            if (t !is BatchRefused) diagnostics.warn("sink") { "the sink refused a batch: $t" }
            false
        }
        if (accepted) exportedCount.addAndGet(signals.size.toLong()) else failedCount.incrementAndGet()
    }

    private fun newId(): String = UUID.randomUUID().toString()

    /**
     * Label filters the way the daemon reads them: `key:value` joined by
     * commas. A key or value the format cannot carry is refused rather than
     * sent, because the daemon would split it into a different filter and
     * answer a different question.
     */
    private fun metricFilter(filter: Map<String, String>?): String? {
        if (filter.isNullOrEmpty()) return null
        return filter.entries.joinToString(",") { (key, value) ->
            require(key.isNotEmpty() && ':' !in key && ',' !in key) {
                "metric filter key \"$key\" cannot be empty or contain ':' or ','"
            }
            require(',' !in value) { "metric filter value \"$value\" for \"$key\" cannot contain ','" }
            "$key:$value"
        }
    }

    /** Render query parameters, dropping the ones that were not supplied. */
    internal fun query(vararg pairs: Pair<String, Any?>): List<Pair<String, String?>> =
        pairs.map { (key, value) ->
            key to when (value) {
                null -> null
                // The API reads a list filter as one comma-separated value.
                is Collection<*> -> value.joinToString(",").takeIf { it.isNotEmpty() }
                else -> value.toString()
            }
        }
}
