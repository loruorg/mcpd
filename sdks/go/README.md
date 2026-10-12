# mcpd for Go

Agent observability and durable runs for Go programs: agent runs, model calls,
tool calls, logs, metrics, scores and human feedback, recorded to a local
`mcpd` daemon, and workflows served from your own process. The daemon owns the
run between steps; your code owns everything inside one.

```go
package main

import (
	"context"
	"errors"
	"fmt"

	mcpd "github.com/mcpd/mcpd/sdks/go"
)

func main() {
	ctx := context.Background()

	m := mcpd.New(
		mcpd.WithProject("support"),
		mcpd.WithService("support-agent", "1.4.2"),
	)
	defer m.Shutdown(ctx)

	ctx, run := m.Start(ctx, "support_agent",
		mcpd.WithSpanKind(mcpd.SpanKindAgentRun),
		mcpd.WithSession("sess-4821"),
		mcpd.WithUser("u-317"),
		mcpd.WithInput("My invoice for March charged me twice."),
	)

	_, call := m.Start(ctx, "draft_answer",
		mcpd.WithSpanKind(mcpd.SpanKindModelGeneration),
		mcpd.WithModel("gpt-4o-mini", "openai"),
	)
	answer := "You were charged twice on 3 March. The duplicate is refunded today."
	call.SetOutput(answer)
	call.SetTokens(812, 96)
	call.End()

	if err := errors.New("smtp: 451 mailbox busy"); err != nil {
		run.RecordError(err, mcpd.ErrorRetryable(true))
	}
	run.SetOutput(answer)
	run.End()

	m.Score(ctx, mcpd.TraceTarget(run.TraceID()), "answer_relevance", 0.86, 0, 1,
		mcpd.ScoreReason("Answered the billing question and cited the invoice."))
	m.Feedback(ctx, mcpd.TraceTarget(run.TraceID()), mcpd.FeedbackKindThumbs, "up")

	if err := m.Flush(ctx); err != nil {
		fmt.Println("flush:", err)
	}
	detail, err := m.GetTrace(ctx, run.TraceID(), "scores")
	if err != nil {
		fmt.Println("read back:", err)
		return
	}
	fmt.Printf("%s: %s, %d spans, %d scores\n",
		detail.Trace.TraceID, detail.Trace.Status, len(detail.Spans), len(detail.Scores))
}
```

Install:

```
go get github.com/mcpd/mcpd/sdks/go
```

Requires Go 1.21 and no third-party dependencies.

A longer, runnable version of the same agent is in
[`examples/go/quickstart.go`](../../examples/go/quickstart.go).

## What it guarantees

- **Instrumentation never fails your program.** No method that records anything
  returns an error or panics, including on a nil client. A failed export is
  logged at most once per flush interval and dropped.
- **Recording does not block.** `Start` and `End` append to an in-memory queue
  and return; a background goroutine batches and sends.
- **Memory is bounded.** The queue holds `WithMaxQueue` signals. When it is
  full the oldest are dropped and counted, visible in `Stats()`.
- **The sampling decision is made once, at the root of a trace,** by hashing the
  trace id, and inherited by every child. A trace is never half-recorded, and
  every process in a fleet makes the same decision.
- **Ids are OpenTelemetry-shaped** and W3C `traceparent` is read on the way in
  and written on the way out, so a trace survives a service boundary.
- **An unfinished span is still exported.** Long-running spans go out when they
  start and are updated when they end, so a run that crashes mid-flight is
  visible. A span that ends before the next flush is sent once, finished.

## Configuration

Every setting has an option and an environment variable. Code wins.

| Option | Environment variable | Default |
| --- | --- | --- |
| `WithEndpoint` | `MCPD_ENDPOINT` | `http://localhost:4317` |
| `WithAPIKey` | `MCPD_API_KEY` | none |
| `WithProject` | `MCPD_PROJECT` | `default` |
| `WithEnvironment` | `MCPD_ENVIRONMENT` | none |
| `WithService` | `MCPD_SERVICE_NAME`, `MCPD_SERVICE_VERSION` | none |
| `WithSampleRate` | `MCPD_SAMPLE` | `1.0` |
| `WithBatchSize` | `MCPD_BATCH_SIZE` | `128` |
| `WithFlushInterval` | `MCPD_FLUSH_INTERVAL_MS` | `2s` |
| `WithMaxQueue` | `MCPD_MAX_QUEUE` | `8192` |
| `WithTimeout` | `MCPD_TIMEOUT_MS` | `10s` |
| `WithEnabled` | `MCPD_ENABLED` | `true` |
| `WithCapturePayloads` | `MCPD_CAPTURE_PAYLOADS` | `true` |
| `WithLogLevel` | `MCPD_LOG_LEVEL` | `debug` |

`mcpd.New()` with no options at all works against a local daemon.

`WithEnabled(false)` turns recording into a no-op that still returns valid spans
and ids, so tests can disable mcpd without changing a call site. The read
methods keep working, because turning instrumentation off should not quietly
turn a query into an empty answer.

`WithCapturePayloads(false)` keeps prompts and completions in the process.
Spans are still recorded with their timing, status, usage, model, model
parameters, correlation and errors; `input` and `output` are dropped before they reach the queue, so
nothing sensitive is ever queued or exported. One span can say otherwise for
itself with `mcpd.WithSpanCapturePayloads(true)`, and the reverse on a client
that captures. Server-side redaction is the other end of the same wire, and the
wrong end for a team that must not send the bytes at all.

`WithSink` replaces where batches go; see [sinks and stats](#sinks-and-stats).

## Tracing

```go
ctx, span := m.Start(ctx, "retrieve_docs", mcpd.WithSpanKind(mcpd.SpanKindRetrieval))
defer span.End()
```

`Start` makes a child of whatever span is in `ctx`, or a root when there is
none. `Trace` always starts a root. `span.Child(ctx, name)` starts a child of a
specific span when you are not carrying it in a context.

On a span: `SetInput`, `SetOutput`, `SetUsage`, `SetTokens`, `SetModel`,
`SetModelParameters`, `SetMetadata`, `SetAttributes`, `SetTags`, `SetStatus`,
`SetSession`, `SetThread`, `SetUser`, `RecordError`, `Event`, `End`,
`EndWithStatus`.

`SetModelParameters` records the sampling settings a call was made with:

```go
temperature, seed, maxTokens := 0.0, int64(0), uint64(1024)
span.SetModelParameters(mcpd.ModelParameters{
	Temperature: &temperature,
	Seed:        &seed,
	MaxTokens:   &maxTokens,
})
```

They are settings, not payload, so they are kept even when payload capture is
off, and they are what somebody reproducing the run has to match. A nil field
is absent; a pointer to zero is a value, so `Seed` set to zero is a seed. A
second call replaces the first, because one set of settings describes one model
call.

`RecordError` takes an `error`, so a recorded failure always has a message:

```go
span.RecordError(err,
	mcpd.ErrorKind("RateLimitError"),
	mcpd.ErrorRetryable(true),
	mcpd.ErrorStack(mcpd.Stack()),
)
```

`End` is idempotent and sets `ok`, or `error` when an error was recorded.

`mcpd.WithExternalParent(spanID)` carries the parent from a tracing system that
is not mcpd, such as the OpenTelemetry span the request was already inside. It
is stored for correlation and never used to parent the span in mcpd's own tree.

`m.Record(span)` queues a `Span` that already ended, ids, parent and times
included: what an adapter for another tracer, a batch import or a replay holds.
The record goes out as given, never re-parented, re-timed or re-sampled;
correlation fields it leaves empty are filled from the client. A record with
ids that are not OpenTelemetry-shaped, an empty name or no start time is
refused: dropped, counted in `Stats().Rejected` and logged, never returned as
an error.

## Sinks and stats

The exporter drains the queue into a batch and hands it to the client's sink.
The HTTP transport is the default; two more ship with the SDK:

```go
sink := &mcpd.RecordingSink{}
m := mcpd.New(mcpd.WithSink(sink))

ctx, span := m.Start(ctx, "support_agent")
span.SetTokens(812, 96)
span.End()
m.Flush(ctx)

spans := sink.Spans() // and Logs, Metrics, Scores, Feedback, Batches, Clear
```

`RecordingSink` is how a test asserts what the instrumentation recorded without
a server. `ConsoleSink` writes one JSON line per signal, shaped
`{"signal":"span",...}` with the wire's field names, to standard error or the
`Writer` you give it. A sink runs on the exporter's goroutine inside the same
guard as the transport: one that returns an error or panics is counted as a
failed batch and logged, and never reaches the code that recorded the signals.

`m.Stats()` answers `Queued`, `Dropped`, `Capacity`, `Exported`, `Failed` and
`Rejected`: what is waiting, what the bound evicted, how much the queue holds,
what the sink accepted, the batches it refused and the spans `Record` refused.

## The other four signals

```go
m.Log(ctx, mcpd.LogLevelInfo, "ticket routed", map[string]any{"queue": "billing"})
m.Metric(ctx, "support.tickets", 1, mcpd.MetricKindCounter, mcpd.MetricUnit("{ticket}"))
m.Score(ctx, mcpd.Target{}, "helpfulness", 0.86, 0, 1, mcpd.ScoreReason("cited the invoice"))
m.Feedback(ctx, mcpd.Target{}, mcpd.FeedbackKindThumbs, "up", mcpd.FeedbackComment("fast"))
```

All four correlate to the active span in `ctx`. `Score` requires `min` and
`max` at the call site: a bare number with no declared scale cannot be compared
to anything.

## Reading data back

The read methods mirror the HTTP API and return the generated types. They return
errors, because the caller is waiting for an answer.

```go
traces, err := m.ListTraces(ctx, mcpd.TraceQuery{
	Status:     mcpd.SpanStatusError,
	HasScore:   "answer_relevance",
	ScoreBelow: &threshold,
	Limit:      50,
})
detail, err := m.GetTrace(ctx, traceID, "logs", "scores")
summary, err := m.AnalyticsSummary(ctx, from, to, "model")
cost, err := m.AnalyticsCost(ctx, from, to, "provider", "1h")
```

Also `ListSpans`, `ListLogs`, `QueryMetrics`, `ListScores`, `ListFeedback`,
`DeleteTrace` and `Health`.

## Durable runs

The same client drives the daemon's durable runs. A workflow is written once, in
code: the graph goes to the daemon as a definition, and the step bodies stay in
this process for a `Worker` to serve. The daemon holds the cursor, the retries,
the timers and the waits, so a run survives a deploy, a crash and a week-long
approval. No model, prompt or credential ever reaches the daemon.

```go
m := mcpd.New(mcpd.WithProject("support"))

support := mcpd.NewWorkflow("support_ticket").
	Step("classify", func(ctx context.Context, step *mcpd.StepContext) (any, error) {
		var in struct{ Ticket string `json:"ticket"` }
		if err := step.Input(&in); err != nil {
			return nil, err
		}
		// Anything: your own model call, a database, an HTTP request.
		kind := "bug"
		if strings.HasPrefix(in.Ticket, "Q") {
			kind = "question"
		}
		return map[string]any{"kind": kind}, nil
	}).
	Agent("draft", func(ctx context.Context, step *mcpd.StepContext) (any, error) {
		// Your own agent loop, as one durable step. A span started from ctx
		// lands under the step in the run's trace.
		_, span := m.Start(ctx, "model_call", mcpd.WithModel("claude-sonnet-4", "anthropic"))
		defer span.End()
		span.SetTokens(812, 37) // the step's usage: the daemon prices the sum
		return "an answer", nil
	},
		mcpd.Input(map[string]any{"kind": "${steps.classify.output.kind}"}),
		mcpd.Retry(mcpd.RetryPolicy{MaxAttempts: 3, Backoff: mcpd.BackoffExponential, Initial: 500 * time.Millisecond}),
	).
	WaitFor("approval", "approved", "${input.ticket}", 7*24*time.Hour,
		mcpd.Guard(mcpd.When("steps.classify.output.kind", "eq", "bug"))).
	Step("send", func(ctx context.Context, step *mcpd.StepContext) (any, error) {
		if err := deliver(ctx, step.IdempotencyKey); err != nil {
			return nil, mcpd.Retryable(err, 2*time.Second)
		}
		return map[string]any{"sent": true}, nil
	}, mcpd.Input(map[string]any{"text": "${steps.draft.output}"}), mcpd.AtMostOnce()).
	Output(map[string]any{"sent": "${steps.send.output.sent}"})

// Serve step bodies for as long as this process lives.
worker := mcpd.NewWorker(m, mcpd.WorkerConcurrency(8)).Register(support)
go worker.Serve(ctx)

// Start a run from anywhere that holds an execute key.
run, err := m.StartRun(ctx, mcpd.StartRun{Definition: "support_ticket", Input: map[string]any{"ticket": "Q-42"}})

// Answer the wait from a webhook or a console. A repeated event id is a duplicate.
_, err = m.PublishEvent(ctx, mcpd.PublishEvent{Event: "approved", Key: "Q-42", Payload: map[string]any{"by": "ops"}, EventID: "hook-1"})

// Follow the journal to the end, then read the run.
err = m.WatchRun(ctx, run.RunID, -1, func(e mcpd.RunEvent) error { fmt.Println(e.Seq, e.Kind); return nil })
detail, err := m.GetRun(ctx, run.RunID)
fmt.Println(detail.Run.Status, string(detail.Run.Output))
```

What the pieces do:

- **`NewWorkflow`** builds the definition. `Step` and `Agent` are claimable and
  carry a body; `Sleep`, `SleepUntil` and `WaitFor` are held by the daemon
  with no process alive; `Parallel`, `ForEach` and `Loop` fan out one
  claimable step per branch, item or iteration; `Subworkflow` starts a child
  run. `Input` templates and `Guard` predicates read `input`, `steps.<id>` and
  `run` with `${path}` references the daemon renders per attempt. A mistake
  such as an id the daemon would refuse comes back from `Document`, so the
  chain reads without error checks at every step.
- **`Worker`** registers the definitions, long-polls for leases and serves
  each one on a goroutine: `started`, heartbeats while the body runs, then
  `complete` or `fail`. The four conflicts the daemon can answer are handled
  by reason, so a call that landed twice is a success and a fenced lease is
  dropped, never reported as a failure. `Claim(ctx, n, runID)` serves one run
  from a script. A body that panics fails its step; the worker lives on.
- **`StepContext`** carries the lease, `Input`, `Prior` and `State` decoders,
  `SetState` to merge keys into the run's state, and `IdempotencyKey` for a
  third party that accepts one. The handler's ctx is cancelled when the lease
  is lost or the run ends, and `context.Cause` says why.
- **Usage adds up on its own.** Every span started from the handler's ctx, or
  from `step.Start`, and every span nested under one adds its usage to the
  step's when it ends: the counts are summed and the last model and provider
  seen win. A body that records its model calls as spans never sums tokens by
  hand, and `max_cost_usd` fires on what the trace shows. `SetUsage` and
  `SetTokens` are the explicit override: the fields they set replace the sum.
- **A worker can be watched from inside.** `WorkerOnLease`,
  `WorkerOnStepStart`, `WorkerOnStepEnd` and `WorkerOnConflict` sit beside
  `WorkerOnError`; a hook that panics is reported there and the loop goes on.
  `worker.Stats()` answers `Claimed`, `Inflight`, `Completed`, `Failed` by
  class, `Conflicts` by reason and `Unservable`, since a fenced lease abandoned
  in silence looks like no work at all from outside.
- **Returning** `mcpd.Retryable(err, wait)` retries within the step's budget.
  A `*mcpd.Failure` names its own class. Any other error is `fatal`, and the
  step's `OnFailure` option decides whether the run fails or moves on.
- **`StartRun`**, **`GetRun`**, **`ListRuns`**, **`CancelRun`**,
  **`RunEvents`**, **`WatchRun`**, **`SignalRun`**, **`PublishEvent`**,
  **`PutDefinition`**, **`GetDefinition`**, **`ListDefinitions`** and the
  worker calls mirror the HTTP run surface one for one. `WaitRun` and
  `StartRunAndWait` follow a run to its end and hand back its output, or a
  `*RunFailedError` carrying the run. A 409 from a worker call is a
  `*StepConflictError` with the daemon's reason; `errors.As` finds it.
- **A lease's deadline** is enforced by the worker itself: a body that runs
  past `deadline_ms` sees its ctx cancelled and the attempt fails `retryable`.
  A value `Input`, `Prior` or `State` cannot decode fails the step
  `invalid_input`.

The wire protocol, the definition document and every closed enum are in
[`spec/definition.schema.json`](../../spec/definition.schema.json) and the
[run contract](../../internal/run-contract-v1.md).

## Across a service boundary

```go
// Outbound.
req, _ := http.NewRequestWithContext(ctx, http.MethodPost, url, body)
mcpd.Inject(ctx, req.Header)

// Inbound.
func handler(w http.ResponseWriter, r *http.Request) {
	ctx, span := m.Start(mcpd.Extract(r.Context(), r.Header), "handle_request")
	defer span.End()
}
```

The upstream sampling decision is inherited, so the two halves of a trace agree.

## Shutting down

```go
defer m.Shutdown(ctx)
```

`Shutdown` flushes, stops the worker and is safe to call twice. A short-lived
process that skips it loses whatever had not been exported. `Flush(ctx)` sends
what is queued without stopping anything.

Both stop waiting at the context's deadline, or after `DefaultDeadline`, 10
seconds, when the context has none, even when the daemon never answers. A
flush that runs out of time returns the context's error and leaves the rest
queued. A shutdown that runs out of time starts no further batch and gives up
what is still queued.

## Types

Everything in `generated.go` is produced from `spec/mcpd.spec.json` by
`tools/codegen`. Do not edit it: change the spec and run `make gen`.

## Tests

```
go test ./...
```
