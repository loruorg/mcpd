import { afterEach, describe, expect, it, vi } from 'vitest'
import { Mcpd } from '../src/index.js'
import type { Span } from '../src/index.js'
import { fakeFetch, spanFixture } from './helpers.js'

afterEach(() => {
  vi.unstubAllEnvs()
})

type Errors = Array<{ message: string; error: unknown }>

function client(fetchImpl = fakeFetch().fetch, options = {}, errors: Errors = []) {
  return new Mcpd({
    endpoint: 'http://daemon.test',
    project: 'test',
    fetch: fetchImpl,
    flushIntervalMs: 10_000,
    flushOnExit: false,
    onError: (message, error) => errors.push({ message, error }),
    ...options,
  })
}

const byName = (spans: Array<Record<string, unknown>>, name: string) =>
  spans.find((s) => s['name'] === name)

describe('payload capture', () => {
  it('capturePayloads=false keeps input and output off a span, an event and a recorded span', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch, { capturePayloads: false })

    const root = mcpd.trace('run', {
      input: { q: 'secret' },
      output: 'secret',
      model: 'claude-sonnet-4',
      usage: { inputTokens: 3 },
    })
    root.setInput({ q: 'also secret' }).setOutput('also secret')
    root.setModel('claude-sonnet-4', 'anthropic').setUsage({ outputTokens: 2 })
    root.setModelParameters({ temperature: 0, seed: 0, stopSequences: ['STOP'] })
    root.event('chunk', { input: 'delta', output: 'delta', attributes: { index: 1 } })
    root.setError('boom').end()
    mcpd.record({ ...spanFixture(1), output: 'secret' })
    await mcpd.flush()

    const spans = fake.spans()
    expect(spans).toHaveLength(3)
    for (const span of spans) {
      expect(span).not.toHaveProperty('input')
      expect(span).not.toHaveProperty('output')
    }
    // Everything that is not a payload survives.
    const run = byName(spans, 'run')!
    expect(run['model']).toBe('claude-sonnet-4')
    expect(run['provider']).toBe('anthropic')
    expect(run['usage']).toEqual({ input_tokens: 3, output_tokens: 2 })
    // Sampling settings are metadata, not payload: they carry no prompt, and
    // they are what a run has to be reproduced from. Capture off must not take
    // them, and a zero seed must survive being read as falsy.
    expect(run['model_parameters']).toEqual({
      temperature: 0,
      seed: 0,
      stop_sequences: ['STOP'],
      has_response_format: false,
    })
    expect(run['status']).toBe('error')
    expect((run['error'] as { message: string }).message).toBe('boom')
    expect(run['end_time']).toBeDefined()
    expect(byName(spans, 'chunk')!['attributes']).toEqual({ index: 1 })
    expect(byName(spans, 'step-1')!['kind']).toBe('tool_call')
    expect(root.capturesPayloads).toBe(false)
    await mcpd.shutdown()
  })

  it('reads MCPD_CAPTURE_PAYLOADS, and code wins over it', async () => {
    vi.stubEnv('MCPD_CAPTURE_PAYLOADS', 'false')
    const fromEnv = fakeFetch()
    const off = client(fromEnv.fetch)
    off.trace('run', { input: 'secret' }).end()
    await off.flush()
    expect(fromEnv.spans()[0]).not.toHaveProperty('input')
    await off.shutdown()

    const fromCode = fakeFetch()
    const on = client(fromCode.fetch, { capturePayloads: true })
    on.trace('run', { input: 'kept' }).end()
    await on.flush()
    expect(fromCode.spans()[0]!['input']).toBe('kept')
    await on.shutdown()
  })

  it('a span overrides the client for itself only, in either direction', async () => {
    const fake = fakeFetch()
    const off = client(fake.fetch, { capturePayloads: false })
    const shown = off.trace('shown', { input: 'public', capturePayloads: true })
    // The override is the span's own: its child follows the client again.
    shown.child('hidden', { input: 'secret' }).end()
    shown.end()
    await off.flush()
    expect(byName(fake.spans(), 'shown')!['input']).toBe('public')
    expect(byName(fake.spans(), 'hidden')).not.toHaveProperty('input')
    await off.shutdown()

    const other = fakeFetch()
    const on = client(other.fetch)
    const quiet = on.trace('quiet', { input: 'secret', capturePayloads: false })
    quiet.setOutput('secret')
    quiet.child('loud', { input: 'public' }).end()
    quiet.end()
    await on.flush()
    const quietSpan = byName(other.spans(), 'quiet')!
    expect(quietSpan).not.toHaveProperty('input')
    expect(quietSpan).not.toHaveProperty('output')
    expect(byName(other.spans(), 'loud')!['input']).toBe('public')
    await on.shutdown()
  })
})

describe('record', () => {
  it('queues a finished span as given, filling only the correlation it left empty', async () => {
    const fake = fakeFetch()
    // Sample 0: a trace started here would be dropped. A recorded span is never re-sampled.
    const mcpd = client(fake.fetch, {
      sample: 0,
      environment: 'prod',
      serviceName: 'billing',
    })
    const span: Span = {
      ...spanFixture(7),
      parentSpanId: 'c3'.repeat(8),
      externalParentSpanId: 'd4'.repeat(8),
      correlation: { project: 'imported', userId: 'u-1' },
    }
    mcpd.record(span)
    mcpd.trace('dropped').end()
    await mcpd.flush()

    const spans = fake.spans()
    expect(spans).toHaveLength(1)
    const wire = spans[0]!
    expect(wire['trace_id']).toBe('a'.repeat(32))
    expect(wire['span_id']).toBe('0000000000000007')
    expect(wire['parent_span_id']).toBe('c3'.repeat(8))
    expect(wire['external_parent_span_id']).toBe('d4'.repeat(8))
    expect(wire['start_time']).toBe('2026-01-01T00:00:00.000000000Z')
    expect(wire['end_time']).toBe('2026-01-01T00:00:00.250000000Z')
    expect(wire['input']).toBe('x'.repeat(64))
    expect(wire['correlation']).toEqual({
      project: 'imported',
      environment: 'prod',
      service_name: 'billing',
      user_id: 'u-1',
    })
    expect(mcpd.stats().rejected).toBe(0)
    await mcpd.shutdown()
  })

  it('refuses each malformed shape by counting, never by throwing', async () => {
    const fake = fakeFetch()
    const errors: Errors = []
    const mcpd = client(fake.fetch, {}, errors)
    const bad: Span[] = [
      { ...spanFixture(1), traceId: 'abc' },
      { ...spanFixture(1), traceId: '0'.repeat(32) },
      { ...spanFixture(1), traceId: 'A'.repeat(32) },
      { ...spanFixture(1), spanId: '0'.repeat(16) },
      { ...spanFixture(1), spanId: 'zz'.repeat(8) },
      { ...spanFixture(1), parentSpanId: '0'.repeat(16) },
      { ...spanFixture(1), name: '' },
      { ...spanFixture(1), startTime: '' },
    ]
    for (const span of bad) expect(() => mcpd.record(span)).not.toThrow()
    expect(() => mcpd.record(null as unknown as Span)).not.toThrow()
    await mcpd.flush()

    expect(fake.spans()).toHaveLength(0)
    expect(mcpd.stats().rejected).toBe(bad.length + 1)
    expect(errors.length).toBeGreaterThan(0)
    expect(errors[0]!.message).toContain('record refused a span')
    await mcpd.shutdown()
  })

  it('is a no-op when the client is disabled', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch, { enabled: false })
    mcpd.record(spanFixture(1))
    await mcpd.flush()
    expect(fake.calls).toHaveLength(0)
    expect(mcpd.stats().rejected).toBe(0)
  })
})

describe('an external parent', () => {
  it('reaches the wire on a span started here', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)
    const root = mcpd.trace('run', { externalParentSpanId: 'ab'.repeat(8) })
    root.child('step', { externalParentSpanId: 'cd'.repeat(8) }).end()
    root.end()
    await mcpd.flush()
    expect(byName(fake.spans(), 'run')!['external_parent_span_id']).toBe('ab'.repeat(8))
    expect(byName(fake.spans(), 'step')!['external_parent_span_id']).toBe('cd'.repeat(8))
    // Parenting inside mcpd's own tree is untouched.
    expect(byName(fake.spans(), 'step')!['parent_span_id']).toBe(root.spanId)
    await mcpd.shutdown()
  })
})

describe('stats', () => {
  it('answers the six counters after recording, exporting and refusing', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch, { maxQueue: 2 })
    mcpd.trace('one').end()
    mcpd.trace('two').end()
    mcpd.trace('three').end()
    mcpd.record({ ...spanFixture(1), name: '' })
    const before = mcpd.stats()
    expect(before).toMatchObject({ queued: 2, dropped: 1, capacity: 2, exported: 0, failed: 0, rejected: 1 })
    await mcpd.flush()
    const after = mcpd.stats()
    expect(after).toMatchObject({ queued: 0, dropped: 1, capacity: 2, exported: 2, failed: 0, rejected: 1 })
    await mcpd.shutdown()
  })
})
