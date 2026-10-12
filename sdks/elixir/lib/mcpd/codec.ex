defmodule Mcpd.Codec.Shape do
  @moduledoc false
  # Reads the field types of every generated struct out of `generated.ex`.
  #
  # Called at compile time by `Mcpd.Codec`, so the decoder is derived from the
  # generated file rather than written by hand beside it: the spec stays the one
  # place the data model is described. The source is read rather than the
  # compiled typespecs because a release strips debug information from its
  # beams, and a decoder that worked in development and returned nothing in
  # production would be the worst kind of drift.

  @doc "Every generated struct module, with its fields and their decode shapes."
  @spec index(Path.t()) :: %{module() => [{atom(), term()}]}
  def index(path) do
    modules =
      path
      |> File.read!()
      |> Code.string_to_quoted!(file: path)
      |> top_level()
      |> Enum.flat_map(&module_type/1)

    enums = for {mod, {:enum, _}} <- modules, into: MapSet.new(), do: mod
    structs = for {mod, {:struct, _}} <- modules, into: MapSet.new(), do: mod

    for {mod, {:struct, fields}} <- modules, into: %{} do
      {mod, Enum.map(fields, fn {name, ast} -> {name, shape(ast, structs, enums)} end)}
    end
  end

  defp top_level({:__block__, _, forms}), do: forms
  defp top_level(form), do: [form]

  # `defmodule Mcpd.X do ... @type t :: ... end`, and nothing else is of interest.
  defp module_type({:defmodule, _, [{:__aliases__, _, parts}, [do: body]]}) do
    module = Module.concat(parts)

    body
    |> top_level()
    |> Enum.find_value([], fn
      {:@, _, [{:type, _, [{:"::", _, [{:t, _, _}, {:%, _, [{:__MODULE__, _, _}, {:%{}, _, fields}]}]}]}]} ->
        [{module, {:struct, fields}}]

      {:@, _, [{:type, _, [{:"::", _, [{:t, _, _}, {:|, _, _} = union]}]}]} ->
        if Enum.all?(flatten_union(union), &is_atom/1), do: [{module, {:enum, []}}]

      _ ->
        nil
    end)
  end

  defp module_type(_), do: []

  defp flatten_union({:|, _, [left, right]}), do: flatten_union(left) ++ flatten_union(right)
  defp flatten_union(leaf), do: [leaf]

  # What a field's type means for decoding. Anything not named here is carried
  # through as the JSON gave it.
  defp shape({{:., _, [{:__aliases__, _, [:String]}, :t]}, _, []}, _s, _e), do: :raw
  defp shape({{:., _, [{:__aliases__, _, [:DateTime]}, :t]}, _, []}, _s, _e), do: :datetime

  defp shape({{:., _, [{:__aliases__, _, parts}, :t]}, _, []}, structs, enums) do
    mod = Module.concat(parts)

    cond do
      MapSet.member?(structs, mod) -> {:struct, mod}
      MapSet.member?(enums, mod) -> {:enum, mod}
      true -> :raw
    end
  end

  defp shape([inner], structs, enums), do: {:list, shape(inner, structs, enums)}

  defp shape({:|, _, _} = union, structs, enums) do
    case union |> flatten_union() |> Enum.reject(&is_nil/1) do
      [only] -> shape(only, structs, enums)
      _ -> :raw
    end
  end

  defp shape(_other, _s, _e), do: :raw
end

defmodule Mcpd.Codec do
  @moduledoc """
  Builds the generated structs from the JSON the daemon answers with.

  The read surface returns `Mcpd.TraceDetail`, `Mcpd.TraceListResponse` and
  the rest of the generated types rather than bare maps, so a field name is
  checked by the compiler where a map key would be checked by nobody.

  Enum fields decode to their atoms, timestamps to `DateTime`, and nested
  structs recursively. A value this build has never heard of, such as an enum
  variant a newer daemon added, is kept as the string it arrived as rather than
  refused: a read that fails on a field it does not need helps nobody.
  Timestamps keep microseconds, which is the most `DateTime` can hold.
  """

  @generated Path.join(__DIR__, "generated.ex")
  @external_resource @generated
  @index Mcpd.Codec.Shape.index(@generated)

  @doc "Build `module` from a decoded JSON object. Anything that is not an object is returned as it is."
  @spec decode(module(), term()) :: term()
  def decode(module, map) when is_map(map) do
    case Map.fetch(@index, module) do
      {:ok, fields} ->
        values =
          for {name, kind} <- fields,
              {:ok, raw} <- [Map.fetch(map, Atom.to_string(name))],
              do: {name, value(kind, raw)}

        # `struct/2` rather than `struct!/2`: a required field the daemon left
        # out arrives as `nil` instead of an exception, because a read must
        # not depend on the two sides being built from the same spec revision
        # to answer at all. The `spec_version` on `health/1` is the check for
        # that, made once, at connect.
        struct(module, values)

      :error ->
        map
    end
  end

  def decode(_module, other), do: other

  @doc "The generated struct modules this decoder knows, for tests that pin the index."
  @spec known() :: [module()]
  def known, do: Map.keys(@index)

  defp value(_kind, nil), do: nil
  defp value({:list, inner}, list) when is_list(list), do: Enum.map(list, &value(inner, &1))
  defp value({:struct, mod}, map) when is_map(map), do: decode(mod, map)

  defp value({:enum, mod}, wire) when is_binary(wire) do
    case mod.from_wire(wire) do
      {:ok, atom} -> atom
      :error -> wire
    end
  end

  defp value(:datetime, text) when is_binary(text) do
    case DateTime.from_iso8601(text) do
      {:ok, at, _offset} -> at
      _ -> text
    end
  end

  defp value(_kind, raw), do: raw
end
