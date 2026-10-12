// The canonical run, served in TypeScript. Prints one JSON line at the end.
import { readFileSync } from 'node:fs'
import { Mcpd, RetryableError, Worker, workflow } from '../../../../sdks/typescript/dist/esm/index.js'

const canonical = JSON.parse(readFileSync(new URL('./canonical.json', import.meta.url), 'utf8'))
const mcpd = new Mcpd({ endpoint: process.env.MCPD_ENDPOINT!, project: process.env.MCPD_PROJECT!, flushOnExit: false })

const wf = workflow({ id: canonical.definition_id })
  .step('classify', (_ctx, input: { ticket: string }) => ({
    kind: input.ticket.startsWith('Q') ? 'question' : 'bug',
  }))
  .agent(
    'draft',
    async (ctx, input: { kind: string }) => {
      ctx.setState({ drafted: true })
      await ctx.withSpan('model_call', (span) => {
        span.setModel('claude-sonnet-4', 'anthropic')
        span.setUsage({ inputTokens: 120, outputTokens: 40 })
      }, { kind: 'model_generation' })
      return `an answer to a ${input.kind}`
    },
    { input: { kind: '${steps.classify.output.kind}' } },
  )
  .waitFor('approval', { event: 'approved', key: '${input.ticket}', timeoutMs: 60_000, acceptParked: true })
  .step(
    'send',
    (ctx, input: { text: string; by: string }) => {
      if (ctx.attempt === 0) throw new RetryableError('relay busy', { retryAfterMs: 100 })
      return { sent: true, by: input.by }
    },
    {
      input: { text: '${steps.draft.output}', by: '${steps.approval.output.by}' },
      effect: 'at_most_once',
      retry: { maxAttempts: 2, backoff: 'fixed', initialMs: 100 },
    },
  )
  .output({ answer: '${steps.draft.output}', sent: '${steps.send.output.sent}', by: '${steps.send.output.by}' })

const worker = new Worker(mcpd, { workerId: 'probe-typescript', waitMs: 1_000, onError: (m, e) => { throw new Error(`${m}: ${String(e)}`) } }).register(wf)
await worker.registerDefinitions()

const run = await mcpd.runs.start({
  definitionId: canonical.definition_id,
  input: canonical.input,
  correlation: { sessionId: canonical.correlation.session_id, userId: canonical.correlation.user_id },
})
const first = await mcpd.events.publish({ event: canonical.event.name, key: canonical.event.key, payload: canonical.event.payload, eventId: canonical.event.event_id })
const second = await mcpd.events.publish({ event: canonical.event.name, key: canonical.event.key, payload: canonical.event.payload, eventId: canonical.event.event_id })

let detail = await mcpd.runs.get(run.runId)
for (let i = 0; i < 60 && detail.run.status === 'running'; i++) {
  await worker.claim(4, run.runId)
  await worker.drain()
  detail = await mcpd.runs.get(run.runId)
  if (detail.run.status === 'running') await new Promise((r) => setTimeout(r, 100))
}
await mcpd.shutdown()
console.log(JSON.stringify({ run_id: run.runId, trace_id: run.traceId, status: detail.run.status, publish: [first.outcome, second.outcome] }))
