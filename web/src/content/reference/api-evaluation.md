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

## Read the review queue

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/review-queue</code></div>

The runs queued for review in the project, newest change first, filtered by status and assignee.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |
| `status` | `ReviewStatus` | · | Only items with this status. |
| `assignee` | `string` | · | Only items assigned to this person. |
| `limit` | `u32` | `100` | Page size, 1 to 500. |

**Response:** `ReviewQueueResponse`

**Authentication:** an API key with `read` scope.

---

## Queue a run for review

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/review-queue</code></div>

Puts a run, or one step of it, in the review queue as `pending`. The run must exist in the project.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |

**Request body:** `CreateReviewItemRequest`

**Response:** `ReviewItem`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No run with that id in the project. |

**Authentication:** an API key with `write` scope.

---

## Review, assign or dismiss an item

<div class="endpoint"><span class="method method--patch">PATCH</span><code>/v1/api/review-queue/{review_id}</code></div>

Changes an item's status, assignee or note. Each change is a new version of the item, so its history is kept.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `review_id` | `string` | The item. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |

**Request body:** `UpdateReviewItemRequest`

**Response:** `ReviewItem`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No review item with that id in the project. |

**Authentication:** an API key with `write` scope.

---

## List the scorers

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/scorers</code></div>

Every scorer this daemon can run: the code scorers, the judged rubrics, the criteria in the config file, and, with `project`, the definitions that project stored. Code scorers are deterministic, cost nothing and give the same answer twice; judged ones make one model call per run.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Also list the scorer definitions this project stored, each at its latest version. |

**Response:** `ScorerInfo`

**Authentication:** an API key with `read` scope.

---

## Run scorers over stored runs

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/score</code></div>

Scores runs that are already stored, and records the results. Naming a scorer that does not exist fails the request before any work happens; a scorer that cannot answer on a given run is reported in `skipped` rather than failing it. A stored definition is named as `id` or `id@version`. Each judged score is recorded with the trace of its own model call, in `score_trace_id`: a `scorer_run` span holding the prompt, the reply, the tokens and the cost.

**Request body:** `ScoreRunRequest`

**Response:** `ScoreRunResult`

**Authentication:** an API key with `write` scope.

---

## List scorer definitions

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/scorer-definitions</code></div>

The latest version of every scorer the project defined, sorted by id.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |

**Response:** `ScorerDefinition`

**Authentication:** an API key with `read` scope.

---

## Store a scorer definition

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/scorer-definitions</code></div>

Stores a new version of a scorer definition. A `judge` definition needs `prompt` and a judge in the config file to run; a `code` definition needs `builtin`, and its `config` is checked against that scorer before anything is stored.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |

**Request body:** `CreateScorerDefinitionRequest`

**Response:** `ScorerDefinition`

**Authentication:** an API key with `write` scope.

---

## Get a scorer definition

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/scorer-definitions/{id}</code></div>

One version of a definition, the latest when `version` is absent.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `id` | `string` | The definition's id. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |
| `version` | `u32` | · | Which version. Absent means the latest. |

**Response:** `ScorerDefinition`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No definition with that id, or no such version. |

**Authentication:** an API key with `read` scope.

---

## List a scorer definition's versions

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/scorer-definitions/{id}/versions</code></div>

Every version of one definition, newest first.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `id` | `string` | The definition's id. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |

**Response:** `ScorerDefinitionVersions`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No definition with that id. |

**Authentication:** an API key with `read` scope.

---

## List live scoring rules

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/evaluation-rules</code></div>

The latest version of every live scoring rule in the project.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |

**Response:** `EvaluationRule`

**Authentication:** an API key with `read` scope.

---

## Store a live scoring rule

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/evaluation-rules</code></div>

Stores a new version of a rule; the daemon applies the latest version of each rule from the next run that finishes. Every named scorer is resolved before anything is stored, and a judged scorer is refused when no judge is configured. Turn a rule off by posting it with `enabled: false`.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |

**Request body:** `CreateEvaluationRuleRequest`

**Response:** `EvaluationRule`

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
