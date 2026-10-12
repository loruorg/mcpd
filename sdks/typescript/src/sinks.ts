/**
 * Where a drained batch goes.
 *
 * The exporter turns the queue into an `IngestBatch` and hands it to the
 * client's sink. The HTTP transport is the default sink and the only one that
 * talks to a daemon; the two here keep the batch in the process, for a test
 * that asserts what was recorded and for a dump while debugging. A sink runs
 * on the exporter's worker inside the same guard as the transport: one that
 * throws is counted as a failed batch and reported, never raised (B1).
 */

import type { Feedback, IngestBatch, LogRecord, MetricPoint, Score, Span } from './generated.js'
import { toWire } from './transport.js'

/** Takes one batch, exactly as the daemon would have received it. */
export interface Sink {
  /**
   * Export one batch. `ingestKey` is stable for the batch across retries, so a
   * sink that forwards to something idempotent can pass it along.
   */
  export(batch: IngestBatch, ingestKey: string): Promise<void> | void
}

/**
 * Keeps every batch in memory.
 *
 * Construct the client with `sink: new RecordingSink()`, run the code under
 * test, `flush()`, and read the signals back. No server, no port, no network.
 */
export class RecordingSink implements Sink {
  /** Every span seen, in arrival order. A span queued twice arrives twice. */
  readonly spans: Span[] = []
  readonly logs: LogRecord[] = []
  readonly metrics: MetricPoint[] = []
  readonly scores: Score[] = []
  readonly feedback: Feedback[] = []
  /** How many batches have been exported. */
  batches = 0

  export(batch: IngestBatch): void {
    this.batches++
    if (batch.spans) this.spans.push(...batch.spans)
    if (batch.logs) this.logs.push(...batch.logs)
    if (batch.metrics) this.metrics.push(...batch.metrics)
    if (batch.scores) this.scores.push(...batch.scores)
    if (batch.feedback) this.feedback.push(...batch.feedback)
  }

  /** Forget everything seen so far. */
  clear(): void {
    this.spans.length = 0
    this.logs.length = 0
    this.metrics.length = 0
    this.scores.length = 0
    this.feedback.length = 0
    this.batches = 0
  }
}

/** What `ConsoleSink` accepts. */
export interface ConsoleSinkOptions {
  /** Where each line goes. Defaults to standard error. */
  write?: (line: string) => void
}

/**
 * Writes one JSON line per signal, shaped `{"signal":"span",...record}` with
 * the field names the daemon uses, to standard error by default.
 */
export class ConsoleSink implements Sink {
  readonly #write: (line: string) => void

  constructor(options: ConsoleSinkOptions = {}) {
    this.#write = options.write ?? defaultWrite
  }

  export(batch: IngestBatch): void {
    for (const span of batch.spans ?? []) this.#line('span', span)
    for (const log of batch.logs ?? []) this.#line('log', log)
    for (const metric of batch.metrics ?? []) this.#line('metric', metric)
    for (const score of batch.scores ?? []) this.#line('score', score)
    for (const feedback of batch.feedback ?? []) this.#line('feedback', feedback)
  }

  #line(signal: string, record: unknown): void {
    const wire = toWire(record) as Record<string, unknown>
    this.#write(JSON.stringify({ signal, ...wire }))
  }
}

function defaultWrite(line: string): void {
  const process = (globalThis as {
    process?: { stderr?: { write?: (chunk: string) => unknown } }
  }).process
  if (typeof process?.stderr?.write === 'function') {
    process.stderr.write(line + '\n')
    return
  }
  console.error(line)
}
