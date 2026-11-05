---
description: How do you calculate LLM cost per project? Price each call from its usage fields, sum the runs, and compare with traces. Learn to cap spend with a monitor.
faq:
  - answer: "To calculate LLM cost per project, read input, cached and output token counts from each call's usage fields, multiply each by its per-million-token rate from the provider's pricing page, and add the results across every call in every run. Then compare the total with spend read from traces."
    question: How do I calculate LLM cost per project?
  - answer: "Reasoning tokens cost the output rate. OpenAI's reasoning guide says reasoning tokens are billed as output tokens even though the API doesn't show them, so a call's output count can exceed the length of the visible answer. Price the output count from the usage fields, not from the text you can read."
    question: Do reasoning tokens cost extra?
  - answer: A calculator includes retries only if you add them to the call count by hand. It multiplies the planned number of calls by average tokens, so loop iterations and context that grows each step are missing too. Summing priced steps from traces counts every call that ran.
    question: Does an LLM cost calculator include retries?
  - answer: If a model has no price in the price book, mcpd records no cost for that step instead of a cost of zero. A zero would quietly lower the project total, while a missing cost shows which model needs a rate. The cost accounting docs cover how pricing works at ingest.
    question: What happens if a model has no price in the price book?
image: /blog/llm-cost-per-project/featured-a2fa057f9894.webp
image_alt: "Illustration for LLM cost per project: Formula and run-level checks"
publishDate: "2026-11-05"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/llm-cost-per-project#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q165194\",\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\",\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]}],\"datePublished\":\"2026-11-05\",\"description\":\"How do you calculate LLM cost per project? Price each call from its usage fields, sum the runs, and compare with traces. Learn to cap spend with a monitor.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"To calculate LLM cost per project, read input, cached and output token counts from each call's usage fields, multiply each by its per-million-token rate from the provider's pricing page, and add the results across every call in every run. Then compare the total with spend read from traces.\"},\"name\":\"How do I calculate LLM cost per project?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Reasoning tokens cost the output rate. OpenAI's reasoning guide says reasoning tokens are billed as output tokens even though the API doesn't show them, so a call's output count can exceed the length of the visible answer. Price the output count from the usage fields, not from the text you can read.\"},\"name\":\"Do reasoning tokens cost extra?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"A calculator includes retries only if you add them to the call count by hand. It multiplies the planned number of calls by average tokens, so loop iterations and context that grows each step are missing too. Summing priced steps from traces counts every call that ran.\"},\"name\":\"Does an LLM cost calculator include retries?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"If a model has no price in the price book, mcpd records no cost for that step instead of a cost of zero. A zero would quietly lower the project total, while a missing cost shows which model needs a rate. The cost accounting docs cover how pricing works at ingest.\"},\"name\":\"What happens if a model has no price in the price book?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q118876059\",\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q21708200\",\"@type\":\"Organization\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q116758847\",\"@type\":\"Organization\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q95\",\"@type\":\"Organization\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]}],\"name\":\"LLM cost per project: Formula and run-level checks\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/fFaYfEXtAqjCtbjk8ahmkl-QEgKCgEzM.webp\"},\"url\":\"https://mcpd.sh/blog/llm-cost-per-project\"},{\"@id\":\"https://mcpd.sh/blog/llm-cost-per-project#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q165194\"},{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"Prompt caching | OpenAI API\",\"url\":\"https://platform.openai.com/docs/guides/prompt-caching\"},{\"@type\":\"CreativeWork\",\"name\":\"Alerting · mcpd\",\"url\":\"https://mcpd.sh/docs/alerting\"},{\"@type\":\"CreativeWork\",\"name\":\"Cost accounting · mcpd\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"Prompt caching | Anthropic\",\"url\":\"https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching\"},{\"@type\":\"CreativeWork\",\"name\":\"Reasoning models | OpenAI API\",\"url\":\"https://platform.openai.com/docs/guides/reasoning\"},{\"@type\":\"CreativeWork\",\"name\":\"Gemini Developer API pricing | Google AI for Developers\",\"url\":\"https://ai.google.dev/gemini-api/docs/pricing\"},{\"@type\":\"CreativeWork\",\"name\":\"Messages examples - Anthropic\",\"url\":\"https://docs.anthropic.com/en/api/messages-examples\"},{\"@type\":\"CreativeWork\",\"name\":\"CostPerPrompt — Live AI API Pricing \\u0026 LLM Cost Calculators\",\"url\":\"https://costperprompt.com/\"},{\"@type\":\"CreativeWork\",\"name\":\"Batch API | OpenAI API\",\"url\":\"https://platform.openai.com/docs/guides/batch\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/integrating\"},{\"@type\":\"CreativeWork\",\"name\":\"LLM Cost Calculator 2026: Compare 10+ Model Prices\",\"url\":\"https://www.ideaplan.io/tools/llm-cost-estimator\"}],\"dateModified\":\"2026-11-05\",\"datePublished\":\"2026-11-05\",\"description\":\"How do you calculate LLM cost per project? Price each call from its usage fields, sum the runs, and compare with traces. Learn to cap spend with a monitor.\",\"headline\":\"LLM cost per project: Formula and run-level checks\",\"image\":\"https://api.birchseek.com/m/fFaYfEXtAqjCtbjk8ahmkl-QEgKCgEzM.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/llm-cost-per-project#webpage\"},\"keywords\":[\"llm cost per project\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/llm-cost-per-project#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q118876059\"},{\"@id\":\"https://www.wikidata.org/wiki/Q21708200\"},{\"@id\":\"https://www.wikidata.org/wiki/Q116758847\"},{\"@id\":\"https://www.wikidata.org/wiki/Q95\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2777},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"defined sequence of activities of an organizational entity, enabled by the systematic organization of resources into processes that transform materials, provide services, or process information\",\"name\":\"Workflow\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Workflow\",\"https://www.wikidata.org/wiki/Q627335\",\"https://www.google.com/search?kgmid=/m/0k5rb\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"collection of data, publication type\",\"name\":\"Data set\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Data_set\",\"https://www.wikidata.org/wiki/Q1172284\",\"https://www.google.com/search?kgmid=/m/02cck\"]},{\"@type\":\"Thing\",\"description\":\"benchmark in natural language understanding\",\"name\":\"General Language Understanding Evaluation benchmark\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q64711428\"]},{\"@type\":\"Thing\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"large language model company\",\"name\":\"LlamaIndex\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LlamaIndex\",\"https://www.wikidata.org/wiki/Q125446413\"]},{\"@type\":\"Thing\",\"description\":\"language understanding across multiple domains\",\"name\":\"MMLU\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MMLU\",\"https://www.wikidata.org/wiki/Q123721276\"]},{\"@type\":\"Thing\",\"description\":\"a lightweight markup language with plain-text formatting syntax that is popularly used\",\"name\":\"Markdown\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Markdown\",\"https://www.wikidata.org/wiki/Q1193600\",\"https://www.google.com/search?kgmid=/m/0jt0gkj\"]},{\"@type\":\"Thing\",\"description\":\"artificial intelligence chatbot developed by Google\",\"name\":\"Google Gemini\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google_Gemini\",\"https://www.wikidata.org/wiki/Q116698014\",\"https://www.google.com/search?kgmid=/g/11ts49p01g\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - llm cost
  - token pricing
  - prompt caching
  - agent observability
  - cost monitoring
  - opentelemetry
title: "LLM cost per project: Formula and run-level checks"
---

**LLM cost per project is the sum of every priced step across runs: uncached input tokens × input rate, plus cached input tokens × cached rate, plus output tokens × output rate (rates per million tokens). Take counts from provider usage fields. Loops, retries, fan-out and growing context push real cost above an estimate, so check it against traces.**

A calculator multiplies planned calls by a flat per-call average, and that average cannot see the one run that looped. An agent feature priced that way can produce a provider invoice well above the estimate, with no obvious cause.

This is for solo operators and small teams shipping an agent, and for platform teams that owe someone a cost ceiling. You will be able to compute llm cost per project from usage fields, find the runs that break the estimate, and cap spend with a scorer and a monitor.

> **Key Takeaways**
> - Each provider names its cached-token field differently, so the cost formula needs a mapping per provider.
> - An average cost per call hides the few runs that cost far more; a run-level view finds them.
> - Judge a cost lever by [cost per run](https://mcpd.sh/docs/cost) before and after a change, not by the rate card.
> - A [latching cost monitor](https://mcpd.sh/docs/alerting), like the one in mcpd, fires once instead of on every evaluation.

## How do you calculate LLM cost per project?

You calculate LLM cost per project by pricing every model call from the usage object the provider returns, then adding those prices across every run in the project. For one call, the formula is:

![Illustration for How do you calculate LLM cost per project?](/blog/llm-cost-per-project/image-1-098e521f2866.webp)

```text
cost = (T_in x R_in + T_cached x R_cached + T_out x R_out) / 1,000,000
```

`T_in` is uncached input tokens, `T_cached` is cached input tokens, `T_out` is output tokens, and each `R` is a per-million-token rate from the provider's pricing page.

Each provider reports the cached portion under its own field. OpenAI returns it as [`cached_tokens` inside the usage object](https://platform.openai.com/docs/guides/prompt-caching), and Anthropic as [`cache_read_input_tokens`](https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching).

OpenAI's cached count is a detail of the prompt total, so subtract it from the input count before applying `R_in`.

Check each provider's usage reference for whether its input field already excludes cached tokens. If it does not and you price cached tokens separately as well, you charge them twice.

Reasoning tokens need no extra term. OpenAI's reasoning guide says they are [billed as output tokens](https://platform.openai.com/docs/guides/reasoning), and Google's pricing page labels Gemini output as ["including thinking tokens"](https://ai.google.dev/gemini-api/docs/pricing). Use the output count the provider reports.

Assume a usage object reports 12,000 input tokens, 8,000 of them cached, and 600 output tokens. Then `T_in` is 12,000 minus 8,000, which is 4,000, `T_cached` is 8,000 and `T_out` is 600:

```text
cost = (4,000 x R_in + 8,000 x R_cached + 600 x R_out) / 1,000,000
```

Substitute your model's three rates from its pricing page on the day you run the calculation. Rates change, so keep the variables in your code and the numbers in a table you can update.

A run's cost is the sum of every model call in its trace, retries and each pass of an agent loop included. Your llm cost per project is the sum of every run tagged with that project. The [cost accounting docs](https://mcpd.sh/docs/cost) cover pricing each step this way.

Use a tokenizer to estimate llm cost per project before a call, and the usage fields to measure it after.

## Why is LLM cost per project higher than calls times average?

LLM cost per project runs higher than calls times average because each call in an agent run is priced on a history that keeps growing, while loops, retries and parallel tool calls multiply the number of calls. The familiar failure is a cheap model, a modest per-call estimate, and one run that looped overnight into a large bill.

Growing history is the main driver of llm cost per project. [Anthropic's Messages API documentation](https://docs.anthropic.com/en/api/messages-examples) says that "The Messages API is stateless, which means that you always send the full conversational history to the API." Every step therefore pays again for every earlier step.

Three things multiply the call count:

- Agent loops turn one task into many model calls, and the count depends on the task.
- Retries repeat a whole call, history included, and bill it again.
- Tool-call fan-out issues several calls in parallel, each carrying the same history.

Assume the system prompt and task take 2,000 input tokens, and each step adds 1,500 tokens of tool result and reply. Step 1 sends 2,000 input tokens, step 2 sends 3,500, step 3 sends 5,000, and step 10 sends 15,500.

The 10 steps total 87,500 input tokens. Multiplying 10 calls by the first call's 2,000 gives 20,000.

Now let a tool keep returning an error while the agent keeps retrying. After 30 steps the last call alone sends 45,500 input tokens, and the run totals 712,500, about eight times the healthy 10-step run.

An average over thousands of runs barely moves when that happens, while the one looping run carries most of the llm cost per project.

Summing priced steps per run replaces the calls-times-average estimate of llm cost per project. Read each model call's usage from the trace, price it once, and group the totals by run.

The looping run then sits far above the rest, and its steps explain it: the same tool span failing, the same model span following, and the input tokens climbing at each repeat. The [runs guide](https://mcpd.sh/docs/runs) shows how to trace a failing step back through a run.

## Which levers lower LLM cost per project?

Five levers lower LLM cost per project, each through a mechanism the providers document:

- Prompt caching
- Batch tiers
- Model tiering
- Shorter context
- A lower thinking setting

Prompt caching lowers llm cost per project by moving repeated input from the input rate to the cached rate.

[Anthropic's prompt caching docs](https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching) split the count into `cache_creation_input_tokens` and `cache_read_input_tokens`, and price a 5-minute cache write at "1.25 times the base input tokens price". Anthropic also sets a minimum cacheable prompt length that varies by model.

With every provider, put the stable prefix first (system prompt, tool schemas), because a cache matches on the start of the prompt.

Batch tiers trade latency for a lower llm cost per project. [OpenAI's Batch API guide](https://platform.openai.com/docs/guides/batch) describes its Batch API as "50% lower costs" with a "24-hour turnaround time".

That suits evaluation sets, backfills and nightly enrichment, and it does not suit a user waiting on an answer. Check each provider's pricing page for its batch rate and window before you move a workload.

Model tiering sends classification, extraction and formatting steps to a smaller tier and keeps the larger model for planning. Listed rates on [CostPerPrompt](https://costperprompt.com/) show the gap: Claude Opus 5 at $5.00 input and $25.00 output per million tokens, Claude Sonnet 5 at $2.00 and $10.00.

A step that moves down a tier pays the lower rate on every call in every run, which lowers llm cost per project.

Shorter context cuts llm cost per project at every later call, because input is billed again on every step. Trim history, truncate tool output and summarise old turns.

Long prompts can also change the rate itself. [Google's Gemini pricing](https://ai.google.dev/gemini-api/docs/pricing) lists separate rates around "200k tokens", so a prompt that crosses the line is priced in a different tier.

A lower thinking setting cuts output. A high reasoning or thinking setting raises the output count on every call that uses it, and those tokens are priced at the output rate.

Judge each lever against the run-level llm cost per project read from traces, not against a rate card. A lower rate that triggers more retries, or a shorter prompt that sends an agent round another loop, can raise the cost of a run.

mcpd prices each step at ingest and [charges cached tokens separately](https://mcpd.sh/docs/cost), so the effect of a change shows up as cost per run before and after.

## How do you work out cost per run, session and user?

You work out cost per run, session and user by pricing every step once and adding the steps up through each level.

1. Price each step. A step is one priced model call, with each token type at its own rate. mcpd prices each step once at ingest from a [price book](https://mcpd.sh/docs/cost) of per-million-token rates, with cached tokens, long-prompt tiers and service tiers handled.

 Because the price is fixed when the step is stored, a later price-book change does not silently reprice runs you have already read.

2. Sum steps into a run. Cost per run is the total of its steps, every loop turn and retry included. Open a run in the interface and compare it with the steps you expected: a run with 40 model calls where you planned for 6 is already explained.

3. Roll runs up to sessions and users. Cost per session is the sum of its runs, and cost per user is the sum of that user's sessions.

 Tag each run with its session and user at the source so the roll-up has something to group on. The [concepts docs](https://mcpd.sh/docs/concepts) define the session and project vocabulary. The result is a measured figure per user rather than a flat per-query assumption.

4. Check for gaps before trusting a total. An unknown model has no cost, not a cost of zero. A step with no price shows as a gap, so it cannot pass for a cheap run. If a model is missing, add its rates to a price book. You can supply your own book.

5. Read the p50 and the tail of cost per run. If 99 of 100 runs cost $0.02 each and one looping run costs $4.00, the mean is $0.0598, about three times the typical run, while the p50 stays at $0.02.

 Sort by cost and open the most expensive run. The p50 tells you what a normal run costs, and the tail points to the run that needs fixing.

A finished roll-up has an llm cost per project total that equals the sum of its runs, no unpriced steps, and a p50 you can multiply by expected volume. Budget the tail separately.

## How do you check an LLM cost per project estimate against actual spend?

You check an LLM cost estimate against actual spend by recording the usage of every model call, summing it by project and environment, and comparing that total with the formula's estimate and the provider invoice.

1. Point the exporter at mcpd. Set the two variables below in the environment of any agent, in any framework or with any provider. Add the project key from your project at [mcpd.sh/app](https://mcpd.sh/app) as an OTLP header (the [OpenTelemetry docs page](https://mcpd.sh/docs/opentelemetry) shows the header).

 Nothing is proxied: calls still go straight to the model provider.

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

2. Confirm the usage attributes arrive.

 [mcpd's integrating guide](https://mcpd.sh/docs/integrating) says mcpd reads both `gen_ai.*` and the OpenInference `llm.*` conventions, so existing instrumentation works unchanged. If you have none yet, the same guide compares the HTTP API, the SDKs and MCP.

3. Check the record per call. Each model call needs the model name; input, output and cached tokens; the run id and the session id; and the user and the environment.

4. Tag the environment on every service. A staging agent that leaves the environment unset lands in the production total and inflates it, so set it once per deployment.

5. Roll up by project and environment. Sum the priced steps for the same dates and time zone as the invoice period. Use the production environment only, because the invoice covers every call made with your key, while your estimate covers production.

6. Compare three numbers. Put the formula's estimate, the traced total and the provider invoice side by side.

 Read the gap with this table.

| Result | Likely cause | Next check |
| --- | --- | --- |
| Traced total above the estimate | Loops, retries or context growing at each step | Sort that period's runs by cost and open the top one |
| Traced total below the invoice | Some calls are not instrumented, or a model has no price | Check instrumentation and the price book |

A good result is a traced total within a few percent of the invoice and a short list of runs that explain any gap against the estimate. 

[Start free on the hosted version](https://mcpd.sh/app) and send one real run to see its cost per step. To run it yourself, build from source at github.com/loruorg/mcpd.

## How do you turn an LLM cost per project estimate into a guardrail?

You turn a cost estimate into a guardrail by enforcing it twice: a `cost_budget` scorer grades every run against a per-run ceiling, and a cost monitor watches spend across the project.

1. Set the ceiling from a normal run. Read the cost of a typical successful run from your traces and add headroom.

 Do not base it on the average of all runs. The runaway runs are in that average, and they would raise the limit that is meant to catch them.

2. Attach the `cost_budget` scorer. It passes or fails each run against the ceiling. Filter for failed scores to find the one expensive run among thousands.

3. Create a cost monitor. [mcpd's alerting documentation](https://mcpd.sh/docs/alerting) says monitors are evaluated every 60 s, and you give the monitor a webhook that reaches your pager or chat.

4. Expect a latch. The monitor fires once, then stays quiet. Without the latch, a spike lasting an hour would page you 60 times, once per evaluation.

5. Wait for a real reading before it resolves. A window with no runs gives no reading, and silence is not recovery. If an outage stops traffic, the alarm must not clear just because nothing was measured.

Assume a normal run costs $0.05, the scorer ceiling is $0.25, and a looping tool call pushes project spend over the monitor's threshold at 14:03. The next evaluation fires the monitor and calls your webhook once.

The evaluations at 14:04, 14:05 and every minute after that send nothing more. The monitor resolves only when a reading comes back under the threshold, and the failed `cost_budget` scores point you to the run that did it.

## Frequently asked questions

### How do I calculate LLM cost per project?

Read input, cached and output token counts from each call's usage fields, multiply each by its per-million-token rate from the provider's pricing page, and add the results across every call in every run. Then compare the total with spend read from traces.

### Do reasoning tokens cost extra?

They cost the output rate. [OpenAI's reasoning guide](https://platform.openai.com/docs/guides/reasoning) says reasoning tokens are billed as output tokens even though the API doesn't show them, so a call's output count can exceed the length of the visible answer. Price the output count from the usage fields, not from the text you can read.

### Does an LLM cost calculator include retries?

A calculator includes retries only if you add them to the call count by hand. A typical tool such as [IdeaPlan's LLM cost calculator](https://www.ideaplan.io/tools/llm-cost-estimator) multiplies the planned number of calls by average tokens, so loop iterations and context that grows each step are missing too. Summing priced steps from traces counts every call that ran.

### What happens if a model has no price in the price book?

mcpd records no cost for that step, not a cost of zero. A zero would quietly lower the project total, while a missing cost shows which model needs a rate. The [cost accounting docs](https://mcpd.sh/docs/cost) cover how pricing works at ingest.

## Next steps: from estimate to guardrail

**Today:** Set `OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh` on one agent and read its cost per run in the interface.

**This week:** Compare the formula's estimate for that agent with the summed cost of its runs. Open the most expensive runs and read their steps, because a looping tool call or retry storm shows up as one run far above the rest.

**This month:** Apply the one lever your data supports: caching, a cheaper model for simple steps, or shorter context. Then set a `cost_budget` scorer on every run and a cost monitor on the project, so a runaway run calls your webhook once instead of surfacing on an invoice.

Your llm cost per project estimate stays a forecast until traces check it. [Start free at mcpd.sh/app](https://mcpd.sh/app), and follow the [quickstart](https://mcpd.sh/docs/quickstart) to your first trace. If you would rather self-host, build from source at github.com/loruorg/mcpd.

## Sources

[Alerting · mcpd](https://mcpd.sh/docs/alerting) · [Prompt caching | OpenAI API](https://platform.openai.com/docs/guides/prompt-caching) · [Prompt caching | Anthropic](https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching) · [Reasoning models | OpenAI API](https://platform.openai.com/docs/guides/reasoning) · [Gemini Developer API pricing | Google AI for Developers](https://ai.google.dev/gemini-api/docs/pricing) · [Messages examples - Anthropic](https://docs.anthropic.com/en/api/messages-examples) · [Batch API | OpenAI API](https://platform.openai.com/docs/guides/batch) · [Cost accounting · mcpd](https://mcpd.sh/docs/cost). Every other source is linked where the article cites it.
