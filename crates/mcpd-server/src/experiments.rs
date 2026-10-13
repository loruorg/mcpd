//! Experiments, case by case: scoring each run against its own case, and
//! reading the results back.
//!
//! The daemon never runs the agent under test. An SDK's experiment runner
//! does: it reads the dataset, calls the target once per case inside a span
//! that carries the experiment id and the case id, and then asks this module
//! to score what was recorded. Keeping the call in the caller's process is
//! what lets the target be any function at all.

use crate::auth::{authorize, Scope};
use crate::error::{ApiError, ApiResult};
use crate::eval::{plan, score_trace, Pass, Planned};
use crate::runs::select_project;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use mcpd_core::{
    DatasetItem, Experiment, ExperimentItemResult, ExperimentResults, ExperimentScoreRequest,
    ScoreDirection, ScoreRunResult, ScoreSource, ScorerSummary, Span, Trace,
};
use mcpd_store::{DocKind, Include, ScoreQuery, TraceQuery};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// The attribute an SDK writes the dataset case a run answered under.
pub const ITEM_ATTRIBUTE: &str = "mcpd.dataset_item_id";

/// The most runs one experiment is read or scored over in one request.
const MAX_RUNS: usize = 2000;

/// The query the experiment routes take.
#[derive(Debug, Default, Deserialize)]
pub struct ProjectOnly {
    project: Option<String>,
}

async fn experiment(state: &AppState, project: &str, id: &str) -> ApiResult<Experiment> {
    state
        .store
        .list_experiments(project)
        .await?
        .into_iter()
        .find(|e| e.experiment_id == id)
        .ok_or_else(|| ApiError::NotFound(format!("no experiment {id:?} in project {project:?}")))
}

/// Every run recorded under an experiment, oldest first.
async fn runs_of(state: &AppState, project: &str, id: &str) -> ApiResult<Vec<Trace>> {
    let mut out = Vec::new();
    let mut cursor = None;
    loop {
        let page = state
            .store
            .list_traces(&TraceQuery {
                project: project.to_owned(),
                experiment_id: Some(id.to_owned()),
                order: mcpd_store::Order::Asc,
                limit: 500,
                cursor: cursor.clone(),
                ..Default::default()
            })
            .await?;
        out.extend(page.items);
        match page.next_cursor {
            Some(c) if out.len() < MAX_RUNS => cursor = Some(c),
            _ => break,
        }
    }
    out.truncate(MAX_RUNS);
    Ok(out)
}

/// The cases of the dataset an experiment ran, by id.
async fn cases(state: &AppState, e: &Experiment) -> ApiResult<HashMap<String, DatasetItem>> {
    if e.dataset_id.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(state
        .store
        .list_dataset_items(&e.dataset_id, 5000)
        .await?
        .into_iter()
        .map(|i| (i.item_id.clone(), i))
        .collect())
}

/// The case a run says it answered.
fn item_of(spans: &[Span]) -> Option<String> {
    let root = spans.iter().find(|s| s.is_root).or_else(|| spans.first())?;
    root.attributes.as_ref()?.get(ITEM_ATTRIBUTE)?.as_str().map(str::to_owned)
}

/// A case's expected answer as the text the code scorers compare with.
fn expected_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// `POST /v1/api/experiments/{experiment_id}/score`
pub async fn score_experiment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ProjectOnly>,
    Json(req): Json<ExperimentScoreRequest>,
) -> ApiResult<Json<ScoreRunResult>> {
    let grant = authorize(&state, &headers, Scope::Write)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    let e = experiment(&state, &project, &id).await?;
    let chosen = plan(&state, &project, &req.scorers).await?;
    let base = req.config.clone().unwrap_or_default();
    for p in &chosen {
        if let Planned::Code { builtin, config: None, .. } = p {
            // `expected` comes from each case, so a scorer that needs only it
            // is not refused for lacking it here.
            if builtin.config_keys() != ["expected"] {
                builtin.check_config(&base).map_err(|message| ApiError::BadParam {
                    field: "config".to_owned(),
                    message: format!("{}: {message}", builtin.id()),
                })?;
            }
        }
    }
    let items = cases(&state, &e).await?;
    let runs = runs_of(&state, &project, &id).await?;
    let mut result = ScoreRunResult {
        traces_scored: 0,
        scores_written: 0,
        skipped: Vec::new(),
        scores: Vec::new(),
    };
    for run in runs {
        let detail = state.store.get_trace(&project, &run.trace_id, &Include::default()).await?;
        let mut config = base.clone();
        if let Some(expected) =
            item_of(&detail.spans).and_then(|i| items.get(&i)).and_then(|i| i.expected.as_ref())
        {
            config.expected = Some(expected_text(expected));
        }
        // Each run gets the whole judge deadline the request has left, which
        // is how `POST /v1/api/score` paces itself too.
        let pass = Pass {
            project: &project,
            config: &config,
            source: ScoreSource::Experiment,
            dry_run: req.dry_run,
            judge_by: std::time::Instant::now()
                + std::time::Duration::from_secs(state.config.request_timeout_seconds.max(1)),
        };
        score_trace(&state, &pass, &chosen, &run.trace_id, &mut result).await?;
    }
    Ok(Json(result))
}

/// `GET /v1/api/experiments/{experiment_id}/results`
pub async fn get_experiment_results(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ProjectOnly>,
) -> ApiResult<Json<ExperimentResults>> {
    let grant = authorize(&state, &headers, Scope::Read)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    let e = experiment(&state, &project, &id).await?;
    let items = cases(&state, &e).await?;
    let runs = runs_of(&state, &project, &id).await?;

    // Every score recorded under the experiment, keyed by run then scorer.
    let mut by_run: HashMap<String, BTreeMap<String, (f64, u32)>> = HashMap::new();
    let mut by_scorer: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut cursor = None;
    loop {
        let page = state
            .store
            .list_scores(&ScoreQuery {
                project: project.clone(),
                experiment_id: Some(id.clone()),
                limit: 500,
                cursor: cursor.clone(),
                ..Default::default()
            })
            .await?;
        for s in &page.items {
            if let Some(t) = &s.trace_id {
                let e =
                    by_run.entry(t.clone()).or_default().entry(s.scorer_id.clone()).or_default();
                e.0 += s.value;
                e.1 += 1;
            }
            by_scorer.entry(s.scorer_id.clone()).or_default().push(s.value);
        }
        match page.next_cursor {
            Some(c) if by_scorer.values().map(Vec::len).sum::<usize>() < MAX_RUNS * 20 => {
                cursor = Some(c);
            }
            _ => break,
        }
    }

    let defined: HashMap<String, ScoreDirection> = state
        .store
        .list_documents(&project, DocKind::Scorer, None)
        .await?
        .iter()
        .map(|d| {
            let d = crate::definitions::to_definition(d);
            (d.id, d.direction)
        })
        .collect();

    let mut out = Vec::with_capacity(runs.len());
    for run in &runs {
        let detail = state.store.get_trace(&project, &run.trace_id, &Include::default()).await?;
        let item_id = item_of(&detail.spans);
        let scores: serde_json::Map<String, Value> = by_run
            .get(&run.trace_id)
            .map(|m| {
                m.iter()
                    .map(|(k, (sum, n))| (k.clone(), Value::from(sum / f64::from((*n).max(1)))))
                    .collect()
            })
            .unwrap_or_default();
        out.push(ExperimentItemResult {
            trace_id: run.trace_id.clone(),
            expected: item_id.as_ref().and_then(|i| items.get(i)).and_then(|i| i.expected.clone()),
            item_id,
            status: run.status,
            duration_ns: run.duration_ns,
            cost: run.cost.as_ref().and_then(|c| c.total_cost),
            input_preview: run.input_preview.clone(),
            output_preview: run.output_preview.clone(),
            scores: Value::Object(scores),
        });
    }
    let scorers = by_scorer
        .into_iter()
        .map(|(scorer_id, v)| {
            let n = v.len();
            let mean = v.iter().sum::<f64>() / n.max(1) as f64;
            ScorerSummary {
                direction: defined
                    .get(&scorer_id)
                    .copied()
                    .unwrap_or_else(|| crate::eval::direction_of(&scorer_id)),
                n: n as u64,
                mean,
                min: v.iter().copied().fold(f64::INFINITY, f64::min),
                max: v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                scorer_id,
            }
        })
        .collect();
    Ok(Json(ExperimentResults {
        experiment_id: id,
        runs: u32::try_from(runs.len()).unwrap_or(u32::MAX),
        failed: u32::try_from(
            runs.iter().filter(|r| r.status == mcpd_core::SpanStatus::Error).count(),
        )
        .unwrap_or(u32::MAX),
        items: out,
        scorers,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_expected_answer_reads_as_text() {
        assert_eq!(expected_text(&json!("Paris")), "Paris");
        assert_eq!(expected_text(&json!({"city": "Paris"})), r#"{"city":"Paris"}"#);
    }
}
