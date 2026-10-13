---
title: "Workers"
description: "The workers endpoints of the mcpd HTTP API."
order: 16
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## Register a worker

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/workers/register</code></div>

Stores the definitions a worker serves and tells it the lease timings. Nothing about the worker itself is stored: dispatch reads the runnable set, never a worker table, so a worker that dies needs no deregistration.

**Request body:** `WorkerRegistration`

**Response:** `WorkerRegistered`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `409` | · | A stored version holds a different body under the number you named. A stored version never changes: post the body under a new version, or leave `version` absent and let the daemon assign one. |

**Authentication:** an API key with `worker` scope.

---

## Claim runnable steps

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/steps/claim</code></div>

Long-polls for runnable steps, up to `wait_ms`. A step is runnable when it has a `pending` attempt whose retry timer, if any, has fired, its run and its root are `running`, no other attempt of the same step is live or inside its reclaim window, and neither the run nor its root has spent `max_steps` or `max_cost_usd`. A run refused for a spent ceiling is ended `failed` with an error naming the ceiling.

**Request body:** `ClaimRequest`

**Response:** `ClaimResponse`

**Authentication:** an API key with `worker` scope.

---

## Declare that a step body is about to run

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/steps/{lease_id}/started</code></div>

Moves the attempt from `leased` to `started`, extends the lease and closes an open reclaim window. Required under `at_most_once`: post it, wait for the 200, then run the body. Optional under `at_least_once`, where skipping it only means a crash mid-body is recorded in the journal as a requeue rather than in the trace as an error on that attempt. The body is empty.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `lease_id` | `string` | The lease from the claim. |

**Response:** `LeaseState`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No lease with that id. |
| `409` | `StepConflict` | The lease is no longer the one that may act on this attempt, or the run has ended. `reason` says which, and what to do next. |

**Authentication:** an API key with `worker` scope.

---

## Keep a lease alive

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/steps/{lease_id}/heartbeat</code></div>

Extends the lease. Post it every `heartbeat_ms` while the body runs, whether or not `started` was posted. `reclaimed` is true when the call found the attempt `orphaned` and restored it to the state it came from. The body is empty.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `lease_id` | `string` | The lease from the claim. |

**Response:** `LeaseState`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No lease with that id. |
| `409` | `StepConflict` | The lease is no longer the one that may act on this attempt, or the run has ended. `reason` says which, and what to do next. |

**Authentication:** an API key with `worker` scope.

---

## Complete a step

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/steps/{lease_id}/complete</code></div>

Ends an attempt with an output. In one transaction the attempt becomes `succeeded`, `state` is merged shallowly into the run, `usage` is priced and charged to the attempt, the run and the root, the step's deadlines are deleted, the closing span is enqueued, and once every slot the cursor requires has ended the cursor advances and everything the next position requires is written.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `lease_id` | `string` | The lease from the claim. |

**Request body:** `StepResult`

**Response:** `StepAck`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No lease with that id. |
| `409` | `StepConflict` | The lease is no longer the one that may act on this attempt, or the run has ended. `reason` says which, and what to do next. |

**Authentication:** an API key with `worker` scope.

---

## Fail a step

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/steps/{lease_id}/fail</code></div>

Ends an attempt with an error. `class` decides what follows: `retryable` writes a retry timer while attempts remain, at the node's backoff or `retry_after_ms` if that is larger; `unservable` returns the attempt to `pending` after one lease length without spending the retry budget; every other class applies the node's `on_error` at once, as does a `retryable` failure with no attempts left. `fail_run` ends the run `failed` with the message; `continue` marks the step `failed` and advances.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `lease_id` | `string` | The lease from the claim. |

**Request body:** `StepFailure`

**Response:** `StepAck`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No lease with that id. |
| `409` | `StepConflict` | The lease is no longer the one that may act on this attempt, or the run has ended. `reason` says which, and what to do next. |

**Authentication:** an API key with `worker` scope.

---
