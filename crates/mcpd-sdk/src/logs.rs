//! The logging signal: how an error reaches a field key, and how a level
//! relates to an OpenTelemetry severity number.
//!
//! Rust is the one SDK whose log fields are already a `serde_json::Value`, so
//! nothing unserialisable can reach the queue and no batch can be lost to one.
//! What it needs from contract section 4.1 is the other half: an error that
//! goes under a key in the same shape every other language sends.

use mcpd_core::LogLevel;
use serde_json::{json, Value};

/// An error as `type` and `message`, the shape every mcpd SDK sends.
///
/// ```
/// let io = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
/// let fields = serde_json::json!({ "error": mcpd_sdk::error_fields(&io) });
/// assert_eq!(fields["error"]["message"], "no such file");
/// ```
///
/// No stack: a Rust error value does not carry one, and the backtrace of
/// whichever task happened to be logging is not where the error came from.
/// The type is the concrete type's name when one is known. A `&dyn Error` is
/// reported as `Error`, because that is all the type system still holds.
pub fn error_fields<E>(error: &E) -> Value
where
    E: std::error::Error + ?Sized,
{
    let message = error.to_string();
    let mut fields = json!({
        "type": short_type_name(std::any::type_name::<E>()),
        "message": if message.is_empty() { "error".to_owned() } else { message },
    });
    // The chain is what an error in Rust has instead of a stack: each layer
    // says what it was doing when the one under it failed.
    let mut causes = Vec::new();
    let mut source = error.source();
    while let Some(current) = source {
        causes.push(Value::String(current.to_string()));
        source = current.source();
    }
    if !causes.is_empty() {
        fields["causes"] = Value::Array(causes);
    }
    fields
}

/// The OpenTelemetry severity number the spec gives a level.
#[must_use]
pub const fn severity_number(level: LogLevel) -> u8 {
    level.severity()
}

/// The level an OpenTelemetry severity number lands on.
///
/// By range, the way the daemon reads an inbound OTLP `LogRecord`: 1 to 4 is
/// trace, 5 to 8 debug, 9 to 12 info, 13 to 16 warn, 17 to 20 error, and 21 and
/// above fatal. A number outside the range is clamped rather than refused,
/// because a sender that invents a 40 still meant something serious.
#[must_use]
pub fn log_level_from_severity(severity: i64) -> LogLevel {
    let mut found = LogLevel::ALL[0];
    for candidate in LogLevel::ALL {
        if severity >= i64::from(candidate.severity()) {
            found = *candidate;
        }
    }
    found
}

/// Read a level by name, case-insensitively. Anything else is `None`.
#[must_use]
pub fn parse_log_level(name: &str) -> Option<LogLevel> {
    name.trim().to_ascii_lowercase().parse().ok()
}

/// Whether `level` is at or above the floor.
#[must_use]
pub const fn meets_level(level: LogLevel, floor: LogLevel) -> bool {
    level.severity() >= floor.severity()
}

/// The last segment of a Rust type path, with the trait-object marker gone.
fn short_type_name(name: &str) -> String {
    let trimmed = name.trim_start_matches("dyn ");
    let head = trimmed.split('<').next().unwrap_or(trimmed);
    head.rsplit("::").next().unwrap_or(head).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Outer;

    impl std::fmt::Display for Outer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("calling the model")
        }
    }

    impl std::error::Error for Outer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&Inner)
        }
    }

    #[derive(Debug)]
    struct Inner;

    impl std::fmt::Display for Inner {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("boom")
        }
    }

    impl std::error::Error for Inner {}

    #[test]
    fn an_error_carries_its_type_message_and_chain() {
        let fields = error_fields(&Outer);
        assert_eq!(fields["type"], "Outer");
        assert_eq!(fields["message"], "calling the model");
        assert_eq!(fields["causes"][0], "boom");
    }

    #[test]
    fn a_trait_object_still_answers() {
        let error: Box<dyn std::error::Error> = Box::new(Inner);
        let fields = error_fields(error.as_ref());
        assert_eq!(fields["message"], "boom");
        assert_eq!(fields["type"], "Error");
        assert!(fields.get("causes").is_none());
    }

    #[test]
    fn severity_maps_both_ways() {
        let expected = [1_u8, 5, 9, 13, 17, 21];
        for (level, want) in LogLevel::ALL.iter().zip(expected) {
            assert_eq!(severity_number(*level), want);
            assert_eq!(log_level_from_severity(i64::from(want)), *level);
        }
        for (from, to, level) in [
            (1, 4, LogLevel::Trace),
            (5, 8, LogLevel::Debug),
            (9, 12, LogLevel::Info),
            (13, 16, LogLevel::Warn),
            (17, 20, LogLevel::Error),
            (21, 24, LogLevel::Fatal),
        ] {
            for n in from..=to {
                assert_eq!(log_level_from_severity(n), level, "severity {n}");
            }
        }
        assert_eq!(log_level_from_severity(0), LogLevel::Trace);
        assert_eq!(log_level_from_severity(-3), LogLevel::Trace);
        assert_eq!(log_level_from_severity(40), LogLevel::Fatal);
    }

    #[test]
    fn a_name_outside_the_enum_is_none() {
        assert_eq!(parse_log_level("WARN"), Some(LogLevel::Warn));
        assert_eq!(parse_log_level(" error "), Some(LogLevel::Error));
        assert_eq!(parse_log_level("verbose"), None);
    }

    #[test]
    fn the_floor_is_a_comparison_on_severity() {
        assert!(meets_level(LogLevel::Error, LogLevel::Warn));
        assert!(meets_level(LogLevel::Warn, LogLevel::Warn));
        assert!(!meets_level(LogLevel::Info, LogLevel::Warn));
    }
}
