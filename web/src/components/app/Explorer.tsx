/**
 * The runs screen: find a run, then read it.
 *
 * A static page that talks to whichever mcpd you point it at. There is no
 * server of ours in the middle, which is why the endpoint is a field rather
 * than a build-time constant, and why "no daemon" is a first-class state with
 * real instructions rather than an error toast.
 *
 * The list is the screen's own work: the filters, the columns, the time range,
 * the paging, the live tail and the keyboard. Reading one run is not: that is
 * `TracePanel`, mounted below the list, so a waterfall opened from a log line
 * and a waterfall opened from here are the same waterfall rather than two that
 * drift.
 *
 * Every selection lives in the URL. A run, a step, a filter, a sort, a range:
 * the whole view is in the address bar, so what somebody pastes into a channel
 * opens on what they were looking at.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  api,
  capabilityNote,
  DaemonError,
  DaemonUnreachable,
  DEFAULT_ENDPOINT,
  ENDPOINT_FROM_BUILD,
  findDaemon,
  refusedCapability,
  storedSession,
} from '@/lib/api'
import type { TraceQuery } from '@/lib/api'
import type { HealthStatus, Trace } from '@/generated/types'
import { TooltipProvider } from '@/components/ui/tooltip'
import { DaemonTile, nowIso, ProjectPicker } from './shell'
import { TracePanel } from './TracePanel'
import { Action } from './trace-ui'
import { useCapabilities } from './capabilities'
import { ColumnDialog } from './Columns'
import { FilterBar, FILTER_BY_ID, filterPhrase, anyApplied, toParams } from './Filters'
import type { FilterValues } from './Filters'
import { RangePicker, DEFAULT_RANGE, rangeBounded, rangeParams, rangePhrase } from './TimeRange'
import type { Range } from './TimeRange'
import { DEFAULT_PREFS, clearPrefs, loadPrefs, savePrefs, visibleColumns } from './list-prefs'
import type { ListPrefs } from './list-prefs'
import { TraceTable } from './TraceTable'

/** One page of runs. The daemon allows up to 500; 60 fills a tall screen twice. */
const PAGE = 60

/** How often the daemon is asked what has arrived. */
const POLL_MS = 4000

/** How long a row stays washed after it arrives. */
const HIGHLIGHT_MS = 4500

export type Conn =
  | { state: 'connecting' }
  | { state: 'connected'; health: HealthStatus }
  | { state: 'absent'; endpoint: string }
  | { state: 'refused'; message: string }

export function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value)
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms)
    return () => clearTimeout(t)
  }, [value, ms])
  return v
}

/**
 * Which daemon to talk to.
 *
 * In order: an explicit `?daemon=` on the URL, then a remembered choice, then
 * **the origin this page was served from**. That last one matters: the daemon
 * compiles this interface into its own binary and serves it, so a page served
 * by mcpd should talk to the mcpd that served it, whatever port that is.
 * Falling back to a hard-coded `localhost:4317` there would show an empty
 * screen to someone whose daemon is running perfectly well on another port.
 */
export function initialEndpoint(): string {
  if (typeof window === 'undefined') return DEFAULT_ENDPOINT
  const fromUrl = new URLSearchParams(location.search).get('daemon')
  if (fromUrl) return fromUrl
  try {
    const saved = localStorage.getItem('mcpd:endpoint')
    if (saved) return saved
  } catch {
    /* Private browsing. Fall through to the origin. */
  }
  // A build that names its daemon, such as the hosted one, is not served by
  // that daemon, so its own origin is the wrong first guess.
  if (ENDPOINT_FROM_BUILD) return DEFAULT_ENDPOINT
  return location.protocol.startsWith('http') ? location.origin : DEFAULT_ENDPOINT
}

/**
 * The project a screen opens on: the logged-in account's own, since it is the
 * only one that account can read, and otherwise `default`.
 */
export function initialProject(): string {
  if (typeof window === 'undefined') return 'default'
  return storedSession()?.project ?? 'default'
}

/* ------------------------------------------------------------- the url --- */

/**
 * The whole view, as it appears in the address bar.
 *
 * Filters carry an `f.` prefix so that adding a dimension can never collide
 * with a parameter the shell owns, and so the whole set can be read back
 * without naming each one here.
 */
interface View {
  project: string
  q: string
  filters: FilterValues
  range: Range
  sort: string
  order: 'asc' | 'desc'
  trace: string | null
  span: string | null
  density: 'compact' | 'roomy' | null
}

function readView(fallbackProject: string): View {
  const p = new URLSearchParams(typeof location === 'undefined' ? '' : location.search)
  const filters: FilterValues = {}
  for (const [k, v] of p.entries()) if (k.startsWith('f.')) filters[k.slice(2)] = v
  const rows = p.get('rows')
  return {
    project: p.get('project') || fallbackProject,
    q: p.get('q') ?? '',
    filters,
    range: {
      preset: p.get('range') || DEFAULT_RANGE.preset,
      from: p.get('from') ?? '',
      to: p.get('to') ?? '',
    },
    sort: p.get('sort') || 'start_time',
    order: p.get('order') === 'asc' ? 'asc' : 'desc',
    trace: p.get('trace'),
    span: p.get('span'),
    // Null, not the default: a view that never named a density takes the
    // reader's own preference, and only a link that named one overrides it.
    density: rows === 'roomy' || rows === 'compact' ? rows : null,
  }
}

/**
 * Write the view back, keeping every parameter this screen does not own.
 *
 * `replaceState`, not `pushState`. Typing in a filter box would otherwise put a
 * history entry behind every keystroke, and the back button would walk them one
 * at a time instead of leaving the screen.
 */
function writeView(v: View) {
  const p = new URLSearchParams(location.search)
  for (const k of [...p.keys()]) if (k.startsWith('f.')) p.delete(k)
  const set = (k: string, value: string | null | undefined) => {
    if (value) p.set(k, value)
    else p.delete(k)
  }
  set('project', v.project === 'default' ? null : v.project)
  set('q', v.q)
  for (const [k, value] of Object.entries(v.filters)) if (value.trim()) p.set(`f.${k}`, value)
  set('range', v.range.preset === DEFAULT_RANGE.preset ? null : v.range.preset)
  set('from', v.range.preset === 'custom' ? v.range.from : null)
  set('to', v.range.preset === 'custom' ? v.range.to : null)
  set('sort', v.sort === 'start_time' ? null : v.sort)
  set('order', v.order === 'desc' ? null : v.order)
  set('trace', v.trace)
  set('span', v.span)
  set('rows', v.density === 'compact' ? null : v.density)
  const q = p.toString()
  history.replaceState(history.state, '', `${location.pathname}${q ? `?${q}` : ''}`)
}

/* ----------------------------------------------------------- failures --- */

/**
 * Why the list is not showing runs.
 *
 * The daemon distinguishes 401 from 403 deliberately, and until now every
 * screen collapsed both into one "refused" panel that threw the code away. They
 * are different problems with different answers: one is a key this browser does
 * not have, the other is a key that is not allowed this project.
 */
type Failure =
  | { kind: 'auth'; message: string }
  | { kind: 'forbidden'; message: string }
  | { kind: 'capability'; capability: string; message: string }
  | { kind: 'rejected'; message: string; field?: string }
  | { kind: 'gone'; endpoint: string }
  | { kind: 'failed'; message: string }

function classify(e: unknown): Failure {
  if (e instanceof DaemonUnreachable) return { kind: 'gone', endpoint: e.endpoint }
  if (e instanceof DaemonError) {
    if (e.status === 401) return { kind: 'auth', message: e.message }
    if (e.status === 403) return { kind: 'forbidden', message: e.message }
    if (e.status === 501) {
      return { kind: 'capability', capability: e.body.capability ?? 'this query', message: e.message }
    }
    if (e.status === 400) return { kind: 'rejected', message: e.message, field: e.body.field }
    return { kind: 'failed', message: e.message }
  }
  return { kind: 'failed', message: e instanceof Error ? e.message : String(e) }
}

const KEY_STORE = 'mcpd:key'

function storedKey(): string {
  try {
    return localStorage.getItem(KEY_STORE) ?? ''
  } catch {
    return ''
  }
}

/* ---------------------------------------------------------------- main --- */

export default function Explorer({
  initialTrace,
  endpoint: fromShell,
  onEndpoint,
}: {
  initialTrace?: string
  /** The shell's endpoint, where one is above this screen. */
  endpoint?: string
  onEndpoint?: (v: string) => void
} = {}) {
  // Controlled by the shell when there is one, so the endpoint the header names
  // is the endpoint this screen is actually reading. It still falls back to its
  // own state so the explorer stands alone.
  const [ownEndpoint, setOwnEndpoint] = useState(initialEndpoint)
  const endpoint = fromShell ?? ownEndpoint
  const setEndpoint = onEndpoint ?? setOwnEndpoint

  const [prefs, setPrefsState] = useState<ListPrefs>(DEFAULT_PREFS)
  const [view, setView] = useState<View>(() => readView(initialProject()))
  const [conn, setConn] = useState<Conn>({ state: 'connecting' })
  const [projects, setProjects] = useState<string[]>([])
  const [apiKey, setApiKey] = useState('')

  const [traces, setTraces] = useState<Trace[]>([])
  const [cursor, setCursor] = useState<string | undefined>()
  const [hasMore, setHasMore] = useState(false)
  const [paged, setPaged] = useState(false)
  const [loading, setLoading] = useState(false)
  const [loadingMore, setLoadingMore] = useState(false)
  const [failure, setFailure] = useState<Failure | null>(null)
  /** Whether the project holds any run at all, asked only when a filtered read came back empty. */
  const [presence, setPresence] = useState<'unknown' | 'some' | 'none'>('unknown')
  const [live, setLive] = useState(true)
  const [arrived, setArrived] = useState<ReadonlySet<string>>(new Set())
  const [visible, setVisible] = useState(true)
  /** Bumped whenever the list should return to the top. */
  const [top, setTop] = useState(0)

  const caps = useCapabilities(endpoint, apiKey || undefined)
  const canFollow = caps?.delta_polling !== false
  const debouncedQuery = useDebounced(view.q, 250)

  // Preferences are read on the client only: this bundle is prerendered, and
  // reading storage during the first render would make the server's markup and
  // the browser's disagree.
  useEffect(() => {
    setPrefsState(loadPrefs())
    setApiKey(storedKey())
  }, [])

  const setPrefs = useCallback(
    (p: ListPrefs) => {
      // Density is both a preference and part of the view, so a link shows the
      // list the way the person who sent it was reading it. The other
      // preferences are this browser's alone and stay out of the address bar.
      if (p.density !== prefs.density) setView((v) => ({ ...v, density: p.density }))
      setPrefsState(p)
      savePrefs(p)
    },
    [prefs.density],
  )

  const resetPrefs = useCallback(() => {
    clearPrefs()
    setPrefsState(DEFAULT_PREFS)
    setView((v) => ({ ...v, density: null }))
  }, [])

  // Arriving from another screen with a run in hand.
  useEffect(() => {
    if (initialTrace) setView((v) => (v.trace === initialTrace ? v : { ...v, trace: initialTrace }))
  }, [initialTrace])

  useEffect(() => {
    writeView(view)
  }, [view])

  // The back button, and any other screen that rewrites the address.
  useEffect(() => {
    const onPop = () => setView(readView(initialProject()))
    window.addEventListener('popstate', onPop)
    return () => window.removeEventListener('popstate', onPop)
  }, [])

  useEffect(() => {
    const onVisible = () => setVisible(document.visibilityState === 'visible')
    document.addEventListener('visibilitychange', onVisible)
    onVisible()
    return () => document.removeEventListener('visibilitychange', onVisible)
  }, [])

  /* ---------------------------------------------------------- connect --- */
  const probe = useCallback(
    async (ep: string, key: string) => {
      setConn({ state: 'connecting' })
      try {
        // The page's own origin may be a documentation host rather than a
        // daemon. `findDaemon` tries the port mcpd listens on by default before
        // reporting nothing, and names the endpoint that answered.
        const { endpoint: found, health } = await findDaemon(ep)
        if (found !== ep) {
          // Adopting it re-runs this probe against the endpoint that answered.
          setEndpoint(found)
          return
        }
        setConn({ state: 'connected', health })
        try {
          localStorage.setItem('mcpd:endpoint', ep)
        } catch {
          /* Private browsing. The endpoint still applies to this session. */
        }
        const list = await api
          .projects({ endpoint: ep, apiKey: key || undefined })
          .catch(() => [] as string[])
        setProjects(list)
        // Move to the project that exists only when the reader never named one.
        // A project on the URL is a deliberate choice, and a daemon that holds
        // no signals for it yet is a fact worth showing rather than one to
        // quietly correct: the alternative is a shared link that opens on
        // somebody else's data.
        setView((v) =>
          list.length && v.project === 'default' && !list.includes('default')
            ? { ...v, project: list[0] }
            : v,
        )
      } catch (e) {
        if (e instanceof DaemonUnreachable) {
          // When the page's origin was never a daemon, the honest report names
          // the default port, which is where the reader can start one.
          if (e.endpoint !== ep) setEndpoint(e.endpoint)
          setConn({ state: 'absent', endpoint: e.endpoint })
        } else if (e instanceof DaemonError) setConn({ state: 'refused', message: e.message })
        else setConn({ state: 'refused', message: String(e) })
      }
    },
    [setEndpoint],
  )

  useEffect(() => {
    void probe(endpoint, apiKey)
  }, [endpoint, apiKey, probe])

  /* ------------------------------------------------------------- read --- */
  // Previews are asked for only while a preview column is on screen. A read
  // with them carries the start of each run's input and output, and the
  // daemon records it in the audit log; a list without them is not recorded.
  const previews = !prefs.hidden.includes('input_preview') || !prefs.hidden.includes('output_preview')
  const query = useMemo<TraceQuery>(() => {
    const filters = toParams(view.filters)
    return {
      project: view.project,
      q: debouncedQuery.trim() || undefined,
      ...filters,
      ...rangeParams(view.range),
      preview: previews ? 'true' : undefined,
      sort: view.sort,
      order: view.order,
      limit: PAGE,
    }
  }, [view.project, view.filters, view.range, view.sort, view.order, debouncedQuery, previews])

  const filtered = anyApplied(view.filters) || Boolean(debouncedQuery.trim()) || rangeBounded(view.range)

  const load = useCallback(
    async (signal?: AbortSignal) => {
      if (conn.state !== 'connected') return
      setLoading(true)
      try {
        const res = await api.traces(query, { endpoint, apiKey: apiKey || undefined, signal })
        setTraces(res.traces)
        setCursor(res.page.next_cursor)
        setHasMore(res.page.has_more)
        setPaged(false)
        setFailure(null)
        // An empty result has two very different causes and an operator acts
        // differently on each, so the daemon is asked which one it is rather
        // than being given a sentence that covers both.
        if (res.traces.length === 0 && filtered) {
          const any = await api.traces(
            { project: view.project, limit: 1 },
            { endpoint, apiKey: apiKey || undefined, signal },
          )
          setPresence(any.traces.length ? 'some' : 'none')
        } else setPresence(res.traces.length ? 'some' : 'none')
      } catch (e) {
        if ((e as { name?: string }).name === 'AbortError') return
        setFailure(classify(e))
      } finally {
        setLoading(false)
      }
    },
    [conn.state, query, endpoint, apiKey, filtered, view.project],
  )

  useEffect(() => {
    const ac = new AbortController()
    void load(ac.signal)
    return () => ac.abort()
  }, [load])

  // A different question deserves the top of its answer. Without this the list
  // keeps the offset it had, and a filter that leaves five runs shows the last
  // of them.
  useEffect(() => setTop((n) => n + 1), [query])

  const more = useCallback(async () => {
    if (!cursor || loadingMore) return
    // Paging and tailing cannot both be true: the tail replaces the first page
    // on every poll, which would tear the older pages off the bottom.
    setLive(false)
    setPaged(true)
    setLoadingMore(true)
    try {
      const res = await api.traces(
        { ...query, cursor },
        { endpoint, apiKey: apiKey || undefined },
      )
      setTraces((prev) => {
        const seen = new Set(prev.map((t) => t.trace_id))
        return [...prev, ...res.traces.filter((t) => !seen.has(t.trace_id))]
      })
      setCursor(res.page.next_cursor)
      setHasMore(res.page.has_more)
    } catch (e) {
      setFailure(classify(e))
    } finally {
      setLoadingMore(false)
    }
  }, [cursor, loadingMore, query, endpoint, apiKey])

  /* ------------------------------------------------------------ tail --- */

  /**
   * The delta cursor, which is not a page cursor.
   *
   * It is opaque and is sent back exactly as it was given, including after a
   * poll that came back empty. Moving it forward from the rows that arrived is
   * what drops a row that lands a moment later with an earlier arrival time.
   */
  const delta = useRef<string | undefined>(undefined)
  const wash = useRef<number | undefined>(undefined)
  // The poll calls the current read without depending on it. `load` is rebuilt
  // whenever a filter changes, and depending on it here restarted the interval,
  // and fired a poll, on every keystroke in a filter box.
  const latest = useRef(load)
  useEffect(() => {
    latest.current = load
  }, [load])

  // A new query means a new watermark: the cursor belongs to the poll, not to
  // the filter set, and reusing it across a project change would report another
  // project's arrivals.
  useEffect(() => {
    delta.current = undefined
  }, [view.project, endpoint])

  useEffect(() => () => window.clearTimeout(wash.current), [])

  useEffect(() => {
    if (!live || conn.state !== 'connected' || paged || !visible) return
    let stopped = false

    const tick = async () => {
      if (stopped) return
      // Without the capability there is no arrival-ordered read at all, so the
      // tail is the whole query run again. It is the same refresh, at the same
      // interval, costing the daemon a great deal more.
      if (!canFollow) {
        void latest.current()
        return
      }
      try {
        const res = await api.deltas(
          {
            project: view.project,
            signal: 'traces',
            cursor: delta.current,
            // The first poll starts at now. Without it the first answer is the
            // whole retention window, and every row in the list would flash as
            // though it had this moment arrived.
            since: delta.current ? undefined : nowIso(),
            limit: 100,
          },
          { endpoint, apiKey: apiKey || undefined },
        )
        if (stopped) return
        delta.current = res.cursor
        const ids = (res.traces ?? []).map((t) => t.trace_id)
        if (!ids.length) return
        setArrived(new Set(ids))
        window.clearTimeout(wash.current)
        wash.current = window.setTimeout(() => setArrived(new Set()), HIGHLIGHT_MS)
        // The delta says what arrived, not what matches. The list is read again
        // so that what is on screen is still the daemon's answer to the query,
        // and a row that arrived but does not match never appears at all.
        void latest.current()
      } catch (e) {
        if (refusedCapability(e)) return
        /* A failed poll is not a failed screen: the next one may answer. */
      }
    }

    void tick()
    const id = setInterval(() => void tick(), POLL_MS)
    return () => {
      stopped = true
      clearInterval(id)
    }
  }, [live, conn.state, paged, visible, canFollow, view.project, endpoint, apiKey])

  /* ----------------------------------------------------------- render --- */

  const columns = useMemo(() => visibleColumns(prefs), [prefs])
  const density = view.density ?? prefs.density

  const setSort = useCallback((key: string) => {
    setView((v) => (v.sort === key ? { ...v, order: v.order === 'desc' ? 'asc' : 'desc' } : { ...v, sort: key, order: 'desc' }))
  }, [])

  const select = useCallback((id: string) => {
    setView((v) => (v.trace === id ? { ...v, trace: null, span: null } : { ...v, trace: id, span: null }))
  }, [])

  if (conn.state !== 'connected') {
    return (
      <Disconnected
        conn={conn}
        endpoint={endpoint}
        onEndpoint={setEndpoint}
        onRetry={() => probe(endpoint, apiKey)}
      />
    )
  }

  return (
    // One provider for the whole screen, so moving between two truncated names
    // reads the second immediately instead of waiting out the open delay again.
    <TooltipProvider delayDuration={350} skipDelayDuration={200}>
      {/* The screen is exactly the viewport below the two rails above it, so
          the list scrolls inside itself and the page does not scroll at all.
          6.75rem is what those two rails measure. */}
      <div className="grid h-[calc(100dvh-6.75rem)] min-h-[32rem] grid-rows-[auto_auto_minmax(0,1fr)]">
        <div className="border-b border-scribe bg-panel-1">
          <div className="flex flex-wrap items-center gap-2.5 px-4 py-2.5">
            <DaemonTile health={conn.health} endpoint={endpoint} />
            <ProjectPicker
              projects={projects}
              project={view.project}
              onProject={(project) => setView((v) => ({ ...v, project, trace: null, span: null }))}
            />

            <div className="well flex min-w-[12rem] flex-1 items-center gap-2 px-2.5 py-2">
              <span className="data select-none text-[0.75rem] text-ink-3" aria-hidden="true">
                /
              </span>
              <input
                value={view.q}
                onChange={(e) => setView((v) => ({ ...v, q: e.target.value }))}
                placeholder="search prompts and completions"
                spellCheck={false}
                aria-label="Search"
                className="data w-full min-w-0 bg-transparent text-[0.8125rem] text-ink outline-none placeholder:text-ink-3"
              />
            </div>

            <button
              type="button"
              onClick={() => {
                const next = !live
                setLive(next)
                // Following again means the first page again. The tail replaces
                // page one on every poll, so it cannot run under pages two and
                // three; saying that in the control is better than a control
                // that looks pressed and does nothing.
                if (next) {
                  setTop((n) => n + 1)
                  void load()
                }
              }}
              aria-pressed={live}
              className="annunciator py-1.5"
              data-lit={live && visible ? 'amber' : ''}
              title={
                !live
                  ? paged
                    ? 'Not refreshing. Press to follow what arrives, which returns to the first page.'
                    : 'Not refreshing. Press to follow what arrives.'
                  : !visible
                    ? 'This tab is in the background, so nothing is being polled. It resumes when you come back.'
                    : canFollow
                      ? 'Following: the daemon is asked every four seconds what has arrived, and the list is read again only when something has'
                      : 'Following by running the whole query again every four seconds, because this backend has no arrival-ordered read'
              }
            >
              <span
                className={
                  live && visible
                    ? 'mcpd-tail-pip inline-block h-1.5 w-1.5 rounded-full bg-current'
                    : 'inline-block h-1.5 w-1.5 rounded-full bg-current opacity-40'
                }
              />
              live
            </button>

            <ColumnDialog prefs={prefs} onPrefs={setPrefs} onReset={resetPrefs} traces={traces} />

            {/* A backend that cannot search says so, rather than answering an
                empty list to a search it never ran. */}
            {caps?.text_search === 'none' && debouncedQuery.trim() !== '' && (
              // Amber, not oxide: a capability this backend does not have is
              // something to know about, not a fault in the run you are reading.
              <p className="order-last w-full text-[0.75rem] leading-snug text-amber">
                {capabilityNote(caps.backend, 'text_search')} What is on screen is the
                unsearched list.
              </p>
            )}

            <span className="data ml-auto text-[0.75rem] text-ink-3">
              {loading && !traces.length ? 'reading…' : `${traces.length}${hasMore ? '+' : ''} run${traces.length === 1 ? '' : 's'}`}
            </span>
          </div>
          <div className="ticks opacity-40" aria-hidden="true" />
        </div>

        <FilterBar
          values={view.filters}
          onChange={(filters) => setView((v) => ({ ...v, filters }))}
          endpoint={endpoint}
          project={view.project}
        >
          <RangePicker range={view.range} onChange={(range) => setView((v) => ({ ...v, range }))} />
        </FilterBar>

        <Split
          fraction={prefs.split}
          onFraction={(split) => setPrefs({ ...prefs, split })}
          open={Boolean(view.trace)}
          list={
            <TraceTable
              traces={traces}
              columns={columns}
              meta={prefs.meta}
              density={density}
              selected={view.trace}
              onSelect={select}
              arrived={arrived}
              sort={view.sort}
              order={view.order}
              onSort={setSort}
              hasMore={hasMore}
              loadingMore={loadingMore}
              onMore={() => void more()}
              paged={paged}
              top={top}
              empty={
                failure ? (
                  <FailureState
                    failure={failure}
                    project={view.project}
                    endpoint={endpoint}
                    apiKey={apiKey}
                    onKey={(k) => {
                      setApiKey(k)
                      try {
                        localStorage.setItem(KEY_STORE, k)
                      } catch {
                        /* The key applies to this session and is not remembered. */
                      }
                    }}
                    onRetry={() => void load()}
                    backend={caps?.backend}
                  />
                ) : loading ? (
                  <p className="legend text-ink-3">Reading</p>
                ) : (
                  <EmptyState
                    filtered={filtered}
                    presence={presence}
                    project={view.project}
                    phrase={[filterPhrase(view.filters), debouncedQuery.trim() ? `text “${debouncedQuery.trim()}”` : '']
                      .filter(Boolean)
                      .join(', ')}
                    range={rangePhrase(view.range)}
                    onClear={() =>
                      setView((v) => ({ ...v, filters: {}, q: '', range: DEFAULT_RANGE }))
                    }
                  />
                )
              }
            />
          }
          detail={
            view.trace ? (
              <TracePanel
                traceId={view.trace}
                project={view.project}
                endpoint={endpoint}
                apiKey={apiKey || undefined}
                spanId={view.span}
                onSpanChange={(span) => setView((v) => ({ ...v, span }))}
                className="border-t border-scribe"
                headerAction={
                  <Action
                    onClick={() => setView((v) => ({ ...v, trace: null, span: null }))}
                    title="Close this run and give the list the whole screen"
                  >
                    close
                  </Action>
                }
              />
            ) : null
          }
        />
      </div>
    </TooltipProvider>
  )
}

/* -------------------------------------------------------------- split --- */

/**
 * The list above, the run below, and a seam between them you can move.
 *
 * The seam is a real control: drag it, or focus it and use the arrow keys. Its
 * position is a preference, because how much list an operator wants against how
 * much waterfall is a decision about the work they are doing, not one this
 * screen can make for them.
 */
function Split({
  fraction,
  onFraction,
  open,
  list,
  detail,
}: {
  fraction: number
  onFraction: (f: number) => void
  open: boolean
  list: React.ReactNode
  detail: React.ReactNode
}) {
  const box = useRef<HTMLDivElement>(null)
  const [dragging, setDragging] = useState(false)

  const set = (clientY: number) => {
    const el = box.current
    if (!el) return
    const r = el.getBoundingClientRect()
    if (r.height < 120) return
    onFraction(Math.min(0.8, Math.max(0.2, (clientY - r.top) / r.height)))
  }

  return (
    <div
      ref={box}
      className="grid min-h-0"
      style={{
        gridTemplateRows: open ? `minmax(4rem, ${fraction}fr) auto minmax(6rem, ${1 - fraction}fr)` : 'minmax(0, 1fr)',
      }}
    >
      {list}
      {open && (
        <div
          role="separator"
          aria-orientation="horizontal"
          aria-label="Resize the list"
          aria-valuenow={Math.round(fraction * 100)}
          aria-valuemin={20}
          aria-valuemax={80}
          tabIndex={0}
          onPointerDown={(e) => {
            e.currentTarget.setPointerCapture(e.pointerId)
            setDragging(true)
          }}
          onPointerMove={(e) => dragging && set(e.clientY)}
          onPointerUp={(e) => {
            e.currentTarget.releasePointerCapture(e.pointerId)
            setDragging(false)
          }}
          onKeyDown={(e) => {
            if (e.key === 'ArrowUp') {
              e.preventDefault()
              onFraction(Math.max(0.2, fraction - 0.04))
            }
            if (e.key === 'ArrowDown') {
              e.preventDefault()
              onFraction(Math.min(0.8, fraction + 0.04))
            }
          }}
          className="group grid h-2.5 cursor-row-resize place-items-center bg-panel-1"
          title="Drag, or use the arrow keys, to give more of the screen to the list or to the run"
        >
          {/* The grip: three short rules with round caps, the same stroke the
              rest of the panel is drawn in. It darkens under the pointer rather
              than growing or lighting up. */}
          <svg width="22" height="5" viewBox="0 0 22 5" aria-hidden="true" className="text-ink-3 transition-colors group-hover:text-ink-2">
            <path
              d="M2 2.5 H6 M9 2.5 H13 M16 2.5 H20"
              stroke="currentColor"
              strokeWidth="1.4"
              strokeLinecap="round"
            />
          </svg>
        </div>
      )}
      {open && detail}
    </div>
  )
}

/* ------------------------------------------------------------- states --- */

function EmptyState({
  filtered,
  presence,
  project,
  phrase,
  range,
  onClear,
}: {
  filtered: boolean
  presence: 'unknown' | 'some' | 'none'
  project: string
  phrase: string
  range: string
  onClear: () => void
}) {
  if (!filtered || presence === 'none') {
    return (
      <div>
        <p className="display text-[1.0625rem]">
          Nothing has run in <span className="data">{project}</span> yet.
        </p>
        <p className="mt-2 max-w-[54ch] text-[0.875rem] leading-relaxed text-ink-2">
          {filtered
            ? 'Not under these filters, and not without them either: this project holds no runs at all.'
            : 'Send one and it appears here within a few seconds. The demo daemon seeds a handful to look at:'}
        </p>
        {!filtered && (
          <div className="well mt-3 flex w-fit items-center gap-2.5 px-3.5 py-2.5">
            <span className="data select-none text-[0.8125rem] text-ink-3">$</span>
            <code className="data text-[0.8125rem] text-ink">mcpd demo</code>
          </div>
        )}
      </div>
    )
  }
  return (
    <div>
      <p className="display text-[1.0625rem]">No run matches.</p>
      <p className="mt-2 max-w-[54ch] text-[0.875rem] leading-relaxed text-ink-2">
        <span className="data text-ink">{project}</span> holds runs, but none {range}
        {phrase ? ` with ${phrase}` : ''}.
      </p>
      <button
        type="button"
        onClick={onClear}
        className="legend panel-raised mt-3.5 px-2.5 py-1.5 text-[0.5625rem] text-ink-3 transition-colors hover:bg-panel-3 hover:text-ink"
      >
        clear every filter
      </button>
    </div>
  )
}

function FailureState({
  failure,
  project,
  endpoint,
  apiKey,
  onKey,
  onRetry,
  backend,
}: {
  failure: Failure
  project: string
  endpoint: string
  apiKey: string
  onKey: (k: string) => void
  onRetry: () => void
  backend?: string
}) {
  const [draft, setDraft] = useState(apiKey)

  const head =
    failure.kind === 'auth'
      ? 'This daemon wants a key.'
      : failure.kind === 'forbidden'
        ? `That key may not read ${project}.`
        : failure.kind === 'capability'
          ? 'This backend cannot answer that.'
          : failure.kind === 'rejected'
            ? // The daemon names the parameter it refused, so the panel can name
              // the control the reader has to go and change.
              `The daemon would not take the ${
                (failure.field && FILTER_BY_ID.get(failure.field)?.label) ?? 'filter'
              } you set.`
            : failure.kind === 'gone'
              ? 'The daemon stopped answering.'
              : 'The query failed.'

  const body =
    failure.kind === 'auth'
      ? 'The daemon answered 401. Either no key was sent, or the one this browser holds has expired. Paste a read key and the whole screen uses it, including the run below.'
      : failure.kind === 'forbidden'
        ? `The daemon answered 403, which is a different fact from a missing key: it read the key and refused this project. ${failure.message}`
        : failure.kind === 'capability'
          ? backend
            ? capabilityNote(backend, failure.capability)
            : failure.message
          : failure.kind === 'rejected'
            ? failure.message
            : failure.kind === 'gone'
              ? `Nothing answered at ${failure.endpoint}. It was answering a moment ago, so it has most likely stopped or the port forward has dropped.`
              : failure.message

  return (
    <div>
      <p className="display text-[1.0625rem]" style={{ color: 'var(--color-oxide)' }}>
        {head}
      </p>
      <p className="mt-2 max-w-[58ch] text-[0.875rem] leading-relaxed text-ink-2">{body}</p>

      {(failure.kind === 'auth' || failure.kind === 'forbidden') && (
        <div className="mt-4 flex flex-wrap items-end gap-2.5">
          <label className="min-w-[min(16rem,100%)]">
            <span className="legend mb-1.5 block text-ink-3">Read key</span>
            <input
              type="password"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && onKey(draft)}
              spellCheck={false}
              aria-label="Read key"
              placeholder={`for ${endpoint.replace(/^https?:\/\//, '')}`}
              className="well data w-full px-3 py-2 text-[0.8125rem] text-ink outline-none"
            />
          </label>
          <button
            type="button"
            onClick={() => onKey(draft)}
            className="legend panel-raised px-3 py-2.5 text-ink transition-colors hover:bg-panel-3"
          >
            Use this key
          </button>
        </div>
      )}

      {failure.kind !== 'auth' && failure.kind !== 'forbidden' && (
        <button
          type="button"
          onClick={onRetry}
          className="legend panel-raised mt-3.5 px-2.5 py-1.5 text-[0.5625rem] text-ink-3 transition-colors hover:bg-panel-3 hover:text-ink"
        >
          ask again
        </button>
      )}
    </div>
  )
}

export function Disconnected({
  conn,
  endpoint,
  onEndpoint,
  onRetry,
}: {
  conn: Conn
  endpoint: string
  onEndpoint: (v: string) => void
  onRetry: () => void
}) {
  const [draft, setDraft] = useState(endpoint)
  // The endpoint moves under this panel when the fallback to the default port
  // runs, and the field has to follow it: a field still naming the docs host
  // beside a headline naming localhost:4317 would be two answers to one
  // question.
  useEffect(() => setDraft(endpoint), [endpoint])

  return (
    <div className="mx-auto grid min-h-[calc(100vh-12rem)] max-w-[46rem] place-items-center px-5 py-16">
      {/* The headline carries the state. It wore a legend above it, which is
          the eyebrow-over-heading shape the panel refuses. */}
      <div className="w-full">
        {conn.state === 'connecting' && (
          <>
            <p className="display text-[1.75rem]">Looking for a daemon…</p>
            {/* The one honest progress indication on this site: a port is being
                probed and the answer is not known yet. It stops when the answer
                arrives rather than spinning for ever. */}
            <div className="probe ticks mt-5 w-full max-w-[22rem]" aria-hidden="true" />
            <p className="mt-3 text-[0.875rem] text-ink-3">
              Probing <span className="data text-ink-2">{endpoint}</span>.
            </p>
          </>
        )}

        {conn.state === 'absent' && (
          <>
            <h1 className="display text-[clamp(1.6rem,3.4vw,2.25rem)]">
              No daemon is listening at {conn.endpoint}.
            </h1>
            <p className="mt-4 max-w-[58ch] text-[0.9375rem] leading-relaxed text-ink-2">
              This interface is a static page. It reads whichever mcpd you point it
              at, and there is no server of ours in between, so if no daemon is
              running there is nothing to show. Start one:
            </p>
            <div className="well mt-5 flex items-center gap-2.5 rounded-[2px] px-3.5 py-3">
              <span className="data select-none text-[0.8125rem] text-ink-3">$</span>
              <code className="data text-[0.8125rem] text-ink">
                docker run -p 4317:4317 ghcr.io/mcpd/mcpd demo
              </code>
            </div>
            <p className="mt-3 text-[0.875rem] text-ink-3">
              The <code className="data text-ink">demo</code> argument seeds a sample
              run so there is something to look at.
            </p>
          </>
        )}

        {conn.state === 'refused' && (
          <>
            <h1 className="display text-[clamp(1.6rem,3.4vw,2.25rem)]">
              The daemon refused the request.
            </h1>
            <p className="mt-4 max-w-[58ch] text-[0.9375rem] leading-relaxed text-ink-2">
              {conn.message}
            </p>
            <p className="mt-3 max-w-[58ch] text-[0.875rem] text-ink-3">
              If it is asking for a key, this daemon has a read key configured.
              Append <code className="data text-ink">?daemon=…</code> to the URL to
              point at a different one.
            </p>
          </>
        )}

        <div className="mt-8 flex flex-wrap items-end gap-3">
          {/* `min-w-[18rem]` alone is 288px, which is wider than the content box
              of a 320px screen, so the grid track grew to fit it and the whole
              interface scrolled sideways. The field still wants a comfortable
              width; it just must not demand one the screen cannot give. */}
          <label className="min-w-[min(18rem,100%)] flex-1">
            <span className="legend mb-2 block text-ink-3">Endpoint</span>
            <input
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && onEndpoint(draft)}
              spellCheck={false}
              className="well data w-full rounded-[2px] px-3 py-2.5 text-[0.8125rem] text-ink outline-none focus-visible:outline-2 focus-visible:outline-amber"
            />
          </label>
          <button
            type="button"
            onClick={() => (draft === endpoint ? onRetry() : onEndpoint(draft))}
            className="legend panel-raised px-4 py-3 text-ink transition-colors hover:bg-panel-3"
          >
            Connect
          </button>
        </div>
      </div>
    </div>
  )
}
