//! Per-endpoint query coverage for the four list endpoints.
//!
//! `http.rs` proves the routes are reachable and refuse the wrong key. This
//! file proves the part a reader actually drives: that a filter narrows to the
//! right rows and not merely to fewer, that a sort orders correctly and stays
//! deterministic across ties, and that paging a cursor to the end returns every
//! row exactly once.
//!
//! Every request goes through `mcpd_server::router`, so the parsing in
//! `query.rs`, the SQL in the store and the error mapping are all under test.
//!
//! The fixture is one batch, described in `SEED DATA` below. It is deliberately
//! small enough to hold in your head and asymmetric enough that a broken filter
//! cannot pass by returning everything.

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

// ------------------------------------------------------------ seed data ---
//
// SEED DATA, project `default`. Six runs, nine spans, eight log lines and six
// scores. The two `checkout` runs share a start time, a duration and a cost so
// that every tie-break has something to break.
//
// run                   root name    kind           status     start     dur    cost  spans
// CHECKOUT_OK           checkout     agent_run      ok         00:00:00  1000   0.25  2
// CHECKOUT_TWIN         checkout     agent_run      ok         00:00:00  1000   0.25  1
// CHECKOUT_FAILED       checkout     agent_run      error      00:01:00  5000   0.5   1
// SUMMARISE_OK          summarise    workflow_run   ok         00:02:00  3000   0.75  3
// SUMMARISE_CANCELLED   summarise    workflow_run   cancelled  00:03:00  2000   1.0   1
// INGEST_OK             ingest_docs  rag_ingestion  ok         00:04:00  4000   none  1
//
// A seventh run lives in project `other`, so every endpoint has something it
// must not leak.

/// The day every timestamp in the fixture falls on.
const DAY: &str = "2026-03-01";

const CHECKOUT_OK: &str = "a0000000000000000000000000000001";
const CHECKOUT_FAILED: &str = "b0000000000000000000000000000002";
const SUMMARISE_OK: &str = "c0000000000000000000000000000003";
const SUMMARISE_CANCELLED: &str = "d0000000000000000000000000000004";
const INGEST_OK: &str = "e0000000000000000000000000000005";
/// Started at the same instant as [`CHECKOUT_OK`], for the same duration, at the
/// same cost. Every ordering test that matters runs through this pair.
const CHECKOUT_TWIN: &str = "f0000000000000000000000000000006";
const OTHER_RUN: &str = "9000000000000000000000000000000a";

const CHECKOUT_ROOT: &str = "a000000000000001";
const CHECKOUT_MODEL: &str = "a000000000000002";
const FAILED_ROOT: &str = "b000000000000001";
const SUMMARISE_ROOT: &str = "c000000000000001";
const SUMMARISE_MODEL: &str = "c000000000000002";
const SUMMARISE_TOOL: &str = "c000000000000003";
const CANCELLED_ROOT: &str = "d000000000000001";
const INGEST_ROOT: &str = "e000000000000001";
const TWIN_ROOT: &str = "f000000000000001";
const OTHER_ROOT: &str = "9000000000000001";

/// Every run in `default`, newest start first, ties broken on the id.
const ALL_RUNS: &[&str] =
    &[INGEST_OK, SUMMARISE_CANCELLED, SUMMARISE_OK, CHECKOUT_FAILED, CHECKOUT_TWIN, CHECKOUT_OK];

/// Every span in `default`, in the same order the list endpoint defaults to.
const ALL_SPANS: &[&str] = &[
    INGEST_ROOT,
    CANCELLED_ROOT,
    SUMMARISE_TOOL,
    SUMMARISE_MODEL,
    SUMMARISE_ROOT,
    FAILED_ROOT,
    CHECKOUT_MODEL,
    TWIN_ROOT,
    CHECKOUT_ROOT,
];

/// Every log line in `default`, newest first.
const ALL_LOGS: &[&str] = &["log-8", "log-7", "log-6", "log-5", "log-4", "log-3", "log-2", "log-1"];

/// Every score in `default`, newest first.
const ALL_SCORES: &[&str] = &["score-6", "score-5", "score-4", "score-3", "score-2", "score-1"];

// ------------------------------------------------------------- fixtures ---
//
// `temp_db`, `app_and_store`, `app_with`, `Res`, `send`, `get` and `post` are
// copies of the helpers in `http.rs`. They are duplicated rather than shared
// because Cargo compiles each integration test as its own crate, and lifting
// them into a shared module would mean editing a file this change does not own.

/// A private database file per test.
///
/// Not `:memory:`, deliberately: `SqliteStore::open` gives every in-memory
/// store the same shared-cache URI, so two tests running on two threads would
/// share one database and race on `migrate`.
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
    let dir = std::env::temp_dir().join(format!("mcpd-query-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

async fn app_and_store(config: ServerConfig) -> (Router, Arc<SqliteStore>) {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let app = router(Arc::new(AppState::new(Arc::clone(&store) as Arc<dyn Store>, config)));
    (app, store)
}

async fn app_with(config: ServerConfig) -> Router {
    app_and_store(config).await.0
}

/// One span, with only the fields the query surface reads.
///
/// `received_at` is fixed rather than "now" so nothing in this file depends on
/// the clock, and no cost is ever computed from the price book: every cost here
/// is stated, so the numbers a sort is checked against are the numbers seeded.
#[allow(clippy::too_many_arguments)]
fn span(
    trace_id: &str,
    span_id: &str,
    parent: Option<&str>,
    name: &str,
    kind: &str,
    status: &str,
    start: &str,
    end: &str,
    extra: Value,
) -> Value {
    let mut v = json!({
        "trace_id": trace_id,
        "span_id": span_id,
        "name": name,
        "kind": kind,
        "status": status,
        "start_time": format!("{DAY}T{start}Z"),
        "end_time": format!("{DAY}T{end}Z"),
        "is_root": parent.is_none(),
        "is_event": false,
        "correlation": { "project": "default" },
        "received_at": format!("{DAY}T00:10:00Z"),
    });
    if let Some(p) = parent {
        v["parent_span_id"] = json!(p);
    }
    let Value::Object(extra) = extra else { panic!("the extra fields must be an object") };
    for (k, val) in extra {
        // A nested key is merged rather than replaced, so a caller can add one
        // correlation field without restating the project.
        match (v.get_mut(&k), &val) {
            (Some(Value::Object(dst)), Value::Object(src)) => {
                dst.extend(src.clone());
            }
            _ => {
                v[k] = val;
            }
        }
    }
    v
}

fn log_line(
    id: &str,
    trace: Option<&str>,
    span_id: Option<&str>,
    level: &str,
    at: &str,
    message: &str,
) -> Value {
    let mut v = json!({
        "log_id": id,
        "timestamp": format!("{DAY}T{at}Z"),
        "level": level,
        "message": message,
        "correlation": { "project": "default" },
        "received_at": format!("{DAY}T00:10:00Z"),
    });
    if let Some(t) = trace {
        v["trace_id"] = json!(t);
    }
    if let Some(s) = span_id {
        v["span_id"] = json!(s);
    }
    v
}

fn score(
    id: &str,
    trace: &str,
    scorer: &str,
    source: &str,
    value: f64,
    at: &str,
    experiment: Option<&str>,
) -> Value {
    let mut correlation = json!({ "project": "default" });
    if let Some(e) = experiment {
        correlation["experiment_id"] = json!(e);
    }
    json!({
        "score_id": id,
        "trace_id": trace,
        "scorer_id": scorer,
        "source": source,
        "value": value,
        "timestamp": format!("{DAY}T{at}Z"),
        "correlation": correlation,
    })
}

/// The whole fixture, as one native ingest batch.
///
/// Every value a filter test asserts on is set here and nowhere else, so a
/// failing test can be read against this one function.
fn seed_batch() -> Value {
    json!({
        "spans": [
            span(CHECKOUT_OK, CHECKOUT_ROOT, None, "checkout", "agent_run", "ok",
                 "00:00:00", "00:00:01", json!({
                "tags": ["paid", "eu"],
                "correlation": {
                    "environment": "production", "session_id": "sess-alpha",
                    "thread_id": "thread-1", "user_id": "user-ann"
                }
            })),
            span(CHECKOUT_OK, CHECKOUT_MODEL, Some(CHECKOUT_ROOT), "generate",
                 "model_generation", "ok", "00:00:00.200", "00:00:00.800", json!({
                "model": "gpt-4o",
                "cost": { "currency": "USD", "total_cost": 0.25, "is_estimated": false },
                "input": { "prompt": "refund the platinum order" },
                "output": { "text": "the platinum refund is queued" },
                "correlation": { "environment": "production", "session_id": "sess-alpha" }
            })),
            span(CHECKOUT_FAILED, FAILED_ROOT, None, "checkout", "agent_run", "error",
                 "00:01:00", "00:01:05", json!({
                "tags": ["paid"],
                "model": "gpt-4o",
                "cost": { "currency": "USD", "total_cost": 0.5, "is_estimated": false },
                "correlation": {
                    "environment": "production", "session_id": "sess-alpha",
                    "thread_id": "thread-2", "user_id": "user-bob"
                }
            })),
            span(SUMMARISE_OK, SUMMARISE_ROOT, None, "summarise", "workflow_run", "ok",
                 "00:02:00", "00:02:03", json!({
                "tags": ["free", "eu"],
                "correlation": {
                    "environment": "staging", "session_id": "sess-beta",
                    "thread_id": "thread-1", "user_id": "user-ann"
                }
            })),
            span(SUMMARISE_OK, SUMMARISE_MODEL, Some(SUMMARISE_ROOT), "generate",
                 "model_generation", "ok", "00:02:00.500", "00:02:01.500", json!({
                "model": "claude-3-5",
                "cost": { "currency": "USD", "total_cost": 0.75, "is_estimated": false },
                "input": { "prompt": "summarise the quarterly report" },
                "correlation": { "environment": "staging", "session_id": "sess-beta" }
            })),
            span(SUMMARISE_OK, SUMMARISE_TOOL, Some(SUMMARISE_ROOT), "fetch_report",
                 "tool_call", "ok", "00:02:01.500", "00:02:02", json!({
                "correlation": { "environment": "staging", "session_id": "sess-beta" }
            })),
            span(SUMMARISE_CANCELLED, CANCELLED_ROOT, None, "summarise", "workflow_run",
                 "cancelled", "00:03:00", "00:03:02", json!({
                "cost": { "currency": "USD", "total_cost": 1.0, "is_estimated": false },
                "correlation": {
                    "environment": "staging", "session_id": "sess-beta",
                    "thread_id": "thread-3", "user_id": "user-cara"
                }
            })),
            span(INGEST_OK, INGEST_ROOT, None, "ingest_docs", "rag_ingestion", "ok",
                 "00:04:00", "00:04:04", json!({
                "tags": ["free"],
                "correlation": {
                    "environment": "production", "session_id": "sess-gamma",
                    "thread_id": "thread-3", "user_id": "user-cara"
                }
            })),
            span(CHECKOUT_TWIN, TWIN_ROOT, None, "checkout", "agent_run", "ok",
                 "00:00:00", "00:00:01", json!({
                "tags": ["paid"],
                "model": "gpt-4o-mini",
                "cost": { "currency": "USD", "total_cost": 0.25, "is_estimated": false },
                "correlation": {
                    "environment": "production", "session_id": "sess-alpha",
                    "thread_id": "thread-1", "user_id": "user-ann"
                }
            })),
            // The other tenant. Every field here is one a `default` filter also
            // matches, so a query that forgets the project returns it.
            span(OTHER_RUN, OTHER_ROOT, None, "checkout", "agent_run", "ok",
                 "00:06:00", "00:06:01", json!({
                "tags": ["paid", "eu"],
                "model": "gpt-4o",
                "correlation": {
                    "project": "other", "environment": "production",
                    "session_id": "sess-alpha", "thread_id": "thread-1",
                    "user_id": "user-ann"
                }
            })),
        ],
        "logs": [
            log_line("log-1", Some(CHECKOUT_OK), Some(CHECKOUT_ROOT), "debug",
                     "00:00:00.100", "opening the cart"),
            log_line("log-2", Some(CHECKOUT_OK), Some(CHECKOUT_MODEL), "info",
                     "00:00:00.200", "calling the model"),
            log_line("log-3", Some(CHECKOUT_FAILED), Some(FAILED_ROOT), "error",
                     "00:01:00.500", "the payment was declined"),
            log_line("log-4", Some(SUMMARISE_OK), Some(SUMMARISE_ROOT), "warn",
                     "00:02:00.500", "retrying the fetch"),
            log_line("log-5", Some(SUMMARISE_OK), Some(SUMMARISE_TOOL), "info",
                     "00:02:00.500", "second attempt"),
            log_line("log-6", Some(SUMMARISE_CANCELLED), Some(CANCELLED_ROOT), "info",
                     "00:03:00.500", "cancelled by the caller"),
            log_line("log-7", None, None, "trace", "00:04:00.500", "heartbeat"),
            log_line("log-8", Some(INGEST_OK), Some(INGEST_ROOT), "fatal",
                     "00:05:00.500", "the indexer died"),
            json!({
                "log_id": "log-other", "trace_id": OTHER_RUN, "level": "error",
                "message": "the payment was declined",
                "timestamp": format!("{DAY}T00:06:00.500Z"),
                "correlation": { "project": "other" },
                "received_at": format!("{DAY}T00:10:00Z"),
            }),
        ],
        "scores": [
            score("score-1", CHECKOUT_OK, "helpfulness", "live", 0.875, "00:00:05", None),
            score("score-2", CHECKOUT_OK, "toxicity", "live", 0.25, "00:00:06", None),
            score("score-3", CHECKOUT_FAILED, "helpfulness", "offline", 0.125, "00:01:05", None),
            score("score-4", SUMMARISE_OK, "toxicity", "manual", 0.0625, "00:02:05",
                  Some("exp-refund")),
            score("score-5", SUMMARISE_OK, "helpfulness", "experiment", 0.5, "00:02:05",
                  Some("exp-refund")),
            score("score-6", SUMMARISE_CANCELLED, "toxicity", "live", 0.75, "00:03:05",
                  Some("exp-summary")),
            json!({
                "score_id": "score-other", "trace_id": OTHER_RUN,
                "scorer_id": "helpfulness", "source": "live", "value": 0.875,
                "timestamp": format!("{DAY}T00:06:05Z"),
                "correlation": { "project": "other" },
            }),
        ],
    })
}

/// A router whose store holds the whole fixture.
///
/// No keys are configured, so authorisation is out of the way and a failure
/// here is a query failure. `http.rs` owns the authorisation cases.
async fn seeded() -> Router {
    let app = app_with(ServerConfig::default()).await;
    let res = send(&app, post("/v1/ingest", seed_batch().to_string())).await;
    assert_eq!(res.status, StatusCode::OK, "seeding failed: {}", res.text());
    // 10 spans, 9 logs, 7 scores. A short count means a span was rejected for a
    // malformed id and every count below would be quietly wrong.
    assert_eq!(res.json()["accepted"], json!(26), "the fixture did not land: {}", res.text());
    app
}

// -------------------------------------------------------------- plumbing ---

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
    let body = res.into_body().collect().await.expect("read the response body").to_bytes().to_vec();
    Res { status, body }
}

/// No credential: this file configures no keys, so authorisation is out of the
/// way and a failure here is a query failure. `http.rs` owns the key cases.
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

// ------------------------------------------------------- query plumbing ---

/// Build a URI from parts, dropping the empty ones.
///
/// A caller passing an empty filter must not produce `?&limit=1`: an empty pair
/// is not a query string every parser accepts, and a test that fails on its own
/// URI proves nothing.
fn uri(path: &str, parts: &[&str]) -> String {
    let parts: Vec<&str> = parts.iter().copied().filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        path.to_owned()
    } else {
        format!("{path}?{}", parts.join("&"))
    }
}

/// The ids one list endpoint returned, in the order it returned them.
async fn ids_of(app: &Router, uri: &str, list: &str, key: &str) -> Vec<String> {
    let res = send(app, get(uri)).await;
    assert_eq!(res.status, StatusCode::OK, "{uri} failed: {}", res.text());
    let v = res.json();
    let rows = v[list]
        .as_array()
        .unwrap_or_else(|| panic!("{uri} returned no `{list}` array: {}", res.text()));
    rows.iter()
        .map(|r| {
            r[key]
                .as_str()
                .unwrap_or_else(|| panic!("a row from {uri} has no `{key}`: {r}"))
                .to_owned()
        })
        .collect()
}

async fn traces_matching(app: &Router, filter: &str) -> Vec<String> {
    ids_of(app, &uri("/v1/api/traces", &[filter]), "traces", "trace_id").await
}

async fn spans_matching(app: &Router, filter: &str) -> Vec<String> {
    ids_of(app, &uri("/v1/api/spans", &[filter]), "spans", "span_id").await
}

async fn logs_matching(app: &Router, filter: &str) -> Vec<String> {
    ids_of(app, &uri("/v1/api/logs", &[filter]), "logs", "log_id").await
}

async fn scores_matching(app: &Router, filter: &str) -> Vec<String> {
    ids_of(app, &uri("/v1/api/scores", &[filter]), "scores", "score_id").await
}

/// Assert the ids came back exactly, in this order.
fn same(got: &[String], want: &[&str], what: &str) {
    let want: Vec<String> = want.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(got, want.as_slice(), "{what}");
}

/// Assert the same rows came back, whatever order they arrived in.
fn same_set(got: &[String], want: &[&str], what: &str) {
    let mut got = got.to_vec();
    got.sort();
    let mut want: Vec<String> = want.iter().map(|s| (*s).to_owned()).collect();
    want.sort();
    assert_eq!(got, want, "{what}");
}

/// Follow `next_cursor` to the end, returning every id in the order the pages
/// gave them.
///
/// Three invariants are checked on the way, because a cursor bug shows up as
/// one of them long before the collected list looks wrong: a page never carries
/// more rows than `limit`, `has_more` always agrees with `next_cursor`, and the
/// walk terminates. The page guard is not paranoia. A cursor that fails to
/// advance is exactly the failure this exists to catch, and without the guard
/// the test hangs rather than fails.
async fn page_through(
    app: &Router,
    path: &str,
    list: &str,
    key: &str,
    filter: &str,
    limit: u32,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    let limit_param = format!("limit={limit}");

    for page in 0..64 {
        let cursor_param = cursor.map(|c| format!("cursor={c}")).unwrap_or_default();
        let target = uri(path, &[filter, &limit_param, &cursor_param]);
        let res = send(app, get(&target)).await;
        assert_eq!(res.status, StatusCode::OK, "page {page} of {target} failed: {}", res.text());
        let v = res.json();

        let rows = v[list]
            .as_array()
            .unwrap_or_else(|| panic!("page {page} of {target} has no `{list}`: {}", res.text()));
        assert!(
            rows.len() <= limit as usize,
            "page {page} of {target} carried {} rows for limit={limit}",
            rows.len()
        );
        for r in rows {
            out.push(r[key].as_str().expect("a row with an id").to_owned());
        }

        let next = v["page"]["next_cursor"].as_str().map(str::to_owned);
        assert_eq!(
            v["page"]["has_more"],
            json!(next.is_some()),
            "page {page} of {target} disagrees with its own cursor: {}",
            res.text()
        );
        match next {
            None => return out,
            Some(c) => cursor = Some(c),
        }
    }
    panic!("paging {path} with limit={limit} never reached a last page");
}

/// A 400 that names the parameter at fault. Returns the message, for a caller
/// that wants to assert what it says.
async fn refused(app: &Router, target: &str, field: &str) -> String {
    let res = send(app, get(target)).await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "{target} was not refused: {}", res.text());
    let (code, message) = res.error();
    assert_eq!(code, "bad_parameter", "{target} returned the wrong error code");
    assert_eq!(res.json()["field"], json!(field), "{target} must name the parameter at fault");
    message
}

// ------------------------------------------------- traces: what it holds ---

#[tokio::test]
async fn the_trace_list_returns_every_run_in_the_project_newest_first() {
    let app = seeded().await;
    same(&traces_matching(&app, "").await, ALL_RUNS, "the default listing is wrong");
}

#[tokio::test]
async fn the_trace_list_never_returns_another_projects_run() {
    // The other tenant's run matches every filter below on its own merits. The
    // only thing keeping it out is the project.
    let app = seeded().await;
    for filter in ["", "name=checkout", "kind=agent_run", "status=ok", "tags=paid", "model=gpt-4o"]
    {
        let got = traces_matching(&app, filter).await;
        assert!(
            !got.iter().any(|id| id == OTHER_RUN),
            "?{filter} leaked another project's run: {got:?}"
        );
    }
}

#[tokio::test]
async fn naming_a_project_returns_that_projects_runs_and_only_those() {
    let app = seeded().await;
    same(&traces_matching(&app, "project=other").await, &[OTHER_RUN], "project=other is wrong");
}

#[tokio::test]
async fn a_project_with_nothing_in_it_is_an_empty_list_and_not_a_404() {
    let app = seeded().await;
    let res = send(&app, get("/v1/api/traces?project=no-such-team")).await;
    assert_eq!(res.status, StatusCode::OK, "an empty project must not be a 404: {}", res.text());
    assert_eq!(res.json()["traces"], json!([]));
    assert_eq!(res.json()["page"]["has_more"], json!(false));
}

// ---------------------------------------------------- traces: filtering ---

#[tokio::test]
async fn filtering_traces_by_status_returns_the_runs_with_that_outcome() {
    let app = seeded().await;
    same_set(
        &traces_matching(&app, "status=ok").await,
        &[CHECKOUT_OK, CHECKOUT_TWIN, SUMMARISE_OK, INGEST_OK],
        "status=ok is wrong",
    );
    same_set(&traces_matching(&app, "status=error").await, &[CHECKOUT_FAILED], "status=error");
    same_set(
        &traces_matching(&app, "status=cancelled").await,
        &[SUMMARISE_CANCELLED],
        "status=cancelled is wrong",
    );
    same_set(&traces_matching(&app, "status=unset").await, &[], "nothing is unset");
}

#[tokio::test]
async fn filtering_traces_by_kind_returns_the_runs_whose_root_is_that_kind() {
    let app = seeded().await;
    same_set(
        &traces_matching(&app, "kind=agent_run").await,
        &[CHECKOUT_OK, CHECKOUT_TWIN, CHECKOUT_FAILED],
        "kind=agent_run is wrong",
    );
    same_set(
        &traces_matching(&app, "kind=workflow_run").await,
        &[SUMMARISE_OK, SUMMARISE_CANCELLED],
        "kind=workflow_run is wrong",
    );
    // `model_generation` is the kind of a child span, never of a root, so this
    // proves the filter reads the root and not any span in the run.
    same_set(&traces_matching(&app, "kind=model_generation").await, &[], "a child kind matched");
}

#[tokio::test]
async fn filtering_traces_by_name_is_exact_and_not_a_prefix() {
    let app = seeded().await;
    same_set(
        &traces_matching(&app, "name=checkout").await,
        &[CHECKOUT_OK, CHECKOUT_TWIN, CHECKOUT_FAILED],
        "name=checkout is wrong",
    );
    same_set(&traces_matching(&app, "name=check").await, &[], "a prefix must not match");
    same_set(&traces_matching(&app, "name=CHECKOUT").await, &[], "the match must be cased");
}

#[tokio::test]
async fn filtering_traces_by_one_tag_returns_the_runs_carrying_it() {
    let app = seeded().await;
    same_set(
        &traces_matching(&app, "tags=paid").await,
        &[CHECKOUT_OK, CHECKOUT_TWIN, CHECKOUT_FAILED],
        "tags=paid is wrong",
    );
    same_set(&traces_matching(&app, "tags=eu").await, &[CHECKOUT_OK, SUMMARISE_OK], "tags=eu");
}

#[tokio::test]
async fn two_tags_mean_both_and_not_either() {
    // The run that carries only one of the two must not come back. An OR here
    // would return three runs and look plausible.
    let app = seeded().await;
    same_set(&traces_matching(&app, "tags=paid,eu").await, &[CHECKOUT_OK], "tags=paid,eu is wrong");
}

#[tokio::test]
async fn a_tag_nothing_carries_is_an_empty_list() {
    let app = seeded().await;
    same_set(&traces_matching(&app, "tags=enterprise").await, &[], "an unknown tag matched");
}

#[tokio::test]
async fn filtering_traces_by_session_returns_one_conversation() {
    let app = seeded().await;
    same_set(
        &traces_matching(&app, "session_id=sess-alpha").await,
        &[CHECKOUT_OK, CHECKOUT_TWIN, CHECKOUT_FAILED],
        "session_id is wrong",
    );
    same_set(
        &traces_matching(&app, "session_id=sess-gamma").await,
        &[INGEST_OK],
        "session_id=sess-gamma is wrong",
    );
}

#[tokio::test]
async fn filtering_traces_by_thread_returns_one_thread() {
    let app = seeded().await;
    same_set(
        &traces_matching(&app, "thread_id=thread-1").await,
        &[CHECKOUT_OK, CHECKOUT_TWIN, SUMMARISE_OK],
        "thread_id=thread-1 is wrong",
    );
    same_set(
        &traces_matching(&app, "thread_id=thread-3").await,
        &[SUMMARISE_CANCELLED, INGEST_OK],
        "thread_id=thread-3 is wrong",
    );
}

#[tokio::test]
async fn filtering_traces_by_user_works_even_though_the_stored_id_is_hashed() {
    // The stored id is a keyed digest. The filter hashes the query value with
    // the same key, so the plain id a caller knows still finds the runs. Without
    // that, "show me this customer's runs" would silently return nothing.
    let app = seeded().await;
    same_set(
        &traces_matching(&app, "user_id=user-ann").await,
        &[CHECKOUT_OK, CHECKOUT_TWIN, SUMMARISE_OK],
        "user_id=user-ann is wrong",
    );
    same_set(&traces_matching(&app, "user_id=user-bob").await, &[CHECKOUT_FAILED], "user_id");

    // And the digest is not stored in the clear, which is the point of hashing.
    let res = send(&app, get("/v1/api/traces?user_id=user-ann")).await;
    assert!(!res.text().contains("user-ann"), "the plain user id was served back");
}

#[tokio::test]
async fn filtering_traces_by_environment_separates_staging_from_production() {
    let app = seeded().await;
    same_set(
        &traces_matching(&app, "environment=production").await,
        &[CHECKOUT_OK, CHECKOUT_TWIN, CHECKOUT_FAILED, INGEST_OK],
        "environment=production is wrong",
    );
    same_set(
        &traces_matching(&app, "environment=staging").await,
        &[SUMMARISE_OK, SUMMARISE_CANCELLED],
        "environment=staging is wrong",
    );
}

#[tokio::test]
async fn filtering_traces_by_model_matches_a_span_inside_the_run_exactly() {
    // The model lives on a span, not on the run, so this filter has to look
    // inside. `gpt-4o-mini` must not answer `gpt-4o`.
    let app = seeded().await;
    same_set(
        &traces_matching(&app, "model=gpt-4o").await,
        &[CHECKOUT_OK, CHECKOUT_FAILED],
        "model=gpt-4o is wrong",
    );
    same_set(&traces_matching(&app, "model=gpt-4o-mini").await, &[CHECKOUT_TWIN], "model=mini");
    same_set(&traces_matching(&app, "model=claude-3-5").await, &[SUMMARISE_OK], "model=claude");
    same_set(&traces_matching(&app, "model=gpt-4").await, &[], "a prefix of a model matched");
}

#[tokio::test]
async fn the_trace_window_includes_its_start_and_excludes_its_end() {
    let app = seeded().await;
    same_set(
        &traces_matching(&app, "from=2026-03-01T00:02:00Z").await,
        &[SUMMARISE_OK, SUMMARISE_CANCELLED, INGEST_OK],
        "from must include a run starting on the boundary",
    );
    same_set(
        &traces_matching(&app, "to=2026-03-01T00:02:00Z").await,
        &[CHECKOUT_OK, CHECKOUT_TWIN, CHECKOUT_FAILED],
        "to must exclude a run starting on the boundary",
    );
    same_set(
        &traces_matching(&app, "from=2026-03-01T00:01:00Z&to=2026-03-01T00:03:00Z").await,
        &[CHECKOUT_FAILED, SUMMARISE_OK],
        "the two bounds together are wrong",
    );
}

#[tokio::test]
async fn a_relative_window_is_read_as_a_duration_before_now() {
    // The whole fixture is dated in the past, so the last hour holds nothing and
    // a wide enough window holds everything. That is the shorthand working in
    // both directions without pinning the test to a clock.
    let app = seeded().await;
    same_set(&traces_matching(&app, "from=-1h").await, &[], "-1h returned stale rows");
    same_set(&traces_matching(&app, "from=-100000h").await, ALL_RUNS, "-100000h lost rows");
}

#[tokio::test]
async fn min_duration_on_traces_is_inclusive_and_measured_in_milliseconds() {
    let app = seeded().await;
    same_set(
        &traces_matching(&app, "min_duration_ms=3000").await,
        &[CHECKOUT_FAILED, SUMMARISE_OK, INGEST_OK],
        "min_duration_ms=3000 must include the run that lasted exactly 3000ms",
    );
    same_set(
        &traces_matching(&app, "min_duration_ms=3001").await,
        &[CHECKOUT_FAILED, INGEST_OK],
        "min_duration_ms=3001 is wrong",
    );
    same_set(&traces_matching(&app, "min_duration_ms=6000").await, &[], "nothing is that slow");
}

#[tokio::test]
async fn min_cost_on_traces_is_inclusive_and_sums_the_spans() {
    // The cheapest two runs cost 0.25 apiece, carried on different spans: one on
    // a child, one on the root. Both have to be summed to be compared.
    let app = seeded().await;
    same_set(
        &traces_matching(&app, "min_cost=0.5").await,
        &[CHECKOUT_FAILED, SUMMARISE_OK, SUMMARISE_CANCELLED],
        "min_cost=0.5 must include the run that cost exactly 0.5",
    );
    same_set(
        &traces_matching(&app, "min_cost=0.25").await,
        &[CHECKOUT_OK, CHECKOUT_TWIN, CHECKOUT_FAILED, SUMMARISE_OK, SUMMARISE_CANCELLED],
        "min_cost=0.25 is wrong",
    );
    same_set(&traces_matching(&app, "min_cost=2").await, &[], "nothing costs that much");
}

#[tokio::test]
async fn has_score_returns_the_runs_that_scorer_actually_scored() {
    let app = seeded().await;
    same_set(
        &traces_matching(&app, "has_score=helpfulness").await,
        &[CHECKOUT_OK, CHECKOUT_FAILED, SUMMARISE_OK],
        "has_score=helpfulness is wrong",
    );
    same_set(
        &traces_matching(&app, "has_score=toxicity").await,
        &[CHECKOUT_OK, SUMMARISE_OK, SUMMARISE_CANCELLED],
        "has_score=toxicity is wrong",
    );
    same_set(&traces_matching(&app, "has_score=nobody").await, &[], "an unknown scorer matched");
}

#[tokio::test]
async fn score_below_narrows_has_score_to_the_runs_under_the_bound() {
    // Strictly below, so the run scored exactly at the bound stays out. This is
    // the filter someone reaches for to find what regressed, and an off-by-one
    // here hands them a run that passed.
    let app = seeded().await;
    same_set(
        &traces_matching(&app, "has_score=helpfulness&score_below=0.5").await,
        &[CHECKOUT_FAILED],
        "score_below=0.5 is wrong",
    );
    same_set(
        &traces_matching(&app, "has_score=helpfulness&score_below=0.875").await,
        &[CHECKOUT_FAILED, SUMMARISE_OK],
        "score_below=0.875 is wrong",
    );
    same_set(
        &traces_matching(&app, "has_score=helpfulness&score_below=0.1").await,
        &[],
        "nothing scored that low",
    );
}

#[tokio::test]
async fn a_full_text_search_finds_the_run_whose_prompt_holds_the_word() {
    let app = seeded().await;
    same_set(&traces_matching(&app, "q=platinum").await, &[CHECKOUT_OK], "q=platinum is wrong");
    same_set(&traces_matching(&app, "q=quarterly").await, &[SUMMARISE_OK], "q=quarterly is wrong");
    same_set(&traces_matching(&app, "q=nonesuch").await, &[], "an absent word matched");
}

#[tokio::test]
async fn two_trace_filters_together_narrow_to_the_intersection() {
    let app = seeded().await;
    // Three runs are named `checkout`; one of them failed.
    same_set(
        &traces_matching(&app, "name=checkout&status=error").await,
        &[CHECKOUT_FAILED],
        "name and status together are wrong",
    );
    // Three runs are in `sess-alpha`; two of them used a gpt model, and only one
    // used this one.
    same_set(
        &traces_matching(&app, "session_id=sess-alpha&model=gpt-4o-mini").await,
        &[CHECKOUT_TWIN],
        "session and model together are wrong",
    );
    // An intersection that is genuinely empty, rather than one filter winning.
    same_set(
        &traces_matching(&app, "environment=staging&tags=paid").await,
        &[],
        "two filters that share nothing must return nothing",
    );
}

// -------------------------------------------------------- traces: order ---

#[tokio::test]
async fn traces_sort_by_start_time_by_default_newest_first() {
    let app = seeded().await;
    same(&traces_matching(&app, "sort=start_time").await, ALL_RUNS, "sort=start_time is wrong");
    same(&traces_matching(&app, "").await, ALL_RUNS, "the default is not sort=start_time");
}

#[tokio::test]
async fn asking_for_ascending_order_reverses_the_trace_listing() {
    let app = seeded().await;
    let mut ascending: Vec<&str> = ALL_RUNS.to_vec();
    ascending.reverse();
    same(&traces_matching(&app, "order=asc").await, &ascending, "order=asc is wrong");
}

#[tokio::test]
async fn traces_sort_by_duration_slowest_first() {
    let app = seeded().await;
    same(
        &traces_matching(&app, "sort=duration").await,
        &[
            CHECKOUT_FAILED,
            INGEST_OK,
            SUMMARISE_OK,
            SUMMARISE_CANCELLED,
            CHECKOUT_TWIN,
            CHECKOUT_OK,
        ],
        "sort=duration is wrong",
    );
}

#[tokio::test]
async fn traces_sort_by_cost_most_expensive_first() {
    let app = seeded().await;
    same(
        &traces_matching(&app, "sort=cost").await,
        &[
            SUMMARISE_CANCELLED,
            SUMMARISE_OK,
            CHECKOUT_FAILED,
            CHECKOUT_TWIN,
            CHECKOUT_OK,
            INGEST_OK,
        ],
        "sort=cost is wrong",
    );
}

#[tokio::test]
async fn traces_sort_by_span_count_largest_first() {
    let app = seeded().await;
    same(
        &traces_matching(&app, "sort=span_count").await,
        &[
            SUMMARISE_OK,
            CHECKOUT_OK,
            CHECKOUT_TWIN,
            INGEST_OK,
            SUMMARISE_CANCELLED,
            CHECKOUT_FAILED,
        ],
        "sort=span_count is wrong",
    );
}

#[tokio::test]
async fn a_tie_on_every_sort_key_is_broken_on_the_run_id_and_never_dropped() {
    // The two `checkout` runs share a start time, a duration and a cost, and
    // three more runs share a span count. A sort that cannot break a tie either
    // loses a row or returns one twice, and both look like a filter bug.
    let app = seeded().await;
    for sort in ["start_time", "duration", "cost", "span_count"] {
        let got = traces_matching(&app, &format!("sort={sort}")).await;
        same_set(&got, ALL_RUNS, &format!("sort={sort} did not return every run"));

        let mut unique = got.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), got.len(), "sort={sort} returned a run twice: {got:?}");
    }
}

#[tokio::test]
async fn the_tied_pair_keeps_its_relative_order_in_both_directions() {
    // Descending puts the higher id first, ascending puts it last. A tie-break
    // that ignored the direction would put them the same way round both times.
    let app = seeded().await;
    let desc = traces_matching(&app, "sort=cost").await;
    let asc = traces_matching(&app, "sort=cost&order=asc").await;

    let at = |v: &[String], id: &str| v.iter().position(|x| x == id).expect("the run is listed");
    assert!(at(&desc, CHECKOUT_TWIN) < at(&desc, CHECKOUT_OK), "descending: {desc:?}");
    assert!(at(&asc, CHECKOUT_OK) < at(&asc, CHECKOUT_TWIN), "ascending: {asc:?}");
}

// ------------------------------------------------------- traces: paging ---

#[tokio::test]
async fn paging_the_trace_list_in_ones_returns_every_run_exactly_once() {
    let app = seeded().await;
    same(
        &page_through(&app, "/v1/api/traces", "traces", "trace_id", "", 1).await,
        ALL_RUNS,
        "paging in ones is wrong",
    );
}

#[tokio::test]
async fn paging_the_trace_list_in_twos_returns_every_run_exactly_once() {
    let app = seeded().await;
    same(
        &page_through(&app, "/v1/api/traces", "traces", "trace_id", "", 2).await,
        ALL_RUNS,
        "paging in twos is wrong",
    );
}

#[tokio::test]
async fn paging_holds_for_every_sort_and_both_directions() {
    // The cost cursor carries micro-units so it can stay an integer, so it is
    // the one most likely to compare against the wrong scale and hand back page
    // one for ever. Paging every sort in ones and twos is how that shows up.
    let app = seeded().await;
    for sort in ["start_time", "duration", "cost", "span_count"] {
        for order in ["desc", "asc"] {
            let filter = format!("sort={sort}&order={order}");
            let whole = traces_matching(&app, &filter).await;
            for limit in [1, 2, 5] {
                let paged =
                    page_through(&app, "/v1/api/traces", "traces", "trace_id", &filter, limit)
                        .await;
                same(
                    &paged,
                    &whole.iter().map(String::as_str).collect::<Vec<_>>(),
                    &format!("paging {filter} in {limit}s did not match the whole listing"),
                );
            }
        }
    }
}

#[tokio::test]
async fn paging_a_filtered_trace_listing_stays_inside_the_filter() {
    let app = seeded().await;
    let paged =
        page_through(&app, "/v1/api/traces", "traces", "trace_id", "name=checkout", 1).await;
    same_set(&paged, &[CHECKOUT_OK, CHECKOUT_TWIN, CHECKOUT_FAILED], "the filter was lost");
}

#[tokio::test]
async fn the_last_page_of_traces_carries_no_cursor() {
    let app = seeded().await;
    let full = send(&app, get("/v1/api/traces?limit=500")).await;
    assert_eq!(full.json()["page"]["has_more"], json!(false), "{}", full.text());
    assert_eq!(full.json()["page"]["next_cursor"], json!(null), "{}", full.text());

    let partial = send(&app, get("/v1/api/traces?limit=2")).await;
    assert_eq!(partial.json()["page"]["has_more"], json!(true), "{}", partial.text());
    assert!(partial.json()["page"]["next_cursor"].is_string(), "{}", partial.text());
}

// -------------------------------------------------------- spans: reading ---

#[tokio::test]
async fn the_span_list_returns_every_span_in_the_project_newest_first() {
    let app = seeded().await;
    same(&spans_matching(&app, "").await, ALL_SPANS, "the default span listing is wrong");
}

#[tokio::test]
async fn the_span_list_never_returns_another_projects_span() {
    let app = seeded().await;
    for filter in ["", "name=checkout", "kind=agent_run", "status=ok", "model=gpt-4o"] {
        let got = spans_matching(&app, filter).await;
        assert!(!got.iter().any(|id| id == OTHER_ROOT), "?{filter} leaked a span: {got:?}");
    }
    same(&spans_matching(&app, "project=other").await, &[OTHER_ROOT], "project=other is wrong");
}

#[tokio::test]
async fn filtering_spans_by_kind_returns_the_steps_of_that_kind() {
    let app = seeded().await;
    same_set(
        &spans_matching(&app, "kind=agent_run").await,
        &[CHECKOUT_ROOT, TWIN_ROOT, FAILED_ROOT],
        "kind=agent_run is wrong",
    );
    same_set(
        &spans_matching(&app, "kind=model_generation").await,
        &[CHECKOUT_MODEL, SUMMARISE_MODEL],
        "kind=model_generation is wrong",
    );
    same_set(&spans_matching(&app, "kind=tool_call").await, &[SUMMARISE_TOOL], "kind=tool_call");
    same_set(&spans_matching(&app, "kind=guardrail").await, &[], "an unused kind matched");
}

#[tokio::test]
async fn filtering_spans_by_status_returns_the_steps_with_that_outcome() {
    let app = seeded().await;
    same_set(&spans_matching(&app, "status=error").await, &[FAILED_ROOT], "status=error is wrong");
    same_set(
        &spans_matching(&app, "status=cancelled").await,
        &[CANCELLED_ROOT],
        "status=cancelled is wrong",
    );
    // Seven of the nine. A span-level status is not the run's rolled-up status,
    // so the failed run's siblings are still `ok`.
    assert_eq!(spans_matching(&app, "status=ok").await.len(), 7, "status=ok is wrong");
}

#[tokio::test]
async fn filtering_spans_by_name_is_exact() {
    let app = seeded().await;
    same_set(
        &spans_matching(&app, "name=generate").await,
        &[CHECKOUT_MODEL, SUMMARISE_MODEL],
        "name=generate is wrong",
    );
    same_set(&spans_matching(&app, "name=fetch_report").await, &[SUMMARISE_TOOL], "name=fetch");
    same_set(&spans_matching(&app, "name=gener").await, &[], "a prefix must not match");
}

#[tokio::test]
async fn filtering_spans_by_model_is_exact() {
    let app = seeded().await;
    same_set(
        &spans_matching(&app, "model=gpt-4o").await,
        &[CHECKOUT_MODEL, FAILED_ROOT],
        "model=gpt-4o is wrong",
    );
    same_set(&spans_matching(&app, "model=gpt-4o-mini").await, &[TWIN_ROOT], "model=mini");
    same_set(&spans_matching(&app, "model=gpt-4").await, &[], "a prefix of a model matched");
}

#[tokio::test]
async fn filtering_spans_by_trace_returns_that_runs_waterfall() {
    let app = seeded().await;
    same_set(
        &spans_matching(&app, &format!("trace_id={CHECKOUT_OK}")).await,
        &[CHECKOUT_ROOT, CHECKOUT_MODEL],
        "trace_id is wrong",
    );
    same_set(
        &spans_matching(&app, &format!("trace_id={SUMMARISE_OK}")).await,
        &[SUMMARISE_ROOT, SUMMARISE_MODEL, SUMMARISE_TOOL],
        "trace_id is wrong",
    );
    same_set(
        &spans_matching(&app, "trace_id=ffffffffffffffffffffffffffffffff").await,
        &[],
        "an unknown run must be an empty list",
    );
}

#[tokio::test]
async fn the_span_window_includes_its_start_and_excludes_its_end() {
    let app = seeded().await;
    same_set(
        &spans_matching(&app, "from=2026-03-01T00:02:00Z").await,
        &[SUMMARISE_ROOT, SUMMARISE_MODEL, SUMMARISE_TOOL, CANCELLED_ROOT, INGEST_ROOT],
        "from must include a span starting on the boundary",
    );
    same_set(
        &spans_matching(&app, "to=2026-03-01T00:02:00Z").await,
        &[CHECKOUT_ROOT, CHECKOUT_MODEL, TWIN_ROOT, FAILED_ROOT],
        "to must exclude a span starting on the boundary",
    );
}

#[tokio::test]
async fn min_duration_on_spans_is_inclusive() {
    let app = seeded().await;
    same_set(
        &spans_matching(&app, "min_duration_ms=3000").await,
        &[FAILED_ROOT, SUMMARISE_ROOT, INGEST_ROOT],
        "min_duration_ms=3000 must include the span that lasted exactly 3000ms",
    );
    // Only the two sub-second steps drop out.
    assert_eq!(spans_matching(&app, "min_duration_ms=1000").await.len(), 7, "min_duration_ms");
    same_set(&spans_matching(&app, "min_duration_ms=6000").await, &[], "nothing is that slow");
}

#[tokio::test]
async fn searching_spans_finds_the_step_whose_payload_holds_the_word() {
    let app = seeded().await;
    same_set(&spans_matching(&app, "q=platinum").await, &[CHECKOUT_MODEL], "q=platinum is wrong");
    same_set(&spans_matching(&app, "q=quarterly").await, &[SUMMARISE_MODEL], "q=quarterly");
    same_set(&spans_matching(&app, "q=nonesuch").await, &[], "an absent word matched");
}

#[tokio::test]
async fn two_span_filters_together_narrow_to_the_intersection() {
    let app = seeded().await;
    same_set(
        &spans_matching(&app, &format!("trace_id={SUMMARISE_OK}&kind=tool_call")).await,
        &[SUMMARISE_TOOL],
        "trace and kind together are wrong",
    );
    same_set(
        &spans_matching(&app, "kind=agent_run&status=error").await,
        &[FAILED_ROOT],
        "kind and status together are wrong",
    );
    same_set(
        &spans_matching(&app, "kind=tool_call&model=gpt-4o").await,
        &[],
        "two filters that share nothing must return nothing",
    );
}

#[tokio::test]
async fn asking_for_ascending_order_reverses_the_span_listing() {
    let app = seeded().await;
    let mut ascending: Vec<&str> = ALL_SPANS.to_vec();
    ascending.reverse();
    same(&spans_matching(&app, "order=asc").await, &ascending, "order=asc is wrong");
}

#[tokio::test]
async fn paging_the_span_list_returns_every_span_exactly_once() {
    // Two spans share a start time, so this also proves the tie-break survives
    // being cut across a page boundary.
    let app = seeded().await;
    for limit in [1, 2, 4] {
        same(
            &page_through(&app, "/v1/api/spans", "spans", "span_id", "", limit).await,
            ALL_SPANS,
            &format!("paging spans in {limit}s is wrong"),
        );
    }
    let mut ascending: Vec<&str> = ALL_SPANS.to_vec();
    ascending.reverse();
    same(
        &page_through(&app, "/v1/api/spans", "spans", "span_id", "order=asc", 1).await,
        &ascending,
        "paging spans ascending is wrong",
    );
}

// --------------------------------------------------------- logs: reading ---

#[tokio::test]
async fn the_log_list_returns_every_line_in_the_project_newest_first() {
    let app = seeded().await;
    same(&logs_matching(&app, "").await, ALL_LOGS, "the default log listing is wrong");
}

#[tokio::test]
async fn the_log_list_never_returns_another_projects_line() {
    let app = seeded().await;
    for filter in ["", "level=error", "q=declined"] {
        let got = logs_matching(&app, filter).await;
        assert!(!got.iter().any(|id| id == "log-other"), "?{filter} leaked a line: {got:?}");
    }
    same(&logs_matching(&app, "project=other").await, &["log-other"], "project=other is wrong");
}

#[tokio::test]
async fn the_log_level_is_a_minimum_and_not_an_equality() {
    // `level=warn` has to return the errors and the fatal line too. Read as an
    // equality it would return one line and look like a working filter.
    let app = seeded().await;
    same_set(&logs_matching(&app, "level=fatal").await, &["log-8"], "level=fatal is wrong");
    same_set(&logs_matching(&app, "level=error").await, &["log-3", "log-8"], "level=error");
    same_set(
        &logs_matching(&app, "level=warn").await,
        &["log-3", "log-4", "log-8"],
        "level=warn is wrong",
    );
    same_set(
        &logs_matching(&app, "level=info").await,
        &["log-2", "log-3", "log-4", "log-5", "log-6", "log-8"],
        "level=info is wrong",
    );
    same_set(&logs_matching(&app, "level=trace").await, ALL_LOGS, "level=trace must hold them all");
}

#[tokio::test]
async fn filtering_logs_by_trace_returns_one_runs_lines() {
    let app = seeded().await;
    same_set(
        &logs_matching(&app, &format!("trace_id={CHECKOUT_OK}")).await,
        &["log-1", "log-2"],
        "trace_id is wrong",
    );
    same_set(
        &logs_matching(&app, &format!("trace_id={SUMMARISE_OK}")).await,
        &["log-4", "log-5"],
        "trace_id is wrong",
    );
    same_set(
        &logs_matching(&app, "trace_id=ffffffffffffffffffffffffffffffff").await,
        &[],
        "an unknown run must be an empty list",
    );
}

#[tokio::test]
async fn filtering_logs_by_span_narrows_further_than_the_trace_does() {
    let app = seeded().await;
    same_set(
        &logs_matching(&app, &format!("span_id={CHECKOUT_MODEL}")).await,
        &["log-2"],
        "span_id is wrong",
    );
    same_set(
        &logs_matching(&app, &format!("span_id={SUMMARISE_TOOL}")).await,
        &["log-5"],
        "span_id is wrong",
    );
}

#[tokio::test]
async fn the_log_window_includes_its_start_and_excludes_its_end() {
    let app = seeded().await;
    same_set(
        &logs_matching(&app, "from=2026-03-01T00:02:00.500Z").await,
        &["log-4", "log-5", "log-6", "log-7", "log-8"],
        "from must include a line stamped on the boundary",
    );
    same_set(
        &logs_matching(&app, "to=2026-03-01T00:02:00.500Z").await,
        &["log-1", "log-2", "log-3"],
        "to must exclude a line stamped on the boundary",
    );
}

#[tokio::test]
async fn searching_logs_finds_the_line_whose_message_holds_the_word() {
    let app = seeded().await;
    same_set(&logs_matching(&app, "q=declined").await, &["log-3"], "q=declined is wrong");
    same_set(&logs_matching(&app, "q=heartbeat").await, &["log-7"], "q=heartbeat is wrong");
    same_set(&logs_matching(&app, "q=nonesuch").await, &[], "an absent word matched");
}

#[tokio::test]
async fn two_log_filters_together_narrow_to_the_intersection() {
    let app = seeded().await;
    same_set(
        &logs_matching(&app, &format!("trace_id={SUMMARISE_OK}&level=warn")).await,
        &["log-4"],
        "trace and level together are wrong",
    );
    same_set(
        &logs_matching(&app, &format!("trace_id={CHECKOUT_OK}&level=error")).await,
        &[],
        "two filters that share nothing must return nothing",
    );
}

#[tokio::test]
async fn asking_for_ascending_order_reverses_the_log_listing() {
    let app = seeded().await;
    let mut ascending: Vec<&str> = ALL_LOGS.to_vec();
    ascending.reverse();
    same(&logs_matching(&app, "order=asc").await, &ascending, "order=asc is wrong");
}

#[tokio::test]
async fn paging_the_log_list_returns_every_line_exactly_once() {
    // Two lines share a timestamp, so a page boundary lands on a tie.
    let app = seeded().await;
    for limit in [1, 2, 3] {
        same(
            &page_through(&app, "/v1/api/logs", "logs", "log_id", "", limit).await,
            ALL_LOGS,
            &format!("paging logs in {limit}s is wrong"),
        );
    }
    same_set(
        &page_through(&app, "/v1/api/logs", "logs", "log_id", "level=info", 1).await,
        &["log-2", "log-3", "log-4", "log-5", "log-6", "log-8"],
        "paging lost the level filter",
    );
}

// ------------------------------------------------------- scores: reading ---

#[tokio::test]
async fn the_score_list_returns_every_score_in_the_project_newest_first() {
    let app = seeded().await;
    same(&scores_matching(&app, "").await, ALL_SCORES, "the default score listing is wrong");
}

#[tokio::test]
async fn the_score_list_never_returns_another_projects_score() {
    let app = seeded().await;
    for filter in ["", "scorer_id=helpfulness", "source=live", "min_value=0.5"] {
        let got = scores_matching(&app, filter).await;
        assert!(!got.iter().any(|id| id == "score-other"), "?{filter} leaked a score: {got:?}");
    }
    same(&scores_matching(&app, "project=other").await, &["score-other"], "project=other");
}

#[tokio::test]
async fn filtering_scores_by_scorer_returns_that_scorers_results() {
    let app = seeded().await;
    same_set(
        &scores_matching(&app, "scorer_id=helpfulness").await,
        &["score-1", "score-3", "score-5"],
        "scorer_id=helpfulness is wrong",
    );
    same_set(
        &scores_matching(&app, "scorer_id=toxicity").await,
        &["score-2", "score-4", "score-6"],
        "scorer_id=toxicity is wrong",
    );
    same_set(&scores_matching(&app, "scorer_id=nobody").await, &[], "an unknown scorer matched");
}

#[tokio::test]
async fn filtering_scores_by_source_separates_the_four_ways_a_score_arrives() {
    let app = seeded().await;
    same_set(
        &scores_matching(&app, "source=live").await,
        &["score-1", "score-2", "score-6"],
        "source=live is wrong",
    );
    same_set(&scores_matching(&app, "source=offline").await, &["score-3"], "source=offline");
    same_set(&scores_matching(&app, "source=manual").await, &["score-4"], "source=manual");
    same_set(&scores_matching(&app, "source=experiment").await, &["score-5"], "source=experiment");
}

#[tokio::test]
async fn filtering_scores_by_trace_returns_one_runs_scores() {
    let app = seeded().await;
    same_set(
        &scores_matching(&app, &format!("trace_id={CHECKOUT_OK}")).await,
        &["score-1", "score-2"],
        "trace_id is wrong",
    );
    same_set(
        &scores_matching(&app, "trace_id=ffffffffffffffffffffffffffffffff").await,
        &[],
        "an unknown run must be an empty list",
    );
}

#[tokio::test]
async fn filtering_scores_by_experiment_returns_that_experiments_scores() {
    // The reference does not list `experiment_id` among the parameters this
    // endpoint takes, but the handler reads it and it is how a comparison is
    // pulled apart. See the report: the spec is missing the parameter.
    let app = seeded().await;
    same_set(
        &scores_matching(&app, "experiment_id=exp-refund").await,
        &["score-4", "score-5"],
        "experiment_id=exp-refund is wrong",
    );
    same_set(
        &scores_matching(&app, "experiment_id=exp-summary").await,
        &["score-6"],
        "exp-summary",
    );
    same_set(&scores_matching(&app, "experiment_id=exp-none").await, &[], "an unknown experiment");
}

#[tokio::test]
async fn the_score_value_bounds_are_both_inclusive() {
    let app = seeded().await;
    same_set(
        &scores_matching(&app, "min_value=0.5").await,
        &["score-1", "score-5", "score-6"],
        "min_value must include the score sitting exactly on the bound",
    );
    same_set(
        &scores_matching(&app, "max_value=0.5").await,
        &["score-2", "score-3", "score-4", "score-5"],
        "max_value must include the score sitting exactly on the bound",
    );
    same_set(
        &scores_matching(&app, "min_value=0.25&max_value=0.75").await,
        &["score-2", "score-5", "score-6"],
        "the two bounds together are wrong",
    );
    same_set(&scores_matching(&app, "min_value=1").await, &[], "nothing scored that high");
}

#[tokio::test]
async fn the_score_window_includes_its_start_and_excludes_its_end() {
    let app = seeded().await;
    same_set(
        &scores_matching(&app, "from=2026-03-01T00:02:05Z").await,
        &["score-4", "score-5", "score-6"],
        "from must include a score stamped on the boundary",
    );
    same_set(
        &scores_matching(&app, "to=2026-03-01T00:02:05Z").await,
        &["score-1", "score-2", "score-3"],
        "to must exclude a score stamped on the boundary",
    );
}

#[tokio::test]
async fn two_score_filters_together_narrow_to_the_intersection() {
    let app = seeded().await;
    same_set(
        &scores_matching(&app, "scorer_id=helpfulness&source=live").await,
        &["score-1"],
        "scorer and source together are wrong",
    );
    same_set(
        &scores_matching(&app, "scorer_id=toxicity&min_value=0.5").await,
        &["score-6"],
        "scorer and a bound together are wrong",
    );
    same_set(
        &scores_matching(&app, "source=manual&min_value=0.5").await,
        &[],
        "two filters that share nothing must return nothing",
    );
}

#[tokio::test]
async fn paging_the_score_list_returns_every_score_exactly_once() {
    // Two scores share a timestamp, so a page boundary lands on a tie.
    let app = seeded().await;
    for limit in [1, 2, 4] {
        same(
            &page_through(&app, "/v1/api/scores", "scores", "score_id", "", limit).await,
            ALL_SCORES,
            &format!("paging scores in {limit}s is wrong"),
        );
    }
    same_set(
        &page_through(&app, "/v1/api/scores", "scores", "score_id", "scorer_id=helpfulness", 1)
            .await,
        &["score-1", "score-3", "score-5"],
        "paging lost the scorer filter",
    );
}

#[tokio::test]
async fn the_score_list_is_newest_first_and_says_so_rather_than_ignoring_order() {
    // The score list sorts newest first and its cursor compares one way only,
    // so there is no ordering to choose. `order=asc` used to be accepted and
    // dropped, which pinned a listing that quietly ignored the parameter; it is
    // now refused by name. The run and span lists do take `order`, so somebody
    // moving between them has every reason to try it here, and being told is
    // the difference between a two-second answer and reading a page of scores
    // in the belief they are the lowest.
    let app = seeded().await;
    let res = send(&app, get("/v1/api/scores?order=asc")).await;
    assert_eq!(
        res.status,
        StatusCode::BAD_REQUEST,
        "the score list accepted an ordering it does not apply: {}",
        res.text()
    );
    assert_eq!(res.json()["field"], "order");
    // Unasked for, the listing is unchanged and still newest first.
    same(&scores_matching(&app, "").await, ALL_SCORES, "the default listing moved");
}

// ---------------------------------------------- empty results, not 404s ---

#[tokio::test]
async fn a_filter_that_matches_nothing_is_a_200_with_an_empty_list_on_every_endpoint() {
    let app = seeded().await;
    for (path, list, filter) in [
        ("/v1/api/traces", "traces", "name=no-such-run"),
        ("/v1/api/spans", "spans", "name=no-such-step"),
        ("/v1/api/logs", "logs", "trace_id=ffffffffffffffffffffffffffffffff"),
        ("/v1/api/scores", "scores", "scorer_id=no-such-scorer"),
    ] {
        let target = uri(path, &[filter]);
        let res = send(&app, get(&target)).await;
        assert_eq!(res.status, StatusCode::OK, "{target} was not a 200: {}", res.text());
        assert_eq!(res.json()[list], json!([]), "{target} did not return an empty list");
        assert_eq!(res.json()["page"]["has_more"], json!(false), "{target} claims another page");
    }
}

#[tokio::test]
async fn an_empty_store_answers_every_list_endpoint_with_an_empty_list() {
    let app = app_with(ServerConfig::default()).await;
    for (path, list) in [
        ("/v1/api/traces", "traces"),
        ("/v1/api/spans", "spans"),
        ("/v1/api/logs", "logs"),
        ("/v1/api/scores", "scores"),
    ] {
        let res = send(&app, get(path)).await;
        assert_eq!(res.status, StatusCode::OK, "{path} on an empty store: {}", res.text());
        assert_eq!(res.json()[list], json!([]), "{path} did not return an empty list");
    }
}

// ------------------------------------------------ refusing bad parameters ---

#[tokio::test]
async fn an_unknown_status_is_refused_and_lists_the_ones_that_work() {
    let app = seeded().await;
    for path in ["/v1/api/traces", "/v1/api/spans"] {
        let message = refused(&app, &format!("{path}?status=maybe"), "status").await;
        assert!(
            message.contains("ok") && message.contains("cancelled"),
            "{path} should list the statuses, said: {message}"
        );
    }
}

#[tokio::test]
async fn an_unknown_kind_is_refused_and_lists_the_ones_that_work() {
    let app = seeded().await;
    for path in ["/v1/api/traces", "/v1/api/spans"] {
        let message = refused(&app, &format!("{path}?kind=teleport"), "kind").await;
        assert!(
            message.contains("agent_run") && message.contains("tool_call"),
            "{path} should list the kinds, said: {message}"
        );
    }
}

#[tokio::test]
async fn an_unknown_log_level_is_refused_and_lists_the_ones_that_work() {
    let app = seeded().await;
    let message = refused(&app, "/v1/api/logs?level=loud", "level").await;
    assert!(
        message.contains("warn") && message.contains("fatal"),
        "the message should list the levels, said: {message}"
    );
}

#[tokio::test]
async fn an_unknown_score_source_is_refused_and_lists_the_ones_that_work() {
    let app = seeded().await;
    let message = refused(&app, "/v1/api/scores?source=guesswork", "source").await;
    assert!(
        message.contains("manual") && message.contains("experiment"),
        "the message should list the sources, said: {message}"
    );
}

#[tokio::test]
async fn an_unsortable_column_is_refused_and_names_the_parameter() {
    let app = seeded().await;
    let message = refused(&app, "/v1/api/traces?sort=whatever", "sort").await;
    assert!(
        message.contains("start_time") && message.contains("span_count"),
        "the message should list the sortable columns, said: {message}"
    );
}

#[tokio::test]
async fn an_unreadable_limit_names_the_parameter_on_every_endpoint() {
    let app = seeded().await;
    for path in ["/v1/api/traces", "/v1/api/spans", "/v1/api/logs", "/v1/api/scores"] {
        refused(&app, &format!("{path}?limit=lots"), "limit").await;
        // Negative is not merely unreadable, it is a plausible mistake: a client
        // that computes a page size can send one.
        refused(&app, &format!("{path}?limit=-1"), "limit").await;
    }
}

#[tokio::test]
async fn an_unreadable_number_names_the_parameter_it_belongs_to() {
    let app = seeded().await;
    for (target, field) in [
        ("/v1/api/traces?min_duration_ms=fast", "min_duration_ms"),
        ("/v1/api/traces?min_cost=free", "min_cost"),
        ("/v1/api/traces?score_below=low", "score_below"),
        ("/v1/api/spans?min_duration_ms=fast", "min_duration_ms"),
        ("/v1/api/scores?min_value=high", "min_value"),
        ("/v1/api/scores?max_value=high", "max_value"),
    ] {
        refused(&app, target, field).await;
    }
}

#[tokio::test]
async fn an_unreadable_time_names_from_or_to_on_every_endpoint() {
    let app = seeded().await;
    for path in ["/v1/api/traces", "/v1/api/spans", "/v1/api/logs", "/v1/api/scores"] {
        let message = refused(&app, &format!("{path}?from=yesterday"), "from").await;
        assert!(
            message.contains("RFC 3339") || message.contains("duration"),
            "{path} should say what a time looks like, said: {message}"
        );
        refused(&app, &format!("{path}?to=soon"), "to").await;
        // A relative shorthand with a unit nobody uses is the other half of the
        // parser, and it must be refused rather than read as an absolute time.
        let message = refused(&app, &format!("{path}?from=-3w"), "from").await;
        assert!(
            message.contains("unknown unit"),
            "{path} should name the unit as the problem, said: {message}"
        );
        refused(&app, &format!("{path}?from=-manyh"), "from").await;
    }
}

#[tokio::test]
async fn a_mangled_cursor_is_refused_by_name_on_every_endpoint() {
    // A cursor is opaque, so a caller that mangles one has no way to reason
    // about a 500. Three shapes: not hex at all, hex that is not text, and
    // text with no separator in it.
    let app = seeded().await;
    for path in ["/v1/api/traces", "/v1/api/spans", "/v1/api/logs", "/v1/api/scores"] {
        for cursor in ["notacursor", "ff", "6e6f74686578"] {
            let target = format!("{path}?cursor={cursor}");
            let res = send(&app, get(&target)).await;
            assert_eq!(
                res.status,
                StatusCode::BAD_REQUEST,
                "{target} was not refused: {}",
                res.text()
            );
            let (code, message) = res.error();
            assert_eq!(code, "invalid_query", "{target} returned the wrong error code");
            assert!(message.contains("cursor"), "{target} should name the cursor, said: {message}");
        }
    }
}

#[tokio::test]
async fn a_cursor_from_one_endpoint_is_refused_or_ignored_but_never_a_500() {
    // Cursors from different endpoints carry different sort values. Handing one
    // to the wrong endpoint is a client bug, and the answer to it is a 4xx or a
    // page, never a stack trace.
    let app = seeded().await;
    let traces = send(&app, get("/v1/api/traces?limit=1")).await;
    let cursor = traces.json()["page"]["next_cursor"].as_str().expect("a cursor").to_owned();

    for path in ["/v1/api/spans", "/v1/api/logs", "/v1/api/scores"] {
        let res = send(&app, get(&format!("{path}?cursor={cursor}"))).await;
        assert!(
            res.status == StatusCode::OK || res.status.is_client_error(),
            "{path} answered a foreign cursor with {}: {}",
            res.status,
            res.text()
        );
    }
}

// --------------------------------------------------- lenient by design ---

#[tokio::test]
async fn an_empty_parameter_value_is_read_as_no_filter_at_all() {
    // A form that submits every field sends the empty ones too. Treating
    // `?status=` as "status must be empty" would return nothing and read as
    // data loss.
    let app = seeded().await;
    same(&traces_matching(&app, "status=&name=&q=").await, ALL_RUNS, "an empty value filtered");
    same(&spans_matching(&app, "kind=&model=").await, ALL_SPANS, "an empty value filtered");
    same(&logs_matching(&app, "level=&trace_id=").await, ALL_LOGS, "an empty value filtered");
    same(&scores_matching(&app, "source=&scorer_id=").await, ALL_SCORES, "an empty value filtered");
}

#[tokio::test]
async fn a_parameter_name_the_endpoint_does_not_know_is_refused_rather_than_ignored() {
    // This reverses an earlier decision, so the reason is here rather than in a
    // commit message. The list endpoints used to accept any parameter and drop
    // the ones they did not know, so that an SDK sending a parameter a newer
    // daemon understands would not fail against an older one. The cost was paid
    // by everybody else: `?statuss=error` returned every run in the project
    // with a 200, and there is nothing about that response to notice. A caller
    // who believed they had filtered read a plausible wrong answer.
    //
    // Forward compatibility is still the thing being traded away, and it is
    // worth naming: a client that sends a filter this daemon has never heard of
    // now gets a 400 rather than a wider result set. That is the better of the
    // two failures. A 400 names the parameter and stops; a silent drop answers
    // a different question and looks like an answer. The full vocabulary of
    // each endpoint, and the refusals, are measured in `tests/vocabulary.rs`.
    let app = seeded().await;
    for query in ["statuss=error", "trace_id=whatever"] {
        let res = send(&app, get(&format!("/v1/api/traces?{query}"))).await;
        assert_eq!(
            res.status,
            StatusCode::BAD_REQUEST,
            "the trace list accepted and dropped {query}: {}",
            res.text()
        );
    }
}

#[tokio::test]
async fn a_limit_below_one_is_clamped_rather_than_refused() {
    let app = seeded().await;
    let res = send(&app, get("/v1/api/traces?limit=0")).await;
    assert_eq!(res.status, StatusCode::OK, "limit=0 was not clamped: {}", res.text());
    assert_eq!(
        res.json()["traces"].as_array().map(Vec::len),
        Some(1),
        "limit=0 should clamp to one row, got: {}",
        res.text()
    );
}

#[tokio::test]
async fn a_limit_past_the_maximum_is_clamped_rather_than_refused() {
    let app = seeded().await;
    for (path, list, len) in [
        ("/v1/api/traces", "traces", 6),
        ("/v1/api/spans", "spans", 9),
        ("/v1/api/logs", "logs", 8),
        ("/v1/api/scores", "scores", 6),
    ] {
        let res = send(&app, get(&format!("{path}?limit=999999"))).await;
        assert_eq!(res.status, StatusCode::OK, "{path} refused a large limit: {}", res.text());
        assert_eq!(
            res.json()[list].as_array().map(Vec::len),
            Some(len),
            "{path} did not return everything: {}",
            res.text()
        );
    }
}

// ------------------------------------ the sorts the reference documents ---

/// `spec/mcpd.spec.json` documents `sort` on `GET /v1/api/spans` as
/// "`start_time`, `duration` or `cost`", and the handler now reads it.
///
/// The three spans that lasted exactly 1000ms are the point of this case: they
/// tie on the sort key, so they come back in span id order, descending with the
/// listing. A tie the sort cannot break is a row dropped or returned twice.
#[tokio::test]
async fn the_span_list_sorts_by_duration_when_asked_to() {
    let app = seeded().await;
    same(
        &spans_matching(&app, "sort=duration").await,
        &[
            FAILED_ROOT,
            INGEST_ROOT,
            SUMMARISE_ROOT,
            CANCELLED_ROOT,
            TWIN_ROOT,
            SUMMARISE_MODEL,
            CHECKOUT_ROOT,
            CHECKOUT_MODEL,
            SUMMARISE_TOOL,
        ],
        "sort=duration is wrong",
    );
}

/// `spec/mcpd.spec.json` documents `sort` on `GET /v1/api/traces` as one of
/// "`start_time`, `duration`, `cost`, `span_count`, `score`", and the handler
/// now accepts all five.
#[tokio::test]
async fn the_trace_list_sorts_by_score_when_asked_to() {
    let app = seeded().await;
    let res = send(&app, get("/v1/api/traces?sort=score")).await;
    assert_eq!(res.status, StatusCode::OK, "sort=score was refused: {}", res.text());
}
