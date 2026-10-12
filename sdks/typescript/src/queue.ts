/**
 * The buffer between an agent and the network.
 *
 * Recording a signal appends to this queue and returns (B2). A background
 * worker turns the queue into batches and posts them. The queue is bounded, and
 * when it is full the oldest signal is dropped (B3): an unbounded buffer turns
 * a network outage into an out-of-memory kill, which is a far worse failure
 * than a gap in a chart.
 */

import type { Feedback, IngestBatch, LogRecord, MetricPoint, Score, Span } from './generated.js'
import type { Sink } from './sinks.js'
import type { Diagnostics } from './transport.js'
// A 128-bit random value, reused here as a batch idempotency key.
import { newTraceId as randomKey } from './context.js'

/** One queued item. The `key` on a span is what lets a later state replace it. */
export type Signal =
  | { kind: 'span'; key: string; value: Span }
  | { kind: 'log'; value: LogRecord }
  | { kind: 'metric'; value: MetricPoint }
  | { kind: 'score'; value: Score }
  | { kind: 'feedback'; value: Feedback }

/** What the queue has done since the client was created. */
export interface QueueStats {
  /** Signals waiting to be sent. */
  queued: number
  /** Signals dropped because the queue was full. */
  dropped: number
  /** How many the queue can hold. */
  capacity: number
}

/**
 * A fixed-capacity ring of signals.
 *
 * Spans are additionally indexed by id. A span is enqueued when it starts and
 * again when it ends (B8); if the start record has not been sent yet, the end
 * record replaces it in place. Long-running spans therefore still reach the
 * daemon while they are open, and short ones cost exactly one record.
 */
export class SignalQueue {
  readonly #ring: (Signal | undefined)[]
  readonly #spans = new Map<string, { kind: 'span'; key: string; value: Span }>()
  readonly #diagnostics: Diagnostics
  #head = 0
  #count = 0
  #dropped = 0

  constructor(capacity: number, diagnostics: Diagnostics) {
    this.#ring = new Array<Signal | undefined>(Math.max(1, capacity))
    this.#diagnostics = diagnostics
  }

  get size(): number {
    return this.#count
  }

  get capacity(): number {
    return this.#ring.length
  }

  get dropped(): number {
    return this.#dropped
  }

  stats(): QueueStats {
    return { queued: this.#count, dropped: this.#dropped, capacity: this.#ring.length }
  }

  push(signal: Signal): void {
    if (signal.kind === 'span') {
      const pending = this.#spans.get(signal.key)
      if (pending) {
        // Still queued: fold the new state into the record already holding a
        // slot rather than spending a second one.
        pending.value = signal.value
        return
      }
    }

    if (this.#count === this.#ring.length) {
      const evicted = this.#ring[this.#head]
      if (evicted?.kind === 'span') this.#spans.delete(evicted.key)
      this.#ring[this.#head] = undefined
      this.#head = (this.#head + 1) % this.#ring.length
      this.#count--
      this.#dropped++
      this.#diagnostics.report(
        `queue full at ${this.#ring.length} signals, dropping oldest`,
        undefined,
      )
    }

    const slot = (this.#head + this.#count) % this.#ring.length
    this.#ring[slot] = signal
    this.#count++
    if (signal.kind === 'span') this.#spans.set(signal.key, signal)
  }

  /** Remove and return up to `max` signals, oldest first. */
  drain(max: number): Signal[] {
    const take = Math.min(max, this.#count)
    const out: Signal[] = []
    for (let i = 0; i < take; i++) {
      const signal = this.#ring[this.#head] as Signal
      this.#ring[this.#head] = undefined
      this.#head = (this.#head + 1) % this.#ring.length
      this.#count--
      if (signal.kind === 'span') this.#spans.delete(signal.key)
      out.push(signal)
    }
    return out
  }
}

/** Group drained signals back into the shape the ingest endpoint accepts. */
export function toBatch(signals: Signal[]): IngestBatch {
  const batch: IngestBatch = {}
  for (const signal of signals) {
    switch (signal.kind) {
      case 'span':
        ;(batch.spans ??= []).push(signal.value)
        break
      case 'log':
        ;(batch.logs ??= []).push(signal.value)
        break
      case 'metric':
        ;(batch.metrics ??= []).push(signal.value)
        break
      case 'score':
        ;(batch.scores ??= []).push(signal.value)
        break
      case 'feedback':
        ;(batch.feedback ??= []).push(signal.value)
        break
    }
  }
  return batch
}

/**
 * How long `flush()` and `shutdown()` wait by default (contract section 2.4).
 *
 * A cap on batches is not a deadline: a daemon that accepts the connection and
 * never answers costs each batch three attempts of the full request timeout,
 * which at the defaults is more than half an hour for a full queue.
 */
export const DEFAULT_DEADLINE_MS = 10_000

/** Resolve `true` when `work` settles in time, `false` when the deadline wins. */
function withDeadline(work: Promise<void>, timeoutMs: number): Promise<boolean> {
  let timer: ReturnType<typeof setTimeout> | undefined
  const expired = new Promise<boolean>((resolve) => {
    timer = setTimeout(() => resolve(false), Math.max(0, timeoutMs))
  })
  return Promise.race([work.then(() => true), expired]).finally(() => clearTimeout(timer))
}

export interface ExporterConfig {
  /** Where a drained batch goes. The HTTP transport, unless the client was given another. */
  sink: Sink
  batchSize: number
  flushIntervalMs: number
  diagnostics: Diagnostics
}

/**
 * The background worker.
 *
 * One export is in flight at a time, chained through a promise, so that a slow
 * daemon produces a queue rather than a pile of concurrent requests. Nothing
 * here ever rejects: a failed export is reported and the batch is let go (B1).
 * The sink runs inside that same guard, so a sink of the user's own that throws
 * is a failed batch and a diagnostic, never an exception in the caller.
 */
export class Exporter {
  readonly #queue: SignalQueue
  readonly #config: ExporterConfig
  #timer: ReturnType<typeof setInterval> | undefined
  #chain: Promise<void> = Promise.resolve()
  // The non-draining pump that is queued behind the running one and has not
  // started yet. Every push at or above the batch size asks for a pump, and
  // while the daemon is slow each of those used to add a link to the chain, so
  // the chain grew with the traffic instead of staying bounded (B3). One such
  // pump is enough: when it starts, it sends every full batch then queued.
  #waiting: Promise<void> | undefined
  #links = 0
  #stopped = false
  // Set when a shutdown's deadline passed. The worker starts no further batch.
  #abandoned = false
  #shutdown: Promise<boolean> | undefined
  #exported = 0
  #failed = 0

  constructor(queue: SignalQueue, config: ExporterConfig) {
    this.#queue = queue
    this.#config = config
  }

  /** Signals the sink accepted, and batches it refused. */
  stats(): { exported: number; failed: number } {
    return { exported: this.#exported, failed: this.#failed }
  }

  /** @internal Pumps queued or running, for the test that bounds them. */
  get pendingPumps(): number {
    return this.#links
  }

  /** Begin the periodic flush. Safe to call more than once. */
  start(): void {
    if (this.#timer !== undefined || this.#stopped) return
    const timer = setInterval(() => {
      void this.#pump(false)
    }, this.#config.flushIntervalMs)
    // An observability timer must not be the reason a CLI refuses to exit.
    ;(timer as unknown as { unref?: () => void }).unref?.()
    this.#timer = timer
  }

  /** Called after every push. A full batch goes now instead of at the next tick. */
  notify(): void {
    if (this.#stopped) return
    if (this.#queue.size >= this.#config.batchSize) void this.#pump(false)
  }

  /**
   * Send everything queued at the moment of the call (B5), waiting at most
   * `timeoutMs`. Resolves `false` when the deadline passed first; what was not
   * sent stays queued for the worker.
   *
   * After `shutdown()` this waits for the shutdown to finish and sends nothing
   * more. A stopped client stays stopped; build a new one to record again.
   */
  flush(timeoutMs: number = DEFAULT_DEADLINE_MS): Promise<boolean> {
    if (this.#stopped) return this.#shutdown ?? Promise.resolve(true)
    return withDeadline(this.#pump(true), timeoutMs)
  }

  /**
   * Flush, stop the worker, and stay stopped (B5), within `timeoutMs`. When
   * the deadline passes the worker starts no further batch, and what is still
   * queued is given up.
   *
   * Idempotent: the second call returns the promise the first one created, so
   * two shutdown paths racing at process exit cannot double-send.
   */
  shutdown(timeoutMs: number = DEFAULT_DEADLINE_MS): Promise<boolean> {
    this.#shutdown ??= (async () => {
      this.#stopped = true
      if (this.#timer !== undefined) {
        clearInterval(this.#timer)
        this.#timer = undefined
      }
      const finished = await withDeadline(this.#pump(true), timeoutMs)
      if (!finished) this.#abandoned = true
      return finished
    })()
    return this.#shutdown
  }

  #pump(drainAll: boolean): Promise<void> {
    if (!drainAll && this.#waiting !== undefined) return this.#waiting
    // A flush fixes its bound before draining starts, so a producer that keeps
    // writing during a flush cannot hold the flush open forever.
    const drainBound = Math.ceil(this.#queue.size / this.#config.batchSize) + 1
    this.#links++
    const link = this.#chain
      .then(async () => {
        if (!drainAll) this.#waiting = undefined
        // A pump that is not a flush sends the full batches queued when it
        // starts, or one partial batch on a timer tick.
        const bound = drainAll
          ? drainBound
          : Math.max(1, Math.floor(this.#queue.size / this.#config.batchSize))
        for (let i = 0; i < bound; i++) {
          if (this.#abandoned || this.#queue.size === 0) return
          const signals = this.#queue.drain(this.#config.batchSize)
          if (signals.length === 0) return
          await this.#send(signals)
        }
      })
      // The chain must never settle rejected: one poisoned link would make
      // every later flush reject, and `flush()` is not allowed to throw.
      .catch((error: unknown) => {
        this.#config.diagnostics.report('the export worker raised', error)
      })
      .finally(() => {
        this.#links--
      })
    this.#chain = link
    if (!drainAll) this.#waiting = link
    return link
  }

  async #send(signals: Signal[]): Promise<void> {
    // Stable for the life of this batch, including across the transport's own
    // retries, so a timeout after the daemon stored the batch cannot
    // double-count it.
    const ingestKey = randomKey()
    try {
      await this.#config.sink.export(toBatch(signals), ingestKey)
      this.#exported += signals.length
    } catch (error) {
      // The batch is gone. Re-queueing it would let a persistent outage fill
      // the queue with signals that can never leave, evicting fresher ones.
      this.#failed++
      this.#config.diagnostics.report('could not export a batch of signals', error)
    }
  }
}
