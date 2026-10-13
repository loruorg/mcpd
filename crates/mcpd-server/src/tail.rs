//! Tail sampling: keep the runs worth keeping, once they have finished.
//!
//! Head sampling decides when a run starts, so it keeps a failure at the same
//! rate as a success. Tail sampling decides when a run has finished and its
//! outcome is known: a run that failed, was slow, cost too much or was scored
//! low is kept, and the rest are kept at a rate. It works on stored runs, in
//! the background, after a delay long enough for scores to arrive, and deletes
//! the runs it drops. Nothing is held in memory waiting for a run to end, and
//! the ingest path is unchanged: metrics derived at ingest still count every
//! run, which is what a dashboard of totals needs.

use crate::state::AppState;
use mcpd_core::{SpanStatus, Trace};
use mcpd_store::TraceQuery;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use time::OffsetDateTime;

/// How often a pass runs.
pub const INTERVAL: Duration = Duration::from_secs(30);

/// The most runs one pass reads per project.
const PAGE: u32 = 500;

/// A project's tail sampling rule, from its block in the config file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TailPolicy {
    /// The share of ordinary runs to keep, from 0 to 1. Chosen by a hash of
    /// the trace id, so every replica keeps the same ones.
    pub rate: f64,
    /// Keep every run in which a step failed.
    #[serde(default = "yes")]
    pub keep_errors: bool,
    /// Keep every run at least this slow, in milliseconds.
    #[serde(default)]
    pub keep_slower_than_ms: Option<u64>,
    /// Keep every run that cost at least this much.
    #[serde(default)]
    pub keep_cost_above: Option<f64>,
    /// Keep every run that any scorer scored below this.
    #[serde(default)]
    pub keep_score_below: Option<f64>,
    /// Keep every run a person gave feedback on.
    #[serde(default = "yes")]
    pub keep_feedback: bool,
    /// How long after a run finishes it is decided, in seconds. Long enough
    /// for live scoring to have scored it, so `keep_score_below` can see the
    /// score.
    #[serde(default = "default_delay")]
    pub decide_after_seconds: u64,
}

const fn yes() -> bool {
    true
}

const fn default_delay() -> u64 {
    120
}

impl TailPolicy {
    /// Refuse a rule that cannot mean anything.
    ///
    /// # Errors
    /// A message naming the setting.
    pub fn check(&self, project: &str) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.rate) || !self.rate.is_finite() {
            return Err(format!(
                "projects.{project}.tail_sampling.rate must be between 0 and 1, got {}",
                self.rate
            ));
        }
        if self.decide_after_seconds == 0 {
            return Err(format!(
                "projects.{project}.tail_sampling.decide_after_seconds must be at least 1, got 0"
            ));
        }
        Ok(())
    }

    /// Why a finished run is kept, or `None` when it is dropped.
    #[must_use]
    pub fn keep(&self, t: &Trace) -> Option<&'static str> {
        if self.keep_errors && (t.status == SpanStatus::Error || t.error_count > 0) {
            return Some("error");
        }
        if let (Some(ms), Some(d)) = (self.keep_slower_than_ms, t.duration_ns) {
            if d >= ms.saturating_mul(1_000_000) {
                return Some("slow");
            }
        }
        if let (Some(limit), Some(c)) =
            (self.keep_cost_above, t.cost.as_ref().and_then(|c| c.total_cost))
        {
            if c >= limit {
                return Some("cost");
            }
        }
        if let Some(below) = self.keep_score_below {
            let low =
                t.score_summary.as_ref().and_then(serde_json::Value::as_object).is_some_and(|m| {
                    m.values().filter_map(serde_json::Value::as_f64).any(|v| v < below)
                });
            if low {
                return Some("score");
            }
        }
        if self.keep_feedback
            && t.feedback_summary
                .as_ref()
                .and_then(serde_json::Value::as_object)
                .is_some_and(|m| !m.is_empty())
        {
            return Some("feedback");
        }
        if sampled(&t.trace_id, self.rate) {
            return Some("rate");
        }
        None
    }
}

/// Whether a run falls in the kept share, by a hash of its trace id.
#[must_use]
pub fn sampled(trace_id: &str, rate: f64) -> bool {
    crate::live::sampled("tail", trace_id, rate)
}

/// Run tail sampling on a timer, for every project that has a rule.
pub fn spawn(state: Arc<AppState>) {
    let rules: Vec<(String, TailPolicy)> = state
        .config
        .projects
        .iter()
        .filter_map(|(name, p)| p.tail_sampling.clone().map(|t| (name.clone(), t)))
        .collect();
    if rules.is_empty() {
        return;
    }
    tracing::info!(projects = rules.len(), "tail sampling finished runs");
    let decided = Arc::new(Mutex::new(HashMap::<(String, String), OffsetDateTime>::new()));
    let looped = Arc::clone(&state);
    crate::spawn_loop(
        &state,
        "tail sampling",
        INTERVAL,
        crate::FirstPass::AfterInterval,
        move || {
            let state = Arc::clone(&looped);
            let rules = rules.clone();
            let decided = Arc::clone(&decided);
            async move {
                for (project, rule) in &rules {
                    match pass(&state, project, rule, &decided, OffsetDateTime::now_utc()).await {
                        Ok(n) if n > 0 => tracing::info!(project, dropped = n, "tail sampling"),
                        Ok(_) => {}
                        Err(e) => {
                            tracing::warn!(project, error = %e, "a tail sampling pass failed")
                        }
                    }
                }
            }
        },
    );
}

/// One pass over one project: decide the runs that finished at least
/// `decide_after_seconds` ago and have not been decided, and delete the ones
/// dropped. Returns how many were dropped. Public so a test can drive it.
///
/// # Errors
/// A storage failure.
pub async fn pass(
    state: &Arc<AppState>,
    project: &str,
    rule: &TailPolicy,
    decided: &Mutex<HashMap<(String, String), OffsetDateTime>>,
    now: OffsetDateTime,
) -> Result<u32, mcpd_store::Error> {
    let delay =
        time::Duration::seconds(i64::try_from(rule.decide_after_seconds).unwrap_or(i64::MAX));
    // A run is looked at for one hour after it becomes due, which is many
    // passes; the memory of what was decided covers the same hour.
    let window = time::Duration::hours(1);
    {
        let mut d = decided.lock().unwrap_or_else(PoisonError::into_inner);
        d.retain(|_, at| *at >= now - delay - window - time::Duration::hours(1));
    }
    let page = state
        .store
        .list_traces(&TraceQuery {
            project: project.to_owned(),
            from: Some(now - delay - window - time::Duration::hours(1)),
            limit: PAGE,
            ..Default::default()
        })
        .await?;
    let mut dropped = 0u32;
    for t in page.items {
        let due = t.is_complete && t.end_time.is_some_and(|e| e <= now - delay);
        if !due {
            continue;
        }
        {
            let mut d = decided.lock().unwrap_or_else(PoisonError::into_inner);
            let key = (project.to_owned(), t.trace_id.clone());
            if d.contains_key(&key) {
                continue;
            }
            d.insert(key, t.start_time);
        }
        if rule.keep(&t).is_some() {
            continue;
        }
        state.store.delete_trace(project, &t.trace_id).await?;
        state.metrics.sampled_out.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        dropped += 1;
    }
    Ok(dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> TailPolicy {
        serde_json::from_value(serde_json::json!({ "rate": 0.0, "keep_slower_than_ms": 1000 }))
            .expect("a rule")
    }

    #[test]
    fn defaults_keep_errors_and_feedback() {
        let r = rule();
        assert!(r.keep_errors && r.keep_feedback);
        assert_eq!(r.decide_after_seconds, 120);
        assert!(r.check("p").is_ok());
        let bad = TailPolicy { rate: 2.0, ..r };
        assert!(bad.check("p").unwrap_err().contains("projects.p.tail_sampling.rate"));
    }
}
