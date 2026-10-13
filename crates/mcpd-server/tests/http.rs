//! The HTTP surface, driven through the real router.
//!
//! Every request here goes through `mcpd_server::router`, so a failure means
//! the wiring is wrong and not merely a handler: the path, the extractors, the
//! authorisation check, the body limit, the CORS layer and the error mapping
//! are all in the path under test. Calling a handler function directly proves
//! nothing about any of that, which is how a route can exist with no way to
//! reach it.

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::Store;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

const READ_KEY: &str = "read-only-key";
const INGEST_KEY: &str = "write-only-key";
const WRITE_KEY: &str = "change-stored-data-key";
const ADMIN_KEY: &str = "destroy-and-audit-key";

/// The run the fixture ingests, so a read route that needs a real identifier
/// has one.
const TRACE_ID: &str = "0123456789abcdef0123456789abcdef";
const SPAN_ID: &str = "0123456789abcdef";

/// Every read route the query API exposes, carrying whatever parameters that
/// route needs to get past its own validation.
///
/// Authorisation is checked before any of that validation runs, so this one
/// list drives both the refused case and the accepted case. A route added to
/// `lib.rs` and forgotten here is a route nobody proves is reachable.
const READ_ROUTES: &[&str] = &[
    "/v1/api/traces",
    "/v1/api/traces/0123456789abcdef0123456789abcdef",
    "/v1/api/spans",
    "/v1/api/logs",
    "/v1/api/metrics?name=mcpd.test.latency",
    "/v1/api/scores",
    // `trace_id` is required here, and refused when absent rather than answered
    // with an empty list. See `list_feedback`.
    "/v1/api/feedback?trace_id=0123456789abcdef0123456789abcdef",
    "/v1/api/analytics/summary",
    "/v1/api/analytics/cost",
    "/v1/api/projects",
    "/v1/api/capabilities",
    "/v1/api/discovery?facet=environment",
    "/v1/api/deltas?signal=traces",
    "/v1/api/scorers",
    "/v1/api/experiments/compare?baseline=before&candidate=after",
    "/v1/api/datasets",
    "/v1/api/datasets/some-dataset/items",
    "/v1/api/experiments",
    "/v1/api/prompts",
    "/v1/api/scorer-definitions",
    "/v1/api/evaluation-rules",
    "/v1/api/review-queue",
    "/v1/api/monitors",
    "/v1/api/slos",
];

/// The ingest doors, which take an ingest key rather than a read key.
const INGEST_ROUTES: &[&str] = &["/v1/traces", "/v1/logs", "/v1/metrics", "/v1/ingest"];

// ------------------------------------------------------------- fixtures ---

/// A private database file per test.
///
/// Not `:memory:`, deliberately: `SqliteStore::open` gives every in-memory
/// store the same shared-cache URI, so two tests running on two threads would
/// share one database and race on `migrate`. A file per test is just as
/// ephemeral and is actually isolated.
fn temp_db() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    // The pid alone is not unique: pids are recycled, and a recycled one reopens
    // the previous run's databases so every count is off by a whole prior suite.
    // That produced 89 false failures in one run of the write suite before it
    // was caught. A per-process stamp, taken once, makes the directory the run's
    // own. The same construction is in tests/sqlite.rs for the same reason.
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir = std::env::temp_dir().join(format!("mcpd-http-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

/// Keys for the two scopes that existed before `write` and `admin` did.
///
/// This is deliberately the *old* shape of a configuration, because that is what
/// every running deployment has: it exercises the fallbacks, where an unset
/// write list accepts the ingest keys and an unset admin list accepts the read
/// keys. [`fully_keyed`] is the new shape.
fn keyed() -> ServerConfig {
    ServerConfig {
        ingest_keys: vec![INGEST_KEY.to_owned()],
        read_keys: vec![READ_KEY.to_owned()],
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

/// The router, and the store behind it.
///
/// The audit log has no route in this build, so the only honest way to assert
/// that a read left a row is to drive the request through the real router and
/// then read the store it wrote to.
async fn app_and_store(config: ServerConfig) -> (Router, Arc<SqliteStore>) {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let app = router(Arc::new(AppState::new(Arc::clone(&store) as Arc<dyn Store>, config)));
    (app, store)
}

async fn app_with(config: ServerConfig) -> Router {
    app_and_store(config).await.0
}

/// The one span every read route can be pointed at.
fn fixture_batch() -> Value {
    json!({
        "spans": [{
            "trace_id": TRACE_ID,
            "span_id": SPAN_ID,
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
    })
}

/// A router whose store already holds [`TRACE_ID`], and that store.
async fn app_and_store_with_data(config: ServerConfig) -> (Router, Arc<SqliteStore>) {
    let key = if config.ingest_keys.is_empty() { None } else { Some(INGEST_KEY) };
    let (app, store) = app_and_store(config).await;
    let res = send(&app, post("/v1/ingest", key, fixture_batch().to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "seeding the fixture failed: {}", res.text());
    assert_eq!(res.json()["accepted"], json!(1), "the fixture span was not stored");

    // A real dataset, because the item routes now resolve the owning project
    // from the store to authorise against it. An id with nothing behind it is a
    // 404, so the fixture has to hold one for the read routes to be exercised.
    store
        .put_dataset(&mcpd_core::Dataset {
            dataset_id: "some-dataset".to_owned(),
            name: "fixture cases".to_owned(),
            description: None,
            item_count: 0,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            correlation: correlation_for("default"),
        })
        .await
        .expect("seed the fixture dataset");
    (app, store)
}

/// A router whose store already holds [`TRACE_ID`].
async fn app_with_data(config: ServerConfig) -> Router {
    app_and_store_with_data(config).await.0
}

/// Every audit row for the default project, newest first.
async fn audit_rows(store: &SqliteStore) -> Vec<mcpd_core::AuditEvent> {
    store
        .list_audit(&mcpd_store::AuditQuery {
            project: "default".to_owned(),
            limit: 100,
            ..Default::default()
        })
        .await
        .expect("read the audit log")
}

// -------------------------------------------------------------- plumbing ---

struct Res {
    status: StatusCode,
    headers: HeaderMap,
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
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.expect("read the response body").to_bytes().to_vec();
    Res { status, headers, body }
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

fn delete(uri: &str, key: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().method("DELETE").uri(uri);
    if let Some(k) = key {
        b = b.header("authorization", format!("Bearer {k}"));
    }
    b.body(Body::empty()).expect("build the request")
}

// ------------------------------------------------------ authorisation ---

#[tokio::test]
async fn every_read_route_refuses_a_caller_with_no_key() {
    let app = app_with_data(keyed()).await;
    for route in READ_ROUTES {
        let res = send(&app, get(route, None)).await;
        assert_eq!(res.status, StatusCode::UNAUTHORIZED, "{route} was open to anyone");
        let (code, message) = res.error();
        assert_eq!(code, "unauthorized", "{route} returned the wrong error code");
        assert!(message.contains("API key"), "{route} should say a key is needed, said: {message}");
    }
}

#[tokio::test]
async fn every_read_route_serves_a_caller_with_the_read_key() {
    let app = app_with_data(keyed()).await;
    for route in READ_ROUTES {
        let res = send(&app, get(route, Some(READ_KEY))).await;
        assert_eq!(res.status, StatusCode::OK, "{route} refused a valid read key: {}", res.text());
    }
}

#[tokio::test]
async fn every_ingest_route_refuses_a_caller_with_no_key() {
    let app = app_with(keyed()).await;
    for route in INGEST_ROUTES {
        let res = send(&app, post(route, None, "{}")).await;
        assert_eq!(res.status, StatusCode::UNAUTHORIZED, "{route} accepted anonymous writes");
        let (code, _) = res.error();
        assert_eq!(code, "unauthorized", "{route} returned the wrong error code");
    }
}

#[tokio::test]
async fn the_native_ingest_door_accepts_the_ingest_key() {
    let app = app_with(keyed()).await;
    let res = send(&app, post("/v1/ingest", Some(INGEST_KEY), fixture_batch().to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["accepted"], json!(1));
}

#[tokio::test]
async fn an_ingest_key_does_not_grant_read_access() {
    // The whole point of two scopes: an agent fleet holding a write key must
    // not be able to read back everyone else's prompts.
    let app = app_with_data(keyed()).await;
    for route in READ_ROUTES {
        let res = send(&app, get(route, Some(INGEST_KEY))).await;
        // 403, not 401: the key is a real one, it is the wrong one for here.
        assert_eq!(res.status, StatusCode::FORBIDDEN, "{route} let an ingest key read");
        let (code, message) = res.error();
        assert_eq!(code, "forbidden");
        assert!(
            message.contains("not valid for reading"),
            "{route} should say the key is wrong for this scope, said: {message}"
        );
    }
}

#[tokio::test]
async fn a_read_key_does_not_grant_write_access() {
    let app = app_with(keyed()).await;
    for route in INGEST_ROUTES {
        let res = send(&app, post(route, Some(READ_KEY), "{}")).await;
        assert_eq!(res.status, StatusCode::FORBIDDEN, "{route} let a read key write");
        let (code, message) = res.error();
        assert_eq!(code, "forbidden");
        assert!(
            message.contains("not valid for ingest"),
            "{route} should say the key is wrong for this scope, said: {message}"
        );
    }
}

#[tokio::test]
async fn acknowledging_an_alert_needs_a_write_key_not_a_read_key() {
    // Acknowledging changes state, so it sits in the ingest scope even though
    // it is reached from the same interface as the read routes. The 404 on the
    // second call is the proof the write key got past authorisation.
    let app = app_with(keyed()).await;

    let refused = send(&app, post("/v1/api/monitors/nope/ack", Some(READ_KEY), "")).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(refused.error().0, "forbidden");

    let allowed = send(&app, post("/v1/api/monitors/nope/ack", Some(INGEST_KEY), "")).await;
    assert_eq!(allowed.status, StatusCode::NOT_FOUND, "{}", allowed.text());
    assert_eq!(allowed.error().0, "not_found");
}

#[tokio::test]
async fn deleting_a_run_needs_a_key_and_then_actually_deletes_it() {
    let app = app_with_data(fully_keyed()).await;
    let path = format!("/v1/api/traces/{TRACE_ID}");

    let refused = send(&app, delete(&path, None)).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "anyone could delete a run");

    let deleted = send(&app, delete(&path, Some(ADMIN_KEY))).await;
    assert_eq!(deleted.status, StatusCode::OK, "{}", deleted.text());
    assert!(
        deleted.json()["deleted"].as_u64().unwrap_or(0) >= 1,
        "the delete reported nothing removed: {}",
        deleted.text()
    );

    let gone = send(&app, get(&path, Some(READ_KEY))).await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND, "the run survived its own deletion");
}

/// The bug this fixes. The reference has always documented
/// `DELETE /v1/api/traces/{id}` as needing the `admin` scope; the handler
/// enforced `Scope::Read`, so a key handed out as read-only could destroy a run
/// and the documentation said it could not.
#[tokio::test]
async fn a_read_key_can_no_longer_delete_a_run() {
    let app = app_with_data(fully_keyed()).await;
    let path = format!("/v1/api/traces/{TRACE_ID}");

    let refused = send(&app, delete(&path, Some(READ_KEY))).await;
    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "a read key deleted a run: {}",
        refused.text()
    );
    assert_eq!(refused.error().0, "forbidden");

    // And the run is still there, which is the part that actually matters.
    let still = send(&app, get(&path, Some(READ_KEY))).await;
    assert_eq!(still.status, StatusCode::OK, "the refused delete removed it anyway");
}

/// A write key changes stored data. It does not destroy it, and it does not read
/// the audit log.
#[tokio::test]
async fn a_write_key_cannot_reach_an_admin_endpoint() {
    let app = app_with_data(fully_keyed()).await;

    let refused = send(&app, delete(&format!("/v1/api/traces/{TRACE_ID}"), Some(WRITE_KEY))).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "a write key deleted a run");
    assert_eq!(refused.error().0, "forbidden");

    // The scope it does hold still works, so this is a boundary and not a
    // broken key.
    let allowed = send(
        &app,
        post(
            "/v1/api/scores",
            Some(WRITE_KEY),
            json!({
                "score_id": "", "trace_id": TRACE_ID, "scorer_id": "manual",
                "source": "manual", "value": 1.0,
                "timestamp": "2026-01-01T00:00:00Z",
                "correlation": { "project": "default" }
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(allowed.status, StatusCode::OK, "the write key could not write: {}", allowed.text());
}

/// An ingest key must not reach the endpoint that copies a stored run's input
/// and output into a dataset. That key lives in every container that emits a
/// span, including the ones running code you did not write.
#[tokio::test]
async fn an_ingest_key_cannot_capture_a_run_into_a_dataset() {
    let app = app_with_data(fully_keyed()).await;
    let body = json!({ "trace_ids": [TRACE_ID], "use_output_as_expected": true }).to_string();

    // The fixture dataset, because capture now also checks the project that owns
    // the destination: writing a case into another team's suite is the same
    // missing check as reading one out of it.
    let refused =
        send(&app, post("/v1/api/datasets/some-dataset/capture", Some(INGEST_KEY), body.clone()))
            .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "an ingest key captured a payload");

    let allowed =
        send(&app, post("/v1/api/datasets/some-dataset/capture", Some(WRITE_KEY), body)).await;
    assert_eq!(allowed.status, StatusCode::OK, "the write key was refused: {}", allowed.text());
}

// ---------------------------------------------------- upgrading in place ---

/// The upgrade guarantee, from the side that matters: a configuration written
/// before the write and admin scopes existed keeps working, unchanged, with
/// exactly the reach it had.
#[tokio::test]
async fn an_unset_admin_key_leaves_deletion_with_the_read_keys() {
    // `keyed()` is the old two-scope shape: no write_keys, no admin_keys.
    let app = app_with_data(keyed()).await;
    let path = format!("/v1/api/traces/{TRACE_ID}");

    let deleted = send(&app, delete(&path, Some(READ_KEY))).await;
    assert_eq!(
        deleted.status,
        StatusCode::OK,
        "upgrading revoked a key that was working: {}",
        deleted.text()
    );
    assert!(deleted.json()["deleted"].as_u64().unwrap_or(0) >= 1);
}

#[tokio::test]
async fn an_unset_write_key_leaves_writing_with_the_ingest_keys() {
    let app = app_with_data(keyed()).await;

    let body = json!({
        "score_id": "", "trace_id": TRACE_ID, "scorer_id": "manual",
        "source": "manual", "value": 1.0, "timestamp": "2026-01-01T00:00:00Z",
        "correlation": { "project": "default" }
    });
    let res = send(&app, post("/v1/api/scores", Some(INGEST_KEY), body.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "upgrading revoked the ingest key: {}", res.text());

    // Acknowledging is a write too, and reached the same way.
    let ack = send(&app, post("/v1/api/monitors/nope/ack", Some(INGEST_KEY), "")).await;
    assert_eq!(ack.status, StatusCode::NOT_FOUND, "{}", ack.text());
}

// ------------------------------------------------ unauthenticated routes ---

#[tokio::test]
async fn the_probes_and_the_scrape_endpoint_need_no_key() {
    // A kubelet cannot carry a credential and neither can a Prometheus scrape
    // job. Putting these behind the API key turns a healthy daemon into a
    // restart loop.
    let app = app_with(keyed()).await;

    let health = send(&app, get("/healthz", None)).await;
    assert_eq!(health.status, StatusCode::OK, "{}", health.text());
    assert_eq!(health.json()["status"], json!("ok"));

    let ready = send(&app, get("/readyz", None)).await;
    assert_eq!(ready.status, StatusCode::OK, "{}", ready.text());
    assert_eq!(ready.json()["status"], json!("ok"));

    let metrics = send(&app, get("/metrics", None)).await;
    assert_eq!(metrics.status, StatusCode::OK);
    assert!(
        metrics.text().contains("mcpd_uptime_seconds"),
        "the scrape body is not Prometheus exposition: {}",
        metrics.text()
    );
}

#[tokio::test]
async fn the_health_probe_carries_the_cors_header_the_browser_interface_needs() {
    // The operator interface may be served from another origin and probes
    // /healthz first. With no allow-origin header the browser blocks the read
    // and the interface can never connect, however healthy the daemon is.
    let app = app_with(keyed()).await;
    let req = Request::builder()
        .method("GET")
        .uri("/healthz")
        .header("origin", "https://console.example.test")
        .body(Body::empty())
        .expect("build the request");
    let res = send(&app, req).await;

    assert_eq!(res.status, StatusCode::OK);
    let allow = res
        .headers
        .get("access-control-allow-origin")
        .expect("/healthz answered with no access-control-allow-origin header");
    assert_eq!(allow, "*", "the probe must be readable from any origin");
}

#[tokio::test]
async fn the_scrape_endpoint_counts_the_ingest_it_actually_handled() {
    let app = app_with(keyed()).await;
    let before = send(&app, get("/metrics", None)).await.text();
    assert!(
        before.contains("mcpd_signals_accepted_total 0"),
        "a fresh server should have accepted nothing: {before}"
    );

    send(&app, post("/v1/ingest", Some(INGEST_KEY), fixture_batch().to_string())).await;

    let after = send(&app, get("/metrics", None)).await.text();
    assert!(
        after.contains("mcpd_signals_accepted_total 1"),
        "the accepted counter did not move: {after}"
    );
    assert!(
        after.contains("mcpd_ingest_requests_total 1"),
        "the ingest request counter did not move: {after}"
    );
}

// ------------------------------------------------------- malformed input ---

#[tokio::test]
async fn a_malformed_json_body_is_a_400_that_names_the_problem() {
    let app = app_with(ServerConfig::default()).await;

    let native = send(&app, post("/v1/ingest", None, "{\"spans\": [")).await;
    assert_eq!(native.status, StatusCode::BAD_REQUEST, "{}", native.text());
    let (code, message) = native.error();
    assert_eq!(code, "bad_request");
    assert!(
        message.contains("ingest batch"),
        "the message should name what failed to parse, said: {message}"
    );

    let otlp = send(&app, post("/v1/traces", None, "not json at all")).await;
    assert_eq!(otlp.status, StatusCode::BAD_REQUEST, "{}", otlp.text());
    let (code, message) = otlp.error();
    assert_eq!(code, "bad_request");
    assert!(
        message.contains("not valid JSON"),
        "the message should say the body was not JSON, said: {message}"
    );
}

#[tokio::test]
async fn a_body_over_the_configured_limit_is_refused() {
    let small = ServerConfig { max_body_bytes: 1024, ..ServerConfig::default() };
    let app = app_with(small).await;

    let under = send(&app, post("/v1/ingest", None, vec![b' '; 512])).await;
    assert_ne!(
        under.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "a body inside the limit was refused as too large"
    );

    let over = send(&app, post("/v1/ingest", None, vec![b' '; 4096])).await;
    assert_eq!(
        over.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "a body four times the limit was accepted: {}",
        over.text()
    );
}

#[tokio::test]
async fn an_unsupported_content_encoding_is_named_rather_than_guessed() {
    let app = app_with(ServerConfig::default()).await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/ingest")
        .header("content-type", "application/json")
        .header("content-encoding", "br")
        .body(Body::from("{}"))
        .expect("build the request");
    let res = send(&app, req).await;

    assert_eq!(res.status, StatusCode::BAD_REQUEST, "{}", res.text());
    let (_, message) = res.error();
    assert!(message.contains("gzip"), "the message should say what is supported, said: {message}");
}

// -------------------------------------------------- hostile query params ---

#[tokio::test]
async fn a_relative_time_ending_in_a_multibyte_character_is_a_400_not_a_panic() {
    // `-1é` used to split the string on a byte that is not a character
    // boundary, which panicked the handler instead of returning this 400.
    let app = app_with(ServerConfig::default()).await;
    for route in ["/v1/api/traces", "/v1/api/spans", "/v1/api/logs", "/v1/api/scores"] {
        let res = send(&app, get(&format!("{route}?from=-1%C3%A9"), None)).await;
        assert_eq!(
            res.status,
            StatusCode::BAD_REQUEST,
            "{route} did not reject the time: {}",
            res.text()
        );
        let (code, _) = res.error();
        assert_eq!(code, "bad_parameter", "{route} returned the wrong error code");
        assert_eq!(
            res.json()["field"],
            json!("from"),
            "{route} should name the parameter at fault"
        );
    }
}

#[tokio::test]
async fn a_bucket_ending_in_a_multibyte_character_is_a_400_not_a_panic() {
    // Same byte-versus-character bug, in the duration parser this time.
    let app = app_with(ServerConfig::default()).await;
    let res = send(&app, get("/v1/api/metrics?name=mcpd.test.latency&bucket=1%C3%A9", None)).await;

    assert_eq!(res.status, StatusCode::BAD_REQUEST, "{}", res.text());
    let (code, message) = res.error();
    assert_eq!(code, "bad_parameter");
    assert_eq!(res.json()["field"], json!("bucket"), "the failing parameter must be named");
    assert!(
        message.contains("unknown unit"),
        "the message should say what is wrong with it, said: {message}"
    );
}

#[tokio::test]
async fn a_garbage_cursor_is_a_400_rather_than_a_storage_error() {
    // A cursor is opaque, so a caller that mangles one gets no useful help
    // from a 500. The store must refuse it as an invalid query.
    let app = app_with(ServerConfig::default()).await;
    for route in ["/v1/api/traces", "/v1/api/spans", "/v1/api/logs", "/v1/api/scores"] {
        let res = send(&app, get(&format!("{route}?cursor=notacursor"), None)).await;
        assert_eq!(
            res.status,
            StatusCode::BAD_REQUEST,
            "{route} did not refuse a mangled cursor: {}",
            res.text()
        );
        let (code, message) = res.error();
        assert_eq!(code, "invalid_query", "{route} returned the wrong error code");
        assert!(
            message.contains("cursor"),
            "{route} should say the cursor is the problem, said: {message}"
        );
    }
}

#[tokio::test]
async fn an_unreadable_limit_names_the_parameter_it_could_not_read() {
    let app = app_with(ServerConfig::default()).await;
    let res = send(&app, get("/v1/api/traces?limit=lots", None)).await;

    assert_eq!(res.status, StatusCode::BAD_REQUEST, "{}", res.text());
    assert_eq!(res.error().0, "bad_parameter");
    assert_eq!(res.json()["field"], json!("limit"));
}

#[tokio::test]
async fn an_unsortable_column_lists_the_ones_that_work() {
    let app = app_with(ServerConfig::default()).await;
    let res = send(&app, get("/v1/api/traces?sort=whatever", None)).await;

    assert_eq!(res.status, StatusCode::BAD_REQUEST, "{}", res.text());
    let (code, message) = res.error();
    assert_eq!(code, "bad_parameter");
    assert!(
        message.contains("start_time") && message.contains("cost"),
        "the message should list the sortable columns, said: {message}"
    );
}

#[tokio::test]
async fn a_metric_window_that_ends_before_it_starts_is_refused() {
    let app = app_with(ServerConfig::default()).await;
    let res = send(
        &app,
        get(
            "/v1/api/metrics?name=mcpd.test.latency&from=2026-01-02T00:00:00Z&to=2026-01-01T00:00:00Z",
            None,
        ),
    )
    .await;

    assert_eq!(res.status, StatusCode::BAD_REQUEST, "{}", res.text());
    assert_eq!(res.json()["field"], json!("to"));
}

#[tokio::test]
async fn a_metric_query_with_no_name_says_so() {
    let app = app_with(ServerConfig::default()).await;
    let res = send(&app, get("/v1/api/metrics", None)).await;

    assert_eq!(res.status, StatusCode::BAD_REQUEST, "{}", res.text());
    assert_eq!(res.error().0, "bad_parameter");
    assert_eq!(res.json()["field"], json!("name"));
}

// --------------------------------------------------------- missing things ---

#[tokio::test]
async fn an_unknown_trace_is_a_404_with_an_error_body() {
    let app = app_with(ServerConfig::default()).await;
    let res = send(&app, get("/v1/api/traces/ffffffffffffffffffffffffffffffff", None)).await;

    assert_eq!(res.status, StatusCode::NOT_FOUND, "{}", res.text());
    let (code, message) = res.error();
    assert_eq!(code, "not_found");
    assert!(
        message.contains("ffffffffffffffffffffffffffffffff"),
        "the message should name what was missing, said: {message}"
    );
}

// -------------------------------------------------------------- round trip ---

#[tokio::test]
async fn a_span_posted_to_native_ingest_can_be_read_back() {
    // The end to end proof that ingest, storage and query agree on the model.
    let app = app_with_data(keyed()).await;

    let detail = send(&app, get(&format!("/v1/api/traces/{TRACE_ID}"), Some(READ_KEY))).await;
    assert_eq!(detail.status, StatusCode::OK, "{}", detail.text());
    let v = detail.json();
    assert_eq!(v["trace"]["trace_id"], json!(TRACE_ID));
    assert_eq!(v["spans"][0]["span_id"], json!(SPAN_ID));
    assert_eq!(v["spans"][0]["name"], json!("checkout"));

    let listed = send(&app, get("/v1/api/traces", Some(READ_KEY))).await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.text());
    let traces = listed.json();
    assert_eq!(
        traces["traces"].as_array().map(Vec::len),
        Some(1),
        "the list did not return the run that was just ingested: {}",
        listed.text()
    );
}

#[tokio::test]
async fn a_span_with_a_bad_identifier_is_rejected_without_failing_the_batch() {
    // One malformed id must not lose the good spans beside it, and the sender
    // has to be told which index was wrong.
    let app = app_with(keyed()).await;
    let mut batch = fixture_batch();
    let good = batch["spans"][0].clone();
    let mut bad = good.clone();
    bad["span_id"] = json!("not-hex");
    batch["spans"] = json!([bad, good]);

    let res = send(&app, post("/v1/ingest", Some(INGEST_KEY), batch.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let v = res.json();
    assert_eq!(v["accepted"], json!(1), "the valid span was lost: {}", res.text());
    assert_eq!(v["rejected"], json!(1), "the invalid span was not reported: {}", res.text());
    assert_eq!(v["errors"][0]["index"], json!(0), "the reported index is wrong: {}", res.text());
    assert_eq!(v["errors"][0]["signal"], json!("span"));
}

#[tokio::test]
async fn with_no_keys_configured_the_whole_surface_is_open() {
    // The first-run experience must not require a credential.
    let app = app_with_data(ServerConfig::default()).await;
    for route in READ_ROUTES {
        let res = send(&app, get(route, None)).await;
        assert_eq!(res.status, StatusCode::OK, "{route} refused an open server: {}", res.text());
    }
}

/// The privacy page promises redaction across all five signals. Feedback posted
/// through the API used to skip it entirely: `author_id` was stored raw and the
/// comment was never scanned, because the handler redacted only `metadata`
/// instead of going through the one intake every other write path uses.
#[tokio::test]
async fn feedback_posted_through_the_api_is_redacted_like_everything_else() {
    let app = app_with(ServerConfig::default()).await;

    let body = json!({
        "feedback_id": "",
        "trace_id": TRACE_ID,
        "kind": "comment",
        "value": "text",
        "comment": "my api_key is sk-live-abcdefghijklmnop, please help",
        "author_id": "customer-4477",
        "timestamp": "2026-01-01T00:00:00Z",
        "correlation": { "project": "default" }
    });

    let res = send(&app, post("/v1/api/feedback", None, body.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let stored = res.json();
    let author = stored["author_id"].as_str().unwrap_or_default();
    assert_ne!(author, "customer-4477", "the author must be hashed, not stored as sent");
    assert!(!author.is_empty(), "hashing must not drop the field");
}

// ------------------------------------------------------------- tenancy ---

/// A project is advertised as the tenancy key. Until keys were scoped to one,
/// that was a label: any valid read key opened every project's prompts, which
/// is the whole thing a platform team running mcpd for several teams needs not
/// to be true.
fn tenanted_config() -> ServerConfig {
    let mut projects = std::collections::BTreeMap::new();
    projects.insert(
        "team-a".to_owned(),
        mcpd_server::state::ProjectConfig {
            ingest_keys: vec!["a-write".to_owned()],
            read_keys: vec!["a-read".to_owned()],
            ..Default::default()
        },
    );
    projects.insert(
        "team-b".to_owned(),
        mcpd_server::state::ProjectConfig {
            ingest_keys: vec!["b-write".to_owned()],
            read_keys: vec!["b-read".to_owned()],
            ..Default::default()
        },
    );
    ServerConfig {
        // A top-level key still reaches everything: that is the single-tenant
        // path and it must keep working.
        read_keys: vec!["operator".to_owned()],
        projects,
        ..ServerConfig::default()
    }
}

/// A `Correlation` naming only its project, which is all these fixtures need.
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

fn span_in(project: &str, trace: &str, span: &str) -> Value {
    json!({
        "trace_id": trace, "span_id": span, "name": "run", "kind": "agent_run",
        "status": "ok", "start_time": "2026-01-01T00:00:00Z",
        "end_time": "2026-01-01T00:00:01Z", "is_root": true, "is_event": false,
        "received_at": "2026-01-01T00:00:01Z",
        "input": "a prompt that belongs to one team",
        "correlation": { "project": project }
    })
}

#[tokio::test]
async fn one_teams_read_key_does_not_open_another_teams_prompts() {
    let app = app_with(tenanted_config()).await;
    let t_a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    let seeded = send(
        &app,
        post(
            "/v1/ingest",
            Some("a-write"),
            json!({ "spans": [span_in("team-a", t_a, "1111111111111111")] }).to_string(),
        ),
    )
    .await;
    assert_eq!(seeded.status, StatusCode::OK, "{}", seeded.text());

    // The owning team reads it.
    let mine = send(&app, get("/v1/api/traces?project=team-a", Some("a-read"))).await;
    assert_eq!(mine.status, StatusCode::OK, "{}", mine.text());
    assert_eq!(mine.json()["traces"].as_array().map(Vec::len), Some(1));

    // The other team does not, and is not told whether the project exists. A
    // valid key that reaches other projects is 403: the credential is fine, the
    // reach is not.
    let theirs = send(&app, get("/v1/api/traces?project=team-a", Some("b-read"))).await;
    assert_eq!(theirs.status, StatusCode::FORBIDDEN, "{}", theirs.text());
    assert!(
        !theirs.error().1.contains("does not exist"),
        "the refusal must not say whether the project is real: {}",
        theirs.error().1
    );

    // And a single-tenant operator key still reaches everything.
    let operator = send(&app, get("/v1/api/traces?project=team-a", Some("operator"))).await;
    assert_eq!(operator.status, StatusCode::OK, "{}", operator.text());
}

#[tokio::test]
async fn a_scoped_ingest_key_cannot_write_into_another_project() {
    let app = app_with(tenanted_config()).await;

    // One batch naming two projects, presented with a key that reaches one.
    let body = json!({
        "spans": [
            span_in("team-a", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "2222222222222222"),
            span_in("team-b", "cccccccccccccccccccccccccccccccc", "3333333333333333")
        ]
    });
    let res = send(&app, post("/v1/ingest", Some("a-write"), body.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let out = res.json();
    assert_eq!(out["accepted"], json!(1), "only the reachable span is stored: {out}");
    assert_eq!(out["rejected"], json!(1), "the other is refused, not dropped in silence: {out}");

    // And it really did not land.
    let peek = send(&app, get("/v1/api/traces?project=team-b", Some("operator"))).await;
    assert_eq!(peek.status, StatusCode::OK, "{}", peek.text());
    assert_eq!(
        peek.json()["traces"].as_array().map(Vec::len),
        Some(0),
        "a key scoped to team-a wrote into team-b"
    );
}

/// A dataset captured from production holds real prompts and completions. The
/// items endpoint is addressed by dataset id alone, so before the owning project
/// was looked up, a read key that learned an id read another team's payloads.
#[tokio::test]
async fn a_scoped_read_key_cannot_read_another_projects_dataset_items() {
    let app = app_with(tenanted_config()).await;

    // team-b owns a dataset, created with team-b's own key.
    let made = send(
        &app,
        post(
            "/v1/api/datasets",
            Some("b-write"),
            json!({
                "dataset_id": "ds-team-b", "name": "b regressions",
                "item_count": 0, "created_at": "2026-01-01T00:00:00Z",
                "correlation": { "project": "team-b" }
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(made.status, StatusCode::OK, "{}", made.text());

    // team-a knows the id and asks for its cases.
    let stolen = send(&app, get("/v1/api/datasets/ds-team-b/items", Some("a-read"))).await;
    assert_eq!(
        stolen.status,
        StatusCode::FORBIDDEN,
        "a key scoped to team-a read team-b's dataset items: {}",
        stolen.text()
    );

    // The owner still reaches it, so the check scopes rather than merely blocks.
    let owner = send(&app, get("/v1/api/datasets/ds-team-b/items", Some("b-read"))).await;
    assert_eq!(owner.status, StatusCode::OK, "{}", owner.text());
}

/// The same missing check on the write side: a case written into another team's
/// dataset corrupts the suite they gate releases on.
#[tokio::test]
async fn a_scoped_write_key_cannot_add_items_to_another_projects_dataset() {
    let app = app_with(tenanted_config()).await;
    let made = send(
        &app,
        post(
            "/v1/api/datasets",
            Some("b-write"),
            json!({
                "dataset_id": "ds-b2", "name": "b cases", "item_count": 0,
                "created_at": "2026-01-01T00:00:00Z",
                "correlation": { "project": "team-b" }
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(made.status, StatusCode::OK, "{}", made.text());

    let res = send(
        &app,
        post(
            "/v1/api/datasets/ds-b2/items",
            Some("a-write"),
            json!([{ "item_id": "i1", "dataset_id": "ds-b2", "input": "x" }]).to_string(),
        ),
    )
    .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN, "{}", res.text());
}

/// An id with no dataset behind it is a 404 rather than an empty list, so a
/// typo reads as a mistake instead of as a dataset that exists and is empty.
///
/// This does distinguish "no such dataset" from "not yours", which is a
/// deliberate trade: dataset ids are random 32-hex, so the caller must already
/// know one for the distinction to tell them anything, and 403 is what lets an
/// operator work out that they need a key for another project.
#[tokio::test]
async fn an_unknown_dataset_is_a_404_rather_than_an_empty_list() {
    let app = app_with(tenanted_config()).await;
    let res = send(&app, get("/v1/api/datasets/no-such-dataset/items", Some("a-read"))).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND, "{}", res.text());
}

/// A score is attached to somebody's run. Writing one into another project is
/// annotating their data.
#[tokio::test]
async fn a_scoped_write_key_cannot_score_another_projects_run() {
    let app = app_with(tenanted_config()).await;
    let res = send(
        &app,
        post(
            "/v1/api/scores",
            Some("a-write"),
            json!({
                "score_id": "s1", "trace_id": "dddddddddddddddddddddddddddddddd",
                "scorer_id": "manual", "source": "manual", "value": 1.0,
                "timestamp": "2026-01-01T00:00:00Z",
                "correlation": { "project": "team-b" }
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN, "{}", res.text());
}

/// Project names are not payload, but they are the org chart. A key scoped to
/// one project used to enumerate every other one.
#[tokio::test]
async fn a_scoped_key_sees_only_its_own_projects_in_the_listing() {
    let app = app_with(tenanted_config()).await;

    // Seed each project with its own key: there is no top-level ingest key in
    // this fixture, which is the point of it.
    for (key, project, trace, span) in [
        ("a-write", "team-a", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "1111111111111111"),
        ("b-write", "team-b", "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", "4444444444444444"),
    ] {
        let seed = json!({ "spans": [span_in(project, trace, span)] });
        let seeded = send(&app, post("/v1/ingest", Some(key), seed.to_string())).await;
        assert_eq!(seeded.status, StatusCode::OK, "{}", seeded.text());
    }

    let scoped = send(&app, get("/v1/api/projects", Some("a-read"))).await;
    assert_eq!(scoped.status, StatusCode::OK, "{}", scoped.text());
    let names: Vec<String> = serde_json::from_value(scoped.json()).unwrap_or_default();
    assert!(names.iter().all(|n| n == "team-a"), "team-a's key saw {names:?}");

    // The operator key is not scoped, so it still sees the whole store.
    let all = send(&app, get("/v1/api/projects", Some("operator"))).await;
    let all_names: Vec<String> = serde_json::from_value(all.json()).unwrap_or_default();
    assert!(all_names.len() >= 2, "an unscoped key should see every project: {all_names:?}");
}

/// The read paths resolve a dataset's owner before returning its cases. That is
/// only a boundary if the ownership record cannot be overwritten, and it could
/// be: re-creating another project's dataset id under your own moved the owner,
/// and every check built on it then said yes.
///
/// Reproduced end to end against a real two-tenant daemon before it was closed.
/// team-a got a 403 on the items endpoint, posted the same id under its own
/// project, and the second read returned team-b's payload.
#[tokio::test]
async fn a_dataset_id_cannot_be_seized_by_recreating_it_in_another_project() {
    let app = app_with(tenanted_config()).await;

    let made = send(
        &app,
        post(
            "/v1/api/datasets",
            Some("b-write"),
            json!({
                "dataset_id": "ds-owned-by-b", "name": "b regressions", "item_count": 0,
                "created_at": "2026-01-01T00:00:00Z",
                "correlation": { "project": "team-b" }
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(made.status, StatusCode::OK, "{}", made.text());

    // The direct read is refused, which is the check that already existed.
    let direct = send(&app, get("/v1/api/datasets/ds-owned-by-b/items", Some("a-read"))).await;
    assert_eq!(direct.status, StatusCode::FORBIDDEN, "{}", direct.text());

    // The seizure attempt: same id, team-a's own project.
    let seize = send(
        &app,
        post(
            "/v1/api/datasets",
            Some("a-write"),
            json!({
                "dataset_id": "ds-owned-by-b", "name": "mine now", "item_count": 0,
                "created_at": "2026-01-01T00:00:00Z",
                "correlation": { "project": "team-a" }
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(
        seize.status,
        StatusCode::FORBIDDEN,
        "team-a took ownership of team-b's dataset id: {}",
        seize.text()
    );

    // And the read is still refused afterwards, which is the property that
    // actually matters. A 403 on the write that still moved the owner would be
    // no fix at all.
    let after = send(&app, get("/v1/api/datasets/ds-owned-by-b/items", Some("a-read"))).await;
    assert_eq!(after.status, StatusCode::FORBIDDEN, "{}", after.text());
}

/// An experiment names a dataset, so the same seizure works through it.
#[tokio::test]
async fn an_experiment_cannot_name_another_projects_dataset() {
    let app = app_with(tenanted_config()).await;
    let made = send(
        &app,
        post(
            "/v1/api/datasets",
            Some("b-write"),
            json!({
                "dataset_id": "ds-b-exp", "name": "b cases", "item_count": 0,
                "created_at": "2026-01-01T00:00:00Z",
                "correlation": { "project": "team-b" }
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(made.status, StatusCode::OK, "{}", made.text());

    let res = send(
        &app,
        post(
            "/v1/api/experiments",
            Some("a-write"),
            json!({
                "experiment_id": "e1", "name": "borrowed", "dataset_id": "ds-b-exp",
                "status": "running", "item_count": 0,
                "correlation": { "project": "team-a" }
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN, "{}", res.text());
}

// --------------------------------------------------------------- audit ---

/// The shipped reference promises that a deletion "is recorded in the audit
/// log". These prove there is one, that it names the key, and that it does not
/// contain it.
///
/// The rows are read from the store rather than over HTTP because
/// `GET /v1/api/audit` has no route in this build. Driving the request through
/// the real router and then reading what it wrote is the honest test either way:
/// it proves the handler recorded the access, not that a second handler can
/// read it back.
#[tokio::test]
async fn reading_a_payload_leaves_an_audit_row_naming_the_key_without_containing_it() {
    let (app, store) = app_and_store_with_data(fully_keyed()).await;

    let res = send(&app, get(&format!("/v1/api/traces/{TRACE_ID}"), Some(READ_KEY))).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let rows = audit_rows(&store).await;
    let row = rows
        .iter()
        .find(|r| r.action == "get_trace")
        .unwrap_or_else(|| panic!("the payload read left no audit row; got {rows:?}"));

    assert_eq!(row.result, mcpd_core::AuditResult::Allowed);
    assert_eq!(row.project, "default");
    assert_eq!(row.resource.as_deref(), Some(TRACE_ID));
    assert_eq!(row.subject, "", "subject ships empty until a proxy asserts one");

    // Names the key.
    assert!(!row.principal.is_empty(), "the row does not say which key acted");
    assert_eq!(
        row.principal,
        mcpd_server::auth::key_digest(READ_KEY),
        "the row names a different key than the one that read"
    );
    assert_ne!(
        row.principal,
        mcpd_server::auth::key_digest(ADMIN_KEY),
        "two keys must not share a principal"
    );

    // Does not contain it. Checked over the whole serialised row rather than the
    // principal alone, because a key that leaked into `resource` or `action`
    // would be as exposed, and this is the assertion that would catch it.
    let serialised = serde_json::to_string(row).expect("serialise the audit row");
    assert!(!serialised.contains(READ_KEY), "the key itself is in the audit row: {serialised}");
}

#[tokio::test]
async fn a_deletion_is_recorded_and_so_is_a_refused_one() {
    let (app, store) = app_and_store_with_data(fully_keyed()).await;
    let path = format!("/v1/api/traces/{TRACE_ID}");

    let refused = send(&app, delete(&path, Some(READ_KEY))).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);

    let deleted = send(&app, delete(&path, Some(ADMIN_KEY))).await;
    assert_eq!(deleted.status, StatusCode::OK, "{}", deleted.text());

    let rows = audit_rows(&store).await;
    let deletes: Vec<_> = rows.iter().filter(|r| r.action == "delete_trace").collect();
    assert_eq!(deletes.len(), 2, "both attempts should be recorded; got {deletes:?}");

    let refused_row = deletes
        .iter()
        .find(|r| r.result == mcpd_core::AuditResult::Refused)
        .expect("the refused delete left no row, which is the row that matters most");
    assert_eq!(refused_row.principal, mcpd_server::auth::key_digest(READ_KEY));

    let allowed_row = deletes
        .iter()
        .find(|r| r.result == mcpd_core::AuditResult::Allowed)
        .expect("the delete that happened left no row");
    assert_eq!(allowed_row.principal, mcpd_server::auth::key_digest(ADMIN_KEY));
    assert_eq!(allowed_row.resource.as_deref(), Some(TRACE_ID));
}

/// On an open daemon there is no key to name, and a blank principal would read
/// as a bug. `anonymous` is a fact worth recording.
#[tokio::test]
async fn an_unprotected_daemon_records_the_read_as_anonymous() {
    let (app, store) = app_and_store_with_data(ServerConfig::default()).await;

    let res = send(&app, get(&format!("/v1/api/traces/{TRACE_ID}"), None)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let rows = audit_rows(&store).await;
    let row = rows.iter().find(|r| r.action == "get_trace").expect("no audit row for the read");
    assert_eq!(row.principal, "anonymous");
}

/// Listing runs is not audited: a trace row carries no prompt or completion, and
/// a row per list request would bury the reads that do return a payload.
#[tokio::test]
async fn listing_runs_is_not_audited_but_searching_spans_is() {
    let (app, store) = app_and_store_with_data(fully_keyed()).await;

    assert_eq!(send(&app, get("/v1/api/traces", Some(READ_KEY))).await.status, StatusCode::OK);
    assert_eq!(send(&app, get("/v1/api/spans", Some(READ_KEY))).await.status, StatusCode::OK);

    let actions: Vec<String> = audit_rows(&store).await.iter().map(|r| r.action.clone()).collect();
    let has = |a: &str| actions.iter().any(|x| x == a);
    assert!(has("list_spans"), "a span search returns payloads: {actions:?}");
    assert!(!has("list_traces"), "the run list carries no payload: {actions:?}");
}

/// `GET /v1/api/audit` takes the admin scope.
///
/// This test needs the route registered in `crates/mcpd-server/src/lib.rs`:
///
/// ```text
/// .route("/v1/api/audit", get(query::list_audit))
/// ```
#[tokio::test]
async fn the_audit_endpoint_takes_the_admin_scope() {
    let (app, _store) = app_and_store_with_data(fully_keyed()).await;

    // Leave a row to read back.
    send(&app, get(&format!("/v1/api/traces/{TRACE_ID}"), Some(READ_KEY))).await;

    assert_eq!(
        send(&app, get("/v1/api/audit", None)).await.status,
        StatusCode::UNAUTHORIZED,
        "the audit log was readable with no key"
    );
    assert_eq!(
        send(&app, get("/v1/api/audit", Some(READ_KEY))).await.status,
        StatusCode::FORBIDDEN,
        "a read key read the audit log"
    );

    let res = send(&app, get("/v1/api/audit", Some(ADMIN_KEY))).await;
    assert_eq!(res.status, StatusCode::OK, "the admin key was refused: {}", res.text());
    let rows = res.json();
    let rows = rows.as_array().expect("the audit log is a list");
    assert!(!rows.is_empty(), "the recorded read is not in the log");
    assert!(
        !res.text().contains(READ_KEY),
        "the audit log handed back the key itself: {}",
        res.text()
    );
}

// ------------------------------------------------------- ingest ceilings ---

/// A daemon listening on a real socket, and the address to reach it at.
///
/// Everything above drives the router in process, which is enough to prove a
/// route exists and who may call it. A ceiling is a claim about what a running
/// daemon does with what a sender sends it, so these are driven over a
/// connection, against a served process, with an HTTP client.
///
/// The port is whatever the OS assigns. A fixed one collides with whatever else
/// is listening on this machine and turns someone else's server into a
/// convincing failure of this one.
async fn serve(config: ServerConfig) -> String {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let app = router(Arc::new(AppState::new(Arc::clone(&store) as Arc<dyn Store>, config)));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind a listener on a port the OS picked");
    let addr = listener.local_addr().expect("read back the bound address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

/// A trace and span id for one signal, derived from the project as well as the
/// number.
///
/// Not the number alone: two projects sharing a trace id can make a test pass
/// while the code under test is confusing them, which is how a missing project
/// filter in the trim once went unnoticed.
fn ids_in(project: &str, n: u32) -> (String, String) {
    let tag = project.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b)));
    (format!("{tag:016x}{n:016x}"), format!("{tag:08x}{n:08x}"))
}

/// `n` spans in one project, each its own trace, so one span is one signal.
fn spans_in(project: &str, n: u32) -> Vec<Value> {
    (0..n)
        .map(|i| {
            let (trace, span) = ids_in(project, i);
            span_in(project, &trace, &span)
        })
        .collect()
}

/// Send a batch to the native ingest door of a served daemon.
async fn ingest_over_http(base: &str, project: &str, n: u32) -> Value {
    let res = reqwest::Client::new()
        .post(format!("{base}/v1/ingest"))
        .json(&json!({ "spans": spans_in(project, n) }))
        .send()
        .await
        .expect("the daemon answered the ingest request");
    let status = res.status();
    let body = res.text().await.expect("the response carries a body");
    assert_eq!(status, reqwest::StatusCode::OK, "the ingest was refused: {body}");
    serde_json::from_str(&body).expect("the daemon answered with an ingest result")
}

/// One counter's value from a scrape of a served daemon.
///
/// Read by name from the exposition text rather than matched as a substring:
/// the `# HELP` line also carries the name, so `contains` would pass on the
/// help text alone and prove nothing about the number.
async fn counter(base: &str, name: &str) -> u64 {
    let text = reqwest::get(format!("{base}/metrics"))
        .await
        .expect("the daemon answered the scrape")
        .text()
        .await
        .expect("the scrape has a body");
    text.lines()
        .find_map(|line| line.strip_prefix(&format!("{name} ")))
        .unwrap_or_else(|| panic!("{name} is not in the scrape:\n{text}"))
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("{name} is not a number ({e}) in:\n{text}"))
}

/// Ceilings written under `projects`, and no top-level number.
fn ceilings_per_project(projects: &[(&str, u32)]) -> ServerConfig {
    ServerConfig {
        projects: projects
            .iter()
            .map(|(name, limit)| {
                (
                    (*name).to_owned(),
                    mcpd_server::state::ProjectConfig {
                        quota_per_minute: Some(*limit),
                        ..Default::default()
                    },
                )
            })
            .collect(),
        ..ServerConfig::default()
    }
}

#[tokio::test]
async fn a_name_the_projects_block_does_not_list_is_stored_unbounded_and_counted() {
    // The behaviour this configuration has, over a connection to a daemon that
    // is running. Ceilings live under `projects` and there is no top-level
    // number, which is the natural way to write per-project limits, so a sender
    // that names anything else is charged to nothing.
    //
    // It is stored rather than refused on purpose: a deployment that upgrades
    // and changes no configuration must not start losing signals it is keeping
    // today. What the daemon does instead is count them, so the operator can
    // see the bypass from the same scrape they already read.
    let base = serve(ceilings_per_project(&[("alpha", 10)])).await;

    let result = ingest_over_http(&base, "invented-by-the-sender", 40).await;
    assert_eq!(result["accepted"], 40, "not one signal is dropped: {result}");
    assert_eq!(result["rejected"], 0, "{result}");
    assert!(result["errors"].is_null(), "nothing was refused, so nothing is reported: {result}");
    assert_eq!(counter(&base, "mcpd_signals_unbounded_total").await, 40, "the bypass is counted");
    assert_eq!(counter(&base, "mcpd_signals_over_quota_total").await, 0);

    // And the ceiling that was written still holds for the project it names.
    let held = ingest_over_http(&base, "alpha", 40).await;
    assert_eq!(held["accepted"], 10, "alpha keeps the ceiling its configuration gave it: {held}");
    assert_eq!(held["rejected"], 30, "{held}");
    assert_eq!(counter(&base, "mcpd_signals_over_quota_total").await, 30);
    assert_eq!(
        counter(&base, "mcpd_signals_unbounded_total").await,
        40,
        "a project with a ceiling of its own is not counted as unbounded"
    );
}

#[tokio::test]
async fn a_daemon_with_no_ceilings_anywhere_ingests_exactly_as_it_did_before() {
    // What every deployment that never opened the quota settings is running.
    // Nothing is limited, nothing is counted as bypassed, and an operator who
    // configured no ceiling is told nothing about one being walked past.
    let base = serve(ServerConfig::default()).await;

    let result = ingest_over_http(&base, "anything-at-all", 200).await;
    assert_eq!(result["accepted"], 200, "{result}");
    assert_eq!(result["rejected"], 0, "{result}");
    assert_eq!(counter(&base, "mcpd_signals_unbounded_total").await, 0);
    assert_eq!(counter(&base, "mcpd_signals_over_quota_total").await, 0);
}

#[tokio::test]
async fn the_top_level_ceiling_still_bounds_a_name_no_configuration_lists() {
    // The deployment that set the flag, behaving as it does today: an invented
    // name shares one counter with every other invented name, so there is
    // nothing left unbounded to count.
    let base = serve(ServerConfig { quota_per_minute: Some(10), ..ServerConfig::default() }).await;

    let result = ingest_over_http(&base, "invented-by-the-sender", 40).await;
    assert_eq!(result["accepted"], 10, "the shared ceiling, and no more: {result}");
    assert_eq!(result["rejected"], 30, "{result}");
    let message = result["errors"][0]["message"].as_str().expect("a refusal carries a reason");
    assert!(message.contains("no configuration lists"), "{message}");
    assert_eq!(counter(&base, "mcpd_signals_over_quota_total").await, 30);
    assert_eq!(
        counter(&base, "mcpd_signals_unbounded_total").await,
        0,
        "with the flag set every name has a counter, so nothing rides in unbounded"
    );
}

// -------------------------------------------------------- OTLP idempotency ---

/// One OTLP log export, with a record per `(nanos, message)` given.
///
/// The records sit in one scope, which is the shape a collector sends and the
/// shape that makes two records with the same content and the same timestamp
/// possible in a single export.
fn otlp_log_export(records: &[(u64, &str)]) -> String {
    let body: Vec<Value> = records
        .iter()
        .map(|(nanos, message)| {
            json!({
                "timeUnixNano": nanos.to_string(),
                "severityNumber": 9,
                "body": { "stringValue": message },
                "attributes": [{ "key": "attempt", "value": { "stringValue": "1" } }]
            })
        })
        .collect();
    json!({
        "resourceLogs": [{
            "resource": { "attributes": [
                { "key": "service.name", "value": { "stringValue": "checkout" } }
            ]},
            "scopeLogs": [{ "scope": { "name": "orders" }, "logRecords": body }]
        }]
    })
    .to_string()
}

/// One OTLP gauge export, with a data point per `(nanos, value)` given.
fn otlp_metric_export(points: &[(u64, f64)]) -> String {
    let data: Vec<Value> = points
        .iter()
        .map(|(nanos, value)| {
            json!({
                "timeUnixNano": nanos.to_string(),
                "asDouble": value,
                "attributes": [{ "key": "queue", "value": { "stringValue": "orders" } }]
            })
        })
        .collect();
    json!({
        "resourceMetrics": [{
            "scopeMetrics": [{
                "metrics": [{
                    "name": "queue.depth",
                    "unit": "1",
                    "gauge": { "dataPoints": data }
                }]
            }]
        }]
    })
    .to_string()
}

async fn stored_logs(store: &SqliteStore) -> Vec<mcpd_core::LogRecord> {
    store
        .list_logs(&mcpd_store::LogQuery {
            project: "default".to_owned(),
            limit: 100,
            ..Default::default()
        })
        .await
        .expect("read the logs back")
        .items
}

/// Every stored point for `queue.depth`, summed into one bucket.
///
/// The sum is the assertion that matters. A duplicated point is not only an
/// extra row, it is an extra observation in every total built on it, which is
/// the harm a retried export used to do silently.
async fn queue_depth_total(store: &SqliteStore) -> (f64, u64) {
    let r = store
        .query_metrics(&mcpd_store::MetricQuery {
            project: "default".to_owned(),
            name: "queue.depth".to_owned(),
            from: time::OffsetDateTime::UNIX_EPOCH,
            to: time::OffsetDateTime::now_utc() + time::Duration::days(3650),
            bucket_seconds: 86_400,
            agg: mcpd_store::Aggregation::Sum,
            group_by: Vec::new(),
            filter: Vec::new(),
        })
        .await
        .expect("read the metrics back");
    let mut value = 0.0;
    let mut count = 0;
    for s in &r.series {
        for p in &s.points {
            value += p.value;
            count += p.count.unwrap_or(0);
        }
    }
    (value, count)
}

#[tokio::test]
async fn a_resent_otlp_log_export_stores_one_copy_of_each_line() {
    let (app, store) = app_and_store(ServerConfig::default()).await;
    // What an exporter does when mcpd answers slowly: it sends the same request
    // again. Every other door in mcpd survives that. These two used to mint a
    // fresh id per decode, so the retry landed as a second set of rows.
    let export = otlp_log_export(&[
        (1_767_225_600_000_000_000, "order placed"),
        (1_767_225_601_000_000_000, "payment captured"),
    ]);

    for _ in 0..3 {
        let res = send(&app, post("/v1/logs", None, export.clone())).await;
        assert_eq!(res.status, StatusCode::OK, "{}", res.text());
        assert_eq!(
            res.json()["partialSuccess"]["rejectedLogRecords"],
            json!(0),
            "a retry is accepted, not refused: {}",
            res.text()
        );
    }

    let logs = stored_logs(&store).await;
    assert_eq!(logs.len(), 2, "three identical exports are one set of lines");
    let mut messages: Vec<&str> = logs.iter().map(|l| l.message.as_str()).collect();
    messages.sort_unstable();
    assert_eq!(messages, vec!["order placed", "payment captured"]);
}

#[tokio::test]
async fn a_resent_otlp_metric_export_stores_one_copy_of_each_point() {
    let (app, store) = app_and_store(ServerConfig::default()).await;
    let export =
        otlp_metric_export(&[(1_767_225_600_000_000_000, 4.0), (1_767_225_601_000_000_000, 7.0)]);

    for _ in 0..3 {
        let res = send(&app, post("/v1/metrics", None, export.clone())).await;
        assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    }

    let (total, observations) = queue_depth_total(&store).await;
    assert_eq!(observations, 2, "three identical exports are two observations");
    assert_eq!(total, 11.0, "a retried export must not inflate the total");
}

#[tokio::test]
async fn log_lines_that_repeat_within_one_export_are_all_kept() {
    let (app, store) = app_and_store(ServerConfig::default()).await;
    // The case a digest over content alone gets wrong. Two of these are the
    // same text at the same nanosecond and are two events; the third is the
    // same text a millisecond later and is a third. All three are real, and a
    // store that kept one of them would be losing data to fix a duplicate.
    let export = otlp_log_export(&[
        (1_767_225_600_000_000_000, "retrying"),
        (1_767_225_600_000_000_000, "retrying"),
        (1_767_225_600_001_000_000, "retrying"),
    ]);

    let res = send(&app, post("/v1/logs", None, export)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let logs = stored_logs(&store).await;
    assert_eq!(logs.len(), 3, "three real events, however alike they look");
}

#[tokio::test]
async fn metric_points_that_repeat_within_one_export_are_all_kept() {
    let (app, store) = app_and_store(ServerConfig::default()).await;
    let export = otlp_metric_export(&[
        (1_767_225_600_000_000_000, 5.0),
        (1_767_225_600_000_000_000, 5.0),
        (1_767_225_600_001_000_000, 5.0),
    ]);

    let res = send(&app, post("/v1/metrics", None, export)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let (total, observations) = queue_depth_total(&store).await;
    assert_eq!(observations, 3, "three real observations, however alike they look");
    assert_eq!(total, 15.0);
}

#[tokio::test]
async fn two_otlp_log_exports_that_differ_in_one_line_are_both_stored() {
    let (app, store) = app_and_store(ServerConfig::default()).await;
    // The other half of the guarantee. Deduplicating a retry is only safe if a
    // near-identical export that is genuinely new still lands whole, so the
    // batch key has to change when any record in the export does.
    let first = otlp_log_export(&[
        (1_767_225_600_000_000_000, "order placed"),
        (1_767_225_601_000_000_000, "payment captured"),
    ]);
    let second = otlp_log_export(&[
        (1_767_225_600_000_000_000, "order placed"),
        (1_767_225_601_000_000_000, "payment refused"),
    ]);

    for body in [first, second] {
        let res = send(&app, post("/v1/logs", None, body)).await;
        assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    }

    let logs = stored_logs(&store).await;
    assert_eq!(logs.len(), 3, "the shared line once, and both endings");
    assert!(logs.iter().any(|l| l.message == "payment captured"));
    assert!(logs.iter().any(|l| l.message == "payment refused"));
}

#[tokio::test]
async fn a_retried_otlp_export_is_counted_as_a_replay_and_not_as_new_data() {
    // The row ids alone already stop a retry from storing twice. This is the
    // other half of what the native door has and these two lacked: the export
    // carries a key derived from what it holds, so the store recognises the
    // replay before it writes anything, and an operator reading the scrape sees
    // a replay counted as a replay rather than as data that arrived twice.
    let base = serve(ServerConfig::default()).await;
    let export = otlp_log_export(&[
        (1_767_225_600_000_000_000, "order placed"),
        (1_767_225_601_000_000_000, "payment captured"),
    ]);
    let client = reqwest::Client::new();
    for _ in 0..2 {
        let res = client
            .post(format!("{base}/v1/logs"))
            .header("content-type", "application/json")
            .body(export.clone())
            .send()
            .await
            .expect("the daemon answered the export");
        assert_eq!(res.status(), reqwest::StatusCode::OK);
    }

    assert_eq!(
        counter(&base, "mcpd_signals_accepted_total").await,
        2,
        "two lines arrived, whatever the sender did afterwards"
    );
    // Two, not one. `mcpd_batches_duplicate_total` is fed from `IngestResult
    // .duplicate`, which is the number of signals the replayed batch carried,
    // so the counter is per signal and the name is per batch. That is a
    // separate defect in `state.rs`, and this asserts what the daemon actually
    // reports rather than what its name promises.
    assert_eq!(
        counter(&base, "mcpd_batches_duplicate_total").await,
        2,
        "the replay was recognised before anything was written"
    );
}

#[tokio::test]
async fn a_resent_otlp_trace_export_stores_one_copy_of_each_span() {
    let (app, store) = app_and_store(ServerConfig::default()).await;
    // The traces door needs no key of its own and no derived id, because OTLP
    // gives a span an id and all three stores upsert on it. This is the check
    // that the claim is true rather than assumed, and the reason the fix for
    // logs and metrics stopped where it did.
    let export = json!({
        "resourceSpans": [{
            "scopeSpans": [{
                "spans": [{
                    "traceId": "4bf92f3577b34da6a3ce929d0e0e4736",
                    "spanId": "00f067aa0ba902b7",
                    "name": "checkout",
                    "startTimeUnixNano": "1767225600000000000",
                    "endTimeUnixNano": "1767225601000000000"
                }]
            }]
        }]
    })
    .to_string();

    for _ in 0..3 {
        let res = send(&app, post("/v1/traces", None, export.clone())).await;
        assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    }

    let d = store
        .get_trace("default", "4bf92f3577b34da6a3ce929d0e0e4736", &mcpd_store::Include::default())
        .await
        .expect("read the run back");
    assert_eq!(d.spans.len(), 1, "the span id is the key, so a resend overwrites");
    assert_eq!(d.trace.span_count, 1);
}

#[tokio::test]
async fn a_resent_otlp_log_export_whose_records_carry_no_time_stores_one_copy() {
    let (app, store) = app_and_store(ServerConfig::default()).await;
    // `timeUnixNano` is optional in OTLP, and a log bridge that only knows when
    // it observed a line leaves it out. The arrival time then stands in for it,
    // and standing in is fine for the stored row but not for the derived id: a
    // clock reading in the digest is a different id on every attempt, so the
    // retry this whole mechanism exists to absorb stored a second copy.
    let export = json!({
        "resourceLogs": [{
            "resource": { "attributes": [
                { "key": "service.name", "value": { "stringValue": "checkout" } }
            ]},
            "scopeLogs": [{ "scope": { "name": "orders" }, "logRecords": [
                { "observedTimeUnixNano": "1767225600000000000",
                  "severityNumber": 9,
                  "body": { "stringValue": "order placed" } }
            ]}]
        }]
    })
    .to_string();

    for _ in 0..3 {
        let res = send(&app, post("/v1/logs", None, export.clone())).await;
        assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    }

    let logs = stored_logs(&store).await;
    assert_eq!(logs.len(), 1, "three identical exports are one line, timed or not");
}

#[tokio::test]
async fn a_resent_otlp_metric_export_whose_points_carry_no_time_stores_one_copy() {
    let (app, store) = app_and_store(ServerConfig::default()).await;
    // The metric half of the same hole. A duplicated point is an extra
    // observation in every total built on it, so this one moves a number an
    // operator reads rather than only adding a row.
    let export = json!({
        "resourceMetrics": [{
            "scopeMetrics": [{
                "metrics": [{
                    "name": "queue.depth",
                    "unit": "1",
                    "gauge": { "dataPoints": [ { "asDouble": 4.0 } ] }
                }]
            }]
        }]
    })
    .to_string();

    for _ in 0..3 {
        let res = send(&app, post("/v1/metrics", None, export.clone())).await;
        assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    }

    let (total, observations) = queue_depth_total(&store).await;
    assert_eq!(observations, 1, "three identical exports are one observation");
    assert_eq!(total, 4.0, "a retried export must not inflate the total");
}

#[tokio::test]
async fn an_export_refused_for_the_wrong_project_can_still_be_sent_again() {
    // The export key is derived from the whole decoded export, and refusal and
    // the quota ceiling both take records back out of the batch afterwards. A
    // key recorded against what was left answers the resend with "already
    // stored" for a line that was never stored, and there is no later attempt
    // that can recover it.
    //
    // A key scoped to "other" reaches nothing in the default project, so this
    // export is refused whole. A correctly scoped key then sends the same
    // export, which is what happens when one exporter in a fleet is
    // misconfigured and another is not.
    let config = ServerConfig {
        ingest_keys: vec!["open-key".to_owned()],
        projects: [(
            "other".to_owned(),
            mcpd_server::state::ProjectConfig {
                ingest_keys: vec!["narrow-key".to_owned()],
                ..Default::default()
            },
        )]
        .into_iter()
        .collect(),
        ..ServerConfig::default()
    };
    let (app, store) = app_and_store(config).await;
    let export = otlp_log_export(&[(1_767_225_600_000_000_000, "order placed")]);

    let refused = send(&app, post("/v1/logs", Some("narrow-key"), export.clone())).await;
    assert_eq!(refused.status, StatusCode::OK, "{}", refused.text());
    assert_eq!(
        refused.json()["partialSuccess"]["rejectedLogRecords"],
        json!(1),
        "the narrow key reaches no project this export names: {}",
        refused.text()
    );
    assert!(stored_logs(&store).await.is_empty(), "nothing was stored");

    let allowed = send(&app, post("/v1/logs", Some("open-key"), export)).await;
    assert_eq!(allowed.status, StatusCode::OK, "{}", allowed.text());
    assert_eq!(stored_logs(&store).await.len(), 1, "the authorised sender's line landed");
}
