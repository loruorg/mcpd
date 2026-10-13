import { describe, expect, it } from 'vitest'
import { Mcpd, PromptRenderError, renderPrompt } from '../src/index.js'
import type { FetchLike, Prompt, TraceQuery } from '../src/index.js'
import { deadFetch, fakeFetch } from './helpers.js'

function client(fetchImpl: FetchLike, options: Record<string, unknown> = {}) {
  return new Mcpd({
    endpoint: 'http://daemon.test',
    project: 'test',
    fetch: fetchImpl,
    flushIntervalMs: 10_000,
    flushOnExit: false,
    onError: () => {},
    ...options,
  })
}

/** A stored prompt as the daemon writes it, in its own spelling. */
function wirePrompt(extra: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    project: 'test',
    name: 'greet',
    version: 3,
    kind: 'text',
    template: 'Hello {{name}}',
    variables: ['name'],
    labels: ['production'],
    created_at: '2026-01-01T00:00:00Z',
    ...extra,
  }
}

function prompt(extra: Partial<Prompt> = {}): Prompt {
  return {
    project: 'test',
    name: 'greet',
    version: 1,
    kind: 'text',
    template: '',
    variables: [],
    labels: [],
    createdAt: '2026-01-01T00:00:00Z',
    ...extra,
  }
}

const BASE = 'http://daemon.test/v1/api/prompts'

describe('getPrompt', () => {
  it('asks for the production label, and on a 404 for the latest version', async () => {
    const fake = fakeFetch([{ status: 404, body: { error: 'not found' } }, { status: 200, body: wirePrompt() }])
    const mcpd = client(fake.fetch)
    const got = await mcpd.getPrompt('greet')

    expect(fake.calls.map((c) => c.url)).toEqual([
      `${BASE}/greet?project=test&label=production`,
      `${BASE}/greet?project=test`,
    ])
    expect(got.version).toBe(3)
    expect(got.createdAt).toBe('2026-01-01T00:00:00Z')
    await mcpd.shutdown()
  })

  it('sends a label or a version as asked, and encodes the name', async () => {
    const fake = fakeFetch([{ status: 200, body: wirePrompt() }])
    const mcpd = client(fake.fetch)
    await mcpd.getPrompt('team/greet', { label: 'staging' })
    await mcpd.getPrompt('greet', { version: 2 })

    expect(fake.calls.map((c) => c.url)).toEqual([
      `${BASE}/team%2Fgreet?project=test&label=staging`,
      `${BASE}/greet?project=test&version=2`,
    ])
    await mcpd.shutdown()
  })

  it('refuses a label and a version together before the wire', async () => {
    const fake = fakeFetch([{ status: 200, body: wirePrompt() }])
    const mcpd = client(fake.fetch)
    await expect(mcpd.getPrompt('greet', { label: 'production', version: 1 })).rejects.toThrow(
      /not both/,
    )
    expect(fake.calls).toHaveLength(0)
    await mcpd.shutdown()
  })

  it('serves a fresh copy with no request', async () => {
    const fake = fakeFetch([{ status: 200, body: wirePrompt() }])
    const mcpd = client(fake.fetch)
    const first = await mcpd.getPrompt('greet', { label: 'production' })
    const second = await mcpd.getPrompt('greet', { label: 'production' })

    expect(fake.calls).toHaveLength(1)
    expect(second).toEqual(first)
    await mcpd.shutdown()
  })

  it('keys the cache by selector', async () => {
    const fake = fakeFetch([{ status: 200, body: wirePrompt() }])
    const mcpd = client(fake.fetch)
    await mcpd.getPrompt('greet', { label: 'production' })
    await mcpd.getPrompt('greet', { label: 'staging' })
    await mcpd.getPrompt('greet', { version: 3 })
    expect(fake.calls).toHaveLength(3)
    await mcpd.shutdown()
  })

  it('serves a stale copy when the refetch fails, and reports it once', async () => {
    const fake = fakeFetch([{ status: 200, body: wirePrompt() }, { status: 500, body: { error: 'down' } }])
    const errors: string[] = []
    const mcpd = client(fake.fetch, { onError: (message: string) => errors.push(message) })
    const first = await mcpd.getPrompt('greet', { cacheTtlMs: 0 })
    const stale = await mcpd.getPrompt('greet', { cacheTtlMs: 0 })

    expect(fake.calls).toHaveLength(2)
    expect(stale).toEqual(first)
    expect(errors).toHaveLength(1)
    expect(errors[0]).toMatch(/greet/)
    await mcpd.shutdown()
  })

  it('serves a stale copy on a 404 as well', async () => {
    const fake = fakeFetch([{ status: 200, body: wirePrompt() }, { status: 404, body: {} }])
    const mcpd = client(fake.fetch)
    await mcpd.getPrompt('greet', { label: 'production', cacheTtlMs: 0 })
    const stale = await mcpd.getPrompt('greet', { label: 'production', cacheTtlMs: 0 })
    expect(stale.version).toBe(3)
    await mcpd.shutdown()
  })

  it('returns a text fallback when nothing was fetched, and never caches it', async () => {
    const fake = fakeFetch([{ status: 500, body: {} }, { status: 200, body: wirePrompt() }])
    const mcpd = client(fake.fetch)
    const fallback = await mcpd.getPrompt('greet', {
      label: 'production',
      fallback: 'Hi {{ name }}, about {{topic}}. Bye {{name}}.',
    })

    expect(fallback).toEqual({
      project: 'test',
      name: 'greet',
      version: 0,
      kind: 'text',
      template: 'Hi {{ name }}, about {{topic}}. Bye {{name}}.',
      variables: ['name', 'topic'],
      labels: [],
      createdAt: '',
    })
    const next = await mcpd.getPrompt('greet', { label: 'production', fallback: 'x' })
    expect(next.version).toBe(3)
    expect(fake.calls).toHaveLength(2)
    await mcpd.shutdown()
  })

  it('returns a chat fallback from an unreachable daemon', async () => {
    const mcpd = client(deadFetch)
    const messages = [
      { role: 'system', content: 'You help {{customer}}.' },
      { role: 'user', content: '{{question}}' },
    ]
    const got = await mcpd.getPrompt('support', { fallback: messages })

    expect(got.kind).toBe('chat')
    expect(got.version).toBe(0)
    expect(got.messages).toEqual(messages)
    expect(got.variables).toEqual(['customer', 'question'])
    await mcpd.shutdown()
  })

  it('throws the fetch error with no copy and no fallback', async () => {
    const fake = fakeFetch([{ status: 500, body: {} }])
    const mcpd = client(fake.fetch)
    await expect(mcpd.getPrompt('greet', { label: 'production' })).rejects.toMatchObject({
      name: 'McpdError',
      status: 500,
    })
    await mcpd.shutdown()
  })

  it('still fetches when the client is disabled', async () => {
    const fake = fakeFetch([{ status: 200, body: wirePrompt() }])
    const mcpd = client(fake.fetch, { enabled: false })
    const got = await mcpd.getPrompt('greet', { label: 'production' })
    expect(got.version).toBe(3)
    expect(fake.calls).toHaveLength(1)
    await mcpd.shutdown()
  })

  it('keeps at most 256 prompts and evicts the oldest', async () => {
    const fake = fakeFetch([{ status: 200, body: wirePrompt() }])
    const mcpd = client(fake.fetch)
    for (let i = 0; i < 257; i++) await mcpd.getPrompt(`p${i}`, { label: 'production' })
    await mcpd.getPrompt('p256', { label: 'production' })
    expect(fake.calls).toHaveLength(257)
    await mcpd.getPrompt('p0', { label: 'production' })
    expect(fake.calls).toHaveLength(258)
    await mcpd.shutdown()
  })

  it('leaves the keys inside messages and config as the user wrote them', async () => {
    const fake = fakeFetch([
      {
        status: 200,
        body: wirePrompt({
          kind: 'chat',
          template: undefined,
          messages: [{ role: 'tool', content: '{{x}}', tool_call_id: 't1' }],
          config: { max_tokens: 100 },
        }),
      },
    ])
    const mcpd = client(fake.fetch)
    const got = await mcpd.getPrompt('greet', { label: 'production' })
    expect(got.messages).toEqual([{ role: 'tool', content: '{{x}}', tool_call_id: 't1' }])
    expect(got.config).toEqual({ max_tokens: 100 })
    await mcpd.shutdown()
  })
})

describe('createPrompt and setPromptLabel', () => {
  it('posts the request to the client project and keeps message keys', async () => {
    const fake = fakeFetch([{ status: 200, body: wirePrompt({ version: 1 }) }])
    const mcpd = client(fake.fetch)
    const stored = await mcpd.createPrompt({
      name: 'greet',
      kind: 'chat',
      messages: [{ role: 'user', content: 'Hi {{name}}', cacheControl: 'ephemeral' }],
      config: { maxTokens: 50 },
      note: 'first',
      labels: ['production'],
    })

    expect(stored.version).toBe(1)
    expect(fake.calls[0]?.method).toBe('POST')
    expect(fake.calls[0]?.url).toBe(`${BASE}?project=test`)
    expect(fake.calls[0]?.body).toEqual({
      name: 'greet',
      kind: 'chat',
      messages: [{ role: 'user', content: 'Hi {{name}}', cacheControl: 'ephemeral' }],
      config: { maxTokens: 50 },
      note: 'first',
      labels: ['production'],
    })
    await mcpd.shutdown()
  })

  it('puts the version to the label, with both path parts encoded', async () => {
    const fake = fakeFetch([{ status: 200, body: wirePrompt({ version: 2 }) }])
    const mcpd = client(fake.fetch)
    const moved = await mcpd.setPromptLabel('team/greet', 'production', 2)

    expect(moved.version).toBe(2)
    expect(fake.calls[0]?.method).toBe('PUT')
    expect(fake.calls[0]?.url).toBe(`${BASE}/team%2Fgreet/labels/production?project=test`)
    expect(fake.calls[0]?.body).toEqual({ version: 2 })
    await mcpd.shutdown()
  })

  it('throws on an error answer', async () => {
    const fake = fakeFetch([{ status: 404, body: {} }])
    const mcpd = client(fake.fetch)
    await expect(mcpd.setPromptLabel('greet', 'production', 9)).rejects.toMatchObject({ status: 404 })
    await expect(mcpd.createPrompt({ name: 'greet', template: 'x' })).rejects.toMatchObject({
      status: 404,
    })
    await mcpd.shutdown()
  })
})

describe('renderPrompt', () => {
  it('fills a text prompt, with whitespace allowed inside the braces', () => {
    const p = prompt({ template: 'Hi {{name}}, from {{ city }} and {{\tcity\n}}.' })
    expect(renderPrompt(p, { name: 'Ada', city: 'Leeds' })).toBe('Hi Ada, from Leeds and Leeds.')
  })

  it('leaves a placeholder that is not a name, and an unclosed one, as written', () => {
    const p = prompt({ template: 'JSON {{1}} {{ a-b }} {{}} {"k": {{v}}} and {{open' })
    expect(renderPrompt(p, { v: 7 })).toBe('JSON {{1}} {{ a-b }} {{}} {"k": 7} and {{open')
  })

  it('accepts dots, digits and underscores after the first character', () => {
    const p = prompt({ template: '{{user.name}} {{_x1}}' })
    expect(renderPrompt(p, { 'user.name': 'Ada', _x1: 'y' })).toBe('Ada y')
  })

  it('substitutes in one pass', () => {
    const p = prompt({ template: '{{a}} {{b}}' })
    expect(renderPrompt(p, { a: '{{b}}', b: 'B' })).toBe('{{b}} B')
  })

  it('names every missing variable in order of first use', () => {
    const p = prompt({ template: '{{ zeta }} {{name}} {{alpha}} {{zeta}}' })
    let thrown: unknown
    try {
      renderPrompt(p, { name: 'Ada', extra: 'ignored' })
    } catch (error) {
      thrown = error
    }
    expect(thrown).toBeInstanceOf(PromptRenderError)
    expect((thrown as PromptRenderError).missing).toEqual(['zeta', 'alpha'])
    expect((thrown as Error).message).toMatch(/zeta, alpha/)
  })

  it('writes a non-string value in its plain string form', () => {
    const p = prompt({ template: '{{n}} {{b}} {{z}}' })
    expect(renderPrompt(p, { n: 3, b: false, z: null })).toBe('3 false null')
  })

  it('renders string content in a chat prompt and keeps every other field', () => {
    const messages = [
      { role: 'system', content: 'You help {{ customer }}.', name: '{{customer}}' },
      { role: 'user', content: [{ type: 'text', text: '{{customer}}' }] },
      'not a message',
    ]
    const p = prompt({ kind: 'chat', template: undefined, messages })
    const out = renderPrompt(p, { customer: 'Ada' })

    expect(out).toEqual([
      { role: 'system', content: 'You help Ada.', name: '{{customer}}' },
      { role: 'user', content: [{ type: 'text', text: '{{customer}}' }] },
      'not a message',
    ])
    // A copy: the cached prompt must still render next time.
    expect(messages[0]).toEqual({ role: 'system', content: 'You help {{ customer }}.', name: '{{customer}}' })
  })

  it('names missing variables across chat messages', () => {
    const messages = [
      { role: 'system', content: '{{a}}' },
      { role: 'user', content: '{{b}} {{a}}' },
    ]
    const p = prompt({ kind: 'chat', template: undefined, messages })
    expect(() => renderPrompt(p, {})).toThrow(/a, b/)
  })
})

describe('span.setPrompt', () => {
  it('sets the name and the version', async () => {
    const mcpd = client(fakeFetch().fetch)
    const span = mcpd.trace('draft')
    span.setPrompt(prompt({ name: 'greet', version: 4 }))
    expect(span.snapshot().attributes).toEqual({ 'mcpd.prompt.name': 'greet', 'mcpd.prompt.version': 4 })
    span.end()
    await mcpd.shutdown()
  })

  it('sets the name only for a fallback', async () => {
    const mcpd = client(fakeFetch().fetch)
    const span = mcpd.trace('draft')
    span.setPrompt(prompt({ name: 'greet', version: 0 }))
    expect(span.snapshot().attributes).toEqual({ 'mcpd.prompt.name': 'greet' })
    span.end()
    await mcpd.shutdown()
  })

  it('sets nothing and never throws on a missing or malformed prompt', async () => {
    const mcpd = client(fakeFetch().fetch)
    const span = mcpd.trace('draft')
    for (const bad of [null, undefined, 42, 'greet', {}, { name: 7, version: 1 }]) {
      expect(() => span.setPrompt(bad as unknown as Prompt)).not.toThrow()
    }
    expect(span.snapshot().attributes).toBeUndefined()
    span.end()
    await mcpd.shutdown()
  })
})

describe('list filters', () => {
  it('sends every trace filter in the daemon spelling, and nothing else', async () => {
    const fake = fakeFetch([{ status: 200, body: { traces: [], page: { has_more: false } } }])
    const mcpd = client(fake.fetch)
    const query = {
      maxDurationMs: 900,
      maxCost: 0.5,
      minTokens: 100,
      hasScore: 'relevance',
      scoreAbove: 0.8,
      hasFeedback: false,
      notAFilter: 'dropped',
    }
    await mcpd.listTraces(query as TraceQuery)
    await mcpd.listTraces({ hasFeedback: true })

    expect(fake.calls.map((c) => c.url)).toEqual([
      'http://daemon.test/v1/api/traces?project=test&max_duration_ms=900&max_cost=0.5' +
        '&min_tokens=100&has_score=relevance&score_above=0.8&has_feedback=false',
      'http://daemon.test/v1/api/traces?project=test&has_feedback=true',
    ])
    await mcpd.shutdown()
  })

  it('sends the prompt filters on the span list, and nothing else', async () => {
    const fake = fakeFetch([{ status: 200, body: { spans: [], page: { has_more: false } } }])
    const mcpd = client(fake.fetch)
    await mcpd.listSpans({ prompt: 'team/greet', promptVersion: 2, hasFeedback: true } as never)

    expect(fake.calls[0]?.url).toBe(
      'http://daemon.test/v1/api/spans?project=test&prompt=team%2Fgreet&prompt_version=2',
    )
    await mcpd.shutdown()
  })
})
