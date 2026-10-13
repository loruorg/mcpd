//! OpenTelemetry ingest.
//!
//! This module is the reason mcpd works with code it has never heard of. Point
//! any OpenTelemetry SDK at `/v1/traces` and its spans become mcpd spans, with
//! the two semantic conventions the AI ecosystem actually uses — OpenTelemetry's
//! `gen_ai.*` and Arize's OpenInference `llm.*` — mapped onto the native model.
//!
//! Both encodings are decoded here: JSON in this file, protobuf in
//! [`protobuf`]. Both lower into the same [`OtlpSpan`], so the mapping below is
//! the single place semantic conventions are interpreted and the two encodings
//! cannot drift apart.
//!
//! One structural decision is worth knowing before you read: OpenTelemetry
//! records a point in time inside a span as a span *event*, and mcpd models a
//! point in time as a span of its own with `is_event` set. [`to_spans`] is
//! where an OTLP request becomes mcpd spans, and it is the function the ingest
//! door calls; [`to_span`] maps a single span and cannot answer either of the
//! questions that need the whole request.

use crate::{
    ids, Correlation, InputTokenDetails, OutputTokenDetails, Span, SpanError, SpanKind, SpanStatus,
    TokenUsage,
};
use serde_json::{Map, Value};
use time::OffsetDateTime;

pub mod protobuf;

/// W3C trace-flags bit 0, carried in the low byte of OTLP's `flags`: the
/// upstream sampled this trace.
const TRACE_FLAG_SAMPLED: u32 = 0x0000_0001;

/// OTLP's `SPAN_FLAGS_CONTEXT_HAS_IS_REMOTE_MASK`. When this bit is clear the
/// producer said nothing about the parent, which is not the same as saying the
/// parent is local.
const SPAN_FLAG_HAS_IS_REMOTE: u32 = 0x0000_0100;

/// OTLP's `SPAN_FLAGS_CONTEXT_IS_REMOTE_MASK`: the parent context arrived from
/// another process, rather than being a span the same tracer created.
const SPAN_FLAG_IS_REMOTE: u32 = 0x0000_0200;

/// How many links or events mcpd reads from a single span.
///
/// An event becomes a stored span, so an unbounded list is a fan-out rather
/// than a large field: a thousand spans carrying a thousand events each is a
/// million rows out of one request. 128 is what the OpenTelemetry SDKs cap
/// their own per-span link and event counts at by default, so a conforming
/// producer is never truncated by this.
const MAX_PER_SPAN: usize = 128;

/// The attribute `to_span` records OTLP's `flags` under.
///
/// The mcpd span model has no column for it, and two decisions need it after
/// the door has closed: the sampler must honour an upstream `sampled` bit
/// ([`upstream_sampled`]), and [`to_spans`] reads the "parent is remote" bits.
/// This is the only attribute mcpd adds to a producer's own, and keeping it is
/// also what lets a reader explain either decision afterwards.
pub const FLAGS_ATTRIBUTE: &str = "mcpd.otlp.flags";

/// A decoded OTLP span, independent of whether it arrived as JSON or protobuf.
#[derive(Debug, Clone, Default)]
pub struct OtlpSpan {
    /// Hex trace id.
    pub trace_id: String,
    /// Hex span id.
    pub span_id: String,
    /// Hex parent span id, when not a root.
    pub parent_span_id: Option<String>,
    /// Span name.
    pub name: String,
    /// Start time, nanoseconds since the Unix epoch.
    pub start_unix_nano: u128,
    /// End time, nanoseconds since the Unix epoch. Zero means still running.
    pub end_unix_nano: u128,
    /// Flattened span attributes.
    pub attributes: Map<String, Value>,
    /// Flattened resource attributes, which carry service identity.
    pub resource: Map<String, Value>,
    /// Status code: 0 unset, 1 ok, 2 error.
    pub status_code: i32,
    /// Status message, usually an error string.
    pub status_message: Option<String>,
    /// OTLP's `flags`. Bits 0 to 7 are the W3C trace flags, so bit 0 is the
    /// sampled bit; bits 8 and 9 say whether the parent context is remote.
    ///
    /// Zero means the producer stated nothing, not "not sampled". OTLP says a
    /// reader must not assume any bit is zero when the whole value is zero,
    /// because a field left at its proto3 default is absent from the wire.
    pub flags: u32,
    /// Causal references this span declared to other spans or traces.
    pub links: Vec<OtlpLink>,
    /// Point-in-time records inside this span.
    pub events: Vec<OtlpEvent>,
}

impl OtlpSpan {
    /// Whether the flags state that this span's parent context is remote.
    ///
    /// `None` when the producer never set the "has is remote" bit, which is
    /// the ordinary case for an SDK older than OTLP 1.1 and is different from
    /// stating that the parent is local.
    #[must_use]
    pub const fn parent_is_remote(&self) -> Option<bool> {
        if self.flags & SPAN_FLAG_HAS_IS_REMOTE == 0 {
            None
        } else {
            Some(self.flags & SPAN_FLAG_IS_REMOTE != 0)
        }
    }
}

/// A link decoded from an OTLP span, before it becomes a [`crate::SpanLink`].
#[derive(Debug, Clone, Default)]
pub struct OtlpLink {
    /// Hex trace id of whatever this link points at.
    pub trace_id: String,
    /// Hex span id, when the link names one span rather than a whole trace.
    pub span_id: Option<String>,
    /// The link's own attributes.
    pub attributes: Map<String, Value>,
}

/// A point-in-time record inside an OTLP span.
///
/// OpenTelemetry calls these span events. `span.recordException(e)` produces
/// one named `exception`, and it is the only place a stack trace appears in a
/// stock OpenTelemetry SDK.
#[derive(Debug, Clone, Default)]
pub struct OtlpEvent {
    /// When it happened, nanoseconds since the Unix epoch. Zero when the
    /// producer did not say.
    pub time_unix_nano: u128,
    /// The event name. `exception` is the one OpenTelemetry gives a meaning.
    pub name: String,
    /// The event's own attributes.
    pub attributes: Map<String, Value>,
}

/// Read an OTLP/JSON `ExportTraceServiceRequest` into flat spans.
///
/// Malformed spans are skipped rather than failing the whole batch: a
/// collector that rejects ten thousand good spans because one was bad is a
/// collector people turn off.
///
/// # Errors
/// Returns [`crate::Error::Otlp`] only when the top-level document is not the
/// expected shape.
pub fn decode_json(body: &Value) -> crate::Result<Vec<OtlpSpan>> {
    let resource_spans = body
        .get("resourceSpans")
        .or_else(|| body.get("resource_spans"))
        .and_then(Value::as_array)
        .ok_or_else(|| crate::Error::Otlp("missing resourceSpans".into()))?;

    let mut out = Vec::new();
    for rs in resource_spans {
        let resource = rs
            .get("resource")
            .and_then(|r| r.get("attributes"))
            .map(flatten_attributes)
            .unwrap_or_default();

        let scope_spans = rs
            .get("scopeSpans")
            .or_else(|| rs.get("scope_spans"))
            .and_then(Value::as_array)
            .map_or(&[][..], |v| v.as_slice());

        for ss in scope_spans {
            let spans = ss.get("spans").and_then(Value::as_array).map_or(&[][..], |v| v.as_slice());
            for s in spans {
                if let Some(span) = decode_one(s, &resource) {
                    out.push(span);
                }
            }
        }
    }
    Ok(out)
}

fn decode_one(s: &Value, resource: &Map<String, Value>) -> Option<OtlpSpan> {
    let trace_id = read_id(s, "traceId", "trace_id", 16)?;
    let span_id = read_id(s, "spanId", "span_id", 8)?;
    let parent_span_id = read_id(s, "parentSpanId", "parent_span_id", 8);

    Some(OtlpSpan {
        trace_id,
        span_id,
        parent_span_id,
        name: s.get("name").and_then(Value::as_str).unwrap_or("span").to_owned(),
        start_unix_nano: read_nano(s, "startTimeUnixNano", "start_time_unix_nano"),
        end_unix_nano: read_nano(s, "endTimeUnixNano", "end_time_unix_nano"),
        attributes: s.get("attributes").map(flatten_attributes).unwrap_or_default(),
        resource: resource.clone(),
        status_code: s
            .get("status")
            .and_then(|st| st.get("code"))
            .and_then(Value::as_i64)
            .unwrap_or(0) as i32,
        status_message: s
            .get("status")
            .and_then(|st| st.get("message"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        flags: read_flags(s),
        links: read_links(s),
        events: read_events(s),
    })
}

/// OTLP's `flags`, declared as a `uint32`.
///
/// OTLP/JSON sends a 32-bit integer as a number, but an exporter that renders
/// every integer the way it must render the 64-bit ones sends a string, so both
/// are read. Anything else, including a value too large to be a `uint32`, is
/// read as "the producer said nothing", which is the same as absent.
fn read_flags(v: &Value) -> u32 {
    match v.get("flags") {
        Some(Value::Number(n)) => n.as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

fn read_links(s: &Value) -> Vec<OtlpLink> {
    let Some(list) = s.get("links").and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .take(MAX_PER_SPAN)
        .filter_map(|l| {
            // A link whose trace id will not validate points at nothing that
            // can ever be looked up, so it is dropped rather than stored as a
            // reference a reader will follow to an empty page. The span id is
            // optional in mcpd's model, so a link that names only a trace is a
            // link to the whole trace rather than a malformed one.
            Some(OtlpLink {
                trace_id: read_id(l, "traceId", "trace_id", 16)?,
                span_id: read_id(l, "spanId", "span_id", 8),
                attributes: l.get("attributes").map(flatten_attributes).unwrap_or_default(),
            })
        })
        .collect()
}

fn read_events(s: &Value) -> Vec<OtlpEvent> {
    let Some(list) = s.get("events").and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .take(MAX_PER_SPAN)
        // An entry that is not an object describes no event. It is dropped
        // rather than read as an empty one, which is what the protobuf reader
        // does with a frame it cannot walk: a record nobody can read must not
        // become a nameless span at the epoch.
        .filter(|e| e.is_object())
        .map(|e| OtlpEvent {
            time_unix_nano: read_nano(e, "timeUnixNano", "time_unix_nano"),
            name: match e.get("name").and_then(Value::as_str) {
                Some(n) if !n.is_empty() => n.to_owned(),
                // The same fallback the protobuf reader applies, so a nameless
                // event does not become a nameless span on one path only.
                _ => "event".to_owned(),
            },
            attributes: e.get("attributes").map(flatten_attributes).unwrap_or_default(),
        })
        .collect()
}

/// OTLP/JSON specifies hex for id fields, but several SDKs emit base64.
/// Accept both rather than silently dropping half the ecosystem's traffic.
fn read_id(v: &Value, camel: &str, snake: &str, bytes: usize) -> Option<String> {
    let raw = v.get(camel).or_else(|| v.get(snake))?.as_str()?;
    if raw.is_empty() {
        return None;
    }
    let hex_len = bytes * 2;
    if raw.len() == hex_len && raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        let lower = raw.to_ascii_lowercase();
        return (!lower.bytes().all(|b| b == b'0')).then_some(lower);
    }
    let decoded = base64_decode(raw)?;
    if decoded.len() != bytes || decoded.iter().all(|b| *b == 0) {
        return None;
    }
    Some(hex::encode(decoded))
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rev = [255u8; 256];
    for (i, c) in T.iter().enumerate() {
        rev[*c as usize] = i as u8;
    }
    rev[b'-' as usize] = 62;
    rev[b'_' as usize] = 63;

    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for b in s.bytes() {
        if b == b'=' || b == b'\n' || b == b'\r' {
            continue;
        }
        let v = rev[b as usize];
        if v == 255 {
            return None;
        }
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

fn read_nano(v: &Value, camel: &str, snake: &str) -> u128 {
    // Nanosecond timestamps exceed 2^53, so OTLP/JSON sends them as strings.
    // Reading them as f64 silently loses precision, which shows up as spans
    // that appear to start microseconds apart when they did not.
    let raw = v.get(camel).or_else(|| v.get(snake));
    match raw {
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        Some(Value::Number(n)) => n.as_u64().map_or(0, u128::from),
        _ => 0,
    }
}

/// Flatten OTLP's `[{key, value:{stringValue}}]` list into a plain map.
pub fn flatten_attributes(v: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    if let Some(list) = v.as_array() {
        for kv in list {
            let Some(k) = kv.get("key").and_then(Value::as_str) else { continue };
            // Same rule as the protobuf reader in `protobuf.rs`: an attribute
            // with no key is dropped, and a key with no value is kept as null.
            // The two decoders used to keep the opposite halves of a degenerate
            // KeyValue, so one batch re-exported over the other protocol came
            // back with a different attribute map. The module header says the
            // two paths produce the same span, and that has to be true of the
            // malformed cases as well or it is not a guarantee.
            if k.is_empty() {
                continue;
            }
            out.insert(k.to_owned(), kv.get("value").map_or(Value::Null, any_value));
        }
    }
    out
}

fn any_value(v: &Value) -> Value {
    for key in ["stringValue", "string_value"] {
        if let Some(s) = v.get(key) {
            return s.clone();
        }
    }
    for key in ["boolValue", "bool_value", "doubleValue", "double_value"] {
        if let Some(s) = v.get(key) {
            return s.clone();
        }
    }
    for key in ["intValue", "int_value"] {
        if let Some(s) = v.get(key) {
            // Sent as a string to survive JSON's 53-bit integer limit.
            if let Some(txt) = s.as_str() {
                if let Ok(n) = txt.parse::<i64>() {
                    return Value::from(n);
                }
            }
            return s.clone();
        }
    }
    for key in ["arrayValue", "array_value"] {
        if let Some(a) = v.get(key).and_then(|a| a.get("values")).and_then(Value::as_array) {
            return Value::Array(a.iter().map(any_value).collect());
        }
    }
    for key in ["kvlistValue", "kvlist_value"] {
        if let Some(a) = v.get(key).and_then(|a| a.get("values")) {
            return Value::Object(flatten_attributes(a));
        }
    }
    if let Some(b) = v.get("bytesValue").or_else(|| v.get("bytes_value")) {
        return b.clone();
    }
    Value::Null
}

fn attr<'a>(s: &'a OtlpSpan, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| s.attributes.get(*k))
}

fn attr_str<'a>(s: &'a OtlpSpan, keys: &[&str]) -> Option<&'a str> {
    attr(s, keys).and_then(Value::as_str)
}

fn attr_u64(s: &OtlpSpan, keys: &[&str]) -> Option<u64> {
    attr(s, keys).and_then(|v| match v {
        Value::Number(n) => n.as_u64(),
        Value::String(t) => t.parse().ok(),
        _ => None,
    })
}

/// Infer an mcpd [`SpanKind`] from whichever convention the producer used.
///
/// Checked in order of specificity: mcpd's own attribute, OpenInference's
/// span kind, GenAI's operation name, then a name-shaped guess. Falling back to
/// [`SpanKind::Generic`] is always safe — mislabelling a span is worse than
/// declining to label it.
#[must_use]
pub fn infer_kind(s: &OtlpSpan) -> SpanKind {
    use std::str::FromStr;

    if let Some(k) = attr_str(s, &["mcpd.span.kind"]) {
        if let Ok(k) = SpanKind::from_str(k) {
            return k;
        }
    }
    // A value this build does not recognise is no answer, so the search carries
    // on. Returning Generic from inside these arms stopped it: a span from a
    // newer OpenInference or OpenTelemetry release was filed as Generic even
    // when two other attributes said plainly what it was, which drops it out of
    // the model views and stops its tokens being billed. These conventions gain
    // values over time, so an unknown one is the expected case rather than a
    // malformed span.
    if let Some(k) = attr_str(s, &["openinference.span.kind"]) {
        let kind = match k.to_ascii_uppercase().as_str() {
            "LLM" => Some(SpanKind::ModelGeneration),
            "CHAIN" => Some(SpanKind::WorkflowRun),
            "TOOL" => Some(SpanKind::ToolCall),
            "AGENT" => Some(SpanKind::AgentRun),
            "RETRIEVER" => Some(SpanKind::Retrieval),
            "EMBEDDING" => Some(SpanKind::RagEmbedding),
            "RERANKER" => Some(SpanKind::RagAction),
            "GUARDRAIL" => Some(SpanKind::Guardrail),
            "EVALUATOR" => Some(SpanKind::ScorerRun),
            _ => None,
        };
        if let Some(kind) = kind {
            return kind;
        }
    }
    if let Some(op) = attr_str(s, &["gen_ai.operation.name"]) {
        let kind = match op {
            "chat" | "text_completion" | "generate_content" => Some(SpanKind::ModelGeneration),
            "embeddings" => Some(SpanKind::RagEmbedding),
            "execute_tool" => Some(SpanKind::ToolCall),
            "invoke_agent" | "create_agent" => Some(SpanKind::AgentRun),
            _ => None,
        };
        if let Some(kind) = kind {
            return kind;
        }
    }
    if attr(s, &["gen_ai.system", "llm.model_name", "llm.provider"]).is_some() {
        return SpanKind::ModelGeneration;
    }
    if attr(s, &["mcp.tool.name"]).is_some() {
        return SpanKind::McpToolCall;
    }
    if attr(s, &["http.request.method", "http.method"]).is_some() {
        return SpanKind::HttpRequest;
    }
    SpanKind::Generic
}

fn extract_usage(s: &OtlpSpan) -> Option<TokenUsage> {
    let input = attr_u64(
        s,
        &[
            "gen_ai.usage.input_tokens",
            "gen_ai.usage.prompt_tokens",
            "llm.token_count.prompt",
            "mcpd.usage.input_tokens",
        ],
    );
    let output = attr_u64(
        s,
        &[
            "gen_ai.usage.output_tokens",
            "gen_ai.usage.completion_tokens",
            "llm.token_count.completion",
            "mcpd.usage.output_tokens",
        ],
    );
    // gen_ai first, like every field around it. These two lists were the other
    // way round, so a span carrying both conventions took its input and output
    // from gen_ai and its total from llm. An OpenInference library running under
    // an OpenTelemetry SDK is an ordinary setup, and it produced a total with no
    // relation to the counts stored beside it. Span::total_tokens prefers the
    // stored total, so every token and cost aggregate over that span was wrong,
    // and nothing about the span looked malformed.
    let total = attr_u64(s, &["gen_ai.usage.total_tokens", "llm.token_count.total"]);

    let cache_read = attr_u64(
        s,
        &[
            "gen_ai.usage.cache_read_input_tokens",
            "llm.token_count.prompt_details.cache_read",
            "mcpd.usage.cache_read_tokens",
        ],
    );
    let cache_write = attr_u64(
        s,
        &[
            "gen_ai.usage.cache_creation_input_tokens",
            "llm.token_count.prompt_details.cache_write",
            "mcpd.usage.cache_write_tokens",
        ],
    );
    let reasoning = attr_u64(
        s,
        &["gen_ai.usage.reasoning_tokens", "llm.token_count.completion_details.reasoning"],
    );
    let audio_in = attr_u64(s, &["llm.token_count.prompt_details.audio"]);
    let audio_out = attr_u64(s, &["llm.token_count.completion_details.audio"]);

    if input.is_none() && output.is_none() && total.is_none() {
        return None;
    }

    // Which convention the input count follows.
    //
    // `cache_read_input_tokens` and `cache_creation_input_tokens` are
    // Anthropic's own field names, and Anthropic reports `input_tokens` net of
    // both. Recording the uncached share here is what stops `cost::compute`
    // reading the total as gross and billing that share as nothing: measured at
    // a 50% under-bill on a span carrying 100 input tokens and 1000 read from
    // cache. OTLP is where most of mcpd's spans come from, so the provider that
    // needs the field is the one whose spans arrived without it.
    //
    // The attribute name alone does not settle it, and keying on the name alone
    // over-bills far worse than the underflow it fixes. `gen_ai.*` is a
    // vendor-neutral namespace, so an instrumentation mapping OpenAI's
    // `prompt_tokens_details.cached_tokens` onto `cache_read_input_tokens` is
    // ordinary, and OpenAI's `prompt_tokens` is GROSS of it. Reading that total
    // as net bills the cached share twice, once at the plain rate and once at
    // the cache rate: measured through a running daemon at 0.003625 for a
    // gpt-4o span carrying 1000 input and 900 cache_read, against the 0.001375
    // the same daemon charges for the same counts arriving from an SDK. A 164%
    // over-bill is not a better failure than a 50% under-bill.
    //
    // So the guess only fires where nothing on the span contradicts it. The
    // span already states who served the call and which model, in the same
    // attributes `to_span` reads below for `provider` and `model`, and the
    // convention belongs to the model vendor rather than to the transport: an
    // Anthropic model is net whether it was served by Anthropic, Bedrock or
    // Vertex, and nothing else is. A span that names neither keeps the
    // name-only reading, because then the attribute name is all there is.
    //
    // This is still a heuristic, and it is wrong in both directions. It
    // under-bills a producer that uses Anthropic's field names for a model
    // whose id says neither "anthropic" nor "claude". It over-bills a gateway
    // that keeps a Claude model id while normalising the usage block to the
    // OpenAI shape, where the input total is gross again. The durable answer is
    // a producer that states its convention, and none does today.
    let net_input = attr(
        s,
        &["gen_ai.usage.cache_read_input_tokens", "gen_ai.usage.cache_creation_input_tokens"],
    )
    .is_some()
        && !contradicts_anthropic(s);

    let input_details = (cache_read.is_some() || cache_write.is_some() || audio_in.is_some())
        .then_some(InputTokenDetails {
            text: input.filter(|_| net_input),
            cache_read,
            cache_write,
            cache_write_5m: None,
            cache_write_1h: None,
            audio: audio_in,
            image: None,
        });
    let output_details = (reasoning.is_some() || audio_out.is_some())
        .then_some(OutputTokenDetails { text: None, reasoning, audio: audio_out, image: None });

    Some(TokenUsage {
        input_tokens: input,
        output_tokens: output,
        total_tokens: total,
        input_details,
        output_details,
    })
}

/// Whether anything this span states about itself rules out Anthropic's
/// net-of-cache input convention.
///
/// The convention belongs to the model vendor, not to the transport, so a
/// Claude call is net whether Anthropic, Bedrock or Vertex served it. A span
/// that names neither a provider nor a model states nothing to rule it out and
/// is not treated as a contradiction. Reads the same attributes, in the same
/// order, that `to_span` reads for `provider` and `model`, so this can never
/// disagree with the provider and model the span is stored under.
fn contradicts_anthropic(s: &OtlpSpan) -> bool {
    let mut named = false;
    for keys in [
        &["gen_ai.system", "llm.provider", "mcpd.provider"][..],
        &["gen_ai.request.model", "gen_ai.response.model", "llm.model_name", "mcpd.model"][..],
    ] {
        if let Some(v) = attr_str(s, keys) {
            named = true;
            let lower = v.to_ascii_lowercase();
            if lower.contains("anthropic") || lower.contains("claude") {
                return false;
            }
        }
    }
    named
}

fn nanos_to_time(n: u128) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(n as i128).unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

/// Convert a decoded OTLP span into an mcpd [`Span`].
///
/// `default_project` is used when the producer did not name one, so a plain
/// OpenTelemetry SDK with no mcpd-specific configuration still lands somewhere
/// sensible instead of being rejected.
///
/// This maps one span in isolation. Two parts of the mapping need the whole
/// request in hand and are done by [`to_spans`] instead: an OTLP span event
/// becomes a span of its own, and `external_parent_span_id` is only knowable
/// once every span in the request has been seen. A door that calls this
/// directly gets neither.
#[must_use]
pub fn to_span(o: &OtlpSpan, default_project: &str, received_at: OffsetDateTime) -> Span {
    let kind = infer_kind(o);

    let res_str = |k: &str| o.resource.get(k).and_then(Value::as_str).map(str::to_owned);

    let correlation = Correlation {
        project: attr_str(o, &["mcpd.project"])
            .map(str::to_owned)
            .or_else(|| res_str("mcpd.project"))
            .unwrap_or_else(|| default_project.to_owned()),
        environment: attr_str(o, &["mcpd.environment"])
            .map(str::to_owned)
            .or_else(|| res_str("deployment.environment.name"))
            .or_else(|| res_str("deployment.environment")),
        service_name: res_str("service.name"),
        service_version: res_str("service.version"),
        session_id: attr_str(o, &["gen_ai.conversation.id", "session.id", "mcpd.session_id"])
            .map(str::to_owned),
        thread_id: attr_str(o, &["mcpd.thread_id", "thread.id"]).map(str::to_owned),
        user_id: attr_str(o, &["user.id", "enduser.id", "mcpd.user_id"]).map(str::to_owned),
        resource_id: attr_str(o, &["mcpd.resource_id"]).map(str::to_owned),
        run_id: attr_str(o, &["mcpd.run_id"]).map(str::to_owned),
        request_id: attr_str(o, &["mcpd.request_id", "http.request.id"]).map(str::to_owned),
        experiment_id: attr_str(o, &["mcpd.experiment_id"]).map(str::to_owned),
        release: attr_str(o, &["mcpd.release"])
            .map(str::to_owned)
            .or_else(|| res_str("service.version")),
    };

    let status = match o.status_code {
        1 => SpanStatus::Ok,
        2 => SpanStatus::Error,
        _ if o.end_unix_nano > 0 => SpanStatus::Ok,
        _ => SpanStatus::Unset,
    };

    // OpenTelemetry's exception convention puts the type, the message and the
    // stack on a span *event* named `exception`, not on the span's attributes,
    // so `span.recordException(e)` from a stock SDK carries the stack somewhere
    // the attribute lookups below never looked, and the stack was lost. Each
    // field falls back to the event: an attribute set on the span itself is a
    // deliberate statement about the span, while the event is a record of one
    // throw among possibly several, so the attribute keeps precedence.
    //
    // The event never changes the status. OpenTelemetry is explicit that
    // recording an exception does not fail a span, and an instrumentation that
    // catches and recovers records one on a span that succeeded; deriving an
    // error status from it would turn every handled exception into a failed
    // run. The stack of a handled exception stays on the event span that
    // `to_spans` builds, which is where it happened.
    let thrown = o.events.iter().find(|e| e.name == "exception");
    let from_event = |key: &str| {
        thrown.and_then(|e| e.attributes.get(key)).and_then(Value::as_str).map(str::to_owned)
    };

    let error = (status == SpanStatus::Error).then(|| SpanError {
        message: o
            .status_message
            .clone()
            .or_else(|| attr_str(o, &["exception.message"]).map(str::to_owned))
            .or_else(|| from_event("exception.message"))
            .unwrap_or_else(|| "unspecified error".to_owned()),
        kind: attr_str(o, &["exception.type"])
            .map(str::to_owned)
            .or_else(|| from_event("exception.type")),
        stack: attr_str(o, &["exception.stacktrace"])
            .map(str::to_owned)
            .or_else(|| from_event("exception.stacktrace")),
        domain: None,
        retryable: None,
        details: None,
    });

    let mut attributes = o.attributes.clone();
    if o.flags != 0 {
        attributes.insert(FLAGS_ATTRIBUTE.to_owned(), Value::from(o.flags));
    }

    let mut span = Span {
        trace_id: o.trace_id.clone(),
        span_id: o.span_id.clone(),
        parent_span_id: o.parent_span_id.clone(),
        external_parent_span_id: None,
        name: o.name.clone(),
        kind,
        status,
        start_time: nanos_to_time(o.start_unix_nano),
        end_time: (o.end_unix_nano > 0).then(|| nanos_to_time(o.end_unix_nano)),
        duration_ns: None,
        is_root: o.parent_span_id.is_none(),
        is_event: false,
        entity_kind: None,
        entity_id: None,
        entity_name: attr_str(o, &["gen_ai.agent.name", "mcpd.entity.name"]).map(str::to_owned),
        input: attr(o, &["input.value", "gen_ai.prompt", "mcpd.input"]).cloned(),
        output: attr(o, &["output.value", "gen_ai.completion", "mcpd.output"]).cloned(),
        attributes: Some(Value::Object(attributes)),
        metadata: None,
        usage: extract_usage(o),
        cost: None,
        // The `gen_ai.request.*` attributes carry these, and reading them is
        // the model-parameter track's work, not the OTLP decoder's default.
        model_parameters: None,
        error,
        links: (!o.links.is_empty()).then(|| o.links.iter().map(to_link).collect()),
        tags: attr(o, &["mcpd.tags"]).and_then(|v| {
            v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
        }),
        correlation,
        model: attr_str(
            o,
            &["gen_ai.request.model", "gen_ai.response.model", "llm.model_name", "mcpd.model"],
        )
        .map(str::to_owned),
        provider: attr_str(o, &["gen_ai.system", "llm.provider", "mcpd.provider"])
            .map(str::to_owned),
        received_at,
        ingest_key: None,
    };
    span.normalise();
    // Ids that survived decoding are already validated; assert cheaply so a
    // future refactor cannot reintroduce an unvalidated path.
    debug_assert!(ids::validate_trace_id(&span.trace_id).is_ok());
    span
}

fn to_link(l: &OtlpLink) -> crate::SpanLink {
    crate::SpanLink {
        trace_id: l.trace_id.clone(),
        span_id: l.span_id.clone(),
        // OTLP has no field for why a link exists, and mcpd's model does. Two
        // conventions put the reason in the link's attributes: mcpd's own, and
        // the one OpenTelemetry's OpenTracing shim writes when it converts a
        // FOLLOWS_FROM reference. Anything else leaves the reason absent
        // rather than guessed, because a wrong reason on a link is worse than
        // none: it is the only thing that says what the link means.
        kind: ["mcpd.link.kind", "opentracing.ref_type"]
            .iter()
            .find_map(|k| l.attributes.get(*k))
            .and_then(Value::as_str)
            .map(str::to_owned),
        attributes: (!l.attributes.is_empty()).then(|| Value::Object(l.attributes.clone())),
    }
}

/// Map a whole decoded request, which is the unit an OTLP door actually holds.
///
/// [`to_span`] maps one span. Two things cannot be decided from one span alone,
/// and both are done here:
///
/// **Span events become spans.** OpenTelemetry records a point in time inside a
/// span as an event on it, and mcpd already models a point in time as a span
/// with `is_event` set and no end. So each event becomes a zero-duration span
/// parented to the span that carried it, which puts it in the waterfall at the
/// instant it happened, keeps it in the same trace, and needs no new column.
/// The alternative, a blob of events hanging off the parent, would be invisible
/// to every query mcpd already has.
///
/// **`external_parent_span_id`** records a parent mcpd never received. The rule
/// is two conditions and is documented on `external_parent` below, along with
/// the two cases it gets wrong.
#[must_use]
pub fn to_spans(
    decoded: &[OtlpSpan],
    default_project: &str,
    received_at: OffsetDateTime,
) -> Vec<Span> {
    let present: std::collections::HashSet<&str> =
        decoded.iter().map(|o| o.span_id.as_str()).collect();

    let mut out = Vec::with_capacity(decoded.len());
    for o in decoded {
        let mut span = to_span(o, default_project, received_at);
        span.external_parent_span_id = external_parent(o, &present);
        for (index, event) in o.events.iter().enumerate() {
            out.push(event_span(o, &span, index, event, default_project, received_at));
        }
        out.push(span);
    }
    out
}

/// Whether this span's parent came from a tracing system mcpd is not receiving,
/// and so belongs in `external_parent_span_id` as well as `parent_span_id`.
///
/// Two conditions, and both are needed.
///
/// The first is the one the field describes: the parent is not among the spans
/// in this request, so mcpd does not hold it. On its own that is wrong far more
/// often than it is right. A span ends after its children, so an OpenTelemetry
/// batch processor exports the children first and the parent in a later batch;
/// keying on absence alone would label almost every child in an ordinary
/// deployment as having a foreign parent, minutes before that parent arrived.
///
/// The second is what makes the first safe: OTLP's flags say the parent context
/// is remote, so the parent was created in another process and this producer
/// will never export it in any batch. Together they mean "another process made
/// this parent, and it did not come through this door either".
///
/// Two things this gets wrong, in the safe direction and the unsafe one:
///
/// - A producer that never sets the remote bits, which is any SDK older than
///   OTLP 1.1, never populates the field at all. It stays absent, which is what
///   it already was, rather than becoming wrong.
/// - A remote parent belonging to a service that also exports to mcpd, in a
///   different request, is labelled external although mcpd does hold it. The
///   field is carried for correlation and is never used to build mcpd's tree,
///   and `parent_span_id` is left in place, so the trace still assembles
///   correctly and the mislabel costs a reader nothing but the label.
fn external_parent(o: &OtlpSpan, present: &std::collections::HashSet<&str>) -> Option<String> {
    let parent = o.parent_span_id.as_ref()?;
    if present.contains(parent.as_str()) || o.parent_is_remote() != Some(true) {
        return None;
    }
    Some(parent.clone())
}

/// One OTLP span event, as an mcpd event span under the span that carried it.
fn event_span(
    carrier: &OtlpSpan,
    mapped: &Span,
    index: usize,
    event: &OtlpEvent,
    default_project: &str,
    received_at: OffsetDateTime,
) -> Span {
    let synthetic = OtlpSpan {
        trace_id: carrier.trace_id.clone(),
        span_id: event_span_id(carrier, index, event),
        parent_span_id: Some(carrier.span_id.clone()),
        name: event.name.clone(),
        // `timeUnixNano` is optional, and an event without one belongs at the
        // start of the span that carried it rather than at the Unix epoch,
        // where it would sort a run to 1970 and stretch every waterfall drawn
        // over it.
        start_unix_nano: if event.time_unix_nano == 0 {
            carrier.start_unix_nano
        } else {
            event.time_unix_nano
        },
        // Left at zero deliberately: an event is a point in time, and a
        // non-zero end is what would make `to_span` read it as a span that
        // finished and give it a status of ok.
        end_unix_nano: 0,
        attributes: event.attributes.clone(),
        resource: carrier.resource.clone(),
        status_code: 0,
        status_message: None,
        flags: carrier.flags,
        links: Vec::new(),
        events: Vec::new(),
    };

    let mut span = to_span(&synthetic, default_project, received_at);
    // Correlation is inherited whole rather than re-derived from the event's
    // own attributes. An event carries whatever the producer attached to that
    // one moment, which is almost never the project or the session, so
    // re-deriving it would file the event under the default project while the
    // span that carried it went somewhere else, and nobody reading the run
    // would ever see it.
    span.correlation = mapped.correlation.clone();
    span.is_event = true;
    span.normalise();
    span
}

/// A stable span id for a span event.
///
/// Derived, not random. An OTLP exporter retries, the retry carries the same
/// events, and all three stores key a span on its id: a random id here would
/// store another copy of every event on every retry, which is exactly the
/// defect the logs and metrics doors were fixed for. The index is in the
/// digest so that two identical events recorded at the same instant on the same
/// span stay two rows rather than collapsing into one.
fn event_span_id(carrier: &OtlpSpan, index: usize, event: &OtlpEvent) -> String {
    use sha2::{Digest, Sha256};

    let mut h = Sha256::new();
    h.update(b"mcpd otlp span event v1\0");
    h.update(carrier.trace_id.as_bytes());
    h.update([0]);
    h.update(carrier.span_id.as_bytes());
    h.update([0]);
    h.update((index as u64).to_be_bytes());
    h.update(event.name.as_bytes());
    h.update([0]);
    h.update(event.time_unix_nano.to_be_bytes());
    let digest = h.finalize();

    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    if bytes.iter().all(|b| *b == 0) {
        // The all-zero span id is reserved, and every store refuses it. Eight
        // leading zero bytes out of SHA-256 is not something anyone will see,
        // but an id that cannot be stored must not be constructible here at
        // all, or the guard is an assumption rather than a guarantee.
        bytes.copy_from_slice(&digest[8..16]);
        bytes[0] |= 1;
    }
    hex::encode(bytes)
}

/// The upstream sampling decision a mapped OTLP span carries, if any.
///
/// Pass it to [`crate::sampling::Policy::decide`], which honours an upstream
/// decision over its own: a distributed trace that some collectors keep and
/// others drop is not a trace.
///
/// Only ever `Some(true)` or `None`, and the asymmetry is deliberate. A set
/// sampled bit is a positive decision by whoever started the trace, and it must
/// beat a local ratio sampler that would otherwise cut this service's half out
/// of it. A clear bit is not the opposite decision, for two reasons: OTLP's
/// `flags` is absent from the wire when it is zero, so "not sampled" and "the
/// producer never said" are the same bytes; and the span is here, which means
/// the producer chose to export it. Reading a clear bit as `Some(false)` would
/// throw away every span from every SDK that does not populate the field. So it
/// falls through to whatever policy this server was configured with.
#[must_use]
pub fn upstream_sampled(span: &Span) -> Option<bool> {
    let flags = span.attributes.as_ref()?.get(FLAGS_ATTRIBUTE)?.as_u64()?;
    (u32::try_from(flags).ok()? & TRACE_FLAG_SAMPLED != 0).then_some(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn otlp_body() -> Value {
        json!({
          "resourceSpans": [{
            "resource": { "attributes": [
              { "key": "service.name", "value": { "stringValue": "checkout-agent" } },
              { "key": "service.version", "value": { "stringValue": "2.1.0" } }
            ]},
            "scopeSpans": [{
              "scope": { "name": "my-instrumentation" },
              "spans": [{
                "traceId": "4bf92f3577b34da6a3ce929d0e0e4736",
                "spanId": "00f067aa0ba902b7",
                "name": "chat gpt-4o",
                "startTimeUnixNano": "1700000000000000000",
                "endTimeUnixNano":   "1700000001500000000",
                "status": { "code": 1 },
                "attributes": [
                  { "key": "gen_ai.system", "value": { "stringValue": "openai" } },
                  { "key": "gen_ai.request.model", "value": { "stringValue": "gpt-4o" } },
                  { "key": "gen_ai.operation.name", "value": { "stringValue": "chat" } },
                  { "key": "gen_ai.usage.input_tokens", "value": { "intValue": "1200" } },
                  { "key": "gen_ai.usage.output_tokens", "value": { "intValue": "350" } }
                ]
              }]
            }]
          }]
        })
    }

    #[test]
    fn decodes_a_realistic_genai_payload() {
        let spans = decode_json(&otlp_body()).unwrap();
        assert_eq!(spans.len(), 1);
        let s = to_span(&spans[0], "default", OffsetDateTime::UNIX_EPOCH);
        assert_eq!(s.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(s.kind, SpanKind::ModelGeneration);
        assert_eq!(s.model.as_deref(), Some("gpt-4o"));
        assert_eq!(s.provider.as_deref(), Some("openai"));
        assert_eq!(s.correlation.service_name.as_deref(), Some("checkout-agent"));
        assert_eq!(s.status, SpanStatus::Ok);
        assert_eq!(s.usage.as_ref().unwrap().input_tokens, Some(1200));
        assert_eq!(s.duration_ns, Some(1_500_000_000));
        assert!(s.is_root);
    }

    #[test]
    fn nanosecond_timestamps_keep_full_precision() {
        // Read as f64 this would round; the assertion below would then fail.
        let spans = decode_json(&otlp_body()).unwrap();
        assert_eq!(spans[0].start_unix_nano, 1_700_000_000_000_000_000);
    }

    #[test]
    fn accepts_base64_ids_as_well_as_hex() {
        let body = json!({ "resourceSpans": [{ "scopeSpans": [{ "spans": [{
            "traceId": "S/kvNXezTaajzpKdDg5HNg==",
            "spanId": "APBnqgupArc=",
            "name": "x",
            "startTimeUnixNano": "1"
        }]}]}]});
        let spans = decode_json(&body).unwrap();
        assert_eq!(spans.len(), 1, "base64 ids must be accepted");
        assert_eq!(spans[0].trace_id.len(), 32);
    }

    #[test]
    fn skips_bad_spans_without_failing_the_batch() {
        let body = json!({ "resourceSpans": [{ "scopeSpans": [{ "spans": [
            { "traceId": "00000000000000000000000000000000", "spanId": "00f067aa0ba902b7", "name": "zero-trace" },
            { "name": "no-ids" },
            { "traceId": "4bf92f3577b34da6a3ce929d0e0e4736", "spanId": "00f067aa0ba902b7", "name": "good", "startTimeUnixNano": "1" }
        ]}]}]});
        let spans = decode_json(&body).unwrap();
        assert_eq!(spans.len(), 1, "only the valid span should survive");
        assert_eq!(spans[0].name, "good");
    }

    #[test]
    fn maps_openinference_span_kinds() {
        for (oi, expect) in [
            ("LLM", SpanKind::ModelGeneration),
            ("TOOL", SpanKind::ToolCall),
            ("AGENT", SpanKind::AgentRun),
            ("RETRIEVER", SpanKind::Retrieval),
            ("EMBEDDING", SpanKind::RagEmbedding),
            ("GUARDRAIL", SpanKind::Guardrail),
        ] {
            let mut s = OtlpSpan::default();
            s.attributes.insert("openinference.span.kind".into(), json!(oi));
            assert_eq!(infer_kind(&s), expect, "OpenInference {oi} mapped wrong");
        }
    }

    #[test]
    fn reads_openinference_token_counts_and_cache_detail() {
        let mut s = OtlpSpan::default();
        s.attributes.insert("llm.token_count.prompt".into(), json!(100));
        s.attributes.insert("llm.token_count.completion".into(), json!(20));
        s.attributes.insert("llm.token_count.prompt_details.cache_read".into(), json!(80));
        let u = extract_usage(&s).unwrap();
        assert_eq!(u.input_tokens, Some(100));
        assert_eq!(u.output_tokens, Some(20));
        assert_eq!(u.input_details.unwrap().cache_read, Some(80));
    }

    #[test]
    fn error_status_produces_a_populated_error() {
        let mut o = OtlpSpan { status_code: 2, ..OtlpSpan::default() };
        o.trace_id = "4bf92f3577b34da6a3ce929d0e0e4736".into();
        o.span_id = "00f067aa0ba902b7".into();
        o.attributes.insert("exception.type".into(), json!("RateLimitError"));
        o.status_message = Some("429 from provider".into());
        let s = to_span(&o, "p", OffsetDateTime::UNIX_EPOCH);
        assert_eq!(s.status, SpanStatus::Error);
        let e = s.error.unwrap();
        assert_eq!(e.message, "429 from provider");
        assert_eq!(e.kind.as_deref(), Some("RateLimitError"));
    }

    #[test]
    fn an_error_span_always_has_a_message() {
        let o = OtlpSpan {
            status_code: 2,
            trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".into(),
            span_id: "00f067aa0ba902b7".into(),
            ..OtlpSpan::default()
        };
        let s = to_span(&o, "p", OffsetDateTime::UNIX_EPOCH);
        assert!(!s.error.unwrap().message.is_empty());
    }

    #[test]
    fn flattens_nested_attribute_values() {
        let v = json!([
            { "key": "a", "value": { "stringValue": "x" } },
            { "key": "b", "value": { "intValue": "9007199254740993" } },
            { "key": "c", "value": { "boolValue": true } },
            { "key": "d", "value": { "arrayValue": { "values": [ { "stringValue": "p" } ] } } },
            { "key": "e", "value": { "kvlistValue": { "values": [ { "key": "k", "value": { "stringValue": "v" } } ] } } }
        ]);
        let m = flatten_attributes(&v);
        assert_eq!(m["a"], json!("x"));
        assert_eq!(m["b"], json!(9_007_199_254_740_993i64), "large ints must not lose precision");
        assert_eq!(m["c"], json!(true));
        assert_eq!(m["d"], json!(["p"]));
        assert_eq!(m["e"]["k"], json!("v"));
    }

    #[test]
    fn missing_resource_spans_is_an_error_but_empty_is_fine() {
        assert!(decode_json(&json!({})).is_err());
        assert!(decode_json(&json!({ "resourceSpans": [] })).unwrap().is_empty());
    }

    #[test]
    fn unfinished_span_has_no_end_time() {
        let body = json!({ "resourceSpans": [{ "scopeSpans": [{ "spans": [{
            "traceId": "4bf92f3577b34da6a3ce929d0e0e4736",
            "spanId": "00f067aa0ba902b7", "name": "running",
            "startTimeUnixNano": "1700000000000000000", "endTimeUnixNano": "0"
        }]}]}]});
        let spans = decode_json(&body).unwrap();
        let s = to_span(&spans[0], "p", OffsetDateTime::UNIX_EPOCH);
        assert!(s.end_time.is_none());
        assert_eq!(s.status, SpanStatus::Unset);
    }
}
