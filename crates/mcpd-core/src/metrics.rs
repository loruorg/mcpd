//! Metric label hygiene.
//!
//! A metric with an unbounded label is not a metric, it is a slow way to run
//! out of memory: every distinct value creates a new time series, and a label
//! carrying a user id or a trace id creates one per user or per request. Every
//! metric backend falls over on this eventually, and the failure shows up as a
//! storage bill rather than an error, which is why it needs stopping at ingest.
//!
//! The identity of a run is not lost by this: it lives on the exemplar, which
//! is the correct place for it and links straight back to the trace.

use serde_json::Value;

/// Label keys that are unbounded by nature and must never become a dimension.
const UNBOUNDED: &[&str] = &[
    "trace_id",
    "traceid",
    "span_id",
    "spanid",
    "user_id",
    "userid",
    "session_id",
    "sessionid",
    "request_id",
    "requestid",
    "run_id",
    "runid",
    "thread_id",
    "threadid",
    "message_id",
    "messageid",
    "email",
    "ip",
    "ip_address",
    "url",
    "path",
    "query",
    "timestamp",
    "uuid",
    "id",
];

/// Drops label keys that would explode a metric backend's series count.
#[derive(Debug, Clone)]
pub struct CardinalityFilter {
    blocked: Vec<String>,
    /// Longest label value kept. A long value is nearly always accidental and
    /// is a cardinality problem in disguise.
    max_value_len: usize,
    /// Most labels kept on one point.
    max_labels: usize,
}

impl Default for CardinalityFilter {
    fn default() -> Self {
        Self {
            blocked: UNBOUNDED.iter().map(|s| (*s).to_owned()).collect(),
            max_value_len: 120,
            max_labels: 24,
        }
    }
}

/// What a filtering pass removed, so the caller can report it rather than
/// silently discarding a dimension somebody was relying on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dropped {
    /// Keys removed because they are unbounded by nature.
    pub keys: Vec<String>,
    /// Keys removed because the point carried too many labels.
    pub over_limit: usize,
    /// Keys whose value was too long to be a real dimension.
    pub over_length: Vec<String>,
}

impl Dropped {
    /// Whether anything was removed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty() && self.over_limit == 0 && self.over_length.is_empty()
    }
}

impl CardinalityFilter {
    /// Block an additional key.
    #[must_use]
    pub fn blocking(mut self, key: impl Into<String>) -> Self {
        self.blocked.push(key.into().to_ascii_lowercase());
        self
    }

    /// Whether this key may be used as a metric dimension.
    #[must_use]
    pub fn allows(&self, key: &str) -> bool {
        let k = key.to_ascii_lowercase();
        !self.blocked.iter().any(|b| k == *b || k.ends_with(&format!(".{b}")))
    }

    /// Filter a label map in place, returning what was removed.
    pub fn filter(&self, labels: &mut Option<Value>) -> Dropped {
        let mut dropped = Dropped::default();
        let Some(Value::Object(map)) = labels.as_mut() else { return dropped };

        map.retain(|k, v| {
            if !self.allows(k) {
                dropped.keys.push(k.clone());
                return false;
            }
            if let Value::String(s) = v {
                if s.len() > self.max_value_len {
                    dropped.over_length.push(k.clone());
                    return false;
                }
            }
            true
        });

        if map.len() > self.max_labels {
            // Keep a deterministic subset rather than an arbitrary one, so the
            // same point always yields the same series.
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();
            for k in keys.into_iter().skip(self.max_labels) {
                map.remove(&k);
                dropped.over_limit += 1;
            }
        }

        if map.is_empty() {
            *labels = None;
        }
        dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn blocks_the_labels_that_explode_a_backend() {
        let f = CardinalityFilter::default();
        for k in ["trace_id", "user_id", "session_id", "request_id", "email", "uuid"] {
            assert!(!f.allows(k), "{k} should be blocked");
        }
    }

    #[test]
    fn keeps_the_labels_that_are_actually_dimensions() {
        let f = CardinalityFilter::default();
        for k in ["model", "provider", "status", "entity_name", "environment", "kind"] {
            assert!(f.allows(k), "{k} should be allowed");
        }
    }

    #[test]
    fn filters_and_reports_what_it_removed() {
        let f = CardinalityFilter::default();
        let mut labels = Some(json!({ "model": "gpt-4o", "user_id": "u-1", "status": "ok" }));
        let dropped = f.filter(&mut labels);
        assert_eq!(dropped.keys, vec!["user_id"]);
        let m = labels.unwrap();
        assert_eq!(m["model"], json!("gpt-4o"));
        assert!(m.get("user_id").is_none());
    }

    #[test]
    fn a_long_value_is_a_cardinality_problem_in_disguise() {
        let f = CardinalityFilter::default();
        let mut labels = Some(json!({ "note": "x".repeat(400) }));
        let dropped = f.filter(&mut labels);
        assert_eq!(dropped.over_length, vec!["note"]);
        assert!(labels.is_none(), "an empty label map becomes absent");
    }

    #[test]
    fn caps_the_number_of_labels_deterministically() {
        let f = CardinalityFilter::default();
        let mut a =
            Some(Value::Object((0..40).map(|i| (format!("k{i:02}"), json!("v"))).collect()));
        let mut b = a.clone();
        let da = f.filter(&mut a);
        f.filter(&mut b);
        assert_eq!(da.over_limit, 16);
        assert_eq!(a, b, "the same point must always yield the same series");
    }

    #[test]
    fn dotted_namespaces_are_matched_too() {
        let f = CardinalityFilter::default();
        assert!(!f.allows("mcpd.user_id"));
        assert!(f.allows("mcpd.model"));
    }

    #[test]
    fn absent_labels_are_left_alone() {
        let f = CardinalityFilter::default();
        let mut none = None;
        assert!(f.filter(&mut none).is_empty());
        assert!(none.is_none());
    }
}
