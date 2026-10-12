/**
 * The workflows screen: every durable run the daemon holds, with its attempts,
 * its waits and its journal beside it.
 *
 * A run is a row the daemon walks, so this screen is the row drawn: where the
 * cursor is, what each attempt answered, who served it, what the run is
 * waiting for, and the events in the order they were committed. The journal
 * follows a running run live, over the daemon's event stream when the browser
 * can hold one open and by polling when it cannot.
 *
 * Nothing here is inferred. A status is the daemon's word for it, an absent
 * cost is shown as absent, and a cancel is a request the daemon answers.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { api, ago, money } from '@/lib/api'
import type { Run, RunDetail, RunEvent, StepAttempt } from '@/generated/types'
import { Disconnected } from './Explorer'
import { clock, DaemonTile, ProjectPicker, useConnection } from './shell'

const PAGE = 50

/** The inspector's id, so a row can name the panel it drives. */
const INSPECTOR_ID = 'mcpd-run-inspector'

const STATUSES = ['running', 'succeeded', 'failed', 'timed_out', 'cancelled'] as const

/** The tone a status takes: the signal colours are for state, not decoration. */
const STATUS_TONE: Record<string, string> = {
  running: 'text-amber',
  succeeded: 'text-ink',
  failed: 'text-oxide',
  timed_out: 'text-oxide',
  cancelled: 'text-ink-3',
}

const ATTEMPT_TONE: Record<string, string> = {
  pending: 'text-ink-3',
  leased: 'text-amber',
  started: 'text-amber',
  orphaned: 'text-oxide',
  succeeded: 'text-ink',
  failed: 'text-oxide',
  timed_out: 'text-oxide',
  skipped: 'text-ink-3',
}

export default function Workflows({
  endpoint,
  onEndpoint,
  project,
  onProject,
  onOpenTrace,
  initialRun,
}: {
  endpoint: string
  onEndpoint: (v: string) => void
  project: string
  onProject: (v: string) => void
  onOpenTrace: (traceId: string) => void
  initialRun?: string
}) {
  const { conn, projects, retry } = useConnection(endpoint, onEndpoint)

  const [status, setStatus] = useState('')
  const [definition, setDefinition] = useState('')
  const [runs, setRuns] = useState<Run[]>([])
  const [cursor, setCursor] = useState<string | undefined>(undefined)
  const [more, setMore] = useState(false)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [openId, setOpenId] = useState<string | null>(initialRun ?? null)

  const params = useMemo(
    () => ({
      project,
      status: status || undefined,
      definition_id: definition.trim() || undefined,
      limit: PAGE,
    }),
    [project, status, definition],
  )

  const load = useCallback(
    async (signal?: AbortSignal) => {
      if (conn.state !== 'connected') return
      setLoading(true)
      setError(null)
      try {
        const res = await api.runs(params, { endpoint, signal })
        setRuns(res.runs)
        setCursor(res.page.next_cursor)
        setMore(res.page.has_more)
      } catch (e) {
        if ((e as Error).name === 'AbortError') return
        setError(e instanceof Error ? e.message : String(e))
      } finally {
        setLoading(false)
      }
    },
    [conn.state, params, endpoint],
  )

  useEffect(() => {
    const ac = new AbortController()
    void load(ac.signal)
    return () => ac.abort()
  }, [load])

  // The list follows the daemon: a run that ends moves in the ordering and
  // its status changes, and an operator watching the list expects to see it.
  useEffect(() => {
    if (conn.state !== 'connected') return
    const t = setInterval(() => void load(), 5000)
    return () => clearInterval(t)
  }, [conn.state, load])

  const readMore = useCallback(async () => {
    if (!cursor) return
    try {
      const res = await api.runs({ ...params, cursor }, { endpoint })
      setRuns((r) => [...r, ...res.runs])
      setCursor(res.page.next_cursor)
      setMore(res.page.has_more)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }, [cursor, params, endpoint])

  if (conn.state !== 'connected') {
    return (
      <Disconnected conn={conn} endpoint={endpoint} onEndpoint={onEndpoint} onRetry={retry} />
    )
  }

  return (
    <div className="grid min-h-[calc(100vh-11rem)] grid-rows-[auto_minmax(0,1fr)]">
      <div className="border-b border-scribe bg-panel-1">
        <div className="grid grid-cols-[auto_1fr] items-center gap-2.5 px-4 py-2.5 lg:flex lg:flex-wrap">
          <DaemonTile health={conn.health} endpoint={endpoint} />
          <ProjectPicker projects={projects} project={project} onProject={onProject} />

          <div className="well col-span-2 flex min-w-0 flex-1 items-center gap-2 rounded-[2px] px-2.5 py-2 lg:col-span-1 lg:min-w-[14rem]">
            <span className="data select-none text-[0.75rem] text-ink-3" aria-hidden="true">
              /
            </span>
            <input
              value={definition}
              onChange={(e) => setDefinition(e.target.value)}
              placeholder="definition id"
              spellCheck={false}
              aria-label="Filter by definition"
              className="data w-full min-w-0 bg-transparent text-[0.8125rem] text-ink outline-none placeholder:text-ink-3"
            />
          </div>

          <select
            value={status}
            onChange={(e) => setStatus(e.target.value)}
            aria-label="Status"
            className="field"
          >
            <option value="">any status</option>
            {STATUSES.map((s) => (
              <option key={s} value={s}>
                {s.replace('_', ' ')}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div className="grid min-h-0 grid-rows-[minmax(0,1fr)_auto] 2xl:grid-cols-[minmax(0,1fr)_minmax(0,34rem)] 2xl:grid-rows-1">
        <div className="min-h-0 overflow-y-auto">
          {error && (
            <p className="border-b border-scribe px-4 py-3 text-[0.8125rem] text-oxide">{error}</p>
          )}
          {runs.length === 0 && !loading && !error && (
            <div className="px-5 py-12">
              <p className="display text-[1.25rem]">No runs in this project.</p>
              <p className="mt-3 max-w-[52ch] text-[0.875rem] leading-relaxed text-ink-2">
                A run appears here the moment a worker registers a workflow and
                something starts one. The daemon started with{' '}
                <span className="data text-ink">demo</span> holds one already.
              </p>
            </div>
          )}
          {runs.length > 0 && (
            <ol aria-label="Runs" className="divide-y divide-scribe-2">
              {runs.map((r) => {
                const on = r.run_id === openId
                return (
                  <li key={r.run_id}>
                    <button
                      type="button"
                      onClick={() => setOpenId(on ? null : r.run_id)}
                      aria-expanded={on}
                      aria-controls={INSPECTOR_ID}
                      className={`grid w-full grid-cols-[minmax(0,1fr)_auto] items-baseline gap-x-4 gap-y-1 px-4 py-3 text-left transition-colors sm:grid-cols-[minmax(9rem,14rem)_minmax(0,1fr)_auto_auto] ${
                        on ? 'bg-panel-2' : 'hover:bg-panel-2'
                      }`}
                    >
                      <span className="data truncate text-[0.8125rem] text-ink">{r.definition_id}</span>
                      <span className="data truncate text-[0.75rem] text-ink-3">
                        {r.run_id} · v{r.definition_version} · {r.cursor.construct}
                        {r.cursor.step_key ? ` ${r.cursor.step_key}` : ''}
                      </span>
                      <span className={`data text-[0.75rem] ${STATUS_TONE[r.status] ?? 'text-ink-2'}`}>
                        {r.status.replace('_', ' ')}
                      </span>
                      <span className="data text-[0.75rem] text-ink-3">{ago(r.updated_at)}</span>
                    </button>
                  </li>
                )
              })}
            </ol>
          )}
          {more && (
            <div className="px-4 py-3">
              <button type="button" onClick={() => void readMore()} className="legend text-ink-2 hover:text-ink">
                Older runs
              </button>
            </div>
          )}
        </div>

        <RunInspector
          id={INSPECTOR_ID}
          endpoint={endpoint}
          runId={openId}
          onOpenTrace={onOpenTrace}
          onChanged={() => void load()}
        />
      </div>
    </div>
  )
}

/* ------------------------------------------------------------ inspector --- */

function RunInspector({
  id,
  endpoint,
  runId,
  onOpenTrace,
  onChanged,
}: {
  id: string
  endpoint: string
  runId: string | null
  onOpenTrace: (traceId: string) => void
  onChanged: () => void
}) {
  const [detail, setDetail] = useState<RunDetail | null>(null)
  const [events, setEvents] = useState<RunEvent[]>([])
  const [error, setError] = useState<string | null>(null)
  const [cancelling, setCancelling] = useState(false)
  const lastSeq = useRef(-1)

  const running = detail?.run.status === 'running'

  const reload = useCallback(async () => {
    if (!runId) return
    try {
      const d = await api.run(runId, { endpoint })
      setDetail(d)
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }, [runId, endpoint])

  // A new run: clear what the last one left and read from the start.
  useEffect(() => {
    setDetail(null)
    setEvents([])
    setError(null)
    lastSeq.current = -1
    if (!runId) return
    void reload()
    let cancelled = false
    ;(async () => {
      try {
        let after = -1
        for (;;) {
          const page = await api.runEvents(runId, { after, limit: 500 }, { endpoint })
          if (cancelled) return
          if (page.events.length) after = page.events[page.events.length - 1]!.seq
          setEvents((ev) => merge(ev, page.events))
          lastSeq.current = Math.max(lastSeq.current, after)
          if (!page.has_more) break
        }
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e))
      }
    })()
    return () => {
      cancelled = true
    }
  }, [runId, endpoint, reload])

  // Follow a running run. The stream is the daemon's own; when the browser
  // cannot hold it open, polling from the last sequence number seen misses
  // nothing, which is what the sequence number is for.
  useEffect(() => {
    if (!runId || !running) return
    let source: EventSource | undefined
    let poll: ReturnType<typeof setInterval> | undefined
    const url = `${endpoint.replace(/\/$/, '')}/v1/api/runs/${runId}/events?after=${lastSeq.current}`
    const onEvent = (e: RunEvent) => {
      setEvents((ev) => merge(ev, [e]))
      lastSeq.current = Math.max(lastSeq.current, e.seq)
      if (e.kind === 'run_ended') {
        onChanged()
      }
      // The run itself changes under every event: re-read its attempts and waits.
      void reload()
    }
    if (typeof EventSource !== 'undefined') {
      source = new EventSource(url)
      source.onmessage = (m) => {
        try {
          onEvent(JSON.parse(m.data) as RunEvent)
        } catch {
          /* A frame that is not an event is not ours. */
        }
      }
      source.onerror = () => {
        source?.close()
        source = undefined
        poll = setInterval(async () => {
          try {
            const page = await api.runEvents(runId, { after: lastSeq.current, limit: 500 }, { endpoint })
            page.events.forEach(onEvent)
          } catch {
            /* Reported on the next reload. */
          }
        }, 3000)
      }
    }
    return () => {
      source?.close()
      if (poll) clearInterval(poll)
    }
  }, [runId, running, endpoint, reload, onChanged])

  if (!runId) {
    return (
      <aside id={id} aria-label="Run detail" className="min-h-0 overflow-y-auto border-t border-scribe-2 p-5 2xl:border-l 2xl:border-t-0">
        <p className="max-w-[40ch] text-[0.875rem] leading-relaxed text-ink-3">
          Choose a run to read its attempts, what it is waiting for, and its journal.
        </p>
      </aside>
    )
  }

  const run = detail?.run

  const cancel = async () => {
    if (!runId) return
    setCancelling(true)
    try {
      await api.cancelRun(runId, { endpoint })
      await reload()
      onChanged()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setCancelling(false)
    }
  }

  return (
    <aside id={id} aria-label="Run detail" className="min-h-0 overflow-y-auto border-t border-scribe-2 p-5 2xl:border-l 2xl:border-t-0">
      {run && (
        <>
          <h3 className="display text-[1.0625rem]">
            {run.definition_id}
            <span className="data ml-2 text-[0.75rem] text-ink-3">v{run.definition_version}</span>
          </h3>

          <dl className="mt-3 grid grid-cols-2 gap-x-4 gap-y-1.5">
            <Fact label="Status" value={run.status.replace('_', ' ')} tone={STATUS_TONE[run.status]} />
            <Fact label="Run" value={run.run_id} />
            <Fact label="Started" value={`${clock(run.created_at)}, ${ago(run.created_at)}`} />
            <Fact label="Ended" value={run.ended_at ? `${clock(run.ended_at)}, ${ago(run.ended_at)}` : 'still running'} muted={!run.ended_at} />
            <Fact label="Steps used" value={String(run.steps_used) + (run.max_steps ? ` of ${run.max_steps}` : '')} />
            <Fact label="Cost" value={run.cost_usd > 0 ? money(run.cost_usd) : 'nothing priced'} muted={run.cost_usd === 0} />
            {run.parent_run_id && <Fact label="Parent" value={run.parent_run_id} />}
            {run.idempotency_key && <Fact label="Idempotency key" value={run.idempotency_key} />}
          </dl>

          {run.error && (
            <div className="well mt-4 rounded-[2px] p-3">
              <p className="legend text-oxide">Error</p>
              <p className="mt-1 text-[0.8125rem] leading-snug text-ink">{run.error}</p>
            </div>
          )}

          <div className="mt-4 flex flex-wrap items-baseline gap-x-5 gap-y-2">
            <button type="button" onClick={() => onOpenTrace(run.trace_id)} className="legend text-ink-2 hover:text-ink">
              Open the trace
            </button>
            {running && (
              <button type="button" onClick={() => void cancel()} disabled={cancelling} className="legend text-oxide hover:text-ink disabled:text-ink-3">
                {cancelling ? 'Cancelling' : 'Cancel the run'}
              </button>
            )}
          </div>

          <Payload label="Input" value={run.input} />
          <Payload label="Output" value={run.output} />
          <Payload label="State" value={run.state} />
        </>
      )}

      {error && <p className="mt-3 text-[0.8125rem] text-oxide">{error}</p>}

      {detail && detail.waits.length > 0 && (
        <section className="mt-5">
          <p className="legend text-ink-3">Waiting for</p>
          <ul className="mt-2 divide-y divide-scribe-2">
            {detail.waits.map((w) => (
              <li key={`${w.step_key}`} className="grid grid-cols-[minmax(0,1fr)_auto] gap-x-4 py-1.5">
                <span className="data truncate text-[0.8125rem] text-ink">
                  {w.event} <span className="text-ink-3">/</span> {w.key}
                </span>
                <span className="data text-[0.75rem] text-ink-3">until {clock(w.expires_at)}</span>
              </li>
            ))}
          </ul>
        </section>
      )}

      {detail && detail.attempts.length > 0 && (
        <section className="mt-5">
          <p className="legend text-ink-3">Attempts</p>
          <ol className="mt-2 divide-y divide-scribe-2">
            {[...detail.attempts]
              .sort((a, b) => a.created_at.localeCompare(b.created_at))
              .map((a) => (
                <Attempt key={`${a.step_key}/${a.attempt}`} attempt={a} />
              ))}
          </ol>
        </section>
      )}

      {events.length > 0 && (
        <section className="mt-5">
          <p className="legend text-ink-3">
            Journal
            {running && (
              <span className="mcpd-tail-pip ml-2 inline-block h-1.5 w-1.5 rounded-full bg-amber align-middle" aria-label="live" />
            )}
          </p>
          <ol className="mt-2 divide-y divide-scribe-2" aria-label="Journal">
            {events.map((e) => (
              <li key={e.seq} className="grid grid-cols-[2.5rem_minmax(0,1fr)_auto] items-baseline gap-x-3 py-1.5">
                <span className="data text-[0.75rem] text-ink-3">{e.seq}</span>
                <span className="data min-w-0 truncate text-[0.8125rem] text-ink">
                  {e.kind.replace(/_/g, ' ')}
                  <span className="ml-2 text-ink-3">{payloadLine(e.payload)}</span>
                </span>
                <span className="data text-[0.75rem] text-ink-3">{clock(e.at)}</span>
              </li>
            ))}
          </ol>
        </section>
      )}
    </aside>
  )
}

function Attempt({ attempt: a }: { attempt: StepAttempt }) {
  const [open, setOpen] = useState(false)
  return (
    <li>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className="grid w-full grid-cols-[minmax(0,1fr)_auto_auto] items-baseline gap-x-3 py-1.5 text-left"
      >
        <span className="data truncate text-[0.8125rem] text-ink">
          {a.step_key}
          <span className="ml-1.5 text-ink-3">#{a.attempt}</span>
          <span className="ml-2 text-[0.75rem] text-ink-3">{a.kind}</span>
        </span>
        <span className={`data text-[0.75rem] ${ATTEMPT_TONE[a.status] ?? 'text-ink-2'}`}>{a.status.replace('_', ' ')}</span>
        <span className="data text-[0.75rem] text-ink-3">{a.lease_owner ?? ''}</span>
      </button>
      {open && (
        <div className="pb-3">
          {a.error && (
            <p className="text-[0.8125rem] text-oxide">
              {a.error.class}: {a.error.message}
            </p>
          )}
          {a.usage && (a.usage.input_tokens != null || a.usage.output_tokens != null) && (
            <p className="data mt-1 text-[0.75rem] text-ink-3">
              {a.usage.input_tokens ?? 0} in, {a.usage.output_tokens ?? 0} out
              {a.cost_usd != null && a.cost_usd > 0 ? `, ${money(a.cost_usd)}` : ''}
            </p>
          )}
          <Payload label="Input" value={a.input} />
          <Payload label="Output" value={a.output} />
        </div>
      )}
    </li>
  )
}

function Fact({ label, value, muted, tone }: { label: string; value: string; muted?: boolean; tone?: string }) {
  return (
    <div className="min-w-0">
      <dt className="legend text-ink-3">{label}</dt>
      <dd className={`data truncate text-[0.8125rem] ${tone ?? (muted ? 'text-ink-3' : 'text-ink')}`} title={value}>
        {value}
      </dd>
    </div>
  )
}

function Payload({ label, value }: { label: string; value: unknown }) {
  if (value === undefined || value === null) return null
  const text = typeof value === 'string' ? value : JSON.stringify(value, null, 2)
  return (
    <div className="mt-4">
      <p className="legend text-ink-3">{label}</p>
      <pre className="data mt-1 max-h-[18rem] overflow-auto whitespace-pre-wrap break-words rounded-[2px] bg-well p-3 text-[0.75rem] leading-relaxed text-ink">
        {text}
      </pre>
    </div>
  )
}

/** Events by sequence number, deduplicated: a page and a stream may overlap. */
function merge(current: RunEvent[], incoming: RunEvent[]): RunEvent[] {
  if (incoming.length === 0) return current
  const seen = new Map<number, RunEvent>()
  for (const e of current) seen.set(e.seq, e)
  for (const e of incoming) seen.set(e.seq, e)
  return [...seen.values()].sort((a, b) => a.seq - b.seq)
}

/** One line of a payload: the fields worth a glance, never the whole object. */
function payloadLine(payload: unknown): string {
  if (!payload || typeof payload !== 'object') return ''
  const p = payload as Record<string, unknown>
  const parts: string[] = []
  for (const key of ['step_key', 'attempt', 'worker_id', 'class', 'message', 'status', 'event', 'construct', 'index']) {
    if (p[key] !== undefined && p[key] !== null) parts.push(`${key} ${String(p[key])}`)
  }
  return parts.join(' · ')
}
