//! Forwarding signals on to another OTLP endpoint.
//!
//! mcpd receives OTLP and can also emit it. That one property replaces most of
//! what a per-vendor exporter does: Langfuse, Datadog, Honeycomb, Grafana and
//! the rest all accept OTLP, so forwarding is one implementation rather than
//! eleven, and it never falls behind a vendor's SDK.
//!
//! Forwarding is **best effort and never blocks ingest**. A span is stored
//! first and queued for forwarding second; if the downstream is down, mcpd keeps
//! working and says so in its own metrics. The alternative, where a customer's
//! observability backend being unavailable stops their telemetry being kept at
//! all, is the failure mode this design exists to avoid.
//!
//! Best effort is not one attempt. A batch is retried the way the SDKs retry
//! against mcpd itself, three attempts with backoff, and only then counted in
//! `mcpd_forward_dropped_spans_total`. That counter is the whole of the
//! promise "says so in its own metrics": before it existed a drop was a line
//! in the log and nothing else.

use mcpd_core::{Span, SpanStatus};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

/// Attempts made for one batch before its spans are counted as dropped.
const ATTEMPTS: u32 = 3;

/// The wait before the second attempt. It doubles each time after. The same
/// shape the SDK contract prescribes for a client of mcpd.
const BACKOFF: Duration = Duration::from_millis(250);

/// The most ingest requests whose forwarded copy may wait on the destination
/// at once.
///
/// One request's copy is every span it stored, so this is what bounds the
/// memory forwarding can hold: this many requests' spans, however slow the
/// destination is. Sixty-four is a choice, not a measurement: room for many
/// requests to wait on a destination that is slow, and a fixed ceiling on one
/// that never answers. See [`dispatch`].
pub const MAX_IN_FLIGHT: usize = 64;

/// The longest a `Retry-After` header is honoured for.
///
/// A destination that asks for an hour is asking mcpd to hold a batch in
/// memory for an hour, on a task that shutdown will not wait that long for.
/// Past this the header is treated as the ordinary backoff.
const RETRY_AFTER_CAP: Duration = Duration::from_secs(30);

/// Where to forward, and how.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Forwarder {
    /// The OTLP/HTTP endpoint, without the signal path.
    pub endpoint: String,
    /// Headers to send, for whatever the destination uses to authenticate.
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// Stop forwarding without removing the configuration.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Most spans in one outbound request.
    #[serde(default = "default_batch")]
    pub batch_size: usize,
}

fn yes() -> bool {
    true
}
fn default_batch() -> usize {
    256
}

impl Forwarder {
    /// The endpoint as it is safe to write to a log: scheme, host, port and
    /// path, with no user, password, query or fragment.
    ///
    /// A collector's URL often carries its credential, in the user part or in
    /// a query parameter, and every line below goes to whoever reads the logs.
    #[must_use]
    pub fn shown_endpoint(&self) -> String {
        match reqwest::Url::parse(&self.endpoint) {
            Ok(mut u) => {
                let _ = u.set_username("");
                let _ = u.set_password(None);
                u.set_query(None);
                u.set_fragment(None);
                u.to_string()
            }
            Err(_) => "an endpoint that is not a URL".to_owned(),
        }
    }

    /// The URL spans are POSTed to.
    #[must_use]
    pub fn traces_url(&self) -> String {
        format!("{}/v1/traces", self.endpoint.trim_end_matches('/'))
    }
}

/// Render spans as an OTLP/JSON `ExportTraceServiceRequest`.
///
/// Grouped by service so the payload is shaped the way a receiver expects,
/// rather than one `ResourceSpans` per span, which is valid but makes every
/// downstream do the grouping again.
#[must_use]
pub fn to_otlp_json(spans: &[Span]) -> Value {
    use std::collections::BTreeMap;

    let mut by_service: BTreeMap<String, Vec<&Span>> = BTreeMap::new();
    for s in spans {
        let key = s.correlation.service_name.clone().unwrap_or_else(|| "mcpd".to_owned());
        by_service.entry(key).or_default().push(s);
    }

    let resource_spans: Vec<Value> = by_service
        .into_iter()
        .map(|(service, spans)| {
            let mut resource_attrs = vec![attr("service.name", json!(service))];
            if let Some(v) = spans[0].correlation.service_version.clone() {
                resource_attrs.push(attr("service.version", json!(v)));
            }
            if let Some(e) = spans[0].correlation.environment.clone() {
                resource_attrs.push(attr("deployment.environment.name", json!(e)));
            }

            json!({
                "resource": { "attributes": resource_attrs },
                "scopeSpans": [{
                    "scope": { "name": "mcpd", "version": mcpd_core::VERSION },
                    "spans": spans.iter().map(|s| span_json(s)).collect::<Vec<_>>(),
                }],
            })
        })
        .collect();

    json!({ "resourceSpans": resource_spans })
}

fn attr(key: &str, value: Value) -> Value {
    // OTLP/JSON types every attribute; an int must go out as a string so it
    // survives a JSON parser's 53-bit integer limit.
    let typed = match &value {
        Value::String(s) => json!({ "stringValue": s }),
        Value::Bool(b) => json!({ "boolValue": b }),
        Value::Number(n) if n.is_f64() => json!({ "doubleValue": n.as_f64() }),
        Value::Number(n) => json!({ "intValue": n.to_string() }),
        other => json!({ "stringValue": other.to_string() }),
    };
    json!({ "key": key, "value": typed })
}

fn span_json(s: &Span) -> Value {
    let mut attrs = Vec::new();

    // Re-emit under the GenAI convention, which is what a downstream receiver
    // will be looking for. mcpd's own fields are carried too, so a round trip
    // through another system and back loses nothing.
    if let Some(m) = &s.model {
        attrs.push(attr("gen_ai.request.model", json!(m)));
    }
    if let Some(p) = &s.provider {
        attrs.push(attr("gen_ai.system", json!(p)));
    }
    if let Some(u) = &s.usage {
        if let Some(v) = u.input_tokens {
            attrs.push(attr("gen_ai.usage.input_tokens", json!(v)));
        }
        if let Some(v) = u.output_tokens {
            attrs.push(attr("gen_ai.usage.output_tokens", json!(v)));
        }
    }
    if let Some(c) = &s.cost {
        if let Some(v) = c.total_cost {
            attrs.push(attr("mcpd.cost.total", json!(v)));
            attrs.push(attr("mcpd.cost.currency", json!(c.currency)));
        }
    }
    attrs.push(attr("mcpd.span.kind", json!(s.kind.as_str())));
    if let Some(n) = &s.entity_name {
        attrs.push(attr("mcpd.entity.name", json!(n)));
    }
    if let Some(v) = &s.correlation.session_id {
        attrs.push(attr("gen_ai.conversation.id", json!(v)));
    }
    if let Some(v) = &s.correlation.user_id {
        // Already hashed at ingest. Forwarding the hash rather than the
        // identity keeps the redaction promise across the boundary.
        attrs.push(attr("user.id", json!(v)));
    }
    if let Some(v) = &s.input {
        attrs.push(attr("input.value", json!(compact(v))));
    }
    if let Some(v) = &s.output {
        attrs.push(attr("output.value", json!(compact(v))));
    }
    if let Some(Value::Object(map)) = &s.attributes {
        for (k, v) in map {
            attrs.push(attr(k, v.clone()));
        }
    }

    let nanos = |t: time::OffsetDateTime| t.unix_timestamp_nanos().max(0).to_string();

    let mut out = json!({
        "traceId": s.trace_id,
        "spanId": s.span_id,
        "name": s.name,
        "kind": 1,
        "startTimeUnixNano": nanos(s.start_time),
        "endTimeUnixNano": s.end_time.map_or_else(|| "0".to_owned(), nanos),
        "attributes": attrs,
        "status": {
            "code": match s.status {
                SpanStatus::Ok => 1,
                SpanStatus::Error => 2,
                _ => 0,
            },
        },
    });

    if let Some(p) = &s.parent_span_id {
        out["parentSpanId"] = json!(p);
    }
    if let Some(e) = &s.error {
        out["status"]["message"] = json!(e.message);
    }
    out
}

/// Serialise a payload for an attribute value, bounded.
///
/// A prompt can be megabytes. Forwarding it whole would make mcpd the reason a
/// downstream ingest limit is hit.
fn compact(v: &Value) -> String {
    let s = match v {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    if s.len() <= 32 * 1024 {
        return s;
    }
    let mut end = 32 * 1024;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [{} bytes truncated]", &s[..end], s.len() - end)
}

/// Copy a batch of spans on to the configured destination.
///
/// Forwarding happens **after** the local write and never fails the request
/// that triggered it. mcpd's own storage is the thing the caller was promised;
/// a collector that is down must not turn into a rejected ingest, so a failure
/// here is retried, then logged and counted in
/// `mcpd_forward_dropped_spans_total`, never returned.
///
/// It also runs after redaction, because the pipeline redacts in place before
/// anything is written. Whatever mcpd was told to hide never reaches the next
/// hop either.
pub async fn send(state: &Arc<crate::AppState>, spans: &[Span]) {
    let Some(fwd) = state.config.forward.as_ref() else { return };
    if !fwd.enabled || spans.is_empty() {
        return;
    }

    for chunk in spans.chunks(fwd.batch_size.max(1)) {
        if !deliver(state, fwd, chunk).await {
            // By spans, not by requests. The question an operator has is how
            // much of the copy is missing, and a request is not a unit of that.
            state.metrics.forward_dropped_spans.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        }
    }
}

/// Copy spans that were just stored on to the destination, on a task of its
/// own.
///
/// On its own task, and only after the local write succeeded. A slow
/// downstream must not add its latency to the caller's request, and a span is
/// never forwarded that mcpd failed to keep. Tracked, so shutdown waits for
/// it: a bare `tokio::spawn` was dropped with the runtime, and a batch that
/// had been stored locally never reached the destination.
///
/// At most [`MAX_IN_FLIGHT`] requests' copies wait on the destination at
/// once. Each waiting task holds a full copy of its request's spans, for up to
/// three attempts of the client's timeout and a capped `Retry-After` between
/// them, and nothing bounded how many there were: a destination that accepts
/// a connection and never answers grew the daemon's memory with every ingest
/// until the process was killed. Past the bound the copy is dropped at once
/// and counted in `mcpd_forward_dropped_spans_total`, the same counter a copy
/// the destination refused lands in, so the local write and the daemon both
/// survive a destination that has stopped.
///
/// A panic while sending is caught, logged under the forwarding name, and the
/// request's spans are counted in the same counter. A chunk delivered before
/// the panic is counted too, so the counter can over-report a copy that is
/// missing and never under-report one.
pub fn dispatch(state: &Arc<crate::AppState>, spans: Vec<Span>) {
    if spans.is_empty() || !wanted(state) {
        return;
    }
    let Ok(slot) = Arc::clone(&state.forward_slots).try_acquire_owned() else {
        state.metrics.forward_dropped_spans.fetch_add(spans.len() as u64, Ordering::Relaxed);
        warn_full(spans.len());
        return;
    };
    let owned = Arc::clone(state);
    state.tasks.spawn(async move {
        // Held until the copy is delivered or given up on, then released.
        let _slot = slot;
        if !crate::run_tick("forwarding", send(&owned, &spans)).await {
            owned.metrics.forward_dropped_spans.fetch_add(spans.len() as u64, Ordering::Relaxed);
        }
    });
}

/// Whether stored spans are copied anywhere, so the ingest path can skip
/// copying them when nothing would be sent.
#[must_use]
pub fn wanted(state: &crate::AppState) -> bool {
    state.config.forward.as_ref().is_some_and(|f| f.enabled)
}

/// Say that copies are being dropped for want of room, at most once a minute.
///
/// Once a minute rather than once a request: the condition is a destination
/// that has stopped answering, every ingest meets it while it lasts, and a
/// line per request would bury the log it is meant to be read in. The counter
/// carries the exact number.
fn warn_full(spans: usize) {
    use std::sync::atomic::AtomicU64;
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = time::OffsetDateTime::now_utc().unix_timestamp().unsigned_abs();
    let last = LAST.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= 60
        && LAST.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_ok()
    {
        tracing::warn!(
            spans,
            in_flight = MAX_IN_FLIGHT,
            "{MAX_IN_FLIGHT} forwarded copies are already waiting on the destination, so new \
             copies are dropped and counted in mcpd_forward_dropped_spans_total until it \
             answers; the local writes are unaffected"
        );
    }
}

/// One batch, with retries. True when the destination accepted it.
///
/// What is retried is what the SDKs retry against mcpd: a 429, a 5xx and a
/// transport failure. Any other 4xx is final on the first answer, because the
/// batch is malformed for that destination and will stay malformed. A
/// `Retry-After` is honoured, capped, and the wait watches the shutdown token:
/// once the process is stopping, the remaining attempts are made at once
/// rather than after the wait, so a batch that was sitting out a thirty-second
/// `Retry-After` gets its last chances inside the grace period instead of
/// holding the stop to the full bound and then being dropped anyway.
async fn deliver(state: &Arc<crate::AppState>, fwd: &Forwarder, chunk: &[Span]) -> bool {
    let payload = to_otlp_json(chunk);
    let mut backoff = BACKOFF;
    for attempt in 1..=ATTEMPTS {
        let mut req = state.outbound.post(fwd.traces_url()).json(&payload);
        for (k, v) in &fwd.headers {
            req = req.header(k, v);
        }
        let (why, asked) = match req.send().await {
            Ok(res) if res.status().is_success() => return true,
            Ok(res)
                if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
                    || res.status().is_server_error() =>
            {
                (format!("answered {}", res.status()), retry_after(res.headers()))
            }
            Ok(res) => {
                tracing::warn!(
                    status = %res.status(),
                    endpoint = %fwd.shown_endpoint(),
                    spans = chunk.len(),
                    "the forwarding destination refused a batch for good; it is not retried"
                );
                return false;
            }
            // Without the URL: the error repeats it, query and all.
            Err(e) => (format!("could not be reached: {}", crate::outbound_error(e)), None),
        };
        if attempt == ATTEMPTS {
            tracing::warn!(
                endpoint = %fwd.shown_endpoint(),
                spans = chunk.len(),
                attempts = ATTEMPTS,
                "the forwarding destination {why}; the batch is dropped after every attempt"
            );
            return false;
        }
        let wait = asked.unwrap_or(backoff);
        tracing::warn!(
            endpoint = %fwd.shown_endpoint(),
            spans = chunk.len(),
            attempt,
            retry_in_ms = wait.as_millis(),
            "the forwarding destination {why}"
        );
        backoff *= 2;
        tokio::select! {
            () = state.shutdown.cancelled() => {
                tracing::info!(
                    endpoint = %fwd.shown_endpoint(),
                    spans = chunk.len(),
                    "shutting down; retrying the batch now rather than after the wait"
                );
            }
            () = tokio::time::sleep(wait) => {}
        }
    }
    false
}

/// The wait a `Retry-After` header asks for, when it asks in seconds.
///
/// The HTTP-date form is not read: a destination that sends one gets the
/// ordinary backoff, which is a shorter wait than it asked for and never a
/// longer one.
fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let secs: u64 =
        headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim().parse().ok()?;
    Some(Duration::from_secs(secs).min(RETRY_AFTER_CAP))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_logged_endpoint_carries_no_credential() {
        let fwd = super::Forwarder {
            endpoint: "https://user:s3cret@collector.example:4318/otlp?api_key=k3y#frag".into(),
            headers: Vec::new(),
            enabled: true,
            batch_size: 256,
        };
        let shown = fwd.shown_endpoint();
        assert_eq!(shown, "https://collector.example:4318/otlp");
        assert!(!shown.contains("s3cret") && !shown.contains("k3y"), "{shown}");
    }

    use super::*;
    use mcpd_core::{Correlation, SpanKind, TokenUsage};
    use time::OffsetDateTime;

    fn span(service: &str) -> Span {
        let mut s = Span {
            trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".into(),
            span_id: "00f067aa0ba902b7".into(),
            parent_span_id: None,
            external_parent_span_id: None,
            name: "chat".into(),
            kind: SpanKind::ModelGeneration,
            status: SpanStatus::Ok,
            start_time: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1_700_000_000),
            end_time: Some(OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1_700_000_002)),
            duration_ns: None,
            is_root: true,
            is_event: false,
            entity_kind: None,
            entity_id: None,
            entity_name: Some("researcher".into()),
            input: Some(json!({ "q": "hello" })),
            output: Some(json!("hi")),
            attributes: None,
            metadata: None,
            usage: Some(TokenUsage {
                input_tokens: Some(1200),
                output_tokens: Some(64),
                total_tokens: None,
                input_details: None,
                output_details: None,
            }),
            cost: None,
            model_parameters: None,
            error: None,
            links: None,
            tags: None,
            correlation: Correlation {
                project: "default".into(),
                environment: Some("production".into()),
                service_name: Some(service.into()),
                service_version: Some("1.0.0".into()),
                session_id: Some("sess-1".into()),
                thread_id: None,
                user_id: Some("sha256:abc".into()),
                resource_id: None,
                run_id: None,
                request_id: None,
                experiment_id: None,
                release: None,
            },
            model: Some("gpt-4o".into()),
            provider: Some("openai".into()),
            received_at: OffsetDateTime::UNIX_EPOCH,
            ingest_key: None,
        };
        s.normalise();
        s
    }

    fn attrs_of(v: &Value) -> std::collections::BTreeMap<String, Value> {
        v["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|kv| (kv["key"].as_str().unwrap().to_owned(), kv["value"].clone()))
            .collect()
    }

    #[test]
    fn what_goes_out_is_what_mcpd_accepts_back() {
        // The strongest guarantee available: forward a span, decode the payload
        // with mcpd's own OTLP reader, and check it maps to the same span.
        let out = to_otlp_json(&[span("checkout")]);
        let decoded = mcpd_core::otlp::decode_json(&out).unwrap();
        assert_eq!(decoded.len(), 1);
        let back = mcpd_core::otlp::to_span(&decoded[0], "default", OffsetDateTime::UNIX_EPOCH);

        assert_eq!(back.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(back.span_id, "00f067aa0ba902b7");
        assert_eq!(back.kind, SpanKind::ModelGeneration);
        assert_eq!(back.model.as_deref(), Some("gpt-4o"));
        assert_eq!(back.provider.as_deref(), Some("openai"));
        assert_eq!(back.usage.unwrap().input_tokens, Some(1200));
        assert_eq!(back.correlation.service_name.as_deref(), Some("checkout"));
        assert_eq!(back.correlation.session_id.as_deref(), Some("sess-1"));
        assert_eq!(back.duration_ns, Some(2_000_000_000));
    }

    #[test]
    fn spans_are_grouped_by_service_not_one_resource_each() {
        let out = to_otlp_json(&[span("a"), span("b"), span("a")]);
        let rs = out["resourceSpans"].as_array().unwrap();
        assert_eq!(rs.len(), 2, "two services, two ResourceSpans");
        let counts: Vec<usize> =
            rs.iter().map(|r| r["scopeSpans"][0]["spans"].as_array().unwrap().len()).collect();
        assert_eq!(counts.iter().sum::<usize>(), 3);
    }

    #[test]
    fn large_integers_go_out_as_strings() {
        // JSON's 53-bit limit would silently mangle a token count otherwise.
        let out = to_otlp_json(&[span("x")]);
        let a = attrs_of(&out);
        assert_eq!(a["gen_ai.usage.input_tokens"]["intValue"], json!("1200"));
    }

    #[test]
    fn the_hashed_user_id_crosses_the_boundary_still_hashed() {
        let a = attrs_of(&to_otlp_json(&[span("x")]));
        assert_eq!(a["user.id"]["stringValue"], json!("sha256:abc"));
    }

    #[test]
    fn an_enormous_payload_is_truncated_not_forwarded_whole() {
        let mut s = span("x");
        s.input = Some(json!("y".repeat(200_000)));
        let a = attrs_of(&to_otlp_json(&[s]));
        let v = a["input.value"]["stringValue"].as_str().unwrap();
        assert!(v.len() < 40_000, "forwarded {} bytes", v.len());
        assert!(v.ends_with("truncated]"));
    }

    #[test]
    fn an_error_carries_its_message() {
        let mut s = span("x");
        s.status = SpanStatus::Error;
        s.error = Some(mcpd_core::SpanError {
            message: "429 from provider".into(),
            kind: None,
            stack: None,
            domain: None,
            retryable: None,
            details: None,
        });
        let out = to_otlp_json(&[s]);
        let st = &out["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["status"];
        assert_eq!(st["code"], json!(2));
        assert_eq!(st["message"], json!("429 from provider"));
    }

    #[test]
    fn a_retry_after_in_seconds_is_honoured_and_capped() {
        let mut h = reqwest::header::HeaderMap::new();
        assert_eq!(retry_after(&h), None);
        h.insert(reqwest::header::RETRY_AFTER, "2".parse().unwrap());
        assert_eq!(retry_after(&h), Some(Duration::from_secs(2)));
        h.insert(reqwest::header::RETRY_AFTER, "3600".parse().unwrap());
        assert_eq!(retry_after(&h), Some(RETRY_AFTER_CAP));
        // The date form is not read, so the ordinary backoff applies.
        h.insert(reqwest::header::RETRY_AFTER, "Wed, 21 Oct 2015 07:28:00 GMT".parse().unwrap());
        assert_eq!(retry_after(&h), None);
    }

    #[test]
    fn the_traces_url_is_built_without_a_double_slash() {
        let f = Forwarder {
            endpoint: "https://collector.example/".into(),
            headers: vec![],
            enabled: true,
            batch_size: 256,
        };
        assert_eq!(f.traces_url(), "https://collector.example/v1/traces");
    }
}
