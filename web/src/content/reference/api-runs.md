---
title: "Runs"
description: "The runs endpoints of the mcpd HTTP API."
order: 15
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## Store a definition

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/definitions</code></div>

Stores one version of a definition. The canonical body is the document with sorted keys and no insignificant whitespace, minus `version`. An identical body already stored is confirmed at its version rather than stored again, so posting the same code twice is a no-op, and a version is immutable once stored. A body the definition schema refuses is a 400 naming the field.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |

**Request body:** `DefinitionDocument`

**Response:** `DefinitionVersion`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `409` | · | A stored version holds a different body under the number you named. A stored version never changes: post the body under a new version, or leave `version` absent and let the daemon assign one. |

**Authentication:** an API key with `write` scope.

---

## List definitions

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/definitions</code></div>

Every stored version of every definition in the project, with its body.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |

**Response:** `Definition`

**Authentication:** an API key with `read` scope.

---

## Get one definition

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/definitions/{id}</code></div>

One definition. Without `version`, the highest stored version, which is the one a run started now would pin.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `id` | `string` | The definition's identifier. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |
| `version` | `u32` | · | Which version. Absent means the highest stored. |

**Response:** `Definition`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No definition with that id in the project, or none at the named version. |

**Authentication:** an API key with `read` scope.

---

## Start a run

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/runs</code></div>

Starts a run and answers it as stored. The run row, its trace id, its first cursor and everything that cursor requires commit in one transaction, so a 200 means the first step is already on disk for a worker to claim. With `idempotency_key`, a repost within 24 hours answers the same run and starts nothing.

**Request body:** `StartRunRequest`

**Response:** `Run`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No definition with that id in the project, or none at the named version. |

**Authentication:** an API key with `execute` scope.

---

## List runs

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/runs</code></div>

Runs in the project, newest first.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |
| `status` | `RunStatus` | · | Only runs in this status. |
| `definition_id` | `string` | · | Only runs of this definition. |
| `limit` | `u32` | `50` | Page size, 1 to 500. |
| `cursor` | `string` | · | Opaque cursor from a previous page. |

**Response:** `RunListResponse`

**Authentication:** an API key with `read` scope.

---

## Get one run with its journal

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/runs/{run_id}</code></div>

One run with every attempt at every step, and the deadlines and waits it currently holds.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `run_id` | `string` | The run to fetch. |

**Response:** `RunDetail`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No run with that id in a project the key reaches. |

**Authentication:** an API key with `read` scope.

---

## Cancel a run

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/runs/{run_id}/cancel</code></div>

Ends the run and every child under it as `cancelled`: their deadlines and waits are deleted and every live attempt is marked `failed`. A worker holding a lease learns on its next call, with a 409 `run_terminal`. A parent of a cancelled child sees the child's step fail and follows its `on_error`, unless the parent itself was the run cancelled. Cancelling a run that has already ended changes nothing and still answers 200. The body is empty.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `run_id` | `string` | The run to cancel. |

**Response:** `Run`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No run with that id in a project the key reaches. |

**Authentication:** an API key with `execute` scope.

---

## Read a run's event log

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/runs/{run_id}/events</code></div>

The run's event log after a sequence number. Send `Accept: text/event-stream` and the same route answers server-sent events instead: one frame per row with `id` set to `seq`, held open until the run ends or you disconnect. `after` is the reattach point in both forms.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `run_id` | `string` | The run to read. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `after` | `i64` | `-1` | Return rows with a sequence number greater than this. Exclusive; the default returns the log from its first row. |
| `limit` | `u32` | `500` | The most rows to return in one page. |

**Response:** `RunEventPage`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No run with that id in a project the key reaches. |

**Authentication:** an API key with `read` scope.

---

## Explain why a step failed

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/runs/{run_id}/cause</code></div>

Walks backwards from one step of a run along the data dependencies its definition declares, and returns the ancestors whose output that step consumed.

A workflow template such as `${steps.classify.output.kind}` is an explicit edge from one step to another, written in the document the run pinned at start, so the chain is read out of the definition rather than guessed from timing or from what happened to run first. The answer names the case a status code cannot: an ancestor that ended `succeeded`, with every step between it and the failure green as well, whose value the failure consumed anyway.

The step key is a query parameter rather than a path segment because a key may contain a slash, as `refund/issue` does, and an encoded slash inside a path segment is rewritten or refused by enough proxies to make it a bad contract.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `run_id` | `string` | The run to read. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `step` | `string` | **required** | The step key to explain, exactly as the journal keys it, such as `refund/issue` or `poll[7]/classify`. |
| `max_depth` | `u32` | `32` | How far back to walk. The walk stops there and sets `truncated`, so a long chain, a loop or a definition that reads its own earlier steps repeatedly cannot produce an unbounded answer. |

**Response:** `StepCause`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No run with that id in a project the key reaches, or the run's definition declares no step with that key. |

**Authentication:** an API key with `read` scope.

---

## Deliver an event to one run

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/runs/{run_id}/signal</code></div>

Delivers an event to one run by id. The daemon reads the run's open wait on `event`, takes its key, and routes the event as the publish endpoint would, with the same deduplication.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `run_id` | `string` | The run that is waiting. |

**Request body:** `SignalRequest`

**Response:** `EventReceipt`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No run with that id, or the run holds no open wait on that event. |

**Authentication:** an API key with `execute` scope.

---

## Publish an event

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/events</code></div>

Delivers an event to whichever run is waiting for it, matched on project, event name and key. Delivery writes the payload as the waiting step's output and moves the run in the same transaction. With no run waiting, the event is parked for 24 hours, and the next run that reaches a matching wait with `accept_parked` consumes it, oldest first.

**Request body:** `PublishEventRequest`

**Response:** `EventReceipt`

**Authentication:** an API key with `execute` scope.

---
