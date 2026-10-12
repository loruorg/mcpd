/**
 * The property filters on the run list.
 *
 * The daemon declares twenty-one query parameters on `GET /v1/api/traces` and
 * this interface used to send six of them. Every dimension below is one of those
 * parameters: nothing here filters in the browser, so what the list shows is
 * always what the daemon matched, and a count is a count of the whole projection
 * rather than of the page that happened to be loaded.
 *
 * Where a dimension has a discovery facet, the field offers the values the
 * daemon has actually seen in this project. Where it has none, the field is a
 * plain text field, which is the honest form: `session_id` takes any string the
 * producer sent, and pretending otherwise by offering a list built from the rows
 * on screen would teach that a session missing from that list does not exist.
 */

import { useMemo, useState } from 'react'
import type { Facet } from '@/generated/types'
import { ALL_SPAN_KIND } from '@/generated/types'
import { CrossMark, FieldGroup, Menu, useDiscovery, ValueField } from './list-ui'
import type { Discovered, MenuItem } from './list-ui'

/** One filterable dimension, and how its value is asked for. */
export interface FilterDef {
  /** The query parameter it sets, which is also its key in the filter map. */
  id: string
  label: string
  /** What it matches, exactly, on the control's title. */
  help: string
  /** The discovery facet that lists its values, where the daemon keeps one. */
  facet?: Facet
  /** A closed set the daemon defines, offered instead of discovery. */
  options?: readonly string[]
  numeric?: boolean
  /**
   * What the number is in, shown beside the field.
   *
   * A bare `100` in a box labelled `slower than` is a figure whose unit the
   * reader has to guess, and guessing wrong by a factor of a thousand is the
   * easy mistake here.
   */
  unit?: string
  /** Several values at once, sent comma separated, as the daemon reads them. */
  multi?: boolean
  placeholder?: string
  width?: string
}

/**
 * The dimensions, in the order they appear in the add menu.
 *
 * Two facets the daemon can discover are missing on purpose. `service_name` and
 * `entity_kind` are answered by `GET /v1/api/discovery`, but `GET /v1/api/traces`
 * takes no parameter for either, so a control for them would be a control that
 * changes nothing. Service is a column instead.
 */
export const FILTERS: FilterDef[] = [
  {
    id: 'status',
    label: 'outcome',
    help: 'The run’s outcome. A run is an error when any step in it failed.',
    options: ['ok', 'error', 'unset', 'cancelled'],
    width: 'w-[5rem]',
  },
  {
    id: 'name',
    label: 'run name',
    help: 'The root step’s name, matched exactly. The values offered are the entity names this project has recorded, which is what a root step is usually named after.',
    facet: 'entity_name',
    width: 'w-[9rem]',
  },
  {
    id: 'kind',
    label: 'kind',
    help: 'The root step’s kind, matched exactly. The list is every kind the spec declares.',
    options: ALL_SPAN_KIND,
    width: 'w-[8rem]',
  },
  {
    id: 'tags',
    label: 'tags',
    help: 'Runs carrying every one of these tags.',
    facet: 'tag',
    multi: true,
    width: 'w-[6rem]',
  },
  {
    id: 'environment',
    label: 'environment',
    help: 'Runs from one deployment environment.',
    facet: 'environment',
    width: 'w-[7rem]',
  },
  {
    id: 'model',
    label: 'model',
    help: 'Runs in which a step called this model.',
    width: 'w-[9rem]',
  },
  {
    id: 'session_id',
    label: 'session',
    help: 'Every run in one conversation.',
    width: 'w-[9rem]',
  },
  {
    id: 'thread_id',
    label: 'thread',
    help: 'Every run in one thread.',
    width: 'w-[9rem]',
  },
  {
    id: 'user_id',
    label: 'user',
    help: 'Every run for one end user. Stored ids are hashed and your value is hashed the same way before matching, so pass the plain id you sent.',
    width: 'w-[9rem]',
  },
  {
    id: 'min_duration_ms',
    label: 'slower than',
    help: 'Only runs at least this slow, in milliseconds.',
    numeric: true,
    unit: 'ms',
    placeholder: '250',
    width: 'w-[4rem]',
  },
  {
    id: 'min_cost',
    label: 'dearer than',
    help: 'Only runs that cost at least this much.',
    numeric: true,
    unit: 'in the run’s own currency',
    placeholder: '0.01',
    width: 'w-[4rem]',
  },
  {
    id: 'has_score',
    label: 'scored by',
    help: 'Only runs carrying a score from this scorer. Add a ceiling to find the bad answers.',
    width: 'w-[8rem]',
  },
]

export const FILTER_BY_ID = new Map(FILTERS.map((f) => [f.id, f]))

/** The applied filters: a dimension id to the value it is set to. */
export type FilterValues = Record<string, string>

/**
 * `score_below` has no control of its own.
 *
 * The daemon reads it only alongside `has_score`, so it is the second field
 * inside that filter rather than a thirteenth dimension that silently does
 * nothing on its own.
 */
const CEILING = 'score_below'

/** The filters as query parameters, dropping every empty one. */
export function toParams(v: FilterValues): Record<string, string | number | undefined> {
  const out: Record<string, string | number | undefined> = {}
  for (const [k, raw] of Object.entries(v)) {
    const value = raw.trim()
    if (!value) continue
    if (k === 'min_duration_ms' || k === 'min_cost' || k === CEILING) {
      const n = Number(value)
      if (Number.isFinite(n)) out[k] = n
      continue
    }
    out[k] = value
  }
  // A ceiling with no scorer matches nothing the daemon can reason about, so it
  // is not sent on its own.
  if (out[CEILING] !== undefined && !out.has_score) delete out[CEILING]
  return out
}

/**
 * Which dimensions are on the bar.
 *
 * A dimension is on the bar as soon as it is added, before it has a value: that
 * is what puts the empty field on screen with the cursor in it. Whether it is
 * actually FILTERING is a different question, and `applied` answers that one.
 */
export function onBar(v: FilterValues): string[] {
  return FILTERS.filter((f) => v[f.id] !== undefined).map((f) => f.id)
}

/** Which dimensions are narrowing the list. `score_below` belongs to `has_score`. */
export function applied(v: FilterValues): string[] {
  return FILTERS.filter((f) => (v[f.id] ?? '').trim()).map((f) => f.id)
}

export function anyApplied(v: FilterValues): boolean {
  return applied(v).length > 0
}

/** The filters as a phrase, for saying what came back empty. */
export function filterPhrase(v: FilterValues): string {
  const parts = applied(v).map((id) => `${FILTER_BY_ID.get(id)?.label ?? id} ${v[id].trim()}`)
  if (!parts.length) return ''
  if (parts.length === 1) return parts[0]
  return `${parts.slice(0, -1).join(', ')} and ${parts[parts.length - 1]}`
}

/* ------------------------------------------------------------------ bar --- */

/**
 * The applied filters, each editable where it stands and each removable on its
 * own, with the dimensions not yet applied behind one menu.
 *
 * The bar holds only what is set. A rail of twelve empty fields is a rail nobody
 * reads; a row of three settings is a sentence about what you are looking at.
 */
export function FilterBar({
  values,
  onChange,
  endpoint,
  project,
  children,
}: {
  values: FilterValues
  onChange: (v: FilterValues) => void
  endpoint: string
  project: string
  /** The time range and anything else that belongs on this band. */
  children?: React.ReactNode
}) {
  const on = onBar(values)
  const off = FILTERS.filter((f) => !on.includes(f.id))

  const set = (id: string, value: string) => onChange({ ...values, [id]: value })
  const remove = (id: string) => {
    const next = { ...values }
    delete next[id]
    if (id === 'has_score') delete next[CEILING]
    onChange(next)
  }

  const add: MenuItem[] = off.map((f) => ({
    id: f.id,
    label: f.label,
    title: f.help,
    // An empty string applies the filter and leaves it unset, which is what puts
    // the field on the bar with the cursor in it.
    onSelect: () => onChange({ ...values, [f.id]: '' }),
  }))

  return (
    <div className="flex flex-wrap items-center gap-2 border-b border-scribe-2 bg-panel-1 px-4 py-2">
      {children}

      {on.map((id) => (
        <Applied
          key={id}
          def={FILTER_BY_ID.get(id)!}
          values={values}
          onSet={set}
          onRemove={() => remove(id)}
          endpoint={endpoint}
          project={project}
        />
      ))}

      <Menu
        label="Add a filter"
        items={add}
        empty="Every dimension the daemon filters on is already on the bar."
        trigger={
          <button
            type="button"
            title="Filter on another dimension"
            className="legend panel-raised shrink-0 px-2 py-1 text-[0.5625rem] text-ink-3 transition-colors hover:bg-panel-3 hover:text-ink data-[state=open]:bg-panel-3 data-[state=open]:text-ink"
          >
            + filter
          </button>
        }
      />

      {on.length > 0 && (
        <button
          type="button"
          onClick={() => onChange({})}
          title="Remove every filter"
          className="legend shrink-0 px-1 py-1 text-[0.5625rem] text-ink-3 transition-colors hover:text-oxide"
        >
          clear all
        </button>
      )}
    </div>
  )
}

function Applied({
  def,
  values,
  onSet,
  onRemove,
  endpoint,
  project,
}: {
  def: FilterDef
  values: FilterValues
  onSet: (id: string, v: string) => void
  onRemove: () => void
  endpoint: string
  project: string
}) {
  const value = values[def.id] ?? ''
  const [typed, setTyped] = useState('')

  // Only a field with a facet asks the daemon, and only while it is on the bar.
  // The prefix is what has been typed since the field was last committed, so
  // the offered values narrow as you type and go back to the full list when the
  // field is emptied.
  const found = useDiscovery(endpoint, project, def.facet, typed, !!def.facet)
  const options: Discovered | undefined = def.options
    ? { values: [...def.options], state: 'ready', note: null }
    : undefined
  const source = def.facet ? found : options

  if (def.multi) {
    const chosen = value.split(',').map((s) => s.trim()).filter(Boolean)
    return (
      <FieldGroup label={def.label} title={def.help} onRemove={onRemove}>
        {chosen.map((tag) => (
          <span key={tag} className="flex shrink-0 items-center gap-1">
            <span className="data text-[0.8125rem] text-ink">{tag}</span>
            <button
              type="button"
              onClick={() => onSet(def.id, chosen.filter((t) => t !== tag).join(','))}
              aria-label={`Remove the tag ${tag}`}
              title={`Remove the tag ${tag}`}
              className="grid h-4 w-4 place-items-center text-ink-3 transition-colors hover:text-oxide"
            >
              <CrossMark size={7} />
            </button>
          </span>
        ))}
        <ValueField
          value=""
          clearAfter
          onCommit={(v) => {
            const t = v.trim()
            if (!t || chosen.includes(t)) return
            onSet(def.id, [...chosen, t].join(','))
            setTyped('')
          }}
          onTyping={setTyped}
          label={`Add a ${def.label} value`}
          placeholder={chosen.length ? 'and' : 'tag'}
          discovery={source}
          width={def.width ?? 'w-[6rem]'}
          autoFocus={!chosen.length}
        />
      </FieldGroup>
    )
  }

  return (
    <FieldGroup label={def.label} title={def.help} onRemove={onRemove}>
      <ValueField
        value={value}
        onCommit={(v) => onSet(def.id, v)}
        onTyping={setTyped}
        label={def.label}
        placeholder={def.placeholder ?? 'any'}
        discovery={source}
        width={def.width}
        numeric={def.numeric}
        autoFocus={!value}
      />
      {def.unit && (
        <span
          className="shrink-0 text-[0.75rem] text-ink-3"
          title={def.id === 'min_cost' ? 'The currency each run was priced in' : undefined}
        >
          {def.id === 'min_cost' ? 'or more' : def.unit}
        </span>
      )}
      {def.id === 'has_score' && (
        <>
          <span className="legend shrink-0 text-[0.5625rem] text-ink-3">under</span>
          <ValueField
            value={values[CEILING] ?? ''}
            onCommit={(v) => onSet(CEILING, v)}
            label="Score ceiling"
            placeholder="0.5"
            numeric
            width="w-[3.25rem]"
          />
        </>
      )}
    </FieldGroup>
  )
}
