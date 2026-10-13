//! Experiments (contract section 5.2): each read and write on its path with
//! its body, and the runner end to end. Reads go to a fake daemon; spans go to
//! a recording sink so they can be read back exactly as they would be sent.

mod common;

use common::{FakeMcpd, Reply};
use mcpd_sdk::model::{Correlation, Span as SpanRecord};
use mcpd_sdk::testing::TestClient;
use mcpd_sdk::{
    Config, Error, Experiment, ExperimentStatus, Mcpd, RunExperiment, ScorerConfig, SpanKind,
    SpanStatus,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn client(endpoint: String) -> Mcpd {
    Mcpd::with_config(Config { endpoint, project: "shop".to_owned(), ..Config::default() })
}

/// A test client whose reads go to `endpoint` and whose spans stay in memory.
fn recording(endpoint: String) -> TestClient {
    TestClient::with_config(
        Config { endpoint, project: "shop".to_owned(), ..Config::default() }.with_sample(0.0),
    )
}

fn stored_experiment(id: &str) -> String {
    json!({
        "experiment_id": id,
        "name": "answers-v2",
        "dataset_id": "questions",
        "status": "running",
        "item_count": 0,
        "metadata": { "promptVersion": 4 },
        "correlation": { "project": "shop" },
    })
    .to_string()
}

fn items(n: usize) -> String {
    let items: Vec<Value> = (0..n)
        .map(|i| {
            json!({
                "item_id": format!("case-{i}"),
                "dataset_id": "questions",
                "input": { "question": format!("q{i}") },
                "expected": { "answerText": format!("a{i}") },
            })
        })
        .collect();
    Value::Array(items).to_string()
}

fn results(id: &str) -> String {
    json!({
        "experiment_id": id,
        "items": [{
            "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
            "item_id": "case-0",
            "status": "ok",
            "scores": { "exact_match": 1.0 },
        }],
        "scorers": [{
            "scorer_id": "exact_match",
            "n": 3,
            "mean": 0.5,
            "min": 0.0,
            "max": 1.0,
            "direction": "higher_is_better",
        }],
        "runs": 3,
        "failed": 1,
    })
    .to_string()
}

const SCORED: &str = r#"{"traces_scored":3,"scores_written":3,"skipped":[],"scores":[]}"#;

/// A daemon that answers every call the runner makes.
async fn daemon(cases: usize) -> FakeMcpd {
    FakeMcpd::routed(
        Vec::new(),
        vec![
            ("/v1/api/experiments", vec![Reply::Body(200, stored_experiment("exp-1"))]),
            ("/items", vec![Reply::Body(200, items(cases))]),
            ("/score", vec![Reply::Body(200, SCORED.to_owned())]),
            ("/results", vec![Reply::Body(200, results("exp-1"))]),
        ],
    )
    .await
}

fn experiment_posts(server: &FakeMcpd) -> Vec<Value> {
    server
        .requests()
        .iter()
        .filter(|r| r.method == "POST" && r.path() == "/v1/api/experiments")
        .map(common::Captured::json)
        .collect()
}

#[tokio::test]
async fn each_read_and_write_goes_to_its_path_with_its_body() {
    let server = FakeMcpd::scripted(vec![
        Reply::Body(
            200,
            json!({
                "trace_id": "run/1",
                "steps": [],
                "tool_sequence": ["lookup"],
                "step_count": 1,
                "tool_count": 1,
                "error_count": 0,
                "goal_reached": true,
            })
            .to_string(),
        ),
        Reply::Body(200, items(2)),
        Reply::Body(200, stored_experiment("exp-1")),
        Reply::Body(200, SCORED.to_owned()),
        Reply::Body(200, results("exp 1")),
    ])
    .await;
    let mcpd = client(server.endpoint());

    let trajectory = mcpd.get_trajectory("run/1").await.unwrap();
    assert_eq!(trajectory.tool_sequence, ["lookup"]);

    let cases = mcpd.list_dataset_items("support qs", Some(10)).await.unwrap();
    assert_eq!(cases.len(), 2);
    assert_eq!(cases[1].expected, Some(json!({ "answerText": "a1" })));

    let draft = Experiment {
        experiment_id: String::new(),
        name: "answers-v2".to_owned(),
        dataset_id: "questions".to_owned(),
        status: ExperimentStatus::Running,
        started_at: None,
        finished_at: None,
        item_count: 0,
        summary: None,
        baseline_id: None,
        cost: None,
        error: None,
        metadata: Some(json!({ "promptVersion": 4 })),
        correlation: Correlation { project: String::new(), ..correlation() },
    };
    let stored = mcpd.create_experiment(&draft).await.unwrap();
    assert_eq!(stored.experiment_id, "exp-1");

    let config = ScorerConfig {
        json_schema: Some(json!({ "type": "object", "required": ["answerText"] })),
        ..ScorerConfig::default()
    };
    let scored =
        mcpd.score_experiment("exp 1", &["exact_match".to_owned()], Some(&config)).await.unwrap();
    assert_eq!(scored.traces_scored, 3);

    let read = mcpd.experiment_results("exp 1").await.unwrap();
    assert_eq!(read.items[0].scores, json!({ "exact_match": 1.0 }));

    let requests = server.requests();
    let route: Vec<(&str, &str, &str)> =
        requests.iter().map(|r| (r.method.as_str(), r.path(), r.query())).collect();
    assert_eq!(
        route,
        [
            ("GET", "/v1/api/traces/run%2F1/trajectory", "project=shop"),
            ("GET", "/v1/api/datasets/support%20qs/items", "limit=10"),
            ("POST", "/v1/api/experiments", ""),
            ("POST", "/v1/api/experiments/exp%201/score", "project=shop"),
            ("GET", "/v1/api/experiments/exp%201/results", "project=shop"),
        ]
    );
    let created = requests[2].json();
    assert_eq!(created["correlation"]["project"], "shop");
    assert_eq!(created["experiment_id"], "");
    assert_eq!(created["metadata"], json!({ "promptVersion": 4 }));
    let score = requests[3].json();
    assert_eq!(score["scorers"], json!(["exact_match"]));
    assert_eq!(
        score["config"]["json_schema"],
        json!({ "type": "object", "required": ["answerText"] })
    );
}

#[tokio::test]
async fn scoring_with_no_config_leaves_it_off_the_body() {
    let server = FakeMcpd::scripted(vec![Reply::Body(200, SCORED.to_owned())]).await;
    client(server.endpoint()).score_experiment("exp-1", &[], None).await.unwrap();
    assert_eq!(server.requests()[0].json(), json!({ "scorers": [] }));
}

fn correlation() -> Correlation {
    serde_json::from_value(json!({ "project": "shop" })).unwrap()
}

#[tokio::test]
async fn a_run_records_each_case_and_carries_on_past_a_failing_one() {
    let server = daemon(3).await;
    let test = recording(server.endpoint());
    let mcpd = test.client().clone();

    let calls = AtomicUsize::new(0);
    let target = |input: Value| {
        calls.fetch_add(1, Ordering::SeqCst);
        let mcpd = mcpd.clone();
        async move {
            let question = input["question"].as_str().unwrap_or_default().to_owned();
            let lookup = mcpd.span("lookup", SpanKind::ToolCall);
            lookup.end();
            if question == "q1" {
                return Err(format!("no answer for {question}"));
            }
            Ok(json!({ "answer": format!("a{}", &question[1..]) }))
        }
    };
    let options = RunExperiment::new()
        .name("answers-v2")
        .scorers(["exact_match"])
        .concurrency(1)
        .metadata(json!({ "promptVersion": 4 }));
    let results = mcpd.run_experiment("questions", target, &options).await.unwrap();
    assert_eq!(results.runs, 3);
    assert_eq!(calls.load(Ordering::SeqCst), 3, "the case after the failing one still ran");

    let roots: Vec<SpanRecord> = test.spans(Some("answers-v2"), Some(SpanKind::AgentRun)).await;
    assert_eq!(roots.len(), 3, "one root span per case, whatever the sample rate");
    for (i, root) in roots.iter().enumerate() {
        assert!(root.parent_span_id.is_none());
        assert_eq!(root.correlation.experiment_id.as_deref(), Some("exp-1"));
        let attributes = root.attributes.as_ref().expect("the case id is an attribute");
        assert_eq!(attributes["mcpd.dataset_item_id"], format!("case-{i}"));
        assert_eq!(root.input, Some(json!({ "question": format!("q{i}") })));
    }
    assert_eq!(roots[0].status, SpanStatus::Ok);
    assert_eq!(roots[0].output, Some(json!({ "answer": "a0" })));
    assert_eq!(roots[1].status, SpanStatus::Error);
    assert_eq!(roots[1].error.as_ref().map(|e| e.message.as_str()), Some("no answer for q1"));
    assert_eq!(roots[2].status, SpanStatus::Ok);
    assert_eq!(roots[2].output, Some(json!({ "answer": "a2" })));

    let lookups = test.spans(Some("lookup"), None).await;
    assert_eq!(lookups.len(), 3);
    for (lookup, root) in lookups.iter().zip(&roots) {
        assert_eq!(lookup.trace_id, root.trace_id);
        assert_eq!(lookup.parent_span_id.as_deref(), Some(root.span_id.as_str()));
    }

    let posts = experiment_posts(&server);
    assert_eq!(posts.len(), 2);
    assert_eq!(posts[0]["status"], "running");
    assert_eq!(posts[0]["experiment_id"], "");
    assert_eq!(posts[0]["name"], "answers-v2");
    assert_eq!(posts[0]["dataset_id"], "questions");
    assert_eq!(posts[0]["correlation"]["project"], "shop");
    assert_eq!(posts[0]["metadata"], json!({ "promptVersion": 4 }));
    assert!(posts[0]["started_at"].is_string());
    assert_eq!(posts[1]["experiment_id"], "exp-1");
    assert_eq!(posts[1]["status"], "succeeded");
    assert_eq!(posts[1]["item_count"], 3);
    assert_eq!(posts[1]["summary"], json!({ "exact_match": 0.5 }));
    assert!(posts[1]["finished_at"].is_string());

    let requests = server.requests();
    let items = requests.iter().find(|r| r.path().ends_with("/items")).unwrap();
    assert_eq!(items.path(), "/v1/api/datasets/questions/items");
    assert_eq!(items.query(), "limit=5000");
    let score = requests.iter().find(|r| r.path().ends_with("/score")).unwrap();
    assert_eq!(score.path(), "/v1/api/experiments/exp-1/score");
    assert_eq!(score.json(), json!({ "scorers": ["exact_match"] }));
}

#[tokio::test]
async fn a_target_that_panics_fails_its_own_case_only() {
    let server = daemon(2).await;
    let test = recording(server.endpoint());
    let target = |input: Value| async move {
        assert_ne!(input["question"], "q0", "the agent fell over");
        Ok::<_, String>("fine")
    };
    test.client().run_experiment("questions", target, &RunExperiment::new()).await.unwrap();

    let roots = test.spans(Some("answers-v2"), None).await;
    assert_eq!(roots.len(), 2);
    assert_eq!(roots[0].status, SpanStatus::Error);
    let message = roots[0].error.as_ref().unwrap().message.clone();
    assert!(message.starts_with("the target panicked"), "{message}");
    assert_eq!(roots[1].status, SpanStatus::Ok);
}

#[tokio::test]
async fn no_more_than_concurrency_targets_are_in_flight() {
    let server = daemon(12).await;
    let test = recording(server.endpoint());
    let in_flight = Arc::new(AtomicUsize::new(0));
    let most = Arc::new(AtomicUsize::new(0));
    let target = |_input: Value| {
        let (in_flight, most) = (Arc::clone(&in_flight), Arc::clone(&most));
        async move {
            let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(10)).await;
            in_flight.fetch_sub(1, Ordering::SeqCst);
            Ok::<_, String>(Value::Null)
        }
    };
    let options = RunExperiment::new().concurrency(3);
    test.client().run_experiment("questions", target, &options).await.unwrap();

    assert_eq!(most.load(Ordering::SeqCst), 3);
    assert_eq!(test.spans(Some("answers-v2"), None).await.len(), 12);
}

#[tokio::test]
async fn a_failed_read_marks_the_experiment_failed_and_raises() {
    let server = FakeMcpd::routed(
        Vec::new(),
        vec![
            ("/v1/api/experiments", vec![Reply::Body(200, stored_experiment("exp-1"))]),
            ("/items", vec![Reply::Status(403, None)]),
        ],
    )
    .await;
    let test = recording(server.endpoint());
    let calls = AtomicUsize::new(0);
    let target = |_input: Value| {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Ok::<_, String>(Value::Null) }
    };

    let error =
        test.client().run_experiment("questions", target, &RunExperiment::new()).await.unwrap_err();
    assert!(matches!(error, Error::Api { status: 403, .. }), "{error:?}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let posts = experiment_posts(&server);
    assert_eq!(posts.len(), 2);
    assert_eq!(posts[1]["experiment_id"], "exp-1");
    assert_eq!(posts[1]["status"], "failed");
    assert!(posts[1]["finished_at"].is_string());
    let recorded = posts[1]["error"].as_str().unwrap();
    assert!(recorded.contains("403"), "{recorded}");
}

#[tokio::test]
async fn a_bad_concurrency_or_a_disabled_client_is_refused_before_any_request() {
    let server = FakeMcpd::start().await;
    let target = |_input: Value| async { Ok::<_, String>(Value::Null) };

    let error = client(server.endpoint())
        .run_experiment("questions", target, &RunExperiment::new().concurrency(0))
        .await
        .unwrap_err();
    assert!(matches!(&error, Error::Config(m) if m.contains("concurrency")), "{error:?}");

    let disabled = Mcpd::with_config(
        Config { endpoint: server.endpoint(), ..Config::default() }.with_enabled(false),
    );
    let error =
        disabled.run_experiment("questions", target, &RunExperiment::new()).await.unwrap_err();
    assert!(matches!(&error, Error::Config(m) if m.contains("enabled")), "{error:?}");

    assert_eq!(server.count(), 0);
}
