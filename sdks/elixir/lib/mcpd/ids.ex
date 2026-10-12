defmodule Mcpd.Ids do
  @moduledoc """
  OpenTelemetry-shaped identifiers: 32 lowercase hex for a trace, 16 for a span.

  Contract behaviour B6. Anything else breaks correlation with every other tool
  that speaks the same wire format, which is the entire reason to use it.
  """

  @doc "A new 128-bit trace id, as 32 lowercase hex characters."
  @spec trace_id() :: String.t()
  def trace_id, do: random_hex(16)

  @doc "A new 64-bit span id, as 16 lowercase hex characters."
  @spec span_id() :: String.t()
  def span_id, do: random_hex(8)

  # An all-zero id is invalid in the W3C spec and means "no parent", so a
  # generator that can emit one produces a span that silently detaches from its
  # trace. Redrawing is cheaper than the bug.
  defp random_hex(bytes) do
    hex = bytes |> :crypto.strong_rand_bytes() |> Base.encode16(case: :lower)
    if String.match?(hex, ~r/^0+$/), do: random_hex(bytes), else: hex
  end

  @doc "Whether a string is a valid id of the given byte length."
  @spec valid?(String.t() | nil, pos_integer()) :: boolean()
  def valid?(id, bytes) when is_binary(id) do
    String.length(id) == bytes * 2 and
      String.match?(id, ~r/^[0-9a-f]+$/) and
      not String.match?(id, ~r/^0+$/)
  end

  def valid?(_, _), do: false
end
