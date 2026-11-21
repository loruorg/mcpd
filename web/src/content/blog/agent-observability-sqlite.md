---
description: Need agent observability with SQLite? Store runs as span trees, use WAL mode, price tokens at ingest and search prompts. Learn when to move to Postgres.
faq:
  - answer: SQLite is enough for one engineer or a small team on one machine. WAL mode lets the interface read while the agent writes, but SQLite allows one writer at a time. When several writers, a team or shared data arrive, move the same data to Postgres or ClickHouse, or forward it as OTLP.
    question: Is SQLite enough for agent observability?
  - answer: A second writer arrived while another held the write lock, and SQLite allows only one writer at a time. Setting busy_timeout makes the blocked connection wait and retry instead of failing at once. WAL mode keeps readers from adding to the contention.
    question: "Why does SQLite say 'database is locked' during trace ingest?"
  - answer: "Recording traces adds no hop to the agent's calls, so it does not slow the agent down. mcpd isn't a proxy, so nothing travels through it to reach a model. A slow or stopped daemon cannot slow or break your agent."
    question: Does recording traces slow my agent down?
  - answer: mcpd works with OpenInference as well as gen_ai.* because it reads both the OpenTelemetry gen_ai.* and the OpenInference llm.* conventions, so existing instrumentation works unchanged. Point your exporter at the daemon with OTEL_EXPORTER_OTLP_ENDPOINT. What it stores, it also forwards as OTLP to any endpoint you name.
    question: Does mcpd work with OpenInference as well as `gen_ai.*`?
image: /blog/agent-observability-sqlite/featured-6e25c7dab28e.webp
image_alt: "Illustration for Agent observability with SQLite: A setup guide"
publishDate: "2026-11-21"
schema_json: "{\"@context\":\"https://schema.org\",\"@graph\":[{\"@id\":\"https://mcpd.sh/blog/agent-observability-sqlite#webpage\",\"@type\":[\"WebPage\",\"FAQPage\"],\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q319417\",\"@type\":\"Thing\",\"description\":\"serverless relational database management system (RDBMS)\",\"name\":\"SQLite\",\"sameAs\":[\"https://en.wikipedia.org/wiki/SQLite\",\"https://www.wikidata.org/wiki/Q319417\",\"https://www.google.com/search?kgmid=/m/01kgq9\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q627688\",\"@type\":\"Thing\",\"description\":\"technique for providing atomicity and durability in database systems, for crash and transaction recovery, by recording changes in a log written to stable storage before writing changes to the database\",\"name\":\"Write-ahead logging\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Write-ahead_logging\",\"https://www.wikidata.org/wiki/Q627688\",\"https://www.google.com/search?kgmid=/m/01kg_v\"]}],\"datePublished\":\"2026-11-21\",\"description\":\"Need agent observability with SQLite? Store runs as span trees, use WAL mode, price tokens at ingest and search prompts. Learn when to move to Postgres.\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/#website\"},\"mainEntity\":[{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"SQLite is enough for one engineer or a small team on one machine. WAL mode lets the interface read while the agent writes, but SQLite allows one writer at a time. When several writers, a team or shared data arrive, move the same data to Postgres or ClickHouse, or forward it as OTLP.\"},\"name\":\"Is SQLite enough for agent observability?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"A second writer arrived while another held the write lock, and SQLite allows only one writer at a time. Setting busy_timeout makes the blocked connection wait and retry instead of failing at once. WAL mode keeps readers from adding to the contention.\"},\"name\":\"Why does SQLite say 'database is locked' during trace ingest?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"Recording traces adds no hop to the agent's calls, so it does not slow the agent down. mcpd isn't a proxy, so nothing travels through it to reach a model. A slow or stopped daemon cannot slow or break your agent.\"},\"name\":\"Does recording traces slow my agent down?\"},{\"@type\":\"Question\",\"acceptedAnswer\":{\"@type\":\"Answer\",\"text\":\"mcpd works with OpenInference as well as gen_ai.* because it reads both the OpenTelemetry gen_ai.* and the OpenInference llm.* conventions, so existing instrumentation works unchanged. Point your exporter at the daemon with OTEL_EXPORTER_OTLP_ENDPOINT. What it stores, it also forwards as OTLP to any endpoint you name.\"},\"name\":\"Does mcpd work with OpenInference as well as `gen_ai.*`?\"}],\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\",\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q2063\",\"@type\":\"Thing\",\"description\":\"text-based open standard designed for human-readable data interchange\",\"name\":\"JSON\",\"sameAs\":[\"https://en.wikipedia.org/wiki/JSON\",\"https://www.wikidata.org/wiki/Q2063\",\"https://www.google.com/search?kgmid=/m/05cntt\"]},{\"@id\":\"https://www.wikidata.org/wiki/Q652289\",\"@type\":\"Thing\",\"description\":\"process of removing sensitive information from a document or other message, so that the document may be distributed to a broader audience\",\"name\":\"Redaction\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Redaction\",\"https://www.wikidata.org/wiki/Q652289\",\"https://www.google.com/search?kgmid=/m/07c4qw\"]}],\"name\":\"Agent observability with SQLite: A setup guide\",\"primaryImageOfPage\":{\"@type\":\"ImageObject\",\"url\":\"https://api.birchseek.com/m/p2ACWKIi0ZMwvdsTSIFzFMSEYxY4iDh4.webp\"},\"url\":\"https://mcpd.sh/blog/agent-observability-sqlite\"},{\"@id\":\"https://mcpd.sh/blog/agent-observability-sqlite#article\",\"@type\":\"BlogPosting\",\"about\":[{\"@id\":\"https://www.wikidata.org/wiki/Q319417\"},{\"@id\":\"https://www.wikidata.org/wiki/Q627688\"}],\"author\":{\"@id\":\"https://mcpd.sh/#organization\"},\"citation\":[{\"@type\":\"CreativeWork\",\"name\":\"loruorg/mcpd README\",\"url\":\"https://github.com/loruorg/mcpd\"},{\"@type\":\"CreativeWork\",\"name\":\"Set A Busy Timeout\",\"url\":\"https://sqlite.org/c3ref/busy_timeout.html\"},{\"@type\":\"CreativeWork\",\"name\":\"Privacy · mcpd\",\"url\":\"https://mcpd.sh/docs/privacy\"},{\"@type\":\"CreativeWork\",\"name\":\"Write-Ahead Logging\",\"url\":\"https://sqlite.org/wal.html\"},{\"@type\":\"CreativeWork\",\"name\":\"Cost · mcpd\",\"url\":\"https://mcpd.sh/docs/cost\"},{\"@type\":\"CreativeWork\",\"name\":\"OpenTelemetry · mcpd\",\"url\":\"https://mcpd.sh/docs/opentelemetry\"},{\"@type\":\"CreativeWork\",\"name\":\"Integrating · mcpd\",\"url\":\"https://mcpd.sh/docs/integrating\"},{\"@type\":\"CreativeWork\",\"name\":\"STRICT Tables - SQLite\",\"url\":\"https://www.sqlite.org/stricttables.html\"},{\"@type\":\"CreativeWork\",\"name\":\"Result and Error Codes\",\"url\":\"https://www.sqlite.org/rescode.html\"},{\"@type\":\"CreativeWork\",\"name\":\"Write-Ahead Logging\",\"url\":\"https://www.sqlite.org/wal.html\"},{\"@type\":\"CreativeWork\",\"name\":\"PRAGMA Statements\",\"url\":\"https://www.sqlite.org/pragma.html\"},{\"@type\":\"CreativeWork\",\"name\":\"Set A Busy Timeout\",\"url\":\"https://www.sqlite.org/c3ref/busy_timeout.html\"},{\"@type\":\"CreativeWork\",\"name\":\"SQLite FTS5 Extension\",\"url\":\"https://www.sqlite.org/fts5.html\"},{\"@type\":\"CreativeWork\",\"name\":\"Alerting · mcpd\",\"url\":\"https://mcpd.sh/docs/alerting\"},{\"@type\":\"CreativeWork\",\"name\":\"VACUUM, SQLite documentation\",\"url\":\"https://www.sqlite.org/lang_vacuum.html\"},{\"@type\":\"CreativeWork\",\"name\":\"Appropriate Uses For SQLite\",\"url\":\"https://sqlite.org/whentouse.html\"},{\"@type\":\"CreativeWork\",\"name\":\"Relay and regions · mcpd\",\"url\":\"https://mcpd.sh/docs/relay\"},{\"@type\":\"CreativeWork\",\"name\":\"Running it · mcpd\",\"url\":\"https://mcpd.sh/docs/operations\"}],\"dateModified\":\"2026-11-21\",\"datePublished\":\"2026-11-21\",\"description\":\"Need agent observability with SQLite? Store runs as span trees, use WAL mode, price tokens at ingest and search prompts. Learn when to move to Postgres.\",\"headline\":\"Agent observability with SQLite: A setup guide\",\"image\":\"https://api.birchseek.com/m/p2ACWKIi0ZMwvdsTSIFzFMSEYxY4iDh4.webp\",\"inLanguage\":\"en\",\"isPartOf\":{\"@id\":\"https://mcpd.sh/blog/agent-observability-sqlite#webpage\"},\"keywords\":[\"agent observability with sqlite\"],\"mainEntityOfPage\":{\"@id\":\"https://mcpd.sh/blog/agent-observability-sqlite#webpage\"},\"mentions\":[{\"@id\":\"https://www.wikidata.org/wiki/Q121746046\"},{\"@id\":\"https://www.wikidata.org/wiki/Q2063\"},{\"@id\":\"https://www.wikidata.org/wiki/Q652289\"}],\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"wordCount\":2844},{\"@id\":\"https://mcpd.sh/#website\",\"@type\":\"WebSite\",\"inLanguage\":\"en\",\"name\":\"mcpd\",\"publisher\":{\"@id\":\"https://mcpd.sh/#organization\"},\"url\":\"https://mcpd.sh/\"},{\"@id\":\"https://mcpd.sh/#organization\",\"@type\":\"Organization\",\"knowsAbout\":[{\"@type\":\"Thing\",\"description\":\"language model built with very large amounts of texts\",\"name\":\"Large language model\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Large_language_model\",\"https://www.wikidata.org/wiki/Q115305900\",\"https://www.google.com/search?kgmid=/g/11kc9956b3\"]},{\"@type\":\"Thing\",\"name\":\"OpenTelemetry\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenTelemetry\",\"https://www.wikidata.org/wiki/Q121746046\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence research organization\",\"name\":\"OpenAI\",\"sameAs\":[\"https://en.wikipedia.org/wiki/OpenAI\",\"https://www.wikidata.org/wiki/Q21708200\",\"https://www.google.com/search?kgmid=/g/11bxc656v6\"]},{\"@type\":\"Thing\",\"description\":\"large language model family developed by Anthropic\",\"name\":\"Claude\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Claude_(AI)\",\"https://www.wikidata.org/wiki/Q118876059\",\"https://www.google.com/search?kgmid=/g/11kq5ghr35\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence corporation\",\"name\":\"Anthropic\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Anthropic\",\"https://www.wikidata.org/wiki/Q116758847\",\"https://www.google.com/search?kgmid=/g/11csrrqr2z\"]},{\"@type\":\"Thing\",\"description\":\"language model application development framework\",\"name\":\"LangChain\",\"sameAs\":[\"https://en.wikipedia.org/wiki/LangChain\",\"https://www.wikidata.org/wiki/Q117340550\"]},{\"@type\":\"Thing\",\"description\":\"hosting service for software projects using Git\",\"name\":\"GitHub\",\"sameAs\":[\"https://en.wikipedia.org/wiki/GitHub\",\"https://www.wikidata.org/wiki/Q364\",\"https://www.google.com/search?kgmid=/m/04g0kcw\"]},{\"@type\":\"Thing\",\"description\":\"software interface between computers and/or programs\",\"name\":\"API\",\"sameAs\":[\"https://en.wikipedia.org/wiki/API\",\"https://www.wikidata.org/wiki/Q165194\",\"https://www.google.com/search?kgmid=/m/0z5n\"]},{\"@type\":\"Thing\",\"description\":\"protocol for applications to provide context to LLMs\",\"name\":\"Model Context Protocol\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Model_Context_Protocol\",\"https://www.wikidata.org/wiki/Q133436854\",\"https://www.google.com/search?kgmid=/g/11x5hnm0vb\"]},{\"@type\":\"Thing\",\"description\":\"permissive free software license\",\"name\":\"MIT License\",\"sameAs\":[\"https://en.wikipedia.org/wiki/MIT_License\",\"https://www.wikidata.org/wiki/Q334661\",\"https://www.google.com/search?kgmid=/m/04vpm\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology corporation\",\"name\":\"Microsoft\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Microsoft\",\"https://www.wikidata.org/wiki/Q2283\",\"https://www.google.com/search?kgmid=/m/04sv4\"]},{\"@type\":\"Thing\",\"description\":\"open-source Column-oriented DBMS (columnar database management system) for online analytical processing (OLAP)\",\"name\":\"ClickHouse\",\"sameAs\":[\"https://en.wikipedia.org/wiki/ClickHouse\",\"https://www.wikidata.org/wiki/Q27825826\",\"https://www.google.com/search?kgmid=/g/11c1x3wdfw\"]},{\"@type\":\"Thing\",\"description\":\"software that anyone is free to use and redistribute in its current state with a permissive licence giving a libre access to its original source code (but not necessarily to modify it)\",\"name\":\"Open-source software\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Open-source_software\",\"https://www.wikidata.org/wiki/Q1130645\",\"https://www.google.com/search?kgmid=/m/01pjyj\"]},{\"@type\":\"Thing\",\"description\":\"Python library\",\"name\":\"langsmith\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q132393290\"]},{\"@type\":\"Thing\",\"description\":\"software to manage containers on a server-cluster\",\"name\":\"Kubernetes\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Kubernetes\",\"https://www.wikidata.org/wiki/Q22661306\",\"https://www.google.com/search?kgmid=/g/11b7lxp79d\"]},{\"@type\":\"Thing\",\"description\":\"American multinational technology company, a subsidiary of Alphabet Inc.\",\"name\":\"Google\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Google\",\"https://www.wikidata.org/wiki/Q95\",\"https://www.google.com/search?kgmid=/m/045c7b\"]},{\"@type\":\"Thing\",\"description\":\"general-purpose programming language\",\"name\":\"Python\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Python_(programming_language)\",\"https://www.wikidata.org/wiki/Q28865\",\"https://www.google.com/search?kgmid=/m/05z1_\"]},{\"@type\":\"Thing\",\"description\":\"creation or optimization of a prompt to be given to an artificial intelligence model\",\"name\":\"Prompt engineering\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Prompt_engineering\",\"https://www.wikidata.org/wiki/Q108941486\",\"https://www.google.com/search?kgmid=/g/11p6kpgt_n\"]},{\"@type\":\"Thing\",\"description\":\"American artificial intelligence software company\",\"name\":\"Braintrust\",\"sameAs\":[\"https://en.wikipedia.org/wiki/Braintrust\",\"https://www.wikidata.org/wiki/Q141164091\"]},{\"@type\":\"Thing\",\"description\":\"Python library\",\"name\":\"mlflow\",\"sameAs\":[\"https://www.wikidata.org/wiki/Q107381745\"]}],\"name\":\"mcpd\",\"url\":\"https://mcpd.sh/\"}]}"
tags:
  - agent observability
  - sqlite
  - opentelemetry
  - llm tracing
  - wal mode
  - redaction
title: "Agent observability with SQLite: A setup guide"
---

**For agent observability with SQLite, store each run as a span tree rather than a flat log, take it in over OpenTelemetry, run SQLite in WAL mode, price tokens once at ingest and keep the exact prompt and completion searchable. mcpd is a daemon that does this with nothing to adopt, and it [forwards what it stores as OTLP](https://github.com/loruorg/mcpd).**

The problem belongs to engineers whose agent works on a laptop and misbehaves in production. A refund agent calls the same order-lookup tool again and again before answering, a customer sends you the duplicate message it produced, and nothing among thousands of runs says which one caused it.

For agent observability with SQLite, one file is enough. This guide shows how to store runs as span trees, keep the file healthy, find the bad run, price it and know when to outgrow the file.

> **Key Takeaways**
> - Send every write through one process and set `busy_timeout` on each connection, because SQLite allows one writer at a time.
> - Store an unknown model's cost as NULL, since a zero makes an unpriced step look free.
> - Redact before the write: a later `DELETE` misses the WAL copy and any backup taken in between.
> - Move to Postgres or ClickHouse when writers on several hosts or a team need the same runs.

## Do you need a framework for agent observability with SQLite?

Agent observability with SQLite needs no framework and no hand-built event bus: point any OpenTelemetry exporter at the mcpd daemon with `OTEL_EXPORTER_OTLP_ENDPOINT`, and it records each model call, tool call and retrieval in a SQLite file by default. There is no schema to design and no library to adopt.

![Illustration for Do you need a framework for agent observability with SQLite?](/blog/agent-observability-sqlite/image-1-5cede56bea1c.webp)

For the hosted version, two environment variables and a key from your project are enough:

```shell
export OTEL_EXPORTER_OTLP_ENDPOINT=https://api.mcpd.sh
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
```

[mcpd's OpenTelemetry guide](https://mcpd.sh/docs/opentelemetry) lists the accepted formats and says mcpd reads both the OpenTelemetry `gen_ai.*` and the OpenInference `llm.*` conventions, so existing instrumentation works unchanged.

Recording can't hurt the agent because of where mcpd sits. According to the [mcpd README](https://github.com/loruorg/mcpd), it is not a gateway and not a proxy: nothing travels through it to reach a model, so a slow or stopped daemon can't slow or break a model call.

The SDKs add a second guard. The [integration guide](https://mcpd.sh/docs/integrating) says recording a span never blocks, never throws and never grows without bound, and a conformance suite checks that all six SDKs and plain curl write structurally identical traces.

The SDKs are a convenience over a plain HTTP API, which the same guide describes.

If you'd rather keep everything on your machine, building from source at github.com/loruorg/mcpd gives the same product. To see a first trace, [start free at mcpd.sh/app](https://mcpd.sh/app), set the two variables and run your agent once.

## What schema does agent observability with SQLite need?

A span tree in SQLite is one `spans` table in which every row carries its own timings and points at its parent, which is what a waterfall needs. A flat audit table has neither, so it can list events but can't draw a run.

1. Create the table as STRICT. [SQLite's documentation on STRICT tables](https://www.sqlite.org/stricttables.html) says that adding the `STRICT` keyword after the closing parenthesis applies strict typing rules to that table, so a stray string can't land in a token column.

```sql
CREATE TABLE spans (
  trace_id       TEXT NOT NULL,
  span_id        TEXT NOT NULL,
  parent_span_id TEXT,
  name           TEXT NOT NULL,
  start_ns       INTEGER NOT NULL,
  end_ns         INTEGER,
  status         TEXT,
  operation      TEXT,
  model          TEXT,
  input_tokens   INTEGER,
  output_tokens  INTEGER,
  cost_usd       REAL,
  input          TEXT,
  output         TEXT,
  attrs          TEXT,
  PRIMARY KEY (trace_id, span_id)
) STRICT;
CREATE INDEX spans_parent ON spans (trace_id, parent_span_id);
```

2. Map each column to both conventions.

 Write the `gen_ai.*` or `llm.*` attribute into `operation`, `model`, `input_tokens` and `output_tokens`, so one column serves both conventions. Leave `cost_usd` NULL for an unknown model.

3. Keep everything else in `attrs`. Store the remaining attributes as JSON text and read them with `json_extract(attrs, '$."tool.name"')`. Promote a key to a column only when you filter on it often.

4. Rebuild the tree with a recursive query. Start at the root span, which has no parent, and join children on `parent_span_id`. Ordering by start time gives the rows of a waterfall, and `depth` gives the indent.

```sql
WITH RECURSIVE tree(span_id, name, start_ns, end_ns, depth) AS (
  SELECT span_id, name, start_ns, end_ns, 0
  FROM spans WHERE trace_id = ?1 AND parent_span_id IS NULL
  UNION ALL
  SELECT s.span_id, s.name, s.start_ns, s.end_ns, t.depth + 1
  FROM spans s JOIN tree t ON s.parent_span_id = t.span_id
  WHERE s.trace_id = ?1
)
SELECT depth, name, start_ns, end_ns FROM tree ORDER BY start_ns;
```

In the result, a looping refund agent appears as the same tool span repeated under one parent, each bar starting where the last ended.

Keeping the mapping in one place at ingest keeps the table stable if attribute names change.

mcpd stores this tree for you.

## How do you configure SQLite for agent trace ingest?

To configure SQLite for agent trace ingest, switch the file to WAL mode, set a busy timeout on every connection, and send every write through one process.

1. Turn on WAL. In the default rollback-journal mode a writer and readers contend, so the interface stalls while the agent is writing spans. The [SQLite WAL documentation](https://www.sqlite.org/wal.html) says "WAL provides more concurrency as readers do not block writers and a writer does not block readers."

 The mode is persistent: [SQLite's PRAGMA documentation](https://www.sqlite.org/pragma.html) says it "stays in effect across multiple database connections and after closing and reopening the database." You set it once.

2. Accept that there is one writer. The WAL documentation says "since there is only one WAL file, there can only be one writer at a time." A second writer has to wait for the first to commit.

3. Set `busy_timeout` on each connection. Without a timeout, the second writer gets SQLITE_BUSY at once, and most drivers show it as "database is locked", as SQLite's list of [result and error codes](https://www.sqlite.org/rescode.html) describes.

 [SQLite's `sqlite3_busy_timeout` documentation](https://www.sqlite.org/c3ref/busy_timeout.html) says the routine "sets a busy handler that sleeps for a specified amount of time when a table is locked." The PRAGMA documentation adds "Each database connection can only have a single busy handler," so run the pragma every time you open a connection.

4. Funnel writes through one process. If a daemon with one ingest path is the only process that writes, writers never queue behind each other, and the timeout only covers short pauses such as a checkpoint. Every reader opens the file read-only.

5. Keep readers short. The WAL file grows until it is checkpointed, and SQLite's WAL documentation says it checkpoints "whenever a COMMIT occurs that causes the WAL file to be 1000 pages or more in size."

 It also warns that if there is always at least one active reader, "no checkpoints will be able to complete and hence the WAL file will grow without bound." Close read transactions when a query finishes.

```sql
PRAGMA journal_mode=WAL;   -- persistent, set once
PRAGMA busy_timeout=5000;  -- milliseconds, set on every connection
```

When the setup works, `PRAGMA journal_mode;` returns `wal`, the interface loads a waterfall while spans keep arriving, and the log shows no "database is locked" errors.

## How do you find the one bad run in a SQLite trace store?

To find the one bad run in a SQLite trace store, start from the monitor that fired, narrow by span count and cost with SQL, then match the stored prompt and completion with FTS5. The queries assume a `spans` table with `trace_id`, `span_id`, `name`, `status`, `cost_usd`, `input` and `output`; rename the columns to match yours.

1. Read the monitor that fired. [mcpd's alerting docs](https://mcpd.sh/docs/alerting) say it evaluates monitors every 60 s on error_rate, latency_p95, cost, score_mean, trace_count and step-count divergence.

 A monitor latches and fires once, so the single alert marks the start of the incident and a later quiet reading doesn't close it.

2. Rank traces by span count and cost. Group spans by trace and put the heaviest run first:

```sql
SELECT trace_id, COUNT(*) AS spans, SUM(cost_usd) AS cost
FROM spans
WHERE status = 'ok'
GROUP BY trace_id
ORDER BY spans DESC, cost DESC
LIMIT 10;
```

If a refund run normally has 6 spans and one shows 40, that run is your candidate. Run it again with `status = 'error'`, since a loop that ends in a timeout won't be marked ok.

3. Search the payload with FTS5. [SQLite's FTS5 documentation](https://www.sqlite.org/fts5.html) describes FTS5 as "an SQLite virtual table module that provides full-text search functionality to database applications." Index the text once:

```sql
CREATE VIRTUAL TABLE span_text USING fts5(
  trace_id UNINDEXED, span_id UNINDEXED, input, output
);
INSERT INTO span_text
  SELECT trace_id, span_id, input, output FROM spans;

SELECT trace_id, span_id FROM span_text
WHERE span_text MATCH 'lookup_order AND "no orders found"';
```

Rows added after the first load need their own insert at ingest, or the index goes stale.

4. Open the waterfall for that trace and order its spans by start time. If the same `lookup_order` span repeats 8 times back to back, you have the loop.

5. Read the repeated step's prompt and completion. If the tool returned an empty list and the completion says the lookup "may have failed" and asks again, the model misread an empty result as an error.

 A summary line such as "tool call, 212 ms" can't show that misreading. The exact text can.

mcpd runs the same path in [the interface's search](https://mcpd.sh/docs/searching).

## How do you work out what an agent run cost?

You work out what an agent run cost by pricing each step once, when its span is ingested, from the model name, its token counts and a price book, then storing the result on that span.

Price at ingest, not at read time. If you price when the page loads, a rate change rewrites the cost of every old run. If you trust a number sent by the producer, every producer has to get it right.

[mcpd's cost docs](https://mcpd.sh/docs/cost) say an unknown model is stored with no cost, not a cost of zero. A zero looks like a free step, while NULL says the price book has no entry.

`SUM` skips NULL, so a total can look complete when it isn't. Count the gaps next to it:

```sql
SELECT SUM(cost_usd) AS priced_cost,
       COUNT(*) FILTER (WHERE cost_usd IS NULL) AS unpriced_spans
FROM spans
WHERE trace_id = ? AND model IS NOT NULL;
```

If `unpriced_spans` isn't 0, the total is a floor.

mcpd does this pricing once per step at ingest, and you can supply your own price book.

In agent observability with SQLite, a looping refund agent shows up as a cost spike. The same step repeats. The run's cost climbs.

## How do you redact traces and keep the SQLite file from growing?

You redact traces by removing or masking sensitive fields before the write, and you keep the SQLite file under control by setting retention in days and running `VACUUM` after large deletes.

Prompts and completions carry names, email addresses and keys that users paste into chats. They are also the biggest rows in the file.

[The mcpd privacy docs](https://mcpd.sh/docs/privacy) say: "Redaction runs before the write, because a secret deleted afterwards was still written down." A later `DELETE` doesn't reach the WAL copy or a backup taken in between.

mcpd offers four redaction modes, applied to every signal, not only spans: drop, mask, partial and keyed hash.

- Drop removes the field. Use it for what you never need, such as an authorisation header.
- Mask swaps the value for a fixed placeholder, for when you only need to know a value was present.
- Partial keeps a few characters, such as the last four digits of a card.
- Keyed hash replaces the value with a keyed digest. The same email always gives the same token, so you can count runs per user without storing the address.

In SQLite, deleting rows doesn't shrink the file: freed pages are reused by new rows, so the file settles at a plateau instead of climbing.

[SQLite's VACUUM documentation](https://www.sqlite.org/lang_vacuum.html) says "The VACUUM command rebuilds the database file, repacking it into a minimal amount of disk space." Run it off-peak, because it rewrites the whole file.

An erasure request about one person works the same way: delete the runs that carry their identifier, then vacuum so the freed pages are rewritten.

A backup is a copy of the file, so it holds every secret that reached the file before redaction could stop it. The [operations docs](https://mcpd.sh/docs/operations) cover backups and storage options, and the [security docs](https://mcpd.sh/docs/security) cover who can read the data.

## When is one file not enough for agent observability with SQLite?

A single SQLite file stops being enough when several writers on different hosts, a team, or people without access to your laptop need the same agent traces.

| Need | One SQLite file | Postgres or ClickHouse in mcpd | OTLP forwarding | Relay beside the agents |
| --- | --- | --- | --- | --- |
| Writers on several hosts | One writer at a time | Database server takes concurrent clients | Each sender exports on its own | Spools, then ships in order |
| Shared team access | One file, one machine | Yes | In the destination's tool | In the daemon behind it |
| Extra infrastructure | None | A database server | A destination endpoint | A relay process per region |

SQLite's own page, [Appropriate Uses For SQLite](https://sqlite.org/whentouse.html), says: "If there are many client programs sending SQL to the same database over a network, then use a client/server database engine instead of SQLite." On writes, it says SQLite "will only allow one writer at any instant in time."

Workers on three hosts writing to one file over a network share is the case that page warns about. A support lead who needs to open a run can't read a file on your machine.

Its README also says "What it stores it also forwards, as OTLP, to any endpoint you name", with presets for Langfuse, LangSmith and others. Check a destination's own self-hosting documentation before you choose it.

When the agents sit in several places, [relay mode](https://mcpd.sh/docs/relay) runs a relay in each region to spool ingest requests before they reach a daemon. The [operations guide](https://mcpd.sh/docs/operations) covers single-instance limits, which tell you when one daemon is no longer enough.

- One SQLite file is best for one engineer, one machine and one writing process.
- Postgres or ClickHouse is best for a team that needs shared access to the same runs.
- OTLP forwarding is best when your traces must also land in a tool you already run.
- A relay is best when agents run in several regions and ingest must survive a daemon outage.

Agent observability with SQLite starts in a file you can open. Later, the same data can move to a database or any OTLP endpoint.

## Frequently asked questions

### Is SQLite enough for agent observability?

It's enough for one engineer or a small team on one machine. [WAL mode](https://www.sqlite.org/wal.html) lets the interface read while the agent writes, but SQLite allows one writer at a time. When several writers, a team or shared data arrive, move the same data to Postgres or ClickHouse, or forward it as OTLP.

### Why does SQLite say 'database is locked' during trace ingest?

A second writer arrived while another held the write lock. [Setting `busy_timeout`](https://www.sqlite.org/c3ref/busy_timeout.html) makes the blocked connection wait and retry instead of failing at once. WAL mode keeps readers from adding to the contention.

### Does recording traces slow my agent down?

It adds no hop to the agent's calls. [mcpd isn't a proxy](https://github.com/loruorg/mcpd), so nothing travels through it to reach a model, and a slow or stopped daemon cannot slow or break your agent.

### Does mcpd work with OpenInference as well as `gen_ai.*`?

Yes. [mcpd's OpenTelemetry docs](https://mcpd.sh/docs/opentelemetry) say it reads both the OpenTelemetry `gen_ai.*` and the OpenInference `llm.*` conventions, so existing instrumentation works unchanged. Point your exporter at the daemon with `OTEL_EXPORTER_OTLP_ENDPOINT`.

## What to do in your first hour, day and week

The first week takes you from one trace to redacted, retained data and a decision about whether one file still fits.

- In the first hour, point an OpenTelemetry exporter at `https://api.mcpd.sh`, or build mcpd from [the mcpd source repository](https://github.com/loruorg/mcpd), and run your agent once. Follow the [mcpd quickstart](https://mcpd.sh/docs/quickstart) and budget five minutes to see the first trace.
- On the first day, find one real run by searching the text of its completion, then set a monitor on error rate or cost.
- In the first week, set redaction so sensitive fields never reach the file, and set retention in days. Check how large the file has grown, then decide whether one file still fits.

[Start free on the hosted version at mcpd.sh/app](https://mcpd.sh/app). You adopt no framework, so there's nothing to remove if it doesn't fit.

## Sources

[loruorg/mcpd README](https://github.com/loruorg/mcpd) · [Write-Ahead Logging](https://sqlite.org/wal.html) · [Write-Ahead Logging](https://www.sqlite.org/wal.html) · [Integrating · mcpd](https://mcpd.sh/docs/integrating) · [PRAGMA Statements](https://www.sqlite.org/pragma.html) · [Set A Busy Timeout](https://www.sqlite.org/c3ref/busy_timeout.html) · [Privacy · mcpd](https://mcpd.sh/docs/privacy) · [Set A Busy Timeout](https://sqlite.org/c3ref/busy_timeout.html). Every other source is linked where the article cites it.
