//! Zipkin v2 ingest.
//!
//! Zipkin's v2 span is flat, and both of its encodings, JSON and proto3, carry
//! the same fields. Each decoder reads into [`ZipkinSpan`] and one function,
//! [`lower`], turns that into the [`OtlpSpan`] the OpenTelemetry doors produce.
//! From there the span takes the same road as any OTLP span, so the semantic
//! conventions a Zipkin tracer writes as tags (`gen_ai.*`, `llm.*`,
//! `http.method`) are read by the one mapping in [`crate::otlp`].
//!
//! What each Zipkin field becomes:
//!
//! - `localEndpoint.serviceName` is the resource's `service.name`.
//! - `tags` are span attributes, as strings, because that is what Zipkin sends.
//! - `kind` is the `span.kind` attribute, lowercased, as Jaeger writes it.
//! - `annotations` are span events, each named by its value.
//! - `remoteEndpoint` is `peer.service`, `network.peer.address` and
//!   `network.peer.port`.
//! - the `error` tag, or `otel.status_code`, sets the status.
//! - `debug` sets the sampled bit, so a forced trace is kept by the sampler.
//! - `shared` marks a server span that reused its client's span id. mcpd keys
//!   a span on its id, so the server half gets an id derived from the shared
//!   one and is parented to it, and both halves are kept.

use crate::otlp::protobuf::{Reader, Wire};
use crate::otlp::{OtlpEvent, OtlpSpan, MAX_PER_SPAN};
use crate::{Error, Result};
use serde_json::{Map, Value};

/// One endpoint, local or remote.
#[derive(Debug, Clone, Default)]
pub struct Endpoint {
    /// The service name, lowercased by most tracers.
    pub service_name: Option<String>,
    /// The IPv4 or IPv6 address, as text.
    pub address: Option<String>,
    /// The port, when one was sent.
    pub port: Option<u16>,
}

/// A Zipkin v2 span, as either encoding carries it.
#[derive(Debug, Clone, Default)]
pub struct ZipkinSpan {
    /// Lowercase hex, 32 characters once padded.
    pub trace_id: String,
    /// Lowercase hex, 16 characters.
    pub id: String,
    /// Lowercase hex, when not a root.
    pub parent_id: Option<String>,
    /// `CLIENT`, `SERVER`, `PRODUCER` or `CONSUMER`.
    pub kind: Option<String>,
    /// The span name.
    pub name: String,
    /// Start, microseconds since the Unix epoch.
    pub timestamp_us: u64,
    /// Duration in microseconds. Absent on a span that has not finished.
    pub duration_us: Option<u64>,
    /// The service that recorded the span.
    pub local: Endpoint,
    /// The other side of the call, when known.
    pub remote: Endpoint,
    /// `(microseconds, value)` pairs.
    pub annotations: Vec<(u64, String)>,
    /// String tags.
    pub tags: Map<String, Value>,
    /// The sender asked for this trace to be kept.
    pub debug: bool,
    /// A server span sharing its client's span id.
    pub shared: bool,
}

/// A hex id, lowercased and padded to `bytes` on the left.
///
/// Zipkin allows a 64-bit trace id, and a 128-bit id written by a tracer that
/// dropped leading zeros is still the same id. Padding both to the width mcpd
/// stores makes each one the id OpenTelemetry would give the same trace.
#[must_use]
pub(crate) fn hex_id(raw: &str, bytes: usize) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > bytes * 2 || !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let lower = raw.to_ascii_lowercase();
    if lower.bytes().all(|b| b == b'0') {
        return None;
    }
    Some(format!("{lower:0>width$}", width = bytes * 2))
}

/// The same, for an id sent as raw bytes, as proto3 sends it.
#[must_use]
pub(crate) fn hex_id_bytes(raw: &[u8], bytes: usize) -> Option<String> {
    if raw.is_empty() || raw.len() > bytes {
        return None;
    }
    hex_id(&hex::encode(raw), bytes)
}

/// Decode a Zipkin v2 JSON body, which is an array of spans.
///
/// A span whose ids will not read is skipped rather than failing the batch,
/// the rule every other door applies.
///
/// # Errors
/// Returns [`Error::Otlp`] when the body is not an array.
pub fn decode_json(body: &Value) -> Result<Vec<ZipkinSpan>> {
    let list = body
        .as_array()
        .ok_or_else(|| Error::Otlp("a Zipkin v2 body is a JSON array of spans".into()))?;
    Ok(list.iter().filter_map(json_span).collect())
}

fn json_endpoint(v: Option<&Value>) -> Endpoint {
    let Some(v) = v else { return Endpoint::default() };
    let text = |k: &str| v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
    Endpoint {
        service_name: text("serviceName").map(str::to_owned),
        address: text("ipv4").or_else(|| text("ipv6")).map(str::to_owned),
        port: v.get("port").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok()),
    }
}

/// A JSON number that may arrive as a string, which some tracers send for
/// values past 2^53.
fn json_u64(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().map(|f| f as u64)),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn json_span(v: &Value) -> Option<ZipkinSpan> {
    let id_of = |k: &str, bytes| v.get(k).and_then(Value::as_str).and_then(|s| hex_id(s, bytes));
    let mut tags = Map::new();
    if let Some(t) = v.get("tags").and_then(Value::as_object) {
        for (k, value) in t {
            let text = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            tags.insert(k.clone(), Value::String(text));
        }
    }
    let annotations = v
        .get("annotations")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|a| {
                    let value = a.get("value").and_then(Value::as_str)?.to_owned();
                    Some((json_u64(a.get("timestamp")).unwrap_or(0), value))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(ZipkinSpan {
        trace_id: id_of("traceId", 16)?,
        id: id_of("id", 8)?,
        parent_id: id_of("parentId", 8),
        kind: v.get("kind").and_then(Value::as_str).map(str::to_owned),
        name: v.get("name").and_then(Value::as_str).unwrap_or("").to_owned(),
        timestamp_us: json_u64(v.get("timestamp")).unwrap_or(0),
        duration_us: json_u64(v.get("duration")),
        local: json_endpoint(v.get("localEndpoint")),
        remote: json_endpoint(v.get("remoteEndpoint")),
        annotations,
        tags,
        debug: v.get("debug").and_then(Value::as_bool).unwrap_or(false),
        shared: v.get("shared").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// Decode a Zipkin proto3 `ListOfSpans`.
///
/// # Errors
/// Returns [`Error::Otlp`] only when the outer message cannot be walked.
pub fn decode_proto(body: &[u8]) -> Result<Vec<ZipkinSpan>> {
    let mut out = Vec::new();
    let mut r = Reader::new(body);
    while !r.done() {
        let (f, w) = r.tag()?;
        if (f, w) == (1, Wire::Bytes) {
            if let Ok(Some(span)) = proto_span(r.bytes()?) {
                out.push(span);
            }
        } else {
            r.skip(w)?;
        }
    }
    Ok(out)
}

fn proto_endpoint(buf: &[u8]) -> Result<Endpoint> {
    let mut out = Endpoint::default();
    let mut r = Reader::new(buf);
    while !r.done() {
        let (f, w) = r.tag()?;
        match (f, w) {
            (1, Wire::Bytes) => out.service_name = Some(r.string()?).filter(|s| !s.is_empty()),
            (2, Wire::Bytes) => {
                if let Ok(octets) = <[u8; 4]>::try_from(r.bytes()?) {
                    out.address = Some(std::net::Ipv4Addr::from(octets).to_string());
                }
            }
            (3, Wire::Bytes) => {
                if let Ok(octets) = <[u8; 16]>::try_from(r.bytes()?) {
                    out.address.get_or_insert(std::net::Ipv6Addr::from(octets).to_string());
                }
            }
            (4, Wire::Varint) => out.port = u16::try_from(r.varint()?).ok(),
            (_, w) => r.skip(w)?,
        }
    }
    Ok(out)
}

fn proto_span(buf: &[u8]) -> Result<Option<ZipkinSpan>> {
    const KINDS: [&str; 5] = ["", "CLIENT", "SERVER", "PRODUCER", "CONSUMER"];
    let mut out = ZipkinSpan::default();
    let mut trace = None;
    let mut id = None;
    let mut r = Reader::new(buf);
    while !r.done() {
        let (f, w) = r.tag()?;
        match (f, w) {
            (1, Wire::Bytes) => trace = hex_id_bytes(r.bytes()?, 16),
            (2, Wire::Bytes) => out.parent_id = hex_id_bytes(r.bytes()?, 8),
            (3, Wire::Bytes) => id = hex_id_bytes(r.bytes()?, 8),
            (4, Wire::Varint) => {
                out.kind = KINDS
                    .get(usize::try_from(r.varint()?).unwrap_or(0))
                    .filter(|k| !k.is_empty())
                    .map(|k| (*k).to_owned());
            }
            (5, Wire::Bytes) => out.name = r.string()?,
            (6, Wire::Fixed64) => out.timestamp_us = r.fixed64()?,
            (7, Wire::Varint) => out.duration_us = Some(r.varint()?),
            (8, Wire::Bytes) => out.local = proto_endpoint(r.bytes()?)?,
            (9, Wire::Bytes) => out.remote = proto_endpoint(r.bytes()?)?,
            (10, Wire::Bytes) => {
                let mut a = Reader::new(r.bytes()?);
                let (mut ts, mut value) = (0, String::new());
                while !a.done() {
                    let (af, aw) = a.tag()?;
                    match (af, aw) {
                        (1, Wire::Fixed64) => ts = a.fixed64()?,
                        (2, Wire::Bytes) => value = a.string()?,
                        (_, w) => a.skip(w)?,
                    }
                }
                out.annotations.push((ts, value));
            }
            (11, Wire::Bytes) => {
                // map<string, string> is a repeated entry { key = 1; value = 2 }.
                let mut e = Reader::new(r.bytes()?);
                let (mut key, mut value) = (String::new(), String::new());
                while !e.done() {
                    let (ef, ew) = e.tag()?;
                    match (ef, ew) {
                        (1, Wire::Bytes) => key = e.string()?,
                        (2, Wire::Bytes) => value = e.string()?,
                        (_, w) => e.skip(w)?,
                    }
                }
                if !key.is_empty() {
                    out.tags.insert(key, Value::String(value));
                }
            }
            (12, Wire::Varint) => out.debug = r.varint()? != 0,
            (13, Wire::Varint) => out.shared = r.varint()? != 0,
            (_, w) => r.skip(w)?,
        }
    }
    let (Some(trace), Some(id)) = (trace, id) else { return Ok(None) };
    out.trace_id = trace;
    out.id = id;
    Ok(Some(out))
}

/// The span id a shared server span is stored under.
///
/// Derived from the shared id, so a resent span lands on the same row, and
/// never equal to it, so it does not replace the client half.
fn shared_span_id(trace_id: &str, id: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"mcpd zipkin shared span v1\0");
    h.update(trace_id.as_bytes());
    h.update([0]);
    h.update(id.as_bytes());
    let digest = h.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    // The all-zero id is reserved; see `otlp::event_span_id`.
    bytes[0] |= 1;
    hex::encode(bytes)
}

/// Turn a Zipkin span into the shape every trace door maps from.
#[must_use]
pub fn lower(z: &ZipkinSpan) -> OtlpSpan {
    let mut resource = Map::new();
    if let Some(service) = &z.local.service_name {
        resource.insert("service.name".into(), Value::String(service.clone()));
    }

    let mut attributes = z.tags.clone();
    if let Some(kind) = &z.kind {
        attributes.insert("span.kind".into(), Value::String(kind.to_ascii_lowercase()));
    }
    if let Some(peer) = &z.remote.service_name {
        attributes.insert("peer.service".into(), Value::String(peer.clone()));
    }
    if let Some(address) = &z.remote.address {
        attributes.insert("network.peer.address".into(), Value::String(address.clone()));
    }
    if let Some(port) = z.remote.port {
        attributes.insert("network.peer.port".into(), Value::from(port));
    }
    if let Some(address) = &z.local.address {
        attributes.insert("network.local.address".into(), Value::String(address.clone()));
    }
    if let Some(port) = z.local.port {
        attributes.insert("network.local.port".into(), Value::from(port));
    }

    // Zipkin marks a failure with an `error` tag whose value is the message,
    // or "true" when there is none. A tracer bridged from OpenTelemetry writes
    // `otel.status_code` as well, which also says when a span succeeded.
    let tag = |k: &str| z.tags.get(k).and_then(Value::as_str);
    let (status_code, status_message) = match (tag("otel.status_code"), tag("error")) {
        (Some(code), _) if code.eq_ignore_ascii_case("ok") => (1, None),
        (Some(code), err) if code.eq_ignore_ascii_case("error") => (
            2,
            tag("otel.status_description")
                .or(err)
                .filter(|m| !m.is_empty() && *m != "true")
                .map(str::to_owned),
        ),
        (_, Some(err)) if err != "false" => {
            (2, Some(err.to_owned()).filter(|m| !m.is_empty() && m != "true"))
        }
        _ => (0, None),
    };

    let start = u128::from(z.timestamp_us) * 1_000;
    let end = match z.duration_us {
        Some(d) if start > 0 => start + u128::from(d) * 1_000,
        _ => 0,
    };

    let (span_id, parent_span_id) = if z.shared {
        (shared_span_id(&z.trace_id, &z.id), Some(z.id.clone()))
    } else {
        (z.id.clone(), z.parent_id.clone())
    };

    let name = if z.name.is_empty() {
        z.kind.as_deref().map_or_else(|| "span".to_owned(), str::to_ascii_lowercase)
    } else {
        z.name.clone()
    };

    OtlpSpan {
        trace_id: z.trace_id.clone(),
        span_id,
        parent_span_id,
        name,
        start_unix_nano: start,
        end_unix_nano: end,
        attributes,
        resource,
        status_code,
        status_message,
        flags: u32::from(z.debug),
        links: Vec::new(),
        events: z
            .annotations
            .iter()
            .take(MAX_PER_SPAN)
            .map(|(ts, value)| OtlpEvent {
                time_unix_nano: u128::from(*ts) * 1_000,
                name: if value.is_empty() { "annotation".to_owned() } else { value.clone() },
                attributes: Map::new(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_json_span_lowers_with_every_field() {
        let body = json!([{
            "traceId": "463ac35c9f6413ad",
            "parentId": "463ac35c9f6413ad",
            "id": "72485a3953bb6124",
            "kind": "CLIENT",
            "name": "get /api",
            "timestamp": 1_556_604_172_355_737u64,
            "duration": 1431,
            "debug": true,
            "localEndpoint": { "serviceName": "frontend", "ipv4": "192.168.99.1", "port": 3306 },
            "remoteEndpoint": { "serviceName": "backend", "ipv4": "172.19.0.2", "port": 9000 },
            "annotations": [{ "timestamp": 1_556_604_172_355_800u64, "value": "ws" }],
            "tags": { "http.method": "GET", "error": "connection reset" }
        }]);
        let spans = decode_json(&body).unwrap();
        assert_eq!(spans.len(), 1);
        let o = lower(&spans[0]);
        assert_eq!(o.trace_id, "0000000000000000463ac35c9f6413ad");
        assert_eq!(o.span_id, "72485a3953bb6124");
        assert_eq!(o.parent_span_id.as_deref(), Some("463ac35c9f6413ad"));
        assert_eq!(o.resource["service.name"], "frontend");
        assert_eq!(o.attributes["span.kind"], "client");
        assert_eq!(o.attributes["peer.service"], "backend");
        assert_eq!(o.attributes["network.peer.port"], 9000);
        assert_eq!(o.attributes["http.method"], "GET");
        assert_eq!(o.status_code, 2);
        assert_eq!(o.status_message.as_deref(), Some("connection reset"));
        assert_eq!(o.start_unix_nano, 1_556_604_172_355_737_000);
        assert_eq!(o.end_unix_nano, 1_556_604_172_355_737_000 + 1_431_000);
        assert_eq!(o.flags, 1);
        assert_eq!(o.events.len(), 1);
        assert_eq!(o.events[0].name, "ws");
    }

    #[test]
    fn a_shared_span_keeps_both_halves() {
        let z = ZipkinSpan {
            trace_id: hex_id("1", 16).unwrap(),
            id: hex_id("2", 8).unwrap(),
            parent_id: hex_id("3", 8),
            shared: true,
            ..ZipkinSpan::default()
        };
        let o = lower(&z);
        assert_ne!(o.span_id, z.id);
        assert_eq!(o.parent_span_id.as_deref(), Some(z.id.as_str()));
        assert_eq!(o.span_id, lower(&z).span_id, "a resend must land on the same row");
    }

    #[test]
    fn a_span_with_no_usable_id_is_skipped() {
        let body = json!([{ "traceId": "zz", "id": "1" }, { "traceId": "1", "id": "0" }]);
        assert!(decode_json(&body).unwrap().is_empty());
        assert!(decode_json(&json!({})).is_err());
    }

    #[test]
    fn an_unfinished_span_has_no_end() {
        let z = ZipkinSpan {
            trace_id: hex_id("1", 16).unwrap(),
            id: hex_id("2", 8).unwrap(),
            timestamp_us: 10,
            ..ZipkinSpan::default()
        };
        assert_eq!(lower(&z).end_unix_nano, 0);
    }
}
