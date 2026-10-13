//! Who a commit wakes.
//!
//! Every write used to wake every waiter. A claim is a write, so an idle
//! worker's empty claim woke its own long-poll, which claimed again at once:
//! one worker with nothing to do ran claim transactions back to back for the
//! whole of its wait, and woke the outbox drain on every one. These tests pin
//! down which writes wake the claim long-poll and which wake the readers.

mod common;

use common::TestStore;
use mcpd_runs::model::DefinitionKind;
use mcpd_runs::store::{DefinitionDocument, RunConfig, StartRun};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use time::OffsetDateTime;
use tokio::sync::Notify;

fn t0() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("a valid instant")
}

async fn store() -> TestStore {
    let store = TestStore::open(RunConfig::default()).await;
    store
        .put_definition(DefinitionDocument {
            project: "checkout".into(),
            id: "one".into(),
            kind: DefinitionKind::Workflow,
            version: Some(1),
            body: json!({"steps": [{"kind": "task", "id": "work"}]}),
        })
        .await
        .expect("define");
    store
}

async fn start(store: &TestStore) -> String {
    store
        .start_run(
            StartRun {
                project: "checkout".into(),
                definition_id: "one".into(),
                ..Default::default()
            },
            t0(),
        )
        .await
        .expect("start")
        .run_id
}

/// Whether `notify` was signalled while `act` ran. The waiter is registered
/// before the act, the way every long-poll in the server registers it.
async fn woke<F: std::future::Future<Output = ()>>(notify: &Arc<Notify>, act: F) -> bool {
    let notified = notify.notified();
    tokio::pin!(notified);
    notified.as_mut().enable();
    act.await;
    tokio::time::timeout(Duration::from_millis(50), notified).await.is_ok()
}

backend_tests! {
async fn a_claim_that_leases_nothing_wakes_nobody() {
    let store = store().await;
    let (runnable, changed) = (store.runnable(), store.notify());

    let idle = || async {
        let leases = store.claim("checkout", "w-1", 1, None, t0()).await.expect("claim");
        assert!(leases.is_empty(), "nothing has started, so nothing is claimable");
    };
    assert!(!woke(&runnable, idle()).await, "an empty claim woke the claim long-poll");
    assert!(!woke(&changed, idle()).await, "an empty claim woke the readers and the drain");
}
}

backend_tests! {
async fn starting_a_run_wakes_a_waiting_worker_and_taking_its_step_does_not() {
    let store = store().await;
    let (runnable, changed) = (store.runnable(), store.notify());

    assert!(
        woke(&runnable, async {
            start(&store).await;
        })
        .await,
        "work became claimable"
    );

    // Taking the step changes the run, which a reader watches, and makes
    // nothing new claimable, which is all a worker waits for.
    let lease = std::sync::Mutex::new(String::new());
    let take = || async {
        let leases = store.claim("checkout", "w-1", 1, None, t0()).await.expect("claim");
        *lease.lock().expect("lease") = leases[0].lease_id.clone();
    };
    let notified = changed.notified();
    tokio::pin!(notified);
    notified.as_mut().enable();
    assert!(!woke(&runnable, take()).await, "a lease taken woke the claim long-poll");
    assert!(
        tokio::time::timeout(Duration::from_millis(50), notified).await.is_ok(),
        "a lease taken writes an event, and the event stream waits for it"
    );

    // A heartbeat frees nothing and writes nothing anybody reads.
    let lease_id = lease.lock().expect("lease").clone();
    let beat = || async {
        store.heartbeat(&lease_id, t0()).await.expect("heartbeat");
    };
    assert!(!woke(&runnable, beat()).await, "a heartbeat woke the claim long-poll");
    assert!(!woke(&changed, beat()).await, "a heartbeat woke the readers");
}
}
