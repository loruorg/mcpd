//! What a list endpoint accepts, and what it does with everything else.
//!
//! mcpd read every list's query string with a lenient map, so a parameter it
//! had never heard of was accepted, dropped, and answered with the whole
//! project. `?service_name=checkout-api` came back 200 with every run in it,
//! and there is nothing about that response to notice. A caller who believed
//! they had filtered read a plausible wrong answer, which is worse than an
//! error, because an error is visible.
//!
//! Two things are measured. Every parameter the specification documents is one
//! the door applies, and everything else is refused by name.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mcpd_server::query::{
    FEEDBACK_PARAMS, LOG_PARAMS, METRIC_PARAMS, SCORE_PARAMS, SPAN_PARAMS, TRACE_PARAMS,
};
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::Store;
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::Arc;
use tower::ServiceExt;

const READ_KEY: &str = "read-only-key";

fn temp_db() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    // A pid is recycled, and a recycled one reopens the previous run's database.
    // The per-process stamp is what makes this directory this run's own.
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir = std::env::temp_dir().join(format!("mcpd-vocab-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

async fn app() -> Router {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let config = ServerConfig { read_keys: vec![READ_KEY.to_owned()], ..ServerConfig::default() };
    router(Arc::new(AppState::new(store as Arc<dyn Store>, config)))
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

async fn get(app: &Router, uri: &str) -> Res {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .header("authorization", format!("Bearer {READ_KEY}"))
        .body(Body::empty())
        .expect("build the request");
    let res = app.clone().oneshot(req).await.expect("the router answered");
    let status = res.status();
    let body = res.into_body().collect().await.expect("read the body").to_bytes().to_vec();
    Res { status, body }
}

/// The query parameter names the specification declares for one endpoint.
fn documented(endpoint_id: &str) -> BTreeSet<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/mcpd.spec.json");
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let spec: Value = serde_json::from_str(&raw).expect("the spec is JSON");
    let endpoints = spec["endpoints"].as_array().expect("the spec lists endpoints");
    let ep = endpoints
        .iter()
        .find(|e| e["id"] == endpoint_id)
        .unwrap_or_else(|| panic!("the spec has no endpoint {endpoint_id}"));
    ep["query"]
        .as_array()
        .unwrap_or_else(|| panic!("{endpoint_id} declares no query parameters"))
        .iter()
        .map(|q| q["name"].as_str().expect("a parameter name").to_owned())
        .collect()
}

// ------------------------------------------- the documented is the enforced ---

/// The join between the specification and the door.
///
/// A parameter documented and not implemented is a filter a reader is told to
/// use and that does nothing. A parameter implemented and not documented is one
/// no SDK will ever send. Both were possible until the door had a vocabulary to
/// compare against.
#[test]
fn the_trace_list_accepts_exactly_what_the_spec_documents() {
    let enforced: BTreeSet<String> = TRACE_PARAMS.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(
        enforced,
        documented("list_traces"),
        "the trace list's vocabulary and spec/mcpd.spec.json disagree"
    );
}

#[test]
fn the_span_list_accepts_exactly_what_the_spec_documents() {
    let enforced: BTreeSet<String> = SPAN_PARAMS.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(
        enforced,
        documented("list_spans"),
        "the span list's vocabulary and spec/mcpd.spec.json disagree"
    );
}

/// The same join for every other list. One table, so a list added without a
/// vocabulary is a missing row here rather than a door that silently drops what
/// it does not recognise.
#[test]
fn every_list_accepts_exactly_what_the_spec_documents() {
    for (endpoint_id, enforced) in [
        ("list_logs", LOG_PARAMS),
        ("list_scores", SCORE_PARAMS),
        ("query_metrics", METRIC_PARAMS),
        ("list_feedback", FEEDBACK_PARAMS),
    ] {
        let enforced: BTreeSet<String> = enforced.iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(
            enforced,
            documented(endpoint_id),
            "{endpoint_id}'s vocabulary and spec/mcpd.spec.json disagree"
        );
    }
}

/// The filters five SDKs send to the feedback door and the daemon never
/// applied. `kind`, `from`, `to` and `cursor` are serialised by the TypeScript,
/// Python, Go, Kotlin and Rust clients, and the request succeeded and answered
/// a different question. They are refused now, which is the answer that lets
/// somebody find out.
#[tokio::test]
async fn the_feedback_door_refuses_the_filters_it_never_applied() {
    let app = app().await;
    for name in ["kind", "from", "to", "cursor"] {
        let res = get(&app, &format!("/v1/api/feedback?trace_id=abc&{name}=x")).await;
        assert_eq!(
            res.status,
            StatusCode::BAD_REQUEST,
            "the feedback list accepted and dropped {name}: {}",
            res.text()
        );
        assert_eq!(res.json()["field"], name);
    }
}

/// Without a run named, the older behaviour was an empty list with a 200, which
/// reads as "nobody has judged anything in this project". mcpd cannot list a
/// project's feedback, and saying so is the only honest answer.
#[tokio::test]
async fn the_feedback_door_refuses_rather_than_answering_an_empty_list() {
    let app = app().await;
    let res = get(&app, "/v1/api/feedback").await;
    assert_eq!(
        res.status,
        StatusCode::BAD_REQUEST,
        "a project-wide feedback list answered as though there were none: {}",
        res.text()
    );
    let body = res.json();
    assert_eq!(body["field"], "trace_id");
    let message = body["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("cannot yet list"),
        "the refusal should say the whole-project list is not served: {message}"
    );
}

/// Every documented parameter, sent one at a time with a value it can read.
///
/// The set comparison above is a comparison of two lists. This drives the real
/// router, so a parameter that is in both lists and that the handler never
/// reads still has to survive the request.
#[tokio::test]
async fn every_documented_trace_parameter_is_served() {
    let app = app().await;
    for (name, value) in [
        ("project", "default"),
        ("q", "refund"),
        ("status", "error"),
        ("kind", "agent_run"),
        ("name", "checkout"),
        ("tags", "beta,eu"),
        ("session_id", "s-1"),
        ("thread_id", "t-1"),
        ("user_id", "u-1"),
        ("environment", "production"),
        ("model", "gpt-4o"),
        ("from", "-1h"),
        ("to", "-1m"),
        ("min_duration_ms", "10"),
        ("min_cost", "0.01"),
        ("has_score", "answer_relevance"),
        ("score_below", "0.5"),
        ("sort", "cost"),
        ("order", "asc"),
        ("limit", "10"),
        ("cursor", ""),
    ] {
        assert!(TRACE_PARAMS.contains(&name), "{name} is not in the enforced vocabulary");
        let res = get(&app, &format!("/v1/api/traces?{name}={value}")).await;
        assert_eq!(res.status, StatusCode::OK, "the trace list refused {name}: {}", res.text());
    }
}

// -------------------------------------------------------------- the refusals ---

/// The failure this whole file exists for: a real correlation field, on the
/// row, offered by discovery, that the list cannot filter on. It used to be
/// accepted and dropped.
#[tokio::test]
async fn a_recorded_but_unfilterable_dimension_says_so() {
    let app = app().await;
    let res = get(&app, "/v1/api/traces?service_name=checkout-api").await;
    assert_eq!(
        res.status,
        StatusCode::BAD_REQUEST,
        "the trace list accepted a filter it does not apply: {}",
        res.text()
    );
    let body = res.json();
    assert_eq!(body["error"], "bad_parameter");
    assert_eq!(body["field"], "service_name", "the refusal did not name the parameter: {body}");
    let message = body["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("does not filter"),
        "the message should say the filter is not applied: {message}"
    );
    assert!(
        message.contains("discovery"),
        "the message should say where the values can still be seen: {message}"
    );
}

/// Every correlation field in the same position answers the same way. A list
/// where one of them is handled and the others fall through to "unknown
/// parameter" is the inconsistency this table exists to prevent.
#[tokio::test]
async fn every_recorded_dimension_answers_the_same_way() {
    let app = app().await;
    for name in
        ["service_name", "service_version", "resource_id", "run_id", "request_id", "release"]
    {
        let res = get(&app, &format!("/v1/api/traces?{name}=x")).await;
        assert_eq!(res.status, StatusCode::BAD_REQUEST, "{name} was accepted and ignored");
        let body = res.json();
        assert_eq!(body["field"], name);
        assert!(
            body["message"].as_str().unwrap_or_default().contains("does not filter"),
            "{name} was refused as an unknown parameter rather than as an unfiltered one: {body}"
        );
    }
}

/// A typo is a different answer: it names what the endpoint does take, so the
/// reader can find the one they meant.
#[tokio::test]
async fn a_misspelt_parameter_lists_what_is_accepted() {
    let app = app().await;
    let res = get(&app, "/v1/api/traces?sesion_id=s-1").await;
    assert_eq!(
        res.status,
        StatusCode::BAD_REQUEST,
        "a misspelt filter was ignored: {}",
        res.text()
    );
    let body = res.json();
    assert_eq!(body["field"], "sesion_id");
    let message = body["message"].as_str().unwrap_or_default();
    assert!(message.contains("session_id"), "the message should list what is accepted: {message}");
    assert!(message.contains("cursor"), "the message should list the whole vocabulary: {message}");
}

/// Two mistakes in one request name the same one every time. A map has no
/// order, so without sorting the offenders the same request produced two
/// different messages on two runs.
#[tokio::test]
async fn two_unknown_parameters_are_reported_the_same_way_every_time() {
    let app = app().await;
    for _ in 0..8 {
        let res = get(&app, "/v1/api/traces?zebra=1&apple=2").await;
        assert_eq!(res.status, StatusCode::BAD_REQUEST);
        let body = res.json();
        assert_eq!(body["field"], "apple", "the reported parameter moved between runs: {body}");
        assert!(
            body["message"].as_str().unwrap_or_default().contains("zebra"),
            "the other mistake went unmentioned: {body}"
        );
    }
}

/// The span list carries no tags, session or user of its own: those belong to
/// the run. The reference has always said so, and the door dropped all three in
/// silence, so a span search narrowed by session returned the whole project.
#[tokio::test]
async fn the_span_list_refuses_a_run_level_filter() {
    let app = app().await;
    for name in ["session_id", "thread_id", "user_id", "tags"] {
        let res = get(&app, &format!("/v1/api/spans?{name}=x")).await;
        assert_eq!(
            res.status,
            StatusCode::BAD_REQUEST,
            "the span list accepted {name}, which belongs to the run: {}",
            res.text()
        );
        assert_eq!(res.json()["field"], name);
    }
}

/// The refusal must not become a way to learn the surface without a key.
#[tokio::test]
async fn the_vocabulary_is_not_readable_without_a_key() {
    let app = app().await;
    let req = Request::builder()
        .method("GET")
        .uri("/v1/api/traces?service_name=x")
        .body(Body::empty())
        .expect("build the request");
    let res = app.clone().oneshot(req).await.expect("the router answered");
    assert_eq!(
        res.status(),
        StatusCode::UNAUTHORIZED,
        "an anonymous caller learned the filter vocabulary"
    );
}

/// A full query with every filter set at once still works. Without this the
/// suite would pass on a door that refused everything.
#[tokio::test]
async fn a_fully_specified_query_is_still_served() {
    let app = app().await;
    let res = get(
        &app,
        "/v1/api/traces?project=default&q=refund&status=error&kind=agent_run&name=checkout\
         &tags=beta&session_id=s&thread_id=t&user_id=u&environment=production&model=gpt-4o\
         &from=-24h&to=-1m&min_duration_ms=5&min_cost=0.001&has_score=relevance\
         &score_below=0.5&sort=cost&order=asc&limit=25",
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "a fully specified query was refused: {}", res.text());
}
