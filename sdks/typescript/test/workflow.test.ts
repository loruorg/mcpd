import { describe, expect, it } from 'vitest'
import { all, any, not, when, workflow } from '../src/index.js'
import type { StepHandler } from '../src/index.js'

const body: StepHandler = () => 'done'

describe('the definition document', () => {
  it('spells every construct the way the daemon reads it', () => {
    const wf = workflow({
      id: 'support',
      version: 2,
      inputSchema: { type: 'object', required: ['ticket'] },
      limits: { maxSteps: 50, runTimeoutMs: 60_000, maxCostUsd: 2 },
    })
      .step('classify', body, {
        input: { ticket: '${input.ticket}' },
        effect: 'at_most_once',
        stepTimeoutMs: 5_000,
        retry: { maxAttempts: 3, backoff: 'exponential', initialMs: 100, maxMs: 1_000 },
      })
      .agent('draft', body, {
        when: when('steps.classify.output.kind', 'eq', 'question'),
        onError: 'continue',
      })
      .sleep('cool', { ms: 10 })
      .sleepUntil('bill', { at: '${input.billing_date}' })
      .waitFor('approval', {
        event: 'approved',
        key: '${input.ticket}',
        timeoutMs: 1_000,
        onTimeout: 'continue',
        acceptParked: true,
      })
      .parallel('checks', (b) => b.step('fraud', body).agent('policy', body))
      .foreach('notify', { over: 'input.recipients', maxItems: 10, concurrency: 2 }, (b) =>
        b.step('send', body, { input: { to: '${item}', position: '${index}' } }),
      )
      .loop('refine', { maxIterations: 3, until: when('steps.rewrite.output.done', 'eq', true) }, (b) =>
        b.step('rewrite', body),
      )
      .subworkflow('escalate', {
        definition: 'escalation',
        version: 1,
        input: { ticket: '${input.ticket}' },
        when: not(when('steps.checks.fraud.output.clean', 'exists')),
      })
      .output({ answer: '${steps.draft.output}' })

    expect(wf.document()).toEqual({
      id: 'support',
      kind: 'workflow',
      version: 2,
      body: {
        input_schema: { type: 'object', required: ['ticket'] },
        limits: { max_steps: 50, run_timeout_ms: 60_000, max_cost_usd: 2 },
        output: { answer: '${steps.draft.output}' },
        steps: [
          {
            kind: 'task',
            id: 'classify',
            input: { ticket: '${input.ticket}' },
            effect: 'at_most_once',
            step_timeout_ms: 5_000,
            retry: { max_attempts: 3, backoff: 'exponential', initial_ms: 100, max_ms: 1_000 },
          },
          {
            kind: 'agent',
            id: 'draft',
            when: { path: 'steps.classify.output.kind', op: 'eq', value: 'question' },
            on_error: 'continue',
          },
          { kind: 'sleep', id: 'cool', ms: 10 },
          { kind: 'sleep_until', id: 'bill', at: '${input.billing_date}' },
          {
            kind: 'wait_event',
            id: 'approval',
            event: 'approved',
            key: '${input.ticket}',
            timeout_ms: 1_000,
            accept_parked: true,
            on_timeout: 'continue',
          },
          {
            kind: 'parallel',
            id: 'checks',
            branches: [
              { kind: 'task', id: 'fraud' },
              { kind: 'agent', id: 'policy' },
            ],
          },
          {
            kind: 'foreach',
            id: 'notify',
            over: 'input.recipients',
            body: { kind: 'task', id: 'send', input: { to: '${item}', position: '${index}' } },
            max_items: 10,
            concurrency: 2,
          },
          {
            kind: 'loop',
            id: 'refine',
            body: { kind: 'task', id: 'rewrite' },
            max_iterations: 3,
            until: { path: 'steps.rewrite.output.done', op: 'eq', value: true },
          },
          {
            kind: 'subworkflow',
            id: 'escalate',
            definition: 'escalation',
            version: 1,
            input: { ticket: '${input.ticket}' },
            when: { not: { path: 'steps.checks.fraud.output.clean', op: 'exists' } },
          },
        ],
      },
    })
  })

  it('leaves out what was not said, so the daemon applies its own defaults', () => {
    const doc = workflow({ id: 'plain' }).step('one', body).document()
    expect(doc).toEqual({ id: 'plain', kind: 'workflow', body: { steps: [{ kind: 'task', id: 'one' }] } })
    expect('version' in doc).toBe(false)
  })

  it('combines predicates the way the contract spells them', () => {
    expect(all(when('a', 'exists'), any(when('b', 'gt', 1), not(when('c', 'in', [1, 2]))))).toEqual({
      all: [
        { path: 'a', op: 'exists' },
        { any: [{ path: 'b', op: 'gt', value: 1 }, { not: { path: 'c', op: 'in', value: [1, 2] } }] },
      ],
    })
  })

  it('refuses an id the daemon would refuse', () => {
    expect(() => workflow({ id: 'Support' })).toThrow(/not a step id/)
    expect(() => workflow({ id: 'ok' }).step('9lives', body)).toThrow(/not a step id/)
    expect(() => workflow({ id: 'ok' }).sleep('has-dash', { ms: 1 })).toThrow(/not a step id/)
  })

  it('holds a foreach or loop body to one claimable step', () => {
    expect(() =>
      workflow({ id: 'ok' }).foreach('each', { over: 'input.items', maxItems: 1 }, (b) =>
        b.step('a', body).step('b', body),
      ),
    ).toThrow(/exactly one body step/)
    expect(() => workflow({ id: 'ok' }).loop('again', { maxIterations: 1 }, () => {})).toThrow(
      /exactly one body step/,
    )
  })
})

describe('finding the handler for a step key', () => {
  const classify: StepHandler = () => 'classify'
  const fraud: StepHandler = () => 'fraud'
  const send: StepHandler = () => 'send'
  const rewrite: StepHandler = () => 'rewrite'
  const wf = workflow({ id: 'support' })
    .step('classify', classify)
    .parallel('checks', (b) => b.step('fraud', fraud))
    .foreach('notify', { over: 'input.to', maxItems: 5 }, (b) => b.step('send', send))
    .loop('refine', { maxIterations: 2 }, (b) => b.step('rewrite', rewrite))

  it('resolves a plain step, a branch, an item and an iteration', () => {
    expect(wf.handlerFor('classify')).toBe(classify)
    expect(wf.handlerFor('checks/fraud')).toBe(fraud)
    expect(wf.handlerFor('notify[3]')).toBe(send)
    expect(wf.handlerFor('notify[3]/send')).toBe(send)
    expect(wf.handlerFor('refine[1]')).toBe(rewrite)
    expect(wf.handlerFor('refine[1]/rewrite')).toBe(rewrite)
  })

  it('has nothing for a key it never registered', () => {
    expect(wf.handlerFor('checks')).toBeUndefined()
    expect(wf.handlerFor('unknown')).toBeUndefined()
    expect(wf.handlerFor('checks/policy')).toBeUndefined()
  })
})
