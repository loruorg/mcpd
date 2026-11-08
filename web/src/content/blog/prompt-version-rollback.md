---
description: "How do you undo a bad prompt edit? A prompt version rollback moves your agent's label to an earlier version with no deploy. Learn how to confirm it worked."
faq:
  - answer: Move the label your agent fetches to the earlier version. Because the agent resolves the label when it runs, the next run uses the older text. Then watch score, error rate and cost on runs linked to that version, and treat silence as unconfirmed.
    question: How do I roll back a prompt without redeploying?
  - answer: A version is one saved text of a prompt. A label is the name your agent fetches, and you point it at a version. The mcpd prompts docs say each step is linked to the prompt version that wrote it.
    question: What is the difference between a prompt version and a label?
  - answer: "Group runs by prompt version, then by model name, and see which grouping splits the scores. If only the model name separates good runs from bad ones, the prompt isn't the cause. Pin a snapshot name where your provider supports one, so the model stays fixed."
    question: How do I know if a quality drop is the prompt or the model?
  - answer: "Yes, you can try prompt rollback with mcpd without installing a package, in three ways. First, set OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh on an OpenTelemetry exporter that you already run. Second, use the hosted version of mcpd at https://mcpd.sh/app. Third, build it from source at github.com/loruorg/mcpd."
    question: Can I try prompt rollback without installing a package?
  - answer: Check that new runs show the older prompt version, then check that score_mean, error_rate and cost recover on a real reading. A stretch with no new runs gives no reading, so wait for traffic before you call the rollback fixed.
    question: How do I know the rollback worked?
image: /blog/prompt-version-rollback/featured-131ebdc42dc6.webp
image_alt: "Illustration for Prompt version rollback: Revert with no redeploy"
publishDate: "2026-11-08"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/prompt-version-rollback#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q128751\",\"@type\":\"Thing\",\"description\":\"collection of computer instructions written using some human-readable computer language\",\"name\":\"Source code\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Source_code\",\"https://www.wikidata.org/wiki/Q128751\",\"https://www.google.com/search?kgmid=/m/06wpv\"]}],\"datePublished\":\"2026-11-08\",\"description\":\"How do you undo a bad prompt edit? A prompt version rollback moves your agent's label to an earlier version with no deploy. Learn how to confirm it worked.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Move the label your agent fetches to the earlier version. Because the agent resolves the label when it runs, the next run uses the older text. Then watch score, error rate and cost on runs linked to that version, and treat silence as unconfirmed.\"},\"name\":\"How do I roll back a prompt without redeploying?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"A version is one saved text of a prompt. A label is the name your agent fetches, and you point it at a version. The mcpd prompts docs say each step is linked to the prompt version that wrote it.\"},\"name\":\"What is the difference between a prompt version and a label?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Group runs by prompt version, then by model name, and see which grouping splits the scores. If only the model name separates good runs from bad ones, the prompt isn't the cause. Pin a snapshot name where your provider supports one, so the model stays fixed.\"},\"name\":\"How do I know if a quality drop is the prompt or the model?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Yes, you can try prompt rollback with mcpd without installing a package, in three ways. First, set OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh on an OpenTelemetry exporter that you already run. Second, use the hosted version of mcpd at https://mcpd.sh/app. Third, build it from source at github.com/loruorg/mcpd.\"},\"name\":\"Can I try prompt rollback without installing a package?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Check that new runs show the older prompt version, then check that score_mean, error_rate and cost recover on a real reading. A stretch with no new runs gives no reading, so wait for traffic before you call the rollback fixed.\"},\"name\":\"How do I know the rollback worked?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q68712963\",\"@type\":\"Thing\",\"description\":\"request to merge a code branch into another; feature available at source code repository hosts such as GitHub or Gitlab\",\"name\":\"pull request\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q68712963\",\"https://www.google.com/search?kgmid=/g/11fmtd8q5x\"]}],\"name\":\"Prompt version rollback: Revert with no redeploy\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/-jtCskdEbais9BANKgD-Ur2N906WCU6v.webp\"},\"url\":\"https://mcpd.sh/blog/prompt-version-rollback\"},{\"@id\":\"https://mcpd.sh/blog/prompt-version-rollback#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q128751\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"Prompts · mcpd\",\"url\":\"https://mcpd.sh/docs/prompts\"},{\"@type\":\"CreativeWork\",\"name\":\"Prompts Are Artifacts, Not Dashboard Config\",\"url\":\"https://usqrd.com/insights/versioning-testing-rolling-back-prompts-production\"},{\"@type\":\"CreativeWork\",\"name\":\"Alerting · mcpd\",\"url\":\"https://mcpd.sh/docs/alerting\"},{\"@type\":\"CreativeWork\",\"name\":\"Testing · mcpd\",\"url\":\"https://mcpd.sh/docs/testing\"}],\"dateModified\":\"2026-11-08\",\"datePublished\":\"2026-11-08\",\"description\":\"How do you undo a bad prompt edit? A prompt version rollback moves your agent's label to an earlier version with no deploy. Learn how to confirm it worked.\",\"headline\":\"Prompt version rollback: Revert with no redeploy\",\"image\":\"https://api.birchseek.com/m/-jtCskdEbais9BANKgD-Ur2N906WCU6v.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/prompt-version-rollback#webpage\"},\"keywords\":[\"prompt version rollback\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/prompt-version-rollback#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q68712963\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2948},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]},{\"@type\":\"Thing\",\"description\":\"Chinese artificial intelligence company\",\"name\":\"DeepSeek\",\"sameAs\":[\"https://en.wikipedia.org/wiki/DeepSeek\",\"https://www.wikidata.org/wiki/Q131577453\",\"https://www.google.com/search?kgmid=/g/11wvrb0s91\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"defined sequence of activities of an organizational entity, enabled by the systematic organization of resources into processes that transform materials, provide services, or process information\",\"name\":\"Workflow\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Workflow\",\"https://www.wikidata.org/wiki/Q627335\",\"https://www.google.com/search?kgmid=/m/0k5rb\"]},{\"@type\":\"Thing\",\"description\":\"maximum number of tokens that a large language model can process at one time\",\"name\":\"Context window\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Context_window\",\"https://www.wikidata.org/wiki/Q125619872\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"chatbot developed by SpaceXAI\",\"name\":\"Grok\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Grok_(chatbot)\",\"https://www.wikidata.org/wiki/Q123361035\",\"https://www.google.com/search?kgmid=/g/11l6lp1dv3\"]},{\"@type\":\"Thing\",\"description\":\"collection of data, publication type\",\"name\":\"Data set\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Data_set\",\"https://www.wikidata.org/wiki/Q1172284\",\"https://www.google.com/search?kgmid=/m/02cck\"]},{\"@type\":\"Thing\",\"description\":\"software for running large language models (LLMs) on a local computer or in the cloud\",\"name\":\"Ollama\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Ollama\",\"https://www.wikidata.org/wiki/Q124636097\"]},{\"@type\":\"Thing\",\"description\":\"Chinese multinational technology company\",\"name\":\"Alibaba Group\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Alibaba_Group\",\"https://www.wikidata.org/wiki/Q1359568\",\"https://www.google.com/search?kgmid=/m/07s6ctl\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - prompt versioning
  - rollback
  - llm observability
  - ai agents
  - monitoring
  - opentelemetry
title: "Prompt version rollback: Revert with no redeploy"
---

**A prompt version rollback moves the label your agent fetches back to an earlier prompt version, so the next call renders the older text with no deploy. Find the bad version from the runs it produced, then confirm recovery with scores, error rate and cost on runs linked to the restored version. Silence isn't recovery: wait for a real reading.**

You edit a prompt and ship it. Nothing errors. The model still returns a 200 and valid text. There's no stack trace to read, and answers get a little worse for days before anyone says so.

If you run the agent alone, nobody else is reading its outputs.

A prompt version rollback acts on what your runtime fetches, so your code stays as it is. By the end you can detect the regression, revert it without a redeploy and prove the revert worked.

> **Key Takeaways**
> - A repo prompt rolls back with a deploy; a fetched prompt rolls back with a label move.
> - A prompt regression, a model change and input drift look alike; only the version recorded on each run separates them.
> - A latching monitor fires once and resolves only on a real reading, so it gives you a trigger and a confirmation.
> - Compare both versions on a dataset with an experiment before moving a label forward again.

## What is prompt version rollback and why does it need no redeploy?

Prompt version rollback points the label your agent fetches at an earlier prompt version, and it needs no redeploy because the agent resolves that label at runtime. The code never changes; only the version behind the label does. Runs linked to each version show whether the rollback took effect.

![Illustration for What is prompt version rollback and why does it need no redeploy?](/blog/prompt-version-rollback/image-1-706079eee4f8.webp)

Two terms carry the mechanism. A **version** is one saved state of a prompt's text. A **label**, such as `production`, is a movable name that points at one version.

A label works like a git branch name. The name moves, and the commits it used to point at stay where they were.

The [mcpd prompts docs](https://mcpd.sh/docs/prompts) cover the exact calls. At runtime the sequence has four steps:

1. Your agent asks the daemon for a prompt by name and label, for example `refund-reply` and `production`.
2. The daemon returns the version the label currently points to.
3. The agent fills in the variables, such as the customer's name and the order, and sends the rendered text to the model.
4. The step that records the model call is linked to the version that wrote it.

Nothing else in the sequence changes.

### A walk-through with assumed names

Assume `refund-reply` has seven versions and `production` points at version 7. Version 7 fixed an edge case, but replies have stopped asking for the order number, so you move `production` back to version 6.

Your code still says `refund-reply` and `production`. The next fetch returns the text of version 6, the agent renders it and the model call goes out with the older wording. You built no image, shipped no release and restarted no process.

The runs recorded before the move link to version 7. The runs recorded after it link to version 6. Filter runs by version to read the prompt and completion on each side of the move, and check whether the request for the order number is back.

### Why no code ships

Your code holds a name and a label, never the prompt text. A prompt version rollback edits what the label points at and leaves the code that asks for it untouched. There's nothing to build or deploy.

A prompt kept in the repo is part of the build. Changing it means a commit, a build and a deploy.

## Prompts in the repo or fetched at runtime: which rolls back better?

Runtime-fetched prompts roll back faster, because a label move takes effect on the next fetch, while repo-held prompts keep their history in git and roll back through a deploy. The choice trades rollback speed against where the history lives.

| Criterion | Repo-held | Runtime-fetched | Hybrid |
| --- | --- | --- | --- |
| Where the text lives | Git | Prompt store | Git |
| How you roll back | Revert commit, deploy | Move a label | Revert commit, deploy |
| Who can do it | Anyone who can deploy | Anyone with prompt write access | Anyone who can deploy |
| What the history shows | Commits and pull requests | Versions and label moves | Commits, plus versions on calls |
| What proves which text served a run | Deployed commit | Version recorded on the run | Version recorded on the run |

Repo-held prompts get the review habits you already have. A pull request shows the diff, git keeps the history, and the rollback travels the same deploy path as code.

That deploy path is also the cost. A bad prompt stays live until a revert has gone through your pipeline.

Runtime-fetched prompts change on the next fetch with no code change. The text served at any moment lives outside git, so if the run doesn't record which version wrote it, you can't prove what the model was given.

mcpd records that link at the run level. Its [prompts feature](https://mcpd.sh/docs/prompts) links each step to the prompt version that wrote it, so history is queryable per run. After you move a label back, the next runs show the older version, and that link proves the move took effect.

A hybrid keeps the text in the repo, keeps pull request review, and still records the prompt version on each call. You give up the label move and keep an exact answer to "which text served this run".

Each approach suits a different team:

- Repo-held suits teams where every prompt change should pass the same review and deploy gate as code.
- Runtime-fetched suits a solo operator or small team that wants to revert without a deploy and records the version on every run.
- Hybrid suits teams that want git as the source of truth and per-run proof of what was served.

## How do you decide a prompt version rollback is needed?

You decide a prompt version rollback is needed when specific runs, not an average, show that an edit changed what the model did. [usqrd's guide to rolling back prompts](https://usqrd.com/insights/versioning-testing-rolling-back-prompts-production) starts from the same observation: "The worst production incidents we get called into rarely start with an exception." Follow the symptom to the runs in five steps.

1. Name the symptom and the edit time. Write down what changed (thinner answers, higher latency, rising cost) and when you moved the label. Anything before that time is your baseline.

2. Search for what the model said. Full-text search finds runs by payload, so a phrase the model should have used, or an apology it now uses, finds the runs that labels miss. Filter to runs after the edit.

3. Open the waterfall of two runs. The waterfall shows each [run](https://mcpd.sh/docs/runs) on one clock. Open one run from before the edit and one from after, and look for a missing tool call, a skipped step or a shifted duration.

4. Read the prompt and the completion. Group the runs by version and read the exact text each received and returned. Judge from several runs per version, because a dashboard average can hide a drop confined to one input type.

5. Set a monitor as the trigger. Create a monitor on `score_mean`, `error_rate` or cost. Per the [alerting docs](https://mcpd.sh/docs/alerting), monitors are evaluated every 60 s, latch, fire once, resolve only on a real reading, and call your webhook.

A search hit at step 2 shows that the symptom exists; it does not show that the edit caused it. A model change or odd inputs can produce the same text, which is why step 4 compares the version recorded on each run.

A refund agent shows the steps end to end. Version 4 of its prompt tells it to stop refunding orders outside the return window, and adds a line telling it to answer from the conversation when it can. A search for "I can't verify your order" after the edit returns the affected runs.

The waterfall of one of those runs shows no `lookup_order` tool span. A run on version 3 has that span before the model replies.

The version 4 prompt and its completion show the model answering from the customer's message alone. Version 3 calls the tool in the same case. That evidence is enough to move the label back to version 3.

A monitor on `score_mean` would have fired after the first window of weak scores, and your webhook would have sent you to these runs.

## How do you tie each run to the prompt version that produced it?

You tie each run to its prompt version by having every model call carry the version that produced it, which splits affected runs from unaffected ones. mcpd records that link on each step, so a quality drop becomes a question you answer with a filter.

1. Send traces without installing a package. Point any OpenTelemetry exporter at mcpd with a key from your project, as the [OpenTelemetry page](https://mcpd.sh/docs/opentelemetry) describes. Other routes are in the [integrating guide](https://mcpd.sh/docs/integrating).

   ```shell
   export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
   export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
   ```

2. Record the model on every call. The prompt version link is mcpd's own feature, separate from the model name you record. Without the model on each run, you cannot tell a model change from an edit.

3. List the affected runs. Start from the version you suspect and open the runs linked to it, then compare them with runs from the version before it. Match the periods and the kind of request: a week of one version against an hour of another proves nothing.

4. Group by version, then by model. If the drop follows the version, the prompt caused it. If it follows the model name, the prompt didn't.

5. Check the model name for drift. An alias in your code can reach a different model next month with no edit on your side, and `gen_ai.response.model` is where that shows up.

6. Check the input mix. If the version and model are constant but the failing runs share a new kind of request, the cause is input drift. Moving the label back wouldn't change anything.

The split you want lines up with one variable: the same model and input mix across both versions, with scores falling only on the newer one. To try this on your own agent, [start free on the hosted version](https://mcpd.sh/app) and set the two variables above.

## How do you verify a prompt rollback worked?

You verify a prompt version rollback by confirming that new runs link to the restored version, then watching `score_mean`, `error_rate` and cost on those runs until a monitor resolves on a real reading.

1. Open a run that started after the label moved. Find the step that wrote the prompt: its linked version should be the older one. If it still shows the newer version, the rollback hasn't taken effect. Don't read scores yet, because you'd be measuring the wrong version.

2. Filter to runs linked to the restored version and read three signals: `score_mean`, `error_rate` and cost. Keep human feedback beside the scores and never merge the two into one number, since mcpd keeps them as separate signals.

3. Read the monitor, not the quiet. A monitor latches and resolves only on a real reading, so one that goes quiet because no runs arrived hasn't recovered.

4. Compare before you move the label forward. Build a dataset from production runs, then use an experiment to compare the newer version against the restored one. The [mcpd testing docs](https://mcpd.sh/docs/testing) cover datasets and experiments that compare two versions. Move the label forward only once the newer version holds up on that comparison.

### What the monitor sequence looks like

With a monitor on `score_mean` for a `json_valid` scorer, a bad edit pushes the score down and the monitor fires once. It stays latched while you move the label back.

New runs arrive linked to the older version and their scores come in. The monitor resolves when a real reading comes back clear.

If traffic stops after the label move, no reading arrives, so the monitor stays latched. The same hold applies to a monitor on `error_rate`.

A frequent error at step 2 is to judge recovery from the whole project's `score_mean`. Runs from the bad version still sit inside that average, so filter to runs linked to the restored version first.

## Which edge cases complicate a prompt rollback?

A prompt version rollback takes more than a label move in four situations: the bad version is several edits back, several prompts changed together, the model changed instead of the prompt, or the bad version has already written runs. Each has a decision rule.

1. The bad version is several edits back. Stepping back one version restores an edit that may already be broken. Group runs by prompt version, find the first version where `score_mean`, `error_rate` or cost moved, and move the label to the last version before that one.

 Decision rule: roll back to the last good version, never to "the previous one".

2. Several prompts changed at once. If an agent fetches three prompts and all three were edited, one rollback of all three hides which edit caused the drop. Move one label at a time and watch the runs linked to each version.

 The prompt version recorded on each step tells you which prompt carried the regression. Decision rule: restore the prompt whose linked runs recover, and leave the others alone until their own runs say otherwise.

3. The model changed, not the prompt. A prompt version rollback can't fix a different model. Compare the model name recorded on runs before and after the drop, and check whether an alias now points at a newer snapshot.

 Where your provider offers pinned model versions, pin one in production, so the model only changes when you change it. Input drift gets the same check: compare the inputs of runs that share one prompt version. If those inputs shifted, the prompt text wasn't the cause.

 Decision rule: if runs on one unchanged prompt version split by model name or by input type, leave the label where it is.

4. The bad version already produced runs. Those runs stay in the record, linked to the version that wrote them. A prompt version rollback changes what future calls render and leaves that history untouched.

 Use those runs as evidence for the post-mortem. You can [build a dataset from failed runs](https://mcpd.sh/docs/testing) and compare two prompt versions on it in an experiment before you move a label forward again. Stored payloads fall under your redaction and retention rules, so check those before you copy prompts and completions into a dataset.

 Decision rule: keep the failed runs, and add the ones that show the failure to a dataset.

Across all four cases, use a prompt version rollback when the signal follows the prompt version, and leave the label alone when it follows the model or the inputs.

## Frequently asked questions

### How do I roll back a prompt without redeploying?

Move the label your agent fetches to the earlier version. Because the agent resolves the label when it runs, the next run uses the older text. Then watch score, error rate and cost on runs linked to that version until a real reading arrives.

### What is the difference between a prompt version and a label?

A version is one saved text of a prompt. A label is the name your agent fetches, and you point it at a version. Moving the label changes which text the next fetch returns and leaves every version as it was.

### How do I know if a quality drop is the prompt or the model?

Group runs by prompt version, then by model name, and see which grouping splits the scores. If only the model name separates good runs from bad ones, the prompt isn't the cause. Pin a snapshot name where your provider offers one, so the model stays fixed.

### Can I try prompt rollback without installing a package?

Yes. Point any OpenTelemetry exporter at mcpd with a project key, and no mcpd package is needed. You can also use the hosted version at https://mcpd.sh/app, or build from source at github.com/loruorg/mcpd.

### How do I know the rollback worked?

New runs show the older prompt version, and `score_mean`, `error_rate` and cost recover on a real reading. No runs means no reading. Don't call it fixed until traffic returns.

## Your next steps for prompt version rollback

Prompt version rollback is quick only when the groundwork exists before the bad edit. Set it up in this order:

- Today: point your OpenTelemetry exporter at `https://api.mcpd.sh` with a project key, run your agent once, and read the exact prompt and completion on that run. The [quickstart for your first trace](https://mcpd.sh/docs/quickstart) covers the steps.
- This week: serve your prompt by name and label, so a rollback is a label move. Add a monitor on `score_mean`, `error_rate` or cost, so a regression has a trigger and a recovery has a confirmation.
- Next time you edit a prompt: compare the new version against the current one on a dataset with an experiment, and move the label only after the comparison holds up.

[Start free at mcpd.sh/app](https://mcpd.sh/app) and read the prompt and completion of your first run before you change anything.

## Sources

[Prompts · mcpd](https://mcpd.sh/docs/prompts) · [Prompts Are Artifacts, Not Dashboard Config](https://usqrd.com/insights/versioning-testing-rolling-back-prompts-production) · [Alerting · mcpd](https://mcpd.sh/docs/alerting) · [Testing · mcpd](https://mcpd.sh/docs/testing)
