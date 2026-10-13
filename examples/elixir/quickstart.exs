# A small support agent, instrumented with mcpd.
#
# Nothing here calls a real model or a real database: the point is the shape of
# the instrumentation, not the agent. Run it against a local mcpd:
#
#     mcpd serve --data ./mcpd.db
#     elixir quickstart.exs
#
# Then open http://localhost:4317 and read the run.

# `span` is a macro, because `do ... end` is a block. Macros are required.
require Mcpd
# #region init
# Every option also reads an environment variable (MCPD_ENDPOINT, MCPD_PROJECT,
# MCPD_API_KEY, ...), so `Mcpd.start_link()` on its own works too.
{:ok, mcpd} =
  Mcpd.start_link(
    endpoint: "http://localhost:4317",
    project: "support-desk",
    environment: "development",
    service_name: "support-agent",
    service_version: "1.4.0"
  )

# #endregion init

# ---------------------------------------------------------------------------
# The fake agent. Stand-ins for a model, a database and a flaky HTTP service.
# Each opens its own span, and none of them is passed a parent: the enclosing
# run is active on this process, so the tree assembles itself.

plan_answer = fn mcpd, question ->
  started = System.monotonic_time()
  # Where a real provider call goes. This is the shape Anthropic answers in.
  Process.sleep(40)
  first_token = Mcpd.ModelCall.first_token(started)
  answer = "Let me check on that."

  response = %{
    "id" => "msg_01",
    "type" => "message",
    "model" => "claude-sonnet-4-5",
    "stop_reason" => "end_turn",
    "content" => [%{"type" => "text", "text" => answer}],
    "usage" => %{"input_tokens" => 172, "cache_read_input_tokens" => 640, "output_tokens" => 37}
  }

  # The model, the token counts mcpd prices, the response id and the finish
  # reason, read from the response in one call.
  opts =
    [
      kind: :model_generation,
      input: [%{"role" => "user", "content" => question}],
      output: answer,
      attributes: first_token
    ] ++ Mcpd.ModelCall.options(response)

  Mcpd.span mcpd, "plan_answer", opts do
    answer
  end
end

look_up_order = fn mcpd, id ->
  Mcpd.span mcpd, "lookup_order", kind: :tool_call, input: %{"order_id" => id} do
    Process.sleep(15)
    "Order #{id} shipped on Tuesday."
  end
end

# A tool call that fails. The span is recorded as an error with the exception on
# it, and the exception still reaches the caller, so the agent decides what to
# do about it rather than mcpd deciding for it.
estimate_delivery = fn mcpd, id ->
  try do
    Mcpd.span mcpd, "estimate_delivery", kind: :tool_call, input: %{"order_id" => id} do
      raise "shipping service returned 503 for order #{id}"
    end
  rescue
    _ ->
      Mcpd.log(mcpd, :warn, "the shipping service is down, answering without an estimate")
      nil
  end
end

# Stand-in for a provider's stream, in the shape OpenAI streams chat completions.
fake_chat_stream = fn text ->
  base = %{"id" => "chatcmpl-01", "model" => "gpt-4o-mini"}
  delta = fn d -> Map.put(base, "choices", [%{"index" => 0, "delta" => d}]) end
  usage = %{"prompt_tokens" => 24, "completion_tokens" => 6, "total_tokens" => 30}

  chunks =
    Enum.map(String.split(text, ~r/(?= )/), &delta.(%{"content" => &1})) ++
      [
        Map.put(base, "choices", [%{"index" => 0, "delta" => %{}, "finish_reason" => "stop"}]),
        base |> Map.put("choices", []) |> Map.put("usage", usage)
      ]

  Stream.each(chunks, fn _ -> Process.sleep(5) end)
end

# #region stream
# A streamed model call. Read the wrapper instead of the provider's stream:
# every chunk comes through unchanged, and when the stream ends the span gets
# the time to first token, the text, the usage and the finish reason.
stream_answer = fn mcpd, question ->
  # Where your provider's streaming call goes.
  stream = fake_chat_stream.("Your order is in transit.")

  mcpd
  |> Mcpd.ModelCall.record_stream(stream, "stream_answer",
    input: [%{"role" => "user", "content" => question}]
  )
  |> Enum.map_join(fn chunk ->
    get_in(chunk, ["choices", Access.at(0), "delta", "content"]) || ""
  end)
end

# #endregion stream

# #region testing
# A unit test. Start `Mcpd.Testing` and pass its client where your code takes
# one, then read back what it recorded: no daemon, no port, no flush.
test_stream_answer = fn ->
  {:ok, testing} = Mcpd.Testing.start()
  answer = stream_answer.(testing.client, "Where is my order A-1042?")

  call = Mcpd.Testing.span(testing, "stream_answer")
  ^answer = call["output"]
  "gpt-4o-mini" = call["model"]
  6 = call["usage"]["output_tokens"]

  [%{span: %{"name" => "stream_answer"}}] = Mcpd.Testing.trees(testing)
  Mcpd.Testing.stop(testing)
end

# #endregion testing

# #region trace
question = "where is my order 41?"

# The block's value is the value of `span`. Inside it, this span is the active
# one for the process, so the helpers below find their parent without being
# handed it.
trace_id =
  Mcpd.span mcpd, "support_agent",
    kind: :agent_run,
    input: question,
    session_id: "session-8f21",
    user_id: "customer-4477",
    tags: ["support", "email"] do
    Mcpd.log(mcpd, :info, "handling a question", %{"channel" => "email"})

    plan = plan_answer.(mcpd, question)
    order = look_up_order.(mcpd, 41)
    estimate = estimate_delivery.(mcpd, 41)

    _answer =
      [plan, order, estimate && "It should arrive #{estimate}."]
      |> Enum.reject(&is_nil/1)
      |> Enum.join(" ")

    Mcpd.metric(mcpd, "support.answers", 1, kind: :counter, unit: "{answer}")
    Mcpd.Context.current().trace_id
  end

# #endregion trace

# A streamed call, and the unit test that checks what it records.
IO.puts("streamed: " <> stream_answer.(mcpd, "Is A-1042 on its way?"))
test_stream_answer.()

# #region score
# A machine evaluation. `min` and `max` are required: 7 is excellent out of 10
# and poor out of 100, and a score with no scale cannot be compared.
Mcpd.score(mcpd, trace_id, "answer_relevance", 0.86,
  min: 0,
  max: 1,
  reason: "answered the question asked, without the delivery estimate"
)

# A person's judgement, from the thumbs-up control in your own product.
Mcpd.feedback(mcpd, trace_id, :thumbs, "up",
  comment: "fast and clear",
  author_id: "customer-4477"
)

# Nothing above touched the network. This is where it goes.
:ok = Mcpd.flush(mcpd)
# #endregion score

# #region query
{:ok, detail} = Mcpd.get_trace(mcpd, trace_id, include: ["scores", "feedback"])

IO.puts("#{detail.trace.name}: #{length(detail.spans)} spans")

for span <- detail.spans do
  ms = div(span.duration_ns || 0, 1_000_000)
  error = (span.error && span.error.message) || ""
  IO.puts("  #{span.kind} #{span.name} #{ms}ms #{error}")
end

# #endregion query

# Send anything still queued and stop the worker. Without this, a process that
# exits quickly loses its last batch.
:ok = Mcpd.shutdown(mcpd)
