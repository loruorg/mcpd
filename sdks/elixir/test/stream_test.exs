defmodule Mcpd.StreamTest do
  @moduledoc """
  Streamed model calls: contract section 3.2, read back through the test
  client so every assertion sees what a daemon would have received.
  """
  use ExUnit.Case, async: true

  alias Mcpd.{ModelCall, StreamAccumulator, Testing}

  require Mcpd

  @openai_chunks [
    %{"id" => "chatcmpl-1", "model" => "gpt-4o", "choices" => [%{"index" => 0, "delta" => %{"role" => "assistant"}}]},
    %{"id" => "chatcmpl-1", "model" => "gpt-4o", "choices" => [%{"index" => 0, "delta" => %{"content" => "Hel"}}]},
    %{"id" => "chatcmpl-1", "model" => "gpt-4o", "choices" => [%{"index" => 0, "delta" => %{"content" => "lo"}}]},
    %{
      "id" => "chatcmpl-1",
      "model" => "gpt-4o",
      "choices" => [%{"index" => 0, "delta" => %{}, "finish_reason" => "stop"}]
    },
    %{
      "id" => "chatcmpl-1",
      "model" => "gpt-4o",
      "service_tier" => "flex",
      "choices" => [],
      "usage" => %{
        "prompt_tokens" => 10,
        "completion_tokens" => 2,
        "total_tokens" => 12,
        "prompt_tokens_details" => %{"cached_tokens" => 4}
      }
    }
  ]

  # Atom keys, the shape a library decoding into atoms hands back.
  @anthropic_events [
    %{
      type: "message_start",
      message: %{
        id: "msg_1",
        type: "message",
        model: "claude-sonnet-4-5",
        usage: %{input_tokens: 20, cache_read_input_tokens: 80, output_tokens: 1}
      }
    },
    %{type: "content_block_start", index: 0, content_block: %{type: "text", text: ""}},
    %{type: "content_block_delta", index: 0, delta: %{type: "text_delta", text: "Checking"}},
    %{type: "content_block_stop", index: 0},
    %{type: "content_block_start", index: 1, content_block: %{type: "tool_use", id: "toolu_1", name: "lookup", input: %{}}},
    %{type: "content_block_delta", index: 1, delta: %{type: "input_json_delta", partial_json: ~s({"id":)}},
    %{type: "content_block_delta", index: 1, delta: %{type: "input_json_delta", partial_json: ~s("A-1"})}},
    %{type: "message_delta", delta: %{stop_reason: "tool_use"}, usage: %{output_tokens: 15}},
    %{type: "message_stop"}
  ]

  defp testing(opts \\ []) do
    {:ok, t} = Testing.start(Keyword.merge([flush_interval: 60_000], opts))
    t
  end

  # A source that raises after `n` elements, the way a dropped connection does.
  defp failing(items, n) do
    Stream.map(Stream.with_index(items), fn
      {_item, ^n} -> raise ArgumentError, "connection reset"
      {item, _} -> item
    end)
  end

  test "reads the usage from a chunk whose choices are null" do
    t = testing()
    usage = %{"prompt_tokens" => 3, "completion_tokens" => 1, "total_tokens" => 4}
    chunk = %{"model" => "gpt-4o", "choices" => nil, "usage" => usage}
    t.client |> ModelCall.record_stream([chunk], "call") |> Enum.to_list()
    s = Testing.span(t, "call")
    assert s["usage"]["output_tokens"] == 1
    assert s["provider"] == "openai"
  end

  test "records an OpenAI stream as options/2 would, and passes every chunk through" do
    t = testing()
    chunks = @openai_chunks ++ ["not a chunk", 42]
    seen = t.client |> ModelCall.record_stream(chunks, "call") |> Enum.to_list()
    assert seen == chunks

    s = Testing.span(t, "call")
    assert s["kind"] == "model_generation"
    assert s["model"] == "gpt-4o"
    assert s["provider"] == "openai"
    assert s["output"] == "Hello"
    assert s["status"] == "ok"

    assert s["usage"] == %{
             "input_tokens" => 10,
             "output_tokens" => 2,
             "total_tokens" => 12,
             "input_details" => %{"text" => 6, "cache_read" => 4}
           }

    attrs = s["attributes"]
    assert attrs["gen_ai.response.id"] == "chatcmpl-1"
    assert attrs["gen_ai.response.finish_reasons"] == ["stop"]
    assert attrs["gen_ai.response.service_tier"] == "flex"
    assert is_number(attrs["mcpd.time_to_first_token_ms"])
    refute Map.has_key?(attrs, "mcpd.stream.truncated")
  end

  test "records the same model, usage and attributes as the call made without streaming" do
    t = testing()
    t.client |> ModelCall.record_stream(@openai_chunks, "call") |> Stream.run()
    s = Testing.span(t, "call")

    whole = %{
      "object" => "chat.completion",
      "id" => "chatcmpl-1",
      "model" => "gpt-4o",
      "service_tier" => "flex",
      "choices" => [%{"index" => 0, "finish_reason" => "stop"}],
      "usage" => List.last(@openai_chunks)["usage"]
    }

    expected = ModelCall.options(whole)
    assert s["model"] == expected[:model]
    assert s["provider"] == expected[:provider]
    assert s["usage"]["input_tokens"] == expected[:usage].input_tokens
    assert Map.take(s["attributes"], Map.keys(expected[:attributes])) == expected[:attributes]
  end

  test "joins Anthropic tool-call deltas and merges message_delta usage" do
    t = testing()
    t.client |> ModelCall.record_stream(@anthropic_events, "call") |> Stream.run()
    s = Testing.span(t, "call")
    assert s["model"] == "claude-sonnet-4-5"
    assert s["provider"] == "anthropic"

    assert s["output"] == %{
             "content" => "Checking",
             "tool_calls" => [%{"id" => "toolu_1", "name" => "lookup", "arguments" => ~s({"id":"A-1"})}]
           }

    assert s["usage"]["input_tokens"] == 100
    assert s["usage"]["output_tokens"] == 15
    assert s["attributes"]["gen_ai.response.id"] == "msg_1"
    assert s["attributes"]["gen_ai.response.finish_reasons"] == ["tool_use"]
  end

  test "joins OpenAI tool-call deltas by index" do
    t = testing()
    call = fn d -> %{"choices" => [%{"index" => 0, "delta" => %{"tool_calls" => [d]}}]} end

    chunks = [
      call.(%{"index" => 0, "id" => "call_1", "function" => %{"name" => "lookup", "arguments" => ~s({"a")}}),
      call.(%{"index" => 1, "id" => "call_2", "function" => %{"name" => "other", "arguments" => "{}"}}),
      call.(%{"index" => 0, "function" => %{"arguments" => ":1}"}}),
      %{"choices" => [%{"index" => 0, "delta" => %{}, "finish_reason" => "tool_calls"}]}
    ]

    t.client |> ModelCall.record_stream(chunks, "call", provider: "azure") |> Stream.run()
    s = Testing.span(t, "call")
    assert s["provider"] == "azure"

    assert s["output"] == %{
             "content" => "",
             "tool_calls" => [
               %{"id" => "call_1", "name" => "lookup", "arguments" => ~s({"a":1})},
               %{"id" => "call_2", "name" => "other", "arguments" => "{}"}
             ]
           }
  end

  test "opens the span at the first read and marks the first token once, at the first content chunk" do
    t = testing()
    parent = self()

    # The source tells the test when the role-only chunk has been read, then
    # waits, so the time to first token has to cover the wait.
    source =
      Stream.flat_map(@openai_chunks, fn chunk ->
        if chunk == Enum.at(@openai_chunks, 1), do: Process.sleep(30)
        if chunk == Enum.at(@openai_chunks, 2), do: Process.sleep(40)
        send(parent, :read)
        [chunk]
      end)

    stream = ModelCall.record_stream(t.client, source, "call")
    # Building the stream reads nothing and opens nothing.
    Process.sleep(20)
    Mcpd.flush(t.client)
    assert Mcpd.Sink.Recorder.spans(t.recorder) == []

    Stream.run(stream)
    assert_received :read
    s = Testing.span(t, "call")
    ttft = s["attributes"]["mcpd.time_to_first_token_ms"]
    assert ttft >= 30
    # Only the first content chunk counts, not a later one.
    assert ttft < s["duration_ns"] / 1_000_000 - 30
  end

  test "a stream that raises ends the span with the error, and the error reaches the caller unchanged" do
    t = testing()

    error =
      assert_raise ArgumentError, fn ->
        t.client |> ModelCall.record_stream(failing(@openai_chunks, 3), "call") |> Enum.to_list()
      end

    assert error.message == "connection reset"
    s = Testing.span(t, "call")
    assert s["status"] == "error"
    assert s["error"]["message"] == "connection reset"
    assert s["error"]["kind"] == "ArgumentError"
    assert s["output"] == "Hello"
  end

  test "a raise in the caller's own code ends the span without marking the call failed" do
    t = testing()

    assert_raise RuntimeError, fn ->
      t.client
      |> ModelCall.record_stream(@openai_chunks, "call")
      |> Enum.each(fn chunk -> if chunk == Enum.at(@openai_chunks, 2), do: raise("mine") end)
    end

    s = Testing.span(t, "call")
    assert s["status"] == "ok"
    assert s["output"] == "Hello"
  end

  test "a caller that stops early still ends the span, once" do
    t = testing()
    taken = t.client |> ModelCall.record_stream(@openai_chunks, "call") |> Enum.take(2)
    assert taken == Enum.take(@openai_chunks, 2)

    assert [s] = Testing.spans(t, name: "call")
    assert s["output"] == "Hel"
    assert s["status"] == "ok"

    found =
      t.client
      |> ModelCall.record_stream(@openai_chunks, "find")
      |> Enum.find(&(&1["choices"] != [] and get_in(&1, ["choices", Access.at(0), "delta", "content"]) == "lo"))

    assert found == Enum.at(@openai_chunks, 2)
    assert Testing.span(t, "find")["output"] == "Hello"
  end

  test "each reading of the stream is its own call" do
    t = testing()
    stream = ModelCall.record_stream(t.client, @openai_chunks, "call")
    Stream.run(stream)
    Stream.run(stream)
    assert length(Testing.spans(t, name: "call")) == 2
  end

  test "nests under the caller's active span" do
    t = testing()

    Mcpd.span t.client, "run" do
      t.client |> ModelCall.record_stream(@openai_chunks, "call") |> Stream.run()
    end

    assert [%{span: %{"name" => "run"}, children: [%{span: %{"name" => "call"} = call}]}] = Testing.trees(t)
    assert call["parent_span_id"] == Testing.span(t, "run")["span_id"]
  end

  test "cuts text past the budget, in code points, and says so" do
    t = testing()
    big = String.duplicate("😀", StreamAccumulator.budget() - 1) <> "ab"
    chunks = [%{"choices" => [%{"index" => 0, "delta" => %{"content" => big}}]}]
    t.client |> ModelCall.record_stream(chunks, "call") |> Stream.run()
    s = Testing.span(t, "call")
    assert length(String.codepoints(s["output"])) == StreamAccumulator.budget()
    assert String.ends_with?(s["output"], "😀a")
    assert s["attributes"]["mcpd.stream.truncated"] == true
  end

  test "text and tool-call arguments share the budget" do
    t = testing()
    half = String.duplicate("x", div(StreamAccumulator.budget(), 2))
    tool = %{"index" => 0, "id" => "c", "function" => %{"name" => "f", "arguments" => half <> half <> "!"}}

    chunks = [
      %{"choices" => [%{"index" => 0, "delta" => %{"content" => half}}]},
      %{"choices" => [%{"index" => 0, "delta" => %{"tool_calls" => [tool]}}]}
    ]

    t.client |> ModelCall.record_stream(chunks, "call") |> Stream.run()
    s = Testing.span(t, "call")
    assert s["output"]["content"] == half
    assert [%{"arguments" => ^half}] = s["output"]["tool_calls"]
    assert s["attributes"]["mcpd.stream.truncated"] == true
  end

  test "keeps at most 64 tool calls" do
    t = testing()

    calls =
      for i <- 0..64 do
        %{"index" => i, "id" => "c#{i}", "function" => %{"name" => "f", "arguments" => "{}"}}
      end

    chunks = [%{"choices" => [%{"index" => 0, "delta" => %{"tool_calls" => calls}}]}]
    t.client |> ModelCall.record_stream(chunks, "call") |> Stream.run()
    s = Testing.span(t, "call")
    assert length(s["output"]["tool_calls"]) == StreamAccumulator.max_tools()
    assert s["attributes"]["mcpd.stream.truncated"] == true
  end

  test "keeps the output in the process with payload capture off" do
    t = testing(capture_payloads: false)
    t.client |> ModelCall.record_stream(@openai_chunks, "call") |> Stream.run()
    s = Testing.span(t, "call")
    refute Map.has_key?(s, "output")
    assert s["model"] == "gpt-4o"
  end

  test "a stream with nothing recognisable sets no output and still ends" do
    t = testing()
    seen = t.client |> ModelCall.record_stream([%{"choices" => "nope"}, nil, {:tuple}], "call") |> Enum.to_list()
    assert seen == [%{"choices" => "nope"}, nil, {:tuple}]
    s = Testing.span(t, "call")
    refute Map.has_key?(s, "output")
    refute Map.has_key?(s, "model")
    assert s["status"] == "ok"
  end
end
