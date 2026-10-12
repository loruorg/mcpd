//! Trace and span identifiers.
//!
//! mcpd uses OpenTelemetry's shapes exactly: a trace id is 128 bits rendered as
//! 32 lowercase hex characters, a span id is 64 bits as 16. Keeping the wire
//! format identical is what lets any OpenTelemetry SDK point at mcpd without a
//! translation layer.

use crate::{Error, Result};
use rand::RngExt;

/// Characters in a hex-encoded trace id.
pub const TRACE_ID_LEN: usize = 32;
/// Characters in a hex-encoded span id.
pub const SPAN_ID_LEN: usize = 16;

/// Generate a new random trace id.
///
/// Never returns the all-zero id, which OpenTelemetry reserves to mean "no
/// trace"; on the vanishingly rare occasion the generator produces it, we draw
/// again rather than emit an id every downstream system will discard.
#[must_use]
pub fn new_trace_id() -> String {
    loop {
        let bytes: [u8; 16] = rand::rng().random();
        if bytes.iter().any(|b| *b != 0) {
            return hex::encode(bytes);
        }
    }
}

/// Generate a new random span id, with the same all-zero guarantee.
#[must_use]
pub fn new_span_id() -> String {
    loop {
        let bytes: [u8; 8] = rand::rng().random();
        if bytes.iter().any(|b| *b != 0) {
            return hex::encode(bytes);
        }
    }
}

fn check(kind: &'static str, value: &str, len: usize) -> Result<()> {
    if value.len() != len {
        return Err(Error::InvalidId { kind, value: value.to_owned(), reason: "wrong length" });
    }
    if !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::InvalidId { kind, value: value.to_owned(), reason: "not hexadecimal" });
    }
    if value.bytes().any(|b| b.is_ascii_uppercase()) {
        return Err(Error::InvalidId {
            kind,
            value: value.to_owned(),
            reason: "must be lowercase",
        });
    }
    if value.bytes().all(|b| b == b'0') {
        return Err(Error::InvalidId {
            kind,
            value: value.to_owned(),
            reason: "all-zero ids are reserved",
        });
    }
    Ok(())
}

/// Validate a trace id.
///
/// # Errors
/// Returns [`Error::InvalidId`] when the value is not 32 lowercase hex
/// characters, or is the reserved all-zero id.
pub fn validate_trace_id(value: &str) -> Result<()> {
    check("trace", value, TRACE_ID_LEN)
}

/// Validate a span id.
///
/// # Errors
/// Returns [`Error::InvalidId`] when the value is not 16 lowercase hex
/// characters, or is the reserved all-zero id.
pub fn validate_span_id(value: &str) -> Result<()> {
    check("span", value, SPAN_ID_LEN)
}

/// A parsed W3C `traceparent` header.
///
/// Accepting this is what lets a trace that began in someone else's service
/// continue into mcpd without losing its parentage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceParent {
    /// The trace this request belongs to.
    pub trace_id: String,
    /// The caller's span, which becomes the parent of whatever we create.
    pub parent_id: String,
    /// The trace-flags byte. Bit 0 is the sampled flag.
    pub flags: u8,
}

impl TraceParent {
    /// Whether the upstream caller sampled this trace.
    #[must_use]
    pub const fn sampled(&self) -> bool {
        self.flags & 0x01 != 0
    }

    /// Render back to the header form.
    #[must_use]
    pub fn to_header(&self) -> String {
        format!("00-{}-{}-{:02x}", self.trace_id, self.parent_id, self.flags)
    }
}

/// Parse a W3C `traceparent` header value.
///
/// Returns `None` for anything malformed, and specifically for version `ff`,
/// which the specification forbids. A bad header is never a reason to reject a
/// request: we simply start a new trace instead.
#[must_use]
pub fn parse_traceparent(value: &str) -> Option<TraceParent> {
    let mut parts = value.trim().split('-');
    let version = parts.next()?;
    let trace_id = parts.next()?;
    let parent_id = parts.next()?;
    let flags = parts.next()?;
    // Any further field is allowed by the spec for future versions, but version
    // 00 must have exactly four.
    if version == "00" && parts.next().is_some() {
        return None;
    }
    if version.len() != 2 || version == "ff" || !version.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    validate_trace_id(trace_id).ok()?;
    validate_span_id(parent_id).ok()?;
    if flags.len() != 2 {
        return None;
    }
    let flags = u8::from_str_radix(flags, 16).ok()?;
    Some(TraceParent { trace_id: trace_id.to_owned(), parent_id: parent_id.to_owned(), flags })
}

/// Convert raw OTLP id bytes to mcpd's hex form.
///
/// # Errors
/// Returns [`Error::InvalidId`] when the slice is not 16 bytes (trace) or
/// 8 bytes (span), or is entirely zero.
pub fn hex_from_bytes(kind: &'static str, bytes: &[u8]) -> Result<String> {
    let expected = if kind == "trace" { 16 } else { 8 };
    if bytes.len() != expected {
        return Err(Error::InvalidId {
            kind,
            value: hex::encode(bytes),
            reason: "wrong byte length",
        });
    }
    if bytes.iter().all(|b| *b == 0) {
        return Err(Error::InvalidId {
            kind,
            value: hex::encode(bytes),
            reason: "all-zero ids are reserved",
        });
    }
    Ok(hex::encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_are_the_right_shape() {
        for _ in 0..64 {
            let t = new_trace_id();
            let s = new_span_id();
            assert_eq!(t.len(), TRACE_ID_LEN);
            assert_eq!(s.len(), SPAN_ID_LEN);
            validate_trace_id(&t).unwrap();
            validate_span_id(&s).unwrap();
        }
    }

    #[test]
    fn generated_ids_are_distinct() {
        let a: std::collections::HashSet<_> = (0..500).map(|_| new_trace_id()).collect();
        assert_eq!(a.len(), 500, "trace ids collided");
    }

    #[test]
    fn rejects_the_reserved_all_zero_ids() {
        assert!(validate_trace_id(&"0".repeat(32)).is_err());
        assert!(validate_span_id(&"0".repeat(16)).is_err());
    }

    #[test]
    fn rejects_wrong_length_and_case_and_non_hex() {
        assert!(validate_trace_id("abc").is_err());
        assert!(validate_trace_id(&"A".repeat(32)).is_err());
        assert!(validate_trace_id(&"g".repeat(32)).is_err());
        assert!(validate_span_id(&"a".repeat(32)).is_err());
    }

    #[test]
    fn parses_a_valid_traceparent() {
        let h = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let p = parse_traceparent(h).expect("should parse");
        assert_eq!(p.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(p.parent_id, "00f067aa0ba902b7");
        assert!(p.sampled());
        assert_eq!(p.to_header(), h);
    }

    #[test]
    fn unsampled_flag_is_read_correctly() {
        let p =
            parse_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00").unwrap();
        assert!(!p.sampled());
    }

    #[test]
    fn rejects_version_ff_and_malformed_headers() {
        let bad = [
            "ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
            "",
            "garbage",
        ];
        for h in bad {
            assert!(parse_traceparent(h).is_none(), "should have rejected {h:?}");
        }
    }

    #[test]
    fn future_versions_may_carry_extra_fields() {
        // Forward compatibility: a version we do not know about is still usable
        // as long as the ids parse.
        let h = "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-something";
        assert!(parse_traceparent(h).is_some());
    }

    #[test]
    fn otlp_bytes_convert_and_validate() {
        let t = hex_from_bytes("trace", &[1u8; 16]).unwrap();
        assert_eq!(t.len(), 32);
        assert!(hex_from_bytes("trace", &[0u8; 16]).is_err());
        assert!(hex_from_bytes("span", &[1u8; 4]).is_err());
    }
}
