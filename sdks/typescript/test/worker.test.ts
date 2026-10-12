import { describe, expect, it, vi } from 'vitest'
import {
  Mcpd,
  McpdError,
  RetryableError,
  RunFailedError,
  StepConflictError,
  StepFailure,
  Worker,
  workflow,
} from '../src/index.js'
import type { FetchLike, StepHandler } from '../src/index.js'
import { fakeFetch } from './helpers.js'

type Errors = Array<{ message: string; error: unknown }>

const collect = (errors: Errors) => (message: string, error: unknown) => {
  errors.push({ message, error })
}

function client(fetchImpl: ReturnType<typeof fakeFetch>['fetch'], errors: Errors = []): Mcpd {
  return new Mcpd({
    endpoint: 'http://daemon.test',
    project: 'test',
    fetch: fetchImpl,
    flushIntervalMs: 10_000,
    flushOnExit: false,
    onError: (message, error) => errors.push({ message, error }),
  })
}

const TRACE = 'a1'.repeat(16)
const STEP_SPAN = 'b2'.repeat(8)

/** A lease as the daemon spells it on the wire. */
function wireLease(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    lease_id: 'lease-1',
    run_id: 'run-1',
    project: 'test',
    definition_id: 'support',
    definition_version: 1,
    step_key: 'classify',
    kind: 'task',
    attempt: 0,
    effect: 'at_least_once',
    expires_at: '2026-09-01T10:00:00Z',
    idempotency_key: 'run-1:classify:0',
    input: { ticket_id: 'T-1' },
    prior: { triage: { status: 'completed', output: { queue_name: 'billing' } } },
    state: { seen_count: 1 },
    correlation: { project: 'test', session_id: 's-1' },
    trace_id: TRACE,
    span_id: STEP_SPAN,
    parent_span_id: 'c3'.repeat(8),
    ...overrides,
  }
}

const claimed = (...leases: Record<string, unknown>[]) => ({ status: 200, body: { leases } })
const idle = { status: 200, body: { leases: [] } }
const started = { status: 200, body: { expires_at: '2026-09-01T10:00:30Z', reclaimed: false } }
const acked = { status: 200, body: { run_status: 'running', next: 'claim' } }
const registered = {
  status: 200,
  body: {
    worker_id: 'w-1',
    registered: [{ id: 'support', version: 1, assigned: true }],
    lease_ttl_ms: 3_000,
    heartbeat_ms: 1_000,
  },
}
const conflict = (reason: string, extra: Record<string, unknown> = {}) => ({
  status: 409,
  body: {
    error: 'conflict',
    reason,
    message: `the daemon says ${reason}`,
    attempt: 0,
    run_status: 'running',
    ...extra,
  },
})

function paths(fake: ReturnType<typeof fakeFetch>): string[] {
  return fake.calls.map((c) => `${c.method} ${c.url.replace('http://daemon.test', '')}`)
}

describe('serving one lease', () => {
  it('posts started, runs the body with its input, prior and state, then completes', async () => {
    const fake = fakeFetch([claimed(wireLease()), started, acked])
    const errors: Errors = []
    const mcpd = client(fake.fetch, errors)
    const seen: unknown[] = []
    const wf = workflow({ id: 'support' }).step('classify', (ctx, input, prior) => {
      seen.push(input, prior, ctx.state, ctx.idempotencyKey, ctx.attempt, ctx.runId, ctx.stepKey)
      ctx.setState({ seen_count: 2, last_ticket: 'T-1' })
      ctx.setUsage({ inputTokens: 10, outputTokens: 5 })
      return { kind: 'question' }
    })
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(wf)

    expect(await worker.claim(1)).toBe(1)
    await worker.drain()

    expect(paths(fake)).toEqual([
      'POST /v1/api/steps/claim',
      'POST /v1/api/steps/lease-1/started',
      'POST /v1/api/steps/lease-1/complete',
    ])
    expect(fake.calls[0]?.body).toEqual({ project: 'test', worker_id: 'w-1', max: 1, wait_ms: 0 })
    // Opaque objects reach the body spelled as the daemon sent them.
    expect(seen).toEqual([
      { ticket_id: 'T-1' },
      { triage: { status: 'completed', output: { queue_name: 'billing' } } },
      { seen_count: 1 },
      'run-1:classify:0',
      0,
      'run-1',
      'classify',
    ])
    expect(fake.calls[2]?.body).toEqual({
      output: { kind: 'question' },
      state: { seen_count: 2, last_ticket: 'T-1' },
      usage: { input_tokens: 10, output_tokens: 5 },
    })
    expect(errors).toEqual([])
    expect(worker.inflight).toBe(0)
    await mcpd.shutdown()
  })

  it('sends null for a body that returns nothing', async () => {
    const fake = fakeFetch([claimed(wireLease()), started, acked])
    const mcpd = client(fake.fetch)
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(
      workflow({ id: 'support' }).step('classify', () => undefined),
    )
    await worker.claim(1)
    await worker.drain()
    expect(fake.calls[2]?.body).toEqual({ output: null })
    await mcpd.shutdown()
  })

  it('records a span from the body under the step span the daemon opened', async () => {
    const fake = fakeFetch([
      claimed(wireLease()),
      started,
      acked,
      { status: 200, body: { accepted: 1, rejected: 0, duplicate: 0 } },
    ])
    const mcpd = client(fake.fetch)
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(
      workflow({ id: 'support' }).agent('classify', async (ctx) => {
        return await ctx.withSpan('model_call', (span) => {
          span.setModel('claude-sonnet-4', 'anthropic')
          return 'ok'
        })
      }),
    )
    await worker.claim(1)
    await worker.drain()
    await mcpd.flush()

    const spans = fake.spans()
    expect(spans).toHaveLength(1)
    expect(spans[0]?.['name']).toBe('model_call')
    expect(spans[0]?.['trace_id']).toBe(TRACE)
    expect(spans[0]?.['parent_span_id']).toBe(STEP_SPAN)
    await mcpd.shutdown()
  })

  it('keeps the run correlation when a span option names a field with no value', async () => {
    const fake = fakeFetch([
      claimed(
        wireLease({
          correlation: {
            project: 'test',
            session_id: 's-1',
            thread_id: 't-1',
            user_id: 'u-1',
            request_id: 'r-1',
            release: '2026.09.1',
          },
        }),
      ),
      started,
      acked,
      { status: 200, body: { accepted: 1, rejected: 0, duplicate: 0 } },
    ])
    const mcpd = client(fake.fetch)
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(
      workflow({ id: 'support' }).agent('classify', async (ctx) => {
        // `sessionId` is named with no value, which is not the same as being
        // told to erase it: the run's session has to reach the span anyway.
        // `threadId` carries a value, so that one wins.
        return await ctx.withSpan('model_call', () => 'ok', {
          correlation: { sessionId: undefined, threadId: 't-2' },
        })
      }),
    )
    await worker.claim(1)
    await worker.drain()
    await mcpd.flush()

    const correlation = fake.spans()[0]?.['correlation'] as Record<string, unknown>
    expect(correlation).toMatchObject({
      session_id: 's-1',
      thread_id: 't-2',
      user_id: 'u-1',
      request_id: 'r-1',
      release: '2026.09.1',
    })
    await mcpd.shutdown()
  })

  it('finds the handler through a branch, an item and an iteration key', async () => {
    const fake = fakeFetch([
      claimed(
        wireLease({ lease_id: 'l-a', step_key: 'checks/fraud' }),
        wireLease({ lease_id: 'l-b', step_key: 'notify[2]' }),
        wireLease({ lease_id: 'l-c', step_key: 'refine[1]' }),
      ),
      started,
      started,
      started,
      acked,
      acked,
      acked,
    ])
    const mcpd = client(fake.fetch)
    const ran: string[] = []
    const record =
      (name: string): StepHandler =>
      () => {
        ran.push(name)
        return name
      }
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(
      workflow({ id: 'support' })
        .parallel('checks', (b) => b.step('fraud', record('fraud')))
        .foreach('notify', { over: 'input.to', maxItems: 3 }, (b) => b.step('send', record('send')))
        .loop('refine', { maxIterations: 2 }, (b) => b.step('rewrite', record('rewrite'))),
    )
    expect(await worker.claim(3)).toBe(3)
    await worker.drain()
    expect(ran.sort()).toEqual(['fraud', 'rewrite', 'send'])
    expect(paths(fake).filter((p) => p.endsWith('/complete'))).toHaveLength(3)
    await mcpd.shutdown()
  })
})

describe('failing a lease', () => {
  it('reports a RetryableError with its class and delay', async () => {
    const fake = fakeFetch([claimed(wireLease()), started, { ...acked, body: { ...acked.body, attempts_left: 2 } }])
    const mcpd = client(fake.fetch)
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(
      workflow({ id: 'support' }).step('classify', () => {
        throw new RetryableError('rate limited', { retryAfterMs: 250, details: { status_code: 429 } })
      }),
    )
    await worker.claim(1)
    await worker.drain()
    expect(paths(fake)[2]).toBe('POST /v1/api/steps/lease-1/fail')
    expect(fake.calls[2]?.body).toEqual({
      error: {
        class: 'retryable',
        message: 'rate limited',
        details: { status_code: 429 },
        retry_after_ms: 250,
      },
    })
    await mcpd.shutdown()
  })

  it('reports any other throw as fatal, and a StepFailure with the class it names', async () => {
    const fake = fakeFetch([claimed(wireLease(), wireLease({ lease_id: 'lease-2', step_key: 'draft' })), started, started, acked, acked])
    const mcpd = client(fake.fetch)
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(
      workflow({ id: 'support' })
        .step('classify', () => {
          throw new Error('the model returned nothing')
        })
        .step('draft', () => {
          throw new StepFailure('no such ticket', { class: 'invalid_input' })
        }),
    )
    await worker.claim(2)
    await worker.drain()
    const fails = fake.calls.filter((c) => c.url.endsWith('/fail')).map((c) => c.body)
    expect(fails).toEqual(
      expect.arrayContaining([
        { error: { class: 'fatal', message: 'the model returned nothing' } },
        { error: { class: 'invalid_input', message: 'no such ticket' } },
      ]),
    )
    await mcpd.shutdown()
  })

  it('hands back a lease it cannot serve without running anything', async () => {
    const fake = fakeFetch([
      claimed(wireLease({ definition_id: 'billing' }), wireLease({ lease_id: 'lease-2', definition_version: 7 })),
      acked,
      acked,
    ])
    const mcpd = client(fake.fetch)
    let ran = false
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(
      workflow({ id: 'support', version: 1 }).step('classify', () => {
        ran = true
      }),
    )
    await worker.claim(2)
    await worker.drain()
    expect(ran).toBe(false)
    expect(paths(fake).slice(1).sort()).toEqual([
      'POST /v1/api/steps/lease-1/fail',
      'POST /v1/api/steps/lease-2/fail',
    ])
    for (const call of fake.calls.slice(1)) {
      const body = call.body as { error: { class: string; message: string } }
      expect(body.error.class).toBe('unservable')
      expect(body.error.message).toContain('w-1 does not serve')
    }
    await mcpd.shutdown()
  })

  it('without onError, writes one line per kind of failure per interval, whatever the step', async () => {
    // Three leases this worker cannot serve, each with its own step key, and a
    // daemon that refuses every fail. Every report used to be written, so a
    // failing run logged one line per step.
    const refused = { status: 400, body: { error: 'bad_request' } }
    const leases = [1, 2, 3].map((n) => wireLease({ lease_id: `lease-${n}`, step_key: `unknown_${n}` }))
    const fake = fakeFetch([claimed(...leases), refused, refused, refused])
    const mcpd = client(fake.fetch)
    const lines: string[] = []
    const spy = vi.spyOn(console, 'error').mockImplementation((message: unknown) => {
      lines.push(String(message))
    })
    try {
      const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(workflow({ id: 'support' }))
      await worker.claim(3)
      await worker.drain()
    } finally {
      spy.mockRestore()
    }
    expect(paths(fake).filter((p) => p.endsWith('/fail'))).toHaveLength(3)
    expect(lines.filter((l) => l.includes('call failed for'))).toHaveLength(1)
    await mcpd.shutdown()
  })
})

describe('the 409 table', () => {
  it('treats already_applied as done and says nothing', async () => {
    const fake = fakeFetch([
      claimed(wireLease()),
      started,
      conflict('already_applied', { attempt_status: 'succeeded' }),
    ])
    const errors: Errors = []
    const mcpd = client(fake.fetch, errors)
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0, onError: collect(errors) }).register(
      workflow({ id: 'support' }).step('classify', () => 1),
    )
    await worker.claim(1)
    await worker.drain()
    expect(paths(fake)).toHaveLength(3)
    expect(errors).toEqual([])
    await mcpd.shutdown()
  })

  it('never runs a body whose started was fenced', async () => {
    const fake = fakeFetch([claimed(wireLease()), conflict('fenced', { attempt_status: 'orphaned' })])
    const errors: Errors = []
    const mcpd = client(fake.fetch, errors)
    let ran = false
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(
      workflow({ id: 'support' }).step('classify', () => {
        ran = true
      }),
    )
    await worker.claim(1)
    await worker.drain()
    expect(ran).toBe(false)
    expect(paths(fake)).toEqual(['POST /v1/api/steps/claim', 'POST /v1/api/steps/lease-1/started'])
    expect(errors).toEqual([])
    await mcpd.shutdown()
  })

  it('abandons a lease whose run ended, and posts nothing more for it', async () => {
    const fake = fakeFetch([claimed(wireLease()), started, conflict('run_terminal', { run_status: 'cancelled' })])
    const errors: Errors = []
    const mcpd = client(fake.fetch, errors)
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0, onError: collect(errors) }).register(
      workflow({ id: 'support' }).step('classify', () => 1),
    )
    await worker.claim(1)
    await worker.drain()
    expect(paths(fake)).toHaveLength(3)
    expect(errors).toEqual([])
    await mcpd.shutdown()
  })

  it('logs not_started loudly, because it is a bug in the worker', async () => {
    const fake = fakeFetch([claimed(wireLease({ effect: 'at_most_once' })), started, conflict('not_started')])
    const errors: Errors = []
    const mcpd = client(fake.fetch)
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0, onError: collect(errors) }).register(
      workflow({ id: 'support' }).step('classify', () => 1),
    )
    await worker.claim(1)
    await worker.drain()
    expect(errors).toHaveLength(1)
    expect(errors[0]?.message).toContain('a bug in this worker')
    expect(errors[0]?.error).toBeInstanceOf(StepConflictError)
    await mcpd.shutdown()
  })

  it('aborts the body when a heartbeat is fenced, and posts no complete', async () => {
    const fake = fakeFetch([
      registered,
      claimed(wireLease()),
      started,
      conflict('fenced', { attempt_status: 'orphaned' }),
    ])
    const errors: Errors = []
    const mcpd = client(fake.fetch, errors)
    let reason = ''
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(
      workflow({ id: 'support' }).step('classify', async (ctx) => {
        await new Promise<void>((resolve) => {
          ctx.signal.addEventListener('abort', () => resolve(), { once: true })
        })
        reason = String((ctx.signal.reason as Error).message)
        return 'too late'
      }),
    )
    await worker.registerDefinitions()
    await worker.claim(1)
    await worker.drain()
    expect(reason).toContain('fenced')
    expect(paths(fake)).toEqual([
      'POST /v1/api/workers/register',
      'POST /v1/api/steps/claim',
      'POST /v1/api/steps/lease-1/started',
      'POST /v1/api/steps/lease-1/heartbeat',
    ])
    expect(errors).toEqual([])
    await mcpd.shutdown()
  }, 5_000)

  it('sends one heartbeat at a time while the daemon is slow to answer them', async () => {
    const base = fakeFetch([registered, claimed(wireLease()), started, acked])
    let open = 0
    let most = 0
    const slowBeats: FetchLike = async (url, init) => {
      if (!url.endsWith('/heartbeat')) return base.fetch(url, init)
      open++
      most = Math.max(most, open)
      await new Promise((resolve) => setTimeout(resolve, 2_500))
      open--
      return new Response(JSON.stringify({ expires_at: '2026-09-01T10:00:30Z' }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      })
    }
    const mcpd = client(slowBeats)
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(
      workflow({ id: 'support' }).step('classify', async () => {
        await new Promise((resolve) => setTimeout(resolve, 3_300))
        return 'done'
      }),
    )
    await worker.registerDefinitions()
    await worker.claim(1)
    await worker.drain()
    expect(most).toBe(1)
    await mcpd.shutdown()
  }, 10_000)

  it('keeps going when a heartbeat fails for any reason but a conflict', async () => {
    // A 400 is not retried by the transport; a 5xx would be, and would land.
    const fake = fakeFetch([registered, claimed(wireLease()), started, { status: 400, body: { error: 'bad_request' } }, acked])
    const errors: Errors = []
    const mcpd = client(fake.fetch)
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0, onError: collect(errors) }).register(
      workflow({ id: 'support' }).step('classify', async () => {
        await new Promise((resolve) => setTimeout(resolve, 1_150))
        return 'done'
      }),
    )
    await worker.registerDefinitions()
    await worker.claim(1)
    await worker.drain()
    expect(paths(fake).at(-1)).toBe('POST /v1/api/steps/lease-1/complete')
    expect(errors.some((e) => e.message.includes('heartbeat failed'))).toBe(true)
    await mcpd.shutdown()
  }, 5_000)
})

describe('the serve loop', () => {
  it('registers, claims until stopped, and resolves once stopped', async () => {
    const fake = fakeFetch([registered, idle])
    const errors: Errors = []
    const mcpd = client(fake.fetch, errors)
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0, concurrency: 2, onError: collect(errors) }).register(
      workflow({ id: 'support' }).step('classify', () => 1),
    )
    const serving = worker.serve()
    await new Promise((resolve) => setTimeout(resolve, 30))
    await worker.stop()
    await serving

    expect(paths(fake)[0]).toBe('POST /v1/api/workers/register')
    expect(fake.calls[0]?.body).toEqual({
      project: 'test',
      worker_id: 'w-1',
      definitions: [{ id: 'support', kind: 'workflow', body: { steps: [{ kind: 'task', id: 'classify' }] } }],
    })
    expect(paths(fake).slice(1).every((p) => p === 'POST /v1/api/steps/claim')).toBe(true)
    expect(fake.calls.length).toBeGreaterThan(2)
    expect(fake.calls[1]?.body).toEqual({ project: 'test', worker_id: 'w-1', max: 2, wait_ms: 0 })
    expect(errors).toEqual([])
    await mcpd.shutdown()
  })

  it('lets an idle claim outlive the client timeout by wait_ms', async () => {
    // The daemon holds an idle claim open for the whole of wait_ms, so the
    // answer arrives after the client's own timeout and within the wait. A
    // claim cut off by its own timeout is what made a healthy idle worker
    // report a failure every ten seconds.
    const aborted: unknown[] = []
    const delayed: FetchLike = (_url, init) =>
      new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          resolve(new Response(JSON.stringify({ leases: [] }), { status: 200, headers: { 'content-type': 'application/json' } }))
        }, 150)
        init.signal?.addEventListener('abort', () => {
          clearTimeout(timer)
          aborted.push(init.signal?.reason)
          reject(new Error('aborted'))
        })
      })
    const mcpd = new Mcpd({
      endpoint: 'http://daemon.test',
      project: 'test',
      fetch: delayed,
      timeoutMs: 50,
      flushIntervalMs: 10_000,
      flushOnExit: false,
    })
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 1_000, onError: () => {} })
    expect(await worker.claim(1)).toBe(0)
    expect(aborted).toEqual([])
    await mcpd.shutdown()
  })

  it('reports a claim that failed and tries again', async () => {
    // A 400 is not retried by the transport, so the worker sees the failure itself.
    const fake = fakeFetch([registered, { status: 400, body: { error: 'bad_request', message: 'no' } }, idle])
    const errors: Errors = []
    const mcpd = client(fake.fetch)
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0, onError: collect(errors) }).register(
      workflow({ id: 'support' }).step('classify', () => 1),
    )
    const serving = worker.serve()
    await new Promise((resolve) => setTimeout(resolve, 30))
    await worker.stop()
    await serving
    expect(errors[0]?.message).toContain('claim failed')
    await mcpd.shutdown()
  })
})

describe('the run api', () => {
  it('starts a run with the fields the daemon expects', async () => {
    const fake = fakeFetch([{ status: 200, body: { run_id: 'run-1', status: 'running' } }])
    const mcpd = client(fake.fetch)
    const run = await mcpd.runs.start({
      definitionId: 'support',
      version: 2,
      input: { ticket_id: 'T-1' },
      idempotencyKey: 'ticket:T-1',
      correlation: { sessionId: 's-1' },
    })
    expect(run.runId).toBe('run-1')
    expect(fake.calls[0]?.url).toBe('http://daemon.test/v1/api/runs')
    expect(fake.calls[0]?.body).toEqual({
      project: 'test',
      definition_id: 'support',
      version: 2,
      input: { ticket_id: 'T-1' },
      idempotency_key: 'ticket:T-1',
      correlation: { project: 'test', session_id: 's-1' },
    })
    await mcpd.shutdown()
  })

  it('stores a definition with its body untouched', async () => {
    const fake = fakeFetch([{ status: 200, body: { id: 'support', version: 1, assigned: true } }])
    const mcpd = client(fake.fetch)
    const doc = workflow({ id: 'support' })
      .step('classify', () => 1, { stepTimeoutMs: 100 })
      .document()
    const stored = await mcpd.definitions.put(doc)
    expect(stored).toEqual({ id: 'support', version: 1, assigned: true })
    expect(fake.calls[0]?.url).toBe('http://daemon.test/v1/api/definitions?project=test')
    expect(fake.calls[0]?.body).toEqual(doc)
    await mcpd.shutdown()
  })

  it('walks the event pages until the run ends, passing the last seq it saw', async () => {
    const fake = fakeFetch([
      {
        status: 200,
        body: {
          events: [
            { run_id: 'run-1', seq: 0, kind: 'run_started', payload: { definition_id: 'support' }, at: 't0' },
            { run_id: 'run-1', seq: 1, kind: 'step_leased', payload: { step_key: 'classify' }, at: 't1' },
          ],
          has_more: true,
        },
      },
      {
        status: 200,
        body: { events: [], has_more: false },
      },
      {
        status: 200,
        body: {
          events: [{ run_id: 'run-1', seq: 2, kind: 'run_ended', payload: { status: 'succeeded' }, at: 't2' }],
          has_more: false,
        },
      },
    ])
    const mcpd = client(fake.fetch)
    const kinds: string[] = []
    for await (const event of mcpd.runs.watch('run-1', { pollMs: 1 })) kinds.push(event.kind)
    expect(kinds).toEqual(['run_started', 'step_leased', 'run_ended'])
    expect(fake.calls.map((c) => c.url)).toEqual([
      'http://daemon.test/v1/api/runs/run-1/events?after=-1&limit=500',
      'http://daemon.test/v1/api/runs/run-1/events?after=1&limit=500',
      'http://daemon.test/v1/api/runs/run-1/events?after=1&limit=500',
    ])
    // The payload is opaque: the daemon's own spelling reaches the reader.
    await mcpd.shutdown()
  })

  it('publishes and signals with the ids the caller supplied', async () => {
    const receipt = { status: 200, body: { outcome: 'delivered', run_id: 'run-1' } }
    const fake = fakeFetch([receipt, receipt])
    const mcpd = client(fake.fetch)
    await mcpd.events.publish({ event: 'approved', key: 'T-1', payload: { by: 'ops' }, eventId: 'evt-1' })
    await mcpd.runs.signal('run-1', { event: 'approved', payload: { by: 'ops' }, eventId: 'evt-2' })
    expect(fake.calls[0]?.url).toBe('http://daemon.test/v1/api/events')
    expect(fake.calls[0]?.body).toEqual({
      project: 'test',
      event: 'approved',
      key: 'T-1',
      payload: { by: 'ops' },
      event_id: 'evt-1',
    })
    expect(fake.calls[1]?.url).toBe('http://daemon.test/v1/api/runs/run-1/signal')
    expect(fake.calls[1]?.body).toEqual({ event: 'approved', payload: { by: 'ops' }, event_id: 'evt-2' })
    await mcpd.shutdown()
  })

  it('raises a 409 from a worker call as a typed conflict, and any other 409 as itself', async () => {
    const fake = fakeFetch([
      conflict('fenced', { attempt_status: 'orphaned' }),
      { status: 409, body: { error: 'version_taken', message: 'support@1 exists with a different body' } },
    ])
    const mcpd = client(fake.fetch)
    const err = await mcpd.steps.complete('lease-1', { output: 1 }).catch((e: unknown) => e)
    expect(err).toBeInstanceOf(StepConflictError)
    const typed = err as StepConflictError
    expect(typed.conflict.reason).toBe('fenced')
    expect(typed.conflict.attemptStatus).toBe('orphaned')
    expect(typed.status).toBe(409)
    expect(typed.retryable).toBe(false)

    const plain = await mcpd.definitions.put({ id: 'support', kind: 'workflow', version: 1, body: {} }).catch((e: unknown) => e)
    expect(plain).toBeInstanceOf(McpdError)
    expect(plain).not.toBeInstanceOf(StepConflictError)
    expect((plain as McpdError).body).toContain('version_taken')
    await mcpd.shutdown()
  })
})

describe('the step deadline', () => {
  it('cancels a body that runs past the lease deadline and fails the attempt retryable', async () => {
    const fake = fakeFetch([claimed(wireLease({ deadline_ms: 150 })), started, acked])
    const errors: Errors = []
    const mcpd = client(fake.fetch, errors)
    let reason = ''
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0, onError: collect(errors) }).register(
      workflow({ id: 'support' }).step('classify', async (ctx) => {
        await new Promise<void>((resolve) => {
          ctx.signal.addEventListener('abort', () => resolve(), { once: true })
        })
        reason = String((ctx.signal.reason as Error).message)
        return 'too late'
      }),
    )
    await worker.claim(1)
    await worker.drain()
    expect(reason).toBe('step deadline of 150ms passed')
    expect(paths(fake)).toEqual([
      'POST /v1/api/steps/claim',
      'POST /v1/api/steps/lease-1/started',
      'POST /v1/api/steps/lease-1/fail',
    ])
    expect(fake.calls[2]?.body).toEqual({
      error: { class: 'retryable', message: 'step deadline of 150ms passed' },
    })
    expect(errors).toEqual([])
    await mcpd.shutdown()
  })

  it('frees the slot at the deadline even when the body ignores its signal', async () => {
    const fake = fakeFetch([claimed(wireLease({ deadline_ms: 100 })), started, acked])
    const mcpd = client(fake.fetch)
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(
      // A body that never looks at ctx.signal and never returns.
      workflow({ id: 'support' }).step('classify', () => new Promise<string>(() => {})),
    )
    await worker.claim(1)
    const began = Date.now()
    await worker.drain()
    expect(Date.now() - began).toBeLessThan(1_000)
    expect(worker.stats().inflight).toBe(0)
    expect(paths(fake).at(-1)).toBe('POST /v1/api/steps/lease-1/fail')
    await mcpd.shutdown()
  }, 5_000)

  it('leaves a body that finishes in time alone', async () => {
    const fake = fakeFetch([claimed(wireLease({ deadline_ms: 5_000 })), started, acked])
    const mcpd = client(fake.fetch)
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(
      workflow({ id: 'support' }).step('classify', () => 'quick'),
    )
    await worker.claim(1)
    await worker.drain()
    expect(paths(fake).at(-1)).toBe('POST /v1/api/steps/lease-1/complete')
    await mcpd.shutdown()
  })
})

describe('typed decoding', () => {
  it('fails a step invalid_input when the decoder rejects the input, prior or state', async () => {
    const fake = fakeFetch([claimed(wireLease()), started, acked])
    const mcpd = client(fake.fetch)
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(
      workflow({ id: 'support' }).step('classify', (ctx) => {
        const prior = ctx.prior((v) => v as Record<string, unknown>)
        expect(Object.keys(prior)).toEqual(['triage'])
        const state = ctx.stateAs((v) => v as { seen_count: number })
        expect(state.seen_count).toBe(1)
        return ctx.input((v) => {
          const o = v as { ticket_id?: unknown }
          if (typeof o.ticket_id !== 'number') throw new Error('ticket_id must be a number')
          return o
        })
      }),
    )
    await worker.claim(1)
    await worker.drain()
    expect(paths(fake)[2]).toBe('POST /v1/api/steps/lease-1/fail')
    expect(fake.calls[2]?.body).toEqual({
      error: { class: 'invalid_input', message: "the step's input could not be read: ticket_id must be a number" },
    })
    await mcpd.shutdown()
  })
})

describe('waiting for a run', () => {
  const runBody = (status: string, extra: Record<string, unknown> = {}) => ({
    run: {
      run_id: 'run-1', project: 'test', definition_id: 'support', definition_version: 1, status,
      cursor: { index: 2, construct: 'end' }, correlation: { project: 'test' }, trace_id: 'a1'.repeat(16),
      steps_used: 2, cost_usd: 0.25, root_run_id: 'run-1', depth: 0,
      created_at: '2026-09-01T10:00:00Z', updated_at: '2026-09-01T10:00:05Z', ...extra,
    },
    attempts: [], timers: [], waits: [],
  })
  const ended = {
    status: 200,
    body: { events: [{ run_id: 'run-1', seq: 0, kind: 'run_ended', payload: { status: 'succeeded' }, at: 't' }], has_more: false },
  }

  it('returns the output once the run ends', async () => {
    const fake = fakeFetch([ended, { status: 200, body: runBody('succeeded', { output: { answer: 42 } }) }])
    const mcpd = client(fake.fetch)
    const result = await mcpd.runs.wait('run-1', { pollMs: 1 })
    expect(result.status).toBe('succeeded')
    expect(result.output).toEqual({ answer: 42 })
    expect(result.costUsd).toBe(0.25)
    expect(result.traceId).toBe('a1'.repeat(16))
    await mcpd.shutdown()
  })

  it('throws a typed error carrying the run when it did not succeed', async () => {
    const fake = fakeFetch([ended, { status: 200, body: runBody('failed', { error: 'send: relay down' }) }])
    const mcpd = client(fake.fetch)
    const err = await mcpd.runs.wait('run-1', { pollMs: 1 }).catch((e: unknown) => e)
    expect(err).toBeInstanceOf(RunFailedError)
    expect((err as RunFailedError).runStatus).toBe('failed')
    expect((err as RunFailedError).run.error).toBe('send: relay down')
    expect((err as Error).message).toBe('run run-1 failed: send: relay down')
    await mcpd.shutdown()
  })

  it('gives up after the timeout while the run is still going', async () => {
    const fake = fakeFetch([{ status: 200, body: { events: [], has_more: false } }])
    const mcpd = client(fake.fetch)
    const err = await mcpd.runs.wait('run-1', { pollMs: 5, timeoutMs: 40 }).catch((e: unknown) => e)
    expect(err).toBeInstanceOf(McpdError)
    expect((err as Error).message).toContain('still running after 40ms')
    await mcpd.shutdown()
  })

  it('starts and waits in one call', async () => {
    const fake = fakeFetch([
      { status: 200, body: runBody('running').run },
      ended,
      { status: 200, body: runBody('succeeded', { output: 'done' }) },
    ])
    const mcpd = client(fake.fetch)
    const result = await mcpd.runs.startAndWait({ definitionId: 'support', input: { a: 1 }, pollMs: 1 })
    expect(result.output).toBe('done')
    expect(fake.calls[0]?.body).toEqual({ project: 'test', definition_id: 'support', input: { a: 1 } })
    await mcpd.shutdown()
  })
})

describe('usage accumulation', () => {
  it('sums the usage of every span under the context, and the last model seen wins', async () => {
    const fake = fakeFetch([claimed(wireLease()), started, acked])
    const errors: Errors = []
    const mcpd = client(fake.fetch, errors)
    const wf = workflow({ id: 'support' }).step('classify', async (ctx) => {
      await ctx.withSpan('first', async (span) => {
        span.setModel('claude-sonnet-4', 'anthropic').setUsage({ inputTokens: 100, outputTokens: 10 })
        // Nested through the ambient parent, not through the context.
        await mcpd.withSpan('nested', async (inner) => {
          inner.setModel('claude-haiku-4', 'anthropic')
          inner.setUsage({ inputTokens: 20, outputTokens: 5, totalTokens: 25 })
        })
      })
      // A container that names a model and spends nothing changes nothing.
      await ctx.withSpan('container', async (span) => {
        span.setModel('gpt-5', 'openai')
      })
      return 'ok'
    })
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(wf)
    await worker.claim(1)
    await worker.drain()

    expect(fake.calls[2]?.body).toEqual({
      output: 'ok',
      // The nested span ends first; the outer one ends last, so its model wins.
      usage: {
        input_tokens: 120,
        output_tokens: 15,
        total_tokens: 25,
        model: 'claude-sonnet-4',
        provider: 'anthropic',
      },
    })
    expect(errors).toEqual([])
    await mcpd.shutdown()
  })

  it('setUsage is an explicit override of what the spans accumulated', async () => {
    const fake = fakeFetch([claimed(wireLease()), started, acked])
    const mcpd = client(fake.fetch)
    const wf = workflow({ id: 'support' }).step('classify', async (ctx) => {
      await ctx.withSpan('model_call', async (span) => {
        span.setModel('claude-sonnet-4', 'anthropic').setUsage({ inputTokens: 100, outputTokens: 10 })
      })
      ctx.setUsage({ inputTokens: 7, model: 'billed-as' })
      return 'ok'
    })
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(wf)
    await worker.claim(1)
    await worker.drain()

    expect((fake.calls[2]?.body as { usage: unknown }).usage).toEqual({
      input_tokens: 7,
      output_tokens: 10,
      model: 'billed-as',
      provider: 'anthropic',
    })
    await mcpd.shutdown()
  })
})

describe('hooks and stats', () => {
  it('calls each hook once with what the contract says', async () => {
    const fake = fakeFetch([claimed(wireLease()), started, conflict('fenced')])
    const mcpd = client(fake.fetch)
    const seen: string[] = []
    const wf = workflow({ id: 'support' }).step('classify', () => 'ok')
    const worker = new Worker(mcpd, {
      workerId: 'w-1',
      waitMs: 0,
      onLease: (lease) => seen.push(`lease ${lease.leaseId}`),
      onStepStart: (ctx) => seen.push(`start ${ctx.stepKey}`),
      onStepEnd: (ctx, outcome) => seen.push(`end ${ctx.stepKey} ${outcome.status}`),
      onConflict: (lease, reason) => seen.push(`conflict ${lease.leaseId} ${reason}`),
    }).register(wf)
    await worker.claim(1)
    await worker.drain()

    expect(seen).toEqual([
      'lease lease-1',
      'start classify',
      'end classify completed',
      'conflict lease-1 fenced',
    ])
    expect(worker.stats()).toEqual({
      claimed: 1,
      inflight: 0,
      completed: 1,
      failed: {},
      conflicts: { fenced: 1 },
      unservable: 0,
    })
    await mcpd.shutdown()
  })

  it('a hook that throws or rejects is reported, and the lease still completes', async () => {
    const fake = fakeFetch([claimed(wireLease()), started, acked])
    const errors: Errors = []
    const mcpd = client(fake.fetch, errors)
    const wf = workflow({ id: 'support' }).step('classify', () => 'ok')
    const worker = new Worker(mcpd, {
      workerId: 'w-1',
      waitMs: 0,
      onError: collect(errors),
      onLease: () => {
        throw new Error('hook bug')
      },
      onStepEnd: async () => Promise.reject(new Error('later bug')),
    }).register(wf)
    await worker.claim(1)
    await worker.drain()
    await new Promise((resolve) => setTimeout(resolve, 0))

    expect(paths(fake)).toEqual([
      'POST /v1/api/steps/claim',
      'POST /v1/api/steps/lease-1/started',
      'POST /v1/api/steps/lease-1/complete',
    ])
    expect(errors.map((e) => e.message)).toEqual([
      'mcpd worker w-1: the onLease hook threw',
      'mcpd worker w-1: the onStepEnd hook rejected',
    ])
    expect(worker.stats().completed).toBe(1)
    await mcpd.shutdown()
  })

  it('counts a completed step, a fatal failure, a fenced conflict and an unservable lease', async () => {
    const fake = fakeFetch([
      claimed(wireLease()),
      started,
      acked,
      claimed(wireLease({ lease_id: 'lease-2', step_key: 'boom' })),
      started,
      acked,
      claimed(wireLease({ lease_id: 'lease-3' })),
      started,
      conflict('fenced'),
      claimed(wireLease({ lease_id: 'lease-4', definition_id: 'other' })),
      acked,
    ])
    const mcpd = client(fake.fetch)
    const wf = workflow({ id: 'support' })
      .step('classify', () => 'ok')
      .step('boom', () => {
        throw new Error('bad')
      })
    const worker = new Worker(mcpd, { workerId: 'w-1', waitMs: 0 }).register(wf)
    for (let i = 0; i < 4; i++) {
      await worker.claim(1)
      await worker.drain()
    }

    expect(worker.stats()).toEqual({
      claimed: 4,
      inflight: 0,
      completed: 2,
      failed: { fatal: 1 },
      conflicts: { fenced: 1 },
      unservable: 1,
    })
    await mcpd.shutdown()
  })
})
