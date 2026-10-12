package dev.mcpd

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineName
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.selects.select
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.DeserializationStrategy
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.serializer
import java.net.InetAddress
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CopyOnWriteArraySet
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import kotlin.time.Duration
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds

/*
 * The worker: the user's own process, serving step bodies to the daemon.
 *
 * It registers the workflows it holds, long-polls for leases, and for each
 * lease posts `started`, heartbeats while the body runs, and posts `complete`
 * or `fail`. Every 409 the daemon can answer is handled by its reason, because
 * a worker that collapses them into one branch abandons steps it has already
 * finished or reports a failure for a success.
 *
 * Nothing here runs a model or reads a stored prompt. The daemon hands the
 * worker a lease with the rendered input and the prior steps; the body does
 * the rest with the classpath, the pool and the credentials this process holds.
 */

/** A failure with a class, thrown from a step body to tell the daemon how to treat it. */
open class StepFailureException(
    message: String,
    /** Whether it is retried, and how the run reads it. */
    val stepClass: StepErrorClass = StepErrorClass.FATAL,
    /** The least the daemon waits before the next attempt. */
    val retryAfter: Duration? = null,
    /** Anything structured worth keeping with the failure. */
    val details: Any? = null,
) : RuntimeException(message)

/** A transient failure: the daemon retries it while the step's budget lasts. */
class RetryableException(message: String, retryAfter: Duration? = null, details: Any? = null) :
    StepFailureException(message, StepErrorClass.RETRYABLE, retryAfter, details)

/** How a step body ended, as handed to a worker's `onStepEnd` hook. */
sealed class StepOutcome {
    /** The body returned and `complete` was posted. */
    object Completed : StepOutcome() {
        override fun toString(): String = "Completed"
    }

    /** The body threw, or the worker failed it, with this class and message. */
    data class Failed(val errorClass: StepErrorClass, val message: String) : StepOutcome()
}

/** What a worker has done since it was built. */
data class WorkerStats(
    /** Leases taken from the daemon. */
    val claimed: Long,
    /** Bodies running right now. */
    val inflight: Int,
    /** Bodies that returned and were completed. */
    val completed: Long,
    /** Bodies that failed, by error class. */
    val failed: Map<String, Int>,
    /** 409s the daemon answered, by reason. */
    val conflicts: Map<String, Int>,
    /** Leases handed back before any body ran, because this worker does not hold them. */
    val unservable: Long,
)

/**
 * What a step body is handed: the lease, the run's state, and the means to
 * report usage, record spans and notice a cancellation.
 */
class StepContext internal constructor(
    private val mcpd: Mcpd,
    /** The lease, in full. */
    val lease: StepLease,
) {
    /** Which run. */
    val runId: String get() = lease.runId

    /** Which step, with any loop or item index folded in. */
    val stepKey: String get() = lease.stepKey

    /** Which attempt, from zero. */
    val attempt: Int get() = lease.attempt

    /** A key for a third party that accepts one. Stable across retries of this step. */
    val idempotencyKey: String get() = lease.idempotencyKey

    /** The step's span, which the daemon opened; record your own spans under it. */
    val parent: TraceParent = TraceParent(lease.traceId, lease.spanId, 1)

    /** Completes with the reason when the lease is lost or the run ends. */
    val cancelled: CompletableDeferred<String> = CompletableDeferred()

    /** True once the daemon has fenced this lease or ended the run. */
    val isCancelled: Boolean get() = cancelled.isCompleted

    private val lock = Any()
    private var state: MutableMap<String, JsonElement>? = null

    /** What [setUsage] set. Replaces what the spans summed, field for field. */
    private var usage: StepUsage? = null

    /** What the spans opened through [span] reported as they ended. */
    private var summed: StepUsage? = null

    /** The step's rendered input, decoded with [deserializer]. */
    fun <T> input(deserializer: DeserializationStrategy<T>): T = decode(lease.input, deserializer)

    /** The step's rendered input, decoded as [T]. */
    inline fun <reified T> input(): T = input(serializer<T>())

    /** The steps that already ended, keyed by step id, decoded with [deserializer]. */
    fun <T> prior(deserializer: DeserializationStrategy<T>): T = decode(lease.prior, deserializer)

    /** The steps that already ended, keyed by step id, decoded as [T]. */
    inline fun <reified T> prior(): T = prior(serializer<T>())

    /** The run's accumulated state as of this lease, decoded with [deserializer]. */
    fun <T> state(deserializer: DeserializationStrategy<T>): T = decode(lease.state, deserializer)

    /** The run's accumulated state as of this lease, decoded as [T]. */
    inline fun <reified T> state(): T = state(serializer<T>())

    private fun <T> decode(element: JsonElement?, deserializer: DeserializationStrategy<T>): T =
        try {
            mcpd.json.decodeFromJsonElement(deserializer, element ?: JsonNull)
        } catch (e: Exception) {
            throw StepFailureException("the step's data could not be read: ${e.message}", StepErrorClass.INVALID_INPUT)
        }

    /** Merge keys into the run's state when the step completes. Top-level keys replace. */
    fun setState(patch: Map<String, Any?>) {
        synchronized(lock) {
            val current = state ?: mutableMapOf<String, JsonElement>().also { state = it }
            for ((key, value) in patch) current[key] = toJsonElement(value)
        }
    }

    /**
     * Report the tokens this step spent, so the daemon prices and charges them.
     *
     * A body that records its model calls as spans under [span] does not need
     * this: their usage is summed into the step as they end. What is set here
     * is an override, and replaces the summed fields it names. Fields set here
     * replace fields set earlier.
     */
    fun setUsage(usage: StepUsage) {
        synchronized(lock) {
            val current = this.usage ?: StepUsage()
            this.usage = StepUsage(
                inputTokens = usage.inputTokens ?: current.inputTokens,
                outputTokens = usage.outputTokens ?: current.outputTokens,
                totalTokens = usage.totalTokens ?: current.totalTokens,
                inputDetails = usage.inputDetails ?: current.inputDetails,
                outputDetails = usage.outputDetails ?: current.outputDetails,
                model = usage.model ?: current.model,
                provider = usage.provider ?: current.provider,
            )
        }
    }

    /** [setUsage] for the two counts most bodies have. */
    fun setTokens(inputTokens: Long, outputTokens: Long) =
        setUsage(StepUsage(inputTokens = inputTokens, outputTokens = outputTokens))

    /**
     * Run [block] inside a span nested under this step in the run's trace,
     * carrying the run's correlation.
     */
    suspend fun <T> span(name: String, kind: SpanKind = SpanKind.GENERIC, block: suspend (ActiveSpan) -> T): T =
        mcpd.span(
            name, kind,
            sessionId = lease.correlation.sessionId,
            threadId = lease.correlation.threadId,
            userId = lease.correlation.userId,
            traceparent = parent.toHeader(),
        ) { span ->
            // Every span nested under this one inherits the sink, so the
            // step's usage is the sum of what its spans spent.
            span.usageSink = { spent, model, provider -> absorb(spent, model, provider) }
            // The same five fields every SDK carries from the run onto a body's span.
            span.setCorrelation(requestId = lease.correlation.requestId, release = lease.correlation.release)
            block(span)
        }

    /** A span under the step ended carrying usage: add it, once. */
    private fun absorb(spent: TokenUsage, model: String?, provider: String?) {
        synchronized(lock) {
            val current = summed ?: StepUsage()
            summed = StepUsage(
                inputTokens = plus(current.inputTokens, spent.inputTokens),
                outputTokens = plus(current.outputTokens, spent.outputTokens),
                totalTokens = plus(current.totalTokens, spent.totalTokens),
                model = model ?: current.model,
                provider = provider ?: current.provider,
            )
        }
    }

    private fun plus(total: Long?, more: Long?): Long? = if (more == null) total else (total ?: 0) + more

    internal fun collected(): Pair<JsonElement?, StepUsage?> = synchronized(lock) {
        val explicit = usage
        val fromSpans = summed
        val merged = when {
            explicit == null -> fromSpans
            fromSpans == null -> explicit
            else -> {
                // The three counts move together: an override that names one
                // of them replaces all three, or a hand-set input and output
                // would travel with a summed total that disagrees with them.
                val counts = explicit.inputTokens != null || explicit.outputTokens != null || explicit.totalTokens != null
                StepUsage(
                    inputTokens = if (counts) explicit.inputTokens else fromSpans.inputTokens,
                    outputTokens = if (counts) explicit.outputTokens else fromSpans.outputTokens,
                    totalTokens = if (counts) explicit.totalTokens else fromSpans.totalTokens,
                    inputDetails = explicit.inputDetails,
                    outputDetails = explicit.outputDetails,
                    model = explicit.model ?: fromSpans.model,
                    provider = explicit.provider ?: fromSpans.provider,
                )
            }
        }
        state?.let { JsonObject(it.toMap()) } to merged
    }

    internal fun cancel(reason: String) {
        cancelled.complete(reason)
    }
}

/**
 * Serves step bodies from this process. See the file comment for the
 * protocol it drives.
 *
 * @param workerId A name for this process, recorded as the owner of every lease it takes.
 * @param concurrency How many steps to run at once.
 * @param project Which project to serve. Needed only when the key reaches several.
 * @param wait How long each claim waits for work, at most a minute.
 * @param onError Where the worker reports its own failures. Without one, the client's diagnostics write each kind of failure at most once per interval.
 * @param onLease A lease was claimed, before anything is posted for it.
 * @param onStepStart `started` was accepted and the body is about to run.
 * @param onStepEnd The body finished: completed, or failed with a class.
 * @param onConflict A 409 was applied, with the daemon's reason.
 *
 * The four hooks are the caller's code and follow the recording rule: each
 * call is wrapped, one that throws is reported through [onError] and the loop
 * goes on, and none is awaited on the completion path.
 */
class Worker(
    private val mcpd: Mcpd,
    val workerId: String = defaultWorkerId(),
    concurrency: Int = 4,
    private val project: String = mcpd.config.project,
    wait: Duration = 20.seconds,
    private val onError: ((String, Throwable) -> Unit)? = null,
    private val onLease: ((StepLease) -> Unit)? = null,
    private val onStepStart: ((StepContext) -> Unit)? = null,
    private val onStepEnd: ((StepContext, StepOutcome) -> Unit)? = null,
    private val onConflict: ((StepLease, String) -> Unit)? = null,
) {
    private val concurrency = concurrency.coerceAtLeast(1)
    private val wait = wait.coerceIn(Duration.ZERO, 60.seconds)
    private val workflows = mutableListOf<Workflow>()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default + CoroutineName("mcpd-worker-$workerId"))
    private val jobs = CopyOnWriteArraySet<Job>()
    private val heartbeatMs = AtomicLong(10_000)
    private val stopping = AtomicBoolean(false)
    private val registerLock = Mutex()
    private val claimedCount = AtomicLong()
    private val completedCount = AtomicLong()
    private val unservableCount = AtomicLong()
    private val failedByClass = ConcurrentHashMap<String, AtomicInteger>()
    private val conflictsByReason = ConcurrentHashMap<String, AtomicInteger>()

    /** The workflows this worker serves. Their definitions are posted by [serve]. */
    fun register(vararg workflows: Workflow): Worker {
        this.workflows.addAll(workflows)
        return this
    }

    /** Steps running right now. */
    val inflight: Int get() = jobs.size

    /** Claims, bodies in flight, completions, failures by class, 409s by reason and unservable leases. */
    fun stats(): WorkerStats = WorkerStats(
        claimed = claimedCount.get(),
        inflight = inflight,
        completed = completedCount.get(),
        failed = failedByClass.entries.associate { it.key to it.value.get() }.toSortedMap(),
        conflicts = conflictsByReason.entries.associate { it.key to it.value.get() }.toSortedMap(),
        unservable = unservableCount.get(),
    )

    /**
     * Report one of the worker's own failures.
     *
     * A caller's [onError] sees every one. Without one, the client's
     * diagnostics write it at most once per interval per [category]. The
     * category, never the message, is the key: a message names its step, and
     * a key that held the step key let every step of a failing run through
     * the limit, so an outage logged one line per step.
     */
    private fun reportError(category: String, message: String, error: Throwable) {
        val hook = onError
        if (hook != null) {
            hook(message, error)
        } else {
            mcpd.diagnostics.warn("worker-$category") { "$message: $error" }
        }
    }

    /** Run one of the caller's hooks, and report rather than propagate whatever it throws. */
    private fun hook(name: String, body: () -> Unit) {
        try {
            body()
        } catch (e: CancellationException) {
            throw e
        } catch (t: Throwable) {
            if (t is ThreadDeath) throw t
            reportError("hook", "mcpd worker $workerId: the $name hook threw", t)
        }
    }

    private fun conflict(lease: StepLease, reason: ConflictReason) {
        conflictsByReason.computeIfAbsent(reason.wire) { AtomicInteger() }.incrementAndGet()
        hook("onConflict") { onConflict?.invoke(lease, reason.wire) }
    }

    private fun ended(ctx: StepContext, outcome: StepOutcome) {
        when (outcome) {
            is StepOutcome.Completed -> completedCount.incrementAndGet()
            is StepOutcome.Failed -> failedByClass.computeIfAbsent(outcome.errorClass.wire) { AtomicInteger() }.incrementAndGet()
        }
        hook("onStepEnd") { onStepEnd?.invoke(ctx, outcome) }
    }

    /**
     * Register, then claim and serve until [stop] is called. Returns once
     * every step in flight has reported.
     */
    suspend fun serve() {
        stopping.set(false)
        registerDefinitions()
        while (!stopping.get()) {
            val free = concurrency - inflight
            if (free <= 0) {
                waitForASlot()
                continue
            }
            val taken = try {
                claim(free)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                reportError("claim", "mcpd worker $workerId: claim failed; retrying", e)
                delay(1.seconds)
                continue
            }
            if (taken == 0 && !stopping.get()) delay(10.milliseconds)
        }
        drain()
    }

    private suspend fun waitForASlot() {
        val running = jobs.toList()
        if (running.isEmpty()) return
        select<Unit> {
            for (job in running) job.onJoin { }
        }
    }

    /** Stop claiming. [serve] returns once the steps in flight have reported; [drain] waits for the same. */
    fun stop() {
        stopping.set(true)
    }

    /** Post every registered definition and read back the lease timings. */
    suspend fun registerDefinitions() {
        registerLock.withLock {
            val registered = mcpd.registerWorker(workerId, workflows.map { it.document() }, project)
            heartbeatMs.set(registered.heartbeatMs.coerceAtLeast(1_000))
        }
    }

    /**
     * One claim, serving what it granted on coroutines of its own. Returns
     * how many leases were taken. [serve] calls this in a loop; a script that
     * drives its own run calls it with that run's id until the run ends, then
     * [drain].
     */
    suspend fun claim(max: Int = concurrency, runId: String? = null): Int {
        val response = mcpd.claimSteps(workerId, max.coerceIn(1, 100), wait, runId, project)
        for (lease in response.leases) {
            claimedCount.incrementAndGet()
            hook("onLease") { onLease?.invoke(lease) }
            val job = scope.launch { serveLease(lease) }
            jobs.add(job)
            job.invokeOnCompletion { jobs.remove(job) }
        }
        return response.leases.size
    }

    /** Wait for every step in flight. */
    suspend fun drain() {
        while (true) {
            val running = jobs.toList()
            if (running.isEmpty()) return
            running.forEach { it.join() }
        }
    }

    private fun handlerFor(lease: StepLease): StepHandler? {
        for (w in workflows) {
            if (w.id != lease.definitionId) continue
            if (w.version != null && w.version != lease.definitionVersion) continue
            w.handlerFor(lease.stepKey)?.let { return it }
        }
        return null
    }

    private enum class Outcome { LANDED, DONE, ABANDON }

    private suspend fun serveLease(lease: StepLease) {
        val handler = handlerFor(lease)
        if (handler == null) {
            unservableCount.incrementAndGet()
            report(lease) {
                mcpd.failStep(
                    lease.leaseId,
                    StepError(
                        `class` = StepErrorClass.UNSERVABLE,
                        message = "worker $workerId does not serve ${lease.definitionId}@${lease.definitionVersion} step ${lease.stepKey}",
                    ),
                )
            }
            return
        }

        // Before the body. Always. This is what tells the daemon the difference
        // between "offered" and "running", and on an at_most_once step it
        // refuses a complete without it.
        if (report(lease) { mcpd.stepStarted(lease.leaseId) } == Outcome.ABANDON) return

        val ctx = StepContext(mcpd, lease)
        hook("onStepStart") { onStepStart?.invoke(ctx) }
        val beat = scope.launch {
            val every = heartbeatMs.get().milliseconds
            while (isActive) {
                delay(every)
                try {
                    mcpd.stepHeartbeat(lease.leaseId)
                } catch (e: StepConflictException) {
                    // The daemon has moved on without us; the body learns through `cancelled`.
                    conflict(lease, e.conflict.reason)
                    ctx.cancel("${e.conflict.reason.wire}: ${e.conflict.message}")
                    return@launch
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    // A transport failure is not evidence of anything; the next beat may land.
                    reportError("heartbeat", "mcpd worker $workerId: heartbeat failed for ${lease.stepKey}", e)
                }
            }
        }

        // The step's own ceiling, enforced here as well as by the daemon: a
        // body that runs past deadline_ms is cancelled through `cancelled` and
        // the attempt is failed retryable, so the slot is freed and the
        // journal says why, instead of the daemon reclaiming a lease still
        // heartbeating from inside a body that will never return.
        val deadline = lease.deadlineMs?.takeIf { it > 0 }?.let { ms ->
            scope.launch {
                delay(ms)
                if (ctx.isCancelled) return@launch
                val reason = "step deadline of ${ms}ms passed"
                ctx.cancel(reason)
                beat.cancel()
                report(lease) { mcpd.failStep(lease.leaseId, StepError(`class` = StepErrorClass.RETRYABLE, message = reason)) }
                ended(ctx, StepOutcome.Failed(StepErrorClass.RETRYABLE, reason))
            }
        }

        try {
            // The body runs as a coroutine of its own and is raced against its
            // cancellation. When the lease is given up, at its deadline or by a
            // fence, the worker stops waiting and the slot is freed then,
            // whether or not the body was watching `cancelled`, as contract
            // section 7.2 says. The body keeps running until it returns or the
            // worker's scope ends; it no longer holds a slot.
            val body = scope.async { handler(ctx) }
            val finished = try {
                select<Boolean> {
                    body.onJoin { true }
                    ctx.cancelled.onAwait { false }
                }
            } catch (e: CancellationException) {
                // Whoever cancelled this lease's coroutine cancelled its body.
                body.cancel()
                throw e
            }
            if (!finished || ctx.isCancelled) {
                // The body has its signal and is left to finish what it does
                // on the way out; what it returns is ignored. A deadline that
                // fired is posting the failure; wait for that, not the body.
                deadline?.join()
                return
            }
            val output = try {
                body.await()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Throwable) {
                if (ctx.isCancelled) {
                    deadline?.join()
                    return
                }
                val failure = toFailure(e)
                report(lease) { mcpd.failStep(lease.leaseId, failure) }
                ended(ctx, StepOutcome.Failed(failure.`class`, failure.message))
                return
            }
            if (ctx.isCancelled) {
                // A deadline that fired is posting the failure; wait for it.
                deadline?.join()
                return
            }
            val (state, usage) = ctx.collected()
            val encoded = try {
                toJsonElement(output)
            } catch (e: Exception) {
                val message = "the step's output could not be encoded: ${e.message}"
                report(lease) { mcpd.failStep(lease.leaseId, StepError(`class` = StepErrorClass.FATAL, message = message)) }
                ended(ctx, StepOutcome.Failed(StepErrorClass.FATAL, message))
                return
            }
            report(lease) { mcpd.completeStep(lease.leaseId, StepResult(output = encoded, state = state, usage = usage)) }
            ended(ctx, StepOutcome.Completed)
        } finally {
            beat.cancel()
            if (deadline != null && !ctx.isCancelled) deadline.cancel()
        }
    }

    /**
     * Post one worker call and apply the 409 table: `already_applied` is a
     * success, and every other reason means abandon this lease.
     */
    private suspend fun report(lease: StepLease, call: suspend () -> Any?): Outcome = try {
        call()
        Outcome.LANDED
    } catch (e: StepConflictException) {
        conflict(lease, e.conflict.reason)
        when (e.conflict.reason) {
            ConflictReason.ALREADY_APPLIED -> Outcome.DONE
            ConflictReason.NOT_STARTED -> {
                reportError("not-started", "mcpd worker $workerId: a bug in this worker: ${lease.stepKey} was reported before started", e)
                Outcome.ABANDON
            }
            ConflictReason.FENCED, ConflictReason.RUN_TERMINAL -> Outcome.ABANDON
        }
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        reportError("call", "mcpd worker $workerId: call failed for ${lease.stepKey}", e)
        Outcome.ABANDON
    }

    private fun toFailure(error: Throwable): StepError {
        if (error is StepFailureException) {
            return StepError(
                `class` = error.stepClass,
                message = error.message ?: error.stepClass.wire,
                details = error.details?.let { toJsonElement(it) },
                retryAfterMs = error.retryAfter?.inWholeMilliseconds,
            )
        }
        return StepError(`class` = StepErrorClass.FATAL, message = error.message?.takeIf { it.isNotEmpty() } ?: error.javaClass.simpleName)
    }

    companion object {
        private fun defaultWorkerId(): String {
            val host = try {
                InetAddress.getLocalHost().hostName
            } catch (e: Exception) {
                "worker"
            }
            return host + "-" + Ids.newSpanId().take(8)
        }
    }
}
