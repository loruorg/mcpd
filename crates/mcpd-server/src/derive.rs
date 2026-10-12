//! Metrics the daemon derives from the spans it stores.
//!
//! A span already carries everything a duration or a token counter is made of:
//! the two timestamps, the status, and the usage the model reported. Deriving
//! the metric here rather than asking every SDK to emit one means the numbers
//! are the same in six languages, and that a trace recorded through raw OTLP
//! gets the same dashboard as one recorded through an mcpd SDK.
//!
//! Nothing here collects new data. Everything it reads arrived with the span
//! and is on its way to the store regardless.
//!
//! # What this costs
//!
//! One derived point is one stored row, so this multiplies write volume by a
//! factor an operator should know before turning it on:
//!
//! * one duration point per finished agent, tool, workflow, model-generation
//!   or processor span, and none at all for the twenty-six other span kinds;
//! * up to thirteen token points per model-generation span that reported usage,
//!   two of which are the totals and eleven of which only appear when the
//!   provider actually billed that line.
//!
//! A trace of one agent run, three generations and two tool calls is six spans
//! and, with full usage detail on every generation, up to forty-five derived
//! rows. That is the worst case and the ordinary one is far smaller: a provider
//! that reports only `input_tokens` and `output_tokens` contributes two points
//! per generation, not thirteen. [`DerivedMetrics`] is the switch, and both
//! halves turn off separately.
//!
//! Two rules keep the cost from landing anywhere it would do damage.
//!
//! Derived points are added **after** the per-project ceiling has been applied,
//! so a derived metric can never displace a signal a sender actually sent. An
//! operator who sized a ceiling for real traffic does not silently lose nine
//! tenths of it to metrics the daemon invented.
//!
//! They are also subtracted back out of the count reported to the sender, so
//! the `accepted` number in an ingest response keeps describing the batch that
//! was sent rather than the batch that was written. A sender that posts six
//! spans and is told forty-five signals were accepted has been given a number
//! it cannot reconcile with anything.

use mcpd_core::metrics::CardinalityFilter;
use mcpd_core::{MetricKind, MetricPoint, Span, SpanKind, SpanStatus, TokenUsage};
use serde_json::json;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

/// Which derived metrics the daemon writes.
///
/// The two halves are separate because their costs are: durations are one row
/// per span and bounded by the span count, token metrics are up to thirteen
/// rows per model call and bounded by nothing an operator can see from the
/// outside. A deployment that wants latency dashboards and not a token series
/// per cache tier can have exactly that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedMetrics {
    /// Emit a duration point for every finished span of a kind that has one.
    #[serde(default = "enabled")]
    pub durations: bool,
    /// Emit token counters for every model-generation span that reported usage.
    #[serde(default = "enabled")]
    pub tokens: bool,
}

/// Both halves are on unless something turns them off, which is what
/// `#[serde(default)]` on each field resolves to.
fn enabled() -> bool {
    true
}

impl Default for DerivedMetrics {
    fn default() -> Self {
        Self { durations: true, tokens: true }
    }
}

impl DerivedMetrics {
    /// Neither half. What `"derived_metrics": { "durations": false, "tokens":
    /// false }` in the config file resolves to; there is no flag for it.
    #[must_use]
    pub const fn off() -> Self {
        Self { durations: false, tokens: false }
    }

    /// Whether anything at all is derived, so a caller can skip the pass.
    #[must_use]
    pub const fn any(&self) -> bool {
        self.durations || self.tokens
    }
}

/// Duration of one agent invocation, in milliseconds.
pub const AGENT_DURATION: &str = "mcpd.agent.duration";
/// Duration of one tool call, whoever executed it, in milliseconds.
pub const TOOL_DURATION: &str = "mcpd.tool.duration";
/// Duration of one workflow run, in milliseconds.
pub const WORKFLOW_DURATION: &str = "mcpd.workflow.duration";
/// Duration of one model generation, in milliseconds.
pub const MODEL_DURATION: &str = "mcpd.model.duration";
/// Duration of one input or output processor, in milliseconds.
pub const PROCESSOR_DURATION: &str = "mcpd.processor.duration";

/// Total input tokens a model call was billed for.
pub const TOTAL_INPUT_TOKENS: &str = "mcpd.model.total_input_tokens";
/// Total output tokens a model call was billed for.
pub const TOTAL_OUTPUT_TOKENS: &str = "mcpd.model.total_output_tokens";
/// Input tokens that were ordinary text: not cached, not audio, not image.
pub const INPUT_TEXT_TOKENS: &str = "mcpd.model.input_text_tokens";
/// Input tokens served from a prompt cache.
pub const INPUT_CACHE_READ_TOKENS: &str = "mcpd.model.input_cache_read_tokens";
/// Input tokens written into a prompt cache.
pub const INPUT_CACHE_WRITE_TOKENS: &str = "mcpd.model.input_cache_write_tokens";
/// Input tokens written into a five-minute ephemeral cache.
pub const INPUT_CACHE_WRITE_5M_TOKENS: &str = "mcpd.model.input_cache_write_5m_tokens";
/// Input tokens written into a one-hour ephemeral cache.
pub const INPUT_CACHE_WRITE_1H_TOKENS: &str = "mcpd.model.input_cache_write_1h_tokens";
/// Audio input tokens.
pub const INPUT_AUDIO_TOKENS: &str = "mcpd.model.input_audio_tokens";
/// Image input tokens, including rendered PDF pages.
pub const INPUT_IMAGE_TOKENS: &str = "mcpd.model.input_image_tokens";
/// Ordinary completion tokens.
pub const OUTPUT_TEXT_TOKENS: &str = "mcpd.model.output_text_tokens";
/// Hidden reasoning tokens, billed and usually invisible in the response.
pub const OUTPUT_REASONING_TOKENS: &str = "mcpd.model.output_reasoning_tokens";
/// Audio output tokens.
pub const OUTPUT_AUDIO_TOKENS: &str = "mcpd.model.output_audio_tokens";
/// Image output tokens.
pub const OUTPUT_IMAGE_TOKENS: &str = "mcpd.model.output_image_tokens";

/// Every duration metric this module can emit, for a reader and for the test
/// that pins the catalogue.
pub const DURATION_METRICS: &[&str] =
    &[AGENT_DURATION, TOOL_DURATION, WORKFLOW_DURATION, MODEL_DURATION, PROCESSOR_DURATION];

/// Every token metric this module can emit, in the order a span produces them.
pub const TOKEN_METRICS: &[&str] = &[
    TOTAL_INPUT_TOKENS,
    TOTAL_OUTPUT_TOKENS,
    INPUT_TEXT_TOKENS,
    INPUT_CACHE_READ_TOKENS,
    INPUT_CACHE_WRITE_TOKENS,
    INPUT_CACHE_WRITE_5M_TOKENS,
    INPUT_CACHE_WRITE_1H_TOKENS,
    INPUT_AUDIO_TOKENS,
    INPUT_IMAGE_TOKENS,
    OUTPUT_TEXT_TOKENS,
    OUTPUT_REASONING_TOKENS,
    OUTPUT_AUDIO_TOKENS,
    OUTPUT_IMAGE_TOKENS,
];

/// The UCUM unit every token counter carries.
const TOKEN_UNIT: &str = "{token}";
/// The UCUM unit every duration carries. Milliseconds, because that is what
/// `Span::duration_ms` reports and what the interface already plots.
const DURATION_UNIT: &str = "ms";

/// The metric name a span of this kind produces, if it produces one.
///
/// The map is deliberately narrow. A metric per span kind would be
/// thirty-two series nobody asked for, and the five here are the ones a
/// latency dashboard is actually built from: the agent, the tools it called,
/// the workflow around it, the model, and the processors either side.
///
/// `ClientToolCall` is the one omission that is a decision rather than an
/// absence. That span is recorded by the server when it emits the marker, and
/// the tool then runs in the caller's process: its duration measures the
/// marker, not the work. Emitting it under `mcpd.tool.duration` would mix two
/// unrelated distributions into one series, and the fast one would drag the
/// percentile down and hide the slow one. The client reports its own.
#[must_use]
pub fn duration_metric_name(kind: SpanKind) -> Option<&'static str> {
    match kind {
        SpanKind::AgentRun => Some(AGENT_DURATION),
        SpanKind::ToolCall | SpanKind::McpToolCall | SpanKind::ProviderToolCall => {
            Some(TOOL_DURATION)
        }
        SpanKind::WorkflowRun => Some(WORKFLOW_DURATION),
        SpanKind::ModelGeneration => Some(MODEL_DURATION),
        SpanKind::ProcessorRun => Some(PROCESSOR_DURATION),
        _ => None,
    }
}

/// The `status` label a duration point carries.
///
/// Three values, not four. `Unset` on a finished span means the producer never
/// said, and treating "never said" as a failure would put every span from a
/// producer that does not set the field into the error series. `Cancelled`
/// stays distinct from `Error` because mcpd models it and the two mean opposite
/// things to whoever is on call: a cancelled step was abandoned deliberately,
/// and counting it as a failure makes a deliberate timeout look like an outage.
///
/// An `error` payload outranks the status field. A span carrying an error and
/// claiming `ok` is a producer bug, and the error is the evidence.
#[must_use]
pub fn status_label(span: &Span) -> &'static str {
    if span.error.is_some() {
        return "error";
    }
    match span.status {
        SpanStatus::Error => "error",
        SpanStatus::Cancelled => "cancelled",
        SpanStatus::Ok | SpanStatus::Unset => "ok",
    }
}

/// The token samples one usage payload produces, in a fixed order.
///
/// This is the emission rule, and its asymmetry is the whole point.
///
/// The two **totals** are emitted whenever they are defined, a zero included. A
/// zero total is a fact about the call: the model was asked and returned
/// nothing. Suppressing it would make a run of empty completions invisible on
/// the chart it should be most obvious on, and it would also make the count of
/// points stop matching the count of calls, so an average computed from the
/// series would silently be an average over the calls that produced output.
///
/// Every **detail** is emitted only when it is strictly positive. A zero there
/// is nearly always an absence rather than an observation: no cache was read,
/// no audio was sent, the provider does not bill image tokens at all. Emitting
/// them anyway writes eleven rows per model call whose only content is that
/// nothing happened, which is most of this feature's write cost for none of its
/// value, and it puts a flat zero line on every breakdown chart for every
/// dimension a deployment does not use.
///
/// The rule is Mastra's, kept deliberately identical so a dashboard written
/// against one reads the same against the other.
#[must_use]
pub fn token_samples(usage: &TokenUsage) -> Vec<(&'static str, u64)> {
    let mut out = Vec::new();
    push_defined(&mut out, TOTAL_INPUT_TOKENS, usage.input_tokens);
    push_defined(&mut out, TOTAL_OUTPUT_TOKENS, usage.output_tokens);

    if let Some(d) = usage.input_details.as_ref() {
        push_positive(&mut out, INPUT_TEXT_TOKENS, d.text);
        push_positive(&mut out, INPUT_CACHE_READ_TOKENS, d.cache_read);
        push_positive(&mut out, INPUT_CACHE_WRITE_TOKENS, d.cache_write);
        push_positive(&mut out, INPUT_CACHE_WRITE_5M_TOKENS, d.cache_write_5m);
        push_positive(&mut out, INPUT_CACHE_WRITE_1H_TOKENS, d.cache_write_1h);
        push_positive(&mut out, INPUT_AUDIO_TOKENS, d.audio);
        push_positive(&mut out, INPUT_IMAGE_TOKENS, d.image);
    }
    if let Some(d) = usage.output_details.as_ref() {
        push_positive(&mut out, OUTPUT_TEXT_TOKENS, d.text);
        push_positive(&mut out, OUTPUT_REASONING_TOKENS, d.reasoning);
        push_positive(&mut out, OUTPUT_AUDIO_TOKENS, d.audio);
        push_positive(&mut out, OUTPUT_IMAGE_TOKENS, d.image);
    }
    out
}

/// A total, kept whenever the provider reported one. Zero is a reading.
fn push_defined(out: &mut Vec<(&'static str, u64)>, name: &'static str, v: Option<u64>) {
    if let Some(v) = v {
        out.push((name, v));
    }
}

/// A detail line, kept only when the provider billed something for it.
fn push_positive(out: &mut Vec<(&'static str, u64)>, name: &'static str, v: Option<u64>) {
    if let Some(v) = v {
        if v > 0 {
            out.push((name, v));
        }
    }
}

/// Every metric the batch's spans imply, ready to be stored beside them.
///
/// `now` is passed rather than read so that one pass over a batch stamps every
/// point it produces with the same instant, the way the intake pipeline already
/// stamps `received_at`.
#[must_use]
pub fn derive(spans: &[Span], settings: DerivedMetrics, now: OffsetDateTime) -> Vec<MetricPoint> {
    if !settings.any() {
        return Vec::new();
    }
    // The same filter the intake pipeline applies to a client's labels. These
    // labels are the daemon's own and their keys are bounded by construction,
    // but the *values* are not: `model` and `provider` are strings a client
    // chose, and a client that puts a whole prompt in the model field would
    // otherwise open a time series per request. Filtering here rather than
    // trusting the source is the same rule applied to the same risk.
    let filter = CardinalityFilter::default();
    let mut out = Vec::new();
    for span in spans {
        if settings.durations {
            if let Some(point) = duration_point(span, &filter, now) {
                out.push(point);
            }
        }
        if settings.tokens {
            token_points(span, &filter, now, &mut out);
        }
    }
    out
}

/// The duration point for one span, when it has both a name and a duration.
fn duration_point(
    span: &Span,
    filter: &CardinalityFilter,
    now: OffsetDateTime,
) -> Option<MetricPoint> {
    let name = duration_metric_name(span.kind)?;
    // `duration_ns` is `None` for a span that has not finished and for an
    // event, which has no duration by definition. Both are read from the field
    // `Span::normalise` recomputed at intake rather than from the timestamps,
    // so a producer that sent an end time before its start time gets the
    // clamped zero everything else in mcpd sees, not a negative millisecond.
    let ms = span.duration_ms()?;
    let mut labels = Some(json!({ "status": status_label(span) }));
    filter.filter(&mut labels);
    Some(point(span, name, ms, MetricKind::Histogram, DURATION_UNIT, labels, now))
}

/// The token points for one span, appended in place.
///
/// Only `model_generation` produces these, which is the level at which usage is
/// reported and priced. A generation's steps and inferences carry their own
/// share of the same tokens, so counting those too would report a call's usage
/// two or three times and make every cost chart derived from the series wrong
/// by a factor nobody could see.
fn token_points(
    span: &Span,
    filter: &CardinalityFilter,
    now: OffsetDateTime,
    out: &mut Vec<MetricPoint>,
) {
    if span.kind != SpanKind::ModelGeneration {
        return;
    }
    let Some(usage) = span.usage.as_ref() else { return };
    // Model and provider rather than status: a token series is read to answer
    // "what is this model costing", and both values are bounded by the
    // provider's catalogue. The identity of the individual call is on the
    // exemplar below, which is where an unbounded value belongs.
    let mut labels = serde_json::Map::new();
    if let Some(model) = span.model.as_deref() {
        labels.insert("model".to_owned(), json!(model));
    }
    if let Some(provider) = span.provider.as_deref() {
        labels.insert("provider".to_owned(), json!(provider));
    }
    // Filtered once rather than once per sample: every point a generation
    // produces carries the identical label map.
    let mut labels = Some(serde_json::Value::Object(labels));
    filter.filter(&mut labels);
    for (name, value) in token_samples(usage) {
        let value = value as f64;
        out.push(point(span, name, value, MetricKind::Counter, TOKEN_UNIT, labels.clone(), now));
    }
}

/// One derived point, carrying the span it came from as its exemplar.
///
/// `labels` arrives already filtered, because a generation's thirteen points
/// share one map and filtering it thirteen times would be the same answer
/// twelve more times.
fn point(
    span: &Span,
    name: &'static str,
    value: f64,
    kind: MetricKind,
    unit: &str,
    labels: Option<serde_json::Value>,
    now: OffsetDateTime,
) -> MetricPoint {
    MetricPoint {
        metric_id: metric_id(span, name),
        name: name.to_owned(),
        kind,
        value,
        unit: Some(unit.to_owned()),
        // The span's own start, not the moment it was derived. A batch that
        // arrives twenty minutes late must land on the chart where it happened,
        // or every latency spike is drawn at the time of the flush that carried
        // it rather than at the time of the incident.
        timestamp: span.start_time,
        labels,
        buckets: None,
        // The exemplar is what turns a spike back into a run you can open, and
        // it is also where the unbounded identifiers live so they never have to
        // be labels.
        trace_id: Some(span.trace_id.clone()),
        span_id: Some(span.span_id.clone()),
        correlation: span.correlation.clone(),
        received_at: now,
    }
}

/// A stable id for one derived point.
///
/// Derived from the span and the metric name rather than allocated, and the
/// reason is that a span is written more than once. An SDK records a span when
/// it starts and again when it ends, and OTLP prescribes resending a whole
/// export after a failure. Every backend stores a metric with
/// `INSERT OR REPLACE` on `metric_id`, so a derived id makes the second write
/// of a span correct the first point rather than add another one beside it. An
/// allocated id would turn one slow model call into three entries in the
/// latency histogram and one retried export into a doubled token bill.
///
/// The trace id and the project are hashed alongside the span id because a span
/// id is unique within its trace and nothing more.
fn metric_id(span: &Span, name: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"mcpd-derived-metric");
    for part in
        [span.correlation.project.as_str(), span.trace_id.as_str(), span.span_id.as_str(), name]
    {
        // Length-prefixed, so two spans whose fields concatenate to the same
        // bytes cannot hash the same and silently replace each other.
        h.update((part.len() as u64).to_le_bytes());
        h.update(part.as_bytes());
    }
    format!("derived-{}", hex::encode(&h.finalize()[..16]))
}
