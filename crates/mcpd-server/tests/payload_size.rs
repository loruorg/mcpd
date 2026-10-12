//! What a run list costs, against what opening one run costs.
//!
//! mcpd's trace list is light by construction: the stored `Trace` projection
//! carries no input, no output and no attributes at all, so there is no second
//! "light" endpoint to add. What there was no answer to is how much that
//! actually saves, and a number copied from another project's documentation is
//! not an answer about this one.
//!
//! This measures it, on a run shaped like a real one, through the real router.
//! The figure it produces is the figure quoted on the "Finding a run" page. If
//! the projection grows a heavy field, this test is what fails.

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
const TRACE_ID: &str = "0123456789abcdef0123456789abcdef";

/// The floor the documentation quotes, in tenths of a percent.
///
/// Measured, not chosen. The fixture below produced 94.6%, and the floor is set
/// just under it so that ordinary drift in a field name does not fail the
/// build while a projection that starts carrying payload does. Whole percent
/// was not enough resolution: integer division of 577 into 10717 rounds the
/// ratio down and reports the saving as one point better than it is.
const DOCUMENTED_REDUCTION_TENTHS: usize = 940;

fn temp_db() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir = std::env::temp_dir().join(format!("mcpd-size-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir.join(format!("{}.db", N.fetch_add(1, Ordering::Relaxed)))
}

/// A run shaped like one somebody would actually investigate: an agent that
/// answers a support question by retrieving three documents, calling a tool and
/// making two model calls. The prompts and completions are the size real ones
/// are, because a measurement over a one-word prompt would say nothing.
fn realistic_run() -> Value {
    let system = "You are the support assistant for a parcel delivery company. \
        Answer only from the retrieved documents. If the documents do not \
        contain the answer, say so and offer to open a ticket. Never guess a \
        delivery date. Never quote a refund amount that is not in the policy \
        document. Keep answers under four sentences and write in plain English.";
    let question = "My parcel was due on Tuesday and the tracking page has said \
        'at the Leeds depot' since Sunday afternoon. Nobody has called. I paid \
        for next day delivery. What is happening and can I have my money back?";
    let answer = "Your parcel is held at the Leeds depot and has not moved since \
        Sunday, which usually means it was missorted onto the wrong round. I \
        have flagged it for the depot team to put on the next van. Because you \
        paid for next day delivery and it is now three days late, the delivery \
        charge is refundable under the service guarantee. I can start that \
        refund now if you would like.";
    let document = "Service guarantee, section 4. Where a next day service is \
        not delivered by the end of the next working day, the delivery charge \
        is refunded in full on request. The refund is made to the original \
        payment method within five working days. The guarantee does not cover \
        delays caused by weather, industrial action or an incorrect address \
        supplied at the point of sale.";

    let span = |span_id: &str,
                parent: Option<&str>,
                name: &str,
                kind: &str,
                input: Value,
                output: Value,
                attributes: Value| {
        let mut v = json!({
            "trace_id": TRACE_ID,
            "span_id": span_id,
            "name": name,
            "kind": kind,
            "status": "ok",
            "start_time": "2026-01-01T00:00:00Z",
            "end_time": "2026-01-01T00:00:03Z",
            "is_root": parent.is_none(),
            "is_event": false,
            "input": input,
            "output": output,
            "attributes": attributes,
            "correlation": {
                "project": "default",
                "environment": "production",
                "service_name": "support-assistant",
                "session_id": "sess-9f21",
                "thread_id": "thr-4410"
            },
            "received_at": "2026-01-01T00:00:03Z"
        });
        if let Some(parent) = parent {
            v["parent_span_id"] = json!(parent);
        }
        v
    };

    json!({
        "spans": [
            span("aaaaaaaaaaaaaaa1", None, "support.answer", "agent_run",
                json!({ "question": question, "customer_tier": "standard" }),
                json!({ "answer": answer, "refund_offered": true }),
                json!({ "agent.version": "2026.08.3", "agent.tools": ["track", "refund"] })),
            span("aaaaaaaaaaaaaaa2", Some("aaaaaaaaaaaaaaa1"), "retrieve.policy", "retrieval",
                json!({ "query": question, "top_k": 3 }),
                json!({ "documents": [document, document, document] }),
                json!({ "retrieval.index": "policies-v4", "retrieval.scores": [0.91, 0.84, 0.77] })),
            span("aaaaaaaaaaaaaaa3", Some("aaaaaaaaaaaaaaa1"), "track.lookup", "tool_call",
                json!({ "tracking_number": "PD-8842-1190-LD" }),
                json!({ "status": "at_depot", "depot": "Leeds", "since": "2026-01-01T14:02:00Z",
                        "events": [{"at": "2025-12-30T09:00:00Z", "what": "collected"},
                                   {"at": "2025-12-31T22:14:00Z", "what": "arrived Leeds"}] }),
                json!({ "tool.name": "track", "tool.attempt": 1 })),
            span("aaaaaaaaaaaaaaa4", Some("aaaaaaaaaaaaaaa1"), "answer.draft", "model_generation",
                json!({ "messages": [{"role": "system", "content": system},
                                     {"role": "user", "content": question},
                                     {"role": "tool", "content": document}] }),
                json!({ "content": answer, "finish_reason": "stop" }),
                json!({ "gen_ai.request.model": "gpt-4o", "gen_ai.request.temperature": 0.2 })),
            span("aaaaaaaaaaaaaaa5", Some("aaaaaaaaaaaaaaa4"), "answer.draft.step", "model_step",
                json!({ "messages": [{"role": "system", "content": system},
                                     {"role": "user", "content": question}] }),
                json!({ "content": answer }),
                json!({ "gen_ai.response.finish_reason": "stop" })),
            span("aaaaaaaaaaaaaaa6", Some("aaaaaaaaaaaaaaa1"), "answer.check", "model_generation",
                json!({ "messages": [{"role": "system", "content": "Check the draft against the policy."},
                                     {"role": "user", "content": answer},
                                     {"role": "tool", "content": document}] }),
                json!({ "content": "The draft matches section 4.", "finish_reason": "stop" }),
                json!({ "gen_ai.request.model": "gpt-4o-mini" })),
            span("aaaaaaaaaaaaaaa7", Some("aaaaaaaaaaaaaaa1"), "refund.start", "tool_call",
                json!({ "order": "PD-8842-1190-LD", "amount": 6.99, "reason": "guarantee_s4" }),
                json!({ "refund_id": "rf-77120", "state": "pending" }),
                json!({ "tool.name": "refund" })),
            span("aaaaaaaaaaaaaaa8", Some("aaaaaaaaaaaaaaa1"), "audit.write", "tool_call",
                json!({ "event": "refund_offered", "actor": "support-assistant" }),
                json!({ "written": true }),
                json!({ "tool.name": "audit" }))
        ]
    })
}

async fn app_with_run() -> Router {
    let store = Arc::new(SqliteStore::open(temp_db(), 2).expect("open the store"));
    store.migrate().await.expect("migrate the store");
    let config = ServerConfig {
        read_keys: vec![READ_KEY.to_owned()],
        ingest_keys: vec![INGEST_KEY.to_owned()],
        ..ServerConfig::default()
    };
    let app = router(Arc::new(AppState::new(store as Arc<dyn Store>, config)));
    let body = realistic_run().to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/ingest")
        .header("content-type", "application/json")
        .header("content-length", body.len().to_string())
        .header("authorization", format!("Bearer {INGEST_KEY}"))
        .body(Body::from(body))
        .expect("build the request");
    let res = app.clone().oneshot(req).await.expect("the router answered");
    assert_eq!(res.status(), StatusCode::OK, "seeding the run failed");
    app
}

async fn body_of(app: &Router, uri: &str) -> Vec<u8> {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .header("authorization", format!("Bearer {READ_KEY}"))
        .body(Body::empty())
        .expect("build the request");
    let res = app.clone().oneshot(req).await.expect("the router answered");
    assert_eq!(res.status(), StatusCode::OK, "{uri} was refused");
    res.into_body().collect().await.expect("read the body").to_bytes().to_vec()
}

/// The measurement the documentation quotes.
///
/// One row of the run list against the whole run, on the same data, over the
/// real router. The list row is the materialised trace projection: names,
/// counts, timings, cost, tags and correlation, and nothing a model said. The
/// full run is every step with its input, output and attributes, which is what
/// a waterfall needs and what a list does not.
#[tokio::test]
async fn a_run_list_row_is_a_fraction_of_the_run_it_names() {
    let app = app_with_run().await;
    let list = body_of(&app, "/v1/api/traces?limit=1").await;
    let detail = body_of(&app, &format!("/v1/api/traces/{TRACE_ID}")).await;

    // The list carries the page envelope as well as the row. Both are what a
    // caller actually receives, so both are counted.
    let list_bytes = list.len();
    let detail_bytes = detail.len();
    assert!(detail_bytes > list_bytes, "the run was not larger than its list row");
    // Tenths, and the division rounds the ratio UP before subtracting, so the
    // reported saving is never better than the measurement.
    let ratio_tenths = (list_bytes * 1000).div_ceil(detail_bytes);
    let reduction = 1000 - ratio_tenths;

    // Printed, because the number is the point of the test and a reader
    // checking the documentation should be able to run one command and see it.
    println!(
        "run list row: {list_bytes} bytes; the whole run: {detail_bytes} bytes; \
         reduction: {}.{}%",
        reduction / 10,
        reduction % 10
    );

    // Sanity: the list must really be carrying the run, not an empty page.
    let listed: Value = serde_json::from_slice(&list).expect("the list is JSON");
    assert_eq!(listed["traces"][0]["trace_id"], TRACE_ID);
    assert_eq!(listed["traces"][0]["span_count"], 8);

    assert!(
        reduction >= DOCUMENTED_REDUCTION_TENTHS,
        "the documented reduction is {}.{}% and this run measures {}.{}%. Either the \
         projection has started carrying payload, or the number on \
         web/src/pages/docs/searching.astro needs re-measuring.",
        DOCUMENTED_REDUCTION_TENTHS / 10,
        DOCUMENTED_REDUCTION_TENTHS % 10,
        reduction / 10,
        reduction % 10
    );
}

/// The reason the list is small: the row carries none of the three heavy
/// fields. Asserted by name, so a field added to the projection that happens to
/// be small today still fails here rather than growing until the measurement
/// above notices.
#[tokio::test]
async fn a_run_list_row_carries_no_payload_at_all() {
    let app = app_with_run().await;
    let list: Value =
        serde_json::from_slice(&body_of(&app, "/v1/api/traces?limit=1").await).expect("JSON");
    let row = &list["traces"][0];
    for heavy in ["input", "output", "attributes"] {
        assert!(
            row.get(heavy).is_none(),
            "the run list row carries `{heavy}`, which is what makes a list expensive: {row}"
        );
    }
    // What it does carry is what a list needs to be useful without opening a
    // run: what it was, how it went, how long, how much, and how to find it.
    for needed in ["trace_id", "name", "kind", "status", "start_time", "span_count", "correlation"]
    {
        assert!(row.get(needed).is_some(), "the run list row has no `{needed}`: {row}");
    }
}
