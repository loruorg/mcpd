# GENERATED FILE. DO NOT EDIT.
#
# Produced from spec/mcpd.spec.json by tools/codegen.
# Edit the spec and run `make gen`. `make check` fails if this file drifts.

# credo:disable-for-this-file

defmodule Mcpd.Spec do
  @moduledoc """
  The version of `spec/mcpd.spec.json` every module in this file came from.

  A daemon reports the same string as `spec_version` on `/healthz`. Compare
  the two at connect time. A mismatch caught there names the problem; the
  same mismatch found later shows up as a field missing for no visible
  reason.
  """

  @spec_version "1.0.0"

  @doc "The spec version this SDK was generated from."
  @spec spec_version() :: String.t()
  def spec_version, do: @spec_version
end

defmodule Mcpd.SpanKind do
  @moduledoc """
  What an operation *is*. The kind drives how a span is rendered in the waterfall, which attributes are meaningful, and which metrics are auto-extracted from it.
  """

  @type t ::
          :agent_run |
          :model_generation |
          :model_step |
          :model_inference |
          :model_chunk |
          :tool_call |
          :client_tool_call |
          :provider_tool_call |
          :mcp_tool_call |
          :workflow_run |
          :workflow_step |
          :workflow_conditional |
          :workflow_conditional_eval |
          :workflow_parallel |
          :workflow_loop |
          :workflow_sleep |
          :workflow_wait_event |
          :memory_operation |
          :rag_ingestion |
          :rag_embedding |
          :rag_vector_operation |
          :rag_action |
          :graph_action |
          :processor_run |
          :scorer_run |
          :scorer_step |
          :guardrail |
          :retrieval |
          :mapping |
          :skill_resolution |
          :workspace_action |
          :http_request |
          :generic

  @values [
    :agent_run,
    :model_generation,
    :model_step,
    :model_inference,
    :model_chunk,
    :tool_call,
    :client_tool_call,
    :provider_tool_call,
    :mcp_tool_call,
    :workflow_run,
    :workflow_step,
    :workflow_conditional,
    :workflow_conditional_eval,
    :workflow_parallel,
    :workflow_loop,
    :workflow_sleep,
    :workflow_wait_event,
    :memory_operation,
    :rag_ingestion,
    :rag_embedding,
    :rag_vector_operation,
    :rag_action,
    :graph_action,
    :processor_run,
    :scorer_run,
    :scorer_step,
    :guardrail,
    :retrieval,
    :mapping,
    :skill_resolution,
    :workspace_action,
    :http_request,
    :generic,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:agent_run), do: "agent_run"
  def to_wire(:model_generation), do: "model_generation"
  def to_wire(:model_step), do: "model_step"
  def to_wire(:model_inference), do: "model_inference"
  def to_wire(:model_chunk), do: "model_chunk"
  def to_wire(:tool_call), do: "tool_call"
  def to_wire(:client_tool_call), do: "client_tool_call"
  def to_wire(:provider_tool_call), do: "provider_tool_call"
  def to_wire(:mcp_tool_call), do: "mcp_tool_call"
  def to_wire(:workflow_run), do: "workflow_run"
  def to_wire(:workflow_step), do: "workflow_step"
  def to_wire(:workflow_conditional), do: "workflow_conditional"
  def to_wire(:workflow_conditional_eval), do: "workflow_conditional_eval"
  def to_wire(:workflow_parallel), do: "workflow_parallel"
  def to_wire(:workflow_loop), do: "workflow_loop"
  def to_wire(:workflow_sleep), do: "workflow_sleep"
  def to_wire(:workflow_wait_event), do: "workflow_wait_event"
  def to_wire(:memory_operation), do: "memory_operation"
  def to_wire(:rag_ingestion), do: "rag_ingestion"
  def to_wire(:rag_embedding), do: "rag_embedding"
  def to_wire(:rag_vector_operation), do: "rag_vector_operation"
  def to_wire(:rag_action), do: "rag_action"
  def to_wire(:graph_action), do: "graph_action"
  def to_wire(:processor_run), do: "processor_run"
  def to_wire(:scorer_run), do: "scorer_run"
  def to_wire(:scorer_step), do: "scorer_step"
  def to_wire(:guardrail), do: "guardrail"
  def to_wire(:retrieval), do: "retrieval"
  def to_wire(:mapping), do: "mapping"
  def to_wire(:skill_resolution), do: "skill_resolution"
  def to_wire(:workspace_action), do: "workspace_action"
  def to_wire(:http_request), do: "http_request"
  def to_wire(:generic), do: "generic"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("agent_run"), do: {:ok, :agent_run}
  def from_wire("model_generation"), do: {:ok, :model_generation}
  def from_wire("model_step"), do: {:ok, :model_step}
  def from_wire("model_inference"), do: {:ok, :model_inference}
  def from_wire("model_chunk"), do: {:ok, :model_chunk}
  def from_wire("tool_call"), do: {:ok, :tool_call}
  def from_wire("client_tool_call"), do: {:ok, :client_tool_call}
  def from_wire("provider_tool_call"), do: {:ok, :provider_tool_call}
  def from_wire("mcp_tool_call"), do: {:ok, :mcp_tool_call}
  def from_wire("workflow_run"), do: {:ok, :workflow_run}
  def from_wire("workflow_step"), do: {:ok, :workflow_step}
  def from_wire("workflow_conditional"), do: {:ok, :workflow_conditional}
  def from_wire("workflow_conditional_eval"), do: {:ok, :workflow_conditional_eval}
  def from_wire("workflow_parallel"), do: {:ok, :workflow_parallel}
  def from_wire("workflow_loop"), do: {:ok, :workflow_loop}
  def from_wire("workflow_sleep"), do: {:ok, :workflow_sleep}
  def from_wire("workflow_wait_event"), do: {:ok, :workflow_wait_event}
  def from_wire("memory_operation"), do: {:ok, :memory_operation}
  def from_wire("rag_ingestion"), do: {:ok, :rag_ingestion}
  def from_wire("rag_embedding"), do: {:ok, :rag_embedding}
  def from_wire("rag_vector_operation"), do: {:ok, :rag_vector_operation}
  def from_wire("rag_action"), do: {:ok, :rag_action}
  def from_wire("graph_action"), do: {:ok, :graph_action}
  def from_wire("processor_run"), do: {:ok, :processor_run}
  def from_wire("scorer_run"), do: {:ok, :scorer_run}
  def from_wire("scorer_step"), do: {:ok, :scorer_step}
  def from_wire("guardrail"), do: {:ok, :guardrail}
  def from_wire("retrieval"), do: {:ok, :retrieval}
  def from_wire("mapping"), do: {:ok, :mapping}
  def from_wire("skill_resolution"), do: {:ok, :skill_resolution}
  def from_wire("workspace_action"), do: {:ok, :workspace_action}
  def from_wire("http_request"), do: {:ok, :http_request}
  def from_wire("generic"), do: {:ok, :generic}
  def from_wire(_), do: :error
end

defmodule Mcpd.SpanStatus do
  @moduledoc """
  The outcome of an operation.
  """

  @type t :: :unset | :ok | :error | :cancelled

  @values [
    :unset,
    :ok,
    :error,
    :cancelled,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:unset), do: "unset"
  def to_wire(:ok), do: "ok"
  def to_wire(:error), do: "error"
  def to_wire(:cancelled), do: "cancelled"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("unset"), do: {:ok, :unset}
  def from_wire("ok"), do: {:ok, :ok}
  def from_wire("error"), do: {:ok, :error}
  def from_wire("cancelled"), do: {:ok, :cancelled}
  def from_wire(_), do: :error
end

defmodule Mcpd.SignalType do
  @moduledoc """
  The six signals mcpd stores. Every one of them is correlated by trace id, most of them by span id as well, and every one of them passes through the same redaction pipeline.
  """

  @type t :: :span | :log | :metric | :score | :feedback | :aspect_note

  @values [
    :span,
    :log,
    :metric,
    :score,
    :feedback,
    :aspect_note,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:span), do: "span"
  def to_wire(:log), do: "log"
  def to_wire(:metric), do: "metric"
  def to_wire(:score), do: "score"
  def to_wire(:feedback), do: "feedback"
  def to_wire(:aspect_note), do: "aspect_note"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("span"), do: {:ok, :span}
  def from_wire("log"), do: {:ok, :log}
  def from_wire("metric"), do: {:ok, :metric}
  def from_wire("score"), do: {:ok, :score}
  def from_wire("feedback"), do: {:ok, :feedback}
  def from_wire("aspect_note"), do: {:ok, :aspect_note}
  def from_wire(_), do: :error
end

defmodule Mcpd.LogLevel do
  @moduledoc """
  Severity, ordered. Filtering is always `>=`.
  """

  @type t :: :trace | :debug | :info | :warn | :error | :fatal

  @values [
    :trace,
    :debug,
    :info,
    :warn,
    :error,
    :fatal,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:trace), do: "trace"
  def to_wire(:debug), do: "debug"
  def to_wire(:info), do: "info"
  def to_wire(:warn), do: "warn"
  def to_wire(:error), do: "error"
  def to_wire(:fatal), do: "fatal"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("trace"), do: {:ok, :trace}
  def from_wire("debug"), do: {:ok, :debug}
  def from_wire("info"), do: {:ok, :info}
  def from_wire("warn"), do: {:ok, :warn}
  def from_wire("error"), do: {:ok, :error}
  def from_wire("fatal"), do: {:ok, :fatal}
  def from_wire(_), do: :error

  @doc "Ordered severity. Filtering is always `>=`."
  @spec severity(t()) :: integer()
  def severity(:trace), do: 1
  def severity(:debug), do: 5
  def severity(:info), do: 9
  def severity(:warn), do: 13
  def severity(:error), do: 17
  def severity(:fatal), do: 21
end

defmodule Mcpd.MetricKind do
  @moduledoc """
  How a numeric observation aggregates.
  """

  @type t :: :counter | :gauge | :histogram

  @values [
    :counter,
    :gauge,
    :histogram,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:counter), do: "counter"
  def to_wire(:gauge), do: "gauge"
  def to_wire(:histogram), do: "histogram"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("counter"), do: {:ok, :counter}
  def from_wire("gauge"), do: {:ok, :gauge}
  def from_wire("histogram"), do: {:ok, :histogram}
  def from_wire(_), do: :error
end

defmodule Mcpd.SamplingKind do
  @moduledoc """
  How mcpd decides which traces to keep.
  """

  @type t :: :always | :never | :ratio | :deterministic | :tail

  @values [
    :always,
    :never,
    :ratio,
    :deterministic,
    :tail,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:always), do: "always"
  def to_wire(:never), do: "never"
  def to_wire(:ratio), do: "ratio"
  def to_wire(:deterministic), do: "deterministic"
  def to_wire(:tail), do: "tail"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("always"), do: {:ok, :always}
  def from_wire("never"), do: {:ok, :never}
  def from_wire("ratio"), do: {:ok, :ratio}
  def from_wire("deterministic"), do: {:ok, :deterministic}
  def from_wire("tail"), do: {:ok, :tail}
  def from_wire(_), do: :error
end

defmodule Mcpd.RedactionMode do
  @moduledoc """
  What happens to a value a redaction rule matched.
  """

  @type t :: :drop | :mask | :partial | :hash

  @values [
    :drop,
    :mask,
    :partial,
    :hash,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:drop), do: "drop"
  def to_wire(:mask), do: "mask"
  def to_wire(:partial), do: "partial"
  def to_wire(:hash), do: "hash"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("drop"), do: {:ok, :drop}
  def from_wire("mask"), do: {:ok, :mask}
  def from_wire("partial"), do: {:ok, :partial}
  def from_wire("hash"), do: {:ok, :hash}
  def from_wire(_), do: :error
end

defmodule Mcpd.ScoreSource do
  @moduledoc """
  Where a score came from. Kept separate from the scorer's identity so the same scorer can run in more than one context.
  """

  @type t :: :live | :offline | :experiment | :manual

  @values [
    :live,
    :offline,
    :experiment,
    :manual,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:live), do: "live"
  def to_wire(:offline), do: "offline"
  def to_wire(:experiment), do: "experiment"
  def to_wire(:manual), do: "manual"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("live"), do: {:ok, :live}
  def from_wire("offline"), do: {:ok, :offline}
  def from_wire("experiment"), do: {:ok, :experiment}
  def from_wire("manual"), do: {:ok, :manual}
  def from_wire(_), do: :error
end

defmodule Mcpd.ScorerKind do
  @moduledoc """
  How a scorer reaches its answer.
  """

  @type t :: :code | :judge | :criterion

  @values [
    :code,
    :judge,
    :criterion,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:code), do: "code"
  def to_wire(:judge), do: "judge"
  def to_wire(:criterion), do: "criterion"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("code"), do: {:ok, :code}
  def from_wire("judge"), do: {:ok, :judge}
  def from_wire("criterion"), do: {:ok, :criterion}
  def from_wire(_), do: :error
end

defmodule Mcpd.ScoreDirection do
  @moduledoc """
  Which way a scorer's value is better. A comparison reads a fall in `toxicity` as an improvement and a fall in `faithfulness` as a regression.
  """

  @type t :: :higher_is_better | :lower_is_better

  @values [
    :higher_is_better,
    :lower_is_better,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:higher_is_better), do: "higher_is_better"
  def to_wire(:lower_is_better), do: "lower_is_better"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("higher_is_better"), do: {:ok, :higher_is_better}
  def from_wire("lower_is_better"), do: {:ok, :lower_is_better}
  def from_wire(_), do: :error
end

defmodule Mcpd.FeedbackKind do
  @moduledoc """
  The shape of a human judgement.
  """

  @type t :: :thumbs | :rating | :correction | :tag | :comment

  @values [
    :thumbs,
    :rating,
    :correction,
    :tag,
    :comment,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:thumbs), do: "thumbs"
  def to_wire(:rating), do: "rating"
  def to_wire(:correction), do: "correction"
  def to_wire(:tag), do: "tag"
  def to_wire(:comment), do: "comment"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("thumbs"), do: {:ok, :thumbs}
  def from_wire("rating"), do: {:ok, :rating}
  def from_wire("correction"), do: {:ok, :correction}
  def from_wire("tag"), do: {:ok, :tag}
  def from_wire("comment"), do: {:ok, :comment}
  def from_wire(_), do: :error
end

defmodule Mcpd.EntityKind do
  @moduledoc """
  The named thing a span belongs to. Bounded and low-cardinality, so it is safe as a metric label.
  """

  @type t :: :agent | :workflow | :tool | :model | :scorer | :processor | :pipeline | :service

  @values [
    :agent,
    :workflow,
    :tool,
    :model,
    :scorer,
    :processor,
    :pipeline,
    :service,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:agent), do: "agent"
  def to_wire(:workflow), do: "workflow"
  def to_wire(:tool), do: "tool"
  def to_wire(:model), do: "model"
  def to_wire(:scorer), do: "scorer"
  def to_wire(:processor), do: "processor"
  def to_wire(:pipeline), do: "pipeline"
  def to_wire(:service), do: "service"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("agent"), do: {:ok, :agent}
  def from_wire("workflow"), do: {:ok, :workflow}
  def from_wire("tool"), do: {:ok, :tool}
  def from_wire("model"), do: {:ok, :model}
  def from_wire("scorer"), do: {:ok, :scorer}
  def from_wire("processor"), do: {:ok, :processor}
  def from_wire("pipeline"), do: {:ok, :pipeline}
  def from_wire("service"), do: {:ok, :service}
  def from_wire(_), do: :error
end

defmodule Mcpd.ExperimentStatus do
  @moduledoc """
  Where an experiment run has got to.
  """

  @type t :: :pending | :running | :succeeded | :failed | :cancelled

  @values [
    :pending,
    :running,
    :succeeded,
    :failed,
    :cancelled,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:pending), do: "pending"
  def to_wire(:running), do: "running"
  def to_wire(:succeeded), do: "succeeded"
  def to_wire(:failed), do: "failed"
  def to_wire(:cancelled), do: "cancelled"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("pending"), do: {:ok, :pending}
  def from_wire("running"), do: {:ok, :running}
  def from_wire("succeeded"), do: {:ok, :succeeded}
  def from_wire("failed"), do: {:ok, :failed}
  def from_wire("cancelled"), do: {:ok, :cancelled}
  def from_wire(_), do: :error
end

defmodule Mcpd.AuditResult do
  @moduledoc """
  Whether authorisation let the request through.
  """

  @type t :: :allowed | :refused

  @values [
    :allowed,
    :refused,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:allowed), do: "allowed"
  def to_wire(:refused), do: "refused"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("allowed"), do: {:ok, :allowed}
  def from_wire("refused"), do: {:ok, :refused}
  def from_wire(_), do: :error
end

defmodule Mcpd.RunStatus do
  @moduledoc """
  Where a run is. Only `running` can move; the other four are terminal, and a terminal run holds no deadline, no wait and no live attempt.
  """

  @type t :: :running | :succeeded | :failed | :timed_out | :cancelled

  @values [
    :running,
    :succeeded,
    :failed,
    :timed_out,
    :cancelled,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:running), do: "running"
  def to_wire(:succeeded), do: "succeeded"
  def to_wire(:failed), do: "failed"
  def to_wire(:timed_out), do: "timed_out"
  def to_wire(:cancelled), do: "cancelled"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("running"), do: {:ok, :running}
  def from_wire("succeeded"), do: {:ok, :succeeded}
  def from_wire("failed"), do: {:ok, :failed}
  def from_wire("timed_out"), do: {:ok, :timed_out}
  def from_wire("cancelled"), do: {:ok, :cancelled}
  def from_wire(_), do: :error
end

defmodule Mcpd.AttemptStatus do
  @moduledoc """
  Where one attempt at one step is. Four states are live and four are terminal, and the daemon never shows a live attempt on an ended run.
  """

  @type t ::
          :pending |
          :leased |
          :started |
          :orphaned |
          :succeeded |
          :failed |
          :timed_out |
          :skipped

  @values [
    :pending,
    :leased,
    :started,
    :orphaned,
    :succeeded,
    :failed,
    :timed_out,
    :skipped,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:pending), do: "pending"
  def to_wire(:leased), do: "leased"
  def to_wire(:started), do: "started"
  def to_wire(:orphaned), do: "orphaned"
  def to_wire(:succeeded), do: "succeeded"
  def to_wire(:failed), do: "failed"
  def to_wire(:timed_out), do: "timed_out"
  def to_wire(:skipped), do: "skipped"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("pending"), do: {:ok, :pending}
  def from_wire("leased"), do: {:ok, :leased}
  def from_wire("started"), do: {:ok, :started}
  def from_wire("orphaned"), do: {:ok, :orphaned}
  def from_wire("succeeded"), do: {:ok, :succeeded}
  def from_wire("failed"), do: {:ok, :failed}
  def from_wire("timed_out"), do: {:ok, :timed_out}
  def from_wire("skipped"), do: {:ok, :skipped}
  def from_wire(_), do: :error
end

defmodule Mcpd.StepKind do
  @moduledoc """
  What a step attempt is an attempt at. `task` and `agent` are claimable. A `wait` row is written by the event that resumes it or the deadline that ends it, so a wait's output and status read like any other step's.
  """

  @type t :: :task | :agent | :wait

  @values [
    :task,
    :agent,
    :wait,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:task), do: "task"
  def to_wire(:agent), do: "agent"
  def to_wire(:wait), do: "wait"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("task"), do: {:ok, :task}
  def from_wire("agent"), do: {:ok, :agent}
  def from_wire("wait"), do: {:ok, :wait}
  def from_wire(_), do: :error
end

defmodule Mcpd.ConstructKind do
  @moduledoc """
  What a run's cursor names: the construct at its current position in the definition's `steps`. Every kind except `end` is a node kind a definition can declare.
  """

  @type t ::
          :task |
          :agent |
          :sleep |
          :sleep_until |
          :wait_event |
          :parallel |
          :foreach |
          :loop |
          :subworkflow |
          :end

  @values [
    :task,
    :agent,
    :sleep,
    :sleep_until,
    :wait_event,
    :parallel,
    :foreach,
    :loop,
    :subworkflow,
    :end,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:task), do: "task"
  def to_wire(:agent), do: "agent"
  def to_wire(:sleep), do: "sleep"
  def to_wire(:sleep_until), do: "sleep_until"
  def to_wire(:wait_event), do: "wait_event"
  def to_wire(:parallel), do: "parallel"
  def to_wire(:foreach), do: "foreach"
  def to_wire(:loop), do: "loop"
  def to_wire(:subworkflow), do: "subworkflow"
  def to_wire(:end), do: "end"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("task"), do: {:ok, :task}
  def from_wire("agent"), do: {:ok, :agent}
  def from_wire("sleep"), do: {:ok, :sleep}
  def from_wire("sleep_until"), do: {:ok, :sleep_until}
  def from_wire("wait_event"), do: {:ok, :wait_event}
  def from_wire("parallel"), do: {:ok, :parallel}
  def from_wire("foreach"), do: {:ok, :foreach}
  def from_wire("loop"), do: {:ok, :loop}
  def from_wire("subworkflow"), do: {:ok, :subworkflow}
  def from_wire("end"), do: {:ok, :end}
  def from_wire(_), do: :error
end

defmodule Mcpd.Effect do
  @moduledoc """
  What running a step body twice would cost, which is what the daemon branches on when a lease is lost. Default `at_least_once`.
  """

  @type t :: :at_least_once | :at_most_once

  @values [
    :at_least_once,
    :at_most_once,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:at_least_once), do: "at_least_once"
  def to_wire(:at_most_once), do: "at_most_once"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("at_least_once"), do: {:ok, :at_least_once}
  def from_wire("at_most_once"), do: {:ok, :at_most_once}
  def from_wire(_), do: :error
end

defmodule Mcpd.TimerKind do
  @moduledoc """
  Every deadline the daemon keeps. Each one is a row the tick fires, and each firing is a transaction that moves the run.
  """

  @type t ::
          :sleep |
          :lease_expiry |
          :reclaim |
          :run_timeout |
          :step_timeout |
          :wait_timeout |
          :retry

  @values [
    :sleep,
    :lease_expiry,
    :reclaim,
    :run_timeout,
    :step_timeout,
    :wait_timeout,
    :retry,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:sleep), do: "sleep"
  def to_wire(:lease_expiry), do: "lease_expiry"
  def to_wire(:reclaim), do: "reclaim"
  def to_wire(:run_timeout), do: "run_timeout"
  def to_wire(:step_timeout), do: "step_timeout"
  def to_wire(:wait_timeout), do: "wait_timeout"
  def to_wire(:retry), do: "retry"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("sleep"), do: {:ok, :sleep}
  def from_wire("lease_expiry"), do: {:ok, :lease_expiry}
  def from_wire("reclaim"), do: {:ok, :reclaim}
  def from_wire("run_timeout"), do: {:ok, :run_timeout}
  def from_wire("step_timeout"), do: {:ok, :step_timeout}
  def from_wire("wait_timeout"), do: {:ok, :wait_timeout}
  def from_wire("retry"), do: {:ok, :retry}
  def from_wire(_), do: :error
end

defmodule Mcpd.RunEventKind do
  @moduledoc """
  What a row in a run's event log records. Every row is written in the transaction that made the change it describes, so a reader that has seen sequence number n has seen every change below it.
  """

  @type t ::
          :run_started |
          :run_cursor |
          :step_leased |
          :step_started |
          :step_finished |
          :step_failed |
          :step_skipped |
          :run_waiting |
          :run_resumed |
          :run_ended

  @values [
    :run_started,
    :run_cursor,
    :step_leased,
    :step_started,
    :step_finished,
    :step_failed,
    :step_skipped,
    :run_waiting,
    :run_resumed,
    :run_ended,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:run_started), do: "run_started"
  def to_wire(:run_cursor), do: "run_cursor"
  def to_wire(:step_leased), do: "step_leased"
  def to_wire(:step_started), do: "step_started"
  def to_wire(:step_finished), do: "step_finished"
  def to_wire(:step_failed), do: "step_failed"
  def to_wire(:step_skipped), do: "step_skipped"
  def to_wire(:run_waiting), do: "run_waiting"
  def to_wire(:run_resumed), do: "run_resumed"
  def to_wire(:run_ended), do: "run_ended"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("run_started"), do: {:ok, :run_started}
  def from_wire("run_cursor"), do: {:ok, :run_cursor}
  def from_wire("step_leased"), do: {:ok, :step_leased}
  def from_wire("step_started"), do: {:ok, :step_started}
  def from_wire("step_finished"), do: {:ok, :step_finished}
  def from_wire("step_failed"), do: {:ok, :step_failed}
  def from_wire("step_skipped"), do: {:ok, :step_skipped}
  def from_wire("run_waiting"), do: {:ok, :run_waiting}
  def from_wire("run_resumed"), do: {:ok, :run_resumed}
  def from_wire("run_ended"), do: {:ok, :run_ended}
  def from_wire(_), do: :error
end

defmodule Mcpd.DefinitionKind do
  @moduledoc """
  What kind of thing a definition describes.
  """

  @type t :: :workflow

  @values [
    :workflow,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:workflow), do: "workflow"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("workflow"), do: {:ok, :workflow}
  def from_wire(_), do: :error
end

defmodule Mcpd.EventOutcome do
  @moduledoc """
  What the daemon did with a published event.
  """

  @type t :: :delivered | :parked | :duplicate

  @values [
    :delivered,
    :parked,
    :duplicate,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:delivered), do: "delivered"
  def to_wire(:parked), do: "parked"
  def to_wire(:duplicate), do: "duplicate"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("delivered"), do: {:ok, :delivered}
  def from_wire("parked"), do: {:ok, :parked}
  def from_wire("duplicate"), do: {:ok, :duplicate}
  def from_wire(_), do: :error
end

defmodule Mcpd.ConflictReason do
  @moduledoc """
  Why a worker call about a lease answered 409. Each reason tells the worker something different to do next.
  """

  @type t :: :fenced | :already_applied | :not_started | :run_terminal

  @values [
    :fenced,
    :already_applied,
    :not_started,
    :run_terminal,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:fenced), do: "fenced"
  def to_wire(:already_applied), do: "already_applied"
  def to_wire(:not_started), do: "not_started"
  def to_wire(:run_terminal), do: "run_terminal"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("fenced"), do: {:ok, :fenced}
  def from_wire("already_applied"), do: {:ok, :already_applied}
  def from_wire("not_started"), do: {:ok, :not_started}
  def from_wire("run_terminal"), do: {:ok, :run_terminal}
  def from_wire(_), do: :error
end

defmodule Mcpd.StepErrorClass do
  @moduledoc """
  How a step failed, which decides whether the daemon retries it.
  """

  @type t :: :retryable | :fatal | :invalid_input | :unservable | :cancelled

  @values [
    :retryable,
    :fatal,
    :invalid_input,
    :unservable,
    :cancelled,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:retryable), do: "retryable"
  def to_wire(:fatal), do: "fatal"
  def to_wire(:invalid_input), do: "invalid_input"
  def to_wire(:unservable), do: "unservable"
  def to_wire(:cancelled), do: "cancelled"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("retryable"), do: {:ok, :retryable}
  def from_wire("fatal"), do: {:ok, :fatal}
  def from_wire("invalid_input"), do: {:ok, :invalid_input}
  def from_wire("unservable"), do: {:ok, :unservable}
  def from_wire("cancelled"), do: {:ok, :cancelled}
  def from_wire(_), do: :error
end

defmodule Mcpd.OnError do
  @moduledoc """
  What happens to a run when a node fails after its retries. Default `fail_run`.
  """

  @type t :: :fail_run | :continue

  @values [
    :fail_run,
    :continue,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:fail_run), do: "fail_run"
  def to_wire(:continue), do: "continue"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("fail_run"), do: {:ok, :fail_run}
  def from_wire("continue"), do: {:ok, :continue}
  def from_wire(_), do: :error
end

defmodule Mcpd.Backoff do
  @moduledoc """
  How the delay between retries grows.
  """

  @type t :: :fixed | :exponential

  @values [
    :fixed,
    :exponential,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:fixed), do: "fixed"
  def to_wire(:exponential), do: "exponential"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("fixed"), do: {:ok, :fixed}
  def from_wire("exponential"), do: {:ok, :exponential}
  def from_wire(_), do: :error
end

defmodule Mcpd.WaitTimeout do
  @moduledoc """
  What a `wait_event` node does when its deadline passes with no event. Default `fail`.
  """

  @type t :: :fail | :continue

  @values [
    :fail,
    :continue,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:fail), do: "fail"
  def to_wire(:continue), do: "continue"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("fail"), do: {:ok, :fail}
  def from_wire("continue"), do: {:ok, :continue}
  def from_wire(_), do: :error
end

defmodule Mcpd.ValueKind do
  @moduledoc """
  The JSON type of a value. Used where mcpd reports the shape of something it holds without returning the thing itself.
  """

  @type t :: :null | :boolean | :number | :string | :array | :object

  @values [
    :null,
    :boolean,
    :number,
    :string,
    :array,
    :object,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:null), do: "null"
  def to_wire(:boolean), do: "boolean"
  def to_wire(:number), do: "number"
  def to_wire(:string), do: "string"
  def to_wire(:array), do: "array"
  def to_wire(:object), do: "object"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("null"), do: {:ok, :null}
  def from_wire("boolean"), do: {:ok, :boolean}
  def from_wire("number"), do: {:ok, :number}
  def from_wire("string"), do: {:ok, :string}
  def from_wire("array"), do: {:ok, :array}
  def from_wire("object"), do: {:ok, :object}
  def from_wire(_), do: :error
end

defmodule Mcpd.MonitorWatch do
  @moduledoc """
  What a monitor measures over its window.
  """

  @type t ::
          :error_rate |
          :latency_p95 |
          :cost |
          :trace_count |
          :score_mean |
          :step_count_divergence |
          :run_repetition

  @values [
    :error_rate,
    :latency_p95,
    :cost,
    :trace_count,
    :score_mean,
    :step_count_divergence,
    :run_repetition,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:error_rate), do: "error_rate"
  def to_wire(:latency_p95), do: "latency_p95"
  def to_wire(:cost), do: "cost"
  def to_wire(:trace_count), do: "trace_count"
  def to_wire(:score_mean), do: "score_mean"
  def to_wire(:step_count_divergence), do: "step_count_divergence"
  def to_wire(:run_repetition), do: "run_repetition"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("error_rate"), do: {:ok, :error_rate}
  def from_wire("latency_p95"), do: {:ok, :latency_p95}
  def from_wire("cost"), do: {:ok, :cost}
  def from_wire("trace_count"), do: {:ok, :trace_count}
  def from_wire("score_mean"), do: {:ok, :score_mean}
  def from_wire("step_count_divergence"), do: {:ok, :step_count_divergence}
  def from_wire("run_repetition"), do: {:ok, :run_repetition}
  def from_wire(_), do: :error
end

defmodule Mcpd.MonitorCompare do
  @moduledoc """
  Which way a monitor's threshold is breached.
  """

  @type t :: :above | :below

  @values [
    :above,
    :below,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:above), do: "above"
  def to_wire(:below), do: "below"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("above"), do: {:ok, :above}
  def from_wire("below"), do: {:ok, :below}
  def from_wire(_), do: :error
end

defmodule Mcpd.MonitorCondition do
  @moduledoc """
  Where a monitor or an objective stands. A tripped condition latches: it stays tripped until the number recovers, and too little data never clears it, because silence is not recovery.
  """

  @type t :: :nominal | :tripped | :acknowledged | :insufficient

  @values [
    :nominal,
    :tripped,
    :acknowledged,
    :insufficient,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:nominal), do: "nominal"
  def to_wire(:tripped), do: "tripped"
  def to_wire(:acknowledged), do: "acknowledged"
  def to_wire(:insufficient), do: "insufficient"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("nominal"), do: {:ok, :nominal}
  def from_wire("tripped"), do: {:ok, :tripped}
  def from_wire("acknowledged"), do: {:ok, :acknowledged}
  def from_wire("insufficient"), do: {:ok, :insufficient}
  def from_wire(_), do: :error
end

defmodule Mcpd.TextSearch do
  @moduledoc """
  What the `q` parameter matches on this deployment's storage backend. Reported by `GET /v1/api/capabilities` so a search box can say what it searches instead of leaving a reader to infer it from a result set.
  """

  @type t :: :none | :substring | :words

  @values [
    :none,
    :substring,
    :words,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:none), do: "none"
  def to_wire(:substring), do: "substring"
  def to_wire(:words), do: "words"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("none"), do: {:ok, :none}
  def from_wire("substring"), do: {:ok, :substring}
  def from_wire("words"), do: {:ok, :words}
  def from_wire(_), do: :error
end

defmodule Mcpd.Partitioning do
  @moduledoc """
  How the storage backend cuts each signal table on its anchor time. It decides whether expiring a day of data is a metadata operation on one part or a row-by-row delete.
  """

  @type t :: :none | :day | :month

  @values [
    :none,
    :day,
    :month,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:none), do: "none"
  def to_wire(:day), do: "day"
  def to_wire(:month), do: "month"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("none"), do: {:ok, :none}
  def from_wire("day"), do: {:ok, :day}
  def from_wire("month"), do: {:ok, :month}
  def from_wire(_), do: :error
end

defmodule Mcpd.Facet do
  @moduledoc """
  A dimension a filter box can offer values for. The set is closed on purpose: each one is a column or a parsed field the daemon keeps a distinct-value list for as signals arrive, so answering is a read of a small table rather than a scan of every span in the retention window.
  """

  @type t ::
          :entity_kind |
          :entity_name |
          :service_name |
          :environment |
          :tag |
          :metric_name |
          :metric_label_key |
          :metric_label_value

  @values [
    :entity_kind,
    :entity_name,
    :service_name,
    :environment,
    :tag,
    :metric_name,
    :metric_label_key,
    :metric_label_value,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:entity_kind), do: "entity_kind"
  def to_wire(:entity_name), do: "entity_name"
  def to_wire(:service_name), do: "service_name"
  def to_wire(:environment), do: "environment"
  def to_wire(:tag), do: "tag"
  def to_wire(:metric_name), do: "metric_name"
  def to_wire(:metric_label_key), do: "metric_label_key"
  def to_wire(:metric_label_value), do: "metric_label_value"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("entity_kind"), do: {:ok, :entity_kind}
  def from_wire("entity_name"), do: {:ok, :entity_name}
  def from_wire("service_name"), do: {:ok, :service_name}
  def from_wire("environment"), do: {:ok, :environment}
  def from_wire("tag"), do: {:ok, :tag}
  def from_wire("metric_name"), do: {:ok, :metric_name}
  def from_wire("metric_label_key"), do: {:ok, :metric_label_key}
  def from_wire("metric_label_value"), do: {:ok, :metric_label_value}
  def from_wire(_), do: :error
end

defmodule Mcpd.DeltaSignal do
  @moduledoc """
  A signal that can be polled for what has arrived. Only the three that carry a server-set arrival time: a score and a piece of feedback are stamped with the caller's own timestamp, so a watermark over them would either skip a late row or repeat every row for ever.
  """

  @type t :: :traces | :spans | :logs

  @values [
    :traces,
    :spans,
    :logs,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:traces), do: "traces"
  def to_wire(:spans), do: "spans"
  def to_wire(:logs), do: "logs"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("traces"), do: {:ok, :traces}
  def from_wire("spans"), do: {:ok, :spans}
  def from_wire("logs"), do: {:ok, :logs}
  def from_wire(_), do: :error
end

defmodule Mcpd.AspectReadiness do
  @moduledoc """
  How far the daemon has got with one aspect for one entity. It is computed from the rows that are stored and never recorded, so it cannot go stale, and it is per entity and per aspect rather than global: one agent can have goal themes to read while its behaviour notes are still collecting.
  """

  @type t :: :collecting | :processing | :ready

  @values [
    :collecting,
    :processing,
    :ready,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:collecting), do: "collecting"
  def to_wire(:processing), do: "processing"
  def to_wire(:ready), do: "ready"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("collecting"), do: {:ok, :collecting}
  def from_wire("processing"), do: {:ok, :processing}
  def from_wire("ready"), do: {:ok, :ready}
  def from_wire(_), do: :error
end

defmodule Mcpd.ThemeState do
  @moduledoc """
  What happened to a theme between the previous snapshot and this one. The daemon matches themes across consecutive snapshots by their wording, so a theme keeps its id while its wording holds, and this records what that match found. It is computed from the two clusterings alone and never from a trace attribute.
  """

  @type t :: :birth | :continue | :split | :merge | :resurrection | :death

  @values [
    :birth,
    :continue,
    :split,
    :merge,
    :resurrection,
    :death,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:birth), do: "birth"
  def to_wire(:continue), do: "continue"
  def to_wire(:split), do: "split"
  def to_wire(:merge), do: "merge"
  def to_wire(:resurrection), do: "resurrection"
  def to_wire(:death), do: "death"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("birth"), do: {:ok, :birth}
  def from_wire("continue"), do: {:ok, :continue}
  def from_wire("split"), do: {:ok, :split}
  def from_wire("merge"), do: {:ok, :merge}
  def from_wire("resurrection"), do: {:ok, :resurrection}
  def from_wire("death"), do: {:ok, :death}
  def from_wire(_), do: :error
end

defmodule Mcpd.ThemeNodeKind do
  @moduledoc """
  What one node of a theme flow stands for. A stage holds its themes, at most one noise node and at most one overflow node, and the three together account for every trace in the stage, which is what lets the shares add up to one.
  """

  @type t :: :theme | :noise | :other

  @values [
    :theme,
    :noise,
    :other,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:theme), do: "theme"
  def to_wire(:noise), do: "noise"
  def to_wire(:other), do: "other"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("theme"), do: {:ok, :theme}
  def from_wire("noise"), do: {:ok, :noise}
  def from_wire("other"), do: {:ok, :other}
  def from_wire(_), do: :error
end

defmodule Mcpd.SnapshotLandmarkReason do
  @moduledoc """
  Why one snapshot is in a landmark selection. A timeline draws a couple of dozen points over a range that may hold thousands of passes, so the daemon chooses them and says why, rather than handing back the newest few and leaving the reader to assume that is the whole story.
  """

  @type t :: :range_start | :range_end | :time_sample

  @values [
    :range_start,
    :range_end,
    :time_sample,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:range_start), do: "range_start"
  def to_wire(:range_end), do: "range_end"
  def to_wire(:time_sample), do: "time_sample"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("range_start"), do: {:ok, :range_start}
  def from_wire("range_end"), do: {:ok, :range_end}
  def from_wire("time_sample"), do: {:ok, :time_sample}
  def from_wire(_), do: :error
end

defmodule Mcpd.KeyScope do
  @moduledoc """
  What an account API key may do. The same six scopes the daemon's configured keys carry, under the same names as the `auth` value of each endpoint.
  """

  @type t :: :ingest | :read | :write | :admin | :execute | :worker

  @values [
    :ingest,
    :read,
    :write,
    :admin,
    :execute,
    :worker,
  ]

  @doc "Every value, in declaration order."
  @spec values() :: [t()]
  def values, do: @values

  @doc "The wire representation of a value."
  @spec to_wire(t()) :: String.t()
  def to_wire(:ingest), do: "ingest"
  def to_wire(:read), do: "read"
  def to_wire(:write), do: "write"
  def to_wire(:admin), do: "admin"
  def to_wire(:execute), do: "execute"
  def to_wire(:worker), do: "worker"

  @doc """
  Parse a wire value.

  Returns `:error` on an unknown value rather than raising: a newer daemon
  may send a value this build has never heard of, and an SDK that crashes on
  one it does not recognise breaks the agent it is meant to be observing.
  """
  @spec from_wire(String.t()) :: {:ok, t()} | :error
  def from_wire("ingest"), do: {:ok, :ingest}
  def from_wire("read"), do: {:ok, :read}
  def from_wire("write"), do: {:ok, :write}
  def from_wire("admin"), do: {:ok, :admin}
  def from_wire("execute"), do: {:ok, :execute}
  def from_wire("worker"), do: {:ok, :worker}
  def from_wire(_), do: :error
end

defmodule Mcpd.InputTokenDetails do
  @moduledoc """
  Input tokens broken down by how the provider billed them. Every field is optional; absent means the provider did not report it, which is different from zero.
  """

  @type t :: %__MODULE__{
          text: non_neg_integer() | nil,
          cache_read: non_neg_integer() | nil,
          cache_write: non_neg_integer() | nil,
          cache_write_5m: non_neg_integer() | nil,
          cache_write_1h: non_neg_integer() | nil,
          audio: non_neg_integer() | nil,
          image: non_neg_integer() | nil
        }

  defstruct [
    text: nil,
    cache_read: nil,
    cache_write: nil,
    cache_write_5m: nil,
    cache_write_1h: nil,
    audio: nil,
    image: nil,
  ]
end

defmodule Mcpd.OutputTokenDetails do
  @moduledoc """
  Output tokens broken down by kind.
  """

  @type t :: %__MODULE__{
          text: non_neg_integer() | nil,
          reasoning: non_neg_integer() | nil,
          audio: non_neg_integer() | nil,
          image: non_neg_integer() | nil
        }

  defstruct [
    text: nil,
    reasoning: nil,
    audio: nil,
    image: nil,
  ]
end

defmodule Mcpd.TokenUsage do
  @moduledoc """
  Token counts for one model call. Totals are authoritative; the detail breakdowns explain them and are what cost is actually computed from.
  """

  @type t :: %__MODULE__{
          input_tokens: non_neg_integer() | nil,
          output_tokens: non_neg_integer() | nil,
          total_tokens: non_neg_integer() | nil,
          input_details: Mcpd.InputTokenDetails.t() | nil,
          output_details: Mcpd.OutputTokenDetails.t() | nil
        }

  defstruct [
    input_tokens: nil,
    output_tokens: nil,
    total_tokens: nil,
    input_details: nil,
    output_details: nil,
  ]
end

defmodule Mcpd.Cost do
  @moduledoc """
  What a span cost, in a single currency. mcpd computes this from a price book at ingest and stores the result, so a later price change never silently rewrites history.
  """

  @type t :: %__MODULE__{
          currency: String.t(),
          is_estimated: boolean(),
          input_cost: float() | nil,
          output_cost: float() | nil,
          total_cost: float() | nil,
          price_book: String.t() | nil
        }

  defstruct [
    currency: "USD",
    is_estimated: true,
    input_cost: nil,
    output_cost: nil,
    total_cost: nil,
    price_book: nil,
  ]
end

defmodule Mcpd.ModelParameters do
  @moduledoc """
  The sampling settings a caller chose for one model call. Recorded because a run nobody can reproduce is a run nobody can debug, and the seed and the sampling settings are what a replay has to match. Every field is what the caller asked for, never what the provider did with it. None of them carries prompt or completion text, so a producer running with payload capture off still records all of them.
  """

  @type t :: %__MODULE__{
          has_response_format: boolean(),
          temperature: float() | nil,
          top_p: float() | nil,
          top_k: non_neg_integer() | nil,
          max_tokens: non_neg_integer() | nil,
          seed: integer() | nil,
          stop_sequences: [String.t()] | nil
        }

  defstruct [
    has_response_format: false,
    temperature: nil,
    top_p: nil,
    top_k: nil,
    max_tokens: nil,
    seed: nil,
    stop_sequences: [],
  ]
end

defmodule Mcpd.SpanError do
  @moduledoc """
  Why a span failed.
  """

  @type t :: %__MODULE__{
          message: String.t(),
          kind: String.t() | nil,
          stack: String.t() | nil,
          domain: String.t() | nil,
          retryable: boolean() | nil,
          details: term() | nil
        }

  @enforce_keys [:message]
  defstruct [
    message: nil,
    kind: nil,
    stack: nil,
    domain: nil,
    retryable: nil,
    details: nil,
  ]
end

defmodule Mcpd.Correlation do
  @moduledoc """
  The descriptive context a signal belongs to. Every field here is queryable and most are safe as metric labels; the high-cardinality ones (`user_id`, `session_id`, `request_id`) are queryable but blocked from metric labels by the cardinality filter.
  """

  @type t :: %__MODULE__{
          project: String.t(),
          environment: String.t() | nil,
          service_name: String.t() | nil,
          service_version: String.t() | nil,
          session_id: String.t() | nil,
          thread_id: String.t() | nil,
          user_id: String.t() | nil,
          resource_id: String.t() | nil,
          run_id: String.t() | nil,
          request_id: String.t() | nil,
          experiment_id: String.t() | nil,
          release: String.t() | nil
        }

  @enforce_keys [:project]
  defstruct [
    project: nil,
    environment: nil,
    service_name: nil,
    service_version: nil,
    session_id: nil,
    thread_id: nil,
    user_id: nil,
    resource_id: nil,
    run_id: nil,
    request_id: nil,
    experiment_id: nil,
    release: nil,
  ]
end

defmodule Mcpd.SpanLink do
  @moduledoc """
  A reference from this span to another, in this trace or a different one. Links express causality that parentage cannot: a batch job caused by twenty requests has twenty links and one parent.
  """

  @type t :: %__MODULE__{
          trace_id: String.t(),
          span_id: String.t() | nil,
          kind: String.t() | nil,
          attributes: term() | nil
        }

  @enforce_keys [:trace_id]
  defstruct [
    trace_id: nil,
    span_id: nil,
    kind: nil,
    attributes: nil,
  ]
end

defmodule Mcpd.Page do
  @moduledoc """
  Cursor pagination. Cursors are opaque and stable; never construct one by hand.
  """

  @type t :: %__MODULE__{
          has_more: boolean(),
          next_cursor: String.t() | nil,
          total: non_neg_integer() | nil
        }

  @enforce_keys [:has_more]
  defstruct [
    has_more: nil,
    next_cursor: nil,
    total: nil,
  ]
end

defmodule Mcpd.IngestBatch do
  @moduledoc """
  One batch of signals sent to the native ingest endpoint. Any combination of the six signals may appear in a single batch; mcpd sorts them out. Supplying `ingest_key` makes the whole batch idempotent, so a client that retries after a timeout never double-counts.
  """

  @type t :: %__MODULE__{
          spans: [Mcpd.Span.t()] | nil,
          logs: [Mcpd.LogRecord.t()] | nil,
          metrics: [Mcpd.MetricPoint.t()] | nil,
          scores: [Mcpd.Score.t()] | nil,
          feedback: [Mcpd.Feedback.t()] | nil,
          aspect_notes: [Mcpd.AspectNote.t()] | nil,
          ingest_key: String.t() | nil
        }

  defstruct [
    spans: [],
    logs: [],
    metrics: [],
    scores: [],
    feedback: [],
    aspect_notes: [],
    ingest_key: nil,
  ]
end

defmodule Mcpd.IngestError do
  @moduledoc """
  Why one signal in a batch was refused.
  """

  @type t :: %__MODULE__{
          signal: Mcpd.SignalType.t(),
          index: non_neg_integer(),
          message: String.t()
        }

  @enforce_keys [:signal, :index, :message]
  defstruct [
    signal: nil,
    index: nil,
    message: nil,
  ]
end

defmodule Mcpd.IngestResult do
  @moduledoc """
  What mcpd did with a batch. Counts are per signal, and `rejected` is never silently zero: a signal mcpd could not store is always reported with a reason.
  """

  @type t :: %__MODULE__{
          accepted: non_neg_integer(),
          rejected: non_neg_integer(),
          duplicate: non_neg_integer(),
          errors: [Mcpd.IngestError.t()] | nil
        }

  @enforce_keys [:accepted, :rejected, :duplicate]
  defstruct [
    accepted: nil,
    rejected: nil,
    duplicate: nil,
    errors: [],
  ]
end

defmodule Mcpd.TraceListResponse do
  @moduledoc """
  A page of traces.
  """

  @type t :: %__MODULE__{
          traces: [Mcpd.Trace.t()],
          page: Mcpd.Page.t()
        }

  @enforce_keys [:traces, :page]
  defstruct [
    traces: [],
    page: nil,
  ]
end

defmodule Mcpd.TraceDetail do
  @moduledoc """
  One trace and everything attached to it.
  """

  @type t :: %__MODULE__{
          trace: Mcpd.Trace.t(),
          spans: [Mcpd.Span.t()],
          logs: [Mcpd.LogRecord.t()] | nil,
          scores: [Mcpd.Score.t()] | nil,
          feedback: [Mcpd.Feedback.t()] | nil
        }

  @enforce_keys [:trace, :spans]
  defstruct [
    trace: nil,
    spans: [],
    logs: [],
    scores: [],
    feedback: [],
  ]
end

defmodule Mcpd.SpanListResponse do
  @moduledoc """
  A page of spans.
  """

  @type t :: %__MODULE__{
          spans: [Mcpd.Span.t()],
          page: Mcpd.Page.t()
        }

  @enforce_keys [:spans, :page]
  defstruct [
    spans: [],
    page: nil,
  ]
end

defmodule Mcpd.LogListResponse do
  @moduledoc """
  A page of log records.
  """

  @type t :: %__MODULE__{
          logs: [Mcpd.LogRecord.t()],
          page: Mcpd.Page.t()
        }

  @enforce_keys [:logs, :page]
  defstruct [
    logs: [],
    page: nil,
  ]
end

defmodule Mcpd.MetricSeriesPoint do
  @moduledoc """
  One bucket of an aggregated series.
  """

  @type t :: %__MODULE__{
          timestamp: DateTime.t(),
          value: float(),
          count: non_neg_integer() | nil,
          exemplar_trace_id: String.t() | nil
        }

  @enforce_keys [:timestamp, :value]
  defstruct [
    timestamp: nil,
    value: nil,
    count: nil,
    exemplar_trace_id: nil,
  ]
end

defmodule Mcpd.MetricHistogram do
  @moduledoc """
  A histogram metric read back as distributions: the buckets of every point in the window added up, one distribution per label combination, with quantiles read from the merged counts.

  Bucketed points from OpenTelemetry are added bucket by bucket; for a cumulative stream only the growth across the window counts. Points with one value each, such as mcpd's own derived durations, give exact quantiles and are placed into OpenTelemetry's default boundaries.
  """

  @type t :: %__MODULE__{
          name: String.t(),
          points_read: non_neg_integer(),
          truncated: boolean(),
          series: [Mcpd.HistogramSeries.t()],
          unit: String.t() | nil
        }

  @enforce_keys [:name, :points_read, :truncated, :series]
  defstruct [
    name: nil,
    points_read: nil,
    truncated: nil,
    series: [],
    unit: nil,
  ]
end

defmodule Mcpd.HistogramSeries do
  @moduledoc """
  One merged distribution.
  """

  @type t :: %__MODULE__{
          bounds: [float()],
          counts: [non_neg_integer()],
          count: non_neg_integer(),
          exact: boolean(),
          points_skipped: non_neg_integer(),
          labels: term() | nil,
          sum: float() | nil,
          min: float() | nil,
          max: float() | nil,
          p50: float() | nil,
          p90: float() | nil,
          p95: float() | nil,
          p99: float() | nil
        }

  @enforce_keys [:bounds, :counts, :count, :exact, :points_skipped]
  defstruct [
    bounds: [],
    counts: [],
    count: nil,
    exact: nil,
    points_skipped: nil,
    labels: nil,
    sum: nil,
    min: nil,
    max: nil,
    p50: nil,
    p90: nil,
    p95: nil,
    p99: nil,
  ]
end

defmodule Mcpd.MetricPointListResponse do
  @moduledoc """
  A page of stored metric points, as they were recorded.
  """

  @type t :: %__MODULE__{
          points: [Mcpd.MetricPoint.t()],
          page: Mcpd.Page.t()
        }

  @enforce_keys [:points, :page]
  defstruct [
    points: [],
    page: nil,
  ]
end

defmodule Mcpd.MetricSeries do
  @moduledoc """
  One line on a chart: a single label combination over time.
  """

  @type t :: %__MODULE__{
          points: [Mcpd.MetricSeriesPoint.t()],
          labels: term() | nil
        }

  @enforce_keys [:points]
  defstruct [
    points: [],
    labels: nil,
  ]
end

defmodule Mcpd.MetricSeriesResponse do
  @moduledoc """
  An aggregated metric query result.
  """

  @type t :: %__MODULE__{
          name: String.t(),
          bucket: String.t(),
          series: [Mcpd.MetricSeries.t()],
          unit: String.t() | nil
        }

  @enforce_keys [:name, :bucket, :series]
  defstruct [
    name: nil,
    bucket: nil,
    series: [],
    unit: nil,
  ]
end

defmodule Mcpd.ScoreListResponse do
  @moduledoc """
  A page of scores.
  """

  @type t :: %__MODULE__{
          scores: [Mcpd.Score.t()],
          page: Mcpd.Page.t()
        }

  @enforce_keys [:scores, :page]
  defstruct [
    scores: [],
    page: nil,
  ]
end

defmodule Mcpd.FeedbackListResponse do
  @moduledoc """
  A page of feedback.
  """

  @type t :: %__MODULE__{
          feedback: [Mcpd.Feedback.t()],
          page: Mcpd.Page.t()
        }

  @enforce_keys [:feedback, :page]
  defstruct [
    feedback: [],
    page: nil,
  ]
end

defmodule Mcpd.LatencyStats do
  @moduledoc """
  Latency distribution over a window, in milliseconds.
  """

  @type t :: %__MODULE__{
          p50: float() | nil,
          p90: float() | nil,
          p95: float() | nil,
          p99: float() | nil,
          mean: float() | nil,
          max: float() | nil
        }

  defstruct [
    p50: nil,
    p90: nil,
    p95: nil,
    p99: nil,
    mean: nil,
    max: nil,
  ]
end

defmodule Mcpd.GroupedStat do
  @moduledoc """
  One row of a grouped breakdown.
  """

  @type t :: %__MODULE__{
          key: String.t(),
          count: non_neg_integer(),
          error_count: non_neg_integer(),
          total_cost: float() | nil,
          input_tokens: non_neg_integer() | nil,
          output_tokens: non_neg_integer() | nil,
          latency: Mcpd.LatencyStats.t() | nil
        }

  @enforce_keys [:key, :count, :error_count]
  defstruct [
    key: nil,
    count: nil,
    error_count: nil,
    total_cost: nil,
    input_tokens: nil,
    output_tokens: nil,
    latency: nil,
  ]
end

defmodule Mcpd.AnalyticsSummary do
  @moduledoc """
  Headline numbers for a window. One request backs the overview screen.
  """

  @type t :: %__MODULE__{
          from: DateTime.t(),
          to: DateTime.t(),
          trace_count: non_neg_integer(),
          span_count: non_neg_integer(),
          error_count: non_neg_integer(),
          error_rate: float(),
          latency: Mcpd.LatencyStats.t(),
          input_tokens: non_neg_integer(),
          output_tokens: non_neg_integer(),
          total_cost: float(),
          currency: String.t(),
          groups: [Mcpd.GroupedStat.t()] | nil
        }

  @enforce_keys [:from, :to, :trace_count, :span_count, :error_count, :error_rate, :latency, :input_tokens, :output_tokens, :total_cost]
  defstruct [
    from: nil,
    to: nil,
    trace_count: nil,
    span_count: nil,
    error_count: nil,
    error_rate: nil,
    latency: nil,
    input_tokens: nil,
    output_tokens: nil,
    total_cost: nil,
    currency: "USD",
    groups: [],
  ]
end

defmodule Mcpd.CostBucket do
  @moduledoc """
  Spend in one time bucket for one group.
  """

  @type t :: %__MODULE__{
          timestamp: DateTime.t(),
          key: String.t(),
          total_cost: float(),
          input_tokens: non_neg_integer() | nil,
          output_tokens: non_neg_integer() | nil
        }

  @enforce_keys [:timestamp, :key, :total_cost]
  defstruct [
    timestamp: nil,
    key: nil,
    total_cost: nil,
    input_tokens: nil,
    output_tokens: nil,
  ]
end

defmodule Mcpd.CostBreakdown do
  @moduledoc """
  Where the money went.
  """

  @type t :: %__MODULE__{
          from: DateTime.t(),
          to: DateTime.t(),
          total_cost: float(),
          groups: [Mcpd.GroupedStat.t()],
          currency: String.t(),
          series: [Mcpd.CostBucket.t()] | nil
        }

  @enforce_keys [:from, :to, :total_cost, :groups]
  defstruct [
    from: nil,
    to: nil,
    total_cost: nil,
    groups: [],
    currency: "USD",
    series: [],
  ]
end

defmodule Mcpd.DeleteResult do
  @moduledoc """
  What a deletion removed.
  """

  @type t :: %__MODULE__{
          deleted: non_neg_integer()
        }

  @enforce_keys [:deleted]
  defstruct [
    deleted: nil,
  ]
end

defmodule Mcpd.HealthStatus do
  @moduledoc """
  Whether the server is alive and able to serve.
  """

  @type t :: %__MODULE__{
          status: String.t(),
          version: String.t(),
          spec_version: String.t(),
          uptime_seconds: non_neg_integer(),
          storage: String.t() | nil,
          detail: String.t() | nil,
          signup: boolean() | nil
        }

  @enforce_keys [:status, :version, :spec_version, :uptime_seconds]
  defstruct [
    status: nil,
    version: nil,
    spec_version: nil,
    uptime_seconds: nil,
    storage: nil,
    detail: nil,
    signup: nil,
  ]
end

defmodule Mcpd.ScorerInfo do
  @moduledoc """
  One scorer mcpd can run, as described to a caller.
  """

  @type t :: %__MODULE__{
          id: String.t(),
          description: String.t(),
          needs_config: boolean(),
          kind: Mcpd.ScorerKind.t(),
          direction: Mcpd.ScoreDirection.t(),
          config_keys: [String.t()],
          available: boolean()
        }

  @enforce_keys [:id, :description, :needs_config, :kind, :direction, :config_keys]
  defstruct [
    id: nil,
    description: nil,
    needs_config: nil,
    kind: nil,
    direction: nil,
    config_keys: [],
    available: true,
  ]
end

defmodule Mcpd.ScorerConfig do
  @moduledoc """
  Settings shared by the scorers that need them. Every field is optional; a scorer that needs one and does not get it declines instead of guessing.
  """

  @type t :: %__MODULE__{
          forbidden: [String.t()],
          keywords: [String.t()],
          expected_tools: [String.t()],
          forbidden_tools: [String.t()],
          context: [String.t()],
          case_sensitive: boolean(),
          needle: String.t() | nil,
          expected: String.t() | nil,
          max_ms: float() | nil,
          max_cost: float() | nil,
          max_tool_calls: non_neg_integer() | nil,
          span_kind: Mcpd.SpanKind.t() | nil,
          span_name: String.t() | nil,
          tool_name: String.t() | nil,
          pattern: String.t() | nil,
          min_chars: non_neg_integer() | nil,
          max_chars: non_neg_integer() | nil,
          json_schema: term() | nil
        }

  @enforce_keys [:forbidden, :keywords, :expected_tools, :forbidden_tools, :context]
  defstruct [
    forbidden: [],
    keywords: [],
    expected_tools: [],
    forbidden_tools: [],
    context: [],
    case_sensitive: false,
    needle: nil,
    expected: nil,
    max_ms: nil,
    max_cost: nil,
    max_tool_calls: nil,
    span_kind: nil,
    span_name: nil,
    tool_name: nil,
    pattern: nil,
    min_chars: nil,
    max_chars: nil,
    json_schema: nil,
  ]
end

defmodule Mcpd.ScoreRunRequest do
  @moduledoc """
  A request to run scorers over runs that are already stored.

  Scoring reads stored runs rather than running inline, so an evaluation can never add latency to the request it is judging, and a scorer that fails can never fail a user's request.
  """

  @type t :: %__MODULE__{
          trace_ids: [String.t()],
          scorers: [String.t()],
          dry_run: boolean(),
          project: String.t() | nil,
          from: DateTime.t() | nil,
          limit: non_neg_integer() | nil,
          config: Mcpd.ScorerConfig.t() | nil,
          source: Mcpd.ScoreSource.t() | nil
        }

  @enforce_keys [:trace_ids, :scorers]
  defstruct [
    trace_ids: [],
    scorers: [],
    dry_run: false,
    project: nil,
    from: nil,
    limit: nil,
    config: nil,
    source: nil,
  ]
end

defmodule Mcpd.SkippedScorer do
  @moduledoc """
  A scorer that declined to answer on one run.
  """

  @type t :: %__MODULE__{
          scorer_id: String.t(),
          trace_id: String.t(),
          reason: String.t()
        }

  @enforce_keys [:scorer_id, :trace_id, :reason]
  defstruct [
    scorer_id: nil,
    trace_id: nil,
    reason: nil,
  ]
end

defmodule Mcpd.ScoreRunResult do
  @moduledoc """
  What one scoring pass did.
  """

  @type t :: %__MODULE__{
          traces_scored: non_neg_integer(),
          scores_written: non_neg_integer(),
          skipped: [Mcpd.SkippedScorer.t()],
          scores: [Mcpd.Score.t()]
        }

  @enforce_keys [:traces_scored, :scores_written, :skipped, :scores]
  defstruct [
    traces_scored: nil,
    scores_written: nil,
    skipped: [],
    scores: [],
  ]
end

defmodule Mcpd.ScorerDelta do
  @moduledoc """
  How one scorer moved between two experiments.
  """

  @type t :: %__MODULE__{
          scorer_id: String.t(),
          baseline_n: non_neg_integer(),
          candidate_n: non_neg_integer(),
          decidable: boolean(),
          direction: Mcpd.ScoreDirection.t(),
          regressed: boolean(),
          baseline_mean: float() | nil,
          candidate_mean: float() | nil,
          delta: float() | nil,
          ci_low: float() | nil,
          ci_high: float() | nil,
          p_value: float() | nil
        }

  @enforce_keys [:scorer_id, :baseline_n, :candidate_n, :decidable, :direction]
  defstruct [
    scorer_id: nil,
    baseline_n: nil,
    candidate_n: nil,
    decidable: nil,
    direction: nil,
    regressed: false,
    baseline_mean: nil,
    candidate_mean: nil,
    delta: nil,
    ci_low: nil,
    ci_high: nil,
    p_value: nil,
  ]
end

defmodule Mcpd.ExperimentComparison do
  @moduledoc """
  Two experiments, compared.

  The point of an experiment is the comparison, so this reports the delta per scorer beside the sample sizes. A mean that moved on four runs is noise, and printing the counts next to it is what stops it being read as a result.
  """

  @type t :: %__MODULE__{
          baseline: String.t(),
          candidate: String.t(),
          scorers: [Mcpd.ScorerDelta.t()],
          regressed: boolean()
        }

  @enforce_keys [:baseline, :candidate, :scorers, :regressed]
  defstruct [
    baseline: nil,
    candidate: nil,
    scorers: [],
    regressed: nil,
  ]
end

defmodule Mcpd.CaptureRequest do
  @moduledoc """
  A request to turn stored runs into dataset cases.
  """

  @type t :: %__MODULE__{
          trace_ids: [String.t()],
          use_output_as_expected: boolean()
        }

  @enforce_keys [:trace_ids]
  defstruct [
    trace_ids: [],
    use_output_as_expected: false,
  ]
end

defmodule Mcpd.CaptureResult do
  @moduledoc """
  What a capture added to a dataset.
  """

  @type t :: %__MODULE__{
          captured: non_neg_integer(),
          requested: non_neg_integer()
        }

  @enforce_keys [:captured, :requested]
  defstruct [
    captured: nil,
    requested: nil,
  ]
end

defmodule Mcpd.ItemsAdded do
  @moduledoc """
  How many cases a dataset gained.
  """

  @type t :: %__MODULE__{
          added: non_neg_integer()
        }

  @enforce_keys [:added]
  defstruct [
    added: nil,
  ]
end

defmodule Mcpd.ErasureResult do
  @moduledoc """
  What an erasure request removed, counted per signal. The subject is not echoed back: it has been erased, and repeating it in a response that may be logged on the way out would write it down again.
  """

  @type t :: %__MODULE__{
          project: String.t(),
          traces: non_neg_integer(),
          spans: non_neg_integer(),
          logs: non_neg_integer(),
          metrics: non_neg_integer(),
          scores: non_neg_integer(),
          feedback: non_neg_integer(),
          deleted: non_neg_integer(),
          datasets: [String.t()],
          runs_erased: non_neg_integer() | nil,
          runs_in_flight: non_neg_integer() | nil,
          aspect_notes_erased: non_neg_integer() | nil,
          dataset_items_erased: non_neg_integer() | nil,
          deletes_pending: boolean() | nil,
          unmatchable_identifiers: non_neg_integer() | nil,
          note: String.t() | nil
        }

  @enforce_keys [:project, :traces, :spans, :logs, :metrics, :scores, :feedback, :deleted, :datasets]
  defstruct [
    project: nil,
    traces: nil,
    spans: nil,
    logs: nil,
    metrics: nil,
    scores: nil,
    feedback: nil,
    deleted: nil,
    datasets: [],
    runs_erased: nil,
    runs_in_flight: nil,
    aspect_notes_erased: nil,
    dataset_items_erased: nil,
    deletes_pending: nil,
    unmatchable_identifiers: nil,
    note: nil,
  ]
end

defmodule Mcpd.DefinitionDocument do
  @moduledoc """
  A definition as you post it: the graph, and the id and version to store it under. The body is validated at write time against `spec/definition.schema.json`; a body the walker cannot run is refused with a 400 naming the field, and unknown fields are refused, so a guard that is spelled wrong is an error and not a run that skips its guard.
  """

  @type t :: %__MODULE__{
          id: String.t(),
          body: term(),
          kind: Mcpd.DefinitionKind.t() | nil,
          version: non_neg_integer() | nil
        }

  @enforce_keys [:id, :body]
  defstruct [
    id: nil,
    body: nil,
    kind: nil,
    version: nil,
  ]
end

defmodule Mcpd.DefinitionVersion do
  @moduledoc """
  Where a posted definition landed.
  """

  @type t :: %__MODULE__{
          id: String.t(),
          version: non_neg_integer(),
          assigned: boolean()
        }

  @enforce_keys [:id, :version, :assigned]
  defstruct [
    id: nil,
    version: nil,
    assigned: nil,
  ]
end

defmodule Mcpd.RunCursor do
  @moduledoc """
  The frontier of a run: which position in the definition's `steps` it has reached, and what sits there.
  """

  @type t :: %__MODULE__{
          index: non_neg_integer(),
          construct: Mcpd.ConstructKind.t(),
          step_key: String.t() | nil
        }

  @enforce_keys [:index, :construct]
  defstruct [
    index: nil,
    construct: nil,
    step_key: nil,
  ]
end

defmodule Mcpd.RunTimer do
  @moduledoc """
  A deadline the daemon holds for a run.
  """

  @type t :: %__MODULE__{
          run_id: String.t(),
          kind: Mcpd.TimerKind.t(),
          due_at: DateTime.t(),
          step_key: String.t() | nil
        }

  @enforce_keys [:run_id, :kind, :due_at]
  defstruct [
    run_id: nil,
    kind: nil,
    due_at: nil,
    step_key: nil,
  ]
end

defmodule Mcpd.RunWait do
  @moduledoc """
  A suspension waiting for an event.
  """

  @type t :: %__MODULE__{
          run_id: String.t(),
          step_key: String.t(),
          event: String.t(),
          key: String.t(),
          expires_at: DateTime.t()
        }

  @enforce_keys [:run_id, :step_key, :event, :key, :expires_at]
  defstruct [
    run_id: nil,
    step_key: nil,
    event: nil,
    key: nil,
    expires_at: nil,
  ]
end

defmodule Mcpd.RunDetail do
  @moduledoc """
  One run and everything the daemon holds for it.
  """

  @type t :: %__MODULE__{
          run: Mcpd.Run.t(),
          attempts: [Mcpd.StepAttempt.t()],
          timers: [Mcpd.RunTimer.t()],
          waits: [Mcpd.RunWait.t()]
        }

  @enforce_keys [:run, :attempts, :timers, :waits]
  defstruct [
    run: nil,
    attempts: [],
    timers: [],
    waits: [],
  ]
end

defmodule Mcpd.RunListResponse do
  @moduledoc """
  A page of runs.
  """

  @type t :: %__MODULE__{
          runs: [Mcpd.Run.t()],
          page: Mcpd.Page.t()
        }

  @enforce_keys [:runs, :page]
  defstruct [
    runs: [],
    page: nil,
  ]
end

defmodule Mcpd.RunEventPage do
  @moduledoc """
  A page of a run's event log.
  """

  @type t :: %__MODULE__{
          events: [Mcpd.RunEvent.t()],
          has_more: boolean()
        }

  @enforce_keys [:events, :has_more]
  defstruct [
    events: [],
    has_more: nil,
  ]
end

defmodule Mcpd.CauseLink do
  @moduledoc """
  One edge of a cause chain: an ancestor step, and the template path through which the step below it read that ancestor's output. An ancestor read through two paths appears twice, once per path.
  """

  @type t :: %__MODULE__{
          step_key: String.t(),
          status: Mcpd.AttemptStatus.t(),
          attempts: non_neg_integer(),
          template_path: String.t(),
          consumed_by: String.t(),
          depth: non_neg_integer(),
          silent_producer: boolean(),
          value_kind: Mcpd.ValueKind.t() | nil,
          value_empty: boolean() | nil
        }

  @enforce_keys [:step_key, :status, :attempts, :template_path, :consumed_by, :depth, :silent_producer]
  defstruct [
    step_key: nil,
    status: nil,
    attempts: nil,
    template_path: nil,
    consumed_by: nil,
    depth: nil,
    silent_producer: nil,
    value_kind: nil,
    value_empty: nil,
  ]
end

defmodule Mcpd.StepCause do
  @moduledoc """
  Why one step of a run failed, answered by walking backwards along the data dependencies its definition declares. The daemon reads the run journal and the pinned definition version, both of which it has to hold to render `${...}` templates at all, so this answer is available with span payload capture off and with no model, prompt or completion involved.
  """

  @type t :: %__MODULE__{
          run_id: String.t(),
          step_key: String.t(),
          status: Mcpd.AttemptStatus.t(),
          attempts: non_neg_integer(),
          chain: [Mcpd.CauseLink.t()],
          truncated: boolean(),
          error: Mcpd.StepError.t() | nil
        }

  @enforce_keys [:run_id, :step_key, :status, :attempts, :chain, :truncated]
  defstruct [
    run_id: nil,
    step_key: nil,
    status: nil,
    attempts: nil,
    chain: [],
    truncated: nil,
    error: nil,
  ]
end

defmodule Mcpd.StartRunRequest do
  @moduledoc """
  A request to start a run.
  """

  @type t :: %__MODULE__{
          definition_id: String.t(),
          project: String.t() | nil,
          version: non_neg_integer() | nil,
          input: term() | nil,
          correlation: Mcpd.Correlation.t() | nil,
          idempotency_key: String.t() | nil
        }

  @enforce_keys [:definition_id]
  defstruct [
    definition_id: nil,
    project: nil,
    version: nil,
    input: nil,
    correlation: nil,
    idempotency_key: nil,
  ]
end

defmodule Mcpd.ClaimRequest do
  @moduledoc """
  A worker asking for work.
  """

  @type t :: %__MODULE__{
          worker_id: String.t(),
          max: non_neg_integer(),
          wait_ms: non_neg_integer(),
          project: String.t() | nil,
          run_id: String.t() | nil
        }

  @enforce_keys [:worker_id, :max, :wait_ms]
  defstruct [
    worker_id: nil,
    max: nil,
    wait_ms: nil,
    project: nil,
    run_id: nil,
  ]
end

defmodule Mcpd.StepLease do
  @moduledoc """
  A step handed to a worker. Every later call about the attempt is addressed by `lease_id`.
  """

  @type t :: %__MODULE__{
          lease_id: String.t(),
          run_id: String.t(),
          project: String.t(),
          definition_id: String.t(),
          definition_version: non_neg_integer(),
          step_key: String.t(),
          kind: Mcpd.StepKind.t(),
          attempt: non_neg_integer(),
          effect: Mcpd.Effect.t(),
          expires_at: DateTime.t(),
          idempotency_key: String.t(),
          prior: term(),
          correlation: Mcpd.Correlation.t(),
          trace_id: String.t(),
          span_id: String.t(),
          parent_span_id: String.t(),
          deadline_ms: non_neg_integer() | nil,
          input: term() | nil,
          state: term() | nil
        }

  @enforce_keys [:lease_id, :run_id, :project, :definition_id, :definition_version, :step_key, :kind, :attempt, :effect, :expires_at, :idempotency_key, :prior, :correlation, :trace_id, :span_id, :parent_span_id]
  defstruct [
    lease_id: nil,
    run_id: nil,
    project: nil,
    definition_id: nil,
    definition_version: nil,
    step_key: nil,
    kind: nil,
    attempt: nil,
    effect: nil,
    expires_at: nil,
    idempotency_key: nil,
    prior: nil,
    correlation: nil,
    trace_id: nil,
    span_id: nil,
    parent_span_id: nil,
    deadline_ms: nil,
    input: nil,
    state: nil,
  ]
end

defmodule Mcpd.ClaimResponse do
  @moduledoc """
  What a claim answered.
  """

  @type t :: %__MODULE__{
          leases: [Mcpd.StepLease.t()]
        }

  @enforce_keys [:leases]
  defstruct [
    leases: [],
  ]
end

defmodule Mcpd.LeaseState do
  @moduledoc """
  What `started` and `heartbeat` answer.
  """

  @type t :: %__MODULE__{
          expires_at: DateTime.t(),
          reclaimed: boolean()
        }

  @enforce_keys [:expires_at, :reclaimed]
  defstruct [
    expires_at: nil,
    reclaimed: nil,
  ]
end

defmodule Mcpd.StepUsage do
  @moduledoc """
  Token usage a worker reports on `complete`. The daemon prices it from its price book and charges the attempt, the run and the root of the run tree.
  """

  @type t :: %__MODULE__{
          input_tokens: non_neg_integer() | nil,
          output_tokens: non_neg_integer() | nil,
          total_tokens: non_neg_integer() | nil,
          input_details: Mcpd.InputTokenDetails.t() | nil,
          output_details: Mcpd.OutputTokenDetails.t() | nil,
          model: String.t() | nil,
          provider: String.t() | nil
        }

  defstruct [
    input_tokens: nil,
    output_tokens: nil,
    total_tokens: nil,
    input_details: nil,
    output_details: nil,
    model: nil,
    provider: nil,
  ]
end

defmodule Mcpd.StepResult do
  @moduledoc """
  What a worker posts to complete a step.
  """

  @type t :: %__MODULE__{
          output: term() | nil,
          state: term() | nil,
          usage: Mcpd.StepUsage.t() | nil
        }

  defstruct [
    output: nil,
    state: nil,
    usage: nil,
  ]
end

defmodule Mcpd.StepError do
  @moduledoc """
  Why a step failed, as the worker reports it.
  """

  @type t :: %__MODULE__{
          class: Mcpd.StepErrorClass.t(),
          message: String.t(),
          details: term() | nil,
          retry_after_ms: non_neg_integer() | nil
        }

  @enforce_keys [:class, :message]
  defstruct [
    class: nil,
    message: nil,
    details: nil,
    retry_after_ms: nil,
  ]
end

defmodule Mcpd.StepFailure do
  @moduledoc """
  What a worker posts to fail a step.
  """

  @type t :: %__MODULE__{
          error: Mcpd.StepError.t()
        }

  @enforce_keys [:error]
  defstruct [
    error: nil,
  ]
end

defmodule Mcpd.StepAck do
  @moduledoc """
  What `complete` and `fail` answer.
  """

  @type t :: %__MODULE__{
          run_status: Mcpd.RunStatus.t(),
          next: String.t(),
          attempts_left: non_neg_integer() | nil
        }

  @enforce_keys [:run_status, :next]
  defstruct [
    run_status: nil,
    next: nil,
    attempts_left: nil,
  ]
end

defmodule Mcpd.StepConflict do
  @moduledoc """
  The body of a 409 from a worker call. `reason` says what is true and what the worker should do next.
  """

  @type t :: %__MODULE__{
          error: String.t(),
          reason: Mcpd.ConflictReason.t(),
          message: String.t(),
          attempt: non_neg_integer(),
          run_status: Mcpd.RunStatus.t(),
          attempt_status: Mcpd.AttemptStatus.t() | nil
        }

  @enforce_keys [:error, :reason, :message, :attempt, :run_status]
  defstruct [
    error: nil,
    reason: nil,
    message: nil,
    attempt: nil,
    run_status: nil,
    attempt_status: nil,
  ]
end

defmodule Mcpd.PublishEventRequest do
  @moduledoc """
  An event for whichever run is waiting for it.
  """

  @type t :: %__MODULE__{
          event: String.t(),
          key: String.t(),
          project: String.t() | nil,
          payload: term() | nil,
          event_id: String.t() | nil
        }

  @enforce_keys [:event, :key]
  defstruct [
    event: nil,
    key: nil,
    project: nil,
    payload: nil,
    event_id: nil,
  ]
end

defmodule Mcpd.EventReceipt do
  @moduledoc """
  What publishing an event did.
  """

  @type t :: %__MODULE__{
          outcome: Mcpd.EventOutcome.t(),
          event_id: String.t()
        }

  @enforce_keys [:outcome, :event_id]
  defstruct [
    outcome: nil,
    event_id: nil,
  ]
end

defmodule Mcpd.SignalRequest do
  @moduledoc """
  An event for one run, addressed by run id rather than by key. The daemon reads the run's open wait on `event`, takes its key, and routes the event as a published one.
  """

  @type t :: %__MODULE__{
          event: String.t(),
          payload: term() | nil,
          event_id: String.t() | nil
        }

  @enforce_keys [:event]
  defstruct [
    event: nil,
    payload: nil,
    event_id: nil,
  ]
end

defmodule Mcpd.WorkerRegistration do
  @moduledoc """
  A worker announcing the definitions it serves.
  """

  @type t :: %__MODULE__{
          worker_id: String.t(),
          definitions: [Mcpd.DefinitionDocument.t()],
          project: String.t() | nil
        }

  @enforce_keys [:worker_id, :definitions]
  defstruct [
    worker_id: nil,
    definitions: [],
    project: nil,
  ]
end

defmodule Mcpd.WorkerRegistered do
  @moduledoc """
  What registering answered.
  """

  @type t :: %__MODULE__{
          worker_id: String.t(),
          registered: [Mcpd.DefinitionVersion.t()],
          lease_ttl_ms: non_neg_integer(),
          heartbeat_ms: non_neg_integer()
        }

  @enforce_keys [:worker_id, :registered, :lease_ttl_ms, :heartbeat_ms]
  defstruct [
    worker_id: nil,
    registered: [],
    lease_ttl_ms: nil,
    heartbeat_ms: nil,
  ]
end

defmodule Mcpd.Monitor do
  @moduledoc """
  A saved condition the daemon evaluates on a schedule. Monitors are configured in the daemon's configuration file, not through the API.
  """

  @type t :: %__MODULE__{
          id: String.t(),
          name: String.t(),
          project: String.t(),
          watch: Mcpd.MonitorWatch.t(),
          compare: Mcpd.MonitorCompare.t(),
          threshold: float(),
          window_seconds: non_neg_integer(),
          min_samples: non_neg_integer(),
          enabled: boolean(),
          scorer_id: String.t() | nil,
          webhook: String.t() | nil
        }

  @enforce_keys [:id, :name, :project, :watch, :compare, :threshold]
  defstruct [
    id: nil,
    name: nil,
    project: nil,
    watch: nil,
    compare: nil,
    threshold: nil,
    window_seconds: 300,
    min_samples: 20,
    enabled: true,
    scorer_id: nil,
    webhook: nil,
  ]
end

defmodule Mcpd.MonitorReading do
  @moduledoc """
  One evaluation of a monitor.
  """

  @type t :: %__MODULE__{
          monitor_id: String.t(),
          at: DateTime.t(),
          samples: non_neg_integer(),
          condition: Mcpd.MonitorCondition.t(),
          detail: String.t(),
          value: float() | nil
        }

  @enforce_keys [:monitor_id, :at, :samples, :condition, :detail]
  defstruct [
    monitor_id: nil,
    at: nil,
    samples: nil,
    condition: nil,
    detail: nil,
    value: nil,
  ]
end

defmodule Mcpd.MonitorState do
  @moduledoc """
  A monitor and the condition it is latched in.
  """

  @type t :: %__MODULE__{
          monitor: Mcpd.Monitor.t(),
          condition: Mcpd.MonitorCondition.t(),
          last: Mcpd.MonitorReading.t() | nil,
          tripped_at: DateTime.t() | nil
        }

  @enforce_keys [:monitor, :condition]
  defstruct [
    monitor: nil,
    condition: nil,
    last: nil,
    tripped_at: nil,
  ]
end

defmodule Mcpd.Slo do
  @moduledoc """
  A service level objective: the fraction of runs that must succeed over a window. A monitor asks whether a number is over a line right now. An objective asks whether the promise is going to be missed and how fast the error budget is burning, over two windows at once, so a two minute blip and a slow leak get the different answers they need. Objectives are configured in the daemon's configuration file, not through the API.
  """

  @type t :: %__MODULE__{
          id: String.t(),
          name: String.t(),
          project: String.t(),
          objective: float(),
          window_days: non_neg_integer(),
          min_samples: non_neg_integer(),
          enabled: boolean(),
          webhook: String.t() | nil
        }

  @enforce_keys [:id, :name, :project, :objective]
  defstruct [
    id: nil,
    name: nil,
    project: nil,
    objective: nil,
    window_days: 30,
    min_samples: 20,
    enabled: true,
    webhook: nil,
  ]
end

defmodule Mcpd.SloReading do
  @moduledoc """
  One evaluation of an objective.
  """

  @type t :: %__MODULE__{
          slo_id: String.t(),
          at: DateTime.t(),
          samples: non_neg_integer(),
          condition: Mcpd.MonitorCondition.t(),
          detail: String.t(),
          success_rate: float() | nil,
          budget_remaining: float() | nil,
          burn_fast: float() | nil,
          burn_slow: float() | nil
        }

  @enforce_keys [:slo_id, :at, :samples, :condition, :detail]
  defstruct [
    slo_id: nil,
    at: nil,
    samples: nil,
    condition: nil,
    detail: nil,
    success_rate: nil,
    budget_remaining: nil,
    burn_fast: nil,
    burn_slow: nil,
  ]
end

defmodule Mcpd.SloState do
  @moduledoc """
  An objective and the condition it is latched in.
  """

  @type t :: %__MODULE__{
          slo: Mcpd.Slo.t(),
          condition: Mcpd.MonitorCondition.t(),
          last: Mcpd.SloReading.t() | nil,
          tripped_at: DateTime.t() | nil
        }

  @enforce_keys [:slo, :condition]
  defstruct [
    slo: nil,
    condition: nil,
    last: nil,
    tripped_at: nil,
  ]
end

defmodule Mcpd.StoreCapabilities do
  @moduledoc """
  What the storage backend behind this daemon can and cannot do. Three backends implement one storage trait and they are not equal: a reader on ClickHouse who searches for `CAFE` with an acute and finds nothing has no way to learn that this backend folds ASCII case and no more. Read this once at connect time and offer only what the deployment answers. Every field is named after the capability an endpoint names when it refuses with 501, so the two can be compared without translation.
  """

  @type t :: %__MODULE__{
          backend: String.t(),
          text_search: Mcpd.TextSearch.t(),
          unicode_case_folding: boolean(),
          immediate_deletes: boolean(),
          discovery: boolean(),
          discovery_label_values: boolean(),
          delta_polling: boolean(),
          per_signal_retention: boolean(),
          declarative_retention: boolean(),
          partitioning: Mcpd.Partitioning.t(),
          partition_drop_retention: boolean(),
          idempotent_schema_init: boolean(),
          theme_analysis: boolean()
        }

  @enforce_keys [:backend, :text_search, :unicode_case_folding, :immediate_deletes, :discovery, :discovery_label_values, :delta_polling, :per_signal_retention, :declarative_retention, :partitioning, :partition_drop_retention, :idempotent_schema_init, :theme_analysis]
  defstruct [
    backend: nil,
    text_search: nil,
    unicode_case_folding: nil,
    immediate_deletes: nil,
    discovery: nil,
    discovery_label_values: nil,
    delta_polling: nil,
    per_signal_retention: nil,
    declarative_retention: nil,
    partitioning: nil,
    partition_drop_retention: nil,
    idempotent_schema_init: nil,
    theme_analysis: nil,
  ]
end

defmodule Mcpd.DiscoveredValue do
  @moduledoc """
  One value a filter dimension takes in a project.
  """

  @type t :: %__MODULE__{
          value: String.t(),
          last_seen: DateTime.t()
        }

  @enforce_keys [:value, :last_seen]
  defstruct [
    value: nil,
    last_seen: nil,
  ]
end

defmodule Mcpd.DiscoveryResponse do
  @moduledoc """
  The values one filter dimension takes, for a filter box. Answered from a distinct-value list the daemon maintains as signals arrive, not by scanning the signal tables, because the question is asked on every keystroke and the tables it would otherwise scan are the largest thing the deployment holds.
  """

  @type t :: %__MODULE__{
          facet: Mcpd.Facet.t(),
          values: [Mcpd.DiscoveredValue.t()],
          key: String.t() | nil
        }

  @enforce_keys [:facet, :values]
  defstruct [
    facet: nil,
    values: [],
    key: nil,
  ]
end

defmodule Mcpd.DeltaResponse do
  @moduledoc """
  What has arrived since a cursor. A poll is not a page: paging walks a fixed result set backwards in the sort you chose, and polling walks forwards in arrival order and never ends, so the two cursors are different things and are deliberately not interchangeable. Rows come back oldest arrival first, so applying them in order to a list you already hold gives the same result as having listed afresh.
  """

  @type t :: %__MODULE__{
          signal: Mcpd.DeltaSignal.t(),
          cursor: String.t(),
          has_more: boolean(),
          traces: [Mcpd.Trace.t()] | nil,
          spans: [Mcpd.Span.t()] | nil,
          logs: [Mcpd.LogRecord.t()] | nil
        }

  @enforce_keys [:signal, :cursor, :has_more]
  defstruct [
    signal: nil,
    cursor: nil,
    has_more: nil,
    traces: [],
    spans: [],
    logs: [],
  ]
end

defmodule Mcpd.AspectLimits do
  @moduledoc """
  The bounds a clustering pass runs under. They are published because each one changes what a reader is looking at: a floor that has not been reached means no themes at all rather than an empty result, and a ceiling that has been reached means every share is a share of what was read. They are copied from the divergence watch, which is the one windowed statistic mcpd already ships. `pass_armed` is the first of them to read: the other three describe what a pass would do, and it says whether one runs.
  """

  @type t :: %__MODULE__{
          min_notes_to_cluster: non_neg_integer(),
          max_notes_per_pass: non_neg_integer(),
          page_size: non_neg_integer(),
          max_aspects: non_neg_integer(),
          pass_armed: boolean()
        }

  @enforce_keys [:min_notes_to_cluster, :max_notes_per_pass, :page_size, :max_aspects, :pass_armed]
  defstruct [
    min_notes_to_cluster: nil,
    max_notes_per_pass: nil,
    page_size: nil,
    max_aspects: nil,
    pass_armed: nil,
  ]
end

defmodule Mcpd.AspectCatalogue do
  @moduledoc """
  Every aspect a project describes traces along, with the bounds the clustering pass runs under. The four built-ins are rows in the same table as anything you define, so there is no fallback list inside a client that can quietly disagree with this one.
  """

  @type t :: %__MODULE__{
          aspects: [Mcpd.Aspect.t()],
          limits: Mcpd.AspectLimits.t()
        }

  @enforce_keys [:aspects, :limits]
  defstruct [
    aspects: [],
    limits: nil,
  ]
end

defmodule Mcpd.EntityIndexRow do
  @moduledoc """
  One entity that has aspect notes, with the roll-up a list needs to be worth reading. A row appears as soon as the first note is recorded, before any theme exists, because an empty screen with a count moving on it is a different thing from an empty screen and a reader has to be able to tell them apart.
  """

  @type t :: %__MODULE__{
          entity_kind: Mcpd.EntityKind.t(),
          entity_name: String.t(),
          trace_count: non_neg_integer(),
          note_count: non_neg_integer(),
          enabled_aspects: non_neg_integer(),
          ready_aspects: non_neg_integer(),
          readiness: Mcpd.AspectReadiness.t(),
          last_note_at: DateTime.t(),
          latest_snapshot_at: DateTime.t() | nil
        }

  @enforce_keys [:entity_kind, :entity_name, :trace_count, :note_count, :enabled_aspects, :ready_aspects, :readiness, :last_note_at]
  defstruct [
    entity_kind: nil,
    entity_name: nil,
    trace_count: nil,
    note_count: nil,
    enabled_aspects: nil,
    ready_aspects: nil,
    readiness: nil,
    last_note_at: nil,
    latest_snapshot_at: nil,
  ]
end

defmodule Mcpd.EntityIndexResponse do
  @moduledoc """
  A page of the entities that have aspect notes.
  """

  @type t :: %__MODULE__{
          entities: [Mcpd.EntityIndexRow.t()],
          page: Mcpd.Page.t()
        }

  @enforce_keys [:entities, :page]
  defstruct [
    entities: [],
    page: nil,
  ]
end

defmodule Mcpd.AspectProgressRow do
  @moduledoc """
  How far one aspect has got for one entity. Counted from stored notes, so it is what has happened and not a projection of what will.
  """

  @type t :: %__MODULE__{
          aspect: String.t(),
          readiness: Mcpd.AspectReadiness.t(),
          note_count: non_neg_integer(),
          trace_count: non_neg_integer(),
          notes_needed: non_neg_integer(),
          last_note_at: DateTime.t() | nil
        }

  @enforce_keys [:aspect, :readiness, :note_count, :trace_count, :notes_needed]
  defstruct [
    aspect: nil,
    readiness: nil,
    note_count: nil,
    trace_count: nil,
    notes_needed: nil,
    last_note_at: nil,
  ]
end

defmodule Mcpd.AspectProgress do
  @moduledoc """
  What the daemon has recorded for one entity, aspect by aspect. This is the answer to why a themes screen is empty, and it separates the three cases that look identical on screen: nothing has been recorded, enough has been recorded and no pass has run yet, or there are themes to read and the reader is looking at the wrong aspect.
  """

  @type t :: %__MODULE__{
          entity_kind: Mcpd.EntityKind.t(),
          entity_name: String.t(),
          readiness: Mcpd.AspectReadiness.t(),
          trace_count: non_neg_integer(),
          aspects: [Mcpd.AspectProgressRow.t()],
          latest_snapshot: Mcpd.ThemeSnapshot.t() | nil
        }

  @enforce_keys [:entity_kind, :entity_name, :readiness, :trace_count, :aspects]
  defstruct [
    entity_kind: nil,
    entity_name: nil,
    readiness: nil,
    trace_count: nil,
    aspects: [],
    latest_snapshot: nil,
  ]
end

defmodule Mcpd.SnapshotLandmark do
  @moduledoc """
  One snapshot in a timeline, with the reason it was chosen.
  """

  @type t :: %__MODULE__{
          snapshot: Mcpd.ThemeSnapshot.t(),
          reason: Mcpd.SnapshotLandmarkReason.t() | nil
        }

  @enforce_keys [:snapshot]
  defstruct [
    snapshot: nil,
    reason: nil,
  ]
end

defmodule Mcpd.SnapshotListResponse do
  @moduledoc """
  A selection of one entity's snapshots, oldest first, so a timeline can be drawn without reordering anything.
  """

  @type t :: %__MODULE__{
          snapshots: [Mcpd.SnapshotLandmark.t()],
          total: non_neg_integer(),
          page: Mcpd.Page.t()
        }

  @enforce_keys [:snapshots, :total, :page]
  defstruct [
    snapshots: [],
    total: nil,
    page: nil,
  ]
end

defmodule Mcpd.ThemeNode do
  @moduledoc """
  One node of one stage of a flow: a theme, the stage's noise, or the overflow the stage folded away.
  """

  @type t :: %__MODULE__{
          node_id: String.t(),
          kind: Mcpd.ThemeNodeKind.t(),
          label: String.t(),
          trace_count: non_neg_integer(),
          coverage: float(),
          theme_id: String.t() | nil
        }

  @enforce_keys [:node_id, :kind, :label, :trace_count, :coverage]
  defstruct [
    node_id: nil,
    kind: nil,
    label: nil,
    trace_count: nil,
    coverage: nil,
    theme_id: nil,
  ]
end

defmodule Mcpd.ThemeStage do
  @moduledoc """
  One aspect's column in a flow. Stages come back in the order the aspects were asked for, and links only ever join neighbouring stages, so the order decides which co-occurrences are visible and is not a display preference.
  """

  @type t :: %__MODULE__{
          aspect: String.t(),
          trace_count: non_neg_integer(),
          nodes: [Mcpd.ThemeNode.t()]
        }

  @enforce_keys [:aspect, :trace_count, :nodes]
  defstruct [
    aspect: nil,
    trace_count: nil,
    nodes: [],
  ]
end

defmodule Mcpd.ThemeLink do
  @moduledoc """
  A ribbon between one node of a stage and one node of the next. It says the two describe the same traces and nothing more: a link between a goal theme and an outcome theme means they were written about the same runs, not that one caused the other.
  """

  @type t :: %__MODULE__{
          source_node_id: String.t(),
          target_node_id: String.t(),
          trace_count: non_neg_integer(),
          source_share: float(),
          target_share: float()
        }

  @enforce_keys [:source_node_id, :target_node_id, :trace_count, :source_share, :target_share]
  defstruct [
    source_node_id: nil,
    target_node_id: nil,
    trace_count: nil,
    source_share: nil,
    target_share: nil,
  ]
end

defmodule Mcpd.ThemeFlow do
  @moduledoc """
  One snapshot read across two to four aspects at once: a stage per aspect, and the links between neighbouring stages. Association, never causation.
  """

  @type t :: %__MODULE__{
          snapshot: Mcpd.ThemeSnapshot.t(),
          stages: [Mcpd.ThemeStage.t()],
          links: [Mcpd.ThemeLink.t()]
        }

  @enforce_keys [:snapshot, :stages, :links]
  defstruct [
    snapshot: nil,
    stages: [],
    links: [],
  ]
end

defmodule Mcpd.ThemeDetail do
  @moduledoc """
  One theme in one snapshot, or the answer that this snapshot did not hold it.
  """

  @type t :: %__MODULE__{
          snapshot: Mcpd.ThemeSnapshot.t(),
          theme: Mcpd.Theme.t() | nil
        }

  @enforce_keys [:snapshot]
  defstruct [
    snapshot: nil,
    theme: nil,
  ]
end

defmodule Mcpd.ThemeExample do
  @moduledoc """
  One note from a theme, with the run it was written about. The two ids are different things: `note_id` identifies the sentence and `trace_id` identifies the run it describes.
  """

  @type t :: %__MODULE__{
          note_id: String.t(),
          trace_id: String.t(),
          text: String.t(),
          similarity: non_neg_integer(),
          trace_started_at: DateTime.t()
        }

  @enforce_keys [:note_id, :trace_id, :text, :similarity, :trace_started_at]
  defstruct [
    note_id: nil,
    trace_id: nil,
    text: nil,
    similarity: nil,
    trace_started_at: nil,
  ]
end

defmodule Mcpd.ThemeExampleListResponse do
  @moduledoc """
  A page of the notes behind one theme.
  """

  @type t :: %__MODULE__{
          examples: [Mcpd.ThemeExample.t()],
          page: Mcpd.Page.t()
        }

  @enforce_keys [:examples, :page]
  defstruct [
    examples: [],
    page: nil,
  ]
end

defmodule Mcpd.ThemeHistoryPoint do
  @moduledoc """
  One theme in one snapshot of its life. Every count belongs to that snapshot alone, and windows overlap, so a run of these is read as a series and never added up.
  """

  @type t :: %__MODULE__{
          snapshot_id: String.t(),
          cutoff_at: DateTime.t(),
          window_from: DateTime.t(),
          window_to: DateTime.t(),
          state: Mcpd.ThemeState.t(),
          note_count: non_neg_integer(),
          trace_count: non_neg_integer(),
          coverage: float(),
          split_from: [String.t()],
          split_into: [String.t()],
          merged_from: [String.t()],
          merged_into: [String.t()]
        }

  @enforce_keys [:snapshot_id, :cutoff_at, :window_from, :window_to, :state, :note_count, :trace_count, :coverage, :split_from, :split_into, :merged_from, :merged_into]
  defstruct [
    snapshot_id: nil,
    cutoff_at: nil,
    window_from: nil,
    window_to: nil,
    state: nil,
    note_count: nil,
    trace_count: nil,
    coverage: nil,
    split_from: [],
    split_into: [],
    merged_from: [],
    merged_into: [],
  ]
end

defmodule Mcpd.ThemeHistory do
  @moduledoc """
  One theme across every stored snapshot that holds it, oldest first. This is the only theme response that names no single snapshot, because the series is the answer.
  """

  @type t :: %__MODULE__{
          theme_id: String.t(),
          aspect: String.t(),
          label: String.t(),
          points: [Mcpd.ThemeHistoryPoint.t()],
          page: Mcpd.Page.t()
        }

  @enforce_keys [:theme_id, :aspect, :label, :points, :page]
  defstruct [
    theme_id: nil,
    aspect: nil,
    label: nil,
    points: [],
    page: nil,
  ]
end

defmodule Mcpd.TraceAspectNotes do
  @moduledoc """
  Every aspect note recorded about one trace, which is at most one per aspect. Read it beside the trace itself: a note is the sentence that put this run in each of its themes, so it is the shortest route from a theme back to what actually happened.
  """

  @type t :: %__MODULE__{
          trace_id: String.t(),
          notes: [Mcpd.AspectNote.t()]
        }

  @enforce_keys [:trace_id, :notes]
  defstruct [
    trace_id: nil,
    notes: [],
  ]
end

defmodule Mcpd.AccountCredentials do
  @moduledoc """
  An email address and a password, for sign-up and for log-in.
  """

  @type t :: %__MODULE__{
          email: String.t(),
          password: String.t()
        }

  @enforce_keys [:email, :password]
  defstruct [
    email: nil,
    password: nil,
  ]
end

defmodule Mcpd.AccountSession do
  @moduledoc """
  A logged-in session. Send `token` as `Authorization: Bearer <token>`.
  """

  @type t :: %__MODULE__{
          token: String.t(),
          expires_at: DateTime.t(),
          email: String.t(),
          project: String.t()
        }

  @enforce_keys [:token, :expires_at, :email, :project]
  defstruct [
    token: nil,
    expires_at: nil,
    email: nil,
    project: nil,
  ]
end

defmodule Mcpd.Account do
  @moduledoc """
  The account a session belongs to.
  """

  @type t :: %__MODULE__{
          email: String.t(),
          project: String.t(),
          created_at: DateTime.t()
        }

  @enforce_keys [:email, :project, :created_at]
  defstruct [
    email: nil,
    project: nil,
    created_at: nil,
  ]
end

defmodule Mcpd.AccountKey do
  @moduledoc """
  One API key an account holds. The secret is not part of it: the daemon keeps only a digest.
  """

  @type t :: %__MODULE__{
          id: String.t(),
          scope: Mcpd.KeyScope.t(),
          prefix: String.t(),
          created_at: DateTime.t(),
          name: String.t() | nil
        }

  @enforce_keys [:id, :scope, :prefix, :created_at]
  defstruct [
    id: nil,
    scope: nil,
    prefix: nil,
    created_at: nil,
    name: nil,
  ]
end

defmodule Mcpd.CreateAccountKeyRequest do
  @moduledoc """
  A request for a new API key.
  """

  @type t :: %__MODULE__{
          scope: Mcpd.KeyScope.t(),
          name: String.t() | nil
        }

  @enforce_keys [:scope]
  defstruct [
    scope: nil,
    name: nil,
  ]
end

defmodule Mcpd.CreatedAccountKey do
  @moduledoc """
  A new API key, with its secret. This is the only time the secret is shown.
  """

  @type t :: %__MODULE__{
          key: Mcpd.AccountKey.t(),
          secret: String.t()
        }

  @enforce_keys [:key, :secret]
  defstruct [
    key: nil,
    secret: nil,
  ]
end

defmodule Mcpd.Span do
  @moduledoc """
  One unit of work. Spans are the backbone of mcpd: logs, metrics, scores and feedback all hang off a span or a trace.

  Stored in the `spans` table.
  """

  @type t :: %__MODULE__{
          trace_id: String.t(),
          span_id: String.t(),
          name: String.t(),
          kind: Mcpd.SpanKind.t(),
          status: Mcpd.SpanStatus.t(),
          start_time: DateTime.t(),
          is_root: boolean(),
          is_event: boolean(),
          correlation: Mcpd.Correlation.t(),
          received_at: DateTime.t(),
          parent_span_id: String.t() | nil,
          external_parent_span_id: String.t() | nil,
          end_time: DateTime.t() | nil,
          duration_ns: non_neg_integer() | nil,
          entity_kind: Mcpd.EntityKind.t() | nil,
          entity_id: String.t() | nil,
          entity_name: String.t() | nil,
          input: term() | nil,
          output: term() | nil,
          attributes: term() | nil,
          metadata: term() | nil,
          usage: Mcpd.TokenUsage.t() | nil,
          cost: Mcpd.Cost.t() | nil,
          model_parameters: Mcpd.ModelParameters.t() | nil,
          error: Mcpd.SpanError.t() | nil,
          links: [Mcpd.SpanLink.t()] | nil,
          tags: [String.t()] | nil,
          model: String.t() | nil,
          provider: String.t() | nil,
          ingest_key: String.t() | nil
        }

  @enforce_keys [:trace_id, :span_id, :name, :kind, :status, :start_time, :is_root, :is_event, :correlation, :received_at]
  defstruct [
    trace_id: nil,
    span_id: nil,
    name: nil,
    kind: nil,
    status: nil,
    start_time: nil,
    is_root: nil,
    is_event: nil,
    correlation: nil,
    received_at: nil,
    parent_span_id: nil,
    external_parent_span_id: nil,
    end_time: nil,
    duration_ns: nil,
    entity_kind: nil,
    entity_id: nil,
    entity_name: nil,
    input: nil,
    output: nil,
    attributes: nil,
    metadata: nil,
    usage: nil,
    cost: nil,
    model_parameters: nil,
    error: nil,
    links: [],
    tags: [],
    model: nil,
    provider: nil,
    ingest_key: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "spans"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["trace_id", "span_id"]
end

defmodule Mcpd.Trace do
  @moduledoc """
  A whole run, projected from its spans. mcpd materialises this at ingest so the list view is one indexed read rather than an aggregation over every span. One per project and trace id: a trace id crosses service boundaries, so two projects that call each other each hold their own run under it, built from their own spans.

  Stored in the `traces` table.
  """

  @type t :: %__MODULE__{
          trace_id: String.t(),
          name: String.t(),
          kind: Mcpd.SpanKind.t(),
          status: Mcpd.SpanStatus.t(),
          start_time: DateTime.t(),
          span_count: non_neg_integer(),
          error_count: non_neg_integer(),
          model_count: non_neg_integer(),
          tool_count: non_neg_integer(),
          correlation: Mcpd.Correlation.t(),
          is_complete: boolean(),
          received_at: DateTime.t(),
          root_span_id: String.t() | nil,
          end_time: DateTime.t() | nil,
          duration_ns: non_neg_integer() | nil,
          usage: Mcpd.TokenUsage.t() | nil,
          cost: Mcpd.Cost.t() | nil,
          tags: [String.t()] | nil,
          score_summary: term() | nil,
          feedback_summary: term() | nil
        }

  @enforce_keys [:trace_id, :name, :kind, :status, :start_time, :span_count, :error_count, :model_count, :tool_count, :correlation, :is_complete, :received_at]
  defstruct [
    trace_id: nil,
    name: nil,
    kind: nil,
    status: nil,
    start_time: nil,
    span_count: nil,
    error_count: nil,
    model_count: nil,
    tool_count: nil,
    correlation: nil,
    is_complete: nil,
    received_at: nil,
    root_span_id: nil,
    end_time: nil,
    duration_ns: nil,
    usage: nil,
    cost: nil,
    tags: [],
    score_summary: nil,
    feedback_summary: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "traces"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["project", "trace_id"]
end

defmodule Mcpd.LogRecord do
  @moduledoc """
  A structured log line, correlated to the span that produced it. Logs are stored beside traces so 'show me the logs for this run' is one query, not a context switch to another tool.

  Stored in the `logs` table.
  """

  @type t :: %__MODULE__{
          log_id: String.t(),
          timestamp: DateTime.t(),
          level: Mcpd.LogLevel.t(),
          message: String.t(),
          correlation: Mcpd.Correlation.t(),
          received_at: DateTime.t(),
          trace_id: String.t() | nil,
          span_id: String.t() | nil,
          fields: term() | nil,
          logger: String.t() | nil
        }

  @enforce_keys [:log_id, :timestamp, :level, :message, :correlation, :received_at]
  defstruct [
    log_id: nil,
    timestamp: nil,
    level: nil,
    message: nil,
    correlation: nil,
    received_at: nil,
    trace_id: nil,
    span_id: nil,
    fields: nil,
    logger: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "logs"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["log_id"]
end

defmodule Mcpd.MetricPoint do
  @moduledoc """
  One numeric observation. mcpd aggregates these into time buckets on read, and keeps raw points only as long as the raw retention class allows.

  Stored in the `metrics` table.
  """

  @type t :: %__MODULE__{
          metric_id: String.t(),
          name: String.t(),
          kind: Mcpd.MetricKind.t(),
          value: float(),
          timestamp: DateTime.t(),
          correlation: Mcpd.Correlation.t(),
          received_at: DateTime.t(),
          unit: String.t() | nil,
          labels: term() | nil,
          buckets: term() | nil,
          trace_id: String.t() | nil,
          span_id: String.t() | nil
        }

  @enforce_keys [:metric_id, :name, :kind, :value, :timestamp, :correlation, :received_at]
  defstruct [
    metric_id: nil,
    name: nil,
    kind: nil,
    value: nil,
    timestamp: nil,
    correlation: nil,
    received_at: nil,
    unit: nil,
    labels: nil,
    buckets: nil,
    trace_id: nil,
    span_id: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "metrics"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["metric_id"]
end

defmodule Mcpd.Score do
  @moduledoc """
  A machine evaluation of a span or a trace.

  Stored in the `scores` table.
  """

  @type t :: %__MODULE__{
          score_id: String.t(),
          scorer_id: String.t(),
          source: Mcpd.ScoreSource.t(),
          value: float(),
          timestamp: DateTime.t(),
          correlation: Mcpd.Correlation.t(),
          trace_id: String.t() | nil,
          span_id: String.t() | nil,
          scorer_version: String.t() | nil,
          min: float() | nil,
          max: float() | nil,
          label: String.t() | nil,
          reason: String.t() | nil,
          score_trace_id: String.t() | nil,
          cost: Mcpd.Cost.t() | nil,
          metadata: term() | nil
        }

  @enforce_keys [:score_id, :scorer_id, :source, :value, :timestamp, :correlation]
  defstruct [
    score_id: nil,
    scorer_id: nil,
    source: nil,
    value: nil,
    timestamp: nil,
    correlation: nil,
    trace_id: nil,
    span_id: nil,
    scorer_version: nil,
    min: 0.0,
    max: 1.0,
    label: nil,
    reason: nil,
    score_trace_id: nil,
    cost: nil,
    metadata: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "scores"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["score_id"]
end

defmodule Mcpd.Feedback do
  @moduledoc """
  A human evaluation. Kept distinct from `Score` because the two answer different questions and must never be averaged together.

  Stored in the `feedback` table.
  """

  @type t :: %__MODULE__{
          feedback_id: String.t(),
          kind: Mcpd.FeedbackKind.t(),
          value: term(),
          timestamp: DateTime.t(),
          correlation: Mcpd.Correlation.t(),
          trace_id: String.t() | nil,
          span_id: String.t() | nil,
          comment: String.t() | nil,
          author_id: String.t() | nil,
          author_kind: String.t() | nil,
          metadata: term() | nil
        }

  @enforce_keys [:feedback_id, :kind, :value, :timestamp, :correlation]
  defstruct [
    feedback_id: nil,
    kind: nil,
    value: nil,
    timestamp: nil,
    correlation: nil,
    trace_id: nil,
    span_id: nil,
    comment: nil,
    author_id: nil,
    author_kind: nil,
    metadata: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "feedback"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["feedback_id"]
end

defmodule Mcpd.Project do
  @moduledoc """
  The tenancy boundary. Every signal belongs to exactly one project; retention, redaction, sampling, budgets and access are all configured per project.

  Stored in the `projects` table.
  """

  @type t :: %__MODULE__{
          project_id: String.t(),
          name: String.t(),
          created_at: DateTime.t(),
          retention_days: non_neg_integer() | nil,
          sampling: term() | nil,
          redaction: term() | nil,
          budget: term() | nil,
          metadata: term() | nil
        }

  @enforce_keys [:project_id, :name, :created_at]
  defstruct [
    project_id: nil,
    name: nil,
    created_at: nil,
    retention_days: nil,
    sampling: nil,
    redaction: nil,
    budget: nil,
    metadata: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "projects"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["project_id"]
end

defmodule Mcpd.Dataset do
  @moduledoc """
  A fixed collection of inputs, with optional expected outputs, that an experiment runs against. Datasets are how a change is proved not to regress.

  Stored in the `datasets` table.
  """

  @type t :: %__MODULE__{
          dataset_id: String.t(),
          name: String.t(),
          item_count: non_neg_integer(),
          created_at: DateTime.t(),
          correlation: Mcpd.Correlation.t(),
          description: String.t() | nil
        }

  @enforce_keys [:dataset_id, :name, :item_count, :created_at, :correlation]
  defstruct [
    dataset_id: nil,
    name: nil,
    item_count: nil,
    created_at: nil,
    correlation: nil,
    description: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "datasets"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["dataset_id"]
end

defmodule Mcpd.DatasetItem do
  @moduledoc """
  One case in a dataset.

  Stored in the `dataset_items` table.
  """

  @type t :: %__MODULE__{
          item_id: String.t(),
          dataset_id: String.t(),
          input: term(),
          expected: term() | nil,
          metadata: term() | nil,
          source_trace_id: String.t() | nil
        }

  @enforce_keys [:item_id, :dataset_id, :input]
  defstruct [
    item_id: nil,
    dataset_id: nil,
    input: nil,
    expected: nil,
    metadata: nil,
    source_trace_id: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "dataset_items"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["item_id"]
end

defmodule Mcpd.Experiment do
  @moduledoc """
  One evaluation run of a system against a dataset. Comparing two experiments is how a change is accepted or rejected.

  Stored in the `experiments` table.
  """

  @type t :: %__MODULE__{
          experiment_id: String.t(),
          name: String.t(),
          dataset_id: String.t(),
          status: Mcpd.ExperimentStatus.t(),
          item_count: non_neg_integer(),
          correlation: Mcpd.Correlation.t(),
          started_at: DateTime.t() | nil,
          finished_at: DateTime.t() | nil,
          summary: term() | nil,
          baseline_id: String.t() | nil,
          cost: Mcpd.Cost.t() | nil,
          error: String.t() | nil,
          metadata: term() | nil
        }

  @enforce_keys [:experiment_id, :name, :dataset_id, :status, :item_count, :correlation]
  defstruct [
    experiment_id: nil,
    name: nil,
    dataset_id: nil,
    status: nil,
    item_count: nil,
    correlation: nil,
    started_at: nil,
    finished_at: nil,
    summary: nil,
    baseline_id: nil,
    cost: nil,
    error: nil,
    metadata: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "experiments"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["experiment_id"]
end

defmodule Mcpd.AuditEvent do
  @moduledoc """
  One recorded attempt to read stored payloads, or to delete them. Written by the handlers that hand prompts and completions back out of mcpd, so the question "who read this run" has an answer. The audit log is not trimmed by the retention pass: a log that deleted itself on the same schedule as the data it describes could not say who read the data that has since gone.

  Stored in the `audit_log` table.
  """

  @type t :: %__MODULE__{
          audit_id: String.t(),
          timestamp: DateTime.t(),
          principal: String.t(),
          subject: String.t(),
          action: String.t(),
          project: String.t(),
          result: Mcpd.AuditResult.t(),
          resource: String.t() | nil
        }

  @enforce_keys [:audit_id, :timestamp, :principal, :subject, :action, :project, :result]
  defstruct [
    audit_id: nil,
    timestamp: nil,
    principal: nil,
    subject: nil,
    action: nil,
    project: nil,
    result: nil,
    resource: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "audit_log"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["audit_id"]
end

defmodule Mcpd.Definition do
  @moduledoc """
  A stored definition. Immutable per version: a run pins the version it started against, and that version never changes underneath it.

  Stored in the `definitions` table.
  """

  @type t :: %__MODULE__{
          project: String.t(),
          id: String.t(),
          version: non_neg_integer(),
          kind: Mcpd.DefinitionKind.t(),
          body: term(),
          created_at: DateTime.t()
        }

  @enforce_keys [:project, :id, :version, :kind, :body, :created_at]
  defstruct [
    project: nil,
    id: nil,
    version: nil,
    kind: nil,
    body: nil,
    created_at: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "definitions"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["project", "id", "version"]
end

defmodule Mcpd.Run do
  @moduledoc """
  One execution of one definition version. The daemon owns the run between steps: its journal, its deadlines, its waits, its cancel and its trace. Your own process owns everything inside a step.

  Stored in the `runs` table.
  """

  @type t :: %__MODULE__{
          run_id: String.t(),
          project: String.t(),
          definition_id: String.t(),
          definition_version: non_neg_integer(),
          status: Mcpd.RunStatus.t(),
          cursor: Mcpd.RunCursor.t(),
          correlation: Mcpd.Correlation.t(),
          trace_id: String.t(),
          steps_used: non_neg_integer(),
          cost_usd: float(),
          root_run_id: String.t(),
          depth: non_neg_integer(),
          created_at: DateTime.t(),
          updated_at: DateTime.t(),
          input: term() | nil,
          output: term() | nil,
          state: term() | nil,
          error: String.t() | nil,
          max_steps: non_neg_integer() | nil,
          max_cost_usd: float() | nil,
          deadline_at: DateTime.t() | nil,
          idempotency_key: String.t() | nil,
          parent_run_id: String.t() | nil,
          parent_step_key: String.t() | nil,
          ended_at: DateTime.t() | nil
        }

  @enforce_keys [:run_id, :project, :definition_id, :definition_version, :status, :cursor, :correlation, :trace_id, :steps_used, :cost_usd, :root_run_id, :depth, :created_at, :updated_at]
  defstruct [
    run_id: nil,
    project: nil,
    definition_id: nil,
    definition_version: nil,
    status: nil,
    cursor: nil,
    correlation: nil,
    trace_id: nil,
    steps_used: nil,
    cost_usd: nil,
    root_run_id: nil,
    depth: nil,
    created_at: nil,
    updated_at: nil,
    input: nil,
    output: nil,
    state: nil,
    error: nil,
    max_steps: nil,
    max_cost_usd: nil,
    deadline_at: nil,
    idempotency_key: nil,
    parent_run_id: nil,
    parent_step_key: nil,
    ended_at: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "runs"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["run_id"]
end

defmodule Mcpd.StepAttempt do
  @moduledoc """
  One attempt at one step of a run. A run's journal is these rows, append only: a retry is a new row, never a rewrite of the old one.

  Stored in the `step_attempts` table.
  """

  @type t :: %__MODULE__{
          run_id: String.t(),
          step_key: String.t(),
          attempt: non_neg_integer(),
          kind: Mcpd.StepKind.t(),
          effect: Mcpd.Effect.t(),
          status: Mcpd.AttemptStatus.t(),
          span_id: String.t(),
          created_at: DateTime.t(),
          input: term() | nil,
          output: term() | nil,
          error: Mcpd.StepError.t() | nil,
          usage: Mcpd.StepUsage.t() | nil,
          cost_usd: float() | nil,
          lease_owner: String.t() | nil,
          lease_until: DateTime.t() | nil,
          started_at: DateTime.t() | nil,
          orphaned_at: DateTime.t() | nil,
          reclaim_until: DateTime.t() | nil,
          attempts_left: non_neg_integer() | nil,
          ended_at: DateTime.t() | nil
        }

  @enforce_keys [:run_id, :step_key, :attempt, :kind, :effect, :status, :span_id, :created_at]
  defstruct [
    run_id: nil,
    step_key: nil,
    attempt: nil,
    kind: nil,
    effect: nil,
    status: nil,
    span_id: nil,
    created_at: nil,
    input: nil,
    output: nil,
    error: nil,
    usage: nil,
    cost_usd: nil,
    lease_owner: nil,
    lease_until: nil,
    started_at: nil,
    orphaned_at: nil,
    reclaim_until: nil,
    attempts_left: nil,
    ended_at: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "step_attempts"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["run_id", "step_key", "attempt"]
end

defmodule Mcpd.RunEvent do
  @moduledoc """
  One row of a run's event log. Written in the transaction that made the change it describes, with `seq` allocated from the run header, so a reader that has seen a row has seen every row below it.

  Stored in the `run_events` table.
  """

  @type t :: %__MODULE__{
          run_id: String.t(),
          seq: non_neg_integer(),
          kind: Mcpd.RunEventKind.t(),
          at: DateTime.t(),
          payload: term() | nil
        }

  @enforce_keys [:run_id, :seq, :kind, :at]
  defstruct [
    run_id: nil,
    seq: nil,
    kind: nil,
    at: nil,
    payload: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "run_events"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["run_id", "seq"]
end

defmodule Mcpd.Aspect do
  @moduledoc """
  An axis that traces are described along. An aspect is a definition and not a measurement: it carries the name a note is filed under, the words an operator reads on screen and, where a model writes the sentence, the instruction handed to whatever produces it. Four ship with the daemon as ordinary rows rather than as a list compiled into a client, so there is one source of truth for what exists: goal, what the user wanted; sentiment, the tone they expressed; behaviour, what the agent did about it; and outcome, how the interaction ended. A stored version never changes, so a note recorded a month ago still names the definition it was written against.

  Stored in the `aspects` table.
  """

  @type t :: %__MODULE__{
          project: String.t(),
          name: String.t(),
          label: String.t(),
          description: String.t(),
          derived: boolean(),
          built_in: boolean(),
          enabled: boolean(),
          sort_order: non_neg_integer(),
          version: non_neg_integer(),
          created_at: DateTime.t(),
          instruction: String.t() | nil
        }

  @enforce_keys [:project, :name, :label, :description, :derived, :built_in, :enabled, :sort_order, :version, :created_at]
  defstruct [
    project: nil,
    name: nil,
    label: nil,
    description: nil,
    derived: nil,
    built_in: nil,
    enabled: nil,
    sort_order: nil,
    version: nil,
    created_at: nil,
    instruction: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "aspects"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["project", "name", "version"]
end

defmodule Mcpd.AspectNote do
  @moduledoc """
  One sentence recorded against one trace along one aspect. A note proves one thing: that this sentence was recorded against that trace by the definition named in `aspect`, at the version in `definition_version`. It does not prove the sentence is true. Nothing here is checked against the trace it describes, so a note is evidence of what a definition said and not evidence of what happened, and the trace is what settles the difference. The daemon also keeps a small sketch of the wording, which is what clustering compares, and that sketch is deliberately not a field on this entity: it is a detail of how the pass is stored, a caller can do nothing with it, and publishing it would fix the hashing where it stands today. The text is payload. It passes through the same redactor as every other signal on the way in, retention sweeps it, and erasing a subject erases it.

  Stored in the `aspect_notes` table.
  """

  @type t :: %__MODULE__{
          note_id: String.t(),
          trace_id: String.t(),
          aspect: String.t(),
          text: String.t(),
          definition_version: non_neg_integer(),
          created_at: DateTime.t(),
          correlation: Mcpd.Correlation.t(),
          model: String.t() | nil
        }

  @enforce_keys [:note_id, :trace_id, :aspect, :text, :definition_version, :created_at, :correlation]
  defstruct [
    note_id: nil,
    trace_id: nil,
    aspect: nil,
    text: nil,
    definition_version: nil,
    created_at: nil,
    correlation: nil,
    model: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "aspect_notes"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["note_id"]
end

defmodule Mcpd.ThemeSnapshot do
  @moduledoc """
  One clustering pass over one entity, covering a moving window of notes. A snapshot is what turns a clustering into a series: every theme belongs to exactly one, and every theme query names one. Windows overlap, so counts from two snapshots must never be added together. A trace whose note falls inside three windows is counted once in each of them, and summing the three counts it three times. Compare snapshots or read one; do not sum them. Reads never recompute either: a pass writes a snapshot on its own timer and every read is a lookup of what it wrote, so refreshing a screen shows the same numbers rather than a fresh clustering.

  Stored in the `theme_snapshots` table.
  """

  @type t :: %__MODULE__{
          snapshot_id: String.t(),
          project: String.t(),
          entity_kind: Mcpd.EntityKind.t(),
          entity_name: String.t(),
          aspects: [String.t()],
          ordinal: non_neg_integer(),
          cutoff_at: DateTime.t(),
          window_from: DateTime.t(),
          window_to: DateTime.t(),
          trace_count: non_neg_integer(),
          note_count: non_neg_integer(),
          capped: boolean()
        }

  @enforce_keys [:snapshot_id, :project, :entity_kind, :entity_name, :aspects, :ordinal, :cutoff_at, :window_from, :window_to, :trace_count, :note_count, :capped]
  defstruct [
    snapshot_id: nil,
    project: nil,
    entity_kind: nil,
    entity_name: nil,
    aspects: [],
    ordinal: nil,
    cutoff_at: nil,
    window_from: nil,
    window_to: nil,
    trace_count: nil,
    note_count: nil,
    capped: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "theme_snapshots"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["snapshot_id"]
end

defmodule Mcpd.Theme do
  @moduledoc """
  A cluster of aspect notes that share their wording, in one snapshot along one aspect. A theme groups aspect notes that share their wording, which is why mcpd needs no model and no embedding to build one. It does not read the sentences, so two notes that mean the same thing in different words land in separate themes, and a theme's label is the note most like the others rather than a summary written for it. The clustering is a pure function of the notes it reads, with no floating-point arithmetic in the comparison, so two replicas of one deployment reading the same notes write the same themes and cannot disagree about where a boundary falls.

  Stored in the `themes` table.
  """

  @type t :: %__MODULE__{
          theme_id: String.t(),
          snapshot_id: String.t(),
          aspect: String.t(),
          entity_kind: Mcpd.EntityKind.t(),
          entity_name: String.t(),
          label: String.t(),
          label_note_id: String.t(),
          label_trace_id: String.t(),
          state: Mcpd.ThemeState.t(),
          note_count: non_neg_integer(),
          trace_count: non_neg_integer(),
          coverage: float()
        }

  @enforce_keys [:theme_id, :snapshot_id, :aspect, :entity_kind, :entity_name, :label, :label_note_id, :label_trace_id, :state, :note_count, :trace_count, :coverage]
  defstruct [
    theme_id: nil,
    snapshot_id: nil,
    aspect: nil,
    entity_kind: nil,
    entity_name: nil,
    label: nil,
    label_note_id: nil,
    label_trace_id: nil,
    state: nil,
    note_count: nil,
    trace_count: nil,
    coverage: nil,
  ]

  @doc "The storage table this entity lives in."
  @spec table() :: String.t()
  def table, do: "themes"

  @doc "The primary key, as declared in the spec."
  @spec primary_key() :: [String.t()]
  def primary_key, do: ["snapshot_id", "aspect", "theme_id"]
end

