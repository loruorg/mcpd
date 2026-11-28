---
description: What is AI agent infinite loop detection? Fingerprint each tool call, flag a repeat only when the result is unchanged. Learn to set it up with OpenTelemetry.
faq:
  - answer: It means a LangChain AgentExecutor reached its max_iterations or max_execution_time before the agent produced a final answer, so it stopped and returned its early-stop outcome. The cap contained the run and does not identify which call repeated, so check the trace for that.
    question: "What does \"agent stopped due to iteration limit or time limit\" mean?"
  - answer: Yes, an agent can loop even when your instructions are correct. A tool error, an ambiguous result or a missing stop condition leaves the model with nothing new to act on, so it repeats the same call. Give each task an explicit finish condition.
    question: Can an agent loop when my instructions are correct?
  - answer: "Detection adds no latency, because mcpd is an observer, not a gateway or a proxy. Nothing travels through it to reach a model, and recording a span never blocks or throws, so a slow or missing mcpd can't break the run."
    question: Does loop detection slow my agent down?
  - answer: "Store them only if you need them for fingerprinting, and redact before storage. mcpd redacts before the write, with four modes: drop, mask, partial or keyed hash. A keyed hash still lets you compare repeated calls without keeping the raw value."
    question: Should I store tool arguments?
image: /blog/ai-agent-infinite-loop-detection/featured-02c623520f91.webp
image_alt: Illustration for How to set up AI agent infinite loop detection
publishDate: "2026-11-28"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/ai-agent-infinite-loop-detection#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q862372\",\"@type\":\"Thing\",\"description\":\"programming idiom\",\"name\":\"Infinite loop\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Infinite_loop\",\"https://www.wikidata.org/wiki/Q862372\",\"https://www.google.com/search?kgmid=/m/0c9qt\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q107383183\",\"@type\":\"Thing\",\"description\":\"Python library\",\"name\":\"retry\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q107383183\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q117340550\",\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]}],\"datePublished\":\"2026-11-28\",\"description\":\"What is AI agent infinite loop detection? Fingerprint each tool call, flag a repeat only when the result is unchanged. Learn to set it up with OpenTelemetry.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"It means a LangChain AgentExecutor reached its max_iterations or max_execution_time before the agent produced a final answer, so it stopped and returned its early-stop outcome. The cap contained the run and does not identify which call repeated, so check the trace for that.\"},\"name\":\"What does \\\"agent stopped due to iteration limit or time limit\\\" mean?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Yes, an agent can loop even when your instructions are correct. A tool error, an ambiguous result or a missing stop condition leaves the model with nothing new to act on, so it repeats the same call. Give each task an explicit finish condition.\"},\"name\":\"Can an agent loop when my instructions are correct?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Detection adds no latency, because mcpd is an observer, not a gateway or a proxy. Nothing travels through it to reach a model, and recording a span never blocks or throws, so a slow or missing mcpd can't break the run.\"},\"name\":\"Does loop detection slow my agent down?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Store them only if you need them for fingerprinting, and redact before storage. mcpd redacts before the write, with four modes: drop, mask, partial or keyed hash. A keyed hash still lets you compare repeated calls without keeping the raw value.\"},\"name\":\"Should I store tool arguments?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q183427\",\"@type\":\"Thing\",\"description\":\"type of function that maps data of arbitrary size to data of fixed size\",\"name\":\"Hash function\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Hash_function\",\"https://www.wikidata.org/wiki/Q183427\",\"https://www.google.com/search?kgmid=/m/03l5h\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q140795851\",\"@type\":\"Organization\",\"description\":\"artificial intelligence model routing platform\",\"name\":\"OpenRouter\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenRouter\",\"https://www.wikidata.org/wiki/Q140795851\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q467707\",\"@type\":\"Thing\",\"description\":\"set of software development tools\",\"name\":\"Software development kit\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Software_development_kit\",\"https://www.wikidata.org/wiki/Q467707\",\"https://www.google.com/search?kgmid=/m/0164kt\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q132393291\",\"@type\":\"Thing\",\"description\":\"Python library\",\"name\":\"LangGraph\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q132393291\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q28865\",\"@type\":\"Thing\",\"description\":\"general-purpose programming language\",\"name\":\"Python\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Python_(programming_language)\",\"https://www.wikidata.org/wiki/Q28865\",\"https://www.google.com/search?kgmid=/m/05z1_\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q183635\",\"@type\":\"Thing\",\"description\":\"process in which information about the past or the present influences the same phenomenon in the present or future; it occurs when outputs of a system are routed back as inputs as part of a chain of cause-and-effect that forms a circuit or loop\",\"name\":\"Feedback\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Feedback\",\"https://www.wikidata.org/wiki/Q183635\",\"https://www.google.com/search?kgmid=/m/03232\"]}],\"name\":\"How to set up AI agent infinite loop detection\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/92FY3yiAFoy3ui-XERpzNsbUEgz5y8e6.webp\"},\"url\":\"https://mcpd.sh/blog/ai-agent-infinite-loop-detection\"},{\"@id\":\"https://mcpd.sh/blog/ai-agent-infinite-loop-detection#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q862372\"},{\"@id\":\"https://www.wikidata.org/wiki/Q107383183\"},{\"@id\":\"https://www.wikidata.org/wiki/Q117340550\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"mcpd README\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"AI Agent Infinite Loop: Why It Happens and Fixes\",\"url\":\"https://futureagi.com/blog/loop-engineering/ai-agent-stuck-in-loop/\"},{\"@type\":\"CreativeWork\",\"name\":\"AI Agent Infinite Loop Detection \\u0026 Prevention\",\"url\":\"https://inkog.io/glossary/infinite-loop-ai-agent\"},{\"@type\":\"CreativeWork\",\"name\":\"Cost · mcpd\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"How to Stop an AI Agent Stuck in an Infinite Loop\",\"url\":\"https://openrouter.ai/blog/tutorials/ai-agent-infinite-loop/\"},{\"@type\":\"CreativeWork\",\"name\":\"AI Agent Infinite Loop Detection: A Technical Guide\",\"url\":\"https://trussed.ai/resources/ai-agent-infinite-loop-detection-a-technical-guide\"},{\"@type\":\"CreativeWork\",\"name\":\"Privacy · mcpd\",\"url\":\"https://mcpd.sh/docs/privacy\"},{\"@type\":\"CreativeWork\",\"name\":\"Evaluation · mcpd\",\"url\":\"https://mcpd.sh/docs/evaluation\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"Alerting · mcpd\",\"url\":\"https://mcpd.sh/docs/alerting\"}],\"dateModified\":\"2026-11-28\",\"datePublished\":\"2026-11-28\",\"description\":\"What is AI agent infinite loop detection? Fingerprint each tool call, flag a repeat only when the result is unchanged. Learn to set it up with OpenTelemetry.\",\"headline\":\"How to set up AI agent infinite loop detection\",\"image\":\"https://api.birchseek.com/m/92FY3yiAFoy3ui-XERpzNsbUEgz5y8e6.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/ai-agent-infinite-loop-detection#webpage\"},\"keywords\":[\"ai agent infinite loop detection\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/ai-agent-infinite-loop-detection#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q183427\"},{\"@id\":\"https://www.wikidata.org/wiki/Q140795851\"},{\"@id\":\"https://www.wikidata.org/wiki/Q467707\"},{\"@id\":\"https://www.wikidata.org/wiki/Q132393291\"},{\"@id\":\"https://www.wikidata.org/wiki/Q28865\"},{\"@id\":\"https://www.wikidata.org/wiki/Q183635\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2739},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"general-purpose programming language\",\"name\":\"Python\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Python_(programming_language)\",\"https://www.wikidata.org/wiki/Q28865\",\"https://www.google.com/search?kgmid=/m/05z1_\"]},{\"@type\":\"Thing\",\"description\":\"permissive free software license\",\"name\":\"MIT License\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MIT_License\",\"https://www.wikidata.org/wiki/Q334661\",\"https://www.google.com/search?kgmid=/m/04vpm\"]},{\"@type\":\"Thing\",\"description\":\"software to manage containers on a server-cluster\",\"name\":\"Kubernetes\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Kubernetes\",\"https://www.wikidata.org/wiki/Q22661306\",\"https://www.google.com/search?kgmid=/g/11b7lxp79d\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence software company\",\"name\":\"Braintrust\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Braintrust\",\"https://www.wikidata.org/wiki/Q141164091\"]},{\"@type\":\"Thing\",\"description\":\"Python library\",\"name\":\"mlflow\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q107381745\"]},{\"@type\":\"Thing\",\"description\":\"Python library\",\"name\":\"langsmith\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q132393290\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"AI-based tool made by Anthropic\",\"name\":\"Claude Code\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_Code\",\"https://www.wikidata.org/wiki/Q138457287\"]},{\"@type\":\"Thing\",\"description\":\"Type of information retrieval using LLMs\",\"name\":\"Retrieval-augmented generation\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Retrieval-augmented_generation\",\"https://www.wikidata.org/wiki/Q121362277\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - ai agents
  - loop detection
  - opentelemetry
  - observability
  - langgraph
  - monitoring
title: How to set up AI agent infinite loop detection
---

**AI agent infinite loop detection is three jobs: spotting a repeat live, finding a looped run afterwards, and containing it. Live detection fingerprints each call (tool name plus canonicalised arguments) and counts a repeat once the previous result is seen. A step or cost cap, as [Future AGI's loop guide](https://futureagi.com/blog/loop-engineering/ai-agent-stuck-in-loop/) puts it, is a backstop, not a detector.** mcpd [records the run through OpenTelemetry](https://github.com/loruorg/mcpd), beside the agent, and a monitor alerts once.

Your LangGraph run ends in a `GraphRecursionError`, or the monthly bill is bigger than traffic explains. The cap fired and the run stopped, but it doesn't say which tool call repeated or what each repeat cost.

After reading, you can set up AI agent infinite loop detection that spots a loop live, finds a past one in recorded runs, and contains the next one.

> **Key Takeaways**
> - [Inkog's glossary](https://inkog.io/glossary/infinite-loop-ai-agent) names the caps to set: `max_iterations` on LangChain's AgentExecutor, the recursion limit in LangGraph.
> - Flag a repeat only when the result or state is also unchanged, since a backed-off retry or a moving poll is healthy.
> - An unknown model [has no cost in mcpd](https://mcpd.sh/docs/cost), not a cost of zero, so a missing price never reads as a free step.
> - Alert on step-count divergence rather than a fixed step number, because normal step counts differ by agent.

## How does AI agent infinite loop detection differ from a step cap?

AI agent infinite loop detection tells you which call repeated and when, while a step cap only stops a run after a counter runs out. Detection confirms each repeat against a progress signal before it flags anything.

![Illustration for How does AI agent infinite loop detection differ from a step cap?](/blog/ai-agent-infinite-loop-detection/image-1-d8c4df444952.webp)

The three jobs belong in different places:

- Live detection reads each call as it happens and flags a repeat while the run is still going. It belongs beside the agent.
- Post-hoc search finds the one looped run among thousands already recorded, using full-text search over the payload and deterministic scorers such as `no_tool_loop` and `tool_efficiency`. It belongs in the trace store.
- Containment ends the run with `max_iterations`, a recursion limit or a cost ceiling. It belongs inside the framework.

A run that dies at its step limit records the stop and nothing about its cause. Raising the limit only lets the same stuck agent run longer.

Repeats come in three shapes. [OpenRouter's loop guide](https://openrouter.ai/blog/tutorials/ai-agent-infinite-loop/) describes the most common one as a model that "keeps calling a tool that fails, usually with the same arguments". [Trussed's technical guide](https://trussed.ai/resources/ai-agent-infinite-loop-detection-a-technical-guide) describes circular delegation, where "Agent A cannot proceed until Agent B responds, and Agent B is waiting on Agent A".

A stalled state is the third: calls vary, but the agent's state stops changing between steps. [Inkog's glossary](https://inkog.io/glossary/infinite-loop-ai-agent) files this whole family under OWASP LLM10:2025 Unbounded Consumption.

mcpd watches from outside the agent's path, so a fault in the detector can't stall a run. Its [README](https://github.com/loruorg/mcpd) says: "It is not a gateway and not a proxy. Nothing travels through mcpd to reach a model, so mcpd being slow or down cannot slow your agent or break it."

## What counts as the same tool call in AI agent infinite loop detection?

A same tool call is one with the same tool name and the same canonicalised arguments, fingerprinted and compared inside a single run. Canonicalising means sorting object keys, collapsing whitespace and normalising types, then hashing the result.

Without it, `{"q": "refund", "limit": 5}` and `{"limit": "5", "q": " refund "}` hash differently although they ask for the same thing.

```python
import hashlib, json, re

def norm(v):
    if isinstance(v, dict):
        return {k: norm(v[k]) for k in sorted(v)}
    if isinstance(v, list):
        return [norm(x) for x in v]
    if isinstance(v, str):
        s = re.sub(r"\s+", " ", v).strip()
        return int(s) if s.isdigit() else s
    return v

def fingerprint(tool, args):
    blob = json.dumps([tool, norm(args)], separators=(",", ":"))
    return hashlib.sha256(blob.encode()).hexdigest()
```

Keep the set of fingerprints per run, never global, because the same search in two different runs is not a loop.

Add a fingerprint to the set only when its result comes back. A repeat is then a call whose fingerprint already has a result you have seen. Two identical calls issued in parallel within one step have no earlier result, so neither is flagged.

### Where exact match misses

A hash comparison misses three patterns:

- Rephrased calls. A model asks for "refund policy", then "policy for refunds", and the hashes differ. Normalise free-text fields (lowercase, sorted tokens) before hashing, or compare them by similarity.
- Volatile arguments. A timestamp, request ID or page cursor changes on every call. Drop volatile fields from the fingerprint, and treat a cursor that never advances as the signal.
- Cycles. Calls to tools A, B, A, B, or A, B, C, A, B, C, never repeat the previous call, so a check against the last fingerprint sees nothing.

For cycles, compare sequences of fingerprints. If the last N fingerprints equal the N before them, for N of 2 or 3, the run is repeating a pattern even though no adjacent pair matches.

## Is it a loop, a retry or a poll that is meant to repeat?

A loop differs from a retry or a poll because a loop repeats a call while nothing changes, whereas a retry or poll repeats a call that is expected to produce a different result or state. Check each pattern against its own progress signal before you flag it.

1. Retry after a 503 with backoff. The call repeats, but the gap between attempts grows and the failure is transient. Confirm that the delay increases and that the status eventually changes. Flag it only when the 503 outlasts the retry budget.

2. Repeated 400 with identical arguments. A 400 says the request itself was rejected, so sending it again can't succeed. An agent that keeps sending it is ignoring the error, which is a loop. Confirm that the fingerprint and the error result are both unchanged between attempts.

3. Polling a job status. The arguments repeat by design, but the result should change until a terminal state: queued, then running, then done. Confirm that the status or a progress field moved since the last poll. Flag it when the status sits unchanged for longer than the job should take, or when polling continues after the terminal state.

4. The same read tool called again. The previous result already answered the question, and the agent asks anyway. Confirm that the new result is identical to the last one and that no write happened in between. A re-read after a write is a legitimate check, because the state it reads has changed.

5. Acting on a flag. Act on a repeat count combined with an unchanged result or state, never on the count alone. Twelve polls with a moving status are healthy. Three identical 400s with identical arguments are not.

A monitor built on this rule stays quiet for the first and third cases, and fires on the second and fourth.

## What does a looping run look like in a trace?

A looping run in a trace shows the same tool-call span repeating with identical input, while each model span between the repeats carries more input tokens than the last. A step count far above the norm tells you which run to open first.

- Repeated tool-call spans are the first signal. In a loop, sibling `execute_tool` spans share the same tool name and the same arguments.

- Step count is the second signal. Count the spans in a run and compare them with runs of the same task. A run with several times the usual number of steps is the one to open first.

- Input tokens and cost climbing per step are the third. Input grows because the whole conversation is resent on every turn.

[OpenRouter's loop guide](https://openrouter.ai/blog/tutorials/ai-agent-infinite-loop/) puts it this way: "Each retry resends the whole conversation, so every turn costs more than the last."

mcpd prices each step once, at ingest, from a price book of per-million-token rates, with tiers for cached tokens and long prompts. A loop's bill is the sum of its steps, so each repeat visibly costs more than the one before.

### A walk-through

Three repeats of a `search_orders` call have this shape:

```text
chat           gen_ai.usage.input_tokens: lowest in the run
execute_tool   gen_ai.tool.name: search_orders, same arguments
chat           gen_ai.usage.input_tokens: higher
execute_tool   gen_ai.tool.name: search_orders, same arguments
chat           gen_ai.usage.input_tokens: higher again
execute_tool   gen_ai.tool.name: search_orders, same arguments
```

The tool name and arguments never change, and the step price rises down the list. Check the result of each call before you call it a loop: if the results differ, the agent is polling.

## How do you find the one looping run among thousands?

To find one looping run among thousands, search the recorded payload for the text that repeats, then score every past run with checks that need no model. Both steps work on stored runs, so nothing changes inside the agent.

Start with the words: a tool error such as "invalid date format", or the sentence the model kept writing.

Search gives you candidates, and scoring ranks the whole set. The built-in scorers `no_tool_loop` and `tool_efficiency` are deterministic: they read the recorded tool calls with plain code and never call a model.

Scores sit beside human feedback. [mcpd's evaluation docs](https://mcpd.sh/docs/evaluation) say, "A score is what a machine thought. Feedback is what a person thought. mcpd keeps them as two signals and never averages one into the other." 

A run can score badly on `tool_efficiency` and still carry thumbs-up feedback, and you read both.

Recorded tool arguments make fingerprinting possible, and they can hold secrets. [mcpd's privacy docs](https://mcpd.sh/docs/privacy) give the order: "Redaction runs before the write, because a secret deleted afterwards was still written down." The [security guide](https://mcpd.sh/docs/security) covers the controls around stored data.

mcpd can also forward the same data as OTLP to another backend, so the search and the scorers don't lock you in.

Once you've found the run, turn it into a test case. The [regression testing guide](https://mcpd.sh/docs/testing) shows how to compare two agent versions on that case, so a fix is checked against the loop that caused it.

## How do you set up AI agent infinite loop detection with OpenTelemetry?

Setting up AI agent infinite loop detection with OpenTelemetry takes three steps: send your agent's spans to mcpd over OTLP, confirm the tool spans arrive, then set your framework's step cap as the containment layer. It changes no agent code if you already emit `gen_ai.*` spans.

1. Point an OTLP exporter at mcpd. Create a project at [mcpd.sh/app](https://mcpd.sh/app), copy its key, and set these variables in the agent's environment. Add the key the way the [OpenTelemetry page in the mcpd docs](https://mcpd.sh/docs/opentelemetry) describes.

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

To self-host, build [the mcpd source on GitHub](https://github.com/loruorg/mcpd) and use your own daemon's address as the endpoint. mcpd reads both `gen_ai.*` and `llm.*` attributes, so existing instrumentation works unchanged.

2. Make one run and check the spans. Run the agent once with a task that needs two or three tools, then open the interface. You should see one trace, a model span per step and a tool-call span per call, each with its input.

If the list is empty, the usual cause is a protocol mismatch: the endpoint expects `http/json` here. A custom loop needs spans created by an OpenTelemetry library or one of the [mcpd integration routes](https://mcpd.sh/docs/integrating).

3. Set the framework cap. The cap ends a run that doesn't stop, but it can't tell you which call repeated. Inkog's glossary lists an [AgentExecutor with `max_iterations=None` and no time limit, and LangGraph cycles that rely only on the recursion limit](https://inkog.io/glossary/infinite-loop-ai-agent), among common causes of runaway loops.

| Framework | Setting | What it does when hit |
|---|---|---|
| LangChain `AgentExecutor` | `max_iterations` | Stops after that many agent steps |
| LangChain `AgentExecutor` | `max_execution_time` | Stops after that much wall-clock time |
| LangChain `AgentExecutor` | `early_stopping_method` | Chooses what the executor returns when it stops early |
| LangGraph | `recursion_limit` | Raises `GraphRecursionError` |
| OpenAI Agents SDK | `max_turns` | Raises `MaxTurnsExceeded` |
| CrewAI | `max_iter` | Limits iterations per agent |

Catch the error so a cap firing is logged, not swallowed:

```python
try:
    graph.invoke(inputs, {"recursion_limit": 25})
except GraphRecursionError:
    log.error("step cap reached")
```

The `25` is your choice, not a framework default. Set it just above the step count of a normal run from step 2.

When the cap fires, the trace ends at the last step and shows why the run stopped. The cause sits in the middle of the trace: the same tool-call span repeating with identical input.

Read the [quickstart](https://mcpd.sh/docs/quickstart) for the full first-trace walkthrough, or [start free at mcpd.sh/app](https://mcpd.sh/app) to record your first capped run.

## When should an AI agent infinite loop detection alert fire and resolve?

A loop alert should fire when a signal leaves your agent's normal range and resolve only when a fresh reading shows it back inside that range. Step count, cost and trace count are the signals to watch.

Step count needs a relative threshold, because a fixed number fits one agent and misfires on another. A research agent may legitimately take 40 steps where a lookup agent never passes 6.

[mcpd's alerting docs](https://mcpd.sh/docs/alerting) list step-count divergence as a monitor metric. It flags runs that depart from the usual step count instead of crossing one number you guessed.

Cost needs a fleet-level check as well as a per-run budget. A cost monitor is the fleet-level signal, and it catches ten slow loops that each stay under a per-run budget but together drain the bill.

A sudden rise in `trace_count` can mean an agent is re-triggering itself or a client is retrying in a storm. A fall to zero means no spans are arriving, which is a different problem.

They latch: a monitor fires once and calls your webhook. A loop that runs for an hour is one page, where an alert that re-fires on every evaluation would page again and again for the same incident.

Resolution works the same way in reverse. A monitor resolves only on a real reading that is back within its threshold, never because data stopped arriving.

If someone kills the agent, or the exporter breaks, no new spans come in. That silence says nothing about the loop, so the monitor stays open until a run reports normal numbers.

## Frequently asked questions

### What does "agent stopped due to iteration limit or time limit" mean?

According to [Inkog's glossary](https://inkog.io/glossary/infinite-loop-ai-agent), it means a LangChain AgentExecutor reached its `max_iterations` or `max_execution_time` before the agent produced a final answer, so it stopped and returned its early-stop outcome. The cap contained the run without saying which call repeated, so check the trace for the repeating span.

### Can an agent loop when my instructions are correct?

Correct instructions don't prevent a loop. A tool error, an ambiguous result or a missing stop condition leaves the model with nothing new to act on, so it repeats the same call. Give each task an explicit finish condition.

### Does loop detection slow my agent down?

Detection adds no latency, because mcpd is an observer, not a gateway or a proxy. Nothing travels through it to reach a model, and the [mcpd README](https://github.com/loruorg/mcpd) states that recording a span never blocks or throws, so a slow or missing mcpd can't break the run.

### Should I store tool arguments?

Store them only if you need them for fingerprinting, and redact before storage. [mcpd's privacy docs](https://mcpd.sh/docs/privacy) say mcpd redacts before the write, with four modes: drop, mask, partial or keyed hash. A keyed hash still lets you compare repeated calls without keeping the raw value.

## Record your next looping run and catch it with a monitor

To start AI agent infinite loop detection, pick one agent and point its OpenTelemetry exporter at mcpd. Set `OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh` with a key from your project, then run the agent until it next misbehaves, or replay a run you know looped.

Keep your framework cap (`max_iterations`, `recursion_limit`, `max_iter` or `max_turns`) as the backstop, and add one step-count monitor with a webhook.

When the monitor fires, search the run for the repeating error, score it with `no_tool_loop`, and save it as a regression case before you ship the fix. [Start free](https://mcpd.sh/app) on the hosted version, or build from source at github.com/loruorg/mcpd.

## Sources

[AI Agent Infinite Loop: Why It Happens and Fixes](https://futureagi.com/blog/loop-engineering/ai-agent-stuck-in-loop/) · [mcpd README](https://github.com/loruorg/mcpd) · [AI Agent Infinite Loop Detection & Prevention](https://inkog.io/glossary/infinite-loop-ai-agent) · [Cost · mcpd](https://mcpd.sh/docs/cost) · [How to Stop an AI Agent Stuck in an Infinite Loop](https://openrouter.ai/blog/tutorials/ai-agent-infinite-loop/) · [AI Agent Infinite Loop Detection: A Technical Guide](https://trussed.ai/resources/ai-agent-infinite-loop-detection-a-technical-guide) · [Privacy · mcpd](https://mcpd.sh/docs/privacy) · [Alerting · mcpd](https://mcpd.sh/docs/alerting). Every other source is linked where the article cites it.
