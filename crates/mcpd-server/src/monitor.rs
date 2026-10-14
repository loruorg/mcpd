//! Monitors: a condition over stored signals, evaluated on a timer.
//!
//! A monitor is not a second query language. It is a saved question with a
//! threshold, evaluated on a schedule, that **latches** when it trips: it stays
//! tripped until someone acknowledges it, exactly like the annunciator tile it
//! is drawn as. An alert that clears itself while nobody was looking is an alert
//! that never happened.
//!
//! Notification is deliberately a webhook and nothing else. mcpd does not know
//! your paging policy and should not pretend to; anything that can receive a
//! POST can route from there.

use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use time::{Duration, OffsetDateTime};

/// What a monitor watches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Watch {
    /// The fraction of runs that failed, 0 to 1.
    ErrorRate,
    /// Run duration at the ninety-fifth percentile, in milliseconds.
    LatencyP95,
    /// Total spend across the window.
    Cost,
    /// How many runs happened. Catches a pipeline that has gone silent.
    TraceCount,
    /// The mean of a named scorer.
    ScoreMean,
    /// How far runs stray from the shape their own definition version usually
    /// takes, counted as the number of runs in the window that strayed.
    ///
    /// The only watch here with no threshold in the units of the thing it
    /// measures. Nobody has to decide how many steps a definition ought to
    /// take, because the recent runs of that definition decide it. See
    /// [`evaluate_divergence`].
    StepCountDivergence,
    /// How much a run repeats itself: the mean, over the runs in the window, of
    /// the most attempts any one of them made at a single node of its
    /// definition.
    ///
    /// A retry and a loop iteration of one node count alike, so one means no
    /// node ran twice. This one does carry a threshold in the units it
    /// measures, unlike [`Watch::StepCountDivergence`], because a mean attempt
    /// count is meaningful in absolute terms and nobody needs a cohort to read
    /// it. See [`evaluate_repetition`].
    RunRepetition,
}

/// Which way the threshold is breached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compare {
    /// Trip when the value rises above the threshold.
    Above,
    /// Trip when the value falls below it. This is the one that catches a
    /// quality regression, and a silent pipeline.
    Below,
}

/// A saved condition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Monitor {
    /// Stable identifier.
    pub id: String,
    /// What a person should read when it trips.
    pub name: String,
    /// Which project it watches.
    pub project: String,
    /// What it measures.
    pub watch: Watch,
    /// Which scorer, when `watch` is `score_mean`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scorer_id: Option<String>,
    /// Which direction trips it.
    pub compare: Compare,
    /// The threshold.
    pub threshold: f64,
    /// How far back each evaluation looks, in seconds.
    #[serde(default = "default_window")]
    pub window_seconds: u32,
    /// The fewest runs the window must contain before the monitor will trip.
    ///
    /// Without this a single failed run in a quiet hour is a 100% error rate,
    /// and the first thing anyone does with an alert that cries wolf is mute it.
    #[serde(default = "default_min_samples")]
    pub min_samples: u64,
    /// How far one run may stray from its definition's usual step count before
    /// it counts as divergent, as a modified z-score.
    ///
    /// Read only by `step_count_divergence`, and separate from `threshold`
    /// because the two answer different questions. This one asks how far is too
    /// far for a single run, and it is a standardised score, so the same 3.5
    /// serves a definition that usually takes four steps and one that usually
    /// takes four hundred. `threshold` then asks how many such runs a window
    /// may hold before anyone is told, which is an operational choice and not a
    /// statistical one.
    ///
    /// Not yet declared on the `Monitor` type in `spec/mcpd.spec.json`, so
    /// `GET /v1/api/monitors` returns it while the generated reference does not
    /// list it. Adding it there, `f64` with a default of `3.5`, carries it into
    /// all six SDKs and closes the gap.
    #[serde(default = "default_z_bound")]
    pub z_bound: f64,
    /// Where to POST when it trips or clears.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook: Option<String>,
    /// Whether it is evaluated at all.
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn default_window() -> u32 {
    300
}
fn default_min_samples() -> u64 {
    20
}
fn default_z_bound() -> f64 {
    DEFAULT_Z_BOUND
}
fn yes() -> bool {
    true
}

/// Where a monitor currently stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    /// Evaluated, and within its threshold.
    Nominal,
    /// Tripped, and not yet acknowledged.
    Tripped,
    /// Tripped, acknowledged, and still breaching.
    Acknowledged,
    /// Not enough data in the window to judge.
    Insufficient,
}

/// One evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reading {
    /// Which monitor.
    pub monitor_id: String,
    /// When it was evaluated. RFC 3339, like every other timestamp mcpd emits.
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    /// The measured value, when there was enough data to measure one.
    pub value: Option<f64>,
    /// How many runs the window held.
    pub samples: u64,
    /// The resulting condition.
    pub condition: Condition,
    /// What a person should read.
    pub detail: String,
}

/// Evaluate one monitor against storage.
///
/// Returns the reading. Latching is the caller's business, because the previous
/// condition lives with the caller.
///
/// # Errors
/// Returns a storage error if the underlying queries fail.
pub async fn evaluate(
    state: &Arc<AppState>,
    monitor: &Monitor,
    now: OffsetDateTime,
) -> Result<Reading, mcpd_store::Error> {
    // The two run-store watches are dispatched before the gate below, because
    // that gate counts traces and they count durable runs. A deployment can
    // serve runs whose spans are ingested somewhere else entirely, and gating on
    // a trace count they do not read would decline every reading they ever took.
    // An exhaustive match rather than a pair of `if`s, so a watch added later
    // has to say which side of the gate it belongs on.
    match monitor.watch {
        Watch::StepCountDivergence => return evaluate_divergence(state, monitor, now).await,
        Watch::RunRepetition => return evaluate_repetition(state, monitor, now).await,
        Watch::ErrorRate
        | Watch::LatencyP95
        | Watch::Cost
        | Watch::TraceCount
        | Watch::ScoreMean => {}
    }

    let from = now - Duration::seconds(i64::from(monitor.window_seconds));
    let summary = state.store.analytics_summary(&monitor.project, from, now, None).await?;
    let samples = summary.trace_count;

    // Refuse to judge a window that is too thin to mean anything.
    if samples < monitor.min_samples {
        return Ok(Reading {
            monitor_id: monitor.id.clone(),
            at: now,
            value: None,
            samples,
            condition: Condition::Insufficient,
            detail: format!(
                "{samples} run(s) in the last {}s; needs at least {}",
                monitor.window_seconds, monitor.min_samples
            ),
        });
    }

    let (value, unit) = match monitor.watch {
        Watch::ErrorRate => (summary.error_rate, "".to_owned()),
        Watch::LatencyP95 => (summary.latency.p95.unwrap_or(0.0), " ms".to_owned()),
        Watch::Cost => (summary.total_cost, format!(" {}", summary.currency)),
        Watch::TraceCount => (samples as f64, " runs".to_owned()),
        Watch::ScoreMean => {
            let scorer = monitor.scorer_id.clone().unwrap_or_default();
            let scores = state
                .store
                .list_scores(&mcpd_store::ScoreQuery {
                    project: monitor.project.clone(),
                    scorer_id: Some(scorer.clone()),
                    from: Some(from),
                    to: Some(now),
                    limit: 500,
                    ..Default::default()
                })
                .await?;
            if scores.items.is_empty() {
                return Ok(Reading {
                    monitor_id: monitor.id.clone(),
                    at: now,
                    value: None,
                    samples: 0,
                    condition: Condition::Insufficient,
                    detail: format!("no {scorer} scores in the last {}s", monitor.window_seconds),
                });
            }
            let n = scores.items.len() as f64;
            let mean = scores.items.iter().map(|s| s.value).sum::<f64>() / n;
            (mean, "".to_owned())
        }
        // Both returned above. Arms rather than a wildcard, so that adding a
        // watch is a compile error here instead of a silent zero.
        Watch::StepCountDivergence => return evaluate_divergence(state, monitor, now).await,
        Watch::RunRepetition => return evaluate_repetition(state, monitor, now).await,
    };

    let breached = match monitor.compare {
        Compare::Above => value > monitor.threshold,
        Compare::Below => value < monitor.threshold,
    };

    Ok(Reading {
        monitor_id: monitor.id.clone(),
        at: now,
        value: Some(value),
        samples,
        condition: if breached { Condition::Tripped } else { Condition::Nominal },
        detail: format!(
            "{} is {value:.4}{unit}, threshold {} {:.4}{unit}, over {samples} run(s)",
            monitor.name,
            match monitor.compare {
                Compare::Above => "above",
                Compare::Below => "below",
            },
            monitor.threshold,
        ),
    })
}

/// A monitor plus the condition it is latched in.
#[derive(Debug, Clone, Serialize)]
pub struct MonitorState {
    /// The monitor.
    pub monitor: Monitor,
    /// Its current condition.
    pub condition: Condition,
    /// The most recent reading.
    pub last: Option<Reading>,
    /// When it first tripped, while it stays tripped.
    ///
    /// RFC 3339, like every other timestamp mcpd emits. Left to its default,
    /// `time` serialises a component array, which no SDK's date parser reads.
    #[serde(with = "time::serde::rfc3339::option")]
    pub tripped_at: Option<OffsetDateTime>,
}

impl MonitorState {
    /// A monitor that has not been evaluated yet.
    ///
    /// The starting condition is `Insufficient`, not `Nominal`: mcpd has read
    /// nothing, and reporting "all clear" before the first evaluation would be
    /// a claim it cannot support.
    #[must_use]
    pub fn new(monitor: Monitor) -> Self {
        Self { monitor, condition: Condition::Insufficient, last: None, tripped_at: None }
    }

    /// Fold a new reading in, applying the latching rule.
    ///
    /// Returns `true` when the caller should notify: on the transition into
    /// tripped, and on the transition back to nominal, and never in between. An
    /// alert that fires every evaluation while a condition persists is one
    /// people filter out.
    pub fn apply(&mut self, reading: Reading) -> bool {
        let was = self.condition;
        let now = reading.condition;

        self.condition = match (was, now) {
            // A tripped monitor stays tripped until it genuinely recovers, and
            // an acknowledged one stays acknowledged while still breaching.
            (Condition::Acknowledged, Condition::Tripped) => Condition::Acknowledged,
            // Too little data does not clear a standing alert: silence is not
            // recovery, and treating it as such is how an outage goes unnoticed.
            (Condition::Tripped | Condition::Acknowledged, Condition::Insufficient) => was,
            (_, other) => other,
        };

        if self.condition == Condition::Tripped && self.tripped_at.is_none() {
            self.tripped_at = Some(reading.at);
        }
        if self.condition == Condition::Nominal {
            self.tripped_at = None;
        }

        self.last = Some(reading);

        let notify_now = matches!(self.condition, Condition::Tripped)
            && !matches!(was, Condition::Tripped | Condition::Acknowledged);
        let recovered = matches!(self.condition, Condition::Nominal)
            && matches!(was, Condition::Tripped | Condition::Acknowledged);
        notify_now || recovered
    }

    /// Acknowledge a tripped monitor. It stays acknowledged until it recovers.
    pub fn acknowledge(&mut self) {
        if self.condition == Condition::Tripped {
            self.condition = Condition::Acknowledged;
        }
    }
}

/// Evaluate every configured monitor once and fold the readings in.
///
/// Returns the monitors whose condition changed, which are the only ones worth
/// telling anyone about.
pub async fn tick(state: &Arc<AppState>, now: OffsetDateTime) -> Vec<MonitorState> {
    let monitors: Vec<Monitor> = state.config.monitors.clone();
    let mut changed = Vec::new();

    for monitor in monitors.iter().filter(|m| m.enabled) {
        // A storage error is not a reading. Recording one as `Insufficient`
        // would be indistinguishable from a quiet window, and a quiet window
        // must never clear a standing alert.
        let reading = match evaluate(state, monitor, now).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(monitor = %monitor.id, error = %e, "could not evaluate a monitor");
                continue;
            }
        };

        let mut states = recover(&state.monitors, "monitor");
        let Some(slot) = states.iter_mut().find(|s| s.monitor.id == monitor.id) else { continue };
        if slot.apply(reading) {
            changed.push(slot.clone());
        }
    }

    changed
}

/// The write half of a state lock, taken back from a holder that panicked.
///
/// A panic while the lock was held poisons it, and this used to read a
/// poisoned lock as "stop": the tick returned early on every pass after, so
/// the monitors stopped latching while the loop itself kept running and
/// looked healthy. The state behind it is derived, folded again from the next
/// reading, so the right answer is to clear the poison, say so, and carry on.
/// Clearing it also lets the list and acknowledge routes read it again.
fn recover<'a, T>(
    lock: &'a std::sync::RwLock<T>,
    what: &str,
) -> std::sync::RwLockWriteGuard<'a, T> {
    lock.write().unwrap_or_else(|poisoned| {
        tracing::warn!(
            "the {what} state was left mid-update by a panic; it is cleared and the next \
             reading folds into it as usual"
        );
        lock.clear_poison();
        poisoned.into_inner()
    })
}

/// POST a transition to a monitor's webhook.
///
/// One generic JSON body rather than a per-vendor payload. Slack, PagerDuty,
/// Opsgenie and every workflow tool accept an inbound webhook, so shaping the
/// body for any one of them would mean maintaining a set of integrations that
/// go stale. The body carries the fields a receiver needs to route and explain
/// the alert, and a relay turns it into whatever a given vendor wants.
///
/// A failed delivery is logged and dropped. Retrying would either block the
/// evaluation loop or need a durable outbound queue, and an alerting path that
/// can silently fall behind is worse than one that visibly missed a message.
async fn notify(state: &Arc<AppState>, s: &MonitorState) {
    let Some(url) = s.monitor.webhook.as_deref() else { return };
    let body = serde_json::json!({
        "monitor_id": s.monitor.id,
        "name": s.monitor.name,
        "project": s.monitor.project,
        "condition": s.condition,
        "watch": s.monitor.watch,
        "threshold": s.monitor.threshold,
        "value": s.last.as_ref().and_then(|r| r.value),
        "samples": s.last.as_ref().map_or(0, |r| r.samples),
        "detail": s.last.as_ref().map(|r| r.detail.clone()),
        "at": s.last.as_ref().map(|r| r.at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()),
    });
    match state.outbound.post(url).json(&body).send().await {
        Ok(res) if res.status().is_success() => {}
        Ok(res) => tracing::warn!(
            monitor = %s.monitor.id,
            status = %res.status(),
            "the alert webhook refused the notification"
        ),
        // The host, and the error without its URL: a webhook URL is often its
        // own credential, and this line goes to whoever reads the logs.
        Err(e) => tracing::warn!(
            monitor = %s.monitor.id,
            host = crate::query::webhook_host(Some(url)).unwrap_or_default(),
            error = crate::outbound_error(e),
            "could not reach the alert webhook"
        ),
    }
}

/// The shortest interval the monitors and objectives are evaluated on, in
/// seconds. A smaller `monitor_interval_seconds` is raised to this, and
/// `mcpd config` prints the raised value, because that is the one that runs.
pub const MIN_INTERVAL_SECONDS: u64 = 5;

/// The name of the lease whose holder announces monitor and objective
/// transitions.
pub const JOB: &str = "monitors";

/// Run the monitors on a timer for the life of the process.
///
/// A transition is always logged, and posted to the monitor's webhook when it
/// has one. The log line carries every field a structured log pipeline needs to
/// route it, so mcpd is useful to an operator who already has one and does not
/// force a webhook on anybody.
pub fn spawn(state: Arc<AppState>) {
    let armed = state.config.monitors.iter().filter(|m| m.enabled).count();
    let objectives = state.config.slos.iter().filter(|s| s.enabled).count();
    if armed == 0 && objectives == 0 {
        return;
    }
    let every = std::time::Duration::from_secs(
        state.config.monitor_interval_seconds.max(MIN_INTERVAL_SECONDS),
    );
    tracing::info!(monitors = armed, interval_seconds = every.as_secs(), "watching conditions");

    // One interval before the first pass, so a monitor never judges an empty
    // window at start-up. A panic in one pass is logged and the next runs.
    //
    // Every replica evaluates, so each answers `GET /v1/api/monitors` from its
    // own readings and a replica that takes over already knows which alerts
    // are latched. Only the holder of the `monitors` lease announces a
    // transition, so a webhook is posted once however many replicas there are.
    let looped = Arc::clone(&state);
    let duty = Arc::new(crate::lease::Duty::new(JOB, crate::lease::ttl_for(every)));
    crate::lease::spawn_leased(&state, duty, every, crate::FirstPass::AfterInterval, move |g| {
        let state = Arc::clone(&looped);
        async move {
            let at = OffsetDateTime::now_utc();
            let slos = tick_slos(&state, at).await;
            let monitors = tick(&state, at).await;
            if g.is_none() {
                return;
            }

            for s in slos {
                let r = s.last.as_ref();
                match s.condition {
                    Condition::Tripped => tracing::error!(
                        slo = %s.slo.id,
                        name = %s.slo.name,
                        project = %s.slo.project,
                        objective = s.slo.objective,
                        budget_remaining = r.and_then(|x| x.budget_remaining),
                        burn_fast = r.and_then(|x| x.burn_fast),
                        burn_slow = r.and_then(|x| x.burn_slow),
                        "an objective is burning its error budget"
                    ),
                    Condition::Nominal => tracing::info!(
                        slo = %s.slo.id,
                        name = %s.slo.name,
                        budget_remaining = r.and_then(|x| x.budget_remaining),
                        "an objective recovered"
                    ),
                    Condition::Acknowledged | Condition::Insufficient => {}
                }
                notify_slo(&state, &s).await;
            }

            for s in monitors {
                let value = s.last.as_ref().and_then(|r| r.value);
                match s.condition {
                    Condition::Tripped => tracing::error!(
                        monitor = %s.monitor.id,
                        name = %s.monitor.name,
                        project = %s.monitor.project,
                        threshold = s.monitor.threshold,
                        value,
                        "a monitor tripped"
                    ),
                    Condition::Nominal => tracing::info!(
                        monitor = %s.monitor.id,
                        name = %s.monitor.name,
                        value,
                        "a monitor recovered"
                    ),
                    Condition::Acknowledged | Condition::Insufficient => {}
                }
                notify(&state, &s).await;
            }
        }
    });
}

// ------------------------------------------------ structural divergence ---
//
// Every other watch here compares a number against a ceiling somebody chose.
// That works while you know what the number should be, and the hardest runaway
// to catch is the one that never reaches any ceiling you would have thought to
// set: a run that loops eight times instead of twice, finishes, costs four
// times what it should, and reports no error at all. Its step count is not high
// in absolute terms. It is high *for that definition*, and only the definition's
// own recent runs can say so.
//
// So this watch sets no threshold in steps. It reads the recent runs of each
// definition version, takes the middle of them as the usual shape, and reports
// the runs that sit further from that middle than a standardised bound allows.

/// The fewest completed runs one definition version needs before this watch
/// will judge anything against it.
///
/// Twelve. The median of twelve sits between the sixth and seventh value, so no
/// single run decides where the middle is; below about ten, one unusual run
/// moves the median itself and the baseline ends up describing the outlier
/// rather than the norm. Twelve also leaves the median absolute deviation room
/// to do the job it was chosen for: it tolerates up to half its sample being
/// outliers, which at twelve is six runaways in the cohort with the baseline
/// still standing.
///
/// It is a floor, not a target, and it is counted inside the monitor's own
/// window, so the two numbers have to be set together: a definition that runs a
/// few times an hour needs a window measured in hours to reach twelve. Until it
/// does, the watch says nothing rather than guessing, because "the usual number
/// of steps" is a claim about a sample and a sample of four is not one.
const MIN_BASELINE_RUNS: usize = 12;

/// The default bound, in modified z-scores, past which one run is divergent.
///
/// 3.5 is the value Iglewicz and Hoaglin publish with the modified z-score, and
/// the statistic is calibrated against it, so it is the number to move away
/// from rather than towards. On normally distributed data it is about one run
/// in two thousand. Step counts are not normally distributed, so read it as a
/// starting point to tune from what the watch reports rather than as a promised
/// false alarm rate.
const DEFAULT_Z_BOUND: f64 = 3.5;

/// The 0.75 quantile of the standard normal distribution, and the reason the
/// modified z-score carries a constant at all.
///
/// For normally distributed data the median absolute deviation converges to
/// 0.6745σ, not to σ, because half the population sits within 0.6745 standard
/// deviations of the middle. A raw `(x − median) / MAD` is therefore measured in
/// units of about two thirds of a standard deviation, and comparing it against
/// a bound like 3.5, which every reader takes to mean standard deviations,
/// would flag runs about a third closer in than intended. Multiplying by 0.6745
/// puts the ratio back into units of σ.
///
/// The same constant appears as its reciprocal, 1.4826, in the other common
/// spelling: `1.4826 × MAD` is the estimate of σ, and dividing by that is the
/// identical operation. This code divides, because the estimate of σ is worth
/// naming and is what the fallback below has to reproduce.
const NORMAL_QUARTILE: f64 = 0.6745;

/// √(π/2), which turns a mean absolute deviation into an estimate of σ.
///
/// Used only when the median absolute deviation is zero. See [`Baseline::of`].
const MEAN_DEVIATION_SCALE: f64 = 1.253_314;

/// How many runs one evaluation reads, at most.
///
/// A monitor tick must not walk an unbounded list. A busy project can hold more
/// runs in a five minute window than there is any point reading, and this walk
/// runs on the evaluation timer, so a slow one delays every monitor behind it.
/// The cap is applied newest first, so what it drops is the oldest end of the
/// window.
const DIVERGENCE_RUN_CAP: usize = 2_000;

/// How many runs one page of that walk asks for.
const DIVERGENCE_PAGE: usize = 500;

/// One run reduced to the five things this watch reads.
///
/// A type of its own rather than [`mcpd_runs::Run`], which has thirty fields. A
/// test that has to fill in the other twenty-five to state "this run took nine
/// steps" is a test nobody writes, and the arithmetic here is the part that has
/// to be exercised.
#[derive(Debug, Clone)]
struct RunShape {
    /// Which run, so the reading can name the one to open.
    run_id: String,
    /// Which definition it runs.
    definition_id: String,
    /// Which version of that definition, pinned at start.
    definition_version: u32,
    /// Steps completed so far.
    steps: f64,
    /// Whether it reached the end of its graph.
    ///
    /// Only a completed run joins a baseline, and only a completed run can be
    /// reported for taking too *few* steps. Ending short is what failing, being
    /// cancelled, timing out and not having finished yet all mean, and calling
    /// that structural divergence would report the failure a second time, in
    /// the one watch that exists to see what the error rate cannot.
    completed: bool,
}

impl RunShape {
    /// Read a run header.
    fn of(run: &mcpd_runs::Run) -> Self {
        Self {
            run_id: run.run_id.clone(),
            definition_id: run.definition_id.clone(),
            definition_version: run.definition_version,
            // Clamped rather than trusted. The column is signed because that
            // is the only integer SQLite has, and one negative value would drag
            // the median somewhere no run has ever been.
            steps: run.steps_used.max(0) as f64,
            completed: run.status == mcpd_runs::RunStatus::Succeeded,
        }
    }
}

/// The middle of a sorted sample.
///
/// An even sample takes the mean of the two middle values. That is the standard
/// definition and it matters here: step counts are integers, so picking one
/// side instead would make the usual shape jump by a whole step as the sample
/// size changes parity.
fn median_of_sorted(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n == 0 {
        // No caller reaches this, and returning zero rather than panicking
        // keeps an arithmetic slip out of the evaluation loop, which runs for
        // the life of the process.
        return 0.0;
    }
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

/// The usual shape of one definition version, and the scale its departures are
/// measured in.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Baseline {
    /// The typical step count.
    median: f64,
    /// An estimate of the standard deviation of the step count, or zero when
    /// the sample has no spread at all.
    sigma: f64,
    /// How many completed runs it was built from.
    sample: usize,
}

impl Baseline {
    /// Build a baseline from the step counts of completed runs.
    ///
    /// `None` when there are too few of them to describe anything.
    fn of(mut steps: Vec<f64>) -> Option<Self> {
        if steps.len() < MIN_BASELINE_RUNS {
            return None;
        }
        let sample = steps.len();
        steps.sort_by(f64::total_cmp);
        let median = median_of_sorted(&steps);

        let mut deviations: Vec<f64> = steps.iter().map(|s| (s - median).abs()).collect();
        deviations.sort_by(f64::total_cmp);
        let mad = median_of_sorted(&deviations);

        // A median absolute deviation of zero is the case that decides whether
        // this watch is usable at all, and it is not a corner case: it holds
        // whenever more than half the sample took the same number of steps,
        // which is the ordinary state of a definition with no loop and no
        // conditional branch. Every run that took one extra step then sits at
        // `(x − median) / 0`.
        //
        // Emitting infinity is not an answer. It cannot be compared usefully,
        // it prints as "inf" in a sentence somebody is meant to act on, and
        // serde writes a non-finite float to the wire as null, so a webhook
        // receiver would be told the value was missing rather than enormous.
        //
        // The answer is the fallback published with the statistic itself: the
        // mean absolute deviation, scaled by √(π/2) so that it estimates the
        // same σ the MAD estimates. It is defined whenever the sample is not
        // one value repeated, and it moves the reading from "infinitely
        // anomalous" to a number with a size. It gives up the MAD's tolerance
        // of outliers, which is exactly why it is second choice: a few
        // runaways inside the baseline will inflate it and hide the next one.
        let sigma = if mad > 0.0 {
            mad / NORMAL_QUARTILE
        } else {
            let mean_deviation = deviations.iter().sum::<f64>() / sample as f64;
            mean_deviation * MEAN_DEVIATION_SCALE
        };

        Some(Self { median, sigma, sample })
    }

    /// How far one run sits from the usual shape.
    fn deviation(&self, steps: f64) -> Deviation {
        let from_median = steps - self.median;
        if self.sigma > 0.0 {
            return Deviation::Score(from_median / self.sigma);
        }
        // Both estimators are zero, which happens only when every completed run
        // in the baseline took exactly the same number of steps. There is no
        // ratio to report: the sample has a width of zero, so a run that
        // differs at all differs by an amount the sample has never seen.
        //
        // Saying that in steps is the honest answer and it is also the more
        // useful one. "Every one of the last twenty runs took 12 steps and this
        // one took 400" needs no statistics to read, and it does not pretend to
        // a precision the sample cannot support.
        if from_median == 0.0 {
            Deviation::Score(0.0)
        } else {
            Deviation::Unprecedented { from_median }
        }
    }
}

/// How far one run sits from its cohort's usual shape.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Deviation {
    /// A modified z-score, in units of one standard deviation of the cohort.
    Score(f64),
    /// The cohort has never varied, and this run departs from it. Carries the
    /// departure in steps, signed, because there is no ratio to carry.
    Unprecedented {
        /// Steps above the usual count, or below it when negative.
        from_median: f64,
    },
}

impl Deviation {
    /// Whether this counts as divergence at `bound`.
    ///
    /// `low_tail` says whether taking fewer steps than usual is evidence of
    /// anything for this run. It is not, for one that failed, was cancelled,
    /// timed out or has not finished: all four end short by definition.
    ///
    /// `bound` does not apply to an unprecedented departure, because there is
    /// no score to hold against it. Raising the bound widens what counts as
    /// ordinary variation, and a cohort with no variation at all has none to
    /// widen: every departure from it is one the sample has never seen. Say so
    /// wherever this is documented, so that raising the bound and still being
    /// told is not a surprise.
    fn diverged(self, bound: f64, low_tail: bool) -> bool {
        match self {
            Self::Score(z) => z > bound || (low_tail && z < -bound),
            Self::Unprecedented { from_median } => from_median > 0.0 || low_tail,
        }
    }

    /// The sort key that puts the run worth opening first.
    fn severity(self) -> (u8, f64) {
        match self {
            Self::Score(z) => (0, z.abs()),
            // A cohort that has never varied at all is the strongest evidence
            // available, so these rank above any finite score.
            Self::Unprecedented { from_median } => (1, from_median.abs()),
        }
    }
}

/// One run the watch is reporting, with enough context to name it in a sentence.
#[derive(Debug, Clone)]
struct Diverged {
    /// The definition version, as `id vN`.
    cohort: String,
    /// Which run.
    run_id: String,
    /// What it took.
    steps: f64,
    /// What its cohort usually takes.
    median: f64,
    /// How many completed runs that usual count rests on.
    sample: usize,
    /// How far out it sits.
    deviation: Deviation,
}

impl Diverged {
    /// One clause of the reading's detail.
    fn phrase(&self) -> String {
        match self.deviation {
            Deviation::Score(z) => format!(
                "{} run {} took {:.0} steps against a usual {:.0} (z {z:.1})",
                self.cohort, self.run_id, self.steps, self.median
            ),
            Deviation::Unprecedented { .. } => format!(
                "{} run {} took {:.0} steps where every one of the last {} completed runs took {:.0}",
                self.cohort, self.run_id, self.steps, self.sample, self.median
            ),
        }
    }
}

/// What one pass over the window found.
#[derive(Debug, Clone, Default)]
struct Judgement {
    /// Runs scored against a baseline.
    scored: u64,
    /// Definition versions that had a baseline to score against.
    judged: usize,
    /// The most completed runs any one definition version had, whether or not
    /// that was enough. It tells an operator how far off a baseline is, rather
    /// than only that there is not one yet.
    best_sample: usize,
    /// The runs that diverged, worst first.
    diverged: Vec<Diverged>,
}

/// Score every run in the window against the usual shape of its own definition
/// version.
///
/// Separated from the fetch so that the arithmetic can be tested against runs
/// written by hand, rather than only against whatever a seeded store happens to
/// produce.
fn judge(shapes: &[RunShape], bound: f64) -> Judgement {
    // Cohorts are keyed by definition *version*, not by definition. A new
    // version is a deliberate change of shape, so a run of v3 is not anomalous
    // for taking a different number of steps from a run of v2, and mixing the
    // two makes a two-humped sample whose median describes neither. The cost is
    // that a freshly published version reports nothing until it has a baseline
    // of its own, and that is the right way round: better to say nothing about
    // a version nobody has run than to call every run of it divergent on the
    // afternoon it ships.
    //
    // A BTreeMap rather than a HashMap, so two evaluations of one window report
    // the same cohorts in the same order.
    let mut cohorts: std::collections::BTreeMap<(&str, u32), Vec<&RunShape>> =
        std::collections::BTreeMap::new();
    for shape in shapes {
        cohorts
            .entry((shape.definition_id.as_str(), shape.definition_version))
            .or_default()
            .push(shape);
    }

    let mut found = Judgement::default();
    for ((id, version), runs) in cohorts {
        let completed: Vec<f64> = runs.iter().filter(|r| r.completed).map(|r| r.steps).collect();
        found.best_sample = found.best_sample.max(completed.len());
        let Some(baseline) = Baseline::of(completed) else { continue };
        found.judged += 1;

        for run in runs {
            found.scored += 1;
            let deviation = baseline.deviation(run.steps);
            if deviation.diverged(bound, run.completed) {
                found.diverged.push(Diverged {
                    cohort: format!("{id} v{version}"),
                    run_id: run.run_id.clone(),
                    steps: run.steps,
                    median: baseline.median,
                    sample: baseline.sample,
                    deviation,
                });
            }
        }
    }

    found.diverged.sort_by(|a, b| {
        b.deviation
            .severity()
            .partial_cmp(&a.deviation.severity())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    found
}

/// Turn one pass into the reading the latching rule folds in.
fn divergence_reading(monitor: &Monitor, shapes: &[RunShape], now: OffsetDateTime) -> Reading {
    let bound = monitor.z_bound;
    let found = judge(shapes, bound);

    let thin = |detail: String| Reading {
        monitor_id: monitor.id.clone(),
        at: now,
        value: None,
        samples: shapes.len() as u64,
        condition: Condition::Insufficient,
        detail,
    };

    if shapes.is_empty() {
        return thin(format!("no run moved in the last {}s", monitor.window_seconds));
    }
    if found.judged == 0 {
        // Deliberately `Insufficient` and not `Nominal`. A window with no
        // baseline in it has not found the definition healthy, it has declined
        // to look, and reporting "all clear" would clear a standing alert on
        // the strength of having measured nothing.
        return thin(format!(
            "no definition version in the last {}s has the {MIN_BASELINE_RUNS} completed run(s) \
             a baseline needs; the most any has is {}",
            monitor.window_seconds, found.best_sample
        ));
    }

    let count = found.diverged.len();
    let breached = match monitor.compare {
        Compare::Above => count as f64 > monitor.threshold,
        Compare::Below => (count as f64) < monitor.threshold,
    };

    let mut detail = format!(
        "{count} of {} run(s) across {} definition version(s) diverged past a modified z of \
         {bound}",
        found.scored, found.judged
    );
    for d in found.diverged.iter().take(3) {
        detail.push_str("; ");
        detail.push_str(&d.phrase());
    }
    if count > 3 {
        detail.push_str(&format!("; and {} more", count - 3));
    }

    Reading {
        monitor_id: monitor.id.clone(),
        at: now,
        value: Some(count as f64),
        samples: found.scored,
        condition: if breached { Condition::Tripped } else { Condition::Nominal },
        detail,
    }
}

/// Carry a run store failure in the error type the evaluation loop handles.
///
/// It has to stay an error and must never become a reading. The loop treats a
/// storage failure as no reading at all, and folding it in as `Insufficient`
/// would make it indistinguishable from a quiet window, which is the one thing
/// that must never clear a standing alert.
fn run_store_error(e: &mcpd_runs::Error) -> mcpd_store::Error {
    mcpd_store::Error::Backend(format!("run store: {e}"))
}

/// The runs in one window that strayed from the shape their own definition
/// version usually takes.
///
/// The same statistic [`evaluate_divergence`] publishes, read for a different
/// purpose: the watch turns it into an alert, and the derived `outcome` aspect
/// turns it into a sentence about one run. Both call [`judge`], so a run that
/// trips the watch and a run the themes screen calls unusual are the same runs.
/// A second copy of the arithmetic would be two answers to one question.
///
/// Empty rather than an error whenever there is nothing to say: a daemon that
/// serves no durable runs, a window with no baseline in it, or a storage error.
/// The caller is writing one clause of one sentence, and a pass that failed
/// because a cause it could not read is worse than a sentence that leaves the
/// clause out.
pub(crate) async fn diverged_in_window(
    state: &Arc<AppState>,
    project: &str,
    from: OffsetDateTime,
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    let Some(runs) = state.runs.as_ref() else { return out };

    let mut shapes: Vec<RunShape> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let filter = mcpd_runs::RunFilter { project: project.to_owned(), ..Default::default() };
        let Ok(page) = runs.list_runs(filter, DIVERGENCE_PAGE, cursor).await else { return out };
        let mut past_window = false;
        for run in &page.runs {
            if run.updated_at < from {
                past_window = true;
                break;
            }
            shapes.push(RunShape::of(run));
        }
        if past_window || shapes.len() >= DIVERGENCE_RUN_CAP {
            break;
        }
        match page.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    shapes.truncate(DIVERGENCE_RUN_CAP);

    for d in judge(&shapes, DEFAULT_Z_BOUND).diverged {
        out.insert(d.run_id);
    }
    out
}

/// Evaluate the structural divergence watch.
///
/// Reads the durable run store rather than the trace projection, because the
/// question is about a definition's shape and only the run header knows which
/// definition version a run pinned and how many steps it has taken.
///
/// # Errors
/// Returns a storage error if the run store cannot be read.
async fn evaluate_divergence(
    state: &Arc<AppState>,
    monitor: &Monitor,
    now: OffsetDateTime,
) -> Result<Reading, mcpd_store::Error> {
    let Some(runs) = state.runs.as_ref() else {
        return Ok(Reading {
            monitor_id: monitor.id.clone(),
            at: now,
            value: None,
            samples: 0,
            condition: Condition::Insufficient,
            detail: "this daemon serves no durable runs, so there are no step counts to compare"
                .to_owned(),
        });
    };

    let from = now - Duration::seconds(i64::from(monitor.window_seconds));
    let mut shapes: Vec<RunShape> = Vec::new();
    let mut cursor: Option<String> = None;

    loop {
        let filter =
            mcpd_runs::RunFilter { project: monitor.project.clone(), ..Default::default() };
        let page = runs
            .list_runs(filter, DIVERGENCE_PAGE, cursor)
            .await
            .map_err(|e| run_store_error(&e))?;

        // The page is ordered by `updated_at` descending, so the first run
        // older than the window is the end of the walk. `updated_at` rather
        // than `created_at` on purpose: a run that is still moving belongs to
        // the window it is moving in, which is what lets this watch see a
        // runaway while it is still running instead of after it has stopped.
        let mut past_window = false;
        for run in &page.runs {
            if run.updated_at < from {
                past_window = true;
                break;
            }
            shapes.push(RunShape::of(run));
        }

        if past_window || shapes.len() >= DIVERGENCE_RUN_CAP {
            break;
        }
        match page.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    if shapes.len() >= DIVERGENCE_RUN_CAP {
        // Not a failure, and not worth a warning, but worth being able to find:
        // the baseline was built from the newest runs of a window that held
        // more than one evaluation reads.
        tracing::debug!(
            monitor = %monitor.id,
            cap = DIVERGENCE_RUN_CAP,
            "the divergence window held more runs than one evaluation reads"
        );
    }
    shapes.truncate(DIVERGENCE_RUN_CAP);

    Ok(divergence_reading(monitor, &shapes, now))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor() -> Monitor {
        Monitor {
            id: "m1".into(),
            name: "error rate".into(),
            project: "default".into(),
            watch: Watch::ErrorRate,
            scorer_id: None,
            compare: Compare::Above,
            threshold: 0.05,
            window_seconds: 300,
            min_samples: 20,
            z_bound: DEFAULT_Z_BOUND,
            webhook: None,
            enabled: true,
        }
    }

    fn reading(condition: Condition) -> Reading {
        Reading {
            monitor_id: "m1".into(),
            at: OffsetDateTime::UNIX_EPOCH,
            value: Some(0.5),
            samples: 100,
            condition,
            detail: String::new(),
        }
    }

    fn state() -> MonitorState {
        MonitorState {
            monitor: monitor(),
            condition: Condition::Nominal,
            last: None,
            tripped_at: None,
        }
    }

    #[test]
    fn it_notifies_on_the_transition_and_not_afterwards() {
        let mut s = state();
        assert!(s.apply(reading(Condition::Tripped)), "the first trip notifies");
        assert!(!s.apply(reading(Condition::Tripped)), "a standing alert does not re-notify");
        assert!(!s.apply(reading(Condition::Tripped)));
        assert!(s.apply(reading(Condition::Nominal)), "recovery notifies once");
        assert!(!s.apply(reading(Condition::Nominal)));
    }

    #[test]
    fn it_latches_until_it_actually_recovers() {
        let mut s = state();
        s.apply(reading(Condition::Tripped));
        assert_eq!(s.condition, Condition::Tripped);
        assert!(s.tripped_at.is_some());
        s.acknowledge();
        assert_eq!(s.condition, Condition::Acknowledged);
        // Still breaching: stays acknowledged rather than re-tripping.
        s.apply(reading(Condition::Tripped));
        assert_eq!(s.condition, Condition::Acknowledged);
        // Genuine recovery clears it.
        s.apply(reading(Condition::Nominal));
        assert_eq!(s.condition, Condition::Nominal);
        assert!(s.tripped_at.is_none());
    }

    #[test]
    fn silence_does_not_clear_a_standing_alert() {
        // A pipeline that stops emitting is not a pipeline that recovered, and
        // clearing on no-data is how an outage goes unnoticed.
        let mut s = state();
        s.apply(reading(Condition::Tripped));
        s.apply(reading(Condition::Insufficient));
        assert_eq!(s.condition, Condition::Tripped);
        assert!(s.tripped_at.is_some());
    }

    #[test]
    fn a_thin_window_is_never_judged() {
        let m = monitor();
        assert_eq!(m.min_samples, 20, "one bad run in a quiet hour is not a 100% error rate");
    }

    #[test]
    fn acknowledging_a_healthy_monitor_does_nothing() {
        let mut s = state();
        s.acknowledge();
        assert_eq!(s.condition, Condition::Nominal);
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;

    /// Every timestamp mcpd emits is RFC 3339. `time` defaults to a component
    /// array, which shipped once and which no SDK's date parser reads.
    #[test]
    fn monitor_timestamps_are_rfc3339() {
        let mut state = MonitorState::new(Monitor {
            id: "m".to_owned(),
            name: "n".to_owned(),
            project: "default".to_owned(),
            watch: Watch::ErrorRate,
            scorer_id: None,
            compare: Compare::Above,
            threshold: 0.5,
            window_seconds: 60,
            min_samples: 1,
            z_bound: DEFAULT_Z_BOUND,
            webhook: None,
            enabled: true,
        });
        let at = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
        state.apply(Reading {
            monitor_id: "m".to_owned(),
            at,
            value: Some(0.9),
            samples: 10,
            condition: Condition::Tripped,
            detail: "over".to_owned(),
        });

        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"tripped_at\":\"2027-01-15"), "tripped_at was {json}");
        assert!(json.contains("\"at\":\"2027-01-15"), "reading.at was {json}");
    }
}

#[cfg(test)]
mod divergence_tests {
    use super::*;

    /// A monitor watching structural divergence, with a threshold of zero, so
    /// any one divergent run trips it. That is the setting the documentation
    /// recommends; a project that tolerates the odd outlier raises it.
    fn divergence() -> Monitor {
        Monitor {
            id: "d1".to_owned(),
            name: "checkout shape".to_owned(),
            project: "default".to_owned(),
            watch: Watch::StepCountDivergence,
            scorer_id: None,
            compare: Compare::Above,
            threshold: 0.0,
            window_seconds: 900,
            min_samples: 20,
            z_bound: DEFAULT_Z_BOUND,
            webhook: None,
            enabled: true,
        }
    }

    fn at() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("a valid instant")
    }

    /// One run of `checkout` v1 that completed.
    fn run(id: &str, steps: i64) -> RunShape {
        RunShape {
            run_id: id.to_owned(),
            definition_id: "checkout".to_owned(),
            definition_version: 1,
            steps: steps as f64,
            completed: true,
        }
    }

    /// `count` completed runs whose step counts step around `steps`, so the
    /// cohort has a real spread rather than a contrived one.
    ///
    /// The pattern is deliberate and worth stating, because every expected
    /// number below is checked by hand against it. For `spread(12, 10)` the
    /// counts are 9, 10, 11 repeating: the median is 10 and the absolute
    /// deviations are 1, 0, 1 repeating, whose median is 1. So MAD = 1 and the
    /// estimate of sigma is 1 / 0.6745, about 1.4826.
    fn spread(count: usize, steps: i64) -> Vec<RunShape> {
        (0..count).map(|i| run(&format!("r{i}"), steps + (i as i64 % 3) - 1)).collect()
    }

    #[test]
    fn a_run_four_times_its_usual_length_is_flagged() {
        // Twelve ordinary runs around ten steps, then one that looped.
        let mut shapes = spread(12, 10);
        shapes.push(run("runaway", 40));

        let reading = divergence_reading(&divergence(), &shapes, at());

        assert_eq!(reading.condition, Condition::Tripped, "{}", reading.detail);
        assert_eq!(reading.value, Some(1.0), "exactly one run diverged");
        assert_eq!(reading.samples, 13, "and all thirteen were scored");
        assert!(reading.detail.contains("runaway"), "the detail names it: {}", reading.detail);
        assert!(reading.detail.contains("40 steps"), "{}", reading.detail);

        // The number itself, checked by hand: MAD is 1, so sigma is 1/0.6745,
        // and (40 - 10) / (1 / 0.6745) is 30 * 0.6745, about 20.2. Far past
        // 3.5, and far below the 40-step ceiling anyone would have set.
        let baseline = Baseline::of(spread(12, 10).iter().map(|r| r.steps).collect())
            .expect("twelve runs is a baseline");
        assert_eq!(baseline.median, 10.0);
        match baseline.deviation(40.0) {
            Deviation::Score(z) => assert!((z - 20.235).abs() < 0.01, "z was {z}"),
            other => panic!("expected a finite score, got {other:?}"),
        }
    }

    #[test]
    fn a_normal_run_is_not_flagged() {
        // The same twelve, and a thirteenth one step outside the usual range.
        // One step is a real difference and it is nowhere near 3.5 sigma, which
        // is the whole point: this watch must not report ordinary variation.
        let mut shapes = spread(12, 10);
        shapes.push(run("ordinary", 12));

        let reading = divergence_reading(&divergence(), &shapes, at());

        assert_eq!(reading.condition, Condition::Nominal, "{}", reading.detail);
        assert_eq!(reading.value, Some(0.0));
        assert!(reading.detail.starts_with("0 of 13 run(s)"), "{}", reading.detail);
    }

    #[test]
    fn a_cohort_that_has_never_varied_never_divides_by_zero() {
        // Twelve runs of exactly nine steps. The median absolute deviation is
        // zero, and so is the mean absolute deviation, so there is no scale to
        // divide by at all.
        let identical: Vec<RunShape> = (0..12).map(|i| run(&format!("r{i}"), 9)).collect();
        let baseline = Baseline::of(identical.iter().map(|r| r.steps).collect())
            .expect("twelve runs is a baseline");
        assert_eq!(baseline.sigma, 0.0, "a sample of one repeated value has no spread");

        // A run that matches the cohort is not anomalous, and must not come
        // back as a NaN from 0/0.
        assert_eq!(baseline.deviation(9.0), Deviation::Score(0.0));

        // A run that does not match is reported without a ratio.
        match baseline.deviation(400.0) {
            Deviation::Unprecedented { from_median } => assert_eq!(from_median, 391.0),
            other => panic!("expected an unprecedented departure, got {other:?}"),
        }

        // End to end. The runaway here is still in flight, which is what makes
        // the cohort's completed sample uniform: a run that finished long would
        // have joined its own baseline and given the mean absolute deviation
        // something to work with. Scoring a run against a sample it belongs to
        // is the conventional form of the statistic and it is why the median
        // absolute deviation was chosen, since it tolerates the runaway sitting
        // inside its own baseline. This case is the one where that leaves
        // nothing to divide by.
        let mut shapes = identical;
        shapes.push(RunShape {
            run_id: "runaway".to_owned(),
            definition_id: "checkout".to_owned(),
            definition_version: 1,
            steps: 400.0,
            completed: false,
        });
        let reading = divergence_reading(&divergence(), &shapes, at());

        assert_eq!(reading.condition, Condition::Tripped, "{}", reading.detail);
        assert_eq!(reading.value, Some(1.0));
        // Nothing infinite reaches the wire, and nothing infinite reaches the
        // sentence somebody has to read.
        let value = reading.value.expect("a value");
        assert!(value.is_finite(), "the value was {value}");
        assert!(!reading.detail.contains("inf"), "{}", reading.detail);
        assert!(!reading.detail.contains("NaN"), "{}", reading.detail);
        assert!(
            reading.detail.contains("every one of the last 12 completed runs took 9"),
            "the detail says it in steps, not in a score: {}",
            reading.detail
        );
    }

    #[test]
    fn the_fallback_gives_a_finite_score_when_the_mad_alone_is_zero() {
        // Eleven runs of nine steps and one of forty-five. More than half the
        // sample is identical, so the median absolute deviation is zero, but
        // the sample is not one value repeated, so the mean absolute deviation
        // is not.
        let mut steps: Vec<f64> = vec![9.0; 11];
        steps.push(45.0);
        let baseline = Baseline::of(steps).expect("twelve runs is a baseline");
        assert_eq!(baseline.median, 9.0);

        // By hand: the deviations are eleven zeros and one 36, so the mean
        // absolute deviation is 3, and sigma is 3 * 1.253314, about 3.76.
        assert!((baseline.sigma - 3.759_942).abs() < 0.001, "sigma was {}", baseline.sigma);
        match baseline.deviation(45.0) {
            Deviation::Score(z) => {
                assert!(z.is_finite(), "the fallback exists to keep this finite");
                assert!((z - 9.575).abs() < 0.01, "z was {z}");
            }
            other => panic!("expected a finite score, got {other:?}"),
        }
    }

    #[test]
    fn too_small_a_sample_declines_to_judge() {
        // Eleven runs, one short of the floor, and one of them plainly a
        // runaway. The watch says nothing, because eleven runs cannot tell a
        // runaway apart from a definition that takes forty steps sometimes.
        let mut shapes: Vec<RunShape> = (0..10).map(|i| run(&format!("r{i}"), 10)).collect();
        shapes.push(run("runaway", 400));
        assert_eq!(shapes.len(), 11);

        let reading = divergence_reading(&divergence(), &shapes, at());

        assert_eq!(reading.condition, Condition::Insufficient, "{}", reading.detail);
        assert_eq!(reading.value, None, "there is no measurement to report");
        assert!(
            reading.detail.contains("the most any has is 11"),
            "the detail says how far off a baseline is: {}",
            reading.detail
        );

        // And the twelfth run is enough.
        shapes.push(run("r10", 10));
        let reading = divergence_reading(&divergence(), &shapes, at());
        assert_eq!(reading.condition, Condition::Tripped, "{}", reading.detail);
    }

    #[test]
    fn a_thin_cohort_is_insufficient_and_not_nominal() {
        // The distinction the latching rule rests on. `Insufficient` leaves a
        // standing alert where it was; `Nominal` clears it. A window with no
        // baseline in it has not found anything healthy.
        let reading = divergence_reading(&divergence(), &spread(4, 10), at());
        assert_eq!(reading.condition, Condition::Insufficient);

        let mut state = MonitorState::new(divergence());
        state.apply(Reading {
            monitor_id: "d1".to_owned(),
            at: at(),
            value: Some(1.0),
            samples: 13,
            condition: Condition::Tripped,
            detail: String::new(),
        });
        state.apply(reading);
        assert_eq!(state.condition, Condition::Tripped, "a thin window must not clear it");
    }

    #[test]
    fn each_definition_version_is_its_own_cohort() {
        // v1 usually takes ten steps and v2 usually takes forty, because v2
        // added work on purpose. Pooled, the median would sit between them and
        // every run of both would look divergent.
        let mut shapes = spread(12, 10);
        shapes.extend((0..12).map(|i| RunShape {
            run_id: format!("v2-{i}"),
            definition_id: "checkout".to_owned(),
            definition_version: 2,
            steps: 40.0 + (i as f64 % 3.0) - 1.0,
            completed: true,
        }));

        let reading = divergence_reading(&divergence(), &shapes, at());
        assert_eq!(reading.condition, Condition::Nominal, "{}", reading.detail);
        assert!(
            reading.detail.contains("2 definition version(s)"),
            "each version was judged on its own: {}",
            reading.detail
        );
    }

    #[test]
    fn a_failed_run_is_not_reported_for_ending_short() {
        // A run that failed on its second step has diverged from the usual ten
        // by any measure, and saying so would be reporting the failure twice in
        // the one watch that exists to see what the error rate cannot.
        let mut shapes = spread(12, 10);
        shapes.push(RunShape {
            run_id: "failed-early".to_owned(),
            definition_id: "checkout".to_owned(),
            definition_version: 1,
            steps: 2.0,
            completed: false,
        });

        let reading = divergence_reading(&divergence(), &shapes, at());
        assert_eq!(reading.condition, Condition::Nominal, "{}", reading.detail);

        // But the same run going the other way is exactly what this watch is
        // for: a run still in flight that has already gone long.
        shapes.push(RunShape {
            run_id: "still-looping".to_owned(),
            definition_id: "checkout".to_owned(),
            definition_version: 1,
            steps: 90.0,
            completed: false,
        });
        let reading = divergence_reading(&divergence(), &shapes, at());
        assert_eq!(reading.condition, Condition::Tripped, "{}", reading.detail);
        assert!(reading.detail.contains("still-looping"), "{}", reading.detail);
    }

    #[test]
    fn a_completed_run_that_ends_short_is_reported() {
        // The other tail, for a run that did finish. A definition that usually
        // takes ten steps returning after two has taken a branch it does not
        // normally take, and no error rate will ever mention it.
        let mut shapes = spread(12, 10);
        shapes.push(run("short-circuit", 2));

        let reading = divergence_reading(&divergence(), &shapes, at());
        assert_eq!(reading.condition, Condition::Tripped, "{}", reading.detail);
        assert!(reading.detail.contains("short-circuit"), "{}", reading.detail);
    }

    #[test]
    fn runs_that_did_not_complete_stay_out_of_the_baseline() {
        // Twelve completed runs around ten steps, twelve that have barely
        // started, and one that is looping. A run in flight has taken some of
        // its steps and not the rest, so its count says how far along it is and
        // nothing about the definition's shape. Letting the twelve fresh ones
        // into the baseline drags the usual count down to nine and inflates the
        // spread enough that the run at forty stops looking unusual at all.
        let mut shapes = spread(12, 10);
        shapes.extend((0..12).map(|i| RunShape {
            run_id: format!("fresh-{i}"),
            definition_id: "checkout".to_owned(),
            definition_version: 1,
            steps: 1.0,
            completed: false,
        }));
        shapes.push(RunShape {
            run_id: "looping".to_owned(),
            definition_id: "checkout".to_owned(),
            definition_version: 1,
            steps: 40.0,
            completed: false,
        });

        let reading = divergence_reading(&divergence(), &shapes, at());
        assert_eq!(reading.condition, Condition::Tripped, "{}", reading.detail);
        assert_eq!(
            reading.value,
            Some(1.0),
            "the fresh runs are not divergent, {}",
            reading.detail
        );
        assert_eq!(reading.samples, 25, "but every one of them was scored");
        assert!(
            reading.detail.contains("took 40 steps against a usual 10"),
            "the usual count comes from the completed runs alone: {}",
            reading.detail
        );
    }

    #[test]
    fn the_threshold_counts_runs_and_not_steps() {
        // Two runaways. A monitor that tolerates one does not trip; one that
        // tolerates none does. This is what `threshold` means for this watch,
        // and it is why the bound lives on its own field: 3.5 is about how far
        // out one run is, and the threshold is about how many.
        let mut shapes = spread(12, 10);
        shapes.push(run("runaway-a", 40));
        shapes.push(run("runaway-b", 44));

        let mut m = divergence();
        m.threshold = 2.0;
        assert_eq!(divergence_reading(&m, &shapes, at()).condition, Condition::Nominal);

        m.threshold = 1.0;
        let reading = divergence_reading(&m, &shapes, at());
        assert_eq!(reading.condition, Condition::Tripped, "{}", reading.detail);
        assert_eq!(reading.value, Some(2.0));
    }

    #[test]
    fn raising_the_bound_lets_a_run_through() {
        let mut shapes = spread(12, 10);
        shapes.push(run("runaway", 40));

        let mut m = divergence();
        assert_eq!(divergence_reading(&m, &shapes, at()).condition, Condition::Tripped);
        // The score is about 20.2, so a bound of 25 covers it.
        m.z_bound = 25.0;
        assert_eq!(divergence_reading(&m, &shapes, at()).condition, Condition::Nominal);
    }

    /// The block on the alerting page, parsed by the code it configures.
    ///
    /// A documented configuration that the daemon refuses to start on is worse
    /// than none, because the reader has no way to tell which of the two is
    /// wrong. If this block changes on the page, change it here.
    #[test]
    fn the_documented_configuration_parses() {
        let written = serde_json::json!({
            "id": "checkout-shape",
            "name": "A checkout run took an unusual number of steps",
            "project": "support-desk",
            "watch": "step_count_divergence",
            "compare": "above",
            "threshold": 0,
            "z_bound": 3.5,
            "window_seconds": 3600,
            "webhook": "https://hooks.example.com/mcpd"
        });
        let m: Monitor = serde_json::from_value(written).expect("the documented monitor parses");
        assert_eq!(m.watch, Watch::StepCountDivergence);
        assert_eq!(m.compare, Compare::Above);
        assert!((m.z_bound - 3.5).abs() < f64::EPSILON);
        assert!((m.threshold - 0.0).abs() < f64::EPSILON);
        assert!(m.enabled, "a monitor is armed unless it says otherwise");

        // And the bound is optional, because it has a default worth having.
        let terse: Monitor = serde_json::from_value(serde_json::json!({
            "id": "d",
            "name": "n",
            "project": "p",
            "watch": "step_count_divergence",
            "compare": "above",
            "threshold": 0
        }))
        .expect("a monitor without a bound parses");
        assert!((terse.z_bound - DEFAULT_Z_BOUND).abs() < f64::EPSILON);
    }

    #[test]
    fn an_empty_window_says_so() {
        let reading = divergence_reading(&divergence(), &[], at());
        assert_eq!(reading.condition, Condition::Insufficient);
        assert_eq!(reading.detail, "no run moved in the last 900s");
    }

    #[test]
    fn the_default_bound_is_the_published_one() {
        assert!(
            (default_z_bound() - 3.5).abs() < f64::EPSILON,
            "3.5 is the value the modified z-score is published with; changing it \
             is a decision, not a tidy-up"
        );
        assert_eq!(MIN_BASELINE_RUNS, 12);
    }

    #[test]
    fn an_even_sample_takes_the_middle_of_the_two_middle_values() {
        assert_eq!(median_of_sorted(&[1.0, 2.0, 3.0, 4.0]), 2.5);
        assert_eq!(median_of_sorted(&[1.0, 2.0, 3.0]), 2.0);
        assert_eq!(median_of_sorted(&[]), 0.0, "and an empty sample does not panic");
    }

    #[test]
    fn the_scale_constant_puts_the_score_in_standard_deviations() {
        // The reason 0.6745 is in the formula. For a sample whose median
        // absolute deviation is 1, the estimate of sigma is 1.4826, so a run
        // sitting 1.4826 steps out scores exactly 1.0. Without the constant it
        // would score 1.4826, and a bound of 3.5 would be flagging runs at
        // about 2.4 sigma instead.
        let baseline =
            Baseline::of(spread(12, 10).iter().map(|r| r.steps).collect()).expect("a baseline");
        assert!((baseline.sigma - 1.482_580).abs() < 0.001, "sigma was {}", baseline.sigma);
        match baseline.deviation(10.0 + baseline.sigma) {
            Deviation::Score(z) => assert!((z - 1.0).abs() < 1e-9, "z was {z}"),
            other => panic!("expected a score, got {other:?}"),
        }
    }

    #[test]
    fn the_worst_run_is_named_first() {
        // The detail carries three examples at most, so which three matters.
        let mut shapes = spread(12, 10);
        shapes.push(run("mild", 20));
        shapes.push(run("severe", 200));
        let reading = divergence_reading(&divergence(), &shapes, at());
        let severe = reading.detail.find("severe").expect("named");
        let mild = reading.detail.find("mild").expect("named");
        assert!(severe < mild, "worst first: {}", reading.detail);
    }
}

// ------------------------------------------------------- run repetition ---
//
// The watch above asks how far a run strays from the shape its own definition
// usually takes, and it needs a cohort of recent runs before it can answer.
// This one asks a narrower question that needs no cohort at all: how many times
// did a run go round the same node?
//
// It sees two faults the error rate cannot. A loop that keeps re-entering the
// same node succeeds, and a retry storm that gets there in the end is a
// success; both hold the error rate at zero while the run does its work over
// and over. It is also the reading a step count cannot give on its own: a run
// of forty steps that visited forty nodes once is a long definition, and a run
// of forty steps that visited one node forty times is stuck.
//
// Unlike divergence, this one has a threshold in the units of the thing it
// measures. One attempt at the busiest node means no node ran twice, whatever
// the definition, so the number reads the same for every project and an
// operator can name a ceiling for it. That makes this an ordinary threshold
// watch, and only its reading discipline is borrowed from the watch above.

/// How many runs one evaluation reads, at most.
///
/// The divergence cap, held equal on purpose: two watches configured with the
/// same window should read the same set of runs, so that an operator comparing
/// their readings is comparing like with like. The reasoning behind the number
/// is the same one, that a monitor tick must not walk an unbounded list and
/// this walk runs on the evaluation timer, where a slow pass delays every
/// monitor behind it. Applied newest first, so what it drops is the oldest end
/// of the window.
///
/// This watch reads more per run than divergence does, because it groups the
/// journal as well as the header. The grouping happens in the database and
/// comes back as a row per node rather than a row per attempt, so the cost per
/// run tracks how many nodes a definition has and not how far one of them
/// fanned out.
const REPETITION_RUN_CAP: usize = 2_000;

/// How many runs one page of that walk asks for, and how many the attempt
/// counts are then read for at a time.
///
/// The run store is one connection behind one mutex, and every worker claiming
/// a step queues on it. Asking for the counts of the whole window in one call
/// would hold that mutex for the length of the window's journal; asking in
/// pages of this size hands it back four times over a full window, so a worker
/// waits for one page rather than for all of them.
const REPETITION_PAGE: usize = 500;

/// One run reduced to the one number this watch reads.
///
/// A type of its own, built by hand in the tests, so the arithmetic can be
/// exercised without a run store behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Repeated {
    /// Which run, so the reading can name the one to open.
    run_id: String,
    /// The definition version it runs, as `id vN`.
    ///
    /// The mean is over the window rather than per version, so this is not a
    /// cohort the way it is for divergence. It is in the sentence because
    /// "run r_9f2 went round eleven times" does not say what to open.
    cohort: String,
    /// The node it attempted most.
    node: String,
    /// How many attempts it made there. One means no node ran twice.
    ///
    /// An integer, and deliberately not a float. It is a count, the sort that
    /// orders the reading's examples is over it, and an integer ordering is
    /// exact, so nothing here has to reason about how two equal floats compare.
    attempts: u32,
}

impl Repeated {
    /// One clause of the reading's detail.
    fn phrase(&self) -> String {
        format!(
            "{} run {} attempted {} {} times",
            self.cohort, self.run_id, self.node, self.attempts
        )
    }
}

/// The busiest node of each run, from the per-node counts of many.
///
/// A `BTreeMap`, because what it decides reaches the sentence an operator
/// reads: two evaluations of one window have to name the same node.
fn peak_nodes(counts: &[mcpd_runs::NodeAttempts]) -> std::collections::BTreeMap<&str, (&str, u32)> {
    let mut peaks: std::collections::BTreeMap<&str, (&str, u32)> =
        std::collections::BTreeMap::new();
    for row in counts {
        let (node, attempts) = (row.node.as_str(), row.attempts);
        let better = match peaks.get(row.run_id.as_str()) {
            None => true,
            // A tie goes to the node that sorts first. Two nodes of one run can
            // hold the same count, and taking whichever the store happened to
            // list last would let the sentence change between two readings of
            // one unchanged window.
            Some(&(held_node, held)) => attempts > held || (attempts == held && node < held_node),
        };
        if better {
            peaks.insert(row.run_id.as_str(), (node, attempts));
        }
    }
    peaks
}

/// What one pass over the window found.
#[derive(Debug, Clone, PartialEq)]
struct Repetition {
    /// The mean, across the runs measured, of the attempts each made at its own
    /// busiest node.
    mean: f64,
    /// How many runs that mean rests on.
    runs: u64,
    /// The runs that repeated something, worst first.
    worst: Vec<Repeated>,
}

/// Reduce the runs of one window to the statistic and the runs worth naming.
///
/// Separated from the fetch so that the arithmetic can be tested against runs
/// written by hand, rather than only against whatever a seeded store produces.
fn measure(runs: &[Repeated]) -> Repetition {
    // Summed as a `u64` before the division. The mean is a float and has to be,
    // but the total is a count of attempts and a window of two thousand runs
    // whose worst node ran a thousand times is still six orders of magnitude
    // inside what an integer holds exactly.
    let total: u64 = runs.iter().map(|r| u64::from(r.attempts)).sum();
    let mean = if runs.is_empty() { 0.0 } else { total as f64 / runs.len() as f64 };

    // Only the runs that repeated something are named. A run whose busiest node
    // ran once is the healthy case and there is nothing in it to open, so
    // listing it would spend the detail's three examples on the wrong runs.
    let mut worst: Vec<Repeated> = runs.iter().filter(|r| r.attempts > 1).cloned().collect();
    // Worst first, and by run id inside a tie, so one window reads the same way
    // twice however the store ordered it.
    worst.sort_by(|a, b| b.attempts.cmp(&a.attempts).then_with(|| a.run_id.cmp(&b.run_id)));

    Repetition { mean, runs: runs.len() as u64, worst }
}

/// Turn one pass into the reading the latching rule folds in.
fn repetition_reading(monitor: &Monitor, runs: &[Repeated], now: OffsetDateTime) -> Reading {
    let thin = |samples: u64, detail: String| Reading {
        monitor_id: monitor.id.clone(),
        at: now,
        value: None,
        samples,
        condition: Condition::Insufficient,
        detail,
    };

    if runs.is_empty() {
        return thin(0, format!("no run moved in the last {}s", monitor.window_seconds));
    }

    let found = measure(runs);
    if found.runs < monitor.min_samples {
        // `Insufficient` and not `Nominal`, for the reason every other watch
        // has a thin-window gate: a handful of runs has not found the project
        // healthy, it has declined to judge it, and reporting all clear here
        // would clear a standing alert on the strength of three runs.
        return thin(
            found.runs,
            format!(
                "{} run(s) in the last {}s; needs at least {}",
                found.runs, monitor.window_seconds, monitor.min_samples
            ),
        );
    }

    let breached = match monitor.compare {
        Compare::Above => found.mean > monitor.threshold,
        Compare::Below => found.mean < monitor.threshold,
    };

    let mut detail = format!(
        "{} is {:.2} attempt(s) at the busiest node of a run, threshold {} {:.2}, over {} run(s)",
        monitor.name,
        found.mean,
        match monitor.compare {
            Compare::Above => "above",
            Compare::Below => "below",
        },
        monitor.threshold,
        found.runs,
    );
    for r in found.worst.iter().take(3) {
        detail.push_str("; ");
        detail.push_str(&r.phrase());
    }
    if found.worst.len() > 3 {
        detail.push_str(&format!("; and {} more", found.worst.len() - 3));
    }

    Reading {
        monitor_id: monitor.id.clone(),
        at: now,
        value: Some(found.mean),
        samples: found.runs,
        condition: if breached { Condition::Tripped } else { Condition::Nominal },
        detail,
    }
}

/// Evaluate the run repetition watch.
///
/// Reads the durable run store rather than the trace projection, because the
/// count of attempts at one node lives in the journal and nowhere else: a
/// retried step and a re-entered loop body both leave attempt rows, and neither
/// leaves anything in a trace that says which node it belongs to.
///
/// # Errors
/// Returns a storage error if the run store cannot be read.
async fn evaluate_repetition(
    state: &Arc<AppState>,
    monitor: &Monitor,
    now: OffsetDateTime,
) -> Result<Reading, mcpd_store::Error> {
    let Some(runs) = state.runs.as_ref() else {
        return Ok(Reading {
            monitor_id: monitor.id.clone(),
            at: now,
            value: None,
            samples: 0,
            condition: Condition::Insufficient,
            detail: "this daemon serves no durable runs, so there are no attempts to count"
                .to_owned(),
        });
    };

    let from = now - Duration::seconds(i64::from(monitor.window_seconds));
    // Newest first and in order, so truncating to the cap drops the oldest end
    // of the window rather than an arbitrary part of it.
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut cursor: Option<String> = None;

    loop {
        let filter =
            mcpd_runs::RunFilter { project: monitor.project.clone(), ..Default::default() };
        let page = runs
            .list_runs(filter, REPETITION_PAGE, cursor)
            .await
            .map_err(|e| run_store_error(&e))?;

        // The page is ordered by `updated_at` descending, so the first run older
        // than the window ends the walk. `updated_at` rather than `created_at`,
        // for the same reason the divergence walk uses it: a run that is still
        // moving belongs to the window it is moving in, which is what lets this
        // watch see a retry storm while it is still going.
        let mut past_window = false;
        for run in &page.runs {
            if run.updated_at < from {
                past_window = true;
                break;
            }
            headers.push((
                run.run_id.clone(),
                format!("{} v{}", run.definition_id, run.definition_version),
            ));
        }

        if past_window || headers.len() >= REPETITION_RUN_CAP {
            break;
        }
        match page.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    if headers.len() >= REPETITION_RUN_CAP {
        // Not a failure, and not worth a warning, but worth being able to find:
        // the mean was taken over the newest runs of a window that held more
        // than one evaluation reads.
        tracing::debug!(
            monitor = %monitor.id,
            cap = REPETITION_RUN_CAP,
            "the repetition window held more runs than one evaluation reads"
        );
    }
    headers.truncate(REPETITION_RUN_CAP);

    let ids: Vec<String> = headers.iter().map(|(id, _)| id.clone()).collect();
    let mut counts: Vec<mcpd_runs::NodeAttempts> = Vec::new();
    for batch in ids.chunks(REPETITION_PAGE) {
        counts.extend(runs.node_attempts(batch).await.map_err(|e| run_store_error(&e))?);
    }

    let peaks = peak_nodes(&counts);
    let measured: Vec<Repeated> = headers
        .iter()
        .filter_map(|(run_id, cohort)| {
            // A run with no attempt row at all is left out of the sample rather
            // than counted as zero. It has not repeated anything because it has
            // not run anything, and a zero would pull the mean below the one
            // that means "no node ran twice", which is the reading the whole
            // threshold is set against.
            let &(node, attempts) = peaks.get(run_id.as_str())?;
            Some(Repeated {
                run_id: run_id.clone(),
                cohort: cohort.clone(),
                node: node.to_owned(),
                attempts,
            })
        })
        .collect();

    Ok(repetition_reading(monitor, &measured, now))
}

#[cfg(test)]
mod repetition_tests {
    use super::*;

    /// A monitor watching run repetition, set where the documentation sets it:
    /// a mean of 1.2 attempts at the busiest node, which is a fifth of the
    /// window's runs going round twice.
    fn repetition() -> Monitor {
        Monitor {
            id: "p1".to_owned(),
            name: "checkout repetition".to_owned(),
            project: "default".to_owned(),
            watch: Watch::RunRepetition,
            scorer_id: None,
            compare: Compare::Above,
            threshold: 1.2,
            window_seconds: 900,
            min_samples: 20,
            z_bound: DEFAULT_Z_BOUND,
            webhook: None,
            enabled: true,
        }
    }

    fn at() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("a valid instant")
    }

    fn repeated(run_id: &str, node: &str, attempts: u32) -> Repeated {
        Repeated {
            run_id: run_id.to_owned(),
            cohort: "checkout v1".to_owned(),
            node: node.to_owned(),
            attempts,
        }
    }

    /// `count` runs in which nothing ran twice, which is the healthy window.
    fn quiet(count: usize) -> Vec<Repeated> {
        (0..count).map(|i| repeated(&format!("r{i}"), "work", 1)).collect()
    }

    #[test]
    fn a_healthy_window_reads_exactly_one() {
        // The number the whole threshold is set against. Every run that is
        // measured attempted at least one node at least once, so the reading
        // has a floor of 1, and 1 is the statement "no node ran twice".
        let reading = repetition_reading(&repetition(), &quiet(20), at());

        assert_eq!(reading.condition, Condition::Nominal, "{}", reading.detail);
        assert_eq!(reading.value, Some(1.0));
        assert_eq!(reading.samples, 20);
        assert!(
            !reading.detail.contains("attempted"),
            "a run that did not repeat is not worth naming: {}",
            reading.detail
        );
    }

    #[test]
    fn one_run_going_round_eleven_times_raises_the_mean() {
        // Twenty ordinary runs and one stuck in a loop. Checked by hand: the
        // total is 20 + 11, over 21 runs, so the mean is 31/21 = 1.476.
        let mut runs = quiet(20);
        runs.push(repeated("looping", "settle", 11));

        let reading = repetition_reading(&repetition(), &runs, at());

        assert_eq!(reading.condition, Condition::Tripped, "{}", reading.detail);
        let value = reading.value.expect("a value");
        assert!((value - 31.0 / 21.0).abs() < 1e-9, "the mean was {value}");
        assert_eq!(reading.samples, 21);
        assert!(reading.detail.contains("is 1.48 attempt(s)"), "{}", reading.detail);
        assert!(
            reading.detail.contains("checkout v1 run looping attempted settle 11 times"),
            "the detail names the run and the node: {}",
            reading.detail
        );
    }

    #[test]
    fn the_error_rate_would_have_seen_none_of_it() {
        // The reason the watch exists, stated as arithmetic. Every run here
        // succeeded, so the error rate is zero and no threshold on it moves.
        // Half the window retried its busiest node three times, which takes the
        // mean to 2 and trips a monitor set at 1.2.
        let mut runs = quiet(10);
        runs.extend((0..10).map(|i| repeated(&format!("retried{i}"), "charge", 3)));

        let reading = repetition_reading(&repetition(), &runs, at());

        assert_eq!(reading.condition, Condition::Tripped, "{}", reading.detail);
        assert_eq!(reading.value, Some(2.0), "10 ones and 10 threes over 20 runs");
    }

    #[test]
    fn a_thin_window_is_insufficient_and_not_nominal() {
        // The distinction the latching rule rests on. `Insufficient` leaves a
        // standing alert where it was; `Nominal` clears it. Three runs have not
        // found the project healthy, they have declined to judge it.
        let reading = repetition_reading(&repetition(), &quiet(3), at());
        assert_eq!(reading.condition, Condition::Insufficient, "{}", reading.detail);
        assert_eq!(reading.value, None, "there is no measurement to report");
        assert_eq!(reading.detail, "3 run(s) in the last 900s; needs at least 20");

        let mut state = MonitorState::new(repetition());
        state.apply(Reading {
            monitor_id: "p1".to_owned(),
            at: at(),
            value: Some(4.0),
            samples: 40,
            condition: Condition::Tripped,
            detail: String::new(),
        });
        state.apply(reading);
        assert_eq!(state.condition, Condition::Tripped, "a thin window must not clear it");
    }

    #[test]
    fn an_empty_window_says_so() {
        let reading = repetition_reading(&repetition(), &[], at());
        assert_eq!(reading.condition, Condition::Insufficient);
        assert_eq!(reading.value, None);
        assert_eq!(reading.detail, "no run moved in the last 900s");
    }

    #[test]
    fn the_busiest_node_wins_and_a_tie_goes_to_the_name() {
        let counts = vec![
            mcpd_runs::NodeAttempts {
                run_id: "r1".to_owned(),
                node: "fetch".to_owned(),
                attempts: 2,
            },
            mcpd_runs::NodeAttempts {
                run_id: "r1".to_owned(),
                node: "settle".to_owned(),
                attempts: 9,
            },
            mcpd_runs::NodeAttempts {
                run_id: "r1".to_owned(),
                node: "write".to_owned(),
                attempts: 1,
            },
            // One run whose two busiest nodes are level.
            mcpd_runs::NodeAttempts {
                run_id: "r2".to_owned(),
                node: "zip".to_owned(),
                attempts: 4,
            },
            mcpd_runs::NodeAttempts {
                run_id: "r2".to_owned(),
                node: "alpha".to_owned(),
                attempts: 4,
            },
        ];

        let peaks = peak_nodes(&counts);
        assert_eq!(peaks.get("r1"), Some(&("settle", 9)));
        assert_eq!(
            peaks.get("r2"),
            Some(&("alpha", 4)),
            "a tie takes the name that sorts first, so the sentence does not move"
        );

        // And the same rows in the other order decide the same way.
        let reversed: Vec<mcpd_runs::NodeAttempts> = counts.iter().rev().cloned().collect();
        assert_eq!(peak_nodes(&reversed), peaks);
    }

    #[test]
    fn the_worst_runs_are_named_first_and_the_rest_are_counted() {
        let mut runs = quiet(20);
        runs.push(repeated("mild", "settle", 2));
        runs.push(repeated("severe", "settle", 30));
        runs.push(repeated("middling", "settle", 7));
        runs.push(repeated("fourth", "settle", 3));

        let reading = repetition_reading(&repetition(), &runs, at());
        let detail = &reading.detail;

        let severe = detail.find("severe").expect("named");
        let middling = detail.find("middling").expect("named");
        let fourth = detail.find("fourth").expect("named");
        assert!(severe < middling && middling < fourth, "worst first: {detail}");
        assert!(!detail.contains("mild"), "only three are named: {detail}");
        assert!(detail.ends_with("; and 1 more"), "{detail}");
    }

    #[test]
    fn raising_the_threshold_lets_the_window_through() {
        let mut runs = quiet(20);
        runs.push(repeated("looping", "settle", 11));

        let mut m = repetition();
        assert_eq!(repetition_reading(&m, &runs, at()).condition, Condition::Tripped);
        // The mean is 1.476, so a threshold of 1.5 covers it.
        m.threshold = 1.5;
        let reading = repetition_reading(&m, &runs, at());
        assert_eq!(reading.condition, Condition::Nominal, "{}", reading.detail);
        // Still named, because the run is still worth opening even when the
        // window as a whole is inside its threshold.
        assert!(reading.detail.contains("looping"), "{}", reading.detail);
    }

    #[test]
    fn the_mean_is_over_every_run_and_not_only_the_repeating_ones() {
        // A window of two thousand runs in which one went round a hundred times
        // barely moves: 1 + 99/2000 is 1.0495. That is the trade the mean makes
        // and it is worth stating, because it is why the detail names the worst
        // runs whatever the mean says.
        let mut runs = quiet(1_999);
        runs.push(repeated("looping", "settle", 100));

        let found = measure(&runs);
        assert_eq!(found.runs, 2_000);
        assert!((found.mean - 1.0495).abs() < 1e-9, "the mean was {}", found.mean);
        assert_eq!(found.worst.len(), 1, "and the one run is still on the list");
    }

    /// The block on the alerting page, parsed by the code it configures.
    ///
    /// A documented configuration the daemon refuses to start on is worse than
    /// none, because the reader cannot tell which of the two is wrong. If this
    /// block changes on the page, change it here.
    #[test]
    fn the_documented_configuration_parses() {
        let written = serde_json::json!({
            "id": "checkout-repetition",
            "name": "Checkout runs are going round the same step",
            "project": "support-desk",
            "watch": "run_repetition",
            "compare": "above",
            "threshold": 1.2,
            "window_seconds": 3600,
            "min_samples": 20,
            "webhook": "https://hooks.example.com/mcpd"
        });
        let m: Monitor = serde_json::from_value(written).expect("the documented monitor parses");
        assert_eq!(m.watch, Watch::RunRepetition);
        assert_eq!(m.compare, Compare::Above);
        assert!((m.threshold - 1.2).abs() < f64::EPSILON);
        assert_eq!(m.window_seconds, 3600);
        assert!(m.enabled, "a monitor is armed unless it says otherwise");
    }

    #[test]
    fn every_published_watch_has_a_variant_the_daemon_reads() {
        // The defect this watch closes, held shut. `run_repetition` was in the
        // spec, in all six generated SDK types and in the published reference,
        // and the daemon had no variant for it, so an operator who wrote the
        // documented value got a daemon that could not read its configuration
        // at all. Nothing compared the two sets, so nothing caught it. This
        // does, and it fails on the next watch added to the spec and not here.
        for published in mcpd_core::MonitorWatch::ALL {
            let wire = serde_json::Value::String(published.as_str().to_owned());
            let parsed: Watch = serde_json::from_value(wire.clone()).unwrap_or_else(|e| {
                panic!(
                    "the spec publishes the watch \"{}\" and the daemon will not read it: {e}",
                    published.as_str()
                )
            });
            assert_eq!(
                serde_json::to_value(parsed).expect("serialise the watch"),
                wire,
                "the daemon reads \"{}\" as a variant that writes itself differently",
                published.as_str()
            );
        }
    }
}

// ------------------------------------------------------------------ SLOs ---

/// A service level objective, and the alerting that follows from it.
///
/// A monitor asks "is the error rate above five percent right now". An SLO asks
/// a different and more useful question: "are we going to miss the target we
/// promised, and how fast are we heading there". The difference matters because
/// a threshold alone cannot tell a two minute blip apart from a slow leak, and
/// those need opposite responses: ignore the first, act on the second.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slo {
    /// Stable identifier.
    pub id: String,
    /// What a person should read.
    pub name: String,
    /// Which project it covers.
    pub project: String,
    /// The fraction of runs that must succeed, between 0 and 1. `0.995` is
    /// "ninety-nine and a half percent of runs succeed".
    pub objective: f64,
    /// How long the promise is measured over.
    #[serde(default = "default_slo_days")]
    pub window_days: u32,
    /// The fewest runs a burn window must hold before it may trip.
    #[serde(default = "default_min_samples")]
    pub min_samples: u64,
    /// Where to POST on a transition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook: Option<String>,
    /// Whether it is evaluated at all.
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn default_slo_days() -> u32 {
    30
}

/// The longest window an objective is measured over, in days: a year.
///
/// The window's start is now minus this many days, and a count past the
/// calendar's range made that subtraction panic inside the timer, on every
/// tick, for every objective after it.
pub const SLO_WINDOW_DAYS_MAX: u32 = 365;

/// The longest wait between two evaluations, in seconds: a day.
///
/// A monitor read less often than daily is not watching anything, and an
/// interval near the top of the integer range overflows the timer's own
/// arithmetic.
pub const INTERVAL_SECONDS_MAX: u64 = 86_400;

/// Refuse the settings that the timer would otherwise fail on, or that can
/// never mean what they say.
///
/// Called at start-up, before the timer exists. Every one of these used to be
/// accepted and then either panicked a tick or quietly evaluated something
/// else: an objective of `99.5` was read as a fraction above one, which leaves
/// a budget of nothing and burns on every run. `mcpd doctor` reported that one
/// as a fault and the daemon started anyway, so the report and the daemon
/// disagreed about whether the configuration was usable.
///
/// # Errors
/// Returns a message naming the objective and the setting, with the range it
/// must be in.
pub fn check_config(slos: &[Slo], interval_seconds: Option<u64>) -> Result<(), String> {
    if let Some(n) = interval_seconds.filter(|n| *n > INTERVAL_SECONDS_MAX) {
        return Err(format!(
            "monitor_interval_seconds must be at most {INTERVAL_SECONDS_MAX}, a day, got {n}"
        ));
    }
    for s in slos {
        if !(0.0..1.0).contains(&s.objective) {
            return Err(format!(
                "objective {} has an objective of {}, which is not a fraction from 0 up to 1. \
                 The error budget is 1 minus this number, so it leaves none and the objective \
                 can never be met. Write 99.5% as 0.995",
                s.id, s.objective
            ));
        }
        if s.window_days == 0 || s.window_days > SLO_WINDOW_DAYS_MAX {
            return Err(format!(
                "objective {} has a window_days of {}; it must be from 1 to \
                 {SLO_WINDOW_DAYS_MAX}. Omit it for the default of {}",
                s.id,
                s.window_days,
                default_slo_days()
            ));
        }
    }
    Ok(())
}

/// The two burn windows, and the rate each must exceed together.
///
/// These are the Google SRE multi-window values and they are not arbitrary. A
/// burn rate of 1 spends the whole budget exactly over the whole window, so 14.4
/// over an hour means "at this rate the month's budget is gone in about two
/// days". Requiring the six hour window to agree is what stops a brief spike
/// paging anyone: a blip moves the one hour number and cannot move the six.
const FAST_WINDOW_HOURS: i64 = 1;
const SLOW_WINDOW_HOURS: i64 = 6;
const FAST_BURN_TRIP: f64 = 14.4;
const SLOW_BURN_TRIP: f64 = 6.0;

/// One evaluation of an SLO.
#[derive(Debug, Clone, Serialize)]
pub struct SloReading {
    /// Which objective.
    pub slo_id: String,
    /// When it was evaluated. RFC 3339, like every other timestamp mcpd emits.
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    /// Observed success rate over the whole window.
    pub success_rate: Option<f64>,
    /// Fraction of the error budget still unspent. Negative once it is
    /// overspent, which is more useful than clamping at zero: it says by how
    /// much the promise was missed.
    pub budget_remaining: Option<f64>,
    /// Burn rate over the fast window.
    pub burn_fast: Option<f64>,
    /// Burn rate over the slow window.
    pub burn_slow: Option<f64>,
    /// Runs in the whole window.
    pub samples: u64,
    /// The resulting condition.
    pub condition: Condition,
    /// What a person should read.
    pub detail: String,
}

/// An SLO plus the condition it is latched in.
#[derive(Debug, Clone, Serialize)]
pub struct SloState {
    /// The objective.
    pub slo: Slo,
    /// Its current condition.
    pub condition: Condition,
    /// The most recent reading.
    pub last: Option<SloReading>,
    /// When it first tripped, while it stays tripped.
    #[serde(with = "time::serde::rfc3339::option")]
    pub tripped_at: Option<OffsetDateTime>,
}

impl SloState {
    /// An objective that has not been evaluated yet.
    #[must_use]
    pub fn new(slo: Slo) -> Self {
        Self { slo, condition: Condition::Insufficient, last: None, tripped_at: None }
    }

    /// Fold a reading in, applying the same latching rule monitors use.
    pub fn apply(&mut self, reading: SloReading) -> bool {
        let was = self.condition;
        self.condition = match (was, reading.condition) {
            (Condition::Acknowledged, Condition::Tripped) => Condition::Acknowledged,
            // Silence is not recovery here either. An agent that stopped
            // serving is not an agent meeting its objective.
            (Condition::Tripped | Condition::Acknowledged, Condition::Insufficient) => was,
            (_, other) => other,
        };
        if self.condition == Condition::Tripped && self.tripped_at.is_none() {
            self.tripped_at = Some(reading.at);
        }
        if self.condition == Condition::Nominal {
            self.tripped_at = None;
        }
        self.last = Some(reading);
        self.condition != was
    }

    /// Acknowledge a tripped objective.
    pub fn acknowledge(&mut self) {
        if self.condition == Condition::Tripped {
            self.condition = Condition::Acknowledged;
        }
    }
}

/// Say how much budget is left in words a person can act on.
///
/// The raw fraction is kept on the reading because it is data, but printing it
/// as a percentage stops being useful the moment it goes negative: "-9900% of
/// the error budget left" is arithmetically true and tells nobody anything. Once
/// the budget is gone the useful number is how far past it you are.
fn budget_phrase(remaining: f64) -> String {
    if remaining >= 0.0 {
        format!("{:.0}% of the error budget left", remaining * 100.0)
    } else {
        format!("the error budget is spent {:.1}x over", 1.0 - remaining)
    }
}

/// The error rate over one window, and how many runs it saw.
async fn error_rate_over(
    state: &Arc<AppState>,
    project: &str,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> Result<(f64, u64), mcpd_store::Error> {
    let s = state.store.analytics_summary(project, from, to, None).await?;
    Ok((s.error_rate, s.trace_count))
}

/// Evaluate one objective.
///
/// # Errors
/// Returns a storage error if the underlying queries fail.
pub async fn evaluate_slo(
    state: &Arc<AppState>,
    slo: &Slo,
    now: OffsetDateTime,
) -> Result<SloReading, mcpd_store::Error> {
    // An objective of 1.0 leaves no budget to burn and every burn rate would be
    // a division by zero. It is also not a promise anyone can keep.
    let budget = (1.0 - slo.objective).max(f64::EPSILON);

    // Saturating, as a second line behind `check_config`: a configuration
    // built without going through start-up must not panic the timer.
    let window_start = now
        .checked_sub(Duration::days(i64::from(slo.window_days.max(1))))
        .unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let (window_errors, samples) = error_rate_over(state, &slo.project, window_start, now).await?;

    let thin = |detail: String| SloReading {
        slo_id: slo.id.clone(),
        at: now,
        success_rate: None,
        budget_remaining: None,
        burn_fast: None,
        burn_slow: None,
        samples,
        condition: Condition::Insufficient,
        detail,
    };

    if samples < slo.min_samples {
        return Ok(thin(format!(
            "{samples} run(s) in {} days; needs at least {}",
            slo.window_days, slo.min_samples
        )));
    }

    let success_rate = 1.0 - window_errors;
    let budget_remaining = 1.0 - (window_errors / budget);

    let (fast_rate, fast_n) =
        error_rate_over(state, &slo.project, now - Duration::hours(FAST_WINDOW_HOURS), now).await?;
    let (slow_rate, slow_n) =
        error_rate_over(state, &slo.project, now - Duration::hours(SLOW_WINDOW_HOURS), now).await?;

    let burn_fast = fast_rate / budget;
    let burn_slow = slow_rate / budget;

    // Both windows must agree, and both must hold enough runs to mean anything.
    // Either one alone is the failure mode this design exists to avoid: the fast
    // window alone pages on a blip, the slow window alone notices a leak hours
    // late.
    let enough = fast_n >= slo.min_samples && slow_n >= slo.min_samples;
    let burning = burn_fast > FAST_BURN_TRIP && burn_slow > SLOW_BURN_TRIP;

    let condition = if enough && burning { Condition::Tripped } else { Condition::Nominal };
    let detail = if !enough {
        format!(
            "{:.3}% of runs succeeded over {} days, {:.0}% of the error budget left. \
             Burn is not judged: {fast_n} run(s) in the last hour and {slow_n} in six, \
             needs {} in each",
            success_rate * 100.0,
            slo.window_days,
            budget_remaining * 100.0,
            slo.min_samples
        )
    } else if burning {
        format!(
            "burning at {burn_fast:.1}x over 1h and {burn_slow:.1}x over 6h, \
             past {FAST_BURN_TRIP}x and {SLOW_BURN_TRIP}x. {}",
            budget_phrase(budget_remaining)
        )
    } else {
        format!(
            "{:.3}% of runs succeeded over {} days against a {:.3}% objective. {}. \
             Burn {burn_fast:.1}x over 1h, {burn_slow:.1}x over 6h",
            success_rate * 100.0,
            slo.window_days,
            slo.objective * 100.0,
            budget_phrase(budget_remaining)
        )
    };

    Ok(SloReading {
        slo_id: slo.id.clone(),
        at: now,
        success_rate: Some(success_rate),
        budget_remaining: Some(budget_remaining),
        burn_fast: Some(burn_fast),
        burn_slow: Some(burn_slow),
        samples,
        condition,
        detail,
    })
}

/// Evaluate every configured objective once and fold the readings in.
pub async fn tick_slos(state: &Arc<AppState>, now: OffsetDateTime) -> Vec<SloState> {
    let slos: Vec<Slo> = state.config.slos.clone();
    let mut changed = Vec::new();

    for slo in slos.iter().filter(|s| s.enabled) {
        // A storage error is not a reading, for the same reason it is not one
        // for a monitor: recording it as `Insufficient` would be
        // indistinguishable from a quiet window, and a quiet window must never
        // clear a standing alert.
        let reading = match evaluate_slo(state, slo, now).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(slo = %slo.id, error = %e, "could not evaluate an objective");
                continue;
            }
        };

        let mut states = recover(&state.slos, "objective");
        let Some(slot) = states.iter_mut().find(|s| s.slo.id == slo.id) else { continue };
        if slot.apply(reading) {
            changed.push(slot.clone());
        }
    }

    changed
}

/// POST an objective's transition to its webhook.
async fn notify_slo(state: &Arc<AppState>, s: &SloState) {
    let Some(url) = s.slo.webhook.as_deref() else { return };
    let r = s.last.as_ref();
    let body = serde_json::json!({
        "slo_id": s.slo.id,
        "name": s.slo.name,
        "project": s.slo.project,
        "condition": s.condition,
        "objective": s.slo.objective,
        "window_days": s.slo.window_days,
        "success_rate": r.and_then(|x| x.success_rate),
        "budget_remaining": r.and_then(|x| x.budget_remaining),
        "burn_fast": r.and_then(|x| x.burn_fast),
        "burn_slow": r.and_then(|x| x.burn_slow),
        "detail": r.map(|x| x.detail.clone()),
        "at": r.map(|x| x
            .at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default()),
    });
    match state.outbound.post(url).json(&body).send().await {
        Ok(res) if res.status().is_success() => {}
        Ok(res) => tracing::warn!(
            slo = %s.slo.id,
            status = %res.status(),
            "the alert webhook refused the notification"
        ),
        // Without the URL, for the reason `notify` gives.
        Err(e) => tracing::warn!(
            slo = %s.slo.id,
            host = crate::query::webhook_host(Some(url)).unwrap_or_default(),
            error = crate::outbound_error(e),
            "could not reach the alert webhook"
        ),
    }
}

#[cfg(test)]
mod slo_tests {
    use super::*;

    fn slo(objective: f64) -> Slo {
        Slo {
            id: "s".to_owned(),
            name: "n".to_owned(),
            project: "default".to_owned(),
            objective,
            window_days: 30,
            min_samples: 20,
            webhook: None,
            enabled: true,
        }
    }

    fn reading(condition: Condition) -> SloReading {
        SloReading {
            slo_id: "s".to_owned(),
            at: OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap(),
            success_rate: Some(0.99),
            budget_remaining: Some(0.4),
            burn_fast: Some(20.0),
            burn_slow: Some(8.0),
            samples: 100,
            condition,
            detail: "d".to_owned(),
        }
    }

    /// The whole reason an SLO is not just a threshold: a spike moves the one
    /// hour number and cannot move the six, so it must not page.
    #[test]
    fn a_blip_does_not_trip_because_both_windows_must_agree() {
        let budget: f64 = 1.0 - 0.995;
        // Five minutes of total failure inside an hour: a very high fast burn.
        let fast = 0.08 / budget;
        // The same outage spread over six hours barely moves that window.
        let slow = 0.013 / budget;
        assert!(fast > FAST_BURN_TRIP, "the fast window should see it");
        assert!(slow < SLOW_BURN_TRIP, "the slow window should not, {slow}");
        assert!(!(fast > FAST_BURN_TRIP && slow > SLOW_BURN_TRIP), "so it must not trip");
    }

    /// And the case it must catch: a sustained leak moves both.
    ///
    /// The numbers are worth stating, because they set how bad things must be
    /// before anyone is woken. On a 99.5% objective the budget is 0.5%, so the
    /// fast threshold of 14.4x is an error rate of about 7.2%. A five percent
    /// error rate is 10x: over the slow threshold, under the fast one, and
    /// therefore deliberately not a page. That is the tuning, not an oversight.
    #[test]
    fn a_sustained_leak_trips_because_both_windows_see_it() {
        let budget: f64 = 1.0 - 0.995;

        let leaking = 0.10;
        assert!(leaking / budget > FAST_BURN_TRIP, "{}", leaking / budget);
        assert!(leaking / budget > SLOW_BURN_TRIP);

        // Just under the fast threshold: the budget is being spent faster than
        // planned, but not fast enough to wake somebody.
        let steady = 0.05;
        assert!(steady / budget > SLOW_BURN_TRIP);
        assert!(steady / budget < FAST_BURN_TRIP, "{}", steady / budget);
    }

    /// An objective of 1.0 leaves no budget. Dividing by it would produce an
    /// infinite burn rate and page for ever, so the epsilon floor matters.
    #[test]
    fn a_perfect_objective_does_not_divide_by_zero() {
        let s = slo(1.0);
        let budget = (1.0 - s.objective).max(f64::EPSILON);
        assert!(budget > 0.0 && (0.0_f64 / budget).is_finite());
    }

    #[test]
    fn an_objective_latches_and_a_thin_window_does_not_clear_it() {
        let mut st = SloState::new(slo(0.995));
        assert!(st.apply(reading(Condition::Tripped)));
        assert_eq!(st.condition, Condition::Tripped);
        // No change reported while it stays tripped.
        assert!(!st.apply(reading(Condition::Tripped)));
        // Traffic stops. That is not recovery.
        assert!(!st.apply(reading(Condition::Insufficient)));
        assert_eq!(st.condition, Condition::Tripped);
        // A real recovery does clear it.
        assert!(st.apply(reading(Condition::Nominal)));
        assert_eq!(st.condition, Condition::Nominal);
        assert!(st.tripped_at.is_none());
    }

    #[test]
    fn acknowledging_survives_a_still_breaching_reading() {
        let mut st = SloState::new(slo(0.995));
        st.apply(reading(Condition::Tripped));
        st.acknowledge();
        assert_eq!(st.condition, Condition::Acknowledged);
        st.apply(reading(Condition::Tripped));
        assert_eq!(st.condition, Condition::Acknowledged, "must not re-page");
        st.apply(reading(Condition::Nominal));
        assert_eq!(st.condition, Condition::Nominal, "and clears on its own");
    }
}
