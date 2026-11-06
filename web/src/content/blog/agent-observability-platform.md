---
description: What is an open source agent observability platform? It records each run as a tree of steps with prompt, completion and cost. Start with stock OpenTelemetry.
faq:
  - answer: mcpd does not sit in the request path. It runs as a daemon, so your agent calls the model provider directly and sends telemetry to mcpd on the side. Nothing travels through mcpd to reach a model, so if mcpd is slow or down the agent is not slowed or broken.
    question: Does an agent observability platform sit in the request path?
  - answer: You can keep the OpenInference or gen_ai instrumentation you already have exactly as it is. mcpd reads both the OpenTelemetry gen_ai.* attributes and the OpenInference llm.* attributes, so you change only where the exporter sends data and the instrumentation code stays unchanged.
    question: Can I use the OpenInference or gen_ai instrumentation I already have?
  - answer: You need neither Postgres nor a cluster to try mcpd. It stores everything in a single SQLite file by default, so a laptop run has no database to stand up. A team moves to Postgres or ClickHouse later by changing its configuration.
    question: Do I need Postgres or a cluster to try it?
  - answer: You can send your data on to another tool later. mcpd forwards what it stores as OTLP to any endpoint you name, so it is one hop in a pipeline you already have. Leaving mcpd later costs only a configuration change.
    question: Can I send my data on to another tool later?
image: /blog/agent-observability-platform/featured-6f493a02ef92.webp
image_alt: "Illustration for Open source agent observability platform: No SDK"
publishDate: "2026-11-06"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/agent-observability-platform#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\",\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q43399271\",\"@type\":\"Thing\",\"description\":\"platform for data analytics and monitoring\",\"name\":\"Grafana\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Grafana\",\"https://www.wikidata.org/wiki/Q43399271\",\"https://www.google.com/search?kgmid=/g/11fy132gmf\"]}],\"datePublished\":\"2026-11-06\",\"description\":\"What is an open source agent observability platform? It records each run as a tree of steps with prompt, completion and cost. Start with stock OpenTelemetry.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"mcpd does not sit in the request path. It runs as a daemon, so your agent calls the model provider directly and sends telemetry to mcpd on the side. Nothing travels through mcpd to reach a model, so if mcpd is slow or down the agent is not slowed or broken.\"},\"name\":\"Does an agent observability platform sit in the request path?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You can keep the OpenInference or gen_ai instrumentation you already have exactly as it is. mcpd reads both the OpenTelemetry gen_ai.* attributes and the OpenInference llm.* attributes, so you change only where the exporter sends data and the instrumentation code stays unchanged.\"},\"name\":\"Can I use the OpenInference or gen_ai instrumentation I already have?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You need neither Postgres nor a cluster to try mcpd. It stores everything in a single SQLite file by default, so a laptop run has no database to stand up. A team moves to Postgres or ClickHouse later by changing its configuration.\"},\"name\":\"Do I need Postgres or a cluster to try it?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You can send your data on to another tool later. mcpd forwards what it stores as OTLP to any endpoint you name, so it is one hop in a pipeline you already have. Leaving mcpd later costs only a configuration change.\"},\"name\":\"Can I send my data on to another tool later?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q16248637\",\"@type\":\"Organization\",\"description\":\"US IT company\",\"name\":\"Datadog\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Datadog\",\"https://www.wikidata.org/wiki/Q16248637\",\"https://www.google.com/search?kgmid=/m/0wfvvyq\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q27825826\",\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q22661306\",\"@type\":\"Thing\",\"description\":\"software to manage containers on a server-cluster\",\"name\":\"Kubernetes\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Kubernetes\",\"https://www.wikidata.org/wiki/Q22661306\",\"https://www.google.com/search?kgmid=/g/11b7lxp79d\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q141164091\",\"@type\":\"Organization\",\"description\":\"American artificial intelligence software company\",\"name\":\"Braintrust\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Braintrust\",\"https://www.wikidata.org/wiki/Q141164091\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q116758847\",\"@type\":\"Organization\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q118876059\",\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]}],\"name\":\"Open source agent observability platform: No SDK\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/VSukSuXLdzmGCry6g4WY2mjDCjZPPbDj.webp\"},\"url\":\"https://mcpd.sh/blog/agent-observability-platform\"},{\"@id\":\"https://mcpd.sh/blog/agent-observability-platform#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\"},{\"@id\":\"https://www.wikidata.org/wiki/Q43399271\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory.\",\"url\":\"https://mcpd.sh\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd README\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"Cost · mcpd\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"Alerting · mcpd\",\"url\":\"https://mcpd.sh/docs/alerting\"},{\"@type\":\"CreativeWork\",\"name\":\"Evaluation · mcpd\",\"url\":\"https://mcpd.sh/docs/evaluation\"},{\"@type\":\"CreativeWork\",\"name\":\"Trace Context, W3C Recommendation\",\"url\":\"https://www.w3.org/TR/trace-context/\"},{\"@type\":\"CreativeWork\",\"name\":\"Models overview, Anthropic\",\"url\":\"https://docs.anthropic.com/en/docs/about-claude/models/overview\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry Protocol Exporter\",\"url\":\"https://opentelemetry.io/docs/specs/otel/protocol/exporter/\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry Protocol (OTLP) Specification\",\"url\":\"https://opentelemetry.io/docs/specs/otlp/\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenInference Semantic Conventions\",\"url\":\"https://arize-ai.github.io/openinference/spec/semantic_conventions.html\"},{\"@type\":\"CreativeWork\",\"name\":\"Apache License, Version 2.0\",\"url\":\"https://www.apache.org/licenses/LICENSE-2.0\"},{\"@type\":\"CreativeWork\",\"name\":\"Testing · mcpd\",\"url\":\"https://mcpd.sh/docs/testing\"},{\"@type\":\"CreativeWork\",\"name\":\"Security · mcpd\",\"url\":\"https://mcpd.sh/docs/security\"},{\"@type\":\"CreativeWork\",\"name\":\"Privacy · mcpd\",\"url\":\"https://mcpd.sh/docs/privacy\"},{\"@type\":\"CreativeWork\",\"name\":\"Relay and regions · mcpd\",\"url\":\"https://mcpd.sh/docs/relay\"}],\"dateModified\":\"2026-11-06\",\"datePublished\":\"2026-11-06\",\"description\":\"What is an open source agent observability platform? It records each run as a tree of steps with prompt, completion and cost. Start with stock OpenTelemetry.\",\"headline\":\"Open source agent observability platform: No SDK\",\"image\":\"https://api.birchseek.com/m/VSukSuXLdzmGCry6g4WY2mjDCjZPPbDj.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/agent-observability-platform#webpage\"},\"keywords\":[\"open source agent observability platform\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/agent-observability-platform#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q16248637\"},{\"@id\":\"https://www.wikidata.org/wiki/Q27825826\"},{\"@id\":\"https://www.wikidata.org/wiki/Q22661306\"},{\"@id\":\"https://www.wikidata.org/wiki/Q141164091\"},{\"@id\":\"https://www.wikidata.org/wiki/Q116758847\"},{\"@id\":\"https://www.wikidata.org/wiki/Q118876059\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2464},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]},{\"@type\":\"Thing\",\"description\":\"defined sequence of activities of an organizational entity, enabled by the systematic organization of resources into processes that transform materials, provide services, or process information\",\"name\":\"Workflow\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Workflow\",\"https://www.wikidata.org/wiki/Q627335\",\"https://www.google.com/search?kgmid=/m/0k5rb\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"Chinese artificial intelligence company\",\"name\":\"DeepSeek\",\"sameAs\":[\"https://en.wikipedia.org/wiki/DeepSeek\",\"https://www.wikidata.org/wiki/Q131577453\",\"https://www.google.com/search?kgmid=/g/11wvrb0s91\"]},{\"@type\":\"Thing\",\"description\":\"collection of data, publication type\",\"name\":\"Data set\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Data_set\",\"https://www.wikidata.org/wiki/Q1172284\",\"https://www.google.com/search?kgmid=/m/02cck\"]},{\"@type\":\"Thing\",\"description\":\"maximum number of tokens that a large language model can process at one time\",\"name\":\"Context window\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Context_window\",\"https://www.wikidata.org/wiki/Q125619872\"]},{\"@type\":\"Thing\",\"description\":\"benchmark in natural language understanding\",\"name\":\"General Language Understanding Evaluation benchmark\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q64711428\"]},{\"@type\":\"Thing\",\"description\":\"chatbot developed by SpaceXAI\",\"name\":\"Grok\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Grok_(chatbot)\",\"https://www.wikidata.org/wiki/Q123361035\",\"https://www.google.com/search?kgmid=/g/11l6lp1dv3\"]},{\"@type\":\"Thing\",\"description\":\"chatbot\",\"name\":\"Kimi\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Kimi_(AI)\",\"https://www.wikidata.org/wiki/Q131993396\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"family of large language models developed by OpenAI\",\"name\":\"GPT-5\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GPT-5\",\"https://www.wikidata.org/wiki/Q135297698\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - agent observability
  - opentelemetry
  - open source
  - llm cost tracking
  - self-hosting
  - agent evaluation
title: "Open source agent observability platform: No SDK"
---

**An open source agent observability platform records each agent run as a tree of model calls, tool calls and retrievals, with the exact prompt, completion and cost of every step, as the [mcpd home page](https://mcpd.sh) describes it. According to the [mcpd README](https://github.com/loruorg/mcpd), mcpd is one: a daemon with nothing to adopt, not a proxy. It works with any framework, language and model provider, and nothing travels through it to reach a model.**

Your agent works on your laptop, then misbehaves in production. One bad answer is buried in thousands of runs, and nobody can attribute the bill to a step.

After reading, you can send a first trace with stock OpenTelemetry packages and read each step's prompt and completion. You can also price a step, find one bad run, and decide what to operate, solo or as a platform team.

> **Key Takeaways**
> - Setup is an OpenTelemetry exporter, and [mcpd reads](https://mcpd.sh/docs/opentelemetry) `gen_ai.*` and `llm.*`.
> - [Cost is priced](https://mcpd.sh/docs/cost) once per step at ingest; an unknown model has no cost, not zero.
> - [Monitors](https://mcpd.sh/docs/alerting), evaluated every 60 seconds, latch and resolve only on a real reading. [Scores and feedback](https://mcpd.sh/docs/evaluation) are never averaged.

## What is an open source agent observability platform, and how does it differ from logs and traces?

Agent observability records each agent run as a tree of model calls, tool calls and retrievals, storing the exact prompt and completion of every step with its latency, tokens and cost. Logs, metrics and traces show that a request was slow or failed. The payload shows why the agent chose what it did.

Infrastructure observability asks whether a service is up and fast. A normal trace can say a request returned 200 in 40 ms, yet the wrong answer lives inside the payload.

mcpd's [concepts guide](https://mcpd.sh/docs/concepts) defines the vocabulary. The [mcpd home page](https://mcpd.sh) puts the shape plainly: a run is a tree of model calls, tool calls and retrievals, and you open any step to read the exact prompt and answer.

A refund agent that calls an order-lookup tool again and again shows the difference. Every HTTP request returns 200, the logs show healthy lookups and the latency chart is flat. The span tree shows the loop:

```text
refund_agent (root span)
  chat
  tool lookup_order   200
  chat
  tool lookup_order   200
  tool lookup_order   200
```

Open the second `chat` span: the prompt holds the lookup result, and the completion asks for the same lookup again.

Two standards make the tree portable. The [W3C Trace Context specification](https://www.w3.org/TR/trace-context/) says the `traceparent` HTTP header "identifies the incoming request in a tracing system". That header is how one trace id and a parent span id follow a request from your API into the agent service and on to its tools.

mcpd reads `gen_ai.*` and the OpenInference `llm.*` names, so existing instrumentation works unchanged, as the [mcpd OpenTelemetry docs](https://mcpd.sh/docs/opentelemetry) state.

## How do you set up an open source agent observability platform without an SDK?

With an open source agent observability platform, setup takes four steps.

The [OpenTelemetry exporter specification](https://opentelemetry.io/docs/specs/otel/protocol/exporter/) names `OTEL_EXPORTER_OTLP_ENDPOINT` and `OTEL_EXPORTER_OTLP_PROTOCOL` as the standard variables. The [OTLP specification](https://opentelemetry.io/docs/specs/otlp/) defines both gRPC and HTTP transports. Any exporter that follows the specification reads them.

1. Install the stock OpenTelemetry packages. No mcpd package is needed.

```shell
pip install opentelemetry-sdk opentelemetry-exporter-otlp-proto-http
```

2. Create a project and a key at the hosted app, then set the two standard variables in your agent's environment. The mcpd [OpenTelemetry docs page](https://mcpd.sh/docs/opentelemetry) gives the endpoint, the protocol value and how the key is passed. Copy them from there: a wrongly passed key makes the export fail and leaves the interface empty.

3. Wrap the model call in a span that carries the `gen_ai.*` attributes. Take the model identifier from your provider's documentation; [Anthropic's model overview](https://docs.anthropic.com/en/docs/about-claude/models/overview) lists `claude-sonnet-4-5`.

```python
from opentelemetry import trace
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import BatchSpanProcessor
from opentelemetry.exporter.otlp.proto.http.trace_exporter import OTLPSpanExporter

provider = TracerProvider()
provider.add_span_processor(BatchSpanProcessor(OTLPSpanExporter()))
trace.set_tracer_provider(provider)
tracer = trace.get_tracer("my-agent")

with tracer.start_as_current_span("chat claude-sonnet-4-5") as span:
    span.set_attribute("gen_ai.operation.name", "chat")
    span.set_attribute("gen_ai.request.model", "claude-sonnet-4-5")
    # call the model here, then record what came back
    span.set_attribute("gen_ai.response.model", "claude-sonnet-4-5")
    span.set_attribute("gen_ai.usage.input_tokens", 1200)
    span.set_attribute("gen_ai.usage.output_tokens", 85)

provider.shutdown()
```

4. Run the script and open the project. You should see one run with a single model-call step, its model names and its token counts. Keep the `provider.shutdown()` call in short scripts: the batch processor sends spans in the background, and a script that exits first sends nothing.

The [OpenInference specification](https://arize-ai.github.io/openinference/spec/semantic_conventions.html) publishes `llm.*` attributes such as `llm.model_name` as a separate convention.

The quickest route is the hosted version at mcpd.sh/app, and [Start free](https://mcpd.sh/app) opens it.

To self-host, build from source at github.com/loruorg/mcpd, following the [mcpd README](https://github.com/loruorg/mcpd). You need Rust and Node, then run `make build`. Node is required because the interface is compiled into the binary.

```shell
make build
./target/release/mcpd demo
```

Open http://localhost:4317 to see the demo data. The [quickstart](https://mcpd.sh/docs/quickstart) walks through sending a trace from your own agent.

## What does open source cover in an agent observability platform?

Open source in an agent observability platform covers three things you can check one at a time: the licence, the features in the open build, and what you operate to get the first trace.

### Which licence applies?

mcpd is Apache-2.0. The [Apache License 2.0 text](https://www.apache.org/licenses/LICENSE-2.0) on apache.org grants each user "a perpetual, worldwide, non-exclusive, no-charge, royalty-free, irrevocable copyright license to reproduce, prepare Derivative Works of, publicly display, publicly perform, sublicense, and distribute the Work".

You're free to run it, change it and redistribute your changes.

### Which features are in the open build?

Every feature is in the open build. The hosted version and the self-hosted build are the same product, with no feature gates.

Enterprise capability is configuration on the same artefact, so a solo engineer on a laptop and a platform team run identical code.

### What do you operate?

[mcpd is one static Rust binary](https://github.com/loruorg/mcpd) with the storage and the web interface built in. Storage is a choice of four:

- One SQLite file, the default.
- `:memory:`, which keeps nothing.
- Postgres, for a team.
- ClickHouse, for a team.

Orchestrators can probe `/healthz` and `/readyz`. The [operations guide](https://mcpd.sh/docs/operations) covers running mcpd on your own infrastructure.

### Is it open at every seam?

Data comes in as OpenTelemetry and goes out the same way. The mcpd README says "What it stores it also forwards, as OTLP, to any endpoint you name".

Forwarding presets cover Langfuse, LangSmith, Braintrust, Arize, Phoenix, Datadog, Honeycomb, Grafana and others.

## How does an open source agent observability platform find one bad run among thousands?

You find one bad agent run among thousands by starting from the complaint, searching what the model said, and following trace ids and metric exemplars to the exact run.

Two complaints show the path. A user reports that the refund agent refused a valid refund, and the monthly bill jumps with no step to blame.

1. Search the payload. Paste a phrase from the agent's reply into search. [mcpd's home page](https://mcpd.sh) says "Search finds a run by what the model said", so a sentence like "I can't process that refund" finds the run directly. Filtering by user or model name alone returns thousands of runs.
2. Open the waterfall and the conversation. For the refused refund, the same refund-lookup tool-call span repeats down the waterfall until the step limit, and the conversation shows each identical tool result. The [runs guide](https://mcpd.sh/docs/runs) covers tracing a failing step.
3. Follow the log line to its span. Logs and traces share a trace id, so every log line links to the step that wrote it. Start from the log your tool printed and land on the exact span, with its prompt and answer.
4. Follow a metric exemplar back to a run. For the bill, start from the `cost` metric. An exemplar on the spike links to one run, where the per-step cost shows a prompt growing on every turn.
5. Turn the finding into a scorer. With `no_tool_loop` running, the refund loop no longer waits for a user complaint to surface.
6. Add monitors. [mcpd evaluates monitors](https://mcpd.sh/docs/alerting) every 60 seconds on `error_rate`, `latency_p95`, `cost`, `score_mean`, `trace_count` and step-count divergence. Divergence catches a run that takes far more steps than its peers.

A monitor latches. It fires once, calls your webhook, and resolves only on a real reading.

Silence isn't recovery: if traffic stops and no reading arrives, the monitor stays fired. Without latching, a quiet hour would clear an alert for a problem that's still there.

## Why does an open source agent observability platform keep scores and feedback separate?

Scores and feedback stay separate because they answer different questions. A score is a machine evaluation measured against a declared scale, such as a deterministic check for tool loops or a cost budget. Feedback is a person's judgement of a run.

[mcpd keeps them](https://mcpd.sh/docs/evaluation) as two signals and never averages one into the other.

An average hides which side was wrong. If a refund run passes `no_tool_loop` with 1.0 and a reviewer marks it wrong, a mean of 0.5 can't tell you whether the check caught the failure or missed it.

A score without a declared scale has a second problem. A value of 0.8 could mean 8 out of 10 or 80% of a budget, so you can't compare it across runs.

Three kinds of scorer produce scores. Deterministic scorers run fixed checks, a model can act as the judge when the question needs reading, and you can define your own scorer for rules only you know.

People work through a review queue, which records feedback beside the scores.

When a production run fails, it can become a case in a dataset, and an experiment then runs two versions of your agent against those cases so you can compare them. The [testing guide](https://mcpd.sh/docs/testing) covers that loop.

## How is the cost of each agent step calculated?

Here's how an open source agent observability platform prices each step.

A step with 12,000 input tokens and 400 output tokens is priced as 12,000 times the input rate plus 400 times the output rate, each divided by a million.

Input tokens don't all cost the same.

Service tiers such as batch or priority processing add another rate. The price book holds these variants, so a step that read half its prompt from cache isn't billed at the full input rate.

If a model name is new, misspelt or served by a gateway under an alias, the step shows its cost as missing.

A zero would read as a free step and drag your totals down without a warning. A missing price stays visible until you add a rate.

To add that rate, supply your own price book. The interface then shows cost per run, per model and per project, so a spike narrows from the month to the model to the run that caused it.

The `cost_budget` scorer checks each finished run against a limit you set. The `cost` monitor watches spend and calls your webhook when it crosses the line.

## How do you keep agent traces private and auditable on your own network?

You keep agent traces private and auditable on your own network with seven controls, and each one closes a gap an auditor will ask about. The [mcpd security documentation](https://mcpd.sh/docs/security) covers them in detail.

1. Redaction runs before storage. As [mcpd's privacy docs](https://mcpd.sh/docs/privacy) put it, without redaction a secret you delete later was still written down.
2. Retention and erasure limit what stays. Retention is set in days, and erasure requests target a person. Without them, prompts that contain personal data stay for as long as the disk does.
3. Nothing leaves by default. With a default configuration there is no update check, no usage reporting and no telemetry, so a firewall review has no traffic you didn't choose to account for.
4. Access uses API keys with six scopes: ingest, read, write, execute, worker and admin. Keys are compared as SHA-256 hashes, so the stored value isn't a usable key. If you set no keys, ingest and reads are open, which is for a laptop only.
5. Accounts, roles and an audit log answer "who saw what".
6. Sampling reduces what you store. Every stored run is a payload you have to protect, so storing fewer of them shrinks that surface.
7. A relay keeps traffic local. You run a relay as a collector beside the agents in each region, and it spools ingest and ships it to the daemons in order. Agents send to a nearby address instead of crossing a network boundary to reach a single daemon. The [relay and regions guide](https://mcpd.sh/docs/relay) covers the setup.

A laptop and a Kubernetes cluster run the same binary. These team-scale controls are configuration, and storage moves from one SQLite file to Postgres or ClickHouse the same way.

## Frequently asked questions

### Does an agent observability platform sit in the request path?

No. mcpd is a daemon, not a gateway or proxy: your agent calls the model provider directly and sends telemetry to mcpd on the side. Nothing travels through mcpd to reach a model, so if mcpd is slow or down the agent is not slowed or broken.

### Can I use the OpenInference or gen_ai instrumentation I already have?

You can keep it as it is. mcpd reads both the OpenTelemetry gen_ai.* attributes and the OpenInference llm.* attributes, so you change only where the exporter sends data.

### Do I need Postgres or a cluster to try it?

You need neither. mcpd stores everything in a single SQLite file by default, so a laptop run has no database to stand up. A team moves to Postgres or ClickHouse by changing configuration.

### Can I send my data on to another tool later?

You can. mcpd forwards what it stores as OTLP to any endpoint you name, so it is one hop in a pipeline you already have. Leaving later takes a configuration change.

## What should you do next?

Start small with your open source agent observability platform, and let your first traces tell you what's worth building next.

**In the next 5 minutes:** set the two OTLP environment variables on an agent you already run and send one trace. Open the run and read the exact prompt and completion of each step. The [quickstart](https://mcpd.sh/docs/quickstart) walks through it.

**Today:** add a `no_tool_loop` or `cost_budget` scorer and one `cost` monitor. The next looping run or cost spike then arrives as a webhook with the run attached.

**This week:** decide whether a single SQLite file is enough for you, or whether your team needs redaction, retention and accounts before more people read the payloads.

[Start free](https://mcpd.sh/app) on the hosted version, and build from source later if you'd rather run it yourself.

## Sources

[mcpd · The unopinionated agent observatory.](https://mcpd.sh) · [mcpd README](https://github.com/loruorg/mcpd) · [OpenTelemetry · mcpd](https://mcpd.sh/docs/opentelemetry) · [Cost · mcpd](https://mcpd.sh/docs/cost) · [Alerting · mcpd](https://mcpd.sh/docs/alerting) · [Evaluation · mcpd](https://mcpd.sh/docs/evaluation) · [Trace Context, W3C Recommendation](https://www.w3.org/TR/trace-context/) · [Security · mcpd](https://mcpd.sh/docs/security). Every other source is linked where the article cites it.
