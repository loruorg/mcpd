/**
 * One run, read in full: the header, the waterfall, the search over it and the
 * step inspector beside it.
 *
 * This is a whole screen's worth of behaviour in one component on purpose. The
 * trace view used to be welded into the runs explorer, so every other place a
 * run needed reading, a log line, a workflow step, a review, had to send the
 * reader to a different screen or copy the waterfall. Mount this instead: it
 * takes a trace id and gives back the tree, the tabs, the search and prev/next.
 *
 * It fetches for itself unless the host already holds the detail, because the
 * common case is a host that has a trace id and nothing else.
 */

import { useCallback, useEffect, useId, useMemo, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import type { Span, TraceDetail } from '@/generated/types'
import {
  api,
  capabilityNote,
  DaemonError,
  duration,
  money,
  tokens,
  traceCostBlank,
} from '@/lib/api'
import { useCapabilities } from './capabilities'
import { SpanInspector } from './SpanInspector'
import { Legend, Waterfall } from './Waterfall'
import {
  buildHaystack,
  buildTree,
  descendantIds,
  extentOf,
  flatten,
  indexNodes,
  kindsPresent,
  searchTree,
} from './trace-tree'
import { Action, SearchField, useDebounced } from './trace-ui'

export interface TracePanelProps {
  /** The run to read. */
  traceId: string
  /** The project it belongs to. Every read the daemon serves is scoped by it. */
  project: string
  /** Which daemon to read. */
  endpoint: string
  /** A bearer key, where the daemon needs one. */
  apiKey?: string
  /** The step to open when the panel first shows this run. */
  spanId?: string | null
  /** Called whenever the reader opens a different step, or closes the open one. */
  onSpanChange?: (spanId: string | null) => void
  /**
   * A detail the host already holds.
   *
   * Pass it and the panel renders it and fetches nothing; leave it undefined and
   * the panel fetches the run itself. `null` means the host is still loading.
   */
  detail?: TraceDetail | null
  /** Extra classes on the panel's outermost element, for a host with its own frame. */
  className?: string
  /** A control of the host's own, shown in the header beside the panel's. */
  headerAction?: ReactNode
}

/** How often a run that is still open is polled for new steps. */
const FOLLOW_MS = 2500

/** One array, so a panel with nothing to show does not rebuild its tree per render. */
const NO_SPANS: Span[] = []

export function TracePanel({
  traceId,
  project,
  endpoint,
  apiKey,
  spanId = null,
  onSpanChange,
  detail: given,
  className = '',
  headerAction,
}: TracePanelProps) {
  const [own, setOwn] = useState<TraceDetail | null>(null)
  const [failure, setFailure] = useState<string | null>(null)
  const [selected, setSelected] = useState<string | null>(spanId)
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set())
  const [raw, setRaw] = useState('')
  const [follow, setFollow] = useState(true)
  const [busy, setBusy] = useState<'idle' | 'reading' | 'downloading'>('idle')
  const query = useDebounced(raw, 140)
  // What this deployment can answer decides which controls the panel offers. A
  // refusal costs the follow control, not the panel.
  const caps = useCapabilities(endpoint, apiKey)

  // Unique per mount, so two panels on one page do not both claim the same
  // inspector id and leave every row pointing at the other's.
  const inspectorId = `mcpd-span-inspector-${useId()}`

  const detail = given !== undefined ? given : own
  const fetching = given === undefined

  const read = useCallback(
    async (signal?: AbortSignal) => {
      try {
        const d = await api.trace(traceId, { project, include: 'all' }, { endpoint, apiKey, signal })
        setOwn(d)
        setFailure(null)
      } catch (e) {
        if ((e as { name?: string }).name === 'AbortError') return
        setFailure(
          e instanceof DaemonError ? e.message : `This run could not be read from ${endpoint}.`,
        )
      }
    },
    [traceId, project, endpoint, apiKey],
  )

  useEffect(() => {
    if (!fetching) return
    const ac = new AbortController()
    setOwn(null)
    void read(ac.signal)
    return () => ac.abort()
  }, [fetching, read])

  useEffect(() => setSelected(spanId), [spanId])

  const choose = useCallback(
    (id: string | null) => {
      setSelected(id)
      onSpanChange?.(id)
    },
    [onSpanChange],
  )

  const spans = detail?.spans ?? NO_SPANS
  const roots = useMemo(() => buildTree(spans), [spans])
  const nodes = useMemo(() => indexNodes(roots), [roots])
  const ext = useMemo(() => extentOf(spans), [spans])
  const haystack = useMemo(() => buildHaystack(spans), [spans])
  const search = useMemo(
    () => searchTree(roots, nodes, haystack, query),
    [roots, nodes, haystack, query],
  )
  const rows = useMemo(
    () => flatten(roots, collapsed, search?.visible),
    [roots, collapsed, search],
  )
  const kinds = useMemo(() => kindsPresent(rows), [rows])

  // A match hidden inside a branch the reader folded away earlier is a match
  // they cannot see. Opening the ancestors of what the search kept is the least
  // that can be done about it, and it leaves every unrelated fold alone.
  useEffect(() => {
    if (!search) return
    setCollapsed((prev) => {
      const next = new Set([...prev].filter((id) => !search.visible.has(id)))
      return next.size === prev.size ? prev : next
    })
  }, [search])

  const toggle = useCallback((id: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev)
      if (!next.delete(id)) next.add(id)
      return next
    })
  }, [])

  const setDescendants = useCallback(
    (id: string, open: boolean) => {
      const node = nodes.get(id)
      if (!node) return
      const ids = [id, ...descendantIds(node)]
      setCollapsed((prev) => {
        const next = new Set(prev)
        for (const d of ids) {
          if (open) next.delete(d)
          else if (nodes.get(d)?.children.length) next.add(d)
        }
        return next
      })
    },
    [nodes],
  )

  const setSiblings = useCallback(
    (parentId: string | undefined, open: boolean) => {
      const family = parentId ? (nodes.get(parentId)?.children ?? []) : roots
      setCollapsed((prev) => {
        const next = new Set(prev)
        for (const c of family) {
          if (!c.children.length) continue
          if (open) next.delete(c.span.span_id)
          else next.add(c.span.span_id)
        }
        return next
      })
    },
    [nodes, roots],
  )

  const collapseAll = useCallback(() => {
    const all = new Set<string>()
    for (const [id, n] of nodes) if (n.children.length) all.add(id)
    setCollapsed(all)
  }, [nodes])

  /* --------------------------------------------------------- following --- */

  const cursor = useRef<string | null>(null)
  const canFollow = caps?.delta_polling !== false
  const live = ext.live && !!detail && !detail.trace.is_complete
  const following = follow && live && canFollow && fetching

  useEffect(() => {
    cursor.current = null
  }, [traceId, endpoint])

  const runStart = detail?.trace.start_time
  useEffect(() => {
    if (!following || !runStart) return
    let stop = false
    const ac = new AbortController()

    const tick = async () => {
      try {
        const page = await api.deltas(
          {
            project,
            signal: 'spans',
            cursor: cursor.current ?? undefined,
            // Without a cursor, start at the run's own beginning rather than at
            // everything the project holds.
            since: cursor.current ? undefined : runStart,
            limit: 200,
          },
          { endpoint, apiKey, signal: ac.signal },
        )
        cursor.current = page.cursor
        const mine = (page.spans ?? []).filter((s: Span) => s.trace_id === traceId)
        if (mine.length && !stop) {
          setOwn((prev) => {
            if (!prev) return prev
            const by = new Map(prev.spans.map((s) => [s.span_id, s]))
            for (const s of mine) by.set(s.span_id, s)
            return { ...prev, spans: [...by.values()] }
          })
        }
      } catch {
        // A poll that fails is not worth a message: the panel still shows the
        // run it read, and the next tick either works or the reader reloads.
      }
    }

    const t = window.setInterval(() => void tick(), FOLLOW_MS)
    void tick()
    return () => {
      stop = true
      ac.abort()
      window.clearInterval(t)
    }
    // Deliberately keyed on the run's start rather than on the detail object:
    // depending on the detail would tear down and restart this loop every time
    // a step arrived, which is every time it does its job.
  }, [following, runStart, project, endpoint, apiKey, traceId])

  /* ---------------------------------------------------------- download --- */

  const download = useCallback(async () => {
    setBusy('downloading')
    try {
      // Read it again rather than saving what is on screen: the panel holds
      // what it needed to draw, and a file called `trace-<id>.json` should be
      // the run, not the run minus whatever the panel did not ask for.
      const full = await api.trace(traceId, { project, include: 'all' }, { endpoint, apiKey })
      const url = URL.createObjectURL(
        new Blob([JSON.stringify(full, null, 2)], { type: 'application/json' }),
      )
      const a = document.createElement('a')
      a.href = url
      a.download = `trace-${traceId}.json`
      document.body.appendChild(a)
      a.click()
      a.remove()
      URL.revokeObjectURL(url)
    } catch (e) {
      setFailure(
        e instanceof DaemonError
          ? e.message
          : `This run could not be downloaded from ${endpoint}.`,
      )
    } finally {
      setBusy('idle')
    }
  }, [traceId, project, endpoint, apiKey])

  /* ------------------------------------------------------------ render --- */

  if (!detail) {
    return (
      <div className={`grid place-content-center p-10 ${className}`}>
        {failure ? (
          <p className="max-w-[42ch] text-center text-[0.875rem] leading-relaxed text-ink-2">
            {failure}
          </p>
        ) : (
          <p className="legend text-ink-3">Reading run…</p>
        )}
      </div>
    )
  }

  const open = selected ? (spans.find((s) => s.span_id === selected) ?? null) : null
  const trace = detail.trace
  const blank = traceCostBlank(trace, spans)

  return (
    <section
      className={`app-enter @container/panel grid min-h-0 grid-rows-[auto_auto_minmax(0,1fr)] overflow-hidden ${className}`}
      aria-label={`Run ${trace.name}`}
    >
      <header className="border-b border-scribe-2 px-5 py-3.5">
        <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
          <h2 className="display text-[1.125rem]">{trace.name}</h2>
          <span className="data text-[0.6875rem] text-ink-3">{trace.trace_id}</span>
          {trace.status === 'error' && (
            <span className="annunciator py-1 text-[0.625rem]" data-lit="oxide">
              Fault
            </span>
          )}
          <div className="ml-auto flex flex-wrap items-center gap-1.5">
            {headerAction}
            {/* Only where the panel owns the data. A host that passed its own
                detail refreshes it on its own terms, and a follow control that
                could not write into that detail would be a control that does
                nothing. */}
            {live && canFollow && fetching && (
              <Action
                onClick={() => setFollow((f) => !f)}
                pressed={following}
                title={
                  following
                    ? 'Following: new steps are polled as they arrive'
                    : 'Not following. Steps arriving now will not appear until you refresh.'
                }
              >
                {following ? 'following' : 'follow'}
              </Action>
            )}
            {fetching && (
              <Action
                onClick={() => {
                  setBusy('reading')
                  void read().finally(() => setBusy('idle'))
                }}
                disabled={busy === 'reading'}
                title="Read this run again"
              >
                {busy === 'reading' ? 'reading' : 'refresh'}
              </Action>
            )}
            <Action
              onClick={() => void download()}
              disabled={busy === 'downloading'}
              title="Save the whole run, including its logs, scores and feedback, as JSON"
            >
              {busy === 'downloading' ? 'saving' : 'download json'}
            </Action>
          </div>
        </div>

        <dl className="mt-2 flex flex-wrap gap-x-6 gap-y-1">
          <Reading label="Duration">
            {trace.duration_ns != null ? duration(trace.duration_ns) : 'not ended'}
          </Reading>
          <Reading label="Steps">{String(trace.span_count)}</Reading>
          <Reading label="Models">{String(trace.model_count)}</Reading>
          <Reading label="Tools">{String(trace.tool_count)}</Reading>
          <Reading label="Tokens">
            {trace.usage?.total_tokens != null
              ? tokens(trace.usage.total_tokens)
              : trace.usage?.input_tokens != null && trace.usage.output_tokens != null
                ? tokens(trace.usage.input_tokens + trace.usage.output_tokens)
                : 'none reported'}
          </Reading>
          <Reading label="Cost" title={blank?.long}>
            {trace.cost?.total_cost != null
              ? money(trace.cost.total_cost, trace.cost.currency)
              : (blank?.short ?? 'not priced')}
          </Reading>
        </dl>

        {live && !canFollow && caps && fetching && (
          <p className="mt-2 max-w-[64ch] text-[0.75rem] leading-snug text-ink-2">
            This run is still open. {capabilityNote(caps.backend, 'delta_polling')} Press refresh
            to see the steps that have arrived since.
          </p>
        )}
        {failure && (
          <p className="mt-2 max-w-[64ch] text-[0.75rem] leading-snug text-oxide">{failure}</p>
        )}
      </header>

      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-scribe-2 px-5 py-2.5">
        <SearchField
          className="w-full max-w-[22rem] @[46rem]/panel:w-[22rem]"
          value={raw}
          onChange={setRaw}
          label="Search this run"
          placeholder="Search names, ids, prompts and completions"
        />
        {search && (
          <span className="text-[0.75rem] text-ink-2">
            {search.count === 0 ? (
              'no step matches'
            ) : (
              <>
                <span className="data text-ink">{search.count}</span> of{' '}
                <span className="data text-ink">{spans.length}</span> steps match
                {search.payloadOnly.size > 0 && (
                  <>
                    , <span className="data text-ink">{search.payloadOnly.size}</span> in their
                    details only
                  </>
                )}
              </>
            )}
          </span>
        )}
        <div className="ml-auto flex items-center gap-1.5">
          <Action onClick={() => setCollapsed(new Set())} title="Open every branch">
            expand all
          </Action>
          <Action onClick={collapseAll} title="Fold every branch">
            collapse all
          </Action>
        </div>
        <div className="w-full">
          {/* `live` is read off the rows on screen, not off the run: a legend
              that keys a hatch nobody can see is a legend for another chart. */}
          <Legend
            kinds={kinds}
            live={rows.some((r) => !r.span.end_time && !r.span.is_event)}
          />
        </div>
      </div>

      {/* The inspector moves beside the waterfall only when the panel is wide
          enough for both. Measured against the PANEL and not the window,
          because this mounts inside a side sheet as readily as in a screen of
          its own, and a viewport breakpoint would put a 28rem inspector beside
          a 24rem waterfall the moment the window was wide and the panel was
          not. Below that it sits underneath, with the full width. */}
      {/* Stacked, the inspector is capped at just over half the height and
          scrolls inside that cap. An `auto` row here read the inspector's full
          content height, which on a step with a prompt in it is thousands of
          pixels, and squeezed the waterfall above it to nothing: the panel
          showed a step detail and no run. */}
      <div className="grid min-h-0 grid-rows-[minmax(8rem,1fr)_minmax(0,55%)] overflow-hidden @[68rem]/panel:grid-cols-[minmax(0,1fr)_minmax(0,27rem)] @[68rem]/panel:grid-rows-1">
        <Waterfall
          rows={rows}
          ext={ext}
          selected={selected}
          onSelect={(id) => choose(id === selected ? null : id)}
          collapsed={collapsed}
          onToggle={toggle}
          onDescendants={setDescendants}
          onSiblings={setSiblings}
          query={query}
          matched={search?.matched ?? null}
          payloadOnly={search?.payloadOnly ?? null}
          inspectorId={inspectorId}
          emptyNote={
            search
              ? 'Nothing in this run matches your search. It reads step names, ids, models, errors and the whole of every prompt, completion, attribute and metadata payload.'
              : 'This run holds no steps.'
          }
        />

        <SpanInspector
          id={inspectorId}
          span={open}
          detail={detail}
          rows={rows}
          onSelect={choose}
          query={query}
          payloadOnly={!!(open && search?.payloadOnly.has(open.span_id))}
          t0={ext.t0}
          className="@[68rem]/panel:border-l @[68rem]/panel:border-t-0"
        />
      </div>
    </section>
  )
}

function Reading({
  label,
  children,
  title,
}: {
  label: string
  children: ReactNode
  title?: string
}) {
  return (
    <div className="flex items-baseline gap-1.5">
      <dt className="legend text-ink-3">{label}</dt>
      <dd className="data text-[0.8125rem] text-ink" title={title}>
        {children}
      </dd>
    </div>
  )
}

export default TracePanel
