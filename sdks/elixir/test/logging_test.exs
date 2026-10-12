defmodule Mcpd.LoggingTest do
  @moduledoc """
  The logging signal of contract section 4: correlation that survives an absent
  override, an exception serialised rather than costing its batch, the level
  floor, and the OpenTelemetry severity mapping.
  """
  use ExUnit.Case, async: false

  alias Mcpd.{Config, FakeDaemon, Logs, Sink}

  require Mcpd

  @accepted ~s({"accepted":1,"rejected":0,"duplicate":0})

  defp recorder(opts \\ []) do
    {:ok, sink} = Sink.Recorder.start_link()

    {:ok, client} =
      Mcpd.start_link(
        Keyword.merge(
          [
            endpoint: "http://127.0.0.1:1",
            project: "test",
            flush_interval: 60_000,
            sink: {Sink.Recorder, sink}
          ],
          opts
        )
      )

    on_exit(fn -> Mcpd.shutdown(client) end)
    {client, sink}
  end

  describe "a correlation key with no value (P301)" do
    test "nil on a log line leaves the run's own session standing" do
      {c, sink} = recorder()

      Mcpd.span c, "run", session_id: "session-7", user_id: "u-1" do
        Mcpd.log(c, :info, "inside")
        Mcpd.metric(c, "latency_ms", 4)
      end

      :ok = Mcpd.flush(c)

      assert [%{"correlation" => correlation}] = Sink.Recorder.logs(sink)
      assert correlation["session_id"] == "session-7"
      assert correlation["user_id"] == "u-1"
      assert [%{"correlation" => %{"session_id" => "session-7"}}] = Sink.Recorder.metrics(sink)
    end
  end

  describe "an exception under a field key (P300)" do
    test "arrives as its type and message" do
      {c, sink} = recorder()
      Mcpd.log(c, :error, "the call failed", %{"error" => %RuntimeError{message: "boom"}, "attempt" => 2})
      Mcpd.metric(c, "failures", 1, labels: %{"cause" => %ArgumentError{message: "label"}})
      :ok = Mcpd.flush(c)

      assert [%{"fields" => fields}] = Sink.Recorder.logs(sink)
      assert fields["attempt"] == 2
      assert fields["error"] == %{"type" => "RuntimeError", "message" => "boom"}
      assert [%{"labels" => labels}] = Sink.Recorder.metrics(sink)
      assert labels["cause"] == %{"type" => "ArgumentError", "message" => "label"}
    end

    test "a value nothing can encode costs its key, not the batch it travelled in" do
      # `:json.encode/1` raises on a tuple and on a pid, and the whole batch
      # went with it: the span, the metric and every other signal beside it.
      daemon = FakeDaemon.start(fn _n -> {200, @accepted} end)
      on_exit(fn -> FakeDaemon.stop(daemon) end)

      {:ok, c} =
        Mcpd.start_link(endpoint: daemon.endpoint, project: "test", flush_interval: 60_000)

      on_exit(fn -> Mcpd.shutdown(c) end)

      Mcpd.span c, "run" do
        Mcpd.log(c, :error, "a request went wrong", %{
          "reply" => {:error, :timeout},
          "who" => self(),
          "size" => 10
        })
      end

      :ok = Mcpd.flush(c)

      assert [line] = FakeDaemon.signals(daemon, "logs")
      assert line["fields"]["size"] == 10
      assert line["fields"]["reply"] == "{:error, :timeout}"
      assert line["fields"]["who"] =~ "#PID"
      assert length(FakeDaemon.signals(daemon, "spans")) > 0, "the batch survived the bad field"
      assert %{failed: 0} = Mcpd.stats(c)
    end

    test "a span's payload and a feedback value cost themselves, not the batch" do
      # A body that returns `{:ok, value}` is captured as the span's output.
      # `:json.encode/1` raised on the tuple and the sender lost every signal
      # in the batch with it.
      daemon = FakeDaemon.start(fn _n -> {200, @accepted} end)
      on_exit(fn -> FakeDaemon.stop(daemon) end)

      {:ok, c} =
        Mcpd.start_link(endpoint: daemon.endpoint, project: "test", flush_interval: 60_000)

      on_exit(fn -> Mcpd.shutdown(c) end)

      Mcpd.with_span(
        c,
        "lookup",
        [
          input: {:order, 42},
          capture_output: true,
          metadata: %{"caller" => self()},
          usage: [input_tokens: 3, output_tokens: 1]
        ],
        fn -> {:ok, %{"id" => 42}} end
      )

      Mcpd.record(c, %{
        "trace_id" => "4bf92f3577b34da6a3ce929d0e0e4736",
        "span_id" => "00f067aa0ba902b7",
        "name" => "recorded",
        "kind" => "tool_call",
        "status" => "ok",
        "start_time" => "2026-09-01T10:00:00Z",
        "is_root" => true,
        "is_event" => false,
        "received_at" => "2026-09-01T10:00:00Z",
        "output" => {:error, :closed}
      })

      Mcpd.feedback(c, "4bf92f3577b34da6a3ce929d0e0e4736", :thumbs, {:up, "fast"})
      Mcpd.log(c, :info, "beside them")
      :ok = Mcpd.flush(c)

      spans = FakeDaemon.signals(daemon, "spans")
      lookup = Enum.find(spans, &(&1["name"] == "lookup" and Map.has_key?(&1, "end_time")))
      assert lookup["input"] == "{:order, 42}"
      assert lookup["output"] == ~s({:ok, %{"id" => 42}})
      assert lookup["metadata"]["caller"] =~ "#PID"
      assert lookup["usage"] == %{"input_tokens" => 3, "output_tokens" => 1}
      assert Enum.find(spans, &(&1["name"] == "recorded"))["output"] == "{:error, :closed}"
      assert [%{"value" => ~s({:up, "fast"})}] = FakeDaemon.signals(daemon, "feedback")
      assert [%{"message" => "beside them"}] = FakeDaemon.signals(daemon, "logs")
      assert %{failed: 0} = Mcpd.stats(c)
    end

    test "describe_error answers only for an exception" do
      assert Logs.describe_error(%RuntimeError{message: "boom"}) ==
               %{"type" => "RuntimeError", "message" => "boom"}

      assert Logs.describe_error("not an exception") == nil
      assert Logs.describe_error(%{a: 1}) == nil
    end
  end

  describe "the level floor (P085)" do
    test "drops trace by default and keeps debug" do
      {c, sink} = recorder()
      assert Config.new().log_level == :debug
      Mcpd.log(c, :trace, "finer than debug")
      Mcpd.log(c, :debug, "kept")
      :ok = Mcpd.flush(c)

      assert Enum.map(Sink.Recorder.logs(sink), & &1["message"]) == ["kept"]
    end

    test "a floor set in code drops everything below it" do
      {c, sink} = recorder(log_level: :warn)
      for level <- Mcpd.LogLevel.values(), do: Mcpd.log(c, level, to_string(level))
      :ok = Mcpd.flush(c)

      assert Enum.map(Sink.Recorder.logs(sink), & &1["message"]) == ["warn", "error", "fatal"]
      assert %{dropped: 0} = Mcpd.stats(c)
    end

    test "reads MCPD_LOG_LEVEL, and code wins over it" do
      System.put_env("MCPD_LOG_LEVEL", "ERROR")
      on_exit(fn -> System.delete_env("MCPD_LOG_LEVEL") end)
      assert Config.new().log_level == :error
      assert Config.new(log_level: :trace).log_level == :trace

      {c, sink} = recorder()
      Mcpd.log(c, :warn, "below the floor")
      :ok = Mcpd.flush(c)
      assert Sink.Recorder.logs(sink) == []
    end

    test "a name outside the enum is ignored" do
      System.put_env("MCPD_LOG_LEVEL", "verbose")
      on_exit(fn -> System.delete_env("MCPD_LOG_LEVEL") end)
      assert Config.new().log_level == :debug
    end

    test "never filters the other four signals" do
      {c, sink} = recorder(log_level: :fatal)

      trace_id =
        Mcpd.span c, "run" do
          Mcpd.metric(c, "latency_ms", 3)
          Mcpd.traceparent() |> String.split("-") |> Enum.at(1)
        end

      Mcpd.score(c, trace_id, "judge", 1.0, min: 0.0, max: 1.0)
      Mcpd.feedback(c, trace_id, :thumbs, "up")
      :ok = Mcpd.flush(c)

      assert length(Sink.Recorder.spans(sink)) == 1
      assert length(Sink.Recorder.metrics(sink)) == 1
      assert length(Sink.Recorder.scores(sink)) == 1
      assert length(Sink.Recorder.feedback(sink)) == 1
      assert Sink.Recorder.logs(sink) == []
    end
  end

  describe "OpenTelemetry severity numbers (P090)" do
    test "every level carries the number the spec gives it" do
      assert Enum.map(Mcpd.LogLevel.values(), &Logs.severity_number/1) == [1, 5, 9, 13, 17, 21]
    end

    test "every number maps back by range" do
      ranges = [
        {1, 4, :trace},
        {5, 8, :debug},
        {9, 12, :info},
        {13, 16, :warn},
        {17, 20, :error},
        {21, 24, :fatal}
      ]

      for {low, high, level} <- ranges, n <- low..high do
        assert Logs.log_level_from_severity(n) == level, "severity #{n}"
      end
    end

    test "a number outside the range is clamped, not refused" do
      assert Logs.log_level_from_severity(0) == :trace
      assert Logs.log_level_from_severity(-3) == :trace
      assert Logs.log_level_from_severity(40) == :fatal
    end

    test "every level round-trips" do
      for level <- Mcpd.LogLevel.values() do
        assert level |> Logs.severity_number() |> Logs.log_level_from_severity() == level
      end
    end
  end
end
