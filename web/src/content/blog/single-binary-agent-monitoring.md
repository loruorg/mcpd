---
description: What is single binary agent monitoring? One executable with built-in storage and UI that traces every model and tool call. Learn setup, monitors and limits.
faq:
  - answer: Monitoring does not slow down or break your agent, because mcpd is not a gateway or a proxy and nothing travels through it to reach a model. The agent sends telemetry out of band, and the SDKs record spans without blocking or throwing. If mcpd is unreachable the agent does not notice, since only telemetry is lost.
    question: Does a single binary monitor slow down or break my agent?
  - answer: You can keep the OpenTelemetry instrumentation you already have unchanged, because mcpd reads both the OpenTelemetry gen_ai.* attributes and the OpenInference llm.* attributes. The only change you need to make is the exporter endpoint. The mcpd OpenTelemetry ingestion guide lists the supported protocols.
    question: Can I use the OpenTelemetry instrumentation I already have?
  - answer: You move storage to Postgres or ClickHouse, and you can also forward what mcpd stores as OTLP to any endpoint you name. That makes mcpd one hop in a pipeline you already run, so the data never has to stay in the SQLite file.
    question: What happens when one SQLite file is not enough?
  - answer: You should store them, because the prompt and the completion are the evidence when an answer is wrong, and search finds a run by what the model said. Set redaction to run before storage, since a secret deleted afterwards was still written down, and set retention in days.
    question: Should I store prompts and completions at all?
image: /blog/single-binary-agent-monitoring/featured-d4842c999bc0.webp
image_alt: "Illustration for Single binary agent monitoring: Find the bad run"
publishDate: "2026-11-03"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/single-binary-agent-monitoring#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"datePublished\":\"2026-11-03\",\"description\":\"What is single binary agent monitoring? One executable with built-in storage and UI that traces every model and tool call. Learn setup, monitors and limits.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Monitoring does not slow down or break your agent, because mcpd is not a gateway or a proxy and nothing travels through it to reach a model. The agent sends telemetry out of band, and the SDKs record spans without blocking or throwing. If mcpd is unreachable the agent does not notice, since only telemetry is lost.\"},\"name\":\"Does a single binary monitor slow down or break my agent?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You can keep the OpenTelemetry instrumentation you already have unchanged, because mcpd reads both the OpenTelemetry gen_ai.* attributes and the OpenInference llm.* attributes. The only change you need to make is the exporter endpoint. The mcpd OpenTelemetry ingestion guide lists the supported protocols.\"},\"name\":\"Can I use the OpenTelemetry instrumentation I already have?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You move storage to Postgres or ClickHouse, and you can also forward what mcpd stores as OTLP to any endpoint you name. That makes mcpd one hop in a pipeline you already run, so the data never has to stay in the SQLite file.\"},\"name\":\"What happens when one SQLite file is not enough?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You should store them, because the prompt and the completion are the evidence when an answer is wrong, and search finds a run by what the model said. Set redaction to run before storage, since a secret deleted afterwards was still written down, and set retention in days.\"},\"name\":\"Should I store prompts and completions at all?\"}],\"name\":\"Single binary agent monitoring: Find the bad run\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/cRHFKo3t72shx0YgzRAojqxL5CDQyAW3.webp\"},\"url\":\"https://mcpd.sh/blog/single-binary-agent-monitoring\"},{\"@id\":\"https://mcpd.sh/blog/single-binary-agent-monitoring#article\",\"@type\":\"BlogPosting\",\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory\",\"url\":\"https://mcpd.sh\"},{\"@type\":\"CreativeWork\",\"name\":\"Alerting · mcpd\",\"url\":\"https://mcpd.sh/docs/alerting\"},{\"@type\":\"CreativeWork\",\"name\":\"Cost · mcpd\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"Privacy · mcpd\",\"url\":\"https://mcpd.sh/docs/privacy\"},{\"@type\":\"CreativeWork\",\"name\":\"loruorg/mcpd\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"Operations · mcpd\",\"url\":\"https://mcpd.sh/docs/operations\"},{\"@type\":\"CreativeWork\",\"name\":\"Write-Ahead Logging - SQLite\",\"url\":\"https://www.sqlite.org/wal.html\"},{\"@type\":\"CreativeWork\",\"name\":\"Trace Context, W3C Recommendation\",\"url\":\"https://www.w3.org/TR/trace-context/\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"Security · mcpd\",\"url\":\"https://mcpd.sh/docs/security\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory\",\"url\":\"https://mcpd.sh/\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenInference semantic conventions\",\"url\":\"https://github.com/Arize-ai/openinference/blob/main/spec/semantic_conventions.md\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry Protocol Exporter\",\"url\":\"https://opentelemetry.io/docs/specs/otel/protocol/exporter/\"},{\"@type\":\"CreativeWork\",\"name\":\"Integrating · mcpd\",\"url\":\"https://mcpd.sh/docs/integrating\"}],\"dateModified\":\"2026-11-03\",\"datePublished\":\"2026-11-03\",\"description\":\"What is single binary agent monitoring? One executable with built-in storage and UI that traces every model and tool call. Learn setup, monitors and limits.\",\"headline\":\"Single binary agent monitoring: Find the bad run\",\"image\":\"https://api.birchseek.com/m/cRHFKo3t72shx0YgzRAojqxL5CDQyAW3.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/single-binary-agent-monitoring#webpage\"},\"keywords\":[\"single binary agent monitoring\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/single-binary-agent-monitoring#webpage\"},\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2498},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"defined sequence of activities of an organizational entity, enabled by the systematic organization of resources into processes that transform materials, provide services, or process information\",\"name\":\"Workflow\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Workflow\",\"https://www.wikidata.org/wiki/Q627335\",\"https://www.google.com/search?kgmid=/m/0k5rb\"]},{\"@type\":\"Thing\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]},{\"@type\":\"Thing\",\"description\":\"collection of data, publication type\",\"name\":\"Data set\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Data_set\",\"https://www.wikidata.org/wiki/Q1172284\",\"https://www.google.com/search?kgmid=/m/02cck\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"benchmark in natural language understanding\",\"name\":\"General Language Understanding Evaluation benchmark\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q64711428\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"large language model company\",\"name\":\"LlamaIndex\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LlamaIndex\",\"https://www.wikidata.org/wiki/Q125446413\"]},{\"@type\":\"Thing\",\"description\":\"Chinese artificial intelligence company\",\"name\":\"DeepSeek\",\"sameAs\":[\"https://en.wikipedia.org/wiki/DeepSeek\",\"https://www.wikidata.org/wiki/Q131577453\",\"https://www.google.com/search?kgmid=/g/11wvrb0s91\"]},{\"@type\":\"Thing\",\"description\":\"language understanding across multiple domains\",\"name\":\"MMLU\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MMLU\",\"https://www.wikidata.org/wiki/Q123721276\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"family of multimodal artificial intelligence models developed by OpenAI\",\"name\":\"GPT-4o\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GPT-4o\",\"https://www.wikidata.org/wiki/Q125919502\",\"https://www.google.com/search?kgmid=/g/11vxhg1zhp\"]},{\"@type\":\"Thing\",\"description\":\"family of large language models developed by OpenAI\",\"name\":\"GPT-5\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GPT-5\",\"https://www.wikidata.org/wiki/Q135297698\"]},{\"@type\":\"Thing\",\"description\":\"a lightweight markup language with plain-text formatting syntax that is popularly used\",\"name\":\"Markdown\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Markdown\",\"https://www.wikidata.org/wiki/Q1193600\",\"https://www.google.com/search?kgmid=/m/0jt0gkj\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - agent monitoring
  - opentelemetry
  - sqlite
  - llm observability
  - cost monitoring
title: "Single binary agent monitoring: Find the bad run"
---

**Single binary agent monitoring means one static executable that carries its own storage and web interface, so no external database is needed. For AI agents, [mcpd](https://mcpd.sh) records each model call, tool call and retrieval with prompt, completion, tokens, latency and cost, correlated into one run. Start by pointing an OpenTelemetry exporter at its endpoint; no package gets installed.**

An agent that works on your laptop loops, overspends or answers wrongly in production, and nothing tells you which of thousands of runs to open. Single binary agent monitoring suits the solo operator or small team who won't stand up a database cluster or write an afternoon of YAML before the first trace.

After reading, you can run one process, find the one run behind a complaint, read its prompt and answer, set monitors that page you once, and know where to go when one host is no longer enough.

> **Key Takeaways**
> - [Monitors run every 60 s](https://mcpd.sh/docs/alerting) and latch: they fire once and resolve only on a real reading.
> - [Cost is priced](https://mcpd.sh/docs/cost) once per step at ingest. An unknown model has no cost, not a cost of zero.
> - [Redaction](https://mcpd.sh/docs/privacy) (drop, mask, partial, hash) runs before the write, so a secret is never written down.
> - [OpenTelemetry in, OTLP out, Apache-2.0](https://github.com/loruorg/mcpd): you can always leave.

## What is single binary agent monitoring?

Single binary agent monitoring means one static executable that receives agent telemetry, stores it in an embedded database such as SQLite, and serves its own web interface, with no external database or collector to run. Each model call, tool call and retrieval becomes a span, so a failed run can be read end to end.

![Illustration for What is single binary agent monitoring?](/blog/single-binary-agent-monitoring/image-1-ce383a23dd9f.webp)

According to [its GitHub repository](https://github.com/loruorg/mcpd), mcpd is built this way, as one static Rust binary with the storage and the interface inside it.

With no database cluster and no separate frontend, single binary agent monitoring has fewer parts to break. You probe one process with `/healthz` and `/readyz`, and it shuts down gracefully on SIGTERM.

The data sits in one file, so there's one thing to back up. [SQLite's documentation on write-ahead logging](https://www.sqlite.org/wal.html) says readers and writers don't block each other, so you can browse runs in the interface while new spans are being written.

Follow the operations page for the exact backup procedure rather than copying a live file blindly.

To upgrade single binary agent monitoring, you replace one executable. No schema service has to stay in step with it.

mcpd is not a gateway or a proxy. Nothing travels through mcpd to reach a model, so if mcpd is slow or down, your agent is unaffected. Your agent calls the model directly and reports spans to the daemon on the side.

After `make build`, one command starts everything:

```shell
./target/release/mcpd demo
```

## What does an agent monitoring binary record that host metrics miss?

An agent monitoring binary records the payload of every step: what the model received, what it returned and which tool it called. A host-metrics agent sees CPU, memory and process uptime, while an agent fails in what the model said. Seven things go on the record:

1. Every model call is a span.
2. Every tool call is a span.
3. Every retrieval is a span in the same tree, so you can see which documents reached the prompt.
4. The prompt and the completion sit on the span. mcpd shows the exact prompt and answer, and [full-text search](https://mcpd.sh/) finds a run by what the model said.
5. Tokens, latency and cost sit on each step.
6. Spans nest into one run by trace id. Across services, the [W3C Trace Context Recommendation](https://www.w3.org/TR/trace-context/) carries that id in the `traceparent` header, for example `00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01`.
7. Logs, metrics and evaluations join the run. mcpd's [concepts page](https://mcpd.sh/docs/concepts) defines six signals correlated by run: spans, logs, metrics, scores, feedback and aspect notes.

[OpenInference](https://github.com/Arize-ai/openinference/blob/main/spec/semantic_conventions.md), Arize's specification, is a second convention. It uses `llm.*` names such as `llm.token_count.prompt`, and [mcpd's OpenTelemetry documentation](https://mcpd.sh/docs/opentelemetry) says mcpd reads it alongside `gen_ai.*`, so existing instrumentation works unchanged.

Storing payloads means storing any secret they contain. [mcpd's security documentation](https://mcpd.sh/docs/security) says redaction runs before storage on every signal, with modes to drop, mask, partially show or hash a value, because a secret deleted afterwards was still written down.

Retention is set in days, and the same page covers erasure requests about a person. With a default configuration nothing leaves the machine.

## How do you set up single binary agent monitoring?

You set up single binary agent monitoring by one of two routes: point an OpenTelemetry exporter at the hosted endpoint, which installs nothing, or build the mcpd binary from source and run it yourself.

### Hosted route

1. Sign up for the hosted version at mcpd.sh/app, create a project and copy a key from it.
2. Set the exporter variables in the environment of the process that runs your agent:

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

3. Send the project key with your requests. The [mcpd OpenTelemetry page](https://mcpd.sh/docs/opentelemetry) gives the header that carries it. A missing key is the usual reason a first trace doesn't show up.
4. Run the agent once. A run appears in the project, with each model call and tool call as a span.

The values are `grpc`, `http/protobuf` and `http/json`.

`OTEL_EXPORTER_OTLP_ENDPOINT` is the base URL the exporter sends to. OTLP is the OpenTelemetry wire protocol, carried over HTTP (protobuf or JSON) and gRPC.

If your agent already exports OpenTelemetry spans, those two variables are all single binary agent monitoring needs. That holds for LangChain, LangGraph, LlamaIndex, CrewAI, Vercel AI SDK, OpenAI Agents SDK, Pydantic AI and your own loop.

### Self-hosted route, built from source

1. Install Rust and Node. Both are required because the web interface is compiled into the binary.
2. Clone `github.com/loruorg/mcpd` and run `make build`.
3. Start the daemon in demo mode and open the interface.

```shell
git clone https://github.com/loruorg/mcpd
cd mcpd
make build
./target/release/mcpd demo
# open http://localhost:4317
```

4. Run `make dist` when you want the optimised build.

With no keys set, ingest and reads are open, which suits a laptop and nothing else. Don't make the daemon reachable from another machine until you've created keys.

To send your agent's spans to a self-hosted daemon, replace `https://api.mcpd.sh` with the address the daemon listens on. The same two variables and the same protocol values apply. The [quickstart](https://mcpd.sh/docs/quickstart) lists the remaining commands.

[Start free](https://mcpd.sh/app) on the hosted version if you want the first trace without building anything.

## How do you get from a bad answer or cost spike to the one run that explains it?

You get from a complaint to the one run that explains it by narrowing the Runs page with a sort or a search until one run is left, then reading its waterfall, its prompts and its cost.

When the complaint is "the agent never finishes", sort runs by duration or step count and open the longest. The waterfall puts every step on one clock, so a loop shows up as a long repeated sequence: the same tool, again and again.

Compare the arguments on each row. Identical arguments mean the model ignored the result, which the tool name alone would hide.

When the complaint is "the bill doubled", sort or group by cost per run, per model and per project to see which one moved. Open the most expensive run and read the tokens and cost on each step. One step with an enormous prompt looks different from a loop of many cheap steps.

When the complaint arrives as a screenshot of a wrong answer, paste a sentence of it into [mcpd's search](https://mcpd.sh/), which matches the payload, so it finds the run by what the model said. Open that step and read the exact prompt and completion.

Then read the retrieval step before it. If the retrieval returned the wrong context, the model answered faithfully from bad input, and the fix belongs in retrieval. If a [prompt version](https://mcpd.sh/docs/prompts) wrote the step, the step links to it.

Once you have the run, turn it into a test. On the testing page, add the failed run to a dataset and attach the deterministic scorers `no_tool_loop` and `tool_efficiency`. They never call a model, so the check is cheap and gives the same result every time.

Expect the old run to fail and the fixed version to pass, so single binary agent monitoring gives you a regression test as well as a trace.

## How do monitors catch tool-call loops and cost spikes without false alarms?

A monitor in mcpd catches a loop or a spike as a small state machine: [mcpd's alerting documentation](https://mcpd.sh/docs/alerting) says it "watches one measure, fires once when the measure crosses its limit, and stays fired until a real reading says the problem has ended."

mcpd evaluates each monitor every 60 s.

A refund agent that calls the same lookup tool again and again shows up as step-count divergence, because its runs take far more steps than usual. A runaway bill shows up in `cost`.

The latch keeps the alarm usable. Without it, a loop that lasts an hour would page you every minute, 60 times, and you'd mute the channel.

Missing data gets the same care. A crashed agent sends no traces, and a monitor that read the gap as a fix would resolve. Silence isn't recovery, so the monitor resolves only when a real reading falls back inside its limit.

Cost needs a price before a monitor can watch it. [mcpd's cost documentation](https://mcpd.sh/docs/cost) says mcpd prices each step once, at ingest, from a price book of per-million-token rates, with cached tokens, long-prompt tiers and service tiers handled.

You can supply your own book.

An unknown model has no cost, not a cost of zero. A step with no price stays visibly unpriced, so a missing entry can't turn a spike into a row of zeros that no monitor would flag. An unpriced model can still look cheap in the totals.

When a monitor fires, it calls your webhook, so the message lands in the chat or on-call tool you already use.

## Where does single binary agent monitoring stop, and how do you grow past it?

One binary stops at five limits (retention, storage, host, volume and destination), and each has a documented way past it.

| Limit | What you meet | Exit |
| --- | --- | --- |
| Retention | Set in days | Change the days, or forward a copy |
| Storage | One SQLite file, one writer | Postgres or ClickHouse |
| Host | One host to lose | Back up the file, run a relay |
| Volume | Every trace kept | Sampling modes |
| Destination | One interface | OTLP forwarding with presets |

Retention is a number of days that you set in configuration. Change it, or forward a copy elsewhere for a longer history.

In single binary agent monitoring, storage defaults to one SQLite file with one writer. For a team, Postgres or ClickHouse replaces the file while the binary stays the same, so several writers and a larger history stop being a problem.

The host is a risk because a single process on a single machine is one machine to lose. On a laptop, a scheduled copy of the file is the backup.

Beyond the laptop, put a relay beside your agents. It takes ingest from them and ships it on, and the [relay and regions guide](https://mcpd.sh/docs/relay) covers running one per region with links between regions.

In single binary agent monitoring, volume has its own control. Sampling runs in five modes: always, never, ratio, deterministic and tail. The mode you pick decides what is kept before it is stored.

Forwarding is the last exit. mcpd sends what it stores as OTLP, so it fits a collector or backend you already run. It accepts any endpoint, and presets exist for Langfuse, LangSmith, Braintrust, Arize, Phoenix, Datadog, Honeycomb, New Relic, Grafana and others.

### One artefact, two audiences

The laptop and the platform team run the same executable. On the platform team's own cluster it starts with `--accounts` and gains organisations and roles, OpenID Connect single sign-on, SCIM, quotas and an audit log.

The same configuration holds Postgres or ClickHouse as the store, redaction before storage, retention in days and a cost ceiling. There isn't a separate edition.

If you outgrow mcpd or want a copy elsewhere, your data leaves as OTLP, a format other tools read.

For single binary agent monitoring, the SQLite default suits one engineer or a small team that wants the first trace without standing up a database. Postgres or ClickHouse with `--accounts` suits a platform team that needs single sign-on, redaction, retention and a cost ceiling on its own cluster.

Forwarding suits anyone who already runs a collector or a backend and wants mcpd as one hop in that pipeline.

## Frequently asked questions

### Does a single binary monitor slow down or break my agent?

No: [mcpd's README](https://github.com/loruorg/mcpd) says mcpd is not a gateway or a proxy, so nothing travels through it to reach a model. The [mcpd SDKs](https://mcpd.sh/docs/integrating) record spans without blocking or throwing. If mcpd is unreachable the agent does not notice, because a slow or stopped daemon loses telemetry, not requests.

### Can I use the OpenTelemetry instrumentation I already have?

You can keep it unchanged. [mcpd's OpenTelemetry guide](https://mcpd.sh/docs/opentelemetry) says mcpd reads both the OpenTelemetry `gen_ai.*` attributes and the OpenInference `llm.*` attributes, so the only change is the exporter endpoint. The same guide lists the supported protocols.

### What happens when one SQLite file isn't enough?

You move storage to Postgres or ClickHouse, and you can forward what mcpd stores as OTLP to any endpoint you name. That makes mcpd one hop in a pipeline you already run, so the data never has to stay in the SQLite file.

### Should I store prompts and completions at all?

Store them, because the prompt and the completion are the evidence when an answer is wrong, and [mcpd's search](https://mcpd.sh/) finds a run by what the model said. [Set redaction](https://mcpd.sh/docs/privacy) to run before storage, since a secret deleted afterwards was still written down, and set retention in days.

## What should you do next?

First, set `OTEL_EXPORTER_OTLP_ENDPOINT` and `OTEL_EXPORTER_OTLP_PROTOCOL` in your agent's environment, run the agent once and confirm that one trace arrives.

Later today, open that run and read the exact prompt and answer on each step. Then add a cost monitor, so a spend spike fires once and stays fired until a real reading ends it.

This week:

- Set how many days of retention you need.
- Pick a redaction mode (drop, mask, partial or hash) for anything secret.
- If one SQLite file on one host is too little, move to Postgres or ClickHouse, or forward a copy as OTLP.

[Start free on the hosted version](https://mcpd.sh/app), send one trace and read it.

## Sources

[mcpd · The unopinionated agent observatory](https://mcpd.sh) · [Alerting · mcpd](https://mcpd.sh/docs/alerting) · [Cost · mcpd](https://mcpd.sh/docs/cost) · [Privacy · mcpd](https://mcpd.sh/docs/privacy) · [loruorg/mcpd](https://github.com/loruorg/mcpd) · [Operations · mcpd](https://mcpd.sh/docs/operations) · [OpenTelemetry · mcpd](https://mcpd.sh/docs/opentelemetry) · [OpenTelemetry Protocol Exporter](https://opentelemetry.io/docs/specs/otel/protocol/exporter/). Every other source is linked where the article cites it.
