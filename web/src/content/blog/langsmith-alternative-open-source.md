---
description: Need a langsmith alternative open source? Langfuse is MIT; Opik, Laminar and MLflow are Apache-2.0; Phoenix is source-available. Compare and migrate via OTLP.
faq:
  - answer: The main open source LangSmith alternatives are Langfuse (MIT, except its ee folders), Comet Opik, Laminar, MLflow and OpenLLMetry (Apache-2.0 per their licence files), plus mcpd (Apache-2.0). Arize Phoenix is source-available under Elastic License 2.0. Choose by team shape, licence and what you must stand up before the first trace.
    question: Which open source LangSmith alternatives are there?
  - answer: Arize Phoenix is source-available under Elastic License 2.0, which is a different category from the Apache-2.0 and MIT tools. You can read and self-host the code, but it sits outside that group. If your policy requires an OSI-approved licence, pick from the other tools.
    question: Does Arize Phoenix count as open source?
  - answer: Self-hosted LangSmith is not free, because self-hosting is an Enterprise-only arrangement. You cannot run it on your own hardware as a free tier. The open source alternatives, including Langfuse and Opik, run on your own infrastructure with no plan to buy.
    question: Is self-hosted LangSmith free?
  - answer: Yes, you can keep your OpenTelemetry instrumentation when the new tool reads the attributes your spans already carry. mcpd reads gen_ai.* and llm.* attributes, so you point the OTLP exporter endpoint at the new target and leave the instrumentation code alone.
    question: Can I keep my OpenTelemetry instrumentation when I leave LangSmith?
  - answer: "Tracing slows an agent only when it sits in the request path or blocks while exporting. With mcpd, recording a span never blocks, never throws and never grows without bound, and nothing proxies model traffic, so an unreachable mcpd doesn't touch the agent."
    question: Will tracing slow my agent down?
image: /blog/langsmith-alternative-open-source/featured-67acb4e34d25.webp
image_alt: A back view of a person at a multi-monitor desk working with hardware and software in a tech-filled room.
publishDate: "2026-11-19"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/langsmith-alternative-open-source#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\",\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\",\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]}],\"datePublished\":\"2026-11-19\",\"description\":\"Need a langsmith alternative open source? Langfuse is MIT; Opik, Laminar and MLflow are Apache-2.0; Phoenix is source-available. Compare and migrate via OTLP.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"The main open source LangSmith alternatives are Langfuse (MIT, except its ee folders), Comet Opik, Laminar, MLflow and OpenLLMetry (Apache-2.0 per their licence files), plus mcpd (Apache-2.0). Arize Phoenix is source-available under Elastic License 2.0. Choose by team shape, licence and what you must stand up before the first trace.\"},\"name\":\"Which open source LangSmith alternatives are there?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Arize Phoenix is source-available under Elastic License 2.0, which is a different category from the Apache-2.0 and MIT tools. You can read and self-host the code, but it sits outside that group. If your policy requires an OSI-approved licence, pick from the other tools.\"},\"name\":\"Does Arize Phoenix count as open source?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Self-hosted LangSmith is not free, because self-hosting is an Enterprise-only arrangement. You cannot run it on your own hardware as a free tier. The open source alternatives, including Langfuse and Opik, run on your own infrastructure with no plan to buy.\"},\"name\":\"Is self-hosted LangSmith free?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Yes, you can keep your OpenTelemetry instrumentation when the new tool reads the attributes your spans already carry. mcpd reads gen_ai.* and llm.* attributes, so you point the OTLP exporter endpoint at the new target and leave the instrumentation code alone.\"},\"name\":\"Can I keep my OpenTelemetry instrumentation when I leave LangSmith?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Tracing slows an agent only when it sits in the request path or blocks while exporting. With mcpd, recording a span never blocks, never throws and never grows without bound, and nothing proxies model traffic, so an unreachable mcpd doesn't touch the agent.\"},\"name\":\"Will tracing slow my agent down?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q334661\",\"@type\":\"Thing\",\"description\":\"permissive free software license\",\"name\":\"MIT License\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MIT_License\",\"https://www.wikidata.org/wiki/Q334661\",\"https://www.google.com/search?kgmid=/m/04vpm\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q28865\",\"@type\":\"Thing\",\"description\":\"general-purpose programming language\",\"name\":\"Python\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Python_(programming_language)\",\"https://www.wikidata.org/wiki/Q28865\",\"https://www.google.com/search?kgmid=/m/05z1_\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q364\",\"@type\":\"Organization\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q467707\",\"@type\":\"Thing\",\"description\":\"set of software development tools\",\"name\":\"Software development kit\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Software_development_kit\",\"https://www.wikidata.org/wiki/Q467707\",\"https://www.google.com/search?kgmid=/m/0164kt\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q978185\",\"@type\":\"Thing\",\"description\":\"programming language, superset of JavaScript that compiles to JavaScript\",\"name\":\"TypeScript\",\"sameAs\":[\"https://en.wikipedia.org/wiki/TypeScript\",\"https://www.wikidata.org/wiki/Q978185\",\"https://www.google.com/search?kgmid=/m/0n50hxv\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q104835286\",\"@type\":\"Thing\",\"description\":\"software license published by Elastic\",\"name\":\"Elastic License\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q104835286\"]}],\"name\":\"LangSmith alternative open source: How to choose\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/qoYkHyZFYyD0ScERTddTU7vCyz0Aswa_.webp\"},\"url\":\"https://mcpd.sh/blog/langsmith-alternative-open-source\"},{\"@id\":\"https://mcpd.sh/blog/langsmith-alternative-open-source#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\"},{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"phoenix/LICENSE at main · Arize-ai/phoenix\",\"url\":\"https://github.com/Arize-ai/phoenix/blob/main/LICENSE\"},{\"@type\":\"CreativeWork\",\"name\":\"langfuse/LICENSE at main · langfuse/langfuse\",\"url\":\"https://github.com/langfuse/langfuse/blob/main/LICENSE\"},{\"@type\":\"CreativeWork\",\"name\":\"LangSmith Alternatives: The Best Open Source Options (2026)\",\"url\":\"https://openobserve.ai/blog/langsmith-alternatives/\"},{\"@type\":\"CreativeWork\",\"name\":\"Cost · mcpd\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd README\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"Forwarding · mcpd\",\"url\":\"https://mcpd.sh/docs/forwarding\"},{\"@type\":\"CreativeWork\",\"name\":\"Apache License, Version 2.0\",\"url\":\"https://www.apache.org/licenses/LICENSE-2.0\"},{\"@type\":\"CreativeWork\",\"name\":\"OSI Approved Licenses\",\"url\":\"https://opensource.org/licenses\"},{\"@type\":\"CreativeWork\",\"name\":\"Elastic License 2.0\",\"url\":\"https://www.elastic.co/licensing/elastic-license\"},{\"@type\":\"CreativeWork\",\"name\":\"Privacy · mcpd\",\"url\":\"https://mcpd.sh/docs/privacy\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd security documentation\",\"url\":\"https://mcpd.sh/docs/security\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd operations guide\",\"url\":\"https://mcpd.sh/docs/operations\"},{\"@type\":\"CreativeWork\",\"name\":\"Evaluation · mcpd\",\"url\":\"https://mcpd.sh/docs/evaluation\"},{\"@type\":\"CreativeWork\",\"name\":\"Evaluation · mcpd\",\"url\":\"https://mcpd.sh/docs/testing\"},{\"@type\":\"CreativeWork\",\"name\":\"Alerting · mcpd\",\"url\":\"https://mcpd.sh/docs/alerting\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenLLMetry LICENSE\",\"url\":\"https://github.com/traceloop/openllmetry/blob/main/LICENSE\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"Integrating · mcpd\",\"url\":\"https://mcpd.sh/docs/integrating\"}],\"dateModified\":\"2026-11-19\",\"datePublished\":\"2026-11-19\",\"description\":\"Need a langsmith alternative open source? Langfuse is MIT; Opik, Laminar and MLflow are Apache-2.0; Phoenix is source-available. Compare and migrate via OTLP.\",\"headline\":\"LangSmith alternative open source: How to choose\",\"image\":\"https://api.birchseek.com/m/qoYkHyZFYyD0ScERTddTU7vCyz0Aswa_.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/langsmith-alternative-open-source#webpage\"},\"keywords\":[\"langsmith alternative open source\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/langsmith-alternative-open-source#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q334661\"},{\"@id\":\"https://www.wikidata.org/wiki/Q28865\"},{\"@id\":\"https://www.wikidata.org/wiki/Q364\"},{\"@id\":\"https://www.wikidata.org/wiki/Q467707\"},{\"@id\":\"https://www.wikidata.org/wiki/Q978185\"},{\"@id\":\"https://www.wikidata.org/wiki/Q104835286\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2487},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"permissive free software license\",\"name\":\"MIT License\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MIT_License\",\"https://www.wikidata.org/wiki/Q334661\",\"https://www.google.com/search?kgmid=/m/04vpm\"]},{\"@type\":\"Thing\",\"description\":\"creation or optimization of a prompt to be given to an artificial intelligence model\",\"name\":\"Prompt engineering\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Prompt_engineering\",\"https://www.wikidata.org/wiki/Q108941486\",\"https://www.google.com/search?kgmid=/g/11p6kpgt_n\"]},{\"@type\":\"Thing\",\"description\":\"software to manage containers on a server-cluster\",\"name\":\"Kubernetes\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Kubernetes\",\"https://www.wikidata.org/wiki/Q22661306\",\"https://www.google.com/search?kgmid=/g/11b7lxp79d\"]},{\"@type\":\"Thing\",\"description\":\"artificial intelligence tool for code autocompletion\",\"name\":\"GitHub Copilot\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub_Copilot\",\"https://www.wikidata.org/wiki/Q107435063\",\"https://www.google.com/search?kgmid=/g/11q83qbj3d\"]},{\"@type\":\"Thing\",\"description\":\"general-purpose programming language\",\"name\":\"Python\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Python_(programming_language)\",\"https://www.wikidata.org/wiki/Q28865\",\"https://www.google.com/search?kgmid=/m/05z1_\"]},{\"@type\":\"Thing\",\"description\":\"Chinese artificial intelligence company\",\"name\":\"DeepSeek\",\"sameAs\":[\"https://en.wikipedia.org/wiki/DeepSeek\",\"https://www.wikidata.org/wiki/Q131577453\",\"https://www.google.com/search?kgmid=/g/11wvrb0s91\"]},{\"@type\":\"Thing\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - langsmith alternative
  - open source
  - llm observability
  - self-hosting
  - opentelemetry
  - ai agents
title: "LangSmith alternative open source: How to choose"
---

**For a langsmith alternative open source teams can self-host, the licence is the first filter: Langfuse is MIT apart from its ee folders; Opik, Laminar, MLflow and OpenLLMetry are Apache-2.0 per their licence files; Arize Phoenix is Elastic License 2.0, which is source-available. Team shape and what has to run before the first trace decide the rest.**

Your agent loops on a tool call in production, and the tracing tool that should show you why is closed, priced per seat or hard to run yourself.

A replacement has to pass three checks: a licence you have read correctly, a fit with your constraint (data residency, self-hosting effort, team size), and a way to move existing traces without re-instrumenting.

> **Key Takeaways**
> - A README badge names one licence, so read the root LICENSE file and any subfolder licence.
> - [Nothing travels through mcpd](https://github.com/loruorg/mcpd) to reach a model, so it cannot slow or break the agent.
> - An unknown model has no cost, not a cost of zero, so a missing price never passes as a free call.
> - mcpd forwards OTLP to Langfuse, LangSmith, Phoenix, Braintrust and other presets, so adoption and exit both skip re-instrumenting.

## What counts as an open source LangSmith alternative?

An open source LangSmith alternative uses a licence approved by the Open Source Initiative, such as Apache-2.0 or MIT. Source-available licences publish the code but restrict use.

![Illustration for What counts as an open source LangSmith alternative?](/blog/langsmith-alternative-open-source/image-1-7b9e8597cc03.webp)

The Open Source Initiative (OSI) keeps the [list of approved licences](https://opensource.org/licenses), and that list is where open source ends and source-available begins. Search it for Elastic License 2.0 and you won't find it, so Phoenix is not an Apache-2.0 or MIT option.

Two licence files show the difference. [Langfuse's LICENSE](https://github.com/langfuse/langfuse/blob/main/LICENSE) begins "Portions of this software are licensed as follows": most of the repository is MIT, and the ee directories sit under a separate licence in ee/LICENSE.

[Phoenix's LICENSE](https://github.com/Arize-ai/phoenix/blob/main/LICENSE) is [Elastic License 2.0](https://www.elastic.co/licensing/elastic-license), which says you may not provide the software to third parties as a hosted or managed service that gives users access to any substantial set of its features.

- [Comet Opik](https://github.com/comet-ml/opik/blob/main/LICENSE)
- [Laminar](https://github.com/lmnr-ai/lmnr/blob/main/LICENSE)
- [MLflow](https://github.com/mlflow/mlflow/blob/master/LICENSE.txt)
- [OpenLLMetry](https://github.com/traceloop/openllmetry/blob/main/LICENSE)
- [mcpd](https://github.com/loruorg/mcpd/blob/main/LICENSE)

What each licence allows depends on what you plan to do with the code:

- Self-host for your own team: all three kinds work.
- With Langfuse, read ee/LICENSE before you touch anything under an ee folder, because the MIT grant stops at that boundary.
- Redistribute or resell: [Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0) lets you distribute copies of the work with or without modifications, and MIT lets you sell copies. Elastic License 2.0 does not allow you to offer the software as a managed service.

A README badge shows one licence name, while a repository can carry several. Check the LICENSE file in the root and any subfolder that has its own.

## Which open source LangSmith alternative fits your team?

The open source LangSmith alternative that fits your team is the one whose position in the request path, storage and licence match your constraints.

| Constraint | Solo operator | Platform team | Stay on LangSmith |
| --- | --- | --- | --- |
| Storage | One SQLite file | Postgres or ClickHouse | Hosted |
| Data residency | Your machine | Your cluster | Self-hosting is Enterprise-only |
| Self-hosting effort | No account, no database | A cluster to run | An Enterprise arrangement |
| Licence | Apache-2.0 (mcpd) | Apache-2.0 (mcpd) | Closed platform |

Start with where the tool sits, because that decides what it can break. A gateway carries every model request, so its outage is your agent's outage.

An SDK or framework integration runs inside your process, so its blocking calls and exceptions are yours.

A daemon watching through OpenTelemetry sits beside the agent. The [mcpd README](https://github.com/loruorg/mcpd) says "Nothing travels through mcpd to reach a model, so mcpd being slow or down cannot slow your agent or break it."

Language coverage is the second filter for any LangSmith alternative open source teams put on a shortlist.

Check that each open source LangSmith alternative on your shortlist covers the languages and frameworks your agents use.

Team shape is the third filter, and it decides which LangSmith alternative open source projects are worth a trial.

A platform team points the same binary at Postgres or ClickHouse and sets redaction, which the [mcpd privacy docs](https://mcpd.sh/docs/privacy) say "runs before the write, because a secret deleted afterwards was still written down."

Price the status quo as one input. [OpenObserve's comparison](https://openobserve.ai/blog/langsmith-alternatives/) says "LangSmith is a closed platform, self-hosting is an Enterprise-only arrangement, and per-seat billing applies to every engineer you add."

Put your seat count and trace retention tier, taken from LangSmith's own pricing and billing documentation, beside that.

Check each open source LangSmith alternative's project health before you commit. Read each candidate's latest announcements, release notes and commit activity for signs of maintenance mode or a change of owner.

Each team shape points to a different setup:

- Solo operator: a daemon with a local file, no sign-up and no change to your request path.
- Platform team: a self-hosted build on Postgres or ClickHouse with redaction, SSO, SCIM and an audit log on your own cluster.
- Stay on LangSmith: teams for whom per-seat billing and Enterprise-only self-hosting are not blockers.

## What does self-hosting a LangSmith alternative involve?

Self-hosting an open source LangSmith alternative means standing up every service the tool needs before it'll accept its first trace, then checking that the self-hosted build carries the features of the hosted one.

LangSmith itself sits outside the free self-host route: [OpenObserve's comparison](https://openobserve.ai/blog/langsmith-alternatives/) states that "self-hosting is an Enterprise-only arrangement". It isn't cloud-only, and it isn't a free download either.

For each open source option, three questions decide how much of your week the install takes. The README and self-hosting docs of Langfuse, Phoenix, Opik, Laminar and MLflow answer them.

| Question | What to look up | mcpd |
| --- | --- | --- |
| Which storage backend? | The database named in the install docs | SQLite file by default |
| What runs before the first trace? | Every service in the compose file or chart | One binary |
| Is any feature missing from the self-hosted build? | Folders or docs marked enterprise or cloud-only | Same artefact as the hosted version |

A tool that needs a database cluster, a queue and an object store is a small platform you'll have to run. It's worth comparing that list with your team's size before you compare dashboards.

mcpd is one static Rust binary with the web interface compiled in. You build it from source at github.com/loruorg/mcpd (Rust and Node are both needed), then run the demo:

```shell
make build
./target/release/mcpd demo
```

Open `http://localhost:4317` and the runs are there. Storage is a SQLite file by default, with Postgres or ClickHouse for a team, as the [operations guide](https://mcpd.sh/docs/operations) describes.

The same artefact serves a platform team. Start it with `--accounts` on your own cluster and you get accounts, SSO, SCIM, quotas and an audit log, with no separate enterprise build.

One default needs care. With no keys set, ingest and reads are open, which the [security documentation](https://mcpd.sh/docs/security) marks as for a laptop only. Set keys before the daemon is reachable from anywhere else.

## How do you migrate to an open source LangSmith alternative?

Migrating from LangSmith to an open source tool takes five steps, and none of them changes your instrumentation code. Existing OpenTelemetry instrumentation already carries the attributes the new tool reads.

1. Keep your instrumentation. Instrumentation that emits OpenTelemetry `gen_ai.*` attributes or OpenInference `llm.*` attributes stays as it is, and only the exporter destination changes. The [OpenTelemetry guide](https://mcpd.sh/docs/opentelemetry) describes OTLP, Zipkin and Jaeger ingestion with support for both attribute conventions, so a service already tracing to another backend usually needs only the new endpoint.

2. Create a project and a key. Sign up at [mcpd.sh/app](https://mcpd.sh/app), or build the daemon from source at github.com/loruorg/mcpd and run it yourself. Copy the project key, because a key from the wrong project sends traces somewhere you won't look.

3. Point the exporter at mcpd. Set the two standard variables, then add the project key through `OTEL_EXPORTER_OTLP_HEADERS`, in the header format the OpenTelemetry guide gives. No package is installed.

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

4. Run a non-Python service the same way. A TypeScript service on the OpenTelemetry Node SDK picks up the same variables when it starts. A Go service uses the same endpoint with `OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf`, because mcpd accepts protobuf as well as JSON over HTTP. If the first run never appears, check the protocol value before the key.

5. Forward OTLP out. Configure mcpd's [forwarding](https://mcpd.sh/docs/forwarding) to send what it stores to any OTLP endpoint, including the tool you run today. You can keep both tools receiving the same spans while you compare them, or turn forwarding on later to leave without re-instrumenting.

### What the first trace looks like

Run an agent with `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_EXPORTER_OTLP_PROTOCOL` and the project key set, and one run appears in the interface a few seconds after it finishes. The waterfall shows each model call and tool call on one clock.

Opening a step shows the exact prompt, the exact completion, the tokens and the cost. Search the run by a phrase the model wrote and the full-text index finds it, which labels alone can't do.

[Start free at mcpd.sh/app](https://mcpd.sh/app) and send the first trace from your own agent.

## How do cost tracking and evaluation differ between these tools?

Cost tracking and evaluation differ between tools in three mechanisms: where a call is priced, what happens to a model the price book doesn't know, and whether machine scores stay apart from human feedback. Read each tool's own pricing and evaluation documentation for those three points.

| Question | mcpd | Ask of any other tool |
| --- | --- | --- |
| Where is cost priced? | Once, at ingest, per step | At ingest, on read, or in a gateway? |
| What if the model is unknown? | No cost, not zero | Zero, a guess, or blank? |
| Machine scores and human feedback | Two signals, never averaged | One blended number or two? |
| Deterministic checks | Never call a model | Does every scorer call an LLM? |
| Alerts on cost or loops | Monitors latch, fire once | Does it re-fire every interval? |

When you weigh a LangSmith alternative open source projects offer, ask each one how it handles cached tokens and pricing tiers.

A later price change doesn't rewrite the cost of runs already stored.

A model missing from the book gets no cost. mcpd's cost docs put it this way: "An unknown model has no cost, not a cost of zero." A blank can be found and fixed, while a false zero sits quietly in your totals.

On evaluation, the [mcpd evaluation docs](https://mcpd.sh/docs/evaluation) state: "A score is what a machine thought. Feedback is what a person thought." The two are never averaged into one number, and a score declares its scale.

Deterministic scorers such as `no_tool_loop`, `cost_budget` and `json_valid` never call a model, and a model can also act as the judge. The [testing guide](https://mcpd.sh/docs/testing) covers these scorers, datasets built from production runs and experiments that compare two versions.

[mcpd monitors](https://mcpd.sh/docs/alerting) run every 60 s on `error_rate`, `latency_p95`, `cost`, `score_mean` and `trace_count`. They latch, fire once, resolve only on a real reading, and call your webhook.

### A looping tool call, caught once

If a refund agent repeats the same lookup tool call, the `no_tool_loop` scorer marks each looping run, and a `cost` monitor on the project fires once to your webhook.

The monitor stays latched while the spend stays high, and it resolves only when a real reading comes back under the threshold. You then [search by what the model said](https://mcpd.sh/docs/searching) to open the first bad run and read its prompt, its completion and the cost of each step.

mcpd fits teams that want cost fixed at ingest, scores and feedback kept apart, and a monitor that doesn't repeat itself. Any other tool fits when its documentation answers the same pricing, scoring and alerting questions the way your team needs.

## Frequently asked questions

### Which open source LangSmith alternatives are there?

The main open source LangSmith alternatives are Langfuse (MIT, except its ee folders), Comet Opik, Laminar, MLflow and OpenLLMetry (Apache-2.0 per their licence files), plus mcpd (Apache-2.0). Arize Phoenix is source-available under Elastic License 2.0. Choose by team shape, licence and what you must stand up before the first trace.

### Does Arize Phoenix count as open source?

Arize Phoenix is source-available under Elastic License 2.0, so you can read and self-host the code, but it's outside the Apache-2.0 and MIT group. If your policy requires an OSI-approved licence, pick from the other tools.

### Is self-hosted LangSmith free?

Self-hosting LangSmith is an Enterprise-only arrangement, so it isn't a free tier you can run on your own hardware. Langfuse, Opik, Laminar, MLflow, OpenLLMetry and mcpd run on your own infrastructure with no plan to buy.

### Can I keep my OpenTelemetry instrumentation when I leave LangSmith?

You can keep it when the new tool reads the attributes your spans already carry. mcpd reads gen_ai.* and llm.* attributes, so you point the OTLP exporter endpoint at the new target and leave the instrumentation code alone.

### Will tracing slow my agent down?

Tracing slows an agent only when it sits in the request path or blocks while exporting. With mcpd, recording a span never blocks, never throws and never grows without bound, and nothing proxies model traffic, so an unreachable mcpd doesn't touch the agent.

## Which open source LangSmith alternative should you try first?

Try the hosted version of mcpd first, because it puts one of your own runs in front of you without installing anything.

Today, create a project at mcpd.sh/app, copy its project key, and point your agent's OpenTelemetry exporter at the hosted endpoint:

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

Open one run and read the exact prompt, the completion and the cost of each step. The [quickstart](https://mcpd.sh/docs/quickstart) walks through the same path.

Your instrumentation stays as it is, so you can run mcpd beside your current tool and remove the exporter setting whenever you like.

This week, compare that run with your shortlist of LangSmith alternative open source projects. Open each project's licence file, and write down what must be running before its first trace. To test self-hosting, build mcpd from source at github.com/loruorg/mcpd.

This quarter, settle three things:

- where traces are stored
- which fields are redacted before storage
- how many days you keep them

[Start free at mcpd.sh/app](https://mcpd.sh/app). Nothing needs installing, and if it doesn't fit, you drop the two environment variables.

## Sources

[langfuse/LICENSE at main · langfuse/langfuse](https://github.com/langfuse/langfuse/blob/main/LICENSE) · [phoenix/LICENSE at main · Arize-ai/phoenix](https://github.com/Arize-ai/phoenix/blob/main/LICENSE) · [mcpd README](https://github.com/loruorg/mcpd) · [Cost · mcpd](https://mcpd.sh/docs/cost) · [Forwarding · mcpd](https://mcpd.sh/docs/forwarding) · [OSI Approved Licenses](https://opensource.org/licenses) · [LangSmith Alternatives: The Best Open Source Options (2026)](https://openobserve.ai/blog/langsmith-alternatives/) · [Integrating · mcpd](https://mcpd.sh/docs/integrating). Every other source is linked where the article cites it.
