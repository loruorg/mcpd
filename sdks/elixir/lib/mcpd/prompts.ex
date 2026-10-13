defmodule Mcpd.PromptRenderError do
  @moduledoc """
  Raised by `Mcpd.Prompts.render!/2`, and returned by `Mcpd.Prompts.render/2`,
  when a template names variables the caller did not give.

  `missing` lists every one of them, in order of first use, so one error is
  enough to fix the call.
  """
  defexception [:prompt, missing: []]

  @type t :: %__MODULE__{prompt: String.t() | nil, missing: [String.t()]}

  @impl true
  def message(%{prompt: prompt, missing: missing}) do
    which = if is_binary(prompt), do: "prompt #{inspect(prompt)}", else: "the prompt"

    "#{which} needs variables that were not given: #{Enum.join(missing, ", ")}. " <>
      "Add each one to the variables you render with."
  end
end

defmodule Mcpd.Prompts do
  @moduledoc """
  Prompts kept in the daemon: fetch one, render it, and link the spans that
  used it (contract section 5.1).

      {:ok, prompt} = Mcpd.get_prompt(mcpd, "support-reply", fallback: "Answer {{question}}")
      {:ok, text} = Mcpd.Prompts.render(prompt, %{question: question})

      Mcpd.span mcpd, "answer", kind: :model_generation,
        attributes: Mcpd.Prompts.attributes(prompt) do
        call_the_model(text)
      end

  Every function here that talks to the daemon is also reachable through
  `Mcpd`. They answer `{:ok, value}` or `{:error, reason}`, like the read API,
  and raise `ArgumentError` only for a call that could never be right, before
  any request is made.
  """

  alias Mcpd.{Codec, Config, Http, Worker}

  # Contract section 5.1: the cache holds at most this many prompts, and the
  # oldest goes first. A client asked for prompts by generated names must not
  # grow without bound (B3).
  @max_entries 256

  @default_ttl 60_000

  # Asked for when the caller names neither a label nor a version. The latest
  # version answers when no version carries it yet.
  @default_label "production"

  @type client :: pid()
  @type fallback :: String.t() | [map()]

  @doc """
  One version of a prompt, from the cache when the copy there is fresh.

  Options:

    * `:label` reads the version a label points at, such as `"staging"`.
    * `:version` reads one numbered version. Not with `:label`.
    * `:cache_ttl`, in milliseconds, how long a fetched copy is served with no
      request. 60 000 by default; 0 fetches every time.
    * `:fallback`, a template (a text prompt) or a list of messages (a chat
      prompt), returned when the fetch fails and nothing was ever fetched. It
      comes back as version 0 with no labels, and is never cached.
    * `:project`, the client's project by default.

  With neither `:label` nor `:version`, the label `production` is asked for,
  and the latest version when nothing carries that label yet.

  When a fetch fails and an older copy exists, the copy is returned however
  old it is, and the failure is reported through the client's diagnostics.
  When nothing was ever fetched and no fallback is given, the fetch error is
  returned. A disabled client still fetches: a prompt is the agent's input,
  not telemetry.
  """
  @spec get_prompt(client(), String.t(), keyword()) :: {:ok, Mcpd.Prompt.t()} | {:error, term()}
  def get_prompt(client, name, opts \\ []) do
    opts = Keyword.validate!(opts, [:label, :version, :cache_ttl, :fallback, :project])
    check_name!(name)
    selector = selector!(opts[:label], opts[:version])
    ttl = ttl!(Keyword.get(opts, :cache_ttl, @default_ttl))
    fallback = fallback!(opts[:fallback])

    config = Worker.config(client)
    project = opts[:project] || (config && config.project) || "default"
    table = table(client)
    key = {project, name, selector}
    now = System.monotonic_time(:millisecond)

    case lookup(table, key) do
      {:ok, prompt, at} when now - at < ttl ->
        {:ok, prompt}

      cached ->
        case fetch(config, project, name, selector) do
          {:ok, prompt} ->
            store(table, key, prompt)
            {:ok, prompt}

          {:error, reason} = failure ->
            case {cached, fallback} do
              {{:ok, prompt, _at}, _} ->
                Worker.warn(
                  client,
                  "prompt #{inspect(name)} could not be fetched (#{inspect(reason)}); " <>
                    "serving the copy fetched earlier"
                )

                {:ok, prompt}

              {:miss, nil} ->
                failure

              {:miss, given} ->
                {:ok, fallback_prompt(project, name, given)}
            end
        end
    end
  end

  @doc """
  Store a new version of a prompt and return it as stored.

  `attrs` is a keyword list, a map or a `%Mcpd.CreatePromptRequest{}`: `name`,
  and `template` for a text prompt or `messages` for a chat prompt, with
  optional `kind`, `config`, `note` and `labels`. A body identical to the
  latest version stores nothing and returns that version, so a deploy can
  post its prompts on every start. The read cache is left as it is.
  """
  @spec create_prompt(client(), keyword() | map() | Mcpd.CreatePromptRequest.t(), keyword()) ::
          {:ok, Mcpd.Prompt.t()} | {:error, term()}
  def create_prompt(client, attrs, opts \\ []) do
    opts = Keyword.validate!(opts, [:project])
    attrs = if is_list(attrs), do: Map.new(attrs), else: attrs

    with_config(client, fn config ->
      url = url(config, "", project: opts[:project] || config.project)
      decode(Http.post_json(url, Mcpd.Runs.wire(attrs), http_opts(config)))
    end)
  end

  @doc """
  Point `label` at `version` of the prompt `name`, and return that version.

  Moving `production` releases a version and moving it back rolls one back,
  with no deploy. The read cache is left as it is, so a running agent sees the
  move within its `cache_ttl`.
  """
  @spec set_prompt_label(client(), String.t(), String.t(), non_neg_integer(), keyword()) ::
          {:ok, Mcpd.Prompt.t()} | {:error, term()}
  def set_prompt_label(client, name, label, version, opts \\ []) do
    opts = Keyword.validate!(opts, [:project])
    check_name!(name)

    unless is_binary(label) and label != "" do
      raise ArgumentError, "set_prompt_label needs a label as a non-empty string, got #{inspect(label)}"
    end

    unless is_integer(version) and version >= 0 do
      raise ArgumentError, "set_prompt_label needs a version number, got #{inspect(version)}"
    end

    with_config(client, fn config ->
      path = "/" <> segment(name) <> "/labels/" <> segment(label)
      url = url(config, path, project: opts[:project] || config.project)
      decode(Http.put_json(url, %{"version" => version}, http_opts(config)))
    end)
  end

  @doc """
  Fill a prompt's variables, by the daemon's own rule.

  A text prompt renders to a string. A chat prompt renders to a copy of its
  messages in which every string `content` is rendered and every other field
  is kept. `variables` is a map or keyword list with string or atom keys; a
  value that is not a string is written in its plain string form.

  A placeholder is `{{name}}`, with whitespace allowed inside the braces.
  Braces around anything that is not a name, and a `{{` never closed, are kept
  as written. Substitution is one pass: a value containing `{{x}}` is not
  rendered again. A variable the template names and `variables` lacks is an
  error naming every one missing.
  """
  @spec render(Mcpd.Prompt.t(), map() | keyword()) ::
          {:ok, String.t() | [map()]} | {:error, Mcpd.PromptRenderError.t()}
  def render(prompt, variables \\ %{})

  def render(%Mcpd.Prompt{} = prompt, variables) when is_map(variables) or is_list(variables) do
    vars = Map.new(variables, fn {k, v} -> {to_string(k), v} end)

    {rendered, missing} =
      case body(prompt) do
        {:text, template} ->
          substitute(template, vars, [], [])

        {:chat, messages} ->
          {out, missing} = Enum.map_reduce(messages, [], &render_message(&1, vars, &2))
          {out, missing}
      end

    case missing do
      [] -> {:ok, rendered}
      _ -> {:error, %Mcpd.PromptRenderError{prompt: prompt.name, missing: Enum.reverse(missing)}}
    end
  end

  def render(prompt, variables) do
    raise ArgumentError,
          "render needs a %Mcpd.Prompt{} and a map of variables, got #{inspect(prompt)} and #{inspect(variables)}"
  end

  @doc "`render/2`, raising `Mcpd.PromptRenderError` when a variable is missing."
  @spec render!(Mcpd.Prompt.t(), map() | keyword()) :: String.t() | [map()]
  def render!(prompt, variables \\ %{}) do
    case render(prompt, variables) do
      {:ok, rendered} -> rendered
      {:error, error} -> raise error
    end
  end

  @doc """
  The span attributes that link a span to the prompt it used:
  `mcpd.prompt.name` and `mcpd.prompt.version`.

  Pass them as the `attributes:` option of `Mcpd.span/4` or `Mcpd.with_span/4`;
  more than one `attributes:` is merged. A fallback (version 0) carries the
  name only. Anything that is not a prompt gives `%{}`, never an exception: a
  link is instrumentation and must not break the run it describes (B1).
  """
  @spec attributes(term()) :: map()
  def attributes(%{name: name} = prompt) when is_binary(name) and name != "" do
    base = %{"mcpd.prompt.name" => name}

    case Map.get(prompt, :version) do
      v when is_integer(v) and v > 0 -> Map.put(base, "mcpd.prompt.version", v)
      _ -> base
    end
  end

  def attributes(_other), do: %{}

  @doc """
  Every variable a template, or a chat prompt's messages, names, in order of
  first use.
  """
  @spec variables(String.t() | term()) :: [String.t()]
  def variables(value), do: value |> collect([]) |> Enum.reverse()

  @doc false
  # The table the worker owns, created at client start.
  def new_cache, do: :ets.new(:mcpd_prompts, [:set, :public, read_concurrency: true, write_concurrency: true])

  @doc false
  def max_entries, do: @max_entries

  @doc false
  # How many prompts a client holds, for the test that pins the bound.
  def cache_size(client) do
    case table(client) do
      nil -> 0
      table -> :ets.info(table, :size)
    end
  rescue
    _ -> 0
  end

  # ------------------------------------------------------------- internals ---

  defp check_name!(name) do
    unless is_binary(name) and name != "" do
      raise ArgumentError, "a prompt name is a non-empty string, got #{inspect(name)}"
    end
  end

  # Refused here rather than sent: the daemon would answer 400, and the caller
  # would learn of the mistake only once the call ran.
  defp selector!(label, version) when label != nil and version != nil do
    raise ArgumentError,
          "get_prompt takes a label or a version, not both: got label #{inspect(label)} " <>
            "and version #{inspect(version)}. Drop one."
  end

  defp selector!(nil, nil), do: :default
  defp selector!(label, nil) when is_binary(label) and label != "", do: {:label, label}
  defp selector!(nil, version) when is_integer(version) and version >= 0, do: {:version, version}

  defp selector!(label, version) do
    raise ArgumentError,
          "get_prompt needs label: as a non-empty string or version: as a whole number, " <>
            "got label #{inspect(label)} and version #{inspect(version)}"
  end

  defp ttl!(ttl) when is_integer(ttl) and ttl >= 0, do: ttl

  defp ttl!(ttl),
    do: raise(ArgumentError, "cache_ttl is a whole number of milliseconds, 0 or more, got #{inspect(ttl)}")

  defp fallback!(nil), do: nil
  defp fallback!(template) when is_binary(template), do: template
  defp fallback!(messages) when is_list(messages), do: messages

  defp fallback!(other) do
    raise ArgumentError,
          "fallback is a template string or a list of message maps, got #{inspect(other)}"
  end

  defp fallback_prompt(project, name, template) when is_binary(template) do
    %Mcpd.Prompt{
      project: project,
      name: name,
      version: 0,
      kind: :text,
      template: template,
      variables: variables(template),
      labels: [],
      created_at: nil
    }
  end

  defp fallback_prompt(project, name, messages) do
    %Mcpd.Prompt{
      project: project,
      name: name,
      version: 0,
      kind: :chat,
      messages: messages,
      variables: variables(messages),
      labels: [],
      created_at: nil
    }
  end

  defp fetch(nil, _project, _name, _selector), do: {:error, :no_client}

  defp fetch(config, project, name, :default) do
    case get(config, project, name, label: @default_label) do
      {:error, {:http, 404, _body, _retry_after}} -> get(config, project, name, [])
      other -> other
    end
  end

  defp fetch(config, project, name, {:label, label}), do: get(config, project, name, label: label)
  defp fetch(config, project, name, {:version, v}), do: get(config, project, name, version: v)

  defp get(config, project, name, selector) do
    url = url(config, "/" <> segment(name), [{:project, project} | selector])
    decode(Http.get_json(url, http_opts(config)))
  end

  defp decode({:ok, %{} = body}), do: {:ok, Codec.decode(Mcpd.Prompt, body)}
  defp decode({:ok, other}), do: {:error, {:decode, other}}
  defp decode({:error, _} = failure), do: failure

  defp url(config, path, query) do
    pairs = for {k, v} <- query, v != nil, do: {Atom.to_string(k), to_string(v)}
    Config.api_url(config, "/prompts" <> path <> "?" <> URI.encode_query(pairs))
  end

  # A name may hold `/`, which would otherwise read as another path segment.
  defp segment(text), do: URI.encode(text, &URI.char_unreserved?/1)

  defp with_config(client, fun) do
    case Worker.config(client) do
      nil -> {:error, :no_client}
      config -> fun.(config)
    end
  end

  defp http_opts(config), do: [api_key: config.api_key, timeout: config.timeout]

  defp table(client) when is_pid(client), do: :persistent_term.get({__MODULE__, client}, nil)
  defp table(_client), do: nil

  # The table belongs to the worker, so it can vanish between finding it and
  # using it if the client stops at that moment. That is a miss, not a crash.
  defp lookup(nil, _key), do: :miss

  defp lookup(table, key) do
    case :ets.lookup(table, key) do
      [{^key, prompt, at, _seq}] -> {:ok, prompt, at}
      [] -> :miss
    end
  rescue
    ArgumentError -> :miss
  end

  defp store(nil, _key, _prompt), do: :ok

  defp store(table, key, prompt) do
    seq = System.unique_integer([:monotonic])
    :ets.insert(table, {key, prompt, System.monotonic_time(:millisecond), seq})
    evict(table)
  rescue
    ArgumentError -> :ok
  end

  # Oldest stored first. Two callers storing at once can each evict, which
  # costs at most an extra miss; the bound itself always holds.
  defp evict(table) do
    if :ets.info(table, :size) > @max_entries do
      {oldest, _} =
        :ets.foldl(
          fn {key, _p, _at, seq}, {_k, best} = acc -> if seq < best, do: {key, seq}, else: acc end,
          {nil, :infinity},
          table
        )

      :ets.delete(table, oldest)
      evict(table)
    end

    :ok
  end

  defp body(%Mcpd.Prompt{kind: :chat, messages: messages}) when is_list(messages), do: {:chat, messages}
  defp body(%Mcpd.Prompt{template: template}) when is_binary(template), do: {:text, template}
  defp body(%Mcpd.Prompt{messages: messages}) when is_list(messages), do: {:chat, messages}

  defp body(prompt) do
    raise ArgumentError,
          "prompt #{inspect(prompt.name)} has neither a template nor a list of messages to render"
  end

  defp render_message(%{} = message, vars, missing) do
    key =
      cond do
        Map.has_key?(message, "content") -> "content"
        Map.has_key?(message, :content) -> :content
        true -> nil
      end

    case key && Map.get(message, key) do
      content when is_binary(content) ->
        {text, missing} = substitute(content, vars, [], missing)
        {Map.put(message, key, text), missing}

      _ ->
        {message, missing}
    end
  end

  defp render_message(other, _vars, missing), do: {other, missing}

  # One pass over the text: what a value contains is output, never read again.
  # `out` is iodata in reverse, `missing` names in reverse order of first use.
  defp substitute(text, vars, out, missing) do
    case :binary.split(text, "{{") do
      [rest] ->
        {finish(out, rest), missing}

      [before, opened] ->
        case :binary.split(opened, "}}") do
          [_unclosed] ->
            {finish(out, text), missing}

          [inner, rest] ->
            name = String.trim(inner)

            {piece, missing} =
              cond do
                not name?(name) -> {["{{", inner, "}}"], missing}
                Map.has_key?(vars, name) -> {plain(Map.fetch!(vars, name)), missing}
                name in missing -> {"", missing}
                true -> {"", [name | missing]}
              end

            substitute(rest, vars, [piece, before | out], missing)
        end
    end
  end

  defp finish(out, rest), do: [rest | out] |> Enum.reverse() |> IO.iodata_to_binary()

  defp plain(v) when is_binary(v), do: v
  defp plain(v) when is_atom(v) or is_number(v), do: to_string(v)

  # A list would be read as characters by `to_string/1`, and a map or tuple has
  # no string form, so those are written as Elixir would print them.
  defp plain(v) when is_struct(v) do
    if String.Chars.impl_for(v), do: to_string(v), else: inspect(v)
  rescue
    _ -> inspect(v)
  end

  defp plain(v), do: inspect(v)

  defp collect(text, acc) when is_binary(text), do: scan(text, acc)
  defp collect(list, acc) when is_list(list), do: Enum.reduce(list, acc, &collect/2)
  defp collect(%{__struct__: _} = struct, acc), do: struct |> Map.from_struct() |> collect(acc)
  defp collect(%{} = map, acc), do: map |> Map.values() |> collect(acc)
  defp collect(_other, acc), do: acc

  defp scan(text, acc) do
    with [_before, opened] <- :binary.split(text, "{{"),
         [inner, rest] <- :binary.split(opened, "}}") do
      name = String.trim(inner)
      acc = if name?(name) and name not in acc, do: [name | acc], else: acc
      scan(rest, acc)
    else
      _ -> acc
    end
  end

  # The daemon's own rule for a variable name (`variables_in` in
  # `crates/mcpd-server/src/prompts.rs`): an ASCII letter or `_`, then ASCII
  # letters, digits, `_` and `.`. Anything else between braces is text, so a
  # JSON example inside a prompt does not invent variables.
  defp name?(<<c, rest::binary>>) when c in ?a..?z or c in ?A..?Z or c == ?_, do: name_rest?(rest)
  defp name?(_), do: false

  defp name_rest?(<<>>), do: true

  defp name_rest?(<<c, rest::binary>>)
       when c in ?a..?z or c in ?A..?Z or c in ?0..?9 or c == ?_ or c == ?.,
       do: name_rest?(rest)

  defp name_rest?(_), do: false
end
