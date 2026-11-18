---
description: Which open source Langfuse alternative is lightest to self-host? mcpd is Apache-2.0, one Rust binary that ingests OTLP. Compare licences and run both together.
faq:
  - answer: "mcpd is an Apache-2.0 open source Langfuse alternative that runs as one Rust binary with built-in SQLite storage and a web interface that ingests OpenTelemetry traces. Licences differ across the alternatives, so check each repository's LICENSE file before calling a tool open source."
    question: What is the lightest open source Langfuse alternative to self-host?
  - answer: Langfuse is open source at its core, which is MIT licensed, according to a 2026 comparison of its alternatives. For anything outside the core, read the LICENSE file in the Langfuse repository and its licensing documentation to confirm the terms that apply before you rely on it.
    question: Is Langfuse open source?
  - answer: You can run mcpd and Langfuse side by side, because mcpd forwards what it stores as OTLP to any endpoint you name, and its forwarding presets include Langfuse. Keep your exporter pointed at mcpd and let it send the same traces onward while you compare the two.
    question: Can I run mcpd and Langfuse side by side?
  - answer: mcpd is a daemon, neither a gateway nor a proxy, and nothing travels through it to reach a model. A slow or down mcpd cannot slow or break your agent. Your agent calls the model provider directly and sends only telemetry to mcpd.
    question: Is mcpd a proxy?
  - answer: mcpd keeps its data in a single SQLite file by default, which means a first trace needs no database cluster. A team can also choose Postgres or ClickHouse storage, and the same binary that runs the SQLite default supports both of those backends.
    question: Where does mcpd keep its data?
image: /blog/open-source-langfuse-alternative/featured-ab5fb1f414a3.webp
image_alt: "Illustration for Open source Langfuse alternative: Licences compared"
publishDate: "2026-11-18"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/open-source-langfuse-alternative#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\",\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q27825826\",\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]}],\"datePublished\":\"2026-11-18\",\"description\":\"Which open source Langfuse alternative is lightest to self-host? mcpd is Apache-2.0, one Rust binary that ingests OTLP. Compare licences and run both together.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"mcpd is an Apache-2.0 open source Langfuse alternative that runs as one Rust binary with built-in SQLite storage and a web interface that ingests OpenTelemetry traces. Licences differ across the alternatives, so check each repository's LICENSE file before calling a tool open source.\"},\"name\":\"What is the lightest open source Langfuse alternative to self-host?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Langfuse is open source at its core, which is MIT licensed, according to a 2026 comparison of its alternatives. For anything outside the core, read the LICENSE file in the Langfuse repository and its licensing documentation to confirm the terms that apply before you rely on it.\"},\"name\":\"Is Langfuse open source?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You can run mcpd and Langfuse side by side, because mcpd forwards what it stores as OTLP to any endpoint you name, and its forwarding presets include Langfuse. Keep your exporter pointed at mcpd and let it send the same traces onward while you compare the two.\"},\"name\":\"Can I run mcpd and Langfuse side by side?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"mcpd is a daemon, neither a gateway nor a proxy, and nothing travels through it to reach a model. A slow or down mcpd cannot slow or break your agent. Your agent calls the model provider directly and sends only telemetry to mcpd.\"},\"name\":\"Is mcpd a proxy?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"mcpd keeps its data in a single SQLite file by default, which means a first trace needs no database cluster. A team can also choose Postgres or ClickHouse storage, and the same binary that runs the SQLite default supports both of those backends.\"},\"name\":\"Where does mcpd keep its data?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q117340550\",\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q334661\",\"@type\":\"Thing\",\"description\":\"permissive free software license\",\"name\":\"MIT License\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MIT_License\",\"https://www.wikidata.org/wiki/Q334661\",\"https://www.google.com/search?kgmid=/m/04vpm\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q134366868\",\"@type\":\"Thing\",\"name\":\"OpenLit\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q134366868\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q21708200\",\"@type\":\"Organization\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q125446413\",\"@type\":\"Organization\",\"description\":\"large language model company\",\"name\":\"LlamaIndex\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LlamaIndex\",\"https://www.wikidata.org/wiki/Q125446413\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q22661306\",\"@type\":\"Thing\",\"description\":\"software to manage containers on a server-cluster\",\"name\":\"Kubernetes\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Kubernetes\",\"https://www.wikidata.org/wiki/Q22661306\",\"https://www.google.com/search?kgmid=/g/11b7lxp79d\"]}],\"name\":\"Open source Langfuse alternative: Licences compared\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/OU8a4tmRI_Kah5yyOy9bkaGnhA_FY81P.webp\"},\"url\":\"https://mcpd.sh/blog/open-source-langfuse-alternative\"},{\"@id\":\"https://mcpd.sh/blog/open-source-langfuse-alternative#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\"},{\"@id\":\"https://www.wikidata.org/wiki/Q27825826\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory.\",\"url\":\"https://mcpd.sh\"},{\"@type\":\"CreativeWork\",\"name\":\"9 Best Langfuse Alternatives for LLM Observability in 2026\",\"url\":\"https://www.cekura.ai/blogs/langfuse-alternative\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"Best Langfuse alternatives (open source, 2026) | OpenLIT\",\"url\":\"https://openlit.io/blogs/langfuse-alternatives\"},{\"@type\":\"CreativeWork\",\"name\":\"Cost · mcpd\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"loruorg/mcpd README\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"Integrating · mcpd\",\"url\":\"https://mcpd.sh/docs/integrating\"},{\"@type\":\"CreativeWork\",\"name\":\"Running it · mcpd\",\"url\":\"https://mcpd.sh/docs/operations\"},{\"@type\":\"CreativeWork\",\"name\":\"Evaluation · mcpd\",\"url\":\"https://mcpd.sh/docs/evaluation\"},{\"@type\":\"CreativeWork\",\"name\":\"Alerting · mcpd\",\"url\":\"https://mcpd.sh/docs/alerting\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd security documentation\",\"url\":\"https://mcpd.sh/docs/security\"},{\"@type\":\"CreativeWork\",\"name\":\"Forwarding · mcpd\",\"url\":\"https://mcpd.sh/docs/forwarding\"}],\"dateModified\":\"2026-11-18\",\"datePublished\":\"2026-11-18\",\"description\":\"Which open source Langfuse alternative is lightest to self-host? mcpd is Apache-2.0, one Rust binary that ingests OTLP. Compare licences and run both together.\",\"headline\":\"Open source Langfuse alternative: Licences compared\",\"image\":\"https://api.birchseek.com/m/OU8a4tmRI_Kah5yyOy9bkaGnhA_FY81P.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/open-source-langfuse-alternative#webpage\"},\"keywords\":[\"open source langfuse alternative\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/open-source-langfuse-alternative#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q117340550\"},{\"@id\":\"https://www.wikidata.org/wiki/Q334661\"},{\"@id\":\"https://www.wikidata.org/wiki/Q134366868\"},{\"@id\":\"https://www.wikidata.org/wiki/Q21708200\"},{\"@id\":\"https://www.wikidata.org/wiki/Q125446413\"},{\"@id\":\"https://www.wikidata.org/wiki/Q22661306\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2636},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"artificial intelligence tool for code autocompletion\",\"name\":\"GitHub Copilot\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub_Copilot\",\"https://www.wikidata.org/wiki/Q107435063\",\"https://www.google.com/search?kgmid=/g/11q83qbj3d\"]},{\"@type\":\"Thing\",\"description\":\"Chinese artificial intelligence company\",\"name\":\"DeepSeek\",\"sameAs\":[\"https://en.wikipedia.org/wiki/DeepSeek\",\"https://www.wikidata.org/wiki/Q131577453\",\"https://www.google.com/search?kgmid=/g/11wvrb0s91\"]},{\"@type\":\"Thing\",\"description\":\"creation or optimization of a prompt to be given to an artificial intelligence model\",\"name\":\"Prompt engineering\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Prompt_engineering\",\"https://www.wikidata.org/wiki/Q108941486\",\"https://www.google.com/search?kgmid=/g/11p6kpgt_n\"]},{\"@type\":\"Thing\",\"description\":\"general-purpose programming language\",\"name\":\"Python\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Python_(programming_language)\",\"https://www.wikidata.org/wiki/Q28865\",\"https://www.google.com/search?kgmid=/m/05z1_\"]},{\"@type\":\"Thing\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]},{\"@type\":\"Thing\",\"description\":\"permissive free software license\",\"name\":\"MIT License\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MIT_License\",\"https://www.wikidata.org/wiki/Q334661\",\"https://www.google.com/search?kgmid=/m/04vpm\"]},{\"@type\":\"Thing\",\"description\":\"software to manage containers on a server-cluster\",\"name\":\"Kubernetes\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Kubernetes\",\"https://www.wikidata.org/wiki/Q22661306\",\"https://www.google.com/search?kgmid=/g/11b7lxp79d\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - langfuse alternative
  - open source
  - llm observability
  - opentelemetry
  - self-hosting
  - mcpd
title: "Open source Langfuse alternative: Licences compared"
---

**An open source Langfuse alternative has to be judged by its repository's LICENSE file: [Langfuse's core is MIT](https://www.cekura.ai/blogs/langfuse-alternative), [OpenLIT is Apache-2.0](https://openlit.io/blogs/langfuse-alternatives), and so is mcpd. mcpd runs as [one static Rust binary](https://mcpd.sh) with storage and the web interface built in, and it [ingests OTLP](https://mcpd.sh/docs/opentelemetry), reading both `gen_ai.*` and `llm.*` attributes, so your existing instrumentation works unchanged.**

You want Langfuse-style traces (the prompt, the completion and the cost of each run), but you don't want to run a multi-service stack to get them, or your security team needs to read the licence before anything is adopted.

After reading, you can sort each open source Langfuse alternative by licence and compare what each one needs to self-host. You can also send a first trace from an OpenTelemetry exporter. Then run old and new side by side while Langfuse stays up.

> **Key Takeaways**
> - An unknown model has [no cost in mcpd](https://mcpd.sh/docs/cost), not a cost of zero, so a missing price can't read as free.
> - Nothing travels [through mcpd](https://github.com/loruorg/mcpd) to reach a model, so mcpd being slow or down cannot break the agent's request path.
> - Moving traces is an exporter change: two environment variables, with forwarding to Langfuse while you compare.
> - Langfuse remains the better fit when your team's prompt and eval workflows already live there.

## Which open source Langfuse alternatives are actually open source?

Langfuse, OpenLIT and mcpd each name an open source licence in their own sources, so all three qualify; they differ in what that licence covers.

![Illustration for Which open source Langfuse alternatives are actually open source?](/blog/open-source-langfuse-alternative/image-1-c657dcdda25c.webp)

| Tool | Licence | What the licence covers |
| --- | --- | --- |
| Langfuse | MIT | The core |
| OpenLIT | Apache-2.0 | The self-hosted stack |
| mcpd | Apache-2.0 | The repository at github.com/loruorg/mcpd |

Langfuse is the reference point. One [comparison guide from Cekura](https://www.cekura.ai/blogs/langfuse-alternative) states that "the core is still MIT licensed".

The word "core" tells you the licence is stated for part of the product. Read the repository's licensing notes before assuming that every directory carries the same terms.

OpenLIT describes itself in [its own comparison post](https://openlit.io/blogs/langfuse-alternatives) as an "Apache-2.0, OpenTelemetry-native self-hosted stack". [mcpd's repository](https://github.com/loruorg/mcpd) lists Apache-2.0 as its licence.

### Check a licence yourself

The same check works on any open source Langfuse alternative in this comparison:

1. Open the repository and read the top-level `LICENSE` file. Note the licence name and the copyright holder.
2. Search the directory tree for further files named `LICENSE` or `LICENCE`. A second file in a subdirectory means that directory has its own terms.
3. Read the project's licensing page, if it has one, and compare it with the files. Where the two disagree, treat the narrower reading as the safe one.
4. Record the licence, the path and the date you checked in your adoption ticket.

These steps confirm that a core under one licence has no other terms attached elsewhere in the tree.

### Best for

- Langfuse suits teams that accept an MIT core and will read the licensing notes for anything outside it.
- OpenLIT suits teams that want an Apache-2.0 licence on a self-hosted stack.
- mcpd suits teams that need Apache-2.0 on the whole repository they build from source.

## Instrumentation library, in-process platform or proxy: which do you need?

An instrumentation library emits traces from inside your code, a platform stores and shows them, and a proxy or gateway sits in the request path to the model. A proxy adds its latency to every call and becomes a point of failure, while a daemon that only receives telemetry can't break the agent.

A library wraps your model and tool calls and emits a span for each one: a timed record holding the prompt, the completion and the token counts. It stores nothing.

A platform receives spans, stores them, indexes the text so you can search for one bad run, prices the tokens and draws the waterfall. The [mcpd concepts guide](https://mcpd.sh/docs/concepts) defines traces, spans and signals in detail.

A proxy or gateway receives every request your agent makes and forwards it to the provider. Each call waits on the proxy, and if the proxy goes down, every call fails even when the model is healthy.

The [mcpd README](https://github.com/loruorg/mcpd) states the contrast: "It is not a gateway and not a proxy. Nothing travels through mcpd to reach a model, so mcpd being slow or down cannot slow your agent or break it." mcpd is a daemon that receives telemetry after the call has already happened.

The SDKs follow the same rule. According to the [mcpd integration guide](https://mcpd.sh/docs/integrating), recording a span never blocks, never throws and never grows without bound, so an unreachable mcpd goes unnoticed by the agent.

The native SDKs are a convenience over a plain HTTP API. The OpenTelemetry route needs no mcpd package: you set `OTEL_EXPORTER_OTLP_ENDPOINT` and keep the instrumentation you already have.

## Is a self-hosted open source Langfuse alternative lighter to run?

A self-hosted open source Langfuse alternative is lighter to run when it ships as one binary with its storage inside, which is how mcpd starts.

| Criterion | Langfuse, self-hosted | mcpd by default | mcpd for a team |
| --- | --- | --- | --- |
| Stateful services | Four | None to stand up | Postgres or ClickHouse |
| Where traces live | Those services | One SQLite file | Your database |
| Core licence | MIT | Apache-2.0 | Apache-2.0 |

[Cekura's Langfuse comparison](https://www.cekura.ai/blogs/langfuse-alternative) lists "not wanting to run four stateful services to self-host" among the reasons teams look elsewhere. The same page says Langfuse's core is still MIT licensed, so the burden is operational; the licence is not the obstacle.

[mcpd](https://mcpd.sh) is one static Rust binary with the storage and the web interface built in.

Building from source needs Rust and Node, because the interface is compiled into the binary:

```shell
git clone https://github.com/loruorg/mcpd
cd mcpd
make build
./target/release/mcpd demo
```

Open http://localhost:4317 and the demo traces are there, with no account or database cluster to set up first. `make dist` produces the optimised build.

A platform team runs the same artefact.

Kubernetes gets `/healthz` and `/readyz` probes and a graceful shutdown on SIGTERM, described in the [mcpd operations guide](https://mcpd.sh/docs/operations).

Weight is not the only test: what you need from the stack matters too. If you rely on Langfuse's prompt and eval workflow, four services may be a fair price for it.

Which open source Langfuse alternative fits depends on the team:

- Langfuse, self-hosted: teams that already run stateful services and want its prompt and eval workflow.
- mcpd by default: one engineer or a small team that wants the first trace on a laptop.
- mcpd for a team: a platform team that wants SSO, SCIM, audit and a database it already operates.

## How do you migrate to an open source Langfuse alternative over OpenTelemetry?

You migrate to an open source Langfuse alternative over OpenTelemetry by changing your OTLP exporter's environment variables, checking that your span attributes carry over, and running both backends side by side before you cut over. The whole change is configuration, so the agent code stays as it is.

1. Change the exporter variables. Set the endpoint and protocol on an agent that already emits `gen_ai.*` spans, then restart it:

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

Add your project key as an OTLP header; the [mcpd OpenTelemetry docs](https://mcpd.sh/docs/opentelemetry) give the header name. Send one request through the agent and open the project in the hosted app. The first run should show its prompt, completion and tokens, with no code change.

2. Check the attribute mapping. List the attributes your spans carry, then check which of them Langfuse's OpenTelemetry integration reads. Any attribute that only one tool reads needs a manual check in both interfaces.

3. Run both in parallel. Either add a second OTLP exporter in your SDK or collector that points at Langfuse, or send everything to mcpd and let it forward onward. According to the [mcpd README](https://github.com/loruorg/mcpd), mcpd forwards as OTLP to any endpoint you name, and Langfuse's OTLP endpoint is one such destination. Forwarding keeps one exporter in your code and leaves Langfuse receiving the same spans.

4. Compare, then cut over. Pick 10 runs and confirm the same trace tree, token counts and payloads in both tools. Check cost last: a gap in mcpd's cost column means a model missing from the price book. When the runs match, remove the Langfuse exporter or forwarding target.

5. Keep the way back open. To return to Langfuse, point the same exporter at Langfuse's endpoint, or keep forwarding on. To leave mcpd, the data was already OTLP, so any backend that accepts OTLP can take it.

Because the change sits in the exporter, it applies to whatever emits the spans. [mcpd's home page](https://mcpd.sh) describes it as working with "Any framework, any language, any model provider," which covers LangChain, LangGraph, LlamaIndex, CrewAI, the Vercel AI SDK and the OpenAI Agents SDK through their existing instrumentation.

Start with the hosted version at [mcpd.sh/app](https://mcpd.sh/app), set the two variables above, and run one request.

## How do you find the one bad run, and what did it cost?

You find the one bad run by searching for what the model said, then reading its waterfall; its cost comes from a price applied once at ingest.

A refund agent that calls a `lookup_order` tool again and again shows the pattern: the day's spend jumps, and a cost monitor fires first. [mcpd's monitors](https://mcpd.sh/docs/alerting) evaluate every 60 s on error_rate, latency_p95, cost, score_mean, trace_count and step-count divergence. They latch, fire once, resolve only on a real reading and call your webhook.

The alert marks the day, and search finds the run. On [mcpd](https://mcpd.sh), search finds a run by what the model said (full-text over the payload), and the interface shows the exact prompt and answer, the tokens used and the cost of each step.

A phrase the model keeps repeating, such as "let me check that order again", narrows thousands of runs to a handful, where a label filter would return them all. Open the hit. The waterfall shows one `lookup_order` span repeated on a single clock, and the full conversation shows the model asking for the same lookup with the same arguments.

The `no_tool_loop` scorer flags that pattern on every run.

Scores are what a machine thought. Feedback from a person stays a separate signal, and mcpd never averages the two.

Cost follows one rule. Each step is priced once, at ingest, from a [price book](https://mcpd.sh/docs/cost) of per-million-token rates, with cached tokens, long-prompt tiers and service tiers handled. You can supply your own book.

An unknown model has no cost, not a cost of zero, so a missing price can't pass as a free run.

## What do self-hosters need for redaction, retention and access control?

A self-hosted observability stack needs seven controls before real prompts reach storage. mcpd's [security documentation](https://mcpd.sh/docs/security) covers each one, and you can read the Langfuse and Phoenix documentation against the same list.

1. **Redaction before storage.**
2. **Retention in days.** Without a limit, prompts that contain customer text pile up indefinitely.
3. **Erasure by person.** Erasure requests name a person, not a date range. Without them, the only answer to a data subject's request is deleting everything.
4. **Keys and scopes.** mcpd has six scopes: ingest, read, write, execute, worker and admin. Keys are compared as SHA-256 hashes. With no keys set, ingest and reads are open, which is for a laptop only.
5. **Organisations, roles, SSO and SCIM.** Roles are viewer, member, admin and owner. Single sign-on works with any OpenID Connect provider (Google, Microsoft Entra ID, Okta, Auth0), and GitHub goes through an OAuth app. SCIM removes leavers from the directory, and quotas stop one team using everyone's capacity.
6. **An audit log.** When an auditor asks who saw what, the audit log gives a recorded answer.
7. **A quiet default.** With a default configuration, nothing leaves the machine, including update checks, usage reporting and telemetry. A network-restricted site doesn't have to block outbound calls to find out.

The [mcpd relay and regions guide](https://mcpd.sh/docs/relay) covers the setup.

## When should you stay on Langfuse?

Stay on Langfuse when your prompt management and evaluation workflows already run on it and work, because moving them costs more than it returns. [OpenLIT's comparison](https://openlit.io/blogs/langfuse-alternatives) credits Langfuse with "mature prompt and eval UX, solid self-host options, and a large community."

Check three things before you move to an open source Langfuse alternative:

- Which features you use. List what your team touches every week: prompt versions, datasets, evaluators, dashboards. A trace viewer is quick to replace; a prompt library your team edits daily is not.
- Where the pain is. [Cekura](https://www.cekura.ai/blogs/langfuse-alternative) names one common reason teams look elsewhere: "not wanting to run four stateful services to self-host." That pain belongs to the deployment, and if it is your only complaint, fixing the stack is a smaller job than replacing the tool.
- The licence. Cekura's write-up says "the core is still MIT licensed." Confirm that in the LICENSE file of Langfuse's own repository and in Langfuse's own licensing documentation.

Staying and adding mcpd also works. The [mcpd README](https://github.com/loruorg/mcpd) says what it stores it also forwards "as OTLP, to any endpoint you name," so Langfuse can be one of those endpoints and both tools can receive the same traces.

## Frequently asked questions

### What is the lightest open source Langfuse alternative to self-host?

[mcpd](https://github.com/loruorg/mcpd) is an Apache-2.0 open source Langfuse alternative: one Rust binary with built-in SQLite storage and a web interface that ingests OpenTelemetry traces. Licences differ across the alternatives, so check each repository's LICENSE file before calling a tool open source.

### Is Langfuse open source?

Langfuse's core is MIT licensed, according to a [2026 comparison of its alternatives](https://www.cekura.ai/blogs/langfuse-alternative). For anything outside the core, read the LICENSE file in its repository and its licensing documentation.

### Can I run mcpd and Langfuse side by side?

You can. mcpd forwards what it stores as OTLP to any endpoint you name, and its [forwarding presets](https://mcpd.sh/docs/forwarding) include Langfuse. Keep your exporter pointed at mcpd and let it send the same traces onward while you compare the two.

### Is mcpd a proxy?

mcpd is a daemon, not a gateway or a proxy. Nothing travels through it to reach a model, so mcpd being slow or down cannot slow or break your agent. Your agent calls the model provider directly and sends only telemetry to mcpd.

### Where does mcpd keep its data?

mcpd keeps data in one SQLite file by default, so a first trace needs no database cluster. A team can use Postgres or ClickHouse storage with the same binary, as the [mcpd operations guide](https://mcpd.sh/docs/operations) describes.

## Next steps: your first trace today, a parallel run this week

Today: open the hosted version, create a project and copy its key. Set `OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh` and `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`, then run your agent once and read the prompt, completion and cost of the first trace.

This week: run mcpd beside Langfuse, either with the Langfuse forwarding preset or a second OTLP exporter, so both receive the same spans. Set redaction and retention before real prompts arrive.

This month: compare real runs in both tools. Then decide whether to move to an open source Langfuse alternative or stay. If your prompt and eval workflows live on Langfuse, staying is a fair result.

Whichever you choose, the data stays portable: OpenTelemetry in, OTLP out, an open licence around it. The hosted version is free with every feature, and you can build from source at [github.com/loruorg/mcpd](https://github.com/loruorg/mcpd) to self-host.

[Start free at mcpd.sh/app](https://mcpd.sh/app) and keep Langfuse running until you're sure. The [quickstart](https://mcpd.sh/docs/quickstart) covers the first trace step by step.

## Sources

[9 Best Langfuse Alternatives for LLM Observability in 2026](https://www.cekura.ai/blogs/langfuse-alternative) · [Best Langfuse alternatives (open source, 2026) | OpenLIT](https://openlit.io/blogs/langfuse-alternatives) · [mcpd · The unopinionated agent observatory.](https://mcpd.sh) · [Cost · mcpd](https://mcpd.sh/docs/cost) · [loruorg/mcpd README](https://github.com/loruorg/mcpd) · [Integrating · mcpd](https://mcpd.sh/docs/integrating) · [Evaluation · mcpd](https://mcpd.sh/docs/evaluation) · [mcpd security documentation](https://mcpd.sh/docs/security). Every other source is linked where the article cites it.
