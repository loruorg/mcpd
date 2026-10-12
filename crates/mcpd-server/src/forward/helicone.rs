//! Helicone, through its logging API.
//!
//! Helicone records model requests rather than traces, and it takes them over
//! its own HTTP API rather than OTLP. So this destination is sent model calls
//! only, one request per call, each in the shape the API reads: the request
//! the provider was sent, the response it gave, and when it started and
//! finished. A span of any other kind is counted as filtered, which is what it
//! is for this destination.

use super::config::Resolved;
use super::mapping::{bounded, is_model, messages};
use mcpd_core::{Span, SpanStatus};
use serde_json::{json, Value};

/// A moment as the API spells it: whole seconds and the milliseconds past.
fn moment(t: time::OffsetDateTime) -> Value {
    let ms = t.unix_timestamp_nanos() / 1_000_000;
    json!({ "seconds": ms.div_euclid(1000) as i64, "milliseconds": ms.rem_euclid(1000) as i64 })
}

/// A payload, bounded, kept as JSON when it is JSON.
fn payload(v: &Value, max: usize) -> Value {
    match v {
        Value::String(s) => json!(bounded(s, max)),
        other if other.to_string().len() > max => json!(bounded(&other.to_string(), max)),
        other => other.clone(),
    }
}

/// The spans one Helicone destination is sent, one log body each.
#[must_use]
pub fn logs(spans: &[Span], r: &Resolved) -> Vec<Value> {
    let max = r.max_value_bytes;
    spans
        .iter()
        .filter(|s| is_model(s.kind) && !s.is_event)
        .map(|s| {
            let model = s.model.clone().unwrap_or_default();
            let mut request = json!({ "model": model });
            match s.input.as_ref().map(|v| (v, messages(v, "user"))) {
                Some((_, Some(list))) => {
                    request["messages"] = list
                        .iter()
                        .map(|(role, content)| {
                            json!({ "role": role, "content": bounded(content, max) })
                        })
                        .collect();
                }
                Some((v, None)) => request["input"] = payload(v, max),
                None => {}
            }

            let mut response = match s.output.as_ref() {
                Some(Value::Object(o)) => {
                    let mut o = o.clone();
                    // A large object is cut to text rather than sent whole.
                    if Value::Object(o.clone()).to_string().len() > max {
                        o = serde_json::Map::new();
                        o.insert(
                            "output".into(),
                            payload(s.output.as_ref().unwrap_or(&Value::Null), max),
                        );
                    }
                    Value::Object(o)
                }
                Some(v) => json!({
                    "choices": [{
                        "index": 0,
                        "message": { "role": "assistant", "content": match v {
                            Value::String(t) => bounded(t, max),
                            other => bounded(&other.to_string(), max),
                        } },
                    }],
                }),
                None => json!({}),
            };
            response["model"] = json!(model);
            if let Some(u) = &s.usage {
                response["usage"] = json!({
                    "prompt_tokens": u.input_tokens,
                    "completion_tokens": u.output_tokens,
                    "total_tokens": s.total_tokens(),
                });
            }
            if let Some(e) = &s.error {
                response["error"] = json!({ "message": e.message });
            }

            let c = &s.correlation;
            let mut meta = serde_json::Map::new();
            meta.insert("Helicone-Property-Project".into(), json!(c.project));
            if let Some(v) = &c.session_id {
                meta.insert("Helicone-Session-Id".into(), json!(v));
            }
            if let Some(v) = &c.user_id {
                meta.insert("Helicone-User-Id".into(), json!(v));
            }
            if let Some(v) = &c.environment {
                meta.insert("Helicone-Property-Environment".into(), json!(v));
            }
            meta.insert("Helicone-Property-Trace-Id".into(), json!(s.trace_id));

            let end = s.end_time.unwrap_or(s.start_time);
            json!({
                "providerRequest": {
                    "url": format!("{}/{}", s.provider.as_deref().unwrap_or("custom"), model),
                    "json": request,
                    "meta": meta,
                },
                "providerResponse": {
                    "json": response,
                    "status": if s.status == SpanStatus::Error { 500 } else { 200 },
                    "headers": {},
                },
                "timing": { "startTime": moment(s.start_time), "endTime": moment(end) },
            })
        })
        .collect()
}
