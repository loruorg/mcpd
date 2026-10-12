//! The run store under the erasure endpoint and the audit log.
//!
//! The run store keeps a run's input, output and state, and every step's input
//! and output, as they were sent. Erasing a person used to leave all of it in
//! place, and reading it back left no row in the audit log, while the same
//! payload read through the trace store did. Every request goes through the
//! real router, over the real run store and the real signal store.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mcpd_core::AuditResult;
use mcpd_runs::{RunStore, SqliteRunStore};
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::{AuditQuery, Store};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

const READ_KEY: &str = "read-key";
const ADMIN_KEY: &str = "admin-key";
const EXECUTE_KEY: &str = "execute-key";
const WORKER_KEY: &str = "worker-key";
const WRITE_KEY: &str = "write-key";
const OTHER_READ_KEY: &str = "other-project-read-key";

fn temp_dir() -> std::path::PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir = std::env::temp_dir().join(format!(
        "mcpd-runs-compliance-http-{}-{stamp}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir
}

/// Every scope keyed, plus a read key that reaches only the project `other`.
fn keyed() -> ServerConfig {
    let mut projects = std::collections::BTreeMap::new();
    projects.insert(
        "other".to_owned(),
        mcpd_server::state::ProjectConfig {
            read_keys: vec![OTHER_READ_KEY.to_owned()],
            ..Default::default()
        },
    );
    ServerConfig {
        ingest_keys: vec!["ingest-key".to_owned()],
        read_keys: vec![READ_KEY.to_owned()],
        write_keys: vec![WRITE_KEY.to_owned()],
        admin_keys: vec![ADMIN_KEY.to_owned()],
        execute_keys: vec![EXECUTE_KEY.to_owned()],
        worker_keys: vec![WORKER_KEY.to_owned()],
        projects,
        ..ServerConfig::default()
    }
}

async fn app(with_runs: bool) -> (Router, Arc<AppState>) {
    let dir = temp_dir();
    let store = SqliteStore::open(dir.join("signals.db"), 2).expect("open the store");
    store.migrate().await.expect("migrate the store");
    let mut state = AppState::new(Arc::new(store) as Arc<dyn Store>, keyed());
    if with_runs {
        let runs =
            SqliteRunStore::open(dir.join("runs.db"), state.config.run_config).expect("open runs");
        runs.migrate().await.expect("migrate runs");
        state = state.with_run_store(Arc::new(runs) as Arc<dyn RunStore>);
    }
    let state = Arc::new(state);
    (router(Arc::clone(&state)), state)
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    key: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {key}"));
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

async fn define(app: &Router) {
    let (status, body) = call(
        app,
        "POST",
        "/v1/api/definitions",
        WRITE_KEY,
        Some(json!({ "id": "reply", "body": { "steps": [{ "kind": "task", "id": "draft" }] } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// Start a run for `user`, carrying something only that person wrote.
async fn start(app: &Router, user: &str) -> Value {
    let (status, body) = call(
        app,
        "POST",
        "/v1/api/runs",
        EXECUTE_KEY,
        Some(json!({
            "definition_id": "reply",
            "input": { "message": format!("{user} wrote this") },
            "correlation": { "project": "default", "user_id": user, "session_id": "s-1" }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

/// Claim the run's one step and complete it, so the run ends.
async fn finish(app: &Router, run_id: &str) {
    let (status, body) = call(
        app,
        "POST",
        "/v1/api/steps/claim",
        WORKER_KEY,
        Some(json!({ "worker_id": "w-1", "max": 1, "wait_ms": 0, "run_id": run_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let lease = body["leases"][0]["lease_id"].as_str().expect("a lease").to_owned();
    let (status, body) = call(
        app,
        "POST",
        &format!("/v1/api/steps/{lease}/complete"),
        WORKER_KEY,
        Some(json!({ "output": { "draft": "a reply to the person" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["run_status"], json!("succeeded"));
}

async fn audit_rows(state: &AppState, project: &str, action: &str) -> Vec<mcpd_core::AuditEvent> {
    state
        .store
        .list_audit(&AuditQuery {
            project: project.to_owned(),
            action: Some(action.to_owned()),
            limit: 100,
            ..Default::default()
        })
        .await
        .expect("read the audit log")
}

// ----------------------------------------------------------------- erasure ---

#[tokio::test]
async fn erasing_a_person_removes_their_ended_runs_and_names_the_ones_still_going() {
    let (app, state) = app(true).await;
    define(&app).await;
    let ended = start(&app, "u_8841").await;
    let ended_id = ended["run_id"].as_str().expect("run id").to_owned();
    let trace_id = ended["trace_id"].as_str().expect("trace id").to_owned();
    finish(&app, &ended_id).await;
    assert!(mcpd_server::runs::drain_once(&state).await > 0, "the run's spans reach the trace");
    let (status, _) =
        call(&app, "GET", &format!("/v1/api/traces/{trace_id}"), READ_KEY, None).await;
    assert_eq!(status, StatusCode::OK, "the trace is there before the erasure");

    let live = start(&app, "u_8841").await;
    let live_id = live["run_id"].as_str().expect("run id").to_owned();
    let other = start(&app, "u_1234").await;
    let other_id = other["run_id"].as_str().expect("run id").to_owned();
    finish(&app, &other_id).await;

    let (status, body) =
        call(&app, "DELETE", "/v1/api/subjects/u_8841?project=default", ADMIN_KEY, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["runs_erased"], json!(1), "{body}");
    assert_eq!(body["runs_in_flight"], json!(1), "{body}");
    assert!(body["traces"].as_u64().unwrap_or(0) >= 1, "the trace store half still runs: {body}");
    let note = body["note"].as_str().unwrap_or_default();
    assert!(note.contains(&live_id), "the run left whole is named, so it can be found: {note}");
    assert!(!body.to_string().contains("u_8841"), "the subject is never echoed back: {body}");

    let (status, _) = call(&app, "GET", &format!("/v1/api/runs/{ended_id}"), READ_KEY, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the ended run is gone from the run store");
    let (status, _) =
        call(&app, "GET", &format!("/v1/api/traces/{trace_id}"), READ_KEY, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "and its trace from the trace store");
    let (status, body) =
        call(&app, "GET", &format!("/v1/api/runs/{live_id}"), READ_KEY, None).await;
    assert_eq!(status, StatusCode::OK, "a run still going is left whole: {body}");
    assert_eq!(body["run"]["status"], json!("running"));
    let (status, _) = call(&app, "GET", &format!("/v1/api/runs/{other_id}"), READ_KEY, None).await;
    assert_eq!(status, StatusCode::OK, "another person's run is not touched");
}

#[tokio::test]
async fn a_span_queued_before_the_erasure_does_not_land_after_it() {
    let (app, state) = app(true).await;
    define(&app).await;
    let run = start(&app, "u_8841").await;
    let run_id = run["run_id"].as_str().expect("run id").to_owned();
    let trace_id = run["trace_id"].as_str().expect("trace id").to_owned();
    finish(&app, &run_id).await;

    // The spans are queued in the run store and not yet drained.
    let (status, body) =
        call(&app, "DELETE", "/v1/api/subjects/u_8841?project=default", ADMIN_KEY, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["runs_erased"], json!(1), "{body}");

    // Left in the outbox, they would be written into the trace store now, with
    // the person's input and output, after the erasure said they were gone.
    assert_eq!(mcpd_server::runs::drain_once(&state).await, 0, "nothing left to drain");
    let (status, _) =
        call(&app, "GET", &format!("/v1/api/traces/{trace_id}"), READ_KEY, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_erasure_that_reached_only_the_run_store_counts_the_runs_in_its_note() {
    // The note read "Nothing matched" beside `runs_erased: 1`, because it only
    // counted the trace store. Twice: once beside another person's drained run,
    // and once in a trace store that holds nothing at all.
    for other_person_drained in [true, false] {
        let (app, state) = app(true).await;
        define(&app).await;
        if other_person_drained {
            let other = start(&app, "u_1234").await;
            finish(&app, other["run_id"].as_str().expect("run id")).await;
            assert!(mcpd_server::runs::drain_once(&state).await > 0);
        }
        let run = start(&app, "u_8841").await;
        finish(&app, run["run_id"].as_str().expect("run id")).await;

        let (status, body) =
            call(&app, "DELETE", "/v1/api/subjects/u_8841?project=default", ADMIN_KEY, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["runs_erased"], json!(1), "{body}");
        assert_eq!(body["deleted"], json!(0), "the spans were never drained: {body}");
        let note = body["note"].as_str().unwrap_or_default();
        assert!(!note.contains("Nothing matched"), "the erased run was not counted: {note}");
        assert!(
            !note.contains("Nothing in this project is attributed"),
            "the run store held an attributed run: {note}"
        );
        assert!(note.contains("1 durable run"), "the note does not count the runs: {note}");
    }
}

#[tokio::test]
async fn a_daemon_without_a_run_store_leaves_the_run_counts_out() {
    let (app, _state) = app(false).await;
    let (status, body) =
        call(&app, "DELETE", "/v1/api/subjects/u_8841?project=default", ADMIN_KEY, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.get("runs_erased").is_none(), "absent, not zero: {body}");
    assert!(body.get("runs_in_flight").is_none(), "absent, not zero: {body}");
}

// ------------------------------------------------------------------- audit ---

#[tokio::test]
async fn every_read_of_a_stored_run_payload_is_audited() {
    let (app, state) = app(true).await;
    define(&app).await;
    let run = start(&app, "u_8841").await;
    let run_id = run["run_id"].as_str().expect("run id").to_owned();

    let reads = [
        ("get_run", format!("/v1/api/runs/{run_id}")),
        ("list_runs", "/v1/api/runs?project=default".to_owned()),
        ("list_run_events", format!("/v1/api/runs/{run_id}/events")),
        ("get_step_cause", format!("/v1/api/runs/{run_id}/cause?step=draft")),
    ];
    for (action, path) in &reads {
        let (status, body) = call(&app, "GET", path, READ_KEY, None).await;
        assert_eq!(status, StatusCode::OK, "{action}: {body}");
        let rows = audit_rows(&state, "default", action).await;
        assert_eq!(rows.len(), 1, "{action} left no audit row");
        assert_eq!(rows[0].result, AuditResult::Allowed, "{action}");
    }
    let get_run = &audit_rows(&state, "default", "get_run").await[0];
    assert_eq!(get_run.resource.as_deref(), Some(run_id.as_str()), "the row names the run");

    // The worker path is not audited: it is a machine loop that claims many
    // times a minute, and a ledger row per claim would bury the reads.
    let (status, body) = call(
        &app,
        "POST",
        "/v1/api/steps/claim",
        WORKER_KEY,
        Some(json!({ "worker_id": "w-1", "max": 1, "wait_ms": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(audit_rows(&state, "default", "claim_steps").await.is_empty());

    let (status, body) =
        call(&app, "POST", &format!("/v1/api/runs/{run_id}/cancel"), EXECUTE_KEY, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = audit_rows(&state, "default", "cancel_run").await;
    assert_eq!(rows.len(), 1, "a cancel answers with the run's payload and ends it");
}

#[tokio::test]
async fn a_refused_read_of_a_run_is_recorded_against_the_project_that_owns_it() {
    let (app, state) = app(true).await;
    define(&app).await;
    let run = start(&app, "u_8841").await;
    let run_id = run["run_id"].as_str().expect("run id").to_owned();

    // The key passes the scope check and fails the project check. The row has
    // to record the second decision, which is the one that refused it.
    let (status, body) =
        call(&app, "GET", &format!("/v1/api/runs/{run_id}"), OTHER_READ_KEY, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let rows = audit_rows(&state, "default", "get_run").await;
    assert_eq!(rows.len(), 1, "a refusal is the row an audit log exists for");
    assert_eq!(rows[0].result, AuditResult::Refused);

    let (status, body) =
        call(&app, "GET", "/v1/api/runs?project=default", OTHER_READ_KEY, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let rows = audit_rows(&state, "default", "list_runs").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].result, AuditResult::Refused);

    // A key with the wrong scope is refused at the first check and recorded.
    let (status, _) = call(&app, "GET", &format!("/v1/api/runs/{run_id}"), WORKER_KEY, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(audit_rows(&state, "default", "get_run").await.len(), 2);
}

#[tokio::test]
async fn a_run_read_answered_with_400_leaves_no_row_saying_it_was_served() {
    let (app, state) = app(true).await;
    define(&app).await;
    let run = start(&app, "u_8841").await;
    let run_id = run["run_id"].as_str().expect("run id").to_owned();

    for (action, path) in [
        ("list_runs", "/v1/api/runs?project=default&status=nonsense".to_owned()),
        ("list_runs", "/v1/api/runs?project=default&cursor=not-a-cursor".to_owned()),
        ("get_step_cause", format!("/v1/api/runs/{run_id}/cause")),
        ("get_step_cause", format!("/v1/api/runs/{run_id}/cause?step=%20")),
    ] {
        let (status, body) = call(&app, "GET", &path, READ_KEY, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {body}");
        let rows = audit_rows(&state, "default", action).await;
        assert!(
            rows.is_empty(),
            "{path} was answered 400 and the log recorded {:?}",
            rows.iter().map(|r| &r.result).collect::<Vec<_>>()
        );
    }

    // The control: the same two reads, valid, are each one `allowed` row.
    let (status, body) = call(
        &app,
        "GET",
        "/v1/api/runs?project=default&status=running&cursor=1:run_a",
        READ_KEY,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) =
        call(&app, "GET", &format!("/v1/api/runs/{run_id}/cause?step=draft"), READ_KEY, None).await;
    assert_ne!(status, StatusCode::BAD_REQUEST, "{body}");
    for action in ["list_runs", "get_step_cause"] {
        let rows = audit_rows(&state, "default", action).await;
        assert_eq!(rows.len(), 1, "{action}: {rows:?}");
        assert_eq!(rows[0].result, AuditResult::Allowed, "{action}");
    }
}

#[tokio::test]
async fn a_refused_key_on_a_run_that_does_not_exist_is_still_recorded() {
    let (app, state) = app(true).await;
    // A key with the wrong scope, and no key at all. Neither may learn whether
    // the run exists, and both are refusals the log exists to hold. There is no
    // run to file them under, so they go under the default project.
    let (status, _) = call(&app, "GET", "/v1/api/runs/run_missing", WORKER_KEY, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) =
        call(&app, "POST", "/v1/api/runs/run_missing/cancel", "not-a-key", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    for action in ["get_run", "cancel_run"] {
        let rows = audit_rows(&state, "default", action).await;
        assert_eq!(rows.len(), 1, "{action} was refused and left no row");
        assert_eq!(rows[0].result, AuditResult::Refused, "{action}");
        assert_eq!(rows[0].resource.as_deref(), Some("run_missing"), "{action}");
    }

    // An accepted key on a missing run is a 404 and still no row: nothing was
    // handed out and the key was not refused.
    let (status, _) = call(&app, "GET", "/v1/api/runs/run_missing", READ_KEY, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(audit_rows(&state, "default", "get_run").await.len(), 1);
}
