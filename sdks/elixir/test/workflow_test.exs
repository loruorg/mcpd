defmodule Mcpd.WorkflowTest do
  @moduledoc "The workflow builder: the document it produces and the handlers it keeps (contract section 7.1)."
  use ExUnit.Case, async: true

  alias Mcpd.Workflow
  import Mcpd.Workflow, only: [where: 3, exists: 1, all_of: 1, any_of: 1, negate: 1]

  defp body(_ctx, _input, _prior), do: "done"

  test "the document spells every construct the way the daemon reads it" do
    wf =
      Workflow.new("support",
        version: 2,
        input_schema: %{"type" => "object", "required" => ["ticket"]},
        limits: [max_steps: 50, run_timeout_ms: 60_000, max_cost_usd: 2]
      )
      |> Workflow.step("classify", &body/3,
        input: %{"ticket" => "${input.ticket}"},
        effect: :at_most_once,
        step_timeout_ms: 5_000,
        retry: [max_attempts: 3, backoff: :exponential, initial_ms: 100, max_ms: 1_000]
      )
      |> Workflow.agent("draft", &body/3, when: where("steps.classify.output.kind", :eq, "question"), on_error: :continue)
      |> Workflow.sleep("cool", 10)
      |> Workflow.sleep_until("bill", "${input.billing_date}")
      |> Workflow.wait_for("approval",
        event: "approved",
        key: "${input.ticket}",
        timeout_ms: 1_000,
        on_timeout: :continue,
        accept_parked: true
      )
      |> Workflow.parallel("checks", fn b -> b |> Workflow.step("fraud", &body/3) |> Workflow.agent("policy", &body/3) end)
      |> Workflow.foreach(
        "notify",
        fn b -> Workflow.step(b, "send", &body/3, input: %{"to" => "${item}", "position" => "${index}"}) end,
        over: "input.recipients",
        max_items: 10,
        concurrency: 2
      )
      |> Workflow.loop("refine", fn b -> Workflow.step(b, "rewrite", &body/3) end,
        max_iterations: 3,
        until: where("steps.rewrite.output.done", "eq", true)
      )
      |> Workflow.subworkflow("escalate", "escalation",
        version: 1,
        input: %{"ticket" => "${input.ticket}"},
        when: negate(exists("steps.checks.fraud.output.clean"))
      )
      |> Workflow.output(%{"answer" => "${steps.draft.output}"})

    doc = Workflow.document(wf)
    assert doc.id == "support"
    assert doc.version == 2
    assert doc.kind == :workflow

    assert doc.body == %{
             "input_schema" => %{"type" => "object", "required" => ["ticket"]},
             "limits" => %{"max_steps" => 50, "run_timeout_ms" => 60_000, "max_cost_usd" => 2},
             "output" => %{"answer" => "${steps.draft.output}"},
             "steps" => [
               %{
                 "kind" => "task",
                 "id" => "classify",
                 "input" => %{"ticket" => "${input.ticket}"},
                 "effect" => "at_most_once",
                 "step_timeout_ms" => 5_000,
                 "retry" => %{"max_attempts" => 3, "backoff" => "exponential", "initial_ms" => 100, "max_ms" => 1_000}
               },
               %{
                 "kind" => "agent",
                 "id" => "draft",
                 "when" => %{"path" => "steps.classify.output.kind", "op" => "eq", "value" => "question"},
                 "on_error" => "continue"
               },
               %{"kind" => "sleep", "id" => "cool", "ms" => 10},
               %{"kind" => "sleep_until", "id" => "bill", "at" => "${input.billing_date}"},
               %{
                 "kind" => "wait_event",
                 "id" => "approval",
                 "event" => "approved",
                 "key" => "${input.ticket}",
                 "timeout_ms" => 1_000,
                 "accept_parked" => true,
                 "on_timeout" => "continue"
               },
               %{
                 "kind" => "parallel",
                 "id" => "checks",
                 "branches" => [%{"kind" => "task", "id" => "fraud"}, %{"kind" => "agent", "id" => "policy"}]
               },
               %{
                 "kind" => "foreach",
                 "id" => "notify",
                 "over" => "input.recipients",
                 "body" => %{"kind" => "task", "id" => "send", "input" => %{"to" => "${item}", "position" => "${index}"}},
                 "max_items" => 10,
                 "concurrency" => 2
               },
               %{
                 "kind" => "loop",
                 "id" => "refine",
                 "body" => %{"kind" => "task", "id" => "rewrite"},
                 "max_iterations" => 3,
                 "until" => %{"path" => "steps.rewrite.output.done", "op" => "eq", "value" => true}
               },
               %{
                 "kind" => "subworkflow",
                 "id" => "escalate",
                 "definition" => "escalation",
                 "version" => 1,
                 "input" => %{"ticket" => "${input.ticket}"},
                 "when" => %{"not" => %{"path" => "steps.checks.fraud.output.clean", "op" => "exists"}}
               }
             ]
           }
  end

  test "what was not said is left out so the daemon applies its defaults" do
    doc = Workflow.new("plain") |> Workflow.step("one", &body/3) |> Workflow.document()
    assert doc.version == nil
    assert doc.body == %{"steps" => [%{"kind" => "task", "id" => "one"}]}
    assert Mcpd.Runs.wire(doc) == %{"id" => "plain", "kind" => "workflow", "body" => %{"steps" => [%{"kind" => "task", "id" => "one"}]}}
  end

  test "predicates combine the way the contract spells them" do
    assert all_of([exists("a"), any_of([where("b", :gt, 1), negate(where("c", :in, [1, 2]))])]) == %{
             "all" => [
               %{"path" => "a", "op" => "exists"},
               %{
                 "any" => [
                   %{"path" => "b", "op" => "gt", "value" => 1},
                   %{"not" => %{"path" => "c", "op" => "in", "value" => [1, 2]}}
                 ]
               }
             ]
           }
  end

  test "an id the daemon would refuse is refused here" do
    assert_raise ArgumentError, ~r/not a step id/, fn -> Workflow.new("Support") end
    assert_raise ArgumentError, ~r/not a step id/, fn -> Workflow.new("ok") |> Workflow.step("9lives", &body/3) end
    assert_raise ArgumentError, ~r/not a step id/, fn -> Workflow.new("ok") |> Workflow.sleep("has-dash", 1) end
  end

  test "a foreach or loop body is one claimable step" do
    assert_raise ArgumentError, ~r/exactly one body step/, fn ->
      Workflow.new("ok")
      |> Workflow.foreach("each", fn b -> b |> Workflow.step("a", &body/3) |> Workflow.step("b", &body/3) end,
        over: "input.items",
        max_items: 1
      )
    end

    assert_raise ArgumentError, ~r/exactly one body step/, fn ->
      Workflow.new("ok") |> Workflow.loop("again", fn b -> b end, max_iterations: 1)
    end
  end

  test "handler_for resolves a plain step, a branch, an item and an iteration" do
    classify = fn _, _, _ -> "classify" end
    fraud = fn _, _, _ -> "fraud" end
    send_it = fn _, _, _ -> "send" end
    rewrite = fn _, _, _ -> "rewrite" end

    wf =
      Workflow.new("support")
      |> Workflow.step("classify", classify)
      |> Workflow.parallel("checks", fn b -> Workflow.step(b, "fraud", fraud) end)
      |> Workflow.foreach("notify", fn b -> Workflow.step(b, "send", send_it) end, over: "input.to", max_items: 5)
      |> Workflow.loop("refine", fn b -> Workflow.step(b, "rewrite", rewrite) end, max_iterations: 2)

    assert Workflow.handler_for(wf, "classify") == classify
    assert Workflow.handler_for(wf, "checks/fraud") == fraud
    assert Workflow.handler_for(wf, "notify[3]") == send_it
    assert Workflow.handler_for(wf, "notify[3]/send") == send_it
    assert Workflow.handler_for(wf, "refine[1]") == rewrite
    assert Workflow.handler_for(wf, "refine[1]/rewrite") == rewrite
    assert Workflow.handler_for(wf, "checks") == nil
    assert Workflow.handler_for(wf, "unknown") == nil
    assert Workflow.handler_for(wf, "checks/policy") == nil
  end
end
