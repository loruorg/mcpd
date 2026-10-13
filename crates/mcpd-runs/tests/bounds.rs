//! Durations a definition or a worker names, and the clock they are added to.
//!
//! A sleep of four hundred trillion milliseconds was accepted, and adding it
//! to the clock panicked inside the transaction that started the run. The
//! release build aborts on a panic, so one definition stopped the daemon for
//! every tenant. These tests hold the three layers that close that: the
//! validator's upper bound, the checked addition behind it for a definition
//! stored before the bound existed, and the clamp on the way into a column.

use mcpd_runs::model::{DefinitionKind, RunStatus, StepErrorClass, TimerKind};
mod common;

use common::TestStore;
use mcpd_runs::store::{DefinitionDocument, RunConfig, StartRun, StepError};
use mcpd_runs::{Error, MAX_DURATION_MS};
use serde_json::{json, Value};
use time::{Duration, OffsetDateTime};

fn t0() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("a valid instant")
}

/// The latest instant a timestamp column can hold.
fn latest() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(i64::MAX)).expect("a valid instant")
}

struct Harness {
    store: TestStore,
}

impl Harness {
    async fn open() -> Self {
        Self { store: TestStore::open(RunConfig::default()).await }
    }

    async fn put(&self, id: &str, body: Value) -> Result<(), Error> {
        self.store
            .put_definition(DefinitionDocument {
                project: "checkout".into(),
                id: id.into(),
                kind: DefinitionKind::Workflow,
                version: Some(1),
                body,
            })
            .await
    }

    /// Write a definition straight into the table, past the validator, the
    /// way a definition stored by an older build would sit there.
    async fn put_unchecked(&self, id: &str, body: &Value) {
        self.store
            .exec(
                "INSERT INTO definitions (project, id, version, kind, body, body_hash, created_at)
                 VALUES ('checkout', ?1, 1, 'workflow', ?2, ?3, 0)",
                &[id, &body.to_string(), &format!("unchecked-{id}")],
            )
            .await;
    }

    async fn start(&self, id: &str) -> Result<String, Error> {
        self.store
            .start_run(
                StartRun {
                    project: "checkout".into(),
                    definition_id: id.into(),
                    definition_version: Some(1),
                    ..Default::default()
                },
                t0(),
            )
            .await
            .map(|run| run.run_id)
    }

    async fn due(&self, run_id: &str, kind: TimerKind) -> OffsetDateTime {
        self.store
            .timers(run_id)
            .await
            .expect("timers")
            .into_iter()
            .find(|t| t.kind == kind)
            .unwrap_or_else(|| panic!("no {} timer on run {run_id}", kind.as_str()))
            .due_at
    }
}

/// One definition per duration field, each holding `ms` in that field and
/// nothing else out of the ordinary, with the path the error has to name.
fn one_per_field(ms: i64) -> Vec<(&'static str, Value)> {
    vec![
        ("steps[0].ms", json!({"steps": [{"kind": "sleep", "id": "nap", "ms": ms}]})),
        (
            "steps[0].timeout_ms",
            json!({"steps": [{"kind": "wait_event", "id": "approval", "event": "approved",
                              "key": "k", "timeout_ms": ms}]}),
        ),
        (
            "steps[0].step_timeout_ms",
            json!({"steps": [{"kind": "task", "id": "work", "step_timeout_ms": ms}]}),
        ),
        (
            "steps[0].retry.initial_ms",
            json!({"steps": [{"kind": "task", "id": "work",
                              "retry": {"max_attempts": 2, "initial_ms": ms, "max_ms": ms}}]}),
        ),
        (
            "steps[0].retry.max_ms",
            json!({"steps": [{"kind": "task", "id": "work",
                              "retry": {"max_attempts": 2, "initial_ms": 1000, "max_ms": ms}}]}),
        ),
        (
            "limits.run_timeout_ms",
            json!({"limits": {"run_timeout_ms": ms},
                   "steps": [{"kind": "task", "id": "work"}]}),
        ),
        (
            "steps[0].branches[0].step_timeout_ms",
            json!({"steps": [{"kind": "parallel", "id": "fan", "branches": [
                {"kind": "task", "id": "left", "step_timeout_ms": ms}
            ]}]}),
        ),
    ]
}

backend_tests! {
async fn every_duration_past_the_bound_is_refused_naming_the_field_the_bound_and_the_value() {
    let h = Harness::open().await;
    let over = MAX_DURATION_MS + 1;
    for (i, (field, body)) in one_per_field(over).into_iter().enumerate() {
        let err = h.put(&format!("over_{i}"), body).await.expect_err(field);
        let Error::Invalid(message) = err else {
            panic!("{field}: expected a refusal naming the field, got {err:?}");
        };
        assert!(message.starts_with(&format!("{field}: ")), "{field}: {message}");
        assert!(message.contains(&over.to_string()), "{field} names the value: {message}");
        assert!(
            message.contains(&MAX_DURATION_MS.to_string()),
            "{field} names the bound: {message}"
        );
    }
    // The case the defect was found with.
    let err = h
        .put(
            "huge",
            json!({"steps": [{"kind": "sleep", "id": "nap", "ms": 400_000_000_000_000_i64}]}),
        )
        .await
        .expect_err("a sleep of four hundred trillion milliseconds");
    assert!(err.to_string().contains("400000000000000"), "{err}");

    // The bound itself is a duration a definition may name.
    for (i, (field, body)) in one_per_field(MAX_DURATION_MS).into_iter().enumerate() {
        h.put(&format!("at_{i}"), body).await.unwrap_or_else(|e| panic!("{field}: {e}"));
    }
}
}

backend_tests! {
async fn a_definition_stored_before_the_bound_starts_a_run_instead_of_stopping_the_daemon() {
    let h = Harness::open().await;
    let huge = 400_000_000_000_000_i64;
    h.put_unchecked(
        "old_sleep",
        &json!({"limits": {"run_timeout_ms": huge},
                "steps": [{"kind": "sleep", "id": "nap", "ms": huge}]}),
    ).await;
    h.put_unchecked(
        "old_wait",
        &json!({"steps": [{"kind": "wait_event", "id": "approval", "event": "approved",
                           "key": "k", "timeout_ms": huge}]}),
    ).await;
    h.put_unchecked(
        "old_task",
        &json!({"steps": [{"kind": "task", "id": "work", "step_timeout_ms": huge}]}),
    ).await;

    // Each start adds the duration to the clock inside its transaction. The
    // addition panicked there, which a test build reports as a failed task and
    // a release build answers by aborting the process.
    let sleeping = h.start("old_sleep").await.expect("a run starts on an old sleep");
    let waiting = h.start("old_wait").await.expect("a run starts on an old wait");
    let working = h.start("old_task").await.expect("a run starts on an old step timeout");

    // The deadlines land on the latest instant a column holds, which reads as
    // never rather than as a deadline already passed.
    assert_eq!(h.due(&sleeping, TimerKind::Sleep).await, latest());
    assert_eq!(h.due(&sleeping, TimerKind::RunTimeout).await, latest());
    assert_eq!(h.due(&waiting, TimerKind::WaitTimeout).await, latest());
    assert_eq!(h.due(&working, TimerKind::StepTimeout).await, latest());

    let report = h.store.run_due_timers(t0() + Duration::days(365)).await.expect("tick");
    assert_eq!(report.fired, 0, "none of those deadlines is due a year later");
}
}

backend_tests! {
async fn a_sleep_until_after_2262_is_not_due_on_the_next_tick() {
    let h = Harness::open().await;
    h.put(
        "far",
        json!({"steps": [
            {"kind": "sleep_until", "id": "nap", "at": "9999-12-31T23:59:59Z"},
            {"kind": "task", "id": "after"}
        ]}),
    )
    .await
    .expect("a sleep_until is rendered per run, so any timestamp is accepted");
    let run_id = h.start("far").await.expect("start");

    // An instant past 2262 has more nanoseconds than an i64 holds. Cast with
    // `as`, it wrapped to a date in the past and the sleep fired at once.
    assert_eq!(h.due(&run_id, TimerKind::Sleep).await, latest());
    let report = h.store.run_due_timers(t0() + Duration::seconds(1)).await.expect("tick");
    assert_eq!(report.fired, 0, "a sleep until the year 9999 is not due now");
    let run = h.store.get_run(&run_id).await.expect("run");
    assert_eq!(run.status, RunStatus::Running);
    assert_eq!(run.cursor.at.step(), Some("nap"), "the run is still asleep");
}
}

backend_tests! {
async fn a_retry_floor_past_the_clock_cannot_stop_the_daemon() {
    let h = Harness::open().await;
    h.put(
        "retrying",
        json!({"steps": [{"kind": "task", "id": "work", "retry": {"max_attempts": 3}}]}),
    )
    .await
    .expect("define");
    let run_id = h.start("retrying").await.expect("start");
    let lease = h.store.claim("checkout", "w-1", 1, None, t0()).await.expect("claim");
    assert_eq!(lease.len(), 1);

    // The HTTP door refuses this; the store must still not panic on it,
    // because the trait is public and the server is not its only caller.
    h.store
        .fail_step(
            &lease[0].lease_id,
            StepError {
                class: StepErrorClass::Retryable,
                message: "rate limited".into(),
                details: None,
                retry_after_ms: Some(i64::MAX),
            },
            t0(),
        )
        .await
        .expect("a retry floor of i64::MAX is written, not panicked on");
    assert_eq!(h.due(&run_id, TimerKind::Retry).await, latest());
}
}
