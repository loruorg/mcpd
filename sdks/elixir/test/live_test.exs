# End-to-end against a running mcpd.
#
# Skipped unless an endpoint is configured, because these need a server. Start
# one and point them at it:
#
#     mcpd demo --data /tmp/mcpd-live.db --listen 127.0.0.1:4317
#     MCPD_TEST_ENDPOINT=http://127.0.0.1:4317 mix test
#
# What they prove that behaviour_test.exs cannot: that the daemon accepts the
# exact shapes this SDK sends, and that what comes back out is what went in.
# Every other test in this package asserts against code this repository also
# wrote, so the two can agree with each other and both be wrong about the
# daemon.
#
# `MCPD_REQUIRE_LIVE=1` turns a missing endpoint into a failure rather than a
# skip. CI sets it in the job that starts a daemon, so if the variable is ever
# renamed or stops reaching the tests the job goes red instead of quietly
# skipping. The Rust SDK's live suite spent a long time reporting passes having
# contacted no server at all, and this is the guard against a repeat.

if System.get_env("MCPD_TEST_ENDPOINT") in [nil, ""] and
     System.get_env("MCPD_REQUIRE_LIVE") not in [nil, ""] do
  defmodule Mcpd.LiveRequiredTest do
    use ExUnit.Case, async: true

    test "the live suite was required to reach a daemon" do
      flunk(
        "MCPD_REQUIRE_LIVE is set but MCPD_TEST_ENDPOINT is not, so this suite " <>
          "would have skipped without reaching a server. That is the exact " <>
          "silence this variable exists to break."
      )
    end
  end
end

defmodule Mcpd.LiveTest do
  use ExUnit.Case, async: false
  use Mcpd

  alias Mcpd.{Context, Spec}

  @endpoint System.get_env("MCPD_TEST_ENDPOINT")

  # Reported as skipped rather than silently absent, so a run with no server
  # says which tests did not happen instead of printing a green summary that
  # covered nothing.
  if @endpoint in [nil, ""], do: @moduletag(:skip)

  # One string for both the raise and the assertion, so the message recorded
  # and the message expected cannot drift apart.
  @gateway_error "sms gateway timed out after 5s"

  test "a whole run survives the round trip, with its tokens, cost, error and session intact" do
    client = client("sdk-live-elixir")

    {:ok, health} = Mcpd.health(client)
    assert health.status == "ok"

    # A compatibility check, not a presence check. A daemon built from a
    # different spec fails here, at connect, rather than downstream on a field
    # that silently is not there.
    assert health.spec_version == Spec.spec_version()

    trace_id =
      Mcpd.span client, "support_agent",
        kind: :agent_run,
        input: "where is my order?",
        output: "It shipped on Tuesday.",
        tags: ["live-test"],
        session_id: "session-live-elixir" do
        Mcpd.span client, "answer",
          kind: :model_generation,
          model: "claude-sonnet-4",
          provider: "anthropic",
          usage: %{"input_tokens" => 1_000, "output_tokens" => 500} do
          :answered
        end

        # The raise reaches the caller unchanged. mcpd records what happened and
        # never decides what the agent does about it.
        assert_raise RuntimeError, @gateway_error, fn ->
          Mcpd.span client, "notify_customer", kind: :tool_call do
            raise @gateway_error
          end
        end

        Mcpd.log(client, :info, "run finished")
        Mcpd.metric(client, "orders.looked_up", 1, kind: :counter)
        Context.current().trace_id
      end

    Mcpd.score(client, trace_id, "answer_relevance", 0.9, min: 0, max: 1)
    Mcpd.feedback(client, trace_id, :thumbs, "up")

    # `flush/1` returns `:ok` even when delivery failed, because B1 forbids it
    # from raising. What proves delivery is the read below, never this call.
    Mcpd.flush(client)

    {:ok, %Mcpd.TraceDetail{} = detail} =
      Mcpd.get_trace(client, trace_id, include: ["logs", "scores", "feedback"])

    trace = detail.trace

    assert trace.name == "support_agent"
    assert trace.kind == :agent_run
    assert trace.error_count == 1
    assert trace.tool_count == 1
    assert trace.model_count == 1
    assert trace.tags == ["live-test"]
    assert trace.correlation.session_id == "session-live-elixir"
    assert trace.usage.input_tokens == 1_000
    assert trace.usage.output_tokens == 500
    assert %DateTime{} = trace.start_time

    # mcpd priced the model call from its own price book at ingest. This SDK
    # sends tokens and never a cost, so a cost here can only have been computed
    # by the daemon, which is the point of asserting it.
    #
    # The `is_number` check is not ceremony. An absent cost reads as `nil`, and
    # `nil > 0` is true under Elixir's term ordering, so on its own the
    # comparison below would pass against a daemon that priced nothing.
    assert is_number(trace.cost.total_cost)
    assert trace.cost.total_cost > 0

    spans = Map.new(detail.spans, &{&1.name, &1})
    assert spans |> Map.keys() |> Enum.sort() == ["answer", "notify_customer", "support_agent"]
    assert spans["answer"].parent_span_id == trace.root_span_id
    assert spans["answer"].kind == :model_generation
    assert spans["notify_customer"].status == :error
    assert spans["notify_customer"].error.message == @gateway_error
    assert spans["notify_customer"].error.kind == "RuntimeError"

    assert [%Mcpd.LogRecord{message: "run finished"}] = detail.logs
    assert [%Mcpd.Score{scorer_id: "answer_relevance", source: :live}] = detail.scores
    assert [%Mcpd.Feedback{kind: :thumbs, value: "up"}] = detail.feedback
  end

  test "the read surface answers, and finds the run just written" do
    project = "sdk-live-elixir-read"
    client = client(project)

    trace_id =
      Mcpd.span client, "readable run", kind: :agent_run, input: "q", output: "a" do
        Mcpd.span client, "step", kind: :workflow_step do
          Mcpd.log(client, :warn, "a step to find")
          Mcpd.metric(client, "orders.looked_up", 1, kind: :counter)
        end

        Context.current().trace_id
      end

    Mcpd.score(client, trace_id, "answer_relevance", 0.5, min: 0, max: 1)
    Mcpd.feedback(client, trace_id, :thumbs, "down")
    Mcpd.flush(client)

    {:ok, %Mcpd.TraceListResponse{traces: traces}} = Mcpd.list_traces(client, name: "readable run", limit: 50)

    assert Enum.any?(traces, &(&1.trace_id == trace_id)),
           "the run just written was not in its own project's listing (#{length(traces)} traces)"

    {:ok, %Mcpd.SpanListResponse{spans: spans}} = Mcpd.list_spans(client, trace_id: trace_id)
    assert spans |> Enum.map(& &1.name) |> Enum.sort() == ["readable run", "step"]

    {:ok, %Mcpd.LogListResponse{logs: logs}} = Mcpd.list_logs(client, trace_id: trace_id)
    assert [%Mcpd.LogRecord{message: "a step to find", level: :warn}] = logs

    {:ok, %Mcpd.ScoreListResponse{scores: scores}} = Mcpd.list_scores(client, trace_id: trace_id)
    assert [%Mcpd.Score{scorer_id: "answer_relevance", value: 0.5}] = scores

    {:ok, %Mcpd.FeedbackListResponse{feedback: feedback}} = Mcpd.list_feedback(client, trace_id)
    assert [%Mcpd.Feedback{kind: :thumbs, value: "down"}] = feedback

    from = DateTime.add(DateTime.utc_now(), -3600, :second)
    to = DateTime.add(DateTime.utc_now(), 60, :second)

    {:ok, %Mcpd.MetricSeriesResponse{} = series} = Mcpd.query_metrics(client, "orders.looked_up", from, to)
    assert series.name == "orders.looked_up"

    {:ok, %Mcpd.AnalyticsSummary{} = summary} = Mcpd.analytics_summary(client, from, to, group_by: "model")
    assert summary.trace_count > 0

    {:ok, %Mcpd.CostBreakdown{} = cost} = Mcpd.analytics_cost(client, from, to, group_by: "model", bucket: "1h")
    assert cost.currency == "USD"
  end

  test "an unfinished run is visible before it ends, and complete after" do
    client = client("sdk-live-elixir-open")

    trace_id =
      Mcpd.span client, "long running", kind: :agent_run do
        Mcpd.flush(client)
        trace_id = Context.current().trace_id

        {:ok, open} = Mcpd.get_trace(client, trace_id)
        assert [%Mcpd.Span{end_time: nil, status: :unset}] = open.spans
        refute open.trace.is_complete

        trace_id
      end

    Mcpd.flush(client)
    {:ok, closed} = Mcpd.get_trace(client, trace_id)
    assert [%Mcpd.Span{end_time: %DateTime{}, status: :ok}] = closed.spans
    assert closed.trace.is_complete
  end

  test "a trace continues across a service boundary through a traceparent header alone" do
    project = "sdk-live-elixir-ctx"
    caller = client(project)
    callee = client(project)

    {trace_id, header} =
      Mcpd.span caller, "caller", kind: :agent_run do
        {Context.current().trace_id, Mcpd.traceparent()}
      end

    assert {:ok, %{trace_id: ^trace_id, span_id: caller_span_id}} =
             Context.parse_traceparent(header)

    # A separate process, because the active span lives in the process
    # dictionary and does not cross a process boundary. Anything the callee
    # joins here, it joined from the header and from nothing else.
    Task.await(
      Task.async(fn ->
        Mcpd.continue(header, fn ->
          Mcpd.span callee, "callee", kind: :tool_call do
            :ok
          end
        end)
      end)
    )

    Mcpd.flush(caller)
    Mcpd.flush(callee)

    {:ok, detail} = Mcpd.get_trace(caller, trace_id)
    assert detail.spans |> Enum.map(& &1.name) |> Enum.sort() == ["callee", "caller"]

    child = Enum.find(detail.spans, &(&1.name == "callee"))
    assert child.trace_id == trace_id
    assert child.parent_span_id == caller_span_id
  end

  test "a batch over a kibibyte is gzipped and the daemon reads it" do
    client = client("sdk-live-elixir-gzip")
    prompt = String.duplicate("where is my order? ", 400)

    trace_id =
      Mcpd.span client, "big prompt", kind: :agent_run, input: prompt do
        Context.current().trace_id
      end

    Mcpd.flush(client)

    {:ok, detail} = Mcpd.get_trace(client, trace_id)
    assert [%Mcpd.Span{input: ^prompt}] = detail.spans
  end

  # ------------------------------------------------------------- helpers ---

  # A project of its own per test, so one test cannot read another's runs and
  # a repeated run against the same daemon still counts what it wrote.
  defp client(project) do
    {:ok, pid} =
      Mcpd.start_link(
        endpoint: @endpoint,
        project: project,
        environment: "test",
        service_name: "mcpd-elixir-live",
        service_version: "0.1.0",
        batch_size: 1_000,
        # An hour. A test that forgets to flush then fails on its assertions
        # rather than passing on a timer that happened to fire first.
        flush_interval: 3_600_000
      )

    on_exit(fn -> Mcpd.shutdown(pid) end)
    pid
  end
end


defmodule Mcpd.LiveRunsTest do
  @moduledoc "The run surface against a real daemon: register, start, serve, read back."
  use ExUnit.Case, async: false

  alias Mcpd.{StepContext, StepWorker, Workflow}

  @endpoint System.get_env("MCPD_TEST_ENDPOINT")
  if @endpoint in [nil, ""], do: @moduletag(:skip)

  defp client do
    project = "sdk-live-elixir-runs-" <> String.slice(Mcpd.Ids.trace_id(), 0, 8)

    {:ok, pid} =
      Mcpd.start_link(
        endpoint: @endpoint,
        project: project,
        environment: "test",
        service_name: "mcpd-elixir-live",
        service_version: "0.1.0",
        batch_size: 1_000,
        flush_interval: 3_600_000
      )

    on_exit(fn -> Mcpd.shutdown(pid) end)
    pid
  end

  defp worker(client, wf, id) do
    {:ok, worker} =
      StepWorker.start_link(client,
        workflows: [wf],
        worker_id: id,
        wait_ms: 1_000,
        on_error: fn message, reason -> flunk("#{message}: #{inspect(reason)}") end
      )

    :ok = StepWorker.register_definitions(worker)
    worker
  end

  defp drive(client, worker, run_id) do
    {:ok, detail} = Mcpd.get_run(client, run_id)

    Enum.reduce_while(1..10, detail, fn _, detail ->
      if detail.run.status == :running do
        {:ok, _} = StepWorker.claim(worker, max: 4, run_id: run_id)
        :ok = StepWorker.drain(worker, 10_000)
        {:ok, detail} = Mcpd.get_run(client, run_id)
        {:cont, detail}
      else
        {:halt, detail}
      end
    end)
  end

  test "a workflow is served, and the run and its trace read back" do
    client = client()

    wf =
      Workflow.new("live_support")
      |> Workflow.step("classify", fn _ctx, input, _prior ->
        %{"kind" => if(String.starts_with?(input["ticket"], "Q"), do: "question", else: "bug")}
      end)
      |> Workflow.agent(
        "draft",
        fn ctx, input, _prior ->
          StepContext.set_usage(ctx, input_tokens: 120, output_tokens: 40)
          StepContext.set_state(ctx, %{"drafted" => true})

          StepContext.span(ctx, "live_model", [kind: :model_generation, model: "claude-sonnet-4", provider: "anthropic",
            usage: %{"input_tokens" => 120, "output_tokens" => 40}], fn -> :ok end)

          "an answer to a #{input["kind"]}"
        end,
        input: %{"kind" => "${steps.classify.output.kind}"}
      )
      |> Workflow.output(%{"answer" => "${steps.draft.output}"})

    worker = worker(client, wf, "live-worker")
    {:ok, run} = Mcpd.start_run(client, wf, input: %{"ticket" => "Q-42"})
    assert run.status == :running
    assert run.definition_id == "live_support"

    detail = drive(client, worker, run.run_id)
    assert detail.run.status == :succeeded, inspect(detail.run.error)
    assert detail.run.output == %{"answer" => "an answer to a question"}
    assert detail.run.state == %{"drafted" => true}
    draft = Enum.find(detail.attempts, &(&1.step_key == "draft"))
    assert draft.status == :succeeded
    assert draft.kind == :agent
    assert draft.input == %{"kind" => "question"}
    assert draft.usage.input_tokens == 120

    kinds = client |> Mcpd.watch_run(run.run_id, poll_ms: 50) |> Enum.map(& &1.kind)
    assert hd(kinds) == :run_started
    assert List.last(kinds) == :run_ended

    :ok = Mcpd.flush(client)
    # The daemon writes the run's own spans through an outbox drained on its
    # own clock, so give it a moment before reading the trace.
    trace =
      Enum.find_value(1..40, fn _ ->
        case Mcpd.get_trace(client, run.trace_id) do
          {:ok, t} ->
            if length(t.spans) >= 4 and Enum.any?(t.spans, &(&1.name == "live_model")), do: t, else: Process.sleep(250)

          _ ->
            Process.sleep(250)
            nil
        end
      end)

    assert trace, "the run's trace with the body's span in it"
    model = Enum.find(trace.spans, &(&1.name == "live_model"))
    assert model.parent_span_id == draft.span_id
  end

  test "a wait resumes on publish, and another run ends on cancel" do
    client = client()

    wf =
      Workflow.new("live_approval")
      |> Workflow.wait_for("approval", event: "approved", key: "${input.ticket}", timeout_ms: 60_000)
      |> Workflow.step("close", fn _ctx, input, _prior -> %{"closed_by" => input["by"]} end, input: %{"by" => "${steps.approval.output.by}"})
      |> Workflow.output("${steps.close.output}")

    worker = worker(client, wf, "live-approver")
    {:ok, waiting} = Mcpd.start_run(client, "live_approval", input: %{"ticket" => "T-7"})

    detail =
      Enum.reduce_while(1..10, nil, fn _, _ ->
        {:ok, d} = Mcpd.get_run(client, waiting.run_id)
        if d.waits == [], do: (Process.sleep(100); {:cont, d}), else: {:halt, d}
      end)

    assert Enum.map(detail.waits, &{&1.event, &1.key}) == [{"approved", "T-7"}]

    {:ok, receipt} = Mcpd.publish_event(client, "approved", "T-7", payload: %{"by" => "ops"}, event_id: "live-approval-elixir-1")
    assert receipt.outcome == :delivered
    {:ok, again} = Mcpd.publish_event(client, "approved", "T-7", event_id: "live-approval-elixir-1")
    assert again.outcome == :duplicate

    detail = drive(client, worker, waiting.run_id)
    assert detail.run.status == :succeeded, inspect(detail.run.error)
    assert detail.run.output == %{"closed_by" => "ops"}

    {:ok, doomed} = Mcpd.start_run(client, "live_approval", input: %{"ticket" => "T-8"})
    {:ok, cancelled} = Mcpd.cancel_run(client, doomed.run_id)
    assert cancelled.status == :cancelled
    assert client |> Mcpd.watch_run(doomed.run_id, poll_ms: 50) |> Enum.map(& &1.kind) |> List.last() == :run_ended

    {:ok, listed} = Mcpd.list_runs(client, definition_id: "live_approval")
    assert length(listed.runs) == 2
  end
end
