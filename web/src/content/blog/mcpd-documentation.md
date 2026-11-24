---
description: Where is the mcpd documentation? It lives at mcpd.sh/docs, organised by task. Get a first trace through hosted, OpenTelemetry or source routes. Start free.
faq:
  - answer: "The mcpd documentation lives at mcpd.sh/docs and is organised by task: concepts, integrating, OpenTelemetry, evaluation, cost, alerting, privacy, security, workflows and the API and data-model references. Start with the quickstart at mcpd.sh/docs/quickstart to see a first trace, then follow the page for the job in front of you."
    question: Where is the mcpd documentation, and where should I start?
  - answer: "mcpd is an observability daemon, and nothing routes through it, so it is neither a gateway nor a proxy. Your agent sends data to mcpd, so a slow or stopped daemon can't slow or break the agent. The SDKs never block or throw."
    question: Is mcpd a gateway or a proxy for MCP servers, and what happens to my agent if it goes down?
  - answer: "Existing OpenTelemetry instrumentation works as it is, so you don't need to change it. mcpd reads both the gen_ai.* attributes and the OpenInference llm.* attributes, and accepts OTLP over HTTP (JSON and protobuf) or gRPC, so you point your exporter at mcpd and runs appear."
    question: Do I have to change my existing OpenTelemetry instrumentation?
  - answer: mcpd is free to use, and you can leave at any time. The hosted and self-hosted versions are the same product, whichever way you run it. You can leave because what mcpd stores it also forwards as OTLP to any endpoint you name.
    question: Is mcpd free, and can I leave later?
image: /blog/mcpd-documentation/featured-e8572aa12728.webp
image_alt: "Illustration for Mcpd documentation: First trace in five minutes"
publishDate: "2026-11-24"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/mcpd-documentation#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q133436854\",\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q8777\",\"@type\":\"Thing\",\"description\":\"application-layer protocol for transferring web resources\",\"name\":\"HTTP\",\"sameAs\":[\"https://en.wikipedia.org/wiki/HTTP\",\"https://www.wikidata.org/wiki/Q8777\",\"https://www.google.com/search?kgmid=/m/03hgt\"]}],\"datePublished\":\"2026-11-24\",\"description\":\"Where is the mcpd documentation? It lives at mcpd.sh/docs, organised by task. Get a first trace through hosted, OpenTelemetry or source routes. Start free.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"The mcpd documentation lives at mcpd.sh/docs and is organised by task: concepts, integrating, OpenTelemetry, evaluation, cost, alerting, privacy, security, workflows and the API and data-model references. Start with the quickstart at mcpd.sh/docs/quickstart to see a first trace, then follow the page for the job in front of you.\"},\"name\":\"Where is the mcpd documentation, and where should I start?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"mcpd is an observability daemon, and nothing routes through it, so it is neither a gateway nor a proxy. Your agent sends data to mcpd, so a slow or stopped daemon can't slow or break the agent. The SDKs never block or throw.\"},\"name\":\"Is mcpd a gateway or a proxy for MCP servers, and what happens to my agent if it goes down?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Existing OpenTelemetry instrumentation works as it is, so you don't need to change it. mcpd reads both the gen_ai.* attributes and the OpenInference llm.* attributes, and accepts OTLP over HTTP (JSON and protobuf) or gRPC, so you point your exporter at mcpd and runs appear.\"},\"name\":\"Do I have to change my existing OpenTelemetry instrumentation?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"mcpd is free to use, and you can leave at any time. The hosted and self-hosted versions are the same product, whichever way you run it. You can leave because what mcpd stores it also forwards as OTLP to any endpoint you name.\"},\"name\":\"Is mcpd free, and can I leave later?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q7455583\",\"@type\":\"Thing\",\"description\":\"web push technology\",\"name\":\"Server-sent events\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Server-sent_events\",\"https://www.wikidata.org/wiki/Q7455583\",\"https://www.google.com/search?kgmid=/m/0bbzryw\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q11189\",\"@type\":\"Thing\",\"description\":\"computer server that makes and receives requests on behalf of a user\",\"name\":\"Proxy server\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Proxy_server\",\"https://www.wikidata.org/wiki/Q11189\",\"https://www.google.com/search?kgmid=/m/0kjbp\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q756100\",\"@type\":\"Thing\",\"description\":\"JavaScript runtime environment\",\"name\":\"Node.js\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Node.js\",\"https://www.wikidata.org/wiki/Q756100\",\"https://www.google.com/search?kgmid=/m/0bbxf89\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q1645574\",\"@type\":\"Thing\",\"description\":\"data serialization format\",\"name\":\"Protocol Buffers\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Protocol_Buffers\",\"https://www.wikidata.org/wiki/Q1645574\",\"https://www.google.com/search?kgmid=/m/04dzxdz\"]}],\"name\":\"Mcpd documentation: First trace in five minutes\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/OzGutVhV9a85HOI8rm_j1BHZGYINA-sG.webp\"},\"url\":\"https://mcpd.sh/blog/mcpd-documentation\"},{\"@id\":\"https://mcpd.sh/blog/mcpd-documentation#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q133436854\"},{\"@id\":\"https://www.wikidata.org/wiki/Q8777\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory.\",\"url\":\"https://mcpd.sh\"},{\"@type\":\"CreativeWork\",\"name\":\"Integrating · mcpd\",\"url\":\"https://mcpd.sh/docs/integrating\"},{\"@type\":\"CreativeWork\",\"name\":\"HTTP API · mcpd\",\"url\":\"https://mcpd.sh/docs/reference/api\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd README\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"GitHub - mozilla-ai/mcpd: Declaratively define and run required tools ...\",\"url\":\"https://github.com/mozilla-ai/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"GitHub - pouriya/mcpd: A ~3MB MCP daemon that exposes any script as a ...\",\"url\":\"https://github.com/pouriya/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"OTLP Exporter Configuration | OpenTelemetry\",\"url\":\"https://opentelemetry.io/docs/languages/sdk-configuration/otlp-exporter/\"},{\"@type\":\"CreativeWork\",\"name\":\"Quickstart · mcpd\",\"url\":\"https://mcpd.sh/docs/quickstart\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenInference Semantic Conventions\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"Cost · mcpd\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"Trace Context (W3C Recommendation)\",\"url\":\"https://www.w3.org/TR/trace-context/\"},{\"@type\":\"CreativeWork\",\"name\":\"Transports - Model Context Protocol\",\"url\":\"https://modelcontextprotocol.io/specification/2025-06-18/basic/transports\"},{\"@type\":\"CreativeWork\",\"name\":\"Relay and regions · mcpd\",\"url\":\"https://mcpd.sh/docs/relay\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd operations guide\",\"url\":\"https://mcpd.sh/docs/operations\"},{\"@type\":\"CreativeWork\",\"name\":\"Security · mcpd\",\"url\":\"https://mcpd.sh/docs/security\"},{\"@type\":\"CreativeWork\",\"name\":\"Runs · mcpd\",\"url\":\"https://mcpd.sh/docs/runs\"},{\"@type\":\"CreativeWork\",\"name\":\"Workflows · mcpd\",\"url\":\"https://mcpd.sh/docs/workflows\"},{\"@type\":\"CreativeWork\",\"name\":\"Concepts · mcpd\",\"url\":\"https://mcpd.sh/docs/concepts\"}],\"dateModified\":\"2026-11-24\",\"datePublished\":\"2026-11-24\",\"description\":\"Where is the mcpd documentation? It lives at mcpd.sh/docs, organised by task. Get a first trace through hosted, OpenTelemetry or source routes. Start free.\",\"headline\":\"Mcpd documentation: First trace in five minutes\",\"image\":\"https://api.birchseek.com/m/OzGutVhV9a85HOI8rm_j1BHZGYINA-sG.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/mcpd-documentation#webpage\"},\"keywords\":[\"mcpd documentation\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/mcpd-documentation#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q7455583\"},{\"@id\":\"https://www.wikidata.org/wiki/Q11189\"},{\"@id\":\"https://www.wikidata.org/wiki/Q756100\"},{\"@id\":\"https://www.wikidata.org/wiki/Q1645574\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2482},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"permissive free software license\",\"name\":\"MIT License\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MIT_License\",\"https://www.wikidata.org/wiki/Q334661\",\"https://www.google.com/search?kgmid=/m/04vpm\"]},{\"@type\":\"Thing\",\"description\":\"general-purpose programming language\",\"name\":\"Python\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Python_(programming_language)\",\"https://www.wikidata.org/wiki/Q28865\",\"https://www.google.com/search?kgmid=/m/05z1_\"]},{\"@type\":\"Thing\",\"description\":\"Python library\",\"name\":\"langsmith\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q132393290\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"software to manage containers on a server-cluster\",\"name\":\"Kubernetes\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Kubernetes\",\"https://www.wikidata.org/wiki/Q22661306\",\"https://www.google.com/search?kgmid=/g/11b7lxp79d\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence software company\",\"name\":\"Braintrust\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Braintrust\",\"https://www.wikidata.org/wiki/Q141164091\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"Python library\",\"name\":\"mlflow\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q107381745\"]},{\"@type\":\"Thing\",\"description\":\"creation or optimization of a prompt to be given to an artificial intelligence model\",\"name\":\"Prompt engineering\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Prompt_engineering\",\"https://www.wikidata.org/wiki/Q108941486\",\"https://www.google.com/search?kgmid=/g/11p6kpgt_n\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - mcpd
  - ai agents
  - observability
  - opentelemetry
  - tracing
  - quickstart
title: "Mcpd documentation: First trace in five minutes"
---

**The mcpd documentation lives at [mcpd.sh/docs](https://mcpd.sh/docs) and runs by task, from the [quickstart](https://mcpd.sh/docs/quickstart) through to the [HTTP API reference](https://mcpd.sh/docs/reference/api). Three routes to a first trace work today: the hosted version at mcpd.sh/app, an OpenTelemetry exporter pointed at https://api.mcpd.sh, or a build from github.com/loruorg/mcpd. After your first call, each step in [the mcpd interface](https://mcpd.sh) shows its prompt, its answer and its cost.**

The mcpd documentation is written for the engineer whose agent's fine on a laptop and misbehaves in production. A typical case is a refund agent that calls the same order-lookup tool again and again, followed by an invoice spike nobody can explain.

With the mcpd documentation, you can send a first trace and find the run behind a complaint. You can then read what the model received and returned, and pick the page for storage, access control or the API.

> **Key Takeaways**
> - Nothing routes through mcpd, so a stopped daemon costs you traces while your agent keeps answering.
> - The mcpd documentation is organised by task, so you open the page for the problem in front of you.

## What does the mcpd documentation cover, and which mcpd is it?

The mcpd documentation covers mcpd, a daemon that watches AI agents: [it records each model call](https://mcpd.sh), tool call and retrieval with its prompt, completion, latency and cost, and shows them in a built-in interface. mcpd isn't a framework, a gateway or a proxy.

![Illustration for What does the mcpd documentation cover, and which mcpd is it?](/blog/mcpd-documentation/image-1-dbe4fd018b2e.webp)

Several unrelated projects share the name mcpd, and they manage or expose MCP servers and tools. [One GitHub project](https://github.com/mozilla-ai/mcpd) describes itself as a daemon that manages MCP servers "via declarative configuration, exposing them as clean HTTP endpoints". [A second project](https://github.com/pouriya/mcpd) "turns any executable script into an MCP tool". This mcpd doesn't do either.

mcpd watches any framework, any language and any model provider. That is why the [integrating guide](https://mcpd.sh/docs/integrating) starts from your existing code and not from a new runtime.

Being a separate process shapes every page of the mcpd documentation. Your agent sends data to mcpd, never through it. Model requests go from your code straight to the provider, and a copy of what happened goes to the daemon on the side.

The [mcpd README](https://github.com/loruorg/mcpd) states the consequence: nothing travels through mcpd to reach a model, so mcpd being slow or down cannot slow your agent or break it. If the daemon stops while your agent is answering customers, the agent keeps calling its order-lookup tool and keeps replying. You lose the traces for those minutes, and the customers lose nothing.

If trace, span and signal are new terms, start with the [concepts page](https://mcpd.sh/docs/concepts), which defines the vocabulary the rest of the docs use.

## How do you get a first trace from the mcpd documentation quickstart?

You get a first trace from the mcpd documentation quickstart through one of three live routes (hosted, OpenTelemetry or source), and each one ends with a run and its spans open in the interface. The [full quickstart page](https://mcpd.sh/docs/quickstart) has every step in detail.

**Route 1, hosted.** [Sign up at mcpd.sh/app](https://mcpd.sh/app), create a project and copy its key. It's free, and every feature's included.

 On [the mcpd home page](https://mcpd.sh), mcpd describes itself as free, open-source observability for AI agents. You need the project key for Route 2 as well.

**Route 2, OpenTelemetry.** Set these two variables in the shell that starts your agent:

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

Then add your project key as an OTLP header, using the exact form on the [mcpd OpenTelemetry page](https://mcpd.sh/docs/opentelemetry). Set all of this before the agent starts, because exporters read their environment at startup. Then run the agent once as you normally do.

OpenTelemetry defines these variables, so they work with any instrumented agent. [OpenTelemetry's OTLP exporter page](https://opentelemetry.io/docs/languages/sdk-configuration/otlp-exporter/) describes `OTEL_EXPORTER_OTLP_ENDPOINT` as "A base endpoint URL for any signal type, with an optionally-specified port number."

The same page says `OTEL_EXPORTER_OTLP_PROTOCOL` "Specifies the OTLP transport protocol to be used for all telemetry data." Its permitted values are `grpc`, `http/protobuf` and `http/json`, and this route uses the last one. Any exporter that follows the specification reads both variables, so your agent code doesn't change.

**Route 3, self-hosted from source.** Clone [github.com/loruorg/mcpd](https://github.com/loruorg/mcpd) and follow these steps:

1. Install Rust and Node. Both are required because the interface is compiled into the binary, so a build without Node fails.
2. Run `make build`.
3. Run `./target/release/mcpd demo`.
4. Open `http://localhost:4317` in your browser.

The quickstart gives `./target/release/mcpd demo` as the command that starts the demo. `make dist` produces the optimised build when you want one.

To check the result, open the interface: your project on the hosted version, or localhost:4317. The run appears in the list. Click it and the spans open in a waterfall, each step with its prompt and completion.

If the list stays empty after the OpenTelemetry route, check that the key header is set. Then check that the agent was restarted after you exported the variables.

[Start free at mcpd.sh/app](https://mcpd.sh/app) and have the first trace on screen in a few minutes.

## What do you see in mcpd after the first model call?

After the first model call, mcpd shows the run as a tree of steps, and each step carries its prompt, its completion, its latency and its cost. [The mcpd home page](https://mcpd.sh) puts it this way: "A run is a tree of model calls, tool calls and retrievals."

Search starts from the complaint. If a customer says your refund agent promised a 90-day window and your policy says 30, paste "90 days" into search. Search runs full-text over the payload, so it finds the run by what the model said, not only by labels. The searching page in the docs covers the query syntax.

Open the run and read it three ways. The waterfall puts every step on one clock, so a slow retrieval or a repeated tool call is visible at a glance. The conversation view shows the full exchange, including each tool call and its result.

Logs, metrics and traces are joined by trace id, so a log line leads back to the step that wrote it.

If your instrumentation follows the [OpenInference specification](https://mcpd.sh/docs/opentelemetry) instead, mcpd reads its `llm.*` attributes too, such as `llm.token_count.prompt`.

Cost appears per step, per run, per model and per project. [mcpd prices each step](https://mcpd.sh/docs/cost) once, when it arrives, from a price book of per-million-token rates, and you can supply your own. An unknown model has no cost, not a cost of zero, so a missing price never looks like a free call.

## How do OpenTelemetry, forwarding and MCP connect mcpd to your stack?

mcpd connects to your stack through three open seams: OpenTelemetry data in, OTLP data out, and one MCP server that reads traces. Each seam uses a published protocol.

**Data in.** The [mcpd OpenTelemetry documentation](https://mcpd.sh/docs/opentelemetry) covers OTLP over HTTP (JSON and protobuf) and gRPC for traces, logs and metrics, plus Zipkin and Jaeger. mcpd reads both the `gen_ai.*` attributes and the OpenInference `llm.*` attributes, so existing instrumentation works unchanged.

To follow one request across services, you pass the `traceparent` header. The [W3C Trace Context Recommendation](https://www.w3.org/TR/trace-context/) says "The traceparent HTTP header field identifies the incoming request in a tracing system", and the header carries a version, a trace id, a parent id and flags. Any service that forwards it keeps its spans in the same trace.

**Data out.** The [mcpd README](https://github.com/loruorg/mcpd) says what mcpd stores it also forwards as OTLP to any endpoint you name, so you can leave at any time. Presets cover Langfuse, LangSmith, Braintrust, Arize, Phoenix, Datadog, Honeycomb, New Relic, Grafana and Sentry.

A [relay mode](https://mcpd.sh/docs/relay) places a collector beside your agents.

**MCP.** This mcpd hosts no tools and routes no calls. [The integrating page](https://mcpd.sh/docs/integrating) says `mcpd mcp` exposes three tools over stdio: `find_runs`, `get_run` and `run_stats`. A coding or support agent uses them to read traces.

The [MCP specification](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports) says "The protocol currently defines two standard transport mechanisms for client-server communication": stdio and Streamable HTTP, which replaced the older HTTP+SSE transport.

With stdio, per the specification, "The client launches the MCP server as a subprocess", so your agent's client starts `mcpd mcp` itself.

## How do you configure mcpd storage, sampling, retention and redaction?

You configure mcpd storage, sampling, retention and redaction through a short list of options, and each one prevents a specific failure. The [mcpd operations guide](https://mcpd.sh/docs/operations) covers the production side: probes, graceful shutdown, backups and upgrades.

Storage is one SQLite file by default; Postgres or ClickHouse suit teams. 

The `:memory:` setting keeps nothing, which suits a demo that must leave no payloads on disk.

Sampling has five modes: always, never, ratio, deterministic and tail. Recording every run of a busy agent drives up storage and cost, and dropping runs at random can lose the one that failed. Tail sampling cuts volume while keeping the runs that matter.

Retention is a number of days. Without a limit the store grows until the disk is full, and old prompts sit around longer than anyone intended. Retention deletes runs after that many days, and an erasure request about a person removes what mcpd holds on them.

Redaction modes differ in what survives. Drop removes the value, mask replaces it, and partial keeps only a fragment. A keyed hash lets you match the same value across runs without reading it.

`/healthz` and `/readyz` give an orchestrator two probes: one to learn the process is alive, another to learn it can take traffic. On SIGTERM mcpd shuts down gracefully, so a rolling restart doesn't cut a write in half.

Data goes elsewhere only when you turn on forwarding and name the endpoint.

## How does mcpd control who can read and write your traces?

mcpd controls who can read and write your traces in the order a request meets its checks: the key's scope first, then redaction before the write, then roles for the people who sign in. Each layer answers a different question an auditor asks.

**Keys decide what a caller may do.** [API keys carry six scopes](https://mcpd.sh/docs/security): ingest, read, write, execute, worker and admin. mcpd compares them as SHA-256 hashes.

Give each key only the scopes its caller needs. A key that sends traces from your agent can't read prompts if it leaks.

**Redaction decides what's ever stored.**

If a debug log could catch a customer's card number, don't wait: set a redaction rule before real user data arrives.

**The same scopes cover agents.** The `mcpd mcp` tools obey the same key scopes, so an agent reading traces gets no more access than its key grants.

**Accounts decide which person sees what.** Organisations have four roles: viewer, member, admin and owner. Single sign-on works with any OpenID Connect provider (Google, Microsoft Entra ID, Okta, Auth0), and GitHub works through an OAuth app.

SCIM, quotas and an audit log complete the set. They run on the hosted service (`--signup`) or on your own cluster (`--accounts`). 

## Where is each part of the mcpd documentation, by task?

The mcpd documentation gives each task its own page, so you start from the complaint and open the page that explains the run behind it.

1. A bill that spiked: open `cost` for the price book, then `alerting` for monitors on spend. The first finds the model and step that cost the most; the second warns you of the next spike before the invoice arrives.
2. A wrong answer in production: open `searching` to find the run by what the model said, then `evaluation` to attach scores and feedback to it. You get from a customer's screenshot to the exact prompt and completion.
3. A looping agent: when the same tool call repeats, the [built-in `no_tool_loop` scorer](https://mcpd.sh/docs) marks that run as a loop. The [runs guide](https://mcpd.sh/docs/runs) covers durable runs, timers, limits and tracing step failures, so start there to find the step where the repeat began.
4. Words you don't recognise: the [concepts page](https://mcpd.sh/docs/concepts) defines traces, spans, signals, correlation and the model hierarchy vocabulary used on every other page.
5. Sending your first data: the [integrating page](https://mcpd.sh/docs/integrating) compares the HTTP API, six SDKs and MCP server integration. Read it before you choose how your code reports.
6. Telemetry you already emit: the [OpenTelemetry page](https://mcpd.sh/docs/opentelemetry) details OTLP, Zipkin and Jaeger ingestion, with support for GenAI and OpenInference semantic conventions.
7. Who can see what: `privacy` and `security` describe what gets stored and who can read it. An auditor's questions land here.
8. Steps that must survive a crash: [workflows](https://mcpd.sh/docs/workflows) covers durable workflow definitions built from nine constructs, with templates, guards, retries and effects.
9. Exact endpoints and fields: the [HTTP API reference](https://mcpd.sh/docs/reference/api) covers ingest, traces, spans, logs, metrics, evaluation, analytics and operations endpoints. `reference/data-model` describes the objects those endpoints return.

## Frequently asked questions

### Where is the mcpd documentation, and where should I start?

Start with the quickstart at mcpd.sh/docs/quickstart to see a first trace, then open the page for the job in front of you. The docs at mcpd.sh/docs are organised by task: concepts, integrating, OpenTelemetry, evaluation, cost, alerting, privacy, security, workflows and the API and data-model references.

### Is mcpd a gateway or a proxy for MCP servers, and what happens to my agent if it goes down?

mcpd is an observability daemon, and nothing routes through it. Your agent sends data to mcpd, not through it, so a slow or stopped daemon can't slow or break the agent. The SDKs never block or throw.

### Do I have to change my existing OpenTelemetry instrumentation?

Existing OpenTelemetry instrumentation works as it is. mcpd reads both the gen_ai.* attributes and the OpenInference llm.* attributes, and accepts OTLP over HTTP (JSON and protobuf) or gRPC, so you point your exporter at mcpd and runs appear.

### Is mcpd free, and can I leave later?

mcpd is free, and the hosted and self-hosted versions are the same product. You can leave at any time, because what mcpd stores it also forwards as OTLP to any endpoint you name.

## What should you do next with mcpd?

Your next step with mcpd is a first trace in five minutes, then redaction, keys and a cost monitor during your first week.

**In the next five minutes:** set `OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh` and `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`, run your agent once, and open the first run as the [quickstart](https://mcpd.sh/docs/quickstart) shows. Read the prompt, the completion, the latency and the cost of each step.

**This week:**

- Set redaction and retention before real user data arrives.
- Create scoped keys, so each service holds only the scope it needs.
- Add a monitor on cost, so a spike reaches your webhook before the invoice does.

The [operations guide](https://mcpd.sh/docs/operations) covers the settings behind each of these.

**Later:** if you already run another backend, forward as OTLP. If the data must stay on your own storage, build from [github.com/loruorg/mcpd](https://github.com/loruorg/mcpd) and run it there.

[Start free](https://mcpd.sh/app), send one trace, and stop there if it doesn't fit.

## Sources

[mcpd · The unopinionated agent observatory.](https://mcpd.sh) · [Integrating · mcpd](https://mcpd.sh/docs/integrating) · [mcpd README](https://github.com/loruorg/mcpd) · [OTLP Exporter Configuration | OpenTelemetry](https://opentelemetry.io/docs/languages/sdk-configuration/otlp-exporter/) · [OpenInference Semantic Conventions](https://mcpd.sh/docs/opentelemetry) · [Cost · mcpd](https://mcpd.sh/docs/cost) · [Transports - Model Context Protocol](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports) · [HTTP API · mcpd](https://mcpd.sh/docs/reference/api). Every other source is linked where the article cites it.
