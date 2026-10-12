//! The SQLite backend for the run core.
//!
//! SQLite is enough for this core and it is the only backend here. A SQLite
//! deployment is one daemon by definition, so the runnable set is taken with a
//! conditional `UPDATE` under a single writer rather than with
//! `SELECT ... FOR UPDATE SKIP LOCKED`. A Postgres backend would implement the
//! same trait with that one difference and is not built.
//!
//! There is one connection, behind one mutex. The reads on the trait are
//! short and the writes are whole transactions, and a run store is a small,
//! hot set of rows rather than a reporting database, so a reader pool would
//! buy contention on the mutex and nothing else.

use crate::canonical::{body_hash, sha256_hex};
use crate::cause::StepCause;
use crate::context::{context_for, parse_key, steps_root, ChildRun, Journal, Scope};
use crate::graph::{Graph, Node, Refusal};
use crate::model::{
    Attempt, AttemptStatus, ConflictReason, Construct, Cursor, Definition, DefinitionKind,
    Delivery, Effect, EventOutcome, InboundEvent, Lease, OnError, OutboxRow, Run, RunEvent,
    RunEventKind, RunStatus, Slot, StepErrorClass, StepKind, Timer, TimerKind, Wait, WaitTimeout,
};
use crate::requirements::{
    attempt_requirements, cursor_requirements, settles_on_arrival, Requirement, Requirements,
};
use crate::store::{
    CompleteOutcome, DefinitionDocument, EventPage, FailOutcome, LeaseAck, NodeAttempts,
    ReconcileReport, RunConfig, RunFilter, RunPage, RunRetention, RunStore, StartRun,
    StepCompletion, StepError, SubjectErasure, TimerReport,
};
use crate::template::render;
use crate::validate::validate;
use crate::{after_ms, new_id, new_span_id, new_trace_id, Error, Result};
use async_trait::async_trait;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde_json::{json, Map, Value};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::sync::Notify;

const SCHEMA: &str = include_str!("schema.sql");

/// The schema this build writes.
pub const SCHEMA_VERSION: u32 = 2;

/// How many due timers one tick handles. A tick that tried to drain an
/// unbounded backlog would hold the writer for as long as the backlog is deep.
const TIMER_BATCH: usize = 256;

/// How many times a transaction that lost the run header's compare-and-set is
/// run again before the caller is told.
const CAS_RETRIES: usize = 3;

/// How many run ids one `node_attempts` statement names.
///
/// SQLite compiles a bound parameter per id, and its default ceiling is 999 on
/// an older build. Two hundred and fifty stays under that with room to spare
/// and still reads a monitor's whole window in a handful of statements, which
/// is the trade this number is making: fewer, wider statements against a limit
/// that varies by build.
const NODE_ATTEMPT_BATCH: usize = 250;

/// The node one attempt row belongs to: its step key with any item or
/// iteration index removed.
///
/// Keys are `id`, `id/sub`, `id[n]` or `id[n]/sub`, and ids match
/// `^[a-z][a-z0-9_]*$`, so `[` and `]` appear only as the index delimiters and
/// this is the same split [`parse_key`] makes. Written as an expression rather
/// than done in Rust so the fan-out collapses in the database: a `foreach` over
/// a thousand items is one node here, and returning a row per item instead
/// would put the whole journal of a busy window through the monitor timer.
const ATTEMPT_NODE: &str = "CASE
        WHEN instr(step_key, '[') > 0 AND instr(step_key, ']') > instr(step_key, '[')
            THEN substr(step_key, 1, instr(step_key, '[') - 1)
                 || substr(step_key, instr(step_key, ']') + 1)
        ELSE step_key
    END";

const ATTEMPT_COLUMNS: &str =
    "run_id, step_key, attempt, kind, effect, status, input, output, error,
        error_class, error_details, usage, cost_usd, span_id, deadline_ms, budget_spent,
        lease_id, lease_owner, lease_until, leased_at, started_at, orphaned_at, reclaim_until,
        ended_at, ended_by_lease_id, created_at";

const RUN_COLUMNS: &str =
    "run_id, project, definition_id, definition_version, status, cursor, input,
        output, error, error_class, trace_id, root_span_id, state, correlation, idempotency_key,
        bindings, version, steps_used, max_steps, cost_usd, max_cost_usd, deadline_at,
        root_run_id, parent_run_id, parent_step_key, depth, created_at, updated_at";

/// An instant as the integer nanoseconds a column holds.
///
/// Clamped rather than cast. A `sleep_until` may render any RFC 3339 year, and
/// an instant past April 2262 wrapped round to a negative number under `as`, so
/// a sleep meant for the far future came due on the next tick.
fn ns(t: OffsetDateTime) -> i64 {
    let n = t.unix_timestamp_nanos();
    i64::try_from(n).unwrap_or(if n < 0 { i64::MIN } else { i64::MAX })
}

fn at(v: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(v)).unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_else(|_| t.unix_timestamp().to_string())
}

fn be<E: std::fmt::Display>(e: E) -> Error {
    Error::Backend(e.to_string())
}

fn json(v: &Option<Value>) -> Option<String> {
    v.as_ref().map(std::string::ToString::to_string)
}

fn unjson(s: Option<String>) -> Option<Value> {
    s.and_then(|s| serde_json::from_str(&s).ok())
}

fn unjson_or_empty(s: Option<String>) -> Value {
    unjson(s).unwrap_or_else(|| Value::Object(Map::new()))
}

fn conflict(
    reason: ConflictReason,
    attempt: &Attempt,
    run_status: RunStatus,
    message: String,
) -> Error {
    Error::Conflict {
        reason,
        attempt: attempt.attempt,
        attempt_status: Some(attempt.status),
        run_status,
        message,
    }
}

/// Whether the caller already believes the cursor is where it says it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Move {
    /// The cursor moves. Written under the run header's compare-and-set.
    Advance,
    /// The cursor is already there and only the rows it requires are missing.
    Repair,
}

/// What applying a set of requirements did.
#[derive(Debug, Default)]
struct Applied {
    rows: usize,
    /// An event was already on the shelf when the run reached the wait, so the
    /// wait is over inside this same commit and the cursor has to move again.
    delivered: bool,
}

/// What happens to a step after its failure is recorded.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Disposition {
    /// The worker's classification decides, per section 7.6.
    Policy { retry_after_ms: Option<i64> },
    /// A reclaim under either effect of an attempt whose body may not have
    /// run, or ran under `at_least_once`: requeue at once and spend nothing.
    /// The worker did not report a failure; the daemon lost contact. The
    /// retry budget is for failures a worker reports, and charging a lost
    /// lease to it would let one daemon deploy end every run whose steps
    /// declare no retry, which is the default.
    RequeueFree,
    /// A reclaim of a started `at_most_once` attempt: the outcome is unknown
    /// and the run fails rather than running the body twice.
    FailRun,
}

/// Who a commit wakes.
///
/// Decided per write rather than once for every write. Every write used to
/// wake every waiter, a claim included, so an idle worker's own empty claim
/// woke its own long-poll and it ran claim transactions back to back for the
/// whole of its wait, and woke the outbox drain on every one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wake {
    /// Nothing anybody waits on changed.
    Nobody,
    /// A run changed, through an event row or a span, and no step became
    /// claimable.
    Readers,
    /// A step may have become claimable, and a run changed.
    Workers,
}

/// The durable run state, in SQLite.
pub struct SqliteRunStore {
    conn: Arc<Mutex<Connection>>,
    config: RunConfig,
    notify: Arc<Notify>,
    runnable: Arc<Notify>,
}

/// How long readiness waits for the run store to answer one read.
const PING_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

impl SqliteRunStore {
    /// Open, or create, a store at `path`. Pass `:memory:` for an ephemeral
    /// one.
    ///
    /// # Errors
    /// Fails when the file cannot be opened or the pragmas cannot be set.
    pub fn open(path: impl AsRef<Path>, config: RunConfig) -> Result<Self> {
        let path = path.as_ref();
        let conn = if path.to_string_lossy() == ":memory:" {
            Connection::open_in_memory().map_err(be)?
        } else {
            Connection::open(path).map_err(be)?
        };
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            config,
            notify: Arc::new(Notify::new()),
            runnable: Arc::new(Notify::new()),
        })
    }

    async fn with_write<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: Fn(&mut Connection) -> Result<T> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            // A panic in row-building code rolls its transaction back and
            // leaves the connection usable, so refusing to reuse it after one
            // bad row is strictly worse than the fault it guards against.
            let mut guard = conn.lock().unwrap_or_else(PoisonError::into_inner);
            let mut last = None;
            for _ in 0..CAS_RETRIES {
                match f(&mut guard) {
                    Err(Error::Retry(reason)) => last = Some(reason),
                    other => return other,
                }
            }
            Err(Error::Backend(format!(
                "the run header moved under every attempt: {}",
                last.unwrap_or_default()
            )))
        })
        .await
        .map_err(|e| Error::Backend(format!("run store task failed: {e}")))?
    }

    /// Run a write and wake whoever `wake` names. A write that failed rolled
    /// back, so it wakes nobody.
    async fn with_write_waking<T, F>(&self, wake: Wake, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: Fn(&mut Connection) -> Result<T> + Send + 'static,
    {
        let out = self.with_write(f).await;
        if out.is_ok() {
            self.wake(wake);
        }
        out
    }

    fn wake(&self, wake: Wake) {
        match wake {
            Wake::Nobody => {}
            Wake::Readers => self.notify.notify_waiters(),
            Wake::Workers => {
                self.notify.notify_waiters();
                self.runnable.notify_waiters();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

fn bad_column(col: usize, what: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, what.into())
}

fn row_to_run(r: &rusqlite::Row<'_>) -> rusqlite::Result<Run> {
    let status: String = r.get(4)?;
    let cursor: String = r.get(5)?;
    let error_class: Option<String> = r.get(9)?;
    let cursor_value: Cursor = serde_json::from_str(&cursor)
        .map_err(|e| bad_column(5, format!("unreadable cursor: {e}")))?;
    Ok(Run {
        run_id: r.get(0)?,
        project: r.get(1)?,
        definition_id: r.get(2)?,
        definition_version: r.get::<_, i64>(3)? as u32,
        status: RunStatus::parse(&status)
            .ok_or_else(|| bad_column(4, format!("unknown run status {status}")))?,
        cursor: cursor_value,
        input: unjson(r.get(6)?),
        output: unjson(r.get(7)?),
        error: r.get(8)?,
        error_class: error_class.as_deref().and_then(StepErrorClass::parse),
        trace_id: r.get(10)?,
        root_span_id: r.get(11)?,
        state: unjson_or_empty(r.get(12)?),
        correlation: unjson_or_empty(r.get(13)?),
        idempotency_key: r.get(14)?,
        bindings: unjson_or_empty(r.get(15)?),
        version: r.get(16)?,
        steps_used: r.get(17)?,
        max_steps: r.get(18)?,
        cost_usd: r.get(19)?,
        max_cost_usd: r.get(20)?,
        deadline_at: r.get::<_, Option<i64>>(21)?.map(at),
        root_run_id: r.get(22)?,
        parent_run_id: r.get(23)?,
        parent_step_key: r.get(24)?,
        depth: r.get(25)?,
        created_at: at(r.get(26)?),
        updated_at: at(r.get(27)?),
    })
}

fn load_run(tx: &Connection, run_id: &str) -> Result<Run> {
    tx.query_row(
        &format!("SELECT {RUN_COLUMNS} FROM runs WHERE run_id = ?1"),
        params![run_id],
        row_to_run,
    )
    .optional()
    .map_err(be)?
    .ok_or_else(|| Error::NotFound { kind: "run", id: run_id.to_string() })
}

fn load_definition(tx: &Connection, project: &str, id: &str, version: u32) -> Result<Definition> {
    tx.query_row(
        "SELECT kind, body, created_at FROM definitions
          WHERE project = ?1 AND id = ?2 AND version = ?3",
        params![project, id, i64::from(version)],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)),
    )
    .optional()
    .map_err(be)?
    .ok_or_else(|| Error::NotFound { kind: "definition", id: format!("{id}@{version}") })
    .and_then(|(kind, body, created_at)| {
        Ok(Definition {
            project: project.to_string(),
            id: id.to_string(),
            version,
            kind: DefinitionKind::parse(&kind)
                .ok_or_else(|| Error::Backend(format!("unknown definition kind {kind}")))?,
            body: serde_json::from_str(&body).map_err(be)?,
            created_at: at(created_at),
        })
    })
}

fn latest_version(tx: &Connection, project: &str, id: &str) -> Result<Option<u32>> {
    tx.query_row(
        "SELECT MAX(version) FROM definitions WHERE project = ?1 AND id = ?2",
        params![project, id],
        |r| r.get::<_, Option<i64>>(0),
    )
    .map_err(be)
    .map(|v| v.map(|v| v as u32))
}

fn load_graph(tx: &Connection, run: &Run) -> Result<Graph> {
    let def = load_definition(tx, &run.project, &run.definition_id, run.definition_version)?;
    Graph::parse(&def.body)
}

fn row_to_attempt(r: &rusqlite::Row<'_>) -> rusqlite::Result<Attempt> {
    let kind: String = r.get(3)?;
    let effect: String = r.get(4)?;
    let status: String = r.get(5)?;
    let error_class: Option<String> = r.get(9)?;
    Ok(Attempt {
        run_id: r.get(0)?,
        step_key: r.get(1)?,
        attempt: r.get(2)?,
        kind: StepKind::parse(&kind)
            .ok_or_else(|| bad_column(3, format!("unknown step kind {kind}")))?,
        effect: Effect::parse(&effect)
            .ok_or_else(|| bad_column(4, format!("unknown effect {effect}")))?,
        status: AttemptStatus::parse(&status)
            .ok_or_else(|| bad_column(5, format!("unknown attempt status {status}")))?,
        input: unjson(r.get(6)?),
        output: unjson(r.get(7)?),
        error: r.get(8)?,
        error_class: error_class.as_deref().and_then(StepErrorClass::parse),
        error_details: unjson(r.get(10)?),
        usage: unjson(r.get(11)?),
        cost_usd: r.get(12)?,
        span_id: r.get(13)?,
        deadline_ms: r.get(14)?,
        budget_spent: r.get::<_, i64>(15)? != 0,
        lease_id: r.get(16)?,
        lease_owner: r.get(17)?,
        lease_until: r.get::<_, Option<i64>>(18)?.map(at),
        leased_at: r.get::<_, Option<i64>>(19)?.map(at),
        started_at: r.get::<_, Option<i64>>(20)?.map(at),
        orphaned_at: r.get::<_, Option<i64>>(21)?.map(at),
        reclaim_until: r.get::<_, Option<i64>>(22)?.map(at),
        ended_at: r.get::<_, Option<i64>>(23)?.map(at),
        ended_by_lease_id: r.get(24)?,
        created_at: at(r.get(25)?),
    })
}

fn read_attempts(tx: &Connection, sql: &str, p: impl rusqlite::Params) -> Result<Vec<Attempt>> {
    let mut stmt = tx.prepare(sql).map_err(be)?;
    let rows = stmt.query_map(p, row_to_attempt).map_err(be)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(be)?);
    }
    Ok(out)
}

fn attempts_of(tx: &Connection, run_id: &str) -> Result<Vec<Attempt>> {
    read_attempts(
        tx,
        &format!(
            "SELECT {ATTEMPT_COLUMNS} FROM step_attempts WHERE run_id = ?1 ORDER BY step_key, attempt"
        ),
        params![run_id],
    )
}

fn attempts_of_step(tx: &Connection, run_id: &str, step_key: &str) -> Result<Vec<Attempt>> {
    read_attempts(
        tx,
        &format!(
            "SELECT {ATTEMPT_COLUMNS} FROM step_attempts
              WHERE run_id = ?1 AND step_key = ?2 ORDER BY attempt"
        ),
        params![run_id, step_key],
    )
}

fn attempts_in(
    tx: &Connection,
    run_id: &str,
    step_key: &str,
    statuses: &str,
) -> Result<Vec<Attempt>> {
    read_attempts(
        tx,
        &format!(
            "SELECT {ATTEMPT_COLUMNS} FROM step_attempts
              WHERE run_id = ?1 AND step_key = ?2 AND status IN ({statuses}) ORDER BY attempt"
        ),
        params![run_id, step_key],
    )
}

fn load_attempt_by_lease(tx: &Connection, lease_id: &str) -> Result<Attempt> {
    read_attempts(
        tx,
        &format!("SELECT {ATTEMPT_COLUMNS} FROM step_attempts WHERE lease_id = ?1"),
        params![lease_id],
    )?
    .pop()
    .ok_or_else(|| Error::NotFound { kind: "lease", id: lease_id.to_string() })
}

fn load_children(tx: &Connection, run_id: &str) -> Result<Vec<ChildRun>> {
    let mut stmt = tx
        .prepare(
            "SELECT parent_step_key, status, output, error, error_class FROM runs
              WHERE parent_run_id = ?1",
        )
        .map_err(be)?;
    let rows = stmt
        .query_map(params![run_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(be)?;
    let mut out = Vec::new();
    for row in rows {
        let (step_key, status, output, error, class) = row.map_err(be)?;
        out.push(ChildRun {
            step_key,
            status: RunStatus::parse(&status)
                .ok_or_else(|| Error::Backend(format!("unknown run status {status}")))?,
            output: unjson(output),
            error,
            error_class: class.as_deref().and_then(StepErrorClass::parse),
        });
    }
    Ok(out)
}

fn load_journal(tx: &Connection, run_id: &str) -> Result<Journal> {
    Ok(Journal { attempts: attempts_of(tx, run_id)?, children: load_children(tx, run_id)? })
}

fn child_run_ids(tx: &Connection, run_id: &str, running_only: bool) -> Result<Vec<String>> {
    let mut stmt = tx
        .prepare(
            "SELECT run_id FROM runs WHERE parent_run_id = ?1 AND (?2 = 0 OR status = 'running')",
        )
        .map_err(be)?;
    let rows = stmt
        .query_map(params![run_id, i64::from(running_only)], |r| r.get(0))
        .map_err(be)?
        .collect::<std::result::Result<_, _>>()
        .map_err(be)?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Spans
// ---------------------------------------------------------------------------

struct SpanSpec<'a> {
    run: &'a Run,
    span_id: &'a str,
    parent_span_id: Option<&'a str>,
    name: &'a str,
    kind: &'a str,
    status: &'a str,
    start: OffsetDateTime,
    end: Option<OffsetDateTime>,
    attributes: Value,
    input: Option<Value>,
    output: Option<Value>,
    error: Option<Value>,
}

/// The wire `Span` shape, built by hand from the field names in
/// `spec/mcpd.spec.json` so this crate does not depend on `mcpd-core`.
fn span_json(spec: &SpanSpec<'_>) -> Value {
    let mut correlation = match &spec.run.correlation {
        Value::Object(map) => map.clone(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => {
            Map::new()
        }
    };
    correlation.insert("project".into(), Value::String(spec.run.project.clone()));
    correlation.insert("run_id".into(), Value::String(spec.run.run_id.clone()));

    let mut span = Map::new();
    span.insert("trace_id".into(), Value::String(spec.run.trace_id.clone()));
    span.insert("span_id".into(), Value::String(spec.span_id.to_string()));
    if let Some(parent) = spec.parent_span_id {
        span.insert("parent_span_id".into(), Value::String(parent.to_string()));
    }
    span.insert("name".into(), Value::String(spec.name.to_string()));
    span.insert("kind".into(), Value::String(spec.kind.to_string()));
    span.insert("status".into(), Value::String(spec.status.to_string()));
    span.insert("start_time".into(), Value::String(rfc3339(spec.start)));
    if let Some(end) = spec.end {
        span.insert("end_time".into(), Value::String(rfc3339(end)));
        let duration = ns(end).saturating_sub(ns(spec.start)).max(0);
        span.insert("duration_ns".into(), json!(duration));
    }
    span.insert("is_root".into(), Value::Bool(spec.parent_span_id.is_none()));
    span.insert("is_event".into(), Value::Bool(false));
    span.insert("entity_kind".into(), Value::String("workflow".into()));
    span.insert("entity_id".into(), Value::String(spec.run.definition_id.clone()));
    span.insert("entity_name".into(), Value::String(spec.run.definition_id.clone()));
    if let Some(input) = &spec.input {
        span.insert("input".into(), input.clone());
    }
    if let Some(output) = &spec.output {
        span.insert("output".into(), output.clone());
    }
    span.insert("attributes".into(), spec.attributes.clone());
    if let Some(error) = &spec.error {
        span.insert("error".into(), error.clone());
    }
    span.insert("correlation".into(), Value::Object(correlation));
    Value::Object(span)
}

fn enqueue_span(
    tx: &Connection,
    run_id: &str,
    span_id: &str,
    ingest_key: &str,
    phase: &str,
    payload: &Value,
    now: OffsetDateTime,
) -> Result<()> {
    tx.execute(
        "INSERT INTO span_outbox (outbox_id, run_id, span_id, ingest_key, phase, payload, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![new_id("obx"), run_id, span_id, ingest_key, phase, payload.to_string(), ns(now)],
    )
    .map_err(be)?;
    Ok(())
}

fn root_span_attributes(run: &Run) -> Value {
    json!({ "definition_id": run.definition_id, "definition_version": run.definition_version })
}

fn open_root_span(
    tx: &Connection,
    run: &Run,
    parent_span_id: Option<&str>,
    now: OffsetDateTime,
) -> Result<()> {
    let payload = span_json(&SpanSpec {
        run,
        span_id: &run.root_span_id,
        parent_span_id,
        name: &run.definition_id,
        kind: "workflow_run",
        status: "unset",
        start: run.created_at,
        end: None,
        attributes: root_span_attributes(run),
        input: run.input.clone(),
        output: None,
        error: None,
    });
    let key = format!("run:{}:root:open", run.run_id);
    enqueue_span(tx, &run.run_id, &run.root_span_id, &key, "open", &payload, now)
}

fn span_status_of(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Running => "unset",
        RunStatus::Succeeded => "ok",
        RunStatus::Failed | RunStatus::TimedOut => "error",
        RunStatus::Cancelled => "cancelled",
    }
}

fn close_root_span(
    tx: &Connection,
    run: &Run,
    parent_span_id: Option<&str>,
    now: OffsetDateTime,
) -> Result<()> {
    let error = run.error.as_ref().map(|message| {
        json!({ "message": message, "kind": run.error_class.map(StepErrorClass::as_str) })
    });
    let payload = span_json(&SpanSpec {
        run,
        span_id: &run.root_span_id,
        parent_span_id,
        name: &run.definition_id,
        kind: "workflow_run",
        status: span_status_of(run.status),
        start: run.created_at,
        end: Some(now),
        attributes: {
            // The run's priced cost, as an attribute: the root is a container
            // and the spans under it carry the tokens.
            let mut attributes = root_span_attributes(run);
            attributes["cost_usd"] = json!(run.cost_usd);
            attributes
        },
        input: run.input.clone(),
        output: run.output.clone(),
        error,
    });
    let key = format!("run:{}:root:close", run.run_id);
    enqueue_span(tx, &run.run_id, &run.root_span_id, &key, "close", &payload, now)
}

fn step_span_kind(kind: StepKind) -> &'static str {
    match kind {
        StepKind::Task => "workflow_step",
        StepKind::Agent => "agent_run",
        StepKind::Wait => "workflow_wait_event",
    }
}

fn step_span_attributes(a: &Attempt) -> Value {
    json!({ "attempt": a.attempt, "step_key": a.step_key })
}

/// The attributes of a closed step span.
///
/// The attempt's usage and cost ride here, not in the span's own `usage` and
/// `cost`: a step span is a container, and the model spans the body recorded
/// under it already carry the tokens they spent. A trace total that summed
/// both would count every token twice. Kept as attributes, the numbers stay
/// visible on the span without entering any aggregate.
fn closed_step_span_attributes(a: &Attempt) -> Value {
    let mut attributes = step_span_attributes(a);
    if let Value::Object(map) = &mut attributes {
        if let Some(usage) = &a.usage {
            map.insert("usage".into(), usage.clone());
        }
        if let Some(cost) = a.cost_usd {
            map.insert("cost_usd".into(), json!(cost));
        }
    }
    attributes
}

/// The open half of a step span, written when the worker declares a body.
fn open_step_span(
    tx: &Connection,
    run: &Run,
    a: &Attempt,
    start: OffsetDateTime,
    now: OffsetDateTime,
) -> Result<()> {
    let payload = span_json(&SpanSpec {
        run,
        span_id: &a.span_id,
        parent_span_id: Some(&run.root_span_id),
        name: &a.step_key,
        kind: step_span_kind(a.kind),
        status: "unset",
        start,
        end: None,
        attributes: step_span_attributes(a),
        input: a.input.clone(),
        output: None,
        error: None,
    });
    let key = format!("run:{}:{}:{}:open", run.run_id, a.step_key, a.attempt);
    enqueue_span(tx, &run.run_id, &a.span_id, &key, "open", &payload, now)
}

/// The closing half of a step span. An attempt that never declared a body has
/// no open row yet, so one is written first, starting at the lease, and the
/// close carries the identical `trace_id`, `span_id` and `start_time`.
fn close_step_span(tx: &Connection, run: &Run, a: &Attempt, now: OffsetDateTime) -> Result<()> {
    let Some(start) = a.started_at.or(a.leased_at) else {
        // Never leased: nothing ran and there is no span to close.
        return Ok(());
    };
    if a.started_at.is_none() {
        open_step_span(tx, run, a, start, now)?;
    }
    let (status, error) = match a.status {
        AttemptStatus::Succeeded => ("ok", None),
        AttemptStatus::Failed | AttemptStatus::TimedOut => (
            "error",
            Some(json!({
                "message": a.error.clone().unwrap_or_else(|| "failed".into()),
                "kind": a.error_class.map(StepErrorClass::as_str),
                "details": a.error_details,
            })),
        ),
        AttemptStatus::Skipped => ("cancelled", None),
        AttemptStatus::Pending
        | AttemptStatus::Leased
        | AttemptStatus::Started
        | AttemptStatus::Orphaned => ("unset", None),
    };
    let payload = span_json(&SpanSpec {
        run,
        span_id: &a.span_id,
        parent_span_id: Some(&run.root_span_id),
        name: &a.step_key,
        kind: step_span_kind(a.kind),
        status,
        start,
        end: Some(now),
        attributes: closed_step_span_attributes(a),
        input: a.input.clone(),
        output: a.output.clone(),
        error,
    });
    let key = format!("run:{}:{}:{}:close", run.run_id, a.step_key, a.attempt);
    enqueue_span(tx, &run.run_id, &a.span_id, &key, "close", &payload, now)
}

fn skipped_span(tx: &Connection, run: &Run, step_key: &str, now: OffsetDateTime) -> Result<()> {
    let span_id = new_span_id();
    let payload = span_json(&SpanSpec {
        run,
        span_id: &span_id,
        parent_span_id: Some(&run.root_span_id),
        name: step_key,
        kind: "workflow_conditional_eval",
        status: "ok",
        start: now,
        end: Some(now),
        attributes: json!({ "result": false, "step_key": step_key }),
        input: None,
        output: None,
        error: None,
    });
    let key = format!("run:{}:{}:0:close", run.run_id, step_key);
    enqueue_span(tx, &run.run_id, &span_id, &key, "close", &payload, now)
}

fn sleep_span(
    tx: &Connection,
    run: &Run,
    step_key: &str,
    start: OffsetDateTime,
    now: OffsetDateTime,
) -> Result<()> {
    let span_id = new_span_id();
    let payload = span_json(&SpanSpec {
        run,
        span_id: &span_id,
        parent_span_id: Some(&run.root_span_id),
        name: step_key,
        kind: "workflow_sleep",
        status: "ok",
        start,
        end: Some(now),
        attributes: json!({ "step_key": step_key }),
        input: None,
        output: None,
        error: None,
    });
    let key = format!("run:{}:{}:0:close", run.run_id, step_key);
    enqueue_span(tx, &run.run_id, &span_id, &key, "close", &payload, now)
}

struct WaitRow {
    step_key: String,
    event: String,
    correlation_key: String,
    expires_at: OffsetDateTime,
    span_id: String,
    created_at: OffsetDateTime,
}

fn load_wait(tx: &Connection, run_id: &str, step_key: &str) -> Result<Option<WaitRow>> {
    tx.query_row(
        "SELECT step_key, event, correlation_key, expires_at, span_id, created_at FROM waits
          WHERE run_id = ?1 AND step_key = ?2",
        params![run_id, step_key],
        |r| {
            Ok(WaitRow {
                step_key: r.get(0)?,
                event: r.get(1)?,
                correlation_key: r.get(2)?,
                expires_at: at(r.get(3)?),
                span_id: r.get(4)?,
                created_at: at(r.get(5)?),
            })
        },
    )
    .optional()
    .map_err(be)
}

#[allow(clippy::too_many_arguments)]
fn wait_span(
    tx: &Connection,
    run: &Run,
    w: &WaitRow,
    phase: &str,
    status: &str,
    end: Option<OffsetDateTime>,
    extra: Value,
    now: OffsetDateTime,
) -> Result<()> {
    let mut attributes = json!({
        "event": w.event, "key": w.correlation_key, "step_key": w.step_key,
        "expires_at": rfc3339(w.expires_at),
    });
    if let (Value::Object(map), Value::Object(more)) = (&mut attributes, extra) {
        map.extend(more);
    }
    let error = (status == "error").then(|| json!({ "message": "wait expired" }));
    let payload = span_json(&SpanSpec {
        run,
        span_id: &w.span_id,
        parent_span_id: Some(&run.root_span_id),
        name: &w.step_key,
        kind: "workflow_wait_event",
        status,
        start: w.created_at,
        end,
        attributes,
        input: None,
        output: None,
        error,
    });
    let key = format!("run:{}:{}:0:{phase}", run.run_id, w.step_key);
    enqueue_span(tx, &run.run_id, &w.span_id, &key, phase, &payload, now)
}

#[allow(clippy::too_many_arguments)]
fn sub_span(
    tx: &Connection,
    run: &Run,
    step_key: &str,
    span_id: &str,
    child: &Run,
    phase: &str,
    start: OffsetDateTime,
    now: OffsetDateTime,
) -> Result<()> {
    let closed = phase == "close";
    let error = (closed && child.status != RunStatus::Succeeded).then(|| {
        json!({
            "message": child.error.clone().unwrap_or_else(|| format!("child run {}", child.status.as_str())),
            "kind": child.error_class.map(StepErrorClass::as_str),
        })
    });
    let payload = span_json(&SpanSpec {
        run,
        span_id,
        parent_span_id: Some(&run.root_span_id),
        name: step_key,
        kind: "workflow_step",
        status: if closed { span_status_of(child.status) } else { "unset" },
        start,
        end: closed.then_some(now),
        attributes: {
            let mut attributes = json!({
                "step_key": step_key, "child_run_id": child.run_id,
                "definition_id": child.definition_id, "definition_version": child.definition_version,
            });
            // The child run's cost, as an attribute for the same reason a
            // step's is: the child's own spans carry the tokens.
            if closed {
                attributes["cost_usd"] = json!(child.cost_usd);
            }
            attributes
        },
        input: child.input.clone(),
        output: if closed { child.output.clone() } else { None },
        error,
    });
    let key = format!("run:{}:{step_key}:0:{phase}", run.run_id);
    enqueue_span(tx, &run.run_id, span_id, &key, phase, &payload, now)
}

// ---------------------------------------------------------------------------
// The event log
// ---------------------------------------------------------------------------

fn emit(
    tx: &Connection,
    run_id: &str,
    kind: RunEventKind,
    payload: Value,
    now: OffsetDateTime,
) -> Result<()> {
    // The sequence is allocated from the run header inside the transaction
    // that already holds it, so two branches of one `parallel` completing at
    // once cannot interleave a gap.
    tx.execute(
        "UPDATE runs SET next_event_seq = next_event_seq + 1 WHERE run_id = ?1",
        params![run_id],
    )
    .map_err(be)?;
    let seq: i64 = tx
        .query_row("SELECT next_event_seq - 1 FROM runs WHERE run_id = ?1", params![run_id], |r| {
            r.get(0)
        })
        .map_err(be)?;
    tx.execute(
        "INSERT INTO run_events (run_id, seq, type, payload, at) VALUES (?1,?2,?3,?4,?5)",
        params![run_id, seq, kind.as_str(), payload.to_string(), ns(now)],
    )
    .map_err(be)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Attempt rows
// ---------------------------------------------------------------------------

/// Write one attempt row: the rendered input, a fresh span id, and the
/// deadline the worker is told. Returns whether a row was written.
fn write_attempt(
    tx: &Connection,
    run: &Run,
    graph: &Graph,
    slot: &Slot,
    attempt: i64,
    now: OffsetDateTime,
) -> Result<bool> {
    let journal = load_journal(tx, &run.run_id)?;
    let scope = graph.scope_for(run, &journal, &slot.step_key);
    let policy = graph.policy(&slot.step_key);
    let input = match policy.and_then(|p| p.input) {
        Some(template) => render(template, &context_for(run, &journal, &scope)),
        None => run.input.clone().unwrap_or(Value::Null),
    };
    let deadline_ms = policy.and_then(|p| p.step_timeout_ms);
    let status = if slot.skipped { AttemptStatus::Skipped } else { AttemptStatus::Pending };
    let changed = tx
        .execute(
            "INSERT INTO step_attempts
                 (run_id, step_key, attempt, kind, effect, status, input, span_id, deadline_ms,
                  budget_spent, ended_at, created_at)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11
              WHERE NOT EXISTS (SELECT 1 FROM step_attempts
                                 WHERE run_id = ?1 AND step_key = ?2 AND attempt = ?3)",
            params![
                run.run_id,
                slot.step_key,
                attempt,
                slot.kind.as_str(),
                slot.effect.as_str(),
                status.as_str(),
                input.to_string(),
                new_span_id(),
                deadline_ms,
                slot.skipped.then(|| ns(now)),
                ns(now),
            ],
        )
        .map_err(be)?;
    if changed > 0 && slot.skipped {
        emit(
            tx,
            &run.run_id,
            RunEventKind::StepSkipped,
            json!({ "step_key": slot.step_key }),
            now,
        )?;
        skipped_span(tx, run, &slot.step_key, now)?;
    }
    Ok(changed > 0)
}

/// Write the next attempt of a step, with its own step timeout re-armed,
/// because `step_timeout_ms` bounds one attempt.
fn requeue(
    tx: &Connection,
    run: &Run,
    graph: &Graph,
    step_key: &str,
    attempt: i64,
    now: OffsetDateTime,
) -> Result<()> {
    let Some(policy) = graph.policy(step_key) else {
        return Err(Error::Backend(format!(
            "step \"{step_key}\" of run {} is not in its definition",
            run.run_id
        )));
    };
    let parts = parse_key(step_key);
    let slot = Slot {
        step_key: step_key.to_string(),
        kind: policy.kind,
        effect: policy.effect,
        step_deadline: policy.step_timeout_ms.map(|ms| after_ms(now, ms)),
        item_index: if parts.sub.is_none() { parts.index } else { None },
        iteration: parts.sub.and(parts.index).map(|i| i as u32),
        skipped: false,
    };
    write_attempt(tx, run, graph, &slot, attempt, now)?;
    if let Some(due_at) = slot.step_deadline {
        tx.execute(
            "INSERT INTO timers (run_id, step_key, kind, due_at, created_at)
             VALUES (?1,?2,'step_timeout',?3,?4)
               ON CONFLICT (run_id, step_key, kind) DO UPDATE SET due_at = ?3, created_at = ?4",
            params![run.run_id, step_key, ns(due_at), ns(now)],
        )
        .map_err(be)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The one place requirements are applied
// ---------------------------------------------------------------------------

/// Write every row `reqs` names that is not already there, and count the ones
/// that were not.
///
/// Every statement is conditional, so applying the same requirements twice
/// writes nothing the second time. That is the property the reconciler stands
/// on: it can call the materialiser's own code against a healthy run.
fn apply(
    tx: &Connection,
    run: &Run,
    reqs: &Requirements,
    cfg: &RunConfig,
    now: OffsetDateTime,
) -> Result<Applied> {
    let mut out = Applied::default();
    let mut graph: Option<Graph> = None;
    for req in &reqs.0 {
        match req {
            Requirement::Timer { step_key, kind, due_at } => {
                out.rows += tx
                    .execute(
                        "INSERT INTO timers (run_id, step_key, kind, due_at, created_at)
                         VALUES (?1,?2,?3,?4,?5)
                           ON CONFLICT (run_id, step_key, kind) DO NOTHING",
                        params![run.run_id, step_key, kind.as_str(), ns(*due_at), ns(now)],
                    )
                    .map_err(be)?;
            }
            Requirement::Wait { step_key, event, correlation_key, expires_at } => {
                let span_id = new_span_id();
                let changed = tx
                    .execute(
                        "INSERT INTO waits (run_id, step_key, project, event, correlation_key,
                                            expires_at, span_id, created_at)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
                           ON CONFLICT (run_id, step_key) DO NOTHING",
                        params![
                            run.run_id,
                            step_key,
                            run.project,
                            event,
                            correlation_key,
                            ns(*expires_at),
                            span_id,
                            ns(now)
                        ],
                    )
                    .map_err(be)?;
                if changed > 0 {
                    out.rows += changed;
                    emit(
                        tx,
                        &run.run_id,
                        RunEventKind::RunWaiting,
                        json!({
                            "step_key": step_key, "event": event, "key": correlation_key,
                            "expires_at": rfc3339(*expires_at),
                        }),
                        now,
                    )?;
                    if let Some(w) = load_wait(tx, &run.run_id, step_key)? {
                        wait_span(tx, run, &w, "open", "unset", None, json!({}), now)?;
                    }
                }
            }
            Requirement::ParkedLookup { step_key, event, correlation_key } => {
                if let Some((event_id, payload)) = tx
                    .query_row(
                        "SELECT event_id, payload FROM parked_events
                          WHERE project = ?1 AND event = ?2 AND correlation_key = ?3
                            AND consumed_by_run_id IS NULL AND expires_at > ?4
                          ORDER BY received_at LIMIT 1",
                        params![run.project, event, correlation_key, ns(now)],
                        |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
                    )
                    .optional()
                    .map_err(be)?
                {
                    tx.execute(
                        "UPDATE parked_events SET consumed_by_run_id = ?1
                          WHERE project = ?2 AND event_id = ?3",
                        params![run.run_id, run.project, event_id],
                    )
                    .map_err(be)?;
                    settle_wait(
                        tx,
                        run,
                        step_key,
                        AttemptStatus::Succeeded,
                        unjson(payload),
                        None,
                        Some(&event_id),
                        now,
                    )?;
                    out.rows += 1;
                    out.delivered = true;
                }
            }
            Requirement::AttemptSlot { slot } => {
                let exists: Option<i64> = tx
                    .query_row(
                        "SELECT 1 FROM step_attempts WHERE run_id = ?1 AND step_key = ?2 LIMIT 1",
                        params![run.run_id, slot.step_key],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(be)?;
                if exists.is_none() {
                    if graph.is_none() {
                        graph = Some(load_graph(tx, run)?);
                    }
                    if let Some(g) = &graph {
                        if write_attempt(tx, run, g, slot, 0, now)? {
                            out.rows += 1;
                        }
                    }
                }
            }
            Requirement::ChildRun {
                step_key,
                child_run_id,
                definition_id,
                definition_version,
                span_id,
            } => {
                out.rows += start_child(
                    tx,
                    run,
                    &ChildSpec {
                        step_key,
                        child_run_id,
                        definition_id,
                        definition_version: *definition_version,
                        span_id,
                    },
                    cfg,
                    now,
                )?;
            }
            Requirement::Skipped { step_key, kind } => {
                let changed = tx
                    .execute(
                        "INSERT INTO step_attempts
                             (run_id, step_key, attempt, kind, effect, status, span_id, ended_at,
                              created_at)
                         SELECT ?1, ?2, 0, ?3, 'at_least_once', 'skipped', ?4, ?5, ?5
                          WHERE NOT EXISTS (SELECT 1 FROM step_attempts
                                             WHERE run_id = ?1 AND step_key = ?2)",
                        params![run.run_id, step_key, kind.as_str(), new_span_id(), ns(now)],
                    )
                    .map_err(be)?;
                if changed > 0 {
                    out.rows += changed;
                    emit(
                        tx,
                        &run.run_id,
                        RunEventKind::StepSkipped,
                        json!({ "step_key": step_key }),
                        now,
                    )?;
                    skipped_span(tx, run, step_key, now)?;
                }
            }
            Requirement::Terminal { status } => {
                out.rows += finish(tx, run, *status, cfg, now)?;
            }
        }
    }
    Ok(out)
}

/// Write the attempt row a wait leaves behind, close its span, and remove the
/// wait and its deadline together. A deadline that outlives the state it
/// guards is the next stale row somebody has to reason about.
///
/// The wait's row is attempt 0, like every other first attempt in the
/// journal.
#[allow(clippy::too_many_arguments)]
fn settle_wait(
    tx: &Connection,
    run: &Run,
    step_key: &str,
    status: AttemptStatus,
    output: Option<Value>,
    error: Option<&str>,
    event_id: Option<&str>,
    now: OffsetDateTime,
) -> Result<()> {
    let wait = load_wait(tx, &run.run_id, step_key)?;
    let span_id = wait.as_ref().map_or_else(new_span_id, |w| w.span_id.clone());
    tx.execute(
        "INSERT INTO step_attempts
             (run_id, step_key, attempt, kind, effect, status, output, error, span_id, ended_at,
              created_at)
         VALUES (?1, ?2, 0, 'wait', 'at_least_once', ?3, ?4, ?5, ?6, ?7, ?7)
           ON CONFLICT (run_id, step_key, attempt)
           DO UPDATE SET status = ?3, output = ?4, error = ?5, ended_at = ?7",
        params![run.run_id, step_key, status.as_str(), json(&output), error, span_id, ns(now)],
    )
    .map_err(be)?;
    if let Some(w) = &wait {
        let (span_status, extra) = match status {
            AttemptStatus::Succeeded => ("ok", json!({ "event_id": event_id })),
            AttemptStatus::TimedOut => ("error", json!({})),
            AttemptStatus::Failed | AttemptStatus::Skipped => ("cancelled", json!({})),
            AttemptStatus::Pending
            | AttemptStatus::Leased
            | AttemptStatus::Started
            | AttemptStatus::Orphaned => ("unset", json!({})),
        };
        wait_span(tx, run, w, "close", span_status, Some(now), extra, now)?;
    }
    if let Some(id) = event_id {
        emit(
            tx,
            &run.run_id,
            RunEventKind::RunResumed,
            json!({ "step_key": step_key, "event_id": id }),
            now,
        )?;
    }
    tx.execute(
        "DELETE FROM waits WHERE run_id = ?1 AND step_key = ?2",
        params![run.run_id, step_key],
    )
    .map_err(be)?;
    tx.execute(
        "DELETE FROM timers WHERE run_id = ?1 AND step_key = ?2 AND kind = 'wait_timeout'",
        params![run.run_id, step_key],
    )
    .map_err(be)?;
    Ok(())
}

/// The identity of a child run, minted by the parent's transaction.
struct ChildSpec<'a> {
    step_key: &'a str,
    child_run_id: &'a str,
    definition_id: &'a str,
    definition_version: u32,
    span_id: &'a str,
}

fn start_child(
    tx: &Connection,
    parent: &Run,
    spec: &ChildSpec<'_>,
    cfg: &RunConfig,
    now: OffsetDateTime,
) -> Result<usize> {
    let already: Option<String> = tx
        .query_row("SELECT run_id FROM runs WHERE run_id = ?1", params![spec.child_run_id], |r| {
            r.get(0)
        })
        .optional()
        .map_err(be)?;
    if already.is_some() {
        return Ok(0);
    }
    let parent_graph = load_graph(tx, parent)?;
    let journal = load_journal(tx, &parent.run_id)?;
    let input_template = parent_graph.steps.iter().find_map(|n| match n {
        Node::Subworkflow { id, input, .. } if id == spec.step_key => Some(input.clone()),
        Node::Subworkflow { .. }
        | Node::Task { .. }
        | Node::Agent { .. }
        | Node::Sleep { .. }
        | Node::SleepUntil { .. }
        | Node::WaitEvent { .. }
        | Node::Parallel { .. }
        | Node::Foreach { .. }
        | Node::Loop { .. } => None,
    });
    let input =
        input_template.flatten().map(|t| render(&t, &context_for(parent, &journal, &Scope::top())));

    let depth = parent.depth + 1;
    let definition =
        load_definition(tx, &parent.project, spec.definition_id, spec.definition_version)?;
    let graph = Graph::parse(&definition.body)?;

    // What stops the child before it starts. Each one is a child row that
    // ends `failed` at once rather than a parent transaction that errors, so
    // the parent step fails through its own `on_error` and a worker's
    // `complete` is never refused for a document it did not write.
    let refusal = if depth > cfg.max_depth {
        Some((
            format!(
                "run tree is deeper than max_depth {}: step \"{}\" of run {} would be depth {depth}",
                cfg.max_depth, spec.step_key, parent.run_id
            ),
            StepErrorClass::Fatal,
        ))
    } else {
        schema_violation(&graph, input.as_ref()).map(|m| (m, StepErrorClass::InvalidInput))
    };

    let mut child = Run {
        run_id: spec.child_run_id.to_string(),
        project: parent.project.clone(),
        definition_id: spec.definition_id.to_string(),
        definition_version: spec.definition_version,
        status: RunStatus::Running,
        cursor: Cursor {
            index: 0,
            at: Construct::End { status: RunStatus::Succeeded },
            skipped: false,
        },
        input,
        output: None,
        error: None,
        error_class: None,
        trace_id: parent.trace_id.clone(),
        root_span_id: new_span_id(),
        state: Value::Object(Map::new()),
        correlation: correlation_with(&parent.correlation, &parent.project, spec.child_run_id),
        idempotency_key: None,
        bindings: pin_bindings(tx, &parent.project, &graph)?,
        version: 0,
        steps_used: 0,
        max_steps: graph.limits.max_steps,
        cost_usd: 0.0,
        max_cost_usd: graph.limits.max_cost_usd,
        deadline_at: graph.limits.run_timeout_ms.map(|ms| after_ms(now, ms)),
        root_run_id: parent.root_run_id.clone(),
        parent_run_id: Some(parent.run_id.clone()),
        parent_step_key: Some(spec.step_key.to_string()),
        depth,
        created_at: now,
        updated_at: now,
    };

    let first = graph.start(&child, &Journal::default(), now);
    if let Ok(cursor) = &first {
        child.cursor = cursor.clone();
    }
    insert_run(tx, &child)?;
    emit(
        tx,
        spec.child_run_id,
        RunEventKind::RunStarted,
        json!({ "definition_id": spec.definition_id, "definition_version": spec.definition_version }),
        now,
    )?;
    sub_span(tx, parent, spec.step_key, spec.span_id, &child, "open", now, now)?;
    open_root_span(tx, &child, Some(spec.span_id), now)?;

    let child_rows = match (refusal, first) {
        (Some((message, class)), _) => {
            fail_run(tx, spec.child_run_id, &message, class, cfg, now)?;
            0
        }
        (None, Err(Refusal(message))) => {
            fail_run(tx, spec.child_run_id, &message, StepErrorClass::Fatal, cfg, now)?;
            0
        }
        (None, Ok(cursor)) => materialise(tx, spec.child_run_id, cursor, Move::Repair, cfg, now)?,
    };
    Ok(1 + child_rows)
}

fn correlation_with(base: &Value, project: &str, run_id: &str) -> Value {
    let mut map = match base {
        Value::Object(map) => map.clone(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => {
            Map::new()
        }
    };
    map.insert("project".into(), Value::String(project.to_string()));
    map.insert("run_id".into(), Value::String(run_id.to_string()));
    Value::Object(map)
}

/// Why an input does not satisfy the definition's `input_schema`, if it does
/// not.
fn schema_violation(graph: &Graph, input: Option<&Value>) -> Option<String> {
    let schema = graph.input_schema.as_ref()?;
    let validator = jsonschema::validator_for(schema).ok()?;
    let instance = input.cloned().unwrap_or(Value::Null);
    let mut errors = validator.iter_errors(&instance);
    let first = errors.next()?;
    let path = first.instance_path().to_string();
    let location = if path.is_empty() { "input".to_string() } else { format!("input{path}") };
    Some(format!("{location}: {first}"))
}

/// Every `subworkflow` step's `definition_id@version`, resolved now so a
/// redeploy mid-run cannot change a child graph under a running parent.
fn pin_bindings(tx: &Connection, project: &str, graph: &Graph) -> Result<Value> {
    let mut bindings = Map::new();
    for node in &graph.steps {
        if let Node::Subworkflow { id, definition, version, .. } = node {
            let pinned = match version {
                Some(v) => {
                    load_definition(tx, project, definition, *v)?;
                    *v
                }
                None => latest_version(tx, project, definition)?.ok_or_else(|| {
                    Error::NotFound { kind: "definition", id: definition.clone() }
                })?,
            };
            bindings.insert(id.clone(), json!({ "definition_id": definition, "version": pinned }));
        }
    }
    Ok(Value::Object(bindings))
}

/// Mark every attempt that is not terminal as failed, because the run it
/// belongs to is over. The journal never shows a live attempt on a dead run.
fn end_live_attempts(tx: &Connection, run: &Run, message: &str, now: OffsetDateTime) -> Result<()> {
    let live = read_attempts(
        tx,
        &format!(
            "SELECT {ATTEMPT_COLUMNS} FROM step_attempts
              WHERE run_id = ?1 AND status IN ('pending','leased','started','orphaned')"
        ),
        params![run.run_id],
    )?;
    for a in live {
        tx.execute(
            "UPDATE step_attempts SET status = 'failed', error = ?1, error_class = 'cancelled',
                    ended_at = ?2
              WHERE run_id = ?3 AND step_key = ?4 AND attempt = ?5",
            params![message, ns(now), a.run_id, a.step_key, a.attempt],
        )
        .map_err(be)?;
        emit(
            tx,
            &run.run_id,
            RunEventKind::StepFailed,
            json!({
                "step_key": a.step_key, "attempt": a.attempt, "class": "cancelled",
                "message": message, "attempts_left": 0,
            }),
            now,
        )?;
        let ended = Attempt {
            status: AttemptStatus::Failed,
            error: Some(message.to_string()),
            error_class: Some(StepErrorClass::Cancelled),
            ended_at: Some(now),
            ..a
        };
        close_step_span(tx, run, &ended, now)?;
    }
    Ok(())
}

/// End a run, and take its deadlines, its suspensions, its live attempts and
/// its children with it. Then move its parent, if it has one.
fn finish(
    tx: &Connection,
    run: &Run,
    status: RunStatus,
    cfg: &RunConfig,
    now: OffsetDateTime,
) -> Result<usize> {
    let changed = tx
        .execute(
            "UPDATE runs SET status = ?1, updated_at = ?2, version = version + 1
              WHERE run_id = ?3 AND status = 'running'",
            params![status.as_str(), ns(now), run.run_id],
        )
        .map_err(be)?;
    if changed == 0 {
        return Ok(0);
    }
    let mut rows = changed;

    // The output template is rendered once, here, against the whole journal.
    if status == RunStatus::Succeeded {
        let graph = load_graph(tx, run)?;
        if let Some(template) = &graph.output {
            let journal = load_journal(tx, &run.run_id)?;
            let output = render(template, &context_for(run, &journal, &Scope::top()));
            tx.execute(
                "UPDATE runs SET output = ?1 WHERE run_id = ?2",
                params![output.to_string(), run.run_id],
            )
            .map_err(be)?;
        }
    }
    let run = load_run(tx, &run.run_id)?;

    let reason = match status {
        RunStatus::Cancelled => "run cancelled".to_string(),
        RunStatus::Running | RunStatus::Succeeded | RunStatus::Failed | RunStatus::TimedOut => {
            format!("run {}", status.as_str())
        }
    };
    end_live_attempts(tx, &run, &reason, now)?;

    // The deletion the design enumerated at exactly this transaction and then
    // listed one table of three. The `waits` half is the one that costs money:
    // a leftover row matches an inbound key, and the next run on that order
    // waits for ever for an approval that was already spent on a dead run.
    let mut stmt = tx.prepare("SELECT step_key FROM waits WHERE run_id = ?1").map_err(be)?;
    let waiting: Vec<String> = stmt
        .query_map(params![run.run_id], |r| r.get(0))
        .map_err(be)?
        .collect::<std::result::Result<_, _>>()
        .map_err(be)?;
    drop(stmt);
    for step_key in waiting {
        if let Some(w) = load_wait(tx, &run.run_id, &step_key)? {
            wait_span(tx, &run, &w, "close", "cancelled", Some(now), json!({}), now)?;
        }
    }
    rows += tx.execute("DELETE FROM waits WHERE run_id = ?1", params![run.run_id]).map_err(be)?;
    rows += tx.execute("DELETE FROM timers WHERE run_id = ?1", params![run.run_id]).map_err(be)?;

    emit(
        tx,
        &run.run_id,
        RunEventKind::RunEnded,
        json!({ "status": status.as_str(), "error": run.error }),
        now,
    )?;
    let parent_span = parent_step_span(tx, &run)?;
    close_root_span(tx, &run, parent_span.as_deref(), now)?;

    // A terminal parent ends its children. Each child's own finish sees this
    // parent already terminal and does not try to move it.
    for child_id in child_run_ids(tx, &run.run_id, true)? {
        rows += cancel_tree(tx, &child_id, cfg, now)?;
    }

    // A terminal child moves the parent past its `subworkflow` step, or fails
    // that step, which then follows the step's own `on_error`.
    if let (Some(parent_id), Some(parent_step)) = (&run.parent_run_id, &run.parent_step_key) {
        let parent = load_run(tx, parent_id)?;
        if !parent.status.is_terminal() {
            // Listed out rather than closed with a catch-all: a construct
            // that can also hold a child run has to be decided here, not
            // defaulted to "no".
            let at_this_step = match &parent.cursor.at {
                Construct::Subworkflow { step, span_id, .. } if step == parent_step => {
                    Some(span_id.clone())
                }
                Construct::Subworkflow { .. }
                | Construct::Task { .. }
                | Construct::Agent { .. }
                | Construct::Sleep { .. }
                | Construct::SleepUntil { .. }
                | Construct::WaitEvent { .. }
                | Construct::Parallel { .. }
                | Construct::Foreach { .. }
                | Construct::Loop { .. }
                | Construct::End { .. } => None,
            };
            if let Some(span_id) = at_this_step {
                sub_span(tx, &parent, parent_step, &span_id, &run, "close", run.created_at, now)?;
                let graph = load_graph(tx, &parent)?;
                match status {
                    RunStatus::Succeeded => {
                        rows += advance_run(tx, &parent, &graph, cfg, now)?;
                    }
                    RunStatus::Running => {}
                    RunStatus::Failed | RunStatus::TimedOut | RunStatus::Cancelled => {
                        let class = run.error_class.unwrap_or(match status {
                            RunStatus::Cancelled => StepErrorClass::Cancelled,
                            RunStatus::Running
                            | RunStatus::Succeeded
                            | RunStatus::Failed
                            | RunStatus::TimedOut => StepErrorClass::Fatal,
                        });
                        let message = format!(
                            "step \"{parent_step}\" failed: child run {} {}: {}",
                            run.run_id,
                            status.as_str(),
                            run.error.clone().unwrap_or_default()
                        );
                        emit(
                            tx,
                            parent_id,
                            RunEventKind::StepFailed,
                            json!({
                                "step_key": parent_step, "attempt": 0, "class": class.as_str(),
                                "message": message, "attempts_left": 0,
                            }),
                            now,
                        )?;
                        match graph.on_error_of(parent_step).unwrap_or_default() {
                            OnError::FailRun => {
                                fail_run(tx, parent_id, &message, class, cfg, now)?;
                            }
                            OnError::Continue => {
                                rows += advance_run(tx, &parent, &graph, cfg, now)?;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(rows)
}

/// The parent step's span a child's root span nests under.
fn parent_step_span(tx: &Connection, run: &Run) -> Result<Option<String>> {
    let Some(parent_id) = &run.parent_run_id else {
        return Ok(None);
    };
    let parent = load_run(tx, parent_id)?;
    let span = match &parent.cursor.at {
        Construct::Subworkflow { step, span_id, .. }
            if Some(step) == run.parent_step_key.as_ref() =>
        {
            Some(span_id.clone())
        }
        Construct::Subworkflow { .. }
        | Construct::Task { .. }
        | Construct::Agent { .. }
        | Construct::Sleep { .. }
        | Construct::SleepUntil { .. }
        | Construct::WaitEvent { .. }
        | Construct::Parallel { .. }
        | Construct::Foreach { .. }
        | Construct::Loop { .. }
        | Construct::End { .. } => None,
    };
    Ok(span.or(Some(parent.root_span_id)))
}

/// Move a run past its current cursor.
fn advance_run(
    tx: &Connection,
    run: &Run,
    graph: &Graph,
    cfg: &RunConfig,
    now: OffsetDateTime,
) -> Result<usize> {
    let run = load_run(tx, &run.run_id)?;
    if run.status.is_terminal() {
        return Ok(0);
    }
    let journal = load_journal(tx, &run.run_id)?;
    match graph.advance(&run.cursor, &run, &journal, now) {
        Ok(next) => materialise(tx, &run.run_id, next, Move::Advance, cfg, now),
        Err(Refusal(message)) => {
            fail_run(tx, &run.run_id, &message, StepErrorClass::Fatal, cfg, now)?;
            Ok(0)
        }
    }
}

/// Cancel one run and every run under it, top down, so a child's finish
/// finds its parent already terminal.
fn cancel_tree(
    tx: &Connection,
    run_id: &str,
    cfg: &RunConfig,
    now: OffsetDateTime,
) -> Result<usize> {
    let run = load_run(tx, run_id)?;
    if run.status.is_terminal() {
        return Ok(0);
    }
    tx.execute(
        "UPDATE runs SET error = COALESCE(error, 'run cancelled'),
                         error_class = COALESCE(error_class, 'cancelled')
          WHERE run_id = ?1",
        params![run_id],
    )
    .map_err(be)?;
    let cursor = Cursor {
        index: run.cursor.index,
        at: Construct::End { status: RunStatus::Cancelled },
        skipped: false,
    };
    materialise(tx, run_id, cursor, Move::Advance, cfg, now)
}

// ---------------------------------------------------------------------------
// The cursor
// ---------------------------------------------------------------------------

/// Whether every slot the cursor requires has a terminal latest attempt with
/// no retry on the way, and there is at least one slot. A position with no
/// slots is decided by `settles_on_arrival` instead.
fn all_slots_settled(tx: &Connection, run: &Run, cursor: &Cursor) -> Result<bool> {
    let reqs = cursor_requirements(run.deadline_at, cursor);
    let slots: Vec<&Slot> = reqs.attempt_slots().collect();
    if slots.is_empty() {
        return Ok(false);
    }
    for slot in slots {
        let Some(last) = attempts_of_step(tx, &run.run_id, &slot.step_key)?.pop() else {
            return Ok(false);
        };
        if !last.status.is_terminal() {
            return Ok(false);
        }
        let retrying: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM timers WHERE run_id = ?1 AND step_key = ?2 AND kind = 'retry'",
                params![run.run_id, slot.step_key],
                |r| r.get(0),
            )
            .optional()
            .map_err(be)?;
        if retrying.is_some() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Move the cursor and write everything the new cursor requires, in one
/// transaction.
///
/// `Move::Repair` is the reconciler's entry point, and it runs the same code
/// with the cursor write skipped. There is no second list of rows to keep in
/// step with this one, which is the whole reason this function exists.
fn materialise(
    tx: &Connection,
    run_id: &str,
    cursor: Cursor,
    mode: Move,
    cfg: &RunConfig,
    now: OffsetDateTime,
) -> Result<usize> {
    let mut cursor = cursor;
    let mut mode = mode;
    let mut rows = 0usize;
    for _ in 0..cfg.materialise_budget {
        let run = load_run(tx, run_id)?;
        if run.status.is_terminal() {
            return Ok(rows);
        }
        if mode == Move::Advance {
            let changed = tx
                .execute(
                    "UPDATE runs SET cursor = ?1, version = version + 1, updated_at = ?2
                      WHERE run_id = ?3 AND version = ?4",
                    params![
                        serde_json::to_string(&cursor).map_err(be)?,
                        ns(now),
                        run_id,
                        run.version
                    ],
                )
                .map_err(be)?;
            if changed == 0 {
                return Err(Error::Retry(format!(
                    "run {run_id} moved underneath this transaction at version {}",
                    run.version
                )));
            }
            emit(
                tx,
                run_id,
                RunEventKind::RunCursor,
                json!({
                    "index": cursor.index,
                    "construct": cursor.at.kind().as_str(),
                    "step_key": cursor.at.step(),
                }),
                now,
            )?;
        }

        let run = load_run(tx, run_id)?;
        let reqs = cursor_requirements(run.deadline_at, &cursor);
        let applied = apply(tx, &run, &reqs, cfg, now)?;
        rows += applied.rows;
        let run = load_run(tx, run_id)?;
        if run.status.is_terminal() {
            return Ok(rows);
        }
        // Either an event was already on the shelf, so the wait is over inside
        // this same commit and the run never enters `waiting` at all, or the
        // position is one nothing will ever move because it asked for no work,
        // or every slot it asked for is already over.
        if !applied.delivered
            && !settles_on_arrival(&cursor)
            && !all_slots_settled(tx, &run, &cursor)?
        {
            return Ok(rows);
        }
        let graph = load_graph(tx, &run)?;
        let journal = load_journal(tx, run_id)?;
        cursor = match graph.advance(&cursor, &run, &journal, now) {
            Ok(next) => next,
            Err(Refusal(message)) => {
                fail_run(tx, run_id, &message, StepErrorClass::Fatal, cfg, now)?;
                return Ok(rows);
            }
        };
        mode = Move::Advance;
    }
    fail_run(
        tx,
        run_id,
        &format!(
            "run advanced {} times in one transaction without settling, which is materialise_budget",
            cfg.materialise_budget
        ),
        StepErrorClass::Fatal,
        cfg,
        now,
    )?;
    Ok(rows)
}

fn insert_run(tx: &Connection, r: &Run) -> Result<()> {
    tx.execute(
        "INSERT INTO runs (run_id, project, definition_id, definition_version, status, cursor,
                           input, output, error, error_class, trace_id, root_span_id, state,
                           correlation, idempotency_key, bindings, version, next_event_seq,
                           steps_used, max_steps, cost_usd, max_cost_usd, deadline_at,
                           root_run_id, parent_run_id, parent_step_key, depth, created_at,
                           updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,0,0,0,?17,0,?18,?19,
                 ?20,?21,?22,?23,?24,?24)",
        params![
            r.run_id,
            r.project,
            r.definition_id,
            i64::from(r.definition_version),
            r.status.as_str(),
            serde_json::to_string(&r.cursor).map_err(be)?,
            json(&r.input),
            json(&r.output),
            r.error,
            r.error_class.map(StepErrorClass::as_str),
            r.trace_id,
            r.root_span_id,
            r.state.to_string(),
            r.correlation.to_string(),
            r.idempotency_key,
            r.bindings.to_string(),
            r.max_steps,
            r.max_cost_usd,
            r.deadline_at.map(ns),
            r.root_run_id,
            r.parent_run_id,
            r.parent_step_key,
            r.depth,
            ns(r.created_at)
        ],
    )
    .map_err(be)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Leases
// ---------------------------------------------------------------------------

fn highest_attempt(tx: &Connection, run_id: &str, step_key: &str) -> Result<i64> {
    tx.query_row(
        "SELECT COALESCE(MAX(attempt), -1) FROM step_attempts WHERE run_id = ?1 AND step_key = ?2",
        params![run_id, step_key],
        |r| r.get(0),
    )
    .map_err(be)
}

/// The lease owner of record, checked rather than assumed. The call addresses
/// a `lease_id`; its run must still be running, the row it names must still be
/// the highest attempt for its step, and the attempt must not have ended. An
/// ended attempt whose ending lease is the caller's own is the caller's own
/// committed work, which is `already_applied`; any other ended attempt, or a
/// later attempt, is `fenced`.
///
/// All of it lives here rather than at the four call sites. When the ended
/// check was written out at two of the four, the two that lacked it let a
/// worker that had already succeeded reopen its attempt.
fn owner_of_record(tx: &Connection, lease_id: &str) -> Result<(Attempt, Run)> {
    let a = load_attempt_by_lease(tx, lease_id)?;
    let run = load_run(tx, &a.run_id)?;
    if run.status.is_terminal() {
        return Err(conflict(
            ConflictReason::RunTerminal,
            &a,
            run.status,
            format!("run {} has ended as {}", run.run_id, run.status.as_str()),
        ));
    }
    let highest = highest_attempt(tx, &a.run_id, &a.step_key)?;
    if a.attempt != highest {
        return Err(conflict(
            ConflictReason::Fenced,
            &a,
            run.status,
            format!(
                "lease {lease_id} holds attempt {} of step \"{}\" and attempt {highest} exists",
                a.attempt, a.step_key
            ),
        ));
    }
    if a.status.is_terminal() {
        let reason = if a.ended_by_lease_id.as_deref() == Some(lease_id) {
            ConflictReason::AlreadyApplied
        } else {
            ConflictReason::Fenced
        };
        return Err(conflict(
            reason,
            &a,
            run.status,
            format!(
                "lease {lease_id} names an attempt that already ended as {}",
                a.status.as_str()
            ),
        ));
    }
    Ok((a, run))
}

/// Restore an attempt to the live state it came from, extend its lease, and
/// swap its reclaim deadline back for a lease deadline.
///
/// The three calls that carry positive evidence (`started`, `heartbeat` and the
/// completing pair) all delete the reclaim timer inside the same transaction as
/// the state change they report.
fn restore(
    tx: &Connection,
    a: &Attempt,
    declare_started: bool,
    cfg: &RunConfig,
    now: OffsetDateTime,
) -> Result<LeaseAck> {
    let started_at = if declare_started { Some(a.started_at.unwrap_or(now)) } else { a.started_at };
    let status = if started_at.is_some() { AttemptStatus::Started } else { AttemptStatus::Leased };
    let lease_until = now + cfg.lease_ttl;
    tx.execute(
        "UPDATE step_attempts
            SET status = ?1, started_at = ?2, lease_until = ?3, orphaned_at = NULL,
                reclaim_until = NULL
          WHERE run_id = ?4 AND step_key = ?5 AND attempt = ?6",
        params![
            status.as_str(),
            started_at.map(ns),
            ns(lease_until),
            a.run_id,
            a.step_key,
            a.attempt
        ],
    )
    .map_err(be)?;
    tx.execute(
        "DELETE FROM timers WHERE run_id = ?1 AND step_key = ?2
                              AND kind IN ('lease_expiry', 'reclaim')",
        params![a.run_id, a.step_key],
    )
    .map_err(be)?;
    let run = load_run(tx, &a.run_id)?;
    let reqs = attempt_requirements(&a.step_key, status, Some(lease_until), None);
    apply(tx, &run, &reqs, cfg, now)?;
    Ok(LeaseAck { expires_at: lease_until, reclaimed: a.status == AttemptStatus::Orphaned })
}

/// Clear the deadlines a finished attempt no longer needs.
fn clear_step_timers(tx: &Connection, run_id: &str, step_key: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM timers WHERE run_id = ?1 AND step_key = ?2
                              AND kind IN ('lease_expiry', 'reclaim', 'step_timeout', 'retry')",
        params![run_id, step_key],
    )
    .map_err(be)?;
    Ok(())
}

/// Move the run on if everything the current cursor asked for is terminal.
fn advance_if_settled(
    tx: &Connection,
    run_id: &str,
    cfg: &RunConfig,
    now: OffsetDateTime,
) -> Result<()> {
    let run = load_run(tx, run_id)?;
    if run.status.is_terminal() {
        return Ok(());
    }
    if !all_slots_settled(tx, &run, &run.cursor)? {
        return Ok(());
    }
    let graph = load_graph(tx, &run)?;
    advance_run(tx, &run, &graph, cfg, now)?;
    Ok(())
}

/// End a run on an error nothing handled.
fn fail_run(
    tx: &Connection,
    run_id: &str,
    message: &str,
    class: StepErrorClass,
    cfg: &RunConfig,
    now: OffsetDateTime,
) -> Result<()> {
    let run = load_run(tx, run_id)?;
    if run.status.is_terminal() {
        return Ok(());
    }
    tx.execute(
        "UPDATE runs SET error = ?1, error_class = ?2 WHERE run_id = ?3",
        params![message, class.as_str(), run_id],
    )
    .map_err(be)?;
    let cursor = Cursor {
        index: run.cursor.index,
        at: Construct::End { status: RunStatus::Failed },
        skipped: false,
    };
    materialise(tx, run_id, cursor, Move::Advance, cfg, now)?;
    Ok(())
}

/// How many attempts a step has left under its policy, counting the rows that
/// spent budget.
fn attempts_left(tx: &Connection, run_id: &str, step_key: &str, max_attempts: u32) -> Result<u32> {
    let spent: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM step_attempts
              WHERE run_id = ?1 AND step_key = ?2 AND budget_spent = 1",
            params![run_id, step_key],
            |r| r.get(0),
        )
        .map_err(be)?;
    Ok(max_attempts.saturating_sub(u32::try_from(spent).unwrap_or(u32::MAX)))
}

/// How many ended attempts of this step failed with exactly the input this one
/// carries.
///
/// The comparison is against the stored text rather than a re-serialised
/// value, so it cannot turn on how a `Value` happens to round-trip. `IS` is
/// used rather than `=` because a step with no input template stores NULL, and
/// under `=` a NULL never equals itself, which would make the case that repeats
/// most readily the one case the breaker could never see.
fn identical_failures(tx: &Connection, run_id: &str, step_key: &str, attempt: i64) -> Result<i64> {
    tx.query_row(
        "SELECT COUNT(*) FROM step_attempts f
          WHERE f.run_id = ?1 AND f.step_key = ?2
            AND f.status IN ('failed','timed_out')
            AND f.input IS (SELECT c.input FROM step_attempts c
                             WHERE c.run_id = ?1 AND c.step_key = ?2 AND c.attempt = ?3)",
        params![run_id, step_key, attempt],
        |r| r.get(0),
    )
    .map_err(be)
}

/// End the run when this step has repeated itself, and say whether it did.
///
/// Stands in front of a retry and nowhere else. A failure that ends the run or
/// advances past it is not a repetition, and a breaker that fired there would
/// replace the step's own error with a less useful one.
///
/// Ends the run the way a spent `max_steps` or `max_cost_usd` does, ignoring
/// the node's `on_error`: a run that has stopped making progress is a runaway,
/// and `on_error: continue` carrying on past it is the thing the breaker
/// exists to stop.
fn trip_repeat_breaker(
    tx: &Connection,
    run: &Run,
    a: &Attempt,
    cfg: &RunConfig,
    now: OffsetDateTime,
) -> Result<bool> {
    if cfg.max_step_repeats == 0 {
        return Ok(false);
    }
    let repeats = identical_failures(tx, &a.run_id, &a.step_key, a.attempt)?;
    if repeats < i64::from(cfg.max_step_repeats) {
        return Ok(false);
    }
    let message = format!(
        "run exceeded max_step_repeats: step \"{}\" failed {repeats} times with an identical \
         input and the run made no progress; fix what keeps failing, or raise \
         max_step_repeats if this step really does need that many attempts at the same input",
        a.step_key
    );
    fail_run(tx, &run.run_id, &message, StepErrorClass::Fatal, cfg, now)?;
    Ok(true)
}

/// Record a failed attempt and decide what happens next: a retry timer, an
/// immediate requeue, a requeue after one lease length, or the node's
/// `on_error`. Section 7.6 and 7.7.
#[allow(clippy::too_many_arguments)]
fn record_failure(
    tx: &Connection,
    run: &Run,
    a: &Attempt,
    status: AttemptStatus,
    error: &StepError,
    ended_by_lease_id: Option<&str>,
    disposition: Disposition,
    cfg: &RunConfig,
    now: OffsetDateTime,
) -> Result<u32> {
    let graph = load_graph(tx, run)?;
    let policy = graph.policy(&a.step_key);
    let retry = policy.map(|p| p.retry.clone()).unwrap_or_default();
    let on_error = policy.map_or_else(OnError::default, |p| p.on_error);

    let spends = match disposition {
        Disposition::Policy { .. } => error.class != StepErrorClass::Unservable,
        Disposition::RequeueFree => false,
        Disposition::FailRun => true,
    };
    tx.execute(
        "UPDATE step_attempts
            SET status = ?1, error = ?2, error_class = ?3, error_details = ?4, ended_at = ?5,
                ended_by_lease_id = ?6, budget_spent = ?7
          WHERE run_id = ?8 AND step_key = ?9 AND attempt = ?10",
        params![
            status.as_str(),
            error.message,
            error.class.as_str(),
            json(&error.details),
            ns(now),
            ended_by_lease_id,
            i64::from(spends),
            a.run_id,
            a.step_key,
            a.attempt
        ],
    )
    .map_err(be)?;
    clear_step_timers(tx, &a.run_id, &a.step_key)?;
    let left = attempts_left(tx, &a.run_id, &a.step_key, retry.max_attempts)?;
    emit(
        tx,
        &run.run_id,
        RunEventKind::StepFailed,
        json!({
            "step_key": a.step_key, "attempt": a.attempt, "class": error.class.as_str(),
            "message": error.message, "attempts_left": left,
        }),
        now,
    )?;
    let ended = Attempt {
        status,
        error: Some(error.message.clone()),
        error_class: Some(error.class),
        error_details: error.details.clone(),
        ended_at: Some(now),
        ..a.clone()
    };
    close_step_span(tx, run, &ended, now)?;

    let retry_timer = |due_at: OffsetDateTime| -> Result<()> {
        tx.execute(
            "INSERT INTO timers (run_id, step_key, kind, due_at, created_at)
             VALUES (?1,?2,'retry',?3,?4)
               ON CONFLICT (run_id, step_key, kind) DO UPDATE SET due_at = ?3, created_at = ?4",
            params![run.run_id, a.step_key, ns(due_at), ns(now)],
        )
        .map_err(be)?;
        Ok(())
    };
    let failure_message = format!("step \"{}\" failed: {}", a.step_key, error.message);
    let apply_on_error = || -> Result<()> {
        match on_error {
            OnError::FailRun => fail_run(tx, &run.run_id, &failure_message, error.class, cfg, now),
            OnError::Continue => advance_if_settled(tx, &run.run_id, cfg, now),
        }
    };

    // The breaker only ever stands in front of one of the three paths below
    // that put the same step back on the queue. `ended` is checked before each
    // of them rather than once up front, so a failure that was going to end or
    // advance the run keeps its own error.
    let repeating = || trip_repeat_breaker(tx, run, a, cfg, now);

    match disposition {
        Disposition::Policy { retry_after_ms } => match error.class {
            StepErrorClass::Retryable => {
                if left > 0 {
                    if !repeating()? {
                        let spent = retry.max_attempts - left;
                        let delay = retry.delay_ms(spent).max(retry_after_ms.unwrap_or(0));
                        retry_timer(after_ms(now, delay))?;
                    }
                } else {
                    apply_on_error()?;
                }
            }
            StepErrorClass::Unservable => {
                if !repeating()? {
                    retry_timer(now + cfg.lease_ttl)?;
                }
            }
            StepErrorClass::Fatal | StepErrorClass::InvalidInput | StepErrorClass::Cancelled => {
                apply_on_error()?;
            }
        },
        Disposition::RequeueFree => {
            if !repeating()? {
                requeue(tx, run, &graph, &a.step_key, a.attempt + 1, now)?;
            }
        }
        Disposition::FailRun => {
            fail_run(tx, &run.run_id, &failure_message, error.class, cfg, now)?;
        }
    }
    Ok(left)
}

/// Whether any claimable work in this run's tree is pending right now.
fn runnable_in_tree(tx: &Connection, root_run_id: &str) -> Result<bool> {
    let found: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM step_attempts a JOIN runs r ON r.run_id = a.run_id
              WHERE r.root_run_id = ?1 AND r.status = 'running' AND a.status = 'pending'
                AND a.kind IN ('task','agent')
                AND NOT EXISTS (SELECT 1 FROM timers t WHERE t.run_id = a.run_id
                                   AND t.step_key = a.step_key AND t.kind = 'retry')
              LIMIT 1",
            params![root_run_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(be)?;
    Ok(found.is_some())
}

// ---------------------------------------------------------------------------
// The due-timer tick
// ---------------------------------------------------------------------------

/// Handle one due deadline.
///
/// The match is exhaustive with no catch-all. An eighth `TimerKind` stops this
/// file compiling until somebody says what fires.
fn fire(tx: &Connection, t: &Timer, cfg: &RunConfig, now: OffsetDateTime) -> Result<()> {
    let created_at: Option<i64> = tx
        .query_row(
            "SELECT created_at FROM timers WHERE run_id = ?1 AND step_key = ?2 AND kind = ?3",
            params![t.run_id, t.step_key, t.kind.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(be)?;
    tx.execute(
        "DELETE FROM timers WHERE run_id = ?1 AND step_key = ?2 AND kind = ?3",
        params![t.run_id, t.step_key, t.kind.as_str()],
    )
    .map_err(be)?;
    let run = load_run(tx, &t.run_id)?;
    if run.status.is_terminal() {
        return Ok(());
    }

    match t.kind {
        TimerKind::Sleep => {
            let at_this_step = match &run.cursor.at {
                Construct::Sleep { step, .. } | Construct::SleepUntil { step, .. } => {
                    *step == t.step_key
                }
                Construct::Task { .. }
                | Construct::Agent { .. }
                | Construct::WaitEvent { .. }
                | Construct::Parallel { .. }
                | Construct::Foreach { .. }
                | Construct::Loop { .. }
                | Construct::Subworkflow { .. }
                | Construct::End { .. } => false,
            };
            if at_this_step {
                sleep_span(tx, &run, &t.step_key, created_at.map_or(now, at), now)?;
                let graph = load_graph(tx, &run)?;
                advance_run(tx, &run, &graph, cfg, now)?;
            }
        }
        TimerKind::LeaseExpiry => {
            // Both live states become `orphaned`. The status guard is the whole
            // of why this terminates: `orphaned` is not in it, so the deadline
            // can never be re-stamped on the same attempt.
            for a in attempts_in(tx, &t.run_id, &t.step_key, "'leased','started'")? {
                if a.lease_until.is_none_or(|u| u > now) {
                    continue;
                }
                let reclaim_until = now + cfg.reclaim_window;
                tx.execute(
                    "UPDATE step_attempts SET status = 'orphaned', orphaned_at = ?1, reclaim_until = ?2
                      WHERE run_id = ?3 AND step_key = ?4 AND attempt = ?5",
                    params![ns(now), ns(reclaim_until), a.run_id, a.step_key, a.attempt],
                )
                .map_err(be)?;
                let reqs = attempt_requirements(
                    &a.step_key,
                    AttemptStatus::Orphaned,
                    None,
                    Some(reclaim_until),
                );
                apply(tx, &run, &reqs, cfg, now)?;
            }
        }
        TimerKind::Reclaim => {
            // The query selects attempts still `orphaned`, which means the
            // window closed with no contact. An attempt a heartbeat, a
            // complete or a fail restored is not `orphaned` any more, so this
            // loop never sees it and the deleted timer row was a no-op.
            for a in attempts_in(tx, &t.run_id, &t.step_key, "'orphaned'")? {
                // The lease id stays on the row, so a worker that comes back
                // holding it is answered `fenced` rather than "no such lease".
                let disposition = match (a.started_at, a.effect) {
                    (None, Effect::AtLeastOnce | Effect::AtMostOnce)
                    | (Some(_), Effect::AtLeastOnce) => Disposition::RequeueFree,
                    (Some(_), Effect::AtMostOnce) => Disposition::FailRun,
                };
                let error = match disposition {
                    Disposition::FailRun => StepError {
                        class: StepErrorClass::Fatal,
                        message: format!(
                            "step \"{}\" was started and its outcome is unknown",
                            a.step_key
                        ),
                        details: None,
                        retry_after_ms: None,
                    },
                    Disposition::RequeueFree | Disposition::Policy { .. } => StepError {
                        class: StepErrorClass::Retryable,
                        message: "worker lease expired".into(),
                        details: None,
                        retry_after_ms: None,
                    },
                };
                record_failure(
                    tx,
                    &run,
                    &a,
                    AttemptStatus::Failed,
                    &error,
                    None,
                    disposition,
                    cfg,
                    now,
                )?;
            }
        }
        TimerKind::RunTimeout => {
            tx.execute(
                "UPDATE runs SET error = ?1, error_class = 'fatal' WHERE run_id = ?2",
                params!["run exceeded run_timeout", t.run_id],
            )
            .map_err(be)?;
            let cursor = Cursor {
                index: run.cursor.index,
                at: Construct::End { status: RunStatus::TimedOut },
                skipped: false,
            };
            materialise(tx, &t.run_id, cursor, Move::Advance, cfg, now)?;
        }
        TimerKind::StepTimeout => {
            for a in
                attempts_in(tx, &t.run_id, &t.step_key, "'pending','leased','started','orphaned'")?
            {
                let error = StepError {
                    class: StepErrorClass::Retryable,
                    message: "step exceeded step_timeout".into(),
                    details: None,
                    retry_after_ms: None,
                };
                record_failure(
                    tx,
                    &run,
                    &a,
                    AttemptStatus::TimedOut,
                    &error,
                    None,
                    Disposition::Policy { retry_after_ms: None },
                    cfg,
                    now,
                )?;
            }
        }
        TimerKind::WaitTimeout => {
            // Still present means the wait expired. Absent means an event
            // already landed, so the deleted row was a no-op.
            if load_wait(tx, &t.run_id, &t.step_key)?.is_some() {
                settle_wait(
                    tx,
                    &run,
                    &t.step_key,
                    AttemptStatus::TimedOut,
                    None,
                    Some("wait expired"),
                    None,
                    now,
                )?;
                let on_timeout = match &run.cursor.at {
                    Construct::WaitEvent { step, on_timeout, .. } if *step == t.step_key => {
                        *on_timeout
                    }
                    Construct::WaitEvent { .. }
                    | Construct::Task { .. }
                    | Construct::Agent { .. }
                    | Construct::Sleep { .. }
                    | Construct::SleepUntil { .. }
                    | Construct::Parallel { .. }
                    | Construct::Foreach { .. }
                    | Construct::Loop { .. }
                    | Construct::Subworkflow { .. }
                    | Construct::End { .. } => WaitTimeout::Continue,
                };
                match on_timeout {
                    WaitTimeout::Fail => {
                        fail_run(
                            tx,
                            &t.run_id,
                            &format!("step \"{}\" timed out waiting for its event", t.step_key),
                            StepErrorClass::Fatal,
                            cfg,
                            now,
                        )?;
                    }
                    WaitTimeout::Continue => {
                        let graph = load_graph(tx, &run)?;
                        advance_run(tx, &run, &graph, cfg, now)?;
                    }
                }
            }
        }
        TimerKind::Retry => {
            let Some(last) = attempts_of_step(tx, &t.run_id, &t.step_key)?.pop() else {
                return Ok(());
            };
            if !last.status.is_terminal() {
                return Ok(());
            }
            let at_this_step = cursor_requirements(run.deadline_at, &run.cursor)
                .attempt_slots()
                .any(|s| s.step_key == t.step_key);
            if at_this_step {
                let graph = load_graph(tx, &run)?;
                requeue(tx, &run, &graph, &t.step_key, last.attempt + 1, now)?;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Route one inbound event: to the wait expiring soonest on its key, or to
/// one run's own wait when `target` names it.
fn route_event(
    tx: &Connection,
    event: &InboundEvent,
    target: Option<&str>,
    cfg: &RunConfig,
    now: OffsetDateTime,
) -> Result<Delivery> {
    let seen: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM parked_events WHERE project = ?1 AND event_id = ?2",
            params![event.project, event.event_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(be)?;

    // The lookup joins `runs` and matches only a non-terminal one, so a row a
    // cancelled run left behind cannot eat a live run's approval, and it
    // matches only a wait still inside its window: the nearest expiry is the
    // stalest wait once the deadline has passed, and a `wait_timeout` fires on
    // a tick, so there is always a gap between a wait expiring and its row
    // being settled.
    let waiting: Option<(String, String)> = tx
        .query_row(
            "SELECT w.run_id, w.step_key FROM waits w
               JOIN runs r ON r.run_id = w.run_id
              WHERE w.project = ?1 AND w.event = ?2 AND w.correlation_key = ?3
                AND r.status = 'running' AND w.expires_at > ?4
                AND (?5 IS NULL OR w.run_id = ?5)
              ORDER BY w.expires_at LIMIT 1",
            params![event.project, event.event, event.correlation_key, ns(now), target],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(be)?;

    // A supplied id is a promise about identity and is honoured on both
    // paths. A derived id is a guess made from bytes: good enough to stop a
    // spare event parking, not good enough to swallow a delivery.
    if seen.is_some() && (!event.id_is_derived || waiting.is_none()) {
        return Ok(Delivery { outcome: EventOutcome::Duplicate, event_id: event.event_id.clone() });
    }

    let outcome = match waiting {
        Some((run_id, step_key)) => {
            if seen.is_some() {
                tx.execute(
                    "UPDATE parked_events
                        SET consumed_by_run_id = ?1, received_at = ?2, expires_at = ?3
                      WHERE project = ?4 AND event_id = ?5",
                    params![run_id, ns(now), ns(now + cfg.park_ttl), event.project, event.event_id],
                )
                .map_err(be)?;
            } else {
                // A delivery record: no payload, and it exists so the
                // provider's fifth retry is a no-op rather than a spare event
                // that jumps the next run's approval.
                tx.execute(
                    "INSERT INTO parked_events (project, event, correlation_key, event_id,
                                                id_is_derived, payload, received_at, expires_at,
                                                consumed_by_run_id)
                     VALUES (?1,?2,?3,?4,?5,NULL,?6,?7,?8)",
                    params![
                        event.project,
                        event.event,
                        event.correlation_key,
                        event.event_id,
                        i64::from(event.id_is_derived),
                        ns(now),
                        ns(now + cfg.park_ttl),
                        run_id
                    ],
                )
                .map_err(be)?;
            }
            let run = load_run(tx, &run_id)?;
            settle_wait(
                tx,
                &run,
                &step_key,
                AttemptStatus::Succeeded,
                event.payload.clone(),
                None,
                Some(&event.event_id),
                now,
            )?;
            let graph = load_graph(tx, &run)?;
            advance_run(tx, &run, &graph, cfg, now)?;
            EventOutcome::Delivered
        }
        None => {
            tx.execute(
                "INSERT INTO parked_events (project, event, correlation_key, event_id,
                                            id_is_derived, payload, received_at, expires_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    event.project,
                    event.event,
                    event.correlation_key,
                    event.event_id,
                    i64::from(event.id_is_derived),
                    json(&event.payload),
                    ns(now),
                    ns(now + cfg.park_ttl)
                ],
            )
            .map_err(be)?;
            EventOutcome::Parked
        }
    };
    Ok(Delivery { outcome, event_id: event.event_id.clone() })
}

// ---------------------------------------------------------------------------
// Definitions
// ---------------------------------------------------------------------------

/// Store a document under the rule of section 4 and answer `(version,
/// assigned)`.
fn store_definition(
    tx: &Connection,
    def: &DefinitionDocument,
    cfg: &RunConfig,
    now: OffsetDateTime,
) -> Result<(u32, bool)> {
    let graph = Graph::parse(&def.body)?;
    validate(&graph, &def.id, cfg.materialise_budget)?;
    let hash = body_hash(&def.body);
    let same_body: Option<i64> = tx
        .query_row(
            "SELECT version FROM definitions WHERE project = ?1 AND id = ?2 AND body_hash = ?3
              ORDER BY version LIMIT 1",
            params![def.project, def.id, hash],
            |r| r.get(0),
        )
        .optional()
        .map_err(be)?;

    let (version, assigned) = match def.version {
        None => match same_body {
            Some(v) => return Ok((v as u32, false)),
            None => (latest_version(tx, &def.project, &def.id)?.map_or(1, |v| v + 1), true),
        },
        Some(named) => {
            let taken: Option<String> = tx
                .query_row(
                    "SELECT body_hash FROM definitions
                      WHERE project = ?1 AND id = ?2 AND version = ?3",
                    params![def.project, def.id, i64::from(named)],
                    |r| r.get(0),
                )
                .optional()
                .map_err(be)?;
            match taken {
                Some(existing) if existing == hash => return Ok((named, false)),
                Some(_) => {
                    return Err(Error::VersionTaken {
                        project: def.project.clone(),
                        id: def.id.clone(),
                        version: named,
                    })
                }
                None => (named, false),
            }
        }
    };
    tx.execute(
        "INSERT INTO definitions (project, id, version, kind, body, body_hash, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            def.project,
            def.id,
            i64::from(version),
            def.kind.as_str(),
            def.body.to_string(),
            hash,
            ns(now)
        ],
    )
    .map_err(be)?;
    Ok((version, assigned))
}

/// Whether the reconciler has to write the retry timer a crash lost: the
/// step's latest attempt failed with budget left, or is waiting to be
/// requeued as unservable, and nothing is counting down for it.
fn awaiting_retry(tx: &Connection, run: &Run, graph: &Graph, step_key: &str) -> Result<bool> {
    let Some(last) = attempts_of_step(tx, &run.run_id, step_key)?.pop() else {
        return Ok(false);
    };
    Ok(match (last.status, last.error_class) {
        (AttemptStatus::Failed | AttemptStatus::TimedOut, Some(StepErrorClass::Retryable)) => {
            let max = graph.policy(step_key).map_or(1, |p| p.retry.max_attempts);
            attempts_left(tx, &run.run_id, step_key, max)? > 0
        }
        (AttemptStatus::Failed | AttemptStatus::TimedOut, Some(StepErrorClass::Unservable)) => true,
        (
            AttemptStatus::Failed | AttemptStatus::TimedOut,
            Some(StepErrorClass::Fatal | StepErrorClass::InvalidInput | StepErrorClass::Cancelled)
            | None,
        )
        | (
            AttemptStatus::Pending
            | AttemptStatus::Leased
            | AttemptStatus::Started
            | AttemptStatus::Orphaned
            | AttemptStatus::Succeeded
            | AttemptStatus::Skipped,
            _,
        ) => false,
    })
}

// ---------------------------------------------------------------------------
// Erasure and retention
// ---------------------------------------------------------------------------

/// How many run trees one retention or erasure transaction deletes. The store
/// has one connection behind one mutex, so a first pass over a year of runs
/// done in a single transaction would hold every claim and every heartbeat for
/// as long as it took.
const RETENTION_BATCH: usize = 200;

/// The `user_id` a run's correlation carries, as SQL. Guarded so a column that
/// is somehow not JSON reads as no subject rather than failing the statement,
/// because a statement that fails is an erasure that does not happen.
const CORRELATION_USER: &str =
    "CASE WHEN json_valid(correlation) THEN json_extract(correlation, '$.user_id') END";

/// Whether any run in the tree under `root` has not ended.
fn tree_is_live(tx: &Connection, root: &str) -> Result<bool> {
    let live: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM runs WHERE root_run_id = ?1 AND status = 'running' LIMIT 1",
            params![root],
            |r| r.get(0),
        )
        .optional()
        .map_err(be)?;
    Ok(live.is_some())
}

/// Delete one run tree and every row that hangs off it, and answer
/// `(runs, rows)`. The caller has already checked that nothing in the tree is
/// running.
///
/// The list of tables is the schema's list of tables keyed by run, plus the
/// parked events those runs consumed. A delivery record dropped here means a
/// provider's late retry of that event parks as new rather than being refused
/// as a duplicate; it is kept for at most a day either way, and it carries the
/// event's payload and correlation key, which are the person's data as much as
/// the run's input is.
fn delete_tree(tx: &Connection, root: &str) -> Result<(usize, usize)> {
    const TREE: &str = "SELECT run_id FROM runs WHERE root_run_id = ?1";
    let mut rows = 0;
    for table in ["step_attempts", "run_events", "timers", "waits", "span_outbox"] {
        rows += tx
            .execute(&format!("DELETE FROM {table} WHERE run_id IN ({TREE})"), params![root])
            .map_err(be)?;
    }
    rows += tx
        .execute(
            &format!("DELETE FROM parked_events WHERE consumed_by_run_id IN ({TREE})"),
            params![root],
        )
        .map_err(be)?;
    let runs = tx.execute("DELETE FROM runs WHERE root_run_id = ?1", params![root]).map_err(be)?;
    Ok((runs, rows + runs))
}

// ---------------------------------------------------------------------------
// The trait
// ---------------------------------------------------------------------------

#[async_trait]
impl RunStore for SqliteRunStore {
    async fn migrate(&self) -> Result<()> {
        self.with_write(|conn| {
            conn.execute_batch(SCHEMA).map_err(be)?;
            conn.execute(
                "INSERT OR IGNORE INTO run_schema_version (version, applied_at) VALUES (?1, ?2)",
                params![SCHEMA_VERSION, ns(OffsetDateTime::now_utc())],
            )
            .map_err(be)?;
            Ok(())
        })
        .await
    }

    async fn ping(&self) -> Result<()> {
        // Bounded, because the store is one connection behind one lock and a
        // probe that waited behind a long write would hang the probe itself.
        // The read still finishes once the lock frees; it is one row.
        let read = self.with_write(|conn| {
            conn.query_row("SELECT version FROM run_schema_version LIMIT 1", [], |_| Ok(()))
                .map_err(be)
        });
        match tokio::time::timeout(PING_WITHIN, read).await {
            Ok(answer) => answer,
            Err(_) => Err(Error::Backend(format!(
                "the run store did not answer within {} s; a write is holding it",
                PING_WITHIN.as_secs()
            ))),
        }
    }

    fn notify(&self) -> Arc<Notify> {
        Arc::clone(&self.notify)
    }

    fn runnable(&self) -> Arc<Notify> {
        Arc::clone(&self.runnable)
    }

    async fn put_definition(&self, def: DefinitionDocument) -> Result<()> {
        self.assign_definition(def).await.map(|_| ())
    }

    async fn assign_definition(&self, def: DefinitionDocument) -> Result<(u32, bool)> {
        let cfg = self.config;
        self.with_write(move |conn| {
            let tx = conn.transaction().map_err(be)?;
            let out = store_definition(&tx, &def, &cfg, OffsetDateTime::now_utc())?;
            tx.commit().map_err(be)?;
            Ok(out)
        })
        .await
    }

    async fn get_definition(&self, project: &str, id: &str, version: u32) -> Result<Definition> {
        let (project, id) = (project.to_string(), id.to_string());
        self.with_write(move |conn| load_definition(conn, &project, &id, version)).await
    }

    async fn latest_definition(&self, project: &str, id: &str) -> Result<Definition> {
        let (project, id) = (project.to_string(), id.to_string());
        self.with_write(move |conn| {
            let version = latest_version(conn, &project, &id)?
                .ok_or_else(|| Error::NotFound { kind: "definition", id: id.clone() })?;
            load_definition(conn, &project, &id, version)
        })
        .await
    }

    async fn list_definitions(&self, project: &str) -> Result<Vec<Definition>> {
        let project = project.to_string();
        self.with_write(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, version FROM definitions WHERE project = ?1 ORDER BY id, version",
                )
                .map_err(be)?;
            let keys: Vec<(String, i64)> = stmt
                .query_map(params![project], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(be)?
                .collect::<std::result::Result<_, _>>()
                .map_err(be)?;
            drop(stmt);
            let mut out = Vec::with_capacity(keys.len());
            for (id, version) in keys {
                out.push(load_definition(conn, &project, &id, version as u32)?);
            }
            Ok(out)
        })
        .await
    }

    async fn start_run(&self, req: StartRun, now: OffsetDateTime) -> Result<Run> {
        let cfg = self.config;
        self.with_write_waking(Wake::Workers, move |conn| {
            let tx = conn.transaction().map_err(be)?;
            if let Some(key) = &req.idempotency_key {
                let existing: Option<String> = tx
                    .query_row(
                        "SELECT run_id FROM runs WHERE project = ?1 AND idempotency_key = ?2",
                        params![req.project, key],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(be)?;
                if let Some(run_id) = existing {
                    return load_run(&tx, &run_id);
                }
            }
            let version = match req.definition_version {
                Some(v) => v,
                None => {
                    latest_version(&tx, &req.project, &req.definition_id)?.ok_or_else(|| {
                        Error::NotFound { kind: "definition", id: req.definition_id.clone() }
                    })?
                }
            };
            let definition = load_definition(&tx, &req.project, &req.definition_id, version)?;
            let graph = Graph::parse(&definition.body)?;
            if let Some(message) = schema_violation(&graph, req.input.as_ref()) {
                return Err(Error::Invalid(format!(
                    "input does not satisfy input_schema: {message}"
                )));
            }
            let run_id = new_id("run");
            let mut run = Run {
                run_id: run_id.clone(),
                project: req.project.clone(),
                definition_id: req.definition_id.clone(),
                definition_version: version,
                status: RunStatus::Running,
                cursor: Cursor {
                    index: 0,
                    at: Construct::End { status: RunStatus::Succeeded },
                    skipped: false,
                },
                input: req.input.clone(),
                output: None,
                error: None,
                error_class: None,
                trace_id: new_trace_id(),
                root_span_id: new_span_id(),
                state: Value::Object(Map::new()),
                correlation: correlation_with(
                    req.correlation.as_ref().unwrap_or(&Value::Null),
                    &req.project,
                    &run_id,
                ),
                idempotency_key: req.idempotency_key.clone(),
                bindings: pin_bindings(&tx, &req.project, &graph)?,
                version: 0,
                steps_used: 0,
                max_steps: graph.limits.max_steps,
                cost_usd: 0.0,
                max_cost_usd: graph.limits.max_cost_usd,
                deadline_at: graph.limits.run_timeout_ms.map(|ms| after_ms(now, ms)),
                root_run_id: run_id.clone(),
                parent_run_id: None,
                parent_step_key: None,
                depth: 0,
                created_at: now,
                updated_at: now,
            };
            let first = graph.start(&run, &Journal::default(), now);
            if let Ok(cursor) = &first {
                run.cursor = cursor.clone();
            }
            match insert_run(&tx, &run) {
                Ok(()) => {}
                Err(Error::Backend(message)) if message.contains("idempotency_key") => {
                    // The unique index is the guarantee; the check above is
                    // the fast path. A key that landed between the two is
                    // this branch.
                    let key = req.idempotency_key.clone().unwrap_or_default();
                    let winner: String = tx
                        .query_row(
                            "SELECT run_id FROM runs WHERE project = ?1 AND idempotency_key = ?2",
                            params![req.project, key],
                            |r| r.get(0),
                        )
                        .map_err(be)?;
                    return load_run(&tx, &winner);
                }
                Err(e) => return Err(e),
            }
            emit(
                &tx,
                &run_id,
                RunEventKind::RunStarted,
                json!({ "definition_id": req.definition_id, "definition_version": version }),
                now,
            )?;
            open_root_span(&tx, &run, None, now)?;
            match first {
                Ok(cursor) => {
                    // The cursor is already where the row says it is, so this
                    // is the repair path's own call: write what it requires
                    // and nothing else.
                    materialise(&tx, &run_id, cursor, Move::Repair, &cfg, now)?;
                }
                Err(Refusal(message)) => {
                    fail_run(&tx, &run_id, &message, StepErrorClass::Fatal, &cfg, now)?;
                }
            }
            let run = load_run(&tx, &run_id)?;
            tx.commit().map_err(be)?;
            Ok(run)
        })
        .await
    }

    async fn get_run(&self, run_id: &str) -> Result<Run> {
        let run_id = run_id.to_string();
        self.with_write(move |conn| load_run(conn, &run_id)).await
    }

    async fn list_runs(
        &self,
        filter: RunFilter,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<RunPage> {
        self.with_write(move |conn| {
            let (after_at, after_id) = match &cursor {
                Some(c) => {
                    let bad =
                        || Error::Invalid(format!("cursor \"{c}\" is not one this store issued"));
                    let (at_text, id) = c.split_once(':').ok_or_else(bad)?;
                    let at_ns: i64 = at_text.parse().map_err(|_| bad())?;
                    (Some(at_ns), Some(id.to_string()))
                }
                None => (None, None),
            };
            let limit = limit.max(1);
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {RUN_COLUMNS} FROM runs
                      WHERE project = ?1
                        AND (?2 IS NULL OR status = ?2)
                        AND (?3 IS NULL OR definition_id = ?3)
                        AND (?4 IS NULL OR updated_at < ?4
                             OR (updated_at = ?4 AND run_id < ?5))
                      ORDER BY updated_at DESC, run_id DESC
                      LIMIT ?6"
                ))
                .map_err(be)?;
            let rows = stmt
                .query_map(
                    params![
                        filter.project,
                        filter.status.map(RunStatus::as_str),
                        filter.definition_id,
                        after_at,
                        after_id,
                        (limit + 1) as i64
                    ],
                    row_to_run,
                )
                .map_err(be)?;
            let mut runs = Vec::new();
            for row in rows {
                runs.push(row.map_err(be)?);
            }
            let next_cursor = if runs.len() > limit {
                runs.truncate(limit);
                runs.last().map(|r| format!("{}:{}", ns(r.updated_at), r.run_id))
            } else {
                None
            };
            Ok(RunPage { runs, next_cursor })
        })
        .await
    }

    async fn claim(
        &self,
        project: &str,
        owner: &str,
        limit: usize,
        run_id: Option<&str>,
        now: OffsetDateTime,
    ) -> Result<Vec<Lease>> {
        let (project, owner, scope) =
            (project.to_string(), owner.to_string(), run_id.map(str::to_string));
        let cfg = self.config;
        let (leases, refused_any) = self
            .with_write(move |conn| {
                let tx = conn.transaction().map_err(be)?;
                // The runnable set: a slot on disk, a live run under a live root,
                // no other attempt of the same step held or inside its reclaim
                // window, and no retry timer still counting down. `orphaned` is in
                // that exclusion list on purpose, which is what makes the window a
                // safety property of one `WHERE` clause rather than a promise. The
                // ceilings and the fan-out concurrency are decided below, because
                // a ceiling that refuses a lease also ends the run.
                let mut stmt = tx
                    .prepare(
                        "SELECT a.run_id, a.step_key, a.attempt
                       FROM step_attempts a
                       JOIN runs r    ON r.run_id = a.run_id
                       JOIN runs root ON root.run_id = r.root_run_id
                      WHERE a.status = 'pending'
                        AND a.kind IN ('task','agent')
                        AND r.status = 'running'
                        AND root.status = 'running'
                        AND r.project = ?1
                        AND (?2 IS NULL OR a.run_id = ?2)
                        AND NOT EXISTS (SELECT 1 FROM step_attempts b
                                         WHERE b.run_id = a.run_id
                                           AND b.step_key = a.step_key
                                           AND b.status IN ('leased','started','orphaned'))
                        AND NOT EXISTS (SELECT 1 FROM timers t
                                         WHERE t.run_id = a.run_id AND t.step_key = a.step_key
                                           AND t.kind = 'retry')
                      ORDER BY r.created_at, a.run_id, a.step_key
                      LIMIT ?3",
                    )
                    .map_err(be)?;
                let candidates: Vec<(String, String, i64)> = stmt
                    .query_map(params![project, scope, limit.max(1) as i64], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                    })
                    .map_err(be)?
                    .collect::<std::result::Result<_, _>>()
                    .map_err(be)?;
                drop(stmt);

                let mut out = Vec::new();
                let mut refused_any = false;
                for (run_id, step_key, attempt) in candidates {
                    let run = load_run(&tx, &run_id)?;
                    if run.status.is_terminal() {
                        continue;
                    }
                    let root = if run.root_run_id == run.run_id {
                        run.clone()
                    } else {
                        load_run(&tx, &run.root_run_id)?
                    };
                    if root.status.is_terminal() {
                        continue;
                    }
                    // A run refused for a spent ceiling ends failed naming the
                    // ceiling, rather than sitting at a status nothing names.
                    let mut refused = false;
                    for candidate in [&root, &run] {
                        if candidate.max_steps.is_some_and(|m| candidate.steps_used >= m) {
                            fail_run(
                                &tx,
                                &candidate.run_id,
                                "run exceeded max_steps",
                                StepErrorClass::Fatal,
                                &cfg,
                                now,
                            )?;
                            refused = true;
                            break;
                        }
                        if candidate.max_cost_usd.is_some_and(|m| candidate.cost_usd >= m) {
                            fail_run(
                                &tx,
                                &candidate.run_id,
                                "run exceeded max_cost_usd",
                                StepErrorClass::Fatal,
                                &cfg,
                                now,
                            )?;
                            refused = true;
                            break;
                        }
                    }
                    if refused {
                        refused_any = true;
                        continue;
                    }
                    // A foreach offers at most `concurrency` items at once.
                    let parts = parse_key(&step_key);
                    if let (Some(_), None) = (parts.index, parts.sub) {
                        let bound = match &run.cursor.at {
                            Construct::Foreach { step, concurrency, .. } if *step == parts.root => {
                                Some(*concurrency)
                            }
                            Construct::Foreach { .. }
                            | Construct::Task { .. }
                            | Construct::Agent { .. }
                            | Construct::Sleep { .. }
                            | Construct::SleepUntil { .. }
                            | Construct::WaitEvent { .. }
                            | Construct::Parallel { .. }
                            | Construct::Loop { .. }
                            | Construct::Subworkflow { .. }
                            | Construct::End { .. } => None,
                        };
                        if let Some(bound) = bound {
                            let live: i64 = tx
                                .query_row(
                                    "SELECT COUNT(*) FROM step_attempts
                                  WHERE run_id = ?1 AND step_key LIKE ?2
                                    AND step_key NOT LIKE '%/%'
                                    AND status IN ('leased','started','orphaned')",
                                    params![run.run_id, format!("{}[%", parts.root)],
                                    |r| r.get(0),
                                )
                                .map_err(be)?;
                            if live >= bound as i64 {
                                continue;
                            }
                        }
                    }

                    let lease_id = new_id("lse");
                    let lease_until = now + cfg.lease_ttl;
                    let changed = tx
                        .execute(
                            "UPDATE step_attempts
                            SET status = 'leased', lease_id = ?1, lease_owner = ?2,
                                lease_until = ?3, leased_at = ?4
                          WHERE run_id = ?5 AND step_key = ?6 AND attempt = ?7
                            AND status = 'pending'",
                            params![
                                lease_id,
                                owner,
                                ns(lease_until),
                                ns(now),
                                run_id,
                                step_key,
                                attempt
                            ],
                        )
                        .map_err(be)?;
                    if changed == 0 {
                        continue;
                    }
                    let reqs = attempt_requirements(
                        &step_key,
                        AttemptStatus::Leased,
                        Some(lease_until),
                        None,
                    );
                    apply(&tx, &run, &reqs, &cfg, now)?;
                    emit(
                        &tx,
                        &run_id,
                        RunEventKind::StepLeased,
                        json!({ "step_key": step_key, "attempt": attempt, "worker_id": owner }),
                        now,
                    )?;
                    let a = load_attempt_by_lease(&tx, &lease_id)?;
                    let journal = load_journal(&tx, &run_id)?;
                    let scope = match (parts.index, parts.sub) {
                        (Some(i), Some(_)) => Scope::iteration(i as u32),
                        (Some(_), None) | (None, _) => Scope::top(),
                    };
                    out.push(Lease {
                        lease_id,
                        run_id: run_id.clone(),
                        project: run.project.clone(),
                        definition_id: run.definition_id.clone(),
                        definition_version: run.definition_version,
                        step_key: step_key.clone(),
                        kind: a.kind,
                        attempt,
                        effect: a.effect,
                        expires_at: lease_until,
                        deadline_ms: a.deadline_ms,
                        idempotency_key: sha256_hex(&format!("{run_id}:{step_key}")),
                        input: a.input.clone().unwrap_or(Value::Null),
                        prior: steps_root(&journal, &scope),
                        state: run.state.clone(),
                        correlation: correlation_with(&run.correlation, &run.project, &run_id),
                        trace_id: run.trace_id.clone(),
                        span_id: a.span_id.clone(),
                        parent_span_id: run.root_span_id.clone(),
                    });
                }
                tx.commit().map_err(be)?;
                Ok((out, refused_any))
            })
            .await?;
        // A lease taken makes nothing new claimable, and a claim that took
        // nothing changed nothing at all, so neither wakes a worker. Waking
        // one here was the spin: every idle claim woke its own long-poll. A
        // run ended for a spent ceiling is the one case that can move a
        // parent run onto new work.
        self.wake(if refused_any {
            Wake::Workers
        } else if leases.is_empty() {
            Wake::Nobody
        } else {
            Wake::Readers
        });
        Ok(leases)
    }

    async fn started(&self, lease_id: &str, now: OffsetDateTime) -> Result<LeaseAck> {
        let lease_id = lease_id.to_string();
        let cfg = self.config;
        self.with_write_waking(Wake::Readers, move |conn| {
            let tx = conn.transaction().map_err(be)?;
            let (a, run) = owner_of_record(&tx, &lease_id)?;
            let first = a.started_at.is_none();
            let ack = restore(&tx, &a, true, &cfg, now)?;
            if first {
                emit(
                    &tx,
                    &run.run_id,
                    RunEventKind::StepStarted,
                    json!({ "step_key": a.step_key, "attempt": a.attempt }),
                    now,
                )?;
                open_step_span(&tx, &run, &a, now, now)?;
            }
            tx.commit().map_err(be)?;
            Ok(ack)
        })
        .await
    }

    async fn heartbeat(&self, lease_id: &str, now: OffsetDateTime) -> Result<LeaseAck> {
        let lease_id = lease_id.to_string();
        let cfg = self.config;
        // A heartbeat writes no event and no span and frees no step, and every
        // live body sends one every few seconds, so waking anybody on it would
        // turn each one into a claim pass per idle worker.
        self.with_write_waking(Wake::Nobody, move |conn| {
            let tx = conn.transaction().map_err(be)?;
            let (a, _run) = owner_of_record(&tx, &lease_id)?;
            let ack = restore(&tx, &a, false, &cfg, now)?;
            tx.commit().map_err(be)?;
            Ok(ack)
        })
        .await
    }

    async fn complete_step(
        &self,
        lease_id: &str,
        completion: StepCompletion,
        now: OffsetDateTime,
    ) -> Result<CompleteOutcome> {
        let lease_id = lease_id.to_string();
        let cfg = self.config;
        self.with_write_waking(Wake::Workers, move |conn| {
            let tx = conn.transaction().map_err(be)?;
            let (a, run) = owner_of_record(&tx, &lease_id)?;
            if a.effect == Effect::AtMostOnce && a.started_at.is_none() {
                return Err(conflict(
                    ConflictReason::NotStarted,
                    &a,
                    run.status,
                    format!(
                        "attempt {} of step \"{}\" is at_most_once and was never started",
                        a.attempt, a.step_key
                    ),
                ));
            }
            if completion.state.as_ref().is_some_and(|s| !s.is_object()) {
                return Err(Error::Invalid("state must be a JSON object".into()));
            }
            let cost = completion.cost_usd.unwrap_or(0.0);
            tx.execute(
                "UPDATE step_attempts
                    SET status = 'succeeded', output = ?1, usage = ?2, cost_usd = ?3,
                        ended_at = ?4, ended_by_lease_id = ?5, budget_spent = 1
                  WHERE run_id = ?6 AND step_key = ?7 AND attempt = ?8",
                params![
                    json(&completion.output),
                    json(&completion.usage),
                    completion.cost_usd,
                    ns(now),
                    lease_id,
                    a.run_id,
                    a.step_key,
                    a.attempt
                ],
            )
            .map_err(be)?;
            clear_step_timers(&tx, &a.run_id, &a.step_key)?;

            // The state merge and the counters. Both counters are commutative,
            // so the root needs no version check. The cost is that every
            // completion anywhere in a tree writes the root row, and the tree
            // serialises on it for the length of one statement.
            if let Some(Value::Object(incoming)) = &completion.state {
                let mut merged = match &run.state {
                    Value::Object(map) => map.clone(),
                    Value::Null
                    | Value::Bool(_)
                    | Value::Number(_)
                    | Value::String(_)
                    | Value::Array(_) => Map::new(),
                };
                for (k, v) in incoming {
                    merged.insert(k.clone(), v.clone());
                }
                tx.execute(
                    "UPDATE runs SET state = ?1 WHERE run_id = ?2",
                    params![Value::Object(merged).to_string(), a.run_id],
                )
                .map_err(be)?;
            }
            tx.execute(
                "UPDATE runs SET steps_used = steps_used + 1, cost_usd = cost_usd + ?1
                  WHERE run_id = ?2",
                params![cost, a.run_id],
            )
            .map_err(be)?;
            if run.root_run_id != run.run_id {
                tx.execute(
                    "UPDATE runs SET steps_used = steps_used + 1, cost_usd = cost_usd + ?1
                      WHERE run_id = ?2",
                    params![cost, run.root_run_id],
                )
                .map_err(be)?;
            }
            emit(
                &tx,
                &run.run_id,
                RunEventKind::StepFinished,
                json!({ "step_key": a.step_key, "attempt": a.attempt }),
                now,
            )?;
            let ended = Attempt {
                status: AttemptStatus::Succeeded,
                output: completion.output.clone(),
                usage: completion.usage.clone(),
                cost_usd: completion.cost_usd,
                ended_at: Some(now),
                ..a.clone()
            };
            close_step_span(&tx, &run, &ended, now)?;
            advance_if_settled(&tx, &a.run_id, &cfg, now)?;
            let after = load_run(&tx, &a.run_id)?;
            let next_runnable = runnable_in_tree(&tx, &after.root_run_id)?;
            tx.commit().map_err(be)?;
            Ok(CompleteOutcome { run_status: after.status, next_runnable })
        })
        .await
    }

    async fn fail_step(
        &self,
        lease_id: &str,
        error: StepError,
        now: OffsetDateTime,
    ) -> Result<FailOutcome> {
        let lease_id = lease_id.to_string();
        let cfg = self.config;
        self.with_write_waking(Wake::Workers, move |conn| {
            let tx = conn.transaction().map_err(be)?;
            let (a, run) = owner_of_record(&tx, &lease_id)?;
            if a.effect == Effect::AtMostOnce && a.started_at.is_none() {
                return Err(conflict(
                    ConflictReason::NotStarted,
                    &a,
                    run.status,
                    format!(
                        "attempt {} of step \"{}\" is at_most_once and was never started",
                        a.attempt, a.step_key
                    ),
                ));
            }
            let attempts_left = record_failure(
                &tx,
                &run,
                &a,
                AttemptStatus::Failed,
                &error,
                Some(&lease_id),
                Disposition::Policy { retry_after_ms: error.retry_after_ms },
                &cfg,
                now,
            )?;
            let after = load_run(&tx, &a.run_id)?;
            let next_runnable = runnable_in_tree(&tx, &after.root_run_id)?;
            tx.commit().map_err(be)?;
            Ok(FailOutcome { run_status: after.status, next_runnable, attempts_left })
        })
        .await
    }

    async fn cancel(&self, run_id: &str, now: OffsetDateTime) -> Result<Run> {
        let run_id = run_id.to_string();
        let cfg = self.config;
        self.with_write_waking(Wake::Workers, move |conn| {
            let tx = conn.transaction().map_err(be)?;
            cancel_tree(&tx, &run_id, &cfg, now)?;
            let run = load_run(&tx, &run_id)?;
            tx.commit().map_err(be)?;
            Ok(run)
        })
        .await
    }

    async fn run_due_timers(&self, now: OffsetDateTime) -> Result<TimerReport> {
        let cfg = self.config;
        let report = self
            .with_write(move |conn| {
                let due: Vec<Timer> = {
                    let mut stmt = conn
                        .prepare(
                            "SELECT run_id, step_key, kind, due_at FROM timers
                              WHERE due_at <= ?1 ORDER BY due_at LIMIT ?2",
                        )
                        .map_err(be)?;
                    let rows = stmt
                        .query_map(params![ns(now), TIMER_BATCH as i64], |r| {
                            Ok((
                                r.get::<_, String>(0)?,
                                r.get::<_, String>(1)?,
                                r.get::<_, String>(2)?,
                                r.get::<_, i64>(3)?,
                            ))
                        })
                        .map_err(be)?
                        .collect::<std::result::Result<Vec<_>, _>>()
                        .map_err(be)?
                        .into_iter()
                        .filter_map(|(run_id, step_key, kind, due_at)| {
                            TimerKind::parse(&kind).map(|kind| Timer {
                                run_id,
                                step_key,
                                kind,
                                due_at: at(due_at),
                            })
                        })
                        .collect();
                    rows
                };

                let mut report = TimerReport::default();
                for t in due {
                    // One transaction per deadline. A run whose graph has gone
                    // missing should cost its own timer, not the whole tick.
                    let tx = conn.transaction().map_err(be)?;
                    match fire(&tx, &t, &cfg, now) {
                        Ok(()) => {
                            tx.commit().map_err(be)?;
                            report.fired += 1;
                        }
                        Err(e) => {
                            drop(tx);
                            tracing::warn!(
                                run_id = %t.run_id, step = %t.step_key, kind = t.kind.as_str(),
                                error = %e, "a due timer could not be handled"
                            );
                            // Pushed forward so the batch behind it moves on:
                            // a timer that fails every tick would otherwise be
                            // first in every batch, for ever.
                            conn.execute(
                                "UPDATE timers SET due_at = ?1
                                  WHERE run_id = ?2 AND step_key = ?3 AND kind = ?4 AND due_at = ?5",
                                params![
                                    ns(now + cfg.timer_retry),
                                    t.run_id,
                                    t.step_key,
                                    t.kind.as_str(),
                                    ns(t.due_at)
                                ],
                            )
                            .map_err(be)?;
                            report.deferred += 1;
                        }
                    }
                }

                let tx = conn.transaction().map_err(be)?;
                report.parked_events_deleted = tx
                    .execute("DELETE FROM parked_events WHERE expires_at <= ?1", params![ns(now)])
                    .map_err(be)?;
                report.idempotency_keys_released = tx
                    .execute(
                        "UPDATE runs SET idempotency_key = NULL
                          WHERE idempotency_key IS NOT NULL AND created_at <= ?1",
                        params![ns(now - cfg.idempotency_window)],
                    )
                    .map_err(be)?;
                tx.commit().map_err(be)?;
                Ok(report)
            })
            .await?;
        if report.fired > 0 {
            self.wake(Wake::Workers);
        }
        Ok(report)
    }

    async fn reconcile(&self, now: OffsetDateTime) -> Result<ReconcileReport> {
        let cfg = self.config;
        let report = self
            .with_write(move |conn| {
                let cutoff = ns(now - cfg.stall_window);
                let stalled: Vec<String> = {
                    let mut stmt = conn
                        .prepare(
                            "SELECT r.run_id FROM runs r
                              WHERE r.status = 'running' AND r.updated_at < ?1
                                AND NOT EXISTS (SELECT 1 FROM step_attempts a
                                                 WHERE a.run_id = r.run_id
                                                   AND a.status IN ('leased','started'))",
                        )
                        .map_err(be)?;
                    let rows = stmt
                        .query_map(params![cutoff], |r| r.get(0))
                        .map_err(be)?
                        .collect::<std::result::Result<_, _>>()
                        .map_err(be)?;
                    rows
                };

                let mut report = ReconcileReport::default();
                for run_id in stalled {
                    let tx = conn.transaction().map_err(be)?;
                    let run = load_run(&tx, &run_id)?;
                    // The materialiser's own function, against the cursor the
                    // run already has. There is no second list of rows here.
                    let mut rows =
                        materialise(&tx, &run_id, run.cursor.clone(), Move::Repair, &cfg, now)?;
                    let run = load_run(&tx, &run_id)?;
                    if !run.status.is_terminal() {
                        // The two deadlines a cursor never names. An `orphaned`
                        // attempt is inside this net rather than outside it.
                        let live = read_attempts(
                            &tx,
                            &format!(
                                "SELECT {ATTEMPT_COLUMNS} FROM step_attempts
                                  WHERE run_id = ?1 AND status IN ('leased','started','orphaned')"
                            ),
                            params![run_id],
                        )?;
                        for a in live {
                            let reqs = attempt_requirements(
                                &a.step_key,
                                a.status,
                                a.lease_until,
                                a.reclaim_until,
                            );
                            rows += apply(&tx, &run, &reqs, &cfg, now)?.rows;
                        }
                        // And the retry a lost timer would have written.
                        let graph = load_graph(&tx, &run)?;
                        let slots: Vec<Slot> = cursor_requirements(run.deadline_at, &run.cursor)
                            .attempt_slots()
                            .cloned()
                            .collect();
                        for slot in slots {
                            if awaiting_retry(&tx, &run, &graph, &slot.step_key)? {
                                rows += tx
                                    .execute(
                                        "INSERT INTO timers (run_id, step_key, kind, due_at, created_at)
                                         VALUES (?1,?2,'retry',?3,?4)
                                           ON CONFLICT (run_id, step_key, kind) DO NOTHING",
                                        params![run_id, slot.step_key, ns(now), ns(now)],
                                    )
                                    .map_err(be)?;
                            }
                        }
                    }
                    if rows > 0 {
                        tx.execute(
                            "UPDATE runs SET updated_at = ?1 WHERE run_id = ?2",
                            params![ns(now), run_id],
                        )
                        .map_err(be)?;
                        report.runs_repaired += 1;
                        report.rows_repaired += rows;
                    }
                    tx.commit().map_err(be)?;
                }

                // The second predicate, and the only one that looks at
                // terminal runs. The stale rows this deletes belong to runs
                // that already ended, which is precisely why the net above
                // could never catch them.
                let tx = conn.transaction().map_err(be)?;
                report.stale_rows_deleted += tx
                    .execute(
                        "DELETE FROM waits WHERE run_id IN
                            (SELECT run_id FROM runs WHERE status != 'running')",
                        [],
                    )
                    .map_err(be)?;
                report.stale_rows_deleted += tx
                    .execute(
                        "DELETE FROM timers WHERE run_id IN
                            (SELECT run_id FROM runs WHERE status != 'running')",
                        [],
                    )
                    .map_err(be)?;
                tx.commit().map_err(be)?;
                Ok(report)
            })
            .await?;
        if report.runs_repaired > 0 {
            self.wake(Wake::Workers);
        }
        Ok(report)
    }

    async fn deliver_event(&self, event: InboundEvent, now: OffsetDateTime) -> Result<Delivery> {
        let cfg = self.config;
        self.with_write_waking(Wake::Workers, move |conn| {
            let tx = conn.transaction().map_err(be)?;
            let outcome = route_event(&tx, &event, None, &cfg, now)?;
            tx.commit().map_err(be)?;
            Ok(outcome)
        })
        .await
    }

    async fn signal(
        &self,
        run_id: &str,
        event: &str,
        payload: Option<Value>,
        event_id: Option<String>,
        now: OffsetDateTime,
    ) -> Result<Delivery> {
        let (run_id, event) = (run_id.to_string(), event.to_string());
        let cfg = self.config;
        self.with_write_waking(Wake::Workers, move |conn| {
            let tx = conn.transaction().map_err(be)?;
            let run = load_run(&tx, &run_id)?;
            let key: String = tx
                .query_row(
                    "SELECT correlation_key FROM waits WHERE run_id = ?1 AND event = ?2
                      ORDER BY expires_at LIMIT 1",
                    params![run_id, event],
                    |r| r.get(0),
                )
                .optional()
                .map_err(be)?
                .ok_or_else(|| Error::NotFound { kind: "wait", id: format!("{run_id}:{event}") })?;
            let (event_id, id_is_derived) = match &event_id {
                Some(id) => (id.clone(), false),
                None => {
                    (InboundEvent::derived_id(&run.project, &event, &key, payload.as_ref()), true)
                }
            };
            let inbound = InboundEvent {
                project: run.project.clone(),
                event: event.clone(),
                correlation_key: key,
                event_id,
                id_is_derived,
                payload: payload.clone(),
            };
            let outcome = route_event(&tx, &inbound, Some(&run_id), &cfg, now)?;
            tx.commit().map_err(be)?;
            Ok(outcome)
        })
        .await
    }

    async fn lease_run(&self, lease_id: &str) -> Result<Run> {
        let lease_id = lease_id.to_string();
        self.with_write(move |conn| {
            let a = load_attempt_by_lease(conn, &lease_id)?;
            load_run(conn, &a.run_id)
        })
        .await
    }

    async fn attempts(&self, run_id: &str) -> Result<Vec<Attempt>> {
        let run_id = run_id.to_string();
        self.with_write(move |conn| attempts_of(conn, &run_id)).await
    }

    async fn node_attempts(&self, run_ids: &[String]) -> Result<Vec<NodeAttempts>> {
        if run_ids.is_empty() {
            return Ok(Vec::new());
        }
        // Sorted and de-duplicated before it is cut into batches, so that the
        // ordering each statement applies composes into one ordering over the
        // whole answer, and so a caller that names a run twice does not get its
        // rows twice.
        let mut run_ids = run_ids.to_vec();
        run_ids.sort();
        run_ids.dedup();
        self.with_write(move |conn| {
            let mut out: Vec<NodeAttempts> = Vec::new();
            for batch in run_ids.chunks(NODE_ATTEMPT_BATCH) {
                let holes = vec!["?"; batch.len()].join(", ");
                // `skipped` is excluded here rather than by the caller because
                // it is the difference between a node that ran twice and a
                // position whose guard was false, and only this table knows.
                let mut stmt = conn
                    .prepare(&format!(
                        "SELECT run_id, node, COUNT(*) FROM (
                             SELECT run_id, {ATTEMPT_NODE} AS node
                               FROM step_attempts
                              WHERE run_id IN ({holes}) AND status <> 'skipped'
                         )
                          GROUP BY run_id, node
                          ORDER BY run_id, node"
                    ))
                    .map_err(be)?;
                let rows = stmt
                    .query_map(params_from_iter(batch.iter()), |r| {
                        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
                    })
                    .map_err(be)?;
                for row in rows {
                    let (run_id, node, attempts) = row.map_err(be)?;
                    // `COUNT(*)` cannot be negative and cannot exceed the rows
                    // in the table, so the clamp is a total conversion rather
                    // than a guard against anything this query can produce.
                    out.push(NodeAttempts {
                        run_id,
                        node,
                        attempts: u32::try_from(attempts).unwrap_or(u32::MAX),
                    });
                }
            }
            // Each batch is ordered, and the batches hold ascending ids in
            // order, so the whole answer is ordered by run id and then node.
            Ok(out)
        })
        .await
    }

    async fn step_cause(&self, run_id: &str, step_key: &str, max_depth: u32) -> Result<StepCause> {
        let (run_id, step_key) = (run_id.to_string(), step_key.to_string());
        self.with_write(move |conn| {
            let run = load_run(conn, &run_id)?;
            // The definition version the run pinned at start, never the latest
            // one: the chain has to be the one this run actually walked, and a
            // version stored since then declares different edges.
            let graph = load_graph(conn, &run)?;
            let journal = load_journal(conn, &run_id)?;
            crate::cause::explain(&run, &graph, &journal, &step_key, max_depth)
        })
        .await
    }

    async fn timers(&self, run_id: &str) -> Result<Vec<Timer>> {
        let run_id = run_id.to_string();
        self.with_write(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT step_key, kind, due_at FROM timers WHERE run_id = ?1
                      ORDER BY kind, step_key",
                )
                .map_err(be)?;
            let rows = stmt
                .query_map(params![run_id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
                })
                .map_err(be)?;
            let mut out = Vec::new();
            for row in rows {
                let (step_key, kind, due_at) = row.map_err(be)?;
                out.push(Timer {
                    run_id: run_id.clone(),
                    step_key,
                    kind: TimerKind::parse(&kind)
                        .ok_or_else(|| Error::Backend(format!("unknown timer kind {kind}")))?,
                    due_at: at(due_at),
                });
            }
            Ok(out)
        })
        .await
    }

    async fn waits(&self, run_id: &str) -> Result<Vec<Wait>> {
        let run_id = run_id.to_string();
        self.with_write(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT step_key, project, event, correlation_key, expires_at, span_id
                       FROM waits WHERE run_id = ?1 ORDER BY step_key",
                )
                .map_err(be)?;
            let rows = stmt
                .query_map(params![run_id], |r| {
                    Ok(Wait {
                        run_id: String::new(),
                        step_key: r.get(0)?,
                        project: r.get(1)?,
                        event: r.get(2)?,
                        correlation_key: r.get(3)?,
                        expires_at: at(r.get(4)?),
                        span_id: r.get(5)?,
                    })
                })
                .map_err(be)?;
            let mut out = Vec::new();
            for row in rows {
                let mut w = row.map_err(be)?;
                w.run_id.clone_from(&run_id);
                out.push(w);
            }
            Ok(out)
        })
        .await
    }

    async fn events(&self, run_id: &str, after: i64, limit: usize) -> Result<EventPage> {
        let run_id = run_id.to_string();
        self.with_write(move |conn| {
            let limit = limit.max(1);
            let mut stmt = conn
                .prepare(
                    "SELECT seq, type, payload, at FROM run_events
                      WHERE run_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
                )
                .map_err(be)?;
            let rows = stmt
                .query_map(params![run_id, after, (limit + 1) as i64], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                })
                .map_err(be)?;
            let mut events = Vec::new();
            for row in rows {
                let (seq, kind, payload, at_ns) = row.map_err(be)?;
                events.push(RunEvent {
                    run_id: run_id.clone(),
                    seq,
                    kind: RunEventKind::parse(&kind)
                        .ok_or_else(|| Error::Backend(format!("unknown run event kind {kind}")))?,
                    payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
                    at: at(at_ns),
                });
            }
            let has_more = events.len() > limit;
            events.truncate(limit);
            Ok(EventPage { events, has_more })
        })
        .await
    }

    async fn claim_outbox(&self, limit: usize, now: OffsetDateTime) -> Result<Vec<OutboxRow>> {
        let cfg = self.config;
        self.with_write(move |conn| {
            let tx = conn.transaction().map_err(be)?;
            let claim_id = new_id("drain");
            // A row is offered when it is unclaimed or its claim has lapsed,
            // and no earlier write for the same span is still held by another
            // drain. Rows come back in `seq` order, so an open row and its
            // close in one batch land in the order they were written.
            let mut stmt = tx
                .prepare(
                    "SELECT o.seq, o.outbox_id, o.run_id, o.span_id, o.ingest_key, o.phase, o.payload
                       FROM span_outbox o
                      WHERE (o.claim_id IS NULL OR o.claimed_until < ?1)
                        AND NOT EXISTS (SELECT 1 FROM span_outbox p
                                         WHERE p.run_id = o.run_id AND p.span_id = o.span_id
                                           AND p.seq < o.seq
                                           AND p.claim_id IS NOT NULL AND p.claimed_until >= ?1)
                      ORDER BY o.seq LIMIT ?2",
                )
                .map_err(be)?;
            let rows: Vec<OutboxRow> = stmt
                .query_map(params![ns(now), limit.max(1) as i64], |r| {
                    Ok(OutboxRow {
                        seq: r.get(0)?,
                        outbox_id: r.get(1)?,
                        run_id: r.get(2)?,
                        span_id: r.get(3)?,
                        ingest_key: r.get(4)?,
                        phase: r.get(5)?,
                        payload: serde_json::from_str(&r.get::<_, String>(6)?)
                            .unwrap_or(Value::Null),
                    })
                })
                .map_err(be)?
                .collect::<std::result::Result<_, _>>()
                .map_err(be)?;
            drop(stmt);
            for row in &rows {
                tx.execute(
                    "UPDATE span_outbox SET claim_id = ?1, claimed_until = ?2 WHERE outbox_id = ?3",
                    params![claim_id, ns(now + cfg.outbox_claim_ttl), row.outbox_id],
                )
                .map_err(be)?;
            }
            tx.commit().map_err(be)?;
            Ok(rows)
        })
        .await
    }

    async fn ack_outbox(&self, outbox_ids: &[String]) -> Result<()> {
        let ids = outbox_ids.to_vec();
        self.with_write(move |conn| {
            let tx = conn.transaction().map_err(be)?;
            for id in &ids {
                tx.execute("DELETE FROM span_outbox WHERE outbox_id = ?1", params![id])
                    .map_err(be)?;
            }
            tx.commit().map_err(be)?;
            Ok(())
        })
        .await
    }

    async fn release_outbox(&self, outbox_ids: &[String]) -> Result<()> {
        let ids = outbox_ids.to_vec();
        self.with_write(move |conn| {
            let tx = conn.transaction().map_err(be)?;
            for id in &ids {
                tx.execute(
                    "UPDATE span_outbox SET claim_id = NULL, claimed_until = NULL
                      WHERE outbox_id = ?1",
                    params![id],
                )
                .map_err(be)?;
            }
            tx.commit().map_err(be)?;
            Ok(())
        })
        .await
    }

    async fn erase_subject(&self, project: &str, user_id: &str) -> Result<SubjectErasure> {
        let (project, user_id) = (project.to_string(), user_id.to_string());
        // Every tree with at least one run attributed to the person. A child
        // copies its parent's correlation, so in practice the whole tree
        // matches; the tree is the unit either way, because a parent's later
        // steps read what its children returned.
        let trees: Vec<(String, i64)> = self
            .with_write(move |conn| {
                let mut stmt = conn
                    .prepare(&format!(
                        "SELECT root_run_id, COUNT(*) FROM runs
                          WHERE project = ?1 AND {CORRELATION_USER} = ?2
                          GROUP BY root_run_id ORDER BY root_run_id"
                    ))
                    .map_err(be)?;
                let trees = stmt
                    .query_map(params![project, user_id], |r| Ok((r.get(0)?, r.get(1)?)))
                    .map_err(be)?
                    .collect::<std::result::Result<_, _>>()
                    .map_err(be)?;
                Ok(trees)
            })
            .await?;

        // One transaction per batch of trees, as retention does, so the writer
        // is free between them. One transaction for the whole subject held
        // the store's only lock for as long as every delete took, and every
        // claim and heartbeat waited behind it, so erasing a person with a
        // long history could let every lease expire.
        //
        // A batch that fails leaves the ones before it done, and the error is
        // returned. Sending the erasure again finishes it, because a tree that
        // has gone is not found again. Whether a tree is live is read inside
        // its own batch, so a run that ended since the list was read is erased.
        let mut out = SubjectErasure::default();
        for batch in trees.chunks(RETENTION_BATCH) {
            let batch = batch.to_vec();
            let part = self
                .with_write(move |conn| {
                    let tx = conn.transaction().map_err(be)?;
                    let mut part = SubjectErasure::default();
                    for (root, matched) in &batch {
                        if tree_is_live(&tx, root)? {
                            part.runs_in_flight += usize::try_from(*matched).unwrap_or(0);
                            part.in_flight_roots.push(root.clone());
                        } else {
                            part.runs_erased += delete_tree(&tx, root)?.0;
                        }
                    }
                    tx.commit().map_err(be)?;
                    Ok(part)
                })
                .await?;
            out.runs_erased += part.runs_erased;
            out.runs_in_flight += part.runs_in_flight;
            // The batches run in root order, so the roots stay sorted.
            out.in_flight_roots.extend(part.in_flight_roots);
        }
        Ok(out)
    }

    async fn apply_retention(&self, project: &str, cutoff: OffsetDateTime) -> Result<RunRetention> {
        let mut total = RunRetention::default();
        loop {
            let project = project.to_string();
            // One batch per transaction, so the writer is free between them.
            let (runs, rows, trees) = self
                .with_write(move |conn| {
                    let tx = conn.transaction().map_err(be)?;
                    // A root's `updated_at` is when it ended, and a tree ends
                    // with its root, because an ending run cancels the
                    // children still running under it. The `NOT EXISTS`
                    // holds the rule that matters regardless: nothing still
                    // running is ever deleted.
                    let mut stmt = tx
                        .prepare(
                            "SELECT r.run_id FROM runs r
                              WHERE r.project = ?1 AND r.run_id = r.root_run_id
                                AND r.status != 'running' AND r.updated_at < ?2
                                AND NOT EXISTS (SELECT 1 FROM runs c
                                                 WHERE c.root_run_id = r.run_id
                                                   AND c.status = 'running')
                              LIMIT ?3",
                        )
                        .map_err(be)?;
                    let roots: Vec<String> = stmt
                        .query_map(params![project, ns(cutoff), RETENTION_BATCH as i64], |r| {
                            r.get(0)
                        })
                        .map_err(be)?
                        .collect::<std::result::Result<_, _>>()
                        .map_err(be)?;
                    drop(stmt);
                    let (mut runs, mut rows) = (0, 0);
                    for root in &roots {
                        let (r, n) = delete_tree(&tx, root)?;
                        runs += r;
                        rows += n;
                    }
                    tx.commit().map_err(be)?;
                    Ok((runs, rows, roots.len()))
                })
                .await?;
            total.runs_deleted += runs;
            total.rows_deleted += rows;
            if trees < RETENTION_BATCH {
                return Ok(total);
            }
        }
    }
}
