//! The review queue, frozen dataset versions and tail sampling, through the
//! real router and store.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::Store;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

fn temp_db() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir =
        std::env::temp_dir().join(format!("mcpd-curation-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

async fn daemon(config: ServerConfig) -> (Router, Arc<AppState>) {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate");
    let state = Arc::new(AppState::new(store as Arc<dyn Store>, config));
    (router(Arc::clone(&state)), state)
}

async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let bytes = body.map(|b| b.to_string().into_bytes()).unwrap_or_default();
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("content-length", bytes.len().to_string())
        .body(Body::from(bytes))
        .expect("build the request");
    let res = app.clone().oneshot(req).await.expect("a response");
    let status = res.status();
    let raw = res.into_body().collect().await.expect("the body").to_bytes();
    (status, serde_json::from_slice(&raw).unwrap_or(Value::Null))
}

fn run(id: u32, status: &str, end: &str) -> Value {
    json!({
        "trace_id": format!("{id:032x}"),
        "span_id": format!("{id:016x}"),
        "name": "answer",
        "kind": "agent_run",
        "status": status,
        "start_time": "2026-01-01T00:00:00Z",
        "end_time": end,
        "is_root": true,
        "is_event": false,
        "correlation": { "project": "default" },
        "received_at": "2026-01-01T00:00:01Z"
    })
}

#[tokio::test]
async fn a_queued_run_is_assigned_reviewed_and_its_history_kept() {
    let (app, state) = daemon(ServerConfig::default()).await;
    let (s, _) = call(
        &app,
        "POST",
        "/v1/ingest",
        Some(json!({"spans": [run(1, "ok", "2026-01-01T00:00:01Z")]})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) =
        call(&app, "POST", "/v1/api/review-queue", Some(json!({"trace_id": "nope"}))).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "a run that does not exist cannot be queued");

    let (s, item) = call(
        &app,
        "POST",
        "/v1/api/review-queue",
        Some(json!({"trace_id": format!("{:032x}", 1), "note": "thumbs down"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{item}");
    assert_eq!(item["status"], "pending");
    let id = item["review_id"].as_str().expect("an id").to_owned();

    let (_, q) = call(&app, "GET", "/v1/api/review-queue?status=pending", None).await;
    assert_eq!(q["pending"], 1);
    assert_eq!(q["items"].as_array().map(Vec::len), Some(1));

    let (_, assigned) = call(
        &app,
        "PATCH",
        &format!("/v1/api/review-queue/{id}"),
        Some(json!({"assignee": "ana"})),
    )
    .await;
    assert_eq!(assigned["version"], 2);
    let (_, mine) = call(&app, "GET", "/v1/api/review-queue?assignee=ana", None).await;
    assert_eq!(mine["items"].as_array().map(Vec::len), Some(1));
    let (_, done) = call(
        &app,
        "PATCH",
        &format!("/v1/api/review-queue/{id}"),
        Some(json!({"status": "reviewed", "note": "the refund rule was wrong"})),
    )
    .await;
    assert_eq!(done["status"], "reviewed");
    assert_eq!(done["assignee"], "ana", "an absent field keeps its value");
    let (_, q) = call(&app, "GET", "/v1/api/review-queue?status=pending", None).await;
    assert_eq!(q["pending"], 0);
    let history = state
        .store
        .list_documents("default", mcpd_store::DocKind::Review, Some(&id))
        .await
        .expect("history");
    assert_eq!(history.len(), 3, "every change is kept");
    let (s, _) = call(&app, "GET", "/v1/api/review-queue?status=maybe", None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_frozen_dataset_version_keeps_its_cases() {
    let (app, _) = daemon(ServerConfig::default()).await;
    let (_, ds) = call(
        &app,
        "POST",
        "/v1/api/datasets",
        Some(json!({
            "dataset_id": "", "name": "faq", "item_count": 0,
            "created_at": "2026-01-01T00:00:00Z", "correlation": { "project": "default" }
        })),
    )
    .await;
    let id = ds["dataset_id"].as_str().expect("id").to_owned();
    let add = |item: &str| json!([{ "item_id": item, "dataset_id": "", "input": item }]);
    call(&app, "POST", &format!("/v1/api/datasets/{id}/items"), Some(add("a"))).await;
    let (s, v1) = call(
        &app,
        "POST",
        &format!("/v1/api/datasets/{id}/versions"),
        Some(json!({"note": "first"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v1}");
    assert_eq!(v1["version"], 1);
    assert_eq!(v1["item_count"], 1);
    let (_, again) =
        call(&app, "POST", &format!("/v1/api/datasets/{id}/versions"), Some(json!({}))).await;
    assert_eq!(again["version"], 1, "the same cases are the same version");

    call(&app, "POST", &format!("/v1/api/datasets/{id}/items"), Some(add("b"))).await;
    let (_, now) = call(&app, "GET", &format!("/v1/api/datasets/{id}/items"), None).await;
    assert_eq!(now.as_array().map(Vec::len), Some(2));
    let (_, then) =
        call(&app, "GET", &format!("/v1/api/datasets/{id}/items?version=1"), None).await;
    assert_eq!(then.as_array().map(Vec::len), Some(1), "version 1 holds what it froze");
    let (s, _) = call(&app, "GET", &format!("/v1/api/datasets/{id}/items?version=9"), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, versions) = call(&app, "GET", &format!("/v1/api/datasets/{id}/versions"), None).await;
    assert_eq!(versions[0]["note"], "first");
    let (s, _) = call(&app, "POST", "/v1/api/datasets/nope/versions", Some(json!({}))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tail_sampling_keeps_the_interesting_runs_and_drops_the_rest() {
    let rule: mcpd_server::tail::TailPolicy = serde_json::from_value(json!({
        "rate": 0.0, "keep_slower_than_ms": 5000, "decide_after_seconds": 60
    }))
    .expect("a rule");
    let (app, state) = daemon(ServerConfig::default()).await;
    let now = time::OffsetDateTime::now_utc();
    let fmt = |t: time::OffsetDateTime| {
        t.format(&time::format_description::well_known::Rfc3339).expect("a time")
    };
    let mut spans = Vec::new();
    for (id, status, secs) in [(1u32, "ok", 1), (2, "error", 1), (3, "ok", 10)] {
        let mut s = run(id, status, "");
        let start = now - time::Duration::minutes(5);
        s["start_time"] = json!(fmt(start));
        s["end_time"] = json!(fmt(start + time::Duration::seconds(secs)));
        spans.push(s);
    }
    // One run that finished a moment ago is not due yet.
    let mut fresh = run(4, "ok", "");
    fresh["start_time"] = json!(fmt(now - time::Duration::seconds(10)));
    fresh["end_time"] = json!(fmt(now - time::Duration::seconds(9)));
    spans.push(fresh);
    let (s, body) = call(&app, "POST", "/v1/ingest", Some(json!({"spans": spans}))).await;
    assert_eq!(s, StatusCode::OK, "{body}");

    let decided = Mutex::new(std::collections::HashMap::new());
    let dropped =
        mcpd_server::tail::pass(&state, "default", &rule, &decided, now).await.expect("a pass");
    assert_eq!(dropped, 1, "only the quick, clean, finished run is dropped");
    let (_, left) = call(&app, "GET", "/v1/api/traces", None).await;
    let mut ids: Vec<String> = left["traces"]
        .as_array()
        .expect("traces")
        .iter()
        .map(|t| t["trace_id"].as_str().unwrap_or_default().to_owned())
        .collect();
    ids.sort();
    assert_eq!(ids, [format!("{:032x}", 2), format!("{:032x}", 3), format!("{:032x}", 4)]);
}
