/**
 * A small isometric drawing kit for the site's figures.
 *
 * World units are about one pixel of the real object: x and y lie on the
 * bench, z points up. A projection is an orthographic camera given by an
 * azimuth and an elevation, so every point projects the same way at build time
 * (the static art) and in the browser (the parts that move). Both import this
 * one module, which is what keeps a moving part registered with the part it
 * sits on.
 *
 * Solids are drawn, never wired: each one is filled in the card's own colour
 * and painted far to near, so a nearer part hides a farther one. Side faces get
 * one of four tones by how they face the light and the top is lighter again.
 * The tones are CSS custom properties, so a theme is a token set and nothing
 * here knows a colour.
 */

export type Flat = [number, number]
export type Point3 = [number, number, number]
export type Plane = [number, number]

export interface Projection {
  origin: Flat
  scale: number
  azimuth: number
  elevation: number
}

export interface Plan {
  x: number
  y: number
  w: number
  d: number
  r: number
}

export interface SolidPaths {
  fill: string
  outline: string
  crease: string
  top: string
  shades: string[]
  bevel: string
}

export type Tone = 'hi' | 'mid' | 'lo' | 'faint'
export type LineTone = Tone | 'lit'

const RAD = Math.PI / 180
const SHADES = 4

function camera(p: Projection) {
  const a = p.azimuth * RAD
  const e = p.elevation * RAD
  return { sinA: Math.sin(a), cosA: Math.cos(a), sinE: Math.sin(e), cosE: Math.cos(e), k: p.scale }
}

export function iso([x, y, z]: Point3, p: Projection): Flat {
  const { sinA, cosA, sinE, cosE, k } = camera(p)
  return [p.origin[0] + (x * sinA - y * cosA) * k, p.origin[1] + ((x * cosA + y * sinA) * sinE - z * cosE) * k]
}

/** Larger is nearer the viewer. Used to sort repeated parts far to near. */
export function depthOf([x, y, z]: Point3, p: Projection) {
  const { sinA, cosA, sinE, cosE } = camera(p)
  return (x * cosA + y * sinA) * cosE + z * sinE
}

/** The floor direction that points at the viewer. A side faces you when its normal agrees with it. */
function toward(p: Projection): Plane {
  const { sinA, cosA } = camera(p)
  return [cosA, sinA]
}

const r1 = (v: number) => {
  const s = (Math.round(v * 10) / 10).toFixed(1).replace(/\.0$/, '')
  return s === '-0' ? '0' : s
}

export function pathOf(points: Flat[], closed = false) {
  if (!points.length) return ''
  return points.map(([x, y], i) => `${i ? 'L' : 'M'}${r1(x)} ${r1(y)}`).join('') + (closed ? 'Z' : '')
}

export function roundedPlan({ x, y, w, d, r }: Plan, steps = 6): Plane[] {
  const rad = Math.max(0, Math.min(r, w / 2, d / 2))
  const corners: [number, number, number][] = [
    [x + w - rad, y + rad, -90],
    [x + w - rad, y + d - rad, 0],
    [x + rad, y + d - rad, 90],
    [x + rad, y + rad, 180],
  ]
  const out: Plane[] = []
  for (const [cx, cy, start] of corners) {
    const n = rad > 0 ? steps : 0
    for (let s = 0; s <= n; s++) {
      const a = (start + (n ? (90 * s) / n : 45)) * RAD
      out.push([cx + rad * Math.cos(a), cy + rad * Math.sin(a)])
    }
  }
  return out
}

export function circleRing(cx: number, cy: number, radius: number, steps = 32): Plane[] {
  return Array.from({ length: steps }, (_, s) => {
    const a = (s / steps) * Math.PI * 2
    return [cx + radius * Math.cos(a), cy + radius * Math.sin(a)] as Plane
  })
}

function area(points: Flat[] | Plane[]) {
  let s = 0
  for (let i = 0; i < points.length; i++) {
    const [ax, ay] = points[i]!
    const [bx, by] = points[(i + 1) % points.length]!
    s += ax * by - bx * ay
  }
  return s / 2
}

const oriented = (pts: Flat[]) => (area(pts) < 0 ? [...pts].reverse() : pts)

function hull(points: Flat[]): Flat[] {
  const s = [...points].sort((a, b) => a[0] - b[0] || a[1] - b[1])
  if (s.length < 3) return s
  const cross = (o: Flat, a: Flat, b: Flat) => (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
  const lo: Flat[] = []
  for (const q of s) {
    while (lo.length >= 2 && cross(lo[lo.length - 2]!, lo[lo.length - 1]!, q) <= 0) lo.pop()
    lo.push(q)
  }
  const up: Flat[] = []
  for (let i = s.length - 1; i >= 0; i--) {
    const q = s[i]!
    while (up.length >= 2 && cross(up[up.length - 2]!, up[up.length - 1]!, q) <= 0) up.pop()
    up.push(q)
  }
  return [...lo.slice(0, -1), ...up.slice(0, -1)]
}

/** Runs of consecutive edges whose facing flag equals `want`, as vertex index lists. */
function runsOf(flags: boolean[], want: boolean) {
  const n = flags.length
  const runs: number[][] = []
  const start = flags.findIndex((f, i) => f !== flags[(i + n - 1) % n])
  if (start < 0) return flags[0] === want ? [Array.from({ length: n + 1 }, (_, i) => i % n)] : []
  let run: number[] | null = null
  for (let step = 0; step < n; step++) {
    const i = (start + step) % n
    if (flags[i] === want) {
      if (!run) run = [i]
      run.push((i + 1) % n)
    } else if (run) {
      runs.push(run)
      run = null
    }
  }
  if (run) runs.push(run)
  return runs
}

/**
 * A convex prism: a floor ring raised by `height` from `z`.
 *
 * The outline is drawn only where the silhouette is, and the crease where the
 * top meets the visible sides, so a box reads as a solid with no back edges.
 * `bevel` insets the top edge on the lit sides by that much, the line a
 * machined chamfer catches.
 */
export function extrude(ring: Plane[], z: number, height: number, p: Projection, bevel = 0): SolidPaths {
  const n = ring.length
  const ccw = area(ring) > 0
  const [vx, vy] = toward(p)
  const { sinA, cosA } = camera(p)
  const normals = ring.map(([ax, ay], i) => {
    const [bx, by] = ring[(i + 1) % n]!
    const dx = bx - ax
    const dy = by - ay
    const len = Math.hypot(dx, dy) || 1
    return (ccw ? [dy / len, -dx / len] : [-dy / len, dx / len]) as Plane
  })
  const facing = normals.map(([nx, ny]) => nx * vx + ny * vy > 1e-9)
  const top = ring.map(([x, y]) => iso([x, y, z + height], p))
  const bottom = ring.map(([x, y]) => iso([x, y, z], p))
  const outline: string[] = []
  const crease: string[] = []
  for (const run of runsOf(facing, false)) outline.push(pathOf(run.map((i) => top[i]!)))
  for (const run of runsOf(facing, true)) {
    crease.push(pathOf(run.map((i) => top[i]!)))
    if (height > 0) outline.push(pathOf(run.map((i) => bottom[i]!)))
  }
  if (height > 0) {
    for (let i = 0; i < n; i++) {
      if (facing[(i + n - 1) % n] !== facing[i]) outline.push(pathOf([top[i]!, bottom[i]!]))
    }
  }
  const buckets: string[][] = Array.from({ length: SHADES }, () => [])
  if (height > 0) {
    // Light from the viewer's upper left: the same direction for every solid,
    // so two boxes side by side agree about where the light is.
    const [lx, ly] = [-sinA, cosA]
    for (let i = 0; i < n; i++) {
      if (!facing[i]) continue
      const [nx, ny] = normals[i]!
      const lit = (nx * lx + ny * ly + 1) / 2
      const shade = Math.min(SHADES - 1, Math.max(0, Math.floor(lit * SHADES)))
      const j = (i + 1) % n
      buckets[shade]!.push(pathOf(oriented([top[i]!, top[j]!, bottom[j]!, bottom[i]!]), true))
    }
  }
  let bev = ''
  if (bevel > 0) {
    const inset = ring.map(([x, y], i) => {
      const [ax, ay] = normals[(i + n - 1) % n]!
      const [bx, by] = normals[i]!
      const mx = ax + bx
      const my = ay + by
      const len = Math.hypot(mx, my) || 1
      return [x - (mx / len) * bevel, y - (my / len) * bevel] as Plane
    })
    const insetTop = inset.map(([x, y]) => iso([x, y, z + height], p))
    bev = runsOf(facing, true)
      .map((run) => pathOf([top[run[0]!]!, ...run.slice(1, -1).map((i) => insetTop[i]!), top[run[run.length - 1]!]!]))
      .join('')
  }
  return {
    fill: pathOf(hull([...top, ...bottom]), true),
    outline: outline.join(''),
    crease: crease.join(''),
    top: pathOf(oriented(top), true),
    shades: buckets.map((b) => b.join('')),
    bevel: bev,
  }
}

export const slab = (plan: Plan, z: number, h: number, p: Projection, steps = 6, bevel = 0) =>
  extrude(roundedPlan(plan, steps), z, h, p, bevel)

export const cylinder = (cx: number, cy: number, radius: number, z: number, h: number, p: Projection, steps = 28, bevel = 0) =>
  extrude(circleRing(cx, cy, radius, steps), z, h, p, bevel)

export const inset = ({ x, y, w, d, r }: Plan, by: number): Plan => ({
  x: x + by,
  y: y + by,
  w: w - by * 2,
  d: d - by * 2,
  r: Math.max(0, r - by),
})

export const onTop = (points: Plane[], z: number, p: Projection, closed = false) =>
  pathOf(points.map(([x, y]) => iso([x, y, z], p)), closed)

export const planOutline = (plan: Plan, z: number, p: Projection, steps = 6) => onTop(roundedPlan(plan, steps), z, p, true)

export const ring = (cx: number, cy: number, radius: number, z: number, p: Projection, steps = 28) =>
  onTop(circleRing(cx, cy, radius, steps), z, p, true)

export const segment = (a: Point3, b: Point3, p: Projection) => pathOf([iso(a, p), iso(b, p)])

export function corners({ x, y, w, d }: Plan, by = 0): Plane[] {
  return [
    [x + by, y + by],
    [x + w - by, y + by],
    [x + by, y + d - by],
    [x + w - by, y + d - by],
  ]
}

export function boxCorners({ x, y, w, d }: Plan, z0: number, z1: number): Point3[] {
  const out: Point3[] = []
  for (const px of [x, x + w]) for (const py of [y, y + d]) for (const z of [z0, z1]) out.push([px, py, z])
  return out
}

/** Vertical grip lines round the visible half of a knob. */
export function knurl(cx: number, cy: number, radius: number, z0: number, z1: number, count: number, p: Projection) {
  const [vx, vy] = toward(p)
  const out: string[] = []
  for (let i = 0; i < count; i++) {
    const a = (i / count) * Math.PI * 2
    const nx = Math.cos(a)
    const ny = Math.sin(a)
    if (nx * vx + ny * vy < 0.18) continue
    out.push(segment([cx + radius * nx, cy + radius * ny, z0 + 0.4], [cx + radius * nx, cy + radius * ny, z1 - 0.4], p))
  }
  return out.join('')
}

/** The near half of a ring round a cylinder: a seam that ends on the silhouette. */
export function sideArc(cx: number, cy: number, radius: number, z: number, p: Projection, steps = 18) {
  const [vx, vy] = toward(p)
  const f = Math.atan2(vy, vx)
  const pts: Flat[] = []
  for (let s = 0; s <= steps; s++) {
    const a = f - Math.PI / 2 + (s / steps) * Math.PI
    pts.push(iso([cx + radius * Math.cos(a), cy + radius * Math.sin(a), z], p))
  }
  return pathOf(pts)
}

/** A seam round the visible sides of a box at height z, ending on its silhouette edges. */
export function sideSeam(plan: Plan, z: number, p: Projection, steps = 6) {
  const ringPts = roundedPlan(plan, steps)
  const n = ringPts.length
  const ccw = area(ringPts) > 0
  const [vx, vy] = toward(p)
  const facing = ringPts.map(([ax, ay], i) => {
    const [bx, by] = ringPts[(i + 1) % n]!
    const [nx, ny] = ccw ? [by - ay, -(bx - ax)] : [-(by - ay), bx - ax]
    return nx * vx + ny * vy > 1e-9
  })
  return runsOf(facing, true)
    .map((run) => pathOf(run.map((i) => iso([ringPts[i]![0], ringPts[i]![1], z], p))))
    .join('')
}

/** Ruler ticks rooted on a strip's edge, every `step`, a long one every `every`. */
export function topTicks(
  from: number, to: number, step: number, every: number, edge: number, z: number,
  lengths: [number, number], p: Projection, axis: 'x' | 'y' = 'x',
) {
  const minor: string[] = []
  const major: string[] = []
  const first = Math.ceil(from / step) * step
  for (let at = first, i = Math.round(first / step); at <= to + 1e-9; at += step, i++) {
    const big = i % every === 0
    const len = big ? lengths[1] : lengths[0]
    const a: Plane = axis === 'x' ? [at, edge] : [edge, at]
    const b: Plane = axis === 'x' ? [at, edge + len] : [edge + len, at]
    ;(big ? major : minor).push(onTop([a, b], z, p))
  }
  return { minor: minor.join(''), major: major.join('') }
}

/** Ticks round a dial face lying flat, from angle `from` through `span`, clockwise seen from above. */
export function radialTicks(
  cx: number, cy: number, radius: number, count: number, every: number, z: number,
  lengths: [number, number], p: Projection, from = 0, span = Math.PI * 2,
) {
  const minor: string[] = []
  const major: string[] = []
  const closed = Math.abs(span - Math.PI * 2) < 1e-9
  for (let i = 0; i < (closed ? count : count + 1); i++) {
    const a = from + (i / count) * span
    const big = i % every === 0
    const inner = radius - (big ? lengths[1] : lengths[0])
    const sx = Math.sin(a)
    const sy = -Math.cos(a)
    ;(big ? major : minor).push(onTop([[cx + sx * inner, cy + sy * inner], [cx + sx * radius, cy + sy * radius]], z, p))
  }
  return { minor: minor.join(''), major: major.join('') }
}

export function screenVector(p: Projection, [dx, dy, dz]: Point3): Flat {
  const o = iso([0, 0, 0], p)
  const m = iso([dx, dy, dz], p)
  return [m[0] - o[0], m[1] - o[1]]
}

export function translateAlong(p: Projection, by: Point3) {
  const [x, y] = screenVector(p, by)
  return `translate(${Math.round(x * 100) / 100} ${Math.round(y * 100) / 100})`
}

/** Fit a camera so these world points fill a `width` by `height` viewBox, less `pad`. */
export function fit(points: Point3[], width: number, height: number, pad = 24, azimuth = 45, elevation = 30): Projection {
  const unit: Projection = { origin: [0, 0], scale: 1, azimuth, elevation }
  const flat = points.map((q) => iso(q, unit))
  const xs = flat.map(([x]) => x)
  const ys = flat.map(([, y]) => y)
  const left = Math.min(...xs)
  const right = Math.max(...xs)
  const up = Math.min(...ys)
  const down = Math.max(...ys)
  const scale = Math.min((width - pad * 2) / (right - left), (height - pad * 2) / (down - up))
  return {
    origin: [width / 2 - ((left + right) / 2) * scale, height / 2 - ((up + down) / 2) * scale],
    scale,
    azimuth,
    elevation,
  }
}

/* ------------------------------------------------------------ markup -- */

export interface SolidStyle {
  tone?: Tone
  crease?: Tone | 'none'
  lit?: boolean
  inner?: string
  halo?: boolean
}

export function solid(paths: SolidPaths, { tone = 'mid', crease = 'faint', lit = false, inner = '', halo = false }: SolidStyle = {}) {
  const out: string[] = []
  if (halo) out.push(`<path class="iso-halo" d="${paths.fill}"/>`)
  out.push(`<path class="iso-fill" d="${paths.fill}"/>`)
  paths.shades.forEach((d, i) => {
    if (d) out.push(`<path class="iso-shade" data-shade="${i}" d="${d}"/>`)
  })
  out.push(`<path class="iso-top" d="${paths.top}"/>`)
  if (crease !== 'none' && paths.crease) out.push(`<path class="iso-line" data-tone="${crease}" d="${paths.crease}"/>`)
  if (crease !== 'none' && paths.bevel) out.push(`<path class="iso-line" data-tone="${crease}" d="${paths.bevel}"/>`)
  if (inner) out.push(inner)
  out.push(`<path class="iso-line iso-edge" data-tone="${tone}" d="${paths.outline}"/>`)
  return `<g class="iso-solid"${lit ? ' data-lit=""' : ''}>${out.join('')}</g>`
}

export const line = (d: string, tone: LineTone = 'lo', extra = '') =>
  d ? `<path class="iso-line" data-tone="${tone}" d="${d}"${extra}/>` : ''

export function dots(points: Flat[], size = 0.5, tone: Tone = 'mid') {
  return `<g class="iso-dots" data-tone="${tone}">${points
    .map(([x, y]) => `<circle cx="${r1(x)}" cy="${r1(y)}" r="${size}"/>`)
    .join('')}</g>`
}

export const halo = (plan: Plan, z: number, p: Projection) => `<path class="iso-halo" d="${slab(plan, z, 0.1, p).fill}"/>`

/** Sort floor points far to near, for feet, pegs and anything repeated. */
export const byDepth = (pts: Plane[], p: Projection) =>
  [...pts].sort((a, b) => depthOf([a[0], a[1], 0], p) - depthOf([b[0], b[1], 0], p))

/**
 * The parts every bench shares: a halo, four feet, a bevelled plate, an inset
 * groove, corner screws, and a ruler strip along the near edge.
 */
export function bench(plate: Plan, thick: number, p: Projection, ruler: { y: number; from: number; to: number } | null) {
  const foot = 4
  const out = [halo(inset(plate, 6), -thick - foot, p)]
  for (const [x, y] of byDepth(corners(plate, 16), p)) {
    out.push(solid(cylinder(x, y, 5.6, -thick - foot, 1.4, p, 24), { tone: 'lo' }))
    out.push(solid(cylinder(x, y, 4.2, -thick - foot + 1.4, foot - 1.4, p, 24), { tone: 'mid' }))
  }
  out.push(solid(slab(plate, -thick, thick, p, 8, 1.4), { tone: 'mid' }))
  out.push(line(planOutline(inset(plate, 4.5), 0, p), 'faint'))
  const screws = corners(plate, 9)
  out.push(line(screws.map(([x, y]) => ring(x, y, 1.9, 0, p, 14)).join(''), 'lo'))
  out.push(dots(screws.map(([x, y]) => iso([x, y, 0], p)), 0.5))
  if (ruler) {
    const strip: Plan = { x: ruler.from, y: ruler.y, w: ruler.to - ruler.from, d: 6, r: 1 }
    out.push(solid(slab(strip, 0, 0.6, p, 3), { tone: 'lo', crease: 'none' }))
    const t = topTicks(strip.x + 3, strip.x + strip.w - 3, 4, 5, strip.y, 0.6, [1.8, 3.4], p)
    out.push(line(t.minor, 'lo'), line(t.major, 'mid'))
  }
  return out.join('')
}
