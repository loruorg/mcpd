/**
 * The evaluations screen.
 *
 * What this daemon can score with, what it has scored, and a way to score the
 * newest runs now. The catalogue comes from `GET /v1/api/scorers`, so a scorer
 * appears here the moment the daemon offers it, and a judged one is marked
 * unavailable until a judge is configured rather than hidden.
 */

import { useEffect, useMemo, useState } from 'react'
import type { Score, ScoreRunResult, ScorerInfo } from '@/generated/types'
import { Disconnected } from './Explorer'
import { useConnection } from './shell'
import { Empty, Failure, fixed, message, more, Section, Toolbar, when } from './screen-kit'
import type { ScreenProps } from './screen-kit'

export default function Evaluations({ endpoint, onEndpoint, project, onProject, onOpenTrace }: ScreenProps) {
  const { conn, projects, retry } = useConnection(endpoint, onEndpoint)
  const [scorers, setScorers] = useState<ScorerInfo[] | null>(null)
  const [scores, setScores] = useState<Score[] | null>(null)
  const [scorerFilter, setScorerFilter] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [nonce, setNonce] = useState(0)

  useEffect(() => {
    if (conn.state !== 'connected') return
    const ac = new AbortController()
    const o = { endpoint, signal: ac.signal }
    setError(null)
    Promise.all([
      more.scorers(o),
      more.scores({ project, scorer_id: scorerFilter || undefined, limit: 100 }, o),
    ])
      .then(([c, s]) => {
        setScorers(c)
        setScores(s.scores)
      })
      .catch((e) => {
        if ((e as Error).name !== 'AbortError') setError(message(e))
      })
    return () => ac.abort()
  }, [conn.state, endpoint, project, scorerFilter, nonce])

  /* The mean per scorer over what was read, for a first look before opening
     the list. The count goes beside it: a mean of three is not a finding. */
  const summary = useMemo(() => {
    const by = new Map<string, { sum: number; n: number }>()
    for (const s of scores ?? []) {
      const e = by.get(s.scorer_id) ?? { sum: 0, n: 0 }
      e.sum += s.value
      e.n += 1
      by.set(s.scorer_id, e)
    }
    return [...by.entries()].sort((a, b) => a[0].localeCompare(b[0]))
  }, [scores])

  if (conn.state !== 'connected') {
    return <Disconnected conn={conn} endpoint={endpoint} onEndpoint={onEndpoint} onRetry={retry} />
  }

  const direction = (id: string) => scorers?.find((s) => s.id === id)?.direction

  return (
    <div className="min-h-[calc(100vh-11rem)]">
      <Toolbar
        health={conn.health}
        endpoint={endpoint}
        projects={projects}
        project={project}
        onProject={onProject}
        status={scores ? `${scores.length} scores read` : ''}
      >
        <select
          value={scorerFilter}
          onChange={(e) => setScorerFilter(e.target.value)}
          aria-label="Scorer"
          className="field"
        >
          <option value="">every scorer</option>
          {scorers?.map((s) => (
            <option key={s.id} value={s.id}>
              {s.id}
            </option>
          ))}
        </select>
      </Toolbar>

      <div className="mx-auto max-w-[86rem] px-4 py-6 sm:px-6">
        {error && <Failure title="Query failed" error={error} />}

        <Section title="Score the newest runs">
          {scorers && (
            <ScoreNow
              endpoint={endpoint}
              project={project}
              scorers={scorers}
              onDone={() => setNonce((n) => n + 1)}
              onOpenTrace={onOpenTrace}
            />
          )}
        </Section>

        <Section title="Means over the scores read">
          {summary.length === 0 ? (
            <Empty>No score recorded in this project yet.</Empty>
          ) : (
            <dl className="grid grid-cols-2 gap-x-6 gap-y-4 sm:grid-cols-4 xl:grid-cols-6">
              {summary.map(([id, e]) => (
                <div key={id} className="min-w-0">
                  <dt className="legend truncate text-ink-3" title={id}>
                    {id}
                  </dt>
                  <dd className="data mt-1 text-[1.0625rem] text-ink">
                    {fixed(e.sum / e.n, 3)}
                    <span className="ml-2 text-[0.75rem] text-ink-3">
                      n={e.n}
                      {direction(id) === 'lower_is_better' ? ', lower is better' : ''}
                    </span>
                  </dd>
                </div>
              ))}
            </dl>
          )}
        </Section>

        <Section title="Recent scores">
          {scores && scores.length > 0 && (
            <div className="panel overflow-x-auto">
              <table className="w-full text-left text-[0.8125rem]">
                <thead className="legend text-ink-3">
                  <tr>
                    <th className="px-3 py-2">When</th>
                    <th className="px-3 py-2">Scorer</th>
                    <th className="px-3 py-2">Value</th>
                    <th className="px-3 py-2">Source</th>
                    <th className="px-3 py-2">Reason</th>
                    <th className="px-3 py-2">Run</th>
                  </tr>
                </thead>
                <tbody>
                  {scores.map((s) => (
                    <tr key={s.score_id} className="border-t border-scribe align-top">
                      <td className="data px-3 py-1.5 whitespace-nowrap">{when(s.timestamp)}</td>
                      <td className="data px-3 py-1.5">{s.scorer_id}</td>
                      <td className="data px-3 py-1.5">
                        {fixed(s.value, 3)}
                        <span className="text-ink-3">
                          {' '}
                          of {fixed(s.min ?? 0, 0)} to {fixed(s.max ?? 1, 0)}
                        </span>
                      </td>
                      <td className="px-3 py-1.5">{s.source}</td>
                      <td className="max-w-[36rem] px-3 py-1.5 text-ink-2">{s.reason ?? ''}</td>
                      <td className="px-3 py-1.5">
                        {s.trace_id && (
                          <button type="button" className="link" onClick={() => onOpenTrace(s.trace_id!)}>
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

        <Section title="Scorers this daemon offers">
          {scorers && (
            <div className="panel overflow-x-auto">
              <table className="w-full text-left text-[0.8125rem]">
                <thead className="legend text-ink-3">
                  <tr>
                    <th className="px-3 py-2">Scorer</th>
                    <th className="px-3 py-2">Kind</th>
                    <th className="px-3 py-2">Better</th>
                    <th className="px-3 py-2">Reads</th>
                    <th className="px-3 py-2">Measures</th>
                  </tr>
                </thead>
                <tbody>
                  {scorers.map((s) => (
                    <tr key={s.id} className={`border-t border-scribe ${s.available ? '' : 'text-ink-3'}`}>
                      <td className="data px-3 py-1.5">{s.id}</td>
                      <td className="px-3 py-1.5">
                        {s.kind}
                        {!s.available && ', needs a judge'}
                      </td>
                      <td className="px-3 py-1.5">{s.direction === 'lower_is_better' ? 'lower' : 'higher'}</td>
                      <td className="data px-3 py-1.5">{s.config_keys.join(', ')}</td>
                      <td className="px-3 py-1.5 text-ink-2">{s.description}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Section>
      </div>
    </div>
  )
}

/** A small form for `POST /v1/api/score`: which scorers, how many runs, store or not. */
function ScoreNow({
  endpoint,
  project,
  scorers,
  onDone,
  onOpenTrace,
}: {
  endpoint: string
  project: string
  scorers: ScorerInfo[]
  onDone: () => void
  onOpenTrace: (id: string) => void
}) {
  const runnable = scorers.filter((s) => s.available && s.config_keys.length === 0)
  const [chosen, setChosen] = useState<string[]>([])
  const [limit, setLimit] = useState(20)
  const [dryRun, setDryRun] = useState(true)
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState<ScoreRunResult | null>(null)
  const [error, setError] = useState<string | null>(null)

  const toggle = (id: string) =>
    setChosen((c) => (c.includes(id) ? c.filter((x) => x !== id) : [...c, id]))

  const run = () => {
    setBusy(true)
    setError(null)
    more
      .scoreRuns({ project, scorers: chosen, limit, dry_run: dryRun, trace_ids: [] }, { endpoint })
      .then((r) => {
        setResult(r)
        if (!dryRun) onDone()
      })
      .catch((e) => setError(message(e)))
      .finally(() => setBusy(false))
  }

  return (
    <div className="panel p-4">
      <p className="text-[0.8125rem] text-ink-3">
        Scorers that need no settings. Leave all unticked for the default code set. Judged scorers make one
        model call per run.
      </p>
      <div className="mt-3 flex flex-wrap gap-x-4 gap-y-2">
        {runnable.map((s) => (
          <label key={s.id} className="flex items-center gap-1.5 text-[0.8125rem]">
            <input type="checkbox" checked={chosen.includes(s.id)} onChange={() => toggle(s.id)} />
            <span className="data">{s.id}</span>
          </label>
        ))}
      </div>
      <div className="mt-3 flex flex-wrap items-center gap-3">
        <label className="flex items-center gap-1.5 text-[0.8125rem]">
          newest
          <input
            type="number"
            min={1}
            max={500}
            value={limit}
            onChange={(e) => setLimit(Math.max(1, Math.min(500, Number(e.target.value) || 1)))}
            className="field w-20"
          />
          runs
        </label>
        <label className="flex items-center gap-1.5 text-[0.8125rem]">
          <input type="checkbox" checked={dryRun} onChange={(e) => setDryRun(e.target.checked)} />
          dry run, store nothing
        </label>
        <button
          type="button"
          disabled={busy}
          onClick={run}
          className="legend panel-raised px-3 py-2 text-ink transition-colors hover:bg-panel-3"
        >
          {busy ? 'Scoring…' : 'Score'}
        </button>
      </div>
      {error && <Failure title="Scoring refused" error={error} />}
      {result && (
        <div className="mt-4">
          <p className="data text-[0.8125rem] text-ink-2">
            {result.traces_scored} runs read, {result.scores.length} scores
            {dryRun ? ' (not stored)' : `, ${result.scores_written} stored`}, {result.skipped.length} declined
          </p>
          {result.skipped.length > 0 && (
            <ul className="mt-2 space-y-1 text-[0.8125rem] text-ink-2">
              {result.skipped.slice(0, 20).map((s, i) => (
                <li key={i}>
                  <span className="data">{s.scorer_id}</span> on{' '}
                  <button type="button" className="link data" onClick={() => onOpenTrace(s.trace_id)}>
                    {s.trace_id.slice(0, 8)}
                  </button>
                  : {s.reason}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  )
}
