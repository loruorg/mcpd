defmodule Mcpd do
  @moduledoc """
  Agent observability for Elixir.

      {:ok, mcpd} = Mcpd.start_link(project: "support-desk")

      answer =
        Mcpd.span mcpd, "support_agent", kind: :agent_run, input: question do
          Mcpd.span mcpd, "plan", kind: :model_generation do
            call_the_model(question)
          end
        end

  Nothing in that path touches the network. A span is appended to a bounded
  queue and a background worker sends it, so an agent's latency never depends on
  mcpd being fast, or up, or running at all.

  The nesting assembles itself. `span/4` makes its span the active one for the
  process while the block runs, so a child started anywhere inside, including in
  a function three calls down, finds its parent without being handed one.
  """

  alias Mcpd.{Codec, Config, Context, Http, Ids, Worker}

  @doc """
  `use Mcpd` to reach the `span` macros without writing `require Mcpd` first.

  `span/3` and `span/4` are macros, because `do ... end` is how Elixir spells a
  block, and a macro has to be required before it can be called. That is a rule
  of the language rather than a choice here, so this exists to make the common
  case one line instead of two.

      defmodule MyAgent do
        use Mcpd
        def run(mcpd), do: Mcpd.span(mcpd, "agent", do: :ok)
      end
  """
  defmacro __using__(_opts) do
    quote do
      require Mcpd
    end
  end

  # The sentinel every SDK sends for `received_at`. The daemon stamps it from
  # its own clock, because a producer's clock is not the one retention is
  # measured against.
  @epoch "1970-01-01T00:00:00.000000000Z"

  # The fields `model_parameters:` carries, exactly as `Mcpd.ModelParameters`
  # names them. Anything else a caller puts in the list is ignored rather than
  # sent, because a key the daemon does not know is a key it will refuse.
  @model_parameter_fields ~w(temperature top_p top_k max_tokens seed stop_sequences has_response_format)

  # The correlation fields a caller may set per signal. The names match the
  # generated `Mcpd.Correlation` exactly, because they reach the wire unchanged
  # and a name invented here would be dropped by the daemon in silence.
  @correlation_fields [
    :project,
    :environment,
    :service_name,
    :service_version,
    :session_id,
    :thread_id,
    :user_id,
    :resource_id,
    :run_id,
    :request_id,
    :experiment_id,
    :release
  ]

  # 2^64 as a float. The sampling rule compares the low 64 bits of a trace id
  # against `sample * 2^64`, in double arithmetic, which is the rule the daemon
  # and every other SDK use. The arithmetic has to match to the last bit or two
  # languages disagree about the same trace.
  @two_to_64 18_446_744_073_709_551_616.0

  @doc """
  Start a client. Every option falls back to its environment variable.

  `Mcpd.start_link()` with no arguments works against a local daemon.
  """
  @spec start_link(keyword()) :: GenServer.on_start()
  def start_link(opts \\ []), do: opts |> Config.new() |> Worker.start_link()

  @doc """
  Run a block inside a span, and return whatever it returns.

      Mcpd.span mcpd, "plan", kind: :model_generation do
        call_the_model(question)
      end

  A macro rather than a function, because `do ... end` is how Elixir spells a
  block and a function would have forced `fn -> ... end` at every call site. The
  first version of this SDK took a function, and the example written against it
  failed immediately with `expected a function, got: [do: ...]`, which is the
  language telling you the API is not shaped like the language.

  The span ends when the block does, whether it returns or raises. A raise is
  recorded as an error and then re-raised: mcpd's job is to see what happened,
  never to decide what the caller does about it.
  """
  defmacro span(client, name, opts_and_block) do
    {block, opts} = Keyword.pop!(opts_and_block, :do)
    build(client, name, opts, block)
  end

  @doc """
  `span/3` with the options given separately from the block.

  Both arities are needed and the reason is a parsing detail worth writing down.
  Elixir collects trailing keywords into ONE list, so

      Mcpd.span mcpd, "plan" do ... end

  arrives as `span(client, name, [do: block])`, three arguments, while

      Mcpd.span mcpd, "plan", kind: :model_generation do ... end

  arrives as `span(client, name, [kind: ...], [do: block])`, four. Defining only
  the first made every call that passed an option fail with
  `function Mcpd.span/4 is undefined`, which is the shape of call every real use
  site has.
  """
  defmacro span(client, name, opts, do: block), do: build(client, name, opts, block)

  defp build(client, name, opts, block) do
    quote do
      Mcpd.with_span(unquote(client), unquote(name), unquote(opts), fn -> unquote(block) end)
    end
  end

  @doc """
  The function form of `span/3`, for when the body is already a function.

      Mcpd.with_span(mcpd, "plan", [kind: :model_generation], fn -> ... end)

  The span goes on the wire when it opens and again when it closes, contract
  behaviour B8: a run that crashes mid-flight is exactly the run you need to
  see, and a span sent only on the way out is never seen at all.
  """
  @spec with_span(pid(), String.t(), keyword(), (-> any())) :: any()
  def with_span(client, name, opts \\ [], fun) do
    span = open(client, name, opts)
    emit(client, span, :open, nil, nil)

    try do
      result = Context.with_span(span, fun)
      emit(client, span, :ok, output(result, opts), nil)
      accumulate(span)
      result
    rescue
      e ->
        # Contract behaviour B10: an error sets the status AND carries a
        # message. "Something failed" is not actionable.
        emit(client, span, :error, nil, error_of(e, __STACKTRACE__))
        accumulate(span)
        reraise e, __STACKTRACE__
    catch
      # A `throw` or an `exit` leaves the block the same way a raise does, and
      # a span left open by either would read as a run that never finished.
      kind, reason ->
        emit(client, span, :error, nil, %{
          "kind" => to_string(kind),
          "message" => Exception.format_exit(reason),
          "stack" => stack(__STACKTRACE__)
        })

        accumulate(span)
        :erlang.raise(kind, reason, __STACKTRACE__)
    end
  end

  @doc """
  Queue a span that already ended, as given.

  What an adapter for another tracing system, a batch import or a replay
  holds: a `%Mcpd.Span{}` or a map in the wire shape, with its own ids, parent,
  start and end. Nothing is re-parented, re-timed or re-sampled. Correlation
  fields the record leaves empty are filled from this client's own; fields it
  sets are kept.

  A record is refused, dropped and counted in `stats/1` under `rejected` when
  its `trace_id` is not 32 lowercase hex, its `span_id` or `parent_span_id`
  is not 16, either is all zero, its `name` is empty or its `start_time` is
  missing. Refusing never raises. `capture_payloads: false`, on the client or
  as an option here, strips `input` and `output` like any other span, and a
  disabled client drops the record like any other signal.
  """
  @spec record(pid(), Mcpd.Span.t() | map(), keyword()) :: :ok
  def record(client, span, opts \\ []) do
    config = Worker.config(client)

    if config != nil and config.enabled do
      case validate_record(Mcpd.Runs.wire(span)) do
        {:ok, map} ->
          capture = Keyword.get(opts, :capture_payloads, config.capture_payloads)
          map = map |> fill_correlation(config) |> strip_payloads(capture) |> encode_payloads()
          Worker.record(client, {map["trace_id"], map["span_id"]}, :spans, map)

        {:error, reason} ->
          Worker.reject(client, "a recorded span was refused and dropped: #{reason}")
      end
    end

    :ok
  rescue
    e -> Worker.reject(client, "a recorded span was refused and dropped: #{Exception.message(e)}")
  catch
    _kind, reason -> Worker.reject(client, "a recorded span was refused and dropped: #{inspect(reason)}")
  end

  @doc """
  What the client has done since it started: `queued`, `dropped`, `capacity`,
  `exported`, `failed`, `rejected` and `redaction_failures`. The same seven
  names in every SDK.
  """
  @spec stats(pid()) :: %{
          queued: non_neg_integer(),
          dropped: non_neg_integer(),
          capacity: non_neg_integer(),
          exported: non_neg_integer(),
          failed: non_neg_integer(),
          rejected: non_neg_integer(),
          redaction_failures: non_neg_integer()
        }
  defdelegate stats(client), to: Worker

  @doc "Record a log line against the active span."
  @spec log(pid(), atom(), String.t(), map()) :: :ok
  def log(client, level, message, fields \\ %{}) do
    # A level outside the enum is left exactly as the caller wrote it and is
    # never held back by the floor: the floor is a filter, not a validator.
    floor_check =
      case Mcpd.Logs.parse_log_level(level) do
        nil -> fn _config -> true end
        parsed -> fn config -> Mcpd.Logs.meets_level(parsed, config.log_level) end
      end

    signal(client, :logs, fn active, config ->
      %{
        "log_id" => Ids.trace_id(),
        "trace_id" => active && active.trace_id,
        "span_id" => active && active.span_id,
        "timestamp" => now_iso(),
        "level" => to_string(level),
        "message" => message,
        "fields" => (map_size(fields) > 0 && Mcpd.Logs.encode_fields(fields)) || nil,
        "received_at" => @epoch,
        "correlation" => correlation(config, [], active)
      }
    end, floor_check)
  end

  @doc "Record a metric point."
  @spec metric(pid(), String.t(), number(), keyword()) :: :ok
  def metric(client, name, value, opts \\ []) do
    signal(client, :metrics, fn active, config ->
      %{
        "metric_id" => Ids.trace_id(),
        "name" => name,
        "kind" => to_string(opts[:kind] || :gauge),
        "value" => value / 1,
        "unit" => opts[:unit],
        "timestamp" => now_iso(),
        "labels" => opts[:labels] && Mcpd.Logs.encode_fields(opts[:labels]),
        "trace_id" => active && active.trace_id,
        "span_id" => active && active.span_id,
        "received_at" => @epoch,
        "correlation" => correlation(config, [], active)
      }
    end)
  end

  @doc """
  Attach a machine evaluation to a run.

  `min` and `max` are required by the contract, not by politeness: 7 is
  excellent out of 10 and poor out of 100, and a score with no declared scale
  cannot be compared with anything.
  """
  @spec score(pid(), String.t(), String.t(), number(), keyword()) :: :ok
  def score(client, trace_id, scorer_id, value, opts \\ []) do
    signal(client, :scores, fn active, config ->
      %{
        "score_id" => Ids.trace_id(),
        "trace_id" => trace_id,
        "span_id" => opts[:span_id],
        "scorer_id" => scorer_id,
        # Live, as in every other SDK. Defaulting to offline here meant the
        # same call filtered differently depending on the language that made
        # it, which is the one thing a cross-language contract exists to stop.
        "source" => to_string(opts[:source] || :live),
        "value" => value / 1,
        "min" => (opts[:min] || 0) / 1,
        "max" => (opts[:max] || 1) / 1,
        "reason" => opts[:reason],
        "label" => opts[:label],
        "timestamp" => now_iso(),
        "correlation" => correlation(config, opts, active)
      }
    end)
  end

  @doc "Attach a person's judgement to a run."
  @spec feedback(pid(), String.t(), atom(), term(), keyword()) :: :ok
  def feedback(client, trace_id, kind, value, opts \\ []) do
    signal(client, :feedback, fn active, config ->
      %{
        "feedback_id" => Ids.trace_id(),
        "trace_id" => trace_id,
        "span_id" => opts[:span_id],
        "kind" => to_string(kind),
        # Whatever the caller judged with, in a shape `:json` is certain to
        # accept. A tuple here used to cost the whole batch it travelled in.
        "value" => Mcpd.Logs.encode_fields(value),
        "comment" => opts[:comment],
        "author_id" => opts[:author_id],
        "author_kind" => opts[:author_kind],
        "timestamp" => now_iso(),
        "correlation" => correlation(config, opts, active)
      }
    end)
  end

  @doc """
  Send everything queued and wait for it, for at most `timeout` milliseconds.

  Returns `{:error, :timeout}` when the deadline passed first, even when the
  daemon never answers; what was not sent stays queued.
  """
  @spec flush(pid(), timeout()) :: :ok | {:error, :timeout}
  defdelegate flush(client, timeout \\ 10_000), to: Worker

  @doc """
  Flush, stop the worker, and return, within `timeout` milliseconds. Safe to
  call twice. When the deadline passes, no further batch starts and what is
  still queued is given up.
  """
  @spec shutdown(pid(), timeout()) :: :ok
  defdelegate shutdown(client, timeout \\ 10_000), to: Worker

  @doc "The config a client is running with, or `nil` once it has been shut down."
  @spec config(pid()) :: Config.t() | nil
  defdelegate config(client), to: Worker

  @doc "A `traceparent` for an outgoing request, so the trace crosses the boundary."
  @spec traceparent() :: String.t() | nil
  def traceparent do
    case Context.current() do
      nil -> nil
      s -> Context.traceparent(s.trace_id, s.span_id, Map.get(s, :sampled, true))
    end
  end

  @doc """
  Continue a trace from an inbound `traceparent`.

  The caller's sampling decision is inherited with the ids: a trace some
  services keep and others drop is not a trace.
  """
  @spec continue(String.t() | nil, (-> any())) :: any()
  def continue(header, fun) do
    case Context.parse_traceparent(header) do
      {:ok, %{trace_id: t, span_id: s, sampled: sampled}} ->
        Context.with_span(
          %{trace_id: t, span_id: s, name: "remote", kind: "generic", sampled: sampled},
          fun
        )

      :error ->
        fun.()
    end
  end

  # ------------------------------------------------------------- reads -------

  @doc """
  A page of runs. Filters are the HTTP API's query parameters, as a keyword
  list: `status`, `kind`, `name`, `tags`, `session_id`, `from`, `to`, `limit`,
  `cursor` and the rest. `from` and `to` take a `DateTime` or a shorthand such
  as `"-1h"`.
  """
  @spec list_traces(pid(), keyword()) :: {:ok, Mcpd.TraceListResponse.t()} | {:error, term()}
  def list_traces(client, filters \\ []), do: read(client, "/traces", filters, Mcpd.TraceListResponse)

  @doc "Read one stored run back, with its spans. `include: [\"logs\", \"scores\", \"feedback\"]` adds the signals attached to it."
  @spec get_trace(pid(), String.t(), keyword()) :: {:ok, Mcpd.TraceDetail.t()} | {:error, term()}
  def get_trace(client, trace_id, opts \\ []) do
    filters = [project: opts[:project], include: opts[:include]]
    read(client, "/traces/" <> trace_id, filters, Mcpd.TraceDetail)
  end

  @doc "A page of steps, across runs."
  @spec list_spans(pid(), keyword()) :: {:ok, Mcpd.SpanListResponse.t()} | {:error, term()}
  def list_spans(client, filters \\ []), do: read(client, "/spans", filters, Mcpd.SpanListResponse)

  @doc "A page of log records. Filtering by level is always `>=`."
  @spec list_logs(pid(), keyword()) :: {:ok, Mcpd.LogListResponse.t()} | {:error, term()}
  def list_logs(client, filters \\ []), do: read(client, "/logs", filters, Mcpd.LogListResponse)

  @doc """
  An aggregated metric series, one line per label combination.

  `:filter` keeps points whose labels have these values, given as a map or a
  keyword list. It is sent as `key:value` pairs joined by commas, so a key
  cannot contain `:` or `,` and a value cannot contain `,`; either raises
  `ArgumentError` before a request is made.
  """
  @spec query_metrics(pid(), String.t(), term(), term(), keyword()) ::
          {:ok, Mcpd.MetricSeriesResponse.t()} | {:error, term()}
  def query_metrics(client, name, from, to, opts \\ []) do
    {filter, opts} = Keyword.pop(opts, :filter)
    filters = Keyword.merge(opts, name: name, from: from, to: to, filter: metric_filter(filter))
    read(client, "/metrics", filters, Mcpd.MetricSeriesResponse)
  end

  # Label filters the way the daemon reads them: `key:value` joined by commas.
  # A map here used to raise from `to_string/1`, and JSON would match no point
  # and answer an empty series. A key or value the format cannot carry is
  # refused rather than sent, because the daemon would split it into a
  # different filter and answer a different question.
  defp metric_filter(nil), do: nil
  defp metric_filter(filter) when filter == %{} or filter == [], do: nil

  defp metric_filter(filter) when is_map(filter) or is_list(filter) do
    Enum.map_join(filter, ",", fn {key, value} ->
      key = to_string(key)
      value = if is_atom(value), do: Atom.to_string(value), else: to_string(value)

      if key == "" or String.contains?(key, [":", ","]) do
        raise ArgumentError, "metric filter key #{inspect(key)} cannot be empty or contain ':' or ','"
      end

      if String.contains?(value, ",") do
        raise ArgumentError, "metric filter value #{inspect(value)} for #{inspect(key)} cannot contain ','"
      end

      key <> ":" <> value
    end)
  end

  @doc "A page of machine evaluations."
  @spec list_scores(pid(), keyword()) :: {:ok, Mcpd.ScoreListResponse.t()} | {:error, term()}
  def list_scores(client, filters \\ []), do: read(client, "/scores", filters, Mcpd.ScoreListResponse)

  @doc """
  The human judgements attached to one run.

  The daemon reads feedback one run at a time and refuses any other filter
  with a 400, so the trace id is required and the options are `:limit` and
  `:project` only. Any other option raises `ArgumentError` before a request
  is made.
  """
  @spec list_feedback(pid(), String.t(), keyword()) ::
          {:ok, Mcpd.FeedbackListResponse.t()} | {:error, term()}
  def list_feedback(client, trace_id, opts \\ [])

  def list_feedback(client, trace_id, opts) when is_binary(trace_id) and trace_id != "" do
    opts = Keyword.validate!(opts, [:limit, :project])
    read(client, "/feedback", [{:trace_id, trace_id} | opts], Mcpd.FeedbackListResponse)
  end

  def list_feedback(_client, trace_id, _opts) do
    raise ArgumentError,
          "list_feedback needs a trace id as a non-empty string, got #{inspect(trace_id)}: " <>
            "the daemon reads feedback one run at a time"
  end

  @doc "Headline numbers for a window: runs, errors, latency, tokens, spend."
  @spec analytics_summary(pid(), term(), term(), keyword()) ::
          {:ok, Mcpd.AnalyticsSummary.t()} | {:error, term()}
  def analytics_summary(client, from, to, opts \\ []) do
    filters = Keyword.merge(opts, from: from, to: to)
    read(client, "/analytics/summary", filters, Mcpd.AnalyticsSummary)
  end

  @doc "Where the money went, optionally bucketed into a time series."
  @spec analytics_cost(pid(), term(), term(), keyword()) ::
          {:ok, Mcpd.CostBreakdown.t()} | {:error, term()}
  def analytics_cost(client, from, to, opts \\ []) do
    filters = Keyword.merge(opts, from: from, to: to)
    read(client, "/analytics/cost", filters, Mcpd.CostBreakdown)
  end

  @doc """
  Whether the daemon is alive, and which spec version it implements.

  Compare `spec_version` with `Mcpd.Spec.spec_version/0` at connect time. A
  mismatch caught there names the problem; the same mismatch found later shows
  up as a field missing for no visible reason.
  """
  # ------------------------------------------------------------- runs -------

  # The run surface, in full in `Mcpd.Runs`; every call is reachable from here
  # so a caller needs one module.
  defdelegate put_definition(client, doc, opts \\ []), to: Mcpd.Runs
  defdelegate get_definition(client, id, opts \\ []), to: Mcpd.Runs
  defdelegate list_definitions(client, opts \\ []), to: Mcpd.Runs
  defdelegate start_run(client, definition, opts \\ []), to: Mcpd.Runs
  defdelegate get_run(client, run_id), to: Mcpd.Runs
  defdelegate list_runs(client, filters \\ []), to: Mcpd.Runs
  defdelegate cancel_run(client, run_id), to: Mcpd.Runs
  defdelegate run_events(client, run_id, opts \\ []), to: Mcpd.Runs
  defdelegate watch_run(client, run_id, opts \\ []), to: Mcpd.Runs
  defdelegate wait_run(client, run_id, opts \\ []), to: Mcpd.Runs
  defdelegate start_run_and_wait(client, definition, opts \\ []), to: Mcpd.Runs
  defdelegate signal_run(client, run_id, event, opts \\ []), to: Mcpd.Runs
  defdelegate publish_event(client, event, key, opts \\ []), to: Mcpd.Runs
  defdelegate register_worker(client, worker_id, definitions, opts \\ []), to: Mcpd.Runs
  defdelegate claim_steps(client, worker_id, opts \\ []), to: Mcpd.Runs
  defdelegate step_started(client, lease_id), to: Mcpd.Runs
  defdelegate step_heartbeat(client, lease_id), to: Mcpd.Runs
  defdelegate complete_step(client, lease_id, opts \\ []), to: Mcpd.Runs
  defdelegate fail_step(client, lease_id, error), to: Mcpd.Runs

  @spec health(pid()) :: {:ok, Mcpd.HealthStatus.t()} | {:error, term()}
  def health(client) do
    case Worker.config(client) do
      nil -> {:error, :no_client}
      config -> fetch(config, String.trim_trailing(config.endpoint, "/") <> "/healthz", Mcpd.HealthStatus)
    end
  end

  @doc false
  # Exposed for the test that pins the wire timestamp format. The daemon rejects
  # anything that is not RFC 3339 in UTC, and that rejection has already been
  # caused once by a formatter bug, so the format is worth a test of its own.
  def __ts_for_test__, do: iso(System.system_time(:nanosecond))

  @doc false
  # The sampling rule, exposed so the test suite can pin it against the other
  # SDKs' vectors.
  def __sampled_for_test__(sample, trace_id), do: sampled?(sample, trace_id)

  # ------------------------------------------------------------- internals ---

  # A trace id is only unique within its project, which is the tenancy key.
  # Leaving it off reads the server's default project and 404s on everything
  # this client wrote, so the client's project is the default for every read.
  defp read(client, path, filters, module) do
    case Worker.config(client) do
      nil ->
        {:error, :no_client}

      config ->
        query =
          filters
          |> Enum.reject(fn {_k, v} -> v == nil end)
          |> Keyword.put_new(:project, config.project)
          |> encode_query()

        fetch(config, Config.api_url(config, path <> "?" <> query), module)
    end
  end

  defp fetch(config, url, module) do
    case Http.get_json(url, api_key: config.api_key, timeout: config.timeout) do
      {:ok, body} -> {:ok, Codec.decode(module, body)}
      {:error, _} = failure -> failure
    end
  end

  # Unset filters are left off, a `DateTime` goes out as RFC 3339, and a list
  # is one comma-separated value, which is how the daemon reads `tags` and
  # `include`.
  defp encode_query(filters) do
    filters
    |> Enum.reject(fn {_k, v} -> v == nil end)
    |> Enum.map(fn {k, v} -> {Atom.to_string(k), param(v)} end)
    |> URI.encode_query()
  end

  defp param(%DateTime{} = at), do: DateTime.to_iso8601(at)
  defp param(list) when is_list(list), do: Enum.map_join(list, ",", &param/1)
  defp param(atom) when is_atom(atom), do: Atom.to_string(atom)
  defp param(other), do: to_string(other)

  # Build a span. Contract behaviour B4: the sampling decision is made once,
  # at the root, from the trace id, and every child inherits it. A child never
  # re-decides, and a trace continued from a `traceparent` inherits the
  # caller's decision.
  defp open(client, name, opts) do
    config = Worker.config(client)
    parent = Context.current()

    {trace_id, parent_span_id, sampled} =
      case parent do
        %{trace_id: t, span_id: s} = p ->
          {t, s, Map.get(p, :sampled, true)}

        nil ->
          t = opts[:trace_id] || Ids.trace_id()
          {t, nil, config != nil and sampled?(config.sample, t)}
      end

    %{
      trace_id: trace_id,
      span_id: Ids.span_id(),
      parent_span_id: parent_span_id,
      name: name,
      kind: to_string(opts[:kind] || :generic),
      started: now_iso(),
      started_at: System.monotonic_time(:nanosecond),
      sampled: sampled,
      # Whether `input` and `output` may leave the process: the span's own
      # word when it gave one, the client's otherwise (contract section 2.1).
      capture: Keyword.get(opts, :capture_payloads, config == nil or config.capture_payloads) == true,
      correlation: correlation(config, opts, parent),
      opts: opts
    }
  rescue
    # Whatever was wrong with the options, the caller's block still has to run
    # and return its value. A span that records nothing is the fallback.
    _ ->
      %{
        trace_id: Ids.trace_id(),
        span_id: Ids.span_id(),
        parent_span_id: nil,
        name: name,
        kind: "generic",
        started: now_iso(),
        started_at: System.monotonic_time(:nanosecond),
        sampled: false,
        capture: false,
        correlation: %{},
        opts: []
      }
  end

  # The low 64 bits of the trace id against the ratio. The same hash the
  # daemon and every other SDK use, so a client and a fleet of collectors keep
  # and drop the same traces without coordinating.
  defp sampled?(sample, _trace_id) when sample >= 1.0, do: true
  defp sampled?(sample, _trace_id) when sample <= 0.0, do: false

  defp sampled?(sample, trace_id) do
    case Integer.parse(String.slice(trace_id, -16, 16), 16) do
      {low, ""} -> low < trunc(sample * @two_to_64)
      _ -> true
    end
  end

  # Whether a signal recorded now should reach the queue at all.
  defp recording?(nil, _active), do: false
  defp recording?(%Config{enabled: false}, _active), do: false
  defp recording?(%Config{}, nil), do: true
  defp recording?(%Config{}, active), do: Map.get(active, :sampled, true)

  # Every recording call goes through here. Contract behaviour B1: nothing on
  # this path may reach the caller, so a payload that cannot be built is
  # reported through the worker's throttle and dropped.
  defp signal(client, kind, build, allow \\ fn _config -> true end) do
    config = Worker.config(client)
    active = Context.current()

    if allow.(config) and recording?(config, active) do
      Worker.record(client, make_ref(), kind, drop_nils(build.(active, config)))
    end

    :ok
  rescue
    e -> Worker.warn(client, "#{kind} could not be recorded and was dropped: #{Exception.message(e)}")
  catch
    _kind, reason -> Worker.warn(client, "#{kind} could not be recorded and was dropped: #{inspect(reason)}")
  end

  defp emit(client, span, phase, output, error) do
    config = Worker.config(client)

    if recording?(config, span) do
      Worker.record(client, {span.trace_id, span.span_id}, :spans, span_record(span, phase, output, error))
    end

    :ok
  rescue
    e -> Worker.warn(client, "span could not be recorded and was dropped: #{Exception.message(e)}")
  catch
    _kind, reason -> Worker.warn(client, "span could not be recorded and was dropped: #{inspect(reason)}")
  end

  # The record for a span as it stands: open, with no end and no duration, or
  # closed. Both go out under the same key, so a close that arrives before the
  # open was sent replaces it in the queue rather than following it.
  defp span_record(span, phase, output, error) do
    o = span.opts
    closed = phase != :open

    drop_nils(%{
      "trace_id" => span.trace_id,
      "span_id" => span.span_id,
      "parent_span_id" => span.parent_span_id,
      "name" => span.name,
      "kind" => span.kind,
      "status" => if(closed, do: to_string(phase), else: "unset"),
      "start_time" => span.started,
      "end_time" => if(closed, do: now_iso()),
      "duration_ns" => if(closed, do: System.monotonic_time(:nanosecond) - span.started_at),
      "is_root" => span.parent_span_id == nil,
      "is_event" => false,
      # The epoch is the sentinel every SDK sends: the daemon stamps
      # `received_at` from its own clock, because a producer's clock is not
      # the one the retention window is measured against.
      "received_at" => @epoch,
      "external_parent_span_id" => o[:external_parent_span_id],
      "input" => if(span.capture, do: o[:input]),
      "output" => if(span.capture, do: output),
      "error" => error,
      "model" => o[:model],
      "provider" => o[:provider],
      "usage" => o[:usage],
      "model_parameters" => model_parameters(o[:model_parameters]),
      "attributes" => attributes(o),
      "metadata" => o[:metadata],
      "tags" => if(span.parent_span_id == nil, do: o[:tags]),
      "correlation" => span.correlation
    })
    |> encode_payloads()
  end

  # The values a span carries for the caller, in a shape `:json` is certain to
  # accept, the way `Mcpd.Logs.encode_fields/1` already treats a log line's
  # fields. A body that returns `{:ok, value}` is captured as its output, and
  # `:json.encode/1` raises on a tuple: the sender then lost the whole batch,
  # every span, metric, score and piece of feedback that shared it (B1). One
  # value with no JSON form now costs itself, written as its `inspect/1`.
  defp encode_payloads(map) do
    map
    |> encode_key("input", &Mcpd.Logs.encode_fields/1)
    |> encode_key("output", &Mcpd.Logs.encode_fields/1)
    |> encode_key("metadata", &Mcpd.Logs.encode_fields/1)
    |> encode_key("attributes", &Mcpd.Logs.encode_fields/1)
    |> encode_key("tags", &Mcpd.Logs.encode_fields/1)
    |> encode_key("usage", &encode_usage/1)
  end

  defp encode_key(map, key, encode) do
    case map do
      %{^key => value} when value != nil -> Map.put(map, key, encode.(value))
      _ -> map
    end
  end

  # Usage is a map, a `%Mcpd.TokenUsage{}` or a keyword list. Unset fields of
  # the struct are left off rather than sent as nulls.
  defp encode_usage(usage) when is_list(usage) do
    if Keyword.keyword?(usage), do: usage |> Map.new() |> encode_usage(), else: Mcpd.Logs.encode_fields(usage)
  end

  defp encode_usage(usage), do: usage |> Mcpd.Runs.wire() |> Mcpd.Logs.encode_fields()

  # How the model was called, in the shape the daemon reads.
  #
  # An Elixir span is described by the options it was opened with, the same way
  # `model:`, `provider:` and `usage:` are, so `model_parameters:` is where a
  # caller says what settings the call was made with. A keyword list is what
  # you write; a `%Mcpd.ModelParameters{}` is accepted too, because an adapter
  # for another tracing system usually holds one already.
  #
  # Nothing here carries prompt or completion text, so these settings are
  # recorded whether or not payload capture is on: they are what a run has to
  # be reproduced from, and dropping them with the prompt would leave a run
  # nobody can repeat.
  #
  # A field left out is absent. A field given is kept whatever its value is:
  # only `nil` means "not set", so `seed: 0` reaches the wire as a seed and
  # `temperature: 0.0` as a temperature. `has_response_format` is always
  # written, because the spec declares it required with a default of false.
  defp model_parameters(nil), do: nil

  defp model_parameters(%Mcpd.ModelParameters{} = params),
    do: params |> Map.from_struct() |> model_parameters()

  defp model_parameters(fields) when is_list(fields) or is_map(fields) do
    fields
    |> Enum.reduce(%{}, fn
      # A key of any other shape, and anything in the list that is not a pair
      # at all, is ignored. A typo in an option list must cost a missing field,
      # never a raise in the middle of somebody's run.
      {key, value}, acc when is_atom(key) or is_binary(key) ->
        name = to_string(key)
        if name in @model_parameter_fields, do: Map.put(acc, name, value), else: acc

      _other, acc ->
        acc
    end)
    |> Map.update("has_response_format", false, &(&1 == true))
    # `Mcpd.ModelParameters` defaults `stop_sequences` to `[]`, so a struct
    # built without one cannot say "unset" the way the other five SDKs do. An
    # empty list carries no more than an absent one here, so it is dropped and
    # the same intent produces the same bytes in every language.
    |> Map.update("stop_sequences", nil, fn list -> if list == [], do: nil, else: list end)
    |> drop_nils()
  end

  # Anything else, including a caller who passed a number or a string by
  # mistake, records nothing rather than raising in the middle of their run.
  defp model_parameters(_other), do: nil

  # Kind-specific detail, such as what `Mcpd.ModelCall` reads off a response.
  # Every `attributes:` in the options is merged, later ones winning, so
  # `[attributes: Mcpd.ModelCall.guardrail(true)] ++ Mcpd.ModelCall.options(r)`
  # keeps both rather than the first silently hiding the second. A value that
  # is not a map is ignored: a mistake in the options costs the field, never a
  # raise in the caller's run.
  defp attributes(opts) do
    merged =
      opts
      |> Keyword.get_values(:attributes)
      |> Enum.reduce(%{}, fn
        %{} = attrs, acc -> Map.merge(acc, attrs)
        _other, acc -> acc
      end)

    if map_size(merged) > 0, do: merged
  end

  # Contract behaviour B10 on the wire: the class under `kind`, which is the
  # field the spec names, and a message that is never blank. An exception
  # raised with an empty message is still an exception of some class, and the
  # class is what a reader can act on.
  defp error_of(e, stacktrace) do
    kind = inspect(e.__struct__)
    message = Exception.message(e)

    %{
      "kind" => kind,
      "message" => if(String.trim(message) == "", do: kind, else: message),
      "stack" => stack(stacktrace)
    }
  end

  defp stack(stacktrace), do: stacktrace |> Exception.format_stacktrace() |> String.slice(0, 4_000)

  # A span closing inside a step body hands its usage to the step (contract
  # section 7.3). The step context leaves its store in the process dictionary
  # for the body's duration; a span closing anywhere else finds nothing there
  # and does nothing. Arithmetic on values already recorded, so nothing here
  # can reach the caller.
  defp accumulate(span) do
    case Process.get(:mcpd_usage_sink) do
      pid when is_pid(pid) ->
        o = span.opts
        Mcpd.StepContext.__accumulate__(pid, o[:usage], o[:model], o[:provider])

      _ ->
        :ok
    end
  rescue
    _ -> :ok
  catch
    _, _ -> :ok
  end

  # The shape `record/3` refuses (contract section 3), each with a reason a
  # reader can act on. Everything else on the record is the caller's.
  defp validate_record(map) when is_map(map) do
    parent = map["parent_span_id"]

    cond do
      not Ids.valid?(map["trace_id"], 16) -> {:error, "trace_id is not 32 lowercase hex, or is all zero"}
      not Ids.valid?(map["span_id"], 8) -> {:error, "span_id is not 16 lowercase hex, or is all zero"}
      parent != nil and not Ids.valid?(parent, 8) -> {:error, "parent_span_id is not 16 lowercase hex, or is all zero"}
      not (is_binary(map["name"]) and map["name"] != "") -> {:error, "name is empty"}
      not is_binary(map["start_time"]) -> {:error, "start_time is missing"}
      true -> {:ok, defaults(map)}
    end
  end

  defp validate_record(_other), do: {:error, "a span record is a map or a %Mcpd.Span{}"}

  # The fields the daemon requires on every span, filled only when the record
  # left them out. A record is otherwise queued as given.
  defp defaults(map) do
    map
    |> Map.put_new("kind", "generic")
    |> Map.put_new("status", "unset")
    |> Map.put_new("is_root", map["parent_span_id"] == nil)
    |> Map.put_new("is_event", false)
    |> Map.put_new("received_at", @epoch)
  end

  defp fill_correlation(map, config) do
    own = map["correlation"] || %{}
    Map.put(map, "correlation", Map.merge(deployment(config), own))
  end

  defp strip_payloads(map, true), do: map
  defp strip_payloads(map, _off), do: Map.drop(map, ["input", "output"])

  # A block's return value is only recorded as the span's output when the caller
  # asked for it. Capturing every return by default would put whatever a
  # function happened to return into storage, which is how a secret ends up in a
  # trace nobody meant to put it in.
  defp output(result, opts), do: if(opts[:capture_output], do: result, else: opts[:output])

  defp drop_nils(map), do: :maps.filter(fn _k, v -> v != nil end, map)

  defp now_iso, do: iso(System.system_time(:nanosecond))

  # RFC 3339 in UTC, nanoseconds preserved. Contract behaviour B9: a local
  # timestamp makes a distributed trace unreadable.
  defp iso(ns) do
    micro = div(ns, 1_000)
    rem_ns = rem(ns, 1_000)

    # `from_unix!/2` at microsecond precision always renders six fractional
    # digits, so appending the remaining three gives nanoseconds. Done with
    # `replace_suffix` rather than a regex with a replacement function: that
    # function is handed the WHOLE match including the `Z`, so appending inside
    # it produced `...637634Z764Z`, which is not a timestamp.
    base = micro |> DateTime.from_unix!(:microsecond) |> DateTime.to_iso8601()
    String.replace_suffix(base, "Z", pad(rem_ns) <> "Z")
  end

  defp pad(n), do: n |> Integer.to_string() |> String.pad_leading(3, "0")

  # Client defaults, then the parent's context, then this call's own options.
  #
  # A child reads its base from its parent rather than from the client, which is
  # what carries the run's correlation down the whole tree. Building every span
  # from the client instead is why session, thread and user used to stop at the
  # root: those fields belong to one run, and the client never hears about them.
  defp correlation(config, opts, parent) do
    base =
      case parent do
        # A span started by `continue/2` from a `traceparent` has no correlation
        # of its own, so match on the key rather than assume it is there.
        %{correlation: %{} = inherited} -> inherited
        _ -> deployment(config)
      end

    Enum.reduce(@correlation_fields, base, fn field, acc ->
      case opts[field] do
        nil -> acc
        value -> Map.put(acc, Atom.to_string(field), value)
      end
    end)
  end

  # What the deployment is, as opposed to what one run is. These four come from
  # the config, so every signal carries them whether or not a span is active.
  defp deployment(nil), do: %{}

  defp deployment(%Config{} = config) do
    drop_nils(%{
      "project" => config.project,
      "environment" => config.environment,
      "service_name" => config.service_name,
      "service_version" => config.service_version
    })
  end
end
