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
| `environment` | `string` | · | Only logs from this environment. |
| `service_name` | `string` | · | Only logs from this service. |
| `service_version` | `string` | · | Only logs from this service version. |
| `session_id` | `string` | · | Only logs from this session. |
| `thread_id` | `string` | · | Only logs from this thread. |
| `user_id` | `string` | · | Only logs about this user. Send the plain id; it is hashed the way the stored one was. |
| `run_id` | `string` | · | Only logs from this durable run. |
| `request_id` | `string` | · | Only logs from this request. |
| `experiment_id` | `string` | · | Only logs recorded by this experiment. |
| `release` | `string` | · | Only logs from this release. |

**Response:** `LogListResponse`

**Authentication:** an API key with `read` scope.

---
