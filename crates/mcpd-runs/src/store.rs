//! The trait the run core is written against.
//!
//! This is a new trait rather than more methods on `mcpd_store::Store`, and
//! the reason is mechanical: `Store` has three backends plus a test-only fake
//! inside a test module in another crate, so widening it makes every addition
//! a four-place change in which the fourth place fails at `cargo test` a long
//! way from the edit. `RunStore` carries the run protocol and nothing else.
//!
//! Every mutating method is a whole transaction. There is no `begin` on this
//! trait, and that is deliberate: advancing a cursor and materialising what
//! the new cursor requires have to commit together, and a trait that hands
//! out a transaction handle is a trait that lets a caller split them.

use crate::cause::StepCause;
use crate::model::{
    Attempt, Definition, DefinitionKind, Delivery, InboundEvent, Lease, OutboxRow, Run, RunEvent,
    RunId, RunStatus, StepErrorClass, Timer, Wait,
};
use crate::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use time::{Duration, OffsetDateTime};
use tokio::sync::Notify;

/// What to start. Section 5 of the contract.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StartRun {
    /// Which project.
    pub project: String,
    /// Which definition.
    pub definition_id: String,
    /// Which version of it. Absent pins the highest stored version.
    pub definition_version: Option<u32>,
    /// What it starts with, validated against `input_schema`.
    pub input: Option<Value>,
    /// The `Correlation` minus `project` and `run_id`, which the store sets.
    pub correlation: Option<Value>,
    /// Unique per project while held. A repost returns the same run.
    pub idempotency_key: Option<String>,
}

/// A definition document to store under the rule of section 4.
#[derive(Debug, Clone, PartialEq)]
pub struct DefinitionDocument {
    /// Which project.
    pub project: String,
    /// Its id.
    pub id: String,
    /// What kind of thing it is.
    pub kind: DefinitionKind,
    /// The version the caller named, or none for the daemon to choose.
    pub version: Option<u32>,
    /// The body.
    pub body: Value,
}

/// The knobs the ticks read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunConfig {
    /// How long a lease lasts without a heartbeat.
    pub lease_ttl: Duration,
    /// How long an expired lease waits for evidence before anything decides
    /// for it. Three heartbeat intervals at the defaults, which gives a live
    /// worker three chances to be heard and a restarting daemon time to finish
    /// binding its port.
    pub reclaim_window: Duration,
    /// How long a run may sit still before the reconciler looks at it.
    pub stall_window: Duration,
    /// How deep a run tree may go.
    pub max_depth: i64,
    /// How many times one transaction may advance a cursor before it gives up
    /// and fails the run. A parked event that resumes a wait moves the cursor
    /// inside the same commit, and a graph of waits with events already on the
    /// shelf could otherwise walk a long way under one lock.
    pub materialise_budget: usize,
    /// How long a run's idempotency key is held before the tick releases it.
    pub idempotency_window: Duration,
    /// How long a parked event waits for a run before the tick deletes it.
    pub park_ttl: Duration,
    /// How long an outbox claim lasts before another drain may take the row.
    pub outbox_claim_ttl: Duration,
    /// How far a timer that could not be handled is pushed forward, so the
    /// batch behind it moves on.
    pub timer_retry: Duration,
    /// How many times one step key may fail with an identical rendered input
    /// before the run is ended as a repetition. Zero turns the breaker off.
    ///
    /// This is a ceiling on going nowhere, and `retry.max_attempts` is not one.
    /// Two of the daemon's own paths retry while spending no retry budget at
    /// all: an `unservable` failure comes back after one lease length for ever,
    /// and a reclaimed lease is requeued free, because neither is a failure the
    /// worker reported. A run stuck on either will retry the same step with the
    /// same input until something else ends it, and nothing else does.
    ///
    /// The step key folds in the branch, the item and the iteration, so an
    /// identical key is an identical position in the graph; an identical input
    /// on top of that means the context it renders from has not moved either.
    /// Both together are the daemon's own honest reading of "this run is not
    /// making progress", from the journal it already writes.
    pub max_step_repeats: u32,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            lease_ttl: Duration::seconds(30),
            reclaim_window: Duration::seconds(30),
            stall_window: Duration::minutes(5),
            max_depth: 8,
            materialise_budget: 64,
            idempotency_window: Duration::hours(24),
            park_ttl: Duration::hours(24),
            outbox_claim_ttl: Duration::seconds(60),
            timer_retry: Duration::seconds(30),
            // High enough that an ordinary retry policy never meets it: the
            // shipped default is one attempt and a definition asking for
            // twenty is unusual. Low enough that a step retrying every lease
            // length stops inside ten minutes rather than for ever.
            max_step_repeats: 16,
        }
    }
}

/// What the due-timer tick did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TimerReport {
    /// How many timers came due and were handled.
    pub fired: usize,
    /// How many timers could not be handled and were pushed forward.
    pub deferred: usize,
    /// How many parked events past their expiry were deleted.
    pub parked_events_deleted: usize,
    /// How many run idempotency keys past the window were released.
    pub idempotency_keys_released: usize,
}

/// What the reconciler repaired.
///
/// A non-zero count is a defect report rather than a success. A reconciler that
/// quietly fixes the same run shape every day is a bug that has been hidden
/// instead of found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReconcileReport {
    /// How many runs needed anything.
    pub runs_repaired: usize,
    /// How many rows were written that a cursor already required.
    pub rows_repaired: usize,
    /// How many `waits` and `timers` rows belonged to a run that had already
    /// ended. These are outside the "not terminal" predicate on purpose: the
    /// stale rows this counts belong to runs that are over, which is precisely
    /// why the rest of the net could never have caught them.
    pub stale_rows_deleted: usize,
}

/// What one whole tick did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TickReport {
    /// The due-timer pass.
    pub timers: TimerReport,
    /// The reconciler pass.
    pub reconcile: ReconcileReport,
}

/// What `started` and `heartbeat` answer. Section 7.3 and 7.4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseAck {
    /// When the lease now lapses.
    pub expires_at: OffsetDateTime,
    /// Whether this call found the attempt `orphaned` and restored it.
    pub reclaimed: bool,
}

/// What a worker sends with `complete`. Section 7.5.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StepCompletion {
    /// The step's output.
    pub output: Option<Value>,
    /// Merged shallowly into the run's state.
    pub state: Option<Value>,
    /// `TokenUsage` plus `model` and `provider`, as the worker reported it.
    pub usage: Option<Value>,
    /// What the server priced that usage at. The store charges it; it does
    /// not hold a price book.
    pub cost_usd: Option<f64>,
}

/// What a worker sends with `fail`. Section 7.6.
#[derive(Debug, Clone, PartialEq)]
pub struct StepError {
    /// How the worker classified it.
    pub class: StepErrorClass,
    /// What went wrong.
    pub message: String,
    /// Structured detail.
    pub details: Option<Value>,
    /// A floor on the retry delay, when the failure named one.
    pub retry_after_ms: Option<i64>,
}

/// What `complete` answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteOutcome {
    /// Where the run is now.
    pub run_status: RunStatus,
    /// Whether more work in this run's tree is runnable now.
    pub next_runnable: bool,
}

/// What `fail` answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailOutcome {
    /// Where the run is now.
    pub run_status: RunStatus,
    /// Whether more work in this run's tree is runnable now.
    pub next_runnable: bool,
    /// How many attempts the step has left.
    pub attempts_left: u32,
}

/// Which runs to list.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunFilter {
    /// Which project.
    pub project: String,
    /// Only runs in this status.
    pub status: Option<RunStatus>,
    /// Only runs of this definition.
    pub definition_id: Option<String>,
}

/// One page of runs, most recently updated first.
#[derive(Debug, Clone, PartialEq)]
pub struct RunPage {
    /// The rows.
    pub runs: Vec<Run>,
    /// Pass back to get the next page, or none when this was the last.
    pub next_cursor: Option<String>,
}

/// How many attempts one run made at one node of its definition.
///
/// A *node* is a step key with its item or iteration index removed, so
/// `fan[7]` and `fan[8]` are the same node and `each[3]/work` and
/// `each[4]/work` are too. That is what makes this a count of repetition
/// rather than of positions: a retry writes another attempt at one step key,
/// and a loop iteration writes a new step key at the same node, and both mean
/// the same node ran again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeAttempts {
    /// Which run.
    pub run_id: RunId,
    /// The node, with any index removed.
    pub node: String,
    /// How many attempts that run made there.
    pub attempts: u32,
}

/// What erasing one person did to the run store.
///
/// A run is attributed to a person by the `user_id` in its correlation, and it
/// is erased a whole tree at a time: the root and every subworkflow run under
/// it, because a parent's later steps read what its children returned.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SubjectErasure {
    /// How many runs were deleted, with their step attempts, event log,
    /// deadlines, waits, the events they consumed and any span still queued
    /// for the trace store.
    pub runs_erased: usize,
    /// How many runs attributed to the person were left in place because some
    /// run in their tree has not ended. Deleting a live run would corrupt the
    /// workflow it is part of, so these are reported rather than touched.
    pub runs_in_flight: usize,
    /// The roots of those trees, sorted: the runs to cancel, or wait for,
    /// before the erasure is sent again.
    pub in_flight_roots: Vec<RunId>,
}

/// What one retention pass did to the run store.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunRetention {
    /// How many runs were deleted.
    pub runs_deleted: usize,
    /// How many rows were deleted in all, the runs themselves included.
    pub rows_deleted: usize,
}

/// One page of a run's event stream.
#[derive(Debug, Clone, PartialEq)]
pub struct EventPage {
    /// The rows, in `seq` order.
    pub events: Vec<RunEvent>,
    /// Whether more rows exist past the last one here.
    pub has_more: bool,
}

/// The durable run state.
#[async_trait]
pub trait RunStore: Send + Sync {
    /// Create the tables. Safe to call on every start.
    async fn migrate(&self) -> Result<()>;

    /// Whether the store can serve now: a real read of the file, answered
    /// within a bound. Readiness asks it, so a run store that cannot be read,
    /// or that one write has held for too long, takes the replica out of
    /// service rather than leaving every run call to time out. A store with
    /// nothing to check answers yes.
    async fn ping(&self) -> Result<()> {
        Ok(())
    }

    /// Signalled after every commit that changed a run: wrote an event row, a
    /// span or a status. The event stream and the outbox drain wait on it. A
    /// waiter takes `notified()` before it looks, looks, and then waits on that
    /// future with its own timeout.
    ///
    /// A commit that changed nothing signals nothing. A claim that leased no
    /// step and a heartbeat are both in that set, and waking on either turned
    /// one idle worker into a loop of empty transactions.
    fn notify(&self) -> Arc<Notify>;

    /// Signalled after every commit that may have made a step claimable: a run
    /// started, a step ended, a deadline fired, an event resumed a wait, a run
    /// was cancelled or repaired. The claim long-poll waits on this and on
    /// nothing else, so its own empty claim can never wake it.
    fn runnable(&self) -> Arc<Notify>;

    /// Store a definition at the version it names, under the rule of section
    /// 4: a taken version with an identical body is a no-op, and a taken
    /// version with a different body is refused.
    async fn put_definition(&self, def: DefinitionDocument) -> Result<()>;

    /// Store a definition under the rule of section 4 and answer
    /// `(version, assigned)`, where `assigned` says the daemon chose the
    /// number.
    async fn assign_definition(&self, def: DefinitionDocument) -> Result<(u32, bool)>;

    /// Read one definition version back.
    async fn get_definition(&self, project: &str, id: &str, version: u32) -> Result<Definition>;

    /// The highest stored version of a definition.
    async fn latest_definition(&self, project: &str, id: &str) -> Result<Definition>;

    /// Every stored version of every definition in a project, by id then
    /// version.
    async fn list_definitions(&self, project: &str) -> Result<Vec<Definition>>;

    /// Start a run, and write everything its first cursor requires in the same
    /// transaction. A repost of a held idempotency key returns the existing
    /// run and starts nothing.
    async fn start_run(&self, req: StartRun, now: OffsetDateTime) -> Result<Run>;

    /// Read a run header.
    async fn get_run(&self, run_id: &str) -> Result<Run>;

    /// Runs of a project, most recently updated first, with a keyset cursor.
    async fn list_runs(
        &self,
        filter: RunFilter,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<RunPage>;

    /// Offer up to `limit` claimable steps to one worker, scoped to one run
    /// when `run_id` is given.
    async fn claim(
        &self,
        project: &str,
        owner: &str,
        limit: usize,
        run_id: Option<&str>,
        now: OffsetDateTime,
    ) -> Result<Vec<Lease>>;

    /// Declare that a body is running. Required under `at_most_once`.
    async fn started(&self, lease_id: &str, now: OffsetDateTime) -> Result<LeaseAck>;

    /// Extend a lease. This is the positive evidence a reclaim window waits
    /// for, so it also closes an open window rather than only pushing a
    /// deadline out.
    async fn heartbeat(&self, lease_id: &str, now: OffsetDateTime) -> Result<LeaseAck>;

    /// Finish a step and move the run.
    async fn complete_step(
        &self,
        lease_id: &str,
        completion: StepCompletion,
        now: OffsetDateTime,
    ) -> Result<CompleteOutcome>;

    /// Fail a step, per section 7.6.
    async fn fail_step(
        &self,
        lease_id: &str,
        error: StepError,
        now: OffsetDateTime,
    ) -> Result<FailOutcome>;

    /// Cancel a run and every run under it, per section 7.8. Idempotent on an
    /// ended run.
    async fn cancel(&self, run_id: &str, now: OffsetDateTime) -> Result<Run>;

    /// Handle everything due: every kind of deadline, parked events past
    /// their expiry, and idempotency keys past their window.
    async fn run_due_timers(&self, now: OffsetDateTime) -> Result<TimerReport>;

    /// Re-derive what every stalled run's cursor requires, and sweep the rows
    /// that ended runs left behind.
    async fn reconcile(&self, now: OffsetDateTime) -> Result<ReconcileReport>;

    /// Take an event from outside and route it by `(project, event, key)`.
    async fn deliver_event(&self, event: InboundEvent, now: OffsetDateTime) -> Result<Delivery>;

    /// Deliver an event to one run by id, through that run's own open wait on
    /// `event`.
    async fn signal(
        &self,
        run_id: &str,
        event: &str,
        payload: Option<Value>,
        event_id: Option<String>,
        now: OffsetDateTime,
    ) -> Result<Delivery>;

    /// The run a lease belongs to, so a call addressed by lease id can be
    /// authorised against the run's project before anything is written.
    async fn lease_run(&self, lease_id: &str) -> Result<Run>;

    /// Every attempt of one run, for the run view and for tests.
    async fn attempts(&self, run_id: &str) -> Result<Vec<Attempt>>;

    /// How many attempts each named run made at each node of its definition.
    ///
    /// Rows for the runs that have any, ordered by run id and then node, so
    /// two reads of one set of runs come back in one order. A run with no
    /// attempt row is absent rather than present with a zero: it has not
    /// repeated anything, it has not run anything, and those are different
    /// claims.
    ///
    /// Attempts that were `skipped` are not counted. A skipped position is one
    /// whose `when` guard was false, so the node never ran, and counting it
    /// would let a `foreach` over a filtered list read as repetition.
    ///
    /// The peak node is left to the caller rather than picked here. The caller
    /// is a monitor whose reading has to be reproducible, so the tie-break
    /// belongs somewhere it can be tested without a database; and the row count
    /// is bounded by how many nodes a definition has rather than by how far one
    /// of them fanned out, because the index is already folded away.
    async fn node_attempts(&self, run_ids: &[String]) -> Result<Vec<NodeAttempts>>;

    /// Why one step of a run failed: the ancestors whose output it consumed,
    /// walked backwards along the data dependencies the run's pinned
    /// definition declares. `max_depth` bounds the walk.
    async fn step_cause(&self, run_id: &str, step_key: &str, max_depth: u32) -> Result<StepCause>;

    /// Every deadline of one run.
    async fn timers(&self, run_id: &str) -> Result<Vec<Timer>>;

    /// Every suspension of one run.
    async fn waits(&self, run_id: &str) -> Result<Vec<Wait>>;

    /// One run's event stream, from `after` exclusive, at most `limit` rows.
    async fn events(&self, run_id: &str, after: i64, limit: usize) -> Result<EventPage>;

    /// Claim up to `limit` span outbox rows for a drain, oldest first, never
    /// a close row ahead of its own open row.
    async fn claim_outbox(&self, limit: usize, now: OffsetDateTime) -> Result<Vec<OutboxRow>>;

    /// Delete rows the drain ingested.
    async fn ack_outbox(&self, outbox_ids: &[String]) -> Result<()>;

    /// Give rows back for the next drain pass.
    async fn release_outbox(&self, outbox_ids: &[String]) -> Result<()>;

    /// Delete every run in `project` whose correlation carries `user_id`,
    /// compared as the run was started with it, and everything that hangs off
    /// those runs.
    ///
    /// The unit is the run tree. A tree is deleted only when every run in it
    /// has ended; a tree with a run still going is left whole and counted in
    /// [`SubjectErasure::runs_in_flight`], never skipped in silence.
    async fn erase_subject(&self, project: &str, user_id: &str) -> Result<SubjectErasure>;

    /// Delete every run tree in `project` whose root ended before `cutoff`,
    /// and everything that hangs off it. A tree with any run that has not
    /// ended is never touched, however old it is.
    async fn apply_retention(&self, project: &str, cutoff: OffsetDateTime) -> Result<RunRetention>;
}
