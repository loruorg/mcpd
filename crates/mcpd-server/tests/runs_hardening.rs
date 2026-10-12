//! The run surface under conditions that used to stop the daemon or tie it up.
//!
//! A definition with an enormous duration, a worker that sends an enormous
//! retry floor, an idle worker long-polling for work, a long-poll longer than
//! the request timeout, and a long-poll in flight when the daemon is told to
//! stop. Every request goes through the real router.

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use mcpd_runs::{
    Attempt, CompleteOutcome, Definition, DefinitionDocument, Delivery, EventPage, FailOutcome,
    InboundEvent, Lease, LeaseAck, NodeAttempts, OutboxRow, ReconcileReport, Run, RunFilter,
    RunPage, RunRetention, RunStore, SqliteRunStore, StartRun, StepCause, StepCompletion,
    StepError, SubjectErasure, Timer, TimerReport, Wait,
};
use mcpd_server::{router, AppState, ServerConfig};
use mcpd_store::sqlite::SqliteStore;
use mcpd_store::Store;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::sync::Notify;
use tower::ServiceExt;

fn temp_dir() -> std::path::PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    static STAMP: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let stamp = STAMP.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    });
    let dir = std::env::temp_dir().join(format!(
        "mcpd-runs-hardening-{}-{stamp}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("create the test directory");
    dir
}

/// The real run store, with a count of the claim transactions it was asked
/// for, and a way to hold the outbox drain between its claim and its write.
/// Everything else passes straight through.
struct Counting {
    inner: SqliteRunStore,
    claims: AtomicU64,
    /// Set to hold the next outbox claim that returns rows, once.
    hold_outbox: AtomicBool,
    /// Notified when a held claim has its rows in hand.
    outbox_claimed: Notify,
    /// Notified to let a held claim return its rows.
    outbox_release: Notify,
    /// Set to panic in the next deadline pass, once.
    panic_timers: AtomicBool,
    /// How many deadline passes were asked for.
    timer_passes: AtomicU64,
    /// Set to panic in the next outbox claim, once.
    panic_outbox: AtomicBool,
    /// How many outbox claims were asked for.
    outbox_claims: AtomicU64,
}

#[async_trait]
impl RunStore for Counting {
    async fn migrate(&self) -> mcpd_runs::Result<()> {
        self.inner.migrate().await
    }
    fn notify(&self) -> Arc<Notify> {
        self.inner.notify()
    }
    fn runnable(&self) -> Arc<Notify> {
        self.inner.runnable()
    }
    async fn put_definition(&self, def: DefinitionDocument) -> mcpd_runs::Result<()> {
        self.inner.put_definition(def).await
    }
    async fn assign_definition(&self, def: DefinitionDocument) -> mcpd_runs::Result<(u32, bool)> {
        self.inner.assign_definition(def).await
    }
    async fn get_definition(
        &self,
        project: &str,
        id: &str,
        version: u32,
    ) -> mcpd_runs::Result<Definition> {
        self.inner.get_definition(project, id, version).await
    }
    async fn latest_definition(&self, project: &str, id: &str) -> mcpd_runs::Result<Definition> {
        self.inner.latest_definition(project, id).await
    }
    async fn list_definitions(&self, project: &str) -> mcpd_runs::Result<Vec<Definition>> {
        self.inner.list_definitions(project).await
    }
    async fn start_run(&self, req: StartRun, now: OffsetDateTime) -> mcpd_runs::Result<Run> {
        self.inner.start_run(req, now).await
    }
    async fn get_run(&self, run_id: &str) -> mcpd_runs::Result<Run> {
        self.inner.get_run(run_id).await
    }
    async fn list_runs(
        &self,
        filter: RunFilter,
        limit: usize,
        cursor: Option<String>,
    ) -> mcpd_runs::Result<RunPage> {
        self.inner.list_runs(filter, limit, cursor).await
    }
    async fn claim(
        &self,
        project: &str,
        owner: &str,
        limit: usize,
        run_id: Option<&str>,
        now: OffsetDateTime,
    ) -> mcpd_runs::Result<Vec<Lease>> {
        self.claims.fetch_add(1, Ordering::Relaxed);
        self.inner.claim(project, owner, limit, run_id, now).await
    }
    async fn started(&self, lease_id: &str, now: OffsetDateTime) -> mcpd_runs::Result<LeaseAck> {
        self.inner.started(lease_id, now).await
    }
    async fn heartbeat(&self, lease_id: &str, now: OffsetDateTime) -> mcpd_runs::Result<LeaseAck> {
        self.inner.heartbeat(lease_id, now).await
    }
    async fn complete_step(
        &self,
        lease_id: &str,
        completion: StepCompletion,
        now: OffsetDateTime,
    ) -> mcpd_runs::Result<CompleteOutcome> {
        self.inner.complete_step(lease_id, completion, now).await
    }
    async fn fail_step(
        &self,
        lease_id: &str,
        error: StepError,
        now: OffsetDateTime,
    ) -> mcpd_runs::Result<FailOutcome> {
        self.inner.fail_step(lease_id, error, now).await
    }
    async fn cancel(&self, run_id: &str, now: OffsetDateTime) -> mcpd_runs::Result<Run> {
        self.inner.cancel(run_id, now).await
    }
    async fn run_due_timers(&self, now: OffsetDateTime) -> mcpd_runs::Result<TimerReport> {
        self.timer_passes.fetch_add(1, Ordering::SeqCst);
        assert!(!self.panic_timers.swap(false, Ordering::SeqCst), "a deadline pass panicked");
        self.inner.run_due_timers(now).await
    }
    async fn reconcile(&self, now: OffsetDateTime) -> mcpd_runs::Result<ReconcileReport> {
        self.inner.reconcile(now).await
    }
    async fn deliver_event(
        &self,
        event: InboundEvent,
        now: OffsetDateTime,
    ) -> mcpd_runs::Result<Delivery> {
        self.inner.deliver_event(event, now).await
    }
    async fn signal(
        &self,
        run_id: &str,
        event: &str,
        payload: Option<Value>,
        event_id: Option<String>,
        now: OffsetDateTime,
    ) -> mcpd_runs::Result<Delivery> {
        self.inner.signal(run_id, event, payload, event_id, now).await
    }
    async fn lease_run(&self, lease_id: &str) -> mcpd_runs::Result<Run> {
        self.inner.lease_run(lease_id).await
    }
    async fn attempts(&self, run_id: &str) -> mcpd_runs::Result<Vec<Attempt>> {
        self.inner.attempts(run_id).await
    }
    async fn node_attempts(&self, run_ids: &[String]) -> mcpd_runs::Result<Vec<NodeAttempts>> {
        self.inner.node_attempts(run_ids).await
    }
    async fn step_cause(
        &self,
        run_id: &str,
        step_key: &str,
        max_depth: u32,
    ) -> mcpd_runs::Result<StepCause> {
        self.inner.step_cause(run_id, step_key, max_depth).await
    }
    async fn timers(&self, run_id: &str) -> mcpd_runs::Result<Vec<Timer>> {
        self.inner.timers(run_id).await
    }
    async fn waits(&self, run_id: &str) -> mcpd_runs::Result<Vec<Wait>> {
        self.inner.waits(run_id).await
    }
    async fn events(&self, run_id: &str, after: i64, limit: usize) -> mcpd_runs::Result<EventPage> {
        self.inner.events(run_id, after, limit).await
    }
    async fn claim_outbox(
        &self,
        limit: usize,
        now: OffsetDateTime,
    ) -> mcpd_runs::Result<Vec<OutboxRow>> {
        self.outbox_claims.fetch_add(1, Ordering::SeqCst);
        assert!(!self.panic_outbox.swap(false, Ordering::SeqCst), "an outbox pass panicked");
        let rows = self.inner.claim_outbox(limit, now).await?;
        if !rows.is_empty() && self.hold_outbox.swap(false, Ordering::SeqCst) {
            self.outbox_claimed.notify_one();
            self.outbox_release.notified().await;
        }
        Ok(rows)
    }
    async fn ack_outbox(&self, outbox_ids: &[String]) -> mcpd_runs::Result<()> {
        self.inner.ack_outbox(outbox_ids).await
    }
    async fn release_outbox(&self, outbox_ids: &[String]) -> mcpd_runs::Result<()> {
        self.inner.release_outbox(outbox_ids).await
    }
    async fn erase_subject(
        &self,
        project: &str,
        user_id: &str,
    ) -> mcpd_runs::Result<SubjectErasure> {
        self.inner.erase_subject(project, user_id).await
    }
    async fn apply_retention(
        &self,
        project: &str,
        cutoff: OffsetDateTime,
    ) -> mcpd_runs::Result<RunRetention> {
        self.inner.apply_retention(project, cutoff).await
    }
}

/// An open daemon with a counting run store, over private files.
async fn app(config: ServerConfig) -> (Router, Arc<AppState>, Arc<Counting>) {
    let dir = temp_dir();
    let store = SqliteStore::open(dir.join("signals.db"), 2).expect("open the store");
    store.migrate().await.expect("migrate the store");
    let inner = SqliteRunStore::open(dir.join("runs.db"), config.run_config).expect("open runs");
    inner.migrate().await.expect("migrate runs");
    let runs = Arc::new(Counting {
        inner,
        claims: AtomicU64::new(0),
        hold_outbox: AtomicBool::new(false),
        outbox_claimed: Notify::new(),
        outbox_release: Notify::new(),
        panic_timers: AtomicBool::new(false),
        timer_passes: AtomicU64::new(0),
        panic_outbox: AtomicBool::new(false),
        outbox_claims: AtomicU64::new(0),
    });
    let state = Arc::new(
        AppState::new(Arc::new(store) as Arc<dyn Store>, config)
            .with_run_store(Arc::clone(&runs) as Arc<dyn RunStore>),
    );
    (router(Arc::clone(&state)), state, runs)
}

async fn call(app: &Router, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let req = Request::builder().method(method).uri(path);
    let req = match body {
        Some(b) => req.header("content-type", "application/json").body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .expect("build the request");
    let res = app.clone().oneshot(req).await.expect("route the request");
    let status = res.status();
    let bytes = res.into_body().collect().await.expect("read the body").to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({ "raw": String::from_utf8_lossy(&bytes) }))
    };
    (status, value)
}

async fn idle_claim(app: &Router, wait_ms: u64) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/v1/api/steps/claim",
        Some(json!({ "worker_id": "idle", "max": 1, "wait_ms": wait_ms })),
    )
    .await
}

// ----------------------------------------------------------------- bounds ---

#[tokio::test]
async fn a_definition_with_a_duration_past_the_bound_is_a_400_naming_it() {
    let (app, _state, _runs) = app(ServerConfig::default()).await;
    let (status, body) = call(
        &app,
        "POST",
        "/v1/api/definitions",
        Some(json!({ "id": "nap", "body": { "steps": [
            { "kind": "sleep", "id": "nap", "ms": 400_000_000_000_000_i64 }
        ]}})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let text = body.to_string();
    assert!(text.contains("steps[0].ms"), "names the field: {text}");
    assert!(text.contains("400000000000000"), "names the value: {text}");
    assert!(text.contains(&mcpd_runs::MAX_DURATION_MS.to_string()), "names the bound: {text}");
}

#[tokio::test]
async fn a_retry_floor_past_the_bound_is_a_400_and_the_lease_is_still_held() {
    let (app, _state, _runs) = app(ServerConfig::default()).await;
    let (status, body) = call(
        &app,
        "POST",
        "/v1/api/definitions",
        Some(json!({ "id": "retrying", "body": { "steps": [
            { "kind": "task", "id": "work", "retry": { "max_attempts": 3 } }
        ]}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) =
        call(&app, "POST", "/v1/api/runs", Some(json!({ "definition_id": "retrying" }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = idle_claim(&app, 0).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let lease = body["leases"][0]["lease_id"].as_str().expect("a lease").to_owned();

    for floor in [json!(u64::MAX), json!(mcpd_runs::MAX_DURATION_MS + 1)] {
        let (status, body) = call(
            &app,
            "POST",
            &format!("/v1/api/steps/{lease}/fail"),
            Some(json!({ "error": {
                "class": "retryable", "message": "rate limited", "retry_after_ms": floor
            }})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{floor}: {body}");
        let text = body.to_string();
        assert!(text.contains("error.retry_after_ms"), "names the field: {text}");
        assert!(text.contains(&floor.to_string()), "names the value: {text}");
        assert!(text.contains(&mcpd_runs::MAX_DURATION_MS.to_string()), "names the bound: {text}");
    }

    // Refused before anything was written, so the worker can report again.
    let (status, body) = call(
        &app,
        "POST",
        &format!("/v1/api/steps/{lease}/fail"),
        Some(json!({ "error": {
            "class": "retryable", "message": "rate limited", "retry_after_ms": 60_000
        }})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

// -------------------------------------------------------------- long-poll ---

#[tokio::test]
async fn an_idle_long_poll_makes_a_bounded_number_of_claim_attempts() {
    let (app, _state, runs) = app(ServerConfig::default()).await;
    let started = Instant::now();
    let (status, body) = idle_claim(&app, 1_500).await;
    let waited = started.elapsed();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["leases"], json!([]));
    assert!(waited >= Duration::from_millis(1_400), "the poll held for its wait: {waited:?}");

    // One look at the start, and one per half-second slice the loop sleeps
    // through when nothing wakes it. An empty claim that woke its own waiter
    // ran thousands of transactions in the same second and a half.
    let claims = runs.claims.load(Ordering::Relaxed);
    assert!(claims <= 6, "an idle long-poll of 1.5 s made {claims} claim attempts");
}

#[tokio::test]
async fn a_long_poll_longer_than_the_request_timeout_answers_before_it() {
    let config = ServerConfig { request_timeout_seconds: 2, ..ServerConfig::default() };
    let (app, _state, _runs) = app(config).await;
    let started = Instant::now();
    // Inside the 0 to 60000 the contract accepts, and longer than this
    // daemon's 2 s request timeout. Held for the whole of it, the timeout
    // layer answered first with a 503 the worker had done nothing to earn.
    let (status, body) = idle_claim(&app, 5_000).await;
    let waited = started.elapsed();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["leases"], json!([]), "an empty answer means reissue the claim");
    assert!(waited < Duration::from_secs(2), "answered after {waited:?}, past the timeout");
    assert!(waited >= Duration::from_millis(1_400), "still a long-poll: {waited:?}");
}

#[tokio::test]
async fn a_long_poll_in_flight_ends_when_the_daemon_is_told_to_stop() {
    let (app, state, _runs) = app(ServerConfig::default()).await;
    let poll = tokio::spawn(async move { idle_claim(&app, 20_000).await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let told = Instant::now();
    state.shutdown.cancel();
    let (status, body) = tokio::time::timeout(Duration::from_secs(3), poll)
        .await
        .expect("the claim was still waiting three seconds after shutdown")
        .expect("the claim task");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["leases"], json!([]));
    assert!(told.elapsed() < Duration::from_secs(1), "ended {:?} after", told.elapsed());
}

// ---------------------------------------------------------------- erasure ---

#[tokio::test]
async fn an_erasure_waits_for_a_drain_that_already_holds_the_runs_spans() {
    let (app, state, runs) = app(ServerConfig::default()).await;
    let (status, body) = call(
        &app,
        "POST",
        "/v1/api/definitions",
        Some(json!({ "id": "reply", "body": { "steps": [{ "kind": "task", "id": "draft" }] } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, run) = call(
        &app,
        "POST",
        "/v1/api/runs",
        Some(json!({
            "definition_id": "reply",
            "input": { "message": "u_8841 wrote this" },
            "correlation": { "project": "default", "user_id": "u_8841" }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{run}");
    let run_id = run["run_id"].as_str().expect("run id").to_owned();
    let trace_id = run["trace_id"].as_str().expect("trace id").to_owned();
    let (status, body) = call(
        &app,
        "POST",
        "/v1/api/steps/claim",
        Some(json!({ "worker_id": "w-1", "max": 1, "wait_ms": 0, "run_id": run_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let lease = body["leases"][0]["lease_id"].as_str().expect("a lease").to_owned();
    let (status, body) = call(
        &app,
        "POST",
        &format!("/v1/api/steps/{lease}/complete"),
        Some(json!({ "output": { "draft": "a reply to the person" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The drain claims the run's spans and holds them in memory, as a pass
    // does between its claim and its write.
    runs.hold_outbox.store(true, Ordering::SeqCst);
    let drain = tokio::spawn({
        let state = Arc::clone(&state);
        async move { mcpd_server::runs::drain_once(&state).await }
    });
    runs.outbox_claimed.notified().await;

    // The erasure arrives in that window. Without the two being exclusive it
    // erased both stores and answered here, and the drain then wrote the spans
    // into the trace store the reply said was clear.
    let mut erase = tokio::spawn({
        let app = app.clone();
        async move { call(&app, "DELETE", "/v1/api/subjects/u_8841?project=default", None).await }
    });
    let early = tokio::time::timeout(Duration::from_millis(300), &mut erase).await;
    runs.outbox_release.notify_one();
    assert!(drain.await.expect("the drain") > 0, "the held spans were written");
    let (status, body) = match early {
        Ok(answered) => answered.expect("the erasure"),
        Err(_) => erase.await.expect("the erasure"),
    };
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["runs_erased"], json!(1), "{body}");

    let (status, body) = call(&app, "GET", &format!("/v1/api/traces/{trace_id}"), None).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a span claimed before the erasure landed after it: {body}"
    );
}

// ------------------------------------------------------------ the loops ---

#[tokio::test]
async fn a_panic_in_one_pass_of_the_run_loops_does_not_stop_them() {
    // Both loops were spawned bare, so a panic in one pass ended the task, and
    // deadlines stopped firing or spans stopped reaching the trace store while
    // every health check stayed green.
    let (_app, state, runs) = app(ServerConfig::default()).await;
    runs.panic_timers.store(true, Ordering::SeqCst);
    runs.panic_outbox.store(true, Ordering::SeqCst);
    mcpd_server::runs::spawn(Arc::clone(&state));

    let give_up = Instant::now() + Duration::from_secs(10);
    while (runs.timer_passes.load(Ordering::SeqCst) < 3
        || runs.outbox_claims.load(Ordering::SeqCst) < 3)
        && Instant::now() < give_up
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        runs.timer_passes.load(Ordering::SeqCst) >= 3,
        "the deadline loop stopped after a panic: {} passes",
        runs.timer_passes.load(Ordering::SeqCst)
    );
    assert!(
        runs.outbox_claims.load(Ordering::SeqCst) >= 3,
        "the outbox drain stopped after a panic: {} claims",
        runs.outbox_claims.load(Ordering::SeqCst)
    );

    // And both still stop when told to, the drain after one last pass.
    let before = runs.outbox_claims.load(Ordering::SeqCst);
    state.shutdown.cancel();
    state.tasks.close();
    tokio::time::timeout(Duration::from_secs(5), state.tasks.wait())
        .await
        .expect("the run loops did not stop on shutdown");
    assert!(runs.outbox_claims.load(Ordering::SeqCst) > before, "no last drain on shutdown");
}
