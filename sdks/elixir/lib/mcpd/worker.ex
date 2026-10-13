defmodule Mcpd.Worker do
  @moduledoc """
  The background worker: a GenServer that owns the queue and sends batches.

  Contract behaviours B2, B3 and B5. Recording a signal is a `cast` to this
  process, so it appends and returns; nothing in the caller's path waits on the
  network. `flush/2` is a `call` with a timeout, so a user who needs delivery
  before the process exits can wait for it explicitly.

  The HTTP itself happens in a separate sender process, one batch at a time.
  Retries sleep, and a worker that slept in its own loop would stop reading its
  mailbox, which is the queue's front door: casts would pile up unbounded
  behind it, and every `flush/2` would wait out the whole backoff. So the
  worker only ever does two things: keep the queue, and hand the next batch to
  a sender when the previous one is done.

  A GenServer is the natural shape here rather than a thread and a lock: the
  mailbox already is the queue's front door, and the OTP supervision tree
  already knows how to keep it alive.

  The front door is bounded too (B3). A mailbox is not: a worker that falls
  behind, or is busy stopping, would let casts pile up without limit. So a
  signal is shed at the call, and counted as dropped, while the worker's
  mailbox already holds `max_queue` messages. The queue drops its oldest
  signal when it is full; the mailbox can only refuse the newest, because a
  message already sent cannot be taken back.

  `flush/2` and `shutdown/2` return by a deadline, 10 seconds unless the
  caller passes another (contract section 2.4).
  """

  use GenServer

  alias Mcpd.{Config, Http, Ids, Queue}

  # Contract section 2.4: how long `flush/2` and `shutdown/2` wait by default.
  @deadline 10_000

  defmodule State do
    @moduledoc false
    defstruct [
      :config,
      :queue,
      :timer,
      :sink,
      :shed,
      inflight: nil,
      waiters: [],
      warned_at: nil,
      shed_seen: 0,
      abandoned: false,
      deadline: nil,
      exported: 0,
      failed: 0,
      rejected: 0
    ]
  end

  @doc "Start a worker for a config."
  @spec start_link(Config.t()) :: GenServer.on_start()
  def start_link(%Config{} = config), do: GenServer.start_link(__MODULE__, config)

  @doc """
  The config a worker was started with, readable without asking the worker.

  Read from `:persistent_term` rather than from the worker's state, because a
  call to the worker is a wait on the worker. Written once at start and erased
  once at stop, which is the pattern that store is for. After `shutdown/2` it
  is `nil`, and every recording call treats that as "record nothing": contract
  behaviour B1 says a signal after shutdown is ignored, never fatal.
  """
  @spec config(pid()) :: Config.t() | nil
  def config(pid) when is_pid(pid) do
    case :persistent_term.get({__MODULE__, pid}, nil) do
      {config, _shed} -> config
      nil -> nil
    end
  end

  def config(_), do: nil

  @doc """
  Queue one signal under a key. Never blocks, never raises.

  The client's `redact` function, when it has one, runs here, in the caller's
  process and before the signal is queued (contract section 2.1.1). Here
  because every span and log line passes through this one door, and in the
  caller rather than the worker so a slow redactor slows its own caller, never
  every other process recording through the same client.
  """
  @spec record(pid(), term(), atom(), map()) :: :ok
  def record(pid, key, kind, payload) when is_atom(kind) and is_map(payload) do
    admit(pid, {:record, key, kind, redacted(pid, kind, payload)})
  end

  @doc "Report an SDK problem through the worker's once-per-interval throttle."
  @spec warn(pid(), String.t()) :: :ok
  def warn(pid, message), do: admit(pid, {:warn, message})

  @doc "Count a span `Mcpd.record/3` refused, and say why through the throttle."
  @spec reject(pid(), String.t()) :: :ok
  def reject(pid, message), do: admit(pid, {:reject, message})

  @doc """
  Send everything queued and wait for it, for at most `timeout`. Idempotent.

  Returns `:ok` once nothing is queued or in flight, and `{:error, :timeout}`
  when the deadline passed first; what was not sent stays queued for the
  worker.
  """
  @spec flush(pid(), timeout()) :: :ok | {:error, :timeout}
  def flush(pid, timeout \\ @deadline) do
    GenServer.call(pid, {:flush, timeout}, timeout)
  catch
    :exit, {:timeout, _} -> {:error, :timeout}
    # A worker that is gone has nothing left to send. B1: never raise.
    _, _ -> :ok
  end

  @doc """
  Flush, then stop, within `timeout`. Safe to call twice.

  One deadline covers flushing and stopping together. When it passes, the
  worker starts no further batch and what is still queued is given up.
  """
  @spec shutdown(pid(), timeout()) :: :ok
  def shutdown(pid, timeout \\ @deadline) do
    if Process.alive?(pid) do
      deadline = deadline_after(timeout)
      flushed = flush(pid, timeout)
      ref = Process.monitor(pid)
      # A cast, not `GenServer.stop/3`: stop kills its helper at its own
      # timeout, and a helper killed before it delivered the request leaves
      # the worker running.
      GenServer.cast(pid, {:stop, flushed != :ok, deadline})

      receive do
        {:DOWN, ^ref, :process, _pid, _reason} -> :ok
      after
        remaining(deadline) -> Process.demonitor(ref, [:flush])
      end
    end

    :ok
  catch
    _, _ -> :ok
  end

  @doc "How many signals were dropped for a full queue or a full mailbox."
  @spec dropped(pid()) :: non_neg_integer()
  def dropped(pid), do: GenServer.call(pid, :dropped, 5_000)

  @doc """
  What the worker has done since it started: `queued`, `dropped`, `capacity`,
  `exported` (signals the sink took), `failed` (batches the sink refused),
  `rejected` (spans `Mcpd.record/3` refused) and `redaction_failures` (values
  `redact` could not process). Zeros once it has stopped.
  """
  @spec stats(pid()) :: %{
          queued: non_neg_integer(),
          dropped: non_neg_integer(),
          capacity: pos_integer(),
          exported: non_neg_integer(),
          failed: non_neg_integer(),
          rejected: non_neg_integer(),
          redaction_failures: non_neg_integer()
        }
  def stats(pid) do
    GenServer.call(pid, :stats, 5_000)
  catch
    _, _ ->
      %{queued: 0, dropped: 0, capacity: 0, exported: 0, failed: 0, rejected: 0, redaction_failures: 0}
  end

  # ------------------------------------------------------------ callbacks ---

  @impl true
  def init(%Config{} = config) do
    # `trap_exit` so a supervisor shutting us down still runs `terminate/2` and
    # gets one last flush. Losing the final batch is exactly the case B5 exists
    # to prevent.
    Process.flag(:trap_exit, true)
    # Slot 1 counts signals shed at a full mailbox, slot 2 values `redact`
    # failed on. Both are written from callers' processes, never from here, so
    # a counter rather than a message keeps them off the worker's mailbox.
    shed = :counters.new(2, [:write_concurrency])
    :persistent_term.put({__MODULE__, self()}, {config, shed})

    {:ok,
     %State{
       config: config,
       queue: Queue.new(config.max_queue),
       timer: schedule(config),
       sink: sink(config),
       shed: shed
     }}
  end

  @impl true
  def handle_cast({:record, key, kind, payload}, state) do
    before = state.queue.dropped
    queue = Queue.push(state.queue, key, kind, payload)
    state = %{state | queue: queue}

    state =
      if queue.dropped > before do
        warn_once(state, "signal queue is full at #{queue.max}, dropping the oldest (#{queue.dropped} so far)")
      else
        state
      end

    if queue.size >= state.config.batch_size do
      {:noreply, maybe_send(state)}
    else
      {:noreply, state}
    end
  end

  def handle_cast({:warn, message}, state), do: {:noreply, warn_once(state, message)}

  def handle_cast({:reject, message}, state),
    do: {:noreply, warn_once(%{state | rejected: state.rejected + 1}, message)}

  def handle_cast({:stop, abandon?, deadline}, state),
    do: {:stop, :normal, %{state | abandoned: abandon?, deadline: deadline}}

  @impl true
  def handle_call({:flush, timeout}, from, state) do
    if idle?(state) do
      {:reply, :ok, state}
    else
      # Answered from `settle/1` once the queue is empty and nothing is in
      # flight. Holding the reply rather than sending synchronously here is what
      # keeps the worker reading its mailbox while a batch is retried. Each
      # waiter leaves at its own deadline, so a daemon that never answers
      # cannot grow the list without bound.
      timer = if timeout == :infinity, do: nil, else: Process.send_after(self(), {:flush_expired, from}, timeout)
      {:noreply, maybe_send(%{state | waiters: [{from, timer} | state.waiters]})}
    end
  end

  def handle_call(:dropped, _from, state), do: {:reply, dropped_total(state), state}

  def handle_call(:stats, _from, state) do
    {:reply,
     %{
       queued: state.queue.size,
       dropped: dropped_total(state),
       capacity: state.queue.max,
       exported: state.exported,
       failed: state.failed,
       rejected: state.rejected,
       redaction_failures: :counters.get(state.shed, 2)
     }, state}
  end

  @impl true
  def handle_info(:tick, state) do
    state = state |> report_shed() |> maybe_send()
    {:noreply, %{state | timer: schedule(state.config)}}
  end

  def handle_info({:flush_expired, from}, state) do
    case List.keytake(state.waiters, from, 0) do
      {{^from, _timer}, waiters} ->
        GenServer.reply(from, {:error, :timeout})
        {:noreply, %{state | waiters: waiters}}

      nil ->
        {:noreply, state}
    end
  end

  def handle_info({:sent, pid, result, count}, %State{inflight: %{pid: pid}} = state) do
    {:noreply, delivered(state, result, count)}
  end

  def handle_info({:DOWN, ref, :process, _pid, _reason}, %State{inflight: %{ref: ref}} = state) do
    {:noreply, state |> Map.put(:inflight, nil) |> maybe_send() |> settle()}
  end

  def handle_info(_other, state), do: {:noreply, state}

  @impl true
  def terminate(_reason, state) do
    # A supervisor's shutdown reaches here without one of our deadlines, so it
    # gets the default: a stop that waits on a daemon that never answers holds
    # up whatever is stopping it.
    finish(state, state.deadline || deadline_after(@deadline))
    :persistent_term.erase({__MODULE__, self()})
    :ok
  end

  # --------------------------------------------------------------- internals ---

  # Cast, unless the worker's mailbox already holds `max_queue` messages. The
  # check reads the length the runtime keeps, so it costs no copy of the
  # mailbox. A remote pid cannot be asked, and is sent to as before.
  defp admit(pid, message) do
    case :persistent_term.get({__MODULE__, pid}, nil) do
      {%Config{max_queue: max}, shed} ->
        if backlogged?(pid, max) do
          :counters.add(shed, 1, 1)
        else
          GenServer.cast(pid, message)
        end

      nil ->
        GenServer.cast(pid, message)
    end

    :ok
  catch
    # A dead worker must not take the caller's request down. B1 again.
    _, _ -> :ok
  end

  # The fields `redact` sees, per signal (contract section 2.1.1). Metrics,
  # scores and feedback carry no payload of the caller's, so they pass as
  # they are.
  @redacted %{spans: ["input", "output", "attributes", "metadata"], logs: ["fields"]}

  # The signal with `redact` applied field by field. It fails closed: a field
  # the redactor raised, threw or exited on is dropped and counted, never sent
  # as it was, because a broken redactor is the case where the original does
  # the most harm. What the redactor returns is made JSON-safe again, since a
  # tuple it hands back would otherwise cost the whole batch at encode time.
  # Capture has already stripped `input` and `output` when it is off, so a
  # value capture kept back never reaches the redactor.
  defp redacted(pid, kind, payload) do
    with {:ok, fields} <- Map.fetch(@redacted, kind),
         {%Config{redact: redact}, shed} when is_function(redact, 1) <-
           :persistent_term.get({__MODULE__, pid}, nil) do
      Enum.reduce(fields, payload, fn field, acc ->
        case acc do
          %{^field => value} when value != nil -> redact_field(acc, field, value, redact, pid, shed)
          _ -> acc
        end
      end)
    else
      _ -> payload
    end
  end

  # A `nil` answer is the redactor dropping the field on purpose, so it is not
  # counted. `attributes`, `metadata` and a log line's `fields` are maps on the
  # wire, so anything else the redactor returns for them is unusable and fails
  # closed like a raise; `input` and `output` may be any value.
  @maps ["attributes", "metadata", "fields"]

  defp redact_field(payload, field, value, redact, pid, shed) do
    case redact.(value) do
      nil ->
        Map.delete(payload, field)

      kept when field in @maps and (not is_map(kept) or is_struct(kept)) ->
        redaction_failed(payload, field, pid, shed, "it returned something that is not a map")

      kept ->
        Map.put(payload, field, Mcpd.Logs.encode_fields(kept))
    end
  rescue
    e -> redaction_failed(payload, field, pid, shed, Exception.message(e))
  catch
    kind, reason -> redaction_failed(payload, field, pid, shed, "#{kind} #{inspect(reason)}")
  end

  defp redaction_failed(payload, field, pid, shed, why) do
    :counters.add(shed, 2, 1)
    warn(pid, "redact failed on #{field}, which was dropped: #{why}")
    Map.delete(payload, field)
  end

  defp backlogged?(pid, max) when node(pid) == node() do
    case Process.info(pid, :message_queue_len) do
      {:message_queue_len, len} -> len >= max
      nil -> false
    end
  end

  defp backlogged?(_pid, _max), do: false

  defp dropped_total(state), do: state.queue.dropped + :counters.get(state.shed, 1)

  # Say once per interval that the mailbox has been shedding, the same way a
  # full queue is reported.
  defp report_shed(state) do
    shed = :counters.get(state.shed, 1)

    if shed > state.shed_seen do
      state
      |> warn_once("the worker's mailbox is full at #{state.config.max_queue} messages, dropping new signals (#{shed} so far)")
      |> Map.put(:shed_seen, shed)
    else
      state
    end
  end

  defp deadline_after(:infinity), do: :infinity
  defp deadline_after(ms), do: System.monotonic_time(:millisecond) + ms

  defp remaining(:infinity), do: :infinity
  defp remaining(deadline), do: max(deadline - System.monotonic_time(:millisecond), 0)

  defp schedule(%Config{flush_interval: ms}), do: Process.send_after(self(), :tick, ms)

  defp idle?(%State{inflight: nil, queue: queue}), do: Queue.empty?(queue)
  defp idle?(_), do: false

  # Hand the next batch to a fresh sender, unless one is already out. One batch
  # in flight at a time keeps the order and keeps a slow daemon from being hit
  # by every queued batch at once.
  defp maybe_send(%State{inflight: nil} = state) do
    if Queue.empty?(state.queue) do
      state
    else
      {items, queue} = Queue.take(state.queue, state.config.batch_size)
      worker = self()
      sink = state.sink
      body = batch(items)
      count = length(items)

      {pid, ref} =
        spawn_monitor(fn ->
          send(worker, {:sent, self(), deliver(sink, body), count})
        end)

      %{state | queue: queue, inflight: %{pid: pid, ref: ref}}
    end
  end

  defp maybe_send(state), do: state

  # Reply to everyone waiting on a flush once there is nothing left to send.
  defp settle(%State{waiters: []} = state), do: state

  defp settle(state) do
    if idle?(state) do
      for {from, timer} <- state.waiters do
        if timer, do: Process.cancel_timer(timer)
        GenServer.reply(from, :ok)
      end

      %{state | waiters: []}
    else
      state
    end
  end

  # The sink, with the HTTP one given the address it sends to. Every other
  # sink is the user's own and takes whatever was given beside it.
  defp sink(%Config{sink: {Mcpd.Sink.Http, opts}} = config) when is_list(opts) do
    {Mcpd.Sink.Http,
     Keyword.merge([url: Config.ingest_url(config), api_key: config.api_key, timeout: config.timeout], opts)}
  end

  defp sink(%Config{sink: {module, opts}}), do: {module, opts}

  # One batch through the sink, inside the guard the transport has always had.
  # Contract behaviour B1 for a sink the user wrote: whatever it raises, the
  # sender process reports an error and the worker counts a failed batch.
  defp deliver({module, opts}, body) do
    case module.export(body, body["ingest_key"], opts) do
      :ok -> {:ok, nil}
      {:ok, _} = ok -> ok
      {:error, _} = failure -> failure
      other -> {:error, {:sink, {:bad_answer, other}}}
    end
  rescue
    e -> {:error, {:sink, Exception.message(e)}}
  catch
    kind, reason -> {:error, {:sink, {kind, reason}}}
  end

  # What a batch's fate does to the counters. A failure is swallowed, and
  # logged at most once per interval. The retries already happened in the
  # sender. Holding the batch any longer would need a durable queue, and a
  # silent one that falls hours behind is worse than a visible gap.
  defp delivered(state, {:ok, _}, count), do: %{state | exported: state.exported + count}

  defp delivered(state, {:error, reason}, _count) do
    warn_once(%{state | failed: state.failed + 1}, "could not deliver a batch: #{inspect(reason)}")
  end

  # On the way out: send what is left, one batch at a time, until the
  # deadline. The batch in flight is never cut short; its sender is not linked
  # to this process and finishes on its own. A shutdown that ran out of time
  # sets `abandoned`, and then nothing further starts at all.
  defp finish(state, deadline) do
    state = await_inflight(state, deadline)

    cond do
      state.inflight != nil -> state
      state.abandoned or Queue.empty?(state.queue) -> state
      remaining(deadline) == 0 -> state
      true -> state |> maybe_send() |> finish(deadline)
    end
  end

  # A sender still out when the worker stops is given until the deadline, and
  # never longer than its own retries can take.
  defp await_inflight(%State{inflight: nil} = state, _deadline), do: state

  defp await_inflight(%State{inflight: %{ref: ref}, config: config} = state, deadline) do
    wait = min(remaining(deadline), Http.max_attempts() * config.timeout + 60_000)

    receive do
      {:DOWN, ^ref, :process, _pid, _reason} -> %{state | inflight: nil}
    after
      wait -> state
    end
  end

  # Contract section 6: every batch carries an `ingest_key`, and the key is
  # stable for the body it is sent with. It is what lets mcpd tell a retry from
  # a second batch. Without one, a request that timed out after the daemon had
  # already stored it is re-sent and counted again, so a single slow response
  # doubles every signal in the batch.
  defp batch(items) do
    items
    |> Enum.reduce(%{}, fn {kind, payload}, acc ->
      key = to_string(kind)
      Map.update(acc, key, [payload], &(&1 ++ [payload]))
    end)
    |> Map.put("ingest_key", Ids.trace_id())
  end

  # One warning per flush interval at most. An unreachable daemon otherwise
  # fills the user's log with the same line thousands of times, which is its own
  # kind of outage.
  #
  # Written straight to standard error rather than through `Logger`. The
  # default console handler writes to standard output, and a diagnostic
  # arriving there can land in the middle of the agent's own output, which
  # every other SDK keeps clean. Going through `Logger` would need the user's
  # logging configuration changed to get the same result, and an observability
  # library has no business editing that.
  defp warn_once(state, message) do
    now = System.monotonic_time(:millisecond)

    if state.warned_at == nil or now - state.warned_at > state.config.flush_interval do
      try do
        :io.put_chars(:standard_error, "mcpd: " <> message <> "\n")
      catch
        # Standard error can be closed on a detached process. The warning is
        # then lost, and that is the whole of the damage.
        _, _ -> :ok
      end

      %{state | warned_at: now}
    else
      state
    end
  end
end
