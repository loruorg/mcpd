import { describe, expect, it } from 'vitest'
import { Mcpd } from '../src/index.js'
import type { FetchLike } from '../src/index.js'
import { TestClient } from '../src/testing.js'

interface Call {
  method: string
  url: string
  body: unknown
}

/** A daemon that answers by method and path, and records every request. */
function daemon(routes: Record<string, (body: unknown) => { status?: number; body: unknown }>) {
  const calls: Call[] = []
  const fetch: FetchLike = async (url, init) => {
    const method = init.method ?? 'GET'
    const body = typeof init.body === 'string' ? JSON.parse(init.body) : undefined
    calls.push({ method, url, body })
    const path = new URL(url).pathname
    const route = routes[`${method} ${path}`]
    if (route === undefined) return new Response('{"error":"no route"}', { status: 404 })
    const answer = route(body)
    return new Response(JSON.stringify(answer.body), {
      status: answer.status ?? 200,
      headers: { 'content-type': 'application/json' },
    })
  }
  return { fetch, calls }
}

const RESULTS = {
  experiment_id: 'exp1',
  items: [{ trace_id: 't1', item_id: 'a', status: 'ok', scores: { exact_match: 1 } }],
  scorers: [{ scorer_id: 'exact_match', n: 2, mean: 0.5, min: 0, max: 1, direction: 'higher_is_better' }],
  runs: 3,
  failed: 1,
}

function experimentRoutes(items: unknown[]) {
  return {
    'POST /v1/api/experiments': (body: unknown) => ({
      body: { ...(body as object), experiment_id: 'exp1' },
    }),
    'GET /v1/api/datasets/ds1/items': () => ({ body: items }),
    'POST /v1/api/experiments/exp1/score': () => ({
      body: { traces_scored: 3, scores_written: 3, skipped: [], scores: [] },
    }),
    'GET /v1/api/experiments/exp1/results': () => ({ body: RESULTS }),
  }
}

const ITEMS = [
  { item_id: 'a', dataset_id: 'ds1', input: { question: 'capital of France', userName: 'ada' } },
  { item_id: 'b', dataset_id: 'ds1', input: { question: 'boom' } },
  { item_id: 'c', dataset_id: 'ds1', input: { question: 'capital of Spain' } },
]

describe('experiment reads and writes', () => {
  it('sends each on its path with its body', async () => {
    const fake = daemon({
      'GET /v1/api/traces/t1/trajectory': () => ({
        body: { trace_id: 't1', steps: [], tool_sequence: ['search'], step_count: 1, tool_count: 1, error_count: 0, goal_reached: true },
      }),
      ...experimentRoutes(ITEMS),
      'GET /v1/api/traces': () => ({ body: { traces: [], page: { has_more: false } } }),
    })
    const mcpd = new Mcpd({ endpoint: 'http://daemon.test', project: 'p', fetch: fake.fetch, flushOnExit: false })

    const trajectory = await mcpd.getTrajectory('t1')
    expect(trajectory.toolSequence).toEqual(['search'])
    const items = await mcpd.listDatasetItems('ds1', { limit: 10 })
    expect(items[0]?.input).toEqual({ question: 'capital of France', userName: 'ada' })
    await mcpd.createExperiment({
      experimentId: '', name: 'n', datasetId: 'ds1', status: 'pending', itemCount: 0,
      correlation: { project: '' }, metadata: { promptVersion: 3 },
    })
    await mcpd.scoreExperiment('exp1', ['json_schema'], { jsonSchema: { required: ['userName'] } })
    const results = await mcpd.experimentResults('exp1')
    expect(results.items[0]?.scores).toEqual({ exact_match: 1 })
    expect(results.scorers[0]?.scorerId).toBe('exact_match')
    await mcpd.listTraces({ experimentId: 'exp1', preview: true })

    expect(fake.calls.map((c) => `${c.method} ${c.url}`)).toEqual([
      'GET http://daemon.test/v1/api/traces/t1/trajectory?project=p',
      'GET http://daemon.test/v1/api/datasets/ds1/items?limit=10',
      'POST http://daemon.test/v1/api/experiments',
      'POST http://daemon.test/v1/api/experiments/exp1/score?project=p',
      'GET http://daemon.test/v1/api/experiments/exp1/results?project=p',
      'GET http://daemon.test/v1/api/traces?project=p&experiment_id=exp1&preview=true',
    ])
    expect(fake.calls[2]?.body).toEqual({
      experiment_id: '', name: 'n', dataset_id: 'ds1', status: 'pending', item_count: 0,
      correlation: { project: 'p' }, metadata: { promptVersion: 3 },
    })
    expect(fake.calls[3]?.body).toEqual({
      scorers: ['json_schema'],
      config: { json_schema: { required: ['userName'] } },
    })
    await mcpd.shutdown()
  })
})

describe('runExperiment', () => {
  it('runs every case in its own span, records an error and carries on, then scores', async () => {
    const fake = daemon(experimentRoutes(ITEMS))
    const mcpd = new TestClient({ endpoint: 'http://daemon.test', project: 'p', fetch: fake.fetch, sample: 0 })

    const results = await mcpd.runExperiment(
      'ds1',
      async (input) => {
        const question = (input as { question: string }).question
        if (question === 'boom') throw new Error('the model refused')
        await mcpd.withSpan('lookup', () => undefined)
        return question.endsWith('France') ? 'Paris' : 'Madrid'
      },
      { name: 'capitals-v2', scorers: ['exact_match'], metadata: { prompt: 'v2' } },
    )
    expect(results.runs).toBe(3)

    const roots = await mcpd.spans({ name: 'capitals-v2' })
    expect(roots).toHaveLength(3)
    const byItem = new Map(roots.map((s) => [(s.attributes as Record<string, string>)['mcpd.dataset_item_id'], s]))
    for (const span of roots) {
      expect(span.kind).toBe('agent_run')
      expect(span.correlation.experimentId).toBe('exp1')
      expect(span.parentSpanId).toBeUndefined()
    }
    expect(byItem.get('a')?.input).toEqual({ question: 'capital of France', userName: 'ada' })
    expect(byItem.get('a')?.output).toBe('Paris')
    expect(byItem.get('a')?.status).toBe('ok')
    expect(byItem.get('b')?.status).toBe('error')
    expect(byItem.get('b')?.error?.message).toBe('the model refused')
    expect(byItem.get('c')?.output).toBe('Madrid')
    // A span the target opens is a child of its case's run.
    const lookups = await mcpd.spans({ name: 'lookup' })
    expect(lookups.map((s) => s.parentSpanId).sort()).toEqual(
      [byItem.get('a')?.spanId, byItem.get('c')?.spanId].sort(),
    )

    const posts = fake.calls.filter((c) => c.url === 'http://daemon.test/v1/api/experiments')
    expect(posts).toHaveLength(2)
    expect(posts[0]?.body).toMatchObject({
      experiment_id: '', name: 'capitals-v2', dataset_id: 'ds1', status: 'running',
      correlation: { project: 'p' }, metadata: { prompt: 'v2' },
    })
    expect((posts[0]?.body as { started_at?: string }).started_at).toMatch(/^\d{4}-/)
    expect(posts[1]?.body).toMatchObject({
      experiment_id: 'exp1', status: 'succeeded', item_count: 3, summary: { exact_match: 0.5 },
    })
    expect((posts[1]?.body as { finished_at?: string }).finished_at).toMatch(/^\d{4}-/)
    const score = fake.calls.find((c) => c.url.includes('/score'))
    expect(score?.body).toEqual({ scorers: ['exact_match'] })
    expect(fake.calls.find((c) => c.url.includes('/items'))?.url).toBe(
      'http://daemon.test/v1/api/datasets/ds1/items?limit=5000',
    )
    await mcpd.shutdown()
  })

  it('never has more than `concurrency` targets in flight', async () => {
    const many = Array.from({ length: 12 }, (_, i) => ({ item_id: `i${i}`, dataset_id: 'ds1', input: i }))
    const fake = daemon(experimentRoutes(many))
    const mcpd = new TestClient({ endpoint: 'http://daemon.test', project: 'p', fetch: fake.fetch })
    let inFlight = 0
    let most = 0
    await mcpd.runExperiment(
      'ds1',
      async (input) => {
        inFlight++
        most = Math.max(most, inFlight)
        await new Promise((r) => setTimeout(r, 5))
        inFlight--
        return input
      },
      { concurrency: 3 },
    )
    expect(most).toBe(3)
    expect(await mcpd.spans({ name: 'ds1' })).toHaveLength(12)
    await mcpd.shutdown()
  })

  it('marks the experiment failed and throws when a read fails', async () => {
    const routes = experimentRoutes(ITEMS) as Record<string, (body: unknown) => { status?: number; body: unknown }>
    routes['GET /v1/api/datasets/ds1/items'] = () => ({ status: 403, body: { error: 'dataset belongs to another project' } })
    const fake = daemon(routes)
    const mcpd = new TestClient({ endpoint: 'http://daemon.test', project: 'p', fetch: fake.fetch })
    let called = false
    await expect(
      mcpd.runExperiment('ds1', () => {
        called = true
      }),
    ).rejects.toThrow(/403/)
    expect(called).toBe(false)
    const posts = fake.calls.filter((c) => c.url === 'http://daemon.test/v1/api/experiments')
    expect(posts.map((p) => (p.body as { status: string }).status)).toEqual(['running', 'failed'])
    expect((posts[1]?.body as { error?: string }).error).toMatch(/403/)
    await mcpd.shutdown()
  })

  it('refuses a concurrency below 1 and a disabled client before any request', async () => {
    const fake = daemon(experimentRoutes(ITEMS))
    const mcpd = new TestClient({ endpoint: 'http://daemon.test', project: 'p', fetch: fake.fetch })
    await expect(mcpd.runExperiment('ds1', () => 1, { concurrency: 0 })).rejects.toThrow(/at least 1/)
    const off = new Mcpd({ endpoint: 'http://daemon.test', project: 'p', fetch: fake.fetch, enabled: false, flushOnExit: false })
    await expect(off.runExperiment('ds1', () => 1)).rejects.toThrow(/enabled/)
    expect(fake.calls).toHaveLength(0)
    await mcpd.shutdown()
  })
})
