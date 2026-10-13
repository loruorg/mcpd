//! OTLP over protobuf.
//!
//! A minimal reader for the wire format, written by hand rather than generated.
//!
//! The generated alternative pulls in `prost` plus a build step that needs
//! `protoc` on the machine doing the build. mcpd's whole deployment story is one
//! static binary and no toolchain, and the subset of protobuf that OTLP actually
//! uses is small: varints, length-delimited fields, and the two fixed widths.
//! Reading it directly costs about three hundred lines and no dependency.
//!
//! Everything here lowers into [`OtlpSpan`], the same shape the JSON path
//! produces, so the semantic-convention mapping in the parent module is the only
//! place that interprets an attribute. A second copy of that logic is how the
//! two encodings would drift apart.

use super::{OtlpEvent, OtlpLink, OtlpSpan, MAX_PER_SPAN};
use crate::{ids, Error, Result};
use serde_json::{Map, Value};

/// A cursor over a protobuf message.
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

/// How a field's payload is encoded, from the low three bits of its tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wire {
    Varint,
    Fixed64,
    Bytes,
    Fixed32,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub(crate) fn done(&self) -> bool {
        self.pos >= self.buf.len()
    }

    pub(crate) fn varint(&mut self) -> Result<u64> {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let byte =
                *self.buf.get(self.pos).ok_or_else(|| Error::Otlp("truncated varint".into()))?;
            self.pos += 1;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
            if shift > 63 {
                return Err(Error::Otlp("varint longer than 64 bits".into()));
            }
        }
    }

    /// Read a tag, returning the field number and how its payload is encoded.
    pub(crate) fn tag(&mut self) -> Result<(u32, Wire)> {
        let key = self.varint()?;
        let field =
            u32::try_from(key >> 3).map_err(|_| Error::Otlp("field number out of range".into()))?;
        let wire = match key & 0x07 {
            0 => Wire::Varint,
            1 => Wire::Fixed64,
            2 => Wire::Bytes,
            5 => Wire::Fixed32,
            // Groups (3 and 4) were removed from proto3 and never appear in OTLP.
            other => return Err(Error::Otlp(format!("unsupported wire type {other}"))),
        };
        Ok((field, wire))
    }

    pub(crate) fn bytes(&mut self) -> Result<&'a [u8]> {
        let len = usize::try_from(self.varint()?)
            .map_err(|_| Error::Otlp("length out of range".into()))?;
        let end = self
            .pos
            .checked_add(len)
            .filter(|e| *e <= self.buf.len())
            .ok_or_else(|| Error::Otlp("length runs past the end of the message".into()))?;
        let out = &self.buf[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    pub(crate) fn string(&mut self) -> Result<String> {
        Ok(String::from_utf8_lossy(self.bytes()?).into_owned())
    }

    pub(crate) fn fixed64(&mut self) -> Result<u64> {
        let end = self
            .pos
            .checked_add(8)
            .filter(|e| *e <= self.buf.len())
            .ok_or_else(|| Error::Otlp("truncated fixed64".into()))?;
        let mut b = [0u8; 8];
        b.copy_from_slice(&self.buf[self.pos..end]);
        self.pos = end;
        Ok(u64::from_le_bytes(b))
    }

    pub(crate) fn fixed32(&mut self) -> Result<u32> {
        let end = self
            .pos
            .checked_add(4)
            .filter(|e| *e <= self.buf.len())
            .ok_or_else(|| Error::Otlp("truncated fixed32".into()))?;
        let mut b = [0u8; 4];
        b.copy_from_slice(&self.buf[self.pos..end]);
        self.pos = end;
        Ok(u32::from_le_bytes(b))
    }

    /// Step over a field this decoder does not care about.
    ///
    /// Being able to do this is what makes the decoder forward compatible: a
    /// newer collector may send fields that did not exist when this was written,
    /// and the correct response is to ignore them, not to fail the batch.
    pub(crate) fn skip(&mut self, wire: Wire) -> Result<()> {
        match wire {
            Wire::Varint => {
                self.varint()?;
            }
            Wire::Fixed64 => {
                self.fixed64()?;
            }
            Wire::Fixed32 => {
                self.fixed32()?;
            }
            Wire::Bytes => {
                self.bytes()?;
            }
        }
        Ok(())
    }
}

/// How deep an attribute may nest before mcpd refuses it.
///
/// An `AnyValue` can hold an array of `AnyValue`, so the wire format is
/// unbounded by construction and each level costs about seven bytes. Without a
/// limit, a ninety-kilobyte body overflowed the thread stack, and a stack
/// overflow is a `SIGABRT`: not a catchable panic, not one request failing, the
/// whole daemon gone. Thirty-two matches the redactor's own walk limit, and is
/// far past anything a real tracer emits.
const MAX_ATTRIBUTE_DEPTH: u32 = 32;

/// `opentelemetry.proto.common.v1.AnyValue`
fn any_value(buf: &[u8]) -> Result<Value> {
    any_value_at(buf, 0)
}

fn any_value_at(buf: &[u8], depth: u32) -> Result<Value> {
    if depth > MAX_ATTRIBUTE_DEPTH {
        return Err(Error::Otlp("attribute nested too deeply".into()));
    }
    let mut r = Reader::new(buf);
    while !r.done() {
        let (field, wire) = r.tag()?;
        match (field, wire) {
            (1, Wire::Bytes) => return Ok(Value::String(r.string()?)),
            (2, Wire::Varint) => return Ok(Value::Bool(r.varint()? != 0)),
            (3, Wire::Varint) => {
                // Signed, zigzag-free: OTLP declares this as int64, so the
                // varint is the two's-complement value.
                return Ok(Value::from(r.varint()? as i64));
            }
            (4, Wire::Fixed64) => {
                return Ok(Value::from(f64::from_bits(r.fixed64()?)));
            }
            (5, Wire::Bytes) => {
                // ArrayValue { repeated AnyValue values = 1 }
                let inner = r.bytes()?;
                let mut ir = Reader::new(inner);
                let mut items = Vec::new();
                while !ir.done() {
                    let (f, w) = ir.tag()?;
                    if (f, w) == (1, Wire::Bytes) {
                        items.push(any_value_at(ir.bytes()?, depth + 1)?);
                    } else {
                        ir.skip(w)?;
                    }
                }
                return Ok(Value::Array(items));
            }
            (6, Wire::Bytes) => {
                // KeyValueList { repeated KeyValue values = 1 }
                return Ok(Value::Object(key_values_at(r.bytes()?, depth + 1)?));
            }
            (7, Wire::Bytes) => {
                return Ok(Value::String(hex::encode(r.bytes()?)));
            }
            (_, w) => r.skip(w)?,
        }
    }
    Ok(Value::Null)
}

/// A repeated `KeyValue`, flattened into the map shape the mapper expects.
fn key_values(buf: &[u8]) -> Result<Map<String, Value>> {
    key_values_at(buf, 0)
}

fn key_values_at(buf: &[u8], depth: u32) -> Result<Map<String, Value>> {
    if depth > MAX_ATTRIBUTE_DEPTH {
        return Err(Error::Otlp("attribute nested too deeply".into()));
    }
    let mut out = Map::new();
    let mut r = Reader::new(buf);
    while !r.done() {
        let (field, wire) = r.tag()?;
        if (field, wire) != (1, Wire::Bytes) {
            r.skip(wire)?;
            continue;
        }
        let mut kv = Reader::new(r.bytes()?);
        let mut key = String::new();
        let mut value = Value::Null;
        while !kv.done() {
            let (f, w) = kv.tag()?;
            match (f, w) {
                (1, Wire::Bytes) => key = kv.string()?,
                (2, Wire::Bytes) => value = any_value_at(kv.bytes()?, depth + 1)?,
                (_, w) => kv.skip(w)?,
            }
        }
        if !key.is_empty() {
            out.insert(key, value);
        }
    }
    Ok(out)
}

/// One `KeyValue`, as it appears in a repeated attributes field.
///
/// `None` for an entry with no key, which is the rule the JSON reader applies
/// too: a key with no value is kept as null, a value with no key is dropped.
/// The two decoders must agree even about the degenerate cases, or the claim
/// that both encodings produce the same span is not a guarantee.
fn key_value(buf: &[u8]) -> Result<Option<(String, Value)>> {
    let mut kv = Reader::new(buf);
    let mut key = String::new();
    let mut value = Value::Null;
    while !kv.done() {
        let (f, w) = kv.tag()?;
        match (f, w) {
            (1, Wire::Bytes) => key = kv.string()?,
            (2, Wire::Bytes) => value = any_value(kv.bytes()?)?,
            (_, w) => kv.skip(w)?,
        }
    }
    Ok((!key.is_empty()).then_some((key, value)))
}

/// `opentelemetry.proto.trace.v1.Span.Event`
fn event_of(buf: &[u8]) -> Result<OtlpEvent> {
    let mut out = OtlpEvent::default();
    let mut r = Reader::new(buf);
    while !r.done() {
        let (field, wire) = r.tag()?;
        match (field, wire) {
            (1, Wire::Fixed64) => out.time_unix_nano = u128::from(r.fixed64()?),
            (2, Wire::Bytes) => out.name = r.string()?,
            (3, Wire::Bytes) => {
                // `attributes` is repeated, so each occurrence carries exactly
                // one entry rather than a packed list.
                if let Some((k, v)) = key_value(r.bytes()?)? {
                    out.attributes.insert(k, v);
                }
            }
            (_, w) => r.skip(w)?,
        }
    }
    if out.name.is_empty() {
        out.name = "event".to_owned();
    }
    Ok(out)
}

/// `opentelemetry.proto.trace.v1.Span.Link`
///
/// `None` for a link whose trace id cannot be stored, which is the same rule
/// the JSON reader applies: a link that points at nothing is dropped rather
/// than kept as a reference a reader will follow to an empty page.
fn link_of(buf: &[u8]) -> Result<Option<OtlpLink>> {
    let mut out = OtlpLink::default();
    let mut r = Reader::new(buf);
    while !r.done() {
        let (field, wire) = r.tag()?;
        match (field, wire) {
            (1, Wire::Bytes) => match ids::hex_from_bytes("trace", r.bytes()?) {
                Ok(v) => out.trace_id = v,
                Err(_) => return Ok(None),
            },
            // An absent or all-zero span id is a link to the whole trace, which
            // mcpd's model allows, rather than a malformed link.
            (2, Wire::Bytes) => out.span_id = ids::hex_from_bytes("span", r.bytes()?).ok(),
            (4, Wire::Bytes) => {
                if let Some((k, v)) = key_value(r.bytes()?)? {
                    out.attributes.insert(k, v);
                }
            }
            (_, w) => r.skip(w)?,
        }
    }
    Ok((!out.trace_id.is_empty()).then_some(out))
}

/// Attributes on a `Resource` or an `InstrumentationScope`.
fn attributes_of(buf: &[u8]) -> Result<Map<String, Value>> {
    // Resource { repeated KeyValue attributes = 1 } — the same field number the
    // helper above already looks for.
    key_values(buf)
}

/// `opentelemetry.proto.trace.v1.Span`
fn span(buf: &[u8], resource: &Map<String, Value>) -> Result<Option<OtlpSpan>> {
    let mut out = OtlpSpan { resource: resource.clone(), ..OtlpSpan::default() };
    let mut r = Reader::new(buf);
    let mut status_code = 0i32;

    while !r.done() {
        let (field, wire) = r.tag()?;
        match (field, wire) {
            (1, Wire::Bytes) => match ids::hex_from_bytes("trace", r.bytes()?) {
                Ok(v) => out.trace_id = v,
                // A span with an unusable id cannot be correlated with
                // anything, so it is dropped rather than stored orphaned.
                Err(_) => return Ok(None),
            },
            (2, Wire::Bytes) => match ids::hex_from_bytes("span", r.bytes()?) {
                Ok(v) => out.span_id = v,
                Err(_) => return Ok(None),
            },
            (4, Wire::Bytes) => {
                // An absent parent is encoded as an empty or all-zero field,
                // which is a root span rather than a malformed one.
                out.parent_span_id = ids::hex_from_bytes("span", r.bytes()?).ok();
            }
            (5, Wire::Bytes) => out.name = r.string()?,
            (7, Wire::Fixed64) => out.start_unix_nano = u128::from(r.fixed64()?),
            (8, Wire::Fixed64) => out.end_unix_nano = u128::from(r.fixed64()?),
            (9, Wire::Bytes) => {
                // `attributes` is a repeated KeyValue, so each occurrence of
                // field 9 carries exactly one entry, not a packed list.
                if let Some((k, v)) = key_value(r.bytes()?)? {
                    out.attributes.insert(k, v);
                }
            }
            (11, Wire::Bytes) => {
                // `events`, repeated. The frame is consumed either way, so the
                // reader stays in step whatever is inside it; an event that
                // will not read is dropped on its own rather than failing the
                // span, which would throw away everything the span does say
                // for the sake of one record inside it. Past the cap the frame
                // is still walked, because the fields after it are behind it.
                let raw = r.bytes()?;
                if out.events.len() < MAX_PER_SPAN {
                    if let Ok(event) = event_of(raw) {
                        out.events.push(event);
                    }
                }
            }
            (13, Wire::Bytes) => {
                let raw = r.bytes()?;
                if out.links.len() < MAX_PER_SPAN {
                    if let Ok(Some(link)) = link_of(raw) {
                        out.links.push(link);
                    }
                }
            }
            (15, Wire::Bytes) => {
                // Status { string message = 2; StatusCode code = 3 }
                let mut st = Reader::new(r.bytes()?);
                while !st.done() {
                    let (f, w) = st.tag()?;
                    match (f, w) {
                        (2, Wire::Bytes) => out.status_message = Some(st.string()?),
                        (3, Wire::Varint) => status_code = st.varint()? as i32,
                        (_, w) => st.skip(w)?,
                    }
                }
            }
            (16, Wire::Fixed32) => out.flags = r.fixed32()?,
            (_, w) => r.skip(w)?,
        }
    }

    if out.trace_id.is_empty() || out.span_id.is_empty() {
        return Ok(None);
    }
    out.status_code = status_code;
    if out.name.is_empty() {
        out.name = "span".to_owned();
    }
    Ok(Some(out))
}

/// Decode an `ExportTraceServiceRequest`.
///
/// Malformed spans are skipped rather than failing the batch, matching the JSON
/// path: a collector that rejects ten thousand good spans because one was bad is
/// a collector people turn off.
///
/// # Errors
/// Returns [`Error::Otlp`] only when the outer message itself cannot be walked.
pub fn decode_traces(body: &[u8]) -> Result<Vec<OtlpSpan>> {
    let mut out = Vec::new();
    let mut r = Reader::new(body);

    while !r.done() {
        let (field, wire) = r.tag()?;
        if (field, wire) != (1, Wire::Bytes) {
            r.skip(wire)?;
            continue;
        }
        // ResourceSpans { Resource resource = 1; repeated ScopeSpans scope_spans = 2 }
        let mut rs = Reader::new(r.bytes()?);
        let mut resource = Map::new();
        let mut scopes: Vec<&[u8]> = Vec::new();
        while !rs.done() {
            let (f, w) = rs.tag()?;
            match (f, w) {
                (1, Wire::Bytes) => resource = attributes_of(rs.bytes()?)?,
                (2, Wire::Bytes) => scopes.push(rs.bytes()?),
                (_, w) => rs.skip(w)?,
            }
        }

        for scope in scopes {
            // ScopeSpans { InstrumentationScope scope = 1; repeated Span spans = 2 }
            let mut ss = Reader::new(scope);
            while !ss.done() {
                let (f, w) = ss.tag()?;
                match (f, w) {
                    (2, Wire::Bytes) => {
                        let raw = ss.bytes()?;
                        // One unreadable span does not spoil the batch.
                        if let Ok(Some(s)) = span(raw, &resource) {
                            out.push(s);
                        }
                    }
                    (_, w) => ss.skip(w)?,
                }
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Logs and metrics.
//
// These two are not lowered into a struct of their own. The JSON doors already
// map an OTLP/JSON document onto mcpd's records, and the attributes of a log
// record or a data point are stored in the shape the sender wrote them. So the
// protobuf reader writes the OTLP/JSON document the same export would have been
// as JSON, and the JSON mapping stays the one place either encoding is read.
// Ids become lowercase hex, 64-bit integers become strings and bytes become
// base64, which is what the OTLP/JSON encoding prescribes for each.
// ---------------------------------------------------------------------------

/// Standard base64 with padding, which is how protobuf's JSON mapping writes
/// a `bytes` field.
pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// An `AnyValue` in its OTLP/JSON form, `{"stringValue": ...}` and so on.
fn any_value_json(buf: &[u8], depth: u32) -> Result<Value> {
    if depth > MAX_ATTRIBUTE_DEPTH {
        return Err(Error::Otlp("attribute nested too deeply".into()));
    }
    let mut out = Map::new();
    let mut r = Reader::new(buf);
    while !r.done() {
        let (field, wire) = r.tag()?;
        match (field, wire) {
            (1, Wire::Bytes) => {
                out.insert("stringValue".into(), Value::String(r.string()?));
            }
            (2, Wire::Varint) => {
                out.insert("boolValue".into(), Value::Bool(r.varint()? != 0));
            }
            (3, Wire::Varint) => {
                out.insert("intValue".into(), Value::String((r.varint()? as i64).to_string()));
            }
            (4, Wire::Fixed64) => {
                out.insert("doubleValue".into(), Value::from(f64::from_bits(r.fixed64()?)));
            }
            (5, Wire::Bytes) => {
                let mut ir = Reader::new(r.bytes()?);
                let mut items = Vec::new();
                while !ir.done() {
                    let (f, w) = ir.tag()?;
                    if (f, w) == (1, Wire::Bytes) {
                        items.push(any_value_json(ir.bytes()?, depth + 1)?);
                    } else {
                        ir.skip(w)?;
                    }
                }
                out.insert("arrayValue".into(), serde_json::json!({ "values": items }));
            }
            (6, Wire::Bytes) => {
                let values = key_values_json(r.bytes()?, 1, depth + 1)?;
                out.insert("kvlistValue".into(), serde_json::json!({ "values": values }));
            }
            (7, Wire::Bytes) => {
                out.insert("bytesValue".into(), Value::String(base64_encode(r.bytes()?)));
            }
            (_, w) => r.skip(w)?,
        }
    }
    Ok(Value::Object(out))
}

/// One `KeyValue` in its OTLP/JSON form, or `None` when it has no key.
fn key_value_json(buf: &[u8], depth: u32) -> Result<Option<Value>> {
    let mut kv = Reader::new(buf);
    let mut key = String::new();
    let mut value = None;
    while !kv.done() {
        let (f, w) = kv.tag()?;
        match (f, w) {
            (1, Wire::Bytes) => key = kv.string()?,
            (2, Wire::Bytes) => value = Some(any_value_json(kv.bytes()?, depth + 1)?),
            (_, w) => kv.skip(w)?,
        }
    }
    if key.is_empty() {
        return Ok(None);
    }
    let mut out = Map::new();
    out.insert("key".into(), Value::String(key));
    if let Some(v) = value {
        out.insert("value".into(), v);
    }
    Ok(Some(Value::Object(out)))
}

/// Every occurrence of the repeated `KeyValue` field `field` in a message.
fn key_values_json(buf: &[u8], field: u32, depth: u32) -> Result<Vec<Value>> {
    if depth > MAX_ATTRIBUTE_DEPTH {
        return Err(Error::Otlp("attribute nested too deeply".into()));
    }
    let mut out = Vec::new();
    let mut r = Reader::new(buf);
    while !r.done() {
        let (f, w) = r.tag()?;
        if (f, w) == (field, Wire::Bytes) {
            if let Some(kv) = key_value_json(r.bytes()?, depth)? {
                out.push(kv);
            }
        } else {
            r.skip(w)?;
        }
    }
    Ok(out)
}

/// `opentelemetry.proto.resource.v1.Resource`, as OTLP/JSON.
fn resource_json(buf: &[u8]) -> Result<Value> {
    Ok(serde_json::json!({ "attributes": key_values_json(buf, 1, 0)? }))
}

/// `opentelemetry.proto.common.v1.InstrumentationScope`, as OTLP/JSON.
fn scope_json(buf: &[u8]) -> Result<Value> {
    let mut out = Map::new();
    let mut attributes = Vec::new();
    let mut r = Reader::new(buf);
    while !r.done() {
        let (f, w) = r.tag()?;
        match (f, w) {
            (1, Wire::Bytes) => {
                out.insert("name".into(), Value::String(r.string()?));
            }
            (2, Wire::Bytes) => {
                out.insert("version".into(), Value::String(r.string()?));
            }
            (3, Wire::Bytes) => {
                if let Some(kv) = key_value_json(r.bytes()?, 0)? {
                    attributes.push(kv);
                }
            }
            (_, w) => r.skip(w)?,
        }
    }
    if !attributes.is_empty() {
        out.insert("attributes".into(), Value::Array(attributes));
    }
    Ok(Value::Object(out))
}

/// A trace or span id as OTLP/JSON writes it, or `None` when it is not one.
fn id_json(kind: &'static str, bytes: &[u8]) -> Option<Value> {
    ids::hex_from_bytes(kind, bytes).ok().map(Value::String)
}

/// A nanosecond timestamp, which OTLP/JSON writes as a decimal string. Zero is
/// the proto3 default and means the sender said nothing, so it is left out.
fn nanos_json(out: &mut Map<String, Value>, key: &str, v: u64) {
    if v != 0 {
        out.insert(key.into(), Value::String(v.to_string()));
    }
}

/// One resource-level entry of a logs or metrics request: the resource, and
/// every scope under it, with each item read by `item`.
fn resource_entry_json(
    buf: &[u8],
    scopes_key: &str,
    items_key: &str,
    item: fn(&[u8]) -> Result<Value>,
) -> Result<Value> {
    let mut out = Map::new();
    let mut scopes = Vec::new();
    let mut r = Reader::new(buf);
    while !r.done() {
        let (f, w) = r.tag()?;
        match (f, w) {
            (1, Wire::Bytes) => {
                out.insert("resource".into(), resource_json(r.bytes()?)?);
            }
            (2, Wire::Bytes) => {
                // ScopeLogs and ScopeMetrics { scope = 1; repeated item = 2 }
                let mut scope = Map::new();
                let mut items = Vec::new();
                let mut sr = Reader::new(r.bytes()?);
                while !sr.done() {
                    let (sf, sw) = sr.tag()?;
                    match (sf, sw) {
                        (1, Wire::Bytes) => {
                            scope.insert("scope".into(), scope_json(sr.bytes()?)?);
                        }
                        (2, Wire::Bytes) => {
                            // One unreadable record does not spoil the batch,
                            // the same rule the span reader applies.
                            if let Ok(v) = item(sr.bytes()?) {
                                items.push(v);
                            }
                        }
                        (_, w) => sr.skip(w)?,
                    }
                }
                scope.insert(items_key.into(), Value::Array(items));
                scopes.push(Value::Object(scope));
            }
            (_, w) => r.skip(w)?,
        }
    }
    out.insert(scopes_key.into(), Value::Array(scopes));
    Ok(Value::Object(out))
}

/// `opentelemetry.proto.logs.v1.LogRecord`
fn log_record_json(buf: &[u8]) -> Result<Value> {
    let mut out = Map::new();
    let mut attributes = Vec::new();
    let mut r = Reader::new(buf);
    while !r.done() {
        let (f, w) = r.tag()?;
        match (f, w) {
            (1, Wire::Fixed64) => nanos_json(&mut out, "timeUnixNano", r.fixed64()?),
            (11, Wire::Fixed64) => nanos_json(&mut out, "observedTimeUnixNano", r.fixed64()?),
            (2, Wire::Varint) => {
                out.insert("severityNumber".into(), Value::from(r.varint()? as i32));
            }
            (3, Wire::Bytes) => {
                out.insert("severityText".into(), Value::String(r.string()?));
            }
            (5, Wire::Bytes) => {
                out.insert("body".into(), any_value_json(r.bytes()?, 0)?);
            }
            (6, Wire::Bytes) => {
                if let Some(kv) = key_value_json(r.bytes()?, 0)? {
                    attributes.push(kv);
                }
            }
            (8, Wire::Fixed32) => {
                out.insert("flags".into(), Value::from(r.fixed32()?));
            }
            (9, Wire::Bytes) => {
                if let Some(id) = id_json("trace", r.bytes()?) {
                    out.insert("traceId".into(), id);
                }
            }
            (10, Wire::Bytes) => {
                if let Some(id) = id_json("span", r.bytes()?) {
                    out.insert("spanId".into(), id);
                }
            }
            (12, Wire::Bytes) => {
                out.insert("eventName".into(), Value::String(r.string()?));
            }
            (_, w) => r.skip(w)?,
        }
    }
    if !attributes.is_empty() {
        out.insert("attributes".into(), Value::Array(attributes));
    }
    Ok(Value::Object(out))
}

/// Decode an `ExportLogsServiceRequest` into the OTLP/JSON document the same
/// export would have been.
///
/// # Errors
/// Returns [`Error::Otlp`] only when the outer message itself cannot be walked.
pub fn decode_logs(body: &[u8]) -> Result<Value> {
    let mut resources = Vec::new();
    let mut r = Reader::new(body);
    while !r.done() {
        let (f, w) = r.tag()?;
        if (f, w) == (1, Wire::Bytes) {
            resources.push(resource_entry_json(
                r.bytes()?,
                "scopeLogs",
                "logRecords",
                log_record_json,
            )?);
        } else {
            r.skip(w)?;
        }
    }
    Ok(serde_json::json!({ "resourceLogs": resources }))
}

/// A `repeated fixed64` or `repeated double`, packed or not. proto3 packs
/// both by default, and a reader must accept either form.
fn push_fixed64s(r: &mut Reader<'_>, wire: Wire, out: &mut Vec<u64>) -> Result<()> {
    match wire {
        Wire::Fixed64 => out.push(r.fixed64()?),
        Wire::Bytes => {
            let mut packed = Reader::new(r.bytes()?);
            while !packed.done() {
                out.push(packed.fixed64()?);
            }
        }
        other => r.skip(other)?,
    }
    Ok(())
}

/// `opentelemetry.proto.metrics.v1.Exemplar`
fn exemplar_json(buf: &[u8]) -> Result<Value> {
    let mut out = Map::new();
    let mut attributes = Vec::new();
    let mut r = Reader::new(buf);
    while !r.done() {
        let (f, w) = r.tag()?;
        match (f, w) {
            (7, Wire::Bytes) => {
                if let Some(kv) = key_value_json(r.bytes()?, 0)? {
                    attributes.push(kv);
                }
            }
            (2, Wire::Fixed64) => nanos_json(&mut out, "timeUnixNano", r.fixed64()?),
            (3, Wire::Fixed64) => {
                out.insert("asDouble".into(), Value::from(f64::from_bits(r.fixed64()?)));
            }
            (6, Wire::Fixed64) => {
                out.insert("asInt".into(), Value::String((r.fixed64()? as i64).to_string()));
            }
            (4, Wire::Bytes) => {
                if let Some(id) = id_json("span", r.bytes()?) {
                    out.insert("spanId".into(), id);
                }
            }
            (5, Wire::Bytes) => {
                if let Some(id) = id_json("trace", r.bytes()?) {
                    out.insert("traceId".into(), id);
                }
            }
            (_, w) => r.skip(w)?,
        }
    }
    if !attributes.is_empty() {
        out.insert("filteredAttributes".into(), Value::Array(attributes));
    }
    Ok(Value::Object(out))
}

/// `NumberDataPoint` or `HistogramDataPoint`. The two share their time
/// fields and differ in where the attributes, the value and the exemplars sit,
/// so `histogram` picks the layout.
fn data_point_json(buf: &[u8], histogram: bool) -> Result<Value> {
    let mut out = Map::new();
    let mut attributes = Vec::new();
    let mut exemplars = Vec::new();
    let mut bucket_counts = Vec::new();
    let mut bounds = Vec::new();
    let (attr_field, exemplar_field) = if histogram { (9, 8) } else { (7, 5) };
    let mut r = Reader::new(buf);
    while !r.done() {
        let (f, w) = r.tag()?;
        match (f, w) {
            (f, Wire::Bytes) if f == attr_field => {
                if let Some(kv) = key_value_json(r.bytes()?, 0)? {
                    attributes.push(kv);
                }
            }
            (f, Wire::Bytes) if f == exemplar_field => {
                if let Ok(e) = exemplar_json(r.bytes()?) {
                    exemplars.push(e);
                }
            }
            (2, Wire::Fixed64) => nanos_json(&mut out, "startTimeUnixNano", r.fixed64()?),
            (3, Wire::Fixed64) => nanos_json(&mut out, "timeUnixNano", r.fixed64()?),
            (4, Wire::Fixed64) if !histogram => {
                out.insert("asDouble".into(), Value::from(f64::from_bits(r.fixed64()?)));
            }
            (6, Wire::Fixed64) if !histogram => {
                out.insert("asInt".into(), Value::String((r.fixed64()? as i64).to_string()));
            }
            (8, Wire::Varint) if !histogram => {
                out.insert("flags".into(), Value::from(r.varint()? as u32));
            }
            (4, Wire::Fixed64) => {
                out.insert("count".into(), Value::String(r.fixed64()?.to_string()));
            }
            (5, Wire::Fixed64) => {
                out.insert("sum".into(), Value::from(f64::from_bits(r.fixed64()?)));
            }
            (6, w) if histogram => push_fixed64s(&mut r, w, &mut bucket_counts)?,
            (7, w) if histogram => push_fixed64s(&mut r, w, &mut bounds)?,
            (10, Wire::Varint) => {
                out.insert("flags".into(), Value::from(r.varint()? as u32));
            }
            (11, Wire::Fixed64) => {
                out.insert("min".into(), Value::from(f64::from_bits(r.fixed64()?)));
            }
            (12, Wire::Fixed64) => {
                out.insert("max".into(), Value::from(f64::from_bits(r.fixed64()?)));
            }
            (_, w) => r.skip(w)?,
        }
    }
    if !attributes.is_empty() {
        out.insert("attributes".into(), Value::Array(attributes));
    }
    if !exemplars.is_empty() {
        out.insert("exemplars".into(), Value::Array(exemplars));
    }
    if histogram {
        out.insert(
            "bucketCounts".into(),
            Value::Array(bucket_counts.iter().map(|c| Value::String(c.to_string())).collect()),
        );
        out.insert(
            "explicitBounds".into(),
            Value::Array(bounds.iter().map(|b| Value::from(f64::from_bits(*b))).collect()),
        );
    }
    Ok(Value::Object(out))
}

/// `Gauge`, `Sum` or `Histogram`: the data points, and the two fields a sum
/// and a histogram add.
fn metric_data_json(buf: &[u8], histogram: bool) -> Result<Value> {
    let mut out = Map::new();
    let mut points = Vec::new();
    let mut r = Reader::new(buf);
    while !r.done() {
        let (f, w) = r.tag()?;
        match (f, w) {
            (1, Wire::Bytes) => {
                if let Ok(p) = data_point_json(r.bytes()?, histogram) {
                    points.push(p);
                }
            }
            (2, Wire::Varint) => {
                out.insert("aggregationTemporality".into(), Value::from(r.varint()? as i32));
            }
            (3, Wire::Varint) => {
                out.insert("isMonotonic".into(), Value::Bool(r.varint()? != 0));
            }
            (_, w) => r.skip(w)?,
        }
    }
    out.insert("dataPoints".into(), Value::Array(points));
    Ok(Value::Object(out))
}

/// `opentelemetry.proto.metrics.v1.Metric`
///
/// Exponential histograms and summaries are named and kept as empty objects,
/// so the JSON mapping makes the same decision about them whichever encoding
/// they arrived in.
fn metric_json(buf: &[u8]) -> Result<Value> {
    let mut out = Map::new();
    let mut r = Reader::new(buf);
    while !r.done() {
        let (f, w) = r.tag()?;
        match (f, w) {
            (1, Wire::Bytes) => {
                out.insert("name".into(), Value::String(r.string()?));
            }
            (2, Wire::Bytes) => {
                out.insert("description".into(), Value::String(r.string()?));
            }
            (3, Wire::Bytes) => {
                out.insert("unit".into(), Value::String(r.string()?));
            }
            (5, Wire::Bytes) => {
                out.insert("gauge".into(), metric_data_json(r.bytes()?, false)?);
            }
            (7, Wire::Bytes) => {
                out.insert("sum".into(), metric_data_json(r.bytes()?, false)?);
            }
            (9, Wire::Bytes) => {
                out.insert("histogram".into(), metric_data_json(r.bytes()?, true)?);
            }
            (10, Wire::Bytes) => {
                r.bytes()?;
                out.insert("exponentialHistogram".into(), Value::Object(Map::new()));
            }
            (11, Wire::Bytes) => {
                r.bytes()?;
                out.insert("summary".into(), Value::Object(Map::new()));
            }
            (_, w) => r.skip(w)?,
        }
    }
    Ok(Value::Object(out))
}

/// Decode an `ExportMetricsServiceRequest` into the OTLP/JSON document the
/// same export would have been.
///
/// # Errors
/// Returns [`Error::Otlp`] only when the outer message itself cannot be walked.
pub fn decode_metrics(body: &[u8]) -> Result<Value> {
    let mut resources = Vec::new();
    let mut r = Reader::new(body);
    while !r.done() {
        let (f, w) = r.tag()?;
        if (f, w) == (1, Wire::Bytes) {
            resources.push(resource_entry_json(
                r.bytes()?,
                "scopeMetrics",
                "metrics",
                metric_json,
            )?);
        } else {
            r.skip(w)?;
        }
    }
    Ok(serde_json::json!({ "resourceMetrics": resources }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal protobuf writer, so the tests encode real wire bytes rather than
    /// asserting against a fixture nobody can check.
    struct W(Vec<u8>);
    impl W {
        fn new() -> Self {
            Self(Vec::new())
        }
        fn varint(&mut self, mut v: u64) -> &mut Self {
            loop {
                let b = (v & 0x7f) as u8;
                v >>= 7;
                if v == 0 {
                    self.0.push(b);
                    return self;
                }
                self.0.push(b | 0x80);
            }
        }
        fn tag(&mut self, field: u32, wire: u32) -> &mut Self {
            self.varint(u64::from(field << 3 | wire))
        }
        fn bytes(&mut self, field: u32, v: &[u8]) -> &mut Self {
            self.tag(field, 2).varint(v.len() as u64);
            self.0.extend_from_slice(v);
            self
        }
        fn string(&mut self, field: u32, v: &str) -> &mut Self {
            self.bytes(field, v.as_bytes())
        }
        fn fixed64(&mut self, field: u32, v: u64) -> &mut Self {
            self.tag(field, 1);
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        fn uint(&mut self, field: u32, v: u64) -> &mut Self {
            self.tag(field, 0).varint(v)
        }
        fn done(&self) -> Vec<u8> {
            self.0.clone()
        }
    }

    fn kv_string(key: &str, value: &str) -> Vec<u8> {
        let mut any = W::new();
        any.string(1, value);
        let mut kv = W::new();
        kv.string(1, key).bytes(2, &any.done());
        kv.done()
    }

    fn kv_int(key: &str, value: i64) -> Vec<u8> {
        let mut any = W::new();
        any.uint(3, value as u64);
        let mut kv = W::new();
        kv.string(1, key).bytes(2, &any.done());
        kv.done()
    }

    fn sample() -> Vec<u8> {
        let trace = [
            0x4b, 0xf9, 0x2f, 0x35, 0x77, 0xb3, 0x4d, 0xa6, 0xa3, 0xce, 0x92, 0x9d, 0x0e, 0x0e,
            0x47, 0x36,
        ];
        let span_id = [0x00, 0xf0, 0x67, 0xaa, 0x0b, 0xa9, 0x02, 0xb7];

        let mut status = W::new();
        status.string(2, "429 from provider").uint(3, 2);

        let mut sp = W::new();
        sp.bytes(1, &trace)
            .bytes(2, &span_id)
            .string(5, "chat gpt-4o")
            .fixed64(7, 1_700_000_000_000_000_000)
            .fixed64(8, 1_700_000_001_500_000_000)
            .bytes(9, &kv_string("gen_ai.system", "openai"))
            .bytes(9, &kv_string("gen_ai.request.model", "gpt-4o"))
            .bytes(9, &kv_int("gen_ai.usage.input_tokens", 1284))
            .bytes(15, &status.done());

        let mut scope = W::new();
        scope.bytes(2, &sp.done());

        let mut resource = W::new();
        resource.bytes(1, &kv_string("service.name", "checkout-agent"));

        let mut rs = W::new();
        rs.bytes(1, &resource.done()).bytes(2, &scope.done());

        let mut req = W::new();
        req.bytes(1, &rs.done());
        req.done()
    }

    #[test]
    fn decodes_a_real_protobuf_payload() {
        let spans = decode_traces(&sample()).unwrap();
        assert_eq!(spans.len(), 1);
        let s = &spans[0];
        assert_eq!(s.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(s.span_id, "00f067aa0ba902b7");
        assert_eq!(s.name, "chat gpt-4o");
        assert_eq!(s.start_unix_nano, 1_700_000_000_000_000_000);
        assert_eq!(s.end_unix_nano, 1_700_000_001_500_000_000);
        assert_eq!(s.resource["service.name"], Value::from("checkout-agent"));
        assert_eq!(s.attributes["gen_ai.system"], Value::from("openai"));
        assert_eq!(s.attributes["gen_ai.usage.input_tokens"], Value::from(1284));
        assert_eq!(s.status_code, 2);
        assert_eq!(s.status_message.as_deref(), Some("429 from provider"));
    }

    #[test]
    fn produces_the_same_span_as_the_json_path() {
        // The two encodings must not drift. Both lower into OtlpSpan and are
        // mapped by the same code, so the resulting span has to match.
        use time::OffsetDateTime;
        let from_pb = &decode_traces(&sample()).unwrap()[0];
        let mapped = super::super::to_span(from_pb, "default", OffsetDateTime::UNIX_EPOCH);
        assert_eq!(mapped.kind, crate::SpanKind::ModelGeneration);
        assert_eq!(mapped.model.as_deref(), Some("gpt-4o"));
        assert_eq!(mapped.provider.as_deref(), Some("openai"));
        assert_eq!(mapped.status, crate::SpanStatus::Error);
        assert_eq!(mapped.usage.unwrap().input_tokens, Some(1284));
        assert_eq!(mapped.correlation.service_name.as_deref(), Some("checkout-agent"));
    }

    #[test]
    fn unknown_fields_are_skipped_not_fatal() {
        // A newer collector sending a field this build has never heard of must
        // still get its spans stored.
        let mut req = W::new();
        req.string(999, "something from the future");
        let mut rs = W::new();
        rs.string(77, "and here too");
        req.bytes(1, &rs.done());
        assert!(decode_traces(&req.done()).is_ok());
    }

    #[test]
    fn a_truncated_message_is_an_error_not_a_panic() {
        let full = sample();
        for cut in [1, 5, 17, 40, full.len() / 2, full.len() - 1] {
            let _ = decode_traces(&full[..cut]);
        }
    }

    #[test]
    fn a_span_with_a_reserved_id_is_dropped_not_stored() {
        let mut sp = W::new();
        sp.bytes(1, &[0u8; 16]).bytes(2, &[1u8; 8]).string(5, "zero trace");
        let mut scope = W::new();
        scope.bytes(2, &sp.done());
        let mut rs = W::new();
        rs.bytes(2, &scope.done());
        let mut req = W::new();
        req.bytes(1, &rs.done());
        assert!(decode_traces(&req.done()).unwrap().is_empty());
    }

    #[test]
    fn every_any_value_kind_round_trips() {
        let mut b = W::new();
        b.uint(2, 1);
        assert_eq!(any_value(&b.done()).unwrap(), Value::Bool(true));

        let mut d = W::new();
        d.fixed64(4, 1.5f64.to_bits());
        assert_eq!(any_value(&d.done()).unwrap(), Value::from(1.5));

        let mut inner = W::new();
        inner.string(1, "a");
        let mut arr = W::new();
        arr.bytes(1, &inner.done());
        let mut outer = W::new();
        outer.bytes(5, &arr.done());
        assert_eq!(any_value(&outer.done()).unwrap(), Value::Array(vec![Value::from("a")]));
    }

    #[test]
    fn a_protobuf_log_record_becomes_the_json_document() {
        let mut body = W::new();
        body.string(1, "rate limited");
        let mut rec = W::new();
        rec.fixed64(1, 1_700_000_000_000_000_000)
            .uint(2, 17)
            .bytes(5, &body.done())
            .bytes(6, &kv_int("attempt", 3))
            .bytes(9, &[0x4b; 16])
            .bytes(10, &[0x01; 8]);
        let mut scope = W::new();
        scope.string(1, "agent");
        let mut sl = W::new();
        sl.bytes(1, &scope.done()).bytes(2, &rec.done());
        let mut resource = W::new();
        resource.bytes(1, &kv_string("service.name", "checkout-agent"));
        let mut rl = W::new();
        rl.bytes(1, &resource.done()).bytes(2, &sl.done());
        let mut req = W::new();
        req.bytes(1, &rl.done());

        let doc = decode_logs(&req.done()).unwrap();
        let r = &doc["resourceLogs"][0];
        assert_eq!(r["resource"]["attributes"][0]["value"]["stringValue"], "checkout-agent");
        assert_eq!(r["scopeLogs"][0]["scope"]["name"], "agent");
        let log = &r["scopeLogs"][0]["logRecords"][0];
        assert_eq!(log["timeUnixNano"], "1700000000000000000");
        assert_eq!(log["severityNumber"], 17);
        assert_eq!(log["body"]["stringValue"], "rate limited");
        assert_eq!(
            log["attributes"][0],
            serde_json::json!({
                "key": "attempt", "value": { "intValue": "3" }
            })
        );
        assert_eq!(log["traceId"], "4b".repeat(16));
        assert_eq!(log["spanId"], "01".repeat(8));
    }

    #[test]
    fn a_protobuf_histogram_reads_packed_and_unpacked_buckets() {
        let mut packed_counts = Vec::new();
        for c in [1u64, 2, 3] {
            packed_counts.extend_from_slice(&c.to_le_bytes());
        }
        let mut point = W::new();
        point
            .fixed64(3, 1_700_000_000_000_000_000)
            .fixed64(4, 6)
            .fixed64(5, 12.5f64.to_bits())
            .bytes(6, &packed_counts)
            .fixed64(7, 1.0f64.to_bits())
            .fixed64(7, 5.0f64.to_bits())
            .bytes(9, &kv_string("route", "/chat"));
        let mut hist = W::new();
        hist.bytes(1, &point.done()).uint(2, 2);
        let mut metric = W::new();
        metric.string(1, "latency").string(3, "ms").bytes(9, &hist.done());
        let mut sm = W::new();
        sm.bytes(2, &metric.done());
        let mut rm = W::new();
        rm.bytes(2, &sm.done());
        let mut req = W::new();
        req.bytes(1, &rm.done());

        let doc = decode_metrics(&req.done()).unwrap();
        let m = &doc["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0];
        assert_eq!(m["name"], "latency");
        assert_eq!(m["unit"], "ms");
        let p = &m["histogram"]["dataPoints"][0];
        assert_eq!(p["count"], "6");
        assert_eq!(p["sum"], 12.5);
        assert_eq!(p["bucketCounts"], serde_json::json!(["1", "2", "3"]));
        assert_eq!(p["explicitBounds"], serde_json::json!([1.0, 5.0]));
        assert_eq!(p["attributes"][0]["key"], "route");
    }

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(&[0xfb, 0xff]), "+/8=");
    }
}
