---
description: "Is an LLM trace GDPR personal data? Yes, if it holds a person's text. Learn to redact before storage, set retention, limit access and answer erasure requests."
faq:
  - answer: "GDPR applies when a trace contains information about an identifiable person, exactly as it does to any other stored data. Redact before the trace is written, set a retention period, restrict who can read payloads, keep an audit log, and be able to find and delete one person's runs on request."
    question: Does GDPR apply to LLM traces and logs?
  - answer: Masking a field usually leaves a pseudonymised trace, which is not anonymous. A keyed hash can be re-linked by whoever holds the key, and the surrounding text, such as order details or names in free text, can still point to one person. Keep treating the trace as personal data.
    question: Is masking a prompt enough to make a trace anonymous?
  - answer: "You can send traces to a vendor outside the EU once a lawful transfer mechanism is in place, and the vendor is your processor, so a processor contract comes first. Confirm the mechanism against the vendor's published terms. Self-hosting the trace store keeps payloads on infrastructure you already control."
    question: Can I send traces to a US observability vendor?
  - answer: Log the spans that let you debug and show how the agent behaved. Storing every full prompt adds risk, so redact personal fields, shorten retention and keep full payloads only where someone reviews them. Too few logs weaken accountability, and too many work against minimisation.
    question: Do I need to log prompts at all?
image: /blog/llm-trace-gdpr/featured-a433f4d5bc77.webp
image_alt: "Illustration for LLM trace GDPR: Redact, expire and delete data"
publishDate: "2026-11-13"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/llm-trace-gdpr#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\",\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q1172506\",\"@type\":\"Thing\",\"description\":\"European Union regulation on the processing of personal data\",\"name\":\"General Data Protection Regulation\",\"sameAs\":[\"https://en.wikipedia.org/wiki/General_Data_Protection_Regulation\",\"https://www.wikidata.org/wiki/Q1172506\",\"https://www.google.com/search?kgmid=/m/0pk_7xs\"]}],\"datePublished\":\"2026-11-13\",\"description\":\"Is an LLM trace GDPR personal data? Yes, if it holds a person's text. Learn to redact before storage, set retention, limit access and answer erasure requests.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"GDPR applies when a trace contains information about an identifiable person, exactly as it does to any other stored data. Redact before the trace is written, set a retention period, restrict who can read payloads, keep an audit log, and be able to find and delete one person's runs on request.\"},\"name\":\"Does GDPR apply to LLM traces and logs?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Masking a field usually leaves a pseudonymised trace, which is not anonymous. A keyed hash can be re-linked by whoever holds the key, and the surrounding text, such as order details or names in free text, can still point to one person. Keep treating the trace as personal data.\"},\"name\":\"Is masking a prompt enough to make a trace anonymous?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"You can send traces to a vendor outside the EU once a lawful transfer mechanism is in place, and the vendor is your processor, so a processor contract comes first. Confirm the mechanism against the vendor's published terms. Self-hosting the trace store keeps payloads on infrastructure you already control.\"},\"name\":\"Can I send traces to a US observability vendor?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Log the spans that let you debug and show how the agent behaved. Storing every full prompt adds risk, so redact personal fields, shorten retention and keep full payloads only where someone reviews them. Too few logs weaken accountability, and too many work against minimisation.\"},\"name\":\"Do I need to log prompts at all?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q626741\",\"@type\":\"Thing\",\"description\":\"organizational efforts to comply with relevant laws, policies, and regulations\",\"name\":\"Regulatory compliance\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Regulatory_compliance\",\"https://www.wikidata.org/wiki/Q626741\",\"https://www.google.com/search?kgmid=/m/053ldb\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q63981718\",\"@type\":\"Thing\",\"description\":\"legal term from the GDPR: any information relating to an identified or identifiable natural person\",\"name\":\"personal data\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q63981718\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q456632\",\"@type\":\"Thing\",\"description\":\"appropriate collection, use and dissemination of personal data in products and services, as well as their protection and related legal and political issues\",\"name\":\"Information privacy\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Information_privacy\",\"https://www.wikidata.org/wiki/Q456632\",\"https://www.google.com/search?kgmid=/g/122ntwzb\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\",\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q136461020\",\"@type\":\"Thing\",\"name\":\"Schrems II\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Schrems_II\",\"https://www.wikidata.org/wiki/Q136461020\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q26877724\",\"@type\":\"Thing\",\"description\":\"body of the European Union ensure that data privacy laws are consistently applied in the EU countries\",\"name\":\"European Data Protection Board\",\"sameAs\":[\"https://en.wikipedia.org/wiki/European_Data_Protection_Board\",\"https://www.wikidata.org/wiki/Q26877724\",\"https://www.google.com/search?kgmid=/g/11cp8qfz18\"]}],\"name\":\"LLM trace GDPR: Redact, expire and delete data\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/GJkA5Z5DeZpiuPkVUD3NlPUg7tA5_Xsj.webp\"},\"url\":\"https://mcpd.sh/blog/llm-trace-gdpr\"},{\"@id\":\"https://mcpd.sh/blog/llm-trace-gdpr#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q115305900\"},{\"@id\":\"https://www.wikidata.org/wiki/Q1172506\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"Privacy · mcpd\",\"url\":\"https://mcpd.sh/docs/privacy\"},{\"@type\":\"CreativeWork\",\"name\":\"Opinion 28/2024 on certain data protection aspects related to the processing of personal data in the context of AI models, EDPB\",\"url\":\"https://www.edpb.europa.eu/our-work-tools/our-documents/opinion-board-art-64/opinion-282024-certain-data-protection-aspects_en\"},{\"@type\":\"CreativeWork\",\"name\":\"Do LLMs 'store' personal data? This is asking the wrong question, IAPP\",\"url\":\"https://iapp.org/news/a/do-llms-store-personal-data-this-is-asking-the-wrong-question\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd · The unopinionated agent observatory.\",\"url\":\"https://mcpd.sh\"},{\"@type\":\"CreativeWork\",\"name\":\"Security · mcpd\",\"url\":\"https://mcpd.sh/docs/security\"},{\"@type\":\"CreativeWork\",\"name\":\"Recommendations 01/2020 on measures that supplement transfer tools to ensure compliance with the EU level of protection of personal data\",\"url\":\"https://www.edpb.europa.eu/our-work-tools/our-documents/recommendations/recommendations-012020-measures-supplement-transfer_en\"},{\"@type\":\"CreativeWork\",\"name\":\"mcpd repository\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"Regulation (EU) 2016/679 (GDPR)\",\"url\":\"https://www.promptquorum.com/local-llms/data-residency-sovereign-ai-eu-gdpr-enterprise\"}],\"dateModified\":\"2026-11-13\",\"datePublished\":\"2026-11-13\",\"description\":\"Is an LLM trace GDPR personal data? Yes, if it holds a person's text. Learn to redact before storage, set retention, limit access and answer erasure requests.\",\"headline\":\"LLM trace GDPR: Redact, expire and delete data\",\"image\":\"https://api.birchseek.com/m/GJkA5Z5DeZpiuPkVUD3NlPUg7tA5_Xsj.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/llm-trace-gdpr#webpage\"},\"keywords\":[\"llm trace gdpr\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/llm-trace-gdpr#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q626741\"},{\"@id\":\"https://www.wikidata.org/wiki/Q63981718\"},{\"@id\":\"https://www.wikidata.org/wiki/Q456632\"},{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\"},{\"@id\":\"https://www.wikidata.org/wiki/Q136461020\"},{\"@id\":\"https://www.wikidata.org/wiki/Q26877724\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2464},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"Chinese artificial intelligence company\",\"name\":\"DeepSeek\",\"sameAs\":[\"https://en.wikipedia.org/wiki/DeepSeek\",\"https://www.wikidata.org/wiki/Q131577453\",\"https://www.google.com/search?kgmid=/g/11wvrb0s91\"]},{\"@type\":\"Thing\",\"description\":\"French AI company\",\"name\":\"Mistral AI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Mistral_AI\",\"https://www.wikidata.org/wiki/Q119718658\",\"https://www.google.com/search?kgmid=/g/11kqf7g_rz\"]},{\"@type\":\"Thing\",\"description\":\"artificial intelligence tool for code autocompletion\",\"name\":\"GitHub Copilot\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub_Copilot\",\"https://www.wikidata.org/wiki/Q107435063\",\"https://www.google.com/search?kgmid=/g/11q83qbj3d\"]},{\"@type\":\"Thing\",\"description\":\"defined sequence of activities of an organizational entity, enabled by the systematic organization of resources into processes that transform materials, provide services, or process information\",\"name\":\"Workflow\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Workflow\",\"https://www.wikidata.org/wiki/Q627335\",\"https://www.google.com/search?kgmid=/m/0k5rb\"]},{\"@type\":\"Thing\",\"description\":\"maximum number of tokens that a large language model can process at one time\",\"name\":\"Context window\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Context_window\",\"https://www.wikidata.org/wiki/Q125619872\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"process in which information about the past or the present influences the same phenomenon in the present or future; it occurs when outputs of a system are routed back as inputs as part of a chain of cause-and-effect that forms a circuit or loop\",\"name\":\"Feedback\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Feedback\",\"https://www.wikidata.org/wiki/Q183635\",\"https://www.google.com/search?kgmid=/m/03232\"]},{\"@type\":\"Thing\",\"description\":\"chatbot developed by SpaceXAI\",\"name\":\"Grok\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Grok_(chatbot)\",\"https://www.wikidata.org/wiki/Q123361035\",\"https://www.google.com/search?kgmid=/g/11l6lp1dv3\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - gdpr
  - llm observability
  - data retention
  - pii redaction
  - erasure requests
  - opentelemetry
title: "LLM trace GDPR: Redact, expire and delete data"
---

**LLM trace GDPR compliance starts from one fact: a trace is personal data whenever a prompt, completion, tool argument or retrieved document is about a person. The tracing layer must then redact before the write, expire data on a schedule, limit who reads payloads, log access, and find and delete one person's runs. [mcpd redacts before storage](https://mcpd.sh/docs/privacy), because a secret deleted afterwards was still written down.**

A refund agent misreads order numbers, so tracing goes on. Weeks later the trace store holds customers' emails and order histories in the prompts, kept with no expiry. Most GDPR writing about LLMs stops at the model, yet the trace store keeps a second copy of every prompt and answer.

After reading, you can classify what your traces hold, redact it on the way in, and answer an erasure request straight from the trace store.

> **Key Takeaways**
> - Masking and keyed hashing are pseudonymisation: whoever holds the key can re-link the value, so the trace stays in GDPR scope.
> - A redaction rule must cover both `gen_ai.*` and `llm.*` attribute names, plus the logs linked to each span.
> - A tracing vendor or forwarding endpoint is a sub-processor (Article 28) and possibly a Chapter V transfer.
> - Full-text search over payloads makes an Article 15 or 17 request answerable straight from the trace store.

## Is an LLM trace personal data under GDPR?

An LLM trace is personal data under GDPR whenever a prompt, completion, tool argument or retrieved document contains information relating to an identified or identifiable person.

![Illustration for Is an LLM trace personal data under GDPR?](/blog/llm-trace-gdpr/image-1-df713b8bddda.webp)

The test is about content, so it applies wherever text lands in a trace.

Text lands in four places: the input messages on a model span, the completion on that span, the arguments on a tool-call span, and the documents on a retrieval span. A customer's email in a refund prompt qualifies. So does an order history returned by a lookup tool.

Masking leaves the LLM trace GDPR answer unchanged. A keyed hash is pseudonymisation, not anonymisation: whoever holds the key can re-link the value, so the data stays in scope.

For LLM trace GDPR work, each duty translates into a requirement for the trace store:

- Article 15, access: you must be able to find and return one person's runs.
- Article 17, erasure: you must be able to delete those runs, wherever the text sits.
- Article 28, processors: a hosted trace vendor processes on your behalf and needs a contract.
- Articles 44-49, transfers: forwarding traces to another region or vendor is a transfer.

[The EDPB's Opinion 28/2024](https://www.edpb.europa.eu/our-work-tools/our-documents/opinion-board-art-64/opinion-282024-certain-data-protection-aspects_en) (December 2024) covers personal data in AI models, and the [IAPP has published analysis](https://iapp.org/news/a/do-llms-store-personal-data-this-is-asking-the-wrong-question) disputing the Hamburg DPA's discussion paper and arguing that whether an LLM "stores" personal data is the wrong question.

That debate concerns model weights. For LLM trace GDPR work it doesn't matter: a trace holds the prompt and completion text itself, so it's personal data without any theory about what a model retains.

## Where does personal data land inside an LLM trace?

Personal data lands in an LLM trace wherever a span records content: the prompt and completion on the model span, arguments and results on the tool span, and document text on the retrieval span. A support agent that looks up orders produces spans like these, named by the current OpenTelemetry GenAI conventions:

```text
invoke_agent support_agent       root: session or user id, if you attach one
├─ chat                          gen_ai.input.messages   <- email
│                                gen_ai.output.messages  <- reply naming the customer
├─ execute_tool lookup_orders    gen_ai.tool.call.arguments <- email
│                                gen_ai.tool.call.result    <- order history
└─ retrieval                     document text <- note naming the customer
```

```yaml
# illustrative, not a real capture
- name: chat
  attributes:
    gen_ai.input.messages: '[{"role":"user","parts":[{"type":"text","content":"Where is my refund? ana@example.com"}]}]'
- name: execute_tool lookup_orders
  attributes:
    gen_ai.tool.call.result: '{"orders":[{"id":"A-1042","item":"Kettle"}]}'
```

[mcpd reads both schemes](https://mcpd.sh/docs/opentelemetry), so a redaction rule must cover `gen_ai.*` and `llm.*`.

Logs are a signal too, and an LLM trace GDPR review has to cover them. A log line linked to a span can repeat the same email, and in mcpd [every log line links back to its step](https://mcpd.sh), so a run's logs belong in every rule and every deletion.

## How do you log LLM traces under GDPR without storing everything?

A tracing layer logs LLM traces under GDPR without storing everything by redacting before the write, expiring data after a set number of days, limiting who can read payloads and recording who did read them. Missing prompt logs leave a debugging and accountability gap, while payloads kept forever work against Article 5(1)(c) and 5(1)(e).

1. Redact before the write. [mcpd applies redaction before storage](https://mcpd.sh/docs/privacy), on every signal, in four modes: drop, mask, partial and keyed hash.

 A clean-up job leaves a window in which the value already sits in the store. Set rules on the attributes that carry content: the prompt and completion on the model span, and the arguments and results on tool spans.

 - Choose drop for values you never need, such as card numbers and credentials.
 - Choose partial when a fragment helps a human, such as the last four characters of an order number.
 - Choose mask when only the shape matters.
 - Choose keyed hash when you need to see that two runs involve the same customer without reading the email.

 Mask and keyed hash are pseudonymisation under Article 4(5), not anonymisation. Whoever holds the key or the original can re-link the value, so those traces stay personal data.

2. Set retention. Pick a number you can defend for your purpose and configure retention in days. Expired runs leave the store on their own.

 Timings, token counts, cost and tool names stay useful for debugging and for your Article 30 records after the payloads are gone.

3. Restrict who reads payloads. mcpd's [security documentation](https://mcpd.sh/docs/security) lists six API key scopes (ingest, read, write, execute, worker, admin) and four roles (viewer, member, admin, owner).

 Give agents ingest-only keys, so a leaked key can write spans but not read them. Keys are compared as SHA-256 hashes.

4. Keep an audit log.

Solo operators can start with step 1 and a short retention period. That's the core of LLM trace GDPR hygiene. Platform teams add steps 3 and 4.

Try these settings on a free project: [start free on the hosted version](https://mcpd.sh/app).

## How do you answer a GDPR erasure or access request about an LLM trace?

You answer an erasure or access request about an LLM trace by finding every run that contains the person's identifier, exporting or deleting those runs, and then chasing each copy the trace store passed on.

Take a support agent's trace with a customer's email in the prompt and an order history in a tool result. An Article 17 request arrives naming that email.

1. Scope the systems. List every project and environment that receives payloads, the logs signal, and every OTLP forwarding destination. Searching only production is the usual mistake, because staging often holds real customer text too.
2. Search the payloads for the identifier. Labels such as a user id won't carry the email, so use full-text search over the payload with the address and the order number. mcpd's [search finds a run by what the model said](https://mcpd.sh), and the `find_runs` MCP tool does the same for an agent, under the same key scopes. Try variants such as upper-case letters and plus-addressing.
3. Review and export. Open each match and confirm it concerns this person. For an Article 15 request, export those runs after removing other people's data, since a shared conversation can name a second customer.
4. Delete through an erasure request about the person. Submit the identifier from the request to mcpd and follow the behaviour described in [mcpd's privacy docs](https://mcpd.sh/docs/privacy). Success is the same search from step 2 returning zero runs.
5. Check downstream copies. Look at forwarding destinations, backups, and [datasets built from production runs](https://mcpd.sh/docs/concepts). Deleting in the trace store leaves any copy already forwarded elsewhere, so send the request to each destination's owner.
6. Record the request. Note the date received, the identifier type, the runs found and deleted, and each downstream copy you chased.

Redaction would've kept most of this out. Mask or hash the email and drop the order history before the write, and there's nothing to find or delete in the prompt.

A keyed hash still groups one person's runs without storing the address. Hashing the address from the request gives you the value to search for. Whoever holds the key can re-link it, so keep the key out of the trace store.

Don't skip retention in your LLM trace GDPR setup: a short period caps the exposure even if no request ever arrives.

## Is your LLM tracing vendor a sub-processor, and where does the data go?

A hosted tracing vendor is your processor under Article 28, and every forwarding destination you add is a further processor and a possible transfer.

| | Hosted tracing | Self-hosted tracing | OTLP forwarding on top |
| --- | --- | --- | --- |
| Your processor | The hosting vendor | Your own infrastructure provider | Each destination you name |
| Article 28 contract | Yes, with sub-processor flow-down | Only with your cloud or storage provider | Yes, one per destination |
| Chapter V transfer | If vendor or its hosting is outside the EEA | Only if your own hosting is | If the destination is outside the EEA |
| Leaves your network by default | Yes, that is the service | No | Only once you configure it |
| Who controls data location | The vendor, by contract | You | You, by the endpoint you name |

Chapter V (Articles 44 to 49) applies when the vendor or its hosting sits outside the EEA.

If you rely on an adequacy decision for a US vendor, check that it still applies and covers that vendor.

Where there is no adequacy decision, the [EDPB's Recommendations 01/2020](https://www.edpb.europa.eu/our-work-tools/our-documents/recommendations/recommendations-012020-measures-supplement-transfer_en) on supplementary measures, issued after the Schrems II ruling, apply.

mcpd settles the location question in its defaults. Storage is one SQLite file, or Postgres or ClickHouse for a team, and the same binary runs on your own cluster.

[mcpd is not a gateway](https://github.com/loruorg/mcpd) or a proxy, so no prompt travels through it to reach a model.

The hosted version at mcpd.sh/app and self-hosted are the same product, so your choice decides who the processor is.

Relay mode and links between regions keep data in the region where it was written.

Hosted tracing suits a solo operator who wants a first trace in minutes and will sign the vendor's processor terms. Self-hosting fits a platform team whose data cannot leave its network and who wants no external processor for traces. Forwarding works for a team that already runs an observability vendor and has a contract and a transfer basis for it.

## What is a safe LLM trace GDPR default for a solo operator and a platform team?

A safe GDPR default for a solo operator is to redact before the write, keep a short retention and leave the data on one machine, while a platform team adds self-hosting, access control and a records entry.

### Solo operator

1. Pick drop, mask, partial or keyed hash for the prompt and completion attributes.
2. Set retention in days, choosing a short number you can justify for your own use case.
3. Keep the SQLite file on the machine, where the default configuration leaves it.
4. Forward nowhere until you can name the OTLP destination.
5. Search one customer's email and confirm it lists their runs, so an access or erasure request has a starting point.

For the first trace, follow the [mcpd quickstart](https://mcpd.sh/docs/quickstart). Use the hosted version at mcpd.sh/app, set `OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh` with `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`, or build from github.com/loruorg/mcpd.

### Platform team

1. Self-host on your own Kubernetes cluster with `--accounts`, backed by Postgres or ClickHouse.
2. Sign in through OpenID Connect, with roles for each organisation.
3. Issue scoped keys per project, so ingest keys cannot read payloads.
4. Keep the audit log that answers who saw what, and set quotas.
5. Set retention per policy, and use relay and regions to control where data lands.
6. Add an Article 30 entry that names the trace store and every forwarding destination.

Before go-live, ask your data protection officer whether a DPIA is needed for the agent.

## Frequently asked questions

### Does GDPR apply to LLM traces and logs?

GDPR applies when a trace contains information about an identifiable person, exactly as it does to any other stored data. The trace store then needs redaction, retention, access control, an audit log and a way to find and delete one person's runs.

### Is masking a prompt enough to make a trace anonymous?

Masking a field usually leaves a pseudonymised trace, not an anonymous one. A keyed hash can be re-linked by whoever holds the key, and surrounding text such as order details or names can still point to one person, so keep treating the trace as personal data.

### Can I send traces to a US observability vendor?

Sending traces to a vendor outside the EU needs a lawful transfer mechanism, and the vendor is your processor, so a processor contract comes first. Confirm the mechanism against the vendor's published terms. Self-hosting the trace store keeps payloads on infrastructure you already control.

### Do I need to log prompts at all?

Log the spans that let you debug and show how the agent behaved. Storing every full prompt adds risk, so redact personal fields, shorten retention and keep full payloads only where someone reviews them. Too few logs weaken accountability, and too many work against minimisation.

## What LLM trace GDPR steps should you take this week?

Start today by mapping one real trace and switching on redaction, then spend the week on retention, keys and forwarding, and the month on records and contracts.

**Today**

- Open one real trace and mark where personal data sits: the prompt and completion on the model span, arguments on the tool span, document text on the retrieval span.
- Turn on redaction before storage for those attributes.

**This week**

- Set retention in days.
- Tighten key scopes so only the people who need payloads can read them.
- Check where forwarding sends data.
- Pick one customer's email and test that a payload search returns every run containing it.

**This month**

- Add the trace store and every destination to your records of processing.
- Review each sub-processor's terms.
- Decide between hosted and self-hosted.

[Start free on the hosted version](https://mcpd.sh/app), send it one trace and run the Today steps before you close the tab.

## Sources

[Privacy · mcpd](https://mcpd.sh/docs/privacy) · [Regulation (EU) 2016/679 (GDPR)](https://www.promptquorum.com/local-llms/data-residency-sovereign-ai-eu-gdpr-enterprise) · [Opinion 28/2024 on certain data protection aspects related…](https://www.edpb.europa.eu/our-work-tools/our-documents/opinion-board-art-64/opinion-282024-certain-data-protection-aspects_en) · [Do LLMs 'store' personal data? This is asking the wrong…](https://iapp.org/news/a/do-llms-store-personal-data-this-is-asking-the-wrong-question) · [OpenTelemetry · mcpd](https://mcpd.sh/docs/opentelemetry) · [mcpd · The unopinionated agent observatory.](https://mcpd.sh) · [Security · mcpd](https://mcpd.sh/docs/security) · [Recommendations 01/2020 on measures that supplement…](https://www.edpb.europa.eu/our-work-tools/our-documents/recommendations/recommendations-012020-measures-supplement-transfer_en). Every other source is linked where the article cites it.
