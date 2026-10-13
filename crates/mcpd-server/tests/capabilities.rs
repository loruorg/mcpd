//! What a backend can do, and what it says when it cannot.
//!
//! mcpd has one storage trait and three backends behind it, and they are not
//! equal. Before this suite the difference was discoverable only by running a
//! query and reading a wrong answer: a filter box drawn from a discovery call
//! came back empty on a backend that keeps no distinct-value list, and an empty
//! list reads as "this project has no environments" rather than as "this
//! deployment cannot answer that".
//!
//! Two things are measured here, and they are two halves of one promise.
//! `GET /v1/api/capabilities` publishes what the backend can do, and every door
//! whose capability is false refuses with 501 naming the same word. A caller
//! reads the first and never meets the second.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::Store;
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

const READ_KEY: &str = "read-only-key";

/// A private database file per test.
///
/// The pid alone is not unique: pids are recycled, and a recycled one reopens
/// the previous run's database, so every count is off by a whole prior suite.
/// A per-process stamp makes the directory this run's own. The same
/// construction is in `tests/http.rs` for the same reason.
fn temp_db() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir = std::env::temp_dir().join(format!("mcpd-caps-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

/// A backend that has declared no capabilities at all.
///
/// It is a real SQLite store for everything the trait requires, and takes the
/// trait's own defaults for the five optional methods. That is deliberately not
/// a hand-written refusal: it is exactly the position a fourth backend is in on
/// the day somebody adds one, so what this suite measures is what that backend
/// would do rather than what a mock was told to say.
struct Bare(Arc<SqliteStore>);

#[async_trait::async_trait]
impl Store for Bare {
    fn name(&self) -> &'static str {
        "bare"
    }
    async fn migrate(&self) -> mcpd_store::Result<()> {
        self.0.migrate().await
    }
    async fn ping(&self) -> mcpd_store::Result<()> {
        self.0.ping().await
    }
    async fn ingest(
        &self,
        batch: mcpd_store::Batch,
    ) -> mcpd_store::Result<mcpd_core::IngestResult> {
        self.0.ingest(batch).await
    }
    async fn list_traces(
        &self,
        q: &mcpd_store::TraceQuery,
    ) -> mcpd_store::Result<mcpd_store::Paged<mcpd_core::Trace>> {
        self.0.list_traces(q).await
    }
    async fn get_trace(
        &self,
        project: &str,
        trace_id: &str,
        include: &mcpd_store::Include,
    ) -> mcpd_store::Result<mcpd_core::TraceDetail> {
        self.0.get_trace(project, trace_id, include).await
    }
    async fn delete_trace(&self, project: &str, trace_id: &str) -> mcpd_store::Result<u32> {
        self.0.delete_trace(project, trace_id).await
    }
    async fn erase_subject(
        &self,
        project: &str,
        user_id: &str,
    ) -> mcpd_store::Result<mcpd_store::ErasureOutcome> {
        self.0.erase_subject(project, user_id).await
    }
    async fn list_spans(
        &self,
        q: &mcpd_store::SpanQuery,
    ) -> mcpd_store::Result<mcpd_store::Paged<mcpd_core::Span>> {
        self.0.list_spans(q).await
    }
    async fn list_logs(
        &self,
        q: &mcpd_store::LogQuery,
    ) -> mcpd_store::Result<mcpd_store::Paged<mcpd_core::LogRecord>> {
        self.0.list_logs(q).await
    }
    async fn query_metrics(
        &self,
        q: &mcpd_store::MetricQuery,
    ) -> mcpd_store::Result<mcpd_core::MetricSeriesResponse> {
        self.0.query_metrics(q).await
    }
    async fn list_feedback(
        &self,
        q: &mcpd_store::FeedbackQuery,
    ) -> mcpd_store::Result<mcpd_store::Paged<mcpd_core::Feedback>> {
        self.0.list_feedback(q).await
    }
    async fn list_metric_points(
        &self,
        q: &mcpd_store::MetricPointQuery,
    ) -> mcpd_store::Result<mcpd_store::Paged<mcpd_core::MetricPoint>> {
        self.0.list_metric_points(q).await
    }
    async fn list_scores(
        &self,
        q: &mcpd_store::ScoreQuery,
    ) -> mcpd_store::Result<mcpd_store::Paged<mcpd_core::Score>> {
        self.0.list_scores(q).await
    }
    async fn analytics_summary(
        &self,
        project: &str,
        from: time::OffsetDateTime,
        to: time::OffsetDateTime,
        group_by: Option<&str>,
    ) -> mcpd_store::Result<mcpd_core::AnalyticsSummary> {
        self.0.analytics_summary(project, from, to, group_by).await
    }
    async fn analytics_cost(
        &self,
        project: &str,
        from: time::OffsetDateTime,
        to: time::OffsetDateTime,
        group_by: &str,
        bucket_seconds: Option<u32>,
    ) -> mcpd_store::Result<mcpd_core::CostBreakdown> {
        self.0.analytics_cost(project, from, to, group_by, bucket_seconds).await
    }
    async fn enforce_retention(
        &self,
        project: &str,
        older_than: time::OffsetDateTime,
    ) -> mcpd_store::Result<u64> {
        self.0.enforce_retention(project, older_than).await
    }
    async fn list_projects(&self) -> mcpd_store::Result<Vec<String>> {
        self.0.list_projects().await
    }
    async fn record_audit(&self, event: &mcpd_core::AuditEvent) -> mcpd_store::Result<()> {
        self.0.record_audit(event).await
    }
    async fn list_audit(
        &self,
        q: &mcpd_store::AuditQuery,
    ) -> mcpd_store::Result<Vec<mcpd_core::AuditEvent>> {
        self.0.list_audit(q).await
    }
    async fn put_dataset(&self, dataset: &mcpd_core::Dataset) -> mcpd_store::Result<()> {
        self.0.put_dataset(dataset).await
    }
    async fn list_datasets(&self, project: &str) -> mcpd_store::Result<Vec<mcpd_core::Dataset>> {
        self.0.list_datasets(project).await
    }
    async fn dataset_project(&self, dataset_id: &str) -> mcpd_store::Result<Option<String>> {
        self.0.dataset_project(dataset_id).await
    }
    async fn add_dataset_items(
        &self,
        dataset_id: &str,
        items: &[mcpd_core::DatasetItem],
    ) -> mcpd_store::Result<u32> {
        self.0.add_dataset_items(dataset_id, items).await
    }
    async fn list_dataset_items(
        &self,
        dataset_id: &str,
        limit: u32,
    ) -> mcpd_store::Result<Vec<mcpd_core::DatasetItem>> {
        self.0.list_dataset_items(dataset_id, limit).await
    }
    async fn put_experiment(&self, experiment: &mcpd_core::Experiment) -> mcpd_store::Result<()> {
        self.0.put_experiment(experiment).await
    }
    async fn list_experiments(
        &self,
        project: &str,
    ) -> mcpd_store::Result<Vec<mcpd_core::Experiment>> {
        self.0.list_experiments(project).await
    }
    async fn put_document(
        &self,
        doc: &mcpd_store::NewDocument,
    ) -> mcpd_store::Result<(mcpd_store::StoredDocument, bool)> {
        self.0.put_document(doc).await
    }
    async fn list_documents(
        &self,
        project: &str,
        kind: mcpd_store::DocKind,
        id: Option<&str>,
    ) -> mcpd_store::Result<Vec<mcpd_store::StoredDocument>> {
        self.0.list_documents(project, kind, id).await
    }
    async fn get_document(
        &self,
        project: &str,
        kind: mcpd_store::DocKind,
        id: &str,
        version: Option<u32>,
    ) -> mcpd_store::Result<Option<mcpd_store::StoredDocument>> {
        self.0.get_document(project, kind, id, version).await
    }
    async fn set_document_label(
        &self,
        project: &str,
        kind: mcpd_store::DocKind,
        id: &str,
        label: &str,
        version: Option<u32>,
    ) -> mcpd_store::Result<bool> {
        self.0.set_document_label(project, kind, id, label, version).await
    }
    async fn list_document_labels(
        &self,
        project: &str,
        kind: mcpd_store::DocKind,
        id: Option<&str>,
    ) -> mcpd_store::Result<Vec<mcpd_store::DocumentLabel>> {
        self.0.list_document_labels(project, kind, id).await
    }
}

fn keyed() -> ServerConfig {
    ServerConfig { read_keys: vec![READ_KEY.to_owned()], ..ServerConfig::default() }
}

/// The router over the real SQLite store, which declares its capabilities.
async fn full_app() -> Router {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    router(Arc::new(AppState::new(store as Arc<dyn Store>, keyed())))
}

/// The router over a backend that declares nothing.
async fn bare_app() -> Router {
    let inner = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    let store = Arc::new(Bare(inner));
    store.migrate().await.expect("migrate the store");
    router(Arc::new(AppState::new(store as Arc<dyn Store>, keyed())))
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
}

async fn get(app: &Router, uri: &str, key: Option<&str>) -> Res {
    let mut b = Request::builder().method("GET").uri(uri);
    if let Some(k) = key {
        b = b.header("authorization", format!("Bearer {k}"));
    }
    let req = b.body(Body::empty()).expect("build the request");
    let res = app.clone().oneshot(req).await.expect("the router answered");
    let status = res.status();
    let body = res.into_body().collect().await.expect("read the body").to_bytes().to_vec();
    Res { status, body }
}

// ------------------------------------------------------ the capability set ---

/// The drift guard between the two declarations of this struct.
///
/// `mcpd_store::StoreCapabilities` is what a backend fills in and
/// `mcpd_core::StoreCapabilities` is what the spec publishes, and the handler
/// copies one into the other field by field. A field added to the store and not
/// to the spec would otherwise vanish on the way out with nothing failing, and
/// the endpoint would quietly under-report what the deployment can do, which is
/// the exact failure it exists to prevent.
#[tokio::test]
async fn the_capability_set_publishes_every_field_the_store_declares() {
    let store = SqliteStore::open(temp_db(), 2).expect("open the store");
    let declared = store.capabilities();
    let app = full_app().await;

    let res = get(&app, "/v1/api/capabilities", Some(READ_KEY)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", String::from_utf8_lossy(&res.body));
    let body = res.json();

    assert_eq!(body["backend"], Value::from(declared.backend), "the backend is misreported");
    for (name, value) in declared.fields() {
        let served = body.get(name).unwrap_or_else(|| {
            panic!("the store declares `{name}` and the capability set does not publish it: {body}")
        });
        let rendered = match served {
            Value::Bool(b) => b.to_string(),
            Value::String(s) => s.clone(),
            other => panic!("`{name}` came back as {other}, which is neither a flag nor a name"),
        };
        assert_eq!(rendered, value, "`{name}` is published as something the store did not say");
    }
}

/// The capability set says which database is behind the daemon, so it is not a
/// fact for anyone who can reach the port.
#[tokio::test]
async fn the_capability_set_needs_a_read_key() {
    let app = full_app().await;
    let res = get(&app, "/v1/api/capabilities", None).await;
    assert_eq!(res.status, StatusCode::UNAUTHORIZED, "the capability set was open to anyone");
}

/// A backend that declares nothing must report nothing, not inherit the
/// answers of the store it happens to be wrapping.
#[tokio::test]
async fn a_backend_that_declares_nothing_reports_nothing() {
    let app = bare_app().await;
    let res = get(&app, "/v1/api/capabilities", Some(READ_KEY)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", String::from_utf8_lossy(&res.body));
    let body = res.json();
    assert_eq!(body["backend"], "bare");
    assert_eq!(body["discovery"], false, "an undeclared backend advertised discovery");
    assert_eq!(body["delta_polling"], false, "an undeclared backend advertised polling");
    assert_eq!(body["text_search"], "none");
}

// ------------------------------------------------------------ the refusals ---

/// Every door whose capability can be false, and the capability it names.
///
/// One list drives the refusal cases, so a door added without a capability
/// check is a door missing from here rather than a silent difference between
/// two backends.
const CAPABILITY_DOORS: &[(&str, &str)] = &[
    ("/v1/api/discovery?facet=environment", "discovery"),
    ("/v1/api/discovery?facet=metric_label_value&key=model", "discovery"),
    ("/v1/api/deltas?signal=traces", "delta_polling"),
];

#[tokio::test]
async fn a_missing_capability_is_a_501_that_names_it() {
    let app = bare_app().await;
    for (route, capability) in CAPABILITY_DOORS {
        let res = get(&app, route, Some(READ_KEY)).await;
        assert_eq!(
            res.status,
            StatusCode::NOT_IMPLEMENTED,
            "{route} did not answer 501 on a backend that cannot serve it: {}",
            String::from_utf8_lossy(&res.body)
        );
        let body = res.json();
        assert_eq!(body["error"], "not_implemented", "{route} returned the wrong code: {body}");
        assert_eq!(
            body["capability"], *capability,
            "{route} refused without naming the capability a caller should have checked: {body}"
        );
        let message = body["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("bare"),
            "{route} refused without saying which backend refused: {message}"
        );
        assert!(
            message.contains("/v1/api/capabilities"),
            "{route} refused without pointing at the list of what does work: {message}"
        );
    }
}

/// The word in the refusal has to be a word the capability set publishes, or a
/// caller cannot act on it. This is the join the two halves promise.
#[tokio::test]
async fn every_capability_a_door_names_is_one_the_set_publishes() {
    let app = bare_app().await;
    let published = get(&app, "/v1/api/capabilities", Some(READ_KEY)).await.json();
    for (route, capability) in CAPABILITY_DOORS {
        assert!(
            published.get(capability).is_some(),
            "{route} refuses naming `{capability}`, which the capability set does not publish"
        );
    }
}

/// The same backend, with the capability present, answers rather than refusing.
/// Without this the suite would pass on a daemon that answered 501 to
/// everything.
#[tokio::test]
async fn a_backend_that_has_the_capability_answers() {
    let app = full_app().await;
    for (route, capability) in CAPABILITY_DOORS {
        let res = get(&app, route, Some(READ_KEY)).await;
        assert_eq!(
            res.status,
            StatusCode::OK,
            "{route} refused on a backend that advertises {capability}: {}",
            String::from_utf8_lossy(&res.body)
        );
    }
}
