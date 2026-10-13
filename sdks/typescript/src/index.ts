/**
 * mcpd for TypeScript and JavaScript.
 *
 * ```ts
 * import { Mcpd } from '@mcpd/sdk'
 *
 * const mcpd = new Mcpd({ endpoint: 'http://localhost:4317', project: 'support' })
 *
 * await mcpd.withSpan('answer_question', async (run) => {
 *   run.setInput({ question: 'where is my order?' })
 *   run.setOutput({ answer: 'it ships tomorrow' })
 * })
 *
 * await mcpd.shutdown()
 * ```
 *
 * Nothing exported from here throws while recording. Reads throw, because a
 * query you awaited and cannot answer is news.
 */

export { Mcpd } from './client.js'
export type {
  BaseQuery,
  CostQuery,
  FeedbackInput,
  FeedbackQuery,
  LogQuery,
  McpdOptions,
  McpdStats,
  MetricQuery,
  ScoreInput,
  ScoreQuery,
  SignalTarget,
  SpanQuery,
  TraceOptions,
  TraceQuery,
  WindowQuery,
} from './client.js'

export { McpdSpan } from './span.js'
export { responseFacts, responseShape, usageFromAnthropic, usageFromOpenAI } from './model-call.js'
export type { ResponseFacts } from './model-call.js'
export { STREAM_MAX_TOOL_CALLS, STREAM_TEXT_BUDGET } from './stream.js'
export type { ErrorOptions, ModelParameterInput, SpanHost, SpanOptions, UsageReport } from './span.js'

// Where batches go: the HTTP transport by default, memory for a test, the
// console for a look.
export { ConsoleSink, RecordingSink } from './sinks.js'
export type { ConsoleSinkOptions, Sink } from './sinks.js'

// Durable workflows: the builder, the worker that serves them, and the run
// surface of the daemon as a client.
export { Workflow, BranchBuilder, workflow, when, all, any, not } from './workflow.js'
export type {
  Limits,
  NodeOptions,
  Predicate,
  PredicateOp,
  Prior,
  PriorStep,
  RetryPolicy,
  StepHandler,
  StepOptions,
  WorkflowOptions,
} from './workflow.js'
export { RetryableError, StepContext, StepFailure, Worker } from './worker.js'
export type { StepOutcome, WorkerOptions, WorkerStats } from './worker.js'
export { DefinitionsApi, EventsApi, RunsApi, StepConflictError, RunFailedError, StepsApi } from './runs.js'
export type { Scoped, StartRunOptions, RunResult } from './runs.js'

// The logging signal: severity numbers both ways, the level floor, and the
// serialiser that keeps an error from collapsing to an empty object.
export {
  describeError,
  encodeFields,
  logLevelFromSeverity,
  logSeverity,
  meetsLevel,
  parseLogLevel,
} from './logging.js'
export type { ErrorFields } from './logging.js'

export { McpdError } from './transport.js'
export type { Diagnostics, FetchLike, QueryValue } from './transport.js'

export type { QueueStats, Signal } from './queue.js'

export {
  formatTraceparent,
  isSpanId,
  isTraceId,
  newSpanId,
  newTraceId,
  parseTraceparent,
} from './context.js'
export type { SpanContext } from './context.js'

// The data model, generated from spec/mcpd.spec.json. Never hand-edited.
export * from './generated.js'
