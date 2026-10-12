import { parseType, snake, pascal, banner, allObjects, orderFields } from './lib.mjs'

/**
 * The Elixir emitter.
 *
 * Elixir has no static type system to generate structs against, so the shape a
 * generator can usefully carry is different from the other five: a `defstruct`
 * for the fields and their defaults, a `@type t` for the dialyzer, and the
 * enums as functions returning their values. That is the honest maximum, and
 * more than it sounds, because `@enforce_keys` turns a missing required field
 * into a compile-time error rather than a nil that reaches the wire.
 */

const MAP = {
  string: 'String.t()',
  bool: 'boolean()',
  f64: 'float()',
  u64: 'non_neg_integer()',
  u32: 'non_neg_integer()',
  i64: 'integer()',
  json: 'term()',
  timestamp: 'DateTime.t()',
}

/** A module name for a spec type. `Mcpd.Span`, `Mcpd.TokenUsage`. */
const mod = (name) => `Mcpd.${pascal(name)}`

function ty(t, spec) {
  const { base, array, optional } = parseType(t)
  let r = MAP[base]
  if (!r) {
    // An enum is its own atom union; an object is a nested struct.
    r = spec.enums[base] ? `Mcpd.${pascal(base)}.t()` : `${mod(base)}.t()`
  }
  if (array) r = `[${r}]`
  if (optional) r = `${r} | nil`
  return r
}

/** Elixir literal for a spec default. */
function lit(type, value) {
  const { base } = parseType(type)
  if (value === null) return 'nil'
  if (base === 'string') return JSON.stringify(value)
  if (base === 'bool') return value ? 'true' : 'false'
  if (base === 'f64') return Number.isInteger(value) ? `${value}.0` : String(value)
  return String(value)
}

export function emitElixir(spec) {
  const o = [banner('# '), '']
  o.push('# credo:disable-for-this-file')
  o.push('')

  // Elixir has no module-level constant a caller outside the module can read,
  // so the spec version is an attribute behind a function. It gets its own
  // module rather than hanging off one of the structs, because it belongs to
  // all of them equally.
  o.push('defmodule Mcpd.Spec do')
  o.push('  @moduledoc """')
  o.push('  The version of `spec/mcpd.spec.json` every module in this file came from.')
  o.push('')
  o.push('  A daemon reports the same string as `spec_version` on `/healthz`. Compare')
  o.push('  the two at connect time. A mismatch caught there names the problem; the')
  o.push('  same mismatch found later shows up as a field missing for no visible')
  o.push('  reason.')
  o.push('  """')
  o.push('')
  o.push(`  @spec_version "${spec.specVersion}"`)
  o.push('')
  o.push('  @doc "The spec version this SDK was generated from."')
  o.push('  @spec spec_version() :: String.t()')
  o.push('  def spec_version, do: @spec_version')
  o.push('end')
  o.push('')

  // ------------------------------------------------------------- enums ---
  for (const [name, e] of Object.entries(spec.enums)) {
    const M = `Mcpd.${pascal(name)}`
    o.push(`defmodule ${M} do`)
    // Written line by line rather than through `comment()`: that helper
    // prefixes every line, which turns a multi-line doc into a repeated
    // `@moduledoc """` inside the doc it is supposed to open.
    o.push('  @moduledoc """')
    for (const line of String(e.doc).split('\n')) o.push(`  ${line}`.trimEnd())
    o.push('  """')
    o.push('')
    const atoms = e.values.map((v) => `:${snake(v.name)}`)
    const oneLine = `  @type t :: ${atoms.join(' | ')}`
    if (oneLine.length <= 98) {
      o.push(oneLine)
    } else {
      o.push('  @type t ::')
      atoms.forEach((a, i) => o.push(`          ${a}${i === atoms.length - 1 ? '' : ' |'}`))
    }
    o.push('')
    o.push('  @values [')
    for (const v of e.values) o.push(`    :${snake(v.name)},`)
    o.push('  ]')
    o.push('')
    o.push('  @doc "Every value, in declaration order."')
    o.push('  @spec values() :: [t()]')
    o.push('  def values, do: @values')
    o.push('')
    o.push('  @doc "The wire representation of a value."')
    o.push('  @spec to_wire(t()) :: String.t()')
    for (const v of e.values) o.push(`  def to_wire(:${snake(v.name)}), do: "${v.name}"`)
    o.push('')
    o.push('  @doc """')
    o.push('  Parse a wire value.')
    o.push('')
    o.push('  Returns `:error` on an unknown value rather than raising: a newer daemon')
    o.push('  may send a value this build has never heard of, and an SDK that crashes on')
    o.push('  one it does not recognise breaks the agent it is meant to be observing.')
    o.push('  """')
    o.push('  @spec from_wire(String.t()) :: {:ok, t()} | :error')
    for (const v of e.values) o.push(`  def from_wire("${v.name}"), do: {:ok, :${snake(v.name)}}`)
    o.push('  def from_wire(_), do: :error')

    if (e.values.every((v) => 'severity' in v)) {
      o.push('')
      o.push('  @doc "Ordered severity. Filtering is always `>=`."')
      o.push('  @spec severity(t()) :: integer()')
      for (const v of e.values) o.push(`  def severity(:${snake(v.name)}), do: ${v.severity}`)
    }
    o.push('end')
    o.push('')
  }

  // ----------------------------------------------------------- structs ---
  for (const { name, def } of allObjects(spec)) {
    const M = mod(name)
    const fields = orderFields(def.fields)
    const required = fields.filter((f) => !parseType(f.type).optional && f.default === undefined)

    o.push(`defmodule ${M} do`)
    o.push('  @moduledoc """')
    for (const line of String(def.doc).split('\n')) o.push(`  ${line}`.trimEnd())
    if (def.table) {
      o.push('')
      o.push(`  Stored in the \`${def.table}\` table.`)
    }
    o.push('  """')
    o.push('')
    o.push('  @type t :: %__MODULE__{')
    o.push(fields.map((f) => `          ${snake(f.name)}: ${ty(f.type, spec)}`).join(',\n'))
    o.push('        }')
    o.push('')

    if (required.length) {
      // A required field left out is a compile error here rather than a nil
      // that only fails once it reaches the daemon.
      o.push(`  @enforce_keys [${required.map((f) => `:${snake(f.name)}`).join(', ')}]`)
    }
    o.push('  defstruct [')
    for (const f of fields) {
      const d = f.default !== undefined ? lit(f.type, f.default) : null
      const { array } = parseType(f.type)
      if (d !== null) o.push(`    ${snake(f.name)}: ${d},`)
      else if (array) o.push(`    ${snake(f.name)}: [],`)
      else o.push(`    ${snake(f.name)}: nil,`)
    }
    o.push('  ]')

    if (def.table) {
      o.push('')
      o.push('  @doc "The storage table this entity lives in."')
      o.push('  @spec table() :: String.t()')
      o.push(`  def table, do: "${def.table}"`)
    }
    if (def.primaryKey) {
      o.push('')
      o.push('  @doc "The primary key, as declared in the spec."')
      o.push('  @spec primary_key() :: [String.t()]')
      o.push(`  def primary_key, do: [${def.primaryKey.map((k) => `"${snake(k)}"`).join(', ')}]`)
    }
    o.push('end')
    o.push('')
  }

  return o.join('\n').replace(/\n{3,}/g, '\n\n') + '\n'
}
