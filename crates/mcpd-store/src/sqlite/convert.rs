//! Conversion between SQLite rows and the model types.
//!
//! Kept apart from the queries so that a schema change touches one file, and so
//! the mapping can be tested without a database.

use super::*;

/// Nanoseconds since the Unix epoch, as stored.
pub(super) fn to_nanos(t: OffsetDateTime) -> i64 {
    // i64 nanoseconds covers 1678–2262. Clamping rather than wrapping keeps a
    // absurd timestamp from sorting before every real row.
    t.unix_timestamp_nanos().clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// Inverse of [`to_nanos`].
pub(super) fn from_nanos(n: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(n)).unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

/// The current instant, in stored form.
pub(super) fn now_nanos() -> i64 {
    to_nanos(OffsetDateTime::now_utc())
}

/// Serialise a JSON value for a TEXT column, dropping `null`.
pub(super) fn json_out(v: &Option<Value>) -> Option<String> {
    match v {
        Some(Value::Null) | None => None,
        Some(v) => serde_json::to_string(v).ok(),
    }
}

/// Parse a TEXT column back to JSON, tolerating malformed content.
///
/// A row that cannot be parsed yields `None` rather than failing the query: one
/// bad payload must not make a whole trace unreadable.
pub(super) fn json_in(s: Option<String>) -> Option<Value> {
    s.and_then(|s| serde_json::from_str(&s).ok())
}

/// Serialise any serialisable value for a TEXT column.
pub(super) fn enc<T: serde::Serialize>(v: &Option<T>) -> Option<String> {
    v.as_ref().and_then(|v| serde_json::to_string(v).ok())
}

/// Deserialise a TEXT column into a typed value, tolerating malformed content.
pub(super) fn dec<T: serde::de::DeserializeOwned>(s: Option<String>) -> Option<T> {
    s.and_then(|s| serde_json::from_str(&s).ok())
}

/// Tags are stored as a JSON array so a trace can be filtered on them with
/// `json_each` without a join table.
pub(super) fn tags_out(tags: &Option<Vec<String>>) -> Option<String> {
    tags.as_ref().filter(|t| !t.is_empty()).and_then(|t| serde_json::to_string(t).ok())
}

pub(super) fn tags_in(s: Option<String>) -> Option<Vec<String>> {
    dec::<Vec<String>>(s).filter(|v| !v.is_empty())
}

pub(super) fn kind_of(s: &str) -> SpanKind {
    SpanKind::from_str(s).unwrap_or(SpanKind::Generic)
}

pub(super) fn status_of(s: &str) -> SpanStatus {
    SpanStatus::from_str(s).unwrap_or(SpanStatus::Unset)
}

/// The columns [`row_to_span`] expects, in order.
pub(super) const SPAN_COLUMNS: &str = "\
    trace_id, span_id, parent_span_id, external_parent_span_id, name, kind, status, \
    start_time, end_time, duration_ns, is_root, is_event, entity_kind, entity_id, entity_name, \
    input, output, attributes, metadata, usage_json, cost_json, error_json, links_json, tags, \
    project, environment, service_name, service_version, session_id, thread_id, user_id, \
    resource_id, run_id, request_id, experiment_id, release, model, provider, received_at";

pub(super) fn row_to_span(r: &rusqlite::Row<'_>) -> rusqlite::Result<Span> {
    Ok(Span {
        trace_id: r.get(0)?,
        span_id: r.get(1)?,
        parent_span_id: r.get(2)?,
        external_parent_span_id: r.get(3)?,
        name: r.get(4)?,
        kind: kind_of(&r.get::<_, String>(5)?),
        status: status_of(&r.get::<_, String>(6)?),
        start_time: from_nanos(r.get(7)?),
        end_time: r.get::<_, Option<i64>>(8)?.map(from_nanos),
        duration_ns: r.get::<_, Option<i64>>(9)?.map(|v| v.max(0) as u64),
        is_root: r.get::<_, i64>(10)? != 0,
        is_event: r.get::<_, i64>(11)? != 0,
        entity_kind: r.get::<_, Option<String>>(12)?.and_then(|s| dec(Some(format!("\"{s}\"")))),
        entity_id: r.get(13)?,
        entity_name: r.get(14)?,
        input: json_in(r.get(15)?),
        output: json_in(r.get(16)?),
        attributes: json_in(r.get(17)?),
        metadata: json_in(r.get(18)?),
        usage: dec::<TokenUsage>(r.get(19)?),
        cost: dec::<Cost>(r.get(20)?),
        // No column holds these yet. The spec carries the shape; the storage
        // column, the migration and the write path are the model-parameter
        // track's work.
        model_parameters: None,
        error: dec::<SpanError>(r.get(21)?),
        links: dec::<Vec<SpanLink>>(r.get(22)?),
        tags: tags_in(r.get(23)?),
        correlation: Correlation {
            project: r.get(24)?,
            environment: r.get(25)?,
            service_name: r.get(26)?,
            service_version: r.get(27)?,
            session_id: r.get(28)?,
            thread_id: r.get(29)?,
            user_id: r.get(30)?,
            resource_id: r.get(31)?,
            run_id: r.get(32)?,
            request_id: r.get(33)?,
            experiment_id: r.get(34)?,
            release: r.get(35)?,
        },
        model: r.get(36)?,
        provider: r.get(37)?,
        received_at: from_nanos(r.get(38)?),
        ingest_key: None,
    })
}

/// The columns [`row_to_trace`] expects, in order.
pub(super) const TRACE_COLUMNS: &str = "\
    trace_id, root_span_id, name, kind, status, start_time, end_time, duration_ns, \
    span_count, error_count, input_tokens, output_tokens, total_cost, currency, \
    model_count, tool_count, tags, project, environment, service_name, session_id, \
    thread_id, user_id, experiment_id, release, score_summary, feedback_summary, \
    is_complete, received_at";

pub(super) fn row_to_trace(r: &rusqlite::Row<'_>) -> rusqlite::Result<Trace> {
    let input_tokens: i64 = r.get(10)?;
    let output_tokens: i64 = r.get(11)?;
    let total_cost: f64 = r.get(12)?;
    Ok(Trace {
        trace_id: r.get(0)?,
        root_span_id: r.get(1)?,
        name: r.get(2)?,
        kind: kind_of(&r.get::<_, String>(3)?),
        status: status_of(&r.get::<_, String>(4)?),
        start_time: from_nanos(r.get(5)?),
        end_time: r.get::<_, Option<i64>>(6)?.map(from_nanos),
        duration_ns: r.get::<_, Option<i64>>(7)?.map(|v| v.max(0) as u64),
        span_count: r.get::<_, i64>(8)?.max(0) as u32,
        error_count: r.get::<_, i64>(9)?.max(0) as u32,
        // Zero tokens and zero cost are reported as absent rather than as a
        // figure, because "we did not price this" and "this was free" are
        // different claims and the UI renders them differently.
        usage: (input_tokens > 0 || output_tokens > 0).then(|| TokenUsage {
            input_tokens: Some(input_tokens.max(0) as u64),
            output_tokens: Some(output_tokens.max(0) as u64),
            total_tokens: None,
            input_details: None,
            output_details: None,
        }),
        cost: (total_cost > 0.0).then(|| Cost {
            currency: r.get::<_, String>(13).unwrap_or_else(|_| "USD".into()),
            input_cost: None,
            output_cost: None,
            total_cost: Some(total_cost),
            price_book: None,
            is_estimated: true,
        }),
        model_count: r.get::<_, i64>(14)?.max(0) as u32,
        tool_count: r.get::<_, i64>(15)?.max(0) as u32,
        tags: tags_in(r.get(16)?),
        correlation: Correlation {
            project: r.get(17)?,
            environment: r.get(18)?,
            service_name: r.get(19)?,
            service_version: None,
            session_id: r.get(20)?,
            thread_id: r.get(21)?,
            user_id: r.get(22)?,
            resource_id: None,
            run_id: None,
            request_id: None,
            experiment_id: r.get(23)?,
            release: r.get(24)?,
        },
        score_summary: json_in(r.get(25)?),
        feedback_summary: json_in(r.get(26)?),
        is_complete: r.get::<_, i64>(27)? != 0,
        received_at: from_nanos(r.get(28)?),
    })
}

pub(super) const LOG_COLUMNS: &str = "\
    log_id, trace_id, span_id, timestamp, level, message, fields, logger, \
    project, environment, service_name, session_id, user_id, received_at, \
    service_version, thread_id, resource_id, run_id, request_id, experiment_id, release";

pub(super) fn row_to_log(r: &rusqlite::Row<'_>) -> rusqlite::Result<LogRecord> {
    Ok(LogRecord {
        log_id: r.get(0)?,
        trace_id: r.get(1)?,
        span_id: r.get(2)?,
        timestamp: from_nanos(r.get(3)?),
        level: LogLevel::from_str(&r.get::<_, String>(4)?).unwrap_or(LogLevel::Info),
        message: r.get(5)?,
        fields: json_in(r.get(6)?),
        logger: r.get(7)?,
        correlation: Correlation {
            project: r.get(8)?,
            environment: r.get(9)?,
            service_name: r.get(10)?,
            service_version: r.get(14)?,
            session_id: r.get(11)?,
            thread_id: r.get(15)?,
            user_id: r.get(12)?,
            resource_id: r.get(16)?,
            run_id: r.get(17)?,
            request_id: r.get(18)?,
            experiment_id: r.get(19)?,
            release: r.get(20)?,
        },
        received_at: from_nanos(r.get(13)?),
    })
}

pub(super) const SCORE_COLUMNS: &str = "\
    score_id, trace_id, span_id, scorer_id, scorer_version, source, value, \
    min_value, max_value, label, reason, score_trace_id, cost_json, metadata, \
    timestamp, project, environment, experiment_id";

pub(super) fn row_to_score(r: &rusqlite::Row<'_>) -> rusqlite::Result<Score> {
    Ok(Score {
        score_id: r.get(0)?,
        trace_id: r.get(1)?,
        span_id: r.get(2)?,
        scorer_id: r.get(3)?,
        scorer_version: r.get(4)?,
        source: ScoreSource::from_str(&r.get::<_, String>(5)?).unwrap_or(ScoreSource::Manual),
        value: r.get(6)?,
        min: r.get(7)?,
        max: r.get(8)?,
        label: r.get(9)?,
        reason: r.get(10)?,
        score_trace_id: r.get(11)?,
        cost: dec::<Cost>(r.get(12)?),
        metadata: json_in(r.get(13)?),
        timestamp: from_nanos(r.get(14)?),
        correlation: Correlation {
            project: r.get(15)?,
            environment: r.get(16)?,
            service_name: None,
            service_version: None,
            session_id: None,
            thread_id: None,
            user_id: None,
            resource_id: None,
            run_id: None,
            request_id: None,
            experiment_id: r.get(17)?,
            release: None,
        },
    })
}

pub(super) const FEEDBACK_COLUMNS: &str = "\
    feedback_id, trace_id, span_id, kind, value, comment, author_id, author_kind, \
    timestamp, metadata, project, environment";

pub(super) fn row_to_feedback(r: &rusqlite::Row<'_>) -> rusqlite::Result<Feedback> {
    Ok(Feedback {
        feedback_id: r.get(0)?,
        trace_id: r.get(1)?,
        span_id: r.get(2)?,
        kind: FeedbackKind::from_str(&r.get::<_, String>(3)?).unwrap_or(FeedbackKind::Comment),
        value: json_in(r.get(4)?).unwrap_or(Value::Null),
        comment: r.get(5)?,
        author_id: r.get(6)?,
        author_kind: r.get(7)?,
        timestamp: from_nanos(r.get(8)?),
        metadata: json_in(r.get(9)?),
        correlation: Correlation {
            project: r.get(10)?,
            environment: r.get(11)?,
            service_name: None,
            service_version: None,
            session_id: None,
            thread_id: None,
            user_id: None,
            resource_id: None,
            run_id: None,
            request_id: None,
            experiment_id: None,
            release: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nanos_round_trip_exactly() {
        let t = OffsetDateTime::from_unix_timestamp_nanos(1_700_000_000_123_456_789).unwrap();
        assert_eq!(from_nanos(to_nanos(t)), t);
    }

    #[test]
    fn malformed_json_reads_as_absent_not_an_error() {
        assert!(json_in(Some("{not json".into())).is_none());
        assert!(dec::<TokenUsage>(Some("{\"input_tokens\": ".into())).is_none());
        // A wrong-but-valid shape is also refused rather than half-read.
        assert!(dec::<TokenUsage>(Some("\"a string\"".into())).is_none());
    }

    #[test]
    fn null_json_is_not_stored() {
        assert!(json_out(&Some(Value::Null)).is_none());
        assert!(json_out(&None).is_none());
        assert_eq!(json_out(&Some(serde_json::json!({"a":1}))).unwrap(), r#"{"a":1}"#);
    }

    #[test]
    fn empty_tags_are_not_stored() {
        assert!(tags_out(&Some(vec![])).is_none());
        assert!(tags_in(Some("[]".into())).is_none());
        assert_eq!(tags_in(Some(r#"["a"]"#.into())).unwrap(), vec!["a"]);
    }

    #[test]
    fn unknown_enum_values_degrade_rather_than_fail() {
        // A row written by a newer mcpd must still be readable by an older one.
        assert_eq!(kind_of("some_future_kind"), SpanKind::Generic);
        assert_eq!(status_of("weird"), SpanStatus::Unset);
    }
}
