import { describe, expect, it, vi } from 'vitest'
import {
  McpdError,
  RateLimitedDiagnostics,
  Transport,
  camelKey,
  encodeQuery,
  fromWire,
  snakeKey,
  toWire,
} from '../src/transport.js'
import type { FetchLike } from '../src/index.js'
import { fakeFetch, spanFixture } from './helpers.js'

const silent = { report: () => {} }

function transport(fetchImpl: FetchLike, extra: Record<string, unknown> = {}): Transport {
  return new Transport({
    endpoint: 'http://daemon.test/',
    timeoutMs: 1_000,
    fetchImpl,
    diagnostics: silent,
    baseBackoffMs: 1,
    ...extra,
  })
}

describe('key case', () => {
  it('converts the model both ways', () => {
    expect(snakeKey('sessionId')).toBe('session_id')
    expect(snakeKey('externalParentSpanId')).toBe('external_parent_span_id')
    expect(camelKey('external_parent_span_id')).toBe('externalParentSpanId')
    expect(camelKey(snakeKey('minDurationMs'))).toBe('minDurationMs')
  })

  it('handles the two names the mechanical rule gets wrong', () => {
    expect(snakeKey('cacheWrite5m')).toBe('cache_write_5m')
    expect(snakeKey('cacheWrite1h')).toBe('cache_write_1h')
    expect(camelKey('cache_write_5m')).toBe('cacheWrite5m')
    expect(camelKey('cache_write_1h')).toBe('cacheWrite1h')
  })

  it('leaves the percentile names alone', () => {
    expect(snakeKey('p50')).toBe('p50')
    expect(camelKey('p99')).toBe('p99')
  })

  it('never rewrites the keys inside a payload', () => {
    // The prompt is the product. Rewriting a user's key would corrupt it.
    const wire = toWire({
      spanId: 'abc',
      input: { userName: 'ada', nested: { anotherKey: [{ deepKey: 1 }] } },
      metadata: { camelCase: true },
      attributes: { toolName: 'search' },
    }) as Record<string, unknown>

    expect(wire['span_id']).toBe('abc')
    expect(wire['input']).toEqual({ userName: 'ada', nested: { anotherKey: [{ deepKey: 1 }] } })
    expect(wire['metadata']).toEqual({ camelCase: true })
    expect(wire['attributes']).toEqual({ toolName: 'search' })
  })

  it('round-trips a nested model object', () => {
    const span = {
      traceId: 'a',
      usage: { inputTokens: 10, inputDetails: { cacheRead: 2, cacheWrite5m: 1 } },
      correlation: { project: 'p', sessionId: 's' },
      links: [{ traceId: 'b', spanId: 'c' }],
    }
    const wire = toWire(span) as Record<string, unknown>
    expect(wire['usage']).toEqual({
      input_tokens: 10,
      input_details: { cache_read: 2, cache_write_5m: 1 },
    })
    expect(fromWire(wire)).toEqual(span)
  })

  it('drops undefined rather than sending nulls', () => {
    expect(toWire({ a: 1, b: undefined })).toEqual({ a: 1 })
  })
})

describe('query encoding', () => {
  it('converts keys, joins arrays with commas and skips empties', () => {
    // The daemon keeps the last of a repeated key, so `tags=a&tags=b` would
    // ask for traces tagged `b` alone.
    expect(
      encodeQuery({ project: 'p', minDurationMs: 250, tags: ['a', 'b'], name: undefined, q: '' }),
    ).toBe('?project=p&min_duration_ms=250&tags=a%2Cb')
    expect(encodeQuery({ tags: [] })).toBe('')
  })

  it('is empty when there is nothing to send', () => {
    expect(encodeQuery(undefined)).toBe('')
    expect(encodeQuery({})).toBe('')
  })
})

describe('retry', () => {
  it('retries 503 and keeps the same ingest key so a batch cannot double-count', async () => {
    const fake = fakeFetch([{ status: 503 }, { status: 503 }, { status: 200 }])
    await transport(fake.fetch).ingest({ spans: [] }, 'key-1')

    expect(fake.calls).toHaveLength(3)
    const keys = fake.calls.map((c) => (c.body as { ingest_key: string }).ingest_key)
    expect(keys).toEqual(['key-1', 'key-1', 'key-1'])
  })

  it('retries each of the statuses the contract names, and gives up after three attempts', async () => {
    for (const status of [429, 502, 503, 504]) {
      const fake = fakeFetch([{ status }])
      await expect(transport(fake.fetch).ingest({}, 'k')).rejects.toBeInstanceOf(McpdError)
      expect(fake.calls).toHaveLength(3)
    }
  })

  it('never retries a 4xx that is not 429', async () => {
    for (const status of [400, 401, 403, 404, 413, 422]) {
      const fake = fakeFetch([{ status }])
      await expect(transport(fake.fetch).ingest({}, 'k')).rejects.toMatchObject({ status })
      expect(fake.calls).toHaveLength(1)
    }
  })

  it('does not retry a 500, which will fail the same way again', async () => {
    const fake = fakeFetch([{ status: 500 }])
    await expect(transport(fake.fetch).ingest({}, 'k')).rejects.toMatchObject({ status: 500 })
    expect(fake.calls).toHaveLength(1)
  })

  it('retries a connection error', async () => {
    let attempts = 0
    const fetchImpl: FetchLike = async () => {
      attempts++
      if (attempts < 3) throw new TypeError('fetch failed')
      return new Response(JSON.stringify({ accepted: 1, rejected: 0, duplicate: 0 }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      })
    }
    const result = await transport(fetchImpl).ingest({}, 'k')
    expect(attempts).toBe(3)
    expect(result.accepted).toBe(1)
  })

  it('honours Retry-After instead of its own curve', async () => {
    const fake = fakeFetch([{ status: 429, headers: { 'retry-after': '0' } }, { status: 200 }])
    const started = Date.now()
    await transport(fake.fetch, { baseBackoffMs: 5_000 }).ingest({}, 'k')
    // The header asked for no delay, so the 5s backoff must not have been used.
    expect(Date.now() - started).toBeLessThan(1_000)
    expect(fake.calls).toHaveLength(2)
  })
})

describe('requests', () => {
  it('sends the bearer token when a key is configured', async () => {
    const fake = fakeFetch([{ status: 200 }])
    await transport(fake.fetch, { apiKey: 'secret' }).ingest({}, 'k')
    expect(fake.calls[0]?.headers['authorization']).toBe('Bearer secret')
  })

  it('sends no authorization header when no key is configured', async () => {
    const fake = fakeFetch([{ status: 200 }])
    await transport(fake.fetch).ingest({}, 'k')
    expect(fake.calls[0]?.headers['authorization']).toBeUndefined()
  })

  it('gzips a body over a kibibyte and leaves a small one alone', async () => {
    const small = fakeFetch([{ status: 200 }])
    await transport(small.fetch).ingest({}, 'k')
    expect(small.calls[0]?.headers['content-encoding']).toBeUndefined()

    const big = fakeFetch([{ status: 200 }])
    const spans = Array.from({ length: 40 }, (_value, i) => spanFixture(i))
    await transport(big.fetch).ingest({ spans }, 'k')

    expect(big.calls[0]?.headers['content-encoding']).toBe('gzip')
    // The helper could only parse this body by gunzipping it first, which is
    // the same thing the daemon does with `content-encoding: gzip`.
    expect((big.calls[0]?.body as { spans: unknown[] }).spans).toHaveLength(40)
  })

  it('strips a trailing slash from the endpoint so paths concatenate cleanly', async () => {
    const fake = fakeFetch([{ status: 200 }])
    await transport(fake.fetch).get('/healthz')
    expect(fake.calls[0]?.url).toBe('http://daemon.test/healthz')
  })
})

describe('diagnostics', () => {
  it('reports the first failure of a kind and suppresses the rest for an interval', () => {
    const sink = vi.fn()
    const diagnostics = new RateLimitedDiagnostics(60_000, sink)
    for (let i = 0; i < 100; i++) diagnostics.report('export failed', new Error('boom'))
    expect(sink).toHaveBeenCalledTimes(1)
  })

  it('counts what it suppressed and says so in the next line', () => {
    const sink = vi.fn()
    const diagnostics = new RateLimitedDiagnostics(0, sink)
    diagnostics.report('export failed', undefined)
    diagnostics.report('export failed', undefined)
    expect(sink).toHaveBeenCalledTimes(2)
    expect(sink.mock.calls[1]?.[0]).toContain('mcpd: export failed')
  })

  it('never throws, even when the sink does', () => {
    const diagnostics = new RateLimitedDiagnostics(0, () => {
      throw new Error('the logger is broken too')
    })
    expect(() => diagnostics.report('anything', undefined)).not.toThrow()
  })
})
