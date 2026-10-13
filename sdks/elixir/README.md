# mcpd for Elixir

Agent observability and durable runs for Elixir, with no dependencies: traces,
logs, metrics, scores and feedback sent to an [mcpd](https://mcpd.sh) daemon
on the HTTP client and JSON codec OTP already ships, and workflows served from
your own node. The daemon owns the run between steps; your code owns everything
inside one. Recording never blocks and never raises; reads and run calls answer
`{:ok, value}` or `{:error, reason}`.

```elixir
{:ok, mcpd} = Mcpd.start_link(project: "support")
use Mcpd

trace_id =
  Mcpd.span mcpd, "support_agent", kind: :agent_run, input: "where is my order?", session_id: "sess-8121" do
    Mcpd.span mcpd, "plan", kind: :model_generation, model: "gpt-4o-mini", provider: "openai",
      usage: %{"input_tokens" => 812, "output_tokens" => 96} do
      "I should look the order up first."
    end

    Mcpd.log(mcpd, :info, "answered", %{"tool_calls" => 1})
    Mcpd.metric(mcpd, "support.answers", 1, kind: :counter, labels: %{"outcome" => "ok"})
    Mcpd.traceparent() |> String.split("-") |> Enum.at(1)
  end

Mcpd.score(mcpd, trace_id, "answer_relevance", 0.82, min: 0.0, max: 1.0)
Mcpd.feedback(mcpd, trace_id, :thumbs, "up")
:ok = Mcpd.flush(mcpd)
{:ok, detail} = Mcpd.get_trace(mcpd, trace_id, include: ["scores"])
IO.inspect(detail.trace.span_count)
Mcpd.shutdown(mcpd)
```

Start a daemon to send it to:

```console
$ mcpd demo --data ./mcpd.db
```

## Install

```elixir
{:mcpd, "~> 0.1"}
```

Elixir 1.17 or later on OTP 27 or later, because the SDK calls OTP 27's `:json` module.

## Configuration

`Mcpd.start_link/1` reads `MCPD_ENDPOINT`, `MCPD_API_KEY`, `MCPD_PROJECT`,
`MCPD_ENVIRONMENT`, `MCPD_SERVICE_NAME`, `MCPD_SERVICE_VERSION`,
`MCPD_SAMPLE`, `MCPD_BATCH_SIZE`, `MCPD_FLUSH_INTERVAL_MS`, `MCPD_MAX_QUEUE`,
`MCPD_TIMEOUT_MS`, `MCPD_ENABLED`, `MCPD_CAPTURE_PAYLOADS` and
`MCPD_LOG_LEVEL`; the same names
as keyword options win over the environment.

### Keep payloads in the process

`capture_payloads: false`, or `MCPD_CAPTURE_PAYLOADS=false`, keeps prompts and
completions where they are. Spans still go out with their timing, status,
usage, model, model parameters, correlation and errors; `input:`, `output:` and a block's
`capture_output:` value are dropped before they reach the queue, so nothing
sensitive is queued and nothing sensitive leaves on a flush. One span can say
otherwise for itself with `capture_payloads: true`, or hide itself on a client
that captures with `capture_payloads: false`.

### How the model was called

`model_parameters:` records the sampling settings a call was made with, as a
keyword list or a `%Mcpd.ModelParameters{}`:

```elixir
Mcpd.span mcpd, "answer",
  kind: :model_generation,
  model_parameters: [temperature: 0.0, seed: 0, max_tokens: 1024] do
  call_the_model()
end
```

They are settings, not payload, so they are kept even when
`capture_payloads: false` is in force, and they are what somebody reproducing
the run has to match. A field you leave out is absent; one you give is kept
whatever it is, so `seed: 0` is a seed. It is an option rather than a setter
because an Elixir span is described by the options it was opened with, the same
way `model:`, `provider:` and `usage:` already are.

### Assert what you recorded

A sink is where a drained batch goes. The HTTP transport is the default;
`Mcpd.Sink.Recorder` keeps every batch in memory, so a test reads back what
the instrumentation recorded without a daemon, and `Mcpd.Sink.Console` prints
one JSON line per signal to stderr.

```elixir
{:ok, recorder} = Mcpd.Sink.Recorder.start_link()
{:ok, mcpd} = Mcpd.start_link(sink: {Mcpd.Sink.Recorder, recorder})

Mcpd.span mcpd, "plan", kind: :model_generation, do: :ok
:ok = Mcpd.flush(mcpd)

[%{"name" => "plan", "status" => "ok"}] = Mcpd.Sink.Recorder.spans(recorder)
%{exported: 1, failed: 0, rejected: 0} = Mcpd.stats(mcpd)
```

A sink of your own implements `Mcpd.Sink` (`export/3`) and is given as
`{module, opts}`. One that raises is a failed batch, counted and reported
through the worker's throttle, never raised to the caller. `Mcpd.stats/1`
answers `queued`, `dropped`, `capacity`, `exported`, `failed`, `rejected` and
`redaction_failures`.

`Mcpd.flush/2` and `Mcpd.shutdown/2` return within their timeout, 10 seconds
by default, even when the daemon never answers. A flush that ran out of time
returns `{:error, :timeout}` and leaves the rest queued; a shutdown that ran out
of time starts no further batch and gives up what is still queued. The
worker's mailbox is bounded like its queue: while it holds `max_queue`
messages, a new signal is dropped at the call and counted under `dropped`.

### Record a span that already ended

`Mcpd.record/3` queues a `%Mcpd.Span{}`, or a map in the wire shape, as given:
its own ids, parent, start and end, never re-parented, re-timed or
re-sampled. That is what an adapter for another tracing system or a batch
import holds. Correlation fields the record leaves empty are filled from the
client; a record whose ids are not OpenTelemetry-shaped, whose name is empty
or whose `start_time` is missing is refused, counted under `rejected`, and
never raises. `external_parent_span_id:` on any span carries a parent from a
tracing system that is not mcpd, for correlation only.

## Durable runs

The same client drives the daemon's durable runs. A workflow is written once, in
code: the graph goes to the daemon as a definition, and the step bodies stay in
this node for a `Mcpd.StepWorker` to serve. The daemon holds the cursor, the
retries, the timers and the waits, so a run survives a deploy, a crash and a
week-long approval. No model, prompt or credential ever reaches the daemon.

```elixir
alias Mcpd.{StepContext, StepWorker, Workflow}
import Mcpd.Workflow, only: [where: 3]

support =
  Workflow.new("support_ticket")
  |> Workflow.step("classify", fn _ctx, input, _prior ->
    # Anything: your own model call, a database, an HTTP request.
    %{"kind" => if(String.starts_with?(input["ticket"], "Q"), do: "question", else: "bug")}
  end)
  |> Workflow.agent(
    "draft",
    fn ctx, input, _prior ->
      # Your own agent loop, as one durable step. A span from the context
      # lands under the step in the run's trace, and its usage is the step's.
      opts = [kind: :model_generation, model: "claude-sonnet-4", provider: "anthropic",
        usage: %{"input_tokens" => 812, "output_tokens" => 37}]
      StepContext.span(ctx, "model_call", opts, fn -> :ok end)

      "an answer to a #{input["kind"]}"
    end,
    input: %{"kind" => "${steps.classify.output.kind}"},
    retry: [max_attempts: 3, backoff: :exponential, initial_ms: 500]
  )
  |> Workflow.wait_for("approval",
    event: "approved",
    key: "${input.ticket}",
    timeout_ms: 7 * 24 * 3_600_000,
    when: where("steps.classify.output.kind", :eq, "bug")
  )
  |> Workflow.step(
    "send",
    fn ctx, input, _prior ->
      case deliver(input["text"], ctx.idempotency_key) do
        :ok -> %{"sent" => true}
        :busy -> raise Mcpd.StepException, message: "mail relay busy", class: :retryable, retry_after_ms: 2_000
      end
    end,
    input: %{"text" => "${steps.draft.output}"},
    effect: :at_most_once
  )
  |> Workflow.output(%{"sent" => "${steps.send.output.sent}"})

# Serve step bodies for as long as this node lives.
{:ok, worker} = StepWorker.start_link(mcpd, workflows: [support], concurrency: 8)
:ok = StepWorker.serve(worker)

# Start a run from anywhere that holds an execute key.
{:ok, run} = Mcpd.start_run(mcpd, support, input: %{"ticket" => "Q-42"})

# Answer the wait from a webhook or a console. A repeated event id is a duplicate.
{:ok, _receipt} = Mcpd.publish_event(mcpd, "approved", "Q-42", payload: %{"by" => "ops"}, event_id: "hook-1")

# Follow the journal to the end, then read the run.
mcpd |> Mcpd.watch_run(run.run_id) |> Enum.each(&IO.inspect({&1.seq, &1.kind}))
{:ok, detail} = Mcpd.get_run(mcpd, run.run_id)
IO.inspect({detail.run.status, detail.run.output})
```

What the pieces do:

- **`Mcpd.Workflow`** builds the definition. `step` and `agent` are claimable
  and carry a body; `sleep`, `sleep_until` and `wait_for` are held by the
  daemon with no process alive; `parallel`, `foreach` and `loop` fan out one
  claimable step per branch, item or iteration; `subworkflow` starts a child
  run. `input` templates and `when` guards (`where/3`, `exists/1`, `all_of/1`,
  `any_of/1`, `negate/1`) read `input`, `steps.<id>` and `run` with `${path}`
  references the daemon renders per attempt. An id the daemon would refuse
  raises as it is added.
- **`Mcpd.StepWorker`** registers the definitions, long-polls for leases and
  serves each one on a process of its own: `started`, heartbeats while the
  body runs, then `complete` or `fail`. The four conflicts the daemon can
  answer are handled by reason, so a call that landed twice is a success and a
  fenced lease is dropped, never reported as a failure. `claim/2` with
  `run_id:` serves one run from a script.
- **`Mcpd.StepContext`** carries the lease, `state/1`, `set_state/2` to merge
  keys into the run's state, `span/4` to nest spans under the step,
  `idempotency_key` for a third party that accepts one, and `cancelled?/1`,
  true once the lease is lost or the run ends. The body runs inside the step's
  trace context, so a plain `Mcpd.span` in it nests too.
- **Usage** is summed from the body's spans: every span the body closes in
  its own process adds its `usage:` to the step's, and the last `model:` and
  `provider:` seen win, so the daemon prices what the trace shows and
  `max_cost_usd` fires on it. `set_usage/2` is the explicit override; the
  fields it sets replace the accumulated ones.
- **Hooks and stats.** `on_lease`, `on_step_start`, `on_step_end` (the
  context and `:completed` or `{:failed, class, message}`) and `on_conflict`
  (the lease and the daemon's 409 reason) sit beside `on_error` in
  `StepWorker.start_link/2`; a hook that raises is reported and the loop goes
  on. `StepWorker.stats/1` answers `claimed`, `inflight`, `completed`,
  `failed` by class, `conflicts` by reason and `unservable`.
- **Raising** `Mcpd.StepException` with `class: :retryable` retries within the
  step's budget; any class can be named. Any other raise is `fatal`, and the
  step's `on_error` decides whether the run fails or moves on.
- **`Mcpd.start_run/3`**, **`get_run/2`**, **`list_runs/2`**,
  **`cancel_run/2`**, **`run_events/3`**, **`watch_run/3`** (a stream),
  **`signal_run/4`**, **`publish_event/4`**, **`put_definition/3`**,
  **`get_definition/3`**, **`list_definitions/2`** and the worker calls mirror
  the HTTP run surface one for one. `Mcpd.wait_run/3` and
  `Mcpd.start_run_and_wait/3` follow a run to its end and answer
  `{:ok, %Mcpd.RunResult{}}`, or `{:error, {:run_failed, run}}`. A 409 from a
  worker call answers `{:error, {:conflict, %Mcpd.StepConflict{}}}` with the
  daemon's reason.
- **A lease's deadline** is enforced by the worker itself: a body that runs
  past `deadline_ms` sees `cancelled?/1` turn true and the attempt fails
  `:retryable`. `StepContext.input/2`, `prior/2` and `state_as/2` check a
  value with a function of your own, and a value it refuses fails the step
  `:invalid_input`.

The wire protocol, the definition document and every closed enum are in
[`spec/definition.schema.json`](../../spec/definition.schema.json) and the
[run contract](../../internal/run-contract-v1.md).

## Reading

```elixir
{:ok, traces} = Mcpd.list_traces(mcpd, status: :error, from: "-1h", limit: 20)
{:ok, detail} = Mcpd.get_trace(mcpd, trace_id, include: ["logs", "scores"])
{:ok, summary} = Mcpd.analytics_summary(mcpd, "-24h", "-0s", group_by: "model")
```

Also `list_spans`, `list_logs`, `list_scores`, `list_feedback`, `query_metrics`,
`analytics_cost`, `get_trajectory`, `list_dataset_items`, `experiment_results`
and `health`.

## Experiments

`run_experiment` calls your function with each case of a dataset, each in a
root span carrying the experiment and case ids, then scores the answers.

```elixir
{:ok, results} =
  Mcpd.run_experiment(mcpd, "support-questions", &MyAgent.answer/1,
    name: "answers-v2",
    scorers: ["exact_match"]
  )
```

`create_experiment` and `score_experiment` are the steps it takes, for when
you run the cases yourself.

## Types

Every wire type is generated from `spec/mcpd.spec.json` into
`lib/mcpd/generated.ex`. Do not edit that file; edit the spec and run
`make gen`.

## Tests

```console
$ mix test
$ MCPD_TEST_ENDPOINT=http://127.0.0.1:4317 mix test
```

The suite runs offline; with an endpoint set, the end-to-end tests run too.
