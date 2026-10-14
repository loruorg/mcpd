//! Background jobs on several replicas sharing one store.
//!
//! Each replica here is a full `AppState` over the same SQLite file, which is
//! how two daemons share a store. The store's own lease semantics are proved
//! on all three backends in `mcpd-store/tests/conformance.rs`; this checks the
//! loop that uses them: one replica runs the job, and another takes it over
//! when the first stops.

mod support;

use mcpd_server::lease::{spawn_leased, Duty};
use mcpd_server::{AppState, FirstPass, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::Store;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::temp_dir;

async fn replicas(n: usize) -> Vec<Arc<AppState>> {
    let path = temp_dir("leases").join("shared.db");
    let mut out = Vec::new();
    for _ in 0..n {
        let store = SqliteStore::open(&path, 2).expect("open the shared store");
        store.migrate().await.expect("migrate");
        out.push(Arc::new(AppState::new(
            Arc::new(store) as Arc<dyn Store>,
            ServerConfig::default(),
        )));
    }
    out
}

#[tokio::test]
async fn every_replica_has_its_own_holder_name() {
    let r = replicas(2).await;
    assert_ne!(r[0].replica, r[1].replica);
}

#[tokio::test]
async fn one_replica_holds_a_job_and_another_takes_it_when_released() {
    let r = replicas(2).await;
    let (a, b) =
        (Duty::new("job", Duration::from_secs(30)), Duty::new("job", Duration::from_secs(30)));
    let first = a.claim(&r[0], None).await.expect("the first claim on a free lease is granted");
    assert!(first.fresh, "a lease this replica did not hold before starts a new term");
    assert!(b.claim(&r[1], None).await.is_none(), "a held lease is not granted to another");
    let renewed = a.claim(&r[0], Some("42")).await.expect("the holder renews");
    assert!(!renewed.fresh, "a renewal continues the term");

    a.release(&r[0]).await;
    let taken = b.claim(&r[1], None).await.expect("a released lease goes to the next claimant");
    assert!(taken.fresh);
    assert_eq!(taken.lease.progress.as_deref(), Some("42"), "progress passes to the new holder");
    assert!(a.claim(&r[0], None).await.is_none(), "the old holder does not take it back");
}

/// A holder that stops renewing without giving the lease up, as a crashed one
/// does, loses it when it expires.
#[tokio::test]
async fn a_holder_that_stops_renewing_loses_the_job_when_the_lease_expires() {
    let r = replicas(2).await;
    let ttl = Duration::from_millis(150);
    let (a, b) = (Duty::new("crash", ttl), Duty::new("crash", ttl));
    assert!(a.claim(&r[0], None).await.is_some());
    assert!(b.claim(&r[1], None).await.is_none(), "not before it expires");
    tokio::time::sleep(ttl + Duration::from_millis(50)).await;
    assert!(b.claim(&r[1], None).await.is_some(), "an expired lease goes to the next claimant");
}

/// Three replicas run the same loop; passes are counted per replica. Only one
/// replica's count moves, and when it shuts down another's starts to.
#[tokio::test]
async fn the_loop_runs_on_one_replica_and_fails_over_on_shutdown() {
    let r = replicas(3).await;
    let counts: Vec<Arc<AtomicU32>> = (0..3).map(|_| Arc::new(AtomicU32::new(0))).collect();
    for (state, count) in r.iter().zip(&counts) {
        let count = Arc::clone(count);
        let duty = Arc::new(Duty::new("counted", Duration::from_secs(30)));
        spawn_leased(state, duty, Duration::from_millis(20), FirstPass::AtStart, move |g| {
            let count = Arc::clone(&count);
            async move {
                if g.is_some() {
                    count.fetch_add(1, Ordering::Relaxed);
                }
            }
        });
    }
    let ran =
        |c: &[Arc<AtomicU32>]| c.iter().map(|c| c.load(Ordering::Relaxed)).collect::<Vec<_>>();
    let deadline = Instant::now() + Duration::from_secs(10);
    while ran(&counts).iter().sum::<u32>() < 10 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let before = ran(&counts);
    let running: Vec<usize> = (0..3).filter(|&i| before[i] > 0).collect();
    assert_eq!(running.len(), 1, "exactly one replica ran the job, got {before:?}");
    let holder = running[0];

    r[holder].shutdown.cancel();
    r[holder].tasks.close();
    r[holder].tasks.wait().await;
    let stopped_at = ran(&counts)[holder];

    // The holder gave the lease up as it stopped, so another replica takes it
    // at its next pass rather than after thirty seconds.
    let deadline = Instant::now() + Duration::from_secs(5);
    let others = |c: &[u32]| (0..3).filter(|&i| i != holder && c[i] > 0).count();
    while others(&ran(&counts)) == 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let after = ran(&counts);
    assert_eq!(after[holder], stopped_at, "a stopped replica runs nothing more");
    assert_eq!(others(&after), 1, "exactly one other replica took the job over, got {after:?}");
    for s in &r {
        s.shutdown.cancel();
    }
}
