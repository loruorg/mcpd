/**
 * The experiment runner against a running daemon, with every empty shape it
 * sends. Skipped unless `MCPD_TEST_ENDPOINT` is set, like live.test.ts.
 */

import { describe, expect, it } from 'vitest'
import { Mcpd, newTraceId } from '../src/index.js'

const env = (globalThis as { process?: { env?: Record<string, string | undefined> } }).process
  ?.env
const endpoint = env?.['MCPD_TEST_ENDPOINT']

/** A dataset holding `cases`, written with plain fetch so only the runner is under test. */
async function seedDataset(project: string, cases: Array<{ input: unknown; expected: unknown }>) {
  const datasetId = `ts-live-${newTraceId().slice(0, 12)}`
  const post = async (path: string, body: unknown) => {
    const res = await fetch(`${endpoint}${path}`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
    })
    if (!res.ok) throw new Error(`${path} -> ${res.status} ${await res.text()}`)
  }
  await post('/v1/api/datasets', {
    dataset_id: datasetId,
    name: datasetId,
    item_count: 0,
    created_at: '2026-01-01T00:00:00Z',
    correlation: { project },
  })
  await post(
    `/v1/api/datasets/${datasetId}/items`,
    cases.map((c, i) => ({ item_id: `${datasetId}-${i}`, dataset_id: datasetId, ...c })),
  )
  return datasetId
}

describe.skipIf(!endpoint)('an experiment against a live daemon', () => {
  it('runs over nulls and empty lists and scores with no settings', async () => {
    const project = `sdk-live-exp-${newTraceId().slice(0, 8)}`
    const mcpd = new Mcpd({ endpoint: endpoint as string, project, flushOnExit: false })
    const nested = { q: 'hi', n: null, l: [] }
    const datasetId = await seedDataset(project, [
      { input: nested, expected: 'hi' },
      { input: null, expected: 'x' },
    ])
    const seen: unknown[] = []
    const results = await mcpd.runExperiment(
      datasetId,
      (input) => {
        seen.push(input)
        return 'hi'
      },
      { config: { needle: 'hi' } },
    )
    expect(seen).toEqual(expect.arrayContaining([nested, null]))
    expect([results.runs, results.failed, results.items.length]).toEqual([2, 0, 2])

    const recorded: Record<string, unknown> = {}
    for (const item of results.items) {
      const detail = await mcpd.getTrace(item.traceId)
      recorded[item.itemId ?? ''] = detail.spans.find((s) => s.isRoot)?.input
    }
    expect(recorded[`${datasetId}-0`]).toEqual(nested)
    expect(recorded[`${datasetId}-1`] ?? null).toBeNull()

    const experiment = await mcpd.createExperiment({
      experimentId: '',
      name: 'ts-live-empty',
      datasetId,
      status: 'running',
      itemCount: 0,
      correlation: { project },
    })
    await mcpd.scoreExperiment(experiment.experimentId)
    await mcpd.scoreExperiment(experiment.experimentId, [], {
      forbidden: [],
      keywords: [],
      expectedTools: [],
      forbiddenTools: [],
      context: [],
    })
    await mcpd.shutdown()
  }, 30_000)
})
