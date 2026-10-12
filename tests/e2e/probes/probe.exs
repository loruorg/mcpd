# The canonical run, in Elixir. Prints its trace id on the last line.
{:ok, mcpd} = Mcpd.start_link(project: System.get_env("MCPD_PROJECT"))

# `span` is a macro, because `do ... end` is a block. Macros are required.
require Mcpd
trace_id =
  Mcpd.span mcpd, "support_agent",
    kind: :agent_run,
    input: "where is my order 41?",
    session_id: "sess-conformance",
    thread_id: "thread-7",
    user_id: "user-41",
    request_id: "req-9001",
    release: "2026.08.1" do
    Mcpd.span mcpd, "plan_answer",
      kind: :model_generation,
      model: "gpt-4o",
      provider: "openai",
      usage: %{"input_tokens" => 1180, "output_tokens" => 96} do
      :ok
    end

    Mcpd.span mcpd, "lookup_order", kind: :tool_call do
      :ok
    end

    try do
      Mcpd.span mcpd, "estimate_delivery", kind: :tool_call do
        raise "shipping service returned 503"
      end
    rescue
      _ -> :ok
    end

    # Both read the active span, so they are recorded inside the run.
    Mcpd.log(mcpd, :warn, "estimate_delivery failed, answering from the cached estimate", %{
      "step" => "estimate_delivery"
    })

    Mcpd.metric(mcpd, "conformance.run_steps", 3,
      kind: :counter,
      unit: "{step}",
      labels: %{"outcome" => "degraded", "lang" => "elixir"}
    )

    Mcpd.Context.current().trace_id
  end

Mcpd.score(mcpd, trace_id, "answer_relevance", 0.86, min: 0, max: 1, source: :live)
# No source: the suite compares what each SDK defaults to.
Mcpd.score(mcpd, trace_id, "scale_check", 3, min: 1, max: 5)
Mcpd.feedback(mcpd, trace_id, :thumbs, "up")
:ok = Mcpd.shutdown(mcpd)
IO.puts(trace_id)
