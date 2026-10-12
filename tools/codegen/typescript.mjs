import { parseType, pascal, camel, comment, banner, allObjects, defaultLiteral } from './lib.mjs'

const MAP = { string:'string', bool:'boolean', f64:'number', u64:'number', u32:'number',
  i64:'number', json:'unknown', timestamp:'string' }

function ty(t) {
  const { base, array } = parseType(t)
  let r = MAP[base] ?? pascal(base)
  if (array) r = `${r}[]`
  return r
}

/**
 * Emit the TypeScript model.
 *
 * Two shapes, because two consumers handle genuinely different bytes:
 *
 * - `wire: false` (the SDK) gives idiomatic camelCase. The SDK converts to and
 *   from the daemon's snake_case in one place, its transport layer, the way the
 *   Rust, Python, Go and Kotlin SDKs use serde attributes and json tags.
 * - `wire: true` (the operator interface) gives the daemon's own field names.
 *   That page calls `fetch` directly with no mapping layer, so camelCase types
 *   there would simply be a lie about the object in hand.
 */
export function emitTypeScript(spec, { wire = false } = {}) {
  const key = (n) => (wire ? n : camel(n))
  const o = [banner('// '), '',
    wire
      ? '// Field names here are the daemon\'s own, because this model is used with'
      : '// Field names here are camelCase. The SDK converts to and from the daemon\'s',
    wire
      ? '// raw `fetch` and no conversion layer.'
      : '// snake_case in its transport layer, in one place.',
    '']

  o.push('/**')
  o.push(' * The version of `spec/mcpd.spec.json` these types were generated from.')
  o.push(' *')
  o.push(' * A daemon reports the same string on `/healthz`. Compare the two at connect')
  o.push(' * time. A mismatch caught there names the problem; the same mismatch found')
  o.push(' * later shows up as a field missing for no visible reason.')
  o.push(' */')
  o.push(`export const specVersion: string = '${spec.specVersion}'`)
  o.push('')
  for (const [name, e] of Object.entries(spec.enums)) {
    o.push(comment(e.doc, ' * ', ' ').replace(/^/, '/**\n') + ' */')
    o.push(`export const ${pascal(name)} = {`)
    for (const v of e.values) {
      o.push(comment(v.doc, '  // '))
      o.push(`  ${pascal(v.name)}: '${v.name}',`)
    }
    o.push('} as const', '')
    o.push(`export type ${pascal(name)} = typeof ${pascal(name)}[keyof typeof ${pascal(name)}]`, '')
    o.push(`/** Every \`${pascal(name)}\` value, in declaration order. */`)
    o.push(`export const ALL_${name.replace(/([a-z])([A-Z])/g,'$1_$2').toUpperCase()}: readonly ${pascal(name)}[] = [${e.values.map(v => `'${v.name}'`).join(', ')}]`, '')
  }
  for (const { name, def } of allObjects(spec)) {
    o.push('/**')
    o.push(comment(def.doc, ' * ').trimEnd())
    if (def.table) o.push(` * Stored in the \`${def.table}\` table.`)
    o.push(' */')
    o.push(`export interface ${pascal(name)} {`)
    for (const f of def.fields) {
      o.push('  /**')
      o.push(comment(f.doc, '   * ').trimEnd())
      o.push('   */')
      const { optional } = parseType(f.type)
      o.push(`  ${key(f.name)}${optional ? '?' : ''}: ${ty(f.type)}`)
    }
    o.push('}', '')
    const defaulted = def.fields.filter(f => f.default !== undefined)
    if (defaulted.length) {
      o.push(`/** Spec-declared defaults for \`${pascal(name)}\`. Spread these when constructing one. */`)
      o.push(`export const ${name[0].toLowerCase() + name.slice(1)}Defaults = {`)
      for (const f of defaulted) o.push(`  ${key(f.name)}: ${defaultLiteral('ts', f.type, f.default)},`)
      o.push(`} satisfies Partial<${pascal(name)}>`, '')
    }
  }
  return o.join('\n')
}
