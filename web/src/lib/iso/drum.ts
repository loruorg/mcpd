/**
 * The run drum: the specimen run wound round a recorder drum.
 *
 * Time runs round the drum and each span has its own track along the axis, so
 * the paper is the waterfall rolled into a cylinder. A pen bar lies across the
 * drum; turning the drum moves the run under it, and the readout names the
 * span the pen is on. The WebGL figure and its SVG fallback both read the
 * mapping below, and both read the rows from `@/lib/specimen`.
 */
import * as k from './kit'
import type { Plan, Point3 } from './kit'
import { SPECIMEN, SPECIMEN_MS } from '@/lib/specimen'

export const WIDTH = 640
export const HEIGHT = 300

/** The run occupies this share of the paper; the rest is the clip that holds it. */
export const U0 = 0.04
export const U1 = 0.88
/** Tracks sit between these shares of the drum's length, one per span. */
export const V0 = 0.1
export const V1 = 0.9

/** The pen's angle on the drum, from the top toward the viewer, in radians. */
export const PEN = 0.7

export const uOf = (ms: number) => U0 + (ms / SPECIMEN_MS) * (U1 - U0)
export const msOf = (u: number) => ((u - U0) / (U1 - U0)) * SPECIMEN_MS

const wrap = (x: number) => x - Math.floor(x)

/** Where on the paper the pen is when the drum has turned by `turn` radians. */
export const penU = (turn: number) => wrap((PEN - turn) / (Math.PI * 2))
/** The turn that puts time `ms` under the pen. */
export const turnFor = (ms: number) => PEN - uOf(ms) * Math.PI * 2

/** The span the pen reads at `ms`: the deepest one open at that moment. */
export function spanAt(ms: number) {
  let best = -1
  SPECIMEN.forEach((r, i) => {
    if (ms >= r.at && ms <= r.at + r.ms && (best < 0 || r.depth >= SPECIMEN[best]!.depth)) best = i
  })
  return best
}

const secs = (ms: number) => (ms >= 1000 ? `${(ms / 1000).toFixed(2)} s` : `${ms} ms`)

export function readoutAt(u: number) {
  if (u < U0 || u > U1) return `${secs(SPECIMEN_MS)} · end of run · ${SPECIMEN.length} spans`
  const ms = Math.round(msOf(u))
  const i = spanAt(ms)
  const r = SPECIMEN[i]
  if (!r) return `${secs(ms)}`
  return `${(ms / 1000).toFixed(2)} s · ${r.kind} ${r.name} · ${secs(r.ms)}${r.cost ? ` · ${r.cost}` : ''}${r.status === 'error' ? ' · fail' : ''}`
}

/** Paper spans, for the shader: start u, end u, track index, kind index. */
export const KIND_ORDER = ['agent', 'model', 'tool', 'memory', 'guardrail']
export const SPANS = SPECIMEN.map((r, i) => [uOf(r.at), uOf(r.at + r.ms), i, Math.max(0, KIND_ORDER.indexOf(r.kind))] as const)
export const TRACKS = SPECIMEN.length

/** The token each kind of bar is painted in, the same map the waterfall uses. */
export const KIND_TOKEN = ['--color-ink', '--color-slate', '--color-amber', '--color-ink-4', '--color-oxide']

/* ------------------------------------------------------ the SVG fallback -- */

/*
 * The same drum drawn with the isometric kit, at rest with the pen on the
 * first tool call. It is what a reader sees with WebGL off, and what the
 * canvas fades in over once its first frame is drawn.
 */
const R = 40
const HALF = 50
const AXIS = 62
const PLATE: Plan = { x: -86, y: -46, w: 172, d: 92, r: 6 }
const P = k.fit([...k.boxCorners(PLATE, -12, 0), [0, 0, AXIS + R + 6], [HALF, 0, AXIS + R]], WIDTH, HEIGHT, 24, 60, 26)
const at = (q: Point3) => k.iso(q, P)
const S = k.solid
const L = k.line

export const REST_MS = 2640
const REST_TURN = turnFor(REST_MS)

function view(): Point3 {
  const a = (P.azimuth * Math.PI) / 180
  const e = (P.elevation * Math.PI) / 180
  return [Math.cos(a) * Math.cos(e), Math.sin(a) * Math.cos(e), Math.sin(e)]
}

/** A point on the drum's surface: `theta` from the top toward +y. */
const onDrum = (x: number, theta: number, r = R): Point3 => [x, r * Math.sin(theta), AXIS + r * Math.cos(theta)]

function hull(points: k.Flat[]): k.Flat[] {
  const s = [...points].sort((a, b) => a[0] - b[0] || a[1] - b[1])
  const cross = (o: k.Flat, a: k.Flat, b: k.Flat) => (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
  const lo: k.Flat[] = []
  for (const q of s) {
    while (lo.length >= 2 && cross(lo[lo.length - 2]!, lo[lo.length - 1]!, q) <= 0) lo.pop()
    lo.push(q)
  }
  const up: k.Flat[] = []
  for (let i = s.length - 1; i >= 0; i--) {
    const q = s[i]!
    while (up.length >= 2 && cross(up[up.length - 2]!, up[up.length - 1]!, q) <= 0) up.pop()
    up.push(q)
  }
  return [...lo.slice(0, -1), ...up.slice(0, -1)]
}

/** A round part along x, from x0 to x1, its +x end face toward the viewer. */
function roundX(x0: number, x1: number, r: number, tone: k.Tone = 'mid', face = 'iso-top') {
  const ring = (x: number) => Array.from({ length: 48 }, (_, i) => at(onDrum(x, (i / 48) * Math.PI * 2, r)))
  const body = hull([...ring(x0), ...ring(x1)])
  const end = ring(x1)
  return `<g class="iso-solid"><path class="iso-fill" d="${k.pathOf(body, true)}"/><path class="iso-shade" data-shade="2" d="${k.pathOf(body, true)}"/><path class="${face}" d="${k.pathOf(end, true)}"/><path class="iso-line iso-edge" data-tone="${tone}" d="${k.pathOf(body, true)}"/><path class="iso-line" data-tone="faint" d="${k.pathOf(end, true)}"/></g>`
}

function cheek(x: number) {
  const plan: Plan = { x: x - 4, y: -14, w: 8, d: 28, r: 2 }
  return S(k.slab(plan, 0, AXIS + 4, P, 3, 0.6), { tone: 'mid' }) + roundX(x - 2, x + 6, 9, 'mid')
}

export function fallbackSvg() {
  const out: string[] = []
  out.push(k.bench(PLATE, 8, P, { y: PLATE.y + PLATE.d - 12, from: PLATE.x + 12, to: PLATE.x + 110 }))
  out.push(cheek(-HALF - 8))
  out.push(roundX(-HALF - 3, -HALF, R + 3, 'mid'))
  // The paper: body first, then each span's bar on the half that faces you.
  out.push(`<g class="iso-solid"><path class="iso-fill" d="${k.pathOf(hull([...ring(-HALF), ...ring(HALF)]), true)}" style="fill:var(--iso-chart)"/><path class="iso-line iso-edge" data-tone="mid" d="${k.pathOf(hull([...ring(-HALF), ...ring(HALF)]), true)}"/></g>`)
  const V = view()
  const faces = (theta: number) => Math.sin(theta) * V[1] + Math.cos(theta) * V[2] > 0.05
  const trackX = (i: number) => -HALF + (V0 + ((i + 0.5) / TRACKS) * (V1 - V0)) * HALF * 2
  for (let i = 0; i <= TRACKS; i++) {
    const x = -HALF + (V0 + (i / TRACKS) * (V1 - V0)) * HALF * 2
    out.push(L(visibleArc(x, 0, 1, faces), 'faint'))
  }
  SPANS.forEach(([u0, u1, track, kind]) => {
    const d = visibleArc(trackX(track), u0, u1, faces)
    if (d) out.push(`<path d="${d}" fill="none" stroke="var(${KIND_TOKEN[kind]})" stroke-width="5" stroke-linecap="butt"/>`)
  })
  out.push(roundX(HALF, HALF + 3, R + 3, 'mid'))
  // The pen bar across the drum, and its two arms.
  const bar = (x: number) => onDrum(x, PEN, R + 4)
  out.push(`<path class="iso-line" data-tone="hi" style="stroke-width:2.4px" d="${k.pathOf([at(bar(-HALF - 6)), at(bar(HALF + 6))])}"/>`)
  out.push(cheek(HALF + 8))
  return out.join('')

  function ring(x: number) {
    return Array.from({ length: 48 }, (_, i) => at(onDrum(x, (i / 48) * Math.PI * 2)))
  }
  function visibleArc(x: number, u0: number, u1: number, ok: (t: number) => boolean) {
    const pts: k.Flat[] = []
    const parts: string[] = []
    const n = Math.max(2, Math.ceil((u1 - u0) * 160))
    for (let j = 0; j <= n; j++) {
      const u = u0 + ((u1 - u0) * j) / n
      const theta = u * Math.PI * 2 + REST_TURN
      if (ok(theta)) pts.push(at(onDrum(x, theta, R + 0.3)))
      else if (pts.length) {
        parts.push(k.pathOf(pts))
        pts.length = 0
      }
    }
    if (pts.length > 1) parts.push(k.pathOf(pts))
    return parts.join('')
  }
}
