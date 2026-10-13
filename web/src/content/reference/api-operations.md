---
title: "Operations"
description: "The operations endpoints of the mcpd HTTP API."
order: 12
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## Liveness

<div class="endpoint"><span class="method method--get">GET</span><code>/healthz</code></div>

Returns 200 as long as the process is running. Wire this to a Kubernetes liveness probe. It never touches storage, so a slow database cannot cause a restart loop.

**Response:** `HealthStatus`

**Authentication:** none; this endpoint is public.

---

## Readiness

<div class="endpoint"><span class="method method--get">GET</span><code>/readyz</code></div>

Returns 200 once storage is reachable and migrations have completed. Wire this to a Kubernetes readiness probe so traffic is not sent to an instance that cannot serve it. On `mcpd relay` it reports the spool and the upstreams instead: 503 when the spool is full, or when no upstream has answered for 30 seconds and the spool is at least 80% full, and `degraded` with a 200 while it is spooling for an upstream it cannot reach.

**Response:** `HealthStatus`

**Authentication:** none; this endpoint is public.

---

## Prometheus metrics about mcpd itself

<div class="endpoint"><span class="method method--get">GET</span><code>/metrics</code></div>

mcpd's own counters in Prometheus text format: signals accepted, rejected, over quota and unbounded, duplicate batches, traces sampled out, ingest and query requests handled, request errors, audit rows that could not be written, each forwarding destination's deliveries, retries, dead letters and spool depth, and uptime. An observability tool that cannot be observed is a liability.

**Response:** `String`

**Authentication:** none; this endpoint is public.

---

## What this deployment's storage backend can do

<div class="endpoint"><span class="method method--get">GET</span><code>/v1/api/capabilities</code></div>

The capability set of the store behind this daemon. mcpd has one storage trait and three backends, and they differ: discovery, delta polling, per-signal retention and case folding outside ASCII are not on all three. Read this once and offer only what the deployment answers, rather than discovering a difference from an empty list. An endpoint whose capability is false refuses with 501 and names the same field, so nothing here can disagree with what the daemon actually does.

**Response:** `StoreCapabilities`

**Authentication:** an API key with `read` scope.

---
