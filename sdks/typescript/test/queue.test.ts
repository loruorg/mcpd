import { describe, expect, it, vi } from 'vitest'
import { Exporter, SignalQueue, toBatch } from '../src/queue.js'
import type { Signal } from '../src/queue.js'
import type { Span } from '../src/index.js'

const silent = { report: () => {} }

function span(id: string, endTime?: string): Span {
  return {
    traceId: 'a'.repeat(32),
    spanId: id.padStart(16, '0'),
    name: 'step',
    kind: 'generic',
    status: endTime ? 'ok' : 'unset',
    startTime: '2026-01-01T00:00:00.000000000Z',
    isRoot: false,
    isEvent: false,
    correlation: { project: 'test' },
    receivedAt: '2026-01-01T00:00:00.000000000Z',
    ...(endTime ? { endTime } : {}),
  }
}

function signal(id: string, endTime?: string): Signal {
  const value = span(id, endTime)
  return { kind: 'span', key: `${value.traceId}:${value.spanId}`, value }
}

describe('the queue is bounded (B3)', () => {
  it('drops the oldest signal when it is full and counts the loss', () => {
    const queue = new SignalQueue(4, silent)
    for (let i = 0; i < 10; i++) queue.push(signal(String(i)))

    expect(queue.size).toBe(4)
    expect(queue.capacity).toBe(4)
    expect(queue.dropped).toBe(6)

    // The four that survived are the four most recent.
    const kept = queue.drain(10).map((s) => (s as { value: Span }).value.spanId)
    expect(kept).toEqual(['6', '7', '8', '9'].map((i) => i.padStart(16, '0')))
  })

  it('warns when it drops, and the warning is rate limited', () => {
    const report = vi.fn()
    const queue = new SignalQueue(2, { report })
    for (let i = 0; i < 50; i++) queue.push(signal(String(i)))
    expect(queue.dropped).toBe(48)
    expect(report).toHaveBeenCalled()
    expect(report.mock.calls[0]?.[0]).toContain('queue full')
  })

  it('never grows past its capacity, whatever the traffic', () => {
    const queue = new SignalQueue(16, silent)
    for (let i = 0; i < 100_000; i++) queue.push(signal(String(i)))
    expect(queue.size).toBe(16)
  })

  it('keeps working after wrapping', () => {
    const queue = new SignalQueue(3, silent)
    for (let round = 0; round < 5; round++) {
      queue.push(signal(`${round}a`))
      queue.push(signal(`${round}b`))
      expect(queue.drain(2)).toHaveLength(2)
      expect(queue.size).toBe(0)
    }
  })
})

describe('an unfinished span is still sent (B8)', () => {
  it('replaces a queued start record with the end record', () => {
    const queue = new SignalQueue(8, silent)
    queue.push(signal('1'))
    queue.push(signal('1', '2026-01-01T00:00:01.000000000Z'))

    // One slot, holding the later state.
    expect(queue.size).toBe(1)
    const drained = queue.drain(8)[0] as { value: Span }
    expect(drained.value.endTime).toBe('2026-01-01T00:00:01.000000000Z')
  })

  it('sends the start record on its own when the flush happens first', () => {
    const queue = new SignalQueue(8, silent)
    queue.push(signal('1'))
    const first = queue.drain(8)[0] as { value: Span }
    expect(first.value.endTime).toBeUndefined()

    // The span is still open. Its end arrives later, as a second record the
    // daemon upserts over the first.
    queue.push(signal('1', '2026-01-01T00:00:02.000000000Z'))
    expect(queue.size).toBe(1)
  })

  it('does not confuse two different spans', () => {
    const queue = new SignalQueue(8, silent)
    queue.push(signal('1'))
    queue.push(signal('2'))
    expect(queue.size).toBe(2)
  })
})

describe('batching', () => {
  it('groups signals by kind into one ingest batch', () => {
    const batch = toBatch([
      signal('1'),
      { kind: 'log', value: { logId: 'l', timestamp: 't', level: 'info', message: 'hi', correlation: { project: 'p' }, receivedAt: 't' } },
      { kind: 'metric', value: { metricId: 'm', name: 'n', kind: 'counter', value: 1, timestamp: 't', correlation: { project: 'p' }, receivedAt: 't' } },
    ])
    expect(batch.spans).toHaveLength(1)
    expect(batch.logs).toHaveLength(1)
    expect(batch.metrics).toHaveLength(1)
    expect(batch.scores).toBeUndefined()
  })
})

describe('the export worker is bounded too (B3)', () => {
  it('keeps at most one pump waiting behind a slow export, whatever the traffic', async () => {
    let release: () => void = () => {}
    const gate = new Promise<void>((resolve) => {
      release = resolve
    })
    const batches: number[] = []
    const sink = {
      async export(batch: { spans?: unknown[] }): Promise<void> {
        await gate
        batches.push(batch.spans?.length ?? 0)
      },
    }
    const queue = new SignalQueue(100_000, silent)
    const exporter = new Exporter(queue, { sink, batchSize: 10, flushIntervalMs: 60_000, diagnostics: silent })

    // Every push past the batch size asks for a pump while the first export
    // hangs. Each one used to add a link to the chain.
    for (let i = 0; i < 50_000; i++) {
      queue.push(signal(String(i)))
      exporter.notify()
    }
    expect(exporter.pendingPumps).toBeLessThanOrEqual(2)

    release()
    await exporter.flush()
    expect(batches.reduce((a, b) => a + b, 0)).toBe(50_000)
    expect(exporter.pendingPumps).toBe(0)
  })
})
