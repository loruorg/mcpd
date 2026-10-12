// A durable workflow in TypeScript: the graph goes to the daemon as a
// definition, the step bodies stay in this process for a Worker to serve.
//
// The regions are read into the documentation at build time.
import { Mcpd, RetryableError, Worker, when, workflow } from '@mcpd/sdk'

const mcpd = new Mcpd({ project: 'support' })

async function deliver(text: string, idempotencyKey: string): Promise<boolean> {
  return text.length > 0 && idempotencyKey.length > 0
}

// #region workflow
const support = workflow({ id: 'support_ticket' })
  .step('classify', async (ctx, input: { ticket: string }) => ({
    kind: input.ticket.startsWith('Q') ? 'question' : 'bug',
  }))
  .agent(
    'draft',
    async (ctx, input: { kind: string }) => {
      // Your own model call, recorded under this step in the run's trace. Its
      // usage adds up into the step's, which the daemon prices.
      return await ctx.withSpan('model_call', async (span) => {
        span.setModel('claude-sonnet-4', 'anthropic')
        span.setUsage({ inputTokens: 812, outputTokens: 37 })
        return `an answer to a ${input.kind}`
      })
    },
    { input: { kind: '${steps.classify.output.kind}' } },
  )
  .waitFor('approval', {
    event: 'approved',
    key: '${input.ticket}',
    timeoutMs: 7 * 24 * 3_600_000,
    when: when('steps.classify.output.kind', 'eq', 'bug'),
  })
  .step(
    'send',
    async (ctx, input: { text: string }) => {
      if (!(await deliver(input.text, ctx.idempotencyKey))) {
        throw new RetryableError('mail relay busy', { retryAfterMs: 2_000 })
      }
      return { sent: true }
    },
    {
      input: { text: '${steps.draft.output}' },
      effect: 'at_most_once',
      retry: { maxAttempts: 3, backoff: 'exponential', initialMs: 500 },
    },
  )
  .output({ sent: '${steps.send.output.sent}' })
// #endregion workflow

// #region worker
// Serve step bodies for as long as this process lives. The worker registers
// the definition, long-polls for leases, heartbeats while a body runs, and
// answers every 409 by its reason.
const worker = new Worker(mcpd, { concurrency: 8 }).register(support)
void worker.serve()
// #endregion worker

// #region run
// Start a run from anywhere that holds an execute key, answer the wait from a
// webhook, then follow the journal to the end and read the run.
const run = await mcpd.runs.start({
  definitionId: 'support_ticket',
  input: { ticket: 'Q-42' },
})

await mcpd.events.publish({
  event: 'approved',
  key: 'Q-42',
  payload: { by: 'ops' },
  eventId: 'hook-1', // a repeat of this id is a duplicate, not a second event
})

for await (const event of mcpd.runs.watch(run.runId)) {
  console.log(event.seq, event.kind)
}
const detail = await mcpd.runs.get(run.runId)
console.log(detail.run.status, detail.run.output)
// #endregion run

await worker.stop()
await mcpd.shutdown()
