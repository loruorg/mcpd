//! Two things the run core has to be able to answer about a run that went
//! wrong, both from what the journal and the pinned definition already hold.
//!
//! The first is the one a status code cannot answer. When a step fails, the
//! ancestor that produced the value it choked on has usually **succeeded**: it
//! returned syntactically valid output that was semantically wrong, so its
//! status is `succeeded` and no search over statuses will ever reach it. The
//! tests below assert that the walk marks that ancestor, and marks it as green
//! rather than hiding it among the failures.
//!
//! The second is the breaker. A run whose same step key keeps failing with an
//! identical rendered input is making no progress, and two of the daemon's own
//! paths retry without spending any retry budget, so nothing else stops it.

use mcpd_runs::cause::ValueKind;
use mcpd_runs::model::{AttemptStatus, DefinitionKind, RunStatus, StepErrorClass};
use mcpd_runs::store::{
    DefinitionDocument, RunConfig, RunStore, StartRun, StepCompletion, StepError,
};
use mcpd_runs::SqliteRunStore;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use time::{Duration, OffsetDateTime};

fn t0() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("a valid instant")
}

fn at(secs: i64) -> OffsetDateTime {
    t0() + Duration::seconds(secs)
}

/// A pid alone is not unique: pids are recycled, and a recycled one reopens
/// the previous run's database, so every count is off by a whole prior suite.
static STAMP: AtomicU64 = AtomicU64::new(0);

fn temp_db() -> PathBuf {
    let n = STAMP.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("mcpd-cause-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join("runs.db")
}

fn done(output: Value) -> StepCompletion {
    StepCompletion { output: Some(output), ..Default::default() }
}

fn failure(class: StepErrorClass, message: &str) -> StepError {
    StepError { class, message: message.into(), details: None, retry_after_ms: None }
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

    async fn define(&self, id: &str, body: Value) {
        self.store
            .put_definition(DefinitionDocument {
                project: "shop".into(),
                id: id.into(),
                kind: DefinitionKind::Workflow,
                version: Some(1),
                body,
            })
            .await
            .expect("put definition");
    }

    async fn start(&self, id: &str, input: Value) -> String {
        self.store
            .start_run(
                StartRun {
                    project: "shop".into(),
                    definition_id: id.into(),
                    definition_version: Some(1),
                    input: Some(input),
                    correlation: None,
                    idempotency_key: None,
                },
                t0(),
            )
            .await
            .expect("start run")
            .run_id
    }

    async fn claim_one(&self, run_id: &str, now: OffsetDateTime) -> mcpd_runs::model::Lease {
        let mut leases =
            self.store.claim("shop", "worker-1", 1, Some(run_id), now).await.expect("claim");
        assert_eq!(leases.len(), 1, "expected one lease for run {run_id}");
        leases.remove(0)
    }

    async fn claim_all(&self, run_id: &str, now: OffsetDateTime) -> Vec<mcpd_runs::model::Lease> {
        self.store.claim("shop", "worker-1", 20, Some(run_id), now).await.expect("claim")
    }

    /// Run one step to a successful end, whatever its key.
    async fn finish(&self, run_id: &str, output: Value, now: OffsetDateTime) -> String {
        let lease = self.claim_one(run_id, now).await;
        let key = lease.step_key.clone();
        self.store.complete_step(&lease.lease_id, done(output), now).await.expect("complete");
        key
    }

    async fn cause(&self, run_id: &str, step: &str) -> mcpd_runs::cause::StepCause {
        self.store.step_cause(run_id, step, 32).await.expect("cause")
    }

    async fn run(&self, run_id: &str) -> mcpd_runs::model::Run {
        self.store.get_run(run_id).await.expect("run")
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// The link for one `(step_key, template_path)` pair, so an assertion names
/// the edge it is about rather than an offset into a list.
fn link<'a>(
    cause: &'a mcpd_runs::cause::StepCause,
    step_key: &str,
    path: &str,
) -> &'a mcpd_runs::cause::CauseLink {
    cause.chain.iter().find(|l| l.step_key == step_key && l.template_path == path).unwrap_or_else(
        || {
            panic!(
                "no link for {step_key} through {path}; the chain holds {:?}",
                cause
                    .chain
                    .iter()
                    .map(|l| (l.step_key.as_str(), l.template_path.as_str()))
                    .collect::<Vec<_>>()
            )
        },
    )
}

// ---------------------------------------------------------------------------
// A. The green ancestor
// ---------------------------------------------------------------------------

/// The whole point of the feature. `classify` succeeds and hands back an empty
/// list, `enrich` succeeds passing it along, `charge` fails on it. Every step
/// in the chain is green, so no status-based search reaches `classify`, and
/// the walk has to.
#[tokio::test]
async fn the_ancestor_that_produced_the_bad_value_succeeded() {
    let h = Harness::open().await;
    h.define(
        "triage",
        json!({"steps": [
            {"kind": "task", "id": "classify"},
            {"kind": "task", "id": "enrich",
             "input": {"kinds": "${steps.classify.output.kinds}"}},
            {"kind": "task", "id": "charge",
             "input": {"lines": "${steps.enrich.output.lines}"}}
        ]}),
    )
    .await;
    let run_id = h.start("triage", json!({"order": "ord_1"})).await;

    h.finish(&run_id, json!({"kinds": []}), t0()).await;
    h.finish(&run_id, json!({"lines": []}), at(1)).await;
    let lease = h.claim_one(&run_id, at(2)).await;
    h.store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "no lines to charge"), at(2))
        .await
        .expect("fail");

    let cause = h.cause(&run_id, "charge").await;
    assert_eq!(cause.step_key, "charge");
    assert_eq!(cause.status, AttemptStatus::Failed);
    assert_eq!(cause.attempts, 1);
    assert_eq!(
        cause.error.as_ref().map(|e| e.message.as_str()),
        Some("no lines to charge"),
        "the answer carries why the step itself failed"
    );
    assert!(!cause.truncated);

    let direct = link(&cause, "enrich", "steps.enrich.output.lines");
    assert_eq!(direct.depth, 1);
    assert_eq!(direct.consumed_by, "charge");
    assert_eq!(direct.status, AttemptStatus::Succeeded);
    assert!(direct.silent_producer, "enrich is green and sits behind nothing that failed");
    assert_eq!(direct.value_kind, Some(ValueKind::Array));
    assert_eq!(direct.value_empty, Some(true), "it handed over an empty list");

    let deep = link(&cause, "classify", "steps.classify.output.kinds");
    assert_eq!(deep.depth, 2);
    assert_eq!(deep.consumed_by, "enrich");
    assert_eq!(deep.status, AttemptStatus::Succeeded);
    assert!(deep.silent_producer, "the whole path back to classify is green");
    assert_eq!(deep.value_kind, Some(ValueKind::Array));
    assert_eq!(deep.value_empty, Some(true));

    // Depth ascending, and every hop named.
    assert_eq!(
        cause.chain.iter().map(|l| (l.step_key.as_str(), l.depth)).collect::<Vec<_>>(),
        vec![("enrich", 1), ("classify", 2)]
    );
}

/// `silent_producer` is not `status == succeeded`. An ancestor sitting behind
/// a step that did fail is not silent: that failure already explains the run,
/// and marking the ancestor would point a reader at the wrong step.
#[tokio::test]
async fn an_ancestor_behind_a_failed_step_is_not_a_silent_producer() {
    let h = Harness::open().await;
    h.define(
        "chain",
        json!({"steps": [
            {"kind": "task", "id": "fetch"},
            {"kind": "task", "id": "parse", "on_error": "continue",
             "input": {"body": "${steps.fetch.output.body}"}},
            {"kind": "task", "id": "store", "when": {"path": "run.run_id", "op": "exists"},
             "input": {"rows": "${steps.parse.output.rows}"}}
        ]}),
    )
    .await;
    let run_id = h.start("chain", json!({})).await;

    h.finish(&run_id, json!({"body": "<html>"}), t0()).await;
    let lease = h.claim_one(&run_id, at(1)).await;
    h.store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "not json"), at(1))
        .await
        .expect("fail parse");
    let lease = h.claim_one(&run_id, at(2)).await;
    assert_eq!(lease.step_key, "store");
    h.store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "no rows"), at(2))
        .await
        .expect("fail store");

    let cause = h.cause(&run_id, "store").await;
    let parse = link(&cause, "parse", "steps.parse.output.rows");
    assert_eq!(parse.status, AttemptStatus::Failed);
    assert!(!parse.silent_producer, "a failed ancestor is not silent");
    assert_eq!(parse.value_kind, None, "a failed step has no output to read the shape of");
    assert_eq!(parse.value_empty, None);

    let fetch = link(&cause, "fetch", "steps.fetch.output.body");
    assert_eq!(fetch.status, AttemptStatus::Succeeded);
    assert!(
        !fetch.silent_producer,
        "fetch is green but sits behind parse, which failed and already explains this run"
    );
    assert_eq!(fetch.value_kind, Some(ValueKind::String));
    assert_eq!(fetch.value_empty, Some(false));
}

/// A guard is a data dependency too: `when` decides whether the step runs at
/// all, so the step it reads belongs in the chain.
#[tokio::test]
async fn a_guard_declares_an_edge_as_much_as_an_input_does() {
    let h = Harness::open().await;
    h.define(
        "gated",
        json!({"steps": [
            {"kind": "task", "id": "score"},
            {"kind": "task", "id": "review",
             "when": {"path": "steps.score.output.risk", "op": "gt", "value": 0.5}}
        ]}),
    )
    .await;
    let run_id = h.start("gated", json!({})).await;
    h.finish(&run_id, json!({"risk": 0.9}), t0()).await;
    let lease = h.claim_one(&run_id, at(1)).await;
    h.store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "reviewer offline"), at(1))
        .await
        .expect("fail");

    let cause = h.cause(&run_id, "review").await;
    let score = link(&cause, "score", "steps.score.output.risk");
    assert_eq!(score.depth, 1);
    assert!(score.silent_producer);
    assert_eq!(score.value_kind, Some(ValueKind::Number));
    assert_eq!(score.value_empty, Some(false));
}

/// A template that reads `input` names no ancestor. The chain is empty and the
/// answer says so, rather than inventing a hop.
#[tokio::test]
async fn a_step_that_reads_only_the_run_input_has_an_empty_chain() {
    let h = Harness::open().await;
    h.define(
        "flat",
        json!({"steps": [
            {"kind": "task", "id": "only", "input": {"id": "${input.order_id}"}}
        ]}),
    )
    .await;
    let run_id = h.start("flat", json!({"order_id": "ord_9"})).await;
    let lease = h.claim_one(&run_id, t0()).await;
    h.store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "gone"), t0())
        .await
        .expect("fail");

    let cause = h.cause(&run_id, "only").await;
    assert!(cause.chain.is_empty(), "input is not a step: {:?}", cause.chain);
    assert!(!cause.truncated);
}

/// A `foreach` item key carries an index, and the array it fanned out over is
/// itself an edge: the item exists at all because an earlier step produced it.
#[tokio::test]
async fn a_foreach_item_names_its_index_and_the_step_the_array_came_from() {
    let h = Harness::open().await;
    h.define(
        "fanout",
        json!({"steps": [
            {"kind": "task", "id": "listing"},
            {"kind": "foreach", "id": "handle", "over": "steps.listing.output.skus",
             "max_items": 8, "concurrency": 8,
             "body": {"kind": "task", "id": "one", "input": {"sku": "${item}"}}}
        ]}),
    )
    .await;
    let run_id = h.start("fanout", json!({})).await;
    h.finish(&run_id, json!({"skus": ["a", "b"]}), t0()).await;

    let leases = h.claim_all(&run_id, at(1)).await;
    assert_eq!(leases.len(), 2);
    let target = leases.iter().find(|l| l.step_key == "handle[1]").expect("second item");
    h.store
        .fail_step(&target.lease_id, failure(StepErrorClass::Fatal, "no such sku"), at(1))
        .await
        .expect("fail");

    let cause = h.cause(&run_id, "handle[1]").await;
    assert_eq!(cause.step_key, "handle[1]");
    let listing = link(&cause, "listing", "steps.listing.output.skus");
    assert_eq!(listing.depth, 1);
    assert_eq!(listing.consumed_by, "handle[1]");
    assert!(listing.silent_producer);
    assert_eq!(listing.value_kind, Some(ValueKind::Array));
    assert_eq!(listing.value_empty, Some(false));
}

/// A `parallel` branch key carries a slash, and a sibling branch read by name
/// is a real edge.
#[tokio::test]
async fn a_parallel_branch_reads_a_sibling_by_its_nested_name() {
    let h = Harness::open().await;
    h.define(
        "split",
        json!({"steps": [
            {"kind": "parallel", "id": "checks", "branches": [
                {"kind": "task", "id": "fraud"},
                {"kind": "task", "id": "stock"}
            ]},
            {"kind": "task", "id": "decide",
             "input": {"f": "${steps.checks.fraud.output.score}"}}
        ]}),
    )
    .await;
    let run_id = h.start("split", json!({})).await;
    let leases = h.claim_all(&run_id, t0()).await;
    for lease in leases {
        let out = json!({"score": 0.1});
        h.store.complete_step(&lease.lease_id, done(out), t0()).await.expect("complete");
    }
    let lease = h.claim_one(&run_id, at(1)).await;
    assert_eq!(lease.step_key, "decide");
    h.store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "undecidable"), at(1))
        .await
        .expect("fail");

    let cause = h.cause(&run_id, "decide").await;
    let fraud = link(&cause, "checks/fraud", "steps.checks.fraud.output.score");
    assert_eq!(fraud.depth, 1);
    assert_eq!(fraud.status, AttemptStatus::Succeeded);
    assert!(fraud.silent_producer);
    assert_eq!(fraud.value_kind, Some(ValueKind::Number));
    assert!(
        !cause.chain.iter().any(|l| l.step_key == "checks/stock"),
        "the sibling nobody read is not an ancestor"
    );
}

/// A loop iteration reads the iteration before it, so the chain walks
/// backwards through the loop and stops when the walk runs out of iterations.
#[tokio::test]
async fn a_loop_iteration_reads_the_iteration_before_it() {
    let h = Harness::open().await;
    h.define(
        "refine",
        json!({"steps": [
            {"kind": "loop", "id": "polish", "max_iterations": 3,
             "body": {"kind": "task", "id": "pass",
                      "input": {"prev": "${steps.polish.outputs[0]}"}}}
        ]}),
    )
    .await;
    let run_id = h.start("refine", json!({})).await;
    let first = h.finish(&run_id, json!({"draft": "one"}), t0()).await;
    assert_eq!(first, "polish[0]/pass");

    let lease = h.claim_one(&run_id, at(1)).await;
    assert_eq!(lease.step_key, "polish[1]/pass");
    h.store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "no better"), at(1))
        .await
        .expect("fail");

    let cause = h.cause(&run_id, "polish[1]/pass").await;
    let prev = link(&cause, "polish[0]/pass", "steps.polish.outputs[0]");
    assert_eq!(prev.depth, 1);
    assert_eq!(prev.status, AttemptStatus::Succeeded);
    assert!(prev.silent_producer);
    assert_eq!(prev.value_kind, Some(ValueKind::Object));
    assert_eq!(prev.value_empty, Some(false));
}

/// A subworkflow is a step of its parent with no attempt row of its own: its
/// status and output come from the child run. The boundary is a hop in the
/// chain like any other, and the walk does not cross into the child.
#[tokio::test]
async fn a_subworkflow_boundary_is_one_hop_read_from_the_child_run() {
    let h = Harness::open().await;
    h.define(
        "child",
        json!({"steps": [{"kind": "task", "id": "inner"}],
                             "output": {"total": "${steps.inner.output.total}"}}),
    )
    .await;
    h.define(
        "parent",
        json!({"steps": [
            {"kind": "subworkflow", "id": "price", "definition": "child", "version": 1},
            {"kind": "task", "id": "bill", "input": {"total": "${steps.price.output.total}"}}
        ]}),
    )
    .await;
    let run_id = h.start("parent", json!({})).await;

    // The child's own step, claimed through the project rather than the run.
    let mut child = h.store.claim("shop", "worker-1", 1, None, t0()).await.expect("claim child");
    assert_eq!(child.len(), 1);
    let child = child.remove(0);
    assert_ne!(child.run_id, run_id);
    h.store.complete_step(&child.lease_id, done(json!({"total": 0})), t0()).await.expect("child");

    let lease = h.claim_one(&run_id, at(1)).await;
    assert_eq!(lease.step_key, "bill");
    h.store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "nothing to bill"), at(1))
        .await
        .expect("fail");

    let cause = h.cause(&run_id, "bill").await;
    let price = link(&cause, "price", "steps.price.output.total");
    assert_eq!(price.depth, 1);
    assert_eq!(price.status, AttemptStatus::Succeeded, "read from the child run's own status");
    assert!(price.silent_producer);
    assert_eq!(price.value_kind, Some(ValueKind::Number));
    assert_eq!(
        cause.chain.len(),
        1,
        "the walk stops at the boundary; the child's steps are the child's own answer"
    );
}

/// The bound is a bound. A chain longer than `max_depth` comes back as a
/// prefix, and says so.
#[tokio::test]
async fn max_depth_truncates_and_says_so() {
    let h = Harness::open().await;
    h.define(
        "ladder",
        json!({"steps": [
            {"kind": "task", "id": "a"},
            {"kind": "task", "id": "b", "input": {"v": "${steps.a.output.v}"}},
            {"kind": "task", "id": "c", "input": {"v": "${steps.b.output.v}"}},
            {"kind": "task", "id": "d", "input": {"v": "${steps.c.output.v}"}}
        ]}),
    )
    .await;
    let run_id = h.start("ladder", json!({})).await;
    for i in 0..3 {
        h.finish(&run_id, json!({"v": i}), at(i)).await;
    }
    let lease = h.claim_one(&run_id, at(4)).await;
    h.store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "end"), at(4))
        .await
        .expect("fail");

    let whole = h.store.step_cause(&run_id, "d", 32).await.expect("whole");
    assert_eq!(whole.chain.len(), 3);
    assert!(!whole.truncated);

    let cut = h.store.step_cause(&run_id, "d", 2).await.expect("cut");
    assert_eq!(cut.chain.iter().map(|l| l.step_key.as_str()).collect::<Vec<_>>(), vec!["c", "b"]);
    assert!(cut.truncated, "the walk stopped with an ancestor left unvisited");

    let none = h.store.step_cause(&run_id, "d", 0).await.expect("zero");
    assert!(none.chain.is_empty());
    assert!(none.truncated);
}

/// Two calls against one run answer the same list in the same order.
#[tokio::test]
async fn the_answer_is_stable_across_calls() {
    let h = Harness::open().await;
    h.define(
        "wide",
        json!({"steps": [
            {"kind": "task", "id": "one"},
            {"kind": "task", "id": "two"},
            {"kind": "task", "id": "three",
             "input": {"b": "${steps.two.output.v}", "a": "${steps.one.output.v}",
                       "both": "${steps.one.output.v} and ${steps.two.output.v}"}}
        ]}),
    )
    .await;
    let run_id = h.start("wide", json!({})).await;
    h.finish(&run_id, json!({"v": 1}), t0()).await;
    h.finish(&run_id, json!({"v": 2}), at(1)).await;
    let lease = h.claim_one(&run_id, at(2)).await;
    h.store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "no"), at(2))
        .await
        .expect("fail");

    let first = h.cause(&run_id, "three").await;
    let second = h.cause(&run_id, "three").await;
    assert_eq!(first.chain, second.chain);
    // One reference each, whatever order the rendered object put them in, and
    // a repeated reference counted once.
    assert_eq!(first.chain.len(), 2);
}

/// The endpoint answers about any step, not only a failed one, and refuses
/// what it cannot answer for: a step the definition never declared, and one
/// this run never reached.
#[tokio::test]
async fn it_answers_for_any_step_it_has_evidence_of_and_refuses_the_rest() {
    let h = Harness::open().await;
    h.define(
        "small",
        json!({"steps": [
            {"kind": "task", "id": "first"},
            {"kind": "task", "id": "second", "input": {"v": "${steps.first.output.v}"}},
            {"kind": "task", "id": "third", "input": {"v": "${steps.second.output.v}"}}
        ]}),
    )
    .await;
    let run_id = h.start("small", json!({})).await;
    h.finish(&run_id, json!({"v": 1}), t0()).await;

    let green = h.cause(&run_id, "first").await;
    assert_eq!(green.status, AttemptStatus::Succeeded);
    assert!(green.error.is_none());
    assert!(green.chain.is_empty());

    // Claimable but not yet claimed: the row exists, so the chain is already
    // answerable and the status says where it stands.
    let waiting = h.cause(&run_id, "second").await;
    assert_eq!(waiting.status, AttemptStatus::Pending);
    assert_eq!(waiting.chain.len(), 1);
    assert_eq!(waiting.chain[0].step_key, "first");

    let err = h.store.step_cause(&run_id, "nowhere", 32).await.expect_err("no such step");
    assert!(matches!(err, mcpd_runs::Error::NotFound { .. }), "{err}");

    // Declared, but this run has not reached it, so there is nothing to
    // answer about it.
    let err = h.store.step_cause(&run_id, "third", 32).await.expect_err("never reached");
    assert!(matches!(err, mcpd_runs::Error::NotFound { .. }), "{err}");
}

/// The wire says an ancestor read through two paths appears twice, once per
/// path, because two paths are two different claims about what went wrong. The
/// same path written twice is still one link.
#[tokio::test]
async fn one_ancestor_read_through_two_paths_is_two_links() {
    let h = Harness::open().await;
    h.define(
        "twice",
        json!({"steps": [
            {"kind": "task", "id": "source"},
            {"kind": "task", "id": "sink",
             "input": {"a": "${steps.source.output.kinds}",
                       "b": "${steps.source.output.total}",
                       "again": "${steps.source.output.kinds}"}}
        ]}),
    )
    .await;
    let run_id = h.start("twice", json!({})).await;
    h.finish(&run_id, json!({"kinds": [], "total": 0}), t0()).await;
    let lease = h.claim_one(&run_id, at(1)).await;
    h.store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "no"), at(1))
        .await
        .expect("fail");

    let cause = h.cause(&run_id, "sink").await;
    assert_eq!(cause.chain.len(), 2, "two paths, and the repeated one counted once");
    let kinds = link(&cause, "source", "steps.source.output.kinds");
    assert_eq!(kinds.value_kind, Some(ValueKind::Array));
    assert_eq!(kinds.value_empty, Some(true), "the empty one is the suspect");
    let total = link(&cause, "source", "steps.source.output.total");
    assert_eq!(total.value_kind, Some(ValueKind::Number));
    assert_eq!(total.value_empty, Some(false), "zero is a number, not an absence");
}

/// A definition may name a step that never produced anything, and two steps
/// may name each other. Neither is refused when the document is stored, so the
/// walk has to survive both: the first yields no link, and the second
/// terminates instead of walking round for ever.
#[tokio::test]
async fn the_walk_terminates_on_a_document_that_reads_itself_in_a_circle() {
    let h = Harness::open().await;
    h.define(
        "circular",
        json!({"steps": [
            {"kind": "task", "id": "first",
             "input": {"later": "${steps.second.output.v}", "ghost": "${steps.absent.output}"}},
            {"kind": "task", "id": "second", "input": {"earlier": "${steps.first.output.v}"}}
        ]}),
    )
    .await;
    let run_id = h.start("circular", json!({})).await;
    h.finish(&run_id, json!({"v": 1}), t0()).await;
    let lease = h.claim_one(&run_id, at(1)).await;
    assert_eq!(lease.step_key, "second");
    h.store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "loop"), at(1))
        .await
        .expect("fail");

    let cause = h.cause(&run_id, "second").await;
    // second reads first; first reads second, which is already on the walk,
    // and `absent`, which the document never declares.
    assert_eq!(
        cause.chain.iter().map(|l| l.step_key.as_str()).collect::<Vec<_>>(),
        vec!["first"],
        "{:?}",
        cause.chain
    );
    assert!(!cause.truncated, "it finished, it did not run out of depth");
}

// ---------------------------------------------------------------------------
// B. The breaker
// ---------------------------------------------------------------------------

/// An `unservable` failure spends no retry budget, so the step comes back
/// after one lease length for ever. The breaker is what ends it.
#[tokio::test]
async fn a_step_that_repeats_with_the_same_input_ends_the_run() {
    let h = Harness::with_config(RunConfig { max_step_repeats: 3, ..RunConfig::default() }).await;
    h.define("stuck", json!({"steps": [{"kind": "task", "id": "call"}]})).await;
    let run_id = h.start("stuck", json!({"q": "x"})).await;

    for i in 0..2 {
        let lease = h.claim_one(&run_id, at(i * 60)).await;
        let out = h
            .store
            .fail_step(&lease.lease_id, failure(StepErrorClass::Unservable, "not mine"), at(i * 60))
            .await
            .expect("fail");
        assert_eq!(out.run_status, RunStatus::Running, "still inside the breaker at {i}");
        h.store.run_due_timers(at(i * 60 + 31)).await.expect("requeue");
    }

    let lease = h.claim_one(&run_id, at(120)).await;
    let out = h
        .store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Unservable, "not mine"), at(120))
        .await
        .expect("fail");
    assert_eq!(out.run_status, RunStatus::Failed, "the third identical failure trips the breaker");

    let run = h.run(&run_id).await;
    let error = run.error.clone().unwrap_or_default();
    assert!(error.contains("max_step_repeats"), "{error}");
    assert!(error.contains("call"), "the error names the step: {error}");
    assert!(error.contains('3'), "the error names the count: {error}");
    assert!(
        h.store.timers(&run_id).await.expect("timers").is_empty(),
        "no retry is left counting down behind an ended run"
    );
}

/// The breaker counts identical inputs, not failures. One step key can fail
/// three times and still be making progress, because a sibling finishing
/// underneath it changes what its template renders to. A counter that only
/// looked at failures would have ended this run; the run then keeps going and
/// trips only once the input really does stop moving.
#[tokio::test]
async fn a_step_whose_input_changes_between_attempts_is_not_repeating() {
    let h = Harness::with_config(RunConfig { max_step_repeats: 3, ..RunConfig::default() }).await;
    h.define(
        "moving",
        json!({"steps": [
            {"kind": "parallel", "id": "checks", "branches": [
                {"kind": "task", "id": "slow"},
                {"kind": "task", "id": "reader",
                 "input": {"v": "${steps.checks.slow.output.v}"},
                 "retry": {"max_attempts": 20, "backoff": "fixed", "initial_ms": 1000}}
            ]}
        ]}),
    )
    .await;
    let run_id = h.start("moving", json!({})).await;

    let leases = h.claim_all(&run_id, t0()).await;
    let slow = leases.iter().find(|l| l.step_key == "checks/slow").expect("slow").clone();
    let reader = leases.iter().find(|l| l.step_key == "checks/reader").expect("reader");
    assert_eq!(reader.input, json!({"v": null}), "the sibling has produced nothing yet");

    // Two failures against the same rendered input.
    h.store
        .fail_step(&reader.lease_id, failure(StepErrorClass::Retryable, "empty"), t0())
        .await
        .expect("fail 1");
    h.store.run_due_timers(at(1)).await.expect("retry 1");
    let reader = h.claim_one(&run_id, at(1)).await;
    assert_eq!(reader.step_key, "checks/reader");
    h.store
        .fail_step(&reader.lease_id, failure(StepErrorClass::Retryable, "empty"), at(1))
        .await
        .expect("fail 2");

    // The sibling lands, so the next render of the same step is different.
    h.store.complete_step(&slow.lease_id, done(json!({"v": 7})), at(2)).await.expect("slow");
    h.store.run_due_timers(at(2)).await.expect("retry 2");
    let reader = h.claim_one(&run_id, at(2)).await;
    assert_eq!(reader.input, json!({"v": 7}), "the input moved");
    let out = h
        .store
        .fail_step(&reader.lease_id, failure(StepErrorClass::Retryable, "still bad"), at(2))
        .await
        .expect("fail 3");
    assert_eq!(
        out.run_status,
        RunStatus::Running,
        "three failures, but only two of them were the same piece of work"
    );

    // Now it really is repeating: two more against the input that stopped
    // moving reaches the ceiling of three.
    for i in 3..5 {
        h.store.run_due_timers(at(i)).await.expect("retry");
        let reader = h.claim_one(&run_id, at(i)).await;
        h.store
            .fail_step(&reader.lease_id, failure(StepErrorClass::Retryable, "still bad"), at(i))
            .await
            .expect("fail");
    }
    let run = h.run(&run_id).await;
    assert_eq!(run.status, RunStatus::Failed);
    assert!(run.error.unwrap_or_default().contains("max_step_repeats"));
}

/// Zero turns the breaker off, which is what an operator who wants the old
/// behaviour writes.
#[tokio::test]
async fn zero_turns_the_breaker_off() {
    let h = Harness::with_config(RunConfig { max_step_repeats: 0, ..RunConfig::default() }).await;
    h.define("stuck", json!({"steps": [{"kind": "task", "id": "call"}]})).await;
    let run_id = h.start("stuck", json!({})).await;
    for i in 0..6 {
        let lease = h.claim_one(&run_id, at(i * 60)).await;
        h.store
            .fail_step(&lease.lease_id, failure(StepErrorClass::Unservable, "not mine"), at(i * 60))
            .await
            .expect("fail");
        h.store.run_due_timers(at(i * 60 + 31)).await.expect("requeue");
    }
    assert_eq!(h.run(&run_id).await.status, RunStatus::Running);
}

/// A failure that ends or advances the run is not a repetition: the breaker
/// only stands in front of a retry.
#[tokio::test]
async fn the_breaker_does_not_touch_a_run_that_is_ending_anyway() {
    let h = Harness::with_config(RunConfig { max_step_repeats: 1, ..RunConfig::default() }).await;
    h.define("once", json!({"steps": [{"kind": "task", "id": "call"}]})).await;
    let run_id = h.start("once", json!({})).await;
    let lease = h.claim_one(&run_id, t0()).await;
    let out = h
        .store
        .fail_step(&lease.lease_id, failure(StepErrorClass::Fatal, "the real reason"), t0())
        .await
        .expect("fail");
    assert_eq!(out.run_status, RunStatus::Failed);
    let error = h.run(&run_id).await.error.unwrap_or_default();
    assert!(error.contains("the real reason"), "the step's own error survives: {error}");
    assert!(!error.contains("max_step_repeats"), "{error}");
}
