---
title: "Evaluation"
description: "The evaluation endpoints of the mcpd HTTP API."
order: 8
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## List scores

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/scores</code></div>

Every machine evaluation, filterable by scorer, source, run, experiment, value range and time.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |
| `scorer_id` | `string` | · | Only this scorer. |
| `source` | `ScoreSource` | · | Only scores from this source. |
| `trace_id` | `string` | · | Only scores for this run. |
| `experiment_id` | `string` | · | Only scores recorded by this experiment. |
| `min_value` | `f64` | · | Lower bound, inclusive. |
| `max_value` | `f64` | · | Upper bound, inclusive. |
| `from` | `timestamp` | · | Start of the window. |
| `to` | `timestamp` | · | End of the window. |
| `limit` | `u32` | `50` | Page size. |
| `cursor` | `string` | · | Opaque cursor. |

**Response:** `ScoreListResponse`

**Authentication:** an API key with `read` scope.

---

## Attach a score

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/scores</code></div>

Records a machine evaluation against a span or a trace. Supply `min` and `max`: a bare number with no declared scale cannot be compared to anything.

**Request body:** `Score`

**Response:** `Score`

**Authentication:** an API key with `write` scope.

---

## Attach human feedback

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/feedback</code></div>

Records a person's judgement of a run or a step. Call this from your own product's thumbs-up control and the judgement lands on the exact trace that produced the answer.

**Request body:** `Feedback`

**Response:** `Feedback`

**Authentication:** an API key with `write` scope.

---

## List human feedback

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/feedback</code></div>

A project's human judgements, newest first, filtered by run, step, kind, author and time window and paged with a cursor. With no filter it is every piece of feedback the project holds.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |
| `trace_id` | `string` | · | Only feedback on this run. |
| `limit` | `u32` | `50` | Page size, 1 to 500. |
| `span_id` | `string` | · | Only feedback on this step. |
| `kind` | `FeedbackKind` | · | Only this kind of judgement. |
| `author_kind` | `string` | · | Only from this kind of author, such as `end_user` or `reviewer`. |
| `from` | `timestamp` | · | Start of the window. |
| `to` | `timestamp` | · | End of the window. |
| `cursor` | `string` | · | From a previous page. Pages run newest first. |

**Response:** `FeedbackListResponse`

**Authentication:** an API key with `read` scope.

---

## List the built-in scorers

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/scorers</code></div>

Every scorer mcpd ships, with what it measures and whether it needs configuration. Scorers are deterministic code, not model calls, so they cost nothing to run and give the same answer twice.

**Response:** `ScorerInfo`

**Authentication:** an API key with `read` scope.

---

## Run scorers over stored runs

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/score</code></div>

Scores runs that are already stored, and records the results. Naming a scorer that does not exist fails the request before any work happens; a scorer that cannot answer on a given run is reported in `skipped` rather than failing it.

**Request body:** `ScoreRunRequest`

**Response:** `ScoreRunResult`

**Authentication:** an API key with `write` scope.

---

## Compare two experiments

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/experiments/compare</code></div>

Reports the per-scorer delta between two experiments beside the sample size on each side, and refuses to call a comparison decidable below twenty observations.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `baseline` | `string` | **required** | The experiment to compare against. |
| `candidate` | `string` | **required** | The experiment being judged. |
| `project` | `string` | · | Which project to read. |

**Response:** `ExperimentComparison`

**Authentication:** an API key with `read` scope.

---
