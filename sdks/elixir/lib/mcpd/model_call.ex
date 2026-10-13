defmodule Mcpd.ModelCall do
  @moduledoc """
  Model calls: what an OpenAI- or Anthropic-shaped response says about the
  call that produced it, as span options (contract section 3.1).

      response = call_the_model(question)

      Mcpd.span mcpd, "answer", [kind: :model_generation] ++ Mcpd.ModelCall.options(response) do
        response
      end

  An Elixir span is described by the options it was opened with, and the SDK
  has no mutable span handle for a `record_response` to sit on. So each piece
  here is a function that returns what such a call would have set: the
  `model:`, `provider:`, `usage:` and `attributes:` that `Mcpd.span/4` and
  `Mcpd.with_span/4` accept.

  Nothing here imports a provider SDK. A response is read as a map with string
  or atom keys, so a parsed JSON body, a provider library's struct and a
  hand-built fixture all work. A field that is missing or of the wrong type is
  left absent rather than raised on: instrumentation never raises (B1).
  """

  alias Mcpd.{InputTokenDetails, OutputTokenDetails, TokenUsage}

  @doc """
  Token usage from an OpenAI chat-completions or responses-API response, or
  `nil` when it reports none.

  The prompt total is gross, so the plain text share is the total less the
  cached and audio tokens.
  """
  @spec usage_from_openai(term()) :: TokenUsage.t() | nil
  def usage_from_openai(response) do
    with usage when usage != nil <- obj(get(response, :usage)) do
      input = int(get(usage, :prompt_tokens)) || int(get(usage, :input_tokens))
      output = int(get(usage, :completion_tokens)) || int(get(usage, :output_tokens))
      in_detail = obj(get(usage, :prompt_tokens_details)) || obj(get(usage, :input_tokens_details))
      out_detail = obj(get(usage, :completion_tokens_details)) || obj(get(usage, :output_tokens_details))
      cached = int(get(in_detail, :cached_tokens))
      audio_in = int(get(in_detail, :audio_tokens))

      text =
        if input != nil and (cached != nil or audio_in != nil),
          do: max(0, input - (cached || 0) - (audio_in || 0))

      input_details = compact(%InputTokenDetails{text: text, cache_read: cached, audio: audio_in})

      output_details =
        compact(%OutputTokenDetails{
          reasoning: int(get(out_detail, :reasoning_tokens)),
          audio: int(get(out_detail, :audio_tokens))
        })

      total = int(get(usage, :total_tokens)) || if(input != nil and output != nil, do: input + output)

      compact(%TokenUsage{
        input_tokens: input,
        output_tokens: output,
        total_tokens: total,
        input_details: input_details,
        output_details: output_details
      })
    end
  rescue
    _ -> nil
  end

  @doc """
  Token usage from an Anthropic messages response, or `nil` when it reports
  none.

  `input_tokens` there is net of the cache, so it is the text share, and the
  prompt's size is the sum of it and both cache meters.
  """
  @spec usage_from_anthropic(term()) :: TokenUsage.t() | nil
  def usage_from_anthropic(response) do
    with usage when usage != nil <- obj(get(response, :usage)) do
      text = int(get(usage, :input_tokens))
      read = int(get(usage, :cache_read_input_tokens))
      write = int(get(usage, :cache_creation_input_tokens))
      split = obj(get(usage, :cache_creation))
      output = int(get(usage, :output_tokens))

      input =
        if text == nil and read == nil and write == nil,
          do: nil,
          else: (text || 0) + (read || 0) + (write || 0)

      compact(%TokenUsage{
        input_tokens: input,
        output_tokens: output,
        total_tokens: if(input != nil and output != nil, do: input + output),
        input_details:
          compact(%InputTokenDetails{
            text: text,
            cache_read: read,
            cache_write: write,
            cache_write_5m: int(get(split, :ephemeral_5m_input_tokens)),
            cache_write_1h: int(get(split, :ephemeral_1h_input_tokens))
          })
      })
    end
  rescue
    _ -> nil
  end

  @doc """
  The span options a response implies: `model:`, `provider:`, `usage:` and
  `attributes:`, each only when the response reports it.

  The provider is `provider` when given, else `"openai"` or `"anthropic"` by
  the response's shape. The attributes are `gen_ai.response.id`,
  `gen_ai.response.finish_reasons` and `gen_ai.response.service_tier`; the
  daemon prices a batch or flex call from the last one. Never `input:` or
  `output:`: the text is the caller's to record or not, under payload capture.
  """
  @spec options(term(), String.t() | nil) :: keyword()
  def options(response, provider \\ nil) do
    r = obj(response) || %{}
    shape = shape(r)

    usage =
      case shape do
        "anthropic" -> usage_from_anthropic(r)
        "openai" -> usage_from_openai(r)
        nil -> nil
      end

    attributes =
      %{
        "gen_ai.response.id" => str(get(r, :id)),
        "gen_ai.response.finish_reasons" => finish_reasons(r),
        "gen_ai.response.service_tier" =>
          str(get(r, :service_tier)) || str(get(obj(get(r, :usage)), :service_tier))
      }
      |> drop_nils()

    [
      model: str(get(r, :model)),
      provider: (is_binary(provider) && provider != "" && provider) || shape,
      usage: usage,
      attributes: if(map_size(attributes) > 0, do: attributes)
    ]
    |> Enum.reject(fn {_k, v} -> v == nil end)
  rescue
    _ -> []
  end

  @doc """
  The time to first token as an attribute map, `mcpd.time_to_first_token_ms`,
  from `started` to now.

  `started` is a `System.monotonic_time/0` reading taken when the call began,
  in `unit` (native by default). The monotonic clock, because the wall clock
  can step backwards and a negative latency reads as a bug. With no mutable
  span to remember the first call, the caller takes the reading once, at the
  first token, and passes the map as the span's `attributes:`.
  """
  @spec first_token(integer(), System.time_unit()) :: map()
  def first_token(started, unit \\ :native) do
    elapsed = System.monotonic_time(:nanosecond) - System.convert_time_unit(started, unit, :nanosecond)
    %{"mcpd.time_to_first_token_ms" => max(elapsed, 0) / 1_000_000}
  rescue
    _ -> %{}
  end

  @doc """
  A guardrail's outcome as an attribute map: `mcpd.guardrail.tripped`, and
  `mcpd.guardrail.reason` when given.

  Tripping is not an error by itself; the caller decides whether the span
  also fails.
  """
  @spec guardrail(boolean(), String.t() | nil) :: map()
  def guardrail(tripped, reason \\ nil) do
    base = %{"mcpd.guardrail.tripped" => tripped == true}
    if is_binary(reason), do: Map.put(base, "mcpd.guardrail.reason", reason), else: base
  end

  @doc """
  Record a streamed model call (contract section 3.2).

      stream = call_the_model_streaming(question)

      mcpd
      |> Mcpd.ModelCall.record_stream(stream, "answer", input: question)
      |> Enum.map_join(&chunk_text/1)

  Returns a lazy stream that yields every element of `enumerable` unchanged,
  in order. Read it in place of the original. The span opens when the stream
  is first read, under the span active in the reading process, and is
  recorded once when reading stops: at the end, when you stop early
  (`Enum.take/2`, `Enum.find/2`, a halted `Stream`), or when the enumerable
  raises. A raise is recorded as the span's error and reaches you unchanged.

  What the chunks said becomes the span: the model, provider, usage and
  `gen_ai.response.*` attributes that `options/2` gives for the same call made
  without streaming, `mcpd.time_to_first_token_ms` at the first chunk that
  carries text or a tool-call delta, and the output: the text, or
  `%{"content" => text, "tool_calls" => [...]}` when a tool call arrived.
  OpenAI chat-completion chunks and Anthropic message events are read, as maps
  with string or atom keys; any other element passes through and is skipped.

  Text and tool-call arguments share a budget of 32,768 code points, and at
  most 64 tool calls are kept. Past either, the rest is not kept and
  `mcpd.stream.truncated` is set to `true`.

  `opts` takes the options `Mcpd.span/4` does, plus `provider:`. `kind:`
  defaults to `:model_generation`.
  """
  @spec record_stream(pid(), Enumerable.t(), String.t(), keyword()) :: Enumerable.t()
  def record_stream(client, enumerable, name, opts \\ []) do
    opts = Keyword.put_new(opts, :kind, :model_generation)

    # A fresh reference per enumeration, because a stream can be read more
    # than once and each reading is its own call.
    fn acc, fun ->
      ref = make_ref()

      enumerable
      |> guard(ref)
      |> Stream.transform(
        fn -> start_stream(client, name, opts) end,
        fn chunk, state -> {[chunk], observe(client, state, chunk)} end,
        fn state -> {[], state} end,
        fn state -> finish_stream(client, state, opts, Process.delete(ref)) end
      )
      |> Enumerable.reduce(acc, fun)
    end
  end

  defp start_stream(client, name, opts) do
    %{span: Mcpd.__open__(client, name, opts), acc: Mcpd.StreamAccumulator.new(), first: nil}
  end

  # Instrumentation never raises (B1): a chunk that breaks the accumulator is
  # reported and skipped, and the chunk still reaches the caller.
  defp observe(client, state, chunk) do
    {acc, content} = Mcpd.StreamAccumulator.add(state.acc, chunk)

    first =
      if content and state.first == nil,
        do: first_token(state.span.started_at, :nanosecond),
        else: state.first

    %{state | acc: acc, first: first}
  rescue
    e ->
      Mcpd.Worker.warn(client, "a stream chunk could not be read: #{Exception.message(e)}")
      state
  end

  defp finish_stream(client, state, opts, failure) do
    acc = state.acc
    {provider, opts} = Keyword.pop(opts, :provider)
    found = acc |> Mcpd.StreamAccumulator.response() |> then(&(&1 && options(&1, provider)))
    found = found || if(provider, do: [provider: provider], else: [])
    {found_attributes, found} = Keyword.pop(found, :attributes)
    truncated = if acc.truncated, do: %{"mcpd.stream.truncated" => true}

    # What the stream reported comes first, because a span reads the first
    # `model:` it is given; attributes merge with the later ones winning.
    close =
      found ++
        opts ++
        Enum.reject([attributes: found_attributes, attributes: state.first, attributes: truncated], fn {_k, v} ->
          v == nil
        end)

    output = Mcpd.StreamAccumulator.output(acc) || opts[:output]
    Mcpd.__close__(client, state.span, close, output, failure)
  rescue
    e -> Mcpd.Worker.warn(client, "a stream could not be recorded: #{Exception.message(e)}")
  end

  # Remembers what the provider's enumerable raised, so the span can carry it.
  # Only the source is wrapped: `Stream.transform/5` pulls it one element at a
  # time, so a raise in the caller's own code never passes through here and
  # never marks the call as failed.
  defp guard(enumerable, ref) do
    fn acc, fun -> guarded(&Enumerable.reduce(enumerable, &1, fun), acc, ref) end
  end

  defp guarded(reduce, acc, ref) do
    case reduce.(acc) do
      {:suspended, acc, next} -> {:suspended, acc, &guarded(next, &1, ref)}
      other -> other
    end
  catch
    kind, reason ->
      Process.put(ref, {kind, reason, __STACKTRACE__})
      :erlang.raise(kind, reason, __STACKTRACE__)
  end

  # ------------------------------------------------------------- internals ---

  # Which provider shaped this response, or nil when neither did. The order is
  # the contract's: Anthropic's `type` or cache meter first, because a responses
  # API body and a messages body both carry `input_tokens`.
  defp shape(r) do
    usage = obj(get(r, :usage))

    cond do
      get(r, :type) in ["message", :message] or has?(usage, :cache_read_input_tokens) -> "anthropic"
      has?(usage, :prompt_tokens) or has?(usage, :input_tokens_details) -> "openai"
      is_list(get(r, :choices)) or get(r, :object) in ["response", :response] -> "openai"
      has?(usage, :input_tokens) -> "anthropic"
      true -> nil
    end
  end

  # Each choice's `finish_reason`, or Anthropic's `stop_reason`, or, for a
  # responses-API body that carries neither, its `status`.
  defp finish_reasons(r) do
    choices = get(r, :choices)

    reasons =
      if(is_list(choices), do: Enum.map(choices, &str(get(obj(&1), :finish_reason))), else: []) ++
        [str(get(r, :stop_reason))]

    reasons = Enum.reject(reasons, &is_nil/1)

    reasons =
      if reasons == [] and get(r, :object) in ["response", :response],
        do: Enum.reject([str(get(r, :status))], &is_nil/1),
        else: reasons

    if reasons == [], do: nil, else: reasons
  end

  # A map to read from, or nil. A provider library's struct is read through
  # its fields, so nothing here has to know the library.
  defp obj(%{__struct__: _} = s), do: Map.from_struct(s)
  defp obj(m) when is_map(m), do: m
  defp obj(_), do: nil

  # A field by atom or string key, whichever the response uses.
  defp get(m, key) when is_map(m) do
    case Map.fetch(m, key) do
      {:ok, v} -> v
      :error -> Map.get(m, Atom.to_string(key))
    end
  end

  defp get(_, _), do: nil

  defp has?(m, key) when is_map(m), do: Map.has_key?(m, key) or Map.has_key?(m, Atom.to_string(key))
  defp has?(_, _), do: false

  defp int(v) when is_integer(v) and v >= 0, do: v
  defp int(v) when is_float(v) and v >= 0, do: trunc(v)
  defp int(_), do: nil

  defp str(v) when is_binary(v) and v != "", do: v
  defp str(_), do: nil

  # A struct with every field unset is no report at all, so it is nil rather
  # than an empty record that would read as "reported zero of everything".
  defp compact(%{__struct__: _} = s) do
    if s |> Map.from_struct() |> Enum.all?(fn {_k, v} -> v == nil end), do: nil, else: s
  end

  defp drop_nils(map), do: :maps.filter(fn _k, v -> v != nil end, map)
end
