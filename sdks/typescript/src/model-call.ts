/**
 * Model calls: what an OpenAI- or Anthropic-shaped response says about the
 * call that produced it, mapped onto mcpd's usage record (SDK contract 3.1).
 *
 * Nothing here imports a provider SDK. The response is read as a plain object,
 * so the provider's own response object, a parsed JSON body and a hand-built
 * test fixture all work, and a field that is missing or of the wrong type is
 * left absent rather than raised on.
 */

import type { InputTokenDetails, OutputTokenDetails, TokenUsage } from './generated.js'

type Obj = Record<string, unknown>

function obj(v: unknown): Obj | undefined {
  return v !== null && typeof v === 'object' && !Array.isArray(v) ? (v as Obj) : undefined
}

function int(v: unknown): number | undefined {
  return typeof v === 'number' && Number.isFinite(v) && v >= 0 ? Math.floor(v) : undefined
}

function str(v: unknown): string | undefined {
  return typeof v === 'string' && v.length > 0 ? v : undefined
}

/** Drop the keys whose value is undefined, and the object itself when empty. */
function compact<T extends object>(o: T): T | undefined {
  const out = Object.fromEntries(Object.entries(o).filter(([, v]) => v !== undefined)) as T
  return Object.keys(out).length > 0 ? out : undefined
}

/**
 * Token usage from an OpenAI chat-completions or responses-API response.
 *
 * The prompt total is gross, so the plain text share is the total less the
 * cached and audio tokens.
 */
export function usageFromOpenAI(response: unknown): TokenUsage | undefined {
  const usage = obj(obj(response)?.usage)
  if (usage === undefined) return undefined
  const input = int(usage.prompt_tokens) ?? int(usage.input_tokens)
  const output = int(usage.completion_tokens) ?? int(usage.output_tokens)
  const inDetail = obj(usage.prompt_tokens_details) ?? obj(usage.input_tokens_details)
  const outDetail = obj(usage.completion_tokens_details) ?? obj(usage.output_tokens_details)
  const cached = int(inDetail?.cached_tokens)
  const audioIn = int(inDetail?.audio_tokens)
  const inputDetails = compact<InputTokenDetails>({
    text:
      input !== undefined && (cached !== undefined || audioIn !== undefined)
        ? Math.max(0, input - (cached ?? 0) - (audioIn ?? 0))
        : undefined,
    cacheRead: cached,
    audio: audioIn,
  })
  const outputDetails = compact<OutputTokenDetails>({
    reasoning: int(outDetail?.reasoning_tokens),
    audio: int(outDetail?.audio_tokens),
  })
  return compact<TokenUsage>({
    inputTokens: input,
    outputTokens: output,
    totalTokens: int(usage.total_tokens) ?? (input !== undefined && output !== undefined ? input + output : undefined),
    inputDetails,
    outputDetails,
  })
}

/**
 * Token usage from an Anthropic messages response.
 *
 * `input_tokens` there is net of the cache, so it is the text share, and the
 * prompt's size is the sum of it and both cache meters.
 */
export function usageFromAnthropic(response: unknown): TokenUsage | undefined {
  const usage = obj(obj(response)?.usage)
  if (usage === undefined) return undefined
  const text = int(usage.input_tokens)
  const read = int(usage.cache_read_input_tokens)
  const write = int(usage.cache_creation_input_tokens)
  const split = obj(usage.cache_creation)
  const output = int(usage.output_tokens)
  const input =
    text === undefined && read === undefined && write === undefined
      ? undefined
      : (text ?? 0) + (read ?? 0) + (write ?? 0)
  return compact<TokenUsage>({
    inputTokens: input,
    outputTokens: output,
    totalTokens: input !== undefined && output !== undefined ? input + output : undefined,
    inputDetails: compact<InputTokenDetails>({
      text,
      cacheRead: read,
      cacheWrite: write,
      cacheWrite5m: int(split?.ephemeral_5m_input_tokens),
      cacheWrite1h: int(split?.ephemeral_1h_input_tokens),
    }),
  })
}

/** Which provider shaped this response, or undefined when neither did. */
export function responseShape(response: unknown): 'openai' | 'anthropic' | undefined {
  const r = obj(response)
  if (r === undefined) return undefined
  const usage = obj(r.usage)
  if (r.type === 'message' || (usage !== undefined && 'cache_read_input_tokens' in usage)) return 'anthropic'
  if (usage !== undefined && ('prompt_tokens' in usage || 'input_tokens_details' in usage)) return 'openai'
  if (Array.isArray(r.choices) || r.object === 'response') return 'openai'
  if (usage !== undefined && 'input_tokens' in usage) return 'anthropic'
  return undefined
}

/** What `recordResponse` sets on a span. */
export interface ResponseFacts {
  model?: string
  provider?: string
  usage?: TokenUsage
  attributes: Record<string, unknown>
}

/**
 * Everything a response says about its call: the model, the provider, the
 * usage, and the id, finish reasons and service tier as `gen_ai.response.*`
 * attributes. Never the text: that stays the caller's to record or not.
 */
export function responseFacts(response: unknown, provider?: string): ResponseFacts {
  const r = obj(response) ?? {}
  const shape = responseShape(response)
  const attributes: Record<string, unknown> = {}
  const id = str(r.id)
  if (id !== undefined) attributes['gen_ai.response.id'] = id
  const reasons: string[] = []
  if (Array.isArray(r.choices)) {
    for (const c of r.choices) {
      const reason = str(obj(c)?.finish_reason)
      if (reason !== undefined) reasons.push(reason)
    }
  }
  const stop = str(r.stop_reason)
  if (stop !== undefined) reasons.push(stop)
  if (reasons.length === 0 && r.object === 'response') {
    const status = str(r.status)
    if (status !== undefined) reasons.push(status)
  }
  if (reasons.length > 0) attributes['gen_ai.response.finish_reasons'] = reasons
  const tier = str(r.service_tier) ?? str(obj(r.usage)?.service_tier)
  if (tier !== undefined) attributes['gen_ai.response.service_tier'] = tier
  const usage =
    shape === 'anthropic' ? usageFromAnthropic(response) : shape === 'openai' ? usageFromOpenAI(response) : undefined
  return {
    model: str(r.model),
    provider: provider ?? shape,
    usage,
    attributes,
  }
}
