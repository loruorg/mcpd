// GENERATED FILE. DO NOT EDIT.
//
// Produced from spec/mcpd.spec.json by tools/codegen.
// Edit the spec and run `make gen`. `make check` fails if this file drifts.

package mcpd

import (
	"encoding/json"
	"time"
)

// SpecVersion is the version of spec/mcpd.spec.json these types were generated
// from. A daemon reports the same string on /healthz. Compare the two at connect
// time. A mismatch caught there names the problem; the same mismatch found later
// shows up as a field missing for no visible reason.
const SpecVersion = "1.0.0"

// Go has no addressable literal, so a spec-declared default on an optional
// field needs a helper that returns a pointer to it.
func ptrString(v string) *string    { return &v }
func ptrBool(v bool) *bool          { return &v }
func ptrFloat64(v float64) *float64 { return &v }
func ptrUint64(v uint64) *uint64    { return &v }
func ptrUint32(v uint32) *uint32    { return &v }
func ptrInt64(v int64) *int64       { return &v }

// What an operation *is*. The kind drives how a span is rendered in the waterfall,
// which attributes are meaningful, and which metrics are auto-extracted from it.
type SpanKind string

const (
	// One agent invocation. Usually the root of a trace.
	SpanKindAgentRun SpanKind = "agent_run"
	// One logical generation: everything from the first request to the final answer,
	// including every intermediate step.
	SpanKindModelGeneration SpanKind = "model_generation"
	// One step inside a generation. A generation that calls tools twice has three steps.
	SpanKindModelStep SpanKind = "model_step"
	// The provider round-trip alone, excluding processors and tool execution. Measure pure
	// model latency here.
	SpanKindModelInference SpanKind = "model_inference"
	// A single streamed chunk. Recorded as an event, not a duration.
	SpanKindModelChunk SpanKind = "model_chunk"
	// A tool your code executes.
	SpanKindToolCall SpanKind = "tool_call"
	// A tool the calling client executes. The server records the marker; the client
	// returns the detail.
	SpanKindClientToolCall SpanKind = "client_tool_call"
	// A tool the model provider executes server-side, such as hosted web search or code
	// execution.
	SpanKindProviderToolCall SpanKind = "provider_tool_call"
	// A tool reached over the Model Context Protocol.
	SpanKindMcpToolCall SpanKind = "mcp_tool_call"
	// Control passing from one agent to another. The span's name is the agent handed to;
	// its parent is the agent handing off.
	SpanKindHandoff SpanKind = "handoff"
	// One workflow execution. Usually the root of a trace.
	SpanKindWorkflowRun SpanKind = "workflow_run"
	// One step within a workflow.
	SpanKindWorkflowStep SpanKind = "workflow_step"
	// A branch point, with the conditions it evaluated.
	SpanKindWorkflowConditional SpanKind = "workflow_conditional"
	// One condition evaluated inside a branch point.
	SpanKindWorkflowConditionalEval SpanKind = "workflow_conditional_eval"
	// A fan-out of concurrent branches.
	SpanKindWorkflowParallel SpanKind = "workflow_parallel"
	// A loop construct: foreach, do-while or do-until.
	SpanKindWorkflowLoop SpanKind = "workflow_loop"
	// A deliberate pause.
	SpanKindWorkflowSleep SpanKind = "workflow_sleep"
	// A wait for an external event, with its timeout.
	SpanKindWorkflowWaitEvent SpanKind = "workflow_wait_event"
	// A read from or write to agent memory.
	SpanKindMemoryOperation SpanKind = "memory_operation"
	// One ingestion pipeline run: load, chunk, extract, embed, upsert.
	SpanKindRagIngestion SpanKind = "rag_ingestion"
	// One embedding call, possibly batched.
	SpanKindRagEmbedding SpanKind = "rag_embedding"
	// Vector store I/O: query, upsert, delete or fetch.
	SpanKindRagVectorOperation SpanKind = "rag_vector_operation"
	// A RAG-specific transform: chunk, extract metadata or rerank.
	SpanKindRagAction SpanKind = "rag_action"
	// A graph build, traverse, update or prune.
	SpanKindGraphAction SpanKind = "graph_action"
	// An input or output processor.
	SpanKindProcessorRun SpanKind = "processor_run"
	// One scorer evaluating a target.
	SpanKindScorerRun SpanKind = "scorer_run"
	// One stage of a scorer's pipeline.
	SpanKindScorerStep SpanKind = "scorer_step"
	// A safety or policy check, and whether it tripped.
	SpanKindGuardrail SpanKind = "guardrail"
	// A generic retrieval that is not vector-store specific.
	SpanKindRetrieval SpanKind = "retrieval"
	// An inline data transform between pipeline stages.
	SpanKindMapping SpanKind = "mapping"
	// A dynamic resolver deciding which skills an agent may use.
	SpanKindSkillResolution SpanKind = "skill_resolution"
	// A filesystem, sandbox, search or computer-use action.
	SpanKindWorkspaceAction SpanKind = "workspace_action"
	// An outbound HTTP call made on the agent's behalf.
	SpanKindHTTPRequest SpanKind = "http_request"
	// Anything else. Always valid; never inferred.
	SpanKindGeneric SpanKind = "generic"
)

// AllSpanKinds lists every SpanKind value, in declaration order.
var AllSpanKinds = []SpanKind{SpanKindAgentRun, SpanKindModelGeneration, SpanKindModelStep, SpanKindModelInference, SpanKindModelChunk, SpanKindToolCall, SpanKindClientToolCall, SpanKindProviderToolCall, SpanKindMcpToolCall, SpanKindHandoff, SpanKindWorkflowRun, SpanKindWorkflowStep, SpanKindWorkflowConditional, SpanKindWorkflowConditionalEval, SpanKindWorkflowParallel, SpanKindWorkflowLoop, SpanKindWorkflowSleep, SpanKindWorkflowWaitEvent, SpanKindMemoryOperation, SpanKindRagIngestion, SpanKindRagEmbedding, SpanKindRagVectorOperation, SpanKindRagAction, SpanKindGraphAction, SpanKindProcessorRun, SpanKindScorerRun, SpanKindScorerStep, SpanKindGuardrail, SpanKindRetrieval, SpanKindMapping, SpanKindSkillResolution, SpanKindWorkspaceAction, SpanKindHTTPRequest, SpanKindGeneric}

// String returns the wire representation.
func (v SpanKind) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v SpanKind) Valid() bool {
	for _, c := range AllSpanKinds {
		if c == v {
			return true
		}
	}
	return false
}

// The outcome of an operation.
type SpanStatus string

const (
	// Still running, or the producer never said.
	SpanStatusUnset SpanStatus = "unset"
	// Completed successfully.
	SpanStatusOk SpanStatus = "ok"
	// Failed. `error` carries the detail.
	SpanStatusError SpanStatus = "error"
	// Abandoned deliberately, by a caller or a timeout.
	SpanStatusCancelled SpanStatus = "cancelled"
)

// AllSpanStatuss lists every SpanStatus value, in declaration order.
var AllSpanStatuss = []SpanStatus{SpanStatusUnset, SpanStatusOk, SpanStatusError, SpanStatusCancelled}

// String returns the wire representation.
func (v SpanStatus) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v SpanStatus) Valid() bool {
	for _, c := range AllSpanStatuss {
		if c == v {
			return true
		}
	}
	return false
}

// The six signals mcpd stores. Every one of them is correlated by trace id, most of
// them by span id as well, and every one of them passes through the same redaction
// pipeline.
type SignalType string

const (
	// A unit of work with a start and, usually, an end.
	SignalTypeSpan SignalType = "span"
	// A structured log record.
	SignalTypeLog SignalType = "log"
	// A numeric observation.
	SignalTypeMetric SignalType = "metric"
	// A machine evaluation of a span or trace.
	SignalTypeScore SignalType = "score"
	// A human evaluation of a span or trace.
	SignalTypeFeedback SignalType = "feedback"
	// One sentence recorded against a trace along one aspect. Stored, redacted and
	// retained like the other five, and erased with the subject it belongs to.
	SignalTypeAspectNote SignalType = "aspect_note"
)

// AllSignalTypes lists every SignalType value, in declaration order.
var AllSignalTypes = []SignalType{SignalTypeSpan, SignalTypeLog, SignalTypeMetric, SignalTypeScore, SignalTypeFeedback, SignalTypeAspectNote}

// String returns the wire representation.
func (v SignalType) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v SignalType) Valid() bool {
	for _, c := range AllSignalTypes {
		if c == v {
			return true
		}
	}
	return false
}

// Severity, ordered. Filtering is always `>=`.
type LogLevel string

const (
	// Finer than debug. Off by default.
	LogLevelTrace LogLevel = "trace"
	// Detail useful while diagnosing.
	LogLevelDebug LogLevel = "debug"
	// Normal operation.
	LogLevelInfo LogLevel = "info"
	// Recoverable, but worth reading.
	LogLevelWarn LogLevel = "warn"
	// The operation failed.
	LogLevelError LogLevel = "error"
	// The process cannot continue.
	LogLevelFatal LogLevel = "fatal"
)

// AllLogLevels lists every LogLevel value, in declaration order.
var AllLogLevels = []LogLevel{LogLevelTrace, LogLevelDebug, LogLevelInfo, LogLevelWarn, LogLevelError, LogLevelFatal}

// String returns the wire representation.
func (v LogLevel) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v LogLevel) Valid() bool {
	for _, c := range AllLogLevels {
		if c == v {
			return true
		}
	}
	return false
}

// Severity returns the ordered severity. Filtering is always >=.
func (v LogLevel) Severity() int {
	switch v {
	case LogLevelTrace:
		return 1
	case LogLevelDebug:
		return 5
	case LogLevelInfo:
		return 9
	case LogLevelWarn:
		return 13
	case LogLevelError:
		return 17
	case LogLevelFatal:
		return 21
	}
	return 0
}

// How a numeric observation aggregates.
type MetricKind string

const (
	// Monotonically increasing. Aggregates by sum.
	MetricKindCounter MetricKind = "counter"
	// A point-in-time value. Aggregates by last, min, max or mean.
	MetricKindGauge MetricKind = "gauge"
	// A distribution. Aggregates by bucket, and answers quantiles.
	MetricKindHistogram MetricKind = "histogram"
)

// AllMetricKinds lists every MetricKind value, in declaration order.
var AllMetricKinds = []MetricKind{MetricKindCounter, MetricKindGauge, MetricKindHistogram}

// String returns the wire representation.
func (v MetricKind) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v MetricKind) Valid() bool {
	for _, c := range AllMetricKinds {
		if c == v {
			return true
		}
	}
	return false
}

// How mcpd decides which traces to keep.
type SamplingKind string

const (
	// Keep everything. The default, and correct until volume says otherwise.
	SamplingKindAlways SamplingKind = "always"
	// Keep nothing. Useful to disable a single project without redeploying.
	SamplingKindNever SamplingKind = "never"
	// Keep a fixed fraction, decided once at the root so a trace is never half-recorded.
	SamplingKindRatio SamplingKind = "ratio"
	// Keep a fixed fraction, decided by hashing the trace id. Every collector in a fleet
	// makes the same decision without coordinating.
	SamplingKindDeterministic SamplingKind = "deterministic"
	// Buffer the trace, then decide once it finishes. Keeps every error and every slow
	// trace regardless of rate.
	SamplingKindTail SamplingKind = "tail"
)

// AllSamplingKinds lists every SamplingKind value, in declaration order.
var AllSamplingKinds = []SamplingKind{SamplingKindAlways, SamplingKindNever, SamplingKindRatio, SamplingKindDeterministic, SamplingKindTail}

// String returns the wire representation.
func (v SamplingKind) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v SamplingKind) Valid() bool {
	for _, c := range AllSamplingKinds {
		if c == v {
			return true
		}
	}
	return false
}

// What happens to a value a redaction rule matched.
type RedactionMode string

const (
	// Remove the key entirely, as if it was never sent.
	RedactionModeDrop RedactionMode = "drop"
	// Replace the whole value with a fixed token.
	RedactionModeMask RedactionMode = "mask"
	// Keep a documented prefix and suffix, mask the middle. Enough to recognise a value
	// without disclosing it.
	RedactionModePartial RedactionMode = "partial"
	// Replace with a keyed SHA-256 digest. Equal inputs stay equal, so you can still group
	// and join, but the original cannot be recovered.
	RedactionModeHash RedactionMode = "hash"
)

// AllRedactionModes lists every RedactionMode value, in declaration order.
var AllRedactionModes = []RedactionMode{RedactionModeDrop, RedactionModeMask, RedactionModePartial, RedactionModeHash}

// String returns the wire representation.
func (v RedactionMode) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v RedactionMode) Valid() bool {
	for _, c := range AllRedactionModes {
		if c == v {
			return true
		}
	}
	return false
}

// Where a score came from. Kept separate from the scorer's identity so the same scorer
// can run in more than one context.
type ScoreSource string

const (
	// Produced online, as the traced run happened.
	ScoreSourceLive ScoreSource = "live"
	// Produced later, by scoring stored traces.
	ScoreSourceOffline ScoreSource = "offline"
	// Produced by an experiment run against a dataset.
	ScoreSourceExperiment ScoreSource = "experiment"
	// Entered by a person, through the API or the UI.
	ScoreSourceManual ScoreSource = "manual"
)

// AllScoreSources lists every ScoreSource value, in declaration order.
var AllScoreSources = []ScoreSource{ScoreSourceLive, ScoreSourceOffline, ScoreSourceExperiment, ScoreSourceManual}

// String returns the wire representation.
func (v ScoreSource) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v ScoreSource) Valid() bool {
	for _, c := range AllScoreSources {
		if c == v {
			return true
		}
	}
	return false
}

// How a scorer reaches its answer.
type ScorerKind string

const (
	// Deterministic code in the daemon. Free, and gives the same answer twice.
	ScorerKindCode ScorerKind = "code"
	// A built-in rubric graded by the configured judge model, in steps: preprocess,
	// analyze, score, reason.
	ScorerKindJudge ScorerKind = "judge"
	// A criterion the operator wrote under `judge.criteria`, graded by the judge model in
	// one call.
	ScorerKindCriterion ScorerKind = "criterion"
)

// AllScorerKinds lists every ScorerKind value, in declaration order.
var AllScorerKinds = []ScorerKind{ScorerKindCode, ScorerKindJudge, ScorerKindCriterion}

// String returns the wire representation.
func (v ScorerKind) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v ScorerKind) Valid() bool {
	for _, c := range AllScorerKinds {
		if c == v {
			return true
		}
	}
	return false
}

// Which way a scorer's value is better. A comparison reads a fall in `toxicity` as an
// improvement and a fall in `faithfulness` as a regression.
type ScoreDirection string

const (
	// A larger value is a better run.
	ScoreDirectionHigherIsBetter ScoreDirection = "higher_is_better"
	// A smaller value is a better run, as for `toxicity`, `bias` and `hallucination`.
	ScoreDirectionLowerIsBetter ScoreDirection = "lower_is_better"
)

// AllScoreDirections lists every ScoreDirection value, in declaration order.
var AllScoreDirections = []ScoreDirection{ScoreDirectionHigherIsBetter, ScoreDirectionLowerIsBetter}

// String returns the wire representation.
func (v ScoreDirection) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v ScoreDirection) Valid() bool {
	for _, c := range AllScoreDirections {
		if c == v {
			return true
		}
	}
	return false
}

// The shape of a human judgement.
type FeedbackKind string

const (
	// Binary. `value` is `up` or `down`.
	FeedbackKindThumbs FeedbackKind = "thumbs"
	// Ordinal. `value` is a number inside the scale the producer declares.
	FeedbackKindRating FeedbackKind = "rating"
	// The right answer. `value` is text.
	FeedbackKindCorrection FeedbackKind = "correction"
	// A label chosen from a set. `value` is the label.
	FeedbackKindTag FeedbackKind = "tag"
	// Free prose, with no score attached.
	FeedbackKindComment FeedbackKind = "comment"
)

// AllFeedbackKinds lists every FeedbackKind value, in declaration order.
var AllFeedbackKinds = []FeedbackKind{FeedbackKindThumbs, FeedbackKindRating, FeedbackKindCorrection, FeedbackKindTag, FeedbackKindComment}

// String returns the wire representation.
func (v FeedbackKind) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v FeedbackKind) Valid() bool {
	for _, c := range AllFeedbackKinds {
		if c == v {
			return true
		}
	}
	return false
}

// The named thing a span belongs to. Bounded and low-cardinality, so it is safe as a
// metric label.
type EntityKind string

const (
	// An agent.
	EntityKindAgent EntityKind = "agent"
	// A workflow.
	EntityKindWorkflow EntityKind = "workflow"
	// A tool.
	EntityKindTool EntityKind = "tool"
	// A model.
	EntityKindModel EntityKind = "model"
	// A scorer.
	EntityKindScorer EntityKind = "scorer"
	// A processor.
	EntityKindProcessor EntityKind = "processor"
	// A RAG or data pipeline.
	EntityKindPipeline EntityKind = "pipeline"
	// A plain service with no richer identity.
	EntityKindService EntityKind = "service"
)

// AllEntityKinds lists every EntityKind value, in declaration order.
var AllEntityKinds = []EntityKind{EntityKindAgent, EntityKindWorkflow, EntityKindTool, EntityKindModel, EntityKindScorer, EntityKindProcessor, EntityKindPipeline, EntityKindService}

// String returns the wire representation.
func (v EntityKind) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v EntityKind) Valid() bool {
	for _, c := range AllEntityKinds {
		if c == v {
			return true
		}
	}
	return false
}

// Where an experiment run has got to.
type ExperimentStatus string

const (
	// Created, not started.
	ExperimentStatusPending ExperimentStatus = "pending"
	// In progress.
	ExperimentStatusRunning ExperimentStatus = "running"
	// Every item completed.
	ExperimentStatusSucceeded ExperimentStatus = "succeeded"
	// Aborted. `error` says why.
	ExperimentStatusFailed ExperimentStatus = "failed"
	// Stopped by a person.
	ExperimentStatusCancelled ExperimentStatus = "cancelled"
)

// AllExperimentStatuss lists every ExperimentStatus value, in declaration order.
var AllExperimentStatuss = []ExperimentStatus{ExperimentStatusPending, ExperimentStatusRunning, ExperimentStatusSucceeded, ExperimentStatusFailed, ExperimentStatusCancelled}

// String returns the wire representation.
func (v ExperimentStatus) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v ExperimentStatus) Valid() bool {
	for _, c := range AllExperimentStatuss {
		if c == v {
			return true
		}
	}
	return false
}

// Whether authorisation let the request through.
type AuditResult string

const (
	// The key carried the scope the endpoint needs, so mcpd went on to handle the request.
	AuditResultAllowed AuditResult = "allowed"
	// Authorisation refused the request before it reached storage. Recording the attempt
	// is the point of the row.
	AuditResultRefused AuditResult = "refused"
)

// AllAuditResults lists every AuditResult value, in declaration order.
var AllAuditResults = []AuditResult{AuditResultAllowed, AuditResultRefused}

// String returns the wire representation.
func (v AuditResult) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v AuditResult) Valid() bool {
	for _, c := range AllAuditResults {
		if c == v {
			return true
		}
	}
	return false
}

// Where a run is. Only `running` can move; the other four are terminal, and a terminal
// run holds no deadline, no wait and no live attempt.
type RunStatus string

const (
	// Making progress, or waiting for something that will move it: a lease, a timer or an
	// event.
	RunStatusRunning RunStatus = "running"
	// Reached the end of the graph. `output` holds what the definition's `output` template
	// rendered, when it declares one.
	RunStatusSucceeded RunStatus = "succeeded"
	// Ended on an error nothing handled: a step that failed under `on_error: fail_run`, a
	// spent ceiling, or a cancelled child. `error` says which.
	RunStatusFailed RunStatus = "failed"
	// Ended because the run's own deadline passed.
	RunStatusTimedOut RunStatus = "timed_out"
	// Ended because a caller asked, through the cancel endpoint or by cancelling an
	// ancestor run.
	RunStatusCancelled RunStatus = "cancelled"
)

// AllRunStatuss lists every RunStatus value, in declaration order.
var AllRunStatuss = []RunStatus{RunStatusRunning, RunStatusSucceeded, RunStatusFailed, RunStatusTimedOut, RunStatusCancelled}

// String returns the wire representation.
func (v RunStatus) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v RunStatus) Valid() bool {
	for _, c := range AllRunStatuss {
		if c == v {
			return true
		}
	}
	return false
}

// Where one attempt at one step is. Four states are live and four are terminal, and
// the daemon never shows a live attempt on an ended run.
type AttemptStatus string

const (
	// On disk and offerable to a worker, once its retry timer, if any, has fired.
	AttemptStatusPending AttemptStatus = "pending"
	// Held by a worker that has not yet posted `started`.
	AttemptStatusLeased AttemptStatus = "leased"
	// Held by a worker that has posted `started`, so a body is running.
	AttemptStatusStarted AttemptStatus = "started"
	// The lease expired with no heartbeat. Not offerable while the reclaim window is open:
	// a heartbeat inside the window restores the attempt, and a window that closes with no
	// contact requeues it or fails the run, depending on `effect` and on whether the body
	// had started.
	AttemptStatusOrphaned AttemptStatus = "orphaned"
	// Finished with an output.
	AttemptStatusSucceeded AttemptStatus = "succeeded"
	// Finished with an error, or marked failed because the run ended while the attempt was
	// live.
	AttemptStatusFailed AttemptStatus = "failed"
	// The step's own deadline passed before the attempt ended.
	AttemptStatusTimedOut AttemptStatus = "timed_out"
	// Never ran, because the step's `when` guard was false.
	AttemptStatusSkipped AttemptStatus = "skipped"
)

// AllAttemptStatuss lists every AttemptStatus value, in declaration order.
var AllAttemptStatuss = []AttemptStatus{AttemptStatusPending, AttemptStatusLeased, AttemptStatusStarted, AttemptStatusOrphaned, AttemptStatusSucceeded, AttemptStatusFailed, AttemptStatusTimedOut, AttemptStatusSkipped}

// String returns the wire representation.
func (v AttemptStatus) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v AttemptStatus) Valid() bool {
	for _, c := range AllAttemptStatuss {
		if c == v {
			return true
		}
	}
	return false
}

// What a step attempt is an attempt at. `task` and `agent` are claimable. A `wait` row
// is written by the event that resumes it or the deadline that ends it, so a wait's
// output and status read like any other step's.
type StepKind string

const (
	// Claimable work. The worker runs whatever code it registered under this step id.
	StepKindTask StepKind = "task"
	// Claimable work whose body is the worker's own agent loop. Identical on the wire to
	// `task`; the kind is recorded so the trace and the interface can tell an agent turn
	// from a plain step.
	StepKindAgent StepKind = "agent"
	// A suspension the daemon resumes on an event or ends on a deadline. Never offered to
	// a worker.
	StepKindWait StepKind = "wait"
)

// AllStepKinds lists every StepKind value, in declaration order.
var AllStepKinds = []StepKind{StepKindTask, StepKindAgent, StepKindWait}

// String returns the wire representation.
func (v StepKind) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v StepKind) Valid() bool {
	for _, c := range AllStepKinds {
		if c == v {
			return true
		}
	}
	return false
}

// What a run's cursor names: the construct at its current position in the definition's
// `steps`. Every kind except `end` is a node kind a definition can declare.
type ConstructKind string

const (
	// Claimable work.
	ConstructKindTask ConstructKind = "task"
	// Claimable work whose body is the worker's agent loop.
	ConstructKindAgent ConstructKind = "agent"
	// A relative delay, in milliseconds. No process is alive during it.
	ConstructKindSleep ConstructKind = "sleep"
	// An absolute delay, to an RFC 3339 instant rendered from a template.
	ConstructKindSleepUntil ConstructKind = "sleep_until"
	// A suspension addressed by an event name and a key rendered per run.
	ConstructKindWaitEvent ConstructKind = "wait_event"
	// A fixed fan-out over named branches, each branch one claimable step. Done when every
	// branch has ended.
	ConstructKindParallel ConstructKind = "parallel"
	// A fan-out over the items of an array, each item one claimable step, with at most
	// `concurrency` of them offerable at once.
	ConstructKindForeach ConstructKind = "foreach"
	// A bounded loop whose body is one claimable step per iteration, ended by `until` or
	// by `max_iterations`.
	ConstructKindLoop ConstructKind = "loop"
	// A child run of another definition. Its output becomes this step's output, and a
	// cancel walks down into it.
	ConstructKindSubworkflow ConstructKind = "subworkflow"
	// The end of the graph. Never declared in a definition: the cursor reaches it when the
	// last step has ended.
	ConstructKindEnd ConstructKind = "end"
)

// AllConstructKinds lists every ConstructKind value, in declaration order.
var AllConstructKinds = []ConstructKind{ConstructKindTask, ConstructKindAgent, ConstructKindSleep, ConstructKindSleepUntil, ConstructKindWaitEvent, ConstructKindParallel, ConstructKindForeach, ConstructKindLoop, ConstructKindSubworkflow, ConstructKindEnd}

// String returns the wire representation.
func (v ConstructKind) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v ConstructKind) Valid() bool {
	for _, c := range AllConstructKinds {
		if c == v {
			return true
		}
	}
	return false
}

// What running a step body twice would cost, which is what the daemon branches on when
// a lease is lost. Default `at_least_once`.
type Effect string

const (
	// Running the body twice is acceptable. A lost lease requeues the attempt. The
	// default.
	EffectAtLeastOnce Effect = "at_least_once"
	// Running the body twice is not acceptable. `started` must be posted before the body
	// runs, and a started attempt whose lease is lost fails the run rather than running
	// again.
	EffectAtMostOnce Effect = "at_most_once"
)

// AllEffects lists every Effect value, in declaration order.
var AllEffects = []Effect{EffectAtLeastOnce, EffectAtMostOnce}

// String returns the wire representation.
func (v Effect) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v Effect) Valid() bool {
	for _, c := range AllEffects {
		if c == v {
			return true
		}
	}
	return false
}

// Every deadline the daemon keeps. Each one is a row the tick fires, and each firing
// is a transaction that moves the run.
type TimerKind string

const (
	// A `sleep` or `sleep_until` coming due.
	TimerKindSleep TimerKind = "sleep"
	// A lease that has not been heartbeated. Firing moves the attempt to `orphaned` and
	// opens a reclaim window one lease length long.
	TimerKindLeaseExpiry TimerKind = "lease_expiry"
	// The close of the reclaim window an expired lease opened. Firing requeues the attempt
	// or fails the run, depending on `effect` and on whether the body had started.
	TimerKindReclaim TimerKind = "reclaim"
	// The whole run's ceiling, from `limits.run_timeout_ms`.
	TimerKindRunTimeout TimerKind = "run_timeout"
	// One attempt's ceiling, from the node's `step_timeout_ms`. It bounds a worker that
	// heartbeats for ever.
	TimerKindStepTimeout TimerKind = "step_timeout"
	// A `wait_event` whose event has not arrived by `timeout_ms`.
	TimerKindWaitTimeout TimerKind = "wait_timeout"
	// The backoff before a requeued attempt becomes claimable. Firing writes the next
	// attempt as `pending`.
	TimerKindRetry TimerKind = "retry"
)

// AllTimerKinds lists every TimerKind value, in declaration order.
var AllTimerKinds = []TimerKind{TimerKindSleep, TimerKindLeaseExpiry, TimerKindReclaim, TimerKindRunTimeout, TimerKindStepTimeout, TimerKindWaitTimeout, TimerKindRetry}

// String returns the wire representation.
func (v TimerKind) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v TimerKind) Valid() bool {
	for _, c := range AllTimerKinds {
		if c == v {
			return true
		}
	}
	return false
}

// What a row in a run's event log records. Every row is written in the transaction
// that made the change it describes, so a reader that has seen sequence number n has
// seen every change below it.
type RunEventKind string

const (
	// The run was created. Payload: `definition_id`, `definition_version`.
	RunEventKindRunStarted RunEventKind = "run_started"
	// The cursor moved. Payload: `index`, `construct`, and `step_key` when the construct
	// has one.
	RunEventKindRunCursor RunEventKind = "run_cursor"
	// A worker claimed an attempt. Payload: `step_key`, `attempt`, `worker_id`.
	RunEventKindStepLeased RunEventKind = "step_leased"
	// A worker posted `started`. Payload: `step_key`, `attempt`.
	RunEventKindStepStarted RunEventKind = "step_started"
	// An attempt succeeded. Payload: `step_key`, `attempt`.
	RunEventKindStepFinished RunEventKind = "step_finished"
	// An attempt failed. Payload: `step_key`, `attempt`, `class`, `message`,
	// `attempts_left`.
	RunEventKindStepFailed RunEventKind = "step_failed"
	// A `when` guard was false. Payload: `step_key`.
	RunEventKindStepSkipped RunEventKind = "step_skipped"
	// The run reached a `wait_event`. Payload: `step_key`, `event`, `key`, `expires_at`.
	RunEventKindRunWaiting RunEventKind = "run_waiting"
	// An event resumed a waiting run. Payload: `step_key`, `event_id`.
	RunEventKindRunResumed RunEventKind = "run_resumed"
	// The run reached a terminal status. Payload: `status`, and `error` when there is one.
	RunEventKindRunEnded RunEventKind = "run_ended"
)

// AllRunEventKinds lists every RunEventKind value, in declaration order.
var AllRunEventKinds = []RunEventKind{RunEventKindRunStarted, RunEventKindRunCursor, RunEventKindStepLeased, RunEventKindStepStarted, RunEventKindStepFinished, RunEventKindStepFailed, RunEventKindStepSkipped, RunEventKindRunWaiting, RunEventKindRunResumed, RunEventKindRunEnded}

// String returns the wire representation.
func (v RunEventKind) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v RunEventKind) Valid() bool {
	for _, c := range AllRunEventKinds {
		if c == v {
			return true
		}
	}
	return false
}

// What kind of thing a definition describes.
type DefinitionKind string

const (
	// A graph of steps the daemon walks. The only kind in version 1: `agent` definitions
	// arrive with the in-process agent loop, which is not served yet.
	DefinitionKindWorkflow DefinitionKind = "workflow"
)

// AllDefinitionKinds lists every DefinitionKind value, in declaration order.
var AllDefinitionKinds = []DefinitionKind{DefinitionKindWorkflow}

// String returns the wire representation.
func (v DefinitionKind) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v DefinitionKind) Valid() bool {
	for _, c := range AllDefinitionKinds {
		if c == v {
			return true
		}
	}
	return false
}

// What the daemon did with a published event.
type EventOutcome string

const (
	// A waiting run took it and moved, in the same transaction.
	EventOutcomeDelivered EventOutcome = "delivered"
	// No run was waiting. The event is held for 24 hours for the next run that reaches a
	// matching wait with `accept_parked`.
	EventOutcomeParked EventOutcome = "parked"
	// This event id has been seen before, so nothing was written. The publish endpoint
	// says when a derived id is treated this way.
	EventOutcomeDuplicate EventOutcome = "duplicate"
)

// AllEventOutcomes lists every EventOutcome value, in declaration order.
var AllEventOutcomes = []EventOutcome{EventOutcomeDelivered, EventOutcomeParked, EventOutcomeDuplicate}

// String returns the wire representation.
func (v EventOutcome) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v EventOutcome) Valid() bool {
	for _, c := range AllEventOutcomes {
		if c == v {
			return true
		}
	}
	return false
}

// Why a worker call about a lease answered 409. Each reason tells the worker something
// different to do next.
type ConflictReason string

const (
	// A later attempt exists, or another lease terminated this attempt. Abandon the work
	// and post nothing more for this lease.
	ConflictReasonFenced ConflictReason = "fenced"
	// This lease's own earlier `complete` or `fail` landed and its reply was lost. The
	// step is done; `attempt_status` says how. Post nothing more.
	ConflictReasonAlreadyApplied ConflictReason = "already_applied"
	// `complete` or `fail` arrived for a `leased` attempt under `at_most_once`. A bug in
	// the worker: post `started` first, and log it loudly.
	ConflictReasonNotStarted ConflictReason = "not_started"
	// The run has ended. Stop, and stop claiming for this run.
	ConflictReasonRunTerminal ConflictReason = "run_terminal"
)

// AllConflictReasons lists every ConflictReason value, in declaration order.
var AllConflictReasons = []ConflictReason{ConflictReasonFenced, ConflictReasonAlreadyApplied, ConflictReasonNotStarted, ConflictReasonRunTerminal}

// String returns the wire representation.
func (v ConflictReason) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v ConflictReason) Valid() bool {
	for _, c := range AllConflictReasons {
		if c == v {
			return true
		}
	}
	return false
}

// How a step failed, which decides whether the daemon retries it.
type StepErrorClass string

const (
	// Worth another attempt. The daemon retries while attempts remain, at the node's
	// backoff, and honours `retry_after_ms` when it is larger.
	StepErrorClassRetryable StepErrorClass = "retryable"
	// Another attempt would fail the same way. No retry; the node's `on_error` applies.
	StepErrorClassFatal StepErrorClass = "fatal"
	// The step's input cannot be acted on. No retry; the node's `on_error` applies.
	StepErrorClassInvalidInput StepErrorClass = "invalid_input"
	// This worker does not serve the definition version, or does not recognise the step
	// key. The attempt returns to `pending` after one lease length without spending the
	// retry budget, so another worker can take it.
	StepErrorClassUnservable StepErrorClass = "unservable"
	// The worker gave the attempt up on request. No retry; the node's `on_error` applies.
	StepErrorClassCancelled StepErrorClass = "cancelled"
)

// AllStepErrorClasss lists every StepErrorClass value, in declaration order.
var AllStepErrorClasss = []StepErrorClass{StepErrorClassRetryable, StepErrorClassFatal, StepErrorClassInvalidInput, StepErrorClassUnservable, StepErrorClassCancelled}

// String returns the wire representation.
func (v StepErrorClass) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v StepErrorClass) Valid() bool {
	for _, c := range AllStepErrorClasss {
		if c == v {
			return true
		}
	}
	return false
}

// What happens to a run when a node fails after its retries. Default `fail_run`.
type OnError string

const (
	// End the run `failed`, with the step's error message as the run's error.
	OnErrorFailRun OnError = "fail_run"
	// Mark the step `failed` and move on, so a later `when` guard can read
	// `steps.<id>.status` and `steps.<id>.error`.
	OnErrorContinue OnError = "continue"
)

// AllOnErrors lists every OnError value, in declaration order.
var AllOnErrors = []OnError{OnErrorFailRun, OnErrorContinue}

// String returns the wire representation.
func (v OnError) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v OnError) Valid() bool {
	for _, c := range AllOnErrors {
		if c == v {
			return true
		}
	}
	return false
}

// How the delay between retries grows.
type Backoff string

const (
	// Every retry waits `initial_ms`.
	BackoffFixed Backoff = "fixed"
	// Each retry waits twice as long as the last, starting at `initial_ms` and capped at
	// `max_ms`.
	BackoffExponential Backoff = "exponential"
)

// AllBackoffs lists every Backoff value, in declaration order.
var AllBackoffs = []Backoff{BackoffFixed, BackoffExponential}

// String returns the wire representation.
func (v Backoff) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v Backoff) Valid() bool {
	for _, c := range AllBackoffs {
		if c == v {
			return true
		}
	}
	return false
}

// What a `wait_event` node does when its deadline passes with no event. Default
// `fail`.
type WaitTimeout string

const (
	// The node fails. Its `on_error` decides what the run does next.
	WaitTimeoutFail WaitTimeout = "fail"
	// The wait ends and the run moves on to the next node.
	WaitTimeoutContinue WaitTimeout = "continue"
)

// AllWaitTimeouts lists every WaitTimeout value, in declaration order.
var AllWaitTimeouts = []WaitTimeout{WaitTimeoutFail, WaitTimeoutContinue}

// String returns the wire representation.
func (v WaitTimeout) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v WaitTimeout) Valid() bool {
	for _, c := range AllWaitTimeouts {
		if c == v {
			return true
		}
	}
	return false
}

// The JSON type of a value. Used where mcpd reports the shape of something it holds
// without returning the thing itself.
type ValueKind string

const (
	// The value was JSON null.
	ValueKindNull ValueKind = "null"
	// The value was `true` or `false`.
	ValueKindBoolean ValueKind = "boolean"
	// The value was a JSON number.
	ValueKindNumber ValueKind = "number"
	// The value was a JSON string.
	ValueKindString ValueKind = "string"
	// The value was a JSON array.
	ValueKindArray ValueKind = "array"
	// The value was a JSON object.
	ValueKindObject ValueKind = "object"
)

// AllValueKinds lists every ValueKind value, in declaration order.
var AllValueKinds = []ValueKind{ValueKindNull, ValueKindBoolean, ValueKindNumber, ValueKindString, ValueKindArray, ValueKindObject}

// String returns the wire representation.
func (v ValueKind) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v ValueKind) Valid() bool {
	for _, c := range AllValueKinds {
		if c == v {
			return true
		}
	}
	return false
}

// What a monitor measures over its window.
type MonitorWatch string

const (
	// The fraction of runs that failed, 0 to 1.
	MonitorWatchErrorRate MonitorWatch = "error_rate"
	// Run duration at the ninety-fifth percentile, in milliseconds.
	MonitorWatchLatencyP95 MonitorWatch = "latency_p95"
	// Total spend across the window.
	MonitorWatchCost MonitorWatch = "cost"
	// How many runs happened. Catches a pipeline that has gone silent.
	MonitorWatchTraceCount MonitorWatch = "trace_count"
	// The mean of the scorer named by `scorer_id`.
	MonitorWatchScoreMean MonitorWatch = "score_mean"
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
	MonitorWatchStepCountDivergence MonitorWatch = "step_count_divergence"
	// How much a run repeats itself. For each run in the window, the highest number of
	// attempts it made at any one node of its definition, counting a retry and a loop
	// iteration of that node alike; the watch reads the mean of that number. One means no
	// node ran twice. A loop that re-enters the same node, and a retry storm, both raise
	// this before either reaches `error_rate`.
	MonitorWatchRunRepetition MonitorWatch = "run_repetition"
)

// AllMonitorWatchs lists every MonitorWatch value, in declaration order.
var AllMonitorWatchs = []MonitorWatch{MonitorWatchErrorRate, MonitorWatchLatencyP95, MonitorWatchCost, MonitorWatchTraceCount, MonitorWatchScoreMean, MonitorWatchStepCountDivergence, MonitorWatchRunRepetition}

// String returns the wire representation.
func (v MonitorWatch) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v MonitorWatch) Valid() bool {
	for _, c := range AllMonitorWatchs {
		if c == v {
			return true
		}
	}
	return false
}

// Which way a monitor's threshold is breached.
type MonitorCompare string

const (
	// Trip when the value rises above the threshold.
	MonitorCompareAbove MonitorCompare = "above"
	// Trip when the value falls below it. This is the direction that catches a quality
	// regression, and a silent pipeline.
	MonitorCompareBelow MonitorCompare = "below"
)

// AllMonitorCompares lists every MonitorCompare value, in declaration order.
var AllMonitorCompares = []MonitorCompare{MonitorCompareAbove, MonitorCompareBelow}

// String returns the wire representation.
func (v MonitorCompare) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v MonitorCompare) Valid() bool {
	for _, c := range AllMonitorCompares {
		if c == v {
			return true
		}
	}
	return false
}

// Where a monitor or an objective stands. A tripped condition latches: it stays
// tripped until the number recovers, and too little data never clears it, because
// silence is not recovery.
type MonitorCondition string

const (
	// Evaluated, and within its threshold.
	MonitorConditionNominal MonitorCondition = "nominal"
	// Tripped, and not yet acknowledged.
	MonitorConditionTripped MonitorCondition = "tripped"
	// Tripped, acknowledged, and still breaching. Returns to `nominal` on its own when the
	// number recovers.
	MonitorConditionAcknowledged MonitorCondition = "acknowledged"
	// Not enough data in the window to judge. Also the state before the first evaluation.
	MonitorConditionInsufficient MonitorCondition = "insufficient"
)

// AllMonitorConditions lists every MonitorCondition value, in declaration order.
var AllMonitorConditions = []MonitorCondition{MonitorConditionNominal, MonitorConditionTripped, MonitorConditionAcknowledged, MonitorConditionInsufficient}

// String returns the wire representation.
func (v MonitorCondition) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v MonitorCondition) Valid() bool {
	for _, c := range AllMonitorConditions {
		if c == v {
			return true
		}
	}
	return false
}

// What the `q` parameter matches on this deployment's storage backend. Reported by
// `GET /v1/api/capabilities` so a search box can say what it searches instead of
// leaving a reader to infer it from a result set.
type TextSearch string

const (
	// `q` is not applied at all. No backend that ships declares this; a backend that did
	// would be telling an interface to hide its search box rather than let somebody type
	// into one that returns everything.
	TextSearchNone TextSearch = "none"
	// `q` matches anywhere inside a word, so `heck` finds `checkout`.
	TextSearchSubstring TextSearch = "substring"
	// `q` matches whole words only, which is what a person typing two words into a search
	// box expects. There is no stemming: `refund` does not find `refunds`.
	TextSearchWords TextSearch = "words"
)

// AllTextSearchs lists every TextSearch value, in declaration order.
var AllTextSearchs = []TextSearch{TextSearchNone, TextSearchSubstring, TextSearchWords}

// String returns the wire representation.
func (v TextSearch) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v TextSearch) Valid() bool {
	for _, c := range AllTextSearchs {
		if c == v {
			return true
		}
	}
	return false
}

// How the storage backend cuts each signal table on its anchor time. It decides
// whether expiring a day of data is a metadata operation on one part or a row-by-row
// delete.
type Partitioning string

const (
	// One table, no partitions. Retention removes rows.
	PartitioningNone Partitioning = "none"
	// One partition per day.
	PartitioningDay Partitioning = "day"
	// One partition per month.
	PartitioningMonth Partitioning = "month"
)

// AllPartitionings lists every Partitioning value, in declaration order.
var AllPartitionings = []Partitioning{PartitioningNone, PartitioningDay, PartitioningMonth}

// String returns the wire representation.
func (v Partitioning) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v Partitioning) Valid() bool {
	for _, c := range AllPartitionings {
		if c == v {
			return true
		}
	}
	return false
}

// A dimension a filter box can offer values for. The set is closed on purpose: each
// one is a column or a parsed field the daemon keeps a distinct-value list for as
// signals arrive, so answering is a read of a small table rather than a scan of every
// span in the retention window.
type Facet string

const (
	// The kind of thing a step was: an agent run, a tool call, a retrieval, and so on.
	FacetEntityKind Facet = "entity_kind"
	// The name of that thing, such as the tool that was called.
	FacetEntityName Facet = "entity_name"
	// The service that reported the signal.
	FacetServiceName Facet = "service_name"
	// The environment it came from, such as `production`.
	FacetEnvironment Facet = "environment"
	// A tag carried on a run.
	FacetTag Facet = "tag"
	// A metric name, for the name box of a chart.
	FacetMetricName Facet = "metric_name"
	// A label key a metric point carries. Project-wide: these are every key seen in the
	// project, not the keys of one named metric.
	FacetMetricLabelKey Facet = "metric_label_key"
	// A value one label key takes. Needs `key`, because the answer would otherwise be
	// every value of every label mixed together.
	FacetMetricLabelValue Facet = "metric_label_value"
)

// AllFacets lists every Facet value, in declaration order.
var AllFacets = []Facet{FacetEntityKind, FacetEntityName, FacetServiceName, FacetEnvironment, FacetTag, FacetMetricName, FacetMetricLabelKey, FacetMetricLabelValue}

// String returns the wire representation.
func (v Facet) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v Facet) Valid() bool {
	for _, c := range AllFacets {
		if c == v {
			return true
		}
	}
	return false
}

// A signal that can be polled for what has arrived. Only the three that carry a
// server-set arrival time: a score and a piece of feedback are stamped with the
// caller's own timestamp, so a watermark over them would either skip a late row or
// repeat every row for ever.
type DeltaSignal string

const (
	// The run projection, which is what a run list refreshes from.
	DeltaSignalTraces DeltaSignal = "traces"
	// Steps, for a waterfall that is still filling in.
	DeltaSignalSpans DeltaSignal = "spans"
	// Log lines.
	DeltaSignalLogs DeltaSignal = "logs"
)

// AllDeltaSignals lists every DeltaSignal value, in declaration order.
var AllDeltaSignals = []DeltaSignal{DeltaSignalTraces, DeltaSignalSpans, DeltaSignalLogs}

// String returns the wire representation.
func (v DeltaSignal) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v DeltaSignal) Valid() bool {
	for _, c := range AllDeltaSignals {
		if c == v {
			return true
		}
	}
	return false
}

// How far the daemon has got with one aspect for one entity. It is computed from the
// rows that are stored and never recorded, so it cannot go stale, and it is per entity
// and per aspect rather than global: one agent can have goal themes to read while its
// behaviour notes are still collecting.
type AspectReadiness string

const (
	// Fewer notes than a clustering pass needs. The floor is 50 notes for one aspect on
	// one entity, and below it the daemon writes no snapshot at all rather than sorting a
	// handful of sentences into themes nobody should draw a conclusion from.
	AspectReadinessCollecting AspectReadiness = "collecting"
	// Enough notes to cluster, and no snapshot covers this aspect yet. The next pass will
	// write one. Nothing is wrong and nothing needs doing.
	AspectReadinessProcessing AspectReadiness = "processing"
	// A snapshot covers this aspect, so themes exist and every theme route answers.
	AspectReadinessReady AspectReadiness = "ready"
)

// AllAspectReadinesss lists every AspectReadiness value, in declaration order.
var AllAspectReadinesss = []AspectReadiness{AspectReadinessCollecting, AspectReadinessProcessing, AspectReadinessReady}

// String returns the wire representation.
func (v AspectReadiness) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v AspectReadiness) Valid() bool {
	for _, c := range AllAspectReadinesss {
		if c == v {
			return true
		}
	}
	return false
}

// What happened to a theme between the previous snapshot and this one. The daemon
// matches themes across consecutive snapshots by their wording, so a theme keeps its
// id while its wording holds, and this records what that match found. It is computed
// from the two clusterings alone and never from a trace attribute.
type ThemeState string

const (
	// The first snapshot this theme appears in.
	ThemeStateBirth ThemeState = "birth"
	// Present in the previous snapshot and in this one, matched to the same id. The
	// ordinary state of a theme that is holding together.
	ThemeStateContinue ThemeState = "continue"
	// The previous snapshot held one theme where this one holds several. The history
	// point's `split_from` names the theme it came out of.
	ThemeStateSplit ThemeState = "split"
	// Several themes in the previous snapshot are one theme here. The history point's
	// `merged_from` names them.
	ThemeStateMerge ThemeState = "merge"
	// Present in an earlier snapshot, absent from the one immediately before this, and
	// back now under its original id. A theme that comes and goes reads as one theme with
	// gaps rather than as several short-lived ones.
	ThemeStateResurrection ThemeState = "resurrection"
	// The theme was in the previous snapshot and is in none of this one's clusters.
	// Reachable only through a theme's history: a dead theme has no row in the snapshot it
	// died in, so it is never a node in a flow.
	ThemeStateDeath ThemeState = "death"
)

// AllThemeStates lists every ThemeState value, in declaration order.
var AllThemeStates = []ThemeState{ThemeStateBirth, ThemeStateContinue, ThemeStateSplit, ThemeStateMerge, ThemeStateResurrection, ThemeStateDeath}

// String returns the wire representation.
func (v ThemeState) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v ThemeState) Valid() bool {
	for _, c := range AllThemeStates {
		if c == v {
			return true
		}
	}
	return false
}

// What one node of a theme flow stands for. A stage holds its themes, at most one
// noise node and at most one overflow node, and the three together account for every
// trace in the stage, which is what lets the shares add up to one.
type ThemeNodeKind string

const (
	// One theme. It carries a `theme_id`, so it can be filtered on, opened and followed
	// through its history.
	ThemeNodeKindTheme ThemeNodeKind = "theme"
	// The notes in this stage that joined no theme. Noise is not a failure bucket: it
	// holds rare requests, wording nothing else matched and patterns that have not
	// recurred yet, and a large noise share means varied traffic or too few notes rather
	// than something going wrong. Its notes are read from the examples route with `noise`
	// as the theme id.
	ThemeNodeKindNoise ThemeNodeKind = "noise"
	// The themes past `theme_limit_per_stage`, folded into one node so the stage totals
	// still add up. It carries no `theme_id` and cannot be selected, filtered on or
	// opened, because it is a presentation fold and not a group of anything.
	ThemeNodeKindOther ThemeNodeKind = "other"
)

// AllThemeNodeKinds lists every ThemeNodeKind value, in declaration order.
var AllThemeNodeKinds = []ThemeNodeKind{ThemeNodeKindTheme, ThemeNodeKindNoise, ThemeNodeKindOther}

// String returns the wire representation.
func (v ThemeNodeKind) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v ThemeNodeKind) Valid() bool {
	for _, c := range AllThemeNodeKinds {
		if c == v {
			return true
		}
	}
	return false
}

// Why one snapshot is in a landmark selection. A timeline draws a couple of dozen
// points over a range that may hold thousands of passes, so the daemon chooses them
// and says why, rather than handing back the newest few and leaving the reader to
// assume that is the whole story.
type SnapshotLandmarkReason string

const (
	// The oldest snapshot in the requested range. Always included, so a timeline starts
	// where the range does.
	SnapshotLandmarkReasonRangeStart SnapshotLandmarkReason = "range_start"
	// The newest snapshot in the requested range. Always included, so a timeline ends at
	// the current state.
	SnapshotLandmarkReasonRangeEnd SnapshotLandmarkReason = "range_end"
	// Chosen to spread the remaining points evenly over the range. Sampling is by time and
	// not by ordinal, so an hour of frequent passes does not crowd out a quiet week.
	SnapshotLandmarkReasonTimeSample SnapshotLandmarkReason = "time_sample"
)

// AllSnapshotLandmarkReasons lists every SnapshotLandmarkReason value, in declaration order.
var AllSnapshotLandmarkReasons = []SnapshotLandmarkReason{SnapshotLandmarkReasonRangeStart, SnapshotLandmarkReasonRangeEnd, SnapshotLandmarkReasonTimeSample}

// String returns the wire representation.
func (v SnapshotLandmarkReason) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v SnapshotLandmarkReason) Valid() bool {
	for _, c := range AllSnapshotLandmarkReasons {
		if c == v {
			return true
		}
	}
	return false
}

// What an account API key may do. The same six scopes the daemon's configured keys
// carry, under the same names as the `auth` value of each endpoint.
type KeyScope string

const (
	// Write signals, and read nothing. The key that lives in every process that emits a
	// span.
	KeyScopeIngest KeyScope = "ingest"
	// Read stored signals: runs, spans, logs, metrics, scores, analytics and datasets.
	KeyScopeRead KeyScope = "read"
	// Change stored data: scores, feedback, datasets, experiments and acknowledgements.
	KeyScopeWrite KeyScope = "write"
	// Delete data and read the audit log. Also accepted where `execute` or `worker` is
	// needed.
	KeyScopeAdmin KeyScope = "admin"
	// Start, cancel and signal runs, and publish events to them.
	KeyScopeExecute KeyScope = "execute"
	// Claim steps and report their outcome. The key that lives in the process running step
	// bodies.
	KeyScopeWorker KeyScope = "worker"
)

// AllKeyScopes lists every KeyScope value, in declaration order.
var AllKeyScopes = []KeyScope{KeyScopeIngest, KeyScopeRead, KeyScopeWrite, KeyScopeAdmin, KeyScopeExecute, KeyScopeWorker}

// String returns the wire representation.
func (v KeyScope) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v KeyScope) Valid() bool {
	for _, c := range AllKeyScopes {
		if c == v {
			return true
		}
	}
	return false
}

// What a member may do in an organisation. Each role maps onto the six key scopes on
// every project the organisation owns, unless a project role says otherwise.
type OrgRole string

const (
	// Every scope on every project, and everything an admin may do. Only an owner may add
	// or remove an owner, change quotas or delete the organisation.
	OrgRoleOwner OrgRole = "owner"
	// Every scope on every project. Invites and removes members below owner, sets roles,
	// creates and deletes projects, and manages SSO, SCIM and every key in the
	// organisation.
	OrgRoleAdmin OrgRole = "admin"
	// The `ingest`, `read`, `write`, `execute` and `worker` scopes. Everything but
	// `admin`: a member cannot delete data or read the audit log.
	OrgRoleMember OrgRole = "member"
	// The `read` scope only.
	OrgRoleViewer OrgRole = "viewer"
)

// AllOrgRoles lists every OrgRole value, in declaration order.
var AllOrgRoles = []OrgRole{OrgRoleOwner, OrgRoleAdmin, OrgRoleMember, OrgRoleViewer}

// String returns the wire representation.
func (v OrgRole) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v OrgRole) Valid() bool {
	for _, c := range AllOrgRoles {
		if c == v {
			return true
		}
	}
	return false
}

// A role on one project that replaces a member's organisation role there. Owners and
// admins keep their reach on every project whatever a project role says.
type ProjectRole string

const (
	// No access to this project.
	ProjectRoleNone ProjectRole = "none"
	// The `read` scope on this project.
	ProjectRoleViewer ProjectRole = "viewer"
	// Every scope but `admin` on this project.
	ProjectRoleMember ProjectRole = "member"
	// Every scope on this project. It does not let the member manage the organisation.
	ProjectRoleAdmin ProjectRole = "admin"
)

// AllProjectRoles lists every ProjectRole value, in declaration order.
var AllProjectRoles = []ProjectRole{ProjectRoleNone, ProjectRoleViewer, ProjectRoleMember, ProjectRoleAdmin}

// String returns the wire representation.
func (v ProjectRole) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v ProjectRole) Valid() bool {
	for _, c := range AllProjectRoles {
		if c == v {
			return true
		}
	}
	return false
}

// How an organisation's members log in with single sign-on.
type SsoProvider string

const (
	// Any OpenID Connect provider, such as Google, Microsoft Entra ID, Okta or Auth0,
	// found through its issuer's discovery document. Authorization code flow with PKCE,
	// state and nonce.
	SsoProviderOidc SsoProvider = "oidc"
	// GitHub, through an OAuth app. GitHub issues no ID token, so the member is the GitHub
	// user id and the email is the primary verified address GitHub reports.
	SsoProviderGithub SsoProvider = "github"
)

// AllSsoProviders lists every SsoProvider value, in declaration order.
var AllSsoProviders = []SsoProvider{SsoProviderOidc, SsoProviderGithub}

// String returns the wire representation.
func (v SsoProvider) String() string { return string(v) }

// Valid reports whether v is a value this version of the spec defines.
func (v SsoProvider) Valid() bool {
	for _, c := range AllSsoProviders {
		if c == v {
			return true
		}
	}
	return false
}

// Input tokens broken down by how the provider billed them. Every field is optional;
// absent means the provider did not report it, which is different from zero.
type InputTokenDetails struct {
	// Ordinary text tokens: not cached, not audio, not image.
	Text *uint64 `json:"text,omitempty"`
	// Tokens served from a prompt cache. Usually billed at a large discount.
	CacheRead *uint64 `json:"cache_read,omitempty"`
	// Tokens written into a prompt cache. Usually billed at a premium.
	CacheWrite *uint64 `json:"cache_write,omitempty"`
	// Tokens written to a five-minute ephemeral cache, where the provider prices TTLs
	// separately.
	CacheWrite5m *uint64 `json:"cache_write_5m,omitempty"`
	// Tokens written to a one-hour ephemeral cache.
	CacheWrite1h *uint64 `json:"cache_write_1h,omitempty"`
	// Audio input tokens.
	Audio *uint64 `json:"audio,omitempty"`
	// Image input tokens, including rendered PDF pages.
	Image *uint64 `json:"image,omitempty"`
}

// Output tokens broken down by kind.
type OutputTokenDetails struct {
	// Ordinary completion tokens.
	Text *uint64 `json:"text,omitempty"`
	// Hidden reasoning tokens. Billed, and usually invisible in the response.
	Reasoning *uint64 `json:"reasoning,omitempty"`
	// Audio output tokens.
	Audio *uint64 `json:"audio,omitempty"`
	// Image output tokens.
	Image *uint64 `json:"image,omitempty"`
}

// Token counts for one model call. Totals are authoritative; the detail breakdowns
// explain them and are what cost is actually computed from.
type TokenUsage struct {
	// Total input tokens.
	InputTokens *uint64 `json:"input_tokens,omitempty"`
	// Total output tokens.
	OutputTokens *uint64 `json:"output_tokens,omitempty"`
	// Input plus output, when the provider reports a total that differs from the sum.
	TotalTokens *uint64 `json:"total_tokens,omitempty"`
	// How the input tokens were billed.
	InputDetails *InputTokenDetails `json:"input_details,omitempty"`
	// How the output tokens were billed.
	OutputDetails *OutputTokenDetails `json:"output_details,omitempty"`
}

// What a span cost, in a single currency. mcpd computes this from a price book at
// ingest and stores the result, so a later price change never silently rewrites
// history.
type Cost struct {
	// ISO 4217 code. `USD` unless the price book says otherwise.
	Currency string `json:"currency"`
	// Cost attributable to input tokens.
	InputCost *float64 `json:"input_cost,omitempty"`
	// Cost attributable to output tokens.
	OutputCost *float64 `json:"output_cost,omitempty"`
	// The sum, including any meter that is neither input nor output.
	TotalCost *float64 `json:"total_cost,omitempty"`
	// Identifier of the price book used, so a figure can be traced back to the rates that
	// produced it.
	PriceBook *string `json:"price_book,omitempty"`
	// True when mcpd derived the cost from a price book, false when the producer reported
	// it directly.
	IsEstimated bool `json:"is_estimated"`
}

// NewCost returns a Cost with every spec-declared default applied.
func NewCost() Cost {
	return Cost{
		Currency:    "USD",
		IsEstimated: true,
	}
}

// The sampling settings a caller chose for one model call. Recorded because a run
// nobody can reproduce is a run nobody can debug, and the seed and the sampling
// settings are what a replay has to match. Every field is what the caller asked for,
// never what the provider did with it. None of them carries prompt or completion text,
// so a producer running with payload capture off still records all of them.
type ModelParameters struct {
	// The sampling temperature the caller asked for. Absent when the caller left the
	// provider's default in place, which is not the same as sending zero.
	Temperature *float64 `json:"temperature,omitempty"`
	// Nucleus sampling: the caller asked the sampler to consider the smallest set of
	// tokens whose probabilities reach this fraction. Absent when the caller set none.
	TopP *float64 `json:"top_p,omitempty"`
	// The caller asked the sampler to consider only this many of the most likely tokens.
	// Not every provider offers it.
	TopK *uint32 `json:"top_k,omitempty"`
	// The ceiling the caller put on output length. An answer that stops in the middle of a
	// sentence usually stopped here, and this is the first field to read when one does.
	MaxTokens *uint64 `json:"max_tokens,omitempty"`
	// The sampling seed the caller passed. Nothing in mcpd reads it. It is recorded
	// because without it a run cannot be reproduced at all, and a comparison between two
	// runs that differ only in their seed is worth nothing.
	Seed *int64 `json:"seed,omitempty"`
	// The stop sequences the caller supplied. They are short strings the caller wrote, so
	// they are settings rather than payload, and an answer that ends early often ends on
	// one of them. Do not put prompt text here.
	StopSequences []string `json:"stop_sequences,omitempty"`
	// True when the caller constrained the output to a fixed shape, such as a JSON schema
	// or a grammar. The shape itself is not recorded: it can be large and it is
	// caller-owned content, and what a reader needs is whether the constraint was on.
	HasResponseFormat bool `json:"has_response_format"`
}

// NewModelParameters returns a ModelParameters with every spec-declared default applied.
func NewModelParameters() ModelParameters {
	return ModelParameters{
		HasResponseFormat: false,
	}
}

// Why a span failed.
type SpanError struct {
	// A human-readable message. Required: an error with no message is not actionable.
	Message string `json:"message"`
	// The error class or code, such as `RateLimitError` or `ECONNRESET`.
	Kind *string `json:"kind,omitempty"`
	// A stack trace, when the producer captured one.
	Stack *string `json:"stack,omitempty"`
	// Which subsystem raised it.
	Domain *string `json:"domain,omitempty"`
	// Whether the producer considered the failure worth retrying.
	Retryable *bool `json:"retryable,omitempty"`
	// Structured detail. Passes through redaction like any other payload.
	Details json.RawMessage `json:"details,omitempty"`
}

// The descriptive context a signal belongs to. Every field here is queryable and most
// are safe as metric labels; the high-cardinality ones (`user_id`, `session_id`,
// `request_id`) are queryable but blocked from metric labels by the cardinality
// filter.
type Correlation struct {
	// Which project this belongs to. The primary tenancy key.
	Project string `json:"project"`
	// Deployment environment, such as `production` or `staging`.
	Environment *string `json:"environment,omitempty"`
	// The emitting service.
	ServiceName *string `json:"service_name,omitempty"`
	// The emitting service's version, so a regression can be pinned to a release.
	ServiceVersion *string `json:"service_version,omitempty"`
	// A user-facing session. Groups many traces into one conversation.
	SessionID *string `json:"session_id,omitempty"`
	// A conversation thread within a session.
	ThreadID *string `json:"thread_id,omitempty"`
	// The end user. Hashed with a keyed digest at ingest, so runs can still be grouped by
	// customer without storing who the customer is. Filtering on it hashes your query
	// value the same way, so lookup keeps working. Rotating the key deliberately breaks
	// the link.
	UserID *string `json:"user_id,omitempty"`
	// A tenant or workspace inside the calling application.
	ResourceID *string `json:"resource_id,omitempty"`
	// The producer's own identifier for this run.
	RunID *string `json:"run_id,omitempty"`
	// The inbound request that caused this work.
	RequestID *string `json:"request_id,omitempty"`
	// Set when the run was produced by an experiment.
	ExperimentID *string `json:"experiment_id,omitempty"`
	// A release or git sha, for regression attribution.
	Release *string `json:"release,omitempty"`
}

// A reference from this span to another, in this trace or a different one. Links
// express causality that parentage cannot: a batch job caused by twenty requests has
// twenty links and one parent.
type SpanLink struct {
	// The linked trace.
	TraceID string `json:"trace_id"`
	// The linked span, when the link points at one rather than a whole trace.
	SpanID *string `json:"span_id,omitempty"`
	// Why the link exists, such as `follows_from`, `retry_of` or `replay_of`.
	Kind *string `json:"kind,omitempty"`
	// Anything else about the relationship.
	Attributes json.RawMessage `json:"attributes,omitempty"`
}

// Cursor pagination. Cursors are opaque and stable; never construct one by hand.
type Page struct {
	// Pass as `cursor` to fetch the next page. Absent on the last page.
	NextCursor *string `json:"next_cursor,omitempty"`
	// Whether another page exists.
	HasMore bool `json:"has_more"`
	// Total matching rows, when the backend could count them cheaply.
	Total *uint64 `json:"total,omitempty"`
}

// One batch of signals sent to the native ingest endpoint. Any combination of the six
// signals may appear in a single batch; mcpd sorts them out. Supplying `ingest_key`
// makes the whole batch idempotent, so a client that retries after a timeout never
// double-counts.
type IngestBatch struct {
	// Spans to record.
	Spans []Span `json:"spans,omitempty"`
	// Log records to record.
	Logs []LogRecord `json:"logs,omitempty"`
	// Metric observations to record.
	Metrics []MetricPoint `json:"metrics,omitempty"`
	// Machine evaluations to attach.
	Scores []Score `json:"scores,omitempty"`
	// Human evaluations to attach.
	Feedback []Feedback `json:"feedback,omitempty"`
	// Aspect notes to record. A note is a sentence about a trace, so it arrives with that
	// trace's other signals, under the same idempotency key and through the same redactor.
	AspectNotes []AspectNote `json:"aspect_notes,omitempty"`
	// Idempotency key for this batch. Replaying a batch with a key mcpd has already
	// accepted is a no-op that still returns 200.
	IngestKey *string `json:"ingest_key,omitempty"`
}

// Why one signal in a batch was refused.
type IngestError struct {
	// Which signal kind the failure was in.
	Signal SignalType `json:"signal"`
	// Position within that signal's array, so a client can find the offending record.
	Index uint32 `json:"index"`
	// What was wrong, in terms the sender can act on.
	Message string `json:"message"`
}

// What mcpd did with a batch. Counts are per signal, and `rejected` is never silently
// zero: a signal mcpd could not store is always reported with a reason.
type IngestResult struct {
	// How many signals were stored.
	Accepted uint32 `json:"accepted"`
	// How many were refused.
	Rejected uint32 `json:"rejected"`
	// How many were recognised as a replay of an already-accepted batch and skipped.
	Duplicate uint32 `json:"duplicate"`
	// One entry per rejected signal, naming what was wrong and where.
	Errors []IngestError `json:"errors,omitempty"`
}

// A page of traces.
type TraceListResponse struct {
	// The matching traces, in the requested order.
	Traces []Trace `json:"traces"`
	// Cursor and totals.
	Page Page `json:"page"`
}

// One trace and everything attached to it.
type TraceDetail struct {
	// The trace projection.
	Trace Trace `json:"trace"`
	// Every span, ordered by start time then by depth, so a waterfall renders without
	// sorting.
	Spans []Span `json:"spans"`
	// Logs, when `include` asked for them.
	Logs []LogRecord `json:"logs,omitempty"`
	// Scores, when `include` asked for them.
	Scores []Score `json:"scores,omitempty"`
	// Feedback, when `include` asked for them.
	Feedback []Feedback `json:"feedback,omitempty"`
}

// A page of spans.
type SpanListResponse struct {
	// The matching spans.
	Spans []Span `json:"spans"`
	// Cursor and totals.
	Page Page `json:"page"`
}

// A page of log records.
type LogListResponse struct {
	// The matching records.
	Logs []LogRecord `json:"logs"`
	// Cursor and totals.
	Page Page `json:"page"`
}

// One bucket of an aggregated series.
type MetricSeriesPoint struct {
	// Start of the bucket.
	Timestamp time.Time `json:"timestamp"`
	// The aggregated value.
	Value float64 `json:"value"`
	// How many raw observations fell in this bucket. Distinguishes a genuine zero from no
	// data.
	Count *uint64 `json:"count,omitempty"`
	// A trace that contributed to this bucket, so a spike on the chart opens the run that
	// caused it.
	ExemplarTraceID *string `json:"exemplar_trace_id,omitempty"`
}

// A histogram metric read back as distributions: the buckets of every point in the
// window added up, one distribution per label combination, with quantiles read from
// the merged counts. Bucketed points from OpenTelemetry are added bucket by bucket;
// for a cumulative stream only the growth across the window counts. Points with one
// value each, such as mcpd's own derived durations, give exact quantiles and are
// placed into OpenTelemetry's default boundaries.
type MetricHistogram struct {
	// The metric read.
	Name string `json:"name"`
	// UCUM unit of the observations, for axis labelling.
	Unit *string `json:"unit,omitempty"`
	// How many stored points were merged.
	PointsRead uint64 `json:"points_read"`
	// Whether the window held more points than one read merges, so the newest ones were
	// used and the rest left out. Narrow the window or add a filter.
	Truncated bool `json:"truncated"`
	// One distribution per label combination.
	Series []HistogramSeries `json:"series"`
}

// One merged distribution.
type HistogramSeries struct {
	// The `group_by` label values that identify this series. Absent when nothing was
	// grouped.
	Labels json.RawMessage `json:"labels,omitempty"`
	// Upper bound of each bucket but the last, which is unbounded above.
	Bounds []float64 `json:"bounds"`
	// Observations per bucket: one more entry than `bounds`.
	Counts []uint64 `json:"counts"`
	// Every observation counted.
	Count uint64 `json:"count"`
	// Their sum.
	Sum *float64 `json:"sum,omitempty"`
	// The smallest observation, when the points recorded single values.
	Min *float64 `json:"min,omitempty"`
	// The largest observation, when the points recorded single values.
	Max *float64 `json:"max,omitempty"`
	// Median.
	P50 *float64 `json:"p50,omitempty"`
	// 90th percentile.
	P90 *float64 `json:"p90,omitempty"`
	// 95th percentile.
	P95 *float64 `json:"p95,omitempty"`
	// 99th percentile.
	P99 *float64 `json:"p99,omitempty"`
	// Whether the quantiles are exact, read from single observations, or interpolated
	// linearly inside the bucket the rank falls in.
	Exact bool `json:"exact"`
	// Points left out because their bucket boundaries differ from the ones most of this
	// series uses.
	PointsSkipped uint64 `json:"points_skipped"`
}

// A page of stored metric points, as they were recorded.
type MetricPointListResponse struct {
	// The matching points.
	Points []MetricPoint `json:"points"`
	// Cursor and totals.
	Page Page `json:"page"`
}

// One line on a chart: a single label combination over time.
type MetricSeries struct {
	// The label values that identify this series.
	Labels json.RawMessage `json:"labels,omitempty"`
	// Bucketed values, oldest first.
	Points []MetricSeriesPoint `json:"points"`
}

// An aggregated metric query result.
type MetricSeriesResponse struct {
	// The metric queried.
	Name string `json:"name"`
	// UCUM unit, for axis labelling.
	Unit *string `json:"unit,omitempty"`
	// The bucket width actually used, which may differ from the request when mcpd chose
	// it.
	Bucket string `json:"bucket"`
	// One entry per label combination.
	Series []MetricSeries `json:"series"`
}

// A page of scores.
type ScoreListResponse struct {
	// The matching scores.
	Scores []Score `json:"scores"`
	// Cursor and totals.
	Page Page `json:"page"`
}

// A page of feedback.
type FeedbackListResponse struct {
	// The matching feedback.
	Feedback []Feedback `json:"feedback"`
	// Cursor and totals.
	Page Page `json:"page"`
}

// Latency distribution over a window, in milliseconds.
type LatencyStats struct {
	// Median.
	P50 *float64 `json:"p50,omitempty"`
	// Ninetieth percentile.
	P90 *float64 `json:"p90,omitempty"`
	// Ninety-fifth percentile.
	P95 *float64 `json:"p95,omitempty"`
	// Ninety-ninth percentile.
	P99 *float64 `json:"p99,omitempty"`
	// Arithmetic mean. Reported alongside quantiles, never instead of them.
	Mean *float64 `json:"mean,omitempty"`
	// Slowest observation in the window.
	Max *float64 `json:"max,omitempty"`
}

// One row of a grouped breakdown.
type GroupedStat struct {
	// The group's value, such as a model id.
	Key string `json:"key"`
	// How many runs fell in this group.
	Count uint64 `json:"count"`
	// How many of them failed.
	ErrorCount uint64 `json:"error_count"`
	// Spend attributable to this group.
	TotalCost *float64 `json:"total_cost,omitempty"`
	// Input tokens for this group.
	InputTokens *uint64 `json:"input_tokens,omitempty"`
	// Output tokens for this group.
	OutputTokens *uint64 `json:"output_tokens,omitempty"`
	// Latency distribution within this group.
	Latency *LatencyStats `json:"latency,omitempty"`
}

// Headline numbers for a window. One request backs the overview screen.
type AnalyticsSummary struct {
	// Start of the window.
	From time.Time `json:"from"`
	// End of the window.
	To time.Time `json:"to"`
	// Runs started in the window.
	TraceCount uint64 `json:"trace_count"`
	// Steps recorded in the window.
	SpanCount uint64 `json:"span_count"`
	// Runs that failed.
	ErrorCount uint64 `json:"error_count"`
	// Failures as a fraction of runs, between 0 and 1.
	ErrorRate float64 `json:"error_rate"`
	// Run duration distribution.
	Latency LatencyStats `json:"latency"`
	// Input tokens consumed.
	InputTokens uint64 `json:"input_tokens"`
	// Output tokens produced.
	OutputTokens uint64 `json:"output_tokens"`
	// Total spend in the window.
	TotalCost float64 `json:"total_cost"`
	// Currency of `total_cost`.
	Currency string `json:"currency"`
	// Per-group rows, when `group_by` was supplied.
	Groups []GroupedStat `json:"groups,omitempty"`
}

// NewAnalyticsSummary returns a AnalyticsSummary with every spec-declared default applied.
func NewAnalyticsSummary() AnalyticsSummary {
	return AnalyticsSummary{
		Currency: "USD",
	}
}

// Spend in one time bucket for one group.
type CostBucket struct {
	// Start of the bucket.
	Timestamp time.Time `json:"timestamp"`
	// The group this row belongs to.
	Key string `json:"key"`
	// Spend in this bucket.
	TotalCost float64 `json:"total_cost"`
	// Input tokens in this bucket.
	InputTokens *uint64 `json:"input_tokens,omitempty"`
	// Output tokens in this bucket.
	OutputTokens *uint64 `json:"output_tokens,omitempty"`
}

// Where the money went.
type CostBreakdown struct {
	// Start of the window.
	From time.Time `json:"from"`
	// End of the window.
	To time.Time `json:"to"`
	// Currency of every figure in this response.
	Currency string `json:"currency"`
	// Total spend across the window.
	TotalCost float64 `json:"total_cost"`
	// Totals per group, largest first.
	Groups []GroupedStat `json:"groups"`
	// A time series, when `bucket` was supplied.
	Series []CostBucket `json:"series,omitempty"`
}

// NewCostBreakdown returns a CostBreakdown with every spec-declared default applied.
func NewCostBreakdown() CostBreakdown {
	return CostBreakdown{
		Currency: "USD",
	}
}

// What a deletion removed.
type DeleteResult struct {
	// How many records were removed, across every signal.
	Deleted uint32 `json:"deleted"`
}

// Whether the server is alive and able to serve.
type HealthStatus struct {
	// `ok` or `degraded`.
	Status string `json:"status"`
	// The mcpd version serving this request.
	Version string `json:"version"`
	// The data-model spec version this build implements. A client can refuse to talk to an
	// incompatible server.
	SpecVersion string `json:"spec_version"`
	// How long this process has been running.
	UptimeSeconds uint64 `json:"uptime_seconds"`
	// Which storage backend is configured.
	Storage *string `json:"storage,omitempty"`
	// What is degraded, when something is.
	Detail *string `json:"detail,omitempty"`
	// True when this daemon offers open sign-up: anyone may create an account at `POST
	// /v1/api/account/signup` and log in at `POST /v1/api/account/login`. Absent when it
	// does not, which is the default. The interface reads this to decide whether to show
	// its sign-up and log-in screens.
	Signup *bool `json:"signup,omitempty"`
	// True when this daemon has accounts, with or without open sign-up. Absent when it
	// does not. The interface reads this to show its log-in screen.
	Accounts *bool `json:"accounts,omitempty"`
	// True when at least one organisation on this daemon logs in with single sign-on, so
	// the log-in screen offers it. Absent otherwise.
	Sso *bool `json:"sso,omitempty"`
}

// One scorer mcpd can run, as described to a caller.
type ScorerInfo struct {
	// Stable identifier, recorded on every score it produces.
	ID string `json:"id"`
	// What it measures, in one line.
	Description string `json:"description"`
	// Whether it needs settings to mean anything. A scorer that needs an expected answer
	// cannot run without one.
	NeedsConfig bool `json:"needs_config"`
	// How the scorer reaches its answer.
	Kind ScorerKind `json:"kind"`
	// Which way its value is better.
	Direction ScoreDirection `json:"direction"`
	// The `ScorerConfig` settings it reads. Empty when it needs none.
	ConfigKeys []string `json:"config_keys"`
	// Whether this daemon can run it now. A judged scorer is unavailable until a `judge`
	// block names an endpoint and a model.
	Available bool `json:"available"`
}

// NewScorerInfo returns a ScorerInfo with every spec-declared default applied.
func NewScorerInfo() ScorerInfo {
	return ScorerInfo{
		Available: true,
	}
}

// Settings shared by the scorers that need them. Every field is optional; a scorer
// that needs one and does not get it declines instead of guessing.
type ScorerConfig struct {
	// Text the answer must contain, for `contains`.
	Needle *string `json:"needle,omitempty"`
	// The expected answer, for `exact_match`.
	Expected *string `json:"expected,omitempty"`
	// A budget in milliseconds, for `latency_budget`.
	MaxMs *float64 `json:"max_ms,omitempty"`
	// A budget in currency units, for `cost_budget`.
	MaxCost *float64 `json:"max_cost,omitempty"`
	// The most tool calls a healthy run should make, for `tool_efficiency`.
	MaxToolCalls *uint32 `json:"max_tool_calls,omitempty"`
	// The kind of span that must be present in the run, for `span_present`.
	SpanKind *SpanKind `json:"span_kind,omitempty"`
	// The span name that must be present in the run, for `span_present`. Matched exactly
	// against `Span.name`.
	SpanName *string `json:"span_name,omitempty"`
	// The tool that must have run and ended `ok`, for `tool_succeeded`. Matched exactly
	// against the `name` of a span whose kind is `tool_call`, `client_tool_call`,
	// `provider_tool_call` or `mcp_tool_call`.
	ToolName *string `json:"tool_name,omitempty"`
	// Text the answer must not contain, for `excludes`. Any one of them fails the run.
	Forbidden []string `json:"forbidden"`
	// Words or phrases the answer should mention, for `keyword_coverage`. The score is the
	// share that appear.
	Keywords []string `json:"keywords"`
	// A regular expression the answer must match somewhere, for `matches_pattern`.
	Pattern *string `json:"pattern,omitempty"`
	// The tools the run should call, for `tool_call_accuracy`, and the order it should
	// call them in, for `tool_order`. Matched exactly against tool-call span names.
	ExpectedTools []string `json:"expected_tools"`
	// Tools the run must not call, for `tool_not_called`.
	ForbiddenTools []string `json:"forbidden_tools"`
	// The shortest acceptable answer in characters, for `answer_length`.
	MinChars *uint32 `json:"min_chars,omitempty"`
	// The longest acceptable answer in characters, for `answer_length`.
	MaxChars *uint32 `json:"max_chars,omitempty"`
	// A JSON Schema the answer must satisfy, for `json_schema`. `type`, `properties`,
	// `required`, `items`, `enum`, `minimum`, `maximum`, `minLength`, `maxLength` and
	// `additionalProperties: false` are checked.
	JSONSchema json.RawMessage `json:"json_schema,omitempty"`
	// The passages the answer should rest on, for the judged scorers that read context.
	// When empty, the outputs of the run's `retrieval` and `rag_vector_operation` spans
	// are used.
	Context []string `json:"context"`
	// Case sensitivity for text comparisons.
	CaseSensitive bool `json:"case_sensitive"`
}

// NewScorerConfig returns a ScorerConfig with every spec-declared default applied.
func NewScorerConfig() ScorerConfig {
	return ScorerConfig{
		CaseSensitive: false,
	}
}

// A request to run scorers over runs that are already stored. Scoring reads stored
// runs rather than running inline, so an evaluation can never add latency to the
// request it is judging, and a scorer that fails can never fail a user's request.
type ScoreRunRequest struct {
	// Which project's runs to score. Defaults to the server's default project.
	Project *string `json:"project,omitempty"`
	// Score exactly these runs. Takes precedence over the window.
	TraceIDs []string `json:"trace_ids"`
	// Otherwise, score runs started after this time.
	From *time.Time `json:"from,omitempty"`
	// Most runs to score in one request, newest first, when `trace_ids` is empty. Unset,
	// it is 100 for deterministic scorers, and for a pass with a judged scorer it is as
	// many runs as fit in the request deadline. A judged pass that names more runs than
	// fit, through `limit` or `trace_ids`, is refused with the cap.
	Limit *uint32 `json:"limit,omitempty"`
	// Which scorers to run, by id. Empty means every scorer that needs no configuration.
	Scorers []string `json:"scorers"`
	// Settings for the scorers that need them.
	Config *ScorerConfig `json:"config,omitempty"`
	// What to record as the origin of these scores.
	Source *ScoreSource `json:"source,omitempty"`
	// Report what would happen and store nothing.
	DryRun bool `json:"dry_run"`
}

// NewScoreRunRequest returns a ScoreRunRequest with every spec-declared default applied.
func NewScoreRunRequest() ScoreRunRequest {
	return ScoreRunRequest{
		DryRun: false,
	}
}

// A scorer that declined to answer on one run.
type SkippedScorer struct {
	// Which scorer declined.
	ScorerID string `json:"scorer_id"`
	// Which run it declined on.
	TraceID string `json:"trace_id"`
	// Why. Never silent: a scorer that cannot answer is more interesting than one that
	// returned a number.
	Reason string `json:"reason"`
}

// What one scoring pass did.
type ScoreRunResult struct {
	// Runs looked at.
	TracesScored uint32 `json:"traces_scored"`
	// Scores produced and stored.
	ScoresWritten uint32 `json:"scores_written"`
	// Every scorer that declined, with its reason.
	Skipped []SkippedScorer `json:"skipped"`
	// The scores themselves, so the caller sees them without a second read.
	Scores []Score `json:"scores"`
}

// How one scorer moved between two experiments.
type ScorerDelta struct {
	// Which scorer.
	ScorerID string `json:"scorer_id"`
	// Mean on the baseline, when it ran there.
	BaselineMean *float64 `json:"baseline_mean,omitempty"`
	// Number of baseline observations.
	BaselineN uint64 `json:"baseline_n"`
	// Mean on the candidate.
	CandidateMean *float64 `json:"candidate_mean,omitempty"`
	// Number of candidate observations.
	CandidateN uint64 `json:"candidate_n"`
	// Candidate minus baseline.
	Delta *float64 `json:"delta,omitempty"`
	// Whether the sample is large enough for the delta to mean anything. A deliberately
	// blunt rule rather than a significance test: below twenty observations on either
	// side, mcpd says the comparison is not decidable rather than dressing noise up as a
	// finding.
	Decidable bool `json:"decidable"`
	// Which way this scorer's value is better, from the catalogue. Scorers the daemon does
	// not know read as higher is better.
	Direction ScoreDirection `json:"direction"`
	// Lower end of the 95% confidence interval on `delta`, from Welch's t-test. Absent
	// when either side has fewer than two observations.
	CiLow *float64 `json:"ci_low,omitempty"`
	// Upper end of that interval.
	CiHigh *float64 `json:"ci_high,omitempty"`
	// Two-sided p-value for the difference in means.
	PValue *float64 `json:"p_value,omitempty"`
	// Whether this scorer moved the wrong way: decidable, and the whole interval on the
	// worse side of zero.
	Regressed bool `json:"regressed"`
}

// NewScorerDelta returns a ScorerDelta with every spec-declared default applied.
func NewScorerDelta() ScorerDelta {
	return ScorerDelta{
		Regressed: false,
	}
}

// Two experiments, compared. The point of an experiment is the comparison, so this
// reports the delta per scorer beside the sample sizes. A mean that moved on four runs
// is noise, and printing the counts next to it is what stops it being read as a
// result.
type ExperimentComparison struct {
	// The baseline's identifier.
	Baseline string `json:"baseline"`
	// The candidate's identifier.
	Candidate string `json:"candidate"`
	// One row per scorer present on either side.
	Scorers []ScorerDelta `json:"scorers"`
	// Whether any decidable scorer moved down.
	Regressed bool `json:"regressed"`
}

// A request to turn stored runs into dataset cases.
type CaptureRequest struct {
	// The runs to capture. Each becomes one item, keeping its trace id so anyone can go
	// back and read what actually happened.
	TraceIDs []string `json:"trace_ids"`
	// Record each run's own output as the expected answer. Leave this off for a run that
	// went wrong: capturing a bad answer as the reference would enshrine the bug.
	UseOutputAsExpected bool `json:"use_output_as_expected"`
}

// NewCaptureRequest returns a CaptureRequest with every spec-declared default applied.
func NewCaptureRequest() CaptureRequest {
	return CaptureRequest{
		UseOutputAsExpected: false,
	}
}

// What a capture added to a dataset.
type CaptureResult struct {
	// Items added.
	Captured uint32 `json:"captured"`
	// Runs asked for. A gap means a run was not found.
	Requested uint32 `json:"requested"`
}

// How many cases a dataset gained.
type ItemsAdded struct {
	// Items added.
	Added uint32 `json:"added"`
}

// What an erasure request removed, counted per signal. The subject is not echoed back:
// it has been erased, and repeating it in a response that may be logged on the way out
// would write it down again.
type ErasureResult struct {
	// The project the erasure ran in. No other project was read or touched.
	Project string `json:"project"`
	// How many runs were removed.
	Traces uint32 `json:"traces"`
	// How many spans were removed, with the prompts and completions they carried.
	Spans uint32 `json:"spans"`
	// How many log lines were removed.
	Logs uint32 `json:"logs"`
	// How many metric points were removed.
	Metrics uint32 `json:"metrics"`
	// How many scores were removed.
	Scores uint32 `json:"scores"`
	// How many pieces of feedback were removed.
	Feedback uint32 `json:"feedback"`
	// The total across every signal, so a caller that wants one number does not add them
	// up.
	Deleted uint32 `json:"deleted"`
	// How many durable runs were removed from the run store, each with its step attempts,
	// event log, deadlines, waits, the events it consumed and any span still queued for
	// the trace store. A run is matched on the `user_id` in its correlation, as it was
	// started with it, and removed with the whole tree of subworkflow runs it belongs to.
	// Not part of `deleted`, which counts the trace store. Absent when this daemon does
	// not serve runs.
	RunsErased *uint32 `json:"runs_erased,omitempty"`
	// How many runs attributed to this person were left untouched because a run in their
	// tree has not ended. Removing a live run would corrupt the workflow it is part of, so
	// these are counted rather than skipped in silence, and `note` names the root runs.
	// Cancel them, or let them end, then send the erasure again. Absent when this daemon
	// does not serve runs.
	RunsInFlight *uint32 `json:"runs_in_flight,omitempty"`
	// How many aspect notes were removed, matched by the run they describe and by the
	// person stamped on them. Not part of `deleted`. Absent only from a store that holds
	// no notes.
	AspectNotesErased *uint32 `json:"aspect_notes_erased,omitempty"`
	// How many dataset cases captured from an erased run were emptied in place. They are
	// not deleted, because a case was kept on purpose, so they are not part of `deleted`.
	DatasetItemsErased *uint32 `json:"dataset_items_erased,omitempty"`
	// The datasets those emptied cases sit in, so the reply can say where they are.
	Datasets []string `json:"datasets"`
	// True when the storage backend applies the deletes after this reply is sent, as
	// ClickHouse does, so the counts say what matched rather than what has already left
	// the disk.
	DeletesPending *bool `json:"deletes_pending,omitempty"`
	// How many stored identifiers this request could not have matched, because they were
	// written in the other form, hashed or plain, from the one given. A number above zero
	// means the counts may not be the whole answer.
	UnmatchableIdentifiers *uint32 `json:"unmatchable_identifiers,omitempty"`
	// Why the numbers above may not be the whole answer, in sentences an operator can
	// repeat. Absent when they are the whole answer.
	Note *string `json:"note,omitempty"`
}

// A definition as you post it: the graph, and the id and version to store it under.
// The body is validated at write time against `spec/definition.schema.json`; a body
// the walker cannot run is refused with a 400 naming the field, and unknown fields are
// refused, so a guard that is spelled wrong is an error and not a run that skips its
// guard.
type DefinitionDocument struct {
	// The definition's identifier within its project. Matches `^[a-z][a-z0-9_]*$`.
	ID string `json:"id"`
	// What kind of definition. Absent means `workflow`, the only kind version 1 serves.
	Kind *DefinitionKind `json:"kind,omitempty"`
	// The version to store the body under. Absent asks the daemon to choose: the version
	// whose stored body is identical, or one past the highest stored. A named version that
	// is free is taken; one held by an identical body is confirmed; one held by a
	// different body is a 409, because a stored version never changes.
	Version *uint32 `json:"version,omitempty"`
	// The document itself: `input_schema`, `limits`, `steps` and `output`. The definition
	// schema names every field.
	Body json.RawMessage `json:"body"`
}

// Where a posted definition landed.
type DefinitionVersion struct {
	// The definition's identifier.
	ID string `json:"id"`
	// The version the body is stored under.
	Version uint32 `json:"version"`
	// True when the daemon chose the number because the body was new. False when the body
	// already existed at this version, or you named the version yourself.
	Assigned bool `json:"assigned"`
}

// The frontier of a run: which position in the definition's `steps` it has reached,
// and what sits there.
type RunCursor struct {
	// The index into `steps`, or their count at the end of the graph.
	Index uint32 `json:"index"`
	// What is at that position.
	Construct ConstructKind `json:"construct"`
	// The step this position belongs to, when the construct has one. A fan-out names its
	// parent step; the branch and item keys are on the attempts.
	StepKey *string `json:"step_key,omitempty"`
}

// A deadline the daemon holds for a run.
type RunTimer struct {
	// Which run.
	RunID string `json:"run_id"`
	// The step the deadline belongs to. Absent for a run-level deadline.
	StepKey *string `json:"step_key,omitempty"`
	// Which deadline.
	Kind TimerKind `json:"kind"`
	// When it fires.
	DueAt time.Time `json:"due_at"`
}

// A suspension waiting for an event.
type RunWait struct {
	// Which run.
	RunID string `json:"run_id"`
	// The `wait_event` step that is waiting.
	StepKey string `json:"step_key"`
	// The event name an inbound event must match.
	Event string `json:"event"`
	// The rendered key that picks this run out of every run waiting on that event.
	Key string `json:"key"`
	// When waiting stops and the node's `on_timeout` applies.
	ExpiresAt time.Time `json:"expires_at"`
}

// One run and everything the daemon holds for it.
type RunDetail struct {
	// The run.
	Run Run `json:"run"`
	// Every attempt, in the order they were written.
	Attempts []StepAttempt `json:"attempts"`
	// The deadlines currently held. Empty once the run has ended.
	Timers []RunTimer `json:"timers"`
	// The suspensions currently open. Empty once the run has ended.
	Waits []RunWait `json:"waits"`
}

// A page of runs.
type RunListResponse struct {
	// The matching runs, newest first.
	Runs []Run `json:"runs"`
	// Cursor and totals.
	Page Page `json:"page"`
}

// A page of a run's event log.
type RunEventPage struct {
	// The rows after the requested sequence number, in order.
	Events []RunEvent `json:"events"`
	// Whether rows beyond the last one here existed when the page was read. A run that is
	// still moving will write more.
	HasMore bool `json:"has_more"`
}

// One edge of a cause chain: an ancestor step, and the template path through which the
// step below it read that ancestor's output. An ancestor read through two paths
// appears twice, once per path.
type CauseLink struct {
	// The ancestor step, keyed the way the journal keys it: `refund/issue`, `classify[3]`,
	// `poll[7]/classify`.
	StepKey string `json:"step_key"`
	// Where the ancestor's latest attempt ended.
	Status AttemptStatus `json:"status"`
	// How many attempts were made at the ancestor. More than one on a step that ended
	// `succeeded` is worth reading: it got there, but not the first time.
	Attempts uint32 `json:"attempts"`
	// The path as the definition writes it inside `${...}`, such as
	// `steps.classify.output.kind`. The definition declares this edge, so the chain is
	// read rather than inferred from timing.
	TemplatePath string `json:"template_path"`
	// The step key that read this ancestor through `template_path`. On a direct edge it is
	// the step the answer is about; deeper in the chain it is another ancestor.
	ConsumedBy string `json:"consumed_by"`
	// How far this ancestor sits from the step the answer is about. `1` is read directly,
	// `2` is read by something the step read directly, and so on.
	Depth uint32 `json:"depth"`
	// True when this ancestor ended `succeeded` and every step between it and the failed
	// step ended `succeeded` too. Nothing on that path is marked failed, so no status code
	// points at it and the value it produced is the suspect. It is not the same as
	// `status` being `succeeded`: an ancestor sitting behind a step that did fail is not a
	// silent producer, because that failure already explains the run.
	SilentProducer bool `json:"silent_producer"`
	// The JSON type `template_path` resolved to on the ancestor's stored output. The value
	// itself is never returned, only its shape. Absent when there is no output to read,
	// which is the case for a step that never ran and for one that failed.
	ValueKind *ValueKind `json:"value_kind,omitempty"`
	// True when that value was null, an empty string, an empty array or an empty object. A
	// step that ended `succeeded` and handed the step below it nothing is the failure a
	// status code cannot catch, and this is the field that shows it. Absent for the same
	// reason `value_kind` is.
	ValueEmpty *bool `json:"value_empty,omitempty"`
}

// Why one step of a run failed, answered by walking backwards along the data
// dependencies its definition declares. The daemon reads the run journal and the
// pinned definition version, both of which it has to hold to render `${...}` templates
// at all, so this answer is available with span payload capture off and with no model,
// prompt or completion involved.
type StepCause struct {
	// The run this answer is about.
	RunID string `json:"run_id"`
	// The step this answer is about, as it was asked for.
	StepKey string `json:"step_key"`
	// Where that step's latest attempt ended.
	Status AttemptStatus `json:"status"`
	// How many attempts were made at it.
	Attempts uint32 `json:"attempts"`
	// Why the latest attempt failed, when it did. Absent on a step that did not fail,
	// because the endpoint answers for any step, not only a failed one.
	Error *StepError `json:"error,omitempty"`
	// The ancestors whose output this step consumed, directly or transitively. Ordered by
	// `depth` ascending, then by the order the paths appear in the definition, so two
	// calls against one run return the same list in the same order. Empty when the step
	// reads nothing but the run input.
	Chain []CauseLink `json:"chain"`
	// True when the walk stopped at `max_depth` with ancestors left unvisited, so `chain`
	// is a prefix of the answer rather than the whole of it.
	Truncated bool `json:"truncated"`
}

// A request to start a run.
type StartRunRequest struct {
	// Which project. A selector: refused with 403 on a mismatch with the key's grant,
	// filled in for a single-project key, and a 400 when a multi-project key names none.
	Project *string `json:"project,omitempty"`
	// Which definition to run.
	DefinitionID string `json:"definition_id"`
	// Which version. Absent pins the highest stored version at this moment, and the run
	// records it.
	Version *uint32 `json:"version,omitempty"`
	// The run's input. Validated against the definition's `input_schema` when it declares
	// one; a failure is a 400.
	Input json.RawMessage `json:"input,omitempty"`
	// Context to store on the run and copy onto every span the daemon writes for it.
	// `project` and `run_id` inside it are ignored: the run's own values win.
	Correlation *Correlation `json:"correlation,omitempty"`
	// Makes the start idempotent per project for 24 hours: a repost returns the same run
	// and starts nothing, and two simultaneous posts of one key produce one run.
	IdempotencyKey *string `json:"idempotency_key,omitempty"`
}

// A worker asking for work.
type ClaimRequest struct {
	// Which project. A selector: refused with 403 on a mismatch with the key's grant,
	// filled in for a single-project key, and a 400 when a multi-project key names none.
	Project *string `json:"project,omitempty"`
	// Who is asking. Recorded on the attempt as `lease_owner` and in the run's event log.
	// Nothing else about the worker is stored.
	WorkerID string `json:"worker_id"`
	// The most leases to hand back, 1 to 100.
	Max uint32 `json:"max"`
	// How long to hold the request open waiting for work, 0 to 60000 milliseconds. The
	// daemon holds a claim for at most four fifths of its request timeout, 24000 at the
	// default 30 seconds, so a longer wait is answered early rather than cut off with a
	// 503. An empty answer means nothing became runnable in time: reissue the claim.
	WaitMs uint32 `json:"wait_ms"`
	// Restrict the claim to one run, which is how a script drives its own run inline.
	RunID *string `json:"run_id,omitempty"`
}

// A step handed to a worker. Every later call about the attempt is addressed by
// `lease_id`.
type StepLease struct {
	// The address of every later call about this attempt.
	LeaseID string `json:"lease_id"`
	// Which run.
	RunID string `json:"run_id"`
	// Which project the run belongs to.
	Project string `json:"project"`
	// Which definition the run is executing.
	DefinitionID string `json:"definition_id"`
	// Which version. Fail the lease with class `unservable` if you do not serve it.
	DefinitionVersion uint32 `json:"definition_version"`
	// The step to run. A worker that does not recognise it fails the lease with class
	// `unservable` rather than guessing.
	StepKey string `json:"step_key"`
	// Whether this is a plain task or an agent turn.
	Kind StepKind `json:"kind"`
	// Which attempt this is. The fence: a call about this lease is refused once a later
	// attempt exists.
	Attempt uint32 `json:"attempt"`
	// Under `at_most_once`, post `started` and wait for its 200 before running the body.
	Effect Effect `json:"effect"`
	// When the lease lapses. Heartbeat every `heartbeat_ms` to keep it.
	ExpiresAt time.Time `json:"expires_at"`
	// The step's own timeout, when the node declares one. It bounds the attempt whatever
	// the heartbeats say.
	DeadlineMs *uint64 `json:"deadline_ms,omitempty"`
	// A key stable across every attempt of this step: the SHA-256 of `run_id:step_key`.
	// Pass it to any downstream call that can deduplicate.
	IdempotencyKey string `json:"idempotency_key"`
	// The node's `input` template rendered for this attempt, or the run's input when the
	// node declares none.
	Input json.RawMessage `json:"input,omitempty"`
	// The ended attempts of earlier steps, keyed by step id: `{ status, output?, error? }`
	// for the latest attempt of each, or `{ status: skipped }`. Only ended attempts
	// appear.
	Prior json.RawMessage `json:"prior"`
	// The run's state object as it stands.
	State json.RawMessage `json:"state,omitempty"`
	// The run's correlation, with `run_id` set. Put it on every span you record inside
	// this step.
	Correlation Correlation `json:"correlation"`
	// The run's trace. Set it on every span you record inside this step.
	TraceID string `json:"trace_id"`
	// The span the daemon has already opened for this attempt. Set it as `parent_span_id`
	// on your own spans, so a model call or a tool call nests under the step.
	SpanID string `json:"span_id"`
	// The run's root span, which this attempt's span hangs off.
	ParentSpanID string `json:"parent_span_id"`
}

// What a claim answered.
type ClaimResponse struct {
	// Zero to `max` leases. Empty when nothing became runnable within `wait_ms`.
	Leases []StepLease `json:"leases"`
}

// What `started` and `heartbeat` answer.
type LeaseState struct {
	// The lease's new expiry.
	ExpiresAt time.Time `json:"expires_at"`
	// True when this call found the attempt `orphaned` and restored it to the state it
	// came from.
	Reclaimed bool `json:"reclaimed"`
}

// Token usage a worker reports on `complete`. The daemon prices it from its price book
// and charges the attempt, the run and the root of the run tree.
type StepUsage struct {
	// Total input tokens.
	InputTokens *uint64 `json:"input_tokens,omitempty"`
	// Total output tokens.
	OutputTokens *uint64 `json:"output_tokens,omitempty"`
	// Input plus output, when the provider reports a total that differs from the sum.
	TotalTokens *uint64 `json:"total_tokens,omitempty"`
	// How the input tokens were billed.
	InputDetails *InputTokenDetails `json:"input_details,omitempty"`
	// How the output tokens were billed.
	OutputDetails *OutputTokenDetails `json:"output_details,omitempty"`
	// The model that produced the usage, which the price book needs to price it.
	Model *string `json:"model,omitempty"`
	// The provider that served the model.
	Provider *string `json:"provider,omitempty"`
}

// What a worker posts to complete a step.
type StepResult struct {
	// The step's output. Later steps read it as `steps.<id>.output`.
	Output json.RawMessage `json:"output,omitempty"`
	// An object to merge shallowly into the run's state.
	State json.RawMessage `json:"state,omitempty"`
	// Token usage to price and charge.
	Usage *StepUsage `json:"usage,omitempty"`
}

// Why a step failed, as the worker reports it.
type StepError struct {
	// What kind of failure. Decides whether the daemon retries.
	Class StepErrorClass `json:"class"`
	// What went wrong, for the person reading the run.
	Message string `json:"message"`
	// Structured detail. Stored on the attempt.
	Details json.RawMessage `json:"details,omitempty"`
	// The least the daemon should wait before the next attempt, when the worker knows: a
	// rate limit's reset, say. Honoured when it is larger than the backoff. At most
	// 315360000000 (3650 days), the bound on every duration in a definition; a larger
	// value is a 400 naming this field.
	RetryAfterMs *uint64 `json:"retry_after_ms,omitempty"`
}

// What a worker posts to fail a step.
type StepFailure struct {
	// Why it failed.
	Error StepError `json:"error"`
}

// What `complete` and `fail` answer.
type StepAck struct {
	// Where the run is after this call.
	RunStatus RunStatus `json:"run_status"`
	// `claim` when more work is runnable now, `idle` otherwise.
	Next string `json:"next"`
	// After a `fail`: how many attempts the node's retry budget still allows.
	AttemptsLeft *uint32 `json:"attempts_left,omitempty"`
}

// The body of a 409 from a worker call. `reason` says what is true and what the worker
// should do next.
type StepConflict struct {
	// Always `conflict`.
	Error string `json:"error"`
	// Which conflict.
	Reason ConflictReason `json:"reason"`
	// The reason, in words.
	Message string `json:"message"`
	// The attempt the lease addressed.
	Attempt uint32 `json:"attempt"`
	// Where that attempt is, when it still exists. Under `already_applied` this says how
	// the step ended.
	AttemptStatus *AttemptStatus `json:"attempt_status,omitempty"`
	// Where the run is.
	RunStatus RunStatus `json:"run_status"`
}

// An event for whichever run is waiting for it.
type PublishEventRequest struct {
	// Which project. A selector: refused with 403 on a mismatch with the key's grant,
	// filled in for a single-project key, and a 400 when a multi-project key names none.
	Project *string `json:"project,omitempty"`
	// The event name, matched against open waits.
	Event string `json:"event"`
	// Matched against the rendered `key` of each waiting run. Delivered to the matching
	// run whose wait expires soonest.
	Key string `json:"key"`
	// Becomes the waiting step's output.
	Payload json.RawMessage `json:"payload,omitempty"`
	// Your own id for this event. A supplied id seen before answers `duplicate` and writes
	// nothing, whether or not a run is waiting. Absent, the daemon derives one from
	// `project`, `event`, `key` and `payload`, and a derived id counts as a duplicate only
	// when no run is waiting, because two identical approvals for two runs on one key are
	// two events. Send your own id from any handler that can be retried.
	EventID *string `json:"event_id,omitempty"`
}

// What publishing an event did.
type EventReceipt struct {
	// Delivered, parked or duplicate.
	Outcome EventOutcome `json:"outcome"`
	// The id used, supplied or derived.
	EventID string `json:"event_id"`
}

// An event for one run, addressed by run id rather than by key. The daemon reads the
// run's open wait on `event`, takes its key, and routes the event as a published one.
type SignalRequest struct {
	// The event name. The run must hold an open wait on it.
	Event string `json:"event"`
	// Becomes the waiting step's output.
	Payload json.RawMessage `json:"payload,omitempty"`
	// Your own id for this event, deduplicated as on a published event.
	EventID *string `json:"event_id,omitempty"`
}

// A worker announcing the definitions it serves.
type WorkerRegistration struct {
	// Which project. A selector: refused with 403 on a mismatch with the key's grant,
	// filled in for a single-project key, and a 400 when a multi-project key names none.
	Project *string `json:"project,omitempty"`
	// Who is registering. Not stored: dispatch reads the runnable set, never a worker
	// table.
	WorkerID string `json:"worker_id"`
	// Each is stored under the versioning rule, exactly as if posted to the definitions
	// endpoint.
	Definitions []DefinitionDocument `json:"definitions"`
}

// What registering answered.
type WorkerRegistered struct {
	// The id you registered with.
	WorkerID string `json:"worker_id"`
	// Where each definition landed, in the order posted.
	Registered []DefinitionVersion `json:"registered"`
	// How long a lease lasts without a heartbeat.
	LeaseTtlMs uint64 `json:"lease_ttl_ms"`
	// How often to heartbeat a held lease: a third of `lease_ttl_ms`.
	HeartbeatMs uint64 `json:"heartbeat_ms"`
}

// A saved condition the daemon evaluates on a schedule. Monitors are configured in the
// daemon's configuration file, not through the API.
type Monitor struct {
	// Stable identifier.
	ID string `json:"id"`
	// What a person should read when it trips.
	Name string `json:"name"`
	// Which project it watches.
	Project string `json:"project"`
	// What it measures.
	Watch MonitorWatch `json:"watch"`
	// Which scorer, when `watch` is `score_mean`.
	ScorerID *string `json:"scorer_id,omitempty"`
	// Which direction trips it.
	Compare MonitorCompare `json:"compare"`
	// The threshold.
	Threshold float64 `json:"threshold"`
	// How far back each evaluation looks.
	WindowSeconds uint32 `json:"window_seconds"`
	// The fewest runs the window must contain before the monitor will trip. Without it a
	// single failed run in a quiet hour is a 100% error rate, and the first thing anyone
	// does with an alert that cries wolf is mute it.
	MinSamples uint64 `json:"min_samples"`
	// Where to POST when it trips or clears. Set as a full URL in the config file; the API
	// returns only its host, because a webhook URL is often its own credential.
	Webhook *string `json:"webhook,omitempty"`
	// Whether it is evaluated at all.
	Enabled bool `json:"enabled"`
}

// NewMonitor returns a Monitor with every spec-declared default applied.
func NewMonitor() Monitor {
	return Monitor{
		WindowSeconds: 300,
		MinSamples:    20,
		Enabled:       true,
	}
}

// One evaluation of a monitor.
type MonitorReading struct {
	// Which monitor.
	MonitorID string `json:"monitor_id"`
	// When it was evaluated.
	At time.Time `json:"at"`
	// The measured value, when there was enough data to measure one.
	Value *float64 `json:"value,omitempty"`
	// How many runs the window held.
	Samples uint64 `json:"samples"`
	// The resulting condition.
	Condition MonitorCondition `json:"condition"`
	// What a person should read.
	Detail string `json:"detail"`
}

// A monitor and the condition it is latched in.
type MonitorState struct {
	// The monitor.
	Monitor Monitor `json:"monitor"`
	// Its current condition.
	Condition MonitorCondition `json:"condition"`
	// The most recent reading. Absent before the first evaluation.
	Last *MonitorReading `json:"last,omitempty"`
	// When it first tripped, while it stays tripped.
	TrippedAt *time.Time `json:"tripped_at,omitempty"`
}

// A service level objective: the fraction of runs that must succeed over a window. A
// monitor asks whether a number is over a line right now. An objective asks whether
// the promise is going to be missed and how fast the error budget is burning, over two
// windows at once, so a two minute blip and a slow leak get the different answers they
// need. Objectives are configured in the daemon's configuration file, not through the
// API.
type Slo struct {
	// Stable identifier.
	ID string `json:"id"`
	// What a person should read.
	Name string `json:"name"`
	// Which project it covers.
	Project string `json:"project"`
	// The fraction of runs that must succeed, between 0 and 1. `0.995` is ninety-nine and
	// a half percent of runs.
	Objective float64 `json:"objective"`
	// How long the promise is measured over.
	WindowDays uint32 `json:"window_days"`
	// The fewest runs a burn window must hold before it may trip.
	MinSamples uint64 `json:"min_samples"`
	// Where to POST on a transition. Set as a full URL in the config file; the API returns
	// only its host, because a webhook URL is often its own credential.
	Webhook *string `json:"webhook,omitempty"`
	// Whether it is evaluated at all.
	Enabled bool `json:"enabled"`
}

// NewSlo returns a Slo with every spec-declared default applied.
func NewSlo() Slo {
	return Slo{
		WindowDays: 30,
		MinSamples: 20,
		Enabled:    true,
	}
}

// One evaluation of an objective.
type SloReading struct {
	// Which objective.
	SloID string `json:"slo_id"`
	// When it was evaluated.
	At time.Time `json:"at"`
	// The observed success rate over the whole window.
	SuccessRate *float64 `json:"success_rate,omitempty"`
	// The fraction of the error budget still unspent. Negative once it is overspent, which
	// says by how much the promise was missed.
	BudgetRemaining *float64 `json:"budget_remaining,omitempty"`
	// The burn rate over the one hour window. A rate of 1 spends the whole budget exactly
	// over the whole window.
	BurnFast *float64 `json:"burn_fast,omitempty"`
	// The burn rate over the six hour window. Both windows must exceed their trip rate
	// together before the objective trips, which is what stops a brief spike paging
	// anyone.
	BurnSlow *float64 `json:"burn_slow,omitempty"`
	// Runs in the whole window.
	Samples uint64 `json:"samples"`
	// The resulting condition.
	Condition MonitorCondition `json:"condition"`
	// What a person should read.
	Detail string `json:"detail"`
}

// An objective and the condition it is latched in.
type SloState struct {
	// The objective.
	Slo Slo `json:"slo"`
	// Its current condition.
	Condition MonitorCondition `json:"condition"`
	// The most recent reading. Absent before the first evaluation.
	Last *SloReading `json:"last,omitempty"`
	// When it first tripped, while it stays tripped.
	TrippedAt *time.Time `json:"tripped_at,omitempty"`
}

// What the storage backend behind this daemon can and cannot do. Three backends
// implement one storage trait and they are not equal: a reader on ClickHouse who
// searches for `CAFE` with an acute and finds nothing has no way to learn that this
// backend folds ASCII case and no more. Read this once at connect time and offer only
// what the deployment answers. Every field is named after the capability an endpoint
// names when it refuses with 501, so the two can be compared without translation.
type StoreCapabilities struct {
	// Which backend this describes: `sqlite`, `postgres` or `clickhouse`. The same string
	// `/healthz` reports as `storage`.
	Backend string `json:"backend"`
	// What the `q` parameter matches.
	TextSearch TextSearch `json:"text_search"`
	// Whether `q` folds case for letters outside ASCII. False means a search for `CAFE`
	// with an acute does not find the lowercase form, which a reader otherwise meets as an
	// empty result they cannot explain.
	UnicodeCaseFolding bool `json:"unicode_case_folding"`
	// Whether a delete is on disk when the call returns. False means the backend accepted
	// the delete and applies it afterwards, so a count says what matched rather than what
	// has already gone.
	ImmediateDeletes bool `json:"immediate_deletes"`
	// Whether `GET /v1/api/discovery` answers. False means a filter box has to be filled
	// from what is already on screen.
	Discovery bool `json:"discovery"`
	// Whether the `metric_label_value` facet is answerable, not only the keys. Separate
	// because the values are the expensive half: a key set is small and bounded, and a
	// value set is not.
	DiscoveryLabelValues bool `json:"discovery_label_values"`
	// Whether `GET /v1/api/deltas` answers, so a list refreshes from a cursor rather than
	// by running the whole query again.
	DeltaPolling bool `json:"delta_polling"`
	// Whether retention can carry a different window per signal. False means one window
	// covers all five.
	PerSignalRetention bool `json:"per_signal_retention"`
	// Whether the window can be handed to the database engine, so nothing has to run a
	// delete on a timer.
	DeclarativeRetention bool `json:"declarative_retention"`
	// How the signal tables are cut on their anchor time.
	Partitioning Partitioning `json:"partitioning"`
	// Whether a retention pass can drop a whole partition instead of deleting rows one at
	// a time.
	PartitionDropRetention bool `json:"partition_drop_retention"`
	// Whether a start with nothing to apply issues no schema statement at all. False means
	// every process replays the schema on every boot, which is harmless on one node and is
	// metadata churn across a replicated deployment.
	IdempotentSchemaInit bool `json:"idempotent_schema_init"`
	// Whether this deployment answers the Intelligence endpoints. False means the store
	// holds no note or theme tables and every one of those routes refuses with 501 naming
	// this capability, so a client decides once whether to offer the feature rather than
	// showing a reader an empty page they cannot account for. Every backend that answers
	// gives the same themes: the clustering runs in the daemon over stored notes and not
	// in SQL, so there is nothing here for a backend to do differently.
	ThemeAnalysis bool `json:"theme_analysis"`
}

// One value a filter dimension takes in a project.
type DiscoveredValue struct {
	// The value, exactly as it is stored, so it can be sent straight back as a filter with
	// no further mapping.
	Value string `json:"value"`
	// When mcpd last received a signal carrying it. This is arrival time and not the
	// signal's own timestamp, so a backfill of last year's data does not make a value look
	// current. Carried so a caller can rank down or grey out a value nothing has reported
	// for a month. Values are ordered by `value` and not by this: a list somebody reads
	// alphabetically is stable between polls, and one ordered by recency reshuffles under
	// them as data arrives.
	LastSeen time.Time `json:"last_seen"`
}

// The values one filter dimension takes, for a filter box. Answered from a
// distinct-value list the daemon maintains as signals arrive, not by scanning the
// signal tables, because the question is asked on every keystroke and the tables it
// would otherwise scan are the largest thing the deployment holds.
type DiscoveryResponse struct {
	// Which dimension was asked for.
	Facet Facet `json:"facet"`
	// The label key the values belong to, echoed back for `metric_label_value`. Absent for
	// every other facet.
	Key *string `json:"key,omitempty"`
	// The values, ordered by value, ascending.
	Values []DiscoveredValue `json:"values"`
}

// What has arrived since a cursor. A poll is not a page: paging walks a fixed result
// set backwards in the sort you chose, and polling walks forwards in arrival order and
// never ends, so the two cursors are different things and are deliberately not
// interchangeable. Rows come back oldest arrival first, so applying them in order to a
// list you already hold gives the same result as having listed afresh.
type DeltaResponse struct {
	// Which signal these rows are. Exactly one of the three arrays below is present, and
	// it is this one.
	Signal DeltaSignal `json:"signal"`
	// Where the next poll resumes. Always present, and unchanged from the request when
	// nothing came back: moving it forward on an empty poll is what would drop a row that
	// lands a moment later with an earlier arrival time. Treat it as meaningless, and send
	// back the one you were given.
	Cursor string `json:"cursor"`
	// True when the page filled and more is waiting, so a caller draining a backlog polls
	// again at once rather than waiting for its next tick.
	HasMore bool `json:"has_more"`
	// The runs that arrived, when `signal` is `traces`.
	Traces []Trace `json:"traces,omitempty"`
	// The steps that arrived, when `signal` is `spans`.
	Spans []Span `json:"spans,omitempty"`
	// The log lines that arrived, when `signal` is `logs`.
	Logs []LogRecord `json:"logs,omitempty"`
}

// The bounds a clustering pass runs under. They are published because each one changes
// what a reader is looking at: a floor that has not been reached means no themes at
// all rather than an empty result, and a ceiling that has been reached means every
// share is a share of what was read. They are copied from the divergence watch, which
// is the one windowed statistic mcpd already ships. `pass_armed` is the first of them
// to read: the other three describe what a pass would do, and it says whether one
// runs.
type AspectLimits struct {
	// How many notes one aspect on one entity needs before a pass runs at all, 50 by
	// default. Below it the aspect reads `collecting` and no snapshot is written.
	MinNotesToCluster uint64 `json:"min_notes_to_cluster"`
	// The most notes one pass reads, 20,000 by default, taken newest first. A window
	// holding more is clustered from the newest of them and its snapshot is marked
	// `capped`.
	MaxNotesPerPass uint64 `json:"max_notes_per_pass"`
	// How many notes the pass reads at a time while it fills a window, 500 by default. It
	// bounds what the pass holds, not what it covers.
	PageSize uint32 `json:"page_size"`
	// How many aspects one project may define, the four built-ins included. It is
	// configuration and is published here so a client that lets an operator add an aspect
	// knows when to stop instead of finding out from a refusal.
	MaxAspects uint32 `json:"max_aspects"`
	// Whether this deployment runs the clustering pass at all. False means no snapshot
	// will ever be written however many notes arrive, so every aspect stays `collecting`
	// or `processing` for ever and no theme route will have anything to answer with. It is
	// configuration and not a fault, and it is not the same thing as the 501 the theme
	// routes give: that says the storage backend cannot hold the tables, which nobody can
	// switch on, while this says nobody has switched the pass on yet. It is published here
	// rather than on every progress row because it is one fact about the deployment, and a
	// client reads this route once to learn what the aspects are called.
	PassArmed bool `json:"pass_armed"`
}

// Every aspect a project describes traces along, with the bounds the clustering pass
// runs under. The four built-ins are rows in the same table as anything you define, so
// there is no fallback list inside a client that can quietly disagree with this one.
type AspectCatalogue struct {
	// The aspects, in `sort_order`. Disabled aspects are included, because notes already
	// recorded under one stay readable and a name that vanished from this list would make
	// them look like a mistake.
	Aspects []Aspect `json:"aspects"`
	// The bounds a pass runs under in this deployment.
	Limits AspectLimits `json:"limits"`
}

// One entity that has aspect notes, with the roll-up a list needs to be worth reading.
// A row appears as soon as the first note is recorded, before any theme exists,
// because an empty screen with a count moving on it is a different thing from an empty
// screen and a reader has to be able to tell them apart.
type EntityIndexRow struct {
	// The kind of thing this row is about.
	EntityKind EntityKind `json:"entity_kind"`
	// Its name.
	EntityName string `json:"entity_name"`
	// How many of its traces carry at least one note. This is not the entity's trace
	// count: writing a note lags ingestion, so a busy agent can show far fewer here.
	TraceCount uint64 `json:"trace_count"`
	// How many notes are recorded about it.
	NoteCount uint64 `json:"note_count"`
	// How many aspects are switched on for this entity's project.
	EnabledAspects uint32 `json:"enabled_aspects"`
	// How many of those have themes to read. The two numbers read as `2 of 4` and are the
	// honest answer to whether this row is worth opening yet.
	ReadyAspects uint32 `json:"ready_aspects"`
	// The row's overall state, which is the least advanced of its enabled aspects, so a
	// row says `processing` while any one of them is still short of a snapshot. It never
	// reports the best case.
	Readiness AspectReadiness `json:"readiness"`
	// When the newest note about this entity was recorded, so a reader can tell a list
	// that is filling from one that stopped.
	LastNoteAt time.Time `json:"last_note_at"`
	// The cutoff of this entity's newest snapshot. Absent until a pass has written one,
	// which is the ordinary state of a row that is still collecting.
	LatestSnapshotAt *time.Time `json:"latest_snapshot_at,omitempty"`
}

// A page of the entities that have aspect notes.
type EntityIndexResponse struct {
	// The matching entities, most recently written to first.
	Entities []EntityIndexRow `json:"entities"`
	// Cursor and totals.
	Page Page `json:"page"`
}

// How far one aspect has got for one entity. Counted from stored notes, so it is what
// has happened and not a projection of what will.
type AspectProgressRow struct {
	// Which aspect this row is about.
	Aspect string `json:"aspect"`
	// The state this aspect is in for this entity.
	Readiness AspectReadiness `json:"readiness"`
	// How many notes are recorded along it.
	NoteCount uint64 `json:"note_count"`
	// How many distinct traces those notes were written about.
	TraceCount uint64 `json:"trace_count"`
	// How many more notes this aspect needs before a pass will run. Zero once the floor is
	// reached, which is what turns a wait into a number a reader can watch.
	NotesNeeded uint64 `json:"notes_needed"`
	// When the newest note along this aspect was recorded. Absent when there are none.
	LastNoteAt *time.Time `json:"last_note_at,omitempty"`
}

// What the daemon has recorded for one entity, aspect by aspect. This is the answer to
// why a themes screen is empty, and it separates the three cases that look identical
// on screen: nothing has been recorded, enough has been recorded and no pass has run
// yet, or there are themes to read and the reader is looking at the wrong aspect.
type AspectProgress struct {
	// The kind of entity this describes.
	EntityKind EntityKind `json:"entity_kind"`
	// Its name.
	EntityName string `json:"entity_name"`
	// The entity's overall state, which is the least advanced of its enabled aspects.
	Readiness AspectReadiness `json:"readiness"`
	// How many of this entity's traces carry at least one note.
	TraceCount uint64 `json:"trace_count"`
	// One row per enabled aspect, in catalogue order.
	Aspects []AspectProgressRow `json:"aspects"`
	// The newest snapshot for this entity, when a pass has written one. Absent while every
	// aspect is still collecting.
	LatestSnapshot *ThemeSnapshot `json:"latest_snapshot,omitempty"`
}

// One snapshot in a timeline, with the reason it was chosen.
type SnapshotLandmark struct {
	// The snapshot.
	Snapshot ThemeSnapshot `json:"snapshot"`
	// Why this snapshot is in the selection. Present for `presentation=landmarks` only; an
	// inventory returns consecutive snapshots and chooses nothing, so there is nothing to
	// report.
	Reason *SnapshotLandmarkReason `json:"reason,omitempty"`
}

// A selection of one entity's snapshots, oldest first, so a timeline can be drawn
// without reordering anything.
type SnapshotListResponse struct {
	// The selected snapshots, oldest first.
	Snapshots []SnapshotLandmark `json:"snapshots"`
	// How many snapshots the entity has in the requested range. This is the number the
	// selection was drawn from and not the number returned, so a timeline can say what it
	// is standing in for.
	Total uint64 `json:"total"`
	// Cursor and totals.
	Page Page `json:"page"`
}

// One node of one stage of a flow: a theme, the stage's noise, or the overflow the
// stage folded away.
type ThemeNode struct {
	// Identifies this node inside this flow, and is what a link names at either end. It is
	// not a theme id: the noise and overflow nodes have one of these and no theme id.
	NodeID string `json:"node_id"`
	// Whether this node is a theme, the noise or the overflow.
	Kind ThemeNodeKind `json:"kind"`
	// The theme this node stands for. Present only when `kind` is `theme`, which is what
	// makes the other two impossible to drill into by accident.
	ThemeID *string `json:"theme_id,omitempty"`
	// What to show for this node: a theme's own label, or the words for noise or overflow.
	Label string `json:"label"`
	// How many traces this node accounts for.
	TraceCount uint64 `json:"trace_count"`
	// This node's share of the stage's traces, between 0 and 1. A stage's nodes cover all
	// of it, so its shares add to 1.
	Coverage float64 `json:"coverage"`
}

// One aspect's column in a flow. Stages come back in the order the aspects were asked
// for, and links only ever join neighbouring stages, so the order decides which
// co-occurrences are visible and is not a display preference.
type ThemeStage struct {
	// Which aspect this stage is.
	Aspect string `json:"aspect"`
	// How many traces this stage accounts for.
	TraceCount uint64 `json:"trace_count"`
	// The stage's nodes, largest first, with the noise and overflow nodes last.
	Nodes []ThemeNode `json:"nodes"`
}

// A ribbon between one node of a stage and one node of the next. It says the two
// describe the same traces and nothing more: a link between a goal theme and an
// outcome theme means they were written about the same runs, not that one caused the
// other.
type ThemeLink struct {
	// The node this link leaves, in the earlier stage.
	SourceNodeID string `json:"source_node_id"`
	// The node it reaches, in the next stage.
	TargetNodeID string `json:"target_node_id"`
	// How many traces are assigned to both ends.
	TraceCount uint64 `json:"trace_count"`
	// That count as a fraction of the source node's traces, between 0 and 1.
	SourceShare float64 `json:"source_share"`
	// That count as a fraction of the target node's traces, between 0 and 1. The two
	// shares differ whenever the nodes are different sizes, and reading only one of them
	// is how a thin ribbon gets mistaken for a weak relationship.
	TargetShare float64 `json:"target_share"`
}

// One snapshot read across two to four aspects at once: a stage per aspect, and the
// links between neighbouring stages. Association, never causation.
type ThemeFlow struct {
	// The snapshot this flow was computed from, so every count on the page can be read
	// against the window it came from.
	Snapshot ThemeSnapshot `json:"snapshot"`
	// One stage per aspect, in the order they were asked for.
	Stages []ThemeStage `json:"stages"`
	// The links between neighbouring stages. There are none between stages that are not
	// neighbours, which is why the order of the aspects matters.
	Links []ThemeLink `json:"links"`
}

// One theme in one snapshot, or the answer that this snapshot did not hold it.
type ThemeDetail struct {
	// The snapshot that was read, with its own counts, so a caller can write `28 of 70
	// traces in this window` without a second request.
	Snapshot ThemeSnapshot `json:"snapshot"`
	// The theme, when this snapshot holds it. Absent is an answer and not an error: the
	// theme did not exist in that window, which is what scrubbing a timeline back past a
	// theme's birth looks like. A theme id this entity has never had is a 404 instead.
	Theme *Theme `json:"theme,omitempty"`
}

// One note from a theme, with the run it was written about. The two ids are different
// things: `note_id` identifies the sentence and `trace_id` identifies the run it
// describes.
type ThemeExample struct {
	// The note this example is.
	NoteID string `json:"note_id"`
	// The trace the note was written about.
	TraceID string `json:"trace_id"`
	// The sentence, after redaction. This is the wording that put the trace in this theme,
	// so a reader can see why it landed here rather than take it on trust.
	Text string `json:"text"`
	// How much this note's wording overlaps the theme's label, counted out of 64. It is a
	// count of shared sketch positions and not a percentage, and it is an integer on
	// purpose: no part of the clustering uses floating-point arithmetic, so two replicas
	// cannot order the same notes differently.
	Similarity uint32 `json:"similarity"`
	// When the run began, so examples can be read against the period they came from rather
	// than against when the note was written.
	TraceStartedAt time.Time `json:"trace_started_at"`
}

// A page of the notes behind one theme.
type ThemeExampleListResponse struct {
	// The notes, most like the theme's label first.
	Examples []ThemeExample `json:"examples"`
	// Cursor and totals.
	Page Page `json:"page"`
}

// One theme in one snapshot of its life. Every count belongs to that snapshot alone,
// and windows overlap, so a run of these is read as a series and never added up.
type ThemeHistoryPoint struct {
	// The snapshot this point comes from.
	SnapshotID string `json:"snapshot_id"`
	// When that snapshot became the current state.
	CutoffAt time.Time `json:"cutoff_at"`
	// Start of that snapshot's window, inclusive.
	WindowFrom time.Time `json:"window_from"`
	// End of that snapshot's window, exclusive.
	WindowTo time.Time `json:"window_to"`
	// What had happened to the theme by this point.
	State ThemeState `json:"state"`
	// How many notes were in the theme here.
	NoteCount uint64 `json:"note_count"`
	// How many distinct traces those notes were written about.
	TraceCount uint64 `json:"trace_count"`
	// The theme's share of the notes that snapshot read for this aspect, between 0 and 1.
	// Coverage and raw count can move in opposite directions, and coverage is the one that
	// says whether a theme is gaining ground.
	Coverage float64 `json:"coverage"`
	// The themes this one came out of at this point. Empty unless `state` is `split`.
	SplitFrom []string `json:"split_from"`
	// The themes that came out of this one at this point. Both directions are stored, so a
	// caller never has to read a second theme's history to learn what happened to this
	// one.
	SplitInto []string `json:"split_into"`
	// The themes that became this one at this point. Empty unless `state` is `merge`.
	MergedFrom []string `json:"merged_from"`
	// The theme this one became at this point, when it stopped being its own theme.
	MergedInto []string `json:"merged_into"`
}

// One theme across every stored snapshot that holds it, oldest first. This is the only
// theme response that names no single snapshot, because the series is the answer.
type ThemeHistory struct {
	// The theme this history is of.
	ThemeID string `json:"theme_id"`
	// The aspect it was clustered along.
	Aspect string `json:"aspect"`
	// The theme's label at its newest point. A label can change between snapshots, because
	// the note most like the others can change while the theme holds together.
	Label string `json:"label"`
	// The series, oldest first, so nothing has to be reordered before it is drawn or
	// differenced.
	Points []ThemeHistoryPoint `json:"points"`
	// Cursor and totals.
	Page Page `json:"page"`
}

// Every aspect note recorded about one trace, which is at most one per aspect. Read it
// beside the trace itself: a note is the sentence that put this run in each of its
// themes, so it is the shortest route from a theme back to what actually happened.
type TraceAspectNotes struct {
	// The trace these notes are about.
	TraceID string `json:"trace_id"`
	// The notes, in catalogue order. Empty when nothing has written one yet, which is
	// ordinary for a recent trace.
	Notes []AspectNote `json:"notes"`
}

// An email address and a password, for sign-up and for log-in.
type AccountCredentials struct {
	// The account's email address. Compared without regard to case, and with surrounding
	// spaces removed.
	Email string `json:"email"`
	// At least 10 characters and at most 1024 bytes. Stored only as an argon2id hash.
	Password string `json:"password"`
	// The six-digit code from an authenticator app, or one of the recovery codes, when the
	// account has two-step log-in on. Ignored by sign-up.
	TotpCode *string `json:"totp_code,omitempty"`
}

// A logged-in session. Send `token` as `Authorization: Bearer <token>`.
type AccountSession struct {
	// The session token. It is shown once: the daemon keeps only its SHA-256 digest.
	Token string `json:"token"`
	// When the session stops working. Log in again after this.
	ExpiresAt time.Time `json:"expires_at"`
	// The account's email address, as stored.
	Email string `json:"email"`
	// The project reads go to when they name none. The account's own project, or the first
	// project of the organisation a single sign-on session is limited to. Absent when the
	// session reaches no project yet.
	Project *string `json:"project,omitempty"`
	// Set when the session came from an organisation's single sign-on. The session then
	// reaches that organisation's projects only.
	OrgID *string `json:"org_id,omitempty"`
}

// The account a session belongs to.
type Account struct {
	// The account's email address, as stored.
	Email string `json:"email"`
	// The project created with the account at sign-up. Absent for an account that was
	// invited, or created by single sign-on or SCIM.
	Project *string `json:"project,omitempty"`
	// When the account was created.
	CreatedAt time.Time `json:"created_at"`
	// Whether the address has been confirmed by a link sent to it.
	EmailVerified bool `json:"email_verified"`
	// Whether two-step log-in is on.
	TotpEnabled bool `json:"totp_enabled"`
	// Whether the account can log in with a password. False for an account created by
	// single sign-on or SCIM that has never set one.
	HasPassword bool `json:"has_password"`
	// The organisation that created this account through single sign-on or SCIM. That
	// organisation can deactivate it.
	ManagedBy *string `json:"managed_by,omitempty"`
}

// One API key an account holds. The secret is not part of it: the daemon keeps only a
// digest.
type AccountKey struct {
	// The key's identifier, for revoking it. Not a secret.
	ID string `json:"id"`
	// What the key may do.
	Scope KeyScope `json:"scope"`
	// The one project the key reaches.
	Project string `json:"project"`
	// The label given when the key was created.
	Name *string `json:"name,omitempty"`
	// The first characters of the secret, so you can tell which key a process holds
	// without the daemon storing the secret.
	Prefix string `json:"prefix"`
	// When the key was created.
	CreatedAt time.Time `json:"created_at"`
	// The email of the account that created the key. Set in an organisation's key list, so
	// an admin can see whose key it is.
	CreatedBy *string `json:"created_by,omitempty"`
}

// A request for a new API key.
type CreateAccountKeyRequest struct {
	// What the key may do. One scope per key.
	Scope KeyScope `json:"scope"`
	// A label for the key, such as the service that will hold it. At most 100 characters.
	Name *string `json:"name,omitempty"`
	// The project the key reaches. Defaults to the account's own project. Your role on the
	// project must allow the scope.
	Project *string `json:"project,omitempty"`
}

// A new API key, with its secret. This is the only time the secret is shown.
type CreatedAccountKey struct {
	// The key as it is listed from now on.
	Key AccountKey `json:"key"`
	// The key itself. Send it as `Authorization: Bearer <secret>`. Copy it now: the daemon
	// keeps only its SHA-256 digest and cannot show it again.
	Secret string `json:"secret"`
}

// A plain answer that changes nothing you can read back, such as "check your email".
type AccountNotice struct {
	// What happened, and what to do next.
	Message string `json:"message"`
}

// A one-time token from a link the daemon sent by email.
type TokenRequest struct {
	// The token, as it appears in the link.
	Token string `json:"token"`
}

// A request for a password reset link.
type PasswordForgotRequest struct {
	// The address to send the link to. The answer is the same whether or not an account
	// uses it.
	Email string `json:"email"`
}

// A new password, with the token from a reset link.
type PasswordResetRequest struct {
	// The token from the reset link. It works once, for one hour.
	Token string `json:"token"`
	// The new password. At least 10 characters and at most 1024 bytes.
	Password string `json:"password"`
	// A code from the authenticator app, or a recovery code, when the account has two-step
	// log-in on. A reset proves the address, not the second factor.
	TotpCode *string `json:"totp_code,omitempty"`
}

// A new password for the logged-in account.
type PasswordChangeRequest struct {
	// The password in use now. Required when the account has one.
	CurrentPassword *string `json:"current_password,omitempty"`
	// The new password. At least 10 characters and at most 1024 bytes.
	NewPassword string `json:"new_password"`
}

// A new email address for the logged-in account.
type EmailChangeRequest struct {
	// The new address. A link is sent to it, and the change happens when the link is
	// opened.
	Email string `json:"email"`
	// The account's password. Required when the account has one.
	Password *string `json:"password,omitempty"`
}

// Confirmation for deleting the logged-in account.
type AccountDeleteRequest struct {
	// The account's password. Required when the account has one.
	Password *string `json:"password,omitempty"`
	// A code from the authenticator app. Required when two-step log-in is on.
	TotpCode *string `json:"totp_code,omitempty"`
}

// What deleting an account removed.
type AccountDeletion struct {
	// Organisations that had this account as their only member, deleted with it.
	OrgsDeleted uint32 `json:"orgs_deleted"`
	// Projects whose stored signals and runs were erased, because their organisation was
	// deleted.
	ProjectsErased []string `json:"projects_erased"`
}

// One live session of the logged-in account. The token is not part of it.
type SessionInfo struct {
	// The session's identifier, for ending it. Not a secret.
	ID string `json:"id"`
	// When the session started.
	CreatedAt time.Time `json:"created_at"`
	// When it stops working.
	ExpiresAt time.Time `json:"expires_at"`
	// True for the session this request carries.
	Current bool `json:"current"`
	// How it started: `password`, `sso`, `invite`, `verify` or `reset`.
	Via string `json:"via"`
	// The organisation a single sign-on session is limited to.
	OrgID *string `json:"org_id,omitempty"`
	// The browser or client that started it, as it described itself.
	UserAgent *string `json:"user_agent,omitempty"`
	// The client address that started it.
	Address *string `json:"address,omitempty"`
}

// A new authenticator secret, not yet in use. Confirm it with a code to turn two-step
// log-in on.
type TotpSetup struct {
	// The secret in base32, for typing into an authenticator app.
	Secret string `json:"secret"`
	// The same secret as an `otpauth://` URI.
	OtpauthUri string `json:"otpauth_uri"`
	// The URI as a QR code, an SVG document.
	QrSvg string `json:"qr_svg"`
}

// A code from an authenticator app.
type TotpCodeRequest struct {
	// The six-digit code.
	Code string `json:"code"`
}

// Two-step log-in is on.
type TotpEnabled struct {
	// Ten single-use codes that stand in for the authenticator app. They are shown once:
	// the daemon keeps only their digests.
	RecoveryCodes []string `json:"recovery_codes"`
}

// Confirmation for turning two-step log-in off.
type TotpDisableRequest struct {
	// The account's password. Required when the account has one.
	Password *string `json:"password,omitempty"`
	// A code from the authenticator app, or a recovery code.
	Code string `json:"code"`
}

// An organisation: the owner of projects, members and single sign-on settings.
type Org struct {
	// The organisation's identifier.
	ID string `json:"id"`
	// Its display name.
	Name string `json:"name"`
	// Your role in it.
	Role OrgRole `json:"role"`
	// When it was created.
	CreatedAt time.Time `json:"created_at"`
	// The most signals all its projects together may store per minute. Absent means no
	// ceiling.
	QuotaPerMinute *uint32 `json:"quota_per_minute,omitempty"`
	// How long its projects keep data, in days. Absent means forever.
	RetentionDays *uint32 `json:"retention_days,omitempty"`
	// How many members it has.
	Members uint32 `json:"members"`
	// How many projects it owns.
	Projects uint32 `json:"projects"`
	// True when the daemon's configuration file declares this organisation. Its single
	// sign-on settings are then read from that file and cannot be changed here.
	ManagedByConfig bool `json:"managed_by_config"`
}

// A new organisation.
type CreateOrgRequest struct {
	// Its display name, at most 100 characters.
	Name string `json:"name"`
}

// Changes to an organisation. A field left out keeps its value.
type UpdateOrgRequest struct {
	// A new display name, at most 100 characters.
	Name *string `json:"name,omitempty"`
	// A ceiling on signals per minute across all the organisation's projects, at most the
	// daemon's own ceiling. 0 restores the daemon's default. Owner only.
	QuotaPerMinute *uint32 `json:"quota_per_minute,omitempty"`
	// How long to keep data, in days, at most the daemon's own window. 0 restores the
	// daemon's default. Owner only.
	RetentionDays *uint32 `json:"retention_days,omitempty"`
}

// What deleting an organisation removed.
type OrgDeletion struct {
	// Projects whose stored signals and runs were erased.
	ProjectsErased []string `json:"projects_erased"`
}

// A role on one project.
type ProjectRoleGrant struct {
	// The project.
	Project string `json:"project"`
	// The role there.
	Role ProjectRole `json:"role"`
}

// One member of an organisation.
type OrgMember struct {
	// The member's account.
	AccountID string `json:"account_id"`
	// The member's email address.
	Email string `json:"email"`
	// The member's role.
	Role OrgRole `json:"role"`
	// How they joined: `created`, `invite`, `sso`, `scim` or `config`.
	Via string `json:"via"`
	// When they joined.
	JoinedAt time.Time `json:"joined_at"`
	// Roles on single projects that replace the organisation role there.
	ProjectRoles []ProjectRoleGrant `json:"project_roles"`
	// True when this organisation created the account through single sign-on or SCIM.
	Managed bool `json:"managed"`
	// Whether the member has two-step log-in on.
	TotpEnabled bool `json:"totp_enabled"`
}

// A new role for a member.
type UpdateMemberRequest struct {
	// The role. Only an owner may grant or remove `owner`, and the last owner cannot be
	// demoted.
	Role OrgRole `json:"role"`
}

// A role for a member on one project.
type SetProjectRoleRequest struct {
	// A project the organisation owns.
	Project string `json:"project"`
	// The role there. Absent removes the project role, so the organisation role applies
	// again.
	Role *ProjectRole `json:"role,omitempty"`
}

// An invitation not yet accepted. The token is not part of it.
type OrgInvite struct {
	// The invitation's identifier, for revoking it.
	ID string `json:"id"`
	// The address it was sent to.
	Email string `json:"email"`
	// The role it grants.
	Role OrgRole `json:"role"`
	// Project roles it grants as well.
	ProjectRoles []ProjectRoleGrant `json:"project_roles"`
	// The email of the member who sent it. Absent when the configuration file or SCIM sent
	// it.
	InvitedBy *string `json:"invited_by,omitempty"`
	// When it was sent.
	CreatedAt time.Time `json:"created_at"`
	// When it stops working.
	ExpiresAt time.Time `json:"expires_at"`
}

// An invitation to an organisation.
type CreateInviteRequest struct {
	// Who to invite. The link is sent to this address.
	Email string `json:"email"`
	// The role to grant. Only an owner may invite an owner.
	Role OrgRole `json:"role"`
	// Project roles to grant as well.
	ProjectRoles []ProjectRoleGrant `json:"project_roles,omitempty"`
}

// What an invitation offers, read with its token before accepting it.
type InviteLookup struct {
	// The organisation.
	OrgName string `json:"org_name"`
	// The address it was sent to. Accepting it joins the account with this address.
	Email string `json:"email"`
	// The role it grants.
	Role OrgRole `json:"role"`
	// Whether an account with this address exists. If it does, log in as it to accept; if
	// not, accepting creates it.
	AccountExists bool `json:"account_exists"`
	// When it stops working.
	ExpiresAt time.Time `json:"expires_at"`
}

// Acceptance of an invitation.
type InviteAcceptRequest struct {
	// The token from the invitation link.
	Token string `json:"token"`
	// A password for the new account, when no account uses the invited address. Ignored
	// otherwise.
	Password *string `json:"password,omitempty"`
}

// An accepted invitation.
type InviteAccepted struct {
	// The organisation joined.
	Org Org `json:"org"`
	// A session for the account accepting created. Absent when an existing account
	// accepted with its own session.
	Session *AccountSession `json:"session,omitempty"`
}

// A project an organisation owns.
type OrgProject struct {
	// The project name. Random, so nobody can choose one that already holds somebody
	// else's data.
	Project string `json:"project"`
	// A display label.
	Label *string `json:"label,omitempty"`
	// When it was created or attached.
	CreatedAt time.Time `json:"created_at"`
}

// A new project in an organisation.
type CreateProjectRequest struct {
	// A display label, at most 100 characters.
	Label *string `json:"label,omitempty"`
}

// How much an organisation is using, against its ceilings. Counted since the daemon
// started.
type OrgUsage struct {
	// The organisation.
	OrgID string `json:"org_id"`
	// Its ceiling on signals per minute, when it has one.
	QuotaPerMinute *uint32 `json:"quota_per_minute,omitempty"`
	// Signals stored in the current one-minute window.
	AdmittedThisMinute uint64 `json:"admitted_this_minute"`
	// Signals stored since the daemon started.
	AdmittedTotal uint64 `json:"admitted_total"`
	// Signals refused for the ceiling since the daemon started.
	RefusedTotal uint64 `json:"refused_total"`
	// How long its projects keep data, in days.
	RetentionDays *uint32 `json:"retention_days,omitempty"`
	// Members.
	Members uint32 `json:"members"`
	// Projects.
	Projects uint32 `json:"projects"`
	// API keys on its projects.
	Keys uint32 `json:"keys"`
	// When counting started.
	Since time.Time `json:"since"`
}

// An organisation's single sign-on settings. The client secret is not part of it.
type SsoConfig struct {
	// The kind of provider.
	Provider SsoProvider `json:"provider"`
	// The OpenID Connect issuer URL. Absent for GitHub.
	Issuer *string `json:"issuer,omitempty"`
	// The client id registered with the provider.
	ClientID string `json:"client_id"`
	// Whether a client secret is stored.
	HasClientSecret bool `json:"has_client_secret"`
	// Email domains this organisation claims. Single sign-on creates accounts only for
	// these domains, and "Log in with SSO" finds the organisation by them.
	Domains []string `json:"domains"`
	// When true, members whose email is in a claimed domain cannot log in with a password.
	Enforced bool `json:"enforced"`
	// The role given to a member single sign-on creates, when no group maps to a role.
	// Never `owner`.
	DefaultRole OrgRole `json:"default_role"`
	// The ID token claim that lists the member's groups, mapped to roles by the group
	// roles.
	GroupsClaim *string `json:"groups_claim,omitempty"`
	// The redirect URL to register with the provider.
	RedirectURL string `json:"redirect_url"`
	// True when these settings come from the configuration file.
	ManagedByConfig bool `json:"managed_by_config"`
	// When they were last changed.
	UpdatedAt time.Time `json:"updated_at"`
}

// New single sign-on settings for an organisation.
type PutSsoConfigRequest struct {
	// The kind of provider.
	Provider SsoProvider `json:"provider"`
	// The OpenID Connect issuer URL, such as `https://accounts.google.com`. Required for
	// `oidc`. Must be `https`.
	Issuer *string `json:"issuer,omitempty"`
	// The client id registered with the provider.
	ClientID string `json:"client_id"`
	// The client secret. Left out, the stored one is kept.
	ClientSecret *string `json:"client_secret,omitempty"`
	// Email domains to claim. You can claim the domain of your own verified address. A
	// domain belongs to one organisation.
	Domains []string `json:"domains"`
	// Refuse password log-in for members in the claimed domains. Turning it on ends their
	// password sessions.
	Enforced bool `json:"enforced"`
	// The role for a member single sign-on creates. `admin`, `member` or `viewer`.
	DefaultRole OrgRole `json:"default_role"`
	// The ID token claim that lists groups, such as `groups`.
	GroupsClaim *string `json:"groups_claim,omitempty"`
}

// A group from the identity provider, and the role its members get.
type GroupRole struct {
	// The group name, as single sign-on or SCIM sends it. Compared without regard to case.
	Group string `json:"group"`
	// The role. Never `owner`.
	Role OrgRole `json:"role"`
}

// Whether an organisation has a SCIM token. The token is not part of it.
type ScimTokenInfo struct {
	// Whether a token exists.
	Configured bool `json:"configured"`
	// The first characters of the token.
	Prefix *string `json:"prefix,omitempty"`
	// When it was created.
	CreatedAt *time.Time `json:"created_at,omitempty"`
	// The SCIM base URL to give your identity provider.
	BaseURL string `json:"base_url"`
}

// A new SCIM token. This is the only time it is shown, and it replaces any earlier
// token.
type CreatedScimToken struct {
	// The bearer token for your identity provider. The daemon keeps only its SHA-256
	// digest.
	Token string `json:"token"`
	// Its first characters.
	Prefix string `json:"prefix"`
	// When it was created.
	CreatedAt time.Time `json:"created_at"`
	// The SCIM base URL to give your identity provider.
	BaseURL string `json:"base_url"`
}

// Where to start single sign-on. Give one of the two.
type SsoStartRequest struct {
	// Your email address. Its domain finds the organisation.
	Email *string `json:"email,omitempty"`
	// The organisation, when you know it.
	OrgID *string `json:"org_id,omitempty"`
}

// Where to send the browser for single sign-on.
type SsoStart struct {
	// The provider's authorization URL, with state, nonce and the PKCE challenge in it.
	AuthorizeURL string `json:"authorize_url"`
	// A secret to keep in this browser and send back with the callback. It ties the
	// callback to the browser that started it, so a link from somebody else cannot log you
	// in as them.
	Binding string `json:"binding"`
	// The organisation.
	OrgName string `json:"org_name"`
}

// What the provider sent back to the redirect URL.
type SsoCallbackRequest struct {
	// The `state` parameter.
	State string `json:"state"`
	// The `code` parameter.
	Code string `json:"code"`
	// The binding from the start of the flow.
	Binding string `json:"binding"`
}

// One unit of work. Spans are the backbone of mcpd: logs, metrics, scores and feedback
// all hang off a span or a trace.
// Stored in the spans table.
type Span struct {
	// 128-bit trace id, 32 lowercase hex characters. OpenTelemetry-compatible.
	TraceID string `json:"trace_id"`
	// 64-bit span id, 16 lowercase hex characters. Unique within the trace.
	SpanID string `json:"span_id"`
	// The parent span within this trace. Absent on the root.
	ParentSpanID *string `json:"parent_span_id,omitempty"`
	// A parent from a foreign tracing system that mcpd did not create. Carried for
	// correlation, never used for mcpd's own tree.
	ExternalParentSpanID *string `json:"external_parent_span_id,omitempty"`
	// What this operation is called. Keep it low-cardinality: `search_docs`, not
	// `search_docs(query=hello)`.
	Name string `json:"name"`
	// What kind of operation this is.
	Kind SpanKind `json:"kind"`
	// The outcome.
	Status SpanStatus `json:"status"`
	// When the operation began, in UTC with nanosecond precision.
	StartTime time.Time `json:"start_time"`
	// When it finished. Absent while the span is still running.
	EndTime *time.Time `json:"end_time,omitempty"`
	// Derived from the timestamps at ingest, stored so the database never has to compute
	// it to sort.
	DurationNs *uint64 `json:"duration_ns,omitempty"`
	// True when this span has no parent inside the trace. Materialised so the trace list
	// never scans children.
	IsRoot bool `json:"is_root"`
	// True for a point in time rather than a duration. Event spans have no end.
	IsEvent bool `json:"is_event"`
	// The kind of thing that produced this span.
	EntityKind *EntityKind `json:"entity_kind,omitempty"`
	// Stable identifier of the producing entity.
	EntityID *string `json:"entity_id,omitempty"`
	// Display name of the producing entity.
	EntityName *string `json:"entity_name,omitempty"`
	// What went in. For a model call this is the full prompt, and it is the single most
	// valuable field mcpd stores.
	Input json.RawMessage `json:"input,omitempty"`
	// What came out. For a model call this is the completion.
	Output json.RawMessage `json:"output,omitempty"`
	// Kind-specific structured detail. See the attribute reference for what each kind
	// carries.
	Attributes json.RawMessage `json:"attributes,omitempty"`
	// Anything you want to attach. mcpd never interprets this.
	Metadata json.RawMessage `json:"metadata,omitempty"`
	// Token counts, on model and embedding spans.
	Usage *TokenUsage `json:"usage,omitempty"`
	// Computed cost, on spans that consumed tokens.
	Cost *Cost `json:"cost,omitempty"`
	// How the model was called: the sampling settings the caller chose. Absent on a span
	// that made no model call, and on one whose producer did not record them.
	ModelParameters *ModelParameters `json:"model_parameters,omitempty"`
	// Failure detail, when `status` is `error`.
	Error *SpanError `json:"error,omitempty"`
	// Causal references to other spans or traces.
	Links []SpanLink `json:"links,omitempty"`
	// Free labels for filtering. Only meaningful on a root span, where they label the
	// whole trace.
	Tags []string `json:"tags,omitempty"`
	// Which project, environment, session, thread and user this belongs to.
	Correlation Correlation `json:"correlation"`
	// Model identifier, denormalised out of `attributes` so it can be indexed and grouped.
	Model *string `json:"model,omitempty"`
	// Model provider, denormalised for the same reason.
	Provider *string `json:"provider,omitempty"`
	// When mcpd accepted this span. Differs from `start_time` for buffered or replayed
	// data, and is what retention is measured against.
	ReceivedAt time.Time `json:"received_at"`
	// Producer-supplied idempotency key. A retried batch with the same key is accepted
	// once.
	IngestKey *string `json:"ingest_key,omitempty"`
}

// Table is the storage table Span lives in.
func (Span) Table() string { return "spans" }

// A whole run, projected from its spans. mcpd materialises this at ingest so the list
// view is one indexed read rather than an aggregation over every span. One per project
// and trace id: a trace id crosses service boundaries, so two projects that call each
// other each hold their own run under it, built from their own spans.
// Stored in the traces table.
type Trace struct {
	// The trace this summarises.
	TraceID string `json:"trace_id"`
	// The root span, when one has arrived.
	RootSpanID *string `json:"root_span_id,omitempty"`
	// The root span's name.
	Name string `json:"name"`
	// The root span's kind.
	Kind SpanKind `json:"kind"`
	// `error` when any span in the trace failed, otherwise the root's status.
	Status SpanStatus `json:"status"`
	// Earliest start across the trace.
	StartTime time.Time `json:"start_time"`
	// Latest end across the trace. Absent while anything is still running.
	EndTime *time.Time `json:"end_time,omitempty"`
	// Wall-clock span of the whole trace.
	DurationNs *uint64 `json:"duration_ns,omitempty"`
	// How many spans have arrived.
	SpanCount uint32 `json:"span_count"`
	// How many of them failed.
	ErrorCount uint32 `json:"error_count"`
	// Token usage summed across every model span in the trace.
	Usage *TokenUsage `json:"usage,omitempty"`
	// Cost summed across the trace. This is the number a bill is reconciled against.
	Cost *Cost `json:"cost,omitempty"`
	// How many model calls the run made.
	ModelCount uint32 `json:"model_count"`
	// How many tool calls the run made.
	ToolCount uint32 `json:"tool_count"`
	// Tags from the root span.
	Tags []string `json:"tags,omitempty"`
	// Context inherited from the root span.
	Correlation Correlation `json:"correlation"`
	// Mean score per scorer id, maintained as scores arrive, so the list can sort by
	// quality without a join.
	ScoreSummary json.RawMessage `json:"score_summary,omitempty"`
	// Counts per feedback kind and value.
	FeedbackSummary json.RawMessage `json:"feedback_summary,omitempty"`
	// True once the root span has ended and no span is still open.
	IsComplete bool `json:"is_complete"`
	// When the first span of this trace arrived.
	ReceivedAt time.Time `json:"received_at"`
}

// Table is the storage table Trace lives in.
func (Trace) Table() string { return "traces" }

// A structured log line, correlated to the span that produced it. Logs are stored
// beside traces so 'show me the logs for this run' is one query, not a context switch
// to another tool.
// Stored in the logs table.
type LogRecord struct {
	// Unique id for this record.
	LogID string `json:"log_id"`
	// The trace this line belongs to, when it was emitted inside one.
	TraceID *string `json:"trace_id,omitempty"`
	// The span this line belongs to.
	SpanID *string `json:"span_id,omitempty"`
	// When the line was emitted.
	Timestamp time.Time `json:"timestamp"`
	// Severity.
	Level LogLevel `json:"level"`
	// The rendered message.
	Message string `json:"message"`
	// Structured key/value context. Redacted like any payload.
	Fields json.RawMessage `json:"fields,omitempty"`
	// Which logger emitted it.
	Logger *string `json:"logger,omitempty"`
	// Project, environment, session and user.
	Correlation Correlation `json:"correlation"`
	// When mcpd accepted it.
	ReceivedAt time.Time `json:"received_at"`
}

// Table is the storage table LogRecord lives in.
func (LogRecord) Table() string { return "logs" }

// One numeric observation. mcpd aggregates these into time buckets on read, and keeps
// raw points only as long as the raw retention class allows.
// Stored in the metrics table.
type MetricPoint struct {
	// Unique id for this observation.
	MetricID string `json:"metric_id"`
	// Metric name, such as `mcpd.model.duration`.
	Name string `json:"name"`
	// How it aggregates.
	Kind MetricKind `json:"kind"`
	// The observed value.
	Value float64 `json:"value"`
	// UCUM unit, such as `ms`, `By` or `{token}`.
	Unit *string `json:"unit,omitempty"`
	// When it was observed.
	Timestamp time.Time `json:"timestamp"`
	// Dimensions. Passed through the cardinality filter, which rejects unbounded keys.
	Labels json.RawMessage `json:"labels,omitempty"`
	// Histogram bucket boundaries and counts, on histogram points.
	Buckets json.RawMessage `json:"buckets,omitempty"`
	// Exemplar link: the trace that produced this observation.
	TraceID *string `json:"trace_id,omitempty"`
	// Exemplar link: the span that produced it. This is what turns a spike on a chart into
	// a specific run you can open.
	SpanID *string `json:"span_id,omitempty"`
	// Project and environment.
	Correlation Correlation `json:"correlation"`
	// When mcpd accepted it.
	ReceivedAt time.Time `json:"received_at"`
}

// Table is the storage table MetricPoint lives in.
func (MetricPoint) Table() string { return "metrics" }

// A machine evaluation of a span or a trace.
// Stored in the scores table.
type Score struct {
	// Unique id.
	ScoreID string `json:"score_id"`
	// The scored trace.
	TraceID *string `json:"trace_id,omitempty"`
	// The scored span, when the score is about one step rather than the whole run.
	SpanID *string `json:"span_id,omitempty"`
	// Which scorer produced this, such as `answer_relevance`.
	ScorerID string `json:"scorer_id"`
	// The scorer's version. A score is only comparable to another score from the same
	// version.
	ScorerVersion *string `json:"scorer_version,omitempty"`
	// Live, offline, experiment or manual.
	Source ScoreSource `json:"source"`
	// The numeric result.
	Value float64 `json:"value"`
	// Lower bound of the scale. Without it, a bare number is not interpretable.
	Min *float64 `json:"min,omitempty"`
	// Upper bound of the scale.
	Max *float64 `json:"max,omitempty"`
	// A categorical result, when the scorer produces one alongside the number.
	Label *string `json:"label,omitempty"`
	// Why. For an LLM judge this is the judge's explanation, and it is what makes a score
	// arguable rather than magic.
	Reason *string `json:"reason,omitempty"`
	// The trace of the scoring run itself, so a suspicious score can be debugged like any
	// other run.
	ScoreTraceID *string `json:"score_trace_id,omitempty"`
	// What producing this score cost. Judged evaluation is not free and mcpd refuses to
	// hide that.
	Cost *Cost `json:"cost,omitempty"`
	// Anything else.
	Metadata json.RawMessage `json:"metadata,omitempty"`
	// When the score was produced.
	Timestamp time.Time `json:"timestamp"`
	// Inherited from the target.
	Correlation Correlation `json:"correlation"`
}

// NewScore returns a Score with every spec-declared default applied.
func NewScore() Score {
	return Score{
		Min: ptrFloat64(0),
		Max: ptrFloat64(1),
	}
}

// Table is the storage table Score lives in.
func (Score) Table() string { return "scores" }

// A human evaluation. Kept distinct from `Score` because the two answer different
// questions and must never be averaged together.
// Stored in the feedback table.
type Feedback struct {
	// Unique id.
	FeedbackID string `json:"feedback_id"`
	// The trace being judged.
	TraceID *string `json:"trace_id,omitempty"`
	// The span being judged.
	SpanID *string `json:"span_id,omitempty"`
	// Thumbs, rating, correction, tag or comment.
	Kind FeedbackKind `json:"kind"`
	// The judgement. Its shape follows `kind`.
	Value json.RawMessage `json:"value"`
	// Free prose the person added.
	Comment *string `json:"comment,omitempty"`
	// Who gave it. Hashed by default.
	AuthorID *string `json:"author_id,omitempty"`
	// Whether the author was an end user, a reviewer or an operator.
	AuthorKind *string `json:"author_kind,omitempty"`
	// When it was given.
	Timestamp time.Time `json:"timestamp"`
	// Anything else.
	Metadata json.RawMessage `json:"metadata,omitempty"`
	// Inherited from the target.
	Correlation Correlation `json:"correlation"`
}

// Table is the storage table Feedback lives in.
func (Feedback) Table() string { return "feedback" }

// The tenancy boundary. Every signal belongs to exactly one project; retention,
// redaction, sampling, budgets and access are all configured per project.
// Stored in the projects table.
type Project struct {
	// Stable identifier, used in API paths and ingest keys.
	ProjectID string `json:"project_id"`
	// Display name.
	Name string `json:"name"`
	// When it was created.
	CreatedAt time.Time `json:"created_at"`
	// How long to keep signals. Absent means the server default.
	RetentionDays *uint32 `json:"retention_days,omitempty"`
	// The sampling policy for this project.
	Sampling json.RawMessage `json:"sampling,omitempty"`
	// Extra redaction rules, applied on top of the server defaults.
	Redaction json.RawMessage `json:"redaction,omitempty"`
	// Cost budget and what happens when it is exceeded.
	Budget json.RawMessage `json:"budget,omitempty"`
	// Anything else.
	Metadata json.RawMessage `json:"metadata,omitempty"`
}

// Table is the storage table Project lives in.
func (Project) Table() string { return "projects" }

// A fixed collection of inputs, with optional expected outputs, that an experiment
// runs against. Datasets are how a change is proved not to regress.
// Stored in the datasets table.
type Dataset struct {
	// Stable identifier.
	DatasetID string `json:"dataset_id"`
	// Display name.
	Name string `json:"name"`
	// What this dataset is for.
	Description *string `json:"description,omitempty"`
	// How many items it holds.
	ItemCount uint32 `json:"item_count"`
	// When it was created.
	CreatedAt time.Time `json:"created_at"`
	// Which project owns it.
	Correlation Correlation `json:"correlation"`
}

// Table is the storage table Dataset lives in.
func (Dataset) Table() string { return "datasets" }

// One case in a dataset.
// Stored in the dataset_items table.
type DatasetItem struct {
	// Stable identifier.
	ItemID string `json:"item_id"`
	// Which dataset this belongs to.
	DatasetID string `json:"dataset_id"`
	// What to feed the system under test.
	Input json.RawMessage `json:"input"`
	// The reference answer, when there is one. Absent for open-ended cases judged by a
	// scorer.
	Expected json.RawMessage `json:"expected,omitempty"`
	// Anything else, such as a difficulty label or a source citation.
	Metadata json.RawMessage `json:"metadata,omitempty"`
	// The production trace this case was captured from, when it was. This is how a bug
	// report becomes a permanent regression test.
	SourceTraceID *string `json:"source_trace_id,omitempty"`
}

// Table is the storage table DatasetItem lives in.
func (DatasetItem) Table() string { return "dataset_items" }

// One evaluation run of a system against a dataset. Comparing two experiments is how a
// change is accepted or rejected.
// Stored in the experiments table.
type Experiment struct {
	// Stable identifier.
	ExperimentID string `json:"experiment_id"`
	// Display name.
	Name string `json:"name"`
	// What it ran against.
	DatasetID string `json:"dataset_id"`
	// Where it got to.
	Status ExperimentStatus `json:"status"`
	// When it began.
	StartedAt *time.Time `json:"started_at,omitempty"`
	// When it ended.
	FinishedAt *time.Time `json:"finished_at,omitempty"`
	// Cases attempted.
	ItemCount uint32 `json:"item_count"`
	// Aggregate score per scorer, with mean, standard deviation and count.
	Summary json.RawMessage `json:"summary,omitempty"`
	// The experiment this one is compared against.
	BaselineID *string `json:"baseline_id,omitempty"`
	// What the whole run cost.
	Cost *Cost `json:"cost,omitempty"`
	// Why it failed, when it did.
	Error *string `json:"error,omitempty"`
	// The configuration under test: model, prompt version, git sha.
	Metadata json.RawMessage `json:"metadata,omitempty"`
	// Which project owns it.
	Correlation Correlation `json:"correlation"`
}

// Table is the storage table Experiment lives in.
func (Experiment) Table() string { return "experiments" }

// One recorded attempt to read stored payloads, or to delete them. Written by the
// handlers that hand prompts and completions back out of mcpd, so the question "who
// read this run" has an answer. The audit log is not trimmed by the retention pass: a
// log that deleted itself on the same schedule as the data it describes could not say
// who read the data that has since gone.
// Stored in the audit_log table.
type AuditEvent struct {
	// Unique id.
	AuditID string `json:"audit_id"`
	// When the request was handled.
	Timestamp time.Time `json:"timestamp"`
	// Which key acted, as a short hex digest of it. The key itself is never written here.
	// `anonymous` when the request carried no key, which is what an unprotected daemon
	// looks like.
	Principal string `json:"principal"`
	// The end identity a proxy asserted for this request. Always empty in this build. The
	// field exists from the first release so that proxy identity can be added later
	// without a migration.
	Subject string `json:"subject"`
	// What was done, named as the endpoint id: `get_trace`, `list_spans`, `delete_trace`.
	Action string `json:"action"`
	// The project the request was made against.
	Project string `json:"project"`
	// The trace or dataset the request named, when it named one.
	Resource *string `json:"resource,omitempty"`
	// Whether authorisation allowed the request or refused it.
	Result AuditResult `json:"result"`
}

// Table is the storage table AuditEvent lives in.
func (AuditEvent) Table() string { return "audit_log" }

// A stored definition. Immutable per version: a run pins the version it started
// against, and that version never changes underneath it.
// Stored in the definitions table.
type Definition struct {
	// Which project owns it.
	Project string `json:"project"`
	// Its identifier within the project.
	ID string `json:"id"`
	// Its version. A run started without one pins the highest stored version at that
	// moment.
	Version uint32 `json:"version"`
	// What kind of definition it is.
	Kind DefinitionKind `json:"kind"`
	// The document as stored, minus `version`.
	Body json.RawMessage `json:"body"`
	// When this version was stored.
	CreatedAt time.Time `json:"created_at"`
}

// Table is the storage table Definition lives in.
func (Definition) Table() string { return "definitions" }

// One execution of one definition version. The daemon owns the run between steps: its
// journal, its deadlines, its waits, its cancel and its trace. Your own process owns
// everything inside a step.
// Stored in the runs table.
type Run struct {
	// Its identifier, minted at start.
	RunID string `json:"run_id"`
	// Which project owns it.
	Project string `json:"project"`
	// Which definition it runs.
	DefinitionID string `json:"definition_id"`
	// Which version. Pinned at start and never re-resolved.
	DefinitionVersion uint32 `json:"definition_version"`
	// Where it is.
	Status RunStatus `json:"status"`
	// The frontier: where in the graph the run has reached.
	Cursor RunCursor `json:"cursor"`
	// What it was started with, after validation against the definition's `input_schema`.
	Input json.RawMessage `json:"input,omitempty"`
	// What the definition's `output` template rendered when the run reached the end of the
	// graph.
	Output json.RawMessage `json:"output,omitempty"`
	// A JSON object merged shallowly on every `complete` that sends one. Later steps read
	// it through the lease.
	State json.RawMessage `json:"state,omitempty"`
	// Why it failed, when it did: the failing step's message, or the ceiling it spent.
	Error *string `json:"error,omitempty"`
	// The context stored at start and copied onto every span the daemon writes for the
	// run, with `run_id` set to this run's id.
	Correlation Correlation `json:"correlation"`
	// The trace every span of this run belongs to. 32 lowercase hex characters, minted at
	// start.
	TraceID string `json:"trace_id"`
	// How many steps this run has completed. Counted against `max_steps` on the root of
	// the run tree.
	StepsUsed uint32 `json:"steps_used"`
	// The step ceiling from the definition's `limits`, when it declares one. On a root it
	// bounds the whole tree.
	MaxSteps *uint32 `json:"max_steps,omitempty"`
	// What the run's steps have cost so far, priced from the usage workers reported.
	CostUsd float64 `json:"cost_usd"`
	// The spend ceiling from `limits`, when declared. A claim refused for a spent ceiling
	// ends the run `failed`.
	MaxCostUsd *float64 `json:"max_cost_usd,omitempty"`
	// When `limits.run_timeout_ms` passes. Absent when the definition declares no timeout,
	// and once the run has ended.
	DeadlineAt *time.Time `json:"deadline_at,omitempty"`
	// The key the start request carried, when it carried one. A repost of it within 24
	// hours returns this run.
	IdempotencyKey *string `json:"idempotency_key,omitempty"`
	// The root of the run tree, which ceilings are enforced against. Equal to `run_id` on
	// a root.
	RootRunID string `json:"root_run_id"`
	// The run whose `subworkflow` step started this one.
	ParentRunID *string `json:"parent_run_id,omitempty"`
	// The step in the parent that this run is the body of.
	ParentStepKey *string `json:"parent_step_key,omitempty"`
	// How many parents lie between this run and the root. Zero on a root.
	Depth uint32 `json:"depth"`
	// When it started.
	CreatedAt time.Time `json:"created_at"`
	// When it last moved.
	UpdatedAt time.Time `json:"updated_at"`
	// When it reached a terminal status.
	EndedAt *time.Time `json:"ended_at,omitempty"`
}

// Table is the storage table Run lives in.
func (Run) Table() string { return "runs" }

// One attempt at one step of a run. A run's journal is these rows, append only: a
// retry is a new row, never a rewrite of the old one.
// Stored in the step_attempts table.
type StepAttempt struct {
	// Which run.
	RunID string `json:"run_id"`
	// Which step, with any enclosing branch, item or iteration folded into the key:
	// `refund/issue`, `classify[3]`, `poll[7]/classify`.
	StepKey string `json:"step_key"`
	// Which attempt this is. The fence: a lease for an earlier attempt is refused once a
	// later one exists.
	Attempt uint32 `json:"attempt"`
	// What kind of step.
	Kind StepKind `json:"kind"`
	// What a duplicated body would cost.
	Effect Effect `json:"effect"`
	// Where the attempt is.
	Status AttemptStatus `json:"status"`
	// The rendered input handed to the worker, on a claimable step.
	Input json.RawMessage `json:"input,omitempty"`
	// What the worker returned on `complete`, or the event payload on a resumed wait.
	Output json.RawMessage `json:"output,omitempty"`
	// Why it failed, when it did.
	Error *StepError `json:"error,omitempty"`
	// The token usage the worker reported on `complete`.
	Usage *StepUsage `json:"usage,omitempty"`
	// What the daemon priced that usage at.
	CostUsd *float64 `json:"cost_usd,omitempty"`
	// The span the daemon opened for this attempt. 16 lowercase hex characters, minted
	// when the row was written. A worker's own spans nest under it.
	SpanID string `json:"span_id"`
	// The `worker_id` holding the lease, while one does.
	LeaseOwner *string `json:"lease_owner,omitempty"`
	// When the lease lapses without a heartbeat.
	LeaseUntil *time.Time `json:"lease_until,omitempty"`
	// When the worker posted `started`. Absent when it never did, which is what the
	// reclaim rule reads.
	StartedAt *time.Time `json:"started_at,omitempty"`
	// When the lease lapsed.
	OrphanedAt *time.Time `json:"orphaned_at,omitempty"`
	// When the reclaim window closes.
	ReclaimUntil *time.Time `json:"reclaim_until,omitempty"`
	// How many attempts the node's retry budget still allows after this one.
	AttemptsLeft *uint32 `json:"attempts_left,omitempty"`
	// When it reached a terminal status.
	EndedAt *time.Time `json:"ended_at,omitempty"`
	// When the row was written.
	CreatedAt time.Time `json:"created_at"`
}

// Table is the storage table StepAttempt lives in.
func (StepAttempt) Table() string { return "step_attempts" }

// One row of a run's event log. Written in the transaction that made the change it
// describes, with `seq` allocated from the run header, so a reader that has seen a row
// has seen every row below it.
// Stored in the run_events table.
type RunEvent struct {
	// Which run.
	RunID string `json:"run_id"`
	// Its position in this run's stream, from zero and without gaps. Pass it as `after` to
	// reattach.
	Seq uint64 `json:"seq"`
	// What happened.
	Kind RunEventKind `json:"kind"`
	// The detail, shaped by `kind`.
	Payload json.RawMessage `json:"payload,omitempty"`
	// When it was written.
	At time.Time `json:"at"`
}

// Table is the storage table RunEvent lives in.
func (RunEvent) Table() string { return "run_events" }

// An axis that traces are described along. An aspect is a definition and not a
// measurement: it carries the name a note is filed under, the words an operator reads
// on screen and, where a model writes the sentence, the instruction handed to whatever
// produces it. Four ship with the daemon as ordinary rows rather than as a list
// compiled into a client, so there is one source of truth for what exists: goal, what
// the user wanted; sentiment, the tone they expressed; behaviour, what the agent did
// about it; and outcome, how the interaction ended. A stored version never changes, so
// a note recorded a month ago still names the definition it was written against.
// Stored in the aspects table.
type Aspect struct {
	// Which project owns it. Aspects are project-scoped, like every other tenancy boundary
	// here.
	Project string `json:"project"`
	// The aspect's name, lowercase with underscores, such as `goal` or `refund_reason`.
	// This is what a note is filed under and what every query parameter names.
	Name string `json:"name"`
	// The display name, such as `Goal`. Stored rather than derived from the name, so an
	// aspect reads the same in every screen and every language of SDK.
	Label string `json:"label"`
	// One line saying what this axis is about, shown beside the label so a reader does not
	// have to infer the axis from its name.
	Description string `json:"description"`
	// The text handed to whatever writes a note along this aspect, at most 2000 bytes. The
	// daemon stores it and hands it out; what reads it and writes the sentence sits
	// outside the daemon. Absent when `derived` is true, because a derived aspect needs no
	// instruction.
	Instruction *string `json:"instruction,omitempty"`
	// Whether the daemon writes this aspect's notes itself, with no model anywhere.
	// `outcome` ships derived, written from the run's status, the step error class, the
	// silent producer flag on the cause report and the divergence watch, so a deployment
	// with nothing configured still fills one aspect and can see the whole pipeline work
	// before a model is wired to anything.
	Derived bool `json:"derived"`
	// Whether the daemon seeded this aspect. A built-in is a row like any other and can be
	// switched off, but it cannot be deleted, because notes already name it.
	BuiltIn bool `json:"built_in"`
	// Whether notes are written along this aspect in this project. It is configuration and
	// read-only here. Switching an aspect on affects notes recorded afterwards and never
	// rewrites what is stored.
	Enabled bool `json:"enabled"`
	// Where this aspect sits in every list and in the default column order, ascending.
	// Stored, so the order an operator learns is the order they keep.
	SortOrder uint32 `json:"sort_order"`
	// This definition's version. Editing an aspect stores a new version rather than
	// changing this one, so analysis already recorded is never rewritten under a reader.
	Version uint32 `json:"version"`
	// When this version was stored.
	CreatedAt time.Time `json:"created_at"`
}

// Table is the storage table Aspect lives in.
func (Aspect) Table() string { return "aspects" }

// One sentence recorded against one trace along one aspect. A note proves one thing:
// that this sentence was recorded against that trace by the definition named in
// `aspect`, at the version in `definition_version`. It does not prove the sentence is
// true. Nothing here is checked against the trace it describes, so a note is evidence
// of what a definition said and not evidence of what happened, and the trace is what
// settles the difference. The daemon also keeps a small sketch of the wording, which
// is what clustering compares, and that sketch is deliberately not a field on this
// entity: it is a detail of how the pass is stored, a caller can do nothing with it,
// and publishing it would fix the hashing where it stands today. The text is payload.
// It passes through the same redactor as every other signal on the way in, retention
// sweeps it, and erasing a subject erases it.
// Stored in the aspect_notes table.
type AspectNote struct {
	// Unique id of the note. This identifies the sentence and never the run: a note id and
	// a trace id are different things and conflating them is the mistake this separate
	// name exists to prevent.
	NoteID string `json:"note_id"`
	// The trace this sentence is about.
	TraceID string `json:"trace_id"`
	// Which aspect this note is filed under, matching `Aspect.name`.
	Aspect string `json:"aspect"`
	// The sentence, after redaction. It is the only text the read API returns for a note,
	// and it is what the clustering pass reads.
	Text string `json:"text"`
	// The version of the aspect definition this note was written against. Editing a
	// definition mints a new version, so an older note keeps naming what was actually
	// asked for at the time.
	DefinitionVersion uint32 `json:"definition_version"`
	// Which model wrote the sentence, when one did. Absent for a derived aspect, where the
	// daemon wrote the note itself.
	Model *string `json:"model,omitempty"`
	// When the note was recorded. Writing a note lags the trace it describes, sometimes by
	// a long way, so a trace can be visible for hours before it carries one.
	CreatedAt time.Time `json:"created_at"`
	// Inherited from the trace, so a note is scoped, filtered, retained and erased exactly
	// as its trace is.
	Correlation Correlation `json:"correlation"`
}

// Table is the storage table AspectNote lives in.
func (AspectNote) Table() string { return "aspect_notes" }

// One clustering pass over one entity, covering a moving window of notes. A snapshot
// is what turns a clustering into a series: every theme belongs to exactly one, and
// every theme query names one. Windows overlap, so counts from two snapshots must
// never be added together. A trace whose note falls inside three windows is counted
// once in each of them, and summing the three counts it three times. Compare snapshots
// or read one; do not sum them. Reads never recompute either: a pass writes a snapshot
// on its own timer and every read is a lookup of what it wrote, so refreshing a screen
// shows the same numbers rather than a fresh clustering.
// Stored in the theme_snapshots table.
type ThemeSnapshot struct {
	// Opaque id, echoed back unchanged. Never build one by hand and never read meaning
	// into its shape.
	SnapshotID string `json:"snapshot_id"`
	// Which project this pass covered.
	Project string `json:"project"`
	// The kind of thing this pass covered.
	EntityKind EntityKind `json:"entity_kind"`
	// The name of that thing. An entity is the pair of a kind and a name, which spans
	// already carry, so nothing new is assigned to identify one.
	EntityName string `json:"entity_name"`
	// The aspects this pass covered, in catalogue order. An aspect with too few notes when
	// the pass ran is absent, so a missing name means it is still collecting and not that
	// anything failed.
	Aspects []string `json:"aspects"`
	// Where this snapshot falls in the entity's series, counting from one at the oldest
	// snapshot still stored.
	Ordinal uint64 `json:"ordinal"`
	// When this snapshot became the current state. A timeline is ordered by this and not
	// by the window bounds, because two passes can cover overlapping windows.
	CutoffAt time.Time `json:"cutoff_at"`
	// Start of the window of notes read, inclusive.
	WindowFrom time.Time `json:"window_from"`
	// End of the window of notes read, exclusive.
	WindowTo time.Time `json:"window_to"`
	// How many distinct traces the notes read belong to.
	TraceCount uint64 `json:"trace_count"`
	// How many notes the pass actually read. Every coverage figure in this snapshot is a
	// share of this number and never of the number of notes in the window, which is what
	// makes `capped` worth reading.
	NoteCount uint64 `json:"note_count"`
	// Whether the pass reached its ceiling of 20,000 notes and clustered the newest of
	// them rather than all of them. True means the window holds more notes than were read,
	// so the denominator of every share here is `note_count` and not the window.
	Capped bool `json:"capped"`
}

// Table is the storage table ThemeSnapshot lives in.
func (ThemeSnapshot) Table() string { return "theme_snapshots" }

// A cluster of aspect notes that share their wording, in one snapshot along one
// aspect. A theme groups aspect notes that share their wording, which is why mcpd
// needs no model and no embedding to build one. It does not read the sentences, so two
// notes that mean the same thing in different words land in separate themes, and a
// theme's label is the note most like the others rather than a summary written for it.
// The clustering is a pure function of the notes it reads, with no floating-point
// arithmetic in the comparison, so two replicas of one deployment reading the same
// notes write the same themes and cannot disagree about where a boundary falls.
// Stored in the themes table.
type Theme struct {
	// Opaque id, stable across snapshots, which is what makes a history possible. A theme
	// keeps its id while its wording holds, so the same id in two snapshots is the same
	// theme. Nothing is encoded in the shape of it.
	ThemeID string `json:"theme_id"`
	// The snapshot this theme belongs to.
	SnapshotID string `json:"snapshot_id"`
	// Which aspect it was clustered along. Clustering runs once per aspect, so one trace
	// can sit in a different theme in each of them.
	Aspect string `json:"aspect"`
	// The kind of entity this theme was built for.
	EntityKind EntityKind `json:"entity_kind"`
	// The name of that entity.
	EntityName string `json:"entity_name"`
	// The theme's label, which is the text of one of its own notes: the note most like the
	// rest of them. Nothing wrote it for the screen, so it reads as one real sentence
	// about one real trace rather than as a description of the group, and it can change
	// between snapshots as the group does.
	Label string `json:"label"`
	// The note the label is taken from.
	LabelNoteID string `json:"label_note_id"`
	// The trace that note was written about, so a reader can open the run behind the label
	// instead of taking the label's word for it.
	LabelTraceID string `json:"label_trace_id"`
	// What happened to this theme between the previous snapshot and this one.
	State ThemeState `json:"state"`
	// How many notes are in this theme.
	NoteCount uint64 `json:"note_count"`
	// How many distinct traces those notes were written about.
	TraceCount uint64 `json:"trace_count"`
	// This theme's share of the notes the snapshot read for this aspect, between 0 and 1.
	// The denominator is the snapshot's `note_count`, so in a capped snapshot it is a
	// share of what was read.
	Coverage float64 `json:"coverage"`
}

// Table is the storage table Theme lives in.
func (Theme) Table() string { return "themes" }
