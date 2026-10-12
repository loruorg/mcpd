//! The canonical run, served in Rust. Prints one JSON line at the end.

use std::time::Duration;

use mcpd_sdk::{
    Config, Mcpd, PublishEvent, RetryPolicy, RunStatus, SpanKind, StartRun, StepContext,
    StepFailure, StepOptions, Worker, Workflow,
};
use serde_json::{json, Value};

#[tokio::main]
async fn main() {
    let canonical: Value =
        serde_json::from_str(include_str!("../../../runs/canonical.json")).expect("canonical.json");
    let definition_id = canonical["definition_id"].as_str().unwrap().to_owned();
    let mcpd = Mcpd::with_config(Config::from_env());

    let wf = Workflow::new(definition_id.as_str())
        .step(
            "classify",
            |ctx: StepContext| async move {
                let input: Value = ctx.input()?;
                let kind = if input["ticket"].as_str().unwrap_or("").starts_with('Q') { "question" } else { "bug" };
                Ok::<_, StepFailure>(json!({ "kind": kind }))
            },
            StepOptions::new(),
        )
        .agent(
            "draft",
            |ctx: StepContext| async move {
                let input: Value = ctx.input()?;
                ctx.set_state(json!({ "drafted": true }));
                let span = ctx.span("model_call", SpanKind::ModelGeneration);
                span.set_model("claude-sonnet-4", "anthropic");
                span.set_tokens(120, 40);
                span.end();
                Ok::<_, StepFailure>(format!("an answer to a {}", input["kind"].as_str().unwrap_or("")))
            },
            StepOptions::new().input(json!({ "kind": "${steps.classify.output.kind}" })),
        )
        .wait_for("approval", "approved", "${input.ticket}", Duration::from_secs(60), StepOptions::new().accept_parked())
        .step(
            "send",
            |ctx: StepContext| async move {
                let input: Value = ctx.input()?;
                if ctx.attempt() == 0 {
                    return Err(StepFailure::retryable("relay busy").retry_after(Duration::from_millis(100)));
                }
                Ok(json!({ "sent": true, "by": input["by"] }))
            },
            StepOptions::new()
                .input(json!({ "text": "${steps.draft.output}", "by": "${steps.approval.output.by}" }))
                .at_most_once()
                .retry(RetryPolicy::attempts(2).fixed(Duration::from_millis(100))),
        )
        .output(json!({ "answer": "${steps.draft.output}", "sent": "${steps.send.output.sent}", "by": "${steps.send.output.by}" }));

    let worker = Worker::builder(&mcpd)
        .id("probe-rust")
        .wait(Duration::from_secs(1))
        .on_error(|message, error| panic!("{message}: {error}"))
        .register(wf)
        .build();
    worker.register_definitions().await.expect("register");

    let mut correlation = mcpd_sdk::model::Correlation {
        project: String::new(),
        environment: None,
        service_name: None,
        service_version: None,
        session_id: None,
        thread_id: None,
        user_id: None,
        resource_id: None,
        run_id: None,
        request_id: None,
        experiment_id: None,
        release: None,
    };
    correlation.session_id = canonical["correlation"]["session_id"].as_str().map(str::to_owned);
    correlation.user_id = canonical["correlation"]["user_id"].as_str().map(str::to_owned);
    let run = mcpd
        .start_run(StartRun::new(definition_id.as_str()).input(canonical["input"].clone()).correlation(correlation))
        .await
        .expect("start");

    let event = &canonical["event"];
    let publish = || {
        PublishEvent::new(event["name"].as_str().unwrap(), event["key"].as_str().unwrap())
            .payload(event["payload"].clone())
            .event_id(event["event_id"].as_str().unwrap())
    };
    let first = mcpd.publish_event(publish()).await.expect("publish");
    let second = mcpd.publish_event(publish()).await.expect("publish again");

    let mut detail = mcpd.get_run(&run.run_id).await.expect("get run");
    for _ in 0..60 {
        if detail.run.status != RunStatus::Running {
            break;
        }
        worker.claim(4, Some(&run.run_id)).await.expect("claim");
        worker.drain().await;
        detail = mcpd.get_run(&run.run_id).await.expect("get run");
        if detail.run.status == RunStatus::Running {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    mcpd.shutdown().await.expect("shutdown");
    println!(
        "{}",
        json!({
            "run_id": run.run_id,
            "trace_id": run.trace_id,
            "status": detail.run.status,
            "publish": [first.outcome, second.outcome],
        })
    );
}
