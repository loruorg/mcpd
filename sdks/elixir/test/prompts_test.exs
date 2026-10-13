defmodule Mcpd.PromptsTest do
  @moduledoc """
  Contract section 5.1, against a daemon that answers: which version is asked
  for, what the cache serves and when, the fallback, rendering by the daemon's
  own variable rule, and the attributes that link a span to its prompt. Also
  the query filters of section 5 that came with it.
  """
  use ExUnit.Case, async: false

  import ExUnit.CaptureIO

  alias Mcpd.{FakeDaemon, Prompts}

  @accepted ~s({"accepted":1,"rejected":0,"duplicate":0})

  defp prompt_json(opts \\ []) do
    version = Keyword.get(opts, :version, 3)
    name = Keyword.get(opts, :name, "support-reply")
    labels = Keyword.get(opts, :labels, ["production"])
    template = Keyword.get(opts, :template, "Answer {{question}} for {{ customer }}.")

    :json.encode(%{
      "project" => "shop",
      "name" => name,
      "version" => version,
      "kind" => "text",
      "template" => template,
      "variables" => ["question", "customer"],
      "labels" => labels,
      "created_at" => "2026-10-01T12:00:00Z"
    })
    |> IO.iodata_to_binary()
  end

  # ------------------------------------------------------ which version ---

  test "the default label falls back to the latest version on a 404" do
    daemon =
      start_daemon(fn _n, head ->
        if query(head)["label"] == "production",
          do: {404, ~s({"error":"not found"})},
          else: {200, prompt_json(labels: [])}
      end)

    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")

    assert {:ok, %Mcpd.Prompt{name: "support-reply", version: 3, kind: :text}} =
             Mcpd.get_prompt(c, "support-reply")

    [first, second] = FakeDaemon.requests(daemon)
    assert path(first) == "/v1/api/prompts/support-reply"
    assert query(first.head) == %{"project" => "shop", "label" => "production"}
    assert query(second.head) == %{"project" => "shop"}, "the retry asks for the latest version"

    Mcpd.shutdown(c)
  end

  test "a label or a version goes on the wire as given, and a name with / stays one segment" do
    daemon = start_daemon(fn _n, _head -> {200, prompt_json(name: "team/reply")} end)
    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")

    assert {:ok, _} = Mcpd.get_prompt(c, "team/reply", label: "staging")
    assert {:ok, _} = Mcpd.get_prompt(c, "team/reply", version: 2)

    [by_label, by_version] = FakeDaemon.requests(daemon)
    "GET " <> target = FakeDaemon.request_line(by_label)
    assert String.starts_with?(target, "/v1/api/prompts/team%2Freply?")
    assert query(by_label.head) == %{"project" => "shop", "label" => "staging"}
    assert query(by_version.head) == %{"project" => "shop", "version" => "2"}

    Mcpd.shutdown(c)
  end

  test "a label and a version together are refused before the wire" do
    daemon = start_daemon(fn _n, _head -> {200, prompt_json()} end)
    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")

    error = assert_raise ArgumentError, fn -> Mcpd.get_prompt(c, "support-reply", label: "production", version: 2) end
    assert error.message =~ "not both"
    assert FakeDaemon.requests(daemon) == []

    Mcpd.shutdown(c)
  end

  # ------------------------------------------------------------- cache ---

  test "a fresh copy is served with no request" do
    daemon = start_daemon(fn _n, _head -> {200, prompt_json()} end)
    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")

    assert {:ok, first} = Mcpd.get_prompt(c, "support-reply")
    assert {:ok, ^first} = Mcpd.get_prompt(c, "support-reply")
    assert length(FakeDaemon.requests(daemon)) == 1

    # A different selector is a different entry.
    assert {:ok, _} = Mcpd.get_prompt(c, "support-reply", version: 3)
    assert length(FakeDaemon.requests(daemon)) == 2

    Mcpd.shutdown(c)
  end

  test "a stale copy is served when the refetch fails, and the failure is reported" do
    daemon =
      start_daemon(fn
        1, _head -> {200, prompt_json()}
        2, _head -> {500, ~s({"error":"boom"})}
        _n, _head -> :close
      end)

    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")
    assert {:ok, fetched} = Mcpd.get_prompt(c, "support-reply", cache_ttl: 0)

    stderr =
      capture_io(:stderr, fn ->
        assert {:ok, ^fetched} = Mcpd.get_prompt(c, "support-reply", cache_ttl: 0)
        assert {:ok, ^fetched} = Mcpd.get_prompt(c, "support-reply", cache_ttl: 0)
        # A call to the worker, so its warning has been written before the
        # capture ends.
        Mcpd.stats(c)
      end)

    assert length(FakeDaemon.requests(daemon)) == 3, "cache_ttl: 0 fetches every time"
    assert stderr =~ "support-reply"
    assert stderr =~ "serving the copy fetched earlier"

    Mcpd.shutdown(c)
  end

  test "the cache holds 256 prompts and evicts the oldest" do
    daemon =
      start_daemon(fn _n, head ->
        {200, prompt_json(version: String.to_integer(query(head)["version"]))}
      end)

    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")

    for v <- 1..260, do: {:ok, _} = Mcpd.get_prompt(c, "support-reply", version: v)
    assert Prompts.cache_size(c) == Prompts.max_entries()
    assert Prompts.max_entries() == 256

    before = length(FakeDaemon.requests(daemon))
    {:ok, _} = Mcpd.get_prompt(c, "support-reply", version: 260)
    assert length(FakeDaemon.requests(daemon)) == before, "the newest is still cached"
    {:ok, _} = Mcpd.get_prompt(c, "support-reply", version: 1)
    assert length(FakeDaemon.requests(daemon)) == before + 1, "the oldest was evicted"

    Mcpd.shutdown(c)
  end

  test "a disabled client still fetches" do
    daemon = start_daemon(fn _n, _head -> {200, prompt_json()} end)
    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop", enabled: false)

    assert {:ok, %Mcpd.Prompt{version: 3}} = Mcpd.get_prompt(c, "support-reply", version: 3)
    assert length(FakeDaemon.requests(daemon)) == 1

    Mcpd.shutdown(c)
  end

  # ---------------------------------------------------------- fallback ---

  test "a fallback is returned when nothing was fetched, and is never cached" do
    daemon = start_daemon(fn _n, _head -> {500, ~s({"error":"down"})} end)
    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")

    assert {:ok, text} = Mcpd.get_prompt(c, "support-reply", version: 1, fallback: "Hi {{ name }}, {{x}} {{name}}")
    assert %Mcpd.Prompt{project: "shop", name: "support-reply", version: 0, kind: :text, labels: []} = text
    assert text.variables == ["name", "x"]

    messages = [%{"role" => "system", "content" => "You help {{customer}}."}, %{role: "user", content: "{{q}}"}]
    assert {:ok, chat} = Mcpd.get_prompt(c, "support-reply", version: 1, fallback: messages)
    assert %Mcpd.Prompt{version: 0, kind: :chat, messages: ^messages} = chat
    assert chat.variables == ["customer", "q"]

    assert length(FakeDaemon.requests(daemon)) == 2, "each call asked the daemon again"

    assert {:error, {:http, 500, _body, _}} = Mcpd.get_prompt(c, "support-reply", version: 1)

    Mcpd.shutdown(c)
  end

  test "an unreachable daemon with no copy and no fallback is an error, not a raise" do
    {:ok, c} = client(endpoint: "http://127.0.0.1:1", project: "shop", timeout: 500)
    assert {:error, _reason} = Mcpd.get_prompt(c, "support-reply")
    assert {:ok, %Mcpd.Prompt{version: 0}} = Mcpd.get_prompt(c, "support-reply", fallback: "x")
    Mcpd.shutdown(c)
  end

  # --------------------------------------------------------- rendering ---

  defp text(template), do: %Mcpd.Prompt{project: "p", name: "t", version: 1, kind: :text, template: template, variables: [], labels: [], created_at: nil}

  test "rendering allows whitespace in the braces" do
    assert {:ok, "Hi Ada, Ada and Ada."} = Prompts.render(text("Hi {{name}}, {{ name }} and {{\tname  }}."), %{"name" => "Ada"})
    assert {:ok, "Hi Ada"} = Prompts.render(text("Hi {{ name }}"), name: "Ada")
    assert {:ok, "v 1.5 true ok"} = Prompts.render(text("v {{a}} {{b}} {{c}}"), %{a: 1.5, b: true, c: :ok})
  end

  test "a placeholder that is not a name is left as written" do
    template = ~s(JSON {{"a": 1}} {{ 1x }} {{a b}} {{}} {{ok.v_2}} and {{open)
    assert {:ok, rendered} = Prompts.render(text(template), %{"ok.v_2" => "Y"})
    assert rendered == ~s(JSON {{"a": 1}} {{ 1x }} {{a b}} {{}} Y and {{open)
  end

  test "substitution is one pass" do
    assert {:ok, "a {{b}} c"} = Prompts.render(text("a {{x}} c"), %{"x" => "{{b}}", "b" => "B"})
  end

  test "a missing variable is named in the error, every one in order of first use" do
    prompt = text("{{b}} {{ a }} {{b}} {{c}}")
    assert {:error, %Mcpd.PromptRenderError{missing: ["b", "a"]} = error} = Prompts.render(prompt, %{"c" => 1, "extra" => 2})
    assert Exception.message(error) =~ "b, a"

    raised = assert_raise Mcpd.PromptRenderError, fn -> Prompts.render!(prompt, %{}) end
    assert raised.missing == ["b", "a", "c"]
    assert Exception.message(raised) =~ ~s(prompt "t")
  end

  test "a chat prompt renders every string content and keeps every other field" do
    prompt = %Mcpd.Prompt{
      project: "p",
      name: "chat",
      version: 2,
      kind: :chat,
      messages: [
        %{"role" => "system", "content" => "You help {{customer}}.", "name" => "{{customer}}"},
        %{"role" => "user", "content" => [%{"type" => "text", "text" => "{{customer}}"}]},
        %{role: "assistant", content: "Hello {{ customer }}"}
      ],
      variables: ["customer"],
      labels: [],
      created_at: nil
    }

    assert {:ok, [system, user, assistant]} = Mcpd.render_prompt(prompt, customer: "Ada")
    assert system == %{"role" => "system", "content" => "You help Ada.", "name" => "{{customer}}"}
    assert user == Enum.at(prompt.messages, 1), "a content that is not a string is copied unchanged"
    assert assistant == %{role: "assistant", content: "Hello Ada"}

    assert {:error, %Mcpd.PromptRenderError{missing: ["customer"]}} = Prompts.render(prompt, %{})
  end

  # ----------------------------------------------------------- linking ---

  test "attributes carry the name and version, and never raise on a bad prompt" do
    prompt = text("x") |> Map.put(:name, "support-reply") |> Map.put(:version, 4)
    assert Prompts.attributes(prompt) == %{"mcpd.prompt.name" => "support-reply", "mcpd.prompt.version" => 4}
    assert Prompts.attributes(%{prompt | version: 0}) == %{"mcpd.prompt.name" => "support-reply"}

    for bad <- [nil, %{}, "support-reply", 42, %{name: ""}, %{name: 7, version: 1}, {:ok, prompt}] do
      assert Prompts.attributes(bad) == %{}
    end
  end

  test "the attributes reach the wire on a span, merged with the span's others" do
    daemon = start_daemon(fn _n, _head -> {200, @accepted} end)
    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")
    prompt = text("x") |> Map.put(:name, "support-reply") |> Map.put(:version, 4)

    opts = [kind: :model_generation, attributes: Prompts.attributes(prompt), attributes: %{"other" => 1}]
    Mcpd.with_span(c, "answer", opts, fn -> :ok end)
    :ok = Mcpd.flush(c)

    span = daemon |> FakeDaemon.signals("spans") |> List.last()
    assert span["attributes"] == %{"mcpd.prompt.name" => "support-reply", "mcpd.prompt.version" => 4, "other" => 1}

    Mcpd.shutdown(c)
  end

  # ----------------------------------------------------------- writing ---

  test "create_prompt posts the request to the client's project" do
    daemon = start_daemon(fn _n, _head -> {200, prompt_json(version: 1)} end)
    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")

    assert {:ok, %Mcpd.Prompt{version: 1}} =
             Mcpd.create_prompt(c, name: "support-reply", template: "Answer {{question}}", labels: ["production"])

    assert {:ok, _} =
             Mcpd.create_prompt(c, %Mcpd.CreatePromptRequest{name: "chat", kind: :chat, messages: [%{"role" => "user", "content" => "hi"}]})

    [kw, struct] = FakeDaemon.requests(daemon)
    assert FakeDaemon.request_line(kw) =~ "POST /v1/api/prompts?"
    assert query(kw.head) == %{"project" => "shop"}
    assert :json.decode(kw.body) == %{"name" => "support-reply", "template" => "Answer {{question}}", "labels" => ["production"]}
    assert %{"name" => "chat", "kind" => "chat", "messages" => [%{"role" => "user", "content" => "hi"}]} = :json.decode(struct.body)

    Mcpd.shutdown(c)
  end

  test "set_prompt_label puts the version to the encoded label path" do
    daemon =
      start_daemon(fn
        1, _head -> {200, prompt_json(version: 2)}
        _n, _head -> {404, ~s({"error":"no such version"})}
      end)

    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")

    assert {:ok, %Mcpd.Prompt{version: 2}} = Mcpd.set_prompt_label(c, "team/reply", "production", 2)
    assert {:error, {:http, 404, _, _}} = Mcpd.set_prompt_label(c, "team/reply", "production", 99)

    [request | _] = FakeDaemon.requests(daemon)
    "PUT " <> target = FakeDaemon.request_line(request)
    assert String.starts_with?(target, "/v1/api/prompts/team%2Freply/labels/production?")
    assert query(request.head) == %{"project" => "shop"}
    assert :json.decode(request.body) == %{"version" => 2}

    Mcpd.shutdown(c)
  end

  # ----------------------------------------------- query filters, sec 5 ---

  test "list_traces sends the new filters, has_feedback as true or false, and nothing else" do
    daemon = start_daemon(fn _n, _head -> {200, ~s({"traces":[],"page":{"has_more":false}})} end)
    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")

    assert {:ok, %Mcpd.TraceListResponse{}} =
             Mcpd.list_traces(c,
               max_duration_ms: 2_000,
               max_cost: 0.25,
               min_tokens: 1_000,
               has_score: "relevance",
               score_above: 0.8,
               has_feedback: false,
               experiment_id: "exp-1",
               preview: false
             )

    assert {:ok, _} = Mcpd.list_traces(c, has_feedback: true, preview: true)

    [first, second] = FakeDaemon.requests(daemon)

    assert query(first.head) == %{
             "project" => "shop",
             "max_duration_ms" => "2000",
             "max_cost" => "0.25",
             "min_tokens" => "1000",
             "has_score" => "relevance",
             "score_above" => "0.8",
             "has_feedback" => "false",
             "experiment_id" => "exp-1",
             "preview" => "false"
           }

    assert query(second.head) == %{"project" => "shop", "has_feedback" => "true", "preview" => "true"}

    assert_raise ArgumentError, ~r/prompt/, fn -> Mcpd.list_traces(c, prompt: "support-reply") end
    assert length(FakeDaemon.requests(daemon)) == 2, "a refused filter makes no request"

    Mcpd.shutdown(c)
  end

  test "list_spans sends prompt and prompt_version, and nothing else" do
    daemon = start_daemon(fn _n, _head -> {200, ~s({"spans":[],"page":{"has_more":false}})} end)
    {:ok, c} = client(endpoint: daemon.endpoint, project: "shop")

    assert {:ok, %Mcpd.SpanListResponse{}} = Mcpd.list_spans(c, prompt: "team/reply", prompt_version: 3)
    [request] = FakeDaemon.requests(daemon)
    assert path(request) == "/v1/api/spans"
    assert query(request.head) == %{"project" => "shop", "prompt" => "team/reply", "prompt_version" => "3"}

    assert_raise ArgumentError, ~r/has_feedback/, fn -> Mcpd.list_spans(c, has_feedback: true) end
    assert length(FakeDaemon.requests(daemon)) == 1

    Mcpd.shutdown(c)
  end

  # --------------------------------------------------------- internals ---

  defp target(head) do
    [_method, target | _] = head |> String.split("\r\n") |> hd() |> String.split(" ")
    URI.parse(target)
  end

  defp path(request), do: target(request.head).path
  defp query(head), do: URI.decode_query(target(head).query || "")

  defp start_daemon(reply) do
    daemon = FakeDaemon.start(reply)
    on_exit(fn -> FakeDaemon.stop(daemon) end)
    daemon
  end

  defp client(opts), do: Mcpd.start_link(Keyword.put_new(opts, :flush_interval, 60_000))
end
