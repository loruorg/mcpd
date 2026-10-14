//! One replica per background job.
//!
//! Several daemons can share one store. Every one of them runs the same
//! loops, and some of those loops must do their work once, not once per
//! replica: live scoring pays a judge per call, tail sampling deletes runs,
//! and a monitor's webhook should reach its receiver once. Each such loop
//! claims a lease in the store before a pass and runs the pass only while it
//! holds it. The holder renews the lease while it works; when it stops, by
//! crashing or by shutting down, another replica takes the lease at its next
//! pass. A daemon alone with its store holds every lease itself.

use crate::state::AppState;
use mcpd_store::{Lease, LeaseClaim};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use time::OffsetDateTime;

/// A name for this process as a lease holder.
#[must_use]
pub fn replica_name() -> String {
    let host = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .ok()
        .filter(|h| !h.trim().is_empty())
        .unwrap_or_else(|| "mcpd".to_owned());
    format!("{}-{}-{:08x}", host.trim(), std::process::id(), rand::random::<u32>())
}

/// How long a lease lasts: three passes, so one slow pass or one missed
/// renewal does not hand the job to another replica, and a holder that has
/// stopped is replaced within three intervals.
#[must_use]
pub fn ttl_for(every: Duration) -> Duration {
    every.saturating_mul(3)
}

/// A lease the current pass holds.
#[derive(Debug, Clone)]
pub struct Granted {
    /// The lease as the store returned it.
    pub lease: Lease,
    /// Whether this replica did not hold it at its previous claim, so the
    /// pass is the first of a new term and should continue from
    /// [`Lease::progress`] rather than from its own memory.
    pub fresh: bool,
}

/// One background job's lease, from this replica's side.
#[derive(Debug)]
pub struct Duty {
    job: &'static str,
    ttl: Duration,
    held: AtomicBool,
}

impl Duty {
    /// A job named `job`, whose lease lasts `ttl`.
    #[must_use]
    pub const fn new(job: &'static str, ttl: Duration) -> Self {
        Self { job, ttl, held: AtomicBool::new(false) }
    }

    /// The job's name.
    #[must_use]
    pub const fn job(&self) -> &'static str {
        self.job
    }

    /// Take or renew the lease, recording `progress` when given. `None` when
    /// another replica holds it, or when the store cannot answer: running a
    /// job twice is the outcome this exists to prevent, so a doubt is a no.
    pub async fn claim(&self, state: &AppState, progress: Option<&str>) -> Option<Granted> {
        let now = OffsetDateTime::now_utc();
        let claim = LeaseClaim {
            job: self.job,
            holder: &state.replica,
            now,
            ttl: time::Duration::try_from(self.ttl).unwrap_or(time::Duration::MAX),
            progress,
        };
        let lease = match state.store.claim_lease(&claim).await {
            Ok(l) => l,
            Err(e) => {
                if self.held.swap(false, Ordering::Relaxed) {
                    tracing::warn!(job = self.job, error = %e,
                        "could not renew this replica's lease; the job pauses here until it can");
                }
                return None;
            }
        };
        let mine = lease.held_by(&state.replica, now);
        let before = self.held.swap(mine, Ordering::Relaxed);
        match (before, mine) {
            (false, true) => tracing::info!(job = self.job, replica = %state.replica,
                "this replica now runs the job"),
            (true, false) => tracing::info!(job = self.job, holder = %lease.holder,
                "another replica runs the job now"),
            _ => {}
        }
        mine.then_some(Granted { lease, fresh: !before })
    }

    /// Give the lease up, so another replica takes it at its next pass rather
    /// than after it expires.
    pub async fn release(&self, state: &AppState) {
        if !self.held.swap(false, Ordering::Relaxed) {
            return;
        }
        if let Err(e) =
            state.store.release_lease(self.job, &state.replica, OffsetDateTime::now_utc()).await
        {
            tracing::warn!(job = self.job, error = %e,
                "could not give up the lease; another replica takes it when it expires");
        }
    }
}

/// [`crate::spawn_loop`], with the job's lease claimed before every pass.
///
/// The pass is given the lease when this replica holds it and `None` when
/// another does, so a job with work every replica must do, such as keeping
/// its own copy of a monitor's state, can still do that part. While a pass
/// runs the lease is renewed every third of its length, so a pass longer than
/// the lease does not hand the job to a second replica halfway through. On
/// shutdown the lease is given up.
pub fn spawn_leased<P, F>(
    state: &Arc<AppState>,
    duty: Arc<Duty>,
    every: Duration,
    first: crate::FirstPass,
    mut pass: P,
) where
    P: FnMut(Option<Granted>) -> F + Send + 'static,
    F: Future<Output = ()> + Send + 'static,
{
    let stop = state.shutdown.clone();
    let state = Arc::clone(state);
    let looped = Arc::clone(&state);
    state.tasks.spawn(async move {
        let state = looped;
        let mut timer = tokio::time::interval(every);
        if first == crate::FirstPass::AfterInterval {
            timer.tick().await;
        }
        loop {
            tokio::select! {
                () = stop.cancelled() => break,
                _ = timer.tick() => {}
            }
            let granted = duty.claim(&state, None).await;
            let holding = granted.is_some();
            let run = crate::run_tick(duty.job(), async { pass(granted).await });
            tokio::pin!(run);
            let beat = (duty.ttl / 3).max(Duration::from_millis(10));
            loop {
                tokio::select! {
                    _ = &mut run => break,
                    () = tokio::time::sleep(beat), if holding => {
                        if duty.claim(&state, None).await.is_none() {
                            tracing::warn!(job = duty.job(),
                                "the lease was lost during a pass; the pass finishes and the \
                                 next one waits for the lease");
                        }
                    }
                }
            }
        }
        duty.release(&state).await;
    });
}
