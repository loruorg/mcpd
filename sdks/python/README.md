# mcpd for Python

Agent observability and durable runs that are a process, not a dependency.
This package sends traces, logs, metrics, scores and feedback to an
[mcpd](https://mcpd.sh) daemon, and serves durable workflows from your own
process: the daemon owns the run between steps, your code owns everything
inside one. It has no required runtime dependencies, it never blocks your
agent, and it never raises out of an instrumentation call.

```python
from mcpd import FeedbackKind, Mcpd, SpanKind

mcpd = Mcpd()  # http://localhost:4317, or set MCPD_ENDPOINT

with mcpd.trace("support_agent", session_id="sess-8121", user_id="u-3312") as run:
    run.set_input({"question": "where is my order?"})

    with mcpd.span("plan", kind=SpanKind.MODEL_GENERATION) as generation:
        generation.set_model("gpt-4o-mini", provider="openai")
        generation.set_input([{"role": "user", "content": "where is my order?"}])
        generation.set_output("I should look the order up first.")
        generation.set_usage(input_tokens=812, output_tokens=96)

    try:
        with mcpd.span("lookup_order", kind=SpanKind.TOOL_CALL) as tool:
            tool.set_input({"order_id": "A-7741"})
            raise TimeoutError("orders API did not answer in 2s")
    except TimeoutError:
        mcpd.log("warn", "answering without the live lookup")

    run.set_output("Your order shipped on Tuesday.")

mcpd.score("answer_relevance", 0.82, min=0.0, max=1.0, trace_id=run.trace_id,
           reason="named the order and gave a delivery window")
mcpd.feedback(FeedbackKind.THUMBS, "up", trace_id=run.trace_id)

mcpd.flush()
detail = mcpd.get_trace(run.trace_id, include=["scores"])
print(detail.trace.span_count, "spans,", detail.trace.cost.total_cost, "USD")

mcpd.shutdown()
```

Start a daemon to send it to:

```console
$ mcpd demo --data ./mcpd.db
```

## Install

```console
$ pip install mcpd
```

Python 3.9 or newer. `pip install "mcpd[httpx]"` adds connection reuse through
`httpx`; the SDK behaves identically without it.

## What it guarantees

These are the behaviours the [SDK contract](../SDK-CONTRACT.md) fixes across
every mcpd SDK, and what they mean here.

| Behaviour | In Python |
| --- | --- |
| Instrumentation never raises | Every recording call is guarded. A failure is logged at most once per interval and swallowed. Read calls are the exception and do raise. |
| Non-blocking | Recording appends to an in-memory queue and returns. A daemon thread does the HTTP. |
| Bounded memory | `max_queue` signals, default 8192. When full the oldest are dropped, `mcpd.dropped` counts them, and one warning is logged per interval. |
| One trace decision | Sampling is decided at the root from the trace id and inherited by every child. An inbound `traceparent` always wins. |
| Explicit shutdown | `flush()` and `shutdown()` are idempotent. Use `aflush()` and `ashutdown()` inside an event loop. A client also flushes at interpreter exit. Both return within `timeout`, 10 seconds by default, even when the daemon never answers: `flush()` returns `False` and leaves the rest queued, and `shutdown()` starts no further batch and gives up what is still queued. |
| OpenTelemetry ids | 32 lowercase hex for traces, 16 for spans, never all zero. |
| W3C context | `traceparent` is parsed on the way in and `span.traceparent` is emitted on the way out. |
| Unfinished spans | A span is exported when it starts and updated when it ends, so a run that crashes mid-flight is still visible. |
| UTC nanoseconds | Timestamps come from `time.time_ns()` and go on the wire with nine digits. |
| Errors carry a message | `set_error` sets `status = error` and always populates `error.message`. |

## Configuration

Every field is settable in code and by environment variable. Code wins.

| Argument | Environment | Default |
| --- | --- | --- |
| `endpoint` | `MCPD_ENDPOINT` | `http://localhost:4317` |
| `api_key` | `MCPD_API_KEY` | none |
| `project` | `MCPD_PROJECT` | `default` |
| `environment` | `MCPD_ENVIRONMENT` | none |
| `service_name` | `MCPD_SERVICE_NAME` | none |
| `service_version` | `MCPD_SERVICE_VERSION` | none |
| `sample` | `MCPD_SAMPLE` | `1.0` |
| `enabled` | `MCPD_ENABLED` | `true` |
| `capture_payloads` | `MCPD_CAPTURE_PAYLOADS` | `true` |
| `log_level` | `MCPD_LOG_LEVEL` | `debug` |
| `batch_size` | `MCPD_BATCH_SIZE` | `128` |
| `flush_interval` | `MCPD_FLUSH_INTERVAL_MS` | `2.0` seconds |
| `max_queue` | `MCPD_MAX_QUEUE` | `8192` |
| `timeout` | `MCPD_TIMEOUT_MS` | `10.0` seconds |

`enabled=False` makes every call a no-op that still returns valid objects, so
you can disable mcpd in tests without changing the code under test.
`capture_payloads=False` keeps prompts and completions in the process: spans
are still recorded with their timing, status, usage, model, model parameters,
correlation and errors, and `input` and `output` are dropped before they reach the queue. A
span can say otherwise for itself with `capture_payloads=True` in its start
fields, and the other way round.

## Tracing

`mcpd.trace(name)` starts a run. `mcpd.span(name)` starts a step under whatever
is currently active, using `contextvars`, so it works across `await` and inside
tasks. Both are context managers that end the span and record an exception if
the block raised.

```python
with mcpd.trace("nightly_summariser") as run:
    with mcpd.span("fetch", kind="http_request") as step:
        step.set_output({"documents": 42})
```

Async is the same object with two more keywords:

```python
async with mcpd.trace("agent") as run:
    async with mcpd.span("generate", kind=SpanKind.MODEL_GENERATION) as generation:
        await model.complete(prompt)
    await mcpd.aflush()
```

Without a `with` block, end the span yourself. It is still exported when it
starts, so a span that never ends is visible as a span that never ended.

```python
run = mcpd.trace("streaming_answer")
...
run.end()
```

The setters are `set_input`, `set_output`, `set_usage`, `set_model`,
`set_model_parameters`, `set_error`, `set_metadata`, `set_attributes`,
`set_tags`, `set_entity`, `set_cost`, `set_status` and `add_link`. All of them
return the span, and most can also be passed as a keyword when the span is
started:

```python
with mcpd.span("rerank", kind="rag_action", model="bge-reranker", tags=["rag"]):
    ...
```

`set_model_parameters` records the sampling settings a call was made with:

```python
with mcpd.span("answer", kind="model_generation") as call:
    call.set_model_parameters(temperature=0, seed=0, max_tokens=1024)
```

They are settings, not payload, so they are kept even when `capture_payloads`
is off, and they are what somebody reproducing the run has to match. An
argument you leave out is absent; one you pass is kept whatever it is, so
`seed=0` is a seed. A second call replaces the first, because one set of
settings describes one model call.

`span.event("cache_miss")` records a point in time rather than a duration.

## Across a service boundary

```python
# outbound
requests.post(url, headers={"traceparent": mcpd.traceparent()}, json=payload)

# inbound
with mcpd.trace("handler", traceparent=request.headers.get("traceparent")) as run:
    ...
```

A malformed header starts a new trace rather than failing. An inbound decision
not to sample is obeyed, because a trace that some services keep and others drop
is not a trace.

## The other four signals

```python
mcpd.log("warn", "retrying", {"attempt": 2})
mcpd.metric("support.answers", 1, "counter", labels={"outcome": "ok"}, unit="{answer}")
mcpd.score("groundedness", 0.91, min=0.0, max=1.0, reason="every claim cited")
mcpd.feedback("thumbs", "down", comment="wrong order")
```

All four attach to the active span when there is one. `min` and `max` are
keyword-only and required on `score`: a bare number with no declared scale
cannot be compared to anything.

## Sinks, stats and finished spans

A sink is where a drained batch goes. The HTTP transport is the default. A
`RecordingSink` keeps every batch in memory, so a test can assert what the
instrumentation recorded without a server; a `ConsoleSink` prints one JSON line
per signal to stderr. A sink that raises is a failed batch, counted and
reported, never an exception in your code.

```python
from mcpd import Mcpd, RecordingSink

sink = RecordingSink()
mcpd = Mcpd(sink=sink)
with mcpd.trace("answer") as run:
    run.set_usage(input_tokens=12)
mcpd.flush()
assert sink.spans[0].usage.input_tokens == 12
print(mcpd.stats())  # queued, dropped, capacity, exported, failed, rejected
```

`mcpd.record(span)` queues a `SpanRecord` that already ended, ids, parent and
times as you hold them: what an adapter for another tracing system or a batch
import has. It is never re-parented, re-timed or re-sampled. A malformed record
is dropped and counted in `stats().rejected`, never raised.
`external_parent_span_id=` on any span names a parent from a tracing system that
is not mcpd, carried for correlation and never used for parenting.

## Reading data back

The read methods mirror the HTTP API and return the generated types. They are
synchronous and they raise `McpdError` on failure. `from_` and `to` accept a
`datetime` or a shorthand such as `-15m`, `-1h` or `-7d`.

```python
traces = mcpd.list_traces(status="error", from_="-1h", limit=20)
detail = mcpd.get_trace(traces.traces[0].trace_id, include=["logs", "scores"])
summary = mcpd.analytics_summary("-24h", "-0s", group_by="model")
cost = mcpd.analytics_cost("-7d", "-0s", group_by="model", bucket="1d")
```

Also available: `list_spans`, `list_logs`, `list_scores`, `list_feedback`,
`query_metrics`, `delete_trace`, `get_trajectory` and `health`.

## Experiments

`run_experiment` runs your code over every case of a dataset, each case in its
own root span, and scores each answer against its case. A case that raises is a
failed run, and the others still run.

```python
results = mcpd.run_experiment("support-questions", answer, name="answers-v2",
                              scorers=["exact_match"], concurrency=4)
```

A plain function runs on a pool of threads; an `async def` target runs on one
event loop. From async code, call `await mcpd.arun_experiment(...)`. The parts
are there too: `list_dataset_items`, `create_experiment`, `score_experiment` and
`experiment_results`.

In async code, run them off the loop:

```python
detail = await asyncio.to_thread(mcpd.get_trace, trace_id)
```

## Durable runs

The same client drives the daemon's durable runs. A workflow is written once, in
code: the graph goes to the daemon as a definition, and the step bodies stay in
this process for a `Worker` to serve. The daemon holds the cursor, the retries,
the timers and the waits, so a run survives a deploy, a crash and a week-long
approval. No model, prompt or credential ever reaches the daemon.

```python
from mcpd import Mcpd, RetryableError, RetryPolicy, Worker, when, workflow

mcpd = Mcpd(project="support")
support = workflow("support_ticket")


@support.step("classify")
def classify(ctx, input, prior):
    # Anything: your own model call, a database, an HTTP request.
    return {"kind": "question" if input["ticket"].startswith("Q") else "bug"}


@support.agent("draft", input={"kind": "${steps.classify.output.kind}"},
               retry=RetryPolicy(max_attempts=3, backoff="exponential", initial_ms=500))
def draft(ctx, input, prior):
    # Your own agent loop, as one durable step. Spans recorded through the
    # context land under the step in the run's trace.
    with ctx.span("model_call") as span:
        span.set_model("claude-sonnet-4", "anthropic")
        span.set_usage(input_tokens=812, output_tokens=37)
        text = f"an answer to a {input['kind']}"
    return text


support.wait_for("approval", event="approved", key="${input.ticket}",
                 timeout_ms=7 * 24 * 3_600_000,
                 when=when("steps.classify.output.kind", "eq", "bug"))


@support.step("send", input={"text": "${steps.draft.output}"}, effect="at_most_once")
def send(ctx, input, prior):
    if ctx.cancelled.is_set():
        return None
    if not deliver(input["text"], ctx.idempotency_key):
        raise RetryableError("mail relay busy", retry_after_ms=2_000)
    return {"sent": True}


support.output({"sent": "${steps.send.output.sent}"})

# Serve step bodies for as long as this process lives. `serve()` blocks; run it
# on a thread of its own when the process does other work too.
worker = Worker(mcpd, concurrency=8).register(support)
threading.Thread(target=worker.serve, daemon=True).start()

# Start a run from anywhere that holds an execute key.
run = mcpd.runs.start(support, input={"ticket": "Q-42"})

# Answer the wait from a webhook or a console. A repeated event id is a duplicate.
mcpd.events.publish("approved", "Q-42", payload={"by": "ops"}, event_id="hook-1")

# Follow the journal to the end, then read the run.
for event in mcpd.runs.watch(run.run_id):
    print(event.seq, event.kind)
detail = mcpd.runs.get(run.run_id)
print(detail.run.status, detail.run.output)
```

What the pieces do:

- **`workflow()`** builds the definition. `step` and `agent` are claimable and
  carry a body, as a call or a decorator; `sleep`, `sleep_until` and `wait_for`
  are held by the daemon with no process alive; `parallel`, `foreach` and
  `loop` fan out one claimable step per branch, item or iteration;
  `subworkflow` starts a child run. `input` templates and `when` guards read
  `input`, `steps.<id>` and `run` with `${path}` references the daemon renders
  per attempt.
- **`Worker`** registers the definitions, long-polls for leases on a thread
  pool and serves each one: `started`, heartbeats while the body runs, then
  `complete` or `fail`. The four conflicts the daemon can answer are handled
  by reason, so a call that landed twice is a success and a fenced lease is
  dropped, never reported as a failure. `worker.claim(n, run_id)` serves one
  run from a script.
- **`StepContext`** carries the lease, `state`, `set_state` to merge keys into
  the run's state, `span` to nest spans under the step, `idempotency_key` for
  a third party that accepts one, and `cancelled`, an event set when the lease
  is lost or the run ends. What the body's spans spend is the step's usage:
  tokens are summed as each span ends and the last model seen wins, so the
  daemon prices the step from what the trace shows. `set_usage` is an explicit
  override, field by field, for a body that records no spans.
- **Hooks and stats.** `Worker(on_lease=, on_step_start=, on_step_end=,
  on_conflict=)` sit beside `on_error`; `on_step_end` gets a `StepOutcome`
  (`completed`, or `failed` with its class). A hook that raises is reported
  through `on_error` and the loop goes on. `worker.stats()` answers `claimed`,
  `inflight`, `completed`, `failed` by class, `conflicts` by 409 reason and
  `unservable`.
- **Raising** a `RetryableError` retries within the step's budget. A
  `StepFailure` names its own class. Any other exception is `fatal`, and the
  step's `on_error` decides whether the run fails or moves on.
- **`mcpd.runs`**, **`mcpd.definitions`**, **`mcpd.events`** and
  **`mcpd.steps`** mirror the HTTP run surface one for one. `runs.wait` and
  `runs.start_and_wait` follow a run to its end and hand back its output, or
  raise `RunFailedError` carrying the run. A 409 from a worker call is raised
  as `StepConflictError` with the daemon's reason.
- **A lease's deadline** is enforced by the worker itself: a body that runs
  past `deadline_ms` sees `ctx.cancelled` set and the attempt fails
  `retryable`. `ctx.input(decode)`, `ctx.prior(decode)` and
  `ctx.state_as(decode)` check a value with any callable, a dataclass or a
  pydantic model included, and a value the decoder refuses fails the step
  `invalid_input`.

The wire protocol, the definition document and every closed enum are in
[`spec/definition.schema.json`](../../spec/definition.schema.json) and the
[run contract](../../internal/run-contract-v1.md).

## Types

Every wire type is generated from `spec/mcpd.spec.json` into `mcpd/_generated.py`
and re-exported from `mcpd`. Do not edit that file; edit the spec and run
`make gen`. The one name that collides is the stored `Span` record, which is
exported as `SpanRecord` so that `Span` can be the live handle you hold.

```python
from mcpd import SpanKind, SpanStatus, TokenUsage, TraceDetail
from mcpd import records          # every generated type, unabridged
```

## Tests

```console
$ python3 -m unittest discover
```

The suite runs offline. To include the end-to-end tests, point them at a running
daemon:

```console
$ mcpd demo --data /tmp/mcpd-test.db --listen 127.0.0.1:4317
$ MCPD_TEST_ENDPOINT=http://127.0.0.1:4317 python3 -m unittest discover
```

A full worked example is in [`examples/python/quickstart.py`](../../examples/python/quickstart.py).
