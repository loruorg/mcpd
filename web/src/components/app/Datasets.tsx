/**
 * The datasets and experiments screen.
 *
 * A dataset is the set of cases a version has to pass, often captured from
 * production runs that went wrong. An experiment is one version's scores
 * against it. The comparison at the bottom is the reason both exist: per
 * scorer, the move, its 95% interval, and whether it is a regression given
 * which way that scorer is better.
 */

import { useEffect, useState } from 'react'
import type { Dataset, DatasetItem, Experiment, ExperimentComparison } from '@/generated/types'
import { Disconnected } from './Explorer'
import { useConnection } from './shell'
import { Empty, Failure, fixed, message, more, Section, Toolbar, when } from './screen-kit'
import type { ScreenProps } from './screen-kit'

export default function Datasets({ endpoint, onEndpoint, project, onProject, onOpenTrace }: ScreenProps) {
  const { conn, projects, retry } = useConnection(endpoint, onEndpoint)
  const [datasets, setDatasets] = useState<Dataset[] | null>(null)
  const [experiments, setExperiments] = useState<Experiment[] | null>(null)
  const [open, setOpen] = useState<string | null>(null)
  const [items, setItems] = useState<DatasetItem[] | null>(null)
  const [baseline, setBaseline] = useState('')
  const [candidate, setCandidate] = useState('')
  const [comparison, setComparison] = useState<ExperimentComparison | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (conn.state !== 'connected') return
    const ac = new AbortController()
    const o = { endpoint, signal: ac.signal }
    setError(null)
    Promise.all([more.datasets(project, o), more.experiments(project, o)])
      .then(([d, e]) => {
        setDatasets(d)
        setExperiments(e)
        if (e.length >= 2) {
          setCandidate((c) => c || e[0].experiment_id)
          setBaseline((b) => b || e[1].experiment_id)
        }
      })
      .catch((e) => {
        if ((e as Error).name !== 'AbortError') setError(message(e))
      })
    return () => ac.abort()
  }, [conn.state, endpoint, project])

  useEffect(() => {
    if (!open) return
    setItems(null)
    more
      .datasetItems(open, { endpoint })
      .then(setItems)
      .catch((e) => setError(message(e)))
  }, [open, endpoint])

  useEffect(() => {
    if (!baseline || !candidate || baseline === candidate) {
      setComparison(null)
      return
    }
    more
      .compare(project, baseline, candidate, { endpoint })
      .then(setComparison)
      .catch((e) => setError(message(e)))
  }, [baseline, candidate, project, endpoint])

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
        status={datasets ? `${datasets.length} datasets, ${experiments?.length ?? 0} experiments` : ''}
      />
      <div className="mx-auto max-w-[86rem] px-4 py-6 sm:px-6">
        {error && <Failure title="Query failed" error={error} />}

        <Section title="Datasets">
          {datasets && datasets.length === 0 ? (
            <Empty>
              No dataset in this project. Create one with <code>POST /v1/api/datasets</code>, then capture runs
              into it from the run list.
            </Empty>
          ) : (
            <div className="panel overflow-x-auto">
              <table className="w-full text-left text-[0.8125rem]">
                <thead className="legend text-ink-3">
                  <tr>
                    <th className="px-3 py-2">Dataset</th>
                    <th className="px-3 py-2">Cases</th>
                    <th className="px-3 py-2">Created</th>
                    <th className="px-3 py-2">For</th>
                  </tr>
                </thead>
                <tbody>
                  {datasets?.map((d) => (
                    <tr key={d.dataset_id} className="border-t border-scribe">
                      <td className="px-3 py-1.5">
                        <button
                          type="button"
                          className="link"
                          aria-expanded={open === d.dataset_id}
                          onClick={() => setOpen(open === d.dataset_id ? null : d.dataset_id)}
                        >
                          {d.name}
                        </button>
                      </td>
                      <td className="data px-3 py-1.5">{d.item_count}</td>
                      <td className="data px-3 py-1.5 whitespace-nowrap">{when(d.created_at)}</td>
                      <td className="px-3 py-1.5 text-ink-2">{d.description ?? ''}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Section>

        {open && (
          <Section title="Cases">
            {items === null ? (
              <Empty>Reading…</Empty>
            ) : items.length === 0 ? (
              <Empty>This dataset holds no case yet.</Empty>
            ) : (
              <div className="panel overflow-x-auto">
                <table className="w-full text-left text-[0.8125rem]">
                  <thead className="legend text-ink-3">
                    <tr>
                      <th className="px-3 py-2">Input</th>
                      <th className="px-3 py-2">Expected</th>
                      <th className="px-3 py-2">From run</th>
                    </tr>
                  </thead>
                  <tbody>
                    {items.map((it) => (
                      <tr key={it.item_id} className="border-t border-scribe align-top">
                        <td className="data max-w-[32rem] px-3 py-1.5 break-words">{preview(it.input)}</td>
                        <td className="data max-w-[32rem] px-3 py-1.5 break-words text-ink-2">
                          {it.expected == null ? 'open-ended' : preview(it.expected)}
                        </td>
                        <td className="px-3 py-1.5">
                          {it.source_trace_id && (
                            <button type="button" className="link" onClick={() => onOpenTrace(it.source_trace_id!)}>
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
        )}

        <Section title="Experiments">
          {experiments && experiments.length === 0 ? (
            <Empty>
              No experiment recorded. Record one per version with <code>POST /v1/api/experiments</code> and tag
              its scores with the experiment id.
            </Empty>
          ) : (
            <div className="panel overflow-x-auto">
              <table className="w-full text-left text-[0.8125rem]">
                <thead className="legend text-ink-3">
                  <tr>
                    <th className="px-3 py-2">Experiment</th>
                    <th className="px-3 py-2">Status</th>
                    <th className="px-3 py-2">Cases</th>
                    <th className="px-3 py-2">Started</th>
                    <th className="px-3 py-2">Configuration</th>
                  </tr>
                </thead>
                <tbody>
                  {experiments?.map((e) => (
                    <tr key={e.experiment_id} className="border-t border-scribe">
                      <td className="px-3 py-1.5">
                        {e.name} <span className="data text-ink-3">{e.experiment_id}</span>
                      </td>
                      <td className="px-3 py-1.5">{e.status}</td>
                      <td className="data px-3 py-1.5">{e.item_count}</td>
                      <td className="data px-3 py-1.5 whitespace-nowrap">{when(e.started_at)}</td>
                      <td className="data max-w-[28rem] truncate px-3 py-1.5 text-ink-2">
                        {e.metadata ? JSON.stringify(e.metadata) : ''}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Section>

        {experiments && experiments.length >= 2 && (
          <Section
            title="Compare two experiments"
            aside={
              <div className="flex flex-wrap items-center gap-2 text-[0.8125rem]">
                <select value={baseline} onChange={(e) => setBaseline(e.target.value)} aria-label="Baseline" className="field">
                  {experiments.map((e) => (
                    <option key={e.experiment_id} value={e.experiment_id}>
                      baseline {e.name}
                    </option>
                  ))}
                </select>
                <select value={candidate} onChange={(e) => setCandidate(e.target.value)} aria-label="Candidate" className="field">
                  {experiments.map((e) => (
                    <option key={e.experiment_id} value={e.experiment_id}>
                      candidate {e.name}
                    </option>
                  ))}
                </select>
              </div>
            }
          >
            {comparison && <Comparison c={comparison} />}
          </Section>
        )}
      </div>
    </div>
  )
}

function Comparison({ c }: { c: ExperimentComparison }) {
  return (
    <div>
      <p className={`text-[0.875rem] ${c.regressed ? 'text-oxide' : 'text-ink-2'}`}>
        {c.regressed
          ? 'At least one scorer regressed: its whole 95% interval is on the worse side of zero.'
          : 'No scorer regressed.'}
      </p>
      <div className="panel mt-3 overflow-x-auto">
        <table className="w-full text-left text-[0.8125rem]">
          <thead className="legend text-ink-3">
            <tr>
              <th className="px-3 py-2">Scorer</th>
              <th className="px-3 py-2">Baseline</th>
              <th className="px-3 py-2">Candidate</th>
              <th className="px-3 py-2">Delta</th>
              <th className="px-3 py-2">95% interval</th>
              <th className="px-3 py-2">p</th>
              <th className="px-3 py-2">Reading</th>
            </tr>
          </thead>
          <tbody>
            {c.scorers.map((s) => (
              <tr key={s.scorer_id} className="border-t border-scribe">
                <td className="data px-3 py-1.5">{s.scorer_id}</td>
                <td className="data px-3 py-1.5">
                  {fixed(s.baseline_mean)} <span className="text-ink-3">n={s.baseline_n}</span>
                </td>
                <td className="data px-3 py-1.5">
                  {fixed(s.candidate_mean)} <span className="text-ink-3">n={s.candidate_n}</span>
                </td>
                <td className="data px-3 py-1.5">{s.delta == null ? '' : (s.delta >= 0 ? '+' : '') + fixed(s.delta)}</td>
                <td className="data px-3 py-1.5">
                  {s.ci_low == null ? '' : `${fixed(s.ci_low)} to ${fixed(s.ci_high)}`}
                </td>
                <td className="data px-3 py-1.5">{fixed(s.p_value, 4)}</td>
                <td className={`px-3 py-1.5 ${s.regressed ? 'text-oxide' : 'text-ink-2'}`}>
                  {!s.decidable
                    ? 'too few runs to decide'
                    : s.regressed
                      ? 'regressed'
                      : s.direction === 'lower_is_better'
                        ? 'no regression, lower is better'
                        : 'no regression'}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

function preview(v: unknown): string {
  const s = typeof v === 'string' ? v : JSON.stringify(v)
  return s.length > 300 ? `${s.slice(0, 300)}…` : s
}
