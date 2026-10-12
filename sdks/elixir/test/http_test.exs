defmodule Mcpd.HttpTest do
  @moduledoc """
  The transport's connections. A worker's claim is a long poll that holds its
  request open, and a read made beside it must not wait for it to finish.
  """
  use ExUnit.Case, async: false

  # A daemon that keeps connections open between requests, as mcpd does, and
  # holds a claim for `hold` milliseconds before it answers. The shared fake
  # closes every connection after one answer, which hides the case this file
  # is about: a client that queues a request behind a busy connection.
  defp keep_alive_daemon(hold) do
    {:ok, listen} =
      :gen_tcp.listen(0, [:binary, packet: :raw, active: false, reuseaddr: true, ip: {127, 0, 0, 1}])

    {:ok, port} = :inet.port(listen)

    acceptor =
      spawn(fn ->
        receive do
          :owned -> accept(listen, hold)
        end
      end)

    :ok = :gen_tcp.controlling_process(listen, acceptor)
    send(acceptor, :owned)
    on_exit(fn -> Process.exit(acceptor, :kill) end)
    "http://127.0.0.1:#{port}"
  end

  defp accept(listen, hold) do
    case :gen_tcp.accept(listen) do
      {:ok, socket} ->
        server = spawn(fn -> receive(do: (:owned -> serve(socket, hold, ""))) end)
        :ok = :gen_tcp.controlling_process(socket, server)
        send(server, :owned)
        accept(listen, hold)

      {:error, _} ->
        :ok
    end
  end

  # One request after another on the same connection, until the client closes.
  defp serve(socket, hold, buffer) do
    case :binary.split(buffer, "\r\n\r\n") do
      [head, rest] ->
        {_body, buffer} = read_body(socket, rest, content_length(head))

        if String.contains?(head, "/v1/api/steps/claim"), do: Process.sleep(hold)

        payload = if String.contains?(head, "/healthz"), do: ~s({"status":"ok"}), else: ~s({"leases":[]})

        :gen_tcp.send(
          socket,
          "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: #{byte_size(payload)}\r\n\r\n" <>
            payload
        )

        serve(socket, hold, buffer)

      [_incomplete] ->
        case :gen_tcp.recv(socket, 0, 30_000) do
          {:ok, data} -> serve(socket, hold, buffer <> data)
          {:error, _} -> :gen_tcp.close(socket)
        end
    end
  end

  defp read_body(socket, have, want) when byte_size(have) < want do
    {:ok, data} = :gen_tcp.recv(socket, 0, 5_000)
    read_body(socket, have <> data, want)
  end

  defp read_body(_socket, have, want), do: :erlang.split_binary(have, want)

  defp content_length(head) do
    head
    |> String.split("\r\n")
    |> Enum.find_value(0, fn line ->
      case String.split(line, ":", parts: 2) do
        [key, value] -> if String.downcase(key) == "content-length", do: value |> String.trim() |> String.to_integer()
        _ -> nil
      end
    end)
  end

  test "a read is answered promptly while a long poll holds a connection to the same host" do
    url = keep_alive_daemon(3_000)

    # OTP's default `:httpc` profile keeps a connection alive once it has
    # answered, and queues the next request to the same host on it even while
    # it is busy. A read made while a worker's claim was held open on such a
    # connection waited for the claim, and timed out although the daemon would
    # have answered it at once. The first request here is what leaves a
    # connection alive to be reused, as a worker's registration does.
    assert {:ok, _} = Mcpd.Http.get_json(url <> "/healthz", timeout: 10_000)

    polls =
      for n <- 1..2 do
        Task.async(fn ->
          Mcpd.Http.post_json(url <> "/v1/api/steps/claim", %{"worker_id" => "w-#{n}"}, wait_ms: 3_000, timeout: 10_000)
        end)
      end

    Process.sleep(300)
    started = System.monotonic_time(:millisecond)
    assert {:ok, %{"status" => "ok"}} = Mcpd.Http.get_json(url <> "/healthz", timeout: 10_000)
    took = System.monotonic_time(:millisecond) - started
    assert took < 1_000, "the read waited #{took}ms behind the long polls"

    for poll <- polls, do: assert({:ok, %{"leases" => []}} = Task.await(poll, 10_000))
  end

  test "the SDK's requests go through a profile of its own, and OTP's default is left as it was" do
    :inets.start()
    keys = [:max_keep_alive_length, :max_pipeline_length, :max_sessions]
    {:ok, before} = :httpc.get_options(keys, :default)
    profile = Mcpd.Http.profile()
    assert {:ok, [max_keep_alive_length: 0, max_pipeline_length: 0, max_sessions: _]} = :httpc.get_options(keys, profile)
    assert {:ok, ^before} = :httpc.get_options(keys, :default)
  end
end
