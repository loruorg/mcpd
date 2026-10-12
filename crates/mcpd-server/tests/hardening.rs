//! The layers around every route, driven through the real router.
//!
//! Each test here guards a defect a hostile review found in the daemon as it
//! was: a body limit that was not the configured one, a gzip body with no
//! ceiling on what it expanded to, a query API every origin could read, no
//! deadline on any request, an error counter that never moved, and a redaction
//! rule an operator had no way to add. The router is the thing under test, so
//! every request goes through `mcpd_server::router` and every refusal is read
//! back as the JSON shape `error.rs` promises.

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mcpd_core::redact::{Redactor, Rule};
use mcpd_core::RedactionMode;
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::{ClickHouseConfig, ClickHouseStore, Store};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

const TRACE_ID: &str = "0123456789abcdef0123456789abcdef";
const SPAN_ID: &str = "0123456789abcdef";

// ------------------------------------------------------------- fixtures ---

/// A private database file per test, named from the pid and a per-process
/// stamp. See `tests/http.rs` for why the pid alone is not enough.
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
        std::env::temp_dir().join(format!("mcpd-hardening-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

async fn sqlite() -> Arc<dyn Store> {
    let store = SqliteStore::open(temp_db(), 2).expect("open the store");
    store.migrate().await.expect("migrate the store");
    Arc::new(store)
}

/// The router over an embedded store, and the state behind it.
async fn app_and_state(config: ServerConfig) -> (Router, Arc<AppState>) {
    let state = Arc::new(AppState::new(sqlite().await, config));
    (router(Arc::clone(&state)), state)
}

async fn app_with(config: ServerConfig) -> Router {
    app_and_state(config).await.0
}

/// A router whose storage is a ClickHouse store pointed at `url`, which is
/// how a request is made to fail or hang without a fake store: nothing behind
/// the URL is the real backend, so every query is a real transport failure.
fn app_on_clickhouse(config: ServerConfig, url: &str) -> (Router, Arc<AppState>) {
    let store =
        ClickHouseStore::new(ClickHouseConfig { url: url.to_owned(), ..Default::default() })
            .expect("build the clickhouse store");
    let state = Arc::new(AppState::new(Arc::new(store), config));
    (router(Arc::clone(&state)), state)
}

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

    fn header(&self, name: &str) -> Option<String> {
        self.headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_owned)
    }
}

async fn send(app: &Router, req: Request<Body>) -> Res {
    let res = app.clone().oneshot(req).await.expect("the router returned a response");
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.expect("read the response body").to_bytes().to_vec();
    Res { status, headers, body }
}

fn get(uri: &str) -> Request<Body> {
    Request::builder().method("GET").uri(uri).body(Body::empty()).expect("build the request")
}

fn post(uri: &str, body: impl Into<Vec<u8>>) -> Request<Body> {
    let body: Vec<u8> = body.into();
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        // A real client always sends this, and the body limit reads it.
        .header("content-length", body.len().to_string())
        .body(Body::from(body))
        .expect("build the request")
}

/// One counter's value from a scrape through the router.
async fn counter(app: &Router, name: &str) -> u64 {
    let text = send(app, get("/metrics")).await.text();
    text.lines()
        .find_map(|line| line.strip_prefix(&format!("{name} ")))
        .unwrap_or_else(|| panic!("{name} is not in the scrape:\n{text}"))
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("{name} is not a number ({e}) in:\n{text}"))
}

/// A port nothing listens on: bound by the OS, then released.
fn closed_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a port the OS picked");
    l.local_addr().expect("read the bound address").port()
}

// ------------------------------------------------------------ body limit ---

/// The defect: axum applies a 2 MiB limit of its own inside every body
/// extractor, and the configured 32 MiB never reached it. A 3 MiB body was
/// refused with an empty 413 from a limit no flag names.
#[tokio::test]
async fn a_three_mebibyte_body_is_inside_the_default_limit_and_is_parsed() {
    let app = app_with(ServerConfig::default()).await;
    let body = vec![b'x'; 3 * 1024 * 1024];
    let res = send(&app, post("/v1/ingest", body)).await;
    assert_ne!(
        res.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "a body inside the documented limit was refused as too large: {}",
        res.text()
    );
    // It reached the handler, which read all of it and found it was not a
    // batch.
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "{}", res.text());
    let (code, message) = res.error();
    assert_eq!(code, "bad_request");
    assert!(
        message.contains("ingest batch"),
        "the handler should have tried to parse it, said: {message}"
    );
}

#[tokio::test]
async fn a_body_over_the_limit_is_refused_in_the_json_shape_naming_the_limit() {
    let small = ServerConfig { max_body_bytes: 1024, ..ServerConfig::default() };
    let app = app_with(small).await;

    let res = send(&app, post("/v1/ingest", vec![b' '; 4096])).await;
    assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE, "{}", res.text());
    let (code, message) = res.error();
    assert_eq!(code, "payload_too_large");
    assert!(message.contains("4096 bytes"), "the message should name the size sent: {message}");
    assert!(message.contains("1024 bytes"), "the message should name the limit: {message}");
}

#[tokio::test]
async fn a_body_sent_without_a_length_is_still_held_to_the_limit() {
    // A chunked upload declares no length, so the limit is met while reading
    // rather than before. The refusal has to come out in the same shape.
    let small = ServerConfig { max_body_bytes: 1024, ..ServerConfig::default() };
    let app = app_with(small).await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/ingest")
        .header("content-type", "application/json")
        .body(Body::from(vec![b' '; 4096]))
        .expect("build the request");
    let res = send(&app, req).await;
    assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE, "{}", res.text());
    let (code, message) = res.error();
    assert_eq!(code, "payload_too_large");
    assert!(message.contains("1024 bytes"), "the message should name the limit: {message}");
}

#[tokio::test]
async fn the_limit_applies_to_the_query_api_as_well_as_ingest() {
    // One limit, and it is `max_body_bytes` on every route that reads a body.
    let small = ServerConfig { max_body_bytes: 1024, ..ServerConfig::default() };
    let app = app_with(small).await;
    let res = send(&app, post("/v1/api/scores", vec![b' '; 4096])).await;
    assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE, "{}", res.text());
    assert_eq!(res.error().0, "payload_too_large");
}

/// A gzip body of a few MiB of zeros compresses a thousand to one. The limit
/// on the wire is not a limit on what it becomes in memory.
#[tokio::test]
async fn a_gzip_bomb_is_refused_at_the_configured_limit_not_inflated_whole() {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    let limit = 64 * 1024;
    let app = app_with(ServerConfig { max_body_bytes: limit, ..ServerConfig::default() }).await;

    // Four MiB decompressed, a few KiB on the wire, so the wire size passes
    // the body limit and only the decompression cap can stop it.
    let mut enc = GzEncoder::new(Vec::new(), Compression::best());
    for _ in 0..64 {
        enc.write_all(&[0u8; 64 * 1024]).expect("compress a block");
    }
    let bomb = enc.finish().expect("finish the gzip stream");
    assert!(bomb.len() < limit, "the bomb is {} bytes on the wire", bomb.len());

    let req = Request::builder()
        .method("POST")
        .uri("/v1/ingest")
        .header("content-type", "application/json")
        .header("content-encoding", "gzip")
        .header("content-length", bomb.len().to_string())
        .body(Body::from(bomb))
        .expect("build the request");
    let res = send(&app, req).await;
    assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE, "{}", res.text());
    let (code, message) = res.error();
    assert_eq!(code, "payload_too_large");
    assert!(
        message.contains(&format!("{limit} bytes")),
        "the message should name the limit: {message}"
    );
    assert!(message.contains("gzip"), "the message should say it was the gzip body: {message}");
}

#[tokio::test]
async fn a_gzip_body_inside_the_limit_is_still_accepted() {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    let app = app_with(ServerConfig::default()).await;
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(fixture_batch().to_string().as_bytes()).expect("compress the batch");
    let body = enc.finish().expect("finish the gzip stream");
    let req = Request::builder()
        .method("POST")
        .uri("/v1/ingest")
        .header("content-type", "application/json")
        .header("content-encoding", "gzip")
        .header("content-length", body.len().to_string())
        .body(Body::from(body))
        .expect("build the request");
    let res = send(&app, req).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["accepted"], json!(1));
}

// ------------------------------------------------------------------ CORS ---

fn cross_origin(uri: &str, origin: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("origin", origin)
        .body(Body::empty())
        .expect("build the request")
}

fn preflight(uri: &str, origin: &str) -> Request<Body> {
    Request::builder()
        .method("OPTIONS")
        .uri(uri)
        .header("origin", origin)
        .header("access-control-request-method", "GET")
        .header("access-control-request-headers", "authorization")
        .body(Body::empty())
        .expect("build the request")
}

fn allowing(origins: &[&str]) -> ServerConfig {
    ServerConfig {
        cors_origins: origins.iter().map(|o| (*o).to_owned()).collect(),
        ..ServerConfig::default()
    }
}

/// The bundled interface is served from the daemon's own origin, so its
/// requests carry no `Origin` header at all. That path has to keep working
/// with nothing configured, or the default deployment ships a dead interface.
#[tokio::test]
async fn with_no_origins_configured_the_same_origin_interface_still_reads_the_api() {
    let app = app_with(ServerConfig::default()).await;
    let res = send(&app, get("/v1/api/traces")).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.header("access-control-allow-origin"), None);
}

/// The defect: every origin was allowed on every data route, so any page open
/// in the same browser as a daemon with an open read API could read the
/// prompts it stores.
#[tokio::test]
async fn with_no_origins_configured_a_page_on_another_origin_gets_no_allow_header() {
    let app = app_with(ServerConfig::default()).await;
    let res = send(&app, cross_origin("/v1/api/traces", "https://evil.example")).await;
    // The daemon answers; the browser is what refuses to hand the page the
    // body, and it does that on the missing header.
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(
        res.header("access-control-allow-origin"),
        None,
        "a data route allowed a browser page on another origin to read it"
    );

    let pre = send(&app, preflight("/v1/api/traces", "https://evil.example")).await;
    assert_eq!(
        pre.header("access-control-allow-origin"),
        None,
        "a preflight from an unlisted origin was granted"
    );
}

#[tokio::test]
async fn a_configured_origin_is_allowed_and_only_that_one() {
    let app = app_with(allowing(&["https://console.example"])).await;

    let listed = send(&app, cross_origin("/v1/api/traces", "https://console.example")).await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.text());
    assert_eq!(
        listed.header("access-control-allow-origin").as_deref(),
        Some("https://console.example"),
        "the configured origin was not allowed"
    );

    let other = send(&app, cross_origin("/v1/api/traces", "https://other.example")).await;
    assert_eq!(
        other.header("access-control-allow-origin"),
        None,
        "an origin that was not configured was allowed"
    );

    // And the interface on the daemon's own origin is unaffected.
    let same = send(&app, get("/v1/api/traces")).await;
    assert_eq!(same.status, StatusCode::OK, "{}", same.text());
}

#[tokio::test]
async fn a_preflight_from_a_configured_origin_permits_the_authorization_header() {
    // A browser sends the key in a header, so the preflight has to say that
    // header and the methods the interface uses are permitted, or no
    // authenticated read ever leaves the page.
    let app = app_with(allowing(&["https://console.example"])).await;
    let pre = send(&app, preflight("/v1/api/traces", "https://console.example")).await;
    assert!(pre.status.is_success(), "the preflight was refused: {}", pre.text());
    assert_eq!(
        pre.header("access-control-allow-origin").as_deref(),
        Some("https://console.example")
    );
    let methods = pre.header("access-control-allow-methods").unwrap_or_default();
    for m in ["GET", "POST", "DELETE"] {
        assert!(methods.contains(m), "the preflight did not permit {m}: {methods}");
    }
    let headers = pre.header("access-control-allow-headers").unwrap_or_default().to_lowercase();
    assert!(headers.contains("authorization"), "the key header is not permitted: {headers}");
}

#[tokio::test]
async fn a_star_restores_every_origin_and_has_to_be_typed() {
    let app = app_with(allowing(&["*"])).await;
    let res = send(&app, cross_origin("/v1/api/traces", "https://anything.example")).await;
    assert_eq!(res.header("access-control-allow-origin").as_deref(), Some("*"));
}

#[tokio::test]
async fn the_probes_stay_readable_from_any_origin_whatever_is_configured() {
    // The interface probes /healthz before it does anything else, and that
    // probe must succeed from wherever the interface is served.
    for config in [ServerConfig::default(), allowing(&["https://console.example"])] {
        let app = app_with(config).await;
        for route in ["/healthz", "/readyz", "/metrics"] {
            let res = send(&app, cross_origin(route, "https://elsewhere.example")).await;
            assert_eq!(
                res.header("access-control-allow-origin").as_deref(),
                Some("*"),
                "{route} is not readable from another origin"
            );
        }
    }
}

// --------------------------------------------------------------- timeout ---

/// A storage backend that accepts a connection and never answers held the
/// request for as long as the client waited. The deadline is what turns that
/// into an answer, and the answer names the flag.
#[tokio::test]
async fn a_request_that_runs_past_the_deadline_is_answered_with_the_flag_to_raise() {
    // A listener that never accepts: the handshake completes into the backlog
    // and the store's query waits on a reply that is never written.
    let sink = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind a sink");
    let url = format!("http://{}", sink.local_addr().expect("read the bound address"));
    let config = ServerConfig { request_timeout_seconds: 1, ..ServerConfig::default() };
    let (app, _state) = app_on_clickhouse(config, &url);

    let started = std::time::Instant::now();
    let res = send(&app, get("/v1/api/traces")).await;
    let took = started.elapsed();
    assert_eq!(res.status, StatusCode::SERVICE_UNAVAILABLE, "{}", res.text());
    let (code, message) = res.error();
    assert_eq!(code, "timeout");
    assert!(message.contains("1s"), "the message should name the deadline: {message}");
    assert!(
        message.contains("--request-timeout-seconds"),
        "the message should name the flag that raises it: {message}"
    );
    assert!(took < std::time::Duration::from_secs(10), "the deadline did not fire: {took:?}");

    // A timed-out request is a request the server failed to answer.
    assert_eq!(counter(&app, "mcpd_request_errors_total").await, 1);
    drop(sink);
}

// ---------------------------------------------------------- error counter ---

/// The defect: `mcpd_request_errors_total` was rendered and never incremented,
/// so an operator alerting on it was alerting on a number that could not move.
#[tokio::test]
async fn a_server_error_moves_the_error_counter() {
    let url = format!("http://127.0.0.1:{}", closed_port());
    let (app, _state) = app_on_clickhouse(ServerConfig::default(), &url);
    assert_eq!(counter(&app, "mcpd_request_errors_total").await, 0);

    let res = send(&app, get("/v1/api/traces")).await;
    assert_eq!(res.status, StatusCode::INTERNAL_SERVER_ERROR, "{}", res.text());
    assert_eq!(res.error().0, "storage_error");
    assert_eq!(counter(&app, "mcpd_request_errors_total").await, 1);

    // A refusal that is the caller's fault is not a server error.
    let bad = send(&app, get("/v1/api/traces?limit=not-a-number")).await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST, "{}", bad.text());
    assert_eq!(counter(&app, "mcpd_request_errors_total").await, 1);
}

#[tokio::test]
async fn the_forward_drop_counter_is_rendered() {
    let app = app_with(ServerConfig::default()).await;
    assert_eq!(counter(&app, "mcpd_forward_dropped_spans_total").await, 0);
}

// ------------------------------------------------------------- redaction ---

/// The defect: `Redactor::with_rule` existed and nothing in the daemon called
/// it, so an operator whose tools put an account number under a key the
/// built-in rules do not know had no way to mask it. This is the rule the new
/// flag builds, applied through the real pipeline.
#[tokio::test]
async fn a_span_carrying_a_key_the_operator_named_is_stored_masked() {
    let redactor = Redactor::with_defaults()
        .with_rule(Rule { key_contains: vec!["ticket".to_owned()], mode: RedactionMode::Mask });
    let state =
        Arc::new(AppState::new(sqlite().await, ServerConfig::default()).with_redactor(redactor));
    let app = router(Arc::clone(&state));

    let mut batch = fixture_batch();
    batch["spans"][0]["attributes"] = json!({ "ticket_id": "T-42", "note": "kept" });
    batch["spans"][0]["input"] = json!({ "supportTicket": "T-9", "q": "where is my order" });
    let res = send(&app, post("/v1/ingest", batch.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let detail = send(&app, get(&format!("/v1/api/traces/{TRACE_ID}"))).await;
    assert_eq!(detail.status, StatusCode::OK, "{}", detail.text());
    let span = &detail.json()["spans"][0];
    assert_eq!(span["attributes"]["ticket_id"], json!("[redacted]"), "{span}");
    assert_eq!(span["attributes"]["note"], json!("kept"), "{span}");
    // Matched on a lowercase substring, so a camel-cased key is caught too.
    assert_eq!(span["input"]["supportTicket"], json!("[redacted]"), "{span}");
    assert_eq!(span["input"]["q"], json!("where is my order"), "{span}");
}

#[tokio::test]
async fn a_string_cap_set_by_the_operator_cuts_a_long_value() {
    let state = Arc::new(
        AppState::new(sqlite().await, ServerConfig::default())
            .with_redactor(Redactor::with_defaults().with_max_string(16)),
    );
    let app = router(Arc::clone(&state));

    let mut batch = fixture_batch();
    batch["spans"][0]["output"] = json!({ "text": "x".repeat(100) });
    let res = send(&app, post("/v1/ingest", batch.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let detail = send(&app, get(&format!("/v1/api/traces/{TRACE_ID}"))).await;
    let text = detail.json()["spans"][0]["output"]["text"].as_str().unwrap_or_default().to_owned();
    assert!(text.starts_with(&"x".repeat(16)), "{text}");
    assert!(text.contains("84 bytes truncated"), "{text}");
}

/// Readiness asks the run store as well. A replica whose run store cannot be
/// read answered every run and worker call with an error while `/readyz` said
/// it was ready, because only the signal store was asked.
#[tokio::test]
async fn readiness_fails_when_the_run_store_cannot_be_read() {
    let path = temp_db().with_extension("runs.db");
    // Opened and never migrated, so the read readiness makes finds no table.
    let runs = mcpd_runs::SqliteRunStore::open(&path, mcpd_runs::RunConfig::default())
        .expect("open the run store");
    let state = Arc::new(
        AppState::new(sqlite().await, ServerConfig::default()).with_run_store(Arc::new(runs)),
    );
    let app = router(Arc::clone(&state));
    let res = app
        .oneshot(Request::get("/readyz").body(Body::empty()).expect("build"))
        .await
        .expect("a response");
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = res.into_body().collect().await.expect("read").to_bytes();
    let body: Value = serde_json::from_slice(&body).expect("json");
    assert!(
        body["detail"].as_str().unwrap_or_default().contains("run store"),
        "the reason must name the run store: {body}"
    );

    // Migrated, the same daemon is ready.
    let runs = mcpd_runs::SqliteRunStore::open(&path, mcpd_runs::RunConfig::default())
        .expect("reopen the run store");
    mcpd_runs::RunStore::migrate(&runs).await.expect("migrate the run store");
    let state = Arc::new(
        AppState::new(sqlite().await, ServerConfig::default()).with_run_store(Arc::new(runs)),
    );
    let res = router(state)
        .oneshot(Request::get("/readyz").body(Body::empty()).expect("build"))
        .await
        .expect("a response");
    assert_eq!(res.status(), StatusCode::OK);
}
