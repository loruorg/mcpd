//! The experiment runner against a running mcpd, with every empty shape it
//! sends. Ignored by default, like `tests/live.rs`:
//!
//! ```text
//! MCPD_TEST_ENDPOINT=http://127.0.0.1:4317 \
//!   cargo test -p mcpd-sdk --test live_experiment -- --include-ignored
//! ```

use std::collections::HashMap;
use std::sync::Mutex;

use mcpd_sdk::{Config, Experiment, ExperimentStatus, Include, Mcpd, RunExperiment, ScorerConfig};
use serde_json::{json, Value};

const PROJECT: &str = "sdk-live-rust-experiment";

fn endpoint() -> Option<String> {
    let endpoint = std::env::var("MCPD_TEST_ENDPOINT").ok();
    assert!(
        endpoint.is_some() || std::env::var("MCPD_REQUIRE_LIVE").is_err(),
        "MCPD_REQUIRE_LIVE is set but MCPD_TEST_ENDPOINT is not, so this suite would have \
         passed without reaching a server."
    );
    endpoint
}

/// A dataset holding `cases`, written with plain HTTP so only the runner is
/// under test.
async fn seed_dataset(endpoint: &str, cases: &[Value]) -> String {
    let id = format!("rs-live-{}", time::OffsetDateTime::now_utc().unix_timestamp_nanos());
    let http = reqwest::Client::new();
    let dataset = json!({
        "dataset_id": id, "name": id, "item_count": 0,
        "created_at": "2026-01-01T00:00:00Z", "correlation": { "project": PROJECT },
    });
    let items: Vec<Value> = cases
        .iter()
        .enumerate()
        .map(|(i, case)| {
            let mut item = case.clone();
            item["item_id"] = json!(format!("{id}-{i}"));
            item["dataset_id"] = json!(id);
            item
        })
        .collect();
    for (path, body) in [
        ("/v1/api/datasets".to_owned(), dataset),
        (format!("/v1/api/datasets/{id}/items"), Value::Array(items)),
    ] {
        let answer = http
            .post(format!("{endpoint}{path}"))
            .json(&body)
            .send()
            .await
            .expect("the daemon should answer");
        assert!(answer.status().is_success(), "{path}: {}", answer.status());
    }
    id
}

#[tokio::test]
#[ignore = "needs a running mcpd; set MCPD_TEST_ENDPOINT"]
async fn an_experiment_runs_over_nulls_and_empty_lists() {
    let Some(endpoint) = endpoint() else { return };
    let mcpd =
        Mcpd::with_config(Config::default().with_endpoint(endpoint.clone()).with_project(PROJECT));
    let nested = json!({ "q": "hi", "n": null, "l": [] });
    let dataset_id = seed_dataset(
        &endpoint,
        &[json!({ "input": nested, "expected": "hi" }), json!({ "input": null, "expected": "x" })],
    )
    .await;

    let seen = Mutex::new(Vec::new());
    let config: ScorerConfig = serde_json::from_value(json!({ "needle": "hi" })).unwrap();
    let results = mcpd
        .run_experiment(
            &dataset_id,
            |input| {
                seen.lock().unwrap().push(input);
                async { Ok::<_, String>("hi") }
            },
            &RunExperiment::new().config(config.clone()),
        )
        .await
        .expect("the daemon should accept every request the runner sends");
    let seen = seen.into_inner().unwrap();
    assert!(seen.contains(&nested) && seen.contains(&Value::Null), "inputs: {seen:?}");
    assert_eq!((results.runs, results.failed, results.items.len()), (2, 0, 2));

    let mut recorded = HashMap::new();
    for item in &results.items {
        let detail = mcpd.get_trace(&item.trace_id, Include::default()).await.unwrap();
        let root = detail.spans.iter().find(|s| s.is_root).expect("a root span");
        recorded.insert(item.item_id.clone().unwrap_or_default(), root.input.clone());
    }
    assert_eq!(recorded[&format!("{dataset_id}-0")], Some(nested));
    assert!(matches!(recorded[&format!("{dataset_id}-1")], None | Some(Value::Null)));

    let experiment = mcpd
        .create_experiment(&Experiment {
            experiment_id: String::new(),
            name: "rs-live-empty".to_owned(),
            dataset_id,
            status: ExperimentStatus::Running,
            started_at: None,
            finished_at: None,
            item_count: 0,
            summary: None,
            baseline_id: None,
            cost: None,
            error: None,
            metadata: None,
            correlation: serde_json::from_value(json!({ "project": "" })).unwrap(),
        })
        .await
        .expect("the experiment should be created");
    mcpd.score_experiment(&experiment.experiment_id, &[], None).await.expect("no scorers");
    mcpd.score_experiment(&experiment.experiment_id, &[], Some(&config))
        .await
        .expect("a config with every list empty");
    mcpd.shutdown().await.unwrap();
}
