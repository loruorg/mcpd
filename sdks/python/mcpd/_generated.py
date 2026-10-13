# GENERATED FILE. DO NOT EDIT.
#
# Produced from spec/mcpd.spec.json by tools/codegen.
# Edit the spec and run `make gen`. `make check` fails if this file drifts.


from __future__ import annotations

import sys

from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from typing import Any

# `slots=True` arrived in 3.10. On 3.9 the same dataclasses are emitted without
# it, which costs a little memory and changes nothing else.
_SLOTS = {"slots": True} if sys.version_info >= (3, 10) else {}

#: The version of `spec/mcpd.spec.json` these types were generated from. A
#: daemon reports the same string on `/healthz`. Compare the two at connect
#: time. A mismatch caught there names the problem; the same mismatch found
#: later shows up as a field missing for no visible reason.
SPEC_VERSION = "1.0.0"

class SpanKind(str, Enum):
    """What an operation *is*. The kind drives how a span is rendered in the waterfall, which attributes are meaningful, and which metrics are auto-extracted from it."""

    AGENT_RUN = "agent_run"
    """One agent invocation. Usually the root of a trace."""
    MODEL_GENERATION = "model_generation"
    """One logical generation: everything from the first request to the final answer, including every intermediate step."""
    MODEL_STEP = "model_step"
    """One step inside a generation. A generation that calls tools twice has three steps."""
    MODEL_INFERENCE = "model_inference"
    """The provider round-trip alone, excluding processors and tool execution. Measure pure model latency here."""
    MODEL_CHUNK = "model_chunk"
    """A single streamed chunk. Recorded as an event, not a duration."""
    TOOL_CALL = "tool_call"
    """A tool your code executes."""
    CLIENT_TOOL_CALL = "client_tool_call"
    """A tool the calling client executes. The server records the marker; the client returns the detail."""
    PROVIDER_TOOL_CALL = "provider_tool_call"
    """A tool the model provider executes server-side, such as hosted web search or code execution."""
    MCP_TOOL_CALL = "mcp_tool_call"
    """A tool reached over the Model Context Protocol."""
    HANDOFF = "handoff"
    """Control passing from one agent to another. The span's name is the agent handed to; its parent is the agent handing off."""
    WORKFLOW_RUN = "workflow_run"
    """One workflow execution. Usually the root of a trace."""
    WORKFLOW_STEP = "workflow_step"
    """One step within a workflow."""
    WORKFLOW_CONDITIONAL = "workflow_conditional"
    """A branch point, with the conditions it evaluated."""
    WORKFLOW_CONDITIONAL_EVAL = "workflow_conditional_eval"
    """One condition evaluated inside a branch point."""
    WORKFLOW_PARALLEL = "workflow_parallel"
    """A fan-out of concurrent branches."""
    WORKFLOW_LOOP = "workflow_loop"
    """A loop construct: foreach, do-while or do-until."""
    WORKFLOW_SLEEP = "workflow_sleep"
    """A deliberate pause."""
    WORKFLOW_WAIT_EVENT = "workflow_wait_event"
    """A wait for an external event, with its timeout."""
    MEMORY_OPERATION = "memory_operation"
    """A read from or write to agent memory."""
    RAG_INGESTION = "rag_ingestion"
    """One ingestion pipeline run: load, chunk, extract, embed, upsert."""
    RAG_EMBEDDING = "rag_embedding"
    """One embedding call, possibly batched."""
    RAG_VECTOR_OPERATION = "rag_vector_operation"
    """Vector store I/O: query, upsert, delete or fetch."""
    RAG_ACTION = "rag_action"
    """A RAG-specific transform: chunk, extract metadata or rerank."""
    GRAPH_ACTION = "graph_action"
    """A graph build, traverse, update or prune."""
    PROCESSOR_RUN = "processor_run"
    """An input or output processor."""
    SCORER_RUN = "scorer_run"
    """One scorer evaluating a target."""
    SCORER_STEP = "scorer_step"
    """One stage of a scorer's pipeline."""
    GUARDRAIL = "guardrail"
    """A safety or policy check, and whether it tripped."""
    RETRIEVAL = "retrieval"
    """A generic retrieval that is not vector-store specific."""
    MAPPING = "mapping"
    """An inline data transform between pipeline stages."""
    SKILL_RESOLUTION = "skill_resolution"
    """A dynamic resolver deciding which skills an agent may use."""
    WORKSPACE_ACTION = "workspace_action"
    """A filesystem, sandbox, search or computer-use action."""
    HTTP_REQUEST = "http_request"
    """An outbound HTTP call made on the agent's behalf."""
    GENERIC = "generic"
    """Anything else. Always valid; never inferred."""

class SpanStatus(str, Enum):
    """The outcome of an operation."""

    UNSET = "unset"
    """Still running, or the producer never said."""
    OK = "ok"
    """Completed successfully."""
    ERROR = "error"
    """Failed. `error` carries the detail."""
    CANCELLED = "cancelled"
    """Abandoned deliberately, by a caller or a timeout."""

class SignalType(str, Enum):
    """The six signals mcpd stores. Every one of them is correlated by trace id, most of them by span id as well, and every one of them passes through the same redaction pipeline."""

    SPAN = "span"
    """A unit of work with a start and, usually, an end."""
    LOG = "log"
    """A structured log record."""
    METRIC = "metric"
    """A numeric observation."""
    SCORE = "score"
    """A machine evaluation of a span or trace."""
    FEEDBACK = "feedback"
    """A human evaluation of a span or trace."""
    ASPECT_NOTE = "aspect_note"
    """One sentence recorded against a trace along one aspect. Stored, redacted and retained like the other five, and erased with the subject it belongs to."""

class LogLevel(str, Enum):
    """Severity, ordered. Filtering is always `>=`."""

    TRACE = "trace"
    """Finer than debug. Off by default."""
    DEBUG = "debug"
    """Detail useful while diagnosing."""
    INFO = "info"
    """Normal operation."""
    WARN = "warn"
    """Recoverable, but worth reading."""
    ERROR = "error"
    """The operation failed."""
    FATAL = "fatal"
    """The process cannot continue."""

    @property
    def severity(self) -> int:
        """Ordered severity. Filtering is always `>=`."""
        return {"trace": 1, "debug": 5, "info": 9, "warn": 13, "error": 17, "fatal": 21}[self.value]

class MetricKind(str, Enum):
    """How a numeric observation aggregates."""

    COUNTER = "counter"
    """Monotonically increasing. Aggregates by sum."""
    GAUGE = "gauge"
    """A point-in-time value. Aggregates by last, min, max or mean."""
    HISTOGRAM = "histogram"
    """A distribution. Aggregates by bucket, and answers quantiles."""

class SamplingKind(str, Enum):
    """How mcpd decides which traces to keep."""

    ALWAYS = "always"
    """Keep everything. The default, and correct until volume says otherwise."""
    NEVER = "never"
    """Keep nothing. Useful to disable a single project without redeploying."""
    RATIO = "ratio"
    """Keep a fixed fraction, decided once at the root so a trace is never half-recorded."""
    DETERMINISTIC = "deterministic"
    """Keep a fixed fraction, decided by hashing the trace id. Every collector in a fleet makes the same decision without coordinating."""
    TAIL = "tail"
    """Buffer the trace, then decide once it finishes. Keeps every error and every slow trace regardless of rate."""

class RedactionMode(str, Enum):
    """What happens to a value a redaction rule matched."""

    DROP = "drop"
    """Remove the key entirely, as if it was never sent."""
    MASK = "mask"
    """Replace the whole value with a fixed token."""
    PARTIAL = "partial"
    """Keep a documented prefix and suffix, mask the middle. Enough to recognise a value without disclosing it."""
    HASH = "hash"
    """Replace with a keyed SHA-256 digest. Equal inputs stay equal, so you can still group and join, but the original cannot be recovered."""

class ScoreSource(str, Enum):
    """Where a score came from. Kept separate from the scorer's identity so the same scorer can run in more than one context."""

    LIVE = "live"
    """Produced online, as the traced run happened."""
    OFFLINE = "offline"
    """Produced later, by scoring stored traces."""
    EXPERIMENT = "experiment"
    """Produced by an experiment run against a dataset."""
    MANUAL = "manual"
    """Entered by a person, through the API or the UI."""

class ScorerKind(str, Enum):
    """How a scorer reaches its answer."""

    CODE = "code"
    """Deterministic code in the daemon. Free, and gives the same answer twice."""
    JUDGE = "judge"
    """A built-in rubric graded by the configured judge model, in steps: preprocess, analyze, score, reason."""
    CRITERION = "criterion"
    """A criterion the operator wrote under `judge.criteria`, graded by the judge model in one call."""
    CUSTOM = "custom"
    """A scorer definition stored in the project through the API: a judge prompt, or a code scorer with its settings. Versioned, and the version is on every score it produces."""

class ScoreDirection(str, Enum):
    """Which way a scorer's value is better. A comparison reads a fall in `toxicity` as an improvement and a fall in `faithfulness` as a regression."""

    HIGHER_IS_BETTER = "higher_is_better"
    """A larger value is a better run."""
    LOWER_IS_BETTER = "lower_is_better"
    """A smaller value is a better run, as for `toxicity`, `bias` and `hallucination`."""

class FeedbackKind(str, Enum):
    """The shape of a human judgement."""

    THUMBS = "thumbs"
    """Binary. `value` is `up` or `down`."""
    RATING = "rating"
    """Ordinal. `value` is a number inside the scale the producer declares."""
    CORRECTION = "correction"
    """The right answer. `value` is text."""
    TAG = "tag"
    """A label chosen from a set. `value` is the label."""
    COMMENT = "comment"
    """Free prose, with no score attached."""

class EntityKind(str, Enum):
    """The named thing a span belongs to. Bounded and low-cardinality, so it is safe as a metric label."""

    AGENT = "agent"
    """An agent."""
    WORKFLOW = "workflow"
    """A workflow."""
    TOOL = "tool"
    """A tool."""
    MODEL = "model"
    """A model."""
    SCORER = "scorer"
    """A scorer."""
    PROCESSOR = "processor"
    """A processor."""
    PIPELINE = "pipeline"
    """A RAG or data pipeline."""
    SERVICE = "service"
    """A plain service with no richer identity."""

class ExperimentStatus(str, Enum):
    """Where an experiment run has got to."""

    PENDING = "pending"
    """Created, not started."""
    RUNNING = "running"
    """In progress."""
    SUCCEEDED = "succeeded"
    """Every item completed."""
    FAILED = "failed"
    """Aborted. `error` says why."""
    CANCELLED = "cancelled"
    """Stopped by a person."""

class AuditResult(str, Enum):
    """Whether authorisation let the request through."""

    ALLOWED = "allowed"
    """The key carried the scope the endpoint needs, so mcpd went on to handle the request."""
    REFUSED = "refused"
    """Authorisation refused the request before it reached storage. Recording the attempt is the point of the row."""

class RunStatus(str, Enum):
    """Where a run is. Only `running` can move; the other four are terminal, and a terminal run holds no deadline, no wait and no live attempt."""

    RUNNING = "running"
    """Making progress, or waiting for something that will move it: a lease, a timer or an event."""
    SUCCEEDED = "succeeded"
    """Reached the end of the graph. `output` holds what the definition's `output` template rendered, when it declares one."""
    FAILED = "failed"
    """Ended on an error nothing handled: a step that failed under `on_error: fail_run`, a spent ceiling, or a cancelled child. `error` says which."""
    TIMED_OUT = "timed_out"
    """Ended because the run's own deadline passed."""
    CANCELLED = "cancelled"
    """Ended because a caller asked, through the cancel endpoint or by cancelling an ancestor run."""

class AttemptStatus(str, Enum):
    """Where one attempt at one step is. Four states are live and four are terminal, and the daemon never shows a live attempt on an ended run."""

    PENDING = "pending"
    """On disk and offerable to a worker, once its retry timer, if any, has fired."""
    LEASED = "leased"
    """Held by a worker that has not yet posted `started`."""
    STARTED = "started"
    """Held by a worker that has posted `started`, so a body is running."""
    ORPHANED = "orphaned"
    """The lease expired with no heartbeat. Not offerable while the reclaim window is open: a heartbeat inside the window restores the attempt, and a window that closes with no contact requeues it or fails the run, depending on `effect` and on whether the body had started."""
    SUCCEEDED = "succeeded"
    """Finished with an output."""
    FAILED = "failed"
    """Finished with an error, or marked failed because the run ended while the attempt was live."""
    TIMED_OUT = "timed_out"
    """The step's own deadline passed before the attempt ended."""
    SKIPPED = "skipped"
    """Never ran, because the step's `when` guard was false."""

class StepKind(str, Enum):
    """What a step attempt is an attempt at. `task` and `agent` are claimable. A `wait` row is written by the event that resumes it or the deadline that ends it, so a wait's output and status read like any other step's."""

    TASK = "task"
    """Claimable work. The worker runs whatever code it registered under this step id."""
    AGENT = "agent"
    """Claimable work whose body is the worker's own agent loop. Identical on the wire to `task`; the kind is recorded so the trace and the interface can tell an agent turn from a plain step."""
    WAIT = "wait"
    """A suspension the daemon resumes on an event or ends on a deadline. Never offered to a worker."""

class ConstructKind(str, Enum):
    """What a run's cursor names: the construct at its current position in the definition's `steps`. Every kind except `end` is a node kind a definition can declare."""

    TASK = "task"
    """Claimable work."""
    AGENT = "agent"
    """Claimable work whose body is the worker's agent loop."""
    SLEEP = "sleep"
    """A relative delay, in milliseconds. No process is alive during it."""
    SLEEP_UNTIL = "sleep_until"
    """An absolute delay, to an RFC 3339 instant rendered from a template."""
    WAIT_EVENT = "wait_event"
    """A suspension addressed by an event name and a key rendered per run."""
    PARALLEL = "parallel"
    """A fixed fan-out over named branches, each branch one claimable step. Done when every branch has ended."""
    FOREACH = "foreach"
    """A fan-out over the items of an array, each item one claimable step, with at most `concurrency` of them offerable at once."""
    LOOP = "loop"
    """A bounded loop whose body is one claimable step per iteration, ended by `until` or by `max_iterations`."""
    SUBWORKFLOW = "subworkflow"
    """A child run of another definition. Its output becomes this step's output, and a cancel walks down into it."""
    END = "end"
    """The end of the graph. Never declared in a definition: the cursor reaches it when the last step has ended."""

class Effect(str, Enum):
    """What running a step body twice would cost, which is what the daemon branches on when a lease is lost. Default `at_least_once`."""

    AT_LEAST_ONCE = "at_least_once"
    """Running the body twice is acceptable. A lost lease requeues the attempt. The default."""
    AT_MOST_ONCE = "at_most_once"
    """Running the body twice is not acceptable. `started` must be posted before the body runs, and a started attempt whose lease is lost fails the run rather than running again."""

class TimerKind(str, Enum):
    """Every deadline the daemon keeps. Each one is a row the tick fires, and each firing is a transaction that moves the run."""

    SLEEP = "sleep"
    """A `sleep` or `sleep_until` coming due."""
    LEASE_EXPIRY = "lease_expiry"
    """A lease that has not been heartbeated. Firing moves the attempt to `orphaned` and opens a reclaim window one lease length long."""
    RECLAIM = "reclaim"
    """The close of the reclaim window an expired lease opened. Firing requeues the attempt or fails the run, depending on `effect` and on whether the body had started."""
    RUN_TIMEOUT = "run_timeout"
    """The whole run's ceiling, from `limits.run_timeout_ms`."""
    STEP_TIMEOUT = "step_timeout"
    """One attempt's ceiling, from the node's `step_timeout_ms`. It bounds a worker that heartbeats for ever."""
    WAIT_TIMEOUT = "wait_timeout"
    """A `wait_event` whose event has not arrived by `timeout_ms`."""
    RETRY = "retry"
    """The backoff before a requeued attempt becomes claimable. Firing writes the next attempt as `pending`."""

class RunEventKind(str, Enum):
    """What a row in a run's event log records. Every row is written in the transaction that made the change it describes, so a reader that has seen sequence number n has seen every change below it."""

    RUN_STARTED = "run_started"
    """The run was created. Payload: `definition_id`, `definition_version`."""
    RUN_CURSOR = "run_cursor"
    """The cursor moved. Payload: `index`, `construct`, and `step_key` when the construct has one."""
    STEP_LEASED = "step_leased"
    """A worker claimed an attempt. Payload: `step_key`, `attempt`, `worker_id`."""
    STEP_STARTED = "step_started"
    """A worker posted `started`. Payload: `step_key`, `attempt`."""
    STEP_FINISHED = "step_finished"
    """An attempt succeeded. Payload: `step_key`, `attempt`."""
    STEP_FAILED = "step_failed"
    """An attempt failed. Payload: `step_key`, `attempt`, `class`, `message`, `attempts_left`."""
    STEP_SKIPPED = "step_skipped"
    """A `when` guard was false. Payload: `step_key`."""
    RUN_WAITING = "run_waiting"
    """The run reached a `wait_event`. Payload: `step_key`, `event`, `key`, `expires_at`."""
    RUN_RESUMED = "run_resumed"
    """An event resumed a waiting run. Payload: `step_key`, `event_id`."""
    RUN_ENDED = "run_ended"
    """The run reached a terminal status. Payload: `status`, and `error` when there is one."""

class DefinitionKind(str, Enum):
    """What kind of thing a definition describes."""

    WORKFLOW = "workflow"
    """A graph of steps the daemon walks. The only kind in version 1: `agent` definitions arrive with the in-process agent loop, which is not served yet."""

class EventOutcome(str, Enum):
    """What the daemon did with a published event."""

    DELIVERED = "delivered"
    """A waiting run took it and moved, in the same transaction."""
    PARKED = "parked"
    """No run was waiting. The event is held for 24 hours for the next run that reaches a matching wait with `accept_parked`."""
    DUPLICATE = "duplicate"
    """This event id has been seen before, so nothing was written. The publish endpoint says when a derived id is treated this way."""

class ConflictReason(str, Enum):
    """Why a worker call about a lease answered 409. Each reason tells the worker something different to do next."""

    FENCED = "fenced"
    """A later attempt exists, or another lease terminated this attempt. Abandon the work and post nothing more for this lease."""
    ALREADY_APPLIED = "already_applied"
    """This lease's own earlier `complete` or `fail` landed and its reply was lost. The step is done; `attempt_status` says how. Post nothing more."""
    NOT_STARTED = "not_started"
    """`complete` or `fail` arrived for a `leased` attempt under `at_most_once`. A bug in the worker: post `started` first, and log it loudly."""
    RUN_TERMINAL = "run_terminal"
    """The run has ended. Stop, and stop claiming for this run."""

class StepErrorClass(str, Enum):
    """How a step failed, which decides whether the daemon retries it."""

    RETRYABLE = "retryable"
    """Worth another attempt. The daemon retries while attempts remain, at the node's backoff, and honours `retry_after_ms` when it is larger."""
    FATAL = "fatal"
    """Another attempt would fail the same way. No retry; the node's `on_error` applies."""
    INVALID_INPUT = "invalid_input"
    """The step's input cannot be acted on. No retry; the node's `on_error` applies."""
    UNSERVABLE = "unservable"
    """This worker does not serve the definition version, or does not recognise the step key. The attempt returns to `pending` after one lease length without spending the retry budget, so another worker can take it."""
    CANCELLED = "cancelled"
    """The worker gave the attempt up on request. No retry; the node's `on_error` applies."""

class OnError(str, Enum):
    """What happens to a run when a node fails after its retries. Default `fail_run`."""

    FAIL_RUN = "fail_run"
    """End the run `failed`, with the step's error message as the run's error."""
    CONTINUE = "continue"
    """Mark the step `failed` and move on, so a later `when` guard can read `steps.<id>.status` and `steps.<id>.error`."""

class Backoff(str, Enum):
    """How the delay between retries grows."""

    FIXED = "fixed"
    """Every retry waits `initial_ms`."""
    EXPONENTIAL = "exponential"
    """Each retry waits twice as long as the last, starting at `initial_ms` and capped at `max_ms`."""

class WaitTimeout(str, Enum):
    """What a `wait_event` node does when its deadline passes with no event. Default `fail`."""

    FAIL = "fail"
    """The node fails. Its `on_error` decides what the run does next."""
    CONTINUE = "continue"
    """The wait ends and the run moves on to the next node."""

class ValueKind(str, Enum):
    """The JSON type of a value. Used where mcpd reports the shape of something it holds without returning the thing itself."""

    NULL = "null"
    """The value was JSON null."""
    BOOLEAN = "boolean"
    """The value was `true` or `false`."""
    NUMBER = "number"
    """The value was a JSON number."""
    STRING = "string"
    """The value was a JSON string."""
    ARRAY = "array"
    """The value was a JSON array."""
    OBJECT = "object"
    """The value was a JSON object."""

class MonitorWatch(str, Enum):
    """What a monitor measures over its window."""

    ERROR_RATE = "error_rate"
    """The fraction of runs that failed, 0 to 1."""
    LATENCY_P95 = "latency_p95"
    """Run duration at the ninety-fifth percentile, in milliseconds."""
    COST = "cost"
    """Total spend across the window."""
    TRACE_COUNT = "trace_count"
    """How many runs happened. Catches a pipeline that has gone silent."""
    SCORE_MEAN = "score_mean"
    """The mean of the scorer named by `scorer_id`."""
    STEP_COUNT_DIVERGENCE = "step_count_divergence"
    """How far a run strays from the usual shape of its own definition. This watch sets no threshold in steps, because a step count is never high in absolute terms, only high for that definition. It reads the recent completed runs of one definition version, takes the median step count as the usual shape, and reports a run whose modified z-score against the median absolute deviation exceeds the monitor's bound, 3.5 by default. The median absolute deviation is used rather than a mean and a standard deviation because it tolerates up to half its sample being outliers, which is the case here: a runaway cohort must not be allowed to redefine normal. Fewer than twelve completed runs and the watch says nothing rather than guessing. A run that takes several times its usual number of steps trips this long before it reaches `max_steps`, and while its error rate is still zero."""
    RUN_REPETITION = "run_repetition"
    """How much a run repeats itself. For each run in the window, the highest number of attempts it made at any one node of its definition, counting a retry and a loop iteration of that node alike; the watch reads the mean of that number. One means no node ran twice. A loop that re-enters the same node, and a retry storm, both raise this before either reaches `error_rate`."""

class MonitorCompare(str, Enum):
    """Which way a monitor's threshold is breached."""

    ABOVE = "above"
    """Trip when the value rises above the threshold."""
    BELOW = "below"
    """Trip when the value falls below it. This is the direction that catches a quality regression, and a silent pipeline."""

class MonitorCondition(str, Enum):
    """Where a monitor or an objective stands. A tripped condition latches: it stays tripped until the number recovers, and too little data never clears it, because silence is not recovery."""

    NOMINAL = "nominal"
    """Evaluated, and within its threshold."""
    TRIPPED = "tripped"
    """Tripped, and not yet acknowledged."""
    ACKNOWLEDGED = "acknowledged"
    """Tripped, acknowledged, and still breaching. Returns to `nominal` on its own when the number recovers."""
    INSUFFICIENT = "insufficient"
    """Not enough data in the window to judge. Also the state before the first evaluation."""

class TextSearch(str, Enum):
    """What the `q` parameter matches on this deployment's storage backend. Reported by `GET /v1/api/capabilities` so a search box can say what it searches instead of leaving a reader to infer it from a result set."""

    NONE = "none"
    """`q` is not applied at all. No backend that ships declares this; a backend that did would be telling an interface to hide its search box rather than let somebody type into one that returns everything."""
    SUBSTRING = "substring"
    """`q` matches anywhere inside a word, so `heck` finds `checkout`."""
    WORDS = "words"
    """`q` matches whole words only, which is what a person typing two words into a search box expects. There is no stemming: `refund` does not find `refunds`."""

class Partitioning(str, Enum):
    """How the storage backend cuts each signal table on its anchor time. It decides whether expiring a day of data is a metadata operation on one part or a row-by-row delete."""

    NONE = "none"
    """One table, no partitions. Retention removes rows."""
    DAY = "day"
    """One partition per day."""
    MONTH = "month"
    """One partition per month."""

class Facet(str, Enum):
    """A dimension a filter box can offer values for. The set is closed on purpose: each one is a column or a parsed field the daemon keeps a distinct-value list for as signals arrive, so answering is a read of a small table rather than a scan of every span in the retention window."""

    ENTITY_KIND = "entity_kind"
    """The kind of thing a step was: an agent run, a tool call, a retrieval, and so on."""
    ENTITY_NAME = "entity_name"
    """The name of that thing, such as the tool that was called."""
    SERVICE_NAME = "service_name"
    """The service that reported the signal."""
    ENVIRONMENT = "environment"
    """The environment it came from, such as `production`."""
    TAG = "tag"
    """A tag carried on a run."""
    METRIC_NAME = "metric_name"
    """A metric name, for the name box of a chart."""
    METRIC_LABEL_KEY = "metric_label_key"
    """A label key a metric point carries. Project-wide: these are every key seen in the project, not the keys of one named metric."""
    METRIC_LABEL_VALUE = "metric_label_value"
    """A value one label key takes. Needs `key`, because the answer would otherwise be every value of every label mixed together."""

class DeltaSignal(str, Enum):
    """A signal that can be polled for what has arrived. Only the three that carry a server-set arrival time: a score and a piece of feedback are stamped with the caller's own timestamp, so a watermark over them would either skip a late row or repeat every row for ever."""

    TRACES = "traces"
    """The run projection, which is what a run list refreshes from."""
    SPANS = "spans"
    """Steps, for a waterfall that is still filling in."""
    LOGS = "logs"
    """Log lines."""

class AspectReadiness(str, Enum):
    """How far the daemon has got with one aspect for one entity. It is computed from the rows that are stored and never recorded, so it cannot go stale, and it is per entity and per aspect rather than global: one agent can have goal themes to read while its behaviour notes are still collecting."""

    COLLECTING = "collecting"
    """Fewer notes than a clustering pass needs. The floor is 50 notes for one aspect on one entity, and below it the daemon writes no snapshot at all rather than sorting a handful of sentences into themes nobody should draw a conclusion from."""
    PROCESSING = "processing"
    """Enough notes to cluster, and no snapshot covers this aspect yet. The next pass will write one. Nothing is wrong and nothing needs doing."""
    READY = "ready"
    """A snapshot covers this aspect, so themes exist and every theme route answers."""

class ThemeState(str, Enum):
    """What happened to a theme between the previous snapshot and this one. The daemon matches themes across consecutive snapshots by their wording, so a theme keeps its id while its wording holds, and this records what that match found. It is computed from the two clusterings alone and never from a trace attribute."""

    BIRTH = "birth"
    """The first snapshot this theme appears in."""
    CONTINUE = "continue"
    """Present in the previous snapshot and in this one, matched to the same id. The ordinary state of a theme that is holding together."""
    SPLIT = "split"
    """The previous snapshot held one theme where this one holds several. The history point's `split_from` names the theme it came out of."""
    MERGE = "merge"
    """Several themes in the previous snapshot are one theme here. The history point's `merged_from` names them."""
    RESURRECTION = "resurrection"
    """Present in an earlier snapshot, absent from the one immediately before this, and back now under its original id. A theme that comes and goes reads as one theme with gaps rather than as several short-lived ones."""
    DEATH = "death"
    """The theme was in the previous snapshot and is in none of this one's clusters. Reachable only through a theme's history: a dead theme has no row in the snapshot it died in, so it is never a node in a flow."""

class ThemeNodeKind(str, Enum):
    """What one node of a theme flow stands for. A stage holds its themes, at most one noise node and at most one overflow node, and the three together account for every trace in the stage, which is what lets the shares add up to one."""

    THEME = "theme"
    """One theme. It carries a `theme_id`, so it can be filtered on, opened and followed through its history."""
    NOISE = "noise"
    """The notes in this stage that joined no theme. Noise is not a failure bucket: it holds rare requests, wording nothing else matched and patterns that have not recurred yet, and a large noise share means varied traffic or too few notes rather than something going wrong. Its notes are read from the examples route with `noise` as the theme id."""
    OTHER = "other"
    """The themes past `theme_limit_per_stage`, folded into one node so the stage totals still add up. It carries no `theme_id` and cannot be selected, filtered on or opened, because it is a presentation fold and not a group of anything."""

class SnapshotLandmarkReason(str, Enum):
    """Why one snapshot is in a landmark selection. A timeline draws a couple of dozen points over a range that may hold thousands of passes, so the daemon chooses them and says why, rather than handing back the newest few and leaving the reader to assume that is the whole story."""

    RANGE_START = "range_start"
    """The oldest snapshot in the requested range. Always included, so a timeline starts where the range does."""
    RANGE_END = "range_end"
    """The newest snapshot in the requested range. Always included, so a timeline ends at the current state."""
    TIME_SAMPLE = "time_sample"
    """Chosen to spread the remaining points evenly over the range. Sampling is by time and not by ordinal, so an hour of frequent passes does not crowd out a quiet week."""

class KeyScope(str, Enum):
    """What an account API key may do. The same six scopes the daemon's configured keys carry, under the same names as the `auth` value of each endpoint."""

    INGEST = "ingest"
    """Write signals, and read nothing. The key that lives in every process that emits a span."""
    READ = "read"
    """Read stored signals: runs, spans, logs, metrics, scores, analytics and datasets."""
    WRITE = "write"
    """Change stored data: scores, feedback, datasets, experiments and acknowledgements."""
    ADMIN = "admin"
    """Delete data and read the audit log. Also accepted where `execute` or `worker` is needed."""
    EXECUTE = "execute"
    """Start, cancel and signal runs, and publish events to them."""
    WORKER = "worker"
    """Claim steps and report their outcome. The key that lives in the process running step bodies."""

class OrgRole(str, Enum):
    """What a member may do in an organisation. Each role maps onto the six key scopes on every project the organisation owns, unless a project role says otherwise."""

    OWNER = "owner"
    """Every scope on every project, and everything an admin may do. Only an owner may add or remove an owner, change quotas or delete the organisation."""
    ADMIN = "admin"
    """Every scope on every project. Invites and removes members below owner, sets roles, creates and deletes projects, and manages SSO, SCIM and every key in the organisation."""
    MEMBER = "member"
    """The `ingest`, `read`, `write`, `execute` and `worker` scopes. Everything but `admin`: a member cannot delete data or read the audit log."""
    VIEWER = "viewer"
    """The `read` scope only."""

class ProjectRole(str, Enum):
    """A role on one project that replaces a member's organisation role there. Owners and admins keep their reach on every project whatever a project role says."""

    NONE = "none"
    """No access to this project."""
    VIEWER = "viewer"
    """The `read` scope on this project."""
    MEMBER = "member"
    """Every scope but `admin` on this project."""
    ADMIN = "admin"
    """Every scope on this project. It does not let the member manage the organisation."""

class SsoProvider(str, Enum):
    """How an organisation's members log in with single sign-on."""

    OIDC = "oidc"
    """Any OpenID Connect provider, such as Google, Microsoft Entra ID, Okta or Auth0, found through its issuer's discovery document. Authorization code flow with PKCE, state and nonce."""
    GITHUB = "github"
    """GitHub, through an OAuth app. GitHub issues no ID token, so the member is the GitHub user id and the email is the primary verified address GitHub reports."""

class PromptKind(str, Enum):
    """The shape of a prompt's body."""

    TEXT = "text"
    """One template string, for a completion-style call."""
    CHAT = "chat"
    """An ordered list of messages, each with a `role` and a `content` template, for a chat-style call."""

class ScorerDefinitionKind(str, Enum):
    """How a stored scorer definition reaches its answer."""

    JUDGE = "judge"
    """The configured judge model grades the run against the definition's `prompt`, in one call."""
    CODE = "code"
    """One of the daemon's code scorers, named in `builtin`, with the settings in `config` fixed. Free, and gives the same answer twice."""

class ReviewStatus(str, Enum):
    """Where an item in the review queue has got to."""

    PENDING = "pending"
    """Waiting for a person."""
    REVIEWED = "reviewed"
    """A person looked at it and recorded what they found."""
    DISMISSED = "dismissed"
    """A person looked at it and it needed nothing."""

@dataclass(**_SLOTS)
class InputTokenDetails:
    """Input tokens broken down by how the provider billed them. Every field is optional; absent means the provider did not report it, which is different from zero.
    """

    text: int | None = None
    """Ordinary text tokens: not cached, not audio, not image."""
    cache_read: int | None = None
    """Tokens served from a prompt cache. Usually billed at a large discount."""
    cache_write: int | None = None
    """Tokens written into a prompt cache. Usually billed at a premium."""
    cache_write_5m: int | None = None
    """Tokens written to a five-minute ephemeral cache, where the provider prices TTLs separately."""
    cache_write_1h: int | None = None
    """Tokens written to a one-hour ephemeral cache."""
    audio: int | None = None
    """Audio input tokens."""
    image: int | None = None
    """Image input tokens, including rendered PDF pages."""

@dataclass(**_SLOTS)
class OutputTokenDetails:
    """Output tokens broken down by kind.
    """

    text: int | None = None
    """Ordinary completion tokens."""
    reasoning: int | None = None
    """Hidden reasoning tokens. Billed, and usually invisible in the response."""
    audio: int | None = None
    """Audio output tokens."""
    image: int | None = None
    """Image output tokens."""

@dataclass(**_SLOTS)
class TokenUsage:
    """Token counts for one model call. Totals are authoritative; the detail breakdowns explain them and are what cost is actually computed from.
    """

    input_tokens: int | None = None
    """Total input tokens."""
    output_tokens: int | None = None
    """Total output tokens."""
    total_tokens: int | None = None
    """Input plus output, when the provider reports a total that differs from the sum."""
    input_details: InputTokenDetails | None = None
    """How the input tokens were billed."""
    output_details: OutputTokenDetails | None = None
    """How the output tokens were billed."""

@dataclass(**_SLOTS)
class Cost:
    """What a span cost, in a single currency. mcpd computes this from a price book at ingest and stores the result, so a later price change never silently rewrites history.
    """

    currency: str = "USD"
    """ISO 4217 code. `USD` unless the price book says otherwise."""
    is_estimated: bool = True
    """True when mcpd derived the cost from a price book, false when the producer reported it directly."""
    input_cost: float | None = None
    """Cost attributable to input tokens."""
    output_cost: float | None = None
    """Cost attributable to output tokens."""
    total_cost: float | None = None
    """The sum, including any meter that is neither input nor output."""
    price_book: str | None = None
    """Identifier of the price book used, so a figure can be traced back to the rates that produced it."""

@dataclass(**_SLOTS)
class ModelParameters:
    """The sampling settings a caller chose for one model call. Recorded because a run nobody can reproduce is a run nobody can debug, and the seed and the sampling settings are what a replay has to match. Every field is what the caller asked for, never what the provider did with it. None of them carries prompt or completion text, so a producer running with payload capture off still records all of them.
    """

    has_response_format: bool = False
    """True when the caller constrained the output to a fixed shape, such as a JSON schema or a grammar. The shape itself is not recorded: it can be large and it is caller-owned content, and what a reader needs is whether the constraint was on."""
    temperature: float | None = None
    """The sampling temperature the caller asked for. Absent when the caller left the provider's default in place, which is not the same as sending zero."""
    top_p: float | None = None
    """Nucleus sampling: the caller asked the sampler to consider the smallest set of tokens whose probabilities reach this fraction. Absent when the caller set none."""
    top_k: int | None = None
    """The caller asked the sampler to consider only this many of the most likely tokens. Not every provider offers it."""
    max_tokens: int | None = None
    """The ceiling the caller put on output length. An answer that stops in the middle of a sentence usually stopped here, and this is the first field to read when one does."""
    seed: int | None = None
    """The sampling seed the caller passed. Nothing in mcpd reads it. It is recorded because without it a run cannot be reproduced at all, and a comparison between two runs that differ only in their seed is worth nothing."""
    stop_sequences: list[str] | None = None
    """The stop sequences the caller supplied. They are short strings the caller wrote, so they are settings rather than payload, and an answer that ends early often ends on one of them. Do not put prompt text here."""

@dataclass(**_SLOTS)
class SpanError:
    """Why a span failed.
    """

    message: str
    """A human-readable message. Required: an error with no message is not actionable."""
    kind: str | None = None
    """The error class or code, such as `RateLimitError` or `ECONNRESET`."""
    stack: str | None = None
    """A stack trace, when the producer captured one."""
    domain: str | None = None
    """Which subsystem raised it."""
    retryable: bool | None = None
    """Whether the producer considered the failure worth retrying."""
    details: Any | None = None
    """Structured detail. Passes through redaction like any other payload."""

@dataclass(**_SLOTS)
class Correlation:
    """The descriptive context a signal belongs to. Every field here is queryable and most are safe as metric labels; the high-cardinality ones (`user_id`, `session_id`, `request_id`) are queryable but blocked from metric labels by the cardinality filter.
    """

    project: str
    """Which project this belongs to. The primary tenancy key."""
    environment: str | None = None
    """Deployment environment, such as `production` or `staging`."""
    service_name: str | None = None
    """The emitting service."""
    service_version: str | None = None
    """The emitting service's version, so a regression can be pinned to a release."""
    session_id: str | None = None
    """A user-facing session. Groups many traces into one conversation."""
    thread_id: str | None = None
    """A conversation thread within a session."""
    user_id: str | None = None
    """The end user. Hashed with a keyed digest at ingest, so runs can still be grouped by customer without storing who the customer is. Filtering on it hashes your query value the same way, so lookup keeps working. Rotating the key deliberately breaks the link."""
    resource_id: str | None = None
    """A tenant or workspace inside the calling application."""
    run_id: str | None = None
    """The producer's own identifier for this run."""
    request_id: str | None = None
    """The inbound request that caused this work."""
    experiment_id: str | None = None
    """Set when the run was produced by an experiment."""
    release: str | None = None
    """A release or git sha, for regression attribution."""

@dataclass(**_SLOTS)
class SpanLink:
    """A reference from this span to another, in this trace or a different one. Links express causality that parentage cannot: a batch job caused by twenty requests has twenty links and one parent.
    """

    trace_id: str
    """The linked trace."""
    span_id: str | None = None
    """The linked span, when the link points at one rather than a whole trace."""
    kind: str | None = None
    """Why the link exists, such as `follows_from`, `retry_of` or `replay_of`."""
    attributes: Any | None = None
    """Anything else about the relationship."""

@dataclass(**_SLOTS)
class Page:
    """Cursor pagination. Cursors are opaque and stable; never construct one by hand.
    """

    has_more: bool
    """Whether another page exists."""
    next_cursor: str | None = None
    """Pass as `cursor` to fetch the next page. Absent on the last page."""
    total: int | None = None
    """Total matching rows, when the backend could count them cheaply."""

@dataclass(**_SLOTS)
class IngestBatch:
    """One batch of signals sent to the native ingest endpoint. Any combination of the six signals may appear in a single batch; mcpd sorts them out. Supplying `ingest_key` makes the whole batch idempotent, so a client that retries after a timeout never double-counts.
    """

    spans: list[Span] | None = None
    """Spans to record."""
    logs: list[LogRecord] | None = None
    """Log records to record."""
    metrics: list[MetricPoint] | None = None
    """Metric observations to record."""
    scores: list[Score] | None = None
    """Machine evaluations to attach."""
    feedback: list[Feedback] | None = None
    """Human evaluations to attach."""
    aspect_notes: list[AspectNote] | None = None
    """Aspect notes to record. A note is a sentence about a trace, so it arrives with that trace's other signals, under the same idempotency key and through the same redactor."""
    ingest_key: str | None = None
    """Idempotency key for this batch. Replaying a batch with a key mcpd has already accepted is a no-op that still returns 200."""

@dataclass(**_SLOTS)
class IngestError:
    """Why one signal in a batch was refused.
    """

    signal: SignalType
    """Which signal kind the failure was in."""
    index: int
    """Position within that signal's array, so a client can find the offending record."""
    message: str
    """What was wrong, in terms the sender can act on."""

@dataclass(**_SLOTS)
class IngestResult:
    """What mcpd did with a batch. Counts are per signal, and `rejected` is never silently zero: a signal mcpd could not store is always reported with a reason.
    """

    accepted: int
    """How many signals were stored."""
    rejected: int
    """How many were refused."""
    duplicate: int
    """How many were recognised as a replay of an already-accepted batch and skipped."""
    errors: list[IngestError] | None = None
    """One entry per rejected signal, naming what was wrong and where."""

@dataclass(**_SLOTS)
class TraceListResponse:
    """A page of traces.
    """

    page: Page
    """Cursor and totals."""
    traces: list[Trace] = field(default_factory=list)
    """The matching traces, in the requested order."""

@dataclass(**_SLOTS)
class TraceDetail:
    """One trace and everything attached to it.
    """

    trace: Trace
    """The trace projection."""
    spans: list[Span] = field(default_factory=list)
    """Every span, ordered by start time then by depth, so a waterfall renders without sorting."""
    logs: list[LogRecord] | None = None
    """Logs, when `include` asked for them."""
    scores: list[Score] | None = None
    """Scores, when `include` asked for them."""
    feedback: list[Feedback] | None = None
    """Feedback, when `include` asked for them."""

@dataclass(**_SLOTS)
class SpanListResponse:
    """A page of spans.
    """

    page: Page
    """Cursor and totals."""
    spans: list[Span] = field(default_factory=list)
    """The matching spans."""

@dataclass(**_SLOTS)
class LogListResponse:
    """A page of log records.
    """

    page: Page
    """Cursor and totals."""
    logs: list[LogRecord] = field(default_factory=list)
    """The matching records."""

@dataclass(**_SLOTS)
class MetricSeriesPoint:
    """One bucket of an aggregated series.
    """

    timestamp: datetime
    """Start of the bucket."""
    value: float
    """The aggregated value."""
    count: int | None = None
    """How many raw observations fell in this bucket. Distinguishes a genuine zero from no data."""
    exemplar_trace_id: str | None = None
    """A trace that contributed to this bucket, so a spike on the chart opens the run that caused it."""

@dataclass(**_SLOTS)
class MetricHistogram:
    """A histogram metric read back as distributions: the buckets of every point in the window added up, one distribution per label combination, with quantiles read from the merged counts.

Bucketed points from OpenTelemetry are added bucket by bucket; for a cumulative stream only the growth across the window counts. Points with one value each, such as mcpd's own derived durations, give exact quantiles and are placed into OpenTelemetry's default boundaries.
    """

    name: str
    """The metric read."""
    points_read: int
    """How many stored points were merged."""
    truncated: bool
    """Whether the window held more points than one read merges, so the newest ones were used and the rest left out. Narrow the window or add a filter."""
    series: list[HistogramSeries] = field(default_factory=list)
    """One distribution per label combination."""
    unit: str | None = None
    """UCUM unit of the observations, for axis labelling."""

@dataclass(**_SLOTS)
class HistogramSeries:
    """One merged distribution.
    """

    count: int
    """Every observation counted."""
    exact: bool
    """Whether the quantiles are exact, read from single observations, or interpolated linearly inside the bucket the rank falls in."""
    points_skipped: int
    """Points left out because their bucket boundaries differ from the ones most of this series uses."""
    bounds: list[float] = field(default_factory=list)
    """Upper bound of each bucket but the last, which is unbounded above."""
    counts: list[int] = field(default_factory=list)
    """Observations per bucket: one more entry than `bounds`."""
    labels: Any | None = None
    """The `group_by` label values that identify this series. Absent when nothing was grouped."""
    sum: float | None = None
    """Their sum."""
    min: float | None = None
    """The smallest observation, when the points recorded single values."""
    max: float | None = None
    """The largest observation, when the points recorded single values."""
    p50: float | None = None
    """Median."""
    p90: float | None = None
    """90th percentile."""
    p95: float | None = None
    """95th percentile."""
    p99: float | None = None
    """99th percentile."""

@dataclass(**_SLOTS)
class MetricPointListResponse:
    """A page of stored metric points, as they were recorded.
    """

    page: Page
    """Cursor and totals."""
    points: list[MetricPoint] = field(default_factory=list)
    """The matching points."""

@dataclass(**_SLOTS)
class MetricSeries:
    """One line on a chart: a single label combination over time.
    """

    points: list[MetricSeriesPoint] = field(default_factory=list)
    """Bucketed values, oldest first."""
    labels: Any | None = None
    """The label values that identify this series."""

@dataclass(**_SLOTS)
class MetricSeriesResponse:
    """An aggregated metric query result.
    """

    name: str
    """The metric queried."""
    bucket: str
    """The bucket width actually used, which may differ from the request when mcpd chose it."""
    series: list[MetricSeries] = field(default_factory=list)
    """One entry per label combination."""
    unit: str | None = None
    """UCUM unit, for axis labelling."""

@dataclass(**_SLOTS)
class ScoreListResponse:
    """A page of scores.
    """

    page: Page
    """Cursor and totals."""
    scores: list[Score] = field(default_factory=list)
    """The matching scores."""

@dataclass(**_SLOTS)
class FeedbackListResponse:
    """A page of feedback.
    """

    page: Page
    """Cursor and totals."""
    feedback: list[Feedback] = field(default_factory=list)
    """The matching feedback."""

@dataclass(**_SLOTS)
class LatencyStats:
    """Latency distribution over a window, in milliseconds.
    """

    p50: float | None = None
    """Median."""
    p90: float | None = None
    """Ninetieth percentile."""
    p95: float | None = None
    """Ninety-fifth percentile."""
    p99: float | None = None
    """Ninety-ninth percentile."""
    mean: float | None = None
    """Arithmetic mean. Reported alongside quantiles, never instead of them."""
    max: float | None = None
    """Slowest observation in the window."""

@dataclass(**_SLOTS)
class GroupedStat:
    """One row of a grouped breakdown.
    """

    key: str
    """The group's value, such as a model id."""
    count: int
    """How many runs fell in this group."""
    error_count: int
    """How many of them failed."""
    total_cost: float | None = None
    """Spend attributable to this group."""
    input_tokens: int | None = None
    """Input tokens for this group."""
    output_tokens: int | None = None
    """Output tokens for this group."""
    latency: LatencyStats | None = None
    """Latency distribution within this group."""

@dataclass(**_SLOTS)
class AnalyticsSummary:
    """Headline numbers for a window. One request backs the overview screen.
    """

    from_: datetime
    """Start of the window."""
    to: datetime
    """End of the window."""
    trace_count: int
    """Runs started in the window."""
    span_count: int
    """Steps recorded in the window."""
    error_count: int
    """Runs that failed."""
    error_rate: float
    """Failures as a fraction of runs, between 0 and 1."""
    latency: LatencyStats
    """Run duration distribution."""
    input_tokens: int
    """Input tokens consumed."""
    output_tokens: int
    """Output tokens produced."""
    total_cost: float
    """Total spend in the window."""
    currency: str = "USD"
    """Currency of `total_cost`."""
    groups: list[GroupedStat] | None = None
    """Per-group rows, when `group_by` was supplied."""

@dataclass(**_SLOTS)
class CostBucket:
    """Spend in one time bucket for one group.
    """

    timestamp: datetime
    """Start of the bucket."""
    key: str
    """The group this row belongs to."""
    total_cost: float
    """Spend in this bucket."""
    input_tokens: int | None = None
    """Input tokens in this bucket."""
    output_tokens: int | None = None
    """Output tokens in this bucket."""

@dataclass(**_SLOTS)
class CostBreakdown:
    """Where the money went.
    """

    from_: datetime
    """Start of the window."""
    to: datetime
    """End of the window."""
    total_cost: float
    """Total spend across the window."""
    groups: list[GroupedStat] = field(default_factory=list)
    """Totals per group, largest first."""
    currency: str = "USD"
    """Currency of every figure in this response."""
    series: list[CostBucket] | None = None
    """A time series, when `bucket` was supplied."""

@dataclass(**_SLOTS)
class DeleteResult:
    """What a deletion removed.
    """

    deleted: int
    """How many records were removed, across every signal."""

@dataclass(**_SLOTS)
class HealthStatus:
    """Whether the server is alive and able to serve.
    """

    status: str
    """`ok` or `degraded`."""
    version: str
    """The mcpd version serving this request."""
    spec_version: str
    """The data-model spec version this build implements. A client can refuse to talk to an incompatible server."""
    uptime_seconds: int
    """How long this process has been running."""
    storage: str | None = None
    """Which storage backend is configured."""
    detail: str | None = None
    """What is degraded, when something is."""
    signup: bool | None = None
    """True when this daemon offers open sign-up: anyone may create an account at `POST /v1/api/account/signup` and log in at `POST /v1/api/account/login`. Absent when it does not, which is the default. The interface reads this to decide whether to show its sign-up and log-in screens."""
    accounts: bool | None = None
    """True when this daemon has accounts, with or without open sign-up. Absent when it does not. The interface reads this to show its log-in screen."""
    sso: bool | None = None
    """True when at least one organisation on this daemon logs in with single sign-on, so the log-in screen offers it. Absent otherwise."""

@dataclass(**_SLOTS)
class ScorerInfo:
    """One scorer mcpd can run, as described to a caller.
    """

    id: str
    """Stable identifier, recorded on every score it produces."""
    description: str
    """What it measures, in one line."""
    needs_config: bool
    """Whether it needs settings to mean anything. A scorer that needs an expected answer cannot run without one."""
    kind: ScorerKind
    """How the scorer reaches its answer."""
    direction: ScoreDirection
    """Which way its value is better."""
    config_keys: list[str] = field(default_factory=list)
    """The `ScorerConfig` settings it reads. Empty when it needs none."""
    available: bool = True
    """Whether this daemon can run it now. A judged scorer is unavailable until a `judge` block names an endpoint and a model."""

@dataclass(**_SLOTS)
class ScorerConfig:
    """Settings shared by the scorers that need them. Every field is optional; a scorer that needs one and does not get it declines instead of guessing.
    """

    forbidden: list[str] = field(default_factory=list)
    """Text the answer must not contain, for `excludes`. Any one of them fails the run."""
    keywords: list[str] = field(default_factory=list)
    """Words or phrases the answer should mention, for `keyword_coverage`. The score is the share that appear."""
    expected_tools: list[str] = field(default_factory=list)
    """The tools the run should call, for `tool_call_accuracy`, and the order it should call them in, for `tool_order`. Matched exactly against tool-call span names."""
    forbidden_tools: list[str] = field(default_factory=list)
    """Tools the run must not call, for `tool_not_called`."""
    context: list[str] = field(default_factory=list)
    """The passages the answer should rest on, for the judged scorers that read context. When empty, the outputs of the run's `retrieval` and `rag_vector_operation` spans are used."""
    case_sensitive: bool = False
    """Case sensitivity for text comparisons."""
    needle: str | None = None
    """Text the answer must contain, for `contains`."""
    expected: str | None = None
    """The expected answer, for `exact_match`."""
    max_ms: float | None = None
    """A budget in milliseconds, for `latency_budget`."""
    max_cost: float | None = None
    """A budget in currency units, for `cost_budget`."""
    max_tool_calls: int | None = None
    """The most tool calls a healthy run should make, for `tool_efficiency`."""
    span_kind: SpanKind | None = None
    """The kind of span that must be present in the run, for `span_present`."""
    span_name: str | None = None
    """The span name that must be present in the run, for `span_present`. Matched exactly against `Span.name`."""
    tool_name: str | None = None
    """The tool that must have run and ended `ok`, for `tool_succeeded`. Matched exactly against the `name` of a span whose kind is `tool_call`, `client_tool_call`, `provider_tool_call` or `mcp_tool_call`."""
    pattern: str | None = None
    """A regular expression the answer must match somewhere, for `matches_pattern`."""
    trajectory_match: str | None = None
    """How `trajectory` compares the tools the run called with `expected_tools`: `strict` for the same calls in the same order and nothing else, `ordered` for the expected calls in order with others allowed between them, `unordered` for every expected call in any order. Defaults to `ordered`."""
    max_steps: int | None = None
    """The most steps a healthy run takes, for `trajectory`. A step is a tool call, a model generation or a handoff."""
    min_chars: int | None = None
    """The shortest acceptable answer in characters, for `answer_length`."""
    max_chars: int | None = None
    """The longest acceptable answer in characters, for `answer_length`."""
    json_schema: Any | None = None
    """A JSON Schema the answer must satisfy, for `json_schema`. `type`, `properties`, `required`, `items`, `enum`, `minimum`, `maximum`, `minLength`, `maxLength` and `additionalProperties: false` are checked."""

@dataclass(**_SLOTS)
class ScoreRunRequest:
    """A request to run scorers over runs that are already stored.

Scoring reads stored runs rather than running inline, so an evaluation can never add latency to the request it is judging, and a scorer that fails can never fail a user's request.
    """

    trace_ids: list[str] = field(default_factory=list)
    """Score exactly these runs. Takes precedence over the window."""
    scorers: list[str] = field(default_factory=list)
    """Which scorers to run, by id. Empty means every scorer that needs no configuration."""
    dry_run: bool = False
    """Report what would happen and store nothing."""
    project: str | None = None
    """Which project's runs to score. Defaults to the server's default project."""
    from_: datetime | None = None
    """Otherwise, score runs started after this time."""
    limit: int | None = None
    """Most runs to score in one request, newest first, when `trace_ids` is empty. Unset, it is 100 for deterministic scorers, and for a pass with a judged scorer it is as many runs as fit in the request deadline. A judged pass that names more runs than fit, through `limit` or `trace_ids`, is refused with the cap."""
    config: ScorerConfig | None = None
    """Settings for the scorers that need them."""
    source: ScoreSource | None = None
    """What to record as the origin of these scores."""

@dataclass(**_SLOTS)
class SkippedScorer:
    """A scorer that declined to answer on one run.
    """

    scorer_id: str
    """Which scorer declined."""
    trace_id: str
    """Which run it declined on."""
    reason: str
    """Why. Never silent: a scorer that cannot answer is more interesting than one that returned a number."""

@dataclass(**_SLOTS)
class ScoreRunResult:
    """What one scoring pass did.
    """

    traces_scored: int
    """Runs looked at."""
    scores_written: int
    """Scores produced and stored."""
    skipped: list[SkippedScorer] = field(default_factory=list)
    """Every scorer that declined, with its reason."""
    scores: list[Score] = field(default_factory=list)
    """The scores themselves, so the caller sees them without a second read."""

@dataclass(**_SLOTS)
class ScorerDelta:
    """How one scorer moved between two experiments.
    """

    scorer_id: str
    """Which scorer."""
    baseline_n: int
    """Number of baseline observations."""
    candidate_n: int
    """Number of candidate observations."""
    decidable: bool
    """Whether the sample is large enough for the delta to mean anything. A deliberately blunt rule rather than a significance test: below twenty observations on either side, mcpd says the comparison is not decidable rather than dressing noise up as a finding."""
    direction: ScoreDirection
    """Which way this scorer's value is better, from the catalogue. Scorers the daemon does not know read as higher is better."""
    regressed: bool = False
    """Whether this scorer moved the wrong way: decidable, and the whole interval on the worse side of zero."""
    baseline_mean: float | None = None
    """Mean on the baseline, when it ran there."""
    candidate_mean: float | None = None
    """Mean on the candidate."""
    delta: float | None = None
    """Candidate minus baseline."""
    ci_low: float | None = None
    """Lower end of the 95% confidence interval on `delta`, from Welch's t-test. Absent when either side has fewer than two observations."""
    ci_high: float | None = None
    """Upper end of that interval."""
    p_value: float | None = None
    """Two-sided p-value for the difference in means."""

@dataclass(**_SLOTS)
class ExperimentComparison:
    """Two experiments, compared.

The point of an experiment is the comparison, so this reports the delta per scorer beside the sample sizes. A mean that moved on four runs is noise, and printing the counts next to it is what stops it being read as a result.
    """

    baseline: str
    """The baseline's identifier."""
    candidate: str
    """The candidate's identifier."""
    regressed: bool
    """Whether any decidable scorer moved down."""
    scorers: list[ScorerDelta] = field(default_factory=list)
    """One row per scorer present on either side."""

@dataclass(**_SLOTS)
class CaptureRequest:
    """A request to turn stored runs into dataset cases.
    """

    trace_ids: list[str] = field(default_factory=list)
    """The runs to capture. Each becomes one item, keeping its trace id so anyone can go back and read what actually happened."""
    use_output_as_expected: bool = False
    """Record each run's own output as the expected answer. Leave this off for a run that went wrong: capturing a bad answer as the reference would enshrine the bug."""

@dataclass(**_SLOTS)
class CaptureResult:
    """What a capture added to a dataset.
    """

    captured: int
    """Items added."""
    requested: int
    """Runs asked for. A gap means a run was not found."""

@dataclass(**_SLOTS)
class ItemsAdded:
    """How many cases a dataset gained.
    """

    added: int
    """Items added."""

@dataclass(**_SLOTS)
class ErasureResult:
    """What an erasure request removed, counted per signal. The subject is not echoed back: it has been erased, and repeating it in a response that may be logged on the way out would write it down again.
    """

    project: str
    """The project the erasure ran in. No other project was read or touched."""
    traces: int
    """How many runs were removed."""
    spans: int
    """How many spans were removed, with the prompts and completions they carried."""
    logs: int
    """How many log lines were removed."""
    metrics: int
    """How many metric points were removed."""
    scores: int
    """How many scores were removed."""
    feedback: int
    """How many pieces of feedback were removed."""
    deleted: int
    """The total across every signal, so a caller that wants one number does not add them up."""
    datasets: list[str] = field(default_factory=list)
    """The datasets those emptied cases sit in, so the reply can say where they are."""
    runs_erased: int | None = None
    """How many durable runs were removed from the run store, each with its step attempts, event log, deadlines, waits, the events it consumed and any span still queued for the trace store. A run is matched on the `user_id` in its correlation, as it was started with it, and removed with the whole tree of subworkflow runs it belongs to. Not part of `deleted`, which counts the trace store. Absent when this daemon does not serve runs."""
    runs_in_flight: int | None = None
    """How many runs attributed to this person were left untouched because a run in their tree has not ended. Removing a live run would corrupt the workflow it is part of, so these are counted rather than skipped in silence, and `note` names the root runs. Cancel them, or let them end, then send the erasure again. Absent when this daemon does not serve runs."""
    aspect_notes_erased: int | None = None
    """How many aspect notes were removed, matched by the run they describe and by the person stamped on them. Not part of `deleted`. Absent only from a store that holds no notes."""
    dataset_items_erased: int | None = None
    """How many dataset cases captured from an erased run were emptied in place. They are not deleted, because a case was kept on purpose, so they are not part of `deleted`."""
    deletes_pending: bool | None = None
    """True when the storage backend applies the deletes after this reply is sent, as ClickHouse does, so the counts say what matched rather than what has already left the disk."""
    unmatchable_identifiers: int | None = None
    """How many stored identifiers this request could not have matched, because they were written in the other form, hashed or plain, from the one given. A number above zero means the counts may not be the whole answer."""
    note: str | None = None
    """Why the numbers above may not be the whole answer, in sentences an operator can repeat. Absent when they are the whole answer."""

@dataclass(**_SLOTS)
class DefinitionDocument:
    """A definition as you post it: the graph, and the id and version to store it under. The body is validated at write time against `spec/definition.schema.json`; a body the walker cannot run is refused with a 400 naming the field, and unknown fields are refused, so a guard that is spelled wrong is an error and not a run that skips its guard.
    """

    id: str
    """The definition's identifier within its project. Matches `^[a-z][a-z0-9_]*$`."""
    body: Any
    """The document itself: `input_schema`, `limits`, `steps` and `output`. The definition schema names every field."""
    kind: DefinitionKind | None = None
    """What kind of definition. Absent means `workflow`, the only kind version 1 serves."""
    version: int | None = None
    """The version to store the body under. Absent asks the daemon to choose: the version whose stored body is identical, or one past the highest stored. A named version that is free is taken; one held by an identical body is confirmed; one held by a different body is a 409, because a stored version never changes."""

@dataclass(**_SLOTS)
class DefinitionVersion:
    """Where a posted definition landed.
    """

    id: str
    """The definition's identifier."""
    version: int
    """The version the body is stored under."""
    assigned: bool
    """True when the daemon chose the number because the body was new. False when the body already existed at this version, or you named the version yourself."""

@dataclass(**_SLOTS)
class RunCursor:
    """The frontier of a run: which position in the definition's `steps` it has reached, and what sits there.
    """

    index: int
    """The index into `steps`, or their count at the end of the graph."""
    construct: ConstructKind
    """What is at that position."""
    step_key: str | None = None
    """The step this position belongs to, when the construct has one. A fan-out names its parent step; the branch and item keys are on the attempts."""

@dataclass(**_SLOTS)
class RunTimer:
    """A deadline the daemon holds for a run.
    """

    run_id: str
    """Which run."""
    kind: TimerKind
    """Which deadline."""
    due_at: datetime
    """When it fires."""
    step_key: str | None = None
    """The step the deadline belongs to. Absent for a run-level deadline."""

@dataclass(**_SLOTS)
class RunWait:
    """A suspension waiting for an event.
    """

    run_id: str
    """Which run."""
    step_key: str
    """The `wait_event` step that is waiting."""
    event: str
    """The event name an inbound event must match."""
    key: str
    """The rendered key that picks this run out of every run waiting on that event."""
    expires_at: datetime
    """When waiting stops and the node's `on_timeout` applies."""

@dataclass(**_SLOTS)
class RunDetail:
    """One run and everything the daemon holds for it.
    """

    run: Run
    """The run."""
    attempts: list[StepAttempt] = field(default_factory=list)
    """Every attempt, in the order they were written."""
    timers: list[RunTimer] = field(default_factory=list)
    """The deadlines currently held. Empty once the run has ended."""
    waits: list[RunWait] = field(default_factory=list)
    """The suspensions currently open. Empty once the run has ended."""

@dataclass(**_SLOTS)
class RunListResponse:
    """A page of runs.
    """

    page: Page
    """Cursor and totals."""
    runs: list[Run] = field(default_factory=list)
    """The matching runs, newest first."""

@dataclass(**_SLOTS)
class RunEventPage:
    """A page of a run's event log.
    """

    has_more: bool
    """Whether rows beyond the last one here existed when the page was read. A run that is still moving will write more."""
    events: list[RunEvent] = field(default_factory=list)
    """The rows after the requested sequence number, in order."""

@dataclass(**_SLOTS)
class CauseLink:
    """One edge of a cause chain: an ancestor step, and the template path through which the step below it read that ancestor's output. An ancestor read through two paths appears twice, once per path.
    """

    step_key: str
    """The ancestor step, keyed the way the journal keys it: `refund/issue`, `classify[3]`, `poll[7]/classify`."""
    status: AttemptStatus
    """Where the ancestor's latest attempt ended."""
    attempts: int
    """How many attempts were made at the ancestor. More than one on a step that ended `succeeded` is worth reading: it got there, but not the first time."""
    template_path: str
    """The path as the definition writes it inside `${...}`, such as `steps.classify.output.kind`. The definition declares this edge, so the chain is read rather than inferred from timing."""
    consumed_by: str
    """The step key that read this ancestor through `template_path`. On a direct edge it is the step the answer is about; deeper in the chain it is another ancestor."""
    depth: int
    """How far this ancestor sits from the step the answer is about. `1` is read directly, `2` is read by something the step read directly, and so on."""
    silent_producer: bool
    """True when this ancestor ended `succeeded` and every step between it and the failed step ended `succeeded` too. Nothing on that path is marked failed, so no status code points at it and the value it produced is the suspect. It is not the same as `status` being `succeeded`: an ancestor sitting behind a step that did fail is not a silent producer, because that failure already explains the run."""
    value_kind: ValueKind | None = None
    """The JSON type `template_path` resolved to on the ancestor's stored output. The value itself is never returned, only its shape. Absent when there is no output to read, which is the case for a step that never ran and for one that failed."""
    value_empty: bool | None = None
    """True when that value was null, an empty string, an empty array or an empty object. A step that ended `succeeded` and handed the step below it nothing is the failure a status code cannot catch, and this is the field that shows it. Absent for the same reason `value_kind` is."""

@dataclass(**_SLOTS)
class StepCause:
    """Why one step of a run failed, answered by walking backwards along the data dependencies its definition declares. The daemon reads the run journal and the pinned definition version, both of which it has to hold to render `${...}` templates at all, so this answer is available with span payload capture off and with no model, prompt or completion involved.
    """

    run_id: str
    """The run this answer is about."""
    step_key: str
    """The step this answer is about, as it was asked for."""
    status: AttemptStatus
    """Where that step's latest attempt ended."""
    attempts: int
    """How many attempts were made at it."""
    truncated: bool
    """True when the walk stopped at `max_depth` with ancestors left unvisited, so `chain` is a prefix of the answer rather than the whole of it."""
    chain: list[CauseLink] = field(default_factory=list)
    """The ancestors whose output this step consumed, directly or transitively. Ordered by `depth` ascending, then by the order the paths appear in the definition, so two calls against one run return the same list in the same order. Empty when the step reads nothing but the run input."""
    error: StepError | None = None
    """Why the latest attempt failed, when it did. Absent on a step that did not fail, because the endpoint answers for any step, not only a failed one."""

@dataclass(**_SLOTS)
class StartRunRequest:
    """A request to start a run.
    """

    definition_id: str
    """Which definition to run."""
    project: str | None = None
    """Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none."""
    version: int | None = None
    """Which version. Absent pins the highest stored version at this moment, and the run records it."""
    input: Any | None = None
    """The run's input. Validated against the definition's `input_schema` when it declares one; a failure is a 400."""
    correlation: Correlation | None = None
    """Context to store on the run and copy onto every span the daemon writes for it. `project` and `run_id` inside it are ignored: the run's own values win."""
    idempotency_key: str | None = None
    """Makes the start idempotent per project for 24 hours: a repost returns the same run and starts nothing, and two simultaneous posts of one key produce one run."""

@dataclass(**_SLOTS)
class ClaimRequest:
    """A worker asking for work.
    """

    worker_id: str
    """Who is asking. Recorded on the attempt as `lease_owner` and in the run's event log. Nothing else about the worker is stored."""
    max: int
    """The most leases to hand back, 1 to 100."""
    wait_ms: int
    """How long to hold the request open waiting for work, 0 to 60000 milliseconds. The daemon holds a claim for at most four fifths of its request timeout, 24000 at the default 30 seconds, so a longer wait is answered early rather than cut off with a 503. An empty answer means nothing became runnable in time: reissue the claim."""
    project: str | None = None
    """Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none."""
    run_id: str | None = None
    """Restrict the claim to one run, which is how a script drives its own run inline."""

@dataclass(**_SLOTS)
class StepLease:
    """A step handed to a worker. Every later call about the attempt is addressed by `lease_id`.
    """

    lease_id: str
    """The address of every later call about this attempt."""
    run_id: str
    """Which run."""
    project: str
    """Which project the run belongs to."""
    definition_id: str
    """Which definition the run is executing."""
    definition_version: int
    """Which version. Fail the lease with class `unservable` if you do not serve it."""
    step_key: str
    """The step to run. A worker that does not recognise it fails the lease with class `unservable` rather than guessing."""
    kind: StepKind
    """Whether this is a plain task or an agent turn."""
    attempt: int
    """Which attempt this is. The fence: a call about this lease is refused once a later attempt exists."""
    effect: Effect
    """Under `at_most_once`, post `started` and wait for its 200 before running the body."""
    expires_at: datetime
    """When the lease lapses. Heartbeat every `heartbeat_ms` to keep it."""
    idempotency_key: str
    """A key stable across every attempt of this step: the SHA-256 of `run_id:step_key`. Pass it to any downstream call that can deduplicate."""
    prior: Any
    """The ended attempts of earlier steps, keyed by step id: `{ status, output?, error? }` for the latest attempt of each, or `{ status: skipped }`. Only ended attempts appear."""
    correlation: Correlation
    """The run's correlation, with `run_id` set. Put it on every span you record inside this step."""
    trace_id: str
    """The run's trace. Set it on every span you record inside this step."""
    span_id: str
    """The span the daemon has already opened for this attempt. Set it as `parent_span_id` on your own spans, so a model call or a tool call nests under the step."""
    parent_span_id: str
    """The run's root span, which this attempt's span hangs off."""
    deadline_ms: int | None = None
    """The step's own timeout, when the node declares one. It bounds the attempt whatever the heartbeats say."""
    input: Any | None = None
    """The node's `input` template rendered for this attempt, or the run's input when the node declares none."""
    state: Any | None = None
    """The run's state object as it stands."""

@dataclass(**_SLOTS)
class ClaimResponse:
    """What a claim answered.
    """

    leases: list[StepLease] = field(default_factory=list)
    """Zero to `max` leases. Empty when nothing became runnable within `wait_ms`."""

@dataclass(**_SLOTS)
class LeaseState:
    """What `started` and `heartbeat` answer.
    """

    expires_at: datetime
    """The lease's new expiry."""
    reclaimed: bool
    """True when this call found the attempt `orphaned` and restored it to the state it came from."""

@dataclass(**_SLOTS)
class StepUsage:
    """Token usage a worker reports on `complete`. The daemon prices it from its price book and charges the attempt, the run and the root of the run tree.
    """

    input_tokens: int | None = None
    """Total input tokens."""
    output_tokens: int | None = None
    """Total output tokens."""
    total_tokens: int | None = None
    """Input plus output, when the provider reports a total that differs from the sum."""
    input_details: InputTokenDetails | None = None
    """How the input tokens were billed."""
    output_details: OutputTokenDetails | None = None
    """How the output tokens were billed."""
    model: str | None = None
    """The model that produced the usage, which the price book needs to price it."""
    provider: str | None = None
    """The provider that served the model."""

@dataclass(**_SLOTS)
class StepResult:
    """What a worker posts to complete a step.
    """

    output: Any | None = None
    """The step's output. Later steps read it as `steps.<id>.output`."""
    state: Any | None = None
    """An object to merge shallowly into the run's state."""
    usage: StepUsage | None = None
    """Token usage to price and charge."""

@dataclass(**_SLOTS)
class StepError:
    """Why a step failed, as the worker reports it.
    """

    class_: StepErrorClass
    """What kind of failure. Decides whether the daemon retries."""
    message: str
    """What went wrong, for the person reading the run."""
    details: Any | None = None
    """Structured detail. Stored on the attempt."""
    retry_after_ms: int | None = None
    """The least the daemon should wait before the next attempt, when the worker knows: a rate limit's reset, say. Honoured when it is larger than the backoff. At most 315360000000 (3650 days), the bound on every duration in a definition; a larger value is a 400 naming this field."""

@dataclass(**_SLOTS)
class StepFailure:
    """What a worker posts to fail a step.
    """

    error: StepError
    """Why it failed."""

@dataclass(**_SLOTS)
class StepAck:
    """What `complete` and `fail` answer.
    """

    run_status: RunStatus
    """Where the run is after this call."""
    next: str
    """`claim` when more work is runnable now, `idle` otherwise."""
    attempts_left: int | None = None
    """After a `fail`: how many attempts the node's retry budget still allows."""

@dataclass(**_SLOTS)
class StepConflict:
    """The body of a 409 from a worker call. `reason` says what is true and what the worker should do next.
    """

    error: str
    """Always `conflict`."""
    reason: ConflictReason
    """Which conflict."""
    message: str
    """The reason, in words."""
    attempt: int
    """The attempt the lease addressed."""
    run_status: RunStatus
    """Where the run is."""
    attempt_status: AttemptStatus | None = None
    """Where that attempt is, when it still exists. Under `already_applied` this says how the step ended."""

@dataclass(**_SLOTS)
class PublishEventRequest:
    """An event for whichever run is waiting for it.
    """

    event: str
    """The event name, matched against open waits."""
    key: str
    """Matched against the rendered `key` of each waiting run. Delivered to the matching run whose wait expires soonest."""
    project: str | None = None
    """Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none."""
    payload: Any | None = None
    """Becomes the waiting step's output."""
    event_id: str | None = None
    """Your own id for this event. A supplied id seen before answers `duplicate` and writes nothing, whether or not a run is waiting. Absent, the daemon derives one from `project`, `event`, `key` and `payload`, and a derived id counts as a duplicate only when no run is waiting, because two identical approvals for two runs on one key are two events. Send your own id from any handler that can be retried."""

@dataclass(**_SLOTS)
class EventReceipt:
    """What publishing an event did.
    """

    outcome: EventOutcome
    """Delivered, parked or duplicate."""
    event_id: str
    """The id used, supplied or derived."""

@dataclass(**_SLOTS)
class SignalRequest:
    """An event for one run, addressed by run id rather than by key. The daemon reads the run's open wait on `event`, takes its key, and routes the event as a published one.
    """

    event: str
    """The event name. The run must hold an open wait on it."""
    payload: Any | None = None
    """Becomes the waiting step's output."""
    event_id: str | None = None
    """Your own id for this event, deduplicated as on a published event."""

@dataclass(**_SLOTS)
class WorkerRegistration:
    """A worker announcing the definitions it serves.
    """

    worker_id: str
    """Who is registering. Not stored: dispatch reads the runnable set, never a worker table."""
    definitions: list[DefinitionDocument] = field(default_factory=list)
    """Each is stored under the versioning rule, exactly as if posted to the definitions endpoint."""
    project: str | None = None
    """Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none."""

@dataclass(**_SLOTS)
class WorkerRegistered:
    """What registering answered.
    """

    worker_id: str
    """The id you registered with."""
    lease_ttl_ms: int
    """How long a lease lasts without a heartbeat."""
    heartbeat_ms: int
    """How often to heartbeat a held lease: a third of `lease_ttl_ms`."""
    registered: list[DefinitionVersion] = field(default_factory=list)
    """Where each definition landed, in the order posted."""

@dataclass(**_SLOTS)
class Monitor:
    """A saved condition the daemon evaluates on a schedule. Monitors are configured in the daemon's configuration file, not through the API.
    """

    id: str
    """Stable identifier."""
    name: str
    """What a person should read when it trips."""
    project: str
    """Which project it watches."""
    watch: MonitorWatch
    """What it measures."""
    compare: MonitorCompare
    """Which direction trips it."""
    threshold: float
    """The threshold."""
    window_seconds: int = 300
    """How far back each evaluation looks."""
    min_samples: int = 20
    """The fewest runs the window must contain before the monitor will trip. Without it a single failed run in a quiet hour is a 100% error rate, and the first thing anyone does with an alert that cries wolf is mute it."""
    enabled: bool = True
    """Whether it is evaluated at all."""
    scorer_id: str | None = None
    """Which scorer, when `watch` is `score_mean`."""
    webhook: str | None = None
    """Where to POST when it trips or clears. Set as a full URL in the config file; the API returns only its host, because a webhook URL is often its own credential."""

@dataclass(**_SLOTS)
class MonitorReading:
    """One evaluation of a monitor.
    """

    monitor_id: str
    """Which monitor."""
    at: datetime
    """When it was evaluated."""
    samples: int
    """How many runs the window held."""
    condition: MonitorCondition
    """The resulting condition."""
    detail: str
    """What a person should read."""
    value: float | None = None
    """The measured value, when there was enough data to measure one."""

@dataclass(**_SLOTS)
class MonitorState:
    """A monitor and the condition it is latched in.
    """

    monitor: Monitor
    """The monitor."""
    condition: MonitorCondition
    """Its current condition."""
    last: MonitorReading | None = None
    """The most recent reading. Absent before the first evaluation."""
    tripped_at: datetime | None = None
    """When it first tripped, while it stays tripped."""

@dataclass(**_SLOTS)
class Slo:
    """A service level objective: the fraction of runs that must succeed over a window. A monitor asks whether a number is over a line right now. An objective asks whether the promise is going to be missed and how fast the error budget is burning, over two windows at once, so a two minute blip and a slow leak get the different answers they need. Objectives are configured in the daemon's configuration file, not through the API.
    """

    id: str
    """Stable identifier."""
    name: str
    """What a person should read."""
    project: str
    """Which project it covers."""
    objective: float
    """The fraction of runs that must succeed, between 0 and 1. `0.995` is ninety-nine and a half percent of runs."""
    window_days: int = 30
    """How long the promise is measured over."""
    min_samples: int = 20
    """The fewest runs a burn window must hold before it may trip."""
    enabled: bool = True
    """Whether it is evaluated at all."""
    webhook: str | None = None
    """Where to POST on a transition. Set as a full URL in the config file; the API returns only its host, because a webhook URL is often its own credential."""

@dataclass(**_SLOTS)
class SloReading:
    """One evaluation of an objective.
    """

    slo_id: str
    """Which objective."""
    at: datetime
    """When it was evaluated."""
    samples: int
    """Runs in the whole window."""
    condition: MonitorCondition
    """The resulting condition."""
    detail: str
    """What a person should read."""
    success_rate: float | None = None
    """The observed success rate over the whole window."""
    budget_remaining: float | None = None
    """The fraction of the error budget still unspent. Negative once it is overspent, which says by how much the promise was missed."""
    burn_fast: float | None = None
    """The burn rate over the one hour window. A rate of 1 spends the whole budget exactly over the whole window."""
    burn_slow: float | None = None
    """The burn rate over the six hour window. Both windows must exceed their trip rate together before the objective trips, which is what stops a brief spike paging anyone."""

@dataclass(**_SLOTS)
class SloState:
    """An objective and the condition it is latched in.
    """

    slo: Slo
    """The objective."""
    condition: MonitorCondition
    """Its current condition."""
    last: SloReading | None = None
    """The most recent reading. Absent before the first evaluation."""
    tripped_at: datetime | None = None
    """When it first tripped, while it stays tripped."""

@dataclass(**_SLOTS)
class StoreCapabilities:
    """What the storage backend behind this daemon can and cannot do. Three backends implement one storage trait and they are not equal: a reader on ClickHouse who searches for `CAFE` with an acute and finds nothing has no way to learn that this backend folds ASCII case and no more. Read this once at connect time and offer only what the deployment answers. Every field is named after the capability an endpoint names when it refuses with 501, so the two can be compared without translation.
    """

    backend: str
    """Which backend this describes: `sqlite`, `postgres` or `clickhouse`. The same string `/healthz` reports as `storage`."""
    text_search: TextSearch
    """What the `q` parameter matches."""
    unicode_case_folding: bool
    """Whether `q` folds case for letters outside ASCII. False means a search for `CAFE` with an acute does not find the lowercase form, which a reader otherwise meets as an empty result they cannot explain."""
    immediate_deletes: bool
    """Whether a delete is on disk when the call returns. False means the backend accepted the delete and applies it afterwards, so a count says what matched rather than what has already gone."""
    discovery: bool
    """Whether `GET /v1/api/discovery` answers. False means a filter box has to be filled from what is already on screen."""
    discovery_label_values: bool
    """Whether the `metric_label_value` facet is answerable, not only the keys. Separate because the values are the expensive half: a key set is small and bounded, and a value set is not."""
    delta_polling: bool
    """Whether `GET /v1/api/deltas` answers, so a list refreshes from a cursor rather than by running the whole query again."""
    per_signal_retention: bool
    """Whether retention can carry a different window per signal. False means one window covers all five."""
    declarative_retention: bool
    """Whether the window can be handed to the database engine, so nothing has to run a delete on a timer."""
    partitioning: Partitioning
    """How the signal tables are cut on their anchor time."""
    partition_drop_retention: bool
    """Whether a retention pass can drop a whole partition instead of deleting rows one at a time."""
    idempotent_schema_init: bool
    """Whether a start with nothing to apply issues no schema statement at all. False means every process replays the schema on every boot, which is harmless on one node and is metadata churn across a replicated deployment."""
    theme_analysis: bool
    """Whether this deployment answers the Intelligence endpoints. False means the store holds no note or theme tables and every one of those routes refuses with 501 naming this capability, so a client decides once whether to offer the feature rather than showing a reader an empty page they cannot account for. Every backend that answers gives the same themes: the clustering runs in the daemon over stored notes and not in SQL, so there is nothing here for a backend to do differently."""

@dataclass(**_SLOTS)
class DiscoveredValue:
    """One value a filter dimension takes in a project.
    """

    value: str
    """The value, exactly as it is stored, so it can be sent straight back as a filter with no further mapping."""
    last_seen: datetime
    """When mcpd last received a signal carrying it. This is arrival time and not the signal's own timestamp, so a backfill of last year's data does not make a value look current. Carried so a caller can rank down or grey out a value nothing has reported for a month. Values are ordered by `value` and not by this: a list somebody reads alphabetically is stable between polls, and one ordered by recency reshuffles under them as data arrives."""

@dataclass(**_SLOTS)
class DiscoveryResponse:
    """The values one filter dimension takes, for a filter box. Answered from a distinct-value list the daemon maintains as signals arrive, not by scanning the signal tables, because the question is asked on every keystroke and the tables it would otherwise scan are the largest thing the deployment holds.
    """

    facet: Facet
    """Which dimension was asked for."""
    values: list[DiscoveredValue] = field(default_factory=list)
    """The values, ordered by value, ascending."""
    key: str | None = None
    """The label key the values belong to, echoed back for `metric_label_value`. Absent for every other facet."""

@dataclass(**_SLOTS)
class DeltaResponse:
    """What has arrived since a cursor. A poll is not a page: paging walks a fixed result set backwards in the sort you chose, and polling walks forwards in arrival order and never ends, so the two cursors are different things and are deliberately not interchangeable. Rows come back oldest arrival first, so applying them in order to a list you already hold gives the same result as having listed afresh.
    """

    signal: DeltaSignal
    """Which signal these rows are. Exactly one of the three arrays below is present, and it is this one."""
    cursor: str
    """Where the next poll resumes. Always present, and unchanged from the request when nothing came back: moving it forward on an empty poll is what would drop a row that lands a moment later with an earlier arrival time. Treat it as meaningless, and send back the one you were given."""
    has_more: bool
    """True when the page filled and more is waiting, so a caller draining a backlog polls again at once rather than waiting for its next tick."""
    traces: list[Trace] | None = None
    """The runs that arrived, when `signal` is `traces`."""
    spans: list[Span] | None = None
    """The steps that arrived, when `signal` is `spans`."""
    logs: list[LogRecord] | None = None
    """The log lines that arrived, when `signal` is `logs`."""

@dataclass(**_SLOTS)
class AspectLimits:
    """The bounds a clustering pass runs under. They are published because each one changes what a reader is looking at: a floor that has not been reached means no themes at all rather than an empty result, and a ceiling that has been reached means every share is a share of what was read. They are copied from the divergence watch, which is the one windowed statistic mcpd already ships. `pass_armed` is the first of them to read: the other three describe what a pass would do, and it says whether one runs.
    """

    min_notes_to_cluster: int
    """How many notes one aspect on one entity needs before a pass runs at all, 50 by default. Below it the aspect reads `collecting` and no snapshot is written."""
    max_notes_per_pass: int
    """The most notes one pass reads, 20,000 by default, taken newest first. A window holding more is clustered from the newest of them and its snapshot is marked `capped`."""
    page_size: int
    """How many notes the pass reads at a time while it fills a window, 500 by default. It bounds what the pass holds, not what it covers."""
    max_aspects: int
    """How many aspects one project may define, the four built-ins included. It is configuration and is published here so a client that lets an operator add an aspect knows when to stop instead of finding out from a refusal."""
    pass_armed: bool
    """Whether this deployment runs the clustering pass at all. False means no snapshot will ever be written however many notes arrive, so every aspect stays `collecting` or `processing` for ever and no theme route will have anything to answer with. It is configuration and not a fault, and it is not the same thing as the 501 the theme routes give: that says the storage backend cannot hold the tables, which nobody can switch on, while this says nobody has switched the pass on yet. It is published here rather than on every progress row because it is one fact about the deployment, and a client reads this route once to learn what the aspects are called."""

@dataclass(**_SLOTS)
class AspectCatalogue:
    """Every aspect a project describes traces along, with the bounds the clustering pass runs under. The four built-ins are rows in the same table as anything you define, so there is no fallback list inside a client that can quietly disagree with this one.
    """

    limits: AspectLimits
    """The bounds a pass runs under in this deployment."""
    aspects: list[Aspect] = field(default_factory=list)
    """The aspects, in `sort_order`. Disabled aspects are included, because notes already recorded under one stay readable and a name that vanished from this list would make them look like a mistake."""

@dataclass(**_SLOTS)
class EntityIndexRow:
    """One entity that has aspect notes, with the roll-up a list needs to be worth reading. A row appears as soon as the first note is recorded, before any theme exists, because an empty screen with a count moving on it is a different thing from an empty screen and a reader has to be able to tell them apart.
    """

    entity_kind: EntityKind
    """The kind of thing this row is about."""
    entity_name: str
    """Its name."""
    trace_count: int
    """How many of its traces carry at least one note. This is not the entity's trace count: writing a note lags ingestion, so a busy agent can show far fewer here."""
    note_count: int
    """How many notes are recorded about it."""
    enabled_aspects: int
    """How many aspects are switched on for this entity's project."""
    ready_aspects: int
    """How many of those have themes to read. The two numbers read as `2 of 4` and are the honest answer to whether this row is worth opening yet."""
    readiness: AspectReadiness
    """The row's overall state, which is the least advanced of its enabled aspects, so a row says `processing` while any one of them is still short of a snapshot. It never reports the best case."""
    last_note_at: datetime
    """When the newest note about this entity was recorded, so a reader can tell a list that is filling from one that stopped."""
    latest_snapshot_at: datetime | None = None
    """The cutoff of this entity's newest snapshot. Absent until a pass has written one, which is the ordinary state of a row that is still collecting."""

@dataclass(**_SLOTS)
class EntityIndexResponse:
    """A page of the entities that have aspect notes.
    """

    page: Page
    """Cursor and totals."""
    entities: list[EntityIndexRow] = field(default_factory=list)
    """The matching entities, most recently written to first."""

@dataclass(**_SLOTS)
class AspectProgressRow:
    """How far one aspect has got for one entity. Counted from stored notes, so it is what has happened and not a projection of what will.
    """

    aspect: str
    """Which aspect this row is about."""
    readiness: AspectReadiness
    """The state this aspect is in for this entity."""
    note_count: int
    """How many notes are recorded along it."""
    trace_count: int
    """How many distinct traces those notes were written about."""
    notes_needed: int
    """How many more notes this aspect needs before a pass will run. Zero once the floor is reached, which is what turns a wait into a number a reader can watch."""
    last_note_at: datetime | None = None
    """When the newest note along this aspect was recorded. Absent when there are none."""

@dataclass(**_SLOTS)
class AspectProgress:
    """What the daemon has recorded for one entity, aspect by aspect. This is the answer to why a themes screen is empty, and it separates the three cases that look identical on screen: nothing has been recorded, enough has been recorded and no pass has run yet, or there are themes to read and the reader is looking at the wrong aspect.
    """

    entity_kind: EntityKind
    """The kind of entity this describes."""
    entity_name: str
    """Its name."""
    readiness: AspectReadiness
    """The entity's overall state, which is the least advanced of its enabled aspects."""
    trace_count: int
    """How many of this entity's traces carry at least one note."""
    aspects: list[AspectProgressRow] = field(default_factory=list)
    """One row per enabled aspect, in catalogue order."""
    latest_snapshot: ThemeSnapshot | None = None
    """The newest snapshot for this entity, when a pass has written one. Absent while every aspect is still collecting."""

@dataclass(**_SLOTS)
class SnapshotLandmark:
    """One snapshot in a timeline, with the reason it was chosen.
    """

    snapshot: ThemeSnapshot
    """The snapshot."""
    reason: SnapshotLandmarkReason | None = None
    """Why this snapshot is in the selection. Present for `presentation=landmarks` only; an inventory returns consecutive snapshots and chooses nothing, so there is nothing to report."""

@dataclass(**_SLOTS)
class SnapshotListResponse:
    """A selection of one entity's snapshots, oldest first, so a timeline can be drawn without reordering anything.
    """

    total: int
    """How many snapshots the entity has in the requested range. This is the number the selection was drawn from and not the number returned, so a timeline can say what it is standing in for."""
    page: Page
    """Cursor and totals."""
    snapshots: list[SnapshotLandmark] = field(default_factory=list)
    """The selected snapshots, oldest first."""

@dataclass(**_SLOTS)
class ThemeNode:
    """One node of one stage of a flow: a theme, the stage's noise, or the overflow the stage folded away.
    """

    node_id: str
    """Identifies this node inside this flow, and is what a link names at either end. It is not a theme id: the noise and overflow nodes have one of these and no theme id."""
    kind: ThemeNodeKind
    """Whether this node is a theme, the noise or the overflow."""
    label: str
    """What to show for this node: a theme's own label, or the words for noise or overflow."""
    trace_count: int
    """How many traces this node accounts for."""
    coverage: float
    """This node's share of the stage's traces, between 0 and 1. A stage's nodes cover all of it, so its shares add to 1."""
    theme_id: str | None = None
    """The theme this node stands for. Present only when `kind` is `theme`, which is what makes the other two impossible to drill into by accident."""

@dataclass(**_SLOTS)
class ThemeStage:
    """One aspect's column in a flow. Stages come back in the order the aspects were asked for, and links only ever join neighbouring stages, so the order decides which co-occurrences are visible and is not a display preference.
    """

    aspect: str
    """Which aspect this stage is."""
    trace_count: int
    """How many traces this stage accounts for."""
    nodes: list[ThemeNode] = field(default_factory=list)
    """The stage's nodes, largest first, with the noise and overflow nodes last."""

@dataclass(**_SLOTS)
class ThemeLink:
    """A ribbon between one node of a stage and one node of the next. It says the two describe the same traces and nothing more: a link between a goal theme and an outcome theme means they were written about the same runs, not that one caused the other.
    """

    source_node_id: str
    """The node this link leaves, in the earlier stage."""
    target_node_id: str
    """The node it reaches, in the next stage."""
    trace_count: int
    """How many traces are assigned to both ends."""
    source_share: float
    """That count as a fraction of the source node's traces, between 0 and 1."""
    target_share: float
    """That count as a fraction of the target node's traces, between 0 and 1. The two shares differ whenever the nodes are different sizes, and reading only one of them is how a thin ribbon gets mistaken for a weak relationship."""

@dataclass(**_SLOTS)
class ThemeFlow:
    """One snapshot read across two to four aspects at once: a stage per aspect, and the links between neighbouring stages. Association, never causation.
    """

    snapshot: ThemeSnapshot
    """The snapshot this flow was computed from, so every count on the page can be read against the window it came from."""
    stages: list[ThemeStage] = field(default_factory=list)
    """One stage per aspect, in the order they were asked for."""
    links: list[ThemeLink] = field(default_factory=list)
    """The links between neighbouring stages. There are none between stages that are not neighbours, which is why the order of the aspects matters."""

@dataclass(**_SLOTS)
class ThemeDetail:
    """One theme in one snapshot, or the answer that this snapshot did not hold it.
    """

    snapshot: ThemeSnapshot
    """The snapshot that was read, with its own counts, so a caller can write `28 of 70 traces in this window` without a second request."""
    theme: Theme | None = None
    """The theme, when this snapshot holds it. Absent is an answer and not an error: the theme did not exist in that window, which is what scrubbing a timeline back past a theme's birth looks like. A theme id this entity has never had is a 404 instead."""

@dataclass(**_SLOTS)
class ThemeExample:
    """One note from a theme, with the run it was written about. The two ids are different things: `note_id` identifies the sentence and `trace_id` identifies the run it describes.
    """

    note_id: str
    """The note this example is."""
    trace_id: str
    """The trace the note was written about."""
    text: str
    """The sentence, after redaction. This is the wording that put the trace in this theme, so a reader can see why it landed here rather than take it on trust."""
    similarity: int
    """How much this note's wording overlaps the theme's label, counted out of 64. It is a count of shared sketch positions and not a percentage, and it is an integer on purpose: no part of the clustering uses floating-point arithmetic, so two replicas cannot order the same notes differently."""
    trace_started_at: datetime
    """When the run began, so examples can be read against the period they came from rather than against when the note was written."""

@dataclass(**_SLOTS)
class ThemeExampleListResponse:
    """A page of the notes behind one theme.
    """

    page: Page
    """Cursor and totals."""
    examples: list[ThemeExample] = field(default_factory=list)
    """The notes, most like the theme's label first."""

@dataclass(**_SLOTS)
class ThemeHistoryPoint:
    """One theme in one snapshot of its life. Every count belongs to that snapshot alone, and windows overlap, so a run of these is read as a series and never added up.
    """

    snapshot_id: str
    """The snapshot this point comes from."""
    cutoff_at: datetime
    """When that snapshot became the current state."""
    window_from: datetime
    """Start of that snapshot's window, inclusive."""
    window_to: datetime
    """End of that snapshot's window, exclusive."""
    state: ThemeState
    """What had happened to the theme by this point."""
    note_count: int
    """How many notes were in the theme here."""
    trace_count: int
    """How many distinct traces those notes were written about."""
    coverage: float
    """The theme's share of the notes that snapshot read for this aspect, between 0 and 1. Coverage and raw count can move in opposite directions, and coverage is the one that says whether a theme is gaining ground."""
    split_from: list[str] = field(default_factory=list)
    """The themes this one came out of at this point. Empty unless `state` is `split`."""
    split_into: list[str] = field(default_factory=list)
    """The themes that came out of this one at this point. Both directions are stored, so a caller never has to read a second theme's history to learn what happened to this one."""
    merged_from: list[str] = field(default_factory=list)
    """The themes that became this one at this point. Empty unless `state` is `merge`."""
    merged_into: list[str] = field(default_factory=list)
    """The theme this one became at this point, when it stopped being its own theme."""

@dataclass(**_SLOTS)
class ThemeHistory:
    """One theme across every stored snapshot that holds it, oldest first. This is the only theme response that names no single snapshot, because the series is the answer.
    """

    theme_id: str
    """The theme this history is of."""
    aspect: str
    """The aspect it was clustered along."""
    label: str
    """The theme's label at its newest point. A label can change between snapshots, because the note most like the others can change while the theme holds together."""
    page: Page
    """Cursor and totals."""
    points: list[ThemeHistoryPoint] = field(default_factory=list)
    """The series, oldest first, so nothing has to be reordered before it is drawn or differenced."""

@dataclass(**_SLOTS)
class TraceAspectNotes:
    """Every aspect note recorded about one trace, which is at most one per aspect. Read it beside the trace itself: a note is the sentence that put this run in each of its themes, so it is the shortest route from a theme back to what actually happened.
    """

    trace_id: str
    """The trace these notes are about."""
    notes: list[AspectNote] = field(default_factory=list)
    """The notes, in catalogue order. Empty when nothing has written one yet, which is ordinary for a recent trace."""

@dataclass(**_SLOTS)
class AccountCredentials:
    """An email address and a password, for sign-up and for log-in.
    """

    email: str
    """The account's email address. Compared without regard to case, and with surrounding spaces removed."""
    password: str
    """At least 10 characters and at most 1024 bytes. Stored only as an argon2id hash."""
    totp_code: str | None = None
    """The six-digit code from an authenticator app, or one of the recovery codes, when the account has two-step log-in on. Ignored by sign-up."""

@dataclass(**_SLOTS)
class AccountSession:
    """A logged-in session. Send `token` as `Authorization: Bearer <token>`.
    """

    token: str
    """The session token. It is shown once: the daemon keeps only its SHA-256 digest."""
    expires_at: datetime
    """When the session stops working. Log in again after this."""
    email: str
    """The account's email address, as stored."""
    project: str | None = None
    """The project reads go to when they name none. The account's own project, or the first project of the organisation a single sign-on session is limited to. Absent when the session reaches no project yet."""
    org_id: str | None = None
    """Set when the session came from an organisation's single sign-on. The session then reaches that organisation's projects only."""

@dataclass(**_SLOTS)
class Account:
    """The account a session belongs to.
    """

    email: str
    """The account's email address, as stored."""
    created_at: datetime
    """When the account was created."""
    email_verified: bool
    """Whether the address has been confirmed by a link sent to it."""
    totp_enabled: bool
    """Whether two-step log-in is on."""
    has_password: bool
    """Whether the account can log in with a password. False for an account created by single sign-on or SCIM that has never set one."""
    project: str | None = None
    """The project created with the account at sign-up. Absent for an account that was invited, or created by single sign-on or SCIM."""
    managed_by: str | None = None
    """The organisation that created this account through single sign-on or SCIM. That organisation can deactivate it."""

@dataclass(**_SLOTS)
class AccountKey:
    """One API key an account holds. The secret is not part of it: the daemon keeps only a digest.
    """

    id: str
    """The key's identifier, for revoking it. Not a secret."""
    scope: KeyScope
    """What the key may do."""
    project: str
    """The one project the key reaches."""
    prefix: str
    """The first characters of the secret, so you can tell which key a process holds without the daemon storing the secret."""
    created_at: datetime
    """When the key was created."""
    name: str | None = None
    """The label given when the key was created."""
    created_by: str | None = None
    """The email of the account that created the key. Set in an organisation's key list, so an admin can see whose key it is."""

@dataclass(**_SLOTS)
class CreateAccountKeyRequest:
    """A request for a new API key.
    """

    scope: KeyScope
    """What the key may do. One scope per key."""
    name: str | None = None
    """A label for the key, such as the service that will hold it. At most 100 characters."""
    project: str | None = None
    """The project the key reaches. Defaults to the account's own project. Your role on the project must allow the scope."""

@dataclass(**_SLOTS)
class CreatedAccountKey:
    """A new API key, with its secret. This is the only time the secret is shown.
    """

    key: AccountKey
    """The key as it is listed from now on."""
    secret: str
    """The key itself. Send it as `Authorization: Bearer <secret>`. Copy it now: the daemon keeps only its SHA-256 digest and cannot show it again."""

@dataclass(**_SLOTS)
class AccountNotice:
    """A plain answer that changes nothing you can read back, such as "check your email".
    """

    message: str
    """What happened, and what to do next."""

@dataclass(**_SLOTS)
class TokenRequest:
    """A one-time token from a link the daemon sent by email.
    """

    token: str
    """The token, as it appears in the link."""

@dataclass(**_SLOTS)
class PasswordForgotRequest:
    """A request for a password reset link.
    """

    email: str
    """The address to send the link to. The answer is the same whether or not an account uses it."""

@dataclass(**_SLOTS)
class PasswordResetRequest:
    """A new password, with the token from a reset link.
    """

    token: str
    """The token from the reset link. It works once, for one hour."""
    password: str
    """The new password. At least 10 characters and at most 1024 bytes."""
    totp_code: str | None = None
    """A code from the authenticator app, or a recovery code, when the account has two-step log-in on. A reset proves the address, not the second factor."""

@dataclass(**_SLOTS)
class PasswordChangeRequest:
    """A new password for the logged-in account.
    """

    new_password: str
    """The new password. At least 10 characters and at most 1024 bytes."""
    current_password: str | None = None
    """The password in use now. Required when the account has one."""

@dataclass(**_SLOTS)
class EmailChangeRequest:
    """A new email address for the logged-in account.
    """

    email: str
    """The new address. A link is sent to it, and the change happens when the link is opened."""
    password: str | None = None
    """The account's password. Required when the account has one."""

@dataclass(**_SLOTS)
class AccountDeleteRequest:
    """Confirmation for deleting the logged-in account.
    """

    password: str | None = None
    """The account's password. Required when the account has one."""
    totp_code: str | None = None
    """A code from the authenticator app. Required when two-step log-in is on."""

@dataclass(**_SLOTS)
class AccountDeletion:
    """What deleting an account removed.
    """

    orgs_deleted: int
    """Organisations that had this account as their only member, deleted with it."""
    projects_erased: list[str] = field(default_factory=list)
    """Projects whose stored signals and runs were erased, because their organisation was deleted."""

@dataclass(**_SLOTS)
class SessionInfo:
    """One live session of the logged-in account. The token is not part of it.
    """

    id: str
    """The session's identifier, for ending it. Not a secret."""
    created_at: datetime
    """When the session started."""
    expires_at: datetime
    """When it stops working."""
    current: bool
    """True for the session this request carries."""
    via: str
    """How it started: `password`, `sso`, `invite`, `verify` or `reset`."""
    org_id: str | None = None
    """The organisation a single sign-on session is limited to."""
    user_agent: str | None = None
    """The browser or client that started it, as it described itself."""
    address: str | None = None
    """The client address that started it."""

@dataclass(**_SLOTS)
class TotpSetup:
    """A new authenticator secret, not yet in use. Confirm it with a code to turn two-step log-in on.
    """

    secret: str
    """The secret in base32, for typing into an authenticator app."""
    otpauth_uri: str
    """The same secret as an `otpauth://` URI."""
    qr_svg: str
    """The URI as a QR code, an SVG document."""

@dataclass(**_SLOTS)
class TotpCodeRequest:
    """A code from an authenticator app.
    """

    code: str
    """The six-digit code."""

@dataclass(**_SLOTS)
class TotpEnabled:
    """Two-step log-in is on.
    """

    recovery_codes: list[str] = field(default_factory=list)
    """Ten single-use codes that stand in for the authenticator app. They are shown once: the daemon keeps only their digests."""

@dataclass(**_SLOTS)
class TotpDisableRequest:
    """Confirmation for turning two-step log-in off.
    """

    code: str
    """A code from the authenticator app, or a recovery code."""
    password: str | None = None
    """The account's password. Required when the account has one."""

@dataclass(**_SLOTS)
class Org:
    """An organisation: the owner of projects, members and single sign-on settings.
    """

    id: str
    """The organisation's identifier."""
    name: str
    """Its display name."""
    role: OrgRole
    """Your role in it."""
    created_at: datetime
    """When it was created."""
    members: int
    """How many members it has."""
    projects: int
    """How many projects it owns."""
    managed_by_config: bool
    """True when the daemon's configuration file declares this organisation. Its single sign-on settings are then read from that file and cannot be changed here."""
    quota_per_minute: int | None = None
    """The most signals all its projects together may store per minute. Absent means no ceiling."""
    retention_days: int | None = None
    """How long its projects keep data, in days. Absent means forever."""

@dataclass(**_SLOTS)
class CreateOrgRequest:
    """A new organisation.
    """

    name: str
    """Its display name, at most 100 characters."""

@dataclass(**_SLOTS)
class UpdateOrgRequest:
    """Changes to an organisation. A field left out keeps its value.
    """

    name: str | None = None
    """A new display name, at most 100 characters."""
    quota_per_minute: int | None = None
    """A ceiling on signals per minute across all the organisation's projects, at most the daemon's own ceiling. 0 restores the daemon's default. Owner only."""
    retention_days: int | None = None
    """How long to keep data, in days, at most the daemon's own window. 0 restores the daemon's default. Owner only."""

@dataclass(**_SLOTS)
class OrgDeletion:
    """What deleting an organisation removed.
    """

    projects_erased: list[str] = field(default_factory=list)
    """Projects whose stored signals and runs were erased."""

@dataclass(**_SLOTS)
class ProjectRoleGrant:
    """A role on one project.
    """

    project: str
    """The project."""
    role: ProjectRole
    """The role there."""

@dataclass(**_SLOTS)
class OrgMember:
    """One member of an organisation.
    """

    account_id: str
    """The member's account."""
    email: str
    """The member's email address."""
    role: OrgRole
    """The member's role."""
    via: str
    """How they joined: `created`, `invite`, `sso`, `scim` or `config`."""
    joined_at: datetime
    """When they joined."""
    managed: bool
    """True when this organisation created the account through single sign-on or SCIM."""
    totp_enabled: bool
    """Whether the member has two-step log-in on."""
    project_roles: list[ProjectRoleGrant] = field(default_factory=list)
    """Roles on single projects that replace the organisation role there."""

@dataclass(**_SLOTS)
class UpdateMemberRequest:
    """A new role for a member.
    """

    role: OrgRole
    """The role. Only an owner may grant or remove `owner`, and the last owner cannot be demoted."""

@dataclass(**_SLOTS)
class SetProjectRoleRequest:
    """A role for a member on one project.
    """

    project: str
    """A project the organisation owns."""
    role: ProjectRole | None = None
    """The role there. Absent removes the project role, so the organisation role applies again."""

@dataclass(**_SLOTS)
class OrgInvite:
    """An invitation not yet accepted. The token is not part of it.
    """

    id: str
    """The invitation's identifier, for revoking it."""
    email: str
    """The address it was sent to."""
    role: OrgRole
    """The role it grants."""
    created_at: datetime
    """When it was sent."""
    expires_at: datetime
    """When it stops working."""
    project_roles: list[ProjectRoleGrant] = field(default_factory=list)
    """Project roles it grants as well."""
    invited_by: str | None = None
    """The email of the member who sent it. Absent when the configuration file or SCIM sent it."""

@dataclass(**_SLOTS)
class CreateInviteRequest:
    """An invitation to an organisation.
    """

    email: str
    """Who to invite. The link is sent to this address."""
    role: OrgRole
    """The role to grant. Only an owner may invite an owner."""
    project_roles: list[ProjectRoleGrant] | None = None
    """Project roles to grant as well."""

@dataclass(**_SLOTS)
class InviteLookup:
    """What an invitation offers, read with its token before accepting it.
    """

    org_name: str
    """The organisation."""
    email: str
    """The address it was sent to. Accepting it joins the account with this address."""
    role: OrgRole
    """The role it grants."""
    account_exists: bool
    """Whether an account with this address exists. If it does, log in as it to accept; if not, accepting creates it."""
    expires_at: datetime
    """When it stops working."""

@dataclass(**_SLOTS)
class InviteAcceptRequest:
    """Acceptance of an invitation.
    """

    token: str
    """The token from the invitation link."""
    password: str | None = None
    """A password for the new account, when no account uses the invited address. Ignored otherwise."""

@dataclass(**_SLOTS)
class InviteAccepted:
    """An accepted invitation.
    """

    org: Org
    """The organisation joined."""
    session: AccountSession | None = None
    """A session for the account accepting created. Absent when an existing account accepted with its own session."""

@dataclass(**_SLOTS)
class OrgProject:
    """A project an organisation owns.
    """

    project: str
    """The project name. Random, so nobody can choose one that already holds somebody else's data."""
    created_at: datetime
    """When it was created or attached."""
    label: str | None = None
    """A display label."""

@dataclass(**_SLOTS)
class CreateProjectRequest:
    """A new project in an organisation.
    """

    label: str | None = None
    """A display label, at most 100 characters."""

@dataclass(**_SLOTS)
class OrgUsage:
    """How much an organisation is using, against its ceilings. Counted since the daemon started.
    """

    org_id: str
    """The organisation."""
    admitted_this_minute: int
    """Signals stored in the current one-minute window."""
    admitted_total: int
    """Signals stored since the daemon started."""
    refused_total: int
    """Signals refused for the ceiling since the daemon started."""
    members: int
    """Members."""
    projects: int
    """Projects."""
    keys: int
    """API keys on its projects."""
    since: datetime
    """When counting started."""
    quota_per_minute: int | None = None
    """Its ceiling on signals per minute, when it has one."""
    retention_days: int | None = None
    """How long its projects keep data, in days."""

@dataclass(**_SLOTS)
class SsoConfig:
    """An organisation's single sign-on settings. The client secret is not part of it.
    """

    provider: SsoProvider
    """The kind of provider."""
    client_id: str
    """The client id registered with the provider."""
    has_client_secret: bool
    """Whether a client secret is stored."""
    enforced: bool
    """When true, members whose email is in a claimed domain cannot log in with a password."""
    default_role: OrgRole
    """The role given to a member single sign-on creates, when no group maps to a role. Never `owner`."""
    redirect_url: str
    """The redirect URL to register with the provider."""
    managed_by_config: bool
    """True when these settings come from the configuration file."""
    updated_at: datetime
    """When they were last changed."""
    domains: list[str] = field(default_factory=list)
    """Email domains this organisation claims. Single sign-on creates accounts only for these domains, and "Log in with SSO" finds the organisation by them."""
    issuer: str | None = None
    """The OpenID Connect issuer URL. Absent for GitHub."""
    groups_claim: str | None = None
    """The ID token claim that lists the member's groups, mapped to roles by the group roles."""

@dataclass(**_SLOTS)
class PutSsoConfigRequest:
    """New single sign-on settings for an organisation.
    """

    provider: SsoProvider
    """The kind of provider."""
    client_id: str
    """The client id registered with the provider."""
    enforced: bool
    """Refuse password log-in for members in the claimed domains. Turning it on ends their password sessions."""
    default_role: OrgRole
    """The role for a member single sign-on creates. `admin`, `member` or `viewer`."""
    domains: list[str] = field(default_factory=list)
    """Email domains to claim. You can claim the domain of your own verified address. A domain belongs to one organisation."""
    issuer: str | None = None
    """The OpenID Connect issuer URL, such as `https://accounts.google.com`. Required for `oidc`. Must be `https`."""
    client_secret: str | None = None
    """The client secret. Left out, the stored one is kept."""
    groups_claim: str | None = None
    """The ID token claim that lists groups, such as `groups`."""

@dataclass(**_SLOTS)
class GroupRole:
    """A group from the identity provider, and the role its members get.
    """

    group: str
    """The group name, as single sign-on or SCIM sends it. Compared without regard to case."""
    role: OrgRole
    """The role. Never `owner`."""

@dataclass(**_SLOTS)
class ScimTokenInfo:
    """Whether an organisation has a SCIM token. The token is not part of it.
    """

    configured: bool
    """Whether a token exists."""
    base_url: str
    """The SCIM base URL to give your identity provider."""
    prefix: str | None = None
    """The first characters of the token."""
    created_at: datetime | None = None
    """When it was created."""

@dataclass(**_SLOTS)
class CreatedScimToken:
    """A new SCIM token. This is the only time it is shown, and it replaces any earlier token.
    """

    token: str
    """The bearer token for your identity provider. The daemon keeps only its SHA-256 digest."""
    prefix: str
    """Its first characters."""
    created_at: datetime
    """When it was created."""
    base_url: str
    """The SCIM base URL to give your identity provider."""

@dataclass(**_SLOTS)
class SsoStartRequest:
    """Where to start single sign-on. Give one of the two.
    """

    email: str | None = None
    """Your email address. Its domain finds the organisation."""
    org_id: str | None = None
    """The organisation, when you know it."""

@dataclass(**_SLOTS)
class SsoStart:
    """Where to send the browser for single sign-on.
    """

    authorize_url: str
    """The provider's authorization URL, with state, nonce and the PKCE challenge in it."""
    binding: str
    """A secret to keep in this browser and send back with the callback. It ties the callback to the browser that started it, so a link from somebody else cannot log you in as them."""
    org_name: str
    """The organisation."""

@dataclass(**_SLOTS)
class SsoCallbackRequest:
    """What the provider sent back to the redirect URL.
    """

    state: str
    """The `state` parameter."""
    code: str
    """The `code` parameter."""
    binding: str
    """The binding from the start of the flow."""

@dataclass(**_SLOTS)
class CreatePromptRequest:
    """A new version of a prompt. Posting a body identical to the latest version stores nothing new and returns that version, so a deploy that posts its prompts on every start is a no-op when nothing changed.
    """

    name: str
    """The prompt's name. A new name creates the prompt at version 1."""
    kind: PromptKind | None = None
    """`text` or `chat`. Defaults to `chat` when `messages` is set, otherwise `text`."""
    template: str | None = None
    """The template, for a `text` prompt."""
    messages: Any | None = None
    """The messages, for a `chat` prompt."""
    config: Any | None = None
    """Model settings to store with it."""
    note: str | None = None
    """What changed."""
    labels: list[str] | None = None
    """Labels to point at the stored version once it is stored, such as `staging`."""

@dataclass(**_SLOTS)
class PromptLabelRequest:
    """Which version a label should point at.
    """

    version: int
    """The version. It must exist."""

@dataclass(**_SLOTS)
class PromptVersionsResponse:
    """Every version of one prompt, newest first.
    """

    name: str
    """The prompt's name."""
    versions: list[Prompt] = field(default_factory=list)
    """Its versions, newest first, each with the labels that point at it."""

@dataclass(**_SLOTS)
class CreateScorerDefinitionRequest:
    """A new version of a scorer definition. A body identical to the latest version is not stored again.
    """

    id: str
    """The definition's id. A new id creates it at version 1."""
    kind: ScorerDefinitionKind
    """`judge` or `code`."""
    description: str
    """What it measures, in a sentence."""
    direction: ScoreDirection | None = None
    """Which way is better. Defaults to `higher_is_better`."""
    prompt: str | None = None
    """For `judge`: the criterion."""
    builtin: str | None = None
    """For `code`: the code scorer to run."""
    config: ScorerConfig | None = None
    """For `code`: its settings."""
    note: str | None = None
    """What changed."""

@dataclass(**_SLOTS)
class ScorerDefinitionVersions:
    """Every version of one scorer definition, newest first.
    """

    id: str
    """The definition's id."""
    versions: list[ScorerDefinition] = field(default_factory=list)
    """Its versions, newest first."""

@dataclass(**_SLOTS)
class CreateEvaluationRuleRequest:
    """A new version of a live scoring rule. Posting the same body as the latest version stores nothing new.
    """

    id: str
    """The rule's id: lower-case letters, digits, `-` and `_`, up to 64."""
    scorers: list[str] = field(default_factory=list)
    """The scorers to run. At least one."""
    enabled: bool | None = None
    """Whether it runs. Defaults to `false`, so a rule is read back and checked before it spends anything."""
    sample_rate: float | None = None
    """The share of matching runs to score, from 0 to 1. Defaults to 1."""
    name: str | None = None
    """Only runs whose root step has this name."""
    kind: SpanKind | None = None
    """Only runs whose root step is this kind."""
    environment: str | None = None
    """Only runs from this environment."""
    tags: list[str] | None = None
    """Only runs carrying all of these tags."""
    status: SpanStatus | None = None
    """Only runs with this outcome."""
    config: ScorerConfig | None = None
    """Settings for the code scorers that need them."""
    max_judged_per_minute: int | None = None
    """The most judged scores per minute. Defaults to 30."""
    note: str | None = None
    """What changed."""

@dataclass(**_SLOTS)
class TrajectoryStep:
    """One action an agent took: a tool call, a model generation or a handoff, in the order they started.
    """

    index: int
    """Its position in the trajectory, from 0."""
    span_id: str
    """The span it was read from."""
    kind: SpanKind
    """What kind of action."""
    name: str
    """The tool, model step or agent it names."""
    status: SpanStatus
    """How it ended."""
    start_offset_ms: float
    """When it started, in milliseconds after the run started."""
    duration_ms: float | None = None
    """How long it took. Absent while it is still running."""
    model: str | None = None
    """The model, for a model generation."""
    input_preview: str | None = None
    """The start of its input as plain text, up to 240 characters."""
    output_preview: str | None = None
    """The start of its output as plain text, up to 240 characters."""
    tokens: int | None = None
    """Input and output tokens together, for a model generation."""

@dataclass(**_SLOTS)
class Trajectory:
    """A run read as the sequence of actions its agent took. This is what a trajectory is scored on: which tools were called, in what order, how many steps it took, and whether it ended with an answer.
    """

    trace_id: str
    """The run."""
    step_count: int
    """How many steps."""
    tool_count: int
    """How many of them were tool calls."""
    error_count: int
    """How many of them failed."""
    goal_reached: bool
    """True when the run ended `ok` with a non-empty answer."""
    steps: list[TrajectoryStep] = field(default_factory=list)
    """Every action, in start order."""
    tool_sequence: list[str] = field(default_factory=list)
    """The names of the tool calls, in order. What `expected_tools` is compared with."""
    final_output_preview: str | None = None
    """The start of the run's answer, from the root step's output."""

@dataclass(**_SLOTS)
class ExperimentScoreRequest:
    """Which scorers to run over every run recorded under an experiment.
    """

    scorers: list[str] = field(default_factory=list)
    """The scorers, as `POST /v1/api/score` takes them. Empty runs the code scorers that need no settings."""
    dry_run: bool = False
    """Store nothing; report what would be stored."""
    config: ScorerConfig | None = None
    """Settings for the code scorers. `expected` is filled per run from the dataset case the run answered, when the run names one."""

@dataclass(**_SLOTS)
class ExperimentItemResult:
    """One case of an experiment: the run that answered it and how each scorer marked it.
    """

    trace_id: str
    """The run that answered the case."""
    status: SpanStatus
    """How the run ended."""
    scores: Any
    """The mean value per scorer for this run, keyed by scorer id."""
    item_id: str | None = None
    """The dataset case, when the run named one in its `mcpd.dataset_item_id` attribute."""
    duration_ns: int | None = None
    """How long the run took."""
    cost: float | None = None
    """What the run cost."""
    input_preview: str | None = None
    """The start of the run's input."""
    output_preview: str | None = None
    """The start of the run's answer."""
    expected: Any | None = None
    """The case's expected answer, when it has one."""

@dataclass(**_SLOTS)
class ScorerSummary:
    """One scorer's marks across an experiment.
    """

    scorer_id: str
    """The scorer."""
    n: int
    """How many scores."""
    mean: float
    """Their mean."""
    min: float
    """The lowest."""
    max: float
    """The highest."""
    direction: ScoreDirection
    """Which way is better."""

@dataclass(**_SLOTS)
class ExperimentResults:
    """An experiment case by case, with a summary per scorer. Two of these joined on `item_id` are the per-case comparison of two versions.
    """

    experiment_id: str
    """The experiment."""
    runs: int
    """How many runs are recorded under it."""
    failed: int
    """How many of them ended in an error."""
    items: list[ExperimentItemResult] = field(default_factory=list)
    """One row per run recorded under the experiment, oldest first."""
    scorers: list[ScorerSummary] = field(default_factory=list)
    """Per scorer, across every run."""

@dataclass(**_SLOTS)
class CreateReviewItemRequest:
    """A run to queue for review.
    """

    trace_id: str
    """The run."""
    span_id: str | None = None
    """The step, when it is one step."""
    feedback_id: str | None = None
    """The feedback that prompted it."""
    assignee: str | None = None
    """Who should review it."""
    note: str | None = None
    """Why it is queued."""

@dataclass(**_SLOTS)
class UpdateReviewItemRequest:
    """A change to a review item. Absent fields keep their value.
    """

    status: ReviewStatus | None = None
    """The new status."""
    assignee: str | None = None
    """The new assignee. An empty string unassigns it."""
    note: str | None = None
    """What the reviewer found."""

@dataclass(**_SLOTS)
class ReviewQueueResponse:
    """The review queue, newest first.
    """

    pending: int
    """How many items in the project are still pending, whatever the filter."""
    items: list[ReviewItem] = field(default_factory=list)
    """The items that matched, newest change first."""

@dataclass(**_SLOTS)
class DatasetVersion:
    """A frozen copy of a dataset's cases. An experiment that names a version runs against exactly those cases, whatever has been added to the dataset since.
    """

    dataset_id: str
    """The dataset."""
    version: int
    """Its number, from 1."""
    item_count: int
    """How many cases it froze."""
    created_at: datetime
    """When it was frozen."""
    note: str | None = None
    """What changed since the version before."""
    created_by: str | None = None
    """Which key or account froze it."""

@dataclass(**_SLOTS)
class CreateDatasetVersionRequest:
    """Freeze the dataset's current cases as a new version.
    """

    note: str | None = None
    """What changed since the version before."""

@dataclass(**_SLOTS)
class Span:
    """One unit of work. Spans are the backbone of mcpd: logs, metrics, scores and feedback all hang off a span or a trace.

    Stored in the `spans` table.
    """

    trace_id: str
    """128-bit trace id, 32 lowercase hex characters. OpenTelemetry-compatible."""
    span_id: str
    """64-bit span id, 16 lowercase hex characters. Unique within the trace."""
    name: str
    """What this operation is called. Keep it low-cardinality: `search_docs`, not `search_docs(query=hello)`."""
    kind: SpanKind
    """What kind of operation this is."""
    status: SpanStatus
    """The outcome."""
    start_time: datetime
    """When the operation began, in UTC with nanosecond precision."""
    is_root: bool
    """True when this span has no parent inside the trace. Materialised so the trace list never scans children."""
    is_event: bool
    """True for a point in time rather than a duration. Event spans have no end."""
    correlation: Correlation
    """Which project, environment, session, thread and user this belongs to."""
    received_at: datetime
    """When mcpd accepted this span. Differs from `start_time` for buffered or replayed data, and is what retention is measured against."""
    parent_span_id: str | None = None
    """The parent span within this trace. Absent on the root."""
    external_parent_span_id: str | None = None
    """A parent from a foreign tracing system that mcpd did not create. Carried for correlation, never used for mcpd's own tree."""
    end_time: datetime | None = None
    """When it finished. Absent while the span is still running."""
    duration_ns: int | None = None
    """Derived from the timestamps at ingest, stored so the database never has to compute it to sort."""
    entity_kind: EntityKind | None = None
    """The kind of thing that produced this span."""
    entity_id: str | None = None
    """Stable identifier of the producing entity."""
    entity_name: str | None = None
    """Display name of the producing entity."""
    input: Any | None = None
    """What went in. For a model call this is the full prompt, and it is the single most valuable field mcpd stores."""
    output: Any | None = None
    """What came out. For a model call this is the completion."""
    attributes: Any | None = None
    """Kind-specific structured detail. See the attribute reference for what each kind carries."""
    metadata: Any | None = None
    """Anything you want to attach. mcpd never interprets this."""
    usage: TokenUsage | None = None
    """Token counts, on model and embedding spans."""
    cost: Cost | None = None
    """Computed cost, on spans that consumed tokens."""
    model_parameters: ModelParameters | None = None
    """How the model was called: the sampling settings the caller chose. Absent on a span that made no model call, and on one whose producer did not record them."""
    error: SpanError | None = None
    """Failure detail, when `status` is `error`."""
    links: list[SpanLink] | None = None
    """Causal references to other spans or traces."""
    tags: list[str] | None = None
    """Free labels for filtering. Only meaningful on a root span, where they label the whole trace."""
    model: str | None = None
    """Model identifier, denormalised out of `attributes` so it can be indexed and grouped."""
    provider: str | None = None
    """Model provider, denormalised for the same reason."""
    ingest_key: str | None = None
    """Producer-supplied idempotency key. A retried batch with the same key is accepted once."""

    TABLE = "spans"

@dataclass(**_SLOTS)
class Trace:
    """A whole run, projected from its spans. mcpd materialises this at ingest so the list view is one indexed read rather than an aggregation over every span. One per project and trace id: a trace id crosses service boundaries, so two projects that call each other each hold their own run under it, built from their own spans.

    Stored in the `traces` table.
    """

    trace_id: str
    """The trace this summarises."""
    name: str
    """The root span's name."""
    kind: SpanKind
    """The root span's kind."""
    status: SpanStatus
    """`error` when any span in the trace failed, otherwise the root's status."""
    start_time: datetime
    """Earliest start across the trace."""
    span_count: int
    """How many spans have arrived."""
    error_count: int
    """How many of them failed."""
    model_count: int
    """How many model calls the run made."""
    tool_count: int
    """How many tool calls the run made."""
    correlation: Correlation
    """Context inherited from the root span."""
    is_complete: bool
    """True once the root span has ended and no span is still open."""
    received_at: datetime
    """When the first span of this trace arrived."""
    root_span_id: str | None = None
    """The root span, when one has arrived."""
    end_time: datetime | None = None
    """Latest end across the trace. Absent while anything is still running."""
    duration_ns: int | None = None
    """Wall-clock span of the whole trace."""
    usage: TokenUsage | None = None
    """Token usage summed across every model span in the trace."""
    cost: Cost | None = None
    """Cost summed across the trace. This is the number a bill is reconciled against."""
    input_preview: str | None = None
    """The start of the root step's input as plain text, up to 240 characters, for a list column. For a chat input it is the last user message. Filled by the trace list when it is asked with `preview=true`; the full input is on the span."""
    output_preview: str | None = None
    """The start of the root step's output as plain text, up to 240 characters. Filled by the trace list when it is asked with `preview=true`; the full output is on the span."""
    tags: list[str] | None = None
    """Tags from the root span."""
    score_summary: Any | None = None
    """Mean score per scorer id, maintained as scores arrive, so the list can sort by quality without a join."""
    feedback_summary: Any | None = None
    """Counts per feedback kind and value."""

    TABLE = "traces"

@dataclass(**_SLOTS)
class LogRecord:
    """A structured log line, correlated to the span that produced it. Logs are stored beside traces so 'show me the logs for this run' is one query, not a context switch to another tool.

    Stored in the `logs` table.
    """

    log_id: str
    """Unique id for this record."""
    timestamp: datetime
    """When the line was emitted."""
    level: LogLevel
    """Severity."""
    message: str
    """The rendered message."""
    correlation: Correlation
    """Project, environment, session and user."""
    received_at: datetime
    """When mcpd accepted it."""
    trace_id: str | None = None
    """The trace this line belongs to, when it was emitted inside one."""
    span_id: str | None = None
    """The span this line belongs to."""
    fields: Any | None = None
    """Structured key/value context. Redacted like any payload."""
    logger: str | None = None
    """Which logger emitted it."""

    TABLE = "logs"

@dataclass(**_SLOTS)
class MetricPoint:
    """One numeric observation. mcpd aggregates these into time buckets on read, and keeps raw points only as long as the raw retention class allows.

    Stored in the `metrics` table.
    """

    metric_id: str
    """Unique id for this observation."""
    name: str
    """Metric name, such as `mcpd.model.duration`."""
    kind: MetricKind
    """How it aggregates."""
    value: float
    """The observed value."""
    timestamp: datetime
    """When it was observed."""
    correlation: Correlation
    """Project and environment."""
    received_at: datetime
    """When mcpd accepted it."""
    unit: str | None = None
    """UCUM unit, such as `ms`, `By` or `{token}`."""
    labels: Any | None = None
    """Dimensions. Passed through the cardinality filter, which rejects unbounded keys."""
    buckets: Any | None = None
    """Histogram bucket boundaries and counts, on histogram points."""
    trace_id: str | None = None
    """Exemplar link: the trace that produced this observation."""
    span_id: str | None = None
    """Exemplar link: the span that produced it. This is what turns a spike on a chart into a specific run you can open."""

    TABLE = "metrics"

@dataclass(**_SLOTS)
class Score:
    """A machine evaluation of a span or a trace.

    Stored in the `scores` table.
    """

    score_id: str
    """Unique id."""
    scorer_id: str
    """Which scorer produced this, such as `answer_relevance`."""
    source: ScoreSource
    """Live, offline, experiment or manual."""
    value: float
    """The numeric result."""
    timestamp: datetime
    """When the score was produced."""
    correlation: Correlation
    """Inherited from the target."""
    trace_id: str | None = None
    """The scored trace."""
    span_id: str | None = None
    """The scored span, when the score is about one step rather than the whole run."""
    scorer_version: str | None = None
    """The scorer's version. A score is only comparable to another score from the same version."""
    min: float | None = 0
    """Lower bound of the scale. Without it, a bare number is not interpretable."""
    max: float | None = 1
    """Upper bound of the scale."""
    label: str | None = None
    """A categorical result, when the scorer produces one alongside the number."""
    reason: str | None = None
    """Why. For an LLM judge this is the judge's explanation, and it is what makes a score arguable rather than magic."""
    score_trace_id: str | None = None
    """The trace of the scoring run itself, so a suspicious score can be debugged like any other run."""
    cost: Cost | None = None
    """What producing this score cost. Judged evaluation is not free and mcpd refuses to hide that."""
    metadata: Any | None = None
    """Anything else."""

    TABLE = "scores"

@dataclass(**_SLOTS)
class Feedback:
    """A human evaluation. Kept distinct from `Score` because the two answer different questions and must never be averaged together.

    Stored in the `feedback` table.
    """

    feedback_id: str
    """Unique id."""
    kind: FeedbackKind
    """Thumbs, rating, correction, tag or comment."""
    value: Any
    """The judgement. Its shape follows `kind`."""
    timestamp: datetime
    """When it was given."""
    correlation: Correlation
    """Inherited from the target."""
    trace_id: str | None = None
    """The trace being judged."""
    span_id: str | None = None
    """The span being judged."""
    comment: str | None = None
    """Free prose the person added."""
    author_id: str | None = None
    """Who gave it. Hashed by default."""
    author_kind: str | None = None
    """Whether the author was an end user, a reviewer or an operator."""
    metadata: Any | None = None
    """Anything else."""

    TABLE = "feedback"

@dataclass(**_SLOTS)
class Project:
    """The tenancy boundary. Every signal belongs to exactly one project; retention, redaction, sampling, budgets and access are all configured per project.

    Stored in the `projects` table.
    """

    project_id: str
    """Stable identifier, used in API paths and ingest keys."""
    name: str
    """Display name."""
    created_at: datetime
    """When it was created."""
    retention_days: int | None = None
    """How long to keep signals. Absent means the server default."""
    sampling: Any | None = None
    """The sampling policy for this project."""
    redaction: Any | None = None
    """Extra redaction rules, applied on top of the server defaults."""
    budget: Any | None = None
    """Cost budget and what happens when it is exceeded."""
    metadata: Any | None = None
    """Anything else."""

    TABLE = "projects"

@dataclass(**_SLOTS)
class Dataset:
    """A fixed collection of inputs, with optional expected outputs, that an experiment runs against. Datasets are how a change is proved not to regress.

    Stored in the `datasets` table.
    """

    dataset_id: str
    """Stable identifier."""
    name: str
    """Display name."""
    item_count: int
    """How many items it holds."""
    created_at: datetime
    """When it was created."""
    correlation: Correlation
    """Which project owns it."""
    description: str | None = None
    """What this dataset is for."""

    TABLE = "datasets"

@dataclass(**_SLOTS)
class DatasetItem:
    """One case in a dataset.

    Stored in the `dataset_items` table.
    """

    item_id: str
    """Stable identifier."""
    dataset_id: str
    """Which dataset this belongs to."""
    input: Any
    """What to feed the system under test."""
    expected: Any | None = None
    """The reference answer, when there is one. Absent for open-ended cases judged by a scorer."""
    metadata: Any | None = None
    """Anything else, such as a difficulty label or a source citation."""
    source_trace_id: str | None = None
    """The production trace this case was captured from, when it was. This is how a bug report becomes a permanent regression test."""

    TABLE = "dataset_items"

@dataclass(**_SLOTS)
class Experiment:
    """One evaluation run of a system against a dataset. Comparing two experiments is how a change is accepted or rejected.

    Stored in the `experiments` table.
    """

    experiment_id: str
    """Stable identifier."""
    name: str
    """Display name."""
    dataset_id: str
    """What it ran against."""
    status: ExperimentStatus
    """Where it got to."""
    item_count: int
    """Cases attempted."""
    correlation: Correlation
    """Which project owns it."""
    started_at: datetime | None = None
    """When it began."""
    finished_at: datetime | None = None
    """When it ended."""
    summary: Any | None = None
    """Aggregate score per scorer, with mean, standard deviation and count."""
    baseline_id: str | None = None
    """The experiment this one is compared against."""
    cost: Cost | None = None
    """What the whole run cost."""
    error: str | None = None
    """Why it failed, when it did."""
    metadata: Any | None = None
    """The configuration under test: model, prompt version, git sha."""

    TABLE = "experiments"

@dataclass(**_SLOTS)
class AuditEvent:
    """One recorded attempt to read stored payloads, or to delete them. Written by the handlers that hand prompts and completions back out of mcpd, so the question "who read this run" has an answer. The audit log is not trimmed by the retention pass: a log that deleted itself on the same schedule as the data it describes could not say who read the data that has since gone.

    Stored in the `audit_log` table.
    """

    audit_id: str
    """Unique id."""
    timestamp: datetime
    """When the request was handled."""
    principal: str
    """Which key acted, as a short hex digest of it. The key itself is never written here. `anonymous` when the request carried no key, which is what an unprotected daemon looks like."""
    subject: str
    """The end identity a proxy asserted for this request. Always empty in this build. The field exists from the first release so that proxy identity can be added later without a migration."""
    action: str
    """What was done, named as the endpoint id: `get_trace`, `list_spans`, `delete_trace`."""
    project: str
    """The project the request was made against."""
    result: AuditResult
    """Whether authorisation allowed the request or refused it."""
    resource: str | None = None
    """The trace or dataset the request named, when it named one."""

    TABLE = "audit_log"

@dataclass(**_SLOTS)
class Definition:
    """A stored definition. Immutable per version: a run pins the version it started against, and that version never changes underneath it.

    Stored in the `definitions` table.
    """

    project: str
    """Which project owns it."""
    id: str
    """Its identifier within the project."""
    version: int
    """Its version. A run started without one pins the highest stored version at that moment."""
    kind: DefinitionKind
    """What kind of definition it is."""
    body: Any
    """The document as stored, minus `version`."""
    created_at: datetime
    """When this version was stored."""

    TABLE = "definitions"

@dataclass(**_SLOTS)
class Run:
    """One execution of one definition version. The daemon owns the run between steps: its journal, its deadlines, its waits, its cancel and its trace. Your own process owns everything inside a step.

    Stored in the `runs` table.
    """

    run_id: str
    """Its identifier, minted at start."""
    project: str
    """Which project owns it."""
    definition_id: str
    """Which definition it runs."""
    definition_version: int
    """Which version. Pinned at start and never re-resolved."""
    status: RunStatus
    """Where it is."""
    cursor: RunCursor
    """The frontier: where in the graph the run has reached."""
    correlation: Correlation
    """The context stored at start and copied onto every span the daemon writes for the run, with `run_id` set to this run's id."""
    trace_id: str
    """The trace every span of this run belongs to. 32 lowercase hex characters, minted at start."""
    steps_used: int
    """How many steps this run has completed. Counted against `max_steps` on the root of the run tree."""
    cost_usd: float
    """What the run's steps have cost so far, priced from the usage workers reported."""
    root_run_id: str
    """The root of the run tree, which ceilings are enforced against. Equal to `run_id` on a root."""
    depth: int
    """How many parents lie between this run and the root. Zero on a root."""
    created_at: datetime
    """When it started."""
    updated_at: datetime
    """When it last moved."""
    input: Any | None = None
    """What it was started with, after validation against the definition's `input_schema`."""
    output: Any | None = None
    """What the definition's `output` template rendered when the run reached the end of the graph."""
    state: Any | None = None
    """A JSON object merged shallowly on every `complete` that sends one. Later steps read it through the lease."""
    error: str | None = None
    """Why it failed, when it did: the failing step's message, or the ceiling it spent."""
    max_steps: int | None = None
    """The step ceiling from the definition's `limits`, when it declares one. On a root it bounds the whole tree."""
    max_cost_usd: float | None = None
    """The spend ceiling from `limits`, when declared. A claim refused for a spent ceiling ends the run `failed`."""
    deadline_at: datetime | None = None
    """When `limits.run_timeout_ms` passes. Absent when the definition declares no timeout, and once the run has ended."""
    idempotency_key: str | None = None
    """The key the start request carried, when it carried one. A repost of it within 24 hours returns this run."""
    parent_run_id: str | None = None
    """The run whose `subworkflow` step started this one."""
    parent_step_key: str | None = None
    """The step in the parent that this run is the body of."""
    ended_at: datetime | None = None
    """When it reached a terminal status."""

    TABLE = "runs"

@dataclass(**_SLOTS)
class StepAttempt:
    """One attempt at one step of a run. A run's journal is these rows, append only: a retry is a new row, never a rewrite of the old one.

    Stored in the `step_attempts` table.
    """

    run_id: str
    """Which run."""
    step_key: str
    """Which step, with any enclosing branch, item or iteration folded into the key: `refund/issue`, `classify[3]`, `poll[7]/classify`."""
    attempt: int
    """Which attempt this is. The fence: a lease for an earlier attempt is refused once a later one exists."""
    kind: StepKind
    """What kind of step."""
    effect: Effect
    """What a duplicated body would cost."""
    status: AttemptStatus
    """Where the attempt is."""
    span_id: str
    """The span the daemon opened for this attempt. 16 lowercase hex characters, minted when the row was written. A worker's own spans nest under it."""
    created_at: datetime
    """When the row was written."""
    input: Any | None = None
    """The rendered input handed to the worker, on a claimable step."""
    output: Any | None = None
    """What the worker returned on `complete`, or the event payload on a resumed wait."""
    error: StepError | None = None
    """Why it failed, when it did."""
    usage: StepUsage | None = None
    """The token usage the worker reported on `complete`."""
    cost_usd: float | None = None
    """What the daemon priced that usage at."""
    lease_owner: str | None = None
    """The `worker_id` holding the lease, while one does."""
    lease_until: datetime | None = None
    """When the lease lapses without a heartbeat."""
    started_at: datetime | None = None
    """When the worker posted `started`. Absent when it never did, which is what the reclaim rule reads."""
    orphaned_at: datetime | None = None
    """When the lease lapsed."""
    reclaim_until: datetime | None = None
    """When the reclaim window closes."""
    attempts_left: int | None = None
    """How many attempts the node's retry budget still allows after this one."""
    ended_at: datetime | None = None
    """When it reached a terminal status."""

    TABLE = "step_attempts"

@dataclass(**_SLOTS)
class RunEvent:
    """One row of a run's event log. Written in the transaction that made the change it describes, with `seq` allocated from the run header, so a reader that has seen a row has seen every row below it.

    Stored in the `run_events` table.
    """

    run_id: str
    """Which run."""
    seq: int
    """Its position in this run's stream, from zero and without gaps. Pass it as `after` to reattach."""
    kind: RunEventKind
    """What happened."""
    at: datetime
    """When it was written."""
    payload: Any | None = None
    """The detail, shaped by `kind`."""

    TABLE = "run_events"

@dataclass(**_SLOTS)
class Aspect:
    """An axis that traces are described along. An aspect is a definition and not a measurement: it carries the name a note is filed under, the words an operator reads on screen and, where a model writes the sentence, the instruction handed to whatever produces it. Four ship with the daemon as ordinary rows rather than as a list compiled into a client, so there is one source of truth for what exists: goal, what the user wanted; sentiment, the tone they expressed; behaviour, what the agent did about it; and outcome, how the interaction ended. A stored version never changes, so a note recorded a month ago still names the definition it was written against.

    Stored in the `aspects` table.
    """

    project: str
    """Which project owns it. Aspects are project-scoped, like every other tenancy boundary here."""
    name: str
    """The aspect's name, lowercase with underscores, such as `goal` or `refund_reason`. This is what a note is filed under and what every query parameter names."""
    label: str
    """The display name, such as `Goal`. Stored rather than derived from the name, so an aspect reads the same in every screen and every language of SDK."""
    description: str
    """One line saying what this axis is about, shown beside the label so a reader does not have to infer the axis from its name."""
    derived: bool
    """Whether the daemon writes this aspect's notes itself, with no model anywhere. `outcome` ships derived, written from the run's status, the step error class, the silent producer flag on the cause report and the divergence watch, so a deployment with nothing configured still fills one aspect and can see the whole pipeline work before a model is wired to anything."""
    built_in: bool
    """Whether the daemon seeded this aspect. A built-in is a row like any other and can be switched off, but it cannot be deleted, because notes already name it."""
    enabled: bool
    """Whether notes are written along this aspect in this project. It is configuration and read-only here. Switching an aspect on affects notes recorded afterwards and never rewrites what is stored."""
    sort_order: int
    """Where this aspect sits in every list and in the default column order, ascending. Stored, so the order an operator learns is the order they keep."""
    version: int
    """This definition's version. Editing an aspect stores a new version rather than changing this one, so analysis already recorded is never rewritten under a reader."""
    created_at: datetime
    """When this version was stored."""
    instruction: str | None = None
    """The text handed to whatever writes a note along this aspect, at most 2000 bytes. The daemon stores it and hands it out; what reads it and writes the sentence sits outside the daemon. Absent when `derived` is true, because a derived aspect needs no instruction."""

    TABLE = "aspects"

@dataclass(**_SLOTS)
class AspectNote:
    """One sentence recorded against one trace along one aspect. A note proves one thing: that this sentence was recorded against that trace by the definition named in `aspect`, at the version in `definition_version`. It does not prove the sentence is true. Nothing here is checked against the trace it describes, so a note is evidence of what a definition said and not evidence of what happened, and the trace is what settles the difference. The daemon also keeps a small sketch of the wording, which is what clustering compares, and that sketch is deliberately not a field on this entity: it is a detail of how the pass is stored, a caller can do nothing with it, and publishing it would fix the hashing where it stands today. The text is payload. It passes through the same redactor as every other signal on the way in, retention sweeps it, and erasing a subject erases it.

    Stored in the `aspect_notes` table.
    """

    note_id: str
    """Unique id of the note. This identifies the sentence and never the run: a note id and a trace id are different things and conflating them is the mistake this separate name exists to prevent."""
    trace_id: str
    """The trace this sentence is about."""
    aspect: str
    """Which aspect this note is filed under, matching `Aspect.name`."""
    text: str
    """The sentence, after redaction. It is the only text the read API returns for a note, and it is what the clustering pass reads."""
    definition_version: int
    """The version of the aspect definition this note was written against. Editing a definition mints a new version, so an older note keeps naming what was actually asked for at the time."""
    created_at: datetime
    """When the note was recorded. Writing a note lags the trace it describes, sometimes by a long way, so a trace can be visible for hours before it carries one."""
    correlation: Correlation
    """Inherited from the trace, so a note is scoped, filtered, retained and erased exactly as its trace is."""
    model: str | None = None
    """Which model wrote the sentence, when one did. Absent for a derived aspect, where the daemon wrote the note itself."""

    TABLE = "aspect_notes"

@dataclass(**_SLOTS)
class ThemeSnapshot:
    """One clustering pass over one entity, covering a moving window of notes. A snapshot is what turns a clustering into a series: every theme belongs to exactly one, and every theme query names one. Windows overlap, so counts from two snapshots must never be added together. A trace whose note falls inside three windows is counted once in each of them, and summing the three counts it three times. Compare snapshots or read one; do not sum them. Reads never recompute either: a pass writes a snapshot on its own timer and every read is a lookup of what it wrote, so refreshing a screen shows the same numbers rather than a fresh clustering.

    Stored in the `theme_snapshots` table.
    """

    snapshot_id: str
    """Opaque id, echoed back unchanged. Never build one by hand and never read meaning into its shape."""
    project: str
    """Which project this pass covered."""
    entity_kind: EntityKind
    """The kind of thing this pass covered."""
    entity_name: str
    """The name of that thing. An entity is the pair of a kind and a name, which spans already carry, so nothing new is assigned to identify one."""
    ordinal: int
    """Where this snapshot falls in the entity's series, counting from one at the oldest snapshot still stored."""
    cutoff_at: datetime
    """When this snapshot became the current state. A timeline is ordered by this and not by the window bounds, because two passes can cover overlapping windows."""
    window_from: datetime
    """Start of the window of notes read, inclusive."""
    window_to: datetime
    """End of the window of notes read, exclusive."""
    trace_count: int
    """How many distinct traces the notes read belong to."""
    note_count: int
    """How many notes the pass actually read. Every coverage figure in this snapshot is a share of this number and never of the number of notes in the window, which is what makes `capped` worth reading."""
    capped: bool
    """Whether the pass reached its ceiling of 20,000 notes and clustered the newest of them rather than all of them. True means the window holds more notes than were read, so the denominator of every share here is `note_count` and not the window."""
    aspects: list[str] = field(default_factory=list)
    """The aspects this pass covered, in catalogue order. An aspect with too few notes when the pass ran is absent, so a missing name means it is still collecting and not that anything failed."""

    TABLE = "theme_snapshots"

@dataclass(**_SLOTS)
class Theme:
    """A cluster of aspect notes that share their wording, in one snapshot along one aspect. A theme groups aspect notes that share their wording, which is why mcpd needs no model and no embedding to build one. It does not read the sentences, so two notes that mean the same thing in different words land in separate themes, and a theme's label is the note most like the others rather than a summary written for it. The clustering is a pure function of the notes it reads, with no floating-point arithmetic in the comparison, so two replicas of one deployment reading the same notes write the same themes and cannot disagree about where a boundary falls.

    Stored in the `themes` table.
    """

    theme_id: str
    """Opaque id, stable across snapshots, which is what makes a history possible. A theme keeps its id while its wording holds, so the same id in two snapshots is the same theme. Nothing is encoded in the shape of it."""
    snapshot_id: str
    """The snapshot this theme belongs to."""
    aspect: str
    """Which aspect it was clustered along. Clustering runs once per aspect, so one trace can sit in a different theme in each of them."""
    entity_kind: EntityKind
    """The kind of entity this theme was built for."""
    entity_name: str
    """The name of that entity."""
    label: str
    """The theme's label, which is the text of one of its own notes: the note most like the rest of them. Nothing wrote it for the screen, so it reads as one real sentence about one real trace rather than as a description of the group, and it can change between snapshots as the group does."""
    label_note_id: str
    """The note the label is taken from."""
    label_trace_id: str
    """The trace that note was written about, so a reader can open the run behind the label instead of taking the label's word for it."""
    state: ThemeState
    """What happened to this theme between the previous snapshot and this one."""
    note_count: int
    """How many notes are in this theme."""
    trace_count: int
    """How many distinct traces those notes were written about."""
    coverage: float
    """This theme's share of the notes the snapshot read for this aspect, between 0 and 1. The denominator is the snapshot's `note_count`, so in a capped snapshot it is a share of what was read."""

    TABLE = "themes"

@dataclass(**_SLOTS)
class Prompt:
    """One version of a named prompt. A version never changes once stored: a new body is a new version, and labels such as `production` and `staging` point at the version a caller should use. SDKs fetch a prompt by name and label, render it with variables, and link the spans that used it, so a trace shows which version produced an answer.

    Stored in the `documents` table.
    """

    project: str
    """Which project owns it."""
    name: str
    """The prompt's name, unique within the project. Letters, digits, `-`, `_`, `.` and `/`, up to 128 characters."""
    version: int
    """Its version, from 1, assigned by the daemon in the order versions arrive."""
    kind: PromptKind
    """Whether the body is one template or a list of messages."""
    created_at: datetime
    """When this version was stored."""
    variables: list[str] = field(default_factory=list)
    """Every variable the body names, in order of first use, without duplicates."""
    labels: list[str] = field(default_factory=list)
    """The labels that point at this version now."""
    template: str | None = None
    """The template, for a `text` prompt. Variables are written `{{name}}`."""
    messages: Any | None = None
    """The messages, for a `chat` prompt: an array of objects with `role` and `content`, where `content` is a template."""
    config: Any | None = None
    """Model settings stored with the prompt, such as `model` and `temperature`. The daemon stores them and does not read them."""
    note: str | None = None
    """What changed in this version, written by whoever stored it."""
    created_by: str | None = None
    """Which key or account stored it."""

    TABLE = "documents"

@dataclass(**_SLOTS)
class ScorerDefinition:
    """A scorer the project defined: a judge prompt written for this agent, or a code scorer with its settings fixed. Stored as numbered versions; a version never changes. Name it in `scorers` wherever a scorer id is taken, as `id` for the latest version or `id@3` for version 3.

    Stored in the `documents` table.
    """

    project: str
    """Which project owns it."""
    id: str
    """Its id, unique in the project: lower-case letters, digits and `_`, up to 64. It cannot be the id of a scorer mcpd ships."""
    version: int
    """Its version, from 1, assigned by the daemon."""
    kind: ScorerDefinitionKind
    """Judged by the model, or computed by code."""
    description: str
    """What it measures, in a sentence. Shown in the scorer catalogue."""
    direction: ScoreDirection
    """Which way is better. Comparisons and live alerts read it."""
    created_at: datetime
    """When this version was stored."""
    prompt: str | None = None
    """For a `judge` definition: the criterion the model grades against, such as `The answer cites the refund policy by section number.` The daemon adds the run's input and output and asks for a value from 0 to 1 with a one-sentence reason."""
    builtin: str | None = None
    """For a `code` definition: the code scorer it runs, such as `keyword_coverage`."""
    config: ScorerConfig | None = None
    """For a `code` definition: the settings the code scorer runs with."""
    note: str | None = None
    """What changed in this version."""
    created_by: str | None = None
    """Which key or account stored it."""

    TABLE = "documents"

@dataclass(**_SLOTS)
class EvaluationRule:
    """A rule for live scoring: which finished runs to score, with which scorers, and how many of them. The daemon reads finished runs as they arrive and scores the sampled ones in the background, so a quality drop shows up in minutes rather than at the next offline pass. Scores it writes have source `live`.

    Stored in the `documents` table.
    """

    project: str
    """Which project it scores."""
    id: str
    """Its id, unique in the project."""
    version: int
    """Its version, from 1. Each change is a new version."""
    enabled: bool
    """Whether it runs. A rule is stored disabled unless the request says otherwise."""
    sample_rate: float
    """The share of matching runs to score, from 0 to 1. Chosen by a hash of the trace id, so the same run is in or out on every replica and every restart."""
    max_judged_per_minute: int
    """The most judged scores the rule may produce in one minute, across the daemon. Each one is a model call, so this caps what the rule can spend. Runs over it are skipped, not queued."""
    created_at: datetime
    """When this version was stored."""
    scorers: list[str] = field(default_factory=list)
    """The scorers to run: shipped ids, criteria from the config file, or stored definitions as `id` or `id@version`."""
    tags: list[str] = field(default_factory=list)
    """Only runs carrying all of these tags."""
    name: str | None = None
    """Only runs whose root step has this name."""
    kind: SpanKind | None = None
    """Only runs whose root step is this kind."""
    environment: str | None = None
    """Only runs from this environment."""
    status: SpanStatus | None = None
    """Only runs with this outcome."""
    config: ScorerConfig | None = None
    """Settings for the code scorers that need them."""
    note: str | None = None
    """What changed in this version."""
    created_by: str | None = None
    """Which key or account stored it."""

    TABLE = "documents"

@dataclass(**_SLOTS)
class ReviewItem:
    """A run, or one step of it, queued for a person to review. Queue the runs a low score or a thumbs-down flagged, assign them, and mark each reviewed or dismissed; every change is kept, so the item's history says who decided what and when. Corrections and labels go on the run as feedback.

    Stored in the `documents` table.
    """

    project: str
    """Which project."""
    review_id: str
    """Its id."""
    version: int
    """How many times it has changed, from 1."""
    trace_id: str
    """The run to review."""
    status: ReviewStatus
    """Where it has got to."""
    created_at: datetime
    """When it was queued."""
    updated_at: datetime
    """When it last changed."""
    span_id: str | None = None
    """The step to look at, when it is one step."""
    feedback_id: str | None = None
    """The feedback that put it in the queue, when it was feedback."""
    assignee: str | None = None
    """Who should review it."""
    note: str | None = None
    """Why it is queued, or what the reviewer found."""
    updated_by: str | None = None
    """Which key or account made the latest change."""

    TABLE = "documents"

#: Wire name -> attribute name, for the fields Python could not spell.
RESERVED_FIELD_NAMES = {
    "from": "from_",
    "class": "class_",
}
