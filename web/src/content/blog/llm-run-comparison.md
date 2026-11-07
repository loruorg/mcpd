---
description: "Run an LLM run comparison on your own traffic: send one task through two versions and read quality, cost, latency and steps together. Learn how to set it up."
faq:
  - answer: To compare two LLM runs, send both through OpenTelemetry to mcpd, open them side by side on one clock, and read quality scores, cost per step, latency and step count together. Use your own traffic, and keep scores and human feedback as separate signals.
    question: How do I compare two LLM runs?
  - answer: "Benchmark scores are not enough on their own to choose a model, because they don't cover your prompts, your tools or your retrieval. If you cite one, name the edition you read, then confirm the pick on runs of your own workload."
    question: Are benchmark scores enough to choose a model?
  - answer: A blank cost means the cost is unknown, so it does not mean the run was free. An unknown model has no cost in mcpd, which is different from a cost of zero. A missing price therefore never reads as a free step when you compare cost per step across two runs.
    question: Does a blank cost mean the run was free?
  - answer: "You do not need to change your framework or add a gateway. Set OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh and send OpenTelemetry spans with gen_ai.* attributes from the code you already run. mcpd is a daemon, not a proxy, so nothing in your request path changes."
    question: Do I need to change my framework or add a gateway?
  - answer: "Scores and feedback stay separate because they measure different things: a score is machine evaluation, and feedback is human evaluation. mcpd never averages one into the other, so when the two disagree, that disagreement stays visible when you read two runs side by side."
    question: Why do scores and feedback stay separate?
image: /blog/llm-run-comparison/featured-93ff10e44cda.webp
image_alt: Four stylized obelisks with different colored accents stand in a large, modern data center.
publishDate: "2026-11-07"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/llm-run-comparison#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\",\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q21708200\",\"@type\":\"Organization\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]}],\"datePublished\":\"2026-11-07\",\"description\":\"Run an LLM run comparison on your own traffic: send one task through two versions and read quality, cost, latency and steps together. Learn how to set it up.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"To compare two LLM runs, send both through OpenTelemetry to mcpd, open them side by side on one clock, and read quality scores, cost per step, latency and step count together. Use your own traffic, and keep scores and human feedback as separate signals.\"},\"name\":\"How do I compare two LLM runs?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Benchmark scores are not enough on their own to choose a model, because they don't cover your prompts, your tools or your retrieval. If you cite one, name the edition you read, then confirm the pick on runs of your own workload.\"},\"name\":\"Are benchmark scores enough to choose a model?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"A blank cost means the cost is unknown, so it does not mean the run was free. An unknown model has no cost in mcpd, which is different from a cost of zero. A missing price therefore never reads as a free step when you compare cost per step across two runs.\"},\"name\":\"Does a blank cost mean the run was free?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You do not need to change your framework or add a gateway. Set OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh and send OpenTelemetry spans with gen_ai.* attributes from the code you already run. mcpd is a daemon, not a proxy, so nothing in your request path changes.\"},\"name\":\"Do I need to change my framework or add a gateway?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Scores and feedback stay separate because they measure different things: a score is machine evaluation, and feedback is human evaluation. mcpd never averages one into the other, so when the two disagree, that disagreement stays visible when you read two runs side by side.\"},\"name\":\"Why do scores and feedback stay separate?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q125619872\",\"@type\":\"Thing\",\"description\":\"maximum number of tokens that a large language model can process at one time\",\"name\":\"Context window\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Context_window\",\"https://www.wikidata.org/wiki/Q125619872\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q6654550\",\"@type\":\"Thing\",\"name\":\"LiveBench\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LiveBench\",\"https://www.wikidata.org/wiki/Q6654550\",\"https://www.google.com/search?kgmid=/m/027q44x\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q364\",\"@type\":\"Organization\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]}],\"name\":\"LLM run comparison: Cost, latency and quality\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/Sg3koNPdRHwH9p67ZQsIbDNeD1XXyYUW.webp\"},\"url\":\"https://mcpd.sh/blog/llm-run-comparison\"},{\"@id\":\"https://mcpd.sh/blog/llm-run-comparison#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\"},{\"@id\":\"https://www.wikidata.org/wiki/Q21708200\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"loruorg/mcpd on GitHub\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"AI Model Comparison - llm-stats.com\",\"url\":\"https://llm-stats.com/models/compare\"},{\"@type\":\"CreativeWork\",\"name\":\"Evaluation · mcpd\",\"url\":\"https://mcpd.sh/docs/evaluation\"},{\"@type\":\"CreativeWork\",\"name\":\"LLM Benchmarks — Open-Model Leaderboards \\u0026 Scores | llmrun\",\"url\":\"https://llmrun.dev/benchmark\"},{\"@type\":\"CreativeWork\",\"name\":\"Reasoning models guide, OpenAI\",\"url\":\"https://platform.openai.com/docs/guides/reasoning\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd cost documentation\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"Artificial Analysis Models Leaderboard\",\"url\":\"https://artificialanalysis.ai/leaderboards/models\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"Alerting · mcpd\",\"url\":\"https://mcpd.sh/docs/alerting\"},{\"@type\":\"CreativeWork\",\"name\":\"Finding a run · mcpd\",\"url\":\"https://mcpd.sh/docs/searching\"}],\"dateModified\":\"2026-11-07\",\"datePublished\":\"2026-11-07\",\"description\":\"Run an LLM run comparison on your own traffic: send one task through two versions and read quality, cost, latency and steps together. Learn how to set it up.\",\"headline\":\"LLM run comparison: Cost, latency and quality\",\"image\":\"https://api.birchseek.com/m/Sg3koNPdRHwH9p67ZQsIbDNeD1XXyYUW.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/llm-run-comparison#webpage\"},\"keywords\":[\"llm run comparison\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/llm-run-comparison#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q125619872\"},{\"@id\":\"https://www.wikidata.org/wiki/Q6654550\"},{\"@id\":\"https://www.wikidata.org/wiki/Q364\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2686},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]},{\"@type\":\"Thing\",\"description\":\"defined sequence of activities of an organizational entity, enabled by the systematic organization of resources into processes that transform materials, provide services, or process information\",\"name\":\"Workflow\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Workflow\",\"https://www.wikidata.org/wiki/Q627335\",\"https://www.google.com/search?kgmid=/m/0k5rb\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"Chinese artificial intelligence company\",\"name\":\"DeepSeek\",\"sameAs\":[\"https://en.wikipedia.org/wiki/DeepSeek\",\"https://www.wikidata.org/wiki/Q131577453\",\"https://www.google.com/search?kgmid=/g/11wvrb0s91\"]},{\"@type\":\"Thing\",\"description\":\"collection of data, publication type\",\"name\":\"Data set\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Data_set\",\"https://www.wikidata.org/wiki/Q1172284\",\"https://www.google.com/search?kgmid=/m/02cck\"]},{\"@type\":\"Thing\",\"description\":\"software to manage containers on a server-cluster\",\"name\":\"Kubernetes\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Kubernetes\",\"https://www.wikidata.org/wiki/Q22661306\",\"https://www.google.com/search?kgmid=/g/11b7lxp79d\"]},{\"@type\":\"Thing\",\"description\":\"maximum number of tokens that a large language model can process at one time\",\"name\":\"Context window\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Context_window\",\"https://www.wikidata.org/wiki/Q125619872\"]},{\"@type\":\"Thing\",\"description\":\"benchmark in natural language understanding\",\"name\":\"General Language Understanding Evaluation benchmark\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q64711428\"]},{\"@type\":\"Thing\",\"description\":\"chatbot developed by SpaceXAI\",\"name\":\"Grok\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Grok_(chatbot)\",\"https://www.wikidata.org/wiki/Q123361035\",\"https://www.google.com/search?kgmid=/g/11l6lp1dv3\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - llm run comparison
  - llm observability
  - opentelemetry
  - cost per run
  - agent evaluation
  - benchmarks
title: "LLM run comparison: Cost, latency and quality"
---

**An llm run comparison sends the same task through two models, prompts or agent versions and reads quality, cost, latency and step count together. Run it on your own traffic, because a public leaderboard doesn't use your prompts, tools or retrieval. The [mcpd daemon on GitHub](https://github.com/loruorg/mcpd) builds the comparison from your OpenTelemetry data and never sits in the request path, so your agent's calls don't pass through it.**

You swapped in a cheaper or newer model because it looked strong on a public leaderboard. Now the agent is slower, the bill went up, or the answers are wrong, and nothing in your logs says why.

An LLM run comparison on your own traffic answers that. You will be able to set up two versions on real traffic, read the cost of every step, and tell an unknown value from a zero.

> **Key Takeaways**
> - Quote a benchmark only with its edition and date, and confirm the choice on runs of your own workload.
> - A model that is cheaper per token can cost more per run when it loops on a tool.
> - Treat a missing price or latency as unknown, and keep it out of means and totals while saying how many runs you excluded.
> - Keep machine scores and human feedback as separate signals, so you can see which one moved.

## What does an LLM run comparison actually compare?

An LLM run comparison compares whole runs of the same task, side by side, on quality, cost, latency and step count. Because the runs come from your own traffic, the result describes your tools and retrieval.

![Illustration for What does an LLM run comparison actually compare?](/blog/llm-run-comparison/image-1-a2924e2fae30.webp)

The unit of an LLM run comparison is the run.

A run is a trace, each unit of work in it is a span, and each model call, tool call or retrieval is a step. You can open any step to read its prompt, completion, tokens and cost.

You can compare three kinds of variant:

- Two models behind the same prompt and tools.
- Two prompt versions. mcpd [links each step to the prompt version](https://mcpd.sh/docs/prompts) that wrote it, so a change shows up on the steps it touched.
- Two agent versions, where the code, tools or retrieval changed.

Read the four axes of an LLM run comparison together, because no single one settles the choice. A model that answers better but takes more steps or tokens buys its quality with cost and time.

Public comparison tables split these into columns. [llm-stats](https://llm-stats.com/models/compare), for example, lets you "compare leading AI models side by side across benchmarks, price, context window, speed, latency, modality, and license." Each column comes from the table's own tasks, so none of them describes what one of your runs does.

A dataset of 40 production runs, replayed through your current agent version and a candidate, gives you 40 pairs of runs. Each pair starts from the same input and differs only in the version under test.

You then read both runs of a pair on one clock: quality scores, cost per run, wall-clock time and the number of steps.

## Why can't a public benchmark replace an LLM run comparison on your workload?

A public benchmark score doesn't predict how a model runs on your workload because it measures the model on someone else's questions, without your prompts, tools or retrieval. GPQA, LiveBench and ARC-AGI each fix a set of questions or tasks and score every model against that set.

Your agent sends its own system prompt, calls tools with your schemas and reads documents from your index. The benchmark contains none of that.

A score also says nothing about how a run unfolds. A model that answers a benchmark question in one call may take six steps on your task because it retries a tool or misreads a result. Cost and step count come from the run, so only a run can show them.

Composite indices move the score one step further from its questions. [llmrun's benchmark page](https://llmrun.dev/benchmark) describes its composite as "One open-ended quality index for open and proprietary LLMs, fitted across every public leaderboard we track." Many boards become one figure, and the figure no longer tells you which question set produced it.

[mcpd's evaluation docs](https://mcpd.sh/docs/evaluation) take the opposite route: "A score is what a machine thought. Feedback is what a person thought. mcpd keeps them as two signals and never averages one into the other."

When you quote a benchmark, name the benchmark and its edition. A score only means something against the question set that produced it.

Use an LLM run comparison on your own workload instead:

- Send the same task to both candidates with the same prompt, the same tools and the same retrieval.
- Use your production dataset as the test set for the LLM run comparison.
- Read the two runs on quality, cost, latency and step count together.

If both candidates export their runs over [OpenTelemetry](https://mcpd.sh/docs/opentelemetry), you can read them side by side without changing your framework.

## How does an LLM run comparison work out cost per run instead of price per token?

You work out cost per run by pricing each step from its own token counts and adding the steps up. The price per million tokens is one input to that sum.

If a model charges $1 per million input tokens and $4 per million output tokens, a step that sends 20,000 tokens and gets 500 back costs $0.022. An agent that takes five such steps costs $0.11, and more once each step re-sends a longer conversation.

Two things change the sum in an LLM run comparison.

Output tokens have their own rate. At $4 per million against $1 for input, a model that writes long answers costs more per step than its input rate suggests.

 

Reasoning effort reaches the bill through thinking tokens. [OpenAI's reasoning guide](https://platform.openai.com/docs/guides/reasoning) says reasoning tokens are "billed as output tokens".

The effort setting changes your bill through how many of those tokens the model produces. Current rates are listed on each provider's own pricing page.

mcpd prices each step once, at ingest, from a [price book](https://mcpd.sh/docs/cost) of per-million-token rates. The book handles cached tokens, long-prompt tiers and service tiers, and you can replace it with your own.

Pricing at ingest keeps every LLM run comparison stable over time. If you edit the book later, runs already recorded keep the cost they were priced at, so a comparison you ran last week still shows last week's numbers.

## How are time to first token, latency, tokens and cost per run defined?

Time to first token and latency come from span timestamps, token counts come from span attributes, and cost per run is those counts multiplied by price book rates and summed across the steps. Each measure needs one definition and one named input, so two runs can be compared without arguing about what a number means.

[Artificial Analysis's leaderboard page](https://artificialanalysis.ai/leaderboards/models) describes speed as "time per task in minutes & latency as time to first answer token". The same word, latency, can therefore mean a first-token time on one page and a total time on another.

mcpd defines seven measures for a run:

| Measure | Definition | Input |
|---|---|---|
| Time to first token | From the start of a model-call span to the first output token. A step with no first-token timestamp, such as a non-streamed call, has no first-token time. | First-token timestamp on the same clock as the span |
| Latency per step | The span's end time minus its start time | Both timestamps read from one clock |
| Latency per run | The root span's end time minus its start time, including waits between steps and tool calls as well as model time | Root span timestamps |
| Input tokens | The count the provider reports for the prompt | `gen_ai.usage.input_tokens` span attribute |
| Output tokens | The count the provider reports for the completion | `gen_ai.usage.output_tokens` span attribute |
| Cost per step | Input and output tokens multiplied by the model's rates in the price book | The two token attributes and the model name on the span |
| Cost per run | The sum of cost per step across the run | Cost per step |

A first-token time says how soon a reader sees output, and a run's latency says how long the whole task took. A model with a fast first token can still lose on run latency if it takes more steps.

For token counts and model names, point your exporter at the [OpenTelemetry endpoint](https://mcpd.sh/docs/opentelemetry) and let your existing instrumentation write the `gen_ai.*` attributes.

## What do you do when a price or latency is missing from an LLM run comparison?

When a price or latency is missing, record it as unknown, keep it out of means and totals, and compare only the runs where both versions have the value. A dash in a comparison table means nobody measured it, and reading it as zero changes the result.

[llm-stats.com's model comparison](https://llm-stats.com/models/compare) follows the same practice: "Missing live measurements are left blank rather than estimated."

If your price book has a rate for model A but has never seen model B, and B's missing price is read as 0, B costs nothing per run. It then wins on cost every time, on a saving nobody measured.

mcpd prevents this when a run is ingested. In [mcpd's cost model](https://mcpd.sh/docs/cost), an unknown model has no cost, not a cost of zero. The interface shows a blank where the cost would be, and a blank can't make a total look smaller.

To give the model a real rate, supply your own price book with the per-million-token rates you pay.

Latency follows the same rule in an LLM run comparison. A step with no recorded end time has no duration, so it can't pass as the fastest step in the run.

Report how many runs you excluded next to the result, so a reader knows how much of the dataset the comparison covers.

Then treat each blank as a task. Check whether the model is missing from your price book, or whether the span never closed because the exporter dropped it or a request is still in flight. Fill the gap, then read the comparison again.

## How do you set up an LLM run comparison in mcpd?

You compare two LLM runs in mcpd by sending traces from both versions over OpenTelemetry and opening the runs side by side in the interface. Your agent code stays as it is.

1. Get a project key. Sign up at the hosted version, [mcpd.sh/app](https://mcpd.sh/app), or build the daemon from source at github.com/loruorg/mcpd. Create a project and copy its key.

2. Point your OpenTelemetry exporter at mcpd. Set two environment variables, then add the project key using the header format on the [OpenTelemetry docs page](https://mcpd.sh/docs/opentelemetry):

   ```shell
   export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
   export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
   ```

 mcpd reads `gen_ai.*` and OpenInference `llm.*` attributes, so existing instrumentation works unchanged.

3. Run the same task through both versions. Send one input to version A, then the same input to version B. With different inputs, you can't attribute a difference to the version.

4. Open both runs. Find them in the interface (the [runs guide](https://mcpd.sh/docs/runs) covers durable runs) and read four things side by side:
 - the waterfall of both runs on one clock
 - the full conversation, tool calls included
 - the exact prompt and completion at each step
 - the cost of each step

5. Find the step where the runs diverge. If the model that is cheaper per token calls the same tool 9 times where the other calls it twice, the waterfall shows the repeated spans, and the per-step costs add up to more than the dearer model's run.

6. Repeat the LLM run comparison over real traffic. The [evaluation](https://mcpd.sh/docs/evaluation) and [testing](https://mcpd.sh/docs/testing) docs describe both.

If both runs show up with no tokens or cost, check that your instrumentation emits `gen_ai.*` or `llm.*` attributes on the model-call spans.

Start with one pair of runs on the hosted version: [start free](https://mcpd.sh/app).

## Why does a cheaper model cost more per LLM run?

A cheaper model costs more per LLM run when it takes more steps, because a run's cost is the sum of its steps and each extra step is billed on its own.

Each tool-call round adds a step with its own prompt and completion tokens. The whole conversation so far is sent again as input, so every step carries more tokens than the one before. Cost per run rises with step count even when the rate per token is lower.

If model A answers a refund question in 3 steps, and model B, at half A's rate per token, calls the same lookup tool with the same arguments 9 times before answering, B re-sends a longer conversation each round. B's input tokens grow to more than twice A's, so its run costs more despite the lower price.

The built-in `no_tool_loop` scorer, described in [mcpd's evaluation docs](https://mcpd.sh/docs/evaluation), flags that repeat.

A monitor watches the whole fleet. The [step-count divergence monitor](https://mcpd.sh/docs/alerting) is evaluated every 60 s, and it latches: it fires once and resolves only on a real reading, so silence doesn't count as recovery.

To find the affected runs, use [full-text search over the payload](https://mcpd.sh/docs/searching) and query for what the model said, such as the repeated phrase before each retry.

A looping run can still end with a correct answer that a reviewer approves. The score shows the loop and the feedback shows the answer was acceptable, and an average of the two would hide both.

## Frequently asked questions

### How do I set up an LLM run comparison?

Send both runs to mcpd over OpenTelemetry, open them side by side on one clock, and read quality scores, cost per step, latency and step count together. Use inputs from your own traffic for both versions.

### Are benchmark scores enough to choose a model?

Benchmark scores don't cover your prompts, your tools or your retrieval, so they can't settle the choice alone. If you cite one, name the edition you read, then confirm the pick on runs of your own workload.

### Does a blank cost mean the run was free?

A blank cost means unknown. An unknown model has no cost in mcpd, not a cost of zero, so a missing price never reads as a free step, as [mcpd's cost documentation](https://mcpd.sh/docs/cost) explains. Leave that run out of cost totals until the model has a rate in your price book.

### Do I need to change my framework or add a gateway?

You need neither. mcpd is a daemon, not a proxy, so nothing in your request path changes, as the [mcpd README](https://github.com/loruorg/mcpd) explains.

### Why do scores and feedback stay separate?

A score is machine evaluation and feedback is human evaluation, so they measure different things. mcpd never averages one into the other, so a disagreement between them stays visible; the [evaluation docs](https://mcpd.sh/docs/evaluation) cover both signals.

## Next steps: run your first LLM run comparison

Work toward real traffic in three stages: one trace, then two versions of your agent, then a dataset.

**Today:** point `OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh` at a project and send your first trace. The [quickstart](https://mcpd.sh/docs/quickstart) covers the steps, and your agent code stays as it is.

**This week:** record runs of your current model and the candidate, or of two prompt or agent versions, on the same tasks. Open two of them side by side and read the step count, the cost per step and the exact prompt at each step.

**This month:**

- Build a dataset from production runs.
- Add the `no_tool_loop` and `cost_budget` scorers, so a cheaper model that loops on a tool fails a check before it shows up on an invoice.
- Set the step-count divergence monitor.
- Replace the price book with your own rates.

[Start free at the hosted version](https://mcpd.sh/app) and send your first trace. If you stop, you remove one environment variable and your agent runs as before.

## Sources

[loruorg/mcpd on GitHub](https://github.com/loruorg/mcpd) · [AI Model Comparison - llm-stats.com](https://llm-stats.com/models/compare) · [LLM Benchmarks — Open-Model Leaderboards & Scores | llmrun](https://llmrun.dev/benchmark) · [Evaluation · mcpd](https://mcpd.sh/docs/evaluation) · [Reasoning models guide, OpenAI](https://platform.openai.com/docs/guides/reasoning) · [mcpd cost documentation](https://mcpd.sh/docs/cost) · [Artificial Analysis Models Leaderboard](https://artificialanalysis.ai/leaderboards/models) · [OpenTelemetry · mcpd](https://mcpd.sh/docs/opentelemetry). Every other source is linked where the article cites it.
