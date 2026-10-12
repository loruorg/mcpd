defmodule Mcpd.Queue do
  @moduledoc """
  A bounded, keyed signal queue that drops the oldest when full.

  Contract behaviour B3. An unbounded buffer turns a network outage into an
  out-of-memory kill, which is the observability library killing the process it
  exists to observe.

  Oldest-first is the deliberate choice. When mcpd is unreachable and the queue
  fills, the newest signals are the ones describing what is happening now, and
  those are what somebody will want to read when it comes back.

  Entries are keyed, which is what makes contract behaviour B8 affordable. A
  span is queued when it starts and again when it ends; when the end arrives
  before the worker has sent the start, the second write replaces the first in
  place, keeps its position, and the span costs one record instead of two.
  """

  @type key :: term()
  @type t :: %__MODULE__{
          order: :queue.queue(key()),
          items: %{key() => {atom(), map()}},
          size: non_neg_integer(),
          max: pos_integer(),
          dropped: non_neg_integer()
        }
  defstruct order: nil, items: %{}, size: 0, max: 8_192, dropped: 0

  @doc "An empty queue bounded at `max` items."
  @spec new(pos_integer()) :: t()
  def new(max), do: %__MODULE__{order: :queue.new(), items: %{}, size: 0, max: max, dropped: 0}

  @doc """
  Queue one signal under `key`.

  A key already queued is updated in place and keeps its position, so the
  order still reflects when the work started. A new key that would exceed the
  bound costs the oldest entry, and the loss is counted.
  """
  @spec push(t(), key(), atom(), map()) :: t()
  def push(%__MODULE__{} = q, key, kind, payload) do
    cond do
      Map.has_key?(q.items, key) ->
        %{q | items: Map.put(q.items, key, {kind, payload})}

      q.size >= q.max ->
        {{:value, oldest}, rest} = :queue.out(q.order)

        %{
          q
          | order: :queue.in(key, rest),
            items: q.items |> Map.delete(oldest) |> Map.put(key, {kind, payload}),
            dropped: q.dropped + 1
        }

      true ->
        %{
          q
          | order: :queue.in(key, q.order),
            items: Map.put(q.items, key, {kind, payload}),
            size: q.size + 1
        }
    end
  end

  @doc "Take up to `n` signals, oldest first, and return them with the rest."
  @spec take(t(), pos_integer()) :: {[{atom(), map()}], t()}
  def take(%__MODULE__{} = q, n) do
    count = min(n, q.size)
    {front, rest} = :queue.split(count, q.order)
    keys = :queue.to_list(front)
    taken = Enum.map(keys, &Map.fetch!(q.items, &1))
    {taken, %{q | order: rest, items: Map.drop(q.items, keys), size: q.size - count}}
  end

  @doc "Whether the queue holds nothing."
  @spec empty?(t()) :: boolean()
  def empty?(%__MODULE__{size: 0}), do: true
  def empty?(_), do: false
end
