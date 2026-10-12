import { parseType, pascal, comment, banner, allObjects, defaultLiteral } from './lib.mjs'

const MAP = { string:'string', bool:'bool', f64:'float64', u64:'uint64', u32:'uint32',
  i64:'int64', json:'json.RawMessage', timestamp:'time.Time' }

// Go initialisms — golint expects these upper-cased.
const INIT = { Id:'ID', Ids:'IDs', Url:'URL', Api:'API', Http:'HTTP', Json:'JSON', Ns:'Ns' }
function goName(s) {
  let p = pascal(s)
  for (const [k, v] of Object.entries(INIT)) {
    p = p.replace(new RegExp(`${k}(?=[A-Z]|$)`, 'g'), v)
  }
  return p
}

// gofmt aligns a run of one-line declarations into columns. The output has to
// match it byte for byte, or `gofmt -l` flags a file nobody may edit by hand.
function aligned(rows, indent = '') {
  const w = Math.max(...rows.map(([head]) => head.length))
  return rows.map(([head, tail]) => `${indent}${head.padEnd(w)} ${tail}`)
}

function ty(t) {
  const { base, array, optional } = parseType(t)
  let r = MAP[base] ?? goName(base)
  if (array) return `[]${r}`
  if (optional && base !== 'json') r = `*${r}`
  return r
}

export function emitGo(spec) {
  // banner() already ends with a newline, so the join supplies the one blank
  // line gofmt keeps. A second would be collapsed by gofmt; none would make
  // the banner the package's doc comment.
  const o = [banner('// '), 'package mcpd', '',
    'import (', '\t"encoding/json"', '\t"time"', ')', '',
    '// SpecVersion is the version of spec/mcpd.spec.json these types were generated',
    '// from. A daemon reports the same string on /healthz. Compare the two at connect',
    '// time. A mismatch caught there names the problem; the same mismatch found later',
    '// shows up as a field missing for no visible reason.',
    `const SpecVersion = "${spec.specVersion}"`,
    '',
    '// Go has no addressable literal, so a spec-declared default on an optional',
    '// field needs a helper that returns a pointer to it.',
    ...aligned([
      ['func ptrString(v string) *string', '{ return &v }'],
      ['func ptrBool(v bool) *bool', '{ return &v }'],
      ['func ptrFloat64(v float64) *float64', '{ return &v }'],
      ['func ptrUint64(v uint64) *uint64', '{ return &v }'],
      ['func ptrUint32(v uint32) *uint32', '{ return &v }'],
      ['func ptrInt64(v int64) *int64', '{ return &v }'],
    ]),
    '']

  for (const [name, e] of Object.entries(spec.enums)) {
    const T = goName(name)
    o.push(comment(e.doc, '// '))
    o.push(`type ${T} string`, '')
    o.push('const (')
    for (const v of e.values) {
      o.push(comment(v.doc, '\t// '))
      o.push(`\t${T}${goName(v.name)} ${T} = "${v.name}"`)
    }
    o.push(')', '')
    o.push(`// All${T}s lists every ${T} value, in declaration order.`)
    o.push(`var All${T}s = []${T}{${e.values.map(v => `${T}${goName(v.name)}`).join(', ')}}`, '')
    o.push(`// String returns the wire representation.`)
    o.push(`func (v ${T}) String() string { return string(v) }`, '')
    o.push(`// Valid reports whether v is a value this version of the spec defines.`)
    o.push(`func (v ${T}) Valid() bool {`)
    o.push(`\tfor _, c := range All${T}s {`, '\t\tif c == v {', '\t\t\treturn true', '\t\t}', '\t}', '\treturn false', '}', '')
    if (e.values.every(v => 'severity' in v)) {
      o.push(`// Severity returns the ordered severity. Filtering is always >=.`)
      o.push(`func (v ${T}) Severity() int {`)
      o.push(`\tswitch v {`)
      for (const v of e.values) o.push(`\tcase ${T}${goName(v.name)}:\n\t\treturn ${v.severity}`)
      o.push('\t}', '\treturn 0', '}', '')
    }
  }

  for (const { name, def } of allObjects(spec)) {
    o.push(comment(def.doc, '// '))
    if (def.table) o.push(`// Stored in the ${def.table} table.`)
    o.push(`type ${goName(name)} struct {`)
    for (const f of def.fields) {
      o.push(comment(f.doc, '\t// '))
      const { optional } = parseType(f.type)
      o.push(`\t${goName(f.name)} ${ty(f.type)} \`json:"${f.name}${optional ? ',omitempty' : ''}"\``)
    }
    o.push('}', '')
    const defaulted = def.fields.filter(f => f.default !== undefined)
    if (defaulted.length) {
      o.push(`// New${goName(name)} returns a ${goName(name)} with every spec-declared default applied.`)
      o.push(`func New${goName(name)}() ${goName(name)} {`)
      o.push(`\treturn ${goName(name)}{`)
      o.push(...aligned(defaulted.map(f => [`${goName(f.name)}:`, `${defaultLiteral('go', f.type, f.default)},`]), '\t\t'))
      o.push('\t}', '}', '')
    }
    if (def.table) {
      o.push(`// Table is the storage table ${goName(name)} lives in.`)
      o.push(`func (${goName(name)}) Table() string { return "${def.table}" }`, '')
    }
  }
  return o.join('\n')
}
