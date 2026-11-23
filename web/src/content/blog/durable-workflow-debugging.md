---
description: "Durable workflow debugging starts with the complaint: search by what a step returned, then read its timer, lease or retry count on one trace. Start free."
faq:
  - answer: "Open its trace and read the last step's state. A timer or wait_event step means the run is waiting by design, a step with an expired lease means a worker died, and a rising retry count means the step is retrying. Each needs a different fix."
    question: How do I debug a stuck durable run?
  - answer: Search by what the run did. Filter by status or time window, or run a full-text search over step payloads for a phrase the user saw or a value a step returned. Open the matching run and read its step tree on one timeline.
    question: How do I find a workflow run without its ID?
  - answer: Look for two attempts of the same step in the trace. A retry after a failure, or a pickup after a lease expired, leaves a second attempt with its own timing, input and output. If the step sent an email or a refund, check that system too.
    question: How can I tell whether a step ran twice?
  - answer: No specific framework is required; durable workflow debugging needs OpenTelemetry traces. Point an exporter at mcpd and pass the trace context to each worker, and your code keeps its language and structure. Redaction before storage lets you keep exact inputs and outputs without keeping secrets.
    question: Does durable workflow debugging need a specific framework?
image: /blog/durable-workflow-debugging/featured-5134d3c0a70f.webp
image_alt: "Illustration for Durable workflow debugging: Find the bad run fast"
publishDate: "2026-11-23"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/durable-workflow-debugging#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q845566\",\"@type\":\"Thing\",\"description\":\"process of finding and resolving defects or problems within a computer program\",\"name\":\"Debugging\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Debugging\",\"https://www.wikidata.org/wiki/Q845566\",\"https://www.google.com/search?kgmid=/m/01vtxk\"]}],\"datePublished\":\"2026-11-23\",\"description\":\"Durable workflow debugging starts with the complaint: search by what a step returned, then read its timer, lease or retry count on one trace. Start free.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Open its trace and read the last step's state. A timer or wait_event step means the run is waiting by design, a step with an expired lease means a worker died, and a rising retry count means the step is retrying. Each needs a different fix.\"},\"name\":\"How do I debug a stuck durable run?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Search by what the run did. Filter by status or time window, or run a full-text search over step payloads for a phrase the user saw or a value a step returned. Open the matching run and read its step tree on one timeline.\"},\"name\":\"How do I find a workflow run without its ID?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Look for two attempts of the same step in the trace. A retry after a failure, or a pickup after a lease expired, leaves a second attempt with its own timing, input and output. If the step sent an email or a refund, check that system too.\"},\"name\":\"How can I tell whether a step ran twice?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"No specific framework is required; durable workflow debugging needs OpenTelemetry traces. Point an exporter at mcpd and pass the trace context to each worker, and your code keeps its language and structure. Redaction before storage lets you keep exact inputs and outputs without keeping secrets.\"},\"name\":\"Does durable workflow debugging need a specific framework?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q364\",\"@type\":\"Organization\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q2063\",\"@type\":\"Thing\",\"description\":\"text-based open standard designed for human-readable data interchange\",\"name\":\"JSON\",\"sameAs\":[\"https://en.wikipedia.org/wiki/JSON\",\"https://www.wikidata.org/wiki/Q2063\",\"https://www.google.com/search?kgmid=/m/05cntt\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q111031095\",\"@type\":\"Thing\",\"description\":\"quality of a system evolving solely based on its initial conditions, without randomness\",\"name\":\"determinism\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q111031095\"]}],\"name\":\"Durable workflow debugging: Find the bad run fast\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/McKb2qT5LfMNyQN9dKpt9U8cmwVr0l8I.webp\"},\"url\":\"https://mcpd.sh/blog/durable-workflow-debugging\"},{\"@id\":\"https://mcpd.sh/blog/durable-workflow-debugging#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q845566\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"Diagnose and troubleshoot issues in Durable Functions\",\"url\":\"https://learn.microsoft.com/en-us/azure/durable-task/durable-functions/durable-functions-diagnostics\"},{\"@type\":\"CreativeWork\",\"name\":\"Trace Context, W3C Recommendation\",\"url\":\"https://www.w3.org/TR/trace-context/\"},{\"@type\":\"CreativeWork\",\"name\":\"Debugging Workflows - Durable execution for workflows and agents\",\"url\":\"https://conductor-oss.github.io/conductor/devguide/how-tos/Workflows/debugging-workflows.html\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory.\",\"url\":\"https://mcpd.sh\"},{\"@type\":\"CreativeWork\",\"name\":\"Durable Functions Troubleshooting Guide | Microsoft Learn\",\"url\":\"https://learn.microsoft.com/en-us/azure/durable-task/durable-functions/durable-functions-troubleshooting-guide\"},{\"@type\":\"CreativeWork\",\"name\":\"Cost · mcpd\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"Privacy · mcpd\",\"url\":\"https://mcpd.sh/docs/privacy\"},{\"@type\":\"CreativeWork\",\"name\":\"Alerting · mcpd\",\"url\":\"https://mcpd.sh/docs/alerting\"}],\"dateModified\":\"2026-11-23\",\"datePublished\":\"2026-11-23\",\"description\":\"Durable workflow debugging starts with the complaint: search by what a step returned, then read its timer, lease or retry count on one trace. Start free.\",\"headline\":\"Durable workflow debugging: Find the bad run fast\",\"image\":\"https://api.birchseek.com/m/McKb2qT5LfMNyQN9dKpt9U8cmwVr0l8I.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/durable-workflow-debugging#webpage\"},\"keywords\":[\"durable workflow debugging\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/durable-workflow-debugging#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q364\"},{\"@id\":\"https://www.wikidata.org/wiki/Q2063\"},{\"@id\":\"https://www.wikidata.org/wiki/Q111031095\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2886},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"permissive free software license\",\"name\":\"MIT License\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MIT_License\",\"https://www.wikidata.org/wiki/Q334661\",\"https://www.google.com/search?kgmid=/m/04vpm\"]},{\"@type\":\"Thing\",\"description\":\"general-purpose programming language\",\"name\":\"Python\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Python_(programming_language)\",\"https://www.wikidata.org/wiki/Q28865\",\"https://www.google.com/search?kgmid=/m/05z1_\"]},{\"@type\":\"Thing\",\"description\":\"Python library\",\"name\":\"langsmith\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q132393290\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"software to manage containers on a server-cluster\",\"name\":\"Kubernetes\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Kubernetes\",\"https://www.wikidata.org/wiki/Q22661306\",\"https://www.google.com/search?kgmid=/g/11b7lxp79d\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence software company\",\"name\":\"Braintrust\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Braintrust\",\"https://www.wikidata.org/wiki/Q141164091\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"Python library\",\"name\":\"mlflow\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q107381745\"]},{\"@type\":\"Thing\",\"description\":\"creation or optimization of a prompt to be given to an artificial intelligence model\",\"name\":\"Prompt engineering\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Prompt_engineering\",\"https://www.wikidata.org/wiki/Q108941486\",\"https://www.google.com/search?kgmid=/g/11p6kpgt_n\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - durable workflows
  - debugging
  - opentelemetry
  - idempotency
  - payload redaction
  - monitoring
title: "Durable workflow debugging: Find the bad run fast"
---

**Durable workflow debugging is a path from a complaint to one durable run, then to one step's exact input and output, on a single timeline. A stuck run is waiting on a timer or event, waiting for a dead worker's lease to expire, or retrying, and replaying a step can repeat its side effects. The setup is two OpenTelemetry environment variables, with no framework to adopt.**

A durable workflow that sleeps for a week, waits on an event or calls a model has returned a wrong answer, or never finished. Your project holds thousands of runs, and you have a complaint but no run ID.

By the end you can find the bad run by status, by what a step returned or by time. You can tell a timer, a missing event, a dead worker and a retry apart, check whether a step ran twice, and keep exact payloads without keeping secrets.

> **Key Takeaways**
> - A quiet run isn't a healthy one, so search by what a step said to find it.
> - Build idempotency keys from the run ID and step name, then confirm in the trace that a retry repeated nothing.
> - Write redaction rules before the first run; a rule added later leaves earlier payloads untouched.
> - Let monitors latch and resolve only on a real reading, so an empty window never closes an incident.

## How do you set up durable workflow debugging without a framework?

You set up durable workflow debugging without a framework by pointing an OpenTelemetry exporter at mcpd and passing the trace context to each worker. There is no SDK to adopt and no cloud resource to create first.

![Illustration for How do you set up durable workflow debugging without a framework?](/blog/durable-workflow-debugging/image-1-0f5eadd00053.webp)

1. Get a project key. Start on the hosted version at [mcpd.sh/app](https://mcpd.sh/app), or build from source at `github.com/loruorg/mcpd`, then copy a key from your project.
2. Set the exporter variables on the process that starts runs and on every worker. Send the key as the [mcpd OpenTelemetry guide](https://mcpd.sh/docs/opentelemetry) describes.

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

3. Pass `traceparent` to the worker. The [W3C Trace Context specification](https://www.w3.org/TR/trace-context/) shows the header as four dash-separated fields (version, trace id, parent id, trace flags), such as `00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01`. If the worker starts a fresh root span instead of reading it, the step lands in a separate trace and the run looks incomplete.

4. Run one workflow and open it. You should see the steps on one timeline, with the worker's spans nested under the run.

A worker started with those two variables claims a step, reads the incoming `traceparent` and opens its span as a child of that parent id. The step's prompt and completion then sit under the same trace id as the run, so you don't join two tools by hand. Other [ways to instrument your code](https://mcpd.sh/docs/integrating) work the same way.

Look at the run for position, retry count and waits, and at the worker's span for what the model received and returned.

[Microsoft Learn's Durable Functions diagnostics page](https://learn.microsoft.com/en-us/azure/durable-task/durable-functions/durable-functions-diagnostics) covers how to "configure tracking and logging" before you "inspect distributed traces". With the OpenTelemetry route, the first trace needs only the two variables and a key. [Start free](https://mcpd.sh/app) and send one run.

## How does durable workflow debugging find the one bad run?

You find the one bad durable run by starting from the symptom a user reported and narrowing your project's runs until one timeline is left. [Conductor's debugging page](https://conductor-oss.github.io/conductor/devguide/how-tos/Workflows/debugging-workflows.html) begins its procedure with `conductor workflow get-execution <workflow-id> -c`, which needs a workflow ID you already hold. A trace-first path begins with the complaint, so it works when all you have is a sentence from a user.

1. Filter by status or time. In the interface, filter the runs list to failed or still-running runs, or set the time range around the complaint. When the complaint is a wrong answer, look past status too: a run that finished with status success can still be the bad one.

2. Search by what a step said. [mcpd's home page](https://mcpd.sh) says "Search finds a run by what the model said," which means full-text over the payload. Type words you expect in the input or output, such as `order 4471` or the error text a tool returned.  The [searching docs](https://mcpd.sh/docs/searching) list the filters and syntax you can combine.

3. Read the step tree on one timeline. Open the run, where workflow steps, model calls and waits sit on one clock in the same trace, and find the first step that failed, ran long or shows more than one attempt. A long gap before a step is usually a timer or an event wait. The [runs docs](https://mcpd.sh/docs/runs) cover how a durable run records its steps and failures.

4. Open the failing step's input and output. Compare what the step received with what it returned. If the input is already wrong, move up the tree to the step that produced it, because the failing step is often only where the damage shows. You're done when you can say which exact input produced which wrong output.

5. Keep the run as a test. Add the run to a dataset as a case, or attach a deterministic score that fails on the bad output. After you fix the step, run the new version against that case, as the [regression testing docs](https://mcpd.sh/docs/testing) describe, so the same bug can't return unnoticed.

The whole durable workflow debugging path, from complaint to the step's payload, takes a few minutes when payloads are stored. Starting from a known ID is faster when you have one, but a complaint rarely arrives with an ID attached.

## Durable workflow debugging: is the run stuck, waiting or retrying?

A durable run that looks stuck is waiting by design, waiting on a dead worker's lease, or retrying, and the step in the trace shows which.

| State | What it means | What to check | What to do |
| --- | --- | --- | --- |
| Waiting | Parked on `sleep`, `sleep_until` or `wait_event` | Wake time on a timer; whether the event was ever sent | Nothing for a timer; send the missing event or cancel the run |
| Worker died | A worker claimed the step, then stopped | Lease on the step, and which worker held it | Let another worker reclaim it; check one is running |
| Retrying | The step failed and runs again | Retry count, read twice; the error on each attempt | Fix the cause; check for repeated side effects |

In durable workflow debugging, the waiting state is the one people misread. A run parked on `sleep`, `sleep_until` or `wait_event` is healthy, because the daemon holds the run while no worker is busy.

A `wait_event` step has no lease to expire, so a week of silence looks the same as a dead run unless you check whether the event arrived. The [workflows docs](https://mcpd.sh/docs/workflows) define these constructs, and the [runs docs](https://mcpd.sh/docs/runs) cover waits, timers and cancellation.

A dead worker shows up on the step itself. A worker that dies mid-step leaves its claim behind, and once the lease expires another worker picks the step up. Until then the run looks stuck without being broken.

A retrying step shows a retry count that rises each time you open it. Read it twice, a minute apart. A count that moves means the run is working through failures. A count that stays put on an expired lease points back to a dead worker.

A status field can't make this split. [Conductor's debugging page](https://conductor-oss.github.io/conductor/devguide/how-tos/Workflows/debugging-workflows.html) tells you to identify the failed or timed-out task and record its `reasonForIncompletion`, input, output, worker ID and retry count, so it starts from a status and a task that already failed.

[Microsoft's troubleshooting guide](https://learn.microsoft.com/en-us/azure/durable-task/durable-functions/durable-functions-troubleshooting-guide) is organised as a list of symptoms: it says "Find your symptom in the following list and follow the linked steps to diagnose and resolve the issue." Both are fair places to begin. A trace adds the state of the step: parked, leased or retrying.

Two runs that both look idle after an hour can need opposite actions. One is parked on `wait_event` for an approval, and its step tree shows no lease and no retries. The other has a step whose lease has expired because its worker was restarted.

You leave the first alone and send the approval if it never came. You watch the second until another worker claims it.

Status-field inspection suits a run that has already failed or timed out, where the engine's message names the cause. A step-level trace suits a quiet run, where you need to know whether it is waiting, orphaned or retrying.

## Why does a retried step repeat work in a durable workflow?

A retried or replayed step repeats work because the engine re-runs it after a failure or an expired lease without knowing whether the first attempt reached the outside world. A refund, an email or a model call may then happen twice, and the trace shows whether the step ran twice.

A worker calls a payment API, the refund succeeds, and the worker crashes before it reports back. Its lease runs out, another worker claims the step, and the refund is issued a second time. The engine saw only silence.

A retry after a timeout and a replay after a restart share that blind spot. [Microsoft's Durable Functions diagnostics page](https://learn.microsoft.com/en-us/azure/durable-task/durable-functions/durable-functions-diagnostics) lists how to "write replay-safe code" next to logging and tracing, so treat replay as expected behaviour.

To confirm a double run, open the suspect step in the trace and compare its attempts:

- Attempts: look for more than one attempt on the same step, or a retry count above 0.
- Timings: a second attempt that starts right after a failure suggests a retry. One that starts after a long gap suggests an expired lease.
- Input and output: identical inputs on both attempts, with the first ending in an error or no output, mean the outside system may already have acted.

The design rule follows: make every side effect idempotent, then read the trace to check. Send the payment API a key built from the run ID and the step name, so a second call returns the first result instead of a second refund.

Retry settings live in the workflow definition, described in the [mcpd workflows guide](https://mcpd.sh/docs/workflows).

A key can't undo a repeated model call. It shows up as two sets of tokens and two costs in the same trace.

## How do you debug an agent step inside a durable run?

You debug an agent step inside a durable run by opening the run's trace, selecting the agent step, and reading its model span next to the workflow cursor. Allow about five minutes for a run that waited a week for an event and then returned a wrong answer.

1. Find the agent step in the step tree. The cursor shows where the run woke after the wait. If the model span floats outside the run, the worker didn't pass the trace context.

2. Read the model span by its GenAI names. The [OpenTelemetry GenAI conventions](https://opentelemetry.io/docs/specs/semconv/gen-ai/gen-ai-spans/) name the span `{gen_ai.operation.name} {gen_ai.request.model}` and carry `gen_ai.usage.input_tokens` and `gen_ai.usage.output_tokens`.

3. Open the exact prompt and completion. They arrive as `gen_ai.input.messages` and `gen_ai.output.messages`. Compare the prompt with the event payload that ended the wait. A week-old value baked into the prompt is a common cause of a wrong answer.

4. Check the cost. mcpd prices each step once at ingest from a [price book](https://mcpd.sh/docs/cost). If the cost field is empty, the model is missing from the book. An unknown model has no cost, not a cost of zero, so don't read a blank as free.

Success looks like one step where you can see the prompt, the completion, both token counts and the cost, on the same timeline as the cursor. For the full run model, read the [guide to durable runs](https://mcpd.sh/docs/runs).

## Can you keep step payloads without storing secrets?

Yes. With redaction in place, durable workflow debugging keeps the exact input and output of a step with the secret parts already removed or disguised.

A byte count tells you a step returned 2,048 bytes. It can't tell you the step returned the wrong customer's address, and you can't reconstruct a decision from a number.

1. List the fields that carry secrets or personal data. Look at tool results (access tokens, card numbers), request headers and prompts. Users paste secrets into prompts, and that field is the one most often forgotten.

2. Pick one of the four modes for each field.

```text
drop     the value is removed
mask     the value is replaced by a fixed placeholder
partial  a small part of the value is kept
hash     the value is replaced by a keyed hash
```

 Use drop for tokens you never need to read. Use hash for an email address: equal values give equal hashes, so you can find every run for one customer without storing the address. [mcpd's privacy docs](https://mcpd.sh/docs/privacy) give the configuration keys for each mode.

3. Set the rules before the first run. mcpd's docs put it this way: "Redaction runs before the write, because a secret deleted afterwards was still written down." A rule added later doesn't clean what is already stored. Spans, logs and metrics are covered by the same rules.

4. Set the retention period. Payloads older than that stop being stored. For an erasure request about one person, hashed fields help you find the runs that mention them.

5. Keep credentials in the workers. Workers hold the keys for the tools and models they call. 

6. Check the result. Open a finished run, select a step and read its input and output. Success looks like a readable payload where the sensitive fields show a placeholder, a partial value or a hash. If a raw secret is still visible, add a rule for that field and run the step again.

The whole check takes about five minutes on a run you already have.

## How do you catch a durable run going wrong without watching it?

A monitor catches a durable run going wrong by checking a metric on a schedule and calling your webhook when the reading crosses a threshold. [mcpd's alerting docs](https://mcpd.sh/docs/alerting) describe monitors on error_rate, cost and step-count divergence, evaluated every 60 s.

Each metric matches a failure that durable workflow debugging would otherwise find late. Step-count divergence flags a looping run: a loop that never meets its exit condition keeps adding steps while comparable runs finish in their usual number.

Cost flags a run whose agent steps retry, because every retry is another model call with its own tokens and price.

A monitor latches. It fires once when the reading crosses the threshold, calls the webhook, and stays fired instead of paging you again every 60 s. It resolves only on a real reading that is back under the threshold.

That last rule exists because a run that stops producing data looks quiet. If a worker dies or a run sits waiting, no new spans arrive, and an empty window can pass for a healthy one. The docs say it in four words: "Silence is not recovery." A monitor that resolved on silence would close the incident when you most need it open.

The webhook receives the alert, so it can open a ticket or post to a channel. From there you search for the run by what a step said and read its step tree.

## Frequently asked questions

### How do I debug a stuck durable run?

Open its trace and read the last step's state. A timer or wait_event step means the run is waiting by design, a step with an expired lease means a worker died, and a rising retry count means the step is retrying.

### How do I find a workflow run without its ID?

Filter by status or time window, or run a full-text search over step payloads for a phrase the user saw or a value a step returned. Open the matching run and read its step tree on one timeline.

### How can I tell whether a step ran twice?

Look for two attempts of the same step in the trace. A retry after a failure, or a pickup after a lease expired, leaves a second attempt with its own timing, input and output. If the step sent an email or a refund, check that system too.

### Does durable workflow debugging need a specific framework?

No, it needs OpenTelemetry traces. Point an exporter at mcpd and pass the trace context to each worker, and your code keeps its language and structure.

## What to do next with your durable runs

Work through three durable workflow debugging steps, each larger than the last.

- In five minutes: set `OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh` and `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`, then read the first trace.
- This week: add redaction, search for a recent bad run by what a step said, and keep that run as a dataset case.
- This month: add monitors on `error_rate`, cost and step-count divergence, so a wrong run gets flagged without anyone watching.

[Start free](https://mcpd.sh/app) on the hosted version, or build from source at `github.com/loruorg/mcpd`. The exporter is plain OpenTelemetry, so undoing it means removing two environment variables.

## Sources

[Trace Context, W3C Recommendation](https://www.w3.org/TR/trace-context/) · [Diagnose and troubleshoot issues in Durable Functions](https://learn.microsoft.com/en-us/azure/durable-task/durable-functions/durable-functions-diagnostics) · [Debugging Workflows - Durable execution for workflows and…](https://conductor-oss.github.io/conductor/devguide/how-tos/Workflows/debugging-workflows.html) · [mcpd · The unopinionated agent observatory.](https://mcpd.sh) · [Durable Functions Troubleshooting Guide | Microsoft Learn](https://learn.microsoft.com/en-us/azure/durable-task/durable-functions/durable-functions-troubleshooting-guide) · [Cost · mcpd](https://mcpd.sh/docs/cost) · [Privacy · mcpd](https://mcpd.sh/docs/privacy) · [Alerting · mcpd](https://mcpd.sh/docs/alerting)
