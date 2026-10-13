/**
 * A span: one step of an agent run, with a start and usually an end.
 *
 * Every method here is total. None of them throws, none of them returns null,
 * and all of them keep working after `end()` has been called, because
 * instrumentation that can fail is worse than no instrumentation at all (B1).
 */

import { responseFacts } from './model-call.js'
import type {
  Correlation,
  EntityKind,
  ModelParameters,
  Span as SpanData,
  SpanKind,
  SpanLink,
  SpanStatus,
  TokenUsage,
} from './generated.js'
import { modelParametersDefaults } from './generated.js'
import type { SpanContext } from './context.js'
import { formatTraceparent, newSpanId, nowNs, formatTimestamp } from './context.js'

/**
 * What a span needs from the client that made it.
 *
 * Declared here rather than imported from the client so that the two modules
 * do not depend on each other's implementation.
 */
export interface SpanHost {
  /** Put a snapshot of the span on the queue. Never throws. */
  enqueueSpan(span: SpanData, key: string): void
  /** Report an SDK-internal failure. Never throws. */
  diagnose(operation: string, error: unknown): void
  /** Project, environment and service, plus whatever the caller added. */
  baseCorrelation(): Correlation
  /** Whether `input` and `output` may leave the process, unless a span says otherwise. */
  capturePayloads(): boolean
}

/**
 * What a span hands upward when it ends carrying usage.
 *
 * Internal: a step context installs a sink on the span it opens, and every
 * span nested under it inherits the sink, so the step's usage is the sum of
 * what its spans spent without the body adding tokens by hand.
 */
export interface UsageReport {
  usage: TokenUsage
  model?: string | undefined
  provider?: string | undefined
}

/** @internal */
export type UsageSink = (report: UsageReport) => void

/**
 * The sampling settings of one model call, as a caller has them.
 *
 * Every field is optional, `hasResponseFormat` included: it stands for the
 * spec's default of `false`, so a caller who did not constrain the output has
 * nothing to say. This is deliberately not `ModelParameters` itself, which
 * requires that flag.
 */
export type ModelParameterInput = Partial<ModelParameters>

/** Fields you can set when a span starts. All of them are optional. */
export interface SpanOptions {
  /** What kind of operation this is. Defaults to `generic` for a child. */
  kind?: SpanKind
  /** Parent to attach to. Overrides whatever is ambient. */
  parent?: McpdSpan | SpanContext | undefined
  /** An inbound W3C `traceparent`. Continues that trace instead of starting one (B7). */
  traceparent?: string | undefined
  /** A parent from a tracing system that is not mcpd, carried for correlation only. */
  externalParentSpanId?: string
  /**
   * Whether this span's `input` and `output` may leave the process. Overrides
   * the client's `capturePayloads` for this span only, in either direction.
   */
  capturePayloads?: boolean
  /** What went in. For a model call, the prompt. */
  input?: unknown
  /** What came out. For a model call, the completion. */
  output?: unknown
  /** Kind-specific structured detail. */
  attributes?: unknown
  /** Anything you want to attach. mcpd never interprets it. */
  metadata?: unknown
  /** Free labels for filtering. Only meaningful on a root span. */
  tags?: string[]
  /** Model identifier, such as `claude-sonnet-4-6`. */
  model?: string
  /** Model provider, such as `anthropic`. */
  provider?: string
  /** Token counts. */
  usage?: TokenUsage
  /** What kind of thing produced this span. */
  entityKind?: EntityKind
  /** Stable id of the producing entity. */
  entityId?: string
  /** Display name of the producing entity. */
  entityName?: string
  /** Causal references to other spans or traces. */
  links?: SpanLink[]
  /** Correlation overrides: session, thread, user, run, request, release. */
  correlation?: Partial<Correlation>
  /** Start time as RFC 3339. Defaults to now. Use it when replaying history. */
  startTime?: string
}

/** Everything `setError` accepts beyond the message. */
export interface ErrorOptions {
  /** The error class or code, such as `RateLimitError`. */
  kind?: string
  /** A stack trace, when you have one. */
  stack?: string
  /** Which subsystem raised it. */
  domain?: string
  /** Whether you considered the failure worth retrying. */
  retryable?: boolean
  /** Structured detail. Redacted like any other payload. */
  details?: unknown
}

/**
 * Pull a message and a stack out of whatever was thrown.
 *
 * JavaScript can throw anything, and `String(value)` on a plain object gives
 * `[object Object]`, which is exactly the unactionable message B10 exists to
 * prevent.
 */
export function describeThrown(thrown: unknown): { message: string; kind?: string; stack?: string } {
  if (thrown instanceof Error) {
    const out: { message: string; kind?: string; stack?: string } = {
      message: thrown.message || thrown.name || 'Error',
      kind: thrown.name,
    }
    if (thrown.stack) out.stack = thrown.stack
    return out
  }
  if (typeof thrown === 'string' && thrown.length > 0) return { message: thrown }
  try {
    return { message: JSON.stringify(thrown) ?? String(thrown) }
  } catch {
    return { message: String(thrown) }
  }
}

export class McpdSpan {
  readonly #host: SpanHost
  readonly #data: SpanData
  readonly #startNs: bigint
  readonly #sampled: boolean
  readonly #key: string
  readonly #capture: boolean
  #usageSink: UsageSink | undefined
  #usageReported = false
  #ended = false

  /**
   * Not part of the public surface: spans are created through
   * `Mcpd.trace()`, `Mcpd.withSpan()` or `span.child()`.
   */
  constructor(host: SpanHost, data: SpanData, sampled: boolean, startNs: bigint, capture = true) {
    this.#host = host
    this.#data = data
    this.#sampled = sampled
    this.#startNs = startNs
    this.#capture = capture
    this.#key = `${data.traceId}:${data.spanId}`
  }

  /** 32 lowercase hex characters, shared by every span in this run. */
  get traceId(): string {
    return this.#data.traceId
  }

  /** 16 lowercase hex characters, unique within the trace. */
  get spanId(): string {
    return this.#data.spanId
  }

  /** The parent inside this trace, absent on a root. */
  get parentSpanId(): string | undefined {
    return this.#data.parentSpanId
  }

  /** The sampling decision this trace was given at its root (B4). */
  get sampled(): boolean {
    return this.#sampled
  }

  /** True once `end()` has run. A second `end()` is ignored. */
  get ended(): boolean {
    return this.#ended
  }

  /** Whether this span's `input` and `output` are kept in the process. */
  get capturesPayloads(): boolean {
    return this.#capture
  }

  /** The current state of the span. A copy: mutating it changes nothing. */
  snapshot(): SpanData {
    return { ...this.#data }
  }

  /**
   * What this span is tagged with, as it stands now. A copy.
   *
   * Read by a child on the way up and by a signal recorded while this span is
   * active, so both see a field set on the run after it started.
   */
  get correlation(): Correlation {
    return { ...this.#data.correlation }
  }

  /** This span, in the shape `traceparent` carries. */
  context(): SpanContext {
    return { traceId: this.traceId, spanId: this.spanId, sampled: this.#sampled }
  }

  /**
   * A W3C `traceparent` header value for this span.
   *
   * Put it on an outbound HTTP request and the service on the other side
   * continues this trace instead of starting a new one (B7).
   */
  traceparent(): string {
    return formatTraceparent(this.context())
  }

  /** Start a child span. Ends independently of this one. */
  child(name: string, options: SpanOptions = {}): McpdSpan {
    return this.#guard('child', () => startChild(this.#host, this, name, options), () =>
      startDetached(this.#host, this, name),
    )
  }

  /**
   * Run `fn` inside a child span, ending it when `fn` settles.
   *
   * The child is recorded as failed if `fn` throws, and the error is rethrown
   * unchanged: the SDK never swallows the caller's exception, only its own.
   */
  async withChild<T>(
    name: string,
    fn: (span: McpdSpan) => T | Promise<T>,
    options: SpanOptions = {},
  ): Promise<T> {
    const span = this.child(name, options)
    return await runInSpan(span, fn)
  }

  /**
   * Record a point in time inside this span.
   *
   * An event is a span with no end, which is how the daemon stores a streamed
   * chunk or a retry marker without inventing a zero-length duration.
   */
  event(name: string, options: SpanOptions = {}): void {
    this.#guard(
      'event',
      () => {
        // Built rather than started, because an event is emitted exactly once
        // and has no open state for a later record to update.
        const span = createSpan(
          this.#host,
          name,
          { traceId: this.traceId, spanId: newSpanId(), sampled: this.#sampled },
          this.spanId,
          'generic',
          options,
          this.correlation,
        )
        span.#data.isEvent = true
        span.#data.status = 'ok'
        span.#ended = true
        span.#usageSink = this.#usageSink
        span.#emit()
        span.#reportUsage()
      },
      () => undefined,
    )
  }

  /**
   * What went in. For a model call, the full prompt.
   *
   * Ignored when payload capture is off for this span: the value never reaches
   * the queue, so it can never leave the process.
   */
  setInput(value: unknown): this {
    return this.#set('setInput', () => {
      if (this.#capture) this.#data.input = value
    })
  }

  /** What came out. For a model call, the completion. Same rule as `setInput`. */
  setOutput(value: unknown): this {
    return this.#set('setOutput', () => {
      if (this.#capture) this.#data.output = value
    })
  }

  /** Token counts. The daemon prices them at ingest. */
  setUsage(usage: TokenUsage): this {
    return this.#set('setUsage', () => {
      this.#data.usage = { ...this.#data.usage, ...usage }
    })
  }

  /** Which model, and whose. Both are indexed, so keep them exact. */
  setModel(model: string, provider?: string): this {
    return this.#set('setModel', () => {
      this.#data.model = model
      if (provider !== undefined) this.#data.provider = provider
    })
  }

  /**
   * How the model was called: temperature, nucleus and top-k sampling, the
   * output ceiling, the seed, the stop sequences, and whether the output was
   * constrained to a fixed shape.
   *
   * These are settings, not payload. They carry no prompt and no completion,
   * so they are recorded whether or not payload capture is on: they are what a
   * reader needs to reproduce the call, and dropping them with the prompt would
   * leave a run nobody can repeat.
   *
   * One call describes one model call, so a second call replaces the first
   * rather than merging into it: settings half from one request and half from
   * another would describe a call nobody made. A field you leave out is absent,
   * and `hasResponseFormat` falls back to the spec's `false`.
   *
   * A field you do pass is kept whatever its value is. `seed: 0` is a seed and
   * `temperature: 0` is a temperature; only `undefined` means "not given".
   */
  setModelParameters(parameters: ModelParameterInput): this {
    return this.#set('setModelParameters', () => {
      this.#data.modelParameters = buildModelParameters(parameters)
    })
  }

  /**
   * Record what an OpenAI- or Anthropic-shaped response says about its call:
   * the model, the provider, the token usage with its cache and reasoning
   * meters, and the response id, finish reasons and service tier as
   * `gen_ai.response.*` attributes. The daemon prices a batch or flex call
   * from the tier.
   *
   * It never records the text: set it with `setOutput` if you want it kept.
   * An unknown shape records what it can and raises nothing.
   */
  recordResponse(response: unknown, provider?: string): this {
    return this.#set('recordResponse', () => {
      const facts = responseFacts(response, provider)
      if (facts.model !== undefined) this.#data.model = facts.model
      if (facts.provider !== undefined) this.#data.provider = facts.provider
      if (facts.usage !== undefined) this.#data.usage = facts.usage
      if (Object.keys(facts.attributes).length > 0) {
        this.#data.attributes = { ...(this.#data.attributes as object), ...facts.attributes }
      }
    })
  }

  /**
   * Record the time to first token, from this span's start to now, as
   * `mcpd.time_to_first_token_ms`. Only the first call counts, so it can sit
   * inside a streaming loop.
   */
  markFirstToken(): this {
    return this.#set('markFirstToken', () => {
      const attrs = (this.#data.attributes ?? {}) as Record<string, unknown>
      if (attrs['mcpd.time_to_first_token_ms'] !== undefined) return
      const ms = Number(nowNs() - this.#startNs) / 1e6
      this.#data.attributes = { ...attrs, 'mcpd.time_to_first_token_ms': ms }
    })
  }

  /**
   * Record a guardrail's outcome as `mcpd.guardrail.tripped` and, when given,
   * `mcpd.guardrail.reason`. Tripping is not an error by itself; call
   * `setError` as well if it should fail the span.
   */
  setGuardrail(tripped: boolean, reason?: string): this {
    return this.#set('setGuardrail', () => {
      const add: Record<string, unknown> = { 'mcpd.guardrail.tripped': tripped }
      if (reason !== undefined) add['mcpd.guardrail.reason'] = reason
      this.#data.attributes = { ...(this.#data.attributes as object), ...add }
    })
  }

  /** Kind-specific structured detail, merged with anything already set. */
  setAttributes(attributes: Record<string, unknown>): this {
    return this.#set('setAttributes', () => {
      this.#data.attributes = { ...(this.#data.attributes as object), ...attributes }
    })
  }

  /** Anything you want to attach, merged with anything already set. */
  setMetadata(metadata: Record<string, unknown>): this {
    return this.#set('setMetadata', () => {
      this.#data.metadata = { ...(this.#data.metadata as object), ...metadata }
    })
  }

  /**
   * Labels for filtering.
   *
   * Tags belong to the whole run, so the daemon reads them from the root span.
   * Setting them on a child is accepted and stored, but the trace list will not
   * see them.
   */
  setTags(tags: string[]): this {
    return this.#set('setTags', () => {
      this.#data.tags = [...tags]
    })
  }

  /** Session, thread, user, run, request or release, merged in. */
  setCorrelation(correlation: Partial<Correlation>): this {
    return this.#set('setCorrelation', () => {
      this.#data.correlation = { ...this.#data.correlation, ...correlation }
    })
  }

  /**
   * Mark the span failed (B10).
   *
   * The message is required, and setting an error sets `status` to `error` in
   * the same move, so the two can never disagree.
   */
  setError(message: string, options: ErrorOptions = {}): this {
    return this.#set('setError', () => {
      this.#data.status = 'error'
      this.#data.error = {
        message: message.length > 0 ? message : 'no message given',
        ...options,
      }
    })
  }

  /**
   * Finish the span and queue it.
   *
   * Calling it twice is a no-op: the first end wins, so a `finally` block and
   * an explicit call cannot produce two different durations.
   */
  end(status?: SpanStatus): void {
    this.#guard(
      'end',
      () => {
        if (this.#ended) return
        this.#ended = true
        const endNs = nowNs()
        this.#data.endTime = formatTimestamp(endNs)
        this.#data.durationNs = Number(endNs - this.#startNs)
        if (status !== undefined) this.#data.status = status
        else if (this.#data.status === 'unset') this.#data.status = 'ok'
        this.#emit()
        this.#reportUsage()
      },
      () => undefined,
    )
  }

  /** Queue the current state. Called on start and again on end (B8). */
  #emit(): void {
    if (!this.#sampled) return
    this.#host.enqueueSpan({ ...this.#data }, this.#key)
  }

  /**
   * Hand this span's usage to the step it runs under, once.
   *
   * Only a span that carries usage reports, so a container span that names a
   * model and spends nothing does not overwrite the model that did.
   */
  #reportUsage(): void {
    if (this.#usageReported || this.#usageSink === undefined) return
    const usage = this.#data.usage
    if (usage === undefined) return
    this.#usageReported = true
    const report: UsageReport = { usage: { ...usage } }
    if (this.#data.model !== undefined) report.model = this.#data.model
    if (this.#data.provider !== undefined) report.provider = this.#data.provider
    this.#usageSink(report)
  }

  #set(operation: string, apply: () => void): this {
    this.#guard(operation, apply, () => undefined)
    return this
  }

  #guard<T>(operation: string, apply: () => T, fallback: () => T): T {
    try {
      return apply()
    } catch (error) {
      this.#host.diagnose(`span.${operation}`, error)
      return fallback()
    }
  }

  /** Internal: emit the start record. */
  static begin(span: McpdSpan): McpdSpan {
    span.#emit()
    return span
  }

  /**
   * Internal: install the sink a step context accumulates usage through.
   * Children inherit it in `startChild`; nothing about it reaches the wire.
   */
  static adoptUsageSink(span: McpdSpan, sink: UsageSink | undefined): void {
    span.#usageSink = sink
  }

  /** Internal: the sink this span reports usage to, if any. */
  static usageSinkOf(span: McpdSpan): UsageSink | undefined {
    return span.#usageSink
  }
}

/**
 * The record for one set of sampling settings.
 *
 * Only a field that was actually supplied is written, and the test is for
 * `undefined` rather than for truth. That is the whole point: `0` is a real
 * temperature and a real seed, and `false` is a real answer to whether the
 * output was constrained, so none of them may be read as "not given".
 */
function buildModelParameters(supplied: ModelParameterInput): ModelParameters {
  const built: ModelParameters = { ...modelParametersDefaults }
  if (supplied.temperature !== undefined) built.temperature = supplied.temperature
  if (supplied.topP !== undefined) built.topP = supplied.topP
  if (supplied.topK !== undefined) built.topK = supplied.topK
  if (supplied.maxTokens !== undefined) built.maxTokens = supplied.maxTokens
  if (supplied.seed !== undefined) built.seed = supplied.seed
  if (supplied.stopSequences !== undefined) built.stopSequences = [...supplied.stopSequences]
  if (supplied.hasResponseFormat !== undefined) {
    built.hasResponseFormat = supplied.hasResponseFormat
  }
  return built
}

/**
 * Build a span, without deciding where its parent came from.
 *
 * `create` is the one place a `SpanData` is constructed, so every span in the
 * SDK carries the same required fields whether it is a root, a child or an
 * event.
 */
export function createSpan(
  host: SpanHost,
  name: string,
  context: SpanContext,
  parentSpanId: string | undefined,
  defaultKind: SpanKind,
  options: SpanOptions,
  base?: Correlation,
): McpdSpan {
  const startNs = nowNs()
  const startTime = options.startTime ?? formatTimestamp(startNs)
  const data: SpanData = {
    traceId: context.traceId,
    spanId: context.spanId,
    name,
    kind: options.kind ?? defaultKind,
    status: 'unset',
    startTime,
    isRoot: parentSpanId === undefined,
    isEvent: false,
    // `base` is what this span inherits: the client's own for a root, the
    // parent's for a child. A child built from the client's would drop every
    // field the run was tagged with, and a run whose session stops at the root
    // cannot be filtered by session or erased by subject.
    correlation: { ...(base ?? host.baseCorrelation()), ...options.correlation },
    // The daemon stamps its own arrival time; this is the producer's view, and
    // it is what a replayed batch is measured against.
    receivedAt: startTime,
  }
  const capture = options.capturePayloads ?? host.capturePayloads()
  if (parentSpanId !== undefined) data.parentSpanId = parentSpanId
  if (options.externalParentSpanId !== undefined) {
    data.externalParentSpanId = options.externalParentSpanId
  }
  if (capture && options.input !== undefined) data.input = options.input
  if (capture && options.output !== undefined) data.output = options.output
  if (options.attributes !== undefined) data.attributes = options.attributes
  if (options.metadata !== undefined) data.metadata = options.metadata
  if (options.tags !== undefined) data.tags = [...options.tags]
  if (options.model !== undefined) data.model = options.model
  if (options.provider !== undefined) data.provider = options.provider
  if (options.usage !== undefined) data.usage = options.usage
  if (options.entityKind !== undefined) data.entityKind = options.entityKind
  if (options.entityId !== undefined) data.entityId = options.entityId
  if (options.entityName !== undefined) data.entityName = options.entityName
  if (options.links !== undefined) data.links = [...options.links]

  return new McpdSpan(host, data, context.sampled, startNs, capture)
}

/** A child of `parent`, inheriting its trace and its sampling decision (B4). */
export function startChild(
  host: SpanHost,
  parent: McpdSpan,
  name: string,
  options: SpanOptions,
): McpdSpan {
  const span = createSpan(
    host,
    name,
    { traceId: parent.traceId, spanId: newSpanId(), sampled: parent.sampled },
    parent.spanId,
    'generic',
    options,
    parent.correlation,
  )
  McpdSpan.adoptUsageSink(span, McpdSpan.usageSinkOf(parent))
  return McpdSpan.begin(span)
}

/**
 * The last resort when building a child threw.
 *
 * Returns a span that is structurally valid and records nothing, so the caller
 * can carry on calling methods on it.
 */
function startDetached(host: SpanHost, parent: McpdSpan, name: string): McpdSpan {
  const data: SpanData = {
    traceId: parent.traceId,
    spanId: newSpanId(),
    parentSpanId: parent.spanId,
    name,
    kind: 'generic',
    status: 'unset',
    startTime: formatTimestamp(nowNs()),
    isRoot: false,
    isEvent: false,
    correlation: safeCorrelation(host, parent),
    receivedAt: formatTimestamp(nowNs()),
  }
  return new McpdSpan(host, data, false, nowNs())
}

function safeCorrelation(host: SpanHost, parent?: McpdSpan): Correlation {
  try {
    return parent ? parent.correlation : host.baseCorrelation()
  } catch {
    return { project: 'default' }
  }
}

/**
 * Run `fn` with `span` open, closing it however `fn` finishes.
 *
 * A thrown value is recorded on the span and then rethrown untouched. The
 * caller's control flow is never altered by the fact that it is being observed.
 */
export async function runInSpan<T>(
  span: McpdSpan,
  fn: (span: McpdSpan) => T | Promise<T>,
): Promise<T> {
  try {
    const result = await fn(span)
    span.end()
    return result
  } catch (error) {
    const described = describeThrown(error)
    const options: ErrorOptions = {}
    if (described.kind !== undefined) options.kind = described.kind
    if (described.stack !== undefined) options.stack = described.stack
    span.setError(described.message, options)
    span.end('error')
    throw error
  }
}
