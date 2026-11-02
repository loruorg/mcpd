---
description: What goes in an LLM audit log? One trace per run holds every model call, tool call and its cost. Learn to record, redact and retain it with OpenTelemetry.
faq:
  - answer: Tracing and an LLM audit log hold the same data and answer different questions. Tracing finds the bad run. The audit log must also show that the record was retained, redacted and access-controlled, so you can answer an auditor as well as debug.
    question: What is an LLM audit log, and how is it different from LLM tracing?
  - answer: Keep event metadata for as long as your obligations require, and keep prompt and completion content for a shorter, configured number of days. For providers of high-risk AI systems, Article 19 of the EU AI Act sets a minimum of six months for automatically generated logs.
    question: How long should you keep an LLM audit log?
  - answer: "You do not have to store prompts and completions, because content capture is opt-in in the OpenTelemetry GenAI conventions and you store only what you choose. Redact or drop what you don't need before it is written, and keep the event metadata either way."
    question: Do I have to store prompts and completions?
  - answer: The audit log does not slow down or break your agent when it runs beside the agent. mcpd is a daemon fed by OpenTelemetry, not a gateway or proxy, so nothing travels through it to reach a model. The trade-off is that it gives no commit-before-response guarantee.
    question: Can the audit log slow down or break my agent?
  - answer: You can send the audit log to your existing SIEM by forwarding it as OTLP to any endpoint you name, so your SIEM or collector receives it. mcpd forwards what it stores, which makes it one hop in a pipeline you already run.
    question: Can I send the audit log to my existing SIEM?
image: /blog/llm-audit-log/featured-c775ca3f6f9e.webp
image_alt: "Illustration for LLM audit log: Record, redact and retain agent runs"
publishDate: "2026-11-02"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/llm-audit-log#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"datePublished\":\"2026-11-02\",\"description\":\"What goes in an LLM audit log? One trace per run holds every model call, tool call and its cost. Learn to record, redact and retain it with OpenTelemetry.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Tracing and an LLM audit log hold the same data and answer different questions. Tracing finds the bad run. The audit log must also show that the record was retained, redacted and access-controlled, so you can answer an auditor as well as debug.\"},\"name\":\"What is an LLM audit log, and how is it different from LLM tracing?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Keep event metadata for as long as your obligations require, and keep prompt and completion content for a shorter, configured number of days. For providers of high-risk AI systems, Article 19 of the EU AI Act sets a minimum of six months for automatically generated logs.\"},\"name\":\"How long should you keep an LLM audit log?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You do not have to store prompts and completions, because content capture is opt-in in the OpenTelemetry GenAI conventions and you store only what you choose. Redact or drop what you don't need before it is written, and keep the event metadata either way.\"},\"name\":\"Do I have to store prompts and completions?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"The audit log does not slow down or break your agent when it runs beside the agent. mcpd is a daemon fed by OpenTelemetry, not a gateway or proxy, so nothing travels through it to reach a model. The trade-off is that it gives no commit-before-response guarantee.\"},\"name\":\"Can the audit log slow down or break my agent?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You can send the audit log to your existing SIEM by forwarding it as OTLP to any endpoint you name, so your SIEM or collector receives it. mcpd forwards what it stores, which makes it one hop in a pipeline you already run.\"},\"name\":\"Can I send the audit log to my existing SIEM?\"}],\"name\":\"LLM audit log: Record, redact and retain agent runs\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/01gEVWtDZ29dAHcGTWDdOfYmcFhMsN49.webp\"},\"url\":\"https://mcpd.sh/blog/llm-audit-log\"},{\"@id\":\"https://mcpd.sh/blog/llm-audit-log#article\",\"@type\":\"BlogPosting\",\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory.\",\"url\":\"https://mcpd.sh\"},{\"@type\":\"CreativeWork\",\"name\":\"Privacy · mcpd\",\"url\":\"https://mcpd.sh/docs/privacy\"},{\"@type\":\"CreativeWork\",\"name\":\"Cost · mcpd\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"Arize-ai/openinference\",\"url\":\"https://github.com/Arize-ai/openinference\"},{\"@type\":\"CreativeWork\",\"name\":\"Trace Context | W3C Recommendation\",\"url\":\"https://www.w3.org/TR/trace-context/\"},{\"@type\":\"CreativeWork\",\"name\":\"datetime — Basic date and time types | Python documentation\",\"url\":\"https://docs.python.org/3/library/datetime.html\"},{\"@type\":\"CreativeWork\",\"name\":\"Security · mcpd\",\"url\":\"https://mcpd.sh/docs/security\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory.\",\"url\":\"https://mcpd.sh/\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd README\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"Integrating · mcpd\",\"url\":\"https://mcpd.sh/docs/integrating\"},{\"@type\":\"CreativeWork\",\"name\":\"LLM Audit Logging Best Practices: Building Records That Survive a ...\",\"url\":\"https://www.deepinspect.ai/blog/llm-audit-logging-best-practices\"},{\"@type\":\"CreativeWork\",\"name\":\"Operations · mcpd\",\"url\":\"https://mcpd.sh/docs/operations\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd alerting docs\",\"url\":\"https://mcpd.sh/docs/alerting\"},{\"@type\":\"CreativeWork\",\"name\":\"OWASP Top 10 for LLM Applications\",\"url\":\"https://genai.owasp.org/llm-top-10/\"},{\"@type\":\"CreativeWork\",\"name\":\"Regulation (EU) 2024/1689 (Artificial Intelligence Act), EUR-Lex\",\"url\":\"https://aidefense.dev/posts/llm-audit-logging-what-to-log/\"}],\"dateModified\":\"2026-11-02\",\"datePublished\":\"2026-11-02\",\"description\":\"What goes in an LLM audit log? One trace per run holds every model call, tool call and its cost. Learn to record, redact and retain it with OpenTelemetry.\",\"headline\":\"LLM audit log: Record, redact and retain agent runs\",\"image\":\"https://api.birchseek.com/m/01gEVWtDZ29dAHcGTWDdOfYmcFhMsN49.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/llm-audit-log#webpage\"},\"keywords\":[\"llm audit log\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/llm-audit-log#webpage\"},\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2689},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"defined sequence of activities of an organizational entity, enabled by the systematic organization of resources into processes that transform materials, provide services, or process information\",\"name\":\"Workflow\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Workflow\",\"https://www.wikidata.org/wiki/Q627335\",\"https://www.google.com/search?kgmid=/m/0k5rb\"]},{\"@type\":\"Thing\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]},{\"@type\":\"Thing\",\"description\":\"collection of data, publication type\",\"name\":\"Data set\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Data_set\",\"https://www.wikidata.org/wiki/Q1172284\",\"https://www.google.com/search?kgmid=/m/02cck\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"benchmark in natural language understanding\",\"name\":\"General Language Understanding Evaluation benchmark\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q64711428\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"large language model company\",\"name\":\"LlamaIndex\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LlamaIndex\",\"https://www.wikidata.org/wiki/Q125446413\"]},{\"@type\":\"Thing\",\"description\":\"Chinese artificial intelligence company\",\"name\":\"DeepSeek\",\"sameAs\":[\"https://en.wikipedia.org/wiki/DeepSeek\",\"https://www.wikidata.org/wiki/Q131577453\",\"https://www.google.com/search?kgmid=/g/11wvrb0s91\"]},{\"@type\":\"Thing\",\"description\":\"language understanding across multiple domains\",\"name\":\"MMLU\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MMLU\",\"https://www.wikidata.org/wiki/Q123721276\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"family of multimodal artificial intelligence models developed by OpenAI\",\"name\":\"GPT-4o\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GPT-4o\",\"https://www.wikidata.org/wiki/Q125919502\",\"https://www.google.com/search?kgmid=/g/11vxhg1zhp\"]},{\"@type\":\"Thing\",\"description\":\"family of large language models developed by OpenAI\",\"name\":\"GPT-5\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GPT-5\",\"https://www.wikidata.org/wiki/Q135297698\"]},{\"@type\":\"Thing\",\"description\":\"a lightweight markup language with plain-text formatting syntax that is popularly used\",\"name\":\"Markdown\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Markdown\",\"https://www.wikidata.org/wiki/Q1193600\",\"https://www.google.com/search?kgmid=/m/0jt0gkj\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - llm audit log
  - opentelemetry
  - ai agents
  - data retention
  - redaction
  - access control
title: "LLM audit log: Record, redact and retain agent runs"
---

**An llm audit log for an agent is one trace per run. In [mcpd](https://mcpd.sh), each model call, tool call and retrieval is a nested span with the exact prompt, completion, tokens, latency and cost under one trace id. Content is [redacted before storage](https://mcpd.sh/docs/privacy) and kept fewer days than event metadata, while scoped keys and roles restrict who can read it.**

This is for engineers whose agent misbehaved in production (a refund tool called in a loop overnight, a bill that spiked) and who now owe a finance lead or an auditor an exact account. A chat log of single completions can't say which call belonged to which run, so it isn't an LLM audit log.

After reading, you can record a multi-step run over OpenTelemetry with no framework change, redact it before storage, set retention, restrict access and forward the record to a SIEM.

> **Key Takeaways**
> - Event metadata gets the long retention clock and prompt content the short one, because the AI Act and GDPR pull opposite ways.
> - Record the model name the provider returns, and read a missing cost as an unknown model rather than a free call.
> - A recorder beside the agent never blocks a model call, but it can't commit the record before the response returns.
> - Give each agent an ingest-only key, so it can add to the history without reading it back.

## What does an LLM audit log need to record for an agent run?

An LLM audit log for an agent run needs the run's structure as well as its text: which step called which, what each model received and returned, which model answered and what each step cost.

![Illustration for What does an LLM audit log need to record for an agent run?](/blog/llm-audit-log/image-1-ccf6f1de7a68.webp)

When a refund agent calls `issue_refund` over and over until morning, finance asks which step ran, with what input, at what cost. A log of single completions can't answer, so it can't serve as an LLM audit log: nothing ties a call to its run or shows which tool result fed the next prompt.

A [trace with nested spans](https://mcpd.sh/docs/concepts) can. One run has this shape:

```text
trace_id: <one id for the whole run>
invoke_agent refund-agent            (root span)
├─ chat <response model>             model call 1
├─ execute_tool lookup_order         tool call
├─ retrieval refund-policy           retrieval
├─ chat <response model>             model call 2
├─ execute_tool issue_refund         tool call, input X
├─ chat <response model>             model call 3
└─ execute_tool issue_refund         tool call, input X again
```

Each model span carries `gen_ai.system_instructions`, `gen_ai.input.messages` and `gen_ai.output.messages`. It also carries `gen_ai.request.model`, `gen_ai.response.model`, `gen_ai.usage.input_tokens` and `gen_ai.usage.output_tokens`.

[mcpd's OpenTelemetry docs](https://mcpd.sh/docs/opentelemetry) add that mcpd also reads the OpenInference `llm.*` attributes, so existing instrumentation from either convention works unchanged. [Arize describes OpenInference](https://github.com/Arize-ai/openinference) as "a set of conventions and plugins that is complementary to OpenTelemetry to enable tracing of AI applications."

In your LLM audit log, record the model name the provider returns, not a version string you've typed.

Cost goes on the span as well. [mcpd's cost docs](https://mcpd.sh/docs/cost) explain that mcpd prices each step once at ingest from a price book of per-million-token rates, so a span holds something like `<input tokens> × <rate per million>`, with cached tokens and long-prompt tiers handled.

An unknown model has no cost, not a cost of zero.

## How do you set up an LLM audit log with OpenTelemetry?

You set up an LLM audit log with OpenTelemetry by pointing any OTLP exporter at the mcpd daemon, so no mcpd package goes into your code. The [mcpd OpenTelemetry docs](https://mcpd.sh/docs/opentelemetry) cover the endpoint and header details.

The route works with LangChain, LangGraph, LlamaIndex, CrewAI, Vercel AI SDK, OpenAI Agents SDK, Pydantic AI or your own loop, and with OpenAI, Anthropic, Google Gemini, Mistral, Azure OpenAI, Groq, Ollama and vLLM. mcpd's [home page](https://mcpd.sh/) says it handles "Any framework, any language, any model provider."

1. Choose where the record goes. For the hosted version, sign up at mcpd.sh/app and create a project key. To self-host, clone github.com/loruorg/mcpd, run `make build` (Rust and Node are both needed), then run `./target/release/mcpd demo` and open http://localhost:4317.

2. Set the exporter variables in the environment of the process that runs your agent:

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
# send the project key as the exporter header, in the form shown at
# https://mcpd.sh/docs/opentelemetry
```

 For the self-hosted demo, use your local address as the endpoint.

3. Switch on content capture with the setting your instrumentation library documents. Until you do, a traced run shows only metadata, and your LLM audit log won't hold any prompt text. The [GenAI span conventions](https://opentelemetry.io/docs/specs/semconv/gen-ai/gen-ai-spans/) warn that these attributes can hold sensitive information, so turn capture on only where redaction is configured.

4. Stamp custom audit fields, such as a ticket number, as span attributes with a timezone-aware time:

```python
from datetime import datetime, timezone
from opentelemetry import trace

span = trace.get_current_span()
span.set_attribute("audit.approved_at", datetime.now(timezone.utc).isoformat())
```

 Avoid `datetime.utcnow()`. The [Python datetime documentation](https://docs.python.org/3/library/datetime.html) marks it deprecated and says to use datetime.now() with UTC instead.

5. Run the agent once and open the trace. You should see one run with nested spans for each model call, tool call and retrieval. If it's empty, check that the protocol is `http/json` and that the key header matches the docs.

 When agents call each other across services, the [W3C traceparent header](https://www.w3.org/TR/trace-context/) carries the trace id, so both sides land in one run.

The [quickstart](https://mcpd.sh/docs/quickstart) walks through the same first trace with your own agent. To skip the build, [start free on the hosted version](https://mcpd.sh/app).

## Should an audit log sit in the request path or beside the agent?

An audit log belongs in the request path when no response may exist without its record, and beside the agent when the agent must stay up whatever happens to the recorder.

| Property | Proxy in the request path | mcpd beside the agent |
| --- | --- | --- |
| Write path | Model traffic passes through it | OpenTelemetry sent to a separate daemon |
| Recorder failure | Can stop the model call | Agent doesn't notice |
| Commit before response | Yes, by design | No |

A proxy can write the record before the answer returns. [One audit-logging guide](https://www.deepinspect.ai/blog/llm-audit-logging-best-practices) advises you to "route sanctioned calls through an independent enforcement point and commit the record before the model response returns."

mcpd is a separate daemon beside the agent, fed by OpenTelemetry. As the [mcpd README](https://github.com/loruorg/mcpd) puts it, mcpd is not a gateway or proxy: nothing travels through it to reach a model, so it cannot slow or break the agent.

The audited code doesn't write its own LLM audit log, and the agent never waits on it. [mcpd's integration docs](https://mcpd.sh/docs/integrating) hold the SDKs to the same rule: they never block, never throw and never grow without bound, so if mcpd is unreachable the agent does not notice.

For an LLM audit log, the price of that design is commit-before-response. A daemon beside the agent can't refuse to answer until the record is written, so a response can return before its LLM audit log entry exists.

The README also says mcpd forwards what it stores as OTLP to any endpoint you name, which makes it one hop in a pipeline you already run.

The same guide says mcpd is Apache-2.0 licensed, stores everything in one SQLite file by default, and takes Postgres or ClickHouse for a team. Hosted at mcpd.sh/app, it is the same product, and a [relay mode](https://mcpd.sh/docs/relay) places a collector beside your agents.

## How do you find the run an auditor is asking about?

You find the run an auditor is asking about by searching the payload text for something the agent said, then opening the one trace that matches. Start from the complaint, such as a customer who says the agent refunded twice. The [runs documentation](https://mcpd.sh/docs/runs) covers tracing step failures inside a run.

1. Search for the words the agent used. Paste a sentence it sent the customer into search: mcpd's [home page](https://mcpd.sh) says search "finds a run by what the model said," so a run with a missing user tag still turns up. A filter on labels alone skips every run that was tagged wrongly.
2. Open the waterfall and read the full conversation with its tool calls. Every run sits on one clock, each step shows its cost, and logs link to the exact span by trace id.
3. Read the scores. Deterministic scorers such as `no_tool_loop` and `cost_budget` flag a loop or an overspend without calling a model, so the verdict is repeatable.
4. Check the monitor. [mcpd's alerting docs](https://mcpd.sh/docs/alerting) say monitors are evaluated every 60 s on `error_rate`, `latency_p95`, cost, `score_mean`, `trace_count` and step-count divergence. A monitor latches, fires once, resolves only on a real reading and calls your webhook.

 Silence isn't recovery: a quiet monitor with no readings in your LLM audit log doesn't mean the problem is gone.

For the double refund, the search returns one run. Its waterfall shows the refund tool span repeating, and the cost on each repeat shows where the spend went.

`no_tool_loop` has failed on that run, and the cost monitor's firing record shows when someone was notified. You end with one trace id that answers what the model saw, what it did and who was told.

## How do you redact prompts before they reach the audit log?

You redact prompts before they reach the audit log by running redaction rules before storage, so a secret is never written down in the first place. As the [mcpd security docs](https://mcpd.sh/docs/security) put it, a value deleted afterwards was still stored, so redaction runs before the write.

The OWASP GenAI project lists [LLM02:2025 Sensitive Information Disclosure](https://genai.owasp.org/llm-top-10/) and LLM07:2025 System Prompt Leakage in its Top 10 for LLM Applications. `gen_ai.system_instructions` is a content attribute alongside the message attributes, so system prompts need redaction rules too.

1. List what is sensitive. Start with prompts, completions, tool arguments and system instructions. Check tool spans as well, because the same card number often appears in a prompt and in a tool call.
2. Pick a mode per field:
 - drop, for a value you never need: the field is not stored.
 - mask, when you only need to know a value was there: it is replaced by a fixed placeholder.
 - partial, when a person must recognise the value, for example from the last characters of an account number.
 - keyed hash, for identifiers you join on: the same value gives the same keyed digest across runs, so you can match it without reading it.
3. Apply the rules to every signal. Spans, logs, metrics and the other signals all take them, because a user ID in a log line leaks as easily as one in a prompt.
4. Test with a fake secret. Send one run containing a made-up key, then search the payload text for it. A correct setup returns no match, and the redacted field shows its placeholder, fragment or digest.

The security docs also say that with a default configuration nothing leaves the machine, and that covers update checks, usage reporting and telemetry.

## How long should you keep an LLM audit log?

Keep an LLM audit log in two parts: the event record for months, and the prompt and completion content for a shorter, configured number of days, because the two parts fall under different rules.

[Article 12 of Regulation (EU) 2024/1689](https://aidefense.dev/posts/llm-audit-logging-what-to-log/) says high-risk AI systems "shall technically allow for the automatic recording of events (logs) over the lifetime of the system".

GDPR pulls the other way for the content of an LLM audit log.

A prompt can hold a name or an order number, so the content part of an LLM audit log gets the short clock. Trace id, span names, model, token counts, cost and timestamps hold no prompt text, so they get the long one.

| Part of the record | What it holds | Kept for |
| --- | --- | --- |
| Event | trace id, span names, model, token counts, cost, timestamps | N months (at least 6 under Article 19, if it applies to you) |
| Content | prompts, completions, tool arguments | M days, with M shorter than the event period |

In mcpd, retention is set in days, and erasure requests about a person are supported, as [mcpd's security docs](https://mcpd.sh/docs/security) describe. When someone asks for erasure, you delete their content at once instead of waiting for the M-day clock to expire.

This is not legal advice. Whether Articles 12 and 19 apply depends on your system's risk class and your role.

## Who can read the audit log, and who audits the readers?

Access to an LLM audit log in mcpd is controlled by scoped API keys and account roles, and accounts add a separate audit log of the product itself. The run record says what the model received and returned, and the access controls decide who can reach that record.

The [mcpd security docs](https://mcpd.sh/docs/security) say API keys carry one of six scopes: ingest, read, write, execute, worker and admin. The daemon compares keys as SHA-256 hashes, and each key is scoped to a project.

An ingest-only key can write to the LLM audit log but can't read it back, so it's the key to give an agent.

The `mcpd mcp` server follows the same rules, so a support agent reading traces through it gets the access its key grants and nothing more. With no keys set, ingest and reads are open, which suits a laptop only.

For a team, accounts add organisations with four roles: viewer, member, admin and owner.

SCIM and quotas come with accounts too. The security docs list what the product's audit log records.

If your security team reviews access in a SIEM, forward the run record there as OTLP, so sign-ins and the runs those users could open sit in one system.

## Frequently asked questions

### What is an LLM audit log, and how is it different from LLM tracing?

Tracing and an LLM audit log hold the same data and answer different questions. Tracing finds the bad run, while the audit log must also show that the record was retained, redacted and access-controlled, so you can answer an auditor as well as debug.

### How long should you keep an LLM audit log?

Keep event metadata for as long as your obligations require, and prompt and completion content for a shorter, configured number of days. Article 19 of the EU AI Act covers how long providers of high-risk AI systems keep automatically generated logs, and GDPR limits how long personal data stays identifiable.

### Do I have to store prompts and completions?

No. Content capture is opt-in in the OpenTelemetry GenAI conventions, so you store only what you choose to. Redact or drop what you don't need before it is written, and keep the event metadata either way.

### Can the audit log slow down or break my agent?

Not when it runs beside the agent. mcpd is a daemon fed by OpenTelemetry, and nothing travels through it to reach a model. The trade-off is that it gives no commit-before-response guarantee.

### Can I send the audit log to my existing SIEM?

Yes. mcpd forwards what it stores as OTLP to any endpoint you name, including the collector in front of your SIEM.

## Next steps: your first audit trail today

In five minutes, set `OTEL_EXPORTER_OTLP_ENDPOINT` to `https://api.mcpd.sh`, or build from [github.com/loruorg/mcpd](https://github.com/loruorg/mcpd), and run a real agent. Open its trace and follow it from the root span to the last tool call.

This week, set a redaction mode for each sensitive field, then set the content retention days. Keep the event clock and the content clock separate, so the event record outlasts the prompt text.

This month, create scoped keys with an ingest-only key for each agent, and forward OTLP to your SIEM so your LLM audit log sits beside your other security records.

Your first llm audit log needs one exporter setting and one agent run. [Start free](https://mcpd.sh/app) on the hosted version and point the exporter at it.

## Sources

[mcpd · The unopinionated agent observatory.](https://mcpd.sh) · [Privacy · mcpd](https://mcpd.sh/docs/privacy) · [OpenTelemetry · mcpd](https://mcpd.sh/docs/opentelemetry) · [Arize-ai/openinference](https://github.com/Arize-ai/openinference) · [Cost · mcpd](https://mcpd.sh/docs/cost) · [mcpd README](https://github.com/loruorg/mcpd) · [Operations · mcpd](https://mcpd.sh/docs/operations) · [Security · mcpd](https://mcpd.sh/docs/security). Every other source is linked where the article cites it.
