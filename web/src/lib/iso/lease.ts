/**
 * The lease board: the daemon's board holds one tag per step of a run; a
 * worker holds a tag only while its heartbeats arrive.
 *
 * The numbers are the daemon's defaults: a lease of 30 s (`lease_ttl`), a
 * heartbeat every third of that, and a reclaim window of one lease length
 * after a lease lapses. A tag whose window closes goes back on the board
 * without spending the retry budget, and the other worker claims it. The
 * status words are the attempt statuses the API returns.
 *
 * The board runs on simulated seconds, `SPEED` of them per real second, so a
 * 30 s lease plays in five. The model is a pure state machine: the build runs
 * it to `START` to draw the first frame, and the page runs the same code on.
 */
import * as k from './kit'
import type { Plan, Point3, Projection } from './kit'

export const WIDTH = 640
export const HEIGHT = 300

export const LEASE = 30
export const HEARTBEAT = LEASE / 3
export const RECLAIM = LEASE
export const SPEED = 6

export interface Step {
  id: string
  /** Seconds of work in the body, or of waiting for a `wait_event`. */
  secs: number
  wait?: boolean
}
export const STEPS: Step[] = [
  { id: 'classify', secs: 18 },
  { id: 'draft', secs: 26 },
  { id: 'approval', secs: 36, wait: true },
  { id: 'send', secs: 14 },
]
export const WORKERS = ['worker-a', 'worker-b']

/* ------------------------------------------------------------ the model -- */

export type Phase = 'pending' | 'out' | 'run' | 'back' | 'wait' | 'done'

export interface State {
  t: number
  step: number
  phase: Phase
  /** Simulated seconds spent in this phase. */
  inPhase: number
  worker: number
  alive: [boolean, boolean]
  body: number
  lastBeat: number
  /** How the tag came back: `succeeded`, or `pending` after a reclaim. */
  returned: 'succeeded' | 'pending'
  done: boolean[]
  /** The tour stops the first worker once, in the second step, unless the reader takes over. */
  autoStop: boolean
}

export const initial = (): State => ({
  t: 0, step: 0, phase: 'pending', inPhase: 0, worker: 0, alive: [true, true], body: 0,
  lastBeat: 0, returned: 'succeeded', done: STEPS.map(() => false), autoStop: true,
})

/** Simulated seconds a tag takes to fly between the board and a worker. */
export const FLIGHT = 0.7 * SPEED
const PAUSE = 0.5 * SPEED

const go = (s: State, phase: Phase) => {
  s.phase = phase
  s.inPhase = 0
}

/** Advance the model by `dt` simulated seconds. */
export function advance(s: State, dt: number) {
  s.t += dt
  s.inPhase += dt
  const step = STEPS[s.step]!
  switch (s.phase) {
    case 'pending':
      if (s.inPhase < PAUSE) break
      if (step.wait) {
        go(s, 'wait')
        break
      }
      // Claim goes to a live worker; a stopped one claims nothing.
      s.worker = s.alive[s.worker] ? s.worker : 1 - s.worker
      if (!s.alive[s.worker]) break
      s.body = 0
      go(s, 'out')
      break
    case 'out':
      if (s.inPhase >= FLIGHT) {
        s.lastBeat = s.t
        go(s, 'run')
      }
      break
    case 'run': {
      const alive = s.alive[s.worker]
      if (alive) {
        s.body += dt
        if (s.t - s.lastBeat >= HEARTBEAT) s.lastBeat = s.t
        if (s.autoStop && s.step === 1 && s.body >= 9) {
          s.alive[s.worker] = false
          s.autoStop = false
        }
        if (s.body >= step.secs) {
          s.returned = 'succeeded'
          go(s, 'back')
        }
      } else if (s.t - s.lastBeat >= LEASE + RECLAIM) {
        // The window closed with no contact: requeued, without spending a retry.
        s.returned = 'pending'
        go(s, 'back')
      }
      break
    }
    case 'back':
      if (s.inPhase >= FLIGHT) {
        if (s.returned === 'succeeded') {
          s.done[s.step] = true
          s.step += 1
          if (s.step >= STEPS.length) {
            go(s, 'done')
            break
          }
        } else {
          // The stopped worker comes back up once its step is in other hands.
          const stopped = s.worker
          s.worker = 1 - stopped
          setTimeout0(s, stopped)
        }
        go(s, 'pending')
      }
      break
    case 'wait':
      if (s.inPhase >= step.secs) {
        s.done[s.step] = true
        s.step += 1
        go(s, 'pending')
      }
      break
    case 'done':
      if (s.inPhase >= 3 * SPEED) {
        const autoStop = s.autoStop
        Object.assign(s, initial(), { autoStop })
      }
      break
  }
}

/** A restarted worker is up again by the time the next claim is made. */
function setTimeout0(s: State, w: number) {
  s.alive[w] = true
}

/** The reader stops the worker holding the tag, or restarts a stopped one. */
export function toggle(s: State) {
  s.autoStop = false
  if (s.phase === 'run' || s.phase === 'out') s.alive[s.worker] = !s.alive[s.worker]
  else s.alive = [true, true]
}

export type Status = 'pending' | 'leased' | 'started' | 'orphaned' | 'succeeded'

export function statusOf(s: State): Status {
  if (s.phase === 'out') return 'leased'
  if (s.phase === 'run') return s.t - s.lastBeat > LEASE ? 'orphaned' : 'started'
  if (s.phase === 'back') return s.returned
  return 'pending'
}

export function readoutOf(s: State) {
  if (s.phase === 'done') return `run succeeded · ${STEPS.length} steps`
  const step = STEPS[s.step]!
  if (s.phase === 'wait') return `${step.id} · wait_event · no worker · ${Math.ceil(step.secs - s.inPhase)} s`
  const st = statusOf(s)
  const who = s.phase === 'pending' ? 'on the board' : WORKERS[s.worker]
  const since = s.t - s.lastBeat
  const clock =
    s.phase !== 'run' ? '' : since <= LEASE ? ` · lease ${Math.ceil(LEASE - since)} s` : ` · reclaim ${Math.ceil(LEASE + RECLAIM - since)} s`
  return `${step.id} · ${who} · ${st}${clock}`
}

/** Seconds since the last heartbeat, what the dial's needle shows. */
export const sinceBeat = (s: State) => (s.phase === 'run' ? Math.min(LEASE + RECLAIM, s.t - s.lastBeat) : 0)

/* ------------------------------------------------------------ the world -- */

const PLATE: Plan = { x: -156, y: -62, w: 312, d: 104, r: 8 }
const THICK = 9
const BOARD: Plan = { x: -136, y: -50, w: 272, d: 8, r: 1.5 }
const BOARD_H = 68
const FACE = BOARD.y + BOARD.d
export const HOOKS = [-108, -70, -32, 6]
const HOOK_Z = 44
const DIAL = { x: 84, z: 36, r: 22 }
export const UNITS = [-78, 40]
const UNIT_Y = 4
const UNIT_D = 26
const UNIT_TOP = 14

export const P: Projection = k.fit(
  [...k.boxCorners(PLATE, -THICK - 4, 0), [BOARD.x, BOARD.y, BOARD_H + 4], [BOARD.x + BOARD.w, BOARD.y, BOARD_H + 4]],
  WIDTH,
  HEIGHT,
  20,
  58,
  30,
)
const at = (q: Point3) => k.iso(q, P)
const S = k.solid
const L = k.line

/**
 * Draw on the board's front face in flat coordinates: `u` along the board, `v`
 * downward from height `z0`. Everything inside the returned transform lies in
 * the face plane, so a dial drawn as circles is foreshortened with the board.
 */
export function faceMatrix(y: number, z0 = 0) {
  const o = at([0, y, z0])
  const u = k.screenVector(P, [1, 0, 0])
  const v = k.screenVector(P, [0, 0, -1])
  const f = (n: number) => Math.round(n * 1000) / 1000
  return `matrix(${f(u[0])} ${f(u[1])} ${f(v[0])} ${f(v[1])} ${f(o[0])} ${f(o[1])})`
}

/**
 * A short round boss whose axis points out of the board (along +y), shaded as
 * a solid: the hull of its two end circles is the barrel, the near circle its
 * face.
 */
function boss(x: number, z: number, r: number, y0: number, y1: number, tone: k.Tone = 'mid', face = 'iso-top') {
  const ring = (y: number) =>
    Array.from({ length: 40 }, (_, i) => {
      const a = (i / 40) * Math.PI * 2
      return at([x + r * Math.cos(a), y, z + r * Math.sin(a)])
    })
  const back = ring(y0)
  const front = ring(y1)
  const hullPts = hull2([...back, ...front])
  return `<g class="iso-solid"><path class="iso-fill" d="${k.pathOf(hullPts, true)}"/><path class="iso-shade" data-shade="1" d="${k.pathOf(hullPts, true)}"/><path class="${face}" d="${k.pathOf(front, true)}"/><path class="iso-line iso-edge" data-tone="${tone}" d="${k.pathOf(hullPts, true)}"/><path class="iso-line" data-tone="faint" d="${k.pathOf(front, true)}"/></g>`
}

function hull2(points: k.Flat[]): k.Flat[] {
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

/* -------------------------------------------------------- static parts -- */

function board() {
  const out: string[] = []
  const plinth: Plan = { x: BOARD.x - 4, y: BOARD.y - 4, w: BOARD.w + 8, d: BOARD.d + 8, r: 2 }
  out.push(S(k.slab(plinth, 0, 5, P, 3, 0.5), { tone: 'mid' }))
  out.push(k.dots(k.corners(plinth, 2.5).map(([x, y]) => at([x, y, 5])), 0.45))
  out.push(S(k.slab(BOARD, 5, BOARD_H - 5, P, 3, 0.8), { tone: 'mid' }))
  out.push(L(k.planOutline(k.inset(BOARD, 2), BOARD_H, P, 3), 'faint'))
  // Face furniture in the face plane: a scribed border, the screws, and under
  // each hook a lamp and a label plate whose rules stand for the step's name.
  const face: string[] = []
  face.push(`<rect class="iso-line" data-tone="faint" x="${BOARD.x + 5}" y="5" width="${BOARD.w - 10}" height="${BOARD_H - 12}" rx="1.5"/>`)
  for (const [u, v] of [[BOARD.x + 9, 9], [BOARD.x + BOARD.w - 9, 9], [BOARD.x + 9, BOARD_H - 11], [BOARD.x + BOARD.w - 9, BOARD_H - 11]]) {
    face.push(`<circle class="iso-line" data-tone="lo" cx="${u}" cy="${v}" r="1.6"/><circle cx="${u}" cy="${v}" r="0.45" fill="var(--iso-mid)"/>`)
  }
  HOOKS.forEach((u) => {
    face.push(`<rect class="iso-line" data-tone="lo" x="${u - 9}" y="${BOARD_H - 22}" width="18" height="8" rx="0.8"/>`)
    face.push(`<path class="iso-line" data-tone="faint" d="M${u - 6} ${BOARD_H - 19.5}h9M${u - 6} ${BOARD_H - 16.5}h6"/>`)
  })
  // The dial's scale, 0 to 60 s over 270 degrees: the lease, then the reclaim window.
  const arc = (from: number, to: number, r: number) => {
    const a0 = angleOf(from)
    const a1 = angleOf(to)
    const p0 = [DIAL.x + r * Math.sin(a0), -r * Math.cos(a0)]
    const p1 = [DIAL.x + r * Math.sin(a1), -r * Math.cos(a1)]
    return `M${p0[0]!.toFixed(2)} ${p0[1]!.toFixed(2)}A${r} ${r} 0 ${a1 - a0 > Math.PI ? 1 : 0} 1 ${p1[0]!.toFixed(2)} ${p1[1]!.toFixed(2)}`
  }
  const dial: string[] = []
  dial.push(`<circle cx="${DIAL.x}" cy="0" r="${DIAL.r - 3}" fill="var(--iso-top)"/>`)
  dial.push(`<path class="iso-line" data-tone="mid" d="${arc(0, LEASE, DIAL.r - 5)}"/>`)
  dial.push(`<path class="iso-line" data-tone="lit" style="stroke-width:1.6px" d="${arc(LEASE, LEASE + RECLAIM, DIAL.r - 5.6)}"/>`)
  for (let s = 0; s <= LEASE + RECLAIM; s += 5) {
    const a = angleOf(s)
    const big = s % HEARTBEAT === 0
    const r0 = DIAL.r - 5
    const r1 = r0 - (big ? 4 : 2.2)
    dial.push(`<path class="iso-line" data-tone="${big ? 'mid' : 'lo'}" d="M${(DIAL.x + r0 * Math.sin(a)).toFixed(2)} ${(-r0 * Math.cos(a)).toFixed(2)}L${(DIAL.x + r1 * Math.sin(a)).toFixed(2)} ${(-r1 * Math.cos(a)).toFixed(2)}"/>`)
  }
  out.push(`<g transform="${faceMatrix(FACE, BOARD_H)}">${face.join('')}</g>`)
  out.push(boss(DIAL.x, DIAL.z, DIAL.r, FACE, FACE + 3))
  out.push(`<g transform="${faceMatrix(FACE + 3, DIAL.z)}">${dial.join('')}<circle class="iso-line" data-tone="lo" cx="${DIAL.x}" cy="0" r="${DIAL.r - 3}"/></g>`)
  return out.join('')
}

/** Dial angle for `s` seconds: -135 degrees at zero, +135 at a full reclaim window. */
export const angleOf = (s: number) => ((-135 + (270 * s) / (LEASE + RECLAIM)) * Math.PI) / 180

/** The needle, in the dial's face coordinates, for `s` seconds since a heartbeat. */
export function needleAt(s: number) {
  const a = angleOf(s)
  const r = DIAL.r - 7
  return `M${(DIAL.x - 3 * Math.sin(a)).toFixed(2)} ${(3 * Math.cos(a)).toFixed(2)}L${(DIAL.x + r * Math.sin(a)).toFixed(2)} ${(-r * Math.cos(a)).toFixed(2)}`
}
export const NEEDLE_FRAME = () => faceMatrix(FACE + 3.2, DIAL.z)
export const DIAL_HUB = DIAL.x

/** The back half of each hook, from the board to the tag. */
function hooksBack() {
  return HOOKS.map((x) => S(k.slab({ x: x - 3, y: FACE - 0.5, w: 6, d: 1.5, r: 0.6 }, HOOK_Z - 3, 6, P, 2), { tone: "mid" }) + S(k.slab({ x: x - 1, y: FACE + 1, w: 2, d: 0.2, r: 0.3 }, HOOK_Z - 1, 2, P, 1), { tone: "mid", crease: "none" })).join("")
}
/** The front half, which passes through the tag's hole and holds it. */
function hooksFront() {
  return HOOKS.map((x) => S(k.slab({ x: x - 1, y: FACE + 2.6, w: 2, d: 3.4, r: 0.4 }, HOOK_Z - 1, 2, P, 2), { tone: "hi", crease: "lo" }) + S(k.slab({ x: x - 1.4, y: FACE + 5.6, w: 2.8, d: 1, r: 0.4 }, HOOK_Z - 1, 3.4, P, 2), { tone: "hi", crease: "lo" })).join("")
}

/** A step lamp on the board, as a dot in the face plane. Positions for the live script. */
export const LAMPS = HOOKS.map((x) => at([x, FACE, HOOK_Z + 12]))

function rail() {
  const out: string[] = []
  const bar: Plan = { x: -140, y: UNIT_Y + UNIT_D / 2 - 3, w: 280, d: 6, r: 1 }
  // A cable tray the two workers sit on: the network between them and the daemon.
  out.push(S(k.slab(bar, 0, 2.4, P, 2, 0.3), { tone: 'lo' }))
  out.push(L(k.onTop([[bar.x + 3, bar.y + 3], [bar.x + bar.w - 3, bar.y + 3]], 2.4, P), 'faint'))
  return out.join('')
}

function unit(w: number) {
  const c = UNITS[w]!
  const plan: Plan = { x: c - 20, y: UNIT_Y, w: 40, d: UNIT_D, r: 2.5 }
  const out: string[] = []
  out.push(k.halo(k.inset(plan, 3), 0, P))
  out.push(S(k.slab(plan, 2.4, UNIT_TOP - 2.4, P, 4, 0.7), { tone: 'mid' }))
  out.push(L(k.sideSeam(plan, 6, P, 4), 'faint'))
  out.push(k.dots(k.corners(plan, 3).map(([x, y]) => at([x, y, UNIT_TOP])), 0.45))
  // Worker number, engraved as one or two dots.
  out.push(k.dots(Array.from({ length: w + 1 }, (_, i) => at([c - 17 + i * 3, UNIT_Y + UNIT_D - 3, UNIT_TOP])), 0.7, 'hi'))
  // The holder the tag stands in.
  out.push(S(k.slab({ x: c - 15, y: UNIT_Y + 4, w: 16, d: 8, r: 1 }, UNIT_TOP, 4, P, 3, 0.4), { tone: 'mid' }))
  out.push(L(k.planOutline({ x: c - 13.5, y: UNIT_Y + 7, w: 13, d: 2, r: 0.6 }, UNIT_TOP + 4, P, 2), 'lo'))
  // The body's progress dial, face up.
  out.push(S(k.cylinder(c + 9, UNIT_Y + 14, 7.5, UNIT_TOP, 2.6, P, 30, 0.4), { tone: 'hi', crease: 'lo' }))
  const ticks = k.radialTicks(c + 9, UNIT_Y + 14, 6.4, 12, 3, UNIT_TOP + 2.6, [1, 1.8], P)
  out.push(L(ticks.minor, 'lo'), L(ticks.major, 'mid'))
  return out.join('')
}

export const unitLamp = (w: number) => at([UNITS[w]! + 15, UNIT_Y + UNIT_D, 9])
/** The progress needle on a worker's dial, for a body `share` done. */
export function progressAt(w: number, share: number) {
  const c = UNITS[w]!
  const a = share * Math.PI * 2
  return k.onTop([[c + 9, UNIT_Y + 14], [c + 9 + Math.sin(a) * 5.2, UNIT_Y + 14 - Math.cos(a) * 5.2]], UNIT_TOP + 2.7, P)
}

/** A tag, built standing at the origin, thin along y. */
export function tagSvg() {
  const plan: Plan = { x: -6, y: -0.8, w: 12, d: 1.6, r: 0.4 }
  const holeZ = 15
  const hole = Array.from({ length: 24 }, (_, i) => {
    const a = (i / 24) * Math.PI * 2
    return at([2.2 * Math.cos(a), 0.8, holeZ + 2.2 * Math.sin(a)])
  })
  // Drawn where the origin projects; the live script moves it with a translate.
  return S(k.slab(plan, 0, 18, P, 1, 0.3), { tone: 'hi', crease: 'lo' }) +
    `<path class="iso-line" data-tone="lo" d="${k.pathOf(hole, true)}"/>` +
    `<path class="iso-line" data-tone="faint" d="${k.pathOf([at([-4, 0.8, 6]), at([3, 0.8, 6])])}${k.pathOf([at([-4, 0.8, 9]), at([1, 0.8, 9])])}"/>`
}

/** Where a tag is, in world units, for the hook of step `i`. */
export const onHook = (i: number): Point3 => [HOOKS[i]!, FACE + 1.8, HOOK_Z - 15 + 0]
/** Where a tag stands in worker `w`'s holder. */
export const inHolder = (w: number): Point3 => [UNITS[w]! - 7, UNIT_Y + 8, UNIT_TOP + 2]

/** A tag's place between the hook and the holder, `share` of the way, lifted on an arc. */
export function tagPlace(i: number, w: number, share: number): Point3 {
  const a = onHook(i)
  const b = inHolder(w)
  const e = share <= 0 ? 0 : share >= 1 ? 1 : share * share * (3 - 2 * share)
  const lift = Math.sin(Math.PI * e) * 14
  return [a[0] + (b[0] - a[0]) * e, a[1] + (b[1] - a[1]) * e, a[2] + (b[2] - a[2]) * e + lift]
}

export const screenOf = (q: Point3) => k.screenVector(P, q)
export const translate = (q: Point3) => k.translateAlong(P, q)

export function art() {
  return {
    back: [k.bench(PLATE, THICK, P, null), board(), hooksBack()].join(''),
    hooksFront: hooksFront(),
    front: [rail(), unit(0), unit(1)].join(''),
  }
}
