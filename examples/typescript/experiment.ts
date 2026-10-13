// An experiment in TypeScript: run every case of a dataset through your own
// code, score each answer against its case, and read the results.
//
// The regions are read into the documentation at build time.
import { Mcpd } from '@mcpd/sdk'

const mcpd = new Mcpd({ project: 'support' })

// The code under test. Anything it records nests under the case's run.
async function answer(input: unknown): Promise<string> {
  const { question } = input as { question: string }
  return await mcpd.withSpan('model_call', async (span) => {
    span.setModel('claude-sonnet-4', 'anthropic')
    return question.includes('refund') ? 'Refunds take five days.' : 'It ships tomorrow.'
  })
}

// #region experiment
// Each case runs in its own root span, four at a time. A case that throws is
// recorded as a failed run, and the others still run.
const results = await mcpd.runExperiment('support-questions', answer, {
  name: 'answers-v2',
  scorers: ['exact_match', 'similarity'],
  metadata: { prompt: 'answer@4' },
})

for (const scorer of results.scorers) {
  console.log(`${scorer.scorerId}: mean ${scorer.mean.toFixed(2)} over ${scorer.n}`)
}
for (const item of results.items) {
  if (item.status === 'error') console.log(`case ${item.itemId} failed: ${item.traceId}`)
}

// Read one run back as the steps its agent took.
const first = results.items[0]
if (first !== undefined) {
  const trajectory = await mcpd.getTrajectory(first.traceId)
  console.log(`${trajectory.stepCount} steps, tools: ${trajectory.toolSequence.join(' > ')}`)
}
// #endregion experiment

await mcpd.shutdown()
