defmodule Mcpd.ExperimentsTest do
  @moduledoc """
  Contract section 5.2, against a daemon that answers: each read and write on
  its path with its body, and the runner end to end, with spans read back from
  a test client while the reads and writes go to the fake daemon.
  """
  use ExUnit.Case, async: false

  alias Mcpd.FakeDaemon

  @experiment ~s({"experiment_id":"exp-1","name":"answers-v2","dataset_id":"ds-1","status":"running",) <>
                ~s("item_count":0,"correlation":{"project":"shop"},"started_at":"2026-10-01T12:00:00Z",) <>
                ~s("metadata":{"prompt":"answer@4","note":null}})

  @scored ~s({"traces_scored":3,"scores_written":3,"skipped":[],"scores":[]})

  @results ~s({"experiment_id":"exp-1","runs":3,"failed":1,) <>
             ~s("items":[{"trace_id":"0af7651916cd43dd8448eb211c80319c","status":"ok",) <>
             ~s("item_id":"c1","scores":{"exact_match":1.0,"llm_judge":0.5},"expected":{"maxTokens":5}}],) <>
             ~s("scorers":[{"scorer_id":"exact_match","n":3,"mean":0.5,"min":0.0,"max":1.0,) <>
             ~s("direction":"higher_is_better"}]})

  defp items_json(n) do
    for(
      i <- 1..n,
      do: %{"item_id" => "c#{i}", "dataset_id" => "ds-1", "input" => %{"question" => "q#{i}"}}
    )
    |> :json.encode()
    |> IO.iodata_to_binary()
  end

  # Routes by method and path, so the order the runner makes requests in is
  # asserted on what was received rather than baked into the answers.
  defp route(items, overrides \\ %{}) do
    fn _n, head ->
      [method, target | _] = head |> String.split("\r\n") |> hd() |> String.split(" ")
      path = URI.parse(target).path

      case Map.get(overrides, {method, path}) do
        nil -> default(method, path, items)
        answer -> answer
      end
    end
  end

  defp default("POST", "/v1/api/experiments", _items), do: {200, @experiment}
  defp default("GET", "/v1/api/datasets/ds-1/items", items), do: {200, items}
  defp default("POST", "/v1/api/experiments/exp-1/score", _items), do: {200, @scored}
  defp default("GET", "/v1/api/experiments/exp-1/results", _items), do: {200, @results}
  defp default(_method, _path, _items), do: {404, ~s({"error":"not found"})}

  # ------------------------------------------------------ reads and writes ---

  test "get_trajectory reads the run's trajectory with the client's project" do
    body =
      ~s({"trace_id":"t1","steps":[{"index":0,"span_id":"s1","kind":"tool_call","name":"lookup",) <>
        ~s("status":"ok","start_offset_ms":0.0}],"tool_sequence":["lookup"],"step_count":1,) <>
        ~s("tool_count":1,"error_count":0,"goal_reached":true})

    daemon = start_daemon(fn _n, _head -> {200, body} end)
    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")

    assert {:ok, %Mcpd.Trajectory{step_count: 1, tool_sequence: ["lookup"], steps: [step]}} =
             Mcpd.get_trajectory(c, "t/1")

    assert %Mcpd.TrajectoryStep{kind: :tool_call, name: "lookup"} = step
    [request] = FakeDaemon.requests(daemon)
    assert FakeDaemon.request_line(request) =~ ~r{^GET /v1/api/traces/t%2F1/trajectory\?}
    assert query(request.head) == %{"project" => "shop"}
  end

  test "list_dataset_items sends limit and nothing else, and keeps the case's JSON as it was" do
    items = ~s([{"item_id":"c1","dataset_id":"a/b","input":{"maxTokens":5,"note":null}}])
    daemon = start_daemon(fn _n, _head -> {200, items} end)
    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")

    assert {:ok, [%Mcpd.DatasetItem{item_id: "c1", input: %{"maxTokens" => 5, "note" => :null}}]} =
             Mcpd.list_dataset_items(c, "a/b", limit: 50)

    assert {:ok, _} = Mcpd.list_dataset_items(c, "a/b")
    [first, second] = FakeDaemon.requests(daemon)
    assert path(first) == "/v1/api/datasets/a%2Fb/items"

    assert query(first.head) == %{"limit" => "50"},
           "no project: the daemon finds it from the dataset"

    assert query(second.head) == %{}

    assert_raise ArgumentError, ~r/limit/, fn ->
      Mcpd.list_dataset_items(c, "a/b", project: "x")
    end
  end

  test "create_experiment fills the project, keeps metadata as given, and decodes the answer" do
    daemon = start_daemon(fn _n, _head -> {200, @experiment} end)
    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")

    assert {:ok, %Mcpd.Experiment{experiment_id: "exp-1", status: :running, metadata: metadata}} =
             Mcpd.create_experiment(c,
               experiment_id: "",
               name: "answers-v2",
               dataset_id: "ds-1",
               status: :running,
               item_count: 0,
               metadata: %{"promptVersion" => 4, "note" => nil}
             )

    assert metadata == %{"prompt" => "answer@4", "note" => :null}

    assert {:ok, _} =
             Mcpd.create_experiment(c, %Mcpd.Experiment{
               experiment_id: "exp-1",
               name: "n",
               dataset_id: "ds-1",
               status: :failed,
               item_count: 2,
               correlation: %Mcpd.Correlation{project: "other"},
               error: "boom"
             })

    [first, second] = FakeDaemon.requests(daemon)
    assert path(first) == "/v1/api/experiments"
    assert query(first.head) == %{}

    assert :json.decode(first.body) == %{
             "experiment_id" => "",
             "name" => "answers-v2",
             "dataset_id" => "ds-1",
             "status" => "running",
             "item_count" => 0,
             "correlation" => %{"project" => "shop"},
             "metadata" => %{"promptVersion" => 4, "note" => :null}
           }

    sent = :json.decode(second.body)
    assert sent["correlation"] == %{"project" => "other"}, "a project the caller named is kept"
    assert sent["status"] == "failed" and sent["error"] == "boom"
  end

  test "score_experiment posts the scorers, the config only when given, and the project" do
    daemon = start_daemon(fn _n, _head -> {200, @scored} end)
    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")
    schema = %{"type" => "object", "additionalProperties" => false}

    assert {:ok, %Mcpd.ScoreRunResult{traces_scored: 3}} =
             Mcpd.score_experiment(c, "exp/1", ["exact_match", "json_schema"],
               json_schema: schema
             )

    assert {:ok, _} = Mcpd.score_experiment(c, "exp/1")

    [first, second] = FakeDaemon.requests(daemon)
    assert FakeDaemon.request_line(first) =~ ~r{^POST /v1/api/experiments/exp%2F1/score\?}
    assert query(first.head) == %{"project" => "shop"}

    assert :json.decode(first.body) == %{
             "scorers" => ["exact_match", "json_schema"],
             "config" => %{"json_schema" => schema}
           }

    assert :json.decode(second.body) == %{"scorers" => []}
  end

  test "experiment_results reads with the project and keeps each case's scores keyed by scorer id" do
    daemon = start_daemon(fn _n, _head -> {200, @results} end)
    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")

    assert {:ok, %Mcpd.ExperimentResults{runs: 3, failed: 1, items: [item], scorers: [scorer]}} =
             Mcpd.experiment_results(c, "exp-1")

    assert item.scores == %{"exact_match" => 1.0, "llm_judge" => 0.5}
    assert item.expected == %{"maxTokens" => 5}

    assert %Mcpd.ScorerSummary{scorer_id: "exact_match", mean: 0.5, direction: :higher_is_better} =
             scorer

    [request] = FakeDaemon.requests(daemon)
    assert path(request) == "/v1/api/experiments/exp-1/results"
    assert query(request.head) == %{"project" => "shop"}
  end

  test "an error answer comes back as an error" do
    daemon = start_daemon(fn _n, _head -> {403, ~s({"error":"forbidden"})} end)
    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")

    assert {:error, {:http, 403, _, _}} = Mcpd.experiment_results(c, "exp-1")
    assert {:error, {:http, 403, _, _}} = Mcpd.create_experiment(c, name: "x", dataset_id: "d")
  end

  # ------------------------------------------------------------ the runner ---

  test "a run opens one root span per case, records a failing case and carries on, and succeeds" do
    daemon = start_daemon(route(items_json(3)))
    # A sample rate of zero proves the case spans are kept whatever the rate.
    {:ok, testing} = Mcpd.Testing.start(endpoint: daemon.endpoint, project: "shop", sample: 0.0)
    on_exit(fn -> Mcpd.Testing.stop(testing) end)
    client = testing.client

    target = fn %{"question" => question} ->
      if question == "q2", do: raise("no answer for q2")
      Mcpd.with_span(client, "lookup", [kind: :tool_call], fn -> "answer to " <> question end)
    end

    assert {:ok, %Mcpd.ExperimentResults{runs: 3}} =
             Mcpd.run_experiment(client, "ds-1", target,
               name: "answers-v2",
               scorers: ["exact_match"],
               concurrency: 1,
               metadata: %{"prompt" => "answer@4"}
             )

    cases = Mcpd.Testing.spans(testing, name: "answers-v2")
    assert length(cases) == 3, "one root span per case"
    by_case = Map.new(cases, &{&1["attributes"]["mcpd.dataset_item_id"], &1})
    assert Map.keys(by_case) |> Enum.sort() == ["c1", "c2", "c3"]

    for {id, span} <- by_case do
      assert span["kind"] == "agent_run"
      assert span["is_root"] == true
      assert span["parent_span_id"] == nil
      assert span["correlation"]["experiment_id"] == "exp-1"
      assert span["correlation"]["project"] == "shop"
      assert span["input"] == %{"question" => "q" <> String.trim_leading(id, "c")}
    end

    assert by_case["c1"]["status"] == "ok"
    assert by_case["c1"]["output"] == "answer to q1"
    assert by_case["c2"]["status"] == "error"
    assert by_case["c2"]["error"]["message"] == "no answer for q2"
    assert by_case["c3"]["status"] == "ok", "the case after the failing one still ran"
    assert by_case["c3"]["output"] == "answer to q3"

    lookups = Mcpd.Testing.spans(testing, name: "lookup")
    assert length(lookups) == 2
    parents = Enum.map(lookups, & &1["parent_span_id"]) |> Enum.sort()
    assert parents == Enum.sort([by_case["c1"]["span_id"], by_case["c3"]["span_id"]])

    assert Enum.all?(
             lookups,
             &(&1["trace_id"] in [by_case["c1"]["trace_id"], by_case["c3"]["trace_id"]])
           )

    lines = FakeDaemon.requests(daemon) |> Enum.map(&"#{method(&1)} #{path(&1)}")

    assert lines == [
             "POST /v1/api/experiments",
             "GET /v1/api/datasets/ds-1/items",
             "POST /v1/api/experiments/exp-1/score",
             "GET /v1/api/experiments/exp-1/results",
             "POST /v1/api/experiments"
           ]

    [create, items, score, _results, finish] = FakeDaemon.requests(daemon)
    created = :json.decode(create.body)
    assert created["experiment_id"] == ""
    assert created["status"] == "running"
    assert created["name"] == "answers-v2" and created["dataset_id"] == "ds-1"
    assert created["item_count"] == 0
    assert created["correlation"] == %{"project" => "shop"}
    assert created["metadata"] == %{"prompt" => "answer@4"}
    assert {:ok, _, 0} = DateTime.from_iso8601(created["started_at"])
    assert query(items.head) == %{"limit" => "5000"}

    assert :json.decode(score.body) == %{"scorers" => ["exact_match"]}
    assert query(score.head) == %{"project" => "shop"}

    finished = :json.decode(finish.body)
    assert finished["experiment_id"] == "exp-1"
    assert finished["status"] == "succeeded"
    assert finished["item_count"] == 3
    assert finished["summary"] == %{"exact_match" => 0.5}

    assert finished["metadata"] == %{"prompt" => "answer@4", "note" => :null},
           "the stored fields are kept"

    assert {:ok, _, 0} = DateTime.from_iso8601(finished["finished_at"])
  end

  test "no more than concurrency targets are in flight" do
    daemon = start_daemon(route(items_json(12)))
    {:ok, testing} = Mcpd.Testing.start(endpoint: daemon.endpoint, project: "shop")
    on_exit(fn -> Mcpd.Testing.stop(testing) end)

    gauge = :atomics.new(2, [])

    target = fn _input ->
      now = :atomics.add_get(gauge, 1, 1)
      peak(gauge, now)
      Process.sleep(40)
      :atomics.sub(gauge, 1, 1)
      :ok
    end

    assert {:ok, _} = Mcpd.run_experiment(testing.client, "ds-1", target, concurrency: 3)
    assert :atomics.get(gauge, 2) == 3
    assert length(Mcpd.Testing.spans(testing, kind: :agent_run)) == 12
  end

  test "a failed read posts the experiment failed with the error and returns it, and no target runs" do
    daemon =
      start_daemon(
        route(items_json(3), %{{"GET", "/v1/api/datasets/ds-1/items"} => {403, "forbidden"}})
      )

    {:ok, testing} = Mcpd.Testing.start(endpoint: daemon.endpoint, project: "shop")
    on_exit(fn -> Mcpd.Testing.stop(testing) end)
    calls = :counters.new(1, [])

    assert {:error, {:http, 403, "forbidden", _}} =
             Mcpd.run_experiment(testing.client, "ds-1", fn _ -> :counters.add(calls, 1, 1) end)

    assert :counters.get(calls, 1) == 0
    assert Mcpd.Testing.spans(testing) == []

    [_create, _items, failed] = FakeDaemon.requests(daemon)
    assert path(failed) == "/v1/api/experiments"
    body = :json.decode(failed.body)
    assert body["experiment_id"] == "exp-1"
    assert body["status"] == "failed"
    assert body["error"] =~ "403"
    assert body["error"] =~ "forbidden"
    assert is_binary(body["finished_at"])
  end

  test "a concurrency below 1 and a disabled client are refused before any request" do
    daemon = start_daemon(route(items_json(1)))
    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")

    assert_raise ArgumentError, ~r/concurrency must be a whole number of at least 1, got 0/, fn ->
      Mcpd.run_experiment(c, "ds-1", fn x -> x end, concurrency: 0)
    end

    {:ok, off} = client(endpoint: daemon.endpoint, project: "shop", enabled: false)

    assert_raise ArgumentError, ~r/enabled client/, fn ->
      Mcpd.run_experiment(off, "ds-1", fn x -> x end)
    end

    assert FakeDaemon.requests(daemon) == []
  end

  # --------------------------------------------------------- internals ---

  defp peak(gauge, now) do
    seen = :atomics.get(gauge, 2)

    if now > seen and :atomics.compare_exchange(gauge, 2, seen, now) != :ok do
      peak(gauge, now)
    end
  end

  defp target(head) do
    [_method, target | _] = head |> String.split("\r\n") |> hd() |> String.split(" ")
    URI.parse(target)
  end

  defp method(request), do: request |> FakeDaemon.request_line() |> String.split(" ") |> hd()
  defp path(request), do: target(request.head).path
  defp query(head), do: URI.decode_query(target(head).query || "")

  defp start_daemon(reply) do
    daemon = FakeDaemon.start(reply)
    on_exit(fn -> FakeDaemon.stop(daemon) end)
    daemon
  end

  defp client(opts) do
    {:ok, c} = Mcpd.start_link(Keyword.put_new(opts, :flush_interval, 60_000))
    on_exit(fn -> if Process.alive?(c), do: Mcpd.shutdown(c) end)
    {:ok, c}
  end
end
