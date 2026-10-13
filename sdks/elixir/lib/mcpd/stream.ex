defmodule Mcpd.StreamAccumulator do
  @moduledoc false
  # What one streamed model call has said so far (contract section 3.2).
  #
  # A streamed response spreads the facts `Mcpd.ModelCall.options/2` reads
  # across its chunks: the id and model in the first, the text in the middle,
  # the finish reason and the usage at the end. This folds them in one chunk at
  # a time and builds the response the provider would have sent without
  # streaming, so the span records what it would have for the same call made
  # the other way.
  #
  # Text and tool-call arguments share one budget, and tool calls are capped by
  # count, so a long stream costs bounded memory. Text is kept as reversed
  # iodata, because appending to a binary chunk by chunk copies it each time.

  # Code points of text and tool-call arguments kept, together.
  @budget 32_768
  # Tool calls kept. Later ones are ignored.
  @max_tools 64

  @anthropic_events ~w(message_start content_block_start content_block_delta message_delta)

  defstruct shape: nil,
            truncated: false,
            id: nil,
            model: nil,
            tier: nil,
            text: [],
            budget: @budget,
            tools: %{},
            finish: %{},
            stop: nil,
            usage: nil

  @type t :: %__MODULE__{}

  def budget, do: @budget
  def max_tools, do: @max_tools

  def new, do: %__MODULE__{}

  @doc false
  # Fold one chunk in. Answers the new state and whether the chunk carried text
  # or a tool-call delta.
  @spec add(t(), term()) :: {t(), boolean()}
  def add(acc, chunk) do
    c = obj(chunk)

    cond do
      c == nil -> {acc, false}
      is_list(get(c, :choices)) -> openai(acc, c)
      # A typed chunk with no choices can serialise them as null.
      null_choices?(c) -> openai(acc, c)
      name(get(c, :type)) in @anthropic_events -> anthropic(acc, c, name(get(c, :type)))
      true -> {acc, false}
    end
  end

  defp openai(acc, c) do
    acc = %{
      acc
      | shape: acc.shape || "openai",
        id: acc.id || str(get(c, :id)),
        model: acc.model || str(get(c, :model)),
        tier: acc.tier || str(get(c, :service_tier))
    }

    acc =
      case obj(get(c, :usage)) do
        nil -> acc
        usage -> %{acc | usage: usage}
      end

    (get(c, :choices) || [])
    |> Enum.with_index()
    |> Enum.reduce({acc, false}, fn {raw, position}, {acc, content} ->
      case obj(raw) do
        nil -> {acc, content}
        choice -> openai_choice(acc, content, choice, index(get(choice, :index), position))
      end
    end)
  end

  defp openai_choice(acc, content, choice, at) do
    acc =
      case str(get(choice, :finish_reason)) do
        nil -> acc
        reason -> %{acc | finish: Map.put(acc.finish, at, reason)}
      end

    delta = obj(get(choice, :delta))

    if at != 0 or delta == nil do
      {acc, content}
    else
      {acc, content} =
        case str(get(delta, :content)) do
          nil -> {acc, content}
          text -> {append_text(acc, text), true}
        end

      case get(delta, :tool_calls) do
        calls when is_list(calls) ->
          calls
          |> Enum.with_index()
          |> Enum.reduce({acc, content}, fn {raw, i}, {acc, content} ->
            case obj(raw) do
              nil -> {acc, content}
              call -> {openai_tool(acc, call, index(get(call, :index), i)), true}
            end
          end)

        _ ->
          {acc, content}
      end
    end
  end

  defp openai_tool(acc, call, at) do
    fun = obj(get(call, :function))

    acc
    |> tool(at, str(get(call, :id)), str(get(fun, :name)))
    |> append_args(at, str(get(fun, :arguments)))
  end

  defp anthropic(acc, c, "message_start") do
    acc = %{acc | shape: acc.shape || "anthropic"}
    message = obj(get(c, :message))
    acc = %{acc | id: acc.id || str(get(message, :id)), model: acc.model || str(get(message, :model))}

    case obj(get(message, :usage)) do
      nil -> {acc, false}
      usage -> {%{acc | usage: usage}, false}
    end
  end

  defp anthropic(acc, c, "content_block_start") do
    acc = %{acc | shape: acc.shape || "anthropic"}
    block = obj(get(c, :content_block))

    cond do
      name(get(block, :type)) == "tool_use" ->
        at = index(get(c, :index), map_size(acc.tools))
        {tool(acc, at, str(get(block, :id)), str(get(block, :name))), true}

      (text = str(get(block, :text))) != nil ->
        {append_text(acc, text), true}

      true ->
        {acc, false}
    end
  end

  defp anthropic(acc, c, "content_block_delta") do
    acc = %{acc | shape: acc.shape || "anthropic"}
    delta = obj(get(c, :delta))

    case name(get(delta, :type)) do
      "text_delta" ->
        case str(get(delta, :text)) do
          nil -> {acc, false}
          text -> {append_text(acc, text), true}
        end

      "input_json_delta" ->
        case str(get(delta, :partial_json)) do
          nil -> {acc, false}
          args -> {append_args(acc, index(get(c, :index), -1), args), true}
        end

      _ ->
        {acc, false}
    end
  end

  defp anthropic(acc, c, "message_delta") do
    acc = %{acc | shape: acc.shape || "anthropic"}
    acc = %{acc | stop: str(get(obj(get(c, :delta)), :stop_reason)) || acc.stop}

    case obj(get(c, :usage)) do
      nil ->
        {acc, false}

      usage ->
        # Each field reported here replaces the one from `message_start`; a
        # field it leaves out or sends as null keeps the earlier value.
        fresh = :maps.filter(fn _k, v -> v != nil end, usage)
        {%{acc | usage: Map.merge(acc.usage || %{}, fresh)}, false}
    end
  end

  # The tool call at `at`, made on first sight with the id and name it arrived
  # with. Past the cap a new one is ignored and the stream marked truncated.
  defp tool(acc, at, id, name) do
    case Map.fetch(acc.tools, at) do
      {:ok, known} ->
        known = %{known | id: known.id || id, name: known.name || name}
        %{acc | tools: Map.put(acc.tools, at, known)}

      :error when map_size(acc.tools) >= @max_tools ->
        %{acc | truncated: true}

      :error ->
        %{acc | tools: Map.put(acc.tools, at, %{id: id, name: name, args: []})}
    end
  end

  defp append_text(acc, text) do
    {kept, acc} = take(acc, text)
    %{acc | text: [acc.text | kept]}
  end

  defp append_args(acc, _at, nil), do: acc

  defp append_args(acc, at, args) do
    case Map.fetch(acc.tools, at) do
      {:ok, known} ->
        {kept, acc} = take(acc, args)
        %{acc | tools: Map.put(acc.tools, at, %{known | args: [known.args | kept]})}

      :error ->
        acc
    end
  end

  # As much of `s` as the budget still allows, counted in code points.
  defp take(%{budget: budget} = acc, _s) when budget <= 0, do: {"", %{acc | truncated: true}}

  defp take(acc, s) do
    case walk(s, acc.budget, 0, 0) do
      {:all, n} -> {s, %{acc | budget: acc.budget - n}}
      {:cut, bytes} -> {binary_part(s, 0, bytes), %{acc | budget: 0, truncated: true}}
    end
  end

  # A byte that is not valid UTF-8 counts as one code point, so a malformed
  # chunk still costs budget rather than slipping past it.
  defp walk(<<>>, _left, n, _bytes), do: {:all, n}
  defp walk(_rest, 0, _n, bytes), do: {:cut, bytes}

  defp walk(<<c::utf8, rest::binary>>, left, n, bytes),
    do: walk(rest, left - 1, n + 1, bytes + byte_size(<<c::utf8>>))

  defp walk(<<_, rest::binary>>, left, n, bytes), do: walk(rest, left - 1, n + 1, bytes + 1)

  @doc false
  # The response the provider would have sent, or nil when no chunk was
  # recognised.
  @spec response(t()) :: map() | nil
  def response(%{shape: "openai"} = acc) do
    choices =
      acc.finish
      |> Enum.sort()
      |> Enum.map(fn {i, reason} -> %{"index" => i, "finish_reason" => reason} end)

    %{"object" => "chat.completion", "choices" => choices}
    |> put("id", acc.id)
    |> put("model", acc.model)
    |> put("service_tier", acc.tier)
    |> put("usage", acc.usage)
  end

  def response(%{shape: "anthropic"} = acc) do
    %{"type" => "message"}
    |> put("id", acc.id)
    |> put("model", acc.model)
    |> put("stop_reason", acc.stop)
    |> put("usage", acc.usage)
  end

  def response(_acc), do: nil

  @doc false
  # The text, or the text and the tool calls, or nil when neither arrived.
  @spec output(t()) :: term()
  def output(acc) do
    text = IO.iodata_to_binary(acc.text)

    if map_size(acc.tools) == 0 do
      if text == "", do: nil, else: text
    else
      calls =
        acc.tools
        |> Enum.sort_by(fn {at, _} -> at end)
        |> Enum.map(fn {_, t} ->
          %{"arguments" => IO.iodata_to_binary(t.args)}
          |> put("id", t.id)
          |> put("name", t.name)
        end)

      %{"content" => text, "tool_calls" => calls}
    end
  end

  defp put(map, _key, nil), do: map
  defp put(map, key, value), do: Map.put(map, key, value)

  defp obj(%{__struct__: _} = s), do: Map.from_struct(s)
  defp obj(m) when is_map(m), do: m
  defp obj(_), do: nil

  defp get(m, key) when is_map(m) do
    case Map.fetch(m, key) do
      {:ok, v} -> v
      :error -> Map.get(m, Atom.to_string(key))
    end
  end

  defp get(_, _), do: nil

  defp null_choices?(c) do
    Enum.any?([:choices, "choices"], &(Map.has_key?(c, &1) and Map.get(c, &1) == nil))
  end

  # An event or block type, written as a string or as an atom.
  defp name(v) when is_binary(v), do: v
  defp name(v) when is_atom(v) and v not in [nil, true, false], do: Atom.to_string(v)
  defp name(_), do: nil

  defp str(v) when is_binary(v) and v != "", do: v
  defp str(_), do: nil

  defp index(v, _fallback) when is_integer(v) and v >= 0, do: v
  defp index(_, fallback), do: fallback
end
