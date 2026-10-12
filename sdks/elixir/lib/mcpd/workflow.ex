defmodule Mcpd.Workflow do
  @moduledoc """
  The authoring surface for durable workflows.

  A workflow is data the daemon holds and bodies the daemon never sees. The
  builder produces both from one module: `document/1` is the definition the
  daemon walks, in the shape `spec/definition.schema.json` describes, and the
  handlers stay in this process for a `Mcpd.StepWorker` to serve. There is no
  YAML to keep in step with the code, because the code is where the graph is
  written.

  Every construct of the run contract has a function here. A branch, a foreach
  body and a loop body are one claimable step each; a multi-step body is a
  `subworkflow/4`.

      wf =
        Mcpd.Workflow.new("support_ticket")
        |> Mcpd.Workflow.step("classify", fn _ctx, input, _prior ->
          %{"kind" => if(String.starts_with?(input["ticket"], "Q"), do: "question", else: "bug")}
        end)
        |> Mcpd.Workflow.agent("draft", &draft/3, input: %{"kind" => "${steps.classify.output.kind}"})
        |> Mcpd.Workflow.output(%{"answer" => "${steps.draft.output}"})

  A handler is a three-arity function: the `Mcpd.StepContext`, the rendered
  input and the prior steps. It returns the step's output. A raised
  `Mcpd.StepFailure` carries its own class; any other raise is `fatal`.
  """

  alias Mcpd.DefinitionDocument

  @type handler :: (Mcpd.StepContext.t(), term(), term() -> term())
  @type predicate :: map()

  @type t :: %__MODULE__{
          id: String.t(),
          version: non_neg_integer() | nil,
          input_schema: term(),
          limits: map() | nil,
          nodes: [map()],
          handlers: %{optional(String.t()) => handler()},
          body_of: %{optional(String.t()) => String.t()},
          output: term()
        }

  defstruct id: nil,
            version: nil,
            input_schema: nil,
            limits: nil,
            nodes: [],
            handlers: %{},
            body_of: %{},
            output: :none

  defmodule Branch do
    @moduledoc "What a parallel branch, a foreach body or a loop body may hold: one claimable step each."
    @type t :: %__MODULE__{nodes: [map()], handlers: map()}
    defstruct nodes: [], handlers: %{}
  end

  # ------------------------------------------------------------ predicates ---

  @doc """
  A leaf predicate: `where("steps.order.output.amount_usd", "gt", 100)`.

  A path reads `input`, `steps.<id>`, `run`, and `item`, `index` or
  `iter.index` where they apply. `op` is one of eq, ne, lt, lte, gt, gte, in,
  matches.
  """
  @spec where(String.t(), String.t() | atom(), term()) :: predicate()
  def where(path, op, value), do: %{"path" => path, "op" => to_string(op), "value" => value}

  @doc "A leaf predicate that holds when the path has a value."
  @spec exists(String.t()) :: predicate()
  def exists(path), do: %{"path" => path, "op" => "exists"}

  @doc "Holds when every predicate holds."
  @spec all_of([predicate()]) :: predicate()
  def all_of(predicates) when is_list(predicates), do: %{"all" => predicates}

  @doc "Holds when at least one predicate holds."
  @spec any_of([predicate()]) :: predicate()
  def any_of(predicates) when is_list(predicates), do: %{"any" => predicates}

  @doc "Holds when the predicate does not."
  @spec negate(predicate()) :: predicate()
  def negate(predicate), do: %{"not" => predicate}

  # -------------------------------------------------------------- builder ---

  @doc """
  Start building a workflow. `id` is unique within the project.

  Options: `version` names the version this document owns (absent, the daemon
  assigns one); `input_schema` is a JSON Schema the run's input must satisfy;
  `limits` is a keyword or map of `max_steps`, `run_timeout_ms`, `max_cost_usd`.
  """
  @spec new(String.t(), keyword()) :: t()
  def new(id, opts \\ []) do
    %__MODULE__{
      id: check_id!(id),
      version: opts[:version],
      input_schema: opts[:input_schema],
      limits: opts[:limits] && Map.new(opts[:limits], fn {k, v} -> {to_string(k), v} end)
    }
  end

  @doc """
  A claimable step a worker serves.

  Options: `when` (a predicate), `input` (a template; default the run's
  input), `effect` (`:at_least_once` default, `:at_most_once`),
  `step_timeout_ms`, `retry` (`max_attempts`, `backoff`, `initial_ms`,
  `max_ms`) and `on_error` (`:fail_run` default, `:continue`).
  """
  @spec step(t() | Branch.t(), String.t(), handler(), keyword()) :: t() | Branch.t()
  def step(target, id, handler, opts \\ []) when is_function(handler, 3),
    do: add(target, "task", id, handler, opts)

  @doc "A claimable step recorded as an agent turn: your own loop, however many turns, as one durable step."
  @spec agent(t() | Branch.t(), String.t(), handler(), keyword()) :: t() | Branch.t()
  def agent(target, id, handler, opts \\ []) when is_function(handler, 3),
    do: add(target, "agent", id, handler, opts)

  defp add(%Branch{} = b, kind, id, handler, opts) do
    id = check_id!(id)
    %Branch{b | nodes: b.nodes ++ [claimable(kind, id, opts)], handlers: Map.put(b.handlers, id, handler)}
  end

  defp add(%__MODULE__{} = wf, kind, id, handler, opts) do
    id = check_id!(id)
    %__MODULE__{wf | nodes: wf.nodes ++ [claimable(kind, id, opts)], handlers: Map.put(wf.handlers, id, handler)}
  end

  @doc "A delay. No process is alive during it."
  @spec sleep(t(), String.t(), non_neg_integer(), keyword()) :: t()
  def sleep(%__MODULE__{} = wf, id, ms, opts \\ []),
    do: push(wf, node("sleep", check_id!(id), %{"ms" => ms}, opts))

  @doc "A delay until an instant, rendered from a template such as `${input.billing_date}`."
  @spec sleep_until(t(), String.t(), String.t(), keyword()) :: t()
  def sleep_until(%__MODULE__{} = wf, id, at, opts \\ []),
    do: push(wf, node("sleep_until", check_id!(id), %{"at" => at}, opts))

  @doc """
  A pause until an event arrives.

  The daemon holds the deadline and the mailbox; the key is rendered per run,
  so `${input.order_id}` addresses this run and no other. Options: `event`,
  `key`, `timeout_ms` (all required), `accept_parked`, `on_timeout`
  (`:fail` default, `:continue`), `when`, `on_error`.
  """
  @spec wait_for(t(), String.t(), keyword()) :: t()
  def wait_for(%__MODULE__{} = wf, id, opts) do
    fields = %{
      "event" => Keyword.fetch!(opts, :event),
      "key" => Keyword.fetch!(opts, :key),
      "timeout_ms" => Keyword.fetch!(opts, :timeout_ms)
    }

    fields =
      fields
      |> put_if("accept_parked", opts[:accept_parked])
      |> put_if("on_timeout", plain(opts[:on_timeout]))

    push(wf, node("wait_event", check_id!(id), fields, opts))
  end

  @doc "Named branches, each one claimable step, all on disk before any is offered. `build` receives a `Branch` and returns it."
  @spec parallel(t(), String.t(), (Branch.t() -> Branch.t()), keyword()) :: t()
  def parallel(%__MODULE__{} = wf, id, build, opts \\ []) when is_function(build, 1) do
    %Branch{} = b = build.(%Branch{})
    wf = push(wf, node("parallel", check_id!(id), %{"branches" => b.nodes}, opts))
    %__MODULE__{wf | handlers: Map.merge(wf.handlers, b.handlers)}
  end

  @doc """
  One claimable step per element of the array at `over`.

  The body's input template can read `item` and `index`; afterwards
  `steps.<id>.outputs` is the array of outputs in item order. Options:
  `over` and `max_items` (required), `concurrency`, `when`, `on_error`.
  """
  @spec foreach(t(), String.t(), (Branch.t() -> Branch.t()), keyword()) :: t()
  def foreach(%__MODULE__{} = wf, id, build, opts) when is_function(build, 1) do
    {body, handlers} = one_body!(id, "foreach", build)

    fields =
      %{"over" => Keyword.fetch!(opts, :over), "body" => body, "max_items" => Keyword.fetch!(opts, :max_items)}
      |> put_if("concurrency", opts[:concurrency])

    wf |> push(node("foreach", check_id!(id), fields, opts)) |> adopt(id, handlers)
  end

  @doc """
  A bounded loop. `until` is evaluated after each iteration and reads
  `steps.<body id>` for that iteration; `iter.index` is the iteration number.
  Options: `max_iterations` (required), `until`, `when`, `on_error`.
  """
  @spec loop(t(), String.t(), (Branch.t() -> Branch.t()), keyword()) :: t()
  def loop(%__MODULE__{} = wf, id, build, opts) when is_function(build, 1) do
    {body, handlers} = one_body!(id, "loop", build)

    fields =
      %{"body" => body, "max_iterations" => Keyword.fetch!(opts, :max_iterations)}
      |> put_if("until", opts[:until])

    wf |> push(node("loop", check_id!(id), fields, opts)) |> adopt(id, handlers)
  end

  @doc "A child run of another definition. Its output becomes `steps.<id>.output`. Options: `version`, `input`, `when`, `on_error`."
  @spec subworkflow(t(), String.t(), String.t(), keyword()) :: t()
  def subworkflow(%__MODULE__{} = wf, id, definition, opts \\ []) do
    fields =
      %{"definition" => definition}
      |> put_if("version", opts[:version])
      |> put_if("input", opts[:input])

    push(wf, node("subworkflow", check_id!(id), fields, opts))
  end

  @doc "The template rendered when the run reaches the end of the graph."
  @spec output(t(), term()) :: t()
  def output(%__MODULE__{} = wf, template), do: %__MODULE__{wf | output: template}

  @doc "The definition document the daemon stores and walks."
  @spec document(t()) :: DefinitionDocument.t()
  def document(%__MODULE__{} = wf) do
    body =
      %{"steps" => wf.nodes}
      |> put_if("input_schema", wf.input_schema)
      |> put_if("limits", wf.limits)
      |> then(fn b -> if wf.output == :none, do: b, else: Map.put(b, "output", wf.output) end)

    %DefinitionDocument{id: wf.id, kind: :workflow, version: wf.version, body: body}
  end

  @doc """
  The handler a lease's step key addresses. `order`, `checks/fraud`,
  `notify[3]` and `refine[2]/rewrite` each resolve to the lexical id that
  registered the body.
  """
  @spec handler_for(t(), String.t()) :: handler() | nil
  def handler_for(%__MODULE__{} = wf, step_key) do
    {head, sub} =
      case String.split(step_key, "/", parts: 2) do
        [h, s] -> {h, s}
        [h] -> {h, nil}
      end

    [root | rest] = String.split(head, "[", parts: 2)

    cond do
      sub != nil -> Map.get(wf.handlers, sub)
      rest != [] -> Map.get(wf.handlers, Map.get(wf.body_of, root, root))
      true -> Map.get(wf.handlers, root)
    end
  end

  # ------------------------------------------------------------ internals ---

  defp push(wf, node), do: %__MODULE__{wf | nodes: wf.nodes ++ [node]}

  defp adopt(wf, id, handlers) do
    Enum.reduce(handlers, wf, fn {body_id, handler}, acc ->
      %__MODULE__{
        acc
        | handlers: Map.put(acc.handlers, body_id, handler),
          body_of: Map.put(acc.body_of, id, body_id)
      }
    end)
  end

  defp one_body!(id, what, build) do
    %Branch{} = b = build.(%Branch{})

    case b.nodes do
      [body] -> {body, b.handlers}
      _ -> raise ArgumentError, "#{what} \"#{id}\" needs exactly one body step; use a subworkflow for more"
    end
  end

  defp node(kind, id, fields, opts) do
    %{"kind" => kind, "id" => id}
    |> Map.merge(fields)
    |> common(opts)
  end

  defp claimable(kind, id, opts) do
    retry =
      case opts[:retry] do
        nil -> nil
        r -> r |> Enum.map(fn {k, v} -> {to_string(k), plain(v)} end) |> Map.new()
      end

    %{"kind" => kind, "id" => id}
    |> common(opts)
    |> put_if("input", opts[:input])
    |> put_if("effect", plain(opts[:effect]))
    |> put_if("step_timeout_ms", opts[:step_timeout_ms])
    |> put_if("retry", retry)
  end

  defp common(node, opts) do
    node
    |> put_if("when", opts[:when])
    |> put_if("on_error", plain(opts[:on_error]))
  end

  defp put_if(map, _key, nil), do: map
  defp put_if(map, key, value), do: Map.put(map, key, value)

  defp plain(nil), do: nil
  defp plain(atom) when is_atom(atom), do: Atom.to_string(atom)
  defp plain(other), do: other

  defp check_id!(id) when is_binary(id) do
    if Regex.match?(~r/^[a-z][a-z0-9_]*$/, id),
      do: id,
      else: raise(ArgumentError, "\"#{id}\" is not a step id; an id matches ^[a-z][a-z0-9_]*$")
  end

  defp check_id!(other), do: raise(ArgumentError, "#{inspect(other)} is not a step id")
end
