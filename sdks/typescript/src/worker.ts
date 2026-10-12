/**
 * The worker: the user's own process, serving step bodies to the daemon.
 *
 * It registers the workflows it holds, long-polls for leases, and for each
 * lease posts `started`, heartbeats while the body runs, and posts `complete`
 * or `fail`. Every 409 the daemon can answer is handled by its reason, because
 * a worker that collapses them into one branch abandons steps it has already
 * finished or reports a failure for a success.
 *
 * Nothing here runs a model or reads a stored prompt. The daemon hands the
 * worker a lease with the rendered input and the prior steps; the body does
 * the rest with the imports, the pool and the credentials this process holds.
 */

import type { Mcpd } from './client.js'
import type { SpanContext } from './context.js'
import { contextReady } from './context.js'
import type {
  ConflictReason,
  Correlation,
  StepErrorClass,
  StepLease,
  StepUsage,
} from './generated.js'
import { StepConflictError } from './runs.js'
import type { SpanOptions, UsageReport } from './span.js'
import { McpdSpan, runInSpan } from './span.js'
import { McpdError } from './transport.js'
import type { Prior, StepHandler, Workflow } from './workflow.js'

/** A failure with a class, thrown from a step body to tell the daemon how to treat it. */
export class StepFailure extends Error {
  readonly class: StepErrorClass
  readonly retryAfterMs: number | undefined
  readonly details: unknown

  constructor(
    message: string,
    options: { class?: StepErrorClass; retryAfterMs?: number; details?: unknown } = {},
  ) {
    super(message)
    this.name = 'StepFailure'
    this.class = options.class ?? 'fatal'
    this.retryAfterMs = options.retryAfterMs
    this.details = options.details
  }
}

/** A transient failure: the daemon retries it while the step's budget lasts. */
export class RetryableError extends StepFailure {
  constructor(message: string, options: { retryAfterMs?: number; details?: unknown } = {}) {
    super(message, { ...options, class: 'retryable' })
    this.name = 'RetryableError'
  }
}

/**
 * What a step body is handed besides its input and the prior steps.
 *
 * `signal` aborts when the daemon fences this lease or ends the run, so a body
 * that hands it to its own I/O stops when it should. `withSpan` records a span
 * under the step the daemon opened, so a model call inside the body lands in
 * the run's trace at the right depth.
 */
export class StepContext {
  /** The lease, in full. */
  readonly lease: StepLease
  /** Which run. */
  readonly runId: string
  /** Which step, with any loop or item index folded in. */
  readonly stepKey: string
  /** Which attempt, from zero. */
  readonly attempt: number
  /** A key for a third party that accepts one. Stable across retries of this step. */
  readonly idempotencyKey: string
  /** Aborted when the lease is lost or the run ends. */
  readonly signal: AbortSignal
  /** The step's span, which the daemon opened; record your own spans under it. */
  readonly parent: SpanContext
  readonly #mcpd: Mcpd
  #state: Record<string, unknown> | undefined
  #usage: StepUsage | undefined
  #accumulated: StepUsage | undefined

  constructor(mcpd: Mcpd, lease: StepLease, signal: AbortSignal) {
    this.#mcpd = mcpd
    this.lease = lease
    this.runId = lease.runId
    this.stepKey = lease.stepKey
    this.attempt = lease.attempt
    this.idempotencyKey = lease.idempotencyKey
    this.signal = signal
    this.parent = { traceId: lease.traceId, spanId: lease.spanId, sampled: true }
  }

  /** The run's accumulated state as of this lease. */
  get state(): unknown {
    return this.lease.state
  }

  /**
   * The step's rendered input, checked by `decode`. A decoder that throws
   * fails the step `invalid_input`, never `fatal`: the input was rendered by
   * the daemon from a template, so a shape the body did not expect is a
   * template mistake, and one attempt is the right thing to charge for it.
   */
  input<T>(decode: (value: unknown) => T): T {
    return decodeOr(this.lease.input, decode, 'input')
  }

  /** The prior steps, keyed by id, checked by `decode`. Same rule as `input`. */
  prior<T>(decode: (value: unknown) => T): T {
    return decodeOr(this.lease.prior ?? {}, decode, 'prior')
  }

  /** The run's state, checked by `decode`. Same rule as `input`. */
  stateAs<T>(decode: (value: unknown) => T): T {
    return decodeOr(this.lease.state, decode, 'state')
  }

  /** Merge keys into the run's state when the step completes. Top-level keys replace. */
  setState(patch: Record<string, unknown>): void {
    this.#state = { ...(this.#state ?? {}), ...patch }
  }

  /**
   * Report the tokens this step spent, so the daemon prices and charges them.
   *
   * An explicit override: the fields set here replace what the step's spans
   * accumulated, so a body that records no spans reports what it sets.
   */
  setUsage(usage: StepUsage): void {
    this.#usage = { ...(this.#usage ?? {}), ...usage }
  }

  /**
   * Run `fn` inside a span nested under this step in the run's trace, carrying
   * the run's correlation: session, thread, user, request and release.
   *
   * The span, and every span nested under it, adds its usage to the step's
   * when it ends, so a body that records its model calls as spans never sums
   * tokens by hand.
   */
  async withSpan<T>(
    name: string,
    fn: (span: McpdSpan) => T | Promise<T>,
    options: SpanOptions = {},
  ): Promise<T> {
    const c = this.lease.correlation
    const correlation: Partial<Correlation> = {}
    if (c.sessionId !== undefined) correlation.sessionId = c.sessionId
    if (c.threadId !== undefined) correlation.threadId = c.threadId
    if (c.userId !== undefined) correlation.userId = c.userId
    if (c.requestId !== undefined) correlation.requestId = c.requestId
    if (c.release !== undefined) correlation.release = c.release
    // A key named with no value is not an instruction to erase it. Spreading
    // `options.correlation` would copy an explicit `undefined` over the run's
    // own value, and a step span that lost its session cannot be found by
    // session, by the trace filters or by a subject erasure, along with every
    // span under it.
    for (const key of Object.keys(options.correlation ?? {}) as Array<keyof Correlation>) {
      const value = options.correlation?.[key]
      if (value !== undefined && value !== null) correlation[key] = value
    }
    await contextReady()
    const span = this.#mcpd.startSpan(name, {
      ...options,
      correlation,
      parent: this.parent,
    })
    McpdSpan.adoptUsageSink(span, (report) => this.#accumulate(report))
    return await this.#mcpd.withActiveSpan(span, () => runInSpan(span, fn))
  }

  /**
   * What the body accumulated, read by the worker when it completes: the
   * state patch, and the spans' usage with the explicit override on top.
   */
  collected(): { state: Record<string, unknown> | undefined; usage: StepUsage | undefined } {
    let usage: StepUsage | undefined
    if (this.#accumulated !== undefined || this.#usage !== undefined) {
      usage = { ...(this.#accumulated ?? {}), ...(this.#usage ?? {}) }
    }
    return { state: this.#state, usage }
  }

  /** Sum a span's usage into the step's: tokens add, the last model seen wins. */
  #accumulate(report: UsageReport): void {
    const acc = this.#accumulated ?? {}
    const next: StepUsage = { ...acc }
    const inputTokens = sum(acc.inputTokens, report.usage.inputTokens)
    const outputTokens = sum(acc.outputTokens, report.usage.outputTokens)
    const totalTokens = sum(acc.totalTokens, report.usage.totalTokens)
    if (inputTokens !== undefined) next.inputTokens = inputTokens
    if (outputTokens !== undefined) next.outputTokens = outputTokens
    if (totalTokens !== undefined) next.totalTokens = totalTokens
    if (report.model !== undefined) next.model = report.model
    if (report.provider !== undefined) next.provider = report.provider
    this.#accumulated = next
  }
}

/** Add two optional counts. Two absences stay absent, so a field nobody set is not sent as zero. */
function sum(a: number | undefined, b: number | undefined): number | undefined {
  if (a === undefined && b === undefined) return undefined
  return (a ?? 0) + (b ?? 0)
}

function decodeOr<T>(value: unknown, decode: (value: unknown) => T, what: string): T {
  try {
    return decode(value)
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error)
    throw new StepFailure(`the step's ${what} could not be read: ${reason}`, { class: 'invalid_input' })
  }
}

/** How a step body finished, as `onStepEnd` sees it. */
export type StepOutcome =
  | { status: 'completed' }
  | { status: 'failed'; class: StepErrorClass; message: string }

/** What the worker has done since it was built. */
export interface WorkerStats {
  /** Leases taken by `claim`. */
  claimed: number
  /** Bodies running right now. */
  inflight: number
  /** Bodies that returned. */
  completed: number
  /** Bodies that failed, by error class. */
  failed: Record<string, number>
  /** 409s applied, by the daemon's reason. */
  conflicts: Record<string, number>
  /** Leases refused before the body because this worker does not serve them. */
  unservable: number
}

/** What `new Worker` takes. */
export interface WorkerOptions {
  /** A name for this process, recorded as the owner of every lease it takes. */
  workerId?: string
  /** How many steps to run at once. */
  concurrency?: number
  /** Which project to serve. Needed only when the key reaches several. */
  project?: string
  /** How long each claim waits for work, in milliseconds. */
  waitMs?: number
  /** Where the worker reports its own failures. Defaults to `console.error`. */
  onError?: (message: string, error: unknown) => void
  /** A lease was claimed, before anything is posted for it. */
  onLease?: (lease: StepLease) => unknown
  /** `started` was accepted and the body is about to run. */
  onStepStart?: (ctx: StepContext) => unknown
  /** The body finished: completed, or failed with a class. */
  onStepEnd?: (ctx: StepContext, outcome: StepOutcome) => unknown
  /** A 409 was applied, with the daemon's reason. */
  onConflict?: (lease: StepLease, reason: ConflictReason) => unknown
}

/**
 * Serves step bodies from this process. See the module comment for the
 * protocol it drives.
 */
export class Worker {
  readonly #mcpd: Mcpd
  readonly #workerId: string
  readonly #concurrency: number
  readonly #project: string | undefined
  readonly #waitMs: number
  readonly #userOnError: ((message: string, error: unknown) => void) | undefined
  readonly #logged = new Map<string, number>()
  readonly #hooks: Pick<WorkerOptions, 'onLease' | 'onStepStart' | 'onStepEnd' | 'onConflict'>
  readonly #workflows: Workflow[] = []
  readonly #inflight = new Set<Promise<void>>()
  readonly #counts = {
    claimed: 0,
    completed: 0,
    failed: {} as Record<string, number>,
    conflicts: {} as Record<string, number>,
    unservable: 0,
  }
  #heartbeatMs = 10_000
  #stopping = false
  #wake: (() => void) | undefined

  constructor(mcpd: Mcpd, options: WorkerOptions = {}) {
    this.#mcpd = mcpd
    this.#workerId = options.workerId ?? defaultWorkerId()
    this.#concurrency = Math.max(1, options.concurrency ?? 4)
    this.#project = options.project
    this.#waitMs = Math.min(60_000, Math.max(0, options.waitMs ?? 20_000))
    this.#userOnError = options.onError
    this.#hooks = {
      onLease: options.onLease,
      onStepStart: options.onStepStart,
      onStepEnd: options.onStepEnd,
      onConflict: options.onConflict,
    }
  }

  /** The workflows this worker serves. Their definitions are posted by `serve`. */
  register(...workflows: Workflow[]): this {
    this.#workflows.push(...workflows)
    return this
  }

  /** The name every lease records as its owner. */
  get workerId(): string {
    return this.#workerId
  }

  /** Steps running right now. */
  get inflight(): number {
    return this.#inflight.size
  }

  /**
   * Claims, completions, failures by class and conflicts by reason. A fenced
   * lease abandoned in silence looks like no work from outside; this is where
   * it shows.
   */
  stats(): WorkerStats {
    return {
      claimed: this.#counts.claimed,
      inflight: this.#inflight.size,
      completed: this.#counts.completed,
      failed: { ...this.#counts.failed },
      conflicts: { ...this.#counts.conflicts },
      unservable: this.#counts.unservable,
    }
  }

  /**
   * Register, then claim and serve until `stop()` is called. Resolves once
   * every step in flight has reported.
   */
  async serve(): Promise<void> {
    this.#stopping = false
    await this.registerDefinitions()
    while (!this.#stopping) {
      const free = this.#concurrency - this.#inflight.size
      if (free <= 0) {
        await Promise.race(this.#inflight)
        continue
      }
      try {
        const claimed = await this.claim(free)
        if (claimed === 0 && !this.#stopping) await this.#pause(0)
      } catch (error) {
        this.#onError('claim', `mcpd worker ${this.#workerId}: claim failed; retrying`, error)
        await this.#pause(1_000)
      }
    }
    await Promise.allSettled(this.#inflight)
  }

  /** Stop claiming and wait for the steps in flight. */
  async stop(): Promise<void> {
    this.#stopping = true
    this.#wake?.()
    await Promise.allSettled(this.#inflight)
  }

  /** Post every registered definition and read back the lease timings. */
  async registerDefinitions(): Promise<void> {
    const registered = await this.#mcpd.steps.register({
      workerId: this.#workerId,
      definitions: this.#workflows.map((w) => w.document()),
      project: this.#project,
    })
    this.#heartbeatMs = Math.max(1_000, registered.heartbeatMs)
  }

  /**
   * One claim, serving what it granted. Returns how many leases were taken.
   * `serve` calls this in a loop; a script that drives its own run calls it
   * with that run's id until the run ends.
   */
  async claim(max: number = this.#concurrency, runId?: string): Promise<number> {
    const response = await this.#mcpd.steps.claim({
      workerId: this.#workerId,
      max: Math.min(100, Math.max(1, max)),
      waitMs: this.#waitMs,
      project: this.#project,
      runId,
    })
    for (const lease of response.leases) {
      this.#counts.claimed++
      this.#hook('onLease', () => this.#hooks.onLease?.(lease))
      const task = this.#serve(lease).finally(() => {
        this.#inflight.delete(task)
      })
      this.#inflight.add(task)
    }
    return response.leases.length
  }

  /** Wait for every step in flight. */
  async drain(): Promise<void> {
    await Promise.allSettled(this.#inflight)
  }

  #pause(ms: number): Promise<void> {
    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        this.#wake = undefined
        resolve()
      }, ms)
      this.#wake = () => {
        clearTimeout(timer)
        this.#wake = undefined
        resolve()
      }
    })
  }

  #handlerFor(lease: StepLease): StepHandler | undefined {
    for (const w of this.#workflows) {
      if (w.id !== lease.definitionId) continue
      if (w.version !== undefined && w.version !== lease.definitionVersion) continue
      const handler = w.handlerFor(lease.stepKey)
      if (handler) return handler
    }
    return undefined
  }

  /**
   * Report one of the worker's own failures.
   *
   * A caller's `onError` sees every one. Without one, each kind of failure is
   * written at most once per error interval. The kind is the key, never the
   * message: a message names its step, and a key that did would let every
   * step of a failing run through the limit.
   */
  #onError(kind: string, message: string, error: unknown): void {
    if (this.#userOnError !== undefined) {
      this.#userOnError(message, error)
      return
    }
    const now = Date.now()
    const last = this.#logged.get(kind)
    if (last !== undefined && now - last < this.#mcpd.errorIntervalMs) return
    this.#logged.set(kind, now)
    console.error(message, error)
  }

  /**
   * Call one of the user's hooks. A hook that throws, or returns a promise
   * that rejects, is reported and forgotten: the serve loop is never the
   * hook's to break, and no hook is awaited on the completion path.
   */
  #hook(name: string, call: () => unknown): void {
    try {
      const result = call()
      if (isThenable(result)) {
        result.then(undefined, (error: unknown) => {
          this.#onError('hook', `mcpd worker ${this.#workerId}: the ${name} hook rejected`, error)
        })
      }
    } catch (error) {
      this.#onError('hook', `mcpd worker ${this.#workerId}: the ${name} hook threw`, error)
    }
  }

  #failed(ctx: StepContext, cls: StepErrorClass, message: string): void {
    this.#counts.failed[cls] = (this.#counts.failed[cls] ?? 0) + 1
    this.#hook('onStepEnd', () => this.#hooks.onStepEnd?.(ctx, { status: 'failed', class: cls, message }))
  }

  async #serve(lease: StepLease): Promise<void> {
    const steps = this.#mcpd.steps
    const handler = this.#handlerFor(lease)
    if (!handler) {
      this.#counts.unservable++
      await this.#report(lease, () =>
        steps.fail(lease.leaseId, {
          error: {
            class: 'unservable',
            message: `worker ${this.#workerId} does not serve ${lease.definitionId}@${lease.definitionVersion} step ${lease.stepKey}`,
          },
        }),
      )
      return
    }

    // Before the body. Always. This is what tells the daemon the difference
    // between "offered" and "running", and on an at_most_once step it refuses
    // a complete without it.
    const abort = new AbortController()
    const started = await this.#report(lease, () => steps.started(lease.leaseId))
    if (started === 'abandon') return

    const ctx = new StepContext(this.#mcpd, lease, abort.signal)
    this.#hook('onStepStart', () => this.#hooks.onStepStart?.(ctx))

    // The step's own ceiling, enforced here as well as by the daemon: a body
    // that runs past `deadline_ms` is cancelled through its signal and the
    // attempt is failed retryable, so the worker's slot is freed and the
    // journal says why, instead of the daemon reclaiming a lease that is
    // still heartbeating from inside a body that will never return.
    let deadline: ReturnType<typeof setTimeout> | undefined
    let deadlineReported: Promise<unknown> | undefined
    if (lease.deadlineMs !== undefined && lease.deadlineMs > 0) {
      deadline = setTimeout(() => {
        if (abort.signal.aborted) return
        const message = `step deadline of ${lease.deadlineMs}ms passed`
        clearInterval(beat)
        this.#failed(ctx, 'retryable', message)
        deadlineReported = this.#report(lease, () =>
          steps.fail(lease.leaseId, { error: { class: 'retryable', message } }),
        ).catch((error: unknown) => {
          this.#onError('call', `mcpd worker ${this.#workerId}: could not fail ${lease.stepKey}`, error)
        })
        abort.abort(new Error(message))
      }, lease.deadlineMs)
    }
    // One heartbeat at a time. A heartbeat the daemon is slow to answer would
    // otherwise be joined by the next tick's, and the next, each holding a
    // request open, for as long as the daemon stays slow.
    let beating = false
    const beat = setInterval(() => {
      if (beating) return
      beating = true
      steps
        .heartbeat(lease.leaseId)
        .catch((error: unknown) => {
          if (error instanceof StepConflictError) {
            // The daemon has moved on without us; the body learns through its signal.
            abort.abort(new Error(`${error.conflict.reason}: ${error.conflict.message}`))
            clearInterval(beat)
            this.#conflict(lease, error.conflict.reason)
            return
          }
          // A transport failure is not evidence of anything; the next beat may land.
          this.#onError('heartbeat', `mcpd worker ${this.#workerId}: heartbeat failed for ${lease.stepKey}`, error)
        })
        .finally(() => {
          beating = false
        })
    }, this.#heartbeatMs)

    try {
      let output: unknown
      try {
        // The body is raced against its own cancellation signal. A body that
        // ignores the signal keeps running, which nothing in JavaScript can
        // prevent, but the worker stops waiting for it: the slot is freed at
        // the deadline or the fence, as contract section 7.2 requires, rather
        // than whenever the body happens to return. What it returns or throws
        // after that is ignored.
        const body = (async () => await handler(ctx, lease.input, (lease.prior ?? {}) as Prior))()
        body.catch(() => undefined)
        const outcome = await Promise.race([body.then((value) => ({ value })), untilAborted(abort.signal)])
        if (outcome === undefined) {
          // The slot is held only until the daemon has heard why.
          await deadlineReported
          return
        }
        output = outcome.value
      } catch (error) {
        if (abort.signal.aborted) return
        const failure = toFailure(error)
        this.#failed(ctx, failure.class, failure.message)
        await this.#report(lease, () =>
          steps.fail(lease.leaseId, {
            error: {
              class: failure.class,
              message: failure.message,
              details: failure.details,
              retryAfterMs: failure.retryAfterMs,
            },
          }),
        )
        return
      }
      if (abort.signal.aborted) return
      this.#counts.completed++
      this.#hook('onStepEnd', () => this.#hooks.onStepEnd?.(ctx, { status: 'completed' }))
      const { state, usage } = ctx.collected()
      await this.#report(lease, () =>
        steps.complete(lease.leaseId, {
          output: output === undefined ? null : output,
          state,
          usage,
        }),
      )
    } finally {
      clearInterval(beat)
      if (deadline !== undefined) clearTimeout(deadline)
    }
  }

  /**
   * Post one worker call and apply the 409 table: `already_applied` is a
   * success, and every other reason means abandon this lease.
   */
  async #report<T>(lease: StepLease, call: () => Promise<T>): Promise<T | 'done' | 'abandon'> {
    try {
      return await call()
    } catch (error) {
      if (error instanceof StepConflictError) {
        this.#conflict(lease, error.conflict.reason)
        switch (error.conflict.reason) {
          case 'already_applied':
            return 'done'
          case 'not_started':
            this.#onError(
              'not_started',
              `mcpd worker ${this.#workerId}: a bug in this worker: ${lease.stepKey} was reported before started`,
              error,
            )
            return 'abandon'
          case 'fenced':
          case 'run_terminal':
            return 'abandon'
          default:
            return 'abandon'
        }
      }
      if (error instanceof McpdError) {
        this.#onError('call', `mcpd worker ${this.#workerId}: call failed for ${lease.stepKey}`, error)
        return 'abandon'
      }
      throw error
    }
  }

  #conflict(lease: StepLease, reason: ConflictReason): void {
    this.#counts.conflicts[reason] = (this.#counts.conflicts[reason] ?? 0) + 1
    this.#hook('onConflict', () => this.#hooks.onConflict?.(lease, reason))
  }
}

/** Resolves, with nothing, once `signal` aborts. */
function untilAborted(signal: AbortSignal): Promise<undefined> {
  return new Promise((resolve) => {
    if (signal.aborted) resolve(undefined)
    else signal.addEventListener('abort', () => resolve(undefined), { once: true })
  })
}

function isThenable(value: unknown): value is PromiseLike<unknown> {
  return (
    value !== null &&
    (typeof value === 'object' || typeof value === 'function') &&
    typeof (value as { then?: unknown }).then === 'function'
  )
}

function toFailure(error: unknown): {
  class: StepErrorClass
  message: string
  details: unknown
  retryAfterMs: number | undefined
} {
  if (error instanceof StepFailure) {
    return {
      class: error.class,
      message: error.message,
      details: error.details,
      retryAfterMs: error.retryAfterMs,
    }
  }
  const message = error instanceof Error ? error.message : String(error)
  return { class: 'fatal', message: message || 'the step body threw', details: undefined, retryAfterMs: undefined }
}

function defaultWorkerId(): string {
  const host =
    (globalThis as { process?: { env?: Record<string, string | undefined> } }).process?.env?.[
      'HOSTNAME'
    ] ?? 'worker'
  return `${host}-${Math.random().toString(16).slice(2, 10)}`
}
