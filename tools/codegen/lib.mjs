// Shared parsing + naming helpers for every generator.
// The spec is the only input; nothing here reads generated output.
import fs from 'node:fs'
import path from 'node:path'

export const ROOT = path.resolve(new URL('../..', import.meta.url).pathname)
export const SPEC_PATH = path.join(ROOT, 'spec', 'mcpd.spec.json')

export function loadSpec() {
  const spec = JSON.parse(fs.readFileSync(SPEC_PATH, 'utf8'))
  validate(spec)
  return spec
}

/** Parse `Foo[]?` into { base:'Foo', array:true, optional:true }. */
export function parseType(t) {
  let s = String(t), optional = false, array = false
  if (s.endsWith('?')) { optional = true; s = s.slice(0, -1) }
  if (s.endsWith('[]')) { array = true; s = s.slice(0, -2) }
  if (s.endsWith('?')) { optional = true; s = s.slice(0, -1) }
  return { base: s, array, optional }
}

export const PRIMITIVES = new Set(['string', 'bool', 'f64', 'u64', 'u32', 'i64', 'json', 'timestamp'])

/**
 * A name the generators can turn into an identifier in every language. Enum
 * values are rendered verbatim on the wire and pascal-cased or upper-cased into
 * identifiers, so a dotted or capitalised value breaks Rust and Kotlin; field
 * names go through the same casing. One pattern, checked once, is what keeps
 * a spelling from surfacing as a compile error six emitters away.
 */
export const IDENT = /^[a-z][a-z0-9_]*$/

/**
 * Rust has no escape the Rust emitter uses for a field named after a keyword,
 * so `type` or `match` as a field name would emit a struct that does not
 * compile. Refused here, where the message can say what to rename.
 */
const RUST_KEYWORDS = new Set(['as', 'async', 'await', 'break', 'const', 'continue', 'crate',
  'dyn', 'else', 'enum', 'extern', 'false', 'fn', 'for', 'if', 'impl', 'in', 'let', 'loop',
  'match', 'mod', 'move', 'mut', 'pub', 'ref', 'return', 'self', 'static', 'struct', 'super',
  'trait', 'true', 'type', 'unsafe', 'use', 'where', 'while', 'abstract', 'become', 'box', 'do',
  'final', 'macro', 'override', 'priv', 'try', 'typeof', 'unsized', 'virtual', 'yield'])

/**
 * What an endpoint may require. The six scopes are the server's `Scope` enum.
 * `session` is an account's session token from `POST /v1/api/account/login`,
 * which no API key satisfies, so a leaked key cannot mint more keys.
 */
export const AUTH_VALUES = new Set(['none', 'ingest', 'read', 'write', 'admin', 'execute', 'worker', 'session'])

/** Fail loudly on a spec that references something that does not exist. */
function validate(spec) {
  const known = new Set([...PRIMITIVES, ...Object.keys(spec.enums), ...Object.keys(spec.types), ...Object.keys(spec.entities)])
  const errs = []
  const checkFields = (owner, fields) => {
    for (const f of fields) {
      const { base } = parseType(f.type)
      if (!known.has(base)) errs.push(`${owner}.${f.name}: unknown type "${f.type}"`)
      if (!f.doc) errs.push(`${owner}.${f.name}: missing doc string`)
      if (!IDENT.test(f.name)) errs.push(`${owner}.${f.name}: field name must match ${IDENT}`)
      if (RUST_KEYWORDS.has(f.name)) errs.push(`${owner}.${f.name}: field name is a Rust keyword; rename it`)
    }
  }
  for (const [n, e] of Object.entries(spec.enums)) {
    for (const v of e.values) {
      if (!IDENT.test(v.name)) errs.push(`enum ${n} value "${v.name}": must match ${IDENT}`)
      if (!v.doc) errs.push(`enum ${n} value "${v.name}": missing doc string`)
    }
  }
  for (const [n, t] of Object.entries(spec.types)) checkFields(n, t.fields)
  for (const [n, e] of Object.entries(spec.entities)) checkFields(n, e.fields)
  for (const ep of spec.endpoints) {
    if (!ep.doc) errs.push(`endpoint ${ep.id}: missing doc string`)
    if (!AUTH_VALUES.has(ep.auth)) errs.push(`endpoint ${ep.id}: auth "${ep.auth}" is not one of ${[...AUTH_VALUES].join(', ')}`)
    for (const q of [...(ep.query ?? []), ...(ep.params ?? [])]) {
      const { base } = parseType(q.type)
      if (!known.has(base)) errs.push(`endpoint ${ep.id} param ${q.name}: unknown type "${q.type}"`)
      if (!q.doc) errs.push(`endpoint ${ep.id} param ${q.name}: missing doc string`)
      if (!IDENT.test(q.name)) errs.push(`endpoint ${ep.id} param ${q.name}: name must match ${IDENT}`)
    }
    // Request/response types must resolve, unless they are an externally-owned
    // OTLP message (prefixed `otlp.`) or a bare primitive.
    for (const [slot, ref] of [['request', ep.request], ['response', ep.response]]) {
      if (!ref) continue
      const { base } = parseType(ref.type)
      if (base.startsWith('otlp.')) continue
      if (!known.has(base)) errs.push(`endpoint ${ep.id} ${slot}: unknown type "${ref.type}"`)
    }
    // The extra statuses an endpoint answers. 200 is `response`; 400, 401 and
    // 429 are every endpoint's. Anything else is declared here so the OpenAPI
    // document and the reference can say what a 404 or a 409 means.
    for (const [code, r] of Object.entries(ep.responses ?? {})) {
      if (!/^[1-5]\d\d$/.test(code)) errs.push(`endpoint ${ep.id} responses: "${code}" is not an HTTP status`)
      if (code === '200') errs.push(`endpoint ${ep.id} responses: 200 belongs in "response", not "responses"`)
      if (!r || !r.doc) errs.push(`endpoint ${ep.id} responses ${code}: missing doc string`)
      if (r?.type && !known.has(parseType(r.type).base)) errs.push(`endpoint ${ep.id} responses ${code}: unknown type "${r.type}"`)
    }
  }
  if (errs.length) {
    console.error('Spec validation failed:\n  ' + errs.join('\n  '))
    process.exit(1)
  }
}

export const pascal = s => s.replace(/(^|[_\-\s]+)([a-z0-9])/g, (_, __, c) => c.toUpperCase())
export const camel  = s => { const p = pascal(s); return p[0].toLowerCase() + p.slice(1) }
export const snake  = s => s.replace(/([a-z0-9])([A-Z])/g, '$1_$2').replace(/[\s\-]+/g, '_').toLowerCase()
export const SCREAM = s => snake(s).toUpperCase()

/** Wrap a doc string as a comment block with the given line prefix. */
export function comment(doc, prefix = '/// ', indent = '') {
  if (!doc) return ''
  const words = doc.split(/\s+/)
  const lines = []
  let cur = ''
  for (const w of words) {
    if ((cur + ' ' + w).trim().length > 84) { lines.push(cur.trim()); cur = w }
    else cur += ' ' + w
  }
  if (cur.trim()) lines.push(cur.trim())
  return lines.map(l => `${indent}${prefix}${l}`).join('\n')
}

export const BANNER_LINES = [
  'GENERATED FILE. DO NOT EDIT.',
  '',
  'Produced from spec/mcpd.spec.json by tools/codegen.',
  'Edit the spec and run `make gen`. `make check` fails if this file drifts.',
]

export function banner(prefix = '// ') {
  return BANNER_LINES.map(l => (l ? prefix + l : prefix.trimEnd())).join('\n') + '\n'
}

export function write(rel, content) {
  const p = path.join(ROOT, rel)
  fs.mkdirSync(path.dirname(p), { recursive: true })
  const prev = fs.existsSync(p) ? fs.readFileSync(p, 'utf8') : null
  if (prev === content) return { path: rel, changed: false }
  fs.writeFileSync(p, content)
  return { path: rel, changed: true }
}

/** Entities and types together, in declaration order — SDKs need both. */
export function allObjects(spec) {
  return [
    ...Object.entries(spec.types).map(([name, def]) => ({ name, def, kind: 'type' })),
    ...Object.entries(spec.entities).map(([name, def]) => ({ name, def, kind: 'entity' })),
  ]
}

/** Render a spec `default` value as a source literal for the target language. */
export function defaultLiteral(lang, type, value) {
  const { base } = parseType(type)
  const isStr = base === 'string'
  switch (lang) {
    case 'rust': {
      const { optional, base } = parseType(type)
      let lit
      if (isStr) lit = `${JSON.stringify(value)}.to_string()`
      else if (base === 'f64') lit = Number.isInteger(value) ? `${value}.0` : String(value)
      else lit = String(value)
      return optional ? `Some(${lit})` : lit
    }
    case 'ts':     return JSON.stringify(value)
    case 'python': return typeof value === 'boolean' ? (value ? 'True' : 'False') : JSON.stringify(value)
    case 'go': {
      const { optional, base } = parseType(type)
      const lit = base === 'f64'
        ? (Number.isInteger(value) ? `${value}` : String(value))
        : JSON.stringify(value)
      // Go has no addressable literal, so an optional field's default has to go
      // through a helper that returns a pointer.
      if (!optional) return lit
      const helper = { f64: 'ptrFloat64', u64: 'ptrUint64', u32: 'ptrUint32', i64: 'ptrInt64', string: 'ptrString', bool: 'ptrBool' }[base]
      if (!helper) throw new Error(`no Go pointer helper for an optional ${base} with a default`)
      return `${helper}(${lit})`
    }
    case 'kotlin': {
      const { base } = parseType(type)
      if (isStr) return JSON.stringify(value)
      // Kotlin does not widen Int to Double implicitly: `val x: Double? = 0`
      // does not compile.
      if (base === 'f64') return Number.isInteger(value) ? `${value}.0` : String(value)
      return String(value)
    }
  }
}

/**
 * Order fields so a language with positional defaults stays legal:
 * required-with-no-default, then defaulted, then optional.
 */
export function orderFields(fields) {
  const rank = f => {
    const o = parseType(f.type).optional
    if (!o && f.default === undefined) return 0
    if (!o) return 1
    return 2
  }
  return [...fields].sort((a, b) => rank(a) - rank(b))
}

/**
 * A block comment whose continuation lines are safe in every target language.
 *
 * Kotlin block comments NEST, so a wrapped doc whose every line starts with
 * `/**` opens a new comment on each line and never closes: the file stops
 * compiling. This emits one opener, ` * ` continuations, and one closer.
 */
export function blockComment(doc, indent = '') {
  if (!doc) return ''
  const words = doc.split(/\s+/)
  const lines = []
  let cur = ''
  for (const w of words) {
    if ((cur + ' ' + w).trim().length > 80) { lines.push(cur.trim()); cur = w }
    else cur += ' ' + w
  }
  if (cur.trim()) lines.push(cur.trim())
  if (lines.length === 1) return `${indent}/** ${lines[0]} */`
  return [
    `${indent}/**`,
    ...lines.map(l => `${indent} * ${l}`),
    `${indent} */`,
  ].join('\n')
}
