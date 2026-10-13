---
title: "Monitors"
description: "The monitors endpoints of the mcpd HTTP API."
order: 17
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## List monitors

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/monitors</code></div>

Every configured monitor the key reaches, with the condition it is latched in and its most recent reading. A monitor names the project it watches and carries the webhook it posts to, so the list is filtered by the key's project grant rather than returned whole. Monitors are configured in the daemon's configuration file, so there is nothing to create here.

**Response:** `MonitorState`

**Authentication:** an API key with `read` scope.

---

## Acknowledge a monitor

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/monitors/{id}/ack</code></div>

Acknowledges a tripped monitor. Acknowledging silences the transition without pretending the condition cleared: the monitor stays `acknowledged` while it still breaches, and returns to `nominal` on its own when the number recovers. Acknowledging a monitor that is not tripped changes nothing. The body is empty. A key that does not reach the project the monitor watches is refused with `403`, naming the monitor rather than the project that owns it.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `id` | `string` | The monitor's identifier. |

**Response:** `MonitorState`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No monitor with that id. |

**Authentication:** an API key with `write` scope.

---

## List objectives

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/slos</code></div>

Every configured service level objective the key reaches, filtered by the key's project grant the way the monitor list is. Objectives are configured in the daemon's configuration file, so there is nothing to create here.

**Response:** `SloState`

**Authentication:** an API key with `read` scope.

---

## Acknowledge an objective

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/slos/{id}/ack</code></div>

Acknowledges a tripped objective, under the same latching rule as a monitor: it stays `acknowledged` while the burn rate still breaches, and returns to `nominal` when it recovers. The body is empty. The project grant is checked the way it is on a monitor acknowledgement.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `id` | `string` | The objective's identifier. |

**Response:** `SloState`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No objective with that id. |

**Authentication:** an API key with `write` scope.

---
