# mcpd for Kotlin

Agent observability for JVM services. Coroutine-first, no framework, and no
dependency on mcpd being up: recording a span appends to a bounded in-memory
queue and returns, and a background worker batches and sends.

```kotlin
import dev.mcpd.FeedbackKind
import dev.mcpd.Mcpd
import dev.mcpd.McpdConfig
import dev.mcpd.SpanKind
import kotlinx.coroutines.runBlocking

fun main() = runBlocking {
    val mcpd = Mcpd(McpdConfig(project = "support-desk", environment = "development"))

    val traceId = mcpd.span("support_agent", SpanKind.AGENT_RUN, input = "where is my order?") { run ->
        val answer = mcpd.span("plan_answer", SpanKind.MODEL_GENERATION) { generation ->
            generation.setModel("gpt-4o", provider = "openai")
            generation.setInput(listOf(mapOf("role" to "user", "content" to "where is my order?")))
            generation.setUsage(inputTokens = 1180, outputTokens = 96)
            "It shipped on Tuesday."
        }
        run.setOutput(answer)
        run.traceId
    }

    mcpd.score(traceId, scorerId = "answer_relevance", value = 0.86, min = 0.0, max = 1.0)
    mcpd.feedback(traceId, FeedbackKind.THUMBS, "up")
    mcpd.flush()

    val trace = mcpd.getTrace(traceId, include = listOf("scores"))
    println("${trace.trace.name}: ${trace.spans.size} spans, ${trace.trace.status}")

    mcpd.shutdown()
}
```

Point it at a running mcpd:

```
mcpd serve --data ./mcpd.db
```

## Install

Built with Kotlin 2.4, for JVM 17 or later.

```kotlin
dependencies {
    implementation("dev.mcpd:mcpd-kotlin:0.1.0")
}
```

The only dependencies are kotlinx coroutines, serialization and datetime. HTTP
is the JDK's own client, so nothing is added to a service that already has an
opinion about HTTP.

## Configuration

Every field reads an environment variable as its default and is settable in
code, so code wins. `Mcpd()` with no arguments works against a local mcpd.

| Field | Environment variable | Default |
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
| `flushInterval` | `MCPD_FLUSH_INTERVAL_MS` | `2s` |
| `maxQueue` | `MCPD_MAX_QUEUE` | `8192` |
| `timeout` | `MCPD_TIMEOUT_MS` | `10s` |

`enabled = false` makes every recording call a no-op that still returns valid
spans, so mcpd can be turned off in tests without touching the code under test.
Reads still go to the server: a query is something you are waiting on, and an
empty answer would be a lie.

`capturePayloads = false` keeps prompts and completions in the process. A span
is still recorded with its timing, status, usage, model, model parameters,
correlation and errors; `input` and `output` are dropped before they reach the queue, so
nothing sensitive is queued and nothing sensitive leaves on a flush. One span
can override it for itself, in either direction, with `capturePayloads` as it
starts. Redaction on the daemon is the other end of the same wire, and the
wrong end for a team that must not send the bytes at all.

## Tracing

`span` is the idiomatic form. It ends the span, records what the block threw,
and makes the span active for everything the block calls.

```kotlin
mcpd.span("retrieve_context", SpanKind.RETRIEVAL) { span ->
    span.setInput(query)
    val docs = index.search(query)
    span.setOutput(docs.map { it.id })
    span.setAttribute("retrieval.hits", docs.size)
    docs
}
```

Propagation uses coroutine context, so a span started here is the parent of
anything started inside the block, however many suspend calls down. Read it
with `mcpd.currentSpan()`.

When a block is not the right shape, start and end the span yourself:

```kotlin
val run = mcpd.trace("nightly_reindex", SpanKind.WORKFLOW_RUN, tags = listOf("batch"))
try {
    reindex()
    run.end()
} catch (e: Exception) {
    run.setError(e)
    run.end()
}
```

`run.child(...)` starts a child, `run.event(...)` records a point in time, and
`run.setInput`, `setOutput`, `setUsage`, `setModel`, `setModelParameters`,
`setError`, `setMetadata`, `setTags` and `setAttribute` fill it in. Every one of
them is safe to call from any thread and none of them throw.

`setModelParameters` records the sampling settings a call was made with:

```kotlin
call.setModelParameters(temperature = 0.0, seed = 0, maxTokens = 1024)
```

They are settings, not payload, so they are kept even when `capturePayloads` is
off, and they are what somebody reproducing the run has to match. An argument
you leave out is absent; one you pass is kept whatever it is, so `seed = 0` is
a seed. A second call replaces the first, because one set of settings describes
one model call.

A span that belongs to another tracer's tree names it with
`externalParentSpanId`: carried for correlation, never used as a parent inside
mcpd's own tree. A span another system already finished is handed over whole:

```kotlin
// Queued as given: never re-parented, re-timed or re-sampled. A malformed id
// or an empty name is dropped and counted, never thrown.
mcpd.record(span)
```

### Across a service boundary

```kotlin
// Outgoing: hand the header to the next service.
request.header("traceparent", span.traceparent())

// Incoming: continue the trace instead of starting a new one.
mcpd.span("checkout", traceparent = headers["traceparent"]) { ... }
```

The W3C `traceparent` carries the sampling decision with it, so a trace is
never half recorded.

## Assert what you recorded

The exporter hands every batch to a `Sink`. The HTTP transport is the default
and the only sink that talks to a daemon; the two others keep the batch in the
process, which is how a test proves its instrumentation recorded the span, the
score and the usage it meant to without standing up a server.

```kotlin
val recorder = RecordingSink()
val mcpd = Mcpd(McpdConfig(sink = recorder))

mcpd.span("support_agent") { run -> run.setUsage(inputTokens = 12, outputTokens = 3) }
mcpd.flush()

check(recorder.spans.single().usage?.inputTokens == 12L)
check(recorder.batches == 1)

// One JSON line per signal on standard error, for a look while debugging.
val loud = Mcpd(McpdConfig(sink = ConsoleSink()))
```

A sink that throws is a failed batch, reported once per interval; the caller
recording the signal never hears about it. `mcpd.stats()` answers `queued`,
`dropped`, `capacity`, `exported`, `failed` and `rejected`, the same six names
in every SDK.

## The other four signals

```kotlin
mcpd.log(LogLevel.INFO, "retrieved context", mapOf("hits" to 8))
mcpd.metric("cache.hit_rate", 0.94, MetricKind.GAUGE, labels = mapOf("index" to "faq"))
mcpd.score(traceId, scorerId = "toxicity", value = 0.01, min = 0.0, max = 1.0, reason = "clean")
mcpd.feedback(traceId, FeedbackKind.RATING, 4, comment = "nearly right")
```

Logs and metrics attach to the active span, so a spike on a chart opens the run
that caused it. `score` requires `min` and `max`: 7 is excellent out of 10 and
poor out of 100, and a number with no declared scale cannot be compared to
anything.

## Durable runs

The same client drives the daemon's durable runs. A workflow is written once, in
code: the graph goes to the daemon as a definition, and the step bodies stay in
this process for a `Worker` to serve. The daemon holds the cursor, the retries,
the timers and the waits, so a run survives a deploy, a crash and a week-long
approval. No model, prompt or credential ever reaches the daemon.

```kotlin
val mcpd = Mcpd(McpdConfig(project = "support"))

val support = Workflow("support_ticket")
    .step("classify") { ctx ->
        // Anything: your own model call, a database, an HTTP request.
        val ticket = ctx.input<JsonObject>()["ticket"]!!.jsonPrimitive.content
        mapOf("kind" to if (ticket.startsWith("Q")) "question" else "bug")
    }
    .agent(
        "draft",
        StepOptions(
            input = mapOf("kind" to "\${steps.classify.output.kind}"),
            retry = RetryPolicy(maxAttempts = 3, backoff = Backoff.EXPONENTIAL, initial = 500.milliseconds),
        ),
    ) { ctx ->
        // Your own agent loop, as one durable step. A span from the context
        // lands under the step in the run's trace, and its usage is summed
        // into the step's, so the daemon prices the run without a hand count.
        ctx.span("model_call", SpanKind.MODEL_GENERATION) { span ->
            span.setModel("claude-sonnet-4", "anthropic")
            span.setUsage(inputTokens = 812, outputTokens = 37)
        }
        "an answer"
    }
    .waitFor(
        "approval", "approved", "\${input.ticket}", 7.days,
        StepOptions(guard = predicate("steps.classify.output.kind", "eq", "bug")),
    )
    .step("send", StepOptions(input = mapOf("text" to "\${steps.draft.output}"), effect = Effect.AT_MOST_ONCE)) { ctx ->
        if (!deliver(ctx.input<JsonObject>()["text"]!!.jsonPrimitive.content, ctx.idempotencyKey)) {
            throw RetryableException("mail relay busy", retryAfter = 2.seconds)
        }
        mapOf("sent" to true)
    }
    .output(mapOf("sent" to "\${steps.send.output.sent}"))

// Serve step bodies for as long as this process lives.
val worker = Worker(mcpd, concurrency = 8).register(support)
scope.launch { worker.serve() }

// Start a run from anywhere that holds an execute key.
val run = mcpd.startRun("support_ticket", input = mapOf("ticket" to "Q-42"))

// Answer the wait from a webhook or a console. A repeated event id is a duplicate.
mcpd.publishEvent("approved", "Q-42", payload = mapOf("by" to "ops"), eventId = "hook-1")

// Follow the journal to the end, then read the run.
mcpd.watchRun(run.runId) { event -> println("${'$'}{event.seq} ${'$'}{event.kind}") }
val detail = mcpd.getRun(run.runId)
println("${'$'}{detail.run.status} ${'$'}{detail.run.output}")
```

What the pieces do:

- **`Workflow`** builds the definition. `step` and `agent` are claimable and
  carry a body; `sleep`, `sleepUntil` and `waitFor` are held by the daemon
  with no process alive; `parallel`, `forEach` and `loop` fan out one
  claimable step per branch, item or iteration; `subworkflow` starts a child
  run. `StepOptions` carries the `input` template, the `guard` predicate, the
  effect, the timeout, the retry policy and the rest; `${path}` references
  read `input`, `steps.<id>` and `run` and are rendered per attempt. An id the
  daemon would refuse throws as it is added.
- **`Worker`** registers the definitions, long-polls for leases and serves
  each one on a coroutine: `started`, heartbeats while the body runs, then
  `complete` or `fail`. The four conflicts the daemon can answer are handled
  by reason, so a call that landed twice is a success and a fenced lease is
  dropped, never reported as a failure. `claim(n, runId)` serves one run from
  a script. Four optional hooks sit beside `onError`: `onLease`,
  `onStepStart`, `onStepEnd` (with a `StepOutcome`) and `onConflict` (with the
  daemon's reason); a hook that throws is reported and the loop goes on.
  `worker.stats()` answers `claimed`, `inflight`, `completed`, `failed` by
  class, `conflicts` by reason and `unservable`.
- **`StepContext`** carries the lease, `input`, `prior` and `state` decoders,
  `setState` to merge keys into the run's state, `span` to nest spans under
  the step, `idempotencyKey` for a third party that accepts one, and
  `cancelled`, a deferred that completes when the lease is lost or the run
  ends. Every span opened through `span`, and every span nested under it,
  adds its usage to the step's as it ends, so a body that records its model
  calls never sums tokens by hand; `setUsage` and `setTokens` override what
  the spans summed.
- **Throwing** a `RetryableException` retries within the step's budget. A
  `StepFailureException` names its own class. Any other throw is `fatal`, and
  the step's `onError` decides whether the run fails or moves on.
- **`startRun`**, **`getRun`**, **`listRuns`**, **`cancelRun`**,
  **`runEvents`**, **`watchRun`**, **`signalRun`**, **`publishEvent`**,
  **`putDefinition`**, **`getDefinition`**, **`listDefinitions`** and the
  worker calls mirror the HTTP run surface one for one. `waitRun` and
  `startRunAndWait` follow a run to its end and hand back its output, or
  throw `RunFailedException` carrying the run. A 409 from a worker call is a
  `StepConflictException` with the daemon's reason.
- **A lease's deadline** is enforced by the worker itself: a body that runs
  past `deadline_ms` sees `cancelled` complete and the attempt fails
  `retryable`. A value `input`, `prior` or `state` cannot decode fails the
  step `invalid_input`.

The wire protocol, the definition document and every closed enum are in
[`spec/definition.schema.json`](../../spec/definition.schema.json) and the
[run contract](../../internal/run-contract-v1.md).

## Reading

The read methods mirror the HTTP API one for one and return the generated
types.

```kotlin
val failures = mcpd.listTraces(status = SpanStatus.ERROR, from = Clock.System.now() - 1.hours)
val trace = mcpd.getTrace(failures.traces.first().traceId, include = listOf("logs", "scores"))
val cost = mcpd.analyticsCost(from, to, groupBy = "model")
```

Also `listSpans`, `listLogs`, `queryMetrics`, `listScores`, `listFeedback`,
`analyticsSummary` and `health`. These are the only methods that throw, and
they throw `McpdException`.

## Shutting down

```kotlin
mcpd.flush()     // send everything queued, and wait
mcpd.shutdown()  // flush, stop the worker, safe to call twice
```

`Mcpd` is `AutoCloseable`, so `use { }` and a shutdown hook both work. A
serverless function or a short-lived worker that never calls one of these loses
its last batch.

Both return within `Mcpd.DEFAULT_DEADLINE`, 10 seconds, even when the daemon
never answers, and take a `Duration` for another limit. `flush` returns `false`
when it ran out of time and leaves the rest queued. `shutdown` then starts no
further batch and gives up what is still queued.

## What it guarantees

- Instrumentation never throws. A failed export, an unreachable server or a
  value that would not serialise is reported at most once every 30 seconds
  through `java.util.logging` and otherwise swallowed.
- Memory is bounded. The queue holds `maxQueue` signals; past that the oldest
  are dropped and counted, readable as `droppedSignals` and in `stats()`. An
  unbounded buffer turns a network outage into an out-of-memory kill.
- The sampling decision is made once, at the root, and inherited.
- Ids are OpenTelemetry-shaped: 32 lowercase hex for a trace, 16 for a span,
  never all-zero.
- A span is exported when it starts and again when it ends, so a run that
  crashed mid-flight is still visible. When a span ends before its start record
  has been sent, the record is updated in place and only one is sent.
- Batches are gzipped above 1 KiB and carry an idempotency key that survives a
  retry, so a request that timed out after the server stored it cannot
  double-count. Retries are limited to 429, 502, 503, 504 and connection
  failures: three attempts, exponential backoff with jitter, honouring
  `Retry-After`.
- Timestamps are UTC. Durations are measured from a monotonic clock, so an NTP
  step cannot produce a negative one.

## Tests

```
gradle test
```

The tests stand up a real HTTP server on a loopback port and assert on the
bytes that arrive, so a field that never made it onto the wire fails the test.
They need no network and no running mcpd.

## The example

[`examples/kotlin/Quickstart.kt`](../../examples/kotlin/Quickstart.kt) is a
small support agent with a model call, a tool call, a failing tool call, a
score and a piece of feedback.

```
cd examples/kotlin && gradle run
```

## Types

Everything in `dev.mcpd.Generated.kt` is generated from `spec/mcpd.spec.json`.
Do not edit it; edit the spec and run `make gen`.
