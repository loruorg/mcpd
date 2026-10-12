# A durable workflow in Elixir: the graph goes to the daemon as a definition,
# the step bodies stay in this node for a StepWorker to serve.
#
# The regions are read into the documentation at build time.
alias Mcpd.{StepContext, StepWorker, Workflow}
import Mcpd.Workflow, only: [where: 3]

{:ok, mcpd} = Mcpd.start_link(project: "support")

deliver = fn text, idempotency_key -> text != "" and idempotency_key != "" end

# #region workflow
support =
  Workflow.new("support_ticket")
  |> Workflow.step("classify", fn _ctx, input, _prior ->
    kind = if String.starts_with?(input["ticket"], "Q"), do: "question", else: "bug"
    %{"kind" => kind}
  end)
  |> Workflow.agent(
    "draft",
    fn ctx, input, _prior ->
      # Your own model call, recorded under this step in the run's trace. Its
      # usage is the step's: the daemon prices what the trace shows.
      opts = [kind: :model_generation, model: "claude-sonnet-4", provider: "anthropic",
        usage: %{"input_tokens" => 812, "output_tokens" => 37}]
      StepContext.span(ctx, "model_call", opts, fn -> :ok end)

      "an answer to a #{input["kind"]}"
    end,
    input: %{"kind" => "${steps.classify.output.kind}"}
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
      unless deliver.(input["text"], ctx.idempotency_key) do
        raise Mcpd.StepException,
          message: "mail relay busy", class: :retryable, retry_after_ms: 2_000
      end

      %{"sent" => true}
    end,
    input: %{"text" => "${steps.draft.output}"},
    effect: :at_most_once,
    retry: [max_attempts: 3, backoff: :exponential, initial_ms: 500]
  )
  |> Workflow.output(%{"sent" => "${steps.send.output.sent}"})
# #endregion workflow

# #region worker
# Serve step bodies for as long as this node lives: register, long-poll for
# leases, heartbeat while a body runs, answer every 409 by its reason.
{:ok, worker} = StepWorker.start_link(mcpd, workflows: [support], concurrency: 8)
:ok = StepWorker.serve(worker)
# #endregion worker

# #region run
# Start a run from anywhere that holds an execute key, answer the wait from a
# webhook, then follow the journal to the end and read the run.
{:ok, run} = Mcpd.start_run(mcpd, support, input: %{"ticket" => "Q-42"})

# A repeat of this event id is a duplicate, not a second event.
{:ok, _} =
  Mcpd.publish_event(mcpd, "approved", "Q-42",
    payload: %{"by" => "ops"},
    event_id: "hook-1"
  )

mcpd |> Mcpd.watch_run(run.run_id) |> Enum.each(&IO.inspect({&1.seq, &1.kind}))
{:ok, detail} = Mcpd.get_run(mcpd, run.run_id)
IO.inspect({detail.run.status, detail.run.output})
# #endregion run

:ok = StepWorker.stop(worker)
Mcpd.shutdown(mcpd)
