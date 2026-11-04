---
description: How do you run self hosted LLM tracing on Postgres? Run one binary, set its storage to Postgres and send OpenTelemetry spans. Learn the setup and start free.
faq:
  - answer: Self-hosted LLM tracing on Postgres means a daemon that receives OpenTelemetry spans from your agent and stores prompts, completions, timings and cost in a Postgres database you run. With mcpd it is one binary plus Postgres, chosen through the storage setting, with the interface built in.
    question: What is self-hosted LLM tracing on Postgres?
  - answer: You can trace an agent without a vendor SDK, because mcpd reads standard OpenTelemetry. On the exporter your agent already uses, set OTEL_EXPORTER_OTLP_ENDPOINT and OTEL_EXPORTER_OTLP_PROTOCOL. After that, any spans carrying gen_ai.* or OpenInference llm.* attributes arrive in mcpd as runs.
    question: Can I trace an agent without a vendor SDK?
  - answer: "You do not need ClickHouse, Redis or S3 to self-host mcpd, because it requires only the daemon and Postgres. By comparison, Langfuse's production stack runs web, worker, Postgres and ClickHouse, so a Postgres-only store like mcpd's leaves one database to back up."
    question: Do I need ClickHouse, Redis or S3 to self-host mcpd?
  - answer: "Tracing does not slow down or break your agent when the daemon is down. Recording a span never blocks or throws, and nothing travels through mcpd to reach a model, so a slow or stopped daemon can't delay your agent."
    question: Does tracing slow down or break my agent if the daemon is down?
  - answer: You can forward traces to Langfuse or another backend later, because mcpd forwards what it stores as OTLP to any endpoint you name. It has presets for Langfuse, LangSmith, Datadog and others, so the Postgres-backed daemon becomes one hop in a pipeline you already have.
    question: Can I forward traces to Langfuse or another backend later?
image: /blog/self-hosted-llm-tracing-postgres/featured-a75e09923cd1.webp
image_alt: "Illustration for Self hosted LLM tracing on Postgres: One daemon"
publishDate: "2026-11-04"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/self-hosted-llm-tracing-postgres#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"datePublished\":\"2026-11-04\",\"description\":\"How do you run self hosted LLM tracing on Postgres? Run one binary, set its storage to Postgres and send OpenTelemetry spans. Learn the setup and start free.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Self-hosted LLM tracing on Postgres means a daemon that receives OpenTelemetry spans from your agent and stores prompts, completions, timings and cost in a Postgres database you run. With mcpd it is one binary plus Postgres, chosen through the storage setting, with the interface built in.\"},\"name\":\"What is self-hosted LLM tracing on Postgres?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You can trace an agent without a vendor SDK, because mcpd reads standard OpenTelemetry. On the exporter your agent already uses, set OTEL_EXPORTER_OTLP_ENDPOINT and OTEL_EXPORTER_OTLP_PROTOCOL. After that, any spans carrying gen_ai.* or OpenInference llm.* attributes arrive in mcpd as runs.\"},\"name\":\"Can I trace an agent without a vendor SDK?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You do not need ClickHouse, Redis or S3 to self-host mcpd, because it requires only the daemon and Postgres. By comparison, Langfuse's production stack runs web, worker, Postgres and ClickHouse, so a Postgres-only store like mcpd's leaves one database to back up.\"},\"name\":\"Do I need ClickHouse, Redis or S3 to self-host mcpd?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Tracing does not slow down or break your agent when the daemon is down. Recording a span never blocks or throws, and nothing travels through mcpd to reach a model, so a slow or stopped daemon can't delay your agent.\"},\"name\":\"Does tracing slow down or break my agent if the daemon is down?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You can forward traces to Langfuse or another backend later, because mcpd forwards what it stores as OTLP to any endpoint you name. It has presets for Langfuse, LangSmith, Datadog and others, so the Postgres-backed daemon becomes one hop in a pipeline you already have.\"},\"name\":\"Can I forward traces to Langfuse or another backend later?\"}],\"name\":\"Self hosted LLM tracing on Postgres: One daemon\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/I4wsgaP9ttqikI71Sh3BAQqMeP5EwgV_.webp\"},\"url\":\"https://mcpd.sh/blog/self-hosted-llm-tracing-postgres\"},{\"@id\":\"https://mcpd.sh/blog/self-hosted-llm-tracing-postgres#article\",\"@type\":\"BlogPosting\",\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"Langfuse Self-Hosted LLM Tracing (Docker Compose + Python SDK)\",\"url\":\"https://ossaihub.com/code/langfuse-self-hosted-tracing/\"},{\"@type\":\"CreativeWork\",\"name\":\"loruorg/mcpd README\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"Integrating · mcpd\",\"url\":\"https://mcpd.sh/docs/integrating\"},{\"@type\":\"CreativeWork\",\"name\":\"Self-host Langfuse (Open Source LLM Observability) - Langfuse\",\"url\":\"https://langfuse.com/self-hosting\"},{\"@type\":\"CreativeWork\",\"name\":\"Forwarding · mcpd\",\"url\":\"https://mcpd.sh/docs/forwarding\"},{\"@type\":\"CreativeWork\",\"name\":\"Configuration · mcpd\",\"url\":\"https://mcpd.sh/docs/configuration\"},{\"@type\":\"CreativeWork\",\"name\":\"PostgreSQL Documentation: Continuous Archiving and Point-in-Time Recovery (PITR)\",\"url\":\"https://www.postgresql.org/docs/current/continuous-archiving.html\"},{\"@type\":\"CreativeWork\",\"name\":\"Operations · mcpd\",\"url\":\"https://mcpd.sh/docs/operations\"},{\"@type\":\"CreativeWork\",\"name\":\"PostgreSQL Documentation: Table Partitioning\",\"url\":\"https://www.postgresql.org/docs/current/ddl-partitioning.html\"},{\"@type\":\"CreativeWork\",\"name\":\"PostgreSQL Documentation: Preferred Index Types for Text Search\",\"url\":\"https://www.postgresql.org/docs/current/textsearch-indexes.html\"},{\"@type\":\"CreativeWork\",\"name\":\"PostgreSQL Documentation: pg_dump\",\"url\":\"https://www.postgresql.org/docs/current/app-pgdump.html\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory.\",\"url\":\"https://mcpd.sh\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd privacy documentation\",\"url\":\"https://mcpd.sh/docs/privacy\"},{\"@type\":\"CreativeWork\",\"name\":\"Security · mcpd\",\"url\":\"https://mcpd.sh/docs/security\"}],\"dateModified\":\"2026-11-04\",\"datePublished\":\"2026-11-04\",\"description\":\"How do you run self hosted LLM tracing on Postgres? Run one binary, set its storage to Postgres and send OpenTelemetry spans. Learn the setup and start free.\",\"headline\":\"Self hosted LLM tracing on Postgres: One daemon\",\"image\":\"https://api.birchseek.com/m/I4wsgaP9ttqikI71Sh3BAQqMeP5EwgV_.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/self-hosted-llm-tracing-postgres#webpage\"},\"keywords\":[\"self hosted llm tracing postgres\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/self-hosted-llm-tracing-postgres#webpage\"},\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2838},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"defined sequence of activities of an organizational entity, enabled by the systematic organization of resources into processes that transform materials, provide services, or process information\",\"name\":\"Workflow\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Workflow\",\"https://www.wikidata.org/wiki/Q627335\",\"https://www.google.com/search?kgmid=/m/0k5rb\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"collection of data, publication type\",\"name\":\"Data set\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Data_set\",\"https://www.wikidata.org/wiki/Q1172284\",\"https://www.google.com/search?kgmid=/m/02cck\"]},{\"@type\":\"Thing\",\"description\":\"benchmark in natural language understanding\",\"name\":\"General Language Understanding Evaluation benchmark\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q64711428\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"large language model company\",\"name\":\"LlamaIndex\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LlamaIndex\",\"https://www.wikidata.org/wiki/Q125446413\"]},{\"@type\":\"Thing\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]},{\"@type\":\"Thing\",\"description\":\"language understanding across multiple domains\",\"name\":\"MMLU\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MMLU\",\"https://www.wikidata.org/wiki/Q123721276\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"a lightweight markup language with plain-text formatting syntax that is popularly used\",\"name\":\"Markdown\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Markdown\",\"https://www.wikidata.org/wiki/Q1193600\",\"https://www.google.com/search?kgmid=/m/0jt0gkj\"]},{\"@type\":\"Thing\",\"description\":\"artificial intelligence chatbot developed by Google\",\"name\":\"Google Gemini\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google_Gemini\",\"https://www.wikidata.org/wiki/Q116698014\",\"https://www.google.com/search?kgmid=/g/11ts49p01g\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - llm tracing
  - postgres
  - opentelemetry
  - self hosting
  - observability
  - data retention
title: "Self hosted LLM tracing on Postgres: One daemon"
---

Point any OpenTelemetry exporter at it: spans arrive over [OTLP](https://mcpd.sh/docs/opentelemetry) with `gen_ai.*` or OpenInference `llm.*` attributes, so you need no vendor SDK and no extra services beside the database you already run.**

Your agent misbehaves in production, the prompts can't leave your network, and the database you already run is Postgres. That's where self hosted LLM tracing Postgres setups fit. You want no more than one daemon and one database to operate.

You'll send a first trace from plain OpenTelemetry environment variables and store it in Postgres. You'll also set redaction and retention as configuration, and trace a user's complaint to the one run that caused it.

> **Key Takeaways**
> - On Postgres, mcpd adds one process and no second database, so your existing backup routine covers the traces.
> - [mcpd's privacy documentation](https://mcpd.sh/docs/privacy) puts redaction before the write, so set it before real prompts arrive.
> - The [mcpd README](https://github.com/loruorg/mcpd) places mcpd beside the agent, not in front of the model, so a stopped daemon cannot stall a call.
> - OTLP forwarding keeps mcpd one hop in your pipeline, so moving to another backend is a configuration change.

## How do you send a first LLM trace with only OpenTelemetry env vars?

You send a first LLM trace by pointing the OpenTelemetry exporter your agent already uses at mcpd with two environment variables, so no vendor SDK enters your code.

![Illustration for How do you send a first LLM trace with only OpenTelemetry env vars?](/blog/self-hosted-llm-tracing-postgres/image-1-7f87f2c16352.webp)

1. Pick one of the three start routes that work today:
 - The hosted version: sign up at [mcpd.sh/app](https://mcpd.sh/app), create a project and copy its key.
 - The OTLP endpoint `https://api.mcpd.sh`, which the hosted version uses.
 - The source at github.com/loruorg/mcpd: run `make build`, then `./target/release/mcpd demo`, and open http://localhost:4317. Rust and Node are both needed, because the interface is compiled into the binary. `make dist` gives the optimised build.

2. Export the two variables in the shell that starts your agent:

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
python agent.py   # any agent that already exports OpenTelemetry spans
```

3. Pass the project key through the standard `OTEL_EXPORTER_OTLP_HEADERS` variable, using the exact header name given in the [mcpd OpenTelemetry docs](https://mcpd.sh/docs/opentelemetry). If you self-host, set the endpoint to the address your daemon listens on instead.

4. Run the agent once and open the interface. The run appears as a span tree. Open a step to read the prompt, the completion, the tokens and the cost.

If the run doesn't show up, check that the protocol matches what you exported. The daemon takes OTLP over HTTP (JSON and protobuf) and gRPC, plus Zipkin and Jaeger, as the OpenTelemetry docs list.

mcpd reads the OpenTelemetry `gen_ai.*` attributes and the OpenInference `llm.*` attributes, so existing instrumentation works unchanged.

Pin your instrumentation library and read its changelog before you upgrade.

## Why a self hosted LLM tracing Postgres route is safe for the agent

The OpenTelemetry route is safe for the agent because recording sits outside the request path. [mcpd's integration documentation](https://mcpd.sh/docs/integrating) says a recorded span never blocks, never throws and never grows without bound, so an unreachable daemon goes unnoticed by the agent.

A proxy or gateway works differently, because every model call passes through it. The [mcpd README](https://github.com/loruorg/mcpd) states that nothing travels through mcpd to reach a model, so mcpd being slow or down cannot slow your agent or break it.

The [quickstart](https://mcpd.sh/docs/quickstart) covers the same first trace step by step. To skip the build, [start free on the hosted version](https://mcpd.sh/app).

## What does a self hosted LLM tracing Postgres setup need to run?

Self hosted LLM tracing needs a store for spans and payloads plus the processes that write to it: one daemon for mcpd with Postgres, and web, worker and two databases for Langfuse in production.

| Criterion | mcpd with Postgres | Langfuse on Docker Compose | Langfuse on Kubernetes |
| --- | --- | --- | --- |
| Application processes | One daemon | Web and worker | Web and worker |
| Databases holding traces | Postgres | Postgres and ClickHouse | Postgres and ClickHouse |
| Documented use | Single node or team | Local use and testing | Production |
| Backups and scaling | Your Postgres routine | None on a single VM | Your infrastructure |

A trace store must hold spans with nesting and timings, prompt and completion payloads, and tokens and cost per step. It also links logs and metrics by trace id. The routes differ in how many processes and databases carry that load.

[Langfuse's self-hosting overview](https://langfuse.com/self-hosting) describes Docker Compose as the route for local use and testing, on a single VM without high availability, scaling or backups. For production it lists Kubernetes (Helm) and cloud-specific guides, where the infrastructure is yours.

A [walkthrough of the production stack](https://ossaihub.com/code/langfuse-self-hosted-tracing/) names its parts as web, worker, Postgres and ClickHouse, so Postgres is one of two databases there.

[mcpd's configuration documentation](https://mcpd.sh/docs/configuration) describes one static binary with the storage and the interface built in. Its storage is SQLite by default, and you set it to Postgres or ClickHouse for a team. With Postgres, the daemon is the only process you add.

For a team that already runs a managed Postgres instance, Langfuse's production stack adds a web container, a worker and ClickHouse. ClickHouse is a second database to back up and upgrade.

mcpd adds one process, with its storage setting pointed at the Postgres connection string. The team's existing backup routine then covers the traces too.

Leaving is also a configuration change. [mcpd's forwarding docs](https://mcpd.sh/docs/forwarding) say it forwards what it stores as OTLP to any endpoint you name, with presets for Langfuse, LangSmith, Braintrust, Arize, Phoenix, Datadog, Honeycomb and others. The daemon then sits as one hop in a pipeline you already run.

Best for:

- mcpd with Postgres: teams that already back up Postgres and want one process added to it.
- Langfuse on Docker Compose: a trial on one machine, where losing the data is acceptable.
- Langfuse on Kubernetes: teams that already operate a cluster and accept running web, worker and two databases.

## Is self hosted LLM tracing Postgres storage enough, and when do you move to ClickHouse?

Postgres is enough when one team writes traces to a single database you already back up, and you search payloads with full-text indexes. Move to ClickHouse when aggregates over very large trace volumes become the main workload.

[mcpd's configuration](https://mcpd.sh/docs/configuration) supports both through its storage setting, so the choice stays a configuration change.

Size the database from three numbers: the bytes in one prompt plus its completion, the runs per day, and the retention in days. Multiply them, then add room for indexes.

Measure your own payloads first. An agent that pastes retrieved documents into every prompt writes far more per run than one that sends short tool arguments.

Retention keeps that product flat. On a large table, declarative partitioning by time lets you remove old data by dropping a partition instead of deleting rows. [PostgreSQL's partitioning documentation](https://www.postgresql.org/docs/current/ddl-partitioning.html) says "Bulk loads and deletes can be accomplished by adding or removing partitions".

Finding the bad run means searching what the model said. A GIN index over a full-text search column does that in Postgres. [PostgreSQL's text search documentation](https://www.postgresql.org/docs/current/textsearch-indexes.html) names GIN as one of the index types that speed up full text searches.

Back up in two layers:

- Logical backup: the [PostgreSQL documentation for pg_dump](https://www.postgresql.org/docs/current/app-pgdump.html) describes `pg_dump` as "a utility for backing up a PostgreSQL database". Use it for a portable copy you can restore into a fresh instance.
- Point-in-time recovery: [continuous archiving](https://www.postgresql.org/docs/current/continuous-archiving.html) keeps a base backup plus the write-ahead log, so you can restore to the minute before a bad migration.

Traces contain prompts, so test a restore into a scratch database on a schedule.

The [mcpd operations documentation](https://mcpd.sh/docs/operations) lists `/healthz` and `/readyz` endpoints for your orchestrator's probes, and the daemon shuts down gracefully on SIGTERM. A rolling restart therefore doesn't cut a write in half.

These signs show that analytical queries belong on ClickHouse:

1. Your most frequent queries scan weeks of spans to compute cost, latency or error aggregates.
2. Those scans slow ingest or the interface for everyone else.
3. The table is big enough that dumps and restores no longer fit your recovery window.

mcpd documents ClickHouse as its other store for a team, next to Postgres. Because the store is a setting, you can start on the database you already run and move the analytical load later without changing your exporters.

## How do you find the one bad run from a user complaint?

You find the one bad run by searching for what the model said, opening its span tree, and reading the prompt, the completion and the cost in that order. Take a support ticket saying the agent "kept calling the refund tool".

1. Search for the words, not a label. Paste a phrase from the complaint, or from the reply the user quoted, into the search box. [mcpd's home page](https://mcpd.sh) says that "Search finds a run by what the model said, and every log line links back to the step that wrote it."

2. Don't search on user id or environment alone. That misses runs where the identifying text lives in the completion.

3. Open the run and read the waterfall. Every step sits on one clock. A looping run shows the same tool-call span repeated under a single root span. The [runs documentation](https://mcpd.sh/docs/runs) shows how steps and failures appear in a run.

4. Read the prompt and the completion on the repeating span. The home page says you can "read the exact prompt and answer, the tokens it used and what it cost" by opening a step.

5. Check where those fields come from. The model, token and message fields come from `gen_ai.*` attributes such as `gen_ai.request.model` and `gen_ai.usage.input_tokens`. OpenInference attributes such as `llm.model_name` and `llm.token_count.prompt` fill the same fields. If each tool result tells the model the refund is pending, the model asks again, and that explains the loop.

6. Confirm with the deterministic scorers. `no_tool_loop`, `tool_efficiency` and `cost_budget` never call a model, so they flag the same run every time you re-score it. A failed `no_tool_loop` on this run confirms what you saw in the tree.

7. Check the cost of the run. Cost is priced once at ingest, per step, from a price book of per-million-token rates.

8. Read a blank cost cell carefully. An unknown model has no cost, not a cost of zero. A blank cost cell means the model is missing from the price book, and the run was not free.

9. Keep the run as a test. Turn the failed run into a regression case by following the [testing documentation](https://mcpd.sh/docs/testing), then re-run it after you fix the tool result or add a retry cap.

A support agent can do the search and the reading without opening the interface. `mcpd mcp` exposes `find_runs`, `get_run` and `run_stats` over stdio, under the same API key scopes as everyone else.

## How do you set redaction, retention and erasure for stored prompts?

You set redaction, retention and erasure for stored prompts as configuration on the mcpd daemon: decide what is never written, how long the rest lives, and how to remove one person.

The exact setting names are in the [mcpd privacy documentation](https://mcpd.sh/docs/privacy).

1. Choose a redaction mode for each sensitive field. mcpd offers drop, mask, partial and keyed hash, applied before storage on every signal.

2. Use drop for API keys and card numbers, and mask for free text you never need. Use partial to keep a last-four suffix, and keyed hash for identifiers you still want to compare across runs.

4. Send a test trace that contains a fake secret, then search for it in the interface. Success is zero results. A hit means no rule covers that field.

5. Set retention in days on the daemon. Pick the shortest window that still covers your investigations. If support tickets reach you within 30 days, 30 days is enough. After the window, older runs no longer appear in the interface.

6. Match retention to your Postgres backups. A run past retention still sits in every backup taken before it expired. Set backup expiry to the retention window plus a short margin, so deleted prompts age out of the backups too.

7. Decide how you will find one person's data before you're asked. Pick the identifier your instrumentation already puts on spans, such as a user or session id.

8. File an erasure request about that person on the daemon, then check the result by searching for the identifier.

9. Confirm what leaves the machine. Forwarding is the only outbound path, and it exists only when you configure an endpoint. Copies forwarded as OTLP sit outside the daemon's retention, so set retention at the receiving end as well.

Retention limits how long prompts exist. API key scopes, SSO and the audit log decide who can read them in the meantime, as the [mcpd security documentation](https://mcpd.sh/docs/security) describes.

## Who can read your self hosted LLM traces, and what do you bind where?

You control who reads self hosted LLM traces with key scopes, accounts with roles, SSO, an audit log and a deliberate choice of which interface listens where. Each step closes one way a prompt could leak.

1. Set keys before the first real trace. [mcpd's security documentation](https://mcpd.sh/docs/security) lists six scopes: `ingest`, `read`, `write`, `execute`, `worker` and `admin`. mcpd compares keys as SHA-256 hashes. With no keys set, ingest and reads are open, which is for a laptop only.
2. Give agents `ingest` only. An agent key sits in the environment of every process that exports spans, so it's the key most likely to leak. With `ingest` alone, a leaked key can add spans but cannot read a prompt.
3. Give people `read`. Engineers who open the interface and search payloads need to see prompts, and nothing more. Keep `write` and `admin` for the few people who change settings.
4. Turn on accounts when more than a few people look. Pass `--accounts` on your own cluster, or `--signup` on the hosted service.
5. Connect single sign-on. SCIM, quotas and an audit log sit in the same configuration, so an auditor asking who did what gets a record.
6. Bind the two listeners separately. OTLP ingest only has to be reachable from the networks your agents run in. The interface faces people, behind your VPN or reverse proxy. Take the exact bind flags from the same security page.
7. Keep Postgres off every public interface. Only the daemon talks to it, so it can listen on a private address that your agents and your engineers never reach.
8. Point load balancer and orchestrator probes at `/healthz` and `/readyz`.

## Frequently asked questions

### What is self-hosted LLM tracing on Postgres?

Self-hosted LLM tracing on Postgres means a daemon that receives OpenTelemetry spans from your agent and stores prompts, completions, timings and cost in a Postgres database you run. With mcpd it is one binary plus Postgres, chosen through the storage setting, with the interface built in.

### Can I trace an agent without a vendor SDK?

You can, because mcpd reads standard OpenTelemetry. Set `OTEL_EXPORTER_OTLP_ENDPOINT` and `OTEL_EXPORTER_OTLP_PROTOCOL` on the exporter your agent already uses, and spans with `gen_ai.*` or OpenInference `llm.*` attributes arrive as [runs](https://mcpd.sh/docs/opentelemetry).

### Do I need ClickHouse, Redis or S3 to self-host mcpd?

You need only the daemon and Postgres. ClickHouse is an optional store for a team whose main workload becomes aggregates over very large trace volumes, and you choose it through the same storage setting.

### Does tracing slow down or break my agent if the daemon is down?

It doesn't. A recorded span never blocks or throws, and nothing travels through mcpd to reach a model, so a slow or stopped daemon can't delay your agent.

### Can I forward traces to Langfuse or another backend later?

You can. mcpd forwards what it stores as OTLP to any endpoint you name, with presets for Langfuse, LangSmith, Datadog and others, so the Postgres-backed daemon becomes one hop in a pipeline you already have.

## What to do in the next hour, day and week

Send a first trace in the next hour, move storage to Postgres today, and set redaction and retention this week.

**In the next hour:** export `OTEL_EXPORTER_OTLP_ENDPOINT` and `OTEL_EXPORTER_OTLP_PROTOCOL`, run your agent once and open the first trace. The [OpenTelemetry guide](https://mcpd.sh/docs/opentelemetry) lists the accepted protocols.

**Today:** point storage at Postgres, give each key one scope for one job, and schedule `pg_dump` backups. Restore one backup into a scratch database to prove it works.

**This week:** set the redaction mode and the retention days, so prompts are masked before the write and expire on a schedule. Add forwarding if you need a second backend.

To see a real trace before you run anything, [Start free](https://mcpd.sh/app) on the hosted version. Send the same two variables, read the prompt and completion, then move the setup onto your own Postgres when you're ready.

## Sources

[OpenTelemetry · mcpd](https://mcpd.sh/docs/opentelemetry) · [Integrating · mcpd](https://mcpd.sh/docs/integrating) · [loruorg/mcpd README](https://github.com/loruorg/mcpd) · [Self-host Langfuse (Open Source LLM Observability)…](https://langfuse.com/self-hosting) · [Langfuse Self-Hosted LLM Tracing (Docker Compose + Python…](https://ossaihub.com/code/langfuse-self-hosted-tracing/) · [Configuration · mcpd](https://mcpd.sh/docs/configuration) · [mcpd · The unopinionated agent observatory.](https://mcpd.sh) · [Security · mcpd](https://mcpd.sh/docs/security). Every other source is linked where the article cites it.
