//! From mcpd's signals to what one destination is sent.
//!
//! Three steps, in this order, for every destination on its own:
//!
//! 1. **Filter**: the destination's projects, environments, kinds and sample.
//! 2. **Redact**: what this destination is not sent, on top of what mcpd
//!    redacted before it stored anything. The stored copy is never touched.
//! 3. **Map**: the attributes the destination's conventions name. Every set
//!    carries the OpenTelemetry GenAI attributes and mcpd's own, so a copy sent
//!    to another mcpd loses nothing; the vendor sets add the namespace that
//!    vendor reads, such as `langfuse.*` or `openinference.*`.

use super::config::{Conventions, Filter, Redact, Resolved};
use super::model::{
    AnyValue, Attrs, Export, Group, OtData, OtEvent, OtHistogram, OtLink, OtLog, OtMetric, OtPoint,
    OtSpan,
};
use mcpd_core::{LogLevel, LogRecord, MetricKind, MetricPoint, Span, SpanKind, SpanStatus};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

// --------------------------------------------------------------- filter ---

/// Whether a trace falls inside a sampled fraction.
///
/// Read from the low 64 bits of the trace id, which W3C requires to be random,
/// so the same trace gets the same answer at every destination and on every
/// replica, and a trace is sent whole or not at all.
#[must_use]
pub fn sampled(trace_id: &str, fraction: Option<f64>) -> bool {
    let Some(f) = fraction else { return true };
    if f >= 1.0 {
        return true;
    }
    if f <= 0.0 {
        return false;
    }
    let tail = &trace_id[trace_id.len().saturating_sub(16)..];
    let n = u64::from_str_radix(tail, 16).unwrap_or(0);
    (n as f64) < f * (u64::MAX as f64)
}

fn place_ok(filter: &Filter, project: &str, environment: Option<&str>) -> bool {
    (filter.projects.is_empty() || filter.projects.iter().any(|p| p == project))
        && (filter.environments.is_empty()
            || environment.is_some_and(|e| filter.environments.iter().any(|x| x == e)))
}

/// The spans a destination is sent. An event follows the span that carries it,
/// so a kind filter that keeps a model call keeps its chunks with it.
#[must_use]
pub fn filter_spans(spans: &[Span], filter: &Filter) -> Vec<Span> {
    let own = |s: &Span| {
        place_ok(filter, &s.correlation.project, s.correlation.environment.as_deref())
            && sampled(&s.trace_id, filter.sample)
            && (filter.kinds.is_empty() || filter.kinds.contains(&s.kind))
    };
    let kept: HashSet<&str> =
        spans.iter().filter(|s| !s.is_event && own(s)).map(|s| s.span_id.as_str()).collect();
    let present: HashSet<&str> = spans.iter().map(|s| s.span_id.as_str()).collect();
    spans
        .iter()
        .filter(|s| {
            if !s.is_event {
                return kept.contains(s.span_id.as_str());
            }
            match &s.parent_span_id {
                Some(p) if present.contains(p.as_str()) => kept.contains(p.as_str()),
                _ => own(s),
            }
        })
        .cloned()
        .collect()
}

/// The log records a destination is sent. A record inside a trace follows the
/// trace's sample.
#[must_use]
pub fn filter_logs(logs: &[LogRecord], filter: &Filter) -> Vec<LogRecord> {
    logs.iter()
        .filter(|l| {
            place_ok(filter, &l.correlation.project, l.correlation.environment.as_deref())
                && l.trace_id.as_deref().is_none_or(|t| sampled(t, filter.sample))
        })
        .cloned()
        .collect()
}

/// The metric points a destination is sent. A point has no trace to sample
/// by, so only the place filters apply.
#[must_use]
pub fn filter_metrics(points: &[MetricPoint], filter: &Filter) -> Vec<MetricPoint> {
    points
        .iter()
        .filter(|m| place_ok(filter, &m.correlation.project, m.correlation.environment.as_deref()))
        .cloned()
        .collect()
}

// --------------------------------------------------------------- redact ---

/// The token a masked value becomes, the same one the daemon's redactor uses.
const MASK: &str = "[redacted]";

fn mask_keys(v: &mut Value, keys: &[String]) {
    match v {
        Value::Object(map) => {
            for (k, val) in map.iter_mut() {
                let lower = k.to_ascii_lowercase();
                if keys.iter().any(|n| lower.contains(n.as_str())) {
                    *val = json!(MASK);
                } else {
                    mask_keys(val, keys);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|i| mask_keys(i, keys)),
        _ => {}
    }
}

fn lowered(r: &Redact) -> Vec<String> {
    r.key_contains.iter().map(|k| k.trim().to_ascii_lowercase()).filter(|k| !k.is_empty()).collect()
}

/// Hold back from one span what this destination is not sent.
pub fn redact_span(s: &mut Span, r: &Redact) {
    if r.drop_payloads {
        s.input = None;
        s.output = None;
        if let Some(e) = s.error.as_mut() {
            e.stack = None;
            e.details = None;
        }
    }
    let keys = lowered(r);
    if keys.is_empty() {
        return;
    }
    for v in [&mut s.input, &mut s.output, &mut s.attributes, &mut s.metadata].into_iter().flatten()
    {
        mask_keys(v, &keys);
    }
}

/// Hold back from one log record what this destination is not sent.
pub fn redact_log(l: &mut LogRecord, r: &Redact) {
    if r.drop_payloads {
        l.fields = None;
    }
    let keys = lowered(r);
    if let (false, Some(f)) = (keys.is_empty(), l.fields.as_mut()) {
        mask_typed_or_plain(f, &keys);
    }
}

/// Hold back from one metric point what this destination is not sent.
pub fn redact_metric(m: &mut MetricPoint, r: &Redact) {
    let keys = lowered(r);
    if let (false, Some(f)) = (keys.is_empty(), m.labels.as_mut()) {
        mask_typed_or_plain(f, &keys);
    }
}

/// Mask a field set that is either a plain object or OTLP's typed list of
/// `{key, value}`, which is how fields that arrived over OTLP are stored.
fn mask_typed_or_plain(v: &mut Value, keys: &[String]) {
    if let Value::Array(items) = v {
        for kv in items.iter_mut() {
            let k = kv.get("key").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
            if keys.iter().any(|n| k.contains(n.as_str())) {
                kv["value"] = json!({ "stringValue": MASK });
            }
        }
    } else {
        mask_keys(v, keys);
    }
}

// ---------------------------------------------------------- conversions ---

/// A JSON value as an attribute. Objects, and arrays holding objects, go out
/// as their JSON text: most receivers index a nested map poorly or not at all,
/// and a string survives every one of them.
fn any(v: &Value, max: usize) -> Option<AnyValue> {
    Some(match v {
        Value::Null => return None,
        Value::String(s) => AnyValue::Str(bounded(s, max)),
        Value::Bool(b) => AnyValue::Bool(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => AnyValue::Int(i),
            None => AnyValue::Double(n.as_f64().unwrap_or(0.0)),
        },
        Value::Array(items) if items.iter().all(|i| !i.is_object() && !i.is_array()) => {
            AnyValue::Array(items.iter().filter_map(|i| any(i, max)).collect())
        }
        other => AnyValue::Str(bounded(&other.to_string(), max)),
    })
}

/// A string, bounded. A prompt can be megabytes, and forwarding it whole would
/// make mcpd the reason a vendor's per-attribute limit is hit.
pub(crate) fn bounded(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max.saturating_sub(40);
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [{} bytes truncated]", &s[..end], s.len() - end)
}

/// A payload as text: a string as itself, anything else as its JSON.
fn text(v: &Value, max: usize) -> String {
    match v {
        Value::String(s) => bounded(s, max),
        other => bounded(&other.to_string(), max),
    }
}

/// One OTLP typed value (`{"stringValue": ...}` and the rest) as an attribute.
fn typed(v: &Value, max: usize) -> Option<AnyValue> {
    let o = v.as_object()?;
    if let Some(s) = o.get("stringValue").and_then(Value::as_str) {
        return Some(AnyValue::Str(bounded(s, max)));
    }
    if let Some(b) = o.get("boolValue").and_then(Value::as_bool) {
        return Some(AnyValue::Bool(b));
    }
    if let Some(i) = o.get("intValue") {
        let n = i.as_i64().or_else(|| i.as_str().and_then(|s| s.parse().ok()))?;
        return Some(AnyValue::Int(n));
    }
    if let Some(d) = o.get("doubleValue").and_then(Value::as_f64) {
        return Some(AnyValue::Double(d));
    }
    if let Some(a) = o.get("arrayValue").and_then(|a| a.get("values")).and_then(Value::as_array) {
        return Some(AnyValue::Array(a.iter().filter_map(|x| typed(x, max)).collect()));
    }
    if let Some(m) = o.get("kvlistValue").and_then(|a| a.get("values")).and_then(Value::as_array) {
        return Some(AnyValue::Map(typed_list(m, max)));
    }
    None
}

fn typed_list(items: &[Value], max: usize) -> Attrs {
    items
        .iter()
        .filter_map(|kv| {
            let k = kv.get("key")?.as_str()?;
            Some((k.to_owned(), typed(kv.get("value")?, max)?))
        })
        .collect()
}

/// A field set, as attributes: a plain object's entries, or OTLP's typed list.
fn fields_attrs(v: Option<&Value>, max: usize) -> Attrs {
    match v {
        Some(Value::Object(m)) => {
            m.iter().filter_map(|(k, v)| Some((k.clone(), any(v, max)?))).collect()
        }
        Some(Value::Array(items)) if items.iter().all(|i| i.get("key").is_some()) => {
            typed_list(items, max)
        }
        Some(other) => vec![("fields".into(), AnyValue::Str(text(other, max)))],
        None => Vec::new(),
    }
}

fn nanos(t: time::OffsetDateTime) -> u64 {
    u64::try_from(t.unix_timestamp_nanos().max(0)).unwrap_or(u64::MAX)
}

/// The resource a signal belongs to. `mcpd.project` rides along so a copy sent
/// to another mcpd lands in the same project rather than the receiver's
/// default.
fn resource_of(c: &mcpd_core::Correlation, extra: &Attrs) -> Attrs {
    let mut r: Attrs = vec![(
        "service.name".into(),
        AnyValue::Str(c.service_name.clone().unwrap_or_else(|| "mcpd".to_owned())),
    )];
    if let Some(v) = &c.service_version {
        r.push(("service.version".into(), AnyValue::Str(v.clone())));
    }
    if let Some(e) = &c.environment {
        r.push(("deployment.environment.name".into(), AnyValue::Str(e.clone())));
    }
    r.push(("mcpd.project".into(), AnyValue::Str(c.project.clone())));
    for (k, v) in extra {
        r.retain(|(x, _)| x != k);
        r.push((k.clone(), v.clone()));
    }
    r
}

fn resource_key(r: &Attrs) -> String {
    r.iter().map(|(k, v)| format!("{k}={v:?}")).collect::<Vec<_>>().join("\u{1f}")
}

/// Group items by resource, keeping first-seen order so output is stable.
fn grouped<T>(items: impl Iterator<Item = (Attrs, T)>) -> Vec<Group<T>> {
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<Group<T>> = Vec::new();
    for (resource, item) in items {
        let key = resource_key(&resource);
        let at = *index.entry(key).or_insert_with(|| {
            out.push(Group { resource, scope: None, items: Vec::new() });
            out.len() - 1
        });
        out[at].items.push(item);
    }
    out
}

// --------------------------------------------------------------- spans ---

/// The GenAI operation a kind of span performs.
fn operation(kind: SpanKind) -> Option<&'static str> {
    Some(match kind {
        SpanKind::ModelGeneration | SpanKind::ModelStep | SpanKind::ModelInference => "chat",
        SpanKind::ToolCall
        | SpanKind::ClientToolCall
        | SpanKind::ProviderToolCall
        | SpanKind::McpToolCall => "execute_tool",
        SpanKind::AgentRun => "invoke_agent",
        SpanKind::RagEmbedding => "embeddings",
        _ => return None,
    })
}

pub(crate) fn is_model(kind: SpanKind) -> bool {
    matches!(kind, SpanKind::ModelGeneration | SpanKind::ModelStep | SpanKind::ModelInference)
}

fn is_tool(kind: SpanKind) -> bool {
    matches!(
        kind,
        SpanKind::ToolCall
            | SpanKind::ClientToolCall
            | SpanKind::ProviderToolCall
            | SpanKind::McpToolCall
    )
}

fn openinference_kind(kind: SpanKind) -> &'static str {
    match kind {
        k if is_model(k) => "LLM",
        k if is_tool(k) => "TOOL",
        SpanKind::AgentRun => "AGENT",
        SpanKind::Retrieval | SpanKind::RagVectorOperation => "RETRIEVER",
        SpanKind::RagEmbedding => "EMBEDDING",
        SpanKind::Guardrail => "GUARDRAIL",
        SpanKind::ScorerRun | SpanKind::ScorerStep => "EVALUATOR",
        _ => "CHAIN",
    }
}

fn langfuse_type(s: &Span) -> &'static str {
    if s.is_event {
        return "event";
    }
    match s.kind {
        k if is_model(k) => "generation",
        k if is_tool(k) => "tool",
        SpanKind::AgentRun => "agent",
        SpanKind::Retrieval | SpanKind::RagVectorOperation => "retriever",
        SpanKind::RagEmbedding => "embedding",
        SpanKind::Guardrail => "guardrail",
        SpanKind::ScorerRun | SpanKind::ScorerStep => "evaluator",
        SpanKind::WorkflowRun | SpanKind::WorkflowStep => "chain",
        _ => "span",
    }
}

fn langsmith_kind(kind: SpanKind) -> &'static str {
    match kind {
        k if is_model(k) => "llm",
        k if is_tool(k) => "tool",
        SpanKind::Retrieval | SpanKind::RagVectorOperation => "retriever",
        SpanKind::RagEmbedding => "embedding",
        _ => "chain",
    }
}

fn braintrust_type(kind: SpanKind) -> &'static str {
    match kind {
        k if is_model(k) => "llm",
        k if is_tool(k) => "tool",
        SpanKind::ScorerRun | SpanKind::ScorerStep => "score",
        _ => "task",
    }
}

fn laminar_type(kind: SpanKind) -> &'static str {
    match kind {
        k if is_model(k) => "LLM",
        k if is_tool(k) => "TOOL",
        SpanKind::ScorerRun | SpanKind::ScorerStep => "EVALUATOR",
        _ => "DEFAULT",
    }
}

/// A prompt or completion as chat messages, when it has that shape: a list of
/// `{role, content}`, an object holding one under `messages` or `choices`, or
/// a bare string, which is one message from `default_role`.
pub(crate) fn messages(v: &Value, default_role: &str) -> Option<Vec<(String, String)>> {
    let content = |m: &Value| -> String {
        match m.get("content") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) | None => String::new(),
            Some(other) => other.to_string(),
        }
    };
    let from_list = |items: &[Value]| -> Option<Vec<(String, String)>> {
        let out: Vec<(String, String)> = items
            .iter()
            .filter_map(|m| {
                let m = m.get("message").unwrap_or(m);
                Some((m.get("role")?.as_str()?.to_owned(), content(m)))
            })
            .collect();
        (!out.is_empty()).then_some(out)
    };
    match v {
        Value::String(s) => Some(vec![(default_role.to_owned(), s.clone())]),
        Value::Array(items) => from_list(items),
        Value::Object(o) => {
            if let Some(Value::Array(items)) = o.get("messages").or_else(|| o.get("choices")) {
                return from_list(items);
            }
            if o.contains_key("role") {
                return Some(vec![(
                    o.get("role").and_then(Value::as_str).unwrap_or(default_role).to_owned(),
                    content(v),
                )]);
            }
            None
        }
        _ => None,
    }
}

/// Messages in the OpenTelemetry GenAI shape: a role and a list of parts.
fn semconv_messages(list: &[(String, String)]) -> String {
    let v: Vec<Value> = list
        .iter()
        .map(|(role, content)| {
            json!({ "role": role, "parts": [{ "type": "text", "content": content }] })
        })
        .collect();
    Value::Array(v).to_string()
}

/// The most chat messages written out one attribute per field, for the
/// OpenInference flattened form. Past this the JSON in `input.value` still
/// carries all of them.
const MAX_FLAT_MESSAGES: usize = 32;

/// Every attribute one span is written with, under one destination's
/// conventions.
#[allow(clippy::too_many_lines)]
fn span_attrs(s: &Span, conventions: Conventions, max: usize) -> Attrs {
    // A map so a mapped attribute replaces an original of the same name rather
    // than sending the key twice, which OTLP leaves undefined. Sorted, so a
    // payload is the same bytes every time and the recorded fixtures hold.
    let mut a: BTreeMap<String, AnyValue> = BTreeMap::new();
    let put = |a: &mut BTreeMap<String, AnyValue>, k: &str, v: AnyValue| {
        a.insert(k.to_owned(), v);
    };
    let str_v = |s: &str| AnyValue::Str(bounded(s, max));
    let json_text = |v: &Value| AnyValue::Str(text(v, max));

    if let Some(Value::Object(map)) = &s.attributes {
        for (k, v) in map {
            if k == mcpd_core::otlp::FLAGS_ATTRIBUTE {
                continue;
            }
            if let Some(x) = any(v, max) {
                put(&mut a, k, x);
            }
        }
    }

    // mcpd's own, which is what makes a copy sent to another mcpd come back as
    // the same span.
    put(&mut a, "mcpd.span.kind", AnyValue::Str(s.kind.as_str().to_owned()));
    if let Some(n) = &s.entity_name {
        put(&mut a, "mcpd.entity.name", str_v(n));
    }
    if let Some(c) = &s.cost {
        if let Some(v) = c.total_cost {
            put(&mut a, "mcpd.cost.total", AnyValue::Double(v));
            put(&mut a, "mcpd.cost.currency", AnyValue::Str(c.currency.clone()));
        }
    }
    if let Some(tags) = &s.tags {
        put(&mut a, "mcpd.tags", AnyValue::Array(tags.iter().map(|t| str_v(t)).collect()));
    }
    if s.is_event {
        put(&mut a, "mcpd.span.is_event", AnyValue::Bool(true));
    }

    // The OpenTelemetry GenAI conventions, which every set carries.
    if let Some(op) = operation(s.kind) {
        put(&mut a, "gen_ai.operation.name", AnyValue::Str(op.to_owned()));
    }
    if let Some(m) = &s.model {
        put(&mut a, "gen_ai.request.model", str_v(m));
    }
    if let Some(p) = &s.provider {
        put(&mut a, "gen_ai.system", str_v(p));
        put(&mut a, "gen_ai.provider.name", str_v(p));
    }
    let (input_tokens, output_tokens) =
        s.usage.as_ref().map_or((None, None), |u| (u.input_tokens, u.output_tokens));
    let total_tokens = s.total_tokens();
    if let Some(v) = input_tokens {
        put(&mut a, "gen_ai.usage.input_tokens", AnyValue::Int(v as i64));
    }
    if let Some(v) = output_tokens {
        put(&mut a, "gen_ai.usage.output_tokens", AnyValue::Int(v as i64));
    }
    if let Some(p) = &s.model_parameters {
        if let Some(t) = p.temperature {
            put(&mut a, "gen_ai.request.temperature", AnyValue::Double(t));
        }
        if let Some(t) = p.top_p {
            put(&mut a, "gen_ai.request.top_p", AnyValue::Double(t));
        }
        if let Some(t) = p.max_tokens {
            put(&mut a, "gen_ai.request.max_tokens", AnyValue::Int(t as i64));
        }
    }
    if s.kind == SpanKind::AgentRun {
        if let Some(n) = &s.entity_name {
            put(&mut a, "gen_ai.agent.name", str_v(n));
        }
    }
    if is_tool(s.kind) {
        put(&mut a, "gen_ai.tool.name", str_v(s.entity_name.as_deref().unwrap_or(&s.name)));
    }
    let c = &s.correlation;
    if let Some(v) = &c.session_id {
        put(&mut a, "gen_ai.conversation.id", str_v(v));
        put(&mut a, "session.id", str_v(v));
    }
    if let Some(v) = &c.user_id {
        // Hashed at ingest when the redactor hashes it. Forwarding the hash
        // rather than the identity keeps the redaction promise across the
        // boundary.
        put(&mut a, "user.id", str_v(v));
    }
    if let Some(e) = &s.error {
        put(&mut a, "error.type", str_v(e.kind.as_deref().unwrap_or("error")));
    }

    let params_json = s.model_parameters.as_ref().and_then(|p| serde_json::to_value(p).ok());
    let usage_json = || {
        let mut u = serde_json::Map::new();
        if let Some(v) = input_tokens {
            u.insert("input".into(), json!(v));
        }
        if let Some(v) = output_tokens {
            u.insert("output".into(), json!(v));
        }
        if let Some(v) = total_tokens {
            u.insert("total".into(), json!(v));
        }
        Value::Object(u)
    };

    match conventions {
        Conventions::GenAi => {
            if let Some(v) = &s.input {
                put(&mut a, "input.value", json_text(v));
            }
            if let Some(v) = &s.output {
                put(&mut a, "output.value", json_text(v));
            }
        }
        Conventions::GenAiMessages => {
            let model = is_model(s.kind);
            if let Some(v) = &s.input {
                match messages(v, "user").filter(|_| model) {
                    Some(m) => put(&mut a, "gen_ai.input.messages", str_v(&semconv_messages(&m))),
                    None if is_tool(s.kind) => {
                        put(&mut a, "gen_ai.tool.call.arguments", json_text(v));
                    }
                    None => put(&mut a, "input.value", json_text(v)),
                }
            }
            if let Some(v) = &s.output {
                match messages(v, "assistant").filter(|_| model) {
                    Some(m) => {
                        put(&mut a, "gen_ai.output.messages", str_v(&semconv_messages(&m)));
                    }
                    None if is_tool(s.kind) => {
                        put(&mut a, "gen_ai.tool.call.result", json_text(v));
                    }
                    None => put(&mut a, "output.value", json_text(v)),
                }
            }
        }
        Conventions::Openinference => {
            put(
                &mut a,
                "openinference.span.kind",
                AnyValue::Str(openinference_kind(s.kind).to_owned()),
            );
            let mime = |v: &Value| if v.is_string() { "text/plain" } else { "application/json" };
            if let Some(v) = &s.input {
                put(&mut a, "input.value", json_text(v));
                put(&mut a, "input.mime_type", AnyValue::Str(mime(v).to_owned()));
                if is_model(s.kind) {
                    for (i, (role, content)) in messages(v, "user")
                        .unwrap_or_default()
                        .iter()
                        .take(MAX_FLAT_MESSAGES)
                        .enumerate()
                    {
                        put(&mut a, &format!("llm.input_messages.{i}.message.role"), str_v(role));
                        put(
                            &mut a,
                            &format!("llm.input_messages.{i}.message.content"),
                            str_v(content),
                        );
                    }
                }
            }
            if let Some(v) = &s.output {
                put(&mut a, "output.value", json_text(v));
                put(&mut a, "output.mime_type", AnyValue::Str(mime(v).to_owned()));
                if is_model(s.kind) {
                    for (i, (role, content)) in messages(v, "assistant")
                        .unwrap_or_default()
                        .iter()
                        .take(MAX_FLAT_MESSAGES)
                        .enumerate()
                    {
                        put(&mut a, &format!("llm.output_messages.{i}.message.role"), str_v(role));
                        put(
                            &mut a,
                            &format!("llm.output_messages.{i}.message.content"),
                            str_v(content),
                        );
                    }
                }
            }
            if let Some(m) = &s.model {
                put(&mut a, "llm.model_name", str_v(m));
            }
            if let Some(p) = &s.provider {
                put(&mut a, "llm.provider", str_v(p));
                put(&mut a, "llm.system", str_v(p));
            }
            if let Some(v) = input_tokens {
                put(&mut a, "llm.token_count.prompt", AnyValue::Int(v as i64));
            }
            if let Some(v) = output_tokens {
                put(&mut a, "llm.token_count.completion", AnyValue::Int(v as i64));
            }
            if let Some(v) = total_tokens {
                put(&mut a, "llm.token_count.total", AnyValue::Int(v as i64));
            }
            if let Some(p) = &params_json {
                put(&mut a, "llm.invocation_parameters", json_text(p));
            }
            if is_tool(s.kind) {
                put(&mut a, "tool.name", str_v(s.entity_name.as_deref().unwrap_or(&s.name)));
            }
            if let Some(m) = &s.metadata {
                put(&mut a, "metadata", json_text(m));
            }
            if let Some(tags) = &s.tags {
                put(&mut a, "tag.tags", AnyValue::Array(tags.iter().map(|t| str_v(t)).collect()));
            }
        }
        Conventions::Langfuse => {
            put(&mut a, "langfuse.observation.type", AnyValue::Str(langfuse_type(s).to_owned()));
            if let Some(v) = &s.input {
                put(&mut a, "langfuse.observation.input", json_text(v));
                if s.is_root {
                    put(&mut a, "langfuse.trace.input", json_text(v));
                }
            }
            if let Some(v) = &s.output {
                put(&mut a, "langfuse.observation.output", json_text(v));
                if s.is_root {
                    put(&mut a, "langfuse.trace.output", json_text(v));
                }
            }
            if let Some(m) = &s.model {
                put(&mut a, "langfuse.observation.model.name", str_v(m));
            }
            if s.usage.is_some() {
                put(&mut a, "langfuse.observation.usage_details", json_text(&usage_json()));
            }
            if let Some(total) = s.cost.as_ref().and_then(|c| c.total_cost) {
                put(
                    &mut a,
                    "langfuse.observation.cost_details",
                    json_text(&json!({ "total": total })),
                );
            }
            if let Some(p) = &params_json {
                put(&mut a, "langfuse.observation.model.parameters", json_text(p));
            }
            let level = match s.status {
                SpanStatus::Error => "ERROR",
                _ => "DEFAULT",
            };
            put(&mut a, "langfuse.observation.level", AnyValue::Str(level.to_owned()));
            if let Some(e) = &s.error {
                put(&mut a, "langfuse.observation.status_message", str_v(&e.message));
            }
            if let Some(v) = &c.session_id {
                put(&mut a, "langfuse.session.id", str_v(v));
            }
            if let Some(v) = &c.user_id {
                put(&mut a, "langfuse.user.id", str_v(v));
            }
            if let Some(v) = &c.environment {
                put(&mut a, "langfuse.environment", str_v(v));
            }
            if let Some(v) = &c.release {
                put(&mut a, "langfuse.release", str_v(v));
            }
            if let Some(v) = &c.service_version {
                put(&mut a, "langfuse.version", str_v(v));
            }
            if s.is_root {
                put(&mut a, "langfuse.trace.name", str_v(&s.name));
                if let Some(tags) = &s.tags {
                    put(
                        &mut a,
                        "langfuse.trace.tags",
                        AnyValue::Array(tags.iter().map(|t| str_v(t)).collect()),
                    );
                }
            }
            if let Some(Value::Object(m)) = &s.metadata {
                for (k, v) in m {
                    put(&mut a, &format!("langfuse.observation.metadata.{k}"), json_text(v));
                }
            }
        }
        Conventions::Langsmith => {
            put(&mut a, "langsmith.span.kind", AnyValue::Str(langsmith_kind(s.kind).to_owned()));
            if s.is_root {
                put(&mut a, "langsmith.trace.name", str_v(&s.name));
            }
            if let Some(v) = &c.session_id {
                put(&mut a, "langsmith.trace.session_id", str_v(v));
            }
            if let Some(v) = &c.user_id {
                put(&mut a, "langsmith.metadata.user_id", str_v(v));
            }
            if let Some(v) = &s.input {
                put(&mut a, "gen_ai.prompt", json_text(v));
            }
            if let Some(v) = &s.output {
                put(&mut a, "gen_ai.completion", json_text(v));
            }
            if let Some(tags) = &s.tags {
                put(&mut a, "langsmith.span.tags", str_v(&tags.join(",")));
            }
            if let Some(Value::Object(m)) = &s.metadata {
                for (k, v) in m {
                    put(&mut a, &format!("langsmith.metadata.{k}"), json_text(v));
                }
            }
        }
        Conventions::Braintrust => {
            put(
                &mut a,
                "braintrust.span_attributes",
                AnyValue::Str(
                    json!({ "type": braintrust_type(s.kind), "name": s.name }).to_string(),
                ),
            );
            if let Some(v) = &s.input {
                put(&mut a, "braintrust.input_json", AnyValue::Str(bounded(&v.to_string(), max)));
            }
            if let Some(v) = &s.output {
                put(&mut a, "braintrust.output_json", AnyValue::Str(bounded(&v.to_string(), max)));
            }
            let mut meta = match &s.metadata {
                Some(Value::Object(m)) => m.clone(),
                _ => serde_json::Map::new(),
            };
            for (k, v) in [
                ("model", s.model.clone()),
                ("provider", s.provider.clone()),
                ("session_id", c.session_id.clone()),
                ("user_id", c.user_id.clone()),
                ("environment", c.environment.clone()),
            ] {
                if let Some(v) = v {
                    meta.insert(k.to_owned(), json!(v));
                }
            }
            if let Some(p) = &params_json {
                meta.insert("model_parameters".to_owned(), p.clone());
            }
            put(&mut a, "braintrust.metadata", json_text(&Value::Object(meta)));
            if s.usage.is_some() {
                let mut m = serde_json::Map::new();
                if let Some(v) = input_tokens {
                    m.insert("prompt_tokens".into(), json!(v));
                }
                if let Some(v) = output_tokens {
                    m.insert("completion_tokens".into(), json!(v));
                }
                if let Some(v) = total_tokens {
                    m.insert("tokens".into(), json!(v));
                }
                put(&mut a, "braintrust.metrics", json_text(&Value::Object(m)));
            }
            if let Some(tags) = &s.tags {
                put(
                    &mut a,
                    "braintrust.tags",
                    AnyValue::Array(tags.iter().map(|t| str_v(t)).collect()),
                );
            }
        }
        Conventions::Laminar => {
            put(&mut a, "lmnr.span.type", AnyValue::Str(laminar_type(s.kind).to_owned()));
            if let Some(v) = &s.input {
                put(&mut a, "lmnr.span.input", AnyValue::Str(bounded(&v.to_string(), max)));
            }
            if let Some(v) = &s.output {
                put(&mut a, "lmnr.span.output", AnyValue::Str(bounded(&v.to_string(), max)));
            }
            if let Some(v) = &c.session_id {
                put(&mut a, "lmnr.association.properties.session_id", str_v(v));
            }
            if let Some(v) = &c.user_id {
                put(&mut a, "lmnr.association.properties.user_id", str_v(v));
            }
        }
    }

    a.into_iter().collect()
}

/// OTLP's span kind: 3 for a call out to a model or another service, 1 for
/// everything that happens inside the process.
fn otlp_kind(kind: SpanKind) -> i32 {
    match kind {
        SpanKind::ModelGeneration
        | SpanKind::ModelStep
        | SpanKind::ModelInference
        | SpanKind::HttpRequest
        | SpanKind::McpToolCall
        | SpanKind::ProviderToolCall => 3,
        _ => 1,
    }
}

fn link_of(l: &mcpd_core::SpanLink, max: usize) -> OtLink {
    let mut attrs = fields_attrs(l.attributes.as_ref(), max);
    if let Some(k) = &l.kind {
        attrs.retain(|(x, _)| x != "mcpd.link.kind");
        attrs.push(("mcpd.link.kind".into(), AnyValue::Str(k.clone())));
    }
    OtLink { trace_id: l.trace_id.clone(), span_id: l.span_id.clone().unwrap_or_default(), attrs }
}

/// One span as OTLP. An event span is written as a span here; [`spans`]
/// folds it into its parent when the parent is in the same batch.
fn ot_span(s: &Span, r: &Resolved) -> OtSpan {
    let start = nanos(s.start_time);
    let flags = s
        .attributes
        .as_ref()
        .and_then(|a| a.get(mcpd_core::otlp::FLAGS_ATTRIBUTE))
        .and_then(Value::as_u64)
        .and_then(|f| u32::try_from(f).ok())
        .unwrap_or(0);
    OtSpan {
        trace_id: s.trace_id.clone(),
        span_id: s.span_id.clone(),
        parent_span_id: s.parent_span_id.clone(),
        name: s.name.clone(),
        kind: otlp_kind(s.kind),
        start_ns: start,
        // An event has no end, and an end of zero reads as a span that never
        // finished to most receivers; a point in time ends where it starts.
        end_ns: s.end_time.map_or(if s.is_event { start } else { 0 }, nanos),
        attrs: span_attrs(s, r.conventions, r.max_value_bytes),
        events: Vec::new(),
        links: s.links.iter().flatten().map(|l| link_of(l, r.max_value_bytes)).collect(),
        status_code: match s.status {
            SpanStatus::Ok => 1,
            SpanStatus::Error => 2,
            _ => 0,
        },
        status_message: s.error.as_ref().map(|e| e.message.clone()),
        flags,
    }
}

/// Spans as one destination's OTLP export.
///
/// An event span whose carrier is in the same batch goes back onto it as an
/// OTLP span event, which is the shape it most likely arrived in and the one
/// every receiver draws on a timeline. One whose carrier is not here goes out
/// as a zero-length span, so it is still delivered rather than lost.
#[must_use]
pub fn spans(spans: &[Span], r: &Resolved) -> Export {
    let carriers: HashSet<&str> =
        spans.iter().filter(|s| !s.is_event).map(|s| s.span_id.as_str()).collect();
    let mut events: HashMap<&str, Vec<OtEvent>> = HashMap::new();
    for s in spans.iter().filter(|s| s.is_event) {
        if let Some(p) = s.parent_span_id.as_deref().filter(|p| carriers.contains(p)) {
            let mut attrs = span_attrs(s, Conventions::GenAi, r.max_value_bytes);
            attrs.retain(|(k, _)| k != "mcpd.span.is_event");
            events.entry(p).or_default().push(OtEvent {
                time_ns: nanos(s.start_time),
                name: s.name.clone(),
                attrs,
            });
        }
    }
    let items = spans
        .iter()
        .filter(|s| {
            !s.is_event || s.parent_span_id.as_deref().is_none_or(|p| !carriers.contains(p))
        })
        .map(|s| {
            let mut o = ot_span(s, r);
            if let Some(e) = events.remove(s.span_id.as_str()) {
                o.events = e;
            }
            (resource_of(&s.correlation, &r.resource), o)
        });
    Export::Traces(grouped(items))
}

// ----------------------------------------------------------------- logs ---

fn severity(level: LogLevel) -> (i32, &'static str) {
    match level {
        LogLevel::Trace => (1, "TRACE"),
        LogLevel::Debug => (5, "DEBUG"),
        LogLevel::Info => (9, "INFO"),
        LogLevel::Warn => (13, "WARN"),
        LogLevel::Error => (17, "ERROR"),
        _ => (21, "FATAL"),
    }
}

/// Log records as one destination's OTLP export, grouped by resource and then
/// by logger, which OTLP carries as the instrumentation scope.
#[must_use]
pub fn logs(records: &[LogRecord], r: &Resolved) -> Export {
    let max = r.max_value_bytes;
    let mut groups: Vec<Group<OtLog>> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for l in records {
        let resource = resource_of(&l.correlation, &r.resource);
        let key = format!("{}\u{1e}{}", resource_key(&resource), l.logger.as_deref().unwrap_or(""));
        let at = *index.entry(key).or_insert_with(|| {
            groups.push(Group { resource, scope: l.logger.clone(), items: Vec::new() });
            groups.len() - 1
        });
        let (n, t) = severity(l.level);
        let mut attrs = fields_attrs(l.fields.as_ref(), max);
        if let Some(v) = &l.correlation.session_id {
            attrs.push(("session.id".into(), AnyValue::Str(v.clone())));
        }
        groups[at].items.push(OtLog {
            time_ns: nanos(l.timestamp),
            observed_ns: nanos(l.received_at),
            severity_number: n,
            severity_text: t.to_owned(),
            body: bounded(&l.message, max),
            attrs,
            trace_id: l.trace_id.clone(),
            span_id: l.span_id.clone(),
        });
    }
    Export::Logs(groups)
}

// -------------------------------------------------------------- metrics ---

fn u64_list(v: Option<&Value>) -> Vec<u64> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok())))
                .collect()
        })
        .unwrap_or_default()
}

/// Metric points as one destination's OTLP export, one metric per resource,
/// name, kind and unit.
#[must_use]
pub fn metrics(points: &[MetricPoint], r: &Resolved) -> Export {
    let max = r.max_value_bytes;
    let mut groups: Vec<Group<OtMetric>> = Vec::new();
    let mut gindex: HashMap<String, usize> = HashMap::new();
    let mut mindex: HashMap<(usize, String), usize> = HashMap::new();
    for p in points {
        let resource = resource_of(&p.correlation, &r.resource);
        let rk = resource_key(&resource);
        let g = *gindex.entry(rk).or_insert_with(|| {
            groups.push(Group { resource, scope: None, items: Vec::new() });
            groups.len() - 1
        });
        let unit = p.unit.clone().unwrap_or_default();
        let mk = (g, format!("{}\u{1e}{}\u{1e}{unit}", p.name, p.kind.as_str()));
        let m = *mindex.entry(mk).or_insert_with(|| {
            groups[g].items.push(OtMetric {
                name: p.name.clone(),
                unit: unit.clone(),
                data: match p.kind {
                    MetricKind::Counter => OtData::Sum(Vec::new()),
                    MetricKind::Histogram => OtData::Histogram(Vec::new()),
                    _ => OtData::Gauge(Vec::new()),
                },
            });
            groups[g].items.len() - 1
        });
        let attrs = fields_attrs(p.labels.as_ref(), max);
        let exemplar = p.trace_id.clone().map(|t| (t, p.span_id.clone()));
        let time_ns = nanos(p.timestamp);
        match &mut groups[g].items[m].data {
            OtData::Gauge(v) | OtData::Sum(v) => {
                v.push(OtPoint { attrs, time_ns, value: p.value, exemplar });
            }
            OtData::Histogram(v) => {
                let b = p.buckets.as_ref();
                let bounds: Vec<f64> = b
                    .and_then(|b| b.get("bounds"))
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_f64).collect())
                    .unwrap_or_default();
                let bucket_counts = u64_list(b.and_then(|b| b.get("counts")));
                let count = b
                    .and_then(|b| b.get("count"))
                    .and_then(|c| c.as_u64().or_else(|| c.as_str().and_then(|s| s.parse().ok())))
                    .unwrap_or_else(|| bucket_counts.iter().sum());
                v.push(OtHistogram {
                    attrs,
                    time_ns,
                    count,
                    sum: Some(p.value),
                    bucket_counts,
                    bounds,
                    exemplar,
                });
            }
        }
    }
    Export::Metrics(groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampling_is_decided_by_the_trace_and_is_monotonic_in_the_fraction() {
        let low = "00000000000000000000000000000001";
        let high = "0000000000000000ffffffffffffffff";
        assert!(sampled(low, Some(0.01)));
        assert!(!sampled(high, Some(0.99)));
        assert!(sampled(high, Some(1.0)));
        assert!(!sampled(low, Some(0.0)));
        assert!(sampled(low, None));
    }

    #[test]
    fn a_string_past_the_bound_is_cut_on_a_character_boundary_and_says_so() {
        let s = "é".repeat(100);
        let out = bounded(&s, 64);
        assert!(out.len() < 100, "{}", out.len());
        assert!(out.ends_with("truncated]"));
    }

    #[test]
    fn chat_messages_are_read_from_every_common_shape() {
        let list = json!([{ "role": "system", "content": "be brief" }, { "role": "user", "content": "hi" }]);
        assert_eq!(messages(&list, "user").map(|m| m.len()), Some(2));
        let wrapped = json!({ "messages": list });
        assert_eq!(messages(&wrapped, "user").map(|m| m.len()), Some(2));
        let choices =
            json!({ "choices": [{ "message": { "role": "assistant", "content": "ok" } }] });
        assert_eq!(
            messages(&choices, "assistant"),
            Some(vec![("assistant".to_owned(), "ok".to_owned())])
        );
        assert_eq!(messages(&json!("plain"), "user"), Some(vec![("user".into(), "plain".into())]));
        assert_eq!(messages(&json!({ "q": 1 }), "user"), None);
    }

    #[test]
    fn typed_otlp_fields_go_back_out_typed() {
        let v = json!([
            { "key": "a", "value": { "stringValue": "x" } },
            { "key": "b", "value": { "intValue": "7" } }
        ]);
        let attrs = fields_attrs(Some(&v), 1024);
        assert_eq!(
            attrs,
            vec![("a".into(), AnyValue::Str("x".into())), ("b".into(), AnyValue::Int(7))]
        );
    }

    #[test]
    fn masking_reaches_typed_fields_and_nested_objects() {
        let mut typed = json!([{ "key": "account_no", "value": { "stringValue": "123" } }]);
        mask_typed_or_plain(&mut typed, &["account".into()]);
        assert_eq!(typed[0]["value"]["stringValue"], json!(MASK));
        let mut plain = json!({ "outer": { "Account": "123", "ok": 1 } });
        mask_keys(&mut plain, &["account".into()]);
        assert_eq!(plain["outer"]["Account"], json!(MASK));
        assert_eq!(plain["outer"]["ok"], json!(1));
    }
}
