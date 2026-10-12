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
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

/// How a field's payload is encoded, from the low three bits of its tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wire {
    Varint,
    Fixed64,
    Bytes,
    Fixed32,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn done(&self) -> bool {
        self.pos >= self.buf.len()
    }

    fn varint(&mut self) -> Result<u64> {
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
    fn tag(&mut self) -> Result<(u32, Wire)> {
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

    fn bytes(&mut self) -> Result<&'a [u8]> {
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

    fn string(&mut self) -> Result<String> {
        Ok(String::from_utf8_lossy(self.bytes()?).into_owned())
    }

    fn fixed64(&mut self) -> Result<u64> {
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

    fn fixed32(&mut self) -> Result<u32> {
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
    fn skip(&mut self, wire: Wire) -> Result<()> {
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
}
