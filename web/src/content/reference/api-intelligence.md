---
title: "Intelligence"
description: "The intelligence endpoints of the mcpd HTTP API."
order: 17
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## The aspects a project describes traces along

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/aspects</code></div>

Every aspect this project has, with the bounds a clustering pass runs under.

An aspect is an axis. Four ship with the daemon: `goal` is what the user wanted, `sentiment` is the tone they expressed, `behaviour` is what the agent did about it, and `outcome` is how the interaction ended. They are ordinary rows rather than a list compiled into a client, so nothing can hold a second opinion about what exists.

`outcome` ships derived, which means the daemon writes its notes itself from the run's status, the step error class, the silent producer flag on the cause report and the divergence watch, with no model anywhere. A deployment that has configured nothing therefore fills one aspect on its own, which is enough to see the whole pipeline work before a model is wired to anything.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |

**Response:** `AspectCatalogue`

**Authentication:** an API key with `read` scope.

---

## The entities that have aspect notes

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/aspects/entities</code></div>

Which agents, workflows and tools have enough recorded to be worth opening, and which are still filling up.

A row appears as soon as the first note about an entity is recorded, before any theme exists. That is deliberate: an empty screen with a count moving on it is a different thing from an empty screen, and a reader has to be able to tell them apart.

Every roll-up here is computed and none of it is optional, because mcpd serves one version of itself and has no older server to tolerate.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |
| `entity_kind` | `EntityKind` | · | Only entities of this kind. |
| `limit` | `u32` | `50` | Page size, 1 to 200. |
| `cursor` | `string` | · | Opaque cursor from a previous page. |

**Response:** `EntityIndexResponse`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `501` | · | This deployment's storage backend does not implement what this endpoint needs. The body names the capability in its `capability` field, using the same word `GET /v1/api/capabilities` publishes as `theme_analysis`, so a caller can read the capability set once and stop asking rather than retrying a refusal that will never change. |

**Authentication:** an API key with `read` scope.

---

## How far the pipeline has got for one entity

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/aspects/progress</code></div>

The honest empty state. For one entity: how many of its traces carry notes, and per aspect how many notes exist, what state that aspect is in, and how many more notes it needs before a clustering pass will run. Every number is counted from stored rows, so nothing here is an estimate of what is coming.

An aspect reads `collecting` below the floor of 50 notes, `processing` once it has them and no snapshot covers it yet, and `ready` when one does. Read this before concluding that a themes screen is broken: the three states look identical on screen and mean different things.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |
| `entity_kind` | `EntityKind` | **required** | The kind of entity to read. An entity is the pair of a kind and a name, which is the identity spans already carry. |
| `entity_name` | `string` | **required** | The name of that entity. |

**Response:** `AspectProgress`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No entity of that kind and name in a project this key reaches. |
| `501` | · | This deployment's storage backend does not implement what this endpoint needs. The body names the capability in its `capability` field, using the same word `GET /v1/api/capabilities` publishes as `theme_analysis`, so a caller can read the capability set once and stop asking rather than retrying a refusal that will never change. |

**Authentication:** an API key with `read` scope.

---

## One entity's clustering snapshots

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/themes/snapshots</code></div>

The timeline. Each snapshot is one clustering pass over a moving window of notes, and every theme query names exactly one of them.

There are two presentations. `inventory` returns consecutive snapshots for a caller walking the series. `landmarks` returns a bounded, time balanced selection across the whole range, with the oldest and the newest snapshot in that range always included and the rest sampled by time, which is what a timeline of a couple of dozen points needs; each row then says which of the two reasons put it there.

Windows overlap. Two snapshots describe overlapping sets of traces, so their counts must never be added together.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |
| `entity_kind` | `EntityKind` | **required** | The kind of entity to read. An entity is the pair of a kind and a name, which is the identity spans already carry. |
| `entity_name` | `string` | **required** | The name of that entity. |
| `aspects` | `string[]` | · | Only snapshots that cover all of these aspects. Unset means every snapshot for the entity, whatever it covers. |
| `presentation` | `string` | `inventory` | `inventory` for consecutive snapshots, `landmarks` for a time balanced selection across the range. |
| `from` | `timestamp` | · | Only snapshots whose cutoff falls at or after this. It bounds the range a landmark selection is drawn from as well as what is returned. |
| `to` | `timestamp` | · | Only snapshots whose cutoff falls before this. |
| `limit` | `u32` | `24` | How many snapshots to return, 1 to 100. A landmark selection of more than a couple of dozen points is more than a timeline can draw and more than a reader can scrub. |
| `cursor` | `string` | · | Opaque cursor from a previous page. |

**Response:** `SnapshotListResponse`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No entity of that kind and name in a project this key reaches. |
| `501` | · | This deployment's storage backend does not implement what this endpoint needs. The body names the capability in its `capability` field, using the same word `GET /v1/api/capabilities` publishes as `theme_analysis`, so a caller can read the capability set once and stop asking rather than retrying a refusal that will never change. |

**Authentication:** an API key with `read` scope.

---

## Themes across two to four aspects at once

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/themes/flow</code></div>

The cross-aspect view: one stage per aspect, its nodes, and the links between neighbouring stages. A link's count is the number of traces assigned to both of its ends.

This shows association and not causation. A ribbon between a goal theme and an outcome theme means the two were written about the same runs. It does not mean one caused the other.

The order of `aspects` is meaningful and is not a display preference. Links only ever join neighbouring stages, so goal beside outcome answers a different question from goal beside behaviour, and asking for the same aspects in another order asks another question. Fewer than two aspects is refused by name rather than answered with an empty flow, because one stage has no links in it.

Each stage carries at most one noise node and at most one overflow node. Noise is the notes that joined no theme, and it is not a failure bucket: it is rare requests, wording nothing else matched and patterns that have not recurred yet, so a large noise share means varied traffic or too few notes. Overflow is everything past `theme_limit_per_stage`, folded into one node so the stage totals still add up.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |
| `entity_kind` | `EntityKind` | **required** | The kind of entity to read. An entity is the pair of a kind and a name, which is the identity spans already carry. |
| `entity_name` | `string` | **required** | The name of that entity. |
| `aspects` | `string[]` | **required** | Two to four aspect names, in the order the stages are wanted. The order changes the answer. |
| `snapshot_id` | `string` | **required** | Which snapshot to read. Every count in the response belongs to this one window. |
| `theme_limit_per_stage` | `u32` | `8` | How many themes to return per stage, 1 to 100, before the rest are folded into the overflow node. The fold happens on the server because only the server knows what it truncated. |
| `filter` | `string[]` | · | Drill in. Repeat as `filter=aspect:theme_id`, or `filter=aspect:noise`, up to four times with a different aspect each time. The daemon recomputes the counts and the shares over the traces that survive every filter, so the answer does not change shape with the size of the snapshot and a client never has to fetch the assignments and filter them itself. |

**Response:** `ThemeFlow`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No such snapshot for that entity, or `aspects` names an aspect the project does not have. |
| `501` | · | This deployment's storage backend does not implement what this endpoint needs. The body names the capability in its `capability` field, using the same word `GET /v1/api/capabilities` publishes as `theme_analysis`, so a caller can read the capability set once and stop asking rather than retrying a refusal that will never change. |

**Authentication:** an API key with `read` scope.

---

## One theme in one snapshot

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/themes/{theme_id}</code></div>

A theme's label, lifecycle state and counts in one snapshot, returned beside the snapshot's own counts so a caller can write `28 of 70 traces in this window` without a second request.

A theme this snapshot does not hold is a 200 with no theme rather than a 404. Scrubbing a timeline back past a theme's birth is a reasonable question, and the answer is that the theme was not there yet. The 404 is kept for a theme id this entity has never had, which is a different mistake and deserves a different answer.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `theme_id` | `string` | The theme to read. `noise` is reserved and is not a theme: read a stage's noise from the flow. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |
| `entity_kind` | `EntityKind` | **required** | The kind of entity to read. An entity is the pair of a kind and a name, which is the identity spans already carry. |
| `entity_name` | `string` | **required** | The name of that entity. |
| `aspect` | `string` | **required** | Which aspect the theme was clustered along. |
| `snapshot_id` | `string` | **required** | Which snapshot to read it in. |

**Response:** `ThemeDetail`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No such snapshot, or a theme id this entity has never had. A theme that exists and is absent from this snapshot is a 200 with no theme. |
| `501` | · | This deployment's storage backend does not implement what this endpoint needs. The body names the capability in its `capability` field, using the same word `GET /v1/api/capabilities` publishes as `theme_analysis`, so a caller can read the capability set once and stop asking rather than retrying a refusal that will never change. |

**Authentication:** an API key with `read` scope.

---

## The notes behind a theme

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/themes/{theme_id}/examples</code></div>

The sentences that put traces in this theme, each with the run it was written about, so a reader can go from a theme to the runs behind it.

Ordered by how much each note's wording overlaps the theme's label, most alike first, with the note id breaking a tie so the order is total and the same on every read and every replica.

Pass `noise` as the theme id to read the notes that joined no theme in this snapshot. There is no separate noise route: noise is a node in the flow and its notes are read here, because a second pair of routes returning an identically shaped answer would be two more rows in this reference and no new question answered.

The text is payload. This endpoint takes a read key, it is recorded in the audit ledger like every other endpoint that hands back what a user wrote, and the notes it returns are swept by retention and erased with their subject.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `theme_id` | `string` | The theme to read, or `noise` for the notes in this snapshot that joined no theme. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |
| `entity_kind` | `EntityKind` | **required** | The kind of entity to read. An entity is the pair of a kind and a name, which is the identity spans already carry. |
| `entity_name` | `string` | **required** | The name of that entity. |
| `aspect` | `string` | **required** | Which aspect the theme was clustered along. |
| `snapshot_id` | `string` | **required** | Which snapshot to read it in. |
| `filter` | `string[]` | · | Drill in. Repeat as `filter=aspect:theme_id`, or `filter=aspect:noise`, up to four times with a different aspect each time. The daemon recomputes the counts and the shares over the traces that survive every filter, so the answer does not change shape with the size of the snapshot and a client never has to fetch the assignments and filter them itself. |
| `limit` | `u32` | `50` | Page size, 1 to 100. |
| `cursor` | `string` | · | Opaque cursor from a previous page. |

**Response:** `ThemeExampleListResponse`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No such snapshot, or a theme id this entity has never had. |
| `501` | · | This deployment's storage backend does not implement what this endpoint needs. The body names the capability in its `capability` field, using the same word `GET /v1/api/capabilities` publishes as `theme_analysis`, so a caller can read the capability set once and stop asking rather than retrying a refusal that will never change. |

**Authentication:** an API key with `read` scope.

---

## One theme's life across snapshots

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/themes/{theme_id}/history</code></div>

Every stored snapshot that holds this theme, with its state, counts and coverage at each point, and the themes it split from, split into, merged from or merged into.

This is the only theme route that names no snapshot, because the series is the answer. Points come back oldest first, so nothing has to be reordered before it is drawn or differenced.

Retention bounds what can be answered. The oldest point returned is the oldest snapshot still stored, which is a lower bound on the theme's age and not necessarily its birth, so a series that starts at the edge of the window says when mcpd started looking rather than when the theme started.

Windows overlap, so read the points as a series and never add their counts together.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `theme_id` | `string` | The theme to follow. |

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | **required** | Which project to read. Required. |
| `entity_kind` | `EntityKind` | **required** | The kind of entity to read. An entity is the pair of a kind and a name, which is the identity spans already carry. |
| `entity_name` | `string` | **required** | The name of that entity. |
| `aspect` | `string` | **required** | Which aspect the theme was clustered along. |
| `limit` | `u32` | `100` | Page size, 1 to 500. |
| `cursor` | `string` | · | Opaque cursor from a previous page. |

**Response:** `ThemeHistory`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | A theme id this entity has never had. |
| `501` | · | This deployment's storage backend does not implement what this endpoint needs. The body names the capability in its `capability` field, using the same word `GET /v1/api/capabilities` publishes as `theme_analysis`, so a caller can read the capability set once and stop asking rather than retrying a refusal that will never change. |

**Authentication:** an API key with `read` scope.

---

## What was written about one trace

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/traces/{trace_id}/aspects</code></div>

Every aspect note recorded about one trace, in catalogue order. A trace id crosses service boundaries, so two projects can each hold a run under it: name the project to read that one. Without it, the projects the key reaches are tried in order and the first that holds the trace answers.

Read it beside the trace itself. A note is the sentence that put this run in each of its themes, so it is the shortest route from a theme back to what happened, and it is also the place to check a theme you do not believe: a note proves a definition said this about the trace at a version, and the trace proves the rest.

An empty list is the ordinary answer for a recent trace. Writing a note lags ingestion.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `trace_id` | `string` | The trace to read notes for. |
| `project` | `string` | The project whose run of this trace to read. Left out, the first project the key reaches that holds the trace. |

**Response:** `TraceAspectNotes`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No trace with that id in a project this key reaches. |
| `501` | · | This deployment's storage backend does not implement what this endpoint needs. The body names the capability in its `capability` field, using the same word `GET /v1/api/capabilities` publishes as `theme_analysis`, so a caller can read the capability set once and stop asking rather than retrying a refusal that will never change. |

**Authentication:** an API key with `read` scope.

---
