---
description: Which Helicone alternative open source tool fits? Use an Apache-2.0 observer like mcpd that stays out of your request path. Compare licences and start a trace.
faq:
  - answer: "The best open source alternative depends on the job you need done. If you need a gateway that routes requests, pick another gateway. If you need to see what each agent run did, pick an observer that ingests OpenTelemetry, such as mcpd (Apache-2.0), and check each tool's licence file before you self-host."
    question: What is the best open source alternative to Helicone?
  - answer: Helicone is described as an open-source LLM observability platform that you can either self-host or run as a managed cloud. It also routes calls through a gateway, so read its repository licence file to confirm the terms before you decide to self-host.
    question: Is Helicone open source?
  - answer: "One tracker lists both tools as open source or self-hostable, and that label covers different licence terms for each. Read Langfuse's licensing page and the Phoenix repository's licence file to see exactly what you may self-host and what stays gated."
    question: Are Langfuse and Phoenix open source?
  - answer: You can keep Helicone running while you try mcpd. mcpd forwards what it stores as OTLP to any endpoint you name, and it has a Helicone preset, so Helicone keeps receiving spans while you compare the two views side by side.
    question: Can I keep Helicone while I try another tool?
  - answer: An out-of-path observer does not slow your agent down, because nothing travels through mcpd to reach a model. mcpd is free, and the hosted version and the self-hosted build are the same product, so you get the same thing either way.
    question: Does an observer slow my agent, and does mcpd cost anything?
image: /blog/helicone-alternative-open-source/featured-81fc9295dffd.webp
image_alt: "Illustration for Helicone alternative open source: Pick by licence"
publishDate: "2026-11-20"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/helicone-alternative-open-source#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\",\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q21708200\",\"@type\":\"Organization\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]}],\"datePublished\":\"2026-11-20\",\"description\":\"Which Helicone alternative open source tool fits? Use an Apache-2.0 observer like mcpd that stays out of your request path. Compare licences and start a trace.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"The best open source alternative depends on the job you need done. If you need a gateway that routes requests, pick another gateway. If you need to see what each agent run did, pick an observer that ingests OpenTelemetry, such as mcpd (Apache-2.0), and check each tool's licence file before you self-host.\"},\"name\":\"What is the best open source alternative to Helicone?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Helicone is described as an open-source LLM observability platform that you can either self-host or run as a managed cloud. It also routes calls through a gateway, so read its repository licence file to confirm the terms before you decide to self-host.\"},\"name\":\"Is Helicone open source?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"One tracker lists both tools as open source or self-hostable, and that label covers different licence terms for each. Read Langfuse's licensing page and the Phoenix repository's licence file to see exactly what you may self-host and what stays gated.\"},\"name\":\"Are Langfuse and Phoenix open source?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You can keep Helicone running while you try mcpd. mcpd forwards what it stores as OTLP to any endpoint you name, and it has a Helicone preset, so Helicone keeps receiving spans while you compare the two views side by side.\"},\"name\":\"Can I keep Helicone while I try another tool?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"An out-of-path observer does not slow your agent down, because nothing travels through mcpd to reach a model. mcpd is free, and the hosted version and the self-hosted build are the same product, so you get the same thing either way.\"},\"name\":\"Does an observer slow my agent, and does mcpd cost anything?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\",\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q116758847\",\"@type\":\"Organization\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q364\",\"@type\":\"Organization\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q334661\",\"@type\":\"Thing\",\"description\":\"permissive free software license\",\"name\":\"MIT License\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MIT_License\",\"https://www.wikidata.org/wiki/Q334661\",\"https://www.google.com/search?kgmid=/m/04vpm\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q104835286\",\"@type\":\"Thing\",\"description\":\"software license published by Elastic\",\"name\":\"Elastic License\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q104835286\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q27825826\",\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]}],\"name\":\"Helicone alternative open source: Pick by licence\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/6slTvHjnShtW-jFxVueOv3SLk3o9PjTj.webp\"},\"url\":\"https://mcpd.sh/blog/helicone-alternative-open-source\"},{\"@id\":\"https://mcpd.sh/blog/helicone-alternative-open-source#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\"},{\"@id\":\"https://www.wikidata.org/wiki/Q21708200\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"loruorg/mcpd repository\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"Cost · mcpd\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"Forwarding · mcpd\",\"url\":\"https://mcpd.sh/docs/forwarding\"},{\"@type\":\"CreativeWork\",\"name\":\"7 Best Helicone Alternatives (Self-Hosted) - Run This Ai\",\"url\":\"https://runthisai.com/en/helicone-alternatives\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"Helicone LICENSE\",\"url\":\"https://github.com/Helicone/helicone/blob/main/LICENSE\"},{\"@type\":\"CreativeWork\",\"name\":\"Langfuse LICENSE\",\"url\":\"https://github.com/langfuse/langfuse/blob/main/LICENSE\"},{\"@type\":\"CreativeWork\",\"name\":\"Self-host Langfuse\",\"url\":\"https://langfuse.com/self-hosting\"},{\"@type\":\"CreativeWork\",\"name\":\"Arize Phoenix LICENSE\",\"url\":\"https://github.com/Arize-ai/phoenix/blob/main/LICENSE\"},{\"@type\":\"CreativeWork\",\"name\":\"Opik LICENSE\",\"url\":\"https://github.com/comet-ml/opik/blob/main/LICENSE\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenLLMetry README\",\"url\":\"https://github.com/traceloop/openllmetry\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory.\",\"url\":\"https://mcpd.sh/docs/searching\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory.\",\"url\":\"https://mcpd.sh\"},{\"@type\":\"CreativeWork\",\"name\":\"Alerting · mcpd\",\"url\":\"https://mcpd.sh/docs/alerting\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd privacy documentation\",\"url\":\"https://mcpd.sh/docs/privacy\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd security documentation\",\"url\":\"https://mcpd.sh/docs/security\"},{\"@type\":\"CreativeWork\",\"name\":\"Helicone alternatives, including open source\",\"url\":\"https://howmakersbuild.com/tools/helicone/alternatives\"}],\"dateModified\":\"2026-11-20\",\"datePublished\":\"2026-11-20\",\"description\":\"Which Helicone alternative open source tool fits? Use an Apache-2.0 observer like mcpd that stays out of your request path. Compare licences and start a trace.\",\"headline\":\"Helicone alternative open source: Pick by licence\",\"image\":\"https://api.birchseek.com/m/6slTvHjnShtW-jFxVueOv3SLk3o9PjTj.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/helicone-alternative-open-source#webpage\"},\"keywords\":[\"helicone alternative open source\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/helicone-alternative-open-source#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\"},{\"@id\":\"https://www.wikidata.org/wiki/Q116758847\"},{\"@id\":\"https://www.wikidata.org/wiki/Q364\"},{\"@id\":\"https://www.wikidata.org/wiki/Q334661\"},{\"@id\":\"https://www.wikidata.org/wiki/Q104835286\"},{\"@id\":\"https://www.wikidata.org/wiki/Q27825826\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2425},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"permissive free software license\",\"name\":\"MIT License\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MIT_License\",\"https://www.wikidata.org/wiki/Q334661\",\"https://www.google.com/search?kgmid=/m/04vpm\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"general-purpose programming language\",\"name\":\"Python\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Python_(programming_language)\",\"https://www.wikidata.org/wiki/Q28865\",\"https://www.google.com/search?kgmid=/m/05z1_\"]},{\"@type\":\"Thing\",\"description\":\"creation or optimization of a prompt to be given to an artificial intelligence model\",\"name\":\"Prompt engineering\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Prompt_engineering\",\"https://www.wikidata.org/wiki/Q108941486\",\"https://www.google.com/search?kgmid=/g/11p6kpgt_n\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence software company\",\"name\":\"Braintrust\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Braintrust\",\"https://www.wikidata.org/wiki/Q141164091\"]},{\"@type\":\"Thing\",\"description\":\"Python library\",\"name\":\"langsmith\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q132393290\"]},{\"@type\":\"Thing\",\"description\":\"Python library\",\"name\":\"mlflow\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q107381745\"]},{\"@type\":\"Thing\",\"description\":\"software to manage containers on a server-cluster\",\"name\":\"Kubernetes\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Kubernetes\",\"https://www.wikidata.org/wiki/Q22661306\",\"https://www.google.com/search?kgmid=/g/11b7lxp79d\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - llm observability
  - helicone
  - opentelemetry
  - open source
  - ai agents
  - self-hosting
title: "Helicone alternative open source: Pick by licence"
---

**The right Helicone alternative open source option depends on the job: replacing a gateway that routes requests, or adding an observer that reads agent runs from outside the request path. [mcpd](https://github.com/loruorg/mcpd) is the observer, licensed Apache-2.0 and never in the request path, so if it goes down your agent keeps running. For any gateway, check the licence before you adopt it.**

Your agent loops in production, and all you have is a list of request logs: one row per call, nothing that shows which tool call triggered the next. Choosing a helicone alternative open source tool then comes down to two things: its licence and what it costs to run yourself.

After reading, you can pick a Helicone alternative open source tool on those two terms, set up a first trace, and keep or forward your existing Helicone data.

> **Key Takeaways**
> - Read licence status from each project's own LICENSE file, such as [Langfuse's LICENSE](https://github.com/langfuse/langfuse/blob/main/LICENSE) and [Phoenix's licence file](https://github.com/Arize-ai/phoenix/blob/main/LICENSE).
> - A gateway's outages become your agent's outages; an out-of-path OpenTelemetry observer that fails loses only traces.
> - Cost needs a price book: [in mcpd](https://mcpd.sh/docs/cost), an unknown model has no cost, not a cost of zero.
> - [mcpd forwards OTLP](https://github.com/loruorg/mcpd) to a named endpoint, [with a Helicone preset](https://mcpd.sh/docs/forwarding), so you can adopt it and keep Helicone.

## Why look for an open source Helicone alternative now?

A good reason to look for an open source Helicone alternative is a concrete requirement: where your data lives, who runs the service, or whether the tool should sit in your request path at all.

![Illustration for Why look for an open source Helicone alternative now?](/blog/helicone-alternative-open-source/image-1-bb023f303e52.webp)

Self-hosting on its own is weak grounds for leaving. [Run This Ai](https://runthisai.com/en/helicone-alternatives) describes Helicone as "an open-source LLM observability platform" and says you can "Self-host it for full control over your data, or use the managed cloud." If your only requirement is a service you run yourself, check that route first.

The request path is the stronger reason. Run This Ai also says Helicone is "Integrated with OpenAI, Anthropic, Gemini, and 100+ LLM providers via a unified gateway." A gateway routes your requests, and an observer reads what happened afterwards, so you can choose each one separately.

The two integration shapes fail differently:

- With a base-URL change, your agent sends every model call to the gateway, so a slow or unavailable gateway slows or blocks the agent.
- With logging after the call, the agent calls the provider directly and ships a record afterwards, so an unavailable logger loses data and the agent carries on.

Data control is the third reason. Prompts and completions contain customer text, so you may want redaction before storage and a retention period you set, whichever tool you pick.

An "open source" label settles none of this. It doesn't tell you which components you can self-host, or which features sit behind a paid tier.

## Proxy or OpenTelemetry observer: what happens when it goes down?

A gateway-mode proxy passes its latency and outages on to your agent, while an out-of-path observer that is slow or down loses only the trace. The proxy sits in the request path and receives every model call. The observer receives copies of spans over OpenTelemetry after the call, so the agent still reaches the model.

Helicone's integration is described by [Run This Ai](https://runthisai.com/en/helicone-alternatives) as reaching providers "via a unified gateway". In gateway mode you change the model client's base URL, so each call travels through the gateway before it reaches the model. Whatever happens to the gateway happens to the call.

Logging that runs after the response has returned, from any product, behaves like an observer. Check which of the two your integration does.

If an agent loops on a search tool 40 times in one run, the two designs part ways in three bad states:

- Slow: a gateway adds its delay to each of the 40 calls. An observer adds nothing, because the exporter runs beside the call.
- Down: a gateway returns errors or timeouts to the agent unless you have wired a fallback route. An observer loses the spans for that window.
- Rate limited: a gateway that refuses a request refuses the agent's call. An observer that refuses a span refuses a copy of something that already happened.

The setup differs in the same way. With OpenTelemetry you set `OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh` and `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`. The model client keeps its original base URL.

mcpd is built as an observer. [Its README](https://github.com/loruorg/mcpd) says: "It is not a gateway and not a proxy. Nothing travels through mcpd to reach a model, so mcpd being slow or down cannot slow your agent or break it." Recording a span never blocks, never throws and never grows without bound, so an unreachable mcpd goes unnoticed by the agent.

Existing instrumentation keeps working. The [mcpd OpenTelemetry documentation](https://mcpd.sh/docs/opentelemetry) covers support for GenAI and OpenInference semantic conventions, which means both `gen_ai.*` and `llm.*` attributes are read.

Replacing a gateway means deciding who routes, caches and rate limits your model traffic. Replacing an observer means deciding where traces are read. You can change one and keep the other.

## Which Helicone alternatives are open source, and what does self-hosting take?

Helicone, Langfuse, Opik, OpenLLMetry and OpenLIT carry open licences in their repositories, Arize Phoenix carries a source-available one, and self-hosting ranges from one binary to several containers with storage components.

| | Helicone | Langfuse | Arize Phoenix | mcpd |
| --- | --- | --- | --- | --- |
| Licence file | Apache-2.0 | MIT, except `ee/` | Elastic License 2.0 | Apache-2.0 |
| Class | Open source | Open source core | Source-available | Open source |
| Gated by the licence | Nothing named | `ee/` directory | Hosted-service use | Nothing named |

[Helicone's repository LICENSE file](https://github.com/Helicone/helicone/blob/main/LICENSE) is the Apache licence, "Version 2.0, January 2004".

[Langfuse's LICENSE](https://github.com/langfuse/langfuse/blob/main/LICENSE) says "Portions of this software are licensed as follows" and places the content under the `ee/` directory under a separate licence, with the rest under MIT.

Self-hosting Langfuse means more than one process. [Langfuse's self-hosting documentation](https://langfuse.com/self-hosting) says it "consists of two application containers, storage components, and an optional LLM API/Gateway."

[The Phoenix repository's LICENSE](https://github.com/Arize-ai/phoenix/blob/main/LICENSE) names Elastic License 2.0. That licence says "You may not provide the software to third parties as a hosted or managed service, where the service provides users with access to any substantial set of the features or functionality of the software." That restriction puts Phoenix in the source-available category.

[Opik, OpenLLMetry and OpenLIT](https://github.com/comet-ml/opik/blob/main/LICENSE) each state "Version 2.0, January 2004" in their LICENSE files, which makes all three Apache-2.0. [OpenLLMetry's README](https://github.com/traceloop/openllmetry) describes the project as "Open-source observability for your LLM application, based on OpenTelemetry".

[mcpd's repository](https://github.com/loruorg/mcpd) is Apache-2.0, and its README says it is "one static Rust binary" with storage and the web interface built in. It uses one SQLite file by default and Postgres or ClickHouse for a team.  The [operations guide](https://mcpd.sh/docs/operations) lists the single-instance limits.

Each Helicone alternative open source option suits a different team:

- Helicone suits teams that want the Apache-2.0 platform and a managed cloud or self-hosting.
- Langfuse suits teams that can run two application containers plus storage components, and accept a separate licence on `ee/`.
- Arize Phoenix suits teams whose legal review accepts Elastic License 2.0.
- OpenLLMetry suits teams that want OpenTelemetry instrumentation under Apache-2.0.
- mcpd suits engineers who want a first trace from one binary and one SQLite file.

## What does agent-level observability show that request logs do not?

Agent-level observability shows how calls relate inside one run: which tool result led to which model call, and what each step cost. Request-level logging records each call on its own: model, tokens, latency. It can't tell you that call 14 happened because tool call 13 returned an empty result.

A looping tool call or a cost spike shows up in request logs as a stack of near-identical rows, and nothing in the rows says why. [mcpd's home page](https://mcpd.sh) describes the alternative: "A run is a tree of model calls, tool calls and retrievals." Each step opens to the exact prompt and answer, the tokens it used and what it cost.

The [runs guide](https://mcpd.sh/docs/runs) covers how a run is structured.

A `lookup_order` tool that returns an empty result and gets called 30 more times appears in request logs as 30 model calls. The trace shows one run with `lookup_order` repeated under the same agent span. Each repeat has its own prompt, completion and cost, so you can see the model resending the same arguments.

Cost comes from the step. [mcpd's cost documentation](https://mcpd.sh/docs/cost) says it prices each step once, at ingest, from a price book of per-million-token rates, and you can supply your own book. An unknown model has no cost, not a cost of zero, so a missing rate never looks like a free step.

To find the run in the first place, [full-text search over the payload](https://mcpd.sh/docs/searching) matches what the model said, so a phrase like "I couldn't find that order" leads straight to the run.

[mcpd monitors](https://mcpd.sh/docs/alerting) are evaluated every 60 s, latch, fire once and call your webhook.

## Helicone alternative open source setup: how do you keep your Helicone data?

You can set up mcpd beside Helicone in three moves: send a first trace over OpenTelemetry, remove the Helicone base-URL override from your client, and forward the same spans to Helicone for as long as you want them there.

### Path 1: get a first trace

1. Sign up at [mcpd.sh/app](https://mcpd.sh/app) and create a project. Copy the key it shows you.
2. Set the exporter in the process that runs your agent:

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

3. Add the key as the authentication header. Copy the exact header name and format from the [OpenTelemetry docs](https://mcpd.sh/docs/opentelemetry) and the [quickstart](https://mcpd.sh/docs/quickstart).
4. Run the agent once and open the project. You should see one run with nested spans, and the prompt and completion on each model step. 

Your existing `gen_ai.*` or OpenInference `llm.*` instrumentation stays as it is, and the OpenTelemetry route needs no mcpd package. 

### Path 1b: run it yourself instead

The source is at [github.com/loruorg/mcpd](https://github.com/loruorg/mcpd). You need Rust and Node, because the interface is compiled into the binary.

```shell
git clone https://github.com/loruorg/mcpd
cd mcpd
make build
./target/release/mcpd demo
```

Open `http://localhost:4317`. When you're ready to send real traces, point the same two environment variables at your own address.

### Path 2: move off the Helicone base URL

1. Remove the base-URL override from the client, so model calls go straight to the provider again.
2. Add the OTLP exporter from Path 1 to the same process.
3. Run both side by side. Keep Helicone's dashboard open next to the mcpd project and compare run counts and token totals for the same hour. Don't cut over until they match.

If OpenTelemetry isn't an option for your stack, the [integration guide](https://mcpd.sh/docs/integrating) compares the HTTP API, the SDKs and the MCP server.

### Path 3: keep Helicone receiving data

To let Helicone keep its history and its dashboards, forward from mcpd instead of routing through Helicone. mcpd forwards what it stores as OTLP to any endpoint you name. Open the [forwarding documentation](https://mcpd.sh/docs/forwarding), pick the Helicone preset, and copy its config keys exactly as printed.

To leave Helicone later, delete its forwarding entry. Your agent's request path never changes.

To try the first path now, [start free at mcpd.sh/app](https://mcpd.sh/app).

## Which Helicone alternative open source setup fits a solo engineer or a platform team?

The same mcpd artefact serves a solo engineer and a platform team, and configuration is the only difference between them.

| Need | Solo engineer | Platform team |
| --- | --- | --- |
| First trace | One binary | Same binary |
| Storage | One SQLite file | Postgres or ClickHouse |
| Privacy | Nothing leaves the machine | Redaction and retention |
| Cost ceiling | Price book | Monitors on cost |

A solo engineer builds from source, runs `./target/release/mcpd demo` and opens `http://localhost:4317`. Data lands in one SQLite file.

The hosted version at mcpd.sh/app is the other quick start.

A platform team turns on controls. Redaction runs before storage (drop, mask, partial or keyed hash) on every signal, retention is set in days, and erasure requests can target a person. API keys carry six scopes and are compared as SHA-256 hashes.

Storage moves to Postgres or ClickHouse.

For a cost ceiling, a monitor on cost fires a webhook when spend crosses your threshold.

## Frequently asked questions

### What is the best open source alternative to Helicone?

It depends on the job. If you need a gateway that routes requests, pick another gateway. If you need to see what each agent run did, pick an observer that ingests OpenTelemetry, such as [mcpd (Apache-2.0)](https://github.com/loruorg/mcpd), and check each tool's licence file before you self-host.

### Is Helicone open source?

[Run This Ai](https://runthisai.com/en/helicone-alternatives) describes Helicone as an open-source LLM observability platform that you can self-host or run as a managed cloud. It also routes calls through a gateway. Its [repository licence file](https://github.com/Helicone/helicone/blob/main/LICENSE) is Apache-2.0, so read it before you self-host.

### Are Langfuse and Phoenix open source?

[One tracker](https://howmakersbuild.com/tools/helicone/alternatives) lists both as open source or self-hostable, but the licence terms differ.

[Langfuse's LICENSE](https://github.com/langfuse/langfuse/blob/main/LICENSE) puts the `ee/` directory under a separate licence, and [Phoenix's LICENSE](https://github.com/Arize-ai/phoenix/blob/main/LICENSE) is Elastic License 2.0, which restricts offering it as a hosted or managed service.

### Can I keep Helicone while I try another tool?

You can. mcpd forwards what it stores as OTLP to any endpoint you name, and it has a [Helicone forwarding preset](https://mcpd.sh/docs/forwarding), so Helicone keeps receiving spans while you compare the two views side by side.

### Does an observer slow my agent?

An out-of-path observer doesn't, because nothing travels through mcpd to reach a model. mcpd is free, and the hosted version and the self-hosted build are the same product.

## Next steps: test a Helicone alternative open source tool

The cheapest way to test a Helicone alternative open source is one exporter setting, so start there and add the rest as you need it.

- In one day: run mcpd beside Helicone and turn on forwarding with the Helicone preset, then compare what each tool shows for a looping run.
- In one week: set redaction (drop, mask, partial or hash), a retention period in days, and a cost monitor with a threshold you choose.

If you decide to leave, your spans can go to any endpoint you name.

## Sources

[loruorg/mcpd repository](https://github.com/loruorg/mcpd) · [Cost · mcpd](https://mcpd.sh/docs/cost) · [Forwarding · mcpd](https://mcpd.sh/docs/forwarding) · [7 Best Helicone Alternatives (Self-Hosted) - Run This Ai](https://runthisai.com/en/helicone-alternatives) · [OpenTelemetry · mcpd](https://mcpd.sh/docs/opentelemetry) · [Arize Phoenix LICENSE](https://github.com/Arize-ai/phoenix/blob/main/LICENSE) · [mcpd privacy documentation](https://mcpd.sh/docs/privacy) · [mcpd security documentation](https://mcpd.sh/docs/security). Every other source is linked where the article cites it.
