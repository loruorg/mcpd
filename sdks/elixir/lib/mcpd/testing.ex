defmodule Mcpd.Testing do
  @moduledoc """
  A client for your own unit tests (contract section 2.5).

      test "answers with the model's text" do
        {:ok, testing} = Mcpd.Testing.start()
        answer = MyAgent.answer(testing.client, "Where is my order?")

        span = Mcpd.Testing.span(testing, "model_call")
        assert span["output"] == answer
        assert span["usage"]["output_tokens"] == 37
      end

  `testing.client` is a real `Mcpd` client whose sink is an
  `Mcpd.Sink.Recorder`, so what you read back went through sampling, payload
  capture, redaction and the queue exactly as it would on the way to a
  daemon. Nothing leaves the process. Every reader flushes first, so a test
  never calls `Mcpd.flush/2` itself.

  Records are the wire maps the daemon would have read, with string keys.
  """

  @enforce_keys [:client, :recorder]
  defstruct [:client, :recorder]

  @type t :: %__MODULE__{client: pid(), recorder: pid()}

  @typedoc "One finished span and the finished spans under it, by start time."
  @type tree :: %{span: map(), children: [tree()]}

  defmodule Error do
    @moduledoc "Raised by `Mcpd.Testing.span/2` when there is not exactly one match."
    defexception [:message]
  end

  @doc """
  Start a test client, linked to the calling process so it stops with your
  test.

  Takes every `Mcpd.start_link/1` option except `:sink`. `enabled` defaults to
  `true` and `sample` to `1.0` whatever the environment says, so a CI job that
  sets `MCPD_ENABLED=false` does not turn your test client off.
  """
  @spec start(keyword()) :: {:ok, t()} | {:error, term()}
  def start(opts \\ []) do
    opts =
      opts
      |> Keyword.delete(:sink)
      |> Keyword.put_new(:enabled, true)
      |> Keyword.put_new(:sample, 1.0)

    with {:ok, recorder} <- Mcpd.Sink.Recorder.start_link(),
         {:ok, client} <- Mcpd.start_link([{:sink, {Mcpd.Sink.Recorder, recorder}} | opts]) do
      {:ok, %__MODULE__{client: client, recorder: recorder}}
    end
  end

  @doc "Stop the client and the recorder."
  @spec stop(t()) :: :ok
  def stop(%__MODULE__{client: client, recorder: recorder}) do
    Mcpd.shutdown(client)
    if Process.alive?(recorder), do: Agent.stop(recorder)
    :ok
  end

  @doc """
  The finished spans, one per span id, in the order each id first arrived.

  A span goes on the wire when it opens and again when it closes, so the
  last record for each id is the one kept, and a span still open is left
  out. `name:` and `kind:` filter; `kind:` takes an atom or a string.
  """
  @spec spans(t(), keyword()) :: [map()]
  def spans(%__MODULE__{} = testing, filter \\ []) do
    name = filter[:name]
    kind = filter[:kind] && to_string(filter[:kind])

    testing
    |> read(&Mcpd.Sink.Recorder.spans/1)
    |> finished()
    |> Enum.filter(fn s -> (name == nil or s["name"] == name) and (kind == nil or s["kind"] == kind) end)
  end

  @doc """
  The one finished span called `name`. Raises `Mcpd.Testing.Error` when there
  is none or more than one, with the count and the names that were recorded.
  """
  @spec span(t(), String.t()) :: map()
  def span(%__MODULE__{} = testing, name) do
    all = spans(testing)

    case Enum.filter(all, &(&1["name"] == name)) do
      [one] ->
        one

      found ->
        seen =
          case all |> Enum.map(& &1["name"]) |> Enum.uniq() do
            [] -> "none"
            names -> Enum.join(names, ", ")
          end

        raise Error,
              "expected one finished span named #{inspect(name)}, found #{length(found)}; " <>
                "spans recorded: #{seen}"
    end
  end

  @doc "Every log record, oldest first."
  @spec logs(t()) :: [map()]
  def logs(%__MODULE__{} = testing), do: read(testing, &Mcpd.Sink.Recorder.logs/1)

  @doc "Every metric point, oldest first."
  @spec metrics(t()) :: [map()]
  def metrics(%__MODULE__{} = testing), do: read(testing, &Mcpd.Sink.Recorder.metrics/1)

  @doc "Every score, oldest first."
  @spec scores(t()) :: [map()]
  def scores(%__MODULE__{} = testing), do: read(testing, &Mcpd.Sink.Recorder.scores/1)

  @doc """
  Every piece of feedback, oldest first. Not `feedback/1`, because
  `Mcpd.feedback/5` is the call that records one.
  """
  @spec recorded_feedback(t()) :: [map()]
  def recorded_feedback(%__MODULE__{} = testing), do: read(testing, &Mcpd.Sink.Recorder.feedback/1)

  @doc """
  Every trace as a tree of finished spans: the roots, each with its children
  ordered by start time. A span whose parent was not recorded is a root.
  """
  @spec trees(t()) :: [tree()]
  def trees(%__MODULE__{} = testing) do
    spans = spans(testing)
    ids = MapSet.new(spans, & &1["span_id"])

    children =
      Enum.group_by(spans, fn s ->
        parent = s["parent_span_id"]
        if parent != nil and parent != s["span_id"] and MapSet.member?(ids, parent), do: parent, else: :root
      end)

    children |> Map.get(:root, []) |> build(children)
  end

  @doc "Flush, then forget everything recorded so far."
  @spec reset(t()) :: :ok
  def reset(%__MODULE__{client: client, recorder: recorder}) do
    Mcpd.flush(client)
    Mcpd.Sink.Recorder.clear(recorder)
  end

  defp build(spans, children) do
    spans
    |> Enum.sort_by(& &1["start_time"])
    |> Enum.map(fn s -> %{span: s, children: build(Map.get(children, s["span_id"], []), children)} end)
  end

  defp read(%__MODULE__{client: client, recorder: recorder}, fetch) do
    Mcpd.flush(client)
    fetch.(recorder)
  end

  # The last record per span id, in first-arrival order, ended spans only.
  defp finished(records) do
    {order, last} =
      Enum.reduce(records, {[], %{}}, fn s, {order, last} ->
        id = s["span_id"]
        order = if Map.has_key?(last, id), do: order, else: [id | order]
        {order, Map.put(last, id, s)}
      end)

    order
    |> Enum.reverse()
    |> Enum.map(&Map.fetch!(last, &1))
    |> Enum.filter(&Map.has_key?(&1, "end_time"))
  end
end
