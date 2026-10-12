//! The canonical run, in Rust. Prints its trace id on the last line.

use mcpd_core::{FeedbackKind, LogLevel, MetricKind, SpanKind};
use mcpd_sdk::{Config, Mcpd, Scale, Target};

#[tokio::main]
async fn main() {
    let mcpd = Mcpd::with_config(Config::from_env());

    let run = mcpd.trace_of("support_agent", SpanKind::AgentRun);
    run.set_input("where is my order 41?");
    // Only two of the five correlation fields have a setter here. The harness
    // knows, and reports the other three as a gap rather than a difference.
    run.set_session("sess-conformance");
    run.set_thread("thread-7");
    run.set_user("user-41");
    run.set_request("req-9001");
    run.set_release("2026.08.1");

    let gen = run.child_of("plan_answer", SpanKind::ModelGeneration);
    gen.set_model("gpt-4o", "openai");
    gen.set_tokens(1180, 96);
    gen.end();

    run.child_of("lookup_order", SpanKind::ToolCall).end();

    let failing = run.child_of("estimate_delivery", SpanKind::ToolCall);
    failing.set_error("shipping service returned 503");
    failing.end();

    // The log and the metric read the current span from a task-local, and only
    // `scope` writes it. Outside one they would carry no trace at all.
    run.sync_scope(|| {
        mcpd.log_with(
            LogLevel::Warn,
            "estimate_delivery failed, answering from the cached estimate",
            Some(serde_json::json!({ "step": "estimate_delivery" })),
        );
        mcpd.metric(
            "conformance.run_steps",
            3.0,
            MetricKind::Counter,
            Some(serde_json::json!({ "outcome": "degraded", "lang": "rust" })),
            Some("{step}"),
        );
    });

    let trace_id = run.trace_id().to_owned();
    run.end();

    mcpd.score(Target::trace(&trace_id), "answer_relevance", 0.86, Scale::UNIT, None);
    mcpd.score(Target::trace(&trace_id), "scale_check", 3.0, Scale::new(1.0, 5.0), None);
    mcpd.feedback(Target::trace(&trace_id), FeedbackKind::Thumbs, "up", None);
    let _ = mcpd.flush().await;

    println!("{trace_id}");
}
