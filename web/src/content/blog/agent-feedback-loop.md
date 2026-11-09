---
description: "What is an agent feedback loop? Two loops: a runtime loop inside each run and an improvement loop across runs. Learn to set both up with OpenTelemetry."
faq:
  - answer: The agent loop is the inner cycle inside one run, while the feedback loop in the improvement sense spans many runs. In the inner cycle, the agent calls the model, runs a tool, observes the result, and repeats until done or capped. The feedback loop collects evidence, changes one thing, compares against a baseline, and releases.
    question: "What's the difference between an agent loop and an agent feedback loop?"
  - answer: "You don't need an SDK or a specific framework. Point the standard OpenTelemetry exporter variables, such as OTEL_EXPORTER_OTLP_ENDPOINT, at an endpoint, and any framework, language or model provider that emits spans works. Our OpenTelemetry guide covers the details of that setup."
    question: Do I need an SDK or a specific framework to build one?
  - answer: "You can trust an LLM judge only partly, because Microsoft's agent framework docs warn that an evaluator can be probabilistic. Use deterministic scorers wherever a rule can decide the outcome, and check the judge against human feedback on a sample of its scores."
    question: Can I trust an LLM judge to score my agent?
  - answer: Every loop needs a cap, and the right number depends on the task. The framework docs say to always bound autonomous loops. Read step counts from real runs before you pick a number, then set the cap for that task.
    question: How many iterations should an agent be allowed?
  - answer: Improving an agent usually does not mean retraining the model, because most fixes change the prompt, a tool, retrieval or routing. Whichever of those you change, you compare the candidate with the baseline on a dataset before release. That comparison shows whether the change helped.
    question: Does improving an agent mean retraining the model?
image: /blog/agent-feedback-loop/featured-019f938648f8.webp
image_alt: "Illustration for Agent feedback loop: From bad run to safe release"
publishDate: "2026-11-09"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/agent-feedback-loop#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q183635\",\"@type\":\"Thing\",\"description\":\"process in which information about the past or the present influences the same phenomenon in the present or future; it occurs when outputs of a system are routed back as inputs as part of a chain of cause-and-effect that forms a circuit or loop\",\"name\":\"Feedback\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Feedback\",\"https://www.wikidata.org/wiki/Q183635\",\"https://www.google.com/search?kgmid=/m/03232\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\",\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]}],\"datePublished\":\"2026-11-09\",\"description\":\"What is an agent feedback loop? Two loops: a runtime loop inside each run and an improvement loop across runs. Learn to set both up with OpenTelemetry.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"The agent loop is the inner cycle inside one run, while the feedback loop in the improvement sense spans many runs. In the inner cycle, the agent calls the model, runs a tool, observes the result, and repeats until done or capped. The feedback loop collects evidence, changes one thing, compares against a baseline, and releases.\"},\"name\":\"What's the difference between an agent loop and an agent feedback loop?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You don't need an SDK or a specific framework. Point the standard OpenTelemetry exporter variables, such as OTEL_EXPORTER_OTLP_ENDPOINT, at an endpoint, and any framework, language or model provider that emits spans works. Our OpenTelemetry guide covers the details of that setup.\"},\"name\":\"Do I need an SDK or a specific framework to build one?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You can trust an LLM judge only partly, because Microsoft's agent framework docs warn that an evaluator can be probabilistic. Use deterministic scorers wherever a rule can decide the outcome, and check the judge against human feedback on a sample of its scores.\"},\"name\":\"Can I trust an LLM judge to score my agent?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Every loop needs a cap, and the right number depends on the task. The framework docs say to always bound autonomous loops. Read step counts from real runs before you pick a number, then set the cap for that task.\"},\"name\":\"How many iterations should an agent be allowed?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Improving an agent usually does not mean retraining the model, because most fixes change the prompt, a tool, retrieval or routing. Whichever of those you change, you compare the candidate with the baseline on a dataset before release. That comparison shows whether the change helped.\"},\"name\":\"Does improving an agent mean retraining the model?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q2283\",\"@type\":\"Organization\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q146768\",\"@type\":\"Thing\",\"description\":\"computer software that provides services to software applications\",\"name\":\"Middleware\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Middleware\",\"https://www.wikidata.org/wiki/Q146768\",\"https://www.google.com/search?kgmid=/m/0j2b7zq\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q364\",\"@type\":\"Organization\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q7574597\",\"@type\":\"Thing\",\"description\":\"ACM Special Interest Group\",\"name\":\"Special Interest Group on Information Retrieval\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Special_Interest_Group_on_Information_Retrieval\",\"https://www.wikidata.org/wiki/Q7574597\",\"https://www.google.com/search?kgmid=/m/03ctz3h\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q11660\",\"@type\":\"Thing\",\"description\":\"field that develops and studies software enabling machines to exhibit intelligent behavior\",\"name\":\"Artificial intelligence\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Artificial_intelligence\",\"https://www.wikidata.org/wiki/Q11660\",\"https://www.google.com/search?kgmid=/m/0mkz\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\",\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]}],\"name\":\"Agent feedback loop: From bad run to safe release\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/btgtBOxXWX4PE9e2cAXa-TUFzDRyTVV0.webp\"},\"url\":\"https://mcpd.sh/blog/agent-feedback-loop\"},{\"@id\":\"https://mcpd.sh/blog/agent-feedback-loop#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q183635\"},{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"Scores and feedback · mcpd\",\"url\":\"https://mcpd.sh/docs/evaluation\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd README\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"Cost · mcpd\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"The AI Agent Feedback Loop: From Evaluation to Continuous Improvement\",\"url\":\"https://dev.to/imshashank/the-ai-agent-feedback-loop-from-evaluation-to-continuous-improvement-5hm4\"},{\"@type\":\"CreativeWork\",\"name\":\"Agent Looping | Microsoft Agent Framework\",\"url\":\"https://learn.microsoft.com/en-us/agent-framework/agents/looping\"},{\"@type\":\"CreativeWork\",\"name\":\"Agentic Feedback Loop Modeling Improves Recommendation and User Simulation\",\"url\":\"https://arxiv.org/html/2410.20027v2\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory.\",\"url\":\"https://mcpd.sh\"},{\"@type\":\"CreativeWork\",\"name\":\"Privacy · mcpd\",\"url\":\"https://mcpd.sh/docs/privacy\"},{\"@type\":\"CreativeWork\",\"name\":\"Alerting · mcpd\",\"url\":\"https://mcpd.sh/docs/alerting\"}],\"dateModified\":\"2026-11-09\",\"datePublished\":\"2026-11-09\",\"description\":\"What is an agent feedback loop? Two loops: a runtime loop inside each run and an improvement loop across runs. Learn to set both up with OpenTelemetry.\",\"headline\":\"Agent feedback loop: From bad run to safe release\",\"image\":\"https://api.birchseek.com/m/btgtBOxXWX4PE9e2cAXa-TUFzDRyTVV0.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/agent-feedback-loop#webpage\"},\"keywords\":[\"agent feedback loop\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/agent-feedback-loop#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q2283\"},{\"@id\":\"https://www.wikidata.org/wiki/Q146768\"},{\"@id\":\"https://www.wikidata.org/wiki/Q364\"},{\"@id\":\"https://www.wikidata.org/wiki/Q7574597\"},{\"@id\":\"https://www.wikidata.org/wiki/Q11660\"},{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2830},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]},{\"@type\":\"Thing\",\"description\":\"Chinese artificial intelligence company\",\"name\":\"DeepSeek\",\"sameAs\":[\"https://en.wikipedia.org/wiki/DeepSeek\",\"https://www.wikidata.org/wiki/Q131577453\",\"https://www.google.com/search?kgmid=/g/11wvrb0s91\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"defined sequence of activities of an organizational entity, enabled by the systematic organization of resources into processes that transform materials, provide services, or process information\",\"name\":\"Workflow\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Workflow\",\"https://www.wikidata.org/wiki/Q627335\",\"https://www.google.com/search?kgmid=/m/0k5rb\"]},{\"@type\":\"Thing\",\"description\":\"maximum number of tokens that a large language model can process at one time\",\"name\":\"Context window\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Context_window\",\"https://www.wikidata.org/wiki/Q125619872\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@type\":\"Thing\",\"description\":\"chatbot developed by SpaceXAI\",\"name\":\"Grok\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Grok_(chatbot)\",\"https://www.wikidata.org/wiki/Q123361035\",\"https://www.google.com/search?kgmid=/g/11l6lp1dv3\"]},{\"@type\":\"Thing\",\"description\":\"collection of data, publication type\",\"name\":\"Data set\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Data_set\",\"https://www.wikidata.org/wiki/Q1172284\",\"https://www.google.com/search?kgmid=/m/02cck\"]},{\"@type\":\"Thing\",\"description\":\"artificial intelligence tool for code autocompletion\",\"name\":\"GitHub Copilot\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub_Copilot\",\"https://www.wikidata.org/wiki/Q107435063\",\"https://www.google.com/search?kgmid=/g/11q83qbj3d\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - llm agents
  - opentelemetry
  - agent evaluation
  - observability
  - regression testing
  - tool loops
title: "Agent feedback loop: From bad run to safe release"
---

**An agent feedback loop is two loops: the inner runtime loop (act, observe, verify, repeat, within a bound) and the outer improvement loop (production runs, evidence, one change, comparison, release). A saved bad run, a dataset and a candidate-versus-baseline comparison close the outer one. Traces, scores and feedback sent over OpenTelemetry show what happened in both.**

A refund agent calls the same order-lookup tool on every iteration until the provider bill spikes. Every call succeeds, so no error log points to the run, and the only evidence is a customer complaint with no run attached.

If you ship an LLM agent to production, an agent feedback loop lets you close that gap. You can find the run behind a complaint, read the exact prompt and completion, keep the run as a test, and ship one change safely.

> **Key Takeaways**
> - [Scores (machine) and feedback (human)](https://mcpd.sh/docs/evaluation) stay separate signals, never averaged, and each score declares its scale.
> - An unknown model has no cost, not a cost of zero: each step is priced once from [a price book](https://mcpd.sh/docs/cost).
> - A failures-only dataset is biased, so [compare a candidate with a baseline](https://dev.to/imshashank/the-ai-agent-feedback-loop-from-evaluation-to-continuous-improvement-5hm4) on the same dataset, one change at a time.
> - [OpenTelemetry environment variables](https://mcpd.sh/docs/opentelemetry) set up the loop, and [OTLP forwarding](https://github.com/loruorg/mcpd) lets you leave.

## What is an agent feedback loop, and why are there two?

An agent feedback loop is the cycle in which an agent's results are observed and fed back into what happens next. It runs at two levels: an inner runtime loop that acts, observes, verifies and repeats within a bound, and an outer improvement loop that turns production evidence into one tested change.

![A split process illustration shows a chaotic, fiery loop refining debris on the left, which connects to a clean, optimized](/blog/agent-feedback-loop/image-1-4394a658468a.webp)

### The inner loop: inside one run

The inner loop of an agent feedback loop lives inside a single run. The agent calls the model, calls a tool, reads the result, checks it and goes round again.

[Microsoft's Agent Framework documentation](https://learn.microsoft.com/en-us/agent-framework/agents/looping) describes agent looping as re-invoking an agent "until a completion condition is satisfied".

You observe this half of the agent feedback loop as one trace. Repeated model and tool spans sit nested under a single root span, and the waterfall of that run shows each pass.

The [concepts guide](https://mcpd.sh/docs/concepts) defines traces and spans. A refund agent stuck on one order lookup shows up as the same tool span stacked down the page.

### The outer loop: across many runs

The outer agent feedback loop spans many runs. No single trace contains it, because it is made of scores, feedback, datasets and experiment comparisons gathered from production.

One published description of the process, [a dev.to write-up](https://dev.to/imshashank/the-ai-agent-feedback-loop-from-evaluation-to-continuous-improvement-5hm4), says to "propose one bounded change and compare the candidate with the baseline under recorded conditions."

The change is usually a new prompt, a revised tool definition, a different retrieval setup or a new routing rule. Retraining the model is one possible change, and the loop does not depend on it.

### A third meaning in recommender research

Researchers also use the term. The [SIGIR '25 paper](https://arxiv.org/html/2410.20027v2) "Agentic Feedback Loop Modeling Improves Recommendation and User Simulation" applies it to recommender systems and user simulation, a separate use from operating agents in production.

## How do you instrument an agent feedback loop with OpenTelemetry?

You instrument an agent feedback loop with OpenTelemetry by pointing an exporter at an endpoint through environment variables and emitting one span per agent run, model call, tool call and retrieval. No SDK or framework middleware is involved.

1. Set the exporter. Set the protocol to `http/json`:

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
export OTEL_EXPORTER_OTLP_HEADERS="authorization=Bearer $MCPD_KEY"
```

Create the project key at [mcpd.sh/app](https://mcpd.sh/app). A missing header is the usual reason nothing appears.

2. Emit the span tree. Your refund agent's run should look like this:

```text
invoke_agent refund_agent
├─ chat gpt-4o
├─ execute_tool lookup_order
├─ retrieval
└─ chat gpt-4o
```

3. Put the evidence on the model spans.

 Record the model, the token counts, the prompt and the completion on every call, because a loop that repeats a tool call shows up in the intermediate completions, not the final answer.

4. Open the run. The trace should list every step with its payload and tokens. Spans that arrive without a `gen_ai.request.model` can't be priced.

Pin the instrumentation version you deploy.

mcpd reads both the `gen_ai.*` and the OpenInference `llm.*` conventions, so existing instrumentation works unchanged. The [OpenTelemetry ingestion guide](https://mcpd.sh/docs/opentelemetry) lists what it accepts.

Cost isn't an attribute you send. [mcpd prices each step](https://mcpd.sh/docs/cost) once at ingest from a price book of per-million-token rates, which covers cached tokens, long-prompt tiers and service tiers. Rate structures are on each provider's pricing page.

To try this, [start free on the hosted version](https://mcpd.sh/app) or build from source at github.com/loruorg/mcpd.

## How do you get from a complaint to the one bad run?

In an agent feedback loop, you get from a complaint to the one bad run by searching for what the model said, narrowing by what happened, and reading the prompt and completion in the trace.

1. Search the payload for the model's words. When a customer reports that the agent refunded the wrong order, take a phrase from the reply they saw, such as "I've refunded order". [The mcpd home page](https://mcpd.sh) says "Search finds a run by what the model said", so this works even when no label or user ID was attached.

 Searching only by labels fails here, because a run with the wrong refund carries the same labels as a correct one.

2. Narrow the matches with aspect notes. An aspect note is one sentence about a finished run along one axis: goal, sentiment, behaviour or outcome. Filter for a behaviour note that mentions repeated lookups, or an outcome note that mentions a refund.

 Treat the note as a pointer: it summarises the run and proves nothing.

3. Open the trace and read the payload, since the prompt and the completion are the evidence. The [runs documentation](https://mcpd.sh/docs/runs) covers reading a run. In a trace of this complaint, the tool returns two orders and the model refunds the first without asking which one:

```text
step 2  tool_call  lookup_order {"customer": "c_1042"}
        result     {"orders": ["A-77", "A-81"]}
step 3  tool_call  lookup_order {"customer": "c_1042"}
        result     {"orders": ["A-77", "A-81"]}
step 4  completion "I've refunded order A-77."
```

The failure is now two steps you can point at: the repeated call in step 3 and the unchecked choice in step 4.

4. Save the run as a regression case. Add it to a dataset so the fix is tested against the exact input that failed; the [mcpd testing guide](https://mcpd.sh/docs/testing) covers building datasets from production runs. Save the case before you change the prompt, because afterwards you can no longer reproduce the failure as it happened.

5. Hand the same path to a support agent, if you want one. `mcpd mcp` exposes `find_runs`, `get_run` and `run_stats` over the Model Context Protocol, with the same key scopes, so a support agent can read traces without a person opening the interface.

## Scores vs feedback: why not average them into one number?

In an agent feedback loop, scores and feedback stay separate because averaging a passing score with a negative review produces a middling number that hides the disagreement between them.

| | Deterministic scorer | LLM judge | Review queue |
| --- | --- | --- | --- |
| Source | A rule in code | A model | A person |
| Calls a model | No | Yes | No |
| Cost to produce | Compute only | A model call per judgement | A person's time |
| Catches | One narrow condition | Open-ended quality | What mattered to the user |

[mcpd's evaluation docs](https://mcpd.sh/docs/evaluation) put the split plainly: "A score is what a machine thought. Feedback is what a person thought." mcpd stores them as two signals, and each score declares its scale, so a pass/fail check can't be mistaken for a rating.

In the refund case, `no_tool_loop` marks down the run that repeats the order lookup.

That scorer says nothing about whether the refund amount was right. You can define your own scorer for other narrow checks.

A model can also be the judge, which covers open-ended questions such as whether the answer addressed the complaint. A judge is a model call that can be wrong, so compare its verdicts with human feedback on a sample of runs before you rely on it at volume.

A review queue collects feedback from a person, the human half of the agent feedback loop. It's slow, and it's the ground truth about what mattered. A reviewer can mark a run that every scorer passed as unhelpful, and that disagreement is the finding.

In an agent feedback loop, each method has its job:

- Deterministic scorers check loops, budgets and output formats on every run.
- An LLM judge rates open-ended quality at volume, once its verdicts match human feedback.
- The review queue settles the cases where a person's view decides what counts as a failure.

## How do you stop an agent loop from running away?

You stop an agent loop from running away by stacking four layers: an iteration cap, loop detection on repeated tool calls, a cost ceiling and a monitor on step counts. Each one catches a failure the others miss.

1. Set an iteration cap in the runtime. Microsoft's [Agent looping page](https://learn.microsoft.com/en-us/agent-framework/agents/looping) says "Always bound autonomous loops" because "a completion condition can fail, a model can stall, and an evaluator can be probabilistic." The page also labels agent looping experimental.

 Read the default for your version and set an explicit number. Without a cap, a stalled model never stops.

2. Detect repeated tool calls. Compare each new call's tool name and arguments with the last few, and stop the run or hand it to a person after the same pair repeats. Every call in a refund loop can succeed, so error rate stays flat and only the repetition shows.

 On finished runs, mcpd's [`no_tool_loop` scorer](https://mcpd.sh/docs/evaluation) flags the same pattern.

3. Put a ceiling on cost, and check that cost exists. In [mcpd's price book](https://mcpd.sh/docs/cost), an unknown model has no cost, not a cost of zero, so a missing price shows up as a gap. Without that rule, a ceiling would compare against 0 and never trip.

 If your model is missing, supply your own book before you rely on the ceiling.

4. Add a monitor on step-count divergence. mcpd [evaluates monitors every 60 s](https://mcpd.sh/docs/alerting), and one of them watches whether runs diverge from their normal number of steps. Set it to call your webhook.

A monitor has to latch: it fires once, stays open, and resolves only on a real reading. If it re-fired every minute, you'd stop reading the pages.

If it cleared when traffic went quiet, an outage would look like recovery. The alerting docs put it as "Silence is not recovery."

When all four layers work, a looping run ends at the cap and its trace shows the repeated call. The cost ceiling trips before the bill does, and one webhook arrives instead of a stream.

## How do you close the agent feedback loop with a safe release?

You close the agent feedback loop with a safe release by running a candidate and a baseline on the same dataset, changing one thing between them, and reading the deltas next to their sample sizes.

1. Pick one change for this turn of the agent feedback loop: a prompt version, a tool description, a retrieval setting or a routing rule. If you edit the prompt and the retrieval top-k together, a delta can't be traced to either.

2. Tie every step to a version. Keep the prompt as a labelled, versioned object fetched at runtime, so each step links to the version that wrote it. The [prompts documentation](https://mcpd.sh/docs/prompts) covers versions and labels.

 Editing a prompt in place breaks the comparison, because baseline and candidate then share a name.

3. Run both on the same dataset, with the same cases, the same scorers and the same scale. Record the model, the temperature and the dataset version next to each result.

4. Count missing results on their own line. A case that didn't run, or wasn't scored, is neither a pass nor a failure, so report three counts: passed, failed and missing.

 A candidate that fails to run on 10 cases hasn't improved, and folding those cases into the pass rate hides the regression. A [dev.to write-up on agent feedback loops](https://dev.to/imshashank/the-ai-agent-feedback-loop-from-evaluation-to-continuous-improvement-5hm4) says to "review regressions and missing results before release."

5. Read each delta with its sample size: "+0.08 on 12 cases" and "+0.08 on 400 cases" are different findings. Check the cases that moved in the wrong direction first, since those are the ones that block a release.

6. State what the dataset can't tell you. A dataset built from failures measures recovery on known bad cases, not behaviour on typical traffic.

 The retained traces are also skewed: a tail sampling rule that keeps every errored trace and a fraction of the rest fills the dataset with errors by design. The same write-up puts it plainly: "Production sampling and instrumentation determine which failures are available to investigate."

 Before you release, also run the candidate on a set of ordinary runs drawn by a ratio or deterministic sampling mode.

The release record that closes the agent feedback loop is a short table: one named change, two version labels, passed, failed and missing counts for each, deltas with their case counts, and a separate check on typical traffic.

## How do you keep feedback data private and avoid lock-in?

You keep feedback data private by deciding, before the loop starts, what gets stored, how long it stays and where it can go next. Feedback holds the most sensitive material in the agent feedback loop: prompts, completions, judge inputs and a person's comments.

Make three decisions up front for the agent feedback loop:

- Redaction: which fields are dropped, masked, partially shown or replaced with a keyed hash before anything is written.
- Retention: how many days each kind of data is kept.
- Erasure: how you find and delete everything about one person when they ask.

In an agent feedback loop, redaction has to come first. [The mcpd privacy docs](https://mcpd.sh/docs/privacy) say, "Redaction runs before the write, because a secret deleted afterwards was still written down." Apply the same rules to judge inputs and feedback text as you apply to spans.

A judge endpoint receives your payload, so limit what it sees and trust it on purpose. The mechanics are in mcpd's [security documentation](https://mcpd.sh/docs/security).

The exit stays open. [The mcpd README](https://github.com/loruorg/mcpd) says, "What it stores it also forwards, as OTLP, to any endpoint you name", so the improvement loop is one hop in a pipeline you already run.

## Frequently asked questions

### What's the difference between an agent loop and an agent feedback loop?

The agent loop is the inner cycle inside one run: call the model, run a tool, observe the result, repeat until done or capped. The agent feedback loop in the improvement sense spans many runs: collect evidence, change one thing, compare against a baseline, release.

### Do I need an SDK or a specific framework to build an agent feedback loop?

You don't. Point the standard OpenTelemetry exporter variables, such as `OTEL_EXPORTER_OTLP_ENDPOINT`, at an endpoint, and any framework, language or model provider that emits spans works. Our [OpenTelemetry guide](https://mcpd.sh/docs/opentelemetry) covers the details.

### Can I trust an LLM judge to score my agent?

Only partly. [Microsoft's agent framework docs](https://learn.microsoft.com/en-us/agent-framework/agents/looping) warn that an evaluator can be probabilistic. Use deterministic scorers wherever a rule can decide, and check the judge against human feedback on a sample.

### How many iterations should an agent be allowed?

Set a cap on every loop. The right number depends on the task, so read step counts from real runs before you pick one.

### Does improving an agent mean retraining the model?

Usually not. Most fixes change the prompt, a tool, retrieval or routing, and you compare the candidate with the baseline on a dataset before release.

## Where to start with your agent feedback loop

Start your agent feedback loop small, with one run you can read.

**Today:** set `OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh` and `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`, with a key from your project. Run the agent once and open that run to see a prompt and its completion.

**This week:** save the bad run as a regression case. Turn on `no_tool_loop` and `cost_budget`, and set a monitor so a diverging run reaches you before the bill does.

**Next release:** change one thing, then compare candidate and baseline on the same dataset. Read the deltas next to the sample sizes, and count missing results separately from failures.

[Start free at mcpd.sh/app](https://mcpd.sh/app). Setup is two environment variables, so you can remove them as easily as you added them. To run it yourself, build from source at github.com/loruorg/mcpd.

## Sources

[Scores and feedback · mcpd](https://mcpd.sh/docs/evaluation) · [Cost · mcpd](https://mcpd.sh/docs/cost) · [The AI Agent Feedback Loop: From Evaluation to Continuous…](https://dev.to/imshashank/the-ai-agent-feedback-loop-from-evaluation-to-continuous-improvement-5hm4) · [OpenTelemetry · mcpd](https://mcpd.sh/docs/opentelemetry) · [mcpd README](https://github.com/loruorg/mcpd) · [Agent Looping | Microsoft Agent Framework](https://learn.microsoft.com/en-us/agent-framework/agents/looping) · [Agentic Feedback Loop Modeling Improves Recommendation and…](https://arxiv.org/html/2410.20027v2) · [Alerting · mcpd](https://mcpd.sh/docs/alerting). Every other source is linked where the article cites it.
