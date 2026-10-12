# mcpd-sdk

The Rust SDK for [mcpd](https://mcpd.sh). Instrument an agent and ship its
spans, logs, metrics, scores and feedback.

```rust
use mcpd_sdk::{FeedbackKind, Mcpd, Scale, SpanKind, Target, TokenUsage};

#[tokio::main]
async fn main() {
    // Reads MCPD_ENDPOINT, MCPD_API_KEY, MCPD_PROJECT and the rest from the
    // environment. With nothing set it talks to a local mcpd.
    let mcpd = Mcpd::new();

    // The root span is the run. The sampling decision is made here, once, and
    // every child inherits it.
    let run = mcpd.trace("support agent");
    run.set_input("where is my order?");
    run.set_session("session-4f21");

    let lookup = run.child_of("lookup_order", SpanKind::ToolCall);
    lookup.set_output(serde_json::json!({ "status": "shipped", "eta": "Tuesday" }));
    lookup.end();

    let call = run.child_of("answer", SpanKind::ModelGeneration);
    call.set_model("claude-sonnet-4", "anthropic");
    call.set_input("Where is order 5512?");
    call.set_output("It shipped and arrives Tuesday.");
    call.set_usage(TokenUsage {
        input_tokens: Some(412),
        output_tokens: Some(96),
        ..TokenUsage::default()
    });
    call.end();

    run.set_output("It shipped and arrives Tuesday.");
    let trace_id = run.trace_id().to_owned();
    run.end();

    // A machine judgement, and a person's. The scale is not optional.
    mcpd.score(Target::trace(&trace_id), "answer_relevance", 0.9, Scale::UNIT, None);
    mcpd.feedback(Target::trace(&trace_id), FeedbackKind::Thumbs, "up", None);

    // Short-lived processes must shut down, or they exit with their spans still
    // in memory.
    if let Err(e) = mcpd.shutdown().await {
        eprintln!("mcpd could not export: {e}");
    }
}
```

Add it to `Cargo.toml`:

```toml
[dependencies]
mcpd-sdk = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Then start a server and open it:

```sh
mcpd demo --listen 127.0.0.1:4317
```

The full worked example, including a tool that fails and a read of the finished
trace, is in [`examples/rust/quickstart.rs`](../../examples/rust/quickstart.rs).

## What it guarantees

- **It never fails your program.** No instrumentation call returns an error or
  panics. An unreachable mcpd, a serialisation failure, a full queue: all are
  logged at most once per flush interval and swallowed.
- **It never blocks your program.** Recording appends to an in-memory queue and
  returns. Export happens on a background tokio task.
- **It never grows without bound.** The queue is capped at `max_queue`. When it
  is full the oldest signals are discarded and `mcpd.stats().dropped` counts
  them.
- **A trace is never half recorded.** The sampling decision is made once, at the
  root, and inherited by every child and by every downstream service.
- **An unfinished span is still sent.** Spans are exported when they start and
  updated when they end, so a run that crashes mid-flight is still readable.

## Configuration

Every field is settable in code and by environment variable. Code set after
`Config::from_env()` wins.

| Field | Environment variable | Default |
| --- | --- | --- |
| `endpoint` | `MCPD_ENDPOINT` | `http://localhost:4317` |
| `api_key` | `MCPD_API_KEY` | none |
| `project` | `MCPD_PROJECT` | `default` |
| `environment` | `MCPD_ENVIRONMENT` | none |
| `service_name` | `MCPD_SERVICE_NAME` | none |
| `service_version` | `MCPD_SERVICE_VERSION` | none |
| `sample` | `MCPD_SAMPLE` | `1.0` |
| `batch_size` | `MCPD_BATCH_SIZE` | `128` |
| `flush_interval` | `MCPD_FLUSH_INTERVAL_MS` | `2s` |
| `max_queue` | `MCPD_MAX_QUEUE` | `8192` |
| `timeout` | `MCPD_TIMEOUT_MS` | `10s` |
| `enabled` | `MCPD_ENABLED` | `true` |
| `capture_payloads` | `MCPD_CAPTURE_PAYLOADS` | `true` |
| `log_level` | `MCPD_LOG_LEVEL` | `debug` |

```rust
use mcpd_sdk::{Config, Mcpd};

let mcpd = Mcpd::with_config(
    Config::from_env()
        .with_project("support")
        .with_environment("production")
        .with_service("checkout", Some("1.4.0"))
        .with_sample(0.1),
);
```

`enabled = false` makes every call a no-op that still returns valid objects, so
a test can turn mcpd off without changing the code under test. Ids are still
well formed and `traceparent` still propagates.

## Keep payloads in the process

`capture_payloads = false` keeps prompts and completions where they are. Spans
still carry their timing, status, usage, model, model parameters, correlation
and errors; `input` and `output` are dropped before they reach the queue, so
nothing sensitive is ever queued or flushed. A span can override the client for itself, in either
direction.

```rust
# use mcpd_sdk::{Config, Mcpd, SpanKind};
let mcpd = Mcpd::with_config(Config::from_env().with_capture_payloads(false));
let run = mcpd.trace("support agent");
run.set_input("stays in the process");

let public = run.child_of("lookup_order", SpanKind::ToolCall);
public.capture_payloads(true);
public.set_output("order 5512 shipped, and this leaves");
```

## Assert what you recorded

The exporter hands every batch to the client's sink. The HTTP transport is
the default; a `RecordingSink` keeps batches in memory, which is how a test
checks what the instrumentation recorded without a server, and a
`ConsoleSink` prints one JSON line per signal to standard error while you
debug. A sink that fails is a failed export, reported once per interval and
never raised.

```rust
# use std::sync::Arc;
# use mcpd_sdk::{Config, Mcpd, RecordingSink};
# async fn f() {
let sink = Arc::new(RecordingSink::new());
let mcpd = Mcpd::with_config(Config::default().with_sink(sink.clone()));

mcpd.trace("support agent").end();
let _ = mcpd.flush().await;

// Sent on start and again on end, as on the wire.
assert_eq!(sink.spans().len(), 2);
assert_eq!(sink.spans()[1].name, "support agent");
# }
```

`mcpd.stats()` answers `queued`, `dropped`, `capacity`, `exported`, `failed`
and `rejected`, the same six names as every other SDK.

## Record a finished span

An adapter for another tracing system, a batch import or a replay holds spans
whose ids, parent, start and end are already known. `record` queues one as
given: never re-parented, re-timed or re-sampled. Correlation fields the
record leaves empty are filled from the client's; a record whose ids are not
OpenTelemetry-shaped, whose name is empty or whose start time is missing is
dropped and counted in `stats().rejected`, never raised.

```rust
# use mcpd_sdk::{model::Span, Mcpd};
# fn f(mcpd: &Mcpd, span: Span) {
mcpd.record(span);
# }
```

A span's `set_external_parent` carries a parent from a tracing system that is
not mcpd, for correlation only.

## Spans

A `Span` is an RAII guard: it ends when it is dropped, so an early return or a
`?` cannot leave one open.

```rust
# use mcpd_sdk::{Mcpd, SpanKind};
# async fn f(mcpd: &Mcpd) -> Result<(), Box<dyn std::error::Error>> {
let run = mcpd.trace("support agent");
{
    let step = run.child_of("retrieve", SpanKind::Retrieval);
    step.set_input("order 5512");
    // ... whatever this step does; the span ends at the closing brace
}
run.end();
# Ok(())
# }
```

`set_model_parameters` records the sampling settings a call was made with.
They are settings, not payload, so they are kept even when `capture_payloads`
is off, and they are what somebody reproducing the run has to match:

```rust
# use mcpd_sdk::{Mcpd, SpanKind};
# fn f(mcpd: &Mcpd) {
use mcpd_sdk::model::ModelParameters;

let call = mcpd.span("answer", SpanKind::ModelGeneration);
call.set_model_parameters(ModelParameters {
    temperature: Some(0.0),
    seed: Some(0),
    max_tokens: Some(1024),
    ..ModelParameters::default()
});
call.end();
# }
```

`None` on a field is absent; `Some(0)` is a value, so a zero seed is a seed. A
second call replaces the first, because one set of settings describes one model
call.

Recording a failure sets the status to `error`, and the message is required:

```rust
# use mcpd_sdk::{Mcpd, SpanKind};
# fn f(mcpd: &Mcpd) {
let tool = mcpd.span("notify_customer", SpanKind::ToolCall);
tool.set_error("sms gateway timed out after 5s");
tool.end();
# }
```

## Correlation

Five fields say who a run was for: session, thread, user, request and release.
Set them on the run, once. Every step started afterwards inherits them, and so
does every log, metric, score and piece of feedback recorded inside the run's
scope. That is what makes a session filter return the whole run rather than its
first span, and what makes erasure by subject reach every row the run wrote.

```rust
# use mcpd_sdk::{LogLevel, Mcpd, SpanKind};
# fn f(mcpd: &Mcpd) {
let run = mcpd.trace("support agent");
run.set_session("session-4f21");
run.set_thread("thread-7");
run.set_user("user-41");
run.set_request("req-9001");
run.set_release("2026.08.1");

// Started after the run was tagged, so it carries all five.
let step = run.child_of("lookup_order", SpanKind::ToolCall);
step.end();

run.sync_scope(|| mcpd.log(LogLevel::Warn, "the shipping service is slow"));
run.end();
# }
```

A child may override one field without losing the rest, which is how a
sub-agent gets its own session and keeps the user it is acting for. The user id
is hashed at ingest, so mcpd stores a digest and never the value you sent.

## Context

Inside a process, context travels in a tokio task-local. `Span::scope` sets it,
and `Mcpd::span` picks it up, so a helper three layers down nests correctly
without being handed the parent span.

```rust
# use mcpd_sdk::{Mcpd, SpanKind};
# async fn f(mcpd: &Mcpd) {
let run = mcpd.trace("support agent");
run.scope(async {
    // No parameter was threaded here, and this still nests under the run.
    let step = mcpd.span("lookup_order", SpanKind::ToolCall);
    step.end();
})
.await;
run.end();
# }
```

Between processes, it travels in a W3C `traceparent` header:

```rust
# use mcpd_sdk::{Mcpd, SpanKind};
# async fn f(mcpd: &Mcpd, span: &mcpd_sdk::Span, inbound_header: Option<&str>) {
// Outbound: put the header on the request.
let header = span.traceparent();

// Inbound: continue the caller's trace instead of starting a new one.
let run = mcpd.trace_from("charge card", inbound_header, SpanKind::AgentRun);
run.end();
# }
```

## Durable runs

The same client drives the daemon's durable runs. A workflow is written once, in
code: the graph goes to the daemon as a definition, and the step bodies stay in
this process for a `Worker` to serve. The daemon holds the cursor, the retries,
the timers and the waits, so a run survives a deploy, a crash and a week-long
approval. No model, prompt or credential ever reaches the daemon.

```rust,no_run
use std::time::Duration;

use mcpd_sdk::{
    when, Mcpd, PublishEvent, RetryPolicy, SpanKind, StartRun, StepContext, StepFailure,
    StepOptions, Worker, Workflow,
};
use serde_json::{json, Value};

# async fn deliver(_text: &str, _key: &str) -> Result<(), String> { Ok(()) }
#[tokio::main]
async fn main() -> Result<(), mcpd_sdk::Error> {
    let mcpd = Mcpd::new();

    let support = Workflow::new("support_ticket")
        .step(
            "classify",
            |ctx: StepContext| async move {
                // Anything: your own model call, a database, an HTTP request.
                let input: Value = ctx.input()?;
                let kind = if input["ticket"].as_str().unwrap_or("").starts_with('Q') { "question" } else { "bug" };
                Ok::<_, StepFailure>(json!({ "kind": kind }))
            },
            StepOptions::new(),
        )
        .agent(
            "draft",
            |ctx: StepContext| async move {
                // Your own agent loop, as one durable step. A span from the
                // context lands under the step in the run's trace, and its
                // usage is the step's usage.
                let span = ctx.span("model_call", SpanKind::ModelGeneration);
                span.set_model("claude-sonnet-4", "anthropic");
                span.set_tokens(812, 37);
                span.end();
                Ok::<_, StepFailure>("an answer")
            },
            StepOptions::new()
                .input(json!({ "kind": "${steps.classify.output.kind}" }))
                .retry(RetryPolicy::attempts(3).exponential(Duration::from_millis(500))),
        )
        .wait_for(
            "approval",
            "approved",
            "${input.ticket}",
            Duration::from_secs(7 * 24 * 3600),
            StepOptions::new().when(when("steps.classify.output.kind", "eq", "bug")),
        )
        .step(
            "send",
            |ctx: StepContext| async move {
                let input: Value = ctx.input()?;
                deliver(input["text"].as_str().unwrap_or(""), ctx.idempotency_key())
                    .await
                    .map_err(|e| StepFailure::retryable(e).retry_after(Duration::from_secs(2)))?;
                Ok::<_, StepFailure>(json!({ "sent": true }))
            },
            StepOptions::new().input(json!({ "text": "${steps.draft.output}" })).at_most_once(),
        )
        .output(json!({ "sent": "${steps.send.output.sent}" }));

    // Serve step bodies for as long as this process lives.
    let worker = Worker::builder(&mcpd).concurrency(8).register(support).build();
    let serving = { let w = worker.clone(); tokio::spawn(async move { w.serve().await }) };

    // Start a run from anywhere that holds an execute key.
    let run = mcpd.start_run(StartRun::new("support_ticket").input(json!({ "ticket": "Q-42" }))).await?;

    // Answer the wait from a webhook or a console. A repeated event id is a duplicate.
    mcpd.publish_event(PublishEvent::new("approved", "Q-42").payload(json!({ "by": "ops" })).event_id("hook-1")).await?;

    // Follow the journal to the end, then read the run.
    mcpd.watch_run(&run.run_id, -1, |event| println!("{} {:?}", event.seq, event.kind)).await?;
    let detail = mcpd.get_run(&run.run_id).await?;
    println!("{:?} {:?}", detail.run.status, detail.run.output);

    worker.stop();
    let _ = serving.await;
    Ok(())
}
```

What the pieces do:

- **`Workflow::new`** builds the definition. `step` and `agent` are claimable
  and carry a body; `sleep`, `sleep_until` and `wait_for` are held by the
  daemon with no process alive; `parallel`, `foreach` and `repeat` fan out one
  claimable step per branch, item or iteration; `subworkflow` starts a child
  run. `StepOptions` carries the `input` template, the `when` guard, the
  effect, the timeout, the retry policy and the rest; `${path}` references
  read `input`, `steps.<id>` and `run` and are rendered per attempt. A
  mistake such as an id the daemon would refuse comes back from `document`.
- **`Worker`** registers the definitions, long-polls for leases and serves
  each one on a task: `started`, heartbeats while the body runs, then
  `complete` or `fail`. The four conflicts the daemon can answer are handled
  by reason, so a call that landed twice is a success and a fenced lease is
  dropped, never reported as a failure. `claim(n, Some(run_id))` serves one
  run from a script.
- **`StepContext`** carries the lease, `input`, `prior` and `state` decoders,
  `set_state` to merge keys into the run's state, `span` to nest spans under
  the step, `idempotency_key` for a third party that accepts one, and
  `cancelled()`, a future that resolves when the lease is lost or the run
  ends.
- **Usage** adds up on its own. Every span opened through the context, and
  every span nested under one, adds its tokens to the step when it ends, and
  the last model and provider seen win, so the daemon prices what the trace
  shows. `set_usage` and `set_tokens` are the explicit override for a body
  that records no spans: the fields they set replace the sum.
- **Hooks and stats** make a worker observable from inside: `on_lease`,
  `on_step_start`, `on_step_end` and `on_conflict` sit beside `on_error` on
  the builder, each wrapped so a panic is reported and the loop goes on, and
  `worker.stats()` answers `claimed`, `inflight`, `completed`, `failed` by
  class, `conflicts` by reason and `unservable`.
- **Returning** `StepFailure::retryable(..)` retries within the step's
  budget. Any `StepFailure` names its own class. A `String`, `&str`,
  `serde_json::Error` or `mcpd_sdk::Error` converts to a fatal failure with
  `?`, and the step's `on_error` decides whether the run fails or moves on.
- **`start_run`**, **`get_run`**, **`list_runs`**, **`cancel_run`**,
  **`run_events`**, **`watch_run`**, **`signal_run`**, **`publish_event`**,
  **`put_definition`**, **`get_definition`**, **`list_definitions`** and the
  worker calls mirror the HTTP run surface one for one. `wait_run` and
  `start_run_and_wait` follow a run to its end and hand back its output, or
  `Error::RunFailed` carrying the run. A 409 from a worker call is
  `Error::Conflict` with the daemon's reason.
- **A lease's deadline** is enforced by the worker itself: a body that runs
  past `deadline_ms` sees `cancelled()` resolve and the attempt fails
  `retryable`. A value `input`, `prior` or `state` cannot decode fails the
  step `invalid_input`.

The wire protocol, the definition document and every closed enum are in
[`spec/definition.schema.json`](../../spec/definition.schema.json) and the
[run contract](../../internal/run-contract-v1.md).

## Reading

The same client reads. Every method mirrors one HTTP endpoint and returns the
generated types.

```rust
# use mcpd_sdk::{Include, Mcpd, TraceFilter};
# use mcpd_sdk::model::SpanStatus;
# async fn f(mcpd: &Mcpd) -> Result<(), mcpd_sdk::Error> {
// The runs that failed in the last window, slowest first.
let failures = mcpd
    .list_traces(&TraceFilter {
        status: Some(SpanStatus::Error),
        sort: Some("duration".to_owned()),
        limit: Some(20),
        ..TraceFilter::default()
    })
    .await?;

for trace in &failures.traces {
    let detail = mcpd.get_trace(&trace.trace_id, Include::all()).await?;
    println!("{}: {} spans", detail.trace.name, detail.spans.len());
}
# Ok(())
# }
```

`list_spans`, `list_logs`, `query_metrics`, `list_scores`, `list_feedback`,
`analytics_summary`, `analytics_cost` and `health` are all there, with the same
shape.

## Flushing

`flush` sends what is queued. `shutdown` flushes, stops the worker and is safe to
call twice. Both are awaitable, and both return the last export failure so you
can log it; ignoring it is a reasonable default, because an observability
failure is not an application failure.

Both return within `DEFAULT_DEADLINE`, 10 seconds, even when the daemon never
answers; `flush_within` and `shutdown_within` take another limit. A flush that
runs out of time leaves the rest queued for the worker. A shutdown that runs
out of time starts no further batch and gives up what is still queued.

```rust
# async fn f(mcpd: &mcpd_sdk::Mcpd) {
if let Err(e) = mcpd.shutdown().await {
    eprintln!("mcpd could not export: {e}");
}
# }
```

## Tests

```sh
cargo test -p mcpd-sdk
```

The suite runs against a stand-in server that captures what the SDK sent. There
is also an end-to-end suite that needs a real mcpd:

```sh
mcpd demo --data /tmp/mcpd-live.db --listen 127.0.0.1:4317
MCPD_TEST_ENDPOINT=http://127.0.0.1:4317 cargo test -p mcpd-sdk --test live -- --include-ignored
```

## Types

Every model type comes from `spec/mcpd.spec.json` by way of `mcpd-core`, and is
re-exported here as `mcpd_sdk::model`. None of it is hand-written, in this SDK
or in any other.
