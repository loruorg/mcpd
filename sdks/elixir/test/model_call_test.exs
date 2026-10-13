defmodule Mcpd.ModelCallTest do
  @moduledoc """
  Model calls and in-process redaction: contract sections 3.1 and 2.1.1,
  checked against what a sink is handed.
  """
  use ExUnit.Case, async: false

  import ExUnit.CaptureIO

  alias Mcpd.{InputTokenDetails, ModelCall, OutputTokenDetails, Sink, TokenUsage}

  require Mcpd

  @openai_chat %{
    "id" => "chatcmpl-1",
    "object" => "chat.completion",
    "model" => "gpt-4o-2024-08-06",
    "service_tier" => "flex",
    "choices" => [%{"index" => 0, "finish_reason" => "stop", "message" => %{"role" => "assistant", "content" => "hi"}}],
    "usage" => %{
      "prompt_tokens" => 1000,
      "completion_tokens" => 200,
      "total_tokens" => 1200,
      "prompt_tokens_details" => %{"cached_tokens" => 600, "audio_tokens" => 0},
      "completion_tokens_details" => %{"reasoning_tokens" => 50}
    }
  }

  # Atom keys, the shape a provider library decoding into maps of atoms hands
  # back, so both key styles are exercised.
  @anthropic_message %{
    id: "msg_1",
    type: "message",
    model: "claude-sonnet-4-5",
    stop_reason: "end_turn",
    usage: %{
      input_tokens: 100,
      cache_read_input_tokens: 900,
      cache_creation_input_tokens: 50,
      cache_creation: %{ephemeral_5m_input_tokens: 20, ephemeral_1h_input_tokens: 30},
      output_tokens: 40,
      service_tier: "batch"
    }
  }

  defmodule Usage do
    @moduledoc false
    defstruct [:input_tokens, :output_tokens, :input_tokens_details]
  end

  defmodule Response do
    @moduledoc false
    defstruct [:object, :usage]
  end

  defp recording_client(opts \\ []) do
    {:ok, recorder} = Sink.Recorder.start_link()

    {:ok, c} =
      Mcpd.start_link(
        Keyword.merge(
          [endpoint: "http://127.0.0.1:1", flush_interval: 60_000, sink: {Sink.Recorder, recorder}],
          opts
        )
      )

    on_exit(fn -> Mcpd.shutdown(c) end)
    {c, recorder}
  end

  defp span_named(recorder, name),
    do: recorder |> Sink.Recorder.spans() |> Enum.filter(&(&1["name"] == name)) |> List.last()

  describe "model calls" do
    test "maps OpenAI usage with the gross prompt split into text and cache" do
      assert ModelCall.usage_from_openai(@openai_chat) == %TokenUsage{
               input_tokens: 1000,
               output_tokens: 200,
               total_tokens: 1200,
               input_details: %InputTokenDetails{text: 400, cache_read: 600, audio: 0},
               output_details: %OutputTokenDetails{reasoning: 50}
             }

      responses = %{
        "object" => "response",
        "usage" => %{"input_tokens" => 10, "output_tokens" => 5, "input_tokens_details" => %{"cached_tokens" => 4}}
      }

      expected = %TokenUsage{
        input_tokens: 10,
        output_tokens: 5,
        total_tokens: 15,
        input_details: %InputTokenDetails{text: 6, cache_read: 4}
      }

      assert ModelCall.usage_from_openai(responses) == expected

      # A provider library's structs are read through their fields.
      struct = %Response{
        object: "response",
        usage: %Usage{input_tokens: 10, output_tokens: 5, input_tokens_details: %{cached_tokens: 4}}
      }

      assert ModelCall.usage_from_openai(struct) == expected
    end

    test "maps Anthropic usage, whose input is net of the cache" do
      assert ModelCall.usage_from_anthropic(@anthropic_message) == %TokenUsage{
               input_tokens: 1050,
               output_tokens: 40,
               total_tokens: 1090,
               input_details: %InputTokenDetails{
                 text: 100,
                 cache_read: 900,
                 cache_write: 50,
                 cache_write_5m: 20,
                 cache_write_1h: 30
               }
             }
    end

    test "reads nothing from a shape it does not know, and never raises" do
      assert ModelCall.usage_from_openai(nil) == nil
      assert ModelCall.usage_from_anthropic(%{"usage" => %{"input_tokens" => "many"}}) == nil
      assert ModelCall.options(42) == []
      assert ModelCall.options(%{"usage" => "x"}) == []
      assert ModelCall.options([1, 2]) == []
      assert ModelCall.first_token(:not_a_time) == %{}
    end

    test "records the model, usage, id, finish reasons and tier, and never the text" do
      {c, recorder} = recording_client()

      Mcpd.span c, "a", [kind: :model_generation] ++ ModelCall.options(@openai_chat) do
        :ok
      end

      Mcpd.span c, "b", [kind: :model_generation] ++ ModelCall.options(@anthropic_message, "aws.bedrock") do
        :ok
      end

      :ok = Mcpd.flush(c)

      a = span_named(recorder, "a")
      b = span_named(recorder, "b")
      assert a["model"] == "gpt-4o-2024-08-06"
      assert a["provider"] == "openai"
      refute Map.has_key?(a, "output")

      assert a["attributes"] == %{
               "gen_ai.response.id" => "chatcmpl-1",
               "gen_ai.response.finish_reasons" => ["stop"],
               "gen_ai.response.service_tier" => "flex"
             }

      assert a["usage"]["input_tokens"] == 1000
      assert a["usage"]["input_details"] == %{"text" => 400, "cache_read" => 600, "audio" => 0}
      assert b["provider"] == "aws.bedrock"
      assert b["attributes"]["gen_ai.response.service_tier"] == "batch"
      assert b["attributes"]["gen_ai.response.finish_reasons"] == ["end_turn"]
    end

    test "records the first token, and a guardrail outcome, beside the response's attributes" do
      {c, recorder} = recording_client()
      started = System.monotonic_time()
      ttft = ModelCall.first_token(started)
      Process.sleep(5)

      opts =
        [attributes: ttft, attributes: ModelCall.guardrail(true, "pii in output")] ++
          ModelCall.options(@openai_chat)

      Mcpd.span(c, "stream", opts, do: :ok)
      :ok = Mcpd.flush(c)

      attrs = span_named(recorder, "stream")["attributes"]
      assert is_float(attrs["mcpd.time_to_first_token_ms"])
      assert attrs["mcpd.time_to_first_token_ms"] < 5
      assert attrs["mcpd.guardrail.tripped"] == true
      assert attrs["mcpd.guardrail.reason"] == "pii in output"
      assert attrs["gen_ai.response.id"] == "chatcmpl-1"
      assert ModelCall.guardrail(false) == %{"mcpd.guardrail.tripped" => false}
    end
  end

  describe "redact" do
    test "rewrites payloads before they are queued, and drops what it raises on" do
      redact = fn value ->
        if is_binary(value) and String.contains?(value, "boom"), do: raise("redactor broke")
        value |> :json.encode() |> IO.iodata_to_binary() |> String.replace(~r/\d{4}-\d{4}/, "****") |> :json.decode()
      end

      {c, recorder} = recording_client(redact: redact)

      stderr =
        capture_io(:stderr, fn ->
          Mcpd.span(c, "run", [input: %{"card" => "1234-5678"}, output: "boom"], do: :ok)
          Mcpd.log(c, :info, "paid", %{card: "1234-5678"})
          :ok = Mcpd.flush(c)
        end)

      rec = span_named(recorder, "run")
      assert rec["input"] == %{"card" => "****"}
      refute Map.has_key?(rec, "output")
      assert [%{"fields" => %{"card" => "****"}}] = Sink.Recorder.logs(recorder)
      assert Mcpd.stats(c).redaction_failures == 1
      assert stderr =~ "redact failed on output"
    end

    test "a redactor that is not a function fails closed" do
      {c, recorder} = recording_client(redact: :not_a_function)

      capture_io(:stderr, fn ->
        Mcpd.span(c, "run", [input: "secret"], do: :ok)
        :ok = Mcpd.flush(c)
      end)

      refute Map.has_key?(span_named(recorder, "run"), "input")
      assert Mcpd.stats(c).redaction_failures >= 1
    end

    test "a map field answered with something that is not a map is dropped and counted; nil is not" do
      redact = fn
        %{"drop" => _} -> nil
        %{} -> "not a map"
        value -> value
      end

      {c, recorder} = recording_client(redact: redact)

      capture_io(:stderr, fn ->
        Mcpd.span(c, "run", [input: "kept", metadata: %{"team" => "a"}, attributes: %{"drop" => 1}], do: :ok)
        Mcpd.log(c, :info, "line", %{card: "1234-5678"})
        :ok = Mcpd.flush(c)
      end)

      rec = span_named(recorder, "run")
      assert rec["input"] == "kept"
      refute Map.has_key?(rec, "metadata")
      refute Map.has_key?(rec, "attributes")
      assert [log] = Sink.Recorder.logs(recorder)
      refute Map.has_key?(log, "fields")
      # Metadata on the open record and the close record, and the log's fields.
      assert Mcpd.stats(c).redaction_failures == 3
    end

    test "never sees what payload capture already stripped" do
      {:ok, seen} = Agent.start_link(fn -> [] end)
      redact = fn value -> Agent.update(seen, &[value | &1]) && value end
      {c, _recorder} = recording_client(redact: redact, capture_payloads: false)

      Mcpd.span(c, "run", [input: "secret", metadata: %{"team" => "a"}], do: :ok)
      :ok = Mcpd.flush(c)

      refute "secret" in Agent.get(seen, & &1)
      assert %{"team" => "a"} in Agent.get(seen, & &1)
    end
  end
end
