import { describe, expect, it } from 'vitest'
import { Mcpd } from '../src/index.js'
import type { Correlation } from '../src/index.js'
import { fakeFetch } from './helpers.js'

/**
 * Correlation: what a run is tagged with, and what inherits it.
 *
 * The property under test is not "a span has a session id". It is that the five
 * run-scoped fields are set once, on the run, and every step and every log line
 * underneath it carries the same ones. A trace whose correlation stops at the
 * root cannot be filtered by session or erased by subject, which is most of
 * what correlation is for.
 */

/** The five fields a caller sets on a run, and what each is called on the wire.
 *
 * The SDK's own types are camelCase and the transport maps them to the spec's
 * snake_case on the way out, so a test that reads a captured body has to ask
 * for the wire name. Asking for the camelCase one gets `undefined` from every
 * signal, which passes nothing and proves nothing. */
const TAGS: Partial<Correlation> = {
  sessionId: 'sess-1',
  threadId: 'thread-7',
  userId: 'user-41',
  requestId: 'req-9001',
  release: '2026.08.1',
}

const RUN_FIELDS: Array<[wire: string, value: string]> = [
  ['session_id', 'sess-1'],
  ['thread_id', 'thread-7'],
  ['user_id', 'user-41'],
  ['request_id', 'req-9001'],
  ['release', '2026.08.1'],
]

function client(fetchImpl: ReturnType<typeof fakeFetch>) {
  return new Mcpd({
    endpoint: 'http://daemon.test',
    project: 'corr',
    environment: 'staging',
    serviceName: 'probe',
    serviceVersion: '9.9.9',
    fetch: fetchImpl.fetch,
    flushIntervalMs: 10_000,
    flushOnExit: false,
    onError: () => {},
  })
}

type Batch = Record<string, Array<Record<string, unknown>> | undefined>

/** Every signal of one kind from every batch that was sent. */
function sent(fake: ReturnType<typeof fakeFetch>, kind: string): Array<Record<string, unknown>> {
  const out: Array<Record<string, unknown>> = []
  for (const call of fake.calls) {
    const batch = call.body as Batch | undefined
    for (const signal of batch?.[kind] ?? []) out.push(signal)
  }
  return out
}

function correlationOf(signal: Record<string, unknown> | undefined): Record<string, unknown> {
  return (signal?.correlation ?? {}) as Record<string, unknown>
}

function spanNamed(fake: ReturnType<typeof fakeFetch>, name: string): Record<string, unknown> {
  const found = fake.spans().filter((s) => s.name === name)
  expect(found.length, `no span called ${name} was sent`).toBeGreaterThan(0)
  // B8 puts a span on the wire on start and again on end. The queue replaces an
  // unsent earlier state, so the last copy is the one a reader sees.
  return found[found.length - 1]!
}

describe('correlation inheritance', () => {
  it('a child inherits what the run was tagged with', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake)

    const run = mcpd.trace('support_agent', { correlation: TAGS })
    run.child('lookup_order', { kind: 'tool_call' }).end()
    run.end()
    await mcpd.shutdown()

    const root = correlationOf(spanNamed(fake, 'support_agent'))
    const child = correlationOf(spanNamed(fake, 'lookup_order'))
    for (const [field] of RUN_FIELDS) {
      expect(child[field], `the child did not inherit ${field}`).toBe(root[field])
    }
  })

  it('a field set on the run after it started reaches a child started afterwards', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake)

    const run = mcpd.trace('support_agent')
    run.setCorrelation({ sessionId: 'sess-late', userId: 'user-late' })
    run.child('lookup_order', { kind: 'tool_call' }).end()
    run.end()
    await mcpd.shutdown()

    const child = correlationOf(spanNamed(fake, 'lookup_order'))
    expect(child.session_id).toBe('sess-late')
    expect(child.user_id).toBe('user-late')
  })

  it('a grandchild inherits it too', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake)

    const run = mcpd.trace('support_agent', { correlation: TAGS })
    const step = run.child('plan_answer', { kind: 'model_generation' })
    step.child('call_model').end()
    step.end()
    run.end()
    await mcpd.shutdown()

    const inner = correlationOf(spanNamed(fake, 'call_model'))
    for (const [field, value] of RUN_FIELDS) {
      expect(inner[field], `the grandchild lost ${field}`).toBe(value)
    }
  })

  it('a child may override one field without losing the rest', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake)

    const run = mcpd.trace('support_agent', { correlation: TAGS })
    run.child('sub_agent', { kind: 'agent_run', correlation: { sessionId: 'sess-2' } }).end()
    run.end()
    await mcpd.shutdown()

    const child = correlationOf(spanNamed(fake, 'sub_agent'))
    expect(child.session_id, 'the override was lost').toBe('sess-2')
    expect(child.user_id, 'the override erased the rest').toBe('user-41')
    expect(child.release).toBe('2026.08.1')
  })

  it('an event inherits the run correlation', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake)

    const run = mcpd.trace('support_agent', { correlation: TAGS })
    run.event('first_token')
    run.end()
    await mcpd.shutdown()

    const event = correlationOf(spanNamed(fake, 'first_token'))
    expect(event.session_id).toBe('sess-1')
    expect(event.release).toBe('2026.08.1')
  })

  it('a span started under the active one inherits from it', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake)

    const run = mcpd.trace('support_agent', { correlation: TAGS })
    await mcpd.withActiveSpan(run, async () => {
      await mcpd.withSpan('lookup_order', (span) => {
        span.setInput('41')
      })
    })
    run.end()
    await mcpd.shutdown()

    const child = correlationOf(spanNamed(fake, 'lookup_order'))
    expect(child.session_id).toBe('sess-1')
    expect(child.user_id).toBe('user-41')
  })
})

describe('the other four signals', () => {
  it('a log inside a run carries the run correlation', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake)

    const run = mcpd.trace('support_agent', { correlation: TAGS })
    mcpd.withActiveSpan(run, () => {
      mcpd.log('warn', 'estimate_delivery failed')
    })
    run.end()
    await mcpd.shutdown()

    const logs = sent(fake, 'logs')
    expect(logs.length).toBe(1)
    const line = correlationOf(logs[0])
    for (const [field, value] of RUN_FIELDS) {
      expect(line[field], `the log dropped ${field}`).toBe(value)
    }
    // The deployment three still come from the client.
    expect(line.project).toBe('corr')
    expect(line.environment).toBe('staging')
    expect(line.service_name).toBe('probe')
  })

  it('a metric, a score and feedback carry it too', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake)

    const run = mcpd.trace('support_agent', { correlation: TAGS })
    mcpd.withActiveSpan(run, () => {
      mcpd.metric('run_steps', 3, 'counter')
      mcpd.score({ scorerId: 'relevance', value: 0.86, min: 0, max: 1 })
      mcpd.feedback({ kind: 'thumbs', value: 'up' })
    })
    run.end()
    await mcpd.shutdown()

    for (const kind of ['metrics', 'scores', 'feedback']) {
      const signals = sent(fake, kind)
      expect(signals.length, `expected one ${kind}`).toBe(1)
      expect(correlationOf(signals[0]).session_id, `the ${kind} dropped the session`).toBe('sess-1')
    }
  })

  it('a log outside any run still carries the client correlation', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake)

    mcpd.log('info', 'started')
    await mcpd.shutdown()

    const logs = sent(fake, 'logs')
    expect(logs.length).toBe(1)
    const line = correlationOf(logs[0])
    expect(line.project).toBe('corr')
    expect(line.service_name).toBe('probe')
    // Nothing to inherit, so the run-scoped fields are absent rather than stale.
    expect(line.session_id).toBeUndefined()
  })
})
