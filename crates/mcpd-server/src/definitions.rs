//! Scorer definitions and live scoring rules, stored as versioned documents.
//!
//! A scorer definition is a project's own scorer: a judge prompt written for
//! its agent, or a code scorer with its settings fixed. An evaluation rule says
//! which finished runs `live.rs` scores, with what, and how many. Both are
//! bodies that never change once stored, numbered by the store, so a score can
//! always name the exact version that produced it.

use crate::auth::{authorize, principal, Scope};
use crate::error::{ApiError, ApiResult};
use crate::eval::Planned;
use crate::judge::JudgeConfig;
use crate::runs::select_project;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use mcpd_core::judged::Judged;
use mcpd_core::scoring::{Builtin, ScorerConfig};
use mcpd_core::{
    CreateEvaluationRuleRequest, CreateScorerDefinitionRequest, EvaluationRule, ScoreDirection,
    ScorerDefinition, ScorerDefinitionKind, ScorerDefinitionVersions,
};
use mcpd_store::{DocKind, NewDocument, StoredDocument};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use time::OffsetDateTime;

/// The judged scores a rule may produce in a minute when it does not say.
pub const DEFAULT_JUDGED_PER_MINUTE: u32 = 30;

/// The query the definition and rule routes take.
#[derive(Debug, Default, Deserialize)]
pub struct DocQuery {
    project: Option<String>,
    version: Option<u32>,
}

fn bad(field: &str, message: impl Into<String>) -> ApiError {
    ApiError::BadParam { field: field.to_owned(), message: message.into() }
}

/// An id: a lower-case letter, then lower-case letters, digits, `_` and,
/// where `dash` allows, `-`; 64 at most.
fn check_id(id: &str, dash: bool) -> ApiResult<()> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id.starts_with(|c: char| c.is_ascii_lowercase())
        && id.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || (dash && c == '-')
        });
    if ok {
        Ok(())
    } else {
        Err(bad(
            "id",
            format!(
                "{id:?} is not an id; start with a lower-case letter and use lower-case letters, \
                 digits{} and `_`, up to 64",
                if dash { ", `-`" } else { "" }
            ),
        ))
    }
}

fn opt_str(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_owned)
}

fn config_of(v: &Value) -> Option<ScorerConfig> {
    v.get("config").cloned().and_then(|c| serde_json::from_value(c).ok())
}

/// A stored definition version as the API returns it.
#[must_use]
pub fn to_definition(doc: &StoredDocument) -> ScorerDefinition {
    let b = &doc.body;
    ScorerDefinition {
        project: doc.project.clone(),
        id: doc.id.clone(),
        version: doc.version,
        kind: opt_str(b, "kind")
            .and_then(|k| k.parse().ok())
            .unwrap_or(ScorerDefinitionKind::Judge),
        description: opt_str(b, "description").unwrap_or_default(),
        direction: opt_str(b, "direction")
            .and_then(|d| d.parse().ok())
            .unwrap_or(ScoreDirection::HigherIsBetter),
        prompt: opt_str(b, "prompt"),
        builtin: opt_str(b, "builtin"),
        config: config_of(b),
        note: doc.note.clone(),
        created_by: doc.created_by.clone(),
        created_at: doc.created_at,
    }
}

/// What a pass runs for a stored definition.
///
/// # Errors
/// A definition whose code scorer this build no longer ships, which can only
/// be a definition stored by another version of mcpd.
pub fn planned_from(doc: &StoredDocument, judge: Option<&JudgeConfig>) -> ApiResult<Planned> {
    let d = to_definition(doc);
    match d.kind {
        ScorerDefinitionKind::Code => {
            let name = d.builtin.clone().unwrap_or_default();
            let builtin = Builtin::from_id(&name).ok_or_else(|| {
                bad(
                    "scorers",
                    format!(
                        "scorer definition {:?} runs the code scorer {name:?}, which this daemon \
                         does not ship; store a new version that names one it does",
                        d.id
                    ),
                )
            })?;
            Ok(Planned::Code {
                builtin,
                id: d.id,
                version: Some(d.version),
                config: Some(Box::new(d.config.unwrap_or_default())),
            })
        }
        ScorerDefinitionKind::Judge => Ok(Planned::Criterion {
            version: format!("{}:{}", d.version, judge.map(|j| j.model.as_str()).unwrap_or("")),
            id: d.id,
            text: d.prompt.unwrap_or_default(),
        }),
    }
}

/// The checked body of a definition request: the parts that make a version.
fn definition_body(state: &AppState, req: &CreateScorerDefinitionRequest) -> ApiResult<Value> {
    check_id(&req.id, false)?;
    if Builtin::from_id(&req.id).is_some() || Judged::from_id(&req.id).is_some() {
        return Err(bad(
            "id",
            format!("{:?} is the id of a scorer mcpd ships; choose another", req.id),
        ));
    }
    if state.config.judge.as_ref().is_some_and(|j| j.criteria.contains_key(&req.id)) {
        return Err(bad(
            "id",
            format!(
                "{:?} is a criterion in this daemon's config file, which would always run in its \
                 place; choose another id",
                req.id
            ),
        ));
    }
    if req.description.trim().is_empty() {
        return Err(bad("description", "say in a sentence what the scorer measures"));
    }
    let mut body = json!({
        "kind": req.kind.as_str(),
        "description": req.description.trim(),
        "direction": req.direction.unwrap_or(ScoreDirection::HigherIsBetter).as_str(),
    });
    match req.kind {
        ScorerDefinitionKind::Judge => {
            let prompt = req.prompt.as_deref().map(str::trim).unwrap_or_default();
            if prompt.is_empty() {
                return Err(bad(
                    "prompt",
                    "a judge definition needs a `prompt`: the criterion the model grades against",
                ));
            }
            if req.builtin.is_some() || req.config.is_some() {
                return Err(bad(
                    "builtin",
                    "a judge definition takes a `prompt`, not `builtin` or `config`",
                ));
            }
            body["prompt"] = Value::String(prompt.to_owned());
        }
        ScorerDefinitionKind::Code => {
            let name = req.builtin.as_deref().unwrap_or_default();
            let builtin = Builtin::from_id(name).ok_or_else(|| {
                let known: Vec<&str> = Builtin::ALL.iter().map(|b| b.id()).collect();
                bad(
                    "builtin",
                    format!("no code scorer called {name:?}; available: {}", known.join(", ")),
                )
            })?;
            if req.prompt.is_some() {
                return Err(bad(
                    "prompt",
                    "a code definition takes `builtin` and `config`, not a `prompt`",
                ));
            }
            let config = req.config.clone().unwrap_or_default();
            builtin.check_config(&config).map_err(|m| bad("config", format!("{name}: {m}")))?;
            if builtin.needs_config() && config == ScorerConfig::default() {
                return Err(bad(
                    "config",
                    format!("{name} needs settings to run: {}", builtin.config_keys().join(" or ")),
                ));
            }
            body["builtin"] = Value::String(name.to_owned());
            body["config"] = serde_json::to_value(&config).unwrap_or(Value::Null);
        }
    }
    Ok(body)
}

/// `GET /v1/api/scorer-definitions`
pub async fn list_scorer_definitions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<DocQuery>,
) -> ApiResult<Json<Vec<ScorerDefinition>>> {
    let grant = authorize(&state, &headers, Scope::Read)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    let docs = state.store.list_documents(&project, DocKind::Scorer, None).await?;
    Ok(Json(docs.iter().map(to_definition).collect()))
}

/// `POST /v1/api/scorer-definitions`
pub async fn create_scorer_definition(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<DocQuery>,
    Json(req): Json<CreateScorerDefinitionRequest>,
) -> ApiResult<Json<ScorerDefinition>> {
    let grant = authorize(&state, &headers, Scope::Write)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    let body = definition_body(&state, &req)?;
    let (doc, _) = state
        .store
        .put_document(&NewDocument {
            project,
            kind: DocKind::Scorer,
            id: req.id.clone(),
            body,
            note: req.note.clone().filter(|n| !n.trim().is_empty()),
            created_by: Some(principal(&headers)),
            created_at: OffsetDateTime::now_utc(),
        })
        .await?;
    Ok(Json(to_definition(&doc)))
}

/// `GET /v1/api/scorer-definitions/{id}`
pub async fn get_scorer_definition(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<DocQuery>,
) -> ApiResult<Json<ScorerDefinition>> {
    let grant = authorize(&state, &headers, Scope::Read)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    let doc = state
        .store
        .get_document(&project, DocKind::Scorer, &id, q.version)
        .await?
        .ok_or_else(|| match q.version {
            Some(v) => ApiError::NotFound(format!("scorer definition {id:?} has no version {v}")),
            None => {
                ApiError::NotFound(format!("no scorer definition {id:?} in project {project:?}"))
            }
        })?;
    Ok(Json(to_definition(&doc)))
}

/// `GET /v1/api/scorer-definitions/{id}/versions`
pub async fn list_scorer_definition_versions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<DocQuery>,
) -> ApiResult<Json<ScorerDefinitionVersions>> {
    let grant = authorize(&state, &headers, Scope::Read)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    let docs = state.store.list_documents(&project, DocKind::Scorer, Some(&id)).await?;
    if docs.is_empty() {
        return Err(ApiError::NotFound(format!(
            "no scorer definition {id:?} in project {project:?}"
        )));
    }
    Ok(Json(ScorerDefinitionVersions { id, versions: docs.iter().map(to_definition).collect() }))
}

// ------------------------------------------------------------------ rules ---

/// A stored rule version as the API returns it.
#[must_use]
pub fn to_rule(doc: &StoredDocument) -> EvaluationRule {
    let b = &doc.body;
    let strings = |k: &str| -> Vec<String> {
        b.get(k)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    };
    EvaluationRule {
        project: doc.project.clone(),
        id: doc.id.clone(),
        version: doc.version,
        enabled: b.get("enabled").and_then(Value::as_bool).unwrap_or(false),
        scorers: strings("scorers"),
        sample_rate: b.get("sample_rate").and_then(Value::as_f64).unwrap_or(1.0),
        name: opt_str(b, "name"),
        kind: opt_str(b, "kind").and_then(|k| k.parse().ok()),
        environment: opt_str(b, "environment"),
        tags: strings("tags"),
        status: opt_str(b, "status").and_then(|k| k.parse().ok()),
        config: config_of(b),
        max_judged_per_minute: b
            .get("max_judged_per_minute")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(DEFAULT_JUDGED_PER_MINUTE),
        note: doc.note.clone(),
        created_by: doc.created_by.clone(),
        created_at: doc.created_at,
    }
}

/// `GET /v1/api/evaluation-rules`
pub async fn list_evaluation_rules(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<DocQuery>,
) -> ApiResult<Json<Vec<EvaluationRule>>> {
    let grant = authorize(&state, &headers, Scope::Read)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    let docs = state.store.list_documents(&project, DocKind::EvalRule, None).await?;
    Ok(Json(docs.iter().map(to_rule).collect()))
}

/// `POST /v1/api/evaluation-rules`
pub async fn create_evaluation_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<DocQuery>,
    Json(req): Json<CreateEvaluationRuleRequest>,
) -> ApiResult<Json<EvaluationRule>> {
    let grant = authorize(&state, &headers, Scope::Write)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    check_id(&req.id, true)?;
    if req.scorers.is_empty() {
        return Err(bad("scorers", "name at least one scorer for the rule to run"));
    }
    let rate = req.sample_rate.unwrap_or(1.0);
    if !(0.0..=1.0).contains(&rate) || !rate.is_finite() {
        return Err(bad("sample_rate", format!("sample_rate must be between 0 and 1, got {rate}")));
    }
    // Every scorer is resolved now, so a rule that could never run is refused
    // when it is written rather than skipped in the background for ever.
    let planned = crate::eval::plan(&state, &project, &req.scorers).await?;
    let config = req.config.clone().unwrap_or_default();
    for p in &planned {
        if let Planned::Code { builtin, config: None, .. } = p {
            builtin
                .check_config(&config)
                .map_err(|m| bad("config", format!("{}: {m}", builtin.id())))?;
            if builtin.needs_config() && config == ScorerConfig::default() {
                return Err(bad(
                    "config",
                    format!(
                        "{} needs settings to run: {}",
                        builtin.id(),
                        builtin.config_keys().join(" or ")
                    ),
                ));
            }
        }
    }
    let mut body = json!({
        "enabled": req.enabled.unwrap_or(false),
        "scorers": req.scorers,
        "sample_rate": rate,
        "tags": req.tags.clone().unwrap_or_default(),
        "max_judged_per_minute": req.max_judged_per_minute.unwrap_or(DEFAULT_JUDGED_PER_MINUTE),
    });
    if let Some(v) = &req.name {
        body["name"] = Value::String(v.clone());
    }
    if let Some(v) = req.kind {
        body["kind"] = Value::String(v.as_str().to_owned());
    }
    if let Some(v) = &req.environment {
        body["environment"] = Value::String(v.clone());
    }
    if let Some(v) = req.status {
        body["status"] = Value::String(v.as_str().to_owned());
    }
    if let Some(c) = &req.config {
        body["config"] = serde_json::to_value(c).unwrap_or(Value::Null);
    }
    let (doc, _) = state
        .store
        .put_document(&NewDocument {
            project,
            kind: DocKind::EvalRule,
            id: req.id.clone(),
            body,
            note: req.note.clone().filter(|n| !n.trim().is_empty()),
            created_by: Some(principal(&headers)),
            created_at: OffsetDateTime::now_utc(),
        })
        .await?;
    Ok(Json(to_rule(&doc)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_checked() {
        assert!(check_id("cites_policy", false).is_ok());
        assert!(check_id("cites-policy", false).is_err());
        assert!(check_id("cites-policy", true).is_ok());
        assert!(check_id("Cites", true).is_err());
        assert!(check_id("9lives", true).is_err());
    }

    #[test]
    fn a_rule_reads_back_with_its_defaults() {
        let doc = StoredDocument {
            project: "p".into(),
            kind: DocKind::EvalRule,
            id: "r".into(),
            version: 2,
            body: json!({"enabled": true, "scorers": ["no_error"]}),
            note: None,
            created_by: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
        };
        let r = to_rule(&doc);
        assert!(r.enabled);
        assert!((r.sample_rate - 1.0).abs() < f64::EPSILON);
        assert_eq!(r.max_judged_per_minute, DEFAULT_JUDGED_PER_MINUTE);
        assert_eq!(r.scorers, ["no_error"]);
    }
}
