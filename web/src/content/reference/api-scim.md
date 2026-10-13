---
title: "SCIM"
description: "The scim endpoints of the mcpd HTTP API."
order: 22
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## Read the SCIM service provider configuration

<div class="endpoint"><span class="method method--get">GET</span><code>/scim/v2/ServiceProviderConfig</code></div>

What this SCIM service supports: PATCH and filtering on, bulk, sort, ETags and password change off. The body is SCIM 2.0 JSON (RFC 7643 and RFC 7644), sent as `application/scim+json`. Answers 404 when the daemon has no accounts.

**Response:** `Json`

**Authentication:** the organisation's SCIM token from `POST /v1/api/orgs/{org_id}/scim-token`. No other credential is accepted.

---

## List SCIM schemas

<div class="endpoint"><span class="method method--get">GET</span><code>/scim/v2/Schemas</code></div>

The User and Group schemas. The body is SCIM 2.0 JSON (RFC 7643 and RFC 7644), sent as `application/scim+json`. Answers 404 when the daemon has no accounts.

**Response:** `Json`

**Authentication:** the organisation's SCIM token from `POST /v1/api/orgs/{org_id}/scim-token`. No other credential is accepted.

---

## List SCIM resource types

<div class="endpoint"><span class="method method--get">GET</span><code>/scim/v2/ResourceTypes</code></div>

User and Group. The body is SCIM 2.0 JSON (RFC 7643 and RFC 7644), sent as `application/scim+json`. Answers 404 when the daemon has no accounts.

**Response:** `Json`

**Authentication:** the organisation's SCIM token from `POST /v1/api/orgs/{org_id}/scim-token`. No other credential is accepted.

---

## List users

<div class="endpoint"><span class="method method--get">GET</span><code>/scim/v2/Users</code></div>

The organisation's provisioned users. The body is SCIM 2.0 JSON (RFC 7643 and RFC 7644), sent as `application/scim+json`. Answers 404 when the daemon has no accounts.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `filter` | `string` | · | A SCIM filter. `eq` on `userName`, `externalId`, `emails.value` or `displayName`, and `and` between them. |
| `count` | `u32` | · | Most resources per page, at most 200. `startIndex` gives the first, from 1. |

**Response:** `Json`

**Authentication:** the organisation's SCIM token from `POST /v1/api/orgs/{org_id}/scim-token`. No other credential is accepted.

---

## Provision a user

<div class="endpoint"><span class="method method--post">POST</span><code>/scim/v2/Users</code></div>

Provisions a user. An address in a claimed domain with no account gets one, managed by the organisation. An address with an account becomes a member when that account accepts the invitation sent to it. Roles come from groups. Audited as `scim_user_create`. The body is SCIM 2.0 JSON (RFC 7643 and RFC 7644), sent as `application/scim+json`. Answers 404 when the daemon has no accounts.

**Request body:** `Json`

**Response:** `Json`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `409` | · | A user with that `userName` is already provisioned. |

**Authentication:** the organisation's SCIM token from `POST /v1/api/orgs/{org_id}/scim-token`. No other credential is accepted.

---

## Read a user

<div class="endpoint"><span class="method method--get">GET</span><code>/scim/v2/Users/{id}</code></div>

The body is SCIM 2.0 JSON (RFC 7643 and RFC 7644), sent as `application/scim+json`. Answers 404 when the daemon has no accounts.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `id` | `string` | The resource's `id`. |

**Response:** `Json`

**Authentication:** the organisation's SCIM token from `POST /v1/api/orgs/{org_id}/scim-token`. No other credential is accepted.

---

## Replace a user

<div class="endpoint"><span class="method method--put">PUT</span><code>/scim/v2/Users/{id}</code></div>

Replaces a user. `active: false` deprovisions: the membership ends, the account's sessions and keys for the organisation end, and an account the organisation manages is deactivated. Audited as `scim_user_update`. The body is SCIM 2.0 JSON (RFC 7643 and RFC 7644), sent as `application/scim+json`. Answers 404 when the daemon has no accounts.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `id` | `string` | The resource's `id`. |

**Request body:** `Json`

**Response:** `Json`

**Authentication:** the organisation's SCIM token from `POST /v1/api/orgs/{org_id}/scim-token`. No other credential is accepted.

---

## Change a user

<div class="endpoint"><span class="method method--patch">PATCH</span><code>/scim/v2/Users/{id}</code></div>

Applies PATCH operations to a user, with the same deprovisioning rule as replace. Audited as `scim_user_update`. The body is SCIM 2.0 JSON (RFC 7643 and RFC 7644), sent as `application/scim+json`. Answers 404 when the daemon has no accounts.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `id` | `string` | The resource's `id`. |

**Request body:** `Json`

**Response:** `Json`

**Authentication:** the organisation's SCIM token from `POST /v1/api/orgs/{org_id}/scim-token`. No other credential is accepted.

---

## Delete a user

<div class="endpoint"><span class="method method--delete">DELETE</span><code>/scim/v2/Users/{id}</code></div>

Deprovisions a user and forgets them. An account the organisation manages is deleted. Answers 204. Audited as `scim_user_delete`. The body is SCIM 2.0 JSON (RFC 7643 and RFC 7644), sent as `application/scim+json`. Answers 404 when the daemon has no accounts.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `id` | `string` | The resource's `id`. |

**Response:** `Json`

**Authentication:** the organisation's SCIM token from `POST /v1/api/orgs/{org_id}/scim-token`. No other credential is accepted.

---

## List groups

<div class="endpoint"><span class="method method--get">GET</span><code>/scim/v2/Groups</code></div>

The organisation's provisioned groups. The body is SCIM 2.0 JSON (RFC 7643 and RFC 7644), sent as `application/scim+json`. Answers 404 when the daemon has no accounts.

**Query parameters**

| Name | Type | Default | Description |
| --- | --- | --- | --- |
| `filter` | `string` | · | A SCIM filter. `eq` on `userName`, `externalId`, `emails.value` or `displayName`, and `and` between them. |
| `count` | `u32` | · | Most resources per page, at most 200. `startIndex` gives the first, from 1. |

**Response:** `Json`

**Authentication:** the organisation's SCIM token from `POST /v1/api/orgs/{org_id}/scim-token`. No other credential is accepted.

---

## Provision a group

<div class="endpoint"><span class="method method--post">POST</span><code>/scim/v2/Groups</code></div>

Provisions a group. Its members get the role the group maps to. Audited as `scim_group_create`. The body is SCIM 2.0 JSON (RFC 7643 and RFC 7644), sent as `application/scim+json`. Answers 404 when the daemon has no accounts.

**Request body:** `Json`

**Response:** `Json`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `409` | · | A group with that `displayName` is already provisioned. |

**Authentication:** the organisation's SCIM token from `POST /v1/api/orgs/{org_id}/scim-token`. No other credential is accepted.

---

## Read a group

<div class="endpoint"><span class="method method--get">GET</span><code>/scim/v2/Groups/{id}</code></div>

The body is SCIM 2.0 JSON (RFC 7643 and RFC 7644), sent as `application/scim+json`. Answers 404 when the daemon has no accounts.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `id` | `string` | The resource's `id`. |

**Response:** `Json`

**Authentication:** the organisation's SCIM token from `POST /v1/api/orgs/{org_id}/scim-token`. No other credential is accepted.

---

## Replace a group

<div class="endpoint"><span class="method method--put">PUT</span><code>/scim/v2/Groups/{id}</code></div>

Replaces a group and its members, and recomputes their roles. Audited as `scim_group_update`. The body is SCIM 2.0 JSON (RFC 7643 and RFC 7644), sent as `application/scim+json`. Answers 404 when the daemon has no accounts.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `id` | `string` | The resource's `id`. |

**Request body:** `Json`

**Response:** `Json`

**Authentication:** the organisation's SCIM token from `POST /v1/api/orgs/{org_id}/scim-token`. No other credential is accepted.

---

## Change a group

<div class="endpoint"><span class="method method--patch">PATCH</span><code>/scim/v2/Groups/{id}</code></div>

Adds or removes members, or renames the group, and recomputes roles. Audited as `scim_group_update`. The body is SCIM 2.0 JSON (RFC 7643 and RFC 7644), sent as `application/scim+json`. Answers 404 when the daemon has no accounts.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `id` | `string` | The resource's `id`. |

**Request body:** `Json`

**Response:** `Json`

**Authentication:** the organisation's SCIM token from `POST /v1/api/orgs/{org_id}/scim-token`. No other credential is accepted.

---

## Delete a group

<div class="endpoint"><span class="method method--delete">DELETE</span><code>/scim/v2/Groups/{id}</code></div>

Deletes a group and recomputes its members' roles. Answers 204. Audited as `scim_group_delete`. The body is SCIM 2.0 JSON (RFC 7643 and RFC 7644), sent as `application/scim+json`. Answers 404 when the daemon has no accounts.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `id` | `string` | The resource's `id`. |

**Response:** `Json`

**Authentication:** the organisation's SCIM token from `POST /v1/api/orgs/{org_id}/scim-token`. No other credential is accepted.

---
