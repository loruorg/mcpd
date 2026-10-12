import { parseType, pascal, snake, SCREAM, comment, banner, allObjects, defaultLiteral, orderFields } from './lib.mjs'

const MAP = { string:'str', bool:'bool', f64:'float', u64:'int', u32:'int', i64:'int',
  json:'Any', timestamp:'datetime' }

// Spec names that collide with Python syntax. `from` is the only one today; the
// set exists so a future spec field called `class` or `lambda` fails safely
// instead of emitting a file that will not parse.
const KEYWORDS = new Set(['False','None','True','and','as','assert','async','await','break',
  'class','continue','def','del','elif','else','except','finally','for','from','global','if',
  'import','in','is','lambda','nonlocal','not','or','pass','raise','return','try','while',
  'with','yield'])

/** The attribute name for a spec field. Keywords get a trailing underscore. */
function attr(name) {
  const s = snake(name)
  return KEYWORDS.has(s) ? `${s}_` : s
}

function ty(t) {
  const { base, array, optional } = parseType(t)
  let r = MAP[base] ?? pascal(base)
  if (array) r = `list[${r}]`
  if (optional) r = `${r} | None`
  return r
}

/** The default clause for a field, or '' when it has none. */
function defaultClause(f) {
  const p = parseType(f.type)
  if (f.default !== undefined) return ` = ${defaultLiteral('python', f.type, f.default)}`
  if (p.optional) return ' = None'
  if (p.array) return ' = field(default_factory=list)'
  return ''
}

/**
 * Python's rule is stricter than the shared one: a field with `default_factory`
 * counts as defaulted, so a required list must still sort after a required
 * scalar. Sorting here rather than in `orderFields` keeps the other four
 * languages' field order untouched.
 */
function pythonOrder(fields) {
  const ranked = orderFields(fields)
  return [...ranked].sort((a, b) => (defaultClause(a) ? 1 : 0) - (defaultClause(b) ? 1 : 0))
}

export function emitPython(spec) {
  const o = [banner('# '), '', 'from __future__ import annotations', '', 'import sys', '',
    'from dataclasses import dataclass, field', 'from datetime import datetime',
    'from enum import Enum', 'from typing import Any', '',
    '# `slots=True` arrived in 3.10. On 3.9 the same dataclasses are emitted without',
    '# it, which costs a little memory and changes nothing else.',
    '_SLOTS = {"slots": True} if sys.version_info >= (3, 10) else {}', '']

  o.push('#: The version of `spec/mcpd.spec.json` these types were generated from. A')
  o.push('#: daemon reports the same string on `/healthz`. Compare the two at connect')
  o.push('#: time. A mismatch caught there names the problem; the same mismatch found')
  o.push('#: later shows up as a field missing for no visible reason.')
  o.push(`SPEC_VERSION = "${spec.specVersion}"`)
  o.push('')

  for (const [name, e] of Object.entries(spec.enums)) {
    o.push(`class ${pascal(name)}(str, Enum):`)
    o.push(`    """${e.doc}"""`, '')
    for (const v of e.values) {
      o.push(`    ${SCREAM(v.name)} = "${v.name}"`)
      o.push(`    """${v.doc}"""`)
    }
    o.push('')
    if (e.values.every(v => 'severity' in v)) {
      o.push('    @property')
      o.push('    def severity(self) -> int:')
      o.push('        """Ordered severity. Filtering is always `>=`."""')
      o.push(`        return {${e.values.map(v => `"${v.name}": ${v.severity}`).join(', ')}}[self.value]`)
      o.push('')
    }
  }

  const renamed = []
  for (const { name, def } of allObjects(spec)) {
    o.push('@dataclass(**_SLOTS)')
    o.push(`class ${pascal(name)}:`)
    o.push(`    """${def.doc}`)
    if (def.table) o.push(`\n    Stored in the \`${def.table}\` table.`)
    o.push('    """', '')
    // required-no-default, then defaulted, then optional — dataclass ordering rule
    for (const f of pythonOrder(def.fields)) {
      const a = attr(f.name)
      if (a !== f.name) renamed.push([f.name, a])
      o.push(`    ${a}: ${ty(f.type)}${defaultClause(f)}`)
      o.push(`    """${f.doc}"""`)
    }
    if (def.table) {
      o.push('')
      o.push(`    TABLE = "${def.table}"`)
    }
    o.push('')
  }

  // A codec cannot guess that `from_` goes on the wire as `from`, so the
  // mapping is data, not a naming convention it has to reverse-engineer.
  o.push('#: Wire name -> attribute name, for the fields Python could not spell.')
  o.push('RESERVED_FIELD_NAMES = {')
  for (const [wire, a] of [...new Map(renamed).entries()]) o.push(`    "${wire}": "${a}",`)
  o.push('}')
  o.push('')
  return o.join('\n')
}
