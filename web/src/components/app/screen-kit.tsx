/**
 * What the evaluation, dataset, metric and monitor screens share: the typed
 * calls they make, the toolbar every screen opens with, and the small states a
 * reading can be in. Kept apart from `shell.tsx` so those screens can grow
 * without touching the frame the older ones are built on.
 */

import type { ReactNode } from 'react'
import { request } from '@/lib/api'
import type { ClientOptions } from '@/lib/api'
import type {
  Dataset,
  DatasetItem,
  DiscoveryResponse,
  Experiment,
  ExperimentComparison,
  HealthStatus,
  MetricHistogram,
  MetricPointListResponse,
  MonitorState,
  ScoreListResponse,
  ScoreRunRequest,
  ScoreRunResult,
  ScorerInfo,
  SloState,
} from '@/generated/types'
import { DaemonTile, ProjectPicker } from './shell'

const qs = (params: Record<string, string | number | boolean | undefined | null>) => {
  const p = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== '') p.set(k, String(v))
  }
  const s = p.toString()
  return s ? `?${s}` : ''
}

export const more = {
  scorers: (o?: ClientOptions, project?: string) =>
    request<ScorerInfo[]>(`/v1/api/scorers${qs({ project })}`, o),
  scores: (
    params: { project: string; scorer_id?: string; trace_id?: string; experiment_id?: string; limit?: number },
    o?: ClientOptions,
  ) => request<ScoreListResponse>(`/v1/api/scores${qs(params)}`, o),
  scoreRuns: (body: ScoreRunRequest, o?: ClientOptions) =>
    request<ScoreRunResult>('/v1/api/score', o, body),
  datasets: (project: string, o?: ClientOptions) =>
    request<Dataset[]>(`/v1/api/datasets${qs({ project })}`, o),
  datasetItems: (id: string, o?: ClientOptions) =>
    request<DatasetItem[]>(`/v1/api/datasets/${encodeURIComponent(id)}/items${qs({ limit: 200 })}`, o),
  capture: (id: string, project: string, traceIds: string[], useOutput: boolean, o?: ClientOptions) =>
    request<{ captured: number; requested: number }>(
      `/v1/api/datasets/${encodeURIComponent(id)}/capture${qs({ project })}`,
      o,
      { trace_ids: traceIds, use_output_as_expected: useOutput },
    ),
  experiments: (project: string, o?: ClientOptions) =>
    request<Experiment[]>(`/v1/api/experiments${qs({ project })}`, o),
  compare: (project: string, baseline: string, candidate: string, o?: ClientOptions) =>
    request<ExperimentComparison>(
      `/v1/api/experiments/compare${qs({ project, baseline, candidate })}`,
      o,
    ),
  monitors: (o?: ClientOptions) => request<MonitorState[]>('/v1/api/monitors', o),
  slos: (o?: ClientOptions) => request<SloState[]>('/v1/api/slos', o),
  ackMonitor: (id: string, o?: ClientOptions) =>
    request<MonitorState>(`/v1/api/monitors/${encodeURIComponent(id)}/ack`, o, {}),
  ackSlo: (id: string, o?: ClientOptions) =>
    request<SloState>(`/v1/api/slos/${encodeURIComponent(id)}/ack`, o, {}),
  metricNames: (project: string, o?: ClientOptions) =>
    request<DiscoveryResponse>(`/v1/api/discovery${qs({ project, facet: 'metric_name', limit: 200 })}`, o),
  metricLabelKeys: (project: string, o?: ClientOptions) =>
    request<DiscoveryResponse>(
      `/v1/api/discovery${qs({ project, facet: 'metric_label_key', limit: 100 })}`,
      o,
    ),
  histogram: (
    params: { project: string; name: string; from: string; to: string; group_by?: string; filter?: string },
    o?: ClientOptions,
  ) => request<MetricHistogram>(`/v1/api/metrics/histogram${qs(params)}`, o),
  points: (
    params: { project: string; name?: string; from?: string; limit?: number; cursor?: string },
    o?: ClientOptions,
  ) => request<MetricPointListResponse>(`/v1/api/metrics/points${qs(params)}`, o),
}

/** The props every screen in the selector receives. */
export interface ScreenProps {
  endpoint: string
  onEndpoint: (v: string) => void
  project: string
  onProject: (v: string) => void
  onOpenTrace: (id: string) => void
}

export function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

/** The bar every screen opens with: which daemon, which project, then its own controls. */
export function Toolbar({
  health,
  endpoint,
  projects,
  project,
  onProject,
  children,
  status,
}: {
  health: HealthStatus
  endpoint: string
  projects: string[]
  project: string
  onProject: (v: string) => void
  children?: ReactNode
  status?: string
}) {
  return (
    <div className="border-b border-scribe bg-panel-1">
      <div className="flex flex-wrap items-center gap-2.5 px-4 py-2.5">
        <DaemonTile health={health} endpoint={endpoint} />
        <ProjectPicker projects={projects} project={project} onProject={onProject} />
        {children}
        <span className="data ml-auto text-[0.75rem] text-ink-3">{status ?? ''}</span>
      </div>
      <div className="ticks opacity-40" aria-hidden="true" />
    </div>
  )
}

/** A failed reading, with the daemon's own words. */
export function Failure({ title, error }: { title: string; error: string }) {
  return (
    <div className="py-3">
      <p className="legend text-oxide">{title}</p>
      <p className="mt-2 max-w-[70ch] text-[0.875rem] leading-relaxed text-ink-2">{error}</p>
    </div>
  )
}

/** A section with a legend heading, as the spend screen draws them. */
export function Section({ title, children, aside }: { title: string; children: ReactNode; aside?: ReactNode }) {
  return (
    <section className="mt-8 first:mt-0">
      <div className="flex flex-wrap items-baseline gap-3">
        <h2 className="legend text-ink-3">{title}</h2>
        {aside && <div className="ml-auto">{aside}</div>}
      </div>
      <div className="mt-3">{children}</div>
    </section>
  )
}

/** What a reading shows before it has anything. */
export function Empty({ children }: { children: ReactNode }) {
  return <p className="max-w-[70ch] text-[0.875rem] leading-relaxed text-ink-3">{children}</p>
}

export function fixed(v: number | null | undefined, digits = 3): string {
  return v == null || Number.isNaN(v) ? '' : v.toFixed(digits)
}

export function when(iso: string | null | undefined): string {
  if (!iso) return ''
  const d = new Date(iso)
  return d.toLocaleString(undefined, { hour12: false })
}
