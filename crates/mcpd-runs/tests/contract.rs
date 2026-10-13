//! What `internal/run-contract-v1.md` promises, held by the store.
//!
//! `tests/runs.rs` holds the construct and deadline tables and the reclaim
//! window. This file holds the rest of the contract: versioning, idempotent
//! starts, the four 409 reasons, cancel, guards, retries, `on_error`, the
//! iteration constructs, subworkflow mapping, event deduplication, the event
//! log and the span outbox. Three of the tests below walk an enum with no
//! catch-all, so a value nothing writes is a compile error here and not a
//! review finding.

mod common;

use common::TestStore;
use mcpd_runs::model::{
    AttemptStatus, ConflictReason, DefinitionKind, EventOutcome, InboundEvent, RunEventKind,
    RunStatus, StepErrorClass, TimerKind,
};
use mcpd_runs::store::{
    DefinitionDocument, RunConfig, RunFilter, StartRun, StepCompletion, StepError,
};
use mcpd_runs::Error;
use serde_json::{json, Value};
use std::collections::HashSet;
use time::{Duration, OffsetDateTime};

fn t0() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("a valid instant")
}

fn at(seconds: i64) -> OffsetDateTime {
    t0() + Duration::seconds(seconds)
}

fn done(output: Value) -> StepCompletion {
    StepCompletion { output: Some(output), ..Default::default() }
}

fn failure(class: StepErrorClass, message: &str) -> StepError {
    StepError { class, message: message.into(), details: None, retry_after_ms: None }
}

fn event(id: &str, key: &str) -> InboundEvent {
    InboundEvent {
        project: "shop".into(),
        event: "approved".into(),
        correlation_key: key.into(),
        event_id: id.into(),
        id_is_derived: false,
        payload: Some(json!({ "by": "ops" })),
    }
}

fn derived(key: &str, payload: Value) -> InboundEvent {
    InboundEvent {
        project: "shop".into(),
        event: "approved".into(),
        correlation_key: key.into(),
        event_id: InboundEvent::derived_id("shop", "approved", key, Some(&payload)),
        id_is_derived: true,
        payload: Some(payload),
    }
}

struct Harness {
    store: TestStore,
}

impl Harness {
    async fn open() -> Self {
        Self { store: TestStore::open(RunConfig::default()).await }
    }

    fn document(id: &str, version: Option<u32>, body: Value) -> DefinitionDocument {
        DefinitionDocument {
            project: "shop".into(),
            id: id.into(),
            kind: DefinitionKind::Workflow,
            version,
            body,
        }
    }

    async fn define(&self, id: &str, body: Value) {
        self.store.put_definition(Self::document(id, Some(1), body)).await.expect("put definition");
    }

    async fn start(&self, id: &str, input: Value) -> String {
        self.start_at(id, input, t0()).await
    }

    async fn start_at(&self, id: &str, input: Value, now: OffsetDateTime) -> String {
        self.store
            .start_run(
                StartRun {
                    project: "shop".into(),
                    definition_id: id.into(),
                    definition_version: None,
                    input: Some(input),
                    correlation: Some(json!({ "session_id": "sess_1", "user_id": "u_7" })),
                    idempotency_key: None,
                },
                now,
            )
            .await
            .expect("start run")
            .run_id
    }

    /// One lease from one run, and the whole answer when there is none.
    async fn claim_one(&self, run_id: &str, now: OffsetDateTime) -> mcpd_runs::model::Lease {
        let mut leases =
            self.store.claim("shop", "worker-1", 1, Some(run_id), now).await.expect("claim");
        assert_eq!(leases.len(), 1, "expected one lease for run {run_id}");
        leases.remove(0)
    }

    async fn claim_all(&self, run_id: &str, now: OffsetDateTime) -> Vec<mcpd_runs::model::Lease> {
        self.store.claim("shop", "worker-1", 20, Some(run_id), now).await.expect("claim")
    }

    async fn run(&self, run_id: &str) -> mcpd_runs::model::Run {
        self.store.get_run(run_id).await.expect("run")
    }

    async fn attempt(&self, run_id: &str, step_key: &str, n: i64) -> mcpd_runs::model::Attempt {
        self.store
            .attempts(run_id)
            .await
            .expect("attempts")
            .into_iter()
            .find(|a| a.step_key == step_key && a.attempt == n)
            .unwrap_or_else(|| panic!("no attempt {n} of {step_key}"))
    }

    async fn pending(&self, run_id: &str) -> HashSet<String> {
        self.store
            .attempts(run_id)
            .await
            .expect("attempts")
            .into_iter()
            .filter(|a| a.status == AttemptStatus::Pending)
            .map(|a| a.step_key)
            .collect()
    }

    async fn event_kinds(&self, run_id: &str) -> Vec<RunEventKind> {
        self.store
            .events(run_id, -1, 1000)
            .await
            .expect("events")
            .events
            .into_iter()
            .map(|e| e.kind)
            .collect()
    }

    async fn timer_kinds(&self, run_id: &str) -> HashSet<TimerKind> {
        self.store.timers(run_id).await.expect("timers").into_iter().map(|t| t.kind).collect()
    }
}

fn reason_of(err: &Error) -> ConflictReason {
    match err {
        Error::Conflict { reason, .. } => *reason,
        other => panic!("expected a conflict, got {other}"),
    }
}

const ONE_STEP: &str = r#"{"steps": [{"kind": "task", "id": "work"}]}"#;

fn one_step() -> Value {
    serde_json::from_str(ONE_STEP).expect("json")
}

fn two_steps() -> Value {
    json!({"steps": [{"kind": "task", "id": "a"}, {"kind": "task", "id": "b"}]})
}

// ---------------------------------------------------------------------------
// Every enum value has a writer
// ---------------------------------------------------------------------------

backend_tests! {
async fn every_run_status_has_a_writer() {
    let h = Harness::open().await;
    h.define("one", one_step()).await;
    h.define(
        "slow",
        json!({"steps": [{"kind": "task", "id": "work"}], "limits": {"run_timeout_ms": 1000}}),
    )
    .await;
    for status in RunStatus::ALL {
        let run_id = match status {
            RunStatus::Running => h.start("one", json!({})).await,
            RunStatus::Succeeded => {
                let run_id = h.start("one", json!({})).await;
                let lease = h.claim_one(&run_id, t0()).await;
                h.store.complete_step(&lease.lease_id, done(json!(1)), t0()).await.expect("ok");
                run_id
            }
            RunStatus::Failed => {
                let run_id = h.start("one", json!({})).await;
                let lease = h.claim_one(&run_id, t0()).await;
                h.store
                    .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "boom"), t0())
                    .await
                    .expect("fail");
                run_id
            }
            RunStatus::TimedOut => {
                let run_id = h.start("slow", json!({})).await;
                h.store.run_due_timers(at(2)).await.expect("tick");
                run_id
            }
            RunStatus::Cancelled => {
                let run_id = h.start("one", json!({})).await;
                h.store.cancel(&run_id, t0()).await.expect("cancel");
                run_id
            }
        };
        assert_eq!(h.run(&run_id).await.status, status, "nothing writes {status:?}");
    }
}
}

backend_tests! {
async fn every_attempt_status_has_a_writer() {
    let h = Harness::open().await;
    h.define("one", one_step()).await;
    h.define(
        "bounded",
        json!({"steps": [{"kind": "task", "id": "work", "step_timeout_ms": 1000}]}),
    )
    .await;
    h.define(
        "guarded",
        json!({"steps": [
            {"kind": "task", "id": "work", "when": {"path": "input.go", "op": "eq", "value": true}}
        ]}),
    )
    .await;
    for status in AttemptStatus::ALL {
        let run_id = match status {
            AttemptStatus::Pending => h.start("one", json!({})).await,
            AttemptStatus::Leased => {
                let run_id = h.start("one", json!({})).await;
                h.claim_one(&run_id, t0()).await;
                run_id
            }
            AttemptStatus::Started => {
                let run_id = h.start("one", json!({})).await;
                let lease = h.claim_one(&run_id, t0()).await;
                h.store.started(&lease.lease_id, t0()).await.expect("started");
                run_id
            }
            AttemptStatus::Orphaned => {
                let run_id = h.start("one", json!({})).await;
                h.claim_one(&run_id, t0()).await;
                h.store.run_due_timers(at(31)).await.expect("tick");
                run_id
            }
            AttemptStatus::Succeeded => {
                let run_id = h.start("one", json!({})).await;
                let lease = h.claim_one(&run_id, t0()).await;
                h.store.complete_step(&lease.lease_id, done(json!(1)), t0()).await.expect("ok");
                run_id
            }
            AttemptStatus::Failed => {
                let run_id = h.start("one", json!({})).await;
                let lease = h.claim_one(&run_id, t0()).await;
                h.store
                    .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "boom"), t0())
                    .await
                    .expect("fail");
                run_id
            }
            AttemptStatus::TimedOut => {
                let run_id = h.start("bounded", json!({})).await;
                h.claim_one(&run_id, t0()).await;
                h.store.run_due_timers(at(2)).await.expect("tick");
                run_id
            }
            AttemptStatus::Skipped => h.start("guarded", json!({ "go": false })).await,
        };
        assert_eq!(h.attempt(&run_id, "work", 0).await.status, status, "nothing writes {status:?}");
    }
}
}

backend_tests! {
async fn every_run_event_kind_has_a_writer() {
    let h = Harness::open().await;
    h.define(
        "all",
        json!({"steps": [
            {"kind": "task", "id": "skipme", "when": {"path": "input.never", "op": "exists"}},
            {"kind": "task", "id": "work", "retry": {"max_attempts": 2}},
            {"kind": "wait_event", "id": "approval", "event": "approved",
             "key": "${input.order}", "timeout_ms": 60000}
        ]}),
    )
    .await;
    let run_id = h.start("all", json!({ "order": "o1" })).await;
    let lease = h.claim_one(&run_id, t0()).await;
    h.store.started(&lease.lease_id, t0()).await.expect("started");
    h.store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Retryable, "429"), t0())
        .await
        .expect("fail");
    h.store.run_due_timers(at(5)).await.expect("retry fires");
    let lease = h.claim_one(&run_id, at(5)).await;
    h.store.complete_step(&lease.lease_id, done(json!({})), at(5)).await.expect("complete");
    h.store.deliver_event(event("e1", "o1"), at(6)).await.expect("deliver");

    let seen: HashSet<RunEventKind> = h.event_kinds(&run_id).await.into_iter().collect();
    for kind in RunEventKind::ALL {
        assert!(seen.contains(&kind), "nothing writes {kind:?}: saw {seen:?}");
    }
    assert_eq!(h.run(&run_id).await.status, RunStatus::Succeeded);
}
}

// ---------------------------------------------------------------------------
// The four 409 reasons
// ---------------------------------------------------------------------------

backend_tests! {
async fn a_landed_complete_whose_reply_was_lost_is_already_applied() {
    let h = Harness::open().await;
    h.define("two", two_steps()).await;
    let run_id = h.start("two", json!({})).await;
    let lease = h.claim_one(&run_id, t0()).await;
    h.store.complete_step(&lease.lease_id, done(json!({ "x": 1 })), t0()).await.expect("first");

    let err = h
        .store
        .complete_step(&lease.lease_id, done(json!({ "x": 2 })), at(1))
        .await
        .expect_err("a second complete on the same lease is refused");
    assert_eq!(reason_of(&err), ConflictReason::AlreadyApplied);
    if let Error::Conflict { attempt_status, run_status, .. } = &err {
        assert_eq!(*attempt_status, Some(AttemptStatus::Succeeded));
        assert_eq!(*run_status, RunStatus::Running);
    }
    assert_eq!(h.attempt(&run_id, "a", 0).await.output, Some(json!({ "x": 1 })));
}
}

backend_tests! {
async fn a_lease_a_later_attempt_replaced_is_fenced() {
    let h = Harness::open().await;
    h.define("one", one_step()).await;
    let run_id = h.start("one", json!({})).await;
    let old = h.claim_one(&run_id, t0()).await;
    h.store.run_due_timers(at(31)).await.expect("expiry");
    h.store.run_due_timers(at(90)).await.expect("reclaim");
    assert_eq!(h.attempt(&run_id, "work", 1).await.status, AttemptStatus::Pending);

    let err = h.store.heartbeat(&old.lease_id, at(91)).await.expect_err("fenced");
    assert_eq!(reason_of(&err), ConflictReason::Fenced);
    let err =
        h.store.complete_step(&old.lease_id, done(json!(1)), at(91)).await.expect_err("fenced");
    assert_eq!(reason_of(&err), ConflictReason::Fenced);
}
}

backend_tests! {
async fn an_at_most_once_attempt_cannot_complete_before_it_started() {
    let h = Harness::open().await;
    h.define("once", json!({"steps": [{"kind": "task", "id": "work", "effect": "at_most_once"}]}))
        .await;
    let run_id = h.start("once", json!({})).await;
    let lease = h.claim_one(&run_id, t0()).await;
    let err = h
        .store
        .complete_step(&lease.lease_id, done(json!(1)), t0())
        .await
        .expect_err("not started");
    assert_eq!(reason_of(&err), ConflictReason::NotStarted);
    let err = h
        .store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "x"), t0())
        .await
        .expect_err("not started");
    assert_eq!(reason_of(&err), ConflictReason::NotStarted);

    h.store.started(&lease.lease_id, t0()).await.expect("started");
    h.store.complete_step(&lease.lease_id, done(json!(1)), t0()).await.expect("now it lands");
    assert_eq!(h.run(&run_id).await.status, RunStatus::Succeeded);
}
}

backend_tests! {
async fn every_call_on_an_ended_run_is_run_terminal() {
    let h = Harness::open().await;
    h.define("two", two_steps()).await;
    let run_id = h.start("two", json!({})).await;
    let lease = h.claim_one(&run_id, t0()).await;
    h.store.cancel(&run_id, at(1)).await.expect("cancel");

    for call in ["heartbeat", "started", "complete", "fail"] {
        let err = match call {
            "heartbeat" => h.store.heartbeat(&lease.lease_id, at(2)).await.err(),
            "started" => h.store.started(&lease.lease_id, at(2)).await.err(),
            "complete" => h.store.complete_step(&lease.lease_id, done(json!(1)), at(2)).await.err(),
            _ => h
                .store
                .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "x"), at(2))
                .await
                .err(),
        }
        .unwrap_or_else(|| panic!("{call} succeeded on a cancelled run"));
        assert_eq!(reason_of(&err), ConflictReason::RunTerminal, "{call}");
    }
    assert!(h.timer_kinds(&run_id).await.is_empty(), "a heartbeat must not revive a timer");
}
}

// ---------------------------------------------------------------------------
// Cancel
// ---------------------------------------------------------------------------

backend_tests! {
async fn cancel_walks_the_tree_and_fails_every_live_attempt() {
    let h = Harness::open().await;
    h.define(
        "child",
        json!({"steps": [
            {"kind": "parallel", "id": "fan", "branches": [
                {"kind": "task", "id": "a"}, {"kind": "task", "id": "b"}
            ]}
        ]}),
    )
    .await;
    h.define(
        "parent",
        json!({"steps": [
            {"kind": "subworkflow", "id": "sub", "definition": "child"},
            {"kind": "task", "id": "after"}
        ]}),
    )
    .await;
    let parent = h.start("parent", json!({})).await;
    let leases = h.store.claim("shop", "worker-1", 10, None, t0()).await.expect("claim");
    assert_eq!(leases.len(), 2, "both branches of the child are offered");
    let child = leases[0].run_id.clone();
    assert_ne!(child, parent);

    let cancelled = h.store.cancel(&parent, at(1)).await.expect("cancel");
    assert_eq!(cancelled.status, RunStatus::Cancelled);
    assert_eq!(h.run(&child).await.status, RunStatus::Cancelled, "cancel walks down");

    for a in h.store.attempts(&child).await.expect("attempts") {
        assert_eq!(a.status, AttemptStatus::Failed, "{}", a.step_key);
        assert_eq!(a.error_class, Some(StepErrorClass::Cancelled));
    }
    assert!(h.timer_kinds(&child).await.is_empty());
    assert!(h.store.waits(&parent).await.expect("waits").is_empty());
    assert!(h.event_kinds(&parent).await.contains(&RunEventKind::RunEnded));

    let again = h.store.cancel(&parent, at(2)).await.expect("idempotent");
    assert_eq!(again.status, RunStatus::Cancelled);
    assert_eq!(again.updated_at, cancelled.updated_at, "a second cancel changes nothing");

    let err = h.store.heartbeat(&leases[0].lease_id, at(3)).await.expect_err("terminal");
    assert_eq!(reason_of(&err), ConflictReason::RunTerminal);
}
}

backend_tests! {
async fn a_failed_run_leaves_no_live_sibling_attempt() {
    let h = Harness::open().await;
    h.define(
        "fan",
        json!({"steps": [{"kind": "parallel", "id": "fan", "branches": [
            {"kind": "task", "id": "a"}, {"kind": "task", "id": "b"}
        ]}]}),
    )
    .await;
    let run_id = h.start("fan", json!({})).await;
    let leases = h.claim_all(&run_id, t0()).await;
    assert_eq!(leases.len(), 2);
    h.store
        .fail_step(&leases[0].lease_id, failure(StepErrorClass::Fatal, "boom"), t0())
        .await
        .expect("fail");
    assert_eq!(h.run(&run_id).await.status, RunStatus::Failed);
    let b = h.attempt(&run_id, "fan/b", 0).await;
    assert_eq!(
        b.status,
        AttemptStatus::Failed,
        "the journal never shows a live attempt on a dead run"
    );
    let err = h.store.heartbeat(&leases[1].lease_id, at(1)).await.expect_err("terminal");
    assert_eq!(reason_of(&err), ConflictReason::RunTerminal);
}
}

// ---------------------------------------------------------------------------
// Starts and versions
// ---------------------------------------------------------------------------

backend_tests! {
async fn a_reposted_idempotency_key_returns_the_same_run() {
    let h = Harness::open().await;
    h.define("one", one_step()).await;
    let start = |key: &str| StartRun {
        project: "shop".into(),
        definition_id: "one".into(),
        definition_version: None,
        input: None,
        correlation: None,
        idempotency_key: Some(key.into()),
    };
    let first = h.store.start_run(start("ticket-1"), t0()).await.expect("first");
    let second = h.store.start_run(start("ticket-1"), at(1)).await.expect("second");
    assert_eq!(first.run_id, second.run_id);
    let third = h.store.start_run(start("ticket-2"), at(1)).await.expect("third");
    assert_ne!(first.run_id, third.run_id);

    // The key is released after the window, and a repost then starts a new run.
    h.store.run_due_timers(t0() + Duration::hours(25)).await.expect("sweep");
    let fourth =
        h.store.start_run(start("ticket-1"), t0() + Duration::hours(25)).await.expect("fourth");
    assert_ne!(fourth.run_id, first.run_id);
}
}

backend_tests! {
async fn two_simultaneous_posts_of_one_key_produce_one_run() {
    let h = Harness::open().await;
    let store = h.store.shared();
    store.put_definition(Harness::document("one", Some(1), one_step())).await.expect("define");
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let store = std::sync::Arc::clone(&store);
        tasks.push(tokio::spawn(async move {
            store
                .start_run(
                    StartRun {
                        project: "shop".into(),
                        definition_id: "one".into(),
                        definition_version: None,
                        input: None,
                        correlation: None,
                        idempotency_key: Some("race".into()),
                    },
                    t0(),
                )
                .await
                .expect("start")
                .run_id
        }));
    }
    let mut ids = HashSet::new();
    for t in tasks {
        ids.insert(t.await.expect("join"));
    }
    assert_eq!(ids.len(), 1, "eight posts of one key are one run");
    let page = store
        .list_runs(RunFilter { project: "shop".into(), ..Default::default() }, 50, None)
        .await
        .expect("list");
    assert_eq!(page.runs.len(), 1);
}
}

backend_tests! {
async fn definition_versions_follow_the_section_four_rule() {
    let h = Harness::open().await;
    let body_a = one_step();
    let body_b = two_steps();
    let s = &h.store;
    assert_eq!(
        s.assign_definition(Harness::document("wf", None, body_a.clone())).await.expect("a"),
        (1, true)
    );
    assert_eq!(
        s.assign_definition(Harness::document("wf", None, body_a.clone())).await.expect("same"),
        (1, false),
        "an identical body posted without a version burns nothing"
    );
    assert_eq!(
        s.assign_definition(Harness::document("wf", None, body_b.clone())).await.expect("b"),
        (2, true)
    );
    assert_eq!(
        s.assign_definition(Harness::document("wf", Some(2), body_b.clone()))
            .await
            .expect("named same"),
        (2, false)
    );
    assert_eq!(
        s.assign_definition(Harness::document("wf", Some(7), body_a.clone()))
            .await
            .expect("named free"),
        (7, false)
    );
    let err = s
        .assign_definition(Harness::document("wf", Some(2), body_a.clone()))
        .await
        .expect_err("a taken version with a different body is refused");
    assert!(matches!(err, Error::VersionTaken { version: 2, .. }), "got {err}");
    let err =
        s.put_definition(Harness::document("wf", Some(2), body_a)).await.expect_err("put too");
    assert!(matches!(err, Error::VersionTaken { .. }));

    assert_eq!(s.latest_definition("shop", "wf").await.expect("latest").version, 7);
    let all = s.list_definitions("shop").await.expect("list");
    assert_eq!(all.iter().map(|d| d.version).collect::<Vec<_>>(), vec![1, 2, 7]);

    // A version named in a body is not part of its identity.
    let mut with_version = body_b.clone();
    with_version["version"] = json!(99);
    assert_eq!(
        s.assign_definition(Harness::document("wf", None, with_version)).await.expect("v"),
        (2, false)
    );
}
}

backend_tests! {
async fn every_fixture_document_is_judged_the_way_the_schema_says() {
    let h = Harness::open().await;
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/documents");
    let mut valid = 0;
    for entry in std::fs::read_dir(root.join("valid")).expect("valid dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "json") {
            let body: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");
            let id = format!("doc_{valid}");
            h.store
                .put_definition(Harness::document(&id, None, body))
                .await
                .unwrap_or_else(|e| panic!("{} was refused: {e}", path.display()));
            valid += 1;
        }
    }
    assert!(valid >= 10, "expected the schema's example documents, found {valid}");

    let mut invalid = 0;
    for entry in std::fs::read_dir(root.join("invalid")).expect("invalid dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "json") {
            let body: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");
            let id = format!("bad_{invalid}");
            let outcome = h.store.put_definition(Harness::document(&id, None, body)).await;
            assert!(outcome.is_err(), "{} was accepted", path.display());
            invalid += 1;
        }
    }
    assert!(invalid >= 50, "expected the invalid fixtures, found {invalid}");
}
}

backend_tests! {
async fn an_input_that_fails_the_schema_is_refused_before_a_run_exists() {
    let h = Harness::open().await;
    h.define(
        "typed",
        json!({
            "input_schema": {"type": "object", "required": ["order"], "properties": {"order": {"type": "string"}}},
            "steps": [{"kind": "task", "id": "work"}]
        }),
    )
    .await;
    let err = h
        .store
        .start_run(
            StartRun {
                project: "shop".into(),
                definition_id: "typed".into(),
                definition_version: None,
                input: Some(json!({ "order": 7 })),
                correlation: None,
                idempotency_key: None,
            },
            t0(),
        )
        .await
        .expect_err("a number is not a string");
    assert!(matches!(err, Error::Invalid(_)), "got {err}");
    let page = h
        .store
        .list_runs(RunFilter { project: "shop".into(), ..Default::default() }, 10, None)
        .await
        .expect("list");
    assert!(page.runs.is_empty());
}
}

backend_tests! {
async fn runs_are_listed_newest_first_with_a_cursor_and_filters() {
    let h = Harness::open().await;
    h.define("one", one_step()).await;
    h.define("two", two_steps()).await;
    let mut ids = Vec::new();
    for i in 0..5 {
        ids.push(h.start_at(if i % 2 == 0 { "one" } else { "two" }, json!({}), at(i)).await);
    }
    let lease = h.claim_one(&ids[0], at(10)).await;
    h.store.complete_step(&lease.lease_id, done(json!(1)), at(10)).await.expect("complete");

    let page = h
        .store
        .list_runs(RunFilter { project: "shop".into(), ..Default::default() }, 2, None)
        .await
        .expect("page 1");
    assert_eq!(page.runs.len(), 2);
    assert_eq!(page.runs[0].run_id, ids[0], "the run that moved last is first");
    let rest = h
        .store
        .list_runs(
            RunFilter { project: "shop".into(), ..Default::default() },
            10,
            page.next_cursor.clone(),
        )
        .await
        .expect("page 2");
    assert_eq!(rest.runs.len(), 3);
    assert!(rest.next_cursor.is_none());
    let seen: HashSet<String> =
        page.runs.iter().chain(rest.runs.iter()).map(|r| r.run_id.clone()).collect();
    assert_eq!(seen.len(), 5, "pages do not overlap or skip");

    let done_only = h
        .store
        .list_runs(
            RunFilter {
                project: "shop".into(),
                status: Some(RunStatus::Succeeded),
                definition_id: None,
            },
            10,
            None,
        )
        .await
        .expect("filter");
    assert_eq!(done_only.runs.len(), 1);
    let twos = h
        .store
        .list_runs(
            RunFilter { project: "shop".into(), status: None, definition_id: Some("two".into()) },
            10,
            None,
        )
        .await
        .expect("filter");
    assert_eq!(twos.runs.len(), 2);
    let other = h
        .store
        .list_runs(RunFilter { project: "other".into(), ..Default::default() }, 10, None)
        .await
        .expect("other project");
    assert!(other.runs.is_empty(), "a project sees its own runs only");
}
}

// ---------------------------------------------------------------------------
// Guards, retries and on_error
// ---------------------------------------------------------------------------

backend_tests! {
async fn a_false_guard_skips_the_node_and_the_run_moves_on() {
    let h = Harness::open().await;
    h.define(
        "guarded",
        json!({"steps": [
            {"kind": "task", "id": "big", "when": {"path": "input.amount", "op": "gt", "value": 100}},
            {"kind": "task", "id": "small", "when": {"path": "input.amount", "op": "lte", "value": 100}}
        ]}),
    )
    .await;
    let run_id = h.start("guarded", json!({ "amount": 40 })).await;
    assert_eq!(h.attempt(&run_id, "big", 0).await.status, AttemptStatus::Skipped);
    assert_eq!(h.pending(&run_id).await, HashSet::from(["small".to_string()]));
    let lease = h.claim_one(&run_id, t0()).await;
    assert_eq!(lease.prior["big"]["status"], json!("skipped"), "prior carries the skip");
    h.store.complete_step(&lease.lease_id, done(json!({})), t0()).await.expect("complete");
    assert_eq!(h.run(&run_id).await.status, RunStatus::Succeeded);
}
}

backend_tests! {
async fn retries_are_spaced_by_the_policy_and_end_in_on_error() {
    let h = Harness::open().await;
    h.define(
        "flaky",
        json!({"steps": [{
            "kind": "task", "id": "call",
            "retry": {"max_attempts": 3, "backoff": "exponential", "initial_ms": 1000, "max_ms": 10000}
        }]}),
    )
    .await;
    let run_id = h.start("flaky", json!({})).await;

    let lease = h.claim_one(&run_id, t0()).await;
    let out = h
        .store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Retryable, "429"), t0())
        .await
        .expect("fail 1");
    assert_eq!(out.attempts_left, 2);
    let retry = h.store.timers(&run_id).await.expect("timers");
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].kind, TimerKind::Retry);
    assert_eq!(retry[0].due_at, t0() + Duration::milliseconds(1000));
    assert!(
        h.claim_all(&run_id, t0()).await.is_empty(),
        "nothing is claimable while the backoff runs"
    );

    h.store.run_due_timers(at(1)).await.expect("retry fires");
    let lease = h.claim_one(&run_id, at(1)).await;
    assert_eq!(lease.attempt, 1);
    let out = h
        .store
        .fail_step(
            &lease.lease_id,
            StepError {
                class: StepErrorClass::Retryable,
                message: "429".into(),
                details: None,
                retry_after_ms: Some(30_000),
            },
            at(1),
        )
        .await
        .expect("fail 2");
    assert_eq!(out.attempts_left, 1);
    let retry = h.store.timers(&run_id).await.expect("timers");
    assert_eq!(
        retry[0].due_at,
        at(1) + Duration::milliseconds(30_000),
        "a retry_after larger than the backoff wins"
    );

    h.store.run_due_timers(at(40)).await.expect("retry fires");
    let lease = h.claim_one(&run_id, at(40)).await;
    assert_eq!(lease.attempt, 2);
    let out = h
        .store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Retryable, "429"), at(40))
        .await
        .expect("fail 3");
    assert_eq!(out.attempts_left, 0);
    assert_eq!(out.run_status, RunStatus::Failed, "the budget is spent and on_error is fail_run");
}
}

backend_tests! {
async fn a_fatal_failure_skips_the_retries_and_an_unservable_one_spends_none() {
    let h = Harness::open().await;
    h.define(
        "flaky",
        json!({"steps": [{"kind": "task", "id": "call", "retry": {"max_attempts": 3}}]}),
    )
    .await;
    let run_id = h.start("flaky", json!({})).await;
    let lease = h.claim_one(&run_id, t0()).await;
    let out = h
        .store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Unservable, "not mine"), t0())
        .await
        .expect("unservable");
    assert_eq!(out.attempts_left, 3, "unservable spends nothing");
    assert_eq!(out.run_status, RunStatus::Running);
    let retry = h.store.timers(&run_id).await.expect("timers");
    assert_eq!(retry[0].due_at, at(30), "back after one lease length");

    h.store.run_due_timers(at(31)).await.expect("requeue");
    let lease = h.claim_one(&run_id, at(31)).await;
    let out = h
        .store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "never"), at(31))
        .await
        .expect("fatal");
    assert_eq!(out.run_status, RunStatus::Failed, "fatal does not retry");
    assert!(h.run(&run_id).await.error.as_deref().is_some_and(|e| e.contains("never")));
}
}

backend_tests! {
async fn on_error_continue_lets_a_later_guard_read_the_failure() {
    let h = Harness::open().await;
    h.define(
        "escalating",
        json!({"steps": [
            {"kind": "task", "id": "refund", "on_error": "continue"},
            {"kind": "task", "id": "escalate",
             "when": {"path": "steps.refund.status", "op": "eq", "value": "failed"},
             "input": {"why": "${steps.refund.error.message}"}},
            {"kind": "task", "id": "thank",
             "when": {"path": "steps.refund.status", "op": "eq", "value": "succeeded"}}
        ]}),
    )
    .await;
    let run_id = h.start("escalating", json!({})).await;
    let lease = h.claim_one(&run_id, t0()).await;
    let out = h
        .store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "card declined"), t0())
        .await
        .expect("fail");
    assert_eq!(out.run_status, RunStatus::Running, "continue does not end the run");
    assert_eq!(h.pending(&run_id).await, HashSet::from(["escalate".to_string()]));
    let lease = h.claim_one(&run_id, t0()).await;
    assert_eq!(lease.input, json!({ "why": "card declined" }));
    assert_eq!(lease.prior["refund"]["error"]["class"], json!("fatal"));
    h.store.complete_step(&lease.lease_id, done(json!({})), t0()).await.expect("complete");
    assert_eq!(h.attempt(&run_id, "thank", 0).await.status, AttemptStatus::Skipped);
    assert_eq!(h.run(&run_id).await.status, RunStatus::Succeeded);
}
}

// ---------------------------------------------------------------------------
// The iteration constructs
// ---------------------------------------------------------------------------

backend_tests! {
async fn foreach_renders_each_item_bounds_concurrency_and_collects_outputs() {
    let h = Harness::open().await;
    h.define(
        "mailer",
        json!({"steps": [
            {"kind": "foreach", "id": "send", "over": "input.people", "concurrency": 1, "max_items": 10,
             "body": {"kind": "task", "id": "mail", "input": {"to": "${item.email}", "n": "${index}"}}},
            {"kind": "task", "id": "report", "input": {"sent": "${steps.send.outputs}"}}
        ]}),
    )
    .await;
    let run_id =
        h.start("mailer", json!({ "people": [{ "email": "a@x" }, { "email": "b@x" }] })).await;
    assert_eq!(
        h.pending(&run_id).await,
        HashSet::from(["send[0]".to_string(), "send[1]".to_string()])
    );

    let first = h.claim_all(&run_id, t0()).await;
    assert_eq!(first.len(), 1, "concurrency 1 offers one item at a time");
    assert_eq!(first[0].step_key, "send[0]");
    assert_eq!(first[0].input, json!({ "to": "a@x", "n": 0 }));
    assert!(h.claim_all(&run_id, t0()).await.is_empty());
    h.store.complete_step(&first[0].lease_id, done(json!("sent a")), t0()).await.expect("a");

    let second = h.claim_one(&run_id, t0()).await;
    assert_eq!(second.input, json!({ "to": "b@x", "n": 1 }));
    h.store.complete_step(&second.lease_id, done(json!("sent b")), t0()).await.expect("b");

    let report = h.claim_one(&run_id, t0()).await;
    assert_eq!(report.step_key, "report");
    assert_eq!(report.input, json!({ "sent": ["sent a", "sent b"] }));
}
}

backend_tests! {
async fn a_foreach_over_too_many_items_or_a_non_array_fails_the_run() {
    let h = Harness::open().await;
    h.define(
        "tiny",
        json!({"steps": [
            {"kind": "foreach", "id": "each", "over": "input.items", "max_items": 1,
             "body": {"kind": "task", "id": "one"}}
        ]}),
    )
    .await;
    let run_id = h.start("tiny", json!({ "items": [1, 2] })).await;
    let run = h.run(&run_id).await;
    assert_eq!(run.status, RunStatus::Failed);
    assert!(run.error.as_deref().is_some_and(|e| e.contains("max_items")), "got {:?}", run.error);
    let run_id = h.start("tiny", json!({ "items": "nope" })).await;
    let run = h.run(&run_id).await;
    assert_eq!(run.status, RunStatus::Failed);
    assert!(
        run.error.as_deref().is_some_and(|e| e.contains("rather than an array")),
        "got {:?}",
        run.error
    );
}
}

backend_tests! {
async fn a_loop_stops_when_until_says_so_and_collects_its_outputs() {
    let h = Harness::open().await;
    h.define(
        "poller",
        json!({"steps": [
            {"kind": "loop", "id": "poll", "max_iterations": 5,
             "until": {"path": "steps.check.output.ready", "op": "eq", "value": true},
             "body": {"kind": "task", "id": "check", "input": {"n": "${iter.index}"}}},
            {"kind": "task", "id": "after", "input": {"seen": "${steps.poll.outputs}"}}
        ]}),
    )
    .await;
    let run_id = h.start("poller", json!({})).await;
    for i in 0..3 {
        let lease = h.claim_one(&run_id, t0()).await;
        assert_eq!(lease.step_key, format!("poll[{i}]/check"));
        assert_eq!(lease.input, json!({ "n": i }));
        h.store
            .complete_step(&lease.lease_id, done(json!({ "ready": i == 2 })), t0())
            .await
            .expect("complete");
    }
    let after = h.claim_one(&run_id, t0()).await;
    assert_eq!(after.step_key, "after", "the loop stopped at the third iteration");
    assert_eq!(
        after.input,
        json!({ "seen": [{ "ready": false }, { "ready": false }, { "ready": true }] })
    );
}
}

// ---------------------------------------------------------------------------
// Subworkflows
// ---------------------------------------------------------------------------

backend_tests! {
async fn a_subworkflow_maps_its_input_and_output_and_pins_the_latest_version() {
    let h = Harness::open().await;
    h.define(
        "settle",
        json!({"steps": [{"kind": "task", "id": "pay"}], "output": {"paid": "${steps.pay.output.amount}"}}),
    )
    .await;
    h.store
        .assign_definition(Harness::document(
            "settle",
            None,
            json!({"steps": [{"kind": "task", "id": "pay", "input": {"amount": "${input.amount}"}}],
                   "output": {"paid": "${steps.pay.output.amount}", "v": 2}}),
        ))
        .await
        .expect("version 2");
    h.define(
        "order",
        json!({"steps": [
            {"kind": "subworkflow", "id": "settle", "definition": "settle",
             "input": {"amount": "${input.total}"}},
            {"kind": "task", "id": "notify", "input": {"paid": "${steps.settle.output.paid}"}}
        ]}),
    )
    .await;
    let parent = h.start("order", json!({ "total": 42 })).await;
    let bindings = h.run(&parent).await.bindings;
    assert_eq!(bindings["settle"]["version"], json!(2), "absent version pins the latest at start");

    let lease = h.store.claim("shop", "worker-1", 1, None, t0()).await.expect("claim").remove(0);
    assert_ne!(lease.run_id, parent);
    assert_eq!(lease.definition_version, 2);
    assert_eq!(
        lease.input,
        json!({ "amount": 42 }),
        "the child's input is rendered from the parent"
    );
    assert_eq!(h.run(&lease.run_id).await.trace_id, h.run(&parent).await.trace_id, "one trace");
    h.store.complete_step(&lease.lease_id, done(json!({ "amount": 42 })), t0()).await.expect("pay");

    let child = h.run(&lease.run_id).await;
    assert_eq!(child.status, RunStatus::Succeeded);
    assert_eq!(child.output, Some(json!({ "paid": 42, "v": 2 })));
    let notify = h.claim_one(&parent, t0()).await;
    assert_eq!(
        notify.input,
        json!({ "paid": 42 }),
        "the child's output is the parent step's output"
    );
}
}

backend_tests! {
async fn a_child_that_fails_fails_the_parent_step_which_follows_its_on_error() {
    let h = Harness::open().await;
    h.define("child", one_step()).await;
    h.define(
        "strict",
        json!({"steps": [{"kind": "subworkflow", "id": "sub", "definition": "child"}]}),
    )
    .await;
    h.define(
        "lenient",
        json!({"steps": [
            {"kind": "subworkflow", "id": "sub", "definition": "child", "on_error": "continue"},
            {"kind": "task", "id": "after",
             "when": {"path": "steps.sub.status", "op": "eq", "value": "failed"}}
        ]}),
    )
    .await;
    for (def, expect) in [("strict", RunStatus::Failed), ("lenient", RunStatus::Running)] {
        let parent = h.start(def, json!({})).await;
        let lease =
            h.store.claim("shop", "worker-1", 1, None, t0()).await.expect("claim").remove(0);
        h.store
            .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "no"), t0())
            .await
            .expect("fail");
        let run = h.run(&parent).await;
        assert_eq!(run.status, expect, "{def}");
        if expect == RunStatus::Running {
            assert_eq!(h.pending(&parent).await, HashSet::from(["after".to_string()]));
            let after = h.claim_one(&parent, t0()).await;
            assert_eq!(after.prior["sub"]["status"], json!("failed"));
            assert_eq!(after.prior["sub"]["error"]["class"], json!("fatal"));
        } else {
            assert!(
                run.error.as_deref().is_some_and(|e| e.contains("child run")),
                "got {:?}",
                run.error
            );
        }
    }
}
}

backend_tests! {
async fn a_childs_own_limits_apply_to_it_and_the_roots_cost_ceiling_ends_the_tree() {
    let h = Harness::open().await;
    h.define("two", json!({"steps": [{"kind": "task", "id": "a"}, {"kind": "task", "id": "b"}], "limits": {"max_steps": 1}}))
        .await;
    let run_id = h.start("two", json!({})).await;
    let lease = h.claim_one(&run_id, t0()).await;
    h.store.complete_step(&lease.lease_id, done(json!(1)), t0()).await.expect("a");
    assert!(h.claim_all(&run_id, t0()).await.is_empty(), "max_steps 1 refuses the second lease");
    let run = h.run(&run_id).await;
    assert_eq!(run.status, RunStatus::Failed);
    assert!(run.error.as_deref().is_some_and(|e| e.contains("max_steps")));

    h.define(
        "pricey",
        json!({"steps": [{"kind": "task", "id": "a"}, {"kind": "task", "id": "b"}, {"kind": "task", "id": "c"}],
               "limits": {"max_cost_usd": 1.0}}),
    )
    .await;
    let run_id = h.start("pricey", json!({})).await;
    let lease = h.claim_one(&run_id, t0()).await;
    h.store
        .complete_step(
            &lease.lease_id,
            StepCompletion {
                cost_usd: Some(0.6),
                usage: Some(json!({ "input_tokens": 10, "model": "m" })),
                ..Default::default()
            },
            t0(),
        )
        .await
        .expect("a");
    let lease = h.claim_one(&run_id, t0()).await;
    assert_eq!(lease.step_key, "b", "0.6 is under 1.0");
    h.store
        .complete_step(
            &lease.lease_id,
            StepCompletion { cost_usd: Some(0.6), ..Default::default() },
            t0(),
        )
        .await
        .expect("b");
    assert!(h.claim_all(&run_id, t0()).await.is_empty(), "1.2 is over the ceiling");
    let run = h.run(&run_id).await;
    assert_eq!(run.status, RunStatus::Failed);
    assert!(run.error.as_deref().is_some_and(|e| e.contains("max_cost_usd")));
    assert!((run.cost_usd - 1.2).abs() < 1e-9);
    assert_eq!(h.attempt(&run_id, "a", 0).await.cost_usd, Some(0.6));
}
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

backend_tests! {
async fn a_supplied_event_id_deduplicates_and_a_derived_one_deduplicates_parking_only() {
    let h = Harness::open().await;
    h.define(
        "approver",
        json!({"steps": [{"kind": "wait_event", "id": "approval", "event": "approved",
                          "key": "${input.order}", "timeout_ms": 86_400_000}]}),
    )
    .await;
    let first = h.start("approver", json!({ "order": "o1" })).await;
    let second = h.start("approver", json!({ "order": "o1" })).await;

    // Two identical derived events are two approvals while two runs wait.
    let d1 = h.store.deliver_event(derived("o1", json!({ "ok": true })), at(1)).await.expect("d1");
    let d2 = h.store.deliver_event(derived("o1", json!({ "ok": true })), at(2)).await.expect("d2");
    assert_eq!(d1.outcome, EventOutcome::Delivered);
    assert_eq!(
        d2.outcome,
        EventOutcome::Delivered,
        "a derived id must not swallow a second run's approval"
    );
    assert_eq!(h.run(&first).await.status, RunStatus::Succeeded);
    assert_eq!(h.run(&second).await.status, RunStatus::Succeeded);

    // With nobody waiting, the same derived bytes park once and repeat as a duplicate.
    let p1 = h.store.deliver_event(derived("o9", json!({ "ok": true })), at(3)).await.expect("p1");
    let p2 = h.store.deliver_event(derived("o9", json!({ "ok": true })), at(4)).await.expect("p2");
    assert_eq!(p1.outcome, EventOutcome::Parked);
    assert_eq!(p2.outcome, EventOutcome::Duplicate);

    // A supplied id is honoured on both paths.
    let third = h.start("approver", json!({ "order": "o2" })).await;
    let fourth = h.start("approver", json!({ "order": "o2" })).await;
    assert_eq!(
        h.store.deliver_event(event("evt-1", "o2"), at(5)).await.expect("s1").outcome,
        EventOutcome::Delivered
    );
    assert_eq!(
        h.store.deliver_event(event("evt-1", "o2"), at(6)).await.expect("s2").outcome,
        EventOutcome::Duplicate,
        "the provider's retry is not a second approval"
    );
    let ended = [h.run(&third).await.status, h.run(&fourth).await.status];
    assert!(ended.contains(&RunStatus::Succeeded) && ended.contains(&RunStatus::Running));

    // Parked events past their expiry are swept.
    let swept = h.store.run_due_timers(t0() + Duration::hours(25)).await.expect("sweep");
    assert!(swept.parked_events_deleted >= 1);
}
}

backend_tests! {
async fn a_signal_addresses_one_run_through_its_own_wait() {
    let h = Harness::open().await;
    h.define(
        "approver",
        json!({"steps": [{"kind": "wait_event", "id": "approval", "event": "approved",
                          "key": "${input.order}", "timeout_ms": 86_400_000}]}),
    )
    .await;
    let run_id = h.start("approver", json!({ "order": "o1" })).await;
    let other = h.start("approver", json!({ "order": "o1" })).await;
    let missing =
        h.store.signal(&run_id, "declined", None, None, at(1)).await.expect_err("no such wait");
    assert!(matches!(missing, Error::NotFound { kind: "wait", .. }), "got {missing}");

    let out = h
        .store
        .signal(&other, "approved", Some(json!({ "by": "me" })), None, at(1))
        .await
        .expect("signal");
    assert_eq!(out.outcome, EventOutcome::Delivered);
    assert_eq!(h.run(&other).await.status, RunStatus::Succeeded, "the named run took it");
    assert_eq!(h.run(&run_id).await.status, RunStatus::Running, "not the one expiring soonest");
    assert_eq!(h.attempt(&other, "approval", 0).await.output, Some(json!({ "by": "me" })));
}
}

backend_tests! {
async fn a_wait_that_expires_fails_the_run_by_default() {
    let h = Harness::open().await;
    h.define(
        "approver",
        json!({"steps": [{"kind": "wait_event", "id": "approval", "event": "approved",
                          "key": "k", "timeout_ms": 1000}]}),
    )
    .await;
    let run_id = h.start("approver", json!({})).await;
    h.store.run_due_timers(at(2)).await.expect("tick");
    let run = h.run(&run_id).await;
    assert_eq!(run.status, RunStatus::Failed);
    assert!(
        run.error.as_deref().is_some_and(|e| e.contains("timed out waiting")),
        "got {:?}",
        run.error
    );
    assert_eq!(h.attempt(&run_id, "approval", 0).await.status, AttemptStatus::TimedOut);
}
}

// ---------------------------------------------------------------------------
// State, correlation, sleep_until, and the outbox
// ---------------------------------------------------------------------------

backend_tests! {
async fn state_merges_shallowly_and_reaches_the_next_lease() {
    let h = Harness::open().await;
    h.define("two", two_steps()).await;
    let run_id = h.start("two", json!({})).await;
    let lease = h.claim_one(&run_id, t0()).await;
    assert_eq!(lease.state, json!({}));
    h.store
        .complete_step(
            &lease.lease_id,
            StepCompletion {
                state: Some(json!({ "a": 1, "nested": { "x": 1 } })),
                ..Default::default()
            },
            t0(),
        )
        .await
        .expect("a");
    let lease = h.claim_one(&run_id, t0()).await;
    assert_eq!(lease.state, json!({ "a": 1, "nested": { "x": 1 } }));
    assert_eq!(lease.correlation["session_id"], json!("sess_1"));
    assert_eq!(lease.correlation["run_id"], json!(run_id));
    h.store
        .complete_step(
            &lease.lease_id,
            StepCompletion {
                state: Some(json!({ "nested": { "y": 2 }, "b": 2 })),
                ..Default::default()
            },
            t0(),
        )
        .await
        .expect("b");
    assert_eq!(
        h.run(&run_id).await.state,
        json!({ "a": 1, "b": 2, "nested": { "y": 2 } }),
        "top-level keys replace; nothing merges deeper"
    );
    let err = h
        .store
        .complete_step(
            &lease.lease_id,
            StepCompletion { state: Some(json!(3)), ..Default::default() },
            at(1),
        )
        .await;
    assert!(err.is_err());
}
}

backend_tests! {
async fn sleep_until_renders_its_instant_from_the_input() {
    let h = Harness::open().await;
    h.define(
        "billing",
        json!({"steps": [
            {"kind": "sleep_until", "id": "day", "at": "${input.when}"},
            {"kind": "task", "id": "bill"}
        ]}),
    )
    .await;
    let run_id = h.start("billing", json!({ "when": "2023-11-14T22:14:20Z" })).await;
    let timers = h.store.timers(&run_id).await.expect("timers");
    assert_eq!(timers[0].kind, TimerKind::Sleep);
    assert_eq!(timers[0].due_at, at(60));
    let bad = h.start("billing", json!({ "when": "tomorrow" })).await;
    let run = h.run(&bad).await;
    assert_eq!(run.status, RunStatus::Failed);
    assert!(run.error.as_deref().is_some_and(|e| e.contains("RFC 3339")), "got {:?}", run.error);
}
}

backend_tests! {
async fn a_whole_run_leaves_its_spans_in_the_outbox_in_order() {
    let h = Harness::open().await;
    h.define(
        "traced",
        json!({"steps": [
            {"kind": "task", "id": "work"},
            {"kind": "sleep", "id": "nap", "ms": 1000},
            {"kind": "task", "id": "skipped", "when": {"path": "input.x", "op": "exists"}}
        ]}),
    )
    .await;
    let run_id = h.start("traced", json!({})).await;
    let lease = h.claim_one(&run_id, t0()).await;
    h.store.started(&lease.lease_id, at(1)).await.expect("started");
    h.store
        .complete_step(
            &lease.lease_id,
            StepCompletion {
                output: Some(json!({ "ok": true })),
                usage: Some(json!({ "input_tokens": 5, "output_tokens": 2, "model": "m1", "provider": "p1" })),
                cost_usd: Some(0.01),
                ..Default::default()
            },
            at(2),
        )
        .await
        .expect("complete");
    h.store.run_due_timers(at(4)).await.expect("sleep fires");
    assert_eq!(h.run(&run_id).await.status, RunStatus::Succeeded);

    let rows = h.store.claim_outbox(100, at(5)).await.expect("claim outbox");
    let keys: Vec<String> = rows.iter().map(|r| r.ingest_key.clone()).collect();
    assert_eq!(
        keys,
        vec![
            format!("run:{run_id}:root:open"),
            format!("run:{run_id}:work:0:open"),
            format!("run:{run_id}:work:0:close"),
            format!("run:{run_id}:nap:0:close"),
            format!("run:{run_id}:skipped:0:close"),
            format!("run:{run_id}:root:close"),
        ]
    );
    let run = h.run(&run_id).await;
    let open = &rows[1].payload;
    let close = &rows[2].payload;
    for span in [open, close] {
        assert_eq!(span["trace_id"], json!(run.trace_id));
        assert_eq!(span["span_id"], json!(lease.span_id));
        assert_eq!(span["parent_span_id"], json!(run.root_span_id));
        assert_eq!(span["kind"], json!("workflow_step"));
        assert_eq!(span["correlation"]["run_id"], json!(run_id));
        assert_eq!(span["correlation"]["project"], json!("shop"));
    }
    assert_eq!(
        open["start_time"], close["start_time"],
        "the close replaces the open on every backend"
    );
    assert!(open.get("end_time").is_none());
    assert_eq!(close["status"], json!("ok"));
    // The priced usage rides in the attributes. A step span is a container:
    // the model spans the body recorded under it carry the tokens, and a
    // trace total must count them once.
    assert_eq!(close["attributes"]["usage"]["input_tokens"], json!(5));
    assert_eq!(close["attributes"]["usage"]["model"], json!("m1"));
    assert_eq!(close["attributes"]["cost_usd"], json!(0.01));
    assert!(close.get("usage").is_none(), "no usage on the container span");
    assert!(close.get("cost").is_none(), "no cost on the container span");
    assert!(close.get("model").is_none());
    assert_eq!(rows[3].payload["kind"], json!("workflow_sleep"));
    assert_eq!(rows[4].payload["kind"], json!("workflow_conditional_eval"));
    assert_eq!(rows[5].payload["is_root"], json!(true));
    assert_eq!(rows[5].payload["status"], json!("ok"));

    // A claimed row is not offered again until its claim lapses; an acked row is gone.
    assert!(h.store.claim_outbox(100, at(6)).await.expect("again").is_empty());
    let ids: Vec<String> = rows.iter().map(|r| r.outbox_id.clone()).collect();
    h.store.release_outbox(&ids[..1]).await.expect("release");
    let back = h.store.claim_outbox(100, at(7)).await.expect("released");
    assert_eq!(back.len(), 1);
    h.store.ack_outbox(&ids).await.expect("ack");
    assert!(h.store.claim_outbox(100, t0() + Duration::hours(1)).await.expect("empty").is_empty());
}
}

backend_tests! {
async fn a_lease_carries_everything_a_worker_needs() {
    let h = Harness::open().await;
    h.define(
        "shape",
        json!({"steps": [{"kind": "agent", "id": "reply", "effect": "at_most_once", "step_timeout_ms": 5000,
                          "input": {"q": "${input.question}"}}]}),
    )
    .await;
    let run_id = h.start("shape", json!({ "question": "where is it" })).await;
    let lease = h.claim_one(&run_id, t0()).await;
    let run = h.run(&run_id).await;
    assert_eq!(lease.project, "shop");
    assert_eq!(lease.definition_id, "shape");
    assert_eq!(lease.definition_version, 1);
    assert_eq!(lease.kind, mcpd_runs::model::StepKind::Agent);
    assert_eq!(lease.effect, mcpd_runs::model::Effect::AtMostOnce);
    assert_eq!(lease.deadline_ms, Some(5000));
    assert_eq!(lease.expires_at, at(30));
    assert_eq!(lease.input, json!({ "q": "where is it" }));
    assert_eq!(lease.idempotency_key.len(), 64);
    assert_eq!(lease.trace_id, run.trace_id);
    assert_eq!(lease.parent_span_id, run.root_span_id);
    assert_eq!(lease.span_id.len(), 16);
    assert_eq!(lease.attempt, 0);
}
}
