/**
 * The metrics screen.
 *
 * One question: how is this metric distributed over the window. For a
 * histogram the daemon adds up the buckets of every point and reads quantiles
 * from the merged counts; this screen draws those buckets and prints the
 * readings beside them. Below, the stored points themselves, so a reader can
 * check what a producer actually sent.
 */

import { useEffect, useState } from 'react'
import type { HistogramSeries, MetricHistogram, MetricPoint } from '@/generated/types'
import { Disconnected } from './Explorer'
import { LOG_WINDOWS, nowIso, since, useConnection } from './shell'
import { Empty, Failure, fixed, message, more, Section, Toolbar, when } from './screen-kit'
import type { ScreenProps } from './screen-kit'

/* A metrics window always has a start: an unbounded histogram read would ask
   the daemon to merge everything it holds. */
const WINDOWS = LOG_WINDOWS.filter((w) => w.seconds > 0)

export default function Metrics({ endpoint, onEndpoint, project, onProject, onOpenTrace }: ScreenProps) {
  const { conn, projects, retry } = useConnection(endpoint, onEndpoint)
  const [names, setNames] = useState<string[]>([])
  const [labelKeys, setLabelKeys] = useState<string[]>([])
  const [name, setName] = useState('')
  const [groupBy, setGroupBy] = useState('')
  const [windowId, setWindowId] = useState('24h')
  const [hist, setHist] = useState<MetricHistogram | null>(null)
  const [points, setPoints] = useState<MetricPoint[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  const win = WINDOWS.find((w) => w.id === windowId) ?? WINDOWS[1]

  useEffect(() => {
    if (conn.state !== 'connected') return
    const o = { endpoint }
    more
      .metricNames(project, o)
      .then((r) => {
        const v = r.values.map((x) => x.value)
        setNames(v)
        setName((n) => (n && v.includes(n) ? n : (v[0] ?? '')))
      })
      .catch(() => setNames([]))
    more
      .metricLabelKeys(project, o)
      .then((r) => setLabelKeys(r.values.map((x) => x.value)))
      .catch(() => setLabelKeys([]))
  }, [conn.state, endpoint, project])

  useEffect(() => {
    if (conn.state !== 'connected' || !name) return
    const ac = new AbortController()
    const o = { endpoint, signal: ac.signal }
    const from = since(win.seconds)!
    setLoading(true)
    setError(null)
    Promise.all([
      more.histogram({ project, name, from, to: nowIso(), group_by: groupBy || undefined }, o),
      more.points({ project, name, from, limit: 50 }, o),
    ])
      .then(([h, p]) => {
        setHist(h)
        setPoints(p.points)
      })
      .catch((e) => {
        if ((e as Error).name !== 'AbortError') setError(message(e))
      })
      .finally(() => setLoading(false))
    return () => ac.abort()
  }, [conn.state, endpoint, project, name, groupBy, win.seconds])

  if (conn.state !== 'connected') {
    return <Disconnected conn={conn} endpoint={endpoint} onEndpoint={onEndpoint} onRetry={retry} />
  }

  return (
    <div className="min-h-[calc(100vh-11rem)]">
      <Toolbar
        health={conn.health}
        endpoint={endpoint}
        projects={projects}
        project={project}
        onProject={onProject}
        status={loading ? 'reading…' : hist ? `${hist.points_read} points` : ''}
      >
        <select value={name} onChange={(e) => setName(e.target.value)} aria-label="Metric" className="field">
          {names.length === 0 && <option value="">no metric recorded</option>}
          {names.map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
        </select>
        <select value={windowId} onChange={(e) => setWindowId(e.target.value)} aria-label="Window" className="field">
          {WINDOWS.map((w) => (
            <option key={w.id} value={w.id}>
              {w.label}
            </option>
          ))}
        </select>
        <select value={groupBy} onChange={(e) => setGroupBy(e.target.value)} aria-label="Split by" className="field">
          <option value="">one distribution</option>
          {labelKeys.map((k) => (
            <option key={k} value={k}>
              by {k}
            </option>
          ))}
        </select>
      </Toolbar>

      <div className="mx-auto max-w-[86rem] px-4 py-6 sm:px-6">
        {error ? (
          <Failure title="Query failed" error={error} />
        ) : !name ? (
          <Empty>
            This project has recorded no metric yet. Send OTLP metrics to <code>/v1/metrics</code>, or
            let the daemon derive duration and token metrics from the spans it stores.
          </Empty>
        ) : (
          <>
            <Section title={`Distribution over the ${win.label}`}>
              {hist && hist.series.length === 0 && <Empty>No point of {name} in the {win.label}.</Empty>}
              {hist?.truncated && (
                <p className="mb-3 text-[0.8125rem] text-ink-3">
                  The window held more points than one read merges, so the newest were used. Narrow the
                  window or split by a label.
                </p>
              )}
              <div className="grid gap-4 lg:grid-cols-2">
                {hist?.series.map((s, i) => (
                  <Distribution key={i} series={s} unit={hist.unit} />
                ))}
              </div>
            </Section>

            <Section title="Stored points, newest first">
              {points && points.length === 0 ? (
                <Empty>No point in the window.</Empty>
              ) : (
                <div className="panel overflow-x-auto">
                  <table className="w-full text-left text-[0.8125rem]">
                    <thead className="legend text-ink-3">
                      <tr>
                        <th className="px-3 py-2">Time</th>
                        <th className="px-3 py-2">Kind</th>
                        <th className="px-3 py-2">Value</th>
                        <th className="px-3 py-2">Labels</th>
                        <th className="px-3 py-2">Run</th>
                      </tr>
                    </thead>
                    <tbody>
                      {points?.map((p) => (
                        <tr key={p.metric_id} className="border-t border-scribe">
                          <td className="data px-3 py-1.5 whitespace-nowrap">{when(p.timestamp)}</td>
                          <td className="px-3 py-1.5">{p.kind}</td>
                          <td className="data px-3 py-1.5">
                            {fixed(p.value, 3)} {p.unit ?? ''}
                          </td>
                          <td className="data max-w-[28rem] truncate px-3 py-1.5 text-ink-2">
                            {p.labels ? JSON.stringify(p.labels) : ''}
                          </td>
                          <td className="px-3 py-1.5">
                            {p.trace_id && (
                              <button type="button" className="link" onClick={() => onOpenTrace(p.trace_id!)}>
                                open
                              </button>
                            )}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </Section>
          </>
        )}
      </div>
    </div>
  )
}

/** One merged distribution: its buckets drawn as bars, its quantiles as readings. */
function Distribution({ series, unit }: { series: HistogramSeries; unit?: string }) {
  const max = Math.max(1, ...series.counts)
  const label = (i: number) => {
    const lo = i === 0 ? '' : series.bounds[i - 1]
    const hi = i < series.bounds.length ? series.bounds[i] : ''
    if (i === 0) return `≤ ${hi}`
    if (i === series.bounds.length) return `> ${lo}`
    return `${lo} to ${hi}`
  }
  const readings: [string, number | null | undefined][] = [
    ['p50', series.p50],
    ['p90', series.p90],
    ['p95', series.p95],
    ['p99', series.p99],
  ]
  return (
    <div className="panel p-4">
      <div className="flex flex-wrap items-baseline gap-3">
        <span className="data text-[0.8125rem] text-ink">
          {series.labels ? JSON.stringify(series.labels) : 'all points'}
        </span>
        <span className="data ml-auto text-[0.75rem] text-ink-3">
          {series.count} observations{series.exact ? ', exact quantiles' : ', interpolated'}
          {series.points_skipped > 0 ? `, ${series.points_skipped} points with other bounds left out` : ''}
        </span>
      </div>
      <dl className="mt-3 grid grid-cols-4 gap-3">
        {readings.map(([k, v]) => (
          <div key={k}>
            <dt className="legend text-ink-3">{k}</dt>
            <dd className="data mt-1 text-[1rem] text-ink">
              {v == null ? '' : `${fixed(v, v < 10 ? 3 : 1)}${unit ? ` ${unit}` : ''}`}
            </dd>
          </div>
        ))}
      </dl>
      <div className="mt-4 space-y-1" role="list" aria-label="Buckets">
        {series.counts.map((c, i) =>
          c === 0 && (i === 0 || i === series.counts.length - 1) ? null : (
            <div key={i} role="listitem" className="grid grid-cols-[8rem_1fr_3.5rem] items-center gap-2">
              <span className="data truncate text-right text-[0.6875rem] text-ink-3">{label(i)}</span>
              <span className="h-3 bg-chart">
                <span
                  className="block h-3"
                  style={{
                    width: `${(c / max) * 100}%`,
                    background: 'color-mix(in oklab, var(--color-slate) 80%, var(--color-chart))',
                  }}
                />
              </span>
              <span className="data text-[0.6875rem] text-ink-2">{c}</span>
            </div>
          ),
        )}
      </div>
    </div>
  )
}
