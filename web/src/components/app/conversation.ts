/**
 * One run read back as the conversation its spans already encode.
 *
 * Nothing here is added to the wire. mcpd stores no conversation: it stores a
 * tree of steps, and every step carries what went into it and what came out.
 * A model step's input is a prompt and its output is a completion; a tool
 * step's input is a call and its output is that call's result; a nested agent
 * is a change of speaker. Read in time order, those are already a transcript.
 *
 * The whole file is pure, so the view renders what it returns and the two can
 * never disagree about what was said. Two rules govern every decision below.
 *
 * It never guesses. Where a payload is not a message, the turn says so and
 * shows what the step actually recorded, in the measurement face, rather than
 * dressing a JSON blob as speech.
 *
 * It never repeats itself silently. A prompt grows by one message per step, so
 * rendering every prompt in full would print the same six messages six times.
 * Only what is new is shown, and the step that repeated the rest says how many
 * it repeated, because "this step said nothing new" is itself a reading, and on
 * a looping agent it is the whole diagnosis.
 */

import type { Span, TraceDetail } from '@/generated/types'
import { extentOf, stopReason } from './trace-tree'

/**
 * Who is speaking.
 *
 * `note` is not a speaker: it is the transcript telling you about itself, and
 * it is drawn quietly for that reason.
 */
export type Voice = 'user' | 'assistant' | 'system' | 'tool' | 'handoff' | 'fault' | 'note'

export interface Turn {
  /** Stable across renders; a span can produce more than one turn. */
  key: string
  voice: Voice
  /** The role, as the producer named it where it named one. */
  speaker: string
  /** Who or what that role was, this time: a model id, a tool name, an agent. */
  attribution?: string
  spanId: string
  spanName: string
  kind: string
  /** Milliseconds into the run, or null when the step's clock cannot be read. */
  offsetMs: number | null
  /** Prose, set in the reading face. */
  text?: string
  /** Machine data: arguments, a result, or a payload that is not prose. */
  data?: string
  dataLabel?: string
  /** A tool's result, kept apart from its call so the two can be labelled. */
  result?: string
  resultLabel?: string
  /** Why this turn reads the way it does, where the tree could not say plainly. */
  note?: string
  /** The step has not ended, so this turn is what there is so far. */
  running?: boolean
  error?: { message: string; kind?: string }
}

export interface SkippedStep {
  name: string
  kind: string
}

export interface Conversation {
  turns: Turn[]
  /** Steps that produced at least one turn. */
  used: number
  /** Steps in the run. */
  total: number
  /** Steps that carry no speech, named so the reader can go and read them. */
  skipped: SkippedStep[]
  /** True when no step in the run carries anything a reader could read back. */
  empty: boolean
}

/* --------------------------------------------------------------- kinds --- */

/*
 * Which kinds speak. Deliberately narrow: a retrieval, a guardrail and a
 * memory read are steps in the run and not turns in the conversation, and
 * folding them in would pad the transcript with things nobody said. They are
 * counted and named instead, so the reader knows exactly what is not here.
 */
const MODEL_KINDS = new Set(['model_generation', 'model_step', 'model_inference', 'model_chunk'])
const TOOL_KINDS = new Set(['tool_call', 'client_tool_call', 'provider_tool_call', 'mcp_tool_call'])
const AGENT_KINDS = new Set(['agent_run', 'workflow_run'])

/* ------------------------------------------------------------ payloads --- */

function obj(v: unknown): Record<string, unknown> | null {
  return v != null && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : null
}

/**
 * The keys a producer puts prose under.
 *
 * Order matters: `content` and `text` are the message shapes, and the rest are
 * the envelopes an agent framework wraps a question or an answer in. Anything
 * not on this list is data, not speech, and is shown as data.
 */
const PROSE_KEYS = [
  'content',
  'text',
  'answer',
  'reply',
  'message',
  'question',
  'prompt',
  'query',
  'completion',
  'response',
  'output',
  'input',
]

/**
 * Prose out of a payload, or null when the payload is not prose.
 *
 * Handles the three shapes text actually arrives in: a bare string, a list of
 * content blocks (`{type:'text', text}`), and a single-key envelope. `depth`
 * stops a self-referential envelope from recursing for ever.
 */
export function asProse(v: unknown, depth = 0): string | null {
  if (typeof v === 'string') return v.trim() ? v : null
  if (depth > 3) return null
  if (Array.isArray(v)) {
    const parts: string[] = []
    for (const item of v) {
      if (typeof item === 'string') {
        if (item.trim()) parts.push(item)
        continue
      }
      const o = obj(item)
      if (!o) continue
      // A tool_use block is a call, not speech: `callsOf` reads those.
      if (typeof o.type === 'string' && o.type !== 'text') continue
      const t = asProse(o.text ?? o.content, depth + 1)
      if (t) parts.push(t)
    }
    return parts.length ? parts.join('\n\n') : null
  }
  const o = obj(v)
  if (!o) return null
  for (const k of PROSE_KEYS) {
    if (!(k in o)) continue
    const t = asProse(o[k], depth + 1)
    if (t) return t
  }
  return null
}

/**
 * Keys that carry no information once the prose beside them has been read.
 *
 * `role` and `type` say what the envelope is, which the transcript has already
 * said by drawing a speaker. Listing them as "other fields" would put a note on
 * every completion mcpd holds.
 */
const PLUMBING = new Set(['role', 'name', 'id', 'type', 'index', 'refusal', 'finish_reason', 'finishReason', 'stop_reason', 'stopReason'])

/**
 * Which field the prose came out of, and what else the envelope carried.
 *
 * Reading one key out of an object and showing it as speech is the one place
 * this file could quietly drop something. It does not: the turn says which
 * field it read and names the fields it did not, so a reader who wants the
 * order id knows the step is holding it.
 */
export function proseSource(v: unknown): { key: string; others: string[] } | null {
  const o = obj(v)
  if (!o) return null
  for (const k of PROSE_KEYS) {
    if (!(k in o)) continue
    if (asProse(o[k], 1) != null) {
      return { key: k, others: Object.keys(o).filter((x) => x !== k && !PLUMBING.has(x)) }
    }
  }
  return null
}

/** "a, b and 2 more", for naming fields without printing a schema. */
function nameList(keys: string[]): string {
  if (keys.length <= 2) return keys.join(' and ')
  return `${keys.slice(0, 2).join(', ')} and ${keys.length - 2} more`
}

/** The sentence a turn carries when its prose was read out of an envelope. */
function readFrom(v: unknown, what: string): string | undefined {
  const src = proseSource(v)
  if (!src || !src.others.length) return undefined
  return `Read from the ${src.key} field of ${what}, which also carried ${nameList(src.others)}.`
}

export interface Call {
  name: string
  args?: unknown
}

/** The tool calls a completion asked for, in the shapes providers write them. */
export function callsOf(v: unknown): Call[] {
  const out: Call[] = []
  const push = (item: unknown) => {
    const o = obj(item)
    if (!o) return
    // OpenAI wraps the call in `function`; Anthropic writes a `tool_use` block.
    const fn = obj(o.function)
    if (fn && typeof fn.name === 'string') {
      out.push({ name: fn.name, args: fn.arguments })
      return
    }
    if (typeof o.name === 'string') {
      out.push({ name: o.name, args: o.arguments ?? o.args ?? o.input ?? o.parameters })
    }
  }
  const o = obj(v)
  if (o) {
    const list = o.tool_calls ?? o.toolCalls ?? o.tool_call
    if (Array.isArray(list)) list.forEach(push)
    else if (list) push(list)
    if (Array.isArray(o.content)) {
      for (const block of o.content) {
        const b = obj(block)
        if (b && (b.type === 'tool_use' || b.type === 'tool-call')) push(b)
      }
    }
  } else if (Array.isArray(v)) {
    for (const block of v) {
      const b = obj(block)
      if (b && (b.type === 'tool_use' || b.type === 'tool-call')) push(b)
    }
  }
  return out
}

export interface Msg {
  role?: string
  content?: unknown
  name?: string
}

/** The message list inside a prompt, in the shapes a prompt arrives in. */
export function messagesOf(v: unknown): Msg[] | null {
  const isMsgList = (x: unknown): x is Msg[] =>
    Array.isArray(x) && x.length > 0 && x.every((m) => obj(m) !== null && 'role' in (m as object))
  if (isMsgList(v)) return v
  const o = obj(v)
  if (!o) return null
  for (const key of ['messages', 'prompt', 'input', 'conversation']) {
    const inner = o[key]
    if (isMsgList(inner)) return inner
    const nested = obj(inner)
    if (nested && isMsgList(nested.messages)) return nested.messages as Msg[]
  }
  return null
}

/** A payload as data, for the cases prose cannot carry. */
function asData(v: unknown): string | null {
  if (v == null) return null
  if (typeof v === 'string') return v.trim() ? v : null
  try {
    const s = JSON.stringify(v, null, 2)
    return s && s !== '{}' && s !== '[]' ? s : null
  } catch {
    return String(v)
  }
}

/** Arguments as one line, where they are a flat object, else as data. */
function argLine(args: unknown): string | null {
  if (args == null) return null
  // A provider may send arguments as a JSON string. Read it, so the reader sees
  // `order_id: "4417"` rather than an escaped blob.
  let value = args
  if (typeof args === 'string') {
    const trimmed = args.trim()
    if (!trimmed) return null
    if (trimmed.startsWith('{')) {
      try {
        value = JSON.parse(trimmed)
      } catch {
        return trimmed
      }
    } else {
      return trimmed
    }
  }
  const o = obj(value)
  if (!o) return asData(value)
  const keys = Object.keys(o)
  if (!keys.length) return null
  const flat = keys.every((k) => {
    const t = typeof o[k]
    return t === 'string' || t === 'number' || t === 'boolean' || o[k] === null
  })
  if (!flat) return asData(value)
  return keys.map((k) => `${k}: ${JSON.stringify(o[k])}`).join('  ')
}

/* ------------------------------------------------------------ reading --- */

const ms = (iso: string): number => {
  const t = new Date(iso).getTime()
  return Number.isNaN(t) ? Number.NaN : t
}

/** How a message is recognised as one already read, whichever step carried it. */
function fingerprint(role: string, body: string): string {
  return `${role} ${body.slice(0, 2000)}`
}

/** The role a producer wrote, reduced to the four the transcript can voice. */
function voiceOf(role: string | undefined): Voice {
  const r = (role ?? '').toLowerCase()
  if (r === 'user' || r === 'human') return 'user'
  if (r === 'system' || r === 'developer') return 'system'
  if (r === 'tool' || r === 'function') return 'tool'
  if (r === 'assistant' || r === 'model' || r === 'ai') return 'assistant'
  return 'note'
}

/**
 * Drop an argument block that the very next turn repeats verbatim.
 *
 * A model asks for a tool with some arguments, and the tool step records the
 * same arguments as its input. Both are true, and mcpd holds both, so printing
 * two identical recesses one under the other is honest and unreadable. Where
 * they differ, which is the case worth seeing, both stay: this collapses only
 * the pair that says the same thing twice, and says that it did.
 */
function collapseRepeatedCall(turns: Turn[]): void {
  for (let i = 0; i < turns.length - 1; i++) {
    const ask = turns[i]
    const call = turns[i + 1]
    if (ask.voice !== 'assistant' || !ask.data) continue
    if (call.voice !== 'tool' || call.data !== ask.data) continue
    ask.data = undefined
    ask.dataLabel = undefined
    ask.note = [ask.note, 'Its arguments are the call shown below.'].filter(Boolean).join(' ')
  }
}

export function readConversation(detail: TraceDetail): Conversation {
  const spans = detail.spans ?? []
  const total = spans.length
  if (!total) return { turns: [], used: 0, total: 0, skipped: [], empty: true }

  const t0 = extentOf(spans).t0
  // Time order across the whole tree, not tree order: a tool call three levels
  // down that happened between two model calls belongs between them, which is
  // the one thing a waterfall shows and a flattened tree does not.
  const order = spans
    .map((s, i) => ({ s, i }))
    .sort((a, b) => {
      const da = ms(a.s.start_time)
      const db = ms(b.s.start_time)
      if (Number.isNaN(da) || Number.isNaN(db) || da === db) return a.i - b.i
      return da - db
    })
    .map((x) => x.s)

  const root = spans.find((s) => s.is_root) ?? order[0]
  const turns: Turn[] = []
  const seen = new Set<string>()
  const toolsShown = new Set<string>()
  const contributed = new Set<string>()
  const skipped: SkippedStep[] = []
  let n = 0

  const offsetOf = (s: Span): number | null => {
    const at = ms(s.start_time)
    return Number.isNaN(at) ? null : at - t0
  }

  const add = (
    s: Span,
    // `offsetMs` is optional rather than absent so the run's closing turns can
    // sit at the moment the root ENDED. Taking the root's start there would put
    // the run's answer at the top of its own clock, at the foot of the page.
    turn: Omit<Turn, 'key' | 'spanId' | 'spanName' | 'kind' | 'offsetMs'> & {
      offsetMs?: number | null
    },
  ) => {
    turns.push({
      key: `${s.span_id}-${n++}`,
      spanId: s.span_id,
      spanName: s.name,
      kind: s.kind,
      offsetMs: offsetOf(s),
      ...turn,
    })
    contributed.add(s.span_id)
  }

  /** Emit the messages of a prompt that have not been read yet. */
  const emitMessages = (s: Span, list: Msg[], from: string): { repeated: number; fed: number } => {
    let repeated = 0
    let fed = 0
    for (const m of list) {
      const prose = asProse(m.content)
      const data = prose ? null : asData(m.content)
      const body = prose ?? data ?? ''
      const role = (m.role ?? '').toLowerCase()
      const print = fingerprint(role, body)
      if (!body || seen.has(print)) {
        repeated += 1
        continue
      }
      // A result fed back into the next prompt is the same result the tool step
      // already showed. Counting it and naming the tool says more than printing
      // it twice does.
      if ((role === 'tool' || role === 'function') && m.name && toolsShown.has(m.name)) {
        seen.add(print)
        fed += 1
        continue
      }
      seen.add(print)
      const voice = voiceOf(m.role)
      add(s, {
        voice: voice === 'note' ? 'user' : voice,
        speaker: m.role ?? 'message',
        attribution: m.name ?? from,
        text: prose ?? undefined,
        data: prose ? undefined : (data ?? undefined),
        dataLabel: prose ? undefined : 'message',
        note: m.role
          ? undefined
          : 'This message names no role. It is shown in the order the prompt held it.',
      })
    }
    return { repeated, fed }
  }

  /* ---- the opening, from the run's own input ---- */

  if (root) {
    const list = messagesOf(root.input)
    if (list) {
      emitMessages(root, list, 'the run input')
    } else {
      const prose = asProse(root.input)
      if (prose) {
        seen.add(fingerprint('user', prose))
        add(root, {
          voice: 'user',
          speaker: 'user',
          attribution: 'the run input',
          text: prose,
          note: readFrom(root.input, "the run's input"),
        })
      } else {
        const data = asData(root.input)
        if (data) {
          add(root, {
            voice: 'user',
            speaker: 'run input',
            attribution: root.entity_name ?? undefined,
            data,
            dataLabel: 'input',
            note: "The run's input is not a message, so it is shown as it was recorded.",
          })
        }
      }
    }
  }

  /* ---- every step after it ---- */

  let speaker = root?.entity_name ?? root?.name ?? null

  for (const s of order) {
    if (root && s.span_id === root.span_id) continue

    if (AGENT_KINDS.has(s.kind)) {
      const to = s.entity_name ?? s.name
      // A handoff is a CHANGE of speaker. A nested run under the same name is
      // the same speaker doing more work, and announcing "x hands to x" would
      // be the interface inventing an event the tree never recorded.
      if (to !== speaker) {
        add(s, {
          voice: 'handoff',
          speaker: to,
          attribution: speaker ?? undefined,
          text: speaker ? `${speaker} hands to ${to}` : `${to} takes over`,
        })
        speaker = to
      }
      const prose = asProse(s.output)
      if (prose && !seen.has(fingerprint('assistant', prose))) {
        seen.add(fingerprint('assistant', prose))
        add(s, { voice: 'assistant', speaker: 'assistant', attribution: to, text: prose })
      }
      if (!contributed.has(s.span_id)) skipped.push({ name: s.name, kind: s.kind })
      continue
    }

    if (MODEL_KINDS.has(s.kind)) {
      const who = s.model ?? s.entity_name ?? speaker ?? 'the model'
      const list = messagesOf(s.input)
      let carried: { repeated: number; fed: number } | null = null
      if (list) {
        carried = emitMessages(s, list, who)
      } else {
        const prose = asProse(s.input)
        if (prose && !seen.has(fingerprint('user', prose))) {
          seen.add(fingerprint('user', prose))
          add(s, { voice: 'user', speaker: 'user', attribution: who, text: prose })
        }
      }

      const note: string[] = []
      if (carried?.repeated) {
        note.push(
          `The prompt repeated ${carried.repeated} message${carried.repeated === 1 ? '' : 's'} already above.`,
        )
      }
      if (carried?.fed) {
        note.push(
          `It fed back ${carried.fed} tool result${carried.fed === 1 ? '' : 's'} shown above.`,
        )
      }
      const stop = stopReason(s)
      if (stop?.atLimit) {
        note.push(`The completion stopped at the token limit (${stop.key}: ${stop.value}).`)
      }

      const calls = callsOf(s.output)
      const prose = asProse(s.output)
      const running = !s.end_time && !s.is_event

      if (prose) {
        seen.add(fingerprint('assistant', prose))
        const from = readFrom(s.output, 'this completion')
        if (from) note.unshift(from)
        add(s, {
          voice: 'assistant',
          speaker: 'assistant',
          attribution: who,
          text: prose,
          note: note.join(' ') || undefined,
          running,
          error: s.error ? { message: s.error.message, kind: s.error.kind } : undefined,
        })
      }

      if (calls.length) {
        // A call with no arguments is a sentence, not a payload. Drawing an
        // empty recess labelled "arguments" to hold the words "no arguments"
        // is a box around the absence of a thing.
        const args = calls.map((c) => argLine(c.args))
        const named = calls.map((c) => c.name).join(', ')
        const bare = args.every((a) => a == null)
        add(s, {
          voice: 'assistant',
          speaker: 'assistant',
          attribution: who,
          text: bare ? `Asks for ${named}, with no arguments` : `Asks for ${named}`,
          data: bare ? undefined : args.map((a) => a ?? 'no arguments').join('\n'),
          dataLabel: calls.length === 1 ? 'arguments' : 'arguments, in order',
          note: prose ? undefined : note.join(' ') || undefined,
          running,
          error: !prose && s.error ? { message: s.error.message, kind: s.error.kind } : undefined,
        })
      }

      if (!prose && !calls.length) {
        const data = asData(s.output)
        // The speaker is still the assistant: what is missing is what it said,
        // not who was speaking. Naming the absence as if it were a speaker
        // ("no completion") prints the same fact twice and invents a voice.
        add(s, {
          voice: data ? 'assistant' : 'note',
          speaker: 'assistant',
          attribution: who,
          data: data ?? undefined,
          dataLabel: data ? 'output' : undefined,
          text: running
            ? 'This step has not answered yet.'
            : data
              ? undefined
              : 'This step recorded no completion.',
          note: data
            ? `This completion is not a message, so it is shown as it was recorded. ${note.join(' ')}`.trim()
            : note.join(' ') || undefined,
          running,
          error: s.error ? { message: s.error.message, kind: s.error.kind } : undefined,
        })
      }
      continue
    }

    if (TOOL_KINDS.has(s.kind)) {
      const name = s.entity_name ?? s.name
      toolsShown.add(name)
      const running = !s.end_time && !s.is_event
      const result = asProse(s.output) ?? asData(s.output)
      const call = argLine(s.input)
      add(s, {
        voice: 'tool',
        speaker: 'tool',
        attribution: name,
        text: call ? undefined : 'Called with no arguments.',
        data: call ?? undefined,
        dataLabel: call ? 'call' : undefined,
        result: result ?? undefined,
        resultLabel: result ? 'result' : undefined,
        note: result
          ? undefined
          : running
            ? 'This call has not returned yet.'
            : 'This call recorded no result.',
        running,
        error: s.error ? { message: s.error.message, kind: s.error.kind } : undefined,
      })
      continue
    }

    if (s.status === 'error' && s.error) {
      // Not speech, but a failure changes what happens next, so it belongs in
      // the record of what happened.
      add(s, {
        voice: 'fault',
        speaker: 'fault',
        attribution: s.entity_name ?? s.name,
        error: { message: s.error.message, kind: s.error.kind },
      })
      continue
    }

    skipped.push({ name: s.name, kind: s.kind })
  }

  /* ---- the run's own answer ---- */

  if (root) {
    // The run's answer was recorded when the root ENDED, so it sits at that
    // moment on the clock in the margin. Taking the root's start would date the
    // last thing in the transcript as the first thing that happened.
    const rootEnd = root.end_time ? ms(root.end_time) - t0 : null
    const closing = rootEnd != null && !Number.isNaN(rootEnd) ? { offsetMs: rootEnd } : {}
    const prose = asProse(root.output)
    if (prose) {
      const print = fingerprint('assistant', prose)
      if (!seen.has(print)) {
        seen.add(print)
        add(root, {
          ...closing,
          voice: 'assistant',
          speaker: 'assistant',
          attribution: root.entity_name ?? root.name,
          text: prose,
          note: [readFrom(root.output, "the run's output"), "The run's own output, recorded on its root step."]
            .filter(Boolean)
            .join(' '),
        })
      }
    } else {
      const data = asData(root.output)
      if (data) {
        add(root, {
          ...closing,
          voice: 'assistant',
          speaker: 'run output',
          attribution: root.entity_name ?? root.name,
          data,
          dataLabel: 'output',
          note: "The run's output is not a message, so it is shown as it was recorded.",
        })
      }
    }
    if (root.error) {
      add(root, {
        ...closing,
        voice: 'fault',
        speaker: 'fault',
        attribution: root.entity_name ?? root.name,
        error: { message: root.error.message, kind: root.error.kind },
        note: root.output == null ? 'The run ended here and recorded no answer.' : undefined,
      })
    }
  }

  collapseRepeatedCall(turns)

  // A root that said nothing is still a step the reader is owed an account of,
  // and it never reaches the loop above, which walks everything under it.
  if (root && !contributed.has(root.span_id)) {
    skipped.unshift({ name: root.name, kind: root.kind })
  }

  return {
    turns,
    used: contributed.size,
    total,
    skipped,
    empty: turns.length === 0,
  }
}
