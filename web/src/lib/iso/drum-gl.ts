/**
 * The run drum in WebGL2: a recorder drum on a stand, lit, in perspective.
 *
 * No library. The parts are a handful of meshes built here once (boxes,
 * cylinders along x, a beam between two points), one shader lights them all,
 * and the paper's bars are drawn in that shader from the span table, so the
 * picture is computed from the run rather than painted onto a texture.
 *
 * Colours are read from the page's tokens when the renderer starts, so the
 * drum is painted in the panel's own colours and follows a theme change on the
 * next `paint()`.
 */
import { SPANS, TRACKS, U0, U1, V0, V1, KIND_TOKEN, PEN } from './drum'
import { SPECIMEN_MS } from '@/lib/specimen'

type V3 = [number, number, number]

/* --------------------------------------------------------------- meshes -- */

interface Mesh {
  data: number[]
}
const mesh = (): Mesh => ({ data: [] })
const vert = (m: Mesh, p: V3, n: V3, u = 0, v = 0) => m.data.push(p[0], p[1], p[2], n[0], n[1], n[2], u, v)
function quad(m: Mesh, a: V3, b: V3, c: V3, d: V3, n: V3) {
  vert(m, a, n); vert(m, b, n); vert(m, c, n)
  vert(m, a, n); vert(m, c, n); vert(m, d, n)
}

/** An axis-aligned box with a small chamfer read by the lighting, not modelled. */
function box(m: Mesh, [x0, y0, z0]: V3, [x1, y1, z1]: V3) {
  quad(m, [x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0], [0, 1, 0])
  quad(m, [x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [0, -1, 0])
  quad(m, [x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1], [0, 0, 1])
  quad(m, [x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [0, 0, -1])
  quad(m, [x1, y0, z1], [x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [1, 0, 0])
  quad(m, [x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0], [-1, 0, 0])
}

/**
 * A cylinder along x. Its side carries paper coordinates: u round it (0 at the
 * top, rising toward the viewer) and v along it, so the shader can lay the
 * tracks out in the drum's own frame.
 */
function cylX(m: Mesh, x0: number, x1: number, r: number, [cy, cz]: [number, number], caps = true, n = 96) {
  const at = (a: number): [number, number] => [cy + r * Math.cos(a), cz + r * Math.sin(a)]
  for (let i = 0; i < n; i++) {
    const a0 = (i / n) * Math.PI * 2
    const a1 = ((i + 1) / n) * Math.PI * 2
    const [y0, z0] = at(a0)
    const [y1, z1] = at(a1)
    const n0: V3 = [0, Math.cos(a0), Math.sin(a0)]
    const n1: V3 = [0, Math.cos(a1), Math.sin(a1)]
    vert(m, [x0, y0, z0], n0, i / n, 0); vert(m, [x1, y0, z0], n0, i / n, 1); vert(m, [x1, y1, z1], n1, (i + 1) / n, 1)
    vert(m, [x0, y0, z0], n0, i / n, 0); vert(m, [x1, y1, z1], n1, (i + 1) / n, 1); vert(m, [x0, y1, z1], n1, (i + 1) / n, 0)
    if (caps) {
      vert(m, [x1, cy, cz], [1, 0, 0]); vert(m, [x1, y0, z0], [1, 0, 0]); vert(m, [x1, y1, z1], [1, 0, 0])
      vert(m, [x0, cy, cz], [-1, 0, 0]); vert(m, [x0, y1, z1], [-1, 0, 0]); vert(m, [x0, y0, z0], [-1, 0, 0])
    }
  }
}

/** A square beam of half-width `w` from a to b. */
function beam(m: Mesh, a: V3, b: V3, w: number) {
  const d = norm(sub(b, a))
  const up: V3 = Math.abs(d[1]) > 0.9 ? [1, 0, 0] : [0, 1, 0]
  const s = norm(cross(d, up))
  const t = cross(s, d)
  const c = (p: V3, i: number, j: number): V3 => [p[0] + (s[0] * i + t[0] * j) * w, p[1] + (s[1] * i + t[1] * j) * w, p[2] + (s[2] * i + t[2] * j) * w]
  const corners: [number, number][] = [[-1, -1], [1, -1], [1, 1], [-1, 1]]
  for (let k = 0; k < 4; k++) {
    const [i0, j0] = corners[k]!
    const [i1, j1] = corners[(k + 1) % 4]!
    const nrm = norm([s[0] * (i0 + i1) + t[0] * (j0 + j1), s[1] * (i0 + i1) + t[1] * (j0 + j1), s[2] * (i0 + i1) + t[2] * (j0 + j1)])
    quad(m, c(a, i0, j0), c(a, i1, j1), c(b, i1, j1), c(b, i0, j0), nrm)
  }
}

const sub = (a: V3, b: V3): V3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
const cross = (a: V3, b: V3): V3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
const norm = (a: V3): V3 => {
  const l = Math.hypot(...a) || 1
  return [a[0] / l, a[1] / l, a[2] / l]
}

/* ------------------------------------------------------------- matrices -- */

type M4 = Float32Array
function perspective(fov: number, aspect: number, near: number, far: number): M4 {
  const f = 1 / Math.tan(fov / 2)
  const nf = 1 / (near - far)
  return new Float32Array([f / aspect, 0, 0, 0, 0, f, 0, 0, 0, 0, (far + near) * nf, -1, 0, 0, 2 * far * near * nf, 0])
}
function lookAt(eye: V3, at: V3, up: V3): M4 {
  const z = norm(sub(eye, at))
  const x = norm(cross(up, z))
  const y = cross(z, x)
  return new Float32Array([
    x[0], y[0], z[0], 0, x[1], y[1], z[1], 0, x[2], y[2], z[2], 0,
    -(x[0] * eye[0] + x[1] * eye[1] + x[2] * eye[2]),
    -(y[0] * eye[0] + y[1] * eye[1] + y[2] * eye[2]),
    -(z[0] * eye[0] + z[1] * eye[1] + z[2] * eye[2]), 1,
  ])
}
function mul(a: M4, b: M4): M4 {
  const o = new Float32Array(16)
  for (let c = 0; c < 4; c++) for (let r = 0; r < 4; r++) {
    let s = 0
    for (let k = 0; k < 4; k++) s += a[k * 4 + r]! * b[c * 4 + k]!
    o[c * 4 + r] = s
  }
  return o
}
/** A turn about the drum's axis (x) by `t` radians, toward the viewer at the top. */
function turnX(t: number): M4 {
  const c = Math.cos(t)
  const s = Math.sin(t)
  return new Float32Array([1, 0, 0, 0, 0, c, s, 0, 0, -s, c, 0, 0, 0, 0, 1])
}
const IDENTITY = new Float32Array([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1])

/* -------------------------------------------------------------- shaders -- */

const VS = `#version 300 es
in vec3 aPos; in vec3 aNrm; in vec2 aUV;
uniform mat4 uVP; uniform mat4 uModel;
out vec3 vN; out vec2 vUV; out vec3 vW; out vec3 vO; out vec3 vON;
void main() {
  vec4 w = uModel * vec4(aPos, 1.0);
  vW = w.xyz; vN = mat3(uModel) * aNrm; vUV = aUV; vO = aPos; vON = aNrm;
  gl_Position = uVP * w;
}`

const FS = `#version 300 es
precision highp float;
in vec3 vN; in vec2 vUV; in vec3 vW; in vec3 vO; in vec3 vON;
uniform vec3 uColor; uniform int uMode; uniform vec3 uEye;
uniform vec3 uPaper; uniform vec3 uRule; uniform vec3 uKinds[5]; uniform vec3 uPenInk;
uniform vec4 uSpans[8]; uniform int uCount; uniform float uTracks; uniform vec4 uRun; uniform float uPenU; uniform int uSel;
uniform float uTicks;
out vec4 frag;

// Antialiased band: 1 inside [a, b], with a one-pixel edge.
float band(float x, float a, float b) {
  float w = fwidth(x) * 0.75;
  return smoothstep(a - w, a + w, x) * (1.0 - smoothstep(b - w, b + w, x));
}
float hair(float x, float at, float px) {
  float w = fwidth(x);
  return 1.0 - smoothstep(0.0, w * px, abs(x - at));
}

void main() {
  vec3 n = normalize(vN);
  vec3 v = normalize(uEye - vW);
  vec3 base = uColor;
  if (uMode == 1) {
    // The paper. Track rules, a light time grid, the bars, and the pen's line.
    float u = vUV.x, t = vUV.y;
    base = uPaper;
    float inRun = band(u, uRun.x, uRun.y);
    float tv = (t - uRun.z) / (uRun.w - uRun.z) * uTracks;
    for (int i = 0; i <= 8; i++) {
      if (float(i) > uTracks) break;
      base = mix(base, uRule, 0.35 * hair(tv, float(i), 1.0) * inRun * band(t, uRun.z - 0.01, uRun.w + 0.01));
    }
    float sec = (u - uRun.x) / (uRun.y - uRun.x) * uTicks;
    base = mix(base, uRule, 0.18 * hair(fract(sec + 0.5), 0.5, 1.0) * inRun * band(t, uRun.z, uRun.w));
    for (int i = 0; i < 8; i++) {
      if (i >= uCount) break;
      vec4 s = uSpans[i];
      float c = s.z + 0.5;
      float on = band(u, s.x, max(s.y, s.x + 0.0016)) * band(tv, c - 0.28, c + 0.28);
      vec3 ink = uKinds[int(s.w)];
      base = mix(base, ink, on * (i == uSel ? 1.0 : 0.82));
    }
    // The clip that holds the paper's two ends.
    float clip = 1.0 - band(u, uRun.x - 0.025, uRun.y + 0.06);
    base = mix(base, uColor, clip);
    base = mix(base, uPenInk, 0.85 * hair(u, uPenU, 1.4));
  } else if (uMode == 3 && abs(vON.x) > 0.5) {
    // A flange's end face: a turned groove near the rim, one round the hub, and
    // eight screws on a bolt circle. In the part's own frame, so they turn with it.
    float r = length(vO.yz);
    float a = atan(vO.z, vO.y);
    float groove = hair(r, 0.94, 1.2) + hair(r, 0.36, 1.2);
    float cell = mod(a + 3.14159 / 8.0, 3.14159 / 4.0) - 3.14159 / 8.0;
    float screw = 1.0 - smoothstep(0.028, 0.028 + fwidth(r) * 1.5, length(vec2(r - 0.8, cell * 0.8)));
    base = mix(base, uRule, 0.55 * clamp(groove, 0.0, 1.0));
    base = mix(base, uRule * 0.8, 0.7 * screw);
  } else if (uMode == 2) {
    // The base: a soft contact shadow under the drum, from the drum's own footprint.
    float d = length(vec2(max(abs(vW.x) - 1.1, 0.0), vW.z * 1.4));
    base *= 1.0 - 0.18 * (1.0 - smoothstep(0.0, 0.9, d));
  }
  vec3 L = normalize(vec3(-0.45, 0.85, 0.55));
  float diff = max(dot(n, L), 0.0);
  float hemi = 0.5 + 0.5 * n.y;
  vec3 h = normalize(L + v);
  float spec = pow(max(dot(n, h), 0.0), 48.0) * (uMode == 1 ? 0.05 : 0.12);
  vec3 col = base * (0.62 + 0.22 * hemi + 0.26 * diff) + spec;
  // A darker limb where the surface turns away: the hairline edge the rest of the panel is drawn with.
  float rim = pow(1.0 - max(dot(n, v), 0.0), 3.0);
  col *= 1.0 - 0.28 * rim;
  frag = vec4(col, 1.0);
}`

/* ------------------------------------------------------------- renderer -- */

export interface Drum {
  paint: (turn: number, sel: number, penU: number) => void
  resize: () => void
  readColours: () => void
}

/** Read a CSS colour expression as linear-ish RGB in 0..1, through a 2D canvas. */
function colourReader(scope: HTMLElement) {
  const probe = document.createElement('span')
  probe.style.display = 'none'
  scope.appendChild(probe)
  const c2 = document.createElement('canvas').getContext('2d', { willReadFrequently: true })!
  return (expr: string): V3 => {
    probe.style.color = expr
    c2.clearRect(0, 0, 1, 1)
    c2.fillStyle = getComputedStyle(probe).color
    c2.fillRect(0, 0, 1, 1)
    const [r, g, b] = c2.getImageData(0, 0, 1, 1).data
    return [r! / 255, g! / 255, b! / 255]
  }
}

export function createDrum(canvas: HTMLCanvasElement, scope: HTMLElement): Drum | null {
  const gl = canvas.getContext('webgl2', { antialias: true, alpha: true, premultipliedAlpha: true, preserveDrawingBuffer: true })
  if (!gl) return null
  const compile = (type: number, src: string) => {
    const s = gl.createShader(type)!
    gl.shaderSource(s, src)
    gl.compileShader(s)
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) ?? 'shader')
    return s
  }
  const prog = gl.createProgram()!
  gl.attachShader(prog, compile(gl.VERTEX_SHADER, VS))
  gl.attachShader(prog, compile(gl.FRAGMENT_SHADER, FS))
  gl.linkProgram(prog)
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return null
  gl.useProgram(prog)
  const U = (n: string) => gl.getUniformLocation(prog, n)

  // World: drum radius 1 along x, its axis at the origin; the bench below.
  const R = 1
  const HALF = 1.15
  const FLOOR = -1.5
  const parts: { m: Mesh; colour: string; mode: number; turns: boolean }[] = []
  const add = (colour: string, mode: number, turns: boolean, build: (m: Mesh) => void) => {
    const m = mesh()
    build(m)
    parts.push({ m, colour, mode, turns })
  }
  add('var(--iso-shade-2)', 2, false, (m) => box(m, [-1.95, FLOOR - 0.14, -0.82], [1.95, FLOOR, 0.82]))
  add('var(--iso-top)', 0, false, (m) => box(m, [-1.91, FLOOR, -0.78], [1.91, FLOOR + 0.03, 0.78]))
  for (const sx of [-1, 1]) {
    const x = sx * (HALF + 0.26)
    add('var(--iso-shade-2)', 0, false, (m) => {
      box(m, [x - 0.07, FLOOR, -0.32], [x + 0.07, 0.0, 0.32])
      box(m, [x - 0.1, FLOOR, -0.4], [x + 0.1, FLOOR + 0.08, 0.4])
      cylX(m, x - 0.1, x + 0.1, 0.2, [0, 0], true, 48)
    })
  }
  add('var(--iso-mid)', 0, false, (m) => cylX(m, -HALF - 0.45, HALF + 0.45, 0.06, [0, 0], true, 24))
  // The drum: paper round the side, flanges and hubs at the ends.
  add('var(--iso-shade-1)', 1, true, (m) => cylX(m, -HALF, HALF, R, [0, 0], false, 192))
  add('var(--iso-top)', 3, true, (m) => {
    cylX(m, -HALF - 0.05, -HALF, R + 0.06, [0, 0], true, 128)
    cylX(m, HALF, HALF + 0.05, R + 0.06, [0, 0], true, 128)
  })
  add('var(--iso-shade-1)', 0, true, (m) => {
    cylX(m, -HALF - 0.12, -HALF - 0.05, 0.28, [0, 0], true, 48)
    cylX(m, HALF + 0.05, HALF + 0.12, 0.28, [0, 0], true, 48)
  })
  // The pen bar across the drum, held at both ends by arms from the cheeks.
  const py = (R + 0.07) * Math.cos(PEN)
  const pz = (R + 0.07) * Math.sin(PEN)
  add('var(--iso-hi)', 0, false, (m) => {
    beam(m, [-HALF - 0.1, py, pz], [HALF + 0.1, py, pz], 0.025)
    for (const sx of [-1, 1]) beam(m, [sx * (HALF + 0.26), 0.0, 0.0], [sx * (HALF + 0.1), py, pz], 0.03)
  })
  add('var(--iso-lit)', 0, false, (m) => box(m, [-0.06, py - 0.04, pz - 0.04], [0.06, py + 0.07, pz + 0.07]))

  const bufs = parts.map((p) => {
    const vao = gl.createVertexArray()!
    gl.bindVertexArray(vao)
    const b = gl.createBuffer()!
    gl.bindBuffer(gl.ARRAY_BUFFER, b)
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(p.m.data), gl.STATIC_DRAW)
    const stride = 32
    const attr = (name: string, size: number, off: number) => {
      const loc = gl.getAttribLocation(prog, name)
      if (loc < 0) return
      gl.enableVertexAttribArray(loc)
      gl.vertexAttribPointer(loc, size, gl.FLOAT, false, stride, off)
    }
    attr('aPos', 3, 0)
    attr('aNrm', 3, 12)
    attr('aUV', 2, 24)
    return { vao, count: p.m.data.length / 8 }
  })

  const read = colourReader(scope)
  let colours: V3[] = []
  let paper: V3 = [1, 1, 1]
  let rule: V3 = [0, 0, 0]
  let kinds: V3[] = []
  let penInk: V3 = [0, 0, 0]
  const readColours = () => {
    colours = parts.map((p) => read(p.colour))
    paper = read('var(--iso-chart)')
    rule = read('var(--iso-lo)')
    penInk = read('var(--iso-hi)')
    kinds = KIND_TOKEN.map((t) => read(`var(${t})`))
  }
  readColours()

  gl.enable(gl.DEPTH_TEST)
  gl.uniform4fv(U('uSpans'), new Float32Array(SPANS.flatMap((s) => [s[0], s[1], s[2], s[3]])))
  gl.uniform1i(U('uCount'), SPANS.length)
  gl.uniform1f(U('uTracks'), TRACKS)
  gl.uniform4f(U('uRun'), U0, U1, V0, V1)
  gl.uniform1f(U('uTicks'), SPECIMEN_MS / 1000)

  const eye: V3 = [4.1, 2.6, 6.7]
  const resize = () => {
    const dpr = Math.min(window.devicePixelRatio || 1, 2)
    const w = Math.max(1, Math.round(canvas.clientWidth * dpr))
    const h = Math.max(1, Math.round(canvas.clientHeight * dpr))
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w
      canvas.height = h
    }
  }

  const paint = (turn: number, sel: number, penU: number) => {
    resize()
    gl.viewport(0, 0, canvas.width, canvas.height)
    gl.clearColor(0, 0, 0, 0)
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT)
    const aspect = canvas.width / canvas.height
    // Fit the bench's width whatever the stage's shape: a narrow stage widens the field.
    const fov = 2 * Math.atan(Math.tan((27 * Math.PI) / 360) * Math.max(1, 2.13 / aspect))
    const vp = mul(perspective(fov, aspect, 0.1, 50), lookAt(eye, [0.12, -0.5, 0], [0, 1, 0]))
    gl.uniformMatrix4fv(U('uVP'), false, vp)
    gl.uniform3f(U('uEye'), eye[0], eye[1], eye[2])
    gl.uniform3fv(U('uPaper'), paper)
    gl.uniform3fv(U('uRule'), rule)
    gl.uniform3fv(U('uPenInk'), penInk)
    gl.uniform3fv(U('uKinds'), new Float32Array(kinds.flat()))
    gl.uniform1f(U('uPenU'), penU)
    gl.uniform1i(U('uSel'), sel)
    const spin = turnX(turn)
    parts.forEach((p, i) => {
      gl.uniformMatrix4fv(U('uModel'), false, p.turns ? spin : IDENTITY)
      gl.uniform3fv(U('uColor'), colours[i]!)
      gl.uniform1i(U('uMode'), p.mode)
      gl.bindVertexArray(bufs[i]!.vao)
      gl.drawArrays(gl.TRIANGLES, 0, bufs[i]!.count)
    })
  }

  return { paint, resize, readColours }
}
