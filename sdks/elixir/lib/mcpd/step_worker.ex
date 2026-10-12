defmodule Mcpd.StepWorker do
  @moduledoc """
  The worker: the user's own process, serving step bodies to the daemon.

  It registers the workflows it holds, long-polls for leases, and for each
  lease posts `started`, heartbeats while the body runs, and posts `complete`
  or `fail`. Every 409 the daemon can answer is handled by its reason, because
  a worker that collapses them into one branch abandons steps it has already
  finished or reports a failure for a success.

  Nothing here runs a model or reads a stored prompt. The daemon hands the
  worker a lease with the rendered input and the prior steps; the body does
  the rest with the modules, the pool and the credentials this node holds.

      {:ok, worker} = Mcpd.StepWorker.start_link(mcpd, workflows: [support], concurrency: 8)
      :ok = Mcpd.StepWorker.serve(worker)

  Options to `start_link/2`: `workflows` (a list of `Mcpd.Workflow`),
  `worker_id` (a name for this node, recorded as the owner of every lease),
  `concurrency` (how many bodies run at once, default 4), `wait_ms` (how long
  each claim waits for work, default 20 000, at most 60 000), `project` (needed
  only when the key reaches several) and `on_error`, a two-arity function the
  worker reports its own failures through. Without one, each kind of failure
  goes to `Logger.warning/1` at most once per interval: the key is the kind,
  never the step, so a daemon refusing every step of a run is one line, not
  one per step.

  Four more hooks let the worker be watched from inside, each optional:
  `on_lease` (a `%Mcpd.StepLease{}`, once it is claimed and before anything is
  posted), `on_step_start` (the `%Mcpd.StepContext{}`, once `started` was
  accepted), `on_step_end` (the context and `:completed` or
  `{:failed, class, message}`) and `on_conflict` (the lease and the daemon's
  reason for a 409, as a string). A hook that raises is reported through
  `on_error` and the loop goes on. `stats/1` answers what the worker has done.
  """

  use GenServer
  require Logger

  alias Mcpd.{Runs, StepContext, StepException, StepLease, Workflow}

  defmodule State do
    @moduledoc false
    defstruct [
      :client,
      :id,
      :concurrency,
      :wait_ms,
      :project,
      :on_error,
      :on_lease,
      :on_step_start,
      :on_step_end,
      :on_conflict,
      workflows: [],
      counts: %{claimed: 0, completed: 0, failed: %{}, conflicts: %{}, unservable: 0},
      heartbeat_ms: 10_000,
      tasks: %{},
      loop: nil,
      stopping: false,
      waiters: [],
      logged: %{}
    ]
  end

  # ------------------------------------------------------------------ api ---

  @doc "Start a worker on `client`. See the module documentation for the options."
  @spec start_link(pid(), keyword()) :: GenServer.on_start()
  def start_link(client, opts \\ []), do: GenServer.start_link(__MODULE__, {client, opts})

  @doc "Add workflows this worker serves. Their definitions are posted by `serve/1` or `register_definitions/1`."
  @spec register(GenServer.server(), [Workflow.t()]) :: :ok
  def register(worker, workflows) when is_list(workflows), do: GenServer.call(worker, {:register, workflows})

  @doc "Post every registered definition and read back the lease timings."
  @spec register_definitions(GenServer.server()) :: :ok | {:error, term()}
  def register_definitions(worker), do: GenServer.call(worker, :register_definitions, :infinity)

  @doc """
  One claim, serving what it granted on processes of its own. Returns how
  many leases were taken. `serve/1` does this in a loop; a script that drives
  its own run calls it with `run_id:` until the run ends, then `drain/2`.
  """
  @spec claim(GenServer.server(), keyword()) :: {:ok, non_neg_integer()} | {:error, term()}
  def claim(worker, opts \\ []) do
    settings = GenServer.call(worker, :settings)
    max = opts[:max] || settings.concurrency

    case Runs.claim_steps(settings.client, settings.id,
           max: max |> max(1) |> min(100),
           wait_ms: settings.wait_ms,
           run_id: opts[:run_id],
           project: settings.project
         ) do
      {:ok, %{leases: leases}} ->
        :ok = GenServer.call(worker, {:serve, leases})
        {:ok, length(leases)}

      failure ->
        failure
    end
  end

  @doc "Wait for every step in flight."
  @spec drain(GenServer.server(), timeout()) :: :ok
  def drain(worker, timeout \\ :infinity), do: GenServer.call(worker, :drain, timeout)

  @doc "Register, then claim and serve in the background until `stop/2`."
  @spec serve(GenServer.server()) :: :ok | {:error, term()}
  def serve(worker), do: GenServer.call(worker, :serve, :infinity)

  @doc "Stop claiming and wait for the steps in flight."
  @spec stop(GenServer.server(), timeout()) :: :ok
  def stop(worker, timeout \\ :infinity), do: GenServer.call(worker, :stop, timeout)

  @doc "Steps running right now."
  @spec inflight(GenServer.server()) :: non_neg_integer()
  def inflight(worker), do: GenServer.call(worker, :inflight)

  @doc "The name every lease records as its owner."
  @spec worker_id(GenServer.server()) :: String.t()
  def worker_id(worker), do: GenServer.call(worker, :settings).id

  @doc """
  What this worker has done since it started: `claimed` leases, `inflight`
  bodies, `completed` bodies, `failed` bodies by error class, `conflicts` by
  the daemon's 409 reason, and `unservable` leases handed back before any
  body ran. A fenced lease abandoned in silence looks like no work at all from
  outside; this is where it shows.
  """
  @spec stats(GenServer.server()) :: %{
          claimed: non_neg_integer(),
          inflight: non_neg_integer(),
          completed: non_neg_integer(),
          failed: %{String.t() => non_neg_integer()},
          conflicts: %{String.t() => non_neg_integer()},
          unservable: non_neg_integer()
        }
  def stats(worker), do: GenServer.call(worker, :stats)

  # ------------------------------------------------------------- callbacks --

  @impl true
  def init({client, opts}) do
    state = %State{
      client: client,
      id: opts[:worker_id] || default_id(),
      concurrency: max(opts[:concurrency] || 4, 1),
      wait_ms: (opts[:wait_ms] || 20_000) |> max(0) |> min(60_000),
      project: opts[:project],
      on_error: opts[:on_error],
      on_lease: opts[:on_lease],
      on_step_start: opts[:on_step_start],
      on_step_end: opts[:on_step_end],
      on_conflict: opts[:on_conflict],
      workflows: opts[:workflows] || []
    }

    {:ok, state}
  end

  @impl true
  def handle_call({:register, workflows}, _from, state),
    do: {:reply, :ok, %State{state | workflows: state.workflows ++ workflows}}

  def handle_call(:settings, _from, state), do: {:reply, settings(state), state}

  def handle_call(:register_definitions, _from, state) do
    case do_register(state) do
      {:ok, state} -> {:reply, :ok, state}
      {:error, reason} -> {:reply, {:error, reason}, state}
    end
  end

  def handle_call({:serve, leases}, _from, state) do
    settings = settings(state)

    tasks =
      Enum.reduce(leases, state.tasks, fn lease, acc ->
        {pid, ref} = spawn_monitor(fn -> serve_lease(settings, lease) end)
        Map.put(acc, ref, pid)
      end)

    counts = Map.update!(state.counts, :claimed, &(&1 + length(leases)))
    {:reply, :ok, %State{state | tasks: tasks, counts: counts}}
  end

  def handle_call(:stats, _from, state),
    do: {:reply, Map.put(state.counts, :inflight, map_size(state.tasks)), state}

  def handle_call(:free, _from, state),
    do: {:reply, state.concurrency - map_size(state.tasks), state}

  def handle_call(:inflight, _from, state), do: {:reply, map_size(state.tasks), state}

  def handle_call(:drain, from, state) do
    if map_size(state.tasks) == 0,
      do: {:reply, :ok, state},
      else: {:noreply, %State{state | waiters: [from | state.waiters]}}
  end

  def handle_call(:serve, _from, %State{loop: nil} = state) do
    case do_register(state) do
      {:ok, state} ->
        worker = self()
        settings = settings(state)
        loop = spawn(fn -> claim_loop(worker, settings) end)
        {:reply, :ok, %State{state | loop: loop, stopping: false}}

      {:error, reason} ->
        {:reply, {:error, reason}, state}
    end
  end

  def handle_call(:serve, _from, state), do: {:reply, :ok, state}

  def handle_call(:stop, from, state) do
    if state.loop, do: Process.exit(state.loop, :kill)
    state = %State{state | loop: nil, stopping: true}

    if map_size(state.tasks) == 0,
      do: {:reply, :ok, state},
      else: {:noreply, %State{state | waiters: [from | state.waiters]}}
  end

  @impl true
  def handle_cast({:count, event}, state), do: {:noreply, %State{state | counts: count(state.counts, event)}}

  # A lease given up at its deadline or by a fence frees its slot now, whether
  # or not the body heeds `cancelled?/1` (contract section 7.2). The body's
  # process is left to finish on its own; its exit is no longer awaited.
  def handle_cast({:given_up, pid}, state) do
    case Enum.find(state.tasks, fn {_ref, task} -> task == pid end) do
      {ref, _} ->
        Process.demonitor(ref, [:flush])
        handle_info({:DOWN, ref, :process, pid, :given_up}, state)

      nil ->
        {:noreply, state}
    end
  end

  # The default error report, once per interval per kind of failure.
  def handle_cast({:log, kind, message, reason}, state) do
    now = System.monotonic_time(:millisecond)
    interval = log_interval(state.client)

    case Map.get(state.logged, kind) do
      last when is_integer(last) and now - last < interval ->
        {:noreply, state}

      _ ->
        Logger.warning("#{message}: #{inspect(reason)}")
        {:noreply, %State{state | logged: Map.put(state.logged, kind, now)}}
    end
  end

  @impl true
  def handle_info({:DOWN, ref, :process, _pid, _reason}, state) do
    tasks = Map.delete(state.tasks, ref)
    state = %State{state | tasks: tasks}

    if map_size(tasks) == 0 do
      Enum.each(state.waiters, &GenServer.reply(&1, :ok))
      {:noreply, %State{state | waiters: []}}
    else
      {:noreply, state}
    end
  end

  def handle_info(_other, state), do: {:noreply, state}

  # ------------------------------------------------------------ internals ---

  defp settings(state) do
    %{
      worker: self(),
      client: state.client,
      id: state.id,
      concurrency: state.concurrency,
      wait_ms: state.wait_ms,
      project: state.project,
      on_error: state.on_error,
      on_lease: state.on_lease,
      on_step_start: state.on_step_start,
      on_step_end: state.on_step_end,
      on_conflict: state.on_conflict,
      workflows: state.workflows,
      heartbeat_ms: state.heartbeat_ms
    }
  end

  defp count(counts, :completed), do: Map.update!(counts, :completed, &(&1 + 1))
  defp count(counts, :unservable), do: Map.update!(counts, :unservable, &(&1 + 1))
  defp count(counts, {:failed, class}), do: Map.update!(counts, :failed, &Map.update(&1, class, 1, fn n -> n + 1 end))
  defp count(counts, {:conflict, reason}), do: Map.update!(counts, :conflicts, &Map.update(&1, reason, 1, fn n -> n + 1 end))

  # A hook is the user's code, so it follows the recording rule: wrapped,
  # reported through `on_error` when it raises, never able to stop the loop.
  defp hook(settings, name, args) do
    case settings[name] do
      fun when is_function(fun) -> apply(fun, args)
      _ -> :ok
    end

    :ok
  rescue
    e -> report_error(settings, :hook, "mcpd worker #{settings.id}: the #{name} hook raised", e)
  catch
    kind, reason -> report_error(settings, :hook, "mcpd worker #{settings.id}: the #{name} hook raised", {kind, reason})
  end

  defp counted(settings, event), do: GenServer.cast(settings.worker, {:count, event})

  defp do_register(state) do
    case Runs.register_worker(state.client, state.id, state.workflows, project: state.project) do
      {:ok, registered} -> {:ok, %State{state | heartbeat_ms: max(registered.heartbeat_ms, 1_000)}}
      {:error, reason} -> {:error, reason}
    end
  end

  # The claim loop runs in a process of its own: a long poll must not hold the
  # worker's mailbox, and `stop/2` can kill it between claims.
  defp claim_loop(worker, settings) do
    free = GenServer.call(worker, :free)

    if free <= 0 do
      Process.sleep(50)
    else
      case Runs.claim_steps(settings.client, settings.id,
             max: min(free, 100),
             wait_ms: settings.wait_ms,
             project: settings.project
           ) do
        {:ok, %{leases: []}} ->
          Process.sleep(10)

        {:ok, %{leases: leases}} ->
          :ok = GenServer.call(worker, {:serve, leases})

        {:error, reason} ->
          report_error(settings, :claim, "mcpd worker #{settings.id}: claim failed; retrying", reason)
          Process.sleep(1_000)
      end
    end

    claim_loop(worker, settings)
  end

  defp handler_for(workflows, %StepLease{} = lease) do
    Enum.find_value(workflows, fn wf ->
      cond do
        wf.id != lease.definition_id -> nil
        wf.version != nil and wf.version != lease.definition_version -> nil
        true -> Workflow.handler_for(wf, lease.step_key)
      end
    end)
  end

  defp serve_lease(settings, %StepLease{} = lease) do
    client = settings.client
    hook(settings, :on_lease, [lease])

    case handler_for(settings.workflows, lease) do
      nil ->
        message =
          "worker #{settings.id} does not serve #{lease.definition_id}@#{lease.definition_version} step #{lease.step_key}"

        counted(settings, :unservable)
        report(settings, lease, Runs.fail_step(client, lease.lease_id, class: :unservable, message: message))

      handler ->
        # Before the body. Always. This is what tells the daemon the
        # difference between "offered" and "running", and on an at_most_once
        # step it refuses a complete without it.
        if report(settings, lease, Runs.step_started(client, lease.lease_id)) != :abandon do
          run_body(settings, lease, handler)
        end
    end
  end

  defp run_body(settings, lease, handler) do
    client = settings.client
    ctx = StepContext.new(client, lease)
    hook(settings, :on_step_start, [ctx])
    body = self()
    beater = spawn(fn -> beat(settings, lease, ctx, body) end)

    # The step's own ceiling, enforced here as well as by the daemon: a body
    # that runs past `deadline_ms` is cancelled through `cancelled?/1` and the
    # attempt is failed retryable, so the process is freed and the journal
    # says why, instead of the daemon reclaiming a lease still heartbeating
    # from inside a body that will never return. The deadline process does
    # the accounting and frees the slot itself, so a body that never looks at
    # `cancelled?/1` does not hold the slot past its deadline.
    deadline =
      case lease.deadline_ms do
        ms when is_integer(ms) and ms > 0 ->
          spawn(fn ->
            Process.sleep(ms)

            unless StepContext.cancelled?(ctx) do
              reason = "step deadline of #{ms}ms passed"
              StepContext.cancel(ctx, reason)
              Process.exit(beater, :kill)
              report(settings, lease, Runs.fail_step(client, lease.lease_id, class: :retryable, message: reason))
              counted(settings, {:failed, "retryable"})
              hook(settings, :on_step_end, [ctx, {:failed, :retryable, reason}])
              GenServer.cast(settings.worker, {:given_up, body})
              send(body, {:deadline_reported, self()})
            end
          end)

        _ ->
          nil
      end

    # The body runs inside the step's trace context, and every span it closes
    # in this process hands its usage to the context (contract section 7.3).
    result =
      try do
        {:ok,
         StepContext.with_usage_sink(ctx, fn ->
           Mcpd.continue(ctx.traceparent, fn -> handler.(ctx, lease.input, lease.prior || %{}) end)
         end)}
      rescue
        e in StepException -> {:failed, e}
        e -> {:failed, %StepException{message: Exception.message(e), class: :fatal}}
      catch
        kind, reason -> {:failed, %StepException{message: Exception.format_exit({kind, reason}), class: :fatal}}
      end

    Process.exit(beater, :kill)

    # A deadline that fired is posting the failure; this lease is not
    # finished until that post has landed. One that has not fired is not
    # needed any more.
    if deadline do
      if StepContext.cancelled?(ctx) and Process.alive?(deadline) do
        receive do
          {:deadline_reported, ^deadline} -> :ok
        after
          30_000 -> :ok
        end
      else
        Process.exit(deadline, :kill)
      end
    end

    cond do
      # The deadline fired, and its process posted and counted the failure; a
      # fence, or the end of the run, left the lease not ours to finish. The
      # body's own outcome is not what happened either way.
      StepContext.cancelled?(ctx) ->
        :ok

      true ->
        outcome =
          case result do
            {:failed, %StepException{} = f} ->
              report(
                settings,
                lease,
                Runs.fail_step(client, lease.lease_id,
                  class: f.class,
                  message: f.message,
                  details: f.details,
                  retry_after_ms: f.retry_after_ms
                )
              )

              counted(settings, {:failed, to_string(f.class)})
              {:failed, f.class, f.message}

            {:ok, output} ->
              {state, usage} = StepContext.collected(ctx)
              report(settings, lease, Runs.complete_step(client, lease.lease_id, output: output, state: state, usage: usage))
              counted(settings, :completed)
              :completed
          end

        # After the post, never before it: a slow hook must not hold the
        # daemon's answer.
        hook(settings, :on_step_end, [ctx, outcome])
    end

    StepContext.release(ctx)
  end

  defp beat(settings, lease, ctx, body) do
    Process.sleep(settings.heartbeat_ms)

    case Runs.step_heartbeat(settings.client, lease.lease_id) do
      {:ok, _} ->
        beat(settings, lease, ctx, body)

      {:error, {:conflict, conflict}} ->
        # The daemon has moved on without us; the body learns through
        # `cancelled?`, and its slot is freed whether or not it looks.
        StepContext.cancel(ctx, "#{conflict.reason}: #{conflict.message}")
        GenServer.cast(settings.worker, {:given_up, body})

      {:error, reason} ->
        # A transport failure is not evidence of anything; the next beat may land.
        report_error(settings, :heartbeat, "mcpd worker #{settings.id}: heartbeat failed for #{lease.step_key}", reason)
        beat(settings, lease, ctx, body)
    end
  end

  # One worker call through the 409 table: `already_applied` is a success, and
  # every other reason means abandon this lease. Every reason is counted and
  # handed to `on_conflict`, because a fenced lease abandoned in silence looks
  # exactly like no work at all from outside.
  defp report(_settings, _lease, {:ok, _}), do: :landed

  defp report(settings, lease, {:error, {:conflict, %{reason: :already_applied} = conflict}}) do
    conflicted(settings, lease, conflict)
    :done
  end

  defp report(settings, lease, {:error, {:conflict, %{reason: :not_started} = conflict} = reason}) do
    conflicted(settings, lease, conflict)

    report_error(
      settings,
      :not_started,
      "mcpd worker #{settings.id}: a bug in this worker: #{lease.step_key} was reported before started",
      reason
    )

    :abandon
  end

  defp report(settings, lease, {:error, {:conflict, conflict}}) do
    conflicted(settings, lease, conflict)
    :abandon
  end

  defp report(settings, lease, {:error, reason}) do
    report_error(settings, :call, "mcpd worker #{settings.id}: call failed for #{lease.step_key}", reason)
    :abandon
  end

  defp conflicted(settings, lease, conflict) do
    reason = to_string(Map.get(conflict, :reason) || "unknown")
    counted(settings, {:conflict, reason})
    hook(settings, :on_conflict, [lease, reason])
  end

  # The caller's `on_error` sees every failure. Without one, the worker's own
  # process writes each kind at most once per interval.
  defp report_error(%{on_error: fun}, _kind, message, reason) when is_function(fun, 2), do: fun.(message, reason)
  defp report_error(settings, kind, message, reason), do: GenServer.cast(settings.worker, {:log, kind, message, reason})

  defp log_interval(client) do
    case Mcpd.Worker.config(client) do
      %{flush_interval: ms} -> ms
      _ -> 2_000
    end
  end

  defp default_id do
    host =
      case :inet.gethostname() do
        {:ok, name} -> to_string(name)
        _ -> "worker"
      end

    host <> "-" <> String.slice(Mcpd.Ids.span_id(), 0, 8)
  end
end
