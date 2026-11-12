---
description: What does LLM trace sampling need? Keep every errored, slow, costly or looping run, sample the rest by trace ID, and leave metrics unsampled. Learn how.
faq:
  - answer: Keep every trace that errored, ran slow, went over budget, looped on a tool, scored low or drew negative feedback. Pick the rest at a fixed ratio from a hash of the trace ID, and leave counters and metrics unsampled so your totals stay correct.
    question: How should I sample LLM traces?
  - answer: "Head sampling is not enough for an LLM app, because it controls volume and nothing else. It decides before the outcome is known, so it can't keep a run because it later looped or scored low. OpenTelemetry's sampling concepts say the same: it can't decide from the entire trace."
    question: Is head sampling enough for an LLM app?
  - answer: Sampling changes your cost and error-rate numbers when you compute them from stored traces, because dropped runs vanish from every sum. Counters and metrics see every run, so computing volume, cost and error rate from them keeps your totals correct even when traces are sampled.
    question: Does sampling change my cost and error-rate numbers?
  - answer: Tail sampling does need a Collector in OpenTelemetry, because the tail_sampling processor lives in the Collector contrib. mcpd offers a tail mode in the daemon itself, so your OpenTelemetry exporter can point at api.mcpd.sh directly instead of going through a separate Collector.
    question: Does tail sampling need a Collector?
  - answer: "The prompt of a dropped trace isn't stored at all, so you can't search for it later. Only the metadata you choose to keep remains for that trace, such as the counts and cost totals that your metrics record for every run."
    question: What happens to the prompt of a dropped trace?
image: /blog/llm-trace-sampling/featured-e0d5d97aef9a.webp
image_alt: "Illustration for LLM trace sampling: Keep the runs that matter"
publishDate: "2026-11-12"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/llm-trace-sampling#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\",\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]}],\"datePublished\":\"2026-11-12\",\"description\":\"What does LLM trace sampling need? Keep every errored, slow, costly or looping run, sample the rest by trace ID, and leave metrics unsampled. Learn how.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Keep every trace that errored, ran slow, went over budget, looped on a tool, scored low or drew negative feedback. Pick the rest at a fixed ratio from a hash of the trace ID, and leave counters and metrics unsampled so your totals stay correct.\"},\"name\":\"How should I sample LLM traces?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Head sampling is not enough for an LLM app, because it controls volume and nothing else. It decides before the outcome is known, so it can't keep a run because it later looped or scored low. OpenTelemetry's sampling concepts say the same: it can't decide from the entire trace.\"},\"name\":\"Is head sampling enough for an LLM app?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Sampling changes your cost and error-rate numbers when you compute them from stored traces, because dropped runs vanish from every sum. Counters and metrics see every run, so computing volume, cost and error rate from them keeps your totals correct even when traces are sampled.\"},\"name\":\"Does sampling change my cost and error-rate numbers?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Tail sampling does need a Collector in OpenTelemetry, because the tail_sampling processor lives in the Collector contrib. mcpd offers a tail mode in the daemon itself, so your OpenTelemetry exporter can point at api.mcpd.sh directly instead of going through a separate Collector.\"},\"name\":\"Does tail sampling need a Collector?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"The prompt of a dropped trace isn't stored at all, so you can't search for it later. Only the metadata you choose to keep remains for that trace, such as the counts and cost totals that your metrics record for every run.\"},\"name\":\"What happens to the prompt of a dropped trace?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\",\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q8777\",\"@type\":\"Thing\",\"description\":\"application-layer protocol for transferring web resources\",\"name\":\"HTTP\",\"sameAs\":[\"https://en.wikipedia.org/wiki/HTTP\",\"https://www.wikidata.org/wiki/Q8777\",\"https://www.google.com/search?kgmid=/m/03hgt\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q364\",\"@type\":\"Organization\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]}],\"name\":\"LLM trace sampling: Keep the runs that matter\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/bjUQDDH_ORs-SWXQtczBZfCXleqqHlJE.webp\"},\"url\":\"https://mcpd.sh/blog/llm-trace-sampling\"},{\"@id\":\"https://mcpd.sh/blog/llm-trace-sampling#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"Sampling | OpenTelemetry\",\"url\":\"https://opentelemetry.io/docs/concepts/sampling/\"},{\"@type\":\"CreativeWork\",\"name\":\"Evaluation · mcpd\",\"url\":\"https://mcpd.sh/docs/evaluation\"},{\"@type\":\"CreativeWork\",\"name\":\"Cost · mcpd\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"Tail Sampling Processor README, OpenTelemetry Collector contrib\",\"url\":\"https://github.com/open-telemetry/opentelemetry-collector-contrib/blob/main/processor/tailsamplingprocessor/README.md\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"Semantic conventions for generative AI client spans, OpenTelemetry\",\"url\":\"https://dev.to/nainikmehta/llm-observability-trace-cost-sampling-privacy-7gp\"},{\"@type\":\"CreativeWork\",\"name\":\"Trace Sampling - Confident AI Docs\",\"url\":\"https://www.confident-ai.com/docs/llm-tracing/features/sampling\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry Trace SDK specification\",\"url\":\"https://opentelemetry.io/docs/specs/otel/trace/sdk/\"},{\"@type\":\"CreativeWork\",\"name\":\"Sampling · mcpd\",\"url\":\"https://mcpd.sh/docs/sampling\"},{\"@type\":\"CreativeWork\",\"name\":\"Privacy · mcpd\",\"url\":\"https://mcpd.sh/docs/privacy\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd README\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"Tail Sampling Processor, opentelemetry-collector-contrib\",\"url\":\"https://github.com/open-telemetry/opentelemetry-collector-contrib/tree/main/processor/tailsamplingprocessor\"}],\"dateModified\":\"2026-11-12\",\"datePublished\":\"2026-11-12\",\"description\":\"What does LLM trace sampling need? Keep every errored, slow, costly or looping run, sample the rest by trace ID, and leave metrics unsampled. Learn how.\",\"headline\":\"LLM trace sampling: Keep the runs that matter\",\"image\":\"https://api.birchseek.com/m/bjUQDDH_ORs-SWXQtczBZfCXleqqHlJE.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/llm-trace-sampling#webpage\"},\"keywords\":[\"llm trace sampling\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/llm-trace-sampling#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\"},{\"@id\":\"https://www.wikidata.org/wiki/Q8777\"},{\"@id\":\"https://www.wikidata.org/wiki/Q364\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2987},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"Chinese artificial intelligence company\",\"name\":\"DeepSeek\",\"sameAs\":[\"https://en.wikipedia.org/wiki/DeepSeek\",\"https://www.wikidata.org/wiki/Q131577453\",\"https://www.google.com/search?kgmid=/g/11wvrb0s91\"]},{\"@type\":\"Thing\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]},{\"@type\":\"Thing\",\"description\":\"artificial intelligence tool for code autocompletion\",\"name\":\"GitHub Copilot\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub_Copilot\",\"https://www.wikidata.org/wiki/Q107435063\",\"https://www.google.com/search?kgmid=/g/11q83qbj3d\"]},{\"@type\":\"Thing\",\"description\":\"defined sequence of activities of an organizational entity, enabled by the systematic organization of resources into processes that transform materials, provide services, or process information\",\"name\":\"Workflow\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Workflow\",\"https://www.wikidata.org/wiki/Q627335\",\"https://www.google.com/search?kgmid=/m/0k5rb\"]},{\"@type\":\"Thing\",\"description\":\"maximum number of tokens that a large language model can process at one time\",\"name\":\"Context window\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Context_window\",\"https://www.wikidata.org/wiki/Q125619872\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"process in which information about the past or the present influences the same phenomenon in the present or future; it occurs when outputs of a system are routed back as inputs as part of a chain of cause-and-effect that forms a circuit or loop\",\"name\":\"Feedback\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Feedback\",\"https://www.wikidata.org/wiki/Q183635\",\"https://www.google.com/search?kgmid=/m/03232\"]},{\"@type\":\"Thing\",\"description\":\"chatbot developed by SpaceXAI\",\"name\":\"Grok\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Grok_(chatbot)\",\"https://www.wikidata.org/wiki/Q123361035\",\"https://www.google.com/search?kgmid=/g/11l6lp1dv3\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - llm observability
  - trace sampling
  - opentelemetry
  - tail sampling
  - ai agents
  - cost tracking
title: "LLM trace sampling: Keep the runs that matter"
---

**LLM trace sampling works when you keep traces by evidence and sample only the remainder. Keep every trace that errored, ran slow, went over its cost budget, looped on a tool, scored low or drew negative feedback; sample the rest at a fixed ratio decided deterministically by trace ID. Leave counters and metrics unsampled, so volume, cost and error rate stay correct.**

This is for engineers whose agent traces have outgrown their storage budget and who are about to switch on a global ratio. A ratio decided at the root drops a run that loops on a tool and still returns a success status as readily as any routine run.

You'll be able to write an LLM trace sampling policy with real attribute names, keep traces whole across services, keep cost and error metrics accurate, and find a kept run again by searching what the model said.

> **Key Takeaways**
> - Name your cost attribute yourself and confirm your instrumentation emits it.
> - An unknown model has no cost, not zero cost, so a cost threshold never treats it as free.
> - Tail sampling needs every span of a trace on the same Collector instance, so route by trace ID.
> - Redact before storage and set retention in days, for kept payloads and dropped-run metadata alike.

## What is LLM trace sampling, and why does a global ratio fail?

LLM trace sampling records only a chosen share of traces to control storage and cost, and a global ratio fails because it decides before the run has done anything worth keeping. A trace here is the full tree of model calls and tool calls in a run.

![Illustration for What is LLM trace sampling, and why does a global ratio fail?](/blog/llm-trace-sampling/image-1-60d2f48e8348.webp)

In LLM trace sampling, head sampling decides when the trace starts. Tail sampling decides after it finishes, so it's able to keep errors, slow runs, expensive runs and low-scored runs.

The [OpenTelemetry sampling documentation](https://opentelemetry.io/docs/concepts/sampling/) defines both modes. It says "Head sampling is a sampling technique used to make a sampling decision as early as possible", and that tail sampling is where "the decision to sample a trace takes place by considering all or most of the spans within the trace."

The same page states the cost of deciding early: "you cannot ensure that all traces with an error within them are sampled with head sampling alone."

A run is one execution of your agent. Its trace is the tree of spans (model calls, tool calls, retrievals) under a root span, as the [mcpd concepts page](https://mcpd.sh/docs/concepts) defines them.

A refund agent that calls the same lookup tool again and again burns tokens, replies politely and finishes with an OK status. A rule on status code sees nothing wrong with that run.

A global ratio decides at the root span, before the loop exists, so it keeps such runs only at its configured share. A routine run and the run behind a complaint are equally likely to be dropped.

Tail sampling can see the loop, but something has to hold the spans until the trace is complete. Only then can an LLM trace sampling policy read evidence from the finished trace, and that's the job of its keep rules.

## Which LLM traces should a sampling policy always keep?

A sampling policy should always keep six kinds of trace: runs that errored, ran over a latency budget, went over a cost budget, looped on a tool, scored low or drew negative feedback. Each rule catches a different failure and reads a different signal.

| Keep rule | What it catches | Signal it reads |
|---|---|---|
| Error | A model call or tool call that failed | Span status: any span in the run with status error keeps the whole trace |
| Latency over a budget | A run that finished but made the user wait, such as a retry storm or a slow tool | The run's duration, read from the root span |
| Cost over a budget | The run behind a spend spike: a huge prompt, a long completion or too many steps | The cost recorded on the run's steps |
| Tool loop | An agent that calls the same tool again and again in one run and still ends with an OK status | Repeated tool calls inside a single run |
| Low score | A wrong or empty answer that no status code reveals | A score, with the scale it was measured against |
| Negative feedback | What a customer or reviewer judged wrong when no machine check did | Feedback from a person |

Every rule after the error rule catches runs that finish with a success status, which a keep-errors-only policy never sees.

[mcpd's evaluation docs](https://mcpd.sh/docs/evaluation) separate the last two signals: "A score is what a machine thought. Feedback is what a person thought. mcpd keeps them as two signals and never averages one into the other." A keep rule can read either one.

Aspect notes are a third, separate signal: one sentence about a finished run along one axis.

Cost needs care. According to [mcpd's cost documentation](https://mcpd.sh/docs/cost), mcpd prices each step once, at ingest, from a price book of per-million-token rates, and an unknown model has no cost, not a cost of zero.

A cost-threshold rule therefore never treats an unpriced model as free. Code that defaults the cost to `0.0` does the opposite: the rule silently never fires for that model.

Set every budget and cutoff in your LLM trace sampling policy from your own traffic. Read what your normal runs cost and how long they take, then place the line above that. Don't sample a run unless it matches none of the six rules.

The looping refund agent has an OK status, so the error rule skips it, and the tool-loop or latency rule keeps it. The [runs guide](https://mcpd.sh/docs/runs) shows how to read a kept run step by step, from the first repeated call to the answer that went out.

## How do you write an LLM trace sampling policy in OpenTelemetry?

An OpenTelemetry LLM trace sampling policy is a list of keep rules in the Collector's `tail_sampling` processor, with a `probabilistic` rule last as the ratio for everything else. The processor keeps a trace when any policy decides to sample it, so the list works as an OR-chain.

The Collector contrib README for the [tail sampling processor](https://github.com/open-telemetry/opentelemetry-collector-contrib/blob/main/processor/tailsamplingprocessor/README.md) documents each key in the policy.

1. Set the buffer. Size `decision_wait` and `num_traces` from your own run length and traffic. A decision wait shorter than your longest run splits that run.
2. Add the error rule. A `status_code` policy with `status_codes: [ERROR]` keeps failed runs.
3. Add the latency rule. A `latency` policy takes `threshold_ms`. Set it from your own latency distribution.
4. Add token rules. A `numeric_attribute` policy takes `key`, `min_value` and `max_value`. Point it at `gen_ai.usage.input_tokens` and `gen_ai.usage.output_tokens`. These attributes only exist if your instrumentation sets them, so check one real span first.
5. Add a cost rule on your own attribute. The policy reads `app.llm.cost_usd`, a custom attribute your code must set. It isn't one of the `gen_ai.*` names, so a model without a price should leave it unset instead of writing 0.
6. Add a model rule. A `string_attribute` policy with `key` and `values` keeps every trace that touched a model you're rolling out.
7. End with the ratio. A `probabilistic` policy with `sampling_percentage` keeps a share of what no rule caught.

```yaml
processors:
  tail_sampling:
    decision_wait: 10s        # longer than your longest run
    num_traces: 50000         # size to your traffic and memory
    policies:
      - name: keep-errors
        type: status_code
        status_code:
          status_codes: [ERROR]
      - name: keep-slow
        type: latency
        latency:
          threshold_ms: ${env:LLM_LATENCY_BUDGET_MS}
      - name: keep-heavy-input
        type: numeric_attribute
        numeric_attribute:
          key: gen_ai.usage.input_tokens
          min_value: ${env:LLM_INPUT_TOKEN_BUDGET}
      - name: keep-heavy-output
        type: numeric_attribute
        numeric_attribute:
          key: gen_ai.usage.output_tokens
          min_value: ${env:LLM_OUTPUT_TOKEN_BUDGET}
      - name: keep-costly
        type: numeric_attribute
        numeric_attribute:
          key: app.llm.cost_usd   # custom attribute, not gen_ai.*
          min_value: ${env:LLM_COST_BUDGET_USD}
      - name: keep-new-model
        type: string_attribute
        string_attribute:
          key: gen_ai.request.model
          values:
            - ${env:LLM_ROLLOUT_MODEL}
      - name: fallback-ratio
        type: probabilistic
        probabilistic:
          sampling_percentage: ${env:LLM_FALLBACK_PERCENT}
```

A status rule alone misses a run that looped on a tool and still returned success. The token and cost rules catch it, because a looping run piles up tokens.

The `gen_ai.request.model` and `gen_ai.usage.*` names come from the OpenTelemetry [GenAI span conventions](https://dev.to/nainikmehta/llm-observability-trace-cost-sampling-privacy-7gp). They only exist if your instrumentation sets them, so check a real span before you pin dashboards to the names.

Existing instrumentation may use the OpenInference `llm.*` names instead, and the policy keys must match whichever your spans carry. mcpd reads both conventions, as its [OpenTelemetry docs page](https://mcpd.sh/docs/opentelemetry) describes.

## How do you keep a sampled trace whole?

You keep a sampled trace whole by making one decision per trace, at its root, and having every later span follow it. [Confident AI's sampling documentation](https://www.confident-ai.com/docs/llm-tracing/features/sampling) describes the goal: "Sampling is decided once, at the start of each trace. That means a trace is either fully exported or not at all."

The failure to avoid is a ratio applied per span or per service. If the agent service keeps a share of its spans and the tool service rolls its own dice, you get an agent span with no tool spans, or tool spans with no agent above them.

A refund run that looped then shows the loop with no parent, or a parent with no loop.

The [OpenTelemetry Trace SDK specification](https://opentelemetry.io/docs/specs/otel/trace/sdk/) lists four built-in samplers: `AlwaysOn`, `AlwaysOff`, `TraceIdRatioBased` and `ParentBased`.

A ratio computed from the trace ID gives every process the same answer for the same trace, because the input is the same everywhere. `ParentBased` follows the root's decision, carried in the W3C trace-context sampled flag, so child spans are never decided again.

mcpd's [sampling documentation](https://mcpd.sh/docs/sampling) uses the same idea: the decision is made at the root, and a deterministic hash lets several processes reach the same decision for the same trace.

Tail sampling decides after the trace finishes, so it has operating limits. The Collector contrib README for the [`tail_sampling` processor](https://github.com/open-telemetry/opentelemetry-collector-contrib/blob/main/processor/tailsamplingprocessor/README.md) says spans are held until `decision_wait` expires, and `num_traces` bounds how many traces stay in memory. Longer waits and busier traffic mean more memory.

The README also states that "all spans for a given trace MUST be received by the same collector instance for effective sampling decisions." In a load-balanced fleet, route by trace ID in front of the tail samplers, so no instance decides on half a trace.

## Why does LLM trace sampling undercount cost and error rate?

Sampled traces distort cost and error rate because a dashboard built from stored traces can only count what was stored. If 1 in N traces is kept, a count of runs, tokens or dollars computed from the stored set is about 1/N of the truth.

A month of spend read from those traces looks like one Nth of the month you were billed for.

Error rate drifts the other way. A policy that keeps every errored trace and only a fraction of the successes stores nearly all failures and few good runs, so the error share in the stored set is inflated. The same chart shows too little volume and too high a failure share.

The fix is to take volume, cost and error rate from signals that LLM trace sampling never touches:

- Counters of runs, errors, tokens and cost, incremented for every run.
- Histograms of latency, so a p95 reflects every run and not only the kept ones.
- Metrics with exemplars. An exemplar attaches a trace ID to a data point, so a spike on a chart links to one run you can open.

A monitor on `error_rate`, `latency_p95`, cost or `trace_count` should read these signals. Pointed at sampled traces, it watches a thinned series and its thresholds drift with the ratio.

When a trace is dropped, keep a cheap record in its place: the model, input and output token counts, the priced cost, the status and the duration. It holds no prompt or completion, so it costs little to store.

That record should increment the same counters your keep rules read. The keep path and the drop path then agree, and a run that sampling discards still appears in every total.

A dropped run on a model missing from the price book still has no cost, so its record must leave cost unknown rather than write 0. The [cost guide](https://mcpd.sh/docs/cost) covers how mcpd prices a run.

## How do redaction and retention interact with LLM trace sampling?

Sampling decides which traces are stored, redaction decides what text is written for them, and retention decides how many days it stays. Each step can only act on what the previous one let through, so the order matters.

Redaction has to come first. The [mcpd privacy docs](https://mcpd.sh/docs/privacy) put the reason in one line: "Redaction runs before the write, because a secret deleted afterwards was still written down."

The modes are drop, mask, partial and keyed hash, and they apply to every signal, not only to spans.

LLM trace sampling doesn't replace redaction. A kept trace holds the full prompt and completion, so a card number in a customer's message is a leak the moment that trace is stored. Set redaction for the fields you can't afford to write, then let sampling decide how many traces carry the rest.

Storing the unsampled majority in a second, lighter tier is a policy you set yourself:

- Kept traces hold the full payload. The prompt and the completion are the evidence, and you can't investigate a wrong answer without them.
- Everything else holds metadata only: model, tokens, cost, duration and status. That's enough to keep volume, spend and error rate accurate, at a fraction of the bytes.

Retention is set in days. Choose the number from how long an investigation of a flagged run takes you, and apply it to the whole store.

For a request about a person, mcpd supports erasure requests, so you can remove one individual's data without waiting for the retention clock. The [mcpd security guide](https://mcpd.sh/docs/security) covers what is stored and where it lives.

## How do you sample beside the agent with mcpd?

You sample beside the agent with mcpd by sending traces to the daemon and choosing a sampling mode there, so the keep-or-drop decision never runs in your agent's call path. As the [mcpd README](https://github.com/loruorg/mcpd) puts it, mcpd is not a gateway or a proxy: "Nothing travels through mcpd to reach a model, so mcpd being slow or down cannot slow your agent or break it."

1. Point your exporter at mcpd, with no Collector in between. For the hosted version, create a project at [mcpd.sh/app](https://mcpd.sh/app), copy its key and set:

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

 To self-host, build from source at github.com/loruorg/mcpd with `make build`, then run `./target/release/mcpd demo`. The sampling configuration is the same on both.

2. Choose a mode. mcpd's [sampling modes](https://mcpd.sh/docs/sampling) are always, never, ratio, deterministic and tail. Use deterministic rather than a random ratio when more than one process emits spans: the decision comes from the trace ID, so every process reaches the same answer and a run is never half-recorded. Take the exact option names and the tail conditions from the sampling page.

3. Match tail sampling to its limits. The daemon holds a trace's spans in memory until the run finishes, so memory grows with run length and with how long the decision waits. Send each agent's spans to one daemon, or each process judges a fragment.

4. Add a relay for several hosts or regions.

5. Check the result. Run the agent once. A kept run shows every span in the waterfall, and a dropped run leaves nothing behind.

When a customer reports a bill spike, you have no trace ID to search by. Search for a sentence the model wrote, such as its repeated apology, and mcpd's full-text search over the payload finds the run. In the waterfall, the repeated tool calls show the loop as a stack of identical steps.

To try this on your own agent, [start free on the hosted version](https://mcpd.sh/app).

## Frequently asked questions

### How should I sample LLM traces?

Run keep rules for error, latency, cost, tool loop, low score and negative feedback in a tail sampler, and pass only the remainder to a ratio computed from the trace ID. Count volume, cost and errors outside sampling.

### Is head sampling enough for an LLM app?

Head sampling controls volume and nothing else. It decides before the outcome is known, so it can't keep a run because it later looped or scored low. [OpenTelemetry's sampling concepts](https://opentelemetry.io/docs/concepts/sampling/) say the same: it can't decide from the entire trace.

### Does sampling change my cost and error-rate numbers?

It does when you compute them from stored traces, because dropped runs vanish from every sum. Compute volume, cost and error rate from counters and metrics, which see every run.

### Does tail sampling need a Collector?

In OpenTelemetry it does: the [`tail_sampling` processor](https://github.com/open-telemetry/opentelemetry-collector-contrib/tree/main/processor/tailsamplingprocessor) lives in the Collector contrib. mcpd offers a [tail mode in the daemon](https://mcpd.sh/docs/sampling), so your exporter can point at `https://api.mcpd.sh` directly.

### What happens to the prompt of a dropped trace?

It isn't stored, so you can't search for it later. Only the metadata you choose to keep remains, such as the counts and cost totals in your metrics.

## How do you start LLM trace sampling today?

Start LLM trace sampling in three steps, and sample nothing until you've seen real traces.

- Today: point an OpenTelemetry exporter at `https://api.mcpd.sh` and watch real traces arrive.
- This week: write the keep rules for error, latency, cost, tool loop, low score and negative feedback, with the ratio rule as the fallback. Move counters and metrics outside sampling.
- This month: compare the runs you kept with the runs people complained about. Set redaction and retention in days, then tighten the ratio.

The [sampling guide](https://mcpd.sh/docs/sampling) lists the modes and their options.

## Sources

[Sampling | OpenTelemetry](https://opentelemetry.io/docs/concepts/sampling/) · [Evaluation · mcpd](https://mcpd.sh/docs/evaluation) · [Cost · mcpd](https://mcpd.sh/docs/cost) · [Tail Sampling Processor README, OpenTelemetry Collector…](https://github.com/open-telemetry/opentelemetry-collector-contrib/blob/main/processor/tailsamplingprocessor/README.md) · [Semantic conventions for generative AI client spans…](https://dev.to/nainikmehta/llm-observability-trace-cost-sampling-privacy-7gp) · [OpenTelemetry Trace SDK specification](https://opentelemetry.io/docs/specs/otel/trace/sdk/) · [Sampling · mcpd](https://mcpd.sh/docs/sampling) · [Privacy · mcpd](https://mcpd.sh/docs/privacy). Every other source is linked where the article cites it.
