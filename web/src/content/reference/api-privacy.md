---
title: "Privacy"
description: "The privacy endpoints of the mcpd HTTP API."
order: 13
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## Erase everything recorded about one person

<div class="endpoint"><span class="method method--delete">DELETE</span><code>/v1/api/subjects/{user_id}</code></div>

Removes every run in the project attributed to this user, with the spans, logs, metrics, scores and feedback attached to those runs, and any log line recorded against the user on its own. On a daemon that serves durable runs, it also removes every ended run whose correlation carries this `user_id`, with its whole tree of subworkflow runs and everything stored for them; a run that has not ended is counted in `runs_in_flight` and left whole. This answers a deletion request in one call, instead of finding each run by hand and deleting it trace by trace. Only the project in the request is touched, and only if the key reaches it: the same user id in a project the key cannot open is left alone and is not counted, so a count can never disclose that the subject exists elsewhere. Returns a count per signal, so the reply to the person can say what went. The erasure is recorded in the audit log.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `user_id` | `string` | The person to erase, written as the `user_id` your instrumentation set on the run. Send it as you sent it at ingest; the daemon applies the same hashing it applied then. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project to erase within. Defaults to the account's own project for an account credential, else the daemon's default project. |

**Response:** `ErasureResult`

**Authentication:** an API key with `admin` scope.

---
