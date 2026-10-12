/**
 * The pieces every operator screen needs before it can show anything.
 *
 * The trace explorer shipped first and set the shape of all of this, so the
 * endpoint rule, the connection states and the "no daemon" panel are imported
 * from it rather than written again here. A second copy would be free to drift,
 * and the first thing to drift would be the fallback that finds a daemon on the
 * default port when the page is served from somewhere else.
 */

import { useCallback, useEffect, useState } from 'react'
import { api, DaemonError, DaemonUnreachable, findDaemon } from '@/lib/api'
import type { HealthStatus } from '@/generated/types'
import type { Conn } from './Explorer'

/**
 * Probe a daemon and read the projects it holds.
 *
 * `onEndpoint` must be stable across renders, because it is a dependency of the
 * probe and an inline function would re-probe the daemon on every render. A
 * `useState` setter is what this expects.
 */
export function useConnection(endpoint: string, onEndpoint: (v: string) => void) {
  const [conn, setConn] = useState<Conn>({ state: 'connecting' })
  const [projects, setProjects] = useState<string[]>([])

  const probe = useCallback(
    async (ep: string) => {
      setConn({ state: 'connecting' })
      try {
        // The page's own origin may be a documentation host rather than a
        // daemon. `findDaemon` tries the default port before giving up, the
        // same order the explorer uses, and names the endpoint that answered.
        const { endpoint: found, health } = await findDaemon(ep)
        if (found !== ep) {
          // Re-probed once the shell adopts it; the effect below runs again.
          onEndpoint(found)
          return
        }
        setConn({ state: 'connected', health })
        try {
          localStorage.setItem('mcpd:endpoint', ep)
        } catch {
          /* Private browsing. The endpoint still applies to this session. */
        }
        setProjects(await api.projects({ endpoint: ep }).catch(() => [] as string[]))
      } catch (e) {
        if (e instanceof DaemonUnreachable) {
          // When the page's origin was never a daemon, the honest report names
          // the default port, which is where the reader can start one.
          if (e.endpoint !== ep) onEndpoint(e.endpoint)
          setConn({ state: 'absent', endpoint: e.endpoint })
        } else if (e instanceof DaemonError) setConn({ state: 'refused', message: e.message })
        else setConn({ state: 'refused', message: String(e) })
      }
    },
    [onEndpoint],
  )

  useEffect(() => {
    void probe(endpoint)
  }, [endpoint, probe])

  const retry = useCallback(() => void probe(endpoint), [probe, endpoint])
  return { conn, projects, retry }
}

/** The daemon tile that opens every screen's rail. */
export function DaemonTile({ health, endpoint }: { health: HealthStatus; endpoint: string }) {
  return (
    <span
      className="annunciator py-1.5"
      data-lit="olive"
      title={`${endpoint} · mcpd ${health.version}`}
    >
      <span className="inline-block h-1.5 w-1.5 rounded-full bg-current opacity-75" />
      mcpd {health.version}
    </span>
  )
}

/** Shown only when the daemon holds more than one project, as in the explorer. */
export function ProjectPicker({
  projects,
  project,
  onProject,
}: {
  projects: string[]
  project: string
  onProject: (v: string) => void
}) {
  if (projects.length < 2) return null
  return (
    <select
      value={project}
      onChange={(e) => onProject(e.target.value)}
      aria-label="Project"
      className="field"
    >
      {projects.map((x) => (
        <option key={x} value={x}>
          {x}
        </option>
      ))}
    </select>
  )
}

export interface TimeWindow {
  id: string
  label: string
  /** Zero means no bound at all, which is not the same as a very long window. */
  seconds: number
  /** The bucket width to ask for, where the screen draws a series. */
  bucket?: string
  /** The same width in seconds, so a chart can lay out the buckets that came
      back on a real time axis instead of side by side. */
  bucketSeconds?: number
}

export const LOG_WINDOWS: TimeWindow[] = [
  { id: '1h', label: 'last hour', seconds: 3600 },
  { id: '24h', label: 'last 24 hours', seconds: 86_400 },
  { id: '7d', label: 'last 7 days', seconds: 604_800 },
  { id: 'all', label: 'everything held', seconds: 0 },
]

/* Windows for spend, each with a bucket that lands between 24 and 30 columns.
   A bucket the daemon cannot parse is a 400, so these use its own units. */
export const COST_WINDOWS: TimeWindow[] = [
  { id: '24h', label: 'last 24 hours', seconds: 86_400, bucket: '1h', bucketSeconds: 3_600 },
  { id: '7d', label: 'last 7 days', seconds: 604_800, bucket: '6h', bucketSeconds: 21_600 },
  { id: '30d', label: 'last 30 days', seconds: 2_592_000, bucket: '1d', bucketSeconds: 86_400 },
]

/** The start of a window, or undefined where the window has no lower bound. */
export function since(seconds: number): string | undefined {
  if (!seconds) return undefined
  return new Date(Date.now() - seconds * 1000).toISOString()
}

export function nowIso(): string {
  return new Date().toISOString()
}

/**
 * What mcpd can group by.
 *
 * This list is the daemon's allowlist, which it names in the error it returns
 * for anything else: model, provider, entity_name, environment, status,
 * session_id, user_id, service_name, name. Adding a tenth here produces a 400,
 * so the two lists have to be changed together.
 *
 * `unit` is not cosmetic. A grouping resolved against the span table counts
 * steps, and one resolved against the trace table counts runs, so a single
 * "runs" heading over that column would be wrong for half of these.
 */
export interface GroupDimension {
  id: string
  label: string
  unit: 'step' | 'run'
}

export const COST_GROUPS: GroupDimension[] = [
  { id: 'model', label: 'model', unit: 'step' },
  { id: 'provider', label: 'provider', unit: 'step' },
  { id: 'entity_name', label: 'step name', unit: 'step' },
  { id: 'name', label: 'run name', unit: 'run' },
  { id: 'environment', label: 'environment', unit: 'run' },
  { id: 'service_name', label: 'service', unit: 'run' },
  { id: 'session_id', label: 'session', unit: 'run' },
  { id: 'user_id', label: 'user', unit: 'run' },
  { id: 'status', label: 'outcome', unit: 'run' },
]

/** A clock time for a log line. The date is carried by the day divider. */
export function clock(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return 'unreadable time'
  return d.toLocaleTimeString(undefined, { hour12: false })
}

export function day(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return 'unreadable date'
  return d.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' })
}

/** A window as a sentence, for saying which window came back empty. */
export function windowPhrase(w: TimeWindow): string {
  return w.seconds ? `in the ${w.label}` : 'at any time'
}

/** Severity order, lowest first. Matches the daemon's `LogLevel`. */
export const LEVEL_TONE: Record<string, string> = {
  trace: 'text-ink-3',
  debug: 'text-ink-3',
  info: 'text-ink-2',
  warn: 'text-amber',
  error: 'text-oxide',
  fatal: 'text-oxide',
}
