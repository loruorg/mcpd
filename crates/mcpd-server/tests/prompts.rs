//! Prompt management through the real router: versions, labels, the fetch an
//! SDK makes, and the spans linked to a prompt.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::Store;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

const READ: &str = "prompt-read";
const WRITE: &str = "prompt-write";

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
        std::env::temp_dir().join(format!("mcpd-prompts-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

async fn app() -> Router {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate");
    let config = ServerConfig {
        read_keys: vec![READ.to_owned()],
        write_keys: vec![WRITE.to_owned()],
        ingest_keys: vec![WRITE.to_owned()],
        ..ServerConfig::default()
    };
    router(Arc::new(AppState::new(store as Arc<dyn Store>, config)))
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    key: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let bytes = body.map(|b| b.to_string().into_bytes()).unwrap_or_default();
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {key}"))
        .header("content-type", "application/json")
        .header("content-length", bytes.len().to_string())
        .body(Body::from(bytes))
        .expect("build the request");
    let res = app.clone().oneshot(req).await.expect("a response");
    let status = res.status();
    let raw = res.into_body().collect().await.expect("the body").to_bytes();
    (status, serde_json::from_slice(&raw).unwrap_or(Value::Null))
}

#[tokio::test]
async fn a_prompt_is_versioned_labelled_and_fetched_by_label() {
    let app = app().await;
    let (s, v1) = call(
        &app,
        "POST",
        "/v1/api/prompts",
        WRITE,
        Some(json!({"name": "support/triage", "template": "Sort this: {{ ticket }}", "note": "first"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v1}");
    assert_eq!(v1["version"], 1);
    assert_eq!(v1["kind"], "text");
    assert_eq!(v1["variables"], json!(["ticket"]));

    // The same body again is the same version, whatever the note says.
    let (_, again) = call(
        &app,
        "POST",
        "/v1/api/prompts",
        WRITE,
        Some(json!({"name": "support/triage", "template": "Sort this: {{ ticket }}", "note": "x"})),
    )
    .await;
    assert_eq!(again["version"], 1);

    let (_, v2) = call(
        &app,
        "POST",
        "/v1/api/prompts",
        WRITE,
        Some(json!({
            "name": "support/triage",
            "messages": [{"role": "system", "content": "Sort {{ticket}} for {{team}}"}],
            "labels": ["staging"]
        })),
    )
    .await;
    assert_eq!(v2["version"], 2);
    assert_eq!(v2["kind"], "chat");
    assert_eq!(v2["labels"], json!(["staging"]));
    assert_eq!(v2["variables"], json!(["ticket", "team"]));

    let name = "support%2Ftriage";
    let (s, _) = call(
        &app,
        "PUT",
        &format!("/v1/api/prompts/{name}/labels/production"),
        WRITE,
        Some(json!({"version": 1})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    let (_, prod) =
        call(&app, "GET", &format!("/v1/api/prompts/{name}?label=production"), READ, None).await;
    assert_eq!(prod["version"], 1, "{prod}");
    let (_, latest) = call(&app, "GET", &format!("/v1/api/prompts/{name}"), READ, None).await;
    assert_eq!(latest["version"], 2);
    let (_, latest) =
        call(&app, "GET", &format!("/v1/api/prompts/{name}?label=latest"), READ, None).await;
    assert_eq!(latest["version"], 2);

    let (_, history) =
        call(&app, "GET", &format!("/v1/api/prompts/{name}/versions"), READ, None).await;
    let versions: Vec<i64> = history["versions"]
        .as_array()
        .expect("versions")
        .iter()
        .map(|v| v["version"].as_i64().unwrap_or(0))
        .collect();
    assert_eq!(versions, [2, 1]);

    let (_, list) = call(&app, "GET", "/v1/api/prompts", READ, None).await;
    assert_eq!(list.as_array().map(Vec::len), Some(1));
    assert_eq!(list[0]["version"], 2);

    // Rolling back is moving the label.
    let (s, _) = call(
        &app,
        "PUT",
        &format!("/v1/api/prompts/{name}/labels/production"),
        WRITE,
        Some(json!({"version": 9})),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "a label cannot point at a version that does not exist");

    let (s, gone) =
        call(&app, "DELETE", &format!("/v1/api/prompts/{name}/labels/production"), WRITE, None)
            .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(gone["deleted"], 1);
    let (s, _) =
        call(&app, "GET", &format!("/v1/api/prompts/{name}?label=production"), READ, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn prompt_writes_are_checked_and_need_a_write_key() {
    let app = app().await;
    let (s, _) =
        call(&app, "POST", "/v1/api/prompts", READ, Some(json!({"name": "a", "template": "x"})))
            .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "a read key cannot store a prompt");
    let (s, body) = call(&app, "POST", "/v1/api/prompts", WRITE, Some(json!({"name": "a"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(body.to_string().contains("template"), "{body}");
    let (s, _) = call(
        &app,
        "POST",
        "/v1/api/prompts",
        WRITE,
        Some(json!({"name": "a", "template": "x", "labels": ["latest"]})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "`latest` is reserved");
    let (s, _) = call(&app, "GET", "/v1/api/prompts/nothing", READ, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn spans_filter_on_the_prompt_they_used() {
    let app = app().await;
    let span = |id: &str, version: u32| {
        json!({
            "trace_id": format!("{id:0>32}"),
            "span_id": format!("{id:0>16}"),
            "name": "answer",
            "kind": "model_generation",
            "status": "ok",
            "start_time": "2026-01-01T00:00:00Z",
            "end_time": "2026-01-01T00:00:01Z",
            "is_root": true,
            "is_event": false,
            "received_at": "2026-01-01T00:00:01Z",
            "attributes": {"mcpd.prompt.name": "greet", "mcpd.prompt.version": version},
            "correlation": {"project": "default"}
        })
    };
    let (s, body) = call(
        &app,
        "POST",
        "/v1/ingest",
        WRITE,
        Some(json!({"spans": [span("a1", 1), span("a2", 2)]})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    let (_, all) = call(&app, "GET", "/v1/api/spans?prompt=greet", READ, None).await;
    assert_eq!(all["spans"].as_array().map(Vec::len), Some(2), "{all}");
    let (_, one) =
        call(&app, "GET", "/v1/api/spans?prompt=greet&prompt_version=2", READ, None).await;
    assert_eq!(one["spans"].as_array().map(Vec::len), Some(1));
}
