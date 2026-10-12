//! The cause endpoint, driven through the real router.
//!
//! The route, the extractors, the key scope, the project reach and the wire
//! shape are all under test here, because a handler with a green unit test and
//! no route behind it is a 404 with a passing suite.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mcpd_runs::{RunStore, SqliteRunStore};
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::Store;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

const WRITE_KEY: &str = "write-key";
const READ_KEY: &str = "read-key";
const INGEST_KEY: &str = "ingest-key";
const EXECUTE_KEY: &str = "execute-key";
const WORKER_KEY: &str = "worker-key";

/// A pid alone is not unique: pids are recycled, and a recycled one reopens
/// the previous run's database, so every count is off by a whole prior suite.
fn temp_dir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir = std::env::temp_dir().join(format!(
        "mcpd-cause-http-{}-{stamp}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir
}

fn keyed() -> ServerConfig {
    ServerConfig {
        ingest_keys: vec![INGEST_KEY.to_owned()],
        read_keys: vec![READ_KEY.to_owned()],
        write_keys: vec![WRITE_KEY.to_owned()],
        execute_keys: vec![EXECUTE_KEY.to_owned()],
        worker_keys: vec![WORKER_KEY.to_owned()],
        ..ServerConfig::default()
    }
}

async fn app() -> Router {
    let dir = temp_dir();
    let config = keyed();
    let store = SqliteStore::open(dir.join("signals.db"), 2).expect("open the store");
    store.migrate().await.expect("migrate the store");
    let runs = SqliteRunStore::open(dir.join("runs.db"), config.run_config).expect("open runs");
    runs.migrate().await.expect("migrate runs");
    let state = Arc::new(
        AppState::new(Arc::new(store) as Arc<dyn Store>, config)
            .with_run_store(Arc::new(runs) as Arc<dyn RunStore>),
    );
    router(state)
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    key: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(k) = key {
        req = req.header("authorization", format!("Bearer {k}"));
    }
    let req = match body {
        Some(b) => req.header("content-type", "application/json").body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .expect("build the request");
    let res = app.clone().oneshot(req).await.expect("route the request");
    let status = res.status();
    let bytes = res.into_body().collect().await.expect("read the body").to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({ "raw": String::from_utf8_lossy(&bytes) }))
    };
    (status, value)
}

/// A definition whose middle step is the one that goes quietly wrong: it
/// succeeds and hands an empty array to the step below it. The branch key
/// carries a slash on purpose, so the query parameter is exercised as the
/// reason it is a query parameter.
async fn define(app: &Router) {
    let doc = json!({
        "id": "triage",
        "kind": "workflow",
        "body": { "steps": [
            { "kind": "parallel", "id": "gather", "branches": [
                { "kind": "task", "id": "fetch", "input": { "id": "${input.case_id}" } }
            ]},
            { "kind": "task", "id": "charge",
              "input": { "lines": "${steps.gather.fetch.output.lines}" } }
        ]}
    });
    let (status, body) = call(app, "POST", "/v1/api/definitions", Some(WRITE_KEY), Some(doc)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn claim(app: &Router, run_id: &str) -> Value {
    let (status, body) = call(
        app,
        "POST",
        "/v1/api/steps/claim",
        Some(WORKER_KEY),
        Some(json!({ "worker_id": "w1", "max": 1, "wait_ms": 0, "run_id": run_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let leases = body["leases"].as_array().expect("leases").clone();
    assert_eq!(leases.len(), 1, "expected one lease: {body}");
    leases[0].clone()
}

/// Start a run, take the branch to a green end with an empty list, then fail
/// the step that read it.
async fn wrong_run(app: &Router) -> String {
    define(app).await;
    let (status, body) = call(
        app,
        "POST",
        "/v1/api/runs",
        Some(EXECUTE_KEY),
        Some(json!({ "definition_id": "triage", "input": { "case_id": "c_1" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let run_id = body["run_id"].as_str().expect("a run id").to_owned();

    let lease = claim(app, &run_id).await;
    assert_eq!(lease["step_key"], json!("gather/fetch"));
    let (status, body) = call(
        app,
        "POST",
        &format!("/v1/api/steps/{}/complete", lease["lease_id"].as_str().expect("lease id")),
        Some(WORKER_KEY),
        Some(json!({ "output": { "lines": [] } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let lease = claim(app, &run_id).await;
    assert_eq!(lease["step_key"], json!("charge"));
    let (status, body) = call(
        app,
        "POST",
        &format!("/v1/api/steps/{}/fail", lease["lease_id"].as_str().expect("lease id")),
        Some(WORKER_KEY),
        Some(json!({ "error": { "class": "fatal", "message": "nothing to charge" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    run_id
}

#[tokio::test]
async fn the_endpoint_names_the_green_step_the_bad_value_came_from() {
    let app = app().await;
    let run_id = wrong_run(&app).await;

    let (status, body) = call(
        &app,
        "GET",
        &format!("/v1/api/runs/{run_id}/cause?step=charge"),
        Some(READ_KEY),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["run_id"], json!(run_id));
    assert_eq!(body["step_key"], json!("charge"));
    assert_eq!(body["status"], json!("failed"));
    assert_eq!(body["attempts"], json!(1));
    assert_eq!(body["error"]["message"], json!("nothing to charge"));
    assert_eq!(body["truncated"], json!(false));

    let chain = body["chain"].as_array().expect("a chain").clone();
    assert_eq!(chain.len(), 1, "{body}");
    let link = &chain[0];
    // The wire is snake_case, and every field of the declared shape is here.
    assert_eq!(link["step_key"], json!("gather/fetch"));
    assert_eq!(link["status"], json!("succeeded"));
    assert_eq!(link["attempts"], json!(1));
    assert_eq!(link["template_path"], json!("steps.gather.fetch.output.lines"));
    assert_eq!(link["consumed_by"], json!("charge"));
    assert_eq!(link["depth"], json!(1));
    assert_eq!(link["silent_producer"], json!(true), "the step that did it is green");
    assert_eq!(link["value_kind"], json!("array"));
    assert_eq!(link["value_empty"], json!(true));
}

/// A step key with a slash goes through the query parameter unharmed, which is
/// the whole reason the key is not a path segment.
#[tokio::test]
async fn a_step_key_with_a_slash_is_addressable() {
    let app = app().await;
    let run_id = wrong_run(&app).await;
    let (status, body) = call(
        &app,
        "GET",
        &format!("/v1/api/runs/{run_id}/cause?step=gather%2Ffetch"),
        Some(READ_KEY),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["step_key"], json!("gather/fetch"));
    assert_eq!(body["status"], json!("succeeded"));
    assert!(body["chain"].as_array().expect("a chain").is_empty(), "it read only the run input");
}

#[tokio::test]
async fn the_step_parameter_is_required_and_an_unknown_step_is_not_found() {
    let app = app().await;
    let run_id = wrong_run(&app).await;

    let (status, body) =
        call(&app, "GET", &format!("/v1/api/runs/{run_id}/cause"), Some(READ_KEY), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["field"], json!("step"), "{body}");

    let (status, body) = call(
        &app,
        "GET",
        &format!("/v1/api/runs/{run_id}/cause?step=nowhere"),
        Some(READ_KEY),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) =
        call(&app, "GET", "/v1/api/runs/run_missing/cause?step=charge", Some(READ_KEY), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// The declared scope is the enforced one. An ingest key reads nothing.
#[tokio::test]
async fn it_needs_a_read_key() {
    let app = app().await;
    let run_id = wrong_run(&app).await;
    let path = format!("/v1/api/runs/{run_id}/cause?step=charge");

    let (status, _) = call(&app, "GET", &path, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = call(&app, "GET", &path, Some(INGEST_KEY), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = call(&app, "GET", &path, Some(READ_KEY), None).await;
    assert_eq!(status, StatusCode::OK);
}

/// `max_depth` is honoured and capped, so no request can ask for a longer walk
/// than the daemon is willing to do.
#[tokio::test]
async fn max_depth_is_honoured_and_capped() {
    let app = app().await;
    let run_id = wrong_run(&app).await;

    let (status, body) = call(
        &app,
        "GET",
        &format!("/v1/api/runs/{run_id}/cause?step=charge&max_depth=0"),
        Some(READ_KEY),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["chain"].as_array().expect("a chain").is_empty());
    assert_eq!(body["truncated"], json!(true));

    let (status, body) = call(
        &app,
        "GET",
        &format!("/v1/api/runs/{run_id}/cause?step=charge&max_depth=100000"),
        Some(READ_KEY),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["chain"].as_array().expect("a chain").len(), 1);
}
