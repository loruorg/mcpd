//! A run read as the actions its agent took.
//!
//! A trace is a tree of spans, most of which are plumbing: processors,
//! retrieval internals, model steps inside a generation. A trajectory keeps
//! the actions a reviewer judges an agent by, in the order they started: each
//! tool call, each model generation and each handoff. That is the sequence
//! the `trajectory` scorer compares with what the run should have done.

use crate::preview::value_preview;
use crate::{Span, SpanKind, SpanStatus, Trajectory, TrajectoryStep};

/// Whether a span kind is a tool call of any kind.
#[must_use]
pub const fn is_tool(kind: SpanKind) -> bool {
    matches!(
        kind,
        SpanKind::ToolCall
            | SpanKind::ClientToolCall
            | SpanKind::ProviderToolCall
            | SpanKind::McpToolCall
    )
}

/// The trajectory of a run, from its spans in any order.
///
/// A model generation is the unit of a model action. A run that records only
/// the provider round trips, with no generation span, has each round trip
/// counted instead, so a trace from a bare OpenTelemetry instrumentation still
/// has model steps.
#[must_use]
pub fn trajectory(trace_id: &str, spans: &[Span]) -> Trajectory {
    let has_generation = spans.iter().any(|s| s.kind == SpanKind::ModelGeneration);
    let model_kind =
        if has_generation { SpanKind::ModelGeneration } else { SpanKind::ModelInference };
    let mut picked: Vec<&Span> = spans
        .iter()
        .filter(|s| {
            !s.is_event && (is_tool(s.kind) || s.kind == model_kind || s.kind == SpanKind::Handoff)
        })
        .collect();
    picked.sort_by(|a, b| a.start_time.cmp(&b.start_time).then(a.span_id.cmp(&b.span_id)));
    let origin = spans.iter().map(|s| s.start_time).min();
    let ms = |ns: i128| ns as f64 / 1e6;
    let steps: Vec<TrajectoryStep> = picked
        .iter()
        .enumerate()
        .map(|(i, s)| TrajectoryStep {
            index: u32::try_from(i).unwrap_or(u32::MAX),
            span_id: s.span_id.clone(),
            kind: s.kind,
            name: s.name.clone(),
            status: s.status,
            start_offset_ms: origin.map_or(0.0, |o| ms((s.start_time - o).whole_nanoseconds())),
            duration_ms: s.end_time.map(|e| ms((e - s.start_time).whole_nanoseconds()).max(0.0)),
            model: s.model.clone(),
            input_preview: s.input.as_ref().and_then(value_preview),
            output_preview: s.output.as_ref().and_then(value_preview),
            tokens: s
                .usage
                .as_ref()
                .map(|u| u.input_tokens.unwrap_or(0) + u.output_tokens.unwrap_or(0))
                .filter(|n| *n > 0),
        })
        .collect();
    let root =
        spans.iter().find(|s| s.is_root).or_else(|| spans.iter().min_by_key(|s| s.start_time));
    let final_output_preview = root.and_then(|r| r.output.as_ref()).and_then(value_preview);
    let failed = spans.iter().any(|s| s.status == SpanStatus::Error);
    Trajectory {
        trace_id: trace_id.to_owned(),
        tool_sequence: steps.iter().filter(|s| is_tool(s.kind)).map(|s| s.name.clone()).collect(),
        step_count: u32::try_from(steps.len()).unwrap_or(u32::MAX),
        tool_count: u32::try_from(steps.iter().filter(|s| is_tool(s.kind)).count())
            .unwrap_or(u32::MAX),
        error_count: u32::try_from(steps.iter().filter(|s| s.status == SpanStatus::Error).count())
            .unwrap_or(u32::MAX),
        goal_reached: !failed && final_output_preview.is_some(),
        final_output_preview,
        steps,
    }
}

/// How `trajectory` compares the tools called with the tools expected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    /// The same calls, in the same order, and nothing else.
    Strict,
    /// The expected calls in order, with others allowed between them.
    Ordered,
    /// Every expected call, in any order.
    Unordered,
}

impl Match {
    /// Parse the `trajectory_match` setting. Absent is `ordered`.
    ///
    /// # Errors
    /// A value that is none of the three, with the three named.
    pub fn parse(raw: Option<&str>) -> Result<Self, String> {
        match raw {
            None | Some("ordered") => Ok(Self::Ordered),
            Some("strict") => Ok(Self::Strict),
            Some("unordered") => Ok(Self::Unordered),
            Some(other) => {
                Err(format!("trajectory_match is strict, ordered or unordered, not {other:?}"))
            }
        }
    }
}

/// The share of `expected` the `called` sequence satisfies, from 0 to 1.
///
/// Partial credit by design. A run that made four of five expected calls in
/// order is closer than one that made none, and a binary answer would put
/// both at zero.
#[must_use]
pub fn tool_match(expected: &[String], called: &[String], how: Match) -> f64 {
    if expected.is_empty() {
        return 1.0;
    }
    match how {
        Match::Unordered => {
            let mut left: Vec<&String> = called.iter().collect();
            let mut hit = 0usize;
            for e in expected {
                if let Some(i) = left.iter().position(|c| *c == e) {
                    left.swap_remove(i);
                    hit += 1;
                }
            }
            hit as f64 / expected.len() as f64
        }
        Match::Ordered => {
            let mut want = expected.iter().peekable();
            let mut hit = 0usize;
            for c in called {
                if want.peek().is_some_and(|w| *w == c) {
                    want.next();
                    hit += 1;
                }
            }
            hit as f64 / expected.len() as f64
        }
        Match::Strict => {
            if expected == called {
                return 1.0;
            }
            // The longest common subsequence over the longer of the two, so an
            // extra call costs as much as a missing one.
            let (n, m) = (expected.len(), called.len());
            let mut t = vec![vec![0usize; m + 1]; n + 1];
            for i in (0..n).rev() {
                for j in (0..m).rev() {
                    t[i][j] = if expected[i] == called[j] {
                        t[i + 1][j + 1] + 1
                    } else {
                        t[i + 1][j].max(t[i][j + 1])
                    };
                }
            }
            t[0][0] as f64 / n.max(m) as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn the_three_ways_of_matching_give_partial_credit() {
        let want = names(&["search", "lookup", "refund"]);
        let called = names(&["search", "log", "refund"]);
        assert!((tool_match(&want, &called, Match::Unordered) - 2.0 / 3.0).abs() < 1e-9);
        // `lookup` never came, so the order stops counting after `search`.
        assert!((tool_match(&want, &called, Match::Ordered) - 1.0 / 3.0).abs() < 1e-9);
        assert!((tool_match(&want, &called, Match::Strict) - 2.0 / 3.0).abs() < 1e-9);
        assert!((tool_match(&want, &want, Match::Strict) - 1.0).abs() < 1e-9);
        let extra = names(&["search", "lookup", "refund", "refund"]);
        assert!(tool_match(&want, &extra, Match::Strict) < 1.0, "an extra call is not strict");
        assert!((tool_match(&want, &extra, Match::Ordered) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn an_unknown_mode_is_refused_by_name() {
        assert_eq!(Match::parse(None), Ok(Match::Ordered));
        assert!(Match::parse(Some("loose")).unwrap_err().contains("strict, ordered or unordered"));
    }
}
