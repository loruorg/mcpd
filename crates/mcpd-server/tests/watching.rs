//! Monitors and objectives evaluated against a real store.
//!
//! The unit tests beside `monitor.rs` cover the latching state machine by
//! handing it readings that were written by hand. Nothing there calls
//! `evaluate`, so the part that turns stored spans into a number, the whole
//! `Watch` dispatch, was never run; and the SLO tests re-did the burn-rate
//! arithmetic in the test instead of calling `evaluate_slo`, which cannot fail
//! when the real arithmetic changes.
//!
//! So every test here seeds spans, lets the store build the trace projection
//! the server reads, and calls the real function. The expected numbers are
//! stated as constants a person can check by hand, never recomputed from the
//! same expression the code under test uses.

use mcpd_core::{Correlation, Cost, Score, ScoreSource, Span, SpanKind, SpanStatus};
use mcpd_runs::RunStore;
use mcpd_server::monitor::{
    evaluate, evaluate_slo, tick, Compare, Condition, Monitor, MonitorState, Slo, Watch,
};
use mcpd_server::{AppState, ServerConfig};
use mcpd_store::{sqlite::SqliteStore, Batch, Store};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use time::{Duration, OffsetDateTime};

/// Every test measures against one project, because every test gets a database
/// of its own.
const PROJECT: &str = "default";

/// A fixed instant, so a window either contains a run or does not, with no
/// dependence on when the suite happens to run.
fn now() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("a valid instant")
}

/// A store of its own for each test.
///
/// Deliberately not `:memory:`: `SqliteStore` opens the in-memory database
/// under one fixed shared-cache name, so every in-memory store in the process
/// is the same database and tests running in parallel would count each other's
/// runs.
async fn store() -> Arc<dyn Store> {
    // A counter rather than a clock. Two tests starting in the same nanosecond
    // can be handed the same name by a timestamp, and two tests sharing one
    // database file measure each other's runs.
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join("mcpd-watching-tests").join(format!(
        "{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    // A recycled process id could leave a database, and its write-ahead log,
    // from an earlier run sitting at this path.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temporary directory");
    let s = SqliteStore::open(dir.join("mcpd.db"), 2).expect("open the store");
    s.migrate().await.expect("migrate the store");
    Arc::new(s)
}

/// State around a store. `monitors` matters only to `tick`, which reads the
/// configured set rather than being handed one.
fn watching(store: &Arc<dyn Store>, monitors: Vec<Monitor>) -> Arc<AppState> {
    Arc::new(AppState::new(Arc::clone(store), ServerConfig { monitors, ..ServerConfig::default() }))
}

/// A monitor with everything a given test does not care about already decided.
fn watch(what: Watch, compare: Compare, threshold: f64) -> Monitor {
    Monitor {
        id: "m1".to_owned(),
        name: "watched".to_owned(),
        project: PROJECT.to_owned(),
        watch: what,
        scorer_id: None,
        compare,
        threshold,
        window_seconds: 300,
        min_samples: 20,
        z_bound: 3.5,
        webhook: None,
        enabled: true,
    }
}

/// A group of identical finished runs.
///
/// `first_id` keeps the identifiers of two groups in one store apart; reusing
/// one would overwrite the earlier group rather than adding to it, and the
/// counts under test would silently be wrong.
struct Runs {
    first_id: u32,
    at: OffsetDateTime,
    count: u32,
    errors: u32,
    duration_ms: i64,
    cost: f64,
}

impl Default for Runs {
    fn default() -> Self {
        Self { first_id: 0, at: now(), count: 0, errors: 0, duration_ms: 100, cost: 0.0 }
    }
}

impl Runs {
    /// One root span per run.
    ///
    /// Spans, not trace rows: the summary the monitors read is a projection the
    /// store rebuilds from spans, so writing the projection directly would test
    /// the test rather than the path the server takes.
    fn spans(&self) -> Vec<Span> {
        (0..self.count)
            .map(|i| {
                let id = self.first_id + i;
                let status = if i < self.errors { SpanStatus::Error } else { SpanStatus::Ok };
                Span {
                    trace_id: format!("{id:032x}"),
                    span_id: format!("{id:016x}"),
                    parent_span_id: None,
                    external_parent_span_id: None,
                    name: "agent-run".to_owned(),
                    kind: SpanKind::AgentRun,
                    status,
                    start_time: self.at,
                    end_time: Some(self.at + Duration::milliseconds(self.duration_ms)),
                    duration_ns: None,
                    is_root: true,
                    is_event: false,
                    entity_kind: None,
                    entity_id: None,
                    entity_name: None,
                    input: None,
                    output: None,
                    attributes: None,
                    metadata: None,
                    usage: None,
                    cost: Some(Cost {
                        currency: "USD".to_owned(),
                        input_cost: None,
                        output_cost: None,
                        total_cost: Some(self.cost),
                        price_book: Some("builtin".to_owned()),
                        is_estimated: false,
                    }),
                    model_parameters: None,
                    error: None,
                    links: None,
                    tags: None,
                    correlation: correlation(),
                    model: None,
                    provider: None,
                    received_at: self.at,
                    ingest_key: None,
                }
            })
            .collect()
    }
}

fn correlation() -> Correlation {
    Correlation {
        project: PROJECT.to_owned(),
        environment: Some("production".to_owned()),
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
    }
}

async fn seed(store: &Arc<dyn Store>, runs: &Runs) {
    let r = store
        .ingest(Batch { spans: runs.spans(), ..Batch::default() })
        .await
        .expect("the seed to be written");
    // A seed that did not land would leave every assertion below measuring an
    // empty window, which reads as a passing test for the wrong reason.
    assert_eq!(r.accepted, runs.count, "the seed must land in full");
}

/// Scores for one scorer, all carrying the same value.
fn scores(scorer: &str, first_id: u32, at: OffsetDateTime, count: u32, value: f64) -> Vec<Score> {
    (0..count)
        .map(|i| Score {
            score_id: format!("sc-{}", first_id + i),
            trace_id: None,
            span_id: None,
            scorer_id: scorer.to_owned(),
            scorer_version: None,
            source: ScoreSource::Live,
            value,
            min: Some(0.0),
            max: Some(1.0),
            label: None,
            reason: None,
            score_trace_id: None,
            cost: None,
            metadata: None,
            timestamp: at,
            correlation: correlation(),
        })
        .collect()
}

async fn seed_scores(store: &Arc<dyn Store>, scores: Vec<Score>) {
    let n = scores.len() as u32;
    let r =
        store.ingest(Batch { scores, ..Batch::default() }).await.expect("the scores to be written");
    assert_eq!(r.accepted, n, "the scores must land in full");
}

/// Compare two numbers that came back through SQL and floating point.
#[track_caller]
fn close(actual: Option<f64>, expected: f64) {
    let actual = actual.expect("a measured value");
    assert!((actual - expected).abs() < 1e-9, "expected {expected}, measured {actual}");
}

// ------------------------------------------------------------- the watches ---

#[tokio::test]
async fn an_error_rate_is_read_from_the_stored_runs_and_trips_only_past_its_threshold() {
    let store = store().await;
    seed(
        &store,
        &Runs { count: 20, errors: 5, at: now() - Duration::seconds(60), ..Runs::default() },
    )
    .await;
    let state = watching(&store, Vec::new());

    let tripped = evaluate(&state, &watch(Watch::ErrorRate, Compare::Above, 0.05), now())
        .await
        .expect("evaluate");
    assert_eq!(tripped.condition, Condition::Tripped);
    assert_eq!(tripped.samples, 20);
    close(tripped.value, 0.25);

    // The same stored runs, judged against a threshold they do not breach.
    let nominal = evaluate(&state, &watch(Watch::ErrorRate, Compare::Above, 0.50), now())
        .await
        .expect("evaluate");
    assert_eq!(nominal.condition, Condition::Nominal);
    close(nominal.value, 0.25);
}

#[tokio::test]
async fn latency_p95_trips_on_the_stored_tail_of_slow_runs() {
    let store = store().await;
    let at = now() - Duration::seconds(60);
    seed(&store, &Runs { count: 18, duration_ms: 100, at, ..Runs::default() }).await;
    seed(&store, &Runs { first_id: 100, count: 2, duration_ms: 5000, at, ..Runs::default() }).await;
    let state = watching(&store, Vec::new());

    // Nearest rank over twenty runs puts the ninety-fifth percentile on the
    // nineteenth slowest, which is the first of the two slow ones.
    let tripped = evaluate(&state, &watch(Watch::LatencyP95, Compare::Above, 1000.0), now())
        .await
        .expect("evaluate");
    assert_eq!(tripped.condition, Condition::Tripped);
    close(tripped.value, 5000.0);

    let nominal = evaluate(&state, &watch(Watch::LatencyP95, Compare::Above, 6000.0), now())
        .await
        .expect("evaluate");
    assert_eq!(nominal.condition, Condition::Nominal);
    close(nominal.value, 5000.0);
}

#[tokio::test]
async fn one_slow_run_in_twenty_does_not_reach_the_ninety_fifth_percentile() {
    // The reading has to be the percentile and not the maximum, or every
    // latency monitor pages on the first outlier.
    let store = store().await;
    let at = now() - Duration::seconds(60);
    seed(&store, &Runs { count: 19, duration_ms: 100, at, ..Runs::default() }).await;
    seed(&store, &Runs { first_id: 100, count: 1, duration_ms: 5000, at, ..Runs::default() }).await;
    let state = watching(&store, Vec::new());

    let r = evaluate(&state, &watch(Watch::LatencyP95, Compare::Above, 1000.0), now())
        .await
        .expect("evaluate");
    close(r.value, 100.0);
    assert_eq!(r.condition, Condition::Nominal);
}

#[tokio::test]
async fn cost_sums_the_window_and_trips_over_budget() {
    let store = store().await;
    seed(
        &store,
        &Runs { count: 20, cost: 0.006, at: now() - Duration::seconds(60), ..Runs::default() },
    )
    .await;
    let state = watching(&store, Vec::new());

    let tripped =
        evaluate(&state, &watch(Watch::Cost, Compare::Above, 0.10), now()).await.expect("evaluate");
    assert_eq!(tripped.condition, Condition::Tripped);
    close(tripped.value, 0.12);

    let nominal =
        evaluate(&state, &watch(Watch::Cost, Compare::Above, 0.50), now()).await.expect("evaluate");
    assert_eq!(nominal.condition, Condition::Nominal);
    close(nominal.value, 0.12);
}

#[tokio::test]
async fn a_thin_stream_trips_a_trace_count_watched_from_below() {
    // The silent-pipeline case: enough runs to judge, too few to be healthy.
    let store = store().await;
    seed(&store, &Runs { count: 12, at: now() - Duration::seconds(60), ..Runs::default() }).await;
    let state = watching(&store, Vec::new());

    let mut m = watch(Watch::TraceCount, Compare::Below, 20.0);
    m.min_samples = 5;
    let tripped = evaluate(&state, &m, now()).await.expect("evaluate");
    assert_eq!(tripped.condition, Condition::Tripped);
    close(tripped.value, 12.0);
    assert_eq!(tripped.samples, 12);

    m.threshold = 5.0;
    let nominal = evaluate(&state, &m, now()).await.expect("evaluate");
    assert_eq!(nominal.condition, Condition::Nominal);
    close(nominal.value, 12.0);
}

#[tokio::test]
async fn a_score_mean_averages_the_window_and_trips_below_its_floor() {
    let store = store().await;
    let at = now() - Duration::seconds(60);
    seed(&store, &Runs { count: 20, at, ..Runs::default() }).await;
    seed_scores(&store, scores("answer_relevance", 0, at, 20, 0.4)).await;
    // Older, better scores that the window must exclude. Averaging them in
    // would give 0.65 and clear a real regression.
    seed_scores(&store, scores("answer_relevance", 100, now() - Duration::hours(2), 20, 0.9)).await;
    let state = watching(&store, Vec::new());

    let mut m = watch(Watch::ScoreMean, Compare::Below, 0.7);
    m.scorer_id = Some("answer_relevance".to_owned());
    let tripped = evaluate(&state, &m, now()).await.expect("evaluate");
    assert_eq!(tripped.condition, Condition::Tripped);
    close(tripped.value, 0.4);

    m.threshold = 0.2;
    let nominal = evaluate(&state, &m, now()).await.expect("evaluate");
    assert_eq!(nominal.condition, Condition::Nominal);
    close(nominal.value, 0.4);
}

#[tokio::test]
async fn a_window_thinner_than_min_samples_is_never_judged() {
    // Three failed runs is a hundred percent error rate and means nothing. The
    // monitor must decline rather than trip on it.
    let store = store().await;
    seed(
        &store,
        &Runs { count: 3, errors: 3, at: now() - Duration::seconds(60), ..Runs::default() },
    )
    .await;
    let state = watching(&store, Vec::new());

    let r = evaluate(&state, &watch(Watch::ErrorRate, Compare::Above, 0.05), now())
        .await
        .expect("evaluate");
    assert_eq!(r.condition, Condition::Insufficient);
    assert_eq!(r.samples, 3);
    assert!(r.value.is_none(), "a window too thin to judge has no value to report");
}

#[tokio::test]
async fn a_score_mean_with_no_scores_declines_rather_than_reading_zero() {
    // A missing scorer must not read as a mean of zero, which on a monitor
    // watching from below is indistinguishable from total failure.
    let store = store().await;
    let at = now() - Duration::seconds(60);
    seed(&store, &Runs { count: 20, at, ..Runs::default() }).await;
    seed_scores(&store, scores("faithfulness", 0, at, 20, 0.95)).await;
    let state = watching(&store, Vec::new());

    let mut m = watch(Watch::ScoreMean, Compare::Below, 0.5);
    m.scorer_id = Some("answer_relevance".to_owned());
    let r = evaluate(&state, &m, now()).await.expect("evaluate");
    assert_eq!(r.condition, Condition::Insufficient, "a zero mean would have tripped this");
    assert!(r.value.is_none());
    assert!(r.detail.contains("answer_relevance"), "detail was {}", r.detail);
}

// ------------------------------------------------------------------- SLOs ---

/// An objective with a budget of half a percent, so the arithmetic below is
/// checkable by hand: a ten percent error rate is a burn of twenty.
fn objective() -> Slo {
    Slo {
        id: "s1".to_owned(),
        name: "runs succeed".to_owned(),
        project: PROJECT.to_owned(),
        objective: 0.995,
        window_days: 30,
        min_samples: 20,
        webhook: None,
        enabled: true,
    }
}

#[tokio::test]
async fn a_sustained_error_rate_burns_the_budget_and_trips_the_objective() {
    let store = store().await;
    // Ten percent failing in the last hour, and the same rate over the five
    // hours before it, so both burn windows see the leak.
    seed(
        &store,
        &Runs { count: 40, errors: 4, at: now() - Duration::minutes(30), ..Runs::default() },
    )
    .await;
    seed(
        &store,
        &Runs {
            first_id: 1000,
            count: 40,
            errors: 4,
            at: now() - Duration::hours(3),
            ..Runs::default()
        },
    )
    .await;
    let state = watching(&store, Vec::new());

    let r = evaluate_slo(&state, &objective(), now()).await.expect("evaluate_slo");
    assert_eq!(r.condition, Condition::Tripped);
    assert_eq!(r.samples, 80);
    close(r.success_rate, 0.9);
    // 0.10 spent against a 0.005 budget: twenty times over on both windows.
    close(r.burn_fast, 20.0);
    close(r.burn_slow, 20.0);
    close(r.budget_remaining, -19.0);
}

#[tokio::test]
async fn a_short_blip_does_not_trip_because_the_six_hour_window_disagrees() {
    let store = store().await;
    // Twenty percent of the last hour failed, which alone is a burn of forty.
    seed(
        &store,
        &Runs { count: 20, errors: 4, at: now() - Duration::minutes(30), ..Runs::default() },
    )
    .await;
    // The five hours before it were clean, so over six hours the same four
    // failures are two percent, a burn of four.
    seed(
        &store,
        &Runs { first_id: 1000, count: 180, at: now() - Duration::hours(3), ..Runs::default() },
    )
    .await;
    let state = watching(&store, Vec::new());

    let r = evaluate_slo(&state, &objective(), now()).await.expect("evaluate_slo");
    close(r.burn_fast, 40.0);
    close(r.burn_slow, 4.0);
    // Forty is far past the fast trip of 14.4, so a single-window rule would
    // have paged here. Both windows must agree, and the slow one does not.
    assert_eq!(r.condition, Condition::Nominal, "{}", r.detail);
}

// --------------------------------------------------------------- latching ---

#[tokio::test]
async fn a_quiet_window_does_not_clear_a_standing_alert() {
    let store = store().await;
    let t0 = now();
    seed(&store, &Runs { count: 20, errors: 5, at: t0 - Duration::seconds(60), ..Runs::default() })
        .await;
    // A clean stretch two hours later, and nothing at all in between.
    seed(
        &store,
        &Runs {
            first_id: 1000,
            count: 20,
            at: t0 + Duration::hours(2) - Duration::seconds(60),
            ..Runs::default()
        },
    )
    .await;

    let m = watch(Watch::ErrorRate, Compare::Above, 0.05);
    let state = watching(&store, Vec::new());
    let mut standing = MonitorState::new(m.clone());

    let first = evaluate(&state, &m, t0).await.expect("evaluate");
    assert_eq!(first.condition, Condition::Tripped);
    assert!(standing.apply(first), "the first trip notifies");
    assert_eq!(standing.condition, Condition::Tripped);
    assert_eq!(standing.tripped_at, Some(t0));

    // An hour later the window holds nothing. Silence is not recovery.
    let quiet = evaluate(&state, &m, t0 + Duration::hours(1)).await.expect("evaluate");
    assert_eq!(quiet.condition, Condition::Insufficient);
    assert_eq!(quiet.samples, 0);
    assert!(!standing.apply(quiet), "a quiet window is not a transition");
    assert_eq!(standing.condition, Condition::Tripped, "silence must not clear the alert");
    assert_eq!(standing.tripped_at, Some(t0), "and must not restart the clock");

    // Traffic returns, and it is clean. That is a real recovery.
    let recovered = evaluate(&state, &m, t0 + Duration::hours(2)).await.expect("evaluate");
    assert_eq!(recovered.condition, Condition::Nominal);
    assert!(standing.apply(recovered), "recovery notifies once");
    assert_eq!(standing.condition, Condition::Nominal);
    assert!(standing.tripped_at.is_none());
}

#[tokio::test]
async fn a_tick_folds_the_reading_into_the_shared_state_and_reports_it_once() {
    let store = store().await;
    seed(
        &store,
        &Runs { count: 20, errors: 5, at: now() - Duration::seconds(60), ..Runs::default() },
    )
    .await;
    let state = watching(&store, vec![watch(Watch::ErrorRate, Compare::Above, 0.05)]);

    let changed = tick(&state, now()).await;
    assert_eq!(changed.len(), 1, "the trip is a change worth reporting");
    assert_eq!(changed[0].condition, Condition::Tripped);
    close(changed[0].last.as_ref().and_then(|r| r.value), 0.25);

    let again = tick(&state, now()).await;
    assert!(again.is_empty(), "a standing alert is not reported again");
    let held = state.monitors.read().expect("the monitor states");
    assert_eq!(held[0].condition, Condition::Tripped, "and it stays latched");
}

// -------------------------------------------- structural divergence ---
//
// The unit tests beside `monitor.rs` cover the statistics against runs written
// by hand. These cover the half that reads storage: that the watch takes its
// step counts from the durable run store rather than the trace projection, that
// it holds to its window, and that it says so rather than guessing when the
// daemon serves no runs at all.

/// A definition whose step count is the length of its input array.
///
/// One definition version producing runs of genuinely different shapes is the
/// situation this watch exists for, and a fixed list of tasks cannot produce
/// it: every run of three tasks takes three steps.
fn fanout() -> serde_json::Value {
    serde_json::json!({
        "steps": [{
            "kind": "foreach",
            "id": "each",
            "over": "input.items",
            "body": { "kind": "task", "id": "work" },
            "concurrency": 8,
            "max_items": 500
        }]
    })
}

/// A definition that runs one node once, which is the shape of a run that has
/// not repeated itself at all.
fn steady() -> serde_json::Value {
    serde_json::json!({ "steps": [{ "kind": "task", "id": "work" }] })
}

/// A definition whose loop re-enters one node nine times.
///
/// A loop rather than a `foreach`, because the run repetition watch exists for
/// the run that goes round the same node and a fan-out over nine items is
/// nine different pieces of work. Both read the same way to the watch, which is
/// what the fan-out test below pins.
fn spinner() -> serde_json::Value {
    serde_json::json!({
        "steps": [{
            "kind": "loop",
            "id": "go",
            "body": { "kind": "task", "id": "inner" },
            "max_iterations": 9
        }]
    })
}

/// A signal store, a run store beside it, and the state that reads both.
async fn watching_runs() -> (Arc<AppState>, Arc<dyn mcpd_runs::RunStore>) {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join("mcpd-divergence-tests").join(format!(
        "{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temporary directory");

    let signals = SqliteStore::open(dir.join("mcpd.db"), 2).expect("open the store");
    signals.migrate().await.expect("migrate the store");
    let runs =
        mcpd_runs::SqliteRunStore::open(dir.join("runs.db"), ServerConfig::default().run_config)
            .expect("open the run store");
    runs.migrate().await.expect("migrate the run store");
    let runs: Arc<dyn mcpd_runs::RunStore> = Arc::new(runs);

    for (id, body) in [("checkout", fanout()), ("steady", steady()), ("spinner", spinner())] {
        runs.put_definition(mcpd_runs::DefinitionDocument {
            project: PROJECT.to_owned(),
            id: id.to_owned(),
            kind: mcpd_runs::DefinitionKind::Workflow,
            version: Some(1),
            body,
        })
        .await
        .expect("store the definition");
    }

    let state = Arc::new(
        AppState::new(Arc::new(signals) as Arc<dyn Store>, ServerConfig::default())
            .with_run_store(Arc::clone(&runs)),
    );
    (state, runs)
}

/// Start a run over `items` items. Nothing has claimed a step yet.
async fn start_run(
    runs: &Arc<dyn mcpd_runs::RunStore>,
    items: usize,
    at: OffsetDateTime,
) -> String {
    runs.start_run(
        mcpd_runs::StartRun {
            project: PROJECT.to_owned(),
            definition_id: "checkout".to_owned(),
            definition_version: Some(1),
            input: Some(serde_json::json!({ "items": vec![1; items] })),
            correlation: None,
            idempotency_key: None,
        },
        at,
    )
    .await
    .expect("start the run")
    .run_id
}

/// Drive a run to the end, so its step count is one the store counted rather
/// than one the test wrote into a row.
async fn drive(runs: &Arc<dyn mcpd_runs::RunStore>, run_id: &str, at: OffsetDateTime) {
    loop {
        let leases =
            runs.claim(PROJECT, "worker", 64, Some(run_id), at).await.expect("claim steps");
        if leases.is_empty() {
            break;
        }
        for lease in leases {
            runs.complete_step(
                &lease.lease_id,
                mcpd_runs::StepCompletion {
                    output: Some(serde_json::json!(1)),
                    ..Default::default()
                },
                at,
            )
            .await
            .expect("complete the step");
        }
    }
}

async fn finished_run(
    runs: &Arc<dyn mcpd_runs::RunStore>,
    items: usize,
    at: OffsetDateTime,
) -> String {
    let id = start_run(runs, items, at).await;
    drive(runs, &id, at).await;
    id
}

fn divergence_watch() -> Monitor {
    Monitor {
        id: "d1".to_owned(),
        name: "checkout shape".to_owned(),
        project: PROJECT.to_owned(),
        watch: Watch::StepCountDivergence,
        scorer_id: None,
        compare: Compare::Above,
        threshold: 0.0,
        window_seconds: 900,
        min_samples: 20,
        z_bound: 3.5,
        webhook: None,
        enabled: true,
    }
}

#[tokio::test]
async fn a_run_that_ran_away_is_found_in_the_run_store() {
    let (state, runs) = watching_runs().await;
    let at = now() - Duration::seconds(60);

    // Twelve runs whose shape varies the way ordinary work varies.
    for i in 0..12 {
        finished_run(&runs, 3 + i % 3, at).await;
    }
    // And one that fanned out over a list forty long.
    let runaway = finished_run(&runs, 40, at).await;

    let reading =
        evaluate(&state, &divergence_watch(), now()).await.expect("evaluate the divergence watch");

    assert_eq!(reading.condition, Condition::Tripped, "{}", reading.detail);
    assert_eq!(reading.value, Some(1.0), "{}", reading.detail);
    assert_eq!(reading.samples, 13);
    assert!(reading.detail.contains(&runaway), "the detail names it: {}", reading.detail);
}

#[tokio::test]
async fn a_run_still_in_flight_is_judged_before_it_finishes() {
    // The point of the watch. A run that has already taken four times its
    // definition's usual number of steps is reported while it is still going,
    // long before it reaches any ceiling.
    let (state, runs) = watching_runs().await;
    let at = now() - Duration::seconds(60);
    for i in 0..12 {
        finished_run(&runs, 3 + i % 3, at).await;
    }

    let quiet =
        evaluate(&state, &divergence_watch(), now()).await.expect("evaluate the divergence watch");
    assert_eq!(quiet.condition, Condition::Nominal, "{}", quiet.detail);

    // A run over forty items, driven but never finished: the store leaves it
    // `running` because the last claim is still outstanding.
    let looping = start_run(&runs, 40, at).await;
    for _ in 0..3 {
        let leases =
            runs.claim(PROJECT, "worker", 16, Some(&looping), at).await.expect("claim steps");
        for lease in leases {
            runs.complete_step(&lease.lease_id, mcpd_runs::StepCompletion::default(), at)
                .await
                .expect("complete the step");
        }
    }
    assert_eq!(
        runs.get_run(&looping).await.expect("read the run").status,
        mcpd_runs::RunStatus::Running,
        "the run under test has to still be going"
    );

    let reading =
        evaluate(&state, &divergence_watch(), now()).await.expect("evaluate the divergence watch");
    assert_eq!(reading.condition, Condition::Tripped, "{}", reading.detail);
    assert!(reading.detail.contains(&looping), "{}", reading.detail);
}

#[tokio::test]
async fn a_thin_run_store_declines_rather_than_reporting_all_clear() {
    let (state, runs) = watching_runs().await;
    let at = now() - Duration::seconds(60);
    for i in 0..4 {
        finished_run(&runs, 3 + i, at).await;
    }

    let reading =
        evaluate(&state, &divergence_watch(), now()).await.expect("evaluate the divergence watch");
    assert_eq!(reading.condition, Condition::Insufficient, "{}", reading.detail);
    assert_eq!(reading.value, None);
    assert!(reading.detail.contains("the most any has is 4"), "{}", reading.detail);
}

#[tokio::test]
async fn runs_outside_the_window_are_not_read() {
    let (state, runs) = watching_runs().await;
    // A full baseline, but all of it two hours old.
    let old = now() - Duration::hours(2);
    for i in 0..12 {
        finished_run(&runs, 3 + i % 3, old).await;
    }
    finished_run(&runs, 40, old).await;

    let reading =
        evaluate(&state, &divergence_watch(), now()).await.expect("evaluate the divergence watch");
    assert_eq!(reading.condition, Condition::Insufficient, "{}", reading.detail);
    assert_eq!(reading.detail, "no run moved in the last 900s");

    // Widen the window past them and the same runs are read.
    let mut wide = divergence_watch();
    wide.window_seconds = 24 * 3600;
    let reading = evaluate(&state, &wide, now()).await.expect("evaluate the divergence watch");
    assert_eq!(reading.condition, Condition::Tripped, "{}", reading.detail);
}

#[tokio::test]
async fn a_daemon_with_no_run_store_says_so() {
    // The state a deployment is in when it collects traces and serves no runs.
    // Reporting `Nominal` here would clear a standing alert on the strength of
    // having read nothing at all.
    let store = store().await;
    let state = watching(&store, Vec::new());
    let reading =
        evaluate(&state, &divergence_watch(), now()).await.expect("evaluate the divergence watch");
    assert_eq!(reading.condition, Condition::Insufficient);
    assert_eq!(reading.value, None);
    assert_eq!(
        reading.detail,
        "this daemon serves no durable runs, so there are no step counts to compare"
    );
}

#[tokio::test]
async fn the_divergence_watch_does_not_read_the_trace_count_gate() {
    // The signal store here is empty, so `min_samples` against the trace count
    // would decline every reading this watch ever takes. It reads the run store
    // instead, and a deployment can serve runs whose spans are ingested
    // somewhere else entirely.
    let (state, runs) = watching_runs().await;
    let at = now() - Duration::seconds(60);
    for i in 0..12 {
        finished_run(&runs, 3 + i % 3, at).await;
    }
    let mut m = divergence_watch();
    m.min_samples = 10_000;

    let reading = evaluate(&state, &m, now()).await.expect("evaluate the divergence watch");
    assert_eq!(reading.condition, Condition::Nominal, "{}", reading.detail);
}

// The run repetition watch, against a real run store.
//
// The unit tests beside `monitor.rs` hand the arithmetic runs written by hand.
// These cover the half that reads storage: that the count comes from the
// journal the daemon actually wrote, that a loop's iterations fold onto the one
// node they re-entered, and that the watch holds to its window.

/// Start a run of `definition`, with `input` when it takes one.
async fn start_of(
    runs: &Arc<dyn mcpd_runs::RunStore>,
    definition: &str,
    input: Option<serde_json::Value>,
    at: OffsetDateTime,
) -> String {
    runs.start_run(
        mcpd_runs::StartRun {
            project: PROJECT.to_owned(),
            definition_id: definition.to_owned(),
            definition_version: Some(1),
            input,
            correlation: None,
            idempotency_key: None,
        },
        at,
    )
    .await
    .expect("start the run")
    .run_id
}

/// One finished run of `definition`.
async fn finished_of(
    runs: &Arc<dyn mcpd_runs::RunStore>,
    definition: &str,
    at: OffsetDateTime,
) -> String {
    let id = start_of(runs, definition, None, at).await;
    drive(runs, &id, at).await;
    id
}

/// A monitor watching run repetition, set where the documentation sets it.
fn repetition_watch() -> Monitor {
    Monitor {
        id: "p1".to_owned(),
        name: "checkout repetition".to_owned(),
        project: PROJECT.to_owned(),
        watch: Watch::RunRepetition,
        scorer_id: None,
        compare: Compare::Above,
        threshold: 1.2,
        window_seconds: 900,
        min_samples: 20,
        z_bound: 3.5,
        webhook: None,
        enabled: true,
    }
}

#[tokio::test]
async fn a_run_going_round_one_node_is_found_in_the_run_store() {
    let (state, runs) = watching_runs().await;
    let at = now() - Duration::seconds(60);

    // Twenty runs that did one thing once, and one that went round nine times.
    for _ in 0..20 {
        finished_of(&runs, "steady", at).await;
    }
    let looping = finished_of(&runs, "spinner", at).await;

    let reading =
        evaluate(&state, &repetition_watch(), now()).await.expect("evaluate the repetition watch");

    assert_eq!(reading.condition, Condition::Tripped, "{}", reading.detail);
    assert_eq!(reading.samples, 21);
    // By hand: twenty runs at 1 and one at 9, so the mean is 29/21 = 1.381.
    let value = reading.value.expect("a value");
    assert!((value - 29.0 / 21.0).abs() < 1e-9, "the mean was {value}");
    assert!(
        reading.detail.contains(&format!("spinner v1 run {looping} attempted go/inner 9 times")),
        "the detail names the run and the node it went round: {}",
        reading.detail
    );
}

#[tokio::test]
async fn every_run_doing_its_work_once_reads_exactly_one() {
    let (state, runs) = watching_runs().await;
    let at = now() - Duration::seconds(60);
    for _ in 0..20 {
        finished_of(&runs, "steady", at).await;
    }

    let reading =
        evaluate(&state, &repetition_watch(), now()).await.expect("evaluate the repetition watch");

    assert_eq!(reading.condition, Condition::Nominal, "{}", reading.detail);
    assert_eq!(reading.value, Some(1.0), "one is the floor and the healthy reading");
    assert!(!reading.detail.contains("attempted"), "{}", reading.detail);
}

#[tokio::test]
async fn a_fan_out_reads_as_repetition_and_the_documentation_says_so() {
    // The one place this watch says something an operator has to know about
    // before setting a threshold. A `foreach` over nine items is nine attempts
    // at one node, and the journal cannot tell that apart from a loop that went
    // round nine times: both are indexed keys under one node, and only the
    // definition knows which construct wrote them. The watch reads the literal
    // statistic the specification defines, so a definition with a wide fan-out
    // sits high and its threshold has to be set for it.
    let (state, runs) = watching_runs().await;
    let at = now() - Duration::seconds(60);
    for _ in 0..20 {
        finished_of(&runs, "steady", at).await;
    }
    let wide = finished_run(&runs, 9, at).await;

    let reading =
        evaluate(&state, &repetition_watch(), now()).await.expect("evaluate the repetition watch");

    assert_eq!(reading.condition, Condition::Tripped, "{}", reading.detail);
    assert!(
        reading.detail.contains(&format!("checkout v1 run {wide} attempted each 9 times")),
        "{}",
        reading.detail
    );
}

#[tokio::test]
async fn a_thin_window_declines_rather_than_reporting_all_clear() {
    let (state, runs) = watching_runs().await;
    let at = now() - Duration::seconds(60);
    // Three runs, one of them plainly stuck. Three runs is not a window.
    for _ in 0..2 {
        finished_of(&runs, "steady", at).await;
    }
    finished_of(&runs, "spinner", at).await;

    let reading =
        evaluate(&state, &repetition_watch(), now()).await.expect("evaluate the repetition watch");

    assert_eq!(reading.condition, Condition::Insufficient, "{}", reading.detail);
    assert_eq!(reading.value, None);
    assert_eq!(reading.detail, "3 run(s) in the last 900s; needs at least 20");
}

#[tokio::test]
async fn repetition_holds_to_its_window() {
    let (state, runs) = watching_runs().await;
    let old = now() - Duration::hours(2);
    for _ in 0..20 {
        finished_of(&runs, "steady", old).await;
    }
    finished_of(&runs, "spinner", old).await;

    let reading =
        evaluate(&state, &repetition_watch(), now()).await.expect("evaluate the repetition watch");
    assert_eq!(reading.condition, Condition::Insufficient, "{}", reading.detail);
    assert_eq!(reading.detail, "no run moved in the last 900s");

    // Widen the window past them and the same runs are read.
    let mut wide = repetition_watch();
    wide.window_seconds = 24 * 3600;
    let reading = evaluate(&state, &wide, now()).await.expect("evaluate the repetition watch");
    assert_eq!(reading.condition, Condition::Tripped, "{}", reading.detail);
}

#[tokio::test]
async fn a_daemon_with_no_run_store_says_so_for_repetition() {
    // The state a deployment is in when it collects traces and serves no runs.
    // Reporting `Nominal` here would clear a standing alert on the strength of
    // having read nothing at all.
    let store = store().await;
    let state = watching(&store, Vec::new());
    let reading =
        evaluate(&state, &repetition_watch(), now()).await.expect("evaluate the repetition watch");
    assert_eq!(reading.condition, Condition::Insufficient);
    assert_eq!(reading.value, None);
    assert_eq!(
        reading.detail,
        "this daemon serves no durable runs, so there are no attempts to count"
    );
}

#[tokio::test]
async fn the_repetition_watch_does_not_read_the_trace_count_gate() {
    // The signal store here is empty, so `min_samples` against the trace count
    // would decline every reading this watch ever takes. It counts the runs it
    // read from the run store instead, and a deployment can serve runs whose
    // spans are ingested somewhere else entirely.
    let (state, runs) = watching_runs().await;
    let at = now() - Duration::seconds(60);
    for _ in 0..20 {
        finished_of(&runs, "steady", at).await;
    }

    let reading =
        evaluate(&state, &repetition_watch(), now()).await.expect("evaluate the repetition watch");
    assert_eq!(reading.condition, Condition::Nominal, "{}", reading.detail);
    assert_eq!(reading.samples, 20, "the samples are runs, not traces");
}
