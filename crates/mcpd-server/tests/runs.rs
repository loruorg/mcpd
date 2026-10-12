//! The run surface, driven through the real router.
//!
//! Every request goes through `mcpd_server::router`, so the path, the
//! extractors, the key scope, the project selector, the error mapping and the
//! 409 body are all under test. The store behind it is the real run store on
//! a private file, and the signal store is the real embedded one, so the last
//! test can read a run's trace back the way the interface does.

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
const EXECUTE_KEY: &str = "execute-key";
const WORKER_KEY: &str = "worker-key";
const OTHER_WORKER_KEY: &str = "other-project-worker-key";

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
        "mcpd-runs-http-{}-{stamp}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir
}

/// The router, its state, and the run store, over private files.
async fn app(config: ServerConfig) -> (Router, Arc<AppState>) {
    let dir = temp_dir();
    let store = SqliteStore::open(dir.join("signals.db"), 2).expect("open the store");
    store.migrate().await.expect("migrate the store");
    let runs = SqliteRunStore::open(dir.join("runs.db"), config.run_config).expect("open runs");
    runs.migrate().await.expect("migrate runs");
    let state = Arc::new(
        AppState::new(Arc::new(store) as Arc<dyn Store>, config)
            .with_run_store(Arc::new(runs) as Arc<dyn RunStore>),
    );
    (router(Arc::clone(&state)), state)
}

/// Every scope keyed separately, plus a second project whose worker key must
/// not reach the first project's leases.
fn keyed() -> ServerConfig {
    let mut projects = std::collections::BTreeMap::new();
    projects.insert(
        "other".to_owned(),
        mcpd_server::state::ProjectConfig {
            worker_keys: vec![OTHER_WORKER_KEY.to_owned()],
            ..Default::default()
        },
    );
    ServerConfig {
        ingest_keys: vec!["ingest-key".to_owned()],
        read_keys: vec![READ_KEY.to_owned()],
        write_keys: vec![WRITE_KEY.to_owned()],
        admin_keys: vec!["admin-key".to_owned()],
        execute_keys: vec![EXECUTE_KEY.to_owned()],
        worker_keys: vec![WORKER_KEY.to_owned()],
        projects,
        ..ServerConfig::default()
    }
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    key: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    call_with(app, method, path, key, body, &[]).await
}

async fn call_with(
    app: &Router,
    method: &str,
    path: &str,
    key: Option<&str>,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(k) = key {
        req = req.header("authorization", format!("Bearer {k}"));
    }
    for (name, value) in headers {
        req = req.header(*name, *value);
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

fn refund_definition() -> Value {
    json!({
        "id": "refund_review",
        "kind": "workflow",
        "body": {
            "limits": { "max_steps": 20, "run_timeout_ms": 600000 },
            "steps": [
                { "kind": "task", "id": "order", "input": { "order_id": "${input.order_id}" } },
                { "kind": "wait_event", "id": "approval", "event": "approved",
                  "key": "${input.order_id}", "timeout_ms": 60000,
                  "when": { "path": "steps.order.output.amount_usd", "op": "gt", "value": 100 } },
                { "kind": "task", "id": "refund", "effect": "at_most_once",
                  "input": { "amount": "${steps.order.output.amount_usd}" } }
            ],
            "output": { "refunded": "${steps.refund.output.refund_id}" }
        }
    })
}

async fn define(app: &Router) {
    let (status, body) =
        call(app, "POST", "/v1/api/definitions", Some(WRITE_KEY), Some(refund_definition())).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["version"], json!(1));
    assert_eq!(body["assigned"], json!(true));
}

async fn start(app: &Router, order_id: &str) -> String {
    let (status, body) = call(
        app,
        "POST",
        "/v1/api/runs",
        Some(EXECUTE_KEY),
        Some(json!({ "definition_id": "refund_review", "input": { "order_id": order_id } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["run_id"].as_str().expect("a run id").to_owned()
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

// ------------------------------------------------------------------ tests ---

#[tokio::test]
async fn a_whole_run_drives_through_the_worker_protocol_and_leaves_a_trace() {
    let (app, state) = app(keyed()).await;
    define(&app).await;
    let run_id = start(&app, "ord_1").await;

    let lease = claim(&app, &run_id).await;
    assert_eq!(lease["step_key"], json!("order"));
    assert_eq!(lease["input"], json!({ "order_id": "ord_1" }));
    assert_eq!(lease["definition_version"], json!(1));
    assert_eq!(lease["correlation"]["run_id"], json!(run_id));
    let lease_id = lease["lease_id"].as_str().expect("lease id").to_owned();

    let (status, body) =
        call(&app, "POST", &format!("/v1/api/steps/{lease_id}/started"), Some(WORKER_KEY), None)
            .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["reclaimed"], json!(false));
    let (status, _) =
        call(&app, "POST", &format!("/v1/api/steps/{lease_id}/heartbeat"), Some(WORKER_KEY), None)
            .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = call(
        &app,
        "POST",
        &format!("/v1/api/steps/{lease_id}/complete"),
        Some(WORKER_KEY),
        Some(json!({
            "output": { "amount_usd": 40 },
            "usage": { "input_tokens": 100, "output_tokens": 20, "model": "gpt-4o", "provider": "openai" }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["run_status"], json!("running"));
    assert_eq!(body["next"], json!("claim"), "the refund step is runnable now");

    // 40 USD skips the approval, so the refund is offered at once.
    let lease = claim(&app, &run_id).await;
    assert_eq!(lease["step_key"], json!("refund"));
    assert_eq!(lease["effect"], json!("at_most_once"));
    assert_eq!(lease["input"], json!({ "amount": 40 }));
    assert_eq!(lease["prior"]["approval"]["status"], json!("skipped"));
    assert_eq!(lease["prior"]["order"]["output"]["amount_usd"], json!(40));
    let lease_id = lease["lease_id"].as_str().expect("lease id").to_owned();

    // at_most_once without started is the worker's bug, and the body says so.
    let (status, body) = call(
        &app,
        "POST",
        &format!("/v1/api/steps/{lease_id}/complete"),
        Some(WORKER_KEY),
        Some(json!({ "output": { "refund_id": "re_1" } })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], json!("conflict"));
    assert_eq!(body["reason"], json!("not_started"));
    assert_eq!(body["run_status"], json!("running"));

    call(&app, "POST", &format!("/v1/api/steps/{lease_id}/started"), Some(WORKER_KEY), None).await;
    let (status, body) = call(
        &app,
        "POST",
        &format!("/v1/api/steps/{lease_id}/complete"),
        Some(WORKER_KEY),
        Some(json!({ "output": { "refund_id": "re_1" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["run_status"], json!("succeeded"));
    assert_eq!(body["next"], json!("idle"));

    // A second complete on the same lease is the worker's own landed call.
    let (status, body) = call(
        &app,
        "POST",
        &format!("/v1/api/steps/{lease_id}/complete"),
        Some(WORKER_KEY),
        Some(json!({ "output": { "refund_id": "re_2" } })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["reason"], json!("run_terminal"));

    let (status, detail) =
        call(&app, "GET", &format!("/v1/api/runs/{run_id}"), Some(READ_KEY), None).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["run"]["status"], json!("succeeded"));
    assert_eq!(detail["run"]["output"], json!({ "refunded": "re_1" }));
    assert_eq!(detail["run"]["steps_used"], json!(2));
    assert!(detail["run"]["cost_usd"].as_f64().expect("cost") > 0.0, "usage was priced: {detail}");
    let attempts = detail["attempts"].as_array().expect("attempts");
    assert_eq!(attempts.len(), 3);
    let order = attempts.iter().find(|a| a["step_key"] == "order").expect("order");
    assert_eq!(order["status"], json!("succeeded"));
    assert_eq!(order["usage"]["model"], json!("gpt-4o"));
    assert!(order["cost_usd"].as_f64().expect("cost") > 0.0);
    assert!(detail["timers"].as_array().expect("timers").is_empty());

    let (status, page) =
        call(&app, "GET", &format!("/v1/api/runs/{run_id}/events?after=-1"), Some(READ_KEY), None)
            .await;
    assert_eq!(status, StatusCode::OK);
    let kinds: Vec<&str> = page["events"]
        .as_array()
        .expect("events")
        .iter()
        .filter_map(|e| e["kind"].as_str())
        .collect();
    assert_eq!(kinds.first(), Some(&"run_started"));
    assert_eq!(kinds.last(), Some(&"run_ended"));
    assert!(kinds.contains(&"step_leased") && kinds.contains(&"step_finished"));
    assert!(kinds.contains(&"step_skipped"));
    assert_eq!(page["has_more"], json!(false));

    // The trace: drained into the signal store through the ordinary ingest path.
    let written = mcpd_server::runs::drain_once(&state).await;
    assert!(written >= 6, "expected the run's spans to be written, got {written}");
    let trace_id = detail["run"]["trace_id"].as_str().expect("trace id");
    let (status, trace) = call(
        &app,
        "GET",
        &format!("/v1/api/traces/{trace_id}?project=default"),
        Some(READ_KEY),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{trace}");
    let spans = trace["spans"].as_array().expect("spans");
    let names: Vec<&str> = spans.iter().filter_map(|s| s["name"].as_str()).collect();
    assert!(names.contains(&"refund_review"), "root span: {names:?}");
    assert!(names.contains(&"order") && names.contains(&"refund"), "step spans: {names:?}");
    let root = spans.iter().find(|s| s["name"] == "refund_review").expect("root");
    assert_eq!(root["kind"], json!("workflow_run"));
    assert_eq!(root["status"], json!("ok"));
    assert!(root["end_time"].is_string(), "the root span was closed: {root}");
    let order_span = spans.iter().find(|s| s["name"] == "order").expect("order span");
    assert_eq!(order_span["parent_span_id"], root["span_id"]);
    // The priced usage and cost ride in the step span's attributes. The span
    // itself carries neither, so a trace total counts the tokens on the model
    // spans a body records under it once and never the container's sum.
    assert_eq!(order_span["attributes"]["usage"]["model"], json!("gpt-4o"));
    assert!(order_span["attributes"]["cost_usd"].as_f64().expect("priced") > 0.0);
    assert!(order_span["model"].is_null(), "no model on the container: {order_span}");
    assert!(order_span["cost"].is_null(), "no cost on the container: {order_span}");
    // Draining again writes nothing: every row was acknowledged.
    assert_eq!(mcpd_server::runs::drain_once(&state).await, 0);
}

#[tokio::test]
async fn an_approval_pauses_the_run_until_an_event_arrives_and_a_cancel_ends_it() {
    let (app, _state) = app(keyed()).await;
    define(&app).await;
    let run_id = start(&app, "ord_2").await;
    let lease = claim(&app, &run_id).await;
    let lease_id = lease["lease_id"].as_str().expect("lease id").to_owned();
    let (status, _) = call(
        &app,
        "POST",
        &format!("/v1/api/steps/{lease_id}/complete"),
        Some(WORKER_KEY),
        Some(json!({ "output": { "amount_usd": 240 } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, detail) =
        call(&app, "GET", &format!("/v1/api/runs/{run_id}"), Some(READ_KEY), None).await;
    let waits = detail["waits"].as_array().expect("waits");
    assert_eq!(waits.len(), 1, "{detail}");
    assert_eq!(waits[0]["event"], json!("approved"));
    assert_eq!(waits[0]["key"], json!("ord_2"));
    assert_eq!(detail["run"]["cursor"]["construct"], json!("wait_event"));

    // Nothing to claim while the run waits, and the long poll returns on time.
    let started = std::time::Instant::now();
    let (status, body) = call(
        &app,
        "POST",
        "/v1/api/steps/claim",
        Some(WORKER_KEY),
        Some(json!({ "worker_id": "w1", "max": 1, "wait_ms": 300, "run_id": run_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["leases"].as_array().expect("leases").is_empty());
    assert!(started.elapsed() >= std::time::Duration::from_millis(250), "the poll waited");
    assert!(started.elapsed() < std::time::Duration::from_secs(3), "and did not hang");

    // An event with a supplied id delivers once and is a duplicate after.
    let (status, receipt) = call(
        &app,
        "POST",
        "/v1/api/events",
        Some(EXECUTE_KEY),
        Some(json!({ "event": "approved", "key": "ord_2", "payload": { "by": "ops" }, "event_id": "evt-1" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{receipt}");
    assert_eq!(receipt["outcome"], json!("delivered"));
    assert_eq!(receipt["event_id"], json!("evt-1"));
    let (_, again) = call(
        &app,
        "POST",
        "/v1/api/events",
        Some(EXECUTE_KEY),
        Some(json!({ "event": "approved", "key": "ord_2", "event_id": "evt-1" })),
    )
    .await;
    assert_eq!(again["outcome"], json!("duplicate"));

    let lease = claim(&app, &run_id).await;
    assert_eq!(lease["step_key"], json!("refund"));
    assert_eq!(lease["prior"]["approval"]["status"], json!("succeeded"));
    assert_eq!(lease["prior"]["approval"]["output"], json!({ "by": "ops" }));
    let lease_id = lease["lease_id"].as_str().expect("lease id").to_owned();
    call(&app, "POST", &format!("/v1/api/steps/{lease_id}/started"), Some(WORKER_KEY), None).await;

    // Cancel while the worker holds the refund: the worker learns on its next call.
    let (status, cancelled) =
        call(&app, "POST", &format!("/v1/api/runs/{run_id}/cancel"), Some(EXECUTE_KEY), None).await;
    assert_eq!(status, StatusCode::OK, "{cancelled}");
    assert_eq!(cancelled["status"], json!("cancelled"));
    assert!(cancelled["ended_at"].is_string());
    let (status, body) =
        call(&app, "POST", &format!("/v1/api/steps/{lease_id}/heartbeat"), Some(WORKER_KEY), None)
            .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["reason"], json!("run_terminal"));
    assert_eq!(body["run_status"], json!("cancelled"));

    // A signal to an ended run is a 404: it holds no wait.
    let (status, _) = call(
        &app,
        "POST",
        &format!("/v1/api/runs/{run_id}/signal"),
        Some(EXECUTE_KEY),
        Some(json!({ "event": "approved" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, listed) =
        call(&app, "GET", "/v1/api/runs?status=cancelled", Some(READ_KEY), None).await;
    assert_eq!(listed["runs"].as_array().expect("runs").len(), 1);
    assert_eq!(listed["page"]["has_more"], json!(false));
}

#[tokio::test]
async fn a_signal_reaches_one_run_by_its_id() {
    let (app, _state) = app(keyed()).await;
    define(&app).await;
    let first = start(&app, "ord_9").await;
    let second = start(&app, "ord_9").await;
    for run_id in [&first, &second] {
        let lease = claim(&app, run_id).await;
        let lease_id = lease["lease_id"].as_str().expect("lease id").to_owned();
        call(
            &app,
            "POST",
            &format!("/v1/api/steps/{lease_id}/complete"),
            Some(WORKER_KEY),
            Some(json!({ "output": { "amount_usd": 500 } })),
        )
        .await;
    }
    let (status, receipt) = call(
        &app,
        "POST",
        &format!("/v1/api/runs/{second}/signal"),
        Some(EXECUTE_KEY),
        Some(json!({ "event": "approved", "payload": { "by": "me" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{receipt}");
    assert_eq!(receipt["outcome"], json!("delivered"));
    let (_, second_detail) =
        call(&app, "GET", &format!("/v1/api/runs/{second}"), Some(READ_KEY), None).await;
    assert!(second_detail["waits"].as_array().expect("waits").is_empty(), "the named run moved");
    let (_, first_detail) =
        call(&app, "GET", &format!("/v1/api/runs/{first}"), Some(READ_KEY), None).await;
    assert_eq!(
        first_detail["waits"].as_array().expect("waits").len(),
        1,
        "the other one still waits"
    );
}

#[tokio::test]
async fn every_run_endpoint_enforces_its_scope_and_its_project() {
    let (app, _state) = app(keyed()).await;
    define(&app).await;
    let run_id = start(&app, "ord_3").await;

    // The wrong scope is a 403, no key is a 401.
    let (status, _) = call(
        &app,
        "POST",
        "/v1/api/runs",
        Some(READ_KEY),
        Some(json!({ "definition_id": "refund_review" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) =
        call(&app, "POST", "/v1/api/runs", None, Some(json!({ "definition_id": "refund_review" })))
            .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(
        &app,
        "POST",
        "/v1/api/steps/claim",
        Some(EXECUTE_KEY),
        Some(json!({ "worker_id": "w", "max": 1, "wait_ms": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) =
        call(&app, "GET", &format!("/v1/api/runs/{run_id}"), Some(WORKER_KEY), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a worker key reads no run");
    let (status, _) =
        call(&app, "POST", "/v1/api/definitions", Some(EXECUTE_KEY), Some(refund_definition()))
            .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // An admin key drives the run surface without a key of its own.
    let (status, _) = call(&app, "GET", "/v1/api/runs", Some("admin-key"), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "admin does not imply read");
    let (status, body) = call(
        &app,
        "POST",
        "/v1/api/steps/claim",
        Some("admin-key"),
        Some(json!({ "worker_id": "ops", "max": 1, "wait_ms": 0, "run_id": run_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let lease_id = body["leases"][0]["lease_id"].as_str().expect("lease id").to_owned();

    // A worker key scoped to another project cannot touch this lease, and the
    // refusal does not say whether the lease exists.
    let (status, body) = call(
        &app,
        "POST",
        &format!("/v1/api/steps/{lease_id}/heartbeat"),
        Some(OTHER_WORKER_KEY),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, body) = call(
        &app,
        "POST",
        "/v1/api/steps/claim",
        Some(OTHER_WORKER_KEY),
        Some(json!({ "worker_id": "w2", "max": 1, "wait_ms": 0, "project": "default" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    // With no project named, the single-project grant fills it in.
    let (status, body) = call(
        &app,
        "POST",
        "/v1/api/steps/claim",
        Some(OTHER_WORKER_KEY),
        Some(json!({ "worker_id": "w2", "max": 1, "wait_ms": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["leases"].as_array().expect("leases").is_empty());

    // Bounds on the claim are refused by name.
    let (status, body) = call(
        &app,
        "POST",
        "/v1/api/steps/claim",
        Some(WORKER_KEY),
        Some(json!({ "worker_id": "w", "max": 0, "wait_ms": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["field"], json!("max"));
}

#[tokio::test]
async fn definitions_are_versioned_and_a_taken_version_is_a_409() {
    let (app, _state) = app(keyed()).await;
    define(&app).await;
    let (status, body) =
        call(&app, "POST", "/v1/api/definitions", Some(WRITE_KEY), Some(refund_definition())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "id": "refund_review", "version": 1, "assigned": false }));

    let mut changed = refund_definition();
    changed["body"]["steps"][2]["id"] = json!("refund_now");
    changed["body"]["output"] = json!({ "refunded": "${steps.refund_now.output.refund_id}" });
    let (status, body) =
        call(&app, "POST", "/v1/api/definitions", Some(WRITE_KEY), Some(changed.clone())).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["version"], json!(2));

    changed["version"] = json!(1);
    let (status, body) =
        call(&app, "POST", "/v1/api/definitions", Some(WRITE_KEY), Some(changed)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], json!("version_taken"));

    let mut bad = refund_definition();
    bad["id"] = json!("broken");
    bad["body"]["steps"][0]["wehn"] = json!({ "path": "input.x", "op": "exists" });
    let (status, body) =
        call(&app, "POST", "/v1/api/definitions", Some(WRITE_KEY), Some(bad)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["message"].as_str().expect("message").contains("wehn"), "{body}");

    let (status, body) =
        call(&app, "GET", "/v1/api/definitions/refund_review", Some(READ_KEY), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["version"], json!(2), "the latest without a version named");
    let (_, body) =
        call(&app, "GET", "/v1/api/definitions/refund_review?version=1", Some(READ_KEY), None)
            .await;
    assert_eq!(body["version"], json!(1));
    let (_, body) = call(&app, "GET", "/v1/api/definitions", Some(READ_KEY), None).await;
    assert_eq!(body.as_array().expect("list").len(), 2);
    let (status, _) = call(&app, "GET", "/v1/api/definitions/nothing", Some(READ_KEY), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A register stores the same way and reports the lease timings.
    let (status, body) = call(
        &app,
        "POST",
        "/v1/api/workers/register",
        Some(WORKER_KEY),
        Some(json!({ "worker_id": "w1", "definitions": [refund_definition()] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["registered"][0]["version"], json!(1));
    assert_eq!(body["lease_ttl_ms"], json!(30000));
    assert_eq!(body["heartbeat_ms"], json!(10000));
}

#[tokio::test]
async fn a_daemon_without_a_run_store_says_so() {
    let dir = temp_dir();
    let store = SqliteStore::open(dir.join("signals.db"), 2).expect("open the store");
    store.migrate().await.expect("migrate the store");
    let app =
        router(Arc::new(AppState::new(Arc::new(store) as Arc<dyn Store>, ServerConfig::default())));
    let (status, body) = call(&app, "GET", "/v1/api/runs", None, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body["message"].as_str().expect("message").contains("--runs-data"));
}

#[tokio::test]
async fn the_event_stream_replays_from_a_sequence_and_closes_when_the_run_ends() {
    let (app, _state) = app(ServerConfig::default()).await;
    define(&app).await;
    let run_id = start(&app, "ord_4").await;
    let lease = claim(&app, &run_id).await;
    let lease_id = lease["lease_id"].as_str().expect("lease id").to_owned();
    call(
        &app,
        "POST",
        &format!("/v1/api/steps/{lease_id}/complete"),
        None,
        Some(json!({ "output": { "amount_usd": 10 } })),
    )
    .await;
    let lease = claim(&app, &run_id).await;
    let lease_id = lease["lease_id"].as_str().expect("lease id").to_owned();
    call(&app, "POST", &format!("/v1/api/steps/{lease_id}/started"), None, None).await;
    call(
        &app,
        "POST",
        &format!("/v1/api/steps/{lease_id}/complete"),
        None,
        Some(json!({ "output": { "refund_id": "re_9" } })),
    )
    .await;

    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/api/runs/{run_id}/events?after=2"))
        .header("accept", "text/event-stream")
        .body(Body::empty())
        .expect("request");
    let res = app.clone().oneshot(req).await.expect("route");
    assert_eq!(res.status(), StatusCode::OK);
    assert!(res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream")));
    let text =
        String::from_utf8(res.into_body().collect().await.expect("body").to_bytes().to_vec())
            .expect("utf8");
    assert!(
        !text.contains("id: 0\n") && !text.contains("id: 2\n"),
        "rows at or before `after` are not replayed: {text}"
    );
    assert!(text.contains("id: 3\n"), "the first row after the seek is sent: {text}");
    assert!(text.contains("event: run_ended"), "the stream carried the end and closed: {text}");
}
