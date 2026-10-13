//! Query handlers.
//!
//! Every list endpoint takes the same shape of filter, returns the same shape
//! of page, and rejects an unparseable parameter by naming it. The UI and the
//! five SDKs all read these, so a vague error here costs five debugging
//! sessions rather than one.

use crate::auth::{authorize, authorize_project, Grant, Scope};
use crate::error::{ApiError, ApiResult};
use crate::pipeline::Intake;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use mcpd_core::{
    AnalyticsSummary, AuditEvent, AuditResult, CostBreakdown, DeleteResult, DeltaResponse,
    DeltaSignal, DiscoveredValue, DiscoveryResponse, EntityKind, ErasureResult, Facet, Feedback,
    FeedbackListResponse, HealthStatus, LogLevel, LogListResponse, MetricSeriesResponse, Page,
    Partitioning, Score, ScoreListResponse, ScoreSource, SpanKind, SpanListResponse, SpanStatus,
    StoreCapabilities, TextSearch, TraceDetail, TraceListResponse,
};
use mcpd_store::{
    Aggregation, AuditQuery, Batch, ErasureOutcome, Include, LogQuery, MetricQuery, Order, Paged,
    ScoreQuery, SpanQuery, SpanSort, TraceQuery, TraceSort,
};
use std::collections::HashMap;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::sync::Arc;
use time::OffsetDateTime;

/// Raw query string, parsed leniently so one bad parameter names itself.
type Params = HashMap<String, String>;

fn get<'a>(p: &'a Params, key: &str) -> Option<&'a str> {
    p.get(key).map(String::as_str).filter(|s| !s.is_empty())
}

fn parse<T: std::str::FromStr>(p: &Params, key: &str) -> ApiResult<Option<T>> {
    match get(p, key) {
        None => Ok(None),
        Some(raw) => raw.parse::<T>().map(Some).map_err(|_| ApiError::BadParam {
            field: key.to_owned(),
            message: format!("{key}={raw:?} could not be read as a {}", std::any::type_name::<T>()),
        }),
    }
}

/// How far back a relative time such as `-7d` may reach, in days: about a
/// hundred years, further than any retention window this daemon accepts.
const RELATIVE_DAYS_MAX: i64 = 36_500;

/// Parse an RFC 3339 timestamp, or a relative shorthand like `-1h`.
///
/// The shorthand exists because typing an ISO timestamp by hand to look at the
/// last hour is friction nobody should pay.
fn parse_time(p: &Params, key: &str) -> ApiResult<Option<OffsetDateTime>> {
    let Some(raw) = get(p, key) else { return Ok(None) };
    if let Some(rest) = raw.strip_prefix('-') {
        // Split on the last CHARACTER, not the last byte. `str::split_at` asserts a
        // char boundary, so `?from=-1é` panicked the handler instead of returning
        // the 400 this function exists to return.
        let unit_ch = rest.chars().next_back();
        let (n, unit) = rest.split_at(rest.len() - unit_ch.map_or(0, char::len_utf8));
        let n: i64 = n.parse().map_err(|_| ApiError::BadParam {
            field: key.to_owned(),
            message: format!("{raw:?} is not a duration; try -15m, -1h, -7d"),
        })?;
        let per_unit: i64 = match unit {
            "s" => 1,
            "m" => 60,
            "h" => 3_600,
            "d" => 86_400,
            _ => {
                return Err(ApiError::BadParam {
                    field: key.to_owned(),
                    message: format!("{raw:?} has an unknown unit; use s, m, h or d"),
                })
            }
        };
        // Checked, every step of it. `Duration::days` panics when the count
        // overflows and subtracting a duration past the calendar's range
        // panics too, so `?from=-9999999d` stopped the daemon from a
        // read-only request. The bound is stated rather than left at
        // whatever the date type can hold, so the refusal can name it.
        let too_far = || ApiError::BadParam {
            field: key.to_owned(),
            message: format!(
                "{key}={raw:?} is further from now than this server reads; a relative time \
                 goes back at most {RELATIVE_DAYS_MAX} days, so use a value from -1s to \
                 -{RELATIVE_DAYS_MAX}d, or an RFC 3339 timestamp"
            ),
        };
        let seconds = n
            .checked_mul(per_unit)
            .filter(|s| s.unsigned_abs() <= RELATIVE_DAYS_MAX.unsigned_abs() * 86_400)
            .ok_or_else(too_far)?;
        return OffsetDateTime::now_utc()
            .checked_sub(time::Duration::seconds(seconds))
            .map(Some)
            .ok_or_else(too_far);
    }
    OffsetDateTime::parse(raw, &time::format_description::well_known::Rfc3339).map(Some).map_err(
        |_| ApiError::BadParam {
            field: key.to_owned(),
            message: format!(
                "{raw:?} is not an RFC 3339 timestamp or a relative duration like -1h"
            ),
        },
    )
}

fn parse_enum<T: std::str::FromStr>(p: &Params, key: &str, allowed: &str) -> ApiResult<Option<T>> {
    match get(p, key) {
        None => Ok(None),
        Some(raw) => raw.parse::<T>().map(Some).map_err(|_| ApiError::BadParam {
            field: key.to_owned(),
            message: format!("{raw:?} is not a valid {key}; expected one of: {allowed}"),
        }),
    }
}

/// The project a read means: the one named, else an account's own project,
/// else the daemon's default. An account reaches one project, so a read that
/// names none means that one; the grant is still checked afterwards.
fn project(p: &Params, state: &AppState, headers: &HeaderMap) -> String {
    match get(p, "project") {
        Some(named) => named.to_owned(),
        None => crate::auth::pinned_project(state, headers)
            .unwrap_or_else(|| state.config.default_project.clone()),
    }
}

fn order(p: &Params) -> Order {
    match get(p, "order") {
        Some("asc") => Order::Asc,
        _ => Order::Desc,
    }
}

fn page_of<T>(paged: &Paged<T>) -> Page {
    Page {
        next_cursor: paged.next_cursor.clone(),
        has_more: paged.next_cursor.is_some(),
        total: paged.total,
    }
}

/// Every parameter `GET /v1/api/traces` accepts.
///
/// Public because it is one half of a promise the spec makes: `list_traces`
/// declares a query vocabulary, and this is the vocabulary the door enforces.
/// `tests/vocabulary.rs` compares the two, so a parameter documented and not
/// implemented, or implemented and not documented, fails the build rather than
/// reaching a caller.
pub const TRACE_PARAMS: &[&str] = &[
    "project",
    "q",
    "status",
    "kind",
    "name",
    "tags",
    "session_id",
    "thread_id",
    "user_id",
    "environment",
    "model",
    "from",
    "to",
    "min_duration_ms",
    "min_cost",
    "has_score",
    "score_below",
    "sort",
    "order",
    "limit",
    "cursor",
];

/// Every parameter `GET /v1/api/spans` accepts.
pub const SPAN_PARAMS: &[&str] = &[
    "project",
    "q",
    "kind",
    "status",
    "name",
    "model",
    "trace_id",
    "from",
    "to",
    "min_duration_ms",
    "sort",
    "order",
    "limit",
    "cursor",
];

/// Every parameter `GET /v1/api/logs` accepts.
pub const LOG_PARAMS: &[&str] =
    &["project", "q", "level", "trace_id", "span_id", "from", "to", "order", "limit", "cursor"];

/// Every parameter `GET /v1/api/scores` accepts.
pub const SCORE_PARAMS: &[&str] = &[
    "project",
    "scorer_id",
    "source",
    "trace_id",
    "experiment_id",
    "min_value",
    "max_value",
    "from",
    "to",
    "limit",
    "cursor",
];

/// Every parameter `GET /v1/api/metrics` accepts.
pub const METRIC_PARAMS: &[&str] =
    &["project", "name", "from", "to", "bucket", "agg", "group_by", "filter"];

/// Every parameter `GET /v1/api/feedback` accepts.
pub const FEEDBACK_PARAMS: &[&str] = &["project", "trace_id", "limit"];

/// Values mcpd records on every trace and cannot yet filter a list by.
///
/// Kept apart from an unknown parameter because the answer a caller needs is a
/// different one. `service_name=api` is not a typo: the value is on the row,
/// `GET /v1/api/discovery` offers it as a filter dimension, and there is every
/// reason to expect the filter to apply. Answering "unknown parameter" would
/// send somebody hunting a spelling mistake that is not there.
///
/// Each entry is a correlation field carried on the stored trace projection.
/// The gap is the query, not the data, which is why the message says where the
/// value can still be seen.
const TRACE_RECORDED_NOT_FILTERED: &[&str] = &[
    "service_name",
    "service_version",
    "resource_id",
    "run_id",
    "request_id",
    "experiment_id",
    "release",
];

/// Refuse a parameter this endpoint does not apply, by name.
///
/// The rule this enforces is that a door either applies a parameter or refuses
/// it. mcpd read every list's query string with a lenient map, so
/// `?service_name=checkout-api` was accepted, dropped and answered with the
/// whole project: a plausible wrong answer, handed back with a 200, to somebody
/// who believed they had filtered. That is worse than an error, because there
/// is nothing about the response to notice.
///
/// Offenders are sorted before one is reported. A map has no order, so without
/// that a request with two unknown parameters names a different one on each
/// call and the same mistake produces two different error messages.
fn only(
    p: &Params,
    endpoint: &str,
    accepted: &[&str],
    recorded_not_filtered: &[&str],
) -> ApiResult<()> {
    let mut unknown: Vec<&str> =
        p.keys().map(String::as_str).filter(|k| !accepted.contains(k)).collect();
    if unknown.is_empty() {
        return Ok(());
    }
    unknown.sort_unstable();
    let field = unknown[0];
    // Named, but not explained: the reasons differ between them, and one
    // refusal that tried to give every reason would be unreadable. A caller
    // fixing them one at a time gets the right reason for each.
    let also = if unknown.len() > 1 {
        format!(" Also refused, for reasons of their own: {}.", unknown[1..].join(", "))
    } else {
        String::new()
    };
    let message = if recorded_not_filtered.contains(&field) {
        format!(
            "{endpoint} does not filter on {field:?}. mcpd records it on every trace, and \
             GET /v1/api/discovery lists the values it takes where that dimension is one it \
             offers, but no filter is applied to it here. Accepting it and ignoring it would \
             answer a different question from the one asked.{also}"
        )
    } else {
        format!(
            "{field:?} is not a parameter of {endpoint}. It takes: {}. A parameter that was \
             accepted and then ignored would answer a different question from the one asked, \
             so it is refused instead.{also}",
            accepted.join(", ")
        )
    };
    Err(ApiError::BadParam { field: field.to_owned(), message })
}

fn kind_names() -> String {
    SpanKind::ALL.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", ")
}

/// The address the request came from, when the server knows it.
///
/// `main.rs` serves the router with `into_make_service_with_connect_info`, which
/// stores the peer address as a `ConnectInfo` extension; the tests drive the
/// same router in-process with `oneshot`, where there is no connection and so
/// no address. axum 0.8 no longer lets a handler take `Option<ConnectInfo<_>>`
/// (an optional extractor has to define what "absent" means), so this reads the
/// extension itself and makes the in-process case a `None` rather than a 500.
/// The audit trail records the address when it is there and nothing otherwise,
/// which is the same rule the handlers applied before.
pub struct Peer(pub Option<std::net::SocketAddr>);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for Peer {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        Ok(Peer(
            parts.extensions.get::<axum::extract::ConnectInfo<std::net::SocketAddr>>().map(|c| c.0),
        ))
    }
}

// ------------------------------------------------------------------ audit ---

/// Record one attempt, and never let recording it fail the request.
///
/// The read this describes has already been authorised by the time this runs. A
/// storage failure here must not turn a served read into a 500: an audit log
/// that can take down the read path is a worse liability than the gap it guards
/// against. The failure is logged and counted as `mcpd_audit_write_failures_total`
/// so a broken audit log is visible rather than silent.
async fn record(
    state: &AppState,
    headers: &HeaderMap,
    peer: Option<std::net::SocketAddr>,
    action: &str,
    project: &str,
    resource: Option<&str>,
    result: AuditResult,
) {
    let event = AuditEvent {
        audit_id: mcpd_core::ids::new_trace_id(),
        timestamp: OffsetDateTime::now_utc(),
        // The digest, never the key. An audit log that stores credentials is a
        // credential store nobody guards.
        principal: crate::auth::principal(headers),
        // Only ever set when a proxy the operator named asserted it. A header
        // is forgeable by anyone who can reach the port, so an untrusted peer,
        // an unknown peer or an unusable value all record nothing rather than
        // a guess.
        subject: state
            .config
            .trusted_identity
            .subject(headers, peer.map(|p| p.ip()))
            .unwrap_or_default(),
        action: action.to_owned(),
        project: project.to_owned(),
        resource: resource.map(str::to_owned),
        result,
    };
    if let Err(e) = state.store.record_audit(&event).await {
        state.metrics.audit_write_failures.fetch_add(1, AtomicOrdering::Relaxed);
        tracing::error!(action, project, "the audit row could not be written: {e}");
    }
}

/// Wrap an authorisation decision in an audit row, whichever way it went.
///
/// A refused attempt is exactly the row an audit log exists for, so it is
/// recorded before the error is returned. The decision is passed in rather than
/// made here, so the one endpoint that cannot be scoped to a project uses the
/// same helper as the ones that can.
pub(crate) async fn audited(
    state: &AppState,
    headers: &HeaderMap,
    peer: Option<std::net::SocketAddr>,
    decision: ApiResult<Grant>,
    action: &str,
    project: &str,
    resource: Option<&str>,
) -> ApiResult<Grant> {
    let result = if decision.is_ok() { AuditResult::Allowed } else { AuditResult::Refused };
    record(state, headers, peer, action, project, resource, result).await;
    decision
}

/// [`audited`], for a handler that has a request to validate before it reads.
///
/// Three steps, in an order that matters. The key is checked first, so a
/// caller without one learns nothing about what this endpoint accepts. The
/// request is validated second, and a request that fails is answered with its
/// 400 and no row at all: nothing was read, and a row written before
/// validation recorded `allowed` for a response that served nothing. The row
/// is written last, `allowed` or `refused`, before anything is read.
///
/// `validate` runs only when the key was accepted, and what it returns is
/// handed back for the read to use, so the query that was validated is the
/// query that runs.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn audited_valid<T>(
    state: &AppState,
    headers: &HeaderMap,
    peer: Option<std::net::SocketAddr>,
    decision: ApiResult<Grant>,
    validate: impl FnOnce() -> ApiResult<T>,
    action: &str,
    project: &str,
    resource: Option<&str>,
) -> ApiResult<T> {
    let valid = match &decision {
        Ok(_) => Some(validate()?),
        Err(_) => None,
    };
    audited(state, headers, peer, decision, action, project, resource).await?;
    // `audited` hands a refusal back, so reaching here means the key was
    // accepted and `validate` ran.
    valid.ok_or_else(|| ApiError::Internal("a refused request reached its read".to_owned()))
}

/// The `cursor` parameter, refused here when no store could have written it.
///
/// Every store writes a page cursor as the hex of `value|id`, where the value
/// is a whole number, or a decimal for a sort on cost. The store refused any
/// other shape with a 400, but only after the audit row, so the row recorded
/// `allowed` for a request that read nothing. Called from the `validate` step
/// of [`audited_valid`] instead, like every other parameter. The test is the
/// stores' own, and so is the refusal, word for word, so a caller sees the
/// same `invalid_query` the searching page documents.
fn page_cursor(p: &Params, decimal: bool) -> ApiResult<Option<String>> {
    let Some(raw) = get(p, "cursor") else { return Ok(None) };
    let text = hex::decode(raw).ok().and_then(|b| String::from_utf8(b).ok());
    let usable = text.as_deref().and_then(|t| t.split_once('|')).is_some_and(|(value, _)| {
        if decimal {
            value.parse::<f64>().is_ok()
        } else {
            value.parse::<i64>().is_ok()
        }
    });
    if usable {
        Ok(Some(raw.to_owned()))
    } else {
        Err(ApiError::Storage(mcpd_store::Error::InvalidQuery("malformed cursor".to_owned())))
    }
}

/// `GET /v1/api/audit`
pub async fn list_audit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(p): Query<Params>,
) -> ApiResult<Json<Vec<AuditEvent>>> {
    let project = project(&p, &state, &headers);
    authorize_project(&state, &headers, Scope::Admin, &project)?;
    // Reading the log is not itself recorded. A log that recorded every read of
    // itself would grow without a payload ever being read, and the row would say
    // nothing the holder of the admin key does not already know.
    let q = AuditQuery {
        project,
        action: get(&p, "action").map(str::to_owned),
        from: parse_time(&p, "from")?,
        to: parse_time(&p, "to")?,
        limit: parse::<u32>(&p, "limit")?.unwrap_or(100).clamp(1, 1000),
    };
    Ok(Json(state.store.list_audit(&q).await?))
}

// ------------------------------------------------------------------ reads ---

/// `GET /v1/api/traces`
pub async fn list_traces(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(p): Query<Params>,
) -> ApiResult<Json<TraceListResponse>> {
    let project = project(&p, &state, &headers);
    authorize_project(&state, &headers, Scope::Read, &project)?;
    // Before anything is read, and after the key is checked. A refusal that
    // named the vocabulary before authorising would tell an anonymous caller
    // what this daemon serves.
    only(&p, "GET /v1/api/traces", TRACE_PARAMS, TRACE_RECORDED_NOT_FILTERED)?;
    state.metrics.query_requests.fetch_add(1, AtomicOrdering::Relaxed);

    let q = TraceQuery {
        project: project.clone(),
        q: get(&p, "q").map(str::to_owned),
        status: parse_enum::<SpanStatus>(&p, "status", "unset, ok, error, cancelled")?,
        kind: parse_enum::<SpanKind>(&p, "kind", &kind_names())?,
        name: get(&p, "name").map(str::to_owned),
        tags: get(&p, "tags")
            .map(|s| s.split(',').map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()).collect())
            .unwrap_or_default(),
        session_id: get(&p, "session_id").map(str::to_owned),
        thread_id: get(&p, "thread_id").map(str::to_owned),
        // Stored user ids are hashed, so the filter value has to be hashed
        // with the same key to match. Without this, hashing would quietly
        // break "show me this customer's runs", which is the main reason the
        // field exists.
        user_id: state.redactor.hash_identifier(get(&p, "user_id")),
        environment: get(&p, "environment").map(str::to_owned),
        model: get(&p, "model").map(str::to_owned),
        from: parse_time(&p, "from")?,
        to: parse_time(&p, "to")?,
        min_duration_ms: parse(&p, "min_duration_ms")?,
        min_cost: parse(&p, "min_cost")?,
        has_score: get(&p, "has_score").map(str::to_owned),
        score_below: parse(&p, "score_below")?,
        sort: match get(&p, "sort") {
            Some("duration") => TraceSort::Duration,
            Some("cost") => TraceSort::Cost,
            Some("span_count") => TraceSort::SpanCount,
            // The lowest score the run was given. A run nothing has scored yet
            // ranks as a zero, so `order=asc` is "worst first" with the
            // unscored runs at the front of it.
            Some("score") => TraceSort::Score,
            Some("start_time") | None => TraceSort::StartTime,
            Some(other) => {
                return Err(ApiError::BadParam {
                    field: "sort".to_owned(),
                    message: format!(
                        "{other:?} is not sortable; use start_time, duration, cost, \
                         span_count or score"
                    ),
                })
            }
        },
        order: order(&p),
        limit: parse::<u32>(&p, "limit")?.unwrap_or(50).clamp(1, 500),
        cursor: get(&p, "cursor").map(str::to_owned),
    };

    let paged = state.store.list_traces(&q).await?;
    Ok(Json(TraceListResponse { page: page_of(&paged), traces: paged.items }))
}

/// `GET /v1/api/traces/{trace_id}`
pub async fn get_trace(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(trace_id): Path<String>,
    Query(p): Query<Params>,
) -> ApiResult<Json<TraceDetail>> {
    let project = project(&p, &state, &headers);
    // Audited: this hands back the run's prompts and completions in full.
    audited(
        &state,
        &headers,
        peer.0,
        authorize_project(&state, &headers, Scope::Read, &project),
        "get_trace",
        &project,
        Some(&trace_id),
    )
    .await?;
    state.metrics.query_requests.fetch_add(1, AtomicOrdering::Relaxed);
    let include = Include::parse(get(&p, "include"));
    let detail = state.store.get_trace(&project, &trace_id, &include).await?;
    Ok(Json(detail))
}

/// `DELETE /v1/api/traces/{trace_id}`
pub async fn delete_trace(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(trace_id): Path<String>,
    Query(p): Query<Params>,
) -> ApiResult<Json<DeleteResult>> {
    // Admin, as the reference has always documented it. It enforced the read
    // scope until this was fixed, so any key that could read a run could also
    // destroy it. With an unset admin list this still accepts the read keys, so
    // the fix changes what a deployment can express, not whether it starts.
    let project = project(&p, &state, &headers);
    audited(
        &state,
        &headers,
        peer.0,
        authorize_project(&state, &headers, Scope::Admin, &project),
        "delete_trace",
        &project,
        Some(&trace_id),
    )
    .await?;
    let deleted = state.store.delete_trace(&project, &trace_id).await?;
    tracing::info!(trace_id = %trace_id, deleted, "trace deleted");
    Ok(Json(DeleteResult { deleted }))
}

/// `GET /v1/api/spans`
pub async fn list_spans(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Query(p): Query<Params>,
) -> ApiResult<Json<SpanListResponse>> {
    let project = project(&p, &state, &headers);
    // Audited: a span carries the input and output of one step. The whole
    // query is built before the row, so a parameter answered with 400 is never
    // recorded as a read that was served.
    let q = audited_valid(
        &state,
        &headers,
        peer.0,
        authorize_project(&state, &headers, Scope::Read, &project),
        || {
            // A span list carries no tags, session or user of its own: those
            // belong to the run. The reference has always said so, and the
            // door silently accepted and dropped all three, so a search
            // narrowed by session returned every span in the project.
            only(&p, "GET /v1/api/spans", SPAN_PARAMS, TRACE_RECORDED_NOT_FILTERED)?;
            let sort = match get(&p, "sort") {
                Some("duration") => SpanSort::Duration,
                Some("cost") => SpanSort::Cost,
                Some("start_time") | None => SpanSort::StartTime,
                Some(other) => {
                    return Err(ApiError::BadParam {
                        field: "sort".to_owned(),
                        message: format!(
                            "{other:?} is not sortable; use start_time, duration or cost"
                        ),
                    })
                }
            };
            Ok(SpanQuery {
                project: project.clone(),
                q: get(&p, "q").map(str::to_owned),
                kind: parse_enum::<SpanKind>(&p, "kind", &kind_names())?,
                status: parse_enum::<SpanStatus>(&p, "status", "unset, ok, error, cancelled")?,
                name: get(&p, "name").map(str::to_owned),
                model: get(&p, "model").map(str::to_owned),
                trace_id: get(&p, "trace_id").map(str::to_owned),
                from: parse_time(&p, "from")?,
                to: parse_time(&p, "to")?,
                min_duration_ms: parse(&p, "min_duration_ms")?,
                sort,
                order: order(&p),
                limit: parse::<u32>(&p, "limit")?.unwrap_or(50).clamp(1, 500),
                cursor: page_cursor(&p, sort == SpanSort::Cost)?,
            })
        },
        "list_spans",
        &project,
        get(&p, "trace_id"),
    )
    .await?;
    state.metrics.query_requests.fetch_add(1, AtomicOrdering::Relaxed);
    let paged = state.store.list_spans(&q).await?;
    Ok(Json(SpanListResponse { page: page_of(&paged), spans: paged.items }))
}

/// `GET /v1/api/logs`
pub async fn list_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Query(p): Query<Params>,
) -> ApiResult<Json<LogListResponse>> {
    let project = project(&p, &state, &headers);
    // Audited: log messages and structured fields are payloads too. Validated
    // before the row, for the reason `audited_valid` gives.
    let q = audited_valid(
        &state,
        &headers,
        peer.0,
        authorize_project(&state, &headers, Scope::Read, &project),
        || {
            only(&p, "GET /v1/api/logs", LOG_PARAMS, &[])?;
            Ok(LogQuery {
                project: project.clone(),
                q: get(&p, "q").map(str::to_owned),
                level: parse_enum::<LogLevel>(
                    &p,
                    "level",
                    "trace, debug, info, warn, error, fatal",
                )?,
                trace_id: get(&p, "trace_id").map(str::to_owned),
                span_id: get(&p, "span_id").map(str::to_owned),
                from: parse_time(&p, "from")?,
                to: parse_time(&p, "to")?,
                order: order(&p),
                limit: parse::<u32>(&p, "limit")?.unwrap_or(100).clamp(1, 1000),
                cursor: page_cursor(&p, false)?,
            })
        },
        "list_logs",
        &project,
        get(&p, "trace_id"),
    )
    .await?;
    state.metrics.query_requests.fetch_add(1, AtomicOrdering::Relaxed);
    let paged = state.store.list_logs(&q).await?;
    Ok(Json(LogListResponse { page: page_of(&paged), logs: paged.items }))
}

/// `GET /v1/api/metrics`
pub async fn query_metrics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(p): Query<Params>,
) -> ApiResult<Json<MetricSeriesResponse>> {
    let project = project(&p, &state, &headers);
    authorize_project(&state, &headers, Scope::Read, &project)?;
    only(&p, "GET /v1/api/metrics", METRIC_PARAMS, &[])?;
    state.metrics.query_requests.fetch_add(1, AtomicOrdering::Relaxed);

    let name = get(&p, "name").ok_or_else(|| ApiError::BadParam {
        field: "name".to_owned(),
        message: "a metric name is required".to_owned(),
    })?;
    let from = parse_time(&p, "from")?
        .unwrap_or_else(|| OffsetDateTime::now_utc() - time::Duration::hours(1));
    let to = parse_time(&p, "to")?.unwrap_or_else(OffsetDateTime::now_utc);
    if to <= from {
        return Err(ApiError::BadParam {
            field: "to".to_owned(),
            message: "the end of the window must be after its start".to_owned(),
        });
    }

    let q = MetricQuery {
        project: project.clone(),
        name: name.to_owned(),
        from,
        to,
        bucket_seconds: get(&p, "bucket").map(parse_duration_seconds).transpose()?.unwrap_or(0),
        agg: match get(&p, "agg") {
            Some("sum") => Aggregation::Sum,
            Some("count") => Aggregation::Count,
            Some("min") => Aggregation::Min,
            Some("max") => Aggregation::Max,
            Some("p50") => Aggregation::P50,
            Some("p90") => Aggregation::P90,
            Some("p95") => Aggregation::P95,
            Some("p99") => Aggregation::P99,
            Some("mean") | None => Aggregation::Mean,
            Some(other) => {
                return Err(ApiError::BadParam {
                    field: "agg".to_owned(),
                    message: format!(
                        "{other:?} is not an aggregation; use sum, count, mean, min, max, p50, p90, p95 or p99"
                    ),
                })
            }
        },
        group_by: get(&p, "group_by")
            .map(|s| s.split(',').map(|k| k.trim().to_owned()).filter(|k| !k.is_empty()).collect())
            .unwrap_or_default(),
        // `filter=model:gpt-4o,status:ok`. A colon rather than `=` so the value
        // does not need escaping inside a query string.
        filter: get(&p, "filter")
            .map(|raw| {
                raw.split(',')
                    .filter_map(|pair| pair.split_once(':'))
                    .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
                    .filter(|(k, _)| !k.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
    };
    Ok(Json(state.store.query_metrics(&q).await?))
}

/// Parse `30s`, `5m`, `1h`, `1d` into seconds.
fn parse_duration_seconds(raw: &str) -> ApiResult<u32> {
    // The last character, not the last byte: `?bucket=1é` panicked here.
    let unit_ch = raw.chars().next_back();
    let (num, unit) = raw.split_at(raw.len() - unit_ch.map_or(0, char::len_utf8));
    let n: u32 = num.parse().map_err(|_| ApiError::BadParam {
        field: "bucket".to_owned(),
        message: format!("{raw:?} is not a duration; try 30s, 5m, 1h or 1d"),
    })?;
    // `checked_mul`, because `4294968s` in hours overflows a u32 and a release
    // build wraps it silently into a tiny bucket rather than saying no.
    let scaled = |mult: u32| {
        n.checked_mul(mult).ok_or_else(|| ApiError::BadParam {
            field: "bucket".to_owned(),
            message: format!("{raw:?} is too large a duration"),
        })
    };
    Ok(match unit {
        "s" => n,
        "m" => scaled(60)?,
        "h" => scaled(3600)?,
        "d" => scaled(86400)?,
        _ => {
            return Err(ApiError::BadParam {
                field: "bucket".to_owned(),
                message: format!("{raw:?} has an unknown unit; use s, m, h or d"),
            })
        }
    })
}

/// `GET /v1/api/scores`
pub async fn list_scores(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(p): Query<Params>,
) -> ApiResult<Json<ScoreListResponse>> {
    let project = project(&p, &state, &headers);
    authorize_project(&state, &headers, Scope::Read, &project)?;
    only(&p, "GET /v1/api/scores", SCORE_PARAMS, &[])?;
    state.metrics.query_requests.fetch_add(1, AtomicOrdering::Relaxed);
    let q = ScoreQuery {
        project: project.clone(),
        scorer_id: get(&p, "scorer_id").map(str::to_owned),
        source: parse_enum::<ScoreSource>(&p, "source", "live, offline, experiment, manual")?,
        trace_id: get(&p, "trace_id").map(str::to_owned),
        experiment_id: get(&p, "experiment_id").map(str::to_owned),
        min_value: parse(&p, "min_value")?,
        max_value: parse(&p, "max_value")?,
        from: parse_time(&p, "from")?,
        to: parse_time(&p, "to")?,
        limit: parse::<u32>(&p, "limit")?.unwrap_or(50).clamp(1, 500),
        cursor: get(&p, "cursor").map(str::to_owned),
    };
    let paged = state.store.list_scores(&q).await?;
    Ok(Json(ScoreListResponse { page: page_of(&paged), scores: paged.items }))
}

/// `POST /v1/api/scores`
pub async fn create_score(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut score): Json<Score>,
) -> ApiResult<Json<Score>> {
    // Scoped to the project the score names, not merely to the write scope. A
    // score is attached to somebody's run, so a key that could write into any
    // project could annotate another team's traces.
    authorize_project(&state, &headers, Scope::Write, &score.correlation.project)?;
    // Against the same ceiling the batch doors use. A score is a stored row on
    // the same disk, so a project barred from spending its quota on spans could
    // otherwise spend it here instead. `charge_one` existed with tests and no
    // caller, which is why those tests passed while this door stayed open.
    if let Some(refusal) = crate::ingest::charge_one(&state, &score.correlation.project) {
        return Err(ApiError::TooManyRequests(refusal));
    }
    if score.trace_id.is_none() && score.span_id.is_none() {
        return Err(ApiError::BadRequest(
            "a score must name the trace_id or span_id it is about".to_owned(),
        ));
    }
    // A scorer id is what every read path groups by and what a monitor watches
    // by name. An empty one produces a row that can never be found, filtered or
    // alerted on, which is worse than a refusal.
    if score.scorer_id.trim().is_empty() {
        return Err(ApiError::BadParam {
            field: "scorer_id".to_owned(),
            message: "a score needs a scorer_id, the id of the scorer that produced it".to_owned(),
        });
    }
    // A score declares the scale it was measured against, so a value outside it
    // is not comparable to anything, including a later score from the same
    // scorer. Storing it makes every average and every threshold that reads it
    // quietly wrong.
    if let (Some(min), Some(max)) = (score.min, score.max) {
        if score.value < min || score.value > max {
            return Err(ApiError::BadParam {
                field: "value".to_owned(),
                message: format!(
                    "value {} is outside the scale this score declares, minimum {min} \
                     and maximum {max}",
                    score.value
                ),
            });
        }
    }
    if score.score_id.is_empty() {
        score.score_id = mcpd_core::ids::new_trace_id();
    }
    // Through the one intake, not a hand-rolled subset of it. This handler used
    // to redact only `metadata`, so a score written through the API skipped the
    // identifier hashing every other write path applies. `pipeline.rs` says
    // anything that writes goes through `Intake::prepare`, and that sentence has
    // to be true of the HTTP door as well.
    let named_trace = score.trace_id.clone().unwrap_or_default();
    let mut batch = Batch { scores: vec![score], ..Batch::default() };
    Intake::new(&state.redactor, &state.price_book, &state.config.sampling)
        .with_projects(&state.project_policies)
        .prepare(&mut batch);
    // `prepare` may now remove the record: sampling decides every signal, not
    // only spans, so a score naming a trace the sampler discarded comes back
    // from intake with nothing in it. Answer the sender instead of indexing an
    // empty vector, and instead of storing a score whose trace no reader can
    // open.
    let Some(score) = batch.scores.first().cloned() else {
        return Err(ApiError::SampledOut(format!(
            "the trace {named_trace} is not recorded here, because sampling discarded it, so a score naming it has nothing to attach to"
        )));
    };
    state.store.ingest(batch).await?;
    Ok(Json(score))
}

/// `POST /v1/api/feedback`
pub async fn create_feedback(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut feedback): Json<Feedback>,
) -> ApiResult<Json<Feedback>> {
    authorize_project(&state, &headers, Scope::Write, &feedback.correlation.project)?;
    if let Some(refusal) = crate::ingest::charge_one(&state, &feedback.correlation.project) {
        return Err(ApiError::TooManyRequests(refusal));
    }
    if feedback.trace_id.is_none() && feedback.span_id.is_none() {
        return Err(ApiError::BadRequest(
            "feedback must name the trace_id or span_id it is about".to_owned(),
        ));
    }
    if feedback.feedback_id.is_empty() {
        feedback.feedback_id = mcpd_core::ids::new_trace_id();
    }
    // Same reason, and it mattered more here: the intake hashes `author_id` and
    // redacts `comment`, and this handler did neither. A person's name and
    // whatever they typed were stored raw, against the guarantee on the privacy
    // page that redaction covers all five signals.
    let named_trace = feedback.trace_id.clone().unwrap_or_default();
    let mut batch = Batch { feedback: vec![feedback], ..Batch::default() };
    Intake::new(&state.redactor, &state.price_book, &state.config.sampling)
        .with_projects(&state.project_policies)
        .prepare(&mut batch);
    // Same as the score door above, and for the same reason.
    let Some(feedback) = batch.feedback.first().cloned() else {
        return Err(ApiError::SampledOut(format!(
            "the trace {named_trace} is not recorded here, because sampling discarded it, so feedback naming it has nothing to attach to"
        )));
    };
    state.store.ingest(batch).await?;
    Ok(Json(feedback))
}

/// `GET /v1/api/feedback`
///
/// Feedback for one run, read from the trace detail. That is the whole of what
/// this endpoint serves, and `trace_id` is refused when absent rather than
/// answered with an empty list. The empty list was the older behaviour and it
/// was the dishonest one: a project full of reviewer comments answered
/// `{"feedback": []}` with a 200, which reads as "nobody has judged anything
/// here". A project-wide feedback list needs a store method that does not
/// exist yet, so the door says so instead of implying the answer is none.
///
/// The five SDKs that send `kind`, `from`, `to` and `cursor` here are refused
/// by name for the same reason: the daemon never applied any of them.
pub async fn list_feedback(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Query(p): Query<Params>,
) -> ApiResult<Json<FeedbackListResponse>> {
    let project = project(&p, &state, &headers);
    // Project-scoped, like every other read of a payload. It checked the scope
    // and not the project, so a key scoped to one team could read another team's
    // reviewer comments by naming their project.
    // Validated before the row, for the reason `audited_valid` gives.
    let (trace_id, limit) = audited_valid(
        &state,
        &headers,
        peer.0,
        authorize_project(&state, &headers, Scope::Read, &project),
        || {
            only(&p, "GET /v1/api/feedback", FEEDBACK_PARAMS, &[])?;
            let trace_id = get(&p, "trace_id").ok_or_else(|| ApiError::BadParam {
                field: "trace_id".to_owned(),
                message: "name the run whose judgements you want. This endpoint reads the \
                          feedback attached to one run, and mcpd cannot yet list a whole \
                          project's feedback, so an answer with no trace_id would be an empty \
                          list that reads as though nobody had judged anything."
                    .to_owned(),
            })?;
            let limit = parse::<u32>(&p, "limit")?.unwrap_or(50).clamp(1, 500) as usize;
            Ok((trace_id, limit))
        },
        "list_feedback",
        &project,
        get(&p, "trace_id"),
    )
    .await?;
    let detail = state
        .store
        .get_trace(&project, trace_id, &Include { feedback: true, ..Include::default() })
        .await?;
    let mut items = detail.feedback.unwrap_or_default();
    items.truncate(limit);
    Ok(Json(FeedbackListResponse {
        page: Page { next_cursor: None, has_more: false, total: Some(items.len() as u64) },
        feedback: items,
    }))
}

/// `GET /v1/api/analytics/summary`
pub async fn analytics_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(p): Query<Params>,
) -> ApiResult<Json<AnalyticsSummary>> {
    let project = project(&p, &state, &headers);
    authorize_project(&state, &headers, Scope::Read, &project)?;
    state.metrics.query_requests.fetch_add(1, AtomicOrdering::Relaxed);
    let from = parse_time(&p, "from")?
        .unwrap_or_else(|| OffsetDateTime::now_utc() - time::Duration::hours(24));
    let to = parse_time(&p, "to")?.unwrap_or_else(OffsetDateTime::now_utc);
    let s = state.store.analytics_summary(&project, from, to, get(&p, "group_by")).await?;
    Ok(Json(s))
}

/// `GET /v1/api/analytics/cost`
pub async fn analytics_cost(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(p): Query<Params>,
) -> ApiResult<Json<CostBreakdown>> {
    let project = project(&p, &state, &headers);
    authorize_project(&state, &headers, Scope::Read, &project)?;
    state.metrics.query_requests.fetch_add(1, AtomicOrdering::Relaxed);
    let from = parse_time(&p, "from")?
        .unwrap_or_else(|| OffsetDateTime::now_utc() - time::Duration::days(7));
    let to = parse_time(&p, "to")?.unwrap_or_else(OffsetDateTime::now_utc);
    let bucket = get(&p, "bucket").map(parse_duration_seconds).transpose()?;
    let c = state
        .store
        .analytics_cost(&project, from, to, get(&p, "group_by").unwrap_or("model"), bucket)
        .await?;
    Ok(Json(c))
}

/// `GET /v1/api/projects`
pub async fn list_projects(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<String>>> {
    // Filtered by the grant, not merely gated by the read scope. Project names
    // are not payload, but they are the org chart: which teams exist, what they
    // are working on, what got renamed. A key scoped to one project used to
    // enumerate every other one from here, which undoes the boundary the rest
    // of this file spends its effort establishing.
    let grant = authorize(&state, &headers, Scope::Read)?;
    let mut names = state.store.list_projects().await?;
    if let Some(allowed) = grant.projects() {
        names.retain(|n| allowed.contains(n));
    }
    Ok(Json(names))
}

// ------------------------------------------------------------ operations ---

/// `GET /healthz` — liveness.
///
/// Never touches storage: a slow database must not cause a restart loop.
pub async fn healthz(State(state): State<Arc<AppState>>) -> Json<HealthStatus> {
    Json(HealthStatus {
        status: "ok".to_owned(),
        version: mcpd_core::VERSION.to_owned(),
        spec_version: mcpd_core::SPEC_VERSION.to_owned(),
        uptime_seconds: state.uptime_seconds(),
        storage: Some(state.store.name().to_owned()),
        detail: None,
        // Absent rather than `false` when off, so the probe a self-hosted
        // daemon answers is byte for byte what it was before accounts existed.
        signup: state.config.signup.then_some(true),
    })
}

/// `GET /readyz` — readiness. Touches storage, so traffic is only sent to an
/// instance that can actually serve it.
pub async fn readyz(
    State(state): State<Arc<AppState>>,
) -> (axum::http::StatusCode, Json<HealthStatus>) {
    // Both stores. A replica whose run store cannot be read answers every run
    // and worker call with an error, and readiness was the one place that
    // could have taken it out of service and did not look.
    let runs = match &state.runs {
        Some(runs) => runs.ping().await.map_err(|e| format!("the run store is not reachable: {e}")),
        None => Ok(()),
    };
    let (status, detail, code) = match (state.store.ping().await, runs) {
        (Ok(()), Ok(())) => ("ok", None, axum::http::StatusCode::OK),
        (Err(e), _) => (
            "degraded",
            Some(format!("storage is not reachable: {e}")),
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
        ),
        (Ok(()), Err(why)) => ("degraded", Some(why), axum::http::StatusCode::SERVICE_UNAVAILABLE),
    };
    (
        code,
        Json(HealthStatus {
            status: status.to_owned(),
            version: mcpd_core::VERSION.to_owned(),
            spec_version: mcpd_core::SPEC_VERSION.to_owned(),
            uptime_seconds: state.uptime_seconds(),
            storage: Some(state.store.name().to_owned()),
            detail,
            signup: state.config.signup.then_some(true),
        }),
    )
}

/// `GET /metrics` — mcpd's own Prometheus metrics.
pub async fn self_metrics(
    State(state): State<Arc<AppState>>,
) -> ([(&'static str, &'static str); 1], String) {
    (
        [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
        state.metrics.render(state.uptime_seconds()),
    )
}

// ------------------------------------------------------------ evaluation ---

/// `GET /v1/api/scorers` — the built-in scorer catalogue.
pub async fn list_scorers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<crate::eval::ScorerInfo>>> {
    authorize(&state, &headers, Scope::Read)?;
    Ok(Json(crate::eval::catalogue(state.config.judge.as_ref())))
}

/// `POST /v1/api/score` — run scorers over stored runs.
///
/// The project is authorised here, against the same name `run_scorers` will
/// resolve, and that is not a formality. This door names its project in the
/// request BODY rather than the query string, so the plain `authorize` it used
/// to call checked only that the key was a write key somewhere. It discarded
/// the grant, and the grant is the part that says which projects a key reaches.
/// A key issued to one project could name another in the body and have this
/// endpoint read that project's runs and write scores back into it. Every
/// neighbouring write door already used `authorize_project`; this one did not.
///
/// Resolved twice on purpose. `run_scorers` needs the name too, and a helper
/// shared between them would be one more place for the default to drift. The
/// two lines must agree, and `a_write_key_cannot_score_another_project` fails
/// if they stop agreeing.
pub async fn score_runs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<crate::eval::ScoreRunRequest>,
) -> ApiResult<Json<crate::eval::ScoreRunResult>> {
    let project = req.project.clone().unwrap_or_else(|| state.config.default_project.clone());
    authorize_project(&state, &headers, Scope::Write, &project)?;
    Ok(Json(crate::eval::run_scorers(&state, req).await?))
}

/// `GET /v1/api/experiments/compare` — compare two experiments.
pub async fn compare_experiments(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(p): Query<Params>,
) -> ApiResult<Json<crate::eval::ExperimentComparison>> {
    let project = project(&p, &state, &headers);
    authorize_project(&state, &headers, Scope::Read, &project)?;
    let baseline = get(&p, "baseline").ok_or_else(|| ApiError::BadParam {
        field: "baseline".to_owned(),
        message: "name the experiment to compare against".to_owned(),
    })?;
    let candidate = get(&p, "candidate").ok_or_else(|| ApiError::BadParam {
        field: "candidate".to_owned(),
        message: "name the experiment being judged".to_owned(),
    })?;

    // Read both sides at full page size: a comparison over a truncated sample
    // would report a delta that the data does not support.
    let fetch = |experiment: String| {
        let store = state.store.clone();
        let project = project.clone();
        async move {
            store
                .list_scores(&mcpd_store::ScoreQuery {
                    project,
                    experiment_id: Some(experiment),
                    limit: 500,
                    ..Default::default()
                })
                .await
        }
    };

    let base = fetch(baseline.to_owned()).await?;
    let cand = fetch(candidate.to_owned()).await?;
    Ok(Json(crate::eval::compare(baseline, candidate, &base.items, &cand.items)))
}

// ---------------------------------------------- datasets and experiments ---

/// `GET /v1/api/datasets`
pub async fn list_datasets(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(p): Query<Params>,
) -> ApiResult<Json<Vec<mcpd_core::Dataset>>> {
    let project = project(&p, &state, &headers);
    authorize_project(&state, &headers, Scope::Read, &project)?;
    Ok(Json(state.store.list_datasets(&project).await?))
}

/// `POST /v1/api/datasets`
pub async fn create_dataset(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut d): Json<mcpd_core::Dataset>,
) -> ApiResult<Json<mcpd_core::Dataset>> {
    authorize_project(&state, &headers, Scope::Write, &d.correlation.project)?;
    // A caller-supplied id that already belongs to somebody else is refused
    // before anything is written. Without this the tenancy boundary on datasets
    // is decoration: the read paths resolve the owner with `dataset_project`,
    // so re-creating another project's id under your own moves the owner and
    // every one of those checks then says yes. Reproduced end to end before it
    // was closed: a 403 on the items endpoint became a 200 after three
    // requests, and the other team's payload came back out.
    if !d.dataset_id.is_empty() {
        if let Some(owner) = state.store.dataset_project(&d.dataset_id).await? {
            let grant = authorize(&state, &headers, Scope::Write)?;
            if !grant.covers(&owner) {
                return Err(ApiError::Forbidden(format!(
                    "dataset {} belongs to another project",
                    d.dataset_id
                )));
            }
        }
    }
    if d.dataset_id.is_empty() {
        d.dataset_id = mcpd_core::ids::new_trace_id();
    }
    if d.name.trim().is_empty() {
        return Err(ApiError::BadParam {
            field: "name".to_owned(),
            message: "a dataset needs a name a person can recognise later".to_owned(),
        });
    }
    state.store.put_dataset(&d).await?;
    Ok(Json(d))
}

/// `GET /v1/api/datasets/{id}/items`
pub async fn list_dataset_items(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(dataset_id): Path<String>,
    Query(p): Query<Params>,
) -> ApiResult<Json<Vec<mcpd_core::DatasetItem>>> {
    // Scoped to the project that owns the dataset, which is looked up rather
    // than taken from the caller. This is the endpoint where it matters most: a
    // dataset captured from production holds real inputs and outputs, so a read
    // key that learned an id used to read another team's payloads through here.
    // An unknown id is refused the same way as one the key may not reach, so
    // this cannot be used to test which ids exist.
    let owner = state.store.dataset_project(&dataset_id).await?;
    let project = owner.clone().unwrap_or_else(|| project(&p, &state, &headers));
    let limit = audited_valid(
        &state,
        &headers,
        peer.0,
        match owner {
            Some(ref owner) => authorize_project(&state, &headers, Scope::Read, owner),
            None => authorize(&state, &headers, Scope::Read)
                .and_then(|_| Err(ApiError::NotFound(format!("no dataset with id {dataset_id}")))),
        },
        || Ok(parse::<u32>(&p, "limit")?.unwrap_or(500)),
        "list_dataset_items",
        &project,
        Some(&dataset_id),
    )
    .await?;
    Ok(Json(state.store.list_dataset_items(&dataset_id, limit).await?))
}

/// `POST /v1/api/datasets/{id}/items`
pub async fn add_dataset_items(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(dataset_id): Path<String>,
    Json(mut items): Json<Vec<mcpd_core::DatasetItem>>,
) -> ApiResult<Json<serde_json::Value>> {
    // The scope is checked BEFORE the id is looked up, and the order is the
    // whole point. Looking up first and returning NotFound for an unknown id
    // answered 404 for an id that does not exist and 401 for one that does, so
    // anyone able to reach the port could enumerate dataset ids without a
    // credential. The read side got this right and this side did not.
    let grant = authorize(&state, &headers, Scope::Write)?;
    match state.store.dataset_project(&dataset_id).await? {
        Some(ref owner) if !grant.covers(owner) => {
            return Err(ApiError::Forbidden(format!(
                "that key does not reach the project that owns dataset {dataset_id}"
            )))
        }
        Some(_) => {}
        None => return Err(ApiError::NotFound(format!("no dataset with id {dataset_id}"))),
    }
    for it in &mut items {
        if it.item_id.is_empty() {
            it.item_id = mcpd_core::ids::new_trace_id();
        }
        it.dataset_id = dataset_id.clone();
        // Through the redactor, like every other write path. This endpoint held
        // captured production inputs and stored them exactly as sent, so a
        // credential in a field named after what it is survived here while the
        // same field was masked everywhere else. `pipeline.rs` says anything
        // that writes goes through the intake, and that has to be true of the
        // door that takes payloads by hand as well.
        it.input = state.redactor.redact(&it.input);
        it.expected = state.redactor.redact_opt(&it.expected);
        it.metadata = state.redactor.redact_opt(&it.metadata);
    }
    let n = state.store.add_dataset_items(&dataset_id, &items).await?;
    Ok(Json(serde_json::json!({ "added": n })))
}

/// `POST /v1/api/datasets/{id}/capture` — turn stored runs into cases.
///
/// This is the point of datasets: a production run that went wrong becomes a
/// permanent regression test, carrying its own trace id so anyone can go back
/// and read what actually happened.
pub async fn capture_into_dataset(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(dataset_id): Path<String>,
    Query(p): Query<Params>,
    Json(body): Json<serde_json::Value>,
) -> ApiResult<Json<serde_json::Value>> {
    let project = project(&p, &state, &headers);
    // Two projects are involved and both have to be covered: the runs are read
    // out of `project`, and the cases are written into whichever project owns
    // the dataset. Without the second check a key scoped to one team could copy
    // its own runs into another team's regression suite.
    //
    // Both decisions are made before the row is written, because a row written
    // between them recorded `allowed` for a request the owner check then refused
    // with a 403. A log that reports the wrong outcome is worse than one with a
    // gap in it: the gap is visible, and this is read as fact.
    let owner = state.store.dataset_project(&dataset_id).await?;
    // Write, not ingest, and audited. This reads any named run's input and
    // output and copies them into a dataset, so authorising it with the key that
    // lives in every container that emits a span was the wrong door entirely.
    // An unknown id is refused the same way as one the key may not reach, and
    // only after the read side passed, so this cannot be used to test which ids
    // exist.
    let decision =
        authorize_project(&state, &headers, Scope::Write, &project).and_then(|grant| match owner {
            Some(ref owner) => {
                authorize_project(&state, &headers, Scope::Write, owner).map(|_| grant)
            }
            None => Err(ApiError::NotFound(format!("no dataset with id {dataset_id}"))),
        });
    // The body is checked before the row, for the reason `audited_valid`
    // gives: a capture that names no run reads nothing and copies nothing.
    let trace_ids: Vec<String> = audited_valid(
        &state,
        &headers,
        peer.0,
        decision,
        || {
            let trace_ids: Vec<String> = body
                .get("trace_ids")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                .unwrap_or_default();
            if trace_ids.is_empty() {
                return Err(ApiError::BadParam {
                    field: "trace_ids".to_owned(),
                    message: "name the runs to capture, as a non-empty `trace_ids` array of \
                              trace id strings"
                        .to_owned(),
                });
            }
            Ok(trace_ids)
        },
        "capture_into_dataset",
        &project,
        Some(&dataset_id),
    )
    .await?;
    let proj = project.clone();

    let mut items = Vec::new();
    for trace_id in &trace_ids {
        let detail = match state.store.get_trace(&proj, trace_id, &Include::default()).await {
            Ok(d) => d,
            Err(mcpd_store::Error::NotFound { .. }) => continue,
            Err(e) => return Err(e.into()),
        };
        let root = detail.spans.iter().find(|s| s.is_root).or_else(|| detail.spans.first());
        let Some(root) = root else { continue };
        items.push(mcpd_core::DatasetItem {
            item_id: mcpd_core::ids::new_trace_id(),
            dataset_id: dataset_id.clone(),
            input: root.input.clone().unwrap_or(serde_json::Value::Null),
            // The run's own output becomes the reference only when it was
            // right. Capturing a bad answer as "expected" would enshrine the
            // bug, so the caller says so explicitly.
            expected: body
                .get("use_output_as_expected")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
                .then(|| root.output.clone())
                .flatten(),
            metadata: Some(serde_json::json!({
                "captured_from": trace_id,
                "captured_at": OffsetDateTime::now_utc().unix_timestamp(),
                "original_status": root.status.as_str(),
            })),
            source_trace_id: Some(trace_id.clone()),
        });
    }

    let n = state.store.add_dataset_items(&dataset_id, &items).await?;
    Ok(Json(serde_json::json!({ "captured": n, "requested": trace_ids.len() })))
}

/// `GET /v1/api/experiments`
pub async fn list_experiments(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(p): Query<Params>,
) -> ApiResult<Json<Vec<mcpd_core::Experiment>>> {
    let project = project(&p, &state, &headers);
    authorize_project(&state, &headers, Scope::Read, &project)?;
    Ok(Json(state.store.list_experiments(&project).await?))
}
/// The subset of a configured list a grant reaches, in configuration order.
///
/// A function of its own so the rule can be tested without standing up a store.
/// The defect it replaces was a bare `states.clone()` in two list handlers:
/// both checked the scope and neither checked the project, so a read key
/// scoped to one project received every project's monitors, and a monitor
/// carries the webhook it posts to. A list handler is exactly the shape where
/// a missing second layer is invisible in review, because the code that is
/// wrong is the code that is not there.
fn reachable<T: Clone>(states: &[T], grant: &Grant, project_of: impl Fn(&T) -> &str) -> Vec<T> {
    states.iter().filter(|s| grant.covers(project_of(s))).cloned().collect()
}

/// `POST /v1/api/experiments`
pub async fn create_experiment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut e): Json<mcpd_core::Experiment>,
) -> ApiResult<Json<mcpd_core::Experiment>> {
    authorize_project(&state, &headers, Scope::Write, &e.correlation.project)?;
    // The same shape as create_dataset: an experiment names a dataset, so
    // pointing one at another project's dataset would read its cases through a
    // door that never checked who owned them.
    if !e.dataset_id.is_empty() {
        if let Some(owner) = state.store.dataset_project(&e.dataset_id).await? {
            let grant = authorize(&state, &headers, Scope::Write)?;
            if !grant.covers(&owner) {
                return Err(ApiError::Forbidden(format!(
                    "dataset {} belongs to another project",
                    e.dataset_id
                )));
            }
        }
    }
    if e.experiment_id.is_empty() {
        e.experiment_id = mcpd_core::ids::new_trace_id();
    }
    if e.started_at.is_none() {
        e.started_at = Some(OffsetDateTime::now_utc());
    }
    state.store.put_experiment(&e).await?;
    Ok(Json(e))
}
// ------------------------------------------------------------- monitors ---

/// The host a webhook posts to, and nothing else of its URL.
///
/// A webhook URL is often its own credential: a Slack incoming webhook carries
/// its token in the path, and other services put one in the query string. The
/// routes that return a monitor are read with a read or a write key, and
/// neither is a key anybody meant to carry an alerting credential, so they name
/// where alerts go and not how to send one. A URL that cannot be parsed is left
/// out rather than echoed, because it cannot be shortened safely.
pub(crate) fn webhook_host(url: Option<&str>) -> Option<String> {
    url.and_then(|u| reqwest::Url::parse(u).ok()).and_then(|u| u.host_str().map(str::to_owned))
}

/// A monitor as the API returns it: its webhook cut down to the host.
fn shown_monitor(mut s: crate::monitor::MonitorState) -> crate::monitor::MonitorState {
    s.monitor.webhook = webhook_host(s.monitor.webhook.as_deref());
    s
}

/// An objective as the API returns it: its webhook cut down to the host.
fn shown_slo(mut s: crate::monitor::SloState) -> crate::monitor::SloState {
    s.slo.webhook = webhook_host(s.slo.webhook.as_deref());
    s
}

/// `GET /v1/api/monitors`
pub async fn list_monitors(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<crate::monitor::MonitorState>>> {
    let grant = authorize(&state, &headers, Scope::Read)?;
    let states = state
        .monitors
        .read()
        .map_err(|_| ApiError::Internal("the monitor state is poisoned".to_owned()))?;
    // Filtered by the grant, not returned whole. A monitor carries the webhook
    // it posts to, so an unfiltered list handed every project's alerting
    // destinations to any read key, including one scoped to a single project.
    // And the webhook is cut to its host, because the rest of it can be the
    // credential that posts to the channel.
    Ok(Json(
        reachable(&states, &grant, |s| &s.monitor.project).into_iter().map(shown_monitor).collect(),
    ))
}

/// `POST /v1/api/monitors/{id}/ack`
///
/// Acknowledging silences the transition without pretending the condition
/// cleared: the monitor stays acknowledged while it still breaches, and returns
/// to nominal on its own when the underlying number recovers.
pub async fn ack_monitor(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<crate::monitor::MonitorState>> {
    let grant = authorize(&state, &headers, Scope::Write)?;
    let mut states = state
        .monitors
        .write()
        .map_err(|_| ApiError::Internal("the monitor state is poisoned".to_owned()))?;
    let slot = states
        .iter_mut()
        .find(|s| s.monitor.id == id)
        .ok_or_else(|| ApiError::NotFound(format!("no monitor with id {id}")))?;
    // The id is global but the monitor belongs to a project, so the grant is
    // checked after the lookup and before the write. The refusal names the
    // monitor the caller already named, never the project that owns it.
    if !grant.covers(&slot.monitor.project) {
        return Err(ApiError::Forbidden(format!(
            "that key does not reach the project that owns monitor {id}"
        )));
    }
    slot.acknowledge();
    Ok(Json(shown_monitor(slot.clone())))
}

/// `GET /v1/api/slos`
pub async fn list_slos(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<crate::monitor::SloState>>> {
    let grant = authorize(&state, &headers, Scope::Read)?;
    let states = state
        .slos
        .read()
        .map_err(|_| ApiError::Internal("the objective state is poisoned".to_owned()))?;
    Ok(Json(reachable(&states, &grant, |s| &s.slo.project).into_iter().map(shown_slo).collect()))
}

/// `POST /v1/api/slos/{id}/ack`
pub async fn ack_slo(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<crate::monitor::SloState>> {
    let grant = authorize(&state, &headers, Scope::Write)?;
    let mut states = state
        .slos
        .write()
        .map_err(|_| ApiError::Internal("the objective state is poisoned".to_owned()))?;
    let slot = states
        .iter_mut()
        .find(|s| s.slo.id == id)
        .ok_or_else(|| ApiError::NotFound(format!("no objective with id {id}")))?;
    if !grant.covers(&slot.slo.project) {
        return Err(ApiError::Forbidden(format!(
            "that key does not reach the project that owns objective {id}"
        )));
    }
    slot.acknowledge();
    Ok(Json(shown_slo(slot.clone())))
}

/// Why an erasure's numbers may not be the whole answer, and whether anybody
/// can act on the reason.
struct Advisory {
    /// The sentences to hand back, or `None` when there is nothing to say.
    note: Option<String>,
    /// True when this reply names something an operator can do about it.
    ///
    /// It is what decides the log level. A fully successful erasure that
    /// emptied a captured dataset case carries a note, and logging that at WARN
    /// taught an operator to ignore the level, which is worse than not logging
    /// it. Only the two reasons with a way out set this: rows in a form this
    /// request could not match, and a zero the endpoint cannot account for.
    unresolved: bool,
}

/// Say why an erasure's numbers may not be the whole answer, or say nothing.
///
/// A zero from this endpoint is the most dangerous value it can return, because
/// it is what an operator repeats to a person as "we hold nothing about you".
/// Stored identifiers are hashed with the redaction key, and nothing records
/// which key hashed which row, so a zero can equally mean the key moved after
/// those rows were written. The endpoint cannot tell the two apart. What it can
/// do is never hand back a bare zero that reads like the first one.
///
/// A number above zero needed the same treatment and did not have it. After a
/// key rotation every identifier is a digest, including the subject's, so
/// nothing looks mixed and `unmatchable` stays at zero. The request matches the
/// rows written under the current key, deletes them, and reports a count that
/// reads as the whole answer while the subject's older rows are still on disk
/// under a digest no request can produce. That is a partial erasure reported as
/// a complete one, and it is what the `opaque_remaining` arm below exists for.
///
/// It reports rather than refuses. Refusing would break the two properties this
/// endpoint was built with: a repeat after a timeout has to be answerable, and
/// the reply must be the same for "never seen" and "erased an hour ago", or the
/// endpoint becomes a way to ask whether a person is known to a project. A
/// refusal would also block every erasure in a deployment whose redaction
/// setting ever changed, including the ones that correctly matched nothing.
fn erasure_note(outcome: &ErasureOutcome) -> Advisory {
    let mut lines: Vec<String> = Vec::new();
    let ids = &outcome.identifiers;
    let mut unresolved = false;

    if ids.unmatchable > 0 {
        unresolved = true;
        lines.push(format!(
            "{} stored identifiers in this project are in a form this request could not \
             match, so rows written under a different redaction setting were not searched \
             and rows about this person may remain. Restore the redaction setting those \
             rows were written under and repeat the request.",
            ids.unmatchable
        ));
    }

    // Durable runs are counted apart from `deleted`, which is the trace store
    // alone. A run whose spans had not been drained yet is erased from the run
    // store and leaves the trace store at zero, and the two lines below read
    // that zero as "Nothing matched" beside `runs_erased: 1`.
    let runs_erased = outcome.result.runs_erased.unwrap_or(0);
    let runs_matched = runs_erased.saturating_add(outcome.result.runs_in_flight.unwrap_or(0));

    if outcome.result.deleted == 0 && runs_matched > 0 {
        // A run left whole has its own line, added by the caller.
        let mut line = if runs_erased > 0 {
            format!(
                "{runs_erased} durable runs attributed to this person were erased from the \
                 run store, with any of their spans still waiting to be written. "
            )
        } else {
            String::new()
        };
        if ids.nothing_attributed && ids.unmatchable == 0 {
            line.push_str(
                "Nothing in the trace store matched, and nothing there is attributed to any \
                 subject, so there is no stored identifier a key change could be hiding.",
            );
        } else {
            unresolved = true;
            line.push_str(
                "Nothing in the trace store matched. Stored identifiers there are hashed with \
                 the redaction key, so rows written under a different redaction key cannot \
                 be matched by this endpoint.",
            );
        }
        lines.push(line);
    } else if outcome.result.deleted == 0 {
        if ids.nothing_attributed && ids.unmatchable == 0 {
            // The one thing the endpoint can prove, stated no wider than the
            // evidence. It used to end "so nothing about this person was stored
            // here", which is false in a project that never sets a user id:
            // every run there is unattributed, and unattributed is not absent.
            lines.push(
                "Nothing in this project is attributed to any subject, so there is no \
                 stored identifier a key change could be hiding. That is not the same as \
                 holding nothing about this person: a project that never sets a user id \
                 still records runs, and this endpoint does not search their payloads."
                    .to_owned(),
            );
        } else {
            unresolved = true;
            // This used to end with the number of distinct subjects the project
            // holds. That is a population count of somebody else's data,
            // answering a question about one person, and the caller can act on
            // none of it.
            lines.push(
                "Nothing matched. Stored identifiers are hashed with the redaction key, so \
                 this means no stored identifier hashes to this subject under the key in \
                 force now. It does not mean nothing about this person was ever stored: \
                 rows written under a different redaction key cannot be matched by this \
                 endpoint."
                    .to_owned(),
            );
        }
    } else if ids.opaque_remaining {
        // The partial erasure that reads as a complete one. Nothing here is an
        // accusation that a key moved, because nothing on disk could support
        // one: the claim is only that a digest this request did not match is
        // still stored, and that the daemon cannot tell it apart from this same
        // person under an earlier key.
        lines.push(
            "Digests this request did not match are still stored in this project. \
             Identifiers are one-way, and nothing records which key wrote which row, so \
             none of them can be told apart from this same person under an earlier \
             redaction key. These counts are what this request could address, not proof \
             that nothing else about this person is held."
                .to_owned(),
        );
    }

    if outcome.deferred {
        lines.push(
            "This backend applies the deletes after this reply, so the counts say what \
             matched, not that every row has already left the disk."
                .to_owned(),
        );
    }

    if outcome.dataset_items > 0 {
        lines.push(format!(
            "{} captured dataset cases were emptied in place rather than deleted, so a \
             regression test is not destroyed without anyone being told. They hold no \
             payload any more.",
            outcome.dataset_items
        ));
    }

    Advisory { note: (!lines.is_empty()).then(|| lines.join(" ")), unresolved }
}

/// `DELETE /v1/api/subjects/{user_id}`
///
/// Answers a deletion request about a person in one call. The alternative the
/// reference used to point at was finding every run by hand and deleting it
/// trace by trace, which is a procedure rather than an answer.
pub async fn erase_subject(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(user_id): Path<String>,
    Query(p): Query<Params>,
) -> ApiResult<Json<ErasureResult>> {
    let project = project(&p, &state, &headers);

    // Refuse a blank subject before it can reach a WHERE clause.
    //
    // This is not defensive tidying. ClickHouse stores `user_id` as a
    // non-nullable `String`, so every row that carries no user at all holds
    // `''`. With redaction turned off the identifier is passed through
    // unchanged, so a blank id would reach the backend as `user_id = ''` and
    // match every anonymous run in the project. The one request that must never
    // over-delete would delete nearly everything.
    if user_id.trim().is_empty() {
        return Err(ApiError::BadParam {
            field: "user_id".to_owned(),
            message: "user_id is empty; name the person to erase".to_owned(),
        });
    }

    // Hash the subject the same way ingest hashed it.
    //
    // Stored user ids are hashed by the redactor on the way in, so comparing
    // the raw path segment against the stored column would match nothing on
    // every deployment that has redaction on, and the endpoint would report a
    // confident row of zeroes for a person whose data is still there. That is
    // the worst failure this endpoint has, because it looks like success.
    // `list_traces` hashes its `user_id` filter for the same reason.
    //
    // The hash is only as good as the key being the one the rows were written
    // with. Nothing records which key wrote which row, so the store also
    // reports what it holds that this request could not have addressed, and
    // `erasure_note` turns that into a sentence rather than leaving a zero to
    // speak for itself.
    let subject =
        state.redactor.hash_identifier(Some(user_id.as_str())).unwrap_or_else(|| user_id.clone());

    // Admin, and audited, exactly as `delete_trace` is. This destroys more in
    // one call than any other endpoint, so the row recording it matters more.
    //
    // `authorize_project` is what bounds the blast radius to one tenant: the
    // same person can appear in several projects, the grant decides which one
    // this call may touch, and the store is handed that project and no other.
    //
    // The audited resource is the hashed subject, never the raw one. The audit
    // log outlives the data it describes, on purpose, so writing the plaintext
    // identifier there would leave the very name the person asked to remove
    // sitting in a table that retention does not sweep.
    audited(
        &state,
        &headers,
        peer.0,
        authorize_project(&state, &headers, Scope::Admin, &project),
        "erase_subject",
        &project,
        Some(subject.as_str()),
    )
    .await?;

    // The run store first, and then the trace store. A run's spans wait in
    // its outbox until the drain writes them through ingest, so erasing the
    // trace store first left a window in which a queued span of an erased run
    // landed after the erasure that should have removed it. Deleting the runs
    // deletes the queue; the trace store erasure then removes whatever had
    // already been written.
    //
    // Matched on the identifier as given, not on `subject`. The run store keeps
    // a run's correlation as it was sent, unhashed, and the hash is applied
    // only to the spans the drain writes into the trace store.
    //
    // The order alone does not cover a drain pass that claimed a run's spans
    // before this began and writes them after it. Held across both erasures,
    // so such a pass finishes first; `AppState::outbox_gate` says more.
    let turn = state.outbox_gate.lock().await;
    let runs = match &state.runs {
        Some(store) => Some(
            store
                .erase_subject(&project, &user_id)
                .await
                .map_err(|e| ApiError::Internal(format!("run store: {e}")))?,
        ),
        None => None,
    };

    let mut erased = state.store.erase_subject(&project, &subject).await?;
    drop(turn);
    if let Some(r) = &runs {
        erased.result.runs_erased = Some(u32::try_from(r.runs_erased).unwrap_or(u32::MAX));
        erased.result.runs_in_flight = Some(u32::try_from(r.runs_in_flight).unwrap_or(u32::MAX));
    }
    let mut advisory = erasure_note(&erased);
    // A run left whole is something the operator has to act on, so it is a
    // line in the note and it raises the log level. Without the run ids there
    // would be no way to find the runs: nothing lists runs by person.
    if let Some(r) = runs.as_ref().filter(|r| r.runs_in_flight > 0) {
        const NAMED: usize = 20;
        let mut roots =
            r.in_flight_roots.iter().take(NAMED).cloned().collect::<Vec<_>>().join(", ");
        if r.in_flight_roots.len() > NAMED {
            roots.push_str(&format!(" and {} more", r.in_flight_roots.len() - NAMED));
        }
        let line = format!(
            "{} durable runs attributed to this person have not ended and were left whole, \
             with everything the run store holds for them. Cancel these root runs with POST \
             /v1/api/runs/{{run_id}}/cancel, or let them end, then send this request again: \
             {roots}.",
            r.runs_in_flight
        );
        advisory.note = Some(match advisory.note.take() {
            Some(note) => format!("{note} {line}"),
            None => line,
        });
        advisory.unresolved = true;
    }

    // A subject with nothing stored is a 200 carrying zeroes, not a 404.
    //
    // Two reasons, and the second is the one that matters. First, the caller
    // asked for a state ("nothing about this person is stored"), and that state
    // now holds; an error would say the request failed when it did exactly what
    // was asked. Second, a 404 would answer a question nobody is entitled to
    // ask: it would tell an admin key for one project whether a person exists
    // anywhere it can see, turning this endpoint into an existence oracle over
    // the subject id. Zeroes are the same answer for "never seen" and "already
    // erased", which is the answer that leaks nothing.
    //
    // What the zero must never do is stand on its own. The note carries the
    // reason it might be zero, and the lines below put the same fact in the
    // operator's log, so a zero cannot pass as "no data about this person"
    // in either place.
    //
    // The level is the difference between a caveat and a call for help. A note
    // saying a captured dataset case was emptied went out at WARN, so a
    // completely successful erasure logged a warning, and an operator who sees
    // that on every run stops reading the ones that matter.
    match advisory.note.as_deref() {
        Some(note) if advisory.unresolved => {
            tracing::warn!(project = %project, note, "an erasure could not answer in full");
        }
        Some(note) => tracing::info!(project = %project, note, "an erasure carries a caveat"),
        None => {}
    }
    tracing::info!(
        project = %project,
        traces = erased.result.traces,
        spans = erased.result.spans,
        logs = erased.result.logs,
        deleted = erased.result.deleted,
        dataset_items = erased.dataset_items,
        aspect_notes = erased.aspect_notes,
        unmatchable = erased.identifiers.unmatchable,
        runs = runs.as_ref().map(|r| r.runs_erased),
        runs_in_flight = runs.as_ref().map(|r| r.runs_in_flight),
        "subject erased"
    );
    // The generated wire type, every field of it. This was a hand-written
    // wrapper that flattened the counts and added six fields the spec did not
    // describe, so every SDK's type for this reply was missing half of it.
    Ok(Json(ErasureResult {
        aspect_notes_erased: Some(erased.aspect_notes),
        dataset_items_erased: Some(erased.dataset_items),
        datasets: erased.datasets,
        deletes_pending: Some(erased.deferred),
        unmatchable_identifiers: Some(erased.identifiers.unmatchable),
        note: advisory.note,
        ..erased.result
    }))
}

// ------------------------------------------------------- capabilities ---

/// The backend's own statement about itself, as the wire type.
///
/// Written out field by field rather than re-serialising the store's struct,
/// because the two are separately declared: `mcpd_store::StoreCapabilities` is
/// what a backend fills in, and `mcpd_core::StoreCapabilities` is what the spec
/// publishes to six SDKs. A field added to one and not the other would
/// otherwise vanish silently on the way out.
/// `the_capability_set_publishes_every_field_the_store_declares` is what makes
/// that impossible: it walks `StoreCapabilities::fields()`, the store's own
/// list, and fails on a name the response does not carry.
fn wire_capabilities(caps: &mcpd_store::StoreCapabilities) -> StoreCapabilities {
    StoreCapabilities {
        backend: caps.backend.to_owned(),
        text_search: match caps.text_search {
            mcpd_store::TextSearch::None => TextSearch::None,
            mcpd_store::TextSearch::Substring => TextSearch::Substring,
            mcpd_store::TextSearch::Words => TextSearch::Words,
        },
        unicode_case_folding: caps.unicode_case_folding,
        immediate_deletes: caps.immediate_deletes,
        discovery: caps.discovery,
        discovery_label_values: caps.discovery_label_values,
        delta_polling: caps.delta_polling,
        per_signal_retention: caps.per_signal_retention,
        declarative_retention: caps.declarative_retention,
        partitioning: match caps.partitioning {
            mcpd_store::Partitioning::None => Partitioning::None,
            mcpd_store::Partitioning::Day => Partitioning::Day,
            mcpd_store::Partitioning::Month => Partitioning::Month,
        },
        partition_drop_retention: caps.partition_drop_retention,
        idempotent_schema_init: caps.idempotent_schema_init,
        theme_analysis: caps.theme_analysis,
    }
}

/// Refuse an endpoint whose capability this backend does not have.
///
/// The refusal is raised at the door rather than left to the store, and the two
/// are the same 501 with the same capability name because `error.rs` maps
/// `mcpd_store::Error::Unsupported` onto the same variant. Checking here buys
/// the caller two things a refusal from inside the store cannot: the door
/// answers before a project is resolved or a query is built, and every
/// capability reads the same way whether or not the backend happens to refuse
/// with a sentence of its own.
fn require(state: &AppState, capability: &'static str, have: bool, reason: &str) -> ApiResult<()> {
    if have {
        return Ok(());
    }
    Err(ApiError::NotImplemented {
        capability: capability.to_owned(),
        message: format!(
            "this deployment stores signals in {backend}, which does not support \
             {capability}: {reason}. GET /v1/api/capabilities lists what it does support.",
            backend = state.store.name(),
        ),
    })
}

/// `GET /v1/api/capabilities`
///
/// Not audited, and not project-scoped. It reads no stored signal: the answer
/// is a property of the deployment, identical for every project, and a caller
/// that cannot learn it is a caller who finds the same fact out from an empty
/// list instead.
pub async fn get_capabilities(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ApiResult<Json<StoreCapabilities>> {
    authorize(&state, &headers, Scope::Read)?;
    Ok(Json(wire_capabilities(&state.store.capabilities())))
}

// --------------------------------------------------------- discovery ---

/// Every parameter `GET /v1/api/discovery` accepts.
pub const DISCOVERY_PARAMS: &[&str] = &["project", "facet", "key", "prefix", "limit"];

/// Every parameter `GET /v1/api/deltas` accepts.
pub const DELTA_PARAMS: &[&str] = &["project", "signal", "cursor", "since", "limit"];

/// The wire facet, from the one the store maintains its lists under.
///
/// Two declarations of the same eight names, for the same reason as
/// [`wire_capabilities`]: one belongs to the storage crate and one is published
/// by the spec. `tests/discovery.rs` walks `Facet::ALL` and parses each name
/// back through the store's own parser, so a ninth added to one and not the
/// other fails rather than answering an empty list.
fn store_facet(facet: Facet) -> mcpd_store::Facet {
    match facet {
        Facet::EntityKind => mcpd_store::Facet::EntityKind,
        Facet::EntityName => mcpd_store::Facet::EntityName,
        Facet::ServiceName => mcpd_store::Facet::ServiceName,
        Facet::Environment => mcpd_store::Facet::Environment,
        Facet::Tag => mcpd_store::Facet::Tag,
        Facet::MetricName => mcpd_store::Facet::MetricName,
        Facet::MetricLabelKey => mcpd_store::Facet::MetricLabelKey,
        Facet::MetricLabelValue => mcpd_store::Facet::MetricLabelValue,
    }
}

fn facet_names() -> String {
    Facet::ALL.iter().map(|f| f.as_str()).collect::<Vec<_>>().join(", ")
}

/// `GET /v1/api/discovery`
///
/// Not audited. It returns no payload: a facet value is a label, an
/// environment name or a tag, which is the shape of the data rather than the
/// data. Auditing every keystroke in a filter box would also bury the rows that
/// record somebody reading a prompt, which is what the log is for.
pub async fn discover(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(p): Query<Params>,
) -> ApiResult<Json<DiscoveryResponse>> {
    let project = project(&p, &state, &headers);
    authorize_project(&state, &headers, Scope::Read, &project)?;
    // Before the parameters are read, and that order is deliberate. On a
    // backend that keeps no distinct-value list this door never answers,
    // whatever is in the query string, so it says so once instead of grading
    // the caller's spelling first.
    let caps = state.store.capabilities();
    require(
        &state,
        "discovery",
        caps.discovery,
        "it keeps no distinct-value list, so there is nothing to read a filter box from without \
         scanning every signal in the retention window",
    )?;
    only(&p, "GET /v1/api/discovery", DISCOVERY_PARAMS, &[])?;
    state.metrics.query_requests.fetch_add(1, AtomicOrdering::Relaxed);

    let facet: Facet =
        parse_enum(&p, "facet", &facet_names())?.ok_or_else(|| ApiError::BadParam {
            field: "facet".to_owned(),
            message: format!("name the dimension to list: {}", facet_names()),
        })?;
    if facet == Facet::MetricLabelValue {
        require(
            &state,
            "discovery_label_values",
            caps.discovery_label_values,
            "it keeps the label keys and not their values, because a key set is small and \
             bounded and a value set is neither",
        )?;
    }
    let key = get(&p, "key").map(str::to_owned);
    // Refused here as well as in the store, because the message can say what to
    // do about it. Asking for every label's values at once is not a filter box,
    // it is the whole label index.
    if facet == Facet::MetricLabelValue && key.is_none() {
        return Err(ApiError::BadParam {
            field: "key".to_owned(),
            message: "metric_label_value needs a key: name the one label whose values you want, \
                      or the answer is every label's values mixed together"
                .to_owned(),
        });
    }

    let q = mcpd_store::DiscoveryQuery {
        project,
        facet: store_facet(facet),
        key: key.clone(),
        prefix: get(&p, "prefix").map(str::to_owned),
        limit: parse::<u32>(&p, "limit")?.unwrap_or(100),
    };
    let found = state.store.discover(&q).await?;
    Ok(Json(DiscoveryResponse {
        facet,
        // Echoed back only where it meant something, so a caller cannot read a
        // key into an answer that ignored it.
        key: (facet == Facet::MetricLabelValue).then_some(key).flatten(),
        values: found
            .into_iter()
            .map(|d| DiscoveredValue { value: d.value, last_seen: d.last_seen })
            .collect(),
    }))
}

/// `GET /v1/api/deltas`
pub async fn poll_deltas(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(p): Query<Params>,
) -> ApiResult<Json<DeltaResponse>> {
    let project = project(&p, &state, &headers);
    authorize_project(&state, &headers, Scope::Read, &project)?;
    require(
        &state,
        "delta_polling",
        state.store.capabilities().delta_polling,
        "it has no arrival-ordered read, so a list refreshes by running its query again",
    )?;
    only(&p, "GET /v1/api/deltas", DELTA_PARAMS, &[])?;
    state.metrics.query_requests.fetch_add(1, AtomicOrdering::Relaxed);

    let signal: DeltaSignal =
        parse_enum(&p, "signal", "traces, spans, logs")?.unwrap_or(DeltaSignal::Traces);
    let q = mcpd_store::DeltaQuery {
        project,
        signal: match signal {
            DeltaSignal::Traces => mcpd_store::DeltaSignal::Traces,
            DeltaSignal::Spans => mcpd_store::DeltaSignal::Spans,
            DeltaSignal::Logs => mcpd_store::DeltaSignal::Logs,
        },
        cursor: get(&p, "cursor").map(str::to_owned),
        since: parse_time(&p, "since")?,
        limit: parse::<u32>(&p, "limit")?.unwrap_or(100),
    };
    let delta = state.store.poll(&q).await?;

    // Sorted into the array its signal names, and the other two stay absent.
    // The store hands back one enum per row so the trait can stay object safe,
    // and a caller that asked for logs should not have to read a tag on every
    // row to find out it got logs.
    let mut out = DeltaResponse {
        signal,
        cursor: delta.cursor,
        has_more: delta.has_more,
        traces: None,
        spans: None,
        logs: None,
    };
    match signal {
        DeltaSignal::Traces => {
            out.traces = Some(
                delta
                    .items
                    .into_iter()
                    .filter_map(|i| match i {
                        mcpd_store::PolledSignal::Trace(t) => Some(*t),
                        _ => None,
                    })
                    .collect(),
            );
        }
        DeltaSignal::Spans => {
            out.spans = Some(
                delta
                    .items
                    .into_iter()
                    .filter_map(|i| match i {
                        mcpd_store::PolledSignal::Span(s) => Some(*s),
                        _ => None,
                    })
                    .collect(),
            );
        }
        DeltaSignal::Logs => {
            out.logs = Some(
                delta
                    .items
                    .into_iter()
                    .filter_map(|i| match i {
                        mcpd_store::PolledSignal::Log(l) => Some(*l),
                        _ => None,
                    })
                    .collect(),
            );
        }
    }
    Ok(Json(out))
}

// ------------------------------------------------------- intelligence ---
//
// The nine `Intelligence` endpoints. They read what the analysis pass wrote and
// never recompute: a pass writes a snapshot on its own timer, and every route
// here is a lookup, so an operator refreshing a screen re-reads the same
// snapshot rather than a fresh clustering.
//
// Three of them return prose a user wrote. Those take the read scope like every
// other payload route and are recorded in the audit ledger, because a note is
// the shortest sentence in this daemon that says what somebody asked for.

/// Every parameter `GET /v1/api/aspects/entities` accepts.
pub const ENTITY_INDEX_PARAMS: &[&str] = &["project", "entity_kind", "limit", "cursor"];

/// Every parameter `GET /v1/api/themes/flow` accepts.
pub const THEME_FLOW_PARAMS: &[&str] = &[
    "project",
    "entity_kind",
    "entity_name",
    "aspects",
    "snapshot_id",
    "theme_limit_per_stage",
    "filter",
];

/// Query pairs as they arrived, so a parameter can appear more than once.
///
/// `Params` is a map and a map keeps one value per key, which is right for
/// every other endpoint here and wrong for two of these: `aspects` decides how
/// many stages a flow has and `filter` is documented as being repeated up to
/// four times. Collapsing either to its last value would answer a different
/// question from the one that was asked, silently.
type Pairs = Vec<(String, String)>;

/// Every value given for one key, in the order they arrived.
///
/// Accepts both spellings: repeated (`filter=a:1&filter=b:2`) and comma
/// separated (`aspects=goal,outcome`). The reference documents the first for
/// `filter` and the second reads naturally for `aspects`, and refusing either
/// would be a distinction nobody can see from the outside.
fn every(pairs: &Pairs, key: &str) -> Vec<String> {
    pairs
        .iter()
        .filter(|(k, _)| k == key)
        .flat_map(|(_, v)| v.split(','))
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .collect()
}

/// The entity a route is about: the pair of a kind and a name.
fn entity(p: &Params) -> ApiResult<(EntityKind, String)> {
    let kind: EntityKind =
        parse_enum(p, "entity_kind", &kind_names())?.ok_or_else(|| ApiError::BadParam {
            field: "entity_kind".to_owned(),
            message: format!("name the kind of entity to read; one of: {}", kind_names()),
        })?;
    let name = get(p, "entity_name")
        .ok_or_else(|| ApiError::BadParam {
            field: "entity_name".to_owned(),
            message: "name the entity to read. An entity is the pair of a kind and a name, \
                      which is the identity spans already carry"
                .to_owned(),
        })?
        .to_owned();
    Ok((kind, name))
}

/// Refuse when this deployment's backend does not hold the analysis tables.
///
/// Raised at the door, so every one of these routes refuses the same way and a
/// caller reads `GET /v1/api/capabilities` once instead of learning the
/// difference between two backends one refusal at a time.
fn require_themes(state: &AppState) -> ApiResult<()> {
    require(
        state,
        "theme_analysis",
        state.store.capabilities().theme_analysis,
        "it does not hold aspect notes, snapshots or themes",
    )
}

/// `GET /v1/api/aspects`
///
/// Not gated on `theme_analysis`. The catalogue is a fact about the daemon and
/// not about its storage: a client has to be able to learn what the four
/// aspects are, and what bounds a pass runs under, on a deployment whose
/// backend cannot hold a theme.
pub async fn list_aspects(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(p): Query<Params>,
) -> ApiResult<Json<mcpd_core::AspectCatalogue>> {
    let project = project(&p, &state, &headers);
    authorize_project(&state, &headers, Scope::Read, &project)?;
    Ok(Json(mcpd_core::AspectCatalogue {
        aspects: crate::aspects::catalogue(&project, &state.config.aspects, state.started_at),
        limits: crate::aspects::limits(&state.config.aspects),
    }))
}

/// One entity's roll-up, folded out of the per-aspect counts.
struct EntityRow {
    kind: EntityKind,
    name: String,
    notes: u64,
    trace_count: u64,
    last_note_at: OffsetDateTime,
    per_aspect: std::collections::BTreeMap<String, (u64, u64, OffsetDateTime)>,
}

/// Fold the store's per-aspect roll-up into one row per entity.
///
/// `trace_count` is the largest of the per-aspect trace counts and never their
/// sum. One trace usually carries a note along every enabled aspect, so adding
/// them would report four times the runs a screen can actually open.
fn fold_rollup(rows: Vec<mcpd_store::AspectNoteRollup>) -> Vec<EntityRow> {
    let mut by: std::collections::BTreeMap<(String, String), EntityRow> =
        std::collections::BTreeMap::new();
    for r in rows {
        let key = (r.entity_kind.as_str().to_owned(), r.entity_name.clone());
        let row = by.entry(key).or_insert_with(|| EntityRow {
            kind: r.entity_kind,
            name: r.entity_name.clone(),
            notes: 0,
            trace_count: 0,
            last_note_at: OffsetDateTime::UNIX_EPOCH,
            per_aspect: std::collections::BTreeMap::new(),
        });
        row.notes += r.note_count;
        row.trace_count = row.trace_count.max(r.trace_count);
        row.last_note_at = row.last_note_at.max(r.last_note_at);
        row.per_aspect.insert(r.aspect, (r.note_count, r.trace_count, r.last_note_at));
    }
    let mut out: Vec<EntityRow> = by.into_values().collect();
    // Most recently written to first, the entity name breaking a tie, so a
    // page boundary falls in the same place on every read.
    out.sort_by(|a, b| b.last_note_at.cmp(&a.last_note_at).then_with(|| a.name.cmp(&b.name)));
    out
}

/// Which aspects the newest snapshot for one entity covers.
async fn covered_aspects(
    state: &AppState,
    project: &str,
    kind: EntityKind,
    name: &str,
) -> ApiResult<Option<mcpd_core::ThemeSnapshot>> {
    let page = state
        .store
        .list_theme_snapshots(&mcpd_store::SnapshotQuery {
            project: project.to_owned(),
            entity: Some((kind, name.to_owned())),
            order: Order::Desc,
            limit: 1,
            ..mcpd_store::SnapshotQuery::default()
        })
        .await?;
    Ok(page.items.into_iter().next())
}

/// `GET /v1/api/aspects/entities`
pub async fn list_aspect_entities(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(p): Query<Params>,
) -> ApiResult<Json<mcpd_core::EntityIndexResponse>> {
    let project = project(&p, &state, &headers);
    authorize_project(&state, &headers, Scope::Read, &project)?;
    require_themes(&state)?;

    let kind: Option<EntityKind> = parse_enum(&p, "entity_kind", &kind_names())?;
    let limit = parse::<u32>(&p, "limit")?.unwrap_or(50).clamp(1, 200);
    let cursor = get(&p, "cursor").map(str::to_owned);

    let rows = fold_rollup(
        state
            .store
            .aspect_note_rollup(&mcpd_store::AspectRollupQuery {
                project: project.clone(),
                entity_kind: kind,
                entity: None,
                max_groups: crate::aspects::MAX_ASPECTS.saturating_mul(
                    u32::try_from(crate::aspects::ENTITIES_PER_PASS).unwrap_or(200),
                ),
            })
            .await?,
    );

    // Paged here rather than in SQL. The roll-up is a group-by whose result is
    // bounded by the entities a project has multiplied by the aspects it
    // defines, and the fold above turns several rows into one, so a page
    // boundary can only be decided after the fold.
    // The cursor carries the sort key of the last row handed back, which is the
    // pair the fold above sorts on. Carrying the name alone would be a cursor
    // over half a sort key, and a walk that resumed from it would repeat or
    // skip every row that shares a timestamp with it.
    let start = match &cursor {
        None => 0,
        Some(c) => {
            let (at, after) = decode_index_cursor(c)?;
            rows.iter()
                .position(|r| r.last_note_at < at || (r.last_note_at == at && r.name > after))
                .unwrap_or(rows.len())
        }
    };
    let page: Vec<&EntityRow> = rows.iter().skip(start).take(limit as usize).collect();
    let has_more = start + page.len() < rows.len();

    let enabled = crate::aspects::enabled_names(&state.config.aspects);
    let mut entities = Vec::with_capacity(page.len());
    for row in &page {
        let snapshot = covered_aspects(&state, &project, row.kind, &row.name).await?;
        let covered = snapshot.as_ref().map(|s| s.aspects.clone()).unwrap_or_default();
        let mut ready_aspects = 0u32;
        let mut states = Vec::with_capacity(enabled.len());
        for aspect in &enabled {
            let notes = row.per_aspect.get(aspect).map_or(0, |(n, _, _)| *n);
            let is_ready = covered.iter().any(|c| c == aspect);
            if is_ready {
                ready_aspects += 1;
            }
            states.push(crate::aspects::readiness(notes, is_ready));
        }
        // The least advanced of them, and never the best case: a reader who
        // opens a row that says `ready` and finds three empty columns has been
        // told something untrue.
        let readiness = crate::aspects::overall(states);
        entities.push(mcpd_core::EntityIndexRow {
            entity_kind: row.kind,
            entity_name: row.name.clone(),
            trace_count: row.trace_count,
            note_count: row.notes,
            enabled_aspects: u32::try_from(enabled.len()).unwrap_or(u32::MAX),
            ready_aspects,
            readiness,
            last_note_at: row.last_note_at,
            latest_snapshot_at: snapshot.map(|s| s.cutoff_at),
        });
    }

    let next_cursor = has_more
        .then(|| page.last().map(|r| encode_index_cursor(r.last_note_at, &r.name)))
        .flatten();
    Ok(Json(mcpd_core::EntityIndexResponse {
        entities,
        page: Page { next_cursor: next_cursor.clone(), has_more, total: Some(rows.len() as u64) },
    }))
}

/// A cursor over a list this daemon folded in memory.
///
/// Opaque by construction, like every other cursor here: a caller must never
/// build or read one, so the encoding can change without breaking anybody.
fn encode_index_cursor(at: OffsetDateTime, after: &str) -> String {
    hex::encode(format!("{}|{after}", at.unix_timestamp_nanos()))
}

fn decode_index_cursor(cursor: &str) -> ApiResult<(OffsetDateTime, String)> {
    let bad = || ApiError::BadParam {
        field: "cursor".to_owned(),
        message: "that cursor was not one this endpoint issued".to_owned(),
    };
    let raw = hex::decode(cursor).ok().and_then(|b| String::from_utf8(b).ok()).ok_or_else(bad)?;
    let (nanos, name) = raw.split_once('|').ok_or_else(bad)?;
    let nanos: i128 = nanos.parse().map_err(|_| bad())?;
    let at = OffsetDateTime::from_unix_timestamp_nanos(nanos).map_err(|_| bad())?;
    Ok((at, name.to_owned()))
}

/// `GET /v1/api/aspects/progress`
pub async fn get_aspect_progress(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(p): Query<Params>,
) -> ApiResult<Json<mcpd_core::AspectProgress>> {
    let project = project(&p, &state, &headers);
    authorize_project(&state, &headers, Scope::Read, &project)?;
    require_themes(&state)?;
    let (kind, name) = entity(&p)?;

    let rows = fold_rollup(
        state
            .store
            .aspect_note_rollup(&mcpd_store::AspectRollupQuery {
                project: project.clone(),
                entity_kind: None,
                entity: Some((kind, name.clone())),
                max_groups: crate::aspects::MAX_ASPECTS,
            })
            .await?,
    );
    let snapshot = covered_aspects(&state, &project, kind, &name).await?;

    // Nothing recorded and no pass ever run is a 404 and not an empty row, and
    // that distinction is the point of this route. "No notes at all" means
    // nothing is wired; `collecting` means notes are arriving and the floor has
    // not been reached. A zeroed row would make the two look identical, which
    // is exactly the confusion the endpoint exists to remove.
    if rows.is_empty() && snapshot.is_none() {
        return Err(ApiError::NotFound(format!(
            "nothing has recorded an aspect note about the {} {name:?} in the project \
             {project:?}, so there is no progress to report",
            kind.as_str()
        )));
    }
    let row = rows.into_iter().next();
    let covered = snapshot.as_ref().map(|s| s.aspects.clone()).unwrap_or_default();

    let mut aspects = Vec::new();
    let mut states = Vec::new();
    for aspect in crate::aspects::enabled_names(&state.config.aspects) {
        let counts = row.as_ref().and_then(|r| r.per_aspect.get(&aspect)).copied();
        let (notes, traces, last) = counts.map_or((0, 0, None), |(n, t, at)| (n, t, Some(at)));
        let is_ready = covered.contains(&aspect);
        let state_of = crate::aspects::readiness(notes, is_ready);
        states.push(state_of);
        aspects.push(mcpd_core::AspectProgressRow {
            aspect,
            readiness: state_of,
            note_count: notes,
            trace_count: traces,
            notes_needed: crate::aspects::notes_needed(notes),
            last_note_at: last,
        });
    }

    Ok(Json(mcpd_core::AspectProgress {
        entity_kind: kind,
        entity_name: name,
        // The least advanced of the enabled aspects, and `collecting` when a
        // deployment has switched them all off.
        readiness: crate::aspects::overall(states),
        trace_count: row.as_ref().map_or(0, |r| r.trace_count),
        aspects,
        latest_snapshot: snapshot,
    }))
}

/// `GET /v1/api/traces/{trace_id}/aspects`
///
/// The project is optional. A trace id crosses service boundaries, so two
/// projects can each hold a run under the same id; `project` names the one to
/// read. Without it the projects this key reaches are walked in order and the
/// first that holds the trace answers, which is what makes that 404 mean "no
/// trace with that id in a project this key reaches".
pub async fn list_trace_aspect_notes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(trace_id): Path<String>,
    Query(p): Query<Params>,
) -> ApiResult<Json<mcpd_core::TraceAspectNotes>> {
    only(&p, "GET /v1/api/traces/{trace_id}/aspects", &["project"], &[])?;
    let named = get(&p, "project").map(str::to_owned);
    let decision = match &named {
        Some(project) => authorize_project(&state, &headers, Scope::Read, project),
        None => authorize(&state, &headers, Scope::Read),
    };
    // The note text is payload, so every answer is recorded, served or
    // refused, through `audited`. A refusal used to return before any row was
    // written, and a served read was recorded only when a note was found, so a
    // refused attempt, a trace with no notes and a trace that did not exist
    // were three answers the log never saw.
    //
    // The project is found before the row is written, because the row names
    // the project that answered. A refusal, and a trace in no project the key
    // reaches, name the default project: the request itself names none.
    let found = match &decision {
        Ok(grant) => {
            require_themes(&state)?;
            let reachable: Vec<String> = match (&named, grant.projects()) {
                (Some(project), _) => vec![project.clone()],
                (None, Some(only)) => only.iter().cloned().collect(),
                (None, None) => state.store.list_projects().await?,
            };
            Some(find_trace_notes(&state, &reachable, &trace_id).await?)
        }
        Err(_) => None,
    };
    let project = match &found {
        Some(TraceNotes::Notes(project, _) | TraceNotes::Empty(project)) => project.clone(),
        Some(TraceNotes::Missing) | None => {
            named.clone().unwrap_or_else(|| state.config.default_project.clone())
        }
    };
    audited(
        &state,
        &headers,
        peer.0,
        decision,
        "read_trace_aspect_notes",
        &project,
        Some(&trace_id),
    )
    .await?;

    match found {
        Some(TraceNotes::Notes(_, notes)) => {
            let mut notes: Vec<mcpd_core::AspectNote> = notes.into_iter().map(|n| n.note).collect();
            // Catalogue order, which is the order every screen shows the
            // aspects in. The store returns them by the time they were written,
            // which is the order a worker happened to run in and means nothing
            // to a reader.
            let order = crate::aspects::enabled_names(&state.config.aspects);
            notes.sort_by_key(|n| order.iter().position(|a| *a == n.aspect).unwrap_or(usize::MAX));
            Ok(Json(mcpd_core::TraceAspectNotes { trace_id, notes }))
        }
        Some(TraceNotes::Empty(_)) => {
            Ok(Json(mcpd_core::TraceAspectNotes { trace_id, notes: Vec::new() }))
        }
        Some(TraceNotes::Missing) | None => {
            Err(ApiError::NotFound(format!("no trace {trace_id:?} in a project this key reaches")))
        }
    }
}

/// What the projects a key reaches hold about one trace.
enum TraceNotes {
    /// The first project, in the order walked, with notes about the trace.
    Notes(String, Vec<mcpd_store::StoredNote>),
    /// A project that holds the trace and no note about it, which is the
    /// ordinary state of a recent run.
    Empty(String),
    /// No project this key reaches holds the trace.
    Missing,
}

/// Walk the given projects, in order, for one trace's notes.
async fn find_trace_notes(
    state: &AppState,
    reachable: &[String],
    trace_id: &str,
) -> ApiResult<TraceNotes> {
    for project in reachable {
        let notes = state
            .store
            .list_aspect_notes(&mcpd_store::AspectNoteQuery {
                project: project.clone(),
                trace_id: Some(trace_id.to_owned()),
                order: Order::Asc,
                limit: crate::aspects::MAX_ASPECTS,
                ..mcpd_store::AspectNoteQuery::default()
            })
            .await?;
        if !notes.items.is_empty() {
            return Ok(TraceNotes::Notes(project.clone(), notes.items));
        }
    }
    // No notes anywhere this key reaches. The trace may still exist and carry
    // none, so the trace itself decides between an empty answer and a 404.
    for project in reachable {
        if state.store.get_trace(project, trace_id, &Include::default()).await.is_ok() {
            return Ok(TraceNotes::Empty(project.clone()));
        }
    }
    Ok(TraceNotes::Missing)
}

/// Whether this entity is one the analysis knows about at all.
///
/// A 404 on the theme routes means the entity has never been written about, and
/// that is a different mistake from asking for a window it has no snapshot in.
/// The check is the same on all three backends: notes or snapshots, and nothing
/// backend-specific decides it.
async fn entity_is_known(
    state: &AppState,
    project: &str,
    kind: EntityKind,
    name: &str,
) -> ApiResult<bool> {
    let notes = state
        .store
        .list_aspect_notes(&mcpd_store::AspectNoteQuery {
            project: project.to_owned(),
            entity: Some((kind, name.to_owned())),
            limit: 1,
            ..mcpd_store::AspectNoteQuery::default()
        })
        .await?;
    if !notes.items.is_empty() {
        return Ok(true);
    }
    Ok(covered_aspects(state, project, kind, name).await?.is_some())
}

/// `GET /v1/api/themes/snapshots`
pub async fn list_theme_snapshots(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(p): Query<Params>,
    Query(pairs): Query<Pairs>,
) -> ApiResult<Json<mcpd_core::SnapshotListResponse>> {
    let project = project(&p, &state, &headers);
    authorize_project(&state, &headers, Scope::Read, &project)?;
    require_themes(&state)?;
    let (kind, name) = entity(&p)?;
    if !entity_is_known(&state, &project, kind, &name).await? {
        return Err(unknown_entity(kind, &name, &project));
    }

    let aspects = every(&pairs, "aspects");
    let from = parse_time(&p, "from")?;
    let to = parse_time(&p, "to")?;
    let limit = parse::<u32>(&p, "limit")?.unwrap_or(24).clamp(1, 100);
    let presentation = get(&p, "presentation").unwrap_or("inventory");
    let base = mcpd_store::SnapshotQuery {
        project: project.clone(),
        entity: Some((kind, name)),
        aspects: aspects.clone(),
        from,
        to,
        // Oldest first, so a timeline can be drawn without reordering anything.
        order: Order::Asc,
        limit,
        cursor: get(&p, "cursor").map(str::to_owned),
        ..mcpd_store::SnapshotQuery::default()
    };

    match presentation {
        "inventory" => {
            let page = state.store.list_theme_snapshots(&base).await?;
            let total = page.total.unwrap_or(page.items.len() as u64);
            Ok(Json(mcpd_core::SnapshotListResponse {
                snapshots: page
                    .items
                    .iter()
                    .map(|s| mcpd_core::SnapshotLandmark {
                        snapshot: s.clone(),
                        // An inventory chooses nothing, so it has nothing to
                        // report about why a row is in it.
                        reason: None,
                    })
                    .collect(),
                total,
                page: page_of(&page),
            }))
        }
        "landmarks" => landmarks(&state, base, limit).await.map(Json),
        other => Err(ApiError::BadParam {
            field: "presentation".to_owned(),
            message: format!(
                "{other:?} is not a presentation; use inventory for consecutive snapshots or \
                 landmarks for a time balanced selection"
            ),
        }),
    }
}

/// A bounded, time balanced selection across the whole range.
///
/// The oldest and the newest snapshot in the range are always in it, so a
/// timeline starts where the range does and ends at the current state, and the
/// rest are chosen by time rather than by ordinal: an hour of frequent passes
/// must not crowd out a quiet week.
///
/// It is `limit` single-row reads and never a read of the range. A range can
/// hold thousands of passes and this answers with at most a hundred points, so
/// reading them all to throw nearly all of them away would make the cost of
/// this endpoint the size of the history rather than the size of the answer.
async fn landmarks(
    state: &AppState,
    base: mcpd_store::SnapshotQuery,
    limit: u32,
) -> ApiResult<mcpd_core::SnapshotListResponse> {
    let one = |order: Order, from: Option<OffsetDateTime>| mcpd_store::SnapshotQuery {
        order,
        limit: 1,
        cursor: None,
        from: from.or(base.from),
        ..base.clone()
    };

    let oldest = state.store.list_theme_snapshots(&one(Order::Asc, None)).await?;
    let newest = state.store.list_theme_snapshots(&one(Order::Desc, None)).await?;
    let total = oldest.total.unwrap_or(0);
    let (Some(first), Some(last)) = (oldest.items.first(), newest.items.first()) else {
        return Ok(mcpd_core::SnapshotListResponse {
            snapshots: Vec::new(),
            total,
            page: Page { next_cursor: None, has_more: false, total: Some(total) },
        });
    };

    // Keyed by cutoff and id so the selection is ordered oldest first and a
    // snapshot chosen twice, which happens when the range is shorter than the
    // number of points asked for, appears once.
    let mut chosen: std::collections::BTreeMap<
        (OffsetDateTime, String),
        mcpd_core::SnapshotLandmark,
    > = std::collections::BTreeMap::new();
    let mut put = |s: &mcpd_core::ThemeSnapshot, reason: mcpd_core::SnapshotLandmarkReason| {
        chosen.entry((s.cutoff_at, s.snapshot_id.clone())).or_insert_with(|| {
            mcpd_core::SnapshotLandmark { snapshot: s.clone(), reason: Some(reason) }
        });
    };
    put(first, mcpd_core::SnapshotLandmarkReason::RangeStart);
    put(last, mcpd_core::SnapshotLandmarkReason::RangeEnd);

    // The interior points, at evenly spaced times between the two ends. The
    // snapshot taken is the first at or after each target, which is what makes
    // the spacing a property of time and not of how many passes ran.
    let span = (last.cutoff_at - first.cutoff_at).whole_seconds().max(0);
    let interior = i64::from(limit).saturating_sub(2).max(0);
    for step in 1..=interior {
        let at = first.cutoff_at + time::Duration::seconds(span * step / (interior + 1));
        let page = state.store.list_theme_snapshots(&one(Order::Asc, Some(at))).await?;
        if let Some(s) = page.items.first() {
            put(s, mcpd_core::SnapshotLandmarkReason::TimeSample);
        }
    }

    Ok(mcpd_core::SnapshotListResponse {
        snapshots: chosen.into_values().collect(),
        total,
        // A landmark selection is the whole answer for the range it was asked
        // about, so there is no next page to walk to. `total` is what says how
        // many passes the selection stands in for.
        page: Page { next_cursor: None, has_more: false, total: Some(total) },
    })
}

/// The 404 every theme route gives for an entity nothing has written about.
fn unknown_entity(kind: EntityKind, name: &str, project: &str) -> ApiError {
    ApiError::NotFound(format!(
        "no {} {name:?} with any aspect note or theme in the project {project:?}",
        kind.as_str()
    ))
}

/// Read one snapshot by id, or say it is not this entity's.
async fn snapshot_by_id(
    state: &AppState,
    project: &str,
    kind: EntityKind,
    name: &str,
    snapshot_id: &str,
) -> ApiResult<mcpd_core::ThemeSnapshot> {
    let page = state
        .store
        .list_theme_snapshots(&mcpd_store::SnapshotQuery {
            project: project.to_owned(),
            snapshot_id: Some(snapshot_id.to_owned()),
            entity: Some((kind, name.to_owned())),
            limit: 1,
            ..mcpd_store::SnapshotQuery::default()
        })
        .await?;
    page.items.into_iter().next().ok_or_else(|| {
        ApiError::NotFound(format!(
            "no snapshot {snapshot_id:?} for the {} {name:?} in the project {project:?}",
            kind.as_str()
        ))
    })
}

/// Every membership of one snapshot for one aspect, to the pass ceiling.
async fn members_of(
    state: &AppState,
    project: &str,
    snapshot_id: &str,
    aspect: &str,
) -> ApiResult<Vec<mcpd_store::ThemeMember>> {
    let mut out = Vec::new();
    let mut cursor = None;
    loop {
        let page = state
            .store
            .list_theme_members(&mcpd_store::ThemeMemberQuery {
                project: project.to_owned(),
                snapshot_id: snapshot_id.to_owned(),
                aspect: Some(aspect.to_owned()),
                theme_id: None,
                limit: crate::aspects::PASS_PAGE,
                cursor,
            })
            .await?;
        out.extend(page.items);
        if out.len() as u64 >= crate::aspects::MAX_NOTES_PER_PASS {
            // The same ceiling the pass wrote under, so this can never read
            // more rows than one pass could have written for one aspect.
            return Ok(out);
        }
        match page.next_cursor {
            Some(c) => cursor = Some(c),
            None => return Ok(out),
        }
    }
}

/// Which theme each trace joined, for one aspect of one snapshot.
///
/// One entry per trace. A trace carries at most one note per aspect, so the
/// first membership is the only one; taking the first rather than the last
/// makes a duplicate harmless instead of arbitrary.
fn by_trace(members: &[mcpd_store::ThemeMember]) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for m in members {
        out.entry(m.trace_id.clone()).or_insert_with(|| m.theme_id.clone());
    }
    out
}

/// One `filter=aspect:theme_id`, parsed.
fn parse_filters(raw: &[String]) -> ApiResult<std::collections::BTreeMap<String, String>> {
    let mut out = std::collections::BTreeMap::new();
    for f in raw {
        let (aspect, theme) = f.split_once(':').ok_or_else(|| ApiError::BadParam {
            field: "filter".to_owned(),
            message: format!("{f:?} is not a filter; write it as aspect:theme_id, or aspect:noise"),
        })?;
        if !crate::aspects::is_known(aspect) {
            return Err(ApiError::BadParam {
                field: "filter".to_owned(),
                message: format!("{aspect:?} is not an aspect this deployment has"),
            });
        }
        if out.insert(aspect.to_owned(), theme.to_owned()).is_some() {
            return Err(ApiError::BadParam {
                field: "filter".to_owned(),
                message: format!(
                    "{aspect:?} is filtered twice; one filter per aspect, because two themes \
                     of one aspect describe no trace at all"
                ),
            });
        }
    }
    if out.len() > 4 {
        return Err(ApiError::BadParam {
            field: "filter".to_owned(),
            message: "at most four filters, one per aspect".to_owned(),
        });
    }
    Ok(out)
}

/// `GET /v1/api/themes/flow`
pub async fn get_theme_flow(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Query(p): Query<Params>,
    Query(pairs): Query<Pairs>,
) -> ApiResult<Json<mcpd_core::ThemeFlow>> {
    let project = project(&p, &state, &headers);
    // Audited: every stage names its themes by their labels, and a label is the
    // text of one note, so the flow hands out payload as the examples route does.
    let (kind, name, stages, snapshot_id, limit, filters) = audited_valid(
        &state,
        &headers,
        peer.0,
        authorize_project(&state, &headers, Scope::Read, &project),
        || {
            require_themes(&state)?;
            let (kind, name) = entity(&p)?;
            let stages = every(&pairs, "aspects");
            if !(2..=4).contains(&stages.len()) {
                // Refused by name rather than answered with an empty flow: one
                // stage has no links in it, and an empty answer would read as
                // "these two aspects never co-occur" rather than as "you asked
                // for one stage".
                return Err(ApiError::BadParam {
                    field: "aspects".to_owned(),
                    message: format!(
                        "a flow has two to four stages and {} were named; the order is \
                         meaningful, because links only ever join neighbouring stages",
                        stages.len()
                    ),
                });
            }
            let snapshot_id = get(&p, "snapshot_id").ok_or_else(|| ApiError::BadParam {
                field: "snapshot_id".to_owned(),
                message: "name the snapshot to read; every count in the answer belongs to one \
                          window"
                    .to_owned(),
            })?;
            let limit = parse::<u32>(&p, "theme_limit_per_stage")?.unwrap_or(8).clamp(1, 100);
            let filters = parse_filters(&every(&pairs, "filter"))?;
            Ok((kind, name, stages, snapshot_id, limit, filters))
        },
        "get_theme_flow",
        &project,
        None,
    )
    .await?;

    let snapshot = snapshot_by_id(&state, &project, kind, &name, snapshot_id).await?;
    for aspect in stages.iter().chain(filters.keys()) {
        if !snapshot.aspects.iter().any(|a| a == aspect) {
            return Err(ApiError::NotFound(format!(
                "the snapshot {snapshot_id:?} does not cover the aspect {aspect:?}; it covers {}",
                snapshot.aspects.join(", ")
            )));
        }
    }

    // Read every aspect this answer needs once: the stages, and any aspect a
    // filter names that is not itself a stage.
    let mut wanted: Vec<String> = stages.clone();
    for a in filters.keys() {
        if !wanted.contains(a) {
            wanted.push(a.clone());
        }
    }
    let mut assignment: std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, String>,
    > = std::collections::BTreeMap::new();
    for aspect in &wanted {
        let members = members_of(&state, &project, &snapshot.snapshot_id, aspect).await?;
        assignment.insert(aspect.clone(), by_trace(&members));
    }

    // The traces that survive every filter. Recomputed here rather than left to
    // the caller, because only the server holds the assignments and a client
    // that had to fetch them would be fetching the whole snapshot to draw a
    // dozen nodes.
    let surviving = |trace: &str| -> bool {
        filters.iter().all(|(aspect, theme)| {
            assignment.get(aspect).and_then(|m| m.get(trace)).is_some_and(|t| t == theme)
        })
    };

    let labels = theme_labels(&state, &project, &snapshot.snapshot_id).await?;
    let mut out_stages = Vec::with_capacity(stages.len());
    // Which node each trace sits in, per stage, so the links below are a lookup
    // rather than a second pass over the memberships.
    let mut node_of: Vec<std::collections::BTreeMap<String, String>> = Vec::new();

    for aspect in &stages {
        let empty = std::collections::BTreeMap::new();
        let assigned = assignment.get(aspect).unwrap_or(&empty);
        let mut per_theme: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
            std::collections::BTreeMap::new();
        for (trace, theme) in assigned {
            if !surviving(trace) {
                continue;
            }
            per_theme.entry(theme.clone()).or_default().insert(trace.clone());
        }
        let stage_traces: u64 = per_theme.values().map(|t| t.len() as u64).sum();

        let noise = per_theme.remove(mcpd_store::NOISE_THEME_ID).unwrap_or_default();
        // Largest first, the theme id breaking a tie, so the fold below always
        // cuts the same themes into `other`.
        let mut ranked: Vec<(String, std::collections::BTreeSet<String>)> =
            per_theme.into_iter().collect();
        ranked.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));

        let mut nodes = Vec::new();
        let mut mine: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        let share = |n: usize| -> f64 {
            if stage_traces == 0 {
                0.0
            } else {
                n as f64 / stage_traces as f64
            }
        };
        let mut folded: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for (i, (theme_id, traces)) in ranked.into_iter().enumerate() {
            if i < limit as usize {
                let node_id = format!("{aspect}:{theme_id}");
                for t in &traces {
                    mine.insert(t.clone(), node_id.clone());
                }
                nodes.push(mcpd_core::ThemeNode {
                    node_id,
                    kind: mcpd_core::ThemeNodeKind::Theme,
                    theme_id: Some(theme_id.clone()),
                    label: labels
                        .get(&(aspect.clone(), theme_id))
                        .cloned()
                        .unwrap_or_else(|| "an unnamed theme".to_owned()),
                    trace_count: traces.len() as u64,
                    coverage: share(traces.len()),
                });
            } else {
                folded.extend(traces);
            }
        }
        // Noise and overflow last, and both after the themes, so a stage's
        // nodes read largest first with the two that are not themes at the end.
        if !noise.is_empty() {
            let node_id = format!("{aspect}:noise");
            for t in &noise {
                mine.insert(t.clone(), node_id.clone());
            }
            nodes.push(mcpd_core::ThemeNode {
                node_id,
                kind: mcpd_core::ThemeNodeKind::Noise,
                theme_id: None,
                label: "joined no theme".to_owned(),
                trace_count: noise.len() as u64,
                coverage: share(noise.len()),
            });
        }
        if !folded.is_empty() {
            let node_id = format!("{aspect}:other");
            for t in &folded {
                mine.insert(t.clone(), node_id.clone());
            }
            nodes.push(mcpd_core::ThemeNode {
                node_id,
                kind: mcpd_core::ThemeNodeKind::Other,
                theme_id: None,
                label: "everything else".to_owned(),
                trace_count: folded.len() as u64,
                coverage: share(folded.len()),
            });
        }

        out_stages.push(mcpd_core::ThemeStage {
            aspect: aspect.clone(),
            trace_count: stage_traces,
            nodes,
        });
        node_of.push(mine);
    }

    // Links between neighbouring stages only. That is what makes the order of
    // `aspects` a question rather than a display preference.
    let mut links = Vec::new();
    for pair in 0..out_stages.len().saturating_sub(1) {
        let (left, right) = (&node_of[pair], &node_of[pair + 1]);
        let mut counted: std::collections::BTreeMap<(String, String), u64> =
            std::collections::BTreeMap::new();
        for (trace, source) in left {
            if let Some(target) = right.get(trace) {
                *counted.entry((source.clone(), target.clone())).or_insert(0) += 1;
            }
        }
        for ((source_node_id, target_node_id), trace_count) in counted {
            let size = |stage: &mcpd_core::ThemeStage, id: &str| -> u64 {
                stage.nodes.iter().find(|n| n.node_id == id).map_or(0, |n| n.trace_count)
            };
            let from = size(&out_stages[pair], &source_node_id);
            let to = size(&out_stages[pair + 1], &target_node_id);
            links.push(mcpd_core::ThemeLink {
                source_node_id,
                target_node_id,
                trace_count,
                // Both shares, because they differ whenever the two nodes are
                // different sizes and reading only one is how a thin ribbon
                // gets mistaken for a weak relationship.
                source_share: if from == 0 { 0.0 } else { trace_count as f64 / from as f64 },
                target_share: if to == 0 { 0.0 } else { trace_count as f64 / to as f64 },
            });
        }
    }

    Ok(Json(mcpd_core::ThemeFlow { snapshot, stages: out_stages, links }))
}

/// Every theme's label in one snapshot, keyed by aspect and theme id.
async fn theme_labels(
    state: &AppState,
    project: &str,
    snapshot_id: &str,
) -> ApiResult<std::collections::BTreeMap<(String, String), String>> {
    let page = state
        .store
        .list_themes(&mcpd_store::ThemeQuery {
            project: project.to_owned(),
            snapshot_id: Some(snapshot_id.to_owned()),
            limit: 1000,
            ..mcpd_store::ThemeQuery::default()
        })
        .await?;
    Ok(page
        .items
        .into_iter()
        .filter(|t| t.theme.state != mcpd_core::ThemeState::Death)
        .map(|t| ((t.theme.aspect, t.theme.theme_id), t.theme.label))
        .collect())
}

/// `GET /v1/api/themes/{theme_id}`
pub async fn get_theme(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(theme_id): Path<String>,
    Query(p): Query<Params>,
) -> ApiResult<Json<mcpd_core::ThemeDetail>> {
    let project = project(&p, &state, &headers);
    // Audited: a theme's label is the text of one note, chosen to stand for
    // the rest, so it is payload by the same rule the examples route is.
    let (kind, name, aspect, snapshot_id) = audited_valid(
        &state,
        &headers,
        peer.0,
        authorize_project(&state, &headers, Scope::Read, &project),
        || {
            require_themes(&state)?;
            let (kind, name) = entity(&p)?;
            let aspect = required(&p, "aspect")?;
            let snapshot_id = required(&p, "snapshot_id")?;
            if theme_id == mcpd_store::NOISE_THEME_ID {
                return Err(ApiError::BadParam {
                    field: "theme_id".to_owned(),
                    message: "`noise` is reserved and is not a theme; read a stage's noise from \
                              the flow, and its notes from the examples route"
                        .to_owned(),
                });
            }
            Ok((kind, name, aspect, snapshot_id))
        },
        "get_theme",
        &project,
        Some(&theme_id),
    )
    .await?;

    let snapshot = snapshot_by_id(&state, &project, kind, &name, &snapshot_id).await?;
    let ever = theme_ever(&state, &project, kind, &name, &aspect, &theme_id).await?;
    if !ever {
        return Err(ApiError::NotFound(format!(
            "no theme {theme_id:?} along the aspect {aspect:?} has ever existed for the {} \
             {name:?} in the project {project:?}",
            kind.as_str()
        )));
    }

    // A theme this snapshot does not hold is a 200 with no theme. Scrubbing a
    // timeline back past a theme's birth is a reasonable question and the
    // answer is that the theme was not there yet; the 404 above is kept for a
    // theme id this entity has never had, which is a different mistake.
    let page = state
        .store
        .list_themes(&mcpd_store::ThemeQuery {
            project: project.clone(),
            snapshot_id: Some(snapshot.snapshot_id.clone()),
            entity: Some((kind, name)),
            aspect: Some(aspect),
            theme_id: Some(theme_id),
            limit: 1,
            ..mcpd_store::ThemeQuery::default()
        })
        .await?;
    let theme = page
        .items
        .into_iter()
        .find(|t| t.theme.state != mcpd_core::ThemeState::Death)
        .map(|t| t.theme);
    Ok(Json(mcpd_core::ThemeDetail { snapshot, theme }))
}

/// Whether this entity has ever had a theme with this id along this aspect.
async fn theme_ever(
    state: &AppState,
    project: &str,
    kind: EntityKind,
    name: &str,
    aspect: &str,
    theme_id: &str,
) -> ApiResult<bool> {
    let page = state
        .store
        .list_themes(&mcpd_store::ThemeQuery {
            project: project.to_owned(),
            entity: Some((kind, name.to_owned())),
            aspect: Some(aspect.to_owned()),
            theme_id: Some(theme_id.to_owned()),
            limit: 1,
            ..mcpd_store::ThemeQuery::default()
        })
        .await?;
    Ok(!page.items.is_empty())
}

/// A query parameter this endpoint cannot answer without.
fn required(p: &Params, key: &str) -> ApiResult<String> {
    get(p, key).map(str::to_owned).ok_or_else(|| ApiError::BadParam {
        field: key.to_owned(),
        message: format!("{key} is required"),
    })
}

/// `GET /v1/api/themes/{theme_id}/examples`
pub async fn list_theme_examples(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(theme_id): Path<String>,
    Query(p): Query<Params>,
    Query(pairs): Query<Pairs>,
) -> ApiResult<Json<mcpd_core::ThemeExampleListResponse>> {
    let project = project(&p, &state, &headers);
    // Audited: this hands back the sentences a model wrote about what users
    // asked for, which is payload by the same rule a prompt is.
    // Validated before the row, for the reason `audited_valid` gives.
    let (kind, name, aspect, snapshot_id, limit, filters, cursor) = audited_valid(
        &state,
        &headers,
        peer.0,
        authorize_project(&state, &headers, Scope::Read, &project),
        || {
            require_themes(&state)?;
            let (kind, name) = entity(&p)?;
            let aspect = required(&p, "aspect")?;
            let snapshot_id = required(&p, "snapshot_id")?;
            let limit = parse::<u32>(&p, "limit")?.unwrap_or(50).clamp(1, 100);
            let filters = parse_filters(&every(&pairs, "filter"))?;
            let cursor = page_cursor(&p, false)?;
            Ok((kind, name, aspect, snapshot_id, limit, filters, cursor))
        },
        "list_theme_examples",
        &project,
        Some(&theme_id),
    )
    .await?;

    let snapshot = snapshot_by_id(&state, &project, kind, &name, &snapshot_id).await?;
    if theme_id != mcpd_store::NOISE_THEME_ID
        && !theme_ever(&state, &project, kind, &name, &aspect, &theme_id).await?
    {
        return Err(ApiError::NotFound(format!(
            "no theme {theme_id:?} along the aspect {aspect:?} has ever existed for the {} \
             {name:?} in the project {project:?}",
            kind.as_str()
        )));
    }

    // The filters, read once. A filter names an aspect other than this one, so
    // the traces that survive it are decided from that aspect's assignments.
    let mut keep: Option<std::collections::BTreeSet<String>> = None;
    for (a, theme) in &filters {
        let members = members_of(&state, &project, &snapshot.snapshot_id, a).await?;
        let here: std::collections::BTreeSet<String> = by_trace(&members)
            .into_iter()
            .filter(|(_, t)| t == theme)
            .map(|(trace, _)| trace)
            .collect();
        keep = Some(match keep {
            None => here,
            Some(have) => have.intersection(&here).cloned().collect(),
        });
    }

    let page = state
        .store
        .list_theme_members(&mcpd_store::ThemeMemberQuery {
            project: project.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            aspect: Some(aspect),
            theme_id: Some(theme_id),
            limit,
            cursor,
        })
        .await?;
    let members: Vec<mcpd_store::ThemeMember> = page
        .items
        .iter()
        .filter(|m| keep.as_ref().is_none_or(|k| k.contains(&m.trace_id)))
        .cloned()
        .collect();

    // The sentences themselves, read from the notes table rather than copied
    // onto the membership rows. That is what makes erasing a subject remove the
    // text from every theme's examples by removing it once.
    let ids: Vec<String> = members.iter().map(|m| m.note_id.clone()).collect();
    let notes = state
        .store
        .list_aspect_notes(&mcpd_store::AspectNoteQuery {
            project,
            note_ids: ids,
            limit,
            ..mcpd_store::AspectNoteQuery::default()
        })
        .await?;
    let text: std::collections::BTreeMap<String, String> =
        notes.items.into_iter().map(|n| (n.note.note_id, n.note.text)).collect();

    Ok(Json(mcpd_core::ThemeExampleListResponse {
        examples: members
            .into_iter()
            .filter_map(|m| {
                // A membership whose note has been erased is dropped rather
                // than shown empty: the sentence is gone, and a blank example
                // with a run behind it would invite a reader to guess at it.
                text.get(&m.note_id).map(|t| mcpd_core::ThemeExample {
                    note_id: m.note_id.clone(),
                    trace_id: m.trace_id,
                    text: t.clone(),
                    similarity: m.similarity,
                    trace_started_at: m.trace_started_at,
                })
            })
            .collect(),
        page: page_of(&page),
    }))
}

/// `GET /v1/api/themes/{theme_id}/history`
pub async fn list_theme_history(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    peer: Peer,
    Path(theme_id): Path<String>,
    Query(p): Query<Params>,
) -> ApiResult<Json<mcpd_core::ThemeHistory>> {
    let project = project(&p, &state, &headers);
    // Audited: the answer carries the theme's label, the text of one note.
    let (kind, name, aspect, limit) = audited_valid(
        &state,
        &headers,
        peer.0,
        authorize_project(&state, &headers, Scope::Read, &project),
        || {
            require_themes(&state)?;
            let (kind, name) = entity(&p)?;
            let aspect = required(&p, "aspect")?;
            let limit = parse::<u32>(&p, "limit")?.unwrap_or(100).clamp(1, 500);
            Ok((kind, name, aspect, limit))
        },
        "list_theme_history",
        &project,
        Some(&theme_id),
    )
    .await?;

    let page = state
        .store
        .list_themes(&mcpd_store::ThemeQuery {
            project: project.clone(),
            entity: Some((kind, name.clone())),
            aspect: Some(aspect.clone()),
            theme_id: Some(theme_id.clone()),
            // Oldest first, so the points come back in the order they are drawn
            // and differenced in.
            order: Order::Asc,
            limit,
            cursor: get(&p, "cursor").map(str::to_owned),
            ..mcpd_store::ThemeQuery::default()
        })
        .await?;
    if page.items.is_empty() && get(&p, "cursor").is_none() {
        return Err(ApiError::NotFound(format!(
            "no theme {theme_id:?} along the aspect {aspect:?} has ever existed for the {} \
             {name:?} in the project {project:?}",
            kind.as_str()
        )));
    }

    // The label at the newest point. A label can change between snapshots,
    // because the note most like the others can change while the theme holds
    // together, and the newest is the one a reader is looking at.
    let label = page.items.last().map(|t| t.theme.label.clone()).unwrap_or_default();
    Ok(Json(mcpd_core::ThemeHistory {
        theme_id,
        aspect,
        label,
        points: page
            .items
            .iter()
            .map(|t| mcpd_core::ThemeHistoryPoint {
                snapshot_id: t.theme.snapshot_id.clone(),
                cutoff_at: t.cutoff_at,
                window_from: t.window_from,
                window_to: t.window_to,
                state: t.theme.state,
                note_count: t.theme.note_count,
                trace_count: t.theme.trace_count,
                coverage: t.theme.coverage,
                split_from: t.lineage.split_from.clone(),
                split_into: t.lineage.split_into.clone(),
                merged_from: t.lineage.merged_from.clone(),
                merged_into: t.lineage.merged_into.clone(),
            })
            .collect(),
        page: page_of(&page),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn relative_time_shorthand_is_understood() {
        let p = params(&[("from", "-1h")]);
        let t = parse_time(&p, "from").unwrap().unwrap();
        let delta = (OffsetDateTime::now_utc() - t).whole_minutes();
        assert!((59..=61).contains(&delta), "expected about an hour ago, got {delta} minutes");
    }

    #[test]
    fn absolute_rfc3339_is_understood() {
        let p = params(&[("from", "2026-01-01T00:00:00Z")]);
        assert!(parse_time(&p, "from").unwrap().is_some());
    }

    #[test]
    fn a_bad_time_names_the_parameter() {
        let p = params(&[("from", "yesterday")]);
        match parse_time(&p, "from") {
            Err(ApiError::BadParam { field, message }) => {
                assert_eq!(field, "from");
                assert!(message.contains("RFC 3339"), "unhelpful message: {message}");
            }
            other => panic!("expected a named parameter error, got {other:?}"),
        }
    }

    /// The first value and the last three panicked: in the subtraction past
    /// the calendar's range, or in the unit conversion itself. The two either
    /// side of the bound are refused because the range is now stated.
    #[test]
    fn a_relative_time_out_of_range_is_refused_naming_the_range() {
        for raw in [
            "-9999999d",
            "-36501d",
            "--36501d",
            "-9223372036854775807s",
            "-153722867280912931m",
            "-2562047788015216h",
        ] {
            let p = params(&[("from", raw)]);
            match parse_time(&p, "from") {
                Err(ApiError::BadParam { field, message }) => {
                    assert_eq!(field, "from");
                    assert!(message.contains(raw), "the value is not named: {message}");
                    assert!(message.contains("36500"), "the range is not named: {message}");
                }
                other => panic!("{raw}: expected a named parameter error, got {other:?}"),
            }
        }
        // The edge of the range is inside it.
        assert!(parse_time(&params(&[("from", "-36500d")]), "from").unwrap().is_some());
    }

    #[test]
    fn a_bad_enum_lists_what_is_allowed() {
        let p = params(&[("status", "exploded")]);
        match parse_enum::<SpanStatus>(&p, "status", "unset, ok, error, cancelled") {
            Err(ApiError::BadParam { message, .. }) => {
                assert!(message.contains("ok"), "should list valid values: {message}");
            }
            other => panic!("expected a named parameter error, got {other:?}"),
        }
    }

    #[test]
    fn bucket_durations_parse() {
        assert_eq!(parse_duration_seconds("30s").unwrap(), 30);
        assert_eq!(parse_duration_seconds("5m").unwrap(), 300);
        assert_eq!(parse_duration_seconds("2h").unwrap(), 7200);
        assert_eq!(parse_duration_seconds("1d").unwrap(), 86400);
        assert!(parse_duration_seconds("7w").is_err());
        assert!(parse_duration_seconds("abc").is_err());
    }

    /// An outcome with `deleted` rows removed and `ids` left in the project.
    fn outcome(deleted: u32, ids: mcpd_store::IdentifierProbe) -> ErasureOutcome {
        ErasureOutcome {
            result: ErasureResult {
                project: "default".to_owned(),
                traces: 0,
                spans: 0,
                logs: 0,
                metrics: 0,
                scores: 0,
                feedback: 0,
                deleted,
                runs_erased: None,
                runs_in_flight: None,
                // The rest of the reply, filled in by the server from the outcome.
                aspect_notes_erased: None,
                dataset_items_erased: None,
                datasets: Vec::new(),
                deletes_pending: None,
                unmatchable_identifiers: None,
                note: None,
            },
            dataset_items: 0,
            aspect_notes: 0,
            datasets: Vec::new(),
            identifiers: ids,
            deferred: false,
        }
    }

    /// The whole point of the third arm. A rotation leaves every identifier
    /// hashed, so nothing looks mixed, the request deletes the rows written
    /// under the current key, and the reply used to carry no note at all.
    #[test]
    fn a_partial_erasure_is_not_reported_as_a_complete_one() {
        let rotated = mcpd_store::IdentifierProbe {
            nothing_attributed: false,
            unmatchable: 0,
            opaque_remaining: true,
        };
        let advisory = erasure_note(&outcome(13, rotated));
        let note = advisory.note.expect("a partial erasure came back with nothing to say");
        assert!(note.contains("Digests this request did not match"), "{note}");
        assert!(note.contains("under an earlier redaction key"), "{note}");
        // Nobody can act on it, so it must not be logged as a call for help.
        assert!(!advisory.unresolved, "a caveat with no way out was raised as a warning");
    }

    /// The other half of that gate. With nothing left in the project that a key
    /// change could be hiding, the same call says nothing, so the caveat above
    /// is not a sentence every erasure carries.
    #[test]
    fn an_erasure_with_nothing_left_behind_says_nothing() {
        let empty = mcpd_store::IdentifierProbe {
            nothing_attributed: true,
            unmatchable: 0,
            opaque_remaining: false,
        };
        assert_eq!(erasure_note(&outcome(13, empty)).note, None);
    }

    /// A count in another form is the one reason with a way out, so it is the
    /// one that reaches the operator's log as a warning.
    #[test]
    fn rows_in_another_form_are_reported_as_something_to_act_on() {
        let mixed = mcpd_store::IdentifierProbe {
            nothing_attributed: false,
            unmatchable: 2,
            opaque_remaining: false,
        };
        let advisory = erasure_note(&outcome(2, mixed));
        assert!(advisory.unresolved, "a fixable failure was reported as routine");
        assert!(advisory.note.unwrap_or_default().contains("Restore the redaction setting"));
    }

    /// A successful erasure that emptied a captured case is still a success.
    /// It carried a note, and the note was logged at WARN.
    #[test]
    fn emptying_a_captured_case_is_not_a_warning() {
        let quiet = mcpd_store::IdentifierProbe {
            nothing_attributed: true,
            unmatchable: 0,
            opaque_remaining: false,
        };
        let mut o = outcome(13, quiet);
        o.dataset_items = 2;
        let advisory = erasure_note(&o);
        assert!(advisory.note.unwrap_or_default().contains("emptied in place"));
        assert!(!advisory.unresolved, "a complete erasure was reported as unanswered");
    }

    /// The only zero the endpoint can stand behind, stated no wider than the
    /// evidence: unattributed is not the same as absent.
    #[test]
    fn a_provable_zero_claims_no_more_than_it_can() {
        let empty = mcpd_store::IdentifierProbe {
            nothing_attributed: true,
            unmatchable: 0,
            opaque_remaining: false,
        };
        let advisory = erasure_note(&outcome(0, empty));
        let note = advisory.note.expect("a zero came back with nothing to say");
        assert!(note.contains("no stored identifier a key change could be hiding"), "{note}");
        assert!(
            note.contains("not the same as holding nothing about this person"),
            "the note claims the project holds nothing about the person: {note}"
        );
        assert!(!advisory.unresolved);
    }

    /// A zero the endpoint cannot account for is a warning, and it says nothing
    /// about how many other people the project holds.
    #[test]
    fn an_unaccountable_zero_warns_without_counting_the_population() {
        let others = mcpd_store::IdentifierProbe {
            nothing_attributed: false,
            unmatchable: 0,
            opaque_remaining: true,
        };
        let advisory = erasure_note(&outcome(0, others));
        let note = advisory.note.clone().unwrap_or_default();
        assert!(advisory.unresolved, "an unexplained zero was reported as routine");
        assert!(note.contains("redaction key"), "{note}");
        assert!(
            !note.contains("distinct subjects"),
            "the reply hands a caller the project's subject count: {note}"
        );
    }

    /// The two declarations of the eight facets must not drift.
    ///
    /// `mcpd_core::Facet` is what the spec publishes to six SDKs and
    /// `mcpd_store::Facet` is what the daemon keeps its distinct-value lists
    /// under, and the handler maps one onto the other. A ninth added to the
    /// spec and not to the store would compile and then answer an empty list,
    /// which reads as "this project has nothing of that kind".
    #[test]
    fn every_published_facet_is_one_the_store_maintains() {
        for facet in Facet::ALL {
            let store = mcpd_store::Facet::parse(facet.as_str()).unwrap_or_else(|e| {
                panic!(
                    "the spec publishes facet {:?}, which the store cannot read: {e}",
                    facet.as_str()
                )
            });
            assert_eq!(store.as_str(), facet.as_str(), "the two spellings of one facet disagree");
            assert_eq!(
                store,
                store_facet(*facet),
                "the mapping sends {facet:?} to the wrong facet"
            );
        }
        assert_eq!(
            Facet::ALL.len(),
            8,
            "a facet was added or removed; add it to DiscoveryResponse's doc and to the \
             discovery suite as well"
        );
    }

    /// The same guard for the three signals a poll can name.
    #[test]
    fn every_published_delta_signal_is_one_the_store_polls() {
        for signal in DeltaSignal::ALL {
            let store = mcpd_store::DeltaSignal::parse(signal.as_str()).unwrap_or_else(|e| {
                panic!(
                    "the spec publishes signal {:?}, which the store cannot poll: {e}",
                    signal.as_str()
                )
            });
            assert_eq!(store.as_str(), signal.as_str(), "the two spellings of one signal disagree");
        }
    }

    /// A 501 is only answerable if the word it names is a word the capability
    /// set publishes. Both doors that can raise one are checked against the
    /// store's own field list.
    #[test]
    fn every_capability_a_door_names_is_a_field_of_the_capability_set() {
        let store = mcpd_store::StoreCapabilities::minimal("test");
        let names: Vec<&str> = store.fields().iter().map(|(n, _)| *n).collect();
        for capability in ["discovery", "discovery_label_values", "delta_polling"] {
            assert!(
                names.contains(&capability),
                "a door refuses naming {capability:?}, which the capability set does not carry: \
                 {names:?}"
            );
        }
    }

    #[test]
    fn empty_parameters_are_treated_as_absent() {
        // A UI that submits an empty filter box must not filter on "".
        let p = params(&[("q", ""), ("model", "")]);
        assert!(get(&p, "q").is_none());
        assert!(get(&p, "model").is_none());
    }

    #[test]
    fn a_list_handler_answers_only_the_projects_the_grant_reaches() {
        #[derive(Clone, Debug, PartialEq)]
        struct Row(&'static str);
        let rows = [Row("acme"), Row("beta"), Row("acme")];

        let only_acme = Grant::Only(["acme".to_owned()].into_iter().collect());
        assert_eq!(
            reachable(&rows, &only_acme, |r| r.0),
            vec![Row("acme"), Row("acme")],
            "a scoped key must not see another project's rows"
        );

        assert_eq!(reachable(&rows, &Grant::All, |r| r.0).len(), 3, "an unscoped key sees all");

        let nothing = Grant::Only(std::collections::BTreeSet::new());
        assert!(reachable(&rows, &nothing, |r| r.0).is_empty(), "an empty grant reaches nothing");
    }
}
