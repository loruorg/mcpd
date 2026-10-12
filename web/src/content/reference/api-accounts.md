---
title: "Accounts"
description: "The accounts endpoints of the mcpd HTTP API."
order: 18
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## Create an account

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/signup</code></div>

Creates an account and a new project that belongs to it, and logs it in. The project has a random name, and no other account or key can reach it. The email is checked for shape only; there is no verification mail. Limited per client address: a client that signs up too often is answered with 429 and a `Retry-After` header. Answers 404 when the daemon was not started with `--signup`.

**Request body:** `AccountCredentials`

**Response:** `AccountSession`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `400` | · | The email is not an address, or the password is shorter than 10 characters or longer than 1024 bytes. |
| `404` | · | This daemon does not offer sign-up. |
| `409` | · | An account with this email already exists. Log in instead. |
| `429` | · | Too many sign-ups from this address. Wait for the number of seconds in `Retry-After`. |

**Authentication:** none; this endpoint is public.

---

## Log in

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/login</code></div>

Starts a session for an account. A wrong password and an unknown email get the same 401 with the same message, so the answer does not say which emails have accounts. Limited per client address and per email. Answers 404 when the daemon was not started with `--signup`.

**Request body:** `AccountCredentials`

**Response:** `AccountSession`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `401` | · | The email and password do not match an account. |
| `404` | · | This daemon does not offer accounts. |
| `429` | · | Too many attempts from this address or for this email. Wait for the number of seconds in `Retry-After`. |

**Authentication:** none; this endpoint is public.

---

## Log out

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/logout</code></div>

Ends the session whose token the request carries. The account's API keys keep working. The body is empty. Answers 404 when the daemon was not started with `--signup`.

**Response:** `DeleteResult`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Read the logged-in account

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/account</code></div>

The email and the project of the account whose session the request carries. Answers 404 when the daemon was not started with `--signup`.

**Response:** `Account`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## List the account's API keys

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/account/keys</code></div>

Every API key the account holds, newest first. No secret is returned: the daemon keeps only digests. Answers 404 when the daemon was not started with `--signup`.

**Response:** `AccountKey`

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Create an API key

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/api/account/keys</code></div>

Creates an API key with one scope on the account's project. The secret is in this response and nowhere else. A key reaches the account's project only: a signal it writes that names another project is stored in the account's project instead. An account holds at most 50 keys. Answers 404 when the daemon was not started with `--signup`.

**Request body:** `CreateAccountKeyRequest`

**Response:** `CreatedAccountKey`

**Other responses**

| Status | Body | When |
| --- | --- | --- |
| `400` | · | The scope is not one of the six, the name is too long, or the account already holds 50 keys. |

**Authentication:** an account session token from `POST /v1/api/account/login`. An API key is not accepted.

---

## Revoke an API key

<div class="endpoint"><span class="method method--delete">DELETE</span><code>/v1/api/account/keys/{key_id}</code></div>

Revokes one of the account's API keys. It stops working at once. Answers 404 when the daemon was not started with `--signup`.

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
