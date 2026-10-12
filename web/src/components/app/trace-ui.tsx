/**
 * The small parts the trace panel builds everything else out of: a match
 * highlighter, a copy control, a quiet action button and the panel's own
 * disclosure and tree marks.
 *
 * They live together because they are the panel's vocabulary. A second copy
 * button styled a little differently, or a second highlight colour, is how one
 * interface quietly becomes two.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import * as DropdownMenu from '@radix-ui/react-dropdown-menu'

/* ---------------------------------------------------------- highlight --- */

/**
 * The two marks a search leaves.
 *
 * `direct` is the panel's own selection tint: an amber wash under the matched
 * characters, ink untouched, so a highlighted string is exactly as readable as
 * an unhighlighted one. `indirect` is the second colour, and it means something
 * different: the match is not in what you can see, it is inside this span's
 * payload. They are deliberately different hues rather than two strengths of
 * one, because the reader has to tell them apart at a glance in a dense list.
 */
export const MATCH_TINT = 'color-mix(in oklab, var(--color-amber) 26%, transparent)'
export const PAYLOAD_TINT = 'color-mix(in oklab, var(--color-slate) 22%, transparent)'

/**
 * As many marks as one string is worth painting.
 *
 * A pathological query against a long payload can match thousands of times, and
 * each mark is two DOM nodes. The cap is high enough that the rendered chunk of
 * a payload is marked in full in every ordinary case, and the section says how
 * many matches it found so a truncated marking is never silent.
 */
const MARK_CAP = 1500

/**
 * Text with every occurrence of `query` marked.
 *
 * Case-insensitive, and it walks the string rather than building a regular
 * expression, so a query full of regex metacharacters is a search for those
 * characters rather than a syntax error or an accidental wildcard.
 *
 * The first mark carries `data-first-match`, which is what the panel scrolls to.
 */
export function Highlight({
  text,
  query,
  tint = MATCH_TINT,
}: {
  text: string
  query: string
  tint?: string
}) {
  const parts = useMemo(() => splitMatches(text, query), [text, query])
  if (parts.length < 2) return <>{text}</>
  let seen = 0
  return (
    <>
      {parts.map((p, i) =>
        p.match ? (
          <mark
            key={i}
            data-match=""
            data-first-match={seen++ === 0 ? '' : undefined}
            style={{ background: tint, color: 'inherit', borderRadius: '1px' }}
          >
            {p.text}
          </mark>
        ) : (
          <span key={i}>{p.text}</span>
        ),
      )}
    </>
  )
}

interface Part {
  text: string
  match: boolean
}

export function splitMatches(text: string, query: string): Part[] {
  const q = query.trim()
  if (!q) return [{ text, match: false }]
  const hay = text.toLowerCase()
  const needle = q.toLowerCase()
  const out: Part[] = []
  let at = 0
  let found = 0
  for (;;) {
    const i = hay.indexOf(needle, at)
    if (i < 0 || found >= MARK_CAP) break
    if (i > at) out.push({ text: text.slice(at, i), match: false })
    out.push({ text: text.slice(i, i + needle.length), match: true })
    at = i + needle.length
    found += 1
  }
  if (!out.length) return [{ text, match: false }]
  if (at < text.length) out.push({ text: text.slice(at), match: false })
  return out
}

/** Whether a query appears in a string at all, without building the parts. */
export function hasMatch(text: string, query: string): boolean {
  const q = query.trim().toLowerCase()
  return q.length > 0 && text.toLowerCase().includes(q)
}

/**
 * How many times a query occurs, counted rather than rendered.
 *
 * Uncapped on purpose: this is the figure a reader is told, and capping it
 * would report the marking budget as if it were the number of matches.
 */
export function countMatches(text: string, query: string): number {
  const q = query.trim().toLowerCase()
  if (!q) return 0
  const hay = text.toLowerCase()
  let n = 0
  let at = hay.indexOf(q)
  while (at !== -1) {
    n += 1
    at = hay.indexOf(q, at + q.length)
  }
  return n
}

/* ------------------------------------------------------------- inputs --- */

export function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value)
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms)
    return () => clearTimeout(t)
  }, [value, ms])
  return v
}

/**
 * A text field in the panel's material: a recess, not a bordered box.
 *
 * `onClear` draws the control that empties it, which a search box needs and a
 * browser only supplies on `type="search"` in some engines and never in a shape
 * that belongs to this panel.
 */
export function SearchField({
  value,
  onChange,
  placeholder,
  label,
  className = '',
  inputRef,
}: {
  value: string
  onChange: (v: string) => void
  placeholder: string
  label: string
  className?: string
  inputRef?: React.Ref<HTMLInputElement>
}) {
  return (
    <div className={`well relative flex min-w-0 items-center ${className}`}>
      <input
        ref={inputRef}
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        aria-label={label}
        placeholder={placeholder}
        spellCheck={false}
        // No focus styling of its own: the panel defines one amber ring for
        // everything that takes focus, and a second one here would be a second
        // system.
        className="w-full min-w-0 bg-transparent py-1.5 pl-2.5 pr-7 text-[0.8125rem] text-ink placeholder:text-ink-3"
      />
      {value && (
        <button
          type="button"
          onClick={() => onChange('')}
          aria-label={`Clear ${label.toLowerCase()}`}
          className="absolute right-1.5 grid h-5 w-5 place-items-center text-ink-3 transition-colors hover:text-ink"
        >
          <svg width="9" height="9" viewBox="0 0 9 9" aria-hidden="true">
            <path
              d="M1 1 L8 8 M8 1 L1 8"
              stroke="currentColor"
              strokeWidth="1.3"
              strokeLinecap="round"
            />
          </svg>
        </button>
      )}
    </div>
  )
}

/* ------------------------------------------------------------ actions --- */

/**
 * A quiet action.
 *
 * The house's raised control at toolbar size: panel lettering on a surface a
 * few percent lighter than the one it sits on, with a hairline in that
 * surface's own colour and a light lip along its top edge. It does not lift and
 * it does not glow; pressed is a further step of tone, and disabled loses the
 * raised surface rather than fading the label, because a faded label on this
 * panel stops clearing its background.
 */
export function Action({
  children,
  onClick,
  title,
  disabled,
  pressed,
}: {
  children: ReactNode
  onClick: () => void
  title?: string
  disabled?: boolean
  pressed?: boolean
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      disabled={disabled}
      aria-pressed={pressed}
      className={`legend panel-raised shrink-0 px-2 py-1 text-[0.5625rem] transition-colors ${
        pressed ? 'bg-panel-3 text-ink' : 'text-ink-3 hover:bg-panel-3 hover:text-ink'
      } disabled:cursor-not-allowed disabled:border-transparent disabled:bg-transparent disabled:text-ink-3 disabled:hover:bg-transparent disabled:hover:text-ink-3`}
    >
      {children}
    </button>
  )
}

/**
 * A value you can take with you.
 *
 * The value itself is the control, because a separate copy glyph beside twelve
 * ids is twelve glyphs. The confirmation replaces the label for a moment rather
 * than appearing beside it, so nothing on the panel moves when you press it.
 *
 * A failure is reported rather than swallowed: a clipboard write can be refused
 * outright (an insecure origin, a browser policy), and a control that silently
 * does nothing is worse than one that says it could not.
 */
export function CopyValue({
  value,
  className = '',
  children,
}: {
  value: string
  className?: string
  children: ReactNode
}) {
  const [state, setState] = useState<'idle' | 'copied' | 'failed'>('idle')
  const timer = useRef<number | undefined>(undefined)

  useEffect(() => () => window.clearTimeout(timer.current), [])

  const copy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(value)
      setState('copied')
    } catch {
      setState('failed')
    }
    window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => setState('idle'), 1400)
  }, [value])

  return (
    <button
      type="button"
      onClick={() => void copy()}
      title={state === 'failed' ? 'This browser refused the clipboard' : `Copy ${value}`}
      className={`min-w-0 text-left transition-colors hover:text-amber ${className}`}
    >
      {state === 'idle' ? (
        children
      ) : (
        <span className="legend text-[0.5625rem] text-ink-3">
          {state === 'copied' ? 'copied' : 'copy refused'}
        </span>
      )}
    </button>
  )
}

/* -------------------------------------------------------------- marks --- */

/**
 * The disclosure mark.
 *
 * A right-pointing wedge that turns a quarter when the branch opens. Drawn
 * rather than taken from an icon pack so its stroke and its cap match the
 * ruling on the rest of the panel, and rotated by transform so the mark itself
 * is one shape in both states.
 */
export function Wedge({ open }: { open: boolean }) {
  return (
    <svg
      width="7"
      height="7"
      viewBox="0 0 7 7"
      aria-hidden="true"
      className="transition-transform duration-150 motion-reduce:transition-none"
      style={{ transform: open ? 'rotate(90deg)' : 'none' }}
    >
      <path d="M1.6 0.7 L5.6 3.5 L1.6 6.3 Z" fill="currentColor" />
    </svg>
  )
}

/**
 * The tree mark, on the control that acts on more than one row.
 *
 * A trunk with two branches: the same figure the indentation draws, at glyph
 * size, so the control looks like the thing it operates on.
 */
export function TreeMark() {
  return (
    <svg width="9" height="9" viewBox="0 0 9 9" aria-hidden="true">
      <path
        d="M1.5 0.8 L1.5 7.4 M1.5 2.9 L7 2.9 M1.5 6.4 L7 6.4"
        stroke="currentColor"
        strokeWidth="1.2"
        strokeLinecap="round"
        fill="none"
      />
    </svg>
  )
}

/* --------------------------------------------------------------- menu --- */

/**
 * A menu of actions for one row, on Radix.
 *
 * The primitive is taken for the behaviour nobody should hand-roll: focus moves
 * into the menu and back out to the trigger, Escape and an outside press close
 * it, the arrow keys walk it, and it is positioned so it never opens off the
 * edge of a scrolling list. The look is the panel's own: a raised surface, a
 * hairline in the surface's colour, no shadow.
 */
export function RowMenu({
  label,
  items,
}: {
  label: string
  items: { label: string; onSelect: () => void }[]
}) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger
        aria-label={label}
        title={label}
        className="grid h-4 w-4 place-items-center text-ink-3 transition-colors hover:text-ink data-[state=open]:text-amber"
      >
        <TreeMark />
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          sideOffset={4}
          align="start"
          className="panel-raised z-50 min-w-[12rem] py-1"
          // The one shadow this system allows: a genuinely floating element,
          // cast tight and downward from a single light, so the sheet reads as
          // lifted off the rows rather than painted on them. Tinted to the ink
          // rather than to black.
          style={{ background: 'var(--color-panel-3)', boxShadow: 'var(--shadow-float)' }}
        >
          {items.map((it) => (
            <DropdownMenu.Item
              key={it.label}
              onSelect={it.onSelect}
              className="cursor-pointer px-3 py-1.5 text-[0.8125rem] text-ink-2 outline-none data-[highlighted]:bg-panel-3 data-[highlighted]:text-ink"
            >
              {it.label}
            </DropdownMenu.Item>
          ))}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  )
}
