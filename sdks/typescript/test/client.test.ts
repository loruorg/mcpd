import { afterEach, describe, expect, it, vi } from 'vitest'
import { Mcpd, McpdSpan, isSpanId, isTraceId, parseTraceparent } from '../src/index.js'
import type { CostQuery, FeedbackQuery, FetchLike, MetricQuery, WindowQuery } from '../src/index.js'
import { deadFetch, fakeFetch } from './helpers.js'

afterEach(() => {
  vi.unstubAllEnvs()
})

function client(fetchImpl = fakeFetch().fetch, options = {}) {
  return new Mcpd({
    endpoint: 'http://daemon.test',
    project: 'test',
    fetch: fetchImpl,
    flushIntervalMs: 10_000,
    flushOnExit: false,
    onError: () => {},
    ...options,
  })
}

describe('construction', () => {
  it('works with no arguments at all', () => {
    const mcpd = new Mcpd({ flushOnExit: false })
    expect(mcpd.endpoint).toBe('http://localhost:4317')
    expect(mcpd.project).toBe('default')
    expect(mcpd.enabled).toBe(true)
  })

  it('reads the environment, and lets code win over it', () => {
    vi.stubEnv('MCPD_ENDPOINT', 'http://from-env:4317')
    vi.stubEnv('MCPD_PROJECT', 'from-env')
    vi.stubEnv('MCPD_SAMPLE', '0.5')

    expect(new Mcpd({ flushOnExit: false }).endpoint).toBe('http://from-env:4317')
    expect(new Mcpd({ flushOnExit: false }).project).toBe('from-env')
    expect(new Mcpd({ project: 'from-code', flushOnExit: false }).project).toBe('from-code')
  })

  it('honours MCPD_ENABLED=false', () => {
    vi.stubEnv('MCPD_ENABLED', 'false')
    expect(new Mcpd({ flushOnExit: false }).enabled).toBe(false)
  })

  // The four tuning variables have no accessor, so each is proved by the
  // behaviour it controls rather than by reading it back.
  it('reads MCPD_BATCH_SIZE, and code wins over it', async () => {
    vi.stubEnv('MCPD_BATCH_SIZE', '2')
    const fromEnv = fakeFetch()
    const mcpd = client(fromEnv.fetch)
    for (let i = 0; i < 4; i++) mcpd.trace(`run-${i}`).end()
    await mcpd.flush()
    expect(fromEnv.calls).toHaveLength(2)
    expect(fromEnv.calls.every((c) => (c.body as { spans: unknown[] }).spans.length === 2)).toBe(true)
    await mcpd.shutdown()

    const fromCode = fakeFetch()
    const override = client(fromCode.fetch, { batchSize: 4 })
    for (let i = 0; i < 4; i++) override.trace(`run-${i}`).end()
    await override.flush()
    expect(fromCode.calls).toHaveLength(1)
    await override.shutdown()
  })

  it('reads MCPD_MAX_QUEUE, and code wins over it', async () => {
    vi.stubEnv('MCPD_MAX_QUEUE', '3')
    const mcpd = client(deadFetch)
    for (let i = 0; i < 10; i++) mcpd.trace(`run-${i}`).end()
    expect(mcpd.stats().queued).toBeLessThanOrEqual(3)
    expect(mcpd.stats().dropped).toBeGreaterThanOrEqual(7)
    await mcpd.shutdown()

    const override = client(deadFetch, { maxQueue: 20 })
    for (let i = 0; i < 10; i++) override.trace(`run-${i}`).end()
    expect(override.stats().queued).toBe(10)
    expect(override.stats().dropped).toBe(0)
    await override.shutdown()
  })

  it('reads MCPD_FLUSH_INTERVAL_MS as whole milliseconds, and code wins over it', async () => {
    vi.stubEnv('MCPD_FLUSH_INTERVAL_MS', '20')
    const fromEnv = fakeFetch()
    const mcpd = new Mcpd({ endpoint: 'http://daemon.test', fetch: fromEnv.fetch, flushOnExit: false })
    mcpd.trace('run').end()
    await new Promise((resolve) => setTimeout(resolve, 300))
    // No flush was called: the timer sent it, and the timer came from the
    // environment.
    expect(fromEnv.calls.length).toBeGreaterThanOrEqual(1)
    await mcpd.shutdown()

    const fromCode = fakeFetch()
    const override = new Mcpd({
      endpoint: 'http://daemon.test',
      fetch: fromCode.fetch,
      flushOnExit: false,
      flushIntervalMs: 60_000,
    })
    override.trace('run').end()
    await new Promise((resolve) => setTimeout(resolve, 300))
    expect(fromCode.calls).toHaveLength(0)
    await override.shutdown()
  })

  it('reads MCPD_TIMEOUT_MS as whole milliseconds, and code wins over it', async () => {
    // A fetch that never answers on its own: only the timeout signal ends it,
    // so how long it takes to be aborted is the timeout the client applied.
    const abortedAfter: number[] = []
    const hanging: FetchLike = (_url, init) =>
      new Promise((_resolve, reject) => {
        const started = Date.now()
        init.signal?.addEventListener('abort', () => {
          abortedAfter.push(Date.now() - started)
          reject(new Error('aborted'))
        })
      })

    vi.stubEnv('MCPD_TIMEOUT_MS', '50')
    const mcpd = client(hanging)
    mcpd.trace('run').end()
    await mcpd.flush()
    expect(abortedAfter.length).toBeGreaterThanOrEqual(1)
    expect(abortedAfter[0]).toBeLessThan(1_000)
    await mcpd.shutdown()

    abortedAfter.length = 0
    const override = client(hanging, { timeoutMs: 400 })
    override.trace('run').end()
    await override.flush()
    expect(abortedAfter.length).toBeGreaterThanOrEqual(1)
    expect(abortedAfter[0]).toBeGreaterThanOrEqual(350)
    await override.shutdown()
  }, 20_000)

  it('ignores a tuning variable that is not a whole positive number', async () => {
    vi.stubEnv('MCPD_BATCH_SIZE', 'lots')
    vi.stubEnv('MCPD_MAX_QUEUE', '0')
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)
    for (let i = 0; i < 5; i++) mcpd.trace(`run-${i}`).end()
    expect(mcpd.stats().queued).toBe(5)
    await mcpd.flush()
    expect(fake.calls).toHaveLength(1)
    await mcpd.shutdown()
  })
})

describe('tracing', () => {
  it('gives every span well-formed ids and one shared trace id (B6)', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)

    const run = mcpd.trace('support_agent')
    const child = run.child('call_model', { kind: 'model_generation' })
    child.end()
    run.end()
    await mcpd.shutdown()

    expect(isTraceId(run.traceId)).toBe(true)
    expect(isSpanId(run.spanId)).toBe(true)
    expect(child.traceId).toBe(run.traceId)
    expect(child.parentSpanId).toBe(run.spanId)
    expect(run.parentSpanId).toBeUndefined()

    const sent = fake.spans()
    expect(sent).toHaveLength(2)
    expect(sent.every((s) => s['trace_id'] === run.traceId)).toBe(true)
  })

  it('sends the model call with its tokens and its payloads', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)

    await mcpd.withSpan(
      'generate',
      async (span) => {
        span.setModel('claude-sonnet-4-6', 'anthropic')
        span.setInput([{ role: 'user', content: 'where is my order?' }])
        span.setOutput('It ships tomorrow.')
        span.setUsage({ inputTokens: 812, outputTokens: 64 })
        span.setModelParameters({
          temperature: 0.2,
          topP: 0.95,
          topK: 40,
          maxTokens: 1024,
          seed: 7,
          stopSequences: ['\n\nHuman:'],
          hasResponseFormat: true,
        })
      },
      { kind: 'model_generation' },
    )
    await mcpd.shutdown()

    const span = fake.spans()[0] as Record<string, unknown>
    expect(span['kind']).toBe('model_generation')
    expect(span['model']).toBe('claude-sonnet-4-6')
    expect(span['provider']).toBe('anthropic')
    expect(span['usage']).toEqual({ input_tokens: 812, output_tokens: 64 })
    expect(span['output']).toBe('It ships tomorrow.')
    expect(span['status']).toBe('ok')
    expect(span['end_time']).toBeTypeOf('string')
    expect(span['model_parameters']).toEqual({
      temperature: 0.2,
      top_p: 0.95,
      top_k: 40,
      max_tokens: 1024,
      seed: 7,
      stop_sequences: ['\n\nHuman:'],
      has_response_format: true,
    })
  })

  it('keeps a zero seed and a zero temperature, and defaults the response format flag', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)

    // The classic bug this guards: reading 0 as "not given". A zero seed is
    // the seed a reproducible run is pinned to, and a zero temperature is the
    // setting most worth recording, so neither may be dropped as falsy.
    mcpd.trace('greedy').setModelParameters({ temperature: 0, seed: 0, topP: 0 }).end()
    await mcpd.shutdown()

    expect(fake.spans()[0]!['model_parameters']).toEqual({
      temperature: 0,
      top_p: 0,
      seed: 0,
      has_response_format: false,
    })
  })

  it('replaces model parameters, so the record describes one call', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)

    // Half of one request and half of another would describe a call nobody
    // made, so the second call wins outright.
    const span = mcpd.trace('generate')
    span.setModelParameters({ temperature: 0.1, seed: 99 })
    span.setModelParameters({ maxTokens: 256 })
    span.end()
    await mcpd.shutdown()

    expect(fake.spans()[0]!['model_parameters']).toEqual({
      max_tokens: 256,
      has_response_format: false,
    })
  })

  it('nests spans opened inside a withSpan callback', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)

    let outerId = ''
    let innerParent: string | undefined
    await mcpd.withSpan('outer', async (outer) => {
      outerId = outer.spanId
      await mcpd.withSpan('inner', async (inner) => {
        innerParent = inner.parentSpanId
      })
    })
    await mcpd.shutdown()

    expect(innerParent).toBe(outerId)
  })

  it('records what a callback threw and rethrows it unchanged (B10)', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)
    const boom = new RangeError('the tool returned nothing')

    await expect(
      mcpd.withSpan('call_tool', async () => {
        throw boom
      }),
    ).rejects.toBe(boom)
    await mcpd.shutdown()

    const span = fake.spans()[0] as Record<string, unknown>
    expect(span['status']).toBe('error')
    expect(span['error']).toMatchObject({
      message: 'the tool returned nothing',
      kind: 'RangeError',
    })
    expect((span['error'] as { stack?: string }).stack).toContain('RangeError')
  })

  it('sets status to error whenever an error is recorded (B10)', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)
    const span = mcpd.trace('run')
    span.setError('rate limited', { kind: 'RateLimitError', retryable: true })
    span.end()
    await mcpd.shutdown()

    const sent = fake.spans()[0] as Record<string, unknown>
    expect(sent['status']).toBe('error')
    expect(sent['error']).toEqual({
      message: 'rate limited',
      kind: 'RateLimitError',
      retryable: true,
    })
  })

  it('refuses a blank error message rather than sending nothing actionable (B10)', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)
    const span = mcpd.trace('run')
    span.setError('', { kind: 'Timeout' })
    span.end()
    await mcpd.shutdown()

    const sent = fake.spans()[0] as Record<string, unknown>
    expect(sent['status']).toBe('error')
    const error = sent['error'] as { message: string; kind: string }
    expect(error.kind).toBe('Timeout')
    expect(error.message.length).toBeGreaterThan(0)
    expect(error.message).toBe('no message given')
  })

  it('ends once, however many times end is called', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)
    const span = mcpd.trace('run')
    span.end()
    const first = span.snapshot().endTime
    span.end('error')
    expect(span.snapshot().endTime).toBe(first)
    expect(span.snapshot().status).toBe('ok')
    await mcpd.shutdown()
  })

  it('records an event as a span with no end', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)
    const run = mcpd.trace('run')
    run.event('first_chunk', { kind: 'model_chunk' })
    run.end()
    await mcpd.shutdown()

    const event = fake.spans().find((s) => s['name'] === 'first_chunk') as Record<string, unknown>
    expect(event['is_event']).toBe(true)
    expect(event['end_time']).toBeUndefined()
    expect(event['parent_span_id']).toBe(run.spanId)
  })
})

describe('trace context (B7)', () => {
  it('continues a trace that started in another service', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)
    const inbound = '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01'

    const run = mcpd.trace('handle_request', { traceparent: inbound })
    run.end()
    await mcpd.shutdown()

    expect(run.traceId).toBe('4bf92f3577b34da6a3ce929d0e0e4736')
    expect(run.parentSpanId).toBe('00f067aa0ba902b7')
    expect(run.sampled).toBe(true)
  })

  it('emits a header a downstream service can parse', async () => {
    const mcpd = client()
    const run = mcpd.trace('call_service')
    const parsed = parseTraceparent(run.traceparent())
    expect(parsed).toEqual({ traceId: run.traceId, spanId: run.spanId, sampled: true })
    await mcpd.shutdown()
  })

  it('starts a fresh trace when the inbound header is rubbish', async () => {
    const mcpd = client()
    const run = mcpd.trace('handle_request', { traceparent: 'garbage' })
    expect(isTraceId(run.traceId)).toBe(true)
    expect(run.parentSpanId).toBeUndefined()
    await mcpd.shutdown()
  })
})

describe('sampling is decided once at the root (B4)', () => {
  it('drops the whole trace when the root is not sampled', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch, { sample: 0 })

    const run = mcpd.trace('run')
    const child = run.child('step')
    expect(run.sampled).toBe(false)
    expect(child.sampled).toBe(false)
    child.end()
    run.end()
    await mcpd.shutdown()

    expect(fake.calls).toHaveLength(0)
  })

  it('keeps the whole trace when the root is sampled', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch, { sample: 1 })
    const run = mcpd.trace('run')
    run.child('step').end()
    run.end()
    await mcpd.shutdown()
    expect(fake.spans()).toHaveLength(2)
  })

  it('inherits the upstream decision instead of making its own', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch, { sample: 1 })
    const run = mcpd.trace('run', {
      traceparent: `00-${'a'.repeat(32)}-${'b'.repeat(16)}-00`,
    })
    expect(run.sampled).toBe(false)
    run.end()
    await mcpd.shutdown()
    expect(fake.calls).toHaveLength(0)
  })
})

describe('the other four signals', () => {
  it('correlates a log to the active span', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)

    await mcpd.withSpan('run', async (span) => {
      mcpd.log('warn', 'retrying the tool', { attempt: 2 })
      expect(span.spanId).toBeTruthy()
    })
    await mcpd.shutdown()

    const log = (fake.calls[0]?.body as { logs?: Array<Record<string, unknown>> }).logs?.[0]
    expect(log).toMatchObject({ level: 'warn', message: 'retrying the tool' })
    expect(log?.['fields']).toEqual({ attempt: 2 })
    expect(log?.['span_id']).toBeTypeOf('string')
  })

  it('sends a metric with its labels and unit', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)
    mcpd.metric('support.queue_depth', 12, 'gauge', { unit: '{item}', labels: { team: 'billing' } })
    await mcpd.shutdown()

    const metric = (fake.calls[0]?.body as { metrics?: Array<Record<string, unknown>> })
      .metrics?.[0]
    expect(metric).toMatchObject({ name: 'support.queue_depth', kind: 'gauge', value: 12 })
    expect(metric?.['labels']).toEqual({ team: 'billing' })
  })

  it('carries the declared scale on every score', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)
    const run = mcpd.trace('run')
    run.end()
    mcpd.score({
      traceId: run.traceId,
      scorerId: 'answer_relevance',
      value: 0.82,
      min: 0,
      max: 1,
      reason: 'answered the question but missed the delivery date',
    })
    await mcpd.shutdown()

    const score = (fake.calls[0]?.body as { scores?: Array<Record<string, unknown>> }).scores?.[0]
    expect(score).toMatchObject({
      scorer_id: 'answer_relevance',
      value: 0.82,
      min: 0,
      max: 1,
      source: 'live',
    })
  })

  it('keeps human feedback separate from a machine score', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)
    mcpd.feedback({ traceId: 'a'.repeat(32), kind: 'thumbs', value: 'up', comment: 'exactly right' })
    await mcpd.shutdown()

    const body = fake.calls[0]?.body as { feedback?: Array<Record<string, unknown>>; scores?: unknown }
    expect(body.feedback?.[0]).toMatchObject({ kind: 'thumbs', value: 'up' })
    expect(body.scores).toBeUndefined()
  })
})

describe('batching and flushing (B2, B5)', () => {
  it('sends in batches of the configured size', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch, { batchSize: 4 })

    for (let i = 0; i < 12; i++) {
      const span = mcpd.trace(`run-${i}`)
      span.end()
    }
    await mcpd.flush()

    expect(fake.spans()).toHaveLength(12)
    expect(fake.calls.length).toBeGreaterThanOrEqual(3)
    for (const call of fake.calls) {
      const spans = (call.body as { spans?: unknown[] }).spans ?? []
      expect(spans.length).toBeLessThanOrEqual(4)
    }
  })

  it('returns from a recording call without waiting for the network', async () => {
    let release = (): void => {}
    const blocked = new Promise<void>((resolve) => {
      release = resolve
    })
    const mcpd = client(async () => {
      await blocked
      return new Response('{}', { status: 200, headers: { 'content-type': 'application/json' } })
    }, { batchSize: 1 })

    const started = Date.now()
    for (let i = 0; i < 100; i++) mcpd.trace(`run-${i}`).end()
    expect(Date.now() - started).toBeLessThan(200)
    release()
    await mcpd.shutdown()
  })

  it('flushes and shuts down idempotently', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)
    mcpd.trace('run').end()

    await Promise.all([mcpd.flush(), mcpd.flush()])
    const afterFlush = fake.calls.length
    await mcpd.shutdown()
    await mcpd.shutdown()
    await mcpd.shutdown()

    expect(afterFlush).toBe(1)
    expect(fake.calls).toHaveLength(1)
  })

  it('stops sending after shutdown', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)
    await mcpd.shutdown()
    mcpd.trace('too late').end()
    await mcpd.flush()
    expect(fake.calls).toHaveLength(0)
  })

  it('returns from flush and shutdown by their deadline when the daemon never answers', async () => {
    // Each export hangs for longer than the deadlines below. Against a
    // black-holed daemon every batch costs three attempts of the full request
    // timeout, so a deadline that counts batches instead of time is no bound.
    let exports = 0
    const sink = {
      export: async (): Promise<void> => {
        exports++
        await new Promise((resolve) => setTimeout(resolve, 400))
      },
    }
    const mcpd = client(fakeFetch().fetch, { sink, batchSize: 1 })
    for (let i = 0; i < 5; i++) mcpd.metric('calls', i)

    const started = Date.now()
    expect(await mcpd.flush({ timeoutMs: 50 })).toBe(false)
    await mcpd.shutdown({ timeoutMs: 50 })
    expect(Date.now() - started).toBeLessThan(300)

    // The batch in flight finishes on its own; no further batch starts.
    await new Promise((resolve) => setTimeout(resolve, 1_000))
    expect(exports).toBe(1)
    expect(mcpd.stats().queued).toBe(4)
  })
})

describe('a dead server never raises (B1)', () => {
  it('swallows every export failure', async () => {
    const errors: string[] = []
    const mcpd = client(deadFetch, { onError: (message: string) => errors.push(message) })

    await expect(
      (async () => {
        const run = mcpd.trace('run')
        run.setInput('hello').setOutput('world').setUsage({ inputTokens: 1 })
        run.child('step').end()
        mcpd.log('info', 'still fine')
        mcpd.metric('m', 1, 'counter')
        mcpd.score({ scorerId: 's', value: 1, min: 0, max: 1 })
        mcpd.feedback({ kind: 'thumbs', value: 'up' })
        run.end()
        await mcpd.flush()
        await mcpd.shutdown()
      })(),
    ).resolves.toBeUndefined()

    expect(errors.length).toBeGreaterThan(0)
    expect(errors.some((e) => e.includes('export'))).toBe(true)
  })

  it('does not throw when a 200 on ingest carries a body that is not JSON', async () => {
    // A proxy in front of the daemon that answers HTML. The batch landed, or
    // it did not; either way the agent must not hear about it as an exception.
    const errors: string[] = []
    let calls = 0
    const proxy: FetchLike = async () => {
      calls++
      return new Response('<html>fronted by a proxy</html>', {
        status: 200,
        headers: { 'content-type': 'text/html' },
      })
    }
    const mcpd = client(proxy, { onError: (message: string) => errors.push(message) })
    mcpd.trace('run').end()
    // `true` says the flush finished in time, not that the batch landed.
    await expect(mcpd.flush()).resolves.toBe(true)
    await expect(mcpd.shutdown()).resolves.toBeUndefined()
    // Once: a 200 is not retried, whatever the body says.
    expect(calls).toBe(1)
    expect(errors.length).toBeGreaterThan(0)
  })

  it('does not raise against a port with nothing listening', async () => {
    const mcpd = new Mcpd({
      endpoint: 'http://127.0.0.1:1',
      project: 'test',
      timeoutMs: 200,
      flushOnExit: false,
      onError: () => {},
    })
    mcpd.trace('run').end()
    await expect(mcpd.shutdown()).resolves.toBeUndefined()
  }, 15_000)

  it('keeps the queue bounded while the server is down (B3)', async () => {
    const mcpd = client(deadFetch, { maxQueue: 8, batchSize: 128, onError: () => {} })
    for (let i = 0; i < 500; i++) mcpd.trace(`run-${i}`).end()

    const stats = mcpd.stats()
    expect(stats.queued).toBeLessThanOrEqual(8)
    expect(stats.dropped).toBeGreaterThan(0)
    await mcpd.shutdown()
  })

  it('survives a payload that cannot be serialised', async () => {
    const errors: string[] = []
    const fake = fakeFetch()
    const mcpd = client(fake.fetch, { onError: (m: string) => errors.push(m) })

    const cyclic: Record<string, unknown> = { name: 'loop' }
    cyclic['self'] = cyclic

    const run = mcpd.trace('run')
    run.setInput(cyclic)
    run.end()
    await expect(mcpd.shutdown()).resolves.toBeUndefined()

    // The cycle costs its own key; the span, and the batch, still go out.
    expect(fake.calls).toHaveLength(1)
    expect(fake.spans()[0]?.['input']).toEqual({ name: 'loop', self: '[circular]' })
    expect(errors.some((e) => e.includes('export'))).toBe(false)
  })
})

describe('enabled = false', () => {
  it('records nothing and still returns valid objects', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch, { enabled: false })

    const run = mcpd.trace('run')
    expect(run).toBeInstanceOf(McpdSpan)
    expect(isTraceId(run.traceId)).toBe(true)
    expect(isSpanId(run.spanId)).toBe(true)

    const child = run.child('step')
    expect(child.traceId).toBe(run.traceId)
    expect(child.traceparent()).toMatch(/^00-[0-9a-f]{32}-[0-9a-f]{16}-\d{2}$/)

    const result = await mcpd.withSpan('work', async () => 42)
    expect(result).toBe(42)

    mcpd.log('info', 'nothing happens')
    mcpd.metric('m', 1, 'counter')
    mcpd.score({ scorerId: 's', value: 1, min: 0, max: 1 })
    mcpd.feedback({ kind: 'thumbs', value: 'up' })
    child.end()
    run.end()

    await mcpd.flush()
    await mcpd.shutdown()
    expect(fake.calls).toHaveLength(0)
  })

  it('returns an empty page from the list reads', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch, { enabled: false })
    await expect(mcpd.listTraces()).resolves.toEqual({ traces: [], page: { hasMore: false } })
    await expect(mcpd.listSpans()).resolves.toEqual({ spans: [], page: { hasMore: false } })
    expect(fake.calls).toHaveLength(0)
    await mcpd.shutdown()
  })
})

describe('reads', () => {
  it('scopes a query to the client project and converts the keys', async () => {
    const fake = fakeFetch([{ status: 200, body: { traces: [], page: { has_more: false } } }])
    const mcpd = client(fake.fetch)
    const page = await mcpd.listTraces({ minDurationMs: 500, tags: ['support'] })

    expect(fake.calls[0]?.url).toBe(
      'http://daemon.test/v1/api/traces?project=test&min_duration_ms=500&tags=support',
    )
    expect(page.page.hasMore).toBe(false)
    await mcpd.shutdown()
  })

  it('converts a response back into the camelCase model', async () => {
    const fake = fakeFetch([
      {
        status: 200,
        body: {
          traces: [
            {
              trace_id: 'a'.repeat(32),
              name: 'support_agent',
              kind: 'agent_run',
              status: 'ok',
              start_time: '2026-01-01T00:00:00.000000000Z',
              span_count: 3,
              error_count: 0,
              model_count: 1,
              tool_count: 1,
              is_complete: true,
              received_at: '2026-01-01T00:00:00.000000000Z',
              correlation: { project: 'test', session_id: 's-1' },
              usage: { input_tokens: 10, input_details: { cache_write_5m: 2 } },
            },
          ],
          page: { has_more: false, total: 1 },
        },
      },
    ])
    const mcpd = client(fake.fetch)
    const page = await mcpd.listTraces()
    const trace = page.traces[0]

    expect(trace?.traceId).toBe('a'.repeat(32))
    expect(trace?.spanCount).toBe(3)
    expect(trace?.correlation.sessionId).toBe('s-1')
    expect(trace?.usage?.inputDetails?.cacheWrite5m).toBe(2)
    expect(page.page.total).toBe(1)
    await mcpd.shutdown()
  })

  it('sends the feedback list only the parameters its endpoint documents', async () => {
    const fake = fakeFetch([{ status: 200, body: { feedback: [], page: { has_more: false } } }])
    const mcpd = client(fake.fetch)
    // The extra keys stand for a caller the type checker cannot see, such as
    // plain JavaScript. The daemon answers 400 to every one of them.
    const query = { traceId: 'a'.repeat(32), limit: 5, kind: 'thumbs', from: '-1h', cursor: 'c' }
    await mcpd.listFeedback(query as unknown as FeedbackQuery)

    expect(fake.calls[0]?.url).toBe(
      `http://daemon.test/v1/api/feedback?project=test&trace_id=${'a'.repeat(32)}&limit=5`,
    )
    await expect(mcpd.listFeedback({} as FeedbackQuery)).rejects.toThrow(/needs a traceId/)
    expect(fake.calls).toHaveLength(1)
    await mcpd.shutdown()
  })

  it('sends the metric and analytics reads only the parameters their endpoints document', async () => {
    const fake = fakeFetch([{ status: 200, body: {} }])
    const mcpd = client(fake.fetch)
    // `limit` and `cursor` stand for a caller the type checker cannot see. The
    // metrics endpoint answers 400 to both, and analytics is not paged.
    const paging = { limit: 5, cursor: 'c' }
    await mcpd.queryMetrics({
      name: 'calls',
      from: '-1h',
      to: '-0s',
      groupBy: ['model'],
      filter: { model: 'gpt-4o', status: 'ok' },
      ...paging,
    } as MetricQuery)
    await mcpd.analyticsSummary({ from: '-1h', to: '-0s', ...paging } as WindowQuery)
    await mcpd.analyticsCost({ from: '-1h', to: '-0s', bucket: '1h', ...paging } as CostQuery)

    expect(fake.calls.map((c) => c.url)).toEqual([
      'http://daemon.test/v1/api/metrics?project=test&name=calls&from=-1h&to=-0s&group_by=model' +
        '&filter=model%3Agpt-4o%2Cstatus%3Aok',
      'http://daemon.test/v1/api/analytics/summary?project=test&from=-1h&to=-0s',
      'http://daemon.test/v1/api/analytics/cost?project=test&from=-1h&to=-0s&bucket=1h',
    ])
    // The daemon reads key:value pairs, so a key or value it would split is
    // refused before a request is made.
    const refused: Array<Record<string, string>> = [{ 'a:b': 'x' }, { 'a,b': 'x' }, { '': 'x' }, { model: 'a,b' }]
    for (const filter of refused) {
      await expect(mcpd.queryMetrics({ name: 'calls', from: '-1h', to: '-0s', filter })).rejects.toThrow(
        /metric filter/,
      )
    }
    expect(fake.calls).toHaveLength(3)
    await mcpd.shutdown()
  })

  it('throws when a read fails, rather than hiding the outage', async () => {
    const fake = fakeFetch([{ status: 404, body: { error: 'no such trace' } }])
    const mcpd = client(fake.fetch)
    await expect(mcpd.getTrace('a'.repeat(32))).rejects.toMatchObject({
      name: 'McpdError',
      status: 404,
    })
    await mcpd.shutdown()
  })
})

describe('correlation', () => {
  it('puts the project, environment and service on every signal', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch, {
      environment: 'production',
      serviceName: 'support-api',
      serviceVersion: '2.1.0',
      correlation: { release: 'abc123' },
    })
    const run = mcpd.trace('run', { correlation: { sessionId: 'sess-1', userId: 'u-9' } })
    run.end()
    await mcpd.shutdown()

    expect((fake.spans()[0] as Record<string, unknown>)['correlation']).toEqual({
      project: 'test',
      environment: 'production',
      service_name: 'support-api',
      service_version: '2.1.0',
      release: 'abc123',
      session_id: 'sess-1',
      user_id: 'u-9',
    })
  })
})
