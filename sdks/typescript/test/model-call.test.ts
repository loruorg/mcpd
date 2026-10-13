import { describe, expect, it } from 'vitest'
import { Mcpd, usageFromAnthropic, usageFromOpenAI } from '../src/index.js'
import { fakeFetch } from './helpers.js'

function client(fetchImpl = fakeFetch().fetch, options = {}) {
  return new Mcpd({
    endpoint: 'http://daemon.test',
    project: 'test',
    fetch: fetchImpl,
    flushIntervalMs: 10_000,
    flushOnExit: false,
    onError: () => undefined,
    ...options,
  })
}

const openaiChat = {
  id: 'chatcmpl-1',
  object: 'chat.completion',
  model: 'gpt-4o-2024-08-06',
  service_tier: 'flex',
  choices: [{ index: 0, finish_reason: 'stop', message: { role: 'assistant', content: 'hi' } }],
  usage: {
    prompt_tokens: 1000,
    completion_tokens: 200,
    total_tokens: 1200,
    prompt_tokens_details: { cached_tokens: 600, audio_tokens: 0 },
    completion_tokens_details: { reasoning_tokens: 50 },
  },
}

const anthropicMessage = {
  id: 'msg_1',
  type: 'message',
  model: 'claude-sonnet-4-5',
  stop_reason: 'end_turn',
  usage: {
    input_tokens: 100,
    cache_read_input_tokens: 900,
    cache_creation_input_tokens: 50,
    cache_creation: { ephemeral_5m_input_tokens: 20, ephemeral_1h_input_tokens: 30 },
    output_tokens: 40,
    service_tier: 'batch',
  },
}

describe('model calls', () => {
  it('maps OpenAI usage with the gross prompt split into text and cache', () => {
    expect(usageFromOpenAI(openaiChat)).toEqual({
      inputTokens: 1000,
      outputTokens: 200,
      totalTokens: 1200,
      inputDetails: { text: 400, cacheRead: 600, audio: 0 },
      outputDetails: { reasoning: 50 },
    })
    const responses = {
      object: 'response',
      usage: { input_tokens: 10, output_tokens: 5, input_tokens_details: { cached_tokens: 4 } },
    }
    expect(usageFromOpenAI(responses)).toEqual({
      inputTokens: 10,
      outputTokens: 5,
      totalTokens: 15,
      inputDetails: { text: 6, cacheRead: 4 },
    })
  })

  it('maps Anthropic usage, whose input is net of the cache', () => {
    expect(usageFromAnthropic(anthropicMessage)).toEqual({
      inputTokens: 1050,
      outputTokens: 40,
      totalTokens: 1090,
      inputDetails: { text: 100, cacheRead: 900, cacheWrite: 50, cacheWrite5m: 20, cacheWrite1h: 30 },
    })
  })

  it('reads nothing from a shape it does not know, and never throws', () => {
    expect(usageFromOpenAI(null)).toBeUndefined()
    expect(usageFromAnthropic({ usage: { input_tokens: 'many' } })).toBeUndefined()
    const mcpd = client()
    const span = mcpd.trace('run')
    expect(() => span.recordResponse(42).recordResponse({ usage: 'x' })).not.toThrow()
  })

  it('records the model, usage, id, finish reasons and tier, and never the text', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)
    const a = mcpd.trace('a', { kind: 'model_generation' })
    a.recordResponse(openaiChat).end()
    const b = mcpd.trace('b', { kind: 'model_generation' })
    b.recordResponse(anthropicMessage, 'aws.bedrock').end()
    await mcpd.flush()
    const [ra, rb] = ['a', 'b'].map((n) => fake.spans().filter((s) => s['name'] === n).pop()!)
    expect(ra['model']).toBe('gpt-4o-2024-08-06')
    expect(ra['provider']).toBe('openai')
    expect(ra['output']).toBeUndefined()
    expect(ra['attributes']).toEqual({
      'gen_ai.response.id': 'chatcmpl-1',
      'gen_ai.response.finish_reasons': ['stop'],
      'gen_ai.response.service_tier': 'flex',
    })
    expect((ra['usage'] as Record<string, unknown>)['input_tokens']).toBe(1000)
    expect(rb['provider']).toBe('aws.bedrock')
    expect((rb['attributes'] as Record<string, unknown>)['gen_ai.response.service_tier']).toBe('batch')
    await mcpd.shutdown()
  })

  it('records the first token once, and a guardrail outcome', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch)
    const s = mcpd.trace('stream')
    s.markFirstToken()
    await new Promise((r) => setTimeout(r, 5))
    s.markFirstToken()
    s.setGuardrail(true, 'pii in output').end()
    await mcpd.flush()
    const rec = fake.spans().filter((x) => x['name'] === 'stream').pop()!
    const attrs = rec['attributes'] as Record<string, unknown>
    expect(typeof attrs['mcpd.time_to_first_token_ms']).toBe('number')
    expect(attrs['mcpd.time_to_first_token_ms'] as number).toBeLessThan(5)
    expect(attrs['mcpd.guardrail.tripped']).toBe(true)
    expect(attrs['mcpd.guardrail.reason']).toBe('pii in output')
    await mcpd.shutdown()
  })
})

describe('redact', () => {
  it('rewrites payloads before they are queued, and drops what it throws on', async () => {
    const fake = fakeFetch()
    const mcpd = client(fake.fetch, {
      redact: (v: unknown) => {
        if (typeof v === 'string' && v.includes('boom')) throw new Error('redactor broke')
        return JSON.parse(JSON.stringify(v).replace(/\d{4}-\d{4}/g, '****'))
      },
    })
    const span = mcpd.trace('run', { input: { card: '1234-5678' } })
    span.setOutput('boom').end()
    mcpd.log('info', 'paid', { card: '1234-5678' })
    await mcpd.flush()
    const rec = fake.spans().filter((x) => x['name'] === 'run').pop()!
    expect(rec['input']).toEqual({ card: '****' })
    expect(rec['output']).toBeUndefined()
    const logs = fake.calls.flatMap((c) => (c.body as { logs?: Array<Record<string, unknown>> })?.logs ?? [])
    expect(logs[0]?.['fields']).toEqual({ card: '****' })
    expect(mcpd.stats().redactionFailures).toBe(1)
    // The caller's own value is untouched.
    expect(span.capturesPayloads).toBe(true)
    await mcpd.shutdown()
  })
})
