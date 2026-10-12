defmodule Mcpd.StepException do
  @moduledoc """
  A failure with a class, raised from a step body to tell the daemon how to
  treat it. Any other exception a body raises is `:fatal`.

      raise Mcpd.StepException, message: "mail relay busy", class: :retryable, retry_after_ms: 2_000
  """
  defexception message: "the step failed", class: :fatal, retry_after_ms: nil, details: nil

  @type t :: %__MODULE__{
          message: String.t(),
          class: Mcpd.StepErrorClass.t(),
          retry_after_ms: non_neg_integer() | nil,
          details: term()
        }
end

defmodule Mcpd.StepContext do
  @moduledoc """
  What a step body is handed besides its input and the prior steps.

  `cancelled?/1` turns true when the daemon fences this lease or ends the run,
  so a body that checks it stops when it should. `span/4` records a span under
  the step the daemon opened, so a model call inside the body lands in the
  run's trace at the right depth; so does any `Mcpd.span` opened in the body's
  own process, because the body runs inside the step's trace context.

  Every span the body closes in its own process adds its `usage:`, `model:`
  and `provider:` to the step's usage: the tokens are summed and the last
  model seen wins, so a body that records its model calls as spans never adds
  tokens by hand. `set_usage/2` is the explicit override, and the fields it
  sets replace the accumulated ones.
  """

  alias Mcpd.{Context, StepLease}

  @type t :: %__MODULE__{
          client: pid(),
          lease: StepLease.t(),
          run_id: String.t(),
          step_key: String.t(),
          attempt: non_neg_integer(),
          idempotency_key: String.t(),
          traceparent: String.t(),
          store: pid()
        }

  defstruct [:client, :lease, :run_id, :step_key, :attempt, :idempotency_key, :traceparent, :store]

  @doc false
  def new(client, %StepLease{} = lease) do
    {:ok, store} =
      Agent.start_link(fn -> %{state: nil, usage: nil, accumulated: nil, cancelled: false, reason: nil} end)

    %__MODULE__{
      client: client,
      lease: lease,
      run_id: lease.run_id,
      step_key: lease.step_key,
      attempt: lease.attempt,
      idempotency_key: lease.idempotency_key,
      traceparent: Context.traceparent(lease.trace_id, lease.span_id, true),
      store: store
    }
  end

  @doc false
  def release(%__MODULE__{store: store}), do: if(Process.alive?(store), do: Agent.stop(store))

  @doc "The run's accumulated state as of this lease."
  @spec state(t()) :: term()
  def state(%__MODULE__{lease: lease}), do: lease.state

  @doc """
  The step's rendered input, checked by `decode`.

  A decoder that raises fails the step `:invalid_input`, never `:fatal`: the
  input was rendered by the daemon from a template, so a shape the body did
  not expect is a template mistake, and one attempt is the right thing to
  charge for it. With no decoder the input is returned as it is.
  """
  @spec input(t(), (term() -> term()) | nil) :: term()
  def input(%__MODULE__{lease: lease}, decode \\ nil), do: decode_or(lease.input, decode, "input")

  @doc "The prior steps, keyed by id, checked by `decode`. Same rule as `input/2`."
  @spec prior(t(), (term() -> term()) | nil) :: term()
  def prior(%__MODULE__{lease: lease}, decode \\ nil), do: decode_or(lease.prior || %{}, decode, "prior")

  @doc "The run's state, checked by `decode`. Same rule as `input/2`."
  @spec state_as(t(), (term() -> term())) :: term()
  def state_as(%__MODULE__{lease: lease}, decode), do: decode_or(lease.state, decode, "state")

  defp decode_or(value, nil, _what), do: value

  defp decode_or(value, decode, what) when is_function(decode, 1) do
    decode.(value)
  rescue
    e ->
      raise Mcpd.StepException,
        message: "the step's #{what} could not be read: #{Exception.message(e)}",
        class: :invalid_input
  end

  @doc "Merge keys into the run's state when the step completes. Top-level keys replace."
  @spec set_state(t(), map()) :: :ok
  def set_state(%__MODULE__{store: store}, patch) when is_map(patch) do
    patch = Map.new(patch, fn {k, v} -> {to_string(k), v} end)
    Agent.update(store, fn s -> %{s | state: Map.merge(s.state || %{}, patch)} end)
  end

  @doc """
  Report the tokens this step spent, so the daemon prices and charges them.
  Keys: `input_tokens`, `output_tokens`, `total_tokens`, `model`, `provider`.
  Fields set here replace fields set earlier, and replace what the body's
  spans accumulated.
  """
  @spec set_usage(t(), map() | keyword()) :: :ok
  def set_usage(%__MODULE__{store: store}, usage) do
    usage = Map.new(usage, fn {k, v} -> {to_string(k), v} end)
    Agent.update(store, fn s -> %{s | usage: Map.merge(s.usage || %{}, usage)} end)
  end

  @doc false
  # A span closing inside the body hands its usage here (contract section
  # 7.3): the three counts are summed over the spans that carried them, and
  # the last model and provider seen win. Called by `Mcpd` through the process
  # dictionary; never by a body.
  def __accumulate__(store, usage, model, provider) do
    usage = if is_map(usage), do: Map.new(usage, fn {k, v} -> {to_string(k), v} end), else: %{}
    usage = if Map.has_key?(usage, :__struct__), do: Map.delete(usage, :__struct__), else: usage

    Agent.update(store, fn s ->
      acc = s.accumulated || %{}

      acc =
        Enum.reduce(~w(input_tokens output_tokens total_tokens), acc, fn key, acc ->
          case usage[key] do
            n when is_integer(n) -> Map.update(acc, key, n, &(&1 + n))
            _ -> acc
          end
        end)

      acc = if is_binary(model) and model != "", do: Map.put(acc, "model", model), else: acc
      acc = if is_binary(provider) and provider != "", do: Map.put(acc, "provider", provider), else: acc
      %{s | accumulated: if(map_size(acc) == 0, do: nil, else: acc)}
    end)
  catch
    _, _ -> :ok
  end

  @doc "True once the daemon has fenced this lease or ended the run."
  @spec cancelled?(t()) :: boolean()
  def cancelled?(%__MODULE__{store: store}), do: Agent.get(store, & &1.cancelled)

  @doc "Why, once `cancelled?/1` is true."
  @spec cancel_reason(t()) :: String.t() | nil
  def cancel_reason(%__MODULE__{store: store}), do: Agent.get(store, & &1.reason)

  @doc "Block until the lease is lost or the run ends, or `timeout` passes. Returns whether it was cancelled."
  @spec wait_cancelled(t(), timeout()) :: boolean()
  def wait_cancelled(ctx, timeout \\ :infinity) do
    deadline = if timeout == :infinity, do: :infinity, else: System.monotonic_time(:millisecond) + timeout
    poll_cancelled(ctx, deadline)
  end

  defp poll_cancelled(ctx, deadline) do
    cond do
      cancelled?(ctx) -> true
      deadline != :infinity and System.monotonic_time(:millisecond) >= deadline -> false
      true ->
        Process.sleep(20)
        poll_cancelled(ctx, deadline)
    end
  end

  @doc """
  Run `fun` inside a span nested under this step in the run's trace, carrying
  the run's correlation. Options are `Mcpd.with_span/4`'s.
  """
  @spec span(t(), String.t(), keyword(), (-> any())) :: any()
  def span(%__MODULE__{} = ctx, name, opts \\ [], fun) do
    c = ctx.lease.correlation

    # The same five fields every SDK carries from the run onto a body's span.
    opts =
      opts
      |> put_new_if(:session_id, c.session_id)
      |> put_new_if(:user_id, c.user_id)
      |> put_new_if(:thread_id, c.thread_id)
      |> put_new_if(:request_id, c.request_id)
      |> put_new_if(:release, c.release)

    run = fn -> Mcpd.with_span(ctx.client, name, opts, fun) end
    if Context.current() == nil, do: Mcpd.continue(ctx.traceparent, run), else: run.()
  end

  defp put_new_if(opts, _key, nil), do: opts
  defp put_new_if(opts, key, value), do: Keyword.put_new(opts, key, value)

  @doc false
  def cancel(%__MODULE__{store: store}, reason),
    do: Agent.update(store, fn s -> %{s | cancelled: true, reason: reason} end)

  @doc false
  # What `complete` carries: the state patch, and the usage as the spans
  # accumulated it with the explicit `set_usage/2` fields on top.
  def collected(%__MODULE__{store: store}) do
    Agent.get(store, fn s ->
      usage =
        case {s.accumulated, s.usage} do
          {nil, explicit} -> explicit
          {acc, nil} -> acc
          {acc, explicit} -> Map.merge(acc, explicit)
        end

      {s.state, usage}
    end)
  end

  @doc false
  # Run `fun` with this context as the destination for every span closed in
  # the calling process, restoring whatever was there before.
  def with_usage_sink(%__MODULE__{store: store}, fun) do
    previous = Process.get(:mcpd_usage_sink)
    Process.put(:mcpd_usage_sink, store)

    try do
      fun.()
    after
      if previous, do: Process.put(:mcpd_usage_sink, previous), else: Process.delete(:mcpd_usage_sink)
    end
  end
end
