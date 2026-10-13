/**
 * Streamed model calls (SDK contract 3.2).
 *
 * A streamed response spreads the facts `recordResponse` reads across its
 * chunks: the id and model in the first, the text in the middle, the finish
 * reason and the usage at the end. This collects them, one chunk at a time, and
 * builds the response the provider would have sent without streaming, so the
 * span records exactly what it would have recorded for the same call made the
 * other way.
 *
 * Nothing here raises on a chunk it does not understand, and nothing grows
 * without bound: text and tool-call arguments share one budget, and tool calls
 * are capped by count.
 */

type Obj = Record<string, unknown>

/** Code points of text and tool-call arguments kept, together. */
export const STREAM_TEXT_BUDGET = 32_768
/** Tool calls kept. Later ones are ignored. */
export const STREAM_MAX_TOOL_CALLS = 64

const ANTHROPIC_EVENTS = new Set(['message_start', 'content_block_start', 'content_block_delta', 'message_delta'])

function obj(v: unknown): Obj | undefined {
  return v !== null && typeof v === 'object' && !Array.isArray(v) ? (v as Obj) : undefined
}

function str(v: unknown): string | undefined {
  return typeof v === 'string' && v.length > 0 ? v : undefined
}

function index(v: unknown, fallback: number): number {
  return typeof v === 'number' && Number.isInteger(v) && v >= 0 ? v : fallback
}

interface ToolCall {
  id?: string
  name?: string
  arguments: string
}

/** What one stream has said so far. */
export class StreamAccumulator {
  shape: 'openai' | 'anthropic' | undefined
  truncated = false
  #id: string | undefined
  #model: string | undefined
  #tier: string | undefined
  #text = ''
  #budget = STREAM_TEXT_BUDGET
  #tools = new Map<number, ToolCall>()
  #finish = new Map<number, string>()
  #stop: string | undefined
  #usage: Obj | undefined

  /** Fold one chunk in. Answers whether it carried text or a tool-call delta. */
  add(chunk: unknown): boolean {
    const c = obj(chunk)
    if (c === undefined) return false
    if (Array.isArray(c.choices)) return this.#openai(c)
    if (typeof c.type === 'string' && ANTHROPIC_EVENTS.has(c.type)) return this.#anthropic(c)
    return false
  }

  #openai(c: Obj): boolean {
    this.shape ??= 'openai'
    this.#id ??= str(c.id)
    this.#model ??= str(c.model)
    this.#tier ??= str(c.service_tier)
    const usage = obj(c.usage)
    if (usage !== undefined) this.#usage = usage
    let content = false
    const choices = c.choices as unknown[]
    choices.forEach((raw, position) => {
      const choice = obj(raw)
      if (choice === undefined) return
      const at = index(choice.index, position)
      const reason = str(choice.finish_reason)
      if (reason !== undefined) this.#finish.set(at, reason)
      if (at !== 0) return
      const delta = obj(choice.delta)
      if (delta === undefined) return
      const text = str(delta.content)
      if (text !== undefined) {
        content = true
        this.#text += this.#take(text)
      }
      if (Array.isArray(delta.tool_calls)) {
        delta.tool_calls.forEach((rawCall, i) => {
          const call = obj(rawCall)
          if (call === undefined) return
          content = true
          const tool = this.#tool(index(call.index, i))
          if (tool === undefined) return
          const fn = obj(call.function)
          tool.id ??= str(call.id)
          tool.name ??= str(fn?.name)
          const args = str(fn?.arguments)
          if (args !== undefined) tool.arguments += this.#take(args)
        })
      }
    })
    return content
  }

  #anthropic(c: Obj): boolean {
    this.shape ??= 'anthropic'
    switch (c.type) {
      case 'message_start': {
        const message = obj(c.message)
        this.#id ??= str(message?.id)
        this.#model ??= str(message?.model)
        const usage = obj(message?.usage)
        if (usage !== undefined) this.#usage = { ...usage }
        return false
      }
      case 'content_block_start': {
        const block = obj(c.content_block)
        if (block?.type === 'tool_use') {
          const tool = this.#tool(index(c.index, this.#tools.size))
          if (tool !== undefined) {
            tool.id ??= str(block.id)
            tool.name ??= str(block.name)
          }
          return true
        }
        const text = str(block?.text)
        if (text === undefined) return false
        this.#text += this.#take(text)
        return true
      }
      case 'content_block_delta': {
        const delta = obj(c.delta)
        if (delta?.type === 'text_delta') {
          const text = str(delta.text)
          if (text === undefined) return false
          this.#text += this.#take(text)
          return true
        }
        if (delta?.type === 'input_json_delta') {
          const args = str(delta.partial_json)
          if (args === undefined) return false
          const tool = this.#tools.get(index(c.index, -1))
          if (tool !== undefined) tool.arguments += this.#take(args)
          return true
        }
        return false
      }
      case 'message_delta': {
        this.#stop = str(obj(c.delta)?.stop_reason) ?? this.#stop
        const usage = obj(c.usage)
        if (usage !== undefined) {
          const merged: Obj = { ...this.#usage }
          for (const [k, v] of Object.entries(usage)) if (v !== null && v !== undefined) merged[k] = v
          this.#usage = merged
        }
        return false
      }
    }
    return false
  }

  /** The tool call at `at`, made on first sight, or undefined past the cap. */
  #tool(at: number): ToolCall | undefined {
    const known = this.#tools.get(at)
    if (known !== undefined) return known
    if (this.#tools.size >= STREAM_MAX_TOOL_CALLS) {
      this.truncated = true
      return undefined
    }
    const tool: ToolCall = { arguments: '' }
    this.#tools.set(at, tool)
    return tool
  }

  /** As much of `s` as the budget still allows, counted in code points. */
  #take(s: string): string {
    if (this.#budget <= 0) {
      this.truncated = true
      return ''
    }
    let n = 0
    let cut = 0
    for (const ch of s) {
      if (n === this.#budget) {
        this.truncated = true
        this.#budget = 0
        return s.slice(0, cut)
      }
      n++
      cut += ch.length
    }
    this.#budget -= n
    return s
  }

  /** The response the provider would have sent, or undefined when no chunk was recognised. */
  response(): Obj | undefined {
    const put = (o: Obj, k: string, v: unknown) => {
      if (v !== undefined) o[k] = v
    }
    if (this.shape === 'openai') {
      const out: Obj = { object: 'chat.completion' }
      put(out, 'id', this.#id)
      put(out, 'model', this.#model)
      put(out, 'service_tier', this.#tier)
      out.choices = [...this.#finish.entries()]
        .sort(([a], [b]) => a - b)
        .map(([i, reason]) => ({ index: i, finish_reason: reason }))
      put(out, 'usage', this.#usage)
      return out
    }
    if (this.shape === 'anthropic') {
      const out: Obj = { type: 'message' }
      put(out, 'id', this.#id)
      put(out, 'model', this.#model)
      put(out, 'stop_reason', this.#stop)
      put(out, 'usage', this.#usage)
      return out
    }
    return undefined
  }

  /** The text, or the text and the tool calls, or undefined when neither arrived. */
  output(): unknown {
    if (this.#tools.size === 0) return this.#text.length > 0 ? this.#text : undefined
    const calls = [...this.#tools.entries()]
      .sort(([a], [b]) => a - b)
      .map(([, t]) => {
        const call: Obj = {}
        if (t.id !== undefined) call.id = t.id
        if (t.name !== undefined) call.name = t.name
        call.arguments = t.arguments
        return call
      })
    return { content: this.#text, tool_calls: calls }
  }
}
