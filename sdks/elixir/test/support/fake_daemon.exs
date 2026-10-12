defmodule Mcpd.FakeDaemon do
  @moduledoc false
  # A daemon on a loopback port the operating system chooses, built on
  # `:gen_tcp` so the test suite keeps the SDK's promise of no dependencies.
  #
  # Every test in `mcpd_test.exs` points at port 1 and can therefore only prove
  # that nothing raised. Nothing in this SDK's suite had ever read the bytes it
  # sends, so a field that never reached the wire had never been able to fail a
  # test. That is what this exists for.

  @doc """
  Start a daemon. `reply` is given the request number, one-based, and the
  request head when it takes two arguments, and answers with `{status, body}`,
  `{status, body, headers}`, `:never`, which accepts the connection and then
  says nothing at all, or `:close`, which drops the connection unanswered.
  """
  def start(reply) do
    {:ok, listen} =
      :gen_tcp.listen(0, [:binary, packet: :raw, active: false, reuseaddr: true, ip: {127, 0, 0, 1}])

    {:ok, port} = :inet.port(listen)
    {:ok, collector} = Agent.start(fn -> [] end)

    # The listen socket has to be owned by the process that accepts on it:
    # `:gen_tcp.accept/1` answers `{:error, :not_owner}` to anyone else. So the
    # acceptor waits to be told the transfer is done before it starts.
    acceptor =
      spawn(fn ->
        receive do
          :owned -> accept(listen, collector, reply)
        end
      end)

    :ok = :gen_tcp.controlling_process(listen, acceptor)
    send(acceptor, :owned)

    %{
      endpoint: "http://127.0.0.1:#{port}",
      listen: listen,
      collector: collector,
      acceptor: acceptor
    }
  end

  @doc "Release the port and stop collecting."
  def stop(daemon) do
    # Killing the acceptor closes both sockets with it: an inet port dies with
    # the process that owns it. Closing them from here would be closing a socket
    # this process does not own.
    Process.exit(daemon.acceptor, :kill)
    if Process.alive?(daemon.collector), do: Agent.stop(daemon.collector)
    :ok
  end

  @doc "Every request received so far, oldest first, each with the moment it arrived."
  def requests(daemon), do: Agent.get(daemon.collector, & &1)

  @doc "Every batch received, decoded."
  def batches(daemon), do: daemon |> requests() |> Enum.map(&decode(&1.body))

  @doc "Every signal of one kind across every batch received."
  def signals(daemon, kind), do: daemon |> batches() |> Enum.flat_map(&Map.get(&1, kind, []))

  @doc "One header of a received request, or nil."
  def header(request, name) do
    request.head
    |> String.split("\r\n")
    |> Enum.find_value(fn line ->
      case :binary.split(line, ":") do
        [key, value] -> if String.downcase(key) == name, do: String.trim(value)
        _ -> nil
      end
    end)
  end

  @doc "The request line: method and target."
  def request_line(request), do: request.head |> String.split("\r\n") |> hd()

  defp accept(listen, collector, reply) do
    case :gen_tcp.accept(listen) do
      {:ok, socket} ->
        serve(socket, collector, reply)
        accept(listen, collector, reply)

      {:error, _closed} ->
        :ok
    end
  end

  defp serve(socket, collector, reply) do
    case read_request(socket) do
      {:ok, head, body} ->
        body = if header(%{head: head}, "content-encoding") == "gzip", do: :zlib.gunzip(body), else: body

        n =
          Agent.get_and_update(collector, fn seen ->
            {length(seen) + 1, seen ++ [%{head: head, body: body, at: System.monotonic_time(:millisecond)}]}
          end)

        answer = if is_function(reply, 2), do: reply.(n, head), else: reply.(n)

        case answer do
          # A load balancer that has lost its backend accepts the connection and
          # holds it. The only way out is the client's own timeout. The socket
          # is handed to a holder so the acceptor stays free to see the retries,
          # and the holder is linked so stopping the daemon drops every held
          # connection with it.
          :never ->
            holder = spawn_link(fn -> receive do: (:held -> Process.sleep(30_000)) end)
            :ok = :gen_tcp.controlling_process(socket, holder)
            send(holder, :held)

          # A backend that dies mid-request: the connection closes with no bytes.
          :close ->
            :gen_tcp.close(socket)

          {status, payload} ->
            :gen_tcp.send(socket, response(status, payload, []))
            :gen_tcp.close(socket)

          {status, payload, headers} ->
            :gen_tcp.send(socket, response(status, payload, headers))
            :gen_tcp.close(socket)
        end

      {:error, _reason} ->
        :gen_tcp.close(socket)
    end
  end

  defp read_request(socket, buffer \\ "") do
    case :binary.split(buffer, "\r\n\r\n") do
      [head, rest] ->
        case read_body(socket, rest, content_length(head)) do
          {:ok, body} -> {:ok, head, body}
          {:error, reason} -> {:error, reason}
        end

      [_incomplete] ->
        case :gen_tcp.recv(socket, 0, 5_000) do
          {:ok, data} -> read_request(socket, buffer <> data)
          {:error, reason} -> {:error, reason}
        end
    end
  end

  defp read_body(_socket, have, want) when byte_size(have) >= want, do: {:ok, have}

  defp read_body(socket, have, want) do
    case :gen_tcp.recv(socket, 0, 5_000) do
      {:ok, data} -> read_body(socket, have <> data, want)
      {:error, reason} -> {:error, reason}
    end
  end

  defp content_length(head) do
    case header(%{head: head}, "content-length") do
      nil -> 0
      value -> String.to_integer(value)
    end
  end

  defp response(status, payload, headers) do
    extra = Enum.map_join(headers, "", fn {k, v} -> "#{k}: #{v}\r\n" end)

    "HTTP/1.1 #{status} #{reason(status)}\r\n" <>
      "content-type: application/json\r\n" <>
      "content-length: #{byte_size(payload)}\r\n" <>
      extra <>
      "connection: close\r\n\r\n" <> payload
  end

  defp reason(200), do: "OK"
  defp reason(400), do: "Bad Request"
  defp reason(404), do: "Not Found"
  defp reason(429), do: "Too Many Requests"
  defp reason(500), do: "Internal Server Error"
  defp reason(502), do: "Bad Gateway"
  defp reason(503), do: "Service Unavailable"
  defp reason(504), do: "Gateway Timeout"
  defp reason(_), do: "Status"

  defp decode(body) do
    :json.decode(body)
  rescue
    _ -> %{}
  end
end
