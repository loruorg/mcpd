//! A durable workflow in Rust: the graph goes to the daemon as a definition,
//! the step bodies stay in this process for a `Worker` to serve.
//!
//! The regions are read into the documentation at build time.

use std::time::Duration;

use mcpd_sdk::{
    when, Mcpd, PublishEvent, RetryPolicy, SpanKind, StartRun, StepContext, StepFailure,
    StepOptions, Worker, Workflow,
};
use serde_json::{json, Value};

/// The failure a body raises when the relay is busy: retried, after two seconds.
fn busy(e: String) -> StepFailure {
    StepFailure::retryable(e).retry_after(Duration::from_secs(2))
}

async fn deliver(text: &str, idempotency_key: &str) -> Result<(), String> {
    if text.is_empty() || idempotency_key.is_empty() {
        return Err("nothing to send".to_owned());
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), mcpd_sdk::Error> {
    let mcpd = Mcpd::new();

    let backoff = RetryPolicy::attempts(3).exponential(Duration::from_millis(500));
    let draft_input = json!({ "kind": "${steps.classify.output.kind}" });

    // #region workflow
    let support = Workflow::new("support_ticket")
        .step(
            "classify",
            |ctx: StepContext| async move {
                let input: Value = ctx.input()?;
                let ticket = input["ticket"].as_str().unwrap_or("");
                let kind = if ticket.starts_with('Q') { "question" } else { "bug" };
                Ok::<_, StepFailure>(json!({ "kind": kind }))
            },
            StepOptions::new(),
        )
        .agent(
            "draft",
            |ctx: StepContext| async move {
                // Your own model call, recorded under this step in the run's trace.
                // Its usage is the step's usage: the daemon prices what the span spent.
                let span = ctx.span("model_call", SpanKind::ModelGeneration);
                span.set_model("claude-sonnet-4", "anthropic");
                span.set_tokens(812, 37);
                span.end();
                Ok::<_, StepFailure>("an answer")
            },
            StepOptions::new().input(draft_input),
        )
        .wait_for(
            "approval",
            "approved",
            "${input.ticket}",
            Duration::from_secs(7 * 24 * 3600),
            StepOptions::new().when(when("steps.classify.output.kind", "eq", "bug")),
        )
        .step(
            "send",
            |ctx: StepContext| async move {
                let input: Value = ctx.input()?;
                let text = input["text"].as_str().unwrap_or("");
                deliver(text, ctx.idempotency_key()).await.map_err(busy)?;
                Ok::<_, StepFailure>(json!({ "sent": true }))
            },
            StepOptions::new()
                .input(json!({ "text": "${steps.draft.output}" }))
                .at_most_once()
                .retry(backoff),
        )
        .output(json!({ "sent": "${steps.send.output.sent}" }));
    // #endregion workflow

    // #region worker
    // Serve step bodies for as long as this process lives: register, long-poll
    // for leases, heartbeat while a body runs, answer every 409 by its reason.
    let worker = Worker::builder(&mcpd).concurrency(8).register(support).build();
    let serving = {
        let worker = worker.clone();
        tokio::spawn(async move { worker.serve().await })
    };
    // #endregion worker

    // #region run
    // Start a run from anywhere that holds an execute key, answer the wait from
    // a webhook, then follow the journal to the end and read the run.
    let start = StartRun::new("support_ticket").input(json!({ "ticket": "Q-42" }));
    let run = mcpd.start_run(start).await?;

    // A repeat of this event id is a duplicate, not a second event.
    let payload = json!({ "by": "ops" });
    let approval = PublishEvent::new("approved", "Q-42").payload(payload).event_id("hook-1");
    mcpd.publish_event(approval).await?;

    mcpd.watch_run(&run.run_id, -1, |e| println!("{} {:?}", e.seq, e.kind)).await?;
    let detail = mcpd.get_run(&run.run_id).await?;
    println!("{:?} {:?}", detail.run.status, detail.run.output);
    // #endregion run

    worker.stop();
    let _ = serving.await;
    mcpd.shutdown().await
}
