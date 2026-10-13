---
title: "Organisations"
description: "The organisations endpoints of the mcpd HTTP API."
order: 19
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## List your organisations

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/orgs</code></div>

Every organisation the account belongs to, with its role in each. A single sign-on session lists only the organisation it is limited to. Answers 404 when the daemon has no accounts.

**Response:** `Org`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Create an organisation

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/orgs</code></div>

Creates an organisation with one project, and makes you its owner. With `--signup` any account may create one; without it, only when the configuration file allows it. An account belongs to at most 20 organisations. Answers 404 when the daemon has no accounts.

**Request body:** `CreateOrgRequest`

**Response:** `Org`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `403` | · | This daemon does not let accounts create organisations, or the account belongs to 20 already. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Read an organisation

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/orgs/{org_id}</code></div>

Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Response:** `Org`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Change an organisation

<div class="endpoint"><span class="method method--patch">PATCH</span><code>/v1/api/orgs/{org_id}</code></div>

Renames an organisation, or sets its ceiling and retention within the daemon's limits. Admin to rename, owner for the rest. Audited as `org_update`. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Request body:** `UpdateOrgRequest`

**Response:** `Org`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `403` | · | Your role does not allow the change. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Delete an organisation

<div class="endpoint"><span class="method method--delete">DELETE</span><code>/v1/api/orgs/{org_id}</code></div>

Deletes an organisation, its members' access, its keys, its invitations and its single sign-on and SCIM settings, and erases the stored signals and runs of every project it owns. Owner only. Audited as `org_delete`. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Response:** `OrgDeletion`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `403` | · | Only an owner may delete the organisation. |
| `409` | · | The organisation is declared in the configuration file, or one of its projects is an account's own project. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## List members

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/orgs/{org_id}/members</code></div>

Every member, with their roles. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Response:** `OrgMember`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Change a member's role

<div class="endpoint"><span class="method method--patch">PATCH</span><code>/v1/api/orgs/{org_id}/members/{account_id}</code></div>

Sets a member's organisation role. Admin, and owner to grant or remove `owner`. The last owner cannot be demoted. Keys the member made that the new role no longer allows stop working. Audited as `member_role`. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |
| `account_id` | `string` | The member's `account_id`. |

**Request body:** `UpdateMemberRequest`

**Response:** `OrgMember`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `403` | · | Your role does not allow the change. |
| `409` | · | This would leave the organisation with no owner. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Remove a member

<div class="endpoint"><span class="method method--delete">DELETE</span><code>/v1/api/orgs/{org_id}/members/{account_id}</code></div>

Removes a member, or yourself. Their keys on the organisation's projects are revoked and their single sign-on sessions for it end. Admin to remove others; owner to remove an owner. The last owner cannot leave. Audited as `member_remove`. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |
| `account_id` | `string` | The member's `account_id`. |

**Response:** `DeleteResult`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `403` | · | Your role does not allow it. |
| `409` | · | This would leave the organisation with no owner. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Set a project role

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/orgs/{org_id}/members/{account_id}/project-roles</code></div>

Sets or clears a member's role on one project. Admin. Audited as `member_project_role`. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |
| `account_id` | `string` | The member's `account_id`. |

**Request body:** `SetProjectRoleRequest`

**Response:** `OrgMember`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `403` | · | Your role does not allow it. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## List invitations

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/orgs/{org_id}/invites</code></div>

Every invitation not yet accepted or expired. Admin. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Response:** `OrgInvite`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Invite a member

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/orgs/{org_id}/invites</code></div>

Sends an invitation link to an address. It works once, for seven days. Admin, and owner to invite an owner. Audited as `member_invite`. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Request body:** `CreateInviteRequest`

**Response:** `OrgInvite`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `403` | · | Your role does not allow it. |
| `409` | · | That address is already a member. |
| `503` | · | This daemon has no mail server. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Revoke an invitation

<div class="endpoint"><span class="method method--delete">DELETE</span><code>/v1/api/orgs/{org_id}/invites/{invite_id}</code></div>

Revokes an invitation. Admin. Audited as `invite_revoke`. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |
| `invite_id` | `string` | The invitation's `id`. |

**Response:** `DeleteResult`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Read an invitation

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/invites/lookup</code></div>

What an invitation offers, from the token in its link. Limited per client address. Answers 404 when the daemon has no accounts.

**Request body:** `TokenRequest`

**Response:** `InviteLookup`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `400` | · | The token is unknown, used or expired. |

**Authentication:** none; this endpoint is public.

---

## Accept an invitation

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/invites/accept</code></div>

Joins the organisation. When an account uses the invited address, send that account's session token with the request. When none does, send a password: the account is created with the address confirmed, and logged in. Audited as `invite_accept`. Limited per client address. Answers 404 when the daemon has no accounts.

**Request body:** `InviteAcceptRequest`

**Response:** `InviteAccepted`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `400` | · | The token is unknown, used or expired, or a password is needed and missing. |
| `401` | · | An account uses the invited address, and the request carries no session for it. |

**Authentication:** none; this endpoint is public.

---

## List projects

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/orgs/{org_id}/projects</code></div>

The projects the organisation owns that you can reach. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Response:** `OrgProject`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Create a project

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/orgs/{org_id}/projects</code></div>

Creates a project with a random name in the organisation. Admin. An organisation owns at most 100 projects. Audited as `project_create`. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Request body:** `CreateProjectRequest`

**Response:** `OrgProject`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `403` | · | Your role does not allow it, or the organisation owns 100 projects already. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Delete a project

<div class="endpoint"><span class="method method--delete">DELETE</span><code>/v1/api/orgs/{org_id}/projects/{project}</code></div>

Deletes a project, revokes its keys, and erases its stored signals and runs. Admin. Audited as `project_delete`. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |
| `project` | `string` | The project name. |

**Response:** `OrgDeletion`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `403` | · | Your role does not allow it. |
| `409` | · | The project is an account's own project, or the organisation's last. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## List the organisation's keys

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/orgs/{org_id}/keys</code></div>

Every API key on the organisation's projects, whoever made it. Admin. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Response:** `AccountKey`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Revoke a key in the organisation

<div class="endpoint"><span class="method method--delete">DELETE</span><code>/v1/api/orgs/{org_id}/keys/{key_id}</code></div>

Revokes any key on the organisation's projects. Admin. Audited as `org_key_revoke`. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |
| `key_id` | `string` | The key's `id`. |

**Response:** `DeleteResult`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Read usage

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/orgs/{org_id}/usage</code></div>

Signals stored and refused against the organisation's ceiling. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Response:** `OrgUsage`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Read the organisation's audit log

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/orgs/{org_id}/audit</code></div>

Membership, role, project, single sign-on and SCIM changes, newest first, at most 500. Admin. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Response:** `AuditEvent`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Read group roles

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/orgs/{org_id}/group-roles</code></div>

How identity provider groups map onto roles. Admin. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Response:** `GroupRole`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Set group roles

<div class="endpoint"><span class="method method--put">PUT</span><code>/v1/api/orgs/{org_id}/group-roles</code></div>

Replaces the group mapping. A member single sign-on or SCIM manages gets the highest role among their groups, or the default role when none maps. Owners keep their role. Admin. Audited as `group_roles_update`. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Request body:** `GroupRole`

**Response:** `GroupRole`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `400` | · | A group maps to `owner`, or a group appears twice. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---
