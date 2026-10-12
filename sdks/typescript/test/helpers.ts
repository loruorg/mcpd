import type { FetchLike, Span } from '../src/index.js'

/** One captured request, with its body already parsed back into an object. */
export interface Captured {
  url: string
  method: string
  headers: Record<string, string>
  body: unknown
}

export interface FakeFetch {
  fetch: FetchLike
  calls: Captured[]
  /** Every span in every batch that was sent, in order. */
  spans(): Array<Record<string, unknown>>
}

// Resolved through a variable so TypeScript does not need Node's ambient types
// to check this file, and so the SDK's own zero-dependency rule is not bent.
const zlib = await (import('node:' + 'zlib') as Promise<{
  gunzipSync: (data: Uint8Array) => Uint8Array
}>)

function response(status: number, body: unknown, headers: Record<string, string> = {}): Response {
  return new Response(body === undefined ? '' : JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json', ...headers },
  })
}

/**
 * A `fetch` that records what it was given and answers from a script.
 *
 * Tests never need a listening socket, so they stay fast and never bind a port.
 */
export function fakeFetch(
  script: Array<{ status: number; body?: unknown; headers?: Record<string, string> }> = [],
): FakeFetch {
  const calls: Captured[] = []
  let index = 0

  const fetch: FetchLike = async (url, init) => {
    const headers = (init.headers ?? {}) as Record<string, string>
    let body: unknown
    if (typeof init.body === 'string') {
      body = JSON.parse(init.body)
    } else if (init.body) {
      const bytes = init.body as Uint8Array
      const raw = headers['content-encoding'] === 'gzip' ? zlib.gunzipSync(bytes) : bytes
      body = JSON.parse(new TextDecoder().decode(raw))
    }
    calls.push({ url, method: init.method ?? 'GET', headers, body })

    const step = script[Math.min(index, script.length - 1)]
    index++
    if (!step) return response(200, { accepted: 0, rejected: 0, duplicate: 0 })
    return response(step.status, step.body ?? { accepted: 0, rejected: 0, duplicate: 0 }, step.headers)
  }

  return {
    fetch,
    calls,
    spans() {
      const out: Array<Record<string, unknown>> = []
      for (const call of calls) {
        const batch = call.body as { spans?: Array<Record<string, unknown>> } | undefined
        if (batch?.spans) out.push(...batch.spans)
      }
      return out
    },
  }
}

/** A `fetch` that always fails the way an unreachable daemon does. */
export const deadFetch: FetchLike = async () => {
  throw new TypeError('fetch failed')
}

/** A span with every required field set, for tests that need bulk. */
export function spanFixture(index: number): Span {
  return {
    traceId: 'a'.repeat(32),
    spanId: index.toString(16).padStart(16, '0'),
    name: `step-${index}`,
    kind: 'tool_call',
    status: 'ok',
    startTime: '2026-01-01T00:00:00.000000000Z',
    endTime: '2026-01-01T00:00:00.250000000Z',
    isRoot: false,
    isEvent: false,
    input: 'x'.repeat(64),
    correlation: { project: 'test' },
    receivedAt: '2026-01-01T00:00:00.000000000Z',
  }
}
