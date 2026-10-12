import { describe, expect, it } from 'vitest'
import { ConsoleSink, Mcpd, RecordingSink } from '../src/index.js'
import type { Sink } from '../src/index.js'

type Errors = Array<{ message: string; error: unknown }>

function client(sink: Sink, errors: Errors = []) {
  return new Mcpd({
    project: 'test',
    sink,
    flushIntervalMs: 10_000,
    flushOnExit: false,
    onError: (message, error) => errors.push({ message, error }),
  })
}

describe('sinks', () => {
  it('a RecordingSink sees exactly the signals recorded, in order', async () => {
    const sink = new RecordingSink()
    const mcpd = client(sink)
    await mcpd.withSpan('run', async (span) => {
      span.setInput('question')
      mcpd.log('info', 'hello')
      mcpd.metric('latency_ms', 12)
      mcpd.score({ scorerId: 'judge', value: 1, min: 0, max: 1 })
      mcpd.feedback({ kind: 'thumbs', value: 'up' })
    })
    await mcpd.flush()

    expect(sink.batches).toBe(1)
    expect(sink.spans.map((s) => s.name)).toEqual(['run'])
    expect(sink.spans[0]!.input).toBe('question')
    expect(sink.spans[0]!.endTime).toBeDefined()
    expect(sink.logs.map((l) => l.message)).toEqual(['hello'])
    expect(sink.logs[0]!.traceId).toBe(sink.spans[0]!.traceId)
    expect(sink.metrics.map((m) => m.name)).toEqual(['latency_ms'])
    expect(sink.scores.map((s) => s.scorerId)).toEqual(['judge'])
    expect(sink.feedback.map((f) => f.kind)).toEqual(['thumbs'])
    expect(mcpd.stats().exported).toBe(5)
    expect(mcpd.stats().failed).toBe(0)

    sink.clear()
    expect(sink.spans).toHaveLength(0)
    expect(sink.feedback).toHaveLength(0)
    expect(sink.batches).toBe(0)
    await mcpd.shutdown()
  })

  it('hands the sink an ingest key that is stable for the batch', async () => {
    const keys: string[] = []
    const sink: Sink = { export: (_batch, ingestKey) => void keys.push(ingestKey) }
    const mcpd = client(sink)
    mcpd.trace('run').end()
    await mcpd.flush()
    expect(keys).toHaveLength(1)
    expect(keys[0]).toMatch(/^[0-9a-f]{32}$/)
    await mcpd.shutdown()
  })

  it('a sink that throws is a failed batch the caller never sees', async () => {
    const errors: Errors = []
    const throwing: Sink = {
      export: () => {
        throw new Error('disk full')
      },
    }
    const mcpd = client(throwing, errors)
    mcpd.trace('run').end()
    await expect(mcpd.flush()).resolves.toBe(true)
    expect(mcpd.stats()).toMatchObject({ exported: 0, failed: 1, queued: 0 })
    expect(errors).toHaveLength(1)
    expect(errors[0]!.message).toContain('could not export a batch')
    await mcpd.shutdown()

    const rejecting: Sink = { export: async () => Promise.reject(new Error('later')) }
    const later: Errors = []
    const other = client(rejecting, later)
    other.trace('run').end()
    await expect(other.shutdown()).resolves.toBeUndefined()
    expect(other.stats().failed).toBe(1)
    expect(later).toHaveLength(1)
  })

  it("a ConsoleSink writes one JSON line per signal in the daemon's spelling", async () => {
    const lines: string[] = []
    const mcpd = client(new ConsoleSink({ write: (line) => lines.push(line) }))
    await mcpd.withSpan('run', async () => {
      mcpd.log('warn', 'careful')
    })
    await mcpd.flush()

    expect(lines).toHaveLength(2)
    const records = lines.map((l) => JSON.parse(l) as Record<string, unknown>)
    expect(records[0]!['signal']).toBe('span')
    expect(records[0]!['name']).toBe('run')
    expect(records[0]).toHaveProperty('trace_id')
    expect(records[0]).toHaveProperty('start_time')
    expect(records[0]).not.toHaveProperty('traceId')
    expect(records[1]!['signal']).toBe('log')
    expect(records[1]!['message']).toBe('careful')
    expect(records[1]!['trace_id']).toBe(records[0]!['trace_id'])
    await mcpd.shutdown()
  })
})
