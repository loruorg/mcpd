//! Erasure and retention in the run store.
//!
//! The run store holds a run's input, its output, its state, every step's
//! input and output, and the span payloads still queued for the trace store,
//! all as they were sent. It was outside both the erasure endpoint and the
//! retention pass, so a person erased from the trace store was still there in
//! full, for ever. These tests hold the two deletions to the rule they share:
//! a whole run tree at a time, every table keyed by run, and never a run that
//! has not ended.

use mcpd_runs::model::{DefinitionKind, EventOutcome, InboundEvent, RunStatus};
use mcpd_runs::store::{DefinitionDocument, RunConfig, RunStore, StartRun, StepCompletion};
use mcpd_runs::SqliteRunStore;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use time::{Duration, OffsetDateTime};

fn t0() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("a valid instant")
}

/// Pid plus a per-process stamp, because a recycled pid alone reopens a
/// previous run's database.
fn temp_db() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir = std::env::temp_dir().join(format!(
        "mcpd-runs-compliance-{}-{stamp}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join("runs.db")
}

/// Every table that holds rows for a run, and the column that names it.
const TABLES: [(&str, &str); 7] = [
    ("runs", "run_id"),
    ("step_attempts", "run_id"),
    ("run_events", "run_id"),
    ("timers", "run_id"),
    ("waits", "run_id"),
    ("span_outbox", "run_id"),
    ("parked_events", "consumed_by_run_id"),
];

struct Harness {
    store: SqliteRunStore,
    path: PathBuf,
}

impl Harness {
    async fn open() -> Self {
        let path = temp_db();
        let store = SqliteRunStore::open(&path, RunConfig::default()).expect("open");
        store.migrate().await.expect("migrate");
        let h = Self { store, path };
        for project in ["checkout", "billing"] {
            h.define(project, "child_def", json!({"steps": [{"kind": "task", "id": "work"}]}))
                .await;
            h.define(
                project,
                "parent",
                json!({"steps": [
                    {"kind": "wait_event", "id": "approval", "event": "approved",
                     "key": "${input.order}", "timeout_ms": 60_000},
                    {"kind": "subworkflow", "id": "sub", "definition": "child_def"}
                ]}),
            )
            .await;
            h.define(project, "one", json!({"steps": [{"kind": "task", "id": "work"}]})).await;
        }
        h
    }

    async fn define(&self, project: &str, id: &str, body: Value) {
        self.store
            .put_definition(DefinitionDocument {
                project: project.into(),
                id: id.into(),
                kind: DefinitionKind::Workflow,
                version: Some(1),
                body,
            })
            .await
            .expect("define");
    }

    async fn start(
        &self,
        project: &str,
        id: &str,
        user: Option<&str>,
        input: Value,
        now: OffsetDateTime,
    ) -> String {
        self.store
            .start_run(
                StartRun {
                    project: project.into(),
                    definition_id: id.into(),
                    input: Some(input),
                    correlation: user.map(|u| json!({ "user_id": u, "session_id": "s-1" })),
                    ..Default::default()
                },
                now,
            )
            .await
            .expect("start")
            .run_id
    }

    /// Drive a `parent` run to its end: deliver its approval, then complete
    /// the child's one step. Answers the child's run id.
    async fn finish_parent(&self, project: &str, run_id: &str, order: &str) -> String {
        let delivery = self
            .store
            .deliver_event(
                InboundEvent {
                    project: project.into(),
                    event: "approved".into(),
                    correlation_key: order.into(),
                    event_id: format!("evt-{order}"),
                    id_is_derived: false,
                    payload: Some(json!({ "approver": "a person's name" })),
                },
                t0(),
            )
            .await
            .expect("deliver");
        assert_eq!(delivery.outcome, EventOutcome::Delivered);
        let leases = self.store.claim(project, "w-1", 1, None, t0()).await.expect("claim");
        assert_eq!(leases.len(), 1, "the child's step");
        let child = leases[0].run_id.clone();
        self.store
            .complete_step(
                &leases[0].lease_id,
                StepCompletion { output: Some(json!({ "said": "hello" })), ..Default::default() },
                t0(),
            )
            .await
            .expect("complete");
        let run = self.store.get_run(run_id).await.expect("parent");
        assert_eq!(run.status, RunStatus::Succeeded);
        child
    }

    fn raw(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(&self.path).expect("raw connection")
    }

    /// Rows per table for these runs.
    fn rows(&self, run_ids: &[&str]) -> Vec<(&'static str, i64)> {
        let conn = self.raw();
        let holes = vec!["?"; run_ids.len()].join(", ");
        TABLES
            .iter()
            .map(|(table, column)| {
                let n: i64 = conn
                    .query_row(
                        &format!("SELECT COUNT(*) FROM {table} WHERE {column} IN ({holes})"),
                        rusqlite::params_from_iter(run_ids.iter()),
                        |r| r.get(0),
                    )
                    .expect("count");
                (*table, n)
            })
            .collect()
    }

    fn total(&self, run_ids: &[&str]) -> i64 {
        self.rows(run_ids).iter().map(|(_, n)| n).sum()
    }

    /// The deadline and the suspension a crash can leave behind on an ended
    /// run, which the reconciler sweeps on its own timer and an erasure must
    /// not wait for.
    fn leave_stale_rows(&self, run_id: &str) {
        let conn = self.raw();
        conn.execute(
            "INSERT INTO timers (run_id, step_key, kind, due_at, created_at)
             VALUES (?1, 'approval', 'wait_timeout', 0, 0)",
            [run_id],
        )
        .expect("stale timer");
        conn.execute(
            "INSERT INTO waits (run_id, step_key, project, event, correlation_key, expires_at,
                                span_id, created_at)
             VALUES (?1, 'approval', 'checkout', 'approved', 'o-stale', 0, 'abc', 0)",
            [run_id],
        )
        .expect("stale wait");
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

// ----------------------------------------------------------------- erasure ---

#[tokio::test]
async fn erasing_a_person_deletes_their_ended_run_trees_from_every_table() {
    let h = Harness::open().await;
    let theirs = h.start("checkout", "parent", Some("u_8841"), json!({"order": "o1"}), t0()).await;
    let child = h.finish_parent("checkout", &theirs, "o1").await;
    h.leave_stale_rows(&theirs);
    let someone_else =
        h.start("checkout", "parent", Some("u_1234"), json!({"order": "o2"}), t0()).await;
    let someone_elses_child = h.finish_parent("checkout", &someone_else, "o2").await;
    let other_project =
        h.start("billing", "parent", Some("u_8841"), json!({"order": "o3"}), t0()).await;
    h.finish_parent("billing", &other_project, "o3").await;

    // Every table the erasure has to reach holds something for this tree.
    for (table, n) in h.rows(&[&theirs, &child]) {
        assert!(n > 0, "the test needs a row in {table} to prove it is erased");
    }
    let untouched = h.total(&[&someone_else, &someone_elses_child, &other_project]);

    let erased = h.store.erase_subject("checkout", "u_8841").await.expect("erase");
    assert_eq!(erased.runs_erased, 2, "the run and the child run it started");
    assert_eq!(erased.runs_in_flight, 0);
    assert!(erased.in_flight_roots.is_empty());

    for (table, n) in h.rows(&[&theirs, &child]) {
        assert_eq!(n, 0, "{table} still holds rows for an erased run");
    }
    assert_eq!(
        h.total(&[&someone_else, &someone_elses_child, &other_project]),
        untouched,
        "another person's runs, and this person's runs in another project, are not touched"
    );
    assert!(h.store.get_run(&theirs).await.is_err(), "the run is gone");

    // A second request finds nothing and says so with zeroes.
    let again = h.store.erase_subject("checkout", "u_8841").await.expect("erase again");
    assert_eq!((again.runs_erased, again.runs_in_flight), (0, 0));
}

#[tokio::test]
async fn a_run_still_going_is_reported_and_left_whole_until_it_ends() {
    let h = Harness::open().await;
    let live = h.start("checkout", "one", Some("u_8841"), json!({"order": "o1"}), t0()).await;
    let ended = h.start("checkout", "one", Some("u_8841"), json!({"order": "o2"}), t0()).await;
    h.store.cancel(&ended, t0()).await.expect("cancel");
    let before = h.total(&[&live]);

    let erased = h.store.erase_subject("checkout", "u_8841").await.expect("erase");
    assert_eq!(erased.runs_erased, 1, "the ended run goes");
    assert_eq!(erased.runs_in_flight, 1, "the live run is counted, not skipped in silence");
    assert_eq!(erased.in_flight_roots, vec![live.clone()], "and named, so it can be cancelled");
    assert_eq!(h.total(&[&live]), before, "a live run is not touched at all");
    assert_eq!(h.store.get_run(&live).await.expect("live").status, RunStatus::Running);

    h.store.cancel(&live, t0()).await.expect("cancel");
    let erased = h.store.erase_subject("checkout", "u_8841").await.expect("erase again");
    assert_eq!((erased.runs_erased, erased.runs_in_flight), (1, 0));
    assert_eq!(h.total(&[&live]), 0);
}

#[tokio::test]
async fn a_tree_with_a_child_run_still_going_is_left_whole() {
    let h = Harness::open().await;
    let parent = h.start("checkout", "parent", Some("u_8841"), json!({"order": "o1"}), t0()).await;
    h.store
        .deliver_event(
            InboundEvent {
                project: "checkout".into(),
                event: "approved".into(),
                correlation_key: "o1".into(),
                event_id: "evt-o1".into(),
                id_is_derived: false,
                payload: None,
            },
            t0(),
        )
        .await
        .expect("deliver");
    // The child is running and the parent waits on it: the whole tree is live.
    let erased = h.store.erase_subject("checkout", "u_8841").await.expect("erase");
    assert_eq!(erased.runs_erased, 0);
    assert_eq!(erased.runs_in_flight, 2, "the parent and its child, both attributed");
    assert_eq!(erased.in_flight_roots, vec![parent]);
}

/// An erasure commits one batch of trees at a time, as retention does, so
/// claims and heartbeats get the store's only lock between batches.
///
/// Seen from outside through a batch that fails: the batch before it stays
/// committed, and sending the erasure again finishes the rest. One
/// transaction for the whole person would have rolled every tree back.
#[tokio::test]
async fn an_erasure_commits_a_batch_of_trees_at_a_time() {
    let h = Harness::open().await;
    // One more batch than retention's 200 trees, and a few over.
    let mut ids = Vec::new();
    for i in 0..205 {
        let id = h.start("checkout", "one", Some("u_8841"), json!({"order": i}), t0()).await;
        h.store.cancel(&id, t0()).await.expect("cancel");
        ids.push(id);
    }
    ids.sort();
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let (first, second) = ids.split_at(200);
    h.raw()
        .execute_batch(&format!(
            "CREATE TRIGGER stop_the_second_batch BEFORE DELETE ON runs
             WHEN OLD.run_id = '{}' BEGIN SELECT RAISE(ABORT, 'stopped by the test'); END",
            second[second.len() - 1]
        ))
        .expect("trigger");
    let second_rows = h.total(second);

    h.store.erase_subject("checkout", "u_8841").await.expect_err("the second batch fails");
    assert_eq!(h.total(first), 0, "the first batch committed before the second one failed");
    assert_eq!(h.total(second), second_rows, "the batch that failed rolled back as a whole");

    h.raw().execute_batch("DROP TRIGGER stop_the_second_batch").expect("drop trigger");
    let erased = h.store.erase_subject("checkout", "u_8841").await.expect("erase again");
    assert_eq!((erased.runs_erased, erased.runs_in_flight), (5, 0), "only what was left");
}

/// Retention and erasure find the events a tree consumed by the run that
/// consumed them, while they hold the store's only lock. Without an index on
/// that column each tree read the whole table, and the readiness probe gave
/// up behind a retention pass.
#[tokio::test]
async fn a_tree_finds_its_parked_events_by_index_in_a_new_or_an_existing_file() {
    let h = Harness::open().await;
    // A fresh connection each time: one that is kept plans from the schema it
    // read first, and would not see the index come back.
    let plan = || -> String {
        let conn = h.raw();
        let mut stmt = conn
            .prepare(
                "EXPLAIN QUERY PLAN DELETE FROM parked_events WHERE consumed_by_run_id IN \
                 (SELECT run_id FROM runs WHERE root_run_id = ?1)",
            )
            .expect("plan");
        let rows = stmt
            .query_map(["r"], |r| r.get::<_, String>(3))
            .expect("plan rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("plan text");
        rows.join("\n")
    };
    let fresh = plan();
    assert!(!fresh.contains("SCAN parked_events"), "a new file reads the whole table:\n{fresh}");

    // A file written before the index existed gains it on its next start.
    h.raw().execute_batch("DROP INDEX parked_consumer").expect("drop the index");
    assert!(plan().contains("SCAN parked_events"), "the plan must show the difference");
    let restarted = SqliteRunStore::open(&h.path, RunConfig::default()).expect("reopen");
    restarted.migrate().await.expect("migrate");
    let upgraded = plan();
    assert!(
        !upgraded.contains("SCAN parked_events"),
        "an existing file reads the whole table after a restart:\n{upgraded}"
    );
}

// --------------------------------------------------------------- retention ---

#[tokio::test]
async fn retention_deletes_ended_run_trees_older_than_the_cutoff_and_nothing_else() {
    let h = Harness::open().await;
    let old = h.start("checkout", "parent", None, json!({"order": "o1"}), t0()).await;
    let old_child = h.finish_parent("checkout", &old, "o1").await;
    let recent = h.start("checkout", "one", None, json!({}), t0()).await;
    h.store.cancel(&recent, t0() + Duration::days(10)).await.expect("cancel");
    let still_going = h.start("checkout", "one", None, json!({}), t0()).await;
    let elsewhere = h.start("billing", "one", None, json!({}), t0()).await;
    h.store.cancel(&elsewhere, t0()).await.expect("cancel");
    let kept = h.total(&[&recent, &still_going, &elsewhere]);
    assert!(h.total(&[&old, &old_child]) > 0);

    let report = h.store.apply_retention("checkout", t0() + Duration::days(5)).await.expect("pass");
    assert_eq!(report.runs_deleted, 2, "the old run and its child");
    assert!(report.rows_deleted > report.runs_deleted, "with what hangs off them");
    for (table, n) in h.rows(&[&old, &old_child]) {
        assert_eq!(n, 0, "{table} still holds rows for an expired run");
    }
    assert_eq!(
        h.total(&[&recent, &still_going, &elsewhere]),
        kept,
        "a run that ended after the cutoff, a run that has not ended however old it is, and \
         another project's run are all kept"
    );
    assert_eq!(h.store.get_run(&still_going).await.expect("live").status, RunStatus::Running);
}

#[tokio::test]
async fn one_retention_pass_reaches_past_its_first_batch() {
    let h = Harness::open().await;
    let mut ids = Vec::new();
    for _ in 0..205 {
        let id = h.start("checkout", "one", None, json!({}), t0()).await;
        h.store.cancel(&id, t0()).await.expect("cancel");
        ids.push(id);
    }
    let report = h.store.apply_retention("checkout", t0() + Duration::days(1)).await.expect("pass");
    assert_eq!(report.runs_deleted, 205, "a pass keeps going until nothing expired is left");
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    assert_eq!(h.total(&refs), 0);
}
