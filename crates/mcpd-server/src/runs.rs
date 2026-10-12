//! The run surface: definitions, runs, events and the worker protocol.
//!
//! Sixteen endpoints over `mcpd_runs::RunStore`, the binding text for which is
//! `internal/run-contract-v1.md`. The daemon owns the run between steps and
//! never runs a step body: a worker, which is the user's own process in any
//! language, claims a step, runs it and reports back. Nothing here calls a
//! model or holds a provider key.
//!
//! Two background loops live here as well. The tick fires due deadlines and
//! repairs stalled runs, and the outbox drain writes the spans every run
//! leaves behind through the same ingest path an SDK batch takes, so a run's
//! trace is priced, redacted, stored and forwarded like any other.
//!
//! The project a request names is a selector and never an authority: the key
//! decides what it reaches, the name only says which of those to use.

use crate::auth::{authorize, Grant, Scope};
use crate::error::{ApiError, ApiResult};
use crate::query::Peer;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::StreamExt;
use mcpd_core as wire;
use mcpd_runs::{
    Attempt, Definition, DefinitionDocument, InboundEvent, Lease, RunFilter, RunStore, StartRun,
    StepCompletion, StepError, Timer, Wait,
};
use serde::Deserialize;
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use time::OffsetDateTime;

/// The most leases one claim may ask for.
const MAX_CLAIM: u32 = 100;
/// The longest `wait_ms` a claim may ask for, in milliseconds. What the daemon
/// actually waits is also capped by [`claim_wait_cap`].
const MAX_WAIT_MS: u32 = 60_000;
/// The most event rows one page carries.
const MAX_EVENT_PAGE: usize = 1000;
/// The furthest back a cause walk goes, which is also its default. The
/// spec's own default is 32 and this caps a caller who asks for more, so one
/// request cannot walk a whole definition's worth of edges under one read.
const MAX_CAUSE_DEPTH: u32 = 32;
/// How many outbox rows one drain pass takes.
const DRAIN_BATCH: usize = 256;

// ---------------------------------------------------------------- helpers ---

/// The run store, or the 503 that says this daemon does not serve runs.
fn runs(state: &AppState) -> ApiResult<Arc<dyn RunStore>> {
    state.runs.clone().ok_or_else(|| {
        ApiError::Unavailable(
            "this daemon is not serving runs; start it with --runs-data to turn the run \
             store on"
                .to_owned(),
        )
    })
}

/// Which project a request means, given what its key reaches.
///
/// A named project must be covered by the grant. An unnamed one is filled in
/// from a single-project grant, or from the daemon's default project when the
/// key reaches everything, and refused when the key reaches several projects
/// and the request names none, because a guessed project is the wrong one
/// often enough to be worth a 400.
fn select_project(state: &AppState, grant: &Grant, requested: Option<&str>) -> ApiResult<String> {
    match (requested.map(str::trim).filter(|p| !p.is_empty()), grant) {
        (Some(p), grant) if grant.covers(p) => Ok(p.to_owned()),
        (Some(p), _) => {
            Err(ApiError::Forbidden(format!("that key does not reach the project {p:?}")))
        }
        (None, Grant::All) => Ok(state.config.default_project.clone()),
        (None, Grant::Only(set)) if set.len() == 1 => {
            Ok(set.iter().next().cloned().unwrap_or_default())
        }
        (None, Grant::Only(set)) => Err(ApiError::BadParam {
            field: "project".to_owned(),
            message: format!(
                "this key reaches {} projects; name the one you mean in `project`",
                set.len()
            ),
        }),
    }
}

/// The 403 for a run another project owns. Worded without saying whether the
/// run exists, for the same reason `authorize_project` is.
fn reach(grant: &Grant, project: &str) -> ApiResult<()> {
    if grant.covers(project) {
        Ok(())
    } else {
        Err(ApiError::Forbidden(format!("that key does not reach the project {project:?}")))
    }
}

/// Map a run store failure onto the HTTP answer the contract gives it.
fn map_err(err: mcpd_runs::Error) -> ApiError {
    match err {
        mcpd_runs::Error::NotFound { kind, id } => {
            ApiError::NotFound(format!("{kind} {id} not found"))
        }
        mcpd_runs::Error::Invalid(message) => ApiError::BadRequest(message),
        mcpd_runs::Error::VersionTaken { project, id, version } => ApiError::VersionTaken(format!(
            "definition {id} version {version} in project {project} is already held by a \
             different body; a stored version never changes, so name another number or \
             leave version out and read the assigned one back"
        )),
        mcpd_runs::Error::Conflict { reason, attempt, attempt_status, run_status, message } => {
            let body = || -> ApiResult<wire::StepConflict> {
                Ok(wire::StepConflict {
                    error: "conflict".to_owned(),
                    reason: parse_wire(reason.as_str())?,
                    message,
                    attempt: u32::try_from(attempt).unwrap_or(u32::MAX),
                    attempt_status: match attempt_status {
                        Some(status) => Some(parse_wire(status.as_str())?),
                        None => None,
                    },
                    run_status: parse_wire(run_status.as_str())?,
                })
            };
            match body() {
                Ok(conflict) => ApiError::StepConflict(Box::new(conflict)),
                Err(e) => e,
            }
        }
        mcpd_runs::Error::Backend(message) => ApiError::Internal(format!("run store: {message}")),
        mcpd_runs::Error::Retry(message) => ApiError::Internal(format!("run store: {message}")),
    }
}

/// A crate enum value, respelled as the generated wire enum.
///
/// Both sides spell every value from the contract's table, and a test in this
/// module walks every value of every enum through this function, so a value
/// that stops parsing is a test failure here rather than a 500 in production.
fn parse_wire<T: FromStr>(s: &str) -> ApiResult<T> {
    T::from_str(s).map_err(|_| {
        ApiError::Internal(format!("the run store spelled {s:?}, which the wire does not know"))
    })
}

fn empty_correlation(project: &str) -> wire::Correlation {
    wire::Correlation {
        project: project.to_owned(),
        environment: None,
        service_name: None,
        service_version: None,
        session_id: None,
        thread_id: None,
        user_id: None,
        resource_id: None,
        run_id: None,
        request_id: None,
        experiment_id: None,
        release: None,
    }
}

fn correlation_of(value: &serde_json::Value, project: &str, run_id: &str) -> wire::Correlation {
    let mut c: wire::Correlation = match value {
        serde_json::Value::Object(map) => {
            let mut with_project = map.clone();
            with_project
                .entry("project")
                .or_insert_with(|| serde_json::Value::String(project.to_owned()));
            serde_json::from_value(serde_json::Value::Object(with_project))
                .unwrap_or_else(|_| empty_correlation(project))
        }
        _ => empty_correlation(project),
    };
    c.project = project.to_owned();
    c.run_id = Some(run_id.to_owned());
    c
}

fn to_run(r: mcpd_runs::Run) -> ApiResult<wire::Run> {
    let ended_at = r.status.is_terminal().then_some(r.updated_at);
    Ok(wire::Run {
        correlation: correlation_of(&r.correlation, &r.project, &r.run_id),
        cursor: wire::RunCursor {
            index: u32::try_from(r.cursor.index).unwrap_or(u32::MAX),
            construct: parse_wire(r.cursor.at.kind().as_str())?,
            step_key: r.cursor.at.step().map(str::to_owned),
        },
        status: parse_wire(r.status.as_str())?,
        run_id: r.run_id,
        project: r.project,
        definition_id: r.definition_id,
        definition_version: r.definition_version,
        input: r.input,
        output: r.output,
        state: Some(r.state),
        error: r.error,
        trace_id: r.trace_id,
        steps_used: u32::try_from(r.steps_used).unwrap_or(u32::MAX),
        max_steps: r.max_steps.and_then(|n| u32::try_from(n).ok()),
        cost_usd: r.cost_usd,
        max_cost_usd: r.max_cost_usd,
        deadline_at: r.deadline_at,
        idempotency_key: r.idempotency_key,
        root_run_id: r.root_run_id,
        parent_run_id: r.parent_run_id,
        parent_step_key: r.parent_step_key,
        depth: u32::try_from(r.depth).unwrap_or(u32::MAX),
        created_at: r.created_at,
        updated_at: r.updated_at,
        ended_at,
    })
}

fn to_attempt(a: Attempt) -> ApiResult<wire::StepAttempt> {
    let class = match a.error_class {
        Some(c) => parse_wire(c.as_str())?,
        None => wire::StepErrorClass::Fatal,
    };
    Ok(wire::StepAttempt {
        kind: parse_wire(a.kind.as_str())?,
        effect: parse_wire(a.effect.as_str())?,
        status: parse_wire(a.status.as_str())?,
        error: a.error.map(|message| wire::StepError {
            class,
            message,
            details: a.error_details,
            retry_after_ms: None,
        }),
        usage: a.usage.and_then(|u| serde_json::from_value(u).ok()),
        run_id: a.run_id,
        step_key: a.step_key,
        attempt: u32::try_from(a.attempt).unwrap_or(u32::MAX),
        input: a.input,
        output: a.output,
        cost_usd: a.cost_usd,
        span_id: a.span_id,
        lease_owner: a.lease_owner,
        lease_until: a.lease_until,
        started_at: a.started_at,
        orphaned_at: a.orphaned_at,
        reclaim_until: a.reclaim_until,
        attempts_left: None,
        ended_at: a.ended_at,
        created_at: a.created_at,
    })
}

fn to_timer(t: Timer) -> ApiResult<wire::RunTimer> {
    Ok(wire::RunTimer {
        run_id: t.run_id,
        step_key: (!t.step_key.is_empty()).then_some(t.step_key),
        kind: parse_wire(t.kind.as_str())?,
        due_at: t.due_at,
    })
}

fn to_wait(w: Wait) -> wire::RunWait {
    wire::RunWait {
        run_id: w.run_id,
        step_key: w.step_key,
        event: w.event,
        key: w.correlation_key,
        expires_at: w.expires_at,
    }
}

fn to_event(e: mcpd_runs::RunEvent) -> ApiResult<wire::RunEvent> {
    Ok(wire::RunEvent {
        run_id: e.run_id,
        seq: u64::try_from(e.seq).unwrap_or(0),
        kind: parse_wire(e.kind.as_str())?,
        payload: Some(e.payload),
        at: e.at,
    })
}

fn to_definition(d: Definition) -> ApiResult<wire::Definition> {
    Ok(wire::Definition {
        project: d.project,
        id: d.id,
        version: d.version,
        kind: parse_wire(d.kind.as_str())?,
        body: d.body,
        created_at: d.created_at,
    })
}

fn to_lease(l: Lease) -> ApiResult<wire::StepLease> {
    Ok(wire::StepLease {
        correlation: correlation_of(&l.correlation, &l.project, &l.run_id),
        kind: parse_wire(l.kind.as_str())?,
        effect: parse_wire(l.effect.as_str())?,
        lease_id: l.lease_id,
        run_id: l.run_id,
        project: l.project,
        definition_id: l.definition_id,
        definition_version: l.definition_version,
        step_key: l.step_key,
        attempt: u32::try_from(l.attempt).unwrap_or(u32::MAX),
        expires_at: l.expires_at,
        deadline_ms: l.deadline_ms.and_then(|ms| u64::try_from(ms).ok()),
        idempotency_key: l.idempotency_key,
        input: (!l.input.is_null()).then_some(l.input),
        prior: l.prior,
        state: Some(l.state),
        trace_id: l.trace_id,
        span_id: l.span_id,
        parent_span_id: l.parent_span_id,
    })
}

fn definition_document(project: &str, doc: wire::DefinitionDocument) -> DefinitionDocument {
    DefinitionDocument {
        project: project.to_owned(),
        id: doc.id,
        kind: mcpd_runs::DefinitionKind::Workflow,
        version: doc.version,
        body: doc.body,
    }
}

/// Price the usage a worker reported, with the daemon's own book, so a step's
/// cost is computed the same way an SDK span's is.
fn price(state: &AppState, usage: &wire::StepUsage) -> Option<f64> {
    let model = usage.model.as_deref()?;
    let tokens = wire::TokenUsage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        total_tokens: usage.total_tokens,
        input_details: usage.input_details.clone(),
        output_details: usage.output_details.clone(),
    };
    mcpd_core::cost::compute_with_provider(
        &state.price_book,
        model,
        usage.provider.as_deref(),
        &tokens,
    )
    .and_then(|c| c.total_cost)
}

// ------------------------------------------------------------ definitions ---

/// Query parameters shared by the definition and run routes.
#[derive(Debug, Default, Deserialize)]
pub struct ProjectQuery {
    /// Which project. A selector, required only when the key reaches several.
    pub project: Option<String>,
    /// One definition's versions, or one version of it.
    pub id: Option<String>,
    /// Which version.
    pub version: Option<u32>,
}

/// `POST /v1/api/definitions`: store one version of a definition.
pub async fn put_definition(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<ProjectQuery>,
    Json(doc): Json<wire::DefinitionDocument>,
) -> ApiResult<Json<wire::DefinitionVersion>> {
    let grant = authorize(&state, &headers, Scope::Write)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    let store = runs(&state)?;
    let id = doc.id.clone();
    let (version, assigned) =
        store.assign_definition(definition_document(&project, doc)).await.map_err(map_err)?;
    Ok(Json(wire::DefinitionVersion { id, version, assigned }))
}

/// `GET /v1/api/definitions`: every stored version in a project.
pub async fn list_definitions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<ProjectQuery>,
) -> ApiResult<Json<Vec<wire::Definition>>> {
    let grant = authorize(&state, &headers, Scope::Read)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    let store = runs(&state)?;
    let mut all = store.list_definitions(&project).await.map_err(map_err)?;
    if let Some(id) = q.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        all.retain(|d| d.id == id);
    }
    // Highest version first within an id, which is the one a versionless start pins.
    all.sort_by(|a, b| a.id.cmp(&b.id).then(b.version.cmp(&a.version)));
    Ok(Json(all.into_iter().map(to_definition).collect::<ApiResult<Vec<_>>>()?))
}

/// `GET /v1/api/definitions/{id}`: one definition, the latest version unless named.
pub async fn get_definition(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ProjectQuery>,
) -> ApiResult<Json<wire::Definition>> {
    let grant = authorize(&state, &headers, Scope::Read)?;
    let project = select_project(&state, &grant, q.project.as_deref())?;
    let store = runs(&state)?;
    let def = match q.version {
        Some(v) => store.get_definition(&project, &id, v).await,
        None => store.latest_definition(&project, &id).await,
    }
    .map_err(map_err)?;
    Ok(Json(to_definition(def)?))
}

// ------------------------------------------------------------------- runs ---

/// `POST /v1/api/runs`: start a run.
pub async fn start_run(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<wire::StartRunRequest>,
) -> ApiResult<Json<wire::Run>> {
    let grant = authorize(&state, &headers, Scope::Execute)?;
    let project = select_project(&state, &grant, req.project.as_deref())?;
    let store = runs(&state)?;
    // `project` and `run_id` inside the correlation are the daemon's to set.
    let correlation = req.correlation.map(|c| {
        let mut v = serde_json::to_value(c).unwrap_or_default();
        if let serde_json::Value::Object(map) = &mut v {
            map.remove("project");
            map.remove("run_id");
        }
        v
    });
    let run = store
        .start_run(
            StartRun {
                project,
                definition_id: req.definition_id,
                definition_version: req.version,
                input: req.input,
                correlation,
                idempotency_key: req.idempotency_key.filter(|k| !k.trim().is_empty()),
            },
            OffsetDateTime::now_utc(),
        )
        .await
        .map_err(map_err)?;
    state.metrics.runs_started.fetch_add(1, Ordering::Relaxed);
    Ok(Json(to_run(run)?))
}

/// Query parameters for the run list.
#[derive(Debug, Default, Deserialize)]
pub struct RunListQuery {
    /// Which project.
    pub project: Option<String>,
    /// Only runs in this status.
    pub status: Option<String>,
    /// Only runs of this definition.
    pub definition_id: Option<String>,
    /// Page size, 1 to 500.
    pub limit: Option<u32>,
    /// The cursor a previous page handed back.
    pub cursor: Option<String>,
}

/// `GET /v1/api/runs`: runs in a project, most recently updated first.
pub async fn list_runs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Query(q): Query<RunListQuery>,
) -> ApiResult<Json<wire::RunListResponse>> {
    // Audited, unlike the trace list: every row here carries the run's input,
    // output and state as they were sent. The decision recorded is the last
    // one, whether the key reaches the project the request selects, and a
    // refusal is filed under the project that was asked for.
    let asked = q
        .project
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map_or_else(|| state.config.default_project.clone(), str::to_owned);
    let (decision, project) = match authorize(&state, &headers, Scope::Read) {
        Err(e) => (Err(e), asked),
        Ok(grant) => match select_project(&state, &grant, q.project.as_deref()) {
            Ok(project) => (Ok(grant), project),
            Err(e) => (Err(e), asked),
        },
    };
    // The status and the cursor are read before the row, so a request answered
    // with 400 is not recorded as a read that was served.
    let (status, cursor) = crate::query::audited_valid(
        &state,
        &headers,
        peer.0,
        decision,
        || {
            let status = match q.status.as_deref().filter(|s| !s.is_empty()) {
                None => None,
                Some(s) => {
                    Some(mcpd_runs::RunStatus::parse(s).ok_or_else(|| ApiError::BadParam {
                        field: "status".to_owned(),
                        message: format!(
                            "{s:?} is not a run status; expected one of: {}",
                            mcpd_runs::RunStatus::ALL
                                .iter()
                                .map(|s| s.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    })?)
                }
            };
            Ok((status, run_cursor(q.cursor.as_deref())?))
        },
        "list_runs",
        &project,
        q.definition_id.as_deref().filter(|d| !d.is_empty()),
    )
    .await?;
    let store = runs(&state)?;
    let limit = usize::try_from(q.limit.unwrap_or(50).clamp(1, 500)).unwrap_or(50);
    let page = store
        .list_runs(
            RunFilter { project, status, definition_id: q.definition_id.filter(|d| !d.is_empty()) },
            limit,
            cursor,
        )
        .await
        .map_err(map_err)?;
    Ok(Json(wire::RunListResponse {
        runs: page.runs.into_iter().map(to_run).collect::<ApiResult<Vec<_>>>()?,
        page: wire::Page {
            has_more: page.next_cursor.is_some(),
            next_cursor: page.next_cursor,
            total: None,
        },
    }))
}

/// The run list's `cursor`, refused here when the run store could not have
/// written it.
///
/// The store writes `nanoseconds:run_id` and refuses anything else with a 400,
/// which came after the audit row. The test is the store's own, and so is the
/// refusal, so the answer a caller gets is the one it got before.
fn run_cursor(raw: Option<&str>) -> ApiResult<Option<String>> {
    let Some(c) = raw.filter(|c| !c.is_empty()) else { return Ok(None) };
    match c.split_once(':') {
        Some((at, _)) if at.parse::<i64>().is_ok() => Ok(Some(c.to_owned())),
        _ => Err(map_err(mcpd_runs::Error::Invalid(format!(
            "cursor \"{c}\" is not one this store issued"
        )))),
    }
}

/// Load a run and check the key reaches its project. Worded so a missing run
/// and an unreachable one read the same, which keeps run ids from being an
/// oracle for another project's activity.
async fn reachable_run(
    state: &AppState,
    grant: &Grant,
    store: &dyn RunStore,
    run_id: &str,
) -> ApiResult<mcpd_runs::Run> {
    let _ = state;
    let run = store.get_run(run_id).await.map_err(map_err)?;
    reach(grant, &run.project)?;
    Ok(run)
}

/// Authorise a request about one run and record it in the audit log, whichever
/// way it went.
///
/// The run store keeps a run's input, output, state and every step's input and
/// output as they were sent, unredacted, so handing a run back out is a read
/// of payload by the same rule a trace is. The row records the last decision
/// made, which is whether the key reaches the run's project, and files it
/// under that project, so the team that owns the run sees a refused attempt on
/// it.
///
/// A run that does not exist, asked for with a key that is accepted, is a 404
/// with no row: nothing was handed out and nothing was refused. Asked for with
/// a key that is refused, it is the refusal, recorded under the default
/// project. There is no run to file it under, and a refusal is the row an
/// audit log exists for; leaving it out let a key probe run ids unrecorded.
///
/// `validate` reads the request's own parameters. It runs after both
/// decisions and before the row, only when the key reached the run, so a
/// request answered with 400 leaves no row saying it was served, and a key
/// that is refused learns nothing about what the endpoint accepts. What it
/// returns comes back beside the run.
///
/// The scope decision is made by the caller and passed in, so each handler
/// names the scope it requires in its own body, where a reader and the
/// consistency check both look for it.
#[allow(clippy::too_many_arguments)]
async fn audited_run<T>(
    state: &AppState,
    headers: &HeaderMap,
    peer: Option<std::net::SocketAddr>,
    decision: ApiResult<Grant>,
    validate: impl FnOnce() -> ApiResult<T>,
    action: &str,
    store: &dyn RunStore,
    run_id: &str,
) -> ApiResult<(mcpd_runs::Run, T)> {
    let run = match store.get_run(run_id).await {
        Ok(run) => run,
        Err(e) => {
            // A refused key still hears about its key first, as it always did.
            if decision.is_err() {
                let project = &state.config.default_project;
                crate::query::audited(
                    state,
                    headers,
                    peer,
                    decision,
                    action,
                    project,
                    Some(run_id),
                )
                .await?;
            }
            return Err(map_err(e));
        }
    };
    let decision = decision.and_then(|grant| reach(&grant, &run.project).map(|()| grant));
    let valid = crate::query::audited_valid(
        state,
        headers,
        peer,
        decision,
        validate,
        action,
        &run.project,
        Some(run_id),
    )
    .await?;
    Ok((run, valid))
}

/// `GET /v1/api/runs/{run_id}`: a run and its whole durable state.
pub async fn get_run(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(run_id): Path<String>,
) -> ApiResult<Json<wire::RunDetail>> {
    let store = runs(&state)?;
    let (run, ()) = audited_run(
        &state,
        &headers,
        peer.0,
        authorize(&state, &headers, Scope::Read),
        || Ok(()),
        "get_run",
        store.as_ref(),
        &run_id,
    )
    .await?;
    let attempts = store.attempts(&run_id).await.map_err(map_err)?;
    let timers = store.timers(&run_id).await.map_err(map_err)?;
    let waits = store.waits(&run_id).await.map_err(map_err)?;
    Ok(Json(wire::RunDetail {
        run: to_run(run)?,
        attempts: attempts.into_iter().map(to_attempt).collect::<ApiResult<Vec<_>>>()?,
        timers: timers.into_iter().map(to_timer).collect::<ApiResult<Vec<_>>>()?,
        waits: waits.into_iter().map(to_wait).collect(),
    }))
}

/// `POST /v1/api/runs/{run_id}/cancel`: end a run tree.
pub async fn cancel_run(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(run_id): Path<String>,
) -> ApiResult<Json<wire::Run>> {
    let store = runs(&state)?;
    // Audited: it ends a run tree, and it answers with the run's input,
    // output and state.
    audited_run(
        &state,
        &headers,
        peer.0,
        authorize(&state, &headers, Scope::Execute),
        || Ok(()),
        "cancel_run",
        store.as_ref(),
        &run_id,
    )
    .await?;
    let run = store.cancel(&run_id, OffsetDateTime::now_utc()).await.map_err(map_err)?;
    Ok(Json(to_run(run)?))
}

/// Query parameters for the event log.
#[derive(Debug, Default, Deserialize)]
pub struct EventQuery {
    /// Read rows after this sequence number. `-1` is the first row.
    pub after: Option<i64>,
    /// Page size, 1 to 1000.
    pub limit: Option<u32>,
}

/// `GET /v1/api/runs/{run_id}/events`: the journal as a page, or as a stream.
///
/// With `Accept: text/event-stream` the same route stays open: it sends every
/// row after `after`, then waits for the store to signal a change and sends
/// what arrived, until the run ends. `after` is the reattach point, so a
/// client that drops reconnects at the last `id` it saw and misses nothing
/// that was committed.
pub async fn list_run_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(run_id): Path<String>,
    Query(q): Query<EventQuery>,
) -> ApiResult<Response> {
    let store = runs(&state)?;
    // Audited once per request, a stream included: an event carries a step's
    // error message and an event's correlation key.
    audited_run(
        &state,
        &headers,
        peer.0,
        authorize(&state, &headers, Scope::Read),
        || Ok(()),
        "list_run_events",
        store.as_ref(),
        &run_id,
    )
    .await?;
    let after = q.after.unwrap_or(-1);
    let limit = usize::try_from(q.limit.unwrap_or(500)).unwrap_or(500).clamp(1, MAX_EVENT_PAGE);

    let wants_stream = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/event-stream"));
    if !wants_stream {
        let page = store.events(&run_id, after, limit).await.map_err(map_err)?;
        return Ok(Json(wire::RunEventPage {
            events: page.events.into_iter().map(to_event).collect::<ApiResult<Vec<_>>>()?,
            has_more: page.has_more,
        })
        .into_response());
    }

    let notify = store.notify();
    let shutdown = state.shutdown.clone();
    let stream = futures::stream::unfold((after, false), move |(after, done): (i64, bool)| {
        let store = Arc::clone(&store);
        let notify = Arc::clone(&notify);
        let shutdown = shutdown.clone();
        let run_id = run_id.clone();
        async move {
            if done {
                return None;
            }
            loop {
                // Registered before the read, so a commit between the read
                // and the wait wakes this waiter rather than being missed.
                let notified = notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                let page = match store.events(&run_id, after, MAX_EVENT_PAGE).await {
                    Ok(page) => page,
                    Err(_) => return None,
                };
                if let Some(last) = page.events.last() {
                    let next = last.seq;
                    let ended =
                        page.events.iter().any(|e| e.kind == mcpd_runs::RunEventKind::RunEnded);
                    let frames: Vec<Result<Event, std::convert::Infallible>> = page
                        .events
                        .into_iter()
                        .filter_map(|e| to_event(e).ok())
                        .map(|row| {
                            let id = row.seq.to_string();
                            let name = row.kind.as_str().to_owned();
                            let data = serde_json::to_string(&row).unwrap_or_default();
                            Ok(Event::default().id(id).event(name).data(data))
                        })
                        .collect();
                    return Some((frames, (next, ended)));
                }
                // Nothing new. A run that has ended will write nothing more,
                // so the stream closes rather than holding the connection.
                match store.get_run(&run_id).await {
                    Ok(run) if !run.status.is_terminal() => {}
                    _ => return None,
                }
                tokio::select! {
                    () = shutdown.cancelled() => return None,
                    _ = tokio::time::timeout(Duration::from_secs(5), notified) => {}
                }
            }
        }
    })
    .flat_map(futures::stream::iter);
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()).into_response())
}

/// Query parameters for the cause walk.
#[derive(Debug, Default, Deserialize)]
pub struct CauseQuery {
    /// The step key to explain. A query parameter and not a path segment
    /// because a key may hold a slash.
    pub step: Option<String>,
    /// How far back to walk.
    pub max_depth: Option<u32>,
}

/// A crate cause link, respelled as the generated wire type.
fn to_cause_link(l: mcpd_runs::CauseLink) -> ApiResult<wire::CauseLink> {
    Ok(wire::CauseLink {
        step_key: l.step_key,
        status: parse_wire(l.status.as_str())?,
        attempts: l.attempts,
        template_path: l.template_path,
        consumed_by: l.consumed_by,
        depth: l.depth,
        silent_producer: l.silent_producer,
        value_kind: match l.value_kind {
            Some(kind) => Some(parse_wire(kind.as_str())?),
            None => None,
        },
        value_empty: l.value_empty,
    })
}

fn to_cause(c: mcpd_runs::StepCause) -> ApiResult<wire::StepCause> {
    Ok(wire::StepCause {
        run_id: c.run_id,
        step_key: c.step_key,
        status: parse_wire(c.status.as_str())?,
        attempts: c.attempts,
        error: match c.error {
            Some(e) => Some(wire::StepError {
                class: parse_wire(e.class.as_str())?,
                message: e.message,
                details: e.details,
                retry_after_ms: e.retry_after_ms.and_then(|ms| u64::try_from(ms).ok()),
            }),
            None => None,
        },
        chain: c.chain.into_iter().map(to_cause_link).collect::<ApiResult<Vec<_>>>()?,
        truncated: c.truncated,
    })
}

/// `GET /v1/api/runs/{run_id}/cause`: why one step of a run failed.
///
/// The answer is walked out of the run's own journal and the definition
/// version it pinned at start. Nothing here reads a prompt, a completion or a
/// model, so it answers the same with span payload capture off.
pub async fn get_step_cause(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(run_id): Path<String>,
    Query(q): Query<CauseQuery>,
) -> ApiResult<Json<wire::StepCause>> {
    let store = runs(&state)?;
    // Audited: the answer carries the failing step's error message and the
    // structured details the worker stored with it.
    let (_, step) = audited_run(
        &state,
        &headers,
        peer.0,
        authorize(&state, &headers, Scope::Read),
        || {
            q.step.as_deref().map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| {
                ApiError::BadParam {
                    field: "step".to_owned(),
                    message: "name the step to explain, as the journal keys it, such as \
                              step=refund/issue"
                        .to_owned(),
                }
            })
        },
        "get_step_cause",
        store.as_ref(),
        &run_id,
    )
    .await?;
    let max_depth = q.max_depth.unwrap_or(MAX_CAUSE_DEPTH).min(MAX_CAUSE_DEPTH);
    let cause = store.step_cause(&run_id, step, max_depth).await.map_err(map_err)?;
    Ok(Json(to_cause(cause)?))
}

/// `POST /v1/api/runs/{run_id}/signal`: deliver an event to one run.
pub async fn signal_run(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(req): Json<wire::SignalRequest>,
) -> ApiResult<Json<wire::EventReceipt>> {
    let grant = authorize(&state, &headers, Scope::Execute)?;
    let store = runs(&state)?;
    reachable_run(&state, &grant, store.as_ref(), &run_id).await?;
    let delivery = store
        .signal(
            &run_id,
            &req.event,
            req.payload,
            req.event_id.filter(|id| !id.trim().is_empty()),
            OffsetDateTime::now_utc(),
        )
        .await
        .map_err(map_err)?;
    Ok(Json(wire::EventReceipt {
        outcome: parse_wire(delivery.outcome.as_str())?,
        event_id: delivery.event_id,
    }))
}

/// `POST /v1/api/events`: route an event by name and key.
pub async fn publish_event(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<wire::PublishEventRequest>,
) -> ApiResult<Json<wire::EventReceipt>> {
    let grant = authorize(&state, &headers, Scope::Execute)?;
    let project = select_project(&state, &grant, req.project.as_deref())?;
    let store = runs(&state)?;
    if req.event.trim().is_empty() {
        return Err(ApiError::BadParam {
            field: "event".to_owned(),
            message: "an event needs a name".to_owned(),
        });
    }
    let (event_id, id_is_derived) = match req.event_id.filter(|id| !id.trim().is_empty()) {
        Some(id) => (id, false),
        None => {
            (InboundEvent::derived_id(&project, &req.event, &req.key, req.payload.as_ref()), true)
        }
    };
    let delivery = store
        .deliver_event(
            InboundEvent {
                project,
                event: req.event,
                correlation_key: req.key,
                event_id,
                id_is_derived,
                payload: req.payload,
            },
            OffsetDateTime::now_utc(),
        )
        .await
        .map_err(map_err)?;
    Ok(Json(wire::EventReceipt {
        outcome: parse_wire(delivery.outcome.as_str())?,
        event_id: delivery.event_id,
    }))
}

// ---------------------------------------------------------------- workers ---

/// `POST /v1/api/workers/register`: store what a worker serves.
pub async fn register_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<wire::WorkerRegistration>,
) -> ApiResult<Json<wire::WorkerRegistered>> {
    let grant = authorize(&state, &headers, Scope::Worker)?;
    let project = select_project(&state, &grant, req.project.as_deref())?;
    let store = runs(&state)?;
    if req.worker_id.trim().is_empty() {
        return Err(ApiError::BadParam {
            field: "worker_id".to_owned(),
            message: "a worker needs a name; it is recorded as the owner of every lease it takes"
                .to_owned(),
        });
    }
    let mut registered = Vec::with_capacity(req.definitions.len());
    for doc in req.definitions {
        let id = doc.id.clone();
        let (version, assigned) =
            store.assign_definition(definition_document(&project, doc)).await.map_err(map_err)?;
        registered.push(wire::DefinitionVersion { id, version, assigned });
    }
    let ttl = state.config.run_config.lease_ttl.whole_milliseconds();
    let lease_ttl_ms = u64::try_from(ttl).unwrap_or(30_000);
    Ok(Json(wire::WorkerRegistered {
        worker_id: req.worker_id,
        registered,
        lease_ttl_ms,
        heartbeat_ms: (lease_ttl_ms / 3).max(1_000),
    }))
}

/// `POST /v1/api/steps/claim`: offer runnable steps, waiting up to `wait_ms`.
pub async fn claim_steps(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<wire::ClaimRequest>,
) -> ApiResult<Json<wire::ClaimResponse>> {
    let grant = authorize(&state, &headers, Scope::Worker)?;
    let project = select_project(&state, &grant, req.project.as_deref())?;
    let store = runs(&state)?;
    if req.worker_id.trim().is_empty() {
        return Err(ApiError::BadParam {
            field: "worker_id".to_owned(),
            message: "a claim needs the worker's name".to_owned(),
        });
    }
    if req.max == 0 || req.max > MAX_CLAIM {
        return Err(ApiError::BadParam {
            field: "max".to_owned(),
            message: format!("max must be between 1 and {MAX_CLAIM}, got {}", req.max),
        });
    }
    if req.wait_ms > MAX_WAIT_MS {
        return Err(ApiError::BadParam {
            field: "wait_ms".to_owned(),
            message: format!("wait_ms must be at most {MAX_WAIT_MS}, got {}", req.wait_ms),
        });
    }
    let run_scope = req.run_id.filter(|r| !r.trim().is_empty());
    let wait_ms = u64::from(req.wait_ms).min(claim_wait_cap(state.config.request_timeout_seconds));
    let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms);
    // The store's runnable signal, never its change signal: this loop's own
    // claim is a change when it leases something, and an empty claim used to
    // wake this very waiter, so an idle worker ran claim transactions back to
    // back for its whole wait.
    let notify = store.runnable();
    loop {
        // The waiter is registered before the read so that a run started
        // between the read and the wait wakes this claim rather than costing
        // it a whole poll interval.
        let notified = notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let leases = store
            .claim(
                &project,
                &req.worker_id,
                usize::try_from(req.max).unwrap_or(1),
                run_scope.as_deref(),
                OffsetDateTime::now_utc(),
            )
            .await
            .map_err(map_err)?;
        let now = tokio::time::Instant::now();
        if !leases.is_empty() || now >= deadline {
            state.metrics.steps_claimed.fetch_add(leases.len() as u64, Ordering::Relaxed);
            return Ok(Json(wire::ClaimResponse {
                leases: leases.into_iter().map(to_lease).collect::<ApiResult<Vec<_>>>()?,
            }));
        }
        // A missed wake-up costs one poll interval, never the whole wait.
        let slice = (deadline - now).min(Duration::from_millis(500));
        // Shutdown ends the wait at once with an empty answer, which the
        // worker reads as "reissue". A graceful stop waits for requests in
        // flight, and a claim that held out for its whole wait would hold the
        // stop up by as long.
        tokio::select! {
            () = state.shutdown.cancelled() => {
                return Ok(Json(wire::ClaimResponse { leases: Vec::new() }));
            }
            _ = tokio::time::timeout(slice, notified) => {}
        }
    }
}

/// The longest a claim is held open, in milliseconds: four fifths of the
/// request timeout, 24 s at the default 30 s.
///
/// `wait_ms` may be up to 60 s and the request timeout defaults to 30 s, so a
/// claim held for the whole of a long `wait_ms` was cut off by the timeout
/// layer with a 503 the worker had done nothing to earn. The fifth left over
/// covers the claim transaction after the wait and the reply, so the empty
/// answer arrives before the deadline and the worker reissues as the contract
/// tells it to.
pub(crate) fn claim_wait_cap(request_timeout_seconds: u64) -> u64 {
    request_timeout_seconds.max(1).saturating_mul(1000) / 5 * 4
}

/// The lease's run, checked against the key's reach before anything is
/// written. A worker key scoped to one project cannot touch another project's
/// leases by guessing ids.
async fn reachable_lease(grant: &Grant, store: &dyn RunStore, lease_id: &str) -> ApiResult<()> {
    let run = store.lease_run(lease_id).await.map_err(map_err)?;
    reach(grant, &run.project)
}

/// `POST /v1/api/steps/{lease_id}/started`: declare that a body is running.
pub async fn step_started(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(lease_id): Path<String>,
) -> ApiResult<Json<wire::LeaseState>> {
    let grant = authorize(&state, &headers, Scope::Worker)?;
    let store = runs(&state)?;
    reachable_lease(&grant, store.as_ref(), &lease_id).await?;
    let ack = store.started(&lease_id, OffsetDateTime::now_utc()).await.map_err(map_err)?;
    Ok(Json(wire::LeaseState { expires_at: ack.expires_at, reclaimed: ack.reclaimed }))
}

/// `POST /v1/api/steps/{lease_id}/heartbeat`: extend a lease.
pub async fn step_heartbeat(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(lease_id): Path<String>,
) -> ApiResult<Json<wire::LeaseState>> {
    let grant = authorize(&state, &headers, Scope::Worker)?;
    let store = runs(&state)?;
    reachable_lease(&grant, store.as_ref(), &lease_id).await?;
    let ack = store.heartbeat(&lease_id, OffsetDateTime::now_utc()).await.map_err(map_err)?;
    Ok(Json(wire::LeaseState { expires_at: ack.expires_at, reclaimed: ack.reclaimed }))
}

/// `POST /v1/api/steps/{lease_id}/complete`: finish a step.
pub async fn complete_step(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(lease_id): Path<String>,
    Json(req): Json<wire::StepResult>,
) -> ApiResult<Json<wire::StepAck>> {
    let grant = authorize(&state, &headers, Scope::Worker)?;
    let store = runs(&state)?;
    reachable_lease(&grant, store.as_ref(), &lease_id).await?;
    if req.state.as_ref().is_some_and(|s| !s.is_object()) {
        return Err(ApiError::BadParam {
            field: "state".to_owned(),
            message: "state must be a JSON object; it is merged into the run's state key by key"
                .to_owned(),
        });
    }
    let cost_usd = req.usage.as_ref().and_then(|u| price(&state, u));
    let usage = req.usage.as_ref().map(|u| serde_json::to_value(u).unwrap_or_default());
    let outcome = store
        .complete_step(
            &lease_id,
            StepCompletion { output: req.output, state: req.state, usage, cost_usd },
            OffsetDateTime::now_utc(),
        )
        .await
        .map_err(map_err)?;
    state.metrics.steps_completed.fetch_add(1, Ordering::Relaxed);
    Ok(Json(wire::StepAck {
        run_status: parse_wire(outcome.run_status.as_str())?,
        next: if outcome.next_runnable { "claim" } else { "idle" }.to_owned(),
        attempts_left: None,
    }))
}

/// `POST /v1/api/steps/{lease_id}/fail`: fail a step.
pub async fn fail_step(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(lease_id): Path<String>,
    Json(req): Json<wire::StepFailure>,
) -> ApiResult<Json<wire::StepAck>> {
    let grant = authorize(&state, &headers, Scope::Worker)?;
    let store = runs(&state)?;
    reachable_lease(&grant, store.as_ref(), &lease_id).await?;
    if req.error.message.trim().is_empty() {
        return Err(ApiError::BadParam {
            field: "error.message".to_owned(),
            message: "a failure needs a message; it is what the run's reader sees".to_owned(),
        });
    }
    // The worker's floor is added to the clock to write the retry timer, so it
    // is held to the same bound as every duration a definition may name.
    let retry_after_ms = match req.error.retry_after_ms.map(|ms| (ms, i64::try_from(ms))) {
        None => None,
        Some((_, Ok(ms))) if ms <= mcpd_runs::MAX_DURATION_MS => Some(ms),
        Some((ms, _)) => {
            return Err(ApiError::BadParam {
                field: "error.retry_after_ms".to_owned(),
                message: format!(
                    "error.retry_after_ms must be at most {} (3650 days), got {ms}; send the \
                     wait the failure named, or leave it out to use the step's backoff",
                    mcpd_runs::MAX_DURATION_MS
                ),
            })
        }
    };
    let class = mcpd_runs::StepErrorClass::parse(req.error.class.as_str())
        .unwrap_or(mcpd_runs::StepErrorClass::Fatal);
    let outcome = store
        .fail_step(
            &lease_id,
            StepError {
                class,
                message: req.error.message,
                details: req.error.details,
                retry_after_ms,
            },
            OffsetDateTime::now_utc(),
        )
        .await
        .map_err(map_err)?;
    state.metrics.steps_failed.fetch_add(1, Ordering::Relaxed);
    Ok(Json(wire::StepAck {
        run_status: parse_wire(outcome.run_status.as_str())?,
        next: if outcome.next_runnable { "claim" } else { "idle" }.to_owned(),
        attempts_left: Some(outcome.attempts_left),
    }))
}

// ------------------------------------------------------------ background ---

/// Start the tick and the outbox drain. Does nothing when the daemon serves no
/// runs, so a deployment that never turned the run store on pays nothing.
pub fn spawn(state: Arc<AppState>) {
    let Some(store) = state.runs.clone() else { return };

    // Deadlines every second, the reconciler every thirty. A reconciler pass
    // walks every stalled run and is not free; a deadline that fires a second
    // late is what a worker's own heartbeat interval already tolerates.
    //
    // Through `spawn_loop`, like every other loop the daemon owns, so a panic
    // in one pass is logged and the next pass still runs. Spawned bare, one
    // panic ended the task, and no deadline fired again while every health
    // check stayed green.
    {
        let store = Arc::clone(&store);
        let mut passes: u64 = 0;
        crate::spawn_loop(
            &state,
            "run deadline",
            Duration::from_secs(1),
            crate::FirstPass::AtStart,
            move || {
                passes += 1;
                let reconcile = passes.is_multiple_of(30);
                let store = Arc::clone(&store);
                async move {
                    let now = OffsetDateTime::now_utc();
                    if let Err(e) = store.run_due_timers(now).await {
                        tracing::error!(error = %e, "the run store's deadline tick failed");
                    }
                    if reconcile {
                        match store.reconcile(now).await {
                            Ok(report) if report.runs_repaired > 0 => tracing::warn!(
                                runs = report.runs_repaired,
                                rows = report.rows_repaired,
                                "stalled runs repaired: a cursor moved without writing what it \
                                 requires"
                            ),
                            Ok(_) => {}
                            Err(e) => {
                                tracing::error!(error = %e, "the run store's reconciler failed");
                            }
                        }
                    }
                }
            },
        );
    }

    // The drain wakes on every commit the store signals and at least once a
    // second, and stops between passes on shutdown, so a span enqueued by a
    // request that was answered is written before the process exits.
    //
    // Not `spawn_loop`, which runs on a timer alone and would lose the wake on
    // a commit. Each pass goes through `run_tick` instead, which is the half
    // of `spawn_loop` that survives a panic, for the reason given above.
    {
        let state = Arc::clone(&state);
        let notify = store.notify();
        let pass = |state: &Arc<AppState>| {
            let state = Arc::clone(state);
            crate::run_tick("run outbox drain", async move {
                drain_once(&state).await;
            })
        };
        state.tasks.clone().spawn(async move {
            loop {
                let notified = notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                pass(&state).await;
                tokio::select! {
                    () = state.shutdown.cancelled() => {
                        pass(&state).await;
                        return;
                    }
                    _ = tokio::time::timeout(Duration::from_secs(1), notified) => {}
                }
            }
        });
    }
}

/// Write one batch of outbox rows through the ordinary ingest path.
///
/// Every row is one span with its own ingest key, so a row written twice is
/// recognised by the signal store as a replay. A row the store refuses is
/// released for the next pass and counted; a row that cannot even be read as
/// a span is acknowledged, because no later pass could do better with it.
/// Returns how many rows were written.
pub async fn drain_once(state: &Arc<AppState>) -> usize {
    let Some(store) = state.runs.clone() else { return 0 };
    // From the claim to the acknowledgement. `AppState::outbox_gate` says why.
    let _turn = state.outbox_gate.lock().await;
    let rows = match store.claim_outbox(DRAIN_BATCH, OffsetDateTime::now_utc()).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!(error = %e, "the span outbox could not be read");
            return 0;
        }
    };
    let mut written = 0usize;
    let mut done = Vec::new();
    let mut back = Vec::new();
    for row in rows {
        let mut payload = row.payload;
        if let serde_json::Value::Object(map) = &mut payload {
            // The store writes the wire shape minus the one field only the
            // daemon can know: when it accepted the span.
            map.entry("received_at").or_insert_with(|| {
                serde_json::Value::String(
                    OffsetDateTime::now_utc()
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default(),
                )
            });
        }
        let span: wire::Span = match serde_json::from_value(payload) {
            Ok(span) => span,
            Err(e) => {
                tracing::error!(
                    outbox_id = %row.outbox_id, run_id = %row.run_id, error = %e,
                    "an outbox row is not a span and is dropped"
                );
                done.push(row.outbox_id);
                continue;
            }
        };
        let batch = mcpd_store::Batch { ingest_key: Some(row.ingest_key), ..Default::default() };
        match crate::ingest::finish(state, vec![span], batch, &Grant::All).await {
            Ok(_) => {
                written += 1;
                done.push(row.outbox_id);
            }
            Err(e) => {
                tracing::warn!(
                    outbox_id = %row.outbox_id, run_id = %row.run_id, error = %e,
                    "a run span could not be written and will be retried"
                );
                state.metrics.run_spans_deferred.fetch_add(1, Ordering::Relaxed);
                back.push(row.outbox_id);
            }
        }
    }
    if !done.is_empty() {
        if let Err(e) = store.ack_outbox(&done).await {
            tracing::error!(error = %e, "written outbox rows could not be acknowledged");
        }
    }
    if !back.is_empty() {
        if let Err(e) = store.release_outbox(&back).await {
            tracing::error!(error = %e, "deferred outbox rows could not be released");
        }
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two enum vocabularies are maintained in two crates: the run store
    /// spells its values from the contract's table and the spec generates the
    /// wire enums from the same table. This walks every value of every one
    /// through the respelling, so a value renamed on one side fails here.
    #[test]
    fn every_run_store_enum_value_is_a_wire_value() {
        fn round<W: FromStr>(values: &[&str], name: &str) {
            for v in values {
                assert!(W::from_str(v).is_ok(), "{name}: the wire does not know {v:?}");
            }
        }
        round::<wire::RunStatus>(&mcpd_runs::RunStatus::ALL.map(|v| v.as_str()), "RunStatus");
        round::<wire::AttemptStatus>(
            &mcpd_runs::AttemptStatus::ALL.map(|v| v.as_str()),
            "AttemptStatus",
        );
        round::<wire::StepKind>(&mcpd_runs::StepKind::ALL.map(|v| v.as_str()), "StepKind");
        round::<wire::Effect>(&mcpd_runs::Effect::ALL.map(|v| v.as_str()), "Effect");
        round::<wire::TimerKind>(&mcpd_runs::TimerKind::ALL.map(|v| v.as_str()), "TimerKind");
        round::<wire::RunEventKind>(
            &mcpd_runs::RunEventKind::ALL.map(|v| v.as_str()),
            "RunEventKind",
        );
        round::<wire::ConflictReason>(
            &mcpd_runs::ConflictReason::ALL.map(|v| v.as_str()),
            "ConflictReason",
        );
        round::<wire::StepErrorClass>(
            &mcpd_runs::StepErrorClass::ALL.map(|v| v.as_str()),
            "StepErrorClass",
        );
        round::<wire::EventOutcome>(
            &mcpd_runs::EventOutcome::ALL.map(|v| v.as_str()),
            "EventOutcome",
        );
        round::<wire::ConstructKind>(
            &mcpd_runs::ConstructKind::ALL.map(|v| v.as_str()),
            "ConstructKind",
        );
        round::<wire::DefinitionKind>(
            &mcpd_runs::DefinitionKind::ALL.map(|v| v.as_str()),
            "DefinitionKind",
        );
        // And the other direction: a wire value the store cannot write is a
        // value a client could send that nothing here understands.
        for v in wire::StepErrorClass::ALL {
            assert!(mcpd_runs::StepErrorClass::parse(v.as_str()).is_some(), "{v:?}");
        }
        for v in wire::RunStatus::ALL {
            assert!(mcpd_runs::RunStatus::parse(v.as_str()).is_some(), "{v:?}");
        }
    }

    #[test]
    fn a_claim_is_held_for_at_most_four_fifths_of_the_request_timeout() {
        assert_eq!(claim_wait_cap(30), 24_000, "the default");
        assert_eq!(claim_wait_cap(120), 96_000, "above the 60 s a claim may ask for");
        assert_eq!(claim_wait_cap(1), 800);
        assert_eq!(claim_wait_cap(0), 800, "the router treats a zero timeout as one second");
        assert!(claim_wait_cap(u64::MAX) > 0, "no overflow");
    }

    #[test]
    fn a_project_is_selected_from_the_grant_when_the_request_names_none() {
        let state = AppState::new(
            Arc::new(mcpd_store::sqlite::SqliteStore::open(":memory:", 1).expect("store")),
            crate::ServerConfig::default(),
        );
        assert_eq!(select_project(&state, &Grant::All, None).expect("default"), "default");
        assert_eq!(select_project(&state, &Grant::All, Some("shop")).expect("named"), "shop");
        let one = Grant::Only(std::collections::BTreeSet::from(["shop".to_owned()]));
        assert_eq!(select_project(&state, &one, None).expect("single"), "shop");
        assert!(select_project(&state, &one, Some("other")).is_err(), "not covered");
        let two = Grant::Only(std::collections::BTreeSet::from(["a".to_owned(), "b".to_owned()]));
        assert!(matches!(select_project(&state, &two, None), Err(ApiError::BadParam { .. })));
        assert_eq!(select_project(&state, &two, Some("b")).expect("named"), "b");
    }
}
