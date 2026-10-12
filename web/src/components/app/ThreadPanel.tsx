/**
 * The thread screen: every run in one conversation thread, read in place.
 *
 * mcpd records `thread_id` on the correlation of every span, and the trace list
 * filters by it. What was missing was anywhere to spend that: a log line or a
 * run told you which thread it belonged to and then left you to search for the
 * rest by hand.
 *
 * The shape here is deliberate. The index on the left is built from the runs
 * this project has recently held, so it can only show threads that appear in
 * them, and it says so rather than implying it is the whole set; a thread id
 * from anywhere else can be typed. Opening a run does not leave the thread:
 * the panel appears beside the index, and prev and next walk that thread's own
 * list, so reading six runs of one conversation is six presses rather than six
 * round trips through the explorer.
 */

import { useCallback, useEffect, useMemo, useState } from 'react'
import { api, ago, duration, money } from '@/lib/api'
import type { Trace } from '@/generated/types'
import { Disconnected } from './Explorer'
import { DaemonTile, ProjectPicker, useConnection } from './shell'
import { TracePanel } from './TracePanel'
import { Action, CopyValue, SearchField } from './trace-ui'
import type { TraceQuery } from './Messages'

/** How many recent runs the thread index is built from. */
const SAMPLE = 200

/** How many runs of one thread are listed. */
const THREAD_PAGE = 100

interface ThreadRow {
  id: string
  runs: number
  /** The most recent start in the sample, so the index sorts by activity. */
  last: string
  faults: number
}

/**
 * The threads visible in a sample of recent runs.
 *
 * There is no thread index on the daemon and no discovery facet for one, so
 * this is derived rather than read. Everything that follows from that is said
 * on screen: the counts are counts within the sample, and opening a thread asks
 * the daemon for that thread's own list rather than trusting these rows.
 */
function threadsOf(traces: Trace[]): { threads: ThreadRow[]; unthreaded: number } {
  const by = new Map<string, ThreadRow>()
  let unthreaded = 0
  for (const t of traces) {
    const id = t.correlation.thread_id
    if (!id) {
      unthreaded += 1
      continue
    }
    const row = by.get(id)
    if (row) {
      row.runs += 1
      if (t.start_time > row.last) row.last = t.start_time
      if (t.status === 'error') row.faults += 1
    } else {
      by.set(id, {
        id,
        runs: 1,
        last: t.start_time,
        faults: t.status === 'error' ? 1 : 0,
      })
    }
  }
  const threads = [...by.values()].sort((a, b) => (a.last < b.last ? 1 : a.last > b.last ? -1 : 0))
  return { threads, unthreaded }
}

/* ---------------------------------------------------------------- index --- */

function ThreadIndex({
  threads,
  unthreaded,
  sampled,
  loading,
  error,
  selected,
  onSelect,
}: {
  threads: ThreadRow[]
  unthreaded: number
  sampled: number
  loading: boolean
  error: string | null
  selected: string | null
  onSelect: (id: string) => void
}) {
  return (
    <div className="grid min-h-0 grid-rows-[minmax(0,1fr)_auto] border-b border-scribe-2 lg:border-b-0 lg:border-r">
      {error ? (
        <div className="p-5">
          <p className="legend text-oxide">Query failed</p>
          <p className="mt-2 text-[0.875rem] text-ink-2">{error}</p>
        </div>
      ) : !threads.length ? (
        <div className="p-5">
          <p className="legend text-ink-3">{loading ? 'Reading' : 'No thread here'}</p>
          <p className="mt-2 max-w-[34ch] text-[0.875rem] leading-relaxed text-ink-2">
            {loading
              ? 'Asking the daemon.'
              : sampled === 0
                ? 'This project has no runs yet.'
                : `None of the last ${sampled} runs carries a thread id. Set thread_id on the correlation and runs of one conversation group here.`}
          </p>
        </div>
      ) : (
        <ol className="min-h-0 divide-y divide-scribe-2 overflow-y-auto" role="listbox" aria-label="Threads">
          {threads.map((t) => {
            const on = t.id === selected
            return (
              <li key={t.id}>
                <button
                  type="button"
                  role="option"
                  aria-selected={on}
                  onClick={() => onSelect(t.id)}
                  className={`w-full px-4 py-3 text-left transition-colors ${
                    on ? 'bg-panel-3' : 'hover:bg-panel-2'
                  }`}
                >
                  <div className="flex items-baseline gap-2">
                    <span className="data truncate text-[0.8125rem] text-ink" title={t.id}>
                      {t.id}
                    </span>
                    <span className="data ml-auto shrink-0 text-[0.6875rem] text-ink-3" title={t.last}>
                      {ago(t.last)}
                    </span>
                  </div>
                  <div className="mt-1.5 flex flex-wrap items-baseline gap-x-3">
                    <span className="data text-[0.6875rem] text-ink-3">
                      {t.runs} run{t.runs === 1 ? '' : 's'} in the sample
                    </span>
                    {t.faults > 0 && (
                      <span className="data text-[0.6875rem] text-oxide">{t.faults} faulted</span>
                    )}
                  </div>
                </button>
              </li>
            )
          })}
        </ol>
      )}

      {/* What this index is, stated where it is read rather than in a tooltip.
          A derived list that presents itself as the whole set is the thing that
          sends someone looking for a thread that was never going to be here. */}
      <p className="border-t border-scribe-2 px-4 py-3 text-[0.75rem] leading-snug text-ink-3">
        Built from the last {sampled} run{sampled === 1 ? '' : 's'} of this project, because the
        daemon keeps no thread index.
        {unthreaded > 0 &&
          ` ${unthreaded} of them ${unthreaded === 1 ? 'carries' : 'carry'} no thread id.`}{' '}
        A thread id from anywhere else can be typed above.
      </p>
    </div>
  )
}

/* ----------------------------------------------------------------- runs --- */

function ThreadRuns({
  traces,
  loading,
  error,
  threadId,
  onOpen,
}: {
  traces: Trace[]
  loading: boolean
  error: string | null
  threadId: string
  onOpen: (id: string) => void
}) {
  if (error) {
    return (
      <div className="p-5">
        <p className="legend text-oxide">Query failed</p>
        <p className="mt-2 text-[0.875rem] text-ink-2">{error}</p>
      </div>
    )
  }
  if (!traces.length) {
    return (
      <div className="p-5">
        <p className="legend text-ink-3">{loading ? 'Reading' : 'Nothing here'}</p>
        <p className="mt-2 max-w-[46ch] text-[0.875rem] leading-relaxed text-ink-2">
          {loading
            ? 'Asking the daemon for this thread.'
            : `The daemon holds no run whose thread id is ${threadId}. It may have fallen out of the retention window.`}
        </p>
      </div>
    )
  }
  return (
    <ol className="min-h-0 divide-y divide-scribe-2 overflow-y-auto" aria-label={`Runs in thread ${threadId}`}>
      {traces.map((t, i) => (
        <li key={t.trace_id}>
          <button
            type="button"
            onClick={() => onOpen(t.trace_id)}
            className="grid w-full grid-cols-[2.5rem_minmax(0,1fr)] items-baseline gap-x-4 px-5 py-3.5 text-left transition-colors hover:bg-panel-2"
          >
            {/* Position in the thread, which is what prev and next walk. */}
            <span className="data text-right text-[0.6875rem] text-ink-3">{i + 1}</span>
            <span className="min-w-0">
              <span className="flex items-baseline gap-2">
                <span
                  className={`truncate text-[0.9375rem] ${
                    t.status === 'error' ? 'text-oxide' : 'text-ink'
                  }`}
                >
                  {t.name}
                </span>
                {t.status === 'error' && (
                  <span className="data shrink-0 text-[0.625rem] text-oxide">fault</span>
                )}
                <span className="data ml-auto shrink-0 text-[0.6875rem] text-ink-3" title={t.start_time}>
                  {ago(t.start_time)}
                </span>
              </span>
              <span className="mt-1 flex flex-wrap items-baseline gap-x-3 gap-y-1">
                {t.duration_ns != null && (
                  <span className="data text-[0.6875rem] text-ink-3">
                    {duration(t.duration_ns)}
                  </span>
                )}
                <span className="data text-[0.6875rem] text-ink-3">{t.span_count} steps</span>
                {t.cost?.total_cost != null && (
                  <span className="data text-[0.6875rem] text-ink-3">
                    {money(t.cost.total_cost, t.cost.currency)}
                  </span>
                )}
                {!t.is_complete && <span className="data text-[0.6875rem] text-amber">running</span>}
              </span>
            </span>
          </button>
        </li>
      ))}
    </ol>
  )
}

/* --------------------------------------------------------------- screen --- */

export default function Threads({
  endpoint,
  onEndpoint,
  project,
  onProject,
  initialThread,
  initialTrace,
  onOpenConversation,
}: {
  endpoint: string
  onEndpoint: (v: string) => void
  project: string
  onProject: (v: string) => void
  initialThread?: string
  initialTrace?: string
  /** Read one run of this thread as the conversation its spans encode. */
  onOpenConversation: (traceId: string) => void
}) {
  const { conn, projects, retry } = useConnection(endpoint, onEndpoint)

  const [sample, setSample] = useState<Trace[]>([])
  const [sampleLoading, setSampleLoading] = useState(false)
  const [sampleError, setSampleError] = useState<string | null>(null)

  const [threadId, setThreadId] = useState<string | null>(initialThread ?? null)
  const [typed, setTyped] = useState(initialThread ?? '')
  const [runs, setRuns] = useState<Trace[]>([])
  const [runsLoading, setRunsLoading] = useState(false)
  const [runsError, setRunsError] = useState<string | null>(null)
  const [open, setOpen] = useState<string | null>(initialTrace ?? null)
  const [openSpan, setOpenSpan] = useState<string | null>(null)

  /* ---- the index ---- */

  useEffect(() => {
    if (conn.state !== 'connected') return
    const ac = new AbortController()
    setSampleLoading(true)
    setSampleError(null)
    const params: TraceQuery = { project, sort: 'start_time', order: 'desc', limit: SAMPLE }
    api
      .traces(params, { endpoint, signal: ac.signal })
      .then((res) => setSample(res.traces))
      .catch((e) => {
        if ((e as Error).name === 'AbortError') return
        setSampleError(e instanceof Error ? e.message : String(e))
      })
      .finally(() => setSampleLoading(false))
    return () => ac.abort()
  }, [project, endpoint, conn.state])

  const { threads, unthreaded } = useMemo(() => threadsOf(sample), [sample])

  /* ---- one thread's own list ---- */

  useEffect(() => {
    if (!threadId || conn.state !== 'connected') {
      setRuns([])
      return
    }
    const ac = new AbortController()
    setRunsLoading(true)
    setRunsError(null)
    const params: TraceQuery = {
      project,
      thread_id: threadId,
      sort: 'start_time',
      order: 'asc',
      limit: THREAD_PAGE,
    }
    api
      .traces(params, { endpoint, signal: ac.signal })
      .then((res) => setRuns(res.traces))
      .catch((e) => {
        if ((e as Error).name === 'AbortError') return
        setRunsError(e instanceof Error ? e.message : String(e))
      })
      .finally(() => setRunsLoading(false))
    return () => ac.abort()
  }, [threadId, project, endpoint, conn.state])

  const chooseThread = useCallback((id: string) => {
    setThreadId(id)
    setTyped(id)
    setOpen(null)
    setOpenSpan(null)
  }, [])

  const at = open ? runs.findIndex((t) => t.trace_id === open) : -1
  const prev = at > 0 ? runs[at - 1] : null
  const next = at >= 0 && at < runs.length - 1 ? runs[at + 1] : null

  const step = useCallback((t: Trace | null) => {
    if (!t) return
    setOpen(t.trace_id)
    setOpenSpan(null)
  }, [])

  if (conn.state !== 'connected') {
    return (
      <Disconnected conn={conn} endpoint={endpoint} onEndpoint={onEndpoint} onRetry={retry} />
    )
  }

  return (
    <div className="grid min-h-[calc(100vh-8rem)] grid-rows-[auto_minmax(0,1fr)]">
      <div className="border-b border-scribe bg-panel-1">
        <div className="flex flex-wrap items-center gap-2.5 px-4 py-2.5">
          <DaemonTile health={conn.health} endpoint={endpoint} />
          <ProjectPicker projects={projects} project={project} onProject={onProject} />
          {/* A thread id is exact, so this is a lookup rather than a search:
              the daemon matches it whole. */}
          <form
            className="flex min-w-0 flex-1 items-center gap-2 lg:max-w-[26rem] lg:flex-none"
            onSubmit={(e) => {
              e.preventDefault()
              if (typed.trim()) chooseThread(typed.trim())
            }}
          >
            <SearchField
              className="min-w-0 flex-1 lg:w-[20rem]"
              value={typed}
              onChange={setTyped}
              label="Thread id"
              placeholder="thread id"
            />
            <Action
              onClick={() => typed.trim() && chooseThread(typed.trim())}
              disabled={!typed.trim() || typed.trim() === threadId}
              title="Read the runs in this thread"
            >
              open
            </Action>
          </form>
          <span className="data text-[0.75rem] text-ink-3 lg:ml-auto">
            {sampleLoading
              ? 'reading…'
              : `${threads.length} thread${threads.length === 1 ? '' : 's'}`}
          </span>
        </div>
        <div className="ticks opacity-40" aria-hidden="true" />
      </div>

      <div className="grid min-h-0 lg:grid-cols-[minmax(0,22rem)_minmax(0,1fr)]">
        <ThreadIndex
          threads={threads}
          unthreaded={unthreaded}
          sampled={sample.length}
          loading={sampleLoading}
          error={sampleError}
          selected={threadId}
          onSelect={chooseThread}
        />

        <section className="grid min-h-0 overflow-hidden">
          {!threadId ? (
            <div className="grid place-content-center p-10">
              <p className="max-w-[46ch] text-center text-[0.9375rem] leading-relaxed text-ink-2">
                Choose a thread to read the runs in it. Each one opens here, beside the index,
                with previous and next walking that thread in the order it happened.
              </p>
            </div>
          ) : (
            <div className="grid min-h-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden">
              <header className="flex flex-wrap items-baseline gap-x-4 gap-y-2 border-b border-scribe-2 px-5 py-3">
                <h2 className="legend text-ink-3">Thread</h2>
                <CopyValue value={threadId} className="data text-[0.8125rem] text-ink">
                  <span>{threadId}</span>
                </CopyValue>
                <span className="data text-[0.6875rem] text-ink-3">
                  {runsLoading
                    ? 'reading…'
                    : `${runs.length} run${runs.length === 1 ? '' : 's'}`}
                </span>

                <div className="ml-auto flex flex-wrap items-center gap-1.5">
                  {/* Prev and next are drawn only while a run is open, because
                      with nothing open they have no run to be previous to, and
                      a disabled control whose reason names a position the
                      reader is not at explains nothing. */}
                  {at >= 0 && (
                    <>
                      <Action
                        onClick={() => step(prev)}
                        disabled={!prev}
                        title={
                          prev
                            ? `Previous run: ${prev.name}`
                            : 'This is the first run in this thread'
                        }
                      >
                        prev
                      </Action>
                      <Action
                        onClick={() => step(next)}
                        disabled={!next}
                        title={
                          next ? `Next run: ${next.name}` : 'This is the last run in this thread'
                        }
                      >
                        next
                      </Action>
                      <span className="data text-[0.6875rem] text-ink-3">
                        run {at + 1} of {runs.length}
                      </span>
                    </>
                  )}
                  {open && (
                    <Action
                      onClick={() => {
                        setOpen(null)
                        setOpenSpan(null)
                      }}
                      title="Back to every run in this thread"
                    >
                      all runs
                    </Action>
                  )}
                </div>
              </header>

              {open ? (
                <TracePanel
                  key={open}
                  traceId={open}
                  project={project}
                  endpoint={endpoint}
                  spanId={openSpan}
                  onSpanChange={setOpenSpan}
                  className="min-h-0"
                  headerAction={
                    <Action
                      onClick={() => onOpenConversation(open)}
                      title="Read this run as the conversation its steps encode"
                    >
                      conversation
                    </Action>
                  }
                />
              ) : (
                <ThreadRuns
                  traces={runs}
                  loading={runsLoading}
                  error={runsError}
                  threadId={threadId}
                  onOpen={(id) => {
                    setOpen(id)
                    setOpenSpan(null)
                  }}
                />
              )}
            </div>
          )}
        </section>
      </div>
    </div>
  )
}
