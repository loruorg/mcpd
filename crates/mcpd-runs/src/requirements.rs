//! What a cursor requires, derived once.
//!
//! [`cursor_requirements`] is an exhaustive match over [`Construct`] and
//! [`attempt_requirements`] is an exhaustive match over [`AttemptStatus`].
//! Neither has a catch-all arm, so a new construct or a new attempt state is a
//! compile error rather than a review finding. The materialiser applies these
//! rows when the cursor moves and the reconciler applies the same rows when it
//! finds a run that stopped moving. They are not two code paths that agree:
//! they are one function called from two places, and
//! `crates/mcpd-runs/src/sqlite.rs` calls it exactly once.

use crate::model::{
    AttemptStatus, Construct, ConstructKind, Cursor, RunId, RunStatus, Slot, StepKey, StepKind,
    TimerKind,
};
use time::OffsetDateTime;

/// One row a cursor position requires to exist.
///
/// Every variant is idempotent to apply: applying it to a run that already has
/// the row changes nothing and counts nothing. That property is what lets the
/// reconciler run the materialiser's own code against a healthy run.
#[derive(Debug, Clone, PartialEq)]
pub enum Requirement {
    /// A deadline. `step_key` is empty for a run-level one.
    Timer {
        /// Which step, or empty.
        step_key: StepKey,
        /// Which deadline.
        kind: TimerKind,
        /// When it fires.
        due_at: OffsetDateTime,
    },
    /// The row an inbound event finds a waiting run by.
    Wait {
        /// Which step is waiting.
        step_key: StepKey,
        /// The event name.
        event: String,
        /// The key that picks this run out.
        correlation_key: String,
        /// When waiting stops.
        expires_at: OffsetDateTime,
    },
    /// Consume an event that arrived before the run reached the wait, so the
    /// run never enters `waiting` at all.
    ParkedLookup {
        /// Which step is waiting.
        step_key: StepKey,
        /// The event name.
        event: String,
        /// The key that picks this run out.
        correlation_key: String,
    },
    /// Claimable work on disk. The fan-out is written before anything is
    /// offered, so a crash between the cursor move and the offer costs a
    /// repair rather than a run nothing selects.
    AttemptSlot {
        /// Everything the row needs.
        slot: Slot,
    },
    /// A child run, minted by the parent's transaction.
    ChildRun {
        /// The step the child hangs off.
        step_key: StepKey,
        /// The child's id.
        child_run_id: RunId,
        /// Which definition it runs.
        definition_id: String,
        /// Which version, pinned by the parent rather than resolved now.
        definition_version: u32,
        /// The parent step's span, which the child's root span nests under.
        span_id: String,
    },
    /// The attempt row a false guard leaves behind: attempt 0, `skipped`.
    Skipped {
        /// The node that was skipped.
        step_key: StepKey,
        /// What kind of step it would have been, so the journal says a wait
        /// was skipped and not a task.
        kind: StepKind,
    },
    /// The end of the graph: the terminal status, the output, and the deletion
    /// of this run's `waits` and `timers` rows.
    Terminal {
        /// How the run ended.
        status: RunStatus,
    },
}

/// Everything one position requires.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Requirements(pub Vec<Requirement>);

impl Requirements {
    /// The slots this position expects claimable rows for.
    ///
    /// `complete` reads this to decide whether a fan-out is finished, which is
    /// why a `parallel` advances only once every branch is terminal.
    pub fn attempt_slots(&self) -> impl Iterator<Item = &Slot> {
        self.0.iter().filter_map(|r| match r {
            Requirement::AttemptSlot { slot } => Some(slot),
            Requirement::Timer { .. }
            | Requirement::Wait { .. }
            | Requirement::ParkedLookup { .. }
            | Requirement::ChildRun { .. }
            | Requirement::Skipped { .. }
            | Requirement::Terminal { .. } => None,
        })
    }
}

fn slot_rows(slot: &Slot, rows: &mut Vec<Requirement>) {
    if let Some(due_at) = slot.step_deadline {
        if !slot.skipped {
            rows.push(Requirement::Timer {
                step_key: slot.step_key.clone(),
                kind: TimerKind::StepTimeout,
                due_at,
            });
        }
    }
    rows.push(Requirement::AttemptSlot { slot: slot.clone() });
}

/// What the transaction that advances a cursor onto `cursor` must write.
///
/// `run_deadline` is the run's own ceiling, which is required at every position
/// except the end of the graph. It is derived here rather than only at run
/// start so that the reconciler re-derives it too: a `run_timeout` row lost to
/// a crash is otherwise a run with no ceiling at all, and nothing else in the
/// daemon would notice.
#[must_use]
pub fn cursor_requirements(run_deadline: Option<OffsetDateTime>, cursor: &Cursor) -> Requirements {
    let mut rows = Vec::new();

    if cursor.at.kind() != ConstructKind::End {
        if let Some(due_at) = run_deadline {
            rows.push(Requirement::Timer {
                step_key: String::new(),
                kind: TimerKind::RunTimeout,
                due_at,
            });
        }
    }

    // A node whose guard was false requires one row: attempt 0, skipped. The
    // position then settles on arrival.
    if cursor.skipped {
        if let Some(step) = cursor.at.step() {
            // Exhaustive, so a construct added later has to say what kind of
            // row its skip leaves.
            let kind = match &cursor.at {
                Construct::Agent { .. } => StepKind::Agent,
                Construct::WaitEvent { .. } => StepKind::Wait,
                Construct::Task { .. }
                | Construct::Sleep { .. }
                | Construct::SleepUntil { .. }
                | Construct::Parallel { .. }
                | Construct::Foreach { .. }
                | Construct::Loop { .. }
                | Construct::Subworkflow { .. }
                | Construct::End { .. } => StepKind::Task,
            };
            rows.push(Requirement::Skipped { step_key: step.to_string(), kind });
        }
        return Requirements(rows);
    }

    // One arm per construct, no catch-all. Adding a construct to the graph
    // vocabulary stops this file compiling until somebody decides what it has
    // to write, and the reconciler inherits that decision for free.
    match &cursor.at {
        Construct::Task { slot } | Construct::Agent { slot } => slot_rows(slot, &mut rows),
        Construct::Sleep { step, due_at } | Construct::SleepUntil { step, due_at } => {
            rows.push(Requirement::Timer {
                step_key: step.clone(),
                kind: TimerKind::Sleep,
                due_at: *due_at,
            });
        }
        Construct::WaitEvent {
            step,
            event,
            correlation_key,
            expires_at,
            accept_parked,
            on_timeout: _,
        } => {
            rows.push(Requirement::Wait {
                step_key: step.clone(),
                event: event.clone(),
                correlation_key: correlation_key.clone(),
                expires_at: *expires_at,
            });
            // The deadline is a row here for the same reason a sleep's is. A
            // wait whose only record of expiry was a column on `waits` was
            // reachable by no tick in the daemon, for ever.
            rows.push(Requirement::Timer {
                step_key: step.clone(),
                kind: TimerKind::WaitTimeout,
                due_at: *expires_at,
            });
            if *accept_parked {
                rows.push(Requirement::ParkedLookup {
                    step_key: step.clone(),
                    event: event.clone(),
                    correlation_key: correlation_key.clone(),
                });
            }
        }
        Construct::Parallel { step: _, slots } | Construct::Foreach { step: _, slots, .. } => {
            for slot in slots {
                slot_rows(slot, &mut rows);
            }
        }
        Construct::Loop { step: _, slot, iteration: _, max_iterations: _ } => {
            slot_rows(slot, &mut rows);
        }
        Construct::Subworkflow {
            step,
            child_run_id,
            definition_id,
            definition_version,
            span_id,
        } => {
            rows.push(Requirement::ChildRun {
                step_key: step.clone(),
                child_run_id: child_run_id.clone(),
                definition_id: definition_id.clone(),
                definition_version: *definition_version,
                span_id: span_id.clone(),
            });
        }
        Construct::End { status } => {
            rows.push(Requirement::Terminal { status: *status });
        }
    }

    Requirements(rows)
}

/// Whether a position is over the moment its rows are written, as far as the
/// cursor alone can tell.
///
/// A skipped node, a fan-out with no branches and a `foreach` over no items
/// write no claimable slot, so no completion ever arrives, and they hold no
/// deadline and no wait either. Without this the run stops at that position
/// for ever and the reconciler reports it healthy, because there is nothing
/// missing to repair. The materialiser also settles a position whose every
/// slot is already terminal, which is what moves a run whose slots were all
/// skipped by their own guards.
///
/// Exhaustive with no catch-all, for the same reason the requirements match is:
/// a new construct has to answer this question rather than inherit "no".
#[must_use]
pub fn settles_on_arrival(cursor: &Cursor) -> bool {
    if cursor.skipped {
        return true;
    }
    match &cursor.at {
        Construct::Parallel { slots, .. } | Construct::Foreach { slots, .. } => slots.is_empty(),
        Construct::Task { .. }
        | Construct::Agent { .. }
        | Construct::Sleep { .. }
        | Construct::SleepUntil { .. }
        | Construct::WaitEvent { .. }
        | Construct::Loop { .. }
        | Construct::Subworkflow { .. }
        | Construct::End { .. } => false,
    }
}

/// What one attempt's state requires to exist.
///
/// The two deadlines a cursor never names live here, and they live in a
/// function rather than at their write sites for the same reason: the claim
/// path, the expiry branch and the reconciler all call this, so a state whose
/// deadline is missing is repairable by the same code that wrote it.
#[must_use]
pub fn attempt_requirements(
    step_key: &str,
    status: AttemptStatus,
    lease_until: Option<OffsetDateTime>,
    reclaim_until: Option<OffsetDateTime>,
) -> Requirements {
    let mut rows = Vec::new();
    match status {
        AttemptStatus::Pending => {}
        AttemptStatus::Leased | AttemptStatus::Started => {
            if let Some(due_at) = lease_until {
                rows.push(Requirement::Timer {
                    step_key: step_key.to_string(),
                    kind: TimerKind::LeaseExpiry,
                    due_at,
                });
            }
        }
        AttemptStatus::Orphaned => {
            if let Some(due_at) = reclaim_until {
                rows.push(Requirement::Timer {
                    step_key: step_key.to_string(),
                    kind: TimerKind::Reclaim,
                    due_at,
                });
            }
        }
        AttemptStatus::Succeeded
        | AttemptStatus::Failed
        | AttemptStatus::TimedOut
        | AttemptStatus::Skipped => {}
    }
    Requirements(rows)
}
