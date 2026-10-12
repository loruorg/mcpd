/**
 * Which slice of time the run list is reading.
 *
 * `from` and `to` are two of the twenty-one parameters `GET /v1/api/traces`
 * accepts, and until now nothing in this interface set either of them, so the
 * list always answered from the whole retention window. A time range is the
 * filter an operator reaches for first: something broke this afternoon, not in
 * general.
 *
 * A preset is held as an id and resolved to an instant at the moment a query is
 * built, never stored as a pair of timestamps. Storing the resolved bounds is
 * what makes "the last hour" quietly mean the hour in which the tab was opened.
 */

import { useState } from 'react'
import { FieldGroup } from './list-ui'

export interface Preset {
  id: string
  label: string
  /** Zero means no lower bound, which is not the same as a very long window. */
  seconds: number
}

export const PRESETS: Preset[] = [
  { id: '1h', label: 'last hour', seconds: 3_600 },
  { id: '6h', label: 'last 6 hours', seconds: 21_600 },
  { id: '24h', label: 'last 24 hours', seconds: 86_400 },
  { id: '3d', label: 'last 3 days', seconds: 259_200 },
  { id: '7d', label: 'last 7 days', seconds: 604_800 },
  { id: '14d', label: 'last 14 days', seconds: 1_209_600 },
  { id: '30d', label: 'last 30 days', seconds: 2_592_000 },
  { id: 'all', label: 'everything held', seconds: 0 },
]

/** A custom range is `custom`, plus the two bounds it was given. */
export interface Range {
  preset: string
  /** Local `datetime-local` strings while custom, empty otherwise. */
  from: string
  to: string
}

export const DEFAULT_RANGE: Range = { preset: 'all', from: '', to: '' }

export function presetOf(id: string): Preset | undefined {
  return PRESETS.find((p) => p.id === id)
}

/** A local `datetime-local` value as an instant, or undefined when unusable. */
function localIso(v: string): string | undefined {
  if (!v) return undefined
  const d = new Date(v)
  return Number.isNaN(d.getTime()) ? undefined : d.toISOString()
}

/** The `from` and `to` this range means, resolved now. */
export function rangeParams(r: Range): { from?: string; to?: string } {
  if (r.preset === 'custom') return { from: localIso(r.from), to: localIso(r.to) }
  const p = presetOf(r.preset)
  if (!p || !p.seconds) return {}
  return { from: new Date(Date.now() - p.seconds * 1000).toISOString() }
}

/** Whether this range bounds anything at all. */
export function rangeBounded(r: Range): boolean {
  const { from, to } = rangeParams(r)
  return Boolean(from || to)
}

/** The range as a phrase, for a sentence about what came back empty. */
export function rangePhrase(r: Range): string {
  if (r.preset === 'custom') {
    const { from, to } = rangeParams(r)
    if (from && to) return 'in the range you set'
    if (from) return 'since the time you set'
    if (to) return 'before the time you set'
    return 'at any time'
  }
  const p = presetOf(r.preset)
  return p && p.seconds ? `in the ${p.label}` : 'at any time'
}

/**
 * The picker: the presets as a select, and two instants when the range is custom.
 *
 * The custom bounds are native `datetime-local` fields. A hand-built calendar
 * would be a worse copy of one the browser already ships, tested, localised and
 * reachable from the keyboard.
 */
export function RangePicker({ range, onChange }: { range: Range; onChange: (r: Range) => void }) {
  // Seeded from the resolved preset the first time custom is chosen, so the two
  // fields open on the window the reader was already looking at rather than on
  // 1970.
  const [seeded, setSeeded] = useState(false)

  const choose = (id: string) => {
    if (id !== 'custom') {
      onChange({ preset: id, from: '', to: '' })
      return
    }
    if (seeded && (range.from || range.to)) {
      onChange({ ...range, preset: 'custom' })
      return
    }
    setSeeded(true)
    const p = presetOf(range.preset)
    const from = p?.seconds ? new Date(Date.now() - p.seconds * 1000) : new Date(Date.now() - 86_400_000)
    onChange({ preset: 'custom', from: forInput(from), to: forInput(new Date()) })
  }

  return (
    <div className="flex flex-wrap items-center gap-2">
      <select
        value={range.preset}
        onChange={(e) => choose(e.target.value)}
        aria-label="Time range"
        title="Which slice of time the list reads"
        className="field"
      >
        {PRESETS.map((p) => (
          <option key={p.id} value={p.id}>
            {p.label}
          </option>
        ))}
        <option value="custom">a range of your own</option>
      </select>

      {range.preset === 'custom' && (
        <>
          <FieldGroup label="from" title="The earliest start time to include, inclusive">
            <input
              type="datetime-local"
              step="1"
              value={range.from}
              onChange={(e) => onChange({ ...range, from: e.target.value })}
              aria-label="Range start"
              className="data bg-transparent text-[0.8125rem] text-ink outline-none"
            />
          </FieldGroup>
          <FieldGroup label="to" title="The latest start time to include, exclusive">
            <input
              type="datetime-local"
              step="1"
              value={range.to}
              onChange={(e) => onChange({ ...range, to: e.target.value })}
              aria-label="Range end"
              className="data bg-transparent text-[0.8125rem] text-ink outline-none"
            />
          </FieldGroup>
        </>
      )}
    </div>
  )
}

/** A date as a `datetime-local` field wants it: local time, no zone, to the second. */
function forInput(d: Date): string {
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`
}
