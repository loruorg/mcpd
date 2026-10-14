# The experiment runner against a running mcpd, with every empty shape it
# sends. Skipped unless `MCPD_TEST_ENDPOINT` is set, like live_test.exs.

defmodule Mcpd.LiveExperimentTest do
  use ExUnit.Case, async: false

  @endpoint System.get_env("MCPD_TEST_ENDPOINT")
  @project "sdk-live-elixir-experiment"

  if @endpoint in [nil, ""], do: @moduletag(:skip)

  test "an experiment runs over nulls and empty lists, and records a null as null" do
    {:ok, client} = Mcpd.start_link(endpoint: @endpoint, project: @project, flush_interval: 50)
    on_exit(fn -> Mcpd.shutdown(client) end)

    dataset_id = seed_dataset([
      %{"input" => %{"q" => "hi", "n" => :null, "l" => []}, "expected" => "hi"},
      %{"input" => :null, "expected" => "x"}
    ])

    me = self()

    {:ok, results} =
      Mcpd.run_experiment(
        client,
        dataset_id,
        fn input ->
          send(me, {:input, input})
          "hi"
        end,
        config: [needle: "hi"]
      )

    assert {results.runs, results.failed, length(results.items)} == {2, 0, 2}

    recorded =
      Map.new(results.items, fn item ->
        {:ok, detail} = Mcpd.get_trace(client, item.trace_id)
        {item.item_id, Enum.find(detail.spans, & &1.is_root).input}
      end)

    assert recorded[dataset_id <> "-0"] in [
             %{"q" => "hi", "n" => nil, "l" => []},
             %{"q" => "hi", "n" => :null, "l" => []}
           ]

    assert recorded[dataset_id <> "-1"] in [nil, :null]

    {:ok, experiment} =
      Mcpd.create_experiment(client,
        name: "ex-live-empty",
        dataset_id: dataset_id,
        status: :running,
        item_count: 0
      )

    assert {:ok, _} = Mcpd.score_experiment(client, experiment.experiment_id)
    assert {:ok, _} = Mcpd.score_experiment(client, experiment.experiment_id, [], %{})
  end

  # A dataset holding `cases`, written over plain HTTP so only the runner is
  # under test.
  defp seed_dataset(cases) do
    id = "ex-live-#{System.system_time(:nanosecond)}"
    base = String.trim_trailing(@endpoint, "/") <> "/v1/api/datasets"

    {:ok, _} =
      Mcpd.Http.post_json(base, %{
        "dataset_id" => id,
        "name" => id,
        "item_count" => 0,
        "created_at" => "2026-01-01T00:00:00Z",
        "correlation" => %{"project" => @project}
      })

    items =
      cases
      |> Enum.with_index()
      |> Enum.map(fn {c, i} -> Map.merge(c, %{"item_id" => "#{id}-#{i}", "dataset_id" => id}) end)

    {:ok, _} = Mcpd.Http.post_json(base <> "/" <> id <> "/items", items)
    id
  end
end
