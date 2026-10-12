---
title: "Ingest"
description: "The ingest endpoints of the mcpd HTTP API."
order: 2
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## Receive OpenTelemetry traces

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/traces</code></div>

The standard OTLP/HTTP trace endpoint. Accepts protobuf (`application/x-protobuf`) and JSON (`application/json`), gzip-encoded or uncompressed. Point any OpenTelemetry SDK here and mcpd will record its spans, mapping GenAI and OpenInference semantic conventions onto mcpd's own model. This is the endpoint that makes mcpd framework-agnostic: nothing about it requires an mcpd SDK.

**Request body:** `Otlp.ExportTraceServiceRequest`

**Response:** `Otlp.ExportTraceServiceResponse`

**Authentication:** an API key with `ingest` scope.

---

## Receive OpenTelemetry logs

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/logs</code></div>

The standard OTLP/HTTP log endpoint. Records correlate to spans automatically through their trace and span ids.

**Request body:** `Otlp.ExportLogsServiceRequest`

**Response:** `Otlp.ExportLogsServiceResponse`

**Authentication:** an API key with `ingest` scope.

---

## Receive OpenTelemetry metrics

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/metrics</code></div>

The standard OTLP/HTTP metric endpoint, including exemplars, which mcpd turns into links back to the exact span that produced a data point.

**Request body:** `Otlp.ExportMetricsServiceRequest`

**Response:** `Otlp.ExportMetricsServiceResponse`

**Authentication:** an API key with `ingest` scope.

---

## Receive mcpd signals directly

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/ingest</code></div>

Accepts mcpd's own shapes for all five signals in one batch, posted as a single `IngestBatch` object, gzip-encoded or uncompressed. Use this when you want the full model without translating through OTLP: token detail, cost, scores and feedback all survive intact. Supply `ingest_key` on a batch to make retries idempotent.

**Request body:** `IngestBatch`

**Response:** `IngestResult`

**Authentication:** an API key with `ingest` scope.

---
