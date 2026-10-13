//! Experiments case by case, and runs read as trajectories, through the real
//! router.

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
        std::env::temp_dir().join(format!("mcpd-experiments-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

async fn app() -> Router {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate");
    router(Arc::new(AppState::new(store as Arc<dyn Store>, ServerConfig::default())))
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

fn span(trace: &str, id: &str, parent: Option<&str>, kind: &str, name: &str, at: u32) -> Value {
    let mut s = json!({
        "trace_id": trace,
        "span_id": id,
        "name": name,
        "kind": kind,
        "status": "ok",
        "start_time": format!("2026-01-01T00:00:{at:02}Z"),
        "end_time": format!("2026-01-01T00:00:{:02}Z", at + 1),
        "is_root": parent.is_none(),
        "is_event": false,
        "correlation": { "project": "default" },
        "received_at": "2026-01-01T00:01:00Z"
    });
    if let Some(p) = parent {
        s["parent_span_id"] = Value::String(p.to_owned());
    }
    s
}

#[tokio::test]
async fn an_experiment_is_scored_against_each_case_and_read_back_case_by_case() {
    let app = app().await;
    let (s, ds) = call(
        &app,
        "POST",
        "/v1/api/datasets",
        Some(json!({
            "dataset_id": "", "name": "capitals", "item_count": 0,
            "created_at": "2026-01-01T00:00:00Z", "correlation": { "project": "default" }
        })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{ds}");
    let dataset = ds["dataset_id"].as_str().expect("an id").to_owned();
    let (s, _) = call(
        &app,
        "POST",
        &format!("/v1/api/datasets/{dataset}/items"),
        Some(json!([
            { "item_id": "fr", "dataset_id": "", "input": "France?", "expected": "Paris" },
            { "item_id": "de", "dataset_id": "", "input": "Germany?", "expected": "Berlin" }
        ])),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = call(
        &app,
        "POST",
        "/v1/api/experiments",
        Some(json!({
            "experiment_id": "exp-1", "name": "v2", "dataset_id": dataset, "status": "running",
            "item_count": 2, "correlation": { "project": "default" }
        })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    // What an SDK's runner records: one run per case, under the experiment,
    // naming the case it answered. The second answer is wrong.
    let run = |trace: &str, item: &str, input: &str, output: &str, at: u32| {
        let mut s = span(trace, &trace[16..], None, "agent_run", "capital", at);
        s["input"] = json!(input);
        s["output"] = json!(output);
        s["attributes"] = json!({ "mcpd.dataset_item_id": item });
        s["correlation"]["experiment_id"] = json!("exp-1");
        s
    };
    let (s, body) = call(
        &app,
        "POST",
        "/v1/ingest",
        Some(json!({ "spans": [
            run(&format!("{:032x}", 1), "fr", "France?", "Paris", 1),
            run(&format!("{:032x}", 2), "de", "Germany?", "Munich", 3),
        ]})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{body}");

    let (s, scored) = call(
        &app,
        "POST",
        "/v1/api/experiments/exp-1/score",
        Some(json!({ "scorers": ["exact_match"] })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{scored}");
    assert_eq!(scored["scores_written"], 2, "{scored}");
    assert!(scored["scores"]
        .as_array()
        .expect("scores")
        .iter()
        .all(|s| s["source"] == "experiment"));

    let (s, results) = call(&app, "GET", "/v1/api/experiments/exp-1/results", None).await;
    assert_eq!(s, StatusCode::OK, "{results}");
    assert_eq!(results["runs"], 2);
    let items = results["items"].as_array().expect("items");
    let by = |id: &str| items.iter().find(|i| i["item_id"] == id).cloned().expect("a row per case");
    assert_eq!(by("fr")["scores"]["exact_match"], 1.0);
    assert_eq!(by("de")["scores"]["exact_match"], 0.0);
    assert_eq!(by("de")["expected"], "Berlin");
    assert_eq!(results["scorers"][0]["scorer_id"], "exact_match");
    assert_eq!(results["scorers"][0]["mean"], 0.5);

    let (s, _) = call(&app, "GET", "/v1/api/experiments/nope/results", None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, listed) = call(&app, "GET", "/v1/api/traces?experiment_id=exp-1", None).await;
    assert_eq!(listed["traces"].as_array().map(Vec::len), Some(2));
}

#[tokio::test]
async fn a_run_reads_as_its_trajectory_and_scores_against_the_expected_path() {
    let app = app().await;
    let t = format!("{:032x}", 9);
    let mut root = span(&t, "0000000000000001", None, "agent_run", "refund", 0);
    root["end_time"] = json!("2026-01-01T00:00:20Z");
    root["output"] = json!("Refunded.");
    let mut gen =
        span(&t, "0000000000000002", Some("0000000000000001"), "model_generation", "plan", 1);
    gen["model"] = json!("m");
    let spans = vec![
        root,
        gen,
        span(&t, "0000000000000003", Some("0000000000000001"), "tool_call", "lookup_order", 3),
        span(&t, "0000000000000004", Some("0000000000000001"), "tool_call", "refund", 5),
        span(&t, "0000000000000005", Some("0000000000000001"), "model_step", "inner", 6),
    ];
    let (s, body) = call(&app, "POST", "/v1/ingest", Some(json!({ "spans": spans }))).await;
    assert_eq!(s, StatusCode::OK, "{body}");

    let (s, tr) = call(&app, "GET", &format!("/v1/api/traces/{t}/trajectory"), None).await;
    assert_eq!(s, StatusCode::OK, "{tr}");
    assert_eq!(tr["tool_sequence"], json!(["lookup_order", "refund"]));
    assert_eq!(tr["step_count"], 3, "a generation and two tool calls; the model step is inside");
    assert_eq!(tr["goal_reached"], true);
    assert_eq!(tr["steps"][1]["start_offset_ms"], 3000.0);

    let score = |cfg: Value| {
        let app = app.clone();
        let t = t.clone();
        async move {
            let (s, body) = call(
                &app,
                "POST",
                "/v1/api/score",
                Some(json!({ "trace_ids": [t], "scorers": ["trajectory"], "config": cfg, "dry_run": true })),
            )
            .await;
            assert_eq!(s, StatusCode::OK, "{body}");
            body["scores"][0]["value"].as_f64().unwrap_or(-1.0)
        }
    };
    assert!(
        (score(json!({"expected_tools": ["lookup_order", "refund"]})).await - 1.0).abs() < 1e-9
    );
    // Half the strict path, and an answer: (0.5 + 1) / 2.
    let strict =
        score(json!({"expected_tools": ["refund", "lookup_order"], "trajectory_match": "strict"}))
            .await;
    assert!((strict - 0.75).abs() < 1e-9, "{strict}");
    let steps = score(json!({"max_steps": 1})).await;
    assert!((steps - (1.0 / 3.0 + 1.0) / 2.0).abs() < 1e-9, "{steps}");
    let (s, _) = call(
        &app,
        "POST",
        "/v1/api/score",
        Some(json!({ "trace_ids": [t], "scorers": ["trajectory"], "config": {"expected_tools": ["a"], "trajectory_match": "loose"} })),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::BAD_REQUEST,
        "an unknown match mode is refused before any run is read"
    );
}
