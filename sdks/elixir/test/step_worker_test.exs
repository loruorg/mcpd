defmodule Mcpd.StepWorkerTest do
  @moduledoc "The worker loop and the run API against a scripted daemon (contract sections 7.2 to 7.4)."
  use ExUnit.Case, async: false

  alias Mcpd.{FakeDaemon, StepContext, StepException, StepWorker, Workflow}

  @trace "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"
  @step_span "b2b2b2b2b2b2b2b2"

  defp lease(overrides \\ %{}) do
    Map.merge(
      %{
        "lease_id" => "lease-1",
        "run_id" => "run-1",
        "project" => "test",
        "definition_id" => "support",
        "definition_version" => 1,
        "step_key" => "classify",
        "kind" => "task",
        "attempt" => 0,
        "effect" => "at_least_once",
        "expires_at" => "2026-09-01T10:00:00Z",
        "idempotency_key" => "run-1:classify:0",
        "input" => %{"ticket_id" => "T-1"},
        "prior" => %{"triage" => %{"status" => "succeeded", "output" => %{"queue_name" => "billing"}}},
        "state" => %{"seen_count" => 1},
        "correlation" => %{"project" => "test", "session_id" => "s-1"},
        "trace_id" => @trace,
        "span_id" => @step_span,
        "parent_span_id" => "c3c3c3c3c3c3c3c3"
      },
      overrides
    )
  end

  defp ok(body), do: {200, :json.encode(body) |> IO.iodata_to_binary()}
  defp claimed(leases), do: ok(%{"leases" => leases})
  defp started, do: ok(%{"expires_at" => "2026-09-01T10:00:30Z", "reclaimed" => false})
  defp acked, do: ok(%{"run_status" => "running", "next" => "claim"})

  defp registered,
    do:
      ok(%{
        "worker_id" => "w-1",
        "registered" => [%{"id" => "support", "version" => 1, "assigned" => true}],
        "lease_ttl_ms" => 3_000,
        "heartbeat_ms" => 1_000
      })

  defp conflict(reason, extra \\ %{}) do
    body =
      Map.merge(
        %{"error" => "conflict", "reason" => reason, "message" => "the daemon says #{reason}", "attempt" => 0, "run_status" => "running"},
        extra
      )

    {409, :json.encode(body) |> IO.iodata_to_binary()}
  end

  # Answers by path suffix from per-route queues, repeating the last reply, and
  # everything else from the script in order.
  defp harness(script, routes \\ [], worker_opts \\ []) do
    {:ok, plan} = Agent.start_link(fn -> %{script: script, routes: routes} end)

    daemon =
      FakeDaemon.start(fn _n, head ->
        [_method, target | _] = head |> String.split("\r\n") |> hd() |> String.split(" ")
        path = target |> String.split("?") |> hd()

        Agent.get_and_update(plan, fn plan ->
          case Enum.find(plan.routes, fn {suffix, queue} -> String.ends_with?(path, suffix) and queue != [] end) do
            {suffix, [reply | rest]} ->
              rest = if rest == [], do: [reply], else: rest
              {reply, %{plan | routes: List.keyreplace(plan.routes, suffix, 0, {suffix, rest})}}

            nil ->
              case plan.script do
                [reply | rest] -> {reply, %{plan | script: rest}}
                [] -> {{200, "{}"}, plan}
              end
          end
        end)
      end)

    {:ok, client} = Mcpd.start_link(endpoint: daemon.endpoint, project: "test", flush_interval: 3_600_000)
    {:ok, errors} = Agent.start_link(fn -> [] end)

    opts =
      Keyword.merge(
        [worker_id: "w-1", wait_ms: 0, concurrency: 3, on_error: fn m, e -> Agent.update(errors, &(&1 ++ ["#{m}: #{inspect(e)}"])) end],
        worker_opts
      )

    {:ok, worker} = StepWorker.start_link(client, opts)

    on_exit(fn ->
      Mcpd.shutdown(client)
      FakeDaemon.stop(daemon)
    end)

    %{daemon: daemon, client: client, worker: worker, errors: errors}
  end

  defp paths(h) do
    h.daemon
    |> FakeDaemon.requests()
    |> Enum.map(fn r ->
      [method, target | _] = r |> FakeDaemon.request_line() |> String.split(" ")
      method <> " " <> (target |> String.split("?") |> hd())
    end)
  end

  defp body(h, i), do: h.daemon |> FakeDaemon.requests() |> Enum.at(i) |> Map.fetch!(:body) |> :json.decode()
  defp reported(h), do: Agent.get(h.errors, & &1)

  defp claim_and_drain(h, max) do
    {:ok, n} = StepWorker.claim(h.worker, max: max)
    :ok = StepWorker.drain(h.worker, 10_000)
    n
  end

  test "one lease runs started, the body and complete" do
    h = harness([claimed([lease()]), started(), acked()])
    {:ok, seen} = Agent.start_link(fn -> [] end)

    wf =
      Workflow.new("support")
      |> Workflow.step("classify", fn ctx, input, prior ->
        Agent.update(seen, &(&1 ++ [input, prior, StepContext.state(ctx), ctx.idempotency_key, ctx.attempt, ctx.run_id, ctx.step_key]))
        StepContext.set_state(ctx, %{"seen_count" => 2, last_ticket: "T-1"})
        StepContext.set_usage(ctx, input_tokens: 10, output_tokens: 5)
        %{"kind" => "question"}
      end)

    :ok = StepWorker.register(h.worker, [wf])
    assert claim_and_drain(h, 1) == 1

    assert paths(h) == ["POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started", "POST /v1/api/steps/lease-1/complete"]
    assert body(h, 0) == %{"project" => "test", "worker_id" => "w-1", "max" => 1, "wait_ms" => 0}
    # Opaque objects reach the body spelled as the daemon sent them.
    assert Agent.get(seen, & &1) == [
             %{"ticket_id" => "T-1"},
             %{"triage" => %{"status" => "succeeded", "output" => %{"queue_name" => "billing"}}},
             %{"seen_count" => 1},
             "run-1:classify:0",
             0,
             "run-1",
             "classify"
           ]

    assert body(h, 2) == %{
             "output" => %{"kind" => "question"},
             "state" => %{"seen_count" => 2, "last_ticket" => "T-1"},
             "usage" => %{"input_tokens" => 10, "output_tokens" => 5}
           }

    assert reported(h) == []
    assert StepWorker.inflight(h.worker) == 0
  end

  test "a body that returns nil completes with no output" do
    h = harness([claimed([lease()]), started(), acked()])
    :ok = StepWorker.register(h.worker, [Workflow.new("support") |> Workflow.step("classify", fn _, _, _ -> nil end)])
    claim_and_drain(h, 1)
    assert body(h, 2) == %{}
  end

  test "a span from the body lands under the step span the daemon opened" do
    h = harness([claimed([lease()]), started(), acked(), {200, ~s({"accepted":1,"rejected":0,"duplicate":0})}])

    wf =
      Workflow.new("support")
      |> Workflow.agent("classify", fn ctx, _input, _prior ->
        StepContext.span(ctx, "model_call", [kind: :model_generation, model: "claude-sonnet-4", provider: "anthropic"], fn -> "ok" end)
      end)

    :ok = StepWorker.register(h.worker, [wf])
    claim_and_drain(h, 1)
    :ok = Mcpd.flush(h.client)

    spans = FakeDaemon.signals(h.daemon, "spans")
    assert spans != []

    for span <- spans do
      assert span["name"] == "model_call"
      assert span["trace_id"] == @trace
      assert span["parent_span_id"] == @step_span
      assert span["correlation"]["session_id"] == "s-1"
    end
  end

  test "a branch, an item and an iteration key find their handlers" do
    h =
      harness(
        [
          claimed([
            lease(%{"lease_id" => "l-a", "step_key" => "checks/fraud"}),
            lease(%{"lease_id" => "l-b", "step_key" => "notify[2]"}),
            lease(%{"lease_id" => "l-c", "step_key" => "refine[1]"})
          ])
        ],
        [{"/started", [started()]}, {"/complete", [acked()]}]
      )

    {:ok, ran} = Agent.start_link(fn -> [] end)
    record = fn name -> fn _, _, _ -> Agent.update(ran, &[name | &1]) && name end end

    wf =
      Workflow.new("support")
      |> Workflow.parallel("checks", fn b -> Workflow.step(b, "fraud", record.("fraud")) end)
      |> Workflow.foreach("notify", fn b -> Workflow.step(b, "send", record.("send")) end, over: "input.to", max_items: 3)
      |> Workflow.loop("refine", fn b -> Workflow.step(b, "rewrite", record.("rewrite")) end, max_iterations: 2)

    :ok = StepWorker.register(h.worker, [wf])
    assert claim_and_drain(h, 3) == 3
    assert Enum.sort(Agent.get(ran, & &1)) == ["fraud", "rewrite", "send"]
    assert Enum.count(paths(h), &String.ends_with?(&1, "/complete")) == 3
  end

  test "a retryable exception is reported with its class and delay" do
    h = harness([claimed([lease()]), started(), ok(%{"run_status" => "running", "next" => "idle", "attempts_left" => 2})])

    wf =
      Workflow.new("support")
      |> Workflow.step("classify", fn _, _, _ ->
        raise StepException, message: "rate limited", class: :retryable, retry_after_ms: 250, details: %{"status_code" => 429}
      end)

    :ok = StepWorker.register(h.worker, [wf])
    claim_and_drain(h, 1)
    assert Enum.at(paths(h), 2) == "POST /v1/api/steps/lease-1/fail"

    assert body(h, 2) == %{
             "error" => %{"class" => "retryable", "message" => "rate limited", "details" => %{"status_code" => 429}, "retry_after_ms" => 250}
           }
  end

  test "any other raise is fatal, and a step exception names its class" do
    h =
      harness(
        [claimed([lease(), lease(%{"lease_id" => "lease-2", "step_key" => "draft"})])],
        [{"/started", [started()]}, {"/fail", [acked()]}]
      )

    wf =
      Workflow.new("support")
      |> Workflow.step("classify", fn _, _, _ -> raise "the model returned nothing" end)
      |> Workflow.step("draft", fn _, _, _ -> raise StepException, message: "no such ticket", class: :invalid_input end)

    :ok = StepWorker.register(h.worker, [wf])
    claim_and_drain(h, 2)

    fails =
      h.daemon
      |> FakeDaemon.requests()
      |> Enum.filter(&String.ends_with?(FakeDaemon.request_line(&1), "/fail HTTP/1.1"))
      |> Enum.map(&(:json.decode(&1.body)["error"]))
      |> Enum.sort_by(& &1["class"])

    assert fails == [
             %{"class" => "fatal", "message" => "the model returned nothing"},
             %{"class" => "invalid_input", "message" => "no such ticket"}
           ]
  end

  test "a lease it cannot serve is handed back without running anything" do
    h =
      harness(
        [claimed([lease(%{"definition_id" => "billing"}), lease(%{"lease_id" => "lease-2", "definition_version" => 7})])],
        [{"/fail", [acked()]}]
      )

    {:ok, ran} = Agent.start_link(fn -> false end)
    wf = Workflow.new("support", version: 1) |> Workflow.step("classify", fn _, _, _ -> Agent.update(ran, fn _ -> true end) end)
    :ok = StepWorker.register(h.worker, [wf])
    claim_and_drain(h, 2)
    refute Agent.get(ran, & &1)

    for request <- h.daemon |> FakeDaemon.requests() |> Enum.drop(1) do
      assert String.ends_with?(FakeDaemon.request_line(request), "/fail HTTP/1.1")
      error = :json.decode(request.body)["error"]
      assert error["class"] == "unservable"
      assert error["message"] =~ "w-1 does not serve"
    end
  end

  test "already_applied is done and says nothing" do
    h = harness([claimed([lease()]), started(), conflict("already_applied", %{"attempt_status" => "succeeded"})])
    :ok = StepWorker.register(h.worker, [Workflow.new("support") |> Workflow.step("classify", fn _, _, _ -> 1 end)])
    claim_and_drain(h, 1)
    assert length(paths(h)) == 3
    assert reported(h) == []
  end

  test "a body whose started was fenced never runs" do
    h = harness([claimed([lease()]), conflict("fenced", %{"attempt_status" => "orphaned"})])
    {:ok, ran} = Agent.start_link(fn -> false end)
    :ok = StepWorker.register(h.worker, [Workflow.new("support") |> Workflow.step("classify", fn _, _, _ -> Agent.update(ran, fn _ -> true end) end)])
    claim_and_drain(h, 1)
    refute Agent.get(ran, & &1)
    assert paths(h) == ["POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started"]
    assert reported(h) == []
  end

  test "a lease whose run ended is abandoned with nothing more posted" do
    h = harness([claimed([lease()]), started(), conflict("run_terminal", %{"run_status" => "cancelled"})])
    :ok = StepWorker.register(h.worker, [Workflow.new("support") |> Workflow.step("classify", fn _, _, _ -> 1 end)])
    claim_and_drain(h, 1)
    assert length(paths(h)) == 3
    assert reported(h) == []
  end

  test "not_started is logged loudly because it is a bug in the worker" do
    h = harness([claimed([lease(%{"effect" => "at_most_once"})]), started(), conflict("not_started")])
    :ok = StepWorker.register(h.worker, [Workflow.new("support") |> Workflow.step("classify", fn _, _, _ -> 1 end)])
    claim_and_drain(h, 1)
    assert [message] = reported(h)
    assert message =~ "a bug in this worker"
  end

  test "a fenced heartbeat cancels the body and posts no complete" do
    h = harness([registered(), claimed([lease()]), started(), conflict("fenced", %{"attempt_status" => "orphaned"})])
    {:ok, reason} = Agent.start_link(fn -> nil end)

    wf =
      Workflow.new("support")
      |> Workflow.step("classify", fn ctx, _, _ ->
        true = StepContext.wait_cancelled(ctx, 5_000)
        Agent.update(reason, fn _ -> StepContext.cancel_reason(ctx) end)
        "too late"
      end)

    :ok = StepWorker.register(h.worker, [wf])
    :ok = StepWorker.register_definitions(h.worker)
    claim_and_drain(h, 1)
    # The slot is freed at the fence; the body finishes on its own after it.
    wait_until(fn -> Agent.get(reason, & &1) != nil end)
    assert Agent.get(reason, & &1) =~ "fenced"

    assert paths(h) == [
             "POST /v1/api/workers/register",
             "POST /v1/api/steps/claim",
             "POST /v1/api/steps/lease-1/started",
             "POST /v1/api/steps/lease-1/heartbeat"
           ]

    assert reported(h) == []
  end

  test "a heartbeat that fails for any other reason is reported and the body goes on" do
    h = harness([registered(), claimed([lease()]), started(), {400, ~s({"error":"bad_request","message":"no"})}, acked()])

    wf =
      Workflow.new("support")
      |> Workflow.step("classify", fn _, _, _ ->
        Process.sleep(1_150)
        "done"
      end)

    :ok = StepWorker.register(h.worker, [wf])
    :ok = StepWorker.register_definitions(h.worker)
    claim_and_drain(h, 1)
    assert List.last(paths(h)) == "POST /v1/api/steps/lease-1/complete"
    assert Enum.any?(reported(h), &(&1 =~ "heartbeat failed"))
  end

  test "serve registers, claims until stopped, and returns once stopped" do
    h = harness([registered()], [{"/steps/claim", [claimed([])]}], concurrency: 2)
    :ok = StepWorker.register(h.worker, [Workflow.new("support") |> Workflow.step("classify", fn _, _, _ -> 1 end)])
    :ok = StepWorker.serve(h.worker)
    wait_until(fn -> length(FakeDaemon.requests(h.daemon)) >= 3 end)
    :ok = StepWorker.stop(h.worker, 5_000)

    assert hd(paths(h)) == "POST /v1/api/workers/register"

    assert body(h, 0) == %{
             "project" => "test",
             "worker_id" => "w-1",
             "definitions" => [%{"id" => "support", "kind" => "workflow", "body" => %{"steps" => [%{"kind" => "task", "id" => "classify"}]}}]
           }

    assert Enum.all?(tl(paths(h)), &(&1 == "POST /v1/api/steps/claim"))
    assert body(h, 1) == %{"project" => "test", "worker_id" => "w-1", "max" => 2, "wait_ms" => 0}
    assert reported(h) == []
  end

  test "a claim that failed is reported and tried again" do
    h = harness([registered(), {400, ~s({"error":"bad_request","message":"no"})}], [], [])
    :ok = StepWorker.register(h.worker, [Workflow.new("support") |> Workflow.step("classify", fn _, _, _ -> 1 end)])
    :ok = StepWorker.serve(h.worker)
    wait_until(fn -> reported(h) != [] end)
    :ok = StepWorker.stop(h.worker, 5_000)
    assert hd(reported(h)) =~ "claim failed"
  end

  test "a run starts with the fields the daemon expects" do
    h =
      harness([
        ok(%{
          "run_id" => "run-1",
          "project" => "test",
          "definition_id" => "support",
          "definition_version" => 2,
          "status" => "running",
          "cursor" => %{"index" => 0, "construct" => "task"},
          "correlation" => %{"project" => "test"},
          "trace_id" => @trace,
          "steps_used" => 0,
          "cost_usd" => 0,
          "root_run_id" => "run-1",
          "depth" => 0,
          "created_at" => "2026-09-01T10:00:00Z",
          "updated_at" => "2026-09-01T10:00:00Z"
        })
      ])

    {:ok, run} =
      Mcpd.start_run(h.client, "support",
        version: 2,
        input: %{"ticket_id" => "T-1"},
        idempotency_key: "ticket:T-1",
        correlation: %{session_id: "s-1"}
      )

    assert run.run_id == "run-1"
    assert run.status == :running
    assert hd(paths(h)) == "POST /v1/api/runs"

    assert body(h, 0) == %{
             "project" => "test",
             "definition_id" => "support",
             "version" => 2,
             "input" => %{"ticket_id" => "T-1"},
             "idempotency_key" => "ticket:T-1",
             "correlation" => %{"project" => "test", "session_id" => "s-1"}
           }
  end

  test "a definition is stored with its body untouched" do
    h = harness([ok(%{"id" => "support", "version" => 1, "assigned" => true})])
    wf = Workflow.new("support") |> Workflow.step("classify", fn _, _, _ -> 1 end, step_timeout_ms: 100)
    {:ok, stored} = Mcpd.put_definition(h.client, wf)
    assert {stored.id, stored.version, stored.assigned} == {"support", 1, true}
    assert FakeDaemon.request_line(hd(FakeDaemon.requests(h.daemon))) == "POST /v1/api/definitions?project=test HTTP/1.1"
    assert body(h, 0) == %{"id" => "support", "kind" => "workflow", "body" => %{"steps" => [%{"kind" => "task", "id" => "classify", "step_timeout_ms" => 100}]}}
  end

  test "watch_run walks the pages until the run ends, passing the last seq it saw" do
    h =
      harness([
        ok(%{
          "events" => [
            %{"run_id" => "run-1", "seq" => 0, "kind" => "run_started", "payload" => %{"definition_id" => "support"}, "at" => "2026-09-01T10:00:00Z"},
            %{"run_id" => "run-1", "seq" => 1, "kind" => "step_leased", "payload" => %{"step_key" => "classify"}, "at" => "2026-09-01T10:00:01Z"}
          ],
          "has_more" => true
        }),
        ok(%{"events" => [], "has_more" => false}),
        ok(%{
          "events" => [%{"run_id" => "run-1", "seq" => 2, "kind" => "run_ended", "payload" => %{"status" => "succeeded"}, "at" => "2026-09-01T10:00:02Z"}],
          "has_more" => false
        })
      ])

    kinds = h.client |> Mcpd.watch_run("run-1", poll_ms: 1) |> Enum.map(& &1.kind)
    assert kinds == [:run_started, :step_leased, :run_ended]

    assert h.daemon |> FakeDaemon.requests() |> Enum.map(&(FakeDaemon.request_line(&1) |> String.split("?") |> Enum.at(1))) == [
             "after=-1&limit=500 HTTP/1.1",
             "after=1&limit=500 HTTP/1.1",
             "after=1&limit=500 HTTP/1.1"
           ]
  end

  test "publish and signal carry the ids the caller supplied" do
    receipt = ok(%{"outcome" => "delivered", "event_id" => "evt-1"})
    h = harness([receipt, receipt])
    {:ok, _} = Mcpd.publish_event(h.client, "approved", "T-1", payload: %{"by" => "ops"}, event_id: "evt-1")
    {:ok, _} = Mcpd.signal_run(h.client, "run-1", "approved", payload: %{"by" => "ops"}, event_id: "evt-2")
    assert paths(h) == ["POST /v1/api/events", "POST /v1/api/runs/run-1/signal"]
    assert body(h, 0) == %{"project" => "test", "event" => "approved", "key" => "T-1", "payload" => %{"by" => "ops"}, "event_id" => "evt-1"}
    assert body(h, 1) == %{"event" => "approved", "payload" => %{"by" => "ops"}, "event_id" => "evt-2"}
  end

  test "a 409 from a worker call is a typed conflict, and any other 409 is itself" do
    h =
      harness([
        conflict("fenced", %{"attempt_status" => "orphaned"}),
        {409, ~s({"error":"version_taken","message":"support@1 exists with a different body"})}
      ])

    assert {:error, {:conflict, conflict}} = Mcpd.complete_step(h.client, "lease-1", output: 1)
    assert conflict.reason == :fenced
    assert conflict.attempt_status == :orphaned

    assert {:error, {:http, 409, text, _}} = Mcpd.put_definition(h.client, Workflow.new("support", version: 1))
    assert text =~ "version_taken"
  end

  defp wait_until(fun, deadline \\ 5_000) do
    cond do
      fun.() -> :ok
      deadline <= 0 -> flunk("the condition never held")
      true ->
        Process.sleep(10)
        wait_until(fun, deadline - 10)
    end
  end

  # ------------------------------------------------- 7.3 usage accumulation

  @ingested {200, ~s({"accepted":1,"rejected":0,"duplicate":0})}

  test "usage on the body's spans is summed onto the step, and the last model seen wins" do
    h = harness([claimed([lease()]), started(), acked(), @ingested])
    client = h.client

    wf =
      Workflow.new("support")
      |> Workflow.agent("classify", fn ctx, _input, _prior ->
        first = [kind: :model_generation, model: "m1", provider: "p1", usage: %{"input_tokens" => 10, "output_tokens" => 5}]

        StepContext.span(ctx, "first", first, fn ->
          # Nested under the context's span, through the plain client.
          nested = [model: "m2", usage: %{input_tokens: 20, output_tokens: 7, total_tokens: 27}]
          Mcpd.with_span(client, "nested", nested, fn -> :ok end)
        end)

        # A plain span in the body's own process counts too: it nests under
        # the step. It closes last, so its model is the one the step reports.
        Mcpd.with_span(client, "plain", [usage: %{"input_tokens" => 1}, model: "m3"], fn -> :ok end)
        # A span with no usage adds nothing and changes no model.
        StepContext.span(ctx, "quiet", [], fn -> :ok end)
        "ok"
      end)

    :ok = StepWorker.register(h.worker, [wf])
    claim_and_drain(h, 1)

    assert body(h, 2)["usage"] == %{
             "input_tokens" => 31,
             "output_tokens" => 12,
             "total_tokens" => 27,
             "model" => "m3",
             "provider" => "p1"
           }
  end

  test "set_usage replaces the accumulated fields it names and keeps the rest" do
    h = harness([claimed([lease()]), started(), acked(), @ingested])

    wf =
      Workflow.new("support")
      |> Workflow.agent("classify", fn ctx, _input, _prior ->
        opts = [model: "m1", provider: "p1", usage: %{"input_tokens" => 10, "output_tokens" => 5}]
        StepContext.span(ctx, "call", opts, fn -> :ok end)
        StepContext.set_usage(ctx, input_tokens: 999, model: "m9")
        "ok"
      end)

    :ok = StepWorker.register(h.worker, [wf])
    claim_and_drain(h, 1)
    assert body(h, 2)["usage"] == %{"input_tokens" => 999, "output_tokens" => 5, "model" => "m9", "provider" => "p1"}
  end

  test "a body that records no spans reports what it set, and nothing when it set nothing" do
    h = harness([claimed([lease()]), started(), acked(), claimed([lease(%{"lease_id" => "lease-2"})]), started(), acked()])
    {:ok, turn} = Agent.start_link(fn -> 0 end)

    wf =
      Workflow.new("support")
      |> Workflow.step("classify", fn ctx, _input, _prior ->
        if Agent.get_and_update(turn, &{&1, &1 + 1}) == 0, do: StepContext.set_usage(ctx, input_tokens: 4)
        "ok"
      end)

    :ok = StepWorker.register(h.worker, [wf])
    claim_and_drain(h, 1)
    claim_and_drain(h, 1)
    assert body(h, 2)["usage"] == %{"input_tokens" => 4}
    refute Map.has_key?(body(h, 5), "usage")
  end

  # ----------------------------------------------------- 7.2 hooks and stats

  defp hooks(events) do
    [
      on_lease: fn lease -> Agent.update(events, &(&1 ++ [{:lease, lease.lease_id}])) end,
      on_step_start: fn ctx -> Agent.update(events, &(&1 ++ [{:start, ctx.step_key, ctx.attempt}])) end,
      on_step_end: fn ctx, outcome -> Agent.update(events, &(&1 ++ [{:end, ctx.step_key, outcome}])) end,
      on_conflict: fn lease, reason -> Agent.update(events, &(&1 ++ [{:conflict, lease.lease_id, reason}])) end
    ]
  end

  test "the four hooks fire once each with what the contract says" do
    {:ok, events} = Agent.start_link(fn -> [] end)
    h = harness([claimed([lease()]), started(), acked()], [], hooks(events))
    :ok = StepWorker.register(h.worker, [Workflow.new("support") |> Workflow.step("classify", fn _, _, _ -> 1 end)])
    claim_and_drain(h, 1)
    assert Agent.get(events, & &1) == [{:lease, "lease-1"}, {:start, "classify", 0}, {:end, "classify", :completed}]

    {:ok, events} = Agent.start_link(fn -> [] end)
    h = harness([claimed([lease()]), started(), acked()], [], hooks(events))

    :ok =
      StepWorker.register(h.worker, [
        Workflow.new("support")
        |> Workflow.step("classify", fn _, _, _ -> raise Mcpd.StepException, message: "no relay", class: :retryable end)
      ])

    claim_and_drain(h, 1)
    assert [{:lease, "lease-1"}, {:start, "classify", 0}, {:end, "classify", {:failed, :retryable, "no relay"}}] = Agent.get(events, & &1)

    {:ok, events} = Agent.start_link(fn -> [] end)
    h = harness([claimed([lease()]), conflict("fenced", %{"attempt_status" => "orphaned"})], [], hooks(events))
    :ok = StepWorker.register(h.worker, [Workflow.new("support") |> Workflow.step("classify", fn _, _, _ -> 1 end)])
    claim_and_drain(h, 1)
    assert Agent.get(events, & &1) == [{:lease, "lease-1"}, {:conflict, "lease-1", "fenced"}]
    assert reported(h) == []
  end

  test "without on_error, one line per kind of failure per interval, whatever the step" do
    # Three leases this worker cannot serve, each with its own step key, and a
    # daemon that refuses every `fail`. Every report used to be written, so a
    # failing run logged one line per step.
    leases = for n <- 1..3, do: lease(%{"lease_id" => "lease-#{n}", "step_key" => "unknown_#{n}"})
    h = harness([claimed(leases)], [{"/fail", [{400, ~s({"error":"bad"})}]}], on_error: nil)

    log =
      ExUnit.CaptureLog.capture_log(fn ->
        claim_and_drain(h, 3)
        # The report is a cast to the worker; let it be read.
        _ = StepWorker.stats(h.worker)
      end)

    assert length(Regex.scan(~r/call failed for/, log)) == 1, log
    assert Enum.count(paths(h), &String.ends_with?(&1, "/fail")) == 3
  end

  test "a hook that raises is reported through on_error and the lease still completes" do
    h = harness([claimed([lease()]), started(), acked()], [], on_step_start: fn _ctx -> raise "the hook is broken" end)
    :ok = StepWorker.register(h.worker, [Workflow.new("support") |> Workflow.step("classify", fn _, _, _ -> "done" end)])
    claim_and_drain(h, 1)
    assert paths(h) == ["POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started", "POST /v1/api/steps/lease-1/complete"]
    assert body(h, 2)["output"] == "done"
    assert [message] = reported(h)
    assert message =~ "the on_step_start hook raised"
    assert message =~ "the hook is broken"
  end

  test "stats count a completed step, a fatal failure, a fenced conflict and an unservable lease" do
    h =
      harness([
        claimed([lease()]),
        started(),
        acked(),
        claimed([lease(%{"lease_id" => "lease-2"})]),
        started(),
        acked(),
        claimed([lease(%{"lease_id" => "lease-3"})]),
        conflict("fenced", %{"attempt_status" => "orphaned"}),
        claimed([lease(%{"lease_id" => "lease-4", "definition_id" => "billing"})]),
        acked()
      ])

    wf =
      Workflow.new("support")
      |> Workflow.step("classify", fn ctx, _, _ ->
        if ctx.lease.lease_id == "lease-2", do: raise("boom"), else: "ok"
      end)

    :ok = StepWorker.register(h.worker, [wf])
    assert StepWorker.stats(h.worker) == %{claimed: 0, inflight: 0, completed: 0, failed: %{}, conflicts: %{}, unservable: 0}
    for _ <- 1..4, do: claim_and_drain(h, 1)

    assert StepWorker.stats(h.worker) == %{
             claimed: 4,
             inflight: 0,
             completed: 1,
             failed: %{"fatal" => 1},
             conflicts: %{"fenced" => 1},
             unservable: 1
           }
  end

  describe "the claim long poll" do
    test "an idle claim answered after the client timeout but within the wait is a response" do
      # The daemon holds an idle claim open for the whole of `wait_ms`, so the
      # answer arrives after the client's own timeout and within the wait. A
      # claim cut off by that timeout is what made a healthy idle worker
      # report a failure every ten seconds.
      daemon =
        FakeDaemon.start(fn _n ->
          Process.sleep(300)
          {200, ~s({"leases": []})}
        end)

      {:ok, client} = Mcpd.start_link(endpoint: daemon.endpoint, project: "test", flush_interval: 3_600_000, timeout: 100)

      assert {:ok, %Mcpd.ClaimResponse{leases: []}} = Mcpd.Runs.claim_steps(client, "w-1", max: 1, wait_ms: 2_000)

      Mcpd.shutdown(client)
      FakeDaemon.stop(daemon)
    end
  end
end

defmodule Mcpd.StepWorkerDeadlineTest do
  @moduledoc "The lease deadline, typed decoding, and waiting for a run (contract sections 7.2 to 7.4)."
  use ExUnit.Case, async: false

  alias Mcpd.{FakeDaemon, StepContext, StepWorker, Workflow}

  @trace "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"

  defp lease(overrides \\ %{}) do
    Map.merge(
      %{
        "lease_id" => "lease-1", "run_id" => "run-1", "project" => "test", "definition_id" => "support",
        "definition_version" => 1, "step_key" => "classify", "kind" => "task", "attempt" => 0,
        "effect" => "at_least_once", "expires_at" => "2026-09-01T10:00:00Z", "idempotency_key" => "run-1:classify:0",
        "input" => %{"ticket_id" => "T-1"}, "prior" => %{"triage" => %{"status" => "succeeded"}}, "state" => %{"seen_count" => 1},
        "correlation" => %{"project" => "test"}, "trace_id" => @trace, "span_id" => "b2b2b2b2b2b2b2b2",
        "parent_span_id" => "c3c3c3c3c3c3c3c3"
      },
      overrides
    )
  end

  defp ok(body), do: {200, :json.encode(body) |> IO.iodata_to_binary()}
  defp claimed(leases), do: ok(%{"leases" => leases})
  defp started, do: ok(%{"expires_at" => "2026-09-01T10:00:30Z", "reclaimed" => false})
  defp acked, do: ok(%{"run_status" => "running", "next" => "claim"})

  defp harness(script, routes \\ []) do
    {:ok, plan} = Agent.start_link(fn -> %{script: script, routes: routes} end)

    daemon =
      FakeDaemon.start(fn _n, head ->
        [_method, target | _] = head |> String.split("\r\n") |> hd() |> String.split(" ")
        path = target |> String.split("?") |> hd()

        Agent.get_and_update(plan, fn plan ->
          case Enum.find(plan.routes, fn {suffix, _} -> String.ends_with?(path, suffix) end) do
            {_suffix, reply} ->
              {reply, plan}

            nil ->
              case plan.script do
                [reply | rest] -> {reply, %{plan | script: rest}}
                [] -> {{200, "{}"}, plan}
              end
          end
        end)
      end)

    {:ok, client} = Mcpd.start_link(endpoint: daemon.endpoint, project: "test", flush_interval: 3_600_000)
    {:ok, errors} = Agent.start_link(fn -> [] end)

    {:ok, worker} =
      StepWorker.start_link(client,
        worker_id: "w-1",
        wait_ms: 0,
        on_error: fn m, e -> Agent.update(errors, &(&1 ++ ["#{m}: #{inspect(e)}"])) end
      )

    on_exit(fn ->
      Mcpd.shutdown(client)
      FakeDaemon.stop(daemon)
    end)

    %{daemon: daemon, client: client, worker: worker, errors: errors}
  end

  defp paths(h) do
    h.daemon
    |> FakeDaemon.requests()
    |> Enum.map(fn r ->
      [method, target | _] = r |> FakeDaemon.request_line() |> String.split(" ")
      method <> " " <> (target |> String.split("?") |> hd())
    end)
  end

  defp body(h, i), do: h.daemon |> FakeDaemon.requests() |> Enum.at(i) |> Map.fetch!(:body) |> :json.decode()

  defp claim_and_drain(h) do
    {:ok, _} = StepWorker.claim(h.worker, max: 1)
    :ok = StepWorker.drain(h.worker, 10_000)
  end

  defp wait_for(fun, deadline \\ 5_000) do
    cond do
      fun.() ->
        true

      deadline <= 0 ->
        flunk("the condition never held")

      true ->
        Process.sleep(10)
        wait_for(fun, deadline - 10)
    end
  end

  test "a body that runs past the lease deadline is cancelled and failed retryable" do
    h = harness([claimed([lease(%{"deadline_ms" => 150})]), started(), acked()])
    {:ok, reason} = Agent.start_link(fn -> nil end)

    wf =
      Workflow.new("support")
      |> Workflow.step("classify", fn ctx, _, _ ->
        true = StepContext.wait_cancelled(ctx, 5_000)
        Agent.update(reason, fn _ -> StepContext.cancel_reason(ctx) end)
        "too late"
      end)

    :ok = StepWorker.register(h.worker, [wf])
    claim_and_drain(h)
    # The slot is freed at the deadline; the body finishes on its own after it.
    wait_for(fn -> Agent.get(reason, & &1) != nil end)
    assert Agent.get(reason, & &1) == "step deadline of 150ms passed"

    assert paths(h) == [
             "POST /v1/api/steps/claim",
             "POST /v1/api/steps/lease-1/started",
             "POST /v1/api/steps/lease-1/fail"
           ]

    assert body(h, 2) == %{"error" => %{"class" => "retryable", "message" => "step deadline of 150ms passed"}}
    assert Agent.get(h.errors, & &1) == []
  end

  test "the slot is freed at the deadline even when the body ignores cancelled" do
    h = harness([claimed([lease(%{"deadline_ms" => 100})]), started(), acked()])
    # A body that never looks at `cancelled?/1`.
    wf = Workflow.new("support") |> Workflow.step("classify", fn _, _, _ -> Process.sleep(3_000) end)
    :ok = StepWorker.register(h.worker, [wf])

    began = System.monotonic_time(:millisecond)
    claim_and_drain(h)
    took = System.monotonic_time(:millisecond) - began
    assert took < 1_000, "the slot was held for #{took}ms, past the deadline"
    assert StepWorker.stats(h.worker).inflight == 0
    assert List.last(paths(h)) == "POST /v1/api/steps/lease-1/fail"
    assert %{failed: %{"retryable" => 1}} = StepWorker.stats(h.worker)
  end

  test "a body that finishes in time is left alone" do
    h = harness([claimed([lease(%{"deadline_ms" => 5_000})]), started(), acked()])
    :ok = StepWorker.register(h.worker, [Workflow.new("support") |> Workflow.step("classify", fn _, _, _ -> "quick" end)])
    claim_and_drain(h)
    assert List.last(paths(h)) == "POST /v1/api/steps/lease-1/complete"
  end

  test "a decoder that rejects the input fails the step invalid_input" do
    h = harness([claimed([lease()]), started(), acked()])

    wf =
      Workflow.new("support")
      |> Workflow.step("classify", fn ctx, _input, _prior ->
        %{"triage" => _} = StepContext.prior(ctx, & &1)
        %{"seen_count" => 1} = StepContext.state_as(ctx, & &1)

        StepContext.input(ctx, fn value ->
          if is_integer(value["ticket_id"]), do: value, else: raise(ArgumentError, "ticket_id must be a number")
        end)
      end)

    :ok = StepWorker.register(h.worker, [wf])
    claim_and_drain(h)
    assert Enum.at(paths(h), 2) == "POST /v1/api/steps/lease-1/fail"

    assert body(h, 2) == %{
             "error" => %{"class" => "invalid_input", "message" => "the step's input could not be read: ticket_id must be a number"}
           }
  end

  defp run_body(status, extra \\ %{}) do
    run =
      Map.merge(
        %{
          "run_id" => "run-1", "project" => "test", "definition_id" => "support", "definition_version" => 1,
          "status" => status, "cursor" => %{"index" => 2, "construct" => "end"}, "correlation" => %{"project" => "test"},
          "trace_id" => @trace, "steps_used" => 2, "cost_usd" => 0.25, "root_run_id" => "run-1", "depth" => 0,
          "created_at" => "2026-09-01T10:00:00Z", "updated_at" => "2026-09-01T10:00:05Z"
        },
        extra
      )

    %{"run" => run, "attempts" => [], "timers" => [], "waits" => []}
  end

  defp ended, do: ok(%{"events" => [%{"run_id" => "run-1", "seq" => 0, "kind" => "run_ended", "at" => "2026-09-01T10:00:05Z"}], "has_more" => false})

  test "wait_run returns the output once the run ends" do
    h = harness([ended(), ok(run_body("succeeded", %{"output" => %{"answer" => 42}}))])
    assert {:ok, result} = Mcpd.wait_run(h.client, "run-1", poll_ms: 1)
    assert result.status == :succeeded
    assert result.output == %{"answer" => 42}
    assert result.cost_usd == 0.25
    assert result.trace_id == @trace
  end

  test "wait_run answers a typed failure carrying the run when it did not succeed" do
    h = harness([ended(), ok(run_body("failed", %{"error" => "send: relay down"}))])
    assert {:error, {:run_failed, run}} = Mcpd.wait_run(h.client, "run-1", poll_ms: 1)
    assert run.status == :failed
    assert run.error == "send: relay down"
  end

  test "wait_run gives up after the timeout while the run is still going" do
    h = harness([], [{"/events", ok(%{"events" => [], "has_more" => false})}])
    assert {:error, :timeout} = Mcpd.wait_run(h.client, "run-1", poll_ms: 5, timeout_ms: 40)
  end

  test "start_run_and_wait does both" do
    h = harness([ok(run_body("running")["run"]), ended(), ok(run_body("succeeded", %{"output" => "done"}))])
    assert {:ok, result} = Mcpd.start_run_and_wait(h.client, "support", input: %{"a" => 1}, poll_ms: 1)
    assert result.output == "done"
    assert body(h, 0) == %{"project" => "test", "definition_id" => "support", "input" => %{"a" => 1}}
  end
end
