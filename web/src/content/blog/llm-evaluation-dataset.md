---
description: What goes into an LLM evaluation dataset? Frozen cases from your own production runs, each with input, expected behaviour and metadata. Learn to build one.
faq:
  - answer: A benchmark such as GSM8K or MMLU measures a model on a fixed public task, as its authors defined it, while an evaluation dataset measures your application on your own cases taken from your real inputs. The dataset answers whether this version of your prompt, tools and model still behaves correctly for your users.
    question: What is the difference between an LLM evaluation dataset and a benchmark?
  - answer: "An LLM evaluation dataset can start with the failures you have already seen plus a sample of ordinary traffic, because no published figure fits every application. After that, add one case for each new failure that appears. Each added case means a bug you have fixed can't return unnoticed."
    question: How many rows does an LLM evaluation dataset need?
  - answer: "An LLM judge shouldn't replace human review until you have checked it against human labels, because judge models show known biases. The MT-bench and Chatbot Arena study names position bias, verbosity bias and a bias toward the judge's own outputs. Keep judge scores and human feedback as separate signals."
    question: Can an LLM judge replace human review?
  - answer: You can build a dataset without storing customer data, because redaction runs before storage, so redacted fields were never written. A case frozen from those runs into a dataset holds only the masked or dropped values, and nothing needs to be deleted afterwards.
    question: Can I build a dataset without storing customer data?
image: /blog/llm-evaluation-dataset/featured-638bd46fa433.webp
image_alt: "Illustration for LLM evaluation dataset: Build one from real runs"
publishDate: "2026-10-31"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/llm-evaluation-dataset#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q1172284\",\"@type\":\"Thing\",\"description\":\"collection of data, publication type\",\"name\":\"Data set\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Data_set\",\"https://www.wikidata.org/wiki/Q1172284\",\"https://www.google.com/search?kgmid=/m/02cck\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\",\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q123721276\",\"@type\":\"Thing\",\"description\":\"language understanding across multiple domains\",\"name\":\"MMLU\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MMLU\",\"https://www.wikidata.org/wiki/Q123721276\"]}],\"datePublished\":\"2026-10-31\",\"description\":\"What goes into an LLM evaluation dataset? Frozen cases from your own production runs, each with input, expected behaviour and metadata. Learn to build one.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"A benchmark such as GSM8K or MMLU measures a model on a fixed public task, as its authors defined it, while an evaluation dataset measures your application on your own cases taken from your real inputs. The dataset answers whether this version of your prompt, tools and model still behaves correctly for your users.\"},\"name\":\"What is the difference between an LLM evaluation dataset and a benchmark?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"An LLM evaluation dataset can start with the failures you have already seen plus a sample of ordinary traffic, because no published figure fits every application. After that, add one case for each new failure that appears. Each added case means a bug you have fixed can't return unnoticed.\"},\"name\":\"How many rows does an LLM evaluation dataset need?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"An LLM judge shouldn't replace human review until you have checked it against human labels, because judge models show known biases. The MT-bench and Chatbot Arena study names position bias, verbosity bias and a bias toward the judge's own outputs. Keep judge scores and human feedback as separate signals.\"},\"name\":\"Can an LLM judge replace human review?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You can build a dataset without storing customer data, because redaction runs before storage, so redacted fields were never written. A case frozen from those runs into a dataset holds only the masked or dropped values, and nothing needs to be deleted afterwards.\"},\"name\":\"Can I build a dataset without storing customer data?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q1156402\",\"@type\":\"Thing\",\"description\":\"type of thought and capacity of consciously making sense of things, applying logic, and adapting or justifying practices, institutions, and beliefs based on new or existing information\",\"name\":\"Reasoning\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Reasoning\",\"https://www.wikidata.org/wiki/Q1156402\",\"https://www.google.com/search?kgmid=/g/121v7773\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q364\",\"@type\":\"Organization\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q134468621\",\"@type\":\"Thing\",\"description\":\"use of large language models as automated evaluators\",\"name\":\"LLM-as-a-Judge\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LLM-as-a-Judge\",\"https://www.wikidata.org/wiki/Q134468621\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q121360456\",\"@type\":\"Thing\",\"description\":\"grade school math question-answering dataset\",\"name\":\"GSM8K\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q121360456\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q870780\",\"@type\":\"Thing\",\"description\":\"program or set of instructions that simulates conversation with humans\",\"name\":\"Chatbot\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Chatbot\",\"https://www.wikidata.org/wiki/Q870780\",\"https://www.google.com/search?kgmid=/m/01305y\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q79798\",\"@type\":\"Thing\",\"description\":\"use of software for language translation\",\"name\":\"Machine translation\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Machine_translation\",\"https://www.wikidata.org/wiki/Q79798\",\"https://www.google.com/search?kgmid=/m/050ls\"]}],\"name\":\"LLM evaluation dataset: Build one from real runs\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/z-4oZ2S_miIh1ihx7DByRvwYZAKlgT76.webp\"},\"url\":\"https://mcpd.sh/blog/llm-evaluation-dataset\"},{\"@id\":\"https://mcpd.sh/blog/llm-evaluation-dataset#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q1172284\"},{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\"},{\"@id\":\"https://www.wikidata.org/wiki/Q123721276\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"Measuring Massive Multitask Language Understanding\",\"url\":\"https://arxiv.org/abs/2009.03300\"},{\"@type\":\"CreativeWork\",\"name\":\"cais/mmlu · Datasets at Hugging Face\",\"url\":\"https://huggingface.co/datasets/cais/mmlu\"},{\"@type\":\"CreativeWork\",\"name\":\"Evaluating Large Language Models Trained on Code\",\"url\":\"https://arxiv.org/abs/2107.03374\"},{\"@type\":\"CreativeWork\",\"name\":\"Datasets | DeepEval - The LLM Evaluation Framework\",\"url\":\"https://deepeval.com/docs/evaluation-datasets\"},{\"@type\":\"CreativeWork\",\"name\":\"Training Verifiers to Solve Math Word Problems\",\"url\":\"https://arxiv.org/abs/2110.14168\"},{\"@type\":\"CreativeWork\",\"name\":\"openai/gsm8k · Datasets at Hugging Face\",\"url\":\"https://huggingface.co/datasets/openai/gsm8k\"},{\"@type\":\"CreativeWork\",\"name\":\"Privacy · mcpd\",\"url\":\"https://mcpd.sh/docs/privacy\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory.\",\"url\":\"https://mcpd.sh\"},{\"@type\":\"CreativeWork\",\"name\":\"Scores and feedback · mcpd\",\"url\":\"https://mcpd.sh/docs/evaluation\"},{\"@type\":\"CreativeWork\",\"name\":\"Judging LLM-as-a-Judge with MT-Bench and Chatbot Arena\",\"url\":\"https://arxiv.org/abs/2306.05685\"},{\"@type\":\"CreativeWork\",\"name\":\"Cost · mcpd\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"Prompts · mcpd\",\"url\":\"https://mcpd.sh/docs/prompts\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd security documentation\",\"url\":\"https://mcpd.sh/docs/security\"},{\"@type\":\"CreativeWork\",\"name\":\"GitHub - leobeeson/llm_benchmarks: A collection of benchmarks and datasets for evaluating LLM\",\"url\":\"https://github.com/leobeeson/llm_benchmarks\"}],\"dateModified\":\"2026-10-31\",\"datePublished\":\"2026-10-31\",\"description\":\"What goes into an LLM evaluation dataset? Frozen cases from your own production runs, each with input, expected behaviour and metadata. Learn to build one.\",\"headline\":\"LLM evaluation dataset: Build one from real runs\",\"image\":\"https://api.birchseek.com/m/z-4oZ2S_miIh1ihx7DByRvwYZAKlgT76.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/llm-evaluation-dataset#webpage\"},\"keywords\":[\"llm evaluation dataset\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/llm-evaluation-dataset#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q1156402\"},{\"@id\":\"https://www.wikidata.org/wiki/Q364\"},{\"@id\":\"https://www.wikidata.org/wiki/Q134468621\"},{\"@id\":\"https://www.wikidata.org/wiki/Q121360456\"},{\"@id\":\"https://www.wikidata.org/wiki/Q870780\"},{\"@id\":\"https://www.wikidata.org/wiki/Q79798\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2850},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"defined sequence of activities of an organizational entity, enabled by the systematic organization of resources into processes that transform materials, provide services, or process information\",\"name\":\"Workflow\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Workflow\",\"https://www.wikidata.org/wiki/Q627335\",\"https://www.google.com/search?kgmid=/m/0k5rb\"]},{\"@type\":\"Thing\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]},{\"@type\":\"Thing\",\"description\":\"collection of data, publication type\",\"name\":\"Data set\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Data_set\",\"https://www.wikidata.org/wiki/Q1172284\",\"https://www.google.com/search?kgmid=/m/02cck\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"benchmark in natural language understanding\",\"name\":\"General Language Understanding Evaluation benchmark\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q64711428\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"large language model company\",\"name\":\"LlamaIndex\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LlamaIndex\",\"https://www.wikidata.org/wiki/Q125446413\"]},{\"@type\":\"Thing\",\"description\":\"Chinese artificial intelligence company\",\"name\":\"DeepSeek\",\"sameAs\":[\"https://en.wikipedia.org/wiki/DeepSeek\",\"https://www.wikidata.org/wiki/Q131577453\",\"https://www.google.com/search?kgmid=/g/11wvrb0s91\"]},{\"@type\":\"Thing\",\"description\":\"language understanding across multiple domains\",\"name\":\"MMLU\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MMLU\",\"https://www.wikidata.org/wiki/Q123721276\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"family of multimodal artificial intelligence models developed by OpenAI\",\"name\":\"GPT-4o\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GPT-4o\",\"https://www.wikidata.org/wiki/Q125919502\",\"https://www.google.com/search?kgmid=/g/11vxhg1zhp\"]},{\"@type\":\"Thing\",\"description\":\"family of large language models developed by OpenAI\",\"name\":\"GPT-5\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GPT-5\",\"https://www.wikidata.org/wiki/Q135297698\"]},{\"@type\":\"Thing\",\"description\":\"a lightweight markup language with plain-text formatting syntax that is popularly used\",\"name\":\"Markdown\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Markdown\",\"https://www.wikidata.org/wiki/Q1193600\",\"https://www.google.com/search?kgmid=/m/0jt0gkj\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - llm evaluation
  - evaluation dataset
  - ai agents
  - llm-as-judge
  - regression testing
  - observability
title: "LLM evaluation dataset: Build one from real runs"
---

**An LLM evaluation dataset for a shipped application is a frozen set of recorded cases from your own production runs, distinct from a public benchmark. Each row holds an input, an expected behaviour or reference, and metadata. You use it for regression checks and for comparing versions, scoring with deterministic checks, a judge model and human review as three separate signals.**

You change a prompt or swap a model, and the agent starts answering differently. A public benchmark or a hand-written list of questions can't tell you whether the change helped, because neither contains your users' real traffic.

An LLM evaluation dataset made from your own runs closes that gap. You'll learn to build an LLM evaluation dataset from recorded runs, pick scoring methods, check agent behaviour, compare two versions and watch a score over time.

> **Key Takeaways**
> - A row built from a recorded run carries the exact prompt, completion, tool calls and cost, so the payload is the evidence.
> - Averaging a machine score with human feedback hides disagreement. Every score should declare the scale it was measured against.
> - Redact before storage, and a dataset built from production runs never held the secret.
> - For agents, assert the route as well as the answer: tool calls, loops, cost and latency.

## What is an LLM evaluation dataset, and how is it different from a benchmark?

An LLM evaluation dataset tests one application on cases taken from its own traffic, while a benchmark tests a model on a public task its authors defined. You use the dataset for two jobs: rerunning the same rows after a change as a regression check, and running two prompts or models on identical inputs to compare them.

![Illustration for What is an LLM evaluation dataset, and how is it different from a benchmark?](/blog/llm-evaluation-dataset/image-1-731aabd41222.webp)

Each row has three parts. The input is what the application received, and for an agent it can be a whole conversation. The expectation is a reference answer or a behaviour, such as "calls the refund tool once".

The metadata holds the source run id, the date, tags, and the model and prompt version. This is what lets you trace a case back to the run it came from.

Because a row comes from a recorded run, its fields map to span attributes. That's what lets an LLM evaluation dataset point each row back at the span it came from.

A public benchmark starts from the opposite end, with a task its authors chose. The [GSM8K paper](https://arxiv.org/abs/2110.14168) introduces "a dataset of 8.5K high quality linguistically diverse grade school math word problems" (hosted as [`openai/gsm8k`](https://huggingface.co/datasets/openai/gsm8k)).

The [MMLU paper](https://arxiv.org/abs/2009.03300) describes a test that "covers 57 tasks including elementary mathematics, US history, computer science, law, and more" (hosted as [`cais/mmlu`](https://huggingface.co/datasets/cais/mmlu)).

The [HumanEval paper](https://arxiv.org/abs/2107.03374) describes a new evaluation set "to measure functional correctness for synthesizing programs from docstrings".

None of those tasks covers your agent, and none of them is an LLM evaluation dataset built for it. GSM8K accuracy can't tell you whether your support agent still refuses out-of-policy refunds after a prompt edit. A case built from one of that agent's recorded runs can.

Libraries name the parts differently. The [deepeval documentation](https://deepeval.com/docs/evaluation-datasets) says: "an evaluation dataset, or just dataset, is a collection of goldens. A golden is a precursor to a test case." Before running evals, you convert goldens to test cases.

## How do you build an LLM evaluation dataset from production traffic?

You build an LLM evaluation dataset from production traffic by taking a complaint to the one run behind it, reading that run, redacting it and freezing it as a case. Any framework that exports OpenTelemetry spans can feed this loop, so you adopt no package and no vendor platform first.

1. Send runs to an endpoint. Point your exporter at the hosted version ([mcpd.sh/app](https://mcpd.sh/app)) or at a build from source at github.com/loruorg/mcpd, with a project key as described on the [OpenTelemetry docs page](https://mcpd.sh/docs/opentelemetry). A new run appears in the interface after your next agent call.

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

2. Pick runs by signal: errors, user complaints, low scores, and runs with surprising cost or latency. Add a sample of ordinary runs too, because a dataset made only of failures can't show whether a change broke the cases that already worked.
3. Search by what the model said. [mcpd's home page](https://mcpd.sh) says: "Search finds a run by what the model said, and every log line links back to the step that wrote it." Paste the odd phrase from the complaint and open the matching run.
4. Open the [run](https://mcpd.sh/docs/runs) and read the exact prompt, the exact completion and any tool calls. Write down what a correct answer would have been, or the case will have no expected behaviour.
5. Turn on redaction before you build the dataset. [mcpd's privacy docs](https://mcpd.sh/docs/privacy) state: "Redaction runs before the write, because a secret deleted afterwards was still written down."
6. Add the run to a dataset. The [evaluation docs](https://mcpd.sh/docs/evaluation) describe datasets built from production runs. Adding a run freezes its exact payload as a case, and you can open the dataset to check that the prompt and completion match what you read.

A refund agent that calls the same lookup tool again and again in one conversation shows the loop end to end. A user writes in to complain, you search for an odd phrase the model produced, and the run shows the repeated calls.

You redact the customer's details and add the run to a dataset. From then on, every prompt or model version you test against that case can be checked for repeated tool calls, and any version that still loops fails.

## Deterministic checks, LLM-as-judge or human review: which scoring method fits?

Deterministic checks fit any rule you can write down, a judge model fits open-ended quality, and human review fits calibrating the judge and settling disagreements. Each method answers a different question about the same run.

| | Deterministic checks | Judge model | Human review |
| --- | --- | --- | --- |
| Example checks | `exact_match`, `contains`, `json_valid`, `latency_budget`, `cost_budget`, `answered`, `no_errors` | A rubric for open-ended quality | A reviewer reads the run in the queue |
| Signal produced | Score | Score | Feedback |
| Question answered | Did the run obey a stated rule? | Is the answer good, by this rubric? | Would a person accept this? |
| Applies when | You can write the rule down | The answer is free text | Calibrating the judge, settling disagreements |
| Failure mode | Blind to anything outside the rule | Judge bias | Slow, limited by reviewer time |

A rule-based check returns the same result every time you run it on the same output, and it costs nothing in model calls. It can't judge tone or relevance, because neither fits in a rule.

A judge model covers what rules can't, such as whether a summary is faithful or a reply is helpful. Zheng et al., in the [MT-bench and Chatbot Arena paper](https://arxiv.org/abs/2306.05685), examined "position, verbosity, and self-enhancement biases, as well as limited reasoning ability".

The same paper reports that strong judges such as GPT-4 "can match both controlled and crowdsourced human preferences well, achieving over 80% agreement, the same level of agreement between humans."

That agreement was measured on the paper's tasks. A judge can stand in for review on your task only after you've checked its verdicts against human labels on your own LLM evaluation dataset.

Keep the two outputs apart. mcpd's docs put it this way: "A score is what a machine thought. Feedback is what a person thought."

If a judge averages 0.9 on a case and reviewers give it 0.4, a merged figure reads 0.65 and looks mediocre. The real finding is that the judge and your reviewers disagree, and that case is the one to open.

Every score also declares the scale it was measured against. A result of 0.7 on a 0 to 1 scale differs from 7 on a 10-point scale, so never compare scores without their scales.

Work in this order:

1. Put every rule you can state as a condition (format, containment, budgets, error rules) into a deterministic scorer.
2. Use a judge model for open-ended quality at the volume reviewers can't read, once it has been checked against human labels.
3. Use people to calibrate the judge and to rule on cases where the signals disagree.

## How do you evaluate an agent, not only its final answer?

An agent evaluation dataset checks the route an agent took as well as its answer, because an agent can reach the right answer by a wasteful or unsafe route. A case built from a recorded run holds the tool calls, retrieval spans, tokens and cost, so each property of the route becomes an expectation on data the case already holds.

The [runs documentation](https://mcpd.sh/docs/runs) shows how steps and spans appear in a run. These are the properties to assert:

1. Tool calls: which tool ran, how many times and with which arguments.
2. Retrieval: what was retrieved and whether the answer used it. A judge model or a reviewer reads the retrieval span's returned documents against the completion.
3. Loops: the [`no_tool_loop` scorer](https://mcpd.sh/docs/evaluation) checks a run for a tool called over and over. 
6. [Cost](https://mcpd.sh/docs/cost) is priced once at ingest, per step, from a price book. An unknown model has no cost, not a cost of zero, so a missing price never reads as a free run.
7. A judge model or human review covers quality, since no deterministic check can.

A correct answer reached after repeated tool calls passes an answer-only check and fails a budget. That gap is the reason the dataset asserts the route.

Suppose a support agent answers a refund question correctly but calls an order-lookup tool far more often than it should. `exact_match` on the answer passes. Frozen as a case with a cap on calls to the lookup tool, the same run fails.

The count comes from the recorded tool spans, so nothing depends on parsing the answer text. After a prompt change, the experiment shows which cases now break the call, token or cost budget.

## How do you curate an LLM evaluation dataset over time?

You curate an LLM evaluation dataset over time by treating it as a versioned record: add cases on purpose, label each one, freeze every version and retire cases when the product changes.

1. Take cases from failures, surprising runs and ordinary traffic. Failures and surprises cover the edges, and ordinary runs keep the set close to what users do. Without them, a dataset only ever measures the last incident.
2. Tag cases by failure type and read scores per tag. A set dominated by one failure type rewards fixing that failure and hides regressions elsewhere, because the overall score moves while the other tags slip.
3. Give every case an expected behaviour or a reference, since a case without one can be read but not scored. Settle disagreements about the expected answer through a review queue.
4. Record the human verdict as feedback next to the machine score. [mcpd's evaluation docs](https://mcpd.sh/docs/evaluation) say it "keeps them as two signals and never averages one into the other," so the cases where they disagree stay visible.
5. Freeze each version and add new cases as a new version. A frozen case never changes, so two experiments on the same dataset version compare like with like.
6. Link prompt versions through [mcpd prompts](https://mcpd.sh/docs/prompts), where each step links to the prompt version that wrote it. A score change then traces to a prompt change or a dataset change, never an unknown mix of both.
7. Refresh from production and re-check the judge. Add cases from new production failures, and retire cases for features that no longer exist, since they score behaviour nobody ships.
8. Re-check judge scores against human review on a sample. A judge that drifts from your reviewers still produces confident numbers.

## How do you keep secrets and personal data out of an evaluation dataset?

You keep secrets and personal data out of an LLM evaluation dataset by redacting every run before it is stored, so the dataset never holds them.

Production runs carry customer messages, email addresses and API keys pasted into prompts. If redaction runs after storage, the secret sits in storage until someone deletes it.

Four modes cover most fields: drop, mask, partial and keyed hash.

- `drop` removes the field.
- `mask` replaces the value with a placeholder.
- `partial` keeps a fragment, such as the last characters.
- `keyed hash` replaces the value with a token that repeats for the same input, so you can still group cases by customer.

Platform teams also need cases to leave. Retention is set in days, so old cases age out on their own. Erasure requests name a person, so their data can be removed on request.

With a default configuration nothing leaves the machine, which covers update checks, usage reporting and telemetry. The [mcpd security documentation](https://mcpd.sh/docs/security) covers the controls, and the source build at github.com/loruorg/mcpd runs on your own network.

## How do you go from one bad run to a monitored regression check?

You go from one bad run to a monitored regression check by freezing the run as a case, scoring it, comparing two versions in an experiment and putting a monitor on the score.

1. Send runs to mcpd. Use the hosted version at [mcpd.sh/app](https://mcpd.sh/app), or build from source at github.com/loruorg/mcpd with `make build`, run `./target/release/mcpd demo` and open http://localhost:4317. The [quickstart](https://mcpd.sh/docs/quickstart) walks through the first trace.
2. Find the bad run by searching for the words the model said, such as the wrong refund policy a customer quoted. Read the exact prompt, completion and tool calls before you keep it.
3. Add the run to a dataset. The run becomes a case with its recorded input and metadata. Write the expected behaviour in your own words, since a copied bad answer would pass forever.
4. Attach deterministic scorers such as `contains` or `json_valid` first, because rule-based checks don't need a model call. Add a judge model only for open-ended quality, and record human verdicts as feedback in the review queue.
5. Run the dataset against the old and the new prompt version and compare the scores side by side. Change one thing between versions, or you can't tell which change moved the score.
6. Set a monitor on the case's mean score, with the threshold taken from the passing version's result in the experiment. The monitor watches production runs, so a regression that the dataset missed still surfaces.

The loop is complete when the original complaint is a case that fails on the old version and passes on the new one, and a monitor watches the same score on live traffic.

## Frequently asked questions

### What is the difference between an LLM evaluation dataset and a benchmark?

A benchmark such as GSM8K or MMLU measures a model on a fixed public task, as its authors defined it (see the [llm_benchmarks collection](https://github.com/leobeeson/llm_benchmarks)). An evaluation dataset measures your application on cases from your real inputs, so it shows whether this version of your prompt, tools and model still behaves correctly for your users.

### How many rows does an LLM evaluation dataset need?

No published figure fits every application. Start with the failures you have seen plus a sample of ordinary traffic, then add one case for each new failure so a fixed bug can't return unnoticed.

### Can an LLM judge replace human review?

A judge model shows known biases, so check it against human labels before you rely on it. The [MT-bench and Chatbot Arena study](https://arxiv.org/abs/2306.05685) names position, verbosity and self-enhancement biases. Keep judge scores and human feedback as separate signals.

### Can I build a dataset without storing customer data?

Yes. In mcpd, [redaction runs before storage](https://mcpd.sh/docs/privacy), so redacted fields were never written. A case frozen from those runs holds only the masked or dropped values, and nothing needs deleting afterwards.

### Do I need to install anything to start?

You don't need to install anything. Point any OpenTelemetry exporter at https://api.mcpd.sh (hosted at mcpd.sh/app), or build from source at github.com/loruorg/mcpd.

## Next steps: build your first dataset this week

**Today:** set `OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh` in your agent's environment and get runs flowing. Any framework that emits `gen_ai.*` attributes works, as described in the [OpenTelemetry ingestion docs](https://mcpd.sh/docs/opentelemetry).

**This week:** freeze your first failing or surprising runs as cases, with redaction on, and attach deterministic scorers such as `contains` and `json_valid`. Add a case each time a new failure turns up.

**This month:**

- Add a judge model and check it against cases a person has reviewed.
- Compare two versions in an experiment.
- Set a monitor on the mean score so a regression shows up before a user reports it.

[Start free at mcpd.sh/app](https://mcpd.sh/app) and send your first runs. If your data can't leave the network, build from source at github.com/loruorg/mcpd and point the exporter at your own daemon.

## Sources

[Training Verifiers to Solve Math Word Problems](https://arxiv.org/abs/2110.14168) · [openai/gsm8k · Datasets at Hugging Face](https://huggingface.co/datasets/openai/gsm8k) · [Measuring Massive Multitask Language Understanding](https://arxiv.org/abs/2009.03300) · [cais/mmlu · Datasets at Hugging Face](https://huggingface.co/datasets/cais/mmlu) · [Datasets | DeepEval - The LLM Evaluation Framework](https://deepeval.com/docs/evaluation-datasets) · [Privacy · mcpd](https://mcpd.sh/docs/privacy) · [Judging LLM-as-a-Judge with MT-Bench and Chatbot Arena](https://arxiv.org/abs/2306.05685) · [Scores and feedback · mcpd](https://mcpd.sh/docs/evaluation). Every other source is linked where the article cites it.
