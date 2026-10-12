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

// ---------------------------------------------------------------------------
// The agent
// ---------------------------------------------------------------------------

interface Ticket {
  id: string
  customerId: string
  question: string
}

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

/** A model generation: the prompt, the completion, and what it cost. */
async function draftAnswer(
  ticket: Ticket,
  order: { status: string; eta: string },
): Promise<string> {
  return await mcpd.withSpan(
    'draft_answer',
    async (generation) => {
      const prompt = [
        { role: 'system', content: 'You are a support agent. Be brief and exact.' },
        { role: 'user', content: `${ticket.question}\n\nOrder: ${JSON.stringify(order)}` },
      ]
      generation.setModel('claude-sonnet-4', 'anthropic')
      generation.setInput(prompt)

      // Where a real provider call goes.
      await pause(40)
      const answer = `Your order is ${order.status} and should arrive on ${order.eta}.`

      generation.setOutput(answer)
      // Token counts are what mcpd prices. Report what the provider told you.
      generation.setUsage({
        inputTokens: 812,
        outputTokens: 37,
        inputDetails: { cacheRead: 640, text: 172 },
      })
      // A point in time inside the span, not a duration of its own.
      generation.event('first_token', { kind: 'model_chunk' })

      mcpd.log('info', 'answered from cache-warm prompt', { cached: true })
      return answer
    },
    { kind: 'model_generation', entityKind: 'model', entityName: 'answer_writer' },
  )
}

// ---------------------------------------------------------------------------
// Running it
// ---------------------------------------------------------------------------

async function main(): Promise<void> {
  // The good path.
  const { answer, traceId } = await handleTicket({
    id: 'tk-1',
    customerId: 'cust-77',
    question: 'Where is my order A-1042?',
  })
  console.log('answer:', answer)

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
