---
title: "Accounts"
description: "The accounts endpoints of the mcpd HTTP API."
order: 18
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## Create an account

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/signup</code></div>

Starts an account. The answer is the same whether or not the email already has an account, so it does not say which addresses are in use. A link is sent to the address: for a new account it confirms the address and logs in; for an address already in use it says so and offers a password reset. The account and its project are created now, and the session comes from opening the link. Limited per client address: a client that signs up too often is answered with 429 and a `Retry-After` header. Answers 404 when the daemon was not started with `--signup`, and 503 when it has no way to send mail.

**Request body:** `AccountCredentials`

**Response:** `AccountNotice`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `400` | · | The email is not an address, or the password is shorter than 10 characters or longer than 1024 bytes. |
| `404` | · | This daemon does not offer sign-up. |
| `429` | · | Too many sign-ups from this address. Wait for the number of seconds in `Retry-After`. |
| `503` | · | This daemon has no mail server, so it cannot send the link. The operator sets `--smtp-host`. |

**Authentication:** none; this endpoint is public.

---

## Log in

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/login</code></div>

Starts a session for an account. A wrong password and an unknown email get the same 401 with the same message, so the answer does not say which emails have accounts. With the right password, an account with two-step log-in on and no `totp_code` gets 401 with the code `totp_required`; an address not yet confirmed gets 403 and a new link; and a member of an organisation that enforces single sign-on gets 403 with the code `sso_required`. Limited per client address and per email. Answers 404 when the daemon has no accounts.

**Request body:** `AccountCredentials`

**Response:** `AccountSession`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `401` | · | The email and password do not match an account. |
| `403` | · | The password is right, and the address is not confirmed yet or the organisation requires single sign-on. |
| `404` | · | This daemon does not offer accounts. |
| `429` | · | Too many attempts from this address or for this email. Wait for the number of seconds in `Retry-After`. |

**Authentication:** none; this endpoint is public.

---

## Log out

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/logout</code></div>

Ends the session whose token the request carries. The account's API keys keep working. The body is empty. Answers 404 when the daemon has no accounts.

**Response:** `DeleteResult`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Read the logged-in account

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/account</code></div>

The account whose session the request carries. Answers 404 when the daemon has no accounts.

**Response:** `Account`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## List the account's API keys

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/account/keys</code></div>

Every API key the account holds, on every project, newest first. No secret is returned: the daemon keeps only digests. Answers 404 when the daemon has no accounts.

**Response:** `AccountKey`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Create an API key

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/keys</code></div>

Creates an API key with one scope on one project: the account's own project, or a project of an organisation it belongs to. Your role there must allow the scope, and the key stops working if your role stops allowing it. The secret is in this response and nowhere else. A signal the key writes that names another project is stored in the key's project instead. An account holds at most 50 keys. Answers 404 when the daemon has no accounts.

**Request body:** `CreateAccountKeyRequest`

**Response:** `CreatedAccountKey`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `400` | · | The scope is not one of the six, the name is too long, or the account already holds 50 keys. |
| `403` | · | Your role on that project does not allow the scope. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Revoke an API key

<div class="endpoint"><span class="method method--delete">DELETE</span><code>/v1/api/account/keys/{key_id}</code></div>

Revokes one of the account's API keys. It stops working at once. Answers 404 when the daemon has no accounts.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `key_id` | `string` | The key's `id`, as listed. |

**Response:** `DeleteResult`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | The account holds no key with that id. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Confirm an email address

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/verify</code></div>

Confirms the address with the token from the sign-up link, and logs in. The token works once, for 24 hours. Answers 404 when the daemon has no accounts.

**Request body:** `TokenRequest`

**Response:** `AccountSession`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `400` | · | The token is unknown, used or expired. |

**Authentication:** none; this endpoint is public.

---

## Ask for a password reset

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/password/forgot</code></div>

Sends a reset link to the address when an account uses it. The answer is the same either way. Limited per client address and per email. Answers 404 when the daemon has no accounts.

**Request body:** `PasswordForgotRequest`

**Response:** `AccountNotice`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `503` | · | This daemon has no mail server. |

**Authentication:** none; this endpoint is public.

---

## Reset a password

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/password/reset</code></div>

Sets a new password with the token from a reset link, ends every other session, confirms the address and logs in. The token works once, for one hour. Answers 404 when the daemon has no accounts.

**Request body:** `PasswordResetRequest`

**Response:** `AccountSession`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `400` | · | The token is unknown, used or expired, or the password is too short. |
| `401` | · | The account has two-step log-in on and the code is missing or wrong. The code is `totp_required`, and the link still works. |

**Authentication:** none; this endpoint is public.

---

## Change the password

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/password</code></div>

Sets a new password and ends every other session of the account. Answers 404 when the daemon has no accounts.

**Request body:** `PasswordChangeRequest`

**Response:** `AccountNotice`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `403` | · | The current password is wrong. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Change the email address

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/email</code></div>

Sends a confirmation link to the new address, and a notice to the old one. The address changes when the link is opened. The answer is the same whether or not the new address is in use. Answers 404 when the daemon has no accounts.

**Request body:** `EmailChangeRequest`

**Response:** `AccountNotice`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `403` | · | The password is wrong. |
| `503` | · | This daemon has no mail server. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Confirm a new email address

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/email/confirm</code></div>

Changes the address with the token from the confirmation link. The token works once, for 24 hours. Answers 404 when the daemon has no accounts.

**Request body:** `TokenRequest`

**Response:** `Account`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `400` | · | The token is unknown, used or expired. |
| `409` | · | Another account took the address in the meantime. |

**Authentication:** none; this endpoint is public.

---

## Delete the account

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/delete</code></div>

Deletes the account, its keys and its sessions. Each organisation it is the only member of is deleted too, and the stored signals and runs of that organisation's projects are erased. Refused while the account is the only owner of an organisation that has other members: make another member an owner first. Answers 404 when the daemon has no accounts.

**Request body:** `AccountDeleteRequest`

**Response:** `AccountDeletion`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `403` | · | The password or the code is wrong. |
| `409` | · | The account is the only owner of an organisation with other members. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## List sessions

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/account/sessions</code></div>

Every live session of the account, newest first. Answers 404 when the daemon has no accounts.

**Response:** `SessionInfo`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## End a session

<div class="endpoint"><span class="method method--delete">DELETE</span><code>/v1/api/account/sessions/{session_id}</code></div>

Ends one session of the account. It stops working at once. Answers 404 when the daemon has no accounts.

**Path parameters**

| Name | Type | Description |
| --- | --- | --- |
| `session_id` | `string` | The session's `id`, as listed. |

**Response:** `DeleteResult`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `404` | · | The account has no session with that id. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Start two-step log-in

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/totp/setup</code></div>

Makes a new authenticator secret. Two-step log-in is not on until a code from it is confirmed with `POST /v1/api/account/totp/enable`. Answers 404 when the daemon has no accounts.

**Response:** `TotpSetup`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `409` | · | Two-step log-in is already on. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Turn two-step log-in on

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/totp/enable</code></div>

Confirms the secret from setup with a code from the app, turns two-step log-in on and returns ten recovery codes. Answers 404 when the daemon has no accounts.

**Request body:** `TotpCodeRequest`

**Response:** `TotpEnabled`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `403` | · | The code is wrong. |
| `409` | · | There is no secret waiting to be confirmed, or two-step log-in is already on. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Turn two-step log-in off

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/totp/disable</code></div>

Turns two-step log-in off and drops the recovery codes. Answers 404 when the daemon has no accounts.

**Request body:** `TotpDisableRequest`

**Response:** `AccountNotice`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `403` | · | The password or the code is wrong. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---
