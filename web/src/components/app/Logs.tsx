/**
 * The log screen.
 *
 * mcpd stores a log line with the run it came from, so the job of this screen
 * is to get you from a line to that run. The filters narrow the stream and
 * every line that carries a trace carries a way into the explorer at it.
 *
 * Two facts about the daemon shape this file, and both are easy to get wrong:
 * `level` is a MINIMUM severity rather than an exact match, and the log page
 * carries no total, only a cursor and whether more exist. Neither is presented
 * as anything else here.
 */

import { useCallback, useEffect, useMemo, useState } from 'react'
import { api, ago } from '@/lib/api'
import type { LogRecord } from '@/generated/types'
import { Disconnected, useDebounced } from './Explorer'
import {
  clock,
  day,
  DaemonTile,
  LEVEL_TONE,
  LOG_WINDOWS,
  nowIso,
  ProjectPicker,
  since,
  useConnection,
  windowPhrase,
} from './shell'

const PAGE = 100

/** The inspector's id, so a line row can name the panel it drives. */
const INSPECTOR_ID = 'mcpd-line-inspector'

/**
 * The query this screen sends.
 *
 * `api.logs` types only `project`, `q`, `level`, `trace_id` and `limit`, but
 * the daemon's route also reads `from`, `to`, `span_id`, `order` and `cursor`,
 * and the page it returns carries a `next_cursor` that the narrower type gives
 * no way to spend. The client serialises whatever it is handed, so the window
 * and the cursor do reach the daemon; declaring them here keeps this file
 * type-checked while that signature is widened where it belongs.
 */
interface LogQuery {
  project: string
  q?: string
  level?: string
  trace_id?: string
  from?: string
  limit?: number
  cursor?: string
}

/** Whether this project holds any log line at all, asked only when one is needed. */
type Presence = 'unknown' | 'none' | 'some'

export default function Logs({
  endpoint,
  onEndpoint,
  project,
  onProject,
  onOpenTrace,
}: {
  endpoint: string
  onEndpoint: (v: string) => void
  project: string
  onProject: (v: string) => void
  onOpenTrace: (traceId: string) => void
}) {
  const { conn, projects, retry } = useConnection(endpoint, onEndpoint)

  const [query, setQuery] = useState('')
  const [level, setLevel] = useState('')
  const [windowId, setWindowId] = useState('24h')
  const [traceFilter, setTraceFilter] = useState<string | null>(null)
  const [live, setLive] = useState(true)

  const [head, setHead] = useState<LogRecord[]>([])
  const [tail, setTail] = useState<LogRecord[]>([])
  const [cursor, setCursor] = useState<string | undefined>(undefined)
  const [more, setMore] = useState(false)
  const [loading, setLoading] = useState(false)
  const [olderLoading, setOlderLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [presence, setPresence] = useState<Presence>('unknown')
  const [openId, setOpenId] = useState<string | null>(null)

  const debouncedQuery = useDebounced(query, 250)
  const win = LOG_WINDOWS.find((w) => w.id === windowId) ?? LOG_WINDOWS[1]
  const filtered = Boolean(debouncedQuery || level || traceFilter || win.seconds)

  // The filters as one object, so the loader and the effect that resets paging
  // agree on what a change is.
  const params: LogQuery = useMemo(
    () => ({
      project,
      q: debouncedQuery || undefined,
      level: level || undefined,
      trace_id: traceFilter || undefined,
      from: since(win.seconds),
      limit: PAGE,
    }),
    [project, debouncedQuery, level, traceFilter, win.seconds],
  )

  const load = useCallback(
    async (signal?: AbortSignal) => {
      if (conn.state !== 'connected') return
      setLoading(true)
      setError(null)
      try {
        const res = await api.logs(params, { endpoint, signal })
        setHead(res.logs)
        setTail([])
        setCursor(res.page.next_cursor)
        setMore(res.page.has_more)
        // An empty result has two very different causes, and an operator acts
        // differently on each. Ask the daemon which one it is rather than
        // printing a sentence that covers both.
        if (res.logs.length === 0) {
          if (!filtered) setPresence('none')
          else {
            const any = await api.logs({ project, limit: 1 }, { endpoint, signal })
            setPresence(any.logs.length ? 'some' : 'none')
          }
        } else setPresence('some')
      } catch (e) {
        if ((e as Error).name === 'AbortError') return
        setError(e instanceof Error ? e.message : String(e))
      } finally {
        setLoading(false)
      }
    },
    [conn.state, params, endpoint, filtered, project],
  )

  useEffect(() => {
    const ac = new AbortController()
    void load(ac.signal)
    return () => ac.abort()
  }, [load])

  // Live tail, polling like the explorer does. A socket is the first thing to
  // break across a port-forward, which is where this interface usually runs.
  useEffect(() => {
    if (!live || conn.state !== 'connected') return
    const t = setInterval(() => void load(), 4000)
    return () => clearInterval(t)
  }, [live, conn.state, load])

  const readOlder = useCallback(async () => {
    if (!cursor) return
    // Paging and tailing cannot both be true: the tail replaces the first page
    // on every poll, which would tear the older pages off the bottom.
    setLive(false)
    setOlderLoading(true)
    try {
      const older: LogQuery = { ...params, cursor }
      const res = await api.logs(older, { endpoint })
      setTail((t) => [...t, ...res.logs])
      setCursor(res.page.next_cursor)
      setMore(res.page.has_more)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setOlderLoading(false)
    }
  }, [cursor, params, endpoint])

  const rows = useMemo(() => [...head, ...tail], [head, tail])
  const open = openId ? rows.find((l) => l.log_id === openId) : undefined
  const terms = useMemo(
    () => debouncedQuery.split(/\s+/).filter(Boolean),
    [debouncedQuery],
  )

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
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="search messages and fields"
              spellCheck={false}
              aria-label="Search log lines"
              className="data w-full min-w-0 bg-transparent text-[0.8125rem] text-ink outline-none placeholder:text-ink-3"
            />
          </div>

          {/* Minimum severity, not an exact level: the daemon filters on
              `severity >= level`. The labels say so, because a control reading
              "warn" that also returns errors teaches the operator something
              untrue. `trace` is the lowest level, so offering it would be a
              second control that does what "any level" already does. */}
          <select
            value={level}
            onChange={(e) => setLevel(e.target.value)}
            aria-label="Minimum level"
            className="field"
          >
            <option value="">any level</option>
            <option value="debug">debug and above</option>
            <option value="info">info and above</option>
            <option value="warn">warn and above</option>
            <option value="error">error and above</option>
            <option value="fatal">fatal only</option>
          </select>

          <select
            value={windowId}
            onChange={(e) => setWindowId(e.target.value)}
            aria-label="Window"
            className="field"
          >
            {LOG_WINDOWS.map((w) => (
              <option key={w.id} value={w.id}>
                {w.label}
              </option>
            ))}
          </select>

          <button
            type="button"
            onClick={() => setLive(!live)}
            aria-pressed={live}
            className="annunciator py-1.5"
            data-lit={live ? 'amber' : ''}
            title={live ? 'Polling every four seconds' : 'Not refreshing'}
          >
            <span
              className={
                live
                  ? 'mcpd-tail-pip inline-block h-1.5 w-1.5 rounded-full bg-current'
                  : 'inline-block h-1.5 w-1.5 rounded-full bg-current opacity-40'
              }
            />
            tail
          </button>

          <span className="data text-[0.75rem] text-ink-3 lg:ml-auto">
            {loading && !rows.length
              ? 'reading…'
              : `${rows.length} line${rows.length === 1 ? '' : 's'}${more ? ', more held' : ''}`}
          </span>
        </div>

        {traceFilter && (
          <p className="border-t border-scribe-2 px-4 py-2 text-[0.8125rem] text-ink-2">
            Showing only the lines from run{' '}
            <span className="data text-ink">{traceFilter.slice(0, 12)}</span>.{' '}
            <button
              type="button"
              onClick={() => setTraceFilter(null)}
              className="link text-[0.8125rem]"
            >
              Show every run
            </button>
          </p>
        )}
        <div className="ticks opacity-40" aria-hidden="true" />
      </div>

      <div className="grid min-h-0 xl:grid-cols-[minmax(0,1fr)_minmax(0,26rem)]">
        <Stream
          rows={rows}
          terms={terms}
          loading={loading}
          error={error}
          openId={openId}
          onOpen={setOpenId}
          onOpenTrace={onOpenTrace}
          filtered={filtered}
          presence={presence}
          project={project}
          query={debouncedQuery}
          level={level}
          win={win}
          traceFilter={traceFilter}
          more={more}
          olderLoading={olderLoading}
          onOlder={readOlder}
        />
        <LineInspector
          line={open}
          onOpenTrace={onOpenTrace}
          onPinTrace={(id) => {
            setTraceFilter(id)
            setOpenId(null)
          }}
        />
      </div>
    </div>
  )
}

/* ------------------------------------------------------------- stream --- */

function Stream(p: {
  rows: LogRecord[]
  terms: string[]
  loading: boolean
  error: string | null
  openId: string | null
  onOpen: (id: string | null) => void
  onOpenTrace: (id: string) => void
  filtered: boolean
  presence: Presence
  project: string
  query: string
  level: string
  win: (typeof LOG_WINDOWS)[number]
  traceFilter: string | null
  more: boolean
  olderLoading: boolean
  onOlder: () => void
}) {
  if (p.error) {
    return (
      <div className="border-r border-scribe-2 p-5">
        <p className="legend text-oxide">Query failed</p>
        <p className="mt-2 max-w-[52ch] text-[0.875rem] leading-relaxed text-ink-2">{p.error}</p>
      </div>
    )
  }

  if (!p.rows.length) {
    return (
      <div className="border-r border-scribe-2 p-5">
        <p className="legend text-ink-3">{p.loading ? 'Reading' : 'Nothing to show'}</p>
        {p.loading ? (
          <>
            <p className="mt-2 text-[0.875rem] text-ink-2">Asking the daemon.</p>
            <div className="probe ticks mt-4 w-full max-w-[20rem]" aria-hidden="true" />
          </>
        ) : (
          <EmptyReason
            filtered={p.filtered}
            presence={p.presence}
            project={p.project}
            query={p.query}
            level={p.level}
            win={p.win}
            traceFilter={p.traceFilter}
          />
        )}
      </div>
    )
  }

  let last = ''
  return (
    <div className="min-h-0 overflow-y-auto border-r border-scribe-2">
      <ol>
        {p.rows.map((l) => {
          const d = day(l.timestamp)
          const newDay = d !== last
          last = d
          return (
            <LineRow
              key={l.log_id}
              line={l}
              dayLabel={newDay ? d : null}
              terms={p.terms}
              open={p.openId === l.log_id}
              onOpen={p.onOpen}
              onOpenTrace={p.onOpenTrace}
            />
          )
        })}
      </ol>

      <div className="border-t border-scribe-2 px-4 py-3">
        {p.more ? (
          <button
            type="button"
            onClick={p.onOlder}
            disabled={p.olderLoading}
            className="legend panel-raised px-3 py-2 text-ink transition-colors hover:bg-panel-3 disabled:text-ink-3"
            title="Reading older lines stops the tail, so the pages below cannot be pulled away mid-read"
          >
            {p.olderLoading ? 'Reading' : 'Read older'}
          </button>
        ) : (
          <p className="text-[0.8125rem] text-ink-3">
            That is every line the daemon holds for these filters.
          </p>
        )}
      </div>
    </div>
  )
}

/**
 * Why the stream is empty.
 *
 * "No line in this window" and "this daemon has never been sent a log" are
 * different facts and lead to different next steps, so the screen asks which
 * one is true instead of printing a sentence that covers both.
 */
function EmptyReason(p: {
  filtered: boolean
  presence: Presence
  project: string
  query: string
  level: string
  win: (typeof LOG_WINDOWS)[number]
  traceFilter: string | null
}) {
  if (!p.filtered || p.presence === 'none') {
    return (
      <>
        <p className="mt-2 max-w-[46ch] text-[0.875rem] leading-relaxed text-ink-2">
          mcpd holds no log line at all for project{' '}
          <span className="data text-ink">{p.project}</span>. An SDK that logs through
          mcpd puts its lines here, correlated with the run that wrote them.
        </p>
        {p.filtered && (
          <p className="mt-3 max-w-[46ch] text-[0.875rem] leading-relaxed text-ink-3">
            The filters are not what is hiding them. Nothing has been sent.
          </p>
        )}
      </>
    )
  }

  const parts: string[] = []
  if (p.query) parts.push(`matching "${p.query}"`)
  if (p.level) parts.push(p.level === 'fatal' ? 'at fatal' : `at ${p.level} and above`)
  if (p.traceFilter) parts.push(`in run ${p.traceFilter.slice(0, 12)}`)
  parts.push(windowPhrase(p.win))

  return (
    <>
      <p className="mt-2 max-w-[46ch] text-[0.875rem] leading-relaxed text-ink-2">
        No log line {parts.join(' ')}.
      </p>
      <p className="mt-3 max-w-[46ch] text-[0.875rem] leading-relaxed text-ink-3">
        This project does hold lines outside those filters. Widen the window, lower the
        level, or clear the search. Search matches whole words, so a partial word finds
        nothing.
      </p>
    </>
  )
}

/* ---------------------------------------------------------------- row --- */

function LineRow({
  line,
  dayLabel,
  terms,
  open,
  onOpen,
  onOpenTrace,
}: {
  line: LogRecord
  dayLabel: string | null
  terms: string[]
  open: boolean
  onOpen: (id: string | null) => void
  onOpenTrace: (id: string) => void
}) {
  return (
    <>
      {dayLabel && (
        <li className="seam mt-1 bg-panel-1 px-4 py-1">
          <span className="data text-[0.6875rem] text-ink-3">{dayLabel}</span>
        </li>
      )}
      <li
        className={`relative border-b border-scribe-2 px-4 py-1.5 transition-colors ${
          open ? 'bg-panel-3' : 'hover:bg-panel-2'
        }`}
      >
        <div className="grid grid-cols-[auto_auto_minmax(0,1fr)] items-baseline gap-x-3 gap-y-1 sm:grid-cols-[4.75rem_3.25rem_minmax(0,1fr)_auto]">
          <span className="data text-[0.6875rem] text-ink-3" title={line.timestamp}>
            {clock(line.timestamp)}
          </span>
          <span className={`data text-[0.625rem] ${LEVEL_TONE[line.level] ?? 'text-ink-3'}`}>
            {line.level}
          </span>
          {/* The hit area is stretched over the whole row by the pseudo element,
              so the row behaves the way a row looks. The trace control below is
              positioned, and later in the DOM, so it stays clickable above it. */}
          <button
            type="button"
            onClick={() => onOpen(open ? null : line.log_id)}
            aria-current={open}
            aria-controls={INSPECTOR_ID}
            className="data truncate text-left text-[0.8125rem] text-ink after:absolute after:inset-0 after:content-['']"
          >
            <Highlight text={line.message} terms={terms} />
          </button>
          <div className="relative z-10 col-start-3 sm:col-start-4 sm:row-start-1">
            {line.trace_id ? (
              <button
                type="button"
                onClick={() => onOpenTrace(line.trace_id!)}
                className="link data text-[0.6875rem]"
                title={`Open run ${line.trace_id} in the explorer`}
              >
                {line.trace_id.slice(0, 8)}
              </button>
            ) : (
              <span className="data text-[0.6875rem] text-ink-3" title="Logged outside any run">
                no run
              </span>
            )}
          </div>
        </div>
      </li>
    </>
  )
}

/** Search hits, marked in the line they were found in. */
function Highlight({ text, terms }: { text: string; terms: string[] }) {
  if (!terms.length) return <>{text}</>
  const escaped = terms.map((t) => t.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
  const parts = text.split(new RegExp(`(${escaped.join('|')})`, 'ig'))
  const lower = terms.map((t) => t.toLowerCase())
  return (
    <>
      {parts.map((part, i) =>
        lower.includes(part.toLowerCase()) ? (
          <mark key={i} className="log-match">
            {part}
          </mark>
        ) : (
          <span key={i}>{part}</span>
        ),
      )}
    </>
  )
}

/* ------------------------------------------------------------ details --- */

function LineInspector({
  line,
  onOpenTrace,
  onPinTrace,
}: {
  line?: LogRecord
  onOpenTrace: (id: string) => void
  onPinTrace: (id: string) => void
}) {
  if (!line) {
    return (
      <aside
        id={INSPECTOR_ID}
        aria-label="Line detail"
        className="hidden border-t border-scribe-2 p-5 xl:block xl:border-l xl:border-t-0"
      >
        <p className="legend text-ink-3">Line detail</p>
        <p className="mt-3 max-w-[38ch] text-[0.875rem] leading-relaxed text-ink-3">
          Choose a line to read its structured fields and the correlation it was
          recorded with.
        </p>
      </aside>
    )
  }

  const c = line.correlation
  const pairs: [string, string][] = [
    ['Level', line.level],
    ['Logged', new Date(line.timestamp).toLocaleString()],
    ['Received', ago(line.received_at)],
  ]
  if (line.logger) pairs.push(['Logger', line.logger])
  if (c.environment) pairs.push(['Environment', c.environment])
  if (c.service_name) pairs.push(['Service', c.service_name])
  if (c.service_version) pairs.push(['Version', c.service_version])
  if (c.session_id) pairs.push(['Session', c.session_id])
  if (c.user_id) pairs.push(['User', c.user_id])
  if (line.span_id) pairs.push(['Step', line.span_id])

  return (
    <aside
      id={INSPECTOR_ID}
      aria-label="Line detail"
      className="min-h-0 overflow-y-auto border-t border-scribe-2 p-5 xl:border-l xl:border-t-0"
    >
      <p className={`legend ${LEVEL_TONE[line.level] ?? 'text-ink-3'}`}>{line.level}</p>
      <p className="mt-2 whitespace-pre-wrap text-[0.9375rem] leading-relaxed text-ink">
        {line.message}
      </p>

      <dl className="mt-4 grid grid-cols-2 gap-x-4 gap-y-2">
        {pairs.map(([k, v]) => (
          <div key={k} className="min-w-0">
            <dt className="legend text-ink-3">{k}</dt>
            <dd className="data truncate text-[0.8125rem] text-ink" title={v}>
              {v}
            </dd>
          </div>
        ))}
      </dl>

      {line.trace_id ? (
        <div className="mt-5 border-t border-scribe-2 pt-4">
          <p className="legend text-ink-3">Run</p>
          <p className="data mt-1 break-all text-[0.75rem] text-ink-2">{line.trace_id}</p>
          <div className="mt-3 flex flex-wrap gap-2">
            <button
              type="button"
              onClick={() => onOpenTrace(line.trace_id!)}
              className="legend panel-raised px-3 py-2 text-ink transition-colors hover:bg-panel-3"
            >
              Open the run
            </button>
            <button
              type="button"
              onClick={() => onPinTrace(line.trace_id!)}
              className="link self-center text-[0.8125rem]"
            >
              Show only this run's lines
            </button>
          </div>
        </div>
      ) : (
        <p className="mt-5 border-t border-scribe-2 pt-4 text-[0.8125rem] leading-relaxed text-ink-3">
          This line was written outside any run, so there is no trace to open.
        </p>
      )}

      {line.fields != null && (
        <div className="mt-5">
          <p className="legend text-ink-3">Fields</p>
          <pre className="well scroll-x data mt-2 max-h-64 overflow-auto rounded-[2px] p-2.5 text-[0.75rem] leading-relaxed text-ink">
            {JSON.stringify(line.fields, null, 2)}
          </pre>
        </div>
      )}
    </aside>
  )
}
