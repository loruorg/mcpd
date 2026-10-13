/**
 * A client for your own unit tests (SDK contract 2.5).
 *
 * ```ts
 * import { TestClient } from '@mcpd/sdk/testing'
 *
 * const mcpd = new TestClient()
 * await answerQuestion(mcpd)
 * const call = await mcpd.span('model_call')
 * expect(call.usage?.outputTokens).toBe(37)
 * ```
 *
 * It is a real `Mcpd` whose sink keeps every batch in memory, so what you read
 * back went through sampling, payload capture, redaction and the queue exactly
 * as it would on the way to a daemon. Nothing leaves the process.
 */

import { Mcpd } from './client.js'
import type { McpdOptions } from './client.js'
import type { Feedback, LogRecord, MetricPoint, Score, Span, SpanKind } from './generated.js'
import { RecordingSink } from './sinks.js'

/** Every client option except the sink, which is the point of this client. */
export type TestClientOptions = Omit<McpdOptions, 'sink'>

/** One finished span and the finished spans under it, by start time. */
export interface SpanTree {
  span: Span
  children: SpanTree[]
}

/** What `spans()` can filter on. */
export interface SpanFilter {
  name?: string
  kind?: SpanKind
}

/**
 * An `Mcpd` that records into memory. Pass it wherever your code takes a
 * client, then read back what it recorded. Every accessor flushes first.
 */
export class TestClient extends Mcpd {
  /** The sink underneath, for a test that wants the raw batches. */
  readonly recorder: RecordingSink

  constructor(options: TestClientOptions = {}) {
    const recorder = new RecordingSink()
    // `enabled` and `sample` default to on whatever the environment says: a CI
    // job that turns mcpd off must not turn the test double off with it.
    super({
      flushOnExit: false,
      ...options,
      enabled: options.enabled ?? true,
      sample: options.sample ?? 1,
      sink: recorder,
    })
    this.recorder = recorder
  }

  /** The finished spans, one per span id, in the order each id first arrived. */
  async spans(filter: SpanFilter = {}): Promise<Span[]> {
    await this.flush()
    return finished(this.recorder.spans).filter(
      (s) => (filter.name === undefined || s.name === filter.name) && (filter.kind === undefined || s.kind === filter.kind),
    )
  }

  /** The one finished span called `name`. Throws when there is none, or more than one. */
  async span(name: string): Promise<Span> {
    const found = await this.spans({ name })
    if (found.length !== 1) {
      const seen = [...new Set((await this.spans()).map((s) => s.name))].join(', ') || 'none'
      throw new Error(`expected one finished span named "${name}", found ${found.length}; spans recorded: ${seen}`)
    }
    return found[0] as Span
  }

  async logs(): Promise<LogRecord[]> {
    await this.flush()
    return [...this.recorder.logs]
  }

  async metrics(): Promise<MetricPoint[]> {
    await this.flush()
    return [...this.recorder.metrics]
  }

  async scores(): Promise<Score[]> {
    await this.flush()
    return [...this.recorder.scores]
  }

  async recordedFeedback(): Promise<Feedback[]> {
    await this.flush()
    return [...this.recorder.feedback]
  }

  /** Every trace as a tree. A span whose parent was not recorded is a root. */
  async trees(): Promise<SpanTree[]> {
    const spans = await this.spans()
    const nodes = new Map<string, SpanTree>(spans.map((s) => [s.spanId, { span: s, children: [] }]))
    const roots: SpanTree[] = []
    for (const node of nodes.values()) {
      const parent = node.span.parentSpanId !== undefined ? nodes.get(node.span.parentSpanId) : undefined
      if (parent !== undefined && parent !== node) parent.children.push(node)
      else roots.push(node)
    }
    const order = (list: SpanTree[]): void => {
      list.sort((a, b) => (a.span.startTime < b.span.startTime ? -1 : a.span.startTime > b.span.startTime ? 1 : 0))
      for (const n of list) order(n.children)
    }
    order(roots)
    return roots
  }

  /** Forget everything recorded so far. */
  async reset(): Promise<void> {
    await this.flush()
    this.recorder.clear()
  }
}

/** The last record per span id, in first-arrival order, ended spans only. */
function finished(records: readonly Span[]): Span[] {
  // Setting a key a Map already holds keeps its position, which is the order
  // the contract asks for.
  const last = new Map<string, Span>()
  for (const s of records) last.set(s.spanId, s)
  return [...last.values()].filter((s) => s.endTime !== undefined)
}
