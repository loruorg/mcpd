---
title: "Prompts"
description: "The prompts endpoints of the mcpd HTTP API."
order: 11
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## List prompts

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/prompts</code></div>

The latest version of every prompt in the project, each with its labels, sorted by name.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |

**Response:** `Prompt`

**Authentication:** an API key with `read` scope.

---

## Store a prompt version

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/prompts</code></div>

Stores a new version of a prompt and returns it. A `text` prompt needs `template`, a `chat` prompt needs `messages`. A body identical to the latest version is not stored again: the latest version comes back, with any `labels` applied.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |

**Request body:** `CreatePromptRequest`

**Response:** `Prompt`

**Authentication:** an API key with `write` scope.

---

## Get a prompt

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/prompts/{name}</code></div>

One version of a prompt: the one `label` points at, or the one numbered `version`, or the latest when neither is given. This is what an SDK reads before it renders a prompt.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `name` | `string` | The prompt's name. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |
| `label` | `string` | · | Read the version this label points at, such as `production`. |
| `version` | `u32` | · | Read this version. Not with `label`. |

**Response:** `Prompt`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No prompt with that name in the project, no version with that number, or no version carries that label. |

**Authentication:** an API key with `read` scope.

---

## List a prompt's versions

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/prompts/{name}/versions</code></div>

Every version of one prompt, newest first, with the labels that point at each.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `name` | `string` | The prompt's name. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |

**Response:** `PromptVersionsResponse`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No prompt with that name in the project. |

**Authentication:** an API key with `read` scope.

---

## Point a label at a version

<div class="endpoint"><span class="method method--put">PUT</span><code>/v1/api/prompts/{name}/labels/{label}</code></div>

Moves a label to a version, creating the label if it is new. Moving `production` is how a prompt change is released, and moving it back is how one is rolled back; no code is deployed for either. `latest` is reserved: it always means the newest version.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `name` | `string` | The prompt's name. |
| `label` | `string` | The label. Letters, digits, `-`, `_` and `.`, up to 64 characters. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |

**Request body:** `PromptLabelRequest`

**Response:** `Prompt`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No prompt with that name, or no version with that number. |

**Authentication:** an API key with `write` scope.

---

## Remove a label

<div class="endpoint"><span class="method method--delete">DELETE</span><code>/v1/api/prompts/{name}/labels/{label}</code></div>

Removes a label from whichever version holds it. A caller that then asks for the label is answered 404, so remove a label only once nothing reads it.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `name` | `string` | The prompt's name. |
| `label` | `string` | The label. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project. A selector: refused with 403 on a mismatch with the key's grant, filled in for a single-project key, and a 400 when a multi-project key names none. |

**Response:** `DeleteResult`

**Authentication:** an API key with `write` scope.

---
