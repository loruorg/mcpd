/**
 * The specimen run: the failing trace from `mcpd demo`, the one people open.
 *
 * Every figure here is the one the daemon stores for trace `b3d9e71a`, span for
 * span: the same offsets, the same durations, the same token counts, and the
 * costs its own price book computes from them. Read them back with
 * `GET /v1/api/traces/b3d9e71a4c0f428eb6a5d2317f90c684?include=all` against a
 * daemon started with `mcpd demo`.
 *
 * The waterfall and the run drum both draw from this one array, so the two
 * instruments on the home page cannot disagree about the run they show.
 *
 * The waterfall used to name that trace and then add two spans the trace does
 * not contain, a `recall_thread` and a `max_steps` guardrail, and report nine
 * spans and $0.00054 where the daemon holds seven and $0.00055. An instrument
 * may not do that: the page was showing a capability the daemon had not run.
 */
export interface Row {
  depth: number
  kind: string
  name: string
  /** Milliseconds from the start of the run. */
  at: number
  /** Milliseconds. */
  ms: number
  status?: 'ok' | 'error'
  tokens?: string
  cost?: string
  model?: string
}

export const SPECIMEN: Row[] = [
  { depth: 0, kind: 'agent', name: 'support_agent',  at: 0,    ms: 8420, status: 'error' },
  { depth: 1, kind: 'model', name: 'chat',           at: 200,  ms: 2400, tokens: '890 → 28',   cost: '$0.00015', model: 'gpt-4o-mini' },
  { depth: 1, kind: 'tool',  name: 'returns_policy', at: 2610, ms: 90 },
  { depth: 1, kind: 'model', name: 'chat',           at: 2800, ms: 2400, tokens: '1,100 → 28', cost: '$0.00018', model: 'gpt-4o-mini' },
  { depth: 1, kind: 'tool',  name: 'returns_policy', at: 5210, ms: 90 },
  { depth: 1, kind: 'model', name: 'chat',           at: 5400, ms: 2400, tokens: '1,310 → 28', cost: '$0.00021', model: 'gpt-4o-mini' },
  { depth: 1, kind: 'tool',  name: 'returns_policy', at: 7810, ms: 90 },
]

export const SPECIMEN_TRACE = 'b3d9e71a'
export const SPECIMEN_MS = Math.max(...SPECIMEN.map((r) => r.at + r.ms))
