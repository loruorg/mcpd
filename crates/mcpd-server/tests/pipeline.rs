//! What the intake pipeline does to a batch, and in particular what it does to
//! the four signals that point at a trace when the sampler discards that trace.
//!
//! A log, metric, score or feedback record names a `trace_id` and a `span_id`.
//! If the sampler drops the trace and the pipeline keeps the record, the store
//! ends up holding a log line whose trace cannot be opened, which is a defect a
//! reader meets as a broken link rather than as a sampling policy.

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use mcpd_core::cost::builtin_book;
use mcpd_core::redact::Redactor;
use mcpd_core::sampling::{Decision, Policy};
use mcpd_core::{Correlation, Feedback, LogRecord, MetricPoint, SamplingKind, Score, Span};
use mcpd_server::pipeline::{Intake, Prepared};
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::{Batch, Store};
use serde_json::json;
use std::collections::BTreeMap;
use std::process;
use std::sync::Arc;
use tower::ServiceExt;

/// A trace id whose low sixteen hex digits are `tail`.
///
/// The deterministic sampler hashes exactly those digits, so `tail` is the only
/// part of the id that decides anything and a test can name a trace it knows
/// the policy keeps and one it knows the policy drops.
fn trace(tail: u64) -> String {
    format!("{:016x}{tail:016x}", 0_u64)
}

/// A trace id `Policy::deterministic(0.5)` keeps.
fn kept() -> String {
    trace(1)
}

/// A trace id `Policy::deterministic(0.5)` drops.
fn dropped() -> String {
    trace(u64::MAX)
}

fn correlation(project: &str) -> Correlation {
    serde_json::from_value(json!({ "project": project })).expect("correlation")
}

fn span_in(trace_id: &str, project: &str) -> Span {
    serde_json::from_value(json!({
        "trace_id": trace_id,
        "span_id": "00f067aa0ba902b7",
        "name": "gen",
        "kind": "model_generation",
        "status": "ok",
        "start_time": "2026-09-02T00:00:00Z",
        "is_root": true,
        "is_event": false,
        "correlation": { "project": project },
        "received_at": "2026-09-02T00:00:00Z",
    }))
    .expect("span")
}

fn log_for(trace_id: Option<&str>, project: &str) -> LogRecord {
    LogRecord {
        log_id: "log-1".to_owned(),
        trace_id: trace_id.map(str::to_owned),
        span_id: Some("00f067aa0ba902b7".to_owned()),
        timestamp: time::OffsetDateTime::UNIX_EPOCH,
        level: mcpd_core::LogLevel::Info,
        message: "tool returned".to_owned(),
        fields: None,
        logger: None,
        correlation: correlation(project),
        received_at: time::OffsetDateTime::UNIX_EPOCH,
    }
}

fn metric_for(trace_id: Option<&str>, project: &str) -> MetricPoint {
    MetricPoint {
        metric_id: "metric-1".to_owned(),
        name: "mcpd.model.duration".to_owned(),
        kind: mcpd_core::MetricKind::Histogram,
        value: 12.0,
        unit: Some("ms".to_owned()),
        timestamp: time::OffsetDateTime::UNIX_EPOCH,
        labels: None,
        buckets: None,
        trace_id: trace_id.map(str::to_owned),
        span_id: Some("00f067aa0ba902b7".to_owned()),
        correlation: correlation(project),
        received_at: time::OffsetDateTime::UNIX_EPOCH,
    }
}

fn score_for(trace_id: Option<&str>, project: &str) -> Score {
    Score {
        score_id: "score-1".to_owned(),
        trace_id: trace_id.map(str::to_owned),
        span_id: Some("00f067aa0ba902b7".to_owned()),
        scorer_id: "answer_relevance".to_owned(),
        scorer_version: None,
        source: mcpd_core::ScoreSource::Live,
        value: 0.5,
        min: Some(0.0),
        max: Some(1.0),
        label: None,
        reason: None,
        score_trace_id: None,
        cost: None,
        metadata: None,
        timestamp: time::OffsetDateTime::UNIX_EPOCH,
        correlation: correlation(project),
    }
}

fn feedback_for(trace_id: Option<&str>, project: &str) -> Feedback {
    Feedback {
        feedback_id: "feedback-1".to_owned(),
        trace_id: trace_id.map(str::to_owned),
        span_id: Some("00f067aa0ba902b7".to_owned()),
        kind: mcpd_core::FeedbackKind::Thumbs,
        value: json!("up"),
        comment: None,
        author_id: Some("reviewer-3".to_owned()),
        author_kind: Some("reviewer".to_owned()),
        timestamp: time::OffsetDateTime::UNIX_EPOCH,
        metadata: None,
        correlation: correlation(project),
    }
}

/// Every signal kind at once, all naming the same trace.
fn signals_for(trace_id: Option<&str>, project: &str) -> Batch {
    Batch {
        logs: vec![log_for(trace_id, project)],
        metrics: vec![metric_for(trace_id, project)],
        scores: vec![score_for(trace_id, project)],
        feedback: vec![feedback_for(trace_id, project)],
        ..Batch::default()
    }
}

fn prepare_with(policy: &Policy, batch: &mut Batch) -> Prepared {
    let redactor = Redactor::with_defaults();
    let book = builtin_book();
    Intake::new(&redactor, &book, policy).prepare(batch)
}

fn prepare_per_project(per_project: &BTreeMap<String, Policy>, batch: &mut Batch) -> Prepared {
    let redactor = Redactor::with_defaults();
    let book = builtin_book();
    let global = Policy::always();
    Intake::new(&redactor, &book, &global).with_projects(per_project).prepare(batch)
}

#[test]
fn the_fixture_trace_ids_sample_the_way_every_test_here_assumes() {
    // Without this, a change to the hash would turn every assertion below into
    // a test of nothing at all, and they would still pass.
    let policy = Policy::deterministic(0.5);
    assert_eq!(policy.decide(&kept(), None), Decision::Keep, "the kept id must sample in");
    assert_eq!(policy.decide(&dropped(), None), Decision::Drop, "the dropped id must sample out");
}

#[test]
fn a_sampled_out_trace_takes_its_logs_metrics_scores_and_feedback_with_it() {
    let mut batch = signals_for(Some(&dropped()), "default");
    batch.spans = vec![span_in(&dropped(), "default")];

    let out = prepare_with(&Policy::deterministic(0.5), &mut batch);

    assert!(batch.spans.is_empty(), "the span was sampled out");
    assert!(batch.logs.is_empty(), "a log naming a sampled-out trace must not be stored");
    assert!(batch.metrics.is_empty(), "a metric naming a sampled-out trace must not be stored");
    assert!(batch.scores.is_empty(), "a score naming a sampled-out trace must not be stored");
    assert!(batch.feedback.is_empty(), "feedback naming a sampled-out trace must not be stored");
    assert_eq!(out.sampled_out, 1, "one span was discarded");
    assert_eq!(out.signals_sampled_out, 4, "one of each other signal went with it");
}

#[test]
fn a_signal_survives_a_trace_that_samples_in_with_no_span_in_this_batch() {
    // The spans arrived in an earlier batch and were kept. Consulting only this
    // batch would throw away a log line about a run that is on disk.
    let mut batch = signals_for(Some(&kept()), "default");

    let out = prepare_with(&Policy::deterministic(0.5), &mut batch);

    assert_eq!(batch.logs.len(), 1);
    assert_eq!(batch.metrics.len(), 1);
    assert_eq!(batch.scores.len(), 1);
    assert_eq!(batch.feedback.len(), 1);
    assert_eq!(out.signals_sampled_out, 0);
}

#[test]
fn a_signal_that_names_no_trace_is_not_touched_by_sampling() {
    // A metric with no exemplar cannot be orphaned: there is no trace for it to
    // point at. Dropping it would delete data the sampler never decided about.
    let mut batch = signals_for(None, "default");

    let never = Policy { kind: SamplingKind::Never, ..Policy::default() };
    let out = prepare_with(&never, &mut batch);

    assert_eq!(batch.logs.len(), 1, "a log with no trace id survives any policy");
    assert_eq!(batch.metrics.len(), 1);
    assert_eq!(batch.scores.len(), 1);
    assert_eq!(batch.feedback.len(), 1);
    assert_eq!(out.signals_sampled_out, 0);
}

#[test]
fn a_signal_is_dropped_for_a_trace_sampled_out_in_an_earlier_batch() {
    // The production shape: the SDK flushed the spans in one batch and the log
    // line it correlated to them in the next. The decision is recomputed from
    // the trace id, which is what makes the second batch agree with the first.
    let mut batch = signals_for(Some(&dropped()), "default");

    let out = prepare_with(&Policy::deterministic(0.5), &mut batch);

    assert!(batch.logs.is_empty());
    assert!(batch.metrics.is_empty());
    assert!(batch.scores.is_empty());
    assert!(batch.feedback.is_empty());
    assert_eq!(out.signals_sampled_out, 4);
    assert_eq!(out.sampled_out, 0, "no span was in this batch to discard");
}

#[test]
fn a_ratio_policy_keeps_a_signal_whose_trace_it_cannot_decide_twice() {
    // `ratio` flips a coin per call, so calling it again for a signal is a new
    // decision rather than the trace's decision recalled. Recomputing here
    // would discard signals at the sampling rate whatever happened to their
    // trace, so a signal it cannot answer for is kept.
    let ratio = Policy { kind: SamplingKind::Ratio, probability: 1e-9, ..Policy::default() };
    let mut batch = signals_for(Some(&dropped()), "default");

    let out = prepare_with(&ratio, &mut batch);

    assert_eq!(batch.logs.len(), 1, "a coin flip must not decide a signal's fate");
    assert_eq!(batch.metrics.len(), 1);
    assert_eq!(batch.scores.len(), 1);
    assert_eq!(batch.feedback.len(), 1);
    assert_eq!(out.signals_sampled_out, 0);
}

#[test]
fn one_project_being_sampled_out_does_not_reach_another() {
    let mut per_project = BTreeMap::new();
    per_project
        .insert("loud".to_owned(), Policy { kind: SamplingKind::Never, ..Policy::default() });

    let mut batch = Batch {
        logs: vec![log_for(Some(&kept()), "loud"), log_for(Some(&trace(2)), "calm")],
        ..Batch::default()
    };
    let out = prepare_per_project(&per_project, &mut batch);

    assert_eq!(batch.logs.len(), 1, "only the sampled-out project's log goes");
    assert_eq!(batch.logs[0].correlation.project, "calm");
    assert_eq!(out.signals_sampled_out, 1);
}

#[test]
fn nothing_is_removed_when_no_policy_samples_anything() {
    let mut batch = signals_for(Some(&dropped()), "default");
    batch.spans = vec![span_in(&dropped(), "default")];

    let out = prepare_with(&Policy::always(), &mut batch);

    assert_eq!(batch.spans.len(), 1);
    assert_eq!(batch.logs.len(), 1);
    assert_eq!(batch.metrics.len(), 1);
    assert_eq!(batch.scores.len(), 1);
    assert_eq!(batch.feedback.len(), 1);
    assert_eq!(out.signals_sampled_out, 0);
}

#[test]
fn a_batch_of_one_score_comes_back_empty_when_its_trace_samples_out() {
    // Stated as its own test because it is the invariant the single-record
    // doors depend on: `create_score` and `create_feedback` read the record
    // back out of the batch after `prepare`, and after this change the batch
    // they read can be empty. They must take it with `first()`, not by index.
    let mut batch =
        Batch { scores: vec![score_for(Some(&dropped()), "default")], ..Batch::default() };

    prepare_with(&Policy::deterministic(0.5), &mut batch);

    // Written the way those two doors have to read it from now on.
    let stored: Option<Score> = batch.scores.first().cloned();
    assert!(stored.is_none(), "an unstorable score is not left in the batch");
}

// ------------------------------------------------- the door that indexes ---
//
// `create_score` and `create_feedback` put one record in a batch, run it
// through `Intake::prepare`, and then read it back. They used to index it with
// `batch.scores[0]` and `batch.feedback[0]`: sampling was the one thing in the
// pipeline that removed a signal and it only ever removed spans, so that index
// was always in bounds. Once a score naming a sampled-out trace stopped being
// stored, the batch those two doors read came back empty and the index
// panicked.
//
// The case below was `#[ignore]`d in the idiom `writes.rs` uses, run and
// observed failing, until `query.rs` took the record with `first()` and
// answered the sender when it is absent, the same way the ceiling path refuses
// a single record rather than storing less than it claimed. It runs now, and
// it is what stops the index from coming back.

/// A private database file, named from the process **and** the instant it
/// started. A name from the pid alone is reopened when the operating system
/// recycles one, and the run then reads the previous run's database.
fn temp_db() -> std::path::PathBuf {
    // The clock alone is not unique: on macOS it moves in microseconds, so two
    // tests starting together would open one database. The counter makes the
    // name this test's own.
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("mcpd-pipeline-test-{}-{unique}-{n}", process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join("0.db")
}

/// The real router, over a real store, sampling at one half.
async fn sampling_daemon() -> Router {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let config = ServerConfig { sampling: Policy::deterministic(0.5), ..ServerConfig::default() };
    let state = Arc::new(AppState::new(Arc::clone(&store) as Arc<dyn Store>, config));
    router(state)
}

#[tokio::test]
async fn the_score_door_answers_rather_than_panicking_for_a_sampled_out_trace() {
    let app = sampling_daemon().await;
    let body = json!({
        "score_id": "score-1",
        "trace_id": dropped(),
        "scorer_id": "answer_relevance",
        "source": "live",
        "value": 0.5,
        "timestamp": "2026-09-02T00:00:00Z",
        "correlation": { "project": "default" },
    })
    .to_string();
    let request = Request::builder()
        .method("POST")
        .uri("/v1/api/scores")
        .header("content-type", "application/json")
        .header("content-length", body.len().to_string())
        .body(Body::from(body))
        .expect("build the request");

    let response = app.oneshot(request).await.expect("the router returned a response");

    // The door refuses rather than storing a score whose trace no reader can
    // open, and says why: a silent success would claim a row that is not there.
    assert_eq!(
        response.status(),
        axum::http::StatusCode::UNPROCESSABLE_ENTITY,
        "the door must answer the sender, not panic"
    );
}
