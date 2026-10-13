/**
 * The client. One object holds the configuration, the queue, the worker and
 * the read API.
 *
 * A client built with no arguments works against a daemon on localhost. That is
 * deliberate: the first trace has to be reachable without reading a
 * configuration reference first.
 */

import type {
  AnalyticsSummary,
  Correlation,
  CostBreakdown,
  Feedback,
  FeedbackKind,
  FeedbackListResponse,
  HealthStatus,
  IngestBatch,
  LogLevel,
  LogListResponse,
  LogRecord,
  MetricKind,
  MetricPoint,
  MetricSeriesResponse,
  Score,
  ScoreListResponse,
  ScoreSource,
  Span as SpanData,
  SpanKind,
  SpanListResponse,
  TraceDetail,
  TraceListResponse,
} from './generated.js'
import type { SpanContext } from './context.js'
import {
  contextReady,
  contextStore,
  isSpanId,
  isTraceId,
  newSpanId,
  newTraceId,
  nowTimestamp,
  parseTraceparent,
} from './context.js'
import type { SpanHost, SpanOptions } from './span.js'
import { McpdSpan, createSpan, runInSpan } from './span.js'
import { Exporter, SignalQueue } from './queue.js'
import type { Signal } from './queue.js'
import type { Sink } from './sinks.js'
import { encodeFields, meetsLevel, parseLogLevel } from './logging.js'
import type { Diagnostics, FetchLike, QueryValue } from './transport.js'
import { McpdError, RateLimitedDiagnostics, Transport } from './transport.js'
import { DefinitionsApi, EventsApi, RunsApi, StepsApi } from './runs.js'

/** Every setting, and the environment variable that also sets it. */
export interface McpdOptions {
  /** Where the daemon is. `MCPD_ENDPOINT`. */
  endpoint?: string
  /** Bearer token, when the daemon requires one. `MCPD_API_KEY`. */
  apiKey?: string
  /** The tenancy key every signal belongs to. `MCPD_PROJECT`. */
  project?: string
  /** Deployment environment, such as `production`. `MCPD_ENVIRONMENT`. */
  environment?: string
  /** The emitting service. `MCPD_SERVICE_NAME`. */
  serviceName?: string
  /** The emitting service's version. `MCPD_SERVICE_VERSION`. */
  serviceVersion?: string
  /** Fraction of traces to keep, 0 to 1. `MCPD_SAMPLE`. */
  sample?: number
  /** Signals per batch. `MCPD_BATCH_SIZE`. */
  batchSize?: number
  /** How often the worker sends what it has, in milliseconds. `MCPD_FLUSH_INTERVAL_MS`. */
  flushIntervalMs?: number
  /** How many signals may wait before the oldest are dropped. `MCPD_MAX_QUEUE`. */
  maxQueue?: number
  /** Per-request timeout in milliseconds. `MCPD_TIMEOUT_MS`. */
  timeoutMs?: number
  /**
   * When false nothing is recorded and no signal is sent, and the list reads
   * return an empty page. Everything still returns a valid object, so a test
   * suite can switch mcpd off without changing a line of instrumentation.
   * `MCPD_ENABLED`.
   */
  enabled?: boolean
  /**
   * Whether `input` and `output` may leave the process. When false, spans are
   * still recorded with their timing, status, usage, model, correlation and
   * errors, and the payloads are dropped before they reach the queue. A span
   * can override it for itself. `MCPD_CAPTURE_PAYLOADS`.
   */
  capturePayloads?: boolean
  /**
   * The lowest level `log` records. A line below it is dropped at the call,
   * before the record is built. Defaults to `debug`, which drops `trace` and
   * keeps everything else. `MCPD_LOG_LEVEL`.
   */
  logLevel?: LogLevel
  /**
   * Where drained batches go. Defaults to the HTTP transport. A
   * `RecordingSink` keeps them in memory for a test; a `ConsoleSink` prints
   * them. Chosen in code only.
   */
  sink?: Sink
  /**
   * Applied to every span's input, output, attributes and metadata, and to
   * every log line's fields, before the record is queued. Return the value to
   * keep. A redactor that throws drops that value and counts it in
   * `stats().redactionFailures`: a broken redactor never lets the original
   * through (SDK contract 2.1.1).
   */
  redact?: (value: unknown) => unknown
  /** Correlation fields to put on every signal, such as a release sha. */
  correlation?: Partial<Correlation>
  /** Replace `fetch`. Useful in tests and in runtimes with an unusual client. */
  fetch?: FetchLike
  /** Where the SDK's own failures go. Defaults to one `console.warn` per minute. */
  onError?: (message: string, error: unknown) => void
  /** How often a repeated internal failure may be reported, in milliseconds. */
  errorIntervalMs?: number
  /** Flush on Node's `beforeExit`. On by default. */
  flushOnExit?: boolean
}

/** Options accepted by `trace()`, plus a root-only sampling override. */
export interface TraceOptions extends SpanOptions {
  /** Force this trace to be kept or dropped, ignoring the sample rate. */
  sampled?: boolean
}

/** A score. `min` and `max` are required: a bare number is not comparable. */
export interface ScoreInput {
  /** The scored run. Defaults to the active span's trace. */
  traceId?: string
  /** The scored step, when the score is about one step rather than the run. */
  spanId?: string
  /** Which scorer produced this, such as `answer_relevance`. */
  scorerId: string
  /** The numeric result. */
  value: number
  /** Lower bound of the scale. */
  min: number
  /** Upper bound of the scale. */
  max: number
  /** Why. For a judge, the judge's explanation. */
  reason?: string
  /** A categorical result alongside the number. */
  label?: string
  /** The scorer's version. A score compares only to the same version. */
  scorerVersion?: string
  /** Live, offline, experiment or manual. Defaults to `live`. */
  source?: ScoreSource
  /** The trace of the scoring run itself. */
  scoreTraceId?: string
  /** Anything else. */
  metadata?: Record<string, unknown>
  /** When it was produced. Defaults to now. */
  timestamp?: string
}

/** A human judgement. */
export interface FeedbackInput {
  /** The judged run. Defaults to the active span's trace. */
  traceId?: string
  /** The judged step. */
  spanId?: string
  /** Thumbs, rating, correction, tag or comment. */
  kind: FeedbackKind
  /** The judgement. Its shape follows `kind`. */
  value: unknown
  /** Free prose the person added. */
  comment?: string
  /** Who gave it. The daemon hashes it by default. */
  authorId?: string
  /** Whether the author was an end user, a reviewer or an operator. */
  authorKind?: string
  /** Anything else. */
  metadata?: Record<string, unknown>
  /** When it was given. Defaults to now. */
  timestamp?: string
}

/** Where a log line or a metric point attaches. */
export interface SignalTarget {
  traceId?: string
  spanId?: string
}

/** What the client has done since it was created. */
export interface McpdStats {
  /** Signals waiting to be sent. */
  queued: number
  /** Signals evicted because the queue was full. */
  dropped: number
  /** How many the queue can hold. */
  capacity: number
  /** Signals the sink accepted. */
  exported: number
  /** Batches the sink refused. */
  failed: number
  /** Spans `record()` refused. */
  rejected: number
  /** Values `redact` threw on, dropped rather than sent. */
  redactionFailures: number
  /** Which context mechanism is in use. */
  context: 'async-local-storage' | 'fallback'
}

const DEFAULTS = {
  endpoint: 'http://localhost:4317',
  project: 'default',
  sample: 1,
  batchSize: 128,
  flushIntervalMs: 2_000,
  maxQueue: 8_192,
  timeoutMs: 10_000,
  errorIntervalMs: 60_000,
  logLevel: 'debug',
} as const

function env(name: string): string | undefined {
  const process = (globalThis as { process?: { env?: Record<string, string | undefined> } }).process
  const value = process?.env?.[name]
  return value === undefined || value === '' ? undefined : value
}

function envNumber(name: string): number | undefined {
  const raw = env(name)
  if (raw === undefined) return undefined
  const value = Number(raw)
  return Number.isFinite(value) ? value : undefined
}

/**
 * A whole positive number from the environment, or nothing.
 *
 * The four tuning variables carry their unit in their name, so a value is a
 * plain integer and never a duration string. Anything else, including zero, is
 * ignored rather than used: a batch of zero signals or a queue of zero entries
 * would stop the exporter, and a typo in a deployment variable must not.
 */
function envInteger(name: string): number | undefined {
  const value = envNumber(name)
  if (value === undefined || !Number.isInteger(value) || value <= 0) return undefined
  return value
}

function envBoolean(name: string): boolean | undefined {
  const raw = env(name)?.toLowerCase()
  if (raw === undefined) return undefined
  if (raw === '0' || raw === 'false' || raw === 'no' || raw === 'off') return false
  return true
}

/** Clamp a sample rate into range rather than reject it: a typo must not stop the run. */
function clampSample(value: number | undefined): number {
  if (value === undefined || !Number.isFinite(value)) return DEFAULTS.sample
  return Math.min(1, Math.max(0, value))
}

/**
 * Copy the correlation fields that have a value, and leave the rest standing.
 *
 * A key present with no value is not an instruction to erase one.
 * `Partial<Correlation>` permits an explicitly-present `undefined` and an
 * object spread copies it, so a caller who names `sessionId` without a value
 * would take the session off every signal the client tags. A log line with no
 * session is a log line nobody can find by session again.
 */
function mergeDefined(into: Correlation, from: Partial<Correlation> | undefined): Correlation {
  for (const [key, value] of Object.entries(from ?? {})) {
    if (value !== undefined && value !== null) {
      (into as unknown as Record<string, unknown>)[key] = value
    }
  }
  return into
}

export class Mcpd implements SpanHost {
  readonly #project: string
  readonly #correlation: Correlation
  readonly #sample: number
  readonly #enabled: boolean
  readonly #capture: boolean
  readonly #logLevel: LogLevel
  #rejected = 0
  #redactionFailures = 0
  readonly #redact: ((value: unknown) => unknown) | undefined
  readonly #queue: SignalQueue
  readonly #exporter: Exporter
  readonly #transport: Transport
  readonly #diagnostics: Diagnostics
  readonly #errorIntervalMs: number
  /** Durable runs: start, read, watch, cancel, signal. */
  readonly runs: RunsApi
  /** The versioned definitions the daemon walks. */
  readonly definitions: DefinitionsApi
  /** The door a webhook or an approval posts to. */
  readonly events: EventsApi
  /** The worker protocol, call for call. `Worker` drives it. */
  readonly steps: StepsApi

  constructor(options: McpdOptions = {}) {
    // Code beats the environment, and the environment beats the default.
    const endpoint = options.endpoint ?? env('MCPD_ENDPOINT') ?? DEFAULTS.endpoint
    const apiKey = options.apiKey ?? env('MCPD_API_KEY')
    const project = options.project ?? env('MCPD_PROJECT') ?? DEFAULTS.project
    const environment = options.environment ?? env('MCPD_ENVIRONMENT')
    const serviceName = options.serviceName ?? env('MCPD_SERVICE_NAME')
    const serviceVersion = options.serviceVersion ?? env('MCPD_SERVICE_VERSION')

    this.#project = project
    this.#redact = typeof options.redact === 'function' ? options.redact : undefined
    this.#enabled = options.enabled ?? envBoolean('MCPD_ENABLED') ?? true
    this.#capture = options.capturePayloads ?? envBoolean('MCPD_CAPTURE_PAYLOADS') ?? true
    // An unreadable name is ignored rather than refused: a typo in a
    // deployment variable must not silence a service's logs.
    this.#logLevel = options.logLevel ?? parseLogLevel(env('MCPD_LOG_LEVEL')) ?? DEFAULTS.logLevel
    this.#sample = clampSample(options.sample ?? envNumber('MCPD_SAMPLE'))
    this.#correlation = { project }
    if (environment !== undefined) this.#correlation.environment = environment
    if (serviceName !== undefined) this.#correlation.serviceName = serviceName
    if (serviceVersion !== undefined) this.#correlation.serviceVersion = serviceVersion
    mergeDefined(this.#correlation, options.correlation)

    this.#errorIntervalMs = options.errorIntervalMs ?? DEFAULTS.errorIntervalMs
    this.#diagnostics = new RateLimitedDiagnostics(this.#errorIntervalMs, options.onError)
    this.#transport = new Transport({
      endpoint,
      apiKey,
      timeoutMs: options.timeoutMs ?? envInteger('MCPD_TIMEOUT_MS') ?? DEFAULTS.timeoutMs,
      fetchImpl: options.fetch,
      diagnostics: this.#diagnostics,
      userAgent: 'mcpd-sdk-typescript/0.1.0',
    })
    this.runs = new RunsApi(this.#transport, project)
    this.definitions = new DefinitionsApi(this.#transport, project)
    this.events = new EventsApi(this.#transport, project)
    this.steps = new StepsApi(this.#transport, project)
    this.#queue = new SignalQueue(
      options.maxQueue ?? envInteger('MCPD_MAX_QUEUE') ?? DEFAULTS.maxQueue,
      this.#diagnostics,
    )
    this.#exporter = new Exporter(this.#queue, {
      sink: options.sink ?? new TransportSink(this.#transport, this.#diagnostics),
      batchSize: options.batchSize ?? envInteger('MCPD_BATCH_SIZE') ?? DEFAULTS.batchSize,
      flushIntervalMs:
        options.flushIntervalMs ?? envInteger('MCPD_FLUSH_INTERVAL_MS') ?? DEFAULTS.flushIntervalMs,
      diagnostics: this.#diagnostics,
    })

    if (this.#enabled) {
      this.#exporter.start()
      if (options.flushOnExit !== false) this.#registerExitHook()
    }
  }

  /** Where this client sends. */
  get endpoint(): string {
    return this.#transport.endpoint
  }

  /** The project every signal from this client belongs to. */
  get project(): string {
    return this.#project
  }

  /** False when the client is a no-op. */
  get enabled(): boolean {
    return this.#enabled
  }

  /** Queue depth, drops, exports, refusals and which context mechanism is active. */
  stats(): McpdStats {
    return {
      ...this.#queue.stats(),
      ...this.#exporter.stats(),
      rejected: this.#rejected,
      redactionFailures: this.#redactionFailures,
      context: contextStore().kind,
    }
  }

  // -------------------------------------------------------------------------
  // Tracing
  // -------------------------------------------------------------------------

  /**
   * Start a root span: one agent run.
   *
   * The sampling decision is made here and inherited by every child, so a trace
   * is never half-recorded (B4). Pass `traceparent` to continue a trace that
   * started in another service (B7).
   */
  trace(name: string, options: TraceOptions = {}): McpdSpan {
    return this.#guard(
      'trace',
      () => {
        const inherited = options.parent
          ? toContext(options.parent)
          : parseTraceparent(options.traceparent)
        const context: SpanContext = inherited
          ? { traceId: inherited.traceId, spanId: newSpanId(), sampled: inherited.sampled }
          : {
              traceId: newTraceId(),
              spanId: newSpanId(),
              sampled: options.sampled ?? this.#decide(),
            }
        const span = createSpan(
          this,
          name,
          context,
          inherited?.spanId,
          'agent_run',
          options,
        )
        return McpdSpan.begin(span)
      },
      () => this.#inertSpan(name),
    )
  }

  /**
   * Start a span under whatever is active, or a root when nothing is.
   *
   * This is what `withSpan` uses. Call it directly when you want to end the
   * span yourself.
   */
  startSpan(name: string, options: SpanOptions = {}): McpdSpan {
    const parent = options.parent ?? this.activeSpan()
    if (parent instanceof McpdSpan) return parent.child(name, options)
    if (parent) return this.trace(name, { ...options, parent })
    return this.trace(name, options)
  }

  /**
   * Run `fn` inside a span, ending it when `fn` settles.
   *
   * The span is active for the whole callback, so anything the callback starts
   * nests under it without being passed a handle. If `fn` throws, the span is
   * marked `error` with the thrown message and the error is rethrown unchanged.
   */
  async withSpan<T>(
    name: string,
    fn: (span: McpdSpan) => T | Promise<T>,
    options: SpanOptions = {},
  ): Promise<T> {
    // One microtask, paid once per span, so that the context store is decided
    // before the span exists rather than while it is open.
    await contextReady()
    const span = this.startSpan(name, options)
    return await contextStore().run(span, () => runInSpan(span, fn))
  }

  /** The span the current asynchronous context is running inside, if any. */
  activeSpan(): McpdSpan | undefined {
    const active = contextStore().active<McpdSpan>()
    return active instanceof McpdSpan ? active : undefined
  }

  /** Run `fn` with `span` as the active span. */
  withActiveSpan<T>(span: McpdSpan, fn: () => T): T {
    return contextStore().run(span, fn)
  }

  /**
   * Queue a span that has already ended, with the ids, parent and times the
   * caller holds: what an adapter for another tracing system, a batch import
   * or a replay has in hand.
   *
   * The record is queued as given, never re-parented, re-timed or re-sampled.
   * Correlation fields it leaves empty are filled from this client's; fields
   * it sets are kept. A record whose ids are not OpenTelemetry-shaped, whose
   * name is empty or whose start time is missing is dropped and counted in
   * `stats().rejected`, never thrown on.
   */
  record(span: SpanData): void {
    this.#guard(
      'record',
      () => {
        if (!this.#enabled) return
        const reason = refuseRecord(span)
        if (reason !== undefined) {
          this.#rejected++
          this.#diagnostics.report('record refused a span', reason)
          return
        }
        const given = Object.entries(span.correlation ?? {}).filter(([, v]) => v !== undefined)
        const correlation: Correlation = { ...this.#correlation, ...Object.fromEntries(given) }
        const data: SpanData = { ...span, correlation }
        if (!this.#capture) {
          delete data.input
          delete data.output
        }
        this.#push({ kind: 'span', key: `${data.traceId}:${data.spanId}`, value: data })
      },
      () => undefined,
    )
  }

  // -------------------------------------------------------------------------
  // The other four signals
  // -------------------------------------------------------------------------

  /** A log line, correlated to the active span unless you name another. */
  log(
    level: LogLevel,
    message: string,
    fields?: Record<string, unknown>,
    target: SignalTarget = {},
  ): void {
    this.#guard(
      'log',
      () => {
        if (!this.#enabled) return
        // Below the floor costs one comparison: no record, no queue, no
        // correlation lookup.
        if (!meetsLevel(level, this.#logLevel)) return
        const active = this.activeSpan()
        const record: LogRecord = {
          logId: newTraceId(),
          timestamp: nowTimestamp(),
          level,
          message,
          correlation: this.#signalCorrelation(active),
          receivedAt: nowTimestamp(),
        }
        const traceId = target.traceId ?? active?.traceId
        const spanId = target.spanId ?? active?.spanId
        if (traceId !== undefined) record.traceId = traceId
        if (spanId !== undefined) record.spanId = spanId
        if (fields !== undefined) record.fields = encodeFields(fields)
        this.#push({ kind: 'log', value: record })
      },
      () => undefined,
    )
  }

  /** A numeric observation, with an exemplar link to the span that produced it. */
  metric(
    name: string,
    value: number,
    kind: MetricKind = 'gauge',
    options: { labels?: Record<string, unknown>; unit?: string } & SignalTarget = {},
  ): void {
    this.#guard(
      'metric',
      () => {
        if (!this.#enabled) return
        // JSON has no literal for NaN or an infinity, so the point would go
        // out as a null where the daemon needs a number, and the daemon would
        // refuse the whole batch it travelled in. The point is what cannot be
        // sent, so it alone is dropped (contract section 4.1).
        if (!Number.isFinite(value)) {
          this.#diagnostics.report(
            `metric ${name} has the value ${String(value)}, which JSON cannot carry; the point was dropped`,
            undefined,
          )
          return
        }
        const active = this.activeSpan()
        const point: MetricPoint = {
          metricId: newTraceId(),
          name,
          kind,
          value,
          timestamp: nowTimestamp(),
          correlation: this.#signalCorrelation(active),
          receivedAt: nowTimestamp(),
        }
        const traceId = options.traceId ?? active?.traceId
        const spanId = options.spanId ?? active?.spanId
        if (traceId !== undefined) point.traceId = traceId
        if (spanId !== undefined) point.spanId = spanId
        if (options.labels !== undefined) point.labels = encodeFields(options.labels)
        if (options.unit !== undefined) point.unit = options.unit
        this.#push({ kind: 'metric', value: point })
      },
      () => undefined,
    )
  }

  /**
   * Attach a machine evaluation.
   *
   * `min` and `max` are required by the type, because a score of 0.7 on an
   * undeclared scale cannot be compared to anything.
   */
  score(input: ScoreInput): void {
    this.#guard(
      'score',
      () => {
        if (!this.#enabled) return
        if (![input.value, input.min, input.max].every((n) => Number.isFinite(n))) {
          this.#diagnostics.report(
            `score ${input.scorerId} has value ${String(input.value)} on [${String(input.min)}, ` +
              `${String(input.max)}], which JSON cannot carry; the score was dropped`,
            undefined,
          )
          return
        }
        const active = this.activeSpan()
        const score: Score = {
          scoreId: newTraceId(),
          scorerId: input.scorerId,
          source: input.source ?? 'live',
          value: input.value,
          min: input.min,
          max: input.max,
          timestamp: input.timestamp ?? nowTimestamp(),
          correlation: this.#signalCorrelation(active),
        }
        const traceId = input.traceId ?? active?.traceId
        if (traceId !== undefined) score.traceId = traceId
        if (input.spanId !== undefined) score.spanId = input.spanId
        if (input.reason !== undefined) score.reason = input.reason
        if (input.label !== undefined) score.label = input.label
        if (input.scorerVersion !== undefined) score.scorerVersion = input.scorerVersion
        if (input.scoreTraceId !== undefined) score.scoreTraceId = input.scoreTraceId
        if (input.metadata !== undefined) score.metadata = input.metadata
        this.#push({ kind: 'score', value: score })
      },
      () => undefined,
    )
  }

  /** Attach a human judgement. Kept separate from a score, and never averaged with one. */
  feedback(input: FeedbackInput): void {
    this.#guard(
      'feedback',
      () => {
        if (!this.#enabled) return
        const active = this.activeSpan()
        const feedback: Feedback = {
          feedbackId: newTraceId(),
          kind: input.kind,
          value: input.value,
          timestamp: input.timestamp ?? nowTimestamp(),
          correlation: this.#signalCorrelation(active),
        }
        const traceId = input.traceId ?? active?.traceId
        if (traceId !== undefined) feedback.traceId = traceId
        if (input.spanId !== undefined) feedback.spanId = input.spanId
        if (input.comment !== undefined) feedback.comment = input.comment
        if (input.authorId !== undefined) feedback.authorId = input.authorId
        if (input.authorKind !== undefined) feedback.authorKind = input.authorKind
        if (input.metadata !== undefined) feedback.metadata = input.metadata
        this.#push({ kind: 'feedback', value: feedback })
      },
      () => undefined,
    )
  }

  // -------------------------------------------------------------------------
  // Lifecycle
  // -------------------------------------------------------------------------

  /**
   * Send everything queued. Never rejects (B5).
   *
   * Waits at most `timeoutMs`, 10 seconds by default, and resolves `false`
   * when that passed first. What was not sent by then stays queued.
   */
  async flush(options: { timeoutMs?: number } = {}): Promise<boolean> {
    if (!this.#enabled) return true
    return await this.#exporter.flush(options.timeoutMs)
  }

  /**
   * Flush, stop the worker, and stay stopped. Safe to call twice (B5).
   *
   * Short-lived processes lose their last batch without this, so call it before
   * a serverless handler returns. It returns within `timeoutMs`, 10 seconds by
   * default, even when the daemon never answers; what is still queued then is
   * given up.
   */
  async shutdown(options: { timeoutMs?: number } = {}): Promise<void> {
    await this.#exporter.shutdown(options.timeoutMs)
  }

  // -------------------------------------------------------------------------
  // Reads
  // -------------------------------------------------------------------------

  /**
   * The trace list.
   *
   * Reads throw on failure, unlike the recording methods. A query is something
   * you asked for and are waiting on; silently returning an empty page would
   * hide the outage instead of reporting it.
   */
  async listTraces(query: TraceQuery = {}): Promise<TraceListResponse> {
    if (!this.#enabled) return { traces: [], page: { hasMore: false } }
    return await this.#transport.get<TraceListResponse>('/v1/api/traces', this.#scoped(query))
  }

  /** One trace and every span in it, ordered for a waterfall. */
  async getTrace(
    traceId: string,
    options: { include?: Array<'logs' | 'scores' | 'feedback' | 'metrics'> } = {},
  ): Promise<TraceDetail> {
    const query: Record<string, QueryValue> = {}
    if (options.include?.length) query['include'] = options.include.join(',')
    return await this.#transport.get<TraceDetail>(
      `/v1/api/traces/${encodeURIComponent(traceId)}`,
      this.#scoped(query),
    )
  }

  /** Search individual steps across every trace. */
  async listSpans(query: SpanQuery = {}): Promise<SpanListResponse> {
    if (!this.#enabled) return { spans: [], page: { hasMore: false } }
    return await this.#transport.get<SpanListResponse>('/v1/api/spans', this.#scoped(query))
  }

  /** Search logs. Every result carries the trace id that produced it. */
  async listLogs(query: LogQuery = {}): Promise<LogListResponse> {
    if (!this.#enabled) return { logs: [], page: { hasMore: false } }
    return await this.#transport.get<LogListResponse>('/v1/api/logs', this.#scoped(query))
  }

  /** A metric as a time series, aggregated by the daemon. */
  async queryMetrics(query: MetricQuery): Promise<MetricSeriesResponse> {
    if (!this.#enabled) return { name: query.name, bucket: query.bucket ?? '', series: [] }
    // Picked one by one rather than spread: the endpoint answers 400 to a key
    // it does not take, and a caller outside the type checker can pass any.
    const params = pick(query, ['project', 'name', 'from', 'to', 'bucket', 'agg', 'groupBy'])
    params['filter'] = metricFilter(query.filter)
    return await this.#transport.get<MetricSeriesResponse>('/v1/api/metrics', this.#scoped(params))
  }

  /** Machine evaluations. */
  async listScores(query: ScoreQuery = {}): Promise<ScoreListResponse> {
    if (!this.#enabled) return { scores: [], page: { hasMore: false } }
    return await this.#transport.get<ScoreListResponse>('/v1/api/scores', this.#scoped(query))
  }

  /**
   * The human judgements attached to one run.
   *
   * The daemon reads feedback one run at a time and refuses any other filter
   * with a 400, so `traceId` is required and nothing else is sent.
   */
  async listFeedback(query: FeedbackQuery): Promise<FeedbackListResponse> {
    if (!this.#enabled) return { feedback: [], page: { hasMore: false } }
    if (typeof query?.traceId !== 'string' || query.traceId.length === 0) {
      throw new McpdError('listFeedback needs a traceId: the daemon reads feedback one run at a time')
    }
    const params: Record<string, QueryValue> = { traceId: query.traceId }
    if (query.project !== undefined) params['project'] = query.project
    if (query.limit !== undefined) params['limit'] = query.limit
    return await this.#transport.get<FeedbackListResponse>('/v1/api/feedback', this.#scoped(params))
  }

  /** Run count, error rate, latency quantiles, tokens and cost for a window. */
  async analyticsSummary(query: WindowQuery): Promise<AnalyticsSummary> {
    return await this.#transport.get<AnalyticsSummary>(
      '/v1/api/analytics/summary',
      this.#scoped(pick(query, ['project', 'from', 'to', 'groupBy'])),
    )
  }

  /** Spend over a window, split by model, provider, agent, environment or user. */
  async analyticsCost(query: CostQuery): Promise<CostBreakdown> {
    return await this.#transport.get<CostBreakdown>(
      '/v1/api/analytics/cost',
      this.#scoped(pick(query, ['project', 'from', 'to', 'groupBy', 'bucket'])),
    )
  }

  /** Liveness. Never touches storage, so it answers even when the database is slow. */
  async health(): Promise<HealthStatus> {
    return await this.#transport.get<HealthStatus>('/healthz')
  }

  /** Readiness: storage is reachable and migrations have run. */
  async ready(): Promise<HealthStatus> {
    return await this.#transport.get<HealthStatus>('/readyz')
  }

  // -------------------------------------------------------------------------
  // SpanHost
  // -------------------------------------------------------------------------

  /** @internal */
  enqueueSpan(span: SpanData, key: string): void {
    if (!this.#enabled) return
    this.#push({ kind: 'span', key, value: span })
  }

  /** Whether `input` and `output` leave the process, unless a span says otherwise. */
  capturePayloads(): boolean {
    return this.#capture
  }

  /** The lowest level `log` records. Anything below it never reaches the queue. */
  get logLevel(): LogLevel {
    return this.#logLevel
  }

  /** @internal How often one kind of SDK failure may be reported. */
  get errorIntervalMs(): number {
    return this.#errorIntervalMs
  }

  /** @internal */
  diagnose(operation: string, error: unknown): void {
    this.#diagnostics.report(operation, error)
  }

  /**
   * What a log, a metric, a score or a piece of feedback is tagged with.
   *
   * The run's correlation when one is active, the client's when none is.
   * Taking the client's while inside a run is what left a log line with no
   * session on it, and a log you cannot filter by session is a log you cannot
   * find. The client's fields go on first so a span can add to them and never
   * erase one.
   */
  #signalCorrelation(active: McpdSpan | undefined): Correlation {
    const base: Correlation = { ...this.#correlation }
    if (active === undefined) return base
    // Field by field, never by spread. A span's correlation is allowed to
    // carry a key with no value, and copying that over the client's would take
    // a session the client could still have named off the log line.
    return mergeDefined(base, active.correlation)
  }

  /** @internal */
  baseCorrelation(): Correlation {
    return { ...this.#correlation }
  }

  // -------------------------------------------------------------------------
  // Internals
  // -------------------------------------------------------------------------

  #push(signal: Parameters<SignalQueue['push']>[0]): void {
    if (this.#redact !== undefined) signal = this.#redacted(signal)
    this.#queue.push(encodable(signal))
    this.#exporter.notify()
  }

  /**
   * The signal with `redact` applied to its payload fields, on a copy so the
   * caller's span keeps what it was given. Fails closed, field by field.
   */
  #redacted(signal: Parameters<SignalQueue['push']>[0]): Parameters<SignalQueue['push']>[0] {
    const keys =
      signal.kind === 'span'
        ? (['input', 'output', 'attributes', 'metadata'] as const)
        : signal.kind === 'log'
          ? (['fields'] as const)
          : ([] as const)
    if (keys.length === 0) return signal
    const value = { ...(signal.value as unknown as Record<string, unknown>) }
    for (const key of keys) {
      if (value[key] === undefined) continue
      try {
        const kept = this.#redact!(value[key])
        const mapField = key !== 'input' && key !== 'output'
        if (kept === undefined || kept === null) delete value[key]
        else if (mapField && (typeof kept !== 'object' || Array.isArray(kept))) {
          // A map field the redactor turned into something else cannot be
          // stored as one, so it fails closed like a throw does.
          delete value[key]
          this.#redactionFailures++
        } else value[key] = kept
      } catch (error) {
        delete value[key]
        this.#redactionFailures++
        this.#diagnostics.report('redact', error)
      }
    }
    return { ...signal, value } as unknown as Parameters<SignalQueue['push']>[0]
  }

  /** The root sampling decision (B4). */
  #decide(): boolean {
    if (!this.#enabled) return false
    if (this.#sample >= 1) return true
    if (this.#sample <= 0) return false
    return Math.random() < this.#sample
  }

  /** Add the project to a query unless the caller named one. */
  #scoped<T extends Record<string, QueryValue>>(query: T): Record<string, QueryValue> {
    return { project: this.#project, ...query }
  }

  #guard<T>(operation: string, apply: () => T, fallback: () => T): T {
    try {
      return apply()
    } catch (error) {
      this.#diagnostics.report(operation, error)
      return fallback()
    }
  }

  /**
   * A span that is valid in every way except that it records nothing.
   *
   * Returned when span creation itself failed. The caller keeps a real object
   * with real ids and never has to check for null.
   */
  #inertSpan(name: string): McpdSpan {
    return createSpan(
      this,
      name,
      { traceId: newTraceId(), spanId: newSpanId(), sampled: false },
      undefined,
      'agent_run',
      {},
    )
  }

  /**
   * Flush when Node's event loop empties.
   *
   * A script that forgets `shutdown()` still gets its last batch. `beforeExit`
   * does not fire on an explicit `process.exit()` or on a fatal signal, so it
   * is a safety net and not a replacement for calling `shutdown()`.
   */
  #registerExitHook(): void {
    const process = (globalThis as {
      process?: { once?: (event: string, handler: () => void) => void }
    }).process
    if (typeof process?.once !== 'function') return
    process.once('beforeExit', () => {
      void this.#exporter.flush()
    })
  }
}

/**
 * A signal whose caller-supplied values `JSON.stringify` is certain to accept.
 *
 * A span's input, output, attributes and metadata, a score's metadata and a
 * piece of feedback's value can be anything the caller had to hand. A cycle
 * or a `BigInt` among them made `JSON.stringify` throw inside the transport,
 * after the batch was assembled, and every signal that shared the batch was
 * lost with it. Encoded here, as a log line's fields already are, one value
 * that has no JSON form costs its own key and nothing else (B1, section 4.1).
 */
function encodable(signal: Signal): Signal {
  switch (signal.kind) {
    case 'span': {
      const value = { ...signal.value }
      if (value.input !== undefined) value.input = encodeFields(value.input)
      if (value.output !== undefined) value.output = encodeFields(value.output)
      if (value.attributes !== undefined) value.attributes = encodeFields(value.attributes)
      if (value.metadata !== undefined) value.metadata = encodeFields(value.metadata)
      return { ...signal, value }
    }
    case 'score': {
      if (signal.value.metadata === undefined) return signal
      return { ...signal, value: { ...signal.value, metadata: encodeFields(signal.value.metadata) } }
    }
    case 'feedback': {
      const value = { ...signal.value, value: encodeFields(signal.value.value) }
      if (value.metadata !== undefined) value.metadata = encodeFields(value.metadata)
      return { ...signal, value }
    }
    default:
      return signal
  }
}

/** The named keys of a read query and nothing else, ready for the transport. */
function pick<T extends object>(query: T, keys: ReadonlyArray<keyof T & string>): Record<string, QueryValue> {
  const out: Record<string, QueryValue> = {}
  for (const key of keys) {
    const value = query[key] as QueryValue
    if (value !== undefined) out[key] = value
  }
  return out
}

/**
 * Label filters the way the daemon reads them: `key:value` joined by commas.
 *
 * A key or value the format cannot carry is refused rather than sent, because
 * the daemon would split it into a different filter and answer a different
 * question.
 */
function metricFilter(filter: Record<string, string> | undefined): string | undefined {
  if (filter === undefined) return undefined
  const pairs: string[] = []
  for (const [key, raw] of Object.entries(filter)) {
    const value = String(raw)
    if (key === '' || key.includes(':') || key.includes(',')) {
      throw new McpdError(`metric filter key ${JSON.stringify(key)} cannot be empty or contain ':' or ','`)
    }
    if (value.includes(',')) {
      throw new McpdError(`metric filter value ${JSON.stringify(value)} for ${JSON.stringify(key)} cannot contain ','`)
    }
    pairs.push(`${key}:${value}`)
  }
  return pairs.length > 0 ? pairs.join(',') : undefined
}

/** Read a span context out of whatever the caller passed as a parent. */
function toContext(parent: McpdSpan | SpanContext): SpanContext {
  return parent instanceof McpdSpan ? parent.context() : parent
}

/**
 * Why a span handed to `record()` cannot be queued, or nothing.
 *
 * The daemon would refuse each of these at ingest and take the whole batch's
 * neighbours down with it, so they are refused here, one span at a time.
 */
function refuseRecord(span: SpanData): string | undefined {
  if (span === null || typeof span !== 'object') return 'not an object'
  if (!isTraceId(span.traceId)) return 'trace_id is not 32 lowercase hex, or is all zero'
  if (!isSpanId(span.spanId)) return 'span_id is not 16 lowercase hex, or is all zero'
  if (span.parentSpanId !== undefined && !isSpanId(span.parentSpanId)) {
    return 'parent_span_id is not 16 lowercase hex, or is all zero'
  }
  if (typeof span.name !== 'string' || span.name.length === 0) return 'name is empty'
  if (typeof span.startTime !== 'string' || span.startTime.length === 0) {
    return 'start_time is missing'
  }
  return undefined
}

/**
 * The default sink: the HTTP transport.
 *
 * A daemon that accepted the batch but refused some of its signals answers
 * 200 with an error list, which is reported here so a malformed signal is
 * news and not a silent gap.
 */
class TransportSink implements Sink {
  readonly #transport: Transport
  readonly #diagnostics: Diagnostics

  constructor(transport: Transport, diagnostics: Diagnostics) {
    this.#transport = transport
    this.#diagnostics = diagnostics
  }

  async export(batch: IngestBatch, ingestKey: string): Promise<void> {
    const result = await this.#transport.ingest(batch, ingestKey)
    if (result?.errors?.length) {
      const count =
        (batch.spans?.length ?? 0) +
        (batch.logs?.length ?? 0) +
        (batch.metrics?.length ?? 0) +
        (batch.scores?.length ?? 0) +
        (batch.feedback?.length ?? 0)
      this.#diagnostics.report(
        `daemon rejected ${result.rejected} of ${count} signals: ${result.errors[0]?.message ?? 'no reason given'}`,
        undefined,
      )
    }
  }
}

// ---------------------------------------------------------------------------
// Read query shapes
// ---------------------------------------------------------------------------

/** Common paging and window filters. */
export interface BaseQuery extends Record<string, QueryValue> {
  /** Which project. Defaults to the client's. */
  project?: string
  /** Start of the window, inclusive. RFC 3339. */
  from?: string
  /** End of the window, exclusive. RFC 3339. */
  to?: string
  /** Page size. */
  limit?: number
  /** Opaque cursor from a previous page. */
  cursor?: string
}

export interface TraceQuery extends BaseQuery {
  /** Full text across span names, prompts and completions. */
  q?: string
  status?: string
  kind?: SpanKind
  name?: string
  tags?: string[]
  sessionId?: string
  threadId?: string
  userId?: string
  environment?: string
  model?: string
  minDurationMs?: number
  minCost?: number
  /** Only traces carrying a score from this scorer. */
  hasScore?: string
  /** With `hasScore`: only traces that scorer put below this. Finds bad answers. */
  scoreBelow?: number
  sort?: string
  order?: 'asc' | 'desc'
}

export interface SpanQuery extends BaseQuery {
  q?: string
  kind?: SpanKind
  status?: string
  name?: string
  model?: string
  traceId?: string
  minDurationMs?: number
  sort?: string
  order?: 'asc' | 'desc'
}

export interface LogQuery extends BaseQuery {
  q?: string
  /** Minimum severity, inclusive. */
  level?: LogLevel
  traceId?: string
  spanId?: string
}

/**
 * A metric series query. The endpoint takes no page size and no cursor, and
 * answers 400 to either, so this does not extend `BaseQuery`.
 */
export interface MetricQuery {
  /** Which project. Defaults to the client's. */
  project?: string
  name: string
  from: string
  to: string
  /** Bucket width, such as `1m` or `1h`. Chosen for you when omitted. */
  bucket?: string
  /** `sum`, `count`, `mean`, `min`, `max`, `p50`, `p90`, `p95` or `p99`. */
  agg?: string
  groupBy?: string[]
  /**
   * Keep points whose labels have these values. Sent as `key:value` pairs
   * joined by commas, so a key cannot contain `:` or `,` and a value cannot
   * contain `,`; either throws before a request is made.
   */
  filter?: Record<string, string>
}

export interface ScoreQuery extends BaseQuery {
  scorerId?: string
  source?: ScoreSource
  traceId?: string
  minValue?: number
  maxValue?: number
}

/**
 * The feedback list's parameters, and only those. The endpoint reads one run's
 * judgements and refuses a window, a cursor or a kind with a 400.
 */
export interface FeedbackQuery {
  /** The run whose judgements to read. */
  traceId: string
  /** Which project. Defaults to the client's. */
  project?: string
  /** Page size. */
  limit?: number
}

/**
 * An analytics window. The analytics endpoints are not paged, so this does not
 * extend `BaseQuery` and carries no page size or cursor.
 */
export interface WindowQuery {
  /** Which project. Defaults to the client's. */
  project?: string
  from: string
  to: string
  /** Split by `model`, `entity_name`, `environment`, `status` or `session_id`. */
  groupBy?: string
}

export interface CostQuery extends WindowQuery {
  /** Time bucket for a series, such as `1h`. Omit for totals only. */
  bucket?: string
}
