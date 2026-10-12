defmodule Mcpd.Sink do
  @moduledoc """
  Where a drained batch goes.

  The worker drains its queue into one batch, in the shape `POST /v1/ingest`
  accepts, and hands it to the client's sink together with the batch's
  `ingest_key`. The HTTP sink is the default and the only one that talks to a
  daemon. Two more ship with the SDK: `Mcpd.Sink.Recorder`, which keeps every
  batch in memory so a test can assert what the instrumentation recorded
  without a server, and `Mcpd.Sink.Console`, which prints one JSON line per
  signal for a local look while debugging.

      {:ok, recorder} = Mcpd.Sink.Recorder.start_link()
      {:ok, mcpd} = Mcpd.start_link(sink: {Mcpd.Sink.Recorder, recorder})
      Mcpd.span mcpd, "plan", do: :ok
      :ok = Mcpd.flush(mcpd)
      [span] = Mcpd.Sink.Recorder.spans(recorder)

  A sink runs in the worker's sender process, inside the same guard as the
  transport: one that raises or answers an error is a failed batch, reported
  through the worker's throttle and counted in `Mcpd.stats/1`, never surfaced
  to the caller. A sink is chosen in code only; it has no environment
  variable. The batch a sink sees is what the daemon would have received:
  after sampling, after payload capture, after validation.
  """

  @doc """
  Deliver one batch. `batch` is the wire map, string keys, with `ingest_key`
  already in it; `opts` is whatever was given beside the module in the
  client's `sink:` option.
  """
  @callback export(batch :: map(), ingest_key :: String.t(), opts :: term()) ::
              :ok | {:ok, term()} | {:error, term()}
end

defmodule Mcpd.Sink.Http do
  @moduledoc """
  The default sink: the daemon's ingest endpoint over `Mcpd.Http`, with the
  retry, the gzip and the bearer token of contract section 6. The worker
  fills `url`, `api_key` and `timeout` from the client's own config.
  """
  @behaviour Mcpd.Sink

  @impl true
  def export(batch, _ingest_key, opts) do
    case Mcpd.Http.post_batch(opts[:url], batch, api_key: opts[:api_key], timeout: opts[:timeout]) do
      {:ok, _body} -> :ok
      {:error, _} = failure -> failure
    end
  end
end

defmodule Mcpd.Sink.Recorder do
  @moduledoc """
  A sink that keeps every batch in memory, for a test.

      {:ok, recorder} = Mcpd.Sink.Recorder.start_link()
      {:ok, mcpd} = Mcpd.start_link(sink: {Mcpd.Sink.Recorder, recorder})

  `spans/1`, `logs/1`, `metrics/1`, `scores/1` and `feedback/1` answer the
  records seen so far, oldest first, as the wire maps the daemon would have
  read. `batches/1` is how many batches arrived and `clear/1` forgets them.
  """
  @behaviour Mcpd.Sink

  @kinds ~w(spans logs metrics scores feedback)

  @doc "Start a recorder. The pid is what goes beside the module in `sink:`."
  @spec start_link(keyword()) :: Agent.on_start()
  def start_link(_opts \\ []), do: Agent.start_link(fn -> empty() end)

  @impl true
  def export(batch, _ingest_key, recorder) when is_map(batch) do
    Agent.update(recorder, fn seen ->
      seen = Map.update!(seen, :batches, &(&1 + 1))

      Enum.reduce(@kinds, seen, fn kind, acc ->
        Map.update!(acc, String.to_atom(kind), &(&1 ++ Map.get(batch, kind, [])))
      end)
    end)
  end

  @doc "Every span seen, oldest first."
  @spec spans(pid()) :: [map()]
  def spans(recorder), do: Agent.get(recorder, & &1.spans)

  @doc "Every log record seen, oldest first."
  @spec logs(pid()) :: [map()]
  def logs(recorder), do: Agent.get(recorder, & &1.logs)

  @doc "Every metric point seen, oldest first."
  @spec metrics(pid()) :: [map()]
  def metrics(recorder), do: Agent.get(recorder, & &1.metrics)

  @doc "Every score seen, oldest first."
  @spec scores(pid()) :: [map()]
  def scores(recorder), do: Agent.get(recorder, & &1.scores)

  @doc "Every piece of feedback seen, oldest first."
  @spec feedback(pid()) :: [map()]
  def feedback(recorder), do: Agent.get(recorder, & &1.feedback)

  @doc "How many batches have arrived."
  @spec batches(pid()) :: non_neg_integer()
  def batches(recorder), do: Agent.get(recorder, & &1.batches)

  @doc "Forget everything seen so far."
  @spec clear(pid()) :: :ok
  def clear(recorder), do: Agent.update(recorder, fn _ -> empty() end)

  defp empty, do: %{spans: [], logs: [], metrics: [], scores: [], feedback: [], batches: 0}
end

defmodule Mcpd.Sink.Console do
  @moduledoc """
  A sink that prints one JSON line per signal, for a look while debugging.

      {:ok, mcpd} = Mcpd.start_link(sink: Mcpd.Sink.Console)

  Each line is the record as the daemon would have read it, with one extra
  key, `signal`, naming its kind: `span`, `log`, `metric`, `score` or
  `feedback`. Lines go to standard error, so they never land in the middle of
  the agent's own output; `write: fun` takes each line instead.
  """
  @behaviour Mcpd.Sink

  @kinds [
    {"spans", "span"},
    {"logs", "log"},
    {"metrics", "metric"},
    {"scores", "score"},
    {"feedback", "feedback"}
  ]

  @impl true
  def export(batch, _ingest_key, opts) when is_map(batch) do
    write = (is_list(opts) && opts[:write]) || (&stderr/1)

    for {kind, signal} <- @kinds, record <- Map.get(batch, kind, []) do
      line = record |> Map.put("signal", signal) |> :json.encode() |> IO.iodata_to_binary()
      write.(line <> "\n")
    end

    :ok
  end

  defp stderr(line), do: :io.put_chars(:standard_error, line)
end
