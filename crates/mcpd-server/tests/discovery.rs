//! What to put in a filter box, and what has arrived since you last looked.
//!
//! A filter is only usable if you know what to type into it. mcpd had one
//! discovery route, `GET /v1/api/projects`, out of nine dimensions a run list
//! filters on, so an interface offering an environment picker had to build it
//! from whatever happened to be on the current page. That is not a filter, it
//! is a list of what you can already see.
//!
//! Every value here is read back through the real router after a real ingest,
//! so what is measured is that the write path fills the distinct-value list and
//! the read path serves it, not that a query compiles.

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

const READ_KEY: &str = "read-only-key";
const INGEST_KEY: &str = "write-only-key";

fn temp_db() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    // The pid alone is not unique: a recycled one reopens the previous run's
    // database and every count is off by a whole prior suite.
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir = std::env::temp_dir().join(format!("mcpd-disco-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

fn config() -> ServerConfig {
    ServerConfig {
        read_keys: vec![READ_KEY.to_owned()],
        ingest_keys: vec![INGEST_KEY.to_owned()],
        ..ServerConfig::default()
    }
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
    let res = app.clone().oneshot(req).await.expect("the router answered");
    let status = res.status();
    let body = res.into_body().collect().await.expect("read the body").to_bytes().to_vec();
    Res { status, body }
}

async fn get(app: &Router, uri: &str) -> Res {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .header("authorization", format!("Bearer {READ_KEY}"))
        .body(Body::empty())
        .expect("build the request");
    send(app, req).await
}

/// Two projects, so every answer can be checked for leaking across the
/// boundary that the rest of this daemon spends its effort establishing.
fn fixture() -> Value {
    json!({
        "spans": [
            {
                "trace_id": "0123456789abcdef0123456789abcdef",
                "span_id": "0123456789abcdef",
                "name": "checkout",
                "kind": "agent_run",
                "status": "ok",
                "entity_kind": "agent",
                "entity_name": "checkout-agent",
                "tags": ["beta", "eu-west"],
                "start_time": "2026-01-01T00:00:00Z",
                "end_time": "2026-01-01T00:00:01Z",
                "is_root": true,
                "is_event": false,
                "correlation": {
                    "project": "default",
                    "environment": "production",
                    "service_name": "checkout-api"
                },
                "received_at": "2026-01-01T00:00:01Z"
            },
            {
                "trace_id": "0123456789abcdef0123456789abcdef",
                "span_id": "abcdef0123456789",
                "name": "search",
                "kind": "tool_call",
                "status": "ok",
                "entity_kind": "tool",
                "entity_name": "checkout-search",
                "start_time": "2026-01-01T00:00:00Z",
                "end_time": "2026-01-01T00:00:01Z",
                "is_root": false,
                "is_event": false,
                "correlation": {
                    "project": "default",
                    "environment": "staging",
                    "service_name": "search-api"
                },
                "received_at": "2026-01-01T00:00:01Z"
            },
            {
                "trace_id": "fedcba9876543210fedcba9876543210",
                "span_id": "fedcba9876543210",
                "name": "other",
                "kind": "agent_run",
                "status": "ok",
                "entity_kind": "workflow",
                "entity_name": "another-teams-workflow",
                "tags": ["secret-tag"],
                "start_time": "2026-01-01T00:00:00Z",
                "end_time": "2026-01-01T00:00:01Z",
                "is_root": true,
                "is_event": false,
                "correlation": {
                    "project": "other",
                    "environment": "another-teams-environment",
                    "service_name": "another-teams-service"
                },
                "received_at": "2026-01-01T00:00:01Z"
            }
        ],
        "metrics": [
            {
                "metric_id": "m-1",
                "name": "checkout.latency",
                "kind": "gauge",
                "value": 12.5,
                "timestamp": "2026-01-01T00:00:01Z",
                "labels": { "model": "gpt-4o", "region": "eu" },
                "correlation": { "project": "default" },
                "received_at": "2026-01-01T00:00:01Z"
            },
            {
                "metric_id": "m-2",
                "name": "checkout.tokens",
                "kind": "counter",
                "value": 3.0,
                "timestamp": "2026-01-01T00:00:01Z",
                "labels": { "model": "gpt-4o-mini" },
                "correlation": { "project": "default" },
                "received_at": "2026-01-01T00:00:01Z"
            }
        ]
    })
}

/// A router whose store already holds the fixture.
async fn app_with_data() -> Router {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let app = router(Arc::new(AppState::new(store as Arc<dyn Store>, config())));
    let body = fixture().to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/ingest")
        .header("content-type", "application/json")
        .header("content-length", body.len().to_string())
        .header("authorization", format!("Bearer {INGEST_KEY}"))
        .body(Body::from(body))
        .expect("build the request");
    let res = send(&app, req).await;
    assert_eq!(res.status, StatusCode::OK, "seeding the fixture failed: {}", res.text());
    app
}

/// The values one facet offers, as plain strings.
async fn values(app: &Router, query: &str) -> Vec<String> {
    let res = get(app, &format!("/v1/api/discovery?{query}")).await;
    assert_eq!(res.status, StatusCode::OK, "discovery refused {query}: {}", res.text());
    res.json()["values"]
        .as_array()
        .unwrap_or_else(|| panic!("no values array for {query}"))
        .iter()
        .map(|v| v["value"].as_str().expect("a value is a string").to_owned())
        .collect()
}

// ------------------------------------------------------------ the dimensions ---

/// Every dimension a run list filters on, answered from what has actually
/// arrived. This is the whole point: mcpd served one of these and had eight
/// filters nobody could populate a box for.
#[tokio::test]
async fn every_filter_dimension_offers_the_values_it_has_seen() {
    let app = app_with_data().await;

    assert_eq!(values(&app, "facet=entity_kind").await, vec!["agent", "tool"]);
    assert_eq!(values(&app, "facet=entity_name").await, vec!["checkout-agent", "checkout-search"]);
    assert_eq!(values(&app, "facet=service_name").await, vec!["checkout-api", "search-api"]);
    assert_eq!(values(&app, "facet=environment").await, vec!["production", "staging"]);
    assert_eq!(values(&app, "facet=tag").await, vec!["beta", "eu-west"]);

    // The daemon derives metrics of its own from the spans it stores, so this
    // one is a containment check rather than an equality: asserting the exact
    // set here would make this test fail the day another derived metric is
    // added, which is not what it is measuring.
    let metrics = values(&app, "facet=metric_name").await;
    assert!(metrics.contains(&"checkout.latency".to_owned()), "{metrics:?}");
    assert!(metrics.contains(&"checkout.tokens".to_owned()), "{metrics:?}");

    let keys = values(&app, "facet=metric_label_key").await;
    assert!(keys.contains(&"model".to_owned()), "{keys:?}");
    assert!(keys.contains(&"region".to_owned()), "{keys:?}");

    assert_eq!(
        values(&app, "facet=metric_label_value&key=model").await,
        vec!["gpt-4o", "gpt-4o-mini"]
    );
    assert_eq!(values(&app, "facet=metric_label_value&key=region").await, vec!["eu"]);
}

/// Typeahead. A filter box asks on every keystroke, so a prefix has to narrow
/// the answer rather than the caller narrowing it afterwards.
#[tokio::test]
async fn a_prefix_narrows_every_dimension() {
    let app = app_with_data().await;
    assert_eq!(values(&app, "facet=service_name&prefix=check").await, vec!["checkout-api"]);
    assert_eq!(values(&app, "facet=entity_name&prefix=checkout-s").await, vec!["checkout-search"]);
    assert_eq!(values(&app, "facet=environment&prefix=prod").await, vec!["production"]);
    // A prefix nothing starts with is an empty list, not an error.
    assert!(values(&app, "facet=environment&prefix=zzz").await.is_empty());
}

/// The values are the shape of another team's data: their service names, their
/// environments, their tags. A read key for one project must not learn them.
#[tokio::test]
async fn one_project_cannot_read_another_project_dimensions() {
    let app = app_with_data().await;
    for facet in ["service_name", "environment", "tag", "entity_name", "entity_kind"] {
        let mine = values(&app, &format!("facet={facet}")).await;
        assert!(
            !mine.iter().any(|v| v.starts_with("another-teams") || v == "secret-tag"),
            "the {facet} list leaked another project's values: {mine:?}"
        );
    }
    // Named explicitly, the other project's values are there for a key that
    // reaches it, so the check above is measuring scoping and not an empty
    // table.
    let theirs = values(&app, "project=other&facet=service_name").await;
    assert_eq!(theirs, vec!["another-teams-service"]);
}

/// `last_seen` is when the daemon received a signal carrying the value, not the
/// signal's own timestamp. The fixture's spans are stamped 2026-01-01, and the
/// answer must be now: a backfill of last year's data must not make a value
/// look stale, and dating the row from the payload is how that would happen.
#[tokio::test]
async fn the_values_carry_when_they_were_last_seen() {
    let app = app_with_data().await;
    let res = get(&app, "/v1/api/discovery?facet=environment").await;
    let first = res.json()["values"][0].clone();
    assert_eq!(first["value"], "production");
    let raw = first["last_seen"].as_str().unwrap_or_default().to_owned();
    let seen = time::OffsetDateTime::parse(&raw, &time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|e| panic!("last_seen is not an RFC 3339 timestamp ({e}): {first}"));
    let age = (time::OffsetDateTime::now_utc() - seen).whole_seconds();
    assert!(
        (0..60).contains(&age),
        "last_seen should be when the value arrived, and it is {age} seconds old: {first}"
    );
}

#[tokio::test]
async fn a_limit_bounds_the_list() {
    let app = app_with_data().await;
    assert_eq!(values(&app, "facet=environment&limit=1").await, vec!["production"]);
}

// ---------------------------------------------------------------- refusals ---

/// A typo in a facet name is otherwise an empty list, which reads as "this
/// project has no environments" rather than as a mistake.
#[tokio::test]
async fn an_unknown_facet_names_the_eight() {
    let app = app_with_data().await;
    let res = get(&app, "/v1/api/discovery?facet=enviroment").await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "an unknown facet answered: {}", res.text());
    let body = res.json();
    assert_eq!(body["field"], "facet");
    let message = body["message"].as_str().unwrap_or_default();
    for facet in [
        "entity_kind",
        "entity_name",
        "service_name",
        "environment",
        "tag",
        "metric_name",
        "metric_label_key",
        "metric_label_value",
    ] {
        assert!(message.contains(facet), "the refusal does not list {facet}: {message}");
    }
}

#[tokio::test]
async fn a_missing_facet_is_refused_rather_than_guessed() {
    let app = app_with_data().await;
    let res = get(&app, "/v1/api/discovery").await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "discovery guessed a dimension");
    assert_eq!(res.json()["field"], "facet");
}

/// Label values without a key would be every label's values mixed together,
/// which is the label index rather than a filter box.
#[tokio::test]
async fn label_values_need_the_label() {
    let app = app_with_data().await;
    let res = get(&app, "/v1/api/discovery?facet=metric_label_value").await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "label values were served for no key");
    let body = res.json();
    assert_eq!(body["field"], "key");
    assert!(body["message"].as_str().unwrap_or_default().contains("one label"), "{body}");
}

/// A key on a facet that ignores it must not be echoed back, or a caller reads
/// it as having been applied.
#[tokio::test]
async fn a_key_is_echoed_only_where_it_was_used() {
    let app = app_with_data().await;
    let ignored = get(&app, "/v1/api/discovery?facet=environment&key=model").await.json();
    assert!(ignored.get("key").is_none_or(Value::is_null), "an unused key was echoed: {ignored}");
    let used = get(&app, "/v1/api/discovery?facet=metric_label_value&key=model").await.json();
    assert_eq!(used["key"], "model");
}

#[tokio::test]
async fn discovery_needs_a_read_key() {
    let app = app_with_data().await;
    let req = Request::builder()
        .method("GET")
        .uri("/v1/api/discovery?facet=service_name")
        .body(Body::empty())
        .expect("build the request");
    assert_eq!(send(&app, req).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn discovery_refuses_a_parameter_it_does_not_apply() {
    let app = app_with_data().await;
    let res = get(&app, "/v1/api/discovery?facet=environment&contains=prod").await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "discovery ignored a filter: {}", res.text());
    assert_eq!(res.json()["field"], "contains");
}

// ----------------------------------------------------------------- deltas ---

/// The refresh that is not a re-query. A list already on screen applies what
/// arrived rather than running its whole query again.
#[tokio::test]
async fn a_poll_returns_what_arrived_and_then_nothing() {
    let app = app_with_data().await;

    let first = get(&app, "/v1/api/deltas?signal=traces").await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.text());
    let body = first.json();
    assert_eq!(body["signal"], "traces");
    let traces = body["traces"].as_array().expect("a traces array").clone();
    assert_eq!(traces.len(), 1, "the default project holds one run: {body}");
    assert_eq!(traces[0]["trace_id"], "0123456789abcdef0123456789abcdef");
    assert_eq!(body["spans"], Value::Null, "a poll for traces carried spans as well");
    assert_eq!(body["logs"], Value::Null, "a poll for traces carried logs as well");
    let cursor = body["cursor"].as_str().expect("a cursor").to_owned();

    // Nothing new. The cursor must not move, or a row that lands a moment later
    // with an earlier arrival time is dropped for ever.
    let again = get(&app, &format!("/v1/api/deltas?signal=traces&cursor={cursor}")).await;
    assert_eq!(again.status, StatusCode::OK, "{}", again.text());
    let body = again.json();
    assert_eq!(body["traces"].as_array().map(Vec::len), Some(0), "the same run came back twice");
    assert_eq!(body["cursor"], cursor.as_str(), "an empty poll moved the watermark");
    assert_eq!(body["has_more"], false);
}

#[tokio::test]
async fn a_poll_answers_for_each_signal_it_serves() {
    let app = app_with_data().await;
    let spans = get(&app, "/v1/api/deltas?signal=spans").await.json();
    assert_eq!(spans["signal"], "spans");
    assert_eq!(spans["spans"].as_array().map(Vec::len), Some(2), "{spans}");
    assert_eq!(spans["traces"], Value::Null);

    let logs = get(&app, "/v1/api/deltas?signal=logs").await.json();
    assert_eq!(logs["signal"], "logs");
    assert_eq!(logs["logs"].as_array().map(Vec::len), Some(0), "{logs}");
}

/// Scores and feedback carry the caller's own timestamp and no server-set
/// arrival time, so a watermark over them would either skip a late row or
/// repeat every row for ever. The refusal says so rather than returning an
/// empty page.
#[tokio::test]
async fn a_signal_with_no_arrival_time_is_refused_by_name() {
    let app = app_with_data().await;
    let res = get(&app, "/v1/api/deltas?signal=scores").await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "scores were polled: {}", res.text());
    let body = res.json();
    assert_eq!(body["field"], "signal");
    let message = body["message"].as_str().unwrap_or_default();
    assert!(message.contains("traces"), "the refusal should name what can be polled: {message}");
}

/// A cursor this daemon did not issue is refused, not guessed at. A guess is a
/// silently wrong watermark, which shows up as a list that never updates.
#[tokio::test]
async fn a_cursor_the_daemon_did_not_issue_is_refused() {
    let app = app_with_data().await;
    let res = get(&app, "/v1/api/deltas?signal=traces&cursor=notacursor").await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST, "a forged cursor was accepted: {}", res.text());
    assert_eq!(res.json()["error"], "invalid_query");
}

#[tokio::test]
async fn a_poll_defaults_to_the_run_list() {
    let app = app_with_data().await;
    let body = get(&app, "/v1/api/deltas").await.json();
    assert_eq!(body["signal"], "traces", "a poll with no signal did not default to runs: {body}");
}

#[tokio::test]
async fn a_poll_is_scoped_to_one_project() {
    let app = app_with_data().await;
    let mine = get(&app, "/v1/api/deltas?signal=traces").await.json();
    let ids: Vec<&str> =
        mine["traces"].as_array().unwrap().iter().filter_map(|t| t["trace_id"].as_str()).collect();
    assert!(
        !ids.contains(&"fedcba9876543210fedcba9876543210"),
        "a poll of one project returned another project's run: {ids:?}"
    );
}
