defmodule Mcpd.Http do
  @moduledoc """
  The transport, on OTP's own `:httpc`.

  No HTTP dependency is added for this. A library whose job is to watch a system
  should not be the reason that system's dependency tree changes, and `:httpc`
  ships with the runtime.

  Contract section 6 lives here: a batch is gzipped above 1 KiB, carries the
  bearer token when a key is set, and is retried on 429, 502, 503, 504 and a
  connection error, three attempts in all, with exponential backoff from 250 ms
  and jitter, honouring `Retry-After`. Any other 4xx is a malformed batch, and
  it will still be malformed in a second, so it is never sent twice.
  """

  @user_agent ~c"mcpd-elixir/0.1.0"

  # Bodies larger than this are gzipped. Below it the compression costs more
  # CPU than it saves in bytes.
  @gzip_threshold 1024

  # Total attempts for one batch, the first included.
  @max_attempts 3

  # The first backoff step. Each attempt doubles it, and jitter of up to one
  # base step is added on top: a fleet that all retry at exactly 250 ms
  # reproduces the load spike that caused the failure.
  @base_backoff_ms 250

  # However long a server asks for, the wait stops here. An honest
  # `Retry-After: 3600` must not hold a batch, and a shutdown, for an hour.
  @max_retry_after_ms 30_000

  @retry_statuses [429, 502, 503, 504]

  # The SDK's own `:httpc` profile, see `profile/0`.
  @profile :mcpd

  # The most persistent connections the profile holds to one host, in
  # `:httpc`'s own terms. Its default is 2.
  @max_sessions 8

  @doc "Total attempts made for one batch, the first included."
  @spec max_attempts() :: pos_integer()
  def max_attempts, do: @max_attempts

  @doc "The body size above which a batch is gzipped."
  @spec gzip_threshold() :: pos_integer()
  def gzip_threshold, do: @gzip_threshold

  @doc """
  POST one batch to the ingest URL, retrying the failures worth retrying.

  Returns `{:ok, body}` or `{:error, reason}`, never raises, and never returns
  before the last attempt has been judged. It sleeps between attempts, so it
  belongs in the worker's sender process, never on the agent's.

  Contract behaviour B1 lives here: every failure path returns an error tuple.
  """
  @spec post_batch(String.t(), map(), keyword()) :: {:ok, binary()} | {:error, term()}
  def post_batch(url, body, opts \\ []) do
    payload = body |> :json.encode() |> IO.iodata_to_binary()

    {payload, encoding} =
      if byte_size(payload) > @gzip_threshold,
        do: {:zlib.gzip(payload), [{~c"content-encoding", ~c"gzip"}]},
        else: {payload, []}

    headers = auth_headers(opts[:api_key]) ++ encoding ++ common_headers()
    request = {to_charlist(url), headers, ~c"application/json", payload}
    attempt(fn -> request(:post, request, opts) end, 1)
  rescue
    # `:json.encode/1` raises on a payload it cannot encode, such as a tuple.
    # That is a bug in the caller's data, and the batch is lost, but the
    # sender process and the agent both survive it.
    e -> {:error, {:encode, Exception.message(e)}}
  end

  @doc """
  POST a JSON body to a run endpoint and decode the answer.

  Not retried: a worker call that is retried blindly can land twice, and the
  daemon's own 409 table is how the caller learns what happened. A refused
  request comes back as `{:error, {:http, status, body, retry_after}}` with
  the whole body, so a 409 can be read for its reason.
  """
  @spec post_json(String.t(), term(), keyword()) :: {:ok, term()} | {:error, term()}
  def post_json(url, body, opts \\ []), do: send_json(:post, url, body, opts)

  @doc "PUT a JSON body and decode the answer. Not retried, for the reason `post_json/3` gives."
  @spec put_json(String.t(), term(), keyword()) :: {:ok, term()} | {:error, term()}
  def put_json(url, body, opts \\ []), do: send_json(:put, url, body, opts)

  defp send_json(method, url, body, opts) do
    payload = body |> :json.encode() |> IO.iodata_to_binary()
    headers = auth_headers(opts[:api_key]) ++ common_headers()
    request = {to_charlist(url), headers, ~c"application/json", payload}

    case request(method, request, Keyword.put(opts, :keep_body, true)) do
      {:ok, answer} -> decode(answer)
      other -> other
    end
  rescue
    e -> {:error, {:encode, Exception.message(e)}}
  end

  @doc "GET a JSON document and decode it. Reads are not retried: a caller waiting on an answer would rather have the failure than the delay."
  @spec get_json(String.t(), keyword()) :: {:ok, term()} | {:error, term()}
  def get_json(url, opts \\ []) do
    headers = auth_headers(opts[:api_key]) ++ common_headers()

    case request(:get, {to_charlist(url), headers}, opts) do
      {:ok, body} -> decode(body)
      other -> other
    end
  end

  @doc """
  Whether a failure is worth another attempt.

  A status the server chose is retried only when it is one of the four the
  contract names. Anything that is not a status at all, such as a refused
  connection or a timeout, is exactly what retrying exists for.
  """
  @spec retryable?(term()) :: boolean()
  def retryable?({:http, status, _body, _retry_after}), do: status in @retry_statuses
  def retryable?({:encode, _}), do: false
  def retryable?(_), do: true

  @doc """
  How long to wait before attempt `next`, in milliseconds.

  A `Retry-After` the server sent wins, capped: it knows when it will be ready
  and the backoff curve does not.
  """
  @spec delay_ms(term(), pos_integer()) :: non_neg_integer()
  def delay_ms({:http, _status, _body, retry_after}, _next) when is_integer(retry_after),
    do: min(retry_after, @max_retry_after_ms)

  def delay_ms(_reason, next) do
    base = @base_backoff_ms * Integer.pow(2, next - 2)
    base + :rand.uniform(@base_backoff_ms) - 1
  end

  # ------------------------------------------------------------- internals ---

  defp attempt(send, n) do
    case send.() do
      {:ok, body} ->
        {:ok, body}

      {:error, reason} = failure ->
        if n < @max_attempts and retryable?(reason) do
          Process.sleep(delay_ms(reason, n + 1))
          attempt(send, n + 1)
        else
          failure
        end
    end
  end

  defp decode(body) do
    {:ok, :json.decode(body)}
  rescue
    # A daemon that answers with something other than JSON is a bad response,
    # not a reason to take the caller down with it.
    e -> {:error, {:bad_json, Exception.message(e)}}
  end

  defp common_headers, do: [{~c"accept", ~c"application/json"}, {~c"user-agent", @user_agent}]

  defp auth_headers(nil), do: []
  defp auth_headers(""), do: []
  defp auth_headers(key), do: [{~c"authorization", to_charlist("Bearer " <> key)}]

  @doc """
  The `:httpc` profile every request of this SDK goes through, started on
  first use.

  Its own, and not OTP's `:default`, which belongs to the host application.
  `:default` keeps a connection alive once it has answered and queues the next
  request to the same host on it even while it is busy, so a read made while a
  worker's claim was held open on that connection waited for the claim, up to
  the length of the long poll, and timed out although the daemon would have
  answered at once. Here nothing is queued on a busy connection: with no
  keep-alive queue and no pipeline, every request has a connection of its own.
  """
  @spec profile() :: atom()
  def profile do
    :inets.start()
    :ssl.start()

    case :inets.start(:httpc, profile: @profile) do
      {:ok, _pid} ->
        :ok =
          :httpc.set_options(
            [max_keep_alive_length: 0, max_pipeline_length: 0, max_sessions: @max_sessions],
            @profile
          )

      {:error, {:already_started, _pid}} ->
        :ok
    end

    @profile
  end

  defp request(method, req, opts) do
    profile = profile()
    timeout = opts[:timeout] || 10_000

    # `wait_ms` is how long the daemon may hold the request open before it
    # answers, as it does for a long poll. The request's timeout is that plus
    # the configured timeout; otherwise the client would cancel a poll longer
    # than its own timeout, which is every idle claim. Connecting is not part
    # of the wait, so that bound stays as it was.
    http_opts = [timeout: timeout + (opts[:wait_ms] || 0), connect_timeout: timeout]
    result = :httpc.request(method, req, http_opts, [body_format: :binary], profile)

    case result do
      {:ok, {{_, status, _}, _headers, body}} when status >= 200 and status < 300 ->
        {:ok, body}

      {:ok, {{_, status, _}, headers, body}} ->
        limit = if opts[:keep_body], do: 4_096, else: 300
        {:error, {:http, status, String.slice(to_string(body), 0, limit), retry_after(headers)}}

      {:error, reason} ->
        {:error, reason}
    end
  catch
    # `:httpc` can exit rather than return on some malformed inputs. An
    # instrumentation call must survive that too.
    kind, reason -> {:error, {kind, reason}}
  end

  # `:httpc` lowercases header names, so one comparison is enough. Only the
  # delay-seconds form is read: a date form needs a clock comparison that is
  # wrong the moment the two clocks disagree, and the backoff curve is a fine
  # answer when the header cannot be read.
  defp retry_after(headers) do
    case List.keyfind(headers, ~c"retry-after", 0) do
      {_, value} ->
        case value |> to_string() |> String.trim() |> Integer.parse() do
          {seconds, ""} when seconds >= 0 -> seconds * 1_000
          _ -> nil
        end

      nil ->
        nil
    end
  end
end
