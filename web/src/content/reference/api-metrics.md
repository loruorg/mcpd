---
title: "Metrics"
description: "The metrics endpoints of the mcpd HTTP API."
order: 7
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## Query a metric as a time series

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/metrics</code></div>

Aggregates raw points into time buckets and returns a series per label combination. Supports sum, count, mean, min, max and quantiles, so a chart is one request rather than a client-side fold over raw points.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |
| `name` | `string` | **required** | Metric name. Required. |
| `from` | `timestamp` | **required** | Start of the window. Required. |
| `to` | `timestamp` | **required** | End of the window. Required. |
| `bucket` | `string` | · | Bucket width, such as `1m`, `5m`, `1h`. Chosen automatically when omitted. |
| `agg` | `string` | `mean` | `sum`, `count`, `mean`, `min`, `max`, `p50`, `p90`, `p95` or `p99`. |
| `group_by` | `string[]` | · | Label keys to split the series on. |
| `filter` | `string` | · | Label equality filters, as `key:value` pairs joined by commas: `model:gpt-4o,status:ok`. A point matches when every named label has that value. A colon, not `=`, so the value needs no escaping in a query string. A key cannot contain `:` or `,`, and a value cannot contain `,`. A JSON object here matches no point, and the answer is an empty series. |

**Response:** `MetricSeriesResponse`

**Authentication:** an API key with `read` scope.

---

## Read a histogram metric as distributions

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/metrics/histogram</code></div>

Merges the buckets of every point of a histogram metric in the window into one distribution per label combination, and reads p50, p90, p95 and p99 from the merged counts. Use it for latency and size distributions, where a mean of bucket sums answers nothing.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |
| `name` | `string` | **required** | Metric name. Required. |
| `from` | `timestamp` | · | Start of the window. The last hour when omitted. |
| `to` | `timestamp` | · | End of the window. Now when omitted. |
| `group_by` | `string[]` | · | Label keys to split the distributions on. |
| `filter` | `string` | · | Label equality filters, as `key:value` pairs joined by commas, as on `/v1/api/metrics`. |

**Response:** `MetricHistogram`

**Authentication:** an API key with `read` scope.

---

## List stored metric points

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/metrics/points</code></div>

The raw points behind a chart, newest first, with their labels, buckets and exemplar links. Use it to see exactly what a producer sent.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |
| `name` | `string` | · | Only this metric. |
| `from` | `timestamp` | · | Start of the window. |
| `to` | `timestamp` | · | End of the window. |
| `filter` | `string` | · | Label equality filters, as `key:value` pairs joined by commas. |
| `trace_id` | `string` | · | Only points whose exemplar names this run. |
| `order` | `string` | `desc` | `asc` or `desc`. |
| `limit` | `u32` | `100` | Page size, 1 to 1000. |
| `cursor` | `string` | · | From a previous page. |

**Response:** `MetricPointListResponse`

**Authentication:** an API key with `read` scope.

---
