/**
 * The run list: a table of the trace projection, windowed, with the keyboard
 * semantics its markup promises.
 *
 * Three things here are worth knowing before changing it.
 *
 * The rows are WINDOWED, not revealed. Only the rows near the viewport are in
 * the DOM, and which ones those are is computed from the scroll offset, never
 * from an observer telling a row it may now exist. If no scroll event ever
 * fires and no measurement ever lands, the first screenful is still rendered,
 * because the window opens at a default height rather than at nothing.
 *
 * The header and every row share ONE grid declaration, so a figure is always
 * under the label that names it.
 *
 * The active row is always in the DOM even when it is far outside the window.
 * `aria-activedescendant` names an element by id, and a screen reader cannot
 * announce a row that was never rendered, so keyboard navigation materialises
 * the row before it moves to it rather than after.
 */

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import type { Trace } from '@/generated/types'
import { gridOf, metaCell, minWidthOf, ROW_H } from './list-prefs'
import type { ColumnDef, Density } from './list-prefs'

/** How many rows above and below the viewport are kept in the DOM. */
const OVERSCAN = 8

/** The height the window opens at, before anything has been measured. */
const ASSUMED_VIEW = 640

/**
 * The header's own lettering.
 *
 * Sentence case in the display face, not the engraved legend the panel uses to
 * name a region. Two reasons, and both matter. A column heading is a different
 * job from a panel legend, and dressing every small string in the same
 * tracked-out capitals is how an interface stops having a voice. And tracked
 * capitals are wide: `Environment` in legend lettering does not fit the column
 * its own values fit in, so the heading was being cut off by the cell it names.
 */
const HEAD = { fontFamily: 'var(--font-display)', fontWeight: 600, letterSpacing: '0.01em' }

/**
 * The gutter a cell keeps.
 *
 * Wider on the first and last column of the table, so no line of type is jammed
 * against the edge of the screen. The header and the rows call the same function
 * with the same index, which is what keeps the two on one grid.
 */
function pad(i: number, last: number): string {
  if (i === 0) return 'pl-4 pr-2'
  return i === last ? 'pl-2 pr-4' : 'px-2'
}

/** Which sort key a column heading sets, where it sets one. */
const SORTS: Record<string, string> = {
  created: 'start_time',
  duration: 'duration',
  cost: 'cost',
  steps: 'span_count',
  score: 'score',
}

export function TraceTable({
  traces,
  columns,
  meta,
  density,
  selected,
  onSelect,
  arrived,
  sort,
  order,
  onSort,
  hasMore,
  loadingMore,
  onMore,
  paged,
  top,
  empty,
}: {
  traces: Trace[]
  columns: ColumnDef[]
  meta: string[]
  density: Density
  selected: string | null
  onSelect: (id: string) => void
  /** Runs that arrived in the last poll, washed until the reader has seen them. */
  arrived: ReadonlySet<string>
  sort: string
  order: 'asc' | 'desc'
  onSort: (sort: string) => void
  hasMore: boolean
  loadingMore: boolean
  onMore: () => void
  /** True once a further page has been read, which stops the live tail. */
  paged: boolean
  /**
   * Bumped when the list should go back to the top: a new query, or the tail
   * being switched back on.
   *
   * Scrolling back matters more than it looks. Left at the bottom, the sentinel
   * that reads the next page is on screen the instant the list is replaced, so
   * following again immediately paged again and switched the tail straight back
   * off.
   */
  top: number
  /** What to show in place of rows when there are none. */
  empty: ReactNode
}) {
  const scroller = useRef<HTMLDivElement>(null)
  const sentinel = useRef<HTMLDivElement>(null)
  const head = useRef<HTMLDivElement>(null)
  const [scrollTop, setScrollTop] = useState(0)
  const [viewH, setViewH] = useState(ASSUMED_VIEW)
  /**
   * Where the keyboard is, held as a run rather than as a row number.
   *
   * An index is the wrong handle for a list that is re-read every few seconds:
   * rows arrive above the cursor and it silently means a different run. Holding
   * the id means the cursor stays on the run the reader put it on, and it costs
   * one search of an array of at most a few hundred.
   */
  const [activeId, setActiveId] = useState<string | null>(null)
  // Whether the keyboard is in the list. The active row is marked only while it
  // is, so a list nobody is driving does not carry a second selection mark.
  const [focused, setFocused] = useState(false)
  const rowH = ROW_H[density]
  const n = traces.length
  const found = traces.findIndex((t) => t.trace_id === activeId)
  // A cursor on a run that is no longer in the list falls back to the top,
  // which is where a fresh list starts anyway.
  const active = found >= 0 ? found : 0

  const grid = useMemo(() => gridOf(columns, meta), [columns, meta])
  const minWidth = useMemo(() => minWidthOf(columns, meta), [columns, meta])
  const rightmost = columns.length + meta.length - 1

  /* The window. Clamped to the list, and always holding the active row. */
  const first = Math.max(0, Math.floor(scrollTop / rowH) - OVERSCAN)
  const last = Math.min(n, Math.ceil((scrollTop + viewH) / rowH) + OVERSCAN)
  const rendered: number[] = []
  for (let i = first; i < last; i += 1) rendered.push(i)
  if (active >= 0 && active < n && (active < first || active >= last)) rendered.push(active)

  useEffect(() => {
    if (scroller.current) scroller.current.scrollTop = 0
  }, [top])

  useEffect(() => {
    const el = scroller.current
    if (!el) return
    const ro = new ResizeObserver(() => setViewH(el.clientHeight || ASSUMED_VIEW))
    ro.observe(el)
    setViewH(el.clientHeight || ASSUMED_VIEW)
    return () => ro.disconnect()
  }, [])

  // The selection can move from outside: a link opened on a run, another screen
  // sending the reader here. The cursor follows the selection, but ONLY when the
  // selection itself moves. Following it on every list change would drag the
  // keyboard back to the open run every time the tail polls.
  useEffect(() => {
    if (selected) setActiveId(selected)
  }, [selected])

  /**
   * Bring a row fully into view, moving as little as possible.
   *
   * The offset it scrolls to is measured from the sticky header's real height,
   * not guessed: a row scrolled to `i * rowH` lands UNDER the header, which is
   * the row you cannot read.
   */
  const reveal = useCallback(
    (i: number) => {
      const el = scroller.current
      if (!el) return
      const bar = head.current?.offsetHeight ?? rowH
      const top = bar + i * rowH
      if (top - bar < el.scrollTop) el.scrollTop = Math.max(0, top - bar)
      else if (top + rowH > el.scrollTop + el.clientHeight) {
        el.scrollTop = top + rowH - el.clientHeight
      }
    },
    [rowH],
  )

  const move = useCallback(
    (to: number) => {
      const i = Math.max(0, Math.min(n - 1, to))
      if (traces[i]) setActiveId(traces[i].trace_id)
      reveal(i)
    },
    [n, reveal, traces],
  )

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!n) return
    const page = Math.max(1, Math.floor(viewH / rowH) - 1)
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault()
        move(active + 1)
        break
      case 'ArrowUp':
        e.preventDefault()
        move(active - 1)
        break
      case 'PageDown':
        e.preventDefault()
        move(active + page)
        break
      case 'PageUp':
        e.preventDefault()
        move(active - page)
        break
      case 'Home':
        e.preventDefault()
        move(0)
        break
      case 'End':
        e.preventDefault()
        move(n - 1)
        break
      case 'Enter':
      case ' ':
        e.preventDefault()
        if (traces[active]) onSelect(traces[active].trace_id)
        break
      default:
    }
  }

  /* Infinite scroll. The sentinel decides only whether to ask for ANOTHER page;
     every row already read is in the DOM whether it fires or not, and the same
     request is one press of a control that is always on screen. */
  useEffect(() => {
    const el = sentinel.current
    const root = scroller.current
    if (!el || !root || !hasMore || loadingMore) return
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((x) => x.isIntersecting)) onMore()
      },
      { root, rootMargin: '200px' },
    )
    io.observe(el)
    return () => io.disconnect()
  }, [hasMore, loadingMore, onMore, n])

  // Keeping the scroll where the reader left it is the whole point of a live
  // list: rows are replaced under them every few seconds. React reuses the
  // scroller across those updates, so the browser preserves scrollTop on its
  // own; this only catches the case where the list got shorter than the offset.
  useLayoutEffect(() => {
    const el = scroller.current
    if (!el) return
    const max = Math.max(0, n * rowH + rowH - el.clientHeight)
    if (el.scrollTop > max) el.scrollTop = max
  }, [n, rowH])

  const listId = 'mcpd-run-list'
  const activeDescendant = n && active < n ? `${listId}-${traces[active].trace_id}` : undefined

  return (
    <div
      ref={scroller}
      onScroll={(e) => setScrollTop((e.target as HTMLDivElement).scrollTop)}
      className="scroll-x min-h-0 overflow-y-auto bg-panel-1"
    >
      <div style={{ minWidth }}>
        <div
          ref={head}
          className="sticky top-0 z-10 grid border-b border-scribe bg-panel-2"
          style={{ gridTemplateColumns: grid, boxShadow: '0 1px 0 var(--color-lip)' }}
        >
          {columns.map((c, i) => {
            const key = SORTS[c.id]
            const on = key && key === sort
            return key ? (
              <button
                key={c.id}
                type="button"
                onClick={() => onSort(key)}
                title={`${c.help}\n\nSort by this column.`}
                style={HEAD}
                className={`flex items-center gap-1 truncate py-1.5 text-[0.6875rem] transition-colors hover:text-ink ${pad(i, rightmost)} ${
                  c.end ? 'justify-end' : ''
                } ${on ? 'text-ink' : 'text-ink-3'}`}
              >
                <span className="truncate">{c.label}</span>
                {/* The mark says which way, and appears only on the column that
                    is actually ordering the list. */}
                {on && (
                  <svg width="7" height="7" viewBox="0 0 7 7" aria-hidden="true" className="shrink-0">
                    <path
                      d={order === 'desc' ? 'M0.9 2.2 L3.5 5 L6.1 2.2' : 'M0.9 4.8 L3.5 2 L6.1 4.8'}
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="1.2"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    />
                  </svg>
                )}
              </button>
            ) : (
              <span
                key={c.id}
                title={c.help}
                style={HEAD}
                className={`truncate py-1.5 text-[0.6875rem] text-ink-3 ${pad(i, rightmost)} ${c.end ? 'text-right' : ''}`}
              >
                {c.label}
              </span>
            )
          })}
          {meta.map((k, i) => (
            <span
              key={`meta:${k}`}
              // The raw key, whole, on hover. The header shows what fits.
              title={`${k}\n\nRead from each run’s correlation, then its score means, then its feedback counts.`}
              style={HEAD}
              className={`truncate py-1.5 text-[0.6875rem] text-ink-3 ${pad(columns.length + i, rightmost)}`}
            >
              {k}
            </span>
          ))}
        </div>

        {n === 0 ? (
          <div className="px-4 py-6">{empty}</div>
        ) : (
          <div
            style={{
              position: 'relative',
              height: n * rowH,
              // The focus ring sits INSIDE the rows. At the system's default
              // outset it drew a bright line across the filter bar above the
              // list, which reads as a rule on that bar rather than as focus
              // on this one. Inline, because the rule it has to beat is
              // unlayered and so outranks any utility class.
              outlineOffset: '-2px',
            }}
            // The rows, and only the rows, are the listbox: a heading with a
            // sort control in it is not an option and does not belong in one.
            // One tab stop, walked with the arrow keys. Sixty tab stops down a
            // table is not navigation.
            tabIndex={0}
            role="listbox"
            aria-label="Runs"
            aria-activedescendant={activeDescendant}
            onKeyDown={onKeyDown}
            onFocus={() => {
              setFocused(true)
              // Arriving with no cursor, put it on the open run if there is
              // one, so the first arrow key moves from where the reader is
              // looking.
              if (!activeId && selected) setActiveId(selected)
            }}
            onBlur={() => setFocused(false)}
          >
            {rendered.map((i) => {
              const t = traces[i]
              const on = t.trace_id === selected
              const isNew = arrived.has(t.trace_id)
              return (
                <div
                  key={t.trace_id}
                  id={`${listId}-${t.trace_id}`}
                  role="option"
                  aria-selected={on}
                  onMouseDown={() => {
                    setActiveId(t.trace_id)
                    onSelect(t.trace_id)
                  }}
                  style={{
                    position: 'absolute',
                    top: i * rowH,
                    left: 0,
                    width: '100%',
                    height: rowH,
                    gridTemplateColumns: grid,
                    // The wash on a row that has this moment arrived. Amber, because on
                    // this panel amber means attention, and it fades out rather
                    // than being switched off under the eye.
                    background: on
                      ? 'var(--color-panel-3)'
                      : isNew
                        ? 'color-mix(in oklab, var(--color-amber) 13%, transparent)'
                        : undefined,
                    transition: 'background-color 600ms ease-out',
                    // Where the keyboard is. Inset so the mark sits inside the
                    // row's own edge rather than over the row below it.
                    outline: focused && i === active ? '1px solid var(--color-amber)' : undefined,
                    outlineOffset: '-1px',
                  }}
                  className={`grid cursor-pointer items-center border-b border-scribe-2 ${
                    on ? '' : 'hover:bg-panel-2'
                  }`}
                >
                  {columns.map((c, ci) => {
                    const cell = c.cell(t)
                    return (
                      <span
                        key={c.id}
                        title={cell.title}
                        className={`truncate text-[0.75rem] leading-none ${pad(ci, rightmost)} ${
                          cell.prose ? '' : 'data'
                        } ${c.end ? 'text-right' : ''} ${cell.tone ?? 'text-ink-2'}`}
                      >
                        {cell.text}
                      </span>
                    )
                  })}
                  {meta.map((k, mi) => {
                    const cell = metaCell(t, k)
                    return (
                      <span
                        key={`meta:${k}`}
                        title={cell.title}
                        className={`data truncate text-[0.75rem] leading-none ${pad(columns.length + mi, rightmost)} ${cell.tone ?? 'text-ink-2'}`}
                      >
                        {cell.text}
                      </span>
                    )
                  })}
                </div>
              )
            })}
          </div>
        )}

        {/* The end of the list says which end it is: another page to read, the
            last page, or a tail that has been stopped because paging and
            tailing cannot both be true. */}
        <div ref={sentinel} className="flex items-center gap-3 px-4 py-3">
          {hasMore ? (
            <button
              type="button"
              onClick={onMore}
              disabled={loadingMore}
              className="legend panel-raised px-2.5 py-1 text-[0.5625rem] text-ink-3 transition-colors hover:bg-panel-3 hover:text-ink disabled:cursor-wait"
            >
              {loadingMore ? 'reading' : 'read older runs'}
            </button>
          ) : (
            n > 0 && (
              <span className="text-[0.75rem] text-ink-3">
                {n} run{n === 1 ? '' : 's'}, which is every one that matches.
              </span>
            )
          )}
          {paged && (
            <span className="text-[0.75rem] text-ink-3">
              The live tail stopped when you read past the first page.
            </span>
          )}
        </div>
      </div>
    </div>
  )
}
