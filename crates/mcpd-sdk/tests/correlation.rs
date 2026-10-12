//! Correlation: what a run is tagged with, and what inherits it.
//!
//! The property under test is not "a span has a session id". It is that the
//! five run-scoped fields are set once, on the run, and every step and every
//! log line underneath it carries the same ones. A trace whose correlation
//! stops at the root cannot be filtered by session or erased by subject, which
//! is most of what correlation is for.

mod common;

use common::FakeMcpd;
use mcpd_sdk::{Config, LogLevel, Mcpd, Scale, SpanKind, Target};
use serde_json::Value;
use std::time::Duration;

fn client(endpoint: String) -> Mcpd {
    Mcpd::with_config(Config {
        endpoint,
        project: "corr".to_owned(),
        environment: Some("staging".to_owned()),
        service_name: Some("probe".to_owned()),
        service_version: Some("9.9.9".to_owned()),
        // Long enough that only the explicit flush sends anything, so a test
        // reads one batch rather than racing the worker.
        flush_interval: Duration::from_secs(3600),
        ..Config::default()
    })
}

/// The five fields a caller sets on a run, in the order the contract lists them.
const RUN_FIELDS: [&str; 5] = ["session_id", "thread_id", "user_id", "request_id", "release"];

fn correlation_of(signal: &Value) -> &Value {
    &signal["correlation"]
}

fn field(signal: &Value, name: &str) -> Option<String> {
    correlation_of(signal)[name].as_str().map(str::to_owned)
}

/// The LAST record sent for a span, by name.
///
/// B8 puts a span on the wire twice, once on start and once on end, so the
/// first copy of a root is the one written before the caller had a chance to
/// tag it. The daemon merges the two and keeps the later one; a test that reads
/// the first would be asserting about a state no reader ever sees.
fn span_named<'a>(spans: &'a [Value], name: &str) -> &'a Value {
    spans
        .iter()
        .rev()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("no span called {name} was sent"))
}

fn logs(server: &FakeMcpd) -> Vec<Value> {
    server
        .requests()
        .iter()
        .flat_map(|r| r.json()["logs"].as_array().cloned().unwrap_or_default())
        .collect()
}

fn metrics(server: &FakeMcpd) -> Vec<Value> {
    server
        .requests()
        .iter()
        .flat_map(|r| r.json()["metrics"].as_array().cloned().unwrap_or_default())
        .collect()
}

fn scores(server: &FakeMcpd) -> Vec<Value> {
    server
        .requests()
        .iter()
        .flat_map(|r| r.json()["scores"].as_array().cloned().unwrap_or_default())
        .collect()
}

fn feedback(server: &FakeMcpd) -> Vec<Value> {
    server
        .requests()
        .iter()
        .flat_map(|r| r.json()["feedback"].as_array().cloned().unwrap_or_default())
        .collect()
}

/// Tag a run with all five fields, using the setters a caller has.
fn tag(run: &mcpd_sdk::Span) {
    run.set_session("sess-1");
    run.set_thread("thread-7");
    run.set_user("user-41");
    run.set_request("req-9001");
    run.set_release("2026.08.1");
}

#[tokio::test]
async fn a_run_can_express_every_correlation_field() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("support_agent");
    tag(&run);
    run.end();
    mcpd.flush().await.expect("export should have succeeded");

    let spans = server.all_spans();
    let root = span_named(&spans, "support_agent");
    assert_eq!(field(root, "session_id").as_deref(), Some("sess-1"));
    assert_eq!(field(root, "thread_id").as_deref(), Some("thread-7"));
    assert_eq!(field(root, "user_id").as_deref(), Some("user-41"));
    assert_eq!(field(root, "request_id").as_deref(), Some("req-9001"));
    assert_eq!(field(root, "release").as_deref(), Some("2026.08.1"));
}

#[tokio::test]
async fn a_child_inherits_what_the_run_was_tagged_with() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("support_agent");
    // Tagged AFTER the root exists, which is the only order a caller can use:
    // the run has to be started before anything can be set on it.
    tag(&run);
    let step = run.child_of("lookup_order", SpanKind::ToolCall);
    step.end();
    run.end();
    mcpd.flush().await.expect("export should have succeeded");

    let spans = server.all_spans();
    let root = span_named(&spans, "support_agent");
    let child = span_named(&spans, "lookup_order");
    for name in RUN_FIELDS {
        assert_eq!(
            field(child, name),
            field(root, name),
            "the child did not inherit {name}; root {}, child {}",
            correlation_of(root),
            correlation_of(child),
        );
    }
}

#[tokio::test]
async fn a_grandchild_inherits_it_too() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("support_agent");
    tag(&run);
    let step = run.child_of("plan_answer", SpanKind::ModelGeneration);
    let inner = step.child("call_model");
    inner.end();
    step.end();
    run.end();
    mcpd.flush().await.expect("export should have succeeded");

    let spans = server.all_spans();
    let root = span_named(&spans, "support_agent");
    let inner = span_named(&spans, "call_model");
    for name in RUN_FIELDS {
        assert_eq!(field(inner, name), field(root, name), "the grandchild lost {name}");
    }
}

#[tokio::test]
async fn a_child_may_override_one_field_without_losing_the_rest() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("support_agent");
    tag(&run);
    let step = run.child_of("sub_agent", SpanKind::AgentRun);
    step.set_session("sess-2");
    step.end();
    run.end();
    mcpd.flush().await.expect("export should have succeeded");

    let spans = server.all_spans();
    let child = span_named(&spans, "sub_agent");
    assert_eq!(field(child, "session_id").as_deref(), Some("sess-2"), "the override was lost");
    assert_eq!(field(child, "user_id").as_deref(), Some("user-41"), "the override erased the rest");
    assert_eq!(field(child, "release").as_deref(), Some("2026.08.1"));
}

#[tokio::test]
async fn a_log_inside_a_run_carries_the_run_s_correlation() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("support_agent");
    tag(&run);
    run.sync_scope(|| {
        mcpd.log(LogLevel::Warn, "estimate_delivery failed");
    });
    run.end();
    mcpd.flush().await.expect("export should have succeeded");

    let lines = logs(&server);
    assert_eq!(lines.len(), 1, "expected one log line, got {}", lines.len());
    let line = &lines[0];
    for name in RUN_FIELDS {
        assert!(
            field(line, name).is_some(),
            "the log dropped {name}; correlation was {}",
            correlation_of(line),
        );
    }
    assert_eq!(field(line, "session_id").as_deref(), Some("sess-1"));
    assert_eq!(field(line, "user_id").as_deref(), Some("user-41"));
    // The deployment three still come from the client.
    assert_eq!(field(line, "environment").as_deref(), Some("staging"));
    assert_eq!(correlation_of(line)["project"], "corr");
}

#[tokio::test]
async fn a_metric_a_score_and_feedback_carry_it_too() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("support_agent");
    tag(&run);
    let trace_id = run.trace_id().to_owned();
    run.sync_scope(|| {
        mcpd.counter("run_steps", 3.0, None);
        mcpd.score(Target::trace(&trace_id), "relevance", 0.86, Scale::UNIT, None);
        mcpd.feedback(Target::trace(&trace_id), mcpd_sdk::FeedbackKind::Thumbs, "up", None);
    });
    run.end();
    mcpd.flush().await.expect("export should have succeeded");

    for (what, signals) in
        [("metric", metrics(&server)), ("score", scores(&server)), ("feedback", feedback(&server))]
    {
        assert_eq!(signals.len(), 1, "expected one {what}");
        assert_eq!(
            field(&signals[0], "session_id").as_deref(),
            Some("sess-1"),
            "the {what} dropped the run's session; correlation was {}",
            correlation_of(&signals[0]),
        );
    }
}

#[tokio::test]
async fn a_log_outside_any_run_still_carries_the_client_s_correlation() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    mcpd.log(LogLevel::Info, "started");
    mcpd.flush().await.expect("export should have succeeded");

    let lines = logs(&server);
    assert_eq!(lines.len(), 1);
    assert_eq!(correlation_of(&lines[0])["project"], "corr");
    assert_eq!(field(&lines[0], "service_name").as_deref(), Some("probe"));
    // Nothing to inherit, so the run-scoped fields are absent rather than stale.
    assert!(field(&lines[0], "session_id").is_none());
}

#[tokio::test]
async fn an_event_inherits_the_run_s_correlation() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("support_agent");
    tag(&run);
    run.event("first_token");
    run.end();
    mcpd.flush().await.expect("export should have succeeded");

    let spans = server.all_spans();
    let event = span_named(&spans, "first_token");
    assert_eq!(field(event, "session_id").as_deref(), Some("sess-1"));
    assert_eq!(field(event, "release").as_deref(), Some("2026.08.1"));
}
