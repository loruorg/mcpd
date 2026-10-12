//! The worker loop and the run API against a scripted daemon (contract
//! sections 7.2 to 7.4).

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{FakeMcpd, Reply};
use mcpd_sdk::{
    ClaimSteps, Config, Error, Mcpd, PublishEvent, SpanKind, StartRun, StepContext, StepErrorClass,
    StepFailure, StepOptions, Worker, Workflow,
};
use serde_json::{json, Value};

const TRACE: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const STEP_SPAN: &str = "b2b2b2b2b2b2b2b2";

fn lease(overrides: Value) -> Value {
    let mut lease = json!({
        "lease_id": "lease-1", "run_id": "run-1", "project": "test", "definition_id": "support",
        "definition_version": 1, "step_key": "classify", "kind": "task", "attempt": 0,
        "effect": "at_least_once", "expires_at": "2026-09-01T10:00:00Z", "idempotency_key": "run-1:classify:0",
        "input": {"ticket_id": "T-1"},
        "prior": {"triage": {"status": "succeeded", "output": {"queue_name": "billing"}}},
        "state": {"seen_count": 1},
        "correlation": {"project": "test", "session_id": "s-1"},
        "trace_id": TRACE, "span_id": STEP_SPAN, "parent_span_id": "c3c3c3c3c3c3c3c3"
    });
    if let (Value::Object(base), Value::Object(over)) = (&mut lease, overrides) {
        base.extend(over);
    }
    lease
}

fn ok(body: Value) -> Reply {
    Reply::Body(200, body.to_string())
}

fn claimed(leases: Vec<Value>) -> Reply {
    ok(json!({ "leases": leases }))
}

fn started() -> Reply {
    ok(json!({"expires_at": "2026-09-01T10:00:30Z", "reclaimed": false}))
}

fn acked() -> Reply {
    ok(json!({"run_status": "running", "next": "claim"}))
}

fn registered() -> Reply {
    ok(
        json!({"worker_id": "w-1", "registered": [{"id": "support", "version": 1, "assigned": true}],
              "lease_ttl_ms": 3000, "heartbeat_ms": 1000}),
    )
}

fn conflict(reason: &str, extra: Value) -> Reply {
    let mut body = json!({"error": "conflict", "reason": reason, "message": format!("the daemon says {reason}"),
                          "attempt": 0, "run_status": "running"});
    if let (Value::Object(b), Value::Object(e)) = (&mut body, extra) {
        b.extend(e);
    }
    Reply::Body(409, body.to_string())
}

struct Harness {
    server: FakeMcpd,
    mcpd: Mcpd,
    errors: Arc<Mutex<Vec<String>>>,
}

impl Harness {
    async fn new(replies: Vec<Reply>, routes: Vec<(&str, Vec<Reply>)>) -> Self {
        let server = FakeMcpd::routed(replies, routes).await;
        let mut config = Config::default().with_endpoint(server.endpoint()).with_project("test");
        config.flush_interval = Duration::from_secs(3600);
        let mcpd = Mcpd::with_config(config);
        Self { server, mcpd, errors: Arc::new(Mutex::new(Vec::new())) }
    }

    fn worker(&self, workflow: Workflow) -> Worker {
        let errors = Arc::clone(&self.errors);
        Worker::builder(&self.mcpd)
            .id("w-1")
            .wait(Duration::ZERO)
            .concurrency(3)
            .on_error(move |message, error| {
                errors.lock().unwrap().push(format!("{message}: {error}"))
            })
            .register(workflow)
            .build()
    }

    fn paths(&self) -> Vec<String> {
        self.server.requests().iter().map(|r| format!("{} {}", r.method, r.path())).collect()
    }

    fn body(&self, i: usize) -> Value {
        self.server.requests()[i].json()
    }

    fn reported(&self) -> Vec<String> {
        self.errors.lock().unwrap().clone()
    }
}

async fn claim_and_drain(worker: &Worker, max: usize) -> usize {
    let n = worker.claim(max, None).await.expect("claim");
    worker.drain().await;
    n
}

/// The daemon holds an idle claim open for the whole of `wait`, so the answer
/// arrives after the client's own timeout and within the wait. A claim cut off
/// by that timeout is what made a healthy idle worker report a failure every
/// ten seconds.
#[tokio::test]
async fn an_idle_claim_outlives_the_client_timeout_by_the_wait() {
    let server =
        FakeMcpd::routed(vec![Reply::After(300, 200, r#"{"leases": []}"#.to_owned())], vec![])
            .await;
    let mut config = Config::default().with_endpoint(server.endpoint()).with_project("test");
    config.flush_interval = Duration::from_secs(3600);
    config.timeout = Duration::from_millis(100);
    let mcpd = Mcpd::with_config(config);

    let response = mcpd
        .claim_steps(ClaimSteps {
            worker_id: "w-1".to_owned(),
            max: 1,
            wait: Duration::from_secs(2),
            run_id: None,
            project: None,
        })
        .await
        .expect("an idle claim answered within the wait");

    assert!(response.leases.is_empty());
}

#[tokio::test]
async fn one_lease_runs_started_the_body_and_complete() {
    let h = Harness::new(vec![claimed(vec![lease(json!({}))]), started(), acked()], vec![]).await;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let worker = h.worker(Workflow::new("support").step(
        "classify",
        move |ctx: StepContext| {
            let sink = Arc::clone(&sink);
            async move {
                let input: Value = ctx.input()?;
                let prior: Value = ctx.prior()?;
                let state: Value = ctx.state()?;
                sink.lock().unwrap().extend([
                    input,
                    prior,
                    state,
                    json!(ctx.idempotency_key()),
                    json!(ctx.attempt()),
                    json!(ctx.run_id()),
                    json!(ctx.step_key()),
                ]);
                ctx.set_state(json!({"seen_count": 2, "last_ticket": "T-1"}));
                ctx.set_tokens(10, 5);
                Ok::<_, StepFailure>(json!({"kind": "question"}))
            }
        },
        StepOptions::new(),
    ));
    assert_eq!(claim_and_drain(&worker, 1).await, 1);
    assert_eq!(
        h.paths(),
        [
            "POST /v1/api/steps/claim",
            "POST /v1/api/steps/lease-1/started",
            "POST /v1/api/steps/lease-1/complete"
        ]
    );
    assert_eq!(h.body(0), json!({"project": "test", "worker_id": "w-1", "max": 1, "wait_ms": 0}));
    // Opaque objects reach the body spelled as the daemon sent them.
    assert_eq!(
        *seen.lock().unwrap(),
        vec![
            json!({"ticket_id": "T-1"}),
            json!({"triage": {"status": "succeeded", "output": {"queue_name": "billing"}}}),
            json!({"seen_count": 1}),
            json!("run-1:classify:0"),
            json!(0),
            json!("run-1"),
            json!("classify"),
        ]
    );
    assert_eq!(
        h.body(2),
        json!({"output": {"kind": "question"}, "state": {"seen_count": 2, "last_ticket": "T-1"},
               "usage": {"input_tokens": 10, "output_tokens": 5}})
    );
    assert!(h.reported().is_empty(), "{:?}", h.reported());
    assert_eq!(worker.inflight(), 0);
}

#[tokio::test]
async fn a_span_from_the_body_lands_under_the_step_span_the_daemon_opened() {
    let h = Harness::new(
        vec![claimed(vec![lease(json!({}))]), started(), acked(), Reply::Accepted],
        vec![],
    )
    .await;
    let worker = h.worker(Workflow::new("support").agent(
        "classify",
        |ctx: StepContext| async move {
            let span = ctx.span("model_call", SpanKind::ModelGeneration);
            span.set_model("claude-sonnet-4", "anthropic");
            span.end();
            Ok::<_, StepFailure>("ok")
        },
        StepOptions::new(),
    ));
    claim_and_drain(&worker, 1).await;
    h.mcpd.flush().await.expect("flush");
    // The span goes on the wire when it starts and again when it ends (B8),
    // and both records sit under the step the daemon opened.
    let spans = h.server.all_spans();
    assert!(!spans.is_empty());
    for span in &spans {
        assert_eq!(span["name"], "model_call");
        assert_eq!(span["trace_id"], TRACE);
        assert_eq!(span["parent_span_id"], STEP_SPAN);
    }
    let ended = spans.last().unwrap();
    assert_eq!(ended["status"], "ok");
    assert_eq!(ended["model"], "claude-sonnet-4");
    assert_eq!(ended["correlation"]["session_id"], "s-1");
}

#[tokio::test]
async fn a_branch_an_item_and_an_iteration_key_find_their_handlers() {
    // Three bodies run on three tasks, so the answers are routed by path.
    let h = Harness::new(
        vec![claimed(vec![
            lease(json!({"lease_id": "l-a", "step_key": "checks/fraud"})),
            lease(json!({"lease_id": "l-b", "step_key": "notify[2]"})),
            lease(json!({"lease_id": "l-c", "step_key": "refine[1]"})),
        ])],
        vec![("/started", vec![started()]), ("/complete", vec![acked()])],
    )
    .await;
    let ran = Arc::new(Mutex::new(Vec::new()));
    let record = |name: &'static str| {
        let ran = Arc::clone(&ran);
        move |_ctx: StepContext| {
            let ran = Arc::clone(&ran);
            async move {
                ran.lock().unwrap().push(name);
                Ok::<_, StepFailure>(name)
            }
        }
    };
    let worker = h.worker(
        Workflow::new("support")
            .parallel(
                "checks",
                |b| {
                    b.step("fraud", record("fraud"), StepOptions::new());
                },
                StepOptions::new(),
            )
            .foreach(
                "notify",
                "input.to",
                3,
                |b| {
                    b.step("send", record("send"), StepOptions::new());
                },
                StepOptions::new(),
            )
            .repeat(
                "refine",
                2,
                |b| {
                    b.step("rewrite", record("rewrite"), StepOptions::new());
                },
                StepOptions::new(),
            ),
    );
    assert_eq!(claim_and_drain(&worker, 3).await, 3);
    let mut names = ran.lock().unwrap().clone();
    names.sort_unstable();
    assert_eq!(names, ["fraud", "rewrite", "send"]);
    assert_eq!(h.paths().iter().filter(|p| p.ends_with("/complete")).count(), 3);
}

#[tokio::test]
async fn a_retryable_error_is_reported_with_its_class_and_delay() {
    let h = Harness::new(
        vec![
            claimed(vec![lease(json!({}))]),
            started(),
            ok(json!({"run_status": "running", "next": "idle", "attempts_left": 2})),
        ],
        vec![],
    )
    .await;
    let worker = h.worker(Workflow::new("support").step(
        "classify",
        |_ctx: StepContext| async move {
            Err::<Value, _>(
                StepFailure::retryable("rate limited")
                    .retry_after(Duration::from_millis(250))
                    .details(json!({"status_code": 429})),
            )
        },
        StepOptions::new(),
    ));
    claim_and_drain(&worker, 1).await;
    assert_eq!(h.paths()[2], "POST /v1/api/steps/lease-1/fail");
    assert_eq!(
        h.body(2),
        json!({"error": {"class": "retryable", "message": "rate limited", "details": {"status_code": 429}, "retry_after_ms": 250}})
    );
}

#[tokio::test]
async fn a_plain_error_is_fatal_and_a_failure_names_its_class() {
    let h = Harness::new(
        vec![claimed(vec![
            lease(json!({})),
            lease(json!({"lease_id": "lease-2", "step_key": "draft"})),
        ])],
        vec![("/started", vec![started()]), ("/fail", vec![acked()])],
    )
    .await;
    let worker = h.worker(
        Workflow::new("support")
            .step(
                "classify",
                |_ctx: StepContext| async move { Err::<Value, _>("the model returned nothing") },
                StepOptions::new(),
            )
            .step(
                "draft",
                |_ctx: StepContext| async move {
                    Err::<Value, _>(StepFailure::invalid_input("no such ticket"))
                },
                StepOptions::new(),
            ),
    );
    claim_and_drain(&worker, 2).await;
    let mut fails: Vec<Value> = h
        .server
        .requests()
        .iter()
        .filter(|r| r.path().ends_with("/fail"))
        .map(|r| r.json()["error"].clone())
        .collect();
    fails.sort_by_key(|e| e["class"].as_str().unwrap_or_default().to_owned());
    assert_eq!(
        fails,
        [
            json!({"class": "fatal", "message": "the model returned nothing"}),
            json!({"class": "invalid_input", "message": "no such ticket"}),
        ]
    );
}

#[tokio::test]
async fn a_lease_it_cannot_serve_is_handed_back_without_running_anything() {
    let h = Harness::new(
        vec![claimed(vec![
            lease(json!({"definition_id": "billing"})),
            lease(json!({"lease_id": "lease-2", "definition_version": 7})),
        ])],
        vec![("/fail", vec![acked()])],
    )
    .await;
    let ran = Arc::new(Mutex::new(false));
    let flag = Arc::clone(&ran);
    let worker = h.worker(Workflow::new("support").with_version(1).step(
        "classify",
        move |_ctx: StepContext| {
            let flag = Arc::clone(&flag);
            async move {
                *flag.lock().unwrap() = true;
                Ok::<_, StepFailure>(1)
            }
        },
        StepOptions::new(),
    ));
    claim_and_drain(&worker, 2).await;
    assert!(!*ran.lock().unwrap());
    let requests = h.server.requests();
    assert_eq!(requests.len(), 3);
    for r in requests.iter().skip(1) {
        assert!(r.path().ends_with("/fail"), "{}", r.path());
        let error = &r.json()["error"];
        assert_eq!(error["class"], "unservable");
        assert!(error["message"].as_str().unwrap().contains("w-1 does not serve"));
    }
}

#[tokio::test]
async fn already_applied_is_done_and_says_nothing() {
    let h = Harness::new(
        vec![
            claimed(vec![lease(json!({}))]),
            started(),
            conflict("already_applied", json!({"attempt_status": "succeeded"})),
        ],
        vec![],
    )
    .await;
    let worker = h.worker(Workflow::new("support").step(
        "classify",
        |_ctx: StepContext| async move { Ok::<_, StepFailure>(1) },
        StepOptions::new(),
    ));
    claim_and_drain(&worker, 1).await;
    assert_eq!(h.paths().len(), 3);
    assert!(h.reported().is_empty(), "{:?}", h.reported());
}

#[tokio::test]
async fn a_body_whose_started_was_fenced_never_runs() {
    let h = Harness::new(
        vec![
            claimed(vec![lease(json!({}))]),
            conflict("fenced", json!({"attempt_status": "orphaned"})),
        ],
        vec![],
    )
    .await;
    let ran = Arc::new(Mutex::new(false));
    let flag = Arc::clone(&ran);
    let worker = h.worker(Workflow::new("support").step(
        "classify",
        move |_ctx: StepContext| {
            let flag = Arc::clone(&flag);
            async move {
                *flag.lock().unwrap() = true;
                Ok::<_, StepFailure>(1)
            }
        },
        StepOptions::new(),
    ));
    claim_and_drain(&worker, 1).await;
    assert!(!*ran.lock().unwrap());
    assert_eq!(h.paths(), ["POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started"]);
    assert!(h.reported().is_empty());
}

#[tokio::test]
async fn a_lease_whose_run_ended_is_abandoned_with_nothing_more_posted() {
    let h = Harness::new(
        vec![
            claimed(vec![lease(json!({}))]),
            started(),
            conflict("run_terminal", json!({"run_status": "cancelled"})),
        ],
        vec![],
    )
    .await;
    let worker = h.worker(Workflow::new("support").step(
        "classify",
        |_ctx: StepContext| async move { Ok::<_, StepFailure>(1) },
        StepOptions::new(),
    ));
    claim_and_drain(&worker, 1).await;
    assert_eq!(h.paths().len(), 3);
    assert!(h.reported().is_empty());
}

#[tokio::test]
async fn not_started_is_logged_loudly_because_it_is_a_bug_in_the_worker() {
    let h = Harness::new(
        vec![
            claimed(vec![lease(json!({"effect": "at_most_once"}))]),
            started(),
            conflict("not_started", json!({})),
        ],
        vec![],
    )
    .await;
    let worker = h.worker(Workflow::new("support").step(
        "classify",
        |_ctx: StepContext| async move { Ok::<_, StepFailure>(1) },
        StepOptions::new(),
    ));
    claim_and_drain(&worker, 1).await;
    let reported = h.reported();
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert!(reported[0].contains("a bug in this worker"));
}

#[tokio::test]
async fn a_fenced_heartbeat_cancels_the_body_and_posts_no_complete() {
    let h = Harness::new(
        vec![
            registered(),
            claimed(vec![lease(json!({}))]),
            started(),
            conflict("fenced", json!({"attempt_status": "orphaned"})),
        ],
        vec![],
    )
    .await;
    let reason = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&reason);
    let worker = h.worker(Workflow::new("support").step(
        "classify",
        move |ctx: StepContext| {
            let sink = Arc::clone(&sink);
            async move {
                ctx.cancelled().await;
                *sink.lock().unwrap() = ctx.cancel_reason();
                Ok::<_, StepFailure>("too late")
            }
        },
        StepOptions::new(),
    ));
    worker.register_definitions().await.expect("register");
    claim_and_drain(&worker, 1).await;
    assert!(reason.lock().unwrap().as_deref().unwrap_or_default().contains("fenced"));
    assert_eq!(
        h.paths(),
        [
            "POST /v1/api/workers/register",
            "POST /v1/api/steps/claim",
            "POST /v1/api/steps/lease-1/started",
            "POST /v1/api/steps/lease-1/heartbeat"
        ]
    );
    assert!(h.reported().is_empty());
}

#[tokio::test]
async fn a_heartbeat_that_fails_for_any_other_reason_is_reported_and_the_body_goes_on() {
    let h = Harness::new(
        vec![
            registered(),
            claimed(vec![lease(json!({}))]),
            started(),
            Reply::Body(400, r#"{"error":"bad_request","message":"no"}"#.to_owned()),
            acked(),
        ],
        vec![],
    )
    .await;
    let worker = h.worker(Workflow::new("support").step(
        "classify",
        |_ctx: StepContext| async move {
            tokio::time::sleep(Duration::from_millis(1150)).await;
            Ok::<_, StepFailure>("done")
        },
        StepOptions::new(),
    ));
    worker.register_definitions().await.expect("register");
    claim_and_drain(&worker, 1).await;
    assert_eq!(h.paths().last().map(String::as_str), Some("POST /v1/api/steps/lease-1/complete"));
    assert!(h.reported().iter().any(|e| e.contains("heartbeat failed")), "{:?}", h.reported());
}

#[tokio::test]
async fn serve_registers_claims_until_stopped_and_resolves_once_stopped() {
    let h = Harness::new(vec![registered()], vec![("/steps/claim", vec![claimed(vec![])])]).await;
    let worker = h.worker(Workflow::new("support").step(
        "classify",
        |_ctx: StepContext| async move { Ok::<_, StepFailure>(1) },
        StepOptions::new(),
    ));
    let serving = {
        let worker = worker.clone();
        tokio::spawn(async move { worker.serve().await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    worker.stop();
    tokio::time::timeout(Duration::from_secs(5), serving)
        .await
        .expect("serve stopped")
        .unwrap()
        .expect("serve");

    let paths = h.paths();
    assert_eq!(paths[0], "POST /v1/api/workers/register");
    assert_eq!(
        h.body(0),
        json!({"project": "test", "worker_id": "w-1",
               "definitions": [{"id": "support", "kind": "workflow", "body": {"steps": [{"kind": "task", "id": "classify"}]}}]})
    );
    assert!(paths.len() > 2, "{paths:?}");
    assert!(paths[1..].iter().all(|p| p == "POST /v1/api/steps/claim"));
    assert_eq!(h.body(1), json!({"project": "test", "worker_id": "w-1", "max": 3, "wait_ms": 0}));
    assert!(h.reported().is_empty());
}

#[tokio::test]
async fn a_claim_that_failed_is_reported_and_tried_again() {
    let h = Harness::new(
        vec![
            registered(),
            Reply::Body(400, r#"{"error":"bad_request","message":"no"}"#.to_owned()),
        ],
        vec![],
    )
    .await;
    let worker = h.worker(Workflow::new("support").step(
        "classify",
        |_ctx: StepContext| async move { Ok::<_, StepFailure>(1) },
        StepOptions::new(),
    ));
    let serving = {
        let worker = worker.clone();
        tokio::spawn(async move { worker.serve().await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    worker.stop();
    let _ = tokio::time::timeout(Duration::from_secs(5), serving).await.expect("serve stopped");
    assert!(h.reported().iter().any(|e| e.contains("claim failed")), "{:?}", h.reported());
}

#[tokio::test]
async fn a_run_starts_with_the_fields_the_daemon_expects() {
    let h = Harness::new(
        vec![ok(json!({
            "run_id": "run-1", "project": "test", "definition_id": "support", "definition_version": 2,
            "status": "running", "cursor": {"index": 0, "construct": "task"}, "correlation": {"project": "test"},
            "trace_id": TRACE, "steps_used": 0, "cost_usd": 0.0, "root_run_id": "run-1", "depth": 0,
            "created_at": "2026-09-01T10:00:00Z", "updated_at": "2026-09-01T10:00:00Z"
        }))],
        vec![],
    )
    .await;
    let correlation = mcpd_sdk::model::Correlation {
        project: String::new(),
        environment: None,
        service_name: None,
        service_version: None,
        session_id: Some("s-1".to_owned()),
        thread_id: None,
        user_id: None,
        resource_id: None,
        run_id: None,
        request_id: None,
        experiment_id: None,
        release: None,
    };
    let run = h
        .mcpd
        .start_run(
            StartRun::new("support")
                .version(2)
                .input(json!({"ticket_id": "T-1"}))
                .idempotency_key("ticket:T-1")
                .correlation(correlation),
        )
        .await
        .expect("start");
    assert_eq!(run.run_id, "run-1");
    assert_eq!(
        h.body(0),
        json!({"project": "test", "definition_id": "support", "version": 2, "input": {"ticket_id": "T-1"},
               "idempotency_key": "ticket:T-1", "correlation": {"project": "test", "session_id": "s-1"}})
    );
}

#[tokio::test]
async fn a_definition_is_stored_with_its_body_untouched() {
    let h =
        Harness::new(vec![ok(json!({"id": "support", "version": 1, "assigned": true}))], vec![])
            .await;
    let doc = Workflow::new("support")
        .step(
            "classify",
            |_ctx: StepContext| async move { Ok::<_, StepFailure>(1) },
            StepOptions::new().timeout(Duration::from_millis(100)),
        )
        .document()
        .unwrap();
    let stored = h.mcpd.put_definition(&doc, None).await.expect("put");
    assert_eq!((stored.id.as_str(), stored.version, stored.assigned), ("support", 1, true));
    let requests = h.server.requests();
    assert_eq!(requests[0].query(), "project=test");
    assert_eq!(
        requests[0].json(),
        json!({"id": "support", "kind": "workflow", "body": {"steps": [{"kind": "task", "id": "classify", "step_timeout_ms": 100}]}})
    );
}

#[tokio::test]
async fn watch_run_walks_the_pages_until_the_run_ends_passing_the_last_seq_it_saw() {
    let h = Harness::new(
        vec![
            ok(json!({"events": [
                {"run_id": "run-1", "seq": 0, "kind": "run_started", "payload": {"definition_id": "support"}, "at": "2026-09-01T10:00:00Z"},
                {"run_id": "run-1", "seq": 1, "kind": "step_leased", "payload": {"step_key": "classify"}, "at": "2026-09-01T10:00:01Z"}
            ], "has_more": true})),
            ok(json!({"events": [], "has_more": false})),
            ok(json!({"events": [
                {"run_id": "run-1", "seq": 2, "kind": "run_ended", "payload": {"status": "succeeded"}, "at": "2026-09-01T10:00:02Z"}
            ], "has_more": false})),
        ],
        vec![],
    )
    .await;
    let mut kinds = Vec::new();
    h.mcpd
        .watch_run_every("run-1", -1, Duration::from_millis(1), |e| {
            kinds.push(serde_json::to_value(e.kind).unwrap())
        })
        .await
        .expect("watch");
    assert_eq!(kinds, [json!("run_started"), json!("step_leased"), json!("run_ended")]);
    let queries: Vec<String> = h.server.requests().iter().map(|r| r.query().to_owned()).collect();
    assert_eq!(queries, ["after=-1&limit=500", "after=1&limit=500", "after=1&limit=500"]);
}

#[tokio::test]
async fn publish_and_signal_carry_the_ids_the_caller_supplied() {
    let receipt = ok(json!({"outcome": "delivered", "event_id": "evt-1"}));
    let h = Harness::new(vec![receipt.clone(), receipt], vec![]).await;
    h.mcpd
        .publish_event(
            PublishEvent::new("approved", "T-1").payload(json!({"by": "ops"})).event_id("evt-1"),
        )
        .await
        .expect("publish");
    h.mcpd
        .signal_run("run-1", "approved", Some(json!({"by": "ops"})), Some("evt-2"))
        .await
        .expect("signal");
    let requests = h.server.requests();
    assert_eq!(requests[0].path(), "/v1/api/events");
    assert_eq!(
        requests[0].json(),
        json!({"project": "test", "event": "approved", "key": "T-1", "payload": {"by": "ops"}, "event_id": "evt-1"})
    );
    assert_eq!(requests[1].path(), "/v1/api/runs/run-1/signal");
    assert_eq!(
        requests[1].json(),
        json!({"event": "approved", "payload": {"by": "ops"}, "event_id": "evt-2"})
    );
}

#[tokio::test]
async fn a_409_from_a_worker_call_is_a_typed_conflict_and_any_other_409_is_itself() {
    let h = Harness::new(
        vec![
            conflict("fenced", json!({"attempt_status": "orphaned"})),
            Reply::Body(
                409,
                r#"{"error":"version_taken","message":"support@1 exists with a different body"}"#
                    .to_owned(),
            ),
        ],
        vec![],
    )
    .await;
    let err =
        h.mcpd.complete_step("lease-1", &mcpd_sdk::model::StepResult::default()).await.unwrap_err();
    match err {
        Error::Conflict(c) => {
            assert_eq!(serde_json::to_value(c.reason).unwrap(), "fenced");
            assert_eq!(serde_json::to_value(c.attempt_status).unwrap(), "orphaned");
        }
        other => panic!("expected a conflict, got {other}"),
    }
    let doc = Workflow::new("support").with_version(1).document().unwrap();
    let err = h.mcpd.put_definition(&doc, None).await.unwrap_err();
    match err {
        Error::Api { status, message } => {
            assert_eq!(status, 409);
            assert!(message.contains("different body"), "{message}");
        }
        other => panic!("expected an api error, got {other}"),
    }
    let _ = (
        StepErrorClass::Fatal,
        ClaimSteps {
            worker_id: String::new(),
            max: 1,
            wait: Duration::ZERO,
            run_id: None,
            project: None,
        },
    );
}

#[tokio::test]
async fn a_body_that_runs_past_the_lease_deadline_is_cancelled_and_failed_retryable() {
    let h = Harness::new(
        vec![claimed(vec![lease(json!({"deadline_ms": 150}))]), started(), acked()],
        vec![],
    )
    .await;
    let reason = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&reason);
    let worker = h.worker(Workflow::new("support").step(
        "classify",
        move |ctx: StepContext| {
            let sink = Arc::clone(&sink);
            async move {
                ctx.cancelled().await;
                *sink.lock().unwrap() = ctx.cancel_reason();
                Ok::<_, StepFailure>("too late")
            }
        },
        StepOptions::new(),
    ));
    claim_and_drain(&worker, 1).await;
    assert_eq!(reason.lock().unwrap().as_deref(), Some("step deadline of 150ms passed"));
    assert_eq!(
        h.paths(),
        [
            "POST /v1/api/steps/claim",
            "POST /v1/api/steps/lease-1/started",
            "POST /v1/api/steps/lease-1/fail"
        ]
    );
    assert_eq!(
        h.body(2),
        json!({"error": {"class": "retryable", "message": "step deadline of 150ms passed"}})
    );
    assert!(h.reported().is_empty(), "{:?}", h.reported());
}

#[tokio::test]
async fn the_slot_is_freed_at_the_deadline_even_when_the_body_ignores_cancelled() {
    let h = Harness::new(
        vec![claimed(vec![lease(json!({"deadline_ms": 100}))]), started(), acked()],
        vec![],
    )
    .await;
    let worker = h.worker(Workflow::new("support").step(
        "classify",
        // A body that never looks at `cancelled()`.
        |_ctx: StepContext| async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            Ok::<_, StepFailure>("too late")
        },
        StepOptions::new(),
    ));
    let began = std::time::Instant::now();
    claim_and_drain(&worker, 1).await;
    assert!(
        began.elapsed() < Duration::from_secs(1),
        "the slot was held for {:?}",
        began.elapsed()
    );
    assert_eq!(worker.stats().inflight, 0);
    assert_eq!(h.paths().last().map(String::as_str), Some("POST /v1/api/steps/lease-1/fail"));
}

#[tokio::test]
async fn a_body_that_finishes_in_time_is_left_alone() {
    let h = Harness::new(
        vec![claimed(vec![lease(json!({"deadline_ms": 5000}))]), started(), acked()],
        vec![],
    )
    .await;
    let worker = h.worker(Workflow::new("support").step(
        "classify",
        |_ctx: StepContext| async move { Ok::<_, StepFailure>("quick") },
        StepOptions::new(),
    ));
    claim_and_drain(&worker, 1).await;
    assert_eq!(h.paths().last().map(String::as_str), Some("POST /v1/api/steps/lease-1/complete"));
}

fn run_body(status: &str, extra: Value) -> Value {
    let mut run = json!({
        "run_id": "run-1", "project": "test", "definition_id": "support", "definition_version": 1,
        "status": status, "cursor": {"index": 2, "construct": "end"}, "correlation": {"project": "test"},
        "trace_id": TRACE, "steps_used": 2, "cost_usd": 0.25, "root_run_id": "run-1", "depth": 0,
        "created_at": "2026-09-01T10:00:00Z", "updated_at": "2026-09-01T10:00:05Z"
    });
    if let (Value::Object(r), Value::Object(e)) = (&mut run, extra) {
        r.extend(e);
    }
    json!({"run": run, "attempts": [], "timers": [], "waits": []})
}

fn ended() -> Reply {
    ok(
        json!({"events": [{"run_id": "run-1", "seq": 0, "kind": "run_ended", "at": "2026-09-01T10:00:05Z"}], "has_more": false}),
    )
}

#[tokio::test]
async fn wait_run_returns_the_output_once_the_run_ends() {
    let h = Harness::new(
        vec![ended(), ok(run_body("succeeded", json!({"output": {"answer": 42}})))],
        vec![],
    )
    .await;
    let result =
        h.mcpd.wait_run_every("run-1", -1, None, Duration::from_millis(1)).await.expect("wait");
    assert_eq!(result.output, Some(json!({"answer": 42})));
    assert_eq!(result.output_as::<Value>().unwrap()["answer"], 42);
    assert_eq!(result.trace_id, TRACE);
    assert!((result.cost_usd - 0.25).abs() < f64::EPSILON);
}

#[tokio::test]
async fn wait_run_returns_a_typed_error_carrying_the_run_when_it_did_not_succeed() {
    let h = Harness::new(
        vec![ended(), ok(run_body("failed", json!({"error": "send: relay down"})))],
        vec![],
    )
    .await;
    let err = h.mcpd.wait_run_every("run-1", -1, None, Duration::from_millis(1)).await.unwrap_err();
    match &err {
        Error::RunFailed(run) => assert_eq!(run.error.as_deref(), Some("send: relay down")),
        other => panic!("expected RunFailed, got {other}"),
    }
    assert_eq!(err.to_string(), "run run-1 failed: send: relay down");
}

#[tokio::test]
async fn wait_run_gives_up_after_the_timeout_while_the_run_is_still_going() {
    let h =
        Harness::new(vec![], vec![("/events", vec![ok(json!({"events": [], "has_more": false}))])])
            .await;
    let err = h
        .mcpd
        .wait_run_every("run-1", -1, Some(Duration::from_millis(40)), Duration::from_millis(5))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("still running"), "{err}");
}

#[tokio::test]
async fn start_run_and_wait_does_both() {
    let h = Harness::new(
        vec![
            ok(run_body("running", json!({}))["run"].clone()),
            ended(),
            ok(run_body("succeeded", json!({"output": "done"}))),
        ],
        vec![],
    )
    .await;
    let result = h
        .mcpd
        .start_run_and_wait(StartRun::new("support").input(json!({"a": 1})), None)
        .await
        .expect("run");
    assert_eq!(result.output, Some(json!("done")));
    assert_eq!(
        h.body(0),
        json!({"project": "test", "definition_id": "support", "input": {"a": 1}})
    );
}

// ------------------------------------------- usage accumulation (7.3) ---

#[tokio::test]
async fn usage_on_the_spans_under_the_step_is_summed_onto_the_step() {
    let h = Harness::new(vec![claimed(vec![lease(json!({}))]), started(), acked()], vec![]).await;
    let mcpd = h.mcpd.clone();
    let worker = h.worker(Workflow::new("support").agent(
        "classify",
        move |ctx: StepContext| {
            let mcpd = mcpd.clone();
            async move {
                // No set_usage anywhere: the spans carry the tokens.
                let call = ctx.span("model_call", SpanKind::ModelGeneration);
                call.set_model("claude-sonnet-4", "anthropic");
                call.set_tokens(10, 5);
                {
                    // A child by handle.
                    let tool = call.child_of("tool_model", SpanKind::ModelGeneration);
                    tool.set_model("claude-haiku", "anthropic");
                    tool.set_tokens(3, 2);
                }
                // A child by scope, opened three layers down with no handle.
                call.scope(async {
                    let nested = mcpd.span("embed", SpanKind::RagEmbedding);
                    nested.set_tokens(1, 1);
                    nested.end();
                })
                .await;
                call.end();
                Ok::<_, StepFailure>("ok")
            }
        },
        StepOptions::new(),
    ));
    claim_and_drain(&worker, 1).await;
    assert_eq!(
        h.body(2)["usage"],
        json!({"input_tokens": 14, "output_tokens": 8, "model": "claude-sonnet-4", "provider": "anthropic"}),
        "the sum of every span under the step, with the last model seen"
    );
    assert!(h.reported().is_empty(), "{:?}", h.reported());
}

#[tokio::test]
async fn set_usage_overrides_what_the_spans_summed() {
    let h = Harness::new(vec![claimed(vec![lease(json!({}))]), started(), acked()], vec![]).await;
    let worker = h.worker(Workflow::new("support").agent(
        "classify",
        |ctx: StepContext| async move {
            let call = ctx.span("model_call", SpanKind::ModelGeneration);
            call.set_model("claude-sonnet-4", "anthropic");
            call.set_tokens(10, 5);
            call.end();
            ctx.set_usage(mcpd_sdk::StepUsage {
                input_tokens: Some(100),
                model: Some("gpt-5".to_owned()),
                ..mcpd_sdk::StepUsage::default()
            });
            Ok::<_, StepFailure>("ok")
        },
        StepOptions::new(),
    ));
    claim_and_drain(&worker, 1).await;
    assert_eq!(
        h.body(2)["usage"],
        json!({"input_tokens": 100, "output_tokens": 5, "model": "gpt-5", "provider": "anthropic"}),
        "the fields set explicitly replace the sum; the rest stays"
    );
}

// ------------------------------------------------- hooks and stats (7.2) ---

#[tokio::test]
async fn the_four_hooks_fire_with_what_the_contract_says() {
    let h = Harness::new(
        vec![
            claimed(vec![lease(json!({}))]),
            started(),
            conflict("already_applied", json!({"attempt_status": "succeeded"})),
        ],
        vec![],
    )
    .await;
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let (l, s, e, c) = (seen.clone(), seen.clone(), seen.clone(), seen.clone());
    let worker = Worker::builder(&h.mcpd)
        .id("w-1")
        .wait(Duration::ZERO)
        .on_lease(move |lease| l.lock().unwrap().push(format!("lease:{}", lease.step_key)))
        .on_step_start(move |ctx| s.lock().unwrap().push(format!("start:{}", ctx.step_key())))
        .on_step_end(move |ctx, outcome| {
            e.lock().unwrap().push(format!("end:{}:{outcome:?}", ctx.step_key()));
        })
        .on_conflict(move |lease, reason| {
            c.lock().unwrap().push(format!("conflict:{}:{reason}", lease.step_key));
        })
        .register(Workflow::new("support").step(
            "classify",
            |_ctx: StepContext| async move { Ok::<_, StepFailure>(1) },
            StepOptions::new(),
        ))
        .build();
    claim_and_drain(&worker, 1).await;
    assert_eq!(
        *seen.lock().unwrap(),
        vec![
            "lease:classify",
            "start:classify",
            "end:classify:Completed",
            "conflict:classify:already_applied",
        ]
    );
}

#[tokio::test]
async fn a_hook_that_panics_is_reported_and_the_lease_still_completes() {
    let h = Harness::new(vec![claimed(vec![lease(json!({}))]), started(), acked()], vec![]).await;
    let errors = Arc::clone(&h.errors);
    let worker = Worker::builder(&h.mcpd)
        .id("w-1")
        .wait(Duration::ZERO)
        .on_error(move |message, error| errors.lock().unwrap().push(format!("{message}: {error}")))
        .on_step_start(|_ctx| panic!("the hook fell over"))
        .register(Workflow::new("support").step(
            "classify",
            |_ctx: StepContext| async move { Ok::<_, StepFailure>(1) },
            StepOptions::new(),
        ))
        .build();
    claim_and_drain(&worker, 1).await;
    assert!(h.paths().last().unwrap().ends_with("/complete"), "{:?}", h.paths());
    let reported = h.reported();
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert!(reported[0].contains("on_step_start") && reported[0].contains("fell over"));
}

#[tokio::test]
async fn stats_after_a_completed_step_a_fatal_failure_a_fenced_conflict_and_an_unservable_lease() {
    let h = Harness::new(
        vec![claimed(vec![
            lease(json!({"lease_id": "lease-1"})),
            lease(json!({"lease_id": "lease-2", "step_key": "explode"})),
            lease(json!({"lease_id": "lease-3"})),
            lease(json!({"lease_id": "lease-4", "definition_id": "billing"})),
        ])],
        vec![
            ("/lease-1/started", vec![started()]),
            ("/lease-1/complete", vec![acked()]),
            ("/lease-2/started", vec![started()]),
            ("/lease-2/fail", vec![acked()]),
            ("/lease-3/started", vec![conflict("fenced", json!({}))]),
            ("/lease-4/fail", vec![acked()]),
        ],
    )
    .await;
    let worker = h.worker(
        Workflow::new("support")
            .step(
                "classify",
                |_ctx: StepContext| async move { Ok::<_, StepFailure>(1) },
                StepOptions::new(),
            )
            .step(
                "explode",
                |_ctx: StepContext| async move { Err::<i32, _>(StepFailure::fatal("boom")) },
                StepOptions::new(),
            ),
    );
    let before = worker.stats();
    assert_eq!(before, mcpd_sdk::WorkerStats::default());
    claim_and_drain(&worker, 4).await;
    let stats = worker.stats();
    assert_eq!(stats.claimed, 4);
    assert_eq!(stats.inflight, 0);
    assert_eq!(stats.completed, 1);
    assert_eq!(stats.failed, std::collections::HashMap::from([("fatal".to_owned(), 1)]));
    assert_eq!(stats.conflicts, std::collections::HashMap::from([("fenced".to_owned(), 1)]));
    assert_eq!(stats.unservable, 1);
}

/// Counts the events whose message contains `needle`, so a test can see what
/// the default reporter wrote.
struct Counting {
    needle: &'static str,
    hits: Arc<std::sync::atomic::AtomicUsize>,
}

impl tracing::Subscriber for Counting {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Message(String);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}");
                }
            }
        }
        let mut message = Message(String::new());
        event.record(&mut message);
        if message.0.contains(self.needle) {
            self.hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[tokio::test]
async fn without_on_error_one_line_per_kind_of_failure_per_interval_whatever_the_step() {
    // Three leases this worker cannot serve, each with its own step key, and a
    // daemon that refuses every `fail`. Every report used to be written, so a
    // failing run logged one line per step.
    let leases = (1..=3)
        .map(|n| {
            lease(json!({"lease_id": format!("lease-{n}"), "step_key": format!("unknown_{n}")}))
        })
        .collect();
    let h = Harness::new(
        vec![claimed(leases)],
        vec![(
            "/fail",
            vec![Reply::Body(400, r#"{"error":"bad_request","message":"no"}"#.to_owned())],
        )],
    )
    .await;
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let _guard = tracing::subscriber::set_default(Counting {
        needle: "call failed for",
        hits: Arc::clone(&hits),
    });
    let worker = Worker::builder(&h.mcpd)
        .id("w-1")
        .wait(Duration::ZERO)
        .concurrency(3)
        .register(Workflow::new("support"))
        .build();
    claim_and_drain(&worker, 3).await;
    assert_eq!(h.paths().iter().filter(|p| p.ends_with("/fail")).count(), 3);
    assert_eq!(hits.load(std::sync::atomic::Ordering::Relaxed), 1);
}
