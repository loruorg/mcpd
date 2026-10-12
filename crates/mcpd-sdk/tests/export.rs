//! What the SDK puts on the wire, and what it does when the wire fails.

mod common;

use common::{FakeMcpd, Reply};
use mcpd_sdk::{Config, FeedbackKind, Mcpd, Scale, SpanKind, SpanStatus, Target};
use std::time::Duration;

/// A client pointed at `endpoint`, with a small queue and a long flush interval
/// so every test controls exactly when an export happens.
fn client(endpoint: String) -> Mcpd {
    Mcpd::with_config(Config {
        endpoint,
        project: "shop".to_owned(),
        batch_size: 64,
        flush_interval: Duration::from_secs(3600),
        max_queue: 1024,
        timeout: Duration::from_secs(5),
        ..Config::default()
    })
}

#[tokio::test]
async fn a_run_and_its_children_go_out_in_one_batch() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("support agent");
    run.child_of("lookup_order", SpanKind::ToolCall).end();
    run.child_of("answer", SpanKind::ModelGeneration).end();
    run.end();
    mcpd.flush().await.expect("export should have succeeded");

    assert_eq!(server.count(), 1, "one flush should be one request");
    let request = &server.requests()[0];
    assert_eq!(request.method, "POST");
    assert_eq!(request.path(), "/v1/ingest");
    assert_eq!(request.header("content-type"), Some("application/json"));

    // Three spans, each sent twice: once when it started and once when it ended.
    let spans = request.spans();
    assert_eq!(spans.len(), 6, "expected a start and an end for each of three spans");
    let names: Vec<&str> = spans.iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"support agent"));
    assert!(names.contains(&"lookup_order"));
    assert!(names.contains(&"answer"));
}

#[tokio::test]
async fn an_unfinished_span_is_sent_before_it_ends() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("long running");
    mcpd.flush().await.expect("export should have succeeded");

    let started = server.all_spans();
    assert_eq!(started.len(), 1);
    assert!(started[0]["end_time"].is_null(), "an open span must not carry an end time");
    assert_eq!(started[0]["status"], "unset");

    run.end();
    mcpd.flush().await.expect("export should have succeeded");

    let all = server.all_spans();
    assert_eq!(all.len(), 2, "ending the span should send an update");
    assert!(!all[1]["end_time"].is_null(), "the update must carry an end time");
    assert_eq!(all[1]["status"], "ok");
    assert_eq!(all[0]["span_id"], all[1]["span_id"], "the update must replace the same span");
}

#[tokio::test]
async fn every_signal_kind_travels_in_the_same_batch() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("support agent");
    let trace_id = run.trace_id().to_owned();
    run.scope(async {
        mcpd.log(mcpd_sdk::LogLevel::Info, "answering");
        mcpd.counter("orders.looked_up", 1.0, None);
    })
    .await;
    mcpd.score(Target::trace(&trace_id), "helpfulness", 0.9, Scale::UNIT, Some("named the ETA"));
    mcpd.feedback(Target::trace(&trace_id), FeedbackKind::Thumbs, "up", None);
    run.end();
    mcpd.flush().await.expect("export should have succeeded");

    let body = server.requests()[0].json();
    assert_eq!(body["logs"].as_array().unwrap().len(), 1);
    assert_eq!(body["metrics"].as_array().unwrap().len(), 1);
    assert_eq!(body["scores"].as_array().unwrap().len(), 1);
    assert_eq!(body["feedback"].as_array().unwrap().len(), 1);
    assert!(body["ingest_key"].is_string(), "a batch must be idempotent");

    // A log emitted inside the run carries the run's ids, which is what makes
    // "show me the logs for this trace" one query.
    assert_eq!(body["logs"][0]["trace_id"], trace_id.as_str());
    assert_eq!(body["scores"][0]["min"], 0.0);
    assert_eq!(body["scores"][0]["max"], 1.0);
}

#[tokio::test]
async fn a_full_batch_exports_without_waiting_for_the_interval() {
    let server = FakeMcpd::start().await;
    // The flush interval is an hour: if anything arrives, the batch trigger did
    // it.
    let mcpd = Mcpd::with_config(Config {
        endpoint: server.endpoint(),
        batch_size: 4,
        flush_interval: Duration::from_secs(3600),
        ..Config::default()
    });

    for _ in 0..4 {
        mcpd.log(mcpd_sdk::LogLevel::Info, "tick");
    }

    assert!(
        server.wait_for(1, Duration::from_secs(5)).await,
        "a full batch should export on its own"
    );
}

#[tokio::test]
async fn a_body_over_a_kilobyte_is_gzipped() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("big prompt");
    run.set_input("x".repeat(4096));
    run.end();
    mcpd.flush().await.expect("export should have succeeded");

    let request = &server.requests()[0];
    assert_eq!(request.header("content-encoding"), Some("gzip"));
    assert!(request.body.len() < 4096, "a compressible body did not shrink");
    // The receiver can still read it, which is the only thing that matters.
    assert_eq!(request.spans().len(), 2);
}

#[tokio::test]
async fn a_small_body_is_sent_uncompressed() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());
    mcpd.log(mcpd_sdk::LogLevel::Info, "short");
    mcpd.flush().await.expect("export should have succeeded");
    assert_eq!(server.requests()[0].header("content-encoding"), None);
}

#[tokio::test]
async fn a_retryable_failure_is_retried_with_the_same_ingest_key() {
    let server = FakeMcpd::scripted(vec![
        Reply::Status(503, None),
        Reply::Status(429, Some(0)),
        Reply::Accepted,
    ])
    .await;
    let mcpd = client(server.endpoint());

    mcpd.log(mcpd_sdk::LogLevel::Info, "please land");
    mcpd.flush().await.expect("the third attempt should have succeeded");

    assert_eq!(server.count(), 3, "503 and 429 should both have been retried");
    let keys: Vec<String> = server
        .requests()
        .iter()
        .map(|r| r.json()["ingest_key"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(keys[0], keys[1], "a retry must reuse the key or it double-counts");
    assert_eq!(keys[1], keys[2]);
}

#[tokio::test]
async fn a_bad_request_is_never_retried() {
    // A 400 means the batch is malformed, and it will be just as malformed in
    // 250 milliseconds. Retrying it is how a client floods a server.
    let server = FakeMcpd::scripted(vec![Reply::Status(400, None)]).await;
    let mcpd = client(server.endpoint());

    mcpd.log(mcpd_sdk::LogLevel::Info, "malformed, apparently");
    let result = mcpd.flush().await;

    assert!(result.is_err(), "flush should report a rejected batch");
    assert_eq!(server.count(), 1, "a 4xx other than 429 must not be repeated");
}

#[tokio::test]
async fn a_server_that_keeps_failing_gives_up_after_three_attempts() {
    let server = FakeMcpd::scripted(vec![
        Reply::Status(502, None),
        Reply::Status(502, None),
        Reply::Status(502, None),
        Reply::Status(502, None),
    ])
    .await;
    let mcpd = client(server.endpoint());

    mcpd.log(mcpd_sdk::LogLevel::Info, "never lands");
    assert!(mcpd.flush().await.is_err());
    assert_eq!(server.count(), 3, "three attempts, not four and not forever");
}

#[tokio::test]
async fn a_dead_server_never_raises() {
    // Nothing is listening on this port. Every instrumentation call must still
    // return, and the process must stay standing.
    let mcpd = Mcpd::with_config(Config {
        endpoint: "http://127.0.0.1:9".to_owned(),
        flush_interval: Duration::from_millis(20),
        timeout: Duration::from_millis(50),
        ..Config::default()
    });

    let run = mcpd.trace("support agent");
    run.set_input("where is my order?");
    run.child_of("lookup_order", SpanKind::ToolCall).end();
    mcpd.log(mcpd_sdk::LogLevel::Error, "the tool failed");
    mcpd.counter("failures", 1.0, None);
    mcpd.score(Target::trace(run.trace_id()), "helpfulness", 0.1, Scale::UNIT, None);
    mcpd.feedback(Target::trace(run.trace_id()), FeedbackKind::Thumbs, "down", None);
    run.set_error("no answer");
    run.end();

    // flush reports the failure to the caller who asked; nothing above it did.
    assert!(mcpd.flush().await.is_err());
    assert!(mcpd.shutdown().await.is_err() || true, "shutdown must return, whatever happened");
}

#[tokio::test]
async fn shutdown_flushes_and_is_safe_to_call_twice() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    mcpd.trace("support agent").end();
    mcpd.shutdown().await.expect("shutdown should have exported");
    assert_eq!(server.count(), 1, "shutdown must flush what is queued");

    mcpd.shutdown().await.expect("a second shutdown is a no-op, not a failure");
    assert_eq!(server.count(), 1);

    // Signals recorded after shutdown are dropped rather than queued for ever.
    mcpd.log(mcpd_sdk::LogLevel::Info, "too late");
    assert_eq!(mcpd.queued_signals(), 0);
}

#[tokio::test]
async fn a_disabled_client_sends_nothing_and_still_works() {
    let server = FakeMcpd::start().await;
    let mcpd = Mcpd::with_config(Config {
        endpoint: server.endpoint(),
        enabled: false,
        ..Config::default()
    });

    let run = mcpd.trace("support agent");
    assert!(!run.is_recording());
    // The objects are real: ids are well formed and a traceparent still
    // propagates, so turning mcpd off in a test cannot change what the code
    // under test does.
    assert_eq!(run.trace_id().len(), 32);
    assert_eq!(run.span_id().len(), 16);
    assert!(mcpd_sdk::SpanContext::from_traceparent(&run.traceparent()).is_some());
    run.set_input("anything");
    run.set_tokens(1, 2);
    run.child("tool").end();
    run.end();
    mcpd.log(mcpd_sdk::LogLevel::Info, "nothing to see");
    mcpd.score(Target::trace(run.trace_id()), "s", 1.0, Scale::UNIT, None);

    mcpd.flush().await.expect("flushing a disabled client is a no-op");
    mcpd.shutdown().await.expect("shutting down a disabled client is a no-op");
    assert_eq!(server.count(), 0, "a disabled client must not talk to the network");
    assert_eq!(mcpd.dropped_signals(), 0, "nothing was queued, so nothing was dropped");
}

#[tokio::test]
async fn a_number_json_cannot_carry_costs_its_signal_and_never_the_batch() {
    // serde_json writes NaN as `null`, where the daemon needs a number, and the
    // daemon refuses the whole batch over one. Decoded here as the daemon
    // decodes it, so a `null` in a required number fails this test.
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("support agent");
    mcpd.gauge("ratio", f64::NAN, None);
    mcpd.counter("calls", 1.0, None);
    mcpd.score(Target::trace(run.trace_id()), "broken", f64::INFINITY, Scale::UNIT, None);
    mcpd.score(Target::trace(run.trace_id()), "judge", 0.5, Scale::UNIT, None);
    run.end();
    mcpd.flush().await.expect("the batch should have been accepted");

    let body = server.requests()[0].json();
    let batch: mcpd_sdk::model::IngestBatch =
        serde_json::from_value(body).expect("the daemon could not have decoded this batch");
    let metrics: Vec<_> = batch.metrics.unwrap_or_default().into_iter().map(|m| m.name).collect();
    assert_eq!(metrics, ["calls"]);
    let scores: Vec<_> =
        batch.scores.unwrap_or_default().into_iter().map(|s| s.scorer_id).collect();
    assert_eq!(scores, ["judge"]);
    assert!(!batch.spans.unwrap_or_default().is_empty());
}

#[tokio::test]
async fn the_queue_bound_holds_and_the_loss_is_counted() {
    // A dead server and a queue of four: the fifth signal must cost the first,
    // not the process.
    let mcpd = Mcpd::with_config(Config {
        endpoint: "http://127.0.0.1:9".to_owned(),
        max_queue: 4,
        batch_size: 4,
        flush_interval: Duration::from_secs(3600),
        ..Config::default()
    });

    for i in 0..1_000 {
        mcpd.log(mcpd_sdk::LogLevel::Info, format!("line {i}"));
    }
    assert!(mcpd.queued_signals() <= 4, "the queue grew past max_queue");
    assert!(mcpd.dropped_signals() >= 996, "dropped signals must be counted");
}

#[tokio::test]
async fn an_error_sets_the_status_and_carries_a_message() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("support agent");
    let tool = run.child_of("notify_customer", SpanKind::ToolCall);
    tool.set_error("sms gateway timed out after 5s");
    tool.end();
    run.end_with(SpanStatus::Error);
    mcpd.flush().await.expect("export should have succeeded");

    let spans = server.all_spans();
    let failed = spans
        .iter()
        .find(|s| s["name"] == "notify_customer" && !s["end_time"].is_null())
        .expect("the tool span should have been sent");
    assert_eq!(failed["status"], "error");
    assert_eq!(failed["error"]["message"], "sms gateway timed out after 5s");

    let root = spans
        .iter()
        .find(|s| s["name"] == "support agent" && !s["end_time"].is_null())
        .expect("the root span should have been sent");
    assert_eq!(root["status"], "error");
}

#[tokio::test]
async fn a_span_carries_the_clients_correlation_and_the_run_carries_its_tags() {
    let server = FakeMcpd::start().await;
    let mcpd = Mcpd::with_config(
        Config {
            endpoint: server.endpoint(),
            project: "shop".to_owned(),
            environment: Some("staging".to_owned()),
            flush_interval: Duration::from_secs(3600),
            ..Config::default()
        }
        .with_service("checkout", Some("1.4.0")),
    );

    let run = mcpd.trace("support agent");
    run.set_tags(["billing"]);
    run.set_session("session-1");
    let child = run.child("step");
    // Tags label a run, so they are kept on the root and ignored on a child.
    child.set_tags(["ignored"]);
    child.end();
    run.end();
    mcpd.flush().await.expect("export should have succeeded");

    let spans = server.all_spans();
    // The finished copy of the root, not the one sent when it started: fields
    // set during a run only appear on the update that ends it.
    let root = spans
        .iter()
        .find(|s| s["is_root"] == true && !s["end_time"].is_null())
        .expect("the finished root span should have been sent");
    assert_eq!(root["correlation"]["project"], "shop");
    assert_eq!(root["correlation"]["environment"], "staging");
    assert_eq!(root["correlation"]["service_name"], "checkout");
    assert_eq!(root["correlation"]["service_version"], "1.4.0");
    assert_eq!(root["correlation"]["session_id"], "session-1");
    assert_eq!(root["tags"][0], "billing");

    let step = spans
        .iter()
        .find(|s| s["name"] == "step" && !s["end_time"].is_null())
        .expect("the finished child span should have been sent");
    assert!(step["tags"].is_null(), "tags on a child should have been ignored");
}

#[tokio::test]
async fn timestamps_are_utc_and_the_server_stamps_arrival() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());
    mcpd.trace("support agent").end();
    mcpd.flush().await.expect("export should have succeeded");

    let span = &server.all_spans()[0];
    let start = span["start_time"].as_str().unwrap();
    assert!(start.ends_with('Z'), "{start} is not UTC");
    // The producer's clock does not decide retention, so `received_at` goes out
    // as the epoch and mcpd stamps the real value.
    assert_eq!(span["received_at"], "1970-01-01T00:00:00Z");
}
