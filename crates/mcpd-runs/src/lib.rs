//! The durable run core.
//!
//! A run is a row, a cursor, a journal of step attempts, a set of deadlines, a
//! set of suspensions, an event log and a span outbox. This crate owns all of
//! them and nothing else. It has no HTTP surface and no worker protocol over
//! the wire: those sit above it in `mcpd-server`, and the binding contract for
//! both is `internal/run-contract-v1.md`.
//!
//! # The defect this crate is shaped against
//!
//! The design behind it was reviewed five times. Every round found the same
//! class of fault, and the fifth still found three: a deadline declared in an
//! enum that no code path wrote, and a reconciler whose repair list silently
//! omitted it. Prose review cannot converge on that, because the two lists
//! being compared are in two documents and both are maintained by hand.
//!
//! So there is one list, in [`requirements`], and the type system holds it:
//!
//! * [`requirements::cursor_requirements`] is an exhaustive match over
//!   [`model::Construct`]. Adding a construct to the graph vocabulary does not
//!   compile until somebody says what it writes.
//! * [`requirements::attempt_requirements`] is an exhaustive match over
//!   [`model::AttemptStatus`], and it owns the two deadlines a cursor never
//!   names.
//! * The due-timer tick is an exhaustive match over [`model::TimerKind`], so an
//!   eighth deadline does not compile until somebody says what it fires.
//! * The materialiser and the reconciler are the same function called twice,
//!   with the cursor write skipped on the repair path. They are not two lists
//!   that agree; there is one list.
//!
//! None of those matches has a catch-all arm. A catch-all reintroduces exactly
//! the fault this crate exists to prevent.
//!
//! # What is here
//!
//! Definitions with immutable versions (section 4 of the contract), runs with
//! a start idempotency key and a trace (section 5), events and waits with
//! parking and the supplied-versus-derived deduplication split (section 6),
//! the lease protocol with the four 409 reasons, retries, `on_error` and the
//! reclaim window (section 7), the event log (section 8), and the span outbox
//! every change enqueues into (section 9). Guards, templates and predicates are
//! evaluated here against the context [`context`] builds, and every document is
//! checked by [`validate`] before it is stored.
//!
//! # What is not here
//!
//! Section 10 of the contract: request keys and a `step_requests` table,
//! `after` constraints, compensation, retry-from-step, token deltas, a
//! Postgres run store, the agent loop and multi-step bodies for a branch, an
//! item or an iteration. The pricing of usage is the server's: this crate
//! takes a cost in USD and charges it, and holds no price book.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod canonical;
pub mod cause;
pub mod context;
pub mod graph;
pub mod model;
pub mod predicate;
pub mod requirements;
pub mod sqlite;
pub mod store;
pub mod template;
pub mod tick;
pub mod validate;

pub use cause::{CauseLink, StepCause, ValueKind};
pub use graph::{Graph, Limits, Node, Placed, Refusal, Retry, Site};
pub use model::{
    Attempt, AttemptStatus, Backoff, ConflictReason, Construct, ConstructKind, Cursor, Definition,
    DefinitionKind, Delivery, Effect, EventOutcome, InboundEvent, Lease, OnError, OutboxRow, Run,
    RunEvent, RunEventKind, RunId, RunStatus, Slot, StepErrorClass, StepKey, StepKind, Timer,
    TimerKind, Wait, WaitTimeout,
};
pub use predicate::Predicate;
pub use requirements::{
    attempt_requirements, cursor_requirements, settles_on_arrival, Requirement, Requirements,
};
pub use sqlite::SqliteRunStore;
pub use store::{
    CompleteOutcome, DefinitionDocument, EventPage, FailOutcome, LeaseAck, NodeAttempts,
    ReconcileReport, RunConfig, RunFilter, RunPage, RunRetention, RunStore, StartRun,
    StepCompletion, StepError, SubjectErasure, TickReport, TimerReport,
};
pub use tick::tick;

/// Anything that can go wrong moving a run.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The backend rejected or failed a statement.
    #[error("run store backend error: {0}")]
    Backend(String),

    /// The caller asked for something that does not exist.
    #[error("{kind} {id} not found")]
    NotFound {
        /// What kind of thing.
        kind: &'static str,
        /// Its identifier.
        id: String,
    },

    /// A worker call was refused, with the closed reason of section 7.4 of the
    /// contract. The server answers 409 with every field of this variant, so a
    /// worker can tell a fence from its own landed call.
    #[error("conflict ({}): {message}", reason.as_str())]
    Conflict {
        /// Which conflict.
        reason: ConflictReason,
        /// The attempt number the lease holds.
        attempt: i64,
        /// The state that attempt is in.
        attempt_status: Option<AttemptStatus>,
        /// Where the run is.
        run_status: RunStatus,
        /// What is true, in words a worker's log can carry.
        message: String,
    },

    /// A named definition version is already held by a different body. A
    /// stored version never changes; the caller names another number.
    #[error("definition {id} version {version} in project {project} is already held by a different body")]
    VersionTaken {
        /// Which project.
        project: String,
        /// Which definition.
        id: String,
        /// Which version.
        version: u32,
    },

    /// The request cannot be carried out as written.
    #[error("{0}")]
    Invalid(String),

    /// Two writers reached the same run header and this one lost the
    /// compare-and-set. The store runs the transaction again by itself, so a
    /// caller only sees this when every retry lost.
    #[error("run header moved: {0}")]
    Retry(String),
}

/// Convenience alias. The second parameter lets the walker name its own
/// refusal type without a second alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The longest duration any field of a definition may name, and the longest
/// `retry_after_ms` a worker may send: 3650 days, in milliseconds.
///
/// Every deadline is stored as nanoseconds since the Unix epoch in a signed
/// 64-bit column, which runs out in April 2262, and adding a duration to the
/// clock panics when the sum leaves the range the time library can hold. The
/// release build aborts on a panic, so one sleep step with a large enough `ms`
/// stopped the daemon. This bound keeps every sum far inside both ranges and
/// still admits any wait a business process plausibly has.
pub const MAX_DURATION_MS: i64 = 315_360_000_000;

/// `now` plus `ms` milliseconds, without the panic `+` has on overflow.
///
/// The validator keeps a stored definition inside [`MAX_DURATION_MS`], so the
/// saturating branch is for a definition stored before that bound existed. It
/// lands on the latest instant the store can write, which reads as "never"
/// rather than as a deadline that has already passed.
#[must_use]
pub(crate) fn after_ms(now: time::OffsetDateTime, ms: i64) -> time::OffsetDateTime {
    now.checked_add(time::Duration::milliseconds(ms))
        .map_or_else(latest_instant, |t| t.min(latest_instant()))
}

/// The latest instant a timestamp column holds: `i64::MAX` nanoseconds after
/// the Unix epoch.
#[must_use]
pub(crate) fn latest_instant() -> time::OffsetDateTime {
    time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(i64::MAX))
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
}

/// Mint an identifier.
///
/// Random rather than sequential because a run id appears in URLs a worker
/// holds, and a guessable one would let a caller address a run it was never
/// handed.
#[must_use]
pub(crate) fn new_id(prefix: &str) -> String {
    format!("{prefix}_{:016x}", rand::random::<u64>())
}

/// A span id in the OpenTelemetry shape: 16 lowercase hex, never all zero.
#[must_use]
pub fn new_span_id() -> String {
    loop {
        let v: u64 = rand::random();
        if v != 0 {
            return format!("{v:016x}");
        }
    }
}

/// A trace id in the OpenTelemetry shape: 32 lowercase hex, never all zero.
#[must_use]
pub fn new_trace_id() -> String {
    loop {
        let v: u128 = rand::random();
        if v != 0 {
            return format!("{v:032x}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_have_the_open_telemetry_shape() {
        let span = new_span_id();
        let trace = new_trace_id();
        assert_eq!(span.len(), 16);
        assert_eq!(trace.len(), 32);
        assert!(span.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert!(trace.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_ne!(span, "0".repeat(16));
        assert_ne!(trace, "0".repeat(32));
    }
}
