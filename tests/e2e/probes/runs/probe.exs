# The canonical run, served in Elixir. Prints one JSON line at the end.
alias Mcpd.{StepContext, StepWorker, Workflow}

canonical = File.read!(Path.join(__DIR__, "canonical.json")) |> :json.decode()
definition_id = canonical["definition_id"]
{:ok, mcpd} = Mcpd.start_link(project: System.get_env("MCPD_PROJECT"))

wf =
  Workflow.new(definition_id)
  |> Workflow.step("classify", fn _ctx, input, _prior ->
    %{"kind" => if(String.starts_with?(input["ticket"], "Q"), do: "question", else: "bug")}
  end)
  |> Workflow.agent(
    "draft",
    fn ctx, input, _prior ->
      # No set_usage: the span below carries the tokens, and the step reports their sum.
      StepContext.set_state(ctx, %{"drafted" => true})

      StepContext.span(ctx, "model_call", [kind: :model_generation, model: "claude-sonnet-4", provider: "anthropic",
        usage: %{"input_tokens" => 120, "output_tokens" => 40}], fn -> :ok end)

      "an answer to a #{input["kind"]}"
    end,
    input: %{"kind" => "${steps.classify.output.kind}"}
  )
  |> Workflow.wait_for("approval", event: "approved", key: "${input.ticket}", timeout_ms: 60_000, accept_parked: true)
  |> Workflow.step(
    "send",
    fn ctx, input, _prior ->
      if ctx.attempt == 0, do: raise(Mcpd.StepException, message: "relay busy", class: :retryable, retry_after_ms: 100)
      %{"sent" => true, "by" => input["by"]}
    end,
    input: %{"text" => "${steps.draft.output}", "by" => "${steps.approval.output.by}"},
    effect: :at_most_once,
    retry: [max_attempts: 2, backoff: :fixed, initial_ms: 100]
  )
  |> Workflow.output(%{"answer" => "${steps.draft.output}", "sent" => "${steps.send.output.sent}", "by" => "${steps.send.output.by}"})

{:ok, worker} =
  StepWorker.start_link(mcpd,
    workflows: [wf],
    worker_id: "probe-elixir",
    wait_ms: 1_000,
    on_error: fn message, reason -> raise "#{message}: #{inspect(reason)}" end
  )

:ok = StepWorker.register_definitions(worker)

{:ok, run} = Mcpd.start_run(mcpd, definition_id, input: canonical["input"], correlation: canonical["correlation"])
event = canonical["event"]
{:ok, first} = Mcpd.publish_event(mcpd, event["name"], event["key"], payload: event["payload"], event_id: event["event_id"])
{:ok, second} = Mcpd.publish_event(mcpd, event["name"], event["key"], payload: event["payload"], event_id: event["event_id"])

{:ok, detail} = Mcpd.get_run(mcpd, run.run_id)

detail =
  Enum.reduce_while(1..60, detail, fn _, detail ->
    if detail.run.status == :running do
      {:ok, _} = StepWorker.claim(worker, max: 4, run_id: run.run_id)
      :ok = StepWorker.drain(worker, 30_000)
      {:ok, detail} = Mcpd.get_run(mcpd, run.run_id)
      if detail.run.status == :running, do: Process.sleep(100)
      {:cont, detail}
    else
      {:halt, detail}
    end
  end)

:ok = Mcpd.shutdown(mcpd)

IO.puts(:json.encode(%{"run_id" => run.run_id, "trace_id" => run.trace_id, "status" => to_string(detail.run.status),
  "publish" => [to_string(first.outcome), to_string(second.outcome)]}))
