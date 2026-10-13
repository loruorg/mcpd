/**
 * A support agent, instrumented with mcpd.
 *
 * The agent is fake: it answers one ticket from a hard-coded order book and
 * fails on a second one. Everything around it is real, and is the same code you
 * would write around a real model call.
 *
 * Run it against a local daemon:
 *
 *   mcpd serve --data ./mcpd.db
 *   npm install && node quickstart.ts
 *
 * Then open http://localhost:4317 and read the two runs. Node 22.18 or later
 * runs a TypeScript file directly; on an older Node, compile it first.
 */

// #region init
import { Mcpd } from '@mcpd/sdk'

// Every field here also reads an environment variable, so the same code runs
// unchanged in development and in production. With no arguments at all this
// talks to a daemon on http://localhost:4317.
const mcpd = new Mcpd({
  project: 'support',
  environment: 'development',
  serviceName: 'support-agent',
  serviceVersion: '1.4.0',
})
// #endregion init

import { renderPrompt, type McpdSpan } from '@mcpd/sdk'
import assert from 'node:assert/strict'
import { TestClient } from '@mcpd/sdk/testing'

// ---------------------------------------------------------------------------
// The agent
// ---------------------------------------------------------------------------

interface Ticket {
  id: string
  customerId: string
  question: string
}

/** The answer prompt: stored in mcpd at start, and the fallback if it cannot be read. */
const ANSWER_MESSAGES = [
  { role: 'system', content: 'You are a support agent. Be brief and exact.' },
  { role: 'user', content: '{{question}}\n\nOrder: {{order}}' },
]

const ORDER_BOOK: Record<string, { status: string; eta: string }> = {
  'A-1042': { status: 'in transit', eta: '2026-09-02' },
}

/** Stand-in for the latency of a real call. */
const pause = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms))

// #region trace
/**
 * One ticket, one trace.
 *
 * `withSpan` opens the span, keeps it active for everything the callback
 * starts, and ends it when the callback settles. Nothing inside has to be
 * passed a handle to be nested under it.
 */
async function handleTicket(ticket: Ticket): Promise<{ answer: string; traceId: string }> {
  let traceId = ''
  const answer = await mcpd.withSpan('support_agent', async (run) => {
    // Keep the id: a score or a piece of feedback that arrives later attaches
    // to this run by it.
    traceId = run.traceId
    // Tags label the whole run, so the trace list can filter on them.
    run.setTags(['support', 'tier-1'])
    run.setCorrelation({ sessionId: ticket.id, userId: ticket.customerId })
    run.setInput({ question: ticket.question })

    const order = await lookUpOrder(ticket)
    const answer = await draftAnswer(ticket, order)

    run.setOutput({ answer })
    return answer
  })
  return { answer, traceId }
}
// #endregion trace

/** A tool call: your own code, doing something the model asked for. */
async function lookUpOrder(ticket: Ticket): Promise<{ status: string; eta: string }> {
  return await mcpd.withSpan(
    'lookup_order',
    async (tool) => {
      const orderId = ticket.question.match(/[A-Z]-\d+/)?.[0] ?? 'unknown'
      tool.setInput({ orderId })
      await pause(20)

      const order = ORDER_BOOK[orderId]
      if (!order) {
        // Throwing is enough. The span is marked `error`, the message and the
        // stack are recorded, and the error carries on up untouched.
        throw new Error(`no order ${orderId} in the order book`)
      }

      tool.setOutput(order)
      return order
    },
    { kind: 'tool_call', entityKind: 'tool', entityName: 'order_lookup' },
  )
}

/**
 * Where a real provider call goes. This is the shape Anthropic answers in.
 *
 * The span is passed in so the time to the first token can be marked on it.
 */
async function callModel(generation: McpdSpan, order: { status: string; eta: string }) {
  await pause(40)
  generation.markFirstToken()
  mcpd.log('info', 'answered from cache-warm prompt', { cached: true })
  const answer = `Your order is ${order.status} and should arrive on ${order.eta}.`
  return {
    id: 'msg_01',
    type: 'message',
    model: 'claude-sonnet-4-5',
    stop_reason: 'end_turn',
    content: [{ type: 'text', text: answer }],
    usage: { input_tokens: 172, cache_read_input_tokens: 640, output_tokens: 37 },
  }
}

// #region prompt
/**
 * A model generation. Its prompt is stored in mcpd, so a new version ships by
 * moving the `production` label, with no deploy.
 */
async function draftAnswer(
  ticket: Ticket,
  order: { status: string; eta: string },
): Promise<string> {
  // Cached for a minute. When the daemon cannot answer, the last copy is
  // used, and with no copy the fallback, so an outage never stops an answer.
  const prompt = await mcpd.getPrompt('support-answer', {
    label: 'production',
    fallback: ANSWER_MESSAGES,
  })
  const messages = renderPrompt(prompt, {
    question: ticket.question,
    order: JSON.stringify(order),
  })

  return await mcpd.withSpan(
    'draft_answer',
    async (generation) => {
      // Links the call to its prompt version, so spans can be read per version.
      generation.setPrompt(prompt)
      generation.setInput(messages)
      const response = await callModel(generation, order)
      const answer = response.content[0]?.text ?? ''
      generation.setOutput(answer)
      // The model, the token counts mcpd prices, the response id and the
      // finish reason, read from the response in one call.
      generation.recordResponse(response)
      return answer
    },
    { kind: 'model_generation', entityKind: 'model', entityName: 'answer_writer' },
  )
}
// #endregion prompt

interface ChatChunk {
  id: string
  model: string
  choices: Array<{ index: number; delta: { content?: string }; finish_reason?: string }>
  usage?: { prompt_tokens: number; completion_tokens: number; total_tokens: number }
}

/** Stand-in for a provider's stream, in the shape OpenAI streams chat completions. */
async function* fakeChatStream(text: string): AsyncGenerator<ChatChunk> {
  const base = { id: 'chatcmpl-01', model: 'gpt-4o-mini' }
  for (const word of text.split(/(?= )/)) {
    await pause(5)
    yield { ...base, choices: [{ index: 0, delta: { content: word } }] }
  }
  yield { ...base, choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] }
  const usage = { prompt_tokens: 24, completion_tokens: 6, total_tokens: 30 }
  yield { ...base, choices: [], usage }
}

// #region stream
/**
 * A streamed model call. Read the wrapper instead of the provider's stream:
 * every chunk comes through unchanged, and when the stream ends the span gets
 * the time to first token, the text, the usage and the finish reason.
 */
async function streamAnswer(client: Mcpd, question: string): Promise<string> {
  return await client.withSpan(
    'stream_answer',
    async (generation) => {
      generation.setInput([{ role: 'user', content: question }])
      // Where `await openai.chat.completions.create({ ..., stream: true })` goes.
      const stream = fakeChatStream('Your order is in transit.')
      let answer = ''
      for await (const chunk of generation.recordStream(stream)) {
        answer += chunk.choices[0]?.delta.content ?? ''
      }
      return answer
    },
    { kind: 'model_generation' },
  )
}
// #endregion stream

// #region testing
/**
 * A unit test. Pass a `TestClient` where your code takes an `Mcpd`, then read
 * back what it recorded: no daemon, no port, no flush.
 */
async function testStreamAnswer(): Promise<void> {
  const test = new TestClient()
  const answer = await streamAnswer(test, 'Where is my order A-1042?')

  const call = await test.span('stream_answer')
  assert.equal(call.output, answer)
  assert.equal(call.model, 'gpt-4o-mini')
  assert.equal(call.usage?.outputTokens, 6)

  const [run] = await test.trees()
  assert.equal(run?.span.name, 'stream_answer')
  await test.shutdown()
}
// #endregion testing

// ---------------------------------------------------------------------------
// Running it
// ---------------------------------------------------------------------------

async function main(): Promise<void> {
  // Store the prompt the agent reads, and label it `production`. Posting the
  // same body again stores nothing, so this is safe on every start.
  await mcpd.createPrompt({
    name: 'support-answer',
    messages: ANSWER_MESSAGES,
    note: 'first version',
    labels: ['production'],
  })

  // The good path.
  const { answer, traceId } = await handleTicket({
    id: 'tk-1',
    customerId: 'cust-77',
    question: 'Where is my order A-1042?',
  })
  console.log('answer:', answer)

  // A streamed call, and the unit test that checks what it records.
  console.log('streamed:', await streamAnswer(mcpd, 'Is A-1042 on its way?'))
  await testStreamAnswer()

  // #region score
  // A machine evaluation of the run. `min` and `max` are required: 0.86 on an
  // undeclared scale cannot be compared to anything.
  mcpd.score({
    traceId,
    scorerId: 'answer_relevance',
    value: 0.86,
    min: 0,
    max: 1,
    reason: 'Answered the question and gave the delivery date.',
  })

  // A human evaluation. Kept separate from a score, and never averaged with
  // one, because the two answer different questions.
  mcpd.feedback({
    traceId,
    kind: 'thumbs',
    value: 'up',
    comment: 'Exactly what the customer asked for.',
    authorKind: 'end_user',
  })

  // Anything you already count can travel with the trace.
  mcpd.metric('support.tickets_handled', 1, 'counter', { labels: { queue: 'tier-1' } })
  // #endregion score

  // The failing path. The error is recorded on the span and then rethrown, so
  // the agent's own error handling is unchanged by being observed.
  try {
    await handleTicket({
      id: 'tk-2',
      customerId: 'cust-77',
      question: 'And what about order B-9999?',
    })
  } catch (error) {
    console.log('failed as expected:', (error as Error).message)
  }

  // Nothing has left the process yet: recording is a queue append. `flush`
  // sends what is waiting, and `shutdown` sends it and stops the worker.
  await mcpd.flush()

  // #region query
  // The same client reads. This is the query that finds bad answers: every run
  // that a scorer put below the bar.
  const suspect = await mcpd.listTraces({
    hasScore: 'answer_relevance',
    scoreBelow: 0.5,
    limit: 10,
  })
  console.log(`runs scoring under 0.5: ${suspect.traces.length}`)

  const recent = await mcpd.listTraces({ limit: 5, sort: 'start_time', order: 'desc' })
  for (const trace of recent.traces) {
    const seconds = ((trace.durationNs ?? 0) / 1e9).toFixed(2)
    console.log(
      `${trace.traceId.slice(0, 8)}  ${trace.name.padEnd(16)} ${trace.status.padEnd(6)} ` +
        `${seconds}s  ${trace.spanCount} spans  $${(trace.cost?.totalCost ?? 0).toFixed(6)}`,
    )
  }

  // One run, with every span in it, ordered so a waterfall renders directly.
  const detail = await mcpd.getTrace(traceId, { include: ['scores', 'feedback'] })
  for (const span of detail.spans) {
    console.log(`  ${span.kind.padEnd(18)} ${span.name}`)
  }
  // #endregion query

  // Short-lived processes lose their last batch without this.
  await mcpd.shutdown()
}

await main()
