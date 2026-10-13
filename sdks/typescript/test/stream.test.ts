import { describe, expect, it } from 'vitest'
import { STREAM_TEXT_BUDGET } from '../src/index.js'
import { TestClient } from '../src/testing.js'

const openaiChunks = [
  { id: 'chatcmpl-1', object: 'chat.completion.chunk', model: 'gpt-4o', choices: [{ index: 0, delta: { role: 'assistant' } }] },
  { id: 'chatcmpl-1', object: 'chat.completion.chunk', model: 'gpt-4o', choices: [{ index: 0, delta: { content: 'Hel' } }] },
  { id: 'chatcmpl-1', object: 'chat.completion.chunk', model: 'gpt-4o', choices: [{ index: 0, delta: { content: 'lo' } }] },
  { id: 'chatcmpl-1', object: 'chat.completion.chunk', model: 'gpt-4o', choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] },
  {
    id: 'chatcmpl-1',
    object: 'chat.completion.chunk',
    model: 'gpt-4o',
    service_tier: 'flex',
    choices: [],
    usage: { prompt_tokens: 10, completion_tokens: 2, total_tokens: 12, prompt_tokens_details: { cached_tokens: 4 } },
  },
]

const anthropicEvents = [
  {
    type: 'message_start',
    message: { id: 'msg_1', type: 'message', model: 'claude-sonnet-4-5', usage: { input_tokens: 20, cache_read_input_tokens: 80, output_tokens: 1 } },
  },
  { type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } },
  { type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text: 'Checking' } },
  { type: 'content_block_stop', index: 0 },
  { type: 'content_block_start', index: 1, content_block: { type: 'tool_use', id: 'toolu_1', name: 'lookup', input: {} } },
  { type: 'content_block_delta', index: 1, delta: { type: 'input_json_delta', partial_json: '{"id":' } },
  { type: 'content_block_delta', index: 1, delta: { type: 'input_json_delta', partial_json: '"A-1"}' } },
  { type: 'message_delta', delta: { stop_reason: 'tool_use' }, usage: { output_tokens: 15 } },
  { type: 'message_stop' },
]

async function* from<T>(items: T[], failAfter?: number): AsyncGenerator<T> {
  let i = 0
  for (const item of items) {
    if (failAfter !== undefined && i === failAfter) throw new TypeError('connection reset')
    i++
    yield item
  }
}

async function drain<T>(stream: AsyncIterable<T>): Promise<T[]> {
  const out: T[] = []
  for await (const c of stream) out.push(c)
  return out
}

describe('recordStream', () => {
  it('records an OpenAI stream as recordResponse would, and passes every chunk through', async () => {
    const mcpd = new TestClient()
    const span = mcpd.trace('call', { kind: 'model_generation' })
    const seen = await drain(span.recordStream(from([...openaiChunks, 'not a chunk'])))
    expect(seen).toEqual([...openaiChunks, 'not a chunk'])
    expect(seen[1]).toBe(openaiChunks[1])

    const s = await mcpd.span('call')
    expect(s.model).toBe('gpt-4o')
    expect(s.provider).toBe('openai')
    expect(s.output).toBe('Hello')
    expect(s.usage).toEqual({
      inputTokens: 10,
      outputTokens: 2,
      totalTokens: 12,
      inputDetails: { text: 6, cacheRead: 4 },
    })
    const attrs = s.attributes as Record<string, unknown>
    expect(attrs['gen_ai.response.id']).toBe('chatcmpl-1')
    expect(attrs['gen_ai.response.finish_reasons']).toEqual(['stop'])
    expect(attrs['gen_ai.response.service_tier']).toBe('flex')
    expect(typeof attrs['mcpd.time_to_first_token_ms']).toBe('number')
    expect(attrs['mcpd.stream.truncated']).toBeUndefined()
    expect(s.status).toBe('ok')
  })

  it('joins Anthropic tool-call deltas and merges message_delta usage', async () => {
    const mcpd = new TestClient()
    const span = mcpd.trace('call', { kind: 'model_generation' })
    await drain(span.recordStream(from(anthropicEvents)))
    const s = await mcpd.span('call')
    expect(s.model).toBe('claude-sonnet-4-5')
    expect(s.provider).toBe('anthropic')
    expect(s.output).toEqual({
      content: 'Checking',
      tool_calls: [{ id: 'toolu_1', name: 'lookup', arguments: '{"id":"A-1"}' }],
    })
    expect(s.usage?.inputTokens).toBe(100)
    expect(s.usage?.outputTokens).toBe(15)
    expect((s.attributes as Record<string, unknown>)['gen_ai.response.finish_reasons']).toEqual(['tool_use'])
  })

  it('joins OpenAI tool-call deltas by index', async () => {
    const mcpd = new TestClient()
    const span = mcpd.trace('call')
    const call = (d: unknown) => ({ choices: [{ index: 0, delta: { tool_calls: [d] } }] })
    await drain(
      span.recordStream(
        from([
          call({ index: 0, id: 'call_1', function: { name: 'lookup', arguments: '{"a"' } }),
          call({ index: 0, function: { arguments: ':1}' } }),
          { choices: [{ index: 0, delta: {}, finish_reason: 'tool_calls' }] },
        ]),
      ),
    )
    const s = await mcpd.span('call')
    expect(s.output).toEqual({ content: '', tool_calls: [{ id: 'call_1', name: 'lookup', arguments: '{"a":1}' }] })
  })

  it('marks the first token once, at the first content chunk', async () => {
    const mcpd = new TestClient()
    const span = mcpd.trace('call')
    const stream = span.recordStream(from(openaiChunks))
    await stream.next() // role only: no content yet
    expect(((await mcpd.spans()).length)).toBe(0)
    await drain(stream)
    const attrs = (await mcpd.span('call')).attributes as Record<string, unknown>
    expect(Object.keys(attrs).filter((k) => k === 'mcpd.time_to_first_token_ms')).toHaveLength(1)
  })

  it('ends the span with the error when the stream raises, and rethrows it unchanged', async () => {
    const mcpd = new TestClient()
    const span = mcpd.trace('call')
    await expect(drain(span.recordStream(from(openaiChunks, 3)))).rejects.toThrow(TypeError)
    const s = await mcpd.span('call')
    expect(s.status).toBe('error')
    expect(s.error?.message).toBe('connection reset')
    expect(s.output).toBe('Hello')
  })

  it('ends the span when the caller stops reading early', async () => {
    const mcpd = new TestClient()
    const span = mcpd.trace('call')
    for await (const chunk of span.recordStream(from(openaiChunks))) {
      const c = chunk as { choices: Array<{ delta: { content?: string } }> }
      if (c.choices[0]?.delta.content === 'Hel') break
    }
    const s = await mcpd.span('call')
    expect(s.output).toBe('Hel')
    expect(s.status).toBe('ok')
  })

  it('cuts text past the budget and says so', async () => {
    const mcpd = new TestClient()
    const span = mcpd.trace('call')
    const big = '😀'.repeat(STREAM_TEXT_BUDGET - 1) + 'ab'
    await drain(span.recordStream(from([{ choices: [{ index: 0, delta: { content: big } }] }])))
    const s = await mcpd.span('call')
    expect([...(s.output as string)]).toHaveLength(STREAM_TEXT_BUDGET)
    expect((s.output as string).endsWith('😀a')).toBe(true)
    expect((s.attributes as Record<string, unknown>)['mcpd.stream.truncated']).toBe(true)
  })

  it('keeps the output in the process with payload capture off', async () => {
    const mcpd = new TestClient({ capturePayloads: false })
    const span = mcpd.trace('call')
    await drain(span.recordStream(from(openaiChunks)))
    const s = await mcpd.span('call')
    expect(s.output).toBeUndefined()
    expect(s.model).toBe('gpt-4o')
  })
})
