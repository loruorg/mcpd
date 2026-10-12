//! The values a run is made of.
//!
//! Every enum here that is written to a column or to the wire carries an
//! `ALL` array, an exhaustive `index`, an `as_str` and a `parse` derived from
//! `ALL`. The match inside `index` is what stops a new variant compiling until
//! it is listed, and `every_enum_written_to_a_column_lists_all_of_itself` is
//! what stops it being left out of `ALL`. Two of these enums are load-bearing
//! beyond their own definitions: [`Construct`] is what a cursor names, and
//! [`TimerKind`] is every deadline the design has. Both are matched
//! exhaustively elsewhere with no catch-all arm, so adding a variant is a
//! compile error in the places that have to handle it rather than a review
//! finding in a document.
//!
//! Attempts are numbered from zero. The contract does not fix the origin, and
//! this crate keeps the journal's own convention: attempt `0` is the first
//! attempt, and `max_attempts: 3` allows attempts `0`, `1` and `2`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

/// A run identifier.
pub type RunId = String;
/// A step key: the lexical step id with any enclosing branch, item or
/// iteration folded in. `refund`, `checks/fraud`, `notify[3]`, `poll[7]/check`.
pub type StepKey = String;
/// A lease identifier. It is a column, not a memory address: every worker call
/// is addressed by it, so a lease that lived only in the daemon's process
/// would be unreachable the moment the daemon restarted.
pub type LeaseId = String;

/// Where a run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Making progress, or waiting for something that will move it.
    Running,
    /// Reached the end of the graph.
    Succeeded,
    /// Ended on an error nothing handled.
    Failed,
    /// Ended because a deadline passed.
    TimedOut,
    /// Ended because somebody asked it to.
    Cancelled,
}

impl RunStatus {
    /// Whether nothing will move this run again.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        // Exhaustive on purpose. A new status has to decide this question.
        match self {
            RunStatus::Running => false,
            RunStatus::Succeeded
            | RunStatus::Failed
            | RunStatus::TimedOut
            | RunStatus::Cancelled => true,
        }
    }

    /// The wire and column spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Succeeded => "succeeded",
            RunStatus::Failed => "failed",
            RunStatus::TimedOut => "timed_out",
            RunStatus::Cancelled => "cancelled",
        }
    }

    /// Every status, in one place.
    pub const ALL: [RunStatus; 5] = [
        RunStatus::Running,
        RunStatus::Succeeded,
        RunStatus::Failed,
        RunStatus::TimedOut,
        RunStatus::Cancelled,
    ];

    /// A dense index, and the exhaustive match that makes [`RunStatus::ALL`]
    /// checkable.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            RunStatus::Running => 0,
            RunStatus::Succeeded => 1,
            RunStatus::Failed => 2,
            RunStatus::TimedOut => 3,
            RunStatus::Cancelled => 4,
        }
    }

    /// Parse a column value, derived from [`RunStatus::ALL`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        RunStatus::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// Where one attempt at one step is.
///
/// `Orphaned` is a third live state and it is not runnable. An expired lease is
/// the absence of a signal, not evidence the worker is gone, so the attempt
/// waits out a reclaim window before anything decides for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptStatus {
    /// On disk and offerable. The claim path selects exactly these.
    Pending,
    /// Held by a worker that has not declared a body.
    Leased,
    /// Held by a worker that has declared a body.
    Started,
    /// The lease expired. Excluded from the runnable set by the same predicate
    /// that excludes a held one, so no second body can run while the window is
    /// open.
    Orphaned,
    /// Finished with an output.
    Succeeded,
    /// Finished with an error.
    Failed,
    /// A deadline ended it.
    TimedOut,
    /// A false `when` guard meant it never ran.
    Skipped,
}

impl AttemptStatus {
    /// The wire and column spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            AttemptStatus::Pending => "pending",
            AttemptStatus::Leased => "leased",
            AttemptStatus::Started => "started",
            AttemptStatus::Orphaned => "orphaned",
            AttemptStatus::Succeeded => "succeeded",
            AttemptStatus::Failed => "failed",
            AttemptStatus::TimedOut => "timed_out",
            AttemptStatus::Skipped => "skipped",
        }
    }

    /// Every state, in one place.
    pub const ALL: [AttemptStatus; 8] = [
        AttemptStatus::Pending,
        AttemptStatus::Leased,
        AttemptStatus::Started,
        AttemptStatus::Orphaned,
        AttemptStatus::Succeeded,
        AttemptStatus::Failed,
        AttemptStatus::TimedOut,
        AttemptStatus::Skipped,
    ];

    /// A dense index, and the exhaustive match that makes
    /// [`AttemptStatus::ALL`] checkable.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            AttemptStatus::Pending => 0,
            AttemptStatus::Leased => 1,
            AttemptStatus::Started => 2,
            AttemptStatus::Orphaned => 3,
            AttemptStatus::Succeeded => 4,
            AttemptStatus::Failed => 5,
            AttemptStatus::TimedOut => 6,
            AttemptStatus::Skipped => 7,
        }
    }

    /// Parse a column value, derived from [`AttemptStatus::ALL`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        AttemptStatus::ALL.into_iter().find(|v| v.as_str() == s)
    }

    /// Whether this attempt will never change again.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        match self {
            AttemptStatus::Pending
            | AttemptStatus::Leased
            | AttemptStatus::Started
            | AttemptStatus::Orphaned => false,
            AttemptStatus::Succeeded
            | AttemptStatus::Failed
            | AttemptStatus::TimedOut
            | AttemptStatus::Skipped => true,
        }
    }

    /// Whether a worker holds, or held, this attempt: the states with a lease
    /// on them. A pending attempt has no lease and a terminal one has released
    /// it.
    #[must_use]
    pub fn is_held(self) -> bool {
        match self {
            AttemptStatus::Leased | AttemptStatus::Started | AttemptStatus::Orphaned => true,
            AttemptStatus::Pending
            | AttemptStatus::Succeeded
            | AttemptStatus::Failed
            | AttemptStatus::TimedOut
            | AttemptStatus::Skipped => false,
        }
    }
}

/// What a step attempt is an attempt at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    /// Work a worker claims and runs.
    Task,
    /// A task whose body the worker resolves to the agent loop.
    Agent,
    /// Not claimable. The delivery mover and the wait-timeout branch write this
    /// row, which is what makes a wait's output and status readable at all.
    Wait,
}

impl StepKind {
    /// The wire and column spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            StepKind::Task => "task",
            StepKind::Agent => "agent",
            StepKind::Wait => "wait",
        }
    }

    /// Every kind, in one place.
    pub const ALL: [StepKind; 3] = [StepKind::Task, StepKind::Agent, StepKind::Wait];

    /// A dense index, and the exhaustive match that makes [`StepKind::ALL`]
    /// checkable.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            StepKind::Task => 0,
            StepKind::Agent => 1,
            StepKind::Wait => 2,
        }
    }

    /// Parse a column value, derived from [`StepKind::ALL`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        StepKind::ALL.into_iter().find(|v| v.as_str() == s)
    }

    /// Whether the claim path may offer this kind to a worker.
    #[must_use]
    pub fn is_claimable(self) -> bool {
        match self {
            StepKind::Task | StepKind::Agent => true,
            StepKind::Wait => false,
        }
    }
}

/// What a duplicated body would cost, which is what the reclaim window branches
/// on when it closes with no contact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Running the body twice is acceptable. The default.
    #[default]
    AtLeastOnce,
    /// Running the body twice is not acceptable, so an unknown outcome fails
    /// the run rather than guessing.
    AtMostOnce,
}

impl Effect {
    /// The wire and column spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Effect::AtLeastOnce => "at_least_once",
            Effect::AtMostOnce => "at_most_once",
        }
    }

    /// Both effects, in one place.
    pub const ALL: [Effect; 2] = [Effect::AtLeastOnce, Effect::AtMostOnce];

    /// A dense index, and the exhaustive match that makes [`Effect::ALL`]
    /// checkable.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Effect::AtLeastOnce => 0,
            Effect::AtMostOnce => 1,
        }
    }

    /// Parse a column value, derived from [`Effect::ALL`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Effect::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// Every deadline in the design.
///
/// The due-timer tick matches this enum exhaustively with no catch-all, so a
/// new kind does not compile until it is handled, and
/// `every_timer_kind_has_a_writer` drives one real path per kind and asserts
/// the row is on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimerKind {
    /// A `sleep` or a `sleep_until` coming due.
    Sleep,
    /// A lease that was not heartbeated.
    LeaseExpiry,
    /// The close of the reclaim window an expired lease opened.
    Reclaim,
    /// The whole run's ceiling.
    RunTimeout,
    /// One step's ceiling, which bounds a worker that heartbeats for ever.
    StepTimeout,
    /// A `wait_event` whose event never arrived.
    WaitTimeout,
    /// The backoff before a requeued attempt becomes claimable. Its firing is
    /// what writes the next attempt row.
    Retry,
}

impl TimerKind {
    /// Every kind, in one place, so a test can iterate them.
    pub const ALL: [TimerKind; 7] = [
        TimerKind::Sleep,
        TimerKind::LeaseExpiry,
        TimerKind::Reclaim,
        TimerKind::RunTimeout,
        TimerKind::StepTimeout,
        TimerKind::WaitTimeout,
        TimerKind::Retry,
    ];

    /// A dense index, and the exhaustive match that makes [`TimerKind::ALL`]
    /// checkable.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            TimerKind::Sleep => 0,
            TimerKind::LeaseExpiry => 1,
            TimerKind::Reclaim => 2,
            TimerKind::RunTimeout => 3,
            TimerKind::StepTimeout => 4,
            TimerKind::WaitTimeout => 5,
            TimerKind::Retry => 6,
        }
    }

    /// The wire and column spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            TimerKind::Sleep => "sleep",
            TimerKind::LeaseExpiry => "lease_expiry",
            TimerKind::Reclaim => "reclaim",
            TimerKind::RunTimeout => "run_timeout",
            TimerKind::StepTimeout => "step_timeout",
            TimerKind::WaitTimeout => "wait_timeout",
            TimerKind::Retry => "retry",
        }
    }

    /// Parse a column value, derived from [`TimerKind::ALL`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        TimerKind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// What the run's event log records. Section 8 of the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEventKind {
    /// The run row exists. Payload: `{ definition_id, definition_version }`.
    RunStarted,
    /// The cursor moved. Payload: `{ index, construct, step_key? }`.
    RunCursor,
    /// A worker took an attempt. Payload: `{ step_key, attempt, worker_id }`.
    StepLeased,
    /// A worker declared a body. Payload: `{ step_key, attempt }`.
    StepStarted,
    /// An attempt succeeded. Payload: `{ step_key, attempt }`.
    StepFinished,
    /// An attempt failed or timed out. Payload:
    /// `{ step_key, attempt, class, message, attempts_left }`.
    StepFailed,
    /// A guard was false. Payload: `{ step_key }`.
    StepSkipped,
    /// The run entered a wait. Payload: `{ step_key, event, key, expires_at }`.
    RunWaiting,
    /// An event resumed it. Payload: `{ step_key, event_id }`.
    RunResumed,
    /// The run reached a terminal status. Payload: `{ status, error? }`.
    RunEnded,
}

impl RunEventKind {
    /// Every kind, in one place.
    pub const ALL: [RunEventKind; 10] = [
        RunEventKind::RunStarted,
        RunEventKind::RunCursor,
        RunEventKind::StepLeased,
        RunEventKind::StepStarted,
        RunEventKind::StepFinished,
        RunEventKind::StepFailed,
        RunEventKind::StepSkipped,
        RunEventKind::RunWaiting,
        RunEventKind::RunResumed,
        RunEventKind::RunEnded,
    ];

    /// A dense index, and the exhaustive match that makes
    /// [`RunEventKind::ALL`] checkable.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            RunEventKind::RunStarted => 0,
            RunEventKind::RunCursor => 1,
            RunEventKind::StepLeased => 2,
            RunEventKind::StepStarted => 3,
            RunEventKind::StepFinished => 4,
            RunEventKind::StepFailed => 5,
            RunEventKind::StepSkipped => 6,
            RunEventKind::RunWaiting => 7,
            RunEventKind::RunResumed => 8,
            RunEventKind::RunEnded => 9,
        }
    }

    /// The wire and column spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RunEventKind::RunStarted => "run_started",
            RunEventKind::RunCursor => "run_cursor",
            RunEventKind::StepLeased => "step_leased",
            RunEventKind::StepStarted => "step_started",
            RunEventKind::StepFinished => "step_finished",
            RunEventKind::StepFailed => "step_failed",
            RunEventKind::StepSkipped => "step_skipped",
            RunEventKind::RunWaiting => "run_waiting",
            RunEventKind::RunResumed => "run_resumed",
            RunEventKind::RunEnded => "run_ended",
        }
    }

    /// Parse a column value, derived from [`RunEventKind::ALL`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        RunEventKind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// Why a worker call was refused with a 409. Section 7.4 of the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictReason {
    /// A later attempt exists, or another lease terminated this attempt. The
    /// worker abandons the work.
    Fenced,
    /// This lease's own earlier `complete` or `fail` landed and its reply was
    /// lost. The worker treats the step as done.
    AlreadyApplied,
    /// `complete` or `fail` on a `leased` `at_most_once` attempt. A bug in the
    /// worker: it has to post `started` first.
    NotStarted,
    /// The run has ended. The worker stops, and stops claiming for this run.
    RunTerminal,
}

impl ConflictReason {
    /// Every reason, in one place.
    pub const ALL: [ConflictReason; 4] = [
        ConflictReason::Fenced,
        ConflictReason::AlreadyApplied,
        ConflictReason::NotStarted,
        ConflictReason::RunTerminal,
    ];

    /// A dense index, and the exhaustive match that makes
    /// [`ConflictReason::ALL`] checkable.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            ConflictReason::Fenced => 0,
            ConflictReason::AlreadyApplied => 1,
            ConflictReason::NotStarted => 2,
            ConflictReason::RunTerminal => 3,
        }
    }

    /// The wire spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ConflictReason::Fenced => "fenced",
            ConflictReason::AlreadyApplied => "already_applied",
            ConflictReason::NotStarted => "not_started",
            ConflictReason::RunTerminal => "run_terminal",
        }
    }

    /// Parse a wire value, derived from [`ConflictReason::ALL`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        ConflictReason::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// How a worker classified a failure. Section 7.6 of the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepErrorClass {
    /// Transient. Retried while the budget lasts.
    Retryable,
    /// Will not succeed on a retry.
    Fatal,
    /// The step input does not satisfy the body's contract. A retry cannot
    /// fix the input.
    InvalidInput,
    /// This worker cannot serve this step or version. The attempt returns to
    /// pending after one lease length without spending the retry budget.
    Unservable,
    /// The worker observed a cancel.
    Cancelled,
}

impl StepErrorClass {
    /// Every class, in one place.
    pub const ALL: [StepErrorClass; 5] = [
        StepErrorClass::Retryable,
        StepErrorClass::Fatal,
        StepErrorClass::InvalidInput,
        StepErrorClass::Unservable,
        StepErrorClass::Cancelled,
    ];

    /// A dense index, and the exhaustive match that makes
    /// [`StepErrorClass::ALL`] checkable.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            StepErrorClass::Retryable => 0,
            StepErrorClass::Fatal => 1,
            StepErrorClass::InvalidInput => 2,
            StepErrorClass::Unservable => 3,
            StepErrorClass::Cancelled => 4,
        }
    }

    /// The wire and column spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            StepErrorClass::Retryable => "retryable",
            StepErrorClass::Fatal => "fatal",
            StepErrorClass::InvalidInput => "invalid_input",
            StepErrorClass::Unservable => "unservable",
            StepErrorClass::Cancelled => "cancelled",
        }
    }

    /// Parse a column value, derived from [`StepErrorClass::ALL`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        StepErrorClass::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// What a node does when it fails after its retries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OnError {
    /// End the run `failed` with the message. The default.
    #[default]
    FailRun,
    /// Mark the step `failed` and advance, so a later `when` can read
    /// `steps.<id>.status` and `steps.<id>.error`.
    Continue,
}

impl OnError {
    /// Both values, in one place.
    pub const ALL: [OnError; 2] = [OnError::FailRun, OnError::Continue];

    /// A dense index, and the exhaustive match that makes [`OnError::ALL`]
    /// checkable.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            OnError::FailRun => 0,
            OnError::Continue => 1,
        }
    }

    /// The wire spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            OnError::FailRun => "fail_run",
            OnError::Continue => "continue",
        }
    }

    /// Parse a wire value, derived from [`OnError::ALL`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        OnError::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// How retries are spaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Backoff {
    /// Every retry waits `initial_ms`.
    Fixed,
    /// Retry `n` waits `initial_ms * 2^(n-1)`, capped at `max_ms`. The
    /// default.
    #[default]
    Exponential,
}

impl Backoff {
    /// Both values, in one place.
    pub const ALL: [Backoff; 2] = [Backoff::Fixed, Backoff::Exponential];

    /// A dense index, and the exhaustive match that makes [`Backoff::ALL`]
    /// checkable.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Backoff::Fixed => 0,
            Backoff::Exponential => 1,
        }
    }

    /// The wire spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Backoff::Fixed => "fixed",
            Backoff::Exponential => "exponential",
        }
    }

    /// Parse a wire value, derived from [`Backoff::ALL`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Backoff::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// What a `wait_event` does when its deadline passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WaitTimeout {
    /// End the run `failed`. The default.
    #[default]
    Fail,
    /// Write the wait's attempt as `timed_out` and advance.
    Continue,
}

impl WaitTimeout {
    /// Both values, in one place.
    pub const ALL: [WaitTimeout; 2] = [WaitTimeout::Fail, WaitTimeout::Continue];

    /// A dense index, and the exhaustive match that makes
    /// [`WaitTimeout::ALL`] checkable.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            WaitTimeout::Fail => 0,
            WaitTimeout::Continue => 1,
        }
    }

    /// The wire spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            WaitTimeout::Fail => "fail",
            WaitTimeout::Continue => "continue",
        }
    }

    /// Parse a wire value, derived from [`WaitTimeout::ALL`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        WaitTimeout::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// What kind of thing a definition is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DefinitionKind {
    /// A graph of steps. The only kind in version 1.
    #[default]
    Workflow,
}

impl DefinitionKind {
    /// Every kind, in one place.
    pub const ALL: [DefinitionKind; 1] = [DefinitionKind::Workflow];

    /// A dense index, and the exhaustive match that makes
    /// [`DefinitionKind::ALL`] checkable.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            DefinitionKind::Workflow => 0,
        }
    }

    /// The wire and column spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DefinitionKind::Workflow => "workflow",
        }
    }

    /// Parse a column value, derived from [`DefinitionKind::ALL`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        DefinitionKind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// What happened to an inbound event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventOutcome {
    /// A waiting run took it and moved.
    Delivered,
    /// No run was waiting, so it is on the shelf for one that arrives later.
    Parked,
    /// This event id has been seen before and nothing was written.
    Duplicate,
}

impl EventOutcome {
    /// Every outcome, in one place.
    pub const ALL: [EventOutcome; 3] =
        [EventOutcome::Delivered, EventOutcome::Parked, EventOutcome::Duplicate];

    /// A dense index, and the exhaustive match that makes
    /// [`EventOutcome::ALL`] checkable.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            EventOutcome::Delivered => 0,
            EventOutcome::Parked => 1,
            EventOutcome::Duplicate => 2,
        }
    }

    /// The wire spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            EventOutcome::Delivered => "delivered",
            EventOutcome::Parked => "parked",
            EventOutcome::Duplicate => "duplicate",
        }
    }

    /// Parse a wire value, derived from [`EventOutcome::ALL`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        EventOutcome::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// Which construct a cursor names, without the values it carries.
///
/// This exists so a table-driven test can iterate the constructs. It is derived
/// from [`Construct::kind`], which is an exhaustive match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstructKind {
    /// Claimable work.
    Task,
    /// Claimable work whose body is the agent loop.
    Agent,
    /// A relative delay.
    Sleep,
    /// An absolute delay.
    SleepUntil,
    /// A suspension addressed by an event and a correlation key.
    WaitEvent,
    /// A fixed fan-out over named branches.
    Parallel,
    /// A fan-out over items.
    Foreach,
    /// One iteration of a bounded loop.
    Loop,
    /// A child run.
    Subworkflow,
    /// The end of the graph.
    End,
}

impl ConstructKind {
    /// Every construct the design names, plus the end of the graph.
    pub const ALL: [ConstructKind; 10] = [
        ConstructKind::Task,
        ConstructKind::Agent,
        ConstructKind::Sleep,
        ConstructKind::SleepUntil,
        ConstructKind::WaitEvent,
        ConstructKind::Parallel,
        ConstructKind::Foreach,
        ConstructKind::Loop,
        ConstructKind::Subworkflow,
        ConstructKind::End,
    ];

    /// A dense index, and the exhaustive match that makes
    /// [`ConstructKind::ALL`] checkable.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            ConstructKind::Task => 0,
            ConstructKind::Agent => 1,
            ConstructKind::Sleep => 2,
            ConstructKind::SleepUntil => 3,
            ConstructKind::WaitEvent => 4,
            ConstructKind::Parallel => 5,
            ConstructKind::Foreach => 6,
            ConstructKind::Loop => 7,
            ConstructKind::Subworkflow => 8,
            ConstructKind::End => 9,
        }
    }

    /// The wire spelling, which is what the `run_cursor` payload carries.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ConstructKind::Task => "task",
            ConstructKind::Agent => "agent",
            ConstructKind::Sleep => "sleep",
            ConstructKind::SleepUntil => "sleep_until",
            ConstructKind::WaitEvent => "wait_event",
            ConstructKind::Parallel => "parallel",
            ConstructKind::Foreach => "foreach",
            ConstructKind::Loop => "loop",
            ConstructKind::Subworkflow => "subworkflow",
            ConstructKind::End => "end",
        }
    }
}

/// One claimable slot a cursor position requires, with everything the row
/// write needs.
///
/// `skipped` is decided when the cursor is resolved: a branch, item or body
/// whose own `when` was false is written as a `skipped` attempt rather than a
/// `pending` one, so the fan-out still holds every slot and settles when the
/// rest have ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    /// The step key of the attempt rows.
    pub step_key: StepKey,
    /// What kind of step.
    pub kind: StepKind,
    /// What a duplicated body would cost.
    pub effect: Effect,
    /// When one attempt's own ceiling passes, if the node declares one. Re-armed
    /// on every new attempt, because `step_timeout_ms` bounds one attempt.
    #[serde(with = "time::serde::rfc3339::option")]
    pub step_deadline: Option<OffsetDateTime>,
    /// The item index inside a `foreach`, which is what puts `item` and
    /// `index` in the input template's scope.
    #[serde(default)]
    pub item_index: Option<usize>,
    /// The iteration inside a `loop`, which is what puts `iter.index` in
    /// scope.
    #[serde(default)]
    pub iteration: Option<u32>,
    /// Whether this slot's own guard was false.
    #[serde(default)]
    pub skipped: bool,
}

/// What the cursor names, with everything the materialising transaction needs
/// to write the rows the construct requires.
///
/// The values are resolved by the walker against the journal the transaction
/// already holds, which is what keeps materialisation to arithmetic and row
/// writes: no user code, no network, nothing that can block a transaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "construct", rename_all = "snake_case")]
pub enum Construct {
    /// Claimable work.
    Task {
        /// The one slot.
        slot: Slot,
    },
    /// Claimable work whose body the worker resolves to the agent loop.
    Agent {
        /// The one slot.
        slot: Slot,
    },
    /// A relative delay, already resolved to an instant.
    Sleep {
        /// The step this delay belongs to.
        step: StepKey,
        /// When it is over.
        #[serde(with = "time::serde::rfc3339")]
        due_at: OffsetDateTime,
    },
    /// An absolute delay.
    SleepUntil {
        /// The step this delay belongs to.
        step: StepKey,
        /// When it is over.
        #[serde(with = "time::serde::rfc3339")]
        due_at: OffsetDateTime,
    },
    /// A suspension an inbound event resumes.
    WaitEvent {
        /// The step this suspension belongs to.
        step: StepKey,
        /// The event name an inbound call must match.
        event: String,
        /// The key that picks this run out of every run waiting on that event,
        /// rendered per run.
        correlation_key: String,
        /// When waiting stops.
        #[serde(with = "time::serde::rfc3339")]
        expires_at: OffsetDateTime,
        /// Whether an event that arrived before the run got here counts.
        accept_parked: bool,
        /// What the deadline passing does.
        on_timeout: WaitTimeout,
    },
    /// A fixed fan-out.
    Parallel {
        /// The step the fan-out belongs to.
        step: StepKey,
        /// One slot per branch, all written before any is offered.
        slots: Vec<Slot>,
    },
    /// A fan-out over items.
    Foreach {
        /// The step the fan-out belongs to.
        step: StepKey,
        /// One slot per item, in item order.
        slots: Vec<Slot>,
        /// How many items may be claimable at once.
        concurrency: usize,
    },
    /// One iteration of a bounded loop.
    Loop {
        /// The loop step.
        step: StepKey,
        /// This iteration's body.
        slot: Slot,
        /// Which iteration this is, from zero.
        iteration: u32,
        /// The bound. Carried on the cursor rather than re-read from the
        /// definition, so advancing a loop needs nothing but the cursor.
        max_iterations: u32,
    },
    /// A child run.
    Subworkflow {
        /// The step the child hangs off.
        step: StepKey,
        /// The child's run id, minted by the parent's transaction so the child
        /// row and the cursor that names it commit together.
        child_run_id: RunId,
        /// Which definition the child runs.
        definition_id: String,
        /// Pinned at the parent's start, read from `bindings`.
        definition_version: u32,
        /// The parent step's own span, which the child's root span nests under.
        span_id: String,
    },
    /// The end of the graph.
    End {
        /// How the run ended.
        status: RunStatus,
    },
}

impl Construct {
    /// Which construct this is.
    ///
    /// Exhaustive with no catch-all: a new construct does not compile until it
    /// is named here, and naming it here is what puts it in front of the
    /// requirements match.
    #[must_use]
    pub fn kind(&self) -> ConstructKind {
        match self {
            Construct::Task { .. } => ConstructKind::Task,
            Construct::Agent { .. } => ConstructKind::Agent,
            Construct::Sleep { .. } => ConstructKind::Sleep,
            Construct::SleepUntil { .. } => ConstructKind::SleepUntil,
            Construct::WaitEvent { .. } => ConstructKind::WaitEvent,
            Construct::Parallel { .. } => ConstructKind::Parallel,
            Construct::Foreach { .. } => ConstructKind::Foreach,
            Construct::Loop { .. } => ConstructKind::Loop,
            Construct::Subworkflow { .. } => ConstructKind::Subworkflow,
            Construct::End { .. } => ConstructKind::End,
        }
    }

    /// The lexical step id this position belongs to, or none at the end.
    #[must_use]
    pub fn step(&self) -> Option<&str> {
        match self {
            Construct::Task { slot } | Construct::Agent { slot } => Some(&slot.step_key),
            Construct::Sleep { step, .. }
            | Construct::SleepUntil { step, .. }
            | Construct::WaitEvent { step, .. }
            | Construct::Parallel { step, .. }
            | Construct::Foreach { step, .. }
            | Construct::Loop { step, .. }
            | Construct::Subworkflow { step, .. } => Some(step),
            Construct::End { .. } => None,
        }
    }
}

/// The frontier of a run: which position in the graph, and what sits there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cursor {
    /// Index into the definition's `steps`, or its length at the end.
    pub index: usize,
    /// The construct at that position, resolved.
    pub at: Construct,
    /// Whether the node's own `when` was false when the cursor reached it. A
    /// skipped position writes attempt 0 as `skipped` and settles on arrival.
    #[serde(default)]
    pub skipped: bool,
}

/// A stored definition. Immutable per version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    /// Which project owns it.
    pub project: String,
    /// Its identifier within the project.
    pub id: String,
    /// Its version. A run pins one.
    pub version: u32,
    /// What kind of thing it is.
    pub kind: DefinitionKind,
    /// The document body, exactly as stored.
    pub body: Value,
    /// When this version was written.
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// A run header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    /// Its identifier.
    pub run_id: RunId,
    /// Which project owns it.
    pub project: String,
    /// Which definition it runs.
    pub definition_id: String,
    /// Which version of it.
    pub definition_version: u32,
    /// Where it is.
    pub status: RunStatus,
    /// The frontier.
    pub cursor: Cursor,
    /// What it was started with.
    pub input: Option<Value>,
    /// What it produced: the rendered `output` template.
    pub output: Option<Value>,
    /// Why it ended, when it did not succeed.
    pub error: Option<String>,
    /// The class of the failure that ended it, when a step's failure did.
    pub error_class: Option<StepErrorClass>,
    /// The trace every span of this run belongs to. 32 lowercase hex.
    pub trace_id: String,
    /// The run's root span, which every step span nests under.
    pub root_span_id: String,
    /// The accumulated key/value block, merged shallowly on every `complete`
    /// that sends one.
    pub state: Value,
    /// The correlation stored at start, with `project` and `run_id` set.
    pub correlation: Value,
    /// The caller's key, unique per project while it is held.
    pub idempotency_key: Option<String>,
    /// Every `subworkflow` step's pinned version, keyed by step id:
    /// `{ "<step id>": { "definition_id": ..., "version": n } }`.
    pub bindings: Value,
    /// The compare-and-set column.
    pub version: i64,
    /// How many steps this run has completed.
    pub steps_used: i64,
    /// The step ceiling from this run's own definition.
    pub max_steps: Option<i64>,
    /// What this run's completed steps have cost, in USD.
    pub cost_usd: f64,
    /// The cost ceiling from this run's own definition.
    pub max_cost_usd: Option<f64>,
    /// The run timeout, as an instant.
    pub deadline_at: Option<OffsetDateTime>,
    /// The root of the run tree. Ceilings are enforced against it as well as
    /// against this run.
    pub root_run_id: RunId,
    /// The run that started this one.
    pub parent_run_id: Option<RunId>,
    /// The step in the parent that this run is the body of.
    pub parent_step_key: Option<StepKey>,
    /// How far from the root.
    pub depth: i64,
    /// When it started.
    pub created_at: OffsetDateTime,
    /// When it last moved.
    pub updated_at: OffsetDateTime,
}

/// One attempt at one step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attempt {
    /// Which run.
    pub run_id: RunId,
    /// Which step.
    pub step_key: StepKey,
    /// Which attempt, from zero.
    pub attempt: i64,
    /// What kind of step.
    pub kind: StepKind,
    /// What a duplicated body would cost.
    pub effect: Effect,
    /// Where the attempt is.
    pub status: AttemptStatus,
    /// The rendered input template, which is what the lease hands over.
    pub input: Option<Value>,
    /// What it produced.
    pub output: Option<Value>,
    /// Why it failed.
    pub error: Option<String>,
    /// How the failure was classified.
    pub error_class: Option<StepErrorClass>,
    /// Structured detail the worker sent with the failure.
    pub error_details: Option<Value>,
    /// The usage the worker reported.
    pub usage: Option<Value>,
    /// What the daemon priced that usage at.
    pub cost_usd: Option<f64>,
    /// The span the daemon opened for this attempt. 16 lowercase hex.
    pub span_id: String,
    /// The step timeout the node declares, handed to the worker as
    /// `deadline_ms`.
    pub deadline_ms: Option<i64>,
    /// Whether this attempt counts against `retry.max_attempts`. An
    /// `unservable` failure and a never-started reclaim do not.
    pub budget_spent: bool,
    /// The lease that addresses it, if it is or was held.
    pub lease_id: Option<LeaseId>,
    /// Who holds it.
    pub lease_owner: Option<String>,
    /// When the lease lapses.
    pub lease_until: Option<OffsetDateTime>,
    /// When the lease was granted. The open span's start time when `started`
    /// was never posted.
    pub leased_at: Option<OffsetDateTime>,
    /// When a body was declared. Null means none ever was, which is the
    /// discriminator the reclaim branch reads.
    pub started_at: Option<OffsetDateTime>,
    /// When the lease lapsed.
    pub orphaned_at: Option<OffsetDateTime>,
    /// When the reclaim window closes.
    pub reclaim_until: Option<OffsetDateTime>,
    /// When it reached a terminal status.
    pub ended_at: Option<OffsetDateTime>,
    /// The lease whose `complete` or `fail` ended it. Null when the daemon
    /// ended it. This is what makes `already_applied` decidable.
    pub ended_by_lease_id: Option<LeaseId>,
    /// When the row was written: the slot for attempt 0, the requeue for a
    /// later one, the delivery or the deadline for a wait.
    pub created_at: OffsetDateTime,
}

/// A deadline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timer {
    /// Which run.
    pub run_id: RunId,
    /// Which step, or empty for a run-level deadline.
    pub step_key: StepKey,
    /// Which deadline.
    pub kind: TimerKind,
    /// When it fires.
    pub due_at: OffsetDateTime,
}

/// A suspension waiting for an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wait {
    /// Which run.
    pub run_id: RunId,
    /// Which step.
    pub step_key: StepKey,
    /// Which project.
    pub project: String,
    /// The event name.
    pub event: String,
    /// The key that picks this run out.
    pub correlation_key: String,
    /// When waiting stops. The `wait_timeout` timer row is what fires; the
    /// delivery lookup reads this column to skip a wait whose window has
    /// closed but whose timer has not yet been ticked.
    pub expires_at: OffsetDateTime,
    /// The `workflow_wait_event` span opened for it.
    pub span_id: String,
}

/// A lifecycle record. A disconnected reader reattaches at a sequence number
/// instead of replaying live.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunEvent {
    /// Which run.
    pub run_id: RunId,
    /// Its position in that run's stream.
    pub seq: i64,
    /// What happened.
    pub kind: RunEventKind,
    /// The detail, in the shape section 8 gives for the kind.
    pub payload: Value,
    /// When.
    pub at: OffsetDateTime,
}

/// A granted lease, which is what a worker is handed. Section 7.2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lease {
    /// The address of every later call about this attempt.
    pub lease_id: LeaseId,
    /// Which run.
    pub run_id: RunId,
    /// Which project.
    pub project: String,
    /// Which definition the run pins.
    pub definition_id: String,
    /// Which version of it.
    pub definition_version: u32,
    /// Which step.
    pub step_key: StepKey,
    /// What kind of step.
    pub kind: StepKind,
    /// Which attempt. The fence.
    pub attempt: i64,
    /// What a duplicated body would cost.
    pub effect: Effect,
    /// When the lease lapses without a heartbeat.
    pub expires_at: OffsetDateTime,
    /// The step timeout, or absent.
    pub deadline_ms: Option<i64>,
    /// `sha256` hex of `run_id:step_key`, fixed by the step and not by the
    /// attempt, for a third party that accepts one.
    pub idempotency_key: String,
    /// The rendered input template.
    pub input: Value,
    /// Every ended step so far, keyed by step id, in the shape of section 3.3.
    pub prior: Value,
    /// The run's accumulated state.
    pub state: Value,
    /// The run's correlation with `run_id` set.
    pub correlation: Value,
    /// The run's trace.
    pub trace_id: String,
    /// The span the daemon opened for this attempt.
    pub span_id: String,
    /// The run's root span.
    pub parent_span_id: String,
}

/// An event arriving from outside.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboundEvent {
    /// Which project.
    pub project: String,
    /// The event name.
    pub event: String,
    /// The key that picks a waiting run out.
    pub correlation_key: String,
    /// The id that makes a retry a no-op.
    pub event_id: String,
    /// Whether the daemon derived `event_id` from the bytes because the caller
    /// sent none. A derived id deduplicates parking only: it is a guess made
    /// from bytes, and two genuine approvals for two runs can carry the same
    /// bytes.
    pub id_is_derived: bool,
    /// What it carries.
    pub payload: Option<Value>,
}

impl InboundEvent {
    /// The id the daemon uses when the caller sent none:
    /// `sha256(project:event:key:payload)` as lowercase hex.
    #[must_use]
    pub fn derived_id(project: &str, event: &str, key: &str, payload: Option<&Value>) -> String {
        use sha2::Digest;
        let payload = payload.map_or_else(|| "null".to_string(), crate::canonical::to_string);
        let mut hasher = sha2::Sha256::new();
        hasher.update(project.as_bytes());
        hasher.update(b":");
        hasher.update(event.as_bytes());
        hasher.update(b":");
        hasher.update(key.as_bytes());
        hasher.update(b":");
        hasher.update(payload.as_bytes());
        hex::encode(hasher.finalize())
    }
}

/// What happened to an inbound event, and the id used for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    /// The outcome.
    pub outcome: EventOutcome,
    /// The event id the daemon recorded, whether supplied or derived.
    pub event_id: String,
}

/// One row of the span outbox, in the wire `Span` shape, ready to ingest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutboxRow {
    /// The row's own identity.
    pub outbox_id: String,
    /// Which run.
    pub run_id: RunId,
    /// The idempotency key the drain sends with the batch.
    pub ingest_key: String,
    /// The order rows for one span were written in. The drain sends them in
    /// this order.
    pub seq: i64,
    /// `open` or `close`.
    pub phase: String,
    /// The span, in the wire shape.
    pub payload: Value,
    /// The span this row writes.
    pub span_id: String,
}
