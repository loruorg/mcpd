//! The audit log records the response a request actually got.
//!
//! Every handler here used to write its `allowed` row and then validate the
//! request, so a parameter answered with 400 left a row saying a read was
//! served. Two others left no row at all for answers they gave: the aspect
//! notes route returned a refusal before writing anything, and the theme route
//! was not audited although a theme's label is the text of a note.
//!
//! Requests go through `mcpd_server::router` and the rows are read back from
//! the store, because the defect was always in the order of two calls inside a
//! handler, and only the real path shows the order.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mcpd_core::{AuditEvent, AuditResult};
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::{AuditQuery, Store};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

const READ_KEY: &str = "audit-order-read-key";
const INGEST_KEY: &str = "audit-order-ingest-key";
const WRITE_KEY: &str = "audit-order-write-key";
const TRACE_ID: &str = "0123456789abcdef0123456789abcdef";
const DATASET: &str = "audit-order-dataset";

/// A private database file per test, named from the pid and a stamp taken
/// once, because a recycled pid would otherwise reopen an earlier run's rows.
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
        std::env::temp_dir().join(format!("mcpd-audit-order-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

fn config() -> ServerConfig {
    ServerConfig {
        read_keys: vec![READ_KEY.to_owned()],
        ingest_keys: vec![INGEST_KEY.to_owned()],
        write_keys: vec![WRITE_KEY.to_owned()],
        derived_metrics: mcpd_server::derive::DerivedMetrics { durations: false, tokens: false },
        monitors: vec![serde_json::from_value(json!({
            "id": "errors",
            "name": "error rate",
            "project": "default",
            "watch": "error_rate",
            "compare": "above",
            "threshold": 0.1,
            "webhook": "https://hooks.slack.com/services/T0000/B0000/a-token-that-posts"
        }))
        .expect("a monitor")],
        slos: vec![serde_json::from_value(json!({
            "id": "availability",
            "name": "availability",
            "project": "default",
            "objective": 0.99,
            "webhook": "https://alerts.example.com/hook?token=a-token-that-posts"
        }))
        .expect("an objective")],
        ..ServerConfig::default()
    }
}

/// A router over a store holding one run and one dataset.
async fn seeded() -> (Router, Arc<SqliteStore>) {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let app = router(Arc::new(AppState::new(Arc::clone(&store) as Arc<dyn Store>, config())));
    let span = json!({
        "spans": [{
            "trace_id": TRACE_ID,
            "span_id": "0123456789abcdef",
            "name": "checkout",
            "kind": "agent_run",
            "status": "ok",
            "start_time": "2026-01-01T00:00:00Z",
            "end_time": "2026-01-01T00:00:01Z",
            "is_root": true,
            "is_event": false,
            "correlation": { "project": "default" },
            "received_at": "2026-01-01T00:00:01Z"
        }]
    });
    let res = send(&app, "POST", "/v1/ingest", Some(INGEST_KEY), Some(span)).await;
    assert_eq!(res.0, StatusCode::OK, "seeding the run failed: {}", res.1);
    let dataset: mcpd_core::Dataset = serde_json::from_value(json!({
        "dataset_id": DATASET,
        "name": "cases",
        "item_count": 0,
        "created_at": "2026-01-01T00:00:00Z",
        "correlation": { "project": "default" }
    }))
    .expect("a dataset");
    store.put_dataset(&dataset).await.expect("seed the dataset");
    (app, store)
}

async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    key: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(k) = key {
        b = b.header("authorization", format!("Bearer {k}"));
    }
    let req = match body {
        Some(v) => {
            let bytes = v.to_string().into_bytes();
            b.header("content-type", "application/json")
                .header("content-length", bytes.len().to_string())
                .body(Body::from(bytes))
        }
        None => b.body(Body::empty()),
    }
    .expect("build the request");
    let res = app.clone().oneshot(req).await.expect("a response");
    let status = res.status();
    let bytes = res.into_body().collect().await.expect("a body").to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn rows(store: &SqliteStore) -> Vec<AuditEvent> {
    store
        .list_audit(&AuditQuery { project: "default".to_owned(), limit: 100, ..Default::default() })
        .await
        .expect("read the audit log")
}

/// The rows a request added, given the log before it.
async fn added(store: &SqliteStore, before: usize) -> Vec<AuditEvent> {
    let all = rows(store).await;
    all[..all.len() - before].to_vec()
}

/// Send one request and return its status and the rows it wrote.
async fn audited_request(
    app: &Router,
    store: &SqliteStore,
    method: &str,
    uri: &str,
    key: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value, Vec<AuditEvent>) {
    let before = rows(store).await.len();
    let (status, body) = send(app, method, uri, key, body).await;
    (status, body, added(store, before).await)
}

// --------------------------------------------- a 400 is not a served read --

#[tokio::test]
async fn a_request_answered_with_400_leaves_no_row_saying_it_was_served() {
    let (app, store) = seeded().await;
    // `7c7c` is the hex of `||`: a cursor with the separator and no number.
    let cases: [(&str, &str, &str, Option<Value>); 13] = [
        ("GET", "/v1/api/spans?session_id=s1", READ_KEY, None),
        ("GET", "/v1/api/spans?sort=nonsense", READ_KEY, None),
        ("GET", "/v1/api/spans?cursor=not-a-cursor", READ_KEY, None),
        ("GET", "/v1/api/spans?cursor=7c7c", READ_KEY, None),
        ("GET", "/v1/api/spans?sort=cost&cursor=7c7c", READ_KEY, None),
        ("GET", "/v1/api/logs?level=loud", READ_KEY, None),
        ("GET", "/v1/api/logs?cursor=not-a-cursor", READ_KEY, None),
        ("GET", "/v1/api/logs?cursor=7c7c", READ_KEY, None),
        (
            "GET",
            "/v1/api/themes/t-1/examples?entity_kind=agent&entity_name=support&aspect=goal\
             &snapshot_id=snap-1&cursor=not-a-cursor",
            READ_KEY,
            None,
        ),
        ("GET", "/v1/api/feedback?kind=loud", READ_KEY, None),
        ("GET", "/v1/api/datasets/audit-order-dataset/items?limit=many", READ_KEY, None),
        (
            "POST",
            "/v1/api/datasets/audit-order-dataset/capture",
            WRITE_KEY,
            Some(json!({ "trace_ids": [] })),
        ),
        ("GET", "/v1/api/themes/t-1?entity_kind=agent&entity_name=support", READ_KEY, None),
    ];
    for (method, uri, key, body) in cases {
        let (status, answer, written) =
            audited_request(&app, &store, method, uri, Some(key), body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {uri}: {answer}");
        assert!(
            written.is_empty(),
            "{method} {uri} was answered 400 and the log recorded {:?}",
            written.iter().map(|r| (&r.action, &r.result)).collect::<Vec<_>>()
        );
    }
}

#[tokio::test]
async fn the_same_requests_when_valid_are_recorded_as_served() {
    // The control. Without it the test above passes for a handler that
    // records nothing at all.
    let (app, store) = seeded().await;
    // `31327c61` is the hex of `12|a`, and `312e357c61` of `1.5|a`: the shape
    // every store writes, a number and an id.
    let cases: [(&str, &str, &str, Option<Value>, &str); 8] = [
        ("GET", "/v1/api/spans", READ_KEY, None, "list_spans"),
        ("GET", "/v1/api/spans?cursor=31327c61", READ_KEY, None, "list_spans"),
        ("GET", "/v1/api/spans?sort=cost&cursor=312e357c61", READ_KEY, None, "list_spans"),
        ("GET", "/v1/api/logs", READ_KEY, None, "list_logs"),
        ("GET", "/v1/api/logs?cursor=31327c61", READ_KEY, None, "list_logs"),
        ("GET", &format!("/v1/api/feedback?trace_id={TRACE_ID}"), READ_KEY, None, "list_feedback"),
        (
            "POST",
            "/v1/api/datasets/audit-order-dataset/capture",
            WRITE_KEY,
            Some(json!({ "trace_ids": [TRACE_ID] })),
            "capture_into_dataset",
        ),
        ("GET", "/v1/api/datasets/audit-order-dataset/items", READ_KEY, None, "list_dataset_items"),
    ];
    for (method, uri, key, body, action) in cases {
        let (status, answer, written) =
            audited_request(&app, &store, method, uri, Some(key), body).await;
        assert_eq!(status, StatusCode::OK, "{method} {uri}: {answer}");
        assert_eq!(written.len(), 1, "{method} {uri}: {written:?}");
        assert_eq!(written[0].action, action);
        assert_eq!(written[0].result, AuditResult::Allowed);
    }
}

#[tokio::test]
async fn a_refusal_is_still_recorded_and_still_comes_before_validation() {
    // Validating first must not mean validating before the key is checked: a
    // caller with no key would then learn the vocabulary from the 400.
    let (app, store) = seeded().await;
    let (status, answer, written) =
        audited_request(&app, &store, "GET", "/v1/api/spans?session_id=s1", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{answer}");
    assert_eq!(written.len(), 1, "{written:?}");
    assert_eq!(written[0].action, "list_spans");
    assert_eq!(written[0].result, AuditResult::Refused);
}

// ------------------------------------------------------ the aspect notes --

#[tokio::test]
async fn every_answer_the_notes_route_gives_is_recorded() {
    let (app, store) = seeded().await;
    let uri = format!("/v1/api/traces/{TRACE_ID}/aspects");

    // Refused, by a key that is known and not a read key. The route returned
    // before writing anything.
    let (status, answer, written) =
        audited_request(&app, &store, "GET", &uri, Some(INGEST_KEY), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{answer}");
    assert_eq!(written.len(), 1, "a refusal of a note read left no row: {written:?}");
    assert_eq!(written[0].action, "read_trace_aspect_notes");
    assert_eq!(written[0].result, AuditResult::Refused);
    assert_eq!(written[0].resource.as_deref(), Some(TRACE_ID));

    // Served, for a run that carries no note. The row used to be written only
    // when a note was found.
    let (status, answer, written) =
        audited_request(&app, &store, "GET", &uri, Some(READ_KEY), None).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["notes"], json!([]));
    assert_eq!(written.len(), 1, "{written:?}");
    assert_eq!(written[0].result, AuditResult::Allowed);
    assert_eq!(written[0].project, "default");

    // A trace in no project the key reaches is still an allowed request.
    let (status, _, written) = audited_request(
        &app,
        &store,
        "GET",
        "/v1/api/traces/ffffffffffffffffffffffffffffffff/aspects",
        Some(READ_KEY),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(written.len(), 1, "{written:?}");
    assert_eq!(written[0].result, AuditResult::Allowed);
}

// --------------------------------------------------------------- a theme --

#[tokio::test]
async fn reading_a_theme_is_recorded_like_its_examples() {
    let (app, store) = seeded().await;
    let uri = "/v1/api/themes/t-refund?entity_kind=agent&entity_name=support&aspect=goal\
               &snapshot_id=snap-1";

    let (status, _, written) =
        audited_request(&app, &store, "GET", uri, Some(READ_KEY), None).await;
    // No snapshot is stored, so the read finds nothing; the row records that
    // the key was allowed to look, as every other audited read does.
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(written.len(), 1, "a theme read left no row: {written:?}");
    assert_eq!(written[0].action, "get_theme");
    assert_eq!(written[0].result, AuditResult::Allowed);
    assert_eq!(written[0].resource.as_deref(), Some("t-refund"));

    let (status, _, written) =
        audited_request(&app, &store, "GET", uri, Some(INGEST_KEY), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(written.len(), 1, "{written:?}");
    assert_eq!(written[0].result, AuditResult::Refused);
}

#[tokio::test]
async fn the_theme_flow_and_history_are_recorded_like_a_theme() {
    let (app, store) = seeded().await;
    // Both carry theme labels, and a label is the text of one note.
    for (uri, action) in [
        (
            "/v1/api/themes/flow?entity_kind=agent&entity_name=support&aspects=goal\
             &aspects=outcome&snapshot_id=snap-1",
            "get_theme_flow",
        ),
        (
            "/v1/api/themes/t-refund/history?entity_kind=agent&entity_name=support&aspect=goal",
            "list_theme_history",
        ),
    ] {
        let (_, _, written) = audited_request(&app, &store, "GET", uri, Some(READ_KEY), None).await;
        assert_eq!(written.len(), 1, "{action} left no row: {written:?}");
        assert_eq!(written[0].action, action);
        assert_eq!(written[0].result, AuditResult::Allowed);

        let (status, _, written) =
            audited_request(&app, &store, "GET", uri, Some(INGEST_KEY), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(written.len(), 1, "{action}: {written:?}");
        assert_eq!(written[0].result, AuditResult::Refused);
    }

    // A request answered with 400 says nothing was served.
    let bad = "/v1/api/themes/flow?entity_kind=agent&entity_name=support&aspects=goal\
               &snapshot_id=snap-1";
    let (status, _, written) =
        audited_request(&app, &store, "GET", bad, Some(READ_KEY), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(written.is_empty(), "a 400 left a row: {written:?}");
}

// ---------------------------------------------------------- the webhooks --

#[tokio::test]
async fn a_monitor_names_the_host_its_webhook_posts_to_and_not_the_token() {
    let (app, _) = seeded().await;
    let (status, monitors) = send(&app, "GET", "/v1/api/monitors", Some(READ_KEY), None).await;
    assert_eq!(status, StatusCode::OK, "{monitors}");
    assert_eq!(monitors[0]["monitor"]["webhook"], json!("hooks.slack.com"), "{monitors}");
    assert!(!monitors.to_string().contains("a-token-that-posts"), "{monitors}");

    let (status, slos) = send(&app, "GET", "/v1/api/slos", Some(READ_KEY), None).await;
    assert_eq!(status, StatusCode::OK, "{slos}");
    assert_eq!(slos[0]["slo"]["webhook"], json!("alerts.example.com"), "{slos}");
    assert!(!slos.to_string().contains("a-token-that-posts"), "{slos}");

    // The acknowledgement answers with the same object, to a write key.
    for uri in ["/v1/api/monitors/errors/ack", "/v1/api/slos/availability/ack"] {
        let (status, acked) = send(&app, "POST", uri, Some(WRITE_KEY), Some(json!({}))).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {acked}");
        assert!(!acked.to_string().contains("a-token-that-posts"), "{uri}: {acked}");
    }
}
