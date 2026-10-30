---
description: How do you start tracing AI agents free? Set two OpenTelemetry variables and a project key, run your agent, and see each prompt, answer and cost. Start now.
faq:
  - answer: "Sign up at mcpd.sh/app, create a project key, and set OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh and OTEL_EXPORTER_OTLP_PROTOCOL=http/json in your agent's environment. Add the key as the exporter's authorisation header. Run the agent once and the trace appears in your mcpd project. You install no package."
    question: How do I start tracing AI agents for free?
  - answer: You need no SDK and no framework change. Any agent that already exports OpenTelemetry spans sends them to mcpd once its exporter reads those two environment variables. Spans that carry gen_ai.* attributes show up as model calls. The mcpd OpenTelemetry guide lists the supported conventions.
    question: Do I need an SDK or to change my framework?
  - answer: "Mcpd cannot slow down or break your agent, because it sits beside the agent, not in front of it. Nothing travels through mcpd to reach a model, so it isn't a gateway or a proxy. If the daemon is slow or down, your agent's model calls run the same way."
    question: Can mcpd slow down or break my agent?
  - answer: "Your agent sends spans only to the address in OTEL_EXPORTER_OTLP_ENDPOINT. Set it to https://api.mcpd.sh and the spans, prompts included, go to your project there. Build from source at github.com/loruorg/mcpd and point the exporter at your own machine to keep them local."
    question: Is my prompt data sent anywhere by default?
  - answer: "You can move your traces to another tool, because mcpd forwards what it stores as OTLP to any endpoint you name. Another OpenTelemetry-compatible tool can then receive the same spans. Your agent's instrumentation doesn't change, because it only ever spoke OpenTelemetry."
    question: Can I move my traces to another tool later?
publishDate: "2026-10-30"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/start-tracing-ai-agents-free#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\",\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q125446413\",\"@type\":\"Organization\",\"description\":\"large language model company\",\"name\":\"LlamaIndex\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LlamaIndex\",\"https://www.wikidata.org/wiki/Q125446413\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q364\",\"@type\":\"Organization\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q165194\",\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q21708200\",\"@type\":\"Organization\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]}],\"datePublished\":\"2026-10-30\",\"description\":\"How do you start tracing AI agents free? Set two OpenTelemetry variables and a project key, run your agent, and see each prompt, answer and cost. Start now.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Sign up at mcpd.sh/app, create a project key, and set OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh and OTEL_EXPORTER_OTLP_PROTOCOL=http/json in your agent's environment. Add the key as the exporter's authorisation header. Run the agent once and the trace appears in your mcpd project. You install no package.\"},\"name\":\"How do I start tracing AI agents for free?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You need no SDK and no framework change. Any agent that already exports OpenTelemetry spans sends them to mcpd once its exporter reads those two environment variables. Spans that carry gen_ai.* attributes show up as model calls. The mcpd OpenTelemetry guide lists the supported conventions.\"},\"name\":\"Do I need an SDK or to change my framework?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Mcpd cannot slow down or break your agent, because it sits beside the agent, not in front of it. Nothing travels through mcpd to reach a model, so it isn't a gateway or a proxy. If the daemon is slow or down, your agent's model calls run the same way.\"},\"name\":\"Can mcpd slow down or break my agent?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Your agent sends spans only to the address in OTEL_EXPORTER_OTLP_ENDPOINT. Set it to https://api.mcpd.sh and the spans, prompts included, go to your project there. Build from source at github.com/loruorg/mcpd and point the exporter at your own machine to keep them local.\"},\"name\":\"Is my prompt data sent anywhere by default?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You can move your traces to another tool, because mcpd forwards what it stores as OTLP to any endpoint you name. Another OpenTelemetry-compatible tool can then receive the same spans. Your agent's instrumentation doesn't change, because it only ever spoke OpenTelemetry.\"},\"name\":\"Can I move my traces to another tool later?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q117340550\",\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q141164091\",\"@type\":\"Organization\",\"description\":\"American artificial intelligence software company\",\"name\":\"Braintrust\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Braintrust\",\"https://www.wikidata.org/wiki/Q141164091\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q27825826\",\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q116758847\",\"@type\":\"Organization\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q56069184\",\"@type\":\"Organization\",\"description\":\"San Francisco based cloud computing company\",\"name\":\"Vercel\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Vercel\",\"https://www.wikidata.org/wiki/Q56069184\",\"https://www.google.com/search?kgmid=/g/11jclfls71\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q107381687\",\"@type\":\"Thing\",\"description\":\"Python library for data parsing and validation using Python type hints\",\"name\":\"pydantic\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q107381687\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q124636097\",\"@type\":\"Thing\",\"description\":\"software for running large language models (LLMs) on a local computer or in the cloud\",\"name\":\"Ollama\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Ollama\",\"https://www.wikidata.org/wiki/Q124636097\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q37033\",\"@type\":\"Thing\",\"description\":\"main international standards organization for the World Wide Web\",\"name\":\"World Wide Web Consortium\",\"sameAs\":[\"https://en.wikipedia.org/wiki/World_Wide_Web_Consortium\",\"https://www.wikidata.org/wiki/Q37033\",\"https://www.google.com/search?kgmid=/m/082bb\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q119718658\",\"@type\":\"Organization\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q126050137\",\"@type\":\"Organization\",\"description\":\"American artificial intelligence company\",\"name\":\"Groq\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Groq\",\"https://www.wikidata.org/wiki/Q126050137\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q139761640\",\"@type\":\"Thing\",\"description\":\"open-source artificial intelligence agent framework\",\"name\":\"CrewAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/CrewAI\",\"https://www.wikidata.org/wiki/Q139761640\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q137793309\",\"@type\":\"Thing\",\"description\":\"open-source software for large language model inference\",\"name\":\"VLLM\",\"sameAs\":[\"https://en.wikipedia.org/wiki/VLLM\",\"https://www.wikidata.org/wiki/Q137793309\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q116698014\",\"@type\":\"Thing\",\"description\":\"artificial intelligence chatbot developed by Google\",\"name\":\"Google Gemini\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google_Gemini\",\"https://www.wikidata.org/wiki/Q116698014\",\"https://www.google.com/search?kgmid=/g/11ts49p01g\"]}],\"name\":\"Start tracing AI agents free: Setup in 5 minutes\",\"url\":\"https://mcpd.sh/blog/start-tracing-ai-agents-free\"},{\"@id\":\"https://mcpd.sh/blog/start-tracing-ai-agents-free#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\"},{\"@id\":\"https://www.wikidata.org/wiki/Q125446413\"},{\"@id\":\"https://www.wikidata.org/wiki/Q364\"},{\"@id\":\"https://www.wikidata.org/wiki/Q165194\"},{\"@id\":\"https://www.wikidata.org/wiki/Q21708200\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"mcpd.sh\",\"url\":\"https://mcpd.sh\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd README\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"Tracking distributed tracing for AI agents: how to follow one request across services\",\"url\":\"https://www.braintrust.dev/articles/distributed-tracing-ai-agents\"},{\"@type\":\"CreativeWork\",\"name\":\"Cost · mcpd\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"Privacy · mcpd\",\"url\":\"https://mcpd.sh/docs/privacy\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry Protocol Exporter\",\"url\":\"https://opentelemetry.io/docs/specs/otel/protocol/exporter/\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"Evaluation · mcpd\",\"url\":\"https://mcpd.sh/docs/evaluation\"},{\"@type\":\"CreativeWork\",\"name\":\"Trace Context, W3C Recommendation\",\"url\":\"https://www.w3.org/TR/trace-context/\"},{\"@type\":\"CreativeWork\",\"name\":\"Testing · mcpd\",\"url\":\"https://mcpd.sh/docs/testing\"},{\"@type\":\"CreativeWork\",\"name\":\"Alerting · mcpd\",\"url\":\"https://mcpd.sh/docs/alerting\"}],\"dateModified\":\"2026-10-30\",\"datePublished\":\"2026-10-30\",\"description\":\"How do you start tracing AI agents free? Set two OpenTelemetry variables and a project key, run your agent, and see each prompt, answer and cost. Start now.\",\"headline\":\"Start tracing AI agents free: Setup in 5 minutes\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/start-tracing-ai-agents-free#webpage\"},\"keywords\":[\"start tracing ai agents free\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/start-tracing-ai-agents-free#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q117340550\"},{\"@id\":\"https://www.wikidata.org/wiki/Q141164091\"},{\"@id\":\"https://www.wikidata.org/wiki/Q27825826\"},{\"@id\":\"https://www.wikidata.org/wiki/Q116758847\"},{\"@id\":\"https://www.wikidata.org/wiki/Q56069184\"},{\"@id\":\"https://www.wikidata.org/wiki/Q107381687\"},{\"@id\":\"https://www.wikidata.org/wiki/Q124636097\"},{\"@id\":\"https://www.wikidata.org/wiki/Q37033\"},{\"@id\":\"https://www.wikidata.org/wiki/Q119718658\"},{\"@id\":\"https://www.wikidata.org/wiki/Q126050137\"},{\"@id\":\"https://www.wikidata.org/wiki/Q139761640\"},{\"@id\":\"https://www.wikidata.org/wiki/Q137793309\"},{\"@id\":\"https://www.wikidata.org/wiki/Q116698014\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2572},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"defined sequence of activities of an organizational entity, enabled by the systematic organization of resources into processes that transform materials, provide services, or process information\",\"name\":\"Workflow\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Workflow\",\"https://www.wikidata.org/wiki/Q627335\",\"https://www.google.com/search?kgmid=/m/0k5rb\"]},{\"@type\":\"Thing\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]},{\"@type\":\"Thing\",\"description\":\"collection of data, publication type\",\"name\":\"Data set\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Data_set\",\"https://www.wikidata.org/wiki/Q1172284\",\"https://www.google.com/search?kgmid=/m/02cck\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"benchmark in natural language understanding\",\"name\":\"General Language Understanding Evaluation benchmark\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q64711428\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"large language model company\",\"name\":\"LlamaIndex\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LlamaIndex\",\"https://www.wikidata.org/wiki/Q125446413\"]},{\"@type\":\"Thing\",\"description\":\"Chinese artificial intelligence company\",\"name\":\"DeepSeek\",\"sameAs\":[\"https://en.wikipedia.org/wiki/DeepSeek\",\"https://www.wikidata.org/wiki/Q131577453\",\"https://www.google.com/search?kgmid=/g/11wvrb0s91\"]},{\"@type\":\"Thing\",\"description\":\"language understanding across multiple domains\",\"name\":\"MMLU\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MMLU\",\"https://www.wikidata.org/wiki/Q123721276\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"family of multimodal artificial intelligence models developed by OpenAI\",\"name\":\"GPT-4o\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GPT-4o\",\"https://www.wikidata.org/wiki/Q125919502\",\"https://www.google.com/search?kgmid=/g/11vxhg1zhp\"]},{\"@type\":\"Thing\",\"description\":\"family of large language models developed by OpenAI\",\"name\":\"GPT-5\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GPT-5\",\"https://www.wikidata.org/wiki/Q135297698\"]},{\"@type\":\"Thing\",\"description\":\"a lightweight markup language with plain-text formatting syntax that is popularly used\",\"name\":\"Markdown\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Markdown\",\"https://www.wikidata.org/wiki/Q1193600\",\"https://www.google.com/search?kgmid=/m/0jt0gkj\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - ai agents
  - tracing
  - opentelemetry
  - observability
  - llm cost
title: "Start tracing AI agents free: Setup in 5 minutes"
---

**To start tracing AI agents free, sign up at mcpd.sh/app, create a project and set `OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh` and `OTEL_EXPORTER_OTLP_PROTOCOL=http/json` with that project's API key. Run your agent: each [model call, tool call and retrieval](https://mcpd.sh) appears with its prompt, answer and cost. Any framework, language or provider works, and you don't need a vendor package.**

A refund agent that worked on your laptop calls the same tool again and again in production. Or the model bill spikes overnight, and the logs don't say which run caused it.

With a first trace sent from your own code, you can go from either symptom to one run, one span inside it and the payload that explains it: the prompt the model received, the completion it returned and what the step cost.

> **Key Takeaways**
> - mcpd is not a [gateway or proxy](https://github.com/loruorg/mcpd), so recording a span never blocks or throws, and a slow daemon can't slow your agent.
> - An [unknown model has no cost](https://mcpd.sh/docs/cost), not a cost of zero, so a missing price never reads as free.
> - A [W3C `traceparent` header](https://www.braintrust.dev/articles/distributed-tracing-ai-agents) is vendor-neutral and keeps an agent across API, queue and worker as one trace.
> - [Redaction runs before the write](https://mcpd.sh/docs/privacy), so a masked secret was never stored.

## How do you start tracing AI agents free in five minutes?

You start tracing AI agents free by pointing the agent's OpenTelemetry exporter at `https://api.mcpd.sh` with a project key. It takes about five minutes and adds no package to the agent.

1. Sign up at [mcpd.sh/app](https://mcpd.sh/app).
2. Create a project and copy its key. Store it in your secret manager or an untracked `.env` file, never in the repository.
3. Export the exporter variables in the shell or service environment that launches the agent:

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
# add the project key exactly as https://mcpd.sh/docs/opentelemetry shows
```

The [OpenTelemetry exporter specification](https://opentelemetry.io/docs/specs/otel/protocol/exporter/) defines both variables, and `http/json` is one of its protocol values next to `http/protobuf`. Place the project key where mcpd's [OpenTelemetry documentation](https://mcpd.sh/docs/opentelemetry) says to place it.

4. Run the agent as you normally do. Exporters read these variables at startup, so restart a process that was already running, and set them in the same terminal or service unit that launches it.
5. Open the run in the interface at mcpd.sh/app, inside your project. The newest run is at the top.

## What you see when you start tracing AI agents free

When you start tracing AI agents free, the first trace is a tree: one root span for the run, with each model call, tool call and retrieval nested beneath it in the order they ran.

[mcpd's home page](https://mcpd.sh) puts it as "A run is a tree of model calls, tool calls and retrievals." Opening a step shows its prompt, its answer, its tokens and its cost.

A refund agent that calls a lookup tool twice and then the model once produces a root span with two tool spans and a model span beneath it.

## Why you can start tracing AI agents free with no SDK

You can start tracing AI agents free without an SDK because OpenTelemetry exporters read the same two environment variables in every language.

The setup applies whatever you build with, as long as the instrumentation emits OTLP:

- Frameworks: LangChain, LangGraph, LlamaIndex, CrewAI, the Vercel AI SDK, the OpenAI Agents SDK or Pydantic AI.
- Model providers: OpenAI, Anthropic, Google Gemini, Mistral, Azure OpenAI, Groq, Ollama or vLLM.

mcpd runs as a daemon beside the agent, so there is no framework to build on. Its SDKs are optional convenience over a plain [HTTP API](https://mcpd.sh/docs/reference/api), and the [integration guide](https://mcpd.sh/docs/integrating) shows when to use them.

[Start free at mcpd.sh/app](https://mcpd.sh/app) and send your first run.

## Where can you start tracing AI agents free?

Free covers every feature on all three routes (the hosted version, the OTLP endpoint and a source build), with no paid plan and no feature behind a gate.

| Criterion | Hosted at mcpd.sh/app | OTLP endpoint | Source build |
| --- | --- | --- | --- |
| What you need | A sign-up and a project key | Any OTLP exporter and a project key | Rust and Node |
| Address | mcpd.sh/app | `https://api.mcpd.sh` | `http://localhost:4317` |
| Where data lives | Your hosted project | Your hosted project | One SQLite file by default |
| Account | Yes | Yes, for the key | None |

The hosted version at mcpd.sh/app is the full product. Its ingest address is the OTLP endpoint `https://api.mcpd.sh`, so any exporter in any language reaches it with `OTEL_EXPORTER_OTLP_ENDPOINT` and a project key.

To build from source with Rust and Node installed:

- `make build` compiles the binary, with the interface compiled into it.
- `make dist` produces the optimised build.

Then run `./target/release/mcpd demo` and open `http://localhost:4317`. The code is at [github.com/loruorg/mcpd](https://github.com/loruorg/mcpd).

Hosted and self-hosted are the same product: one static Rust binary that serves a laptop and a team cluster. These are configuration on that binary:

- accounts and roles
- single sign-on
- quotas
- an audit log

A team points the binary at Postgres or ClickHouse instead of the SQLite file. The [operations guide](https://mcpd.sh/docs/operations) covers running it.

You can leave without a migration. The mcpd README says, "What it stores it also forwards, as OTLP, to any endpoint you name." Moving data elsewhere is an OTLP setting.

The code is under the Apache-2.0 licence.

- For a first trace today, with nothing to run, use the hosted version.
- If an agent already exports OpenTelemetry, change one address to the OTLP endpoint.
- A laptop, an air-gapped machine or a team that wants the data on its own storage fits the source build.

## What does a trace of an AI agent contain?

A trace of an AI agent is the record of one run: a tree of spans, one per model call, tool call and retrieval, each with its prompt, completion, token counts, latency and cost. mcpd builds it from OpenTelemetry `gen_ai.*` and OpenInference `llm.*` attributes, so [existing instrumentation works without changes](https://mcpd.sh/docs/opentelemetry).

When users report that a refund agent never finishes, filter for the slow runs and open one in the [runs view](https://mcpd.sh/docs/runs). The waterfall shows a root span for the whole run, with model, tool and retrieval spans nested under it.

In a looping run the same lookup tool span repeats.

```text
agent run (root span)
  model call   prompt, completion, tokens, latency, cost
  tool call    lookup_order
  model call
  tool call    lookup_order
  retrieval    refund policy
```

Open the last model span and read its payload. The prompt holds the tool result, and the completion asks for the same lookup again.

The [mcpd home page](https://mcpd.sh) says "Search finds a run by what the model said", so you can search for the phrase the agent repeats instead of relying on labels alone.

The mcpd docs state that "A score is what a machine thought. Feedback is what a person thought." They stay separate signals and are never averaged into one number.

Scores and feedback join the run's spans, logs, metrics and aspect notes by run. The [concepts page](https://mcpd.sh/docs/concepts) defines each signal.

## How do you keep an agent spanning API, queue and worker in one trace?

An agent stays in one trace across an API, a queue and a worker when every hop passes the W3C `traceparent` header along, so each new span is created as a child of the span that called it.

The [W3C Trace Context Recommendation](https://www.w3.org/TR/trace-context/) defines the header as four fields joined by hyphens, and gives this example: `00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01`.

Read left to right, that is the version (`00`), the trace id, the parent id and the trace flags (`01`). The trace id names the whole run. The parent id names the span that made the call.

A queue has no HTTP request to carry the header, so you put it in the message:

```shell
# API side: store the current context in the job
MSG=$(jq -n --arg tp "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01" \
  '{job:"refund", order:"A-1042", traceparent:$tp}')

# Worker side: read it back before starting any span
echo "$MSG" | jq -r .traceparent
```

In an OpenTelemetry-instrumented worker, the propagator extracts that value and starts its span with the same trace id and the enqueuing span as parent.

Without the header, the worker starts a new trace. [Braintrust's article on distributed tracing](https://www.braintrust.dev/articles/distributed-tracing-ai-agents) says that when context is lost at a service boundary, a failed tool call or slow background job can appear in a separate trace.

The refund run then becomes two fragments, and nothing links the failed tool call to the request that caused it.

mcpd reads W3C `traceparent`, so any OpenTelemetry-instrumented service joins the same trace with no vendor SDK. Logs and metrics carry the same trace id, so they join the run too. The [OpenTelemetry ingestion guide](https://mcpd.sh/docs/opentelemetry) covers the exporter settings.

 

## What controls make tracing AI agents safe in production?

Tracing AI agents is safe in production when each control decides what gets written, how much, for how long, who can read it and what happens to the agent if the tracer fails. mcpd sets every one of them in configuration, and its [security documentation](https://mcpd.sh/docs/security) covers them for a team deployment.

1. Redaction runs before storage, on every signal, with four modes: drop removes the value, mask replaces it, partial keeps a few characters, and keyed hash swaps it for a hash that still lets you match equal values. [mcpd's privacy docs](https://mcpd.sh/docs/privacy) give the reason: "Redaction runs before the write, because a secret deleted afterwards was still written down." Without it, an API key pasted into a prompt lives on in the database, its backups and its exports.

2. Sampling decides which runs are kept, with five modes: always, never, ratio, deterministic and tail. Tail sampling decides once a run is complete, so it can keep the failing and slow runs that a random ratio would drop. Without sampling, a busy agent stores every payload whether anyone will read it or not.

3. Retention is set in days, and data older than that is removed. An erasure request about a person removes what is stored about them. Without both, prompts containing personal data pile up indefinitely and a deletion request has no answer.

4. API keys carry six scopes: ingest, read, write, execute, worker and admin. With no keys set, ingest and reads are open, which is for a laptop only. Give an exporter a key with the ingest scope and a leaked agent credential can't read prompts back.

5. Forwarding goes only where you send it. The only route out is an endpoint you configured, and redaction has already run by the time data reaches it.

### Why a failed tracer can't break the agent

The [mcpd README](https://github.com/loruorg/mcpd) is direct: "It is not a gateway and not a proxy. Nothing travels through mcpd to reach a model, so mcpd being slow or down cannot slow your agent or break it." Your agent calls the model provider directly, and mcpd only receives a copy of what happened.

Recording a span never blocks, never throws and never grows without bound. If mcpd is unreachable, the agent doesn't notice.

## How does cost per step work, and what happens for an unknown model?

mcpd prices every step as its span arrives, so a cost spike on a bill or a chart traces back to one step, and a model it can't price gets no cost at all.

[Cost is priced once](https://mcpd.sh/docs/cost), at ingest, per step, from a price book of per-million-token rates. The book handles cached tokens, long-prompt tiers and service tiers.

The price is written when the span lands, so history doesn't change when rates change later.

If a span names a model the price book doesn't list, the step carries no cost, so a missing price never hides as a free call. You can supply your own price book for the models you name yourself.

When the cost monitor fires, open the project that jumped, then the model, then the most expensive run, and read the step whose token usage grew.

A looping agent repeats the same tool step while the prompt gets longer on each model call.

For a limit on each run, the `cost_budget` scorer is one of the deterministic scorers covered in the [testing guide](https://mcpd.sh/docs/testing). It never calls a model.

The cost monitor watches the total across runs. [mcpd evaluates monitors](https://mcpd.sh/docs/alerting) every 60 seconds, on `error_rate`, `latency_p95`, cost, `score_mean`, `trace_count` and step-count divergence.

A monitor latches and fires once, so a spike that lasts an hour gives you one notification, not 60. It calls your webhook when it fires.

A monitor resolves only on a real reading. Silence is not recovery: if the agent stops sending traces, the monitor can't read a healthy value, so it doesn't declare the problem over.

## Frequently asked questions

### How do I start tracing AI agents for free?

Sign up at mcpd.sh/app, create a project key, and set the two exporter variables in your agent's environment. Add the key as mcpd's OpenTelemetry documentation shows. Run the agent once and the trace appears in your project, with no package installed.

### Do I need an SDK or to change my framework?

You need no SDK and no framework change. Any agent that already exports OpenTelemetry spans sends them to mcpd once its exporter reads those two environment variables. Spans that carry `gen_ai.*` attributes show up as model calls, and mcpd's [OpenTelemetry guide](https://mcpd.sh/docs/opentelemetry) lists the supported conventions.

### Can mcpd slow down or break my agent?

It can't, because mcpd sits beside your agent, not in front of it. As the [mcpd README](https://github.com/loruorg/mcpd) states, if the daemon is slow or down, your agent's model calls run the same way.

### Is my prompt data sent anywhere by default?

Your agent sends spans only to the address in OTEL_EXPORTER_OTLP_ENDPOINT. Set it to https://api.mcpd.sh and the spans, prompts included, go to your project there. Build from source at github.com/loruorg/mcpd and point the exporter at your own machine to keep them local.

### Can I move my traces to another tool later?

Yes. mcpd forwards what it stores as OTLP to any endpoint you name, so another OpenTelemetry-compatible tool can receive the same spans. Your agent's instrumentation doesn't change, because it only ever spoke OpenTelemetry.

## What to do in your first five minutes, first hour and first week

You can start tracing AI agents free in one sitting, then add depth as the agent earns more of your attention.

- In the first five minutes, sign up at mcpd.sh/app, set `OTEL_EXPORTER_OTLP_ENDPOINT` and `OTEL_EXPORTER_OTLP_PROTOCOL` with your project key, run the agent once and open the first run.
- In the first hour, search for a failing run by a phrase the model said and read the payload of the span where it went wrong. Set a redaction mode before real user data arrives.
- In the first week, add a cost monitor so a spike points to its run, and a deterministic scorer that catches a looping tool call. Pass `traceparent` through your queue so the worker's spans join the same trace as the API request.

The way you start tracing AI agents free adds no package to your agent, so removing the two variables undoes it. The [quickstart](https://mcpd.sh/docs/quickstart) walks through the first run in more detail.

## Sources

[mcpd.sh](https://mcpd.sh) · [mcpd README](https://github.com/loruorg/mcpd) · [Cost · mcpd](https://mcpd.sh/docs/cost) · [Tracking distributed tracing for AI agents: how to follow…](https://www.braintrust.dev/articles/distributed-tracing-ai-agents) · [Privacy · mcpd](https://mcpd.sh/docs/privacy) · [OpenTelemetry · mcpd](https://mcpd.sh/docs/opentelemetry) · [Evaluation · mcpd](https://mcpd.sh/docs/evaluation) · [Alerting · mcpd](https://mcpd.sh/docs/alerting). Every other source is linked where the article cites it.
