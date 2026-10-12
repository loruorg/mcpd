# @mcpd/sdk

Agent observability and durable runs for TypeScript and JavaScript. Records
what your agent did, what the model was sent, what it returned, what it cost,
and what a scorer or a person thought of the answer, then posts it to an
[mcpd](https://mcpd.sh) daemon. The same client serves durable workflows: the
daemon owns the run between steps, your process owns everything inside one.

No runtime dependencies. Built-in `fetch` and `node:zlib` only.

## A complete example

```ts
import { Mcpd } from '@mcpd/sdk'

// Reads MCPD_ENDPOINT, MCPD_PROJECT and the rest of the environment. With no
// arguments at all it talks to a daemon on http://localhost:4317.
const mcpd = new Mcpd({ project: 'support', serviceName: 'support-agent' })

// Keep the trace id: a score or a piece of feedback that arrives later attaches
// to the run by it.
let traceId = ''

const answer = await mcpd.withSpan('support_agent', async (run) => {
  traceId = run.traceId
  run.setTags(['tier-1'])
  run.setInput({ question: 'Where is my order A-1042?' })

  // Anything started inside the callback nests under it on its own.
  const order = await mcpd.withSpan(
    'lookup_order',
    async (tool) => {
      tool.setInput({ orderId: 'A-1042' })
      const result = { status: 'in transit', eta: '2026-09-02' }
      tool.setOutput(result)
      return result
    },
    { kind: 'tool_call' },
  )

  const reply = await mcpd.withSpan(
    'draft_answer',
    async (generation) => {
      generation.setModel('claude-sonnet-4', 'anthropic')
      generation.setInput([{ role: 'user', content: 'Where is my order A-1042?' }])

      // Where a real provider call goes.
      const text = `Your order is ${order.status}, arriving ${order.eta}.`

      generation.setOutput(text)
      generation.setUsage({ inputTokens: 812, outputTokens: 37 })
      return text
    },
    { kind: 'model_generation' },
  )

  run.setOutput({ answer: reply })
  return reply
})

console.log(answer)

// A scorer's verdict, and a person's. `min` and `max` are required: a bare
// number with no declared scale cannot be compared to anything.
mcpd.score({ traceId, scorerId: 'answer_relevance', value: 0.86, min: 0, max: 1 })
mcpd.feedback({ traceId, kind: 'thumbs', value: 'up' })

// Nothing has left the process until this runs. Call it before a short-lived
// process exits.
await mcpd.shutdown()
```

## Install

```sh
npm install @mcpd/sdk
```

Run a daemon on the same port and read the run back in the browser:

```sh
mcpd serve --data ./mcpd.db
```

A longer, runnable version of the same agent, including the failure path, is in
[`examples/typescript/quickstart.ts`](../../examples/typescript/quickstart.ts).

## What it records

Five signals, all correlated by trace and span id.

```ts
// Spans: the steps of a run.
const run = mcpd.trace('support_agent')          // a root span
const step = run.child('call_model', { kind: 'model_generation' })
step.event('first_token', { kind: 'model_chunk' })  // a point in time
step.setUsage({ inputTokens: 812, outputTokens: 37 })
step.setModelParameters({ temperature: 0, seed: 0, maxTokens: 1024 })
step.setError('the provider timed out', { kind: 'TimeoutError', retryable: true })
step.end()
run.end()

// Logs, attached to whatever span is active.
mcpd.log('warn', 'retrying the tool', { attempt: 2 })

// Metrics, with an exemplar link back to the span that produced them.
mcpd.metric('support.tickets', 1, 'counter', { labels: { queue: 'tier-1' } })

// Machine evaluation, and human evaluation. Kept apart on purpose: they answer
// different questions and must never be averaged together.
mcpd.score({ traceId: run.traceId, scorerId: 'groundedness', value: 0.4, min: 0, max: 1 })
mcpd.feedback({ traceId: run.traceId, kind: 'correction', value: 'the eta was wrong' })
```

Every recording call is a queue append that returns immediately. Nothing here
throws, whatever the daemon is doing.

`setModelParameters` records the sampling settings a call was made with:
`temperature`, `topP`, `topK`, `maxTokens`, `seed`, `stopSequences` and
`hasResponseFormat`. They are settings, not payload, so they are kept even when
`capturePayloads` is off, and they are what somebody reproducing the run has to
match. A field you leave out is absent; a field you pass is kept whatever it is,
so `seed: 0` is a seed. A second call replaces the first, because one set of
settings describes one model call.

## Correlation

Five fields say who a run was for: session, thread, user, request and release.
Set them on the run, once. Every step started afterwards inherits them, and so
does every log, metric, score and piece of feedback recorded while the run is
active. That is what makes a session filter return the whole run rather than
its first span, and what makes erasure by subject reach every row the run
wrote.

```ts
const run = mcpd.trace('support_agent', {
  correlation: {
    sessionId: 'sess-4f21',
    threadId: 'thread-7',
    userId: 'user-41',
    requestId: 'req-9001',
    release: '2026.08.1',
  },
})

// Started after the run was tagged, so it carries all five.
run.child('lookup_order', { kind: 'tool_call' }).end()

mcpd.withActiveSpan(run, () => mcpd.log('warn', 'the shipping service is slow'))
run.end()
```

`span.setCorrelation({ ... })` tags a run that has already started, and a child
may override one field without losing the rest, which is how a sub-agent gets
its own session and keeps the user it is acting for. The user id is hashed at
ingest, so mcpd stores a digest and never the value you sent.

## Context

On Node and Bun the active span lives in an `AsyncLocalStorage`, so anything
started inside a `withSpan` callback nests under it without being handed a
reference, and two runs interleaved on one event loop never see each other's
spans.

```ts
await mcpd.withSpan('outer', async () => {
  await mcpd.withSpan('inner', async () => {
    mcpd.log('info', 'this line belongs to inner')
  })
})
```

Browsers and most edge runtimes have no `AsyncLocalStorage`. There the SDK
keeps a single saved value instead, which nests correctly and survives `await`
within one run, but cannot tell two concurrent runs apart. When you run agents
concurrently on those runtimes, pass the parent explicitly:

```ts
const run = mcpd.trace('support_agent')
const step = run.child('call_model')       // explicit, correct everywhere
await run.withChild('call_tool', async (tool) => { /* ... */ })
```

`mcpd.stats().context` reports which mechanism is in use.

## Trace context across services

```ts
// Outbound: put this on the request.
await fetch(url, { headers: { traceparent: span.traceparent() } })

// Inbound: continue that trace instead of starting a new one.
const run = mcpd.trace('handle_request', { traceparent: request.headers.get('traceparent') })
```

A malformed header is ignored and a fresh root trace is started.

## Configuration

Every field is settable in code and by environment variable. Code wins.

| Option | Environment variable | Default |
| --- | --- | --- |
| `endpoint` | `MCPD_ENDPOINT` | `http://localhost:4317` |
| `apiKey` | `MCPD_API_KEY` | none |
| `project` | `MCPD_PROJECT` | `default` |
| `environment` | `MCPD_ENVIRONMENT` | none |
| `serviceName` | `MCPD_SERVICE_NAME` | none |
| `serviceVersion` | `MCPD_SERVICE_VERSION` | none |
| `sample` | `MCPD_SAMPLE` | `1.0` |
| `enabled` | `MCPD_ENABLED` | `true` |
| `capturePayloads` | `MCPD_CAPTURE_PAYLOADS` | `true` |
| `logLevel` | `MCPD_LOG_LEVEL` | `debug` |
| `batchSize` | `MCPD_BATCH_SIZE` | `128` |
| `flushIntervalMs` | `MCPD_FLUSH_INTERVAL_MS` | `2000` |
| `maxQueue` | `MCPD_MAX_QUEUE` | `8192` |
| `timeoutMs` | `MCPD_TIMEOUT_MS` | `10000` |
| `sink` | | the HTTP transport |

`enabled: false` records nothing and sends nothing, while every call still
returns a valid object and the list reads return an empty page. A test suite can
switch mcpd off without changing a line of instrumentation.

`capturePayloads: false` keeps prompts and completions in the process. A span is
still recorded with its timing, status, usage, model, model parameters,
correlation and errors;
`input` and `output` are dropped before they reach the queue, so nothing
sensitive can leave on a flush. A span overrides it for itself with
`capturePayloads` in its start options, in either direction.

## Assert what you recorded

A `RecordingSink` replaces the network with memory, so a test reads back the
spans, logs, metrics, scores and feedback the code under test produced, with no
server and no port:

```ts
import { Mcpd, RecordingSink } from '@mcpd/sdk'

const sink = new RecordingSink()
const mcpd = new Mcpd({ sink })

await answerQuestion(mcpd)
await mcpd.flush()

expect(sink.spans.map((s) => s.name)).toEqual(['answer_question', 'model_call'])
expect(sink.scores[0]?.scorerId).toBe('answer_relevance')
```

A `ConsoleSink` prints one JSON line per signal to standard error instead, and
any object with an `export(batch, ingestKey)` method is a sink. A sink that
throws is a failed batch and a diagnostic, never an exception in your code.
`mcpd.stats()` answers `queued`, `dropped`, `capacity`, `exported`, `failed`
and `rejected`.

## Record a finished span

`mcpd.record(span)` queues a span whose ids, parent and times you already hold:
what an adapter for another tracing system, a batch import or a replay has in
hand. It is queued as given, never re-parented, re-timed or re-sampled, with
the correlation it left empty filled from the client. A record whose ids are
not OpenTelemetry-shaped, whose name is empty or whose start time is missing is
dropped and counted in `stats().rejected`. On any span,
`externalParentSpanId` carries a parent from a tracing system that is not mcpd,
for correlation only.

## Reading it back

The same client reads. These methods mirror the HTTP API one for one and return
the generated types.

```ts
// The query that finds bad answers.
const suspect = await mcpd.listTraces({
  hasScore: 'answer_relevance',
  scoreBelow: 0.5,
  from: new Date(Date.now() - 86_400_000).toISOString(),
})

const detail = await mcpd.getTrace(suspect.traces[0].traceId, { include: ['logs', 'scores'] })
for (const span of detail.spans) {
  console.log(span.kind, span.name, span.durationNs)
}

await mcpd.listSpans({ kind: 'tool_call', status: 'error' })
await mcpd.listLogs({ level: 'error' })
await mcpd.listScores({ scorerId: 'answer_relevance' })
await mcpd.listFeedback({ traceId: detail.trace.traceId })
await mcpd.queryMetrics({ name: 'support.tickets', from, to, agg: 'sum' })
await mcpd.analyticsSummary({ from, to, groupBy: 'model' })
await mcpd.analyticsCost({ from, to, groupBy: 'provider' })
await mcpd.health()
```

Reads throw an `McpdError` when they fail. A query you asked for and are waiting
on is not something to swallow.

## Durable runs

The same client drives the daemon's durable runs. A workflow is written once, in
code: the graph goes to the daemon as a definition, and the step bodies stay in
this process for a `Worker` to serve. The daemon holds the cursor, the retries,
the timers and the waits, so a run survives a deploy, a crash and a week-long
approval. No model, prompt or credential ever reaches the daemon.

```ts
import { Mcpd, RetryableError, Worker, when, workflow } from '@mcpd/sdk'

const mcpd = new Mcpd({ project: 'support' })

const support = workflow({ id: 'support_ticket' })
  .step('classify', async (ctx, input: { ticket: string }) => {
    // Anything: your own model call, a database, an HTTP request.
    return { kind: input.ticket.startsWith('Q') ? 'question' : 'bug' }
  })
  .agent(
    'draft',
    async (ctx, input: { kind: string }) => {
      // Your own agent loop, as one durable step. Spans recorded through the
      // context land under the step in the run's trace, and their usage adds
      // up into the step's, which the daemon prices.
      return await ctx.withSpan('model_call', async (span) => {
        span.setModel('claude-sonnet-4', 'anthropic')
        span.setUsage({ inputTokens: 812, outputTokens: 37 })
        return `an answer to a ${input.kind}`
      })
    },
    {
      input: { kind: '${steps.classify.output.kind}' },
      retry: { maxAttempts: 3, backoff: 'exponential', initialMs: 500 },
    },
  )
  .waitFor('approval', {
    event: 'approved',
    key: '${input.ticket}',
    timeoutMs: 7 * 24 * 3_600_000,
    when: when('steps.classify.output.kind', 'eq', 'bug'),
  })
  .step('send', async (ctx, input: { text: string }) => {
    if (ctx.signal.aborted) return
    const ok = await deliver(input.text, ctx.idempotencyKey)
    if (!ok) throw new RetryableError('mail relay busy', { retryAfterMs: 2_000 })
    return { sent: true }
  }, { input: { text: '${steps.draft.output}' }, effect: 'at_most_once' })
  .output({ sent: '${steps.send.output.sent}' })

// Serve step bodies for as long as this process lives.
const worker = new Worker(mcpd, { concurrency: 8 }).register(support)
void worker.serve()

// Start a run from anywhere that holds an execute key.
const run = await mcpd.runs.start({ definitionId: 'support_ticket', input: { ticket: 'Q-42' } })

// Answer the wait from a webhook or a console. A repeated event id is a duplicate.
await mcpd.events.publish({ event: 'approved', key: 'Q-42', payload: { by: 'ops' }, eventId: 'hook-1' })

// Follow the journal to the end, then read the run.
for await (const event of mcpd.runs.watch(run.runId)) console.log(event.seq, event.kind)
const detail = await mcpd.runs.get(run.runId)
console.log(detail.run.status, detail.run.output)
```

What the pieces do:

- **`workflow()`** builds the definition. `step` and `agent` are claimable and
  carry a body; `sleep`, `sleepUntil` and `waitFor` are held by the daemon with
  no process alive; `parallel`, `foreach` and `loop` fan out one claimable step
  per branch, item or iteration; `subworkflow` starts a child run. `input`
  templates and `when` guards read `input`, `steps.<id>` and `run` with
  `${path}` references the daemon renders per attempt.
- **`Worker`** registers the definitions, long-polls for leases and serves each
  one: `started`, heartbeats while the body runs, then `complete` or `fail`. The
  four conflicts the daemon can answer are handled by reason, so a call that
  landed twice is a success and a fenced lease is dropped, never reported as a
  failure. `worker.claim(max, runId)` serves one run from a script.
- **`StepContext`** carries the lease, `state`, `setState` to merge keys into the
  run's state, `withSpan` to nest spans under the step, `idempotencyKey` for a
  third party that accepts one, and `signal`, which aborts when the lease is
  lost or the run ends.
- **Usage adds up.** A span recorded through the context, and every span nested
  under it, adds its usage to the step's when it ends: tokens are summed and
  the last model seen wins, so a body that records its model calls as spans
  never sums tokens by hand. `ctx.setUsage` is an explicit override: the fields
  it sets replace the accumulated ones.
- **Hooks and stats.** `onLease`, `onStepStart`, `onStepEnd` and `onConflict`
  sit beside `onError`, each wrapped so a hook of yours cannot break the serve
  loop. `worker.stats()` counts claims, completions, failures by class,
  conflicts by reason and unservable leases, so a fenced lease abandoned in
  silence is visible from inside the process.
- **Throwing** a `RetryableError` retries within the step's budget. A
  `StepFailure` names its own class. Any other throw is `fatal`, and the step's
  `onError` decides whether the run fails or moves on.
- **`mcpd.runs`**, **`mcpd.definitions`**, **`mcpd.events`** and
  **`mcpd.steps`** mirror the HTTP run surface one for one. `runs.wait` and
  `runs.startAndWait` follow a run to its end and hand back its output, or
  throw `RunFailedError` carrying the run. A 409 from a worker call is thrown
  as `StepConflictError` with the daemon's reason.
- **A lease's deadline** is enforced by the worker itself: a body that runs
  past `deadline_ms` is aborted through `ctx.signal` and the attempt fails
  `retryable`. `ctx.input(decode)`, `ctx.prior(decode)` and
  `ctx.stateAs(decode)` check a value with a decoder of your own, and a value
  the decoder refuses fails the step `invalid_input`.

The wire protocol, the definition document and every closed enum are in
[`spec/definition.schema.json`](../../spec/definition.schema.json) and the
[run contract](../../internal/run-contract-v1.md).

## What you can rely on

- **Instrumentation never throws.** A failed export, an unreachable daemon or a
  payload that will not serialise is reported at most once per interval through
  `onError` and then let go.
- **Recording does not block.** It appends to a queue. The network happens on a
  background worker.
- **Memory is bounded.** The queue holds `maxQueue` signals. When it is full the
  oldest is dropped and counted, which is a smaller failure than an
  out-of-memory kill.
- **The sampling decision is made once, at the root, and inherited.** A trace is
  never half-recorded.
- **`flush()` and `shutdown()` are awaitable, idempotent and bounded in time.**
  `shutdown()` flushes, stops the worker, and is safe to call twice. Both
  return within `timeoutMs`, 10 seconds by default, even when the daemon never
  answers; `flush()` resolves `false` when the deadline passed first.
- **Ids are OpenTelemetry-shaped.** 32 lowercase hex for a trace, 16 for a span,
  never all zero.
- **An open span is still sent.** A span is queued when it starts and again when
  it ends, so a run that crashes mid-flight is still visible. If the start
  record has not left yet, the end record replaces it, so a fast span costs one
  record and not two.
- **Timestamps are UTC RFC 3339 with nanosecond digits.**
- **Recording an error sets the status to `error` and requires a message.**

## Payloads are never rewritten

The daemon speaks `snake_case` and this SDK speaks `camelCase`, so keys are
converted on the wire. Conversion stops at the free-form fields: `input`,
`output`, `attributes`, `metadata`, `fields`, `labels` and their kin are sent
exactly as you wrote them. Your prompt is the most valuable thing mcpd stores
and nothing here edits it.

## Batches, gzip and retries

Batches go to `POST {endpoint}/v1/ingest`, gzipped above 1 KiB where `node:zlib`
is available. Each batch carries an idempotency key that stays the same across
retries, so a timeout after the daemon already stored the batch cannot
double-count it. Retries happen on 429, 502, 503, 504 and on a connection error:
three attempts, exponential backoff from 250 ms with jitter, honouring
`Retry-After`. Any other 4xx is not retried, because the batch is malformed and
will stay malformed.

## Runtimes

Node 20 or later, Deno, Bun, browsers and edge runtimes. ESM first, with a
CommonJS build for `require`. The only Node-specific pieces, `async_hooks` and
`zlib`, are loaded at runtime and skipped where they do not exist.

## Types

The data model in `src/generated.ts` is generated from `spec/mcpd.spec.json`.
Do not edit it: change the spec and run `make gen`.

## Development

```sh
npm install
npm run typecheck
npm test
npm run build

# The integration tests need a daemon and are skipped without one.
mcpd serve --data /tmp/sdk-live.db --listen 127.0.0.1:47401
MCPD_TEST_ENDPOINT=http://127.0.0.1:47401 npm test
```
