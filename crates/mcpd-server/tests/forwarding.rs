//! Forwarding, through the real ingest door, against destinations that answer
//! the way real ones do.
//!
//! Every destination here is a scripted HTTP server on a port the OS assigns.
//! Each case reads back what the destination received and what the daemon
//! says about it in `/metrics`, which is the only place an operator can see a
//! copy that is late or missing.

mod support;

use axum::http::StatusCode;
use mcpd_server::forward::{Destination, Settings, Vendor};
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::Store;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use support::{get, post, sample, temp_dir, Mock};

const WAIT: Duration = Duration::from_secs(10);

async fn state_with(
    destinations: Vec<Destination>,
    spool_dir: Option<std::path::PathBuf>,
    spool_max_bytes: u64,
) -> Arc<AppState> {
    let store = SqliteStore::open(temp_dir("fwd-db").join("mcpd.db"), 2).expect("open the store");
    store.migrate().await.expect("migrate the store");
    let config = ServerConfig {
        forward: Settings { destinations, spool_dir, spool_max_bytes },
        ..ServerConfig::default()
    };
    Arc::new(AppState::new(Arc::new(store) as Arc<dyn Store>, config))
}

fn otlp(url: &str) -> Destination {
    let mut d = Destination::new(Vendor::Otlp, Some(url.to_owned()));
    d.headers = vec![("x-destination-key".to_owned(), "abc".to_owned())];
    d
}

fn named(mut d: Destination, name: &str) -> Destination {
    d.name = Some(name.to_owned());
    d
}

/// A span in its wire shape. Ids are numbered from one; all zeros is invalid.
fn span(i: u32, project: &str, kind: &str) -> Value {
    json!({
        "trace_id": format!("{i:032x}"),
        "span_id": format!("{i:016x}"),
        "name": format!("step {i}"),
        "kind": kind,
        "status": "ok",
        "start_time": "2026-01-01T00:00:00Z",
        "end_time": "2026-01-01T00:00:01Z",
        "is_root": true,
        "is_event": false,
        "input": { "q": "hello", "account_number": "12345678" },
        "output": "hi",
        "correlation": { "project": project, "environment": "production" },
        "received_at": "2026-01-01T00:00:01Z"
    })
}

fn spans(n: u32) -> Vec<Value> {
    (1..=n).map(|i| span(i, "default", "agent_run")).collect()
}

async fn ingest(state: &Arc<AppState>, batch: Value) {
    let app = router(Arc::clone(state));
    let (status, body) =
        post(&app, "/v1/ingest", "application/json", &[], batch.to_string().into_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn metrics(state: &Arc<AppState>) -> String {
    get(&router(Arc::clone(state)), "/metrics").await.1
}

fn spans_in(body: &[u8]) -> Vec<Value> {
    let v: Value = serde_json::from_slice(body).expect("an OTLP JSON body");
    v["resourceSpans"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|r| r["scopeSpans"].as_array().cloned().unwrap_or_default())
        .flat_map(|s| s["spans"].as_array().cloned().unwrap_or_default())
        .collect()
}

fn attr<'a>(span: &'a Value, key: &str) -> Option<&'a Value> {
    span["attributes"].as_array()?.iter().find(|kv| kv["key"] == key).map(|kv| &kv["value"])
}

fn series(name: &str, dest: &str, vendor: &str) -> String {
    format!("{name}{{destination=\"{dest}\",vendor=\"{vendor}\"}}")
}

// ------------------------------------------------------------- the cases ---

#[tokio::test]
async fn a_batch_the_destination_accepts_is_delivered_once_with_its_headers() {
    let dest = Mock::start(&[]).await;
    let state = state_with(vec![otlp(&dest.url)], None, 1 << 20).await;
    ingest(&state, json!({ "spans": spans(3) })).await;

    let got = dest.wait_accepted(1, WAIT).await;
    assert_eq!(got[0].path, "/v1/traces");
    assert_eq!(got[0].header("x-destination-key"), Some("abc"));
    assert_eq!(spans_in(&got[0].body).len(), 3);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(dest.seen().len(), 1, "delivered once");
    let m = metrics(&state).await;
    assert_eq!(sample(&m, &series("mcpd_forward_sent_total", "otlp", "otlp")), Some(3.0), "{m}");
    assert_eq!(sample(&m, &series("mcpd_forward_up", "otlp", "otlp")), Some(1.0));
    assert_eq!(sample(&m, "mcpd_forward_dropped_spans_total"), Some(0.0));
}

/// A 5xx and a 429 are what a destination answers while busy. The batch stays
/// in the spool and is retried, and a `Retry-After` is waited out.
#[tokio::test]
async fn a_5xx_and_a_429_are_retried_and_the_retry_after_is_honoured() {
    let dest = Mock::start(&[(503, None), (429, Some(1))]).await;
    let state = state_with(vec![otlp(&dest.url)], None, 1 << 20).await;
    ingest(&state, json!({ "spans": spans(4) })).await;

    dest.wait_accepted(1, WAIT).await;
    let seen = dest.seen();
    assert_eq!(seen.len(), 3, "a 503, a 429, then the 200");
    assert!(
        seen[2].at - seen[1].at >= Duration::from_millis(950),
        "the Retry-After of one second was not honoured: {:?}",
        seen[2].at - seen[1].at
    );
    let m = metrics(&state).await;
    assert_eq!(
        sample(&m, &series("mcpd_forward_failed_attempts_total", "otlp", "otlp")),
        Some(2.0)
    );
    assert_eq!(sample(&m, &series("mcpd_forward_dead_letter_total", "otlp", "otlp")), Some(0.0));
}

/// A 400 says the batch is wrong for this destination and always will be.
/// It is moved to `dead/` at once, counted by span, and never retried.
#[tokio::test]
async fn a_400_is_final_moved_to_dead_letters_and_counted_by_span() {
    let dest = Mock::start(&[(400, None)]).await;
    let spool = temp_dir("fwd-spool");
    let state = state_with(vec![otlp(&dest.url)], Some(spool.clone()), 1 << 20).await;
    ingest(&state, json!({ "spans": spans(5) })).await;

    dest.wait_seen(1, WAIT).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(dest.seen().len(), 1, "a 400 was retried");
    let m = metrics(&state).await;
    assert_eq!(
        sample(&m, &series("mcpd_forward_dead_letter_total", "otlp", "otlp")),
        Some(5.0),
        "{m}"
    );
    assert_eq!(sample(&m, "mcpd_forward_dropped_spans_total"), Some(5.0));
    let dead = std::fs::read_dir(spool.join("forward-otlp").join("dead")).expect("a dead/ dir");
    assert_eq!(dead.count(), 1, "the refused batch is kept for reading");
}

/// The spool is what makes a restart safe: batches a destination did not
/// take before the stop are on disk, and the next process sends them.
#[tokio::test]
async fn a_backlog_on_disk_survives_a_restart_and_is_sent_by_the_next_process() {
    let spool = temp_dir("fwd-restart");
    let dead = format!("http://127.0.0.1:{}", support::dead_port());
    {
        let state =
            state_with(vec![named(otlp(&dead), "primary")], Some(spool.clone()), 1 << 20).await;
        ingest(&state, json!({ "spans": spans(2) })).await;
        ingest(&state, json!({ "spans": [span(9, "default", "tool_call")] })).await;
        // Let the workers fail at least once.
        tokio::time::sleep(Duration::from_millis(400)).await;
        let m = metrics(&state).await;
        assert_eq!(sample(&m, &series("mcpd_forward_up", "primary", "otlp")), Some(0.0), "{m}");
        assert_eq!(sample(&m, &series("mcpd_forward_spool_entries", "primary", "otlp")), Some(2.0));
        assert_eq!(state.drain(Duration::from_secs(2)).await, 0);
    }
    let dest = Mock::start(&[]).await;
    let state = state_with(vec![named(otlp(&dest.url), "primary")], Some(spool), 1 << 20).await;
    mcpd_server::forward::spawn(&state);
    let got = dest.wait_accepted(2, WAIT).await;
    let total: usize = got.iter().map(|s| spans_in(&s.body).len()).sum();
    assert_eq!(total, 3, "every span stored before the restart reached the destination");
}

/// A destination that is down fills its own spool, and only its own. Past
/// the bound a copy is dropped and counted, and the ingest still succeeds.
#[tokio::test]
async fn a_full_spool_drops_and_counts_the_copy_and_never_fails_the_ingest() {
    let dead = format!("http://127.0.0.1:{}", support::dead_port());
    let state = state_with(vec![otlp(&dead)], None, 600).await;
    for i in 1..=6 {
        ingest(&state, json!({ "spans": [span(i, "default", "agent_run")] })).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let m = metrics(&state).await;
    let full =
        sample(&m, &series("mcpd_forward_spool_full_total", "otlp", "otlp")).expect("the counter");
    assert!(full >= 1.0, "{m}");
    assert_eq!(sample(&m, "mcpd_forward_dropped_spans_total"), Some(full));
}

/// Several destinations at once, each with its own filter.
#[tokio::test]
async fn each_destination_is_sent_only_what_its_filter_keeps() {
    let (a, b, c) = (Mock::start(&[]).await, Mock::start(&[]).await, Mock::start(&[]).await);
    let mut by_project = named(otlp(&a.url), "by-project");
    by_project.filter.projects = vec!["checkout".into()];
    let mut by_kind = named(otlp(&b.url), "by-kind");
    by_kind.filter.kinds = vec![mcpd_core::SpanKind::ToolCall];
    let mut none = named(otlp(&c.url), "sampled-out");
    none.filter.sample = Some(0.0);
    let state = state_with(vec![by_project, by_kind, none], None, 1 << 20).await;

    ingest(
        &state,
        json!({ "spans": [
            span(1, "checkout", "agent_run"),
            span(2, "support", "tool_call"),
            span(3, "checkout", "tool_call"),
        ] }),
    )
    .await;

    let got_a = a.wait_accepted(1, WAIT).await;
    let got_b = b.wait_accepted(1, WAIT).await;
    let names = |s: &[support::Seen]| {
        let mut n: Vec<String> = s
            .iter()
            .flat_map(|x| spans_in(&x.body))
            .map(|s| s["name"].as_str().unwrap_or_default().to_owned())
            .collect();
        n.sort();
        n
    };
    assert_eq!(names(&got_a), vec!["step 1", "step 3"]);
    assert_eq!(names(&got_b), vec!["step 2", "step 3"]);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(c.seen().is_empty(), "a sample of 0 sends nothing");
    let m = metrics(&state).await;
    assert_eq!(
        sample(&m, &series("mcpd_forward_filtered_total", "sampled-out", "otlp")),
        Some(3.0)
    );
}

/// Redaction per destination, on the copy only. The stored span keeps what the
/// daemon's own redactor left in it.
#[tokio::test]
async fn redaction_applies_to_one_destinations_copy_and_never_to_the_stored_span() {
    let (a, b) = (Mock::start(&[]).await, Mock::start(&[]).await);
    let mut no_payloads = named(otlp(&a.url), "no-payloads");
    no_payloads.redact.drop_payloads = true;
    let mut masked = named(otlp(&b.url), "masked");
    masked.redact.key_contains = vec!["account".into()];
    let state = state_with(vec![no_payloads, masked], None, 1 << 20).await;
    ingest(&state, json!({ "spans": [span(1, "default", "agent_run")] })).await;

    let got_a = spans_in(&a.wait_accepted(1, WAIT).await[0].body);
    assert!(attr(&got_a[0], "input.value").is_none(), "{:?}", got_a[0]);
    assert!(attr(&got_a[0], "output.value").is_none());
    let got_b = spans_in(&b.wait_accepted(1, WAIT).await[0].body);
    let input = attr(&got_b[0], "input.value").expect("input kept")["stringValue"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(input.contains("[redacted]") && !input.contains("12345678"), "{input}");

    let stored = state
        .store
        .get_trace("default", &format!("{:032x}", 1), &mcpd_store::Include::default())
        .await
        .expect("the trace");
    let kept = serde_json::to_string(&stored.spans[0].input).expect("json");
    assert!(kept.contains("12345678"), "the stored copy was redacted for a destination: {kept}");
}

/// Links and events used to be dropped by the forwarder. A link goes out as an
/// OTLP link; an event goes back onto the span that carried it.
#[tokio::test]
async fn links_and_events_are_forwarded() {
    let dest = Mock::start(&[]).await;
    let state = state_with(vec![otlp(&dest.url)], None, 1 << 20).await;
    let mut carrier = span(1, "default", "model_generation");
    carrier["links"] = json!([{
        "trace_id": "5bf92f3577b34da6a3ce929d0e0e4736",
        "span_id": "10f067aa0ba902b7",
        "kind": "retry_of",
    }]);
    let event = json!({
        "trace_id": format!("{:032x}", 1),
        "span_id": "00000000000000ee",
        "parent_span_id": format!("{:016x}", 1),
        "name": "first_token",
        "kind": "model_chunk",
        "status": "unset",
        "start_time": "2026-01-01T00:00:00.5Z",
        "is_root": false,
        "is_event": true,
        "correlation": { "project": "default" },
        "received_at": "2026-01-01T00:00:01Z"
    });
    ingest(&state, json!({ "spans": [carrier, event] })).await;

    let got = spans_in(&dest.wait_accepted(1, WAIT).await[0].body);
    assert_eq!(got.len(), 1, "the event is folded into its carrier: {got:?}");
    let links = got[0]["links"].as_array().expect("links");
    assert_eq!(links[0]["traceId"], "5bf92f3577b34da6a3ce929d0e0e4736");
    assert!(links[0]["attributes"].to_string().contains("retry_of"));
    let events = got[0]["events"].as_array().expect("events");
    assert_eq!(events[0]["name"], "first_token");
    assert_eq!(events[0]["timeUnixNano"], "1767225600500000000");
}

/// Logs and the metric points a sender sent go to their own OTLP paths. The
/// points the daemon derives from spans are not forwarded: they restate the
/// spans, and a destination would count everything twice.
#[tokio::test]
async fn logs_and_sent_metrics_are_forwarded_and_derived_metrics_are_not() {
    let dest = Mock::start(&[]).await;
    let state = state_with(vec![otlp(&dest.url)], None, 1 << 20).await;
    ingest(
        &state,
        json!({
            "spans": [span(1, "default", "model_generation")],
            "logs": [{
                "log_id": "log-1",
                "trace_id": format!("{:032x}", 1),
                "timestamp": "2026-01-01T00:00:00Z",
                "level": "warn",
                "message": "slow tool",
                "fields": { "tool": "search" },
                "logger": "agent",
                "correlation": { "project": "default" },
                "received_at": "2026-01-01T00:00:01Z"
            }],
            "metrics": [{
                "metric_id": "m-1",
                "name": "queue.depth",
                "kind": "gauge",
                "value": 7.0,
                "timestamp": "2026-01-01T00:00:00Z",
                "labels": { "queue": "a" },
                "correlation": { "project": "default" },
                "received_at": "2026-01-01T00:00:01Z"
            }]
        }),
    )
    .await;
    let got = dest.wait_accepted(3, WAIT).await;
    let by_path = |p: &str| got.iter().find(|s| s.path == p).unwrap_or_else(|| panic!("no {p}"));
    let logs: Value = serde_json::from_slice(&by_path("/v1/logs").body).expect("json");
    let rec = &logs["resourceLogs"][0]["scopeLogs"][0];
    assert_eq!(rec["scope"]["name"], "agent");
    assert_eq!(rec["logRecords"][0]["body"]["stringValue"], "slow tool");
    assert_eq!(rec["logRecords"][0]["severityNumber"], 13);
    let metrics: Value = serde_json::from_slice(&by_path("/v1/metrics").body).expect("json");
    let all =
        metrics["resourceMetrics"][0]["scopeMetrics"][0]["metrics"].as_array().expect("metrics");
    assert_eq!(all.len(), 1, "only the point the sender sent: {all:?}");
    assert_eq!(all[0]["name"], "queue.depth");
    assert_eq!(all[0]["gauge"]["dataPoints"][0]["asDouble"], 7.0);
}

/// OTLP/gRPC: the protobuf export in gRPC's framing, to the service path,
/// with the status read from `grpc-status`.
#[tokio::test]
async fn grpc_destinations_are_sent_a_framed_protobuf_export() {
    let dest = Mock::start(&[]).await;
    let d = Destination::new(Vendor::OtlpGrpc, Some(dest.url.clone()));
    let state = state_with(vec![d], None, 1 << 20).await;
    ingest(&state, json!({ "spans": spans(2) })).await;
    let got = dest.wait_accepted(1, WAIT).await;
    assert_eq!(got[0].path, "/opentelemetry.proto.collector.trace.v1.TraceService/Export");
    assert_eq!(got[0].header("content-type"), Some("application/grpc"));
    assert_eq!(got[0].body[0], 0, "uncompressed frame");
    let len = u32::from_be_bytes(got[0].body[1..5].try_into().unwrap()) as usize;
    assert_eq!(len, got[0].body.len() - 5);
    let decoded = mcpd_core::otlp::protobuf::decode_traces(got[0].payload()).expect("protobuf");
    assert_eq!(decoded.len(), 2);
}

/// A memory spool cannot outlive the process, so on a stop it gets one
/// attempt at whatever it holds, inside the grace period.
#[tokio::test]
async fn a_memory_spool_is_flushed_once_when_the_daemon_stops() {
    let dest = Mock::start(&[(503, None)]).await;
    let state = state_with(vec![otlp(&dest.url)], None, 1 << 20).await;
    ingest(&state, json!({ "spans": spans(3) })).await;
    dest.wait_seen(1, WAIT).await;
    assert_eq!(state.drain(Duration::from_secs(5)).await, 0);
    assert_eq!(dest.accepted().len(), 1, "the final pass delivered the batch");
    assert_eq!(state.metrics.forward_dropped_spans.load(Ordering::Relaxed), 0);
}

/// The bound on copies waiting to be spooled still holds, and the counter it
/// feeds is still rendered.
#[tokio::test]
async fn the_drop_counter_is_rendered_and_starts_at_zero() {
    let dest = Mock::start(&[]).await;
    let state = state_with(vec![otlp(&dest.url)], None, 1 << 20).await;
    let m = metrics(&state).await;
    assert_eq!(sample(&m, "mcpd_forward_dropped_spans_total"), Some(0.0));
    assert_eq!(state.forward_slots.available_permits(), mcpd_server::forward::MAX_IN_FLIGHT);
}
