---
title: "Logs"
description: "The logs endpoints of the mcpd HTTP API."
order: 6
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## Search logs

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/logs</code></div>

Log search, filtered by minimum severity, text, time window, trace and span. Because logs carry trace ids, every result links straight to the run that produced it.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |
| `q` | `string` | · | Full-text search across the message and structured fields. |
| `level` | `LogLevel` | · | Minimum severity, inclusive. |
| `trace_id` | `string` | · | Only logs from this run. |
| `span_id` | `string` | · | Only logs from this step. |
| `from` | `timestamp` | · | Start of the window. |
| `to` | `timestamp` | · | End of the window. |
| `order` | `string` | `desc` | `asc` or `desc`. Defaults to `desc`, newest first, which is where an investigation starts. |
| `limit` | `u32` | `100` | Page size, 1 to 1000. |
| `cursor` | `string` | · | Opaque cursor. |

**Response:** `LogListResponse`

**Authentication:** an API key with `read` scope.

---
