# An experiment in Elixir: run every case of a dataset through your own code,
# score each answer against its case, and read the results.
#
# The regions are read into the documentation at build time.
{:ok, mcpd} = Mcpd.start_link(project: "support")

# The code under test. Anything it records nests under the case's run, because
# the case's span is the active one in the process that calls it.
answer = fn %{"question" => question} ->
  Mcpd.with_span(mcpd, "model_call", [kind: :model_generation, model: "claude-sonnet-4"], fn ->
    if String.contains?(question, "refund"),
      do: "Refunds take five days.",
      else: "It ships tomorrow."
  end)
end

# #region experiment
# Each case runs in its own task and root span, four at a time. A case that
# raises is recorded as a failed run, and the others still run.
{:ok, results} =
  Mcpd.run_experiment(mcpd, "support-questions", answer,
    name: "answers-v2",
    scorers: ["exact_match", "similarity"],
    metadata: %{"prompt" => "answer@4"}
  )

for scorer <- results.scorers do
  mean = :erlang.float_to_binary(scorer.mean / 1, decimals: 2)
  IO.puts("#{scorer.scorer_id}: mean #{mean} over #{scorer.n}")
end

for item <- results.items, item.status == :error do
  IO.puts("case #{item.item_id} failed: #{item.trace_id}")
end

# Read one run back as the steps its agent took.
case results.items do
  [first | _] ->
    {:ok, trajectory} = Mcpd.get_trajectory(mcpd, first.trace_id)

    IO.puts(
      "#{trajectory.step_count} steps, tools: #{Enum.join(trajectory.tool_sequence, " > ")}"
    )

  [] ->
    :ok
end

# #endregion experiment

Mcpd.shutdown(mcpd)
