//! Histogram metrics read back as distributions, end to end: an OTLP
//! histogram goes in through `/v1/metrics`, and `/v1/api/metrics/histogram`
//! answers with the merged buckets and quantiles read from them.

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
        std::env::temp_dir().join(format!("mcpd-histogram-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

async fn app() -> Router {
    let store = SqliteStore::open(temp_db(), 2).expect("open the store");
    store.migrate().await.expect("migrate the store");
    let state = Arc::new(AppState::new(Arc::new(store) as Arc<dyn Store>, ServerConfig::default()));
    router(state)
}

async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .expect("build the request");
    let res = app.clone().oneshot(req).await.expect("the router answered");
    let status = res.status();
    let bytes = res.into_body().collect().await.expect("read the body").to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn histogram(now_ns: i128, counts: [u64; 3], model: &str) -> Value {
    json!({
        "timeUnixNano": now_ns.to_string(),
        "count": counts.iter().sum::<u64>().to_string(),
        "sum": 1000.0,
        "explicitBounds": [100.0, 1000.0],
        "bucketCounts": counts.map(|c| c.to_string()),
        "attributes": [{ "key": "model", "value": { "stringValue": model } }]
    })
}

#[tokio::test]
async fn an_otlp_histogram_is_read_back_as_merged_buckets_and_quantiles() {
    let app = app().await;
    let now = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
    let body = json!({ "resourceMetrics": [{ "scopeMetrics": [{ "metrics": [{
        "name": "llm.latency", "unit": "ms",
        "histogram": { "aggregationTemporality": 1, "dataPoints": [
            histogram(now - 2_000_000_000, [0, 10, 0], "a"),
            histogram(now - 1_000_000_000, [0, 10, 0], "a"),
            histogram(now - 1_000_000_000, [0, 0, 4], "b"),
        ]}
    }]}]}]});
    let (status, res) = call(&app, "POST", "/v1/metrics", Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{res}");

    let (status, h) =
        call(&app, "GET", "/v1/api/metrics/histogram?name=llm.latency&group_by=model", None).await;
    assert_eq!(status, StatusCode::OK, "{h}");
    assert_eq!(h["points_read"], json!(3));
    assert_eq!(h["unit"], json!("ms"));
    let a = h["series"].as_array().unwrap().iter().find(|s| s["labels"]["model"] == "a").unwrap();
    assert_eq!(a["counts"], json!([0, 20, 0]));
    assert_eq!(a["exact"], json!(false));
    // Half of twenty, inside the 100..1000 bucket: halfway along it.
    assert!((a["p50"].as_f64().unwrap() - 550.0).abs() < 1e-6, "{a}");

    let (status, page) =
        call(&app, "GET", "/v1/api/metrics/points?name=llm.latency&limit=2", None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["points"].as_array().map(Vec::len), Some(2));
    assert_eq!(page["page"]["has_more"], json!(true));
    assert_eq!(page["points"][0]["buckets"]["temporality"], json!(1));
}

#[tokio::test]
async fn a_histogram_read_names_what_it_needs() {
    let app = app().await;
    let (status, res) = call(&app, "GET", "/v1/api/metrics/histogram", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{res}");
    assert_eq!(res["field"], json!("name"));
    let (status, _) = call(&app, "GET", "/v1/api/metrics/points?agg=p95", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
