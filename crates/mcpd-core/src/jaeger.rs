//! Jaeger ingest.
//!
//! Jaeger carries one batch in two encodings: Thrift binary, which its
//! collector takes over HTTP at `/api/traces`, and protobuf, which it takes
//! over gRPC as `jaeger.api_v2.CollectorService/PostSpans`. Both decoders read
//! into [`JaegerSpan`], and [`lower`] turns that into the [`OtlpSpan`] the
//! OpenTelemetry doors produce, so the semantic conventions in its tags are
//! read by the one mapping in [`crate::otlp`].
//!
//! What each Jaeger field becomes:
//!
//! - the process's `serviceName` is the resource's `service.name`, and its tags
//!   are resource attributes.
//! - span tags are span attributes, typed as Jaeger typed them.
//! - each log is a span event, named by its `event` field.
//! - the parent is `parentSpanId`, or the first `CHILD_OF` reference into the
//!   same trace. Every other reference is a span link whose
//!   `opentracing.ref_type` is `child_of` or `follows_from`, which mcpd reads
//!   as the link's kind.
//! - the `error` tag, or `otel.status_code`, sets the status.
//! - the sampled and debug flags set the sampled bit.

use crate::otlp::protobuf::{base64_encode, Reader, Wire};
use crate::otlp::{OtlpEvent, OtlpLink, OtlpSpan, MAX_PER_SPAN};
use crate::zipkin::{hex_id, hex_id_bytes};
use crate::{Error, Result};
use serde_json::{Map, Value};

/// The process that recorded a batch, or one span.
#[derive(Debug, Clone, Default)]
pub struct Process {
    /// The service name.
    pub service_name: String,
    /// Process tags, such as the host name and the client version.
    pub tags: Map<String, Value>,
}

/// How one span refers to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefType {
    /// The referenced span is this span's parent.
    ChildOf,
    /// The referenced span caused this one without waiting for it.
    FollowsFrom,
}

/// A reference from one span to another.
#[derive(Debug, Clone)]
pub struct SpanRef {
    /// How the spans relate.
    pub ref_type: RefType,
    /// Hex trace id.
    pub trace_id: String,
    /// Hex span id.
    pub span_id: String,
}

/// A Jaeger span, as either encoding carries it.
#[derive(Debug, Clone, Default)]
pub struct JaegerSpan {
    /// Hex trace id, 32 characters.
    pub trace_id: String,
    /// Hex span id, 16 characters.
    pub span_id: String,
    /// Thrift's `parentSpanId`, when not zero.
    pub parent_span_id: Option<String>,
    /// The operation name.
    pub operation_name: String,
    /// References to other spans.
    pub references: Vec<SpanRef>,
    /// Bit 0 is sampled, bit 1 is debug.
    pub flags: u32,
    /// Start, microseconds since the Unix epoch.
    pub start_us: u64,
    /// Duration in microseconds.
    pub duration_us: u64,
    /// Span tags.
    pub tags: Map<String, Value>,
    /// `(microseconds, fields)` pairs.
    pub logs: Vec<(u64, Map<String, Value>)>,
    /// A process of the span's own, which overrides the batch's.
    pub process: Option<Process>,
}

/// One batch: a process and the spans it recorded.
#[derive(Debug, Clone, Default)]
pub struct Batch {
    /// The batch's process.
    pub process: Process,
    /// Its spans.
    pub spans: Vec<JaegerSpan>,
}

impl Batch {
    /// Every span, lowered.
    #[must_use]
    pub fn lower(&self) -> Vec<OtlpSpan> {
        self.spans.iter().map(|s| lower(s, &self.process)).collect()
    }
}

// --------------------------------------------------------------- Thrift ---

/// How deep a Thrift value may nest before the reader refuses it. Jaeger's
/// own structures are three deep; the limit is there for the skipper, which
/// walks fields it does not know and would otherwise recurse without bound.
const MAX_THRIFT_DEPTH: u32 = 16;

const T_STOP: u8 = 0;
const T_BOOL: u8 = 2;
const T_BYTE: u8 = 3;
const T_DOUBLE: u8 = 4;
const T_I16: u8 = 6;
const T_I32: u8 = 8;
const T_I64: u8 = 10;
const T_STRING: u8 = 11;
const T_STRUCT: u8 = 12;
const T_MAP: u8 = 13;
const T_SET: u8 = 14;
const T_LIST: u8 = 15;

/// A cursor over Thrift's binary protocol.
struct Thrift<'a> {
    buf: &'a [u8],
    pos: usize,
}

fn truncated() -> Error {
    Error::Otlp("the Thrift batch ends in the middle of a value".into())
}

impl<'a> Thrift<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|e| *e <= self.buf.len()).ok_or_else(truncated)?;
        let out = &self.buf[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn i16(&mut self) -> Result<i16> {
        let b = self.take(2)?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    }

    fn i32(&mut self) -> Result<i32> {
        let b = self.take(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i64(&mut self) -> Result<i64> {
        let mut b = [0u8; 8];
        b.copy_from_slice(self.take(8)?);
        Ok(i64::from_be_bytes(b))
    }

    fn double(&mut self) -> Result<f64> {
        Ok(f64::from_bits(self.i64()? as u64))
    }

    fn binary(&mut self) -> Result<&'a [u8]> {
        let len = usize::try_from(self.i32()?)
            .map_err(|_| Error::Otlp("a Thrift string has a negative length".into()))?;
        self.take(len)
    }

    fn string(&mut self) -> Result<String> {
        Ok(String::from_utf8_lossy(self.binary()?).into_owned())
    }

    /// A field header: its type, and its id unless the type is `STOP`.
    fn field(&mut self) -> Result<(u8, i16)> {
        let ty = self.u8()?;
        if ty == T_STOP {
            return Ok((T_STOP, 0));
        }
        Ok((ty, self.i16()?))
    }

    /// A list or set header. The count is checked against the bytes left, so
    /// a forged count cannot make the reader reserve memory it will never
    /// fill.
    fn list(&mut self) -> Result<(u8, usize)> {
        let ty = self.u8()?;
        let n = usize::try_from(self.i32()?)
            .map_err(|_| Error::Otlp("a Thrift list has a negative length".into()))?;
        if n > self.buf.len() - self.pos {
            return Err(truncated());
        }
        Ok((ty, n))
    }

    fn skip(&mut self, ty: u8, depth: u32) -> Result<()> {
        if depth > MAX_THRIFT_DEPTH {
            return Err(Error::Otlp("the Thrift batch nests too deeply".into()));
        }
        match ty {
            T_BOOL | T_BYTE => {
                self.take(1)?;
            }
            T_I16 => {
                self.take(2)?;
            }
            T_I32 => {
                self.take(4)?;
            }
            T_DOUBLE | T_I64 => {
                self.take(8)?;
            }
            T_STRING => {
                self.binary()?;
            }
            T_STRUCT => loop {
                let (t, _) = self.field()?;
                if t == T_STOP {
                    break;
                }
                self.skip(t, depth + 1)?;
            },
            T_MAP => {
                let k = self.u8()?;
                let v = self.u8()?;
                let n = usize::try_from(self.i32()?)
                    .map_err(|_| Error::Otlp("a Thrift map has a negative length".into()))?;
                if n > self.buf.len() - self.pos {
                    return Err(truncated());
                }
                for _ in 0..n {
                    self.skip(k, depth + 1)?;
                    self.skip(v, depth + 1)?;
                }
            }
            T_SET | T_LIST => {
                let (t, n) = self.list()?;
                for _ in 0..n {
                    self.skip(t, depth + 1)?;
                }
            }
            other => return Err(Error::Otlp(format!("unknown Thrift type {other}"))),
        }
        Ok(())
    }

    /// A list of structs, each read by `item`. Anything else in the slot is
    /// skipped.
    fn structs<T>(&mut self, ty: u8, item: fn(&mut Self) -> Result<T>) -> Result<Vec<T>> {
        if ty != T_LIST {
            self.skip(ty, 0)?;
            return Ok(Vec::new());
        }
        let (t, n) = self.list()?;
        if t != T_STRUCT {
            for _ in 0..n {
                self.skip(t, 1)?;
            }
            return Ok(Vec::new());
        }
        let mut out = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            out.push(item(self)?);
        }
        Ok(out)
    }
}

/// `jaeger.Tag`
fn thrift_tag(t: &mut Thrift<'_>) -> Result<(String, Value)> {
    let (mut key, mut vtype) = (String::new(), 0);
    let (mut s, mut d, mut b, mut l, mut bin) = (None, None, None, None, None);
    loop {
        let (ty, id) = t.field()?;
        match (id, ty) {
            (_, T_STOP) => break,
            (1, T_STRING) => key = t.string()?,
            (2, T_I32) => vtype = t.i32()?,
            (3, T_STRING) => s = Some(t.string()?),
            (4, T_DOUBLE) => d = Some(t.double()?),
            (5, T_BOOL) => b = Some(t.u8()? != 0),
            (6, T_I64) => l = Some(t.i64()?),
            (7, T_STRING) => bin = Some(base64_encode(t.binary()?)),
            (_, ty) => t.skip(ty, 1)?,
        }
    }
    // TagType: STRING 0, DOUBLE 1, BOOL 2, LONG 3, BINARY 4.
    let value = match vtype {
        1 => d.map(Value::from),
        2 => b.map(Value::Bool),
        3 => l.map(Value::from),
        4 => bin.map(Value::String),
        _ => s.map(Value::String),
    };
    Ok((key, value.unwrap_or(Value::Null)))
}

fn thrift_tags(t: &mut Thrift<'_>, ty: u8) -> Result<Map<String, Value>> {
    Ok(t.structs(ty, thrift_tag)?.into_iter().filter(|(k, _)| !k.is_empty()).collect())
}

/// `jaeger.Log`
fn thrift_log(t: &mut Thrift<'_>) -> Result<(u64, Map<String, Value>)> {
    let (mut ts, mut fields) = (0u64, Map::new());
    loop {
        let (ty, id) = t.field()?;
        match (id, ty) {
            (_, T_STOP) => break,
            (1, T_I64) => ts = u64::try_from(t.i64()?).unwrap_or(0),
            (2, ty) => fields = thrift_tags(t, ty)?,
            (_, ty) => t.skip(ty, 1)?,
        }
    }
    Ok((ts, fields))
}

/// A 128-bit id from Thrift's two signed halves.
fn thrift_trace_id(high: i64, low: i64) -> Option<String> {
    hex_id(&format!("{:016x}{:016x}", high as u64, low as u64), 16)
}

fn thrift_span_id(id: i64) -> Option<String> {
    hex_id(&format!("{:016x}", id as u64), 8)
}

/// `jaeger.SpanRef`
fn thrift_ref(t: &mut Thrift<'_>) -> Result<Option<SpanRef>> {
    let (mut kind, mut low, mut high, mut span) = (0, 0, 0, 0);
    loop {
        let (ty, id) = t.field()?;
        match (id, ty) {
            (_, T_STOP) => break,
            (1, T_I32) => kind = t.i32()?,
            (2, T_I64) => low = t.i64()?,
            (3, T_I64) => high = t.i64()?,
            (4, T_I64) => span = t.i64()?,
            (_, ty) => t.skip(ty, 1)?,
        }
    }
    Ok(thrift_trace_id(high, low).zip(thrift_span_id(span)).map(|(trace_id, span_id)| SpanRef {
        ref_type: if kind == 1 { RefType::FollowsFrom } else { RefType::ChildOf },
        trace_id,
        span_id,
    }))
}

/// `jaeger.Span`. `None` when its ids will not read.
fn thrift_span(t: &mut Thrift<'_>) -> Result<Option<JaegerSpan>> {
    let mut out = JaegerSpan::default();
    let (mut low, mut high, mut id, mut parent) = (0, 0, 0, 0);
    loop {
        let (ty, fid) = t.field()?;
        match (fid, ty) {
            (_, T_STOP) => break,
            (1, T_I64) => low = t.i64()?,
            (2, T_I64) => high = t.i64()?,
            (3, T_I64) => id = t.i64()?,
            (4, T_I64) => parent = t.i64()?,
            (5, T_STRING) => out.operation_name = t.string()?,
            (6, ty) => out.references = t.structs(ty, thrift_ref)?.into_iter().flatten().collect(),
            (7, T_I32) => out.flags = t.i32()? as u32,
            (8, T_I64) => out.start_us = u64::try_from(t.i64()?).unwrap_or(0),
            (9, T_I64) => out.duration_us = u64::try_from(t.i64()?).unwrap_or(0),
            (10, ty) => out.tags = thrift_tags(t, ty)?,
            (11, ty) => out.logs = t.structs(ty, thrift_log)?,
            (_, ty) => t.skip(ty, 1)?,
        }
    }
    let (Some(trace), Some(span)) = (thrift_trace_id(high, low), thrift_span_id(id)) else {
        return Ok(None);
    };
    out.trace_id = trace;
    out.span_id = span;
    out.parent_span_id = thrift_span_id(parent);
    Ok(Some(out))
}

/// `jaeger.Process`
fn thrift_process(t: &mut Thrift<'_>) -> Result<Process> {
    let mut out = Process::default();
    loop {
        let (ty, id) = t.field()?;
        match (id, ty) {
            (_, T_STOP) => break,
            (1, T_STRING) => out.service_name = t.string()?,
            (2, ty) => out.tags = thrift_tags(t, ty)?,
            (_, ty) => t.skip(ty, 1)?,
        }
    }
    Ok(out)
}

/// Decode a Thrift binary `jaeger.Batch`, the body of `POST /api/traces`.
///
/// # Errors
/// Returns [`Error::Otlp`] when the bytes are not a Thrift struct.
pub fn decode_thrift(body: &[u8]) -> Result<Batch> {
    let mut t = Thrift { buf: body, pos: 0 };
    let mut out = Batch::default();
    loop {
        let (ty, id) = t.field()?;
        match (id, ty) {
            (_, T_STOP) => break,
            (1, T_STRUCT) => out.process = thrift_process(&mut t)?,
            (2, ty) => out.spans = t.structs(ty, thrift_span)?.into_iter().flatten().collect(),
            (_, ty) => t.skip(ty, 0)?,
        }
    }
    Ok(out)
}

// ------------------------------------------------------------- protobuf ---

/// `google.protobuf.Timestamp` or `Duration`, in microseconds.
fn proto_micros(buf: &[u8]) -> Result<u64> {
    let (mut seconds, mut nanos) = (0i64, 0i64);
    let mut r = Reader::new(buf);
    while !r.done() {
        let (f, w) = r.tag()?;
        match (f, w) {
            (1, Wire::Varint) => seconds = r.varint()? as i64,
            (2, Wire::Varint) => nanos = i64::from(r.varint()? as i32),
            (_, w) => r.skip(w)?,
        }
    }
    let micros = seconds.saturating_mul(1_000_000).saturating_add(nanos / 1_000);
    Ok(u64::try_from(micros).unwrap_or(0))
}

/// `jaeger.api_v2.KeyValue`
fn proto_tag(buf: &[u8]) -> Result<(String, Value)> {
    let (mut key, mut vtype) = (String::new(), 0u64);
    let (mut s, mut b, mut l, mut d, mut bin) = (None, None, None, None, None);
    let mut r = Reader::new(buf);
    while !r.done() {
        let (f, w) = r.tag()?;
        match (f, w) {
            (1, Wire::Bytes) => key = r.string()?,
            (2, Wire::Varint) => vtype = r.varint()?,
            (3, Wire::Bytes) => s = Some(r.string()?),
            (4, Wire::Varint) => b = Some(r.varint()? != 0),
            (5, Wire::Varint) => l = Some(r.varint()? as i64),
            (6, Wire::Fixed64) => d = Some(f64::from_bits(r.fixed64()?)),
            (7, Wire::Bytes) => bin = Some(base64_encode(r.bytes()?)),
            (_, w) => r.skip(w)?,
        }
    }
    // ValueType: STRING 0, BOOL 1, INT64 2, FLOAT64 3, BINARY 4. Not the
    // Thrift order, which is why the two readers each have their own match.
    // proto3 leaves a default value off the wire, so an absent value is the
    // type's zero rather than nothing.
    let value = match vtype {
        1 => Value::Bool(b.unwrap_or(false)),
        2 => Value::from(l.unwrap_or(0)),
        3 => Value::from(d.unwrap_or(0.0)),
        4 => Value::String(bin.unwrap_or_default()),
        _ => Value::String(s.unwrap_or_default()),
    };
    Ok((key, value))
}

fn push_proto_tag(map: &mut Map<String, Value>, buf: &[u8]) -> Result<()> {
    let (k, v) = proto_tag(buf)?;
    if !k.is_empty() {
        map.insert(k, v);
    }
    Ok(())
}

fn proto_process(buf: &[u8]) -> Result<Process> {
    let mut out = Process::default();
    let mut r = Reader::new(buf);
    while !r.done() {
        let (f, w) = r.tag()?;
        match (f, w) {
            (1, Wire::Bytes) => out.service_name = r.string()?,
            (2, Wire::Bytes) => push_proto_tag(&mut out.tags, r.bytes()?)?,
            (_, w) => r.skip(w)?,
        }
    }
    Ok(out)
}

fn proto_span(buf: &[u8]) -> Result<Option<JaegerSpan>> {
    let mut out = JaegerSpan::default();
    let (mut trace, mut id) = (None, None);
    let mut r = Reader::new(buf);
    while !r.done() {
        let (f, w) = r.tag()?;
        match (f, w) {
            (1, Wire::Bytes) => trace = hex_id_bytes(r.bytes()?, 16),
            (2, Wire::Bytes) => id = hex_id_bytes(r.bytes()?, 8),
            (3, Wire::Bytes) => out.operation_name = r.string()?,
            (4, Wire::Bytes) => {
                let mut rr = Reader::new(r.bytes()?);
                let (mut rt, mut rs, mut kind) = (None, None, 0);
                while !rr.done() {
                    let (rf, rw) = rr.tag()?;
                    match (rf, rw) {
                        (1, Wire::Bytes) => rt = hex_id_bytes(rr.bytes()?, 16),
                        (2, Wire::Bytes) => rs = hex_id_bytes(rr.bytes()?, 8),
                        (3, Wire::Varint) => kind = rr.varint()?,
                        (_, w) => rr.skip(w)?,
                    }
                }
                if let (Some(trace_id), Some(span_id)) = (rt, rs) {
                    out.references.push(SpanRef {
                        ref_type: if kind == 1 { RefType::FollowsFrom } else { RefType::ChildOf },
                        trace_id,
                        span_id,
                    });
                }
            }
            (5, Wire::Varint) => out.flags = r.varint()? as u32,
            (6, Wire::Bytes) => out.start_us = proto_micros(r.bytes()?)?,
            (7, Wire::Bytes) => out.duration_us = proto_micros(r.bytes()?)?,
            (8, Wire::Bytes) => push_proto_tag(&mut out.tags, r.bytes()?)?,
            (9, Wire::Bytes) => {
                let mut lr = Reader::new(r.bytes()?);
                let (mut ts, mut fields) = (0, Map::new());
                while !lr.done() {
                    let (lf, lw) = lr.tag()?;
                    match (lf, lw) {
                        (1, Wire::Bytes) => ts = proto_micros(lr.bytes()?)?,
                        (2, Wire::Bytes) => push_proto_tag(&mut fields, lr.bytes()?)?,
                        (_, w) => lr.skip(w)?,
                    }
                }
                out.logs.push((ts, fields));
            }
            (10, Wire::Bytes) => out.process = Some(proto_process(r.bytes()?)?),
            (_, w) => r.skip(w)?,
        }
    }
    let (Some(trace), Some(id)) = (trace, id) else { return Ok(None) };
    out.trace_id = trace;
    out.span_id = id;
    Ok(Some(out))
}

/// Decode a protobuf `jaeger.api_v2.PostSpansRequest`, the message of the
/// gRPC `PostSpans` call.
///
/// # Errors
/// Returns [`Error::Otlp`] only when the outer message cannot be walked.
pub fn decode_post_spans(body: &[u8]) -> Result<Batch> {
    let mut out = Batch::default();
    let mut r = Reader::new(body);
    while !r.done() {
        let (f, w) = r.tag()?;
        if (f, w) != (1, Wire::Bytes) {
            r.skip(w)?;
            continue;
        }
        let mut b = Reader::new(r.bytes()?);
        while !b.done() {
            let (bf, bw) = b.tag()?;
            match (bf, bw) {
                (1, Wire::Bytes) => {
                    if let Ok(Some(span)) = proto_span(b.bytes()?) {
                        out.spans.push(span);
                    }
                }
                (2, Wire::Bytes) => out.process = proto_process(b.bytes()?)?,
                (_, w) => b.skip(w)?,
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- lower ---

/// Turn a Jaeger span into the shape every trace door maps from.
#[must_use]
pub fn lower(j: &JaegerSpan, batch_process: &Process) -> OtlpSpan {
    let process = j.process.as_ref().unwrap_or(batch_process);
    let mut resource = process.tags.clone();
    if !process.service_name.is_empty() {
        resource.insert("service.name".into(), Value::String(process.service_name.clone()));
    }

    // The parent: Thrift's own field, or else the first CHILD_OF into the
    // same trace, which is how the protobuf encoding and OpenTracing state it.
    let parent = j.parent_span_id.clone().or_else(|| {
        j.references
            .iter()
            .find(|r| r.ref_type == RefType::ChildOf && r.trace_id == j.trace_id)
            .map(|r| r.span_id.clone())
    });
    let links = j
        .references
        .iter()
        .filter(|r| {
            !(r.ref_type == RefType::ChildOf
                && r.trace_id == j.trace_id
                && Some(&r.span_id) == parent.as_ref())
        })
        .take(MAX_PER_SPAN)
        .map(|r| {
            let mut attributes = Map::new();
            let kind = match r.ref_type {
                RefType::ChildOf => "child_of",
                RefType::FollowsFrom => "follows_from",
            };
            attributes.insert("opentracing.ref_type".into(), Value::String(kind.into()));
            OtlpLink { trace_id: r.trace_id.clone(), span_id: Some(r.span_id.clone()), attributes }
        })
        .collect();

    let events: Vec<OtlpEvent> = j
        .logs
        .iter()
        .take(MAX_PER_SPAN)
        .map(|(ts, fields)| {
            let mut attributes = fields.clone();
            let name = match attributes.remove("event") {
                Some(Value::String(s)) if !s.is_empty() => s,
                Some(other) if !other.is_null() => other.to_string(),
                _ => "log".to_owned(),
            };
            OtlpEvent { time_unix_nano: u128::from(*ts) * 1_000, name, attributes }
        })
        .collect();

    let tag = |k: &str| j.tags.get(k);
    let tag_str = |k: &str| tag(k).and_then(Value::as_str);
    let flagged = match tag("error") {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
        _ => false,
    };
    // An error log carries the message under OpenTracing's convention.
    let logged_message = j.logs.iter().find_map(|(_, f)| {
        (f.get("event").and_then(Value::as_str) == Some("error"))
            .then(|| f.get("message").and_then(Value::as_str))
            .flatten()
            .map(str::to_owned)
    });
    let (status_code, status_message) = match tag_str("otel.status_code") {
        Some(c) if c.eq_ignore_ascii_case("ok") => (1, None),
        Some(c) if c.eq_ignore_ascii_case("error") => {
            (2, tag_str("otel.status_description").map(str::to_owned).or(logged_message))
        }
        _ if flagged => (2, logged_message),
        _ => (0, None),
    };

    let start = u128::from(j.start_us) * 1_000;
    OtlpSpan {
        trace_id: j.trace_id.clone(),
        span_id: j.span_id.clone(),
        parent_span_id: parent,
        name: if j.operation_name.is_empty() {
            "span".to_owned()
        } else {
            j.operation_name.clone()
        },
        start_unix_nano: start,
        // Jaeger reports a span once it has finished, so a zero duration is an
        // instant, not a span still running.
        end_unix_nano: if start > 0 { start + u128::from(j.duration_us) * 1_000 } else { 0 },
        attributes: j.tags.clone(),
        resource,
        status_code,
        status_message,
        // Jaeger's sampled bit is bit 0 and its debug bit forces sampling.
        flags: u32::from(j.flags & 0b11 != 0),
        links,
        events,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Thrift binary writer, so the tests encode real wire bytes.
    #[derive(Default)]
    struct W(Vec<u8>);
    impl W {
        fn field(&mut self, ty: u8, id: i16) -> &mut Self {
            self.0.push(ty);
            self.0.extend_from_slice(&id.to_be_bytes());
            self
        }
        fn i32(&mut self, id: i16, v: i32) -> &mut Self {
            self.field(T_I32, id);
            self.0.extend_from_slice(&v.to_be_bytes());
            self
        }
        fn i64(&mut self, id: i16, v: i64) -> &mut Self {
            self.field(T_I64, id);
            self.0.extend_from_slice(&v.to_be_bytes());
            self
        }
        fn string(&mut self, id: i16, v: &str) -> &mut Self {
            self.field(T_STRING, id);
            self.0.extend_from_slice(&(v.len() as i32).to_be_bytes());
            self.0.extend_from_slice(v.as_bytes());
            self
        }
        fn list(&mut self, id: i16, items: &[Vec<u8>]) -> &mut Self {
            self.field(T_LIST, id);
            self.0.push(T_STRUCT);
            self.0.extend_from_slice(&(items.len() as i32).to_be_bytes());
            for i in items {
                self.0.extend_from_slice(i);
            }
            self
        }
        fn stop(&mut self) -> Vec<u8> {
            self.0.push(T_STOP);
            std::mem::take(&mut self.0)
        }
    }

    fn string_tag(k: &str, v: &str) -> Vec<u8> {
        W::default().string(1, k).i32(2, 0).string(3, v).stop()
    }

    fn sample() -> Vec<u8> {
        let follows = W::default().i32(1, 1).i64(2, 9).i64(3, 0).i64(4, 77).stop();
        let log = {
            let mut w = W::default();
            w.i64(1, 1_700_000_000_000_100);
            w.list(2, &[string_tag("event", "retry"), string_tag("attempt", "2")]);
            w.stop()
        };
        let span = {
            let mut w = W::default();
            w.i64(1, 0x1234).i64(2, 0).i64(3, 0x55).i64(4, 0x44).string(5, "chat");
            w.list(6, &[follows]);
            w.i32(7, 1).i64(8, 1_700_000_000_000_000).i64(9, 1_500);
            w.list(10, &[string_tag("gen_ai.system", "openai"), string_tag("span.kind", "client")]);
            w.list(11, &[log]);
            w.stop()
        };
        let process = {
            let mut w = W::default();
            w.string(1, "checkout");
            w.list(2, &[string_tag("hostname", "h1")]);
            w.stop()
        };
        let mut batch = W::default();
        batch.field(T_STRUCT, 1);
        batch.0.extend_from_slice(&process);
        batch.list(2, &[span]);
        batch.stop()
    }

    #[test]
    fn a_thrift_batch_lowers_with_every_field() {
        let batch = decode_thrift(&sample()).unwrap();
        let spans = batch.lower();
        assert_eq!(spans.len(), 1);
        let o = &spans[0];
        assert_eq!(o.trace_id, "00000000000000000000000000001234");
        assert_eq!(o.span_id, "0000000000000055");
        assert_eq!(o.parent_span_id.as_deref(), Some("0000000000000044"));
        assert_eq!(o.name, "chat");
        assert_eq!(o.resource["service.name"], "checkout");
        assert_eq!(o.resource["hostname"], "h1");
        assert_eq!(o.attributes["gen_ai.system"], "openai");
        assert_eq!(o.start_unix_nano, 1_700_000_000_000_000_000);
        assert_eq!(o.end_unix_nano, 1_700_000_000_001_500_000);
        assert_eq!(o.flags, 1);
        assert_eq!(o.links.len(), 1);
        assert_eq!(o.links[0].trace_id, "00000000000000000000000000000009");
        assert_eq!(o.links[0].attributes["opentracing.ref_type"], "follows_from");
        assert_eq!(o.events.len(), 1);
        assert_eq!(o.events[0].name, "retry");
        assert_eq!(o.events[0].attributes["attempt"], "2");
    }

    #[test]
    fn a_truncated_batch_is_an_error_not_a_panic() {
        let full = sample();
        for cut in 0..full.len() {
            let _ = decode_thrift(&full[..cut]);
        }
    }

    #[test]
    fn a_forged_list_length_is_refused_before_it_allocates() {
        let mut w = W::default();
        w.field(T_LIST, 2);
        w.0.push(T_STRUCT);
        w.0.extend_from_slice(&i32::MAX.to_be_bytes());
        assert!(decode_thrift(&w.stop()).is_err());
    }

    #[test]
    fn the_first_child_of_is_the_parent_when_no_field_names_one() {
        let j = JaegerSpan {
            trace_id: hex_id("1", 16).unwrap(),
            span_id: hex_id("2", 8).unwrap(),
            references: vec![
                SpanRef {
                    ref_type: RefType::ChildOf,
                    trace_id: hex_id("1", 16).unwrap(),
                    span_id: hex_id("3", 8).unwrap(),
                },
                SpanRef {
                    ref_type: RefType::ChildOf,
                    trace_id: hex_id("1", 16).unwrap(),
                    span_id: hex_id("4", 8).unwrap(),
                },
            ],
            ..JaegerSpan::default()
        };
        let o = lower(&j, &Process::default());
        assert_eq!(o.parent_span_id, hex_id("3", 8));
        assert_eq!(o.links.len(), 1);
        assert_eq!(o.links[0].span_id, hex_id("4", 8));
        assert_eq!(o.links[0].attributes["opentracing.ref_type"], "child_of");
    }
}
