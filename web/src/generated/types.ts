// GENERATED FILE. DO NOT EDIT.
//
// Produced from spec/mcpd.spec.json by tools/codegen.
// Edit the spec and run `make gen`. `make check` fails if this file drifts.


// Field names here are the daemon's own, because this model is used with
// raw `fetch` and no conversion layer.

/**
 * The version of `spec/mcpd.spec.json` these types were generated from.
 *
 * A daemon reports the same string on `/healthz`. Compare the two at connect
 * time. A mismatch caught there names the problem; the same mismatch found
 * later shows up as a field missing for no visible reason.
 */
export const specVersion: string = '1.0.0'

/**
  * What an operation *is*. The kind drives how a span is rendered in the waterfall,
  * which attributes are meaningful, and which metrics are auto-extracted from it. */
export const SpanKind = {
  // One agent invocation. Usually the root of a trace.
  AgentRun: 'agent_run',
  // One logical generation: everything from the first request to the final answer,
  // including every intermediate step.
  ModelGeneration: 'model_generation',
  // One step inside a generation. A generation that calls tools twice has three steps.
  ModelStep: 'model_step',
  // The provider round-trip alone, excluding processors and tool execution. Measure pure
  // model latency here.
  ModelInference: 'model_inference',
  // A single streamed chunk. Recorded as an event, not a duration.
  ModelChunk: 'model_chunk',
  // A tool your code executes.
  ToolCall: 'tool_call',
  // A tool the calling client executes. The server records the marker; the client
  // returns the detail.
  ClientToolCall: 'client_tool_call',
  // A tool the model provider executes server-side, such as hosted web search or code
  // execution.
  ProviderToolCall: 'provider_tool_call',
  // A tool reached over the Model Context Protocol.
  McpToolCall: 'mcp_tool_call',
  // One workflow execution. Usually the root of a trace.
  WorkflowRun: 'workflow_run',
  // One step within a workflow.
  WorkflowStep: 'workflow_step',
  // A branch point, with the conditions it evaluated.
  WorkflowConditional: 'workflow_conditional',
  // One condition evaluated inside a branch point.
  WorkflowConditionalEval: 'workflow_conditional_eval',
  // A fan-out of concurrent branches.
  WorkflowParallel: 'workflow_parallel',
  // A loop construct: foreach, do-while or do-until.
  WorkflowLoop: 'workflow_loop',
  // A deliberate pause.
  WorkflowSleep: 'workflow_sleep',
  // A wait for an external event, with its timeout.
  WorkflowWaitEvent: 'workflow_wait_event',
  // A read from or write to agent memory.
  MemoryOperation: 'memory_operation',
  // One ingestion pipeline run: load, chunk, extract, embed, upsert.
  RagIngestion: 'rag_ingestion',
  // One embedding call, possibly batched.
  RagEmbedding: 'rag_embedding',
  // Vector store I/O: query, upsert, delete or fetch.
  RagVectorOperation: 'rag_vector_operation',
  // A RAG-specific transform: chunk, extract metadata or rerank.
  RagAction: 'rag_action',
  // A graph build, traverse, update or prune.
  GraphAction: 'graph_action',
  // An input or output processor.
  ProcessorRun: 'processor_run',
  // One scorer evaluating a target.
  ScorerRun: 'scorer_run',
  // One stage of a scorer's pipeline.
  ScorerStep: 'scorer_step',
  // A safety or policy check, and whether it tripped.
  Guardrail: 'guardrail',
  // A generic retrieval that is not vector-store specific.
  Retrieval: 'retrieval',
  // An inline data transform between pipeline stages.
  Mapping: 'mapping',
  // A dynamic resolver deciding which skills an agent may use.
  SkillResolution: 'skill_resolution',
  // A filesystem, sandbox, search or computer-use action.
  WorkspaceAction: 'workspace_action',
  // An outbound HTTP call made on the agent's behalf.
  HttpRequest: 'http_request',
  // Anything else. Always valid; never inferred.
  Generic: 'generic',
} as const

export type SpanKind = typeof SpanKind[keyof typeof SpanKind]

/** Every `SpanKind` value, in declaration order. */
export const ALL_SPAN_KIND: readonly SpanKind[] = ['agent_run', 'model_generation', 'model_step', 'model_inference', 'model_chunk', 'tool_call', 'client_tool_call', 'provider_tool_call', 'mcp_tool_call', 'workflow_run', 'workflow_step', 'workflow_conditional', 'workflow_conditional_eval', 'workflow_parallel', 'workflow_loop', 'workflow_sleep', 'workflow_wait_event', 'memory_operation', 'rag_ingestion', 'rag_embedding', 'rag_vector_operation', 'rag_action', 'graph_action', 'processor_run', 'scorer_run', 'scorer_step', 'guardrail', 'retrieval', 'mapping', 'skill_resolution', 'workspace_action', 'http_request', 'generic']

/**
  * The outcome of an operation. */
export const SpanStatus = {
  // Still running, or the producer never said.
  Unset: 'unset',
  // Completed successfully.
  Ok: 'ok',
  // Failed. `error` carries the detail.
  Error: 'error',
  // Abandoned deliberately, by a caller or a timeout.
  Cancelled: 'cancelled',
} as const

export type SpanStatus = typeof SpanStatus[keyof typeof SpanStatus]

/** Every `SpanStatus` value, in declaration order. */
export const ALL_SPAN_STATUS: readonly SpanStatus[] = ['unset', 'ok', 'error', 'cancelled']

/**
  * The six signals mcpd stores. Every one of them is correlated by trace id, most of
  * them by span id as well, and every one of them passes through the same redaction
  * pipeline. */
export const SignalType = {
  // A unit of work with a start and, usually, an end.
  Span: 'span',
  // A structured log record.
  Log: 'log',
  // A numeric observation.
  Metric: 'metric',
  // A machine evaluation of a span or trace.
  Score: 'score',
  // A human evaluation of a span or trace.
  Feedback: 'feedback',
  // One sentence recorded against a trace along one aspect. Stored, redacted and
  // retained like the other five, and erased with the subject it belongs to.
  AspectNote: 'aspect_note',
} as const

export type SignalType = typeof SignalType[keyof typeof SignalType]

/** Every `SignalType` value, in declaration order. */
export const ALL_SIGNAL_TYPE: readonly SignalType[] = ['span', 'log', 'metric', 'score', 'feedback', 'aspect_note']

/**
  * Severity, ordered. Filtering is always `>=`. */
export const LogLevel = {
  // Finer than debug. Off by default.
  Trace: 'trace',
  // Detail useful while diagnosing.
  Debug: 'debug',
  // Normal operation.
  Info: 'info',
  // Recoverable, but worth reading.
  Warn: 'warn',
  // The operation failed.
  Error: 'error',
  // The process cannot continue.
  Fatal: 'fatal',
} as const

export type LogLevel = typeof LogLevel[keyof typeof LogLevel]

/** Every `LogLevel` value, in declaration order. */
export const ALL_LOG_LEVEL: readonly LogLevel[] = ['trace', 'debug', 'info', 'warn', 'error', 'fatal']

/**
  * How a numeric observation aggregates. */
export const MetricKind = {
  // Monotonically increasing. Aggregates by sum.
  Counter: 'counter',
  // A point-in-time value. Aggregates by last, min, max or mean.
  Gauge: 'gauge',
  // A distribution. Aggregates by bucket, and answers quantiles.
  Histogram: 'histogram',
} as const

export type MetricKind = typeof MetricKind[keyof typeof MetricKind]

/** Every `MetricKind` value, in declaration order. */
export const ALL_METRIC_KIND: readonly MetricKind[] = ['counter', 'gauge', 'histogram']

/**
  * How mcpd decides which traces to keep. */
export const SamplingKind = {
  // Keep everything. The default, and correct until volume says otherwise.
  Always: 'always',
  // Keep nothing. Useful to disable a single project without redeploying.
  Never: 'never',
  // Keep a fixed fraction, decided once at the root so a trace is never half-recorded.
  Ratio: 'ratio',
  // Keep a fixed fraction, decided by hashing the trace id. Every collector in a fleet
  // makes the same decision without coordinating.
  Deterministic: 'deterministic',
  // Buffer the trace, then decide once it finishes. Keeps every error and every slow
  // trace regardless of rate.
  Tail: 'tail',
} as const

export type SamplingKind = typeof SamplingKind[keyof typeof SamplingKind]

/** Every `SamplingKind` value, in declaration order. */
export const ALL_SAMPLING_KIND: readonly SamplingKind[] = ['always', 'never', 'ratio', 'deterministic', 'tail']

/**
  * What happens to a value a redaction rule matched. */
export const RedactionMode = {
  // Remove the key entirely, as if it was never sent.
  Drop: 'drop',
  // Replace the whole value with a fixed token.
  Mask: 'mask',
  // Keep a documented prefix and suffix, mask the middle. Enough to recognise a value
  // without disclosing it.
  Partial: 'partial',
  // Replace with a keyed SHA-256 digest. Equal inputs stay equal, so you can still group
  // and join, but the original cannot be recovered.
  Hash: 'hash',
} as const

export type RedactionMode = typeof RedactionMode[keyof typeof RedactionMode]

/** Every `RedactionMode` value, in declaration order. */
export const ALL_REDACTION_MODE: readonly RedactionMode[] = ['drop', 'mask', 'partial', 'hash']

/**
  * Where a score came from. Kept separate from the scorer's identity so the same scorer
  * can run in more than one context. */
export const ScoreSource = {
  // Produced online, as the traced run happened.
  Live: 'live',
  // Produced later, by scoring stored traces.
  Offline: 'offline',
  // Produced by an experiment run against a dataset.
  Experiment: 'experiment',
  // Entered by a person, through the API or the UI.
  Manual: 'manual',
} as const

export type ScoreSource = typeof ScoreSource[keyof typeof ScoreSource]

/** Every `ScoreSource` value, in declaration order. */
export const ALL_SCORE_SOURCE: readonly ScoreSource[] = ['live', 'offline', 'experiment', 'manual']

/**
  * The shape of a human judgement. */
export const FeedbackKind = {
  // Binary. `value` is `up` or `down`.
  Thumbs: 'thumbs',
  // Ordinal. `value` is a number inside the scale the producer declares.
  Rating: 'rating',
  // The right answer. `value` is text.
  Correction: 'correction',
  // A label chosen from a set. `value` is the label.
  Tag: 'tag',
  // Free prose, with no score attached.
  Comment: 'comment',
} as const

export type FeedbackKind = typeof FeedbackKind[keyof typeof FeedbackKind]

/** Every `FeedbackKind` value, in declaration order. */
export const ALL_FEEDBACK_KIND: readonly FeedbackKind[] = ['thumbs', 'rating', 'correction', 'tag', 'comment']

/**
  * The named thing a span belongs to. Bounded and low-cardinality, so it is safe as a
  * metric label. */
export const EntityKind = {
  // An agent.
  Agent: 'agent',
  // A workflow.
  Workflow: 'workflow',
  // A tool.
  Tool: 'tool',
  // A model.
  Model: 'model',
  // A scorer.
  Scorer: 'scorer',
  // A processor.
  Processor: 'processor',
  // A RAG or data pipeline.
  Pipeline: 'pipeline',
  // A plain service with no richer identity.
  Service: 'service',
} as const

export type EntityKind = typeof EntityKind[keyof typeof EntityKind]

/** Every `EntityKind` value, in declaration order. */
export const ALL_ENTITY_KIND: readonly EntityKind[] = ['agent', 'workflow', 'tool', 'model', 'scorer', 'processor', 'pipeline', 'service']

/**
  * Where an experiment run has got to. */
export const ExperimentStatus = {
  // Created, not started.
  Pending: 'pending',
  // In progress.
  Running: 'running',
  // Every item completed.
  Succeeded: 'succeeded',
  // Aborted. `error` says why.
  Failed: 'failed',
  // Stopped by a person.
  Cancelled: 'cancelled',
} as const

export type ExperimentStatus = typeof ExperimentStatus[keyof typeof ExperimentStatus]

/** Every `ExperimentStatus` value, in declaration order. */
export const ALL_EXPERIMENT_STATUS: readonly ExperimentStatus[] = ['pending', 'running', 'succeeded', 'failed', 'cancelled']

/**
  * Whether authorisation let the request through. */
export const AuditResult = {
  // The key carried the scope the endpoint needs, so mcpd went on to handle the request.
  Allowed: 'allowed',
  // Authorisation refused the request before it reached storage. Recording the attempt
  // is the point of the row.
  Refused: 'refused',
} as const

export type AuditResult = typeof AuditResult[keyof typeof AuditResult]

/** Every `AuditResult` value, in declaration order. */
export const ALL_AUDIT_RESULT: readonly AuditResult[] = ['allowed', 'refused']

/**
  * Where a run is. Only `running` can move; the other four are terminal, and a terminal
  * run holds no deadline, no wait and no live attempt. */
export const RunStatus = {
  // Making progress, or waiting for something that will move it: a lease, a timer or an
  // event.
  Running: 'running',
  // Reached the end of the graph. `output` holds what the definition's `output` template
  // rendered, when it declares one.
  Succeeded: 'succeeded',
  // Ended on an error nothing handled: a step that failed under `on_error: fail_run`, a
  // spent ceiling, or a cancelled child. `error` says which.
  Failed: 'failed',
  // Ended because the run's own deadline passed.
  TimedOut: 'timed_out',
  // Ended because a caller asked, through the cancel endpoint or by cancelling an
  // ancestor run.
  Cancelled: 'cancelled',
} as const

export type RunStatus = typeof RunStatus[keyof typeof RunStatus]

/** Every `RunStatus` value, in declaration order. */
export const ALL_RUN_STATUS: readonly RunStatus[] = ['running', 'succeeded', 'failed', 'timed_out', 'cancelled']

/**
  * Where one attempt at one step is. Four states are live and four are terminal, and
  * the daemon never shows a live attempt on an ended run. */
export const AttemptStatus = {
  // On disk and offerable to a worker, once its retry timer, if any, has fired.
  Pending: 'pending',
  // Held by a worker that has not yet posted `started`.
  Leased: 'leased',
  // Held by a worker that has posted `started`, so a body is running.
  Started: 'started',
  // The lease expired with no heartbeat. Not offerable while the reclaim window is open:
  // a heartbeat inside the window restores the attempt, and a window that closes with no
  // contact requeues it or fails the run, depending on `effect` and on whether the body
  // had started.
  Orphaned: 'orphaned',
  // Finished with an output.
  Succeeded: 'succeeded',
  // Finished with an error, or marked failed because the run ended while the attempt was
  // live.
  Failed: 'failed',
  // The step's own deadline passed before the attempt ended.
  TimedOut: 'timed_out',
  // Never ran, because the step's `when` guard was false.
  Skipped: 'skipped',
} as const

export type AttemptStatus = typeof AttemptStatus[keyof typeof AttemptStatus]

/** Every `AttemptStatus` value, in declaration order. */
export const ALL_ATTEMPT_STATUS: readonly AttemptStatus[] = ['pending', 'leased', 'started', 'orphaned', 'succeeded', 'failed', 'timed_out', 'skipped']

/**
  * What a step attempt is an attempt at. `task` and `agent` are claimable. A `wait` row
  * is written by the event that resumes it or the deadline that ends it, so a wait's
  * output and status read like any other step's. */
export const StepKind = {
  // Claimable work. The worker runs whatever code it registered under this step id.
  Task: 'task',
  // Claimable work whose body is the worker's own agent loop. Identical on the wire to
  // `task`; the kind is recorded so the trace and the interface can tell an agent turn
  // from a plain step.
  Agent: 'agent',
  // A suspension the daemon resumes on an event or ends on a deadline. Never offered to
  // a worker.
  Wait: 'wait',
} as const

export type StepKind = typeof StepKind[keyof typeof StepKind]

/** Every `StepKind` value, in declaration order. */
export const ALL_STEP_KIND: readonly StepKind[] = ['task', 'agent', 'wait']

/**
  * What a run's cursor names: the construct at its current position in the definition's
  * `steps`. Every kind except `end` is a node kind a definition can declare. */
export const ConstructKind = {
  // Claimable work.
  Task: 'task',
  // Claimable work whose body is the worker's agent loop.
  Agent: 'agent',
  // A relative delay, in milliseconds. No process is alive during it.
  Sleep: 'sleep',
  // An absolute delay, to an RFC 3339 instant rendered from a template.
  SleepUntil: 'sleep_until',
  // A suspension addressed by an event name and a key rendered per run.
  WaitEvent: 'wait_event',
  // A fixed fan-out over named branches, each branch one claimable step. Done when every
  // branch has ended.
  Parallel: 'parallel',
  // A fan-out over the items of an array, each item one claimable step, with at most
  // `concurrency` of them offerable at once.
  Foreach: 'foreach',
  // A bounded loop whose body is one claimable step per iteration, ended by `until` or
  // by `max_iterations`.
  Loop: 'loop',
  // A child run of another definition. Its output becomes this step's output, and a
  // cancel walks down into it.
  Subworkflow: 'subworkflow',
  // The end of the graph. Never declared in a definition: the cursor reaches it when the
  // last step has ended.
  End: 'end',
} as const

export type ConstructKind = typeof ConstructKind[keyof typeof ConstructKind]

/** Every `ConstructKind` value, in declaration order. */
export const ALL_CONSTRUCT_KIND: readonly ConstructKind[] = ['task', 'agent', 'sleep', 'sleep_until', 'wait_event', 'parallel', 'foreach', 'loop', 'subworkflow', 'end']

/**
  * What running a step body twice would cost, which is what the daemon branches on when
  * a lease is lost. Default `at_least_once`. */
export const Effect = {
  // Running the body twice is acceptable. A lost lease requeues the attempt. The
  // default.
  AtLeastOnce: 'at_least_once',
  // Running the body twice is not acceptable. `started` must be posted before the body
  // runs, and a started attempt whose lease is lost fails the run rather than running
  // again.
  AtMostOnce: 'at_most_once',
} as const

export type Effect = typeof Effect[keyof typeof Effect]

/** Every `Effect` value, in declaration order. */
export const ALL_EFFECT: readonly Effect[] = ['at_least_once', 'at_most_once']

/**
  * Every deadline the daemon keeps. Each one is a row the tick fires, and each firing
  * is a transaction that moves the run. */
export const TimerKind = {
  // A `sleep` or `sleep_until` coming due.
  Sleep: 'sleep',
  // A lease that has not been heartbeated. Firing moves the attempt to `orphaned` and
  // opens a reclaim window one lease length long.
  LeaseExpiry: 'lease_expiry',
  // The close of the reclaim window an expired lease opened. Firing requeues the attempt
  // or fails the run, depending on `effect` and on whether the body had started.
  Reclaim: 'reclaim',
  // The whole run's ceiling, from `limits.run_timeout_ms`.
  RunTimeout: 'run_timeout',
  // One attempt's ceiling, from the node's `step_timeout_ms`. It bounds a worker that
  // heartbeats for ever.
  StepTimeout: 'step_timeout',
  // A `wait_event` whose event has not arrived by `timeout_ms`.
  WaitTimeout: 'wait_timeout',
  // The backoff before a requeued attempt becomes claimable. Firing writes the next
  // attempt as `pending`.
  Retry: 'retry',
} as const

export type TimerKind = typeof TimerKind[keyof typeof TimerKind]

/** Every `TimerKind` value, in declaration order. */
export const ALL_TIMER_KIND: readonly TimerKind[] = ['sleep', 'lease_expiry', 'reclaim', 'run_timeout', 'step_timeout', 'wait_timeout', 'retry']

/**
  * What a row in a run's event log records. Every row is written in the transaction
  * that made the change it describes, so a reader that has seen sequence number n has
  * seen every change below it. */
export const RunEventKind = {
  // The run was created. Payload: `definition_id`, `definition_version`.
  RunStarted: 'run_started',
  // The cursor moved. Payload: `index`, `construct`, and `step_key` when the construct
  // has one.
  RunCursor: 'run_cursor',
  // A worker claimed an attempt. Payload: `step_key`, `attempt`, `worker_id`.
  StepLeased: 'step_leased',
  // A worker posted `started`. Payload: `step_key`, `attempt`.
  StepStarted: 'step_started',
  // An attempt succeeded. Payload: `step_key`, `attempt`.
  StepFinished: 'step_finished',
  // An attempt failed. Payload: `step_key`, `attempt`, `class`, `message`,
  // `attempts_left`.
  StepFailed: 'step_failed',
  // A `when` guard was false. Payload: `step_key`.
  StepSkipped: 'step_skipped',
  // The run reached a `wait_event`. Payload: `step_key`, `event`, `key`, `expires_at`.
  RunWaiting: 'run_waiting',
  // An event resumed a waiting run. Payload: `step_key`, `event_id`.
  RunResumed: 'run_resumed',
  // The run reached a terminal status. Payload: `status`, and `error` when there is one.
  RunEnded: 'run_ended',
} as const

export type RunEventKind = typeof RunEventKind[keyof typeof RunEventKind]

/** Every `RunEventKind` value, in declaration order. */
export const ALL_RUN_EVENT_KIND: readonly RunEventKind[] = ['run_started', 'run_cursor', 'step_leased', 'step_started', 'step_finished', 'step_failed', 'step_skipped', 'run_waiting', 'run_resumed', 'run_ended']

/**
  * What kind of thing a definition describes. */
export const DefinitionKind = {
  // A graph of steps the daemon walks. The only kind in version 1: `agent` definitions
  // arrive with the in-process agent loop, which is not served yet.
  Workflow: 'workflow',
} as const

export type DefinitionKind = typeof DefinitionKind[keyof typeof DefinitionKind]

/** Every `DefinitionKind` value, in declaration order. */
export const ALL_DEFINITION_KIND: readonly DefinitionKind[] = ['workflow']

/**
  * What the daemon did with a published event. */
export const EventOutcome = {
  // A waiting run took it and moved, in the same transaction.
  Delivered: 'delivered',
  // No run was waiting. The event is held for 24 hours for the next run that reaches a
  // matching wait with `accept_parked`.
  Parked: 'parked',
  // This event id has been seen before, so nothing was written. The publish endpoint
  // says when a derived id is treated this way.
  Duplicate: 'duplicate',
} as const

export type EventOutcome = typeof EventOutcome[keyof typeof EventOutcome]

/** Every `EventOutcome` value, in declaration order. */
export const ALL_EVENT_OUTCOME: readonly EventOutcome[] = ['delivered', 'parked', 'duplicate']

/**
  * Why a worker call about a lease answered 409. Each reason tells the worker something
  * different to do next. */
export const ConflictReason = {
  // A later attempt exists, or another lease terminated this attempt. Abandon the work
  // and post nothing more for this lease.
  Fenced: 'fenced',
  // This lease's own earlier `complete` or `fail` landed and its reply was lost. The
  // step is done; `attempt_status` says how. Post nothing more.
  AlreadyApplied: 'already_applied',
  // `complete` or `fail` arrived for a `leased` attempt under `at_most_once`. A bug in
  // the worker: post `started` first, and log it loudly.
  NotStarted: 'not_started',
  // The run has ended. Stop, and stop claiming for this run.
  RunTerminal: 'run_terminal',
} as const

export type ConflictReason = typeof ConflictReason[keyof typeof ConflictReason]

/** Every `ConflictReason` value, in declaration order. */
export const ALL_CONFLICT_REASON: readonly ConflictReason[] = ['fenced', 'already_applied', 'not_started', 'run_terminal']

/**
  * How a step failed, which decides whether the daemon retries it. */
export const StepErrorClass = {
  // Worth another attempt. The daemon retries while attempts remain, at the node's
  // backoff, and honours `retry_after_ms` when it is larger.
  Retryable: 'retryable',
  // Another attempt would fail the same way. No retry; the node's `on_error` applies.
  Fatal: 'fatal',
  // The step's input cannot be acted on. No retry; the node's `on_error` applies.
  InvalidInput: 'invalid_input',
  // This worker does not serve the definition version, or does not recognise the step
  // key. The attempt returns to `pending` after one lease length without spending the
  // retry budget, so another worker can take it.
  Unservable: 'unservable',
  // The worker gave the attempt up on request. No retry; the node's `on_error` applies.
  Cancelled: 'cancelled',
} as const

export type StepErrorClass = typeof StepErrorClass[keyof typeof StepErrorClass]

/** Every `StepErrorClass` value, in declaration order. */
export const ALL_STEP_ERROR_CLASS: readonly StepErrorClass[] = ['retryable', 'fatal', 'invalid_input', 'unservable', 'cancelled']

/**
  * What happens to a run when a node fails after its retries. Default `fail_run`. */
export const OnError = {
  // End the run `failed`, with the step's error message as the run's error.
  FailRun: 'fail_run',
  // Mark the step `failed` and move on, so a later `when` guard can read
  // `steps.<id>.status` and `steps.<id>.error`.
  Continue: 'continue',
} as const

export type OnError = typeof OnError[keyof typeof OnError]

/** Every `OnError` value, in declaration order. */
export const ALL_ON_ERROR: readonly OnError[] = ['fail_run', 'continue']

/**
  * How the delay between retries grows. */
export const Backoff = {
  // Every retry waits `initial_ms`.
  Fixed: 'fixed',
  // Each retry waits twice as long as the last, starting at `initial_ms` and capped at
  // `max_ms`.
  Exponential: 'exponential',
} as const

export type Backoff = typeof Backoff[keyof typeof Backoff]

/** Every `Backoff` value, in declaration order. */
export const ALL_BACKOFF: readonly Backoff[] = ['fixed', 'exponential']

/**
  * What a `wait_event` node does when its deadline passes with no event. Default
  * `fail`. */
export const WaitTimeout = {
  // The node fails. Its `on_error` decides what the run does next.
  Fail: 'fail',
  // The wait ends and the run moves on to the next node.
  Continue: 'continue',
} as const

export type WaitTimeout = typeof WaitTimeout[keyof typeof WaitTimeout]

/** Every `WaitTimeout` value, in declaration order. */
export const ALL_WAIT_TIMEOUT: readonly WaitTimeout[] = ['fail', 'continue']

/**
  * The JSON type of a value. Used where mcpd reports the shape of something it holds
  * without returning the thing itself. */
export const ValueKind = {
  // The value was JSON null.
  Null: 'null',
  // The value was `true` or `false`.
  Boolean: 'boolean',
  // The value was a JSON number.
  Number: 'number',
  // The value was a JSON string.
  String: 'string',
  // The value was a JSON array.
  Array: 'array',
  // The value was a JSON object.
  Object: 'object',
} as const

export type ValueKind = typeof ValueKind[keyof typeof ValueKind]

/** Every `ValueKind` value, in declaration order. */
export const ALL_VALUE_KIND: readonly ValueKind[] = ['null', 'boolean', 'number', 'string', 'array', 'object']

/**
  * What a monitor measures over its window. */
export const MonitorWatch = {
  // The fraction of runs that failed, 0 to 1.
  ErrorRate: 'error_rate',
  // Run duration at the ninety-fifth percentile, in milliseconds.
  LatencyP95: 'latency_p95',
  // Total spend across the window.
  Cost: 'cost',
  // How many runs happened. Catches a pipeline that has gone silent.
  TraceCount: 'trace_count',
  // The mean of the scorer named by `scorer_id`.
  ScoreMean: 'score_mean',
  // How far a run strays from the usual shape of its own definition. This watch sets no
  // threshold in steps, because a step count is never high in absolute terms, only high
  // for that definition. It reads the recent completed runs of one definition version,
  // takes the median step count as the usual shape, and reports a run whose modified
  // z-score against the median absolute deviation exceeds the monitor's bound, 3.5 by
  // default. The median absolute deviation is used rather than a mean and a standard
  // deviation because it tolerates up to half its sample being outliers, which is the
  // case here: a runaway cohort must not be allowed to redefine normal. Fewer than
  // twelve completed runs and the watch says nothing rather than guessing. A run that
  // takes several times its usual number of steps trips this long before it reaches
  // `max_steps`, and while its error rate is still zero.
  StepCountDivergence: 'step_count_divergence',
  // How much a run repeats itself. For each run in the window, the highest number of
  // attempts it made at any one node of its definition, counting a retry and a loop
  // iteration of that node alike; the watch reads the mean of that number. One means no
  // node ran twice. A loop that re-enters the same node, and a retry storm, both raise
  // this before either reaches `error_rate`.
  RunRepetition: 'run_repetition',
} as const

export type MonitorWatch = typeof MonitorWatch[keyof typeof MonitorWatch]

/** Every `MonitorWatch` value, in declaration order. */
export const ALL_MONITOR_WATCH: readonly MonitorWatch[] = ['error_rate', 'latency_p95', 'cost', 'trace_count', 'score_mean', 'step_count_divergence', 'run_repetition']

/**
  * Which way a monitor's threshold is breached. */
export const MonitorCompare = {
  // Trip when the value rises above the threshold.
  Above: 'above',
  // Trip when the value falls below it. This is the direction that catches a quality
  // regression, and a silent pipeline.
  Below: 'below',
} as const

export type MonitorCompare = typeof MonitorCompare[keyof typeof MonitorCompare]

/** Every `MonitorCompare` value, in declaration order. */
export const ALL_MONITOR_COMPARE: readonly MonitorCompare[] = ['above', 'below']

/**
  * Where a monitor or an objective stands. A tripped condition latches: it stays
  * tripped until the number recovers, and too little data never clears it, because
  * silence is not recovery. */
export const MonitorCondition = {
  // Evaluated, and within its threshold.
  Nominal: 'nominal',
  // Tripped, and not yet acknowledged.
  Tripped: 'tripped',
  // Tripped, acknowledged, and still breaching. Returns to `nominal` on its own when the
  // number recovers.
  Acknowledged: 'acknowledged',
  // Not enough data in the window to judge. Also the state before the first evaluation.
  Insufficient: 'insufficient',
} as const

export type MonitorCondition = typeof MonitorCondition[keyof typeof MonitorCondition]

/** Every `MonitorCondition` value, in declaration order. */
export const ALL_MONITOR_CONDITION: readonly MonitorCondition[] = ['nominal', 'tripped', 'acknowledged', 'insufficient']

/**
  * What the `q` parameter matches on this deployment's storage backend. Reported by
  * `GET /v1/api/capabilities` so a search box can say what it searches instead of
  * leaving a reader to infer it from a result set. */
export const TextSearch = {
  // `q` is not applied at all. No backend that ships declares this; a backend that did
  // would be telling an interface to hide its search box rather than let somebody type
  // into one that returns everything.
  None: 'none',
  // `q` matches anywhere inside a word, so `heck` finds `checkout`.
  Substring: 'substring',
  // `q` matches whole words only, which is what a person typing two words into a search
  // box expects. There is no stemming: `refund` does not find `refunds`.
  Words: 'words',
} as const

export type TextSearch = typeof TextSearch[keyof typeof TextSearch]

/** Every `TextSearch` value, in declaration order. */
export const ALL_TEXT_SEARCH: readonly TextSearch[] = ['none', 'substring', 'words']

/**
  * How the storage backend cuts each signal table on its anchor time. It decides
  * whether expiring a day of data is a metadata operation on one part or a row-by-row
  * delete. */
export const Partitioning = {
  // One table, no partitions. Retention removes rows.
  None: 'none',
  // One partition per day.
  Day: 'day',
  // One partition per month.
  Month: 'month',
} as const

export type Partitioning = typeof Partitioning[keyof typeof Partitioning]

/** Every `Partitioning` value, in declaration order. */
export const ALL_PARTITIONING: readonly Partitioning[] = ['none', 'day', 'month']

/**
  * A dimension a filter box can offer values for. The set is closed on purpose: each
  * one is a column or a parsed field the daemon keeps a distinct-value list for as
  * signals arrive, so answering is a read of a small table rather than a scan of every
  * span in the retention window. */
export const Facet = {
  // The kind of thing a step was: an agent run, a tool call, a retrieval, and so on.
  EntityKind: 'entity_kind',
  // The name of that thing, such as the tool that was called.
  EntityName: 'entity_name',
  // The service that reported the signal.
  ServiceName: 'service_name',
  // The environment it came from, such as `production`.
  Environment: 'environment',
  // A tag carried on a run.
  Tag: 'tag',
  // A metric name, for the name box of a chart.
  MetricName: 'metric_name',
  // A label key a metric point carries. Project-wide: these are every key seen in the
  // project, not the keys of one named metric.
  MetricLabelKey: 'metric_label_key',
  // A value one label key takes. Needs `key`, because the answer would otherwise be
  // every value of every label mixed together.
  MetricLabelValue: 'metric_label_value',
} as const

export type Facet = typeof Facet[keyof typeof Facet]

/** Every `Facet` value, in declaration order. */
export const ALL_FACET: readonly Facet[] = ['entity_kind', 'entity_name', 'service_name', 'environment', 'tag', 'metric_name', 'metric_label_key', 'metric_label_value']

/**
  * A signal that can be polled for what has arrived. Only the three that carry a
  * server-set arrival time: a score and a piece of feedback are stamped with the
  * caller's own timestamp, so a watermark over them would either skip a late row or
  * repeat every row for ever. */
export const DeltaSignal = {
  // The run projection, which is what a run list refreshes from.
  Traces: 'traces',
  // Steps, for a waterfall that is still filling in.
  Spans: 'spans',
  // Log lines.
  Logs: 'logs',
} as const

export type DeltaSignal = typeof DeltaSignal[keyof typeof DeltaSignal]

/** Every `DeltaSignal` value, in declaration order. */
export const ALL_DELTA_SIGNAL: readonly DeltaSignal[] = ['traces', 'spans', 'logs']

/**
  * How far the daemon has got with one aspect for one entity. It is computed from the
  * rows that are stored and never recorded, so it cannot go stale, and it is per entity
  * and per aspect rather than global: one agent can have goal themes to read while its
  * behaviour notes are still collecting. */
export const AspectReadiness = {
  // Fewer notes than a clustering pass needs. The floor is 50 notes for one aspect on
  // one entity, and below it the daemon writes no snapshot at all rather than sorting a
  // handful of sentences into themes nobody should draw a conclusion from.
  Collecting: 'collecting',
  // Enough notes to cluster, and no snapshot covers this aspect yet. The next pass will
  // write one. Nothing is wrong and nothing needs doing.
  Processing: 'processing',
  // A snapshot covers this aspect, so themes exist and every theme route answers.
  Ready: 'ready',
} as const

export type AspectReadiness = typeof AspectReadiness[keyof typeof AspectReadiness]

/** Every `AspectReadiness` value, in declaration order. */
export const ALL_ASPECT_READINESS: readonly AspectReadiness[] = ['collecting', 'processing', 'ready']

/**
  * What happened to a theme between the previous snapshot and this one. The daemon
  * matches themes across consecutive snapshots by their wording, so a theme keeps its
  * id while its wording holds, and this records what that match found. It is computed
  * from the two clusterings alone and never from a trace attribute. */
export const ThemeState = {
  // The first snapshot this theme appears in.
  Birth: 'birth',
  // Present in the previous snapshot and in this one, matched to the same id. The
  // ordinary state of a theme that is holding together.
  Continue: 'continue',
  // The previous snapshot held one theme where this one holds several. The history
  // point's `split_from` names the theme it came out of.
  Split: 'split',
  // Several themes in the previous snapshot are one theme here. The history point's
  // `merged_from` names them.
  Merge: 'merge',
  // Present in an earlier snapshot, absent from the one immediately before this, and
  // back now under its original id. A theme that comes and goes reads as one theme with
  // gaps rather than as several short-lived ones.
  Resurrection: 'resurrection',
  // The theme was in the previous snapshot and is in none of this one's clusters.
  // Reachable only through a theme's history: a dead theme has no row in the snapshot it
  // died in, so it is never a node in a flow.
  Death: 'death',
} as const

export type ThemeState = typeof ThemeState[keyof typeof ThemeState]

/** Every `ThemeState` value, in declaration order. */
export const ALL_THEME_STATE: readonly ThemeState[] = ['birth', 'continue', 'split', 'merge', 'resurrection', 'death']

/**
  * What one node of a theme flow stands for. A stage holds its themes, at most one
  * noise node and at most one overflow node, and the three together account for every
  * trace in the stage, which is what lets the shares add up to one. */
export const ThemeNodeKind = {
  // One theme. It carries a `theme_id`, so it can be filtered on, opened and followed
  // through its history.
  Theme: 'theme',
  // The notes in this stage that joined no theme. Noise is not a failure bucket: it
  // holds rare requests, wording nothing else matched and patterns that have not
  // recurred yet, and a large noise share means varied traffic or too few notes rather
  // than something going wrong. Its notes are read from the examples route with `noise`
  // as the theme id.
  Noise: 'noise',
  // The themes past `theme_limit_per_stage`, folded into one node so the stage totals
  // still add up. It carries no `theme_id` and cannot be selected, filtered on or
  // opened, because it is a presentation fold and not a group of anything.
  Other: 'other',
} as const

export type ThemeNodeKind = typeof ThemeNodeKind[keyof typeof ThemeNodeKind]

/** Every `ThemeNodeKind` value, in declaration order. */
export const ALL_THEME_NODE_KIND: readonly ThemeNodeKind[] = ['theme', 'noise', 'other']

/**
  * Why one snapshot is in a landmark selection. A timeline draws a couple of dozen
  * points over a range that may hold thousands of passes, so the daemon chooses them
  * and says why, rather than handing back the newest few and leaving the reader to
  * assume that is the whole story. */
export const SnapshotLandmarkReason = {
  // The oldest snapshot in the requested range. Always included, so a timeline starts
  // where the range does.
  RangeStart: 'range_start',
  // The newest snapshot in the requested range. Always included, so a timeline ends at
  // the current state.
  RangeEnd: 'range_end',
  // Chosen to spread the remaining points evenly over the range. Sampling is by time and
  // not by ordinal, so an hour of frequent passes does not crowd out a quiet week.
  TimeSample: 'time_sample',
} as const

export type SnapshotLandmarkReason = typeof SnapshotLandmarkReason[keyof typeof SnapshotLandmarkReason]

/** Every `SnapshotLandmarkReason` value, in declaration order. */
export const ALL_SNAPSHOT_LANDMARK_REASON: readonly SnapshotLandmarkReason[] = ['range_start', 'range_end', 'time_sample']

/**
  * What an account API key may do. The same six scopes the daemon's configured keys
  * carry, under the same names as the `auth` value of each endpoint. */
export const KeyScope = {
  // Write signals, and read nothing. The key that lives in every process that emits a
  // span.
  Ingest: 'ingest',
  // Read stored signals: runs, spans, logs, metrics, scores, analytics and datasets.
  Read: 'read',
  // Change stored data: scores, feedback, datasets, experiments and acknowledgements.
  Write: 'write',
  // Delete data and read the audit log. Also accepted where `execute` or `worker` is
  // needed.
  Admin: 'admin',
  // Start, cancel and signal runs, and publish events to them.
  Execute: 'execute',
  // Claim steps and report their outcome. The key that lives in the process running step
  // bodies.
  Worker: 'worker',
} as const

export type KeyScope = typeof KeyScope[keyof typeof KeyScope]

/** Every `KeyScope` value, in declaration order. */
export const ALL_KEY_SCOPE: readonly KeyScope[] = ['ingest', 'read', 'write', 'admin', 'execute', 'worker']

/**
  * What a member may do in an organisation. Each role maps onto the six key scopes on
  * every project the organisation owns, unless a project role says otherwise. */
export const OrgRole = {
  // Every scope on every project, and everything an admin may do. Only an owner may add
  // or remove an owner, change quotas or delete the organisation.
  Owner: 'owner',
  // Every scope on every project. Invites and removes members below owner, sets roles,
  // creates and deletes projects, and manages SSO, SCIM and every key in the
  // organisation.
  Admin: 'admin',
  // The `ingest`, `read`, `write`, `execute` and `worker` scopes. Everything but
  // `admin`: a member cannot delete data or read the audit log.
  Member: 'member',
  // The `read` scope only.
  Viewer: 'viewer',
} as const

export type OrgRole = typeof OrgRole[keyof typeof OrgRole]

/** Every `OrgRole` value, in declaration order. */
export const ALL_ORG_ROLE: readonly OrgRole[] = ['owner', 'admin', 'member', 'viewer']

/**
  * A role on one project that replaces a member's organisation role there. Owners and
  * admins keep their reach on every project whatever a project role says. */
export const ProjectRole = {
  // No access to this project.
  None: 'none',
  // The `read` scope on this project.
  Viewer: 'viewer',
  // Every scope but `admin` on this project.
  Member: 'member',
  // Every scope on this project. It does not let the member manage the organisation.
  Admin: 'admin',
} as const

export type ProjectRole = typeof ProjectRole[keyof typeof ProjectRole]

/** Every `ProjectRole` value, in declaration order. */
export const ALL_PROJECT_ROLE: readonly ProjectRole[] = ['none', 'viewer', 'member', 'admin']

/**
  * How an organisation's members log in with single sign-on. */
export const SsoProvider = {
  // Any OpenID Connect provider, such as Google, Microsoft Entra ID, Okta or Auth0,
  // found through its issuer's discovery document. Authorization code flow with PKCE,
  // state and nonce.
  Oidc: 'oidc',
  // GitHub, through an OAuth app. GitHub issues no ID token, so the member is the GitHub
  // user id and the email is the primary verified address GitHub reports.
  Github: 'github',
} as const

export type SsoProvider = typeof SsoProvider[keyof typeof SsoProvider]

/** Every `SsoProvider` value, in declaration order. */
export const ALL_SSO_PROVIDER: readonly SsoProvider[] = ['oidc', 'github']

/**
 * Input tokens broken down by how the provider billed them. Every field is optional;
 * absent means the provider did not report it, which is different from zero.
 */
export interface InputTokenDetails {
  /**
   * Ordinary text tokens: not cached, not audio, not image.
   */
  text?: number
  /**
   * Tokens served from a prompt cache. Usually billed at a large discount.
   */
  cache_read?: number
  /**
   * Tokens written into a prompt cache. Usually billed at a premium.
   */
  cache_write?: number
  /**
   * Tokens written to a five-minute ephemeral cache, where the provider prices TTLs
   * separately.
   */
  cache_write_5m?: number
  /**
   * Tokens written to a one-hour ephemeral cache.
   */
  cache_write_1h?: number
  /**
   * Audio input tokens.
   */
  audio?: number
  /**
   * Image input tokens, including rendered PDF pages.
   */
  image?: number
}

/**
 * Output tokens broken down by kind.
 */
export interface OutputTokenDetails {
  /**
   * Ordinary completion tokens.
   */
  text?: number
  /**
   * Hidden reasoning tokens. Billed, and usually invisible in the response.
   */
  reasoning?: number
  /**
   * Audio output tokens.
   */
  audio?: number
  /**
   * Image output tokens.
   */
  image?: number
}

/**
 * Token counts for one model call. Totals are authoritative; the detail breakdowns
 * explain them and are what cost is actually computed from.
 */
export interface TokenUsage {
  /**
   * Total input tokens.
   */
  input_tokens?: number
  /**
   * Total output tokens.
   */
  output_tokens?: number
  /**
   * Input plus output, when the provider reports a total that differs from the sum.
   */
  total_tokens?: number
  /**
   * How the input tokens were billed.
   */
  input_details?: InputTokenDetails
  /**
   * How the output tokens were billed.
   */
  output_details?: OutputTokenDetails
}

/**
 * What a span cost, in a single currency. mcpd computes this from a price book at
 * ingest and stores the result, so a later price change never silently rewrites
 * history.
 */
export interface Cost {
  /**
   * ISO 4217 code. `USD` unless the price book says otherwise.
   */
  currency: string
  /**
   * Cost attributable to input tokens.
   */
  input_cost?: number
  /**
   * Cost attributable to output tokens.
   */
  output_cost?: number
  /**
   * The sum, including any meter that is neither input nor output.
   */
  total_cost?: number
  /**
   * Identifier of the price book used, so a figure can be traced back to the rates that
   * produced it.
   */
  price_book?: string
  /**
   * True when mcpd derived the cost from a price book, false when the producer reported
   * it directly.
   */
  is_estimated: boolean
}

/** Spec-declared defaults for `Cost`. Spread these when constructing one. */
export const costDefaults = {
  currency: "USD",
  is_estimated: true,
} satisfies Partial<Cost>

/**
 * The sampling settings a caller chose for one model call. Recorded because a run
 * nobody can reproduce is a run nobody can debug, and the seed and the sampling
 * settings are what a replay has to match. Every field is what the caller asked for,
 * never what the provider did with it. None of them carries prompt or completion text,
 * so a producer running with payload capture off still records all of them.
 */
export interface ModelParameters {
  /**
   * The sampling temperature the caller asked for. Absent when the caller left the
   * provider's default in place, which is not the same as sending zero.
   */
  temperature?: number
  /**
   * Nucleus sampling: the caller asked the sampler to consider the smallest set of
   * tokens whose probabilities reach this fraction. Absent when the caller set none.
   */
  top_p?: number
  /**
   * The caller asked the sampler to consider only this many of the most likely tokens.
   * Not every provider offers it.
   */
  top_k?: number
  /**
   * The ceiling the caller put on output length. An answer that stops in the middle of a
   * sentence usually stopped here, and this is the first field to read when one does.
   */
  max_tokens?: number
  /**
   * The sampling seed the caller passed. Nothing in mcpd reads it. It is recorded
   * because without it a run cannot be reproduced at all, and a comparison between two
   * runs that differ only in their seed is worth nothing.
   */
  seed?: number
  /**
   * The stop sequences the caller supplied. They are short strings the caller wrote, so
   * they are settings rather than payload, and an answer that ends early often ends on
   * one of them. Do not put prompt text here.
   */
  stop_sequences?: string[]
  /**
   * True when the caller constrained the output to a fixed shape, such as a JSON schema
   * or a grammar. The shape itself is not recorded: it can be large and it is
   * caller-owned content, and what a reader needs is whether the constraint was on.
   */
  has_response_format: boolean
}

/** Spec-declared defaults for `ModelParameters`. Spread these when constructing one. */
export const modelParametersDefaults = {
  has_response_format: false,
} satisfies Partial<ModelParameters>

/**
 * Why a span failed.
 */
export interface SpanError {
  /**
   * A human-readable message. Required: an error with no message is not actionable.
   */
  message: string
  /**
   * The error class or code, such as `RateLimitError` or `ECONNRESET`.
   */
  kind?: string
  /**
   * A stack trace, when the producer captured one.
   */
  stack?: string
  /**
   * Which subsystem raised it.
   */
  domain?: string
  /**
   * Whether the producer considered the failure worth retrying.
   */
  retryable?: boolean
  /**
   * Structured detail. Passes through redaction like any other payload.
   */
  details?: unknown
}

/**
 * The descriptive context a signal belongs to. Every field here is queryable and most
 * are safe as metric labels; the high-cardinality ones (`user_id`, `session_id`,
 * `request_id`) are queryable but blocked from metric labels by the cardinality
 * filter.
 */
export interface Correlation {
  /**
   * Which project this belongs to. The primary tenancy key.
   */
  project: string
  /**
   * Deployment environment, such as `production` or `staging`.
   */
  environment?: string
  /**
   * The emitting service.
   */
  service_name?: string
  /**
   * The emitting service's version, so a regression can be pinned to a release.
   */
  service_version?: string
  /**
   * A user-facing session. Groups many traces into one conversation.
   */
  session_id?: string
  /**
   * A conversation thread within a session.
   */
  thread_id?: string
  /**
   * The end user. Hashed with a keyed digest at ingest, so runs can still be grouped by
   * customer without storing who the customer is. Filtering on it hashes your query
   * value the same way, so lookup keeps working. Rotating the key deliberately breaks
   * the link.
   */
  user_id?: string
  /**
   * A tenant or workspace inside the calling application.
   */
  resource_id?: string
  /**
   * The producer's own identifier for this run.
   */
  run_id?: string
  /**
   * The inbound request that caused this work.
   */
  request_id?: string
  /**
   * Set when the run was produced by an experiment.
   */
  experiment_id?: string
  /**
   * A release or git sha, for regression attribution.
   */
  release?: string
}

/**
 * A reference from this span to another, in this trace or a different one. Links
 * express causality that parentage cannot: a batch job caused by twenty requests has
 * twenty links and one parent.
 */
export interface SpanLink {
  /**
   * The linked trace.
   */
  trace_id: string
  /**
   * The linked span, when the link points at one rather than a whole trace.
   */
  span_id?: string
  /**
   * Why the link exists, such as `follows_from`, `retry_of` or `replay_of`.
   */
  kind?: string
  /**
   * Anything else about the relationship.
   */
  attributes?: unknown
}

/**
 * Cursor pagination. Cursors are opaque and stable; never construct one by hand.
 */
export interface Page {
  /**
   * Pass as `cursor` to fetch the next page. Absent on the last page.
   */
  next_cursor?: string
  /**
   * Whether another page exists.
   */
  has_more: boolean
  /**
   * Total matching rows, when the backend could count them cheaply.
   */
  total?: number
}

/**
 * One batch of signals sent to the native ingest endpoint. Any combination of the six
 * signals may appear in a single batch; mcpd sorts them out. Supplying `ingest_key`
 * makes the whole batch idempotent, so a client that retries after a timeout never
 * double-counts.
 */
export interface IngestBatch {
  /**
   * Spans to record.
   */
  spans?: Span[]
  /**
   * Log records to record.
   */
  logs?: LogRecord[]
  /**
   * Metric observations to record.
   */
  metrics?: MetricPoint[]
  /**
   * Machine evaluations to attach.
   */
  scores?: Score[]
  /**
   * Human evaluations to attach.
   */
  feedback?: Feedback[]
  /**
   * Aspect notes to record. A note is a sentence about a trace, so it arrives with that
   * trace's other signals, under the same idempotency key and through the same redactor.
   */
  aspect_notes?: AspectNote[]
  /**
   * Idempotency key for this batch. Replaying a batch with a key mcpd has already
   * accepted is a no-op that still returns 200.
   */
  ingest_key?: string
}

/**
 * Why one signal in a batch was refused.
 */
export interface IngestError {
  /**
   * Which signal kind the failure was in.
   */
  signal: SignalType
  /**
   * Position within that signal's array, so a client can find the offending record.
   */
  index: number
  /**
   * What was wrong, in terms the sender can act on.
   */
  message: string
}

/**
 * What mcpd did with a batch. Counts are per signal, and `rejected` is never silently
 * zero: a signal mcpd could not store is always reported with a reason.
 */
export interface IngestResult {
  /**
   * How many signals were stored.
   */
  accepted: number
  /**
   * How many were refused.
   */
  rejected: number
  /**
   * How many were recognised as a replay of an already-accepted batch and skipped.
   */
  duplicate: number
  /**
   * One entry per rejected signal, naming what was wrong and where.
   */
  errors?: IngestError[]
}

/**
 * A page of traces.
 */
export interface TraceListResponse {
  /**
   * The matching traces, in the requested order.
   */
  traces: Trace[]
  /**
   * Cursor and totals.
   */
  page: Page
}

/**
 * One trace and everything attached to it.
 */
export interface TraceDetail {
  /**
   * The trace projection.
   */
  trace: Trace
  /**
   * Every span, ordered by start time then by depth, so a waterfall renders without
   * sorting.
   */
  spans: Span[]
  /**
   * Logs, when `include` asked for them.
   */
  logs?: LogRecord[]
  /**
   * Scores, when `include` asked for them.
   */
  scores?: Score[]
  /**
   * Feedback, when `include` asked for them.
   */
  feedback?: Feedback[]
}

/**
 * A page of spans.
 */
export interface SpanListResponse {
  /**
   * The matching spans.
   */
  spans: Span[]
  /**
   * Cursor and totals.
   */
  page: Page
}

/**
 * A page of log records.
 */
export interface LogListResponse {
  /**
   * The matching records.
   */
  logs: LogRecord[]
  /**
   * Cursor and totals.
   */
  page: Page
}

/**
 * One bucket of an aggregated series.
 */
export interface MetricSeriesPoint {
  /**
   * Start of the bucket.
   */
  timestamp: string
  /**
   * The aggregated value.
   */
  value: number
  /**
   * How many raw observations fell in this bucket. Distinguishes a genuine zero from no
   * data.
   */
  count?: number
  /**
   * A trace that contributed to this bucket, so a spike on the chart opens the run that
   * caused it.
   */
  exemplar_trace_id?: string
}

/**
 * One line on a chart: a single label combination over time.
 */
export interface MetricSeries {
  /**
   * The label values that identify this series.
   */
  labels?: unknown
  /**
   * Bucketed values, oldest first.
   */
  points: MetricSeriesPoint[]
}

/**
 * An aggregated metric query result.
 */
export interface MetricSeriesResponse {
  /**
   * The metric queried.
   */
  name: string
  /**
   * UCUM unit, for axis labelling.
   */
  unit?: string
  /**
   * The bucket width actually used, which may differ from the request when mcpd chose
   * it.
   */
  bucket: string
  /**
   * One entry per label combination.
   */
  series: MetricSeries[]
}

/**
 * A page of scores.
 */
export interface ScoreListResponse {
  /**
   * The matching scores.
   */
  scores: Score[]
  /**
   * Cursor and totals.
   */
  page: Page
}

/**
 * A page of feedback.
 */
export interface FeedbackListResponse {
  /**
   * The matching feedback.
   */
  feedback: Feedback[]
  /**
   * Cursor and totals.
   */
  page: Page
}

/**
 * Latency distribution over a window, in milliseconds.
 */
export interface LatencyStats {
  /**
   * Median.
   */
  p50?: number
  /**
   * Ninetieth percentile.
   */
  p90?: number
  /**
   * Ninety-fifth percentile.
   */
  p95?: number
  /**
   * Ninety-ninth percentile.
   */
  p99?: number
  /**
   * Arithmetic mean. Reported alongside quantiles, never instead of them.
   */
  mean?: number
  /**
   * Slowest observation in the window.
   */
  max?: number
}

/**
 * One row of a grouped breakdown.
 */
export interface GroupedStat {
  /**
   * The group's value, such as a model id.
   */
  key: string
  /**
   * How many runs fell in this group.
   */
  count: number
  /**
   * How many of them failed.
   */
  error_count: number
  /**
   * Spend attributable to this group.
   */
  total_cost?: number
  /**
   * Input tokens for this group.
   */
  input_tokens?: number
  /**
   * Output tokens for this group.
   */
  output_tokens?: number
  /**
   * Latency distribution within this group.
   */
  latency?: LatencyStats
}

/**
 * Headline numbers for a window. One request backs the overview screen.
 */
export interface AnalyticsSummary {
  /**
   * Start of the window.
   */
  from: string
  /**
   * End of the window.
   */
  to: string
  /**
   * Runs started in the window.
   */
  trace_count: number
  /**
   * Steps recorded in the window.
   */
  span_count: number
  /**
   * Runs that failed.
   */
  error_count: number
  /**
   * Failures as a fraction of runs, between 0 and 1.
   */
  error_rate: number
  /**
   * Run duration distribution.
   */
  latency: LatencyStats
  /**
   * Input tokens consumed.
   */
  input_tokens: number
  /**
   * Output tokens produced.
   */
  output_tokens: number
  /**
   * Total spend in the window.
   */
  total_cost: number
  /**
   * Currency of `total_cost`.
   */
  currency: string
  /**
   * Per-group rows, when `group_by` was supplied.
   */
  groups?: GroupedStat[]
}

/** Spec-declared defaults for `AnalyticsSummary`. Spread these when constructing one. */
export const analyticsSummaryDefaults = {
  currency: "USD",
} satisfies Partial<AnalyticsSummary>

/**
 * Spend in one time bucket for one group.
 */
export interface CostBucket {
  /**
   * Start of the bucket.
   */
  timestamp: string
  /**
   * The group this row belongs to.
   */
  key: string
  /**
   * Spend in this bucket.
   */
  total_cost: number
  /**
   * Input tokens in this bucket.
   */
  input_tokens?: number
  /**
   * Output tokens in this bucket.
   */
  output_tokens?: number
}

/**
 * Where the money went.
 */
export interface CostBreakdown {
  /**
   * Start of the window.
   */
  from: string
  /**
   * End of the window.
   */
  to: string
  /**
   * Currency of every figure in this response.
   */
  currency: string
  /**
   * Total spend across the window.
   */
  total_cost: number
  /**
   * Totals per group, largest first.
   */
  groups: GroupedStat[]
  /**
   * A time series, when `bucket` was supplied.
   */
  series?: CostBucket[]
}

/** Spec-declared defaults for `CostBreakdown`. Spread these when constructing one. */
export const costBreakdownDefaults = {
  currency: "USD",
} satisfies Partial<CostBreakdown>

/**
 * What a deletion removed.
 */
export interface DeleteResult {
  /**
   * How many records were removed, across every signal.
   */
  deleted: number
}

/**
 * Whether the server is alive and able to serve.
 */
export interface HealthStatus {
  /**
   * `ok` or `degraded`.
   */
  status: string
  /**
   * The mcpd version serving this request.
   */
  version: string
  /**
   * The data-model spec version this build implements. A client can refuse to talk to an
   * incompatible server.
   */
  spec_version: string
  /**
   * How long this process has been running.
   */
  uptime_seconds: number
  /**
   * Which storage backend is configured.
   */
  storage?: string
  /**
   * What is degraded, when something is.
   */
  detail?: string
  /**
   * True when this daemon offers open sign-up: anyone may create an account at `POST
   * /v1/api/account/signup` and log in at `POST /v1/api/account/login`. Absent when it
   * does not, which is the default. The interface reads this to decide whether to show
   * its sign-up and log-in screens.
   */
  signup?: boolean
  /**
   * True when this daemon has accounts, with or without open sign-up. Absent when it
   * does not. The interface reads this to show its log-in screen.
   */
  accounts?: boolean
  /**
   * True when at least one organisation on this daemon logs in with single sign-on, so
   * the log-in screen offers it. Absent otherwise.
   */
  sso?: boolean
}

/**
 * One scorer mcpd can run, as described to a caller.
 */
export interface ScorerInfo {
  /**
   * Stable identifier, recorded on every score it produces.
   */
  id: string
  /**
   * What it measures, in one line.
   */
  description: string
  /**
   * Whether it needs settings to mean anything. A scorer that needs an expected answer
   * cannot run without one.
   */
  needs_config: boolean
}

/**
 * Settings shared by the scorers that need them. Every field is optional; a scorer
 * that needs one and does not get it declines instead of guessing.
 */
export interface ScorerConfig {
  /**
   * Text the answer must contain, for `contains`.
   */
  needle?: string
  /**
   * The expected answer, for `exact_match`.
   */
  expected?: string
  /**
   * A budget in milliseconds, for `latency_budget`.
   */
  max_ms?: number
  /**
   * A budget in currency units, for `cost_budget`.
   */
  max_cost?: number
  /**
   * The most tool calls a healthy run should make, for `tool_efficiency`.
   */
  max_tool_calls?: number
  /**
   * The kind of span that must be present in the run, for `span_present`.
   */
  span_kind?: SpanKind
  /**
   * The span name that must be present in the run, for `span_present`. Matched exactly
   * against `Span.name`.
   */
  span_name?: string
  /**
   * The tool that must have run and ended `ok`, for `tool_succeeded`. Matched exactly
   * against the `name` of a span whose kind is `tool_call`, `client_tool_call`,
   * `provider_tool_call` or `mcp_tool_call`.
   */
  tool_name?: string
  /**
   * Case sensitivity for text comparisons.
   */
  case_sensitive: boolean
}

/** Spec-declared defaults for `ScorerConfig`. Spread these when constructing one. */
export const scorerConfigDefaults = {
  case_sensitive: false,
} satisfies Partial<ScorerConfig>

/**
 * A request to run scorers over runs that are already stored. Scoring reads stored
 * runs rather than running inline, so an evaluation can never add latency to the
 * request it is judging, and a scorer that fails can never fail a user's request.
 */
export interface ScoreRunRequest {
  /**
   * Which project's runs to score. Defaults to the server's default project.
   */
  project?: string
  /**
   * Score exactly these runs. Takes precedence over the window.
   */
  trace_ids: string[]
  /**
   * Otherwise, score runs started after this time.
   */
  from?: string
  /**
   * Most runs to score in one request, newest first, when `trace_ids` is empty. Unset,
   * it is 100 for deterministic scorers, and for a pass with a judged scorer it is as
   * many runs as fit in the request deadline. A judged pass that names more runs than
   * fit, through `limit` or `trace_ids`, is refused with the cap.
   */
  limit?: number
  /**
   * Which scorers to run, by id. Empty means every scorer that needs no configuration.
   */
  scorers: string[]
  /**
   * Settings for the scorers that need them.
   */
  config?: ScorerConfig
  /**
   * What to record as the origin of these scores.
   */
  source?: ScoreSource
  /**
   * Report what would happen and store nothing.
   */
  dry_run: boolean
}

/** Spec-declared defaults for `ScoreRunRequest`. Spread these when constructing one. */
export const scoreRunRequestDefaults = {
  dry_run: false,
} satisfies Partial<ScoreRunRequest>

/**
 * A scorer that declined to answer on one run.
 */
export interface SkippedScorer {
  /**
   * Which scorer declined.
   */
  scorer_id: string
  /**
   * Which run it declined on.
   */
  trace_id: string
  /**
   * Why. Never silent: a scorer that cannot answer is more interesting than one that
   * returned a number.
   */
  reason: string
}

/**
 * What one scoring pass did.
 */
export interface ScoreRunResult {
  /**
   * Runs looked at.
   */
  traces_scored: number
  /**
   * Scores produced and stored.
   */
  scores_written: number
  /**
   * Every scorer that declined, with its reason.
   */
  skipped: SkippedScorer[]
  /**
   * The scores themselves, so the caller sees them without a second read.
   */
  scores: Score[]
}

/**
 * How one scorer moved between two experiments.
 */
export interface ScorerDelta {
  /**
   * Which scorer.
   */
  scorer_id: string
  /**
   * Mean on the baseline, when it ran there.
   */
  baseline_mean?: number
  /**
   * Number of baseline observations.
   */
  baseline_n: number
  /**
   * Mean on the candidate.
   */
  candidate_mean?: number
  /**
   * Number of candidate observations.
   */
  candidate_n: number
  /**
   * Candidate minus baseline.
   */
  delta?: number
  /**
   * Whether the sample is large enough for the delta to mean anything. A deliberately
   * blunt rule rather than a significance test: below twenty observations on either
   * side, mcpd says the comparison is not decidable rather than dressing noise up as a
   * finding.
   */
  decidable: boolean
}

/**
 * Two experiments, compared. The point of an experiment is the comparison, so this
 * reports the delta per scorer beside the sample sizes. A mean that moved on four runs
 * is noise, and printing the counts next to it is what stops it being read as a
 * result.
 */
export interface ExperimentComparison {
  /**
   * The baseline's identifier.
   */
  baseline: string
  /**
   * The candidate's identifier.
   */
  candidate: string
  /**
   * One row per scorer present on either side.
   */
  scorers: ScorerDelta[]
  /**
   * Whether any decidable scorer moved down.
   */
  regressed: boolean
}

/**
 * A request to turn stored runs into dataset cases.
 */
export interface CaptureRequest {
  /**
   * The runs to capture. Each becomes one item, keeping its trace id so anyone can go
   * back and read what actually happened.
   */
  trace_ids: string[]
  /**
   * Record each run's own output as the expected answer. Leave this off for a run that
   * went wrong: capturing a bad answer as the reference would enshrine the bug.
   */
  use_output_as_expected: boolean
}

/** Spec-declared defaults for `CaptureRequest`. Spread these when constructing one. */
export const captureRequestDefaults = {
  use_output_as_expected: false,
} satisfies Partial<CaptureRequest>

/**
 * What a capture added to a dataset.
 */
export interface CaptureResult {
  /**
   * Items added.
   */
  captured: number
  /**
   * Runs asked for. A gap means a run was not found.
   */
  requested: number
}

/**
 * How many cases a dataset gained.
 */
export interface ItemsAdded {
  /**
   * Items added.
   */
  added: number
}

/**
 * What an erasure request removed, counted per signal. The subject is not echoed back:
 * it has been erased, and repeating it in a response that may be logged on the way out
 * would write it down again.
 */
export interface ErasureResult {
  /**
   * The project the erasure ran in. No other project was read or touched.
   */
  project: string
  /**
   * How many runs were removed.
   */
  traces: number
  /**
   * How many spans were removed, with the prompts and completions they carried.
   */
  spans: number
  /**
   * How many log lines were removed.
   */
  logs: number
  /**
   * How many metric points were removed.
   */
  metrics: number
  /**
   * How many scores were removed.
   */
  scores: number
  /**
   * How many pieces of feedback were removed.
   */
  feedback: number
  /**
   * The total across every signal, so a caller that wants one number does not add them
   * up.
   */
  deleted: number
  /**
   * How many durable runs were removed from the run store, each with its step attempts,
   * event log, deadlines, waits, the events it consumed and any span still queued for
   * the trace store. A run is matched on the `user_id` in its correlation, as it was
   * started with it, and removed with the whole tree of subworkflow runs it belongs to.
   * Not part of `deleted`, which counts the trace store. Absent when this daemon does
   * not serve runs.
   */
  runs_erased?: number
  /**
   * How many runs attributed to this person were left untouched because a run in their
   * tree has not ended. Removing a live run would corrupt the workflow it is part of, so
   * these are counted rather than skipped in silence, and `note` names the root runs.
   * Cancel them, or let them end, then send the erasure again. Absent when this daemon
   * does not serve runs.
   */
  runs_in_flight?: number
  /**
   * How many aspect notes were removed, matched by the run they describe and by the
   * person stamped on them. Not part of `deleted`. Absent only from a store that holds
   * no notes.
   */
  aspect_notes_erased?: number
  /**
   * How many dataset cases captured from an erased run were emptied in place. They are
   * not deleted, because a case was kept on purpose, so they are not part of `deleted`.
   */
  dataset_items_erased?: number
  /**
   * The datasets those emptied cases sit in, so the reply can say where they are.
   */
  datasets: string[]
  /**
   * True when the storage backend applies the deletes after this reply is sent, as
   * ClickHouse does, so the counts say what matched rather than what has already left
   * the disk.
   */
  deletes_pending?: boolean
  /**
   * How many stored identifiers this request could not have matched, because they were
   * written in the other form, hashed or plain, from the one given. A number above zero
   * means the counts may not be the whole answer.
   */
  unmatchable_identifiers?: number
  /**
   * Why the numbers above may not be the whole answer, in sentences an operator can
   * repeat. Absent when they are the whole answer.
   */
  note?: string
}

/**
 * A definition as you post it: the graph, and the id and version to store it under.
 * The body is validated at write time against `spec/definition.schema.json`; a body
 * the walker cannot run is refused with a 400 naming the field, and unknown fields are
 * refused, so a guard that is spelled wrong is an error and not a run that skips its
 * guard.
 */
export interface DefinitionDocument {
  /**
   * The definition's identifier within its project. Matches `^[a-z][a-z0-9_]*$`.
   */
  id: string
  /**
   * What kind of definition. Absent means `workflow`, the only kind version 1 serves.
   */
  kind?: DefinitionKind
  /**
   * The version to store the body under. Absent asks the daemon to choose: the version
   * whose stored body is identical, or one past the highest stored. A named version that
   * is free is taken; one held by an identical body is confirmed; one held by a
   * different body is a 409, because a stored version never changes.
   */
  version?: number
  /**
   * The document itself: `input_schema`, `limits`, `steps` and `output`. The definition
   * schema names every field.
   */
  body: unknown
}

/**
 * Where a posted definition landed.
 */
export interface DefinitionVersion {
  /**
   * The definition's identifier.
   */
  id: string
  /**
   * The version the body is stored under.
   */
  version: number
  /**
   * True when the daemon chose the number because the body was new. False when the body
   * already existed at this version, or you named the version yourself.
   */
  assigned: boolean
}

/**
 * The frontier of a run: which position in the definition's `steps` it has reached,
 * and what sits there.
 */
export interface RunCursor {
  /**
   * The index into `steps`, or their count at the end of the graph.
   */
  index: number
  /**
   * What is at that position.
   */
  construct: ConstructKind
  /**
   * The step this position belongs to, when the construct has one. A fan-out names its
   * parent step; the branch and item keys are on the attempts.
   */
  step_key?: string
}

/**
 * A deadline the daemon holds for a run.
 */
export interface RunTimer {
  /**
   * Which run.
   */
  run_id: string
  /**
   * The step the deadline belongs to. Absent for a run-level deadline.
   */
  step_key?: string
  /**
   * Which deadline.
   */
  kind: TimerKind
  /**
   * When it fires.
   */
  due_at: string
}

/**
 * A suspension waiting for an event.
 */
export interface RunWait {
  /**
   * Which run.
   */
  run_id: string
  /**
   * The `wait_event` step that is waiting.
   */
  step_key: string
  /**
   * The event name an inbound event must match.
   */
  event: string
  /**
   * The rendered key that picks this run out of every run waiting on that event.
   */
  key: string
  /**
   * When waiting stops and the node's `on_timeout` applies.
   */
  expires_at: string
}

/**
 * One run and everything the daemon holds for it.
 */
export interface RunDetail {
  /**
   * The run.
   */
  run: Run
  /**
   * Every attempt, in the order they were written.
   */
  attempts: StepAttempt[]
  /**
   * The deadlines currently held. Empty once the run has ended.
   */
  timers: RunTimer[]
  /**
   * The suspensions currently open. Empty once the run has ended.
   */
  waits: RunWait[]
}

/**
 * A page of runs.
 */
export interface RunListResponse {
  /**
   * The matching runs, newest first.
   */
  runs: Run[]
  /**
   * Cursor and totals.
   */
  page: Page
}

/**
 * A page of a run's event log.
 */
export interface RunEventPage {
  /**
   * The rows after the requested sequence number, in order.
   */
  events: RunEvent[]
  /**
   * Whether rows beyond the last one here existed when the page was read. A run that is
   * still moving will write more.
   */
  has_more: boolean
}

/**
 * One edge of a cause chain: an ancestor step, and the template path through which the
 * step below it read that ancestor's output. An ancestor read through two paths
 * appears twice, once per path.
 */
export interface CauseLink {
  /**
   * The ancestor step, keyed the way the journal keys it: `refund/issue`, `classify[3]`,
   * `poll[7]/classify`.
   */
  step_key: string
  /**
   * Where the ancestor's latest attempt ended.
   */
  status: AttemptStatus
  /**
   * How many attempts were made at the ancestor. More than one on a step that ended
   * `succeeded` is worth reading: it got there, but not the first time.
   */
  attempts: number
  /**
   * The path as the definition writes it inside `${...}`, such as
   * `steps.classify.output.kind`. The definition declares this edge, so the chain is
   * read rather than inferred from timing.
   */
  template_path: string
  /**
   * The step key that read this ancestor through `template_path`. On a direct edge it is
   * the step the answer is about; deeper in the chain it is another ancestor.
   */
  consumed_by: string
  /**
   * How far this ancestor sits from the step the answer is about. `1` is read directly,
   * `2` is read by something the step read directly, and so on.
   */
  depth: number
  /**
   * True when this ancestor ended `succeeded` and every step between it and the failed
   * step ended `succeeded` too. Nothing on that path is marked failed, so no status code
   * points at it and the value it produced is the suspect. It is not the same as
   * `status` being `succeeded`: an ancestor sitting behind a step that did fail is not a
   * silent producer, because that failure already explains the run.
   */
  silent_producer: boolean
  /**
   * The JSON type `template_path` resolved to on the ancestor's stored output. The value
   * itself is never returned, only its shape. Absent when there is no output to read,
   * which is the case for a step that never ran and for one that failed.
   */
  value_kind?: ValueKind
  /**
   * True when that value was null, an empty string, an empty array or an empty object. A
   * step that ended `succeeded` and handed the step below it nothing is the failure a
   * status code cannot catch, and this is the field that shows it. Absent for the same
   * reason `value_kind` is.
   */
  value_empty?: boolean
}

/**
 * Why one step of a run failed, answered by walking backwards along the data
 * dependencies its definition declares. The daemon reads the run journal and the
 * pinned definition version, both of which it has to hold to render `${...}` templates
 * at all, so this answer is available with span payload capture off and with no model,
 * prompt or completion involved.
 */
export interface StepCause {
  /**
   * The run this answer is about.
   */
  run_id: string
  /**
   * The step this answer is about, as it was asked for.
   */
  step_key: string
  /**
   * Where that step's latest attempt ended.
   */
  status: AttemptStatus
  /**
   * How many attempts were made at it.
   */
  attempts: number
  /**
   * Why the latest attempt failed, when it did. Absent on a step that did not fail,
   * because the endpoint answers for any step, not only a failed one.
   */
  error?: StepError
  /**
   * The ancestors whose output this step consumed, directly or transitively. Ordered by
   * `depth` ascending, then by the order the paths appear in the definition, so two
   * calls against one run return the same list in the same order. Empty when the step
   * reads nothing but the run input.
   */
  chain: CauseLink[]
  /**
   * True when the walk stopped at `max_depth` with ancestors left unvisited, so `chain`
   * is a prefix of the answer rather than the whole of it.
   */
  truncated: boolean
}

/**
 * A request to start a run.
 */
export interface StartRunRequest {
  /**
   * Which project. A selector: refused with 403 on a mismatch with the key's grant,
   * filled in for a single-project key, and a 400 when a multi-project key names none.
   */
  project?: string
  /**
   * Which definition to run.
   */
  definition_id: string
  /**
   * Which version. Absent pins the highest stored version at this moment, and the run
   * records it.
   */
  version?: number
  /**
   * The run's input. Validated against the definition's `input_schema` when it declares
   * one; a failure is a 400.
   */
  input?: unknown
  /**
   * Context to store on the run and copy onto every span the daemon writes for it.
   * `project` and `run_id` inside it are ignored: the run's own values win.
   */
  correlation?: Correlation
  /**
   * Makes the start idempotent per project for 24 hours: a repost returns the same run
   * and starts nothing, and two simultaneous posts of one key produce one run.
   */
  idempotency_key?: string
}

/**
 * A worker asking for work.
 */
export interface ClaimRequest {
  /**
   * Which project. A selector: refused with 403 on a mismatch with the key's grant,
   * filled in for a single-project key, and a 400 when a multi-project key names none.
   */
  project?: string
  /**
   * Who is asking. Recorded on the attempt as `lease_owner` and in the run's event log.
   * Nothing else about the worker is stored.
   */
  worker_id: string
  /**
   * The most leases to hand back, 1 to 100.
   */
  max: number
  /**
   * How long to hold the request open waiting for work, 0 to 60000 milliseconds. The
   * daemon holds a claim for at most four fifths of its request timeout, 24000 at the
   * default 30 seconds, so a longer wait is answered early rather than cut off with a
   * 503. An empty answer means nothing became runnable in time: reissue the claim.
   */
  wait_ms: number
  /**
   * Restrict the claim to one run, which is how a script drives its own run inline.
   */
  run_id?: string
}

/**
 * A step handed to a worker. Every later call about the attempt is addressed by
 * `lease_id`.
 */
export interface StepLease {
  /**
   * The address of every later call about this attempt.
   */
  lease_id: string
  /**
   * Which run.
   */
  run_id: string
  /**
   * Which project the run belongs to.
   */
  project: string
  /**
   * Which definition the run is executing.
   */
  definition_id: string
  /**
   * Which version. Fail the lease with class `unservable` if you do not serve it.
   */
  definition_version: number
  /**
   * The step to run. A worker that does not recognise it fails the lease with class
   * `unservable` rather than guessing.
   */
  step_key: string
  /**
   * Whether this is a plain task or an agent turn.
   */
  kind: StepKind
  /**
   * Which attempt this is. The fence: a call about this lease is refused once a later
   * attempt exists.
   */
  attempt: number
  /**
   * Under `at_most_once`, post `started` and wait for its 200 before running the body.
   */
  effect: Effect
  /**
   * When the lease lapses. Heartbeat every `heartbeat_ms` to keep it.
   */
  expires_at: string
  /**
   * The step's own timeout, when the node declares one. It bounds the attempt whatever
   * the heartbeats say.
   */
  deadline_ms?: number
  /**
   * A key stable across every attempt of this step: the SHA-256 of `run_id:step_key`.
   * Pass it to any downstream call that can deduplicate.
   */
  idempotency_key: string
  /**
   * The node's `input` template rendered for this attempt, or the run's input when the
   * node declares none.
   */
  input?: unknown
  /**
   * The ended attempts of earlier steps, keyed by step id: `{ status, output?, error? }`
   * for the latest attempt of each, or `{ status: skipped }`. Only ended attempts
   * appear.
   */
  prior: unknown
  /**
   * The run's state object as it stands.
   */
  state?: unknown
  /**
   * The run's correlation, with `run_id` set. Put it on every span you record inside
   * this step.
   */
  correlation: Correlation
  /**
   * The run's trace. Set it on every span you record inside this step.
   */
  trace_id: string
  /**
   * The span the daemon has already opened for this attempt. Set it as `parent_span_id`
   * on your own spans, so a model call or a tool call nests under the step.
   */
  span_id: string
  /**
   * The run's root span, which this attempt's span hangs off.
   */
  parent_span_id: string
}

/**
 * What a claim answered.
 */
export interface ClaimResponse {
  /**
   * Zero to `max` leases. Empty when nothing became runnable within `wait_ms`.
   */
  leases: StepLease[]
}

/**
 * What `started` and `heartbeat` answer.
 */
export interface LeaseState {
  /**
   * The lease's new expiry.
   */
  expires_at: string
  /**
   * True when this call found the attempt `orphaned` and restored it to the state it
   * came from.
   */
  reclaimed: boolean
}

/**
 * Token usage a worker reports on `complete`. The daemon prices it from its price book
 * and charges the attempt, the run and the root of the run tree.
 */
export interface StepUsage {
  /**
   * Total input tokens.
   */
  input_tokens?: number
  /**
   * Total output tokens.
   */
  output_tokens?: number
  /**
   * Input plus output, when the provider reports a total that differs from the sum.
   */
  total_tokens?: number
  /**
   * How the input tokens were billed.
   */
  input_details?: InputTokenDetails
  /**
   * How the output tokens were billed.
   */
  output_details?: OutputTokenDetails
  /**
   * The model that produced the usage, which the price book needs to price it.
   */
  model?: string
  /**
   * The provider that served the model.
   */
  provider?: string
}

/**
 * What a worker posts to complete a step.
 */
export interface StepResult {
  /**
   * The step's output. Later steps read it as `steps.<id>.output`.
   */
  output?: unknown
  /**
   * An object to merge shallowly into the run's state.
   */
  state?: unknown
  /**
   * Token usage to price and charge.
   */
  usage?: StepUsage
}

/**
 * Why a step failed, as the worker reports it.
 */
export interface StepError {
  /**
   * What kind of failure. Decides whether the daemon retries.
   */
  class: StepErrorClass
  /**
   * What went wrong, for the person reading the run.
   */
  message: string
  /**
   * Structured detail. Stored on the attempt.
   */
  details?: unknown
  /**
   * The least the daemon should wait before the next attempt, when the worker knows: a
   * rate limit's reset, say. Honoured when it is larger than the backoff. At most
   * 315360000000 (3650 days), the bound on every duration in a definition; a larger
   * value is a 400 naming this field.
   */
  retry_after_ms?: number
}

/**
 * What a worker posts to fail a step.
 */
export interface StepFailure {
  /**
   * Why it failed.
   */
  error: StepError
}

/**
 * What `complete` and `fail` answer.
 */
export interface StepAck {
  /**
   * Where the run is after this call.
   */
  run_status: RunStatus
  /**
   * `claim` when more work is runnable now, `idle` otherwise.
   */
  next: string
  /**
   * After a `fail`: how many attempts the node's retry budget still allows.
   */
  attempts_left?: number
}

/**
 * The body of a 409 from a worker call. `reason` says what is true and what the worker
 * should do next.
 */
export interface StepConflict {
  /**
   * Always `conflict`.
   */
  error: string
  /**
   * Which conflict.
   */
  reason: ConflictReason
  /**
   * The reason, in words.
   */
  message: string
  /**
   * The attempt the lease addressed.
   */
  attempt: number
  /**
   * Where that attempt is, when it still exists. Under `already_applied` this says how
   * the step ended.
   */
  attempt_status?: AttemptStatus
  /**
   * Where the run is.
   */
  run_status: RunStatus
}

/**
 * An event for whichever run is waiting for it.
 */
export interface PublishEventRequest {
  /**
   * Which project. A selector: refused with 403 on a mismatch with the key's grant,
   * filled in for a single-project key, and a 400 when a multi-project key names none.
   */
  project?: string
  /**
   * The event name, matched against open waits.
   */
  event: string
  /**
   * Matched against the rendered `key` of each waiting run. Delivered to the matching
   * run whose wait expires soonest.
   */
  key: string
  /**
   * Becomes the waiting step's output.
   */
  payload?: unknown
  /**
   * Your own id for this event. A supplied id seen before answers `duplicate` and writes
   * nothing, whether or not a run is waiting. Absent, the daemon derives one from
   * `project`, `event`, `key` and `payload`, and a derived id counts as a duplicate only
   * when no run is waiting, because two identical approvals for two runs on one key are
   * two events. Send your own id from any handler that can be retried.
   */
  event_id?: string
}

/**
 * What publishing an event did.
 */
export interface EventReceipt {
  /**
   * Delivered, parked or duplicate.
   */
  outcome: EventOutcome
  /**
   * The id used, supplied or derived.
   */
  event_id: string
}

/**
 * An event for one run, addressed by run id rather than by key. The daemon reads the
 * run's open wait on `event`, takes its key, and routes the event as a published one.
 */
export interface SignalRequest {
  /**
   * The event name. The run must hold an open wait on it.
   */
  event: string
  /**
   * Becomes the waiting step's output.
   */
  payload?: unknown
  /**
   * Your own id for this event, deduplicated as on a published event.
   */
  event_id?: string
}

/**
 * A worker announcing the definitions it serves.
 */
export interface WorkerRegistration {
  /**
   * Which project. A selector: refused with 403 on a mismatch with the key's grant,
   * filled in for a single-project key, and a 400 when a multi-project key names none.
   */
  project?: string
  /**
   * Who is registering. Not stored: dispatch reads the runnable set, never a worker
   * table.
   */
  worker_id: string
  /**
   * Each is stored under the versioning rule, exactly as if posted to the definitions
   * endpoint.
   */
  definitions: DefinitionDocument[]
}

/**
 * What registering answered.
 */
export interface WorkerRegistered {
  /**
   * The id you registered with.
   */
  worker_id: string
  /**
   * Where each definition landed, in the order posted.
   */
  registered: DefinitionVersion[]
  /**
   * How long a lease lasts without a heartbeat.
   */
  lease_ttl_ms: number
  /**
   * How often to heartbeat a held lease: a third of `lease_ttl_ms`.
   */
  heartbeat_ms: number
}

/**
 * A saved condition the daemon evaluates on a schedule. Monitors are configured in the
 * daemon's configuration file, not through the API.
 */
export interface Monitor {
  /**
   * Stable identifier.
   */
  id: string
  /**
   * What a person should read when it trips.
   */
  name: string
  /**
   * Which project it watches.
   */
  project: string
  /**
   * What it measures.
   */
  watch: MonitorWatch
  /**
   * Which scorer, when `watch` is `score_mean`.
   */
  scorer_id?: string
  /**
   * Which direction trips it.
   */
  compare: MonitorCompare
  /**
   * The threshold.
   */
  threshold: number
  /**
   * How far back each evaluation looks.
   */
  window_seconds: number
  /**
   * The fewest runs the window must contain before the monitor will trip. Without it a
   * single failed run in a quiet hour is a 100% error rate, and the first thing anyone
   * does with an alert that cries wolf is mute it.
   */
  min_samples: number
  /**
   * Where to POST when it trips or clears. Set as a full URL in the config file; the API
   * returns only its host, because a webhook URL is often its own credential.
   */
  webhook?: string
  /**
   * Whether it is evaluated at all.
   */
  enabled: boolean
}

/** Spec-declared defaults for `Monitor`. Spread these when constructing one. */
export const monitorDefaults = {
  window_seconds: 300,
  min_samples: 20,
  enabled: true,
} satisfies Partial<Monitor>

/**
 * One evaluation of a monitor.
 */
export interface MonitorReading {
  /**
   * Which monitor.
   */
  monitor_id: string
  /**
   * When it was evaluated.
   */
  at: string
  /**
   * The measured value, when there was enough data to measure one.
   */
  value?: number
  /**
   * How many runs the window held.
   */
  samples: number
  /**
   * The resulting condition.
   */
  condition: MonitorCondition
  /**
   * What a person should read.
   */
  detail: string
}

/**
 * A monitor and the condition it is latched in.
 */
export interface MonitorState {
  /**
   * The monitor.
   */
  monitor: Monitor
  /**
   * Its current condition.
   */
  condition: MonitorCondition
  /**
   * The most recent reading. Absent before the first evaluation.
   */
  last?: MonitorReading
  /**
   * When it first tripped, while it stays tripped.
   */
  tripped_at?: string
}

/**
 * A service level objective: the fraction of runs that must succeed over a window. A
 * monitor asks whether a number is over a line right now. An objective asks whether
 * the promise is going to be missed and how fast the error budget is burning, over two
 * windows at once, so a two minute blip and a slow leak get the different answers they
 * need. Objectives are configured in the daemon's configuration file, not through the
 * API.
 */
export interface Slo {
  /**
   * Stable identifier.
   */
  id: string
  /**
   * What a person should read.
   */
  name: string
  /**
   * Which project it covers.
   */
  project: string
  /**
   * The fraction of runs that must succeed, between 0 and 1. `0.995` is ninety-nine and
   * a half percent of runs.
   */
  objective: number
  /**
   * How long the promise is measured over.
   */
  window_days: number
  /**
   * The fewest runs a burn window must hold before it may trip.
   */
  min_samples: number
  /**
   * Where to POST on a transition. Set as a full URL in the config file; the API returns
   * only its host, because a webhook URL is often its own credential.
   */
  webhook?: string
  /**
   * Whether it is evaluated at all.
   */
  enabled: boolean
}

/** Spec-declared defaults for `Slo`. Spread these when constructing one. */
export const sloDefaults = {
  window_days: 30,
  min_samples: 20,
  enabled: true,
} satisfies Partial<Slo>

/**
 * One evaluation of an objective.
 */
export interface SloReading {
  /**
   * Which objective.
   */
  slo_id: string
  /**
   * When it was evaluated.
   */
  at: string
  /**
   * The observed success rate over the whole window.
   */
  success_rate?: number
  /**
   * The fraction of the error budget still unspent. Negative once it is overspent, which
   * says by how much the promise was missed.
   */
  budget_remaining?: number
  /**
   * The burn rate over the one hour window. A rate of 1 spends the whole budget exactly
   * over the whole window.
   */
  burn_fast?: number
  /**
   * The burn rate over the six hour window. Both windows must exceed their trip rate
   * together before the objective trips, which is what stops a brief spike paging
   * anyone.
   */
  burn_slow?: number
  /**
   * Runs in the whole window.
   */
  samples: number
  /**
   * The resulting condition.
   */
  condition: MonitorCondition
  /**
   * What a person should read.
   */
  detail: string
}

/**
 * An objective and the condition it is latched in.
 */
export interface SloState {
  /**
   * The objective.
   */
  slo: Slo
  /**
   * Its current condition.
   */
  condition: MonitorCondition
  /**
   * The most recent reading. Absent before the first evaluation.
   */
  last?: SloReading
  /**
   * When it first tripped, while it stays tripped.
   */
  tripped_at?: string
}

/**
 * What the storage backend behind this daemon can and cannot do. Three backends
 * implement one storage trait and they are not equal: a reader on ClickHouse who
 * searches for `CAFE` with an acute and finds nothing has no way to learn that this
 * backend folds ASCII case and no more. Read this once at connect time and offer only
 * what the deployment answers. Every field is named after the capability an endpoint
 * names when it refuses with 501, so the two can be compared without translation.
 */
export interface StoreCapabilities {
  /**
   * Which backend this describes: `sqlite`, `postgres` or `clickhouse`. The same string
   * `/healthz` reports as `storage`.
   */
  backend: string
  /**
   * What the `q` parameter matches.
   */
  text_search: TextSearch
  /**
   * Whether `q` folds case for letters outside ASCII. False means a search for `CAFE`
   * with an acute does not find the lowercase form, which a reader otherwise meets as an
   * empty result they cannot explain.
   */
  unicode_case_folding: boolean
  /**
   * Whether a delete is on disk when the call returns. False means the backend accepted
   * the delete and applies it afterwards, so a count says what matched rather than what
   * has already gone.
   */
  immediate_deletes: boolean
  /**
   * Whether `GET /v1/api/discovery` answers. False means a filter box has to be filled
   * from what is already on screen.
   */
  discovery: boolean
  /**
   * Whether the `metric_label_value` facet is answerable, not only the keys. Separate
   * because the values are the expensive half: a key set is small and bounded, and a
   * value set is not.
   */
  discovery_label_values: boolean
  /**
   * Whether `GET /v1/api/deltas` answers, so a list refreshes from a cursor rather than
   * by running the whole query again.
   */
  delta_polling: boolean
  /**
   * Whether retention can carry a different window per signal. False means one window
   * covers all five.
   */
  per_signal_retention: boolean
  /**
   * Whether the window can be handed to the database engine, so nothing has to run a
   * delete on a timer.
   */
  declarative_retention: boolean
  /**
   * How the signal tables are cut on their anchor time.
   */
  partitioning: Partitioning
  /**
   * Whether a retention pass can drop a whole partition instead of deleting rows one at
   * a time.
   */
  partition_drop_retention: boolean
  /**
   * Whether a start with nothing to apply issues no schema statement at all. False means
   * every process replays the schema on every boot, which is harmless on one node and is
   * metadata churn across a replicated deployment.
   */
  idempotent_schema_init: boolean
  /**
   * Whether this deployment answers the Intelligence endpoints. False means the store
   * holds no note or theme tables and every one of those routes refuses with 501 naming
   * this capability, so a client decides once whether to offer the feature rather than
   * showing a reader an empty page they cannot account for. Every backend that answers
   * gives the same themes: the clustering runs in the daemon over stored notes and not
   * in SQL, so there is nothing here for a backend to do differently.
   */
  theme_analysis: boolean
}

/**
 * One value a filter dimension takes in a project.
 */
export interface DiscoveredValue {
  /**
   * The value, exactly as it is stored, so it can be sent straight back as a filter with
   * no further mapping.
   */
  value: string
  /**
   * When mcpd last received a signal carrying it. This is arrival time and not the
   * signal's own timestamp, so a backfill of last year's data does not make a value look
   * current. Carried so a caller can rank down or grey out a value nothing has reported
   * for a month. Values are ordered by `value` and not by this: a list somebody reads
   * alphabetically is stable between polls, and one ordered by recency reshuffles under
   * them as data arrives.
   */
  last_seen: string
}

/**
 * The values one filter dimension takes, for a filter box. Answered from a
 * distinct-value list the daemon maintains as signals arrive, not by scanning the
 * signal tables, because the question is asked on every keystroke and the tables it
 * would otherwise scan are the largest thing the deployment holds.
 */
export interface DiscoveryResponse {
  /**
   * Which dimension was asked for.
   */
  facet: Facet
  /**
   * The label key the values belong to, echoed back for `metric_label_value`. Absent for
   * every other facet.
   */
  key?: string
  /**
   * The values, ordered by value, ascending.
   */
  values: DiscoveredValue[]
}

/**
 * What has arrived since a cursor. A poll is not a page: paging walks a fixed result
 * set backwards in the sort you chose, and polling walks forwards in arrival order and
 * never ends, so the two cursors are different things and are deliberately not
 * interchangeable. Rows come back oldest arrival first, so applying them in order to a
 * list you already hold gives the same result as having listed afresh.
 */
export interface DeltaResponse {
  /**
   * Which signal these rows are. Exactly one of the three arrays below is present, and
   * it is this one.
   */
  signal: DeltaSignal
  /**
   * Where the next poll resumes. Always present, and unchanged from the request when
   * nothing came back: moving it forward on an empty poll is what would drop a row that
   * lands a moment later with an earlier arrival time. Treat it as meaningless, and send
   * back the one you were given.
   */
  cursor: string
  /**
   * True when the page filled and more is waiting, so a caller draining a backlog polls
   * again at once rather than waiting for its next tick.
   */
  has_more: boolean
  /**
   * The runs that arrived, when `signal` is `traces`.
   */
  traces?: Trace[]
  /**
   * The steps that arrived, when `signal` is `spans`.
   */
  spans?: Span[]
  /**
   * The log lines that arrived, when `signal` is `logs`.
   */
  logs?: LogRecord[]
}

/**
 * The bounds a clustering pass runs under. They are published because each one changes
 * what a reader is looking at: a floor that has not been reached means no themes at
 * all rather than an empty result, and a ceiling that has been reached means every
 * share is a share of what was read. They are copied from the divergence watch, which
 * is the one windowed statistic mcpd already ships. `pass_armed` is the first of them
 * to read: the other three describe what a pass would do, and it says whether one
 * runs.
 */
export interface AspectLimits {
  /**
   * How many notes one aspect on one entity needs before a pass runs at all, 50 by
   * default. Below it the aspect reads `collecting` and no snapshot is written.
   */
  min_notes_to_cluster: number
  /**
   * The most notes one pass reads, 20,000 by default, taken newest first. A window
   * holding more is clustered from the newest of them and its snapshot is marked
   * `capped`.
   */
  max_notes_per_pass: number
  /**
   * How many notes the pass reads at a time while it fills a window, 500 by default. It
   * bounds what the pass holds, not what it covers.
   */
  page_size: number
  /**
   * How many aspects one project may define, the four built-ins included. It is
   * configuration and is published here so a client that lets an operator add an aspect
   * knows when to stop instead of finding out from a refusal.
   */
  max_aspects: number
  /**
   * Whether this deployment runs the clustering pass at all. False means no snapshot
   * will ever be written however many notes arrive, so every aspect stays `collecting`
   * or `processing` for ever and no theme route will have anything to answer with. It is
   * configuration and not a fault, and it is not the same thing as the 501 the theme
   * routes give: that says the storage backend cannot hold the tables, which nobody can
   * switch on, while this says nobody has switched the pass on yet. It is published here
   * rather than on every progress row because it is one fact about the deployment, and a
   * client reads this route once to learn what the aspects are called.
   */
  pass_armed: boolean
}

/**
 * Every aspect a project describes traces along, with the bounds the clustering pass
 * runs under. The four built-ins are rows in the same table as anything you define, so
 * there is no fallback list inside a client that can quietly disagree with this one.
 */
export interface AspectCatalogue {
  /**
   * The aspects, in `sort_order`. Disabled aspects are included, because notes already
   * recorded under one stay readable and a name that vanished from this list would make
   * them look like a mistake.
   */
  aspects: Aspect[]
  /**
   * The bounds a pass runs under in this deployment.
   */
  limits: AspectLimits
}

/**
 * One entity that has aspect notes, with the roll-up a list needs to be worth reading.
 * A row appears as soon as the first note is recorded, before any theme exists,
 * because an empty screen with a count moving on it is a different thing from an empty
 * screen and a reader has to be able to tell them apart.
 */
export interface EntityIndexRow {
  /**
   * The kind of thing this row is about.
   */
  entity_kind: EntityKind
  /**
   * Its name.
   */
  entity_name: string
  /**
   * How many of its traces carry at least one note. This is not the entity's trace
   * count: writing a note lags ingestion, so a busy agent can show far fewer here.
   */
  trace_count: number
  /**
   * How many notes are recorded about it.
   */
  note_count: number
  /**
   * How many aspects are switched on for this entity's project.
   */
  enabled_aspects: number
  /**
   * How many of those have themes to read. The two numbers read as `2 of 4` and are the
   * honest answer to whether this row is worth opening yet.
   */
  ready_aspects: number
  /**
   * The row's overall state, which is the least advanced of its enabled aspects, so a
   * row says `processing` while any one of them is still short of a snapshot. It never
   * reports the best case.
   */
  readiness: AspectReadiness
  /**
   * When the newest note about this entity was recorded, so a reader can tell a list
   * that is filling from one that stopped.
   */
  last_note_at: string
  /**
   * The cutoff of this entity's newest snapshot. Absent until a pass has written one,
   * which is the ordinary state of a row that is still collecting.
   */
  latest_snapshot_at?: string
}

/**
 * A page of the entities that have aspect notes.
 */
export interface EntityIndexResponse {
  /**
   * The matching entities, most recently written to first.
   */
  entities: EntityIndexRow[]
  /**
   * Cursor and totals.
   */
  page: Page
}

/**
 * How far one aspect has got for one entity. Counted from stored notes, so it is what
 * has happened and not a projection of what will.
 */
export interface AspectProgressRow {
  /**
   * Which aspect this row is about.
   */
  aspect: string
  /**
   * The state this aspect is in for this entity.
   */
  readiness: AspectReadiness
  /**
   * How many notes are recorded along it.
   */
  note_count: number
  /**
   * How many distinct traces those notes were written about.
   */
  trace_count: number
  /**
   * How many more notes this aspect needs before a pass will run. Zero once the floor is
   * reached, which is what turns a wait into a number a reader can watch.
   */
  notes_needed: number
  /**
   * When the newest note along this aspect was recorded. Absent when there are none.
   */
  last_note_at?: string
}

/**
 * What the daemon has recorded for one entity, aspect by aspect. This is the answer to
 * why a themes screen is empty, and it separates the three cases that look identical
 * on screen: nothing has been recorded, enough has been recorded and no pass has run
 * yet, or there are themes to read and the reader is looking at the wrong aspect.
 */
export interface AspectProgress {
  /**
   * The kind of entity this describes.
   */
  entity_kind: EntityKind
  /**
   * Its name.
   */
  entity_name: string
  /**
   * The entity's overall state, which is the least advanced of its enabled aspects.
   */
  readiness: AspectReadiness
  /**
   * How many of this entity's traces carry at least one note.
   */
  trace_count: number
  /**
   * One row per enabled aspect, in catalogue order.
   */
  aspects: AspectProgressRow[]
  /**
   * The newest snapshot for this entity, when a pass has written one. Absent while every
   * aspect is still collecting.
   */
  latest_snapshot?: ThemeSnapshot
}

/**
 * One snapshot in a timeline, with the reason it was chosen.
 */
export interface SnapshotLandmark {
  /**
   * The snapshot.
   */
  snapshot: ThemeSnapshot
  /**
   * Why this snapshot is in the selection. Present for `presentation=landmarks` only; an
   * inventory returns consecutive snapshots and chooses nothing, so there is nothing to
   * report.
   */
  reason?: SnapshotLandmarkReason
}

/**
 * A selection of one entity's snapshots, oldest first, so a timeline can be drawn
 * without reordering anything.
 */
export interface SnapshotListResponse {
  /**
   * The selected snapshots, oldest first.
   */
  snapshots: SnapshotLandmark[]
  /**
   * How many snapshots the entity has in the requested range. This is the number the
   * selection was drawn from and not the number returned, so a timeline can say what it
   * is standing in for.
   */
  total: number
  /**
   * Cursor and totals.
   */
  page: Page
}

/**
 * One node of one stage of a flow: a theme, the stage's noise, or the overflow the
 * stage folded away.
 */
export interface ThemeNode {
  /**
   * Identifies this node inside this flow, and is what a link names at either end. It is
   * not a theme id: the noise and overflow nodes have one of these and no theme id.
   */
  node_id: string
  /**
   * Whether this node is a theme, the noise or the overflow.
   */
  kind: ThemeNodeKind
  /**
   * The theme this node stands for. Present only when `kind` is `theme`, which is what
   * makes the other two impossible to drill into by accident.
   */
  theme_id?: string
  /**
   * What to show for this node: a theme's own label, or the words for noise or overflow.
   */
  label: string
  /**
   * How many traces this node accounts for.
   */
  trace_count: number
  /**
   * This node's share of the stage's traces, between 0 and 1. A stage's nodes cover all
   * of it, so its shares add to 1.
   */
  coverage: number
}

/**
 * One aspect's column in a flow. Stages come back in the order the aspects were asked
 * for, and links only ever join neighbouring stages, so the order decides which
 * co-occurrences are visible and is not a display preference.
 */
export interface ThemeStage {
  /**
   * Which aspect this stage is.
   */
  aspect: string
  /**
   * How many traces this stage accounts for.
   */
  trace_count: number
  /**
   * The stage's nodes, largest first, with the noise and overflow nodes last.
   */
  nodes: ThemeNode[]
}

/**
 * A ribbon between one node of a stage and one node of the next. It says the two
 * describe the same traces and nothing more: a link between a goal theme and an
 * outcome theme means they were written about the same runs, not that one caused the
 * other.
 */
export interface ThemeLink {
  /**
   * The node this link leaves, in the earlier stage.
   */
  source_node_id: string
  /**
   * The node it reaches, in the next stage.
   */
  target_node_id: string
  /**
   * How many traces are assigned to both ends.
   */
  trace_count: number
  /**
   * That count as a fraction of the source node's traces, between 0 and 1.
   */
  source_share: number
  /**
   * That count as a fraction of the target node's traces, between 0 and 1. The two
   * shares differ whenever the nodes are different sizes, and reading only one of them
   * is how a thin ribbon gets mistaken for a weak relationship.
   */
  target_share: number
}

/**
 * One snapshot read across two to four aspects at once: a stage per aspect, and the
 * links between neighbouring stages. Association, never causation.
 */
export interface ThemeFlow {
  /**
   * The snapshot this flow was computed from, so every count on the page can be read
   * against the window it came from.
   */
  snapshot: ThemeSnapshot
  /**
   * One stage per aspect, in the order they were asked for.
   */
  stages: ThemeStage[]
  /**
   * The links between neighbouring stages. There are none between stages that are not
   * neighbours, which is why the order of the aspects matters.
   */
  links: ThemeLink[]
}

/**
 * One theme in one snapshot, or the answer that this snapshot did not hold it.
 */
export interface ThemeDetail {
  /**
   * The snapshot that was read, with its own counts, so a caller can write `28 of 70
   * traces in this window` without a second request.
   */
  snapshot: ThemeSnapshot
  /**
   * The theme, when this snapshot holds it. Absent is an answer and not an error: the
   * theme did not exist in that window, which is what scrubbing a timeline back past a
   * theme's birth looks like. A theme id this entity has never had is a 404 instead.
   */
  theme?: Theme
}

/**
 * One note from a theme, with the run it was written about. The two ids are different
 * things: `note_id` identifies the sentence and `trace_id` identifies the run it
 * describes.
 */
export interface ThemeExample {
  /**
   * The note this example is.
   */
  note_id: string
  /**
   * The trace the note was written about.
   */
  trace_id: string
  /**
   * The sentence, after redaction. This is the wording that put the trace in this theme,
   * so a reader can see why it landed here rather than take it on trust.
   */
  text: string
  /**
   * How much this note's wording overlaps the theme's label, counted out of 64. It is a
   * count of shared sketch positions and not a percentage, and it is an integer on
   * purpose: no part of the clustering uses floating-point arithmetic, so two replicas
   * cannot order the same notes differently.
   */
  similarity: number
  /**
   * When the run began, so examples can be read against the period they came from rather
   * than against when the note was written.
   */
  trace_started_at: string
}

/**
 * A page of the notes behind one theme.
 */
export interface ThemeExampleListResponse {
  /**
   * The notes, most like the theme's label first.
   */
  examples: ThemeExample[]
  /**
   * Cursor and totals.
   */
  page: Page
}

/**
 * One theme in one snapshot of its life. Every count belongs to that snapshot alone,
 * and windows overlap, so a run of these is read as a series and never added up.
 */
export interface ThemeHistoryPoint {
  /**
   * The snapshot this point comes from.
   */
  snapshot_id: string
  /**
   * When that snapshot became the current state.
   */
  cutoff_at: string
  /**
   * Start of that snapshot's window, inclusive.
   */
  window_from: string
  /**
   * End of that snapshot's window, exclusive.
   */
  window_to: string
  /**
   * What had happened to the theme by this point.
   */
  state: ThemeState
  /**
   * How many notes were in the theme here.
   */
  note_count: number
  /**
   * How many distinct traces those notes were written about.
   */
  trace_count: number
  /**
   * The theme's share of the notes that snapshot read for this aspect, between 0 and 1.
   * Coverage and raw count can move in opposite directions, and coverage is the one that
   * says whether a theme is gaining ground.
   */
  coverage: number
  /**
   * The themes this one came out of at this point. Empty unless `state` is `split`.
   */
  split_from: string[]
  /**
   * The themes that came out of this one at this point. Both directions are stored, so a
   * caller never has to read a second theme's history to learn what happened to this
   * one.
   */
  split_into: string[]
  /**
   * The themes that became this one at this point. Empty unless `state` is `merge`.
   */
  merged_from: string[]
  /**
   * The theme this one became at this point, when it stopped being its own theme.
   */
  merged_into: string[]
}

/**
 * One theme across every stored snapshot that holds it, oldest first. This is the only
 * theme response that names no single snapshot, because the series is the answer.
 */
export interface ThemeHistory {
  /**
   * The theme this history is of.
   */
  theme_id: string
  /**
   * The aspect it was clustered along.
   */
  aspect: string
  /**
   * The theme's label at its newest point. A label can change between snapshots, because
   * the note most like the others can change while the theme holds together.
   */
  label: string
  /**
   * The series, oldest first, so nothing has to be reordered before it is drawn or
   * differenced.
   */
  points: ThemeHistoryPoint[]
  /**
   * Cursor and totals.
   */
  page: Page
}

/**
 * Every aspect note recorded about one trace, which is at most one per aspect. Read it
 * beside the trace itself: a note is the sentence that put this run in each of its
 * themes, so it is the shortest route from a theme back to what actually happened.
 */
export interface TraceAspectNotes {
  /**
   * The trace these notes are about.
   */
  trace_id: string
  /**
   * The notes, in catalogue order. Empty when nothing has written one yet, which is
   * ordinary for a recent trace.
   */
  notes: AspectNote[]
}

/**
 * An email address and a password, for sign-up and for log-in.
 */
export interface AccountCredentials {
  /**
   * The account's email address. Compared without regard to case, and with surrounding
   * spaces removed.
   */
  email: string
  /**
   * At least 10 characters and at most 1024 bytes. Stored only as an argon2id hash.
   */
  password: string
  /**
   * The six-digit code from an authenticator app, or one of the recovery codes, when the
   * account has two-step log-in on. Ignored by sign-up.
   */
  totp_code?: string
}

/**
 * A logged-in session. Send `token` as `Authorization: Bearer <token>`.
 */
export interface AccountSession {
  /**
   * The session token. It is shown once: the daemon keeps only its SHA-256 digest.
   */
  token: string
  /**
   * When the session stops working. Log in again after this.
   */
  expires_at: string
  /**
   * The account's email address, as stored.
   */
  email: string
  /**
   * The project reads go to when they name none. The account's own project, or the first
   * project of the organisation a single sign-on session is limited to. Absent when the
   * session reaches no project yet.
   */
  project?: string
  /**
   * Set when the session came from an organisation's single sign-on. The session then
   * reaches that organisation's projects only.
   */
  org_id?: string
}

/**
 * The account a session belongs to.
 */
export interface Account {
  /**
   * The account's email address, as stored.
   */
  email: string
  /**
   * The project created with the account at sign-up. Absent for an account that was
   * invited, or created by single sign-on or SCIM.
   */
  project?: string
  /**
   * When the account was created.
   */
  created_at: string
  /**
   * Whether the address has been confirmed by a link sent to it.
   */
  email_verified: boolean
  /**
   * Whether two-step log-in is on.
   */
  totp_enabled: boolean
  /**
   * Whether the account can log in with a password. False for an account created by
   * single sign-on or SCIM that has never set one.
   */
  has_password: boolean
  /**
   * The organisation that created this account through single sign-on or SCIM. That
   * organisation can deactivate it.
   */
  managed_by?: string
}

/**
 * One API key an account holds. The secret is not part of it: the daemon keeps only a
 * digest.
 */
export interface AccountKey {
  /**
   * The key's identifier, for revoking it. Not a secret.
   */
  id: string
  /**
   * What the key may do.
   */
  scope: KeyScope
  /**
   * The one project the key reaches.
   */
  project: string
  /**
   * The label given when the key was created.
   */
  name?: string
  /**
   * The first characters of the secret, so you can tell which key a process holds
   * without the daemon storing the secret.
   */
  prefix: string
  /**
   * When the key was created.
   */
  created_at: string
  /**
   * The email of the account that created the key. Set in an organisation's key list, so
   * an admin can see whose key it is.
   */
  created_by?: string
}

/**
 * A request for a new API key.
 */
export interface CreateAccountKeyRequest {
  /**
   * What the key may do. One scope per key.
   */
  scope: KeyScope
  /**
   * A label for the key, such as the service that will hold it. At most 100 characters.
   */
  name?: string
  /**
   * The project the key reaches. Defaults to the account's own project. Your role on the
   * project must allow the scope.
   */
  project?: string
}

/**
 * A new API key, with its secret. This is the only time the secret is shown.
 */
export interface CreatedAccountKey {
  /**
   * The key as it is listed from now on.
   */
  key: AccountKey
  /**
   * The key itself. Send it as `Authorization: Bearer <secret>`. Copy it now: the daemon
   * keeps only its SHA-256 digest and cannot show it again.
   */
  secret: string
}

/**
 * A plain answer that changes nothing you can read back, such as "check your email".
 */
export interface AccountNotice {
  /**
   * What happened, and what to do next.
   */
  message: string
}

/**
 * A one-time token from a link the daemon sent by email.
 */
export interface TokenRequest {
  /**
   * The token, as it appears in the link.
   */
  token: string
}

/**
 * A request for a password reset link.
 */
export interface PasswordForgotRequest {
  /**
   * The address to send the link to. The answer is the same whether or not an account
   * uses it.
   */
  email: string
}

/**
 * A new password, with the token from a reset link.
 */
export interface PasswordResetRequest {
  /**
   * The token from the reset link. It works once, for one hour.
   */
  token: string
  /**
   * The new password. At least 10 characters and at most 1024 bytes.
   */
  password: string
  /**
   * A code from the authenticator app, or a recovery code, when the account has two-step
   * log-in on. A reset proves the address, not the second factor.
   */
  totp_code?: string
}

/**
 * A new password for the logged-in account.
 */
export interface PasswordChangeRequest {
  /**
   * The password in use now. Required when the account has one.
   */
  current_password?: string
  /**
   * The new password. At least 10 characters and at most 1024 bytes.
   */
  new_password: string
}

/**
 * A new email address for the logged-in account.
 */
export interface EmailChangeRequest {
  /**
   * The new address. A link is sent to it, and the change happens when the link is
   * opened.
   */
  email: string
  /**
   * The account's password. Required when the account has one.
   */
  password?: string
}

/**
 * Confirmation for deleting the logged-in account.
 */
export interface AccountDeleteRequest {
  /**
   * The account's password. Required when the account has one.
   */
  password?: string
  /**
   * A code from the authenticator app. Required when two-step log-in is on.
   */
  totp_code?: string
}

/**
 * What deleting an account removed.
 */
export interface AccountDeletion {
  /**
   * Organisations that had this account as their only member, deleted with it.
   */
  orgs_deleted: number
  /**
   * Projects whose stored signals and runs were erased, because their organisation was
   * deleted.
   */
  projects_erased: string[]
}

/**
 * One live session of the logged-in account. The token is not part of it.
 */
export interface SessionInfo {
  /**
   * The session's identifier, for ending it. Not a secret.
   */
  id: string
  /**
   * When the session started.
   */
  created_at: string
  /**
   * When it stops working.
   */
  expires_at: string
  /**
   * True for the session this request carries.
   */
  current: boolean
  /**
   * How it started: `password`, `sso`, `invite`, `verify` or `reset`.
   */
  via: string
  /**
   * The organisation a single sign-on session is limited to.
   */
  org_id?: string
  /**
   * The browser or client that started it, as it described itself.
   */
  user_agent?: string
  /**
   * The client address that started it.
   */
  address?: string
}

/**
 * A new authenticator secret, not yet in use. Confirm it with a code to turn two-step
 * log-in on.
 */
export interface TotpSetup {
  /**
   * The secret in base32, for typing into an authenticator app.
   */
  secret: string
  /**
   * The same secret as an `otpauth://` URI.
   */
  otpauth_uri: string
  /**
   * The URI as a QR code, an SVG document.
   */
  qr_svg: string
}

/**
 * A code from an authenticator app.
 */
export interface TotpCodeRequest {
  /**
   * The six-digit code.
   */
  code: string
}

/**
 * Two-step log-in is on.
 */
export interface TotpEnabled {
  /**
   * Ten single-use codes that stand in for the authenticator app. They are shown once:
   * the daemon keeps only their digests.
   */
  recovery_codes: string[]
}

/**
 * Confirmation for turning two-step log-in off.
 */
export interface TotpDisableRequest {
  /**
   * The account's password. Required when the account has one.
   */
  password?: string
  /**
   * A code from the authenticator app, or a recovery code.
   */
  code: string
}

/**
 * An organisation: the owner of projects, members and single sign-on settings.
 */
export interface Org {
  /**
   * The organisation's identifier.
   */
  id: string
  /**
   * Its display name.
   */
  name: string
  /**
   * Your role in it.
   */
  role: OrgRole
  /**
   * When it was created.
   */
  created_at: string
  /**
   * The most signals all its projects together may store per minute. Absent means no
   * ceiling.
   */
  quota_per_minute?: number
  /**
   * How long its projects keep data, in days. Absent means forever.
   */
  retention_days?: number
  /**
   * How many members it has.
   */
  members: number
  /**
   * How many projects it owns.
   */
  projects: number
  /**
   * True when the daemon's configuration file declares this organisation. Its single
   * sign-on settings are then read from that file and cannot be changed here.
   */
  managed_by_config: boolean
}

/**
 * A new organisation.
 */
export interface CreateOrgRequest {
  /**
   * Its display name, at most 100 characters.
   */
  name: string
}

/**
 * Changes to an organisation. A field left out keeps its value.
 */
export interface UpdateOrgRequest {
  /**
   * A new display name, at most 100 characters.
   */
  name?: string
  /**
   * A ceiling on signals per minute across all the organisation's projects, at most the
   * daemon's own ceiling. 0 restores the daemon's default. Owner only.
   */
  quota_per_minute?: number
  /**
   * How long to keep data, in days, at most the daemon's own window. 0 restores the
   * daemon's default. Owner only.
   */
  retention_days?: number
}

/**
 * What deleting an organisation removed.
 */
export interface OrgDeletion {
  /**
   * Projects whose stored signals and runs were erased.
   */
  projects_erased: string[]
}

/**
 * A role on one project.
 */
export interface ProjectRoleGrant {
  /**
   * The project.
   */
  project: string
  /**
   * The role there.
   */
  role: ProjectRole
}

/**
 * One member of an organisation.
 */
export interface OrgMember {
  /**
   * The member's account.
   */
  account_id: string
  /**
   * The member's email address.
   */
  email: string
  /**
   * The member's role.
   */
  role: OrgRole
  /**
   * How they joined: `created`, `invite`, `sso`, `scim` or `config`.
   */
  via: string
  /**
   * When they joined.
   */
  joined_at: string
  /**
   * Roles on single projects that replace the organisation role there.
   */
  project_roles: ProjectRoleGrant[]
  /**
   * True when this organisation created the account through single sign-on or SCIM.
   */
  managed: boolean
  /**
   * Whether the member has two-step log-in on.
   */
  totp_enabled: boolean
}

/**
 * A new role for a member.
 */
export interface UpdateMemberRequest {
  /**
   * The role. Only an owner may grant or remove `owner`, and the last owner cannot be
   * demoted.
   */
  role: OrgRole
}

/**
 * A role for a member on one project.
 */
export interface SetProjectRoleRequest {
  /**
   * A project the organisation owns.
   */
  project: string
  /**
   * The role there. Absent removes the project role, so the organisation role applies
   * again.
   */
  role?: ProjectRole
}

/**
 * An invitation not yet accepted. The token is not part of it.
 */
export interface OrgInvite {
  /**
   * The invitation's identifier, for revoking it.
   */
  id: string
  /**
   * The address it was sent to.
   */
  email: string
  /**
   * The role it grants.
   */
  role: OrgRole
  /**
   * Project roles it grants as well.
   */
  project_roles: ProjectRoleGrant[]
  /**
   * The email of the member who sent it. Absent when the configuration file or SCIM sent
   * it.
   */
  invited_by?: string
  /**
   * When it was sent.
   */
  created_at: string
  /**
   * When it stops working.
   */
  expires_at: string
}

/**
 * An invitation to an organisation.
 */
export interface CreateInviteRequest {
  /**
   * Who to invite. The link is sent to this address.
   */
  email: string
  /**
   * The role to grant. Only an owner may invite an owner.
   */
  role: OrgRole
  /**
   * Project roles to grant as well.
   */
  project_roles?: ProjectRoleGrant[]
}

/**
 * What an invitation offers, read with its token before accepting it.
 */
export interface InviteLookup {
  /**
   * The organisation.
   */
  org_name: string
  /**
   * The address it was sent to. Accepting it joins the account with this address.
   */
  email: string
  /**
   * The role it grants.
   */
  role: OrgRole
  /**
   * Whether an account with this address exists. If it does, log in as it to accept; if
   * not, accepting creates it.
   */
  account_exists: boolean
  /**
   * When it stops working.
   */
  expires_at: string
}

/**
 * Acceptance of an invitation.
 */
export interface InviteAcceptRequest {
  /**
   * The token from the invitation link.
   */
  token: string
  /**
   * A password for the new account, when no account uses the invited address. Ignored
   * otherwise.
   */
  password?: string
}

/**
 * An accepted invitation.
 */
export interface InviteAccepted {
  /**
   * The organisation joined.
   */
  org: Org
  /**
   * A session for the account accepting created. Absent when an existing account
   * accepted with its own session.
   */
  session?: AccountSession
}

/**
 * A project an organisation owns.
 */
export interface OrgProject {
  /**
   * The project name. Random, so nobody can choose one that already holds somebody
   * else's data.
   */
  project: string
  /**
   * A display label.
   */
  label?: string
  /**
   * When it was created or attached.
   */
  created_at: string
}

/**
 * A new project in an organisation.
 */
export interface CreateProjectRequest {
  /**
   * A display label, at most 100 characters.
   */
  label?: string
}

/**
 * How much an organisation is using, against its ceilings. Counted since the daemon
 * started.
 */
export interface OrgUsage {
  /**
   * The organisation.
   */
  org_id: string
  /**
   * Its ceiling on signals per minute, when it has one.
   */
  quota_per_minute?: number
  /**
   * Signals stored in the current one-minute window.
   */
  admitted_this_minute: number
  /**
   * Signals stored since the daemon started.
   */
  admitted_total: number
  /**
   * Signals refused for the ceiling since the daemon started.
   */
  refused_total: number
  /**
   * How long its projects keep data, in days.
   */
  retention_days?: number
  /**
   * Members.
   */
  members: number
  /**
   * Projects.
   */
  projects: number
  /**
   * API keys on its projects.
   */
  keys: number
  /**
   * When counting started.
   */
  since: string
}

/**
 * An organisation's single sign-on settings. The client secret is not part of it.
 */
export interface SsoConfig {
  /**
   * The kind of provider.
   */
  provider: SsoProvider
  /**
   * The OpenID Connect issuer URL. Absent for GitHub.
   */
  issuer?: string
  /**
   * The client id registered with the provider.
   */
  client_id: string
  /**
   * Whether a client secret is stored.
   */
  has_client_secret: boolean
  /**
   * Email domains this organisation claims. Single sign-on creates accounts only for
   * these domains, and "Log in with SSO" finds the organisation by them.
   */
  domains: string[]
  /**
   * When true, members whose email is in a claimed domain cannot log in with a password.
   */
  enforced: boolean
  /**
   * The role given to a member single sign-on creates, when no group maps to a role.
   * Never `owner`.
   */
  default_role: OrgRole
  /**
   * The ID token claim that lists the member's groups, mapped to roles by the group
   * roles.
   */
  groups_claim?: string
  /**
   * The redirect URL to register with the provider.
   */
  redirect_url: string
  /**
   * True when these settings come from the configuration file.
   */
  managed_by_config: boolean
  /**
   * When they were last changed.
   */
  updated_at: string
}

/**
 * New single sign-on settings for an organisation.
 */
export interface PutSsoConfigRequest {
  /**
   * The kind of provider.
   */
  provider: SsoProvider
  /**
   * The OpenID Connect issuer URL, such as `https://accounts.google.com`. Required for
   * `oidc`. Must be `https`.
   */
  issuer?: string
  /**
   * The client id registered with the provider.
   */
  client_id: string
  /**
   * The client secret. Left out, the stored one is kept.
   */
  client_secret?: string
  /**
   * Email domains to claim. You can claim the domain of your own verified address. A
   * domain belongs to one organisation.
   */
  domains: string[]
  /**
   * Refuse password log-in for members in the claimed domains. Turning it on ends their
   * password sessions.
   */
  enforced: boolean
  /**
   * The role for a member single sign-on creates. `admin`, `member` or `viewer`.
   */
  default_role: OrgRole
  /**
   * The ID token claim that lists groups, such as `groups`.
   */
  groups_claim?: string
}

/**
 * A group from the identity provider, and the role its members get.
 */
export interface GroupRole {
  /**
   * The group name, as single sign-on or SCIM sends it. Compared without regard to case.
   */
  group: string
  /**
   * The role. Never `owner`.
   */
  role: OrgRole
}

/**
 * Whether an organisation has a SCIM token. The token is not part of it.
 */
export interface ScimTokenInfo {
  /**
   * Whether a token exists.
   */
  configured: boolean
  /**
   * The first characters of the token.
   */
  prefix?: string
  /**
   * When it was created.
   */
  created_at?: string
  /**
   * The SCIM base URL to give your identity provider.
   */
  base_url: string
}

/**
 * A new SCIM token. This is the only time it is shown, and it replaces any earlier
 * token.
 */
export interface CreatedScimToken {
  /**
   * The bearer token for your identity provider. The daemon keeps only its SHA-256
   * digest.
   */
  token: string
  /**
   * Its first characters.
   */
  prefix: string
  /**
   * When it was created.
   */
  created_at: string
  /**
   * The SCIM base URL to give your identity provider.
   */
  base_url: string
}

/**
 * Where to start single sign-on. Give one of the two.
 */
export interface SsoStartRequest {
  /**
   * Your email address. Its domain finds the organisation.
   */
  email?: string
  /**
   * The organisation, when you know it.
   */
  org_id?: string
}

/**
 * Where to send the browser for single sign-on.
 */
export interface SsoStart {
  /**
   * The provider's authorization URL, with state, nonce and the PKCE challenge in it.
   */
  authorize_url: string
  /**
   * A secret to keep in this browser and send back with the callback. It ties the
   * callback to the browser that started it, so a link from somebody else cannot log you
   * in as them.
   */
  binding: string
  /**
   * The organisation.
   */
  org_name: string
}

/**
 * What the provider sent back to the redirect URL.
 */
export interface SsoCallbackRequest {
  /**
   * The `state` parameter.
   */
  state: string
  /**
   * The `code` parameter.
   */
  code: string
  /**
   * The binding from the start of the flow.
   */
  binding: string
}

/**
 * One unit of work. Spans are the backbone of mcpd: logs, metrics, scores and feedback
 * all hang off a span or a trace.
 * Stored in the `spans` table.
 */
export interface Span {
  /**
   * 128-bit trace id, 32 lowercase hex characters. OpenTelemetry-compatible.
   */
  trace_id: string
  /**
   * 64-bit span id, 16 lowercase hex characters. Unique within the trace.
   */
  span_id: string
  /**
   * The parent span within this trace. Absent on the root.
   */
  parent_span_id?: string
  /**
   * A parent from a foreign tracing system that mcpd did not create. Carried for
   * correlation, never used for mcpd's own tree.
   */
  external_parent_span_id?: string
  /**
   * What this operation is called. Keep it low-cardinality: `search_docs`, not
   * `search_docs(query=hello)`.
   */
  name: string
  /**
   * What kind of operation this is.
   */
  kind: SpanKind
  /**
   * The outcome.
   */
  status: SpanStatus
  /**
   * When the operation began, in UTC with nanosecond precision.
   */
  start_time: string
  /**
   * When it finished. Absent while the span is still running.
   */
  end_time?: string
  /**
   * Derived from the timestamps at ingest, stored so the database never has to compute
   * it to sort.
   */
  duration_ns?: number
  /**
   * True when this span has no parent inside the trace. Materialised so the trace list
   * never scans children.
   */
  is_root: boolean
  /**
   * True for a point in time rather than a duration. Event spans have no end.
   */
  is_event: boolean
  /**
   * The kind of thing that produced this span.
   */
  entity_kind?: EntityKind
  /**
   * Stable identifier of the producing entity.
   */
  entity_id?: string
  /**
   * Display name of the producing entity.
   */
  entity_name?: string
  /**
   * What went in. For a model call this is the full prompt, and it is the single most
   * valuable field mcpd stores.
   */
  input?: unknown
  /**
   * What came out. For a model call this is the completion.
   */
  output?: unknown
  /**
   * Kind-specific structured detail. See the attribute reference for what each kind
   * carries.
   */
  attributes?: unknown
  /**
   * Anything you want to attach. mcpd never interprets this.
   */
  metadata?: unknown
  /**
   * Token counts, on model and embedding spans.
   */
  usage?: TokenUsage
  /**
   * Computed cost, on spans that consumed tokens.
   */
  cost?: Cost
  /**
   * How the model was called: the sampling settings the caller chose. Absent on a span
   * that made no model call, and on one whose producer did not record them.
   */
  model_parameters?: ModelParameters
  /**
   * Failure detail, when `status` is `error`.
   */
  error?: SpanError
  /**
   * Causal references to other spans or traces.
   */
  links?: SpanLink[]
  /**
   * Free labels for filtering. Only meaningful on a root span, where they label the
   * whole trace.
   */
  tags?: string[]
  /**
   * Which project, environment, session, thread and user this belongs to.
   */
  correlation: Correlation
  /**
   * Model identifier, denormalised out of `attributes` so it can be indexed and grouped.
   */
  model?: string
  /**
   * Model provider, denormalised for the same reason.
   */
  provider?: string
  /**
   * When mcpd accepted this span. Differs from `start_time` for buffered or replayed
   * data, and is what retention is measured against.
   */
  received_at: string
  /**
   * Producer-supplied idempotency key. A retried batch with the same key is accepted
   * once.
   */
  ingest_key?: string
}

/**
 * A whole run, projected from its spans. mcpd materialises this at ingest so the list
 * view is one indexed read rather than an aggregation over every span. One per project
 * and trace id: a trace id crosses service boundaries, so two projects that call each
 * other each hold their own run under it, built from their own spans.
 * Stored in the `traces` table.
 */
export interface Trace {
  /**
   * The trace this summarises.
   */
  trace_id: string
  /**
   * The root span, when one has arrived.
   */
  root_span_id?: string
  /**
   * The root span's name.
   */
  name: string
  /**
   * The root span's kind.
   */
  kind: SpanKind
  /**
   * `error` when any span in the trace failed, otherwise the root's status.
   */
  status: SpanStatus
  /**
   * Earliest start across the trace.
   */
  start_time: string
  /**
   * Latest end across the trace. Absent while anything is still running.
   */
  end_time?: string
  /**
   * Wall-clock span of the whole trace.
   */
  duration_ns?: number
  /**
   * How many spans have arrived.
   */
  span_count: number
  /**
   * How many of them failed.
   */
  error_count: number
  /**
   * Token usage summed across every model span in the trace.
   */
  usage?: TokenUsage
  /**
   * Cost summed across the trace. This is the number a bill is reconciled against.
   */
  cost?: Cost
  /**
   * How many model calls the run made.
   */
  model_count: number
  /**
   * How many tool calls the run made.
   */
  tool_count: number
  /**
   * Tags from the root span.
   */
  tags?: string[]
  /**
   * Context inherited from the root span.
   */
  correlation: Correlation
  /**
   * Mean score per scorer id, maintained as scores arrive, so the list can sort by
   * quality without a join.
   */
  score_summary?: unknown
  /**
   * Counts per feedback kind and value.
   */
  feedback_summary?: unknown
  /**
   * True once the root span has ended and no span is still open.
   */
  is_complete: boolean
  /**
   * When the first span of this trace arrived.
   */
  received_at: string
}

/**
 * A structured log line, correlated to the span that produced it. Logs are stored
 * beside traces so 'show me the logs for this run' is one query, not a context switch
 * to another tool.
 * Stored in the `logs` table.
 */
export interface LogRecord {
  /**
   * Unique id for this record.
   */
  log_id: string
  /**
   * The trace this line belongs to, when it was emitted inside one.
   */
  trace_id?: string
  /**
   * The span this line belongs to.
   */
  span_id?: string
  /**
   * When the line was emitted.
   */
  timestamp: string
  /**
   * Severity.
   */
  level: LogLevel
  /**
   * The rendered message.
   */
  message: string
  /**
   * Structured key/value context. Redacted like any payload.
   */
  fields?: unknown
  /**
   * Which logger emitted it.
   */
  logger?: string
  /**
   * Project, environment, session and user.
   */
  correlation: Correlation
  /**
   * When mcpd accepted it.
   */
  received_at: string
}

/**
 * One numeric observation. mcpd aggregates these into time buckets on read, and keeps
 * raw points only as long as the raw retention class allows.
 * Stored in the `metrics` table.
 */
export interface MetricPoint {
  /**
   * Unique id for this observation.
   */
  metric_id: string
  /**
   * Metric name, such as `mcpd.model.duration`.
   */
  name: string
  /**
   * How it aggregates.
   */
  kind: MetricKind
  /**
   * The observed value.
   */
  value: number
  /**
   * UCUM unit, such as `ms`, `By` or `{token}`.
   */
  unit?: string
  /**
   * When it was observed.
   */
  timestamp: string
  /**
   * Dimensions. Passed through the cardinality filter, which rejects unbounded keys.
   */
  labels?: unknown
  /**
   * Histogram bucket boundaries and counts, on histogram points.
   */
  buckets?: unknown
  /**
   * Exemplar link: the trace that produced this observation.
   */
  trace_id?: string
  /**
   * Exemplar link: the span that produced it. This is what turns a spike on a chart into
   * a specific run you can open.
   */
  span_id?: string
  /**
   * Project and environment.
   */
  correlation: Correlation
  /**
   * When mcpd accepted it.
   */
  received_at: string
}

/**
 * A machine evaluation of a span or a trace.
 * Stored in the `scores` table.
 */
export interface Score {
  /**
   * Unique id.
   */
  score_id: string
  /**
   * The scored trace.
   */
  trace_id?: string
  /**
   * The scored span, when the score is about one step rather than the whole run.
   */
  span_id?: string
  /**
   * Which scorer produced this, such as `answer_relevance`.
   */
  scorer_id: string
  /**
   * The scorer's version. A score is only comparable to another score from the same
   * version.
   */
  scorer_version?: string
  /**
   * Live, offline, experiment or manual.
   */
  source: ScoreSource
  /**
   * The numeric result.
   */
  value: number
  /**
   * Lower bound of the scale. Without it, a bare number is not interpretable.
   */
  min?: number
  /**
   * Upper bound of the scale.
   */
  max?: number
  /**
   * A categorical result, when the scorer produces one alongside the number.
   */
  label?: string
  /**
   * Why. For an LLM judge this is the judge's explanation, and it is what makes a score
   * arguable rather than magic.
   */
  reason?: string
  /**
   * The trace of the scoring run itself, so a suspicious score can be debugged like any
   * other run.
   */
  score_trace_id?: string
  /**
   * What producing this score cost. Judged evaluation is not free and mcpd refuses to
   * hide that.
   */
  cost?: Cost
  /**
   * Anything else.
   */
  metadata?: unknown
  /**
   * When the score was produced.
   */
  timestamp: string
  /**
   * Inherited from the target.
   */
  correlation: Correlation
}

/** Spec-declared defaults for `Score`. Spread these when constructing one. */
export const scoreDefaults = {
  min: 0,
  max: 1,
} satisfies Partial<Score>

/**
 * A human evaluation. Kept distinct from `Score` because the two answer different
 * questions and must never be averaged together.
 * Stored in the `feedback` table.
 */
export interface Feedback {
  /**
   * Unique id.
   */
  feedback_id: string
  /**
   * The trace being judged.
   */
  trace_id?: string
  /**
   * The span being judged.
   */
  span_id?: string
  /**
   * Thumbs, rating, correction, tag or comment.
   */
  kind: FeedbackKind
  /**
   * The judgement. Its shape follows `kind`.
   */
  value: unknown
  /**
   * Free prose the person added.
   */
  comment?: string
  /**
   * Who gave it. Hashed by default.
   */
  author_id?: string
  /**
   * Whether the author was an end user, a reviewer or an operator.
   */
  author_kind?: string
  /**
   * When it was given.
   */
  timestamp: string
  /**
   * Anything else.
   */
  metadata?: unknown
  /**
   * Inherited from the target.
   */
  correlation: Correlation
}

/**
 * The tenancy boundary. Every signal belongs to exactly one project; retention,
 * redaction, sampling, budgets and access are all configured per project.
 * Stored in the `projects` table.
 */
export interface Project {
  /**
   * Stable identifier, used in API paths and ingest keys.
   */
  project_id: string
  /**
   * Display name.
   */
  name: string
  /**
   * When it was created.
   */
  created_at: string
  /**
   * How long to keep signals. Absent means the server default.
   */
  retention_days?: number
  /**
   * The sampling policy for this project.
   */
  sampling?: unknown
  /**
   * Extra redaction rules, applied on top of the server defaults.
   */
  redaction?: unknown
  /**
   * Cost budget and what happens when it is exceeded.
   */
  budget?: unknown
  /**
   * Anything else.
   */
  metadata?: unknown
}

/**
 * A fixed collection of inputs, with optional expected outputs, that an experiment
 * runs against. Datasets are how a change is proved not to regress.
 * Stored in the `datasets` table.
 */
export interface Dataset {
  /**
   * Stable identifier.
   */
  dataset_id: string
  /**
   * Display name.
   */
  name: string
  /**
   * What this dataset is for.
   */
  description?: string
  /**
   * How many items it holds.
   */
  item_count: number
  /**
   * When it was created.
   */
  created_at: string
  /**
   * Which project owns it.
   */
  correlation: Correlation
}

/**
 * One case in a dataset.
 * Stored in the `dataset_items` table.
 */
export interface DatasetItem {
  /**
   * Stable identifier.
   */
  item_id: string
  /**
   * Which dataset this belongs to.
   */
  dataset_id: string
  /**
   * What to feed the system under test.
   */
  input: unknown
  /**
   * The reference answer, when there is one. Absent for open-ended cases judged by a
   * scorer.
   */
  expected?: unknown
  /**
   * Anything else, such as a difficulty label or a source citation.
   */
  metadata?: unknown
  /**
   * The production trace this case was captured from, when it was. This is how a bug
   * report becomes a permanent regression test.
   */
  source_trace_id?: string
}

/**
 * One evaluation run of a system against a dataset. Comparing two experiments is how a
 * change is accepted or rejected.
 * Stored in the `experiments` table.
 */
export interface Experiment {
  /**
   * Stable identifier.
   */
  experiment_id: string
  /**
   * Display name.
   */
  name: string
  /**
   * What it ran against.
   */
  dataset_id: string
  /**
   * Where it got to.
   */
  status: ExperimentStatus
  /**
   * When it began.
   */
  started_at?: string
  /**
   * When it ended.
   */
  finished_at?: string
  /**
   * Cases attempted.
   */
  item_count: number
  /**
   * Aggregate score per scorer, with mean, standard deviation and count.
   */
  summary?: unknown
  /**
   * The experiment this one is compared against.
   */
  baseline_id?: string
  /**
   * What the whole run cost.
   */
  cost?: Cost
  /**
   * Why it failed, when it did.
   */
  error?: string
  /**
   * The configuration under test: model, prompt version, git sha.
   */
  metadata?: unknown
  /**
   * Which project owns it.
   */
  correlation: Correlation
}

/**
 * One recorded attempt to read stored payloads, or to delete them. Written by the
 * handlers that hand prompts and completions back out of mcpd, so the question "who
 * read this run" has an answer. The audit log is not trimmed by the retention pass: a
 * log that deleted itself on the same schedule as the data it describes could not say
 * who read the data that has since gone.
 * Stored in the `audit_log` table.
 */
export interface AuditEvent {
  /**
   * Unique id.
   */
  audit_id: string
  /**
   * When the request was handled.
   */
  timestamp: string
  /**
   * Which key acted, as a short hex digest of it. The key itself is never written here.
   * `anonymous` when the request carried no key, which is what an unprotected daemon
   * looks like.
   */
  principal: string
  /**
   * The end identity a proxy asserted for this request. Always empty in this build. The
   * field exists from the first release so that proxy identity can be added later
   * without a migration.
   */
  subject: string
  /**
   * What was done, named as the endpoint id: `get_trace`, `list_spans`, `delete_trace`.
   */
  action: string
  /**
   * The project the request was made against.
   */
  project: string
  /**
   * The trace or dataset the request named, when it named one.
   */
  resource?: string
  /**
   * Whether authorisation allowed the request or refused it.
   */
  result: AuditResult
}

/**
 * A stored definition. Immutable per version: a run pins the version it started
 * against, and that version never changes underneath it.
 * Stored in the `definitions` table.
 */
export interface Definition {
  /**
   * Which project owns it.
   */
  project: string
  /**
   * Its identifier within the project.
   */
  id: string
  /**
   * Its version. A run started without one pins the highest stored version at that
   * moment.
   */
  version: number
  /**
   * What kind of definition it is.
   */
  kind: DefinitionKind
  /**
   * The document as stored, minus `version`.
   */
  body: unknown
  /**
   * When this version was stored.
   */
  created_at: string
}

/**
 * One execution of one definition version. The daemon owns the run between steps: its
 * journal, its deadlines, its waits, its cancel and its trace. Your own process owns
 * everything inside a step.
 * Stored in the `runs` table.
 */
export interface Run {
  /**
   * Its identifier, minted at start.
   */
  run_id: string
  /**
   * Which project owns it.
   */
  project: string
  /**
   * Which definition it runs.
   */
  definition_id: string
  /**
   * Which version. Pinned at start and never re-resolved.
   */
  definition_version: number
  /**
   * Where it is.
   */
  status: RunStatus
  /**
   * The frontier: where in the graph the run has reached.
   */
  cursor: RunCursor
  /**
   * What it was started with, after validation against the definition's `input_schema`.
   */
  input?: unknown
  /**
   * What the definition's `output` template rendered when the run reached the end of the
   * graph.
   */
  output?: unknown
  /**
   * A JSON object merged shallowly on every `complete` that sends one. Later steps read
   * it through the lease.
   */
  state?: unknown
  /**
   * Why it failed, when it did: the failing step's message, or the ceiling it spent.
   */
  error?: string
  /**
   * The context stored at start and copied onto every span the daemon writes for the
   * run, with `run_id` set to this run's id.
   */
  correlation: Correlation
  /**
   * The trace every span of this run belongs to. 32 lowercase hex characters, minted at
   * start.
   */
  trace_id: string
  /**
   * How many steps this run has completed. Counted against `max_steps` on the root of
   * the run tree.
   */
  steps_used: number
  /**
   * The step ceiling from the definition's `limits`, when it declares one. On a root it
   * bounds the whole tree.
   */
  max_steps?: number
  /**
   * What the run's steps have cost so far, priced from the usage workers reported.
   */
  cost_usd: number
  /**
   * The spend ceiling from `limits`, when declared. A claim refused for a spent ceiling
   * ends the run `failed`.
   */
  max_cost_usd?: number
  /**
   * When `limits.run_timeout_ms` passes. Absent when the definition declares no timeout,
   * and once the run has ended.
   */
  deadline_at?: string
  /**
   * The key the start request carried, when it carried one. A repost of it within 24
   * hours returns this run.
   */
  idempotency_key?: string
  /**
   * The root of the run tree, which ceilings are enforced against. Equal to `run_id` on
   * a root.
   */
  root_run_id: string
  /**
   * The run whose `subworkflow` step started this one.
   */
  parent_run_id?: string
  /**
   * The step in the parent that this run is the body of.
   */
  parent_step_key?: string
  /**
   * How many parents lie between this run and the root. Zero on a root.
   */
  depth: number
  /**
   * When it started.
   */
  created_at: string
  /**
   * When it last moved.
   */
  updated_at: string
  /**
   * When it reached a terminal status.
   */
  ended_at?: string
}

/**
 * One attempt at one step of a run. A run's journal is these rows, append only: a
 * retry is a new row, never a rewrite of the old one.
 * Stored in the `step_attempts` table.
 */
export interface StepAttempt {
  /**
   * Which run.
   */
  run_id: string
  /**
   * Which step, with any enclosing branch, item or iteration folded into the key:
   * `refund/issue`, `classify[3]`, `poll[7]/classify`.
   */
  step_key: string
  /**
   * Which attempt this is. The fence: a lease for an earlier attempt is refused once a
   * later one exists.
   */
  attempt: number
  /**
   * What kind of step.
   */
  kind: StepKind
  /**
   * What a duplicated body would cost.
   */
  effect: Effect
  /**
   * Where the attempt is.
   */
  status: AttemptStatus
  /**
   * The rendered input handed to the worker, on a claimable step.
   */
  input?: unknown
  /**
   * What the worker returned on `complete`, or the event payload on a resumed wait.
   */
  output?: unknown
  /**
   * Why it failed, when it did.
   */
  error?: StepError
  /**
   * The token usage the worker reported on `complete`.
   */
  usage?: StepUsage
  /**
   * What the daemon priced that usage at.
   */
  cost_usd?: number
  /**
   * The span the daemon opened for this attempt. 16 lowercase hex characters, minted
   * when the row was written. A worker's own spans nest under it.
   */
  span_id: string
  /**
   * The `worker_id` holding the lease, while one does.
   */
  lease_owner?: string
  /**
   * When the lease lapses without a heartbeat.
   */
  lease_until?: string
  /**
   * When the worker posted `started`. Absent when it never did, which is what the
   * reclaim rule reads.
   */
  started_at?: string
  /**
   * When the lease lapsed.
   */
  orphaned_at?: string
  /**
   * When the reclaim window closes.
   */
  reclaim_until?: string
  /**
   * How many attempts the node's retry budget still allows after this one.
   */
  attempts_left?: number
  /**
   * When it reached a terminal status.
   */
  ended_at?: string
  /**
   * When the row was written.
   */
  created_at: string
}

/**
 * One row of a run's event log. Written in the transaction that made the change it
 * describes, with `seq` allocated from the run header, so a reader that has seen a row
 * has seen every row below it.
 * Stored in the `run_events` table.
 */
export interface RunEvent {
  /**
   * Which run.
   */
  run_id: string
  /**
   * Its position in this run's stream, from zero and without gaps. Pass it as `after` to
   * reattach.
   */
  seq: number
  /**
   * What happened.
   */
  kind: RunEventKind
  /**
   * The detail, shaped by `kind`.
   */
  payload?: unknown
  /**
   * When it was written.
   */
  at: string
}

/**
 * An axis that traces are described along. An aspect is a definition and not a
 * measurement: it carries the name a note is filed under, the words an operator reads
 * on screen and, where a model writes the sentence, the instruction handed to whatever
 * produces it. Four ship with the daemon as ordinary rows rather than as a list
 * compiled into a client, so there is one source of truth for what exists: goal, what
 * the user wanted; sentiment, the tone they expressed; behaviour, what the agent did
 * about it; and outcome, how the interaction ended. A stored version never changes, so
 * a note recorded a month ago still names the definition it was written against.
 * Stored in the `aspects` table.
 */
export interface Aspect {
  /**
   * Which project owns it. Aspects are project-scoped, like every other tenancy boundary
   * here.
   */
  project: string
  /**
   * The aspect's name, lowercase with underscores, such as `goal` or `refund_reason`.
   * This is what a note is filed under and what every query parameter names.
   */
  name: string
  /**
   * The display name, such as `Goal`. Stored rather than derived from the name, so an
   * aspect reads the same in every screen and every language of SDK.
   */
  label: string
  /**
   * One line saying what this axis is about, shown beside the label so a reader does not
   * have to infer the axis from its name.
   */
  description: string
  /**
   * The text handed to whatever writes a note along this aspect, at most 2000 bytes. The
   * daemon stores it and hands it out; what reads it and writes the sentence sits
   * outside the daemon. Absent when `derived` is true, because a derived aspect needs no
   * instruction.
   */
  instruction?: string
  /**
   * Whether the daemon writes this aspect's notes itself, with no model anywhere.
   * `outcome` ships derived, written from the run's status, the step error class, the
   * silent producer flag on the cause report and the divergence watch, so a deployment
   * with nothing configured still fills one aspect and can see the whole pipeline work
   * before a model is wired to anything.
   */
  derived: boolean
  /**
   * Whether the daemon seeded this aspect. A built-in is a row like any other and can be
   * switched off, but it cannot be deleted, because notes already name it.
   */
  built_in: boolean
  /**
   * Whether notes are written along this aspect in this project. It is configuration and
   * read-only here. Switching an aspect on affects notes recorded afterwards and never
   * rewrites what is stored.
   */
  enabled: boolean
  /**
   * Where this aspect sits in every list and in the default column order, ascending.
   * Stored, so the order an operator learns is the order they keep.
   */
  sort_order: number
  /**
   * This definition's version. Editing an aspect stores a new version rather than
   * changing this one, so analysis already recorded is never rewritten under a reader.
   */
  version: number
  /**
   * When this version was stored.
   */
  created_at: string
}

/**
 * One sentence recorded against one trace along one aspect. A note proves one thing:
 * that this sentence was recorded against that trace by the definition named in
 * `aspect`, at the version in `definition_version`. It does not prove the sentence is
 * true. Nothing here is checked against the trace it describes, so a note is evidence
 * of what a definition said and not evidence of what happened, and the trace is what
 * settles the difference. The daemon also keeps a small sketch of the wording, which
 * is what clustering compares, and that sketch is deliberately not a field on this
 * entity: it is a detail of how the pass is stored, a caller can do nothing with it,
 * and publishing it would fix the hashing where it stands today. The text is payload.
 * It passes through the same redactor as every other signal on the way in, retention
 * sweeps it, and erasing a subject erases it.
 * Stored in the `aspect_notes` table.
 */
export interface AspectNote {
  /**
   * Unique id of the note. This identifies the sentence and never the run: a note id and
   * a trace id are different things and conflating them is the mistake this separate
   * name exists to prevent.
   */
  note_id: string
  /**
   * The trace this sentence is about.
   */
  trace_id: string
  /**
   * Which aspect this note is filed under, matching `Aspect.name`.
   */
  aspect: string
  /**
   * The sentence, after redaction. It is the only text the read API returns for a note,
   * and it is what the clustering pass reads.
   */
  text: string
  /**
   * The version of the aspect definition this note was written against. Editing a
   * definition mints a new version, so an older note keeps naming what was actually
   * asked for at the time.
   */
  definition_version: number
  /**
   * Which model wrote the sentence, when one did. Absent for a derived aspect, where the
   * daemon wrote the note itself.
   */
  model?: string
  /**
   * When the note was recorded. Writing a note lags the trace it describes, sometimes by
   * a long way, so a trace can be visible for hours before it carries one.
   */
  created_at: string
  /**
   * Inherited from the trace, so a note is scoped, filtered, retained and erased exactly
   * as its trace is.
   */
  correlation: Correlation
}

/**
 * One clustering pass over one entity, covering a moving window of notes. A snapshot
 * is what turns a clustering into a series: every theme belongs to exactly one, and
 * every theme query names one. Windows overlap, so counts from two snapshots must
 * never be added together. A trace whose note falls inside three windows is counted
 * once in each of them, and summing the three counts it three times. Compare snapshots
 * or read one; do not sum them. Reads never recompute either: a pass writes a snapshot
 * on its own timer and every read is a lookup of what it wrote, so refreshing a screen
 * shows the same numbers rather than a fresh clustering.
 * Stored in the `theme_snapshots` table.
 */
export interface ThemeSnapshot {
  /**
   * Opaque id, echoed back unchanged. Never build one by hand and never read meaning
   * into its shape.
   */
  snapshot_id: string
  /**
   * Which project this pass covered.
   */
  project: string
  /**
   * The kind of thing this pass covered.
   */
  entity_kind: EntityKind
  /**
   * The name of that thing. An entity is the pair of a kind and a name, which spans
   * already carry, so nothing new is assigned to identify one.
   */
  entity_name: string
  /**
   * The aspects this pass covered, in catalogue order. An aspect with too few notes when
   * the pass ran is absent, so a missing name means it is still collecting and not that
   * anything failed.
   */
  aspects: string[]
  /**
   * Where this snapshot falls in the entity's series, counting from one at the oldest
   * snapshot still stored.
   */
  ordinal: number
  /**
   * When this snapshot became the current state. A timeline is ordered by this and not
   * by the window bounds, because two passes can cover overlapping windows.
   */
  cutoff_at: string
  /**
   * Start of the window of notes read, inclusive.
   */
  window_from: string
  /**
   * End of the window of notes read, exclusive.
   */
  window_to: string
  /**
   * How many distinct traces the notes read belong to.
   */
  trace_count: number
  /**
   * How many notes the pass actually read. Every coverage figure in this snapshot is a
   * share of this number and never of the number of notes in the window, which is what
   * makes `capped` worth reading.
   */
  note_count: number
  /**
   * Whether the pass reached its ceiling of 20,000 notes and clustered the newest of
   * them rather than all of them. True means the window holds more notes than were read,
   * so the denominator of every share here is `note_count` and not the window.
   */
  capped: boolean
}

/**
 * A cluster of aspect notes that share their wording, in one snapshot along one
 * aspect. A theme groups aspect notes that share their wording, which is why mcpd
 * needs no model and no embedding to build one. It does not read the sentences, so two
 * notes that mean the same thing in different words land in separate themes, and a
 * theme's label is the note most like the others rather than a summary written for it.
 * The clustering is a pure function of the notes it reads, with no floating-point
 * arithmetic in the comparison, so two replicas of one deployment reading the same
 * notes write the same themes and cannot disagree about where a boundary falls.
 * Stored in the `themes` table.
 */
export interface Theme {
  /**
   * Opaque id, stable across snapshots, which is what makes a history possible. A theme
   * keeps its id while its wording holds, so the same id in two snapshots is the same
   * theme. Nothing is encoded in the shape of it.
   */
  theme_id: string
  /**
   * The snapshot this theme belongs to.
   */
  snapshot_id: string
  /**
   * Which aspect it was clustered along. Clustering runs once per aspect, so one trace
   * can sit in a different theme in each of them.
   */
  aspect: string
  /**
   * The kind of entity this theme was built for.
   */
  entity_kind: EntityKind
  /**
   * The name of that entity.
   */
  entity_name: string
  /**
   * The theme's label, which is the text of one of its own notes: the note most like the
   * rest of them. Nothing wrote it for the screen, so it reads as one real sentence
   * about one real trace rather than as a description of the group, and it can change
   * between snapshots as the group does.
   */
  label: string
  /**
   * The note the label is taken from.
   */
  label_note_id: string
  /**
   * The trace that note was written about, so a reader can open the run behind the label
   * instead of taking the label's word for it.
   */
  label_trace_id: string
  /**
   * What happened to this theme between the previous snapshot and this one.
   */
  state: ThemeState
  /**
   * How many notes are in this theme.
   */
  note_count: number
  /**
   * How many distinct traces those notes were written about.
   */
  trace_count: number
  /**
   * This theme's share of the notes the snapshot read for this aspect, between 0 and 1.
   * The denominator is the snapshot's `note_count`, so in a capped snapshot it is a
   * share of what was read.
   */
  coverage: number
}
