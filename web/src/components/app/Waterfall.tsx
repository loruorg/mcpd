/**
 * The waterfall: every step of a run, in the tree it actually ran as, against
 * one time axis.
 *
 * Alignment is the whole job here. The ruler and every row are laid out on the
 * same grid declaration, so nought to a hundred percent of the scale is nought
 * to a hundred percent of each bar track under it. The grid's columns are
 * fixed rather than content-sized, and the disclosure controls are lifted out
 * of the flow onto the row's own layer, so opening a branch adds rows and moves
 * nothing sideways: the axis a reader was measuring against does not shift
 * under them.
 */

import { useEffect, useRef } from 'react'
import { costBlank, duration, money } from '@/lib/api'
import { barOf, kindColour, kindLabel, stopReason } from './trace-tree'
import type { Extent, Row } from './trace-tree'
import { Highlight, PAYLOAD_TINT, RowMenu, Wedge } from './trace-ui'

/**
 * The columns the ruler and every row share, declared once.
 *
 * A ruler on its own grid is worse than no ruler: it invites you to measure a
 * bar against a scale that does not line up with it.
 *
 * The name column is capped, and capped lower on a narrow panel, because it is
 * greedy: `minmax(0,1fr)` beside it will shrink to nothing rather than hold a
 * name back, and a bar track of zero pixels is a chart drawn into no space.
 */
const ROW_GRID =
  'grid grid-cols-[minmax(7rem,10rem)_minmax(0,1fr)] items-center gap-3 @[46rem]/wf:grid-cols-[minmax(10rem,17rem)_minmax(0,1fr)] @[62rem]/wf:grid-cols-[minmax(12rem,25rem)_minmax(0,1fr)]'

/** How many labelled ticks the ruler carries at its widest. */
const TICKS = 5

/** One indent step, and the depth past which indentation stops growing. */
const INDENT = 0.62
const MAX_INDENT = 7

/** The width the disclosure and the row menu sit in, at every depth. */
const GUTTER = 2.6

export function Waterfall({
  rows,
  ext,
  selected,
  onSelect,
  collapsed,
  onToggle,
  onDescendants,
  onSiblings,
  query,
  matched,
  payloadOnly,
  inspectorId,
  emptyNote,
}: {
  rows: Row[]
  ext: Extent
  selected: string | null
  onSelect: (id: string) => void
  collapsed: ReadonlySet<string>
  onToggle: (id: string) => void
  onDescendants: (id: string, open: boolean) => void
  onSiblings: (parentId: string | undefined, open: boolean) => void
  query: string
  matched: ReadonlySet<string> | null
  payloadOnly: ReadonlySet<string> | null
  inspectorId: string
  /** What to say when a search has left nothing to draw. */
  emptyNote?: string
}) {
  const list = useRef<HTMLDivElement>(null)

  // The first match, brought into the list's own scroller. Set directly rather
  // than through `scrollIntoView`, which would scroll every ancestor as well
  // and pull the host page around whenever somebody typed.
  useEffect(() => {
    const box = list.current
    if (!box || !query || !matched?.size) return
    const first = box.querySelector<HTMLElement>('[data-matched="yes"]')
    if (!first) return
    const top = first.offsetTop - box.clientHeight / 2 + first.offsetHeight / 2
    box.scrollTo({ top: Math.max(0, top), behavior: 'auto' })
  }, [query, matched, rows])

  return (
    <div ref={list} className="@container/wf min-h-0 overflow-y-auto">
      {/* No rows, no ruler: a scale over an empty list measures nothing and
          invites the reader to read a run out of it. */}
      {rows.length > 0 && <TimeAxis totalMs={ext.total} />}
      {rows.length === 0 ? (
        <p className="p-5 text-[0.875rem] leading-relaxed text-ink-2">{emptyNote}</p>
      ) : (
        <ol className="divide-y divide-scribe-2">
          {rows.map((row) => (
            <WaterfallRow
              key={row.span.span_id}
              row={row}
              ext={ext}
              selected={selected === row.span.span_id}
              onSelect={onSelect}
              open={!collapsed.has(row.span.span_id)}
              onToggle={onToggle}
              onDescendants={onDescendants}
              onSiblings={onSiblings}
              query={query}
              matched={!!matched?.has(row.span.span_id)}
              payloadOnly={!!payloadOnly?.has(row.span.span_id)}
              inspectorId={inspectorId}
            />
          ))}
        </ol>
      )}
    </div>
  )
}

function WaterfallRow({
  row,
  ext,
  selected,
  onSelect,
  open,
  onToggle,
  onDescendants,
  onSiblings,
  query,
  matched,
  payloadOnly,
  inspectorId,
}: {
  row: Row
  ext: Extent
  selected: boolean
  onSelect: (id: string) => void
  open: boolean
  onToggle: (id: string) => void
  onDescendants: (id: string, open: boolean) => void
  onSiblings: (parentId: string | undefined, open: boolean) => void
  query: string
  matched: boolean
  payloadOnly: boolean
  inspectorId: string
}) {
  const s = row.span
  const bar = barOf(s, ext)
  const failed = s.status === 'error'
  const tone = kindColour(s.kind)
  const blank = costBlank(s)
  const stop = stopReason(s)
  const indent = Math.min(row.depth, MAX_INDENT) * INDENT

  return (
    <li className="relative">
      <button
        type="button"
        onClick={() => onSelect(s.span_id)}
        aria-current={selected}
        aria-controls={inspectorId}
        data-matched={matched ? 'yes' : undefined}
        className={`${ROW_GRID} w-full px-4 py-1.5 text-left transition-colors ${
          selected ? 'bg-panel-3' : 'hover:bg-panel-2'
        }`}
      >
        <span
          className="flex min-w-0 items-baseline gap-2"
          style={{ paddingLeft: `${indent + GUTTER}rem` }}
        >
          {/* Hidden on a narrow panel: the name column is seven rem there, and
              a kind label beside it would leave three characters of name. The
              bar's colour still says the kind and the inspector names it. */}
          <span
            className={`data hidden shrink-0 text-[0.5625rem] uppercase tracking-wider @[34rem]/wf:inline ${tone.text}`}
          >
            {kindLabel(s.kind)}
          </span>
          <span
            className={`min-w-0 truncate text-[0.8125rem] ${failed ? 'text-oxide' : 'text-ink'}`}
            title={
              payloadOnly
                ? `${s.name}: matches your search in this step's details, not in its name`
                : s.name
            }
            style={payloadOnly ? { background: PAYLOAD_TINT, borderRadius: '1px' } : undefined}
          >
            <Highlight text={s.name} query={query} />
          </span>
          {stop?.atLimit && <LimitMark />}
          {!open && row.hasChildren && (
            <span className="data shrink-0 text-[0.625rem] text-ink-3" title={`${row.descendants} steps folded away`}>
              +{row.descendants}
            </span>
          )}
        </span>

        <span className="flex min-w-0 items-center gap-3">
          <span className="relative h-3 min-w-0 flex-1">
            <Bar bar={bar} total={ext.total} failed={failed} tone={tone.bar} name={s.name} />
          </span>
          <span
            className={`w-14 shrink-0 text-right text-[0.6875rem] ${
              s.duration_ns != null ? 'data text-ink-3' : bar.running ? 'data text-amber' : 'text-ink-3'
            }`}
          >
            {s.duration_ns != null ? duration(s.duration_ns) : s.is_event ? 'event' : 'running'}
          </span>
          <span className="hidden w-20 shrink-0 text-right text-[0.6875rem] text-ink-3 @[40rem]/wf:block">
            {s.cost?.total_cost != null ? (
              <span className="data">{money(s.cost.total_cost, s.cost.currency)}</span>
            ) : blank && blank.code !== 'not-a-model-call' ? (
              // Why there is no figure, where the figure would have been. A step
              // that spends no tokens is left empty: its own kind, at the head of
              // this row, already says so.
              <span title={blank.long}>{blank.short}</span>
            ) : null}
          </span>
        </span>
      </button>

      {/* Lifted onto the row's own layer so the indentation these carry cannot
          push the bar track sideways. The name cell reserves exactly this width
          at every depth, so nothing overlaps and every name at one depth starts
          at one x. */}
      {row.hasChildren && (
        <span
          className="absolute top-1/2 flex -translate-y-1/2 items-center gap-1"
          style={{ left: `${1 + indent}rem` }}
        >
          <button
            type="button"
            onClick={() => onToggle(s.span_id)}
            aria-expanded={open}
            aria-label={`${open ? 'Collapse' : 'Expand'} ${s.name}`}
            title={`${open ? 'Collapse' : 'Expand'} this step (${row.descendants} below it)`}
            className="grid h-4 w-4 place-items-center text-ink-3 transition-colors hover:text-ink"
          >
            <Wedge open={open} />
          </button>
          <RowMenu
            label={`Tree actions for ${s.name}`}
            items={[
              { label: 'Expand descendants', onSelect: () => onDescendants(s.span_id, true) },
              { label: 'Collapse descendants', onSelect: () => onDescendants(s.span_id, false) },
              { label: 'Expand siblings at this level', onSelect: () => onSiblings(row.parentId, true) },
              { label: 'Collapse siblings at this level', onSelect: () => onSiblings(row.parentId, false) },
            ]}
          />
        </span>
      )}
    </li>
  )
}

/**
 * One timing bar.
 *
 * A step that has not ended is hatched rather than filled, because a solid bar
 * would claim an end time nobody has reported. The hatch runs to now, which is
 * where the axis ends while anything is still open, so the bar and the scale
 * agree about how long this has been going.
 */
function Bar({
  bar,
  total,
  failed,
  tone,
  name,
}: {
  bar: { start: number; length: number; running: boolean }
  total: number
  failed: boolean
  tone: string
  name: string
}) {
  const colour = failed ? 'var(--color-oxide)' : tone
  const left = `${Math.max(0, Math.min(100, (bar.start / total) * 100))}%`
  const width = `max(2px, ${Math.max(0, Math.min(100, (bar.length / total) * 100))}%)`
  return (
    <span
      className="app-bar absolute top-1/2 h-[6px] -translate-y-1/2 rounded-[1px]"
      style={{
        left,
        width,
        background: bar.running
          ? // Hard stops, so this is a drawn hatch and not a blend: the
            // drafting convention for something provisional.
            `repeating-linear-gradient(115deg, ${colour} 0 2px, transparent 2px 5px)`
          : colour,
        opacity: failed ? 1 : bar.running ? 1 : 0.82,
      }}
      title={
        bar.running
          ? `${name} has not ended. The bar runs to now.`
          : `${name}: ${duration(bar.length * 1e6)}, starting ${duration(bar.start * 1e6)} into the run`
      }
    />
  )
}

/**
 * The mark on a call that stopped at the model's limit.
 *
 * Output rising into a ceiling it cannot pass. Drawn rather than borrowed from
 * a pack, so it means this one thing on this panel and is not a warning
 * triangle that could mean anything.
 */
function LimitMark() {
  return (
    <span
      className="shrink-0 text-amber"
      title="This call stopped at the model's token limit. Its output is cut short."
    >
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <path
          d="M5 8.7 L5 3.3 M2.7 5.3 L5 3 L7.3 5.3 M1.7 1.4 L8.3 1.4"
          stroke="currentColor"
          strokeWidth="1.25"
          strokeLinecap="round"
          strokeLinejoin="round"
          fill="none"
        />
      </svg>
    </span>
  )
}

/**
 * A tick label.
 *
 * The unit and the precision come from the run's total, not from the tick, so
 * one ruler never mixes seconds with minutes and never steps from `36s` to
 * `1.2m` between two ticks of the same scale.
 */
function axisLabel(ms: number, totalMs: number): string {
  if (ms === 0) return '0'
  if (totalMs >= 600_000) return `${(ms / 60_000).toFixed(1)}m`
  if (totalMs >= 1000) return `${(ms / 1000).toFixed(totalMs >= 10_000 ? 0 : 1)}s`
  return `${Math.round(ms)}ms`
}

/**
 * The waterfall's time ruler.
 *
 * The ticks are computed from the same total the bars are laid out against, and
 * the ruler reserves the same trailing columns the rows do, so the scale ends
 * exactly where every bar track ends. It is inside the scrolling list on
 * purpose: outside it, a scrollbar on the list would make the two differ by the
 * width of that scrollbar and the scale would quietly stop matching the bars.
 */
function TimeAxis({ totalMs }: { totalMs: number }) {
  const ticks = Array.from({ length: TICKS }, (_, i) => ({
    fraction: i / (TICKS - 1),
    label: axisLabel((totalMs / (TICKS - 1)) * i, totalMs),
  }))

  return (
    // Hidden from assistive technology on purpose: every row announces its own
    // duration, which is the figure this scale exists to let you estimate.
    <div
      className={`${ROW_GRID} sticky top-0 z-10 border-b border-scribe-2 bg-panel-1 px-4 pb-1 pt-1.5`}
      aria-hidden="true"
    >
      <div />
      <div className="flex min-w-0 items-center gap-3">
        <div className="@container/track relative h-5 min-w-0 flex-1">
          {ticks.map((t) => (
            <span
              key={`tick-${t.fraction}`}
              className="absolute top-0 block h-1.5 w-px bg-scribe"
              style={{ left: `${(t.fraction * 100).toFixed(3)}%` }}
            />
          ))}
          {ticks.map((t, i) => (
            <span
              key={`label-${t.fraction}`}
              // Which labels survive is decided by the width of the TRACK and
              // not of the panel: the cost column appears at one width and the
              // inspector at another, so the track can get narrower as the
              // panel gets wider. Numbers that do not fit are worse than no
              // numbers, so below 5rem the labels go and the ticks stay.
              className={`data absolute bottom-0 whitespace-nowrap text-[0.625rem] leading-none text-ink-3 @max-[5rem]/track:hidden${
                i % 2 === 1 ? ' @max-[17rem]/track:hidden' : ''
              }${i !== 0 && i !== TICKS - 1 ? ' @max-[7rem]/track:hidden' : ''}`}
              style={{
                left: `${(t.fraction * 100).toFixed(3)}%`,
                // The end labels are pulled inside the track rather than
                // centred on their tick, so neither is clipped by its edge.
                transform: `translateX(${i === 0 ? '0' : i === TICKS - 1 ? '-100%' : '-50%'})`,
              }}
            >
              {t.label}
            </span>
          ))}
        </div>
        {/* The same spacers the rows use, so the axis ends where their bars do. */}
        <span className="w-14 shrink-0" />
        <span className="hidden w-20 shrink-0 @[40rem]/wf:block" />
      </div>
    </div>
  )
}

/**
 * The colour key, limited to the kinds this run actually contains.
 *
 * A legend listing every kind mcpd models would be a list of thirty-three
 * things, thirty of which are not on screen, and the reader would have to find
 * the four that are.
 */
export function Legend({ kinds, live }: { kinds: string[]; live: boolean }) {
  if (!kinds.length) return null
  return (
    <ul className="flex flex-wrap items-center gap-x-4 gap-y-1.5">
      {kinds.map((k) => (
        <li key={k} className="flex items-center gap-1.5">
          <span
            className="h-[4px] w-3.5 shrink-0 rounded-[2px]"
            style={{ background: kindColour(k).bar, opacity: 0.82 }}
            aria-hidden="true"
          />
          <span className="legend text-[0.5625rem] text-ink-3">{kindLabel(k)}</span>
        </li>
      ))}
      {live && (
        <li className="flex items-center gap-1.5">
          <span
            className="h-[4px] w-3.5 shrink-0 rounded-[2px]"
            style={{
              background:
                'repeating-linear-gradient(115deg, var(--color-ink-4) 0 2px, transparent 2px 5px)',
            }}
            aria-hidden="true"
          />
          <span className="legend text-[0.5625rem] text-ink-3">not ended</span>
        </li>
      )}
    </ul>
  )
}
