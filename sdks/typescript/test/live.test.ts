/**
 * Integration tests against a running daemon.
 *
 * Skipped unless `MCPD_TEST_ENDPOINT` is set, so the ordinary test run needs no
 * server:
 *
 *   mcpd serve --data /tmp/sdk-live.db --listen 127.0.0.1:47401
 *   MCPD_TEST_ENDPOINT=http://127.0.0.1:47401 npx vitest run
 *
 * `MCPD_REQUIRE_LIVE=1` turns a missing endpoint into a failure rather than a
 * skip. CI sets it in the job that starts a daemon, so if the variable is ever
 * renamed or stops reaching the tests the job goes red instead of quietly
 * skipping. The Rust SDK's live suite spent a long time reporting passes having
 * contacted no server at all, and this is the guard against a repeat.
 */

import { describe, expect, it } from 'vitest'
import { Mcpd, Worker, newTraceId, workflow } from '../src/index.js'

const env = (globalThis as { process?: { env?: Record<string, string | undefined> } }).process
  ?.env
const endpoint = env?.['MCPD_TEST_ENDPOINT']
const requireLive = env?.['MCPD_REQUIRE_LIVE']

// A skip and a pass look the same in the summary line, which is how a suite
// that reaches nothing reports green. When the guard is set, say so as a
// failure instead. The describe below still skips on its own so the ordinary
// offline run stays quiet.
describe.skipIf(Boolean(endpoint) || !requireLive)('the live suite', () => {
  it('was required to reach a daemon', () => {
    throw new Error(
      'MCPD_REQUIRE_LIVE is set but MCPD_TEST_ENDPOINT is not, so this suite ' +
        'would have skipped without reaching a server. That is the exact ' +
        'silence this variable exists to break.',
    )
  })
})

/** A project of its own per run, so repeated runs never read each other's data. */
const project = `sdk-live-${newTraceId().slice(0, 8)}`

function client(): Mcpd {
  return new Mcpd({
    endpoint: endpoint as string,
    project,
    environment: 'test',
    serviceName: 'sdk-live',
    serviceVersion: '0.1.0',
    flushOnExit: false,
    onError: (message, error) => {
      throw new Error(`${message}: ${String(error)}`)
    },
  })
}

describe.skipIf(!endpoint)('against a live daemon', () => {
  it('answers the health probes', async () => {
    const mcpd = client()
    expect((await mcpd.health()).status).toBeTypeOf('string')
    expect((await mcpd.ready()).status).toBeTypeOf('string')
    await mcpd.shutdown()
  })

  it('round-trips a whole run, with every signal attached', async () => {
    const mcpd = client()

    const traceId = await mcpd.withSpan(
      'live_agent',
      async (run) => {
        run.setTags(['live'])
        run.setInput({ question: 'is this recorded?' })
        run.setCorrelation({ sessionId: 'sess-live', userId: 'user-live' })

        await mcpd.withSpan(
          'live_model',
          async (generation) => {
            generation.setModel('claude-sonnet-4', 'anthropic')
            generation.setInput([{ role: 'user', content: 'is this recorded?' }])
            generation.setOutput('yes')
            generation.setUsage({ inputTokens: 1_000, outputTokens: 100 })
            mcpd.log('info', 'model answered', { streamed: false })
          },
          { kind: 'model_generation' },
        )

        await expect(
          mcpd.withSpan('live_tool', async () => {
            throw new Error('the tool refused')
          }),
        ).rejects.toThrow('the tool refused')

        mcpd.metric('live.calls', 1, 'counter', { labels: { kind: 'test' } })
        run.setOutput({ answer: 'yes' })
        return run.traceId
      },
      { kind: 'agent_run' },
    )

    mcpd.score({ traceId, scorerId: 'live_scorer', value: 0.5, min: 0, max: 1, reason: 'halfway' })
    mcpd.feedback({ traceId, kind: 'rating', value: 4, comment: 'good enough' })
    await mcpd.shutdown()

    const reader = client()
    const detail = await reader.getTrace(traceId, { include: ['logs', 'scores', 'feedback'] })

    expect(detail.trace.name).toBe('live_agent')
    expect(detail.spans).toHaveLength(3)
    expect(detail.trace.correlation.sessionId).toBe('sess-live')
    expect(detail.trace.tags).toContain('live')

    const model = detail.spans.find((s) => s.name === 'live_model')
    expect(model?.usage?.inputTokens).toBe(1_000)
    // The daemon prices the tokens at ingest; the SDK never sends a cost.
    expect(model?.cost?.totalCost).toBeGreaterThan(0)
    expect(model?.parentSpanId).toBe(detail.trace.rootSpanId)

    const tool = detail.spans.find((s) => s.name === 'live_tool')
    expect(tool?.status).toBe('error')
    expect(tool?.error?.message).toBe('the tool refused')

    expect(detail.logs?.[0]?.message).toBe('model answered')
    expect(detail.scores?.[0]?.value).toBe(0.5)
    expect(detail.feedback?.[0]?.value).toBe(4)

    const spans = await reader.listSpans({ traceId, kind: 'model_generation' })
    expect(spans.spans).toHaveLength(1)

    const logs = await reader.listLogs({ traceId })
    expect(logs.logs).toHaveLength(1)

    const scores = await reader.listScores({ scorerId: 'live_scorer' })
    expect(scores.scores).toHaveLength(1)

    // This build serves the feedback list per trace, so name the trace.
    const feedback = await reader.listFeedback({ traceId })
    expect(feedback.feedback).toHaveLength(1)

    await reader.shutdown()
  }, 30_000)

  it('continues a trace across a service boundary (B7)', async () => {
    const upstream = client()
    const downstream = client()

    const caller = upstream.trace('caller')
    const header = caller.traceparent()
    const callee = downstream.trace('callee', { traceparent: header })
    callee.end()
    caller.end()
    await Promise.all([upstream.shutdown(), downstream.shutdown()])

    const reader = client()
    const detail = await reader.getTrace(caller.traceId)
    expect(detail.spans.map((s) => s.name).sort()).toEqual(['callee', 'caller'])
    expect(detail.spans.find((s) => s.name === 'callee')?.parentSpanId).toBe(caller.spanId)
    await reader.shutdown()
  }, 30_000)

  it('sends an unfinished span, then updates it (B8)', async () => {
    const mcpd = client()
    const run = mcpd.trace('slow_run')
    await mcpd.flush()

    const reader = client()
    const open = await reader.getTrace(run.traceId)
    expect(open.spans[0]?.endTime).toBeUndefined()
    expect(open.spans[0]?.status).toBe('unset')

    run.end()
    await mcpd.shutdown()

    const closed = await reader.getTrace(run.traceId)
    expect(closed.spans).toHaveLength(1)
    expect(closed.spans[0]?.endTime).toBeTypeOf('string')
    expect(closed.spans[0]?.status).toBe('ok')
    await reader.shutdown()
  }, 30_000)

  it('gzips a large batch and the daemon reads it', async () => {
    const mcpd = client()
    const prompt = 'x'.repeat(4_096)
    const ids: string[] = []
    for (let i = 0; i < 20; i++) {
      const span = mcpd.trace(`bulk-${i}`)
      span.setInput(prompt)
      span.end()
      ids.push(span.traceId)
    }
    await mcpd.shutdown()

    const reader = client()
    const detail = await reader.getTrace(ids[0] as string)
    expect(detail.spans[0]?.input).toBe(prompt)
    await reader.shutdown()
  }, 30_000)

  it('reports analytics for the window it just filled', async () => {
    const reader = client()
    const to = new Date(Date.now() + 60_000).toISOString()
    const from = new Date(Date.now() - 3_600_000).toISOString()

    const summary = await reader.analyticsSummary({ from, to })
    expect(summary.traceCount).toBeGreaterThan(0)

    const cost = await reader.analyticsCost({ from, to, groupBy: 'model' })
    expect(cost.totalCost).toBeGreaterThan(0)

    const series = await reader.queryMetrics({ name: 'live.calls', from, to, agg: 'sum' })
    expect(series.name).toBe('live.calls')
    await reader.shutdown()
  }, 30_000)
})

describe.skipIf(!endpoint)('runs against a live daemon', () => {
  it('registers a workflow, serves a run, and reads the run and its trace back', async () => {
    const mcpd = client()
    const wf = workflow({ id: 'live_support' })
      .step('classify', (_ctx, input: { ticket: string }) => ({
        kind: input.ticket.startsWith('Q') ? 'question' : 'bug',
      }))
      .agent(
        'draft',
        async (ctx, input: { kind: string }) => {
          ctx.setUsage({ inputTokens: 120, outputTokens: 40 })
          ctx.setState({ drafted: true })
          return await ctx.withSpan('live_model', (span) => {
            span.setModel('claude-sonnet-4', 'anthropic')
            span.setUsage({ inputTokens: 120, outputTokens: 40 })
            return `an answer to a ${input.kind}`
          })
        },
        { input: { kind: '${steps.classify.output.kind}' } },
      )
      .output({ answer: '${steps.draft.output}' })

    const worker = new Worker(mcpd, {
      workerId: 'live-worker',
      waitMs: 1_000,
      onError: (message, error) => {
        throw new Error(`${message}: ${String(error)}`)
      },
    }).register(wf)
    await worker.registerDefinitions()

    const run = await mcpd.runs.start({ definitionId: 'live_support', input: { ticket: 'Q-42' } })
    expect(run.status).toBe('running')
    expect(run.definitionId).toBe('live_support')

    let detail = await mcpd.runs.get(run.runId)
    for (let i = 0; i < 10 && detail.run.status === 'running'; i++) {
      await worker.claim(4, run.runId)
      await worker.drain()
      detail = await mcpd.runs.get(run.runId)
    }
    expect(detail.run.status).toBe('succeeded')
    expect(detail.run.output).toEqual({ answer: 'an answer to a question' })
    expect(detail.run.state).toEqual({ drafted: true })
    const byKey = Object.fromEntries(detail.attempts.map((a) => [a.stepKey, a]))
    expect(byKey['classify']?.status).toBe('succeeded')
    expect(byKey['draft']?.status).toBe('succeeded')
    expect(byKey['draft']?.kind).toBe('agent')
    expect(byKey['draft']?.input).toEqual({ kind: 'question' })
    expect(byKey['draft']?.usage?.inputTokens).toBe(120)

    const kinds: string[] = []
    for await (const event of mcpd.runs.watch(run.runId, { pollMs: 50 })) kinds.push(event.kind)
    expect(kinds[0]).toBe('run_started')
    expect(kinds).toContain('step_finished')
    expect(kinds.at(-1)).toBe('run_ended')

    await mcpd.flush()
    // The daemon writes the run's own spans through an outbox drained on its
    // own clock, so give it a moment before reading the trace.
    let names: string[] = []
    for (let i = 0; i < 40; i++) {
      const trace = await mcpd.getTrace(run.traceId).catch(() => undefined)
      names = trace?.spans.map((s) => s.name) ?? []
      if (names.includes('live_model') && names.length >= 4) break
      await new Promise((resolve) => setTimeout(resolve, 250))
    }
    expect(names).toContain('live_model')
    const trace = await mcpd.getTrace(run.traceId)
    const model = trace.spans.find((s) => s.name === 'live_model')
    expect(model?.parentSpanId).toBe(byKey['draft']?.spanId)
    await mcpd.shutdown()
  }, 60_000)

  it('waits for an event, resumes on publish, and ends another run on cancel', async () => {
    const mcpd = client()
    const wf = workflow({ id: 'live_approval' })
      .waitFor('approval', { event: 'approved', key: '${input.ticket}', timeoutMs: 60_000 })
      .step('close', (_ctx, input: { by: string }) => ({ closed_by: input.by }), {
        input: { by: '${steps.approval.output.by}' },
      })
      .output('${steps.close.output}')
    const worker = new Worker(mcpd, {
      workerId: 'live-approver',
      waitMs: 500,
      onError: (message, error) => {
        throw new Error(`${message}: ${String(error)}`)
      },
    }).register(wf)
    await worker.registerDefinitions()

    const waiting = await mcpd.runs.start({ definitionId: 'live_approval', input: { ticket: 'T-7' } })
    let detail = await mcpd.runs.get(waiting.runId)
    for (let i = 0; i < 10 && detail.waits.length === 0; i++) {
      await new Promise((resolve) => setTimeout(resolve, 100))
      detail = await mcpd.runs.get(waiting.runId)
    }
    expect(detail.waits.map((w) => [w.event, w.key])).toEqual([['approved', 'T-7']])

    const receipt = await mcpd.events.publish({
      event: 'approved',
      key: 'T-7',
      payload: { by: 'ops' },
      eventId: 'live-approval-1',
    })
    expect(receipt.outcome).toBe('delivered')
    const again = await mcpd.events.publish({ event: 'approved', key: 'T-7', eventId: 'live-approval-1' })
    expect(again.outcome).toBe('duplicate')

    for (let i = 0; i < 10 && detail.run.status === 'running'; i++) {
      await worker.claim(1, waiting.runId)
      await worker.drain()
      detail = await mcpd.runs.get(waiting.runId)
    }
    expect(detail.run.status).toBe('succeeded')
    expect(detail.run.output).toEqual({ closed_by: 'ops' })

    const doomed = await mcpd.runs.start({ definitionId: 'live_approval', input: { ticket: 'T-8' } })
    const cancelled = await mcpd.runs.cancel(doomed.runId)
    expect(cancelled.status).toBe('cancelled')
    const kinds: string[] = []
    for await (const event of mcpd.runs.watch(doomed.runId, { pollMs: 50 })) kinds.push(event.kind)
    expect(kinds.at(-1)).toBe('run_ended')

    const listed = await mcpd.runs.list({ definitionId: 'live_approval' })
    expect(listed.runs.map((r) => r.runId).sort()).toEqual([doomed.runId, waiting.runId].sort())
    await mcpd.shutdown()
  }, 60_000)
})
