/**
 * Prompts: the variable rule, rendering, the fallback shape and the read cache.
 *
 * The variable rule is the daemon's own (`variables_in` in
 * `crates/mcpd-server/src/prompts.rs`). A prompt rendered here must name the
 * same variables the daemon lists for it, or the list a reader sees in the UI
 * and the holes an agent fills would disagree.
 */

import type { Prompt } from './generated.js'

/** What `renderPrompt` returns: a string for a `text` prompt, messages for a `chat` one. */
export type RenderedPrompt = string | unknown[]

/** What `getPrompt` serves when the daemon cannot answer and nothing was fetched before. */
export type PromptFallback = string | unknown[]

/** A render that would leave a hole in the prompt. `missing` is in order of first use. */
export class PromptRenderError extends Error {
  readonly missing: string[]

  constructor(missing: string[], message?: string) {
    super(message ?? `prompt variables missing: ${missing.join(', ')}`)
    this.name = 'PromptRenderError'
    this.missing = missing
  }
}

// Rust's `str::trim` strips Unicode White_Space, which is not the set
// `String.prototype.trim` strips (JS adds U+FEFF and lacks U+0085). The daemon
// decides what a name is, so its set is the one used.
const RUST_WHITESPACE = new Set([
  0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x20, 0x85, 0xa0, 0x1680, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004,
  0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200a, 0x2028, 0x2029, 0x202f, 0x205f, 0x3000,
])

function rustTrim(s: string): string {
  let start = 0
  let end = s.length
  while (start < end && RUST_WHITESPACE.has(s.charCodeAt(start))) start++
  while (end > start && RUST_WHITESPACE.has(s.charCodeAt(end - 1))) end--
  return s.slice(start, end)
}

const NAME = /^[A-Za-z_][A-Za-z0-9_.]*$/

/**
 * Walk the placeholders of a template in order.
 *
 * `visit` gets the raw text between the braces and the trimmed name when it is
 * one, and returns what replaces the placeholder. A `{{` with no `}}` after it
 * ends the walk, and the rest is kept as written.
 */
function walk(template: string, visit: (raw: string, name: string | undefined) => string): string {
  let out = ''
  let rest = template
  for (;;) {
    const open = rest.indexOf('{{')
    if (open < 0) break
    const after = rest.slice(open + 2)
    const close = after.indexOf('}}')
    if (close < 0) break
    const raw = after.slice(0, close)
    const name = rustTrim(raw)
    out += rest.slice(0, open) + visit(raw, NAME.test(name) ? name : undefined)
    rest = after.slice(close + 2)
  }
  return out + rest
}

/** Every variable a template names, in order of first use, without duplicates. */
export function variablesIn(template: string, out: string[] = []): string[] {
  walk(template, (raw, name) => {
    if (name !== undefined && !out.includes(name)) out.push(name)
    return ''
  })
  return out
}

/**
 * Every variable named by any string inside a value.
 *
 * Object values are read in sorted key order because the daemon's JSON map is
 * sorted, and the order of first use has to match the list it stores.
 */
function variablesInValue(value: unknown, out: string[]): string[] {
  if (typeof value === 'string') {
    variablesIn(value, out)
  } else if (Array.isArray(value)) {
    for (const item of value) variablesInValue(item, out)
  } else if (value !== null && typeof value === 'object') {
    const record = value as Record<string, unknown>
    for (const key of Object.keys(record).sort()) variablesInValue(record[key], out)
  }
  return out
}

function substitute(template: string, variables: Record<string, unknown>, missing: string[]): string {
  return walk(template, (raw, name) => {
    if (name === undefined) return `{{${raw}}}`
    const value = Object.prototype.hasOwnProperty.call(variables, name) ? variables[name] : undefined
    if (value === undefined) {
      if (!missing.includes(name)) missing.push(name)
      return ''
    }
    return String(value)
  })
}

/**
 * Fill a prompt's variables.
 *
 * A `text` prompt renders to a string. A `chat` prompt renders to a copy of its
 * messages with every string `content` rendered and every other field kept.
 * Substitution is one pass, so a value containing `{{x}}` is written as it is.
 * Throws `PromptRenderError` naming every variable the prompt uses and
 * `variables` lacks, because a prompt with a hole in it is worse than no call.
 */
export function renderPrompt(prompt: Prompt, variables: Record<string, unknown> = {}): RenderedPrompt {
  if (prompt === null || typeof prompt !== 'object') {
    throw new PromptRenderError([], 'renderPrompt needs a prompt, such as one from getPrompt')
  }
  const vars = variables ?? {}
  const missing: string[] = []
  let rendered: RenderedPrompt
  if (prompt.kind === 'chat') {
    const messages = Array.isArray(prompt.messages) ? prompt.messages : []
    rendered = messages.map((message: unknown) => {
      if (message === null || typeof message !== 'object' || Array.isArray(message)) return message
      const copy = { ...(message as Record<string, unknown>) }
      if (typeof copy['content'] === 'string') copy['content'] = substitute(copy['content'], vars, missing)
      return copy
    })
  } else {
    rendered = substitute(typeof prompt.template === 'string' ? prompt.template : '', vars, missing)
  }
  if (missing.length > 0) throw new PromptRenderError(missing)
  return rendered
}

/** The `Prompt` a fallback stands for: version 0, no labels, never stored. */
export function fallbackPrompt(project: string, name: string, fallback: PromptFallback): Prompt {
  const base = { project, name, version: 0, labels: [], createdAt: '' }
  if (typeof fallback === 'string') {
    return { ...base, kind: 'text', template: fallback, variables: variablesIn(fallback) }
  }
  const messages = [...fallback]
  return { ...base, kind: 'chat', messages, variables: variablesInValue(messages, []) }
}

/** The attributes `span.setPrompt` writes, or none for anything that is not a prompt. */
export function promptAttributes(prompt: unknown): Record<string, unknown> | undefined {
  if (prompt === null || typeof prompt !== 'object') return undefined
  const { name, version } = prompt as { name?: unknown; version?: unknown }
  if (typeof name !== 'string' || name.length === 0) return undefined
  const out: Record<string, unknown> = { 'mcpd.prompt.name': name }
  // Version 0 is a fallback, which no stored version matches.
  if (typeof version === 'number' && Number.isInteger(version) && version > 0) {
    out['mcpd.prompt.version'] = version
  }
  return out
}

/**
 * Fetched prompts, bounded (B3).
 *
 * A `Map` keeps insertion order, and a refreshed entry is reinserted, so the
 * first key is always the one fetched longest ago.
 */
export class PromptCache {
  readonly #entries = new Map<string, { prompt: Prompt; at: number }>()
  readonly #max: number

  constructor(max = 256) {
    this.#max = max
  }

  get size(): number {
    return this.#entries.size
  }

  get(key: string): { prompt: Prompt; at: number } | undefined {
    return this.#entries.get(key)
  }

  set(key: string, prompt: Prompt, at: number): void {
    this.#entries.delete(key)
    this.#entries.set(key, { prompt, at })
    while (this.#entries.size > this.#max) {
      const oldest = this.#entries.keys().next().value
      if (oldest === undefined) break
      this.#entries.delete(oldest)
    }
  }
}
