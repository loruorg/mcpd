defmodule Mcpd.SurfaceTest do
  @moduledoc """
  Payload capture, sinks, stats and `record/3`: contract sections 2.1 to 2.3
  and 3, checked against the bytes a daemon receives or a sink is handed.
  """
  use ExUnit.Case, async: false

  import ExUnit.CaptureIO

  alias Mcpd.{Config, FakeDaemon, Ids, Sink}

  require Mcpd

  @accepted ~s({"accepted":1,"rejected":0,"duplicate":0})
  @epoch "1970-01-01T00:00:00.000000000Z"

  defp start_daemon(reply \\ fn _n -> {200, @accepted} end) do
    daemon = FakeDaemon.start(reply)
    on_exit(fn -> FakeDaemon.stop(daemon) end)
    daemon
  end

  defp client(opts) do
    {:ok, c} = Mcpd.start_link(Keyword.put_new(opts, :flush_interval, 60_000))
    on_exit(fn -> Mcpd.shutdown(c) end)
    c
  end

  defp spans(daemon), do: FakeDaemon.signals(daemon, "spans")
  defp closed(daemon), do: daemon |> spans() |> Enum.filter(&Map.has_key?(&1, "end_time"))

  defp finished_span(overrides \\ %{}) do
    Map.merge(
      %Mcpd.Span{
        trace_id: "0af7651916cd43dd8448eb211c80319c",
        span_id: "b7ad6b7169203331",
        parent_span_id: "00f067aa0ba902b7",
        external_parent_span_id: "53ce929d0e0e4736",
        name: "imported",
        kind: :tool_call,
        status: :ok,
        start_time: ~U[2026-09-01 10:00:00.000000Z],
        end_time: ~U[2026-09-01 10:00:01.500000Z],
        duration_ns: 1_500_000_000,
        is_root: false,
        is_event: false,
        correlation: %Mcpd.Correlation{project: "recorded", session_id: "s-9"},
        received_at: ~U[2026-09-01 10:00:02.000000Z],
        input: "the question",
        output: "the answer",
        usage: %Mcpd.TokenUsage{input_tokens: 12, output_tokens: 3},
        model: "m",
        provider: "p"
      },
      overrides
    )
  end

  # ------------------------------------------------------- 2.1 capture ---

  describe "payload capture" do
    test "capture_payloads: false keeps input and output in the process and everything else on the wire" do
      d = start_daemon()
      c = client(endpoint: d.endpoint, capture_payloads: false)

      Mcpd.span c, "plan",
        kind: :model_generation,
        input: "a secret prompt",
        output: "a secret answer",
        capture_output: true,
        model: "m",
        provider: "p",
        usage: %{"input_tokens" => 3},
        model_parameters: [temperature: 0.0, seed: 0, stop_sequences: ["STOP"]],
        session_id: "s-1" do
        "the block's value, also a secret"
      end

      :ok = Mcpd.flush(c)
      assert [span] = closed(d)
      refute Map.has_key?(span, "input")
      refute Map.has_key?(span, "output")
      assert span["name"] == "plan"
      assert span["kind"] == "model_generation"
      assert span["status"] == "ok"
      assert span["model"] == "m"
      assert span["provider"] == "p"
      assert span["usage"] == %{"input_tokens" => 3}
      # Sampling settings are metadata, not payload: no prompt is in them, and
      # they are what the run has to be reproduced from. Capture off must not
      # take them, and a zero seed must survive.
      assert span["model_parameters"] == %{
               "temperature" => 0.0,
               "seed" => 0,
               "stop_sequences" => ["STOP"],
               "has_response_format" => false
             }

      assert span["correlation"]["session_id"] == "s-1"
      assert is_integer(span["duration_ns"])
    end

    test "MCPD_CAPTURE_PAYLOADS reaches the client, code wins over it, and only off means off" do
      for off <- ~w(false 0 no off OFF False) do
        System.put_env("MCPD_CAPTURE_PAYLOADS", off)
        refute Config.new().capture_payloads, off
      end

      for on <- ~w(true 1 yes on anything) do
        System.put_env("MCPD_CAPTURE_PAYLOADS", on)
        assert Config.new().capture_payloads, on
      end

      System.put_env("MCPD_CAPTURE_PAYLOADS", "false")
      assert Config.new(capture_payloads: true).capture_payloads
      System.delete_env("MCPD_CAPTURE_PAYLOADS")
      assert Config.new().capture_payloads

      System.put_env("MCPD_CAPTURE_PAYLOADS", "false")
      d = start_daemon()
      c = client(endpoint: d.endpoint)
      Mcpd.span(c, "plan", [input: "kept home"], do: :ok)
      :ok = Mcpd.flush(c)
      assert [span] = closed(d)
      refute Map.has_key?(span, "input")
    after
      System.delete_env("MCPD_CAPTURE_PAYLOADS")
    end

    test "a span overrides the client in either direction, for itself only" do
      d = start_daemon()
      off = client(endpoint: d.endpoint, capture_payloads: false)

      Mcpd.span off, "shown", input: "public", capture_payloads: true do
        Mcpd.span(off, "child of shown", [input: "still private"], do: :ok)
      end

      on = client(endpoint: d.endpoint, capture_payloads: true)

      Mcpd.span on, "hidden", input: "private", capture_payloads: false do
        Mcpd.span(on, "child of hidden", [input: "public again"], do: :ok)
      end

      :ok = Mcpd.flush(off)
      :ok = Mcpd.flush(on)
      by_name = Map.new(closed(d), &{&1["name"], &1})
      assert by_name["shown"]["input"] == "public"
      refute Map.has_key?(by_name["child of shown"], "input")
      refute Map.has_key?(by_name["hidden"], "input")
      assert by_name["child of hidden"]["input"] == "public again"
    end
  end

  # ------------------------------------------- 3 model call parameters ---

  describe "model_parameters:" do
    test "the settings a model call was made with reach the wire" do
      d = start_daemon()
      c = client(endpoint: d.endpoint)

      Mcpd.span c, "answer",
        kind: :model_generation,
        model_parameters: [
          temperature: 0.2,
          top_p: 0.95,
          top_k: 40,
          max_tokens: 1024,
          seed: 7,
          stop_sequences: ["\n\nHuman:"],
          has_response_format: true
        ] do
        :ok
      end

      :ok = Mcpd.flush(c)
      assert [span] = closed(d)

      assert span["model_parameters"] == %{
               "temperature" => 0.2,
               "top_p" => 0.95,
               "top_k" => 40,
               "max_tokens" => 1024,
               "seed" => 7,
               "stop_sequences" => ["\n\nHuman:"],
               "has_response_format" => true
             }
    end

    test "a zero seed is a seed, and a field nobody set stays absent" do
      # The classic bug this guards: reading zero as "not given". A zero seed
      # is what a reproducible run is pinned to and a zero temperature is the
      # setting most worth recording, so neither may be dropped on the way out.
      d = start_daemon()
      c = client(endpoint: d.endpoint)

      Mcpd.span(c, "greedy", [model_parameters: [temperature: 0.0, top_p: 0.0, seed: 0]], do: :ok)
      :ok = Mcpd.flush(c)
      assert [span] = closed(d)

      assert span["model_parameters"] == %{
               "temperature" => 0.0,
               "top_p" => 0.0,
               "seed" => 0,
               "has_response_format" => false
             }
    end

    test "a %Mcpd.ModelParameters{} is accepted, and a bad option costs a field not a raise" do
      d = start_daemon()
      c = client(endpoint: d.endpoint)

      Mcpd.span(c, "from a struct",
        [model_parameters: %Mcpd.ModelParameters{seed: 0, max_tokens: 8}],
        do: :ok
      )

      assert Mcpd.span(c, "nonsense", [model_parameters: "not a list"], do: :fine) == :fine
      :ok = Mcpd.flush(c)
      by_name = Map.new(closed(d), &{&1["name"], &1})

      assert by_name["from a struct"]["model_parameters"] == %{
               "seed" => 0,
               "max_tokens" => 8,
               "has_response_format" => false
             }

      refute Map.has_key?(by_name["nonsense"], "model_parameters")
    end
  end

  # ----------------------------------------------------------- 3 record ---

  describe "record/3" do
    test "a finished span is queued as given, with the client's correlation underneath" do
      d = start_daemon()
      c = client(endpoint: d.endpoint, project: "recorded", environment: "staging", service_name: "importer")

      assert :ok = Mcpd.record(c, finished_span())
      :ok = Mcpd.flush(c)
      assert [wire] = spans(d)

      assert wire["trace_id"] == "0af7651916cd43dd8448eb211c80319c"
      assert wire["span_id"] == "b7ad6b7169203331"
      assert wire["parent_span_id"] == "00f067aa0ba902b7"
      assert wire["external_parent_span_id"] == "53ce929d0e0e4736"
      assert wire["name"] == "imported"
      assert wire["kind"] == "tool_call"
      assert wire["status"] == "ok"
      assert wire["start_time"] == "2026-09-01T10:00:00.000000Z"
      assert wire["end_time"] == "2026-09-01T10:00:01.500000Z"
      assert wire["duration_ns"] == 1_500_000_000
      assert wire["is_root"] == false
      assert wire["input"] == "the question"
      assert wire["output"] == "the answer"
      assert wire["usage"] == %{"input_tokens" => 12, "output_tokens" => 3}
      assert wire["model"] == "m"

      assert wire["correlation"] == %{
               "project" => "recorded",
               "environment" => "staging",
               "service_name" => "importer",
               "session_id" => "s-9"
             }

      assert %{exported: 1, rejected: 0, failed: 0, queued: 0} = Mcpd.stats(c)
    end

    test "a map in the wire shape works too, and the fields the daemon requires are filled" do
      d = start_daemon()
      c = client(endpoint: d.endpoint, project: "recorded")
      t = Ids.trace_id()
      s = Ids.span_id()

      assert :ok = Mcpd.record(c, %{"trace_id" => t, "span_id" => s, "name" => "bare", "start_time" => "2026-09-01T10:00:00Z"})
      :ok = Mcpd.flush(c)
      assert [wire] = spans(d)
      assert wire["kind"] == "generic"
      assert wire["status"] == "unset"
      assert wire["is_root"] == true
      assert wire["is_event"] == false
      assert wire["received_at"] == @epoch
      assert wire["correlation"] == %{"project" => "recorded"}
    end

    test "each malformed shape is refused, counted and never raised" do
      d = start_daemon()
      c = client(endpoint: d.endpoint)

      malformed = [
        finished_span(%{trace_id: "not-hex"}),
        finished_span(%{trace_id: String.duplicate("0", 32)}),
        finished_span(%{span_id: "b7ad6b716920333"}),
        finished_span(%{span_id: String.duplicate("0", 16)}),
        finished_span(%{parent_span_id: "XYZ"}),
        finished_span(%{name: ""}),
        finished_span(%{start_time: nil}),
        :not_a_span
      ]

      stderr =
        capture_io(:stderr, fn ->
          for bad <- malformed, do: assert(:ok = Mcpd.record(c, bad))
          :ok = Mcpd.flush(c)
        end)

      assert stderr =~ "a recorded span was refused and dropped"
      assert spans(d) == []
      assert %{rejected: 8, exported: 0, failed: 0} = Mcpd.stats(c)
    end

    test "a disabled client drops a record, and capture off strips it unless the call says otherwise" do
      disabled = client(endpoint: "http://127.0.0.1:1", enabled: false)
      assert :ok = Mcpd.record(disabled, finished_span())
      assert %{queued: 0, rejected: 0} = Mcpd.stats(disabled)

      d = start_daemon()
      c = client(endpoint: d.endpoint, capture_payloads: false)
      :ok = Mcpd.record(c, finished_span())
      :ok = Mcpd.record(c, finished_span(%{span_id: "aaaaaaaaaaaaaaaa"}), capture_payloads: true)
      :ok = Mcpd.flush(c)
      by_id = Map.new(spans(d), &{&1["span_id"], &1})
      refute Map.has_key?(by_id["b7ad6b7169203331"], "input")
      refute Map.has_key?(by_id["b7ad6b7169203331"], "output")
      assert by_id["aaaaaaaaaaaaaaaa"]["input"] == "the question"
    end

    test "a record is never re-sampled" do
      d = start_daemon()
      c = client(endpoint: d.endpoint, sample: 0.0)
      Mcpd.span(c, "sampled out", do: :ok)
      :ok = Mcpd.record(c, finished_span())
      :ok = Mcpd.flush(c)
      assert [%{"name" => "imported"}] = spans(d)
    end
  end

  test "external_parent_span_id reaches the wire on a span the client opened" do
    d = start_daemon()
    c = client(endpoint: d.endpoint)
    Mcpd.span(c, "joined", [external_parent_span_id: "1234567890abcdef"], do: :ok)
    :ok = Mcpd.flush(c)
    assert [span] = closed(d)
    assert span["external_parent_span_id"] == "1234567890abcdef"
    assert span["is_root"] == true, "an external parent never parents inside mcpd's own tree"
    refute Map.has_key?(span, "parent_span_id")
  end

  # ------------------------------------------------------------ 2.2 sinks ---

  describe "sinks" do
    test "a recorder sees exactly the signals recorded, and forgets them on clear" do
      {:ok, recorder} = Sink.Recorder.start_link()
      c = client(endpoint: "http://127.0.0.1:1", sink: {Sink.Recorder, recorder})

      trace_id =
        Mcpd.span c, "agent", kind: :agent_run do
          Mcpd.log(c, :info, "hello")
          Mcpd.metric(c, "answers", 1, kind: :counter)
          Mcpd.traceparent() |> String.split("-") |> Enum.at(1)
        end

      Mcpd.score(c, trace_id, "relevance", 0.5, min: 0.0, max: 1.0)
      Mcpd.feedback(c, trace_id, :thumbs, "up")
      :ok = Mcpd.flush(c)

      assert [%{"name" => "agent", "trace_id" => ^trace_id, "status" => "ok"}] = Sink.Recorder.spans(recorder)
      assert [%{"message" => "hello", "trace_id" => ^trace_id}] = Sink.Recorder.logs(recorder)
      assert [%{"name" => "answers", "value" => 1.0}] = Sink.Recorder.metrics(recorder)
      assert [%{"scorer_id" => "relevance", "source" => "live"}] = Sink.Recorder.scores(recorder)
      assert [%{"kind" => "thumbs", "value" => "up"}] = Sink.Recorder.feedback(recorder)
      assert Sink.Recorder.batches(recorder) == 1
      assert %{exported: 5, failed: 0, queued: 0} = Mcpd.stats(c)

      :ok = Sink.Recorder.clear(recorder)
      assert Sink.Recorder.spans(recorder) == []
      assert Sink.Recorder.batches(recorder) == 0
    end

    test "the console sink writes one JSON line per signal, to stderr unless told where" do
      {:ok, lines} = Agent.start_link(fn -> [] end)
      write = fn line -> Agent.update(lines, &(&1 ++ [line])) end
      c = client(endpoint: "http://127.0.0.1:1", sink: {Sink.Console, write: write})

      Mcpd.span c, "agent", do: Mcpd.log(c, :warn, "careful")
      :ok = Mcpd.flush(c)

      decoded = lines |> Agent.get(& &1) |> Enum.map(&:json.decode/1)
      assert Enum.map(decoded, & &1["signal"]) |> Enum.sort() == ["log", "span"]
      assert Enum.all?(Agent.get(lines, & &1), &String.ends_with?(&1, "\n"))
      assert Enum.find(decoded, &(&1["signal"] == "span"))["name"] == "agent"
      assert Enum.find(decoded, &(&1["signal"] == "log"))["message"] == "careful"

      plain = client(endpoint: "http://127.0.0.1:1", sink: Sink.Console)
      Mcpd.span(plain, "on stderr", do: :ok)
      stderr = capture_io(:stderr, fn -> :ok = Mcpd.flush(plain) end)
      assert stderr =~ ~s("signal":"span")
      assert stderr =~ ~s("name":"on stderr")
    end

    test "a sink that raises or refuses is a failed batch the caller never sees" do
      defmodule Boom do
        @behaviour Mcpd.Sink
        def export(_batch, _key, :raise), do: raise("the sink is broken")
        def export(_batch, _key, :refuse), do: {:error, :refused}
      end

      raising = client(endpoint: "http://127.0.0.1:1", sink: {Boom, :raise})
      Mcpd.span(raising, "lost", do: :ok)
      stderr = capture_io(:stderr, fn -> assert :ok = Mcpd.flush(raising) end)
      assert stderr =~ "could not deliver a batch"
      assert stderr =~ "the sink is broken"
      assert %{failed: 1, exported: 0, queued: 0} = Mcpd.stats(raising)

      refusing = client(endpoint: "http://127.0.0.1:1", sink: {Boom, :refuse})
      Mcpd.span(refusing, "lost too", do: :ok)
      capture_io(:stderr, fn -> assert :ok = Mcpd.flush(refusing) end)
      assert %{failed: 1, exported: 0} = Mcpd.stats(refusing)
    end

    test "a bare module is the module with no options, and the HTTP sink is the default" do
      assert Config.new(sink: Sink.Console).sink == {Sink.Console, []}
      assert Config.new().sink == {Sink.Http, []}
      assert Config.new(sink: {Sink.Console, write: &IO.puts/1}).sink |> elem(0) == Sink.Console
    end
  end

  # ------------------------------------------------------------ 2.3 stats ---

  test "stats carry the six names, counted as things happen, and zeros once stopped" do
    {:ok, plan} = Agent.start_link(fn -> :ok end)

    d =
      start_daemon(fn _n ->
        case Agent.get(plan, & &1) do
          :ok -> {200, @accepted}
          :fail -> {500, "no"}
        end
      end)

    c = client(endpoint: d.endpoint, max_queue: 2, batch_size: 10)
    assert Mcpd.stats(c) == %{queued: 0, dropped: 0, capacity: 2, exported: 0, failed: 0, rejected: 0}

    capture_io(:stderr, fn ->
      for n <- 1..3, do: Mcpd.log(c, :info, "line #{n}")
      assert %{queued: 2, dropped: 1, capacity: 2} = Mcpd.stats(c)
      :ok = Mcpd.flush(c)
      assert %{queued: 0, exported: 2, failed: 0} = Mcpd.stats(c)

      Agent.update(plan, fn _ -> :fail end)
      Mcpd.log(c, :info, "doomed")
      :ok = Mcpd.flush(c)
      assert %{exported: 2, failed: 1} = Mcpd.stats(c)

      :ok = Mcpd.record(c, :not_a_span)
      assert %{rejected: 1, dropped: 1} = Mcpd.stats(c)
    end)

    :ok = Mcpd.shutdown(c)
    assert Mcpd.stats(c) == %{queued: 0, dropped: 0, capacity: 0, exported: 0, failed: 0, rejected: 0}
  end

  # -------------------------------------------- 2.4 deadlines, and B3 bounds ---

  defmodule Slow do
    @moduledoc false
    @behaviour Mcpd.Sink
    # A daemon that accepts the connection and answers late, or never.
    def export(_batch, _key, {calls, delay}) do
      :counters.add(calls, 1, 1)
      Process.sleep(delay)
      :ok
    end
  end

  describe "flush and shutdown are bounded" do
    test "both return by their deadline when the sink never answers" do
      # Against a black-holed daemon every batch costs three attempts of the
      # full request timeout. A bound on batches is no bound on time.
      calls = :counters.new(1, [])
      {:ok, c} = Mcpd.start_link(sink: {Slow, {calls, 400}}, batch_size: 1, flush_interval: 60_000)
      for n <- 1..5, do: Mcpd.log(c, :info, "line #{n}")

      started = System.monotonic_time(:millisecond)
      assert Mcpd.flush(c, 50) == {:error, :timeout}, "a flush that ran out of time says so"
      assert Mcpd.shutdown(c, 100) == :ok
      took = System.monotonic_time(:millisecond) - started
      assert took < 350, "one deadline covers flushing and stopping together, took #{took}ms"

      # The batch in flight finishes on its own; no further batch starts.
      Process.sleep(1_000)
      assert :counters.get(calls, 1) == 1
      refute Process.alive?(c)
    end

    test "a flush that gave up leaves the worker's list of waiters" do
      calls = :counters.new(1, [])
      c = client(sink: {Slow, {calls, 60_000}}, batch_size: 1)
      Mcpd.log(c, :info, "stuck")

      for _ <- 1..50, do: assert(Mcpd.flush(c, 10) == {:error, :timeout})
      Process.sleep(50)
      assert :sys.get_state(c).waiters == [], "each waiter leaves at its own deadline"
      Mcpd.shutdown(c, 10)
    end

    test "the worker's mailbox is bounded too, and what it sheds is counted" do
      c = client(endpoint: "http://127.0.0.1:1", max_queue: 100, batch_size: 1_000)
      :sys.suspend(c)
      for n <- 1..1_000, do: Mcpd.log(c, :info, "line #{n}")
      {:message_queue_len, backlog} = Process.info(c, :message_queue_len)
      assert backlog <= 100, "the mailbox grew to #{backlog} behind a worker that was not reading it"
      :sys.resume(c)

      capture_io(:stderr, fn ->
        assert %{dropped: dropped, queued: 100} = Mcpd.stats(c)
        assert dropped == 900
      end)

      Mcpd.shutdown(c, 10)
    end
  end
end
