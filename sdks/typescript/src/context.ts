/**
 * Identifiers, clocks, W3C trace context, and the ambient span store.
 *
 * Three things live here because they are the parts of the SDK that must know
 * which runtime they are on. Everything else in the package is plain
 * JavaScript that runs unchanged in Node, Deno, Bun, a browser and an edge
 * worker.
 */

/** A reference to one span, in the shape W3C `traceparent` carries. */
export interface SpanContext {
  /** 32 lowercase hex characters. */
  traceId: string
  /** 16 lowercase hex characters. */
  spanId: string
  /** The sampling decision, made once at the root and inherited (B4). */
  sampled: boolean
}

// ---------------------------------------------------------------------------
// Identifiers
// ---------------------------------------------------------------------------

const HEX = '0123456789abcdef'

/**
 * Fill a hex string of `bytes * 2` characters.
 *
 * `crypto.getRandomValues` is present in Node 20, Deno, Bun, browsers and every
 * edge runtime we target. The `Math.random` path exists so that an exotic
 * runtime degrades to a working id rather than to a thrown error: a weaker id
 * still correlates, and instrumentation must never throw (B1).
 */
function randomHex(bytes: number): string {
  const buf = new Uint8Array(bytes)
  const c = (globalThis as { crypto?: Crypto }).crypto
  if (c && typeof c.getRandomValues === 'function') {
    c.getRandomValues(buf)
  } else {
    for (let i = 0; i < bytes; i++) buf[i] = Math.floor(Math.random() * 256)
  }
  let out = ''
  for (let i = 0; i < bytes; i++) {
    const b = buf[i] as number
    out += HEX[b >> 4] as string
    out += HEX[b & 0x0f] as string
  }
  return out
}

/** True when every character is `0`. Both id spaces reserve that value. */
function isAllZero(id: string): boolean {
  for (let i = 0; i < id.length; i++) if (id[i] !== '0') return false
  return true
}

/**
 * A new 128-bit trace id.
 *
 * The all-zero id means "no trace" in OpenTelemetry and is discarded by every
 * consumer, so on the vanishingly rare occasion the generator produces it we
 * draw again.
 */
export function newTraceId(): string {
  for (;;) {
    const id = randomHex(16)
    if (!isAllZero(id)) return id
  }
}

/** A new 64-bit span id, with the same all-zero guarantee. */
export function newSpanId(): string {
  for (;;) {
    const id = randomHex(8)
    if (!isAllZero(id)) return id
  }
}

function isHexId(value: unknown, length: number): boolean {
  if (typeof value !== 'string' || value.length !== length) return false
  for (let i = 0; i < length; i++) {
    const ch = value.charCodeAt(i)
    const ok = (ch >= 48 && ch <= 57) || (ch >= 97 && ch <= 102)
    if (!ok) return false
  }
  return !isAllZero(value)
}

/** True when `value` is 32 lowercase hex characters and not all zero. */
export function isTraceId(value: unknown): value is string {
  return isHexId(value, 32)
}

/** True when `value` is 16 lowercase hex characters and not all zero. */
export function isSpanId(value: unknown): value is string {
  return isHexId(value, 16)
}

// ---------------------------------------------------------------------------
// W3C trace context
// ---------------------------------------------------------------------------

/**
 * Parse a W3C `traceparent` header.
 *
 * Returns `undefined` for anything malformed rather than throwing, because the
 * header arrives from another system and a caller who passes a bad one should
 * get a new root trace, not an exception (B1, B7).
 *
 * Version `00` is the only one defined. Later versions are required to keep the
 * first four fields, so an unknown version with a well-formed prefix is
 * accepted; version `ff` is explicitly invalid.
 */
export function parseTraceparent(header: string | null | undefined): SpanContext | undefined {
  if (typeof header !== 'string') return undefined
  const parts = header.trim().split('-')
  if (parts.length < 4) return undefined
  const [version, traceId, spanId, flags] = parts as [string, string, string, string]
  if (version.length !== 2 || version === 'ff') return undefined
  if (!isTraceId(traceId) || !isSpanId(spanId)) return undefined
  if (flags.length !== 2) return undefined
  const parsedFlags = Number.parseInt(flags, 16)
  if (Number.isNaN(parsedFlags)) return undefined
  return { traceId, spanId, sampled: (parsedFlags & 0x01) === 0x01 }
}

/** Render a span context as a W3C `traceparent` header value. */
export function formatTraceparent(ctx: SpanContext): string {
  return `00-${ctx.traceId}-${ctx.spanId}-${ctx.sampled ? '01' : '00'}`
}

// ---------------------------------------------------------------------------
// Clock
// ---------------------------------------------------------------------------

const perf = (globalThis as { performance?: { now(): number; timeOrigin?: number } }).performance
const originNs =
  perf && typeof perf.timeOrigin === 'number' && Number.isFinite(perf.timeOrigin)
    ? BigInt(Math.round(perf.timeOrigin * 1e6))
    : undefined

/**
 * UTC nanoseconds since the epoch.
 *
 * `Date.now()` is capped at milliseconds, which is too coarse to order the
 * spans of a fast agent step. `performance.timeOrigin` plus `performance.now()`
 * keeps sub-millisecond resolution and, unlike `Date.now()`, cannot go
 * backwards when the wall clock is adjusted mid-run (B9).
 */
export function nowNs(): bigint {
  if (originNs !== undefined && perf) {
    return originNs + BigInt(Math.round(perf.now() * 1e6))
  }
  return BigInt(Date.now()) * 1_000_000n
}

/**
 * Render UTC nanoseconds as RFC 3339 with nine fractional digits.
 *
 * The daemon parses RFC 3339 and stores nanoseconds, so emitting the full nine
 * digits is what stops a language boundary from quietly truncating a timestamp
 * to milliseconds.
 */
export function formatTimestamp(ns: bigint): string {
  const seconds = ns / 1_000_000_000n
  const fraction = ns % 1_000_000_000n
  const whole = new Date(Number(seconds) * 1000).toISOString().slice(0, 19)
  return `${whole}.${fraction.toString().padStart(9, '0')}Z`
}

/** The current instant, as the daemon wants to read it. */
export function nowTimestamp(): string {
  return formatTimestamp(nowNs())
}

// ---------------------------------------------------------------------------
// Ambient context
// ---------------------------------------------------------------------------

/**
 * Where the active span is kept for the duration of a callback.
 *
 * Two implementations, chosen at runtime:
 *
 * - `async-local-storage`, on Node and Bun. Correct under concurrency: two
 *   agent runs interleaved on the same event loop each see their own span.
 * - `fallback`, everywhere else. A single saved-and-restored variable. It nests
 *   correctly, and it is correct across `await` points inside one run, but two
 *   runs that interleave can see each other's span. On those runtimes pass the
 *   parent explicitly with `span.child()` or the `parent` option when you run
 *   agents concurrently.
 */
export interface ContextStore {
  readonly kind: 'async-local-storage' | 'fallback'
  active<T>(): T | undefined
  run<T, R>(value: T, fn: () => R): R
}

class FallbackStore implements ContextStore {
  readonly kind = 'fallback' as const
  #current: unknown = undefined

  active<T>(): T | undefined {
    return this.#current as T | undefined
  }

  run<T, R>(value: T, fn: () => R): R {
    const previous = this.#current
    this.#current = value
    let result: R
    try {
      result = fn()
    } catch (error) {
      this.#current = previous
      throw error
    }
    // An async callback returns at its first `await`, long before its work is
    // done. Holding the value until the promise settles is what makes nesting
    // work at all without `AsyncLocalStorage`.
    if (isThenable(result)) {
      return (result as Promise<unknown>).then(
        (value) => {
          this.#current = previous
          return value
        },
        (error: unknown) => {
          this.#current = previous
          throw error
        },
      ) as R
    }
    this.#current = previous
    return result
  }
}

function isThenable(value: unknown): boolean {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as { then?: unknown }).then === 'function'
  )
}

interface AsyncLocalStorageLike {
  getStore(): unknown
  run<R>(store: unknown, fn: () => R): R
}

class AsyncLocalStore implements ContextStore {
  readonly kind = 'async-local-storage' as const
  readonly #als: AsyncLocalStorageLike

  constructor(als: AsyncLocalStorageLike) {
    this.#als = als
  }

  active<T>(): T | undefined {
    return this.#als.getStore() as T | undefined
  }

  run<T, R>(value: T, fn: () => R): R {
    return this.#als.run(value, fn)
  }
}

/**
 * A store that starts as the fallback and upgrades itself to
 * `AsyncLocalStorage` as soon as the dynamic import resolves.
 *
 * The import cannot be static: `node:async_hooks` does not exist in a browser
 * or in most edge runtimes, and a static import would fail the bundle before
 * any code ran. It cannot be top-level `await` either, because that would make
 * the module impossible to emit as CommonJS. So it is started eagerly and the
 * fallback covers the microtask or two before it lands. Anything that awaits,
 * which includes every `withSpan` callback, runs on the upgraded store.
 */
class UpgradingStore implements ContextStore {
  #inner: ContextStore = new FallbackStore()

  get kind(): 'async-local-storage' | 'fallback' {
    return this.#inner.kind
  }

  active<T>(): T | undefined {
    return this.#inner.active<T>()
  }

  run<T, R>(value: T, fn: () => R): R {
    return this.#inner.run(value, fn)
  }

  /**
   * Swap in `AsyncLocalStorage`, but never while a span is open.
   *
   * Changing stores under a running callback would strand its context in the
   * old one, and the child spans started after the swap would come out as
   * roots. In practice the swap lands in the first few microtasks, before any
   * span exists.
   */
  adopt(als: AsyncLocalStorageLike): void {
    if (this.#inner.kind !== 'fallback') return
    if (this.#inner.active() !== undefined) return
    this.#inner = new AsyncLocalStore(als)
  }
}

const store = new UpgradingStore()

// The specifier is built at runtime so that a bundler targeting the browser
// cannot see a Node builtin here and fail the build over it.
const asyncHooks = 'node:async_' + 'hooks'

const probe: Promise<void> = (async () => {
  try {
    const mod = (await import(/* @vite-ignore */ /* webpackIgnore: true */ asyncHooks)) as {
      AsyncLocalStorage?: new () => AsyncLocalStorageLike
    }
    if (mod.AsyncLocalStorage) store.adopt(new mod.AsyncLocalStorage())
  } catch {
    // No async_hooks here. The fallback store is already in place.
  }
})()

/** The process-wide context store. */
export function contextStore(): ContextStore {
  return store
}

/**
 * Resolves once the store has settled on an implementation.
 *
 * `withSpan` awaits this before it opens anything, which costs one microtask
 * and buys a store that never changes underneath a running callback.
 */
export function contextReady(): Promise<void> {
  return probe
}
