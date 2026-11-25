---
description: The mcpd github repo, loruorg/mcpd, holds free, open-source agent observability. Learn to build it from source, send an OpenTelemetry trace and find bad runs.
faq:
  - answer: The mcpd at github.com/loruorg/mcpd is free, open-source agent observability software. It runs as a daemon that records each model call, tool call and retrieval with its prompt, completion and cost. Other projects share the name, and this one is unrelated to them.
    question: Which mcpd is at github.com/loruorg/mcpd?
  - answer: "Clone github.com/loruorg/mcpd, run make build, then run ./target/release/mcpd demo and open http://localhost:4317. The build needs both Rust and Node. The interface is compiled into the binary, so there is no account and no database to set up. The hosted version at mcpd.sh/app needs no build."
    question: How do I run mcpd from GitHub?
  - answer: "You don't need to install an mcpd package to send traces to mcpd. Point a standard OpenTelemetry exporter at https://api.mcpd.sh or at your own daemon. The routes that work today are the source build, the hosted version and an OpenTelemetry exporter."
    question: Do I need to install an mcpd package to send traces?
  - answer: mcpd is free and licensed under Apache-2.0. Hosted and self-hosted are the same product, so no feature sits behind a plan. You can use the hosted version at mcpd.sh/app without building anything, or build the source from github.com/loruorg/mcpd and run it yourself.
    question: Is mcpd free, and which licence does it use?
  - answer: mcpd does not sit between your agent and the model, because it is not a gateway or a proxy. Nothing travels through it to reach a model, so a slow or stopped daemon cannot slow down or break your agent.
    question: Does mcpd sit between my agent and the model?
image: /blog/mcpd-github/featured-baa4a3430320.webp
image_alt: A developer in a hoodie sits at a desk with two monitors in a modern, moody loft at night.
publishDate: "2026-11-25"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/mcpd-github#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q133436854\",\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q286306\",\"@type\":\"Thing\",\"description\":\"command line tool and library for transferring data with URL syntax\",\"name\":\"CURL\",\"sameAs\":[\"https://en.wikipedia.org/wiki/CURL\",\"https://www.wikidata.org/wiki/Q286306\",\"https://www.google.com/search?kgmid=/m/02kxj5\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q364\",\"@type\":\"Organization\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]}],\"datePublished\":\"2026-11-25\",\"description\":\"The mcpd github repo, loruorg/mcpd, holds free, open-source agent observability. Learn to build it from source, send an OpenTelemetry trace and find bad runs.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"The mcpd at github.com/loruorg/mcpd is free, open-source agent observability software. It runs as a daemon that records each model call, tool call and retrieval with its prompt, completion and cost. Other projects share the name, and this one is unrelated to them.\"},\"name\":\"Which mcpd is at github.com/loruorg/mcpd?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Clone github.com/loruorg/mcpd, run make build, then run ./target/release/mcpd demo and open http://localhost:4317. The build needs both Rust and Node. The interface is compiled into the binary, so there is no account and no database to set up. The hosted version at mcpd.sh/app needs no build.\"},\"name\":\"How do I run mcpd from GitHub?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You don't need to install an mcpd package to send traces to mcpd. Point a standard OpenTelemetry exporter at https://api.mcpd.sh or at your own daemon. The routes that work today are the source build, the hosted version and an OpenTelemetry exporter.\"},\"name\":\"Do I need to install an mcpd package to send traces?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"mcpd is free and licensed under Apache-2.0. Hosted and self-hosted are the same product, so no feature sits behind a plan. You can use the hosted version at mcpd.sh/app without building anything, or build the source from github.com/loruorg/mcpd and run it yourself.\"},\"name\":\"Is mcpd free, and which licence does it use?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"mcpd does not sit between your agent and the model, because it is not a gateway or a proxy. Nothing travels through it to reach a model, so a slow or stopped daemon cannot slow down or break your agent.\"},\"name\":\"Does mcpd sit between my agent and the model?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q2063\",\"@type\":\"Thing\",\"description\":\"text-based open standard designed for human-readable data interchange\",\"name\":\"JSON\",\"sameAs\":[\"https://en.wikipedia.org/wiki/JSON\",\"https://www.wikidata.org/wiki/Q2063\",\"https://www.google.com/search?kgmid=/m/05cntt\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q165194\",\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q189248\",\"@type\":\"Thing\",\"description\":\"GNU Project implementation of the standard Unix shell\",\"name\":\"Bash\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Bash_(Unix_shell)\",\"https://www.wikidata.org/wiki/Q189248\",\"https://www.google.com/search?kgmid=/m/01g7l\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q28865\",\"@type\":\"Thing\",\"description\":\"general-purpose programming language\",\"name\":\"Python\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Python_(programming_language)\",\"https://www.wikidata.org/wiki/Q28865\",\"https://www.google.com/search?kgmid=/m/05z1_\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q965769\",\"@type\":\"Thing\",\"description\":\"software development practice based on frequent submission of granular changes\",\"name\":\"Continuous integration\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Continuous_integration\",\"https://www.wikidata.org/wiki/Q965769\",\"https://www.google.com/search?kgmid=/m/05vvqy\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q756100\",\"@type\":\"Thing\",\"description\":\"JavaScript runtime environment\",\"name\":\"Node.js\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Node.js\",\"https://www.wikidata.org/wiki/Q756100\",\"https://www.google.com/search?kgmid=/m/0bbxf89\"]}],\"name\":\"Mcpd github: Build, run and trace your first agent\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/zRV71ybCB2Y3sph0fI1Mbui4SB9YlFLD.webp\"},\"url\":\"https://mcpd.sh/blog/mcpd-github\"},{\"@id\":\"https://mcpd.sh/blog/mcpd-github#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q133436854\"},{\"@id\":\"https://www.wikidata.org/wiki/Q286306\"},{\"@id\":\"https://www.wikidata.org/wiki/Q364\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory.\",\"url\":\"https://mcpd.sh\"},{\"@type\":\"CreativeWork\",\"name\":\"GitHub - loruorg/mcpd\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"Security · mcpd\",\"url\":\"https://mcpd.sh/docs/security\"},{\"@type\":\"CreativeWork\",\"name\":\"Cost · mcpd\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"Integrating · mcpd\",\"url\":\"https://mcpd.sh/docs/integrating\"},{\"@type\":\"CreativeWork\",\"name\":\"Running it · mcpd\",\"url\":\"https://mcpd.sh/docs/operations\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory.\",\"url\":\"https://mcpd.sh/\"},{\"@type\":\"CreativeWork\",\"name\":\"Runs · mcpd\",\"url\":\"https://mcpd.sh/docs/runs\"},{\"@type\":\"CreativeWork\",\"name\":\"Alerting · mcpd\",\"url\":\"https://mcpd.sh/docs/alerting\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory.\",\"url\":\"https://mcpd.sh/docs/searching\"},{\"@type\":\"CreativeWork\",\"name\":\"Regression testing · mcpd\",\"url\":\"https://mcpd.sh/docs/testing\"},{\"@type\":\"CreativeWork\",\"name\":\"Apache License, Version 2.0\",\"url\":\"https://www.apache.org/licenses/LICENSE-2.0\"},{\"@type\":\"CreativeWork\",\"name\":\"Transports - Model Context Protocol\",\"url\":\"https://modelcontextprotocol.io/specification/2025-06-18/basic/transports\"}],\"dateModified\":\"2026-11-25\",\"datePublished\":\"2026-11-25\",\"description\":\"The mcpd github repo, loruorg/mcpd, holds free, open-source agent observability. Learn to build it from source, send an OpenTelemetry trace and find bad runs.\",\"headline\":\"Mcpd github: Build, run and trace your first agent\",\"image\":\"https://api.birchseek.com/m/zRV71ybCB2Y3sph0fI1Mbui4SB9YlFLD.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/mcpd-github#webpage\"},\"keywords\":[\"mcpd github\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/mcpd-github#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q2063\"},{\"@id\":\"https://www.wikidata.org/wiki/Q165194\"},{\"@id\":\"https://www.wikidata.org/wiki/Q189248\"},{\"@id\":\"https://www.wikidata.org/wiki/Q28865\"},{\"@id\":\"https://www.wikidata.org/wiki/Q965769\"},{\"@id\":\"https://www.wikidata.org/wiki/Q756100\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2365},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"permissive free software license\",\"name\":\"MIT License\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MIT_License\",\"https://www.wikidata.org/wiki/Q334661\",\"https://www.google.com/search?kgmid=/m/04vpm\"]},{\"@type\":\"Thing\",\"description\":\"general-purpose programming language\",\"name\":\"Python\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Python_(programming_language)\",\"https://www.wikidata.org/wiki/Q28865\",\"https://www.google.com/search?kgmid=/m/05z1_\"]},{\"@type\":\"Thing\",\"description\":\"Python library\",\"name\":\"langsmith\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q132393290\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"software to manage containers on a server-cluster\",\"name\":\"Kubernetes\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Kubernetes\",\"https://www.wikidata.org/wiki/Q22661306\",\"https://www.google.com/search?kgmid=/g/11b7lxp79d\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence software company\",\"name\":\"Braintrust\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Braintrust\",\"https://www.wikidata.org/wiki/Q141164091\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"Python library\",\"name\":\"mlflow\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q107381745\"]},{\"@type\":\"Thing\",\"description\":\"creation or optimization of a prompt to be given to an artificial intelligence model\",\"name\":\"Prompt engineering\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Prompt_engineering\",\"https://www.wikidata.org/wiki/Q108941486\",\"https://www.google.com/search?kgmid=/g/11p6kpgt_n\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - mcpd
  - agent observability
  - opentelemetry
  - open source
  - self-hosting
  - mcp
title: "Mcpd github: Build, run and trace your first agent"
---

**The mcpd github repository, [loruorg/mcpd](https://github.com/loruorg/mcpd), holds [free, open-source agent observability](https://mcpd.sh): a daemon that records each model call, tool call and retrieval with its prompt, completion and cost. It isn't a framework, gateway or proxy, and nothing travels through it to reach a model. Build it with `make build` (Rust and Node needed), run `./target/release/mcpd demo`, then open http://localhost:4317.**

Your agent worked on your laptop. In production it repeats one tool call until it gives up, or the bill jumps overnight. You need the one bad run out of thousands, with the prompt the model received and the completion it returned.

After reading, you can build mcpd from source, send a first trace over OpenTelemetry, find the run behind a loop or a cost spike, and lock down a self-hosted instance.

> **Key Takeaways**
> - Existing OpenTelemetry `gen_ai.*` or OpenInference `llm.*` instrumentation works unchanged, so no mcpd package is needed to send traces.
> - A blank cost on a step marks a model missing from the price book, not a free call.
> - Set API keys before the port reaches a network: with no keys, ingest and reads are open.
> - `mcpd mcp` lets a coding agent read stored runs through `find_runs`, `get_run` and `run_stats`.

## What is the mcpd GitHub project, and which one is it?

The mcpd GitHub project is the agent observability daemon at github.com/loruorg/mcpd, and it's unrelated to other projects that share its name. A search for mcpd GitHub can return repositories that do different jobs, including one that serves MCP tools over HTTP. The commands for loruorg/mcpd don't apply to them.

A daemon is a separate process that runs beside your agent and receives a record of what happened. The [mcpd README](https://github.com/loruorg/mcpd) states the consequence: "It is not a gateway and not a proxy. Nothing travels through mcpd to reach a model, so mcpd being slow or down cannot slow your agent or break it."

Your agent keeps calling its model provider directly. mcpd only receives the record afterwards, so removing it changes nothing about how the agent reaches a model.

The [concepts page](https://mcpd.sh/docs/concepts) defines four terms:

- A run is one execution of your agent, from request to final answer. The [mcpd home page](https://mcpd.sh) describes it as "a tree of model calls, tool calls and retrievals."
- A trace is the stored record of one run, with every span placed on one clock.
- A span is one timed unit of work inside a trace.
- A step is a span for a model call, tool call or retrieval, and it carries the prompt, the completion, the tokens and the cost.

mcpd works with "Any framework, any language, any model provider," per its home page. You keep the code you have and point an OpenTelemetry exporter at the daemon.

## How do you build the mcpd GitHub source?

You build the mcpd GitHub source with Rust, Node and one `make` target, and you get a single binary that runs the daemon and serves the web interface.

```shell
git clone https://github.com/loruorg/mcpd.git
cd mcpd
rustc --version && node --version
make build
./target/release/mcpd demo
```

1. Clone github.com/loruorg/mcpd, the agent observability project of that name.
2. Confirm Rust and Node are installed. The [loruorg/mcpd README](https://github.com/loruorg/mcpd) explains that both are required because the web interface is compiled into the binary. Rust builds the daemon, and Node builds the interface that Rust then embeds. If either version command prints nothing, install that toolchain first.
3. Run `make build`, which writes the binary to `./target/release/mcpd`.
4. Run `./target/release/mcpd demo`. It starts the daemon with sample data, so you have something to read before any of your own agents send a span.
5. Open http://localhost:4317. The interface comes from the same process, so there's no second server to start.

Once the page loads, open a run. The [mcpd home page](https://mcpd.sh/) lists what you see: a waterfall of every run on one clock, the full conversation with its tool calls, and the cost per run. Click a step to read the exact prompt and completion.

If you'd rather not build anything, the hosted version at [mcpd.sh/app](https://mcpd.sh/app) gives you the same interface.

For an instance that stays up longer than a demo, the daemon exposes `/healthz` and `/readyz` and shuts down gracefully on SIGTERM. The [guide to running mcpd in production](https://mcpd.sh/docs/operations) covers those probes, backups and upgrades.

## How do you send a first trace to mcpd over OpenTelemetry?

You send a first trace to mcpd over OpenTelemetry by setting two standard environment variables on an agent that already emits spans, with no mcpd package installed.

1. Set the endpoint and the protocol in the shell that starts your agent:

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

 Create a project at the [hosted version](https://mcpd.sh/app) to get a key, and pass it the way the [mcpd OpenTelemetry page](https://mcpd.sh/docs/opentelemetry) describes. On a self-hosted daemon, use the daemon's own OTLP address from the docs. `http://localhost:4317` is the interface address, so an exporter pointed there reaches no ingest.

2. Check that your instrumentation writes model-call spans with OpenTelemetry `gen_ai.*` names or OpenInference `llm.*` names. mcpd reads both, so existing instrumentation works unchanged.

3. Run the agent once with a prompt that makes it call the model and one tool. Ingest also accepts OTLP over gRPC, Zipkin, Jaeger and a W3C `traceparent` header, so a service that already uses one of those can join the same trace.

4. Open the run in the interface. Click the model-call step to read the exact prompt and the exact completion, plus the tokens, the latency and the cost. If nothing appears, check that the protocol variable matches what your exporter sends.

The [mcpd cost documentation](https://mcpd.sh/docs/cost) says mcpd prices each step once at ingest from a price book. An unknown model has no cost, not a cost of zero, so a blank cost on a step means the price book lacks that model.

The six native SDKs in the repository under `sdks/` are a convenience over a plain HTTP API. The OpenTelemetry route needs none of them.

To try this without building anything, [Start free](https://mcpd.sh/app) on the hosted version and point your exporter at `https://api.mcpd.sh`.

## How do you find the run behind a looping tool call or a cost spike?

You find the run behind a looping tool call or a cost spike by searching for what the model said, then reading the run's waterfall until one step stands out.

1. Paste a phrase from the user's message or the model's reply into search, as the [searching page](https://mcpd.sh/docs/searching) describes. Search is full-text over the payload, so it finds a run by what the model said, not only by labels. Don't filter by a label first: a label you never set matches nothing.
2. Open the waterfall. The [runs documentation](https://mcpd.sh/docs/runs) puts every step of the run on one clock. For a step that failed, the runs page covers tracing it.
3. Read the prompt and the completion at the step that went wrong. In a loop, compare the tool result inside each repeated prompt. If it never changes, read the completion that asks for the same call again.
4. Catch the next one automatically. Per the [alerting docs](https://mcpd.sh/docs/alerting), monitors are evaluated every 60 s on `cost`, `error_rate` and `latency_p95`. They latch, fire once, resolve only on a real reading, and call your webhook.

Suppose a user reports that a refund "keeps checking the same order". Searching that phrase opens the run, and the waterfall shows one `lookup_order` span repeated down the page. Each repeated prompt carries the same result, and each completion asks for the lookup again, so the fix belongs in that step.

You can then turn the failed run into a regression test, as the [regression testing guide](https://mcpd.sh/docs/testing) shows.

## What is in the mcpd GitHub repository, and why is it built this way?

The mcpd repository at github.com/loruorg/mcpd holds one daemon, six SDKs and the web interface, built so you can start with no setup and leave at any time.

The [loruorg/mcpd repository](https://github.com/loruorg/mcpd) describes mcpd as one static Rust binary with the storage and the web interface built in. `make build` compiles the interface into the binary, so there's one artefact to copy, run and restart, and no web server or database to deploy before the first trace.

SQLite is the default store, a single file on disk. A larger team runs the same binary with Postgres or ClickHouse for storage and `--accounts` for organisations and roles. Moving from a laptop to a team is a configuration change on the same artefact.

The `sdks/` directory holds TypeScript, Python, Rust, Go, Kotlin and Elixir. Recording a span never blocks, never throws and never grows without bound, so an agent doesn't notice if mcpd is unreachable. A conformance suite checks that all six, and plain curl, write structurally identical traces.

The [integrating guide](https://mcpd.sh/docs/integrating) compares the HTTP API, the SDKs and MCP, and the [Go SDK page](https://mcpd.sh/docs/sdks/go) shows one SDK in detail. None of the SDKs are published to a package registry, so you use them from the source tree.

mcpd takes OTLP in and forwards what it stores as OTLP to any endpoint you name. Presets exist for Langfuse, LangSmith, Braintrust, Arize, Phoenix, Datadog, Honeycomb, New Relic, Grafana and others, so those are places your traces can go next.

The README describes the result: "What it stores it also forwards, as OTLP, to any endpoint you name, which makes it one hop in a pipeline you already have rather than the place the data has to stay."

The repository is licensed under Apache-2.0. The [licence text at apache.org](https://www.apache.org/licenses/LICENSE-2.0) grants a "perpetual, worldwide, non-exclusive, no-charge, royalty-free, irrevocable copyright license to reproduce, prepare Derivative Works of, publicly display, publicly perform, sublicense, and distribute the Work".

## Is a self-hosted mcpd safe by default?

A self-hosted mcpd is safe by default only on a laptop, so you set API keys before its port reaches a network. Each default has a setting that closes the gap it leaves.

1. The [mcpd security documentation](https://mcpd.sh/docs/security) states that with no keys configured, anyone who can reach the port can write traces and read every prompt. That suits a laptop. Set keys before you expose the port.
2. API keys carry six scopes: ingest, read, write, execute, worker and admin. mcpd stores and compares them as SHA-256 hashes, so the database doesn't hold keys in a form you can reuse.
3. Give agents the ingest scope only. A key that can write spans but not read them can't expose other people's prompts if it leaks from a container or a CI log.
### What else should you set before you share it?

1. Retention is set in days, and erasure requests work on a person. Without them, prompts that contain personal data stay on disk for as long as the file does.
6. A default configuration sends nothing out. There is no update check, no usage reporting and no telemetry, so the only data leaving the machine is what you choose to forward.
3. A coding agent that calls `find_runs`, `get_run` or `run_stats` sees only what its key allows, so the MCP route is no way around your access rules.

## Can a coding agent read mcpd traces over MCP?

Yes: `mcpd mcp` is a Model Context Protocol server that a coding or support agent launches to read the traces mcpd has already stored, as the [mcpd integration guide](https://mcpd.sh/docs/integrating) describes.

The [Model Context Protocol specification](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports) defines the stdio transport in its transports section: "The client launches the MCP server as a subprocess." The server then reads JSON-RPC messages from its standard input and writes its replies to standard output. Your agent client starts `mcpd mcp` itself, so there's no port to open.

The server offers three tools:

- `find_runs` returns the runs that match your query.
- `get_run` returns one run in full, including each step with its prompt and completion.
- `run_stats` returns aggregate figures across runs.

The key scopes that guard the HTTP API apply here too, and the same guide compares this route with the HTTP API and the SDKs.

A coding agent fixing a failing test can call `find_runs` to locate the failing run, then `get_run` to read the prompt and completion while it edits the code. `mcpd mcp` doesn't route model calls.

## Frequently asked questions

### Which mcpd is at github.com/loruorg/mcpd?

It's [free, open-source agent observability](https://mcpd.sh): a daemon that records each model call, tool call and retrieval with its prompt, completion and cost. Other projects share the name, and this one is unrelated to them.

### How do I run mcpd from GitHub?

Clone [github.com/loruorg/mcpd](https://github.com/loruorg/mcpd), run `make build` (Rust and Node are both needed), then `./target/release/mcpd demo` and open http://localhost:4317. The hosted version at mcpd.sh/app needs no build.

### Do I need to install an mcpd package to send traces?

You don't need one. Point a standard OpenTelemetry exporter at https://api.mcpd.sh or at your own daemon. The routes that work today are the source build, the hosted version and an OpenTelemetry exporter.

### Is mcpd free, and which licence does it use?

According to its repository, mcpd is free and licensed under Apache-2.0.

### Does mcpd sit between my agent and the model?

mcpd isn't a gateway or a proxy. As its README explains, nothing travels through it to reach a model, so a slow or stopped daemon can't slow or break your agent.

## Where should you start with mcpd today?

Pick the route that fits the time you have.

- In the next 5 minutes, build from the mcpd github repository at github.com/loruorg/mcpd with `make build`, run `./target/release/mcpd demo`, and open http://localhost:4317. Or sign up at the hosted version.
- In the next hour, point an OpenTelemetry exporter at it with `OTEL_EXPORTER_OTLP_ENDPOINT`, then read the prompt and completion of the first run.
- This week, set keys and redaction before you share the instance. Add a cost monitor so a spike reaches your webhook before it reaches your invoice.

The exporter endpoint is the only setting your agent holds, so you can change it whenever you want to move or send the data elsewhere.

## Sources

[mcpd · The unopinionated agent observatory.](https://mcpd.sh) · [GitHub - loruorg/mcpd](https://github.com/loruorg/mcpd) · [OpenTelemetry · mcpd](https://mcpd.sh/docs/opentelemetry) · [Security · mcpd](https://mcpd.sh/docs/security) · [Integrating · mcpd](https://mcpd.sh/docs/integrating) · [Running it · mcpd](https://mcpd.sh/docs/operations) · [Cost · mcpd](https://mcpd.sh/docs/cost) · [Transports - Model Context Protocol](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports). Every other source is linked where the article cites it.
