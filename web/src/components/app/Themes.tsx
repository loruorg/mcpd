/**
 * The themes screen.
 *
 * What the runs of one agent, workflow or tool were about, read from aspect
 * notes: which entities have notes, how far each aspect has got towards a
 * snapshot, the themes the latest pass found with how each one moved since
 * the passes before it, and the flow of runs across aspects. A theme opens a
 * drawer with the notes behind it and its history, and every note links to the
 * run it was written about, because a theme is only as good as the runs that
 * prove it.
 *
 * Nothing here is recomputed in the browser. Counts, shares and readiness come
 * from the daemon; the screen only compares two snapshots it was given, and
 * never adds their counts together, because their windows overlap.
 */

import { useEffect, useMemo, useState } from 'react'
import * as Dialog from '@radix-ui/react-dialog'
import type {
  Aspect,
  AspectCatalogue,
  AspectProgress,
  AspectReadiness,
  EntityIndexRow,
  SnapshotLandmark,
  ThemeDetail,
  ThemeExample,
  ThemeFlow,
  ThemeHistory,
  ThemeSnapshot,
  ThemeState,
} from '@/generated/types'
import { DaemonError } from '@/lib/api'
import { Disconnected } from './Explorer'
import { CrossMark } from './list-ui'
import { useConnection } from './shell'
import { Empty, Failure, message, Section, Toolbar, when } from './screen-kit'
import type { ScreenProps } from './screen-kit'
import { ThemeFlowChart } from './ThemeFlowChart'
import { share, themesApi } from './themes-api'
import type { EntityRef } from './themes-api'

/** How many earlier snapshots the trend reads, the current one aside. */
const TREND_DEPTH = 5

/** A move in share smaller than this, in points, reads as steady. */
const STEADY_POINTS = 2

/** What a theme drawer is open on. `noise` is the notes that joined no theme. */
interface Opened {
  aspect: string
  themeId: string
  label: string
}

/** One theme as the list shows it: the current reading and its series. */
interface ThemeRow {
  themeId: string
  label: string
  traceCount: number
  coverage: number
  /** Share in each snapshot of the trend, oldest first; null where absent. */
  series: (number | null)[]
}

interface AspectThemes {
  aspect: string
  traceCount: number
  rows: ThemeRow[]
  noise: { traceCount: number; coverage: number } | null
  /** Themes the previous snapshot held and this one does not. */
  gone: { themeId: string; label: string }[]
}

const READINESS_WORD: Record<AspectReadiness, string> = {
  collecting: 'collecting',
  processing: 'waiting for the next pass',
  ready: 'ready',
}

const STATE_WORD: Record<ThemeState, string> = {
  birth: 'first seen in this snapshot',
  continue: 'held from the previous snapshot',
  split: 'split from a larger theme',
  merge: 'merged from several themes',
  resurrection: 'back after a gap',
  death: 'not seen in this snapshot',
}

const entityKey = (e: { entity_kind: string; entity_name: string }) => `${e.entity_kind}:${e.entity_name}`

export default function Themes({ endpoint, onEndpoint, project, onProject, onOpenTrace }: ScreenProps) {
  const { conn, projects, retry } = useConnection(endpoint, onEndpoint)
  const [catalogue, setCatalogue] = useState<AspectCatalogue | null>(null)
  const [entities, setEntities] = useState<EntityIndexRow[] | null>(null)
  const [chosen, setChosen] = useState<string | null>(null)
  const [progress, setProgress] = useState<AspectProgress | null>(null)
  const [snapshots, setSnapshots] = useState<SnapshotLandmark[] | null>(null)
  const [snapshotId, setSnapshotId] = useState<string | null>(null)
  const [flows, setFlows] = useState<Map<string, ThemeFlow> | null>(null)
  const [opened, setOpened] = useState<Opened | null>(null)
  const [error, setError] = useState<string | null>(null)

  const entity: EntityRef | null = useMemo(() => {
    const row = entities?.find((e) => entityKey(e) === chosen)
    return row ? { kind: row.entity_kind, name: row.entity_name } : null
  }, [entities, chosen])

  // The catalogue and the entity index, per project.
  useEffect(() => {
    if (conn.state !== 'connected') return
    const ac = new AbortController()
    const o = { endpoint, signal: ac.signal }
    setError(null)
    setEntities(null)
    setChosen(null)
    Promise.all([themesApi.catalogue(project, o), themesApi.entities(project, o)])
      .then(([c, e]) => {
        setCatalogue(c)
        setEntities(e.entities)
        // The first entity with themes to read, or the first with notes at all.
        const first = e.entities.find((r) => r.readiness === 'ready') ?? e.entities[0]
        setChosen(first ? entityKey(first) : null)
      })
      .catch((e) => {
        if ((e as Error).name !== 'AbortError') setError(message(e))
      })
    return () => ac.abort()
  }, [conn.state, endpoint, project])

  // Progress and the snapshot series, per entity.
  useEffect(() => {
    if (!entity) return
    const ac = new AbortController()
    const o = { endpoint, signal: ac.signal }
    setProgress(null)
    setSnapshots(null)
    setSnapshotId(null)
    setFlows(null)
    Promise.all([themesApi.progress(project, entity, o), themesApi.snapshots(project, entity, o)])
      .then(([p, s]) => {
        setProgress(p)
        const sorted = [...s.snapshots].sort((a, b) => a.snapshot.ordinal - b.snapshot.ordinal)
        setSnapshots(sorted)
        setSnapshotId(sorted.length ? sorted[sorted.length - 1].snapshot.snapshot_id : null)
      })
      .catch((e) => {
        if ((e as Error).name !== 'AbortError') setError(message(e))
      })
    return () => ac.abort()
  }, [entity, endpoint, project])

  // The snapshots the trend reads: the chosen one and up to five before it.
  const trend: ThemeSnapshot[] = useMemo(() => {
    if (!snapshots || !snapshotId) return []
    const at = snapshots.findIndex((s) => s.snapshot.snapshot_id === snapshotId)
    if (at < 0) return []
    return snapshots.slice(Math.max(0, at - TREND_DEPTH), at + 1).map((s) => s.snapshot)
  }, [snapshots, snapshotId])

  // Every theme of each trend snapshot, read through the flow. It is the one
  // route that lists a snapshot's themes with their counts, and a flow over a
  // single aspect names it twice, because a flow needs two stages.
  useEffect(() => {
    if (!entity || trend.length === 0) return
    const ac = new AbortController()
    const o = { endpoint, signal: ac.signal }
    setFlows(null)
    Promise.all(
      trend.map((s) => {
        const stages = s.aspects.length === 1 ? [s.aspects[0], s.aspects[0]] : s.aspects.slice(0, 4)
        return themesApi
          .flow(project, entity, s.snapshot_id, stages, 100, o)
          .then((f) => [s.snapshot_id, f] as const)
      }),
    )
      .then((pairs) => setFlows(new Map(pairs)))
      .catch((e) => {
        if ((e as Error).name !== 'AbortError') setError(message(e))
      })
    return () => ac.abort()
  }, [entity, trend, endpoint, project])

  const current = trend.length ? trend[trend.length - 1] : null
  const grouped = useMemo(() => (flows && current ? themesOf(trend, flows) : null), [flows, trend, current])

  if (conn.state !== 'connected') {
    return <Disconnected conn={conn} endpoint={endpoint} onEndpoint={onEndpoint} onRetry={retry} />
  }

  const labelOf = (name: string) => catalogue?.aspects.find((a) => a.name === name)?.label ?? name
  const unsupported = error && /theme_analysis/.test(error)

  return (
    <div className="min-h-[calc(100vh-11rem)]">
      <Toolbar
        health={conn.health}
        endpoint={endpoint}
        projects={projects}
        project={project}
        onProject={onProject}
        status={entities ? `${entities.length} ${entities.length === 1 ? 'entity' : 'entities'} with notes` : ''}
      >
        {entities && entities.length > 1 && (
          <select
            value={chosen ?? ''}
            onChange={(e) => setChosen(e.target.value)}
            aria-label="Entity"
            className="field"
          >
            {entities.map((e) => (
              <option key={entityKey(e)} value={entityKey(e)}>
                {e.entity_kind} {e.entity_name}
              </option>
            ))}
          </select>
        )}
        {snapshots && snapshots.length > 1 && (
          <select
            value={snapshotId ?? ''}
            onChange={(e) => setSnapshotId(e.target.value)}
            aria-label="Snapshot"
            className="field"
          >
            {snapshots.map((s) => (
              <option key={s.snapshot.snapshot_id} value={s.snapshot.snapshot_id}>
                snapshot {s.snapshot.ordinal}, {when(s.snapshot.cutoff_at)}
              </option>
            ))}
          </select>
        )}
      </Toolbar>

      <div className="mx-auto max-w-[86rem] px-4 py-6 sm:px-6">
        {error && (
          <Failure
            title={unsupported ? 'This storage backend holds no themes' : 'Query failed'}
            error={error}
          />
        )}

        <Section title="Entities">
          {entities === null ? (
            !error && <Empty>Reading…</Empty>
          ) : entities.length === 0 ? (
            <Empty>
              No aspect notes in this project yet. A note is one sentence about a finished run. Post notes with{' '}
              <code>POST /v1/ingest</code>, or set <code>aspects.enabled</code> in the config file and the daemon
              writes the outcome aspect itself.
            </Empty>
          ) : (
            <EntityTable rows={entities} chosen={chosen} onChoose={setChosen} />
          )}
        </Section>

        {entity && catalogue && (
          <Section title={`Aspects of ${entity.name}`}>
            {progress ? (
              <AspectTable aspects={catalogue.aspects} progress={progress} floor={catalogue.limits.min_notes_to_cluster} />
            ) : (
              <Empty>Reading…</Empty>
            )}
            {!catalogue.limits.pass_armed && (
              <p className="mt-3 max-w-[70ch] text-[0.8125rem] leading-relaxed text-ink-2">
                No clustering pass runs on this daemon: <code>aspects.enabled</code> is off in its config file. Notes
                are still stored, and the snapshots below are the ones already written.
              </p>
            )}
          </Section>
        )}

        {entity && progress && (
          <Section
            title="Themes"
            aside={current && <SnapshotLine snapshot={current} />}
          >
            {snapshots && snapshots.length === 0 ? (
              <Empty>{noSnapshot(progress)}</Empty>
            ) : !grouped ? (
              <Empty>Reading…</Empty>
            ) : (
              <div className="grid gap-6">
                {grouped.map((g) => (
                  <ThemeTable
                    key={g.aspect}
                    group={g}
                    label={labelOf(g.aspect)}
                    trendLength={trend.length}
                    onOpen={(o) => setOpened(o)}
                  />
                ))}
              </div>
            )}
          </Section>
        )}

        {entity && current && flows?.get(current.snapshot_id) && (
          <Section title="Flow across aspects">
            <FlowSection
              key={current.snapshot_id}
              project={project}
              endpoint={endpoint}
              entity={entity}
              snapshot={current}
              labelOf={labelOf}
              onOpen={setOpened}
            />
          </Section>
        )}
      </div>

      {entity && current && (
        <ThemeDrawer
          opened={opened}
          onClose={() => setOpened(null)}
          project={project}
          endpoint={endpoint}
          entity={entity}
          snapshot={current}
          labelOf={labelOf}
          onOpenTrace={(id) => {
            setOpened(null)
            onOpenTrace(id)
          }}
        />
      )}
    </div>
  )
}

/** Why an entity with notes has no snapshot yet, from its own progress. */
function noSnapshot(p: AspectProgress): string {
  if (p.readiness === 'processing') {
    return 'No snapshot yet. This entity has enough notes, and the next clustering pass writes the first one.'
  }
  const short = p.aspects.filter((a) => a.readiness === 'collecting')
  const least = short.length ? Math.min(...short.map((a) => a.notes_needed)) : 0
  return least
    ? `No snapshot yet. Every aspect is still collecting; the closest needs ${least} more ${least === 1 ? 'note' : 'notes'} before a pass will cluster it.`
    : 'No snapshot yet.'
}

/* ------------------------------------------------------------ entities --- */

function EntityTable({
  rows,
  chosen,
  onChoose,
}: {
  rows: EntityIndexRow[]
  chosen: string | null
  onChoose: (k: string) => void
}) {
  return (
    <div className="panel overflow-x-auto">
      <table className="w-full text-left text-[0.8125rem]">
        <thead className="legend text-ink-3">
          <tr>
            <th className="px-3 py-2">Entity</th>
            <th className="px-3 py-2">Kind</th>
            <th className="px-3 py-2 text-right">Runs</th>
            <th className="px-3 py-2 text-right">Notes</th>
            <th className="px-3 py-2">Aspects ready</th>
            <th className="px-3 py-2">State</th>
            <th className="px-3 py-2">Last note</th>
            <th className="px-3 py-2">Last snapshot</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => {
            const k = entityKey(r)
            const on = k === chosen
            return (
              <tr key={k} className={`border-t border-scribe ${on ? 'bg-panel-3' : ''}`}>
                <td className="px-3 py-1.5">
                  <button
                    type="button"
                    className={on ? 'font-medium text-ink' : 'link'}
                    aria-pressed={on}
                    onClick={() => onChoose(k)}
                  >
                    {r.entity_name}
                  </button>
                </td>
                <td className="px-3 py-1.5 text-ink-2">{r.entity_kind}</td>
                <td className="data px-3 py-1.5 text-right">{r.trace_count}</td>
                <td className="data px-3 py-1.5 text-right">{r.note_count}</td>
                <td className="data px-3 py-1.5">
                  {r.ready_aspects} of {r.enabled_aspects}
                </td>
                <td className="px-3 py-1.5 text-ink-2">{READINESS_WORD[r.readiness]}</td>
                <td className="data px-3 py-1.5 whitespace-nowrap text-ink-2">{when(r.last_note_at)}</td>
                <td className="data px-3 py-1.5 whitespace-nowrap text-ink-2">
                  {r.latest_snapshot_at ? when(r.latest_snapshot_at) : 'none yet'}
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}

/* ------------------------------------------------------------- aspects --- */

function AspectTable({
  aspects,
  progress,
  floor,
}: {
  aspects: Aspect[]
  progress: AspectProgress
  floor: number
}) {
  const ordered = [...aspects].sort((a, b) => a.sort_order - b.sort_order)
  return (
    <div className="panel overflow-x-auto">
      <table className="w-full text-left text-[0.8125rem]">
        <thead className="legend text-ink-3">
          <tr>
            <th className="px-3 py-2">Aspect</th>
            <th className="px-3 py-2">Written by</th>
            <th className="px-3 py-2 text-right">Notes</th>
            <th className="px-3 py-2 text-right">Runs</th>
            <th className="px-3 py-2">Progress to a snapshot</th>
          </tr>
        </thead>
        <tbody>
          {ordered.map((a) => {
            const row = progress.aspects.find((r) => r.aspect === a.name)
            const notes = row?.note_count ?? 0
            const fill = Math.min(1, notes / Math.max(1, floor))
            return (
              <tr key={a.name} className="border-t border-scribe align-top">
                <td className="px-3 py-2">
                  <span className="text-ink">{a.label}</span>
                  <span className="mt-0.5 block max-w-[46ch] text-[0.75rem] leading-snug text-ink-3">
                    {a.description}
                  </span>
                </td>
                <td className="px-3 py-2 text-ink-2">
                  {a.derived ? 'the daemon, from run facts' : 'your model'}
                </td>
                <td className="data px-3 py-2 text-right">{notes}</td>
                <td className="data px-3 py-2 text-right">{row?.trace_count ?? 0}</td>
                <td className="px-3 py-2">
                  {!a.enabled ? (
                    <span className="text-ink-3">switched off in this deployment</span>
                  ) : (
                    <div className="flex items-center gap-3">
                      <div className="h-1.5 w-28 shrink-0 overflow-hidden rounded-full bg-well" aria-hidden="true">
                        <div
                          className="h-full rounded-full"
                          style={{
                            width: `${fill * 100}%`,
                            background: row?.readiness === 'ready' ? 'var(--color-accent)' : 'var(--color-ink-3)',
                          }}
                        />
                      </div>
                      <span className="text-ink-2">
                        {!row || row.readiness === 'collecting'
                          ? `collecting, ${row?.notes_needed ?? floor} more ${(row?.notes_needed ?? floor) === 1 ? 'note' : 'notes'} needed`
                          : READINESS_WORD[row.readiness]}
                      </span>
                    </div>
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

/* -------------------------------------------------------------- themes --- */

/** The themes of the newest snapshot in `trend`, with their series. */
function themesOf(trend: ThemeSnapshot[], flows: Map<string, ThemeFlow>): AspectThemes[] {
  const current = trend[trend.length - 1]
  const flow = flows.get(current.snapshot_id)
  if (!flow) return []
  const previous = trend.length > 1 ? flows.get(trend[trend.length - 2].snapshot_id) : undefined
  const seen = new Set<string>()
  const out: AspectThemes[] = []
  for (const stage of flow.stages) {
    // A one-aspect snapshot is read as a flow with the aspect twice.
    if (seen.has(stage.aspect)) continue
    seen.add(stage.aspect)
    const shareIn = (f: ThemeFlow | undefined, themeId: string) => {
      const st = f?.stages.find((s) => s.aspect === stage.aspect)
      const node = st?.nodes.find((n) => n.kind === 'theme' && n.theme_id === themeId)
      return node ? node.coverage : null
    }
    const rows: ThemeRow[] = stage.nodes
      .filter((n) => n.kind === 'theme' && n.theme_id)
      .map((n) => ({
        themeId: n.theme_id!,
        label: n.label,
        traceCount: n.trace_count,
        coverage: n.coverage,
        series: trend.map((s) => shareIn(flows.get(s.snapshot_id), n.theme_id!)),
      }))
      .sort((a, b) => b.traceCount - a.traceCount)
    const noise = stage.nodes.find((n) => n.kind === 'noise')
    const now = new Set(rows.map((r) => r.themeId))
    const before = previous?.stages.find((s) => s.aspect === stage.aspect)
    const gone = (before?.nodes ?? [])
      .filter((n) => n.kind === 'theme' && n.theme_id && !now.has(n.theme_id))
      .map((n) => ({ themeId: n.theme_id!, label: n.label }))
    out.push({
      aspect: stage.aspect,
      traceCount: stage.trace_count,
      rows,
      noise: noise ? { traceCount: noise.trace_count, coverage: noise.coverage } : null,
      gone,
    })
  }
  return out
}

function SnapshotLine({ snapshot: s }: { snapshot: ThemeSnapshot }) {
  return (
    <p className="text-[0.75rem] text-ink-3">
      Snapshot <span className="data text-ink-2">{s.ordinal}</span>, notes from{' '}
      <span className="data text-ink-2">{when(s.window_from)}</span> to{' '}
      <span className="data text-ink-2">{when(s.window_to)}</span>:{' '}
      <span className="data text-ink-2">{s.note_count}</span> notes about{' '}
      <span className="data text-ink-2">{s.trace_count}</span> runs
      {s.capped && ', the newest 20,000 read'}
    </p>
  )
}

/** How a theme moved since the previous snapshot, in words. */
function movement(series: (number | null)[]): { text: string; tone: string } {
  const now = series[series.length - 1]
  const before = series.length > 1 ? series[series.length - 2] : undefined
  if (now == null) return { text: '', tone: 'text-ink-3' }
  if (before === undefined) return { text: 'first snapshot', tone: 'text-ink-3' }
  if (before === null) return { text: 'new', tone: 'text-ink' }
  const points = (now - before) * 100
  if (Math.abs(points) < STEADY_POINTS) return { text: 'steady', tone: 'text-ink-3' }
  return {
    text: `${points > 0 ? 'up' : 'down'} ${Math.abs(points).toFixed(0)} pts`,
    tone: 'text-ink',
  }
}

function ThemeTable({
  group,
  label,
  trendLength,
  onOpen,
}: {
  group: AspectThemes
  label: string
  trendLength: number
  onOpen: (o: Opened) => void
}) {
  return (
    <div>
      <h3 className="display text-[1rem]">
        {label}{' '}
        <span className="data text-[0.75rem] font-normal text-ink-3">
          {group.rows.length} {group.rows.length === 1 ? 'theme' : 'themes'} over {group.traceCount} runs
        </span>
      </h3>
      <div className="panel mt-2 overflow-x-auto">
        {/* Fixed columns, so the tables of every aspect line up under each other. */}
        <table className="w-full min-w-[46rem] table-fixed text-left text-[0.8125rem]">
          <colgroup>
            <col />
            <col className="w-[5rem]" />
            <col className="w-[11rem]" />
            <col className="w-[7rem]" />
            <col className="w-[8rem]" />
          </colgroup>
          <thead className="legend text-ink-3">
            <tr>
              <th className="px-3 py-2">Theme</th>
              <th className="px-3 py-2 text-right">Runs</th>
              <th className="px-3 py-2">Share of notes</th>
              <th className="px-3 py-2" title={`Share in the last ${trendLength} snapshots, oldest on the left`}>
                Trend
              </th>
              <th className="px-3 py-2">Since last</th>
            </tr>
          </thead>
          <tbody>
            {group.rows.length === 0 && (
              <tr className="border-t border-scribe">
                <td colSpan={5} className="px-3 py-2 text-ink-3">
                  No note along this aspect shared its wording with another, so this snapshot has no theme here.
                </td>
              </tr>
            )}
            {group.rows.map((r) => {
              const m = movement(r.series)
              return (
                <tr key={r.themeId} className="border-t border-scribe">
                  <td className="px-3 py-1.5">
                    <button
                      type="button"
                      className="link text-left"
                      onClick={() => onOpen({ aspect: group.aspect, themeId: r.themeId, label: r.label })}
                    >
                      {r.label}
                    </button>
                  </td>
                  <td className="data px-3 py-1.5 text-right">{r.traceCount}</td>
                  <td className="px-3 py-1.5">
                    <ShareBar value={r.coverage} />
                  </td>
                  <td className="px-3 py-1.5">
                    <Sparkline series={r.series} />
                  </td>
                  <td className={`data whitespace-nowrap px-3 py-1.5 ${m.tone}`}>{m.text}</td>
                </tr>
              )
            })}
            {group.noise && group.noise.traceCount > 0 && (
              <tr className="border-t border-scribe">
                <td className="px-3 py-1.5">
                  <button
                    type="button"
                    className="link text-left text-ink-2"
                    onClick={() =>
                      onOpen({ aspect: group.aspect, themeId: 'noise', label: 'Notes that joined no theme' })
                    }
                  >
                    Notes that joined no theme
                  </button>
                </td>
                <td className="data px-3 py-1.5 text-right text-ink-2">{group.noise.traceCount}</td>
                <td className="px-3 py-1.5">
                  <ShareBar value={group.noise.coverage} muted />
                </td>
                <td className="px-3 py-1.5" />
                <td className="px-3 py-1.5" />
              </tr>
            )}
          </tbody>
        </table>
      </div>
      {group.gone.length > 0 && (
        <p className="mt-2 text-[0.75rem] leading-relaxed text-ink-3">
          Not seen since the previous snapshot:{' '}
          {group.gone.map((g, i) => (
            <span key={g.themeId}>
              {i > 0 && '; '}
              <span className="text-ink-2">{g.label}</span>
            </span>
          ))}
        </p>
      )}
    </div>
  )
}

function ShareBar({ value, muted }: { value: number; muted?: boolean }) {
  return (
    <div className="flex items-center gap-2.5">
      <div className="h-1.5 w-24 shrink-0 overflow-hidden rounded-full bg-well" aria-hidden="true">
        <div
          className="h-full rounded-full"
          style={{
            width: `${Math.min(1, value) * 100}%`,
            background: muted ? 'var(--color-ink-4)' : 'var(--color-accent)',
          }}
        />
      </div>
      <span className="data text-ink-2">{share(value)}</span>
    </div>
  )
}

/** A theme's share across the trend snapshots. Gaps are snapshots it was absent from. */
function Sparkline({
  series,
  width = 72,
  height = 18,
}: {
  series: (number | null)[]
  width?: number
  height?: number
}) {
  const present = series.filter((v): v is number => v != null)
  if (series.length < 2 || present.length === 0) return <span className="text-[0.75rem] text-ink-3">one point</span>
  const max = Math.max(...present, 0.01)
  const x = (i: number) => 2 + (i * (width - 4)) / (series.length - 1)
  const y = (v: number) => height - 2 - (v / max) * (height - 4)
  const runs: string[] = []
  let run: string[] = []
  series.forEach((v, i) => {
    if (v == null) {
      if (run.length) runs.push(run.join(' '))
      run = []
    } else run.push(`${x(i).toFixed(1)},${y(v).toFixed(1)}`)
  })
  if (run.length) runs.push(run.join(' '))
  const last = series[series.length - 1]
  return (
    <svg
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      role="img"
      aria-label={`Share by snapshot: ${series.map((v) => (v == null ? 'absent' : share(v))).join(', ')}`}
    >
      <line x1="0" x2={width} y1={height - 2} y2={height - 2} stroke="var(--color-scribe)" />
      {runs.map((pts) => (
        <polyline key={pts} points={pts} fill="none" stroke="var(--color-ink-2)" strokeWidth="1.25" />
      ))}
      {series.map((v, i) =>
        v == null ? null : (
          <circle
            key={i}
            cx={x(i)}
            cy={y(v)}
            r={i === series.length - 1 ? 2.25 : 1.25}
            fill={i === series.length - 1 && last != null ? 'var(--color-accent)' : 'var(--color-ink-2)'}
          />
        ),
      )}
    </svg>
  )
}

/* ---------------------------------------------------------------- flow --- */

function FlowSection({
  project,
  endpoint,
  entity,
  snapshot,
  labelOf,
  onOpen,
}: {
  project: string
  endpoint: string
  entity: EntityRef
  snapshot: ThemeSnapshot
  labelOf: (name: string) => string
  onOpen: (o: Opened) => void
}) {
  const [stages, setStages] = useState<string[]>(() => snapshot.aspects.slice(0, 4))
  const [flow, setFlow] = useState<ThemeFlow | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (stages.length < 2) {
      setFlow(null)
      return
    }
    const ac = new AbortController()
    setError(null)
    themesApi
      .flow(project, entity, snapshot.snapshot_id, stages, 8, { endpoint, signal: ac.signal })
      .then(setFlow)
      .catch((e) => {
        if ((e as Error).name !== 'AbortError') setError(e instanceof DaemonError ? e.message : message(e))
      })
    return () => ac.abort()
  }, [project, endpoint, entity, snapshot.snapshot_id, stages])

  if (snapshot.aspects.length < 2) {
    return (
      <Empty>
        This snapshot covers one aspect, {labelOf(snapshot.aspects[0] ?? '')}. A flow joins two to four aspects, so it
        appears once a second aspect reaches a snapshot.
      </Empty>
    )
  }

  const toggle = (a: string) =>
    setStages((s) => (s.includes(a) ? s.filter((x) => x !== a) : s.length >= 4 ? s : [...s, a]))

  return (
    <div>
      <div className="flex flex-wrap items-center gap-2 text-[0.8125rem]">
        <span className="text-ink-3">Stages, in order:</span>
        {snapshot.aspects.map((a) => {
          const at = stages.indexOf(a)
          return (
            <button
              key={a}
              type="button"
              aria-pressed={at >= 0}
              onClick={() => toggle(a)}
              className={`panel-raised px-2 py-1 transition-colors ${
                at >= 0 ? 'bg-panel-3 text-ink' : 'text-ink-3 hover:bg-panel-3 hover:text-ink'
              }`}
            >
              {at >= 0 && <span className="data mr-1.5 text-ink-3">{at + 1}</span>}
              {labelOf(a)}
            </button>
          )
        })}
      </div>
      <p className="mt-2 max-w-[80ch] text-[0.75rem] leading-relaxed text-ink-3">
        A ribbon joins two themes written about the same runs, and its width is how many runs. That is association,
        not cause. Links join neighbouring stages only, so the order is part of the question.
      </p>
      {error && <Failure title="The flow could not be read" error={error} />}
      {stages.length < 2 ? (
        <div className="mt-4">
          <Empty>Choose at least two aspects.</Empty>
        </div>
      ) : flow ? (
        <div className="panel mt-4 p-4">
          <ThemeFlowChart
            flow={flow}
            labelOf={labelOf}
            onOpen={(aspect, node) =>
              onOpen({
                aspect,
                themeId: node.kind === 'noise' ? 'noise' : node.theme_id!,
                label: node.kind === 'noise' ? 'Notes that joined no theme' : node.label,
              })
            }
          />
        </div>
      ) : (
        !error && (
          <div className="mt-4">
            <Empty>Reading…</Empty>
          </div>
        )
      )}
    </div>
  )
}

/* -------------------------------------------------------------- drawer --- */

function ThemeDrawer({
  opened,
  onClose,
  project,
  endpoint,
  entity,
  snapshot,
  labelOf,
  onOpenTrace,
}: {
  opened: Opened | null
  onClose: () => void
  project: string
  endpoint: string
  entity: EntityRef
  snapshot: ThemeSnapshot
  labelOf: (name: string) => string
  onOpenTrace: (id: string) => void
}) {
  const [detail, setDetail] = useState<ThemeDetail | null>(null)
  const [history, setHistory] = useState<ThemeHistory | null>(null)
  const [examples, setExamples] = useState<ThemeExample[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!opened) return
    const ac = new AbortController()
    const o = { endpoint, signal: ac.signal }
    const { aspect, themeId } = opened
    setDetail(null)
    setHistory(null)
    setExamples(null)
    setError(null)
    const fail = (e: unknown) => {
      if ((e as Error).name !== 'AbortError') setError(message(e))
    }
    themesApi
      .examples(project, entity, aspect, themeId, snapshot.snapshot_id, o)
      .then((r) => setExamples(r.examples))
      .catch(fail)
    // Noise is not a theme: it has no detail and no history of its own.
    if (themeId !== 'noise') {
      themesApi.theme(project, entity, aspect, themeId, snapshot.snapshot_id, o).then(setDetail).catch(fail)
      themesApi.history(project, entity, aspect, themeId, o).then(setHistory).catch(fail)
    }
    return () => ac.abort()
  }, [opened, project, endpoint, entity, snapshot.snapshot_id])

  const theme = detail?.theme
  const noise = opened?.themeId === 'noise'

  return (
    <Dialog.Root open={!!opened} onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-50" style={{ background: 'var(--scrim)' }} />
        <Dialog.Content
          className="fixed inset-y-0 right-0 z-50 grid w-[min(42rem,100vw)] grid-rows-[auto_minmax(0,1fr)] border-l border-scribe"
          style={{ background: 'var(--color-panel-2)', boxShadow: 'var(--shadow-float)' }}
        >
          <header className="flex items-start gap-4 border-b border-scribe-2 px-5 py-4">
            <div className="min-w-0">
              <Dialog.Title className="display text-[1.0625rem] leading-snug">{opened?.label}</Dialog.Title>
              <Dialog.Description className="mt-1 text-[0.75rem] text-ink-3">
                {opened && labelOf(opened.aspect)}, {entity.name}, snapshot {snapshot.ordinal}
              </Dialog.Description>
            </div>
            <Dialog.Close
              aria-label="Close"
              title="Close"
              className="ml-auto mt-1 grid h-5 w-5 shrink-0 place-items-center text-ink-3 transition-colors hover:text-ink"
            >
              <CrossMark size={10} />
            </Dialog.Close>
          </header>

          <div className="overflow-y-auto px-5 pb-8 pt-4">
            {error && <Failure title="Query failed" error={error} />}

            {noise ? (
              <p className="max-w-[60ch] text-[0.8125rem] leading-relaxed text-ink-2">
                These notes shared their wording with no other note in this snapshot. Noise holds rare requests and
                patterns that have not recurred yet.
              </p>
            ) : detail && !theme ? (
              <p className="text-[0.8125rem] text-ink-2">This theme is not in snapshot {snapshot.ordinal}.</p>
            ) : (
              theme && (
                <div>
                  <dl className="grid grid-cols-[auto_1fr] gap-x-5 gap-y-1.5 text-[0.8125rem]">
                    <dt className="legend self-baseline text-ink-3">Runs</dt>
                    <dd className="data">
                      {theme.trace_count} of {detail.snapshot.trace_count} in this window
                    </dd>
                    <dt className="legend self-baseline text-ink-3">Share</dt>
                    <dd className="data">
                      {share(theme.coverage)} of {detail.snapshot.note_count} notes
                    </dd>
                    <dt className="legend self-baseline text-ink-3">State</dt>
                    <dd className="text-ink-2">{STATE_WORD[theme.state]}</dd>
                    <dt className="legend self-baseline text-ink-3">Label from</dt>
                    <dd>
                      <button type="button" className="link" onClick={() => onOpenTrace(theme.label_trace_id)}>
                        open the run behind the label
                      </button>
                    </dd>
                  </dl>
                </div>
              )
            )}

            {!noise && (
              <section className="mt-7">
                <h3 className="legend text-ink-3">History</h3>
                {history === null ? (
                  !error && <Empty>Reading…</Empty>
                ) : (
                  <HistoryView history={history} />
                )}
              </section>
            )}

            <section className="mt-7">
              <h3 className="legend text-ink-3">{noise ? 'Notes' : 'Notes behind it, most alike first'}</h3>
              {examples === null ? (
                !error && <Empty>Reading…</Empty>
              ) : examples.length === 0 ? (
                <Empty>No note in this snapshot.</Empty>
              ) : (
                <ul className="mt-2 divide-y divide-scribe-2 border-y border-scribe-2">
                  {examples.map((x) => (
                    <li key={x.note_id} className="flex items-baseline gap-4 py-2">
                      <p className="min-w-0 flex-1 text-[0.8125rem] leading-snug text-ink">{x.text}</p>
                      <span className="data shrink-0 text-[0.6875rem] text-ink-3">{when(x.trace_started_at)}</span>
                      <button
                        type="button"
                        className="link shrink-0 text-[0.8125rem]"
                        onClick={() => onOpenTrace(x.trace_id)}
                        title={`Open run ${x.trace_id}`}
                      >
                        open run
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </section>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function HistoryView({ history }: { history: ThemeHistory }) {
  const points = history.points
  if (points.length === 0) return <Empty>No stored snapshot holds this theme.</Empty>
  const lineage = (p: ThemeHistory['points'][number]) =>
    [
      p.split_from.length ? `split from ${p.split_from.length}` : '',
      p.split_into.length ? `split into ${p.split_into.length}` : '',
      p.merged_from.length ? `merged from ${p.merged_from.length}` : '',
      p.merged_into.length ? `merged into ${p.merged_into.length}` : '',
    ]
      .filter(Boolean)
      .join(', ')
  return (
    <div className="mt-2">
      {points.length > 1 && <Sparkline series={points.map((p) => (p.state === 'death' ? null : p.coverage))} width={240} height={44} />}
      <table className="mt-3 w-full text-left text-[0.8125rem]">
        <thead className="legend text-ink-3">
          <tr>
            <th className="py-1.5 pr-3">Snapshot</th>
            <th className="py-1.5 pr-3">State</th>
            <th className="py-1.5 pr-3 text-right">Runs</th>
            <th className="py-1.5 pr-3 text-right">Share</th>
          </tr>
        </thead>
        <tbody>
          {points.map((p) => (
            <tr key={p.snapshot_id} className="border-t border-scribe-2 align-top">
              <td className="data whitespace-nowrap py-1.5 pr-3 text-ink-2">{when(p.cutoff_at)}</td>
              <td className="py-1.5 pr-3 text-ink-2">
                {p.state}
                {lineage(p) && <span className="block text-[0.75rem] text-ink-3">{lineage(p)}</span>}
              </td>
              <td className="data py-1.5 pr-3 text-right">{p.trace_count}</td>
              <td className="data py-1.5 pr-3 text-right">{share(p.coverage)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
