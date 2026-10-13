/**
 * The wire. HTTP, gzip, retry, and the one translation this SDK has to do.
 *
 * The daemon speaks `snake_case` JSON; the generated TypeScript model is
 * `camelCase`, because that is what TypeScript users expect to write. The two
 * are converted here and nowhere else, so no other module has to think about
 * it.
 */

import type { IngestBatch, IngestResult } from './generated.js'

/** A failed request. Thrown by the read methods, swallowed by the exporter. */
export class McpdError extends Error {
  /** HTTP status, when the request got that far. */
  readonly status: number | undefined
  /** Whether sending the same bytes again could plausibly work. */
  readonly retryable: boolean
  /** Response body, truncated. Servers put the actionable part first. */
  readonly body: string | undefined

  constructor(
    message: string,
    options: { status?: number; retryable?: boolean; body?: string; cause?: unknown } = {},
  ) {
    super(message, options.cause === undefined ? undefined : { cause: options.cause })
    this.name = 'McpdError'
    this.status = options.status
    this.retryable = options.retryable ?? false
    this.body = options.body
  }
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/** Where the SDK reports its own failures. */
export interface Diagnostics {
  report(key: string, error: unknown): void
}

/**
 * Reports each kind of failure at most once per interval (B1).
 *
 * A daemon that has been down for an hour must not produce an hour of log
 * lines. The key is a category, not a message, so a thousand identical export
 * failures collapse to one line and a count.
 */
export class RateLimitedDiagnostics implements Diagnostics {
  readonly #intervalMs: number
  readonly #sink: ((message: string, error: unknown) => void) | undefined
  readonly #last = new Map<string, number>()
  readonly #suppressed = new Map<string, number>()

  constructor(intervalMs: number, sink?: (message: string, error: unknown) => void) {
    this.#intervalMs = intervalMs
    this.#sink = sink
  }

  report(key: string, error: unknown): void {
    try {
      const now = Date.now()
      const last = this.#last.get(key)
      if (last !== undefined && now - last < this.#intervalMs) {
        this.#suppressed.set(key, (this.#suppressed.get(key) ?? 0) + 1)
        return
      }
      this.#last.set(key, now)
      const hidden = this.#suppressed.get(key) ?? 0
      this.#suppressed.delete(key)
      const suffix = hidden > 0 ? ` (${hidden} more like it were not logged)` : ''
      const message = `mcpd: ${key}${suffix}`
      if (this.#sink) this.#sink(message, error)
      else console.warn(message, error)
    } catch {
      // Reporting a failure must never become a failure of its own.
    }
  }
}

// ---------------------------------------------------------------------------
// Key case
// ---------------------------------------------------------------------------

/**
 * Field names whose value is free-form JSON the daemon never interprets.
 *
 * Conversion stops at these keys. A user's prompt may contain `{"userName":
 * "ada"}`, and rewriting that to `user_name` would corrupt the single most
 * valuable thing mcpd stores. Both spellings are listed so the rule holds in
 * each direction.
 */
const OPAQUE_KEYS = new Set([
  'attributes',
  // A definition document: the daemon parses it, in its own spelling.
  'body',
  'budget',
  'buckets',
  'details',
  'expected',
  'feedbackSummary',
  'feedback_summary',
  'fields',
  'input',
  'labels',
  'metadata',
  'output',
  // A run event's detail, and a lease's prior steps and run state: keyed by
  // step ids the user chose, which must reach the body spelled as written.
  'payload',
  'prior',
  'redaction',
  'sampling',
  'scoreSummary',
  'score_summary',
  'state',
  'summary',
  'value',
])

/**
 * Names the mechanical rule gets wrong.
 *
 * `cacheWrite5m` would become `cache_write5m`, because a digit is not an
 * uppercase letter. These two are the only irregular names in the spec; a
 * general digit rule would break `p50`.
 */
const SNAKE_OVERRIDES: Record<string, string> = {
  cacheWrite5m: 'cache_write_5m',
  cacheWrite1h: 'cache_write_1h',
}

const snakeCache = new Map<string, string>()
const camelCache = new Map<string, string>()

/** `sessionId` becomes `session_id`. */
export function snakeKey(key: string): string {
  const hit = snakeCache.get(key)
  if (hit !== undefined) return hit
  const out =
    SNAKE_OVERRIDES[key] ?? key.replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase()
  snakeCache.set(key, out)
  return out
}

/** `session_id` becomes `sessionId`. */
export function camelKey(key: string): string {
  const hit = camelCache.get(key)
  if (hit !== undefined) return hit
  const out = key.replace(/_([a-z0-9])/g, (_m, c: string) => c.toUpperCase())
  camelCache.set(key, out)
  return out
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (value === null || typeof value !== 'object') return false
  if (Array.isArray(value)) return false
  const proto = Object.getPrototypeOf(value) as object | null
  return proto === Object.prototype || proto === null
}

/**
 * `OPAQUE_KEYS` plus the free-form fields of a prompt.
 *
 * Kept apart because `config` is not opaque everywhere: a scorer's `config`
 * is typed and has to be converted. A prompt's `messages` and `config` are
 * the user's own JSON, sent to a model as written.
 */
export const PROMPT_OPAQUE_KEYS: ReadonlySet<string> = new Set([...OPAQUE_KEYS, 'messages', 'config'])

function convert(value: unknown, key: (k: string) => string, opaque: ReadonlySet<string>): unknown {
  if (Array.isArray(value)) return value.map((v) => convert(v, key, opaque))
  if (!isPlainObject(value)) return value
  const out: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(value)) {
    if (v === undefined) continue
    out[key(k)] = opaque.has(k) ? v : convert(v, key, opaque)
  }
  return out
}

/** Convert a value from the SDK's camelCase model into the daemon's shape. */
export function toWire<T>(value: T, opaque: ReadonlySet<string> = OPAQUE_KEYS): unknown {
  return convert(value, snakeKey, opaque)
}

/** Convert a daemon response into the SDK's camelCase model. */
export function fromWire<T>(value: unknown, opaque: ReadonlySet<string> = OPAQUE_KEYS): T {
  return convert(value, camelKey, opaque) as T
}

// ---------------------------------------------------------------------------
// gzip
// ---------------------------------------------------------------------------

// Built at runtime so a browser bundler does not try to resolve a Node builtin.
const ZLIB = 'node:' + 'zlib'

type GzipFn = (data: Uint8Array) => Uint8Array
let gzipLoad: Promise<GzipFn | undefined> | undefined

/**
 * `node:zlib`'s `gzipSync`, or `undefined` off Node.
 *
 * Compression is an optimisation, never a requirement: where it is not
 * available the body goes out uncompressed and the daemon accepts it.
 */
function loadGzip(): Promise<GzipFn | undefined> {
  gzipLoad ??= (async () => {
    try {
      const zlib = (await import(/* @vite-ignore */ /* webpackIgnore: true */ ZLIB)) as {
        gzipSync?: (buf: Uint8Array, opts?: { level?: number }) => Uint8Array
      }
      if (typeof zlib.gzipSync !== 'function') return undefined
      const fn = zlib.gzipSync
      // Level 6 is zlib's default: the point where more CPU stops buying
      // meaningful size on JSON payloads of this shape.
      return (data: Uint8Array) => fn(data, { level: 6 })
    } catch {
      return undefined
    }
  })()
  return gzipLoad
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

/** What a query parameter may be. Arrays become repeated keys. */
export type QueryValue = string | number | boolean | string[] | undefined

/** The subset of `fetch` this SDK uses. Injectable so tests need no server. */
export type FetchLike = (input: string, init: RequestInit) => Promise<Response>

export interface TransportConfig {
  endpoint: string
  apiKey?: string | undefined
  timeoutMs: number
  fetchImpl?: FetchLike | undefined
  diagnostics: Diagnostics
  /** Total attempts, including the first. */
  attempts?: number
  /** First backoff delay in milliseconds; doubles each attempt. */
  baseBackoffMs?: number
  /** Bodies larger than this are gzipped. */
  compressAbove?: number
  userAgent?: string
}

/** Statuses worth sending the same bytes at again. Anything else 4xx will not fix itself. */
const RETRY_STATUS = new Set([429, 502, 503, 504])

/**
 * A backoff delay.
 *
 * Deliberately not `unref`ed: a flush that is waiting to retry is work the
 * process must stay alive for, and an unreferenced timer would let Node exit
 * with the batch still in hand.
 */
const sleep = (ms: number): Promise<void> =>
  new Promise((resolve) => {
    setTimeout(resolve, ms)
  })

function timeoutSignal(ms: number): { signal: AbortSignal; cancel: () => void } {
  const withTimeout = AbortSignal as unknown as { timeout?: (ms: number) => AbortSignal }
  if (typeof withTimeout.timeout === 'function') {
    return { signal: withTimeout.timeout(ms), cancel: () => {} }
  }
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), ms)
  return { signal: controller.signal, cancel: () => clearTimeout(timer) }
}

/** How long a `Retry-After` header asks us to wait, in milliseconds. */
function retryAfterMs(response: Response): number | undefined {
  const raw = response.headers.get('retry-after')
  if (!raw) return undefined
  const seconds = Number(raw)
  if (Number.isFinite(seconds) && seconds >= 0) return seconds * 1000
  const at = Date.parse(raw)
  if (!Number.isNaN(at)) return Math.max(0, at - Date.now())
  return undefined
}

export class Transport {
  readonly #config: TransportConfig
  readonly #attempts: number
  readonly #baseBackoffMs: number
  readonly #compressAbove: number

  constructor(config: TransportConfig) {
    this.#config = config
    this.#attempts = config.attempts ?? 3
    this.#baseBackoffMs = config.baseBackoffMs ?? 250
    this.#compressAbove = config.compressAbove ?? 1024
  }

  /** The base URL, with any trailing slash removed so paths concatenate cleanly. */
  get endpoint(): string {
    return this.#config.endpoint.replace(/\/+$/, '')
  }

  #fetch(): FetchLike {
    const injected = this.#config.fetchImpl
    if (injected) return injected
    const global = (globalThis as { fetch?: FetchLike }).fetch
    if (!global) {
      throw new McpdError(
        'no fetch implementation: pass one as the `fetch` option, or run on Node 20 or later',
      )
    }
    // Bound, because an unbound `globalThis.fetch` throws "illegal invocation"
    // in browsers.
    return (input, init) => global.call(globalThis, input, init)
  }

  #headers(extra: Record<string, string>): Record<string, string> {
    const headers: Record<string, string> = { accept: 'application/json', ...extra }
    if (this.#config.apiKey) headers['authorization'] = `Bearer ${this.#config.apiKey}`
    if (this.#config.userAgent) headers['user-agent'] = this.#config.userAgent
    return headers
  }

  /**
   * Send one batch to the native ingest endpoint.
   *
   * `ingestKey` is chosen by the caller and stays the same across retries, so a
   * batch that timed out after the daemon had already stored it is recognised
   * as a replay instead of being counted twice.
   */
  async ingest(batch: IngestBatch, ingestKey: string): Promise<IngestResult> {
    const payload = JSON.stringify(toWire({ ...batch, ingestKey }))
    const raw = new TextEncoder().encode(payload)

    // The uncompressed path sends the string straight through, so the encode
    // above is only ever paid for to measure the body and to feed gzip.
    let body: BodyInit = payload
    const headers: Record<string, string> = { 'content-type': 'application/json' }
    if (raw.byteLength > this.#compressAbove) {
      const gzip = await loadGzip()
      if (gzip) {
        // `BodyInit` wants a view over a non-shared ArrayBuffer, which is what
        // zlib returns. Its declared type is merely wider than that.
        body = gzip(raw) as unknown as BodyInit
        headers['content-encoding'] = 'gzip'
      }
    }

    return await this.#send<IngestResult>('POST', '/v1/ingest', {
      headers,
      body,
    })
  }

  /** A read request. Query values are converted to the daemon's key case. */
  async get<T>(
    path: string,
    query?: Record<string, QueryValue>,
    options: { opaque?: ReadonlySet<string> } = {},
  ): Promise<T> {
    return await this.#send<T>('GET', path + encodeQuery(query), { opaque: options.opaque })
  }

  /**
   * A write request that is not a signal batch.
   *
   * `waitMs` is how long the daemon may hold the request open before it
   * answers, as it does for a long poll. The request's timeout is that plus
   * the configured timeout; otherwise the client would cancel a poll longer
   * than its own timeout, which is every idle claim.
   */
  async post<T>(
    path: string,
    payload: unknown,
    options: { waitMs?: number | undefined; opaque?: ReadonlySet<string> } = {},
  ): Promise<T> {
    return await this.#send<T>('POST', path, {
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(toWire(payload, options.opaque)),
      waitMs: options.waitMs,
      opaque: options.opaque,
    })
  }

  /** A write request that replaces a resource, such as a prompt label. */
  async put<T>(path: string, payload: unknown, options: { opaque?: ReadonlySet<string> } = {}): Promise<T> {
    return await this.#send<T>('PUT', path, {
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(toWire(payload, options.opaque)),
      opaque: options.opaque,
    })
  }

  async #send<T>(
    method: string,
    path: string,
    init: {
      headers?: Record<string, string>
      body?: BodyInit
      waitMs?: number | undefined
      opaque?: ReadonlySet<string> | undefined
    } = {},
  ): Promise<T> {
    const url = this.endpoint + path
    const fetchImpl = this.#fetch()
    let lastError: McpdError | undefined

    for (let attempt = 0; attempt < this.#attempts; attempt++) {
      // The outcome is collected rather than thrown, so that a failure we
      // raised ourselves is never confused with a failure `fetch` raised.
      let outcome:
        | { ok: true; text: string }
        | { ok: false; error: McpdError; waitMs?: number | undefined }
        | undefined
      const { signal, cancel } = timeoutSignal(this.#config.timeoutMs + (init.waitMs ?? 0))
      try {
        const response = await fetchImpl(url, {
          method,
          headers: this.#headers(init.headers ?? {}),
          body: init.body,
          signal,
        })
        if (response.ok) {
          outcome = { ok: true, text: await response.text() }
        } else {
          const body = (await response.text().catch(() => '')).slice(0, 512)
          outcome = {
            ok: false,
            error: new McpdError(`${method} ${path} failed with ${response.status}`, {
              status: response.status,
              retryable: RETRY_STATUS.has(response.status),
              body,
            }),
            waitMs: retryAfterMs(response),
          }
        }
      } catch (error) {
        // A connection error, a DNS failure or our own timeout. All three are
        // worth another attempt, and all three look the same from here.
        outcome = {
          ok: false,
          error: new McpdError(`${method} ${path} could not reach ${this.endpoint}`, {
            retryable: true,
            cause: error,
          }),
        }
      } finally {
        cancel()
      }

      if (outcome.ok) {
        if (!outcome.text) return undefined as T
        try {
          return fromWire<T>(JSON.parse(outcome.text), init.opaque)
        } catch (error) {
          // A 200 that is not JSON is a proxy or a captive portal, not mcpd.
          throw new McpdError(`${method} ${path} returned a body that is not JSON`, {
            cause: error,
            body: outcome.text.slice(0, 512),
          })
        }
      }

      lastError = outcome.error
      if (!outcome.error.retryable || attempt === this.#attempts - 1) throw outcome.error
      // A server that named a delay knows better than our curve does.
      await sleep(Math.min(outcome.waitMs ?? this.#backoff(attempt), 60_000))
    }

    throw lastError ?? new McpdError(`${method} ${path} failed`)
  }

  /** Exponential backoff with jitter, so a fleet does not retry in lockstep. */
  #backoff(attempt: number): number {
    const ceiling = Math.min(this.#baseBackoffMs * 2 ** attempt, 30_000)
    return ceiling * (0.5 + Math.random() * 0.5)
  }
}

/** Render query parameters, converting keys and dropping empty values. */
export function encodeQuery(query: Record<string, QueryValue> | undefined): string {
  if (!query) return ''
  const parts: string[] = []
  for (const [key, value] of Object.entries(query)) {
    if (value === undefined || value === null || value === '') continue
    const name = snakeKey(key)
    // One comma-joined value, never a repeated key. The daemon reads the query
    // as a map, so `tags=a&tags=b` keeps only `b` and answers for one tag
    // instead of both.
    const rendered = Array.isArray(value)
      ? value.filter((v) => v !== undefined && v !== null && v !== '').join(',')
      : String(value)
    if (rendered === '') continue
    parts.push(`${encodeURIComponent(name)}=${encodeURIComponent(rendered)}`)
  }
  return parts.length > 0 ? `?${parts.join('&')}` : ''
}
