/**
 * The aspect and theme routes, typed with the generated wire types.
 *
 * Kept out of `screen-kit.ts` because the trace panel reads one of them too,
 * and the trace panel must not import the screen frame it is mounted in.
 */

import { request } from '@/lib/api'
import type { ClientOptions } from '@/lib/api'
import type {
  AspectCatalogue,
  AspectProgress,
  EntityIndexResponse,
  EntityKind,
  SnapshotListResponse,
  ThemeDetail,
  ThemeExampleListResponse,
  ThemeFlow,
  ThemeHistory,
  TraceAspectNotes,
} from '@/generated/types'

type Value = string | number | undefined | null | readonly string[]

/* Repeated keys, such as `aspects`, are appended once per value: the daemon
   reads them in the order given, and for a flow the order is the question. */
function qs(params: Record<string, Value>): string {
  const p = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) {
    if (v === undefined || v === null || v === '') continue
    if (Array.isArray(v)) for (const x of v) p.append(k, x)
    else p.set(k, String(v))
  }
  const s = p.toString()
  return s ? `?${s}` : ''
}

/** A share between 0 and 1 as a percentage, with a decimal below ten. */
export function share(v: number): string {
  const p = v * 100
  return `${p < 10 && p > 0 ? p.toFixed(1) : p.toFixed(0)}%`
}

/** An entity is the pair of a kind and a name. */
export interface EntityRef {
  kind: EntityKind
  name: string
}

const of = (project: string, e: EntityRef) => ({
  project,
  entity_kind: e.kind,
  entity_name: e.name,
})

export const themesApi = {
  catalogue: (project: string, o?: ClientOptions) =>
    request<AspectCatalogue>(`/v1/api/aspects${qs({ project })}`, o),
  entities: (project: string, o?: ClientOptions) =>
    request<EntityIndexResponse>(`/v1/api/aspects/entities${qs({ project, limit: 200 })}`, o),
  progress: (project: string, e: EntityRef, o?: ClientOptions) =>
    request<AspectProgress>(`/v1/api/aspects/progress${qs(of(project, e))}`, o),
  snapshots: (project: string, e: EntityRef, o?: ClientOptions) =>
    request<SnapshotListResponse>(
      `/v1/api/themes/snapshots${qs({ ...of(project, e), presentation: 'landmarks', limit: 24 })}`,
      o,
    ),
  flow: (
    project: string,
    e: EntityRef,
    snapshotId: string,
    aspects: readonly string[],
    limit: number,
    o?: ClientOptions,
  ) =>
    request<ThemeFlow>(
      `/v1/api/themes/flow${qs({
        ...of(project, e),
        snapshot_id: snapshotId,
        aspects,
        theme_limit_per_stage: limit,
      })}`,
      o,
    ),
  theme: (project: string, e: EntityRef, aspect: string, themeId: string, snapshotId: string, o?: ClientOptions) =>
    request<ThemeDetail>(
      `/v1/api/themes/${encodeURIComponent(themeId)}${qs({ ...of(project, e), aspect, snapshot_id: snapshotId })}`,
      o,
    ),
  examples: (project: string, e: EntityRef, aspect: string, themeId: string, snapshotId: string, o?: ClientOptions) =>
    request<ThemeExampleListResponse>(
      `/v1/api/themes/${encodeURIComponent(themeId)}/examples${qs({
        ...of(project, e),
        aspect,
        snapshot_id: snapshotId,
        limit: 20,
      })}`,
      o,
    ),
  history: (project: string, e: EntityRef, aspect: string, themeId: string, o?: ClientOptions) =>
    request<ThemeHistory>(
      `/v1/api/themes/${encodeURIComponent(themeId)}/history${qs({ ...of(project, e), aspect, limit: 100 })}`,
      o,
    ),
  traceNotes: (traceId: string, project: string, o?: ClientOptions) =>
    request<TraceAspectNotes>(`/v1/api/traces/${encodeURIComponent(traceId)}/aspects${qs({ project })}`, o),
}
