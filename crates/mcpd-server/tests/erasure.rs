//! Erasure by subject: `DELETE /v1/api/subjects/{user_id}`.
//!
//! The endpoint answers a deletion request about one person. These tests are
//! written against the ways it can be wrong, in descending order of how badly:
//!
//! 1. It under-deletes and reports success. A run's child spans hold the prompt
//!    and the completion, and usually only the root span carries `user_id`, so
//!    an implementation that deletes the rows matching the subject leaves the
//!    payload behind and returns a confident count.
//! 2. It over-deletes across a tenancy boundary. The same subject id can exist
//!    in a project the caller cannot reach.
//! 3. It matches nothing because stored ids are hashed and the request's is
//!    not, which looks exactly like "this person was never here".
//! 4. It leaves a row that hangs off the subject's span rather than the run,
//!    or a piece of feedback the subject wrote themselves, or a captured copy
//!    of the subject's prompt sitting in a dataset.
//! 5. It reports a bare zero it has no way to justify, because the identifier
//!    is hashed and nothing records which key hashed which row.
//! 6. It accepts a key that should not be able to destroy anything.
//!
//! Requests go through `mcpd_server::router`, so the handler, the hashing, the
//! authorisation and the SQL are under test together. What is left afterwards
//! is read through the `Store` trait rather than through the same list endpoint
//! the erasure ran behind, so a filter that hides a surviving row cannot make
//! one of these tests pass.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mcpd_server::state::ProjectConfig;
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::{
    Aggregation, AuditQuery, Include, LogQuery, MetricQuery, ScoreQuery, SpanQuery, Store,
    TraceQuery,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

// ------------------------------------------------------------ seed data ---
//
// Two people in `default`, and one of them again in `other`.
//
// run        project  root user_id  spans  logs  metrics  scores  feedback
// ANN_RUN    default  user-ann      2      2     2        2       2
// BOB_RUN    default  user-bob      1      1     0        1       1
// ANN_OTHER  other    user-ann      1      1     0        0       0
//
// ANN_RUN's second span carries no `user_id` of its own and holds the prompt
// and the completion. It is the row a per-row erasure would leave behind.
// There is also one log line stamped `user-ann` with no trace at all, which
// only a direct match on `user_id` reaches.
//
// Four rows exist to catch the reaches the handler used not to have:
//
// - `log-ann-span`, `metric-ann-span`, `score-ann-span` and
//   `feedback-ann-span` name ANN_MODEL and carry no trace id and no user id.
//   Only a match on the subject's spans reaches them.
// - `feedback-by-ann` hangs off BOB_RUN and carries `author_id: user-ann`. It
//   is Ann's own prose about somebody else's run, so it goes when Ann does,
//   and Bob's run must survive it going.

const DAY: &str = "2026-03-01";

const ANN_RUN: &str = "a0000000000000000000000000000001";
const BOB_RUN: &str = "b0000000000000000000000000000002";
const ANN_OTHER: &str = "c0000000000000000000000000000003";

const ANN_ROOT: &str = "a000000000000001";
const ANN_MODEL: &str = "a000000000000002";
const BOB_ROOT: &str = "b000000000000001";
const ANN_OTHER_ROOT: &str = "c000000000000001";

const ANN: &str = "user-ann";
const BOB: &str = "user-bob";

/// A third person, and a run of theirs that reuses one of the subject's span
/// ids. Span ids are unique inside a trace, not inside a project.
const CAROL_RUN: &str = "d0000000000000000000000000000004";
const CAROL: &str = "user-carol";

/// A later run by the subject, ingested after the redaction key moved.
const ANN_LATER: &str = "e0000000000000000000000000000005";
const ANN_LATER_ROOT: &str = "e000000000000001";

/// A word only Ann's payload contains, so a full-text hit for it is proof the
/// prompt is still reachable.
const ANN_PROMPT: &str = "zarquon";

/// The total the happy path expects: 1 trace, 2 spans, 3 logs, 2 metrics,
/// 2 scores, 3 feedback.
const ANN_TOTAL: u64 = 13;

// ------------------------------------------------------------- fixtures ---
//
// `temp_db`, `Res`, `send` and the request builders follow the shape used by
// `http.rs` and `queries.rs`. They are duplicated rather than shared because
// Cargo compiles each integration test as its own crate, and lifting them into
// a shared module would mean editing a file this change does not own.

fn temp_db() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("mcpd-erasure-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

fn span(
    trace: &str,
    id: &str,
    parent: Option<&str>,
    project: &str,
    user: Option<&str>,
    extra: Value,
) -> Value {
    let mut correlation = json!({ "project": project });
    if let Some(u) = user {
        correlation["user_id"] = json!(u);
    }
    let mut v = json!({
        "trace_id": trace,
        "span_id": id,
        "name": "checkout",
        "kind": "agent_run",
        "status": "ok",
        "start_time": format!("{DAY}T00:00:00Z"),
        "end_time": format!("{DAY}T00:00:01Z"),
        "is_root": parent.is_none(),
        "is_event": false,
        "correlation": correlation,
        "received_at": format!("{DAY}T00:10:00Z"),
    });
    if let Some(p) = parent {
        v["parent_span_id"] = json!(p);
    }
    let Value::Object(extra) = extra else { panic!("the extra fields must be an object") };
    for (k, val) in extra {
        v[k] = val;
    }
    v
}

fn log_line(id: &str, trace: Option<&str>, project: &str, user: Option<&str>) -> Value {
    let mut correlation = json!({ "project": project });
    if let Some(u) = user {
        correlation["user_id"] = json!(u);
    }
    let mut v = json!({
        "log_id": id,
        "timestamp": format!("{DAY}T00:00:00Z"),
        "level": "info",
        "message": format!("audit line {id}"),
        "correlation": correlation,
        "received_at": format!("{DAY}T00:10:00Z"),
    });
    if let Some(t) = trace {
        v["trace_id"] = json!(t);
    }
    v
}

/// A log line attached to one step and to nothing else: no trace id, no user
/// id. The only thing that reaches it is a match on the subject's spans.
fn log_on_span(id: &str, span: &str, project: &str) -> Value {
    json!({
        "log_id": id,
        "span_id": span,
        "timestamp": format!("{DAY}T00:00:00Z"),
        "level": "info",
        "message": format!("audit line {id}"),
        "correlation": { "project": project },
        "received_at": format!("{DAY}T00:10:00Z"),
    })
}

fn seed_batch() -> Value {
    json!({
        "spans": [
            // The root carries the request the person actually made. It is what
            // a capture into a dataset copies, so it has to be here for that
            // path to be exercised at all.
            span(ANN_RUN, ANN_ROOT, None, "default", Some(ANN), json!({
                "input": { "prompt": format!("refund the {ANN_PROMPT} order") },
            })),
            // No `user_id` on this one, and it is the span holding the payload.
            // A per-row erasure keyed on `user_id` leaves this standing.
            span(ANN_RUN, ANN_MODEL, Some(ANN_ROOT), "default", None, json!({
                "name": "generate",
                "kind": "model_generation",
                "model": "gpt-4o",
                "input": { "prompt": format!("refund the {ANN_PROMPT} order") },
                "output": { "text": format!("the {ANN_PROMPT} refund is queued") },
            })),
            span(BOB_RUN, BOB_ROOT, None, "default", Some(BOB), json!({})),
            span(ANN_OTHER, ANN_OTHER_ROOT, None, "other", Some(ANN), json!({})),
        ],
        "logs": [
            log_line("log-ann-trace", Some(ANN_RUN), "default", Some(ANN)),
            log_line("log-ann-loose", None, "default", Some(ANN)),
            log_on_span("log-ann-span", ANN_MODEL, "default"),
            log_line("log-bob", Some(BOB_RUN), "default", Some(BOB)),
            log_line("log-ann-other", Some(ANN_OTHER), "other", Some(ANN)),
        ],
        "metrics": [
            {
                "metric_id": "metric-ann",
                "name": "tokens",
                "kind": "counter",
                "value": 1.0,
                "timestamp": format!("{DAY}T00:00:00Z"),
                "trace_id": ANN_RUN,
                "correlation": { "project": "default" },
                "received_at": format!("{DAY}T00:10:00Z"),
            },
            {
                "metric_id": "metric-ann-span",
                "name": "tokens",
                "kind": "counter",
                "value": 2.0,
                "timestamp": format!("{DAY}T00:00:00Z"),
                "span_id": ANN_MODEL,
                "correlation": { "project": "default" },
                "received_at": format!("{DAY}T00:10:00Z"),
            },
        ],
        "scores": [
            {
                "score_id": "score-ann", "trace_id": ANN_RUN, "scorer_id": "helpfulness",
                "source": "live", "value": 0.9,
                "timestamp": format!("{DAY}T00:00:00Z"),
                "correlation": { "project": "default" },
            },
            {
                "score_id": "score-ann-span", "span_id": ANN_MODEL, "scorer_id": "helpfulness",
                "source": "live", "value": 0.8,
                "timestamp": format!("{DAY}T00:00:00Z"),
                "correlation": { "project": "default" },
            },
            {
                "score_id": "score-bob", "trace_id": BOB_RUN, "scorer_id": "helpfulness",
                "source": "live", "value": 0.4,
                "timestamp": format!("{DAY}T00:00:00Z"),
                "correlation": { "project": "default" },
            },
        ],
        "feedback": [
            {
                "feedback_id": "feedback-ann",
                "trace_id": ANN_RUN,
                "kind": "thumbs",
                "value": 1,
                "timestamp": format!("{DAY}T00:00:00Z"),
                "correlation": { "project": "default" },
            },
            {
                "feedback_id": "feedback-ann-span",
                "span_id": ANN_MODEL,
                "kind": "thumbs",
                "value": 1,
                "timestamp": format!("{DAY}T00:00:00Z"),
                "correlation": { "project": "default" },
            },
            // Ann's own words about Bob's run. Nothing but the author reaches
            // it, and Bob's run must not go with it.
            {
                "feedback_id": "feedback-by-ann",
                "trace_id": BOB_RUN,
                "kind": "comment",
                "value": "the refund never arrived",
                "author_id": ANN,
                "timestamp": format!("{DAY}T00:00:00Z"),
                "correlation": { "project": "default" },
            },
            // The same author, in the project the erasing key cannot reach.
            // Matching on the author is a new reach, and a new reach is a new
            // way to cross a tenancy boundary.
            {
                "feedback_id": "feedback-by-ann-other",
                "trace_id": ANN_OTHER,
                "kind": "comment",
                "value": "the refund never arrived",
                "author_id": ANN,
                "timestamp": format!("{DAY}T00:00:00Z"),
                "correlation": { "project": "other" },
            },
        ],
    })
}

/// The router and the store behind it, seeded.
async fn seeded(mut config: ServerConfig) -> (Router, Arc<SqliteStore>) {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    // The table at the top of this file counts the rows the seed writes, and
    // the daemon derives a metric from a span by default. Leaving that on would
    // put rows in the store that the table does not name, so every count below
    // would be measuring a fixture nobody wrote down. That a derived point is
    // erased with the run it came from is a real property and it is pinned in
    // `derived_metrics.rs`, where it is the subject rather than a side effect.
    config.derived_metrics = mcpd_server::derive::DerivedMetrics::off();
    // Seeding uses whichever ingest key the test configured. Writing the
    // fixture is not what any of these tests are about, so it must never be the
    // thing that fails: a test about who may erase should not turn into a test
    // about who may ingest.
    let ingest_key = config.ingest_keys.first().cloned();
    let app = router(Arc::new(AppState::new(Arc::clone(&store) as Arc<dyn Store>, config)));

    let res = send(&app, post("/v1/ingest", seed_batch().to_string(), ingest_key.as_deref())).await;
    assert_eq!(res.status, StatusCode::OK, "seeding failed: {}", res.text());
    // Four spans, five logs, two metrics, three scores, four feedback. A short
    // count means a row was rejected and every assertion below would be
    // measuring the wrong fixture.
    assert_eq!(res.json()["accepted"], json!(18), "the fixture did not land: {}", res.text());
    (app, store)
}

// -------------------------------------------------------------- plumbing ---

struct Res {
    status: StatusCode,
    body: Vec<u8>,
}

impl Res {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| {
            panic!("expected a JSON body ({e}), got: {}", String::from_utf8_lossy(&self.body))
        })
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

async fn send(app: &Router, req: Request<Body>) -> Res {
    let res = app.clone().oneshot(req).await.expect("the router returned a response");
    let status = res.status();
    let body = res.into_body().collect().await.expect("read the response body").to_bytes().to_vec();
    Res { status, body }
}

fn erase(subject: &str, query: &str, key: Option<&str>) -> Request<Body> {
    let uri = if query.is_empty() {
        format!("/v1/api/subjects/{subject}")
    } else {
        format!("/v1/api/subjects/{subject}?{query}")
    };
    let mut b = Request::builder().method("DELETE").uri(uri);
    if let Some(k) = key {
        b = b.header("authorization", format!("Bearer {k}"));
    }
    b.body(Body::empty()).expect("build the request")
}

fn post(uri: &str, body: impl Into<Vec<u8>>, key: Option<&str>) -> Request<Body> {
    let body: Vec<u8> = body.into();
    let mut b = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("content-length", body.len().to_string());
    if let Some(k) = key {
        b = b.header("authorization", format!("Bearer {k}"));
    }
    b.body(Body::from(body)).expect("build the request")
}

// ------------------------------------------------- reading what is left ---
//
// Every count below goes to the store directly. Reading through the HTTP list
// endpoints would mean the same filtering code decided both what to erase and
// what to report, so a row that survived behind a filter would be invisible to
// the test as well as to the operator.

async fn trace_count(store: &SqliteStore, project: &str) -> usize {
    store
        .list_traces(&TraceQuery { project: project.to_owned(), limit: 500, ..Default::default() })
        .await
        .expect("list traces")
        .items
        .len()
}

async fn span_count(store: &SqliteStore, project: &str) -> usize {
    store
        .list_spans(&SpanQuery { project: project.to_owned(), limit: 500, ..Default::default() })
        .await
        .expect("list spans")
        .items
        .len()
}

async fn log_count(store: &SqliteStore, project: &str) -> usize {
    store
        .list_logs(&LogQuery { project: project.to_owned(), limit: 500, ..Default::default() })
        .await
        .expect("list logs")
        .items
        .len()
}

async fn score_count(store: &SqliteStore, project: &str) -> usize {
    store
        .list_scores(&ScoreQuery { project: project.to_owned(), limit: 500, ..Default::default() })
        .await
        .expect("list scores")
        .items
        .len()
}

/// The total of every `tokens` observation the metric store still holds.
///
/// Two rows written at the same instant land in one daily bucket, so counting
/// points cannot tell one from two. The sum can, which is what makes a row that
/// survived visible here.
async fn metric_total(store: &SqliteStore, project: &str) -> f64 {
    let q = MetricQuery {
        project: project.to_owned(),
        name: "tokens".to_owned(),
        from: time::macros::datetime!(2026 - 01 - 01 0:00 UTC),
        to: time::macros::datetime!(2027 - 01 - 01 0:00 UTC),
        bucket_seconds: 86_400,
        agg: Aggregation::Sum,
        group_by: Vec::new(),
        filter: Vec::new(),
    };
    store
        .query_metrics(&q)
        .await
        .expect("query metrics")
        .series
        .iter()
        .flat_map(|s| s.points.iter())
        .map(|p| p.value)
        .sum()
}

/// How many observations of `tokens` the metric store still holds.
async fn metric_points(store: &SqliteStore, project: &str) -> usize {
    let q = MetricQuery {
        project: project.to_owned(),
        name: "tokens".to_owned(),
        from: time::macros::datetime!(2026 - 01 - 01 0:00 UTC),
        to: time::macros::datetime!(2027 - 01 - 01 0:00 UTC),
        bucket_seconds: 86_400,
        agg: Aggregation::Count,
        group_by: Vec::new(),
        filter: Vec::new(),
    };
    store
        .query_metrics(&q)
        .await
        .expect("query metrics")
        .series
        .iter()
        .map(|s| s.points.len())
        .sum()
}

/// The feedback still hanging off the run in `other`.
///
/// Read through `get_trace` rather than a list endpoint, for the same reason
/// every other count here is: the erasure must not be the thing that decides
/// what the test can see.
async fn other_feedback(store: &SqliteStore) -> usize {
    store
        .get_trace("other", ANN_OTHER, &Include { feedback: true, ..Include::default() })
        .await
        .expect("read the run in `other`")
        .feedback
        .unwrap_or_default()
        .len()
}

/// How many spans a full-text search for `term` still reaches.
///
/// This goes through `spans_fts`, so it fails when a span row is deleted but
/// its index entry is left behind, which would keep the erased prompt
/// searchable by its own content.
async fn spans_matching(store: &SqliteStore, project: &str, term: &str) -> usize {
    store
        .list_spans(&SpanQuery {
            project: project.to_owned(),
            q: Some(term.to_owned()),
            limit: 500,
            ..Default::default()
        })
        .await
        .expect("search spans")
        .items
        .len()
}

/// The same, for `logs_fts`.
async fn logs_matching(store: &SqliteStore, project: &str, term: &str) -> usize {
    store
        .list_logs(&LogQuery {
            project: project.to_owned(),
            q: Some(term.to_owned()),
            limit: 500,
            ..Default::default()
        })
        .await
        .expect("search logs")
        .items
        .len()
}

// ----------------------------------------------------------------- tests ---

/// The happy path, asserted on what is left rather than only on what was
/// reported. A count is a claim; the surviving rows are the fact.
#[tokio::test]
async fn erasing_a_subject_removes_the_whole_run_and_leaves_everyone_else() {
    let (app, store) = seeded(ServerConfig::default()).await;

    let res = send(&app, erase(ANN, "", None)).await;
    assert_eq!(res.status, StatusCode::OK, "erasure failed: {}", res.text());
    let body = res.json();

    // Per-signal counts are the point of the endpoint: an operator has to be
    // able to say what went.
    assert_eq!(body["project"], json!("default"), "{body}");
    assert_eq!(body["traces"], json!(1), "{body}");
    // Both spans, including the child that never carried the subject's id.
    assert_eq!(body["spans"], json!(2), "the payload span was left behind: {body}");
    // The attached log, the loose one, and the one that names only the span.
    assert_eq!(body["logs"], json!(3), "a log attached by span alone survived: {body}");
    assert_eq!(body["metrics"], json!(2), "a metric attached by span alone survived: {body}");
    assert_eq!(body["scores"], json!(2), "a score attached by span alone survived: {body}");
    // Two on Ann's run, and the one Ann wrote about Bob's.
    assert_eq!(body["feedback"], json!(3), "feedback the subject wrote survived: {body}");
    assert_eq!(body["deleted"], json!(ANN_TOTAL), "the total must be the sum: {body}");

    // What is left in `default`: Bob's run, whole.
    assert_eq!(trace_count(&store, "default").await, 1, "Bob's run should survive");
    assert_eq!(span_count(&store, "default").await, 1);
    assert_eq!(log_count(&store, "default").await, 1);
    assert_eq!(score_count(&store, "default").await, 1, "Bob's score should survive");
    assert_eq!(metric_points(&store, "default").await, 0, "the metric outlived its run");

    // Nothing was left behind on purpose and nothing is stored in another
    // form, so there is no count to explain.
    assert_eq!(body["dataset_items_erased"], json!(0), "{body}");
    assert_eq!(body["unmatchable_identifiers"], json!(0), "{body}");
    assert_eq!(body["deletes_pending"], json!(false), "{body}");

    // Bob is still here, and Bob is a digest. Nothing on disk says which key
    // wrote it, so the endpoint cannot tell Bob's digest from Ann's under an
    // earlier key, and it does not claim to. This reply used to carry no note,
    // which read as "that was all of her".
    let note = body["note"].as_str().unwrap_or_default();
    assert!(
        note.contains("Digests this request did not match"),
        "a count with digests left beside it was reported as the whole answer: {body}"
    );
    assert!(
        note.contains("not proof that nothing else about this person is held"),
        "the note does not say what the counts fail to prove: {body}"
    );
}

/// The prompt and the completion have to leave the full-text index as well as
/// the tables. A row deleted from `spans` but left in `spans_fts` is still
/// reachable by searching its own content, which is the one thing an erasure
/// request has to stop.
#[tokio::test]
async fn erasure_clears_the_full_text_index_as_well_as_the_tables() {
    let (app, store) = seeded(ServerConfig::default()).await;
    assert_eq!(
        spans_matching(&store, "default", ANN_PROMPT).await,
        2,
        "the fixture never indexed the prompt, so this test proves nothing"
    );
    assert_eq!(
        logs_matching(&store, "default", "audit").await,
        4,
        "the fixture never indexed the log messages"
    );

    let res = send(&app, erase(ANN, "", None)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    assert_eq!(
        spans_matching(&store, "default", ANN_PROMPT).await,
        0,
        "the erased prompt is still reachable through the full-text index"
    );
    assert_eq!(
        logs_matching(&store, "default", "audit").await,
        1,
        "an erased log message is still reachable through the full-text index"
    );
}

/// The case the whole design turns on.
///
/// The same subject id exists in `default` and in `other`. The key reaches
/// `default` only. `other` must keep every row, and must contribute nothing to
/// the counts: a count that moved would tell the caller this person exists in a
/// project they are not allowed to see.
#[tokio::test]
async fn a_subject_in_a_project_the_key_cannot_reach_is_untouched_and_uncounted() {
    let mut config = ServerConfig::default();
    config.projects.insert(
        "default".to_owned(),
        ProjectConfig { admin_keys: vec!["admin-default".to_owned()], ..ProjectConfig::default() },
    );
    config.projects.insert(
        "other".to_owned(),
        ProjectConfig { admin_keys: vec!["admin-other".to_owned()], ..ProjectConfig::default() },
    );
    let (app, store) = seeded(config).await;

    assert_eq!(span_count(&store, "other").await, 1, "the fixture must put Ann in `other` too");

    let res = send(&app, erase(ANN, "project=default", Some("admin-default"))).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body = res.json();
    assert_eq!(body["project"], json!("default"), "{body}");
    assert_eq!(body["traces"], json!(1), "a run from `other` was counted: {body}");
    assert_eq!(body["spans"], json!(2), "a span from `other` was counted: {body}");
    assert_eq!(body["logs"], json!(3), "a log from `other` was counted: {body}");
    assert_eq!(body["deleted"], json!(ANN_TOTAL), "{body}");

    // The other tenant is whole, including the feedback the same author wrote
    // there. Matching on `author_id` is a reach that does not go through a
    // trace, so it is the one most able to step over a project boundary.
    assert_eq!(body["feedback"], json!(3), "another tenant's feedback was counted: {body}");
    assert_eq!(
        other_feedback(&store).await,
        1,
        "the subject's feedback in a project the key cannot reach was erased"
    );
    assert_eq!(trace_count(&store, "other").await, 1, "another tenant's run was erased");
    assert_eq!(span_count(&store, "other").await, 1, "another tenant's span was erased");
    assert_eq!(log_count(&store, "other").await, 1, "another tenant's log was erased");
    assert_eq!(
        spans_matching(&store, "other", "checkout").await,
        1,
        "another tenant's span left the full-text index"
    );

    // And naming the other project does not reach it either.
    let refused = send(&app, erase(ANN, "project=other", Some("admin-default"))).await;
    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "a key for one project erased in another: {}",
        refused.text()
    );
    assert_eq!(span_count(&store, "other").await, 1, "the refused call still deleted");

    // The key that does reach `other` finds Ann there, which proves the rows
    // were left alone rather than quietly destroyed by the first call.
    let allowed = send(&app, erase(ANN, "project=other", Some("admin-other"))).await;
    assert_eq!(allowed.status, StatusCode::OK, "{}", allowed.text());
    assert_eq!(allowed.json()["spans"], json!(1), "{}", allowed.text());
    assert_eq!(span_count(&store, "other").await, 0);
}

/// A subject nothing is stored about is a 200 carrying zeroes, not a 404.
///
/// Two reasons. The caller asked for a state, and that state holds, so an error
/// would report a failure for a request that did exactly what was asked. And a
/// 404 would answer a question the caller is not entitled to ask: it would turn
/// the endpoint into an existence oracle over the subject id, telling an admin
/// key whether a person is known to a project. Zeroes are the same answer for
/// "never seen" and "erased an hour ago", which is the answer that leaks
/// nothing.
#[tokio::test]
async fn an_unknown_subject_is_a_200_of_zeroes_rather_than_a_404() {
    let (app, store) = seeded(ServerConfig::default()).await;

    let res = send(&app, erase("nobody-at-all", "", None)).await;
    assert_eq!(res.status, StatusCode::OK, "an unknown subject errored: {}", res.text());
    let body = res.json();
    assert_eq!(body["deleted"], json!(0), "{body}");
    assert_eq!(body["traces"], json!(0), "{body}");
    assert_eq!(body["spans"], json!(0), "{body}");
    assert_eq!(body["logs"], json!(0), "{body}");

    // Nothing was collateral damage on the way to that answer.
    assert_eq!(trace_count(&store, "default").await, 2);
    assert_eq!(span_count(&store, "default").await, 3);

    // Erasing the same person twice gives the same shape of answer the second
    // time, which is what lets a caller retry after a timeout.
    let first = send(&app, erase(ANN, "", None)).await;
    assert_eq!(first.json()["deleted"], json!(ANN_TOTAL), "{}", first.text());
    let again = send(&app, erase(ANN, "", None)).await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(again.json()["deleted"], json!(0), "a repeat must be a no-op: {}", again.text());
}

/// A read key must not be able to erase anyone.
///
/// `delete_trace` enforced the read scope once while the reference documented
/// admin, so a key advertised as read-only could destroy a run. This endpoint
/// destroys far more in one call, so the same mistake is worth its own test.
#[tokio::test]
async fn a_read_key_is_refused_and_nothing_is_deleted() {
    let config = ServerConfig {
        ingest_keys: vec!["ingest-key".to_owned()],
        read_keys: vec!["read-key".to_owned()],
        write_keys: vec!["write-key".to_owned()],
        admin_keys: vec!["admin-key".to_owned()],
        ..ServerConfig::default()
    };
    let (app, store) = seeded(config).await;

    for key in [None, Some("read-key"), Some("write-key"), Some("ingest-key")] {
        let res = send(&app, erase(ANN, "", key)).await;
        let expected = if key.is_none() { StatusCode::UNAUTHORIZED } else { StatusCode::FORBIDDEN };
        assert_eq!(res.status, expected, "key {key:?} was allowed to erase: {}", res.text());
    }
    // The refusals were real, not merely reported.
    assert_eq!(span_count(&store, "default").await, 3, "a refused call still deleted");
    assert_eq!(trace_count(&store, "default").await, 2);

    // The admin key does work, so the refusals above are not passing because
    // the route is broken for everyone.
    let ok = send(&app, erase(ANN, "", Some("admin-key"))).await;
    assert_eq!(ok.status, StatusCode::OK, "the admin key was refused too: {}", ok.text());
    assert_eq!(ok.json()["deleted"], json!(ANN_TOTAL), "{}", ok.text());
}

/// Stored ids are hashed, so the request's id has to be hashed the same way.
///
/// Comparing the raw path segment against the stored column matches nothing on
/// any deployment with redaction on, which is the default. The endpoint would
/// then report a confident row of zeroes for a person whose prompts are still
/// on disk. That is the worst failure this endpoint has, because it looks
/// exactly like success.
#[tokio::test]
async fn a_hashed_subject_id_still_matches_the_person_who_was_ingested() {
    let (app, store) = seeded(ServerConfig::default()).await;

    // The store filters on the column as stored. Handed the raw id, it finds
    // nothing, which is the proof that the column does not hold the raw id.
    let raw = store
        .list_traces(&TraceQuery {
            project: "default".to_owned(),
            user_id: Some(ANN.to_owned()),
            limit: 500,
            ..Default::default()
        })
        .await
        .expect("list traces")
        .items;
    assert!(raw.is_empty(), "the fixture is not exercising hashing at all");

    // The caller sends the id they set at ingest, and the handler hashes it.
    let res = send(&app, erase(ANN, "", None)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(
        res.json()["deleted"],
        json!(ANN_TOTAL),
        "the raw id did not reach the hashed column: {}",
        res.text()
    );
    assert_eq!(span_count(&store, "default").await, 1, "the subject's spans are still on disk");
}

/// A blank subject must be refused rather than matched.
///
/// ClickHouse stores `user_id` as a non-nullable `String`, so every row with no
/// user at all holds `''`. With redaction off the identifier passes through
/// unchanged, so a blank one would reach the backend as `user_id = ''` and
/// match every anonymous run in the project. The one endpoint that must never
/// over-delete would delete nearly everything.
#[tokio::test]
async fn a_blank_subject_is_refused_rather_than_matching_everything() {
    let (app, store) = seeded(ServerConfig::default()).await;

    for blank in ["%20", "%20%20", "%09"] {
        let res = send(&app, erase(blank, "", None)).await;
        assert_eq!(
            res.status,
            StatusCode::BAD_REQUEST,
            "a blank subject {blank:?} was accepted: {}",
            res.text()
        );
    }
    assert_eq!(span_count(&store, "default").await, 3, "a blank subject deleted rows");
    assert_eq!(trace_count(&store, "default").await, 2);
}

/// The erasure leaves an audit row, and that row does not carry the plaintext
/// identifier the request was about.
///
/// The audit log is deliberately outside the retention sweep, so anything
/// written there outlives the data it describes. Writing the raw subject id
/// would put the name back in a table nothing sweeps, straight after the
/// erasure took it away.
#[tokio::test]
async fn the_erasure_is_audited_without_writing_the_subject_back_down() {
    let (app, store) = seeded(ServerConfig::default()).await;

    let res = send(&app, erase(ANN, "", None)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let events = store
        .list_audit(&AuditQuery {
            project: "default".to_owned(),
            action: Some("erase_subject".to_owned()),
            limit: 10,
            ..Default::default()
        })
        .await
        .expect("read the audit log");
    assert_eq!(events.len(), 1, "the erasure was not audited");
    assert_eq!(events[0].project, "default");
    let resource = events[0].resource.clone().unwrap_or_default();
    assert!(!resource.is_empty(), "the audit row names no subject at all");
    assert_ne!(resource, ANN, "the audit log wrote the plaintext subject id back down");
}

/// A refused erasure is recorded too. An attempt to erase a tenant's data with
/// the wrong key is exactly the row an audit log exists for.
#[tokio::test]
async fn a_refused_erasure_is_recorded_as_refused() {
    let config = ServerConfig {
        read_keys: vec!["read-key".to_owned()],
        admin_keys: vec!["admin-key".to_owned()],
        ..ServerConfig::default()
    };
    let (app, store) = seeded(config).await;

    let res = send(&app, erase(ANN, "", Some("read-key"))).await;
    assert_eq!(res.status, StatusCode::FORBIDDEN, "{}", res.text());

    let events = store
        .list_audit(&AuditQuery {
            project: "default".to_owned(),
            action: Some("erase_subject".to_owned()),
            limit: 10,
            ..Default::default()
        })
        .await
        .expect("read the audit log");
    assert_eq!(events.len(), 1, "a refused erasure left no audit row");
    assert_eq!(
        events[0].result,
        mcpd_core::AuditResult::Refused,
        "a refused erasure was recorded as allowed"
    );
}

/// A score, a metric point and a log line can name a `span_id` and carry no
/// trace id at all. They hang off the subject's step and hold the subject's
/// content, and matching on the run alone left every one of them on disk.
///
/// The fixture is checked first through a filter the erasure does not use, so
/// this cannot pass by the rows never having carried a span id in the first
/// place.
#[tokio::test]
async fn signals_attached_by_span_alone_go_with_the_subject() {
    let (app, store) = seeded(ServerConfig::default()).await;

    // The row really is attached to the subject's step, and to nothing else.
    let on_span = store
        .list_logs(&LogQuery {
            project: "default".to_owned(),
            span_id: Some(ANN_MODEL.to_owned()),
            limit: 500,
            ..Default::default()
        })
        .await
        .expect("list logs by span")
        .items;
    assert_eq!(on_span.len(), 1, "the fixture never stored a span id, so this proves nothing");
    assert!(on_span[0].trace_id.is_none(), "the fixture's span-only log carries a trace after all");

    assert_eq!(log_count(&store, "default").await, 4);
    assert_eq!(score_count(&store, "default").await, 3);
    // 1.0 from the run-attached point and 2.0 from the span-attached one.
    assert!(
        (metric_total(&store, "default").await - 3.0).abs() < f64::EPSILON,
        "the fixture never stored the span-attached metric"
    );

    let res = send(&app, erase(ANN, "", None)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    // Bob's log and Bob's score are all that should be left.
    assert_eq!(log_count(&store, "default").await, 1, "a log attached by span alone survived");
    assert_eq!(score_count(&store, "default").await, 1, "a score attached by span alone survived");
    assert!(
        metric_total(&store, "default").await.abs() < f64::EPSILON,
        "a metric attached by span alone survived, worth {}",
        metric_total(&store, "default").await
    );
    assert_eq!(
        store
            .list_logs(&LogQuery {
                project: "default".to_owned(),
                span_id: Some(ANN_MODEL.to_owned()),
                limit: 500,
                ..Default::default()
            })
            .await
            .expect("list logs by span")
            .items
            .len(),
        0,
        "the subject's step still has a log line hanging off it"
    );
}

/// Feedback the subject wrote carries the person's own prose, and it can hang
/// off somebody else's run. It is matched on `author_id`, which the intake
/// hashes exactly as it hashes `user_id`.
///
/// Bob's run is the control: it must lose Ann's comment and keep everything of
/// its own, so this cannot pass by deleting the run the comment sat on.
#[tokio::test]
async fn feedback_the_subject_wrote_goes_and_the_other_run_survives() {
    let (app, store) = seeded(ServerConfig::default()).await;

    let bob_before = store
        .get_trace("default", BOB_RUN, &Include { feedback: true, ..Include::default() })
        .await
        .expect("read Bob's run");
    let comments = bob_before.feedback.clone().unwrap_or_default();
    assert_eq!(comments.len(), 1, "the fixture never put Ann's comment on Bob's run");
    assert_eq!(
        comments[0].value.as_str(),
        Some("the refund never arrived"),
        "the fixture's comment is not the one this test is about"
    );

    let res = send(&app, erase(ANN, "", None)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let bob_after = store
        .get_trace("default", BOB_RUN, &Include { feedback: true, ..Include::default() })
        .await
        .expect("Bob's run was erased along with the comment");
    assert_eq!(
        bob_after.feedback.unwrap_or_default().len(),
        0,
        "the subject's own words survived on another person's run"
    );
    // The control: Bob kept everything else.
    assert_eq!(bob_after.spans.len(), 1, "Bob's run lost a span");
    assert_eq!(trace_count(&store, "default").await, 1, "Bob's run was erased");
    assert_eq!(score_count(&store, "default").await, 1, "Bob's score was erased");
}

/// A run captured into a dataset leaves a verbatim copy of the production
/// input behind. Erasing the person has to take the copy too.
///
/// The case is emptied rather than deleted, because deleting a regression test
/// nobody was told about is its own harm, and the reply has to say how many
/// were emptied and which datasets they are in.
#[tokio::test]
async fn a_captured_case_does_not_keep_the_subject_prompt() {
    let (app, store) = seeded(ServerConfig::default()).await;

    let made = send(
        &app,
        post(
            "/v1/api/datasets",
            json!({
                "dataset_id": "regressions",
                "name": "refund regressions",
                "item_count": 0,
                "created_at": format!("{DAY}T00:00:00Z"),
                "correlation": { "project": "default" },
            })
            .to_string(),
            None,
        ),
    )
    .await;
    assert_eq!(made.status, StatusCode::OK, "creating the dataset failed: {}", made.text());

    let captured = send(
        &app,
        post(
            "/v1/api/datasets/regressions/capture",
            json!({ "trace_ids": [ANN_RUN] }).to_string(),
            None,
        ),
    )
    .await;
    assert_eq!(captured.status, StatusCode::OK, "{}", captured.text());
    assert_eq!(captured.json()["captured"], json!(1), "{}", captured.text());

    // The captured case holds the prompt verbatim. Without this the test would
    // pass against a capture that stored nothing.
    let items = store.list_dataset_items("regressions", 500).await.expect("read the cases");
    assert_eq!(items.len(), 1);
    assert!(
        items[0].input.to_string().contains(ANN_PROMPT),
        "the fixture never captured the prompt, so this proves nothing: {:?}",
        items[0].input
    );

    let res = send(&app, erase(ANN, "", None)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body = res.json();

    let items = store.list_dataset_items("regressions", 500).await.expect("read the cases");
    assert_eq!(items.len(), 1, "the regression case was deleted rather than emptied");
    assert!(
        !items[0].input.to_string().contains(ANN_PROMPT),
        "the erased prompt is still sitting in the dataset: {:?}",
        items[0].input
    );
    assert!(
        items[0].source_trace_id.is_none(),
        "the emptied case still points at the erased run: {:?}",
        items[0].source_trace_id
    );

    // The reply says how many were left and where, because an operator has to
    // be able to tell somebody what happened to them.
    assert_eq!(body["dataset_items_erased"], json!(1), "{body}");
    assert_eq!(body["datasets"], json!(["regressions"]), "{body}");
    assert!(
        body["note"].as_str().unwrap_or_default().contains("dataset"),
        "the reply does not mention the cases at all: {body}"
    );

    // And a repeat does not report the same cases again.
    let again = send(&app, erase(ANN, "", None)).await;
    assert_eq!(again.json()["dataset_items_erased"], json!(0), "{}", again.text());
}

/// A zero is the most dangerous number this endpoint returns, because it is
/// read as "we hold nothing about you". Stored identifiers are hashed and
/// nothing records which key hashed which row, so a zero can equally mean the
/// key moved. The reply has to say so.
#[tokio::test]
async fn a_zero_that_cannot_be_justified_says_why() {
    let (app, _store) = seeded(ServerConfig::default()).await;

    let res = send(&app, erase("nobody-at-all", "", None)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body = res.json();
    assert_eq!(body["deleted"], json!(0), "{body}");
    let note = body["note"].as_str().unwrap_or_default();
    assert!(
        note.contains("redaction key"),
        "a zero came back with nothing to say for itself: {body}"
    );
    assert!(
        note.contains("does not mean nothing about this person was ever stored"),
        "the note does not say what the zero fails to prove: {body}"
    );
}

/// The one zero the endpoint can stand behind: a project that holds no
/// identifiers at all. There is no row a key change could be hiding, so the
/// note says that instead of the caveat.
#[tokio::test]
async fn a_zero_in_an_empty_project_is_reported_as_provable() {
    let (app, _store) = seeded(ServerConfig::default()).await;

    let res = send(&app, erase("nobody-at-all", "project=empty", None)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let note = res.json()["note"].as_str().unwrap_or_default().to_owned();
    assert!(
        note.contains("no stored identifier a key change could be hiding"),
        "the endpoint would not commit to the one thing it can prove: {}",
        res.text()
    );
    // And it commits to no more than that. The sentence used to end "so
    // nothing about this person was stored here", which is false in a project
    // that never sets a user id: every run there is unattributed, and
    // unattributed is not absent.
    assert!(
        note.contains("not the same as holding nothing about this person"),
        "a project with no identifiers was reported as holding nothing: {}",
        res.text()
    );
    assert!(
        !note.contains("redaction key"),
        "a provable zero was reported with a caveat it does not need: {}",
        res.text()
    );
}

/// Turning redaction off, or on, leaves the older rows addressed by an
/// identifier this endpoint can no longer produce. It cannot erase them and it
/// must not pretend they are not there.
///
/// The rows are seeded through a daemon with redaction off, so they hold the
/// plaintext identifier, and then erased through one with redaction on, which
/// hashes it. That is the `--no-redact` flip, reproduced.
#[tokio::test]
async fn rows_written_under_another_redaction_setting_are_reported() {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");

    let plain = AppState::new(Arc::clone(&store) as Arc<dyn Store>, ServerConfig::default())
        .with_redactor(mcpd_core::redact::Redactor::disabled());
    let writer = router(Arc::new(plain));
    let res = send(&writer, post("/v1/ingest", seed_batch().to_string(), None)).await;
    assert_eq!(res.status, StatusCode::OK, "seeding failed: {}", res.text());

    // The identifier really did land in plaintext, so the flip is real.
    let raw = store
        .list_traces(&TraceQuery {
            project: "default".to_owned(),
            user_id: Some(ANN.to_owned()),
            limit: 500,
            ..Default::default()
        })
        .await
        .expect("list traces")
        .items;
    assert_eq!(raw.len(), 1, "the fixture is not exercising the unhashed form at all");

    // Now the same store, read by a daemon that hashes. Nothing can match.
    let hashing = router(Arc::new(AppState::new(
        Arc::clone(&store) as Arc<dyn Store>,
        ServerConfig::default(),
    )));
    let res = send(&hashing, erase(ANN, "", None)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body = res.json();
    assert_eq!(body["deleted"], json!(0), "the hashed id matched a plaintext row: {body}");
    assert!(
        body["unmatchable_identifiers"].as_u64().unwrap_or(0) >= 2,
        "rows this request could never match were not counted: {body}"
    );
    let note = body["note"].as_str().unwrap_or_default();
    assert!(
        note.contains("could not match"),
        "a zero over unreachable rows came back looking like an answer: {body}"
    );
    assert!(note.contains("Restore the redaction setting"), "the note names no way out: {body}");

    // The rows are still there, which is the point: the endpoint said so
    // rather than reporting a zero that reads as nothing being held.
    assert_eq!(span_count(&store, "default").await, 3, "rows went that could not be matched");
}

/// A span id is unique inside its trace, not inside a project. Another run
/// that reuses one must keep its own scores and its own logs.
///
/// The erasure matched attached signals on `span_id IN (...)` with nothing but
/// `project` beside it, so an unrelated run that happened to reuse one of the
/// subject's span ids lost the rows hanging off it. The comment there called
/// that collision harmless because it stayed "inside one tenant where the run
/// is already being erased". The colliding run is not the one being erased.
#[tokio::test]
async fn a_span_id_reused_by_another_run_keeps_its_own_signals() {
    let (app, store) = seeded(ServerConfig::default()).await;

    // Carol's run reuses ANN_MODEL as its span id, and hangs a score, a log
    // and a metric off it. Every one of those names Carol's trace as well.
    let collision = json!({
        "spans": [span(CAROL_RUN, ANN_MODEL, None, "default", Some(CAROL), json!({}))],
        "logs": [
            {
                "log_id": "log-carol",
                "trace_id": CAROL_RUN,
                "span_id": ANN_MODEL,
                "timestamp": format!("{DAY}T00:00:00Z"),
                "level": "info",
                "message": "audit line log-carol",
                "correlation": { "project": "default" },
                "received_at": format!("{DAY}T00:10:00Z"),
            },
            // The same id with no trace to place it. Two runs share the span,
            // so nothing on this row says whose it is.
            log_on_span("log-carol-loose", ANN_MODEL, "default"),
        ],
        "scores": [{
            "score_id": "score-carol", "trace_id": CAROL_RUN, "span_id": ANN_MODEL,
            "scorer_id": "helpfulness", "source": "live", "value": 0.5,
            "timestamp": format!("{DAY}T00:00:00Z"),
            "correlation": { "project": "default" },
        }],
    });
    let res = send(&app, post("/v1/ingest", collision.to_string(), None)).await;
    assert_eq!(
        res.json()["accepted"],
        json!(4),
        "the collision fixture did not land: {}",
        res.text()
    );

    // The collision is real: Carol's score hangs off the same span id as the
    // subject's payload span, in the same project.
    let carol = store
        .get_trace("default", CAROL_RUN, &Include { scores: true, logs: true, feedback: false })
        .await
        .expect("read Carol's run");
    assert_eq!(carol.spans.len(), 1);
    assert_eq!(carol.spans[0].span_id, ANN_MODEL, "the fixture does not reuse the span id");
    let scores = carol.scores.clone().unwrap_or_default();
    assert_eq!(scores.len(), 1, "the fixture never scored Carol's run");
    assert_eq!(scores[0].span_id.as_deref(), Some(ANN_MODEL), "the score names another span");

    let res = send(&app, erase(ANN, "", None)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let carol = store
        .get_trace("default", CAROL_RUN, &Include { scores: true, logs: true, feedback: false })
        .await
        .expect("Carol's run was erased with the subject's");
    assert_eq!(carol.spans.len(), 1, "another run's span went with the subject");
    assert_eq!(
        carol.scores.unwrap_or_default().len(),
        1,
        "another run's score went with the subject, because it reused a span id"
    );
    assert_eq!(
        carol.logs.unwrap_or_default().len(),
        1,
        "another run's log went with the subject, because it reused a span id"
    );

    // Bob's log, Carol's log. `log-carol-loose` is gone: it names a span id two
    // runs share and no trace at all, so nothing on it says whose it is, and an
    // erasure takes the ambiguous row rather than leaving the subject's content
    // on disk. That is the limit of this fix, not an oversight.
    assert_eq!(log_count(&store, "default").await, 2, "the wrong logs survived");
}

/// The case the probe was written for, and the one it could not see.
///
/// Rotating the redaction key changes what every identifier hashes to. The
/// subject's older rows keep a digest no request can produce any more, so an
/// erasure reaches the rows written since the rotation and nothing else. Both
/// forms are digests, so nothing looks mixed, `unmatchable_identifiers` stays
/// at zero, and the reply used to be indistinguishable from a complete one.
#[tokio::test]
async fn a_partial_erasure_after_a_key_rotation_is_not_reported_as_complete() {
    use mcpd_core::redact::Redactor;

    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    // Derivation off for the same reason `seeded` turns it off: the counts
    // below are arithmetic over a fixture written by hand, and a metric the
    // daemon adds for itself is not in it.
    let config = ServerConfig {
        derived_metrics: mcpd_server::derive::DerivedMetrics::off(),
        ..ServerConfig::default()
    };
    let under = |key: &str| {
        router(Arc::new(
            AppState::new(Arc::clone(&store) as Arc<dyn Store>, config.clone())
                .with_redactor(Redactor::with_defaults().with_hash_key(key.to_owned())),
        ))
    };

    // The runs the person made before the rotation.
    let before = under("key-before");
    let res = send(&before, post("/v1/ingest", seed_batch().to_string(), None)).await;
    assert_eq!(res.status, StatusCode::OK, "seeding failed: {}", res.text());

    // The key moves, and the person runs the agent again.
    let after = under("key-after");
    let later = json!({
        "spans": [span(ANN_LATER, ANN_LATER_ROOT, None, "default", Some(ANN), json!({}))],
    });
    let res = send(&after, post("/v1/ingest", later.to_string(), None)).await;
    assert_eq!(res.json()["accepted"], json!(1), "the later run did not land: {}", res.text());

    // Both digests are in the column now, and they differ, which is what makes
    // this a rotation rather than a redaction setting that changed.
    assert_eq!(span_count(&store, "default").await, 4);

    let res = send(&after, erase(ANN, "", None)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body = res.json();

    // The erasure worked, as far as it could reach.
    assert_eq!(
        body["deleted"],
        json!(2),
        "the run written under the new key was not erased: {body}"
    );
    assert_eq!(
        body["unmatchable_identifiers"],
        json!(0),
        "a rotation leaves both forms hashed, so this count cannot see it: {body}"
    );

    // And the subject's older rows are still on disk.
    assert_eq!(
        span_count(&store, "default").await,
        3,
        "the rows written under the earlier key were reachable after all"
    );

    let note = body["note"].as_str().unwrap_or_default();
    assert!(
        note.contains("Digests this request did not match"),
        "a partial erasure was reported as a complete one: {body}"
    );
    assert!(
        note.contains("not proof that nothing else about this person is held"),
        "the note does not say what the counts fail to prove: {body}"
    );
}

/// The other half of that gate, so the caveat is not a sentence every erasure
/// carries. With no identifier left in the project, there is nothing a key
/// change could be hiding and the reply says nothing at all.
#[tokio::test]
async fn an_erasure_that_leaves_no_identifier_behind_carries_no_caveat() {
    let (app, _store) = seeded(ServerConfig::default()).await;

    let first = send(&app, erase(ANN, "", None)).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.text());
    assert!(
        first.json()["note"].as_str().unwrap_or_default().contains("Digests"),
        "the caveat is missing while another subject is still stored: {}",
        first.text()
    );

    let second = send(&app, erase(BOB, "", None)).await;
    assert_eq!(second.status, StatusCode::OK, "{}", second.text());
    let body = second.json();
    assert!(body["deleted"].as_u64().unwrap_or(0) > 0, "{body}");
    assert_eq!(
        body["note"],
        Value::Null,
        "an erasure with nothing left beside it still warned about a key: {body}"
    );
}

/// A caller who hashes identifiers before sending them is not a deployment
/// whose redaction setting changed.
///
/// The probe recognised a hashed identifier by its `sha256:` prefix alone, so a
/// caller's own digest counted as a row written under another setting and every
/// erasure in that deployment came back telling the operator to restore a
/// setting they had never changed. The redactor writes a fixed shape: the
/// prefix and 24 hex characters. A caller's full digest is 64.
#[tokio::test]
async fn an_identifier_the_caller_hashed_is_not_a_changed_redaction_setting() {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let app = router(Arc::new(
        AppState::new(Arc::clone(&store) as Arc<dyn Store>, ServerConfig::default())
            .with_redactor(mcpd_core::redact::Redactor::disabled()),
    ));

    let res = send(&app, post("/v1/ingest", seed_batch().to_string(), None)).await;
    assert_eq!(res.status, StatusCode::OK, "seeding failed: {}", res.text());
    // Somebody else's run, identified by a digest the caller made themselves.
    let pre_hashed = format!("sha256:{}", "ab".repeat(32));
    let own = json!({
        "spans": [span(CAROL_RUN, "d000000000000001", None, "default", Some(&pre_hashed), json!({}))],
    });
    let res = send(&app, post("/v1/ingest", own.to_string(), None)).await;
    assert_eq!(res.json()["accepted"], json!(1), "{}", res.text());

    let res = send(&app, erase(ANN, "", None)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body = res.json();
    assert!(body["deleted"].as_u64().unwrap_or(0) > 0, "the fixture erased nothing: {body}");
    assert_eq!(
        body["unmatchable_identifiers"],
        json!(0),
        "a caller's own digest was counted as a redaction setting that changed: {body}"
    );
    assert_eq!(
        body["note"],
        Value::Null,
        "an erasure with one form of identifier was reported with a caveat: {body}"
    );
}

/// An identifier a caller hashed themselves is a row this request can never
/// match, and it has to be counted as one.
///
/// The probe narrowed the rows it read on the `sha256:` prefix and then asked
/// [`mcpd_store::unmatchable_identifiers`] about what survived. The two did not
/// agree. A caller's own digest carries the prefix and is 64 characters, so the
/// rule calls it the other form and the prefix narrowing called it hashed and
/// dropped it. The count that exists to report an unreachable row reported
/// zero, the operator got the caveat nobody can act on instead of the one that
/// names a way out, and the daemon logged it at INFO rather than WARN.
#[tokio::test]
async fn a_digest_the_caller_wrote_is_counted_as_a_row_this_request_cannot_match() {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");

    // Written while redaction was off, so the caller's own digest lands in the
    // column verbatim. This is the same flip
    // `rows_written_under_another_redaction_setting_are_reported` uses.
    let plain = router(Arc::new(
        AppState::new(Arc::clone(&store) as Arc<dyn Store>, ServerConfig::default())
            .with_redactor(mcpd_core::redact::Redactor::disabled()),
    ));
    let callers_own = format!("sha256:{}", "ab".repeat(32));
    let earlier = json!({
        "spans": [span(CAROL_RUN, "d000000000000001", None, "default",
                       Some(&callers_own), json!({}))],
    });
    let res = send(&plain, post("/v1/ingest", earlier.to_string(), None)).await;
    assert_eq!(res.json()["accepted"], json!(1), "the fixture did not land: {}", res.text());

    // Redaction is turned on. The subject runs again, then asks to be erased,
    // so the erasure matches rows and the "nothing matched" arm is not the one
    // under test.
    let hashing = router(Arc::new(AppState::new(
        Arc::clone(&store) as Arc<dyn Store>,
        ServerConfig::default(),
    )));
    let later = json!({
        "spans": [span(ANN_LATER, ANN_LATER_ROOT, None, "default", Some(ANN), json!({}))],
    });
    let res = send(&hashing, post("/v1/ingest", later.to_string(), None)).await;
    assert_eq!(res.json()["accepted"], json!(1), "the later run did not land: {}", res.text());

    let res = send(&hashing, erase(ANN, "", None)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body = res.json();
    assert!(body["deleted"].as_u64().unwrap_or(0) > 0, "the fixture erased nothing: {body}");

    assert_eq!(
        body["unmatchable_identifiers"],
        json!(1),
        "a row this request could never match was not counted: {body}"
    );
    let note = body["note"].as_str().unwrap_or_default();
    assert!(
        note.contains("Restore the redaction setting"),
        "the reply names no way out of a row it cannot reach: {body}"
    );
    // And the row really is still there, which is why it had to be reported.
    assert_eq!(span_count(&store, "default").await, 1, "{body}");
}

/// The other half of the same gate. A digest the redactor wrote is the shape
/// the probe is looking for, so it must still be read as hashed rather than
/// counted as a row in another form.
#[tokio::test]
async fn a_digest_the_redactor_wrote_is_not_counted_as_another_form() {
    let (app, _store) = seeded(ServerConfig::default()).await;

    let body = send(&app, erase(ANN, "", None)).await.json();
    assert!(body["deleted"].as_u64().unwrap_or(0) > 0, "{body}");
    assert_eq!(
        body["unmatchable_identifiers"],
        json!(0),
        "the other subject's own digest was read as a changed redaction setting: {body}"
    );
    assert!(
        body["note"].as_str().unwrap_or_default().contains("Digests this request did not match"),
        "{body}"
    );
}
