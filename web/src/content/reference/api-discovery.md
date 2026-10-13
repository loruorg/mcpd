---
title: "Discovery"
description: "The discovery endpoints of the mcpd HTTP API."
order: 4
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## The values a filter dimension takes

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/discovery</code></div>

What to offer in a filter box. A filter is only usable if you know what to type into it, and the values are not a fixed list: they are whatever the services reporting to this project happen to send. This reads the distinct-value list the daemon maintains as signals arrive, scoped to one project, prefix-matched for typeahead and ordered by value. Ask for one dimension at a time. One limit is worth knowing before you build against it: `metric_label_key` lists every label key seen anywhere in the project, not the keys of one named metric, because the list is keyed by the label and not by the metric that carried it.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Every dimension is scoped to one. |
| `facet` | `Facet` | **required** | Which dimension to list. Required. |
| `key` | `string` | · | For `metric_label_value`, the label key whose values you want. Required for that facet, because the answer would otherwise be every label's values mixed together. Ignored by every other facet. |
| `prefix` | `string` | · | Offer only values starting with this. Matched literally and case sensitively, because these values are what a filter will be compared against and folding here would offer one that then matches nothing. |
| `limit` | `u32` | `100` | How many values to return, up to 1000. A filter box shows a list somebody reads; ten thousand environments is not a longer list, it is a slower request that renders the same first page. |

**Response:** `DiscoveryResponse`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `501` | · | This deployment's storage backend does not implement what this endpoint needs. The body names the capability in its `capability` field, using the same word `GET /v1/api/capabilities` publishes, so a caller can read the capability set once and stop asking rather than retrying a refusal that will never change. |

**Authentication:** an API key with `read` scope.

---

## What has arrived since you last looked

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/deltas</code></div>

Refresh a list in place instead of running its query again. Hold the cursor from the last poll, send it back, and get the rows that have arrived since, oldest first. The watermark is a row's arrival time and its id together, because a batch writes every row in it with one timestamp and a watermark that could only say 'after this nanosecond' would either replay the whole batch or skip the rest of it. A row written with an arrival time behind the watermark, which a sender can cause by setting `received_at` itself, is not returned; that is the cost of a watermark and no cursor shape avoids it. Sent with `Accept: text/event-stream`, the same request becomes a live tail: the daemon polls every second and sends each batch that brings rows as one `delta` event carrying this response, with the cursor after it as the event id, so a client that reconnects with that cursor misses nothing. A tail closes after thirty minutes and the client reconnects.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to poll. |
| `signal` | `DeltaSignal` | `traces` | Which signal to poll. Defaults to `traces`. |
| `cursor` | `string` | · | The cursor the previous poll returned, unchanged. Absent starts from `since`, or from the oldest row the project holds when that is absent too. A cursor this daemon did not issue is refused rather than guessed at. |
| `since` | `timestamp` | · | Where to start when there is no cursor yet. Ignored once `cursor` is set. |
| `limit` | `u32` | `100` | Page size, 1 to 1000. |

**Response:** `DeltaResponse`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `501` | · | This deployment's storage backend does not implement what this endpoint needs. The body names the capability in its `capability` field, using the same word `GET /v1/api/capabilities` publishes, so a caller can read the capability set once and stop asking rather than retrying a refusal that will never change. |

**Authentication:** an API key with `read` scope.

---
