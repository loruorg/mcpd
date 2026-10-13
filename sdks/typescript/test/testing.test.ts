import { afterEach, describe, expect, it, vi } from 'vitest'
import { Mcpd } from '../src/index.js'
import { TestClient } from '../src/testing.js'

async function answer(mcpd: Mcpd): Promise<void> {
  await mcpd.withSpan('answer', async (run) => {
    await mcpd.withSpan('lookup', async () => 'ok', { kind: 'tool_call' })
    await mcpd.withSpan('generate', async (g) => g.setOutput('it ships tomorrow'), { kind: 'model_generation' })
    mcpd.log('info', 'answered', { order: 'A-1' })
    mcpd.score({ scorerId: 'relevance', value: 0.9, min: 0, max: 1 })
    mcpd.feedback({ kind: 'thumbs', value: 1 })
    run.setOutput('done')
  })
}

describe('TestClient', () => {
  afterEach(() => {
    vi.unstubAllEnvs()
  })

  it('is an Mcpd, and reads back every signal with no flush', async () => {
    const mcpd = new TestClient()
    expect(mcpd).toBeInstanceOf(Mcpd)
    await answer(mcpd)
    expect((await mcpd.spans()).map((s) => s.name)).toEqual(['answer', 'lookup', 'generate'])
    expect((await mcpd.logs()).map((l) => l.message)).toEqual(['answered'])
    expect((await mcpd.scores())[0]?.scorerId).toBe('relevance')
    expect((await mcpd.recordedFeedback())[0]?.kind).toBe('thumbs')
  })

  it('keeps one record per span id, finished spans only', async () => {
    const mcpd = new TestClient()
    const open = mcpd.trace('still_running')
    await answer(mcpd)
    const spans = await mcpd.spans()
    expect(mcpd.recorder.spans.length).toBeGreaterThan(spans.length)
    expect(new Set(spans.map((s) => s.spanId)).size).toBe(spans.length)
    expect(spans.map((s) => s.name)).not.toContain('still_running')
    open.end()
    expect((await mcpd.spans()).map((s) => s.name)).toContain('still_running')
  })

  it('filters by name and kind', async () => {
    const mcpd = new TestClient()
    await answer(mcpd)
    expect((await mcpd.spans({ kind: 'tool_call' })).map((s) => s.name)).toEqual(['lookup'])
    expect((await mcpd.spans({ name: 'generate' }))[0]?.output).toBe('it ships tomorrow')
  })

  it('span(name) fails on none and on two', async () => {
    const mcpd = new TestClient()
    await answer(mcpd)
    await expect(mcpd.span('missing')).rejects.toThrow(/found 0/)
    await answer(mcpd)
    await expect(mcpd.span('lookup')).rejects.toThrow(/found 2/)
  })

  it('builds the tree', async () => {
    const mcpd = new TestClient()
    await answer(mcpd)
    const [root, ...others] = await mcpd.trees()
    expect(others).toHaveLength(0)
    expect(root?.span.name).toBe('answer')
    expect(root?.children.map((c) => c.span.name)).toEqual(['lookup', 'generate'])
  })

  it('reset forgets everything', async () => {
    const mcpd = new TestClient()
    await answer(mcpd)
    await mcpd.reset()
    expect(await mcpd.spans()).toEqual([])
    expect(await mcpd.logs()).toEqual([])
  })

  it('goes through the same redaction as a real client', async () => {
    const mcpd = new TestClient({ redact: (v) => (typeof v === 'string' ? v.replace(/tomorrow/, '[date]') : v) })
    await answer(mcpd)
    expect((await mcpd.span('generate')).output).toBe('it ships [date]')
  })

  it('stays on when the environment turns mcpd off', async () => {
    vi.stubEnv('MCPD_ENABLED', 'false')
    vi.stubEnv('MCPD_SAMPLE', '0')
    const mcpd = new TestClient()
    await answer(mcpd)
    expect(await mcpd.spans()).toHaveLength(3)
  })
})
