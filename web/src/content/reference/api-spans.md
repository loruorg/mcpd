---
title: "Spans"
description: "The spans endpoints of the mcpd HTTP API."
order: 5
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## Search spans across traces

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/spans</code></div>

Search individual steps rather than whole runs. This is how you answer 'which tool call is slow' or 'show me every call to this model that errored', across every trace at once.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |
| `q` | `string` | · | Full-text search across name, input and output. |
| `kind` | `SpanKind` | · | Only this kind of step. |
| `status` | `SpanStatus` | · | Only this outcome. |
| `name` | `string` | · | Exact span name. |
| `model` | `string` | · | Only spans against this model. |
| `trace_id` | `string` | · | Restrict to one trace. |
| `from` | `timestamp` | · | Start of the window. |
| `to` | `timestamp` | · | End of the window. |
| `min_duration_ms` | `u64` | · | Only spans at least this slow. |
| `sort` | `string` | `start_time` | `start_time`, `duration` or `cost`. |
| `order` | `string` | `desc` | `asc` or `desc`. |
| `limit` | `u32` | `50` | Page size, 1 to 500. |
| `cursor` | `string` | · | Opaque cursor. |

**Response:** `SpanListResponse`

**Authentication:** an API key with `read` scope.

---
