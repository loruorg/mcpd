import { parseType, pascal, camel, SCREAM, blockComment, banner, allObjects, defaultLiteral, orderFields } from './lib.mjs'

const MAP = { string:'String', bool:'Boolean', f64:'Double', u64:'Long', u32:'Int',
  i64:'Long', json:'JsonElement', timestamp:'Instant' }

// Kotlin's hard keywords cannot name a property without backticks, and the
// wire name is pinned by `@SerialName` anyway. `class` on `StepError` is the
// one that arrived; the whole list is here so the next one compiles too.
const KEYWORDS = new Set(['as', 'break', 'class', 'continue', 'do', 'else', 'false', 'for', 'fun',
  'if', 'in', 'interface', 'is', 'null', 'object', 'package', 'return', 'super', 'this', 'throw',
  'true', 'try', 'typealias', 'typeof', 'val', 'var', 'when', 'while'])

/** The property name for a spec field: camelCase, backticked when it is a keyword. */
function prop(name) {
  const c = camel(name)
  return KEYWORDS.has(c) ? `\`${c}\`` : c
}

function ty(t) {
  const { base, array, optional } = parseType(t)
  let r = MAP[base] ?? pascal(base)
  if (array) r = `List<${r}>`
  if (optional) r = `${r}?`
  return r
}

export function emitKotlin(spec) {
  const o = [banner('// '), '', 'package dev.mcpd', '',
    'import kotlinx.serialization.SerialName',
    'import kotlinx.serialization.Serializable',
    'import kotlinx.serialization.json.JsonElement',
    'import kotlin.time.Instant', '']

  o.push('/**')
  o.push(' * The version of `spec/mcpd.spec.json` these types were generated from.')
  o.push(' *')
  o.push(' * A daemon reports the same string on `/healthz`. Compare the two at connect')
  o.push(' * time. A mismatch caught there names the problem; the same mismatch found')
  o.push(' * later shows up as a field missing for no visible reason.')
  o.push(' */')
  o.push(`const val specVersion: String = "${spec.specVersion}"`)
  o.push('')

  for (const [name, e] of Object.entries(spec.enums)) {
    o.push(blockComment(e.doc))
    o.push('@Serializable')
    o.push(`enum class ${pascal(name)}(val wire: String) {`)
    for (const v of e.values) {
      o.push(blockComment(v.doc, '    '))
      o.push(`    @SerialName("${v.name}") ${SCREAM(v.name)}("${v.name}"),`)
    }
    o.push('    ;', '')
    o.push('    override fun toString(): String = wire', '')
    if (e.values.every(v => 'severity' in v)) {
      o.push('    /** Ordered severity. Filtering is always `>=`. */')
      o.push('    val severity: Int get() = when (this) {')
      for (const v of e.values) o.push(`        ${SCREAM(v.name)} -> ${v.severity}`)
      o.push('    }', '')
    }
    o.push('    companion object {')
    o.push(`        /** Parse a wire value, or null when this version of the spec does not define it. */`)
    o.push(`        fun fromWire(s: String): ${pascal(name)}? = entries.firstOrNull { it.wire == s }`)
    o.push('    }', '}', '')
  }

  for (const { name, def } of allObjects(spec)) {
    o.push(blockComment(def.doc + (def.table ? ` Stored in the \`${def.table}\` table.` : '')))
    o.push('@Serializable')
    o.push(`data class ${pascal(name)}(`)
    const fields = orderFields(def.fields)
    fields.forEach((f, i) => {
      o.push(blockComment(f.doc, '    '))
      const dflt = f.default !== undefined
        ? ` = ${defaultLiteral('kotlin', f.type, f.default)}`
        : parseType(f.type).optional ? ' = null' : ''
      o.push(`    @SerialName("${f.name}") val ${prop(f.name)}: ${ty(f.type)}${dflt}${i < fields.length - 1 ? ',' : ''}`)
    })
    o.push(') {')
    if (def.table) {
      o.push('    companion object {')
      o.push(`        /** The storage table this entity lives in. */`)
      o.push(`        const val TABLE = "${def.table}"`)
      o.push('    }')
    }
    o.push('}', '')
  }
  return o.join('\n')
}
