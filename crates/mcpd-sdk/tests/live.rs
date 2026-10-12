//! End-to-end tests against a running mcpd.
//!
//! These are ignored by default because they need a server. Start one and point
//! them at it:
//!
//! ```text
//! mcpd demo --data /tmp/mcpd-live.db --listen 127.0.0.1:4317
//! MCPD_TEST_ENDPOINT=http://127.0.0.1:4317 \
//!   cargo test -p mcpd-sdk --test live -- --include-ignored
//! ```
//!
//! What they prove that the fake server cannot: that mcpd accepts the exact
//! shapes this SDK sends, and that what comes back out is what went in.
//!
//! The variable is `MCPD_TEST_ENDPOINT`, the name every other SDK's live suite
//! reads. It used to be a near-twin of that name which nothing ever set, so
//! `live()` returned `None`, every test returned on its first line, and the
//! suite reported three passes having contacted no server at all. A test that
//! cannot fail is worse than a missing one, because it is counted.
//!
//! `MCPD_REQUIRE_LIVE=1` is the guard against that happening again: with it set,
//! a missing endpoint is a panic rather than an early return. CI sets it in the
//! job that starts a daemon, so if the variable is ever renamed or dropped the
//! suite goes red instead of quietly passing.

use mcpd_sdk::{
    Config, FeedbackKind, Include, Mcpd, PublishEvent, Scale, SpanKind, SpanStatus, StartRun,
    StepContext, StepFailure, StepOptions, Target, TokenUsage, Worker, Workflow,
};
use serde_json::{json, Value};
use time::OffsetDateTime;

fn live() -> Option<Mcpd> {
    let endpoint = match std::env::var("MCPD_TEST_ENDPOINT") {
        Ok(e) => e,
        Err(_) => {
            assert!(
                std::env::var("MCPD_REQUIRE_LIVE").is_err(),
                "MCPD_REQUIRE_LIVE is set but MCPD_TEST_ENDPOINT is not, so this suite \
                 would have passed without reaching a server. That is the exact silence \
                 this variable exists to break."
            );
            return None;
        }
    };
    Some(Mcpd::with_config(
        Config::default()
            .with_endpoint(endpoint)
            .with_project("sdk-live")
            .with_environment("test")
            .with_service("mcpd-sdk-tests", Some(env!("CARGO_PKG_VERSION"))),
    ))
}

#[tokio::test]
#[ignore = "needs a running mcpd; set MCPD_TEST_ENDPOINT"]
async fn a_whole_run_survives_the_round_trip() {
    let Some(mcpd) = live() else { return };

    let health = mcpd.health().await.expect("the server should be healthy");
    assert_eq!(health.spec_version, mcpd_sdk::model::SPEC_VERSION);

    let run = mcpd.trace("support agent");
    let trace_id = run.trace_id().to_owned();
    run.set_input("where is my order?");
    run.set_tags(["live-test"]);
    run.set_session("session-live");

    let tool = run.child_of("lookup_order", SpanKind::ToolCall);
    tool.set_output(serde_json::json!({ "status": "shipped" }));
    tool.end();

    let call = run.child_of("answer", SpanKind::ModelGeneration);
    call.set_model("claude-sonnet-4", "anthropic");
    call.set_usage(TokenUsage {
        input_tokens: Some(1000),
        output_tokens: Some(500),
        ..TokenUsage::default()
    });
    call.end();

    let failed = run.child_of("notify_customer", SpanKind::ToolCall);
    failed.set_error("sms gateway timed out after 5s");
    failed.end();

    run.set_output("It shipped on Tuesday.");
    run.end();

    mcpd.score(
        Target::trace(&trace_id),
        "answer_relevance",
        0.9,
        Scale::UNIT,
        Some("named the ETA"),
    );
    mcpd.feedback(Target::trace(&trace_id), FeedbackKind::Thumbs, "up", None);
    mcpd.log(mcpd_sdk::LogLevel::Info, "run finished");
    mcpd.counter("orders.looked_up", 1.0, None);
    mcpd.flush().await.expect("the server should have accepted the batch");

    let detail =
        mcpd.get_trace(&trace_id, Include::all()).await.expect("the trace should be readable");
    assert_eq!(detail.trace.name, "support agent");
    assert_eq!(detail.spans.len(), 4, "four spans, each written once and updated once");
    assert_eq!(detail.trace.error_count, 1);
    assert_eq!(detail.trace.tool_count, 2);
    assert_eq!(detail.trace.model_count, 1);
    assert_eq!(detail.trace.correlation.session_id.as_deref(), Some("session-live"));

    // mcpd priced the model call from its own price book at ingest.
    let cost = detail.trace.cost.as_ref().and_then(|c| c.total_cost).unwrap_or_default();
    assert!(cost > 0.0, "a priced model call should have produced a cost");

    let failed = detail
        .spans
        .iter()
        .find(|s| s.name == "notify_customer")
        .expect("the failed tool span should be stored");
    assert_eq!(failed.status, SpanStatus::Error);
    assert_eq!(
        failed.error.as_ref().map(|e| e.message.as_str()),
        Some("sms gateway timed out after 5s")
    );

    assert_eq!(detail.scores.unwrap_or_default().len(), 1);
    assert_eq!(detail.feedback.unwrap_or_default().len(), 1);

    mcpd.shutdown().await.expect("shutdown should have flushed");
}

#[tokio::test]
#[ignore = "needs a running mcpd; set MCPD_TEST_ENDPOINT"]
async fn the_read_surface_answers() {
    let Some(mcpd) = live() else { return };

    let run = mcpd.trace("readable run");
    let trace_id = run.trace_id().to_owned();
    run.child_of("step", SpanKind::WorkflowStep).end();
    run.end();
    mcpd.flush().await.expect("the server should have accepted the batch");

    let traces = mcpd
        .list_traces(&mcpd_sdk::TraceFilter {
            name: Some("readable run".to_owned()),
            limit: Some(5),
            ..mcpd_sdk::TraceFilter::default()
        })
        .await
        .expect("list_traces should answer");
    assert!(traces.traces.iter().any(|t| t.trace_id == trace_id));

    let spans = mcpd
        .list_spans(&mcpd_sdk::SpanFilter {
            trace_id: Some(trace_id.clone()),
            ..mcpd_sdk::SpanFilter::default()
        })
        .await
        .expect("list_spans should answer");
    assert_eq!(spans.spans.len(), 2);

    let from = OffsetDateTime::now_utc() - time::Duration::hours(1);
    let to = OffsetDateTime::now_utc() + time::Duration::minutes(1);

    let summary = mcpd
        .analytics_summary(from, to, Some("model"))
        .await
        .expect("analytics_summary should answer");
    assert!(summary.trace_count > 0);

    let cost = mcpd
        .analytics_cost(from, to, Some("model"), Some("1h"))
        .await
        .expect("analytics_cost should answer");
    assert_eq!(cost.currency, "USD");

    mcpd.list_logs(&mcpd_sdk::LogFilter::default()).await.expect("list_logs should answer");
    mcpd.list_scores(&mcpd_sdk::ScoreFilter::default()).await.expect("list_scores should answer");
    mcpd.list_feedback(&trace_id).await.expect("list_feedback should answer");
    mcpd.query_metrics(&mcpd_sdk::MetricQuery::new("orders.looked_up", from, to))
        .await
        .expect("query_metrics should answer");

    mcpd.shutdown().await.expect("shutdown should have flushed");
}

#[tokio::test]
#[ignore = "needs a running mcpd; set MCPD_TEST_ENDPOINT"]
async fn a_trace_continues_across_a_service_boundary() {
    let Some(caller) = live() else { return };
    let Some(callee) = live() else { return };

    let outbound = caller.trace("service a");
    let header = outbound.traceparent();
    let inbound = callee.trace_from("service b", Some(&header), SpanKind::AgentRun);
    let trace_id = outbound.trace_id().to_owned();
    inbound.end();
    outbound.end();

    caller.flush().await.expect("the server should have accepted the batch");
    callee.flush().await.expect("the server should have accepted the batch");

    let detail = mcpd_sdk::Mcpd::with_config(caller.config().clone())
        .get_trace(&trace_id, Include::default())
        .await
        .expect("the trace should be readable");
    assert_eq!(detail.spans.len(), 2, "both services should be in one trace");
    let remote = detail.spans.iter().find(|s| s.name == "service b").unwrap();
    assert_eq!(remote.parent_span_id.as_deref(), Some(outbound.span_id()));

    caller.shutdown().await.expect("shutdown should have flushed");
    callee.shutdown().await.expect("shutdown should have flushed");
}

/// A client on a project of its own, so repeated runs never read each other's data.
fn live_runs() -> Option<Mcpd> {
    let endpoint = std::env::var("MCPD_TEST_ENDPOINT").ok()?;
    let project = format!("sdk-live-rust-runs-{}", &mcpd_sdk::model::ids::new_trace_id()[..8]);
    Some(Mcpd::with_config(
        Config::default()
            .with_endpoint(endpoint)
            .with_project(project)
            .with_environment("test")
            .with_service("mcpd-sdk-tests", Some(env!("CARGO_PKG_VERSION"))),
    ))
}

async fn drive(mcpd: &Mcpd, worker: &Worker, run_id: &str) -> mcpd_sdk::RunDetail {
    let mut detail = mcpd.get_run(run_id).await.expect("get run");
    for _ in 0..10 {
        if detail.run.status != mcpd_sdk::RunStatus::Running {
            break;
        }
        worker.claim(4, Some(run_id)).await.expect("claim");
        worker.drain().await;
        detail = mcpd.get_run(run_id).await.expect("get run");
    }
    detail
}

#[tokio::test]
#[ignore = "needs a running mcpd; set MCPD_TEST_ENDPOINT"]
async fn a_workflow_is_served_and_the_run_and_its_trace_read_back() {
    let Some(mcpd) = live_runs() else { return };
    let wf = Workflow::new("live_support")
        .step(
            "classify",
            |ctx: StepContext| async move {
                let input: Value = ctx.input()?;
                let ticket = input["ticket"].as_str().unwrap_or_default();
                let kind = if ticket.starts_with('Q') { "question" } else { "bug" };
                Ok::<_, StepFailure>(json!({"kind": kind}))
            },
            StepOptions::new(),
        )
        .agent(
            "draft",
            |ctx: StepContext| async move {
                let input: Value = ctx.input()?;
                ctx.set_tokens(120, 40);
                ctx.set_state(json!({"drafted": true}));
                let span = ctx.span("live_model", SpanKind::ModelGeneration);
                span.set_model("claude-sonnet-4", "anthropic");
                span.set_tokens(120, 40);
                span.end();
                Ok::<_, StepFailure>(format!(
                    "an answer to a {}",
                    input["kind"].as_str().unwrap_or_default()
                ))
            },
            StepOptions::new().input(json!({"kind": "${steps.classify.output.kind}"})),
        )
        .output(json!({"answer": "${steps.draft.output}"}));
    let worker = Worker::builder(&mcpd)
        .id("live-worker")
        .wait(std::time::Duration::from_secs(1))
        .on_error(|message, error| panic!("{message}: {error}"))
        .register(wf)
        .build();
    worker.register_definitions().await.expect("register");

    let run = mcpd
        .start_run(StartRun::new("live_support").input(json!({"ticket": "Q-42"})))
        .await
        .expect("start");
    assert_eq!(run.status, mcpd_sdk::RunStatus::Running);
    assert_eq!(run.definition_id, "live_support");

    let detail = drive(&mcpd, &worker, &run.run_id).await;
    assert_eq!(detail.run.status, mcpd_sdk::RunStatus::Succeeded, "{:?}", detail.run.error);
    assert_eq!(detail.run.output, Some(json!({"answer": "an answer to a question"})));
    assert_eq!(detail.run.state, Some(json!({"drafted": true})));
    let draft = detail.attempts.iter().find(|a| a.step_key == "draft").expect("draft attempt");
    assert_eq!(draft.status, mcpd_sdk::AttemptStatus::Succeeded);
    assert_eq!(draft.kind, mcpd_sdk::StepKind::Agent);
    assert_eq!(draft.input, Some(json!({"kind": "question"})));
    assert_eq!(draft.usage.as_ref().and_then(|u| u.input_tokens), Some(120));

    let mut kinds = Vec::new();
    mcpd.watch_run_every(&run.run_id, -1, std::time::Duration::from_millis(50), |e| {
        kinds.push(e.kind)
    })
    .await
    .expect("watch");
    assert_eq!(kinds.first(), Some(&mcpd_sdk::RunEventKind::RunStarted));
    assert_eq!(kinds.last(), Some(&mcpd_sdk::RunEventKind::RunEnded));

    mcpd.flush().await.expect("flush");
    // The daemon writes the run's own spans through an outbox drained on its
    // own clock, so give it a moment before reading the trace.
    let mut trace = None;
    for _ in 0..40 {
        if let Ok(t) = mcpd.get_trace(&run.trace_id, Include::default()).await {
            if t.spans.len() >= 4 && t.spans.iter().any(|s| s.name == "live_model") {
                trace = Some(t);
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    let trace = trace.expect("the run's trace with the body's span in it");
    let model = trace.spans.iter().find(|s| s.name == "live_model").expect("live_model span");
    assert_eq!(model.parent_span_id.as_deref(), Some(draft.span_id.as_str()));
    mcpd.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "needs a running mcpd; set MCPD_TEST_ENDPOINT"]
async fn a_wait_resumes_on_publish_and_another_run_ends_on_cancel() {
    let Some(mcpd) = live_runs() else { return };
    let wf = Workflow::new("live_approval")
        .wait_for(
            "approval",
            "approved",
            "${input.ticket}",
            std::time::Duration::from_secs(60),
            StepOptions::new(),
        )
        .step(
            "close",
            |ctx: StepContext| async move {
                let input: Value = ctx.input()?;
                Ok::<_, StepFailure>(json!({"closed_by": input["by"]}))
            },
            StepOptions::new().input(json!({"by": "${steps.approval.output.by}"})),
        )
        .output(json!("${steps.close.output}"));
    let worker = Worker::builder(&mcpd)
        .id("live-approver")
        .wait(std::time::Duration::from_millis(500))
        .on_error(|message, error| panic!("{message}: {error}"))
        .register(wf)
        .build();
    worker.register_definitions().await.expect("register");

    let waiting = mcpd
        .start_run(StartRun::new("live_approval").input(json!({"ticket": "T-7"})))
        .await
        .expect("start");
    let mut detail = mcpd.get_run(&waiting.run_id).await.expect("get");
    for _ in 0..10 {
        if !detail.waits.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        detail = mcpd.get_run(&waiting.run_id).await.expect("get");
    }
    assert_eq!(detail.waits.len(), 1);
    assert_eq!((detail.waits[0].event.as_str(), detail.waits[0].key.as_str()), ("approved", "T-7"));

    let receipt = mcpd
        .publish_event(
            PublishEvent::new("approved", "T-7")
                .payload(json!({"by": "ops"}))
                .event_id("live-approval-rust-1"),
        )
        .await
        .expect("publish");
    assert_eq!(receipt.outcome, mcpd_sdk::EventOutcome::Delivered);
    let again = mcpd
        .publish_event(PublishEvent::new("approved", "T-7").event_id("live-approval-rust-1"))
        .await
        .expect("publish again");
    assert_eq!(again.outcome, mcpd_sdk::EventOutcome::Duplicate);

    let detail = drive(&mcpd, &worker, &waiting.run_id).await;
    assert_eq!(detail.run.status, mcpd_sdk::RunStatus::Succeeded, "{:?}", detail.run.error);
    assert_eq!(detail.run.output, Some(json!({"closed_by": "ops"})));

    let doomed = mcpd
        .start_run(StartRun::new("live_approval").input(json!({"ticket": "T-8"})))
        .await
        .expect("start");
    let cancelled = mcpd.cancel_run(&doomed.run_id).await.expect("cancel");
    assert_eq!(cancelled.status, mcpd_sdk::RunStatus::Cancelled);
    let mut last = None;
    mcpd.watch_run_every(&doomed.run_id, -1, std::time::Duration::from_millis(50), |e| {
        last = Some(e.kind)
    })
    .await
    .expect("watch");
    assert_eq!(last, Some(mcpd_sdk::RunEventKind::RunEnded));

    let listed = mcpd
        .list_runs(&mcpd_sdk::RunFilter {
            definition_id: Some("live_approval".to_owned()),
            ..Default::default()
        })
        .await
        .expect("list");
    assert_eq!(listed.runs.len(), 2);
    mcpd.shutdown().await.expect("shutdown");
}
