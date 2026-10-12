defmodule Mcpd.Context do
  @moduledoc """
  W3C trace context, and the active span for the current process.

  Contract behaviour B7. `traceparent` is what makes a trace survive a service
  boundary, so it is parsed on the way in and emitted on the way out, sampled
  flag included: a trace some services keep and others drop is not a trace.

  The active span lives in the process dictionary. That is the right store in
  Elixir specifically: a process is the unit of concurrency, so two requests
  handled at once cannot see each other's span, and no explicit context has to
  be threaded through every function to keep the tree assembling itself.
  """

  @key :mcpd_active_span

  @doc "The active span in this process, if any."
  @spec current() :: map() | nil
  def current, do: Process.get(@key)

  @doc "Run a function with `span` active, restoring the previous one after."
  @spec with_span(map(), (-> any())) :: any()
  def with_span(span, fun) do
    previous = Process.get(@key)
    Process.put(@key, span)

    try do
      fun.()
    after
      # `after` rather than a plain call: an exception in the user's code must
      # still restore the parent, or every later span in this process attaches
      # to a span that already ended.
      if previous, do: Process.put(@key, previous), else: Process.delete(@key)
    end
  end

  @doc """
  Format a `traceparent` for an outgoing request.

  Version 00. The sampled flag carries the decision this trace made, so a
  downstream service keeps or drops the same trace.
  """
  @spec traceparent(String.t(), String.t(), boolean()) :: String.t()
  def traceparent(trace_id, span_id, sampled \\ true),
    do: "00-#{trace_id}-#{span_id}-" <> if(sampled, do: "01", else: "00")

  @doc """
  Parse an incoming `traceparent`.

  Returns `:error` rather than raising on anything malformed. A caller sending a
  broken header is a reason to start a new trace, never a reason to fail the
  request that carried it.
  """
  @spec parse_traceparent(String.t() | nil) ::
          {:ok, %{trace_id: String.t(), span_id: String.t(), sampled: boolean()}} | :error
  def parse_traceparent(header) when is_binary(header) do
    case header |> String.trim() |> String.split("-") do
      # Four fields for version 00. A later version may append more, and the
      # first four keep their meaning, so those are read and the rest ignored.
      [version, trace_id, span_id, flags | _rest] ->
        if valid_version?(version) and Mcpd.Ids.valid?(trace_id, 16) and
             Mcpd.Ids.valid?(span_id, 8) and valid_flags?(flags) do
          {:ok, %{trace_id: trace_id, span_id: span_id, sampled: sampled?(flags)}}
        else
          :error
        end

      _ ->
        :error
    end
  end

  def parse_traceparent(_), do: :error

  # `ff` is explicitly forbidden by the spec; anything else two-hex is a future
  # version we are allowed to accept the known fields from.
  defp valid_version?(v), do: String.length(v) == 2 and v != "ff" and String.match?(v, ~r/^[0-9a-f]{2}$/)

  defp valid_flags?(f), do: String.match?(f, ~r/^[0-9a-f]{2}$/)

  # Only the lowest bit is the sampled flag. The other seven are reserved, and
  # a future spec that sets one must not read as "not sampled".
  defp sampled?(flags) do
    {bits, ""} = Integer.parse(flags, 16)
    Bitwise.band(bits, 1) == 1
  end
end
