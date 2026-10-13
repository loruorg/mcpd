# The SDK contract

Every mcpd SDK implements this surface. The names change to suit each language's
conventions; the **behaviour does not**. A user who has read the TypeScript
quickstart must be able to predict the Kotlin one.

This file, together with `spec/mcpd.spec.json`, is what makes an API change a
mechanical propagation instead of five independent guesses. When you change
either, see [`AGENTS.md`](../AGENTS.md) for what else must change with it.

## 1. Non-negotiable behaviours

These are the ones that are easy to get wrong and expensive to get wrong.

| # | Behaviour | Why |
| --- | --- | --- |
| B1 | **Never throw from instrumentation.** A failed export, an unreachable server, a serialisation error: all are logged at most once per interval and swallowed. | Observability must never take down the thing it observes. This is the single most important rule in this document. |
| B2 | **Non-blocking by default.** Recording a span appends to an in-memory queue and returns. Network I/O happens on a background worker. | An agent's latency must not depend on mcpd being fast, or up. |
| B3 | **Bounded memory.** The queue has a maximum size. When it is full, the oldest spans are dropped, a counter increments, and a warning is logged once per interval. | An unbounded buffer turns a network outage into an out-of-memory kill. |
| B4 | **The trace decision is made once, at the root, and inherited.** | A half-recorded trace reads as a bug in the user's system. |
| B5 | **`flush()` and `shutdown()` are explicit, awaitable, idempotent and bounded in time.** `shutdown()` flushes, stops the worker, and is safe to call twice. Both return by a deadline (section 2.4). | Serverless and short-lived processes lose everything without this, and a process trying to exit must not be held open by a daemon that never answers. |
| B6 | **Ids are OpenTelemetry-shaped**: 32 lowercase hex for traces, 16 for spans, never all-zero. | Anything else breaks correlation with every other tool. |
| B7 | **W3C `traceparent` is parsed on the way in and emitted on the way out.** | This is what makes a trace survive a service boundary. |
| B8 | **An unfinished span is still sent.** Long-running spans are exported on start and updated on end. | A run that crashes mid-flight is exactly the run you need to see. |
| B9 | **Timestamps are UTC with nanosecond resolution where the language allows.** Never local time. | Local timestamps make a distributed trace unreadable. |
| B10 | **Errors recorded on a span set `status = error` and populate `error.message`.** An error with no message is rejected at the type level where possible. | "Something failed" is not actionable. |

## 2. Client construction

```
Mcpd(
  endpoint:        string   = env MCPD_ENDPOINT  ?? "http://localhost:4317"
  api_key:         string?  = env MCPD_API_KEY
  project:         string   = env MCPD_PROJECT     ?? "default"
  environment:     string?  = env MCPD_ENVIRONMENT
  service_name:    string?  = env MCPD_SERVICE_NAME
  service_version: string?  = env MCPD_SERVICE_VERSION
  sample:          float    = env MCPD_SAMPLE      ?? 1.0
  batch_size:      int      = 128
  flush_interval:  duration = 2s
  max_queue:       int      = 8192
  timeout:         duration = 10s
  enabled:         bool     = env MCPD_ENABLED     ?? true
  capture_payloads: bool    = env MCPD_CAPTURE_PAYLOADS ?? true
  log_level:       LogLevel = env MCPD_LOG_LEVEL ?? debug
  sink:            Sink     = the HTTP transport
  redact:          fn?      = none
)
```

Every field is settable in code and by environment variable, code winning. A
client constructed with no arguments at all must work against a local mcpd.

**The environment variable names are fixed, and identical in every language.**
A value copied from one service to another must keep working when the two are
written in different languages, so these names are not a per-SDK decision:

| Variable | Field | Notes |
| --- | --- | --- |
| `MCPD_ENDPOINT` | endpoint | |
| `MCPD_API_KEY` | api_key | |
| `MCPD_PROJECT` | project | |
| `MCPD_ENVIRONMENT` | environment | |
| `MCPD_SERVICE_NAME` | service_name | |
| `MCPD_SERVICE_VERSION` | service_version | |
| `MCPD_SAMPLE` | sample | A fraction, `0.0` to `1.0` |
| `MCPD_ENABLED` | enabled | `false` makes every call a no-op |
| `MCPD_BATCH_SIZE` | batch_size | |
| `MCPD_FLUSH_INTERVAL_MS` | flush_interval | **Whole milliseconds.** The unit is in the name so no SDK has to parse a duration string. |
| `MCPD_MAX_QUEUE` | max_queue | |
| `MCPD_TIMEOUT_MS` | timeout | **Whole milliseconds**, for the same reason. |
| `MCPD_CAPTURE_PAYLOADS` | capture_payloads | `false` keeps `input` and `output` in the process |
| `MCPD_LOG_LEVEL` | log_level | The lowest level `log` records. A name outside the enum is ignored |

An SDK reading any other `MCPD_*` name is a bug, and `make check` fails on it.
So is an SDK that does not read one of them: the gate checks that every SDK
reads every client name in this table.

`enabled = false` makes every call a no-op that still returns valid objects, so
a user can disable mcpd in tests without changing their code.

### 2.1 Payload capture

`capture_payloads = false` keeps prompts and completions in the process. Spans
are still recorded with their timing, status, usage, model, correlation and
errors, but `input` and `output` are dropped before the record reaches the
queue, whether they came from the start options, from `set_input` and
`set_output`, from an event or from `record`. Nothing sensitive is ever queued,
so nothing sensitive can leave the process on a flush. A span can override the
client for itself with `capture_payloads` in its start options, in either
direction; the override applies to that span only. Server-side redaction is the
other end of the same wire, and the wrong end for a team that must not send the
bytes at all.

### 2.1.1 Redaction in the process

`redact` is a function the client applies to every span's `input`, `output`,
`attributes` and `metadata`, and to every log line's `fields`, before the record
reaches the queue. It receives the value and returns the value to keep, so a
team can mask an account number or drop a field without sending the bytes. It
is code only: no environment variable can carry a function.

Two rules hold in every language.

- **It fails closed.** A `redact` that throws drops that value (the field
  becomes absent) and counts it in `stats().redaction_failures`. So does a
  result for `attributes`, `metadata` or `fields` that is not a map, because it
  cannot be stored as one. A null result drops the field without counting:
  that is the redactor choosing to remove it. A span sent on start and again on
  end (B8) is redacted each time, so a failure on its input counts once per record. It never raises into the caller (B1), and it
  never lets the unredacted value through, because a broken redactor is the
  case where sending the original does the most harm.
- **It runs after `capture_payloads`.** A payload capture has already
  stripped is never handed to `redact`.

### 2.2 Sinks

The exporter drains the queue into an `IngestBatch` and hands it to the
client's sink together with the batch's `ingest_key`:

```
Sink.export(batch: IngestBatch, ingest_key: string)
```

The HTTP transport is the default sink and the only one that talks to a daemon.
Every SDK ships two more, under the SDK's `sinks` module or namespace:

- `RecordingSink` keeps every batch in memory and exposes the `spans`, `logs`,
  `metrics`, `scores` and `feedback` it has seen, in arrival order, plus
  `batches` (how many) and `clear()`. It is how a user asserts what their
  instrumentation recorded without standing up a server.
- `ConsoleSink` writes one JSON line per signal, shaped
  `{"signal": "span", ...record}`, to standard error by default, for a local
  dump while debugging.

A sink runs on the exporter's worker inside the same guard as the transport:
one that throws is counted as a failed export and reported through the
diagnostics channel, never raised to the caller (B1). The sink sees the batch
after sampling, capture and validation, exactly what the daemon would have
received. A sink is chosen in code only; it has no environment variable.

### 2.3 Stats

`stats()` answers what the client has done since it was built: `queued`
(signals waiting), `dropped` (evicted by B3), `capacity`, `exported` (signals
handed to the sink), `failed` (batches the sink refused), `rejected` (spans
`record` refused) and `redaction_failures` (values `redact` could not process,
section 2.1.1). The same seven names in every language.

### 2.4 Flush and shutdown deadlines

`flush()` and `shutdown()` each return by a deadline: 10 seconds, unless the
caller passes another. Each SDK takes it in its own idiom: a timeout argument,
or in Go the context's deadline, with 10 seconds applied when the context has
none. A cap on the number of batches is not a deadline. A daemon that accepts
the connection and never answers costs every batch three attempts of the full
request timeout, and at the defaults (8192 signals, batches of 128, 10 seconds)
that is more than half an hour, for a process that is usually trying to exit.

When the deadline passes:

- `flush()` returns and says it did not finish, in the form the language has
  for that: `false`, an error, or `{:error, :timeout}`. What it had not sent
  stays queued for the worker.
- `shutdown()` returns and the client stays stopped. The worker starts no
  further batch, and what is still queued is given up; it stays visible in
  `stats().queued`.
- A batch already in flight is not cut short. The transport's own timeout and
  retry limit bound it.

The deadline covers the whole call: `shutdown()` does not spend one deadline
flushing and another stopping the worker.

### 2.5 The test client

A user testing their own code needs to see what it recorded without a daemon.
Every SDK ships a test client in a module of its own:

| Language | Name |
| --- | --- |
| TypeScript | `TestClient` from `@mcpd/sdk/testing`, a subclass of `Mcpd` |
| Python | `mcpd.testing.TestClient`, a subclass of `Mcpd` |
| Go | `mcpdtest.New(options...)`, whose `Client` field is a `*mcpd.Client` |
| Kotlin | `dev.mcpd.testing.TestClient`, whose `client` is an `Mcpd` |
| Rust | `mcpd_sdk::testing::TestClient`, whose `client()` is a `Mcpd` |
| Elixir | `Mcpd.Testing.start(opts)`, whose `client` is an `Mcpd` client |

It is a real client whose sink is a `RecordingSink`, so what the code under
test records goes through the same pipeline as in production: sampling,
`capture_payloads`, `redact`, validation and the queue bound. A test sees what
the daemon would have received, and nothing leaves the process.

- It takes every client option except `sink`. `enabled` defaults to `true` and
  `sample` to `1.0` whatever the environment says, so a CI job that sets
  `MCPD_ENABLED=false` does not turn the test client off. Every other option
  takes the client's own default.
- Every accessor flushes first, so the test never calls `flush()` itself.
- `spans(name?, kind?)` answers the finished spans, one per span id. A span is
  queued on start and again on end (B8); the accessor keeps the last record
  seen for each id, in the order each id first arrived, and leaves out a span
  that has not ended.
- `span(name)` answers the one finished span with that name, and fails with a
  message giving the count when there is none or more than one. It is the one
  call here that raises, because it is called from a test and a failed
  assertion is its purpose. Python spells it `recorded_span(name)`, because
  its client already has a `span` that starts one.
- `logs()`, `metrics()`, `scores()` and `recorded_feedback()` answer the
  other signals in arrival order. The last is not `feedback()` because the
  client already has a `feedback` that records one.
- `trees()` answers each trace as a tree of finished spans: the roots, each
  with its children ordered by start time. A span whose parent was not
  recorded is a root.
- `reset()` flushes and forgets everything recorded so far.

## 3. Tracing surface

```
trace(name, kind = agent_run, **fields) -> Span     # starts a root span
span.child(name, kind = generic, **fields) -> Span  # starts a child
span.event(name, **fields)                          # a point in time
span.set_input(value)
span.set_output(value)
span.set_usage(input_tokens, output_tokens, ...)
span.set_model(model, provider)
span.set_model_parameters(temperature?, top_p?, top_k?, max_tokens?, seed?,
                          stop_sequences?, has_response_format?)
span.set_error(message, kind?, stack?, retryable?)
span.set_metadata(map) / span.set_tags(list)        # tags only on a root
span.end(status = ok)
record(span)                                        # a span that already ended
```

Start options include `external_parent_span_id` in every SDK: a parent from a
tracing system that is not mcpd, carried for correlation and never used for
parenting inside mcpd's own tree. They also include `capture_payloads`, the
per-span override of section 2.1.

`record(span)` takes a finished `Span` record whose ids, parent, start and end
the caller already has: what an adapter for another tracing system, a batch
import or a replay holds. The record is queued as given: never re-parented,
never re-timed, never re-sampled. Correlation fields the record leaves empty
are filled from the client's own (project, environment, service); fields it
sets are kept. A record is refused, dropped and counted in `stats().rejected`
when its `trace_id` is not 32 lowercase hex, its `span_id` or `parent_span_id`
is not 16, either is all zero, its `name` is empty or its `start_time` is
missing; refusing never raises (B1). `capture_payloads = false` strips a
recorded span like any other, and `enabled = false` drops it like any other.

`set_model_parameters` records how a model was called, in the spec's
`ModelParameters`. It is the settings the caller chose, never what the provider
did with them, and it is what somebody reproducing the run has to match.

Three rules hold in every language.

- **These are metadata, not payload.** No field carries prompt or completion
  text, `stop_sequences` included: those are short strings the caller wrote. So
  they survive `capture_payloads = false`, which is the case that matters,
  because a run recorded with its prompts kept in the process is still a run
  that has to be reproducible. The response format is recorded as a boolean
  only; the schema or grammar itself is caller-owned content and is not sent.
- **A value of zero is a value.** `seed = 0` is a seed and `temperature = 0` is
  a temperature. Only "not given" is absent, and every SDK tests this, because
  reading zero as absent is the mistake this kind of code invites.
- **One call describes one model call**, so a second call replaces the first
  rather than merging into it. Settings half from one request and half from
  another would describe a call nobody made.

Elixir spells it as the `model_parameters:` option on `Mcpd.span` and
`Mcpd.with_span`, taking a keyword list or a `%Mcpd.ModelParameters{}`. An
Elixir span is described by the options it was opened with, the same way
`model:`, `provider:` and `usage:` already are, and the SDK has no mutable span
handle for a setter to sit on.

Each language additionally offers its **idiomatic scoped form**, which ends the
span automatically and records an error if the block raised:

- TypeScript: `await mcpd.withSpan("name", async (span) => { ... })`
- Python: `with mcpd.span("name") as span:` and `async with`
- Rust: an RAII guard that ends on drop
- Go: `ctx, span := mcpd.Start(ctx, "name"); defer span.End()`
- Kotlin: `mcpd.span("name") { span -> ... }`

Context propagation uses the language's natural mechanism: `context.Context` in
Go, `contextvars` in Python, `AsyncLocalStorage` in TypeScript, coroutine
context in Kotlin, a task-local in Rust.

### 3.1 Model calls

A model call is the span most worth recording well, and recording it by hand
is four calls and a mapping table per provider. Every SDK ships the mapping:

```
usage_from_openai(response)    -> TokenUsage?   # chat completions or responses
usage_from_anthropic(response) -> TokenUsage?   # messages
span.record_response(response, provider?)       # model, usage, id, finish reason, tier
span.mark_first_token()                         # time to first token
span.set_guardrail(tripped, reason?)            # a guardrail span's outcome
```

`response` is the provider's response as the language holds it: the parsed
JSON object, a map, or the provider SDK's own object where the language can
read its fields generically. Nothing here imports a provider SDK.

**OpenAI.** Chat completions report `usage.prompt_tokens`, with
`prompt_tokens_details.cached_tokens` and `audio_tokens`, and
`completion_tokens` with `completion_tokens_details.reasoning_tokens` and
`audio_tokens`. The responses API reports `usage.input_tokens` with
`input_tokens_details.cached_tokens`, and `output_tokens` with
`output_tokens_details.reasoning_tokens`. The prompt total is gross, so
`input_details.text` is the total less the cached and audio tokens.

**Anthropic.** `usage.input_tokens` is net of the cache. `cache_read_input_tokens`
and `cache_creation_input_tokens` are reported beside it, and
`cache_creation.ephemeral_5m_input_tokens` and `ephemeral_1h_input_tokens`
split the write by lifetime when present. So `input_details.text` is
`input_tokens`, the cache meters are copied, and `input_tokens` on the record is
the sum of all three, the size of the prompt.

`record_response` detects the shape (a `type` of `message`, or `input_tokens`
beside `cache_read_input_tokens`, is Anthropic; `prompt_tokens`, or
`input_tokens_details`, is OpenAI) and then:

- sets the model from `response.model`, and the provider from the argument or
  else `openai` or `anthropic` by shape;
- sets the usage from the mapping above;
- sets the attributes `gen_ai.response.id` (the response's `id`),
  `gen_ai.response.finish_reasons` (a list: each choice's `finish_reason`, or
  Anthropic's `stop_reason`, or the responses API's `status`) and
  `gen_ai.response.service_tier` (`service_tier`, or Anthropic's
  `usage.service_tier`), each only when present. The daemon prices a batch or
  flex call from the last one.

It never sets `input` or `output`: the text is the caller's to record or not,
under `capture_payloads`. An unknown shape, a missing field or a wrong type is
not an error: what can be read is recorded, the rest is left absent, and
nothing raises (B1).

`mark_first_token` records `mcpd.time_to_first_token_ms`, the milliseconds
from the span's start to the call, as an attribute. Only the first call counts,
so it can sit inside a streaming loop.

`set_guardrail` records `mcpd.guardrail.tripped`, a boolean, and
`mcpd.guardrail.reason` when given. A guardrail that tripped is not an error
by itself; the caller decides whether to also call `set_error`.

Elixir has no mutable span handle, so it offers the same pieces as functions
that return span options: `Mcpd.ModelCall.usage_from_openai/1`,
`usage_from_anthropic/1`, `options/2` (the `model:`, `provider:`, `usage:` and
`attributes:` that `record_response` would set), `first_token/1` (the attribute
from a start time) and `guardrail/2`.

### 3.2 Streamed model calls

A streamed response arrives as chunks, and the facts `record_response` reads
are spread across them. Every SDK ships a wrapper that collects them:

```
span.record_stream(stream, provider?) -> stream     # the same chunks, recorded
```

`stream` is the provider's stream in the language's own form: an async
iterable in TypeScript, an iterator or async iterator in Python, a
`Next`/`Current`/`Err`/`Close` stream in Go, a `Flow` in Kotlin, a `Stream` in
Rust, an `Enumerable` in Elixir. The wrapper is the same form, and the caller
reads it in place of the original. Elixir has no span handle, so there it is
`Mcpd.ModelCall.record_stream(client, stream, name, opts)`, which opens the
model span when the stream is first read.

Four rules hold in every language.

- **Every chunk passes through unchanged**, in order, none added and none
  dropped. Reading a chunk never raises: a chunk of an unknown shape, a missing
  field or a wrong type is skipped by the accumulator and still handed to the
  caller (B1).
- **The first chunk that carries text or a tool-call delta calls
  `mark_first_token`.**
- **The span is finished exactly once**, when the stream ends, when it raises,
  or when the caller stops reading early (a `break`, a cancelled coroutine, a
  dropped stream, a `Close`). Finishing applies `record_response` to a response
  built from what was collected, sets the output, and ends the span. A stream
  that raised also gets `set_error` with the error's message, and the error
  reaches the caller unchanged: it is the provider's error, not
  instrumentation's. An error the caller's own code raises while reading is
  an early stop, not a failed call, and ends the span with no error.
- **Memory is bounded.** The collected text and the tool calls' arguments
  share one budget of 32,768 Unicode code points. What arrives past it is not
  kept, and `mcpd.stream.truncated = true` is set on the span. At most 64 tool
  calls are kept; later ones are ignored and set the same attribute.

What is collected, by chunk shape:

| Shape | Recognised by | Collected |
| --- | --- | --- |
| OpenAI chat completions | a `choices` key holding a list, or null (a typed chunk with no choices can serialise them that way) | `id`, `model`, `service_tier` (first non-empty wins); text from `choices[0].delta.content`; tool calls from `choices[0].delta.tool_calls`, keyed by `index`, with `id` and `function.name` set once and `function.arguments` appended; each choice's `finish_reason`, by choice index; `usage` from the chunk that carries one (the final chunk under `stream_options.include_usage`) |
| Anthropic messages | a `type` of `message_start`, `content_block_start`, `content_block_delta` or `message_delta` | `id`, `model` and `usage` from `message_start.message`; a `tool_use` block from `content_block_start`, keyed by `index`; text from a `text_delta` and arguments from an `input_json_delta`; `stop_reason` from `message_delta.delta`; and each usage field in `message_delta.usage` replaces the one from `message_start` |

The response handed to `record_response` is shaped like the provider's own:
`{"object": "chat.completion", id, model, service_tier, choices: [{finish_reason}], usage}`
for OpenAI and `{"type": "message", id, model, stop_reason, usage}` for
Anthropic, each field only when it was seen. So the model, the provider, the
usage and the `gen_ai.response.*` attributes come out exactly as they would for
the same call made without streaming.

The output is the text when no tool call arrived, and
`{"content": text, "tool_calls": [{id, name, arguments}]}` when one did, with
`arguments` the collected string. It goes through `set_output`, so
`capture_payloads = false` keeps it in the process like any other output. A
stream that carried neither text nor a tool call sets no output.

## 4. The other four signals

```
log(level, message, fields?)                 # correlated to the active span
metric(name, value, kind, labels?, unit?)
score(trace_id|span_id, scorer_id, value, min, max, reason?)
feedback(trace_id|span_id, kind, value, comment?)
```

`score` requires `min` and `max`: a bare number with no declared scale cannot be
compared to anything, and every SDK enforces this at the call site.

A score sent with no `source` is `live`. This was not written down anywhere, and
one SDK defaulted to `offline` for exactly that reason, so the same call
filtered differently depending on the language that made it. The
cross-language suite now asserts it, but the contract is where it should have
been settled first.

A span inherits its parent's correlation, and a log emitted inside a run carries
that run's correlation rather than the client's. Anything set on a run after it
starts still reaches steps opened afterwards. Three SDKs read correlation from
the client at construction instead, so a session or user set on the run never
reached anything under it.

A correlation field that is present with no value never erases one that has a
value. A log line and a metric point take the run's correlation when a run is
active and the client's when none is, merged field by field, and a field the
nearer source leaves empty falls through to the further one. The three
languages that can carry a key that is present and empty (`undefined` in
TypeScript, `None` in Python, `nil` in Elixir) must not let it through as an
erasure. A log line that loses its session is a log line nobody can find by
session again, and the same call in another language kept it.

### 4.1 Structured fields on a log line

`fields` is a map of whatever the caller has to hand, and two values in it have
cost more than themselves:

- **An error is serialised, never collapsed.** A value that is the language's
  error type (a JavaScript `Error`, a Python `BaseException`, a Go `error`, a
  Kotlin `Throwable`, an Elixir exception struct) is written as an object with
  `type` and `message`, plus `stack` where the language carries one on the value
  itself. A plain JSON encode is not enough: it turns a JavaScript `Error` into
  `{}`, because `message` and `stack` are not enumerable properties, and in
  Python it raises. Go and Elixir report `type` and `message` only, because an
  error value in neither language carries a stack; the field is left out rather
  than filled with the stack of whatever goroutine or process happened to be
  logging. An error nested inside a list or a map is serialised the same way,
  and so is one under a metric's labels.
- **A value that cannot be serialised costs itself and nothing else.** Under
  the same key it becomes a string that names it: its own string form where the
  language has a safe one, and its type where printing the value could itself go
  wrong, as it can in Go on a structure that points at itself. Losing one
  field is a small thing. Losing the batch it travelled in takes every span,
  metric, score and piece of feedback that shared it, and that is B1 broken in
  the quietest possible way: the process survives and the data does not.
- **A number JSON cannot carry costs itself too.** NaN and the two infinities
  have no JSON literal. An encoder either refuses the whole batch over one, or
  writes a `null` where the daemon needs a number and the daemon refuses the
  batch instead. In a field map, labels, metadata, attributes, input or output
  one becomes a string that names it, in the language's own spelling (`NaN`,
  `Infinity`, `+Inf`). As a metric's value, or a score's value, `min` or `max`,
  it is the signal itself that cannot be sent: that one signal is dropped at
  the call and reported through the diagnostics channel, never raised. As an
  optional setting, such as a model parameter, it is left out as though it had
  not been given. Elixir has no such float and is exempt.

Rust is the exception to the shape of this, not to the rule: its `fields` are
already a `serde_json::Value`, so nothing unserialisable can reach the queue.
It exposes the serialiser itself as `error_fields`, and `log_error` puts the
result under the `error` key, so an error goes on the wire in the same shape as
everywhere else. It adds one key the others cannot: `causes`, the chain of
`source` errors, which is what a Rust error carries instead of a stack.

### 4.2 The level floor

`log_level` is the lowest level the client records. A line below the floor is
dropped at the call, before the record is built, so a `debug` line in a
production process costs one comparison and no allocation.

The default floor is `debug`. That records every level the spec defines except
`trace`, whose own definition in the spec says it is off by default; the floor
is what makes that true. Set `log_level` to `trace` to record everything.
`MCPD_LOG_LEVEL` sets the same field, and a name outside the enum is ignored
rather than treated as an error, because a typo in a deployment variable must
not silence a service.

The floor applies to `log` alone. It never filters a span, a metric, a score or
a piece of feedback, and it never applies to what the read API returns: a
client with a floor of `error` still reads `debug` lines another process wrote.

### 4.3 OpenTelemetry severity numbers

Both directions, in every SDK:

```
severity(level) -> int                  # trace 1, debug 5, info 9,
                                        # warn 13, error 17, fatal 21
log_level_from_severity(int) -> level   # 1-4 trace, 5-8 debug, 9-12 info,
                                        # 13-16 warn, 17-20 error, 21 up fatal
```

The numbers are the spec's own: `LogLevel` carries a `severity` for each value
and five of the six SDKs already generate it. The ranges are the daemon's, and
they are what an OTLP `LogRecord` arriving at the door is read by, so an SDK
that mapped them differently would disagree with mcpd about the same number.
A number below 1 reads as `trace` and one above 24 as `fatal`, so a sender's
out-of-range value still becomes a level instead of a rejected record.

## 5. Query surface

Read methods mirror the HTTP API one-for-one, returning the generated types:

```
list_traces(filters) -> TraceListResponse
get_trace(trace_id, include?) -> TraceDetail
list_spans(filters) -> SpanListResponse
list_logs(filters) -> LogListResponse
query_metrics(name, from, to, ...) -> MetricSeriesResponse
list_scores(filters) -> ScoreListResponse
list_feedback(trace_id, limit?) -> FeedbackListResponse
analytics_summary(from, to, group_by?) -> AnalyticsSummary
analytics_cost(from, to, group_by?, bucket?) -> CostBreakdown
health() -> HealthStatus
```

Three more are part of this contract and are **not implemented in any SDK yet**.
They are written down here rather than left out, because the obligation is the
contract's and a surface the daemon serves that no client can reach is a gap
somebody has to be able to find:

```
capabilities() -> StoreCapabilities
discover(facet, key?, prefix?, limit?) -> DiscoveryResponse
poll_deltas(signal?, cursor?, since?, limit?) -> DeltaResponse
```

Two rules go with them when they are added. `capabilities()` is read once at
connect time, not per request: it describes the deployment and never changes
while the daemon is up. And a `501` from `discover` or `poll_deltas` is never
retried. It carries a `capability` field naming a field of `StoreCapabilities`,
and it means this backend will never answer, so a client that retries it retries
for ever.

A list method must send only the parameters the endpoint documents. The daemon
refuses a parameter it does not apply, naming it, rather than dropping it, so a
client that serialises a filter the endpoint has never had turns a silent wrong
answer into a `400`. Five clients once sent `kind`, `from`, `to` and `cursor` to
`list_feedback`, which reads one run's feedback and nothing else. Every SDK now
takes a required trace id, an optional `limit` and the project, and sends
nothing more; a call with no trace id is refused before it reaches the wire.

A list parameter goes on the wire once, as one comma-separated value:
`tags=paid,eu`, never `tags=paid&tags=eu`. Every endpoint splits a list value on
commas, and most keep only the last of a repeated key, so the repeated form
answers for one tag instead of all of them, with a 200.

The metric `filter` is label equality pairs in the daemon's own spelling,
`key:value` joined by commas: `filter=model:gpt-4o,status:ok`. A JSON object in
that place matches no point, and the daemon answers 200 with an empty series.

## 6. Wire format

Batches go to `POST {endpoint}/v1/ingest` as `IngestBatch`, gzip-compressed
above 1 KiB, with `Authorization: Bearer {api_key}` when a key is set and an
`ingest_key` that is stable per batch so a retry cannot double-count.

Retry on 429, 502, 503, 504 and on a connection error: three attempts, exponential
backoff starting at 250 ms with jitter, honouring `Retry-After`. Never retry a
4xx other than 429: the batch is malformed and will stay malformed.

## 7. Run surface (workflows and workers)

Every SDK serves durable runs against the daemon's run surface, described in
`internal/run-contract-v1.md` and typed in `spec/mcpd.spec.json` (tags Runs and
Workers) and `spec/definition.schema.json`. The daemon owns the run between
steps; the user's process owns everything inside a step. No SDK sends a model
name, a prompt or a credential to the daemon as part of a definition.

### 7.1 The builder

A `workflow(id, version?)` builder produces the definition document the daemon
stores, in the daemon's own spelling (`snake_case`, opaque to the wire
conversion), and keeps the step bodies in the process. Every construct has a
method, named in the language's idiom:

```
step(id, body, options)         claimable, kind task
agent(id, body, options)        claimable, kind agent
sleep(id, ms)                   held by the daemon
sleep_until(id, at_template)    held by the daemon
wait_for(id, event, key_template, timeout_ms, on_timeout?, accept_parked?)
parallel(id, branches)          one claimable step per branch
foreach(id, over, max_items, concurrency?, body)
loop(id, max_iterations, until?, body)
subworkflow(id, definition, version?, input?)
output(template)
```

Step options: `when` (a predicate), `input` (a template), `effect`
(`at_least_once` default, `at_most_once`), `step_timeout_ms`, `retry`
(`max_attempts`, `backoff`, `initial_ms`, `max_ms`) and `on_error`
(`fail_run` default, `continue`). Predicates are built with `when(path, op,
value?)`, `all`, `any`, `not`. A foreach or loop body is exactly one claimable
step; the builder refuses more. An id that fails `^[a-z][a-z0-9_]*$` is refused
before anything reaches the daemon.

The builder resolves a lease's step key to the body that registered it:
`classify`, `checks/fraud`, `notify[3]` and `refine[1]` each find their handler.

### 7.2 The worker

A `Worker(client, options)` takes `worker_id`, `concurrency`, `project`,
`wait_ms` and `on_error`, registers the workflows it holds, and serves:

1. `POST /v1/api/workers/register` with every definition, reading back
   `heartbeat_ms` (never below one second).
2. `POST /v1/api/steps/claim` in a loop, long-polling `wait_ms`, for at most
   the free concurrency. The claim request's timeout is `wait_ms` plus the
   client's `timeout`, and nothing else about the transport changes: the
   daemon may hold the request open for the whole of `wait_ms`, so a poll longer
   than the request timeout would be cancelled by the client it was made for.
   The daemon may also answer sooner, with no leases: it holds a claim for at
   most four fifths of its own request timeout, and answers at once when it is
   told to stop. An empty answer means claim again, never an error.
3. Per lease: `started` before the body, always; a heartbeat every
   `heartbeat_ms` while the body runs; then `complete` with the output, the
   merged state patch and the usage, or `fail` with the error's class.
4. A lease for a definition, version or step key the worker does not hold is
   failed with class `unservable` and the body never runs.
5. A lease that carries `deadline_ms` starts a local deadline at `started`.
   When it passes, the body is cancelled through the same signal a fence uses
   and the attempt is failed `retryable` with the message
   `step deadline of <n>ms passed`. The daemon enforces the same ceiling after
   the fact; the worker enforces it first so the slot is freed and the journal
   says why. The slot is freed at the deadline whether or not the body heeds
   the signal: the worker stops waiting for it, and what it returns or raises
   afterwards is ignored. A fence frees the slot the same way. A body that
   ignores the signal keeps running where the language cannot stop it, but it
   no longer holds a slot.

The 409 table is applied by reason and never collapsed: `already_applied` is a
success and nothing more is posted; `fenced` and `run_terminal` abandon the
lease silently; `not_started` is reported through `on_error` as a bug in the
worker. A 409 on a heartbeat aborts the body's cancellation signal and stops the
heartbeat. A heartbeat that fails for any other reason is reported and the next
one is tried.

`worker.claim(max, run_id?)` serves one claim, so a script can drive one run to
its end; `serve()` loops until `stop()`, which waits for the bodies in flight.

A caller's `on_error` sees every failure the worker reports. Without one, the
worker writes each kind of failure (a claim, a heartbeat, a worker call, a hook)
at most once per interval, as B1 asks of instrumentation. The limit is keyed by
the kind and never by the message: a message names its step, and a key that
held the step let every step of a failing run through, one line each.

A worker is a process someone operates, so it can be observed from inside.
Four hooks sit beside `on_error`, each optional:

```
on_lease(lease)                 a lease was claimed, before anything is posted
on_step_start(ctx)              `started` was accepted and the body is about to run
on_step_end(ctx, outcome)       the body finished: completed, or failed with a class
on_conflict(lease, reason)      a 409 was applied, with the daemon's reason
```

Hooks are the user's code, so they follow the recording rule: each call is
wrapped, a hook that throws is reported through `on_error` and the loop goes
on, and no hook is awaited on the completion path. `worker.stats()` answers
`claimed`, `inflight`, `completed`, `failed` (a map by error class),
`conflicts` (a map by 409 reason) and `unservable`, since a `fenced` lease
abandoned in silence looks exactly like no work at all from outside.

### 7.3 The step context

A body receives a context, its rendered input and the prior steps. The context
carries the lease, `run_id`, `step_key`, `attempt`, `idempotency_key`, the
run's `state`, `set_state(patch)` (top-level keys replace on `complete`),
`set_usage(usage)` (the daemon prices it), a cancellation signal that fires
when the lease is lost or the run ends, and `with_span` (or the language's
scoped equivalent) that records a span under the step span the daemon opened,
with the lease's `trace_id` and `span_id` as the parent.

A span the body records through the context, and every span nested under it,
adds its usage to the step's usage when it ends: `input_tokens`,
`output_tokens` and `total_tokens` are summed, and the last `model` and
`provider` seen win. A body that records its model calls as spans therefore
never sums tokens by hand, and `limits.max_cost_usd` fires on what the trace
shows. `set_usage` is an explicit override: the fields it sets replace the
accumulated ones, so a body that records no spans reports what it sets. The
daemon prices the step's usage onto the attempt and the run, and writes it on
the step span as `attributes.usage` and `attributes.cost_usd` rather than as
the span's own usage, so a trace total counts the model spans that spent the
tokens once and never the container that summed them.

A body signals a retry by raising the SDK's retryable error, which carries an
optional `retry_after_ms`; a typed step failure names its class; any other
exception is `fatal`. The message is sent; a stack trace is not.

The context decodes the input, the prior steps and the state on request:
`input`, `prior` and `state` take a type in Rust and Kotlin, a target value in
Go, and a decoder function in TypeScript, Python and Elixir (`state_as` where
`state` is already the raw property). A decoder that refuses the value fails
the step `invalid_input` with the message
`the step's <input|prior|state> could not be read: <reason>`, never the body's
own exception: the value was rendered by the daemon from a template, so a
shape the body did not expect is a template mistake, and one attempt is the
right thing to charge for it.

### 7.4 The run API

`runs.start`, `runs.get`, `runs.list`, `runs.cancel`, `runs.events`,
`runs.watch` (every event from `after` exclusive until `run_ended`, by polling
or the SSE stream), `runs.wait`, `runs.start_and_wait`, `runs.signal`,
`events.publish`, `definitions.put`, `definitions.get`, `definitions.list`,
and the worker calls under `steps`, each mirroring its endpoint one for one.
Run calls throw, like reads. A 409 from a worker call is thrown as a typed
conflict carrying the daemon's reason.

`wait` follows the journal to `run_ended` and answers a run result: the
status, the output, the cost, the steps used, the trace id and the run in
full. A run that ended `failed`, `timed_out` or `cancelled` is a typed
failure carrying the run (`RunFailedError`, `Error::RunFailed`,
`*RunFailedError`, `RunFailedException`, `{:error, {:run_failed, run}}`); a
run still going after the caller's timeout is the language's ordinary timeout
error. `start_and_wait` is the two in one call.

### 7.5 Tests every SDK carries for this surface

- The definition document for a workflow using every construct, compared to
  the expected wire object.
- Id validation and the one-body rule for foreach and loop.
- Handler resolution for a plain, branch, item and iteration key.
- One lease through claim, started, complete with output, state and usage on
  the wire, and the body seeing input, prior and state unconverted.
- A retryable error, a fatal throw, a typed class, and an unservable lease.
- All four 409 reasons on `complete` or `started`, and a fenced heartbeat.
- The serve loop registering, claiming while idle, and stopping.
- A lease with `deadline_ms` whose body runs past it: cancelled, failed
  `retryable`, no `complete`; one whose body ignores the signal, its slot
  freed at the deadline all the same; and one that finishes in time, left
  alone.
- A decoder that refuses the input: the step fails `invalid_input`.
- `wait`: the output once the run ends, the typed failure for a run that did
  not succeed, the timeout while it is still going, and `start_and_wait`.
- Usage accumulation: a body that sets usage on two spans under the context
  and none on the context completes with the sum on the wire and the last
  model seen; a body that also calls `set_usage` completes with what it set.
- The four hooks each called once with what the contract says, a hook that
  throws reported through `on_error` with the lease still completed, and
  `stats()` after a completed step, a fatal failure and a fenced conflict.
- A live test that registers a workflow, starts a run, serves it, and reads
  the run, its events and its trace back, plus a wait answered by `publish`
  and a run ended by `cancel`.

## 8. What every SDK ships

- A README whose first code block is a complete working example.
- Types generated from the spec (never hand-written).
- Tests for: batching, the queue's bound, retry, `traceparent` round-tripping,
  `enabled=false`, and that a dead server never raises.
- Tests for: `capture_payloads=false` from code and from the environment
  leaving `input` and `output` off a span, an event and a recorded span while
  the rest of the record survives, and the per-span override in both
  directions; `record` queuing a well-formed span unchanged and refusing each
  malformed shape by counting rather than raising; `external_parent_span_id`
  reaching the wire; a `RecordingSink` seeing exactly the signals recorded; a
  sink that throws counted as failed with the caller never seeing it; and
  `stats()` after each of those.
- The run surface of section 7, with the tests listed there.
- Tests for the logging signal of section 4: an error under a field key
  reaching the wire as `type`, `message` and (where the language has one)
  `stack`; a value nothing can serialise costing its own key and leaving the
  rest of the batch on the wire; a line below the floor never queued while the
  line above it is, from code and from `MCPD_LOG_LEVEL`; the severity mapping
  in both directions across every level; and, in the languages that can carry a
  key with no value, that key falling through to the run's own value rather
  than erasing it.
- Tests for `record_stream` (section 3.2): an OpenAI stream and an Anthropic
  stream each recording the model, usage, id, finish reason and output that
  `record_response` and `set_output` would; tool-call deltas joined; every
  chunk passed through unchanged, an unknown one included; the first token
  marked once; a stream that raises ending the span with an error and the
  error reaching the caller; a caller that stops early still ending the span;
  and text past the budget cut with `mcpd.stream.truncated` set.
- Tests for the test client (section 2.5): spans, logs, scores and feedback
  read back with no flush; one record per span id; `span(name)` failing on
  none and on two; the tree; `reset()`; `redact` applied; and `enabled` and
  `sample` holding against the environment.
- An example under `examples/<language>/` that the docs include **by reference**.
