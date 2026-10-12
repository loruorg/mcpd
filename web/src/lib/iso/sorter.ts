/**
 * The sorting bench: six kinds of signal arrive mixed on one belt, a reader
 * reads the trace id each one carries, and a transfer table files it into the
 * drawer for that run.
 *
 * The object makes the argument SignalFlow made with arrows: correlation is by
 * the run that produced a signal, not by its kind or when it arrived. Open a
 * drawer and the pieces of that run light up on the belt, scattered among the
 * others, which is what a store that keys everything on `trace_id` sees.
 *
 * This module is the one source for the build (static art) and the browser
 * (the parts that move). Everything here is a pure function of its arguments.
 */
import * as k from './kit'
import type { Plan, Point3, Projection } from './kit'

export const WIDTH = 640
export const HEIGHT = 300

/** The signal kinds, by their wire names. Each is a part with its own proportions. */
export const KINDS = ['span', 'log', 'metric', 'score', 'feedback', 'aspect_note'] as const
export type Kind = (typeof KINDS)[number]

/** Three runs, each with its own drawer. Short trace ids, as the interface prints them. */
export const RUNS = ['4bf9', 'a3ce', '9d10'] as const

/**
 * The stream, in arrival order. Interleaved on purpose: signals of one run do
 * not arrive together, and the figure's point is that they are stored together
 * anyway. The readout counts from this same list.
 */
export const STREAM: [Kind, number][] = [
  ['span', 0], ['log', 1], ['span', 2], ['metric', 0], ['span', 1], ['score', 2],
  ['log', 0], ['span', 2], ['feedback', 1], ['span', 0], ['aspect_note', 2], ['log', 1],
  ['score', 0], ['span', 1], ['metric', 2], ['log', 2], ['span', 0], ['feedback', 0],
]

export function countsOf(run: number) {
  const out = new Map<Kind, number>()
  for (const [kind, r] of STREAM) if (r === run) out.set(kind, (out.get(kind) ?? 0) + 1)
  return KINDS.filter((kd) => out.has(kd)).map((kd) => [kd, out.get(kd)!] as const)
}

/* ------------------------------------------------------------ the world -- */

const PLATE: Plan = { x: -168, y: -60, w: 338, d: 118, r: 8 }
const THICK = 9
const BELT = { from: -150, to: 44, half: 12, bed: 8, top: 14 }
const RAIL = 3
const GANTRY = { x: -62, y: 24, top: 50 }
const HOOD: Plan = { x: -162, y: -19, w: 30, d: 38, r: 2 }
const TRANSFER: Plan = { x: 45, y: -50, w: 20, d: 100, r: 2 }
export const LANES = [-32, 0, 32]
const CAB: Plan = { x: 66, y: -50, w: 46, d: 100, r: 2 }
const CAB_H = 46
export const FRONT = CAB.x + CAB.w
const DRAWER = { half: 13, z0: 9, z1: 33, depth: 36, wall: 5 }
export const OPEN = 30

export const P: Projection = k.fit(
  [...k.boxCorners(PLATE, -THICK - 4, 0), [GANTRY.x, 0, GANTRY.top + 6], [FRONT + OPEN + 4, 0, 0], [CAB.x, CAB.y, CAB_H]],
  WIDTH,
  HEIGHT,
  22,
  58,
  30,
)

const at = (q: Point3) => k.iso(q, P)
const S = k.solid
const L = k.line

/* ---------------------------------------------------------- the pieces -- */

/**
 * One signal as a part on the belt, built at the origin and moved with a
 * transform. Proportions tell the kinds apart without colour: a span is a long
 * bar, a log a thin sheet, a metric a puck, a score a cube, a feedback a pin,
 * an aspect note a folded card.
 */
export function pieceSvg(kind: Kind, z = 0) {
  switch (kind) {
    case 'span':
      return S(k.slab({ x: -8, y: -4, w: 16, d: 8, r: 1 }, z, 4.5, P, 3, 0.4), { tone: 'hi', crease: 'lo' }) +
        L(k.onTop([[-5, 0], [5, 0]], z + 4.5, P), 'faint')
    case 'log':
      return S(k.slab({ x: -6, y: -5, w: 12, d: 10, r: 0.6 }, z, 1.4, P, 2), { tone: 'hi', crease: 'lo' }) +
        L(k.onTop([[-3.5, -2.5], [3.5, -2.5]], z + 1.4, P) + k.onTop([[-3.5, 0], [2, 0]], z + 1.4, P) + k.onTop([[-3.5, 2.5], [3, 2.5]], z + 1.4, P), 'lo')
    case 'metric':
      return S(k.cylinder(0, 0, 4.4, z, 4, P, 22, 0.4), { tone: 'hi', crease: 'lo' }) + L(k.ring(0, 0, 2, z + 4, P, 14), 'faint')
    case 'score':
      return S(k.slab({ x: -3.6, y: -3.6, w: 7.2, d: 7.2, r: 0.6 }, z, 7.2, P, 2, 0.4), { tone: 'hi', crease: 'lo' })
    case 'feedback':
      return S(k.cylinder(0, 0, 3.4, z, 1.6, P, 18), { tone: 'hi', crease: 'lo' }) +
        S(k.cylinder(0, 0, 1.6, z + 1.6, 6.4, P, 14), { tone: 'hi', crease: 'none' })
    case 'aspect_note':
      return S(k.slab({ x: -6, y: -4.5, w: 12, d: 9, r: 0.6 }, z, 2.4, P, 2), { tone: 'hi', crease: 'lo' }) +
        L(k.onTop([[0, -4.5], [0, 4.5]], z + 2.4, P), 'lo')
  }
}

/* ------------------------------------------------------- the static art -- */

function conveyor() {
  const out: string[] = []
  // Legs, far row first, on pads screwed to the plate.
  const legs = k.byDepth([[-128, -9], [-128, 9], [20, -9], [20, 9]], P)
  for (const [x, y] of legs) {
    out.push(S(k.slab({ x: x - 4, y: y - 3, w: 8, d: 6, r: 1 }, 0, 1.2, P, 2), { tone: 'lo' }))
    out.push(S(k.slab({ x: x - 2, y: y - 1.6, w: 4, d: 3.2, r: 0.6 }, 1.2, BELT.bed - 1.2, P, 2), { tone: 'mid', crease: 'none' }))
  }
  // The bed, then the belt laid on it with its two end drums seen from above.
  const bed: Plan = { x: BELT.from, y: -BELT.half - RAIL, w: BELT.to - BELT.from, d: (BELT.half + RAIL) * 2, r: 2 }
  out.push(S(k.slab(bed, BELT.bed, 3.5, P, 4, 0.6), { tone: 'mid' }))
  out.push(L(k.sideSeam(bed, BELT.bed + 1.6, P, 4), 'faint'))
  const belt: Plan = { x: BELT.from + 2, y: -BELT.half, w: BELT.to - BELT.from - 4, d: BELT.half * 2, r: 1.5 }
  out.push(S(k.slab(belt, BELT.bed + 3.5, BELT.top - BELT.bed - 3.5, P, 3), { tone: 'lo', crease: 'faint' }))
  return out.join('')
}

/** The far rail, drawn behind the pieces. */
function farRail() {
  const rail: Plan = { x: BELT.from, y: -BELT.half - RAIL, w: BELT.to - BELT.from, d: RAIL, r: 0.8 }
  return S(k.slab(rail, BELT.bed + 3.5, 6.5, P, 2, 0.3), { tone: 'mid' }) +
    k.dots([-140, -100, -60, -20, 20].map((x) => at([x, -BELT.half - RAIL / 2, BELT.bed + 10])), 0.4)
}

/** The near rail, drawn in front of the pieces it guides. */
function nearRail() {
  const rail: Plan = { x: BELT.from, y: BELT.half, w: BELT.to - BELT.from, d: RAIL, r: 0.8 }
  return S(k.slab(rail, BELT.bed + 3.5, 6.5, P, 2, 0.3), { tone: 'mid' }) +
    k.dots([-140, -100, -60, -20, 20].map((x) => at([x, BELT.half + RAIL / 2, BELT.bed + 10])), 0.4) +
    L(k.sideSeam(rail, BELT.bed + 7, P, 2), 'faint')
}

function post(y: number) {
  const out: string[] = []
  out.push(S(k.slab({ x: GANTRY.x - 6, y: y - 6, w: 12, d: 12, r: 1.5 }, 0, 3, P, 3, 0.4), { tone: 'mid' }))
  out.push(k.dots(k.corners({ x: GANTRY.x - 6, y: y - 6, w: 12, d: 12, r: 0 }, 2.2).map(([x, yy]) => at([x, yy, 3])), 0.4))
  out.push(S(k.slab({ x: GANTRY.x - 3, y: y - 3, w: 6, d: 6, r: 1 }, 3, GANTRY.top - 3, P, 3, 0.3), { tone: 'mid', crease: 'none' }))
  out.push(L(k.sideSeam({ x: GANTRY.x - 3, y: y - 3, w: 6, d: 6, r: 1 }, 22, P, 3), 'faint'))
  return out.join('')
}

/** The bridge across the belt and the reader head hanging from it. */
function bridge() {
  const out: string[] = []
  const beam: Plan = { x: GANTRY.x - 5, y: -GANTRY.y - 4, w: 10, d: GANTRY.y * 2 + 8, r: 1.5 }
  out.push(S(k.slab(beam, GANTRY.top, 6, P, 3, 0.5), { tone: 'mid' }))
  out.push(L(k.planOutline(k.inset(beam, 2), GANTRY.top + 6, P, 3), 'faint'))
  out.push(k.dots([-GANTRY.y, GANTRY.y].map((y) => at([GANTRY.x, y, GANTRY.top + 6])), 0.5))
  // The head: a carrier block, a neck and the window that reads the tag.
  out.push(S(k.slab({ x: GANTRY.x - 4, y: -6, w: 8, d: 12, r: 1 }, GANTRY.top - 8, 8, P, 3, 0.4), { tone: 'mid' }))
  out.push(S(k.slab({ x: GANTRY.x - 7, y: -9, w: 14, d: 18, r: 2 }, GANTRY.top - 20, 12, P, 4, 0.6), { tone: 'hi', crease: 'lo' }))
  out.push(L(k.sideSeam({ x: GANTRY.x - 7, y: -9, w: 14, d: 18, r: 2 }, GANTRY.top - 16, P, 4), 'faint'))
  out.push(L(k.planOutline({ x: GANTRY.x - 4, y: -6, w: 8, d: 12, r: 1 }, GANTRY.top - 8, P, 3), 'faint'))
  return out.join('')
}

function hood() {
  const out: string[] = []
  out.push(S(k.slab(HOOD, BELT.top + 1, 22, P, 3, 0.6), { tone: 'mid' }))
  out.push(L(k.sideSeam(HOOD, BELT.top + 8, P, 3), 'faint'))
  out.push(L(k.planOutline(k.inset(HOOD, 3), BELT.top + 23, P, 3), 'faint'))
  out.push(k.dots(k.corners(HOOD, 3).map(([x, y]) => at([x, y, BELT.top + 23])), 0.45))
  // The intake port on top: the one OTLP endpoint every signal enters by.
  out.push(S(k.cylinder(HOOD.x + 15, 0, 7, BELT.top + 23, 3, P, 26, 0.4), { tone: 'mid' }))
  out.push(S(k.cylinder(HOOD.x + 15, 0, 4.6, BELT.top + 26, 2.4, P, 22), { tone: 'hi', crease: 'lo' }))
  out.push(L(k.ring(HOOD.x + 15, 0, 2.6, BELT.top + 28.4, P, 18), 'lo'))
  return out.join('')
}

function transfer() {
  const out: string[] = []
  out.push(S(k.slab(TRANSFER, 0, BELT.top - 1, P, 3, 0.6), { tone: 'mid' }))
  // Guide slots, one per lane, that the pusher rides in.
  for (const y of LANES) out.push(L(k.planOutline({ x: TRANSFER.x + 3, y: y - 1.2, w: TRANSFER.w - 6, d: 2.4, r: 1.2 }, BELT.top - 1, P, 3), 'faint'))
  out.push(L(k.onTop([[TRANSFER.x + 2, -BELT.half], [TRANSFER.x + 2, BELT.half]], BELT.top - 1, P), 'lo'))
  out.push(k.dots(k.corners(TRANSFER, 3).map(([x, y]) => at([x, y, BELT.top - 1])), 0.45))
  return out.join('')
}

function cabinet() {
  const out: string[] = []
  out.push(S(k.slab(CAB, 0, 4, P, 3, 0.4), { tone: 'lo' }))
  const body = k.inset(CAB, 1.5)
  out.push(S(k.slab(body, 4, CAB_H - 4, P, 3, 0.8), { tone: 'mid' }))
  out.push(L(k.planOutline(k.inset(body, 3), CAB_H, P, 3), 'faint'))
  out.push(k.dots(k.corners(body, 3.5).map(([x, y]) => at([x, y, CAB_H])), 0.5))
  // The three slots on top, where a transfer drops a piece into its drawer.
  for (const y of LANES) out.push(L(k.planOutline({ x: CAB.x + 6, y: y - 8, w: 8, d: 16, r: 1.5 }, CAB_H, P, 3), 'lo'))
  // The recess each drawer front sits in, on the front face.
  for (const y of LANES) {
    const fx = FRONT - 1.5
    const r = DRAWER.half + 1
    out.push(L(k.pathOf([at([fx, y - r, DRAWER.z0 - 1]), at([fx, y + r, DRAWER.z0 - 1]), at([fx, y + r, DRAWER.z1 + 1]), at([fx, y - r, DRAWER.z1 + 1])], true), 'faint'))
  }
  return out.join('')
}

/** A drawer front, with its pull, label plate and lamp, built closed. */
export function drawerFront(lane: number) {
  const y = LANES[lane]!
  const fx = FRONT - 1.5
  const plan: Plan = { x: fx - 1, y: y - DRAWER.half, w: 4, d: DRAWER.half * 2, r: 0.8 }
  const out: string[] = []
  out.push(S(k.slab(plan, DRAWER.z0, DRAWER.z1 - DRAWER.z0, P, 2, 0.4), { tone: 'mid' }))
  const face = fx + 3
  // Label plate with two rule lines that stand for the printed trace id.
  out.push(L(k.pathOf([at([face, y - 8, DRAWER.z1 - 3]), at([face, y + 8, DRAWER.z1 - 3]), at([face, y + 8, DRAWER.z1 - 9]), at([face, y - 8, DRAWER.z1 - 9])], true), 'lo'))
  out.push(L(k.pathOf([at([face, y - 6, DRAWER.z1 - 5]), at([face, y + 3, DRAWER.z1 - 5])]) + k.pathOf([at([face, y - 6, DRAWER.z1 - 7]), at([face, y + 1, DRAWER.z1 - 7])]), 'faint'))
  // The pull: two standoffs and a bar.
  for (const dy of [-6, 6]) out.push(S(k.slab({ x: face, y: y + dy - 1, w: 2.2, d: 2, r: 0.5 }, DRAWER.z0 + 6, 2, P, 2), { tone: 'mid', crease: 'none' }))
  out.push(S(k.slab({ x: face + 2.2, y: y - 7.5, w: 2, d: 15, r: 0.8 }, DRAWER.z0 + 5.6, 2.8, P, 2, 0.3), { tone: 'hi', crease: 'lo' }))
  const [lx, ly] = at([face, y + DRAWER.half - 3, DRAWER.z1 - 6])
  out.push(`<circle class="iso-lamp" data-lamp="${lane}" cx="${lx.toFixed(1)}" cy="${ly.toFixed(1)}" r="1.5"/>`)
  return out.join('')
}

/**
 * The visible part of an open drawer's tray and what is in it.
 *
 * Only the length outside the cabinet is drawn, rebuilt for each opening, so a
 * tray never has to be clipped against a cabinet that is in front of it: what
 * is still inside is not drawn yet.
 */
export function trayAt(lane: number, open: number) {
  if (open < 0.5) return ''
  const y = LANES[lane]!
  const x0 = FRONT - 1.5
  const x1 = x0 + open
  const w = DRAWER.half - 1
  const z = DRAWER.z0
  const out: string[] = []
  const floor: Plan = { x: x0, y: y - w, w: open, d: w * 2, r: 0 }
  out.push(S(k.slab(floor, z, 1.4, P, 1), { tone: 'lo', crease: 'none' }))
  out.push(S(k.slab({ x: x0, y: y - w, w: open, d: 1.2, r: 0 }, z + 1.4, DRAWER.wall, P, 1), { tone: 'mid', crease: 'faint' }))
  // Contents in two rows, the run's pieces in stream order.
  const pieces = STREAM.filter(([, r]) => r === lane).map(([kd]) => kd)
  pieces.forEach((kd, i) => {
    const col = Math.floor(i / 2)
    const px = x1 - 7 - col * 9
    const py = y + (i % 2 ? 5 : -5)
    if (px - 4 < x0) return
    out.push(`<g transform="${k.translateAlong(P, [px, py, z + 1.4])}" data-sel="">${smallPiece(kd)}</g>`)
  })
  out.push(S(k.slab({ x: x0, y: y + w - 1.2, w: open, d: 1.2, r: 0 }, z + 1.4, DRAWER.wall, P, 1), { tone: 'mid', crease: 'faint' }))
  return out.join('')
}

const small = new Map<Kind, string>()
function smallPiece(kd: Kind) {
  if (!small.has(kd)) {
    const svg = {
      span: S(k.slab({ x: -3.6, y: -2, w: 7.2, d: 4, r: 0.5 }, 0, 2.4, P, 2), { tone: 'hi', crease: 'lo' }),
      log: S(k.slab({ x: -3, y: -2.4, w: 6, d: 4.8, r: 0.3 }, 0, 0.8, P, 1), { tone: 'hi', crease: 'lo' }),
      metric: S(k.cylinder(0, 0, 2.4, 0, 2.2, P, 16), { tone: 'hi', crease: 'lo' }),
      score: S(k.slab({ x: -2, y: -2, w: 4, d: 4, r: 0.3 }, 0, 4, P, 1), { tone: 'hi', crease: 'lo' }),
      feedback: S(k.cylinder(0, 0, 1.8, 0, 0.9, P, 14), { tone: 'hi', crease: 'lo' }) + S(k.cylinder(0, 0, 0.9, 0.9, 3.4, P, 10), { tone: 'hi', crease: 'none' }),
      aspect_note: S(k.slab({ x: -3, y: -2.2, w: 6, d: 4.4, r: 0.3 }, 0, 1.2, P, 1), { tone: 'hi', crease: 'lo' }),
    }[kd]
    small.set(kd, svg)
  }
  return small.get(kd)!
}

/* ------------------------------------------------------------- motion -- */

/** Seconds between arrivals, and how long each leg of the trip takes. */
export const GAP = 1.15
const BELT_S = 4.6
const SHIFT_S = 0.7
const DROP_S = 0.55
export const TRIP = BELT_S + SHIFT_S + DROP_S
export const LOOP = STREAM.length * GAP

const ease = (t: number) => (t <= 0 ? 0 : t >= 1 ? 1 : t * t * (3 - 2 * t))
export const BELT_V = (BELT.to - 2 - (HOOD.x + 8)) / BELT_S

/**
 * Where piece `i` is at `clock`, or null when it is not on the bench. One
 * function of time, so the static frame, the reduced-motion frame and every
 * animated frame agree about where everything is.
 */
export function placeOf(i: number, clock: number): { x: number; y: number; z: number; leg: 'belt' | 'table' } | null {
  const lane = STREAM[i]![1]
  // Every piece has an arrival time in each loop; take the one in flight now.
  let age = (clock - i * GAP) % LOOP
  if (age < 0) age += LOOP
  if (age > TRIP) return null
  const startX = HOOD.x + 8
  if (age < BELT_S) return { x: startX + BELT_V * age, y: 0, z: BELT.top, leg: 'belt' }
  const endX = BELT.to - 2
  if (age < BELT_S + SHIFT_S) {
    const s = ease((age - BELT_S) / SHIFT_S)
    return { x: endX + (TRANSFER.x + 10 - endX) * s, y: LANES[lane]! * s, z: BELT.top - 1, leg: 'table' }
  }
  const s = ease((age - BELT_S - SHIFT_S) / DROP_S)
  return { x: TRANSFER.x + 10 + (CAB.x + 14 - TRANSFER.x - 10) * s, y: LANES[lane]!, z: BELT.top - 1, leg: 'table' }
}

/** Has piece i landed in its drawer? Seconds since it did, or -1. */
export function landedAgo(i: number, clock: number) {
  let age = (clock - i * GAP) % LOOP
  if (age < 0) age += LOOP
  const since = age - TRIP
  return since >= 0 && since < 1.2 ? since : -1
}

/** Is a piece under the reader head right now? Its index, or -1. */
export function underReader(clock: number) {
  for (let i = 0; i < STREAM.length; i++) {
    const q = placeOf(i, clock)
    if (q && q.leg === 'belt' && Math.abs(q.x - GANTRY.x) < 9) return i
  }
  return -1
}

/** The belt's cross cleats at this offset, as one path. */
export function cleats(offset: number) {
  const out: string[] = []
  const step = 12
  const from = BELT.from + 4
  const to = BELT.to - 4
  for (let x = from + (offset % step); x <= to; x += step) out.push(k.onTop([[x, -BELT.half + 1.5], [x, BELT.half - 1.5]], BELT.top, P))
  return out.join('')
}

export const READER_LAMP = at([GANTRY.x + 7, 0, GANTRY.top - 14])
export const depthOf = (x: number, y: number) => k.depthOf([x, y, 0], P)
export const translate = (x: number, y: number, z: number) => k.translateAlong(P, [x, y, z])

/** Screen position of each drawer front, for pointer picking. */
export const DRAWER_AT = LANES.map((y) => at([FRONT, y, (DRAWER.z0 + DRAWER.z1) / 2]))

/** The layers, back to front. The live script fills the empty groups. */
export function art() {
  return {
    back: [k.bench(PLATE, THICK, P, { y: PLATE.y + PLATE.d - 13, from: PLATE.x + 14, to: PLATE.x + 150 }), conveyor(), farRail(), post(-GANTRY.y)].join(''),
    afterBelt: [nearRail(), hood(), post(GANTRY.y), bridge()].join(''),
    table: transfer(),
    cabinet: cabinet(),
  }
}
