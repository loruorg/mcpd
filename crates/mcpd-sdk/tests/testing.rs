//! The test client (contract section 2.5).

use mcpd_sdk::testing::TestClient;
use mcpd_sdk::{Config, FeedbackKind, LogLevel, Mcpd, Scale, SpanKind, Target};
use serde_json::{json, Value};

/// The code under test: a run with a tool call, a generation, a log line, a
/// score and a piece of feedback.
fn answer(mcpd: &Mcpd) {
    let run = mcpd.trace("answer");
    run.child_of("lookup", SpanKind::ToolCall).end();
    let generation = run.child_of("generate", SpanKind::ModelGeneration);
    generation.set_output("it ships tomorrow");
    generation.end();
    mcpd.log_with(LogLevel::Info, "answered", Some(json!({ "order": "A-1" })));
    mcpd.score(Target::trace(run.trace_id()), "relevance", 0.9, Scale::UNIT, None);
    mcpd.feedback(Target::trace(run.trace_id()), FeedbackKind::Thumbs, 1, None);
    mcpd.gauge("queue_depth", 3.0, None);
    run.set_output("done");
    run.end();
}

fn names(spans: &[mcpd_sdk::model::Span]) -> Vec<&str> {
    spans.iter().map(|s| s.name.as_str()).collect()
}

#[tokio::test]
async fn every_signal_reads_back_with_no_flush() {
    let test = TestClient::new();
    answer(test.client());
    assert_eq!(names(&test.spans(None, None).await), ["answer", "lookup", "generate"]);
    let logs = test.logs().await;
    assert_eq!(logs.iter().map(|l| l.message.as_str()).collect::<Vec<_>>(), ["answered"]);
    assert_eq!(test.scores().await[0].scorer_id, "relevance");
    assert_eq!(test.recorded_feedback().await[0].kind, FeedbackKind::Thumbs);
    assert_eq!(test.metrics().await[0].name, "queue_depth");
}

#[tokio::test]
async fn one_record_per_span_id_and_finished_spans_only() {
    let test = TestClient::new();
    let open = test.client().trace("still_running");
    answer(test.client());
    let spans = test.spans(None, None).await;
    assert!(test.recorder().spans().len() > spans.len(), "start and end were both recorded");
    let ids: std::collections::HashSet<&str> = spans.iter().map(|s| s.span_id.as_str()).collect();
    assert_eq!(ids.len(), spans.len());
    assert!(spans.iter().all(|s| s.end_time.is_some()));
    assert!(!names(&spans).contains(&"still_running"));
    open.end();
    let spans = test.spans(None, None).await;
    // The record kept is the last one seen, in the place the id first arrived.
    assert_eq!(names(&spans), ["still_running", "answer", "lookup", "generate"]);
    assert_eq!(spans[1].output, Some(json!("done")));
}

#[tokio::test]
async fn spans_filter_by_name_and_kind() {
    let test = TestClient::new();
    answer(test.client());
    assert_eq!(names(&test.spans(None, Some(SpanKind::ToolCall)).await), ["lookup"]);
    let generate = test.spans(Some("generate"), None).await;
    assert_eq!(generate[0].output, Some(json!("it ships tomorrow")));
    assert!(test.spans(Some("generate"), Some(SpanKind::ToolCall)).await.is_empty());
}

#[tokio::test]
async fn span_by_name_fails_on_none_and_on_two() {
    let test = TestClient::new();
    answer(test.client());
    let none = test.span("missing").await.unwrap_err();
    assert_eq!(none.found, 0);
    assert!(none.to_string().contains("found 0"), "{none}");
    assert!(none.to_string().contains("answer, lookup, generate"), "{none}");
    answer(test.client());
    let two = test.span("lookup").await.unwrap_err();
    assert_eq!(two.found, 2);
    assert!(two.to_string().contains("found 2"), "{two}");
    assert_eq!(test.span("missing").await.unwrap_err().seen.len(), 3, "each name once");
}

#[tokio::test]
async fn trees_nest_children_under_their_parent_by_start_time() {
    let test = TestClient::new();
    answer(test.client());
    let trees = test.trees().await;
    assert_eq!(trees.len(), 1);
    let root = &trees[0];
    assert_eq!(root.span.name, "answer");
    let children: Vec<&str> = root.children.iter().map(|c| c.span.name.as_str()).collect();
    assert_eq!(children, ["lookup", "generate"]);
    assert!(root.children.iter().all(|c| c.children.is_empty()));
}

#[tokio::test]
async fn a_span_whose_parent_was_not_recorded_is_a_root() {
    let test = TestClient::new();
    let run = test.client().trace("answer");
    let child = run.child("lookup");
    child.end();
    let trees = test.trees().await;
    assert_eq!(trees.len(), 1, "the open parent is left out");
    assert_eq!(trees[0].span.name, "lookup");
    run.end();
    assert_eq!(test.trees().await.len(), 1);
}

#[tokio::test]
async fn reset_forgets_everything() {
    let test = TestClient::new();
    answer(test.client());
    test.reset().await;
    assert!(test.spans(None, None).await.is_empty());
    assert!(test.logs().await.is_empty());
    assert!(test.scores().await.is_empty());
}

#[tokio::test]
async fn redaction_runs_as_it_would_on_a_real_client() {
    let test = TestClient::with_config(Config::default().with_redact(|v| match v {
        Value::String(s) => Value::String(s.replace("tomorrow", "[date]")),
        other => other,
    }));
    answer(test.client());
    let generate = test.span("generate").await.unwrap();
    assert_eq!(generate.output, Some(json!("it ships [date]")));
}

#[tokio::test]
async fn it_stays_on_when_the_environment_turns_mcpd_off() {
    // The test client overrides both variables, so the other tests in this
    // binary, which read the environment too, are not affected by them.
    std::env::set_var("MCPD_ENABLED", "false");
    std::env::set_var("MCPD_SAMPLE", "0");
    let test = TestClient::new();
    let plain = Config::from_env();
    std::env::remove_var("MCPD_ENABLED");
    std::env::remove_var("MCPD_SAMPLE");
    assert!(!plain.enabled, "the environment did turn a plain client off");
    answer(test.client());
    assert_eq!(test.spans(None, None).await.len(), 3);
}
