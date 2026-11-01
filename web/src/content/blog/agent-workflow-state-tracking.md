---
description: What is agent workflow state tracking? Record the cursor, retries, waits and payloads as OpenTelemetry traces. Learn to trace a stuck run in any framework.
faq:
  - answer: Emit OpenTelemetry spans using the gen_ai.* conventions and point the OTLP exporter at a collector such as mcpd. Each run becomes a span tree with timings, tokens and cost, so you read state from the outside. It works with any framework or language that emits OpenTelemetry.
    question: How do I track agent workflow state in any framework?
  - answer: "Observed state is the trace of what happened during a run, and durable state is what the daemon holds between steps: the cursor, retries, timers and any wait_event. The next step reads durable state, and replaying a trace never writes to it."
    question: What is the difference between observed state and durable state?
  - answer: "When a worker dies mid-step, the step's lease expires and another worker claims the step from the run the daemon still holds. A wait_event stays with the daemon too, so a worker dying doesn't lose a wait of hours or days."
    question: What happens to a workflow when a worker dies mid-step?
  - answer: Attach the no_tool_loop scorer and a step-count divergence monitor, then search for what the model said. The monitor latches, fires once and resolves only on a real reading. Open the flagged run and look for repeated sibling spans in its tree.
    question: How do I find the run where my agent looped?
  - answer: "State tracking stores prompts, completions and tool results, because they are the payloads of each step. Redaction runs before storage (drop, mask, partial or hash), so a secret that was never written can't leak later. Retention, set in days, limits how long the rest stays."
    question: Does state tracking store prompts and secrets, and how do I redact them?
image: /blog/agent-workflow-state-tracking/featured-0eae7b23d646.webp
image_alt: "Illustration for Agent workflow state tracking: Observe any agent"
publishDate: "2026-11-01"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/agent-workflow-state-tracking#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q627335\",\"@type\":\"Thing\",\"description\":\"defined sequence of activities of an organizational entity, enabled by the systematic organization of resources into processes that transform materials, provide services, or process information\",\"name\":\"Workflow\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Workflow\",\"https://www.wikidata.org/wiki/Q627335\",\"https://www.google.com/search?kgmid=/m/0k5rb\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q2283\",\"@type\":\"Organization\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]}],\"datePublished\":\"2026-11-01\",\"description\":\"What is agent workflow state tracking? Record the cursor, retries, waits and payloads as OpenTelemetry traces. Learn to trace a stuck run in any framework.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Emit OpenTelemetry spans using the gen_ai.* conventions and point the OTLP exporter at a collector such as mcpd. Each run becomes a span tree with timings, tokens and cost, so you read state from the outside. It works with any framework or language that emits OpenTelemetry.\"},\"name\":\"How do I track agent workflow state in any framework?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Observed state is the trace of what happened during a run, and durable state is what the daemon holds between steps: the cursor, retries, timers and any wait_event. The next step reads durable state, and replaying a trace never writes to it.\"},\"name\":\"What is the difference between observed state and durable state?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"When a worker dies mid-step, the step's lease expires and another worker claims the step from the run the daemon still holds. A wait_event stays with the daemon too, so a worker dying doesn't lose a wait of hours or days.\"},\"name\":\"What happens to a workflow when a worker dies mid-step?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Attach the no_tool_loop scorer and a step-count divergence monitor, then search for what the model said. The monitor latches, fires once and resolves only on a real reading. Open the flagged run and look for repeated sibling spans in its tree.\"},\"name\":\"How do I find the run where my agent looped?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"State tracking stores prompts, completions and tool results, because they are the payloads of each step. Redaction runs before storage (drop, mask, partial or hash), so a secret that was never written can't leak later. Retention, set in days, limits how long the rest stays.\"},\"name\":\"Does state tracking store prompts and secrets, and how do I redact them?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q165194\",\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q364\",\"@type\":\"Organization\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q20706915\",\"@type\":\"Thing\",\"description\":\"data storage paradigm designed for storing, retrieving, and managing associative arrays\",\"name\":\"Key–value database\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Key%E2%80%93value_database\",\"https://www.wikidata.org/wiki/Q20706915\",\"https://www.google.com/search?kgmid=/m/01319xfd\"]}],\"name\":\"Agent workflow state tracking: Observe any agent\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/5Mo1e-llHqD9fAPsuMg5O55eeqxhjXr5.webp\"},\"url\":\"https://mcpd.sh/blog/agent-workflow-state-tracking\"},{\"@id\":\"https://mcpd.sh/blog/agent-workflow-state-tracking#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q627335\"},{\"@id\":\"https://www.wikidata.org/wiki/Q2283\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory.\",\"url\":\"https://mcpd.sh\"},{\"@type\":\"CreativeWork\",\"name\":\"Microsoft Agent Framework Workflows - State | Microsoft Learn\",\"url\":\"https://learn.microsoft.com/en-us/agent-framework/concepts/workflows/state\"},{\"@type\":\"CreativeWork\",\"name\":\"Workflows · mcpd\",\"url\":\"https://mcpd.sh/docs/workflows\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry Protocol Exporter\",\"url\":\"https://opentelemetry.io/docs/specs/otel/protocol/exporter/\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenInference semantic conventions\",\"url\":\"https://github.com/Arize-ai/openinference/blob/main/spec/semantic_conventions.md\"},{\"@type\":\"CreativeWork\",\"name\":\"OTLP Specification\",\"url\":\"https://opentelemetry.io/docs/specs/otlp/\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"Microsoft Agent Framework: Checkpoints\",\"url\":\"https://learn.microsoft.com/en-us/agent-framework/workflows/checkpoints\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd runs\",\"url\":\"https://mcpd.sh/docs/runs\"},{\"@type\":\"CreativeWork\",\"name\":\"Microsoft Agent Framework: Workflows as Agents\",\"url\":\"https://learn.microsoft.com/en-us/agent-framework/workflows/as-agents\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd alerting docs\",\"url\":\"https://mcpd.sh/docs/alerting\"},{\"@type\":\"CreativeWork\",\"name\":\"Model Context Protocol Specification: Tools\",\"url\":\"https://modelcontextprotocol.io/specification/2025-06-18/server/tools\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd cost documentation\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd privacy documentation\",\"url\":\"https://mcpd.sh/docs/privacy\"}],\"dateModified\":\"2026-11-01\",\"datePublished\":\"2026-11-01\",\"description\":\"What is agent workflow state tracking? Record the cursor, retries, waits and payloads as OpenTelemetry traces. Learn to trace a stuck run in any framework.\",\"headline\":\"Agent workflow state tracking: Observe any agent\",\"image\":\"https://api.birchseek.com/m/5Mo1e-llHqD9fAPsuMg5O55eeqxhjXr5.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/agent-workflow-state-tracking#webpage\"},\"keywords\":[\"agent workflow state tracking\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/agent-workflow-state-tracking#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q165194\"},{\"@id\":\"https://www.wikidata.org/wiki/Q364\"},{\"@id\":\"https://www.wikidata.org/wiki/Q20706915\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2595},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"defined sequence of activities of an organizational entity, enabled by the systematic organization of resources into processes that transform materials, provide services, or process information\",\"name\":\"Workflow\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Workflow\",\"https://www.wikidata.org/wiki/Q627335\",\"https://www.google.com/search?kgmid=/m/0k5rb\"]},{\"@type\":\"Thing\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]},{\"@type\":\"Thing\",\"description\":\"collection of data, publication type\",\"name\":\"Data set\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Data_set\",\"https://www.wikidata.org/wiki/Q1172284\",\"https://www.google.com/search?kgmid=/m/02cck\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"benchmark in natural language understanding\",\"name\":\"General Language Understanding Evaluation benchmark\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q64711428\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"large language model company\",\"name\":\"LlamaIndex\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LlamaIndex\",\"https://www.wikidata.org/wiki/Q125446413\"]},{\"@type\":\"Thing\",\"description\":\"Chinese artificial intelligence company\",\"name\":\"DeepSeek\",\"sameAs\":[\"https://en.wikipedia.org/wiki/DeepSeek\",\"https://www.wikidata.org/wiki/Q131577453\",\"https://www.google.com/search?kgmid=/g/11wvrb0s91\"]},{\"@type\":\"Thing\",\"description\":\"language understanding across multiple domains\",\"name\":\"MMLU\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MMLU\",\"https://www.wikidata.org/wiki/Q123721276\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"family of multimodal artificial intelligence models developed by OpenAI\",\"name\":\"GPT-4o\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GPT-4o\",\"https://www.wikidata.org/wiki/Q125919502\",\"https://www.google.com/search?kgmid=/g/11vxhg1zhp\"]},{\"@type\":\"Thing\",\"description\":\"family of large language models developed by OpenAI\",\"name\":\"GPT-5\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GPT-5\",\"https://www.wikidata.org/wiki/Q135297698\"]},{\"@type\":\"Thing\",\"description\":\"a lightweight markup language with plain-text formatting syntax that is popularly used\",\"name\":\"Markdown\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Markdown\",\"https://www.wikidata.org/wiki/Q1193600\",\"https://www.google.com/search?kgmid=/m/0jt0gkj\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - agent observability
  - opentelemetry
  - durable workflows
  - ai agents
  - tracing
title: "Agent workflow state tracking: Observe any agent"
---

**Agent workflow state tracking records four kinds of state: the cursor (where the run is), retries and timers, waits for external events, and step payloads. You observe them by exporting every run as an OpenTelemetry trace, whose `gen_ai.*` spans can be read from outside the agent in any framework. Durable state, which the daemon holds between steps so the run can continue, is kept separate from that trace.**

An agent passes on your laptop, then in production it loops, stalls on a wait or hands a wrong value to the next step. The only record is scattered logs. The problem belongs to the one-engineer team hunting one bad run among thousands, and to the platform team that needs retention and redaction.

[mcpd](https://mcpd.sh) watches any agent in any framework or language, with nothing to adopt. After reading, you can name each kind of state, observe it from outside the agent, make it survive a crash, and trace a failure to the run behind it.

> **Key Takeaways**
> - Give the cursor and the payloads different lifetimes: the cursor must survive a crash, while completions should expire.
> - Replaying a trace must never write to durable state, or model calls and tool calls run twice.
> - Use monitors that latch, so a quiet hour with no runs never clears a fired monitor.
> - Leave an unknown model's cost empty, so a missing price never passes a cost budget as zero.

## What is agent workflow state tracking?

Agent workflow state tracking means recording everything a workflow must remember to continue, so a run can be inspected, resumed or explained. That state comes in four kinds, and each behaves differently:

![Illustration for What is agent workflow state tracking?](/blog/agent-workflow-state-tracking/image-1-c27b59259fe9.webp)

- The cursor: which step the run is on and which steps are finished. Lose it and a restart repeats work or skips it.
- Retries and timers: how many attempts a step has used, when the next attempt is due, and when a sleep ends.
- Waits for external events: a run paused until an approval, a webhook or a reply arrives. The wait can last hours or days.
- Payloads: the prompt sent, the completion returned and the tool result. These are the largest values and the most sensitive.

The cursor changes at every step. A wait can sit untouched for days. A payload is written once and read later by a person, so it can't share the cursor's rules.

### One framework's key-value answer

[Microsoft Agent Framework's state documentation](https://learn.microsoft.com/en-us/agent-framework/concepts/workflows/state) says that "State allows multiple executors within a workflow to access and modify common data." Its Python API is `WorkflowContext.set_state()` and `get_state()`, and its. NET API is `QueueStateUpdateAsync` and `ReadStateAsync`. That key-value store answers where shared data lives.

If you keep all four kinds as keys in one such store, they'll share one lifetime, one write path and one redaction rule. A cursor that must survive a crash sits beside a completion that should be deleted after 30 days. Nothing in the store says which key is which kind, so agent workflow state tracking can't tell them apart.

### Where each kind lives in mcpd

mcpd keeps the kinds apart. [mcpd's workflow documentation](https://mcpd.sh/docs/workflows) puts it this way: the cursor, retries, timers and waits live in the durable run, which the daemon holds between steps.

Payloads live on spans, the records of each model call, tool call and retrieval that make up the trace. The [concepts page](https://mcpd.sh/docs/concepts) defines traces and spans in full.

For agent workflow state tracking, the split gives two kinds of state. Observed state is what happened, and traces record it. Durable state is what the daemon holds between steps so the next step can run, and replaying a trace must never write to it.

## How do you track agent workflow state from outside the agent?

You track agent workflow state from outside the agent by exporting each run as an OpenTelemetry trace, so the span tree becomes the state history. The cursor is the span you are inside, retries are repeated sibling spans, and payloads are span attributes. Nothing in the agent's framework or language changes.

1. Create a project and a key. Sign up at [mcpd.sh/app](https://mcpd.sh/app), create a project and copy its key. To self-host, build from source at `github.com/loruorg/mcpd` instead. A key from one project writes only to that project, so use a separate project for each environment.

2. Set the exporter variables in the agent's environment, then restart the process, because most SDKs read them once at start-up.

```shell
export OTEL_SERVICE_NAME=refund-agent
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

`OTEL_SERVICE_NAME` labels every run so you can filter by agent, and `OTEL_EXPORTER_OTLP_ENDPOINT` is where spans are sent. The [OpenTelemetry exporter specification](https://opentelemetry.io/docs/specs/otel/protocol/exporter/) lists the protocol value as `http/json`, which selects the JSON encoding.

Add the project key to the exporter's headers as [mcpd's OpenTelemetry guide](https://mcpd.sh/docs/opentelemetry) describes. Leave any trailing path off the endpoint: the exporter appends its own signal path.

3. Emit the span and attribute names that carry state.

Model calls carry `gen_ai.request.model`. Most framework instrumentation libraries already write these, so you add no code.

4. Accept both conventions. OpenInference publishes a separate set of conventions, such as `llm.token_count.prompt`, in its [semantic conventions](https://github.com/Arize-ai/openinference/blob/main/spec/semantic_conventions.md). mcpd reads the OpenTelemetry `gen_ai.*` names and the OpenInference `llm.*` names, so existing instrumentation works unchanged.

5. Run the agent and open the trace. Within a minute or two of a run finishing, it appears in the interface as a waterfall on one clock, with timings, tokens and cost per step. A healthy run shows a root span with the agent's steps nested under it, each showing its prompt and completion.

A run that's still going shows the spans it's finished so far. That's how you inspect a stuck one.

If a custom exporter's runs never appear, check the IDs first. The [OTLP specification](https://opentelemetry.io/docs/specs/otlp/) says trace and span IDs in JSON are hex strings and are "not base64-encoded like is defined in the standard Protobuf JSON Encoding".

The [integration guide](https://mcpd.sh/docs/integrating) compares this route with the HTTP API and the SDKs. To try it, [start free at mcpd.sh/app](https://mcpd.sh/app).

## Observed state or durable state: what survives a crash?

Durable state survives a crash: it is what a workflow holds between steps so it can continue, while observed state is the trace of what happened.

| | Microsoft Agent Framework | mcpd |
| --- | --- | --- |
| Where state is held | Checkpoints saved by the framework | The daemon, outside your worker |
| What resumes | Workflow from a checkpoint | Run from its cursor |
| Unit of recovery | Superstep | One step under a lease |
| How a long wait is held | Not covered here | `wait_event` held by the daemon |

Mixing the two breaks agent workflow state tracking in specific ways. A trace used as a resume point replays model calls and tool calls that already ran. A checkpoint used as a debugging record has no timings, tokens or cost, so it can't tell you why a run looped.

[Microsoft's checkpoint documentation](https://learn.microsoft.com/en-us/agent-framework/workflows/checkpoints) describes checkpoints that save the state of a workflow at specific points during its execution so it can resume from those points later.

In that model, your process and the framework share one runtime, and you configure the checkpoint store yourself.

mcpd splits the roles. The [workflow definition](https://mcpd.sh/docs/workflows) uses nine constructs: task, agent, sleep, sleep_until, wait_event, parallel, foreach, loop and subworkflow. The daemon holds the cursor, retries, timers and waits for an event that may take a week.

A worker claims one step at a time under a lease, and never holds the record of progress itself.

[Every durable run is also a trace](https://mcpd.sh/docs/runs), so the same run gives you the observed history.

During a wait of hours or days, the run stays at its `wait_event` step in the daemon until the event arrives or a `sleep_until` timer fires. Then a worker claims the next step.

### A crash in each model

In Microsoft Agent Framework, a workflow can be used as an agent. Its [workflows-as-agents documentation](https://learn.microsoft.com/en-us/agent-framework/workflows/as-agents) says a serialized session carries the inner workflow checkpoint, and inner agents must keep the same id and name when rehydrated. Rename an inner agent between deployments and the saved state no longer matches it.

In mcpd, the equivalent failure is a worker killed halfway through a step. The worker never held the record of progress, so another worker claims the step once the lease expires, and the cursor is unchanged.

Microsoft Agent Framework suits teams that build the whole workflow in that framework and want recovery inside it. mcpd suits agents in any language or framework, where the daemon should hold the cursor and long waits while your code stays unchanged.

## How do you trace a looping or stuck agent to the run behind it?

You trace a looping or stuck agent to its run by letting a monitor or scorer flag the deviation, searching for what the model said, and reading that run's span tree.

1. Set a step-count divergence monitor. The [alerting docs](https://mcpd.sh/docs/alerting) describe a monitor that is evaluated every 60 s, latches, fires once, resolves only on a real reading, and calls your webhook. Silence is not recovery: a quiet hour with no runs does not clear it.

Point the webhook at a channel someone reads.

2. Score every run with `no_tool_loop`. A run that repeats the same tool call fails it, which gives you a per-run flag to filter on, where the monitor gives you a rate.

3. Search by what the model said. [mcpd's search](https://mcpd.sh/) finds a run by what the model said, so take a phrase from the complaint or from the agent's last message, such as "let me check that order again".

Searching labels alone usually misses the run, because a loop rarely carries one.

4. Read the span tree. Open the run in the [runs view](https://mcpd.sh/docs/runs). A loop appears as repeated sibling spans, and a stuck wait as a span that never closes. For a wrong value, find the first span whose output holds it, then read the next step's input to confirm it was handed forward unchanged.

5. Keep the run as a test. The [testing guide](https://mcpd.sh/docs/testing) covers turning a production run into a regression case, so the fix is checked against the run that exposed it.

### A worked example of agent workflow state tracking

If a refund agent calls `lookup_order` nine times for one customer, the divergence monitor fires and `no_tool_loop` marks that run as failed. A search for the agent's final sentence returns that one run.

Its tree shows nine sibling tool spans with the same input. An empty order ID in the third span's output, retried by every later call, is the cause to fix.

### Hand agent workflow state tracking to a coding or support agent

Run `mcpd mcp` and the same investigation is available to another agent.

The [Model Context Protocol specification](https://modelcontextprotocol.io/specification/2025-06-18/server/tools) says servers use tools so that "language models" can invoke them, so a coding assistant can search for the failing phrase and fetch the run itself. A key with `read` scope can look but not change anything.

## What does agent workflow state tracking cost, and how do you keep it private?

Agent workflow state tracking costs tokens and holds personal data, because the payloads of each step (prompts, completions and tool results) are the state.

In agent workflow state tracking, tokens grow when you pass the full history forward, because each step resends everything before it. Step 2 sends its own prompt plus the prompt and answer of step 1. Step 3 carries steps 1 and 2 as well.

The input of a late step is every earlier step plus its new content, so input tokens climb at each step even when each step adds little. A large tool result is the usual cause, since every later step sends it again.

[mcpd's cost documentation](https://mcpd.sh/docs/cost) says mcpd prices each step once, at ingest, from a price book of per-million-token rates. The book handles cached tokens, long-prompt tiers and service tiers, and you can supply your own. Reading cost per step shows which step the growing history made expensive.

An unknown model has no cost, not a cost of zero. A zero would pass a cost budget and hide the gap. An empty value tells you the model is missing from the book.

Privacy follows the same payloads. Redaction runs before the write, because a secret deleted afterwards was still written down. [mcpd's privacy documentation](https://mcpd.sh/docs/privacy) lists four modes, applied before storage on every signal:

- `drop` removes the value.
- `mask` replaces it with a placeholder.
- `partial` keeps only part of the value.
- `hash` replaces it with a hash made with a secret key, so equal values still match across runs.

Retention is set in days, so old runs expire without anyone deleting them by hand. When a person asks you to erase their data, mcpd handles erasure requests about a person.

The [mcpd security documentation](https://mcpd.sh/docs/security) covers keys, scopes and who can read a payload.

## Frequently asked questions

### How do I track agent workflow state in any framework?

Emit OpenTelemetry spans using the `gen_ai.*` conventions and point the OTLP exporter at a collector such as mcpd. Each run becomes a span tree with timings, tokens and cost, so you read state from the outside. It works with any framework or language that emits OpenTelemetry.

### What is the difference between observed state and durable state?

Observed state is the trace of what happened. Durable state is what the daemon holds between steps: the cursor, retries, timers and any `wait_event`. The next step reads durable state, and replaying a trace never writes to it.

### What happens to a workflow when a worker dies mid-step?

The step's lease expires, and another worker claims the step from the run the daemon still holds. A `wait_event` stays with the daemon too, so a worker dying doesn't lose a wait of hours or days.

### How do I find the run where my agent looped?

Attach the `no_tool_loop` scorer and a step-count divergence monitor, then search for what the model said. The monitor latches, fires once and resolves only on a real reading. Open the flagged run and look for repeated sibling spans in its tree.

### Does state tracking store prompts and secrets, and how do I redact them?

It stores prompts, completions and tool results, because they are the payloads of each step. [Redaction runs before storage](https://mcpd.sh/docs/privacy) (drop, mask, partial or hash), so a secret that was never written can't leak later. Retention, set in days, limits how long the rest stays.

## Track your first workflow run in five minutes

Add agent workflow state tracking in layers, starting with the trace.

- In five minutes: set `OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh` and `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`, add the key from your project, and run the agent once. Open the first span tree and find the cursor, the retries and the payload of each step.
- In an hour: add a step-count divergence monitor, so a run that takes far more steps than usual raises a flag. Attach the `no_tool_loop` scorer to catch repeated tool calls.
- In a week: choose redaction modes and a retention period before sensitive payloads arrive.

Two start routes work today: the hosted version, or a build from source at `github.com/loruorg/mcpd`. [Start free at mcpd.sh/app](https://mcpd.sh/app); you adopt no framework, and your agent code stays as it is. If you'd rather read first, the [quickstart](https://mcpd.sh/docs/quickstart) walks through the same first trace.

## Sources

[mcpd · The unopinionated agent observatory.](https://mcpd.sh) · [Microsoft Agent Framework Workflows - State | Microsoft…](https://learn.microsoft.com/en-us/agent-framework/concepts/workflows/state) · [Workflows · mcpd](https://mcpd.sh/docs/workflows) · [OpenTelemetry Protocol Exporter](https://opentelemetry.io/docs/specs/otel/protocol/exporter/) · [OpenInference semantic conventions](https://github.com/Arize-ai/openinference/blob/main/spec/semantic_conventions.md) · [OpenTelemetry · mcpd](https://mcpd.sh/docs/opentelemetry) · [mcpd cost documentation](https://mcpd.sh/docs/cost) · [mcpd privacy documentation](https://mcpd.sh/docs/privacy). Every other source is linked where the article cites it.
