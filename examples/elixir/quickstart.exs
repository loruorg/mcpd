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

# Store the prompt the next region reads. Posting the same body again stores
# nothing new, so a deploy script can run this on every start.
{:ok, _stored} =
  Mcpd.create_prompt(mcpd,
    name: "support-reply",
    template: "You answer for {{ customer }}. Reply in two sentences: {{question}}",
    labels: ["production"]
  )

# #region prompt
# The version `production` points at, cached for a minute. The fallback is
# used only when the daemon cannot be reached and nothing was fetched yet.
{:ok, prompt} =
  Mcpd.get_prompt(mcpd, "support-reply",
    label: "production",
    fallback: "Reply in two sentences: {{question}}"
  )

# A variable the template names and the map lacks raises, rather than sending
# a prompt with a hole in it. Extra variables are ignored.
text = Mcpd.Prompts.render!(prompt, %{question: question, customer: "Ada"})

# The prompt's name and version go on the span, so a trace shows which version
# produced the answer, and `Mcpd.list_spans(mcpd, prompt: "support-reply")`
# finds every call that used it.
Mcpd.span mcpd, "reply", kind: :model_generation, input: text,
  attributes: Mcpd.Prompts.attributes(prompt) do
  "Your order 41 shipped on Tuesday."
end

# #endregion prompt

# Send anything still queued and stop the worker. Without this, a process that
# exits quickly loses its last batch.
:ok = Mcpd.shutdown(mcpd)
