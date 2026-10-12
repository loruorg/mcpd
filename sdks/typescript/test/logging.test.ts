import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  ALL_LOG_LEVEL,
  Mcpd,
  RecordingSink,
  describeError,
  logLevelFromSeverity,
  logSeverity,
} from '../src/index.js'
import type { LogLevel } from '../src/index.js'
import { fakeFetch } from './helpers.js'

afterEach(() => {
  vi.unstubAllEnvs()
})

function client(sink: RecordingSink, options = {}) {
  return new Mcpd({
    project: 'test',
    sink,
    flushIntervalMs: 10_000,
    flushOnExit: false,
    onError: () => {},
    ...options,
  })
}

describe('a correlation key with no value (P301)', () => {
  it('leaves the client\'s session on a log line and a metric point', async () => {
    const sink = new RecordingSink()
    const mcpd = client(sink, { correlation: { sessionId: 'session-7', userId: 'u-1' } })
    await mcpd.withSpan(
      'step',
      async () => {
        mcpd.log('info', 'inside')
        mcpd.metric('latency_ms', 4)
      },
      // Present, with no value. Not an instruction to erase the run's own.
      { correlation: { sessionId: undefined } },
    )
    await mcpd.flush()

    expect(sink.logs[0]!.correlation.sessionId).toBe('session-7')
    expect(sink.logs[0]!.correlation.userId).toBe('u-1')
    expect(sink.metrics[0]!.correlation.sessionId).toBe('session-7')
    await mcpd.shutdown()
  })

  it('does not let a client correlation option erase the environment', async () => {
    vi.stubEnv('MCPD_ENVIRONMENT', 'production')
    const sink = new RecordingSink()
    const mcpd = client(sink, { correlation: { environment: undefined, release: 'sha-1' } })
    mcpd.log('info', 'outside a span')
    await mcpd.flush()

    expect(sink.logs[0]!.correlation.environment).toBe('production')
    expect(sink.logs[0]!.correlation.release).toBe('sha-1')
    await mcpd.shutdown()
  })

  it('still lets a value win over the client\'s', async () => {
    const sink = new RecordingSink()
    const mcpd = client(sink, { correlation: { sessionId: 'client' } })
    await mcpd.withSpan(
      'step',
      async () => {
        mcpd.log('info', 'inside')
      },
      { correlation: { sessionId: 'run' } },
    )
    await mcpd.flush()

    expect(sink.logs[0]!.correlation.sessionId).toBe('run')
    await mcpd.shutdown()
  })
})

describe('an error under a field key (P300)', () => {
  it('arrives as its type, message and stack', async () => {
    const sink = new RecordingSink()
    const mcpd = client(sink)
    mcpd.log('error', 'the call failed', { error: new TypeError('boom'), attempt: 2 })
    await mcpd.flush()

    const fields = sink.logs[0]!.fields as { error: Record<string, string>; attempt: number }
    expect(fields.attempt).toBe(2)
    expect(fields.error.type).toBe('TypeError')
    expect(fields.error.message).toBe('boom')
    expect(fields.error.stack).toContain('TypeError: boom')
    // The defect this replaces: a plain JSON encode of an Error is `{}`.
    expect(JSON.stringify(new TypeError('boom'))).toBe('{}')
    await mcpd.shutdown()
  })

  it('serialises an error nested in a list or a map, and a metric label', async () => {
    const sink = new RecordingSink()
    const mcpd = client(sink)
    mcpd.log('warn', 'retries exhausted', { attempts: [new Error('first')] })
    mcpd.metric('failures', 1, 'counter', { labels: { cause: new Error('label') } })
    await mcpd.flush()

    // Compared as a whole: an Error left as it was has no enumerable own
    // property at all, so a check on `.message` alone would pass on the defect.
    const fields = sink.logs[0]!.fields as { attempts: Array<Record<string, string>> }
    expect(fields.attempts[0]).toEqual({
      type: 'Error',
      message: 'first',
      stack: expect.any(String),
    })
    const labels = sink.metrics[0]!.labels as { cause: Record<string, string> }
    expect(labels.cause).toEqual({ type: 'Error', message: 'label', stack: expect.any(String) })
    await mcpd.shutdown()
  })

  it('costs one field, not the batch it travelled in', async () => {
    const fetcher = fakeFetch([{ status: 200, body: { accepted: 2, rejected: 0 } }])
    const mcpd = new Mcpd({
      endpoint: 'http://daemon.test',
      project: 'test',
      fetch: fetcher.fetch,
      flushIntervalMs: 10_000,
      flushOnExit: false,
      onError: () => {},
    })
    const cycle: Record<string, unknown> = { name: 'request' }
    cycle.self = cycle
    mcpd.trace('run').end()
    mcpd.log('error', 'a request went wrong', { request: cycle, size: 10n })
    await mcpd.flush()

    // The batch reached the transport, JSON and all, with both signals in it.
    expect(mcpd.stats().failed).toBe(0)
    expect(mcpd.stats().exported).toBe(2)
    const body = fetcher.calls[0]!.body as { logs: Array<{ fields: Record<string, unknown> }> }
    expect(body.logs).toHaveLength(1)
    expect(body.logs[0]!.fields).toEqual({
      request: { name: 'request', self: '[circular]' },
      size: '10',
    })
    await mcpd.shutdown()
  })

  it('costs one value on a span, a score or feedback too, and never the batch', async () => {
    // A cycle or a BigInt in a span's payload made JSON.stringify throw in the
    // transport, and every signal that shared the batch went with it.
    const fetcher = fakeFetch([{ status: 200, body: { accepted: 5, rejected: 0 } }])
    const mcpd = new Mcpd({
      endpoint: 'http://daemon.test',
      project: 'test',
      fetch: fetcher.fetch,
      flushIntervalMs: 10_000,
      flushOnExit: false,
      onError: () => {},
    })
    const cycle: Record<string, unknown> = { name: 'request' }
    cycle.self = cycle
    const run = mcpd.trace('run', { input: cycle })
    run.setOutput({ total: 10n, ratio: Number.NaN })
    run.setMetadata({ caller: cycle })
    run.end()
    mcpd.score({ traceId: run.traceId, scorerId: 'judge', value: 0.5, min: 0, max: 1, metadata: { n: 1n } })
    mcpd.feedback({ traceId: run.traceId, kind: 'thumbs', value: { votes: 2n } })
    // A number JSON cannot carry is the signal itself here, so only it is lost.
    mcpd.metric('ratio', Number.NaN)
    mcpd.score({ traceId: run.traceId, scorerId: 'broken', value: Number.POSITIVE_INFINITY, min: 0, max: 1 })
    mcpd.log('info', 'beside them')
    expect(await mcpd.flush()).toBe(true)

    expect(mcpd.stats().failed).toBe(0)
    expect(fetcher.calls).toHaveLength(1)
    const body = fetcher.calls[0]!.body as Record<string, Array<Record<string, unknown>>>
    const span = body.spans!.find((s) => s.end_time !== undefined)!
    expect(span.input).toEqual({ name: 'request', self: '[circular]' })
    expect(span.output).toEqual({ total: '10', ratio: 'NaN' })
    expect(span.metadata).toEqual({ caller: { name: 'request', self: '[circular]' } })
    expect(body.scores!.map((s) => s.scorer_id)).toEqual(['judge'])
    expect(body.scores![0]!.metadata).toEqual({ n: '1' })
    expect(body.feedback![0]!.value).toEqual({ votes: '2' })
    expect(body.metrics).toBeUndefined()
    expect(body.logs).toHaveLength(1)
    await mcpd.shutdown()
  })

  it('describeError answers only for an error', () => {
    expect(describeError(new RangeError('out'))).toMatchObject({
      type: 'RangeError',
      message: 'out',
    })
    expect(describeError('not an error')).toBeUndefined()
  })
})

describe('the level floor (P085)', () => {
  it('drops trace by default and keeps debug', async () => {
    const sink = new RecordingSink()
    const mcpd = client(sink)
    expect(mcpd.logLevel).toBe('debug')
    mcpd.log('trace', 'finer than debug')
    mcpd.log('debug', 'kept')
    await mcpd.flush()

    expect(sink.logs.map((l) => l.message)).toEqual(['kept'])
    await mcpd.shutdown()
  })

  it('drops everything below a floor set in code', async () => {
    const sink = new RecordingSink()
    const mcpd = client(sink, { logLevel: 'warn' })
    for (const level of ALL_LOG_LEVEL) mcpd.log(level, level)
    await mcpd.flush()

    expect(sink.logs.map((l) => l.message)).toEqual(['warn', 'error', 'fatal'])
    expect(mcpd.stats().dropped).toBe(0)
    await mcpd.shutdown()
  })

  it('reads MCPD_LOG_LEVEL, and code wins over it', async () => {
    vi.stubEnv('MCPD_LOG_LEVEL', 'ERROR')
    const sink = new RecordingSink()
    const fromEnv = client(sink)
    expect(fromEnv.logLevel).toBe('error')
    fromEnv.log('warn', 'below the floor')
    await fromEnv.flush()
    expect(sink.logs).toHaveLength(0)
    await fromEnv.shutdown()

    const fromCode = client(sink, { logLevel: 'trace' })
    expect(fromCode.logLevel).toBe('trace')
    await fromCode.shutdown()
  })

  it('ignores a name that is not a level', () => {
    vi.stubEnv('MCPD_LOG_LEVEL', 'verbose')
    expect(client(new RecordingSink()).logLevel).toBe('debug')
  })

  it('never filters the other four signals', async () => {
    const sink = new RecordingSink()
    const mcpd = client(sink, { logLevel: 'fatal' })
    mcpd.trace('run').end()
    mcpd.metric('latency_ms', 3)
    mcpd.score({ scorerId: 'judge', value: 1, min: 0, max: 1 })
    mcpd.feedback({ kind: 'thumbs', value: 'up' })
    await mcpd.flush()

    expect(sink.spans).toHaveLength(1)
    expect(sink.metrics).toHaveLength(1)
    expect(sink.scores).toHaveLength(1)
    expect(sink.feedback).toHaveLength(1)
    await mcpd.shutdown()
  })
})

describe('OpenTelemetry severity numbers (P090)', () => {
  it('maps every level to the number the spec gives it', () => {
    expect(ALL_LOG_LEVEL.map(logSeverity)).toEqual([1, 5, 9, 13, 17, 21])
  })

  it('maps every number back by range', () => {
    const ranges: Array<[number, number, LogLevel]> = [
      [1, 4, 'trace'],
      [5, 8, 'debug'],
      [9, 12, 'info'],
      [13, 16, 'warn'],
      [17, 20, 'error'],
      [21, 24, 'fatal'],
    ]
    for (const [from, to, level] of ranges) {
      for (let n = from; n <= to; n++) expect(logLevelFromSeverity(n)).toBe(level)
    }
  })

  it('clamps a number outside the range instead of refusing it', () => {
    expect(logLevelFromSeverity(0)).toBe('trace')
    expect(logLevelFromSeverity(-3)).toBe('trace')
    expect(logLevelFromSeverity(40)).toBe('fatal')
  })

  it('round-trips every level the spec defines', () => {
    for (const level of ALL_LOG_LEVEL) {
      expect(logLevelFromSeverity(logSeverity(level))).toBe(level)
      expect(logSeverity(level)).toBeGreaterThan(0)
    }
  })
})
