defmodule Mcpd.RunResult do
  @moduledoc "What `Mcpd.wait_run/3` answers: the run as it ended, with its output to hand."

  @type t :: %__MODULE__{
          status: Mcpd.RunStatus.t(),
          output: term(),
          cost_usd: float(),
          steps_used: non_neg_integer(),
          trace_id: String.t(),
          run: Mcpd.Run.t()
        }

  @enforce_keys [:status, :cost_usd, :steps_used, :trace_id, :run]
  defstruct [:status, :output, :cost_usd, :steps_used, :trace_id, :run]
end

defmodule Mcpd.Runs do
  @moduledoc """
  The run surface of the daemon, as a client: definitions, runs, events, and
  the worker protocol `Mcpd.StepWorker` drives. Every function here is also
  reachable through `Mcpd`.

  These calls answer `{:ok, value}` or `{:error, reason}`, like the read API. A
  worker call refused with a 409 answers `{:error, {:conflict, %Mcpd.StepConflict{}}}`
  carrying the daemon's closed reason, which is what lets a worker tell a fence
  from its own landed call.
  """

  alias Mcpd.{Codec, Config, Http, Worker, Workflow}

  @type client :: pid()
  @type result(t) :: {:ok, t} | {:error, term()}

  @doc "Store one version of a definition, and read the version back. Accepts a `Mcpd.Workflow` or a `Mcpd.DefinitionDocument`."
  @spec put_definition(client(), Workflow.t() | Mcpd.DefinitionDocument.t(), keyword()) ::
          result(Mcpd.DefinitionVersion.t())
  def put_definition(client, doc, opts \\ []) do
    doc = if match?(%Workflow{}, doc), do: Workflow.document(doc), else: doc

    with_config(client, fn config ->
      post(config, "/definitions?" <> query(project: opts[:project] || config.project), doc, Mcpd.DefinitionVersion)
    end)
  end

  @doc "One definition: the latest version, unless `version:` names one."
  @spec get_definition(client(), String.t(), keyword()) :: result(Mcpd.Definition.t())
  def get_definition(client, id, opts \\ []) do
    with_config(client, fn config ->
      get(config, "/definitions/#{id}?" <> query(project: opts[:project] || config.project, version: opts[:version]), Mcpd.Definition)
    end)
  end

  @doc "Every stored version in the project, or every version of one id (`id:`)."
  @spec list_definitions(client(), keyword()) :: result([Mcpd.Definition.t()])
  def list_definitions(client, opts \\ []) do
    with_config(client, fn config ->
      case Http.get_json(Config.api_url(config, "/definitions?" <> query(project: opts[:project] || config.project, id: opts[:id])), http_opts(config)) do
        {:ok, list} when is_list(list) -> {:ok, Enum.map(list, &Codec.decode(Mcpd.Definition, &1))}
        {:ok, other} -> {:error, {:decode, other}}
        failure -> failure
      end
    end)
  end

  @doc """
  Start a run. The response is the run as stored, with its trace id. `definition`
  is an id or a `Mcpd.Workflow`.

  Options: `input`, `version`, `correlation` (a map of the correlation fields),
  `idempotency_key` (unique per project while held: a retry of this call
  returns the same run) and `project`.
  """
  @spec start_run(client(), String.t() | Workflow.t(), keyword()) :: result(Mcpd.Run.t())
  def start_run(client, definition, opts \\ []) do
    id = if match?(%Workflow{}, definition), do: definition.id, else: definition

    with_config(client, fn config ->
      project = opts[:project] || config.project

      body =
        %{"project" => project, "definition_id" => id}
        |> put_if("version", opts[:version])
        |> put_if("input", opts[:input])
        |> put_if("correlation", opts[:correlation] && Map.merge(%{"project" => project}, stringify(opts[:correlation])))
        |> put_if("idempotency_key", opts[:idempotency_key])

      post(config, "/runs", body, Mcpd.Run)
    end)
  end

  @doc "One run and everything durable about it: its journal, deadlines and waits."
  @spec get_run(client(), String.t()) :: result(Mcpd.RunDetail.t())
  def get_run(client, run_id), do: with_config(client, &get(&1, "/runs/#{run_id}", Mcpd.RunDetail))

  @doc "Runs in the project, most recently updated first. Filters: `status`, `definition_id`, `limit`, `cursor`, `project`."
  @spec list_runs(client(), keyword()) :: result(Mcpd.RunListResponse.t())
  def list_runs(client, filters \\ []) do
    with_config(client, fn config ->
      get(config, "/runs?" <> query(Keyword.put_new(filters, :project, config.project)), Mcpd.RunListResponse)
    end)
  end

  @doc "End a run and every child run under it. Cancelling an ended run changes nothing."
  @spec cancel_run(client(), String.t()) :: result(Mcpd.Run.t())
  def cancel_run(client, run_id), do: with_config(client, &post(&1, "/runs/#{run_id}/cancel", %{}, Mcpd.Run))

  @doc "A page of the run's journal, from `after:` exclusive (default -1, the beginning)."
  @spec run_events(client(), String.t(), keyword()) :: result(Mcpd.RunEventPage.t())
  def run_events(client, run_id, opts \\ []) do
    with_config(client, fn config ->
      get(config, "/runs/#{run_id}/events?" <> query(after: opts[:after] || -1, limit: opts[:limit]), Mcpd.RunEventPage)
    end)
  end

  @doc """
  Every event of a run as it happens, from `after:` exclusive, until the run
  ends, as a stream. A client that stops and starts again passes the last
  `seq` it saw and misses nothing that was committed. `poll_ms` is how long to
  wait between empty pages.
  """
  @spec watch_run(client(), String.t(), keyword()) :: Enumerable.t()
  def watch_run(client, run_id, opts \\ []) do
    poll = opts[:poll_ms] || 500

    Stream.resource(
      fn -> {opts[:after] || -1, false} end,
      fn
        {_after, true} ->
          {:halt, nil}

        {after_seq, false} ->
          case run_events(client, run_id, after: after_seq, limit: 500) do
            {:ok, page} ->
              last = Enum.reduce(page.events, after_seq, fn e, acc -> max(acc, e.seq) end)
              ended = Enum.any?(page.events, &(&1.kind == :run_ended))
              if page.events == [] and not page.has_more, do: Process.sleep(poll)
              {page.events, {last, ended}}

            {:error, reason} ->
              raise Mcpd.RunError, reason: reason
          end
      end,
      fn _ -> :ok end
    )
  end

  @doc """
  Wait for a run to end and return it with its output.

  Answers `{:ok, %Mcpd.RunResult{}}` for a run that succeeded,
  `{:error, {:run_failed, %Mcpd.Run{}}}` for one that ended `:failed`,
  `:timed_out` or `:cancelled`, and `{:error, :timeout}` for one still going
  after `timeout_ms`. Options: `after` (default -1), `timeout_ms`, `poll_ms`
  (default 500).
  """
  @spec wait_run(client(), String.t(), keyword()) :: result(Mcpd.RunResult.t())
  def wait_run(client, run_id, opts \\ []) do
    poll = opts[:poll_ms] || 500
    deadline = if opts[:timeout_ms], do: System.monotonic_time(:millisecond) + opts[:timeout_ms]

    case await_end(client, run_id, opts[:after] || -1, poll, deadline) do
      :ended ->
        case get_run(client, run_id) do
          {:ok, %{run: %{status: :succeeded} = run}} ->
            {:ok,
             %Mcpd.RunResult{
               status: run.status,
               output: run.output,
               cost_usd: run.cost_usd,
               steps_used: run.steps_used,
               trace_id: run.trace_id,
               run: run
             }}

          {:ok, %{run: run}} ->
            {:error, {:run_failed, run}}

          failure ->
            failure
        end

      other ->
        other
    end
  end

  defp await_end(client, run_id, after_seq, poll, deadline) do
    case run_events(client, run_id, after: after_seq, limit: 500) do
      {:ok, page} ->
        last = Enum.reduce(page.events, after_seq, fn e, acc -> max(acc, e.seq) end)

        cond do
          Enum.any?(page.events, &(&1.kind == :run_ended)) ->
            :ended

          page.has_more ->
            await_end(client, run_id, last, poll, deadline)

          deadline != nil and System.monotonic_time(:millisecond) >= deadline ->
            {:error, :timeout}

          true ->
            wait = if deadline, do: min(poll, max(deadline - System.monotonic_time(:millisecond), 0)), else: poll
            Process.sleep(wait)
            await_end(client, run_id, last, poll, deadline)
        end

      failure ->
        failure
    end
  end

  @doc "`start_run/3`, then `wait_run/3`. `timeout_ms` and `poll_ms` go to the wait; the rest to the start."
  @spec start_run_and_wait(client(), String.t() | Workflow.t(), keyword()) :: result(Mcpd.RunResult.t())
  def start_run_and_wait(client, definition, opts \\ []) do
    {wait_opts, start_opts} = Keyword.split(opts, [:timeout_ms, :poll_ms])

    case start_run(client, definition, start_opts) do
      {:ok, run} -> wait_run(client, run.run_id, wait_opts)
      failure -> failure
    end
  end

  @doc "Deliver an event to one run through its own open wait. Options: `payload`, `event_id`."
  @spec signal_run(client(), String.t(), String.t(), keyword()) :: result(Mcpd.EventReceipt.t())
  def signal_run(client, run_id, event, opts \\ []) do
    body = %{"event" => event} |> put_if("payload", opts[:payload]) |> put_if("event_id", opts[:event_id])
    with_config(client, &post(&1, "/runs/#{run_id}/signal", body, Mcpd.EventReceipt))
  end

  @doc """
  Route an event by name and key to the run waiting for it. Send your own
  `event_id:` from any handler that can be retried, such as a webhook: a repeat
  of a supplied id is answered `:duplicate` and writes nothing.
  """
  @spec publish_event(client(), String.t(), String.t(), keyword()) :: result(Mcpd.EventReceipt.t())
  def publish_event(client, event, key, opts \\ []) do
    with_config(client, fn config ->
      body =
        %{"project" => opts[:project] || config.project, "event" => event, "key" => key}
        |> put_if("payload", opts[:payload])
        |> put_if("event_id", opts[:event_id])

      post(config, "/events", body, Mcpd.EventReceipt)
    end)
  end

  @doc "Announce a worker and store what it serves."
  @spec register_worker(client(), String.t(), [Workflow.t() | Mcpd.DefinitionDocument.t()], keyword()) ::
          result(Mcpd.WorkerRegistered.t())
  def register_worker(client, worker_id, definitions, opts \\ []) do
    docs = Enum.map(definitions, fn d -> if match?(%Workflow{}, d), do: Workflow.document(d), else: d end)

    with_config(client, fn config ->
      body = %{"project" => opts[:project] || config.project, "worker_id" => worker_id, "definitions" => docs}
      post(config, "/workers/register", body, Mcpd.WorkerRegistered)
    end)
  end

  @doc "Offer up to `max:` runnable steps, waiting up to `wait_ms:` for one. `run_id:` limits the claim to one run."
  @spec claim_steps(client(), String.t(), keyword()) :: result(Mcpd.ClaimResponse.t())
  def claim_steps(client, worker_id, opts \\ []) do
    with_config(client, fn config ->
      wait_ms = opts[:wait_ms] || 0

      body =
        %{
          "project" => opts[:project] || config.project,
          "worker_id" => worker_id,
          "max" => opts[:max] || 1,
          "wait_ms" => wait_ms
        }
        |> put_if("run_id", opts[:run_id])

      post(config, "/steps/claim", body, Mcpd.ClaimResponse, wait_ms: wait_ms)
    end)
  end

  @doc "Declare that the body is running. Required under `at_most_once`."
  @spec step_started(client(), String.t()) :: result(Mcpd.LeaseState.t())
  def step_started(client, lease_id), do: with_config(client, &post(&1, "/steps/#{lease_id}/started", %{}, Mcpd.LeaseState))

  @doc "Extend the lease. The only evidence the daemon has that a worker is alive."
  @spec step_heartbeat(client(), String.t()) :: result(Mcpd.LeaseState.t())
  def step_heartbeat(client, lease_id), do: with_config(client, &post(&1, "/steps/#{lease_id}/heartbeat", %{}, Mcpd.LeaseState))

  @doc "Finish the step. Options: `output`, `state`, `usage`."
  @spec complete_step(client(), String.t(), keyword()) :: result(Mcpd.StepAck.t())
  def complete_step(client, lease_id, opts \\ []) do
    body = %{} |> put_if("output", opts[:output]) |> put_if("state", opts[:state]) |> put_if("usage", opts[:usage])
    with_config(client, &post(&1, "/steps/#{lease_id}/complete", body, Mcpd.StepAck))
  end

  @doc "Fail the step, with the class that decides whether it is retried. `error` is a keyword or map of `class`, `message`, `details`, `retry_after_ms`."
  @spec fail_step(client(), String.t(), keyword() | map()) :: result(Mcpd.StepAck.t())
  def fail_step(client, lease_id, error) do
    error = Map.new(error, fn {k, v} -> {to_string(k), v} end)

    body = %{
      "error" =>
        %{"class" => plain(Map.fetch!(error, "class")), "message" => Map.fetch!(error, "message")}
        |> put_if("details", error["details"])
        |> put_if("retry_after_ms", error["retry_after_ms"])
    }

    with_config(client, &post(&1, "/steps/#{lease_id}/fail", body, Mcpd.StepAck))
  end

  # ------------------------------------------------------------ internals ---

  defp with_config(client, fun) do
    case Worker.config(client) do
      nil -> {:error, :no_client}
      config -> fun.(config)
    end
  end

  defp get(config, path, module) do
    case Http.get_json(Config.api_url(config, path), http_opts(config)) do
      {:ok, body} -> {:ok, Codec.decode(module, body)}
      failure -> failure
    end
  end

  defp post(config, path, body, module, extra \\ []) do
    case Http.post_json(Config.api_url(config, path), wire(body), Keyword.merge(http_opts(config), extra)) do
      {:ok, answer} -> {:ok, Codec.decode(module, answer)}
      {:error, {:http, 409, text, _}} = failure -> conflict(text, failure)
      failure -> failure
    end
  end

  defp conflict(text, failure) do
    case :json.decode(text) do
      %{"reason" => _} = map -> {:error, {:conflict, Codec.decode(Mcpd.StepConflict, map)}}
      _ -> failure
    end
  rescue
    _ -> failure
  end

  defp http_opts(config), do: [api_key: config.api_key, timeout: config.timeout]

  defp query(pairs) do
    pairs
    |> Enum.reject(fn {_k, v} -> v == nil end)
    |> Enum.map_join("&", fn {k, v} -> URI.encode_www_form(to_string(k)) <> "=" <> URI.encode_www_form(plain_string(v)) end)
  end

  defp plain_string(v) when is_atom(v), do: Atom.to_string(v)
  defp plain_string(v), do: to_string(v)

  defp put_if(map, _key, nil), do: map
  defp put_if(map, key, value), do: Map.put(map, key, value)

  defp stringify(map), do: Map.new(map, fn {k, v} -> {to_string(k), v} end)

  defp plain(atom) when is_atom(atom) and not is_boolean(atom) and atom != nil, do: Atom.to_string(atom)
  defp plain(other), do: other

  @doc false
  # Structs to maps, atoms to strings, nils dropped: the shape `:json` encodes
  # and the daemon reads.
  def wire(%DateTime{} = at), do: DateTime.to_iso8601(at)

  def wire(%{__struct__: _} = struct) do
    struct |> Map.from_struct() |> wire()
  end

  def wire(map) when is_map(map) do
    map
    |> Enum.reject(fn {_k, v} -> v == nil end)
    |> Map.new(fn {k, v} -> {to_string(k), wire(v)} end)
  end

  # A list keeps its length, so a nil in it is a null, not a dropped item.
  def wire(list) when is_list(list), do: Enum.map(list, &(if &1 == nil, do: :null, else: wire(&1)))
  # A JSON null as `:json` decodes it. Written as an atom it would go out as
  # the string "null".
  def wire(:null), do: :null
  def wire(atom) when is_atom(atom) and not is_boolean(atom) and atom != nil, do: Atom.to_string(atom)
  def wire(other), do: other
end

defmodule Mcpd.RunError do
  @moduledoc "Raised by `Mcpd.Runs.watch_run/3` when a page of events cannot be read."
  defexception [:reason]

  @impl true
  def message(%{reason: reason}), do: "the run's events could not be read: #{inspect(reason)}"
end
