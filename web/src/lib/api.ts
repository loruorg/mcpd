/**
 * A typed client for a running mcpd, used by the operator interface.
 *
 * The interface is a static page that talks to whichever daemon you point it
 * at, so there is no server of our own in the middle and no account anywhere.
 * That is also why every call here has to treat "no daemon" as an ordinary
 * outcome rather than an error worth a stack trace.
 */

import type {
  AccountNotice,
  Account,
  AccountKey,
  AccountSession,
  CreatedAccountKey,
  DeleteResult,
  KeyScope,
  AnalyticsSummary,
  CostBreakdown,
  DeltaResponse,
  DeltaSignal,
  Definition,
  DiscoveryResponse,
  Facet,
  HealthStatus,
  LogListResponse,
  Run,
  RunDetail,
  RunEventPage,
  RunListResponse,
  Span,
  StoreCapabilities,
  TokenUsage,
  Trace,
  TraceDetail,
  TraceListResponse,
} from '@/generated/types'

/**
 * The daemon a build points at, from `PUBLIC_MCPD_ENDPOINT` at build time.
 *
 * Unset is the build the daemon embeds and the documentation hosts, which
 * looks on the page's own origin and then on the default port. A hosted build
 * names its daemon, such as `https://api.mcpd.sh`. The empty string and
 * `same-origin` both mean the origin the page was served from.
 */
const BUILD_ENDPOINT: string | undefined = import.meta.env.PUBLIC_MCPD_ENDPOINT

/** Whether this build was told where its daemon is. */
export const ENDPOINT_FROM_BUILD = BUILD_ENDPOINT !== undefined

function defaultEndpoint(): string {
  if (BUILD_ENDPOINT === undefined) return 'http://localhost:4317'
  const named = BUILD_ENDPOINT.trim()
  if (named === '' || named === 'same-origin') {
    return typeof window !== 'undefined' ? window.location.origin : 'http://localhost:4317'
  }
  return named.replace(/\/$/, '')
}

export const DEFAULT_ENDPOINT = defaultEndpoint()

/* ------------------------------------------------------------ session --- */

/**
 * A logged-in account, as this browser holds it.
 *
 * Kept with the endpoint that issued it, and sent only to that endpoint: a
 * reader who points the interface at another daemon must not hand that daemon
 * the token for this one.
 */
export interface StoredSession {
  endpoint: string
  token: string
  email: string
  /** Absent for an account with no project of its own. */
  project?: string
  /** Set for a single sign-on session, which reaches this organisation only. */
  org_id?: string
  expires_at: string
}

const SESSION_STORE = 'mcpd:session'

/** The session for `endpoint`, or `null` when there is none or it has expired. */
export function storedSession(endpoint?: string): StoredSession | null {
  try {
    const raw = localStorage.getItem(SESSION_STORE)
    if (!raw) return null
    const s = JSON.parse(raw) as StoredSession
    if (!s.token || new Date(s.expires_at).getTime() <= Date.now()) return null
    if (endpoint !== undefined && s.endpoint !== endpoint.replace(/\/$/, '')) return null
    return s
  } catch {
    return null
  }
}

export function saveSession(endpoint: string, s: AccountSession) {
  const stored: StoredSession = { endpoint: endpoint.replace(/\/$/, ''), ...s }
  try {
    localStorage.setItem(SESSION_STORE, JSON.stringify(stored))
  } catch {
    /* Private browsing. The session lasts until the page closes. */
  }
  memorySession = stored
}

export function clearSession() {
  memorySession = null
  try {
    localStorage.removeItem(SESSION_STORE)
  } catch {
    /* Nothing was stored. */
  }
}

/** Where storage is refused, the session still holds for this page. */
let memorySession: StoredSession | null = null

function sessionFor(endpoint: string): string | undefined {
  const s = storedSession(endpoint) ?? memorySession
  return s && s.endpoint === endpoint ? s.token : undefined
}

export class DaemonUnreachable extends Error {
  /**
   * @param notADaemon Something answered at the endpoint, and it was not mcpd.
   *   A documentation host serving this page answers `/healthz` with a 404 and
   *   an HTML body; that is a different fact from a connection nobody accepted,
   *   and the interface reports it differently.
   */
  constructor(
    readonly endpoint: string,
    cause?: unknown,
    readonly notADaemon = false,
  ) {
    super(`No mcpd answered at ${endpoint}`)
    this.name = 'DaemonUnreachable'
    this.cause = cause
  }
}

export class DaemonError extends Error {
  constructor(
    readonly status: number,
    /**
     * The failure as the daemon writes it. `capability` arrives only on a 501
     * and names the thing this backend cannot do, in the same words
     * `GET /v1/api/capabilities` uses, so a screen can say which control it is
     * withdrawing rather than showing an empty panel.
     */
    readonly body: {
      error?: string
      message?: string
      field?: string
      capability?: string
    },
  ) {
    super(body.message ?? `mcpd returned ${status}`)
    this.name = 'DaemonError'
  }
}

export interface ClientOptions {
  endpoint?: string
  apiKey?: string
  signal?: AbortSignal
}

export async function request<T>(
  path: string,
  opts: ClientOptions = {},
  body?: unknown,
  method?: 'GET' | 'POST' | 'DELETE' | 'PATCH' | 'PUT',
): Promise<T> {
  const endpoint = (opts.endpoint ?? DEFAULT_ENDPOINT).replace(/\/$/, '')
  const headers: Record<string, string> = { accept: 'application/json' }
  // A key the reader pasted wins; otherwise the logged-in session, when there
  // is one for this daemon. Every screen goes through here, so none of them
  // has to know about accounts.
  const credential = opts.apiKey || sessionFor(endpoint)
  if (credential) headers.authorization = `Bearer ${credential}`
  if (body !== undefined) headers['content-type'] = 'application/json'

  let res: Response
  try {
    res = await fetch(`${endpoint}${path}`, {
      method: method ?? (body === undefined ? 'GET' : 'POST'),
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: opts.signal,
      mode: 'cors',
    })
  } catch (cause) {
    // A network failure here almost always means the daemon is not running, or
    // is running somewhere this browser cannot reach. Both are worth saying
    // plainly rather than surfacing a TypeError.
    throw new DaemonUnreachable(endpoint, cause)
  }

  if (!res.ok) {
    // Every failure mcpd returns is JSON with an `error` and a `message`. An
    // answer that is not JSON, or a 404 for the health probe every daemon
    // serves, came from something other than a daemon: the static host this
    // page is served from, usually. Reporting that as "refused, 404" sent the
    // reader looking for a key on a server that does not exist, and stopped the
    // fallback to the default port from ever running.
    const isJson = (res.headers.get('content-type') ?? '').includes('application/json')
    if (!isJson || (path === '/healthz' && res.status === 404)) {
      throw new DaemonUnreachable(endpoint, new Error(`${res.status} from ${endpoint}${path}`), true)
    }
    const body = await res.json().catch(() => ({}))
    throw new DaemonError(res.status, body)
  }
  return res.json() as Promise<T>
}

const qs = (params: Record<string, string | number | undefined | null> | object) => {
  const p = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== '') p.set(k, String(v))
  }
  const s = p.toString()
  return s ? `?${s}` : ''
}

/**
 * Every parameter `GET /v1/api/traces` accepts, as the daemon declares them.
 *
 * All twenty-one, not the six the interface used to send. A filter the client
 * cannot express is a filter nobody can use, and the gap is invisible from this
 * side: the daemon accepts the parameter perfectly well.
 */
export interface TraceQuery {
  project: string
  /** Full text across span names, prompts and completions. */
  q?: string
  status?: string
  kind?: string
  /** Exact root span name. */
  name?: string
  /** Comma separated. A trace matches when it carries all of them. */
  tags?: string
  session_id?: string
  thread_id?: string
  /** The plain id you sent. The daemon hashes it the same way it hashed the stored one. */
  user_id?: string
  environment?: string
  model?: string
  from?: string
  to?: string
  min_duration_ms?: number
  max_duration_ms?: number
  min_cost?: number
  max_cost?: number
  min_tokens?: number
  has_score?: string
  score_below?: number
  score_above?: number
  has_feedback?: string
  sort?: string
  order?: string
  limit?: number
  cursor?: string
}

export const api = {
  health: (o?: ClientOptions) => request<HealthStatus>('/healthz', o),

  projects: (o?: ClientOptions) => request<string[]>('/v1/api/projects', o),

  traces: (params: TraceQuery, o?: ClientOptions) =>
    request<TraceListResponse>(`/v1/api/traces${qs(params)}`, o),

  trace: (id: string, params: { project: string; include?: string }, o?: ClientOptions) =>
    request<TraceDetail>(`/v1/api/traces/${id}${qs(params)}`, o),

  logs: (
    params: { project: string; q?: string; level?: string; trace_id?: string; limit?: number },
    o?: ClientOptions,
  ) => request<LogListResponse>(`/v1/api/logs${qs(params)}`, o),

  summary: (
    params: { project: string; from: string; to: string; group_by?: string },
    o?: ClientOptions,
  ) => request<AnalyticsSummary>(`/v1/api/analytics/summary${qs(params)}`, o),

  cost: (
    params: { project: string; from: string; to: string; group_by?: string; bucket?: string },
    o?: ClientOptions,
  ) => request<CostBreakdown>(`/v1/api/analytics/cost${qs(params)}`, o),

  /* The run surface: durable runs, their journal, and the definitions they run. */
  runs: (
    params: { project: string; status?: string; definition_id?: string; limit?: number; cursor?: string },
    o?: ClientOptions,
  ) => request<RunListResponse>(`/v1/api/runs${qs(params)}`, o),

  run: (id: string, o?: ClientOptions) => request<RunDetail>(`/v1/api/runs/${id}`, o),

  runEvents: (id: string, params: { after?: number; limit?: number }, o?: ClientOptions) =>
    request<RunEventPage>(`/v1/api/runs/${id}/events${qs(params)}`, o),

  cancelRun: (id: string, o?: ClientOptions) => request<Run>(`/v1/api/runs/${id}/cancel`, o, {}),

  definitions: (params: { project: string; id?: string }, o?: ClientOptions) =>
    request<Definition[]>(`/v1/api/definitions${qs(params)}`, o),

  /* What this deployment's storage backend can answer. Read once per endpoint
     and shared, because it is a property of the deployment rather than of a
     project, and because every screen that offers an optional control needs
     the same answer before it draws one. */
  capabilities: (o?: ClientOptions) => request<StoreCapabilities>('/v1/api/capabilities', o),

  /** The values one filter dimension takes, for a filter box. */
  discovery: (
    params: { project: string; facet: Facet; key?: string; prefix?: string; limit?: number },
    o?: ClientOptions,
  ) => request<DiscoveryResponse>(`/v1/api/discovery${qs(params)}`, o),

  /**
   * What has arrived since a cursor.
   *
   * The cursor is opaque and is not a page cursor: send back the one you were
   * given, including when a poll came back empty. Moving it forward yourself is
   * what drops a row that lands a moment later with an earlier arrival time.
   */
  deltas: (
    params: { project: string; signal?: DeltaSignal; cursor?: string; since?: string; limit?: number },
    o?: ClientOptions,
  ) => request<DeltaResponse>(`/v1/api/deltas${qs(params)}`, o),

  /* Accounts, on a daemon started with --signup or --accounts. */
  signup: (email: string, password: string, o?: ClientOptions) =>
    request<AccountNotice>('/v1/api/account/signup', o, { email, password }),

  login: (email: string, password: string, totp_code: string | undefined, o?: ClientOptions) =>
    request<AccountSession>('/v1/api/account/login', o, { email, password, totp_code: totp_code || undefined }),

  logout: (o?: ClientOptions) => request<DeleteResult>('/v1/api/account/logout', o, {}),

  account: (o?: ClientOptions) => request<Account>('/v1/api/account', o),

  accountKeys: (o?: ClientOptions) => request<AccountKey[]>('/v1/api/account/keys', o),

  createAccountKey: (scope: KeyScope, name: string | undefined, o?: ClientOptions, project?: string) =>
    request<CreatedAccountKey>('/v1/api/account/keys', o, { scope, name: name || undefined, project }),

  revokeAccountKey: (id: string, o?: ClientOptions) =>
    request<DeleteResult>(`/v1/api/account/keys/${encodeURIComponent(id)}`, o, undefined, 'DELETE'),
}

/**
 * The account lifecycle, organisations and single sign-on. One generic call,
 * because every one of these is a session door with a JSON body and a JSON
 * answer, and the screens name the path and the type themselves.
 */
export function accountCall<T>(
  method: 'GET' | 'POST' | 'DELETE' | 'PATCH' | 'PUT',
  path: string,
  o: ClientOptions,
  body?: unknown,
): Promise<T> {
  return request<T>(path, o, body ?? (method === 'POST' ? {} : undefined), method)
}

/** A path segment, encoded. */
export const seg = (v: string) => encodeURIComponent(v)

/* ----------------------------------------------------- what is missing --- */

/**
 * The capability a refusal names, or `null` when the failure is something else.
 *
 * A backend that cannot answer an endpoint returns 501 with the capability in
 * the body, so a screen can withdraw one control and keep the rest rather than
 * treating the whole read as broken.
 */
export function refusedCapability(e: unknown): string | null {
  if (e instanceof DaemonError && e.status === 501) return e.body.capability ?? null
  return null
}

/**
 * What the interface loses when a backend does not have a capability.
 *
 * Written for the reader, in the interface's own terms rather than the store's:
 * `discovery: false` is a fact about a table, and what it means on screen is
 * that a filter box has to be filled from the rows already listed. A screen
 * that shows an empty box instead teaches that there is nothing to filter on.
 */
export const CAPABILITY_LOSS: Record<string, string> = {
  discovery:
    'cannot list the values a filter takes, so a filter box is filled from what is already on screen',
  discovery_label_values:
    'keeps metric label keys but not their values, so a label value has to be typed rather than chosen',
  delta_polling:
    'has no arrival-ordered read, so nothing here can follow what arrives: a screen refreshes by running its whole query again',
  text_search: 'does not apply the search term at all',
}

/**
 * One sentence naming a backend and what it cannot do.
 *
 * `backend` comes from the capability set rather than from the caller, so the
 * sentence names the storage the deployment is actually running.
 */
export function capabilityNote(backend: string, capability: string): string {
  const loss = CAPABILITY_LOSS[capability]
  return loss
    ? `This deployment stores signals in ${backend}, which ${loss}.`
    : `This deployment stores signals in ${backend}, which does not support ${capability}.`
}

/* ------------------------------------------------------------ finding --- */

/**
 * Find a daemon, starting where the page was told to look.
 *
 * The page's own origin is the first place to look, because the daemon serves
 * this interface from its own binary. A documentation host serving the same
 * page answers `/healthz` with a 404, and nothing is listening on a `file:`
 * origin at all; in both cases the port mcpd listens on by default is the
 * next place to look before reporting that nothing answered.
 *
 * Resolves with the endpoint that answered, which may differ from `ep`. Rejects
 * with `DaemonUnreachable` naming the endpoint worth reporting: the one asked
 * for when a connection was refused there, and the default when the one asked
 * for was never a daemon. Rejects with `DaemonError` when a daemon answered and
 * refused, which is the one answer that must not be papered over by a fallback.
 *
 * Written once here because the runs, logs and spend screens each probed on
 * their own, and the first thing to drift was this fallback.
 */
export async function findDaemon(
  ep: string,
): Promise<{ endpoint: string; health: HealthStatus }> {
  try {
    return { endpoint: ep, health: await api.health({ endpoint: ep }) }
  } catch (first) {
    if (!(first instanceof DaemonUnreachable) || ep === DEFAULT_ENDPOINT) throw first
    try {
      return { endpoint: DEFAULT_ENDPOINT, health: await api.health({ endpoint: DEFAULT_ENDPOINT }) }
    } catch (second) {
      if (second instanceof DaemonUnreachable && first.notADaemon) throw second
      throw first
    }
  }
}

/* --------------------------------------------------------- formatting --- */

/*
 * Every formatter below takes a value that is present. None of them invents a
 * placeholder for an absent one, and that is the point: a formatter that
 * quietly returns a dash lets a caller render "we do not know" as if it were a
 * layout decision. Where a figure can be missing, the caller has to say why,
 * and TypeScript makes it. See `costBlank` for the case that matters most.
 */

/** Format a nanosecond duration the way an engineer reads it. */
export function duration(ns: number): string {
  const ms = ns / 1e6
  if (ms < 1) return `${(ns / 1000).toFixed(0)}µs`
  if (ms < 1000) return `${ms.toFixed(ms < 10 ? 1 : 0)}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(2)}s`
  return `${Math.floor(ms / 60000)}m ${((ms % 60000) / 1000).toFixed(0)}s`
}

/**
 * Format money without lying about precision.
 *
 * A cost of exactly zero and an unpriced call are different facts, so zero
 * prints as a figure and an absent cost never reaches this function at all.
 */
export function money(v: number, currency = 'USD'): string {
  const symbol = currency === 'USD' ? '$' : currency === 'EUR' ? '€' : `${currency} `
  if (v === 0) return `${symbol}0`
  if (v < 0.01) return `${symbol}${v.toFixed(5)}`
  if (v < 1) return `${symbol}${v.toFixed(4)}`
  return `${symbol}${v.toFixed(2)}`
}

export function tokens(n: number): string {
  if (n < 1000) return String(n)
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`
  return `${(n / 1_000_000).toFixed(1)}M`
}

/** A short, human relative time. Absolute time lives in the tooltip. */
export function ago(iso: string): string {
  const then = new Date(iso).getTime()
  // A timestamp this cannot parse came from the daemon, so show what it sent
  // rather than a placeholder that hides it.
  if (Number.isNaN(then)) return iso
  const s = Math.max(0, (Date.now() - then) / 1000)
  if (s < 45) return `${Math.floor(s)}s ago`
  if (s < 3600) return `${Math.floor(s / 60)}m ago`
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`
  return `${Math.floor(s / 86400)}d ago`
}

/* -------------------------------------------------------- absent cost --- */

/**
 * Why a step or a run carries no cost figure.
 *
 * mcpd prices at ingest and stores the answer. The ingest pipeline computes a
 * cost only for a span that names a model AND carries usage, and
 * `cost::compute` returns nothing when the price book holds no rate for that
 * model, or when the usage names no meter it can price. So an absent cost is
 * never one fact: it is one of several, and the stored span says which.
 *
 * Printing the same dash for all of them is what the incumbent does, and it
 * turns a missing price entry into what looks like a formatting choice. A model
 * missing from the book is missing from every total, so that blank is money
 * leaving the figures unnoticed, and it is worth a word.
 */
export type CostAbsence =
  | 'not-a-model-call'
  | 'no-model'
  | 'no-usage'
  | 'no-meters'
  | 'no-rate'
  | 'no-meter-rate'
  | 'no-total'
  | 'mixed'

export interface CostBlank {
  code: CostAbsence
  /** Short enough to stand in the column the figure would have occupied. */
  short: string
  /** The whole fact, for a title or for a line of its own. */
  long: string
}

const SPAN_BLANK: Record<Exclude<CostAbsence, 'mixed' | 'no-meter-rate'>, CostBlank> = {
  'not-a-model-call': {
    code: 'not-a-model-call',
    short: 'no tokens',
    long: 'This step spends no tokens. Cost is recorded against model calls, so there is no figure to show here.',
  },
  'no-model': {
    code: 'no-model',
    short: 'no model',
    long: 'This step reports no model id, so mcpd had nothing to look up in the price book.',
  },
  'no-usage': {
    code: 'no-usage',
    short: 'no usage',
    long: 'The producer sent no token counts with this call, so there was nothing to price.',
  },
  'no-meters': {
    code: 'no-meters',
    short: 'total only',
    long: 'The producer sent a token total but no input or output counts. mcpd prices each meter at its own rate and will not split a total it was not given.',
  },
  'no-rate': {
    code: 'no-rate',
    short: 'unpriced',
    long: 'The price book this daemon was running with holds no rate for this model, so mcpd stored no figure rather than a zero. Add the model to your price book to bring this call into the totals.',
  },
  'no-total': {
    code: 'no-total',
    short: 'no total',
    long: 'The producer reported a cost with no total in it. mcpd stores what it was sent rather than computing over it.',
  },
}

/** Span kinds that spend tokens, and so can carry a cost. */
const TOKEN_SPENDING_KINDS = new Set<string>([
  'model_generation',
  'model_step',
  'model_inference',
  'model_chunk',
  'rag_embedding',
])

type PricedSpan = Pick<Span, 'kind' | 'model' | 'usage' | 'cost'>

/** Whether a cost figure belongs on this span at all. */
export function pricingApplies(span: Pick<Span, 'kind' | 'model' | 'usage'>): boolean {
  return span.model != null || span.usage != null || TOKEN_SPENDING_KINDS.has(span.kind)
}

/**
 * Meters `cost::compute` can price from the rates every book entry carries.
 *
 * It bills each of these at its own rate `.or(input)` or `.or(output)`, so any
 * one of them produces a figure the moment the model is found at all. That is
 * what makes the model lookup the only remaining explanation when a span
 * carrying one of them still has no cost, and it is the whole basis for
 * `no-rate` saying so.
 *
 * A total on its own is deliberately not here. Providers disagree about what a
 * total counts, so mcpd prices input and output separately and never splits a
 * figure it was not given.
 */
function alwaysPriceable(u: TokenUsage): boolean {
  if (u.input_tokens != null || u.output_tokens != null) return true
  const i = u.input_details
  if (
    i &&
    (i.text != null ||
      i.cache_read != null ||
      i.cache_write != null ||
      i.cache_write_5m != null ||
      i.cache_write_1h != null ||
      i.audio != null)
  ) {
    return true
  }
  const o = u.output_details
  return o != null && (o.text != null || o.audio != null)
}

/**
 * Meters that price only from a rate of their own, named for a reader.
 *
 * `reasoning`, `image` input and `image` output are the three the daemon bills
 * only when the book gives them an explicit rate: every other meter falls back
 * to the plain input or output rate. Most books carry none of the three, so a
 * call reporting only these can go unpriced with the model present in the book,
 * which is exactly the case `no-rate` must not claim.
 */
function optionalMeters(u: TokenUsage): string[] {
  const names: string[] = []
  if (u.output_details?.reasoning != null) names.push('reasoning tokens')
  if (u.input_details?.image != null) names.push('image input tokens')
  if (u.output_details?.image != null) names.push('image output tokens')
  return names
}

/**
 * The reason for a call whose only counts need a meter rate.
 *
 * Two things can produce it and this side cannot tell them apart: the model may
 * be missing from the book, or its entry may carry no rate for these meters.
 * The interface has no way to read the book the daemon priced against, so it
 * names both rather than picking the one that reads better.
 */
function meterRateBlank(meters: string[]): CostBlank {
  const list =
    meters.length === 1
      ? meters[0]
      : `${meters.slice(0, -1).join(', ')} and ${meters[meters.length - 1]}`
  return {
    code: 'no-meter-rate',
    short: 'unpriced',
    long: `The only counts on this call are ${list}, which mcpd bills only from a rate of their own. No figure was stored, so either this model is missing from the price book, or its entry carries no rate for those meters. Check the entry before adding one.`,
  }
}

/** Why this span has no cost, or `null` when it has one. */
export function costBlank(span: PricedSpan): CostBlank | null {
  if (span.cost?.total_cost != null) return null
  if (span.cost) return SPAN_BLANK['no-total']
  if (!pricingApplies(span)) return SPAN_BLANK['not-a-model-call']
  if (!span.model) return SPAN_BLANK['no-model']
  // The pipeline needs a model AND usage before it prices anything, so no usage
  // is the whole reason: the price book was never consulted.
  if (!span.usage) return SPAN_BLANK['no-usage']
  if (!alwaysPriceable(span.usage)) {
    // A bare total is the wholly fixable case, and the counts it asks the
    // producer for would price the call whatever the book holds, so it is the
    // more useful answer even when an optionally-rated meter sits beside it.
    if (span.usage.total_tokens != null) return SPAN_BLANK['no-meters']
    // Only meters that need a rate of their own. `compute` returns nothing here
    // for either of two reasons, and this side cannot see which.
    const meters = optionalMeters(span.usage)
    if (meters.length) return meterRateBlank(meters)
    // An empty usage object is not the same claim as a bare total. Only the
    // second one has a figure to point at, so only the second one may say so.
    return SPAN_BLANK['no-usage']
  }
  // Model, and a meter the book prices for every model it holds, and still
  // nothing stored. `compute` has two ways to decline and only one of them can
  // have fired: the price book lookup. That is what makes `no-rate` an
  // observation rather than a guess.
  return SPAN_BLANK['no-rate']
}

const NO_MODEL_CALL: CostBlank = {
  code: 'not-a-model-call',
  short: 'no model call',
  long: 'No step in this run called a model, so there is nothing to price.',
}

/**
 * Why this run has no cost figure, or `null` when it has one.
 *
 * Pass the run's spans where you have them. The list view does not, so it can
 * say that a run is unpriced but not why; the reason arrives when you open it.
 */
export function traceCostBlank(
  trace: Pick<Trace, 'cost' | 'model_count'>,
  spans?: Span[],
): CostBlank | null {
  if (trace.cost?.total_cost != null) return null

  if (!spans) {
    if (trace.model_count < 1) return NO_MODEL_CALL
    const n = trace.model_count
    return {
      code: 'mixed',
      short: 'not priced',
      long: `${n} model call${n === 1 ? '' : 's'} in this run and no cost against any of them. Open the run to see why.`,
    }
  }

  const priceable = spans.filter(pricingApplies)
  if (!priceable.length) return NO_MODEL_CALL

  const blanks = priceable.map(costBlank).filter((b): b is CostBlank => b !== null)
  if (!blanks.length) {
    // Every step is priced and the run still has no total. That is the daemon
    // disagreeing with itself, and saying so is better than picking a reason.
    return {
      code: 'mixed',
      short: 'not priced',
      long: 'The steps in this run carry costs, but the run itself has no total.',
    }
  }

  const first = blanks[0]
  if (blanks.length === 1) return first

  const reasons = [...new Set(blanks.map((b) => b.short))]
  return {
    // One shared reason keeps its own word here, which says more than a generic
    // one. The sentence is written for the run rather than borrowed from a
    // step, because a step's wording is about one call and this is about six.
    code: reasons.length === 1 ? first.code : 'mixed',
    short: reasons.length === 1 ? first.short : 'not priced',
    long: `${blanks.length} steps here could have carried a cost and none does. ${
      reasons.length === 1 ? `Reason: ${reasons[0]}` : `Reasons: ${reasons.join(', ')}`
    }. Open a step for the detail.`,
  }
}
