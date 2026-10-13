//! Judged scoring against a judge that answers the way real ones do.
//!
//! The judge is a small HTTP server on a port the OS assigns, scripted per
//! test to answer each call quickly, slowly, never, or with far more than a
//! verdict. Every case reads back what the store kept, because a judged score
//! is paid for when the model answers, and the defects here were all about
//! paid work that was not kept or a call that was not bounded.

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use axum::Router;
use http_body_util::BodyExt;
use mcpd_server::judge::JudgeConfig;
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::{ScoreQuery, Store};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tower::ServiceExt;

// ------------------------------------------------------------- fixtures ---

fn temp_db() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    // The pid and a per-process stamp, for the reason `tests/http.rs` gives.
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir =
        std::env::temp_dir().join(format!("mcpd-judging-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

/// What the judge does with one call.
#[derive(Clone, Copy)]
enum Act {
    /// Answer at once with a verdict.
    Answer,
    /// Answer with a verdict after this long.
    After(Duration),
    /// Never answer.
    Hang,
    /// Answer with a valid verdict padded past any sane size.
    Huge,
}

#[derive(Default)]
struct Judge {
    /// One act per call; once spent, every call is answered at once.
    script: VecDeque<Act>,
    calls: usize,
}

type Shared = Arc<Mutex<Judge>>;

fn verdict(reason: &str) -> Value {
    let content = json!({ "value": 0.8, "reason": reason }).to_string();
    json!({ "choices": [{ "message": { "content": content } }] })
}

async fn grade(State(judge): State<Shared>) -> (StatusCode, String) {
    let act = {
        let mut j = judge.lock().expect("the judge lock");
        j.calls += 1;
        j.script.pop_front().unwrap_or(Act::Answer)
    };
    match act {
        Act::Answer => {}
        Act::After(wait) => tokio::time::sleep(wait).await,
        Act::Hang => std::future::pending::<()>().await,
        Act::Huge => return (StatusCode::OK, verdict(&"x".repeat(2 * 1024 * 1024)).to_string()),
    }
    (StatusCode::OK, verdict("it answered the question").to_string())
}

async fn judge(script: &[Act]) -> (String, Shared) {
    let shared: Shared =
        Arc::new(Mutex::new(Judge { script: script.iter().copied().collect(), calls: 0 }));
    let app =
        Router::new().route("/v1/chat/completions", post(grade)).with_state(Arc::clone(&shared));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind a port");
    let addr = listener.local_addr().expect("the bound address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}/v1"), shared)
}

fn judge_config(endpoint: &str, timeout_seconds: u64) -> JudgeConfig {
    serde_json::from_value(json!({
        "endpoint": endpoint,
        "model": "a-judge",
        "criteria": { "relevance": "Does the answer address the question?" },
        "timeout_seconds": timeout_seconds
    }))
    .expect("a judge block from its config shape")
}

/// A daemon judging with `judge`, and `runs` stored runs that each have an
/// answer to grade. Returns the router, the state and the run ids.
async fn daemon(
    judge: JudgeConfig,
    request_timeout_seconds: u64,
    runs: u32,
) -> (Router, Arc<AppState>, Vec<String>) {
    let store = SqliteStore::open(temp_db(), 2).expect("open the store");
    store.migrate().await.expect("migrate the store");
    let config =
        ServerConfig { judge: Some(judge), request_timeout_seconds, ..ServerConfig::default() };
    let state = Arc::new(AppState::new(Arc::new(store) as Arc<dyn Store>, config));
    let app = router(Arc::clone(&state));

    let ids: Vec<String> = (1..=runs).map(|i| format!("{i:032x}")).collect();
    let spans: Vec<Value> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            json!({
                "trace_id": id,
                "span_id": format!("{:016x}", i + 1),
                "name": "answer",
                "kind": "agent_run",
                "status": "ok",
                "start_time": "2026-01-01T00:00:00Z",
                "end_time": "2026-01-01T00:00:01Z",
                "is_root": true,
                "is_event": false,
                "input": "where is my order?",
                "output": "it ships tomorrow",
                "correlation": { "project": "default" },
                "received_at": "2026-01-01T00:00:01Z"
            })
        })
        .collect();
    let res = send(&app, "/v1/ingest", &json!({ "spans": spans })).await;
    assert_eq!(res.0, StatusCode::OK, "{}", res.1);
    (app, state, ids)
}

async fn send(app: &Router, uri: &str, body: &Value) -> (StatusCode, Value) {
    let body = body.to_string();
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("content-length", body.len().to_string())
        .body(Body::from(body))
        .expect("build the request");
    let res = app.clone().oneshot(req).await.expect("the router answered");
    let status = res.status();
    let bytes = res.into_body().collect().await.expect("read the body").to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn stored_scores(state: &AppState) -> usize {
    state
        .store
        .list_scores(&ScoreQuery {
            project: "default".to_owned(),
            limit: 100,
            ..Default::default()
        })
        .await
        .expect("list the scores")
        .items
        .len()
}

fn score_request(ids: &[String]) -> Value {
    json!({ "trace_ids": ids, "scorers": ["relevance"] })
}

// ------------------------------------------------------------- the cases ---

/// The judge's wait was the forwarding client's ten seconds, whatever the
/// judge block said. The configured number is the one a call is held to now.
#[tokio::test]
async fn a_call_is_held_to_the_configured_timeout_and_the_decline_names_the_key() {
    let (url, _judge) = judge(&[Act::After(Duration::from_secs(4))]).await;
    let (app, state, ids) = daemon(judge_config(&url, 1), 30, 1).await;

    let started = Instant::now();
    let (status, body) = send(&app, "/v1/api/score", &score_request(&ids)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(started.elapsed() < Duration::from_secs(3), "the call outlived its timeout");
    let reason = body["skipped"][0]["reason"].as_str().unwrap_or_default();
    assert!(reason.contains("judge.timeout_seconds"), "the key is not named: {body}");
    assert_eq!(stored_scores(&state).await, 0);
}

/// A slow model inside a longer configured timeout is graded rather than
/// declined at ten seconds.
#[tokio::test]
async fn a_judge_slower_than_the_old_fixed_wait_is_graded_when_the_timeout_allows() {
    let (url, _judge) = judge(&[Act::After(Duration::from_millis(1_500))]).await;
    let (app, state, ids) = daemon(judge_config(&url, 5), 30, 1).await;
    let (status, body) = send(&app, "/v1/api/score", &score_request(&ids)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["scores_written"], json!(1), "{body}");
    assert_eq!(stored_scores(&state).await, 1);
}

/// The reply was read whole with no limit.
#[tokio::test]
async fn a_reply_past_the_size_cap_is_declined_and_not_read_whole() {
    let (url, _judge) = judge(&[Act::Huge]).await;
    let (app, state, ids) = daemon(judge_config(&url, 10), 30, 1).await;
    let (status, body) = send(&app, "/v1/api/score", &score_request(&ids)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let reason = body["skipped"][0]["reason"].as_str().unwrap_or_default();
    assert!(reason.contains("1048576 bytes"), "the cap is not named: {body}");
    assert_eq!(stored_scores(&state).await, 0, "an oversized reply produced a score");
}

/// Twenty runs at up to thirty seconds a call cannot fit in a thirty second
/// deadline. That request used to run until the deadline cancelled it and
/// store nothing; it is refused up front, naming the cap and both settings.
#[tokio::test]
async fn a_judged_request_over_what_fits_in_the_deadline_is_refused_naming_the_cap() {
    let (url, judge) = judge(&[]).await;
    let (app, state, ids) = daemon(judge_config(&url, 10), 30, 4).await;

    let (status, body) = send(&app, "/v1/api/score", &score_request(&ids)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["field"], json!("trace_ids"), "{body}");
    let message = body["message"].as_str().unwrap_or_default();
    for needed in ["at most 3 run(s)", "asks for 4", "judge.timeout_seconds", "--request-timeout"] {
        assert!(message.contains(needed), "{needed:?} is missing from: {message}");
    }
    assert_eq!(judge.lock().expect("the judge lock").calls, 0, "a refused request paid for calls");

    // The window form is held to the same cap, through `limit`.
    let (status, body) =
        send(&app, "/v1/api/score", &json!({ "limit": 4, "scorers": ["relevance"] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["field"], json!("limit"), "{body}");

    // At the cap the request runs.
    let (status, body) = send(&app, "/v1/api/score", &score_request(&ids[..3])).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(stored_scores(&state).await, 3);
}

/// A pass that names no runs and sets no `limit` asked for "the newest runs",
/// and is given as many as fit rather than a 400 for a number it never chose.
#[tokio::test]
async fn a_judged_pass_with_no_limit_grades_the_newest_runs_that_fit() {
    let (url, judge) = judge(&[]).await;
    let (app, state, _ids) = daemon(judge_config(&url, 10), 30, 4).await;

    let (status, body) = send(&app, "/v1/api/score", &json!({ "scorers": ["relevance"] })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["traces_scored"], json!(3), "{body}");
    assert_eq!(stored_scores(&state).await, 3);
    assert_eq!(judge.lock().expect("the judge lock").calls, 3);
}

/// When not even one run fits, every form of the request is refused.
#[tokio::test]
async fn a_judged_pass_that_cannot_fit_one_run_is_refused() {
    let (url, judge) = judge(&[]).await;
    let (app, _state, _ids) = daemon(judge_config(&url, 60), 30, 4).await;
    let (status, body) = send(&app, "/v1/api/score", &json!({ "scorers": ["relevance"] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["message"].as_str().unwrap_or_default().contains("not even one run fits"));
    assert_eq!(judge.lock().expect("the judge lock").calls, 0);
}

/// Deterministic scorers cost nothing per run and keep their limits.
#[tokio::test]
async fn a_deterministic_pass_is_not_held_to_the_judged_cap() {
    let (url, _judge) = judge(&[]).await;
    let (app, _state, ids) = daemon(judge_config(&url, 30), 30, 4).await;
    let (status, body) =
        send(&app, "/v1/api/score", &json!({ "trace_ids": ids, "scorers": ["no_errors"] })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["traces_scored"], json!(4), "{body}");
}

/// The request deadline cancels the handler where it stands. Every score
/// was written at the end, so a pass cut short stored nothing, and the calls
/// it had made were paid for and lost. Each judged score is kept as soon as
/// it exists now. The cancellation is driven here the way the deadline layer
/// drives it: by dropping the future mid-call.
#[tokio::test]
async fn a_pass_cut_short_keeps_every_judged_score_it_finished() {
    let (url, judge) = judge(&[Act::Answer, Act::Hang]).await;
    let (_app, state, ids) = daemon(judge_config(&url, 10), 30, 2).await;

    let req: mcpd_server::eval::ScoreRunRequest =
        serde_json::from_value(score_request(&ids)).expect("a score request");
    let cut =
        tokio::time::timeout(Duration::from_secs(2), mcpd_server::eval::run_scorers(&state, req))
            .await;
    assert!(cut.is_err(), "the second call was meant to hang until the pass was cut short");
    assert_eq!(judge.lock().expect("the judge lock").calls, 2);
    assert_eq!(stored_scores(&state).await, 1, "the score the first call paid for was lost");
}

/// Two runs at one second a call fit a two second deadline exactly, by the
/// cap's arithmetic, and left nothing for the rest of the request. A judge that
/// never answered took the whole two seconds, and the client got a 503 from
/// the deadline instead of the two declines. Each call is held to what the
/// deadline leaves now, so the request answers inside it and says why.
#[tokio::test]
async fn calls_that_fill_the_deadline_are_declined_inside_it_rather_than_a_503() {
    let (url, judge) = judge(&[Act::Hang, Act::Hang]).await;
    let (app, state, ids) = daemon(judge_config(&url, 1), 2, 2).await;

    let started = Instant::now();
    let (status, body) = send(&app, "/v1/api/score", &score_request(&ids)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(started.elapsed() < Duration::from_secs(2), "answered at {:?}", started.elapsed());
    let skipped = body["skipped"].as_array().cloned().unwrap_or_default();
    assert_eq!(skipped.len(), 2, "{body}");
    for (entry, id) in skipped.iter().zip(&ids) {
        assert_eq!(entry["scorer_id"], json!("relevance"), "{body}");
        assert_eq!(entry["trace_id"], json!(id), "{body}");
    }
    let last = skipped[1]["reason"].as_str().unwrap_or_default();
    assert!(last.contains("--request-timeout-seconds"), "the deadline is not named: {last}");
    assert!(judge.lock().expect("the judge lock").calls >= 1);
    assert_eq!(stored_scores(&state).await, 0);
}

/// With less time left than a call needs, no call is made at all: it would be
/// paid for and cut off before it could answer.
#[tokio::test]
async fn a_call_with_no_time_left_is_declined_without_being_made() {
    let (url, judge) = judge(&[]).await;
    let (_app, state, ids) = daemon(judge_config(&url, 10), 30, 1).await;
    let detail = state
        .store
        .get_trace("default", &ids[0], &mcpd_store::Include::default())
        .await
        .expect("the stored run");
    let target = mcpd_core::scoring::Target { spans: &detail.spans };
    let declined = mcpd_server::judge::grade(
        &state,
        "relevance",
        &target,
        &ids[0],
        detail.trace.correlation.clone(),
        time::OffsetDateTime::now_utc(),
        Instant::now(),
    )
    .await
    .expect_err("there was no time left to grade");
    let reason = declined.to_string();
    assert!(reason.contains("left no time to call the judge"), "{reason}");
    assert!(reason.contains("--request-timeout-seconds"), "{reason}");
    assert_eq!(judge.lock().expect("the judge lock").calls, 0, "a call was made with no time");
}

/// The decline quoted the client's error, which repeats the whole URL, and a
/// judge endpoint is where a key in a query string is most likely to be. The
/// reason is handed to anyone with a write key, and kept with the pass.
#[tokio::test]
async fn an_unreachable_judge_is_declined_without_its_url() {
    // A port that was free a moment ago refuses the connection at once.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("a free port")
        .port();
    let endpoint = format!("http://127.0.0.1:{port}/v1?api-key=s3cr3t-token");
    let (app, _state, ids) = daemon(judge_config(&endpoint, 5), 30, 1).await;
    let (status, body) = send(&app, "/v1/api/score", &score_request(&ids)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let reason = body["skipped"][0]["reason"].as_str().unwrap_or_default();
    assert!(reason.contains("could not be reached"), "{body}");
    assert!(!body.to_string().contains("s3cr3t-token"), "the key was handed back: {body}");
}

// ------------------------------------------------ the judged scorer library ---

/// A judge that answers the analyze step of a rubric: busy once, then a list
/// of itemised verdicts with the token counts a real endpoint reports.
async fn rubric_judge() -> (String, Arc<Mutex<usize>>) {
    let calls = Arc::new(Mutex::new(0usize));
    let counted = Arc::clone(&calls);
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let counted = Arc::clone(&counted);
            async move {
                let n = {
                    let mut c = counted.lock().expect("the call count");
                    *c += 1;
                    *c
                };
                if n == 1 {
                    return (StatusCode::SERVICE_UNAVAILABLE, "busy".to_owned());
                }
                let content = json!({ "items": [
                    { "item": "it ships tomorrow", "verdict": "yes", "reason": "addresses it" },
                    { "item": "have a nice day", "verdict": "no", "reason": "off topic" }
                ]})
                .to_string();
                let body = json!({
                    "choices": [{ "message": { "content": content } }],
                    "usage": { "prompt_tokens": 1000, "completion_tokens": 100, "total_tokens": 1100 }
                });
                (StatusCode::OK, body.to_string())
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind a port");
    let addr = listener.local_addr().expect("the bound address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}/v1"), calls)
}

/// A shipped rubric needs no criterion in the config: a judge block is
/// enough. The score is counted from the verdicts, keeps them, is priced with
/// the daemon's book, and a busy endpoint is tried again.
#[tokio::test]
async fn a_shipped_rubric_is_graded_in_steps_and_priced() {
    let (url, calls) = rubric_judge().await;
    let mut cfg = judge_config(&url, 5);
    cfg.model = "gpt-4o".to_owned();
    let (app, state, ids) = daemon(cfg, 30, 1).await;
    let req = json!({ "trace_ids": ids, "scorers": ["answer_relevance"] });
    let (status, body) = send(&app, "/v1/api/score", &req).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["scores_written"], json!(1), "{body}");
    assert_eq!(*calls.lock().expect("the call count"), 2, "the busy reply was not retried");
    let score = &body["scores"][0];
    assert_eq!(score["value"], json!(0.5));
    assert!(score["reason"].as_str().unwrap_or_default().contains("have a nice day"), "{score}");
    assert_eq!(score["metadata"]["steps"]["analyze"]["items"].as_array().map(Vec::len), Some(2));
    assert_eq!(score["metadata"]["judge"]["attempts"], json!(2));
    assert!(score["cost"]["total_cost"].as_f64().is_some_and(|c| c > 0.0), "{score}");
    assert_eq!(stored_scores(&state).await, 1);
}

/// A rubric that needs retrieved context declines at the first step, naming
/// it, and calls no model.
#[tokio::test]
async fn a_rubric_without_its_context_declines_at_preprocess_without_a_call() {
    let (url, calls) = rubric_judge().await;
    let (app, _state, ids) = daemon(judge_config(&url, 5), 30, 1).await;
    let req = json!({ "trace_ids": ids, "scorers": ["faithfulness"] });
    let (status, body) = send(&app, "/v1/api/score", &req).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let reason = body["skipped"][0]["reason"].as_str().unwrap_or_default();
    assert!(reason.starts_with("declined at preprocess"), "{body}");
    assert_eq!(*calls.lock().expect("the call count"), 0);
}

/// Without a judge block, naming a rubric is the request's mistake.
#[tokio::test]
async fn a_rubric_with_no_judge_configured_is_refused_with_the_fix() {
    let store = SqliteStore::open(temp_db(), 2).expect("open the store");
    store.migrate().await.expect("migrate the store");
    let state = Arc::new(AppState::new(Arc::new(store) as Arc<dyn Store>, ServerConfig::default()));
    let app = router(Arc::clone(&state));
    let req = json!({ "trace_ids": ["0".repeat(32)], "scorers": ["toxicity"] });
    let (status, body) = send(&app, "/v1/api/score", &req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("judge"), "{body}");
}
