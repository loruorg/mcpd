---
title: "Datasets"
description: "The datasets endpoints of the mcpd HTTP API."
order: 10
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## List datasets

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/datasets</code></div>

Every dataset in the project, with its case count.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project to read. |

**Response:** `Dataset`

**Authentication:** an API key with `read` scope.

---

## Create a dataset

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/datasets</code></div>

A dataset is a named set of cases to run a version of an agent against. Leave `dataset_id` empty and mcpd assigns one.

**Request body:** `Dataset`

**Response:** `Dataset`

**Authentication:** an API key with `write` scope.

---

## List the cases in a dataset

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/datasets/{dataset_id}/items</code></div>

The cases themselves, each with its input, its expected answer when it has one, and the run it was captured from.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `dataset_id` | `string` | Which dataset. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `limit` | `u32` | `500` | Page size. |
| `version` | `u32` | · | Read a frozen version's cases instead of the current ones. |

**Response:** `DatasetItem`

**Authentication:** an API key with `read` scope.

---

## Add cases to a dataset

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/datasets/{dataset_id}/items</code></div>

Adds cases written by hand or exported from somewhere else. Leave each `item_id` empty and mcpd assigns one.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `dataset_id` | `string` | Which dataset. |

**Request body:** `DatasetItem`

**Response:** `ItemsAdded`

**Authentication:** an API key with `write` scope.

---

## Capture production runs as cases

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/datasets/{dataset_id}/capture</code></div>

Turns runs that already happened into permanent cases. This is what datasets are for: the run that went wrong in production on Tuesday becomes a regression test that every later version has to pass, and it keeps its trace id, so the case always points back at the run it came from.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `dataset_id` | `string` | The dataset to add to. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project the runs are in. |

**Request body:** `CaptureRequest`

**Response:** `CaptureResult`

**Authentication:** an API key with `write` scope.

---

## Freeze a dataset version

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/datasets/{dataset_id}/versions</code></div>

Copies the dataset's current cases into a new numbered version. A version never changes, so an experiment run against it today and one run next month read the same cases.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `dataset_id` | `string` | Which dataset. |

**Request body:** `CreateDatasetVersionRequest`

**Response:** `DatasetVersion`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No dataset with that id. |

**Authentication:** an API key with `write` scope.

---

## List a dataset's versions

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/datasets/{dataset_id}/versions</code></div>

Every frozen version of the dataset, newest first.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `dataset_id` | `string` | Which dataset. |

**Response:** `DatasetVersion`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No dataset with that id. |

**Authentication:** an API key with `read` scope.

---

## List experiments

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/experiments</code></div>

Every recorded run of a dataset against a version of an agent.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project to read. |

**Response:** `Experiment`

**Authentication:** an API key with `read` scope.

---

## Record an experiment

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/experiments</code></div>

Registers a run of a dataset so its scores can be compared against another version later. Scores recorded with this experiment's id are what `compare` reads.

**Request body:** `Experiment`

**Response:** `Experiment`

**Authentication:** an API key with `write` scope.

---

## Score an experiment's runs

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/experiments/{experiment_id}/score</code></div>

Runs scorers over every run recorded under the experiment and stores the scores with source `experiment`. A run that names its dataset case in the `mcpd.dataset_item_id` attribute is scored against that case's expected answer, so `exact_match`, `similarity` and the judged rubrics that read `expected` compare each answer with its own case. This is the step an SDK's experiment runner calls after the target has answered every case.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `experiment_id` | `string` | The experiment. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |

**Request body:** `ExperimentScoreRequest`

**Response:** `ScoreRunResult`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No experiment with that id in the project. |

**Authentication:** an API key with `write` scope.

---

## Read an experiment case by case

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/experiments/{experiment_id}/results</code></div>

Every run recorded under the experiment with its case, its answer and its scores, and a summary per scorer.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `experiment_id` | `string` | The experiment. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |

**Response:** `ExperimentResults`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No experiment with that id in the project. |

**Authentication:** an API key with `read` scope.

---
