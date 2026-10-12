//! Instrumenting a support agent with mcpd.
//!
//! The agent is fake and runs in a few milliseconds, but everything around it
//! is real: a run, a model generation with token usage, a tool call, a tool
//! that fails, a score, a piece of human feedback, and a read of the finished
//! trace back out of mcpd.
//!
//! Start a server, then run this:
//!
//! ```text
//! mcpd demo --listen 127.0.0.1:4317
//! cargo run -p mcpd-sdk --example quickstart
//! ```

use mcpd_sdk::{Config, FeedbackKind, Include, Mcpd, Scale, SpanKind, Target, TokenUsage};
use serde_json::json;

#[tokio::main]
async fn main() {
    // #region init
    // Nothing here is required. `Mcpd::new()` reads MCPD_ENDPOINT, MCPD_API_KEY
    // and the rest from the environment and talks to a local mcpd by default.
    // Anything set in code wins over the environment.
    let mcpd = Mcpd::with_config(
        Config::from_env()
            .with_project("support")
            .with_environment("local")
            .with_service("support-agent", Some("1.0.0")),
    );
    // #endregion init

    let question = "Where is order 5512?";

    // #region trace
    // The root span is the run. The sampling decision is made here, once, and
    // every child inherits it.
    let run = mcpd.trace("support agent");
    run.set_input(question);
    run.set_session("session-4f21");
    run.set_user("customer-91");
    run.set_tags(["support", "orders"]);

    // A tool call. `run.child_of` nests it under the run.
    let lookup = run.child_of("lookup_order", SpanKind::ToolCall);
    lookup.set_input(json!({ "order_id": "5512" }));
    let order = json!({ "status": "shipped", "carrier": "DHL", "eta": "Tuesday" });
    lookup.set_output(&order);
    lookup.end();

    // A tool that fails. Recording the error sets the span's status to `error`,
    // and a message is required: "something failed" is not actionable.
    let notify = run.child_of("notify_customer", SpanKind::ToolCall);
    notify.set_input(json!({ "channel": "sms" }));
    notify.set_error("sms gateway timed out after 5s");
    notify.end();
    // The failure did not stop the run: the agent falls back to email, and the
    // trace shows both the attempt and the recovery.
    run.event_with("fell back to email", json!({ "after": "sms gateway timeout" }));

    // #endregion trace
    // The model call. Token usage is what mcpd prices at ingest, so this is the
    // one field worth never leaving out.
    let answer = {
        let generation = run.child_of("answer the customer", SpanKind::ModelGeneration);
        generation.set_model("claude-sonnet-4", "anthropic");
        generation.set_input(json!([
            { "role": "system", "content": "You are a support agent." },
            { "role": "user", "content": question },
        ]));
        let answer = format!(
            "Your order shipped with {} and arrives {}.",
            order["carrier"].as_str().unwrap_or("the carrier"),
            order["eta"].as_str().unwrap_or("soon"),
        );
        generation.set_output(&answer);
        generation.set_usage(TokenUsage {
            input_tokens: Some(412),
            output_tokens: Some(96),
            ..TokenUsage::default()
        });
        answer
        // `generation` is dropped here, which ends the span. Calling `end()`
        // yourself does the same thing; the guard is there so an early return
        // or a panic cannot leave a span open.
    };

    run.set_output(&answer);
    let trace_id = run.trace_id().to_owned();
    run.end();

    // #region score
    // A score is a machine judgement. The scale is not optional: a bare 0.9 with
    // no declared range cannot be compared to anything.
    mcpd.score(
        Target::trace(&trace_id),
        "answer_relevance",
        0.9,
        Scale::UNIT,
        Some("Names the carrier and the delivery day the customer asked about."),
    );

    // Feedback is a person's judgement, kept separate from a score because the
    // two answer different questions and must never be averaged together.
    mcpd.feedback(Target::trace(&trace_id), FeedbackKind::Thumbs, "up", Some("fast and correct"));
    // #endregion score

    // Short-lived processes must flush. Nothing is sent from memory by a program
    // that exits without this.
    if let Err(e) = mcpd.flush().await {
        eprintln!("mcpd could not export: {e}");
    }

    // #region query
    // The same client reads. This is the trace you just wrote.
    match mcpd.get_trace(&trace_id, Include::all()).await {
        Ok(detail) => {
            println!("{}: {} spans", detail.trace.name, detail.spans.len());
            for span in &detail.spans {
                println!(
                    "  {:<22} {:>8} {}",
                    span.name,
                    span.duration_ms().map_or("open".to_owned(), |ms| format!("{ms:.1}ms")),
                    span.status,
                );
            }
            if let Some(cost) = detail.trace.cost.as_ref().and_then(|c| c.total_cost) {
                println!("  cost: {cost:.6} USD");
            }
            for score in detail.scores.iter().flatten() {
                println!("  score {}: {}", score.scorer_id, score.value);
            }
        }
        Err(e) => eprintln!("could not read the trace back: {e}"),
    }
    // #endregion query

    if let Err(e) = mcpd.shutdown().await {
        eprintln!("mcpd could not shut down cleanly: {e}");
    }
}
