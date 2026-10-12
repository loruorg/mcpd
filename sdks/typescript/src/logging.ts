/**
 * The logging signal: what a field map may hold, and how a level relates to an
 * OpenTelemetry severity number.
 *
 * Nothing here throws. A field the caller handed us is data, and data that
 * cannot be encoded is worth less than the batch it travels in.
 */

import type { LogLevel } from './generated.js'
import { ALL_LOG_LEVEL } from './generated.js'

/** An error, flattened into something JSON can carry. */
export interface ErrorFields {
  /** The constructor name, such as `TypeError`. */
  type: string
  /** What it says. */
  message: string
  /** Where it was thrown, when the value carries one. */
  stack?: string
}

/**
 * The severity number the spec gives each level.
 *
 * `spec/mcpd.spec.json` carries these on the `LogLevel` enum and the emitters
 * for the other five languages generate them. The TypeScript emitter writes a
 * const object with no members, so the numbers are repeated here; the test
 * suite asserts this table covers every value of the generated enum, so a level
 * added to the spec fails a test rather than reaching a user as a zero.
 */
const SEVERITY: Record<LogLevel, number> = {
  trace: 1,
  debug: 5,
  info: 9,
  warn: 13,
  error: 17,
  fatal: 21,
}

/** The OpenTelemetry severity number for a level. */
export function logSeverity(level: LogLevel): number {
  return SEVERITY[level] ?? SEVERITY.info
}

/**
 * The level an OpenTelemetry severity number lands on.
 *
 * By range, the way the daemon reads an inbound OTLP `LogRecord`: 1 to 4 is
 * trace, 5 to 8 debug, 9 to 12 info, 13 to 16 warn, 17 to 20 error, and 21 and
 * above fatal. A number outside the range is clamped rather than refused,
 * because a sender that invents a 40 still meant something serious.
 */
export function logLevelFromSeverity(severity: number): LogLevel {
  const ordered = [...ALL_LOG_LEVEL].sort((a, b) => logSeverity(a) - logSeverity(b))
  let level = ordered[0] as LogLevel
  for (const candidate of ordered) {
    if (severity >= logSeverity(candidate)) level = candidate
  }
  return level
}

/** True when `level` is at or above the floor. */
export function meetsLevel(level: LogLevel, floor: LogLevel): boolean {
  return logSeverity(level) >= logSeverity(floor)
}

/** Read a level by name, case-insensitively. Anything else is undefined. */
export function parseLogLevel(name: string | undefined): LogLevel | undefined {
  if (name === undefined) return undefined
  const wanted = name.trim().toLowerCase()
  return ALL_LOG_LEVEL.find((level) => level === wanted)
}

/**
 * An `Error` as `type`, `message` and `stack`.
 *
 * `JSON.stringify(new Error('boom'))` is `{}`: neither `message` nor `stack` is
 * an enumerable own property, so an error logged under a field key arrives at
 * the daemon as an empty object and the one thing the reader needed is gone.
 */
export function describeError(value: unknown): ErrorFields | undefined {
  if (!(value instanceof Error)) return undefined
  const described: ErrorFields = {
    type: value.name || 'Error',
    message: value.message || value.name || 'Error',
  }
  if (typeof value.stack === 'string' && value.stack.length > 0) described.stack = value.stack
  return described
}

/**
 * A caller's field map, in a shape `JSON.stringify` is guaranteed to accept.
 *
 * Two failures are being prevented, and the second is the expensive one. An
 * error collapses to `{}` unless it is flattened. A cycle, a `BigInt` or a
 * value whose `toJSON` throws makes `JSON.stringify` throw inside the transport,
 * where the batch is already assembled: the log line, and every span, metric,
 * score and piece of feedback that shared the batch, are lost together. One
 * unencodable field costs its own key and nothing else.
 */
export function encodeFields(fields: unknown): unknown {
  return encodeValue(fields, new Set<object>(), 0)
}

const MAX_DEPTH = 12

function encodeValue(value: unknown, seen: Set<object>, depth: number): unknown {
  if (value === null) return null
  const type = typeof value
  if (type === 'string' || type === 'boolean') return value
  if (type === 'number') return Number.isFinite(value as number) ? value : String(value)
  if (type === 'bigint') return (value as bigint).toString()
  if (type === 'undefined') return undefined
  if (type === 'function' || type === 'symbol') return String(value)
  const described = describeError(value)
  if (described !== undefined) return described
  if (value instanceof Date) return value.toISOString()
  if (typeof value !== 'object') return String(value)

  const object = value as object
  // A cycle is the common way a caller's object cannot be encoded: a request
  // object holding its own response, a linked node, a class with a back
  // reference. Naming it is more use to the reader than dropping the key.
  if (seen.has(object)) return '[circular]'
  if (depth >= MAX_DEPTH) return '[too deep]'
  seen.add(object)
  try {
    if (Array.isArray(object)) {
      return object.map((item) => {
        const encoded = encodeValue(item, seen, depth + 1)
        return encoded === undefined ? null : encoded
      })
    }
    if (object instanceof Map) {
      const out: Record<string, unknown> = {}
      for (const [key, item] of object) out[String(key)] = encodeValue(item, seen, depth + 1)
      return out
    }
    if (object instanceof Set) {
      return [...object].map((item) => encodeValue(item, seen, depth + 1))
    }
    const source = toJsonable(object)
    if (source !== object) return encodeValue(source, seen, depth + 1)
    const out: Record<string, unknown> = {}
    for (const key of Object.keys(object)) {
      let raw: unknown
      try {
        raw = (object as Record<string, unknown>)[key]
      } catch {
        // A getter that throws is the caller's problem with that one key.
        out[key] = '[unreadable]'
        continue
      }
      const encoded = encodeValue(raw, seen, depth + 1)
      if (encoded !== undefined) out[key] = encoded
    }
    return out
  } catch {
    return safeString(object)
  } finally {
    seen.delete(object)
  }
}

/** Let a value serialise itself, the way `JSON.stringify` would ask it to. */
function toJsonable(value: object): unknown {
  const candidate = (value as { toJSON?: unknown }).toJSON
  if (typeof candidate !== 'function') return value
  try {
    return (candidate as () => unknown).call(value)
  } catch {
    return safeString(value)
  }
}

function safeString(value: unknown): string {
  try {
    return String(value)
  } catch {
    return '[unencodable]'
  }
}
