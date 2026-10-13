---
title: "Ingest"
description: "The ingest endpoints of the mcpd HTTP API."
order: 2
generated: true
---

<!-- GENERATED FILE. DO NOT EDIT.  Produced from spec/mcpd.spec.json by tools/codegen. Edit the spec and run `make gen`. `make check` fails if this file drifts. -->


## Receive OpenTelemetry traces

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/traces</code></div>

The standard OTLP/HTTP trace endpoint. Accepts protobuf (`application/x-protobuf`) and JSON (`application/json`), gzip-encoded or uncompressed, and answers in the encoding it was sent. Point any OpenTelemetry SDK here and mcpd will record its spans, mapping GenAI and OpenInference semantic conventions onto mcpd's own model. Span links and span events are kept. This is the endpoint that makes mcpd framework-agnostic: nothing about it requires an mcpd SDK. The same port also serves OTLP over gRPC.

**Request body:** `Otlp.ExportTraceServiceRequest`

**Response:** `Otlp.ExportTraceServiceResponse`

**Authentication:** an API key with `ingest` scope.

---

## Receive OpenTelemetry logs

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/logs</code></div>

The standard OTLP/HTTP log endpoint. Accepts protobuf (`application/x-protobuf`) and JSON (`application/json`), gzip-encoded or uncompressed, and answers in the encoding it was sent. Records correlate to spans automatically through their trace and span ids, and a resent export is recognised and stored once.

**Request body:** `Otlp.ExportLogsServiceRequest`

**Response:** `Otlp.ExportLogsServiceResponse`

**Authentication:** an API key with `ingest` scope.

---

## Receive OpenTelemetry metrics

<div class="endpoint"><span class="method method--post">POST</span><code>/v1/metrics</code></div>

The standard OTLP/HTTP metric endpoint. Accepts protobuf (`application/x-protobuf`) and JSON (`application/json`), gzip-encoded or uncompressed, and answers in the encoding it was sent. Exemplars are kept, and mcpd turns them into links back to the exact span that produced a data point. A resent export is recognised and stored once.

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

## Receive OpenTelemetry traces over gRPC

<div class="endpoint"><span class="method method--post">POST</span><code>/opentelemetry.proto.collector.trace.v1.TraceService/Export</code></div>

The OTLP/gRPC trace service, on the same port as everything else. The listener takes cleartext HTTP/2 beside HTTP/1.1, so an exporter set to `grpc` needs only this daemon's address. Send the key as `authorization: Bearer <key>` metadata. Messages may be gzip-compressed. Every span takes the same road as one sent to `POST /v1/traces`.

**Request body:** `Otlp.ExportTraceServiceRequest`

**Response:** `Otlp.ExportTraceServiceResponse`

**Authentication:** an API key with `ingest` scope.

---

## Receive OpenTelemetry logs over gRPC

<div class="endpoint"><span class="method method--post">POST</span><code>/opentelemetry.proto.collector.logs.v1.LogsService/Export</code></div>

The OTLP/gRPC logs service, on the same port as everything else. Send the key as `authorization: Bearer <key>` metadata. Messages may be gzip-compressed. Every record takes the same road as one sent to `POST /v1/logs`.

**Request body:** `Otlp.ExportLogsServiceRequest`

**Response:** `Otlp.ExportLogsServiceResponse`

**Authentication:** an API key with `ingest` scope.

---

## Receive OpenTelemetry metrics over gRPC

<div class="endpoint"><span class="method method--post">POST</span><code>/opentelemetry.proto.collector.metrics.v1.MetricsService/Export</code></div>

The OTLP/gRPC metrics service, on the same port as everything else. Send the key as `authorization: Bearer <key>` metadata. Messages may be gzip-compressed. Every data point takes the same road as one sent to `POST /v1/metrics`.

**Request body:** `Otlp.ExportMetricsServiceRequest`

**Response:** `Otlp.ExportMetricsServiceResponse`

**Authentication:** an API key with `ingest` scope.

---

## Receive Zipkin spans

<div class="endpoint"><span class="method method--post">POST</span><code>/api/v2/spans</code></div>

The Zipkin v2 span endpoint. Accepts a JSON array of spans (`application/json`) or a proto3 `ListOfSpans` (`application/x-protobuf`), gzip-encoded or uncompressed, so a Zipkin reporter needs only this daemon's address. `localEndpoint.serviceName` becomes the service, tags become attributes, `kind` becomes the `span.kind` attribute, annotations become span events, and the `error` tag sets the status. A shared server span is stored under an id derived from the one it shares, parented to it, so both halves are kept. Answers `202` with the counts.

**Request body:** `Zipkin.ListOfSpans`

**Response:** `IngestResult`, status `202`

**Authentication:** an API key with `ingest` scope.

---

## Receive Jaeger spans

<div class="endpoint"><span class="method method--post">POST</span><code>/api/traces</code></div>

The Jaeger collector's HTTP endpoint. Takes one `jaeger.Batch` in Thrift's binary protocol (`application/x-thrift`), gzip-encoded or uncompressed. The process becomes the service and its tags the resource, span tags become attributes, logs become span events, and references become the parent and span links: the first `CHILD_OF` in the same trace is the parent, and every other reference is a link whose kind is `child_of` or `follows_from`. Answers `202` with the counts.

**Request body:** `Jaeger.Batch`

**Response:** `IngestResult`, status `202`

**Authentication:** an API key with `ingest` scope.

---

## Receive Jaeger spans over gRPC

<div class="endpoint"><span class="method method--post">POST</span><code>/jaeger.api_v2.CollectorService/PostSpans</code></div>

The Jaeger collector's gRPC service, on the same port as everything else. Send the key as `authorization: Bearer <key>` metadata. Messages may be gzip-compressed. Spans are mapped exactly as `POST /api/traces` maps them.

**Request body:** `Jaeger.PostSpansRequest`

**Response:** `Jaeger.PostSpansResponse`

**Authentication:** an API key with `ingest` scope.

---
