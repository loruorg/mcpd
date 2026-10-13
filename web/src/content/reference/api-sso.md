---
title: "SSO"
description: "The sso endpoints of the mcpd HTTP API."
order: 21
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## Read SSO settings

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/orgs/{org_id}/sso</code></div>

The organisation's single sign-on settings, without the client secret. Admin. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Response:** `SsoConfig`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | No settings are stored, or the organisation is not one you belong to. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Set SSO settings

<div class="endpoint"><span class="method method--put">PUT</span><code>/v1/api/orgs/{org_id}/sso</code></div>

Stores single sign-on settings. For `oidc` the issuer's discovery document is fetched and checked before anything is stored. Admin. Audited as `sso_update`. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Request body:** `PutSsoConfigRequest`

**Response:** `SsoConfig`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `400` | · | A setting is invalid, or the discovery document could not be read. |
| `403` | · | Your role does not allow it, or a domain is not yours to claim. |
| `409` | · | Another organisation claims a domain, or the settings come from the configuration file. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Remove SSO settings

<div class="endpoint"><span class="method method--delete">DELETE</span><code>/v1/api/orgs/{org_id}/sso</code></div>

Removes single sign-on and releases the claimed domains. Admin. Audited as `sso_delete`. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Response:** `DeleteResult`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Read the SCIM token status

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/orgs/{org_id}/scim-token</code></div>

Whether a SCIM token exists, and the base URL. Admin. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Response:** `ScimTokenInfo`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Create a SCIM token

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/orgs/{org_id}/scim-token</code></div>

Creates a SCIM bearer token and revokes any earlier one. Admin. Audited as `scim_token_rotate`. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Response:** `CreatedScimToken`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Revoke the SCIM token

<div class="endpoint"><span class="method method--delete">DELETE</span><code>/v1/api/orgs/{org_id}/scim-token</code></div>

Revokes the SCIM token. Admin. Audited as `scim_token_revoke`. Answers 404 when the daemon has no accounts or the organisation is not one you belong to.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `org_id` | `string` | The organisation's `id`. |

**Response:** `DeleteResult`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Start single sign-on

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/sso/start</code></div>

Finds the organisation by email domain or id and returns the provider URL to send the browser to. State, nonce and PKCE verifier are kept by the daemon for ten minutes. Limited per client address. Answers 404 when the daemon has no accounts.

**Request body:** `SsoStartRequest`

**Response:** `SsoStart`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `400` | · | No organisation logs in with single sign-on for that domain or id. |

**Authentication:** none; this endpoint is public.

---

## Finish single sign-on

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/sso/callback</code></div>

Exchanges the code, checks the ID token's signature, issuer, audience, expiry and nonce, and logs in. A member is found by their identity at the provider; a new one is created only for an address in a claimed domain, and an existing account that is not a member is never taken over. The session reaches this organisation's projects only and lasts 24 hours. Audited as `sso_login`. Limited per client address. Answers 404 when the daemon has no accounts.

**Request body:** `SsoCallbackRequest`

**Response:** `AccountSession`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `400` | · | The state is unknown or expired, the binding does not match, or the provider refused the code. |
| `403` | · | The provider's answer does not identify a member this organisation may log in. |

**Authentication:** none; this endpoint is public.

---
