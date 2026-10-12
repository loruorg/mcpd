//! The shapes the ingest doors accept, and the bounds they hold them to.
//!
//! Three gaps are covered here, each driven through the real router so the
//! extractors, the body limit layer and the error mapping are part of what is
//! tested:
//!
//! 1. `/v1/ingest` took exactly one JSON object. It now also takes an array of
//!    batches, and NDJSON with one batch per line, chosen by the content type.
//! 2. The per-request bounds are the same whatever shape a request arrives in,
//!    and the OTLP log and metric doors have one at all.
//! 3. A refusal for a spent ceiling carries `Retry-After`, which every SDK
//!    already honours and mcpd never sent.

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
/// stamp. Pids are recycled, and a recycled one reopens the previous run's
/// database. See `tests/http.rs` for the whole story.
fn temp_db() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir = std::env::temp_dir().join(format!("mcpd-shapes-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

async fn app_with(config: ServerConfig) -> Router {
    let store = SqliteStore::open(temp_db(), 2).expect("open the store");
    store.migrate().await.expect("migrate the store");
    let state = Arc::new(AppState::new(Arc::new(store), config));
    router(state)
}

struct Res {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}

impl Res {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("the body is not JSON ({e}): {}", self.text()))
    }

    /// The `error` code and `message` of a refusal.
    fn error(&self) -> (String, String) {
        let v = self.json();
        (
            v["error"].as_str().unwrap_or_default().to_owned(),
            v["message"].as_str().unwrap_or_default().to_owned(),
        )
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

fn post(uri: &str, content_type: &str, body: impl Into<Vec<u8>>) -> Request<Body> {
    let body: Vec<u8> = body.into();
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", content_type)
        // A real client always sends this, and the body limit reads it.
        .header("content-length", body.len().to_string())
        .body(Body::from(body))
        .expect("build the request")
}

/// The same request with no declared length, so the body limit is met while
/// reading rather than before.
fn post_chunked(uri: &str, content_type: &str, body: impl Into<Vec<u8>>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", content_type)
        .body(Body::from(body.into()))
        .expect("build the request")
}

fn get(uri: &str) -> Request<Body> {
    Request::builder().method("GET").uri(uri).body(Body::empty()).expect("build the request")
}

/// A 32-character trace id that differs per `n`.
fn trace_id(n: usize) -> String {
    format!("{n:032x}")
}

/// One batch carrying one span of trace `n`.
fn batch(n: usize) -> Value {
    json!({
        "spans": [{
            "trace_id": trace_id(n),
            "span_id": format!("{n:016x}"),
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

/// One batch carrying `count` scores, which is the cheapest signal to make in
/// bulk: no ids to keep distinct and no trace to hang it on.
fn scores(count: usize, from: usize) -> Value {
    let items: Vec<Value> = (0..count)
        .map(|i| {
            json!({
                "score_id": format!("s-{}", from + i),
                "trace_id": trace_id(1),
                "scorer_id": "helpfulness",
                "source": "manual",
                "value": 0.5,
                "timestamp": "2026-01-01T00:00:01Z",
                "correlation": { "project": "default" }
            })
        })
        .collect();
    json!({ "scores": items })
}

// --------------------------------------------------- the one-object door ---

/// The shape that already worked, unchanged. Everything below adds to this
/// door, and the first thing to prove is that nothing was taken away.
#[tokio::test]
async fn one_json_object_is_still_one_batch() {
    let app = app_with(ServerConfig::default()).await;
    let res = send(&app, post("/v1/ingest", "application/json", batch(1).to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["accepted"], 1);
    assert_eq!(res.json()["rejected"], 0);

    let stored = send(&app, get(&format!("/v1/api/traces/{}", trace_id(1)))).await;
    assert_eq!(stored.status, StatusCode::OK, "the span did not land: {}", stored.text());
}

/// A body that is neither shape is still refused in the same words, because
/// that message is what a client with a serialisation bug reads.
#[tokio::test]
async fn a_body_that_is_not_a_batch_is_still_refused_naming_the_batch() {
    let app = app_with(ServerConfig::default()).await;
    let res = send(&app, post("/v1/ingest", "application/json", "\"nonsense\"")).await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "{}", res.text());
    let (code, message) = res.error();
    assert_eq!(code, "bad_request");
    assert!(message.contains("ingest batch"), "the message should name the shape: {message}");
}

// --------------------------------------------------------- the array door ---

#[tokio::test]
async fn an_array_of_batches_is_accepted_and_every_element_is_stored() {
    let app = app_with(ServerConfig::default()).await;
    let body = json!([batch(1), batch(2), batch(3)]).to_string();
    let res = send(&app, post("/v1/ingest", "application/json", body)).await;
    assert_eq!(res.status, StatusCode::OK, "an array of batches was refused: {}", res.text());
    assert_eq!(res.json()["accepted"], 3, "{}", res.text());

    for n in 1..=3 {
        let stored = send(&app, get(&format!("/v1/api/traces/{}", trace_id(n)))).await;
        assert_eq!(stored.status, StatusCode::OK, "element {n} did not land: {}", stored.text());
    }
}

/// The array is told apart from the object by JSON's own grammar, so a
/// malformed element still reports the field that was wrong rather than
/// "did not match any variant".
#[tokio::test]
async fn a_malformed_element_of_an_array_names_what_was_wrong_with_it() {
    let app = app_with(ServerConfig::default()).await;
    let body = json!([batch(1), { "spans": "not a list" }]).to_string();
    let res = send(&app, post("/v1/ingest", "application/json", body)).await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "{}", res.text());
    let (_, message) = res.error();
    assert!(
        message.contains("invalid type") || message.contains("expected"),
        "the serde message should survive: {message}"
    );
}

// -------------------------------------------------------- the NDJSON door ---

#[tokio::test]
async fn ndjson_is_one_batch_per_line() {
    let app = app_with(ServerConfig::default()).await;
    let body = format!("{}\n{}\n{}\n", batch(1), batch(2), batch(3));
    let res = send(&app, post("/v1/ingest", "application/x-ndjson", body)).await;
    assert_eq!(res.status, StatusCode::OK, "ndjson was refused: {}", res.text());
    assert_eq!(res.json()["accepted"], 3, "{}", res.text());

    for n in 1..=3 {
        let stored = send(&app, get(&format!("/v1/api/traces/{}", trace_id(n)))).await;
        assert_eq!(stored.status, StatusCode::OK, "line {n} did not land: {}", stored.text());
    }
}

/// Blank lines, a missing final newline and `\r\n` all arrive from real
/// producers, and none of them is an error.
#[tokio::test]
async fn ndjson_tolerates_blank_lines_carriage_returns_and_no_final_newline() {
    let app = app_with(ServerConfig::default()).await;
    let body = format!("\n{}\r\n\n{}", batch(1), batch(2));
    let res = send(&app, post("/v1/ingest", "application/x-ndjson", body)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["accepted"], 2, "{}", res.text());
}

/// The line number, because "the body is not a batch" is unhelpful when the
/// body is ten thousand of them.
#[tokio::test]
async fn a_bad_ndjson_line_is_refused_naming_the_line() {
    let app = app_with(ServerConfig::default()).await;
    let body = format!("{}\n{{ not json }}\n", batch(1));
    let res = send(&app, post("/v1/ingest", "application/x-ndjson", body)).await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "{}", res.text());
    let (_, message) = res.error();
    assert!(
        message.contains("line 2 is not a valid ingest batch"),
        "the message should name the line that failed: {message}"
    );

    // Nothing was stored: the request is refused whole, the way the one-object
    // door refuses a body it cannot read.
    let stored = send(&app, get(&format!("/v1/api/traces/{}", trace_id(1)))).await;
    assert_eq!(stored.status, StatusCode::NOT_FOUND, "line 1 was stored before line 2 failed");
}

/// One line is one batch, so an `ingest_key` on a line covers that line and
/// nothing else. A resent stream is recognised line by line.
#[tokio::test]
async fn an_ingest_key_on_a_line_makes_that_line_a_duplicate_on_a_resend() {
    let app = app_with(ServerConfig::default()).await;
    let mut keyed = batch(1);
    keyed["ingest_key"] = json!("line-one");
    let body = format!("{keyed}\n{}\n", batch(2));

    let first = send(&app, post("/v1/ingest", "application/x-ndjson", body.clone())).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.text());
    assert_eq!(first.json()["accepted"], 2, "{}", first.text());

    let again = send(&app, post("/v1/ingest", "application/x-ndjson", body)).await;
    assert_eq!(again.status, StatusCode::OK, "{}", again.text());
    assert_eq!(
        again.json()["duplicate"],
        1,
        "the keyed line should be recognised as a replay: {}",
        again.text()
    );
}

/// A gzip body cannot be read a line at a time until it has been inflated, so
/// the door collects it first. The shape it decodes to is the same.
#[tokio::test]
async fn gzip_ndjson_is_inflated_and_then_read_line_by_line() {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    let app = app_with(ServerConfig::default()).await;
    let body = format!("{}\n{}\n", batch(1), batch(2));
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(body.as_bytes()).expect("compress the body");
    let gz = enc.finish().expect("finish the gzip stream");

    let req = Request::builder()
        .method("POST")
        .uri("/v1/ingest")
        .header("content-type", "application/x-ndjson")
        .header("content-encoding", "gzip")
        .header("content-length", gz.len().to_string())
        .body(Body::from(gz))
        .expect("build the request");
    let res = send(&app, req).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["accepted"], 2, "{}", res.text());
}

// -------------------------------------------------- bounds on the request ---

/// The cap is per request, not per line. Counting each line on its own would
/// make NDJSON the way around a limit every other door applies.
#[tokio::test]
async fn the_signal_cap_counts_the_whole_ndjson_request_not_one_line() {
    let config = ServerConfig { max_batch_signals: 10, ..ServerConfig::default() };
    let app = app_with(config).await;

    // Four lines of four scores: no line is over the cap of ten, the request is.
    let body = (0..4).map(|i| format!("{}\n", scores(4, i * 4))).collect::<String>();
    let res = send(&app, post("/v1/ingest", "application/x-ndjson", body)).await;
    assert_eq!(
        res.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "the cap was applied per line, not per request: {}",
        res.text()
    );
    let (code, message) = res.error();
    assert_eq!(code, "payload_too_large");
    assert!(message.contains("10"), "the message should name the cap: {message}");
}

/// The same, in the array shape.
#[tokio::test]
async fn the_signal_cap_counts_the_whole_array_not_one_element() {
    let config = ServerConfig { max_batch_signals: 10, ..ServerConfig::default() };
    let app = app_with(config).await;
    let body = json!([scores(4, 0), scores(4, 4), scores(4, 8)]).to_string();
    let res = send(&app, post("/v1/ingest", "application/json", body)).await;
    assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE, "{}", res.text());
    assert_eq!(res.error().0, "payload_too_large");
}

/// The one-object door's refusal is unchanged: it names what the batch carried
/// and what the server takes.
#[tokio::test]
async fn one_object_over_the_cap_is_refused_in_the_words_it_always_was() {
    let config = ServerConfig { max_batch_signals: 2, ..ServerConfig::default() };
    let app = app_with(config).await;
    let res = send(&app, post("/v1/ingest", "application/json", scores(5, 0).to_string())).await;
    assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE, "{}", res.text());
    let (_, message) = res.error();
    assert!(
        message.contains("batch carries 5 signals"),
        "the existing wording should survive: {message}"
    );
    assert!(message.contains("accepts 2 at a time"), "{message}");
}

/// Nothing is stored when the cap refuses the request, whatever shape it is
/// in. A 413 that had already written half the lines would be a lie.
#[tokio::test]
async fn nothing_is_stored_when_the_request_is_over_the_signal_cap() {
    let config = ServerConfig { max_batch_signals: 2, ..ServerConfig::default() };
    let app = app_with(config).await;
    let body = format!("{}\n{}\n{}\n", batch(1), batch(2), batch(3));
    let res = send(&app, post("/v1/ingest", "application/x-ndjson", body)).await;
    assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE, "{}", res.text());
    for n in 1..=3 {
        let stored = send(&app, get(&format!("/v1/api/traces/{}", trace_id(n)))).await;
        assert_eq!(stored.status, StatusCode::NOT_FOUND, "line {n} was stored anyway");
    }
}

/// The body cap holds across the stream too, and a streamed body meets it
/// while it is being read rather than before.
#[tokio::test]
async fn the_body_cap_holds_an_ndjson_stream_with_no_declared_length() {
    let config = ServerConfig { max_body_bytes: 2048, ..ServerConfig::default() };
    let app = app_with(config).await;
    let line = format!("{}\n", batch(1));
    let body = line.repeat(64);
    assert!(body.len() > 2048, "the body has to be over the cap to test it");

    let res = send(&app, post_chunked("/v1/ingest", "application/x-ndjson", body)).await;
    assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE, "{}", res.text());
    let (code, message) = res.error();
    assert_eq!(code, "payload_too_large");
    assert!(message.contains("2048"), "the message should name the cap: {message}");
}

// ------------------------------------------------ the OTLP logs and metrics ---

/// One OTLP log export carrying `count` records.
fn otlp_logs(count: usize) -> String {
    let records: Vec<Value> = (0..count)
        .map(|i| {
            json!({
                "timeUnixNano": (1_700_000_000_000_000_000u64 + i as u64).to_string(),
                "severityNumber": 9,
                "body": { "stringValue": format!("line {i}") }
            })
        })
        .collect();
    json!({
        "resourceLogs": [{
            "scopeLogs": [{ "scope": { "name": "orders" }, "logRecords": records }]
        }]
    })
    .to_string()
}

/// One OTLP gauge export carrying `count` data points.
fn otlp_metrics(count: usize) -> String {
    let points: Vec<Value> = (0..count)
        .map(|i| {
            json!({
                "timeUnixNano": (1_700_000_000_000_000_000u64 + i as u64).to_string(),
                "asDouble": i as f64
            })
        })
        .collect();
    json!({
        "resourceMetrics": [{
            "scopeMetrics": [{
                "metrics": [{ "name": "queue.depth", "gauge": { "dataPoints": points } }]
            }]
        }]
    })
    .to_string()
}

/// The logs door had no record cap at all, so one body inside the byte limit
/// could carry as many records as fit in it.
#[tokio::test]
async fn the_otlp_logs_door_refuses_more_records_than_the_batch_cap() {
    let config = ServerConfig { max_batch_signals: 5, ..ServerConfig::default() };
    let app = app_with(config).await;
    let res = send(&app, post("/v1/logs", "application/json", otlp_logs(9))).await;
    assert_eq!(
        res.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "the logs door stored an unbounded export: {}",
        res.text()
    );
    let (code, message) = res.error();
    assert_eq!(code, "payload_too_large");
    assert!(message.contains('9') && message.contains('5'), "name both numbers: {message}");
}

#[tokio::test]
async fn an_otlp_log_export_inside_the_cap_is_still_stored() {
    let config = ServerConfig { max_batch_signals: 5, ..ServerConfig::default() };
    let app = app_with(config).await;
    let res = send(&app, post("/v1/logs", "application/json", otlp_logs(5))).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["partialSuccess"]["rejectedLogRecords"], 0);
}

#[tokio::test]
async fn the_otlp_metrics_door_refuses_more_points_than_the_batch_cap() {
    let config = ServerConfig { max_batch_signals: 4, ..ServerConfig::default() };
    let app = app_with(config).await;
    let res = send(&app, post("/v1/metrics", "application/json", otlp_metrics(7))).await;
    assert_eq!(
        res.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "the metrics door stored an unbounded export: {}",
        res.text()
    );
    assert_eq!(res.error().0, "payload_too_large");
}

#[tokio::test]
async fn an_otlp_metric_export_inside_the_cap_is_still_stored() {
    let config = ServerConfig { max_batch_signals: 4, ..ServerConfig::default() };
    let app = app_with(config).await;
    let res = send(&app, post("/v1/metrics", "application/json", otlp_metrics(4))).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["partialSuccess"]["rejectedDataPoints"], 0);
}

// ------------------------------------------------------------ Retry-After ---

fn a_score() -> Value {
    json!({
        "score_id": "",
        "trace_id": trace_id(1),
        "scorer_id": "helpfulness",
        "source": "manual",
        "value": 0.75,
        "timestamp": "2026-01-01T00:00:01Z",
        "correlation": { "project": "retry-after" }
    })
}

/// The SDKs all honour `Retry-After` and mcpd never sent one, so a 429 told a
/// client to back off and left it to guess for how long.
#[tokio::test]
async fn the_over_quota_refusal_carries_a_retry_after_header() {
    let config = ServerConfig { quota_per_minute: Some(1), ..ServerConfig::default() };
    let app = app_with(config).await;

    let first = send(&app, post("/v1/api/scores", "application/json", a_score().to_string())).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.text());

    let over = send(&app, post("/v1/api/scores", "application/json", a_score().to_string())).await;
    assert_eq!(over.status, StatusCode::TOO_MANY_REQUESTS, "{}", over.text());
    assert_eq!(over.error().0, "over_quota");

    let header = over.header("retry-after").expect("a 429 has to say how long to wait");
    let seconds: u64 = header.parse().expect("Retry-After is a whole number of seconds");
    assert!(seconds >= 1, "a wait of zero is a hot loop");
    // The window is a minute, and this ceiling was spent at the start of one,
    // so the wait is most of a minute. Two windows is the arithmetic ceiling.
    assert!(seconds <= 120, "the wait should be derived from the window, got {seconds}");
}

/// The same header on the feedback door, which is the other side of the same
/// counter.
#[tokio::test]
async fn the_feedback_door_sends_retry_after_too() {
    let config = ServerConfig { quota_per_minute: Some(1), ..ServerConfig::default() };
    let app = app_with(config).await;
    let body = json!({
        "feedback_id": "",
        "trace_id": trace_id(1),
        "kind": "thumbs",
        "value": "up",
        "timestamp": "2026-01-01T00:00:01Z",
        "correlation": { "project": "retry-after" }
    })
    .to_string();

    let first = send(&app, post("/v1/api/feedback", "application/json", body.clone())).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.text());
    let over = send(&app, post("/v1/api/feedback", "application/json", body)).await;
    assert_eq!(over.status, StatusCode::TOO_MANY_REQUESTS, "{}", over.text());
    assert!(over.header("retry-after").is_some(), "the feedback door sent no wait");
}

/// A refusal that is not about a ceiling carries no wait, or a client would
/// retry a request that will never succeed.
#[tokio::test]
async fn a_refusal_that_is_not_a_ceiling_carries_no_retry_after() {
    let app = app_with(ServerConfig::default()).await;
    let res = send(&app, post("/v1/ingest", "application/json", "\"nonsense\"")).await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "{}", res.text());
    assert_eq!(res.header("retry-after"), None, "a 400 is not worth retrying");
}
