import { describe, expect, it } from 'vitest'
import {
  formatTraceparent,
  isSpanId,
  isTraceId,
  newSpanId,
  newTraceId,
  parseTraceparent,
} from '../src/index.js'
import { formatTimestamp, nowNs } from '../src/context.js'

describe('identifiers (B6)', () => {
  it('are OpenTelemetry-shaped', () => {
    for (let i = 0; i < 200; i++) {
      const traceId = newTraceId()
      const spanId = newSpanId()
      expect(traceId).toMatch(/^[0-9a-f]{32}$/)
      expect(spanId).toMatch(/^[0-9a-f]{16}$/)
      expect(isTraceId(traceId)).toBe(true)
      expect(isSpanId(spanId)).toBe(true)
    }
  })

  it('are unique', () => {
    const seen = new Set<string>()
    for (let i = 0; i < 1000; i++) seen.add(newTraceId())
    expect(seen.size).toBe(1000)
  })

  it('reject the reserved all-zero value and the wrong shapes', () => {
    expect(isTraceId('0'.repeat(32))).toBe(false)
    expect(isSpanId('0'.repeat(16))).toBe(false)
    expect(isTraceId('ABCDEF01234567890123456789ABCDEF')).toBe(false)
    expect(isTraceId(newSpanId())).toBe(false)
    expect(isSpanId(newTraceId())).toBe(false)
    expect(isTraceId(undefined)).toBe(false)
  })
})

describe('W3C traceparent (B7)', () => {
  it('round-trips', () => {
    const context = { traceId: newTraceId(), spanId: newSpanId(), sampled: true }
    const header = formatTraceparent(context)
    expect(header).toBe(`00-${context.traceId}-${context.spanId}-01`)
    expect(parseTraceparent(header)).toEqual(context)
  })

  it('carries the sampling flag in both directions', () => {
    const dropped = { traceId: newTraceId(), spanId: newSpanId(), sampled: false }
    expect(formatTraceparent(dropped)).toMatch(/-00$/)
    expect(parseTraceparent(formatTraceparent(dropped))?.sampled).toBe(false)
  })

  it('parses a real header from another service', () => {
    const parsed = parseTraceparent(
      '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01',
    )
    expect(parsed).toEqual({
      traceId: '4bf92f3577b34da6a3ce929d0e0e4736',
      spanId: '00f067aa0ba902b7',
      sampled: true,
    })
  })

  it('returns undefined for anything malformed rather than throwing', () => {
    for (const bad of [
      undefined,
      null,
      '',
      'not-a-header',
      '00-tooshort-00f067aa0ba902b7-01',
      `ff-${'a'.repeat(32)}-${'b'.repeat(16)}-01`,
      `00-${'0'.repeat(32)}-${'b'.repeat(16)}-01`,
      `00-${'a'.repeat(32)}-${'b'.repeat(16)}`,
    ]) {
      expect(parseTraceparent(bad as string)).toBeUndefined()
    }
  })

  it('accepts a future version that keeps the first four fields', () => {
    const header = `01-${'a'.repeat(32)}-${'b'.repeat(16)}-01-extra`
    expect(parseTraceparent(header)?.sampled).toBe(true)
  })
})

describe('timestamps (B9)', () => {
  it('are UTC RFC 3339 with nine fractional digits', () => {
    const stamp = formatTimestamp(nowNs())
    expect(stamp).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{9}Z$/)
  })

  it('keep sub-millisecond detail', () => {
    // 1970-01-01T00:00:00.000000123Z. A millisecond clock would lose the 123.
    expect(formatTimestamp(123n)).toBe('1970-01-01T00:00:00.000000123Z')
  })

  it('move forward', () => {
    const first = nowNs()
    let second = nowNs()
    while (second === first) second = nowNs()
    expect(second > first).toBe(true)
  })
})
