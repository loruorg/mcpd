//! Evaluation: running scorers over stored runs, and experiments.
//!
//! Scoring happens **against traces that are already stored**, not inline during
//! a run. That is deliberate: an evaluation that runs in the request path adds
//! latency to the thing being evaluated, and an evaluation that fails should
//! never fail the user's request.

use crate::error::{ApiError, ApiResult};
use crate::judge::JudgeConfig;
use crate::state::AppState;
use mcpd_core::judged::Judged;
use mcpd_core::scoring::{Builtin, ScorerConfig, Target};
use mcpd_core::{Score, ScoreDirection, ScoreSource, ScorerDelta, ScorerKind, SkippedScorer, Span};
use mcpd_store::{Batch, DocKind, Include, TraceQuery};
use std::sync::Arc;

// Re-exported so callers name these where they are used, not where the spec
// happens to emit them.
pub use mcpd_core::{ExperimentComparison, ScoreRunRequest, ScoreRunResult, ScorerInfo};
use time::OffsetDateTime;

/// Every scorer this daemon knows: the code scorers, the judged library, and
/// the criteria the operator configured. A judged scorer is listed whether or
/// not a judge is configured, marked unavailable when none is, so a caller
/// can see what turning one on would give them.
#[must_use]
pub fn catalogue(
    judge: Option<&JudgeConfig>,
    defined: &[mcpd_core::ScorerDefinition],
) -> Vec<ScorerInfo> {
    let keys = |k: &[&str]| k.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let mut out: Vec<ScorerInfo> = Builtin::ALL
        .iter()
        .map(|b| ScorerInfo {
            id: b.id().to_owned(),
            description: b.description().to_owned(),
            needs_config: b.needs_config(),
            kind: ScorerKind::Code,
            direction: ScoreDirection::HigherIsBetter,
            config_keys: keys(b.config_keys()),
            available: true,
        })
        .collect();
    out.extend(Judged::ALL.iter().map(|j| ScorerInfo {
        id: j.id().to_owned(),
        description: j.description().to_owned(),
        needs_config: j.config_keys().contains(&"expected"),
        kind: ScorerKind::Judge,
        direction: j.direction(),
        config_keys: keys(j.config_keys()),
        available: judge.is_some(),
    }));
    if let Some(j) = judge {
        out.extend(j.criteria.iter().filter(|(id, _)| !is_shipped(id)).map(|(id, text)| {
            ScorerInfo {
                id: id.clone(),
                description: text.clone(),
                needs_config: false,
                kind: ScorerKind::Criterion,
                direction: ScoreDirection::HigherIsBetter,
                config_keys: Vec::new(),
                available: true,
            }
        }));
    }
    out.extend(defined.iter().map(|d| {
        let judged = d.kind == mcpd_core::ScorerDefinitionKind::Judge;
        ScorerInfo {
            id: d.id.clone(),
            description: d.description.clone(),
            needs_config: false,
            kind: ScorerKind::Custom,
            direction: d.direction,
            config_keys: Vec::new(),
            available: !judged || judge.is_some(),
        }
    }));
    out
}

/// Whether an id names a scorer mcpd ships, code or judged.
fn is_shipped(id: &str) -> bool {
    Builtin::from_id(id).is_some() || Judged::from_id(id).is_some()
}

/// Which way a scorer's value is better. A scorer mcpd does not ship reads as
/// higher is better, which is what a criterion asks for.
#[must_use]
pub fn direction_of(scorer_id: &str) -> ScoreDirection {
    Judged::from_id(scorer_id).map_or(ScoreDirection::HigherIsBetter, |j| j.direction())
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

/// One scorer a pass will run, resolved from the id a caller named.
#[derive(Debug, Clone)]
pub enum Planned {
    /// A code scorer. `id` and `version` name a stored definition when the
    /// scorer came from one, whose `config` then replaces the request's.
    Code {
        /// The code scorer.
        builtin: Builtin,
        /// The id scores are written under.
        id: String,
        /// The definition version, for a stored definition.
        version: Option<u32>,
        /// The definition's own settings. Boxed: the settings are large and
        /// most planned scorers have none of their own.
        config: Option<Box<ScorerConfig>>,
    },
    /// A rubric from the judged library.
    Judged(Judged),
    /// A criterion graded in one call: from the config file, or a stored
    /// judge definition.
    Criterion {
        /// The id scores are written under.
        id: String,
        /// What the judge grades against.
        text: String,
        /// The version written on each score.
        version: String,
    },
}

impl Planned {
    /// Whether this scorer makes a model call.
    #[must_use]
    pub fn is_judged(&self) -> bool {
        !matches!(self, Self::Code { .. })
    }

    /// The id its scores carry.
    #[must_use]
    pub fn id(&self) -> String {
        match self {
            Self::Code { id, .. } | Self::Criterion { id, .. } => id.clone(),
            Self::Judged(j) => j.id().to_owned(),
        }
    }
}

/// Split `id@3` into the id and the version.
fn split_version(raw: &str) -> Result<(&str, Option<u32>), String> {
    match raw.split_once('@') {
        None => Ok((raw, None)),
        Some((id, v)) => v
            .parse::<u32>()
            .map(|v| (id, Some(v)))
            .map_err(|_| format!("{raw:?} names version {v:?}; a version is a number, as in id@3")),
    }
}

/// Resolve the scorers a request names, in the order it names them.
///
/// The order of lookup is: a code scorer mcpd ships, a criterion in the config
/// file, a rubric from the judged library, then a definition the project
/// stored. A criterion the operator wrote under a shipped rubric's id keeps
/// winning, as it always has. A judged scorer with no judge configured is
/// refused here, before any run is read.
///
/// # Errors
/// [`ApiError::BadParam`] naming the first id that resolves to nothing, with
/// the list of what does.
pub async fn plan(state: &Arc<AppState>, project: &str, ids: &[String]) -> ApiResult<Vec<Planned>> {
    if ids.is_empty() {
        return Ok(default_set()
            .into_iter()
            .map(|b| Planned::Code {
                builtin: b,
                id: b.id().to_owned(),
                version: None,
                config: None,
            })
            .collect());
    }
    let judge = state.config.judge.as_ref();
    let mut out = Vec::with_capacity(ids.len());
    for raw in ids {
        let (id, version) = split_version(raw)
            .map_err(|message| ApiError::BadParam { field: "scorers".to_owned(), message })?;
        let planned = if version.is_none() && Builtin::from_id(id).is_some() {
            Builtin::from_id(id).map(|b| Planned::Code {
                builtin: b,
                id: id.to_owned(),
                version: None,
                config: None,
            })
        } else if let Some(text) =
            judge.and_then(|j| j.criteria.get(id)).filter(|_| version.is_none())
        {
            Some(Planned::Criterion {
                id: id.to_owned(),
                text: text.clone(),
                version: judge.map(|j| j.model.clone()).unwrap_or_default(),
            })
        } else if let Some(j) = Judged::from_id(id).filter(|_| version.is_none()) {
            Some(Planned::Judged(j))
        } else {
            state
                .store
                .get_document(project, DocKind::Scorer, id, version)
                .await?
                .map(|d| crate::definitions::planned_from(&d, judge))
                .transpose()?
        };
        match planned {
            Some(p) => out.push(p),
            None => {
                let mut known: Vec<String> =
                    Builtin::ALL.iter().map(|b| b.id().to_owned()).collect();
                known.extend(Judged::ALL.iter().map(|j| j.id().to_owned()));
                if let Some(j) = judge {
                    known.extend(j.criteria.keys().cloned());
                }
                for d in state.store.list_documents(project, DocKind::Scorer, None).await? {
                    known.push(d.id);
                }
                let what = match version {
                    Some(v) => format!("no scorer called {id:?} has a version {v}"),
                    None => format!("no scorer called {id:?}"),
                };
                return Err(ApiError::BadParam {
                    field: "scorers".to_owned(),
                    message: format!("{what}; available: {}", known.join(", ")),
                });
            }
        }
    }
    let judged: Vec<String> = out.iter().filter(|p| p.is_judged()).map(Planned::id).collect();
    if !judged.is_empty() && judge.is_none() {
        return Err(ApiError::BadParam {
            field: "scorers".to_owned(),
            message: format!(
                "{} grade with a model, and no judge is configured; add a `judge` block with \
                 an endpoint and a model to the config file",
                judged.join(", ")
            ),
        });
    }
    Ok(out)
}

/// Run scorers over stored runs and record the results.
///
/// # Errors
/// Returns [`ApiError`] when a named scorer does not exist, or when storage
/// fails. An individual scorer declining is reported in `skipped`, not an error.
pub async fn run_scorers(state: &Arc<AppState>, req: ScoreRunRequest) -> ApiResult<ScoreRunResult> {
    let judge_by = judged_deadline(state, std::time::Instant::now());
    let project = req.project.clone().unwrap_or_else(|| state.config.default_project.clone());

    // Resolve the scorers first: naming one that does not exist should fail
    // before any work happens, with the list of what does. A judged scorer is
    // never in the default set. Grading costs money on every run it reads, so
    // it happens only when somebody names it.
    let chosen = plan(state, &project, &req.scorers).await?;
    let judged = chosen.iter().filter(|p| p.is_judged()).count();

    // A setting that can never work is the request's fault, not each run's.
    let config = req.config.clone().unwrap_or_default();
    for p in &chosen {
        if let Planned::Code { builtin, config: None, .. } = p {
            builtin.check_config(&config).map_err(|message| ApiError::BadParam {
                field: "config".to_owned(),
                message: format!("{}: {message}", builtin.id()),
            })?;
        }
    }

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
    if judged > 0 {
        let cap = judged_run_cap(state, judged);
        let named = !req.trace_ids.is_empty();
        let (field, asked) =
            if named { ("trace_ids", req.trace_ids.len()) } else { ("limit", limit as usize) };
        if cap == 0 || (asked > cap && (named || req.limit.is_some())) {
            return Err(ApiError::BadParam {
                field: field.to_owned(),
                message: over_judged_cap(state, judged, cap, asked),
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

    let mut result = ScoreRunResult {
        traces_scored: 0,
        scores_written: 0,
        skipped: Vec::new(),
        scores: Vec::new(),
    };
    let pass = Pass { project: &project, config: &config, source, dry_run: req.dry_run, judge_by };
    for trace_id in trace_ids {
        score_trace(state, &pass, &chosen, &trace_id, &mut result).await?;
    }
    Ok(result)
}

/// What one scoring pass shares across the runs it reads.
pub struct Pass<'a> {
    /// The project the runs are in.
    pub project: &'a str,
    /// Settings for the code scorers that have none of their own.
    pub config: &'a ScorerConfig,
    /// What the scores are recorded as.
    pub source: ScoreSource,
    /// Store nothing and report everything.
    pub dry_run: bool,
    /// When every judged call must have ended.
    pub judge_by: std::time::Instant,
}

/// Score one stored run with every planned scorer, keeping each score as it
/// is produced.
///
/// # Errors
/// A storage failure. A scorer declining is a row in `result.skipped`.
pub async fn score_trace(
    state: &Arc<AppState>,
    pass: &Pass<'_>,
    chosen: &[Planned],
    trace_id: &str,
    result: &mut ScoreRunResult,
) -> ApiResult<()> {
    let now = OffsetDateTime::now_utc();
    let detail = match state.store.get_trace(pass.project, trace_id, &Include::default()).await {
        Ok(d) => d,
        Err(mcpd_store::Error::NotFound { .. }) => {
            result.skipped.push(SkippedScorer {
                scorer_id: "*".to_owned(),
                trace_id: trace_id.to_owned(),
                reason: "no such run in this project".to_owned(),
            });
            return Ok(());
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
    for p in chosen {
        let Planned::Code { builtin, id, version, config } = p else { continue };
        let settings = config.as_deref().unwrap_or(pass.config);
        match builtin.run(&target, settings) {
            Some(v) => {
                let mut s = builtin.score(
                    &v,
                    Some(trace_id.to_owned()),
                    None,
                    pass.source,
                    correlation.clone(),
                    now,
                );
                s.scorer_id.clone_from(id);
                if let Some(v) = version {
                    s.scorer_version = Some(v.to_string());
                }
                produced.push(s);
            }
            None => result.skipped.push(SkippedScorer {
                scorer_id: id.clone(),
                trace_id: trace_id.to_owned(),
                reason: if builtin.needs_config() {
                    // Name the setting. With nine fields on `ScorerConfig`
                    // and seven scorers reading them, "needs
                    // configuration" is a decline nobody can act on.
                    format!(
                        "needs configuration that was not supplied: {}",
                        builtin.config_keys().join(" or ")
                    )
                } else {
                    "the run has nothing this scorer can read".to_owned()
                },
            }),
        }
    }

    keep(state, pass.project, pass.dry_run, produced, Vec::new(), result).await?;

    // The judged scorers, one model call each. Sequential on purpose: a
    // scoring pass over many runs must not open as many concurrent
    // connections to somebody's rate-limited endpoint.
    //
    // Each score is kept the moment it exists. They were all written at the
    // end, and the request deadline cancels a handler where it stands, so a
    // pass that ran out of time stored nothing and every model call it had
    // made was paid for and lost.
    for p in chosen {
        let graded = match p {
            Planned::Code { .. } => continue,
            Planned::Judged(j) => {
                crate::judge::grade_judged(
                    state,
                    *j,
                    pass.config,
                    &target,
                    trace_id,
                    correlation.clone(),
                    now,
                    pass.judge_by,
                )
                .await
            }
            Planned::Criterion { id, text, version } => {
                crate::judge::grade_criterion(
                    state,
                    id,
                    text,
                    version,
                    &target,
                    trace_id,
                    correlation.clone(),
                    now,
                    pass.judge_by,
                )
                .await
            }
        };
        match graded {
            Ok(mut graded) => {
                graded.score.source = pass.source;
                let (score, span) = graded.into_parts();
                keep(state, pass.project, pass.dry_run, vec![score], vec![span], result).await?;
            }
            Err(declined) => result.skipped.push(SkippedScorer {
                scorer_id: p.id(),
                trace_id: trace_id.to_owned(),
                reason: declined.to_string(),
            }),
        }
    }
    Ok(())
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
///
/// `spans` are the scorer runs behind judged scores, stored with them. A span
/// whose score did not fit under the ceiling is not stored either.
async fn keep(
    state: &Arc<AppState>,
    project: &str,
    dry_run: bool,
    mut scores: Vec<Score>,
    mut spans: Vec<Span>,
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
        spans.retain(|s| s.trace_id != score.score_trace_id.clone().unwrap_or_default());
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
    state.store.ingest(Batch { scores: scores.clone(), spans, ..Batch::default() }).await?;
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
///
/// Each scorer's means are compared with Welch's t-test, which does not
/// assume the two experiments vary alike. A scorer regressed when both sides
/// have at least [`MIN_SAMPLE`] observations and the whole 95% interval on the
/// difference lies on the worse side of zero, where "worse" follows the
/// scorer's direction: a fall in `toxicity` is an improvement.
#[must_use]
pub fn compare(
    baseline: &str,
    candidate: &str,
    base: &[Score],
    cand: &[Score],
) -> ExperimentComparison {
    compare_with(baseline, candidate, base, cand, &std::collections::BTreeMap::new())
}

/// [`compare`], with the direction of each scorer the project defined, which
/// [`direction_of`] cannot know.
#[must_use]
pub fn compare_with(
    baseline: &str,
    candidate: &str,
    base: &[Score],
    cand: &[Score],
    defined: &std::collections::BTreeMap<String, ScoreDirection>,
) -> ExperimentComparison {
    use std::collections::BTreeMap;

    let mut acc: BTreeMap<String, [Vec<f64>; 2]> = BTreeMap::new();
    for (i, set) in [base, cand].into_iter().enumerate() {
        for s in set {
            acc.entry(s.scorer_id.clone()).or_default()[i].push(s.value);
        }
    }

    let mut regressed_any = false;
    let scorers = acc
        .into_iter()
        .map(|(scorer_id, [b, c])| {
            let direction =
                defined.get(&scorer_id).copied().unwrap_or_else(|| direction_of(&scorer_id));
            let (bm, cm) = (mean(&b), mean(&c));
            let delta = match (bm, cm) {
                (Some(b), Some(c)) => Some(c - b),
                _ => None,
            };
            let welch = mcpd_core::stats::welch(&b, &c);
            let decidable = b.len() as u64 >= MIN_SAMPLE && c.len() as u64 >= MIN_SAMPLE;
            let regressed = decidable
                && welch.is_some_and(|w| match direction {
                    ScoreDirection::HigherIsBetter => w.ci_high < 0.0,
                    ScoreDirection::LowerIsBetter => w.ci_low > 0.0,
                });
            regressed_any |= regressed;
            ScorerDelta {
                scorer_id,
                baseline_mean: bm,
                baseline_n: b.len() as u64,
                candidate_mean: cm,
                candidate_n: c.len() as u64,
                delta,
                decidable,
                direction,
                ci_low: welch.map(|w| w.ci_low),
                ci_high: welch.map(|w| w.ci_high),
                p_value: welch.map(|w| w.p_value),
                regressed,
            }
        })
        .collect();

    ExperimentComparison {
        baseline: baseline.to_owned(),
        candidate: candidate.to_owned(),
        scorers,
        regressed: regressed_any,
    }
}

fn mean(xs: &[f64]) -> Option<f64> {
    (!xs.is_empty()).then(|| xs.iter().sum::<f64>() / xs.len() as f64)
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
        let c = catalogue(None, &[]);
        assert_eq!(c.len(), Builtin::ALL.len() + Judged::ALL.len());
        assert!(c.iter().filter(|s| s.kind == ScorerKind::Judge).all(|s| !s.available));
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
            let row = catalogue(None, &[])
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

    #[test]
    fn a_fall_in_a_lower_is_better_scorer_is_an_improvement() {
        let base: Vec<Score> =
            (0..40).map(|i| score("toxicity", 0.5 + f64::from(i % 2) * 0.01)).collect();
        let cand: Vec<Score> =
            (0..40).map(|i| score("toxicity", 0.1 + f64::from(i % 2) * 0.01)).collect();
        let c = compare("a", "b", &base, &cand);
        assert_eq!(c.scorers[0].direction, ScoreDirection::LowerIsBetter);
        assert!(!c.regressed, "less toxic is better");
        let c = compare("a", "b", &cand, &base);
        assert!(c.regressed, "more toxic is a regression");
        assert!(c.scorers[0].ci_low.unwrap() > 0.0);
    }

    #[test]
    fn a_shift_inside_the_noise_is_not_a_regression() {
        let base: Vec<Score> =
            (0..40).map(|i| score("relevance", if i % 2 == 0 { 0.0 } else { 1.0 })).collect();
        let cand: Vec<Score> =
            (0..40).map(|i| score("relevance", if i % 2 == 0 { 0.0 } else { 0.95 })).collect();
        let c = compare("a", "b", &base, &cand);
        assert!(c.scorers[0].decidable && !c.regressed, "{:?}", c.scorers[0]);
    }
}
