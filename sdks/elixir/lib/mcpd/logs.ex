defmodule Mcpd.Logs do
  @moduledoc """
  The logging signal: what a field map may hold, and how a level relates to an
  OpenTelemetry severity number.

  Nothing here raises. A field the caller handed us is data, and data that
  cannot be encoded is worth less than the batch it travels in: `:json.encode/1`
  raises on a tuple, a pid or an exception struct, and the sender loses every
  span, metric, score and piece of feedback that shared the batch with it.
  """

  alias Mcpd.LogLevel

  # How deep a nested field is followed before it is named rather than walked.
  @max_depth 12

  @doc """
  An exception as its type and its message.

  An Elixir exception carries no stacktrace of its own, so none is reported:
  the stacktrace of whichever process happened to be logging is not where the
  error came from.
  """
  @spec describe_error(term()) :: map() | nil
  def describe_error(%{__struct__: module} = value) do
    if is_exception(value) do
      %{
        "type" => module |> Atom.to_string() |> String.replace_prefix("Elixir.", ""),
        "message" => safe_message(value)
      }
    end
  end

  def describe_error(_), do: nil

  @doc """
  A caller's field map, in a shape `:json.encode/1` is certain to accept.

  One value nothing can encode costs its own key and nothing else.
  """
  @spec encode_fields(term()) :: term()
  def encode_fields(value), do: encode(value, 0)

  @doc "The OpenTelemetry severity number the spec gives a level."
  @spec severity_number(LogLevel.t()) :: integer()
  def severity_number(level), do: LogLevel.severity(level)

  @doc """
  The level an OpenTelemetry severity number lands on.

  By range, the way the daemon reads an inbound OTLP `LogRecord`: 1 to 4 is
  trace, 5 to 8 debug, 9 to 12 info, 13 to 16 warn, 17 to 20 error, and 21 and
  above fatal. A number outside the range is clamped rather than refused,
  because a sender that invents a 40 still meant something serious.
  """
  @spec log_level_from_severity(integer()) :: LogLevel.t()
  def log_level_from_severity(severity) when is_integer(severity) do
    Enum.reduce(LogLevel.values(), hd(LogLevel.values()), fn candidate, found ->
      if severity >= LogLevel.severity(candidate), do: candidate, else: found
    end)
  end

  @doc "Read a level by name, case-insensitively. Anything else is nil."
  @spec parse_log_level(String.t() | atom() | nil) :: LogLevel.t() | nil
  def parse_log_level(nil), do: nil
  def parse_log_level(level) when is_atom(level), do: parse_log_level(Atom.to_string(level))

  def parse_log_level(name) when is_binary(name) do
    case name |> String.trim() |> String.downcase() |> LogLevel.from_wire() do
      {:ok, level} -> level
      :error -> nil
    end
  end

  @doc "Whether `level` is at or above the floor."
  @spec meets_level(LogLevel.t(), LogLevel.t()) :: boolean()
  def meets_level(level, floor), do: LogLevel.severity(level) >= LogLevel.severity(floor)

  defp encode(value, _depth) when is_binary(value) do
    if String.valid?(value), do: value, else: inspect(value)
  end

  defp encode(value, _depth) when is_number(value) or is_boolean(value) or is_nil(value), do: value
  defp encode(value, _depth) when is_atom(value), do: Atom.to_string(value)

  defp encode(%Date{} = value, _depth), do: Date.to_iso8601(value)
  defp encode(%Time{} = value, _depth), do: Time.to_iso8601(value)
  defp encode(%DateTime{} = value, _depth), do: DateTime.to_iso8601(value)
  defp encode(%NaiveDateTime{} = value, _depth), do: NaiveDateTime.to_iso8601(value)

  defp encode(%{__struct__: _} = value, depth) do
    case describe_error(value) do
      nil -> if depth >= @max_depth, do: "[too deep]", else: encode(Map.from_struct(value), depth)
      described -> described
    end
  end

  defp encode(value, depth) when is_map(value) do
    if depth >= @max_depth do
      "[too deep]"
    else
      Map.new(value, fn {key, item} -> {key_of(key), encode(item, depth + 1)} end)
    end
  end

  defp encode(value, depth) when is_list(value) do
    if depth >= @max_depth do
      "[too deep]"
    else
      Enum.map(value, &encode(&1, depth + 1))
    end
  end

  # A tuple, a pid, a reference, a port and a function all have no JSON form.
  # Naming one is more use to the reader than dropping the key it sat under,
  # and far better than losing the batch.
  defp encode(value, _depth), do: inspect(value)

  defp key_of(key) when is_binary(key), do: key
  defp key_of(key) when is_atom(key), do: Atom.to_string(key)
  defp key_of(key), do: inspect(key)

  defp safe_message(value) do
    Exception.message(value)
  rescue
    _ -> inspect(value)
  end
end
