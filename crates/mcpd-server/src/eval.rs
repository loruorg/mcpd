//! Evaluation: running scorers over stored runs, and experiments.
//!
//! Scoring happens **against traces that are already stored**, not inline during
//! a run. That is deliberate: an evaluation that runs in the request path adds
//! latency to the thing being evaluated, and an evaluation that fails should
//! never fail the user's request.

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use mcpd_core::scoring::{Builtin, Target};
use mcpd_core::{Score, ScoreSource, ScorerDelta, SkippedScorer, Span};
use mcpd_store::{Batch, Include, TraceQuery};
use std::sync::Arc;

// Re-exported so callers name these where they are used, not where the spec
// happens to emit them.
pub use mcpd_core::{ExperimentComparison, ScoreRunRequest, ScoreRunResult, ScorerInfo};
use time::OffsetDateTime;

/// Every scorer mcpd ships.
#[must_use]
pub fn catalogue() -> Vec<ScorerInfo> {
    Builtin::ALL
        .iter()
        .map(|b| ScorerInfo {
            id: b.id().to_owned(),
            description: b.description().to_owned(),
            needs_config: b.needs_config(),
        })
        .collect()
}

/// The scorers a request runs when it names none.
///
/// Every scorer that needs no configuration, and nothing else. A scorer that
/// needs settings and was given none can only decline, so putting one in the
/// default set would add a skipped row to every default request and never a
/// score. That rule now decides real behaviour rather than only saving noise:
/// `span_present` and `tool_succeeded` are the checks that catch an agent
/// claiming work it never did, and a run they were never configured for must
/// not be reported as having passed them.
fn default_set() -> Vec<Builtin> {
    Builtin::ALL.iter().copied().filter(|b| !b.needs_config()).collect()
}

/// Run scorers over stored runs and record the results.
///
/// # Errors
/// Returns [`ApiError`] when a named scorer does not exist, or when storage
/// fails. An individual scorer declining is reported in `skipped`, not an error.
pub async fn run_scorers(state: &Arc<AppState>, req: ScoreRunRequest) -> ApiResult<ScoreRunResult> {
    let judge_by = judged_deadline(state, std::time::Instant::now());
    let project = req.project.unwrap_or_else(|| state.config.default_project.clone());

    // Resolve the scorers first: naming one that does not exist should fail
    // before any work happens, with the list of what does.
    // A judged scorer is never in the default set. Grading costs money on every
    // run it reads, so it happens only when somebody names it.
    let judged: Vec<String> = req
        .scorers
        .iter()
        .filter(|id| {
            Builtin::from_id(id).is_none()
                && state.config.judge.as_ref().is_some_and(|j| j.criteria.contains_key(*id))
        })
        .cloned()
        .collect();

    let chosen: Vec<Builtin> = if req.scorers.is_empty() {
        default_set()
    } else {
        let mut out = Vec::new();
        for id in &req.scorers {
            if judged.contains(id) {
                continue;
            }
            match Builtin::from_id(id) {
                Some(b) => out.push(b),
                None => {
                    let mut known: Vec<String> =
                        Builtin::ALL.iter().map(|b| b.id().to_owned()).collect();
                    if let Some(j) = state.config.judge.as_ref() {
                        known.extend(j.criteria.keys().cloned());
                    }
                    return Err(ApiError::BadParam {
                        field: "scorers".to_owned(),
                        message: format!(
                            "no scorer called {id:?}; available: {}",
                            known.join(", ")
                        ),
                    });
                }
            }
        }
        out
    };

    // How many of the newest runs to read when no ids are named.
    let mut limit = req.limit.unwrap_or(100).clamp(1, 500);

    // Judged scoring is held to what fits in the request deadline before any
    // run is read or any model is called. See `judged_run_cap`.
    //
    // A request that names its runs, or sets `limit`, asked for that many, and
    // more than fit is refused with the numbers. A request that named neither
    // asked for "the newest runs", and is given as many of them as fit: with
    // the default deadline and timeout that is one run per criterion, and a
    // default that always answered 400 taught nothing but to set `limit`.
    if !judged.is_empty() {
        let cap = judged_run_cap(state, judged.len());
        let named = !req.trace_ids.is_empty();
        let (field, asked) =
            if named { ("trace_ids", req.trace_ids.len()) } else { ("limit", limit as usize) };
        if cap == 0 || (asked > cap && (named || req.limit.is_some())) {
            return Err(ApiError::BadParam {
                field: field.to_owned(),
                message: over_judged_cap(state, judged.len(), cap, asked),
            });
        }
        if !named {
            limit = limit.min(u32::try_from(cap).unwrap_or(u32::MAX));
        }
    }

    // Which runs to score.
    let trace_ids: Vec<String> = if req.trace_ids.is_empty() {
        let page = state
            .store
            .list_traces(&TraceQuery {
                project: project.clone(),
                // `from` was accepted and dropped. The spec documents it as a
                // window bound and all six SDK models publish it, so a caller
                // asking to score everything since a deploy silently got the
                // newest `limit` runs instead, with no error and no clue. That
                // is the shape of defect this repository keeps finding: the
                // request succeeds and answers a different question.
                from: req.from,
                limit,
                ..Default::default()
            })
            .await?;
        page.items.into_iter().map(|t| t.trace_id).collect()
    } else {
        req.trace_ids.clone()
    };

    // Offline is the honest default: these scores were produced by reading
    // stored runs, not by watching them happen.
    let source = req.source.unwrap_or(ScoreSource::Offline);

    let config = req.config.clone().unwrap_or_default();
    let now = OffsetDateTime::now_utc();
    let mut result = ScoreRunResult {
        traces_scored: 0,
        scores_written: 0,
        skipped: Vec::new(),
        scores: Vec::new(),
    };

    for trace_id in trace_ids {
        let detail = match state.store.get_trace(&project, &trace_id, &Include::default()).await {
            Ok(d) => d,
            Err(mcpd_store::Error::NotFound { .. }) => {
                result.skipped.push(SkippedScorer {
                    scorer_id: "*".to_owned(),
                    trace_id,
                    reason: "no such run in this project".to_owned(),
                });
                continue;
            }
            Err(e) => return Err(e.into()),
        };

        let spans: Vec<Span> = detail.spans;
        let target = Target { spans: &spans };
        let correlation = target
            .root()
            .map(|s| s.correlation.clone())
            .unwrap_or_else(|| detail.trace.correlation.clone());

        result.traces_scored += 1;
        let mut produced = Vec::new();
        for scorer in &chosen {
            match scorer.run(&target, &config) {
                Some(v) => produced.push(scorer.score(
                    &v,
                    Some(trace_id.clone()),
                    None,
                    source,
                    correlation.clone(),
                    now,
                )),
                None => result.skipped.push(SkippedScorer {
                    scorer_id: scorer.id().to_owned(),
                    trace_id: trace_id.clone(),
                    reason: if scorer.needs_config() {
                        // Name the setting. With nine fields on `ScorerConfig`
                        // and seven scorers reading them, "needs
                        // configuration" is a decline nobody can act on.
                        format!(
                            "needs configuration that was not supplied: {}",
                            scorer.config_keys().join(" or ")
                        )
                    } else {
                        "the run has nothing this scorer can read".to_owned()
                    },
                }),
            }
        }

        keep(state, &project, req.dry_run, produced, &mut result).await?;

        // The judged scorers, one model call each. Sequential on purpose: a
        // scoring pass over many runs must not open as many concurrent
        // connections to somebody's rate-limited endpoint.
        //
        // Each score is kept the moment it exists. They were all written at
        // the end, and the request deadline cancels a handler where it
        // stands, so a pass that ran out of time stored nothing and every
        // model call it had made was paid for and lost.
        for id in &judged {
            let graded = crate::judge::grade(
                state,
                id,
                &target,
                &trace_id,
                correlation.clone(),
                now,
                judge_by,
            )
            .await;
            match graded {
                Ok(score) => keep(state, &project, req.dry_run, vec![score], &mut result).await?,
                Err(declined) => result.skipped.push(SkippedScorer {
                    scorer_id: id.clone(),
                    trace_id: trace_id.clone(),
                    reason: declined.to_string(),
                }),
            }
        }
    }

    Ok(result)
}

/// Store scores as they are produced, and report them.
///
/// The ceiling reaches this door too. This wrote straight to the store, past
/// the pipeline and past every quota check, so a project whose ceiling was
/// already spent could land an unbounded number of score rows here while
/// `/v1/api/scores` next door answered 429. Measured at 120 stored rows for a
/// ceiling of ten per minute, with all three counters flat.
///
/// Trimmed rather than refused: the pass has already read the run and called
/// any judged scorer, so failing the whole request would throw away work that
/// cost money. What does not fit is reported per score through `skipped`,
/// which is the field that already exists for "this one did not happen, and
/// here is why". A dry run stores nothing and reports everything.
async fn keep(
    state: &Arc<AppState>,
    project: &str,
    dry_run: bool,
    mut scores: Vec<Score>,
    result: &mut ScoreRunResult,
) -> ApiResult<()> {
    if scores.is_empty() {
        return Ok(());
    }
    if dry_run {
        result.scores.extend(scores);
        return Ok(());
    }
    let wanted = u32::try_from(scores.len()).unwrap_or(u32::MAX);
    let permitted = crate::ingest::charge_many(state, project, wanted) as usize;
    for score in scores.drain(permitted.min(scores.len())..) {
        result.skipped.push(SkippedScorer {
            scorer_id: score.scorer_id,
            trace_id: score.trace_id.unwrap_or_default(),
            reason: format!(
                "over project {project:?}'s ingest ceiling; the score was computed and not stored"
            ),
        });
    }
    if scores.is_empty() {
        return Ok(());
    }
    state.store.ingest(Batch { scores: scores.clone(), ..Batch::default() }).await?;
    result.scores_written =
        result.scores_written.saturating_add(u32::try_from(scores.len()).unwrap_or(u32::MAX));
    result.scores.extend(scores);
    Ok(())
}

/// How many runs one request may grade with `judged` judged scorers.
///
/// Each judged scorer is one model call per run, and each call may take up to
/// `judge.timeout_seconds`. The request deadline cancels the handler where it
/// stands, so a request that asks for more calls than fit inside it can end
/// half done whatever the model does. The number is the worst case on
/// purpose: a cap that held only while the model was quick would fail on the
/// day it was slow, which is the day the calls cost the most to lose.
fn judged_run_cap(state: &AppState, judged: usize) -> usize {
    let per_call = state
        .config
        .judge
        .as_ref()
        .map_or(crate::judge::DEFAULT_TIMEOUT_SECONDS, |j| j.timeout_seconds)
        .max(1);
    let per_run = per_call.saturating_mul(judged.max(1) as u64);
    usize::try_from(state.config.request_timeout_seconds / per_run).unwrap_or(usize::MAX)
}

/// When every judged call of a pass that began at `started` must have ended.
///
/// The request deadline less a margin for what the request does after its
/// last call: storing the score and writing the reply. [`judged_run_cap`]
/// divides the whole deadline by the call timeout, so a pass at the cap whose
/// judge never answers would use all of it. Each call is held to this instead,
/// and a pass that runs out of time answers with declines, inside the deadline,
/// rather than a 503 that says nothing about the runs.
///
/// A twentieth of the deadline, and never less than half a second. The rest
/// of the request is one store write and one reply, so this is slack for a
/// slow disk rather than a measurement.
fn judged_deadline(state: &AppState, started: std::time::Instant) -> std::time::Instant {
    let deadline = std::time::Duration::from_secs(state.config.request_timeout_seconds.max(1));
    let margin = (deadline / 20).max(std::time::Duration::from_millis(500));
    started + deadline.saturating_sub(margin)
}

/// The refusal for a judged request over the cap: the numbers it came from,
/// and the two settings that move it.
fn over_judged_cap(state: &AppState, judged: usize, cap: usize, asked: usize) -> String {
    let per_call = state
        .config
        .judge
        .as_ref()
        .map_or(crate::judge::DEFAULT_TIMEOUT_SECONDS, |j| j.timeout_seconds);
    let deadline = state.config.request_timeout_seconds;
    let needed = per_call.saturating_mul(judged as u64);
    let reach = if cap == 0 {
        format!(
            "so not even one run fits; raise --request-timeout-seconds to at least {needed}, \
             or lower judge.timeout_seconds"
        )
    } else {
        format!(
            "so one request grades at most {cap} run(s), and this one asks for {asked}; send at \
             most {cap} per request, or raise --request-timeout-seconds"
        )
    };
    format!(
        "judged scoring makes one model call per run for each of the {judged} judged scorer(s) \
         named, each call may take up to {per_call}s (judge.timeout_seconds), and the request \
         deadline is {deadline}s (--request-timeout-seconds), {reach}"
    )
}

/// The smallest sample mcpd will call a comparison on.
const MIN_SAMPLE: u64 = 20;

/// Compare the scores recorded under two experiment ids.
#[must_use]
pub fn compare(
    baseline: &str,
    candidate: &str,
    base: &[Score],
    cand: &[Score],
) -> ExperimentComparison {
    use std::collections::BTreeMap;

    let mut acc: BTreeMap<String, ([f64; 2], [u64; 2])> = BTreeMap::new();
    for (i, set) in [base, cand].into_iter().enumerate() {
        for s in set {
            let e = acc.entry(s.scorer_id.clone()).or_insert(([0.0, 0.0], [0, 0]));
            e.0[i] += s.value;
            e.1[i] += 1;
        }
    }

    let mut regressed = false;
    let scorers = acc
        .into_iter()
        .map(|(scorer_id, (sums, counts))| {
            let bm = (counts[0] > 0).then(|| sums[0] / counts[0] as f64);
            let cm = (counts[1] > 0).then(|| sums[1] / counts[1] as f64);
            let delta = match (bm, cm) {
                (Some(b), Some(c)) => Some(c - b),
                _ => None,
            };
            let decidable = counts[0] >= MIN_SAMPLE && counts[1] >= MIN_SAMPLE;
            if decidable && delta.is_some_and(|d| d < -0.01) {
                regressed = true;
            }
            ScorerDelta {
                scorer_id,
                baseline_mean: bm,
                baseline_n: counts[0],
                candidate_mean: cm,
                candidate_n: counts[1],
                delta,
                decidable,
            }
        })
        .collect();

    ExperimentComparison {
        baseline: baseline.to_owned(),
        candidate: candidate.to_owned(),
        scorers,
        regressed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(scorer: &str, value: f64) -> Score {
        Score {
            score_id: mcpd_core::ids::new_trace_id(),
            trace_id: None,
            span_id: None,
            scorer_id: scorer.to_owned(),
            scorer_version: None,
            source: ScoreSource::Experiment,
            value,
            min: Some(0.0),
            max: Some(1.0),
            label: None,
            reason: None,
            score_trace_id: None,
            cost: None,
            metadata: None,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            correlation: mcpd_core::Correlation {
                project: "default".into(),
                environment: None,
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
        }
    }

    #[test]
    fn the_catalogue_lists_every_scorer() {
        let c = catalogue();
        assert_eq!(c.len(), Builtin::ALL.len());
        assert!(c.iter().any(|s| s.id == "no_tool_loop"));
        assert!(c.iter().any(|s| s.needs_config));
    }

    #[test]
    fn the_trace_grounded_scorers_are_configured_or_they_do_not_run() {
        // Both read the trace for evidence of a named thing, and neither knows
        // what to look for until it is told. Running either by default would
        // put a skipped row on every request and answer nothing.
        let ids: Vec<&str> = default_set().iter().map(|b| b.id()).collect();
        for id in ["span_present", "tool_succeeded"] {
            assert!(!ids.contains(&id), "{id} must not run without being configured");
            let row = catalogue()
                .into_iter()
                .find(|s| s.id == id)
                .unwrap_or_else(|| panic!("{id} is missing from the catalogue"));
            assert!(row.needs_config, "{id} must be published as needing configuration");
            assert!(row.description.ends_with('.'), "{id} needs a full sentence");
        }
    }

    #[test]
    fn a_declining_scorer_names_the_setting_it_wanted() {
        // The skip reason is the only thing a caller sees when a scorer
        // produces nothing, so it has to say which field was missing rather
        // than that one was.
        assert_eq!(Builtin::from_id("tool_succeeded").unwrap().config_keys(), ["tool_name"]);
        assert_eq!(
            Builtin::from_id("span_present").unwrap().config_keys().join(" or "),
            "span_kind or span_name"
        );
    }

    #[test]
    fn a_small_sample_is_reported_as_undecidable_not_as_a_result() {
        let base: Vec<Score> = (0..4).map(|_| score("relevance", 0.9)).collect();
        let cand: Vec<Score> = (0..4).map(|_| score("relevance", 0.2)).collect();
        let c = compare("a", "b", &base, &cand);
        let row = &c.scorers[0];
        assert!(!row.decidable, "four runs a side is not a finding");
        assert!(!c.regressed, "an undecidable comparison must not report a regression");
        assert!((row.delta.unwrap() + 0.7).abs() < 1e-9, "the delta is still reported");
    }

    #[test]
    fn a_real_drop_on_a_real_sample_is_a_regression() {
        let base: Vec<Score> = (0..40).map(|_| score("relevance", 0.9)).collect();
        let cand: Vec<Score> = (0..40).map(|_| score("relevance", 0.5)).collect();
        let c = compare("a", "b", &base, &cand);
        assert!(c.scorers[0].decidable);
        assert!(c.regressed);
        assert!((c.scorers[0].delta.unwrap() + 0.4).abs() < 1e-9);
    }

    #[test]
    fn an_improvement_is_not_a_regression() {
        let base: Vec<Score> = (0..40).map(|_| score("relevance", 0.5)).collect();
        let cand: Vec<Score> = (0..40).map(|_| score("relevance", 0.9)).collect();
        assert!(!compare("a", "b", &base, &cand).regressed);
    }

    #[test]
    fn a_scorer_present_on_only_one_side_has_no_delta() {
        let base = vec![score("only_here", 1.0)];
        let cand = vec![score("other", 1.0)];
        let c = compare("a", "b", &base, &cand);
        assert_eq!(c.scorers.len(), 2);
        assert!(c.scorers.iter().all(|r| r.delta.is_none()));
    }
}
