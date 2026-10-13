//! An experiment in Rust: run every case of a dataset through your own code,
//! score each answer against its case, and read the results.
//!
//! The regions are read into the documentation at build time.

use mcpd_sdk::{Config, Mcpd, RunExperiment, SpanKind, SpanStatus};
use serde_json::{json, Value};

/// The code under test. Anything it records nests under the case's run.
async fn answer(mcpd: &Mcpd, input: Value) -> Result<String, String> {
    let question = input["question"].as_str().ok_or("the case has no question")?;
    let call = mcpd.span("model_call", SpanKind::ModelGeneration);
    call.set_model("claude-sonnet-4", "anthropic");
    let reply =
        if question.contains("refund") { "Refunds take five days." } else { "It ships tomorrow." };
    call.set_output(reply);
    call.end();
    Ok(reply.to_owned())
}

#[tokio::main]
async fn main() -> Result<(), mcpd_sdk::Error> {
    let mcpd = Mcpd::with_config(Config::from_env().with_project("support"));

    // #region experiment
    // Each case runs in its own root span, four at a time. A case that fails is
    // recorded as a failed run, and the others still run.
    let options = RunExperiment::new()
        .name("answers-v2")
        .scorers(["exact_match", "similarity"])
        .metadata(json!({ "prompt": "answer@4" }));
    let target = |input: Value| answer(&mcpd, input);
    let results = mcpd.run_experiment("support-questions", target, &options).await?;

    for scorer in &results.scorers {
        println!("{}: mean {:.2} over {}", scorer.scorer_id, scorer.mean, scorer.n);
    }
    for item in &results.items {
        if item.status == SpanStatus::Error {
            println!("case {:?} failed: {}", item.item_id, item.trace_id);
        }
    }

    // Read one run back as the steps its agent took.
    if let Some(first) = results.items.first() {
        let trajectory = mcpd.get_trajectory(&first.trace_id).await?;
        let tools = trajectory.tool_sequence.join(" > ");
        println!("{} steps, tools: {tools}", trajectory.step_count);
    }
    // #endregion experiment

    mcpd.shutdown().await
}
