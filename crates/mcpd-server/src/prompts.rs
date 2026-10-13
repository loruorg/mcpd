//! Prompt management: versioned prompts with labels.
//!
//! The daemon stores prompts and hands them out. It never renders one and
//! never calls a model with one: rendering happens in the SDK, in the caller's
//! process, so a prompt fetch is one indexed read and the daemon stays out of
//! the request path of the agent it observes.

use crate::auth::{authorize, principal, Scope};
use crate::error::{ApiError, ApiResult};
use crate::runs::select_project;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use mcpd_core::{
    CreatePromptRequest, DeleteResult, Prompt, PromptKind, PromptLabelRequest,
    PromptVersionsResponse,
};
use mcpd_store::{DocKind, DocumentLabel, NewDocument, StoredDocument};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use time::OffsetDateTime;

/// The label that always means the newest version. It cannot be set.
pub const LATEST: &str = "latest";

/// The query every prompt route takes.
#[derive(Debug, Default, Deserialize)]
pub struct PromptQuery {
    project: Option<String>,
    label: Option<String>,
    version: Option<u32>,
}

/// The query of the routes that take nothing but a project.
#[derive(Debug, Default, Deserialize)]
pub struct ProjectOnly {
    project: Option<String>,
}

/// A prompt name: letters, digits, `-`, `_`, `.` and `/`, 1 to 128 of them.
fn check_name(name: &str) -> ApiResult<()> {
    let ok = !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('/')
        && !name.ends_with('/')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c));
    if ok {
        Ok(())
    } else {
        Err(ApiError::BadParam {
            field: "name".to_owned(),
            message: format!(
                "{name:?} is not a prompt name; use 1 to 128 letters, digits, `-`, `_`, `.` or \
                 `/`, not starting or ending with `/`"
            ),
        })
    }
}

/// A label: letters, digits, `-`, `_` and `.`, 1 to 64 of them.
fn check_label(label: &str) -> ApiResult<()> {
    let ok = !label.is_empty()
        && label.len() <= 64
        && label.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    if !ok {
        return Err(ApiError::BadParam {
            field: "label".to_owned(),
            message: format!(
                "{label:?} is not a label; use 1 to 64 letters, digits, `-`, `_` or `.`"
            ),
        });
    }
    Ok(())
}

/// Every `{{variable}}` a template names, in order of first use.
///
/// Whitespace inside the braces is allowed, so `{{ name }}` is `name`. A pair
/// of braces around anything that is not a name is left alone and names
/// nothing, so a JSON example inside a prompt does not invent variables.
pub fn variables_in(template: &str, out: &mut Vec<String>) {
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else { break };
        let name = after[..close].trim();
        let valid = !name.is_empty()
            && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
        if valid && !out.iter().any(|v| v == name) {
            out.push(name.to_owned());
        }
        rest = &after[close + 2..];
    }
}

fn variables_in_value(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => variables_in(s, out),
        Value::Array(a) => a.iter().for_each(|x| variables_in_value(x, out)),
        Value::Object(o) => o.values().for_each(|x| variables_in_value(x, out)),
        _ => {}
    }
}

/// The stored body of a request, checked: the parts that make a version.
///
/// The note is not part of it, so a second post that differs only in its note
/// confirms the version rather than storing a copy.
fn body_of(req: &CreatePromptRequest) -> ApiResult<Value> {
    let kind = req.kind.unwrap_or(if req.messages.is_some() {
        PromptKind::Chat
    } else {
        PromptKind::Text
    });
    match kind {
        PromptKind::Text => {
            if req.template.as_deref().is_none_or(|t| t.trim().is_empty()) {
                return Err(ApiError::BadParam {
                    field: "template".to_owned(),
                    message: "a text prompt needs a non-empty `template`".to_owned(),
                });
            }
            if req.messages.is_some() {
                return Err(ApiError::BadParam {
                    field: "messages".to_owned(),
                    message: "a text prompt has a `template` and no `messages`; send \
                              `kind: \"chat\"` to store messages"
                        .to_owned(),
                });
            }
        }
        PromptKind::Chat => {
            let Some(Value::Array(msgs)) = &req.messages else {
                return Err(ApiError::BadParam {
                    field: "messages".to_owned(),
                    message: "a chat prompt needs `messages`, an array of objects with `role` \
                              and `content`"
                        .to_owned(),
                });
            };
            if msgs.is_empty() {
                return Err(ApiError::BadParam {
                    field: "messages".to_owned(),
                    message: "a chat prompt needs at least one message".to_owned(),
                });
            }
            for (i, m) in msgs.iter().enumerate() {
                let role_ok = m.get("role").and_then(Value::as_str).is_some_and(|r| !r.is_empty());
                let content_ok = m.get("content").is_some_and(|c| c.is_string() || c.is_array());
                if !role_ok || !content_ok {
                    return Err(ApiError::BadParam {
                        field: format!("messages[{i}]"),
                        message: "each message needs a string `role` and a `content` that is a \
                                  string or an array of parts"
                            .to_owned(),
                    });
                }
            }
            if req.template.is_some() {
                return Err(ApiError::BadParam {
                    field: "template".to_owned(),
                    message: "a chat prompt has `messages` and no `template`".to_owned(),
                });
            }
        }
    }
    if let Some(c) = &req.config {
        if !c.is_object() {
            return Err(ApiError::BadParam {
                field: "config".to_owned(),
                message: "`config` is an object of model settings, such as \
                          {\"model\": \"gpt-4o\", \"temperature\": 0.2}"
                    .to_owned(),
            });
        }
    }
    let mut body = json!({ "kind": kind.as_str() });
    if let Some(t) = &req.template {
        body["template"] = Value::String(t.clone());
    }
    if let Some(m) = &req.messages {
        body["messages"] = m.clone();
    }
    if let Some(c) = &req.config {
        body["config"] = c.clone();
    }
    Ok(body)
}

/// A stored version as the API returns it, with the labels that point at it.
fn to_prompt(doc: StoredDocument, labels: &[DocumentLabel]) -> Prompt {
    let body = &doc.body;
    let kind = body
        .get("kind")
        .and_then(Value::as_str)
        .and_then(|k| k.parse().ok())
        .unwrap_or(PromptKind::Text);
    let template = body.get("template").and_then(Value::as_str).map(str::to_owned);
    let messages = body.get("messages").cloned();
    let mut variables = Vec::new();
    if let Some(t) = &template {
        variables_in(t, &mut variables);
    }
    if let Some(m) = &messages {
        variables_in_value(m, &mut variables);
    }
    Prompt {
        project: doc.project,
        name: doc.id.clone(),
        version: doc.version,
        kind,
        template,
        messages,
        config: body.get("config").cloned(),
        variables,
        labels: labels
            .iter()
            .filter(|l| l.id == doc.id && l.version == doc.version)
            .map(|l| l.label.clone())
            .collect(),
        note: doc.note,
        created_by: doc.created_by,
        created_at: doc.created_at,
    }
}

fn no_prompt(name: &str, project: &str) -> ApiError {
    ApiError::NotFound(format!("no prompt called {name:?} in project {project:?}"))
}

/// `GET /v1/api/prompts`
pub async fn list_prompts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<ProjectOnly>,
) -> ApiResult<Json<Vec<Prompt>>> {
    let grant = authorize(&state, &headers, Scope::Read)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    let docs = state.store.list_documents(&project, DocKind::Prompt, None).await?;
    let labels = state.store.list_document_labels(&project, DocKind::Prompt, None).await?;
    Ok(Json(docs.into_iter().map(|d| to_prompt(d, &labels)).collect()))
}

/// `POST /v1/api/prompts`
pub async fn create_prompt(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<ProjectOnly>,
    Json(req): Json<CreatePromptRequest>,
) -> ApiResult<Json<Prompt>> {
    let grant = authorize(&state, &headers, Scope::Write)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    check_name(&req.name)?;
    let labels = req.labels.clone().unwrap_or_default();
    for l in &labels {
        check_label(l)?;
        if l == LATEST {
            return Err(reserved());
        }
    }
    let body = body_of(&req)?;
    // Through the redactor like every other write: a prompt pasted with a key
    // in it is still a prompt with a key in it.
    let body = state.redactor.redact(&body);
    let (doc, _) = state
        .store
        .put_document(&NewDocument {
            project: project.clone(),
            kind: DocKind::Prompt,
            id: req.name.clone(),
            body,
            note: req.note.clone().filter(|n| !n.trim().is_empty()),
            created_by: Some(principal(&headers)),
            created_at: OffsetDateTime::now_utc(),
        })
        .await?;
    for l in &labels {
        state
            .store
            .set_document_label(&project, DocKind::Prompt, &doc.id, l, Some(doc.version))
            .await?;
    }
    let all = state.store.list_document_labels(&project, DocKind::Prompt, Some(&doc.id)).await?;
    Ok(Json(to_prompt(doc, &all)))
}

fn reserved() -> ApiError {
    ApiError::BadParam {
        field: "label".to_owned(),
        message: "`latest` always means the newest version and cannot be set or removed".to_owned(),
    }
}

/// `GET /v1/api/prompts/{name}`
pub async fn get_prompt(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(q): Query<PromptQuery>,
) -> ApiResult<Json<Prompt>> {
    let grant = authorize(&state, &headers, Scope::Read)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    let labels = state.store.list_document_labels(&project, DocKind::Prompt, Some(&name)).await?;
    let version = match (q.label.as_deref(), q.version) {
        (Some(_), Some(_)) => {
            return Err(ApiError::BadParam {
                field: "version".to_owned(),
                message: "name a `label` or a `version`, not both".to_owned(),
            })
        }
        (Some(LATEST), None) | (None, None) => None,
        (Some(label), None) => match labels.iter().find(|l| l.label == label) {
            Some(l) => Some(l.version),
            None => {
                return Err(ApiError::NotFound(format!(
                    "no version of prompt {name:?} carries the label {label:?}"
                )))
            }
        },
        (None, Some(v)) => Some(v),
    };
    let doc =
        state.store.get_document(&project, DocKind::Prompt, &name, version).await?.ok_or_else(
            || match version {
                Some(v) => ApiError::NotFound(format!("prompt {name:?} has no version {v}")),
                None => no_prompt(&name, &project),
            },
        )?;
    Ok(Json(to_prompt(doc, &labels)))
}

/// `GET /v1/api/prompts/{name}/versions`
pub async fn list_prompt_versions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(q): Query<ProjectOnly>,
) -> ApiResult<Json<PromptVersionsResponse>> {
    let grant = authorize(&state, &headers, Scope::Read)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    let docs = state.store.list_documents(&project, DocKind::Prompt, Some(&name)).await?;
    if docs.is_empty() {
        return Err(no_prompt(&name, &project));
    }
    let labels = state.store.list_document_labels(&project, DocKind::Prompt, Some(&name)).await?;
    Ok(Json(PromptVersionsResponse {
        name,
        versions: docs.into_iter().map(|d| to_prompt(d, &labels)).collect(),
    }))
}

/// `PUT /v1/api/prompts/{name}/labels/{label}`
pub async fn set_prompt_label(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, label)): Path<(String, String)>,
    Query(q): Query<ProjectOnly>,
    Json(req): Json<PromptLabelRequest>,
) -> ApiResult<Json<Prompt>> {
    let grant = authorize(&state, &headers, Scope::Write)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    check_label(&label)?;
    if label == LATEST {
        return Err(reserved());
    }
    let set = state
        .store
        .set_document_label(&project, DocKind::Prompt, &name, &label, Some(req.version))
        .await?;
    if !set {
        return Err(ApiError::NotFound(format!(
            "prompt {name:?} has no version {} in project {project:?}",
            req.version
        )));
    }
    let doc = state
        .store
        .get_document(&project, DocKind::Prompt, &name, Some(req.version))
        .await?
        .ok_or_else(|| no_prompt(&name, &project))?;
    let labels = state.store.list_document_labels(&project, DocKind::Prompt, Some(&name)).await?;
    Ok(Json(to_prompt(doc, &labels)))
}

/// `DELETE /v1/api/prompts/{name}/labels/{label}`
pub async fn delete_prompt_label(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((name, label)): Path<(String, String)>,
    Query(q): Query<ProjectOnly>,
) -> ApiResult<Json<DeleteResult>> {
    let grant = authorize(&state, &headers, Scope::Write)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    if label == LATEST {
        return Err(reserved());
    }
    let removed =
        state.store.set_document_label(&project, DocKind::Prompt, &name, &label, None).await?;
    Ok(Json(DeleteResult { deleted: u32::from(removed) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_are_named_once_in_order_of_first_use() {
        let mut v = Vec::new();
        variables_in("Hi {{ name }}, about {{topic}}. Bye {{name}}. {\"a\": {{1}}}", &mut v);
        assert_eq!(v, ["name", "topic"]);
    }

    #[test]
    fn a_chat_prompt_needs_messages_with_a_role_and_content() {
        let req = |messages: Value| CreatePromptRequest {
            name: "x".into(),
            kind: Some(PromptKind::Chat),
            template: None,
            messages: Some(messages),
            config: None,
            note: None,
            labels: None,
        };
        assert!(body_of(&req(json!([{"role": "system", "content": "Be brief."}]))).is_ok());
        assert!(body_of(&req(json!([{"content": "no role"}]))).is_err());
        assert!(body_of(&req(json!([]))).is_err());
        assert!(body_of(&req(json!("not a list"))).is_err());
    }

    #[test]
    fn the_kind_follows_the_body_when_it_is_not_named() {
        let req = CreatePromptRequest {
            name: "x".into(),
            kind: None,
            template: None,
            messages: Some(json!([{"role": "user", "content": "{{q}}"}])),
            config: None,
            note: Some("ignored for equality".into()),
            labels: None,
        };
        assert_eq!(body_of(&req).unwrap()["kind"], "chat");
    }

    #[test]
    fn names_and_labels_are_checked() {
        assert!(check_name("support/triage-v2.1").is_ok());
        assert!(check_name("/leading").is_err());
        assert!(check_name("has space").is_err());
        assert!(check_name(&"a".repeat(129)).is_err());
        assert!(check_label("production").is_ok());
        assert!(check_label("prod/eu").is_err());
    }
}
