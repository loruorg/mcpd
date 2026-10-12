//! The OTLP export model, and its two encodings.
//!
//! Every destination that speaks OTLP is sent the same shape, built once per
//! destination from mcpd's own signals by `mapping`, and then written as
//! OTLP/JSON or as protobuf. Holding one model and two writers is what keeps
//! the encodings from drifting: a field added to the model reaches both, and
//! the round-trip tests read the protobuf back with mcpd's own decoder.
//!
//! The protobuf writer is written by hand for the reason the reader in
//! `mcpd_core::otlp::protobuf` is: the subset OTLP uses is small, and a
//! generated codec needs `protoc` on the build machine.

use serde_json::{json, Value};

/// An OTLP attribute value.
#[derive(Debug, Clone, PartialEq)]
pub enum AnyValue {
    /// A string.
    Str(String),
    /// A boolean.
    Bool(bool),
    /// A signed integer.
    Int(i64),
    /// A float.
    Double(f64),
    /// A list of values.
    Array(Vec<AnyValue>),
    /// A nested map, in order.
    Map(Vec<(String, AnyValue)>),
}

impl AnyValue {
    /// The value as JSON would hold it, for the vendor APIs that take JSON.
    #[must_use]
    pub fn to_plain_json(&self) -> Value {
        match self {
            Self::Str(s) => json!(s),
            Self::Bool(b) => json!(b),
            Self::Int(i) => json!(i),
            Self::Double(d) => json!(d),
            Self::Array(a) => Value::Array(a.iter().map(Self::to_plain_json).collect()),
            Self::Map(m) => {
                Value::Object(m.iter().map(|(k, v)| (k.clone(), v.to_plain_json())).collect())
            }
        }
    }

    /// The string inside, when this is one.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// Attributes, in the order they were added.
pub type Attrs = Vec<(String, AnyValue)>;

/// One span, as OTLP describes it.
#[derive(Debug, Clone, Default)]
pub struct OtSpan {
    /// Hex trace id.
    pub trace_id: String,
    /// Hex span id.
    pub span_id: String,
    /// Hex parent span id.
    pub parent_span_id: Option<String>,
    /// The operation name.
    pub name: String,
    /// OTLP's `SpanKind`: 1 internal, 2 server, 3 client.
    pub kind: i32,
    /// Start, nanoseconds since the epoch.
    pub start_ns: u64,
    /// End, nanoseconds since the epoch. Zero for a span that has not ended.
    pub end_ns: u64,
    /// Attributes.
    pub attrs: Attrs,
    /// Points in time inside the span.
    pub events: Vec<OtEvent>,
    /// Links to other spans or traces.
    pub links: Vec<OtLink>,
    /// 0 unset, 1 ok, 2 error.
    pub status_code: i32,
    /// The error message, when the status is an error.
    pub status_message: Option<String>,
    /// W3C trace flags and the parent-is-remote bits, as received.
    pub flags: u32,
}

/// A span event.
#[derive(Debug, Clone, Default)]
pub struct OtEvent {
    /// When it happened, nanoseconds since the epoch.
    pub time_ns: u64,
    /// The event name.
    pub name: String,
    /// Attributes.
    pub attrs: Attrs,
}

/// A span link.
#[derive(Debug, Clone, Default)]
pub struct OtLink {
    /// Hex trace id.
    pub trace_id: String,
    /// Hex span id. Empty for a link to a whole trace.
    pub span_id: String,
    /// Attributes.
    pub attrs: Attrs,
}

/// One log record.
#[derive(Debug, Clone, Default)]
pub struct OtLog {
    /// When the line was emitted.
    pub time_ns: u64,
    /// When mcpd received it.
    pub observed_ns: u64,
    /// OTLP severity number, 1 to 24.
    pub severity_number: i32,
    /// The level's name.
    pub severity_text: String,
    /// The message.
    pub body: String,
    /// Attributes.
    pub attrs: Attrs,
    /// Hex trace id.
    pub trace_id: Option<String>,
    /// Hex span id.
    pub span_id: Option<String>,
}

/// One metric data point.
#[derive(Debug, Clone, Default)]
pub struct OtPoint {
    /// Labels.
    pub attrs: Attrs,
    /// When it was observed.
    pub time_ns: u64,
    /// The value.
    pub value: f64,
    /// The trace and span that produced it, when the point carried one.
    pub exemplar: Option<(String, Option<String>)>,
}

/// One histogram data point.
#[derive(Debug, Clone, Default)]
pub struct OtHistogram {
    /// Labels.
    pub attrs: Attrs,
    /// When it was observed.
    pub time_ns: u64,
    /// Observations in the point.
    pub count: u64,
    /// Their sum, when known.
    pub sum: Option<f64>,
    /// One count per bucket, one more than there are bounds.
    pub bucket_counts: Vec<u64>,
    /// Upper bounds of every bucket but the last.
    pub bounds: Vec<f64>,
    /// The trace and span that produced it.
    pub exemplar: Option<(String, Option<String>)>,
}

/// What kind of metric, with its points.
#[derive(Debug, Clone)]
pub enum OtData {
    /// A point-in-time value.
    Gauge(Vec<OtPoint>),
    /// A monotonic sum. mcpd adds counter points together, so each one is an
    /// increment, which OTLP calls delta temporality.
    Sum(Vec<OtPoint>),
    /// A distribution, also delta.
    Histogram(Vec<OtHistogram>),
}

/// One metric and its points.
#[derive(Debug, Clone)]
pub struct OtMetric {
    /// The metric name.
    pub name: String,
    /// Its unit.
    pub unit: String,
    /// Its points.
    pub data: OtData,
}

/// Items that share a resource.
#[derive(Debug, Clone, Default)]
pub struct Group<T> {
    /// The resource attributes: `service.name` and the like.
    pub resource: Attrs,
    /// The instrumentation scope's name. `None` is mcpd's own.
    pub scope: Option<String>,
    /// The items.
    pub items: Vec<T>,
}

/// One OTLP export request.
#[derive(Debug, Clone)]
pub enum Export {
    /// `ExportTraceServiceRequest`.
    Traces(Vec<Group<OtSpan>>),
    /// `ExportLogsServiceRequest`.
    Logs(Vec<Group<OtLog>>),
    /// `ExportMetricsServiceRequest`.
    Metrics(Vec<Group<OtMetric>>),
}

/// The three OTLP signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Signal {
    /// Spans.
    Traces,
    /// Log records.
    Logs,
    /// Metric points.
    Metrics,
}

impl Signal {
    /// The path segment OTLP/HTTP uses: `traces`, `logs` or `metrics`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Traces => "traces",
            Self::Logs => "logs",
            Self::Metrics => "metrics",
        }
    }

    /// The gRPC method for this signal's export service.
    #[must_use]
    pub const fn grpc_path(self) -> &'static str {
        match self {
            Self::Traces => "/opentelemetry.proto.collector.trace.v1.TraceService/Export",
            Self::Logs => "/opentelemetry.proto.collector.logs.v1.LogsService/Export",
            Self::Metrics => "/opentelemetry.proto.collector.metrics.v1.MetricsService/Export",
        }
    }
}

impl Export {
    /// Which signal this request carries.
    #[must_use]
    pub const fn signal(&self) -> Signal {
        match self {
            Self::Traces(_) => Signal::Traces,
            Self::Logs(_) => Signal::Logs,
            Self::Metrics(_) => Signal::Metrics,
        }
    }

    /// How many items it carries: spans, records or points.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Traces(g) => g.iter().map(|g| g.items.len()).sum(),
            Self::Logs(g) => g.iter().map(|g| g.items.len()).sum(),
            Self::Metrics(g) => g
                .iter()
                .flat_map(|g| &g.items)
                .map(|m| match &m.data {
                    OtData::Gauge(p) | OtData::Sum(p) => p.len(),
                    OtData::Histogram(p) => p.len(),
                })
                .sum(),
        }
    }

    /// Whether it carries nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The request as OTLP/JSON.
    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            Self::Traces(groups) => json!({
                "resourceSpans": groups.iter().map(|g| json!({
                    "resource": { "attributes": attrs_json(&g.resource) },
                    "scopeSpans": [{
                        "scope": scope_json(g.scope.as_deref()),
                        "spans": g.items.iter().map(span_json).collect::<Vec<_>>(),
                    }],
                })).collect::<Vec<_>>()
            }),
            Self::Logs(groups) => json!({
                "resourceLogs": groups.iter().map(|g| json!({
                    "resource": { "attributes": attrs_json(&g.resource) },
                    "scopeLogs": [{
                        "scope": scope_json(g.scope.as_deref()),
                        "logRecords": g.items.iter().map(log_json).collect::<Vec<_>>(),
                    }],
                })).collect::<Vec<_>>()
            }),
            Self::Metrics(groups) => json!({
                "resourceMetrics": groups.iter().map(|g| json!({
                    "resource": { "attributes": attrs_json(&g.resource) },
                    "scopeMetrics": [{
                        "scope": scope_json(g.scope.as_deref()),
                        "metrics": g.items.iter().map(metric_json).collect::<Vec<_>>(),
                    }],
                })).collect::<Vec<_>>()
            }),
        }
    }

    /// The request as protobuf.
    #[must_use]
    pub fn to_protobuf(&self) -> Vec<u8> {
        let mut pb = Pb::default();
        match self {
            Self::Traces(groups) => {
                for g in groups {
                    pb.message(1, |rs| {
                        rs.message(1, |r| attrs_pb(r, 1, &g.resource));
                        rs.message(2, |ss| {
                            ss.message(1, |x| scope_pb(x, g.scope.as_deref()));
                            for s in &g.items {
                                ss.message(2, |p| span_pb(p, s));
                            }
                        });
                    });
                }
            }
            Self::Logs(groups) => {
                for g in groups {
                    pb.message(1, |rl| {
                        rl.message(1, |r| attrs_pb(r, 1, &g.resource));
                        rl.message(2, |sl| {
                            sl.message(1, |x| scope_pb(x, g.scope.as_deref()));
                            for l in &g.items {
                                sl.message(2, |p| log_pb(p, l));
                            }
                        });
                    });
                }
            }
            Self::Metrics(groups) => {
                for g in groups {
                    pb.message(1, |rm| {
                        rm.message(1, |r| attrs_pb(r, 1, &g.resource));
                        rm.message(2, |sm| {
                            sm.message(1, |x| scope_pb(x, g.scope.as_deref()));
                            for m in &g.items {
                                sm.message(2, |p| metric_pb(p, m));
                            }
                        });
                    });
                }
            }
        }
        pb.0
    }
}

// ------------------------------------------------------------------ JSON ---

fn scope_json(name: Option<&str>) -> Value {
    match name {
        Some(n) => json!({ "name": n }),
        None => json!({ "name": "mcpd", "version": mcpd_core::VERSION }),
    }
}

/// OTLP/JSON types every value. An integer goes out as a string so it
/// survives a JSON parser's 53-bit limit, which the specification requires.
pub(crate) fn value_json(v: &AnyValue) -> Value {
    match v {
        AnyValue::Str(s) => json!({ "stringValue": s }),
        AnyValue::Bool(b) => json!({ "boolValue": b }),
        AnyValue::Int(i) => json!({ "intValue": i.to_string() }),
        AnyValue::Double(d) => json!({ "doubleValue": d }),
        AnyValue::Array(a) => {
            json!({ "arrayValue": { "values": a.iter().map(value_json).collect::<Vec<_>>() } })
        }
        AnyValue::Map(m) => json!({ "kvlistValue": { "values": attrs_json(m) } }),
    }
}

fn attrs_json(attrs: &Attrs) -> Vec<Value> {
    attrs.iter().map(|(k, v)| json!({ "key": k, "value": value_json(v) })).collect()
}

fn span_json(s: &OtSpan) -> Value {
    let mut out = json!({
        "traceId": s.trace_id,
        "spanId": s.span_id,
        "name": s.name,
        "kind": s.kind,
        "startTimeUnixNano": s.start_ns.to_string(),
        "endTimeUnixNano": s.end_ns.to_string(),
        "attributes": attrs_json(&s.attrs),
        "status": { "code": s.status_code },
    });
    if let Some(p) = &s.parent_span_id {
        out["parentSpanId"] = json!(p);
    }
    if let Some(m) = &s.status_message {
        out["status"]["message"] = json!(m);
    }
    if s.flags != 0 {
        out["flags"] = json!(s.flags);
    }
    if !s.events.is_empty() {
        out["events"] = s
            .events
            .iter()
            .map(|e| {
                json!({
                    "timeUnixNano": e.time_ns.to_string(),
                    "name": e.name,
                    "attributes": attrs_json(&e.attrs),
                })
            })
            .collect();
    }
    if !s.links.is_empty() {
        out["links"] = s
            .links
            .iter()
            .map(|l| {
                json!({
                    "traceId": l.trace_id,
                    "spanId": l.span_id,
                    "attributes": attrs_json(&l.attrs),
                })
            })
            .collect();
    }
    out
}

fn log_json(l: &OtLog) -> Value {
    let mut out = json!({
        "timeUnixNano": l.time_ns.to_string(),
        "observedTimeUnixNano": l.observed_ns.to_string(),
        "severityNumber": l.severity_number,
        "severityText": l.severity_text,
        "body": { "stringValue": l.body },
        "attributes": attrs_json(&l.attrs),
    });
    if let Some(t) = &l.trace_id {
        out["traceId"] = json!(t);
    }
    if let Some(s) = &l.span_id {
        out["spanId"] = json!(s);
    }
    out
}

fn exemplar_json(e: Option<&(String, Option<String>)>, time_ns: u64, value: f64) -> Vec<Value> {
    e.map(|(trace, span)| {
        let mut x = json!({
            "traceId": trace,
            "timeUnixNano": time_ns.to_string(),
            "asDouble": value,
        });
        if let Some(s) = span {
            x["spanId"] = json!(s);
        }
        vec![x]
    })
    .unwrap_or_default()
}

fn point_json(p: &OtPoint) -> Value {
    let mut out = json!({
        "attributes": attrs_json(&p.attrs),
        "timeUnixNano": p.time_ns.to_string(),
        "asDouble": p.value,
    });
    let ex = exemplar_json(p.exemplar.as_ref(), p.time_ns, p.value);
    if !ex.is_empty() {
        out["exemplars"] = json!(ex);
    }
    out
}

/// OTLP's `AggregationTemporality`: 1 is delta.
const DELTA: i32 = 1;

fn metric_json(m: &OtMetric) -> Value {
    let mut out = json!({ "name": m.name, "unit": m.unit });
    match &m.data {
        OtData::Gauge(points) => {
            out["gauge"] =
                json!({ "dataPoints": points.iter().map(point_json).collect::<Vec<_>>() });
        }
        OtData::Sum(points) => {
            out["sum"] = json!({
                "dataPoints": points.iter().map(point_json).collect::<Vec<_>>(),
                "aggregationTemporality": DELTA,
                "isMonotonic": true,
            });
        }
        OtData::Histogram(points) => {
            out["histogram"] = json!({
                "aggregationTemporality": DELTA,
                "dataPoints": points.iter().map(|h| {
                    let mut p = json!({
                        "attributes": attrs_json(&h.attrs),
                        "timeUnixNano": h.time_ns.to_string(),
                        "count": h.count.to_string(),
                        "bucketCounts": h.bucket_counts.iter().map(ToString::to_string).collect::<Vec<_>>(),
                        "explicitBounds": h.bounds,
                    });
                    if let Some(s) = h.sum {
                        p["sum"] = json!(s);
                    }
                    let ex = exemplar_json(h.exemplar.as_ref(), h.time_ns, h.sum.unwrap_or(0.0));
                    if !ex.is_empty() {
                        p["exemplars"] = json!(ex);
                    }
                    p
                }).collect::<Vec<_>>(),
            });
        }
    }
    out
}

// -------------------------------------------------------------- protobuf ---

/// A protobuf message being written.
#[derive(Default)]
pub(crate) struct Pb(pub(crate) Vec<u8>);

impl Pb {
    fn varint(&mut self, mut v: u64) {
        while v >= 0x80 {
            self.0.push((v as u8) | 0x80);
            v >>= 7;
        }
        self.0.push(v as u8);
    }

    fn tag(&mut self, field: u32, wire: u8) {
        self.varint((u64::from(field) << 3) | u64::from(wire));
    }

    /// A varint field, left out at zero as proto3 does.
    fn uint(&mut self, field: u32, v: u64) {
        if v != 0 {
            self.tag(field, 0);
            self.varint(v);
        }
    }

    /// A signed varint field. A negative `int64` is ten bytes of two's
    /// complement, which is what the cast produces.
    fn int(&mut self, field: u32, v: i64) {
        if v != 0 {
            self.tag(field, 0);
            self.varint(v as u64);
        }
    }

    fn fixed64(&mut self, field: u32, v: u64) {
        if v != 0 {
            self.tag(field, 1);
            self.0.extend_from_slice(&v.to_le_bytes());
        }
    }

    fn fixed32(&mut self, field: u32, v: u32) {
        if v != 0 {
            self.tag(field, 5);
            self.0.extend_from_slice(&v.to_le_bytes());
        }
    }

    /// A double written even when it is zero: every double here is a member
    /// of a oneof, where presence is the value.
    fn double(&mut self, field: u32, v: f64) {
        self.tag(field, 1);
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn bytes(&mut self, field: u32, b: &[u8]) {
        self.tag(field, 2);
        self.varint(b.len() as u64);
        self.0.extend_from_slice(b);
    }

    /// A string field, left out when empty.
    fn string(&mut self, field: u32, s: &str) {
        if !s.is_empty() {
            self.bytes(field, s.as_bytes());
        }
    }

    /// A hex id as the bytes it spells, left out when empty or not hex.
    fn id(&mut self, field: u32, hex_id: &str) {
        if let Ok(b) = hex::decode(hex_id) {
            if !b.is_empty() {
                self.bytes(field, &b);
            }
        }
    }

    /// An embedded message, written even when empty.
    pub(crate) fn message(&mut self, field: u32, build: impl FnOnce(&mut Pb)) {
        let mut inner = Pb::default();
        build(&mut inner);
        self.bytes(field, &inner.0);
    }
}

fn scope_pb(p: &mut Pb, name: Option<&str>) {
    match name {
        Some(n) => p.string(1, n),
        None => {
            p.string(1, "mcpd");
            p.string(2, mcpd_core::VERSION);
        }
    }
}

fn value_pb(p: &mut Pb, v: &AnyValue) {
    match v {
        // Written even when empty: a oneof member is present by being written.
        AnyValue::Str(s) => p.bytes(1, s.as_bytes()),
        AnyValue::Bool(b) => {
            p.tag(2, 0);
            p.varint(u64::from(*b));
        }
        AnyValue::Int(i) => {
            p.tag(3, 0);
            p.varint(*i as u64);
        }
        AnyValue::Double(d) => p.double(4, *d),
        AnyValue::Array(a) => p.message(5, |arr| {
            for item in a {
                arr.message(1, |x| value_pb(x, item));
            }
        }),
        AnyValue::Map(m) => p.message(6, |kv| attrs_pb(kv, 1, m)),
    }
}

fn attrs_pb(p: &mut Pb, field: u32, attrs: &Attrs) {
    for (k, v) in attrs {
        p.message(field, |kv| {
            kv.string(1, k);
            kv.message(2, |x| value_pb(x, v));
        });
    }
}

fn span_pb(p: &mut Pb, s: &OtSpan) {
    p.id(1, &s.trace_id);
    p.id(2, &s.span_id);
    if let Some(parent) = &s.parent_span_id {
        p.id(4, parent);
    }
    p.string(5, &s.name);
    p.uint(6, s.kind.max(0) as u64);
    p.fixed64(7, s.start_ns);
    p.fixed64(8, s.end_ns);
    attrs_pb(p, 9, &s.attrs);
    for e in &s.events {
        p.message(11, |ev| {
            ev.fixed64(1, e.time_ns);
            ev.string(2, &e.name);
            attrs_pb(ev, 3, &e.attrs);
        });
    }
    for l in &s.links {
        p.message(13, |lk| {
            lk.id(1, &l.trace_id);
            lk.id(2, &l.span_id);
            attrs_pb(lk, 4, &l.attrs);
        });
    }
    p.message(15, |st| {
        if let Some(m) = &s.status_message {
            st.string(2, m);
        }
        st.uint(3, s.status_code.max(0) as u64);
    });
    p.fixed32(16, s.flags);
}

fn log_pb(p: &mut Pb, l: &OtLog) {
    p.fixed64(1, l.time_ns);
    p.uint(2, l.severity_number.max(0) as u64);
    p.string(3, &l.severity_text);
    p.message(5, |b| b.bytes(1, l.body.as_bytes()));
    attrs_pb(p, 6, &l.attrs);
    if let Some(t) = &l.trace_id {
        p.id(9, t);
    }
    if let Some(s) = &l.span_id {
        p.id(10, s);
    }
    p.fixed64(11, l.observed_ns);
}

fn exemplar_pb(p: &mut Pb, field: u32, e: &(String, Option<String>), time_ns: u64, v: f64) {
    p.message(field, |x| {
        x.fixed64(2, time_ns);
        x.double(3, v);
        if let Some(s) = &e.1 {
            x.id(4, s);
        }
        x.id(5, &e.0);
    });
}

fn point_pb(p: &mut Pb, pt: &OtPoint) {
    p.fixed64(3, pt.time_ns);
    p.double(4, pt.value);
    if let Some(e) = &pt.exemplar {
        exemplar_pb(p, 5, e, pt.time_ns, pt.value);
    }
    attrs_pb(p, 7, &pt.attrs);
}

fn metric_pb(p: &mut Pb, m: &OtMetric) {
    p.string(1, &m.name);
    p.string(3, &m.unit);
    match &m.data {
        OtData::Gauge(points) => p.message(5, |g| {
            for pt in points {
                g.message(1, |x| point_pb(x, pt));
            }
        }),
        OtData::Sum(points) => p.message(7, |s| {
            for pt in points {
                s.message(1, |x| point_pb(x, pt));
            }
            s.int(2, i64::from(DELTA));
            s.uint(3, 1);
        }),
        OtData::Histogram(points) => p.message(9, |h| {
            for hp in points {
                h.message(1, |x| {
                    x.fixed64(3, hp.time_ns);
                    // `count` is a fixed64 that is meaningful at zero, so it
                    // is written whatever its value.
                    x.tag(4, 1);
                    x.0.extend_from_slice(&hp.count.to_le_bytes());
                    if let Some(sum) = hp.sum {
                        x.double(5, sum);
                    }
                    if !hp.bucket_counts.is_empty() {
                        let mut packed = Vec::with_capacity(hp.bucket_counts.len() * 8);
                        for c in &hp.bucket_counts {
                            packed.extend_from_slice(&c.to_le_bytes());
                        }
                        x.bytes(6, &packed);
                    }
                    if !hp.bounds.is_empty() {
                        let mut packed = Vec::with_capacity(hp.bounds.len() * 8);
                        for b in &hp.bounds {
                            packed.extend_from_slice(&b.to_le_bytes());
                        }
                        x.bytes(7, &packed);
                    }
                    if let Some(e) = &hp.exemplar {
                        exemplar_pb(x, 8, e, hp.time_ns, hp.sum.unwrap_or(0.0));
                    }
                    attrs_pb(x, 9, &hp.attrs);
                });
            }
            h.int(2, i64::from(DELTA));
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_span() -> Export {
        Export::Traces(vec![Group {
            resource: vec![("service.name".into(), AnyValue::Str("checkout".into()))],
            scope: None,
            items: vec![OtSpan {
                trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".into(),
                span_id: "00f067aa0ba902b7".into(),
                parent_span_id: Some("00f067aa0ba902b8".into()),
                name: "chat".into(),
                kind: 1,
                start_ns: 1_700_000_000_000_000_000,
                end_ns: 1_700_000_002_000_000_000,
                attrs: vec![
                    ("gen_ai.request.model".into(), AnyValue::Str("gpt-4o".into())),
                    ("gen_ai.usage.input_tokens".into(), AnyValue::Int(1200)),
                    ("ratio".into(), AnyValue::Double(0.5)),
                    ("ok".into(), AnyValue::Bool(true)),
                    ("empty".into(), AnyValue::Str(String::new())),
                ],
                events: vec![OtEvent {
                    time_ns: 1_700_000_001_000_000_000,
                    name: "exception".into(),
                    attrs: vec![("exception.message".into(), AnyValue::Str("boom".into()))],
                }],
                links: vec![OtLink {
                    trace_id: "5bf92f3577b34da6a3ce929d0e0e4736".into(),
                    span_id: "10f067aa0ba902b7".into(),
                    attrs: vec![("mcpd.link.kind".into(), AnyValue::Str("retry_of".into()))],
                }],
                status_code: 2,
                status_message: Some("429 from provider".into()),
                flags: 0x301,
            }],
        }])
    }

    #[test]
    fn protobuf_reads_back_through_mcpds_own_decoder() {
        let bytes = one_span().to_protobuf();
        let decoded = mcpd_core::otlp::protobuf::decode_traces(&bytes).expect("decodes");
        assert_eq!(decoded.len(), 1);
        let s = &decoded[0];
        assert_eq!(s.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(s.span_id, "00f067aa0ba902b7");
        assert_eq!(s.parent_span_id.as_deref(), Some("00f067aa0ba902b8"));
        assert_eq!(s.name, "chat");
        assert_eq!(s.start_unix_nano, 1_700_000_000_000_000_000);
        assert_eq!(s.end_unix_nano, 1_700_000_002_000_000_000);
        assert_eq!(s.status_code, 2);
        assert_eq!(s.status_message.as_deref(), Some("429 from provider"));
        assert_eq!(s.flags, 0x301);
        assert_eq!(s.attributes["gen_ai.request.model"], json!("gpt-4o"));
        assert_eq!(s.attributes["gen_ai.usage.input_tokens"], json!(1200));
        assert_eq!(s.attributes["ratio"], json!(0.5));
        assert_eq!(s.attributes["ok"], json!(true));
        assert_eq!(s.attributes["empty"], json!(""));
        assert_eq!(s.resource["service.name"], json!("checkout"));
        assert_eq!(s.events.len(), 1);
        assert_eq!(s.events[0].name, "exception");
        assert_eq!(s.links.len(), 1);
        assert_eq!(s.links[0].trace_id, "5bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(s.links[0].attributes["mcpd.link.kind"], json!("retry_of"));
    }

    #[test]
    fn json_reads_back_through_mcpds_own_decoder() {
        let v = one_span().to_json();
        let decoded = mcpd_core::otlp::decode_json(&v).expect("decodes");
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].links.len(), 1);
        assert_eq!(decoded[0].events.len(), 1);
        assert_eq!(decoded[0].attributes["gen_ai.usage.input_tokens"], json!(1200));
    }

    #[test]
    fn a_negative_integer_is_ten_bytes_of_twos_complement() {
        let mut p = Pb::default();
        value_pb(&mut p, &AnyValue::Int(-1));
        // Tag, then ten bytes.
        assert_eq!(p.0.len(), 11);
    }

    #[test]
    fn counts_are_by_item_not_by_group() {
        assert_eq!(one_span().len(), 1);
        let m = Export::Metrics(vec![Group {
            resource: vec![],
            scope: None,
            items: vec![OtMetric {
                name: "x".into(),
                unit: String::new(),
                data: OtData::Sum(vec![OtPoint::default(), OtPoint::default()]),
            }],
        }]);
        assert_eq!(m.len(), 2);
    }
}
