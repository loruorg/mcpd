//! Human curation: the review queue, and frozen dataset versions.
//!
//! Both are versioned documents. A review item's every change is a new
//! version, so who assigned it, who reviewed it and what they wrote is kept in
//! order. A dataset version is a copy of the cases at one moment, so an
//! experiment can name the exact cases it ran against.

use crate::auth::{authorize, principal, Scope};
use crate::error::{ApiError, ApiResult};
use crate::runs::select_project;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use mcpd_core::{
    CreateDatasetVersionRequest, CreateReviewItemRequest, DatasetItem, DatasetVersion, ReviewItem,
    ReviewQueueResponse, ReviewStatus, UpdateReviewItemRequest,
};
use mcpd_store::{DocKind, Include, NewDocument, StoredDocument};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use time::OffsetDateTime;

/// The most cases one dataset version freezes.
pub const MAX_VERSION_ITEMS: u32 = 5000;

/// The query of the review queue routes.
#[derive(Debug, Default, Deserialize)]
pub struct QueueQuery {
    project: Option<String>,
    status: Option<String>,
    assignee: Option<String>,
    limit: Option<u32>,
}

fn opt_str(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned)
}

fn to_item(doc: &StoredDocument) -> ReviewItem {
    let b = &doc.body;
    ReviewItem {
        project: doc.project.clone(),
        review_id: doc.id.clone(),
        version: doc.version,
        trace_id: opt_str(b, "trace_id").unwrap_or_default(),
        span_id: opt_str(b, "span_id"),
        feedback_id: opt_str(b, "feedback_id"),
        status: opt_str(b, "status").and_then(|s| s.parse().ok()).unwrap_or(ReviewStatus::Pending),
        assignee: opt_str(b, "assignee"),
        note: opt_str(b, "note"),
        updated_by: doc.created_by.clone(),
        created_at: b
            .get("created_at")
            .and_then(Value::as_str)
            .and_then(|s| {
                OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
            })
            .unwrap_or(doc.created_at),
        updated_at: doc.created_at,
    }
}

/// `GET /v1/api/review-queue`
pub async fn list_review_items(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<QueueQuery>,
) -> ApiResult<Json<ReviewQueueResponse>> {
    let grant = authorize(&state, &headers, Scope::Read)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    let status: Option<ReviewStatus> = match q.status.as_deref() {
        None | Some("") => None,
        Some(s) => Some(s.parse().map_err(|_| ApiError::BadParam {
            field: "status".to_owned(),
            message: format!("{s:?} is not a review status; use pending, reviewed or dismissed"),
        })?),
    };
    let all: Vec<ReviewItem> = state
        .store
        .list_documents(&project, DocKind::Review, None)
        .await?
        .iter()
        .map(to_item)
        .collect();
    let pending = u32::try_from(all.iter().filter(|i| i.status == ReviewStatus::Pending).count())
        .unwrap_or(u32::MAX);
    let mut items: Vec<ReviewItem> = all
        .into_iter()
        .filter(|i| status.is_none_or(|s| s == i.status))
        .filter(|i| q.assignee.as_ref().is_none_or(|a| i.assignee.as_ref() == Some(a)))
        .collect();
    items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.review_id.cmp(&b.review_id)));
    items.truncate(q.limit.unwrap_or(100).clamp(1, 500) as usize);
    Ok(Json(ReviewQueueResponse { items, pending }))
}

/// `POST /v1/api/review-queue`
pub async fn create_review_item(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<QueueQuery>,
    Json(req): Json<CreateReviewItemRequest>,
) -> ApiResult<Json<ReviewItem>> {
    let grant = authorize(&state, &headers, Scope::Write)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    // The run has to exist here: a queue of ids that point at nothing is a
    // queue nobody can work through.
    state.store.get_trace(&project, &req.trace_id, &Include::default()).await.map_err(
        |e| match e {
            mcpd_store::Error::NotFound { .. } => {
                ApiError::NotFound(format!("no run {} in project {project:?}", req.trace_id))
            }
            other => other.into(),
        },
    )?;
    let now = OffsetDateTime::now_utc();
    let body = json!({
        "trace_id": req.trace_id,
        "span_id": req.span_id,
        "feedback_id": req.feedback_id,
        "status": ReviewStatus::Pending.as_str(),
        "assignee": req.assignee,
        "note": req.note,
        "created_at": now.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
    });
    let (doc, _) = state
        .store
        .put_document(&NewDocument {
            project,
            kind: DocKind::Review,
            id: mcpd_core::ids::new_trace_id(),
            body,
            note: None,
            created_by: Some(principal(&headers)),
            created_at: now,
        })
        .await?;
    Ok(Json(to_item(&doc)))
}

/// `PATCH /v1/api/review-queue/{review_id}`
pub async fn update_review_item(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<QueueQuery>,
    Json(req): Json<UpdateReviewItemRequest>,
) -> ApiResult<Json<ReviewItem>> {
    let grant = authorize(&state, &headers, Scope::Write)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    let held =
        state.store.get_document(&project, DocKind::Review, &id, None).await?.ok_or_else(|| {
            ApiError::NotFound(format!("no review item {id:?} in project {project:?}"))
        })?;
    let mut body = held.body.clone();
    if let Some(s) = req.status {
        body["status"] = Value::String(s.as_str().to_owned());
    }
    if let Some(a) = &req.assignee {
        body["assignee"] = if a.is_empty() { Value::Null } else { Value::String(a.clone()) };
    }
    if let Some(n) = &req.note {
        body["note"] = Value::String(n.clone());
    }
    let (doc, _) = state
        .store
        .put_document(&NewDocument {
            project,
            kind: DocKind::Review,
            id,
            body,
            note: None,
            created_by: Some(principal(&headers)),
            created_at: OffsetDateTime::now_utc(),
        })
        .await?;
    Ok(Json(to_item(&doc)))
}

// ------------------------------------------------------- dataset versions ---

/// The project that owns a dataset, refusing a key that does not reach it the
/// same way the item routes do, so this cannot test which ids exist.
async fn owner(state: &AppState, grant: &crate::auth::Grant, id: &str) -> ApiResult<String> {
    match state.store.dataset_project(id).await? {
        Some(owner) if grant.covers(&owner) => Ok(owner),
        Some(_) => Err(ApiError::Forbidden(format!(
            "that key does not reach the project that owns dataset {id}"
        ))),
        None => Err(ApiError::NotFound(format!("no dataset with id {id}"))),
    }
}

fn to_version(doc: &StoredDocument) -> DatasetVersion {
    DatasetVersion {
        dataset_id: doc.id.clone(),
        version: doc.version,
        item_count: doc
            .body
            .get("items")
            .and_then(Value::as_array)
            .map_or(0, |a| u32::try_from(a.len()).unwrap_or(u32::MAX)),
        note: doc.note.clone(),
        created_by: doc.created_by.clone(),
        created_at: doc.created_at,
    }
}

/// `POST /v1/api/datasets/{dataset_id}/versions`
pub async fn create_dataset_version(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<CreateDatasetVersionRequest>,
) -> ApiResult<Json<DatasetVersion>> {
    let grant = authorize(&state, &headers, Scope::Write)?;
    let project = owner(&state, &grant, &id).await?;
    let mut items = state.store.list_dataset_items(&id, MAX_VERSION_ITEMS + 1).await?;
    if items.len() > MAX_VERSION_ITEMS as usize {
        return Err(ApiError::BadRequest(format!(
            "dataset {id} holds more than {MAX_VERSION_ITEMS} cases, the most one version freezes; \
             split it into smaller datasets"
        )));
    }
    // A stable order, so freezing the same cases twice is the same body and
    // confirms the version rather than storing a copy.
    items.sort_by(|a, b| a.item_id.cmp(&b.item_id));
    let (doc, _) = state
        .store
        .put_document(&NewDocument {
            project,
            kind: DocKind::DatasetVersion,
            id: id.clone(),
            body: json!({ "items": items }),
            note: req.note.clone().filter(|n| !n.trim().is_empty()),
            created_by: Some(principal(&headers)),
            created_at: OffsetDateTime::now_utc(),
        })
        .await?;
    Ok(Json(to_version(&doc)))
}

/// `GET /v1/api/datasets/{dataset_id}/versions`
pub async fn list_dataset_versions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<DatasetVersion>>> {
    let grant = authorize(&state, &headers, Scope::Read)?;
    let project = owner(&state, &grant, &id).await?;
    let docs = state.store.list_documents(&project, DocKind::DatasetVersion, Some(&id)).await?;
    Ok(Json(docs.iter().map(to_version).collect()))
}

/// The cases a frozen version holds, for the item list.
///
/// # Errors
/// A version that does not exist.
pub async fn version_items(
    state: &AppState,
    project: &str,
    id: &str,
    version: u32,
    limit: u32,
) -> ApiResult<Vec<DatasetItem>> {
    let doc = state
        .store
        .get_document(project, DocKind::DatasetVersion, id, Some(version))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("dataset {id} has no version {version}")))?;
    let items: Vec<DatasetItem> =
        serde_json::from_value(doc.body.get("items").cloned().unwrap_or(Value::Null))
            .unwrap_or_default();
    Ok(items.into_iter().take(limit as usize).collect())
}
