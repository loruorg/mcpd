---
title: "Analytics"
description: "The analytics endpoints of the mcpd HTTP API."
order: 9
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## Headline numbers for a time window

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/analytics/summary</code></div>

Run count, error rate, latency quantiles, token totals and cost for a window, optionally split by a dimension. One request backs the overview screen.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |
| `from` | `timestamp` | · | Start of the window. Defaults to 24 hours before `to`. |
| `to` | `timestamp` | · | End of the window. Defaults to now. |
| `group_by` | `string` | · | Split by `model`, `entity_name`, `environment`, `status` or `session_id`. |

**Response:** `AnalyticsSummary`

**Authentication:** an API key with `read` scope.

---

## Cost broken down

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/analytics/cost</code></div>

Spend over a window, split by model, provider, entity, environment, status, session, user, service or run name, with token detail behind each figure. Answers 'where is the money going' without exporting to a spreadsheet.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |
| `from` | `timestamp` | · | Start of the window. Defaults to seven days before `to`. |
| `to` | `timestamp` | · | End of the window. Defaults to now. |
| `group_by` | `string` | `model` | `model`, `provider`, `entity_name`, `environment`, `status`, `session_id`, `user_id`, `service_name` or `name`. |
| `bucket` | `string` | · | Time bucket for a series, such as `1h` or `1d`. Omit for totals only. |

**Response:** `CostBreakdown`

**Authentication:** an API key with `read` scope.

---
