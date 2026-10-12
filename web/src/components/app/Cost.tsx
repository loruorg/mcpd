/**
 * The spend screen.
 *
 * Two questions, in this order: what did this project spend over a window, and
 * where did it go. The first comes from the analytics summary, the second from
 * the cost breakdown grouped by one dimension.
 *
 * The totals and the time series are fetched as two separate requests on
 * purpose. The daemon can group by any of nine dimensions, but it can only
 * bucket three of them over time, and asking for a bucket it cannot produce
 * fails the whole request. Splitting them means a grouping that has no series
 * still shows its totals, and the screen says what the daemon said rather than
 * carrying a hard-coded list of which dimensions work.
 */

import { useCallback, useEffect, useState } from 'react'
import { api, duration, money, tokens } from '@/lib/api'
import type { AnalyticsSummary, CostBucket, GroupedStat } from '@/generated/types'
import { Disconnected } from './Explorer'
import {
  COST_GROUPS,
  COST_WINDOWS,
  DaemonTile,
  nowIso,
  ProjectPicker,
  since,
  useConnection,
} from './shell'

/** How many bands the chart draws before folding the remainder into one. */
const BANDS = 6

/* One hue, stepped by rank. Spend is model activity and slate is the signal
   colour for model activity, so the ramp stays inside the system rather than
   handing four meaningful signal colours out as decoration. */
const RAMP = [100, 86, 72, 58, 46, 36, 28]
const tone = (i: number) =>
  `color-mix(in oklab, var(--color-slate) ${RAMP[Math.min(i, RAMP.length - 1)]}%, var(--color-chart))`

export default function Cost({
  endpoint,
  onEndpoint,
  project,
  onProject,
}: {
  endpoint: string
  onEndpoint: (v: string) => void
  project: string
  onProject: (v: string) => void
}) {
  const { conn, projects, retry } = useConnection(endpoint, onEndpoint)

  const [windowId, setWindowId] = useState('7d')
  const [groupId, setGroupId] = useState('model')
  const [nonce, setNonce] = useState(0)

  const [summary, setSummary] = useState<AnalyticsSummary | null>(null)
  const [groups, setGroups] = useState<GroupedStat[] | null>(null)
  const [currency, setCurrency] = useState('USD')
  const [series, setSeries] = useState<CostBucket[] | null>(null)
  const [seriesError, setSeriesError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [readAt, setReadAt] = useState<Date | null>(null)
  // The window these readings cover, kept so the chart can draw the buckets
  // that came back against the whole window rather than against each other.
  const [range, setRange] = useState<{ from: string; to: string } | null>(null)

  const win = COST_WINDOWS.find((w) => w.id === windowId) ?? COST_WINDOWS[1]
  const dim = COST_GROUPS.find((g) => g.id === groupId) ?? COST_GROUPS[0]

  useEffect(() => {
    if (conn.state !== 'connected') return
    const ac = new AbortController()
    const o = { endpoint, signal: ac.signal }
    // One window for every request in this pass. Recomputing `now` per call
    // would let the totals and the series cover slightly different windows.
    const from = since(win.seconds)!
    const to = nowIso()
    setRange({ from, to })

    setLoading(true)
    setError(null)
    setSeriesError(null)

    Promise.all([
      api.summary({ project, from, to }, o),
      api.cost({ project, from, to, group_by: dim.id }, o),
    ])
      .then(([s, c]) => {
        setSummary(s)
        setGroups(c.groups)
        setCurrency(c.currency)
        setReadAt(new Date())
      })
      .catch((e) => {
        if ((e as Error).name === 'AbortError') return
        setError(e instanceof Error ? e.message : String(e))
      })
      .finally(() => setLoading(false))

    setSeries(null)
    api
      .cost({ project, from, to, group_by: dim.id, bucket: win.bucket }, o)
      .then((c) => setSeries(c.series ?? []))
      .catch((e) => {
        if ((e as Error).name === 'AbortError') return
        setSeriesError(e instanceof Error ? e.message : String(e))
      })

    return () => ac.abort()
  }, [conn.state, endpoint, project, win.seconds, win.bucket, dim.id, nonce])

  if (conn.state !== 'connected') {
    return (
      <Disconnected conn={conn} endpoint={endpoint} onEndpoint={onEndpoint} onRetry={retry} />
    )
  }

  return (
    <div className="min-h-[calc(100vh-11rem)]">
      <div className="border-b border-scribe bg-panel-1">
        <div className="flex flex-wrap items-center gap-2.5 px-4 py-2.5">
          <DaemonTile health={conn.health} endpoint={endpoint} />
          <ProjectPicker projects={projects} project={project} onProject={onProject} />

          <select
            value={windowId}
            onChange={(e) => setWindowId(e.target.value)}
            aria-label="Window"
            className="field"
          >
            {COST_WINDOWS.map((w) => (
              <option key={w.id} value={w.id}>
                {w.label}
              </option>
            ))}
          </select>

          {/* Every option here is a dimension the daemon groups by. It names
              the nine in the error it returns for anything else, and a tenth
              added here would be a 400 rather than a new breakdown. */}
          <select
            value={groupId}
            onChange={(e) => setGroupId(e.target.value)}
            aria-label="Break down by"
            className="field"
          >
            {COST_GROUPS.map((g) => (
              <option key={g.id} value={g.id}>
                by {g.label}
              </option>
            ))}
          </select>

          <button
            type="button"
            onClick={() => setNonce((n) => n + 1)}
            className="legend panel-raised px-3 py-2 text-ink transition-colors hover:bg-panel-3"
            title="Read the window again"
          >
            Read again
          </button>

          <span className="data ml-auto text-[0.75rem] text-ink-3">
            {loading
              ? 'reading…'
              : readAt
                ? `read at ${readAt.toLocaleTimeString(undefined, { hour12: false })}`
                : ''}
          </span>
        </div>
        <div className="ticks opacity-40" aria-hidden="true" />
      </div>

      {error ? (
        <div className="p-5">
          <p className="legend text-oxide">Query failed</p>
          <p className="mt-2 max-w-[60ch] text-[0.875rem] leading-relaxed text-ink-2">{error}</p>
        </div>
      ) : (
        <div className="mx-auto max-w-[86rem] px-4 py-6 sm:px-6">
          <Totals summary={summary} loading={loading} windowLabel={win.label} />

          <section className="mt-8">
            <h2 className="legend text-ink-3">Spend over the {win.label}</h2>
            <div className="mt-3">
              <Series
                series={series}
                error={seriesError}
                bucket={win.bucket ?? ''}
                bucketSeconds={win.bucketSeconds ?? 0}
                range={range}
                dimLabel={dim.label}
                currency={currency}
              />
            </div>
          </section>

          <section className="mt-9">
            <h2 className="legend text-ink-3">Every {dim.label}</h2>
            <div className="mt-3">
              <Breakdown
                groups={groups}
                summary={summary}
                dimLabel={dim.label}
                unit={dim.unit}
                currency={currency}
                loading={loading}
              />
            </div>
          </section>
        </div>
      )}
    </div>
  )
}

/* ------------------------------------------------------------- totals --- */

/**
 * The window as a set of readings.
 *
 * Every figure here can be missing for a reason the operator can act on, so a
 * missing one is replaced by the reason and never by a dash. An empty window
 * and a window whose steps were never priced look identical in the arithmetic
 * and are different problems.
 */
function Totals({
  summary,
  loading,
  windowLabel,
}: {
  summary: AnalyticsSummary | null
  loading: boolean
  windowLabel: string
}) {
  if (!summary) {
    return (
      <div>
        <p className="legend text-ink-3">{loading ? 'Reading' : 'No reading yet'}</p>
        {loading && <div className="probe ticks mt-3 w-full max-w-[20rem]" aria-hidden="true" />}
      </div>
    )
  }

  const used = summary.input_tokens + summary.output_tokens
  const spend =
    summary.trace_count === 0
      ? { text: `no run in the ${windowLabel}`, weak: true }
      : summary.total_cost === 0 && used > 0
        ? { text: 'tokens used, none priced', weak: true }
        : { text: money(summary.total_cost, summary.currency), weak: false }

  const latency =
    summary.latency.p50 != null
      ? { text: duration(summary.latency.p50 * 1e6), weak: false }
      : { text: 'no run finished', weak: true }
  const p95 =
    summary.latency.p95 != null
      ? { text: duration(summary.latency.p95 * 1e6), weak: false }
      : { text: 'no run finished', weak: true }

  const readings: { k: string; v: string; weak: boolean }[] = [
    { k: 'Spend', ...{ v: spend.text, weak: spend.weak } },
    { k: 'Runs', v: String(summary.trace_count), weak: false },
    { k: 'Steps', v: String(summary.span_count), weak: false },
    {
      k: 'Failed',
      v:
        summary.trace_count === 0
          ? 'no run to fail'
          : `${summary.error_count} of ${summary.trace_count}`,
      weak: summary.trace_count === 0,
    },
    { k: 'Tokens in', v: tokens(summary.input_tokens), weak: false },
    { k: 'Tokens out', v: tokens(summary.output_tokens), weak: false },
    { k: 'Median run', ...{ v: latency.text, weak: latency.weak } },
    { k: 'p95 run', ...{ v: p95.text, weak: p95.weak } },
  ]

  return (
    <dl className="grid grid-cols-2 gap-x-6 gap-y-4 sm:grid-cols-4 xl:grid-cols-8">
      {readings.map((r) => (
        <div key={r.k} className="min-w-0">
          <dt className="legend text-ink-3">{r.k}</dt>
          <dd
            className={
              r.weak
                ? 'mt-1 text-[0.8125rem] leading-snug text-ink-3'
                : 'data mt-1 text-[1.0625rem] text-ink'
            }
          >
            {r.v}
          </dd>
        </div>
      ))}
    </dl>
  )
}

/* -------------------------------------------------------------- series --- */

function Series({
  series,
  error,
  bucket,
  bucketSeconds,
  range,
  dimLabel,
  currency,
}: {
  series: CostBucket[] | null
  error: string | null
  bucket: string
  bucketSeconds: number
  range: { from: string; to: string } | null
  dimLabel: string
  currency: string
}) {
  if (error) {
    return (
      <div className="panel p-4">
        <p className="text-[0.875rem] leading-relaxed text-ink-2">
          The daemon could not bucket spend by {dimLabel} over time, so this grouping has
          totals only. The breakdown below is unaffected.
        </p>
        {/* The daemon's own words, in the recess this panel keeps for machine
            output. Kept in full: it names the column that is missing, which is
            what anyone fixing this needs. */}
        <pre className="well scroll-x data mt-3 overflow-auto rounded-[2px] p-2.5 text-[0.75rem] leading-relaxed text-ink-2">
          {error}
        </pre>
      </div>
    )
  }

  if (series === null) {
    return (
      <div className="panel p-4">
        <p className="legend text-ink-3">Reading</p>
        <div className="probe ticks mt-3 w-full max-w-[20rem]" aria-hidden="true" />
      </div>
    )
  }

  // The chart is computed from the rows it draws, including its own ranking and
  // scale, so it cannot claim a shape the data does not have.
  //
  // The axis is the whole window, laid out in buckets, and not the list of
  // buckets that came back. The daemon returns a row only where something ran,
  // so plotting those side by side would put two buckets five days apart next
  // to each other and read as continuous spend.
  const width = bucketSeconds * 1000
  const slots: number[] = []
  if (range && width > 0) {
    const first = Math.floor(new Date(range.from).getTime() / width) * width
    const last = Math.floor(new Date(range.to).getTime() / width) * width
    // A guard, not a limit that bites: the widest window here is 31 buckets.
    for (let t = first; t <= last && slots.length < 400; t += width) slots.push(t)
  }
  const stamps = slots.length
    ? slots.map((t) => new Date(t).toISOString())
    : [...new Set(series.map((s) => s.timestamp))].sort()

  const byKey = new Map<string, number>()
  for (const s of series) byKey.set(s.key, (byKey.get(s.key) ?? 0) + s.total_cost)
  const ranked = [...byKey.entries()].filter(([, v]) => v > 0).sort((a, b) => b[1] - a[1])

  if (!stamps.length || !ranked.length) {
    return (
      <div className="panel p-4">
        <p className="text-[0.875rem] leading-relaxed text-ink-2">
          {stamps.length
            ? 'Steps ran in this window and none of them carried a price, so there is no spend to plot.'
            : 'No step ran in this window, so there is nothing to plot.'}
        </p>
      </div>
    )
  }

  const named = ranked.slice(0, BANDS).map(([k]) => k)
  const restCount = ranked.length - named.length
  const bands = restCount > 0 ? [...named, `${restCount} more`] : named

  // Matched on the instant rather than the string: the daemon's timestamps and
  // the ones generated here are the same moment written two ways.
  const at = new Map<number, CostBucket[]>()
  for (const s of series) {
    const t = new Date(s.timestamp).getTime()
    const list = at.get(t)
    if (list) list.push(s)
    else at.set(t, [s])
  }

  const column = (ts: string) => {
    const cells = new Map<string, number>()
    for (const s of at.get(new Date(ts).getTime()) ?? []) {
      if (s.total_cost <= 0) continue
      const band = named.includes(s.key) ? s.key : `${restCount} more`
      cells.set(band, (cells.get(band) ?? 0) + s.total_cost)
    }
    return cells
  }

  const columns = stamps.map((ts) => ({ ts, cells: column(ts), }))
  const max = Math.max(
    ...columns.map((c) => [...c.cells.values()].reduce((a, b) => a + b, 0)),
    0,
  )
  const stamp = (ts: string) =>
    new Date(ts).toLocaleString(undefined, {
      day: 'numeric',
      month: 'short',
      hour: '2-digit',
      minute: '2-digit',
      hour12: false,
    })

  return (
    <div>
      <div className="rounded-[2px] border border-scribe border-b-lip bg-chart p-3">
        <p className="data text-[0.6875rem] text-ink-3">
          peak {money(max, currency)} per {bucket}
        </p>
        <div className="mt-2 flex h-40 items-end gap-[2px] border-b border-scribe">
          {columns.map(({ ts, cells }) => {
            const total = [...cells.values()].reduce((a, b) => a + b, 0)
            return (
              <div
                key={ts}
                className="flex h-full flex-1 flex-col justify-end"
                title={`${stamp(ts)}: ${money(total, currency)}`}
              >
                {/* Painted top band first so the largest sits at the base. */}
                {[...bands].reverse().map((b) => {
                  const v = cells.get(b) ?? 0
                  if (v <= 0) return null
                  return (
                    <div
                      key={b}
                      style={{
                        height: `${(v / max) * 100}%`,
                        background: tone(bands.indexOf(b)),
                      }}
                    />
                  )
                })}
              </div>
            )
          })}
        </div>
      </div>

      <div className="ticks mt-1 opacity-40" aria-hidden="true" />
      <div className="mt-1.5 flex flex-wrap items-baseline justify-between gap-2">
        <span className="data text-[0.6875rem] text-ink-3">{stamp(stamps[0])}</span>
        <span className="data text-[0.6875rem] text-ink-3">
          {stamps.length} {stamps.length === 1 ? 'column' : 'columns'} of {bucket}
        </span>
        <span className="data text-[0.6875rem] text-ink-3">
          {stamp(stamps[stamps.length - 1])}
        </span>
      </div>

      <ul className="mt-4 flex flex-wrap gap-x-5 gap-y-2">
        {bands.map((b, i) => (
          <li key={b} className="flex items-baseline gap-2">
            <span
              aria-hidden="true"
              className="inline-block h-2 w-2 shrink-0 translate-y-[1px] rounded-[1px]"
              style={{ background: tone(i) }}
            />
            <span className="data text-[0.75rem] text-ink-2">
              {b === '(none)' ? `no ${dimLabel} recorded` : b}
            </span>
            <span className="data text-[0.75rem] text-ink-3">
              {money(
                b.endsWith('more')
                  ? ranked.slice(BANDS).reduce((a, [, v]) => a + v, 0)
                  : (byKey.get(b) ?? 0),
                currency,
              )}
            </span>
          </li>
        ))}
      </ul>
    </div>
  )
}

/* ----------------------------------------------------------- breakdown --- */

function Breakdown({
  groups,
  summary,
  dimLabel,
  unit,
  currency,
  loading,
}: {
  groups: GroupedStat[] | null
  summary: AnalyticsSummary | null
  dimLabel: string
  unit: 'step' | 'run'
  currency: string
  loading: boolean
}) {
  if (!groups) {
    return (
      <div className="panel p-4">
        <p className="legend text-ink-3">{loading ? 'Reading' : 'No reading yet'}</p>
      </div>
    )
  }

  if (!groups.length) {
    return (
      <div className="panel p-4">
        <p className="max-w-[60ch] text-[0.875rem] leading-relaxed text-ink-2">
          {summary && summary.trace_count > 0
            ? `${summary.trace_count} run${summary.trace_count === 1 ? '' : 's'} ran in this
               window, and none of them recorded a ${dimLabel}. mcpd groups only on what a
               run actually carried.`
            : 'No run reached mcpd in this window, so there is nothing to break down.'}
        </p>
      </div>
    )
  }

  const total = groups.reduce((a, g) => a + (g.total_cost ?? 0), 0)

  return (
    <div className="scroll-x panel">
      <table className="w-full min-w-[46rem] border-collapse">
        <thead>
          <tr className="border-b border-scribe">
            <th className="legend px-4 py-2.5 text-left">{dimLabel}</th>
            <th className="legend px-4 py-2.5 text-right">spend</th>
            <th className="legend w-[9rem] px-4 py-2.5 text-left">share</th>
            <th
              className="legend px-4 py-2.5 text-right"
              title={
                unit === 'step'
                  ? 'Steps, because this grouping is resolved against the step table'
                  : 'Runs, because this grouping is resolved against the run table'
              }
            >
              {unit === 'step' ? 'steps' : 'runs'}
            </th>
            <th className="legend px-4 py-2.5 text-right">failed</th>
            <th className="legend px-4 py-2.5 text-right">in</th>
            <th className="legend px-4 py-2.5 text-right">out</th>
            <th className="legend px-4 py-2.5 text-right">mean</th>
          </tr>
        </thead>
        <tbody>
          {groups.map((g, i) => {
            const used = (g.input_tokens ?? 0) + (g.output_tokens ?? 0)
            const priced = g.total_cost != null && g.total_cost > 0
            const share = total > 0 ? ((g.total_cost ?? 0) / total) * 100 : 0
            return (
              <tr key={g.key} className="border-b border-scribe-2 last:border-b-0">
                <td
                  className="max-w-[18rem] truncate px-4 py-2.5 text-[0.875rem] text-ink"
                  title={g.key}
                >
                  {g.key === '(none)' ? (
                    <span className="text-ink-3" title={`These carried no ${dimLabel}`}>
                      no {dimLabel} recorded
                    </span>
                  ) : (
                    g.key
                  )}
                </td>
                <td className="data px-4 py-2.5 text-right text-[0.875rem] text-ink">
                  {g.total_cost != null && g.total_cost > 0 ? (
                    money(g.total_cost, currency)
                  ) : (
                    <span className="text-[0.75rem] text-ink-3">
                      {used > 0 ? 'no price for these tokens' : 'no priced step'}
                    </span>
                  )}
                </td>
                <td className="px-4 py-2.5">
                  <span
                    className="block h-[6px] rounded-[1px]"
                    style={{
                      width: `${Math.max(share, priced ? 1 : 0)}%`,
                      background: tone(Math.min(i, RAMP.length - 1)),
                    }}
                    title={`${share.toFixed(1)}% of the window`}
                  />
                </td>
                <td className="data px-4 py-2.5 text-right text-[0.8125rem] text-ink-2">
                  {g.count}
                </td>
                <td
                  className={`data px-4 py-2.5 text-right text-[0.8125rem] ${
                    g.error_count > 0 ? 'text-oxide' : 'text-ink-3'
                  }`}
                >
                  {g.error_count}
                </td>
                <td className="data px-4 py-2.5 text-right text-[0.8125rem] text-ink-2">
                  {g.input_tokens != null ? tokens(g.input_tokens) : 'not recorded'}
                </td>
                <td className="data px-4 py-2.5 text-right text-[0.8125rem] text-ink-2">
                  {g.output_tokens != null ? tokens(g.output_tokens) : 'not recorded'}
                </td>
                <td className="data px-4 py-2.5 text-right text-[0.8125rem] text-ink-2">
                  {g.latency?.mean != null ? (
                    duration(g.latency.mean * 1e6)
                  ) : (
                    <span className="text-[0.75rem] text-ink-3">none finished</span>
                  )}
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}
