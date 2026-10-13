defmodule Mcpd.TestingTest do
  @moduledoc "The test client: contract section 2.5."
  # Not async: one test sets MCPD_ENABLED and MCPD_SAMPLE for the whole VM.
  use ExUnit.Case, async: false

  alias Mcpd.Testing

  require Mcpd

  defp answer(mcpd) do
    Mcpd.span mcpd, "answer", output: "done" do
      Mcpd.span(mcpd, "lookup", [kind: :tool_call], do: :ok)
      Mcpd.span(mcpd, "generate", [kind: :model_generation, output: "it ships tomorrow"], do: :ok)
      Mcpd.log(mcpd, :info, "answered", %{"order" => "A-1"})
      trace_id = Mcpd.Context.current().trace_id
      Mcpd.score(mcpd, trace_id, "relevance", 0.9, min: 0, max: 1)
      Mcpd.feedback(mcpd, trace_id, :thumbs, 1)
      Mcpd.metric(mcpd, "answers", 1, kind: :counter)
    end
  end

  defp start(opts \\ []) do
    {:ok, t} = Testing.start(Keyword.merge([flush_interval: 60_000], opts))
    t
  end

  test "is a real client, and reads back every signal with no flush" do
    t = start()
    assert is_pid(t.client)
    assert %Mcpd.Config{sink: {Mcpd.Sink.Recorder, _}} = Mcpd.config(t.client)
    answer(t.client)
    assert Enum.map(Testing.spans(t), & &1["name"]) == ["answer", "lookup", "generate"]
    assert Enum.map(Testing.logs(t), & &1["message"]) == ["answered"]
    assert [%{"scorer_id" => "relevance"}] = Testing.scores(t)
    assert [%{"kind" => "thumbs"}] = Testing.recorded_feedback(t)
    assert [%{"name" => "answers"}] = Testing.metrics(t)
  end

  test "keeps one record per span id, finished spans only" do
    t = start()
    parent = self()

    open =
      spawn_link(fn ->
        Mcpd.span t.client, "still_running" do
          send(parent, :opened)

          receive do
            :finish -> :ok
          end
        end

        send(parent, :closed)
      end)

    assert_receive :opened
    answer(t.client)
    spans = Testing.spans(t)
    assert length(Mcpd.Sink.Recorder.spans(t.recorder)) > length(spans)
    assert spans |> Enum.map(& &1["span_id"]) |> Enum.uniq() |> length() == length(spans)
    refute "still_running" in Enum.map(spans, & &1["name"])

    send(open, :finish)
    assert_receive :closed
    assert "still_running" in Enum.map(Testing.spans(t), & &1["name"])
    # First arrival decides the order, so the span opened first comes first.
    assert hd(Testing.spans(t))["name"] == "still_running"
  end

  test "filters by name and kind" do
    t = start()
    answer(t.client)
    assert Enum.map(Testing.spans(t, kind: :tool_call), & &1["name"]) == ["lookup"]
    assert Enum.map(Testing.spans(t, kind: "tool_call"), & &1["name"]) == ["lookup"]
    assert [%{"output" => "it ships tomorrow"}] = Testing.spans(t, name: "generate")
  end

  test "span/2 fails on none and on two, giving the count" do
    t = start()
    answer(t.client)
    assert Testing.span(t, "generate")["output"] == "it ships tomorrow"

    error = assert_raise Testing.Error, fn -> Testing.span(t, "missing") end
    assert error.message =~ "found 0"
    assert error.message =~ "answer, lookup, generate"

    answer(t.client)
    assert_raise Testing.Error, ~r/found 2/, fn -> Testing.span(t, "lookup") end
  end

  test "builds the tree, children by start time" do
    t = start()
    answer(t.client)
    assert [root] = Testing.trees(t)
    assert root.span["name"] == "answer"
    assert Enum.map(root.children, & &1.span["name"]) == ["lookup", "generate"]
    assert Enum.all?(root.children, &(&1.children == []))
  end

  test "a span whose parent was not recorded is a root" do
    t = start()

    Mcpd.continue("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01", fn ->
      Mcpd.span(t.client, "orphan", do: :ok)
    end)

    assert [%{span: %{"name" => "orphan"}, children: []}] = Testing.trees(t)
  end

  test "reset forgets everything" do
    t = start()
    answer(t.client)
    assert :ok = Testing.reset(t)
    assert Testing.spans(t) == []
    assert Testing.logs(t) == []
    assert Testing.scores(t) == []
  end

  test "goes through the same redaction as a real client" do
    redact = fn
      v when is_binary(v) -> String.replace(v, "tomorrow", "[date]")
      v -> v
    end

    t = start(redact: redact)
    answer(t.client)
    assert Testing.span(t, "generate")["output"] == "it ships [date]"
  end

  test "ignores a sink it is given" do
    {:ok, other} = Mcpd.Sink.Recorder.start_link()
    t = start(sink: {Mcpd.Sink.Recorder, other})
    answer(t.client)
    assert length(Testing.spans(t)) == 3
    assert Mcpd.Sink.Recorder.spans(other) == []
  end

  test "stays on when the environment turns mcpd off" do
    System.put_env("MCPD_ENABLED", "false")
    System.put_env("MCPD_SAMPLE", "0")

    on_exit(fn ->
      System.delete_env("MCPD_ENABLED")
      System.delete_env("MCPD_SAMPLE")
    end)

    t = start()
    answer(t.client)
    assert length(Testing.spans(t)) == 3
  end

  test "honours enabled and sample given in code" do
    t = start(enabled: false)
    answer(t.client)
    assert Testing.spans(t) == []

    t = start(sample: 0.0)
    answer(t.client)
    assert Testing.spans(t) == []
  end

  test "stop/1 shuts the client down" do
    t = start()
    assert :ok = Testing.stop(t)
    assert Mcpd.config(t.client) == nil
  end
end
