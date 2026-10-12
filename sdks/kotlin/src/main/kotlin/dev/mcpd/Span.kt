package dev.mcpd

import kotlin.time.Instant
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.time.Duration.Companion.nanoseconds

/** A stack trace longer than this is truncated. Nobody reads the four hundredth frame. */
private const val MAX_STACK_CHARS = 16 * 1024

/**
 * One operation being recorded.
 *
 * This is the live, mutable handle the caller holds. [Span] is the immutable
 * record that goes on the wire; a handle produces one when it starts and
 * another when it ends.
 *
 * Every method here is safe to call from any thread, safe to call after the
 * span has ended, and safe to call when the client is disabled or the trace was
 * not sampled. None of them throw: instrumentation that can fail the operation
 * it observes is worse than no instrumentation.
 */
class ActiveSpan internal constructor(
    internal val client: Mcpd,
    /** The trace this span belongs to. 32 lowercase hex characters. */
    val traceId: String,
    /** This span's id. 16 lowercase hex characters. */
    val spanId: String,
    /** The parent span within this trace, absent on a root. */
    val parentSpanId: String?,
    /** The operation's name. */
    val name: String,
    /** What kind of operation this is. */
    val kind: SpanKind,
    /** True when this span has no parent inside the trace. */
    val isRoot: Boolean,
    /** True for a point in time rather than a duration. */
    val isEvent: Boolean,
    /** Whether the trace was sampled. Decided once at the root and inherited. */
    val sampled: Boolean,
    /** Whether anything will actually be sent for this span. */
    val recorded: Boolean,
    /** When the operation began, in UTC. */
    val startTime: Instant,
    initialCorrelation: Correlation,
    /**
     * Whether `input` and `output` may leave the process. The client's
     * `capturePayloads` unless the span was started with its own.
     */
    val capturesPayloads: Boolean = true,
    /**
     * A parent from a tracing system that is not mcpd, carried for correlation
     * and never used for parenting inside mcpd's own tree.
     */
    val externalParentSpanId: String? = null,
) {
    /**
     * A monotonic reading taken at the same moment as [startTime].
     *
     * Duration is measured from this rather than from two wall-clock readings,
     * because the wall clock can step sideways under NTP and a negative
     * duration in a waterfall reads as a bug in mcpd.
     */
    private val startedAtNanos: Long = System.nanoTime()

    private val lock = Any()
    private val ended = AtomicBoolean(false)
    private val attributes = LinkedHashMap<String, JsonElement>()
    private val metadata = LinkedHashMap<String, JsonElement>()
    private val tags = ArrayList<String>()
    private val links = ArrayList<SpanLink>()

    /** Project, environment, session and user. Inherited by every child. */
    internal var correlation: Correlation = initialCorrelation
        private set

    private var status: SpanStatus = SpanStatus.UNSET
    private var input: JsonElement? = null
    private var output: JsonElement? = null
    private var usage: TokenUsage? = null
    private var modelParameters: ModelParameters? = null
    private var error: SpanError? = null
    private var model: String? = null
    private var provider: String? = null
    private var entityKind: EntityKind? = null
    private var entityId: String? = null
    private var entityName: String? = null
    private var endTime: Instant? = null
    private var durationNs: Long? = null

    /**
     * The queue entry holding this span's start record, while it is still queued.
     *
     * Volatile because a span is routinely started on one thread and ended on
     * another: a coroutine that suspends can resume anywhere in the dispatcher's
     * pool. Without this, the ending thread can read a stale null and queue a
     * second record instead of updating the first, which shows up as a
     * duplicated span in the trace.
     */
    @Volatile
    internal var slot: Signal.SpanSlot? = null

    /**
     * Where this span's usage goes when it ends, if anywhere.
     *
     * A step context installs one on the span it opens, and every span nested
     * under that span inherits it, so the step's usage is the sum of what its
     * spans spent without the body adding tokens by hand. Never on the wire.
     */
    @Volatile
    internal var usageSink: ((TokenUsage, String?, String?) -> Unit)? = null

    private val usageReported = AtomicBoolean(false)

    /** Whether [end] has run. */
    val isEnded: Boolean get() = ended.get()

    /** The W3C `traceparent` header to send with an outgoing request made inside this span. */
    fun traceparent(): String = TraceParent(traceId, spanId, if (sampled) 1 else 0).toHeader()

    /** What went in. For a model call, the full prompt. Kept in the process when capture is off. */
    fun setInput(value: Any?): ActiveSpan = mutate { if (capturesPayloads) input = toJsonElement(value) }

    /** What came out. For a model call, the completion. Kept in the process when capture is off. */
    fun setOutput(value: Any?): ActiveSpan = mutate { if (capturesPayloads) output = toJsonElement(value) }

    /** Which model produced this, and who serves it. */
    fun setModel(model: String, provider: String? = null): ActiveSpan = mutate {
        this.model = model
        if (provider != null) this.provider = provider
    }

    /** Token counts, on a model or embedding span. Cost is computed from these at ingest. */
    fun setUsage(
        inputTokens: Long? = null,
        outputTokens: Long? = null,
        totalTokens: Long? = null,
        inputDetails: InputTokenDetails? = null,
        outputDetails: OutputTokenDetails? = null,
    ): ActiveSpan = setUsage(
        TokenUsage(
            inputTokens = inputTokens,
            outputTokens = outputTokens,
            totalTokens = totalTokens ?: sumOrNull(inputTokens, outputTokens),
            inputDetails = inputDetails,
            outputDetails = outputDetails,
        )
    )

    /** Token counts, already assembled. */
    fun setUsage(usage: TokenUsage): ActiveSpan = mutate { this.usage = usage }

    /**
     * How the model was called: temperature, nucleus and top-k sampling, the
     * output ceiling, the seed, the stop sequences, and whether the output was
     * constrained to a fixed shape.
     *
     * These are settings, not payload. Nothing here carries a prompt or a
     * completion, so they are recorded whether or not payload capture is on.
     * They are what a run has to be reproduced from, and dropping them with
     * the prompt would leave a run nobody can repeat.
     *
     * An argument you leave out is absent; an argument you pass is kept
     * whatever its value is, so `seed = 0` is a seed and `temperature = 0.0`
     * is a temperature. One call describes one model call, so a second call
     * replaces the first: settings half from one request and half from another
     * would describe a call nobody made.
     */
    fun setModelParameters(
        temperature: Double? = null,
        topP: Double? = null,
        topK: Int? = null,
        maxTokens: Long? = null,
        seed: Long? = null,
        stopSequences: List<String>? = null,
        hasResponseFormat: Boolean = false,
    ): ActiveSpan = mutate {
        modelParameters = ModelParameters(
            hasResponseFormat = hasResponseFormat,
            // A setting JSON cannot carry is left out, as though it had not
            // been given, rather than costing the batch (contract section 4.1).
            temperature = temperature?.takeIf { it.isFinite() },
            topP = topP?.takeIf { it.isFinite() },
            topK = topK,
            maxTokens = maxTokens,
            seed = seed,
            stopSequences = stopSequences?.toList(),
        )
    }

    /**
     * Record a failure.
     *
     * The message is required and the status becomes `error`: "something
     * failed" with no message is not something anyone can act on.
     */
    fun setError(
        message: String,
        kind: String? = null,
        stack: String? = null,
        retryable: Boolean? = null,
        domain: String? = null,
    ): ActiveSpan = mutate {
        error = SpanError(
            // B10: an error must carry a message. A blank one reaches the trace
            // list as a failure nobody can act on, so say plainly that the
            // producer gave nothing rather than storing an empty string.
            message = message.ifBlank { NO_MESSAGE },
            kind = kind,
            stack = stack?.take(MAX_STACK_CHARS),
            domain = domain,
            retryable = retryable,
        )
        status = SpanStatus.ERROR
    }

    /** Record a failure from an exception, keeping its class and stack. */
    fun setError(failure: Throwable, retryable: Boolean? = null): ActiveSpan = setError(
        message = failure.message?.takeIf { it.isNotBlank() } ?: failure.toString(),
        kind = failure.javaClass.name,
        stack = failure.stackTraceToString(),
        retryable = retryable,
    )

    private companion object {
        /** Stored when a caller records a failure but gives no message. */
        const val NO_MESSAGE = "no message given"
    }

    /** Attach one piece of kind-specific structured detail. */
    fun setAttribute(key: String, value: Any?): ActiveSpan = mutate {
        attributes[key] = toJsonElement(value)
    }

    /** Attach several. */
    fun setAttributes(values: Map<String, Any?>): ActiveSpan = mutate {
        values.forEach { (key, value) -> attributes[key] = toJsonElement(value) }
    }

    /** Anything you want to keep on the span. mcpd never interprets it. */
    fun setMetadata(values: Map<String, Any?>): ActiveSpan = mutate {
        values.forEach { (key, value) -> metadata[key] = toJsonElement(value) }
    }

    /**
     * Free labels for filtering.
     *
     * Tags label the whole run, so mcpd reads them from the root span. Setting
     * them on a child records them, but nothing will filter on them.
     */
    fun setTags(values: List<String>): ActiveSpan = mutate { tags.addAll(values) }

    /** Free labels for filtering, as varargs. */
    fun setTags(vararg values: String): ActiveSpan = setTags(values.toList())

    /** Who or what this run is for. Applies to this span and everything under it. */
    fun setCorrelation(
        sessionId: String? = null,
        threadId: String? = null,
        userId: String? = null,
        resourceId: String? = null,
        runId: String? = null,
        requestId: String? = null,
        release: String? = null,
    ): ActiveSpan = mutate {
        correlation = correlation.copy(
            sessionId = sessionId ?: correlation.sessionId,
            threadId = threadId ?: correlation.threadId,
            userId = userId ?: correlation.userId,
            resourceId = resourceId ?: correlation.resourceId,
            runId = runId ?: correlation.runId,
            requestId = requestId ?: correlation.requestId,
            release = release ?: correlation.release,
        )
    }

    /**
     * Point at another span or trace this one follows from.
     *
     * Links express causality that parentage cannot: a batch job caused by
     * twenty requests has twenty links and one parent.
     */
    fun addLink(traceId: String, spanId: String? = null, kind: String? = null): ActiveSpan = mutate {
        links.add(SpanLink(traceId = traceId, spanId = spanId, kind = kind))
    }

    /** Start a child operation. It ends independently of this one. */
    fun child(
        name: String,
        kind: SpanKind = SpanKind.GENERIC,
        input: Any? = null,
        attributes: Map<String, Any?>? = null,
        metadata: Map<String, Any?>? = null,
        model: String? = null,
        provider: String? = null,
        entityKind: EntityKind? = null,
        entityId: String? = null,
        entityName: String? = null,
        externalParentSpanId: String? = null,
        capturePayloads: Boolean? = null,
    ): ActiveSpan = client.newSpan(
        name = name,
        kind = kind,
        parent = this,
        remote = null,
        isEvent = false,
        input = input,
        attributes = attributes,
        metadata = metadata,
        tags = null,
        model = model,
        provider = provider,
        entityKind = entityKind,
        entityId = entityId,
        entityName = entityName,
        externalParentSpanId = externalParentSpanId,
        capturePayloads = capturePayloads,
    )

    /**
     * Record a point in time inside this span.
     *
     * An event has no duration and is handed to the exporter immediately, so it
     * survives even if the process dies before this span ends.
     */
    fun event(
        name: String,
        attributes: Map<String, Any?>? = null,
        kind: SpanKind = SpanKind.GENERIC,
        input: Any? = null,
        output: Any? = null,
        capturePayloads: Boolean? = null,
    ): ActiveSpan = client.newSpan(
        name = name,
        kind = kind,
        parent = this,
        remote = null,
        isEvent = true,
        input = input,
        attributes = attributes,
        metadata = null,
        tags = null,
        model = null,
        provider = null,
        entityKind = null,
        entityId = null,
        entityName = null,
        output = output,
        capturePayloads = capturePayloads,
    )

    /** A log line correlated to this span. */
    fun log(level: LogLevel, message: String, fields: Map<String, Any?>? = null): ActiveSpan =
        forward { client.recordLog(level, message, fields, this) }

    /** A numeric observation with this span as its exemplar. */
    fun metric(
        name: String,
        value: Double,
        kind: MetricKind = MetricKind.GAUGE,
        labels: Map<String, Any?>? = null,
        unit: String? = null,
    ): ActiveSpan = forward { client.recordMetric(name, value, kind, labels, unit, this) }

    /** A machine evaluation of this span. `min` and `max` declare the scale. */
    fun score(
        scorerId: String,
        value: Double,
        min: Double,
        max: Double,
        reason: String? = null,
        label: String? = null,
        source: ScoreSource = ScoreSource.LIVE,
        scorerVersion: String? = null,
    ): ActiveSpan = forward {
        client.recordScore(
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
            metadata = null,
            correlation = correlation,
        )
    }

    /** A human judgement of this span. */
    fun feedback(
        kind: FeedbackKind,
        value: Any?,
        comment: String? = null,
        authorId: String? = null,
        authorKind: String? = null,
    ): ActiveSpan = forward {
        client.recordFeedback(
            traceId = traceId,
            spanId = spanId,
            kind = kind,
            value = value,
            comment = comment,
            authorId = authorId,
            authorKind = authorKind,
            metadata = null,
            correlation = correlation,
        )
    }

    /**
     * Finish the span and hand the final record to the exporter.
     *
     * Calling it twice does nothing the second time. With no argument the
     * status is `ok`, unless an error was recorded, in which case it stays
     * `error`: a caller who reported a failure and then ended normally meant
     * the failure.
     */
    fun end(status: SpanStatus? = null) {
        if (isEvent) return
        if (!ended.compareAndSet(false, true)) return
        client.guard("span-end") {
            val elapsed = System.nanoTime() - startedAtNanos
            synchronized(lock) {
                durationNs = elapsed
                endTime = startTime + elapsed.nanoseconds
                this.status = status ?: if (error != null) SpanStatus.ERROR else SpanStatus.OK
            }
            client.recordSpanEnd(this)
            reportUsage()
        }
    }

    /** Hand the usage upward once, to whatever the step context installed. */
    private fun reportUsage() {
        val sink = usageSink ?: return
        val (spent, spentModel, spentProvider) = synchronized(lock) { Triple(usage, model, provider) }
        if (spent == null) return
        if (usageReported.compareAndSet(false, true)) sink(spent, spentModel, spentProvider)
    }

    /** Build the wire record as it stands now. */
    internal fun toRecord(receivedAt: Instant): Span = synchronized(lock) {
        Span(
            traceId = traceId,
            spanId = spanId,
            name = name,
            kind = kind,
            status = status,
            startTime = startTime,
            isRoot = isRoot,
            isEvent = isEvent,
            correlation = correlation,
            receivedAt = receivedAt,
            parentSpanId = parentSpanId,
            externalParentSpanId = externalParentSpanId,
            endTime = endTime,
            durationNs = durationNs,
            entityKind = entityKind,
            entityId = entityId,
            entityName = entityName,
            input = input,
            output = output,
            attributes = attributes.takeIf { it.isNotEmpty() }?.let { JsonObject(LinkedHashMap(it)) },
            metadata = metadata.takeIf { it.isNotEmpty() }?.let { JsonObject(LinkedHashMap(it)) },
            usage = usage,
            modelParameters = modelParameters,
            error = error,
            links = links.takeIf { it.isNotEmpty() }?.toList(),
            tags = tags.takeIf { it.isNotEmpty() }?.toList(),
            model = model,
            provider = provider,
        )
    }

    /** Fields supplied at creation, before anyone else can see the span. */
    internal fun initialise(
        input: Any?,
        attributes: Map<String, Any?>?,
        metadata: Map<String, Any?>?,
        tags: List<String>?,
        model: String?,
        provider: String?,
        entityKind: EntityKind?,
        entityId: String?,
        entityName: String?,
        output: Any? = null,
    ) {
        synchronized(lock) {
            if (input != null && capturesPayloads) this.input = toJsonElement(input)
            if (output != null && capturesPayloads) this.output = toJsonElement(output)
            attributes?.forEach { (key, value) -> this.attributes[key] = toJsonElement(value) }
            metadata?.forEach { (key, value) -> this.metadata[key] = toJsonElement(value) }
            if (tags != null) this.tags.addAll(tags)
            this.model = model
            this.provider = provider
            this.entityKind = entityKind
            this.entityId = entityId
            this.entityName = entityName
            // An event happened; it has no outcome to wait for.
            if (isEvent) this.status = SpanStatus.OK
        }
    }

    /**
     * Run a mutation under the lock, swallowing anything it throws.
     *
     * Every setter goes through here, which is what makes "instrumentation
     * never throws" a property of the class rather than a promise in the
     * documentation.
     */
    private fun mutate(body: () -> Unit): ActiveSpan {
        client.guard("span-mutate") { synchronized(lock) { body() } }
        return this
    }

    /** Hand another signal to the client. Takes no span lock, so nothing can nest. */
    private fun forward(body: () -> Unit): ActiveSpan {
        client.guard("span-signal") { body() }
        return this
    }

    private fun sumOrNull(a: Long?, b: Long?): Long? =
        if (a == null && b == null) null else (a ?: 0) + (b ?: 0)
}

/**
 * Coerce a caller's value into JSON.
 *
 * A prompt, a tool argument or a metadata value can be any object the caller
 * happens to have. Anything not directly representable falls back to its string
 * form: an imperfect record of the payload is worth more than a dropped span,
 * and a dropped span is the only other option.
 */
internal fun toJsonElement(value: Any?): JsonElement = when (value) {
    null -> JsonNull
    is JsonElement -> value
    is String -> JsonPrimitive(value)
    is Boolean -> JsonPrimitive(value)
    // NaN and the infinities have no JSON literal, and the encoder refuses the
    // whole batch over one. Named instead, they cost their own key and nothing
    // else (contract section 4.1).
    is Double -> if (value.isFinite()) JsonPrimitive(value) else JsonPrimitive(value.toString())
    is Float -> if (value.isFinite()) JsonPrimitive(value) else JsonPrimitive(value.toString())
    is Number -> JsonPrimitive(value)
    is Enum<*> -> JsonPrimitive(value.toString())
    is Map<*, *> -> JsonObject(value.entries.associate { (key, item) -> key.toString() to toJsonElement(item) })
    is Iterable<*> -> JsonArray(value.map { toJsonElement(it) })
    is Array<*> -> JsonArray(value.map { toJsonElement(it) })
    else -> JsonPrimitive(value.toString())
}

/** Convert a field or label map, dropping nothing and interpreting nothing. */
internal fun toJsonObject(values: Map<String, Any?>?): JsonObject? =
    values?.takeIf { it.isNotEmpty() }?.let { map ->
        JsonObject(map.entries.associate { (key, value) -> key to toJsonElement(value) })
    }
