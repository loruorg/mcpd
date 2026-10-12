//! Ingest handlers.
//!
//! Every signal, whatever door it came in by, goes through the same pipeline:
//!
//! 1. **Authorise** — an ingest key, when one is configured.
//! 2. **Decode** — OTLP/JSON, OTLP protobuf, or mcpd's native shape.
//! 3. **Sample** — decided once per trace at the root and inherited, so a trace
//!    is never half-recorded.
//! 4. **Redact** — before anything is written. A secret that never lands is a
//!    different guarantee from one deleted afterwards.
//! 5. **Cost** — priced at ingest and stored, so a later price change cannot
//!    silently rewrite history.
//! 6. **Limit**, because one project must not spend another project's disk.
//!    Signals over a project's ceiling are refused and counted in the response.
//! 7. **Store** — one transaction per batch.

use crate::auth::{authorize, Grant, Scope};
use crate::error::{ApiError, ApiResult, QuotaRefusal};
use crate::pipeline::Intake;
use crate::state::{AppState, ProjectQuotas};
use axum::body::Body;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use bytes::Bytes;
use mcpd_core::{ids, otlp, IngestBatch, IngestError, IngestResult, SignalType, Span};
use mcpd_store::Batch;
use sha2::{Digest, Sha256};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use time::OffsetDateTime;

/// Decompress a body when the sender said it was compressed.
///
/// The decompressed size is held to `max_bytes`, the same limit the body itself
/// is held to. The wire size is bounded by the body limit layer and the
/// decompressed size was bounded by nothing, and gzip compresses a run of
/// zeros a thousand to one: a 2 MiB request expanded to 2 GiB in memory before
/// a single byte of it was parsed.
fn decompress(headers: &HeaderMap, body: Bytes, max_bytes: usize) -> ApiResult<Bytes> {
    let encoding = headers
        .get("content-encoding")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    match encoding.as_str() {
        "" | "identity" => Ok(body),
        "gzip" => {
            use std::io::Read;
            // One byte past the limit is all the evidence needed, and reading
            // exactly that many is what keeps the allocation bounded.
            let mut d = flate2::read::GzDecoder::new(&body[..]).take(max_bytes as u64 + 1);
            let mut out = Vec::new();
            d.read_to_end(&mut out)
                .map_err(|e| ApiError::BadRequest(format!("gzip body could not be read: {e}")))?;
            if out.len() > max_bytes {
                return Err(ApiError::TooLarge(format!(
                    "the gzip body decompresses to more than the {max_bytes} bytes this server \
                     accepts per request; send fewer signals per request"
                )));
            }
            Ok(Bytes::from(out))
        }
        other => Err(ApiError::BadRequest(format!(
            "content-encoding {other:?} is not supported; use gzip or send the body uncompressed"
        ))),
    }
}

/// Apply sampling, redaction and costing, then persist.
///
/// Shared by every ingest door so the four steps cannot drift apart.
pub(crate) async fn finish(
    state: &Arc<AppState>,
    spans: Vec<Span>,
    batch: Batch,
    grant: &Grant,
) -> ApiResult<IngestResult> {
    finish_pinned(state, spans, batch, grant, None).await
}

/// [`finish`], with every signal first moved into `pin` when it is set.
///
/// `pin` is an account's project, from `auth::pinned_project`. The idempotency
/// key is put under the project as well: the store holds those keys in one
/// table for every project, so without it one account could send a key it
/// guessed another account would use, and that account's batch would then be
/// skipped as a replay.
pub(crate) async fn finish_pinned(
    state: &Arc<AppState>,
    spans: Vec<Span>,
    mut batch: Batch,
    grant: &Grant,
    pin: Option<&str>,
) -> ApiResult<IngestResult> {
    batch.spans = spans;
    if let Some(project) = pin {
        pin_batch(&mut batch, project);
    }

    // A project-scoped key writes to its own projects and nowhere else. A batch
    // carries a project per signal, so this is enforced here rather than at the
    // door: one request may legitimately name several, and a key that reaches
    // some of them must not have the whole batch refused or the whole batch
    // accepted.
    //
    // Refused signals are counted as rejected with a reason rather than dropped
    // silently, because a fleet misconfigured to write into the wrong project
    // should be discoverable from the response it is already reading.
    let refused = retain_authorised(&mut batch, grant);

    let intake = Intake::new(&state.redactor, &state.price_book, &state.config.sampling)
        .with_projects(&state.project_policies);
    let prepared = intake.prepare(&mut batch);
    if prepared.sampled_out > 0 {
        state.metrics.sampled_out.fetch_add(prepared.sampled_out, Ordering::Relaxed);
    }

    // After the pipeline rather than before it, so the ceiling counts what would
    // actually be stored. A project whose own sampling keeps one trace in ten
    // should get ten times the arrivals: the ceiling is there to bound the disk,
    // and charging a project for traces its sampling had already discarded would
    // make the same number mean something different on every project.
    let over_quota = enforce_quotas(&mut batch, &state.quotas);

    // An idempotency key claims that everything it names was written. Both of
    // the steps above take signals back out of the batch after the key was
    // settled, and both are conditions a sender is told to retry through: a key
    // recorded against the remainder would answer that retry with "already
    // stored" for records that were never stored at all. So the key is dropped
    // rather than made to describe a subset of what it covers.
    if refused > 0 || over_quota.cuts.iter().any(|c| c.rejected > 0) {
        batch.ingest_key = None;
    }

    // Metrics derived from the spans that survived everything above.
    //
    // Deliberately after the ceiling rather than before it. A derived point is
    // a signal in the store like any other, and charging it to the project's
    // allowance would let a feature the daemon turned on for itself evict the
    // spans a sender actually paid to send. An operator who sized a ceiling for
    // real traffic would have lost most of it on upgrade without being told.
    //
    // Also after the idempotency key is settled, because it changes nothing
    // about that: the derived ids are a function of the spans, so a resent
    // export derives the same ids and replaces the same rows.
    // Copied before the derived points are added: those restate the spans, and
    // a destination sent both would count everything twice.
    let forwarded = crate::forward::copy_of(state, &batch);
    let derived = crate::derive::derive(
        &batch.spans,
        state.config.derived_metrics,
        OffsetDateTime::now_utc(),
    );
    let derived_count = derived.len() as u32;
    batch.metrics.extend(derived);

    let mut result = state.store.ingest(batch).await?;

    // The sender is told about the batch it sent, not about the batch that was
    // written. Rows the daemon derived for itself are subtracted back out
    // before the count is reported or counted, so a client that posts six spans
    // is never answered with forty-five accepted signals and left unable to
    // reconcile the number with anything it did.
    //
    // `duplicate` needs the same correction and for a sharper reason: a batch
    // recognised as a replay is reported as its own whole length, so leaving
    // this out told a sender that resent one span that two of its signals were
    // replays. `saturating_sub` on both because at most one of them is
    // non-zero: a batch is either stored or recognised as a replay, so the
    // correction lands on whichever one carried the derived rows.
    result.accepted = result.accepted.saturating_sub(derived_count);
    result.duplicate = result.duplicate.saturating_sub(derived_count);
    if refused > 0 {
        result.rejected += refused;
        result.errors.get_or_insert_with(Vec::new).push(IngestError {
            signal: SignalType::Span,
            index: 0,
            message: format!(
                "{refused} signal(s) named a project this key does not reach and were not stored"
            ),
        });
    }
    let mut over = 0u64;
    for cut in &over_quota.cuts {
        result.rejected += cut.rejected;
        over += u64::from(cut.rejected);
        result.errors.get_or_insert_with(Vec::new).push(quota_error(cut));
    }
    if over > 0 {
        state.metrics.over_quota.fetch_add(over, Ordering::Relaxed);
    }
    if over_quota.unbounded > 0 {
        state.metrics.unbounded.fetch_add(u64::from(over_quota.unbounded), Ordering::Relaxed);
    }
    state.metrics.accepted.fetch_add(u64::from(result.accepted), Ordering::Relaxed);
    state.metrics.rejected.fetch_add(u64::from(result.rejected), Ordering::Relaxed);
    state.metrics.duplicates.fetch_add(u64::from(result.duplicate), Ordering::Relaxed);

    // After the local write succeeded, never before: see `forward::dispatch`.
    crate::forward::dispatch(state, forwarded);

    Ok(result)
}

/// `POST /v1/traces` — the standard OTLP trace endpoint.
pub async fn otlp_traces(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Body,
) -> ApiResult<Json<serde_json::Value>> {
    // The key is checked from the headers before a byte of the body is read.
    // These three doors took `Bytes`, which axum fills before the handler
    // runs, so a client with no key could make the daemon hold up to the body
    // cap per request before being told 401. `Body` defers the read to here,
    // and `collect_body` holds it to the same cap.
    let grant = authorize(&state, &headers, Scope::Ingest)?;
    state.metrics.ingest_requests.fetch_add(1, Ordering::Relaxed);
    let body = collect_body(body, state.config.max_body_bytes).await?;
    let body = decompress(&headers, body, state.config.max_body_bytes)?;
    let content_type =
        headers.get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("application/json");

    // Protobuf is the OpenTelemetry default, so accepting only JSON would mean
    // most SDKs need reconfiguring before they work. Both encodings lower into
    // the same shape and are mapped by the same code.
    let decoded = if content_type.contains("protobuf") {
        otlp::protobuf::decode_traces(&body)
            .map_err(|e| ApiError::BadRequest(format!("not a valid OTLP protobuf payload: {e}")))?
    } else {
        let value: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|e| ApiError::BadRequest(format!("body is not valid JSON: {e}")))?;
        otlp::decode_json(&value)
            .map_err(|e| ApiError::BadRequest(format!("not a valid OTLP trace payload: {e}")))?
    };

    if decoded.len() > state.config.max_batch_signals {
        return Err(ApiError::TooLarge(format!(
            "batch carries {} spans; this server accepts {} at a time",
            decoded.len(),
            state.config.max_batch_signals
        )));
    }

    let now = OffsetDateTime::now_utc();
    // `to_spans`, not one `to_span` per decoded span: an OTLP span event becomes
    // a span of its own, and whether a parent is foreign can only be answered
    // with the whole request in hand. The batch ceiling above counts what the
    // sender sent, which is what the sender can act on; the spans events add are
    // bounded by the decoder's own per-span cap and then by the project ceiling
    // that `finish` applies.
    let spans: Vec<Span> = otlp::to_spans(&decoded, &state.config.default_project, now);

    let pin = crate::auth::pinned_project(&state, &headers);
    let batch = Batch { ingest_key: crate::relay::idempotency_key(&headers), ..Batch::default() };
    let result = finish_pinned(&state, spans, batch, &grant, pin.as_deref()).await?;
    // OTLP's own success shape. Returning `partialSuccess` with a count is how
    // a collector tells a sender it dropped something, and senders read it.
    Ok(Json(serde_json::json!({
        "partialSuccess": {
            "rejectedSpans": result.rejected,
            "errorMessage": ""
        }
    })))
}

/// `POST /v1/logs` — the standard OTLP log endpoint.
pub async fn otlp_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Body,
) -> ApiResult<Json<serde_json::Value>> {
    let grant = authorize(&state, &headers, Scope::Ingest)?;
    state.metrics.ingest_requests.fetch_add(1, Ordering::Relaxed);
    let body = collect_body(body, state.config.max_body_bytes).await?;
    let body = decompress(&headers, body, state.config.max_body_bytes)?;
    let value: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::BadRequest(format!("body is not valid JSON: {e}")))?;

    let logs = crate::ingest::otlp_logs_decode(&value, &state.config.default_project)?;
    // The same ceiling the trace door applies, and for the same reason. Without
    // it this door was bounded by bytes alone, so one body inside the byte cap
    // could carry as many records as fit in it, and every one of them was a
    // stored row. Counted after decoding, because OTLP nests its records three
    // deep and the count is not knowable before.
    if logs.len() > state.config.max_batch_signals {
        return Err(ApiError::TooLarge(format!(
            "batch carries {} log records; this server accepts {} at a time",
            logs.len(),
            state.config.max_batch_signals
        )));
    }
    let ingest_key = crate::relay::idempotency_key(&headers)
        .or_else(|| export_key("otlp-logs", logs.iter().map(|l| l.log_id.as_str())));
    let result = finish_pinned(
        &state,
        Vec::new(),
        Batch { logs, ingest_key, ..Batch::default() },
        &grant,
        crate::auth::pinned_project(&state, &headers).as_deref(),
    )
    .await?;
    Ok(Json(serde_json::json!({
        "partialSuccess": { "rejectedLogRecords": result.rejected, "errorMessage": "" }
    })))
}

/// Decode an OTLP/JSON `ExportLogsServiceRequest`.
fn otlp_logs_decode(
    body: &serde_json::Value,
    default_project: &str,
) -> ApiResult<Vec<mcpd_core::LogRecord>> {
    use mcpd_core::{Correlation, LogLevel, LogRecord};
    let resource_logs = body
        .get("resourceLogs")
        .or_else(|| body.get("resource_logs"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| ApiError::BadRequest("missing resourceLogs".to_owned()))?;

    let now = OffsetDateTime::now_utc();
    let mut out = Vec::new();
    for rl in resource_logs {
        let service = rl
            .get("resource")
            .and_then(|r| r.get("attributes"))
            .and_then(|a| a.as_array())
            .and_then(|list| {
                list.iter()
                    .find(|kv| kv.get("key").and_then(|k| k.as_str()) == Some("service.name"))
            })
            .and_then(|kv| kv.get("value"))
            .and_then(|v| v.get("stringValue"))
            .and_then(|v| v.as_str())
            .map(str::to_owned);

        let scope_logs = rl
            .get("scopeLogs")
            .or_else(|| rl.get("scope_logs"))
            .and_then(|v| v.as_array())
            .map_or(&[][..], |v| v.as_slice());

        for sl in scope_logs {
            let records = sl
                .get("logRecords")
                .or_else(|| sl.get("log_records"))
                .and_then(|v| v.as_array())
                .map_or(&[][..], |v| v.as_slice());
            for r in records {
                // Held as an option, because the arrival time that stands in
                // for a missing one must not reach `derive_id`. A record whose
                // sender gave no time would otherwise hash differently on every
                // attempt, and a resend of it would store a second copy: the
                // one case the derived id exists to stop.
                let stamped = r
                    .get("timeUnixNano")
                    .or_else(|| r.get("time_unix_nano"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<i128>().ok())
                    .and_then(|n| OffsetDateTime::from_unix_timestamp_nanos(n).ok());
                let ts = stamped.unwrap_or(now);
                // OTLP severity numbers map onto our levels by range, which is
                // what the specification prescribes.
                let sev = r.get("severityNumber").and_then(|v| v.as_i64()).unwrap_or(9);
                let level = match sev {
                    1..=4 => LogLevel::Trace,
                    5..=8 => LogLevel::Debug,
                    9..=12 => LogLevel::Info,
                    13..=16 => LogLevel::Warn,
                    17..=20 => LogLevel::Error,
                    _ => LogLevel::Fatal,
                };
                let message = r
                    .get("body")
                    .and_then(|b| b.get("stringValue"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let trace_id = r
                    .get("traceId")
                    .or_else(|| r.get("trace_id"))
                    .and_then(|v| v.as_str())
                    .filter(|s| ids::validate_trace_id(s).is_ok())
                    .map(str::to_owned);
                let span_id = r
                    .get("spanId")
                    .or_else(|| r.get("span_id"))
                    .and_then(|v| v.as_str())
                    .filter(|s| ids::validate_span_id(s).is_ok())
                    .map(str::to_owned);

                let fields = r.get("attributes").cloned();
                let logger = sl
                    .get("scope")
                    .and_then(|s| s.get("name"))
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);

                let log_id = derive_id(
                    "otlp-log",
                    out.len(),
                    &[
                        &stamped.map(|t| t.unix_timestamp_nanos().to_string()).unwrap_or_default(),
                        level.as_str(),
                        &message,
                        &fields.as_ref().map(ToString::to_string).unwrap_or_default(),
                        logger.as_deref().unwrap_or(""),
                        trace_id.as_deref().unwrap_or(""),
                        span_id.as_deref().unwrap_or(""),
                        service.as_deref().unwrap_or(""),
                        default_project,
                    ],
                );

                out.push(LogRecord {
                    log_id,
                    trace_id,
                    span_id,
                    timestamp: ts,
                    level,
                    message,
                    fields,
                    logger,
                    correlation: Correlation {
                        project: default_project.to_owned(),
                        environment: None,
                        service_name: service.clone(),
                        service_version: None,
                        session_id: None,
                        thread_id: None,
                        user_id: None,
                        resource_id: None,
                        run_id: None,
                        request_id: None,
                        experiment_id: None,
                        release: None,
                    },
                    received_at: now,
                });
            }
        }
    }
    Ok(out)
}

/// `POST /v1/metrics` — the standard OTLP metric endpoint.
pub async fn otlp_metrics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Body,
) -> ApiResult<Json<serde_json::Value>> {
    let grant = authorize(&state, &headers, Scope::Ingest)?;
    state.metrics.ingest_requests.fetch_add(1, Ordering::Relaxed);
    let body = collect_body(body, state.config.max_body_bytes).await?;
    let body = decompress(&headers, body, state.config.max_body_bytes)?;
    let value: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::BadRequest(format!("body is not valid JSON: {e}")))?;
    let metrics = otlp_metrics_decode(&value, &state.config.default_project)?;
    // As on the logs door: one data point is one stored row, so the number of
    // them is what has to be bounded, not only the bytes they arrived in.
    if metrics.len() > state.config.max_batch_signals {
        return Err(ApiError::TooLarge(format!(
            "batch carries {} metric points; this server accepts {} at a time",
            metrics.len(),
            state.config.max_batch_signals
        )));
    }
    let ingest_key = crate::relay::idempotency_key(&headers)
        .or_else(|| export_key("otlp-metrics", metrics.iter().map(|m| m.metric_id.as_str())));
    let result = finish_pinned(
        &state,
        Vec::new(),
        Batch { metrics, ingest_key, ..Batch::default() },
        &grant,
        crate::auth::pinned_project(&state, &headers).as_deref(),
    )
    .await?;
    Ok(Json(serde_json::json!({
        "partialSuccess": { "rejectedDataPoints": result.rejected, "errorMessage": "" }
    })))
}

/// Decode an OTLP/JSON `ExportMetricsServiceRequest`.
///
/// Sums and gauges become mcpd points directly. Histograms are recorded by
/// their sum and count with the bucket boundaries preserved, which keeps the
/// data honest without pretending mcpd can re-derive an exact quantile from a
/// pre-aggregated histogram.
fn otlp_metrics_decode(
    body: &serde_json::Value,
    default_project: &str,
) -> ApiResult<Vec<mcpd_core::MetricPoint>> {
    use mcpd_core::{Correlation, MetricKind, MetricPoint};
    let resource_metrics = body
        .get("resourceMetrics")
        .or_else(|| body.get("resource_metrics"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| ApiError::BadRequest("missing resourceMetrics".to_owned()))?;

    let now = OffsetDateTime::now_utc();
    let mut out = Vec::new();
    let corr = || Correlation {
        project: default_project.to_owned(),
        environment: None,
        service_name: None,
        service_version: None,
        session_id: None,
        thread_id: None,
        user_id: None,
        resource_id: None,
        run_id: None,
        request_id: None,
        experiment_id: None,
        release: None,
    };

    for rm in resource_metrics {
        let scope_metrics = rm
            .get("scopeMetrics")
            .or_else(|| rm.get("scope_metrics"))
            .and_then(|v| v.as_array())
            .map_or(&[][..], |v| v.as_slice());
        for sm in scope_metrics {
            let metrics =
                sm.get("metrics").and_then(|v| v.as_array()).map_or(&[][..], |v| v.as_slice());
            for m in metrics {
                let name = m.get("name").and_then(|v| v.as_str()).unwrap_or("unnamed").to_owned();
                let unit = m.get("unit").and_then(|v| v.as_str()).map(str::to_owned);

                let (kind, points) = if let Some(s) = m.get("sum") {
                    (MetricKind::Counter, s.get("dataPoints"))
                } else if let Some(g) = m.get("gauge") {
                    (MetricKind::Gauge, g.get("dataPoints"))
                } else if let Some(h) = m.get("histogram") {
                    (MetricKind::Histogram, h.get("dataPoints"))
                } else {
                    continue;
                };
                let Some(points) = points.and_then(|v| v.as_array()) else { continue };

                for p in points {
                    // An option for the same reason as a log record's: the
                    // arrival time is not part of what the sender sent, so it
                    // must not decide the derived id.
                    let stamped = p
                        .get("timeUnixNano")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse::<i128>().ok())
                        .and_then(|n| OffsetDateTime::from_unix_timestamp_nanos(n).ok());
                    let ts = stamped.unwrap_or(now);
                    let value = p
                        .get("asDouble")
                        .and_then(|v| v.as_f64())
                        .or_else(|| {
                            p.get("asInt").and_then(|v| {
                                v.as_str()
                                    .and_then(|s| s.parse::<f64>().ok())
                                    .or_else(|| v.as_f64())
                            })
                        })
                        .or_else(|| p.get("sum").and_then(|v| v.as_f64()))
                        .unwrap_or(0.0);

                    // Exemplars are the reason a spike on a chart can open the
                    // run that caused it — the single most useful thing OTLP
                    // metrics carry and the thing most tools discard.
                    let exemplar_trace = p
                        .get("exemplars")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.first())
                        .and_then(|e| e.get("traceId"))
                        .and_then(|v| v.as_str())
                        .filter(|s| ids::validate_trace_id(s).is_ok())
                        .map(str::to_owned);

                    let labels = p.get("attributes").cloned();
                    let buckets = (kind == MetricKind::Histogram).then(|| {
                        serde_json::json!({
                            "bounds": p.get("explicitBounds"),
                            "counts": p.get("bucketCounts"),
                            "count": p.get("count"),
                        })
                    });

                    let metric_id = derive_id(
                        "otlp-metric",
                        out.len(),
                        &[
                            &stamped
                                .map(|t| t.unix_timestamp_nanos().to_string())
                                .unwrap_or_default(),
                            &name,
                            kind.as_str(),
                            // The bit pattern, not the printed number, so two
                            // points that differ only past the last digit a
                            // formatter would show are still two points.
                            &value.to_bits().to_string(),
                            unit.as_deref().unwrap_or(""),
                            &labels.as_ref().map(ToString::to_string).unwrap_or_default(),
                            &buckets.as_ref().map(ToString::to_string).unwrap_or_default(),
                            exemplar_trace.as_deref().unwrap_or(""),
                            default_project,
                        ],
                    );

                    out.push(MetricPoint {
                        metric_id,
                        name: name.clone(),
                        kind,
                        value,
                        unit: unit.clone(),
                        timestamp: ts,
                        labels,
                        buckets,
                        trace_id: exemplar_trace,
                        span_id: None,
                        correlation: corr(),
                        received_at: now,
                    });
                }
            }
        }
    }
    Ok(out)
}

/// The shapes `POST /v1/ingest` reads a body in.
enum Shape {
    /// One batch, or an array of them, as one JSON document.
    Json,
    /// One batch per line, read as the body arrives.
    Ndjson,
}

/// Which shape a request is in, decided by its content type and nothing else.
///
/// Nothing is guessed from the bytes. A JSON array and an NDJSON stream can
/// both begin with a character the other shape permits, and a door that
/// sniffed would change what a body means the day a producer added a newline
/// inside a value. Anything not declared as NDJSON is read as JSON, which is
/// what every existing client sends and what a request with no content type at
/// all gets, so the door behaves as it always did unless a sender opts in.
fn shape_of(headers: &HeaderMap) -> Shape {
    let declared = headers.get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("");
    let mime = declared.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    // `application/x-ndjson` is the name the format itself uses and the one
    // exporters send; `jsonl` is the other name in the wild for the same bytes.
    if mime.contains("ndjson") || mime.contains("jsonl") {
        Shape::Ndjson
    } else {
        Shape::Json
    }
}

/// Whether the sender said the body is compressed.
fn is_compressed(headers: &HeaderMap) -> bool {
    headers
        .get("content-encoding")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_ascii_lowercase())
        .is_some_and(|v| !v.is_empty() && v != "identity")
}

/// One batch, or an array of them.
///
/// A visitor rather than an untagged enum: serde reports an untagged failure
/// as "data did not match any variant", which loses the field that was
/// actually wrong, and that field is the whole content of the message. The two
/// shapes are told apart by JSON's own grammar, which is why this needs no
/// second content type of its own.
struct NativeJson(Vec<IngestBatch>);

impl<'de> serde::Deserialize<'de> for NativeJson {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Either;
        impl<'de> serde::de::Visitor<'de> for Either {
            type Value = Vec<IngestBatch>;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an ingest batch, or an array of ingest batches")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Self::Value, A::Error> {
                let one = serde::Deserialize::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                Ok(vec![one])
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                seq: A,
            ) -> Result<Self::Value, A::Error> {
                serde::Deserialize::deserialize(serde::de::value::SeqAccessDeserializer::new(seq))
            }
        }
        d.deserialize_any(Either).map(NativeJson)
    }
}

/// Everything one native request has decoded, and the bounds it is held to.
///
/// The bounds are counted here rather than per element for the reason the
/// second shape exists at all: a request that could carry ten thousand signals
/// per line would make NDJSON the way around a limit every other door applies.
#[derive(Default)]
struct Native {
    /// One entry per unit of work handed to [`finish`].
    units: Vec<Batch>,
    /// Where the batches with no idempotency key are being merged, once one has
    /// arrived.
    keyless: Option<usize>,
    /// Refusals for signals that will not be stored.
    errors: Vec<IngestError>,
    /// Spans seen so far, valid or not, which is the index an error reports.
    spans_seen: usize,
    /// Signals that would be stored, across the whole request.
    signals: usize,
    /// Batches read so far, whether they arrived as elements or as lines.
    batches: usize,
}

impl Native {
    /// Take one decoded batch: validate its span ids, count it, and hold it.
    fn add(&mut self, incoming: IngestBatch, cap: usize) -> ApiResult<()> {
        self.batches += 1;
        let mut spans = Vec::new();
        for s in incoming.spans.unwrap_or_default() {
            // The index counts every span in the request, valid or not, so a
            // request carrying one batch reports exactly the position it always
            // did, and one carrying several still names a position that exists.
            let index = u32::try_from(self.spans_seen).unwrap_or(u32::MAX);
            self.spans_seen += 1;
            // Validate rather than trust: a bad id would poison every join later.
            if let Err(e) = ids::validate_trace_id(&s.trace_id) {
                self.errors.push(IngestError {
                    signal: SignalType::Span,
                    index,
                    message: e.to_string(),
                });
                continue;
            }
            if let Err(e) = ids::validate_span_id(&s.span_id) {
                self.errors.push(IngestError {
                    signal: SignalType::Span,
                    index,
                    message: e.to_string(),
                });
                continue;
            }
            spans.push(s);
        }

        let batch = Batch {
            spans,
            logs: incoming.logs.unwrap_or_default(),
            metrics: incoming.metrics.unwrap_or_default(),
            scores: incoming.scores.unwrap_or_default(),
            feedback: incoming.feedback.unwrap_or_default(),
            aspect_notes: incoming.aspect_notes.unwrap_or_default(),
            ingest_key: incoming.ingest_key,
        };

        self.signals += batch.len();
        if self.signals > cap {
            return Err(self.over_cap(cap));
        }

        match (batch.ingest_key.is_some(), self.keyless) {
            // A key claims that everything its batch named was written, so two
            // keyed batches must not be folded together: one key would end up
            // describing records the other named, and a retry of either would
            // be answered "already stored" for both. Keeping them apart is also
            // what lets a resent stream be recognised line by line.
            (true, _) => self.units.push(batch),
            // Everything with no key is one batch, so the ordinary stream costs
            // one transaction rather than one per line. Nothing is lost by it:
            // a batch with no key claims nothing about what else was written.
            (false, Some(at)) => {
                let into = &mut self.units[at];
                into.spans.extend(batch.spans);
                into.logs.extend(batch.logs);
                into.metrics.extend(batch.metrics);
                into.scores.extend(batch.scores);
                into.feedback.extend(batch.feedback);
                into.aspect_notes.extend(batch.aspect_notes);
            }
            (false, None) => {
                self.keyless = Some(self.units.len());
                self.units.push(batch);
            }
        }
        Ok(())
    }

    /// Take one NDJSON line.
    ///
    /// A blank line is skipped rather than refused. A trailing newline, a
    /// blank line between records and a `\r\n` ending are all things real
    /// producers write, and none of them is a mistake worth refusing an export
    /// for.
    fn add_line(&mut self, line: &[u8], number: usize, cap: usize) -> ApiResult<()> {
        let line = line.trim_ascii();
        if line.is_empty() {
            return Ok(());
        }
        let incoming: IngestBatch = serde_json::from_slice(line).map_err(|e| {
            // The line number, because "the body is not a batch" is no help
            // when the body is ten thousand of them.
            ApiError::BadRequest(format!("line {number} is not a valid ingest batch: {e}"))
        })?;
        self.add(incoming, cap)
    }

    /// The refusal for a request carrying more signals than the server takes.
    fn over_cap(&self, cap: usize) -> ApiError {
        if self.batches == 1 {
            return ApiError::TooLarge(format!(
                "batch carries {} signals; this server accepts {cap} at a time",
                self.signals
            ));
        }
        // Counted across the request, and said so: a sender that split its
        // signals over lines to get under the number would otherwise read this
        // as being about one of them.
        ApiError::TooLarge(format!(
            "this request carries more than the {cap} signals this server accepts, counted \
             across all {} batches in it; send fewer signals per request",
            self.batches
        ))
    }
}

/// Read a whole body, held to the byte cap.
async fn collect_body(body: Body, max_bytes: usize) -> ApiResult<Bytes> {
    axum::body::to_bytes(body, max_bytes).await.map_err(|e| body_error(&e, max_bytes))
}

/// What to answer when reading the body failed.
///
/// A body over the cap arrives here as a read error rather than as a size the
/// handler can measure: the limit lives in a layer, which stops handing over
/// frames the moment the total passes it. The error type it produces belongs to
/// a crate this one does not depend on directly, so the chain is read by its
/// text. Getting that wrong costs a wrong status on a broken upload, never a
/// wrong decision about what to store.
///
/// The sentence is the one `layers::refuse_oversize` uses for a body with no
/// declared length, because it is the same refusal: a handler that read the
/// body itself is answering instead of the layer, and the two doors must not
/// describe the same limit differently.
fn body_error(e: &axum::Error, max_bytes: usize) -> ApiError {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(current) = source {
        if current.to_string().contains("length limit exceeded") {
            return ApiError::TooLarge(format!(
                "request body is larger than the {max_bytes} bytes this server accepts per \
                 request; send fewer signals per request"
            ));
        }
        source = current.source();
    }
    ApiError::BadRequest(format!("the request body could not be read: {e}"))
}

/// Read an NDJSON body a line at a time, decoding each line as it arrives.
///
/// The raw bytes of a line are dropped as soon as it has been decoded, so a
/// large export costs the decoded signals plus one line rather than the whole
/// body and the decoded signals at once. Both bounds still hold across the
/// request: the byte cap is the layer's, which is what ends the loop when a
/// sender streams past it, and the signal cap is counted in [`Native`].
///
/// Nothing is stored until the whole body has been read. A line that cannot be
/// decoded, or a request over either cap, therefore refuses the request whole,
/// which is what the one-object door has always done with a body it could not
/// read.
async fn read_ndjson(
    body: Body,
    max_bytes: usize,
    cap: usize,
    native: &mut Native,
) -> ApiResult<()> {
    use futures::StreamExt;
    let mut stream = body.into_data_stream();
    // Only what is left of a line that straddled two frames, so a frame full of
    // whole lines is never copied.
    let mut carried: Vec<u8> = Vec::new();
    let mut number = 1usize;
    while let Some(frame) = stream.next().await {
        let chunk = frame.map_err(|e| body_error(&e, max_bytes))?;
        let mut rest: &[u8] = &chunk;
        while let Some(at) = rest.iter().position(|b| *b == b'\n') {
            if carried.is_empty() {
                native.add_line(&rest[..at], number, cap)?;
            } else {
                carried.extend_from_slice(&rest[..at]);
                native.add_line(&carried, number, cap)?;
                carried.clear();
            }
            number += 1;
            rest = &rest[at + 1..];
        }
        carried.extend_from_slice(rest);
    }
    // A last line with no newline after it is still a line.
    native.add_line(&carried, number, cap)
}

/// `POST /v1/ingest` — mcpd's native batch endpoint.
///
/// Accepts the full model: token detail, cost, scores and feedback all survive
/// intact, which OTLP cannot express.
///
/// Three shapes arrive here, and the content type says which:
///
/// - one JSON object, the shape every SDK sends;
/// - a JSON array of them, for a sender that has several to hand;
/// - `application/x-ndjson`, one per line, read as the body arrives.
///
/// Each element is treated as the batch it is, so a stream costs the sender one
/// request instead of one per batch and costs mcpd nothing in guarantees: the
/// per-request caps count the whole request, and an `ingest_key` still covers
/// exactly the batch that carried it. A batch that carries a key is stored in
/// its own transaction for that reason, so storage failing part-way through a
/// stream leaves the batches before it stored, exactly as it would have done
/// had the sender posted them one request at a time. Retrying the stream is
/// how a sender recovers from that, and the keys are what make the retry cost
/// nothing. A request of many keyed batches is therefore many transactions,
/// bounded by the request deadline the way any other slow handler is; a stream
/// with no keys in it, which is the ordinary case, is one.
pub async fn native(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Body,
) -> ApiResult<Json<IngestResult>> {
    let grant = authorize(&state, &headers, Scope::Ingest)?;
    let pin = crate::auth::pinned_project(&state, &headers);
    state.metrics.ingest_requests.fetch_add(1, Ordering::Relaxed);

    let cap = state.config.max_batch_signals;
    let max_bytes = state.config.max_body_bytes;
    let mut native = Native::default();
    match shape_of(&headers) {
        // A compressed body has to be whole before it can be inflated, so there
        // is nothing to stream: read it, inflate it under the same cap, then
        // split it.
        Shape::Ndjson if is_compressed(&headers) => {
            let body = decompress(&headers, collect_body(body, max_bytes).await?, max_bytes)?;
            for (i, line) in body.split(|b| *b == b'\n').enumerate() {
                native.add_line(line, i + 1, cap)?;
            }
        }
        Shape::Ndjson => read_ndjson(body, max_bytes, cap, &mut native).await?,
        Shape::Json => {
            let body = decompress(&headers, collect_body(body, max_bytes).await?, max_bytes)?;
            let NativeJson(batches) = serde_json::from_slice(&body).map_err(|e| {
                ApiError::BadRequest(format!("body is not a valid ingest batch: {e}"))
            })?;
            for incoming in batches {
                native.add(incoming, cap)?;
            }
        }
    }

    // A header key covers the signals the body gave no key of its own.
    if let (Some(at), Some(key)) = (native.keyless, crate::relay::idempotency_key(&headers)) {
        native.units[at].ingest_key = Some(key);
    }

    // One pass per unit, after the whole request has been decoded and both caps
    // have held. A request refused for its size therefore stores nothing, which
    // is the difference between a limit and a warning.
    let mut result = IngestResult { accepted: 0, rejected: 0, duplicate: 0, errors: None };
    for mut unit in native.units {
        let spans = std::mem::take(&mut unit.spans);
        let one = finish_pinned(&state, spans, unit, &grant, pin.as_deref()).await?;
        result.accepted += one.accepted;
        result.rejected += one.rejected;
        result.duplicate += one.duplicate;
        if let Some(errors) = one.errors {
            result.errors.get_or_insert_with(Vec::new).extend(errors);
        }
    }

    // Accumulated, not assigned. `finish` records its own rejections, notably a
    // signal named for a project this key does not reach, and overwriting the
    // count here reported those as accepted.
    let errors = native.errors;
    result.rejected += u32::try_from(errors.len()).unwrap_or(u32::MAX);
    if !errors.is_empty() {
        state.metrics.rejected.fetch_add(errors.len() as u64, Ordering::Relaxed);
        result.errors.get_or_insert_with(Vec::new).extend(errors);
    }
    Ok(Json(result))
}

/// Move every signal in a batch into one project.
fn pin_batch(batch: &mut Batch, project: &str) {
    for s in &mut batch.spans {
        project.clone_into(&mut s.correlation.project);
    }
    for l in &mut batch.logs {
        project.clone_into(&mut l.correlation.project);
    }
    for m in &mut batch.metrics {
        project.clone_into(&mut m.correlation.project);
    }
    for s in &mut batch.scores {
        project.clone_into(&mut s.correlation.project);
    }
    for f in &mut batch.feedback {
        project.clone_into(&mut f.correlation.project);
    }
    for n in &mut batch.aspect_notes {
        project.clone_into(&mut n.correlation.project);
    }
    if let Some(key) = batch.ingest_key.take() {
        batch.ingest_key = Some(format!("{project}\u{1f}{key}"));
    }
}

/// Drop every signal the grant does not reach, and report how many.
///
/// `Grant::All` is the common path and does no work at all.
fn retain_authorised(batch: &mut Batch, grant: &Grant) -> u32 {
    let Some(allowed) = grant.projects() else { return 0 };

    let before = batch.len();
    batch.spans.retain(|s| allowed.contains(&s.correlation.project));
    batch.logs.retain(|l| allowed.contains(&l.correlation.project));
    batch.metrics.retain(|m| allowed.contains(&m.correlation.project));
    batch.scores.retain(|s| allowed.contains(&s.correlation.project));
    batch.feedback.retain(|f| allowed.contains(&f.correlation.project));
    batch.aspect_notes.retain(|n| allowed.contains(&n.correlation.project));
    u32::try_from(before - batch.len()).unwrap_or(u32::MAX)
}

/// What one project's ceiling cost this batch.
struct QuotaCut {
    /// The project that was over.
    project: String,
    /// Its ceiling, so the message can name the number to raise.
    limit: u32,
    /// How many of its signals were refused.
    rejected: u32,
    /// Whether that ceiling is the one every unlisted project shares, which
    /// changes what the sender should do about it.
    shared: bool,
}

/// What the ceilings did to one batch.
struct QuotaOutcome {
    /// The projects that were over, one entry each.
    cuts: Vec<QuotaCut>,
    /// Signals kept that no ceiling covers.
    ///
    /// Counted rather than refused. The alternative is to refuse a project
    /// nobody wrote down, which drops the data of a deployment that upgraded
    /// without changing its configuration. So the bypass is reported: this
    /// number is `mcpd_signals_unbounded_total`, and it is how an operator
    /// whose ceilings live only in the `projects` block finds out that a sender
    /// walked past them by naming something else.
    unbounded: u32,
}

/// Refuse whatever is over each project's ceiling, and report it per project.
///
/// **The batch is partly accepted rather than answered with a 429**, and the
/// reason is what the sender does next. A batch may legitimately name several
/// projects, so refusing the whole request punishes the projects that are inside
/// their ceiling, which is the exact harm the ceiling exists to prevent. Worse,
/// an SDK reads a 429 as "send this again": it keeps the batch queued, retries
/// into a ceiling that is still full, and its queue overflows meanwhile, at
/// which point contract behaviour B3 drops the *oldest* signals. A whole-batch
/// refusal therefore converts "the newest excess is refused" into "the oldest
/// good data is lost", and adds a retry storm to a daemon already under flood.
///
/// A count and a reason in the body is the shape ingest already has, every SDK
/// already reads `rejected` and `errors`, and nothing on the wire changes, so
/// this needs no spec change in six languages to be understood.
///
/// Does no work at all when no project has a ceiling, which is the default.
fn enforce_quotas(batch: &mut Batch, quotas: &ProjectQuotas) -> QuotaOutcome {
    if quotas.is_empty() {
        return QuotaOutcome { cuts: Vec::new(), unbounded: 0 };
    }

    let mut wanted: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    let mut unbounded = 0u32;
    unbounded +=
        tally(quotas, batch.spans.iter().map(|s| s.correlation.project.as_str()), &mut wanted);
    unbounded +=
        tally(quotas, batch.logs.iter().map(|l| l.correlation.project.as_str()), &mut wanted);
    unbounded +=
        tally(quotas, batch.metrics.iter().map(|m| m.correlation.project.as_str()), &mut wanted);
    unbounded +=
        tally(quotas, batch.scores.iter().map(|s| s.correlation.project.as_str()), &mut wanted);
    unbounded +=
        tally(quotas, batch.feedback.iter().map(|f| f.correlation.project.as_str()), &mut wanted);
    unbounded += tally(
        quotas,
        batch.aspect_notes.iter().map(|n| n.correlation.project.as_str()),
        &mut wanted,
    );

    let mut cuts = Vec::new();
    for (project, asked) in wanted {
        let Some(limit) = quotas.limit_for(&project) else { continue };
        let allowance = quotas.admit(&project, asked);
        if allowance >= asked {
            continue;
        }
        let (kept, rejected) = trim_project(batch, &project, allowance);
        // Whole traces means the trim can keep less than it was permitted. Hand
        // the difference back, or a project whose traces are larger than its
        // remaining allowance pays its way to zero while storing nothing.
        quotas.refund(&project, allowance.saturating_sub(kept));
        if rejected > 0 {
            let shared = quotas.is_shared(&project);
            cuts.push(QuotaCut { project, limit, rejected, shared });
        }
    }
    QuotaOutcome { cuts, unbounded }
}

/// Count signals per project, ignoring every project without a ceiling, and
/// return how many of them that was.
///
/// The project name arrives in the request body, so only names that are already
/// configured are allowed to put an entry in this map. A name that is not in it
/// is not being limited, and the count of those is the size of the bypass: it
/// is the number an operator sees on `mcpd_signals_unbounded_total`, so the one
/// configuration that leaves the ceiling open is visible from outside the
/// daemon rather than only from `mcpd doctor`.
fn tally<'a>(
    quotas: &ProjectQuotas,
    projects: impl Iterator<Item = &'a str>,
    into: &mut std::collections::BTreeMap<String, u32>,
) -> u32 {
    let mut unbounded = 0u32;
    for project in projects {
        if quotas.limit_for(project).is_none() {
            unbounded = unbounded.saturating_add(1);
            continue;
        }
        if let Some(n) = into.get_mut(project) {
            *n += 1;
        } else {
            into.insert(project.to_owned(), 1);
        }
    }
    unbounded
}

/// Cut one project down to `allowance` signals. Returns what was kept and what
/// was refused, for that project alone.
///
/// Spans go by whole trace, for the reason the sampler does it: half a trace has
/// spans whose parent nobody stored, and that reads as a broken run rather than
/// a missing one. A trace too large for what is left is skipped and the next one
/// tried, rather than ending the pass, so one big trace does not spend the
/// remainder of the allowance on nothing. The other signals stand alone, so they
/// fill whatever is left in arrival order.
fn trim_project(batch: &mut Batch, project: &str, allowance: u32) -> (u32, u32) {
    let before = signals_for(batch, project);
    let mut budget = allowance;

    let mut keep: std::collections::HashSet<String> = std::collections::HashSet::new();
    {
        let mut lengths: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
        let mut arrival: Vec<&str> = Vec::new();
        for span in &batch.spans {
            if span.correlation.project != project {
                continue;
            }
            let seen = lengths.entry(span.trace_id.as_str()).or_insert(0);
            if *seen == 0 {
                arrival.push(span.trace_id.as_str());
            }
            *seen += 1;
        }
        for trace in arrival {
            let len = lengths[trace];
            if len <= budget {
                budget -= len;
                keep.insert(trace.to_owned());
            }
        }
    }
    batch.spans.retain(|s| s.correlation.project != project || keep.contains(&s.trace_id));

    batch.logs.retain(|l| admit_one(l.correlation.project == project, &mut budget));
    batch.metrics.retain(|m| admit_one(m.correlation.project == project, &mut budget));
    batch.scores.retain(|s| admit_one(s.correlation.project == project, &mut budget));
    batch.feedback.retain(|f| admit_one(f.correlation.project == project, &mut budget));
    batch.aspect_notes.retain(|n| admit_one(n.correlation.project == project, &mut budget));

    let after = signals_for(batch, project);
    (after, before.saturating_sub(after))
}

/// Whether one signal that stands alone fits in what is left of the allowance.
fn admit_one(mine: bool, budget: &mut u32) -> bool {
    if !mine {
        return true;
    }
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    true
}

/// How many of the batch's signals belong to one project.
fn signals_for(batch: &Batch, project: &str) -> u32 {
    let n = batch.spans.iter().filter(|s| s.correlation.project == project).count()
        + batch.logs.iter().filter(|l| l.correlation.project == project).count()
        + batch.metrics.iter().filter(|m| m.correlation.project == project).count()
        + batch.scores.iter().filter(|s| s.correlation.project == project).count()
        + batch.feedback.iter().filter(|f| f.correlation.project == project).count()
        + batch.aspect_notes.iter().filter(|n| n.correlation.project == project).count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The entry that tells a sender it was limited, and what to do about it.
///
/// The unit here is the project rather than one record, so there is no index to
/// give. `retain_authorised` reports its refusals the same way, for the same
/// reason: the message names the project, which is the thing the sender has to
/// act on.
fn quota_error(cut: &QuotaCut) -> IngestError {
    IngestError {
        signal: SignalType::Span,
        index: 0,
        message: over_ceiling(cut.rejected, &cut.project, cut.limit, cut.shared),
    }
}

/// What a sender is told when a ceiling refused some of its signals.
///
/// One sentence for both doors, because a score refused by the same counter as
/// a span should not read as a different feature. The shared ceiling gets its
/// own wording: being told "project `analytics-v2` is over its ceiling" about a
/// name you invented in a config file five minutes ago sends you looking for a
/// setting that does not exist, when the answer is to add the project.
pub(crate) fn over_ceiling(rejected: u32, project: &str, limit: u32, shared: bool) -> String {
    if shared {
        return format!(
            "{rejected} signal(s) named project {project}, which no configuration lists. \
             Every unlisted project shares one ceiling of {limit} per minute and it was \
             full, so they were not stored. Add {project} to the `projects` block to give \
             it an allowance of its own, or raise --quota-per-minute."
        );
    }
    format!(
        "{rejected} signal(s) were over project {project}'s ceiling of {limit} per minute \
         and were not stored; send fewer, or raise --quota-per-minute"
    )
}

/// Charge one signal to a project's ceiling, for a door that writes exactly one
/// record rather than a batch.
///
/// `POST /v1/api/scores` and `POST /v1/api/feedback` write a signal each and
/// went through `Intake` but not through this counter, so a project at its
/// ceiling for spans could still store scores and feedback without bound. Both
/// are stored rows on the same disk the ceiling exists to bound.
///
/// Returns the refusal to give the caller, or `None` when the signal is inside
/// the ceiling and has now been charged for. A batch door trims and reports per
/// project because a batch may name several; a door that writes one record has
/// no partial answer to give, so it refuses the request.
#[must_use]
pub fn charge_one(state: &AppState, project: &str) -> Option<QuotaRefusal> {
    let Some(limit) = state.quotas.limit_for(project) else {
        // No ceiling covers this project, so nothing is charged and nothing is
        // refused. Counted so the bypass is visible whichever door a sender
        // used, but not yet on the same terms as the batch door: the batch door
        // counts after the ids are validated and only what it is about to hand
        // to the store, and this runs before `create_score` and
        // `create_feedback` have validated anything. A score with no `trace_id`
        // is counted here and then refused with 400, so the counter can move
        // while nothing reaches the disk. Moving both calls in `query.rs` below
        // their handler's validation is what would make the two agree.
        if !state.quotas.is_empty() {
            state.metrics.unbounded.fetch_add(1, Ordering::Relaxed);
        }
        return None;
    };
    if state.quotas.admit(project, 1) == 1 {
        return None;
    }
    // Counted the same way the batch door counts it, so `/metrics` does not
    // report a different number of refusals depending on which door was used.
    state.metrics.over_quota.fetch_add(1, Ordering::Relaxed);
    state.metrics.rejected.fetch_add(1, Ordering::Relaxed);
    // The wait is read off the counter that did the refusing, at the moment it
    // refused. Anywhere else in the response path the counter is gone and the
    // header would have to be a guess.
    Some(QuotaRefusal {
        message: over_ceiling(1, project, limit, state.quotas.is_shared(project)),
        retry_after_seconds: state.quotas.retry_after_seconds(project),
    })
}

/// Charge `wanted` signals to a project's ceiling and say how many may be
/// stored.
///
/// [`charge_one`] answers the single-record doors. This answers the scorer run,
/// which writes a whole batch of scores straight to the store in one call and
/// so cannot ask one signal at a time.
///
/// It returns a count rather than a refusal because the caller has somewhere
/// honest to put the shortfall: `ScoreRunResult.skipped` already carries a
/// scorer, a run and a reason, so a trimmed batch is reported per score instead
/// of failing a request that did most of its work. That also matches the batch
/// ingest door, which accepts what fits and reports the rest.
///
/// Returns `wanted` unchanged when no ceiling covers the project.
pub fn charge_many(state: &AppState, project: &str, wanted: u32) -> u32 {
    if state.quotas.limit_for(project).is_none() {
        if !state.quotas.is_empty() {
            state.metrics.unbounded.fetch_add(u64::from(wanted), Ordering::Relaxed);
        }
        return wanted;
    }
    let permitted = state.quotas.admit(project, wanted);
    let refused = wanted.saturating_sub(permitted);
    if refused > 0 {
        state.metrics.over_quota.fetch_add(u64::from(refused), Ordering::Relaxed);
        state.metrics.rejected.fetch_add(u64::from(refused), Ordering::Relaxed);
    }
    permitted
}

/// A deterministic id for a record OTLP does not give one.
///
/// OTLP carries no identifier for a log record or a metric point, so mcpd has
/// to invent one, and it used to mint a random one on every decode. That made
/// these two doors the only ones where a retry was not idempotent: an exporter
/// that resent an export after a timeout stored a second copy of every line and
/// every point, and every sum and count built on them was quietly wrong. The
/// native door has never had this, because a client sends the ids.
///
/// Content alone is not enough to derive from. Two identical log lines in one
/// export are two events, not a duplicate, and a digest over the text would
/// keep one and lose the other. So the record's position in the export is part
/// of the digest: it separates genuine repeats inside one payload while staying
/// the same when that payload arrives again.
///
/// The bound on this is worth stating. The ids are stable across a resend of
/// the same request, which is what the OTLP retry rules prescribe, and not
/// across a sender that re-cuts its batches differently between attempts.
fn derive_id(domain: &str, ordinal: usize, parts: &[&str]) -> String {
    let mut h = Sha256::new();
    h.update(domain.as_bytes());
    h.update(ordinal.to_le_bytes());
    for p in parts {
        // Length-prefixed. Without it two records could be split into fields
        // differently and still hash the same, which is one record silently
        // replacing another.
        h.update((p.len() as u64).to_le_bytes());
        h.update(p.as_bytes());
    }
    // Half of SHA-256. 128 bits is past any collision that matters here, and a
    // 32-character id stays readable in a query and in a log line.
    hex::encode(&h.finalize()[..16])
}

/// The idempotency key for a decoded OTLP export.
///
/// Every derived id already covers its record's content and its position, so
/// hashing them in order gives a key that is the same for a resent export and
/// different for one that differs in any record, in their order, or in how many
/// there are. It is the same thing the native door is handed in `ingest_key`,
/// and it lets a retry cost one lookup rather than a row-by-row conflict, and
/// be reported honestly as a duplicate rather than as newly accepted.
///
/// `None` for an empty export, so a payload carrying nothing does not claim a
/// key and leave a row behind in `ingest_keys` for retention to clear.
fn export_key<'a>(domain: &str, ids: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut h = Sha256::new();
    h.update(domain.as_bytes());
    let mut count = 0usize;
    for id in ids {
        count += 1;
        h.update(id.as_bytes());
    }
    if count == 0 {
        return None;
    }
    Some(format!("{domain}-{}", hex::encode(&h.finalize()[..16])))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ProjectConfig, ServerConfig};
    use mcpd_core::{Correlation, LogLevel, LogRecord, SpanKind, SpanStatus};
    use mcpd_store::sqlite::SqliteStore;
    use mcpd_store::Store;
    use std::sync::atomic::AtomicU32;

    /// Ceilings for a set of named projects, counters at zero. Nothing the
    /// configuration does not name has a ceiling.
    fn quotas(limits: &[(&str, u32)]) -> ProjectQuotas {
        ProjectQuotas::new(limits.iter().map(|(p, n)| ((*p).to_owned(), *n)).collect(), None)
    }

    fn correlation(project: &str) -> Correlation {
        Correlation {
            project: project.to_owned(),
            environment: None,
            service_name: None,
            service_version: None,
            session_id: None,
            thread_id: None,
            user_id: None,
            resource_id: None,
            run_id: None,
            request_id: None,
            experiment_id: None,
            release: None,
        }
    }

    /// A trace id unique to the project as well as the number.
    ///
    /// Not the number alone: a trace id is random in real traffic, and a test
    /// that hands two projects the same one can pass while the code under test
    /// confuses them. Removing the project filter from the trim went unnoticed
    /// until this was fixed.
    fn trace_id(project: &str, trace: u64) -> String {
        let tag = project.bytes().fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(u64::from(b)));
        format!("{tag:016x}{trace:016x}")
    }

    /// One span of `trace`, in `project`. Ids are derived from the arguments so
    /// a test can say which trace a span belongs to and read it back.
    fn span(project: &str, trace: u64, span: u64) -> Span {
        Span {
            trace_id: trace_id(project, trace),
            span_id: format!("{span:016x}"),
            parent_span_id: None,
            external_parent_span_id: None,
            name: "work".into(),
            kind: SpanKind::ModelGeneration,
            status: SpanStatus::Ok,
            start_time: OffsetDateTime::UNIX_EPOCH,
            end_time: Some(OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1)),
            duration_ns: None,
            is_root: span == 0,
            is_event: false,
            entity_kind: None,
            entity_id: None,
            entity_name: None,
            input: None,
            output: None,
            attributes: None,
            metadata: None,
            usage: None,
            cost: None,
            model_parameters: None,
            error: None,
            links: None,
            tags: None,
            correlation: correlation(project),
            model: None,
            provider: None,
            received_at: OffsetDateTime::UNIX_EPOCH,
            ingest_key: None,
        }
    }

    fn log(project: &str, n: u64) -> LogRecord {
        LogRecord {
            log_id: format!("log-{n}"),
            trace_id: None,
            span_id: None,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            level: LogLevel::Info,
            message: format!("line {n}"),
            fields: None,
            logger: None,
            correlation: correlation(project),
            received_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    /// `n` spans of `project`, each its own trace, so one span is one signal.
    fn one_span_traces(project: &str, n: u64) -> Vec<Span> {
        (0..n).map(|i| span(project, i, 0)).collect()
    }

    fn spans_of(batch: &Batch, project: &str) -> usize {
        batch.spans.iter().filter(|s| s.correlation.project == project).count()
    }

    #[test]
    fn a_project_inside_its_ceiling_is_left_alone() {
        let mut batch = Batch { spans: one_span_traces("alpha", 5), ..Batch::default() };
        let cuts = enforce_quotas(&mut batch, &quotas(&[("alpha", 10)])).cuts;
        assert!(cuts.is_empty(), "nothing was over, so nothing is reported");
        assert_eq!(batch.spans.len(), 5, "every span survives");
    }

    #[test]
    fn a_project_over_its_ceiling_keeps_the_ceiling_and_the_excess_is_reported() {
        let mut batch = Batch {
            spans: one_span_traces("alpha", 12),
            logs: (0..3).map(|i| log("alpha", i)).collect(),
            ..Batch::default()
        };
        let cuts = enforce_quotas(&mut batch, &quotas(&[("alpha", 10)])).cuts;
        assert_eq!(batch.spans.len() + batch.logs.len(), 10, "exactly the ceiling is stored");
        assert_eq!(cuts.len(), 1);
        assert_eq!(cuts[0].project, "alpha");
        assert_eq!(cuts[0].limit, 10);
        assert_eq!(cuts[0].rejected, 5, "fifteen arrived, ten fit, five are refused");
    }

    #[test]
    fn one_project_over_its_ceiling_does_not_cost_another_in_the_same_batch() {
        // The whole point of the feature. Alpha sends five times its ceiling in
        // the same request beta is using, and beta must not lose a signal.
        let mut spans = one_span_traces("alpha", 50);
        spans.extend(one_span_traces("beta", 8));
        let mut batch = Batch { spans, ..Batch::default() };

        let quotas = quotas(&[("alpha", 10), ("beta", 10)]);
        let cuts = enforce_quotas(&mut batch, &quotas).cuts;

        assert_eq!(spans_of(&batch, "beta"), 8, "beta did nothing wrong and loses nothing");
        assert_eq!(spans_of(&batch, "alpha"), 10, "alpha is held to its own ceiling");
        assert_eq!(cuts.len(), 1, "only alpha is reported");
        assert_eq!(cuts[0].project, "alpha");
        assert_eq!(cuts[0].rejected, 40);
        // And beta still has its full ceiling to spend afterwards, so alpha's
        // flood did not eat into beta's counter either.
        assert_eq!(quotas.admit("beta", 10), 2, "beta has spent eight of its ten, no more");
        assert_eq!(quotas.admit("alpha", 1), 0, "alpha has spent all ten of its own");
    }

    #[test]
    fn a_project_no_ceiling_covers_is_stored_in_full_and_counted_as_unbounded() {
        // Ten thousand signals through a quota set that does not name gamma.
        // Every one of them is kept, and every one of them is counted: this is
        // the bypass, and the count is the only thing that makes it visible.
        let mut batch = Batch { spans: one_span_traces("gamma", 10_000), ..Batch::default() };
        let outcome = enforce_quotas(&mut batch, &quotas(&[("alpha", 10)]));
        assert!(outcome.cuts.is_empty());
        assert_eq!(batch.spans.len(), 10_000);
        assert_eq!(outcome.unbounded, 10_000, "the size of the bypass, in signals");
    }

    #[test]
    fn no_quotas_at_all_leaves_every_project_untouched() {
        let mut spans = one_span_traces("alpha", 4_000);
        spans.extend(one_span_traces("beta", 4_000));
        let mut batch = Batch { spans, ..Batch::default() };
        let outcome = enforce_quotas(&mut batch, &quotas(&[]));
        assert!(outcome.cuts.is_empty());
        assert_eq!(batch.spans.len(), 8_000);
        // Nothing was configured, so nothing is being walked past. A count here
        // would fire on every deployment that never set a ceiling.
        assert_eq!(outcome.unbounded, 0);
    }

    #[test]
    fn spans_are_refused_by_whole_trace_never_half_a_trace() {
        // Three traces of four spans against a ceiling of ten. Two traces fit;
        // the third would take the count to twelve, so it goes entirely.
        let spans: Vec<Span> =
            (0..3).flat_map(|t| (0..4).map(move |s| span("alpha", t, s))).collect();
        let mut batch = Batch { spans, ..Batch::default() };
        enforce_quotas(&mut batch, &quotas(&[("alpha", 10)]));

        let mut per_trace: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for s in &batch.spans {
            *per_trace.entry(s.trace_id.clone()).or_insert(0) += 1;
        }
        assert_eq!(batch.spans.len(), 8, "two whole traces, not the ten a split would give");
        assert!(
            per_trace.values().all(|n| *n == 4),
            "every stored trace is whole, got {per_trace:?}"
        );
    }

    #[test]
    fn the_message_names_the_project_the_count_and_the_number_to_raise() {
        let cut = QuotaCut { project: "alpha".to_owned(), limit: 10, rejected: 5, shared: false };
        let error = quota_error(&cut);
        let message = &error.message;
        assert!(message.contains("5 signal(s)"), "{message}");
        assert!(message.contains("project alpha"), "{message}");
        assert!(message.contains("ceiling of 10 per minute"), "{message}");
        assert!(message.contains("--quota-per-minute"), "{message}");
    }

    #[test]
    fn a_refusal_from_the_shared_ceiling_says_the_project_is_not_configured() {
        // The wording is the whole finding here. A sender told that project
        // `typo` is over its ceiling goes looking for `typo`'s setting, and
        // there is none: the answer is to add the project or raise the flag.
        let cut = QuotaCut { project: "typo".to_owned(), limit: 10, rejected: 5, shared: true };
        let message = quota_error(&cut).message;
        assert!(message.contains("no configuration lists"), "{message}");
        assert!(message.contains("shares one ceiling of 10 per minute"), "{message}");
        assert!(message.contains("`projects` block"), "{message}");
    }

    /// A store of its own for each test that needs one.
    ///
    /// Deliberately not `:memory:`: `SqliteStore` opens the in-memory database
    /// under one shared-cache name, so every in-memory store in the process is
    /// the same database and parallel tests would count each other's rows.
    async fn store() -> Arc<dyn Store> {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join("mcpd-quota-tests").join(format!(
            "{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temporary directory");
        let s = SqliteStore::open(dir.join("mcpd.db"), 2).expect("open the store");
        s.migrate().await.expect("migrate the store");
        Arc::new(s)
    }

    fn limited(projects: &[(&str, u32)]) -> ServerConfig {
        ServerConfig {
            default_project: "default".to_owned(),
            projects: projects
                .iter()
                .map(|(name, limit)| {
                    (
                        (*name).to_owned(),
                        ProjectConfig { quota_per_minute: Some(*limit), ..Default::default() },
                    )
                })
                .collect(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn the_response_tells_the_sender_how_much_was_refused_and_why() {
        // Through `finish`, which is what every ingest door calls, so this
        // covers the counting and the reporting the handlers return verbatim.
        let state = Arc::new(crate::state::AppState::new(
            store().await,
            limited(&[("alpha", 10), ("beta", 10)]),
        ));

        let mut spans = one_span_traces("alpha", 25);
        spans.extend(one_span_traces("beta", 3));
        let result =
            finish(&state, spans, Batch::default(), &Grant::All).await.expect("the batch stores");

        assert_eq!(result.accepted, 13, "alpha's ten plus beta's three");
        assert_eq!(result.rejected, 15, "alpha's excess, counted");
        let errors = result.errors.expect("a refusal always carries a reason");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("project alpha"), "{}", errors[0].message);
        assert!(errors[0].message.contains("15 signal(s)"), "{}", errors[0].message);
        assert_eq!(state.metrics.over_quota.load(Ordering::Relaxed), 15);
    }

    #[tokio::test]
    async fn an_upgrade_that_configures_no_quota_ingests_exactly_as_before() {
        // The compatibility claim, exercised rather than asserted: a default
        // configuration is what every existing deployment already runs.
        let config = ServerConfig::default();
        assert!(config.quota_per_minute.is_none());
        let state = Arc::new(crate::state::AppState::new(store().await, config));
        assert!(state.quotas.is_empty());

        let result =
            finish(&state, one_span_traces("default", 5_000), Batch::default(), &Grant::All)
                .await
                .expect("the batch stores");
        assert_eq!(result.accepted, 5_000);
        assert_eq!(result.rejected, 0);
        assert!(result.errors.is_none());
        assert_eq!(
            state.metrics.unbounded.load(Ordering::Relaxed),
            0,
            "a deployment with no ceilings has no bypass to report"
        );
    }

    #[tokio::test]
    async fn ceilings_only_under_projects_store_an_invented_name_and_count_every_signal() {
        // The gap, through the door that ingests. `alpha` has a ceiling and
        // nothing else does, which is what writing per-project limits gives
        // you. A sender that names anything else is not limited, so the daemon
        // says how much rode through instead of dropping it: refusing here
        // would break a deployment that upgraded and changed no configuration.
        let state = Arc::new(crate::state::AppState::new(store().await, limited(&[("alpha", 10)])));
        assert!(state.quotas.unnamed_are_unbounded(), "the configuration under test");

        let result = finish(
            &state,
            one_span_traces("invented-by-the-sender", 5_000),
            Batch::default(),
            &Grant::All,
        )
        .await
        .expect("the batch stores");

        assert_eq!(result.accepted, 5_000, "not one signal is dropped");
        assert_eq!(result.rejected, 0);
        assert!(result.errors.is_none(), "nothing was refused, so there is nothing to report");
        assert_eq!(
            state.metrics.unbounded.load(Ordering::Relaxed),
            5_000,
            "every unbounded signal is counted, which is how the hole is visible"
        );

        // And the ceiling that was written still holds for the project it names.
        let over = finish(&state, one_span_traces("alpha", 40), Batch::default(), &Grant::All)
            .await
            .expect("the batch stores");
        assert_eq!(over.accepted, 10, "alpha keeps the ceiling its configuration gave it");
        assert_eq!(over.rejected, 30);
        assert_eq!(
            state.metrics.unbounded.load(Ordering::Relaxed),
            5_000,
            "a project with a ceiling is not counted as unbounded"
        );
    }

    /// A daemon with a top-level ceiling and nothing in its `projects` block.
    fn ceiling(limit: u32) -> ServerConfig {
        ServerConfig {
            default_project: "default".to_owned(),
            quota_per_minute: Some(limit),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn a_project_nobody_configured_cannot_walk_past_the_ceiling() {
        // The sender picks the project name, in the body of its own request. A
        // ceiling it can step out of by typing a different string is not a
        // ceiling, and this is the batch door proving it is no longer one.
        let state = Arc::new(crate::state::AppState::new(store().await, ceiling(10)));

        let result = finish(
            &state,
            one_span_traces("not-in-any-config-file", 40),
            Batch::default(),
            &Grant::All,
        )
        .await
        .expect("the batch stores");

        assert_eq!(result.accepted, 10, "the shared ceiling, and no more");
        assert_eq!(result.rejected, 30);
        let errors = result.errors.expect("a refusal always carries a reason");
        assert!(errors[0].message.contains("no configuration lists"), "{}", errors[0].message);
        assert!(!state.quotas.unnamed_are_unbounded(), "the flag is what closes the gap");
        assert_eq!(
            state.metrics.unbounded.load(Ordering::Relaxed),
            0,
            "with the flag set every name has a counter, so nothing is unbounded"
        );
    }

    #[tokio::test]
    async fn a_flood_spread_over_invented_names_is_bounded_by_one_ceiling() {
        let state = Arc::new(crate::state::AppState::new(store().await, ceiling(10)));

        let mut spans = Vec::new();
        for n in 0..20 {
            spans.extend(one_span_traces(&format!("invented-{n}"), 10));
        }
        let result = finish(&state, spans, Batch::default(), &Grant::All).await.expect("it stores");

        assert_eq!(result.accepted, 10, "two hundred arrived across twenty names");
        assert_eq!(result.rejected, 190);
    }

    #[test]
    fn a_project_with_its_own_slot_is_not_charged_the_shared_one() {
        // The named project keeps the allowance its configuration gave it, even
        // while every invented name is spending the shared counter.
        let quotas = ProjectQuotas::new(
            std::collections::BTreeMap::from([("alpha".to_owned(), 10)]),
            Some(10),
        );
        let mut batch = Batch { spans: one_span_traces("made-up", 40), ..Batch::default() };
        assert_eq!(enforce_quotas(&mut batch, &quotas).cuts.len(), 1);
        assert_eq!(batch.spans.len(), 10);

        let mut mine = Batch { spans: one_span_traces("alpha", 10), ..Batch::default() };
        assert!(
            enforce_quotas(&mut mine, &quotas).cuts.is_empty(),
            "alpha's own ten are untouched"
        );
        assert_eq!(mine.spans.len(), 10);
    }

    // ------------------------------------------- the single-record door --

    #[tokio::test]
    async fn a_score_is_charged_to_the_same_ceiling_a_span_is() {
        // `POST /v1/api/scores` and `POST /v1/api/feedback` write a stored row
        // each and went through `Intake` but not through the counter, so a
        // project already at its ceiling could keep writing them without bound.
        let state = crate::state::AppState::new(store().await, limited(&[("alpha", 3)]));

        assert!(charge_one(&state, "alpha").is_none(), "the first is inside the ceiling");
        assert!(charge_one(&state, "alpha").is_none());
        assert!(charge_one(&state, "alpha").is_none());

        let refused = charge_one(&state, "alpha").expect("the fourth is over");
        assert!(refused.message.contains("project alpha"), "{refused}");
        assert!(refused.message.contains("ceiling of 3 per minute"), "{refused}");
        // A refusal a sender can act on says when to come back, and the window
        // is a minute, so the wait is inside one.
        assert!(
            (1..=120).contains(&refused.retry_after_seconds),
            "the wait should come from the window: {}",
            refused.retry_after_seconds
        );
        assert_eq!(state.metrics.over_quota.load(Ordering::Relaxed), 1);
        assert_eq!(state.metrics.rejected.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn a_score_shares_one_counter_with_the_spans_of_the_same_project() {
        // One ceiling per project, not one per door. A project that has spent
        // its allowance on spans has none left for scores, which is the whole
        // point of a number that bounds the disk.
        let state = Arc::new(crate::state::AppState::new(store().await, limited(&[("alpha", 5)])));
        finish(&state, one_span_traces("alpha", 5), Batch::default(), &Grant::All)
            .await
            .expect("the batch stores");

        assert!(charge_one(&state, "alpha").is_some(), "the spans spent the whole ceiling");
    }

    #[tokio::test]
    async fn the_single_record_door_never_refuses_a_project_with_no_ceiling() {
        // The default configuration, where nothing is limited. A door that
        // started refusing here would be an outage dressed as a fix.
        let state = crate::state::AppState::new(store().await, ServerConfig::default());
        for _ in 0..1_000 {
            assert!(charge_one(&state, "default").is_none());
            assert!(charge_one(&state, "anything-at-all").is_none());
        }
        assert_eq!(state.metrics.over_quota.load(Ordering::Relaxed), 0);
        assert_eq!(
            state.metrics.unbounded.load(Ordering::Relaxed),
            0,
            "no ceiling was configured, so there is no bypass of one to report"
        );
    }

    #[tokio::test]
    async fn a_score_no_ceiling_covers_is_stored_and_counted_the_way_a_span_is() {
        // The same gap through the door that writes one record. A score named
        // for a project nobody listed is not refused, for the same reason a
        // span is not, and it lands on the same counter so the size of the
        // bypass does not depend on which door the sender used.
        let state = crate::state::AppState::new(store().await, limited(&[("alpha", 3)]));
        assert!(state.quotas.unnamed_are_unbounded(), "the configuration under test");

        for _ in 0..50 {
            assert!(charge_one(&state, "invented-by-the-sender").is_none(), "never refused");
        }
        assert_eq!(state.metrics.unbounded.load(Ordering::Relaxed), 50);
        assert_eq!(state.metrics.rejected.load(Ordering::Relaxed), 0, "nothing was refused");
    }

    #[tokio::test]
    async fn a_score_named_for_an_unlisted_project_is_charged_the_shared_ceiling() {
        let state = crate::state::AppState::new(store().await, ceiling(2));
        assert!(charge_one(&state, "invented-a").is_none());
        assert!(charge_one(&state, "invented-b").is_none());
        let refused = charge_one(&state, "invented-c").expect("the shared ceiling is full");
        assert!(refused.message.contains("no configuration lists"), "{refused}");
    }
}
