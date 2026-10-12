//! Per-endpoint coverage for the routes that change stored data.
//!
//! `queries.rs` does this for the read surface. This file does it for the
//! write surface, which is the half where a defect costs something: a score
//! attached to the wrong run, a case written into another team's regression
//! suite, a secret stored in the clear, a retry counted twice.
//!
//! Every request goes through `mcpd_server::router`, so the extractor, the
//! authorisation check, the intake pipeline and the store are all under test.
//! Where a handler claims something about what reached storage, the assertion
//! reads it back through the matching read endpoint rather than trusting the
//! response body, because the response is what the handler believed and the
//! read is what actually landed.
//!
//! Six cases here were once marked `#[ignore]`, each naming a gap the handler
//! had, each run and observed failing before it was marked. The handlers have
//! since been fixed and every case runs; nothing in this file is ignored. The
//! idiom is kept for the next gap: mark the case, name what was observed, and
//! keep a companion beside it that pins what the endpoint does today.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mcpd_server::monitor::{Compare, Monitor, Slo, Watch};
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::Store;
use serde_json::{json, Value};
use std::sync::Arc;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tower::ServiceExt;

// ------------------------------------------------------------ seed data ---
//
// SEED DATA, project `default`. Two runs and one dataset, which is the least a
// write test needs: something to attach a score to, something that failed so a
// captured case has a status worth recording, and somewhere to put a case.
//
// run         root span   name      status  input                output
// RUN_OK      OK_ROOT     checkout  ok      "refund the order"   "the refund is queued"
// RUN_FAILED  FAILED_ROOT checkout  error   "refund the order"   none

/// The day every seeded timestamp falls on.
const DAY: &str = "2026-04-01";

const RUN_OK: &str = "a0000000000000000000000000000001";
const RUN_FAILED: &str = "b0000000000000000000000000000002";
const OK_ROOT: &str = "a000000000000001";
const FAILED_ROOT: &str = "b000000000000001";

/// A well-formed trace id that was never ingested.
const MISSING_RUN: &str = "cccccccccccccccccccccccccccccccc";
/// A well-formed span id that was never ingested.
const MISSING_SPAN: &str = "cccccccccccccccc";

/// The dataset the fixture creates, so the item and capture routes have a real
/// id to work against. Both resolve the owning project from the store, and an
/// id with nothing behind it is a 404 before anything else happens.
const DATASET: &str = "regressions";

const MONITOR_ID: &str = "checkout-errors";
const SLO_ID: &str = "checkout-availability";

const READ_KEY: &str = "read-only-key";
const INGEST_KEY: &str = "ingest-only-key";
const WRITE_KEY: &str = "change-stored-data-key";
const ADMIN_KEY: &str = "destroy-and-audit-key";
/// A syntactically fine key that appears in no list at all. The difference
/// between this and [`READ_KEY`] is the difference between 401 and 403.
const STRANGER: &str = "a-key-nobody-configured";

// ------------------------------------------------------------- fixtures ---
//
// `temp_db`, `Res`, `send`, `get`, `post` and the two key configurations are
// copies of the helpers in `http.rs`. They are duplicated rather than shared
// because Cargo compiles each integration test as its own crate, and lifting
// them into a shared module would mean editing files this change does not own.

/// A private database file per test.
///
/// Not `:memory:`, deliberately: `SqliteStore::open` gives every in-memory
/// store the same shared-cache URI, so two tests running on two threads would
/// share one database and race on `migrate`.
///
/// The directory is named for the process **and** the instant it started, which
/// is the one place this deviates from the helper it was copied from. A name
/// built from the pid alone is reused when the operating system recycles one,
/// and the second run then opens the first run's databases: every count is off
/// by a whole previous suite and eighty-nine tests fail at once for a reason
/// that has nothing to do with the code. That happened here before this line
/// existed.
fn temp_db() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::OnceLock;
    static N: AtomicU64 = AtomicU64::new(0);
    static DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
    let dir = DIR.get_or_init(|| {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir =
            std::env::temp_dir().join(format!("mcpd-write-test-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create the test directory");
        dir
    });
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

/// One monitor and one objective, so the two acknowledgement routes have a real
/// id as well as an unknown one.
fn alerts() -> (Vec<Monitor>, Vec<Slo>) {
    (
        vec![Monitor {
            id: MONITOR_ID.to_owned(),
            name: "checkout errors".to_owned(),
            project: "default".to_owned(),
            // TraceCount below a threshold no fixture can reach, so one seeded
            // run is enough to trip it. The arithmetic under test is the
            // latching rule, not the measurement.
            watch: Watch::TraceCount,
            scorer_id: None,
            compare: Compare::Below,
            threshold: 100.0,
            window_seconds: 3600,
            min_samples: 1,
            z_bound: 3.5,
            webhook: None,
            enabled: true,
        }],
        vec![Slo {
            id: SLO_ID.to_owned(),
            name: "checkout availability".to_owned(),
            project: "default".to_owned(),
            objective: 0.99,
            window_days: 30,
            min_samples: 1,
            webhook: None,
            enabled: true,
        }],
    )
}

/// The two-scope shape every deployment written before `write` and `admin`
/// existed still has. Write falls back to the ingest keys here.
fn keyed() -> ServerConfig {
    let (monitors, slos) = alerts();
    ServerConfig {
        ingest_keys: vec![INGEST_KEY.to_owned()],
        read_keys: vec![READ_KEY.to_owned()],
        monitors,
        slos,
        ..ServerConfig::default()
    }
}

/// All four scopes with their own keys, so no scope inherits from another.
fn fully_keyed() -> ServerConfig {
    ServerConfig {
        write_keys: vec![WRITE_KEY.to_owned()],
        admin_keys: vec![ADMIN_KEY.to_owned()],
        ..keyed()
    }
}

/// A config with no keys at all, and the alerts configured.
fn open() -> ServerConfig {
    let (monitors, slos) = alerts();
    ServerConfig { monitors, slos, ..ServerConfig::default() }
}

fn root_span(trace: &str, span: &str, status: &str, output: Option<&str>) -> Value {
    let mut v = json!({
        "trace_id": trace,
        "span_id": span,
        "name": "checkout",
        "kind": "agent_run",
        "status": status,
        "start_time": format!("{DAY}T00:00:00Z"),
        "end_time": format!("{DAY}T00:00:01Z"),
        "is_root": true,
        "is_event": false,
        "input": { "prompt": "refund the order" },
        "correlation": { "project": "default" },
        "received_at": format!("{DAY}T00:00:02Z"),
    });
    if let Some(text) = output {
        v["output"] = json!({ "text": text });
    }
    v
}

/// A recent run, for the two conditions evaluated over a window ending now.
///
/// The seeded fixture is dated, deliberately, so nothing else in this file
/// depends on the clock. A monitor asks what happened in the last hour, so it
/// needs its own run.
async fn seed_a_run_inside_the_alert_windows(app: &Router, status: &str, now: OffsetDateTime) {
    let at = (now - time::Duration::minutes(5)).format(&Rfc3339).expect("format the time");
    let batch = json!({ "spans": [{
        "trace_id": "d0000000000000000000000000000004",
        "span_id": "d000000000000001",
        "name": "checkout", "kind": "agent_run", "status": status,
        "start_time": at, "end_time": at,
        "is_root": true, "is_event": false,
        "correlation": { "project": "default" },
        "received_at": now.format(&Rfc3339).expect("format the time")
    }]});
    let res = send(app, post("/v1/ingest", None, batch.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "seeding a recent run failed: {}", res.text());
    assert_eq!(res.json()["accepted"], json!(1), "{}", res.text());
}

fn seed_batch() -> Value {
    json!({
        "spans": [
            root_span(RUN_OK, OK_ROOT, "ok", Some("the refund is queued")),
            root_span(RUN_FAILED, FAILED_ROOT, "error", None),
        ]
    })
}

/// The router, the state behind it and the store behind that.
///
/// The state is handed back because two of the routes here act on state that
/// lives in memory rather than in the database: acknowledging a monitor changes
/// a latch that only `monitor::tick` can set, and the honest way to reach the
/// tripped condition is to run the real evaluation.
async fn app_state_and_store(config: ServerConfig) -> (Router, Arc<AppState>, Arc<SqliteStore>) {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let state = Arc::new(AppState::new(Arc::clone(&store) as Arc<dyn Store>, config));
    (router(Arc::clone(&state)), state, store)
}

/// The router alone, with the two runs and the dataset already in place.
async fn seeded(config: ServerConfig) -> Router {
    seeded_with_state(config).await.0
}

/// The same, keeping the state and the store.
async fn seeded_with_state(config: ServerConfig) -> (Router, Arc<AppState>, Arc<SqliteStore>) {
    let ingest = if config.ingest_keys.is_empty() { None } else { Some(INGEST_KEY) };
    let (app, state, store) = app_state_and_store(config).await;

    let res = send(&app, post("/v1/ingest", ingest, seed_batch().to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "seeding failed: {}", res.text());
    assert_eq!(res.json()["accepted"], json!(2), "the fixture did not land: {}", res.text());

    // Written through the store rather than the API: the fixture must exist
    // before any authorisation test runs, and creating it through the API would
    // make every one of those tests depend on the create route working.
    store
        .put_dataset(&mcpd_core::Dataset {
            dataset_id: DATASET.to_owned(),
            name: "captured regressions".to_owned(),
            description: None,
            item_count: 0,
            created_at: OffsetDateTime::UNIX_EPOCH,
            correlation: correlation_for("default"),
        })
        .await
        .expect("seed the fixture dataset");

    (app, state, store)
}

fn correlation_for(project: &str) -> mcpd_core::Correlation {
    mcpd_core::Correlation {
        project: project.to_owned(),
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
    }
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

    /// The `error` code and `message` of a failure body, asserting the shape
    /// `error.rs` promises every non-2xx response carries.
    fn error(&self) -> (String, String) {
        let v = self.json();
        let code = v["error"].as_str().unwrap_or_else(|| {
            panic!("an error body must carry a machine-readable `error`, got: {v}")
        });
        let message = v["message"].as_str().unwrap_or_else(|| {
            panic!("an error body must carry a `message` a human can act on, got: {v}")
        });
        (code.to_owned(), message.to_owned())
    }
}

async fn send(app: &Router, req: Request<Body>) -> Res {
    let res = app.clone().oneshot(req).await.expect("the router returned a response");
    let status = res.status();
    let body = res.into_body().collect().await.expect("read the response body").to_bytes().to_vec();
    Res { status, body }
}

fn get(uri: &str, key: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().method("GET").uri(uri);
    if let Some(k) = key {
        b = b.header("authorization", format!("Bearer {k}"));
    }
    b.body(Body::empty()).expect("build the request")
}

fn post(uri: &str, key: Option<&str>, body: impl Into<Vec<u8>>) -> Request<Body> {
    let body: Vec<u8> = body.into();
    let mut b = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        // A real client always sends this, and the body limit reads it.
        .header("content-length", body.len().to_string());
    if let Some(k) = key {
        b = b.header("authorization", format!("Bearer {k}"));
    }
    b.body(Body::from(body)).expect("build the request")
}

/// Every score in the project, newest first.
async fn stored_scores(app: &Router, key: Option<&str>) -> Vec<Value> {
    let res = send(app, get("/v1/api/scores?limit=500", key)).await;
    assert_eq!(res.status, StatusCode::OK, "reading the scores back failed: {}", res.text());
    res.json()["scores"].as_array().cloned().unwrap_or_default()
}

/// Every piece of feedback on one run.
///
/// The listing is keyed on the run: `GET /v1/api/feedback` with no `trace_id`
/// serves an empty page by design, because feedback is read from the trace it
/// is about. That also means feedback naming only a span has no read path at
/// all, which is why the span-only case below asserts on the reply.
async fn stored_feedback(app: &Router, trace: &str, key: Option<&str>) -> Vec<Value> {
    let res = send(app, get(&format!("/v1/api/feedback?trace_id={trace}&limit=500"), key)).await;
    assert_eq!(res.status, StatusCode::OK, "reading the feedback back failed: {}", res.text());
    res.json()["feedback"].as_array().cloned().unwrap_or_default()
}

/// Every case in the fixture dataset.
async fn stored_items(app: &Router, key: Option<&str>) -> Vec<Value> {
    let res = send(app, get(&format!("/v1/api/datasets/{DATASET}/items"), key)).await;
    assert_eq!(res.status, StatusCode::OK, "reading the items back failed: {}", res.text());
    res.json().as_array().cloned().unwrap_or_default()
}

/// A score body with the fields the endpoint requires, and nothing else.
fn a_score() -> Value {
    json!({
        "score_id": "",
        "trace_id": RUN_OK,
        "scorer_id": "helpfulness",
        "source": "manual",
        "value": 0.75,
        "timestamp": format!("{DAY}T00:01:00Z"),
        "correlation": { "project": "default" }
    })
}

/// A feedback body with the fields the endpoint requires, and nothing else.
fn a_feedback() -> Value {
    json!({
        "feedback_id": "",
        "trace_id": RUN_OK,
        "kind": "thumbs",
        "value": "up",
        "timestamp": format!("{DAY}T00:01:00Z"),
        "correlation": { "project": "default" }
    })
}

fn a_dataset(id: &str, name: &str, project: &str) -> Value {
    json!({
        "dataset_id": id,
        "name": name,
        "item_count": 0,
        "created_at": format!("{DAY}T00:00:00Z"),
        "correlation": { "project": project }
    })
}

fn an_item(input: Value) -> Value {
    json!({ "item_id": "", "dataset_id": "", "input": input })
}

fn an_experiment(id: &str, name: &str, dataset: &str, project: &str) -> Value {
    json!({
        "experiment_id": id,
        "name": name,
        "dataset_id": dataset,
        "status": "pending",
        "item_count": 0,
        "correlation": { "project": project }
    })
}

/// Every route that changes stored data, with a body valid enough to reach the
/// authorisation check rather than failing in the extractor.
///
/// Axum runs the body extractor before the handler, so a route whose body does
/// not deserialise never gets as far as the key check and would prove nothing
/// about it. A route added to `lib.rs` and forgotten here is a route whose
/// scope nobody checks.
fn write_routes() -> Vec<(&'static str, String)> {
    vec![
        ("/v1/api/scores", a_score().to_string()),
        ("/v1/api/feedback", a_feedback().to_string()),
        ("/v1/api/datasets", a_dataset("", "a new set", "default").to_string()),
        // An empty list of cases: enough to authorise, and it stores nothing, so
        // running the whole table against one router leaves no residue.
        ("/v1/api/datasets/regressions/items", "[]".to_owned()),
        ("/v1/api/datasets/regressions/capture", json!({ "trace_ids": [RUN_OK] }).to_string()),
        ("/v1/api/experiments", an_experiment("", "a run", DATASET, "default").to_string()),
        ("/v1/api/score", json!({ "trace_ids": [RUN_OK], "dry_run": true }).to_string()),
        ("/v1/api/monitors/checkout-errors/ack", String::new()),
        ("/v1/api/slos/checkout-availability/ack", String::new()),
    ]
}

// ---------------------------------------------------------------- scores ---

#[tokio::test]
async fn a_score_posted_to_the_api_can_be_read_back_from_the_score_list() {
    let app = seeded(open()).await;

    let res = send(&app, post("/v1/api/scores", None, a_score().to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let scores = stored_scores(&app, None).await;
    assert_eq!(scores.len(), 1, "the score did not reach the store: {scores:?}");
    assert_eq!(scores[0]["scorer_id"], json!("helpfulness"));
    assert_eq!(scores[0]["value"], json!(0.75));
    assert_eq!(scores[0]["trace_id"], json!(RUN_OK));
}

#[tokio::test]
async fn a_score_with_no_id_is_given_one_and_it_is_the_id_that_is_stored() {
    let app = seeded(open()).await;

    let res = send(&app, post("/v1/api/scores", None, a_score().to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let minted = res.json()["score_id"].as_str().unwrap_or_default().to_owned();
    assert!(!minted.is_empty(), "the handler returned a score with no id: {}", res.text());

    let scores = stored_scores(&app, None).await;
    assert_eq!(scores[0]["score_id"], json!(minted), "the response and the store disagree");
}

#[tokio::test]
async fn a_score_id_the_caller_chose_is_kept() {
    let app = seeded(open()).await;
    let mut body = a_score();
    body["score_id"] = json!("caller-chosen-1");

    let res = send(&app, post("/v1/api/scores", None, body.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["score_id"], json!("caller-chosen-1"));
}

#[tokio::test]
async fn a_score_naming_neither_a_trace_nor_a_span_is_refused_and_says_which_fields() {
    let app = seeded(open()).await;
    let mut body = a_score();
    body.as_object_mut().expect("an object").remove("trace_id");

    let res = send(&app, post("/v1/api/scores", None, body.to_string())).await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "a score about nothing was stored");
    let (code, message) = res.error();
    assert_eq!(code, "bad_request");
    assert!(
        message.contains("trace_id") && message.contains("span_id"),
        "the refusal must name both fields the caller could have sent, said: {message}"
    );
}

#[tokio::test]
async fn a_score_refused_for_naming_no_target_is_not_stored() {
    // The status code is the promise; this is the part that matters.
    let app = seeded(open()).await;
    let mut body = a_score();
    body.as_object_mut().expect("an object").remove("trace_id");

    send(&app, post("/v1/api/scores", None, body.to_string())).await;
    assert!(stored_scores(&app, None).await.is_empty(), "a refused score reached the store");
}

#[tokio::test]
async fn a_score_about_one_span_rather_than_a_whole_run_is_accepted() {
    let app = seeded(open()).await;
    let mut body = a_score();
    body.as_object_mut().expect("an object").remove("trace_id");
    body["span_id"] = json!(OK_ROOT);

    let res = send(&app, post("/v1/api/scores", None, body.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "a span-level score was refused: {}", res.text());

    let scores = stored_scores(&app, None).await;
    assert_eq!(scores.len(), 1);
    assert_eq!(scores[0]["span_id"], json!(OK_ROOT));
}

/// A score may name a span mcpd has not seen yet. Spans and scores arrive
/// independently, and a scorer that runs before the batch carrying its span has
/// been flushed is ordinary, not an error.
#[tokio::test]
async fn a_score_about_a_span_that_was_never_ingested_is_still_stored() {
    let app = seeded(open()).await;
    let mut body = a_score();
    body["trace_id"] = json!(MISSING_RUN);
    body["span_id"] = json!(MISSING_SPAN);

    let res = send(&app, post("/v1/api/scores", None, body.to_string())).await;
    assert_eq!(
        res.status,
        StatusCode::OK,
        "a score about a future span was refused: {}",
        res.text()
    );

    let scores = stored_scores(&app, None).await;
    assert_eq!(scores.len(), 1, "the score was accepted and then lost");
    assert_eq!(scores[0]["span_id"], json!(MISSING_SPAN));
}

#[tokio::test]
async fn posting_the_same_score_id_twice_stores_one_score() {
    let app = seeded(open()).await;
    let mut body = a_score();
    body["score_id"] = json!("replayed-score");

    for _ in 0..3 {
        let res = send(&app, post("/v1/api/scores", None, body.to_string())).await;
        assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    }

    let scores = stored_scores(&app, None).await;
    assert_eq!(scores.len(), 1, "a replayed score id was counted more than once: {scores:?}");
}

#[tokio::test]
async fn reposting_a_score_id_with_a_new_value_replaces_the_old_one() {
    let app = seeded(open()).await;
    let mut body = a_score();
    body["score_id"] = json!("corrected-score");
    send(&app, post("/v1/api/scores", None, body.to_string())).await;

    body["value"] = json!(0.125);
    let res = send(&app, post("/v1/api/scores", None, body.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let scores = stored_scores(&app, None).await;
    assert_eq!(scores.len(), 1);
    assert_eq!(scores[0]["value"], json!(0.125), "the correction did not land");
}

#[tokio::test]
async fn a_secret_in_a_scores_metadata_is_masked_before_it_reaches_the_store() {
    let app = seeded(open()).await;
    let mut body = a_score();
    body["metadata"] = json!({ "api_key": "sk-live-abcdefghijklmnop", "note": "graded by hand" });

    let res = send(&app, post("/v1/api/scores", None, body.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let scores = stored_scores(&app, None).await;
    let stored = scores[0]["metadata"].to_string();
    assert!(
        !stored.contains("sk-live-abcdefghijklmnop"),
        "the credential was stored in the clear: {stored}"
    );
    assert!(stored.contains("graded by hand"), "redaction ate the field beside it: {stored}");
}

#[tokio::test]
async fn a_user_id_on_a_score_is_hashed_on_its_way_through_the_intake() {
    let app = seeded(open()).await;
    let mut body = a_score();
    body["correlation"] = json!({ "project": "default", "user_id": "user-ann" });

    let res = send(&app, post("/v1/api/scores", None, body.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let hashed = res.json()["correlation"]["user_id"].as_str().unwrap_or_default().to_owned();
    assert_ne!(hashed, "user-ann", "the subject came back as sent");
    assert!(!hashed.is_empty(), "hashing must not drop the field");
}

/// And then the store does not keep it: the `scores` table carries `project`,
/// `environment` and `experiment_id` and no subject column, so a score's
/// `correlation.user_id` is dropped on the way in. The run it points at still
/// carries the hashed subject, which is where an erasure request finds it.
#[tokio::test]
async fn the_subject_on_a_score_is_not_kept_by_the_store() {
    let app = seeded(open()).await;
    let mut body = a_score();
    body["correlation"] = json!({ "project": "default", "user_id": "user-ann" });
    send(&app, post("/v1/api/scores", None, body.to_string())).await;

    let scores = stored_scores(&app, None).await;
    assert!(
        scores[0]["correlation"].get("user_id").is_none()
            || scores[0]["correlation"]["user_id"].is_null(),
        "if this now fails, scores grew a subject column and erasure must cover it: {scores:?}"
    );
}

#[tokio::test]
async fn a_score_body_that_is_not_a_score_is_refused_before_anything_is_stored() {
    let app = seeded(open()).await;

    let res = send(&app, post("/v1/api/scores", None, "{\"scorer_id\": 4}")).await;
    assert!(
        res.status.is_client_error(),
        "a body that is not a score was accepted: {} {}",
        res.status,
        res.text()
    );
    assert!(stored_scores(&app, None).await.is_empty());
}

/// A `Score` declares the scale it is on, and a value outside that scale is not
/// interpretable: `0.75` on a 0-to-1 scale and `0.75` on a 0-to-5 scale mean
/// different things, and a mean taken across both is nonsense.
///
/// IGNORED, and this is a gap rather than a flake. `create_score` performs no
/// range check at all, so this request returns 200 and the store now holds a
/// score of 5.0 whose own `max` says 1.0. Observed: status 200 where 400 was
/// expected. The test below reads the row back and shows what landed.
#[tokio::test]
async fn a_score_above_its_own_declared_maximum_is_refused() {
    let app = seeded(open()).await;
    let mut body = a_score();
    body["value"] = json!(5.0);
    body["min"] = json!(0.0);
    body["max"] = json!(1.0);

    let res = send(&app, post("/v1/api/scores", None, body.to_string())).await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "an uninterpretable score was stored");
    let (_, message) = res.error();
    assert!(message.contains("max"), "the refusal should name the bound it broke: {message}");
}

/// The other end of the same scale. Once ignored for the same reason, having
/// observed status 200 where 400 was expected; the handler now refuses it.
#[tokio::test]
async fn a_score_below_its_own_declared_minimum_is_refused() {
    let app = seeded(open()).await;
    let mut body = a_score();
    body["value"] = json!(-3.0);

    let res = send(&app, post("/v1/api/scores", None, body.to_string())).await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "an uninterpretable score was stored");
}
/// A score with no scorer id cannot be grouped, filtered, compared or charted:
/// every read path keys on `scorer_id`, and the monitors match on it by name.
///
/// IGNORED, and this is a gap. `create_score` mints a missing `score_id` but
/// never looks at `scorer_id`, so an empty one is written straight through.
/// Observed: status 200, and `GET /v1/api/scores` then returns a row whose
/// `scorer_id` is `""`.
#[tokio::test]
async fn a_score_with_an_empty_scorer_id_is_refused() {
    let app = seeded(open()).await;
    let mut body = a_score();
    body["scorer_id"] = json!("");

    let res = send(&app, post("/v1/api/scores", None, body.to_string())).await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "a score nobody can group was stored");
    let (_, message) = res.error();
    assert!(message.contains("scorer_id"), "the refusal should name the field: {message}");
}
// -------------------------------------------------------------- feedback ---

#[tokio::test]
async fn feedback_posted_to_the_api_can_be_read_back_from_the_feedback_list() {
    let app = seeded(open()).await;

    let res = send(&app, post("/v1/api/feedback", None, a_feedback().to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let all = stored_feedback(&app, RUN_OK, None).await;
    assert_eq!(all.len(), 1, "the feedback did not reach the store: {all:?}");
    assert_eq!(all[0]["kind"], json!("thumbs"));
    assert_eq!(all[0]["value"], json!("up"));
    assert_eq!(all[0]["trace_id"], json!(RUN_OK));
}

#[tokio::test]
async fn feedback_with_no_id_is_given_one_and_it_is_the_id_that_is_stored() {
    let app = seeded(open()).await;

    let res = send(&app, post("/v1/api/feedback", None, a_feedback().to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let minted = res.json()["feedback_id"].as_str().unwrap_or_default().to_owned();
    assert!(!minted.is_empty(), "the handler returned feedback with no id");

    let all = stored_feedback(&app, RUN_OK, None).await;
    assert_eq!(all[0]["feedback_id"], json!(minted), "the response and the store disagree");
}

#[tokio::test]
async fn feedback_naming_neither_a_trace_nor_a_span_is_refused_and_says_which_fields() {
    let app = seeded(open()).await;
    let mut body = a_feedback();
    body.as_object_mut().expect("an object").remove("trace_id");

    let res = send(&app, post("/v1/api/feedback", None, body.to_string())).await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "feedback about nothing was stored");
    let (code, message) = res.error();
    assert_eq!(code, "bad_request");
    assert!(
        message.contains("trace_id") && message.contains("span_id"),
        "the refusal must name both fields the caller could have sent, said: {message}"
    );
}

#[tokio::test]
async fn feedback_refused_for_naming_no_target_is_not_stored() {
    let app = seeded(open()).await;
    let mut body = a_feedback();
    body.as_object_mut().expect("an object").remove("trace_id");

    send(&app, post("/v1/api/feedback", None, body.to_string())).await;
    assert!(
        stored_feedback(&app, RUN_OK, None).await.is_empty(),
        "refused feedback reached the store"
    );
}

#[tokio::test]
async fn feedback_about_one_span_rather_than_a_whole_run_is_accepted() {
    let app = seeded(open()).await;
    let mut body = a_feedback();
    body.as_object_mut().expect("an object").remove("trace_id");
    body["span_id"] = json!(OK_ROOT);

    let res = send(&app, post("/v1/api/feedback", None, body.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "span-level feedback was refused: {}", res.text());
    assert_eq!(res.json()["span_id"], json!(OK_ROOT));
    assert!(
        !res.json()["feedback_id"].as_str().unwrap_or_default().is_empty(),
        "it was accepted without being given an id: {}",
        res.text()
    );
}

/// The consequence of the shape above, written down because it is surprising:
/// `create_feedback` accepts feedback that names only a span, and every read
/// path is keyed on `trace_id`, so nothing can serve it back.
#[tokio::test]
async fn feedback_naming_only_a_span_cannot_be_read_back_from_the_run_it_belongs_to() {
    let app = seeded(open()).await;
    let mut body = a_feedback();
    body.as_object_mut().expect("an object").remove("trace_id");
    body["span_id"] = json!(OK_ROOT);
    send(&app, post("/v1/api/feedback", None, body.to_string())).await;

    assert!(
        stored_feedback(&app, RUN_OK, None).await.is_empty(),
        "if this now fails, span-level feedback grew a read path and this test should say so"
    );
}

/// `http.rs` proves the handler's response carries a hashed author. This proves
/// the row does, which is the claim the privacy page actually makes.
#[tokio::test]
async fn the_author_of_a_piece_of_feedback_is_hashed_in_the_store_not_only_in_the_reply() {
    let app = seeded(open()).await;
    let mut body = a_feedback();
    body["author_id"] = json!("customer-4477");

    let res = send(&app, post("/v1/api/feedback", None, body.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let all = stored_feedback(&app, RUN_OK, None).await;
    let author = all[0]["author_id"].as_str().unwrap_or_default();
    assert_ne!(author, "customer-4477", "the author was stored as sent");
    assert!(!author.is_empty(), "hashing must not drop the field");
}

/// `Intake::prepare` passes a feedback comment through `Redactor::redact`, so
/// the write path reads as though free prose is scanned. It is not: redaction
/// A credential typed into a free-text comment is NOT masked, and that is the
/// documented contract rather than a gap. The redactor matches field names, so
/// it can mask a value that arrived under a key called `api_key` and cannot
/// know that a sentence a human typed contains one. The security page says so
/// in as many words: "It does not scan free text, and it is not a content
/// classifier."
///
/// This test pins that boundary from the other side. If someone later adds
/// value scanning, it fails and makes them decide deliberately rather than
/// changing the contract by accident.
#[tokio::test]
async fn a_credential_typed_into_a_free_text_comment_is_not_scanned_for() {
    let app = seeded(open()).await;
    let mut body = a_feedback();
    body["kind"] = json!("comment");
    body["value"] = json!("text");
    body["comment"] = json!("it kept printing my api_key: sk-live-abcdefghijklmnop");

    let res = send(&app, post("/v1/api/feedback", None, body.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let all = stored_feedback(&app, RUN_OK, None).await;
    let comment = all[0]["comment"].as_str().unwrap_or_default();
    assert!(
        comment.contains("sk-live-abcdefghijklmnop"),
        "free text is documented as unscanned; if this now masks, the security page \
         needs changing too: {comment}"
    );
}

#[tokio::test]
async fn a_secret_in_feedback_metadata_is_masked_before_it_reaches_the_store() {
    let app = seeded(open()).await;
    let mut body = a_feedback();
    body["metadata"] = json!({ "session_token": "st-live-0123456789", "surface": "web" });

    let res = send(&app, post("/v1/api/feedback", None, body.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let all = stored_feedback(&app, RUN_OK, None).await;
    let stored = all[0]["metadata"].to_string();
    assert!(!stored.contains("st-live-0123456789"), "the token was stored in the clear: {stored}");
    assert!(stored.contains("web"), "redaction ate the field beside it: {stored}");
}

#[tokio::test]
async fn feedback_of_a_kind_the_model_does_not_define_is_refused() {
    let app = seeded(open()).await;
    let mut body = a_feedback();
    body["kind"] = json!("shrug");

    let res = send(&app, post("/v1/api/feedback", None, body.to_string())).await;
    assert!(
        res.status.is_client_error(),
        "an undefined feedback kind was accepted: {} {}",
        res.status,
        res.text()
    );
    assert!(stored_feedback(&app, RUN_OK, None).await.is_empty());
}

// -------------------------------------------------------------- datasets ---

#[tokio::test]
async fn a_dataset_created_through_the_api_appears_in_the_listing() {
    let app = seeded(open()).await;

    let res = send(
        &app,
        post("/v1/api/datasets", None, a_dataset("", "refund cases", "default").to_string()),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let listed = send(&app, get("/v1/api/datasets", None)).await;
    let names: Vec<String> = listed
        .json()
        .as_array()
        .map(|a| a.iter().filter_map(|d| d["name"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    assert!(
        names.contains(&"refund cases".to_owned()),
        "the dataset is not in the list: {names:?}"
    );
}

#[tokio::test]
async fn a_dataset_with_no_id_is_given_one() {
    let app = seeded(open()).await;

    let res = send(
        &app,
        post("/v1/api/datasets", None, a_dataset("", "refund cases", "default").to_string()),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert!(
        !res.json()["dataset_id"].as_str().unwrap_or_default().is_empty(),
        "the handler returned a dataset with no id: {}",
        res.text()
    );
}

#[tokio::test]
async fn a_dataset_with_an_empty_name_is_refused_and_names_the_field() {
    let app = seeded(open()).await;

    let res =
        send(&app, post("/v1/api/datasets", None, a_dataset("", "", "default").to_string())).await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "a nameless dataset was created");
    let (code, message) = res.error();
    assert_eq!(code, "bad_parameter");
    assert_eq!(res.json()["field"], json!("name"), "the error must name the field: {}", res.text());
    assert!(message.contains("name"), "the message should say what is missing: {message}");
}

#[tokio::test]
async fn a_dataset_named_only_with_spaces_is_refused_too() {
    let app = seeded(open()).await;

    let res =
        send(&app, post("/v1/api/datasets", None, a_dataset("", "   ", "default").to_string()))
            .await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "whitespace passed as a name");
}

#[tokio::test]
async fn a_dataset_refused_for_its_name_is_not_created() {
    let app = seeded(open()).await;

    send(&app, post("/v1/api/datasets", None, a_dataset("ghost", "", "default").to_string())).await;

    let listed = send(&app, get("/v1/api/datasets", None)).await;
    let ids: Vec<String> = listed
        .json()
        .as_array()
        .map(|a| a.iter().filter_map(|d| d["dataset_id"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    assert!(!ids.contains(&"ghost".to_owned()), "a refused dataset was written anyway: {ids:?}");
}

#[tokio::test]
async fn items_added_to_a_dataset_come_back_from_the_item_endpoint() {
    let app = seeded(open()).await;
    let body = json!([an_item(json!({ "prompt": "refund the order" }))]).to_string();

    let res = send(&app, post(&format!("/v1/api/datasets/{DATASET}/items"), None, body)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["added"], json!(1), "the reply undercounted: {}", res.text());

    let items = stored_items(&app, None).await;
    assert_eq!(items.len(), 1, "the case did not reach the store");
    assert_eq!(items[0]["input"]["prompt"], json!("refund the order"));
}

#[tokio::test]
async fn an_item_with_no_id_is_given_one() {
    let app = seeded(open()).await;
    let body = json!([an_item(json!("one"))]).to_string();

    send(&app, post(&format!("/v1/api/datasets/{DATASET}/items"), None, body)).await;

    let items = stored_items(&app, None).await;
    assert!(
        !items[0]["item_id"].as_str().unwrap_or_default().is_empty(),
        "an item was stored with no id: {items:?}"
    );
}

#[tokio::test]
async fn an_item_that_names_a_different_dataset_is_filed_under_the_one_in_the_path() {
    // Otherwise a caller could write into any dataset by naming it in the body,
    // and the ownership check on the path would be looking at the wrong id.
    let app = seeded(open()).await;
    let mut item = an_item(json!("one"));
    item["dataset_id"] = json!("somewhere-else");

    let res = send(
        &app,
        post(&format!("/v1/api/datasets/{DATASET}/items"), None, json!([item]).to_string()),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let items = stored_items(&app, None).await;
    assert_eq!(items.len(), 1, "the case was filed somewhere else");
    assert_eq!(items[0]["dataset_id"], json!(DATASET));
}

#[tokio::test]
async fn adding_several_items_at_once_reports_and_stores_all_of_them() {
    let app = seeded(open()).await;
    let body = json!([an_item(json!("one")), an_item(json!("two")), an_item(json!("three"))]);

    let res =
        send(&app, post(&format!("/v1/api/datasets/{DATASET}/items"), None, body.to_string()))
            .await;
    assert_eq!(res.json()["added"], json!(3), "{}", res.text());
    assert_eq!(stored_items(&app, None).await.len(), 3);
}

#[tokio::test]
async fn adding_items_updates_the_count_the_dataset_listing_reports() {
    let app = seeded(open()).await;
    let body = json!([an_item(json!("one")), an_item(json!("two"))]).to_string();

    send(&app, post(&format!("/v1/api/datasets/{DATASET}/items"), None, body)).await;

    let listed = send(&app, get("/v1/api/datasets", None)).await;
    let found = listed
        .json()
        .as_array()
        .and_then(|a| a.iter().find(|d| d["dataset_id"] == json!(DATASET)).cloned())
        .expect("the fixture dataset is listed");
    assert_eq!(found["item_count"], json!(2), "the count did not follow the items");
}

#[tokio::test]
async fn adding_an_empty_list_of_items_stores_nothing_and_says_so() {
    let app = seeded(open()).await;

    let res = send(&app, post(&format!("/v1/api/datasets/{DATASET}/items"), None, "[]")).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["added"], json!(0));
    assert!(stored_items(&app, None).await.is_empty());
}

#[tokio::test]
async fn adding_items_to_a_dataset_that_does_not_exist_is_a_404_that_names_the_id() {
    let app = seeded(open()).await;
    let body = json!([an_item(json!("one"))]).to_string();

    let res = send(&app, post("/v1/api/datasets/no-such-set/items", None, body)).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND, "{}", res.text());
    let (code, message) = res.error();
    assert_eq!(code, "not_found");
    assert!(message.contains("no-such-set"), "the message should name the id: {message}");
}

/// A dataset case holds whatever a production run was fed, so it is exactly the
/// shape of payload redaction exists for. `add_dataset_items` writes straight to
/// the store without going through `Intake::prepare`, which `pipeline.rs` says
/// every write path uses.
///
/// IGNORED, and this is a gap. Observed: the request returns 200 and
/// `GET /v1/api/datasets/regressions/items` hands the credential back verbatim
/// in `input.api_key`.
#[tokio::test]
async fn a_secret_in_a_dataset_item_is_masked_before_it_reaches_the_store() {
    let app = seeded(open()).await;
    let body =
        json!([an_item(json!({ "api_key": "sk-live-abcdefghijklmnop", "prompt": "refund" }))]);

    let res =
        send(&app, post(&format!("/v1/api/datasets/{DATASET}/items"), None, body.to_string()))
            .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let items = stored_items(&app, None).await;
    let stored = items[0]["input"].to_string();
    assert!(!stored.contains("sk-live-abcdefghijklmnop"), "the credential was stored: {stored}");
}
// --------------------------------------------------------------- capture ---

#[tokio::test]
async fn capturing_a_real_run_makes_a_case_that_names_the_run_it_came_from() {
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [RUN_OK] }).to_string();

    let res = send(&app, post(&format!("/v1/api/datasets/{DATASET}/capture"), None, body)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json(), json!({ "captured": 1, "requested": 1 }), "{}", res.text());

    let items = stored_items(&app, None).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["source_trace_id"], json!(RUN_OK), "the case forgot where it came from");
    assert_eq!(items[0]["input"]["prompt"], json!("refund the order"), "the input was not copied");
}

#[tokio::test]
async fn capturing_records_the_status_the_original_run_ended_in() {
    // This is what makes a captured failure legible a month later.
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [RUN_FAILED] }).to_string();

    send(&app, post(&format!("/v1/api/datasets/{DATASET}/capture"), None, body)).await;

    let items = stored_items(&app, None).await;
    assert_eq!(items[0]["metadata"]["original_status"], json!("error"));
    assert_eq!(items[0]["metadata"]["captured_from"], json!(RUN_FAILED));
}

#[tokio::test]
async fn capturing_leaves_the_expected_answer_empty_unless_the_caller_asks_for_it() {
    // Capturing a bad answer as the reference would enshrine the bug, so the
    // default has to be "no reference".
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [RUN_OK] }).to_string();

    send(&app, post(&format!("/v1/api/datasets/{DATASET}/capture"), None, body)).await;

    let items = stored_items(&app, None).await;
    assert!(
        items[0].get("expected").is_none() || items[0]["expected"].is_null(),
        "the run's own output became the reference without being asked: {items:?}"
    );
}

#[tokio::test]
async fn capturing_with_use_output_as_expected_copies_the_answer_the_run_gave() {
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [RUN_OK], "use_output_as_expected": true }).to_string();

    send(&app, post(&format!("/v1/api/datasets/{DATASET}/capture"), None, body)).await;

    let items = stored_items(&app, None).await;
    assert_eq!(items[0]["expected"]["text"], json!("the refund is queued"));
}

#[tokio::test]
async fn capturing_a_run_that_has_no_output_leaves_the_expected_answer_empty() {
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [RUN_FAILED], "use_output_as_expected": true }).to_string();

    send(&app, post(&format!("/v1/api/datasets/{DATASET}/capture"), None, body)).await;

    let items = stored_items(&app, None).await;
    assert!(
        items[0].get("expected").is_none() || items[0]["expected"].is_null(),
        "a run with no output produced a reference answer: {items:?}"
    );
}

#[tokio::test]
async fn capturing_a_trace_id_that_does_not_exist_captures_nothing_and_still_reports_the_ask() {
    // Reporting both numbers is the point: `captured: 0, requested: 1` says the
    // run was not found, where a bare `0` would look like a server fault.
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [MISSING_RUN] }).to_string();

    let res = send(&app, post(&format!("/v1/api/datasets/{DATASET}/capture"), None, body)).await;
    assert_eq!(
        res.status,
        StatusCode::OK,
        "an unknown run should not fail the request: {}",
        res.text()
    );
    assert_eq!(res.json(), json!({ "captured": 0, "requested": 1 }), "{}", res.text());
    assert!(stored_items(&app, None).await.is_empty(), "a phantom case was written");
}

#[tokio::test]
async fn capturing_a_mix_of_real_and_unknown_runs_keeps_the_real_ones() {
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [RUN_OK, MISSING_RUN, RUN_FAILED] }).to_string();

    let res = send(&app, post(&format!("/v1/api/datasets/{DATASET}/capture"), None, body)).await;
    assert_eq!(res.json(), json!({ "captured": 2, "requested": 3 }), "{}", res.text());
    assert_eq!(stored_items(&app, None).await.len(), 2);
}

#[tokio::test]
async fn capturing_with_no_trace_ids_is_a_400_that_names_the_field() {
    let app = seeded(open()).await;

    let res = send(&app, post(&format!("/v1/api/datasets/{DATASET}/capture"), None, "{}")).await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "{}", res.text());
    assert_eq!(res.json()["field"], json!("trace_ids"), "{}", res.text());
    let (code, message) = res.error();
    assert_eq!(code, "bad_parameter");
    assert!(message.contains("runs"), "the message should say what to send: {message}");
}

#[tokio::test]
async fn capturing_into_a_dataset_that_does_not_exist_is_a_404_that_names_the_id() {
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [RUN_OK] }).to_string();

    let res = send(&app, post("/v1/api/datasets/no-such-set/capture", None, body)).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND, "{}", res.text());
    let (code, message) = res.error();
    assert_eq!(code, "not_found");
    assert!(message.contains("no-such-set"), "the message should name the id: {message}");
}

#[tokio::test]
async fn capturing_a_run_that_is_already_a_case_adds_a_second_copy() {
    // Capture is not idempotent, and nothing claims it is: each call mints a
    // fresh item id. This is written down because the shape invites the
    // opposite assumption.
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [RUN_OK] }).to_string();

    send(&app, post(&format!("/v1/api/datasets/{DATASET}/capture"), None, body.clone())).await;
    send(&app, post(&format!("/v1/api/datasets/{DATASET}/capture"), None, body)).await;

    assert_eq!(stored_items(&app, None).await.len(), 2, "the second capture did not land");
}

// ----------------------------------------------------------- experiments ---

#[tokio::test]
async fn an_experiment_created_through_the_api_appears_in_the_listing() {
    let app = seeded(open()).await;
    let body = an_experiment("", "prompt v2", DATASET, "default").to_string();

    let res = send(&app, post("/v1/api/experiments", None, body)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let listed = send(&app, get("/v1/api/experiments", None)).await;
    let names: Vec<String> = listed
        .json()
        .as_array()
        .map(|a| a.iter().filter_map(|e| e["name"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    assert!(names.contains(&"prompt v2".to_owned()), "the experiment is not listed: {names:?}");
}

#[tokio::test]
async fn an_experiment_with_no_id_is_given_one() {
    let app = seeded(open()).await;
    let body = an_experiment("", "prompt v2", DATASET, "default").to_string();

    let res = send(&app, post("/v1/api/experiments", None, body)).await;
    assert!(
        !res.json()["experiment_id"].as_str().unwrap_or_default().is_empty(),
        "the handler returned an experiment with no id: {}",
        res.text()
    );
}

#[tokio::test]
async fn an_experiment_with_no_start_time_is_given_one() {
    // An experiment with no `started_at` sorts and windows as though it never
    // ran, so the handler fills it in rather than storing a null.
    let app = seeded(open()).await;
    let body = an_experiment("", "prompt v2", DATASET, "default").to_string();

    let res = send(&app, post("/v1/api/experiments", None, body)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let started = res.json()["started_at"].as_str().unwrap_or_default().to_owned();
    assert!(!started.is_empty(), "the experiment was stored with no start time: {}", res.text());
    OffsetDateTime::parse(&started, &Rfc3339).expect("the start time must be RFC 3339");
}

#[tokio::test]
async fn a_start_time_the_caller_supplied_is_kept() {
    let app = seeded(open()).await;
    let mut body = an_experiment("", "prompt v2", DATASET, "default");
    body["started_at"] = json!(format!("{DAY}T09:00:00Z"));

    let res = send(&app, post("/v1/api/experiments", None, body.to_string())).await;
    assert_eq!(res.json()["started_at"], json!(format!("{DAY}T09:00:00Z")), "{}", res.text());
}

/// Score both experiments enough times for the comparison to be decidable.
///
/// `MIN_SAMPLE` in `eval.rs` is twenty per side, so this writes twenty-two of
/// each: a comparison drawn on fewer is reported as not decidable, which the
/// next test is about.
async fn seed_experiment_scores(app: &Router, base: f64, cand: f64, n: usize) {
    let mut scores = Vec::new();
    for i in 0..n {
        for (experiment, value) in [("before", base), ("after", cand)] {
            scores.push(json!({
                "score_id": format!("{experiment}-{i}"),
                "trace_id": RUN_OK,
                "scorer_id": "helpfulness",
                "source": "experiment",
                "value": value,
                "timestamp": format!("{DAY}T00:05:00Z"),
                "correlation": { "project": "default", "experiment_id": experiment }
            }));
        }
    }
    let res = send(app, post("/v1/ingest", None, json!({ "scores": scores }).to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "seeding the experiment scores failed: {}", res.text());
}

#[tokio::test]
async fn comparing_two_experiments_reports_the_delta_and_the_count_behind_it() {
    let app = seeded(open()).await;
    seed_experiment_scores(&app, 0.5, 0.9, 22).await;

    let res =
        send(&app, get("/v1/api/experiments/compare?baseline=before&candidate=after", None)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let body = res.json();
    assert_eq!(body["baseline"], json!("before"));
    assert_eq!(body["candidate"], json!("after"));
    let row = &body["scorers"][0];
    assert_eq!(row["scorer_id"], json!("helpfulness"));
    assert_eq!(row["baseline_n"], json!(22), "{}", res.text());
    assert_eq!(row["candidate_n"], json!(22), "{}", res.text());
    assert!(
        (row["delta"].as_f64().unwrap_or_default() - 0.4).abs() < 1e-9,
        "the delta is wrong: {}",
        res.text()
    );
    assert_eq!(row["decidable"], json!(true));
    assert_eq!(body["regressed"], json!(false), "an improvement was reported as a regression");
}

#[tokio::test]
async fn a_candidate_that_scored_worse_is_reported_as_a_regression() {
    let app = seeded(open()).await;
    seed_experiment_scores(&app, 0.9, 0.5, 22).await;

    let res =
        send(&app, get("/v1/api/experiments/compare?baseline=before&candidate=after", None)).await;
    assert_eq!(res.json()["regressed"], json!(true), "the drop was not reported: {}", res.text());
}

#[tokio::test]
async fn a_comparison_drawn_on_too_few_runs_is_reported_as_not_decidable() {
    // Four observations a side. The delta is real arithmetic and is still
    // returned; what changes is that mcpd refuses to call it a finding.
    let app = seeded(open()).await;
    seed_experiment_scores(&app, 0.9, 0.5, 4).await;

    let res =
        send(&app, get("/v1/api/experiments/compare?baseline=before&candidate=after", None)).await;
    let row = &res.json()["scorers"][0];
    assert_eq!(row["decidable"], json!(false), "four runs a side was called decidable");
    assert_eq!(
        res.json()["regressed"],
        json!(false),
        "an undecidable drop was called a regression"
    );
}

#[tokio::test]
async fn comparing_against_an_experiment_with_no_scores_leaves_its_side_empty() {
    let app = seeded(open()).await;
    seed_experiment_scores(&app, 0.5, 0.9, 2).await;

    let res =
        send(&app, get("/v1/api/experiments/compare?baseline=before&candidate=never-ran", None))
            .await;
    let row = &res.json()["scorers"][0];
    assert_eq!(row["candidate_n"], json!(0), "{}", res.text());
    assert!(row["candidate_mean"].is_null(), "a mean was invented from no data: {}", res.text());
    assert!(row["delta"].is_null(), "a delta was computed against nothing: {}", res.text());
}

#[tokio::test]
async fn a_comparison_with_no_baseline_is_a_400_that_names_the_parameter() {
    let app = seeded(open()).await;

    let res = send(&app, get("/v1/api/experiments/compare?candidate=after", None)).await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "{}", res.text());
    assert_eq!(res.json()["field"], json!("baseline"), "{}", res.text());
}

#[tokio::test]
async fn a_comparison_with_no_candidate_is_a_400_that_names_the_parameter() {
    let app = seeded(open()).await;

    let res = send(&app, get("/v1/api/experiments/compare?baseline=before", None)).await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "{}", res.text());
    assert_eq!(res.json()["field"], json!("candidate"), "{}", res.text());
}

// ------------------------------------------------------- acknowledgement ---

#[tokio::test]
async fn acknowledging_a_monitor_that_does_not_exist_is_a_404_that_names_the_id() {
    let app = seeded(open()).await;

    let res = send(&app, post("/v1/api/monitors/no-such-monitor/ack", None, "")).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND, "{}", res.text());
    let (code, message) = res.error();
    assert_eq!(code, "not_found");
    assert!(message.contains("no-such-monitor"), "the message should name the id: {message}");
}

#[tokio::test]
async fn acknowledging_an_objective_that_does_not_exist_is_a_404_that_names_the_id() {
    let app = seeded(open()).await;

    let res = send(&app, post("/v1/api/slos/no-such-objective/ack", None, "")).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND, "{}", res.text());
    let (code, message) = res.error();
    assert_eq!(code, "not_found");
    assert!(message.contains("no-such-objective"), "the message should name the id: {message}");
}

#[tokio::test]
async fn acknowledging_a_tripped_monitor_moves_it_to_acknowledged() {
    let (app, state, _store) = seeded_with_state(open()).await;
    let now = OffsetDateTime::now_utc();
    seed_a_run_inside_the_alert_windows(&app, "ok", now).await;
    // The real evaluation, not a hand-set latch: the condition the endpoint acts
    // on is the one `tick` produces or the test proves nothing about the daemon.
    let changed = mcpd_server::monitor::tick(&state, now).await;
    assert_eq!(changed.len(), 1, "the fixture monitor did not trip: {changed:?}");
    assert_eq!(changed[0].condition, mcpd_server::monitor::Condition::Tripped);

    let res = send(&app, post(&format!("/v1/api/monitors/{MONITOR_ID}/ack"), None, "")).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["condition"], json!("acknowledged"), "{}", res.text());
}

#[tokio::test]
async fn an_acknowledgement_is_visible_from_the_monitor_listing() {
    let (app, state, _store) = seeded_with_state(open()).await;
    let now = OffsetDateTime::now_utc();
    seed_a_run_inside_the_alert_windows(&app, "ok", now).await;
    mcpd_server::monitor::tick(&state, now).await;
    send(&app, post(&format!("/v1/api/monitors/{MONITOR_ID}/ack"), None, "")).await;

    let listed = send(&app, get("/v1/api/monitors", None)).await;
    assert_eq!(listed.json()[0]["condition"], json!("acknowledged"), "{}", listed.text());
}

#[tokio::test]
async fn acknowledging_an_already_acknowledged_monitor_leaves_it_acknowledged() {
    let (app, state, _store) = seeded_with_state(open()).await;
    let now = OffsetDateTime::now_utc();
    seed_a_run_inside_the_alert_windows(&app, "ok", now).await;
    mcpd_server::monitor::tick(&state, now).await;

    for _ in 0..3 {
        let res = send(&app, post(&format!("/v1/api/monitors/{MONITOR_ID}/ack"), None, "")).await;
        assert_eq!(res.status, StatusCode::OK, "{}", res.text());
        assert_eq!(res.json()["condition"], json!("acknowledged"));
    }
}

#[tokio::test]
async fn acknowledging_a_monitor_that_never_tripped_does_not_pretend_it_did() {
    // Nothing has been evaluated, so the monitor is `insufficient`. An
    // acknowledgement must not move it: silencing an alert nobody raised would
    // report a condition mcpd has not measured.
    let app = seeded(open()).await;

    let res = send(&app, post(&format!("/v1/api/monitors/{MONITOR_ID}/ack"), None, "")).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["condition"], json!("insufficient"), "{}", res.text());
}

#[tokio::test]
async fn acknowledging_a_tripped_objective_moves_it_to_acknowledged() {
    let (app, state, _store) = seeded_with_state(open()).await;
    let now = OffsetDateTime::now_utc();
    // One failed run inside both burn windows: the fast and the slow window
    // agree, which is what the multi-window rule requires before it trips.
    seed_a_run_inside_the_alert_windows(&app, "error", now).await;

    let changed = mcpd_server::monitor::tick_slos(&state, now).await;
    assert_eq!(changed.len(), 1, "the objective did not trip: {changed:?}");

    let res = send(&app, post(&format!("/v1/api/slos/{SLO_ID}/ack"), None, "")).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["condition"], json!("acknowledged"), "{}", res.text());
}

#[tokio::test]
async fn acknowledging_an_objective_that_never_tripped_does_not_pretend_it_did() {
    let app = seeded(open()).await;

    let res = send(&app, post(&format!("/v1/api/slos/{SLO_ID}/ack"), None, "")).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["condition"], json!("insufficient"), "{}", res.text());
}

// -------------------------------------------------------- the scorer run ---

#[tokio::test]
async fn running_the_scorers_over_a_stored_run_writes_what_it_produced() {
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [RUN_OK], "scorers": ["no_errors"] }).to_string();

    let res = send(&app, post("/v1/api/score", None, body)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["traces_scored"], json!(1), "{}", res.text());
    assert_eq!(res.json()["scores_written"], json!(1), "{}", res.text());

    let scores = stored_scores(&app, None).await;
    assert_eq!(scores.len(), 1, "the scoring run reported a write that did not happen");
    assert_eq!(scores[0]["scorer_id"], json!("no_errors"));
    assert_eq!(scores[0]["trace_id"], json!(RUN_OK));
}

#[tokio::test]
async fn a_scorer_scores_a_failed_run_differently_from_a_successful_one() {
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [RUN_OK, RUN_FAILED], "scorers": ["no_errors"] }).to_string();

    let res = send(&app, post("/v1/api/score", None, body)).await;
    assert_eq!(res.json()["traces_scored"], json!(2), "{}", res.text());

    let scores = stored_scores(&app, None).await;
    let values: Vec<f64> = scores.iter().filter_map(|s| s["value"].as_f64()).collect();
    assert!(values.contains(&1.0), "the clean run did not score 1: {values:?}");
    assert!(values.contains(&0.0), "the failed run did not score 0: {values:?}");
}

#[tokio::test]
async fn a_dry_run_reports_the_scores_and_stores_none_of_them() {
    let app = seeded(open()).await;
    let body =
        json!({ "trace_ids": [RUN_OK], "scorers": ["no_errors"], "dry_run": true }).to_string();

    let res = send(&app, post("/v1/api/score", None, body)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["scores_written"], json!(0), "a dry run wrote something");
    assert_eq!(res.json()["scores"].as_array().map(Vec::len), Some(1), "{}", res.text());
    assert!(stored_scores(&app, None).await.is_empty(), "a dry run reached the store");
}

#[tokio::test]
async fn naming_a_scorer_that_does_not_exist_lists_the_ones_that_do() {
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [RUN_OK], "scorers": ["vibes"] }).to_string();

    let res = send(&app, post("/v1/api/score", None, body)).await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "{}", res.text());
    assert_eq!(res.json()["field"], json!("scorers"), "{}", res.text());
    let (_, message) = res.error();
    assert!(message.contains("vibes"), "the message should quote what was asked for: {message}");
    assert!(message.contains("no_errors"), "the message should list what exists: {message}");
}

#[tokio::test]
async fn nothing_is_scored_when_one_named_scorer_is_unknown() {
    // The whole request is refused before any work happens, so a typo in the
    // third of four scorer names does not half-run the pass.
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [RUN_OK], "scorers": ["no_errors", "vibes"] }).to_string();

    send(&app, post("/v1/api/score", None, body)).await;
    assert!(stored_scores(&app, None).await.is_empty(), "a refused pass wrote scores anyway");
}

#[tokio::test]
async fn a_scorer_that_needs_configuration_is_skipped_with_a_reason_rather_than_failing() {
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [RUN_OK], "scorers": ["exact_match"] }).to_string();

    let res = send(&app, post("/v1/api/score", None, body)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let skipped = &res.json()["skipped"][0];
    assert_eq!(skipped["scorer_id"], json!("exact_match"), "{}", res.text());
    assert!(
        skipped["reason"].as_str().unwrap_or_default().contains("configuration"),
        "the reason should say what was missing: {}",
        res.text()
    );
}

#[tokio::test]
async fn scoring_a_run_that_does_not_exist_is_reported_as_skipped_not_as_an_error() {
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [MISSING_RUN], "scorers": ["no_errors"] }).to_string();

    let res = send(&app, post("/v1/api/score", None, body)).await;
    assert_eq!(res.status, StatusCode::OK, "an unknown run failed the whole pass: {}", res.text());
    assert_eq!(res.json()["traces_scored"], json!(0));
    let skipped = &res.json()["skipped"][0];
    assert_eq!(skipped["trace_id"], json!(MISSING_RUN), "{}", res.text());
    assert!(
        skipped["reason"].as_str().unwrap_or_default().contains("no such run"),
        "the reason should say the run was not found: {}",
        res.text()
    );
}

#[tokio::test]
async fn a_scoring_run_records_offline_as_the_source_unless_told_otherwise() {
    // These scores were produced by reading stored runs, not by watching them
    // happen, and calling them `live` would misreport where they came from.
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [RUN_OK], "scorers": ["no_errors"] }).to_string();

    send(&app, post("/v1/api/score", None, body)).await;
    assert_eq!(stored_scores(&app, None).await[0]["source"], json!("offline"));
}

#[tokio::test]
async fn the_source_of_a_scoring_run_can_be_overridden() {
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [RUN_OK], "scorers": ["no_errors"], "source": "experiment" })
        .to_string();

    send(&app, post("/v1/api/score", None, body)).await;
    assert_eq!(stored_scores(&app, None).await[0]["source"], json!("experiment"));
}

#[tokio::test]
async fn a_scoring_run_that_names_no_runs_scores_the_recent_ones() {
    let app = seeded(open()).await;
    let body = json!({ "scorers": ["no_errors"] }).to_string();

    let res = send(&app, post("/v1/api/score", None, body)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(
        res.json()["traces_scored"],
        json!(2),
        "both seeded runs should be scored: {}",
        res.text()
    );
}

#[tokio::test]
async fn a_scoring_run_that_names_no_scorers_runs_every_one_that_needs_no_configuration() {
    let app = seeded(open()).await;
    let body = json!({ "trace_ids": [RUN_OK], "dry_run": true }).to_string();

    let res = send(&app, post("/v1/api/score", None, body)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let produced: Vec<String> = res.json()["scores"]
        .as_array()
        .map(|a| a.iter().filter_map(|s| s["scorer_id"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    assert!(produced.contains(&"no_errors".to_owned()), "{produced:?}");
    assert!(
        !produced.contains(&"exact_match".to_owned()),
        "a scorer that needs configuration ran anyway: {produced:?}"
    );
}

// ---------------------------------------------------------- authorisation ---

#[tokio::test]
async fn every_write_route_refuses_a_caller_with_no_key() {
    let app = seeded(fully_keyed()).await;

    for (route, body) in write_routes() {
        let res = send(&app, post(route, None, body)).await;
        assert_eq!(res.status, StatusCode::UNAUTHORIZED, "{route} let an anonymous caller write");
        let (code, message) = res.error();
        assert_eq!(code, "unauthorized", "{route}");
        assert!(
            message.contains("Authorization"),
            "{route} should say how to send a key, said: {message}"
        );
    }
}

/// The distinction that matters to whoever is holding the key. A key this
/// daemon has never heard of is a credential problem and gets 401; a key that is
/// configured but not for this endpoint is a permission problem and gets 403.
/// Collapsing the two sends an operator hunting for a typo in a key that is
/// perfectly correct.
#[tokio::test]
async fn every_write_route_answers_an_unknown_key_with_401() {
    let app = seeded(fully_keyed()).await;

    for (route, body) in write_routes() {
        let res = send(&app, post(route, Some(STRANGER), body)).await;
        assert_eq!(res.status, StatusCode::UNAUTHORIZED, "{route} did not treat a stranger as one");
        let (code, message) = res.error();
        assert_eq!(code, "unauthorized", "{route}");
        assert!(
            message.contains("not one this daemon knows"),
            "{route} should say the key is unknown, said: {message}"
        );
    }
}

#[tokio::test]
async fn every_write_route_answers_a_real_key_in_the_wrong_scope_with_403() {
    let app = seeded(fully_keyed()).await;

    for (route, body) in write_routes() {
        let res = send(&app, post(route, Some(READ_KEY), body)).await;
        assert_eq!(res.status, StatusCode::FORBIDDEN, "{route} let a read key write");
        let (code, message) = res.error();
        assert_eq!(code, "forbidden", "{route}");
        assert!(
            message.contains("write scope"),
            "{route} should name the scope it needs, said: {message}"
        );
    }
}

#[tokio::test]
async fn every_write_route_serves_a_caller_with_the_write_key() {
    // The other half of the refusal tests: without this, a route that refuses
    // everyone would pass all of them.
    let app = seeded(fully_keyed()).await;

    for (route, body) in write_routes() {
        let res = send(&app, post(route, Some(WRITE_KEY), body)).await;
        assert_eq!(res.status, StatusCode::OK, "{route} refused the write key: {}", res.text());
    }
}

#[tokio::test]
async fn an_ingest_key_stops_reaching_the_write_routes_once_a_write_key_is_configured() {
    // The ingest key lives in every container that emits a span, including the
    // ones running code you did not write. Once an operator has separated the
    // scopes, it must not still change stored data.
    let app = seeded(fully_keyed()).await;

    for (route, body) in write_routes() {
        let res = send(&app, post(route, Some(INGEST_KEY), body)).await;
        assert_eq!(res.status, StatusCode::FORBIDDEN, "{route} let an ingest key write");
        assert_eq!(res.error().0, "forbidden", "{route}");
    }
}

#[tokio::test]
async fn an_unset_write_list_leaves_every_write_route_with_the_ingest_keys() {
    // The upgrade guarantee, from the side that matters: a configuration written
    // before the write scope existed keeps working, unchanged.
    let app = seeded(keyed()).await;

    for (route, body) in write_routes() {
        let res = send(&app, post(route, Some(INGEST_KEY), body)).await;
        assert_eq!(
            res.status,
            StatusCode::OK,
            "upgrading revoked the ingest key on {route}: {}",
            res.text()
        );
    }
}

#[tokio::test]
async fn an_admin_key_is_not_a_write_key() {
    // Admin destroys data and reads the audit log. It is not a superset, and a
    // key handed out to erase a subject should not also be able to annotate runs.
    let app = seeded(fully_keyed()).await;

    for (route, body) in write_routes() {
        let res = send(&app, post(route, Some(ADMIN_KEY), body)).await;
        assert_eq!(res.status, StatusCode::FORBIDDEN, "{route} let an admin key write");
    }
}

#[tokio::test]
async fn comparing_experiments_takes_the_read_scope_and_not_the_write_scope() {
    // It is a GET that reads scores. Sitting beside the write routes in the
    // reference is not a reason for it to take their key.
    let app = seeded(fully_keyed()).await;
    let route = "/v1/api/experiments/compare?baseline=before&candidate=after";

    let refused = send(&app, get(route, Some(WRITE_KEY))).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "a write key read the comparison");
    assert!(
        refused.error().1.contains("reading"),
        "the refusal should name the read scope, said: {}",
        refused.error().1
    );

    let allowed = send(&app, get(route, Some(READ_KEY))).await;
    assert_eq!(allowed.status, StatusCode::OK, "the read key was refused: {}", allowed.text());
}

#[tokio::test]
async fn with_no_keys_configured_every_write_route_is_open() {
    // The first-run experience must not require a credential.
    let app = seeded(open()).await;

    for (route, body) in write_routes() {
        let res = send(&app, post(route, None, body)).await;
        assert_eq!(res.status, StatusCode::OK, "{route} refused an open server: {}", res.text());
    }
}

/// `add_dataset_items` resolves the owning project before it authorises
/// anything, and answers an unknown id with a 404 while still holding no key at
/// all. Every other route that takes an id checks the key first and refuses an
/// unknown id the same way it refuses one the key may not reach, precisely so
/// the endpoint cannot be used to test which ids exist.
///
/// IGNORED: observed status 404 with an anonymous caller against a fully keyed
/// server, where every sibling route answers 401. The effect is that anyone who
/// can reach the port can enumerate dataset ids without a credential.
#[tokio::test]
async fn probing_for_a_dataset_id_needs_a_key_like_everything_else() {
    let app = seeded(fully_keyed()).await;
    let body = json!([an_item(json!("one"))]).to_string();

    let res = send(&app, post("/v1/api/datasets/does-this-exist/items", None, body)).await;
    assert_eq!(
        res.status,
        StatusCode::UNAUTHORIZED,
        "an anonymous caller learned whether a dataset id exists"
    );
}
// -------------------------------------------------------------- tenancy ---

/// Two teams, each with its own keys, and nothing configured at the top level
/// for the write scope. `a-write` reaches `team-a` and nothing else.
fn tenanted() -> ServerConfig {
    let mut projects = std::collections::BTreeMap::new();
    for team in ["team-a", "team-b"] {
        projects.insert(
            team.to_owned(),
            mcpd_server::state::ProjectConfig {
                ingest_keys: vec![format!("{}-ingest", &team[5..6])],
                read_keys: vec![format!("{}-read", &team[5..6])],
                write_keys: vec![format!("{}-write", &team[5..6])],
                ..Default::default()
            },
        );
    }
    ServerConfig { projects, ..ServerConfig::default() }
}

#[tokio::test]
async fn a_write_key_scoped_to_one_team_cannot_score_another_teams_run() {
    let (app, _state, _store) = app_state_and_store(tenanted()).await;
    let mut body = a_score();
    body["correlation"] = json!({ "project": "team-b" });

    let res = send(&app, post("/v1/api/scores", Some("a-write"), body.to_string())).await;
    assert_eq!(res.status, StatusCode::FORBIDDEN, "one team annotated another team's runs");
    let (code, message) = res.error();
    assert_eq!(code, "forbidden");
    assert!(message.contains("team-b"), "the refusal should name the project: {message}");
}

#[tokio::test]
async fn a_write_key_scoped_to_one_team_can_still_score_its_own_run() {
    // The boundary has to scope rather than merely block.
    let (app, _state, _store) = app_state_and_store(tenanted()).await;
    let mut body = a_score();
    body["correlation"] = json!({ "project": "team-a" });

    let res = send(&app, post("/v1/api/scores", Some("a-write"), body.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "a key was refused its own project: {}", res.text());
}

#[tokio::test]
async fn a_write_key_scoped_to_one_team_cannot_leave_feedback_on_another_teams_run() {
    let (app, _state, _store) = app_state_and_store(tenanted()).await;
    let mut body = a_feedback();
    body["correlation"] = json!({ "project": "team-b" });

    let res = send(&app, post("/v1/api/feedback", Some("a-write"), body.to_string())).await;
    assert_eq!(res.status, StatusCode::FORBIDDEN, "{}", res.text());
}

#[tokio::test]
async fn a_write_key_scoped_to_one_team_cannot_create_a_dataset_in_another() {
    let (app, _state, _store) = app_state_and_store(tenanted()).await;
    let body = a_dataset("", "their cases", "team-b").to_string();

    let res = send(&app, post("/v1/api/datasets", Some("a-write"), body)).await;
    assert_eq!(res.status, StatusCode::FORBIDDEN, "{}", res.text());
}

#[tokio::test]
async fn a_write_key_scoped_to_one_team_cannot_create_an_experiment_in_another() {
    let (app, _state, _store) = app_state_and_store(tenanted()).await;
    let body = an_experiment("", "their run", "", "team-b").to_string();

    let res = send(&app, post("/v1/api/experiments", Some("a-write"), body)).await;
    assert_eq!(res.status, StatusCode::FORBIDDEN, "{}", res.text());
}

/// The ingest ceiling reaches the scorer-run door.
///
/// This door wrote its scores straight to the store, past the pipeline and past
/// every quota check, while `/v1/api/scores` next door answered 429 for the
/// same project in the same minute. A project whose ceiling was already spent
/// could land an unbounded number of rows here, counted by nothing.
///
/// The seeding spends part of the ceiling, which is deliberate: the interesting
/// case is a door reached when the project is already near its limit, not one
/// reached with a fresh window.
#[tokio::test]
async fn scores_from_a_scorer_run_are_charged_against_the_ceiling() {
    // Two seeded runs, two scorers, so four scores are computed. The ceiling
    // leaves room for fewer than that once seeding has spent its share.
    let config = ServerConfig { quota_per_minute: Some(3), ..fully_keyed() };
    let app = seeded(config).await;

    let body = json!({ "scorers": ["answered", "no_errors"], "limit": 10 }).to_string();
    let res = send(&app, post("/v1/api/score", Some(WRITE_KEY), body)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let out: Value = res.json();

    let written = out["scores_written"].as_u64().expect("scores_written");
    assert!(written < 4, "the ceiling did not reach this door: {}", res.text());

    // What did not fit is reported, not dropped in silence.
    let skipped = out["skipped"].as_array().expect("skipped");
    assert!(
        skipped.iter().any(|s| s["reason"].as_str().unwrap_or("").contains("ceiling")),
        "the refusal was not reported to the caller: {}",
        res.text()
    );
}

/// `from` bounds the window, rather than being accepted and dropped.
///
/// The spec documents it as a window bound and all six SDK models publish it,
/// so a caller who asked to score everything since a deploy was silently given
/// the newest `limit` runs instead. The request succeeded and answered a
/// different question, which is the defect class this suite exists to catch.
#[tokio::test]
async fn score_runs_honours_the_from_bound_instead_of_ignoring_it() {
    let app = seeded(fully_keyed()).await;

    // Both seeded runs fall on DAY. A bound after them must select nothing.
    let after = json!({ "from": "2099-01-01T00:00:00Z", "scorers": ["answered"] }).to_string();
    let res = send(&app, post("/v1/api/score", Some(WRITE_KEY), after)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body: Value = res.json();
    assert_eq!(
        body["traces_scored"],
        0,
        "a `from` after every stored run still scored something: {}",
        res.text()
    );

    // And a bound before them must still reach them, so the test cannot pass
    // by the endpoint simply scoring nothing.
    let before = json!({ "from": "2000-01-01T00:00:00Z", "scorers": ["answered"] }).to_string();
    let res = send(&app, post("/v1/api/score", Some(WRITE_KEY), before)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body: Value = res.json();
    assert!(
        body["traces_scored"].as_u64().unwrap_or(0) > 0,
        "a `from` before every stored run reached none of them: {}",
        res.text()
    );
}

/// The scorer-run door names its project in the BODY, and that is what made it
/// different from every other write route here.
///
/// It called the plain `authorize`, which answers "is this a write key" and
/// then discards the grant. The grant is the half that says which projects a
/// key reaches, so `team-a`'s key could name `team-b` in the body and have the
/// endpoint read `team-b`'s runs and write scores back into `team-b`. Reads and
/// writes, both across the boundary, through the one write door whose project
/// does not appear in the query string.
#[tokio::test]
async fn a_write_key_scoped_to_one_team_cannot_run_scorers_on_another_teams_runs() {
    let (app, _state, _store) = app_state_and_store(tenanted()).await;
    let body = json!({ "project": "team-b", "scorers": [] }).to_string();

    let res = send(&app, post("/v1/api/score", Some("a-write"), body)).await;
    assert_eq!(
        res.status,
        StatusCode::FORBIDDEN,
        "one team scored another team's runs: {}",
        res.text()
    );
    let (code, message) = res.error();
    assert_eq!(code, "forbidden");
    assert!(message.contains("team-b"), "the refusal should name the project: {message}");
}

/// The boundary has to scope rather than merely block, and the unnamed case is
/// where an over-tight fix would break: an absent `project` means the server
/// default, which `team-a`'s key does not reach either.
#[tokio::test]
async fn a_write_key_scoped_to_one_team_can_still_run_scorers_on_its_own() {
    let (app, _state, _store) = app_state_and_store(tenanted()).await;
    let body = json!({ "project": "team-a", "scorers": [] }).to_string();

    let res = send(&app, post("/v1/api/score", Some("a-write"), body)).await;
    assert_eq!(res.status, StatusCode::OK, "a key was refused its own project: {}", res.text());
}

#[tokio::test]
async fn a_dataset_id_cannot_be_taken_over_by_recreating_it_in_another_project() {
    // Without this the tenancy check on datasets is decoration: the item routes
    // resolve the owner from the store, so moving the owner moves every check
    // with it.
    let (app, _state, store) = app_state_and_store(tenanted()).await;
    store
        .put_dataset(&mcpd_core::Dataset {
            dataset_id: "shared-id".to_owned(),
            name: "team b cases".to_owned(),
            description: None,
            item_count: 0,
            created_at: OffsetDateTime::UNIX_EPOCH,
            correlation: correlation_for("team-b"),
        })
        .await
        .expect("seed the other team's dataset");

    let body = a_dataset("shared-id", "mine now", "team-a").to_string();
    let res = send(&app, post("/v1/api/datasets", Some("a-write"), body)).await;
    assert_eq!(res.status, StatusCode::FORBIDDEN, "a dataset id was seized: {}", res.text());
    assert!(
        res.error().1.contains("shared-id"),
        "the refusal should name the dataset: {}",
        res.error().1
    );
}

#[tokio::test]
async fn an_experiment_cannot_point_at_another_teams_dataset() {
    let (app, _state, store) = app_state_and_store(tenanted()).await;
    store
        .put_dataset(&mcpd_core::Dataset {
            dataset_id: "b-cases".to_owned(),
            name: "team b cases".to_owned(),
            description: None,
            item_count: 0,
            created_at: OffsetDateTime::UNIX_EPOCH,
            correlation: correlation_for("team-b"),
        })
        .await
        .expect("seed the other team's dataset");

    let body = an_experiment("", "sneaky", "b-cases", "team-a").to_string();
    let res = send(&app, post("/v1/api/experiments", Some("a-write"), body)).await;
    assert_eq!(res.status, StatusCode::FORBIDDEN, "an experiment read another team's cases");
}

// ---------------------------------------------------------- idempotency ---

#[tokio::test]
async fn a_replayed_batch_carrying_an_ingest_key_is_not_counted_twice() {
    let (app, _state, _store) = app_state_and_store(ServerConfig::default()).await;
    let mut batch = seed_batch();
    batch["ingest_key"] = json!("retry-after-a-timeout");

    let first = send(&app, post("/v1/ingest", None, batch.to_string())).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.text());
    assert_eq!(first.json()["accepted"], json!(2));
    assert_eq!(first.json()["duplicate"], json!(0));

    let second = send(&app, post("/v1/ingest", None, batch.to_string())).await;
    assert_eq!(second.status, StatusCode::OK, "{}", second.text());
    assert_eq!(second.json()["accepted"], json!(0), "a replay was stored again: {}", second.text());
    assert_eq!(second.json()["duplicate"], json!(2), "{}", second.text());
}

#[tokio::test]
async fn a_replayed_batch_does_not_duplicate_the_scores_it_carried() {
    // The counter is one thing; what the store holds afterwards is the claim.
    let (app, _state, _store) = app_state_and_store(ServerConfig::default()).await;
    let batch = json!({
        "ingest_key": "retry-after-a-timeout",
        "scores": [{
            "score_id": "s-1", "trace_id": RUN_OK, "scorer_id": "helpfulness",
            "source": "live", "value": 0.5,
            "timestamp": format!("{DAY}T00:01:00Z"),
            "correlation": { "project": "default" }
        }]
    });

    send(&app, post("/v1/ingest", None, batch.to_string())).await;
    send(&app, post("/v1/ingest", None, batch.to_string())).await;

    assert_eq!(stored_scores(&app, None).await.len(), 1, "the replay left a second score");
}

#[tokio::test]
async fn two_batches_with_different_ingest_keys_are_both_stored() {
    // The other half: the key must not swallow work that is genuinely new.
    let (app, _state, _store) = app_state_and_store(ServerConfig::default()).await;

    let mut first = json!({ "spans": [root_span(RUN_OK, OK_ROOT, "ok", None)] });
    first["ingest_key"] = json!("batch-1");
    let mut second = json!({ "spans": [root_span(RUN_FAILED, FAILED_ROOT, "error", None)] });
    second["ingest_key"] = json!("batch-2");

    assert_eq!(
        send(&app, post("/v1/ingest", None, first.to_string())).await.json()["accepted"],
        json!(1)
    );
    let res = send(&app, post("/v1/ingest", None, second.to_string())).await;
    assert_eq!(
        res.json()["accepted"],
        json!(1),
        "a new batch was mistaken for a replay: {}",
        res.text()
    );
    assert_eq!(res.json()["duplicate"], json!(0));
}

#[tokio::test]
async fn a_batch_with_no_ingest_key_is_stored_again_on_replay() {
    // Idempotency is opt-in and costs a key. A client that sends none is asking
    // for at-least-once, and this says so rather than leaving it to be guessed.
    let (app, _state, _store) = app_state_and_store(ServerConfig::default()).await;

    let first = send(&app, post("/v1/ingest", None, seed_batch().to_string())).await;
    assert_eq!(first.json()["accepted"], json!(2));
    let second = send(&app, post("/v1/ingest", None, seed_batch().to_string())).await;
    assert_eq!(second.json()["accepted"], json!(2), "{}", second.text());
    assert_eq!(second.json()["duplicate"], json!(0), "{}", second.text());
}

/// A score is a stored row on the same disk a quota exists to bound, so a
/// project barred from spending its ceiling on spans must not be able to spend
/// it here instead.
///
/// The charging helper shipped with three passing tests and NO production
/// caller: the tests called it directly, so they proved the arithmetic and not
/// the door. This drives the endpoint.
#[tokio::test]
async fn a_score_is_charged_against_the_project_ceiling() {
    let mut config = open();
    config.quota_per_minute = Some(2);
    let app = seeded(config).await;

    // A project the fixture never writes to, so the ceiling here is spent by
    // this test alone rather than by whatever seeding happened to store.
    let fresh = "quota-scores";
    for i in 0..2u32 {
        let mut body = a_score();
        body["score_id"] = json!(format!("within-{i}"));
        body["correlation"] = json!({ "project": fresh });
        let res = send(&app, post("/v1/api/scores", None, body.to_string())).await;
        assert_eq!(res.status, StatusCode::OK, "inside the ceiling: {}", res.text());
    }

    let mut body = a_score();
    body["score_id"] = json!("over");
    body["correlation"] = json!({ "project": fresh });
    let res = send(&app, post("/v1/api/scores", None, body.to_string())).await;
    assert_eq!(
        res.status,
        StatusCode::TOO_MANY_REQUESTS,
        "the ceiling did not reach this door: {}",
        res.text()
    );
    let (code, message) = res.error();
    assert_eq!(code, "over_quota");
    assert!(message.contains("ceiling"), "the refusal should say what happened: {message}");
}

/// The same door on the feedback side, which had the same gap.
#[tokio::test]
async fn feedback_is_charged_against_the_project_ceiling() {
    // Three, not one: the ceiling is per project and the fixture spends some of
    // the default project's before this test starts. The assertion below runs
    // against a project the fixture never touches, so three is its whole budget.
    let mut config = open();
    config.quota_per_minute = Some(3);
    let app = seeded(config).await;

    let mut body = a_feedback();
    body["correlation"] = json!({ "project": "quota-feedback" });

    for i in 0..3u32 {
        let res = send(&app, post("/v1/api/feedback", None, body.to_string())).await;
        assert_eq!(res.status, StatusCode::OK, "inside the ceiling at {i}: {}", res.text());
    }

    let second = send(&app, post("/v1/api/feedback", None, body.to_string())).await;
    assert_eq!(
        second.status,
        StatusCode::TOO_MANY_REQUESTS,
        "the ceiling did not reach this door: {}",
        second.text()
    );
}
