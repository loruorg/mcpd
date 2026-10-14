defmodule Mcpd.Experiments do
  @moduledoc """
  Experiments (contract section 5.2): run one version of your agent over every
  case of a dataset, and score each answer against its own case.

      {:ok, results} =
        Mcpd.run_experiment(mcpd, "support-questions", &MyAgent.answer/1,
          name: "answers-v2",
          scorers: ["exact_match"]
        )

  The daemon never calls the agent. This module does, in your node, once per
  case inside a root span that names the experiment and the case, and then asks
  the daemon to score what was recorded.

  Every function here is also reachable through `Mcpd`. They answer
  `{:ok, value}` or `{:error, reason}`, like the read API, and raise
  `ArgumentError` only for a call that could never be right, before any
  request is made.
  """

  alias Mcpd.{Codec, Config, Http, Worker}

  # The attribute the daemon reads to find the case a run answered.
  @item_attribute "mcpd.dataset_item_id"

  # The most cases the daemon answers in one page.
  @max_cases 5_000

  @default_concurrency 4

  @type client :: pid()

  @doc "A run as the tool calls, model generations and handoffs its agent took."
  @spec get_trajectory(client(), String.t()) :: {:ok, Mcpd.Trajectory.t()} | {:error, term()}
  def get_trajectory(client, trace_id) do
    check_id!(trace_id, "get_trajectory", "a trace id")

    with_config(client, fn config ->
      url = url(config, "/traces/" <> segment(trace_id) <> "/trajectory", project: config.project)
      decode(Http.get_json(url, http_opts(config)), Mcpd.Trajectory)
    end)
  end

  @doc """
  The cases of a dataset. `limit:` is the one option, because it is the one
  parameter the endpoint documents: the daemon finds the project from the
  dataset.
  """
  @spec list_dataset_items(client(), String.t(), keyword()) ::
          {:ok, [Mcpd.DatasetItem.t()]} | {:error, term()}
  def list_dataset_items(client, dataset_id, opts \\ []) do
    check_id!(dataset_id, "list_dataset_items", "a dataset id")
    opts = Keyword.validate!(opts, [:limit])

    with_config(client, fn config ->
      url = url(config, "/datasets/" <> segment(dataset_id) <> "/items", limit: opts[:limit])

      case Http.get_json(url, http_opts(config)) do
        {:ok, items} when is_list(items) ->
          {:ok, Enum.map(items, &Codec.decode(Mcpd.DatasetItem, &1))}

        {:ok, other} ->
          {:error, {:decode, other}}

        {:error, _} = failure ->
          failure
      end
    end)
  end

  @doc """
  Record an experiment, or replace one, and return it as stored.

  `experiment` is a `%Mcpd.Experiment{}`, a map or a keyword list. The daemon
  upserts on `experiment_id`: an empty id gets a new one, and posting the same
  id again replaces the record, which is how a status is moved. The client's
  project is used when the correlation names none.
  """
  @spec create_experiment(client(), Mcpd.Experiment.t() | map() | keyword()) ::
          {:ok, Mcpd.Experiment.t()} | {:error, term()}
  def create_experiment(client, experiment) do
    with_config(client, fn config ->
      body = experiment_body(experiment, config.project)

      decode(
        Http.post_json(url(config, "/experiments", []), body, http_opts(config)),
        Mcpd.Experiment
      )
    end)
  end

  @doc """
  Score every run of an experiment, each against the expected answer of the
  dataset case it names.

  `scorers` are the scorer ids `POST /v1/api/score` takes. With none, the
  daemon runs the code scorers that need no settings. `config` is a
  `%Mcpd.ScorerConfig{}`, a map or a keyword list, and is left off the request
  when not given.
  """
  @spec score_experiment(
          client(),
          String.t(),
          [String.t()],
          Mcpd.ScorerConfig.t() | map() | keyword() | nil
        ) ::
          {:ok, Mcpd.ScoreRunResult.t()} | {:error, term()}
  def score_experiment(client, experiment_id, scorers \\ [], config \\ nil) do
    check_id!(experiment_id, "score_experiment", "an experiment id")

    unless is_list(scorers) and Enum.all?(scorers, &is_binary/1) do
      raise ArgumentError,
            "score_experiment takes scorer ids as a list of strings, got #{inspect(scorers)}"
    end

    with_config(client, fn c ->
      body = %{"scorers" => scorers}
      body = if config == nil, do: body, else: Map.put(body, "config", scorer_config(config))
      path = "/experiments/" <> segment(experiment_id) <> "/score"

      decode(
        Http.post_json(url(c, path, project: c.project), body, http_opts(c)),
        Mcpd.ScoreRunResult
      )
    end)
  end

  @doc "An experiment case by case, with a summary per scorer."
  @spec experiment_results(client(), String.t()) ::
          {:ok, Mcpd.ExperimentResults.t()} | {:error, term()}
  def experiment_results(client, experiment_id) do
    check_id!(experiment_id, "experiment_results", "an experiment id")

    with_config(client, fn config ->
      path = "/experiments/" <> segment(experiment_id) <> "/results"

      decode(
        Http.get_json(url(config, path, project: config.project), http_opts(config)),
        Mcpd.ExperimentResults
      )
    end)
  end

  @doc """
  Run `target` over every case of a dataset, score the answers and return the
  results.

  `target` is a one-argument function, called with each case's `input`; what
  it returns is the answer. Each case runs in a task of its own, inside a root
  span of kind `agent_run` named after the experiment, carrying the
  experiment id and the case id, so a span the target opens is a child of its
  case. The case span is kept whatever the sample rate. A target that raises,
  throws or exits fails its own run, and the other cases still run.

  Then the runs are flushed and scored, the experiment is marked `succeeded`
  with each scorer's mean as its summary, and the results are returned. When
  a step of the runner itself fails, the experiment is marked `failed` with
  the message and the step's error is returned.

  Options:

    * `:name`, the experiment's name and its runs' span name. The dataset id
      by default.
    * `:scorers`, scorer ids. Empty by default, which runs the code scorers
      that need no settings.
    * `:config`, settings for the scorers, as `score_experiment/4` takes them.
    * `:concurrency`, how many cases run at once. 4 by default, at least 1.
    * `:metadata`, anything to keep with the experiment, such as the prompt
      version under test.

  A disabled client and a concurrency below 1 raise `ArgumentError` before any
  request, because none of the runs would reach the daemon to be scored.
  """
  @spec run_experiment(client(), String.t(), (term() -> term()), keyword()) ::
          {:ok, Mcpd.ExperimentResults.t()} | {:error, term()}
  def run_experiment(client, dataset_id, target, opts \\ []) do
    opts = Keyword.validate!(opts, [:name, :scorers, :config, :concurrency, :metadata])
    check_id!(dataset_id, "run_experiment", "a dataset id")
    concurrency = Keyword.get(opts, :concurrency, @default_concurrency)

    unless is_function(target, 1) do
      raise ArgumentError,
            "run_experiment needs a one-argument function as its target, got #{inspect(target)}"
    end

    unless is_integer(concurrency) and concurrency >= 1 do
      raise ArgumentError,
            "run_experiment concurrency must be a whole number of at least 1, got #{inspect(concurrency)}"
    end

    config = Worker.config(client)

    unless config != nil and config.enabled do
      raise ArgumentError,
            "run_experiment needs a running, enabled client: no run would reach the daemon to be scored. " <>
              "Start the client with enabled: true, and check MCPD_ENABLED is not false"
    end

    draft = %Mcpd.Experiment{
      experiment_id: "",
      name: opts[:name] || dataset_id,
      dataset_id: dataset_id,
      status: :running,
      started_at: DateTime.utc_now(),
      item_count: 0,
      correlation: %Mcpd.Correlation{project: config.project},
      metadata: opts[:metadata]
    }

    case create_experiment(client, draft) do
      {:ok, experiment} -> run_created(client, experiment, target, concurrency, opts)
      {:error, _} = failure -> failure
    end
  end

  # Every step after the experiment exists. A failure in any of them marks the
  # experiment failed, so it never reads as running for ever.
  defp run_created(client, experiment, target, concurrency, opts) do
    result =
      with {:items, {:ok, items}} <-
             {:items, list_dataset_items(client, experiment.dataset_id, limit: @max_cases)},
           :ok <- run_cases(client, experiment, items, target, concurrency),
           {:flush, :ok} <- {:flush, Mcpd.flush(client)},
           {:score, {:ok, _}} <-
             {:score,
              score_experiment(
                client,
                experiment.experiment_id,
                opts[:scorers] || [],
                opts[:config]
              )},
           {:results, {:ok, results}} <-
             {:results, experiment_results(client, experiment.experiment_id)},
           {:finish, {:ok, _}} <-
             {:finish, create_experiment(client, finished(experiment, items, results))} do
        {:ok, results}
      end

    case result do
      {:ok, results} ->
        {:ok, results}

      {:flush, {:error, reason}} ->
        mark_failed(
          client,
          experiment,
          "the runs did not reach the daemon before the flush deadline, so none was scored"
        )

        {:error, {:flush, reason}}

      {step, {:error, reason}} ->
        mark_failed(client, experiment, "#{describe_step(step)} failed: #{describe(reason)}")
        {:error, reason}
    end
  rescue
    e ->
      mark_failed(client, experiment, Exception.message(e))
      reraise e, __STACKTRACE__
  end

  # A fixed pool of at most `concurrency` tasks pulls the next case, however
  # long the dataset is. The tasks are not linked to the caller, so a target
  # that takes its own process down costs that case and nothing else.
  defp run_cases(client, experiment, items, target, concurrency) do
    {:ok, supervisor} = Task.Supervisor.start_link()

    try do
      supervisor
      |> Task.Supervisor.async_stream_nolink(
        items,
        fn item -> run_case(client, experiment, item, target) end,
        max_concurrency: concurrency,
        ordered: false,
        timeout: :infinity
      )
      |> Enum.each(fn
        {:ok, _} ->
          :ok

        {:exit, reason} ->
          Worker.warn(client, "an experiment case ended with its process: #{inspect(reason)}")
      end)
    after
      Supervisor.stop(supervisor)
    end
  end

  # One case: a root span around the target. A fresh task starts with no
  # active span, so the case span is a root and the target's spans, opened in
  # this same process, are its children.
  defp run_case(client, experiment, item, target) do
    opts = [
      kind: :agent_run,
      # A sampled-out case would be a case with no answer.
      sampled: true,
      input: item.input,
      capture_output: true,
      attributes: %{@item_attribute => item.item_id},
      experiment_id: experiment.experiment_id
    ]

    Mcpd.with_span(client, experiment.name, opts, fn -> target.(item.input) end)
    :ok
  rescue
    # Recorded on the span by `with_span/4`; the next case still runs.
    _ -> :ok
  catch
    _, _ -> :ok
  end

  defp finished(experiment, items, results) do
    summary = Map.new(results.scorers || [], fn scorer -> {scorer.scorer_id, scorer.mean} end)

    %{
      experiment
      | status: :succeeded,
        finished_at: DateTime.utc_now(),
        item_count: length(items),
        summary: summary
    }
  end

  # The original error is what the caller gets back. Failing to record the
  # `failed` status is reported and never replaces it.
  defp mark_failed(client, experiment, message) do
    failed = %{experiment | status: :failed, finished_at: DateTime.utc_now(), error: message}

    case create_experiment(client, failed) do
      {:ok, _} ->
        :ok

      {:error, reason} ->
        Worker.warn(
          client,
          "run_experiment could not record the failed status: #{describe(reason)}"
        )
    end
  rescue
    e ->
      Worker.warn(
        client,
        "run_experiment could not record the failed status: #{Exception.message(e)}"
      )
  end

  defp describe_step(:items), do: "reading the dataset's cases"
  defp describe_step(:score), do: "scoring the experiment"
  defp describe_step(:results), do: "reading the experiment's results"
  defp describe_step(:finish), do: "recording the succeeded status"

  defp describe({:http, status, body, _retry_after}), do: "the daemon answered #{status}: #{body}"
  defp describe(%{__exception__: true} = e), do: Exception.message(e)
  defp describe(reason), do: inspect(reason)

  # ------------------------------------------------------------- wire -------

  # The struct's own fields go through `Mcpd.Runs.wire/1`, which drops unset
  # ones. `metadata` and `summary` are the caller's JSON and go out as given:
  # a `nil` inside them is a value, not an unset field.
  defp experiment_body(experiment, project) do
    fields = fields(experiment)
    {metadata, fields} = pop_either(fields, :metadata)
    {summary, fields} = pop_either(fields, :summary)
    {correlation, fields} = pop_either(fields, :correlation)

    correlation = correlation |> fields() |> Mcpd.Runs.wire()

    correlation =
      if blank?(correlation["project"]),
        do: Map.put(correlation, "project", project),
        else: correlation

    # The daemon requires both. An id left out is a new experiment, as an
    # empty one is, and a new experiment has attempted no cases yet.
    fields
    |> Mcpd.Runs.wire()
    |> Map.put_new("experiment_id", "")
    |> Map.put_new("item_count", 0)
    |> Map.put("correlation", correlation)
    |> put_raw("metadata", metadata)
    |> put_raw("summary", summary)
  end

  # `json_schema` is a JSON Schema document, the caller's own shape.
  defp scorer_config(config) do
    {schema, fields} = config |> fields() |> pop_either(:json_schema)
    fields |> Mcpd.Runs.wire() |> put_raw("json_schema", schema)
  end

  defp fields(nil), do: %{}
  defp fields(%{__struct__: _} = struct), do: Map.from_struct(struct)
  defp fields(list) when is_list(list), do: Map.new(list)
  defp fields(%{} = map), do: map

  defp pop_either(map, key) do
    {atom, map} = Map.pop(map, key)
    {string, map} = Map.pop(map, Atom.to_string(key))
    {if(atom == nil, do: string, else: atom), map}
  end

  defp put_raw(map, _key, nil), do: map
  defp put_raw(map, key, value), do: Map.put(map, key, nulls(value))

  # `:json` writes `nil` as the string "nil" and only `:null` as null, so a
  # `nil` the caller wrote inside their JSON is turned into the null they meant.
  defp nulls(nil), do: :null
  defp nulls(%{__struct__: _} = struct), do: struct
  defp nulls(%{} = map), do: Map.new(map, fn {k, v} -> {k, nulls(v)} end)
  defp nulls(list) when is_list(list), do: Enum.map(list, &nulls/1)
  defp nulls(other), do: other

  defp blank?(value), do: value == nil or value == ""

  defp decode({:ok, %{} = body}, module), do: {:ok, Codec.decode(module, body)}
  defp decode({:ok, other}, _module), do: {:error, {:decode, other}}
  defp decode({:error, _} = failure, _module), do: failure

  defp url(config, path, query) do
    pairs = for {k, v} <- query, v != nil, do: {Atom.to_string(k), to_string(v)}
    suffix = if pairs == [], do: "", else: "?" <> URI.encode_query(pairs)
    Config.api_url(config, path <> suffix)
  end

  # An id may hold `/`, which would otherwise read as another path segment.
  defp segment(text), do: URI.encode(text, &URI.char_unreserved?/1)

  defp check_id!(id, call, what) do
    unless is_binary(id) and id != "" do
      raise ArgumentError, "#{call} needs #{what} as a non-empty string, got #{inspect(id)}"
    end
  end

  defp with_config(client, fun) do
    case Worker.config(client) do
      nil -> {:error, :no_client}
      config -> fun.(config)
    end
  end

  defp http_opts(config), do: [api_key: config.api_key, timeout: config.timeout]
end
