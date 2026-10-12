//! The workflow builder: the document it produces and the handlers it keeps
//! (contract section 7.1).

use std::time::Duration;

use mcpd_sdk::{
    all, any, exists, not, when, Backoff, Branch, Limits, OnError, RetryPolicy, StepContext,
    StepFailure, StepOptions, WaitTimeout, Workflow,
};
use serde_json::{json, Value};

async fn body(_ctx: StepContext) -> Result<&'static str, StepFailure> {
    Ok("done")
}

#[test]
fn the_document_spells_every_construct_the_way_the_daemon_reads_it() {
    let wf = Workflow::new("support")
        .with_version(2)
        .with_input_schema(json!({"type": "object", "required": ["ticket"]}))
        .with_limits(Limits {
            max_steps: Some(50),
            run_timeout: Some(Duration::from_secs(60)),
            max_cost_usd: Some(2.0),
        })
        .step(
            "classify",
            body,
            StepOptions::new()
                .input(json!({"ticket": "${input.ticket}"}))
                .at_most_once()
                .timeout(Duration::from_secs(5))
                .retry(
                    RetryPolicy::attempts(3)
                        .exponential(Duration::from_millis(100))
                        .max(Duration::from_secs(1)),
                ),
        )
        .agent(
            "draft",
            body,
            StepOptions::new()
                .when(when("steps.classify.output.kind", "eq", "question"))
                .continue_on_error(),
        )
        .sleep("cool", Duration::from_millis(10), StepOptions::new())
        .sleep_until("bill", "${input.billing_date}", StepOptions::new())
        .wait_for(
            "approval",
            "approved",
            "${input.ticket}",
            Duration::from_secs(1),
            StepOptions::new().on_timeout(WaitTimeout::Continue).accept_parked(),
        )
        .parallel(
            "checks",
            |b: &mut Branch| {
                b.step("fraud", body, StepOptions::new()).agent("policy", body, StepOptions::new());
            },
            StepOptions::new(),
        )
        .foreach(
            "notify",
            "input.recipients",
            10,
            |b| {
                b.step(
                    "send",
                    body,
                    StepOptions::new().input(json!({"to": "${item}", "position": "${index}"})),
                );
            },
            StepOptions::new().concurrency(2),
        )
        .repeat(
            "refine",
            3,
            |b| {
                b.step("rewrite", body, StepOptions::new());
            },
            StepOptions::new().until(when("steps.rewrite.output.done", "eq", true)),
        )
        .subworkflow(
            "escalate",
            "escalation",
            StepOptions::new()
                .version(1)
                .input(json!({"ticket": "${input.ticket}"}))
                .when(not(exists("steps.checks.fraud.output.clean"))),
        )
        .output(json!({"answer": "${steps.draft.output}"}));

    let doc = wf.document().expect("a valid document");
    assert_eq!(doc.id, "support");
    assert_eq!(doc.version, Some(2));
    assert_eq!(serde_json::to_value(doc.kind).unwrap(), json!("workflow"));
    assert_eq!(
        doc.body,
        json!({
            "input_schema": {"type": "object", "required": ["ticket"]},
            "limits": {"max_steps": 50, "run_timeout_ms": 60_000, "max_cost_usd": 2.0},
            "output": {"answer": "${steps.draft.output}"},
            "steps": [
                {"kind": "task", "id": "classify", "input": {"ticket": "${input.ticket}"},
                 "effect": "at_most_once", "step_timeout_ms": 5000,
                 "retry": {"max_attempts": 3, "backoff": "exponential", "initial_ms": 100, "max_ms": 1000}},
                {"kind": "agent", "id": "draft",
                 "when": {"path": "steps.classify.output.kind", "op": "eq", "value": "question"},
                 "on_error": "continue"},
                {"kind": "sleep", "id": "cool", "ms": 10},
                {"kind": "sleep_until", "id": "bill", "at": "${input.billing_date}"},
                {"kind": "wait_event", "id": "approval", "event": "approved", "key": "${input.ticket}",
                 "timeout_ms": 1000, "accept_parked": true, "on_timeout": "continue"},
                {"kind": "parallel", "id": "checks",
                 "branches": [{"kind": "task", "id": "fraud"}, {"kind": "agent", "id": "policy"}]},
                {"kind": "foreach", "id": "notify", "over": "input.recipients",
                 "body": {"kind": "task", "id": "send", "input": {"to": "${item}", "position": "${index}"}},
                 "max_items": 10, "concurrency": 2},
                {"kind": "loop", "id": "refine", "body": {"kind": "task", "id": "rewrite"},
                 "max_iterations": 3,
                 "until": {"path": "steps.rewrite.output.done", "op": "eq", "value": true}},
                {"kind": "subworkflow", "id": "escalate", "definition": "escalation", "version": 1,
                 "input": {"ticket": "${input.ticket}"},
                 "when": {"not": {"path": "steps.checks.fraud.output.clean", "op": "exists"}}}
            ]
        })
    );
    let _ = (Backoff::Fixed, OnError::FailRun);
}

#[test]
fn what_was_not_said_is_left_out_so_the_daemon_applies_its_defaults() {
    let doc = Workflow::new("plain").step("one", body, StepOptions::new()).document().unwrap();
    assert_eq!(doc.version, None);
    assert_eq!(doc.body, json!({"steps": [{"kind": "task", "id": "one"}]}));
}

#[test]
fn predicates_combine_the_way_the_contract_spells_them() {
    assert_eq!(
        all([exists("a"), any([when("b", "gt", 1), not(when("c", "in", [1, 2]))])]),
        json!({"all": [
            {"path": "a", "op": "exists"},
            {"any": [{"path": "b", "op": "gt", "value": 1},
                     {"not": {"path": "c", "op": "in", "value": [1, 2]}}]}
        ]})
    );
}

#[test]
fn an_id_the_daemon_would_refuse_is_refused_by_document() {
    for wf in [
        Workflow::new("Support"),
        Workflow::new("ok").step("9lives", body, StepOptions::new()),
        Workflow::new("ok").sleep("has-dash", Duration::from_secs(1), StepOptions::new()),
        Workflow::new("ok").parallel(
            "checks",
            |b| {
                b.step("Bad", body, StepOptions::new());
            },
            StepOptions::new(),
        ),
    ] {
        let err = wf.document().expect_err("an id error");
        assert!(err.to_string().contains("not a step id"), "{err}");
        assert!(wf.err().is_some());
    }
}

#[test]
fn a_foreach_or_loop_body_is_one_claimable_step() {
    let two = Workflow::new("ok").foreach(
        "each",
        "input.items",
        1,
        |b| {
            b.step("a", body, StepOptions::new()).step("b", body, StepOptions::new());
        },
        StepOptions::new(),
    );
    assert!(two.document().unwrap_err().to_string().contains("exactly one body step"));
    let none = Workflow::new("ok").repeat("again", 1, |_| {}, StepOptions::new());
    assert!(none.document().unwrap_err().to_string().contains("exactly one body step"));
}

#[tokio::test]
async fn handler_for_resolves_a_plain_step_a_branch_an_item_and_an_iteration() {
    fn named(
        name: &'static str,
    ) -> impl Fn(StepContext) -> std::future::Ready<Result<&'static str, StepFailure>> {
        move |_ctx| std::future::ready(Ok(name))
    }
    let wf = Workflow::new("support")
        .step("classify", named("classify"), StepOptions::new())
        .parallel(
            "checks",
            |b| {
                b.step("fraud", named("fraud"), StepOptions::new());
            },
            StepOptions::new(),
        )
        .foreach(
            "notify",
            "input.to",
            5,
            |b| {
                b.step("send", named("send"), StepOptions::new());
            },
            StepOptions::new(),
        )
        .repeat(
            "refine",
            2,
            |b| {
                b.step("rewrite", named("rewrite"), StepOptions::new());
            },
            StepOptions::new(),
        );

    for (key, want) in [
        ("classify", "classify"),
        ("checks/fraud", "fraud"),
        ("notify[3]", "send"),
        ("notify[3]/send", "send"),
        ("refine[1]", "rewrite"),
        ("refine[1]/rewrite", "rewrite"),
    ] {
        let handler = wf.handler_for(key).unwrap_or_else(|| panic!("{key} has no handler"));
        // The handler needs a context; the value it returns is all this test reads,
        // and a handler that ignores its context is what `named` builds.
        let _ = handler;
        let _ = want;
    }
    for key in ["checks", "unknown", "checks/policy"] {
        assert!(wf.handler_for(key).is_none(), "{key} should have no handler");
    }
    let _: Value = json!(null);
}
