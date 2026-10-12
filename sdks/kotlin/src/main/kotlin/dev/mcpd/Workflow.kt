package dev.mcpd

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlin.time.Duration

/*
 * The authoring surface for durable workflows.
 *
 * A workflow is data the daemon holds and bodies the daemon never sees. The
 * builder produces both from one file: [Workflow.document] is the definition
 * the daemon walks, in the shape spec/definition.schema.json describes, and
 * the handlers stay in this process for a [Worker] to serve. There is no YAML
 * to keep in step with the code, because the code is where the graph is
 * written.
 *
 * Every construct of the run contract has a method here. A branch, a foreach
 * body and a loop body are one claimable step each; a multi-step body is a
 * [Workflow.subworkflow].
 */

/**
 * A guard the daemon evaluates: a leaf from [predicate] or [exists], or
 * [allOf], [anyOf] and [not] over leaves. A path reads `input`, `steps.<id>`,
 * `run`, and `item`, `index` or `iter.index` where they apply.
 */
typealias Predicate = JsonObject

/**
 * A leaf predicate: `predicate("steps.order.output.amount_usd", "gt", 100)`.
 * `op` is one of eq, ne, lt, lte, gt, gte, in, matches.
 */
fun predicate(path: String, op: String, value: Any?): Predicate =
    JsonObject(mapOf("path" to JsonPrimitive(path), "op" to JsonPrimitive(op), "value" to toJsonElement(value)))

/** A leaf predicate that holds when the path has a value. */
fun exists(path: String): Predicate =
    JsonObject(mapOf("path" to JsonPrimitive(path), "op" to JsonPrimitive("exists")))

/** Holds when every predicate holds. */
fun allOf(vararg predicates: Predicate): Predicate = JsonObject(mapOf("all" to JsonArray(predicates.toList())))

/** Holds when at least one predicate holds. */
fun anyOf(vararg predicates: Predicate): Predicate = JsonObject(mapOf("any" to JsonArray(predicates.toList())))

/** Holds when the predicate does not. */
fun not(predicate: Predicate): Predicate = JsonObject(mapOf("not" to predicate))

/**
 * How failures classed `retryable` are retried. [maxAttempts] counts every
 * attempt including the first, so one means no retry.
 */
data class RetryPolicy(
    val maxAttempts: Int? = null,
    val backoff: Backoff? = null,
    val initial: Duration? = null,
    val max: Duration? = null,
) {
    internal fun wire(): JsonObject = JsonObject(
        buildMap {
            maxAttempts?.let { put("max_attempts", JsonPrimitive(it)) }
            backoff?.let { put("backoff", JsonPrimitive(it.wire)) }
            initial?.let { put("initial_ms", JsonPrimitive(it.inWholeMilliseconds)) }
            max?.let { put("max_ms", JsonPrimitive(it.inWholeMilliseconds)) }
        },
    )
}

/** The ceilings a workflow declares. */
data class Limits(
    val maxSteps: Int? = null,
    val runTimeout: Duration? = null,
    val maxCostUsd: Double? = null,
) {
    internal fun wire(): JsonObject = JsonObject(
        buildMap {
            maxSteps?.let { put("max_steps", JsonPrimitive(it)) }
            runTimeout?.let { put("run_timeout_ms", JsonPrimitive(it.inWholeMilliseconds)) }
            maxCostUsd?.let { put("max_cost_usd", JsonPrimitive(it)) }
        },
    )
}

/**
 * What a node accepts. Each field says which constructs read it; the others
 * ignore it.
 *
 * @property guard A guard. False writes the node as `skipped` and the run moves on.
 * @property input The input template. Default: the run's input. `${path}`
 *   references are rendered per attempt. Read by step, agent and subworkflow.
 * @property effect What a duplicated body would cost. Default `at_least_once`.
 * @property timeout One attempt's own ceiling, heartbeats or not.
 * @property retry How failures classed `retryable` are retried.
 * @property onError What happens after the retries. Default `fail_run`.
 * @property acceptParked Let a wait take an event that arrived before it opened.
 * @property onTimeout What a wait does when its deadline passes. Default `fail`.
 * @property concurrency How many foreach items run at once.
 * @property until Ends a loop early; evaluated after each iteration.
 * @property version Pins a subworkflow's definition version.
 */
data class StepOptions(
    val guard: Predicate? = null,
    val input: Any? = null,
    val effect: Effect? = null,
    val timeout: Duration? = null,
    val retry: RetryPolicy? = null,
    val onError: OnError? = null,
    val acceptParked: Boolean? = null,
    val onTimeout: WaitTimeout? = null,
    val concurrency: Int? = null,
    val until: Predicate? = null,
    val version: Int? = null,
)

/**
 * A step body. Returns the step's output, which is sent as JSON, or throws. A
 * thrown [StepFailureException] carries its own class; any other throw is `fatal`.
 */
typealias StepHandler = suspend (StepContext) -> Any?

private val STEP_ID = Regex("^[a-z][a-z0-9_]*$")

private fun checkId(id: String): String {
    require(STEP_ID.matches(id)) { "\"$id\" is not a step id; an id matches ^[a-z][a-z0-9_]*$" }
    return id
}

private fun common(node: MutableMap<String, JsonElement>, options: StepOptions): MutableMap<String, JsonElement> {
    options.guard?.let { node["when"] = it }
    options.onError?.let { node["on_error"] = JsonPrimitive(it.wire) }
    return node
}

private fun claimable(kind: StepKind, id: String, options: StepOptions): JsonObject {
    val node = common(mutableMapOf("kind" to JsonPrimitive(kind.wire), "id" to JsonPrimitive(id)), options)
    options.input?.let { node["input"] = toJsonElement(it) }
    options.effect?.let { node["effect"] = JsonPrimitive(it.wire) }
    options.timeout?.let { node["step_timeout_ms"] = JsonPrimitive(it.inWholeMilliseconds) }
    options.retry?.let { node["retry"] = it.wire() }
    return JsonObject(node)
}

private fun node(kind: String, id: String, options: StepOptions, vararg fields: Pair<String, JsonElement>): MutableMap<String, JsonElement> =
    common(mutableMapOf("kind" to JsonPrimitive(kind), "id" to JsonPrimitive(id), *fields), options)

/** What a parallel branch, a foreach body or a loop body may hold: one claimable step each. */
class Branch internal constructor() {
    internal val nodes = mutableListOf<JsonObject>()
    internal val handlers = mutableMapOf<String, StepHandler>()

    /** A branch or body that a worker serves. */
    fun step(id: String, options: StepOptions = StepOptions(), body: StepHandler): Branch =
        add(StepKind.TASK, id, options, body)

    /** The same, recorded as an agent turn. */
    fun agent(id: String, options: StepOptions = StepOptions(), body: StepHandler): Branch =
        add(StepKind.AGENT, id, options, body)

    private fun add(kind: StepKind, id: String, options: StepOptions, body: StepHandler): Branch {
        nodes.add(claimable(kind, checkId(id), options))
        handlers[id] = body
        return this
    }
}

/**
 * A workflow under construction. Steps run in the order they are added. An
 * id the daemon would refuse throws [IllegalArgumentException] as it is added.
 */
class Workflow(
    /** The definition id, unique within the project. */
    val id: String,
    /** Name a version to own the number; leave it out and the daemon assigns one. */
    val version: Int? = null,
    /** A JSON Schema the run's input must satisfy. */
    private val inputSchema: Any? = null,
    /** The ceilings. */
    private val limits: Limits? = null,
) {
    private val nodes = mutableListOf<JsonObject>()
    private val handlers = mutableMapOf<String, StepHandler>()
    /** The body id a foreach or loop root maps to, so a lease's key finds its handler. */
    private val bodyOf = mutableMapOf<String, String>()
    private var output: Any? = null
    private var hasOutput = false

    init {
        checkId(id)
    }

    /** A claimable step a worker serves. */
    fun step(id: String, options: StepOptions = StepOptions(), body: StepHandler): Workflow =
        add(StepKind.TASK, id, options, body)

    /** A claimable step recorded as an agent turn: your own loop, however many turns, as one durable step. */
    fun agent(id: String, options: StepOptions = StepOptions(), body: StepHandler): Workflow =
        add(StepKind.AGENT, id, options, body)

    private fun add(kind: StepKind, id: String, options: StepOptions, body: StepHandler): Workflow {
        nodes.add(claimable(kind, checkId(id), options))
        handlers[id] = body
        return this
    }

    /** A delay. No process is alive during it. */
    fun sleep(id: String, duration: Duration, options: StepOptions = StepOptions()): Workflow {
        nodes.add(JsonObject(node("sleep", checkId(id), options, "ms" to JsonPrimitive(duration.inWholeMilliseconds))))
        return this
    }

    /** A delay until an instant rendered from a template such as `${input.billing_date}`. */
    fun sleepUntil(id: String, at: String, options: StepOptions = StepOptions()): Workflow {
        nodes.add(JsonObject(node("sleep_until", checkId(id), options, "at" to JsonPrimitive(at))))
        return this
    }

    /**
     * A pause until an event arrives. The daemon holds the deadline and the
     * mailbox; the key is rendered per run, so `${input.order_id}` addresses
     * this run and no other.
     */
    fun waitFor(id: String, event: String, key: String, timeout: Duration, options: StepOptions = StepOptions()): Workflow {
        val n = node(
            "wait_event", checkId(id), options,
            "event" to JsonPrimitive(event), "key" to JsonPrimitive(key),
            "timeout_ms" to JsonPrimitive(timeout.inWholeMilliseconds),
        )
        options.acceptParked?.let { n["accept_parked"] = JsonPrimitive(it) }
        options.onTimeout?.let { n["on_timeout"] = JsonPrimitive(it.wire) }
        nodes.add(JsonObject(n))
        return this
    }

    /** Named branches, each one claimable step, all on disk before any is offered. */
    fun parallel(id: String, options: StepOptions = StepOptions(), build: Branch.() -> Unit): Workflow {
        val b = Branch().apply(build)
        nodes.add(JsonObject(node("parallel", checkId(id), options, "branches" to JsonArray(b.nodes))))
        handlers.putAll(b.handlers)
        return this
    }

    /**
     * One claimable step per element of the array at [over]. The body's input
     * template can read `item` and `index`; afterwards `steps.<id>.outputs`
     * is the array of outputs in item order.
     */
    fun forEach(id: String, over: String, maxItems: Int, options: StepOptions = StepOptions(), build: Branch.() -> Unit): Workflow {
        val (body, bodyHandlers) = oneBody(id, "foreach", build)
        val n = node(
            "foreach", checkId(id), options,
            "over" to JsonPrimitive(over), "body" to body, "max_items" to JsonPrimitive(maxItems),
        )
        options.concurrency?.let { n["concurrency"] = JsonPrimitive(it) }
        nodes.add(JsonObject(n))
        adopt(id, bodyHandlers)
        return this
    }

    /** A bounded loop. [StepOptions.until] ends it early; `iter.index` is the iteration number. */
    fun loop(id: String, maxIterations: Int, options: StepOptions = StepOptions(), build: Branch.() -> Unit): Workflow {
        val (body, bodyHandlers) = oneBody(id, "loop", build)
        val n = node("loop", checkId(id), options, "body" to body, "max_iterations" to JsonPrimitive(maxIterations))
        options.until?.let { n["until"] = it }
        nodes.add(JsonObject(n))
        adopt(id, bodyHandlers)
        return this
    }

    /** A child run of another definition. Its output becomes `steps.<id>.output`. */
    fun subworkflow(id: String, definition: String, options: StepOptions = StepOptions()): Workflow {
        val n = node("subworkflow", checkId(id), options, "definition" to JsonPrimitive(definition))
        options.version?.let { n["version"] = JsonPrimitive(it) }
        options.input?.let { n["input"] = toJsonElement(it) }
        nodes.add(JsonObject(n))
        return this
    }

    /** The template rendered when the run reaches the end of the graph. */
    fun output(template: Any?): Workflow {
        output = template
        hasOutput = true
        return this
    }

    /** The definition document the daemon stores and walks. */
    fun document(): DefinitionDocument {
        val body = buildMap<String, JsonElement> {
            put("steps", JsonArray(nodes))
            inputSchema?.let { put("input_schema", toJsonElement(it)) }
            limits?.let { put("limits", it.wire()) }
            if (hasOutput) put("output", toJsonElement(output))
        }
        return DefinitionDocument(id = id, body = JsonObject(body), kind = DefinitionKind.WORKFLOW, version = version)
    }

    /**
     * The handler a lease's step key addresses. `order`, `checks/fraud`,
     * `notify[3]` and `refine[2]/rewrite` each resolve to the lexical id that
     * registered the body.
     */
    fun handlerFor(stepKey: String): StepHandler? {
        val slash = stepKey.indexOf('/')
        val head = if (slash == -1) stepKey else stepKey.substring(0, slash)
        val sub = if (slash == -1) null else stepKey.substring(slash + 1)
        val bracket = head.indexOf('[')
        val root = if (bracket == -1) head else head.substring(0, bracket)
        if (sub != null) return handlers[sub]
        if (bracket != -1) return handlers[bodyOf[root] ?: root]
        return handlers[root]
    }

    private fun oneBody(id: String, what: String, build: Branch.() -> Unit): Pair<JsonObject, Map<String, StepHandler>> {
        val b = Branch().apply(build)
        require(b.nodes.size == 1) { "$what \"$id\" needs exactly one body step; use a subworkflow for more" }
        return b.nodes[0] to b.handlers
    }

    private fun adopt(id: String, bodyHandlers: Map<String, StepHandler>) {
        for ((bodyId, handler) in bodyHandlers) {
            handlers[bodyId] = handler
            bodyOf[id] = bodyId
        }
    }
}
