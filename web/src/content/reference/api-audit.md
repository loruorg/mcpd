---
title: "Audit"
description: "The audit endpoints of the mcpd HTTP API."
order: 12
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## Read the audit log

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/audit</code></div>

Every recorded read of stored payloads and every deletion, newest first. Writing an audit row never fails the request it describes: a row that cannot be written is logged and counted as `mcpd_audit_write_failures_total` on `/metrics`, and the read still returns.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `project` | `string` | · | Which project to read. Defaults to the account's own project for an account credential, else the daemon's default project. |
| `action` | `string` | · | Only rows recording this action, such as `delete_trace`. |
| `from` | `timestamp` | · | Start of the window. |
| `to` | `timestamp` | · | End of the window. |
| `limit` | `u32` | `100` | Page size, 1 to 1000. |

**Response:** `AuditEvent`

**Authentication:** an API key with `admin` scope.

---
