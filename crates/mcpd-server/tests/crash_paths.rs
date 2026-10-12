//! Requests that used to stop the daemon, driven through the real router.
//!
//! Each case here was one request from anyone who could reach the port, and
//! the shipped binary aborted on a panic, so one request was the whole
//! process. The router is the thing under test: a handler called directly
//! would prove nothing about what a client receives.

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
    let dir = std::env::temp_dir().join(format!("mcpd-crash-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

async fn sqlite() -> Arc<dyn Store> {
    let store = SqliteStore::open(temp_db(), 2).expect("open the store");
    store.migrate().await.expect("migrate the store");
    Arc::new(store)
}

async fn app_and_state(config: ServerConfig) -> (Router, Arc<AppState>) {
    let state = Arc::new(AppState::new(sqlite().await, config));
    (router(Arc::clone(&state)), state)
}

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
}

async fn send(app: &Router, req: Request<Body>) -> Res {
    let res = app.clone().oneshot(req).await.expect("the router returned a response");
    let status = res.status();
    let body = res.into_body().collect().await.expect("read the response body").to_bytes().to_vec();
    Res { status, body }
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
        .header("content-length", body.len().to_string())
        .body(Body::from(body))
        .expect("build the request")
}

// --------------------------------------------------------- the time range ---

/// `Duration::days` overflowed or the subtraction left the calendar, and a
/// read-only request with no key on an open API stopped the daemon.
#[tokio::test]
async fn a_relative_time_out_of_range_is_a_400_naming_the_parameter_and_the_range() {
    let (app, _state) = app_and_state(ServerConfig::default()).await;
    for uri in [
        "/v1/api/traces?from=-9999999d",
        "/v1/api/spans?to=-153722867280912931m",
        "/v1/api/logs?from=-9223372036854775807s",
        "/v1/api/analytics/summary?from=-9999999d",
        "/v1/api/analytics/cost?from=-2562047788015216h",
        "/v1/api/metrics?name=mcpd.test&from=-9999999d",
    ] {
        let res = send(&app, get(uri)).await;
        assert_eq!(res.status, StatusCode::BAD_REQUEST, "{uri}: {}", res.text());
        let body = res.json();
        assert_eq!(body["error"], json!("bad_parameter"), "{uri}: {body}");
        let field = body["field"].as_str().unwrap_or_default().to_owned();
        assert!(field == "from" || field == "to", "{uri}: the parameter is not named: {body}");
        let message = body["message"].as_str().unwrap_or_default();
        assert!(message.contains("36500"), "{uri}: the range is not named: {message}");
    }
}

// ---------------------------------------------------------- the cost path ---

/// The model id is priced at ingest for every span with a model and usage,
/// and ingest is open by default. A multi-byte character in the date tail of
/// the id panicked the pricing.
#[tokio::test]
async fn a_span_whose_model_carries_a_multi_byte_character_is_stored() {
    let (app, _state) = app_and_state(ServerConfig::default()).await;
    let batch = json!({
        "spans": [{
            "trace_id": "0123456789abcdef0123456789abcdef",
            "span_id": "0123456789abcdef",
            "name": "chat",
            "kind": "model_generation",
            "status": "ok",
            "start_time": "2026-01-01T00:00:00Z",
            "end_time": "2026-01-01T00:00:01Z",
            "is_root": true,
            "is_event": false,
            "model": "model-ab€cdefg",
            "usage": { "input_tokens": 10, "output_tokens": 10 },
            "correlation": { "project": "default" },
            "received_at": "2026-01-01T00:00:01Z"
        }]
    });
    let res = send(&app, post("/v1/ingest", batch.to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["accepted"], json!(1), "{}", res.text());
}

// ---------------------------------------------------------------- panics ---

async fn boom() -> &'static str {
    panic!("a handler panicked on purpose")
}

/// No shipped route panics on purpose, so the real layer stack is put around
/// one that does. Without the catch the panic unwound out of the router and a
/// client saw its connection dropped with nothing to read; in the shipped
/// binary, which aborted on a panic, every other connection went with it.
#[tokio::test]
async fn a_handler_that_panics_answers_a_500_in_the_json_shape_and_the_router_carries_on() {
    let state = Arc::new(AppState::new(sqlite().await, ServerConfig::default()));
    let routes = Router::new()
        .route("/v1/api/boom", axum::routing::get(boom))
        .route("/v1/api/fine", axum::routing::get(|| async { "fine" }));
    let app = mcpd_server::layered(routes, Arc::clone(&state));

    let res = send(&app, get("/v1/api/boom")).await;
    assert_eq!(res.status, StatusCode::INTERNAL_SERVER_ERROR, "{}", res.text());
    let body = res.json();
    assert_eq!(body["error"], json!("internal_error"), "{body}");
    let message = body["message"].as_str().unwrap_or_default();
    assert!(message.contains("Retry"), "the client is not told what to do: {message}");
    assert!(
        !message.contains("on purpose"),
        "the panic's own text belongs in the log, not the response: {message}"
    );
    assert_eq!(
        state.metrics.errors.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "a panic is a server error and is counted as one"
    );

    let after = send(&app, get("/v1/api/fine")).await;
    assert_eq!(after.status, StatusCode::OK, "the router stopped serving after a panic");
}

/// A pass that panics is abandoned and the loop runs the next one. Without the
/// catch the first panic ended the task, and the loop it ran stopped for good
/// while nothing reported it.
#[tokio::test]
async fn a_loop_whose_pass_panics_logs_it_and_runs_the_next_pass() {
    use std::sync::atomic::{AtomicU32, Ordering};
    let state = Arc::new(AppState::new(sqlite().await, ServerConfig::default()));
    let passes = Arc::new(AtomicU32::new(0));
    let counted = Arc::clone(&passes);
    mcpd_server::spawn_loop(
        &state,
        "test",
        std::time::Duration::from_millis(10),
        mcpd_server::FirstPass::AtStart,
        move || {
            let counted = Arc::clone(&counted);
            async move {
                if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                    panic!("the first pass panics on purpose");
                }
            }
        },
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while passes.load(Ordering::SeqCst) < 3 && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        passes.load(Ordering::SeqCst) >= 3,
        "the loop stopped after its first pass panicked: {} pass(es) ran",
        passes.load(Ordering::SeqCst)
    );
    // The loop is tracked and stops on shutdown like every other.
    assert_eq!(state.drain(std::time::Duration::from_secs(5)).await, 0);
}

/// A panic while the monitor state was held poisoned its lock, and every tick
/// after that returned at the lock without folding a reading in. The loop
/// kept running and the monitors never moved again.
#[tokio::test]
async fn a_monitor_tick_takes_back_a_state_lock_a_panic_poisoned() {
    let monitor: mcpd_server::monitor::Monitor = serde_json::from_value(json!({
        "id": "errors", "name": "errors", "project": "default",
        "watch": "error_rate", "compare": "above", "threshold": 0.5, "min_samples": 1
    }))
    .expect("a monitor from its config shape");
    let config = ServerConfig { monitors: vec![monitor], ..ServerConfig::default() };
    let (_app, state) = app_and_state(config).await;

    let holder = Arc::clone(&state);
    let poisoned = std::thread::spawn(move || {
        let _guard = holder.monitors.write().expect("the lock is clean before the panic");
        panic!("a holder of the monitor state panicked on purpose");
    })
    .join();
    assert!(poisoned.is_err(), "the holder did not panic");
    assert!(state.monitors.is_poisoned(), "the lock should be poisoned before the tick");

    mcpd_server::monitor::tick(&state, time::OffsetDateTime::now_utc()).await;
    assert!(!state.monitors.is_poisoned(), "the tick left the monitor state poisoned");
    let states = state.monitors.read().expect("readable after the tick");
    assert!(states[0].last.is_some(), "the tick folded no reading into the monitor");
}

#[tokio::test]
async fn a_pass_that_panics_reports_that_it_did_not_finish() {
    assert!(mcpd_server::run_tick("test", async {}).await);
    assert!(
        !mcpd_server::run_tick("test", async {
            panic!("on purpose");
        })
        .await
    );
}

// ------------------------------------------------------- background passes ---

/// Start-up refuses these windows; this is the second line, for settings
/// built without going through it. Either subtraction used to panic the
/// timer on every tick.
#[tokio::test]
async fn a_window_past_the_calendar_does_not_panic_an_objective_or_an_aspect_pass() {
    let slo: mcpd_server::monitor::Slo = serde_json::from_value(json!({
        "id": "far", "name": "far", "project": "default",
        "objective": 0.995, "window_days": u32::MAX
    }))
    .expect("an objective from its config shape");
    let mut config = ServerConfig { slos: vec![slo.clone()], ..ServerConfig::default() };
    // The largest value that converts to a signed count of seconds. Above it
    // the pass already fell back to a day, so it is this one that panicked.
    config.aspects.window_seconds = i64::MAX.unsigned_abs();
    let (_app, state) = app_and_state(config).await;
    let now = time::OffsetDateTime::now_utc();
    mcpd_server::monitor::evaluate_slo(&state, &slo, now).await.expect("an objective reading");
    mcpd_server::aspects::run_once(&state, now).await.expect("an aspect pass");
}

// ------------------------------------------------------------- ingest auth ---

/// A request body that records whether anything read it, and never ends.
///
/// Never ending is the point: a door that reads the body before checking the
/// key waits on it for ever, and the test sees that as a timeout rather than
/// as a 401 that arrived after the bytes were already held.
struct Unread(Arc<std::sync::atomic::AtomicBool>);

impl futures::Stream for Unread {
    type Item = Result<bytes::Bytes, std::io::Error>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        std::task::Poll::Pending
    }
}

/// The three OTLP doors took the body as `Bytes`, which axum reads in full
/// before the handler runs, so a client with no key made the daemon buffer up
/// to the body cap per request before it was refused. The key is now checked
/// from the headers first, on all four doors.
#[tokio::test]
async fn a_request_without_a_key_is_refused_before_its_body_is_read() {
    let config =
        ServerConfig { ingest_keys: vec!["the-only-key".to_owned()], ..ServerConfig::default() };
    let (app, _state) = app_and_state(config).await;
    for door in ["/v1/traces", "/v1/logs", "/v1/metrics", "/v1/ingest"] {
        let read = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let req = Request::builder()
            .method("POST")
            .uri(door)
            .header("content-type", "application/json")
            .body(Body::from_stream(Unread(Arc::clone(&read))))
            .expect("build the request");
        let res = tokio::time::timeout(std::time::Duration::from_secs(5), send(&app, req))
            .await
            .unwrap_or_else(|_| {
                panic!("{door} waited on the body of a request it should have refused")
            });
        assert_eq!(res.status, StatusCode::UNAUTHORIZED, "{door}: {}", res.text());
        assert_eq!(res.json()["error"], json!("unauthorized"), "{door}");
        assert!(
            !read.load(std::sync::atomic::Ordering::SeqCst),
            "{door} read the body before checking the key"
        );
    }
}

/// The order changed, the caps did not: a keyed request over the body cap is
/// still a 413 in the JSON shape, on each OTLP door.
#[tokio::test]
async fn a_keyed_otlp_body_over_the_cap_is_still_refused_naming_the_limit() {
    let config = ServerConfig {
        ingest_keys: vec!["the-only-key".to_owned()],
        max_body_bytes: 1_024,
        ..ServerConfig::default()
    };
    let (app, _state) = app_and_state(config).await;
    for door in ["/v1/traces", "/v1/logs", "/v1/metrics"] {
        let body = vec![b' '; 4_096];
        // No declared length, so the limit is met while the handler reads.
        let req = Request::builder()
            .method("POST")
            .uri(door)
            .header("content-type", "application/json")
            .header("authorization", "Bearer the-only-key")
            .body(Body::from_stream(futures::stream::iter([Ok::<_, std::io::Error>(
                bytes::Bytes::from(body),
            )])))
            .expect("build the request");
        let res = send(&app, req).await;
        assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE, "{door}: {}", res.text());
        assert!(res.json()["message"].as_str().unwrap_or_default().contains("1024"), "{door}");
    }
}
