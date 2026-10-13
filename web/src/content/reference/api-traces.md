---
title: "Traces"
description: "The traces endpoints of the mcpd HTTP API."
order: 3
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## List traces

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/traces</code></div>

The trace list. Filter on status, kind, name, tags, session, thread, user, environment, model, time window, duration, cost, tokens, score and feedback, and sort by start time, duration, cost, span count, tokens or score. With `preview=true` each trace carries a short preview of its input and output. Unlike a plain span dump, this reads the materialised trace projection rather than aggregating spans on every request.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |
| `q` | `string` | · | Full-text search across span names, prompts and completions. This is the field that answers 'find the run where the model said that'. |
| `status` | `SpanStatus` | · | Only traces with this outcome. |
| `kind` | `SpanKind` | · | Only traces whose root is this kind. |
| `name` | `string` | · | Exact root span name. |
| `tags` | `string[]` | · | Traces carrying all of these tags. |
| `session_id` | `string` | · | Every trace in one conversation. |
| `thread_id` | `string` | · | Every trace in one thread. |
| `user_id` | `string` | · | Every trace for one end user. Stored ids are hashed, and this value is hashed the same way before matching, so pass the plain id you sent. |
| `environment` | `string` | · | Restrict to one environment. |
| `experiment_id` | `string` | · | Only runs recorded under this experiment. |
| `model` | `string` | · | Traces that used this model. |
| `from` | `timestamp` | · | Start of the time window, inclusive. |
| `to` | `timestamp` | · | End of the time window, exclusive. |
| `min_duration_ms` | `u64` | · | Only traces at least this slow. |
| `max_duration_ms` | `u64` | · | Only traces at most this slow. |
| `min_cost` | `f64` | · | Only traces at least this expensive. |
| `max_cost` | `f64` | · | Only traces at most this expensive. |
| `min_tokens` | `u64` | · | Only traces that used at least this many tokens, input and output together. |
| `has_score` | `string` | · | Only traces carrying a score from this scorer. |
| `score_below` | `f64` | · | Combined with `has_score`: only traces where that scorer came in under this value. This is the query that finds bad answers. |
| `score_above` | `f64` | · | Combined with `has_score`: only traces where that scorer came in over this value. |
| `has_feedback` | `bool` | · | `true` for only traces a person gave feedback on, `false` for only traces with none. |
| `preview` | `bool` | · | Fill `input_preview` and `output_preview` on each trace. A list read with previews carries the start of each run's input and output, so it is recorded in the audit log as `list_traces`; one without is not. |
| `sort` | `string` | `start_time` | One of `start_time`, `duration`, `cost`, `span_count`, `tokens`, `score`. Defaults to `start_time`. |
| `order` | `string` | `desc` | `asc` or `desc`. Defaults to `desc`. |
| `limit` | `u32` | `50` | Page size, 1 to 500. |
| `cursor` | `string` | · | Opaque cursor from a previous page. |

**Response:** `TraceListResponse`

**Authentication:** an API key with `read` scope.

---

## Get one trace with every span

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/traces/{trace_id}</code></div>

Returns the trace projection and the full span tree in one response, ordered so a client can render the waterfall without a second request or a client-side sort.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `trace_id` | `string` | The trace to fetch. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `include` | `string` | · | Comma-separated extras: `logs`, `scores`, `feedback`, `metrics`. Fetching a run and everything attached to it is one request. |

**Response:** `TraceDetail`

**Authentication:** an API key with `read` scope.

---

## Read a run as a trajectory

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/traces/{trace_id}/trajectory</code></div>

The run as the ordered actions its agent took: tool calls, model generations and handoffs, with the tool sequence, the step count and whether it reached an answer. Score one with the `trajectory` scorer.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `trace_id` | `string` | The run. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |

**Response:** `Trajectory`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No run with that id in the project. |

**Authentication:** an API key with `read` scope.

---

## Delete a trace and everything attached to it

<div class="endpoint"><span class="method method--delete">DELETE</span><code>/v1/api/traces/{trace_id}</code></div>

Removes the trace, its spans, logs, metrics, scores and feedback. Used to satisfy an erasure request. The deletion is recorded in the audit log.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `trace_id` | `string` | The trace to delete. |

**Response:** `DeleteResult`

**Authentication:** an API key with `admin` scope.

---

## List the projects that have data

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/projects</code></div>

Every project mcpd has stored a signal for. Projects are not created up front: a project exists as soon as something arrives carrying its name, which is why this reads the data rather than a registry.

**Response:** `String`

**Authentication:** an API key with `read` scope.

---
