/**
 * What the run list shows, and what it remembers.
 *
 * Two separate things live here on purpose. The COLUMN SET is a description of
 * the trace projection: every column is one field the daemon already sends, and
 * a column reads that field and nothing else, so nothing in the list can show a
 * figure the daemon did not produce. The PREFERENCES are the reader's own, held
 * in this browser only.
 *
 * The persisted shape is versioned. A stored set written by an older build is
 * discarded rather than migrated, because the alternative is a column list that
 * half-applies and a reader who cannot tell which half. Every read and write is
 * guarded: `localStorage` throws outright in some privacy modes, and a list that
 * will not render because a preference could not be read is a worse failure than
 * a list that renders with the defaults.
 */

import type { Trace } from '@/generated/types'
import { ago, duration, money, tokens, traceCostBlank } from '@/lib/api'

/* ---------------------------------------------------------------- time --- */

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

const pad = (n: number) => String(n).padStart(2, '0')

/**
 * One created cell: the date and the clock time together.
 *
 * Built by hand rather than through `toLocaleString`, because the list is a
 * table of times read down a column and a locale format that switches between
 * `9:32` and `09:32`, or drops the seconds, makes that column ragged. The full
 * timestamp the daemon sent is on the cell's title, so nothing is lost.
 */
export function stamp(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return `${pad(d.getDate())} ${MONTHS[d.getMonth()]} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}

/* ------------------------------------------------------------- columns --- */

/** What one cell holds: a string, and how to read it. */
export interface Cell {
  text: string
  /** A colour class where the value carries a signal. Ink otherwise. */
  tone?: string
  /** The whole fact, where the cell shows an abbreviation of it. */
  title?: string
  /** Figures are set in the measurement face; prose and names are not. */
  prose?: boolean
}

export interface ColumnDef {
  id: string
  /** The header, in the panel's legend lettering. */
  label: string
  /** What the column means, on the header's title. */
  help: string
  /**
   * The narrowest the column may become, in rem. Cells ellipsis at this width
   * rather than pushing the table sideways, and the table as a whole only
   * scrolls horizontally when the sum of these will not fit.
   */
  min: number
  /** The widest it may become, in rem. The run name takes what is left over. */
  max: number
  /** Figures read down a column and are right-aligned; names are not. */
  end?: boolean
  /** The three the list cannot be read without. They have no switch. */
  fixed?: boolean
  cell: (t: Trace) => Cell
}

/** The mean of the first scorer, and how many others there are. */
function scores(t: Trace): [string, number][] {
  if (!t.score_summary || typeof t.score_summary !== 'object') return []
  return Object.entries(t.score_summary as Record<string, unknown>).filter(
    (e): e is [string, number] => typeof e[1] === 'number',
  )
}

function counts(v: unknown): [string, number][] {
  if (!v || typeof v !== 'object') return []
  return Object.entries(v as Record<string, unknown>).filter(
    (e): e is [string, number] => typeof e[1] === 'number',
  )
}

const NOTHING: Cell = { text: '', title: 'The daemon reported nothing here.' }

/**
 * A value the daemon did not send.
 *
 * A middle dot rather than a dash or an empty cell: a blank cell reads as a
 * layout mistake and a zero reads as a measurement. The dot is the quietest ink
 * this palette allows for text, and the reason is on the cell.
 */
function absent(why: string): Cell {
  return { text: '·', tone: 'text-ink-3', title: why }
}

function text(v: string | undefined, why: string): Cell {
  return v ? { text: v, title: v } : absent(why)
}

/**
 * The status word.
 *
 * A word in signal colour rather than a badge. A tinted capsule around every
 * outcome in a list of five hundred rows is five hundred capsules, and the
 * colour alone already carries the meaning at a glance.
 */
function statusCell(t: Trace): Cell {
  if (t.status === 'error') {
    return {
      text: 'fault',
      tone: 'text-oxide',
      title: `${t.error_count} of ${t.span_count} steps failed`,
    }
  }
  if (!t.is_complete) {
    return { text: 'running', tone: 'text-amber', title: 'A step in this run has not ended yet' }
  }
  if (t.status === 'ok') return { text: 'ok', tone: 'text-olive', title: 'Every step ended cleanly' }
  if (t.status === 'cancelled') {
    return { text: 'cancelled', tone: 'text-ink-2', title: 'The run was cancelled' }
  }
  return { text: 'unset', tone: 'text-ink-3', title: 'The producer set no outcome on the root step' }
}

/**
 * Every column the run list can show.
 *
 * Order here is the order on screen. Three are fixed, which is the smallest set
 * a row still reads as a run: when it started, what it was, and how it ended.
 */
export const COLUMNS: ColumnDef[] = [
  {
    id: 'created',
    label: 'Created',
    help: 'When the earliest step in the run started, in this browser’s time zone.',
    min: 8.5,
    max: 8.5,
    fixed: true,
    cell: (t) => ({ text: stamp(t.start_time), title: t.start_time }),
  },
  {
    id: 'name',
    label: 'Run',
    help: 'The root step’s name.',
    min: 7,
    max: 26,
    fixed: true,
    cell: (t) => ({
      text: t.name,
      title: t.name,
      prose: true,
      tone: t.status === 'error' ? 'text-oxide' : 'text-ink',
    }),
  },
  {
    id: 'status',
    label: 'Status',
    help: 'The run’s outcome. A run is a fault when any step in it failed.',
    min: 3.75,
    max: 4.75,
    fixed: true,
    cell: statusCell,
  },
  {
    id: 'duration',
    label: 'Took',
    help: 'Wall clock across the whole run, from the earliest start to the latest end.',
    min: 3.5,
    max: 4.75,
    end: true,
    cell: (t) =>
      t.duration_ns != null
        ? { text: duration(t.duration_ns), title: `${t.duration_ns} nanoseconds` }
        : absent('This run has not ended, so it has no duration yet.'),
  },
  {
    id: 'steps',
    label: 'Steps',
    help: 'How many spans have arrived for this run.',
    min: 3.25,
    max: 3.75,
    end: true,
    cell: (t) => ({ text: String(t.span_count), title: `${t.span_count} steps` }),
  },
  {
    id: 'cost',
    label: 'Cost',
    help: 'Priced at ingest from the daemon’s price book. Blank where mcpd stored no figure, with the reason on the cell.',
    min: 5,
    max: 5.5,
    end: true,
    cell: (t) => {
      if (t.cost?.total_cost != null) {
        return {
          text: money(t.cost.total_cost, t.cost.currency),
          title: t.cost.is_estimated ? 'Estimated from the price book' : 'As the producer reported it',
        }
      }
      const blank = traceCostBlank(t)
      return blank
        ? { text: blank.short, tone: 'text-ink-3', title: blank.long, prose: true }
        : NOTHING
    },
  },
  {
    id: 'age',
    label: 'Age',
    help: 'How long ago the run started.',
    min: 4,
    max: 4.5,
    end: true,
    cell: (t) => ({ text: ago(t.start_time), title: t.start_time, prose: true }),
  },
  {
    id: 'kind',
    label: 'Kind',
    help: 'The root step’s kind, which is what the run is: an agent run, a workflow run, a single model call.',
    min: 5,
    max: 7,
    cell: (t) => ({ text: t.kind, title: t.kind }),
  },
  {
    id: 'in',
    label: 'In',
    help: 'Input tokens summed across every model step in the run.',
    min: 3.5,
    max: 4.5,
    end: true,
    cell: (t) =>
      t.usage?.input_tokens != null
        ? { text: tokens(t.usage.input_tokens), title: `${t.usage.input_tokens} input tokens` }
        : absent('No step in this run reported an input token count.'),
  },
  {
    id: 'out',
    label: 'Out',
    help: 'Output tokens summed across every model step in the run.',
    min: 3.5,
    max: 4.5,
    end: true,
    cell: (t) =>
      t.usage?.output_tokens != null
        ? { text: tokens(t.usage.output_tokens), title: `${t.usage.output_tokens} output tokens` }
        : absent('No step in this run reported an output token count.'),
  },
  {
    id: 'models',
    label: 'Models',
    help: 'How many model calls the run made.',
    min: 3.6,
    max: 4.25,
    end: true,
    cell: (t) => ({ text: String(t.model_count), title: `${t.model_count} model calls` }),
  },
  {
    id: 'tools',
    label: 'Tools',
    help: 'How many tool calls the run made.',
    min: 3.6,
    max: 4.25,
    end: true,
    cell: (t) => ({ text: String(t.tool_count), title: `${t.tool_count} tool calls` }),
  },
  {
    id: 'errors',
    label: 'Faults',
    help: 'How many steps in the run failed.',
    min: 3.6,
    max: 4.25,
    end: true,
    cell: (t) => ({
      text: String(t.error_count),
      tone: t.error_count > 0 ? 'text-oxide' : undefined,
      title: `${t.error_count} failed steps`,
    }),
  },
  {
    id: 'score',
    label: 'Score',
    help: 'The mean of the first scorer that ran, named on the cell. A run can carry several.',
    min: 3.5,
    max: 4.25,
    end: true,
    cell: (t) => {
      const all = scores(t)
      if (!all.length) return absent('No scorer has recorded a score for this run.')
      const [id, mean] = all[0]
      return {
        text: mean.toFixed(2),
        title: `${id}, mean ${mean}${all.length > 1 ? `, and ${all.length - 1} more scorer${all.length > 2 ? 's' : ''}` : ''}`,
      }
    },
  },
  {
    id: 'feedback',
    label: 'Feedback',
    help: 'Counts per feedback kind, as they were recorded against this run.',
    min: 4.5,
    max: 6,
    cell: (t) => {
      const all = counts(t.feedback_summary)
      if (!all.length) return absent('Nobody has left feedback on this run.')
      return {
        text: all.map(([k, n]) => `${k} ${n}`).join(', '),
        title: all.map(([k, n]) => `${k}: ${n}`).join('\n'),
      }
    },
  },
  {
    id: 'environment',
    label: 'Environment',
    help: 'The deployment the run came from, as the producer set it.',
    min: 6,
    max: 7,
    cell: (t) => text(t.correlation.environment, 'The producer set no environment.'),
  },
  {
    id: 'service',
    label: 'Service',
    help: 'The service that emitted the run.',
    min: 4.5,
    max: 8,
    cell: (t) => text(t.correlation.service_name, 'The producer set no service name.'),
  },
  {
    id: 'release',
    label: 'Release',
    help: 'The release or commit the run came from, for pinning a regression to a deploy.',
    min: 4,
    max: 6,
    cell: (t) => text(t.correlation.release, 'The producer set no release.'),
  },
  {
    id: 'session',
    label: 'Session',
    help: 'The conversation this run belongs to.',
    min: 4.5,
    max: 8,
    cell: (t) => text(t.correlation.session_id, 'This run is not part of a session.'),
  },
  {
    id: 'thread',
    label: 'Thread',
    help: 'The thread within the session.',
    min: 4.5,
    max: 8,
    cell: (t) => text(t.correlation.thread_id, 'This run is not part of a thread.'),
  },
  {
    id: 'user',
    label: 'User',
    help: 'The end user, hashed at ingest. mcpd never stores who they are.',
    min: 5,
    max: 9,
    cell: (t) => text(t.correlation.user_id, 'This run carries no end user.'),
  },
  {
    id: 'tags',
    label: 'Tags',
    help: 'Tags from the root step.',
    min: 4.5,
    max: 9,
    cell: (t) =>
      t.tags?.length
        ? { text: t.tags.join('  '), title: t.tags.join(', '), prose: true }
        : absent('The root step carries no tags.'),
  },
  {
    id: 'trace_id',
    label: 'Trace id',
    help: 'The run’s id, as every other tool and every log line refers to it.',
    min: 6,
    max: 9,
    cell: (t) => ({ text: t.trace_id, title: t.trace_id }),
  },
  {
    id: 'received',
    label: 'Arrived',
    help: 'When the first step of this run reached mcpd. Later than its start time, and by a lot after a backfill.',
    min: 4,
    max: 5,
    end: true,
    cell: (t) => ({ text: ago(t.received_at), title: t.received_at, prose: true }),
  },
]

export const COLUMN_BY_ID = new Map(COLUMNS.map((c) => [c.id, c]))

/** Columns a reader sees before touching anything. */
const DEFAULT_HIDDEN = COLUMNS.filter(
  (c) =>
    !['created', 'name', 'status', 'duration', 'steps', 'models', 'in', 'out', 'cost'].includes(
      c.id,
    ),
).map((c) => c.id)

/* ---------------------------------------------------- metadata columns --- */

/**
 * A column the reader added by naming a key.
 *
 * The key is looked for in the run's correlation first, then in its score means,
 * then in its feedback counts. Those are the three keyed maps a trace row
 * carries, so a key that appears in none of them is a key this projection does
 * not hold, and the cell says so rather than showing a blank that reads as a
 * zero.
 */
export function metaCell(t: Trace, key: string): Cell {
  const corr = (t.correlation as unknown as Record<string, unknown>)[key]
  if (typeof corr === 'string' && corr) return { text: corr, title: `correlation.${key} = ${corr}` }
  const s = scores(t).find(([k]) => k === key)
  if (s) return { text: s[1].toFixed(2), title: `score ${key}, mean ${s[1]}` }
  const f = counts(t.feedback_summary).find(([k]) => k === key)
  if (f) return { text: String(f[1]), title: `feedback ${key}: ${f[1]}` }
  return absent(`This run carries no ${key}.`)
}

/** Every key the runs on screen actually carry, for the column adder. */
export function metaKeys(traces: Trace[]): string[] {
  const seen = new Set<string>()
  for (const t of traces) {
    for (const [k, v] of Object.entries(t.correlation)) {
      // `project` is the same value on every row in the list and would add a
      // column of one repeated word.
      if (k !== 'project' && typeof v === 'string' && v) seen.add(k)
    }
    for (const [k] of scores(t)) seen.add(k)
    for (const [k] of counts(t.feedback_summary)) seen.add(k)
  }
  return [...seen].sort()
}

/* --------------------------------------------------------- preferences --- */

export type Density = 'compact' | 'roomy'

export interface ListPrefs {
  /**
   * Hidden rather than visible, deliberately. A column added to a later build
   * would be invisible to every reader who had ever saved a preference set if
   * this held the visible ones, and nobody would know to go looking for it.
   */
  hidden: string[]
  meta: string[]
  density: Density
  /** How much of the screen the list keeps once a run is open, 0.2 to 0.8. */
  split: number
}

export const DEFAULT_PREFS: ListPrefs = {
  hidden: DEFAULT_HIDDEN,
  meta: [],
  density: 'compact',
  split: 0.42,
}

const KEY = 'mcpd:runs:list'

/**
 * The schema version.
 *
 * Bumped whenever the meaning of a stored field changes. A stored set from a
 * different version is dropped, not migrated: half-applying an old preference
 * set leaves a reader with a column list they did not choose and cannot explain.
 */
export const PREFS_VERSION = 1

export function loadPrefs(): ListPrefs {
  try {
    const raw = localStorage.getItem(KEY)
    if (!raw) return DEFAULT_PREFS
    const v = JSON.parse(raw) as Partial<ListPrefs> & { version?: number }
    if (v.version !== PREFS_VERSION) return DEFAULT_PREFS
    return {
      hidden: Array.isArray(v.hidden) ? v.hidden.filter((x) => typeof x === 'string') : DEFAULT_PREFS.hidden,
      meta: Array.isArray(v.meta) ? v.meta.filter((x) => typeof x === 'string').slice(0, 8) : [],
      density: v.density === 'roomy' ? 'roomy' : 'compact',
      split:
        typeof v.split === 'number' && v.split >= 0.2 && v.split <= 0.8 ? v.split : DEFAULT_PREFS.split,
    }
  } catch {
    // Private browsing, a disabled store, or a value another tool wrote over
    // ours. The list renders on the defaults either way.
    return DEFAULT_PREFS
  }
}

export function savePrefs(p: ListPrefs): void {
  try {
    localStorage.setItem(KEY, JSON.stringify({ version: PREFS_VERSION, ...p }))
  } catch {
    /* The preference applies to this session and is not remembered. */
  }
}

export function clearPrefs(): void {
  try {
    localStorage.removeItem(KEY)
  } catch {
    /* Nothing was stored, so there is nothing to fail at removing. */
  }
}

/** Row heights, in pixels, one per density. Fixed, because the list windows. */
export const ROW_H: Record<Density, number> = { compact: 27, roomy: 34 }

/* ----------------------------------------------------------- the grid --- */

/**
 * The one grid the header and every row share.
 *
 * Declared once and used by both, because a header on its own grid is worse
 * than no header: it invites you to read a figure under a label that is not
 * over it. Every track has a floor it will not shrink below, so a cell
 * ellipsises inside its column instead of the table growing past the panel.
 */
export function gridOf(columns: ColumnDef[], meta: string[]): string {
  const cols = columns.map((c) => `minmax(${c.min}rem, ${c.max}rem)`)
  const extra = meta.map(() => `minmax(${META_MIN}rem, ${META_MAX}rem)`)
  // A last track that claims whatever is left. Without it the run name absorbs
  // every spare pixel, and a six-column list on a wide screen becomes a name at
  // the far left and four figures at the far right with a gulf between them.
  return [...cols, ...extra, 'minmax(0, 1fr)'].join(' ')
}

export const META_MIN = 3.75
export const META_MAX = 7

/** The width below which the table scrolls sideways inside its own container. */
export function minWidthOf(columns: ColumnDef[], meta: string[]): string {
  const rem = columns.reduce((n, c) => n + c.min, 0) + meta.length * META_MIN
  return `${rem}rem`
}

/** The visible columns, in declaration order. */
export function visibleColumns(prefs: ListPrefs): ColumnDef[] {
  const hidden = new Set(prefs.hidden)
  return COLUMNS.filter((c) => c.fixed || !hidden.has(c.id))
}
