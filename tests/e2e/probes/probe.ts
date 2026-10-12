// The canonical run, in TypeScript. Prints its trace id on the last line.
import { Mcpd, SpanKind, FeedbackKind, LogLevel, MetricKind } from '../../../sdks/typescript/dist/esm/index.js'

const mcpd = new Mcpd({
  endpoint: process.env.MCPD_ENDPOINT!,
  project: process.env.MCPD_PROJECT!,
})

const run = mcpd.trace('support_agent', {
  kind: SpanKind.AgentRun,
  input: 'where is my order 41?',
  correlation: {
    sessionId: 'sess-conformance',
    threadId: 'thread-7',
    userId: 'user-41',
    requestId: 'req-9001',
    release: '2026.08.1',
  },
})

const gen = run.child('plan_answer', { kind: SpanKind.ModelGeneration })
gen.setModel('gpt-4o', 'openai')
gen.setUsage({ inputTokens: 1180, outputTokens: 96 })
gen.end()

run.child('lookup_order', { kind: SpanKind.ToolCall }).end()

const failing = run.child('estimate_delivery', { kind: SpanKind.ToolCall })
failing.setError('shipping service returned 503')
failing.end()

// The log line and the metric point belong to the run, so both are recorded
// with the root span active. That is how a user writes it, and it is what makes
// `trace_id` on a log and an exemplar on a metric arrive at all.
mcpd.withActiveSpan(run, () => {
  mcpd.log(LogLevel.Warn, 'estimate_delivery failed, answering from the cached estimate', {
    step: 'estimate_delivery',
  })
  mcpd.metric('conformance.run_steps', 3, MetricKind.Counter, {
    labels: { outcome: 'degraded', lang: 'typescript' },
    unit: '{step}',
  })
})

run.end()

mcpd.score({ traceId: run.traceId, scorerId: 'answer_relevance', value: 0.86, min: 0, max: 1, source: 'live' })
// No source: the suite compares what each SDK defaults to.
mcpd.score({ traceId: run.traceId, scorerId: 'scale_check', value: 3, min: 1, max: 5 })
mcpd.feedback({ traceId: run.traceId, kind: FeedbackKind.Thumbs, value: 'up' })
await mcpd.shutdown()
console.log(run.traceId)
