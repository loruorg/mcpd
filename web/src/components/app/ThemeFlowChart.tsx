/**
 * One snapshot's flow across aspects, drawn from the daemon's own counts.
 *
 * A column per stage, a bar per node sized by its runs, and a ribbon per link
 * sized by the runs its two ends share. Every height is a count the flow route
 * returned, scaled by its stage's total, so the drawing can be checked against
 * the numbers beside it.
 */

import { useEffect, useRef, useState } from 'react'
import type { ThemeFlow, ThemeNode } from '@/generated/types'
import { share } from './themes-api'

const HEIGHT = 380
const TOP = 28
const NODE_W = 10
const GAP = 10
const FONT_PX = 12

interface Placed {
  node: ThemeNode
  aspect: string
  stage: number
  x: number
  y: number
  h: number
  /** Running offsets for the ribbons leaving and entering this node. */
  out: number
  in: number
}

export function ThemeFlowChart({
  flow,
  labelOf,
  onOpen,
}: {
  flow: ThemeFlow
  labelOf: (name: string) => string
  onOpen: (aspect: string, node: ThemeNode) => void
}) {
  const box = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(900)
  const [hover, setHover] = useState<string | null>(null)

  useEffect(() => {
    const el = box.current
    if (!el) return
    const ro = new ResizeObserver(([e]) => setWidth(Math.max(320, Math.floor(e.contentRect.width))))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const n = flow.stages.length
  // Every label sits to the right of its bar, the last stage's included, so
  // the last column keeps room for its labels rather than writing them back
  // over the stage before it.
  const tail = Math.min(260, Math.floor(width * 0.22))
  const span = (width - NODE_W - tail) / Math.max(1, n - 1)
  const bottom = HEIGHT - 4

  // Lay out every node, stage by stage, keeping the daemon's node order.
  const placed = new Map<string, Placed>()
  flow.stages.forEach((stage, i) => {
    const total = stage.nodes.reduce((s, x) => s + x.trace_count, 0) || 1
    const room = bottom - TOP - GAP * Math.max(0, stage.nodes.length - 1)
    let y = TOP
    for (const node of stage.nodes) {
      const h = Math.max(2, (node.trace_count / total) * room)
      placed.set(node.node_id, { node, aspect: stage.aspect, stage: i, x: i * span, y, h, out: 0, in: 0 })
      y += h + GAP
    }
  })
  const scale = (p: Placed) => {
    const stage = flow.stages[p.stage]
    const total = stage.nodes.reduce((s, x) => s + x.trace_count, 0) || 1
    return (bottom - TOP - GAP * Math.max(0, stage.nodes.length - 1)) / total
  }

  // Ribbons, ordered by the node they leave and then the node they reach, so
  // they stack inside each bar without crossing there.
  const order = (id: string) => {
    const p = placed.get(id)
    return p ? p.stage * 10_000 + p.y : 0
  }
  const links = [...flow.links].sort(
    (a, b) => order(a.source_node_id) - order(b.source_node_id) || order(a.target_node_id) - order(b.target_node_id),
  )
  const ribbons = links.flatMap((l) => {
    const s = placed.get(l.source_node_id)
    const t = placed.get(l.target_node_id)
    if (!s || !t || l.trace_count === 0) return []
    const hs = l.trace_count * scale(s)
    const ht = l.trace_count * scale(t)
    const y0 = s.y + s.out
    const y1 = t.y + t.in
    s.out += hs
    t.in += ht
    const x0 = s.x + NODE_W
    const x1 = t.x
    const mid = (x0 + x1) / 2
    const d = `M${x0},${y0} C${mid},${y0} ${mid},${y1} ${x1},${y1} L${x1},${y1 + ht} C${mid},${y1 + ht} ${mid},${y0 + hs} ${x0},${y0 + hs} Z`
    const lit = hover === l.source_node_id || hover === l.target_node_id
    return [{ key: `${l.source_node_id}>${l.target_node_id}`, d, lit, link: l, s, t }]
  })

  // Room for a label beside a bar: the gap to the next column, less a margin.
  const charsAt = (stage: number) =>
    Math.max(12, Math.floor(((stage === n - 1 ? tail : span - NODE_W) - 40) / (FONT_PX * 0.56)))
  const clip = (s: string, stage: number) => {
    const chars = charsAt(stage)
    return s.length > chars ? `${s.slice(0, chars - 1)}…` : s
  }

  return (
    <div ref={box} className="w-full">
      <svg
        width={width}
        height={HEIGHT}
        viewBox={`0 0 ${width} ${HEIGHT}`}
        role="img"
        aria-label={`Flow across ${flow.stages.map((s) => labelOf(s.aspect)).join(', ')}`}
        className="block overflow-visible"
      >
        {flow.stages.map((stage, i) => {
          return (
            <text
              key={stage.aspect}
              x={i * span}
              y={12}
              className="legend"
              fill="var(--color-ink-3)"
              style={{ fontSize: 11 }}
            >
              {labelOf(stage.aspect)} · {stage.trace_count} runs
            </text>
          )
        })}

        {ribbons.map((r) => (
          <path
            key={r.key}
            d={r.d}
            fill={`color-mix(in oklab, var(--color-accent) ${r.lit ? 42 : 16}%, transparent)`}
            style={{ transition: 'fill 120ms ease-out' }}
          >
            <title>
              {r.link.trace_count} runs: {r.s.node.label} to {r.t.node.label} ({share(r.link.source_share)} of the
              first, {share(r.link.target_share)} of the second)
            </title>
          </path>
        ))}

        {[...placed.values()].map((p) => {
          const openable = p.node.kind === 'noise' || (p.node.kind === 'theme' && !!p.node.theme_id)
          const label =
            p.node.kind === 'noise' ? 'joined no theme' : p.node.kind === 'other' ? `${p.node.label}` : p.node.label
          return (
            <g
              key={p.node.node_id}
              onMouseEnter={() => setHover(p.node.node_id)}
              onMouseLeave={() => setHover(null)}
              onClick={openable ? () => onOpen(p.aspect, p.node) : undefined}
              onKeyDown={
                openable
                  ? (e) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault()
                        onOpen(p.aspect, p.node)
                      }
                    }
                  : undefined
              }
              tabIndex={openable ? 0 : undefined}
              role={openable ? 'button' : undefined}
              aria-label={openable ? `${label}, ${p.node.trace_count} runs` : undefined}
              className={openable ? 'cursor-pointer outline-none' : undefined}
            >
              <title>
                {p.node.label}: {p.node.trace_count} runs, {share(p.node.coverage)}
              </title>
              <rect
                x={p.x}
                y={p.y}
                width={NODE_W}
                height={p.h}
                rx={2}
                fill={p.node.kind === 'theme' ? 'var(--color-accent)' : 'var(--color-ink-4)'}
              />
              {p.h >= 9 && (
                <text
                  x={p.x + NODE_W + 8}
                  y={p.y + Math.min(p.h / 2, 14) + 4}
                  stroke="var(--color-panel-2)"
                  strokeWidth={3}
                  strokeLinejoin="round"
                  paintOrder="stroke"
                  fill={hover === p.node.node_id ? 'var(--color-ink)' : 'var(--color-ink-2)'}
                  style={{ fontSize: FONT_PX }}
                >
                  {clip(label, p.stage)}
                  <tspan fill="var(--color-ink-3)" className="data" style={{ fontSize: 11 }}>
                    {'  '}
                    {p.node.trace_count}
                  </tspan>
                </text>
              )}
            </g>
          )
        })}
      </svg>
    </div>
  )
}
