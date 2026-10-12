defmodule McpdTest do
  @moduledoc """
  The contract, tested.

  Every behaviour in `sdks/SDK-CONTRACT.md` that can be checked without a daemon
  is checked here. The ones that need a real daemon are in the end-to-end suite,
  because the failures that matter most only appear once a request reaches a
  server.
  """
  use ExUnit.Case, async: false

  alias Mcpd.{Codec, Config, Context, Http, Ids, Queue}

  describe "B6 identifiers" do
    test "are OpenTelemetry-shaped and never all zero" do
      for _ <- 1..200 do
        t = Ids.trace_id()
        s = Ids.span_id()
        assert String.length(t) == 32 and String.match?(t, ~r/^[0-9a-f]{32}$/)
        assert String.length(s) == 16 and String.match?(s, ~r/^[0-9a-f]{16}$/)
        refute String.match?(t, ~r/^0+$/)
      end
    end

    test "reject an id of the wrong shape" do
      refute Ids.valid?("00000000000000000000000000000000", 16)
      refute Ids.valid?("XYZ", 16)
      refute Ids.valid?(nil, 16)
      refute Ids.valid?(String.duplicate("a", 31), 16)
    end
  end

  describe "B7 W3C trace context" do
    test "round-trips a traceparent, sampled flag included" do
      t = Ids.trace_id()
      s = Ids.span_id()

      assert {:ok, %{trace_id: ^t, span_id: ^s, sampled: true}} =
               Context.parse_traceparent(Context.traceparent(t, s))

      assert String.ends_with?(Context.traceparent(t, s, false), "-00")

      assert {:ok, %{trace_id: ^t, span_id: ^s, sampled: false}} =
               Context.parse_traceparent(Context.traceparent(t, s, false))
    end

    test "reads a header from another implementation, and the flag bits above the first" do
      assert {:ok, %{sampled: true}} = Context.parse_traceparent("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01")
      assert {:ok, %{sampled: false}} = Context.parse_traceparent("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-00")
      # A reserved bit set does not hide the sampled bit.
      assert {:ok, %{sampled: true}} = Context.parse_traceparent("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-03")
      # A future version may append fields; the first four keep their meaning.
      assert {:ok, %{sampled: true}} = Context.parse_traceparent("01-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01-extra")
    end

    test "a malformed header is an error, never a raise" do
      for bad <- [
            nil,
            "",
            "garbage",
            "00-short-#{Ids.span_id()}-01",
            "ff-#{Ids.trace_id()}-#{Ids.span_id()}-01",
            "00-00000000000000000000000000000000-#{Ids.span_id()}-01",
            "00-#{Ids.trace_id()}-0000000000000000-01",
            "00-#{Ids.trace_id()}-#{Ids.span_id()}-zz"
          ] do
        assert Context.parse_traceparent(bad) == :error, "accepted #{inspect(bad)}"
      end
    end
  end

  describe "B3 the queue is bounded" do
    test "drops the oldest and counts it" do
      q = Enum.reduce(1..10, Queue.new(4), &Queue.push(&2, &1, :logs, %{"n" => &1}))
      assert q.size == 4
      assert q.dropped == 6
      {items, _} = Queue.take(q, 4)
      # The newest survive: when the daemon is unreachable, what is happening
      # now is what somebody will want to read when it comes back.
      assert Enum.map(items, fn {:logs, %{"n" => n}} -> n end) == [7, 8, 9, 10]
    end

    test "a keyed entry is updated in place and keeps its position" do
      q =
        Queue.new(4)
        |> Queue.push(:a, :spans, %{"status" => "unset"})
        |> Queue.push(:b, :logs, %{"n" => 1})
        |> Queue.push(:a, :spans, %{"status" => "ok"})

      # B8 at the queue: the end replaces the start, so a span that opens and
      # closes between two flushes costs one record, in the order it started.
      assert q.size == 2
      {items, rest} = Queue.take(q, 10)
      assert items == [{:spans, %{"status" => "ok"}}, {:logs, %{"n" => 1}}]
      assert Queue.empty?(rest)
    end

    test "a key evicted by the bound can be queued again as a new entry" do
      q =
        Queue.new(1)
        |> Queue.push(:a, :spans, %{"status" => "unset"})
        |> Queue.push(:b, :logs, %{"n" => 1})
        |> Queue.push(:a, :spans, %{"status" => "ok"})

      assert q.dropped == 2
      {items, _} = Queue.take(q, 10)
      assert items == [{:spans, %{"status" => "ok"}}]
    end
  end

  describe "B4 the sampling rule" do
    # The vectors the other SDKs pin: the low 64 bits of the trace id against
    # `sample * 2^64`. Two languages that disagree on one of these would keep
    # and drop different traces from the same fleet.
    test "agrees with the rule the daemon and the other SDKs use" do
      assert Mcpd.__sampled_for_test__(1.0, "0af7651916cd43ddffffffffffffffff")
      refute Mcpd.__sampled_for_test__(0.0, "0af7651916cd43dd0000000000000001")
      assert Mcpd.__sampled_for_test__(0.5, "0af7651916cd43dd0000000000000001")
      refute Mcpd.__sampled_for_test__(0.5, "0af7651916cd43ddffffffffffffffff")
      # 0x8448eb211c80319c is above 2^63, so a half sample drops it.
      refute Mcpd.__sampled_for_test__(0.5, "0af7651916cd43dd8448eb211c80319c")
      assert Mcpd.__sampled_for_test__(0.52, "0af7651916cd43dd8448eb211c80319c")
      # Exactly at the boundary is dropped: the comparison is strict.
      refute Mcpd.__sampled_for_test__(0.5, "0af7651916cd43dd8000000000000000")
      assert Mcpd.__sampled_for_test__(0.5, "0af7651916cd43dd7fffffffffffffff")
    end

    test "is deterministic for a trace id and lands near the rate over a fleet" do
      id = Ids.trace_id()
      assert Mcpd.__sampled_for_test__(0.3, id) == Mcpd.__sampled_for_test__(0.3, id)
      kept = Enum.count(1..2000, fn _ -> Mcpd.__sampled_for_test__(0.5, Ids.trace_id()) end)
      assert kept > 800 and kept < 1200, "a half sample kept #{kept} of 2000"
    end
  end

  describe "section 6 the retry policy" do
    test "retries the four statuses and a connection failure, and nothing else" do
      for status <- [429, 502, 503, 504], do: assert(Http.retryable?({:http, status, "", nil}), "#{status}")
      for status <- [400, 401, 403, 404, 413, 422, 500], do: refute(Http.retryable?({:http, status, "", nil}), "#{status}")
      assert Http.retryable?({:failed_connect, []})
      assert Http.retryable?(:timeout)
      assert Http.retryable?(:socket_closed_remotely)
      refute Http.retryable?({:encode, "a tuple"})
    end

    test "backs off exponentially from 250 ms with jitter, and honours Retry-After capped" do
      for _ <- 1..50 do
        second = Http.delay_ms(:timeout, 2)
        third = Http.delay_ms(:timeout, 3)
        assert second >= 250 and second < 500, "second attempt waits #{second}ms"
        assert third >= 500 and third < 750, "third attempt waits #{third}ms"
      end

      assert Http.delay_ms({:http, 503, "", 1_000}, 2) == 1_000
      # An honest `Retry-After: 3600` must not hold a shutdown for an hour.
      assert Http.delay_ms({:http, 429, "", 3_600_000}, 2) == 30_000
      assert Http.max_attempts() == 3
    end
  end

  describe "configuration" do
    test "works with no arguments at all" do
      c = Config.new()
      assert c.endpoint == "http://localhost:4317"
      assert c.project == "default"
      assert c.enabled
    end

    test "code wins over the environment" do
      System.put_env("MCPD_PROJECT", "from-env")
      assert Config.new().project == "from-env"
      assert Config.new(project: "from-code").project == "from-code"
    after
      System.delete_env("MCPD_PROJECT")
    end

    test "every tuning variable is read in the unit its name says, and code wins" do
      System.put_env("MCPD_BATCH_SIZE", "7")
      System.put_env("MCPD_FLUSH_INTERVAL_MS", "150")
      System.put_env("MCPD_MAX_QUEUE", "33")
      System.put_env("MCPD_TIMEOUT_MS", "900")
      System.put_env("MCPD_SAMPLE", "0.25")

      from_env = Config.new()
      assert from_env.batch_size == 7
      assert from_env.flush_interval == 150
      assert from_env.max_queue == 33
      assert from_env.timeout == 900
      assert from_env.sample == 0.25

      from_code = Config.new(batch_size: 8, flush_interval: 160, max_queue: 34, timeout: 901, sample: 0.5)
      assert from_code.batch_size == 8
      assert from_code.flush_interval == 160
      assert from_code.max_queue == 34
      assert from_code.timeout == 901
      assert from_code.sample == 0.5
    after
      for name <- ~w(MCPD_BATCH_SIZE MCPD_FLUSH_INTERVAL_MS MCPD_MAX_QUEUE MCPD_TIMEOUT_MS MCPD_SAMPLE) do
        System.delete_env(name)
      end
    end

    test "a malformed environment value falls back rather than crashing" do
      System.put_env("MCPD_BATCH_SIZE", "not-a-number")
      System.put_env("MCPD_SAMPLE", "")
      assert Config.new().batch_size == 128
      assert Config.new().sample == 1.0
    after
      System.delete_env("MCPD_BATCH_SIZE")
      System.delete_env("MCPD_SAMPLE")
    end

    test "MCPD_ENABLED=false is respected" do
      System.put_env("MCPD_ENABLED", "false")
      refute Config.new().enabled
    after
      System.delete_env("MCPD_ENABLED")
    end
  end

  describe "the codec" do
    test "knows every generated struct, and no enum" do
      known = Codec.known()
      assert Mcpd.Trace in known
      assert Mcpd.TraceDetail in known
      assert Mcpd.HealthStatus in known
      assert Mcpd.Correlation in known
      refute Mcpd.SpanKind in known
      refute Mcpd.Spec in known
      assert length(known) > 40
    end

    test "keeps a value this build has never heard of rather than refusing the read" do
      span = Codec.decode(Mcpd.Span, %{"span_id" => "s", "kind" => "a_kind_from_next_year", "start_time" => "not a time"})
      assert span.kind == "a_kind_from_next_year"
      assert span.start_time == "not a time"
    end
  end

  describe "B1 instrumentation never takes the caller down" do
    test "an unreachable daemon does not raise, and the block still returns" do
      # Port 1 is not listening. Every send will fail.
      {:ok, c} = Mcpd.start_link(endpoint: "http://127.0.0.1:1", flush_interval: 50)

      assert Mcpd.with_span(c, "run", [], fn -> :the_answer end) == :the_answer
      assert Mcpd.log(c, :info, "still fine") == :ok
      assert Mcpd.metric(c, "m", 1) == :ok
      assert Mcpd.flush(c) == :ok
      assert Mcpd.shutdown(c) == :ok
    end

    test "a raise inside a span is recorded and re-raised unchanged" do
      {:ok, c} = Mcpd.start_link(endpoint: "http://127.0.0.1:1")

      assert_raise RuntimeError, "boom", fn ->
        Mcpd.with_span(c, "run", [], fn -> raise "boom" end)
      end

      Mcpd.shutdown(c)
    end
  end

  describe "B5 flush and shutdown" do
    test "are idempotent" do
      {:ok, c} = Mcpd.start_link(endpoint: "http://127.0.0.1:1")
      assert Mcpd.flush(c) == :ok
      assert Mcpd.flush(c) == :ok
      assert Mcpd.shutdown(c) == :ok
      # Calling shutdown on a stopped client must not raise.
      assert Mcpd.shutdown(c) == :ok
    end
  end

  describe "the span tree assembles itself" do
    test "a child finds its parent through the process, not an argument" do
      {:ok, c} = Mcpd.start_link(endpoint: "http://127.0.0.1:1")

      Mcpd.with_span(c, "root", [], fn ->
        root = Context.current()
        assert root.parent_span_id == nil

        Mcpd.with_span(c, "child", [], fn ->
          child = Context.current()
          assert child.parent_span_id == root.span_id
          assert child.trace_id == root.trace_id
        end)

        # The parent is restored after the child ends.
        assert Context.current().span_id == root.span_id
      end)

      Mcpd.shutdown(c)
    end

    test "the parent is restored even when the child raises" do
      {:ok, c} = Mcpd.start_link(endpoint: "http://127.0.0.1:1")

      Mcpd.with_span(c, "root", [], fn ->
        root = Context.current()
        catch_error(Mcpd.with_span(c, "child", [], fn -> raise "x" end))
        assert Context.current().span_id == root.span_id
      end)

      Mcpd.shutdown(c)
    end

    test "two processes do not see each other's span" do
      {:ok, c} = Mcpd.start_link(endpoint: "http://127.0.0.1:1")
      parent = self()

      Mcpd.with_span(c, "in-this-process", [], fn ->
        spawn(fn -> send(parent, {:seen, Context.current()}) end)
        assert_receive {:seen, nil}, 1_000
      end)

      Mcpd.shutdown(c)
    end
  end

  describe "B9 timestamps" do
    test "are UTC RFC 3339 with nanosecond resolution" do
      ts = :erlang.apply(Mcpd, :__ts_for_test__, [])
      assert String.match?(ts, ~r/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{9}Z$/), ts
    end
  end
end
