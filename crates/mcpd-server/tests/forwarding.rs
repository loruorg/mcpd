//! Forwarding, against a destination that answers the way real ones do.
//!
//! The destination is a small HTTP server on a port the OS assigns, scripted
//! per test to answer each attempt with a chosen status. Every case here reads
//! back two things the daemon exposes: how many attempts the destination saw,
//! and `mcpd_forward_dropped_spans_total`, which is the only way an operator
//! can tell a copy is missing.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::Router;
use mcpd_core::Span;
use mcpd_server::forward::{send, Forwarder};
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::Store;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ------------------------------------------------------------- fixtures ---

fn temp_db() -> std::path::PathBuf {
    use std::sync::atomic::AtomicU64;
    static N: AtomicU64 = AtomicU64::new(0);
    // The pid and a per-process stamp, for the reason `tests/http.rs` gives.
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir =
        std::env::temp_dir().join(format!("mcpd-forward-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

async fn state_forwarding_to(endpoint: &str) -> Arc<AppState> {
    let store = SqliteStore::open(temp_db(), 2).expect("open the store");
    store.migrate().await.expect("migrate the store");
    let config = ServerConfig {
        forward: Some(Forwarder {
            endpoint: endpoint.to_owned(),
            headers: vec![("x-destination-key".to_owned(), "abc".to_owned())],
            enabled: true,
            batch_size: 256,
        }),
        ..ServerConfig::default()
    };
    Arc::new(AppState::new(Arc::new(store) as Arc<dyn Store>, config))
}

/// `n` spans, each its own run. Numbered from one: an id of all zeros is not
/// a valid id, and the ingest door refuses it.
fn spans(n: u32) -> Vec<Span> {
    (1..=n)
        .map(|i| {
            serde_json::from_value(json!({
                "trace_id": format!("{i:032x}"),
                "span_id": format!("{i:016x}"),
                "name": "checkout",
                "kind": "agent_run",
                "status": "ok",
                "start_time": "2026-01-01T00:00:00Z",
                "end_time": "2026-01-01T00:00:01Z",
                "is_root": true,
                "is_event": false,
                "correlation": { "project": "default" },
                "received_at": "2026-01-01T00:00:01Z"
            }))
            .expect("a span from its wire shape")
        })
        .collect()
}

// ------------------------------------------------------ the destination ---

/// What the destination will answer, in order, and what it saw.
#[derive(Default)]
struct Destination {
    /// Statuses to answer with, one per attempt, each with an optional
    /// `Retry-After` in seconds. Once spent, every attempt is a 200.
    script: VecDeque<(u16, Option<u64>)>,
    /// When each attempt arrived.
    attempts: Vec<Instant>,
    /// Spans received in answered-200 batches.
    accepted: usize,
    /// Whether every attempt carried the configured header.
    header_seen: bool,
}

type Shared = Arc<Mutex<Destination>>;

async fn receive(
    State(dest): State<Shared>,
    headers: HeaderMap,
    body: String,
) -> (StatusCode, HeaderMap) {
    let mut d = dest.lock().expect("the destination lock");
    d.attempts.push(Instant::now());
    d.header_seen = headers.get("x-destination-key").is_some_and(|v| v == "abc");
    let (status, retry_after) = d.script.pop_front().unwrap_or((200, None));
    let mut out = HeaderMap::new();
    if let Some(secs) = retry_after {
        out.insert("retry-after", secs.to_string().parse().expect("a header value"));
    }
    if status == 200 {
        let v: Value = serde_json::from_str(&body).expect("an OTLP JSON body");
        d.accepted += v["resourceSpans"]
            .as_array()
            .map(|rs| {
                rs.iter()
                    .flat_map(|r| r["scopeSpans"].as_array().cloned().unwrap_or_default())
                    .map(|s| s["spans"].as_array().map_or(0, Vec::len))
                    .sum::<usize>()
            })
            .unwrap_or(0);
    }
    (StatusCode::from_u16(status).expect("a status"), out)
}

/// Start a destination answering the script, on a port the OS assigns.
async fn destination(script: &[(u16, Option<u64>)]) -> (String, Shared) {
    let shared: Shared = Arc::new(Mutex::new(Destination {
        script: script.iter().copied().collect(),
        ..Default::default()
    }));
    let app = Router::new().route("/v1/traces", post(receive)).with_state(Arc::clone(&shared));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind a port");
    let addr = listener.local_addr().expect("the bound address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), shared)
}

fn attempts(d: &Shared) -> Vec<Instant> {
    d.lock().expect("the destination lock").attempts.clone()
}

fn dropped(state: &AppState) -> u64 {
    state.metrics.forward_dropped_spans.load(Ordering::Relaxed)
}

// ------------------------------------------------------------- the cases ---

#[tokio::test]
async fn a_batch_the_destination_accepts_is_delivered_once_with_its_headers() {
    let (url, dest) = destination(&[]).await;
    let state = state_forwarding_to(&url).await;
    send(&state, &spans(3)).await;
    assert_eq!(attempts(&dest).len(), 1);
    let d = dest.lock().expect("the destination lock");
    assert_eq!(d.accepted, 3);
    assert!(d.header_seen, "the configured header was not sent");
    assert_eq!(dropped(&state), 0);
}

/// A 5xx and a 429 are the answers a destination gives while it is busy or
/// briefly down. One attempt and a log line lost the batch; the retry is what
/// a copy that is meant to be complete needs.
#[tokio::test]
async fn a_5xx_and_a_429_are_retried_and_the_retry_after_is_honoured() {
    let (url, dest) = destination(&[(503, None), (429, Some(1))]).await;
    let state = state_forwarding_to(&url).await;
    send(&state, &spans(4)).await;

    let seen = attempts(&dest);
    assert_eq!(seen.len(), 3, "three attempts: a 503, a 429, then the 200");
    assert!(
        seen[1] - seen[0] >= Duration::from_millis(200),
        "the second attempt came too soon after the 503: {:?}",
        seen[1] - seen[0]
    );
    assert!(
        seen[2] - seen[1] >= Duration::from_secs(1),
        "the Retry-After of one second was not honoured: {:?}",
        seen[2] - seen[1]
    );
    assert_eq!(dest.lock().expect("the destination lock").accepted, 4);
    assert_eq!(dropped(&state), 0, "a batch that was delivered in the end is not a drop");
}

/// A 4xx other than 429 says the batch is wrong for this destination, and it
/// will be wrong on every retry. Retrying it would be three times the load
/// for the same answer.
#[tokio::test]
async fn a_4xx_is_final_on_the_first_answer_and_counted_by_span() {
    let (url, dest) = destination(&[(400, None)]).await;
    let state = state_forwarding_to(&url).await;
    send(&state, &spans(5)).await;
    assert_eq!(attempts(&dest).len(), 1, "a 400 was retried");
    assert_eq!(dropped(&state), 5, "the counter moves by spans, not by requests");
}

#[tokio::test]
async fn a_batch_still_refused_after_three_attempts_is_counted_dropped() {
    let (url, dest) = destination(&[(503, None), (502, None), (500, None)]).await;
    let state = state_forwarding_to(&url).await;
    send(&state, &spans(2)).await;
    assert_eq!(attempts(&dest).len(), 3, "the retry is bounded at three attempts");
    assert_eq!(dropped(&state), 2);
}

#[tokio::test]
async fn an_unreachable_destination_is_counted_after_three_attempts() {
    // A port the OS handed out and nothing listens on.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a port");
        l.local_addr().expect("the bound address").port()
    };
    let state = state_forwarding_to(&format!("http://127.0.0.1:{port}")).await;
    let started = Instant::now();
    send(&state, &spans(7)).await;
    assert_eq!(dropped(&state), 7);
    // Two backoffs, 250 ms then 500 ms, sit between the three attempts.
    assert!(started.elapsed() >= Duration::from_millis(700), "{:?}", started.elapsed());
}

#[tokio::test]
async fn each_chunk_of_a_large_batch_is_counted_on_its_own() {
    // Batch size 2, five spans, so three requests. The middle one fails for
    // good; the other two land. The counter says two spans are missing.
    let (url, dest) = destination(&[(200, None), (400, None), (200, None)]).await;
    let mut state = state_forwarding_to(&url).await;
    Arc::get_mut(&mut state)
        .expect("the only handle")
        .config
        .forward
        .as_mut()
        .expect("forwarding is configured")
        .batch_size = 2;
    send(&state, &spans(5)).await;
    assert_eq!(attempts(&dest).len(), 3);
    assert_eq!(dest.lock().expect("the destination lock").accepted, 3);
    assert_eq!(dropped(&state), 2);
}

/// Through the real ingest door. The forward runs on its own task after the
/// response, and the daemon's shutdown waits for it: a bare `tokio::spawn` was
/// dropped with the runtime, and a batch the sender was told was stored never
/// reached the destination.
#[tokio::test]
async fn a_forward_started_by_an_ingest_is_waited_for_on_drain() {
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let (url, dest) = destination(&[(503, None)]).await;
    let state = state_forwarding_to(&url).await;
    let app = router(Arc::clone(&state));

    let batch = json!({ "spans": spans(3) }).to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/ingest")
        .header("content-type", "application/json")
        .header("content-length", batch.len().to_string())
        .body(Body::from(batch))
        .expect("build the request");
    let res = app.oneshot(req).await.expect("the router answered");
    let status = res.status();
    let body = res.into_body().collect().await.expect("read the body").to_bytes();
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // The first attempt was refused with a 503, so at this moment the batch is
    // waiting to be retried. Drain has to wait through that.
    let unfinished = state.drain(Duration::from_secs(5)).await;
    assert_eq!(unfinished, 0, "drain gave up with a forward still running");
    assert_eq!(attempts(&dest).len(), 2, "the retry did not run before shutdown completed");
    assert_eq!(dest.lock().expect("the destination lock").accepted, 3);
    assert_eq!(dropped(&state), 0);
}

/// Shutdown cuts a retry wait short and spends the remaining attempts at once.
/// Without that, a destination asking for thirty seconds would hold a stopping
/// daemon to its whole grace period and the batch would be dropped anyway.
#[tokio::test]
async fn a_retry_waiting_on_a_long_retry_after_is_made_at_once_when_shutdown_begins() {
    let (url, dest) = destination(&[(429, Some(30))]).await;
    let state = state_forwarding_to(&url).await;
    let owned = Arc::clone(&state);
    let sending = state.tasks.spawn(async move { send(&owned, &spans(1)).await });
    // Give the first attempt time to be answered.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(attempts(&dest).len(), 1);

    let started = Instant::now();
    let unfinished = state.drain(Duration::from_secs(5)).await;
    assert_eq!(unfinished, 0);
    assert!(started.elapsed() < Duration::from_secs(5), "drain waited out the Retry-After");
    sending.await.expect("the forward task finished");
    assert_eq!(attempts(&dest).len(), 2, "the second attempt was not made before the stop");
    assert_eq!(dest.lock().expect("the destination lock").accepted, 1);
    assert_eq!(dropped(&state), 0, "a batch delivered during the stop is not a drop");
}

/// A destination that accepts every request and never answers one.
async fn silent_destination() -> String {
    async fn never() -> StatusCode {
        std::future::pending::<()>().await;
        StatusCode::OK
    }
    let app = Router::new().route("/v1/traces", post(never));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind a port");
    let addr = listener.local_addr().expect("the bound address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

/// Each forward held a full copy of its request's spans for up to three
/// attempts and the waits between them, and nothing bounded how many there
/// were. A destination that accepts and never answers grew memory with every
/// ingest. Past the bound the copy is dropped and counted, and the ingest
/// itself still succeeds.
#[tokio::test]
async fn a_destination_that_never_answers_holds_a_bounded_number_of_copies() {
    use axum::body::Body;
    use axum::http::Request;
    use mcpd_server::forward::MAX_IN_FLIGHT;
    use tower::ServiceExt;

    let state = state_forwarding_to(&silent_destination().await).await;
    let app = router(Arc::clone(&state));
    let extra = 6;
    let all = spans(u32::try_from(MAX_IN_FLIGHT + extra).expect("a small count"));
    for (i, span) in all.into_iter().enumerate() {
        let batch = json!({ "spans": [span] }).to_string();
        let req = Request::builder()
            .method("POST")
            .uri("/v1/ingest")
            .header("content-type", "application/json")
            .header("content-length", batch.len().to_string())
            .body(Body::from(batch))
            .expect("build the request");
        let res = app.clone().oneshot(req).await.expect("the router answered");
        assert_eq!(res.status(), StatusCode::OK, "ingest {i} was refused");
    }

    assert_eq!(
        dropped(&state),
        u64::try_from(extra).expect("a small count"),
        "every copy past the bound is dropped and counted, by span"
    );
    assert_eq!(state.forward_slots.available_permits(), 0, "the bound is full, not exceeded");
    assert!(
        state.tasks.len() <= MAX_IN_FLIGHT,
        "{} forwards are held against a bound of {MAX_IN_FLIGHT}",
        state.tasks.len()
    );
}
