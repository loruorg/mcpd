//! The daemon's pass over the run store.
//!
//! Two ticks, in this order. Deadlines first, because a run whose deadline has
//! passed should not be repaired into a state the reconciler then counts. The
//! reconciler second, because it is the net under everything else.

use crate::store::{RunStore, TickReport};
use crate::Result;
use time::OffsetDateTime;

/// Handle everything due, then repair anything that stopped moving.
///
/// A non-zero `reconcile.runs_repaired` is a defect report rather than a
/// success. Something wrote a cursor without writing what the cursor requires,
/// and the daemon should say so rather than quietly fixing the same run shape
/// every day.
///
/// # Errors
/// Fails when the store does.
pub async fn tick(store: &dyn RunStore, now: OffsetDateTime) -> Result<TickReport> {
    let timers = store.run_due_timers(now).await?;
    let reconcile = store.reconcile(now).await?;
    if reconcile.runs_repaired > 0 {
        tracing::warn!(
            runs = reconcile.runs_repaired,
            rows = reconcile.rows_repaired,
            "stalled runs repaired: a cursor moved without writing what it requires"
        );
    }
    if reconcile.stale_rows_deleted > 0 {
        tracing::warn!(
            rows = reconcile.stale_rows_deleted,
            "ended runs left waits or timers behind"
        );
    }
    if timers.deferred > 0 {
        tracing::warn!(
            timers = timers.deferred,
            "due timers could not be handled and were pushed forward"
        );
    }
    Ok(TickReport { timers, reconcile })
}
