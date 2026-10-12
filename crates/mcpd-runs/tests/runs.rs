//! What the run core has to be true for.
//!
//! Two of these tests are the whole reason the crate is shaped the way it is.
//! `advancing_onto_every_construct_writes_the_rows_it_requires` walks the full
//! construct enum rather than one example, and `every_timer_kind_has_a_writer`
//! walks the full deadline enum and drives a real path per kind. Both hold an
//! exhaustive match with no catch-all, so a construct or a deadline added
//! without a writer is a compile error in this file rather than a defect
//! somebody has to read four documents to find.

use mcpd_runs::model::{
    Attempt, AttemptStatus, ConstructKind, DefinitionKind, Effect, EventOutcome, InboundEvent,
    RunEventKind, RunStatus, StepErrorClass, StepKind, TimerKind,
};
use mcpd_runs::store::{
    DefinitionDocument, RunConfig, RunStore, StartRun, StepCompletion, StepError,
};
use mcpd_runs::{tick, SqliteRunStore};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use time::{Duration, OffsetDateTime};

/// A fixed clock. Every test passes `now` in rather than reading one, so an
/// expiry test costs nothing and never flakes.
fn t0() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("a valid instant")
}

/// A pid alone is not unique: pids are recycled, and a recycled one reopens
/// the previous run's database, so every count is off by a whole prior suite.
static STAMP: AtomicU64 = AtomicU64::new(0);

fn temp_db() -> PathBuf {
    let n = STAMP.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("mcpd-runs-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join("runs.db")
}

/// A completion carrying an output and nothing else.
fn done(output: Value) -> StepCompletion {
    StepCompletion { output: Some(output), ..Default::default() }
}

/// A failure a retry cannot fix.
fn fatal(message: &str) -> StepError {
    StepError {
        class: StepErrorClass::Fatal,
        message: message.into(),
        details: None,
        retry_after_ms: None,
    }
}

struct Harness {
    store: SqliteRunStore,
    path: PathBuf,
}

impl Harness {
    async fn open() -> Self {
        Self::with_config(RunConfig::default()).await
    }

    async fn with_config(config: RunConfig) -> Self {
        let path = temp_db();
        let store = SqliteRunStore::open(&path, config).expect("open");
        store.migrate().await.expect("migrate");
        Self { store, path }
    }

    /// A second connection to the same file, which is how the crash tests
    /// delete the rows a crash would lose.
    fn raw(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(&self.path).expect("raw connection")
    }

    async fn define(&self, id: &str, body: Value) {
        self.store
            .put_definition(DefinitionDocument {
                project: "checkout".into(),
                id: id.into(),
                kind: DefinitionKind::Workflow,
                version: Some(1),
                body,
            })
            .await
            .expect("put definition");
    }

    async fn start(&self, id: &str, now: OffsetDateTime) -> String {
        self.start_with(id, None, now).await
    }

    async fn start_with(&self, id: &str, input: Option<Value>, now: OffsetDateTime) -> String {
        self.store
            .start_run(
                StartRun {
                    project: "checkout".into(),
                    definition_id: id.into(),
                    definition_version: Some(1),
                    input,
                    correlation: None,
                    idempotency_key: None,
                },
                now,
            )
            .await
            .expect("start run")
            .run_id
    }

    async fn timer_set(&self, run_id: &str) -> HashSet<(String, TimerKind)> {
        self.store
            .timers(run_id)
            .await
            .expect("timers")
            .into_iter()
            .map(|t| (t.step_key, t.kind))
            .collect()
    }

    async fn wait_set(&self, run_id: &str) -> HashSet<String> {
        self.store.waits(run_id).await.expect("waits").into_iter().map(|w| w.step_key).collect()
    }

    async fn pending_set(&self, run_id: &str) -> HashSet<String> {
        self.store
            .attempts(run_id)
            .await
            .expect("attempts")
            .into_iter()
            .filter(|a| a.status == AttemptStatus::Pending)
            .map(|a| a.step_key)
            .collect()
    }

    async fn attempt(&self, run_id: &str, step_key: &str, attempt: i64) -> Attempt {
        self.store
            .attempts(run_id)
            .await
            .expect("attempts")
            .into_iter()
            .find(|a| a.step_key == step_key && a.attempt == attempt)
            .unwrap_or_else(|| panic!("no attempt {attempt} of step {step_key}"))
    }

    fn child_count(&self, run_id: &str) -> i64 {
        self.raw()
            .query_row("SELECT COUNT(*) FROM runs WHERE parent_run_id = ?1", [run_id], |r| r.get(0))
            .expect("child count")
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

// ---------------------------------------------------------------------------
// The construct table
// ---------------------------------------------------------------------------

/// What one construct is, and what advancing onto it has to write.
struct Case {
    /// The definition body a run of this case starts against.
    body: Value,
    /// The input the run starts with.
    input: Option<Value>,
    /// The `(step_key, kind)` deadlines the transaction must leave behind.
    timers: Vec<(&'static str, TimerKind)>,
    /// The suspensions it must leave behind.
    waits: Vec<&'static str>,
    /// The claimable slots it must leave behind.
    slots: Vec<&'static str>,
    /// How many child runs it must have started.
    children: i64,
    /// Whether the run is over by the time the transaction returns.
    terminal: bool,
}

/// One case per construct, decided by an exhaustive match.
///
/// This match is the reason a tenth construct cannot ship untested: adding a
/// variant to `ConstructKind` stops this file compiling until somebody writes
/// down what the new construct requires, and the assertion below then holds it
/// to that.
fn case_for(kind: ConstructKind) -> Case {
    match kind {
        ConstructKind::Task => Case {
            body: json!({"steps": [
                {"kind": "task", "id": "work", "step_timeout_ms": 86_400_000}
            ]}),
            input: None,
            timers: vec![("work", TimerKind::StepTimeout)],
            waits: vec![],
            slots: vec!["work"],
            children: 0,
            terminal: false,
        },
        ConstructKind::Agent => Case {
            body: json!({"steps": [
                {"kind": "agent", "id": "brain", "step_timeout_ms": 86_400_000}
            ]}),
            input: None,
            timers: vec![("brain", TimerKind::StepTimeout)],
            waits: vec![],
            slots: vec!["brain"],
            children: 0,
            terminal: false,
        },
        ConstructKind::Sleep => Case {
            body: json!({"steps": [{"kind": "sleep", "id": "nap", "ms": 86_400_000}]}),
            input: None,
            timers: vec![("nap", TimerKind::Sleep)],
            waits: vec![],
            slots: vec![],
            children: 0,
            terminal: false,
        },
        ConstructKind::SleepUntil => Case {
            body: json!({"steps": [
                {"kind": "sleep_until", "id": "wake", "at": "2033-11-14T22:13:20Z"}
            ]}),
            input: None,
            timers: vec![("wake", TimerKind::Sleep)],
            waits: vec![],
            slots: vec![],
            children: 0,
            terminal: false,
        },
        ConstructKind::WaitEvent => Case {
            body: json!({"steps": [{
                "kind": "wait_event", "id": "approval", "event": "approved",
                "key": "order-7", "timeout_ms": 86_400_000
            }]}),
            // The deadline is a row here for the same reason a sleep's is. A
            // wait whose only record of expiry was a column on `waits` was
            // reachable by no tick in the daemon, for ever.
            input: None,
            timers: vec![("approval", TimerKind::WaitTimeout)],
            waits: vec!["approval"],
            slots: vec![],
            children: 0,
            terminal: false,
        },
        ConstructKind::Parallel => Case {
            body: json!({"steps": [
                {"kind": "parallel", "id": "fan", "branches": [{"kind": "task", "id": "a"}, {"kind": "task", "id": "b"}]}
            ]}),
            input: None,
            timers: vec![],
            waits: vec![],
            slots: vec!["fan/a", "fan/b"],
            children: 0,
            terminal: false,
        },
        ConstructKind::Foreach => Case {
            body: json!({"steps": [{"kind": "foreach", "id": "each", "over": "input.items", "body": {"kind": "task", "id": "item"}, "max_items": 10}]}),
            input: Some(json!({"items": [1, 2, 3]})),
            timers: vec![],
            waits: vec![],
            slots: vec!["each[0]", "each[1]", "each[2]"],
            children: 0,
            terminal: false,
        },
        ConstructKind::Loop => Case {
            body: json!({"steps": [
                {"kind": "loop", "id": "poll", "body": {"kind": "task", "id": "check"}, "max_iterations": 3}
            ]}),
            input: None,
            timers: vec![],
            waits: vec![],
            slots: vec!["poll[0]/check"],
            children: 0,
            terminal: false,
        },
        ConstructKind::Subworkflow => Case {
            body: json!({"steps": [
                {"kind": "subworkflow", "id": "refund", "definition": "child", "version": 1}
            ]}),
            input: None,
            timers: vec![],
            waits: vec![],
            slots: vec![],
            children: 1,
            terminal: false,
        },
        ConstructKind::End => Case {
            body: json!({"steps": []}),
            input: None,
            timers: vec![],
            waits: vec![],
            slots: vec![],
            children: 0,
            terminal: true,
        },
    }
}

async fn harness_with_cases() -> Harness {
    let h = Harness::open().await;
    h.define("child", json!({"steps": [{"kind": "task", "id": "inner"}]})).await;
    for kind in ConstructKind::ALL {
        h.define(&format!("case_{}", kind.as_str()), case_for(kind).body).await;
    }
    h
}

#[tokio::test]
async fn advancing_onto_every_construct_writes_the_rows_it_requires() {
    let h = harness_with_cases().await;
    for kind in ConstructKind::ALL {
        let case = case_for(kind);
        let run_id =
            h.start_with(&format!("case_{}", kind.as_str()), case.input.clone(), t0()).await;
        let label = format!("{kind:?}");

        let want_timers: HashSet<(String, TimerKind)> =
            case.timers.iter().map(|(s, k)| ((*s).to_string(), *k)).collect();
        assert_eq!(h.timer_set(&run_id).await, want_timers, "timers for {label}");

        let want_waits: HashSet<String> = case.waits.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(h.wait_set(&run_id).await, want_waits, "waits for {label}");

        let want_slots: HashSet<String> = case.slots.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(h.pending_set(&run_id).await, want_slots, "slots for {label}");

        assert_eq!(h.child_count(&run_id), case.children, "children for {label}");

        let run = h.store.get_run(&run_id).await.expect("run");
        assert_eq!(run.status.is_terminal(), case.terminal, "terminal for {label}");
    }
}

#[tokio::test]
async fn a_crash_between_the_cursor_and_its_rows_is_repaired_for_every_construct() {
    // The failure, exactly: `complete` advanced the cursor and did not create
    // the next construct's row. The run then has no lease to expire and no
    // timer to fire, and nothing in the daemon touches it again.
    let h = harness_with_cases().await;
    for kind in ConstructKind::ALL {
        let case = case_for(kind);
        if case.terminal {
            continue;
        }
        let run_id =
            h.start_with(&format!("case_{}", kind.as_str()), case.input.clone(), t0()).await;

        {
            let raw = h.raw();
            for table in ["timers", "waits", "step_attempts"] {
                raw.execute(&format!("DELETE FROM {table} WHERE run_id = ?1"), [&run_id])
                    .expect("lose the rows a crash would lose");
            }
            // A subworkflow child is a whole run row, and losing it is the same
            // failure one table over. Its stream goes with it, because the two
            // were written by one transaction and a crash cannot split them.
            raw.execute(
                "DELETE FROM run_events WHERE run_id IN
                    (SELECT run_id FROM runs WHERE parent_run_id = ?1)",
                [&run_id],
            )
            .expect("lose the child's stream");
            raw.execute("DELETE FROM runs WHERE parent_run_id = ?1", [&run_id])
                .expect("lose the child");
        }

        let label = format!("{kind:?}");
        assert!(
            h.timer_set(&run_id).await.is_empty()
                && h.wait_set(&run_id).await.is_empty()
                && h.pending_set(&run_id).await.is_empty(),
            "the crash left rows behind for {label}"
        );

        let later = t0() + Duration::minutes(10);
        let report = h.store.reconcile(later).await.expect("reconcile");
        assert!(report.runs_repaired >= 1, "the reconciler repaired nothing for {label}");

        let want_timers: HashSet<(String, TimerKind)> =
            case.timers.iter().map(|(s, k)| ((*s).to_string(), *k)).collect();
        assert_eq!(h.timer_set(&run_id).await, want_timers, "timers re-derived for {label}");
        let want_waits: HashSet<String> = case.waits.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(h.wait_set(&run_id).await, want_waits, "waits re-derived for {label}");
        let want_slots: HashSet<String> = case.slots.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(h.pending_set(&run_id).await, want_slots, "slots re-derived for {label}");
        assert_eq!(h.child_count(&run_id), case.children, "children re-derived for {label}");
    }
}

#[tokio::test]
async fn a_run_at_a_claimable_task_with_no_lease_no_wait_and_no_timer_is_selected_again() {
    // The case round five named by hand: a run whose cursor names a claimable
    // task, holding no lease, no wait row and no timer row, which no tick would
    // ever select again.
    let h = Harness::open().await;
    h.define("one", json!({"steps": [{"kind": "task", "id": "work"}]})).await;
    let run_id = h.start("one", t0()).await;

    h.raw()
        .execute("DELETE FROM step_attempts WHERE run_id = ?1", [&run_id])
        .expect("lose the slot");

    let nothing = h.store.claim("checkout", "worker-1", 10, None, t0()).await.expect("claim");
    assert!(nothing.is_empty(), "the run should be unreachable before the reconciler runs");

    let later = t0() + Duration::minutes(10);
    let report = tick(&h.store, later).await.expect("tick");
    assert_eq!(report.reconcile.runs_repaired, 1);

    let leases = h.store.claim("checkout", "worker-1", 10, None, later).await.expect("claim");
    assert_eq!(leases.len(), 1, "a tick should reach the run again");
    assert_eq!(leases[0].step_key, "work");
}

// ---------------------------------------------------------------------------
// The deadline table
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_timer_kind_has_a_writer() {
    // Five rounds of review kept finding a `timers.kind` that no code path
    // wrote: `step_timeout` in round four, and again in round five on the line
    // that repaired its sibling. The match below is exhaustive with no
    // catch-all, so a seventh kind does not compile until it is driven here,
    // and the assertion then requires the row on disk rather than a claim in a
    // document.
    let h = Harness::open().await;
    h.define("sleeper", json!({"steps": [{"kind": "sleep", "id": "nap", "ms": 86_400_000}]})).await;
    h.define(
        "waiter",
        json!({"steps": [{
            "kind": "wait_event", "id": "approval", "event": "approved",
            "key": "order-7", "timeout_ms": 86_400_000
        }]}),
    )
    .await;
    h.define(
        "bounded_step",
        json!({"steps": [{"kind": "task", "id": "work", "step_timeout_ms": 86_400_000}]}),
    )
    .await;
    h.define(
        "bounded_run",
        json!({
            "steps": [{"kind": "task", "id": "work"}],
            "limits": {"run_timeout_ms": 86_400_000}
        }),
    )
    .await;
    h.define("plain", json!({"steps": [{"kind": "task", "id": "work"}]})).await;
    h.define(
        "retrier",
        json!({"steps": [{"kind": "task", "id": "work", "retry": {"max_attempts": 2}}]}),
    )
    .await;

    for kind in TimerKind::ALL {
        let run_id = match kind {
            TimerKind::Sleep => h.start("sleeper", t0()).await,
            TimerKind::WaitTimeout => h.start("waiter", t0()).await,
            TimerKind::StepTimeout => h.start("bounded_step", t0()).await,
            TimerKind::RunTimeout => h.start("bounded_run", t0()).await,
            TimerKind::LeaseExpiry => {
                let run_id = h.start("plain", t0()).await;
                h.store.claim("checkout", "worker-1", 1, Some(&run_id), t0()).await.expect("claim");
                run_id
            }
            TimerKind::Reclaim => {
                let run_id = h.start("plain", t0()).await;
                h.store.claim("checkout", "worker-1", 1, Some(&run_id), t0()).await.expect("claim");
                // The lease lapses, which opens the window rather than deciding
                // anything.
                h.store.run_due_timers(t0() + Duration::seconds(31)).await.expect("expiry tick");
                run_id
            }
            TimerKind::Retry => {
                let run_id = h.start("retrier", t0()).await;
                let leases = h
                    .store
                    .claim("checkout", "worker-1", 1, Some(&run_id), t0())
                    .await
                    .expect("claim");
                h.store
                    .fail_step(
                        &leases[0].lease_id,
                        StepError {
                            class: StepErrorClass::Retryable,
                            message: "429 from the provider".into(),
                            details: None,
                            retry_after_ms: None,
                        },
                        t0(),
                    )
                    .await
                    .expect("fail");
                run_id
            }
        };
        let kinds: HashSet<TimerKind> =
            h.store.timers(&run_id).await.expect("timers").into_iter().map(|t| t.kind).collect();
        assert!(
            kinds.contains(&kind),
            "nothing writes a {kind:?} timer: run {run_id} holds {kinds:?}"
        );
    }
}

/// Every enum this crate writes to a column carries an `ALL` array and an
/// exhaustive `index`. The match inside `index` is what stops a new variant
/// compiling; this is what stops it being left out of `ALL`, which is what
/// `parse` and the table-driven tests are built on.
fn assert_all_is_complete<T: Copy + std::fmt::Debug>(
    all: &[T],
    index: impl Fn(T) -> usize,
    label: &str,
) {
    let seen: HashSet<usize> = all.iter().map(|v| index(*v)).collect();
    assert_eq!(seen.len(), all.len(), "{label}: ALL repeats a variant");
    for i in 0..all.len() {
        assert!(seen.contains(&i), "{label}: index {i} is missing from ALL");
    }
}

#[test]
fn every_enum_written_to_a_column_lists_all_of_itself() {
    assert_all_is_complete(&RunStatus::ALL, RunStatus::index, "RunStatus");
    assert_all_is_complete(&TimerKind::ALL, TimerKind::index, "TimerKind");
    assert_all_is_complete(&ConstructKind::ALL, ConstructKind::index, "ConstructKind");
    assert_all_is_complete(&AttemptStatus::ALL, AttemptStatus::index, "AttemptStatus");
    assert_all_is_complete(&StepKind::ALL, StepKind::index, "StepKind");
    assert_all_is_complete(&Effect::ALL, Effect::index, "Effect");
}

#[test]
fn every_column_value_survives_the_round_trip_back_out_of_the_database() {
    for v in RunStatus::ALL {
        assert_eq!(RunStatus::parse(v.as_str()), Some(v), "{v:?}");
    }
    for v in TimerKind::ALL {
        assert_eq!(TimerKind::parse(v.as_str()), Some(v), "{v:?}");
    }
    for v in AttemptStatus::ALL {
        assert_eq!(AttemptStatus::parse(v.as_str()), Some(v), "{v:?}");
    }
    for v in StepKind::ALL {
        assert_eq!(StepKind::parse(v.as_str()), Some(v), "{v:?}");
    }
    for v in Effect::ALL {
        assert_eq!(Effect::parse(v.as_str()), Some(v), "{v:?}");
    }
    assert_eq!(TimerKind::parse("idle_timeout"), None);
    assert_eq!(AttemptStatus::parse("running"), None);
    assert_eq!(RunStatus::parse("paused"), None);
}

// ---------------------------------------------------------------------------
// Leases
// ---------------------------------------------------------------------------

async fn leased_run(h: &Harness, effect: &str) -> (String, String) {
    h.define("one", json!({"steps": [{"kind": "task", "id": "work", "effect": effect}]})).await;
    let run_id = h.start("one", t0()).await;
    let leases = h.store.claim("checkout", "worker-1", 1, None, t0()).await.expect("claim");
    assert_eq!(leases.len(), 1);
    (run_id, leases[0].lease_id.clone())
}

#[tokio::test]
async fn an_expired_lease_becomes_orphaned_rather_than_being_requeued_at_once() {
    let h = Harness::open().await;
    let (run_id, _lease) = leased_run(&h, "at_least_once").await;

    h.store.run_due_timers(t0() + Duration::seconds(31)).await.expect("expiry tick");

    let a = h.attempt(&run_id, "work", 0).await;
    assert_eq!(a.status, AttemptStatus::Orphaned);
    assert!(a.reclaim_until.is_some(), "the window needs a close");
    assert!(
        h.timer_set(&run_id).await.contains(&("work".to_string(), TimerKind::Reclaim)),
        "the window's deadline has to be a row, not a column nobody queries"
    );

    // No second lease for that step key while the window is open, under either
    // effect. That is the safety property, and it is a property of one WHERE
    // clause.
    let again = h
        .store
        .claim("checkout", "worker-2", 10, None, t0() + Duration::seconds(32))
        .await
        .expect("claim");
    assert!(again.is_empty(), "an orphaned attempt is not runnable");
}

#[tokio::test]
async fn a_pending_slot_beside_an_orphaned_attempt_is_still_refused() {
    // Two things keep a second body off a step whose window is open, and this
    // covers the second one.
    //
    // The first is that no pending slot exists beside an `orphaned` attempt:
    // the slot is written only when the step has no attempt row at all, so
    // neither the materialiser nor the reconciler can produce one. That is what
    // `an_expired_lease_becomes_orphaned_rather_than_being_requeued_at_once`
    // exercises, and deleting `orphaned` from the claim predicate does not
    // break it, because the predicate never reaches the question.
    //
    // The second is the claim predicate itself, which is the property the
    // design states in those words. The state below is reachable only by
    // writing the row directly, and that is said out loud rather than dressed
    // up: this test exists so the clause is not a `WHERE` fragment with no case
    // behind it, and so a future mover that does create a slot beside a live
    // attempt cannot quietly grant a second lease.
    let h = Harness::open().await;
    let (run_id, _lease) = leased_run(&h, "at_least_once").await;
    h.store.run_due_timers(t0() + Duration::seconds(31)).await.expect("expiry tick");
    assert_eq!(h.attempt(&run_id, "work", 0).await.status, AttemptStatus::Orphaned);

    h.raw()
        .execute(
            "INSERT INTO step_attempts (run_id, step_key, attempt, kind, effect, status, span_id)
             VALUES (?1, 'work', 1, 'task', 'at_least_once', 'pending', 'aaaaaaaaaaaaaaaa')",
            [&run_id],
        )
        .expect("a slot beside an open window");

    let refused = h
        .store
        .claim("checkout", "worker-2", 10, None, t0() + Duration::seconds(32))
        .await
        .expect("claim");
    assert!(refused.is_empty(), "no second lease for a step key while its reclaim window is open");
}

#[tokio::test]
async fn an_expired_lease_is_reclaimed_and_the_attempt_is_not_lost() {
    let h = Harness::open().await;
    let (run_id, _lease) = leased_run(&h, "at_least_once").await;

    h.store.run_due_timers(t0() + Duration::seconds(31)).await.expect("expiry tick");
    h.store.run_due_timers(t0() + Duration::seconds(90)).await.expect("reclaim tick");

    let first = h.attempt(&run_id, "work", 0).await;
    assert_eq!(first.status, AttemptStatus::Failed);
    assert_eq!(first.error.as_deref(), Some("worker lease expired"));
    assert_eq!(
        first.lease_owner.as_deref(),
        Some("worker-1"),
        "the journal is append only: the lost attempt keeps its owner"
    );

    let second = h.attempt(&run_id, "work", 1).await;
    assert_eq!(second.status, AttemptStatus::Pending);

    let leases = h
        .store
        .claim("checkout", "worker-2", 10, None, t0() + Duration::seconds(91))
        .await
        .expect("claim");
    assert_eq!(leases.len(), 1);
    assert_eq!(leases[0].attempt, 1, "the requeue is a new attempt");
}

#[tokio::test]
async fn a_heartbeat_extends_a_lease_and_closes_an_open_reclaim_window() {
    let h = Harness::open().await;
    let (run_id, lease) = leased_run(&h, "at_least_once").await;

    h.store.run_due_timers(t0() + Duration::seconds(31)).await.expect("expiry tick");
    assert_eq!(h.attempt(&run_id, "work", 0).await.status, AttemptStatus::Orphaned);

    let beat = t0() + Duration::seconds(35);
    h.store.heartbeat(&lease, beat).await.expect("heartbeat");

    let a = h.attempt(&run_id, "work", 0).await;
    assert_eq!(
        a.status,
        AttemptStatus::Leased,
        "a heartbeat restores the state the attempt came from"
    );
    assert_eq!(a.lease_until, Some(beat + Duration::seconds(30)));

    let timers = h.timer_set(&run_id).await;
    assert!(
        !timers.contains(&("work".to_string(), TimerKind::Reclaim)),
        "the window's deadline must not outlive the state it guards"
    );
    assert!(timers.contains(&("work".to_string(), TimerKind::LeaseExpiry)));

    // And the window that was open decides nothing when its own old deadline
    // passes, because the row is gone. The new lease runs to t0 + 65, so t0 +
    // 62 is past the dead reclaim deadline and inside the live lease.
    h.store.run_due_timers(t0() + Duration::seconds(62)).await.expect("tick");
    assert_eq!(h.attempt(&run_id, "work", 0).await.status, AttemptStatus::Leased);
}

#[tokio::test]
async fn a_started_at_most_once_step_fails_its_run_when_the_window_closes() {
    let h = Harness::open().await;
    let (run_id, lease) = leased_run(&h, "at_most_once").await;
    h.store.started(&lease, t0()).await.expect("started");

    h.store.run_due_timers(t0() + Duration::seconds(31)).await.expect("expiry tick");
    h.store.run_due_timers(t0() + Duration::seconds(90)).await.expect("reclaim tick");

    let run = h.store.get_run(&run_id).await.expect("run");
    assert_eq!(run.status, RunStatus::Failed);
    assert!(
        run.error.as_deref().is_some_and(|e| e.contains("was started and its outcome is unknown")),
        "got {:?}",
        run.error
    );
}

#[tokio::test]
async fn a_started_at_least_once_step_is_requeued_when_the_window_closes() {
    let h = Harness::open().await;
    let (run_id, lease) = leased_run(&h, "at_least_once").await;
    h.store.started(&lease, t0()).await.expect("started");

    h.store.run_due_timers(t0() + Duration::seconds(31)).await.expect("expiry tick");
    h.store.run_due_timers(t0() + Duration::seconds(90)).await.expect("reclaim tick");

    assert_eq!(h.store.get_run(&run_id).await.expect("run").status, RunStatus::Running);
    assert_eq!(h.attempt(&run_id, "work", 1).await.status, AttemptStatus::Pending);
}

#[tokio::test]
async fn a_worker_a_later_attempt_replaced_cannot_complete_its_step() {
    let h = Harness::open().await;
    let (run_id, lease) = leased_run(&h, "at_least_once").await;
    h.store.run_due_timers(t0() + Duration::seconds(31)).await.expect("expiry tick");
    h.store.run_due_timers(t0() + Duration::seconds(90)).await.expect("reclaim tick");

    let err = h
        .store
        .complete_step(&lease, done(json!({"refund": "issued"})), t0())
        .await
        .expect_err("a fenced worker must not overwrite the retry that took over");
    assert!(format!("{err}").contains("fenced"), "got {err}");
    assert_eq!(h.store.get_run(&run_id).await.expect("run").status, RunStatus::Running);
}

// ---------------------------------------------------------------------------
// Movers
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_sleep_coming_due_moves_the_run_on() {
    let h = Harness::open().await;
    h.define(
        "napper",
        json!({"steps": [
            {"kind": "sleep", "id": "nap", "ms": 60_000},
            {"kind": "task", "id": "after"}
        ]}),
    )
    .await;
    let run_id = h.start("napper", t0()).await;
    assert!(h.pending_set(&run_id).await.is_empty());

    let report = h.store.run_due_timers(t0() + Duration::seconds(61)).await.expect("tick");
    assert_eq!(report.fired, 1);
    assert_eq!(h.pending_set(&run_id).await, HashSet::from(["after".to_string()]));
    assert!(h.timer_set(&run_id).await.is_empty(), "the sleep is over");
}

#[tokio::test]
async fn a_parallel_advances_only_when_every_branch_is_terminal() {
    let h = Harness::open().await;
    h.define(
        "fanout",
        json!({"steps": [
            {"kind": "parallel", "id": "fan", "branches": [{"kind": "task", "id": "a"}, {"kind": "task", "id": "b"}]},
            {"kind": "task", "id": "join"}
        ]}),
    )
    .await;
    let run_id = h.start("fanout", t0()).await;
    let leases = h.store.claim("checkout", "worker-1", 10, None, t0()).await.expect("claim");
    assert_eq!(leases.len(), 2);

    h.store
        .complete_step(&leases[0].lease_id, StepCompletion::default(), t0())
        .await
        .expect("complete");
    assert!(!h.pending_set(&run_id).await.contains("join"), "one branch is not the fan-in");

    h.store
        .complete_step(&leases[1].lease_id, StepCompletion::default(), t0())
        .await
        .expect("complete");
    assert!(h.pending_set(&run_id).await.contains("join"));
    assert_eq!(h.store.get_run(&run_id).await.expect("run").steps_used, 2);
}

#[tokio::test]
async fn a_loop_runs_its_bound_and_then_leaves() {
    let h = Harness::open().await;
    h.define(
        "poller",
        json!({"steps": [
            {"kind": "loop", "id": "poll", "body": {"kind": "task", "id": "check"}, "max_iterations": 3}
        ]}),
    )
    .await;
    let run_id = h.start("poller", t0()).await;
    for i in 0..3 {
        let leases = h.store.claim("checkout", "worker-1", 10, None, t0()).await.expect("claim");
        assert_eq!(leases.len(), 1, "iteration {i}");
        assert_eq!(leases[0].step_key, format!("poll[{i}]/check"));
        h.store
            .complete_step(&leases[0].lease_id, StepCompletion::default(), t0())
            .await
            .expect("complete");
    }
    assert_eq!(h.store.get_run(&run_id).await.expect("run").status, RunStatus::Succeeded);
}

#[tokio::test]
async fn a_child_run_ending_moves_its_parent_past_the_subworkflow_step() {
    let h = Harness::open().await;
    h.define("child", json!({"steps": [{"kind": "task", "id": "inner"}]})).await;
    h.define(
        "parent",
        json!({"steps": [
            {"kind": "subworkflow", "id": "refund", "definition": "child", "version": 1},
            {"kind": "task", "id": "after"}
        ]}),
    )
    .await;
    let parent = h.start("parent", t0()).await;
    assert!(h.pending_set(&parent).await.is_empty());

    let leases = h.store.claim("checkout", "worker-1", 10, None, t0()).await.expect("claim");
    assert_eq!(leases.len(), 1, "the child's step is the claimable one");
    assert_eq!(leases[0].step_key, "inner");
    let child = leases[0].run_id.clone();
    assert_ne!(child, parent);

    h.store
        .complete_step(&leases[0].lease_id, StepCompletion::default(), t0())
        .await
        .expect("complete");

    assert_eq!(h.store.get_run(&child).await.expect("child").status, RunStatus::Succeeded);
    assert_eq!(
        h.pending_set(&parent).await,
        HashSet::from(["after".to_string()]),
        "a terminal child is the tenth cursor mover"
    );
    // Ceilings are enforced against the root, so a child's step counts there.
    assert_eq!(h.store.get_run(&parent).await.expect("parent").steps_used, 1);
}

#[tokio::test]
async fn the_run_ceiling_is_spent_by_a_child_and_refuses_the_next_lease() {
    let h = Harness::open().await;
    h.define("child", json!({"steps": [{"kind": "task", "id": "inner"}]})).await;
    h.define(
        "parent",
        json!({
            "steps": [
                {"kind": "subworkflow", "id": "refund", "definition": "child", "version": 1},
                {"kind": "task", "id": "after"}
            ],
            "limits": {"max_steps": 1}
        }),
    )
    .await;
    let parent = h.start("parent", t0()).await;
    let leases = h.store.claim("checkout", "worker-1", 10, None, t0()).await.expect("claim");
    h.store
        .complete_step(&leases[0].lease_id, StepCompletion::default(), t0())
        .await
        .expect("complete");

    assert!(h.pending_set(&parent).await.contains("after"));
    let refused = h.store.claim("checkout", "worker-1", 10, None, t0()).await.expect("claim");
    assert!(
        refused.is_empty(),
        "max_steps 1 was spent by the child, so the parent's next step is refused"
    );
}

#[tokio::test]
async fn a_run_timeout_ends_the_run_and_takes_its_deadlines_and_waits_with_it() {
    let h = Harness::open().await;
    h.define(
        "slow",
        json!({
            "steps": [{
                "kind": "wait_event", "id": "approval", "event": "approved",
                "key": "order-7", "timeout_ms": 86_400_000
            }],
            "limits": {"run_timeout_ms": 60_000}
        }),
    )
    .await;
    let run_id = h.start("slow", t0()).await;
    assert_eq!(h.wait_set(&run_id).await.len(), 1);

    h.store.run_due_timers(t0() + Duration::seconds(61)).await.expect("tick");

    let run = h.store.get_run(&run_id).await.expect("run");
    assert_eq!(run.status, RunStatus::TimedOut);
    assert!(h.timer_set(&run_id).await.is_empty(), "an ended run keeps no deadlines");
    assert!(
        h.wait_set(&run_id).await.is_empty(),
        "a leftover wait row eats the next run's approval"
    );
}

#[tokio::test]
async fn a_step_timeout_bounds_a_worker_that_heartbeats_for_ever() {
    let h = Harness::open().await;
    h.define(
        "bounded",
        json!({"steps": [{"kind": "task", "id": "work", "step_timeout_ms": 60_000}]}),
    )
    .await;
    let run_id = h.start("bounded", t0()).await;
    let leases = h.store.claim("checkout", "worker-1", 1, None, t0()).await.expect("claim");

    // The worker is alive and says so, which keeps the lease for ever.
    for second in [10, 20, 30, 40, 50, 60, 70] {
        h.store
            .heartbeat(&leases[0].lease_id, t0() + Duration::seconds(second))
            .await
            .expect("heartbeat");
    }
    assert_eq!(h.store.get_run(&run_id).await.expect("run").status, RunStatus::Running);

    h.store.run_due_timers(t0() + Duration::seconds(71)).await.expect("tick");

    let run = h.store.get_run(&run_id).await.expect("run");
    assert_eq!(run.status, RunStatus::Failed);
    assert!(
        run.error.as_deref().is_some_and(|e| e.contains("exceeded step_timeout")),
        "got {:?}",
        run.error
    );
}

// ---------------------------------------------------------------------------
// Waits and events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_event_moves_the_run_that_was_waiting_for_it() {
    let h = Harness::open().await;
    h.define(
        "approver",
        json!({"steps": [
            {"kind": "wait_event", "id": "approval", "event": "approved",
             "key": "order-7", "timeout_ms": 86_400_000},
            {"kind": "task", "id": "refund"}
        ]}),
    )
    .await;
    let run_id = h.start("approver", t0()).await;

    let outcome = h
        .store
        .deliver_event(
            InboundEvent {
                project: "checkout".into(),
                event: "approved".into(),
                correlation_key: "order-7".into(),
                event_id: "evt-1".into(),
                id_is_derived: false,
                payload: Some(json!({"by": "ops"})),
            },
            t0() + Duration::seconds(5),
        )
        .await
        .expect("deliver");
    assert_eq!(outcome.outcome, EventOutcome::Delivered);

    let settled = h.attempt(&run_id, "approval", 0).await;
    assert_eq!(settled.status, AttemptStatus::Succeeded);
    assert_eq!(settled.output, Some(json!({"by": "ops"})));
    assert!(h.wait_set(&run_id).await.is_empty());
    assert!(h.timer_set(&run_id).await.is_empty(), "the wait's deadline goes with the wait");
    assert!(h.pending_set(&run_id).await.contains("refund"));
}

#[tokio::test]
async fn an_event_that_arrived_early_is_consumed_when_the_run_reaches_the_wait() {
    let h = Harness::open().await;
    h.define(
        "approver",
        json!({"steps": [
            {"kind": "wait_event", "id": "approval", "event": "approved",
             "key": "order-7", "timeout_ms": 86_400_000},
            {"kind": "task", "id": "refund"}
        ]}),
    )
    .await;

    let parked = h
        .store
        .deliver_event(
            InboundEvent {
                project: "checkout".into(),
                event: "approved".into(),
                correlation_key: "order-7".into(),
                event_id: "evt-1".into(),
                id_is_derived: false,
                payload: Some(json!({"by": "ops"})),
            },
            t0(),
        )
        .await
        .expect("deliver");
    assert_eq!(parked.outcome, EventOutcome::Parked);

    let run_id = h.start("approver", t0() + Duration::seconds(5)).await;
    assert!(h.wait_set(&run_id).await.is_empty(), "the run never enters waiting at all");
    assert_eq!(h.attempt(&run_id, "approval", 0).await.output, Some(json!({"by": "ops"})));
    assert!(h.pending_set(&run_id).await.contains("refund"));
}

#[tokio::test]
async fn a_second_delivery_of_one_event_id_is_a_no_op() {
    let h = Harness::open().await;
    h.define(
        "approver",
        json!({"steps": [
            {"kind": "wait_event", "id": "approval", "event": "approved",
             "key": "order-7", "timeout_ms": 86_400_000}
        ]}),
    )
    .await;
    h.start("approver", t0()).await;
    let event = InboundEvent {
        project: "checkout".into(),
        event: "approved".into(),
        correlation_key: "order-7".into(),
        event_id: "evt-1".into(),
        id_is_derived: false,
        payload: None,
    };
    assert_eq!(
        h.store.deliver_event(event.clone(), t0()).await.expect("one").outcome,
        EventOutcome::Delivered
    );
    assert_eq!(
        h.store.deliver_event(event, t0()).await.expect("two").outcome,
        EventOutcome::Duplicate,
        "the provider's fifth retry must not become a spare event"
    );
}

#[tokio::test]
async fn a_wait_that_expires_moves_the_run_when_it_says_so() {
    // `on_timeout: continue` is the documented way to reach a step that reads
    // `steps.approval.status`. The default, `fail`, ends the run and is held
    // by its own test.
    let h = Harness::open().await;
    h.define(
        "approver",
        json!({"steps": [
            {"kind": "wait_event", "id": "approval", "event": "approved",
             "key": "order-7", "timeout_ms": 60_000, "on_timeout": "continue"},
            {"kind": "task", "id": "escalate"}
        ]}),
    )
    .await;
    let run_id = h.start("approver", t0()).await;

    let report = h.store.run_due_timers(t0() + Duration::seconds(61)).await.expect("tick");
    assert_eq!(report.fired, 1);

    assert_eq!(h.attempt(&run_id, "approval", 0).await.status, AttemptStatus::TimedOut);
    assert!(h.wait_set(&run_id).await.is_empty());
    assert!(h.pending_set(&run_id).await.contains("escalate"));
}

#[tokio::test]
async fn an_ended_runs_leftover_wait_cannot_eat_the_next_runs_approval() {
    let h = Harness::open().await;
    h.define(
        "approver",
        json!({"steps": [
            {"kind": "wait_event", "id": "approval", "event": "approved",
             "key": "order-7", "timeout_ms": 86_400_000}
        ]}),
    )
    .await;
    h.define(
        "doomed",
        json!({
            "steps": [{
                "kind": "wait_event", "id": "approval", "event": "approved",
                "key": "order-7", "timeout_ms": 86_400_000
            }],
            "limits": {"run_timeout_ms": 60_000}
        }),
    )
    .await;

    let dead = h.start("doomed", t0()).await;
    h.store.run_due_timers(t0() + Duration::seconds(61)).await.expect("tick");
    assert_eq!(h.store.get_run(&dead).await.expect("run").status, RunStatus::TimedOut);

    let live = h.start("approver", t0() + Duration::seconds(70)).await;
    let outcome = h
        .store
        .deliver_event(
            InboundEvent {
                project: "checkout".into(),
                event: "approved".into(),
                correlation_key: "order-7".into(),
                event_id: "evt-1".into(),
                id_is_derived: false,
                payload: Some(json!({"by": "ops"})),
            },
            t0() + Duration::seconds(80),
        )
        .await
        .expect("deliver");
    assert_eq!(outcome.outcome, EventOutcome::Delivered);
    assert_eq!(
        h.attempt(&live, "approval", 0).await.output,
        Some(json!({"by": "ops"})),
        "the approval belongs to the live run"
    );
}

#[tokio::test]
async fn the_reconciler_sweeps_rows_an_ended_run_left_behind_and_counts_them() {
    // Deliberately outside the "not terminal" predicate: the stale rows this
    // catches belong to runs that already ended, which is precisely why the
    // rest of the net could never have caught them.
    let h = Harness::open().await;
    h.define(
        "approver",
        json!({"steps": [
            {"kind": "wait_event", "id": "approval", "event": "approved",
             "key": "order-7", "timeout_ms": 86_400_000}
        ]}),
    )
    .await;
    let run_id = h.start("approver", t0()).await;
    h.raw()
        .execute("UPDATE runs SET status = 'cancelled' WHERE run_id = ?1", [&run_id])
        .expect("a terminator that forgot its cleanup");

    let report = h.store.reconcile(t0() + Duration::minutes(10)).await.expect("reconcile");
    assert_eq!(report.stale_rows_deleted, 2, "one waits row and one wait_timeout row");
    assert!(h.wait_set(&run_id).await.is_empty());
    assert!(h.timer_set(&run_id).await.is_empty());
}

#[tokio::test]
async fn the_reconciler_repairs_nothing_on_a_healthy_run() {
    let h = Harness::open().await;
    h.define(
        "approver",
        json!({"steps": [
            {"kind": "wait_event", "id": "approval", "event": "approved",
             "key": "order-7", "timeout_ms": 86_400_000}
        ]}),
    )
    .await;
    let run_id = h.start("approver", t0()).await;
    let report = h.store.reconcile(t0() + Duration::minutes(10)).await.expect("reconcile");
    assert_eq!(
        report,
        Default::default(),
        "a non-zero count is a defect report, so a healthy run must report zero"
    );
    assert_eq!(h.wait_set(&run_id).await.len(), 1);
}

// ---------------------------------------------------------------------------
// Fields with a reader
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_run_carries_its_input_its_definition_and_a_gapless_event_stream() {
    let h = Harness::open().await;
    h.define("two", json!({"steps": [{"kind": "task", "id": "a"}, {"kind": "task", "id": "b"}]}))
        .await;
    let stored = h.store.get_definition("checkout", "two", 1).await.expect("definition");
    assert_eq!(stored.kind, DefinitionKind::Workflow);

    let run_id = h
        .store
        .start_run(
            StartRun {
                project: "checkout".into(),
                definition_id: "two".into(),
                definition_version: Some(1),
                input: Some(json!({"order": 7})),
                correlation: None,
                idempotency_key: None,
            },
            t0(),
        )
        .await
        .expect("start")
        .run_id;
    assert_eq!(h.store.get_run(&run_id).await.expect("run").input, Some(json!({"order": 7})));

    let leases = h.store.claim("checkout", "worker-1", 10, None, t0()).await.expect("claim");
    h.store
        .complete_step(&leases[0].lease_id, StepCompletion::default(), t0())
        .await
        .expect("complete");

    let events = h.store.events(&run_id, -1, 500).await.expect("events").events;
    let seqs: Vec<i64> = events.iter().map(|e| e.seq).collect();
    assert_eq!(
        seqs,
        (0..seqs.len() as i64).collect::<Vec<_>>(),
        "a reader reattaches at a sequence number, so the stream cannot have a gap"
    );
    assert_eq!(events[0].kind, RunEventKind::RunStarted);
    assert!(events.iter().any(|e| e.kind == RunEventKind::RunCursor));
    assert_eq!(
        h.store.events(&run_id, 0, 500).await.expect("events").events.len(),
        events.len() - 1,
        "reattaching after seq 0 skips exactly one row"
    );
}

#[tokio::test]
async fn an_expired_lease_records_when_the_window_opened_and_when_the_attempt_ended() {
    let h = Harness::open().await;
    let (run_id, _lease) = leased_run(&h, "at_least_once").await;
    h.store.run_due_timers(t0() + Duration::seconds(31)).await.expect("expiry tick");
    let a = h.attempt(&run_id, "work", 0).await;
    assert_eq!(a.orphaned_at, Some(t0() + Duration::seconds(31)));
    assert_eq!(a.reclaim_until, Some(t0() + Duration::seconds(61)));
    assert_eq!(a.ended_at, None, "an orphaned attempt has not ended");

    h.store.run_due_timers(t0() + Duration::seconds(90)).await.expect("reclaim tick");
    assert_eq!(h.attempt(&run_id, "work", 0).await.ended_at, Some(t0() + Duration::seconds(90)));
}

#[tokio::test]
async fn an_event_goes_to_the_wait_that_expires_soonest() {
    let h = Harness::open().await;
    h.define(
        "urgent",
        json!({"steps": [{
            "kind": "wait_event", "id": "approval", "event": "approved",
            "key": "order-7", "timeout_ms": 60_000
        }]}),
    )
    .await;
    h.define(
        "patient",
        json!({"steps": [{
            "kind": "wait_event", "id": "approval", "event": "approved",
            "key": "order-7", "timeout_ms": 86_400_000
        }]}),
    )
    .await;
    let patient = h.start("patient", t0()).await;
    let urgent = h.start("urgent", t0()).await;

    h.store
        .deliver_event(
            InboundEvent {
                project: "checkout".into(),
                event: "approved".into(),
                correlation_key: "order-7".into(),
                event_id: "evt-1".into(),
                id_is_derived: false,
                payload: Some(json!({"by": "ops"})),
            },
            t0() + Duration::seconds(5),
        )
        .await
        .expect("deliver");

    assert_eq!(
        h.attempt(&urgent, "approval", 0).await.output,
        Some(json!({"by": "ops"})),
        "the wait with the nearer expiry takes it"
    );
    assert_eq!(
        h.wait_set(&patient).await.len(),
        1,
        "one event, one wait: the other run keeps waiting"
    );
}

#[tokio::test]
async fn a_definition_that_names_itself_is_refused_before_it_is_stored() {
    // A self-referencing definition is a run tree no ceiling bounds, so the
    // validator refuses it at write time rather than the depth bound refusing
    // it eight runs deep.
    let h = Harness::open().await;
    let err = h
        .store
        .put_definition(DefinitionDocument {
            project: "checkout".into(),
            id: "recursive".into(),
            kind: DefinitionKind::Workflow,
            version: Some(1),
            body: json!({"steps": [
                {"kind": "subworkflow", "id": "again", "definition": "recursive"}
            ]}),
        })
        .await
        .expect_err("an unbounded run tree is a run tree no ceiling touches");
    let message = format!("{err}");
    assert!(message.contains("names the definition it is part of"), "got {message}");

    let stored: i64 =
        h.raw().query_row("SELECT COUNT(*) FROM definitions", [], |r| r.get(0)).expect("count");
    assert_eq!(stored, 0, "nothing is stored for a refused document");
}

// ---------------------------------------------------------------------------
// A worker calling twice
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_worker_that_calls_fail_after_completing_cannot_kill_the_run_it_finished() {
    // `complete_step` and `heartbeat` both refuse a lease whose attempt has
    // ended. `fail_step` did not, so a retried callback from a worker that had
    // already succeeded overwrote the attempt and ended a healthy run.
    let h = Harness::open().await;
    h.define("two", json!({"steps": [{"kind": "task", "id": "a"}, {"kind": "task", "id": "b"}]}))
        .await;
    let run_id = h.start("two", t0()).await;
    let leases = h.store.claim("checkout", "worker-1", 1, None, t0()).await.expect("claim");
    let lease = leases[0].lease_id.clone();
    h.store.complete_step(&lease, done(json!({"paid": true})), t0()).await.expect("complete");

    let err = h
        .store
        .fail_step(&lease, fatal("the callback was retried"), t0())
        .await
        .expect_err("a second call on a spent lease must not end the run");
    assert!(format!("{err}").contains("already ended"), "got {err}");

    let run = h.store.get_run(&run_id).await.expect("run");
    assert_eq!(run.status, RunStatus::Running, "the run had already moved past that step");
    let a = h.attempt(&run_id, "a", 0).await;
    assert_eq!(a.status, AttemptStatus::Succeeded, "the journal keeps what actually happened");
    assert_eq!(a.output, Some(json!({"paid": true})));
    assert!(h.pending_set(&run_id).await.contains("b"));
}

#[tokio::test]
async fn a_worker_that_declares_a_body_after_completing_cannot_reopen_the_attempt() {
    // The worst version of the same missing guard. `started` restored a
    // succeeded attempt to `started` and gave it a fresh lease deadline, so the
    // reclaim path then requeued a step that was already done and a second
    // worker was handed its body.
    let h = Harness::open().await;
    h.define("two", json!({"steps": [{"kind": "task", "id": "a"}, {"kind": "task", "id": "b"}]}))
        .await;
    let run_id = h.start("two", t0()).await;
    let leases = h.store.claim("checkout", "worker-1", 1, None, t0()).await.expect("claim");
    let lease = leases[0].lease_id.clone();
    h.store.complete_step(&lease, done(json!({"paid": true})), t0()).await.expect("complete");

    let err = h
        .store
        .started(&lease, t0() + Duration::seconds(1))
        .await
        .expect_err("a spent lease cannot declare a body");
    assert!(format!("{err}").contains("already ended"), "got {err}");
    assert_eq!(h.attempt(&run_id, "a", 0).await.status, AttemptStatus::Succeeded);
    assert!(
        !h.timer_set(&run_id).await.contains(&("a".to_string(), TimerKind::LeaseExpiry)),
        "a finished attempt must not carry a lease deadline"
    );

    // And the deadline that would have expired decides nothing, so no second
    // worker is ever offered the body of a step that already succeeded.
    h.store.run_due_timers(t0() + Duration::seconds(40)).await.expect("expiry tick");
    h.store.run_due_timers(t0() + Duration::seconds(100)).await.expect("reclaim tick");
    let offered: HashSet<String> = h
        .store
        .claim("checkout", "worker-2", 10, None, t0() + Duration::seconds(101))
        .await
        .expect("claim")
        .into_iter()
        .map(|l| l.step_key)
        .collect();
    assert_eq!(offered, HashSet::from(["b".to_string()]), "step a is done and stays done");
}

// ---------------------------------------------------------------------------
// Positions with no mover
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_fan_out_over_nothing_advances_rather_than_stopping_the_run_for_ever() {
    // A fan-out with no branches writes no slot, so no completion arrives to
    // move it, and it holds no deadline and no wait either. The run stopped
    // there for ever and the reconciler reported it healthy, which is the exact
    // shape of defect this crate is built against.
    let h = Harness::open().await;
    h.define(
        "empty_foreach",
        json!({"steps": [
            {"kind": "foreach", "id": "each", "over": "input.items", "body": {"kind": "task", "id": "item"}, "max_items": 10},
            {"kind": "task", "id": "after"}
        ]}),
    )
    .await;

    for def in ["empty_foreach"] {
        let run_id = h.start_with(def, Some(json!({"items": []})), t0()).await;
        assert_eq!(
            h.pending_set(&run_id).await,
            HashSet::from(["after".to_string()]),
            "{def} has nothing to wait for, so the run is past it"
        );
        assert_eq!(h.store.get_run(&run_id).await.expect("run").status, RunStatus::Running);
    }
}

#[tokio::test]
async fn a_loop_bounded_at_zero_and_a_parallel_with_no_branch_are_refused() {
    // Both used to be accepted and both were positions nothing could move.
    // The schema and the validator now agree that a bound of zero is not a
    // bound and a fan-out over nothing is not a fan-out.
    let h = Harness::open().await;
    for (id, body) in [
        (
            "never",
            json!({"steps": [
                {"kind": "loop", "id": "poll", "body": {"kind": "task", "id": "check"}, "max_iterations": 0}
            ]}),
        ),
        ("nobody", json!({"steps": [{"kind": "parallel", "id": "fan", "branches": []}]})),
    ] {
        let err = h
            .store
            .put_definition(DefinitionDocument {
                project: "checkout".into(),
                id: id.into(),
                kind: DefinitionKind::Workflow,
                version: Some(1),
                body,
            })
            .await
            .expect_err("refused at write time");
        assert!(matches!(err, mcpd_runs::Error::Invalid(_)), "{id}: got {err}");
    }
}

#[tokio::test]
async fn an_expired_wait_does_not_take_the_event_a_live_run_is_waiting_for() {
    // The delivery lookup prefers the nearest expiry, and it did not check that
    // the expiry was still in the future. That ordering therefore guaranteed
    // the opposite of what it was for: the most stale wait won every time, so
    // an approval was eaten by a run whose window had closed an hour earlier
    // and whose `wait_timeout` had not yet been ticked.
    let h = Harness::open().await;
    h.define(
        "urgent",
        json!({"steps": [{
            "kind": "wait_event", "id": "approval", "event": "approved",
            "key": "order-7", "timeout_ms": 60_000
        }]}),
    )
    .await;
    h.define(
        "patient",
        json!({"steps": [{
            "kind": "wait_event", "id": "approval", "event": "approved",
            "key": "order-7", "timeout_ms": 86_400_000
        }]}),
    )
    .await;
    let stale = h.start("urgent", t0()).await;
    let live = h.start("patient", t0()).await;

    let outcome = h
        .store
        .deliver_event(
            InboundEvent {
                project: "checkout".into(),
                event: "approved".into(),
                correlation_key: "order-7".into(),
                event_id: "evt-1".into(),
                id_is_derived: false,
                payload: Some(json!({"by": "ops"})),
            },
            t0() + Duration::hours(1),
        )
        .await
        .expect("deliver");

    assert_eq!(outcome.outcome, EventOutcome::Delivered);
    assert_eq!(
        h.attempt(&live, "approval", 0).await.output,
        Some(json!({"by": "ops"})),
        "the only run still inside its window takes the event"
    );
    assert_eq!(
        h.wait_set(&stale).await.len(),
        1,
        "the expired run keeps its row until its own deadline settles it"
    );
}

/// `node_attempts` counts repetition, which means it has to fold three things
/// that look different in the journal into one number.
///
/// A retry writes another attempt row at the same step key. A loop iteration
/// writes a new step key at the same node. A `foreach` item does too. All three
/// are the same node running again, and the count a monitor reads has to say so
/// without the monitor having to hold the definition to work it out.
#[tokio::test]
async fn node_attempts_folds_retries_and_iterations_onto_one_node() {
    let h = Harness::open().await;
    h.define(
        "repeats",
        json!({"steps": [
            {"kind": "task", "id": "flaky", "retry": {"max_attempts": 3, "initial_ms": 1}},
            {"kind": "foreach", "id": "fan", "over": "input.items",
             "body": {"kind": "task", "id": "one"}, "concurrency": 4, "max_items": 16},
            {"kind": "loop", "id": "go", "body": {"kind": "task", "id": "inner"},
             "max_iterations": 4},
            {"kind": "task", "id": "never",
             "when": {"path": "input.absent", "op": "eq", "value": true}}
        ]}),
    )
    .await;

    let run = h.start_with("repeats", Some(json!({"items": [1, 2, 3]})), t0()).await;

    // Two retryable failures, so `flaky` ends up with attempts 0, 1 and 2. The
    // retry timer is what writes the next attempt row, so the clock has to move
    // past the backoff between them.
    let mut at = t0();
    for _ in 0..2 {
        let leases = h.store.claim("checkout", "w", 1, Some(&run), at).await.expect("claim");
        h.store
            .fail_step(
                &leases[0].lease_id,
                StepError {
                    class: StepErrorClass::Retryable,
                    message: "429 from the provider".into(),
                    details: None,
                    retry_after_ms: None,
                },
                at,
            )
            .await
            .expect("fail");
        at += Duration::seconds(1);
        h.store.run_due_timers(at).await.expect("retry tick");
    }

    // Then everything left, taken as it becomes claimable: the third attempt at
    // `flaky`, the three `fan` items, and the four loop iterations.
    loop {
        let leases = h.store.claim("checkout", "w", 8, Some(&run), at).await.expect("claim");
        if leases.is_empty() {
            break;
        }
        for lease in leases {
            h.store.complete_step(&lease.lease_id, done(json!(1)), at).await.expect("complete");
        }
    }

    assert_eq!(
        h.store.get_run(&run).await.expect("read the run").status,
        RunStatus::Succeeded,
        "the run has to have reached the guarded step for its skip to be written"
    );

    let counts: Vec<(String, u32)> = h
        .store
        .node_attempts(std::slice::from_ref(&run))
        .await
        .expect("node attempts")
        .into_iter()
        .map(|c| (c.node, c.attempts))
        .collect();

    assert_eq!(
        counts,
        // In node order, which is not the order the definition declares them.
        vec![
            // Three step keys, `fan[0]` to `fan[2]`, at one node.
            ("fan".to_owned(), 3),
            // Three attempts at one step key, because two of them were retries.
            ("flaky".to_owned(), 3),
            // Four step keys, `go[0]/inner` to `go[3]/inner`, at one node. The
            // index is folded away and the body id is not, because two bodies
            // of one loop would be two nodes.
            ("go/inner".to_owned(), 4),
        ],
        "and `never` is absent: its guard was false, so the node never ran"
    );
    assert!(
        h.store
            .attempts(&run)
            .await
            .expect("attempts")
            .iter()
            .any(|a| a.step_key == "never" && a.status == AttemptStatus::Skipped),
        "the skipped row does exist, so its absence above is the filter and not the fixture"
    );
}

/// Rows for many runs come back in one order, and a run that is asked about but
/// has nothing is absent rather than present with a zero.
#[tokio::test]
async fn node_attempts_is_ordered_and_leaves_out_what_it_has_nothing_for() {
    let h = Harness::open().await;
    h.define("plain", json!({"steps": [{"kind": "task", "id": "work"}]})).await;

    let first = h.start("plain", t0()).await;
    let second = h.start("plain", t0()).await;

    // Named in one order and read back in another, so the ordering under test
    // is the store's and not the caller's.
    let mut ids = vec![second.clone(), first.clone(), "run_nothing_here".to_owned()];
    ids.sort_by(|a, b| b.cmp(a));
    let counts = h.store.node_attempts(&ids).await.expect("node attempts");

    let mut expected = vec![first, second];
    expected.sort();
    assert_eq!(
        counts.iter().map(|c| (c.run_id.clone(), c.node.clone(), c.attempts)).collect::<Vec<_>>(),
        expected.into_iter().map(|id| (id, "work".to_owned(), 1)).collect::<Vec<_>>(),
        "ascending by run id, and the run that does not exist contributes nothing"
    );

    assert!(
        h.store.node_attempts(&[]).await.expect("no run ids").is_empty(),
        "asking about no runs asks the database nothing"
    );
}
