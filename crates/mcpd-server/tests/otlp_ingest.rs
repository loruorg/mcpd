//! What the OTLP door actually stores.
//!
//! `crates/mcpd-core/tests/otlp_wire.rs` covers the mapping. This file covers
//! the rest of the journey: a real `POST /v1/traces`, through authorisation,
//! the intake pipeline and into SQLite, read back through the store rather than
//! from the response, because the response is what the handler believed and the
//! row is what landed.
//!
//! The four fields under test all used to be dropped at the door: span links,
//! OpenTelemetry span events, the trace-flags byte, and the
//! `external_parent_span_id` that only a whole request can decide.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::{Include, Store};
use serde_json::{json, Value};
use std::sync::Arc;

const TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
const SPAN: &str = "00f067aa0ba902b7";
const PARENT: &str = "7d3efb1a2c4b5e60";
const OTHER_TRACE: &str = "9c1b0e5a7f2d43886ae1c0d9b4f37521";

/// A private database file per test.
///
/// Named from the process **and** the instant it started. A name built from the
/// pid alone is reused once the operating system recycles one, and the second
/// run then opens the first run's database, so every count is off by a whole
/// previous suite. Copied from `http.rs`, which is where that was learned;
/// Cargo builds each integration test as its own crate, so sharing it would
/// mean a module this change does not own.
fn temp_db() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::OnceLock;
    static N: AtomicU64 = AtomicU64::new(0);
    static DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
    let dir = DIR.get_or_init(|| {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir =
            std::env::temp_dir().join(format!("mcpd-otlp-test-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create the test directory");
        dir
    });
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

async fn app_and_store() -> (Router, Arc<SqliteStore>) {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let state =
        Arc::new(AppState::new(Arc::clone(&store) as Arc<dyn Store>, ServerConfig::default()));
    (router(state), store)
}

struct Res {
    status: StatusCode,
    body: Vec<u8>,
}

impl Res {
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

fn traces(content_type: &str, body: impl Into<Vec<u8>>) -> Request<Body> {
    let body: Vec<u8> = body.into();
    Request::builder()
        .method("POST")
        .uri("/v1/traces")
        .header("content-type", content_type)
        .header("content-length", body.len().to_string())
        .body(Body::from(body))
        .expect("build the request")
}

use tower::ServiceExt;

/// Every span stored under the trace, oldest first.
async fn stored(store: &Arc<SqliteStore>) -> Vec<mcpd_core::Span> {
    let mut spans = store
        .get_trace("default", TRACE, &Include::default())
        .await
        .expect("read the run back")
        .spans;
    spans.sort_by_key(|s| s.start_time);
    spans
}

/// A span with an event, a link, a remote parent and the sampled bit set: the
/// four things the door used to throw away, in one export.
fn export() -> String {
    json!({
        "resourceSpans": [{
            "resource": { "attributes": [
                { "key": "service.name", "value": { "stringValue": "checkout-agent" } }
            ]},
            "scopeSpans": [{
                "spans": [{
                    "traceId": TRACE,
                    "spanId": SPAN,
                    "parentSpanId": PARENT,
                    // has_is_remote | is_remote | sampled
                    "flags": 0x301,
                    "name": "chat gpt-4o",
                    "startTimeUnixNano": "1767225600000000000",
                    "endTimeUnixNano": "1767225601000000000",
                    "status": { "code": 2, "message": "the provider refused" },
                    "attributes": [
                        { "key": "gen_ai.system", "value": { "stringValue": "openai" } }
                    ],
                    "links": [{
                        "traceId": OTHER_TRACE,
                        "spanId": PARENT,
                        "attributes": [
                            { "key": "mcpd.link.kind", "value": { "stringValue": "retry_of" } }
                        ]
                    }],
                    "events": [{
                        "timeUnixNano": "1767225600500000000",
                        "name": "exception",
                        "attributes": [
                            { "key": "exception.type",
                              "value": { "stringValue": "RateLimitError" } },
                            { "key": "exception.stacktrace",
                              "value": { "stringValue": "provider.py:88 in chat" } }
                        ]
                    }]
                }]
            }]
        }]
    })
    .to_string()
}

#[tokio::test]
async fn an_export_with_a_link_and_an_event_stores_both() {
    let (app, store) = app_and_store().await;
    let res = send(&app, traces("application/json", export())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let spans = stored(&store).await;
    assert_eq!(spans.len(), 2, "the event should be stored as a span of its own");

    let call = spans.iter().find(|s| s.span_id == SPAN).expect("the model call");
    let links = call.links.as_ref().expect("the link never reached storage");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].trace_id, OTHER_TRACE);
    assert_eq!(links[0].kind.as_deref(), Some("retry_of"));

    // The exception event carries the stack, and the failing span is where a
    // reader looks for it.
    let error = call.error.as_ref().expect("an error span carries an error");
    assert_eq!(error.message, "the provider refused");
    assert_eq!(error.kind.as_deref(), Some("RateLimitError"));
    assert_eq!(error.stack.as_deref(), Some("provider.py:88 in chat"));

    let event = spans.iter().find(|s| s.is_event).expect("the event span");
    assert_eq!(event.name, "exception");
    assert_eq!(event.parent_span_id.as_deref(), Some(SPAN));
    assert_eq!(event.trace_id, TRACE);
    assert_eq!(event.end_time, None, "an event is a point in time");
    assert_eq!(event.duration_ns, None);
    assert_eq!(event.correlation.project, "default");
    assert_eq!(
        event.correlation.service_name.as_deref(),
        Some("checkout-agent"),
        "an event belongs to the same service as the span that carried it"
    );

    // And the flags byte survives, which is what lets the sampler honour an
    // upstream decision. The parent was declared remote and never arrived, so
    // it is recorded as foreign while the tree is left alone.
    assert_eq!(mcpd_core::otlp::upstream_sampled(call), Some(true));
    assert_eq!(call.external_parent_span_id.as_deref(), Some(PARENT));
    assert_eq!(call.parent_span_id.as_deref(), Some(PARENT));
}

#[tokio::test]
async fn resending_an_export_does_not_store_the_events_twice() {
    // The traces door derives no idempotency key: it relies on every store
    // upserting a span on its id. An event span has an id mcpd made up, so
    // that only holds if the id is derived from the export rather than drawn
    // at random, which is the whole reason it is a digest.
    let (app, store) = app_and_store().await;
    for _ in 0..3 {
        let res = send(&app, traces("application/json", export())).await;
        assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    }

    let spans = stored(&store).await;
    assert_eq!(spans.len(), 2, "a retried export stored a second copy of the event");

    let detail =
        store.get_trace("default", TRACE, &Include::default()).await.expect("read the run back");
    assert_eq!(detail.trace.span_count, 2);
}

#[tokio::test]
async fn the_protobuf_door_stores_the_same_thing_as_the_json_one() {
    // Both encodings lower into one shape and are mapped by one function, and
    // this is the check that the claim survives the door rather than stopping
    // at the decoder's own tests.
    let (app, store) = app_and_store().await;
    let res = send(&app, traces("application/x-protobuf", protobuf_export())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let spans = stored(&store).await;
    assert_eq!(spans.len(), 2);
    let call = spans.iter().find(|s| s.span_id == SPAN).expect("the model call");
    assert_eq!(call.links.as_ref().expect("links")[0].trace_id, OTHER_TRACE);
    assert_eq!(call.external_parent_span_id.as_deref(), Some(PARENT));
    assert_eq!(mcpd_core::otlp::upstream_sampled(call), Some(true));

    let event = spans.iter().find(|s| s.is_event).expect("the event span");
    assert_eq!(event.name, "exception");
    assert_eq!(event.parent_span_id.as_deref(), Some(SPAN));
    assert_eq!(
        call.error.as_ref().expect("error").stack.as_deref(),
        Some("provider.py:88 in chat")
    );
}

/// The same export, on the protobuf wire.
fn protobuf_export() -> Vec<u8> {
    fn varint(out: &mut Vec<u8>, mut v: u64) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }
    fn bytes(out: &mut Vec<u8>, field: u32, v: &[u8]) {
        varint(out, u64::from(field << 3 | 2));
        varint(out, v.len() as u64);
        out.extend_from_slice(v);
    }
    fn string(out: &mut Vec<u8>, field: u32, v: &str) {
        bytes(out, field, v.as_bytes());
    }
    fn fixed64(out: &mut Vec<u8>, field: u32, v: u64) {
        varint(out, u64::from(field << 3 | 1));
        out.extend_from_slice(&v.to_le_bytes());
    }
    fn fixed32(out: &mut Vec<u8>, field: u32, v: u32) {
        varint(out, u64::from(field << 3 | 5));
        out.extend_from_slice(&v.to_le_bytes());
    }
    fn uint(out: &mut Vec<u8>, field: u32, v: u64) {
        varint(out, u64::from(field << 3));
        varint(out, v);
    }
    fn kv(key: &str, value: &str) -> Vec<u8> {
        let mut any = Vec::new();
        string(&mut any, 1, value);
        let mut out = Vec::new();
        string(&mut out, 1, key);
        bytes(&mut out, 2, &any);
        out
    }

    let trace = hex::decode(TRACE).expect("hex");
    let span_id = hex::decode(SPAN).expect("hex");
    let parent = hex::decode(PARENT).expect("hex");
    let other = hex::decode(OTHER_TRACE).expect("hex");

    let mut event = Vec::new();
    fixed64(&mut event, 1, 1_767_225_600_500_000_000);
    string(&mut event, 2, "exception");
    bytes(&mut event, 3, &kv("exception.type", "RateLimitError"));
    bytes(&mut event, 3, &kv("exception.stacktrace", "provider.py:88 in chat"));

    let mut link = Vec::new();
    bytes(&mut link, 1, &other);
    bytes(&mut link, 2, &parent);
    bytes(&mut link, 4, &kv("mcpd.link.kind", "retry_of"));

    let mut status = Vec::new();
    string(&mut status, 2, "the provider refused");
    uint(&mut status, 3, 2);

    let mut span = Vec::new();
    bytes(&mut span, 1, &trace);
    bytes(&mut span, 2, &span_id);
    bytes(&mut span, 4, &parent);
    string(&mut span, 5, "chat gpt-4o");
    fixed64(&mut span, 7, 1_767_225_600_000_000_000);
    fixed64(&mut span, 8, 1_767_225_601_000_000_000);
    bytes(&mut span, 9, &kv("gen_ai.system", "openai"));
    bytes(&mut span, 11, &event);
    bytes(&mut span, 13, &link);
    bytes(&mut span, 15, &status);
    fixed32(&mut span, 16, 0x301);

    let mut scope = Vec::new();
    bytes(&mut scope, 2, &span);

    let mut resource = Vec::new();
    bytes(&mut resource, 1, &kv("service.name", "checkout-agent"));

    let mut resource_spans = Vec::new();
    bytes(&mut resource_spans, 1, &resource);
    bytes(&mut resource_spans, 2, &scope);

    let mut out = Vec::new();
    bytes(&mut out, 1, &resource_spans);
    out
}

#[tokio::test]
async fn an_export_with_no_flags_stores_no_flags_attribute() {
    // The attribute is the one thing mcpd adds to a producer's own, and it
    // exists only to carry a value the model has no column for. A producer
    // that said nothing must not have something invented for it.
    let (app, store) = app_and_store().await;
    let body = json!({ "resourceSpans": [{ "scopeSpans": [{ "spans": [{
        "traceId": TRACE, "spanId": SPAN, "name": "checkout",
        "startTimeUnixNano": "1767225600000000000",
        "endTimeUnixNano": "1767225601000000000"
    }]}]}]})
    .to_string();
    let res = send(&app, traces("application/json", body)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    let spans = stored(&store).await;
    assert_eq!(spans.len(), 1);
    let attributes = spans[0].attributes.as_ref().expect("attributes");
    assert_eq!(attributes.get(mcpd_core::otlp::FLAGS_ATTRIBUTE), None);
    assert_eq!(mcpd_core::otlp::upstream_sampled(&spans[0]), None);
    assert_eq!(spans[0].external_parent_span_id, None);
}

#[tokio::test]
async fn the_response_still_counts_only_what_the_sender_sent() {
    // OTLP's `partialSuccess` is read by senders. Events become spans of their
    // own inside mcpd, and that must not turn into a rejection count the sender
    // cannot reconcile with what it exported.
    let (app, _store) = app_and_store().await;
    let res = send(&app, traces("application/json", export())).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body: Value = serde_json::from_slice(&res.body).expect("a JSON body");
    assert_eq!(body["partialSuccess"]["rejectedSpans"], json!(0), "{body}");
}
