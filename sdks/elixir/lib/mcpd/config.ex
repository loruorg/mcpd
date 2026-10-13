defmodule Mcpd.Config do
  @moduledoc """
  Client configuration, with the environment variable names the contract fixes.

  Every field is settable in code and by environment variable, code winning. The
  names are identical in all six SDKs on purpose: a value copied from one
  service to another has to keep working when the two are written in different
  languages, so they are not a per-SDK decision.
  """

  @type t :: %__MODULE__{
          endpoint: String.t(),
          api_key: String.t() | nil,
          project: String.t(),
          environment: String.t() | nil,
          service_name: String.t() | nil,
          service_version: String.t() | nil,
          sample: float(),
          batch_size: pos_integer(),
          flush_interval: pos_integer(),
          max_queue: pos_integer(),
          timeout: pos_integer(),
          enabled: boolean(),
          capture_payloads: boolean(),
          log_level: Mcpd.LogLevel.t(),
          sink: {module(), term()},
          redact: (term() -> term()) | nil
        }

  defstruct endpoint: "http://localhost:4317",
            api_key: nil,
            project: "default",
            environment: nil,
            service_name: nil,
            service_version: nil,
            sample: 1.0,
            batch_size: 128,
            flush_interval: 2_000,
            max_queue: 8_192,
            timeout: 10_000,
            enabled: true,
            capture_payloads: true,
            log_level: :debug,
            sink: {Mcpd.Sink.Http, []},
            redact: nil

  @doc """
  Build a config from keyword options, falling back to the environment.

  `Mcpd.Config.new()` with no arguments must work against a local daemon. That
  is the five-minutes-to-first-trace promise expressed as a default.
  """
  @spec new(keyword()) :: t()
  def new(opts \\ []) do
    %__MODULE__{
      endpoint: pick(opts, :endpoint, "MCPD_ENDPOINT", "http://localhost:4317"),
      api_key: pick(opts, :api_key, "MCPD_API_KEY", nil),
      project: pick(opts, :project, "MCPD_PROJECT", "default"),
      environment: pick(opts, :environment, "MCPD_ENVIRONMENT", nil),
      service_name: pick(opts, :service_name, "MCPD_SERVICE_NAME", nil),
      service_version: pick(opts, :service_version, "MCPD_SERVICE_VERSION", nil),
      sample: opts[:sample] || num("MCPD_SAMPLE", 1.0, :float),
      batch_size: opts[:batch_size] || num("MCPD_BATCH_SIZE", 128, :int),
      flush_interval: opts[:flush_interval] || num("MCPD_FLUSH_INTERVAL_MS", 2_000, :int),
      max_queue: opts[:max_queue] || num("MCPD_MAX_QUEUE", 8_192, :int),
      timeout: opts[:timeout] || num("MCPD_TIMEOUT_MS", 10_000, :int),
      enabled: flag(opts, :enabled, "MCPD_ENABLED"),
      capture_payloads: flag(opts, :capture_payloads, "MCPD_CAPTURE_PAYLOADS"),
      log_level: level(opts[:log_level]),
      sink: sink(opts[:sink]),
      redact: redact(opts[:redact])
    }
  end

  defp pick(opts, key, env, default) do
    case opts[key] do
      nil -> System.get_env(env) || default
      v -> v
    end
  end

  # A malformed environment value falls back to the default rather than
  # crashing. Contract behaviour B1 starts at construction: a typo in a
  # deployment variable must not stop the user's application from booting.
  defp num(env, default, kind) do
    with raw when is_binary(raw) <- System.get_env(env),
         {v, ""} <- parse(raw, kind) do
      v
    else
      _ -> default
    end
  end

  defp parse(raw, :int), do: Integer.parse(raw)
  defp parse(raw, :float), do: Float.parse(raw)

  # A switch is on unless it is set off. `0`, `false`, `no` and `off` are
  # off, in any case; anything else, including a typo, leaves the switch on,
  # because the safe reading of a value nobody meant is the default.
  defp flag(opts, key, env) do
    case opts[key] do
      nil -> System.get_env(env) == nil or String.downcase(System.get_env(env)) not in ~w(0 false no off)
      v -> v == true
    end
  end

  # The lowest level `Mcpd.log/4` records. A name outside the enum is ignored
  # rather than refused, in code and in the environment alike: a typo in a
  # deployment variable must not silence a service's logs.
  defp level(given) do
    Mcpd.Logs.parse_log_level(given) ||
      Mcpd.Logs.parse_log_level(System.get_env("MCPD_LOG_LEVEL")) || :debug
  end

  # Where batches go. A bare module means the module with no options. The
  # HTTP sink is the default and the only one that talks to a daemon; it takes
  # its address from this config, so it needs nothing of its own.
  defp sink(nil), do: {Mcpd.Sink.Http, []}
  defp sink({module, opts}) when is_atom(module), do: {module, opts}
  defp sink(module) when is_atom(module), do: {module, []}

  # A function applied to every span's payload fields and every log line's
  # fields before they are queued (contract section 2.1.1). Code only: no
  # environment variable can carry a function. A value that is not a 1-arity
  # function is not refused, so a misconfiguration never stops the
  # application's boot (B1), and it is not ignored either: the caller asked
  # for redaction, so it fails closed and every payload field is dropped.
  defp redact(nil), do: nil
  defp redact(fun) when is_function(fun, 1), do: fun
  defp redact(_other), do: fn _value -> raise ArgumentError, "redact is not a 1-arity function" end

  @doc "The URL a batch is POSTed to."
  @spec ingest_url(t()) :: String.t()
  def ingest_url(%__MODULE__{endpoint: e}), do: String.trim_trailing(e, "/") <> "/v1/ingest"

  @doc "The base URL the query API is read from."
  @spec api_url(t(), String.t()) :: String.t()
  def api_url(%__MODULE__{endpoint: e}, path),
    do: String.trim_trailing(e, "/") <> "/v1/api" <> path
end
