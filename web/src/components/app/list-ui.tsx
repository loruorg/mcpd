/**
 * The small controls the run list is built from: a menu, a value picker backed
 * by the daemon's discovery endpoint, and the two marks they use.
 *
 * They live together because they are one vocabulary. The list has a filter bar,
 * a column dialog and a time range, and all three need the same menu on the same
 * surface; a second menu styled a little differently is how one interface quietly
 * becomes two.
 */

import { useEffect, useMemo, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import * as DropdownMenu from '@radix-ui/react-dropdown-menu'
import { api, DaemonError, refusedCapability } from '@/lib/api'
import type { Facet } from '@/generated/types'

/* --------------------------------------------------------------- marks --- */

/**
 * The house cross.
 *
 * The same figure the panel's search field clears with, at the same stroke and
 * the same round cap, so removing a filter and clearing a search read as one
 * gesture rather than two borrowed glyphs.
 */
export function CrossMark({ size = 9 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 9 9" aria-hidden="true">
      <path d="M1 1 L8 8 M8 1 L1 8" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
    </svg>
  )
}

/**
 * The mark on a control that opens a list of values.
 *
 * Not the browser's chevron and not an icon pack's: three strokes drawn at the
 * panel's own weight, reading as a stack of choices rather than an arrow. The
 * middle stroke is short, which is the one detail that makes it this panel's.
 */
export function ChoiceMark() {
  return (
    <svg width="9" height="9" viewBox="0 0 9 9" aria-hidden="true">
      <path
        d="M1 2.1 H8 M1 4.5 H5.2 M1 6.9 H8"
        stroke="currentColor"
        strokeWidth="1.2"
        strokeLinecap="round"
      />
    </svg>
  )
}

/* ---------------------------------------------------------------- menu --- */

export interface MenuItem {
  /** A stable key, and the label when `label` is absent. */
  id: string
  label?: ReactNode
  /** Set for a switch; left undefined for a plain action. */
  checked?: boolean
  disabled?: boolean
  title?: string
  onSelect: () => void
  /** Keeps the menu open, for a list of switches. */
  keepOpen?: boolean
}

/**
 * A menu on Radix, in the panel's material.
 *
 * The primitive is taken for the behaviour nobody should hand-roll: focus into
 * the menu and back to the trigger, Escape and outside press to dismiss, arrow
 * keys and typeahead, and positioning that never opens off the edge of the
 * screen. The look is the panel's: a raised surface, a hairline in its own
 * colour, and the one tight directional shadow this system allows for something
 * genuinely floating.
 */
export function Menu({
  trigger,
  items,
  label,
  align = 'start',
  empty,
}: {
  trigger: ReactNode
  items: MenuItem[]
  label: string
  align?: 'start' | 'end'
  empty?: string
}) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild aria-label={label}>
        {trigger}
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          sideOffset={4}
          align={align}
          collisionPadding={8}
          className="panel-raised z-50 max-h-[min(22rem,70vh)] min-w-[11rem] overflow-y-auto py-1"
          style={{
            background: 'var(--color-panel-3)',
            boxShadow: 'var(--shadow-float)',
          }}
        >
          {items.length === 0 && empty && (
            <p className="max-w-[16rem] px-3 py-2 text-[0.75rem] leading-snug text-ink-3">{empty}</p>
          )}
          {items.map((it) => (
            <DropdownMenu.Item
              key={it.id}
              disabled={it.disabled}
              title={it.title}
              onSelect={(e) => {
                if (it.keepOpen) e.preventDefault()
                it.onSelect()
              }}
              className="flex cursor-pointer items-center gap-2 px-3 py-1.5 text-[0.8125rem] text-ink-2 outline-none data-[disabled]:cursor-default data-[highlighted]:bg-panel-3 data-[disabled]:text-ink-3 data-[highlighted]:text-ink"
            >
              {it.checked !== undefined && (
                // A switch reads by its state, not by a tick from an icon set:
                // the box fills with ink when it is on and is an empty recess
                // when it is off.
                <span
                  aria-hidden="true"
                  className="inline-block h-2 w-2 shrink-0 rounded-[1px] border border-scribe"
                  style={{ background: it.checked ? 'var(--color-ink)' : 'var(--color-well)' }}
                />
              )}
              <span className="min-w-0 flex-1 truncate">{it.label ?? it.id}</span>
            </DropdownMenu.Item>
          ))}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  )
}

/* ----------------------------------------------------------- discovery --- */

export type DiscoveryState = 'idle' | 'reading' | 'ready' | 'unsupported' | 'failed'

export interface Discovered {
  values: string[]
  state: DiscoveryState
  /** The daemon's own sentence, where it refused or failed. */
  note: string | null
}

/**
 * The values one filter dimension takes, from `GET /v1/api/discovery`.
 *
 * The prefix is sent to the daemon rather than filtered here, because the answer
 * is a distinct-value list that can be long and the endpoint exists to be asked
 * on a keystroke.
 *
 * A backend without the capability answers 501 with the capability named. That
 * is not a failure of this control: the field stays a field, and the caller is
 * given the daemon's own sentence to show instead of an empty list, which would
 * read as "this project has no environments".
 */
export function useDiscovery(
  endpoint: string,
  project: string,
  facet: Facet | undefined,
  prefix: string,
  enabled: boolean,
): Discovered {
  const [out, setOut] = useState<Discovered>({ values: [], state: 'idle', note: null })

  useEffect(() => {
    if (!facet || !enabled) {
      setOut({ values: [], state: 'idle', note: null })
      return
    }
    const ac = new AbortController()
    // Debounced, because this runs on a keystroke. 180ms is under the pause
    // between two typed characters and over the time a fast typist takes.
    const t = setTimeout(() => {
      setOut((o) => ({ ...o, state: 'reading' }))
      api
        .discovery(
          { project, facet, prefix: prefix.trim() || undefined, limit: 50 },
          { endpoint, signal: ac.signal },
        )
        .then((r) => setOut({ values: r.values.map((v) => v.value), state: 'ready', note: null }))
        .catch((e: unknown) => {
          if ((e as { name?: string }).name === 'AbortError') return
          const capability = refusedCapability(e)
          setOut({
            values: [],
            state: capability ? 'unsupported' : 'failed',
            note:
              e instanceof DaemonError
                ? e.message
                : 'The daemon could not be asked which values this dimension takes.',
          })
        })
    }, 180)
    return () => {
      clearTimeout(t)
      ac.abort()
    }
  }, [endpoint, project, facet, prefix, enabled])

  return out
}

/* --------------------------------------------------------------- field --- */

/**
 * One labelled setting on the panel: a recess holding its own legend, its value
 * and the controls that change or remove it.
 *
 * The label sits inside the recess rather than above it, so a row of eight
 * settings is one band of controls instead of eight stacked pairs, and every
 * value in the row shares a baseline whatever the label above it says.
 */
export function FieldGroup({
  label,
  title,
  children,
  onRemove,
  removeLabel,
}: {
  label: string
  title?: string
  children: ReactNode
  onRemove?: () => void
  removeLabel?: string
}) {
  return (
    <div className="well flex min-w-0 items-center gap-2 py-1 pl-2.5 pr-1.5" title={title}>
      <span className="legend shrink-0 text-[0.5625rem] text-ink-3">{label}</span>
      {children}
      {onRemove && (
        <button
          type="button"
          onClick={onRemove}
          aria-label={removeLabel ?? `Remove the ${label} filter`}
          title={removeLabel ?? `Remove the ${label} filter`}
          className="grid h-4 w-4 shrink-0 place-items-center text-ink-3 transition-colors hover:text-oxide"
        >
          <CrossMark />
        </button>
      )}
    </div>
  )
}

/**
 * A value field that offers the values the daemon has seen.
 *
 * It is a text field first: anything can be typed, because a filter is only
 * useful for the value you have in your hand, which may have arrived a second
 * ago. The mark beside it opens what the daemon knows, filtered by what has been
 * typed so far.
 */
export function ValueField({
  value,
  onCommit,
  onTyping,
  clearAfter,
  placeholder,
  label,
  discovery,
  width = 'w-[7.5rem]',
  numeric,
  autoFocus,
}: {
  value: string
  onCommit: (v: string) => void
  /** Every keystroke, so the values on offer can be narrowed as you type. */
  onTyping?: (v: string) => void
  /**
   * Empty the field once it has committed, for a field that ADDS a value rather
   * than setting one. Without it the value you added is still sitting in
   * the box, and reads as a fourth tag beside the three you chose.
   */
  clearAfter?: boolean
  placeholder?: string
  label: string
  discovery?: Discovered
  width?: string
  numeric?: boolean
  autoFocus?: boolean
}) {
  const [draft, setDraft] = useState(value)
  const committed = useRef(value)

  // The applied value can move under this field: a link opened with filters in
  // it, or the reader clearing everything. The draft follows it, but only when
  // it is genuinely a different value from the one this field last sent.
  useEffect(() => {
    if (value !== committed.current) {
      committed.current = value
      setDraft(value)
    }
  }, [value])

  const commit = (v: string) => {
    committed.current = clearAfter ? '' : v
    if (clearAfter) setDraft('')
    onCommit(v)
  }

  const items = useMemo<MenuItem[]>(
    () =>
      (discovery?.values ?? []).map((v) => ({
        id: v,
        label: v,
        onSelect: () => {
          setDraft(v)
          commit(v)
        },
      })),
    // `commit` is stable enough: it closes over the current onCommit, which the
    // parent redeclares on every render, and rebuilding this list is cheap.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [discovery?.values, onCommit],
  )

  return (
    <span className="flex min-w-0 items-center gap-1">
      <input
        value={draft}
        autoFocus={autoFocus}
        onChange={(e) => {
          setDraft(e.target.value)
          onTyping?.(e.target.value)
        }}
        onBlur={() => draft !== committed.current && commit(draft)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault()
            commit(draft)
          }
          if (e.key === 'Escape') {
            setDraft(committed.current)
            e.currentTarget.blur()
          }
        }}
        inputMode={numeric ? 'decimal' : undefined}
        aria-label={label}
        placeholder={placeholder}
        spellCheck={false}
        className={`data min-w-0 bg-transparent text-[0.8125rem] text-ink outline-none placeholder:text-ink-3 ${width}`}
      />
      {discovery && discovery.state !== 'idle' && (
        <Menu
          label={`Values for ${label}`}
          empty={
            discovery.state === 'unsupported' || discovery.state === 'failed'
              ? (discovery.note ?? 'The daemon offered no values.')
              : discovery.state === 'reading'
                ? 'Asking the daemon.'
                : 'No recorded value starts with what you have typed. It can still be typed in full.'
          }
          items={items}
          trigger={
            <button
              type="button"
              title={
                discovery.state === 'unsupported'
                  ? 'This backend keeps no list of the values this dimension takes'
                  : 'Choose from the values on offer'
              }
              className="grid h-4 w-4 shrink-0 place-items-center text-ink-3 transition-colors hover:text-ink data-[state=open]:text-amber"
            >
              <ChoiceMark />
            </button>
          }
        />
      )}
    </span>
  )
}
