// GENERATED FILE. DO NOT EDIT.
//
// Produced from spec/mcpd.spec.json by tools/codegen.
// Edit the spec and run `make gen`. `make check` fails if this file drifts.


package dev.mcpd

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import kotlin.time.Instant

/**
 * The version of `spec/mcpd.spec.json` these types were generated from.
 *
 * A daemon reports the same string on `/healthz`. Compare the two at connect
 * time. A mismatch caught there names the problem; the same mismatch found
 * later shows up as a field missing for no visible reason.
 */
const val specVersion: String = "1.0.0"

/**
 * What an operation *is*. The kind drives how a span is rendered in the waterfall,
 * which attributes are meaningful, and which metrics are auto-extracted from it.
 */
@Serializable
enum class SpanKind(val wire: String) {
    /** One agent invocation. Usually the root of a trace. */
    @SerialName("agent_run") AGENT_RUN("agent_run"),
    /**
     * One logical generation: everything from the first request to the final answer,
     * including every intermediate step.
     */
    @SerialName("model_generation") MODEL_GENERATION("model_generation"),
    /**
     * One step inside a generation. A generation that calls tools twice has three
     * steps.
     */
    @SerialName("model_step") MODEL_STEP("model_step"),
    /**
     * The provider round-trip alone, excluding processors and tool execution. Measure
     * pure model latency here.
     */
    @SerialName("model_inference") MODEL_INFERENCE("model_inference"),
    /** A single streamed chunk. Recorded as an event, not a duration. */
    @SerialName("model_chunk") MODEL_CHUNK("model_chunk"),
    /** A tool your code executes. */
    @SerialName("tool_call") TOOL_CALL("tool_call"),
    /**
     * A tool the calling client executes. The server records the marker; the client
     * returns the detail.
     */
    @SerialName("client_tool_call") CLIENT_TOOL_CALL("client_tool_call"),
    /**
     * A tool the model provider executes server-side, such as hosted web search or
     * code execution.
     */
    @SerialName("provider_tool_call") PROVIDER_TOOL_CALL("provider_tool_call"),
    /** A tool reached over the Model Context Protocol. */
    @SerialName("mcp_tool_call") MCP_TOOL_CALL("mcp_tool_call"),
    /**
     * Control passing from one agent to another. The span's name is the agent handed
     * to; its parent is the agent handing off.
     */
    @SerialName("handoff") HANDOFF("handoff"),
    /** One workflow execution. Usually the root of a trace. */
    @SerialName("workflow_run") WORKFLOW_RUN("workflow_run"),
    /** One step within a workflow. */
    @SerialName("workflow_step") WORKFLOW_STEP("workflow_step"),
    /** A branch point, with the conditions it evaluated. */
    @SerialName("workflow_conditional") WORKFLOW_CONDITIONAL("workflow_conditional"),
    /** One condition evaluated inside a branch point. */
    @SerialName("workflow_conditional_eval") WORKFLOW_CONDITIONAL_EVAL("workflow_conditional_eval"),
    /** A fan-out of concurrent branches. */
    @SerialName("workflow_parallel") WORKFLOW_PARALLEL("workflow_parallel"),
    /** A loop construct: foreach, do-while or do-until. */
    @SerialName("workflow_loop") WORKFLOW_LOOP("workflow_loop"),
    /** A deliberate pause. */
    @SerialName("workflow_sleep") WORKFLOW_SLEEP("workflow_sleep"),
    /** A wait for an external event, with its timeout. */
    @SerialName("workflow_wait_event") WORKFLOW_WAIT_EVENT("workflow_wait_event"),
    /** A read from or write to agent memory. */
    @SerialName("memory_operation") MEMORY_OPERATION("memory_operation"),
    /** One ingestion pipeline run: load, chunk, extract, embed, upsert. */
    @SerialName("rag_ingestion") RAG_INGESTION("rag_ingestion"),
    /** One embedding call, possibly batched. */
    @SerialName("rag_embedding") RAG_EMBEDDING("rag_embedding"),
    /** Vector store I/O: query, upsert, delete or fetch. */
    @SerialName("rag_vector_operation") RAG_VECTOR_OPERATION("rag_vector_operation"),
    /** A RAG-specific transform: chunk, extract metadata or rerank. */
    @SerialName("rag_action") RAG_ACTION("rag_action"),
    /** A graph build, traverse, update or prune. */
    @SerialName("graph_action") GRAPH_ACTION("graph_action"),
    /** An input or output processor. */
    @SerialName("processor_run") PROCESSOR_RUN("processor_run"),
    /** One scorer evaluating a target. */
    @SerialName("scorer_run") SCORER_RUN("scorer_run"),
    /** One stage of a scorer's pipeline. */
    @SerialName("scorer_step") SCORER_STEP("scorer_step"),
    /** A safety or policy check, and whether it tripped. */
    @SerialName("guardrail") GUARDRAIL("guardrail"),
    /** A generic retrieval that is not vector-store specific. */
    @SerialName("retrieval") RETRIEVAL("retrieval"),
    /** An inline data transform between pipeline stages. */
    @SerialName("mapping") MAPPING("mapping"),
    /** A dynamic resolver deciding which skills an agent may use. */
    @SerialName("skill_resolution") SKILL_RESOLUTION("skill_resolution"),
    /** A filesystem, sandbox, search or computer-use action. */
    @SerialName("workspace_action") WORKSPACE_ACTION("workspace_action"),
    /** An outbound HTTP call made on the agent's behalf. */
    @SerialName("http_request") HTTP_REQUEST("http_request"),
    /** Anything else. Always valid; never inferred. */
    @SerialName("generic") GENERIC("generic"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): SpanKind? = entries.firstOrNull { it.wire == s }
    }
}

/** The outcome of an operation. */
@Serializable
enum class SpanStatus(val wire: String) {
    /** Still running, or the producer never said. */
    @SerialName("unset") UNSET("unset"),
    /** Completed successfully. */
    @SerialName("ok") OK("ok"),
    /** Failed. `error` carries the detail. */
    @SerialName("error") ERROR("error"),
    /** Abandoned deliberately, by a caller or a timeout. */
    @SerialName("cancelled") CANCELLED("cancelled"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): SpanStatus? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * The six signals mcpd stores. Every one of them is correlated by trace id, most
 * of them by span id as well, and every one of them passes through the same
 * redaction pipeline.
 */
@Serializable
enum class SignalType(val wire: String) {
    /** A unit of work with a start and, usually, an end. */
    @SerialName("span") SPAN("span"),
    /** A structured log record. */
    @SerialName("log") LOG("log"),
    /** A numeric observation. */
    @SerialName("metric") METRIC("metric"),
    /** A machine evaluation of a span or trace. */
    @SerialName("score") SCORE("score"),
    /** A human evaluation of a span or trace. */
    @SerialName("feedback") FEEDBACK("feedback"),
    /**
     * One sentence recorded against a trace along one aspect. Stored, redacted and
     * retained like the other five, and erased with the subject it belongs to.
     */
    @SerialName("aspect_note") ASPECT_NOTE("aspect_note"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): SignalType? = entries.firstOrNull { it.wire == s }
    }
}

/** Severity, ordered. Filtering is always `>=`. */
@Serializable
enum class LogLevel(val wire: String) {
    /** Finer than debug. Off by default. */
    @SerialName("trace") TRACE("trace"),
    /** Detail useful while diagnosing. */
    @SerialName("debug") DEBUG("debug"),
    /** Normal operation. */
    @SerialName("info") INFO("info"),
    /** Recoverable, but worth reading. */
    @SerialName("warn") WARN("warn"),
    /** The operation failed. */
    @SerialName("error") ERROR("error"),
    /** The process cannot continue. */
    @SerialName("fatal") FATAL("fatal"),
    ;

    override fun toString(): String = wire

    /** Ordered severity. Filtering is always `>=`. */
    val severity: Int get() = when (this) {
        TRACE -> 1
        DEBUG -> 5
        INFO -> 9
        WARN -> 13
        ERROR -> 17
        FATAL -> 21
    }

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): LogLevel? = entries.firstOrNull { it.wire == s }
    }
}

/** How a numeric observation aggregates. */
@Serializable
enum class MetricKind(val wire: String) {
    /** Monotonically increasing. Aggregates by sum. */
    @SerialName("counter") COUNTER("counter"),
    /** A point-in-time value. Aggregates by last, min, max or mean. */
    @SerialName("gauge") GAUGE("gauge"),
    /** A distribution. Aggregates by bucket, and answers quantiles. */
    @SerialName("histogram") HISTOGRAM("histogram"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): MetricKind? = entries.firstOrNull { it.wire == s }
    }
}

/** How mcpd decides which traces to keep. */
@Serializable
enum class SamplingKind(val wire: String) {
    /** Keep everything. The default, and correct until volume says otherwise. */
    @SerialName("always") ALWAYS("always"),
    /** Keep nothing. Useful to disable a single project without redeploying. */
    @SerialName("never") NEVER("never"),
    /**
     * Keep a fixed fraction, decided once at the root so a trace is never
     * half-recorded.
     */
    @SerialName("ratio") RATIO("ratio"),
    /**
     * Keep a fixed fraction, decided by hashing the trace id. Every collector in a
     * fleet makes the same decision without coordinating.
     */
    @SerialName("deterministic") DETERMINISTIC("deterministic"),
    /**
     * Buffer the trace, then decide once it finishes. Keeps every error and every slow
     * trace regardless of rate.
     */
    @SerialName("tail") TAIL("tail"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): SamplingKind? = entries.firstOrNull { it.wire == s }
    }
}

/** What happens to a value a redaction rule matched. */
@Serializable
enum class RedactionMode(val wire: String) {
    /** Remove the key entirely, as if it was never sent. */
    @SerialName("drop") DROP("drop"),
    /** Replace the whole value with a fixed token. */
    @SerialName("mask") MASK("mask"),
    /**
     * Keep a documented prefix and suffix, mask the middle. Enough to recognise a
     * value without disclosing it.
     */
    @SerialName("partial") PARTIAL("partial"),
    /**
     * Replace with a keyed SHA-256 digest. Equal inputs stay equal, so you can still
     * group and join, but the original cannot be recovered.
     */
    @SerialName("hash") HASH("hash"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): RedactionMode? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * Where a score came from. Kept separate from the scorer's identity so the same
 * scorer can run in more than one context.
 */
@Serializable
enum class ScoreSource(val wire: String) {
    /** Produced online, as the traced run happened. */
    @SerialName("live") LIVE("live"),
    /** Produced later, by scoring stored traces. */
    @SerialName("offline") OFFLINE("offline"),
    /** Produced by an experiment run against a dataset. */
    @SerialName("experiment") EXPERIMENT("experiment"),
    /** Entered by a person, through the API or the UI. */
    @SerialName("manual") MANUAL("manual"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): ScoreSource? = entries.firstOrNull { it.wire == s }
    }
}

/** How a scorer reaches its answer. */
@Serializable
enum class ScorerKind(val wire: String) {
    /** Deterministic code in the daemon. Free, and gives the same answer twice. */
    @SerialName("code") CODE("code"),
    /**
     * A built-in rubric graded by the configured judge model, in steps: preprocess,
     * analyze, score, reason.
     */
    @SerialName("judge") JUDGE("judge"),
    /**
     * A criterion the operator wrote under `judge.criteria`, graded by the judge model
     * in one call.
     */
    @SerialName("criterion") CRITERION("criterion"),
    /**
     * A scorer definition stored in the project through the API: a judge prompt, or a
     * code scorer with its settings. Versioned, and the version is on every score it
     * produces.
     */
    @SerialName("custom") CUSTOM("custom"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): ScorerKind? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * Which way a scorer's value is better. A comparison reads a fall in `toxicity` as
 * an improvement and a fall in `faithfulness` as a regression.
 */
@Serializable
enum class ScoreDirection(val wire: String) {
    /** A larger value is a better run. */
    @SerialName("higher_is_better") HIGHER_IS_BETTER("higher_is_better"),
    /** A smaller value is a better run, as for `toxicity`, `bias` and `hallucination`. */
    @SerialName("lower_is_better") LOWER_IS_BETTER("lower_is_better"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): ScoreDirection? = entries.firstOrNull { it.wire == s }
    }
}

/** The shape of a human judgement. */
@Serializable
enum class FeedbackKind(val wire: String) {
    /** Binary. `value` is `up` or `down`. */
    @SerialName("thumbs") THUMBS("thumbs"),
    /** Ordinal. `value` is a number inside the scale the producer declares. */
    @SerialName("rating") RATING("rating"),
    /** The right answer. `value` is text. */
    @SerialName("correction") CORRECTION("correction"),
    /** A label chosen from a set. `value` is the label. */
    @SerialName("tag") TAG("tag"),
    /** Free prose, with no score attached. */
    @SerialName("comment") COMMENT("comment"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): FeedbackKind? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * The named thing a span belongs to. Bounded and low-cardinality, so it is safe as
 * a metric label.
 */
@Serializable
enum class EntityKind(val wire: String) {
    /** An agent. */
    @SerialName("agent") AGENT("agent"),
    /** A workflow. */
    @SerialName("workflow") WORKFLOW("workflow"),
    /** A tool. */
    @SerialName("tool") TOOL("tool"),
    /** A model. */
    @SerialName("model") MODEL("model"),
    /** A scorer. */
    @SerialName("scorer") SCORER("scorer"),
    /** A processor. */
    @SerialName("processor") PROCESSOR("processor"),
    /** A RAG or data pipeline. */
    @SerialName("pipeline") PIPELINE("pipeline"),
    /** A plain service with no richer identity. */
    @SerialName("service") SERVICE("service"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): EntityKind? = entries.firstOrNull { it.wire == s }
    }
}

/** Where an experiment run has got to. */
@Serializable
enum class ExperimentStatus(val wire: String) {
    /** Created, not started. */
    @SerialName("pending") PENDING("pending"),
    /** In progress. */
    @SerialName("running") RUNNING("running"),
    /** Every item completed. */
    @SerialName("succeeded") SUCCEEDED("succeeded"),
    /** Aborted. `error` says why. */
    @SerialName("failed") FAILED("failed"),
    /** Stopped by a person. */
    @SerialName("cancelled") CANCELLED("cancelled"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): ExperimentStatus? = entries.firstOrNull { it.wire == s }
    }
}

/** Whether authorisation let the request through. */
@Serializable
enum class AuditResult(val wire: String) {
    /**
     * The key carried the scope the endpoint needs, so mcpd went on to handle the
     * request.
     */
    @SerialName("allowed") ALLOWED("allowed"),
    /**
     * Authorisation refused the request before it reached storage. Recording the
     * attempt is the point of the row.
     */
    @SerialName("refused") REFUSED("refused"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): AuditResult? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * Where a run is. Only `running` can move; the other four are terminal, and a
 * terminal run holds no deadline, no wait and no live attempt.
 */
@Serializable
enum class RunStatus(val wire: String) {
    /**
     * Making progress, or waiting for something that will move it: a lease, a timer or
     * an event.
     */
    @SerialName("running") RUNNING("running"),
    /**
     * Reached the end of the graph. `output` holds what the definition's `output`
     * template rendered, when it declares one.
     */
    @SerialName("succeeded") SUCCEEDED("succeeded"),
    /**
     * Ended on an error nothing handled: a step that failed under `on_error:
     * fail_run`, a spent ceiling, or a cancelled child. `error` says which.
     */
    @SerialName("failed") FAILED("failed"),
    /** Ended because the run's own deadline passed. */
    @SerialName("timed_out") TIMED_OUT("timed_out"),
    /**
     * Ended because a caller asked, through the cancel endpoint or by cancelling an
     * ancestor run.
     */
    @SerialName("cancelled") CANCELLED("cancelled"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): RunStatus? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * Where one attempt at one step is. Four states are live and four are terminal,
 * and the daemon never shows a live attempt on an ended run.
 */
@Serializable
enum class AttemptStatus(val wire: String) {
    /** On disk and offerable to a worker, once its retry timer, if any, has fired. */
    @SerialName("pending") PENDING("pending"),
    /** Held by a worker that has not yet posted `started`. */
    @SerialName("leased") LEASED("leased"),
    /** Held by a worker that has posted `started`, so a body is running. */
    @SerialName("started") STARTED("started"),
    /**
     * The lease expired with no heartbeat. Not offerable while the reclaim window is
     * open: a heartbeat inside the window restores the attempt, and a window that
     * closes with no contact requeues it or fails the run, depending on `effect` and
     * on whether the body had started.
     */
    @SerialName("orphaned") ORPHANED("orphaned"),
    /** Finished with an output. */
    @SerialName("succeeded") SUCCEEDED("succeeded"),
    /**
     * Finished with an error, or marked failed because the run ended while the attempt
     * was live.
     */
    @SerialName("failed") FAILED("failed"),
    /** The step's own deadline passed before the attempt ended. */
    @SerialName("timed_out") TIMED_OUT("timed_out"),
    /** Never ran, because the step's `when` guard was false. */
    @SerialName("skipped") SKIPPED("skipped"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): AttemptStatus? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * What a step attempt is an attempt at. `task` and `agent` are claimable. A `wait`
 * row is written by the event that resumes it or the deadline that ends it, so a
 * wait's output and status read like any other step's.
 */
@Serializable
enum class StepKind(val wire: String) {
    /** Claimable work. The worker runs whatever code it registered under this step id. */
    @SerialName("task") TASK("task"),
    /**
     * Claimable work whose body is the worker's own agent loop. Identical on the wire
     * to `task`; the kind is recorded so the trace and the interface can tell an agent
     * turn from a plain step.
     */
    @SerialName("agent") AGENT("agent"),
    /**
     * A suspension the daemon resumes on an event or ends on a deadline. Never offered
     * to a worker.
     */
    @SerialName("wait") WAIT("wait"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): StepKind? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * What a run's cursor names: the construct at its current position in the
 * definition's `steps`. Every kind except `end` is a node kind a definition can
 * declare.
 */
@Serializable
enum class ConstructKind(val wire: String) {
    /** Claimable work. */
    @SerialName("task") TASK("task"),
    /** Claimable work whose body is the worker's agent loop. */
    @SerialName("agent") AGENT("agent"),
    /** A relative delay, in milliseconds. No process is alive during it. */
    @SerialName("sleep") SLEEP("sleep"),
    /** An absolute delay, to an RFC 3339 instant rendered from a template. */
    @SerialName("sleep_until") SLEEP_UNTIL("sleep_until"),
    /** A suspension addressed by an event name and a key rendered per run. */
    @SerialName("wait_event") WAIT_EVENT("wait_event"),
    /**
     * A fixed fan-out over named branches, each branch one claimable step. Done when
     * every branch has ended.
     */
    @SerialName("parallel") PARALLEL("parallel"),
    /**
     * A fan-out over the items of an array, each item one claimable step, with at most
     * `concurrency` of them offerable at once.
     */
    @SerialName("foreach") FOREACH("foreach"),
    /**
     * A bounded loop whose body is one claimable step per iteration, ended by `until`
     * or by `max_iterations`.
     */
    @SerialName("loop") LOOP("loop"),
    /**
     * A child run of another definition. Its output becomes this step's output, and a
     * cancel walks down into it.
     */
    @SerialName("subworkflow") SUBWORKFLOW("subworkflow"),
    /**
     * The end of the graph. Never declared in a definition: the cursor reaches it when
     * the last step has ended.
     */
    @SerialName("end") END("end"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): ConstructKind? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * What running a step body twice would cost, which is what the daemon branches on
 * when a lease is lost. Default `at_least_once`.
 */
@Serializable
enum class Effect(val wire: String) {
    /**
     * Running the body twice is acceptable. A lost lease requeues the attempt. The
     * default.
     */
    @SerialName("at_least_once") AT_LEAST_ONCE("at_least_once"),
    /**
     * Running the body twice is not acceptable. `started` must be posted before the
     * body runs, and a started attempt whose lease is lost fails the run rather than
     * running again.
     */
    @SerialName("at_most_once") AT_MOST_ONCE("at_most_once"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): Effect? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * Every deadline the daemon keeps. Each one is a row the tick fires, and each
 * firing is a transaction that moves the run.
 */
@Serializable
enum class TimerKind(val wire: String) {
    /** A `sleep` or `sleep_until` coming due. */
    @SerialName("sleep") SLEEP("sleep"),
    /**
     * A lease that has not been heartbeated. Firing moves the attempt to `orphaned`
     * and opens a reclaim window one lease length long.
     */
    @SerialName("lease_expiry") LEASE_EXPIRY("lease_expiry"),
    /**
     * The close of the reclaim window an expired lease opened. Firing requeues the
     * attempt or fails the run, depending on `effect` and on whether the body had
     * started.
     */
    @SerialName("reclaim") RECLAIM("reclaim"),
    /** The whole run's ceiling, from `limits.run_timeout_ms`. */
    @SerialName("run_timeout") RUN_TIMEOUT("run_timeout"),
    /**
     * One attempt's ceiling, from the node's `step_timeout_ms`. It bounds a worker
     * that heartbeats for ever.
     */
    @SerialName("step_timeout") STEP_TIMEOUT("step_timeout"),
    /** A `wait_event` whose event has not arrived by `timeout_ms`. */
    @SerialName("wait_timeout") WAIT_TIMEOUT("wait_timeout"),
    /**
     * The backoff before a requeued attempt becomes claimable. Firing writes the next
     * attempt as `pending`.
     */
    @SerialName("retry") RETRY("retry"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): TimerKind? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * What a row in a run's event log records. Every row is written in the transaction
 * that made the change it describes, so a reader that has seen sequence number n
 * has seen every change below it.
 */
@Serializable
enum class RunEventKind(val wire: String) {
    /** The run was created. Payload: `definition_id`, `definition_version`. */
    @SerialName("run_started") RUN_STARTED("run_started"),
    /**
     * The cursor moved. Payload: `index`, `construct`, and `step_key` when the
     * construct has one.
     */
    @SerialName("run_cursor") RUN_CURSOR("run_cursor"),
    /** A worker claimed an attempt. Payload: `step_key`, `attempt`, `worker_id`. */
    @SerialName("step_leased") STEP_LEASED("step_leased"),
    /** A worker posted `started`. Payload: `step_key`, `attempt`. */
    @SerialName("step_started") STEP_STARTED("step_started"),
    /** An attempt succeeded. Payload: `step_key`, `attempt`. */
    @SerialName("step_finished") STEP_FINISHED("step_finished"),
    /**
     * An attempt failed. Payload: `step_key`, `attempt`, `class`, `message`,
     * `attempts_left`.
     */
    @SerialName("step_failed") STEP_FAILED("step_failed"),
    /** A `when` guard was false. Payload: `step_key`. */
    @SerialName("step_skipped") STEP_SKIPPED("step_skipped"),
    /**
     * The run reached a `wait_event`. Payload: `step_key`, `event`, `key`,
     * `expires_at`.
     */
    @SerialName("run_waiting") RUN_WAITING("run_waiting"),
    /** An event resumed a waiting run. Payload: `step_key`, `event_id`. */
    @SerialName("run_resumed") RUN_RESUMED("run_resumed"),
    /**
     * The run reached a terminal status. Payload: `status`, and `error` when there is
     * one.
     */
    @SerialName("run_ended") RUN_ENDED("run_ended"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): RunEventKind? = entries.firstOrNull { it.wire == s }
    }
}

/** What kind of thing a definition describes. */
@Serializable
enum class DefinitionKind(val wire: String) {
    /**
     * A graph of steps the daemon walks. The only kind in version 1: `agent`
     * definitions arrive with the in-process agent loop, which is not served yet.
     */
    @SerialName("workflow") WORKFLOW("workflow"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): DefinitionKind? = entries.firstOrNull { it.wire == s }
    }
}

/** What the daemon did with a published event. */
@Serializable
enum class EventOutcome(val wire: String) {
    /** A waiting run took it and moved, in the same transaction. */
    @SerialName("delivered") DELIVERED("delivered"),
    /**
     * No run was waiting. The event is held for 24 hours for the next run that reaches
     * a matching wait with `accept_parked`.
     */
    @SerialName("parked") PARKED("parked"),
    /**
     * This event id has been seen before, so nothing was written. The publish endpoint
     * says when a derived id is treated this way.
     */
    @SerialName("duplicate") DUPLICATE("duplicate"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): EventOutcome? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * Why a worker call about a lease answered 409. Each reason tells the worker
 * something different to do next.
 */
@Serializable
enum class ConflictReason(val wire: String) {
    /**
     * A later attempt exists, or another lease terminated this attempt. Abandon the
     * work and post nothing more for this lease.
     */
    @SerialName("fenced") FENCED("fenced"),
    /**
     * This lease's own earlier `complete` or `fail` landed and its reply was lost. The
     * step is done; `attempt_status` says how. Post nothing more.
     */
    @SerialName("already_applied") ALREADY_APPLIED("already_applied"),
    /**
     * `complete` or `fail` arrived for a `leased` attempt under `at_most_once`. A bug
     * in the worker: post `started` first, and log it loudly.
     */
    @SerialName("not_started") NOT_STARTED("not_started"),
    /** The run has ended. Stop, and stop claiming for this run. */
    @SerialName("run_terminal") RUN_TERMINAL("run_terminal"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): ConflictReason? = entries.firstOrNull { it.wire == s }
    }
}

/** How a step failed, which decides whether the daemon retries it. */
@Serializable
enum class StepErrorClass(val wire: String) {
    /**
     * Worth another attempt. The daemon retries while attempts remain, at the node's
     * backoff, and honours `retry_after_ms` when it is larger.
     */
    @SerialName("retryable") RETRYABLE("retryable"),
    /**
     * Another attempt would fail the same way. No retry; the node's `on_error`
     * applies.
     */
    @SerialName("fatal") FATAL("fatal"),
    /** The step's input cannot be acted on. No retry; the node's `on_error` applies. */
    @SerialName("invalid_input") INVALID_INPUT("invalid_input"),
    /**
     * This worker does not serve the definition version, or does not recognise the
     * step key. The attempt returns to `pending` after one lease length without
     * spending the retry budget, so another worker can take it.
     */
    @SerialName("unservable") UNSERVABLE("unservable"),
    /**
     * The worker gave the attempt up on request. No retry; the node's `on_error`
     * applies.
     */
    @SerialName("cancelled") CANCELLED("cancelled"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): StepErrorClass? = entries.firstOrNull { it.wire == s }
    }
}

/** What happens to a run when a node fails after its retries. Default `fail_run`. */
@Serializable
enum class OnError(val wire: String) {
    /** End the run `failed`, with the step's error message as the run's error. */
    @SerialName("fail_run") FAIL_RUN("fail_run"),
    /**
     * Mark the step `failed` and move on, so a later `when` guard can read
     * `steps.<id>.status` and `steps.<id>.error`.
     */
    @SerialName("continue") CONTINUE("continue"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): OnError? = entries.firstOrNull { it.wire == s }
    }
}

/** How the delay between retries grows. */
@Serializable
enum class Backoff(val wire: String) {
    /** Every retry waits `initial_ms`. */
    @SerialName("fixed") FIXED("fixed"),
    /**
     * Each retry waits twice as long as the last, starting at `initial_ms` and capped
     * at `max_ms`.
     */
    @SerialName("exponential") EXPONENTIAL("exponential"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): Backoff? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * What a `wait_event` node does when its deadline passes with no event. Default
 * `fail`.
 */
@Serializable
enum class WaitTimeout(val wire: String) {
    /** The node fails. Its `on_error` decides what the run does next. */
    @SerialName("fail") FAIL("fail"),
    /** The wait ends and the run moves on to the next node. */
    @SerialName("continue") CONTINUE("continue"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): WaitTimeout? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * The JSON type of a value. Used where mcpd reports the shape of something it
 * holds without returning the thing itself.
 */
@Serializable
enum class ValueKind(val wire: String) {
    /** The value was JSON null. */
    @SerialName("null") NULL("null"),
    /** The value was `true` or `false`. */
    @SerialName("boolean") BOOLEAN("boolean"),
    /** The value was a JSON number. */
    @SerialName("number") NUMBER("number"),
    /** The value was a JSON string. */
    @SerialName("string") STRING("string"),
    /** The value was a JSON array. */
    @SerialName("array") ARRAY("array"),
    /** The value was a JSON object. */
    @SerialName("object") OBJECT("object"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): ValueKind? = entries.firstOrNull { it.wire == s }
    }
}

/** What a monitor measures over its window. */
@Serializable
enum class MonitorWatch(val wire: String) {
    /** The fraction of runs that failed, 0 to 1. */
    @SerialName("error_rate") ERROR_RATE("error_rate"),
    /** Run duration at the ninety-fifth percentile, in milliseconds. */
    @SerialName("latency_p95") LATENCY_P95("latency_p95"),
    /** Total spend across the window. */
    @SerialName("cost") COST("cost"),
    /** How many runs happened. Catches a pipeline that has gone silent. */
    @SerialName("trace_count") TRACE_COUNT("trace_count"),
    /** The mean of the scorer named by `scorer_id`. */
    @SerialName("score_mean") SCORE_MEAN("score_mean"),
    /**
     * How far a run strays from the usual shape of its own definition. This watch sets
     * no threshold in steps, because a step count is never high in absolute terms,
     * only high for that definition. It reads the recent completed runs of one
     * definition version, takes the median step count as the usual shape, and reports
     * a run whose modified z-score against the median absolute deviation exceeds the
     * monitor's bound, 3.5 by default. The median absolute deviation is used rather
     * than a mean and a standard deviation because it tolerates up to half its sample
     * being outliers, which is the case here: a runaway cohort must not be allowed to
     * redefine normal. Fewer than twelve completed runs and the watch says nothing
     * rather than guessing. A run that takes several times its usual number of steps
     * trips this long before it reaches `max_steps`, and while its error rate is still
     * zero.
     */
    @SerialName("step_count_divergence") STEP_COUNT_DIVERGENCE("step_count_divergence"),
    /**
     * How much a run repeats itself. For each run in the window, the highest number of
     * attempts it made at any one node of its definition, counting a retry and a loop
     * iteration of that node alike; the watch reads the mean of that number. One means
     * no node ran twice. A loop that re-enters the same node, and a retry storm, both
     * raise this before either reaches `error_rate`.
     */
    @SerialName("run_repetition") RUN_REPETITION("run_repetition"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): MonitorWatch? = entries.firstOrNull { it.wire == s }
    }
}

/** Which way a monitor's threshold is breached. */
@Serializable
enum class MonitorCompare(val wire: String) {
    /** Trip when the value rises above the threshold. */
    @SerialName("above") ABOVE("above"),
    /**
     * Trip when the value falls below it. This is the direction that catches a quality
     * regression, and a silent pipeline.
     */
    @SerialName("below") BELOW("below"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): MonitorCompare? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * Where a monitor or an objective stands. A tripped condition latches: it stays
 * tripped until the number recovers, and too little data never clears it, because
 * silence is not recovery.
 */
@Serializable
enum class MonitorCondition(val wire: String) {
    /** Evaluated, and within its threshold. */
    @SerialName("nominal") NOMINAL("nominal"),
    /** Tripped, and not yet acknowledged. */
    @SerialName("tripped") TRIPPED("tripped"),
    /**
     * Tripped, acknowledged, and still breaching. Returns to `nominal` on its own when
     * the number recovers.
     */
    @SerialName("acknowledged") ACKNOWLEDGED("acknowledged"),
    /**
     * Not enough data in the window to judge. Also the state before the first
     * evaluation.
     */
    @SerialName("insufficient") INSUFFICIENT("insufficient"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): MonitorCondition? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * What the `q` parameter matches on this deployment's storage backend. Reported by
 * `GET /v1/api/capabilities` so a search box can say what it searches instead of
 * leaving a reader to infer it from a result set.
 */
@Serializable
enum class TextSearch(val wire: String) {
    /**
     * `q` is not applied at all. No backend that ships declares this; a backend that
     * did would be telling an interface to hide its search box rather than let
     * somebody type into one that returns everything.
     */
    @SerialName("none") NONE("none"),
    /** `q` matches anywhere inside a word, so `heck` finds `checkout`. */
    @SerialName("substring") SUBSTRING("substring"),
    /**
     * `q` matches whole words only, which is what a person typing two words into a
     * search box expects. There is no stemming: `refund` does not find `refunds`.
     */
    @SerialName("words") WORDS("words"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): TextSearch? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * How the storage backend cuts each signal table on its anchor time. It decides
 * whether expiring a day of data is a metadata operation on one part or a
 * row-by-row delete.
 */
@Serializable
enum class Partitioning(val wire: String) {
    /** One table, no partitions. Retention removes rows. */
    @SerialName("none") NONE("none"),
    /** One partition per day. */
    @SerialName("day") DAY("day"),
    /** One partition per month. */
    @SerialName("month") MONTH("month"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): Partitioning? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * A dimension a filter box can offer values for. The set is closed on purpose:
 * each one is a column or a parsed field the daemon keeps a distinct-value list
 * for as signals arrive, so answering is a read of a small table rather than a
 * scan of every span in the retention window.
 */
@Serializable
enum class Facet(val wire: String) {
    /** The kind of thing a step was: an agent run, a tool call, a retrieval, and so on. */
    @SerialName("entity_kind") ENTITY_KIND("entity_kind"),
    /** The name of that thing, such as the tool that was called. */
    @SerialName("entity_name") ENTITY_NAME("entity_name"),
    /** The service that reported the signal. */
    @SerialName("service_name") SERVICE_NAME("service_name"),
    /** The environment it came from, such as `production`. */
    @SerialName("environment") ENVIRONMENT("environment"),
    /** A tag carried on a run. */
    @SerialName("tag") TAG("tag"),
    /** A metric name, for the name box of a chart. */
    @SerialName("metric_name") METRIC_NAME("metric_name"),
    /**
     * A label key a metric point carries. Project-wide: these are every key seen in
     * the project, not the keys of one named metric.
     */
    @SerialName("metric_label_key") METRIC_LABEL_KEY("metric_label_key"),
    /**
     * A value one label key takes. Needs `key`, because the answer would otherwise be
     * every value of every label mixed together.
     */
    @SerialName("metric_label_value") METRIC_LABEL_VALUE("metric_label_value"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): Facet? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * A signal that can be polled for what has arrived. Only the three that carry a
 * server-set arrival time: a score and a piece of feedback are stamped with the
 * caller's own timestamp, so a watermark over them would either skip a late row or
 * repeat every row for ever.
 */
@Serializable
enum class DeltaSignal(val wire: String) {
    /** The run projection, which is what a run list refreshes from. */
    @SerialName("traces") TRACES("traces"),
    /** Steps, for a waterfall that is still filling in. */
    @SerialName("spans") SPANS("spans"),
    /** Log lines. */
    @SerialName("logs") LOGS("logs"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): DeltaSignal? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * How far the daemon has got with one aspect for one entity. It is computed from
 * the rows that are stored and never recorded, so it cannot go stale, and it is
 * per entity and per aspect rather than global: one agent can have goal themes to
 * read while its behaviour notes are still collecting.
 */
@Serializable
enum class AspectReadiness(val wire: String) {
    /**
     * Fewer notes than a clustering pass needs. The floor is 50 notes for one aspect
     * on one entity, and below it the daemon writes no snapshot at all rather than
     * sorting a handful of sentences into themes nobody should draw a conclusion from.
     */
    @SerialName("collecting") COLLECTING("collecting"),
    /**
     * Enough notes to cluster, and no snapshot covers this aspect yet. The next pass
     * will write one. Nothing is wrong and nothing needs doing.
     */
    @SerialName("processing") PROCESSING("processing"),
    /** A snapshot covers this aspect, so themes exist and every theme route answers. */
    @SerialName("ready") READY("ready"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): AspectReadiness? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * What happened to a theme between the previous snapshot and this one. The daemon
 * matches themes across consecutive snapshots by their wording, so a theme keeps
 * its id while its wording holds, and this records what that match found. It is
 * computed from the two clusterings alone and never from a trace attribute.
 */
@Serializable
enum class ThemeState(val wire: String) {
    /** The first snapshot this theme appears in. */
    @SerialName("birth") BIRTH("birth"),
    /**
     * Present in the previous snapshot and in this one, matched to the same id. The
     * ordinary state of a theme that is holding together.
     */
    @SerialName("continue") CONTINUE("continue"),
    /**
     * The previous snapshot held one theme where this one holds several. The history
     * point's `split_from` names the theme it came out of.
     */
    @SerialName("split") SPLIT("split"),
    /**
     * Several themes in the previous snapshot are one theme here. The history point's
     * `merged_from` names them.
     */
    @SerialName("merge") MERGE("merge"),
    /**
     * Present in an earlier snapshot, absent from the one immediately before this, and
     * back now under its original id. A theme that comes and goes reads as one theme
     * with gaps rather than as several short-lived ones.
     */
    @SerialName("resurrection") RESURRECTION("resurrection"),
    /**
     * The theme was in the previous snapshot and is in none of this one's clusters.
     * Reachable only through a theme's history: a dead theme has no row in the
     * snapshot it died in, so it is never a node in a flow.
     */
    @SerialName("death") DEATH("death"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): ThemeState? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * What one node of a theme flow stands for. A stage holds its themes, at most one
 * noise node and at most one overflow node, and the three together account for
 * every trace in the stage, which is what lets the shares add up to one.
 */
@Serializable
enum class ThemeNodeKind(val wire: String) {
    /**
     * One theme. It carries a `theme_id`, so it can be filtered on, opened and
     * followed through its history.
     */
    @SerialName("theme") THEME("theme"),
    /**
     * The notes in this stage that joined no theme. Noise is not a failure bucket: it
     * holds rare requests, wording nothing else matched and patterns that have not
     * recurred yet, and a large noise share means varied traffic or too few notes
     * rather than something going wrong. Its notes are read from the examples route
     * with `noise` as the theme id.
     */
    @SerialName("noise") NOISE("noise"),
    /**
     * The themes past `theme_limit_per_stage`, folded into one node so the stage
     * totals still add up. It carries no `theme_id` and cannot be selected, filtered
     * on or opened, because it is a presentation fold and not a group of anything.
     */
    @SerialName("other") OTHER("other"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): ThemeNodeKind? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * Why one snapshot is in a landmark selection. A timeline draws a couple of dozen
 * points over a range that may hold thousands of passes, so the daemon chooses
 * them and says why, rather than handing back the newest few and leaving the
 * reader to assume that is the whole story.
 */
@Serializable
enum class SnapshotLandmarkReason(val wire: String) {
    /**
     * The oldest snapshot in the requested range. Always included, so a timeline
     * starts where the range does.
     */
    @SerialName("range_start") RANGE_START("range_start"),
    /**
     * The newest snapshot in the requested range. Always included, so a timeline ends
     * at the current state.
     */
    @SerialName("range_end") RANGE_END("range_end"),
    /**
     * Chosen to spread the remaining points evenly over the range. Sampling is by time
     * and not by ordinal, so an hour of frequent passes does not crowd out a quiet
     * week.
     */
    @SerialName("time_sample") TIME_SAMPLE("time_sample"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): SnapshotLandmarkReason? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * What an account API key may do. The same six scopes the daemon's configured keys
 * carry, under the same names as the `auth` value of each endpoint.
 */
@Serializable
enum class KeyScope(val wire: String) {
    /**
     * Write signals, and read nothing. The key that lives in every process that emits
     * a span.
     */
    @SerialName("ingest") INGEST("ingest"),
    /** Read stored signals: runs, spans, logs, metrics, scores, analytics and datasets. */
    @SerialName("read") READ("read"),
    /**
     * Change stored data: scores, feedback, datasets, experiments and
     * acknowledgements.
     */
    @SerialName("write") WRITE("write"),
    /**
     * Delete data and read the audit log. Also accepted where `execute` or `worker` is
     * needed.
     */
    @SerialName("admin") ADMIN("admin"),
    /** Start, cancel and signal runs, and publish events to them. */
    @SerialName("execute") EXECUTE("execute"),
    /**
     * Claim steps and report their outcome. The key that lives in the process running
     * step bodies.
     */
    @SerialName("worker") WORKER("worker"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): KeyScope? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * What a member may do in an organisation. Each role maps onto the six key scopes
 * on every project the organisation owns, unless a project role says otherwise.
 */
@Serializable
enum class OrgRole(val wire: String) {
    /**
     * Every scope on every project, and everything an admin may do. Only an owner may
     * add or remove an owner, change quotas or delete the organisation.
     */
    @SerialName("owner") OWNER("owner"),
    /**
     * Every scope on every project. Invites and removes members below owner, sets
     * roles, creates and deletes projects, and manages SSO, SCIM and every key in the
     * organisation.
     */
    @SerialName("admin") ADMIN("admin"),
    /**
     * The `ingest`, `read`, `write`, `execute` and `worker` scopes. Everything but
     * `admin`: a member cannot delete data or read the audit log.
     */
    @SerialName("member") MEMBER("member"),
    /** The `read` scope only. */
    @SerialName("viewer") VIEWER("viewer"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): OrgRole? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * A role on one project that replaces a member's organisation role there. Owners
 * and admins keep their reach on every project whatever a project role says.
 */
@Serializable
enum class ProjectRole(val wire: String) {
    /** No access to this project. */
    @SerialName("none") NONE("none"),
    /** The `read` scope on this project. */
    @SerialName("viewer") VIEWER("viewer"),
    /** Every scope but `admin` on this project. */
    @SerialName("member") MEMBER("member"),
    /** Every scope on this project. It does not let the member manage the organisation. */
    @SerialName("admin") ADMIN("admin"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): ProjectRole? = entries.firstOrNull { it.wire == s }
    }
}

/** How an organisation's members log in with single sign-on. */
@Serializable
enum class SsoProvider(val wire: String) {
    /**
     * Any OpenID Connect provider, such as Google, Microsoft Entra ID, Okta or Auth0,
     * found through its issuer's discovery document. Authorization code flow with
     * PKCE, state and nonce.
     */
    @SerialName("oidc") OIDC("oidc"),
    /**
     * GitHub, through an OAuth app. GitHub issues no ID token, so the member is the
     * GitHub user id and the email is the primary verified address GitHub reports.
     */
    @SerialName("github") GITHUB("github"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): SsoProvider? = entries.firstOrNull { it.wire == s }
    }
}

/** The shape of a prompt's body. */
@Serializable
enum class PromptKind(val wire: String) {
    /** One template string, for a completion-style call. */
    @SerialName("text") TEXT("text"),
    /**
     * An ordered list of messages, each with a `role` and a `content` template, for a
     * chat-style call.
     */
    @SerialName("chat") CHAT("chat"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): PromptKind? = entries.firstOrNull { it.wire == s }
    }
}

/** How a stored scorer definition reaches its answer. */
@Serializable
enum class ScorerDefinitionKind(val wire: String) {
    /**
     * The configured judge model grades the run against the definition's `prompt`, in
     * one call.
     */
    @SerialName("judge") JUDGE("judge"),
    /**
     * One of the daemon's code scorers, named in `builtin`, with the settings in
     * `config` fixed. Free, and gives the same answer twice.
     */
    @SerialName("code") CODE("code"),
    ;

    override fun toString(): String = wire

    companion object {
        /** Parse a wire value, or null when this version of the spec does not define it. */
        fun fromWire(s: String): ScorerDefinitionKind? = entries.firstOrNull { it.wire == s }
    }
}

/**
 * Input tokens broken down by how the provider billed them. Every field is
 * optional; absent means the provider did not report it, which is different from
 * zero.
 */
@Serializable
data class InputTokenDetails(
    /** Ordinary text tokens: not cached, not audio, not image. */
    @SerialName("text") val text: Long? = null,
    /** Tokens served from a prompt cache. Usually billed at a large discount. */
    @SerialName("cache_read") val cacheRead: Long? = null,
    /** Tokens written into a prompt cache. Usually billed at a premium. */
    @SerialName("cache_write") val cacheWrite: Long? = null,
    /**
     * Tokens written to a five-minute ephemeral cache, where the provider prices TTLs
     * separately.
     */
    @SerialName("cache_write_5m") val cacheWrite5m: Long? = null,
    /** Tokens written to a one-hour ephemeral cache. */
    @SerialName("cache_write_1h") val cacheWrite1h: Long? = null,
    /** Audio input tokens. */
    @SerialName("audio") val audio: Long? = null,
    /** Image input tokens, including rendered PDF pages. */
    @SerialName("image") val image: Long? = null
) {
}

/** Output tokens broken down by kind. */
@Serializable
data class OutputTokenDetails(
    /** Ordinary completion tokens. */
    @SerialName("text") val text: Long? = null,
    /** Hidden reasoning tokens. Billed, and usually invisible in the response. */
    @SerialName("reasoning") val reasoning: Long? = null,
    /** Audio output tokens. */
    @SerialName("audio") val audio: Long? = null,
    /** Image output tokens. */
    @SerialName("image") val image: Long? = null
) {
}

/**
 * Token counts for one model call. Totals are authoritative; the detail breakdowns
 * explain them and are what cost is actually computed from.
 */
@Serializable
data class TokenUsage(
    /** Total input tokens. */
    @SerialName("input_tokens") val inputTokens: Long? = null,
    /** Total output tokens. */
    @SerialName("output_tokens") val outputTokens: Long? = null,
    /** Input plus output, when the provider reports a total that differs from the sum. */
    @SerialName("total_tokens") val totalTokens: Long? = null,
    /** How the input tokens were billed. */
    @SerialName("input_details") val inputDetails: InputTokenDetails? = null,
    /** How the output tokens were billed. */
    @SerialName("output_details") val outputDetails: OutputTokenDetails? = null
) {
}

/**
 * What a span cost, in a single currency. mcpd computes this from a price book at
 * ingest and stores the result, so a later price change never silently rewrites
 * history.
 */
@Serializable
data class Cost(
    /** ISO 4217 code. `USD` unless the price book says otherwise. */
    @SerialName("currency") val currency: String = "USD",
    /**
     * True when mcpd derived the cost from a price book, false when the producer
     * reported it directly.
     */
    @SerialName("is_estimated") val isEstimated: Boolean = true,
    /** Cost attributable to input tokens. */
    @SerialName("input_cost") val inputCost: Double? = null,
    /** Cost attributable to output tokens. */
    @SerialName("output_cost") val outputCost: Double? = null,
    /** The sum, including any meter that is neither input nor output. */
    @SerialName("total_cost") val totalCost: Double? = null,
    /**
     * Identifier of the price book used, so a figure can be traced back to the rates
     * that produced it.
     */
    @SerialName("price_book") val priceBook: String? = null
) {
}

/**
 * The sampling settings a caller chose for one model call. Recorded because a run
 * nobody can reproduce is a run nobody can debug, and the seed and the sampling
 * settings are what a replay has to match. Every field is what the caller asked
 * for, never what the provider did with it. None of them carries prompt or
 * completion text, so a producer running with payload capture off still records
 * all of them.
 */
@Serializable
data class ModelParameters(
    /**
     * True when the caller constrained the output to a fixed shape, such as a JSON
     * schema or a grammar. The shape itself is not recorded: it can be large and it is
     * caller-owned content, and what a reader needs is whether the constraint was on.
     */
    @SerialName("has_response_format") val hasResponseFormat: Boolean = false,
    /**
     * The sampling temperature the caller asked for. Absent when the caller left the
     * provider's default in place, which is not the same as sending zero.
     */
    @SerialName("temperature") val temperature: Double? = null,
    /**
     * Nucleus sampling: the caller asked the sampler to consider the smallest set of
     * tokens whose probabilities reach this fraction. Absent when the caller set none.
     */
    @SerialName("top_p") val topP: Double? = null,
    /**
     * The caller asked the sampler to consider only this many of the most likely
     * tokens. Not every provider offers it.
     */
    @SerialName("top_k") val topK: Int? = null,
    /**
     * The ceiling the caller put on output length. An answer that stops in the middle
     * of a sentence usually stopped here, and this is the first field to read when one
     * does.
     */
    @SerialName("max_tokens") val maxTokens: Long? = null,
    /**
     * The sampling seed the caller passed. Nothing in mcpd reads it. It is recorded
     * because without it a run cannot be reproduced at all, and a comparison between
     * two runs that differ only in their seed is worth nothing.
     */
    @SerialName("seed") val seed: Long? = null,
    /**
     * The stop sequences the caller supplied. They are short strings the caller wrote,
     * so they are settings rather than payload, and an answer that ends early often
     * ends on one of them. Do not put prompt text here.
     */
    @SerialName("stop_sequences") val stopSequences: List<String>? = null
) {
}

/** Why a span failed. */
@Serializable
data class SpanError(
    /** A human-readable message. Required: an error with no message is not actionable. */
    @SerialName("message") val message: String,
    /** The error class or code, such as `RateLimitError` or `ECONNRESET`. */
    @SerialName("kind") val kind: String? = null,
    /** A stack trace, when the producer captured one. */
    @SerialName("stack") val stack: String? = null,
    /** Which subsystem raised it. */
    @SerialName("domain") val domain: String? = null,
    /** Whether the producer considered the failure worth retrying. */
    @SerialName("retryable") val retryable: Boolean? = null,
    /** Structured detail. Passes through redaction like any other payload. */
    @SerialName("details") val details: JsonElement? = null
) {
}

/**
 * The descriptive context a signal belongs to. Every field here is queryable and
 * most are safe as metric labels; the high-cardinality ones (`user_id`,
 * `session_id`, `request_id`) are queryable but blocked from metric labels by the
 * cardinality filter.
 */
@Serializable
data class Correlation(
    /** Which project this belongs to. The primary tenancy key. */
    @SerialName("project") val project: String,
    /** Deployment environment, such as `production` or `staging`. */
    @SerialName("environment") val environment: String? = null,
    /** The emitting service. */
    @SerialName("service_name") val serviceName: String? = null,
    /** The emitting service's version, so a regression can be pinned to a release. */
    @SerialName("service_version") val serviceVersion: String? = null,
    /** A user-facing session. Groups many traces into one conversation. */
    @SerialName("session_id") val sessionId: String? = null,
    /** A conversation thread within a session. */
    @SerialName("thread_id") val threadId: String? = null,
    /**
     * The end user. Hashed with a keyed digest at ingest, so runs can still be grouped
     * by customer without storing who the customer is. Filtering on it hashes your
     * query value the same way, so lookup keeps working. Rotating the key deliberately
     * breaks the link.
     */
    @SerialName("user_id") val userId: String? = null,
    /** A tenant or workspace inside the calling application. */
    @SerialName("resource_id") val resourceId: String? = null,
    /** The producer's own identifier for this run. */
    @SerialName("run_id") val runId: String? = null,
    /** The inbound request that caused this work. */
    @SerialName("request_id") val requestId: String? = null,
    /** Set when the run was produced by an experiment. */
    @SerialName("experiment_id") val experimentId: String? = null,
    /** A release or git sha, for regression attribution. */
    @SerialName("release") val release: String? = null
) {
}

/**
 * A reference from this span to another, in this trace or a different one. Links
 * express causality that parentage cannot: a batch job caused by twenty requests
 * has twenty links and one parent.
 */
@Serializable
data class SpanLink(
    /** The linked trace. */
    @SerialName("trace_id") val traceId: String,
    /** The linked span, when the link points at one rather than a whole trace. */
    @SerialName("span_id") val spanId: String? = null,
    /** Why the link exists, such as `follows_from`, `retry_of` or `replay_of`. */
    @SerialName("kind") val kind: String? = null,
    /** Anything else about the relationship. */
    @SerialName("attributes") val attributes: JsonElement? = null
) {
}

/** Cursor pagination. Cursors are opaque and stable; never construct one by hand. */
@Serializable
data class Page(
    /** Whether another page exists. */
    @SerialName("has_more") val hasMore: Boolean,
    /** Pass as `cursor` to fetch the next page. Absent on the last page. */
    @SerialName("next_cursor") val nextCursor: String? = null,
    /** Total matching rows, when the backend could count them cheaply. */
    @SerialName("total") val total: Long? = null
) {
}

/**
 * One batch of signals sent to the native ingest endpoint. Any combination of the
 * six signals may appear in a single batch; mcpd sorts them out. Supplying
 * `ingest_key` makes the whole batch idempotent, so a client that retries after a
 * timeout never double-counts.
 */
@Serializable
data class IngestBatch(
    /** Spans to record. */
    @SerialName("spans") val spans: List<Span>? = null,
    /** Log records to record. */
    @SerialName("logs") val logs: List<LogRecord>? = null,
    /** Metric observations to record. */
    @SerialName("metrics") val metrics: List<MetricPoint>? = null,
    /** Machine evaluations to attach. */
    @SerialName("scores") val scores: List<Score>? = null,
    /** Human evaluations to attach. */
    @SerialName("feedback") val feedback: List<Feedback>? = null,
    /**
     * Aspect notes to record. A note is a sentence about a trace, so it arrives with
     * that trace's other signals, under the same idempotency key and through the same
     * redactor.
     */
    @SerialName("aspect_notes") val aspectNotes: List<AspectNote>? = null,
    /**
     * Idempotency key for this batch. Replaying a batch with a key mcpd has already
     * accepted is a no-op that still returns 200.
     */
    @SerialName("ingest_key") val ingestKey: String? = null
) {
}

/** Why one signal in a batch was refused. */
@Serializable
data class IngestError(
    /** Which signal kind the failure was in. */
    @SerialName("signal") val signal: SignalType,
    /** Position within that signal's array, so a client can find the offending record. */
    @SerialName("index") val index: Int,
    /** What was wrong, in terms the sender can act on. */
    @SerialName("message") val message: String
) {
}

/**
 * What mcpd did with a batch. Counts are per signal, and `rejected` is never
 * silently zero: a signal mcpd could not store is always reported with a reason.
 */
@Serializable
data class IngestResult(
    /** How many signals were stored. */
    @SerialName("accepted") val accepted: Int,
    /** How many were refused. */
    @SerialName("rejected") val rejected: Int,
    /** How many were recognised as a replay of an already-accepted batch and skipped. */
    @SerialName("duplicate") val duplicate: Int,
    /** One entry per rejected signal, naming what was wrong and where. */
    @SerialName("errors") val errors: List<IngestError>? = null
) {
}

/** A page of traces. */
@Serializable
data class TraceListResponse(
    /** The matching traces, in the requested order. */
    @SerialName("traces") val traces: List<Trace>,
    /** Cursor and totals. */
    @SerialName("page") val page: Page
) {
}

/** One trace and everything attached to it. */
@Serializable
data class TraceDetail(
    /** The trace projection. */
    @SerialName("trace") val trace: Trace,
    /**
     * Every span, ordered by start time then by depth, so a waterfall renders without
     * sorting.
     */
    @SerialName("spans") val spans: List<Span>,
    /** Logs, when `include` asked for them. */
    @SerialName("logs") val logs: List<LogRecord>? = null,
    /** Scores, when `include` asked for them. */
    @SerialName("scores") val scores: List<Score>? = null,
    /** Feedback, when `include` asked for them. */
    @SerialName("feedback") val feedback: List<Feedback>? = null
) {
}

/** A page of spans. */
@Serializable
data class SpanListResponse(
    /** The matching spans. */
    @SerialName("spans") val spans: List<Span>,
    /** Cursor and totals. */
    @SerialName("page") val page: Page
) {
}

/** A page of log records. */
@Serializable
data class LogListResponse(
    /** The matching records. */
    @SerialName("logs") val logs: List<LogRecord>,
    /** Cursor and totals. */
    @SerialName("page") val page: Page
) {
}

/** One bucket of an aggregated series. */
@Serializable
data class MetricSeriesPoint(
    /** Start of the bucket. */
    @SerialName("timestamp") val timestamp: Instant,
    /** The aggregated value. */
    @SerialName("value") val value: Double,
    /**
     * How many raw observations fell in this bucket. Distinguishes a genuine zero from
     * no data.
     */
    @SerialName("count") val count: Long? = null,
    /**
     * A trace that contributed to this bucket, so a spike on the chart opens the run
     * that caused it.
     */
    @SerialName("exemplar_trace_id") val exemplarTraceId: String? = null
) {
}

/**
 * A histogram metric read back as distributions: the buckets of every point in the
 * window added up, one distribution per label combination, with quantiles read
 * from the merged counts. Bucketed points from OpenTelemetry are added bucket by
 * bucket; for a cumulative stream only the growth across the window counts. Points
 * with one value each, such as mcpd's own derived durations, give exact quantiles
 * and are placed into OpenTelemetry's default boundaries.
 */
@Serializable
data class MetricHistogram(
    /** The metric read. */
    @SerialName("name") val name: String,
    /** How many stored points were merged. */
    @SerialName("points_read") val pointsRead: Long,
    /**
     * Whether the window held more points than one read merges, so the newest ones
     * were used and the rest left out. Narrow the window or add a filter.
     */
    @SerialName("truncated") val truncated: Boolean,
    /** One distribution per label combination. */
    @SerialName("series") val series: List<HistogramSeries>,
    /** UCUM unit of the observations, for axis labelling. */
    @SerialName("unit") val unit: String? = null
) {
}

/** One merged distribution. */
@Serializable
data class HistogramSeries(
    /** Upper bound of each bucket but the last, which is unbounded above. */
    @SerialName("bounds") val bounds: List<Double>,
    /** Observations per bucket: one more entry than `bounds`. */
    @SerialName("counts") val counts: List<Long>,
    /** Every observation counted. */
    @SerialName("count") val count: Long,
    /**
     * Whether the quantiles are exact, read from single observations, or interpolated
     * linearly inside the bucket the rank falls in.
     */
    @SerialName("exact") val exact: Boolean,
    /**
     * Points left out because their bucket boundaries differ from the ones most of
     * this series uses.
     */
    @SerialName("points_skipped") val pointsSkipped: Long,
    /**
     * The `group_by` label values that identify this series. Absent when nothing was
     * grouped.
     */
    @SerialName("labels") val labels: JsonElement? = null,
    /** Their sum. */
    @SerialName("sum") val sum: Double? = null,
    /** The smallest observation, when the points recorded single values. */
    @SerialName("min") val min: Double? = null,
    /** The largest observation, when the points recorded single values. */
    @SerialName("max") val max: Double? = null,
    /** Median. */
    @SerialName("p50") val p50: Double? = null,
    /** 90th percentile. */
    @SerialName("p90") val p90: Double? = null,
    /** 95th percentile. */
    @SerialName("p95") val p95: Double? = null,
    /** 99th percentile. */
    @SerialName("p99") val p99: Double? = null
) {
}

/** A page of stored metric points, as they were recorded. */
@Serializable
data class MetricPointListResponse(
    /** The matching points. */
    @SerialName("points") val points: List<MetricPoint>,
    /** Cursor and totals. */
    @SerialName("page") val page: Page
) {
}

/** One line on a chart: a single label combination over time. */
@Serializable
data class MetricSeries(
    /** Bucketed values, oldest first. */
    @SerialName("points") val points: List<MetricSeriesPoint>,
    /** The label values that identify this series. */
    @SerialName("labels") val labels: JsonElement? = null
) {
}

/** An aggregated metric query result. */
@Serializable
data class MetricSeriesResponse(
    /** The metric queried. */
    @SerialName("name") val name: String,
    /**
     * The bucket width actually used, which may differ from the request when mcpd
     * chose it.
     */
    @SerialName("bucket") val bucket: String,
    /** One entry per label combination. */
    @SerialName("series") val series: List<MetricSeries>,
    /** UCUM unit, for axis labelling. */
    @SerialName("unit") val unit: String? = null
) {
}

/** A page of scores. */
@Serializable
data class ScoreListResponse(
    /** The matching scores. */
    @SerialName("scores") val scores: List<Score>,
    /** Cursor and totals. */
    @SerialName("page") val page: Page
) {
}

/** A page of feedback. */
@Serializable
data class FeedbackListResponse(
    /** The matching feedback. */
    @SerialName("feedback") val feedback: List<Feedback>,
    /** Cursor and totals. */
    @SerialName("page") val page: Page
) {
}

/** Latency distribution over a window, in milliseconds. */
@Serializable
data class LatencyStats(
    /** Median. */
    @SerialName("p50") val p50: Double? = null,
    /** Ninetieth percentile. */
    @SerialName("p90") val p90: Double? = null,
    /** Ninety-fifth percentile. */
    @SerialName("p95") val p95: Double? = null,
    /** Ninety-ninth percentile. */
    @SerialName("p99") val p99: Double? = null,
    /** Arithmetic mean. Reported alongside quantiles, never instead of them. */
    @SerialName("mean") val mean: Double? = null,
    /** Slowest observation in the window. */
    @SerialName("max") val max: Double? = null
) {
}

/** One row of a grouped breakdown. */
@Serializable
data class GroupedStat(
    /** The group's value, such as a model id. */
    @SerialName("key") val key: String,
    /** How many runs fell in this group. */
    @SerialName("count") val count: Long,
    /** How many of them failed. */
    @SerialName("error_count") val errorCount: Long,
    /** Spend attributable to this group. */
    @SerialName("total_cost") val totalCost: Double? = null,
    /** Input tokens for this group. */
    @SerialName("input_tokens") val inputTokens: Long? = null,
    /** Output tokens for this group. */
    @SerialName("output_tokens") val outputTokens: Long? = null,
    /** Latency distribution within this group. */
    @SerialName("latency") val latency: LatencyStats? = null
) {
}

/** Headline numbers for a window. One request backs the overview screen. */
@Serializable
data class AnalyticsSummary(
    /** Start of the window. */
    @SerialName("from") val from: Instant,
    /** End of the window. */
    @SerialName("to") val to: Instant,
    /** Runs started in the window. */
    @SerialName("trace_count") val traceCount: Long,
    /** Steps recorded in the window. */
    @SerialName("span_count") val spanCount: Long,
    /** Runs that failed. */
    @SerialName("error_count") val errorCount: Long,
    /** Failures as a fraction of runs, between 0 and 1. */
    @SerialName("error_rate") val errorRate: Double,
    /** Run duration distribution. */
    @SerialName("latency") val latency: LatencyStats,
    /** Input tokens consumed. */
    @SerialName("input_tokens") val inputTokens: Long,
    /** Output tokens produced. */
    @SerialName("output_tokens") val outputTokens: Long,
    /** Total spend in the window. */
    @SerialName("total_cost") val totalCost: Double,
    /** Currency of `total_cost`. */
    @SerialName("currency") val currency: String = "USD",
    /** Per-group rows, when `group_by` was supplied. */
    @SerialName("groups") val groups: List<GroupedStat>? = null
) {
}

/** Spend in one time bucket for one group. */
@Serializable
data class CostBucket(
    /** Start of the bucket. */
    @SerialName("timestamp") val timestamp: Instant,
    /** The group this row belongs to. */
    @SerialName("key") val key: String,
    /** Spend in this bucket. */
    @SerialName("total_cost") val totalCost: Double,
    /** Input tokens in this bucket. */
    @SerialName("input_tokens") val inputTokens: Long? = null,
    /** Output tokens in this bucket. */
    @SerialName("output_tokens") val outputTokens: Long? = null
) {
}

/** Where the money went. */
@Serializable
data class CostBreakdown(
    /** Start of the window. */
    @SerialName("from") val from: Instant,
    /** End of the window. */
    @SerialName("to") val to: Instant,
    /** Total spend across the window. */
    @SerialName("total_cost") val totalCost: Double,
    /** Totals per group, largest first. */
    @SerialName("groups") val groups: List<GroupedStat>,
    /** Currency of every figure in this response. */
    @SerialName("currency") val currency: String = "USD",
    /** A time series, when `bucket` was supplied. */
    @SerialName("series") val series: List<CostBucket>? = null
) {
}

/** What a deletion removed. */
@Serializable
data class DeleteResult(
    /** How many records were removed, across every signal. */
    @SerialName("deleted") val deleted: Int
) {
}

/** Whether the server is alive and able to serve. */
@Serializable
data class HealthStatus(
    /** `ok` or `degraded`. */
    @SerialName("status") val status: String,
    /** The mcpd version serving this request. */
    @SerialName("version") val version: String,
    /**
     * The data-model spec version this build implements. A client can refuse to talk
     * to an incompatible server.
     */
    @SerialName("spec_version") val specVersion: String,
    /** How long this process has been running. */
    @SerialName("uptime_seconds") val uptimeSeconds: Long,
    /** Which storage backend is configured. */
    @SerialName("storage") val storage: String? = null,
    /** What is degraded, when something is. */
    @SerialName("detail") val detail: String? = null,
    /**
     * True when this daemon offers open sign-up: anyone may create an account at `POST
     * /v1/api/account/signup` and log in at `POST /v1/api/account/login`. Absent when
     * it does not, which is the default. The interface reads this to decide whether to
     * show its sign-up and log-in screens.
     */
    @SerialName("signup") val signup: Boolean? = null,
    /**
     * True when this daemon has accounts, with or without open sign-up. Absent when it
     * does not. The interface reads this to show its log-in screen.
     */
    @SerialName("accounts") val accounts: Boolean? = null,
    /**
     * True when at least one organisation on this daemon logs in with single sign-on,
     * so the log-in screen offers it. Absent otherwise.
     */
    @SerialName("sso") val sso: Boolean? = null
) {
}

/** One scorer mcpd can run, as described to a caller. */
@Serializable
data class ScorerInfo(
    /** Stable identifier, recorded on every score it produces. */
    @SerialName("id") val id: String,
    /** What it measures, in one line. */
    @SerialName("description") val description: String,
    /**
     * Whether it needs settings to mean anything. A scorer that needs an expected
     * answer cannot run without one.
     */
    @SerialName("needs_config") val needsConfig: Boolean,
    /** How the scorer reaches its answer. */
    @SerialName("kind") val kind: ScorerKind,
    /** Which way its value is better. */
    @SerialName("direction") val direction: ScoreDirection,
    /** The `ScorerConfig` settings it reads. Empty when it needs none. */
    @SerialName("config_keys") val configKeys: List<String>,
    /**
     * Whether this daemon can run it now. A judged scorer is unavailable until a
     * `judge` block names an endpoint and a model.
     */
    @SerialName("available") val available: Boolean = true
) {
}

/**
 * Settings shared by the scorers that need them. Every field is optional; a scorer
 * that needs one and does not get it declines instead of guessing.
 */
@Serializable
data class ScorerConfig(
    /** Text the answer must not contain, for `excludes`. Any one of them fails the run. */
    @SerialName("forbidden") val forbidden: List<String>,
    /**
     * Words or phrases the answer should mention, for `keyword_coverage`. The score is
     * the share that appear.
     */
    @SerialName("keywords") val keywords: List<String>,
    /**
     * The tools the run should call, for `tool_call_accuracy`, and the order it should
     * call them in, for `tool_order`. Matched exactly against tool-call span names.
     */
    @SerialName("expected_tools") val expectedTools: List<String>,
    /** Tools the run must not call, for `tool_not_called`. */
    @SerialName("forbidden_tools") val forbiddenTools: List<String>,
    /**
     * The passages the answer should rest on, for the judged scorers that read
     * context. When empty, the outputs of the run's `retrieval` and
     * `rag_vector_operation` spans are used.
     */
    @SerialName("context") val context: List<String>,
    /** Case sensitivity for text comparisons. */
    @SerialName("case_sensitive") val caseSensitive: Boolean = false,
    /** Text the answer must contain, for `contains`. */
    @SerialName("needle") val needle: String? = null,
    /** The expected answer, for `exact_match`. */
    @SerialName("expected") val expected: String? = null,
    /** A budget in milliseconds, for `latency_budget`. */
    @SerialName("max_ms") val maxMs: Double? = null,
    /** A budget in currency units, for `cost_budget`. */
    @SerialName("max_cost") val maxCost: Double? = null,
    /** The most tool calls a healthy run should make, for `tool_efficiency`. */
    @SerialName("max_tool_calls") val maxToolCalls: Int? = null,
    /** The kind of span that must be present in the run, for `span_present`. */
    @SerialName("span_kind") val spanKind: SpanKind? = null,
    /**
     * The span name that must be present in the run, for `span_present`. Matched
     * exactly against `Span.name`.
     */
    @SerialName("span_name") val spanName: String? = null,
    /**
     * The tool that must have run and ended `ok`, for `tool_succeeded`. Matched
     * exactly against the `name` of a span whose kind is `tool_call`,
     * `client_tool_call`, `provider_tool_call` or `mcp_tool_call`.
     */
    @SerialName("tool_name") val toolName: String? = null,
    /** A regular expression the answer must match somewhere, for `matches_pattern`. */
    @SerialName("pattern") val pattern: String? = null,
    /** The shortest acceptable answer in characters, for `answer_length`. */
    @SerialName("min_chars") val minChars: Int? = null,
    /** The longest acceptable answer in characters, for `answer_length`. */
    @SerialName("max_chars") val maxChars: Int? = null,
    /**
     * A JSON Schema the answer must satisfy, for `json_schema`. `type`, `properties`,
     * `required`, `items`, `enum`, `minimum`, `maximum`, `minLength`, `maxLength` and
     * `additionalProperties: false` are checked.
     */
    @SerialName("json_schema") val jsonSchema: JsonElement? = null
) {
}

/**
 * A request to run scorers over runs that are already stored. Scoring reads stored
 * runs rather than running inline, so an evaluation can never add latency to the
 * request it is judging, and a scorer that fails can never fail a user's request.
 */
@Serializable
data class ScoreRunRequest(
    /** Score exactly these runs. Takes precedence over the window. */
    @SerialName("trace_ids") val traceIds: List<String>,
    /**
     * Which scorers to run, by id. Empty means every scorer that needs no
     * configuration.
     */
    @SerialName("scorers") val scorers: List<String>,
    /** Report what would happen and store nothing. */
    @SerialName("dry_run") val dryRun: Boolean = false,
    /** Which project's runs to score. Defaults to the server's default project. */
    @SerialName("project") val project: String? = null,
    /** Otherwise, score runs started after this time. */
    @SerialName("from") val from: Instant? = null,
    /**
     * Most runs to score in one request, newest first, when `trace_ids` is empty.
     * Unset, it is 100 for deterministic scorers, and for a pass with a judged scorer
     * it is as many runs as fit in the request deadline. A judged pass that names more
     * runs than fit, through `limit` or `trace_ids`, is refused with the cap.
     */
    @SerialName("limit") val limit: Int? = null,
    /** Settings for the scorers that need them. */
    @SerialName("config") val config: ScorerConfig? = null,
    /** What to record as the origin of these scores. */
    @SerialName("source") val source: ScoreSource? = null
) {
}

/** A scorer that declined to answer on one run. */
@Serializable
data class SkippedScorer(
    /** Which scorer declined. */
    @SerialName("scorer_id") val scorerId: String,
    /** Which run it declined on. */
    @SerialName("trace_id") val traceId: String,
    /**
     * Why. Never silent: a scorer that cannot answer is more interesting than one that
     * returned a number.
     */
    @SerialName("reason") val reason: String
) {
}

/** What one scoring pass did. */
@Serializable
data class ScoreRunResult(
    /** Runs looked at. */
    @SerialName("traces_scored") val tracesScored: Int,
    /** Scores produced and stored. */
    @SerialName("scores_written") val scoresWritten: Int,
    /** Every scorer that declined, with its reason. */
    @SerialName("skipped") val skipped: List<SkippedScorer>,
    /** The scores themselves, so the caller sees them without a second read. */
    @SerialName("scores") val scores: List<Score>
) {
}

/** How one scorer moved between two experiments. */
@Serializable
data class ScorerDelta(
    /** Which scorer. */
    @SerialName("scorer_id") val scorerId: String,
    /** Number of baseline observations. */
    @SerialName("baseline_n") val baselineN: Long,
    /** Number of candidate observations. */
    @SerialName("candidate_n") val candidateN: Long,
    /**
     * Whether the sample is large enough for the delta to mean anything. A
     * deliberately blunt rule rather than a significance test: below twenty
     * observations on either side, mcpd says the comparison is not decidable rather
     * than dressing noise up as a finding.
     */
    @SerialName("decidable") val decidable: Boolean,
    /**
     * Which way this scorer's value is better, from the catalogue. Scorers the daemon
     * does not know read as higher is better.
     */
    @SerialName("direction") val direction: ScoreDirection,
    /**
     * Whether this scorer moved the wrong way: decidable, and the whole interval on
     * the worse side of zero.
     */
    @SerialName("regressed") val regressed: Boolean = false,
    /** Mean on the baseline, when it ran there. */
    @SerialName("baseline_mean") val baselineMean: Double? = null,
    /** Mean on the candidate. */
    @SerialName("candidate_mean") val candidateMean: Double? = null,
    /** Candidate minus baseline. */
    @SerialName("delta") val delta: Double? = null,
    /**
     * Lower end of the 95% confidence interval on `delta`, from Welch's t-test. Absent
     * when either side has fewer than two observations.
     */
    @SerialName("ci_low") val ciLow: Double? = null,
    /** Upper end of that interval. */
    @SerialName("ci_high") val ciHigh: Double? = null,
    /** Two-sided p-value for the difference in means. */
    @SerialName("p_value") val pValue: Double? = null
) {
}

/**
 * Two experiments, compared. The point of an experiment is the comparison, so this
 * reports the delta per scorer beside the sample sizes. A mean that moved on four
 * runs is noise, and printing the counts next to it is what stops it being read as
 * a result.
 */
@Serializable
data class ExperimentComparison(
    /** The baseline's identifier. */
    @SerialName("baseline") val baseline: String,
    /** The candidate's identifier. */
    @SerialName("candidate") val candidate: String,
    /** One row per scorer present on either side. */
    @SerialName("scorers") val scorers: List<ScorerDelta>,
    /** Whether any decidable scorer moved down. */
    @SerialName("regressed") val regressed: Boolean
) {
}

/** A request to turn stored runs into dataset cases. */
@Serializable
data class CaptureRequest(
    /**
     * The runs to capture. Each becomes one item, keeping its trace id so anyone can
     * go back and read what actually happened.
     */
    @SerialName("trace_ids") val traceIds: List<String>,
    /**
     * Record each run's own output as the expected answer. Leave this off for a run
     * that went wrong: capturing a bad answer as the reference would enshrine the bug.
     */
    @SerialName("use_output_as_expected") val useOutputAsExpected: Boolean = false
) {
}

/** What a capture added to a dataset. */
@Serializable
data class CaptureResult(
    /** Items added. */
    @SerialName("captured") val captured: Int,
    /** Runs asked for. A gap means a run was not found. */
    @SerialName("requested") val requested: Int
) {
}

/** How many cases a dataset gained. */
@Serializable
data class ItemsAdded(
    /** Items added. */
    @SerialName("added") val added: Int
) {
}

/**
 * What an erasure request removed, counted per signal. The subject is not echoed
 * back: it has been erased, and repeating it in a response that may be logged on
 * the way out would write it down again.
 */
@Serializable
data class ErasureResult(
    /** The project the erasure ran in. No other project was read or touched. */
    @SerialName("project") val project: String,
    /** How many runs were removed. */
    @SerialName("traces") val traces: Int,
    /** How many spans were removed, with the prompts and completions they carried. */
    @SerialName("spans") val spans: Int,
    /** How many log lines were removed. */
    @SerialName("logs") val logs: Int,
    /** How many metric points were removed. */
    @SerialName("metrics") val metrics: Int,
    /** How many scores were removed. */
    @SerialName("scores") val scores: Int,
    /** How many pieces of feedback were removed. */
    @SerialName("feedback") val feedback: Int,
    /**
     * The total across every signal, so a caller that wants one number does not add
     * them up.
     */
    @SerialName("deleted") val deleted: Int,
    /** The datasets those emptied cases sit in, so the reply can say where they are. */
    @SerialName("datasets") val datasets: List<String>,
    /**
     * How many durable runs were removed from the run store, each with its step
     * attempts, event log, deadlines, waits, the events it consumed and any span still
     * queued for the trace store. A run is matched on the `user_id` in its
     * correlation, as it was started with it, and removed with the whole tree of
     * subworkflow runs it belongs to. Not part of `deleted`, which counts the trace
     * store. Absent when this daemon does not serve runs.
     */
    @SerialName("runs_erased") val runsErased: Int? = null,
    /**
     * How many runs attributed to this person were left untouched because a run in
     * their tree has not ended. Removing a live run would corrupt the workflow it is
     * part of, so these are counted rather than skipped in silence, and `note` names
     * the root runs. Cancel them, or let them end, then send the erasure again. Absent
     * when this daemon does not serve runs.
     */
    @SerialName("runs_in_flight") val runsInFlight: Int? = null,
    /**
     * How many aspect notes were removed, matched by the run they describe and by the
     * person stamped on them. Not part of `deleted`. Absent only from a store that
     * holds no notes.
     */
    @SerialName("aspect_notes_erased") val aspectNotesErased: Int? = null,
    /**
     * How many dataset cases captured from an erased run were emptied in place. They
     * are not deleted, because a case was kept on purpose, so they are not part of
     * `deleted`.
     */
    @SerialName("dataset_items_erased") val datasetItemsErased: Int? = null,
    /**
     * True when the storage backend applies the deletes after this reply is sent, as
     * ClickHouse does, so the counts say what matched rather than what has already
     * left the disk.
     */
    @SerialName("deletes_pending") val deletesPending: Boolean? = null,
    /**
     * How many stored identifiers this request could not have matched, because they
     * were written in the other form, hashed or plain, from the one given. A number
     * above zero means the counts may not be the whole answer.
     */
    @SerialName("unmatchable_identifiers") val unmatchableIdentifiers: Int? = null,
    /**
     * Why the numbers above may not be the whole answer, in sentences an operator can
     * repeat. Absent when they are the whole answer.
     */
    @SerialName("note") val note: String? = null
) {
}

/**
 * A definition as you post it: the graph, and the id and version to store it
 * under. The body is validated at write time against
 * `spec/definition.schema.json`; a body the walker cannot run is refused with a
 * 400 naming the field, and unknown fields are refused, so a guard that is spelled
 * wrong is an error and not a run that skips its guard.
 */
@Serializable
data class DefinitionDocument(
    /** The definition's identifier within its project. Matches `^[a-z][a-z0-9_]*$`. */
    @SerialName("id") val id: String,
    /**
     * The document itself: `input_schema`, `limits`, `steps` and `output`. The
     * definition schema names every field.
     */
    @SerialName("body") val body: JsonElement,
    /**
     * What kind of definition. Absent means `workflow`, the only kind version 1
     * serves.
     */
    @SerialName("kind") val kind: DefinitionKind? = null,
    /**
     * The version to store the body under. Absent asks the daemon to choose: the
     * version whose stored body is identical, or one past the highest stored. A named
     * version that is free is taken; one held by an identical body is confirmed; one
     * held by a different body is a 409, because a stored version never changes.
     */
    @SerialName("version") val version: Int? = null
) {
}

/** Where a posted definition landed. */
@Serializable
data class DefinitionVersion(
    /** The definition's identifier. */
    @SerialName("id") val id: String,
    /** The version the body is stored under. */
    @SerialName("version") val version: Int,
    /**
     * True when the daemon chose the number because the body was new. False when the
     * body already existed at this version, or you named the version yourself.
     */
    @SerialName("assigned") val assigned: Boolean
) {
}

/**
 * The frontier of a run: which position in the definition's `steps` it has
 * reached, and what sits there.
 */
@Serializable
data class RunCursor(
    /** The index into `steps`, or their count at the end of the graph. */
    @SerialName("index") val index: Int,
    /** What is at that position. */
    @SerialName("construct") val construct: ConstructKind,
    /**
     * The step this position belongs to, when the construct has one. A fan-out names
     * its parent step; the branch and item keys are on the attempts.
     */
    @SerialName("step_key") val stepKey: String? = null
) {
}

/** A deadline the daemon holds for a run. */
@Serializable
data class RunTimer(
    /** Which run. */
    @SerialName("run_id") val runId: String,
    /** Which deadline. */
    @SerialName("kind") val kind: TimerKind,
    /** When it fires. */
    @SerialName("due_at") val dueAt: Instant,
    /** The step the deadline belongs to. Absent for a run-level deadline. */
    @SerialName("step_key") val stepKey: String? = null
) {
}

/** A suspension waiting for an event. */
@Serializable
data class RunWait(
    /** Which run. */
    @SerialName("run_id") val runId: String,
    /** The `wait_event` step that is waiting. */
    @SerialName("step_key") val stepKey: String,
    /** The event name an inbound event must match. */
    @SerialName("event") val event: String,
    /** The rendered key that picks this run out of every run waiting on that event. */
    @SerialName("key") val key: String,
    /** When waiting stops and the node's `on_timeout` applies. */
    @SerialName("expires_at") val expiresAt: Instant
) {
}

/** One run and everything the daemon holds for it. */
@Serializable
data class RunDetail(
    /** The run. */
    @SerialName("run") val run: Run,
    /** Every attempt, in the order they were written. */
    @SerialName("attempts") val attempts: List<StepAttempt>,
    /** The deadlines currently held. Empty once the run has ended. */
    @SerialName("timers") val timers: List<RunTimer>,
    /** The suspensions currently open. Empty once the run has ended. */
    @SerialName("waits") val waits: List<RunWait>
) {
}

/** A page of runs. */
@Serializable
data class RunListResponse(
    /** The matching runs, newest first. */
    @SerialName("runs") val runs: List<Run>,
    /** Cursor and totals. */
    @SerialName("page") val page: Page
) {
}

/** A page of a run's event log. */
@Serializable
data class RunEventPage(
    /** The rows after the requested sequence number, in order. */
    @SerialName("events") val events: List<RunEvent>,
    /**
     * Whether rows beyond the last one here existed when the page was read. A run that
     * is still moving will write more.
     */
    @SerialName("has_more") val hasMore: Boolean
) {
}

/**
 * One edge of a cause chain: an ancestor step, and the template path through which
 * the step below it read that ancestor's output. An ancestor read through two
 * paths appears twice, once per path.
 */
@Serializable
data class CauseLink(
    /**
     * The ancestor step, keyed the way the journal keys it: `refund/issue`,
     * `classify[3]`, `poll[7]/classify`.
     */
    @SerialName("step_key") val stepKey: String,
    /** Where the ancestor's latest attempt ended. */
    @SerialName("status") val status: AttemptStatus,
    /**
     * How many attempts were made at the ancestor. More than one on a step that ended
     * `succeeded` is worth reading: it got there, but not the first time.
     */
    @SerialName("attempts") val attempts: Int,
    /**
     * The path as the definition writes it inside `${...}`, such as
     * `steps.classify.output.kind`. The definition declares this edge, so the chain is
     * read rather than inferred from timing.
     */
    @SerialName("template_path") val templatePath: String,
    /**
     * The step key that read this ancestor through `template_path`. On a direct edge
     * it is the step the answer is about; deeper in the chain it is another ancestor.
     */
    @SerialName("consumed_by") val consumedBy: String,
    /**
     * How far this ancestor sits from the step the answer is about. `1` is read
     * directly, `2` is read by something the step read directly, and so on.
     */
    @SerialName("depth") val depth: Int,
    /**
     * True when this ancestor ended `succeeded` and every step between it and the
     * failed step ended `succeeded` too. Nothing on that path is marked failed, so no
     * status code points at it and the value it produced is the suspect. It is not the
     * same as `status` being `succeeded`: an ancestor sitting behind a step that did
     * fail is not a silent producer, because that failure already explains the run.
     */
    @SerialName("silent_producer") val silentProducer: Boolean,
    /**
     * The JSON type `template_path` resolved to on the ancestor's stored output. The
     * value itself is never returned, only its shape. Absent when there is no output
     * to read, which is the case for a step that never ran and for one that failed.
     */
    @SerialName("value_kind") val valueKind: ValueKind? = null,
    /**
     * True when that value was null, an empty string, an empty array or an empty
     * object. A step that ended `succeeded` and handed the step below it nothing is
     * the failure a status code cannot catch, and this is the field that shows it.
     * Absent for the same reason `value_kind` is.
     */
    @SerialName("value_empty") val valueEmpty: Boolean? = null
) {
}

/**
 * Why one step of a run failed, answered by walking backwards along the data
 * dependencies its definition declares. The daemon reads the run journal and the
 * pinned definition version, both of which it has to hold to render `${...}`
 * templates at all, so this answer is available with span payload capture off and
 * with no model, prompt or completion involved.
 */
@Serializable
data class StepCause(
    /** The run this answer is about. */
    @SerialName("run_id") val runId: String,
    /** The step this answer is about, as it was asked for. */
    @SerialName("step_key") val stepKey: String,
    /** Where that step's latest attempt ended. */
    @SerialName("status") val status: AttemptStatus,
    /** How many attempts were made at it. */
    @SerialName("attempts") val attempts: Int,
    /**
     * The ancestors whose output this step consumed, directly or transitively. Ordered
     * by `depth` ascending, then by the order the paths appear in the definition, so
     * two calls against one run return the same list in the same order. Empty when the
     * step reads nothing but the run input.
     */
    @SerialName("chain") val chain: List<CauseLink>,
    /**
     * True when the walk stopped at `max_depth` with ancestors left unvisited, so
     * `chain` is a prefix of the answer rather than the whole of it.
     */
    @SerialName("truncated") val truncated: Boolean,
    /**
     * Why the latest attempt failed, when it did. Absent on a step that did not fail,
     * because the endpoint answers for any step, not only a failed one.
     */
    @SerialName("error") val error: StepError? = null
) {
}

/** A request to start a run. */
@Serializable
data class StartRunRequest(
    /** Which definition to run. */
    @SerialName("definition_id") val definitionId: String,
    /**
     * Which project. A selector: refused with 403 on a mismatch with the key's grant,
     * filled in for a single-project key, and a 400 when a multi-project key names
     * none.
     */
    @SerialName("project") val project: String? = null,
    /**
     * Which version. Absent pins the highest stored version at this moment, and the
     * run records it.
     */
    @SerialName("version") val version: Int? = null,
    /**
     * The run's input. Validated against the definition's `input_schema` when it
     * declares one; a failure is a 400.
     */
    @SerialName("input") val input: JsonElement? = null,
    /**
     * Context to store on the run and copy onto every span the daemon writes for it.
     * `project` and `run_id` inside it are ignored: the run's own values win.
     */
    @SerialName("correlation") val correlation: Correlation? = null,
    /**
     * Makes the start idempotent per project for 24 hours: a repost returns the same
     * run and starts nothing, and two simultaneous posts of one key produce one run.
     */
    @SerialName("idempotency_key") val idempotencyKey: String? = null
) {
}

/** A worker asking for work. */
@Serializable
data class ClaimRequest(
    /**
     * Who is asking. Recorded on the attempt as `lease_owner` and in the run's event
     * log. Nothing else about the worker is stored.
     */
    @SerialName("worker_id") val workerId: String,
    /** The most leases to hand back, 1 to 100. */
    @SerialName("max") val max: Int,
    /**
     * How long to hold the request open waiting for work, 0 to 60000 milliseconds. The
     * daemon holds a claim for at most four fifths of its request timeout, 24000 at
     * the default 30 seconds, so a longer wait is answered early rather than cut off
     * with a 503. An empty answer means nothing became runnable in time: reissue the
     * claim.
     */
    @SerialName("wait_ms") val waitMs: Int,
    /**
     * Which project. A selector: refused with 403 on a mismatch with the key's grant,
     * filled in for a single-project key, and a 400 when a multi-project key names
     * none.
     */
    @SerialName("project") val project: String? = null,
    /** Restrict the claim to one run, which is how a script drives its own run inline. */
    @SerialName("run_id") val runId: String? = null
) {
}

/**
 * A step handed to a worker. Every later call about the attempt is addressed by
 * `lease_id`.
 */
@Serializable
data class StepLease(
    /** The address of every later call about this attempt. */
    @SerialName("lease_id") val leaseId: String,
    /** Which run. */
    @SerialName("run_id") val runId: String,
    /** Which project the run belongs to. */
    @SerialName("project") val project: String,
    /** Which definition the run is executing. */
    @SerialName("definition_id") val definitionId: String,
    /** Which version. Fail the lease with class `unservable` if you do not serve it. */
    @SerialName("definition_version") val definitionVersion: Int,
    /**
     * The step to run. A worker that does not recognise it fails the lease with class
     * `unservable` rather than guessing.
     */
    @SerialName("step_key") val stepKey: String,
    /** Whether this is a plain task or an agent turn. */
    @SerialName("kind") val kind: StepKind,
    /**
     * Which attempt this is. The fence: a call about this lease is refused once a
     * later attempt exists.
     */
    @SerialName("attempt") val attempt: Int,
    /**
     * Under `at_most_once`, post `started` and wait for its 200 before running the
     * body.
     */
    @SerialName("effect") val effect: Effect,
    /** When the lease lapses. Heartbeat every `heartbeat_ms` to keep it. */
    @SerialName("expires_at") val expiresAt: Instant,
    /**
     * A key stable across every attempt of this step: the SHA-256 of
     * `run_id:step_key`. Pass it to any downstream call that can deduplicate.
     */
    @SerialName("idempotency_key") val idempotencyKey: String,
    /**
     * The ended attempts of earlier steps, keyed by step id: `{ status, output?,
     * error? }` for the latest attempt of each, or `{ status: skipped }`. Only ended
     * attempts appear.
     */
    @SerialName("prior") val prior: JsonElement,
    /**
     * The run's correlation, with `run_id` set. Put it on every span you record inside
     * this step.
     */
    @SerialName("correlation") val correlation: Correlation,
    /** The run's trace. Set it on every span you record inside this step. */
    @SerialName("trace_id") val traceId: String,
    /**
     * The span the daemon has already opened for this attempt. Set it as
     * `parent_span_id` on your own spans, so a model call or a tool call nests under
     * the step.
     */
    @SerialName("span_id") val spanId: String,
    /** The run's root span, which this attempt's span hangs off. */
    @SerialName("parent_span_id") val parentSpanId: String,
    /**
     * The step's own timeout, when the node declares one. It bounds the attempt
     * whatever the heartbeats say.
     */
    @SerialName("deadline_ms") val deadlineMs: Long? = null,
    /**
     * The node's `input` template rendered for this attempt, or the run's input when
     * the node declares none.
     */
    @SerialName("input") val input: JsonElement? = null,
    /** The run's state object as it stands. */
    @SerialName("state") val state: JsonElement? = null
) {
}

/** What a claim answered. */
@Serializable
data class ClaimResponse(
    /** Zero to `max` leases. Empty when nothing became runnable within `wait_ms`. */
    @SerialName("leases") val leases: List<StepLease>
) {
}

/** What `started` and `heartbeat` answer. */
@Serializable
data class LeaseState(
    /** The lease's new expiry. */
    @SerialName("expires_at") val expiresAt: Instant,
    /**
     * True when this call found the attempt `orphaned` and restored it to the state it
     * came from.
     */
    @SerialName("reclaimed") val reclaimed: Boolean
) {
}

/**
 * Token usage a worker reports on `complete`. The daemon prices it from its price
 * book and charges the attempt, the run and the root of the run tree.
 */
@Serializable
data class StepUsage(
    /** Total input tokens. */
    @SerialName("input_tokens") val inputTokens: Long? = null,
    /** Total output tokens. */
    @SerialName("output_tokens") val outputTokens: Long? = null,
    /** Input plus output, when the provider reports a total that differs from the sum. */
    @SerialName("total_tokens") val totalTokens: Long? = null,
    /** How the input tokens were billed. */
    @SerialName("input_details") val inputDetails: InputTokenDetails? = null,
    /** How the output tokens were billed. */
    @SerialName("output_details") val outputDetails: OutputTokenDetails? = null,
    /** The model that produced the usage, which the price book needs to price it. */
    @SerialName("model") val model: String? = null,
    /** The provider that served the model. */
    @SerialName("provider") val provider: String? = null
) {
}

/** What a worker posts to complete a step. */
@Serializable
data class StepResult(
    /** The step's output. Later steps read it as `steps.<id>.output`. */
    @SerialName("output") val output: JsonElement? = null,
    /** An object to merge shallowly into the run's state. */
    @SerialName("state") val state: JsonElement? = null,
    /** Token usage to price and charge. */
    @SerialName("usage") val usage: StepUsage? = null
) {
}

/** Why a step failed, as the worker reports it. */
@Serializable
data class StepError(
    /** What kind of failure. Decides whether the daemon retries. */
    @SerialName("class") val `class`: StepErrorClass,
    /** What went wrong, for the person reading the run. */
    @SerialName("message") val message: String,
    /** Structured detail. Stored on the attempt. */
    @SerialName("details") val details: JsonElement? = null,
    /**
     * The least the daemon should wait before the next attempt, when the worker knows:
     * a rate limit's reset, say. Honoured when it is larger than the backoff. At most
     * 315360000000 (3650 days), the bound on every duration in a definition; a larger
     * value is a 400 naming this field.
     */
    @SerialName("retry_after_ms") val retryAfterMs: Long? = null
) {
}

/** What a worker posts to fail a step. */
@Serializable
data class StepFailure(
    /** Why it failed. */
    @SerialName("error") val error: StepError
) {
}

/** What `complete` and `fail` answer. */
@Serializable
data class StepAck(
    /** Where the run is after this call. */
    @SerialName("run_status") val runStatus: RunStatus,
    /** `claim` when more work is runnable now, `idle` otherwise. */
    @SerialName("next") val next: String,
    /** After a `fail`: how many attempts the node's retry budget still allows. */
    @SerialName("attempts_left") val attemptsLeft: Int? = null
) {
}

/**
 * The body of a 409 from a worker call. `reason` says what is true and what the
 * worker should do next.
 */
@Serializable
data class StepConflict(
    /** Always `conflict`. */
    @SerialName("error") val error: String,
    /** Which conflict. */
    @SerialName("reason") val reason: ConflictReason,
    /** The reason, in words. */
    @SerialName("message") val message: String,
    /** The attempt the lease addressed. */
    @SerialName("attempt") val attempt: Int,
    /** Where the run is. */
    @SerialName("run_status") val runStatus: RunStatus,
    /**
     * Where that attempt is, when it still exists. Under `already_applied` this says
     * how the step ended.
     */
    @SerialName("attempt_status") val attemptStatus: AttemptStatus? = null
) {
}

/** An event for whichever run is waiting for it. */
@Serializable
data class PublishEventRequest(
    /** The event name, matched against open waits. */
    @SerialName("event") val event: String,
    /**
     * Matched against the rendered `key` of each waiting run. Delivered to the
     * matching run whose wait expires soonest.
     */
    @SerialName("key") val key: String,
    /**
     * Which project. A selector: refused with 403 on a mismatch with the key's grant,
     * filled in for a single-project key, and a 400 when a multi-project key names
     * none.
     */
    @SerialName("project") val project: String? = null,
    /** Becomes the waiting step's output. */
    @SerialName("payload") val payload: JsonElement? = null,
    /**
     * Your own id for this event. A supplied id seen before answers `duplicate` and
     * writes nothing, whether or not a run is waiting. Absent, the daemon derives one
     * from `project`, `event`, `key` and `payload`, and a derived id counts as a
     * duplicate only when no run is waiting, because two identical approvals for two
     * runs on one key are two events. Send your own id from any handler that can be
     * retried.
     */
    @SerialName("event_id") val eventId: String? = null
) {
}

/** What publishing an event did. */
@Serializable
data class EventReceipt(
    /** Delivered, parked or duplicate. */
    @SerialName("outcome") val outcome: EventOutcome,
    /** The id used, supplied or derived. */
    @SerialName("event_id") val eventId: String
) {
}

/**
 * An event for one run, addressed by run id rather than by key. The daemon reads
 * the run's open wait on `event`, takes its key, and routes the event as a
 * published one.
 */
@Serializable
data class SignalRequest(
    /** The event name. The run must hold an open wait on it. */
    @SerialName("event") val event: String,
    /** Becomes the waiting step's output. */
    @SerialName("payload") val payload: JsonElement? = null,
    /** Your own id for this event, deduplicated as on a published event. */
    @SerialName("event_id") val eventId: String? = null
) {
}

/** A worker announcing the definitions it serves. */
@Serializable
data class WorkerRegistration(
    /**
     * Who is registering. Not stored: dispatch reads the runnable set, never a worker
     * table.
     */
    @SerialName("worker_id") val workerId: String,
    /**
     * Each is stored under the versioning rule, exactly as if posted to the
     * definitions endpoint.
     */
    @SerialName("definitions") val definitions: List<DefinitionDocument>,
    /**
     * Which project. A selector: refused with 403 on a mismatch with the key's grant,
     * filled in for a single-project key, and a 400 when a multi-project key names
     * none.
     */
    @SerialName("project") val project: String? = null
) {
}

/** What registering answered. */
@Serializable
data class WorkerRegistered(
    /** The id you registered with. */
    @SerialName("worker_id") val workerId: String,
    /** Where each definition landed, in the order posted. */
    @SerialName("registered") val registered: List<DefinitionVersion>,
    /** How long a lease lasts without a heartbeat. */
    @SerialName("lease_ttl_ms") val leaseTtlMs: Long,
    /** How often to heartbeat a held lease: a third of `lease_ttl_ms`. */
    @SerialName("heartbeat_ms") val heartbeatMs: Long
) {
}

/**
 * A saved condition the daemon evaluates on a schedule. Monitors are configured in
 * the daemon's configuration file, not through the API.
 */
@Serializable
data class Monitor(
    /** Stable identifier. */
    @SerialName("id") val id: String,
    /** What a person should read when it trips. */
    @SerialName("name") val name: String,
    /** Which project it watches. */
    @SerialName("project") val project: String,
    /** What it measures. */
    @SerialName("watch") val watch: MonitorWatch,
    /** Which direction trips it. */
    @SerialName("compare") val compare: MonitorCompare,
    /** The threshold. */
    @SerialName("threshold") val threshold: Double,
    /** How far back each evaluation looks. */
    @SerialName("window_seconds") val windowSeconds: Int = 300,
    /**
     * The fewest runs the window must contain before the monitor will trip. Without it
     * a single failed run in a quiet hour is a 100% error rate, and the first thing
     * anyone does with an alert that cries wolf is mute it.
     */
    @SerialName("min_samples") val minSamples: Long = 20,
    /** Whether it is evaluated at all. */
    @SerialName("enabled") val enabled: Boolean = true,
    /** Which scorer, when `watch` is `score_mean`. */
    @SerialName("scorer_id") val scorerId: String? = null,
    /**
     * Where to POST when it trips or clears. Set as a full URL in the config file; the
     * API returns only its host, because a webhook URL is often its own credential.
     */
    @SerialName("webhook") val webhook: String? = null
) {
}

/** One evaluation of a monitor. */
@Serializable
data class MonitorReading(
    /** Which monitor. */
    @SerialName("monitor_id") val monitorId: String,
    /** When it was evaluated. */
    @SerialName("at") val at: Instant,
    /** How many runs the window held. */
    @SerialName("samples") val samples: Long,
    /** The resulting condition. */
    @SerialName("condition") val condition: MonitorCondition,
    /** What a person should read. */
    @SerialName("detail") val detail: String,
    /** The measured value, when there was enough data to measure one. */
    @SerialName("value") val value: Double? = null
) {
}

/** A monitor and the condition it is latched in. */
@Serializable
data class MonitorState(
    /** The monitor. */
    @SerialName("monitor") val monitor: Monitor,
    /** Its current condition. */
    @SerialName("condition") val condition: MonitorCondition,
    /** The most recent reading. Absent before the first evaluation. */
    @SerialName("last") val last: MonitorReading? = null,
    /** When it first tripped, while it stays tripped. */
    @SerialName("tripped_at") val trippedAt: Instant? = null
) {
}

/**
 * A service level objective: the fraction of runs that must succeed over a window.
 * A monitor asks whether a number is over a line right now. An objective asks
 * whether the promise is going to be missed and how fast the error budget is
 * burning, over two windows at once, so a two minute blip and a slow leak get the
 * different answers they need. Objectives are configured in the daemon's
 * configuration file, not through the API.
 */
@Serializable
data class Slo(
    /** Stable identifier. */
    @SerialName("id") val id: String,
    /** What a person should read. */
    @SerialName("name") val name: String,
    /** Which project it covers. */
    @SerialName("project") val project: String,
    /**
     * The fraction of runs that must succeed, between 0 and 1. `0.995` is ninety-nine
     * and a half percent of runs.
     */
    @SerialName("objective") val objective: Double,
    /** How long the promise is measured over. */
    @SerialName("window_days") val windowDays: Int = 30,
    /** The fewest runs a burn window must hold before it may trip. */
    @SerialName("min_samples") val minSamples: Long = 20,
    /** Whether it is evaluated at all. */
    @SerialName("enabled") val enabled: Boolean = true,
    /**
     * Where to POST on a transition. Set as a full URL in the config file; the API
     * returns only its host, because a webhook URL is often its own credential.
     */
    @SerialName("webhook") val webhook: String? = null
) {
}

/** One evaluation of an objective. */
@Serializable
data class SloReading(
    /** Which objective. */
    @SerialName("slo_id") val sloId: String,
    /** When it was evaluated. */
    @SerialName("at") val at: Instant,
    /** Runs in the whole window. */
    @SerialName("samples") val samples: Long,
    /** The resulting condition. */
    @SerialName("condition") val condition: MonitorCondition,
    /** What a person should read. */
    @SerialName("detail") val detail: String,
    /** The observed success rate over the whole window. */
    @SerialName("success_rate") val successRate: Double? = null,
    /**
     * The fraction of the error budget still unspent. Negative once it is overspent,
     * which says by how much the promise was missed.
     */
    @SerialName("budget_remaining") val budgetRemaining: Double? = null,
    /**
     * The burn rate over the one hour window. A rate of 1 spends the whole budget
     * exactly over the whole window.
     */
    @SerialName("burn_fast") val burnFast: Double? = null,
    /**
     * The burn rate over the six hour window. Both windows must exceed their trip rate
     * together before the objective trips, which is what stops a brief spike paging
     * anyone.
     */
    @SerialName("burn_slow") val burnSlow: Double? = null
) {
}

/** An objective and the condition it is latched in. */
@Serializable
data class SloState(
    /** The objective. */
    @SerialName("slo") val slo: Slo,
    /** Its current condition. */
    @SerialName("condition") val condition: MonitorCondition,
    /** The most recent reading. Absent before the first evaluation. */
    @SerialName("last") val last: SloReading? = null,
    /** When it first tripped, while it stays tripped. */
    @SerialName("tripped_at") val trippedAt: Instant? = null
) {
}

/**
 * What the storage backend behind this daemon can and cannot do. Three backends
 * implement one storage trait and they are not equal: a reader on ClickHouse who
 * searches for `CAFE` with an acute and finds nothing has no way to learn that
 * this backend folds ASCII case and no more. Read this once at connect time and
 * offer only what the deployment answers. Every field is named after the
 * capability an endpoint names when it refuses with 501, so the two can be
 * compared without translation.
 */
@Serializable
data class StoreCapabilities(
    /**
     * Which backend this describes: `sqlite`, `postgres` or `clickhouse`. The same
     * string `/healthz` reports as `storage`.
     */
    @SerialName("backend") val backend: String,
    /** What the `q` parameter matches. */
    @SerialName("text_search") val textSearch: TextSearch,
    /**
     * Whether `q` folds case for letters outside ASCII. False means a search for
     * `CAFE` with an acute does not find the lowercase form, which a reader otherwise
     * meets as an empty result they cannot explain.
     */
    @SerialName("unicode_case_folding") val unicodeCaseFolding: Boolean,
    /**
     * Whether a delete is on disk when the call returns. False means the backend
     * accepted the delete and applies it afterwards, so a count says what matched
     * rather than what has already gone.
     */
    @SerialName("immediate_deletes") val immediateDeletes: Boolean,
    /**
     * Whether `GET /v1/api/discovery` answers. False means a filter box has to be
     * filled from what is already on screen.
     */
    @SerialName("discovery") val discovery: Boolean,
    /**
     * Whether the `metric_label_value` facet is answerable, not only the keys.
     * Separate because the values are the expensive half: a key set is small and
     * bounded, and a value set is not.
     */
    @SerialName("discovery_label_values") val discoveryLabelValues: Boolean,
    /**
     * Whether `GET /v1/api/deltas` answers, so a list refreshes from a cursor rather
     * than by running the whole query again.
     */
    @SerialName("delta_polling") val deltaPolling: Boolean,
    /**
     * Whether retention can carry a different window per signal. False means one
     * window covers all five.
     */
    @SerialName("per_signal_retention") val perSignalRetention: Boolean,
    /**
     * Whether the window can be handed to the database engine, so nothing has to run a
     * delete on a timer.
     */
    @SerialName("declarative_retention") val declarativeRetention: Boolean,
    /** How the signal tables are cut on their anchor time. */
    @SerialName("partitioning") val partitioning: Partitioning,
    /**
     * Whether a retention pass can drop a whole partition instead of deleting rows one
     * at a time.
     */
    @SerialName("partition_drop_retention") val partitionDropRetention: Boolean,
    /**
     * Whether a start with nothing to apply issues no schema statement at all. False
     * means every process replays the schema on every boot, which is harmless on one
     * node and is metadata churn across a replicated deployment.
     */
    @SerialName("idempotent_schema_init") val idempotentSchemaInit: Boolean,
    /**
     * Whether this deployment answers the Intelligence endpoints. False means the
     * store holds no note or theme tables and every one of those routes refuses with
     * 501 naming this capability, so a client decides once whether to offer the
     * feature rather than showing a reader an empty page they cannot account for.
     * Every backend that answers gives the same themes: the clustering runs in the
     * daemon over stored notes and not in SQL, so there is nothing here for a backend
     * to do differently.
     */
    @SerialName("theme_analysis") val themeAnalysis: Boolean
) {
}

/** One value a filter dimension takes in a project. */
@Serializable
data class DiscoveredValue(
    /**
     * The value, exactly as it is stored, so it can be sent straight back as a filter
     * with no further mapping.
     */
    @SerialName("value") val value: String,
    /**
     * When mcpd last received a signal carrying it. This is arrival time and not the
     * signal's own timestamp, so a backfill of last year's data does not make a value
     * look current. Carried so a caller can rank down or grey out a value nothing has
     * reported for a month. Values are ordered by `value` and not by this: a list
     * somebody reads alphabetically is stable between polls, and one ordered by
     * recency reshuffles under them as data arrives.
     */
    @SerialName("last_seen") val lastSeen: Instant
) {
}

/**
 * The values one filter dimension takes, for a filter box. Answered from a
 * distinct-value list the daemon maintains as signals arrive, not by scanning the
 * signal tables, because the question is asked on every keystroke and the tables
 * it would otherwise scan are the largest thing the deployment holds.
 */
@Serializable
data class DiscoveryResponse(
    /** Which dimension was asked for. */
    @SerialName("facet") val facet: Facet,
    /** The values, ordered by value, ascending. */
    @SerialName("values") val values: List<DiscoveredValue>,
    /**
     * The label key the values belong to, echoed back for `metric_label_value`. Absent
     * for every other facet.
     */
    @SerialName("key") val key: String? = null
) {
}

/**
 * What has arrived since a cursor. A poll is not a page: paging walks a fixed
 * result set backwards in the sort you chose, and polling walks forwards in
 * arrival order and never ends, so the two cursors are different things and are
 * deliberately not interchangeable. Rows come back oldest arrival first, so
 * applying them in order to a list you already hold gives the same result as
 * having listed afresh.
 */
@Serializable
data class DeltaResponse(
    /**
     * Which signal these rows are. Exactly one of the three arrays below is present,
     * and it is this one.
     */
    @SerialName("signal") val signal: DeltaSignal,
    /**
     * Where the next poll resumes. Always present, and unchanged from the request when
     * nothing came back: moving it forward on an empty poll is what would drop a row
     * that lands a moment later with an earlier arrival time. Treat it as meaningless,
     * and send back the one you were given.
     */
    @SerialName("cursor") val cursor: String,
    /**
     * True when the page filled and more is waiting, so a caller draining a backlog
     * polls again at once rather than waiting for its next tick.
     */
    @SerialName("has_more") val hasMore: Boolean,
    /** The runs that arrived, when `signal` is `traces`. */
    @SerialName("traces") val traces: List<Trace>? = null,
    /** The steps that arrived, when `signal` is `spans`. */
    @SerialName("spans") val spans: List<Span>? = null,
    /** The log lines that arrived, when `signal` is `logs`. */
    @SerialName("logs") val logs: List<LogRecord>? = null
) {
}

/**
 * The bounds a clustering pass runs under. They are published because each one
 * changes what a reader is looking at: a floor that has not been reached means no
 * themes at all rather than an empty result, and a ceiling that has been reached
 * means every share is a share of what was read. They are copied from the
 * divergence watch, which is the one windowed statistic mcpd already ships.
 * `pass_armed` is the first of them to read: the other three describe what a pass
 * would do, and it says whether one runs.
 */
@Serializable
data class AspectLimits(
    /**
     * How many notes one aspect on one entity needs before a pass runs at all, 50 by
     * default. Below it the aspect reads `collecting` and no snapshot is written.
     */
    @SerialName("min_notes_to_cluster") val minNotesToCluster: Long,
    /**
     * The most notes one pass reads, 20,000 by default, taken newest first. A window
     * holding more is clustered from the newest of them and its snapshot is marked
     * `capped`.
     */
    @SerialName("max_notes_per_pass") val maxNotesPerPass: Long,
    /**
     * How many notes the pass reads at a time while it fills a window, 500 by default.
     * It bounds what the pass holds, not what it covers.
     */
    @SerialName("page_size") val pageSize: Int,
    /**
     * How many aspects one project may define, the four built-ins included. It is
     * configuration and is published here so a client that lets an operator add an
     * aspect knows when to stop instead of finding out from a refusal.
     */
    @SerialName("max_aspects") val maxAspects: Int,
    /**
     * Whether this deployment runs the clustering pass at all. False means no snapshot
     * will ever be written however many notes arrive, so every aspect stays
     * `collecting` or `processing` for ever and no theme route will have anything to
     * answer with. It is configuration and not a fault, and it is not the same thing
     * as the 501 the theme routes give: that says the storage backend cannot hold the
     * tables, which nobody can switch on, while this says nobody has switched the pass
     * on yet. It is published here rather than on every progress row because it is one
     * fact about the deployment, and a client reads this route once to learn what the
     * aspects are called.
     */
    @SerialName("pass_armed") val passArmed: Boolean
) {
}

/**
 * Every aspect a project describes traces along, with the bounds the clustering
 * pass runs under. The four built-ins are rows in the same table as anything you
 * define, so there is no fallback list inside a client that can quietly disagree
 * with this one.
 */
@Serializable
data class AspectCatalogue(
    /**
     * The aspects, in `sort_order`. Disabled aspects are included, because notes
     * already recorded under one stay readable and a name that vanished from this list
     * would make them look like a mistake.
     */
    @SerialName("aspects") val aspects: List<Aspect>,
    /** The bounds a pass runs under in this deployment. */
    @SerialName("limits") val limits: AspectLimits
) {
}

/**
 * One entity that has aspect notes, with the roll-up a list needs to be worth
 * reading. A row appears as soon as the first note is recorded, before any theme
 * exists, because an empty screen with a count moving on it is a different thing
 * from an empty screen and a reader has to be able to tell them apart.
 */
@Serializable
data class EntityIndexRow(
    /** The kind of thing this row is about. */
    @SerialName("entity_kind") val entityKind: EntityKind,
    /** Its name. */
    @SerialName("entity_name") val entityName: String,
    /**
     * How many of its traces carry at least one note. This is not the entity's trace
     * count: writing a note lags ingestion, so a busy agent can show far fewer here.
     */
    @SerialName("trace_count") val traceCount: Long,
    /** How many notes are recorded about it. */
    @SerialName("note_count") val noteCount: Long,
    /** How many aspects are switched on for this entity's project. */
    @SerialName("enabled_aspects") val enabledAspects: Int,
    /**
     * How many of those have themes to read. The two numbers read as `2 of 4` and are
     * the honest answer to whether this row is worth opening yet.
     */
    @SerialName("ready_aspects") val readyAspects: Int,
    /**
     * The row's overall state, which is the least advanced of its enabled aspects, so
     * a row says `processing` while any one of them is still short of a snapshot. It
     * never reports the best case.
     */
    @SerialName("readiness") val readiness: AspectReadiness,
    /**
     * When the newest note about this entity was recorded, so a reader can tell a list
     * that is filling from one that stopped.
     */
    @SerialName("last_note_at") val lastNoteAt: Instant,
    /**
     * The cutoff of this entity's newest snapshot. Absent until a pass has written
     * one, which is the ordinary state of a row that is still collecting.
     */
    @SerialName("latest_snapshot_at") val latestSnapshotAt: Instant? = null
) {
}

/** A page of the entities that have aspect notes. */
@Serializable
data class EntityIndexResponse(
    /** The matching entities, most recently written to first. */
    @SerialName("entities") val entities: List<EntityIndexRow>,
    /** Cursor and totals. */
    @SerialName("page") val page: Page
) {
}

/**
 * How far one aspect has got for one entity. Counted from stored notes, so it is
 * what has happened and not a projection of what will.
 */
@Serializable
data class AspectProgressRow(
    /** Which aspect this row is about. */
    @SerialName("aspect") val aspect: String,
    /** The state this aspect is in for this entity. */
    @SerialName("readiness") val readiness: AspectReadiness,
    /** How many notes are recorded along it. */
    @SerialName("note_count") val noteCount: Long,
    /** How many distinct traces those notes were written about. */
    @SerialName("trace_count") val traceCount: Long,
    /**
     * How many more notes this aspect needs before a pass will run. Zero once the
     * floor is reached, which is what turns a wait into a number a reader can watch.
     */
    @SerialName("notes_needed") val notesNeeded: Long,
    /** When the newest note along this aspect was recorded. Absent when there are none. */
    @SerialName("last_note_at") val lastNoteAt: Instant? = null
) {
}

/**
 * What the daemon has recorded for one entity, aspect by aspect. This is the
 * answer to why a themes screen is empty, and it separates the three cases that
 * look identical on screen: nothing has been recorded, enough has been recorded
 * and no pass has run yet, or there are themes to read and the reader is looking
 * at the wrong aspect.
 */
@Serializable
data class AspectProgress(
    /** The kind of entity this describes. */
    @SerialName("entity_kind") val entityKind: EntityKind,
    /** Its name. */
    @SerialName("entity_name") val entityName: String,
    /** The entity's overall state, which is the least advanced of its enabled aspects. */
    @SerialName("readiness") val readiness: AspectReadiness,
    /** How many of this entity's traces carry at least one note. */
    @SerialName("trace_count") val traceCount: Long,
    /** One row per enabled aspect, in catalogue order. */
    @SerialName("aspects") val aspects: List<AspectProgressRow>,
    /**
     * The newest snapshot for this entity, when a pass has written one. Absent while
     * every aspect is still collecting.
     */
    @SerialName("latest_snapshot") val latestSnapshot: ThemeSnapshot? = null
) {
}

/** One snapshot in a timeline, with the reason it was chosen. */
@Serializable
data class SnapshotLandmark(
    /** The snapshot. */
    @SerialName("snapshot") val snapshot: ThemeSnapshot,
    /**
     * Why this snapshot is in the selection. Present for `presentation=landmarks`
     * only; an inventory returns consecutive snapshots and chooses nothing, so there
     * is nothing to report.
     */
    @SerialName("reason") val reason: SnapshotLandmarkReason? = null
) {
}

/**
 * A selection of one entity's snapshots, oldest first, so a timeline can be drawn
 * without reordering anything.
 */
@Serializable
data class SnapshotListResponse(
    /** The selected snapshots, oldest first. */
    @SerialName("snapshots") val snapshots: List<SnapshotLandmark>,
    /**
     * How many snapshots the entity has in the requested range. This is the number the
     * selection was drawn from and not the number returned, so a timeline can say what
     * it is standing in for.
     */
    @SerialName("total") val total: Long,
    /** Cursor and totals. */
    @SerialName("page") val page: Page
) {
}

/**
 * One node of one stage of a flow: a theme, the stage's noise, or the overflow the
 * stage folded away.
 */
@Serializable
data class ThemeNode(
    /**
     * Identifies this node inside this flow, and is what a link names at either end.
     * It is not a theme id: the noise and overflow nodes have one of these and no
     * theme id.
     */
    @SerialName("node_id") val nodeId: String,
    /** Whether this node is a theme, the noise or the overflow. */
    @SerialName("kind") val kind: ThemeNodeKind,
    /**
     * What to show for this node: a theme's own label, or the words for noise or
     * overflow.
     */
    @SerialName("label") val label: String,
    /** How many traces this node accounts for. */
    @SerialName("trace_count") val traceCount: Long,
    /**
     * This node's share of the stage's traces, between 0 and 1. A stage's nodes cover
     * all of it, so its shares add to 1.
     */
    @SerialName("coverage") val coverage: Double,
    /**
     * The theme this node stands for. Present only when `kind` is `theme`, which is
     * what makes the other two impossible to drill into by accident.
     */
    @SerialName("theme_id") val themeId: String? = null
) {
}

/**
 * One aspect's column in a flow. Stages come back in the order the aspects were
 * asked for, and links only ever join neighbouring stages, so the order decides
 * which co-occurrences are visible and is not a display preference.
 */
@Serializable
data class ThemeStage(
    /** Which aspect this stage is. */
    @SerialName("aspect") val aspect: String,
    /** How many traces this stage accounts for. */
    @SerialName("trace_count") val traceCount: Long,
    /** The stage's nodes, largest first, with the noise and overflow nodes last. */
    @SerialName("nodes") val nodes: List<ThemeNode>
) {
}

/**
 * A ribbon between one node of a stage and one node of the next. It says the two
 * describe the same traces and nothing more: a link between a goal theme and an
 * outcome theme means they were written about the same runs, not that one caused
 * the other.
 */
@Serializable
data class ThemeLink(
    /** The node this link leaves, in the earlier stage. */
    @SerialName("source_node_id") val sourceNodeId: String,
    /** The node it reaches, in the next stage. */
    @SerialName("target_node_id") val targetNodeId: String,
    /** How many traces are assigned to both ends. */
    @SerialName("trace_count") val traceCount: Long,
    /** That count as a fraction of the source node's traces, between 0 and 1. */
    @SerialName("source_share") val sourceShare: Double,
    /**
     * That count as a fraction of the target node's traces, between 0 and 1. The two
     * shares differ whenever the nodes are different sizes, and reading only one of
     * them is how a thin ribbon gets mistaken for a weak relationship.
     */
    @SerialName("target_share") val targetShare: Double
) {
}

/**
 * One snapshot read across two to four aspects at once: a stage per aspect, and
 * the links between neighbouring stages. Association, never causation.
 */
@Serializable
data class ThemeFlow(
    /**
     * The snapshot this flow was computed from, so every count on the page can be read
     * against the window it came from.
     */
    @SerialName("snapshot") val snapshot: ThemeSnapshot,
    /** One stage per aspect, in the order they were asked for. */
    @SerialName("stages") val stages: List<ThemeStage>,
    /**
     * The links between neighbouring stages. There are none between stages that are
     * not neighbours, which is why the order of the aspects matters.
     */
    @SerialName("links") val links: List<ThemeLink>
) {
}

/** One theme in one snapshot, or the answer that this snapshot did not hold it. */
@Serializable
data class ThemeDetail(
    /**
     * The snapshot that was read, with its own counts, so a caller can write `28 of 70
     * traces in this window` without a second request.
     */
    @SerialName("snapshot") val snapshot: ThemeSnapshot,
    /**
     * The theme, when this snapshot holds it. Absent is an answer and not an error:
     * the theme did not exist in that window, which is what scrubbing a timeline back
     * past a theme's birth looks like. A theme id this entity has never had is a 404
     * instead.
     */
    @SerialName("theme") val theme: Theme? = null
) {
}

/**
 * One note from a theme, with the run it was written about. The two ids are
 * different things: `note_id` identifies the sentence and `trace_id` identifies
 * the run it describes.
 */
@Serializable
data class ThemeExample(
    /** The note this example is. */
    @SerialName("note_id") val noteId: String,
    /** The trace the note was written about. */
    @SerialName("trace_id") val traceId: String,
    /**
     * The sentence, after redaction. This is the wording that put the trace in this
     * theme, so a reader can see why it landed here rather than take it on trust.
     */
    @SerialName("text") val text: String,
    /**
     * How much this note's wording overlaps the theme's label, counted out of 64. It
     * is a count of shared sketch positions and not a percentage, and it is an integer
     * on purpose: no part of the clustering uses floating-point arithmetic, so two
     * replicas cannot order the same notes differently.
     */
    @SerialName("similarity") val similarity: Int,
    /**
     * When the run began, so examples can be read against the period they came from
     * rather than against when the note was written.
     */
    @SerialName("trace_started_at") val traceStartedAt: Instant
) {
}

/** A page of the notes behind one theme. */
@Serializable
data class ThemeExampleListResponse(
    /** The notes, most like the theme's label first. */
    @SerialName("examples") val examples: List<ThemeExample>,
    /** Cursor and totals. */
    @SerialName("page") val page: Page
) {
}

/**
 * One theme in one snapshot of its life. Every count belongs to that snapshot
 * alone, and windows overlap, so a run of these is read as a series and never
 * added up.
 */
@Serializable
data class ThemeHistoryPoint(
    /** The snapshot this point comes from. */
    @SerialName("snapshot_id") val snapshotId: String,
    /** When that snapshot became the current state. */
    @SerialName("cutoff_at") val cutoffAt: Instant,
    /** Start of that snapshot's window, inclusive. */
    @SerialName("window_from") val windowFrom: Instant,
    /** End of that snapshot's window, exclusive. */
    @SerialName("window_to") val windowTo: Instant,
    /** What had happened to the theme by this point. */
    @SerialName("state") val state: ThemeState,
    /** How many notes were in the theme here. */
    @SerialName("note_count") val noteCount: Long,
    /** How many distinct traces those notes were written about. */
    @SerialName("trace_count") val traceCount: Long,
    /**
     * The theme's share of the notes that snapshot read for this aspect, between 0 and
     * 1. Coverage and raw count can move in opposite directions, and coverage is the
     * one that says whether a theme is gaining ground.
     */
    @SerialName("coverage") val coverage: Double,
    /** The themes this one came out of at this point. Empty unless `state` is `split`. */
    @SerialName("split_from") val splitFrom: List<String>,
    /**
     * The themes that came out of this one at this point. Both directions are stored,
     * so a caller never has to read a second theme's history to learn what happened to
     * this one.
     */
    @SerialName("split_into") val splitInto: List<String>,
    /** The themes that became this one at this point. Empty unless `state` is `merge`. */
    @SerialName("merged_from") val mergedFrom: List<String>,
    /** The theme this one became at this point, when it stopped being its own theme. */
    @SerialName("merged_into") val mergedInto: List<String>
) {
}

/**
 * One theme across every stored snapshot that holds it, oldest first. This is the
 * only theme response that names no single snapshot, because the series is the
 * answer.
 */
@Serializable
data class ThemeHistory(
    /** The theme this history is of. */
    @SerialName("theme_id") val themeId: String,
    /** The aspect it was clustered along. */
    @SerialName("aspect") val aspect: String,
    /**
     * The theme's label at its newest point. A label can change between snapshots,
     * because the note most like the others can change while the theme holds together.
     */
    @SerialName("label") val label: String,
    /**
     * The series, oldest first, so nothing has to be reordered before it is drawn or
     * differenced.
     */
    @SerialName("points") val points: List<ThemeHistoryPoint>,
    /** Cursor and totals. */
    @SerialName("page") val page: Page
) {
}

/**
 * Every aspect note recorded about one trace, which is at most one per aspect.
 * Read it beside the trace itself: a note is the sentence that put this run in
 * each of its themes, so it is the shortest route from a theme back to what
 * actually happened.
 */
@Serializable
data class TraceAspectNotes(
    /** The trace these notes are about. */
    @SerialName("trace_id") val traceId: String,
    /**
     * The notes, in catalogue order. Empty when nothing has written one yet, which is
     * ordinary for a recent trace.
     */
    @SerialName("notes") val notes: List<AspectNote>
) {
}

/** An email address and a password, for sign-up and for log-in. */
@Serializable
data class AccountCredentials(
    /**
     * The account's email address. Compared without regard to case, and with
     * surrounding spaces removed.
     */
    @SerialName("email") val email: String,
    /** At least 10 characters and at most 1024 bytes. Stored only as an argon2id hash. */
    @SerialName("password") val password: String,
    /**
     * The six-digit code from an authenticator app, or one of the recovery codes, when
     * the account has two-step log-in on. Ignored by sign-up.
     */
    @SerialName("totp_code") val totpCode: String? = null
) {
}

/** A logged-in session. Send `token` as `Authorization: Bearer <token>`. */
@Serializable
data class AccountSession(
    /** The session token. It is shown once: the daemon keeps only its SHA-256 digest. */
    @SerialName("token") val token: String,
    /** When the session stops working. Log in again after this. */
    @SerialName("expires_at") val expiresAt: Instant,
    /** The account's email address, as stored. */
    @SerialName("email") val email: String,
    /**
     * The project reads go to when they name none. The account's own project, or the
     * first project of the organisation a single sign-on session is limited to. Absent
     * when the session reaches no project yet.
     */
    @SerialName("project") val project: String? = null,
    /**
     * Set when the session came from an organisation's single sign-on. The session
     * then reaches that organisation's projects only.
     */
    @SerialName("org_id") val orgId: String? = null
) {
}

/** The account a session belongs to. */
@Serializable
data class Account(
    /** The account's email address, as stored. */
    @SerialName("email") val email: String,
    /** When the account was created. */
    @SerialName("created_at") val createdAt: Instant,
    /** Whether the address has been confirmed by a link sent to it. */
    @SerialName("email_verified") val emailVerified: Boolean,
    /** Whether two-step log-in is on. */
    @SerialName("totp_enabled") val totpEnabled: Boolean,
    /**
     * Whether the account can log in with a password. False for an account created by
     * single sign-on or SCIM that has never set one.
     */
    @SerialName("has_password") val hasPassword: Boolean,
    /**
     * The project created with the account at sign-up. Absent for an account that was
     * invited, or created by single sign-on or SCIM.
     */
    @SerialName("project") val project: String? = null,
    /**
     * The organisation that created this account through single sign-on or SCIM. That
     * organisation can deactivate it.
     */
    @SerialName("managed_by") val managedBy: String? = null
) {
}

/**
 * One API key an account holds. The secret is not part of it: the daemon keeps
 * only a digest.
 */
@Serializable
data class AccountKey(
    /** The key's identifier, for revoking it. Not a secret. */
    @SerialName("id") val id: String,
    /** What the key may do. */
    @SerialName("scope") val scope: KeyScope,
    /** The one project the key reaches. */
    @SerialName("project") val project: String,
    /**
     * The first characters of the secret, so you can tell which key a process holds
     * without the daemon storing the secret.
     */
    @SerialName("prefix") val prefix: String,
    /** When the key was created. */
    @SerialName("created_at") val createdAt: Instant,
    /** The label given when the key was created. */
    @SerialName("name") val name: String? = null,
    /**
     * The email of the account that created the key. Set in an organisation's key
     * list, so an admin can see whose key it is.
     */
    @SerialName("created_by") val createdBy: String? = null
) {
}

/** A request for a new API key. */
@Serializable
data class CreateAccountKeyRequest(
    /** What the key may do. One scope per key. */
    @SerialName("scope") val scope: KeyScope,
    /**
     * A label for the key, such as the service that will hold it. At most 100
     * characters.
     */
    @SerialName("name") val name: String? = null,
    /**
     * The project the key reaches. Defaults to the account's own project. Your role on
     * the project must allow the scope.
     */
    @SerialName("project") val project: String? = null
) {
}

/** A new API key, with its secret. This is the only time the secret is shown. */
@Serializable
data class CreatedAccountKey(
    /** The key as it is listed from now on. */
    @SerialName("key") val key: AccountKey,
    /**
     * The key itself. Send it as `Authorization: Bearer <secret>`. Copy it now: the
     * daemon keeps only its SHA-256 digest and cannot show it again.
     */
    @SerialName("secret") val secret: String
) {
}

/**
 * A plain answer that changes nothing you can read back, such as "check your
 * email".
 */
@Serializable
data class AccountNotice(
    /** What happened, and what to do next. */
    @SerialName("message") val message: String
) {
}

/** A one-time token from a link the daemon sent by email. */
@Serializable
data class TokenRequest(
    /** The token, as it appears in the link. */
    @SerialName("token") val token: String
) {
}

/** A request for a password reset link. */
@Serializable
data class PasswordForgotRequest(
    /**
     * The address to send the link to. The answer is the same whether or not an
     * account uses it.
     */
    @SerialName("email") val email: String
) {
}

/** A new password, with the token from a reset link. */
@Serializable
data class PasswordResetRequest(
    /** The token from the reset link. It works once, for one hour. */
    @SerialName("token") val token: String,
    /** The new password. At least 10 characters and at most 1024 bytes. */
    @SerialName("password") val password: String,
    /**
     * A code from the authenticator app, or a recovery code, when the account has
     * two-step log-in on. A reset proves the address, not the second factor.
     */
    @SerialName("totp_code") val totpCode: String? = null
) {
}

/** A new password for the logged-in account. */
@Serializable
data class PasswordChangeRequest(
    /** The new password. At least 10 characters and at most 1024 bytes. */
    @SerialName("new_password") val newPassword: String,
    /** The password in use now. Required when the account has one. */
    @SerialName("current_password") val currentPassword: String? = null
) {
}

/** A new email address for the logged-in account. */
@Serializable
data class EmailChangeRequest(
    /**
     * The new address. A link is sent to it, and the change happens when the link is
     * opened.
     */
    @SerialName("email") val email: String,
    /** The account's password. Required when the account has one. */
    @SerialName("password") val password: String? = null
) {
}

/** Confirmation for deleting the logged-in account. */
@Serializable
data class AccountDeleteRequest(
    /** The account's password. Required when the account has one. */
    @SerialName("password") val password: String? = null,
    /** A code from the authenticator app. Required when two-step log-in is on. */
    @SerialName("totp_code") val totpCode: String? = null
) {
}

/** What deleting an account removed. */
@Serializable
data class AccountDeletion(
    /** Organisations that had this account as their only member, deleted with it. */
    @SerialName("orgs_deleted") val orgsDeleted: Int,
    /**
     * Projects whose stored signals and runs were erased, because their organisation
     * was deleted.
     */
    @SerialName("projects_erased") val projectsErased: List<String>
) {
}

/** One live session of the logged-in account. The token is not part of it. */
@Serializable
data class SessionInfo(
    /** The session's identifier, for ending it. Not a secret. */
    @SerialName("id") val id: String,
    /** When the session started. */
    @SerialName("created_at") val createdAt: Instant,
    /** When it stops working. */
    @SerialName("expires_at") val expiresAt: Instant,
    /** True for the session this request carries. */
    @SerialName("current") val current: Boolean,
    /** How it started: `password`, `sso`, `invite`, `verify` or `reset`. */
    @SerialName("via") val via: String,
    /** The organisation a single sign-on session is limited to. */
    @SerialName("org_id") val orgId: String? = null,
    /** The browser or client that started it, as it described itself. */
    @SerialName("user_agent") val userAgent: String? = null,
    /** The client address that started it. */
    @SerialName("address") val address: String? = null
) {
}

/**
 * A new authenticator secret, not yet in use. Confirm it with a code to turn
 * two-step log-in on.
 */
@Serializable
data class TotpSetup(
    /** The secret in base32, for typing into an authenticator app. */
    @SerialName("secret") val secret: String,
    /** The same secret as an `otpauth://` URI. */
    @SerialName("otpauth_uri") val otpauthUri: String,
    /** The URI as a QR code, an SVG document. */
    @SerialName("qr_svg") val qrSvg: String
) {
}

/** A code from an authenticator app. */
@Serializable
data class TotpCodeRequest(
    /** The six-digit code. */
    @SerialName("code") val code: String
) {
}

/** Two-step log-in is on. */
@Serializable
data class TotpEnabled(
    /**
     * Ten single-use codes that stand in for the authenticator app. They are shown
     * once: the daemon keeps only their digests.
     */
    @SerialName("recovery_codes") val recoveryCodes: List<String>
) {
}

/** Confirmation for turning two-step log-in off. */
@Serializable
data class TotpDisableRequest(
    /** A code from the authenticator app, or a recovery code. */
    @SerialName("code") val code: String,
    /** The account's password. Required when the account has one. */
    @SerialName("password") val password: String? = null
) {
}

/** An organisation: the owner of projects, members and single sign-on settings. */
@Serializable
data class Org(
    /** The organisation's identifier. */
    @SerialName("id") val id: String,
    /** Its display name. */
    @SerialName("name") val name: String,
    /** Your role in it. */
    @SerialName("role") val role: OrgRole,
    /** When it was created. */
    @SerialName("created_at") val createdAt: Instant,
    /** How many members it has. */
    @SerialName("members") val members: Int,
    /** How many projects it owns. */
    @SerialName("projects") val projects: Int,
    /**
     * True when the daemon's configuration file declares this organisation. Its single
     * sign-on settings are then read from that file and cannot be changed here.
     */
    @SerialName("managed_by_config") val managedByConfig: Boolean,
    /**
     * The most signals all its projects together may store per minute. Absent means no
     * ceiling.
     */
    @SerialName("quota_per_minute") val quotaPerMinute: Int? = null,
    /** How long its projects keep data, in days. Absent means forever. */
    @SerialName("retention_days") val retentionDays: Int? = null
) {
}

/** A new organisation. */
@Serializable
data class CreateOrgRequest(
    /** Its display name, at most 100 characters. */
    @SerialName("name") val name: String
) {
}

/** Changes to an organisation. A field left out keeps its value. */
@Serializable
data class UpdateOrgRequest(
    /** A new display name, at most 100 characters. */
    @SerialName("name") val name: String? = null,
    /**
     * A ceiling on signals per minute across all the organisation's projects, at most
     * the daemon's own ceiling. 0 restores the daemon's default. Owner only.
     */
    @SerialName("quota_per_minute") val quotaPerMinute: Int? = null,
    /**
     * How long to keep data, in days, at most the daemon's own window. 0 restores the
     * daemon's default. Owner only.
     */
    @SerialName("retention_days") val retentionDays: Int? = null
) {
}

/** What deleting an organisation removed. */
@Serializable
data class OrgDeletion(
    /** Projects whose stored signals and runs were erased. */
    @SerialName("projects_erased") val projectsErased: List<String>
) {
}

/** A role on one project. */
@Serializable
data class ProjectRoleGrant(
    /** The project. */
    @SerialName("project") val project: String,
    /** The role there. */
    @SerialName("role") val role: ProjectRole
) {
}

/** One member of an organisation. */
@Serializable
data class OrgMember(
    /** The member's account. */
    @SerialName("account_id") val accountId: String,
    /** The member's email address. */
    @SerialName("email") val email: String,
    /** The member's role. */
    @SerialName("role") val role: OrgRole,
    /** How they joined: `created`, `invite`, `sso`, `scim` or `config`. */
    @SerialName("via") val via: String,
    /** When they joined. */
    @SerialName("joined_at") val joinedAt: Instant,
    /** Roles on single projects that replace the organisation role there. */
    @SerialName("project_roles") val projectRoles: List<ProjectRoleGrant>,
    /** True when this organisation created the account through single sign-on or SCIM. */
    @SerialName("managed") val managed: Boolean,
    /** Whether the member has two-step log-in on. */
    @SerialName("totp_enabled") val totpEnabled: Boolean
) {
}

/** A new role for a member. */
@Serializable
data class UpdateMemberRequest(
    /**
     * The role. Only an owner may grant or remove `owner`, and the last owner cannot
     * be demoted.
     */
    @SerialName("role") val role: OrgRole
) {
}

/** A role for a member on one project. */
@Serializable
data class SetProjectRoleRequest(
    /** A project the organisation owns. */
    @SerialName("project") val project: String,
    /**
     * The role there. Absent removes the project role, so the organisation role
     * applies again.
     */
    @SerialName("role") val role: ProjectRole? = null
) {
}

/** An invitation not yet accepted. The token is not part of it. */
@Serializable
data class OrgInvite(
    /** The invitation's identifier, for revoking it. */
    @SerialName("id") val id: String,
    /** The address it was sent to. */
    @SerialName("email") val email: String,
    /** The role it grants. */
    @SerialName("role") val role: OrgRole,
    /** Project roles it grants as well. */
    @SerialName("project_roles") val projectRoles: List<ProjectRoleGrant>,
    /** When it was sent. */
    @SerialName("created_at") val createdAt: Instant,
    /** When it stops working. */
    @SerialName("expires_at") val expiresAt: Instant,
    /**
     * The email of the member who sent it. Absent when the configuration file or SCIM
     * sent it.
     */
    @SerialName("invited_by") val invitedBy: String? = null
) {
}

/** An invitation to an organisation. */
@Serializable
data class CreateInviteRequest(
    /** Who to invite. The link is sent to this address. */
    @SerialName("email") val email: String,
    /** The role to grant. Only an owner may invite an owner. */
    @SerialName("role") val role: OrgRole,
    /** Project roles to grant as well. */
    @SerialName("project_roles") val projectRoles: List<ProjectRoleGrant>? = null
) {
}

/** What an invitation offers, read with its token before accepting it. */
@Serializable
data class InviteLookup(
    /** The organisation. */
    @SerialName("org_name") val orgName: String,
    /** The address it was sent to. Accepting it joins the account with this address. */
    @SerialName("email") val email: String,
    /** The role it grants. */
    @SerialName("role") val role: OrgRole,
    /**
     * Whether an account with this address exists. If it does, log in as it to accept;
     * if not, accepting creates it.
     */
    @SerialName("account_exists") val accountExists: Boolean,
    /** When it stops working. */
    @SerialName("expires_at") val expiresAt: Instant
) {
}

/** Acceptance of an invitation. */
@Serializable
data class InviteAcceptRequest(
    /** The token from the invitation link. */
    @SerialName("token") val token: String,
    /**
     * A password for the new account, when no account uses the invited address.
     * Ignored otherwise.
     */
    @SerialName("password") val password: String? = null
) {
}

/** An accepted invitation. */
@Serializable
data class InviteAccepted(
    /** The organisation joined. */
    @SerialName("org") val org: Org,
    /**
     * A session for the account accepting created. Absent when an existing account
     * accepted with its own session.
     */
    @SerialName("session") val session: AccountSession? = null
) {
}

/** A project an organisation owns. */
@Serializable
data class OrgProject(
    /**
     * The project name. Random, so nobody can choose one that already holds somebody
     * else's data.
     */
    @SerialName("project") val project: String,
    /** When it was created or attached. */
    @SerialName("created_at") val createdAt: Instant,
    /** A display label. */
    @SerialName("label") val label: String? = null
) {
}

/** A new project in an organisation. */
@Serializable
data class CreateProjectRequest(
    /** A display label, at most 100 characters. */
    @SerialName("label") val label: String? = null
) {
}

/**
 * How much an organisation is using, against its ceilings. Counted since the
 * daemon started.
 */
@Serializable
data class OrgUsage(
    /** The organisation. */
    @SerialName("org_id") val orgId: String,
    /** Signals stored in the current one-minute window. */
    @SerialName("admitted_this_minute") val admittedThisMinute: Long,
    /** Signals stored since the daemon started. */
    @SerialName("admitted_total") val admittedTotal: Long,
    /** Signals refused for the ceiling since the daemon started. */
    @SerialName("refused_total") val refusedTotal: Long,
    /** Members. */
    @SerialName("members") val members: Int,
    /** Projects. */
    @SerialName("projects") val projects: Int,
    /** API keys on its projects. */
    @SerialName("keys") val keys: Int,
    /** When counting started. */
    @SerialName("since") val since: Instant,
    /** Its ceiling on signals per minute, when it has one. */
    @SerialName("quota_per_minute") val quotaPerMinute: Int? = null,
    /** How long its projects keep data, in days. */
    @SerialName("retention_days") val retentionDays: Int? = null
) {
}

/** An organisation's single sign-on settings. The client secret is not part of it. */
@Serializable
data class SsoConfig(
    /** The kind of provider. */
    @SerialName("provider") val provider: SsoProvider,
    /** The client id registered with the provider. */
    @SerialName("client_id") val clientId: String,
    /** Whether a client secret is stored. */
    @SerialName("has_client_secret") val hasClientSecret: Boolean,
    /**
     * Email domains this organisation claims. Single sign-on creates accounts only for
     * these domains, and "Log in with SSO" finds the organisation by them.
     */
    @SerialName("domains") val domains: List<String>,
    /**
     * When true, members whose email is in a claimed domain cannot log in with a
     * password.
     */
    @SerialName("enforced") val enforced: Boolean,
    /**
     * The role given to a member single sign-on creates, when no group maps to a role.
     * Never `owner`.
     */
    @SerialName("default_role") val defaultRole: OrgRole,
    /** The redirect URL to register with the provider. */
    @SerialName("redirect_url") val redirectUrl: String,
    /** True when these settings come from the configuration file. */
    @SerialName("managed_by_config") val managedByConfig: Boolean,
    /** When they were last changed. */
    @SerialName("updated_at") val updatedAt: Instant,
    /** The OpenID Connect issuer URL. Absent for GitHub. */
    @SerialName("issuer") val issuer: String? = null,
    /**
     * The ID token claim that lists the member's groups, mapped to roles by the group
     * roles.
     */
    @SerialName("groups_claim") val groupsClaim: String? = null
) {
}

/** New single sign-on settings for an organisation. */
@Serializable
data class PutSsoConfigRequest(
    /** The kind of provider. */
    @SerialName("provider") val provider: SsoProvider,
    /** The client id registered with the provider. */
    @SerialName("client_id") val clientId: String,
    /**
     * Email domains to claim. You can claim the domain of your own verified address. A
     * domain belongs to one organisation.
     */
    @SerialName("domains") val domains: List<String>,
    /**
     * Refuse password log-in for members in the claimed domains. Turning it on ends
     * their password sessions.
     */
    @SerialName("enforced") val enforced: Boolean,
    /** The role for a member single sign-on creates. `admin`, `member` or `viewer`. */
    @SerialName("default_role") val defaultRole: OrgRole,
    /**
     * The OpenID Connect issuer URL, such as `https://accounts.google.com`. Required
     * for `oidc`. Must be `https`.
     */
    @SerialName("issuer") val issuer: String? = null,
    /** The client secret. Left out, the stored one is kept. */
    @SerialName("client_secret") val clientSecret: String? = null,
    /** The ID token claim that lists groups, such as `groups`. */
    @SerialName("groups_claim") val groupsClaim: String? = null
) {
}

/** A group from the identity provider, and the role its members get. */
@Serializable
data class GroupRole(
    /**
     * The group name, as single sign-on or SCIM sends it. Compared without regard to
     * case.
     */
    @SerialName("group") val group: String,
    /** The role. Never `owner`. */
    @SerialName("role") val role: OrgRole
) {
}

/** Whether an organisation has a SCIM token. The token is not part of it. */
@Serializable
data class ScimTokenInfo(
    /** Whether a token exists. */
    @SerialName("configured") val configured: Boolean,
    /** The SCIM base URL to give your identity provider. */
    @SerialName("base_url") val baseUrl: String,
    /** The first characters of the token. */
    @SerialName("prefix") val prefix: String? = null,
    /** When it was created. */
    @SerialName("created_at") val createdAt: Instant? = null
) {
}

/**
 * A new SCIM token. This is the only time it is shown, and it replaces any earlier
 * token.
 */
@Serializable
data class CreatedScimToken(
    /**
     * The bearer token for your identity provider. The daemon keeps only its SHA-256
     * digest.
     */
    @SerialName("token") val token: String,
    /** Its first characters. */
    @SerialName("prefix") val prefix: String,
    /** When it was created. */
    @SerialName("created_at") val createdAt: Instant,
    /** The SCIM base URL to give your identity provider. */
    @SerialName("base_url") val baseUrl: String
) {
}

/** Where to start single sign-on. Give one of the two. */
@Serializable
data class SsoStartRequest(
    /** Your email address. Its domain finds the organisation. */
    @SerialName("email") val email: String? = null,
    /** The organisation, when you know it. */
    @SerialName("org_id") val orgId: String? = null
) {
}

/** Where to send the browser for single sign-on. */
@Serializable
data class SsoStart(
    /**
     * The provider's authorization URL, with state, nonce and the PKCE challenge in
     * it.
     */
    @SerialName("authorize_url") val authorizeUrl: String,
    /**
     * A secret to keep in this browser and send back with the callback. It ties the
     * callback to the browser that started it, so a link from somebody else cannot log
     * you in as them.
     */
    @SerialName("binding") val binding: String,
    /** The organisation. */
    @SerialName("org_name") val orgName: String
) {
}

/** What the provider sent back to the redirect URL. */
@Serializable
data class SsoCallbackRequest(
    /** The `state` parameter. */
    @SerialName("state") val state: String,
    /** The `code` parameter. */
    @SerialName("code") val code: String,
    /** The binding from the start of the flow. */
    @SerialName("binding") val binding: String
) {
}

/**
 * A new version of a prompt. Posting a body identical to the latest version stores
 * nothing new and returns that version, so a deploy that posts its prompts on
 * every start is a no-op when nothing changed.
 */
@Serializable
data class CreatePromptRequest(
    /** The prompt's name. A new name creates the prompt at version 1. */
    @SerialName("name") val name: String,
    /** `text` or `chat`. Defaults to `chat` when `messages` is set, otherwise `text`. */
    @SerialName("kind") val kind: PromptKind? = null,
    /** The template, for a `text` prompt. */
    @SerialName("template") val template: String? = null,
    /** The messages, for a `chat` prompt. */
    @SerialName("messages") val messages: JsonElement? = null,
    /** Model settings to store with it. */
    @SerialName("config") val config: JsonElement? = null,
    /** What changed. */
    @SerialName("note") val note: String? = null,
    /** Labels to point at the stored version once it is stored, such as `staging`. */
    @SerialName("labels") val labels: List<String>? = null
) {
}

/** Which version a label should point at. */
@Serializable
data class PromptLabelRequest(
    /** The version. It must exist. */
    @SerialName("version") val version: Int
) {
}

/** Every version of one prompt, newest first. */
@Serializable
data class PromptVersionsResponse(
    /** The prompt's name. */
    @SerialName("name") val name: String,
    /** Its versions, newest first, each with the labels that point at it. */
    @SerialName("versions") val versions: List<Prompt>
) {
}

/**
 * A new version of a scorer definition. A body identical to the latest version is
 * not stored again.
 */
@Serializable
data class CreateScorerDefinitionRequest(
    /** The definition's id. A new id creates it at version 1. */
    @SerialName("id") val id: String,
    /** `judge` or `code`. */
    @SerialName("kind") val kind: ScorerDefinitionKind,
    /** What it measures, in a sentence. */
    @SerialName("description") val description: String,
    /** Which way is better. Defaults to `higher_is_better`. */
    @SerialName("direction") val direction: ScoreDirection? = null,
    /** For `judge`: the criterion. */
    @SerialName("prompt") val prompt: String? = null,
    /** For `code`: the code scorer to run. */
    @SerialName("builtin") val builtin: String? = null,
    /** For `code`: its settings. */
    @SerialName("config") val config: ScorerConfig? = null,
    /** What changed. */
    @SerialName("note") val note: String? = null
) {
}

/** Every version of one scorer definition, newest first. */
@Serializable
data class ScorerDefinitionVersions(
    /** The definition's id. */
    @SerialName("id") val id: String,
    /** Its versions, newest first. */
    @SerialName("versions") val versions: List<ScorerDefinition>
) {
}

/**
 * A new version of a live scoring rule. Posting the same body as the latest
 * version stores nothing new.
 */
@Serializable
data class CreateEvaluationRuleRequest(
    /** The rule's id: lower-case letters, digits, `-` and `_`, up to 64. */
    @SerialName("id") val id: String,
    /** The scorers to run. At least one. */
    @SerialName("scorers") val scorers: List<String>,
    /**
     * Whether it runs. Defaults to `false`, so a rule is read back and checked before
     * it spends anything.
     */
    @SerialName("enabled") val enabled: Boolean? = null,
    /** The share of matching runs to score, from 0 to 1. Defaults to 1. */
    @SerialName("sample_rate") val sampleRate: Double? = null,
    /** Only runs whose root step has this name. */
    @SerialName("name") val name: String? = null,
    /** Only runs whose root step is this kind. */
    @SerialName("kind") val kind: SpanKind? = null,
    /** Only runs from this environment. */
    @SerialName("environment") val environment: String? = null,
    /** Only runs carrying all of these tags. */
    @SerialName("tags") val tags: List<String>? = null,
    /** Only runs with this outcome. */
    @SerialName("status") val status: SpanStatus? = null,
    /** Settings for the code scorers that need them. */
    @SerialName("config") val config: ScorerConfig? = null,
    /** The most judged scores per minute. Defaults to 30. */
    @SerialName("max_judged_per_minute") val maxJudgedPerMinute: Int? = null,
    /** What changed. */
    @SerialName("note") val note: String? = null
) {
}

/**
 * One unit of work. Spans are the backbone of mcpd: logs, metrics, scores and
 * feedback all hang off a span or a trace. Stored in the `spans` table.
 */
@Serializable
data class Span(
    /** 128-bit trace id, 32 lowercase hex characters. OpenTelemetry-compatible. */
    @SerialName("trace_id") val traceId: String,
    /** 64-bit span id, 16 lowercase hex characters. Unique within the trace. */
    @SerialName("span_id") val spanId: String,
    /**
     * What this operation is called. Keep it low-cardinality: `search_docs`, not
     * `search_docs(query=hello)`.
     */
    @SerialName("name") val name: String,
    /** What kind of operation this is. */
    @SerialName("kind") val kind: SpanKind,
    /** The outcome. */
    @SerialName("status") val status: SpanStatus,
    /** When the operation began, in UTC with nanosecond precision. */
    @SerialName("start_time") val startTime: Instant,
    /**
     * True when this span has no parent inside the trace. Materialised so the trace
     * list never scans children.
     */
    @SerialName("is_root") val isRoot: Boolean,
    /** True for a point in time rather than a duration. Event spans have no end. */
    @SerialName("is_event") val isEvent: Boolean,
    /** Which project, environment, session, thread and user this belongs to. */
    @SerialName("correlation") val correlation: Correlation,
    /**
     * When mcpd accepted this span. Differs from `start_time` for buffered or replayed
     * data, and is what retention is measured against.
     */
    @SerialName("received_at") val receivedAt: Instant,
    /** The parent span within this trace. Absent on the root. */
    @SerialName("parent_span_id") val parentSpanId: String? = null,
    /**
     * A parent from a foreign tracing system that mcpd did not create. Carried for
     * correlation, never used for mcpd's own tree.
     */
    @SerialName("external_parent_span_id") val externalParentSpanId: String? = null,
    /** When it finished. Absent while the span is still running. */
    @SerialName("end_time") val endTime: Instant? = null,
    /**
     * Derived from the timestamps at ingest, stored so the database never has to
     * compute it to sort.
     */
    @SerialName("duration_ns") val durationNs: Long? = null,
    /** The kind of thing that produced this span. */
    @SerialName("entity_kind") val entityKind: EntityKind? = null,
    /** Stable identifier of the producing entity. */
    @SerialName("entity_id") val entityId: String? = null,
    /** Display name of the producing entity. */
    @SerialName("entity_name") val entityName: String? = null,
    /**
     * What went in. For a model call this is the full prompt, and it is the single
     * most valuable field mcpd stores.
     */
    @SerialName("input") val input: JsonElement? = null,
    /** What came out. For a model call this is the completion. */
    @SerialName("output") val output: JsonElement? = null,
    /**
     * Kind-specific structured detail. See the attribute reference for what each kind
     * carries.
     */
    @SerialName("attributes") val attributes: JsonElement? = null,
    /** Anything you want to attach. mcpd never interprets this. */
    @SerialName("metadata") val metadata: JsonElement? = null,
    /** Token counts, on model and embedding spans. */
    @SerialName("usage") val usage: TokenUsage? = null,
    /** Computed cost, on spans that consumed tokens. */
    @SerialName("cost") val cost: Cost? = null,
    /**
     * How the model was called: the sampling settings the caller chose. Absent on a
     * span that made no model call, and on one whose producer did not record them.
     */
    @SerialName("model_parameters") val modelParameters: ModelParameters? = null,
    /** Failure detail, when `status` is `error`. */
    @SerialName("error") val error: SpanError? = null,
    /** Causal references to other spans or traces. */
    @SerialName("links") val links: List<SpanLink>? = null,
    /**
     * Free labels for filtering. Only meaningful on a root span, where they label the
     * whole trace.
     */
    @SerialName("tags") val tags: List<String>? = null,
    /**
     * Model identifier, denormalised out of `attributes` so it can be indexed and
     * grouped.
     */
    @SerialName("model") val model: String? = null,
    /** Model provider, denormalised for the same reason. */
    @SerialName("provider") val provider: String? = null,
    /**
     * Producer-supplied idempotency key. A retried batch with the same key is accepted
     * once.
     */
    @SerialName("ingest_key") val ingestKey: String? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "spans"
    }
}

/**
 * A whole run, projected from its spans. mcpd materialises this at ingest so the
 * list view is one indexed read rather than an aggregation over every span. One
 * per project and trace id: a trace id crosses service boundaries, so two projects
 * that call each other each hold their own run under it, built from their own
 * spans. Stored in the `traces` table.
 */
@Serializable
data class Trace(
    /** The trace this summarises. */
    @SerialName("trace_id") val traceId: String,
    /** The root span's name. */
    @SerialName("name") val name: String,
    /** The root span's kind. */
    @SerialName("kind") val kind: SpanKind,
    /** `error` when any span in the trace failed, otherwise the root's status. */
    @SerialName("status") val status: SpanStatus,
    /** Earliest start across the trace. */
    @SerialName("start_time") val startTime: Instant,
    /** How many spans have arrived. */
    @SerialName("span_count") val spanCount: Int,
    /** How many of them failed. */
    @SerialName("error_count") val errorCount: Int,
    /** How many model calls the run made. */
    @SerialName("model_count") val modelCount: Int,
    /** How many tool calls the run made. */
    @SerialName("tool_count") val toolCount: Int,
    /** Context inherited from the root span. */
    @SerialName("correlation") val correlation: Correlation,
    /** True once the root span has ended and no span is still open. */
    @SerialName("is_complete") val isComplete: Boolean,
    /** When the first span of this trace arrived. */
    @SerialName("received_at") val receivedAt: Instant,
    /** The root span, when one has arrived. */
    @SerialName("root_span_id") val rootSpanId: String? = null,
    /** Latest end across the trace. Absent while anything is still running. */
    @SerialName("end_time") val endTime: Instant? = null,
    /** Wall-clock span of the whole trace. */
    @SerialName("duration_ns") val durationNs: Long? = null,
    /** Token usage summed across every model span in the trace. */
    @SerialName("usage") val usage: TokenUsage? = null,
    /** Cost summed across the trace. This is the number a bill is reconciled against. */
    @SerialName("cost") val cost: Cost? = null,
    /**
     * The start of the root step's input as plain text, up to 240 characters, for a
     * list column. For a chat input it is the last user message. Filled by the trace
     * list; the full input is on the span.
     */
    @SerialName("input_preview") val inputPreview: String? = null,
    /**
     * The start of the root step's output as plain text, up to 240 characters. Filled
     * by the trace list; the full output is on the span.
     */
    @SerialName("output_preview") val outputPreview: String? = null,
    /** Tags from the root span. */
    @SerialName("tags") val tags: List<String>? = null,
    /**
     * Mean score per scorer id, maintained as scores arrive, so the list can sort by
     * quality without a join.
     */
    @SerialName("score_summary") val scoreSummary: JsonElement? = null,
    /** Counts per feedback kind and value. */
    @SerialName("feedback_summary") val feedbackSummary: JsonElement? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "traces"
    }
}

/**
 * A structured log line, correlated to the span that produced it. Logs are stored
 * beside traces so 'show me the logs for this run' is one query, not a context
 * switch to another tool. Stored in the `logs` table.
 */
@Serializable
data class LogRecord(
    /** Unique id for this record. */
    @SerialName("log_id") val logId: String,
    /** When the line was emitted. */
    @SerialName("timestamp") val timestamp: Instant,
    /** Severity. */
    @SerialName("level") val level: LogLevel,
    /** The rendered message. */
    @SerialName("message") val message: String,
    /** Project, environment, session and user. */
    @SerialName("correlation") val correlation: Correlation,
    /** When mcpd accepted it. */
    @SerialName("received_at") val receivedAt: Instant,
    /** The trace this line belongs to, when it was emitted inside one. */
    @SerialName("trace_id") val traceId: String? = null,
    /** The span this line belongs to. */
    @SerialName("span_id") val spanId: String? = null,
    /** Structured key/value context. Redacted like any payload. */
    @SerialName("fields") val fields: JsonElement? = null,
    /** Which logger emitted it. */
    @SerialName("logger") val logger: String? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "logs"
    }
}

/**
 * One numeric observation. mcpd aggregates these into time buckets on read, and
 * keeps raw points only as long as the raw retention class allows. Stored in the
 * `metrics` table.
 */
@Serializable
data class MetricPoint(
    /** Unique id for this observation. */
    @SerialName("metric_id") val metricId: String,
    /** Metric name, such as `mcpd.model.duration`. */
    @SerialName("name") val name: String,
    /** How it aggregates. */
    @SerialName("kind") val kind: MetricKind,
    /** The observed value. */
    @SerialName("value") val value: Double,
    /** When it was observed. */
    @SerialName("timestamp") val timestamp: Instant,
    /** Project and environment. */
    @SerialName("correlation") val correlation: Correlation,
    /** When mcpd accepted it. */
    @SerialName("received_at") val receivedAt: Instant,
    /** UCUM unit, such as `ms`, `By` or `{token}`. */
    @SerialName("unit") val unit: String? = null,
    /** Dimensions. Passed through the cardinality filter, which rejects unbounded keys. */
    @SerialName("labels") val labels: JsonElement? = null,
    /** Histogram bucket boundaries and counts, on histogram points. */
    @SerialName("buckets") val buckets: JsonElement? = null,
    /** Exemplar link: the trace that produced this observation. */
    @SerialName("trace_id") val traceId: String? = null,
    /**
     * Exemplar link: the span that produced it. This is what turns a spike on a chart
     * into a specific run you can open.
     */
    @SerialName("span_id") val spanId: String? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "metrics"
    }
}

/** A machine evaluation of a span or a trace. Stored in the `scores` table. */
@Serializable
data class Score(
    /** Unique id. */
    @SerialName("score_id") val scoreId: String,
    /** Which scorer produced this, such as `answer_relevance`. */
    @SerialName("scorer_id") val scorerId: String,
    /** Live, offline, experiment or manual. */
    @SerialName("source") val source: ScoreSource,
    /** The numeric result. */
    @SerialName("value") val value: Double,
    /** When the score was produced. */
    @SerialName("timestamp") val timestamp: Instant,
    /** Inherited from the target. */
    @SerialName("correlation") val correlation: Correlation,
    /** The scored trace. */
    @SerialName("trace_id") val traceId: String? = null,
    /** The scored span, when the score is about one step rather than the whole run. */
    @SerialName("span_id") val spanId: String? = null,
    /**
     * The scorer's version. A score is only comparable to another score from the same
     * version.
     */
    @SerialName("scorer_version") val scorerVersion: String? = null,
    /** Lower bound of the scale. Without it, a bare number is not interpretable. */
    @SerialName("min") val min: Double? = 0.0,
    /** Upper bound of the scale. */
    @SerialName("max") val max: Double? = 1.0,
    /** A categorical result, when the scorer produces one alongside the number. */
    @SerialName("label") val label: String? = null,
    /**
     * Why. For an LLM judge this is the judge's explanation, and it is what makes a
     * score arguable rather than magic.
     */
    @SerialName("reason") val reason: String? = null,
    /**
     * The trace of the scoring run itself, so a suspicious score can be debugged like
     * any other run.
     */
    @SerialName("score_trace_id") val scoreTraceId: String? = null,
    /**
     * What producing this score cost. Judged evaluation is not free and mcpd refuses
     * to hide that.
     */
    @SerialName("cost") val cost: Cost? = null,
    /** Anything else. */
    @SerialName("metadata") val metadata: JsonElement? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "scores"
    }
}

/**
 * A human evaluation. Kept distinct from `Score` because the two answer different
 * questions and must never be averaged together. Stored in the `feedback` table.
 */
@Serializable
data class Feedback(
    /** Unique id. */
    @SerialName("feedback_id") val feedbackId: String,
    /** Thumbs, rating, correction, tag or comment. */
    @SerialName("kind") val kind: FeedbackKind,
    /** The judgement. Its shape follows `kind`. */
    @SerialName("value") val value: JsonElement,
    /** When it was given. */
    @SerialName("timestamp") val timestamp: Instant,
    /** Inherited from the target. */
    @SerialName("correlation") val correlation: Correlation,
    /** The trace being judged. */
    @SerialName("trace_id") val traceId: String? = null,
    /** The span being judged. */
    @SerialName("span_id") val spanId: String? = null,
    /** Free prose the person added. */
    @SerialName("comment") val comment: String? = null,
    /** Who gave it. Hashed by default. */
    @SerialName("author_id") val authorId: String? = null,
    /** Whether the author was an end user, a reviewer or an operator. */
    @SerialName("author_kind") val authorKind: String? = null,
    /** Anything else. */
    @SerialName("metadata") val metadata: JsonElement? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "feedback"
    }
}

/**
 * The tenancy boundary. Every signal belongs to exactly one project; retention,
 * redaction, sampling, budgets and access are all configured per project. Stored
 * in the `projects` table.
 */
@Serializable
data class Project(
    /** Stable identifier, used in API paths and ingest keys. */
    @SerialName("project_id") val projectId: String,
    /** Display name. */
    @SerialName("name") val name: String,
    /** When it was created. */
    @SerialName("created_at") val createdAt: Instant,
    /** How long to keep signals. Absent means the server default. */
    @SerialName("retention_days") val retentionDays: Int? = null,
    /** The sampling policy for this project. */
    @SerialName("sampling") val sampling: JsonElement? = null,
    /** Extra redaction rules, applied on top of the server defaults. */
    @SerialName("redaction") val redaction: JsonElement? = null,
    /** Cost budget and what happens when it is exceeded. */
    @SerialName("budget") val budget: JsonElement? = null,
    /** Anything else. */
    @SerialName("metadata") val metadata: JsonElement? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "projects"
    }
}

/**
 * A fixed collection of inputs, with optional expected outputs, that an experiment
 * runs against. Datasets are how a change is proved not to regress. Stored in the
 * `datasets` table.
 */
@Serializable
data class Dataset(
    /** Stable identifier. */
    @SerialName("dataset_id") val datasetId: String,
    /** Display name. */
    @SerialName("name") val name: String,
    /** How many items it holds. */
    @SerialName("item_count") val itemCount: Int,
    /** When it was created. */
    @SerialName("created_at") val createdAt: Instant,
    /** Which project owns it. */
    @SerialName("correlation") val correlation: Correlation,
    /** What this dataset is for. */
    @SerialName("description") val description: String? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "datasets"
    }
}

/** One case in a dataset. Stored in the `dataset_items` table. */
@Serializable
data class DatasetItem(
    /** Stable identifier. */
    @SerialName("item_id") val itemId: String,
    /** Which dataset this belongs to. */
    @SerialName("dataset_id") val datasetId: String,
    /** What to feed the system under test. */
    @SerialName("input") val input: JsonElement,
    /**
     * The reference answer, when there is one. Absent for open-ended cases judged by a
     * scorer.
     */
    @SerialName("expected") val expected: JsonElement? = null,
    /** Anything else, such as a difficulty label or a source citation. */
    @SerialName("metadata") val metadata: JsonElement? = null,
    /**
     * The production trace this case was captured from, when it was. This is how a bug
     * report becomes a permanent regression test.
     */
    @SerialName("source_trace_id") val sourceTraceId: String? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "dataset_items"
    }
}

/**
 * One evaluation run of a system against a dataset. Comparing two experiments is
 * how a change is accepted or rejected. Stored in the `experiments` table.
 */
@Serializable
data class Experiment(
    /** Stable identifier. */
    @SerialName("experiment_id") val experimentId: String,
    /** Display name. */
    @SerialName("name") val name: String,
    /** What it ran against. */
    @SerialName("dataset_id") val datasetId: String,
    /** Where it got to. */
    @SerialName("status") val status: ExperimentStatus,
    /** Cases attempted. */
    @SerialName("item_count") val itemCount: Int,
    /** Which project owns it. */
    @SerialName("correlation") val correlation: Correlation,
    /** When it began. */
    @SerialName("started_at") val startedAt: Instant? = null,
    /** When it ended. */
    @SerialName("finished_at") val finishedAt: Instant? = null,
    /** Aggregate score per scorer, with mean, standard deviation and count. */
    @SerialName("summary") val summary: JsonElement? = null,
    /** The experiment this one is compared against. */
    @SerialName("baseline_id") val baselineId: String? = null,
    /** What the whole run cost. */
    @SerialName("cost") val cost: Cost? = null,
    /** Why it failed, when it did. */
    @SerialName("error") val error: String? = null,
    /** The configuration under test: model, prompt version, git sha. */
    @SerialName("metadata") val metadata: JsonElement? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "experiments"
    }
}

/**
 * One recorded attempt to read stored payloads, or to delete them. Written by the
 * handlers that hand prompts and completions back out of mcpd, so the question
 * "who read this run" has an answer. The audit log is not trimmed by the retention
 * pass: a log that deleted itself on the same schedule as the data it describes
 * could not say who read the data that has since gone. Stored in the `audit_log`
 * table.
 */
@Serializable
data class AuditEvent(
    /** Unique id. */
    @SerialName("audit_id") val auditId: String,
    /** When the request was handled. */
    @SerialName("timestamp") val timestamp: Instant,
    /**
     * Which key acted, as a short hex digest of it. The key itself is never written
     * here. `anonymous` when the request carried no key, which is what an unprotected
     * daemon looks like.
     */
    @SerialName("principal") val principal: String,
    /**
     * The end identity a proxy asserted for this request. Always empty in this build.
     * The field exists from the first release so that proxy identity can be added
     * later without a migration.
     */
    @SerialName("subject") val subject: String,
    /**
     * What was done, named as the endpoint id: `get_trace`, `list_spans`,
     * `delete_trace`.
     */
    @SerialName("action") val action: String,
    /** The project the request was made against. */
    @SerialName("project") val project: String,
    /** Whether authorisation allowed the request or refused it. */
    @SerialName("result") val result: AuditResult,
    /** The trace or dataset the request named, when it named one. */
    @SerialName("resource") val resource: String? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "audit_log"
    }
}

/**
 * A stored definition. Immutable per version: a run pins the version it started
 * against, and that version never changes underneath it. Stored in the
 * `definitions` table.
 */
@Serializable
data class Definition(
    /** Which project owns it. */
    @SerialName("project") val project: String,
    /** Its identifier within the project. */
    @SerialName("id") val id: String,
    /**
     * Its version. A run started without one pins the highest stored version at that
     * moment.
     */
    @SerialName("version") val version: Int,
    /** What kind of definition it is. */
    @SerialName("kind") val kind: DefinitionKind,
    /** The document as stored, minus `version`. */
    @SerialName("body") val body: JsonElement,
    /** When this version was stored. */
    @SerialName("created_at") val createdAt: Instant
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "definitions"
    }
}

/**
 * One execution of one definition version. The daemon owns the run between steps:
 * its journal, its deadlines, its waits, its cancel and its trace. Your own
 * process owns everything inside a step. Stored in the `runs` table.
 */
@Serializable
data class Run(
    /** Its identifier, minted at start. */
    @SerialName("run_id") val runId: String,
    /** Which project owns it. */
    @SerialName("project") val project: String,
    /** Which definition it runs. */
    @SerialName("definition_id") val definitionId: String,
    /** Which version. Pinned at start and never re-resolved. */
    @SerialName("definition_version") val definitionVersion: Int,
    /** Where it is. */
    @SerialName("status") val status: RunStatus,
    /** The frontier: where in the graph the run has reached. */
    @SerialName("cursor") val cursor: RunCursor,
    /**
     * The context stored at start and copied onto every span the daemon writes for the
     * run, with `run_id` set to this run's id.
     */
    @SerialName("correlation") val correlation: Correlation,
    /**
     * The trace every span of this run belongs to. 32 lowercase hex characters, minted
     * at start.
     */
    @SerialName("trace_id") val traceId: String,
    /**
     * How many steps this run has completed. Counted against `max_steps` on the root
     * of the run tree.
     */
    @SerialName("steps_used") val stepsUsed: Int,
    /** What the run's steps have cost so far, priced from the usage workers reported. */
    @SerialName("cost_usd") val costUsd: Double,
    /**
     * The root of the run tree, which ceilings are enforced against. Equal to `run_id`
     * on a root.
     */
    @SerialName("root_run_id") val rootRunId: String,
    /** How many parents lie between this run and the root. Zero on a root. */
    @SerialName("depth") val depth: Int,
    /** When it started. */
    @SerialName("created_at") val createdAt: Instant,
    /** When it last moved. */
    @SerialName("updated_at") val updatedAt: Instant,
    /**
     * What it was started with, after validation against the definition's
     * `input_schema`.
     */
    @SerialName("input") val input: JsonElement? = null,
    /**
     * What the definition's `output` template rendered when the run reached the end of
     * the graph.
     */
    @SerialName("output") val output: JsonElement? = null,
    /**
     * A JSON object merged shallowly on every `complete` that sends one. Later steps
     * read it through the lease.
     */
    @SerialName("state") val state: JsonElement? = null,
    /** Why it failed, when it did: the failing step's message, or the ceiling it spent. */
    @SerialName("error") val error: String? = null,
    /**
     * The step ceiling from the definition's `limits`, when it declares one. On a root
     * it bounds the whole tree.
     */
    @SerialName("max_steps") val maxSteps: Int? = null,
    /**
     * The spend ceiling from `limits`, when declared. A claim refused for a spent
     * ceiling ends the run `failed`.
     */
    @SerialName("max_cost_usd") val maxCostUsd: Double? = null,
    /**
     * When `limits.run_timeout_ms` passes. Absent when the definition declares no
     * timeout, and once the run has ended.
     */
    @SerialName("deadline_at") val deadlineAt: Instant? = null,
    /**
     * The key the start request carried, when it carried one. A repost of it within 24
     * hours returns this run.
     */
    @SerialName("idempotency_key") val idempotencyKey: String? = null,
    /** The run whose `subworkflow` step started this one. */
    @SerialName("parent_run_id") val parentRunId: String? = null,
    /** The step in the parent that this run is the body of. */
    @SerialName("parent_step_key") val parentStepKey: String? = null,
    /** When it reached a terminal status. */
    @SerialName("ended_at") val endedAt: Instant? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "runs"
    }
}

/**
 * One attempt at one step of a run. A run's journal is these rows, append only: a
 * retry is a new row, never a rewrite of the old one. Stored in the
 * `step_attempts` table.
 */
@Serializable
data class StepAttempt(
    /** Which run. */
    @SerialName("run_id") val runId: String,
    /**
     * Which step, with any enclosing branch, item or iteration folded into the key:
     * `refund/issue`, `classify[3]`, `poll[7]/classify`.
     */
    @SerialName("step_key") val stepKey: String,
    /**
     * Which attempt this is. The fence: a lease for an earlier attempt is refused once
     * a later one exists.
     */
    @SerialName("attempt") val attempt: Int,
    /** What kind of step. */
    @SerialName("kind") val kind: StepKind,
    /** What a duplicated body would cost. */
    @SerialName("effect") val effect: Effect,
    /** Where the attempt is. */
    @SerialName("status") val status: AttemptStatus,
    /**
     * The span the daemon opened for this attempt. 16 lowercase hex characters, minted
     * when the row was written. A worker's own spans nest under it.
     */
    @SerialName("span_id") val spanId: String,
    /** When the row was written. */
    @SerialName("created_at") val createdAt: Instant,
    /** The rendered input handed to the worker, on a claimable step. */
    @SerialName("input") val input: JsonElement? = null,
    /** What the worker returned on `complete`, or the event payload on a resumed wait. */
    @SerialName("output") val output: JsonElement? = null,
    /** Why it failed, when it did. */
    @SerialName("error") val error: StepError? = null,
    /** The token usage the worker reported on `complete`. */
    @SerialName("usage") val usage: StepUsage? = null,
    /** What the daemon priced that usage at. */
    @SerialName("cost_usd") val costUsd: Double? = null,
    /** The `worker_id` holding the lease, while one does. */
    @SerialName("lease_owner") val leaseOwner: String? = null,
    /** When the lease lapses without a heartbeat. */
    @SerialName("lease_until") val leaseUntil: Instant? = null,
    /**
     * When the worker posted `started`. Absent when it never did, which is what the
     * reclaim rule reads.
     */
    @SerialName("started_at") val startedAt: Instant? = null,
    /** When the lease lapsed. */
    @SerialName("orphaned_at") val orphanedAt: Instant? = null,
    /** When the reclaim window closes. */
    @SerialName("reclaim_until") val reclaimUntil: Instant? = null,
    /** How many attempts the node's retry budget still allows after this one. */
    @SerialName("attempts_left") val attemptsLeft: Int? = null,
    /** When it reached a terminal status. */
    @SerialName("ended_at") val endedAt: Instant? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "step_attempts"
    }
}

/**
 * One row of a run's event log. Written in the transaction that made the change it
 * describes, with `seq` allocated from the run header, so a reader that has seen a
 * row has seen every row below it. Stored in the `run_events` table.
 */
@Serializable
data class RunEvent(
    /** Which run. */
    @SerialName("run_id") val runId: String,
    /**
     * Its position in this run's stream, from zero and without gaps. Pass it as
     * `after` to reattach.
     */
    @SerialName("seq") val seq: Long,
    /** What happened. */
    @SerialName("kind") val kind: RunEventKind,
    /** When it was written. */
    @SerialName("at") val at: Instant,
    /** The detail, shaped by `kind`. */
    @SerialName("payload") val payload: JsonElement? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "run_events"
    }
}

/**
 * An axis that traces are described along. An aspect is a definition and not a
 * measurement: it carries the name a note is filed under, the words an operator
 * reads on screen and, where a model writes the sentence, the instruction handed
 * to whatever produces it. Four ship with the daemon as ordinary rows rather than
 * as a list compiled into a client, so there is one source of truth for what
 * exists: goal, what the user wanted; sentiment, the tone they expressed;
 * behaviour, what the agent did about it; and outcome, how the interaction ended.
 * A stored version never changes, so a note recorded a month ago still names the
 * definition it was written against. Stored in the `aspects` table.
 */
@Serializable
data class Aspect(
    /**
     * Which project owns it. Aspects are project-scoped, like every other tenancy
     * boundary here.
     */
    @SerialName("project") val project: String,
    /**
     * The aspect's name, lowercase with underscores, such as `goal` or
     * `refund_reason`. This is what a note is filed under and what every query
     * parameter names.
     */
    @SerialName("name") val name: String,
    /**
     * The display name, such as `Goal`. Stored rather than derived from the name, so
     * an aspect reads the same in every screen and every language of SDK.
     */
    @SerialName("label") val label: String,
    /**
     * One line saying what this axis is about, shown beside the label so a reader does
     * not have to infer the axis from its name.
     */
    @SerialName("description") val description: String,
    /**
     * Whether the daemon writes this aspect's notes itself, with no model anywhere.
     * `outcome` ships derived, written from the run's status, the step error class,
     * the silent producer flag on the cause report and the divergence watch, so a
     * deployment with nothing configured still fills one aspect and can see the whole
     * pipeline work before a model is wired to anything.
     */
    @SerialName("derived") val derived: Boolean,
    /**
     * Whether the daemon seeded this aspect. A built-in is a row like any other and
     * can be switched off, but it cannot be deleted, because notes already name it.
     */
    @SerialName("built_in") val builtIn: Boolean,
    /**
     * Whether notes are written along this aspect in this project. It is configuration
     * and read-only here. Switching an aspect on affects notes recorded afterwards and
     * never rewrites what is stored.
     */
    @SerialName("enabled") val enabled: Boolean,
    /**
     * Where this aspect sits in every list and in the default column order, ascending.
     * Stored, so the order an operator learns is the order they keep.
     */
    @SerialName("sort_order") val sortOrder: Int,
    /**
     * This definition's version. Editing an aspect stores a new version rather than
     * changing this one, so analysis already recorded is never rewritten under a
     * reader.
     */
    @SerialName("version") val version: Int,
    /** When this version was stored. */
    @SerialName("created_at") val createdAt: Instant,
    /**
     * The text handed to whatever writes a note along this aspect, at most 2000 bytes.
     * The daemon stores it and hands it out; what reads it and writes the sentence
     * sits outside the daemon. Absent when `derived` is true, because a derived aspect
     * needs no instruction.
     */
    @SerialName("instruction") val instruction: String? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "aspects"
    }
}

/**
 * One sentence recorded against one trace along one aspect. A note proves one
 * thing: that this sentence was recorded against that trace by the definition
 * named in `aspect`, at the version in `definition_version`. It does not prove the
 * sentence is true. Nothing here is checked against the trace it describes, so a
 * note is evidence of what a definition said and not evidence of what happened,
 * and the trace is what settles the difference. The daemon also keeps a small
 * sketch of the wording, which is what clustering compares, and that sketch is
 * deliberately not a field on this entity: it is a detail of how the pass is
 * stored, a caller can do nothing with it, and publishing it would fix the hashing
 * where it stands today. The text is payload. It passes through the same redactor
 * as every other signal on the way in, retention sweeps it, and erasing a subject
 * erases it. Stored in the `aspect_notes` table.
 */
@Serializable
data class AspectNote(
    /**
     * Unique id of the note. This identifies the sentence and never the run: a note id
     * and a trace id are different things and conflating them is the mistake this
     * separate name exists to prevent.
     */
    @SerialName("note_id") val noteId: String,
    /** The trace this sentence is about. */
    @SerialName("trace_id") val traceId: String,
    /** Which aspect this note is filed under, matching `Aspect.name`. */
    @SerialName("aspect") val aspect: String,
    /**
     * The sentence, after redaction. It is the only text the read API returns for a
     * note, and it is what the clustering pass reads.
     */
    @SerialName("text") val text: String,
    /**
     * The version of the aspect definition this note was written against. Editing a
     * definition mints a new version, so an older note keeps naming what was actually
     * asked for at the time.
     */
    @SerialName("definition_version") val definitionVersion: Int,
    /**
     * When the note was recorded. Writing a note lags the trace it describes,
     * sometimes by a long way, so a trace can be visible for hours before it carries
     * one.
     */
    @SerialName("created_at") val createdAt: Instant,
    /**
     * Inherited from the trace, so a note is scoped, filtered, retained and erased
     * exactly as its trace is.
     */
    @SerialName("correlation") val correlation: Correlation,
    /**
     * Which model wrote the sentence, when one did. Absent for a derived aspect, where
     * the daemon wrote the note itself.
     */
    @SerialName("model") val model: String? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "aspect_notes"
    }
}

/**
 * One clustering pass over one entity, covering a moving window of notes. A
 * snapshot is what turns a clustering into a series: every theme belongs to
 * exactly one, and every theme query names one. Windows overlap, so counts from
 * two snapshots must never be added together. A trace whose note falls inside
 * three windows is counted once in each of them, and summing the three counts it
 * three times. Compare snapshots or read one; do not sum them. Reads never
 * recompute either: a pass writes a snapshot on its own timer and every read is a
 * lookup of what it wrote, so refreshing a screen shows the same numbers rather
 * than a fresh clustering. Stored in the `theme_snapshots` table.
 */
@Serializable
data class ThemeSnapshot(
    /**
     * Opaque id, echoed back unchanged. Never build one by hand and never read meaning
     * into its shape.
     */
    @SerialName("snapshot_id") val snapshotId: String,
    /** Which project this pass covered. */
    @SerialName("project") val project: String,
    /** The kind of thing this pass covered. */
    @SerialName("entity_kind") val entityKind: EntityKind,
    /**
     * The name of that thing. An entity is the pair of a kind and a name, which spans
     * already carry, so nothing new is assigned to identify one.
     */
    @SerialName("entity_name") val entityName: String,
    /**
     * The aspects this pass covered, in catalogue order. An aspect with too few notes
     * when the pass ran is absent, so a missing name means it is still collecting and
     * not that anything failed.
     */
    @SerialName("aspects") val aspects: List<String>,
    /**
     * Where this snapshot falls in the entity's series, counting from one at the
     * oldest snapshot still stored.
     */
    @SerialName("ordinal") val ordinal: Long,
    /**
     * When this snapshot became the current state. A timeline is ordered by this and
     * not by the window bounds, because two passes can cover overlapping windows.
     */
    @SerialName("cutoff_at") val cutoffAt: Instant,
    /** Start of the window of notes read, inclusive. */
    @SerialName("window_from") val windowFrom: Instant,
    /** End of the window of notes read, exclusive. */
    @SerialName("window_to") val windowTo: Instant,
    /** How many distinct traces the notes read belong to. */
    @SerialName("trace_count") val traceCount: Long,
    /**
     * How many notes the pass actually read. Every coverage figure in this snapshot is
     * a share of this number and never of the number of notes in the window, which is
     * what makes `capped` worth reading.
     */
    @SerialName("note_count") val noteCount: Long,
    /**
     * Whether the pass reached its ceiling of 20,000 notes and clustered the newest of
     * them rather than all of them. True means the window holds more notes than were
     * read, so the denominator of every share here is `note_count` and not the window.
     */
    @SerialName("capped") val capped: Boolean
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "theme_snapshots"
    }
}

/**
 * A cluster of aspect notes that share their wording, in one snapshot along one
 * aspect. A theme groups aspect notes that share their wording, which is why mcpd
 * needs no model and no embedding to build one. It does not read the sentences, so
 * two notes that mean the same thing in different words land in separate themes,
 * and a theme's label is the note most like the others rather than a summary
 * written for it. The clustering is a pure function of the notes it reads, with no
 * floating-point arithmetic in the comparison, so two replicas of one deployment
 * reading the same notes write the same themes and cannot disagree about where a
 * boundary falls. Stored in the `themes` table.
 */
@Serializable
data class Theme(
    /**
     * Opaque id, stable across snapshots, which is what makes a history possible. A
     * theme keeps its id while its wording holds, so the same id in two snapshots is
     * the same theme. Nothing is encoded in the shape of it.
     */
    @SerialName("theme_id") val themeId: String,
    /** The snapshot this theme belongs to. */
    @SerialName("snapshot_id") val snapshotId: String,
    /**
     * Which aspect it was clustered along. Clustering runs once per aspect, so one
     * trace can sit in a different theme in each of them.
     */
    @SerialName("aspect") val aspect: String,
    /** The kind of entity this theme was built for. */
    @SerialName("entity_kind") val entityKind: EntityKind,
    /** The name of that entity. */
    @SerialName("entity_name") val entityName: String,
    /**
     * The theme's label, which is the text of one of its own notes: the note most like
     * the rest of them. Nothing wrote it for the screen, so it reads as one real
     * sentence about one real trace rather than as a description of the group, and it
     * can change between snapshots as the group does.
     */
    @SerialName("label") val label: String,
    /** The note the label is taken from. */
    @SerialName("label_note_id") val labelNoteId: String,
    /**
     * The trace that note was written about, so a reader can open the run behind the
     * label instead of taking the label's word for it.
     */
    @SerialName("label_trace_id") val labelTraceId: String,
    /** What happened to this theme between the previous snapshot and this one. */
    @SerialName("state") val state: ThemeState,
    /** How many notes are in this theme. */
    @SerialName("note_count") val noteCount: Long,
    /** How many distinct traces those notes were written about. */
    @SerialName("trace_count") val traceCount: Long,
    /**
     * This theme's share of the notes the snapshot read for this aspect, between 0 and
     * 1. The denominator is the snapshot's `note_count`, so in a capped snapshot it is
     * a share of what was read.
     */
    @SerialName("coverage") val coverage: Double
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "themes"
    }
}

/**
 * One version of a named prompt. A version never changes once stored: a new body
 * is a new version, and labels such as `production` and `staging` point at the
 * version a caller should use. SDKs fetch a prompt by name and label, render it
 * with variables, and link the spans that used it, so a trace shows which version
 * produced an answer. Stored in the `documents` table.
 */
@Serializable
data class Prompt(
    /** Which project owns it. */
    @SerialName("project") val project: String,
    /**
     * The prompt's name, unique within the project. Letters, digits, `-`, `_`, `.` and
     * `/`, up to 128 characters.
     */
    @SerialName("name") val name: String,
    /** Its version, from 1, assigned by the daemon in the order versions arrive. */
    @SerialName("version") val version: Int,
    /** Whether the body is one template or a list of messages. */
    @SerialName("kind") val kind: PromptKind,
    /** Every variable the body names, in order of first use, without duplicates. */
    @SerialName("variables") val variables: List<String>,
    /** The labels that point at this version now. */
    @SerialName("labels") val labels: List<String>,
    /** When this version was stored. */
    @SerialName("created_at") val createdAt: Instant,
    /** The template, for a `text` prompt. Variables are written `{{name}}`. */
    @SerialName("template") val template: String? = null,
    /**
     * The messages, for a `chat` prompt: an array of objects with `role` and
     * `content`, where `content` is a template.
     */
    @SerialName("messages") val messages: JsonElement? = null,
    /**
     * Model settings stored with the prompt, such as `model` and `temperature`. The
     * daemon stores them and does not read them.
     */
    @SerialName("config") val config: JsonElement? = null,
    /** What changed in this version, written by whoever stored it. */
    @SerialName("note") val note: String? = null,
    /** Which key or account stored it. */
    @SerialName("created_by") val createdBy: String? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "documents"
    }
}

/**
 * A scorer the project defined: a judge prompt written for this agent, or a code
 * scorer with its settings fixed. Stored as numbered versions; a version never
 * changes. Name it in `scorers` wherever a scorer id is taken, as `id` for the
 * latest version or `id@3` for version 3. Stored in the `documents` table.
 */
@Serializable
data class ScorerDefinition(
    /** Which project owns it. */
    @SerialName("project") val project: String,
    /**
     * Its id, unique in the project: lower-case letters, digits and `_`, up to 64. It
     * cannot be the id of a scorer mcpd ships.
     */
    @SerialName("id") val id: String,
    /** Its version, from 1, assigned by the daemon. */
    @SerialName("version") val version: Int,
    /** Judged by the model, or computed by code. */
    @SerialName("kind") val kind: ScorerDefinitionKind,
    /** What it measures, in a sentence. Shown in the scorer catalogue. */
    @SerialName("description") val description: String,
    /** Which way is better. Comparisons and live alerts read it. */
    @SerialName("direction") val direction: ScoreDirection,
    /** When this version was stored. */
    @SerialName("created_at") val createdAt: Instant,
    /**
     * For a `judge` definition: the criterion the model grades against, such as `The
     * answer cites the refund policy by section number.` The daemon adds the run's
     * input and output and asks for a value from 0 to 1 with a one-sentence reason.
     */
    @SerialName("prompt") val prompt: String? = null,
    /** For a `code` definition: the code scorer it runs, such as `keyword_coverage`. */
    @SerialName("builtin") val builtin: String? = null,
    /** For a `code` definition: the settings the code scorer runs with. */
    @SerialName("config") val config: ScorerConfig? = null,
    /** What changed in this version. */
    @SerialName("note") val note: String? = null,
    /** Which key or account stored it. */
    @SerialName("created_by") val createdBy: String? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "documents"
    }
}

/**
 * A rule for live scoring: which finished runs to score, with which scorers, and
 * how many of them. The daemon reads finished runs as they arrive and scores the
 * sampled ones in the background, so a quality drop shows up in minutes rather
 * than at the next offline pass. Scores it writes have source `live`. Stored in
 * the `documents` table.
 */
@Serializable
data class EvaluationRule(
    /** Which project it scores. */
    @SerialName("project") val project: String,
    /** Its id, unique in the project. */
    @SerialName("id") val id: String,
    /** Its version, from 1. Each change is a new version. */
    @SerialName("version") val version: Int,
    /** Whether it runs. A rule is stored disabled unless the request says otherwise. */
    @SerialName("enabled") val enabled: Boolean,
    /**
     * The scorers to run: shipped ids, criteria from the config file, or stored
     * definitions as `id` or `id@version`.
     */
    @SerialName("scorers") val scorers: List<String>,
    /**
     * The share of matching runs to score, from 0 to 1. Chosen by a hash of the trace
     * id, so the same run is in or out on every replica and every restart.
     */
    @SerialName("sample_rate") val sampleRate: Double,
    /** Only runs carrying all of these tags. */
    @SerialName("tags") val tags: List<String>,
    /**
     * The most judged scores the rule may produce in one minute, across the daemon.
     * Each one is a model call, so this caps what the rule can spend. Runs over it are
     * skipped, not queued.
     */
    @SerialName("max_judged_per_minute") val maxJudgedPerMinute: Int,
    /** When this version was stored. */
    @SerialName("created_at") val createdAt: Instant,
    /** Only runs whose root step has this name. */
    @SerialName("name") val name: String? = null,
    /** Only runs whose root step is this kind. */
    @SerialName("kind") val kind: SpanKind? = null,
    /** Only runs from this environment. */
    @SerialName("environment") val environment: String? = null,
    /** Only runs with this outcome. */
    @SerialName("status") val status: SpanStatus? = null,
    /** Settings for the code scorers that need them. */
    @SerialName("config") val config: ScorerConfig? = null,
    /** What changed in this version. */
    @SerialName("note") val note: String? = null,
    /** Which key or account stored it. */
    @SerialName("created_by") val createdBy: String? = null
) {
    companion object {
        /** The storage table this entity lives in. */
        const val TABLE = "documents"
    }
}
