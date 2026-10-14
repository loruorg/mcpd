//! Live scoring: the evaluation rules, applied to runs as they finish.
//!
//! Every pass reads each project's enabled rules and the runs that finished
//! since the last pass, and scores the ones a rule matches and samples. It runs
//! beside ingest, never inside it: a judge that is slow or down delays a score,
//! never a span. Scores are written with source `live`.
//!
//! Three bounds keep it cheap. A run is scored by a rule at most once per
//! process. Only runs that started after the daemon did are read, so a restart
//! does not score the past again. And each rule's judged scores are capped per
//! minute, so a burst of traffic cannot turn into a burst of model calls.
//!
//! Replicas sharing a store take turns through a lease (see [`crate::lease`]),
//! so only one of them scores at a time and a judged call is made once.

use crate::definitions::to_rule;
use crate::eval::{plan, score_trace, Pass};
use crate::state::AppState;
use mcpd_core::{EvaluationRule, ScoreRunResult, ScoreSource, Trace};
use mcpd_store::{DocKind, TraceQuery};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use time::OffsetDateTime;

/// How often a pass runs.
pub const INTERVAL: Duration = Duration::from_secs(15);

/// How far back a pass looks for finished runs. A run that takes longer than
/// this from its start to its end is not seen by live scoring; score it with
/// `POST /v1/api/score`.
const LOOKBACK: time::Duration = time::Duration::hours(1);

/// How long a run must have been finished before it is read, so a span that
/// arrives a moment after the root does is part of what gets scored.
const SETTLE: time::Duration = time::Duration::seconds(5);

/// The most runs one pass reads per project.
const PAGE: u32 = 500;

/// What live scoring remembers between passes.
#[derive(Default)]
struct Memory {
    /// Which rule has scored which run, with the run's start, so old entries
    /// can be dropped once they are past the look-back.
    seen: HashMap<(String, String, String), OffsetDateTime>,
    /// Judged scores spent per rule in the current minute.
    spent: HashMap<(String, String), (i64, u32)>,
}

/// Run live scoring on a timer for the life of the process.
///
/// Always started: rules are data written through the API, so there is
/// nothing in the configuration to say whether any will exist. A pass with no
/// enabled rule reads one list per project and stops.
///
/// One replica runs it at a time, the holder of the `live-scoring` lease, so
/// several replicas sharing a store make each judged call once. After each
/// pass the holder records how far it got; a replica that takes the lease
/// over skips the runs that finished before that point, which the previous
/// holder has already scored.
pub fn spawn(state: Arc<AppState>) {
    let started = OffsetDateTime::now_utc();
    let memory = Arc::new(Mutex::new(Memory::default()));
    let duty = Arc::new(crate::lease::Duty::new(JOB, crate::lease::ttl_for(INTERVAL)));
    let looped = Arc::clone(&state);
    let held = Arc::clone(&duty);
    crate::lease::spawn_leased(
        &state,
        duty,
        INTERVAL,
        crate::FirstPass::AfterInterval,
        move |granted| {
            let state = Arc::clone(&looped);
            let memory = Arc::clone(&memory);
            let duty = Arc::clone(&held);
            async move {
                let Some(granted) = granted else { return };
                let now = OffsetDateTime::now_utc();
                let after = if granted.fresh {
                    granted.lease.progress.as_deref().and_then(parse_progress)
                } else {
                    None
                };
                match pass_after(&state, &memory, started, after, now).await {
                    Ok(_) => {
                        let reached = format_progress(now - SETTLE);
                        duty.claim(&state, Some(&reached)).await;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "a live scoring pass failed; the next one retries");
                    }
                }
            }
        },
    );
}

/// The name of live scoring's lease.
pub const JOB: &str = "live-scoring";

/// Progress as the lease records it: nanoseconds since the epoch.
fn format_progress(t: OffsetDateTime) -> String {
    t.unix_timestamp_nanos().to_string()
}

fn parse_progress(s: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp_nanos(s.parse().ok()?).ok()
}

/// One pass over every project. Public so a test can drive it without a timer.
///
/// # Errors
/// A storage failure reading projects or rules. A rule that cannot run is
/// logged and skipped, and the others still run.
pub async fn pass(
    state: &Arc<AppState>,
    memory: &Arc<Mutex<impl MemoryLike>>,
    started: OffsetDateTime,
    now: OffsetDateTime,
) -> Result<u32, mcpd_store::Error> {
    pass_after(state, memory, started, None, now).await
}

/// [`pass`], skipping every run that finished at or before `after`: the
/// point a previous holder of the lease recorded it had reached.
///
/// # Errors
/// As [`pass`].
pub async fn pass_after(
    state: &Arc<AppState>,
    memory: &Arc<Mutex<impl MemoryLike>>,
    started: OffsetDateTime,
    after: Option<OffsetDateTime>,
    now: OffsetDateTime,
) -> Result<u32, mcpd_store::Error> {
    let mut written = 0u32;
    {
        let mut m = memory.lock().unwrap_or_else(PoisonError::into_inner);
        m.forget_before(now - LOOKBACK);
    }
    for project in state.store.list_projects().await? {
        let rules: Vec<EvaluationRule> = state
            .store
            .list_documents(&project, DocKind::EvalRule, None)
            .await?
            .iter()
            .map(to_rule)
            .filter(|r| r.enabled && r.sample_rate > 0.0)
            .collect();
        if rules.is_empty() {
            continue;
        }
        let page = state
            .store
            .list_traces(&TraceQuery {
                project: project.clone(),
                from: Some(started.max(now - LOOKBACK)),
                limit: PAGE,
                ..Default::default()
            })
            .await?;
        let finished: Vec<&Trace> = page
            .items
            .iter()
            .filter(|t| {
                t.is_complete
                    && t.end_time.is_some_and(|e| e <= now - SETTLE && after.is_none_or(|a| e > a))
            })
            .collect();
        if finished.is_empty() {
            continue;
        }
        for rule in &rules {
            written = written.saturating_add(run_rule(state, memory, rule, &finished, now).await);
        }
    }
    Ok(written)
}

/// What [`pass`] needs from its memory. A trait so the test can use the same
/// type the timer does without exposing its fields.
pub trait MemoryLike: Send {
    /// Drop what is older than `cutoff`.
    fn forget_before(&mut self, cutoff: OffsetDateTime);
    /// Record that `rule` has taken `trace`; false when it already had.
    fn take(&mut self, project: &str, rule: &str, trace: &Trace) -> bool;
    /// Spend one judged score of `rule`'s budget for the minute `now` is in.
    fn spend(&mut self, project: &str, rule: &str, now: OffsetDateTime, cap: u32, n: u32) -> bool;
}

impl MemoryLike for Memory {
    fn forget_before(&mut self, cutoff: OffsetDateTime) {
        self.seen.retain(|_, at| *at >= cutoff);
    }

    fn take(&mut self, project: &str, rule: &str, trace: &Trace) -> bool {
        let key = (project.to_owned(), rule.to_owned(), trace.trace_id.clone());
        if self.seen.contains_key(&key) {
            return false;
        }
        self.seen.insert(key, trace.start_time);
        true
    }

    fn spend(&mut self, project: &str, rule: &str, now: OffsetDateTime, cap: u32, n: u32) -> bool {
        let minute = now.unix_timestamp() / 60;
        let slot = self.spent.entry((project.to_owned(), rule.to_owned())).or_insert((minute, 0));
        if slot.0 != minute {
            *slot = (minute, 0);
        }
        if slot.1.saturating_add(n) > cap {
            return false;
        }
        slot.1 += n;
        true
    }
}

/// A fresh memory, for a caller that drives [`pass`] itself.
#[must_use]
pub fn memory() -> Arc<Mutex<impl MemoryLike>> {
    Arc::new(Mutex::new(Memory::default()))
}

async fn run_rule(
    state: &Arc<AppState>,
    memory: &Arc<Mutex<impl MemoryLike>>,
    rule: &EvaluationRule,
    finished: &[&Trace],
    now: OffsetDateTime,
) -> u32 {
    let chosen = match plan(state, &rule.project, &rule.scorers).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(rule = %rule.id, project = %rule.project, error = %e,
                "a live scoring rule names a scorer that cannot run; it is skipped");
            return 0;
        }
    };
    let judged = u32::try_from(chosen.iter().filter(|p| p.is_judged()).count()).unwrap_or(u32::MAX);
    let config = rule.config.clone().unwrap_or_default();
    let mut written = 0u32;
    for trace in finished {
        if !matches(rule, trace) || !sampled(&rule.id, &trace.trace_id, rule.sample_rate) {
            continue;
        }
        {
            let mut m = memory.lock().unwrap_or_else(PoisonError::into_inner);
            if !m.take(&rule.project, &rule.id, trace) {
                continue;
            }
            if judged > 0
                && !m.spend(&rule.project, &rule.id, now, rule.max_judged_per_minute, judged)
            {
                tracing::debug!(rule = %rule.id, trace_id = %trace.trace_id,
                    "over the rule's judged scores for this minute; the run is not scored");
                continue;
            }
        }
        let judge_by = std::time::Instant::now() + judged_window(state, judged);
        let pass = Pass {
            project: &rule.project,
            config: &config,
            source: ScoreSource::Live,
            dry_run: false,
            judge_by,
        };
        let mut result = ScoreRunResult {
            traces_scored: 0,
            scores_written: 0,
            skipped: Vec::new(),
            scores: Vec::new(),
        };
        if let Err(e) = score_trace(state, &pass, &chosen, &trace.trace_id, &mut result).await {
            tracing::warn!(rule = %rule.id, trace_id = %trace.trace_id, error = %e,
                "live scoring could not store a score");
            continue;
        }
        written = written.saturating_add(result.scores_written);
    }
    written
}

/// How long one run's judged calls may take together.
fn judged_window(state: &AppState, judged: u32) -> Duration {
    let per_call = state
        .config
        .judge
        .as_ref()
        .map_or(crate::judge::DEFAULT_TIMEOUT_SECONDS, |j| j.timeout_seconds)
        .max(1);
    Duration::from_secs(per_call.saturating_mul(u64::from(judged.max(1))) + 1)
}

/// Whether a run is one a rule asks for.
#[must_use]
pub fn matches(rule: &EvaluationRule, trace: &Trace) -> bool {
    rule.name.as_ref().is_none_or(|n| *n == trace.name)
        && rule.kind.is_none_or(|k| k == trace.kind)
        && rule.status.is_none_or(|s| s == trace.status)
        && rule
            .environment
            .as_ref()
            .is_none_or(|e| trace.correlation.environment.as_ref() == Some(e))
        && rule.tags.iter().all(|t| trace.tags.as_ref().is_some_and(|have| have.contains(t)))
}

/// Whether a rule samples a run, by a hash of the rule and the trace id.
///
/// The rule id is part of the hash on purpose. Head sampling hashes the trace
/// id alone, so a rule sampling at the same rate on the trace id alone would
/// pick exactly the runs head sampling kept, every time; with its own id
/// mixed in, two rules at 10% pick different tenths.
#[must_use]
pub fn sampled(rule_id: &str, trace_id: &str, rate: f64) -> bool {
    if rate >= 1.0 {
        return true;
    }
    if rate <= 0.0 {
        return false;
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in rule_id.bytes().chain([0u8]).chain(trace_id.bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^= h >> 33;
    (h as f64) < rate * u64::MAX as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampling_is_stable_and_close_to_the_rate() {
        let ids: Vec<String> = (0..10_000).map(|i| format!("{i:032x}")).collect();
        let kept = ids.iter().filter(|t| sampled("r", t, 0.1)).count();
        assert!((800..1200).contains(&kept), "kept {kept} of 10000 at 10%");
        assert_eq!(sampled("r", &ids[7], 0.1), sampled("r", &ids[7], 0.1));
        let other = ids.iter().filter(|t| sampled("r", t, 0.1) && sampled("s", t, 0.1)).count();
        assert!(other < 300, "two rules at 10% overlap on {other} runs, which is not independent");
    }

    #[test]
    fn the_budget_resets_each_minute() {
        let mut m = Memory::default();
        let t = OffsetDateTime::from_unix_timestamp(600).unwrap_or(OffsetDateTime::UNIX_EPOCH);
        assert!(m.spend("p", "r", t, 2, 1));
        assert!(m.spend("p", "r", t, 2, 1));
        assert!(!m.spend("p", "r", t, 2, 1), "the third call in a minute is over a cap of two");
        assert!(m.spend("p", "r", t + time::Duration::minutes(1), 2, 1));
    }
}
