defmodule Mcpd.BehaviourTest do
  @moduledoc """
  The promises that hold whatever the daemon, the clock or the caller does.

  `mcpd_test.exs` checks the pieces in isolation: the id generator, the
  traceparent parser, the queue struct. This file checks the client, and checks
  it against a daemon that answers, so the assertions read the bytes that were
  actually sent rather than the SDK's own idea of what it sent.
  """
  use ExUnit.Case, async: false

  import ExUnit.CaptureIO

  alias Mcpd.{Context, FakeDaemon, Ids, Worker}

  @accepted ~s({"accepted":1,"rejected":0,"duplicate":0})

  # Trace ids whose low 64 bits sit at either end of the range, so a half
  # sample keeps the first and drops the second whatever the jitter.
  @kept_id "0af7651916cd43dd0000000000000001"
  @dropped_id "0af7651916cd43ddffffffffffffffff"

  # ------------------------------------------------------- B3, the bound ---

  test "the worker applies the bound, and counts what it dropped" do
    # `Queue` is tested directly already. What is not tested is that the worker
    # ever passes `max_queue` to it, or that the count is readable, which is the
    # only way an operator learns signals are being lost.
    {:ok, c} = client(endpoint: "http://127.0.0.1:1", max_queue: 4, batch_size: 10_000)

    for i <- 1..10, do: Mcpd.log(c, :info, "line #{i}")

    assert Worker.dropped(c) == 6
    Mcpd.shutdown(c)
  end

  test "the oldest signal is the one dropped, as seen by the daemon" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint, max_queue: 3, batch_size: 10_000)

    for i <- 1..5, do: Mcpd.log(c, :info, "line #{i}")
    Mcpd.flush(c)

    # The newest survive: when the daemon comes back, what is happening now is
    # what somebody will want to read.
    assert Enum.map(FakeDaemon.signals(daemon, "logs"), & &1["message"]) ==
             ["line 3", "line 4", "line 5"]

    Mcpd.shutdown(c)
  end

  test "the overflow warning goes to stderr once per interval, and never to stdout" do
    {:ok, c} = client(endpoint: "http://127.0.0.1:1", max_queue: 2, batch_size: 10_000)

    stdout =
      capture_io(fn ->
        stderr =
          capture_io(:stderr, fn ->
            for i <- 1..50, do: Mcpd.log(c, :info, "line #{i}")
            # A cast is asynchronous; a call behind it proves the casts landed.
            assert Worker.dropped(c) == 48
          end)

        lines = stderr |> String.split("\n", trim: true) |> Enum.filter(&String.starts_with?(&1, "mcpd:"))
        assert length(lines) == 1, "48 drops in one interval produced #{length(lines)} warnings"
        assert hd(lines) =~ "queue is full"
      end)

    assert stdout == "", "a diagnostic reached the agent's stdout: #{inspect(stdout)}"
    Mcpd.shutdown(c)
  end

  # --------------------------------------------- B1, the daemon misbehaves ---

  test "a 500 leaves the caller with its answer and an empty queue" do
    daemon = start_daemon(fn _n -> {500, ~s({"error":"internal"})} end)
    {:ok, c} = client(endpoint: daemon.endpoint)

    assert Mcpd.with_span(c, "run", [], fn -> :the_answer end) == :the_answer
    assert Mcpd.flush(c) == :ok
    # One request: 500 is not one of the four statuses worth retrying. The
    # batch is dropped rather than held.
    assert length(FakeDaemon.requests(daemon)) == 1
    assert :sys.get_state(c).queue.size == 0

    Mcpd.shutdown(c)
  end

  test "a body that is not JSON is a bad answer, not a reason to raise" do
    daemon = start_daemon(fn _n -> {200, "<html>fronted by a proxy</html>"} end)
    {:ok, c} = client(endpoint: daemon.endpoint)

    assert Mcpd.with_span(c, "run", [], fn -> :ok end) == :ok
    assert Mcpd.flush(c) == :ok
    assert length(FakeDaemon.requests(daemon)) == 1

    Mcpd.shutdown(c)
  end

  test "a daemon that accepts the connection and never answers costs only the timeouts" do
    daemon = start_daemon(fn _n -> :never end)
    {:ok, c} = client(endpoint: daemon.endpoint, timeout: 300)

    assert Mcpd.with_span(c, "run", [], fn -> :ok end) == :ok

    started = System.monotonic_time(:millisecond)
    assert Mcpd.flush(c) == :ok
    elapsed = System.monotonic_time(:millisecond) - started

    assert elapsed < 5_000, "flush waited #{elapsed}ms on a daemon that will never answer"
    # A timeout is a connection failure, and a connection failure is retried.
    assert length(FakeDaemon.requests(daemon)) == 3

    Mcpd.shutdown(c)
  end

  test "the export failure is reported on stderr, throttled, and never on stdout" do
    daemon = start_daemon(fn _n -> {500, ~s({"error":"internal"})} end)
    {:ok, c} = client(endpoint: daemon.endpoint, batch_size: 1)

    stdout =
      capture_io(fn ->
        stderr =
          capture_io(:stderr, fn ->
            Mcpd.log(c, :info, "one")
            Mcpd.log(c, :info, "two")
            Mcpd.flush(c)
          end)

        lines = stderr |> String.split("\n", trim: true) |> Enum.filter(&String.starts_with?(&1, "mcpd:"))
        assert length(lines) == 1, "two failed batches in one interval produced #{length(lines)} warnings"
        assert hd(lines) =~ "could not deliver a batch"
      end)

    assert stdout == "", "a diagnostic reached the agent's stdout: #{inspect(stdout)}"
    assert length(FakeDaemon.requests(daemon)) == 2
    Mcpd.shutdown(c)
  end

  test "recording after shutdown is ignored rather than fatal" do
    {:ok, c} = client(endpoint: "http://127.0.0.1:1")
    Mcpd.shutdown(c)

    assert Mcpd.log(c, :info, "too late") == :ok
    assert Mcpd.metric(c, "m", 1) == :ok
    assert Mcpd.score(c, Ids.trace_id(), "s", 1, min: 0, max: 1) == :ok
    assert Mcpd.feedback(c, Ids.trace_id(), :thumbs, "up") == :ok
    assert Mcpd.with_span(c, "too late", [], fn -> :the_answer end) == :the_answer
    assert Mcpd.flush(c) == :ok
    assert Mcpd.config(c) == nil
  end

  test "an option the SDK cannot use costs the span, never the block" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint)

    # A kind that cannot be rendered as a string. The block still runs and its
    # value still comes back, which is the whole of contract behaviour B1.
    assert Mcpd.with_span(c, "run", [kind: %{not: "a kind"}], fn -> :the_answer end) == :the_answer
    assert Mcpd.metric(c, "m", "not a number") == :ok
    Mcpd.flush(c)

    Mcpd.shutdown(c)
  end

  # ----------------------------------------------------------- B7, on the wire ---

  test "traceparent names the active span, and is nil outside one" do
    {:ok, c} = client(endpoint: "http://127.0.0.1:1")

    assert Mcpd.traceparent() == nil

    Mcpd.with_span(c, "outbound", [], fn ->
      active = Context.current()
      header = Mcpd.traceparent()

      assert {:ok, %{trace_id: trace, span_id: span, sampled: true}} = Context.parse_traceparent(header)
      assert trace == active.trace_id
      assert span == active.span_id
      # The sampled flag is set: a span that exists was sampled.
      assert String.ends_with?(header, "-01")
    end)

    assert Mcpd.traceparent() == nil
    Mcpd.shutdown(c)
  end

  test "continue attaches the local span to the caller's, and says so on the wire" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint)

    trace = Ids.trace_id()
    caller = Ids.span_id()

    assert Mcpd.continue(Context.traceparent(trace, caller), fn ->
             Mcpd.with_span(c, "handler", [], fn -> :handled end)
           end) == :handled

    Mcpd.flush(c)

    span = Enum.find(FakeDaemon.signals(daemon, "spans"), &(&1["name"] == "handler"))
    assert span["trace_id"] == trace, "the trace did not survive the service boundary"
    assert span["parent_span_id"] == caller
    assert span["is_root"] == false

    Mcpd.shutdown(c)
  end

  test "a malformed traceparent starts a fresh trace instead of failing the request" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint)

    for header <- [nil, "", "garbage", "00-00000000000000000000000000000000-00f067aa0ba902b7-01"] do
      assert Mcpd.continue(header, fn ->
               Mcpd.with_span(c, "handler", [], fn -> :handled end)
             end) == :handled
    end

    Mcpd.flush(c)
    spans = FakeDaemon.signals(daemon, "spans")

    assert length(spans) == 4
    assert Enum.all?(spans, &(&1["is_root"] == true)), "a broken header left a parent behind"
    assert Enum.all?(spans, &Ids.valid?(&1["trace_id"], 16))
    assert length(Enum.uniq(Enum.map(spans, & &1["trace_id"]))) == 4

    Mcpd.shutdown(c)
  end

  # --------------------------------------------------------- B4, sampling ---

  test "sample: 0.0 records nothing and the block still returns" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint, sample: 0.0)

    assert Mcpd.with_span(c, "run", [], fn ->
             Mcpd.log(c, :info, "inside a dropped run")
             Mcpd.with_span(c, "step", [], fn -> :ok end)
             assert Ids.valid?(Context.current().trace_id, 16)
             assert String.ends_with?(Mcpd.traceparent(), "-00")
             :the_answer
           end) == :the_answer

    Mcpd.flush(c)
    assert FakeDaemon.requests(daemon) == []

    Mcpd.shutdown(c)
  end

  test "the decision is made from the trace id, once, and inherited by every step" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint, sample: 0.5, batch_size: 10_000)

    Mcpd.with_span(c, "kept", [trace_id: @kept_id], fn ->
      Mcpd.with_span(c, "kept-step", [], fn ->
        Mcpd.with_span(c, "kept-grandchild", [], fn -> :ok end)
      end)

      Mcpd.log(c, :info, "kept-log")
    end)

    Mcpd.with_span(c, "dropped", [trace_id: @dropped_id], fn ->
      Mcpd.with_span(c, "dropped-step", [], fn ->
        Mcpd.with_span(c, "dropped-grandchild", [], fn -> :ok end)
      end)

      Mcpd.log(c, :info, "dropped-log")
    end)

    Mcpd.flush(c)

    names = daemon |> FakeDaemon.signals("spans") |> Enum.map(& &1["name"]) |> Enum.sort()
    assert names == ["kept", "kept-grandchild", "kept-step"]
    assert Enum.map(FakeDaemon.signals(daemon, "logs"), & &1["message"]) == ["kept-log"]

    Mcpd.shutdown(c)
  end

  test "an upstream decision in the traceparent overrides the local rate" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint, sample: 0.0)

    # Sampled upstream, so recorded here despite a local rate of zero.
    Mcpd.continue(Context.traceparent(Ids.trace_id(), Ids.span_id(), true), fn ->
      Mcpd.with_span(c, "kept-upstream", [], fn -> :ok end)
    end)

    Mcpd.shutdown(c)
    {:ok, c} = client(endpoint: daemon.endpoint, sample: 1.0)

    # Dropped upstream, so dropped here despite a local rate of one.
    Mcpd.continue(Context.traceparent(Ids.trace_id(), Ids.span_id(), false), fn ->
      Mcpd.with_span(c, "dropped-upstream", [], fn -> :ok end)
    end)

    Mcpd.flush(c)

    assert Enum.map(FakeDaemon.signals(daemon, "spans"), & &1["name"]) == ["kept-upstream"]

    Mcpd.shutdown(c)
  end

  # ------------------------------------------------- enabled: false ---

  test "a disabled client sends nothing and still returns valid objects" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint, enabled: false)

    trace =
      Mcpd.with_span(c, "invisible", [], fn ->
        active = Context.current()
        assert Ids.valid?(active.trace_id, 16)
        assert Ids.valid?(active.span_id, 8)
        assert {:ok, _} = Context.parse_traceparent(Mcpd.traceparent())
        Mcpd.log(c, :info, "nothing to see")
        Mcpd.metric(c, "m", 1)
        Mcpd.with_span(c, "child", [], fn -> :ok end)
        active.trace_id
      end)

    assert Mcpd.score(c, trace, "s", 1, min: 0, max: 1) == :ok
    assert Mcpd.feedback(c, trace, :thumbs, "up") == :ok
    assert Mcpd.flush(c) == :ok

    assert FakeDaemon.requests(daemon) == [], "a disabled client must not talk to the network"
    assert Worker.dropped(c) == 0, "nothing was queued, so nothing was dropped"

    Mcpd.shutdown(c)
  end

  test "MCPD_ENABLED=false reaches the client, and code wins over it" do
    daemon = start_daemon()
    System.put_env("MCPD_ENABLED", "false")

    try do
      {:ok, off} = client(endpoint: daemon.endpoint)
      {:ok, on} = client(endpoint: daemon.endpoint, enabled: true)

      Mcpd.with_span(off, "off", [], fn -> :ok end)
      Mcpd.with_span(on, "on", [], fn -> :ok end)
      Mcpd.flush(off)
      Mcpd.flush(on)

      assert Enum.map(FakeDaemon.signals(daemon, "spans"), & &1["name"]) == ["on"]
      Mcpd.shutdown(off)
      Mcpd.shutdown(on)
    after
      System.delete_env("MCPD_ENABLED")
    end
  end

  # --------------------------------------------------------- concurrency ---

  test "twenty processes recording at once produce twenty correct trees" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint, batch_size: 10_000)

    1..20
    |> Enum.map(fn i ->
      Task.async(fn ->
        Mcpd.with_span(c, "run-#{i}", [], fn ->
          Mcpd.with_span(c, "step-#{i}-a", [], fn -> :ok end)
          Mcpd.with_span(c, "step-#{i}-b", [], fn -> :ok end)
        end)
      end)
    end)
    |> Task.await_many(10_000)

    Mcpd.flush(c)
    spans = FakeDaemon.signals(daemon, "spans")

    assert length(spans) == 60
    roots = Enum.filter(spans, & &1["is_root"])
    assert length(roots) == 20
    assert length(Enum.uniq(Enum.map(roots, & &1["trace_id"]))) == 20

    for root <- roots do
      children = Enum.filter(spans, &(&1["parent_span_id"] == root["span_id"]))
      assert length(children) == 2, "#{root["name"]} lost a child"
      assert Enum.all?(children, &(&1["trace_id"] == root["trace_id"]))
    end

    Mcpd.shutdown(c)
  end

  test "a task started inside a span records a separate trace, because the span is the process's" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint, batch_size: 10_000)

    Mcpd.with_span(c, "root", [], fn ->
      Task.async(fn -> Mcpd.with_span(c, "detached", [], fn -> :ok end) end)
      |> Task.await(5_000)
    end)

    Mcpd.flush(c)
    spans = FakeDaemon.signals(daemon, "spans")
    root = Enum.find(spans, &(&1["name"] == "root"))
    detached = Enum.find(spans, &(&1["name"] == "detached"))

    # The active span lives in the process dictionary, so a task does not
    # inherit it. This is a consequence users have to know about: work handed to
    # another process is a separate trace unless the traceparent goes with it.
    assert detached["is_root"] == true
    assert detached["parent_span_id"] == nil
    assert detached["trace_id"] != root["trace_id"]

    Mcpd.shutdown(c)
  end

  # ------------------------------------------------------ B2, never waits ---

  test "recording never waits on a daemon that is not answering" do
    daemon = start_daemon(fn _n -> :never end)
    {:ok, c} = client(endpoint: daemon.endpoint, timeout: 2_000, batch_size: 1)

    # The first signal sends a batch, and the sender is now stuck for the
    # timeout. Everything after it has to return at once regardless.
    Mcpd.log(c, :info, "the one that gets stuck")
    started = System.monotonic_time(:millisecond)

    for i <- 1..200, do: Mcpd.log(c, :info, "line #{i}")
    assert Mcpd.with_span(c, "run", [], fn -> :the_answer end) == :the_answer

    elapsed = System.monotonic_time(:millisecond) - started
    assert elapsed < 500, "recording 201 signals took #{elapsed}ms while the daemon held the socket"

    # A flush is the one call that waits, and it waits no longer than asked.
    # It says it ran out of time rather than answering `:ok` for a queue it
    # never sent.
    started = System.monotonic_time(:millisecond)
    assert Mcpd.flush(c, 100) == {:error, :timeout}
    assert System.monotonic_time(:millisecond) - started < 1_000

    Mcpd.shutdown(c, 500)
  end

  # ----------------------------------------------------- B5, the lifecycle ---

  test "shutdown sends what was queued without an explicit flush" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint, batch_size: 10_000)

    Mcpd.with_span(c, "last", [], fn -> :ok end)
    assert Mcpd.shutdown(c) == :ok

    assert Enum.map(FakeDaemon.signals(daemon, "spans"), & &1["name"]) == ["last"]
    # The second call has nothing to stop and must still be quiet.
    assert Mcpd.shutdown(c) == :ok
    assert length(FakeDaemon.requests(daemon)) == 1
  end

  test "a flush with nothing to send makes no request" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint)

    assert Mcpd.flush(c) == :ok
    assert FakeDaemon.requests(daemon) == [], "an empty batch costs the daemon a request for nothing"

    Mcpd.shutdown(c)
  end

  test "a full batch goes out on its own, without waiting for the interval" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint, batch_size: 4)

    for _ <- 1..4, do: Mcpd.log(c, :info, "tick")

    assert wait_until(fn -> length(FakeDaemon.requests(daemon)) == 1 end),
           "a full batch should have been sent without a flush"

    Mcpd.shutdown(c)
  end

  # -------------------------------------------- B8, an open span is sent ---

  test "a span open when flush runs is on the wire without an end time" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint)

    Mcpd.with_span(c, "long_running", [input: "the question"], fn ->
      Mcpd.flush(c)
      :ok
    end)

    [open] = FakeDaemon.signals(daemon, "spans")
    assert open["name"] == "long_running"
    assert open["status"] == "unset"
    refute Map.has_key?(open, "end_time"), "an open span must not carry an end time"
    refute Map.has_key?(open, "duration_ns")
    assert open["input"] == "the question", "fields known at the start go out with the start"

    Mcpd.flush(c)
    [^open, closed] = FakeDaemon.signals(daemon, "spans")
    assert closed["span_id"] == open["span_id"], "the update must address the same span"
    assert closed["trace_id"] == open["trace_id"]
    assert closed["status"] == "ok"
    assert is_binary(closed["end_time"])
    assert closed["duration_ns"] >= 0

    Mcpd.shutdown(c)
  end

  test "a span that opens and closes between flushes costs one record" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint)

    Mcpd.with_span(c, "quick", [], fn -> :ok end)
    Mcpd.flush(c)

    assert [span] = FakeDaemon.signals(daemon, "spans")
    assert span["status"] == "ok"
    assert is_binary(span["end_time"])

    Mcpd.shutdown(c)
  end

  # --------------------------------------------- the batch, as it is sent ---

  test "one batch carries every signal kind under its own key" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint, batch_size: 10_000)

    trace =
      Mcpd.with_span(c, "answer", [], fn ->
        Mcpd.log(c, :info, "looked up the order", %{"order_id" => 41})
        Mcpd.metric(c, "orders.looked_up", 1, kind: :counter, unit: "{order}")
        Context.current().trace_id
      end)

    Mcpd.score(c, trace, "answer_relevance", 0.82, min: 0, max: 1)
    Mcpd.feedback(c, trace, :thumbs, "up", comment: "helpful")
    Mcpd.flush(c)

    assert [batch] = FakeDaemon.batches(daemon)
    assert length(batch["spans"]) == 1
    assert length(batch["logs"]) == 1
    assert length(batch["metrics"]) == 1
    assert length(batch["scores"]) == 1
    assert length(batch["feedback"]) == 1

    log = hd(batch["logs"])
    assert log["trace_id"] == trace, "a log with no trace is a log nobody can place"
    assert log["level"] == "info"

    score = hd(batch["scores"])
    # A number with no declared scale cannot be compared to anything.
    assert score["min"] == 0.0
    assert score["max"] == 1.0
    assert score["value"] == 0.82

    Mcpd.shutdown(c)
  end

  test "an error span carries the status, the class and a message" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint)

    assert_raise ArgumentError, "the tool refused", fn ->
      Mcpd.with_span(c, "call_tool", [], fn -> raise ArgumentError, "the tool refused" end)
    end

    Mcpd.flush(c)
    span = hd(FakeDaemon.signals(daemon, "spans"))

    assert span["status"] == "error"
    assert span["error"]["message"] == "the tool refused"
    # `kind` is the field the spec names. The class used to go out as `type`,
    # which the daemon discarded in silence, so every Elixir error arrived
    # with no class at all.
    assert span["error"]["kind"] == "ArgumentError"
    refute Map.has_key?(span["error"], "type")
    assert is_binary(span["error"]["stack"])

    Mcpd.shutdown(c)
  end

  defmodule Wordless do
    defexception message: ""
  end

  test "an error with a blank message still names its class" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint)

    assert_raise Wordless, fn ->
      Mcpd.with_span(c, "call_tool", [], fn -> raise Wordless end)
    end

    Mcpd.flush(c)
    span = hd(FakeDaemon.signals(daemon, "spans"))

    assert span["status"] == "error"
    # "Something failed" is not actionable, so a blank message is refused and
    # the class stands in for it.
    assert span["error"]["message"] == "Mcpd.BehaviourTest.Wordless"
    assert span["error"]["kind"] == "Mcpd.BehaviourTest.Wordless"

    Mcpd.shutdown(c)
  end

  test "a throw and an exit inside a span are recorded as errors and re-thrown" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint, batch_size: 10_000)

    assert catch_throw(Mcpd.with_span(c, "thrower", [], fn -> throw(:bail) end)) == :bail
    assert catch_exit(Mcpd.with_span(c, "exiter", [], fn -> exit(:gone) end)) == :gone

    Mcpd.flush(c)
    spans = Map.new(FakeDaemon.signals(daemon, "spans"), &{&1["name"], &1})

    assert spans["thrower"]["status"] == "error"
    assert spans["thrower"]["error"]["kind"] == "throw"
    assert spans["exiter"]["status"] == "error"
    assert spans["exiter"]["error"]["kind"] == "exit"
    assert spans["exiter"]["error"]["message"] =~ "gone"

    Mcpd.shutdown(c)
  end

  # ------------------------------------------- section 6, the idempotency key ---

  test "every batch carries an ingest_key, and two batches never share one" do
    daemon = start_daemon()
    # One signal per batch, so the two keys compared below come from two bodies.
    {:ok, c} = client(endpoint: daemon.endpoint, batch_size: 1)

    Mcpd.log(c, :info, "one")
    Mcpd.log(c, :info, "two")
    Mcpd.flush(c)

    keys = Enum.map(FakeDaemon.batches(daemon), & &1["ingest_key"])

    assert length(keys) == 2
    assert Enum.all?(keys, &Ids.valid?(&1, 16))
    # Distinct between batches. One key shared by two different bodies is worse
    # than no key at all: the daemon refuses the second as a replay of the
    # first, and those signals are lost with no error anywhere.
    assert Enum.uniq(keys) == keys

    Mcpd.shutdown(c)
  end

  # ------------------------------------------------ section 6, retry ---

  for status <- [429, 502, 503, 504] do
    test "a #{status} is retried, and the retry carries the same ingest_key" do
      status = unquote(status)
      daemon = start_daemon(fn n -> if n < 3, do: {status, ~s({"error":"busy"})}, else: {200, @accepted} end)
      {:ok, c} = client(endpoint: daemon.endpoint)

      Mcpd.log(c, :info, "please land")
      started = System.monotonic_time(:millisecond)
      Mcpd.flush(c)
      elapsed = System.monotonic_time(:millisecond) - started

      requests = FakeDaemon.requests(daemon)
      assert length(requests) == 3, "two failures and the attempt that landed"
      # Exponential from 250 ms: at least 250 before the second attempt and 500
      # before the third, so anything under 750 ms did not back off.
      assert elapsed >= 750, "three attempts in #{elapsed}ms cannot have backed off"

      keys = daemon |> FakeDaemon.batches() |> Enum.map(& &1["ingest_key"]) |> Enum.uniq()
      assert length(keys) == 1, "a retry with a fresh key lets the daemon count the batch twice"

      Mcpd.shutdown(c)
    end
  end

  test "a daemon that keeps failing is given three attempts and then let go" do
    daemon = start_daemon(fn _n -> {503, "busy"} end)
    {:ok, c} = client(endpoint: daemon.endpoint)

    Mcpd.log(c, :info, "never lands")
    assert Mcpd.flush(c) == :ok

    assert length(FakeDaemon.requests(daemon)) == 3, "three attempts, not four and not forever"
    assert :sys.get_state(c).queue.size == 0, "a batch that cannot land is dropped"

    Mcpd.shutdown(c)
  end

  test "a connection that closes with no answer is retried" do
    daemon = start_daemon(fn n -> if n < 3, do: :close, else: {200, @accepted} end)
    {:ok, c} = client(endpoint: daemon.endpoint)

    Mcpd.log(c, :info, "please land")
    Mcpd.flush(c)

    assert length(FakeDaemon.requests(daemon)) == 3

    Mcpd.shutdown(c)
  end

  for status <- [400, 401, 404, 413, 500] do
    test "a #{status} is never retried, because the batch will not change" do
      status = unquote(status)
      daemon = start_daemon(fn _n -> {status, ~s({"error":"no"})} end)
      {:ok, c} = client(endpoint: daemon.endpoint)

      Mcpd.log(c, :info, "malformed, apparently")
      Mcpd.flush(c)

      assert length(FakeDaemon.requests(daemon)) == 1

      Mcpd.shutdown(c)
    end
  end

  test "Retry-After decides when the retry happens" do
    daemon =
      start_daemon(fn n ->
        if n == 1, do: {503, "busy", [{"retry-after", "1"}]}, else: {200, @accepted}
      end)

    {:ok, c} = client(endpoint: daemon.endpoint)

    Mcpd.log(c, :info, "held off")
    Mcpd.flush(c)

    [first, second] = FakeDaemon.requests(daemon)
    gap = second.at - first.at
    # Backoff on its own would be under 500 ms, so a full second can only have
    # come from reading the header.
    assert gap >= 1_000, "the server said one second and the retry came after #{gap}ms"
    assert gap < 5_000

    Mcpd.shutdown(c)
  end

  test "the retry never blocks the caller" do
    daemon = start_daemon(fn n -> if n < 3, do: {503, "busy"}, else: {200, @accepted} end)
    {:ok, c} = client(endpoint: daemon.endpoint, batch_size: 1)

    Mcpd.log(c, :info, "the batch being retried")
    started = System.monotonic_time(:millisecond)
    for i <- 1..50, do: Mcpd.log(c, :info, "line #{i}")
    assert Mcpd.with_span(c, "run", [], fn -> :ok end) == :ok
    elapsed = System.monotonic_time(:millisecond) - started

    assert elapsed < 250, "recording waited #{elapsed}ms behind a retry"

    Mcpd.flush(c)
    Mcpd.shutdown(c)
  end

  # ------------------------------------------------ section 6, the body ---

  test "a body over a kibibyte is gzipped, and a small one is not" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint)

    Mcpd.log(c, :info, "short")
    Mcpd.flush(c)
    Mcpd.with_span(c, "big prompt", [input: String.duplicate("x", 4_096)], fn -> :ok end)
    Mcpd.flush(c)

    [small, big] = FakeDaemon.requests(daemon)
    assert FakeDaemon.header(small, "content-encoding") == nil
    assert FakeDaemon.header(big, "content-encoding") == "gzip"
    assert String.to_integer(FakeDaemon.header(big, "content-length")) < 4_096, "a compressible body did not shrink"
    # The daemon could only read this by gunzipping it, which is what it does
    # with `content-encoding: gzip`.
    assert hd(FakeDaemon.signals(daemon, "spans"))["input"] == String.duplicate("x", 4_096)

    Mcpd.shutdown(c)
  end

  test "an api key goes out as a bearer token, and no key sends no header" do
    daemon = start_daemon(fn _n, _head -> {200, @accepted} end)
    {:ok, keyed} = client(endpoint: daemon.endpoint, api_key: "secret-key")
    {:ok, open} = client(endpoint: daemon.endpoint)

    Mcpd.log(keyed, :info, "signed")
    Mcpd.flush(keyed)
    Mcpd.log(open, :info, "unsigned")
    Mcpd.flush(open)

    [signed, unsigned] = FakeDaemon.requests(daemon)
    assert FakeDaemon.header(signed, "authorization") == "Bearer secret-key"
    assert FakeDaemon.header(unsigned, "authorization") == nil
    assert FakeDaemon.request_line(signed) == "POST /v1/ingest HTTP/1.1"

    Mcpd.shutdown(keyed)
    Mcpd.shutdown(open)
  end

  # ------------------------------------------------------------ correlation ---

  test "a score with no source is live, like every other SDK" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint)

    Mcpd.score(c, Ids.trace_id(), "scale_check", 3, min: 1, max: 5)
    Mcpd.flush(c)

    assert hd(FakeDaemon.signals(daemon, "scores"))["source"] == "live"

    Mcpd.shutdown(c)
  end

  test "a score keeps a source it was given" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint)

    Mcpd.score(c, Ids.trace_id(), "scale_check", 3, min: 1, max: 5, source: :offline)
    Mcpd.flush(c)

    assert hd(FakeDaemon.signals(daemon, "scores"))["source"] == "offline"

    Mcpd.shutdown(c)
  end

  test "the deployment reaches the wire on every kind of signal" do
    daemon = start_daemon()

    {:ok, c} =
      client(
        endpoint: daemon.endpoint,
        project: "support-desk",
        environment: "staging",
        service_name: "support-api",
        service_version: "9.9.9"
      )

    Mcpd.with_span(c, "support_agent", [], fn ->
      Mcpd.log(c, :info, "inside")
      Mcpd.metric(c, "steps", 1, kind: :counter)
    end)

    Mcpd.score(c, Ids.trace_id(), "relevance", 0.9, min: 0, max: 1)
    Mcpd.feedback(c, Ids.trace_id(), :thumbs, "up")
    Mcpd.flush(c)

    deployment = %{
      "project" => "support-desk",
      "environment" => "staging",
      "service_name" => "support-api",
      "service_version" => "9.9.9"
    }

    for kind <- ["spans", "logs", "metrics", "scores", "feedback"] do
      correlation = hd(FakeDaemon.signals(daemon, kind))["correlation"]
      assert Map.take(correlation, Map.keys(deployment)) == deployment, kind
    end

    Mcpd.shutdown(c)
  end

  test "the run's correlation reaches every step, and the log inside it" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint, project: "support-desk")

    run = [
      session_id: "sess-1",
      thread_id: "thread-7",
      user_id: "user-41",
      request_id: "req-9001",
      release: "2026.08.1"
    ]

    Mcpd.with_span(c, "support_agent", run, fn ->
      Mcpd.with_span(c, "plan_answer", [], fn ->
        Mcpd.with_span(c, "lookup_order", [], fn -> :ok end)
      end)

      Mcpd.log(c, :warn, "answering from the cached estimate")
    end)

    Mcpd.flush(c)

    wanted = Map.new(run, fn {k, v} -> {Atom.to_string(k), v} end)
    spans = FakeDaemon.signals(daemon, "spans")

    assert length(spans) == 3

    # The grandchild as well as the child. Inheritance that stops one level
    # down is the same defect wearing a different depth, and a two-span test
    # cannot tell the two apart.
    for span <- spans do
      assert Map.take(span["correlation"], Map.keys(wanted)) == wanted, span["name"]
    end

    log = hd(FakeDaemon.signals(daemon, "logs"))

    assert log["correlation"]["session_id"] == "sess-1"
    assert log["correlation"]["user_id"] == "user-41"

    Mcpd.shutdown(c)
  end

  test "a step may override one inherited field without losing the rest" do
    daemon = start_daemon()
    {:ok, c} = client(endpoint: daemon.endpoint)

    Mcpd.with_span(c, "run", [session_id: "sess-1", release: "2026.08.1"], fn ->
      Mcpd.with_span(c, "step", [session_id: "sess-2"], fn -> :ok end)
    end)

    Mcpd.flush(c)

    step =
      FakeDaemon.signals(daemon, "spans")
      |> Enum.find(&(&1["name"] == "step"))

    assert step["correlation"]["session_id"] == "sess-2"
    assert step["correlation"]["release"] == "2026.08.1"

    Mcpd.shutdown(c)
  end

  # ------------------------------------------------- section 5, the reads ---

  @trace_list ~s({"traces":[{"trace_id":"4bf92f3577b34da6a3ce929d0e0e4736","root_span_id":"00f067aa0ba902b7","name":"support agent","kind":"agent_run","status":"error","start_time":"2026-08-01T12:00:00.123456Z","span_count":4,"error_count":1,"model_count":1,"tool_count":2,"tags":["billing"],"correlation":{"project":"shop","environment":"production"},"is_complete":true,"received_at":"2026-08-01T12:00:01Z"}],"page":{"next_cursor":"c-2","has_more":true,"total":1}})

  test "list_traces sends the filters and decodes the page into the generated structs" do
    daemon = start_daemon(fn _n, _head -> {200, @trace_list} end)
    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")

    assert {:ok, %Mcpd.TraceListResponse{} = page} =
             Mcpd.list_traces(c, status: :error, kind: :agent_run, tags: ["billing", "urgent"], limit: 25, cursor: nil)

    [request] = FakeDaemon.requests(daemon)
    "GET " <> target = FakeDaemon.request_line(request)
    %URI{path: "/v1/api/traces", query: query} = URI.parse(String.replace_suffix(target, " HTTP/1.1", ""))
    query = URI.decode_query(query)
    assert query["project"] == "shop", "a read without the project reads the wrong tenant"
    assert query["status"] == "error"
    assert query["kind"] == "agent_run"
    assert query["tags"] == "billing,urgent"
    assert query["limit"] == "25"
    refute Map.has_key?(query, "cursor"), "an unset filter must not be sent"

    assert %Mcpd.Page{next_cursor: "c-2", has_more: true, total: 1} = page.page
    assert [%Mcpd.Trace{} = trace] = page.traces
    assert trace.name == "support agent"
    assert trace.kind == :agent_run
    assert trace.status == :error
    assert trace.start_time == ~U[2026-08-01 12:00:00.123456Z]
    assert %Mcpd.Correlation{project: "shop", environment: "production"} = trace.correlation
    assert trace.tags == ["billing"]

    Mcpd.shutdown(c)
  end

  test "every read hits its own path and comes back as its own struct" do
    bodies = %{
      "/v1/api/traces/4bf92f3577b34da6a3ce929d0e0e4736" =>
        {Mcpd.TraceDetail, ~s({"trace":{"trace_id":"4bf92f3577b34da6a3ce929d0e0e4736","name":"run","kind":"agent_run","status":"ok","start_time":"2026-08-01T12:00:00Z","span_count":1,"error_count":0,"model_count":0,"tool_count":0,"correlation":{"project":"shop"},"is_complete":true,"received_at":"2026-08-01T12:00:00Z"},"spans":[{"trace_id":"4bf92f3577b34da6a3ce929d0e0e4736","span_id":"00f067aa0ba902b7","name":"run","kind":"tool_call","status":"ok","start_time":"2026-08-01T12:00:00Z","is_root":true,"is_event":false,"correlation":{"project":"shop"},"received_at":"2026-08-01T12:00:00Z","error":{"message":"m","kind":"ArgumentError"}}],"logs":[],"scores":[]})},
      "/v1/api/spans" =>
        {Mcpd.SpanListResponse, ~s({"spans":[{"trace_id":"4bf92f3577b34da6a3ce929d0e0e4736","span_id":"00f067aa0ba902b7","name":"step","kind":"tool_call","status":"ok","start_time":"2026-08-01T12:00:00Z","is_root":false,"is_event":false,"correlation":{"project":"shop"},"received_at":"2026-08-01T12:00:00Z"}],"page":{"has_more":false}})},
      "/v1/api/logs" =>
        {Mcpd.LogListResponse, ~s({"logs":[{"log_id":"l","timestamp":"2026-08-01T12:00:00Z","level":"warn","message":"slow","correlation":{"project":"shop"},"received_at":"2026-08-01T12:00:00Z"}],"page":{"has_more":false}})},
      "/v1/api/metrics" =>
        {Mcpd.MetricSeriesResponse, ~s({"series":[{"labels":{"outcome":"ok"},"points":[{"timestamp":"2026-08-01T12:00:00Z","value":1.5,"count":1}]}],"name":"orders.looked_up","bucket":"1h","unit":"{order}"})},
      "/v1/api/scores" =>
        {Mcpd.ScoreListResponse, ~s({"scores":[{"score_id":"s","scorer_id":"relevance","source":"live","value":0.9,"timestamp":"2026-08-01T12:00:00Z","correlation":{"project":"shop"}}],"page":{"has_more":false}})},
      "/v1/api/feedback" =>
        {Mcpd.FeedbackListResponse, ~s({"feedback":[{"feedback_id":"f","kind":"thumbs","value":"up","timestamp":"2026-08-01T12:00:00Z","correlation":{"project":"shop"}}],"page":{"has_more":false}})},
      "/v1/api/analytics/summary" =>
        {Mcpd.AnalyticsSummary, ~s({"from":"2026-08-01T11:00:00Z","to":"2026-08-01T13:00:00Z","trace_count":7,"span_count":21,"error_count":1,"error_rate":0.14,"latency":{"p50":1,"p90":2,"p95":2,"p99":3,"mean":1.5,"max":3},"input_tokens":10,"output_tokens":5,"total_cost":0.5,"currency":"USD","groups":[]})},
      "/v1/api/analytics/cost" =>
        {Mcpd.CostBreakdown, ~s({"from":"2026-08-01T11:00:00Z","to":"2026-08-01T13:00:00Z","total_cost":0.5,"currency":"USD","groups":[],"series":[]})},
      "/healthz" =>
        {Mcpd.HealthStatus, ~s({"status":"ok","version":"0.1.0","spec_version":"1.0.0","uptime_seconds":9,"storage":"sqlite"})}
    }

    daemon =
      start_daemon(fn _n, head ->
        "GET " <> target = head |> String.split("\r\n") |> hd()
        path = target |> String.replace_suffix(" HTTP/1.1", "") |> URI.parse() |> Map.fetch!(:path)
        {_module, body} = Map.fetch!(bodies, path)
        {200, body}
      end)

    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")
    from = ~U[2026-08-01 11:00:00Z]
    to = "-0s"

    assert {:ok, %Mcpd.TraceDetail{} = detail} =
             Mcpd.get_trace(c, "4bf92f3577b34da6a3ce929d0e0e4736", include: ["logs", "scores"])

    assert %Mcpd.Trace{name: "run"} = detail.trace
    assert [%Mcpd.Span{kind: :tool_call, error: %Mcpd.SpanError{kind: "ArgumentError"}}] = detail.spans

    assert {:ok, %Mcpd.SpanListResponse{spans: [%Mcpd.Span{name: "step", is_root: false}]}} =
             Mcpd.list_spans(c, trace_id: "4bf92f3577b34da6a3ce929d0e0e4736", from: from, to: to)

    assert {:ok, %Mcpd.LogListResponse{logs: [%Mcpd.LogRecord{level: :warn}]}} = Mcpd.list_logs(c, level: :warn)

    assert {:ok, %Mcpd.MetricSeriesResponse{name: "orders.looked_up", series: [series]}} =
             Mcpd.query_metrics(c, "orders.looked_up", from, to,
               bucket: "1h",
               agg: "sum",
               filter: %{"model" => "gpt-4o", status: :ok}
             )

    for bad <- [%{"a:b" => "x"}, %{"a,b" => "x"}, %{"" => "x"}, %{"model" => "a,b"}] do
      assert_raise ArgumentError, fn -> Mcpd.query_metrics(c, "orders.looked_up", from, to, filter: bad) end
    end

    assert %Mcpd.MetricSeries{points: [%Mcpd.MetricSeriesPoint{value: 1.5, timestamp: %DateTime{}}]} = series

    assert {:ok, %Mcpd.ScoreListResponse{scores: [%Mcpd.Score{source: :live, value: 0.9}]}} =
             Mcpd.list_scores(c, scorer_id: "relevance")

    assert {:ok, %Mcpd.FeedbackListResponse{feedback: [%Mcpd.Feedback{kind: :thumbs, value: "up"}]}} =
             Mcpd.list_feedback(c, "4bf92f3577b34da6a3ce929d0e0e4736", limit: 5)

    # The daemon answers 400 to any other key, so a filter it does not take is
    # refused here, before a request is made.
    assert_raise ArgumentError, fn -> Mcpd.list_feedback(c, "4bf92f3577b34da6a3ce929d0e0e4736", kind: :thumbs) end
    assert_raise ArgumentError, fn -> Mcpd.list_feedback(c, kind: :thumbs) end

    assert {:ok, %Mcpd.AnalyticsSummary{trace_count: 7, latency: %Mcpd.LatencyStats{p95: 2}}} =
             Mcpd.analytics_summary(c, from, to, group_by: "model")

    assert {:ok, %Mcpd.CostBreakdown{currency: "USD", total_cost: 0.5}} =
             Mcpd.analytics_cost(c, from, to, group_by: "model", bucket: "1h")

    assert {:ok, %Mcpd.HealthStatus{status: "ok", spec_version: "1.0.0"}} = Mcpd.health(c)

    queries =
      daemon
      |> FakeDaemon.requests()
      |> Map.new(fn request ->
        "GET " <> target = FakeDaemon.request_line(request)
        %URI{path: path, query: query} = target |> String.replace_suffix(" HTTP/1.1", "") |> URI.parse()
        {path, URI.decode_query(query || "")}
      end)

    assert map_size(queries) == map_size(bodies), "a read hit a path the table did not expect"
    assert queries["/v1/api/traces/4bf92f3577b34da6a3ce929d0e0e4736"]["include"] == "logs,scores"
    assert queries["/v1/api/metrics"]["name"] == "orders.looked_up"
    assert queries["/v1/api/metrics"]["from"] == "2026-08-01T11:00:00Z", "a DateTime goes out as RFC 3339"
    assert queries["/v1/api/metrics"]["to"] == "-0s", "a shorthand goes out as it was given"

    # The daemon reads key:value pairs. A map used to raise from to_string/1.
    assert queries["/v1/api/metrics"]["filter"] in ["model:gpt-4o,status:ok", "status:ok,model:gpt-4o"]
    assert queries["/v1/api/analytics/cost"]["bucket"] == "1h"

    assert queries["/v1/api/feedback"] == %{
             "project" => "shop",
             "trace_id" => "4bf92f3577b34da6a3ce929d0e0e4736",
             "limit" => "5"
           }
    refute Map.has_key?(queries["/healthz"], "project"), "the health probe takes no project"

    for path <- Map.keys(bodies), path != "/healthz" do
      assert queries[path]["project"] == "shop", "#{path} was read without the client's project"
    end

    Mcpd.shutdown(c)
  end

  test "a read that fails says so instead of returning nothing" do
    daemon = start_daemon(fn _n, _head -> {404, ~s({"error":"not_found","message":"trace abc not found"})} end)
    {:ok, c} = client(endpoint: daemon.endpoint)

    assert {:error, {:http, 404, body, _retry_after}} = Mcpd.get_trace(c, "abc")
    assert body =~ "trace abc not found"
    # A read is not retried: a caller waiting on an answer would rather have
    # the failure than the delay.
    assert length(FakeDaemon.requests(daemon)) == 1

    Mcpd.shutdown(c)
    assert Mcpd.get_trace(c, "abc") == {:error, :no_client}
  end

  test "a read against a body that is not JSON fails, because a read is being waited on" do
    daemon = start_daemon(fn _n, _head -> {200, "<html>fronted by a proxy</html>"} end)
    {:ok, c} = client(endpoint: daemon.endpoint)

    assert {:error, {:bad_json, _}} = Mcpd.health(c)

    Mcpd.shutdown(c)
  end

  # ----------------------------------------------------------- helpers ---

  defp start_daemon(reply \\ fn _n -> {200, @accepted} end) do
    daemon = FakeDaemon.start(reply)
    on_exit(fn -> FakeDaemon.stop(daemon) end)
    daemon
  end

  # A long flush interval everywhere: these tests export when they say so, not
  # when a timer happens to fire in the middle of an assertion.
  defp client(opts), do: Mcpd.start_link(Keyword.put_new(opts, :flush_interval, 60_000))

  defp wait_until(condition, deadline \\ 5_000) do
    cond do
      condition.() -> true
      deadline <= 0 -> false
      true ->
        Process.sleep(10)
        wait_until(condition, deadline - 10)
    end
  end
end
