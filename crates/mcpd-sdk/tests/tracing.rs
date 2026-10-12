//! Context: what a trace inherits, what it carries across a boundary, and what
//! happens when sampling says no.

mod common;

use common::FakeMcpd;
use mcpd_sdk::model::ModelParameters;
use mcpd_sdk::{Config, Mcpd, SpanContext, SpanKind};
use std::time::Duration;

fn client(endpoint: String) -> Mcpd {
    Mcpd::with_config(Config {
        endpoint,
        flush_interval: Duration::from_secs(3600),
        ..Config::default()
    })
}

#[tokio::test]
async fn a_traceparent_carries_a_trace_across_a_service_boundary() {
    let server = FakeMcpd::start().await;
    let caller = client(server.endpoint());
    let callee = client(server.endpoint());

    // Service A starts the run and puts its context on the outbound request.
    let outbound = caller.trace("checkout");
    let header = outbound.traceparent();
    assert!(header.starts_with("00-"), "{header} is not a W3C traceparent");

    // Service B reads the header and continues the same trace.
    let inbound = callee.trace_from("charge card", Some(&header), SpanKind::AgentRun);
    assert_eq!(inbound.trace_id(), outbound.trace_id(), "the trace did not survive the hop");
    assert_ne!(inbound.span_id(), outbound.span_id());
    inbound.end();
    outbound.end();

    caller.flush().await.expect("export should have succeeded");
    callee.flush().await.expect("export should have succeeded");

    let spans = server.all_spans();
    let remote = spans.iter().find(|s| s["name"] == "charge card").unwrap();
    assert_eq!(
        remote["parent_span_id"].as_str(),
        Some(outbound.span_id()),
        "the downstream span should hang off the caller's span"
    );
    assert_eq!(remote["is_root"], false);
}

#[tokio::test]
async fn an_unsampled_caller_stops_the_callee_recording() {
    let server = FakeMcpd::start().await;
    let callee = client(server.endpoint());

    // The upstream decision always wins: a trace that some services keep and
    // others drop is not a trace.
    let unsampled = SpanContext { sampled: false, ..SpanContext::root(false) };
    let span = callee.trace_from("charge card", Some(&unsampled.traceparent()), SpanKind::AgentRun);

    assert!(!span.is_recording());
    assert_eq!(span.trace_id(), unsampled.trace_id);
    span.end();
    callee.flush().await.expect("nothing to send is not a failure");
    assert_eq!(server.count(), 0);
}

#[tokio::test]
async fn a_malformed_traceparent_starts_a_new_trace_instead_of_failing() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let span = mcpd.trace_from("charge card", Some("not-a-header"), SpanKind::AgentRun);
    assert!(span.is_recording());
    assert_eq!(span.trace_id().len(), 32);
    span.end();

    mcpd.flush().await.expect("export should have succeeded");
    assert_eq!(server.all_spans()[0]["parent_span_id"], serde_json::Value::Null);
}

#[tokio::test]
async fn the_sampling_decision_is_made_once_and_inherited() {
    let server = FakeMcpd::start().await;
    let mcpd = Mcpd::with_config(Config {
        endpoint: server.endpoint(),
        sample: 0.0,
        flush_interval: Duration::from_secs(3600),
        ..Config::default()
    });

    let run = mcpd.trace("support agent");
    assert!(!run.is_recording(), "sample=0 should record nothing");
    let child = run.child_of("lookup_order", SpanKind::ToolCall);
    assert!(!child.is_recording(), "a child of an unrecorded span is not recorded");
    assert_eq!(child.trace_id(), run.trace_id(), "the ids are still real");
    // The context still propagates, so a downstream service makes the same
    // decision rather than starting a half trace of its own.
    assert!(child.traceparent().ends_with("-00"));
    child.end();
    run.end();

    mcpd.flush().await.expect("nothing to send is not a failure");
    assert_eq!(server.count(), 0);
}

#[tokio::test]
async fn everything_is_recorded_at_full_sampling() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());
    let run = mcpd.trace("support agent");
    assert!(run.is_recording());
    assert!(run.traceparent().ends_with("-01"));
    run.end();
    mcpd.flush().await.expect("export should have succeeded");
    assert_eq!(server.all_spans().len(), 2);
}

#[tokio::test]
async fn dropping_a_span_ends_it() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    {
        let _guard = mcpd.trace("support agent");
        // No end() call. The guard goes out of scope here.
    }
    mcpd.flush().await.expect("export should have succeeded");

    let spans = server.all_spans();
    assert_eq!(spans.len(), 2, "a start and an end");
    assert!(!spans[1]["end_time"].is_null(), "drop should have ended the span");
    assert_eq!(spans[1]["status"], "ok");
}

#[tokio::test]
async fn ending_a_span_twice_sends_one_update() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("support agent");
    run.end();
    run.end();
    drop(run);
    mcpd.flush().await.expect("export should have succeeded");

    let ended: Vec<_> =
        server.all_spans().into_iter().filter(|s| !s["end_time"].is_null()).collect();
    assert_eq!(ended.len(), 1, "ending twice should not send two updates");
}

#[tokio::test]
async fn a_change_after_the_end_is_ignored() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("support agent");
    run.set_output("answered");
    run.end();
    run.set_output("changed my mind");
    mcpd.flush().await.expect("export should have succeeded");

    let ended = server
        .all_spans()
        .into_iter()
        .find(|s| !s["end_time"].is_null())
        .expect("the finished span should have been sent");
    assert_eq!(ended["output"], "answered");
}

#[tokio::test]
async fn a_helper_finds_the_current_run_through_the_task_local() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("support agent");
    run.scope(async {
        // Nothing here was handed the run, and it still nests under it.
        assert_eq!(mcpd_sdk::current().unwrap().span_id, run.span_id());
        let step = mcpd.span("lookup_order", SpanKind::ToolCall);
        assert!(step.is_recording());
        step.end();
    })
    .await;
    run.end();
    assert!(mcpd_sdk::current().is_none(), "the scope should not leak");

    mcpd.flush().await.expect("export should have succeeded");
    let step = server.all_spans().into_iter().find(|s| s["name"] == "lookup_order").unwrap();
    assert_eq!(step["parent_span_id"].as_str(), Some(run.span_id()));
    assert_eq!(step["trace_id"].as_str(), Some(run.trace_id()));
}

#[tokio::test]
async fn a_span_with_nothing_current_is_a_root() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let orphan = mcpd.span("lookup_order", SpanKind::ToolCall);
    orphan.end();
    mcpd.flush().await.expect("export should have succeeded");

    assert_eq!(server.all_spans()[0]["is_root"], true);
}

#[tokio::test]
async fn an_event_is_a_point_in_time_under_its_span() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("support agent");
    run.event_with("fell back to email", serde_json::json!({ "after": "sms timeout" }));
    run.end();
    mcpd.flush().await.expect("export should have succeeded");

    let event = server
        .all_spans()
        .into_iter()
        .find(|s| s["name"] == "fell back to email")
        .expect("the event should have been sent");
    assert_eq!(event["is_event"], true);
    assert!(event["end_time"].is_null(), "an event has no duration");
    assert_eq!(event["parent_span_id"].as_str(), Some(run.span_id()));
    assert_eq!(event["attributes"]["after"], "sms timeout");
}

#[tokio::test]
async fn a_model_span_carries_the_fields_cost_is_computed_from() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let run = mcpd.trace("support agent");
    let call = run.child_of("answer", SpanKind::ModelGeneration);
    call.set_model("claude-sonnet-4", "anthropic");
    call.set_tokens(412, 96);
    call.end();
    run.end();
    mcpd.flush().await.expect("export should have succeeded");

    let generation = server
        .all_spans()
        .into_iter()
        .find(|s| s["name"] == "answer" && !s["end_time"].is_null())
        .unwrap();
    assert_eq!(generation["kind"], "model_generation");
    assert_eq!(generation["model"], "claude-sonnet-4");
    assert_eq!(generation["provider"], "anthropic");
    assert_eq!(generation["usage"]["input_tokens"], 412);
    assert_eq!(generation["usage"]["output_tokens"], 96);
}

#[tokio::test]
async fn a_model_span_carries_the_settings_the_call_was_made_with() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let call = mcpd.trace("answer");
    call.set_model_parameters(ModelParameters {
        temperature: Some(0.2),
        top_p: Some(0.95),
        top_k: Some(40),
        max_tokens: Some(1024),
        seed: Some(7),
        stop_sequences: Some(vec!["\n\nHuman:".to_owned()]),
        has_response_format: true,
    });
    call.end();
    mcpd.flush().await.expect("export should have succeeded");

    let sent = server
        .all_spans()
        .into_iter()
        .find(|s| s["name"] == "answer" && !s["end_time"].is_null())
        .unwrap();
    let p = &sent["model_parameters"];
    assert_eq!(p["temperature"], 0.2);
    assert_eq!(p["top_p"], 0.95);
    assert_eq!(p["top_k"], 40);
    assert_eq!(p["max_tokens"], 1024);
    assert_eq!(p["seed"], 7);
    assert_eq!(p["stop_sequences"][0], "\n\nHuman:");
    assert_eq!(p["has_response_format"], true);
}

/// The classic bug this guards: reading zero as "not given". A zero seed is
/// what a reproducible run is pinned to and a zero temperature is the setting
/// most worth recording, so neither may be dropped on the way out. This reads
/// the JSON the fake daemon actually received, so a field lost to
/// `skip_serializing_if` is missing here.
#[tokio::test]
async fn a_zero_seed_is_a_seed() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    let call = mcpd.trace("greedy");
    call.set_model_parameters(ModelParameters {
        temperature: Some(0.0),
        top_p: Some(0.0),
        seed: Some(0),
        ..ModelParameters::default()
    });
    call.end();
    mcpd.flush().await.expect("export should have succeeded");

    let sent = server
        .all_spans()
        .into_iter()
        .find(|s| s["name"] == "greedy" && !s["end_time"].is_null())
        .unwrap();
    let p = &sent["model_parameters"];
    assert_eq!(p["seed"], 0, "a zero seed was dropped as if it were absent");
    assert_eq!(p["temperature"], 0.0, "a zero temperature was dropped");
    assert_eq!(p["top_p"], 0.0, "a zero top_p was dropped");
    assert_eq!(p["has_response_format"], false);
    assert!(p["max_tokens"].is_null(), "a field nobody set must stay absent");
}

/// B10: a recorded error must always carry a message. An empty one reaches the
/// trace list as a failure nobody can act on.
#[tokio::test]
async fn an_error_recorded_with_a_blank_message_still_says_something() {
    let server = FakeMcpd::start().await;
    let mcpd = client(server.endpoint());

    for blank in ["", "   ", "\n\t"] {
        let span = mcpd.trace("run");
        span.set_error(blank);
        span.end();
    }
    let _ = mcpd.flush().await;

    let spans = server.all_spans();
    let errored: Vec<_> = spans.iter().filter(|s| s.get("error").is_some()).collect();
    assert!(!errored.is_empty(), "the errored spans should have been sent");
    for s in errored {
        let msg = s["error"]["message"].as_str().unwrap_or("");
        assert!(!msg.trim().is_empty(), "a blank message was stored verbatim");
        assert_eq!(msg, "no message given");
    }
}
