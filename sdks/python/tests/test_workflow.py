"""The workflow builder: the document it produces and the handlers it keeps (contract section 7.1)."""

import unittest

from mcpd import Limits, RetryPolicy, all_of, any_of, not_, when, workflow
from mcpd.transport import encode


def body(ctx, input, prior):
    return "done"


class DocumentTest(unittest.TestCase):
    def test_every_construct_is_spelled_the_way_the_daemon_reads_it(self):
        wf = (
            workflow("support", version=2, input_schema={"type": "object", "required": ["ticket"]},
                     limits=Limits(max_steps=50, run_timeout_ms=60_000, max_cost_usd=2))
            .step("classify", body, input={"ticket": "${input.ticket}"}, effect="at_most_once",
                  step_timeout_ms=5_000,
                  retry=RetryPolicy(max_attempts=3, backoff="exponential", initial_ms=100, max_ms=1_000))
            .agent("draft", body, when=when("steps.classify.output.kind", "eq", "question"), on_error="continue")
            .sleep("cool", 10)
            .sleep_until("bill", "${input.billing_date}")
            .wait_for("approval", event="approved", key="${input.ticket}", timeout_ms=1_000,
                      on_timeout="continue", accept_parked=True)
            .parallel("checks", lambda b: b.step("fraud", body).agent("policy", body))
            .foreach("notify", lambda b: b.step("send", body, input={"to": "${item}", "position": "${index}"}),
                     over="input.recipients", max_items=10, concurrency=2)
            .loop("refine", lambda b: b.step("rewrite", body), max_iterations=3,
                  until=when("steps.rewrite.output.done", "eq", True))
            .subworkflow("escalate", definition="escalation", version=1, input={"ticket": "${input.ticket}"},
                         when=not_(when("steps.checks.fraud.output.clean", "exists")))
            .output({"answer": "${steps.draft.output}"})
        )
        self.assertEqual(encode(wf.document()), {
            "id": "support",
            "kind": "workflow",
            "version": 2,
            "body": {
                "input_schema": {"type": "object", "required": ["ticket"]},
                "limits": {"max_steps": 50, "run_timeout_ms": 60_000, "max_cost_usd": 2},
                "output": {"answer": "${steps.draft.output}"},
                "steps": [
                    {"kind": "task", "id": "classify", "input": {"ticket": "${input.ticket}"},
                     "effect": "at_most_once", "step_timeout_ms": 5_000,
                     "retry": {"max_attempts": 3, "backoff": "exponential", "initial_ms": 100, "max_ms": 1_000}},
                    {"kind": "agent", "id": "draft",
                     "when": {"path": "steps.classify.output.kind", "op": "eq", "value": "question"},
                     "on_error": "continue"},
                    {"kind": "sleep", "id": "cool", "ms": 10},
                    {"kind": "sleep_until", "id": "bill", "at": "${input.billing_date}"},
                    {"kind": "wait_event", "id": "approval", "event": "approved", "key": "${input.ticket}",
                     "timeout_ms": 1_000, "accept_parked": True, "on_timeout": "continue"},
                    {"kind": "parallel", "id": "checks",
                     "branches": [{"kind": "task", "id": "fraud"}, {"kind": "agent", "id": "policy"}]},
                    {"kind": "foreach", "id": "notify", "over": "input.recipients",
                     "body": {"kind": "task", "id": "send", "input": {"to": "${item}", "position": "${index}"}},
                     "max_items": 10, "concurrency": 2},
                    {"kind": "loop", "id": "refine", "body": {"kind": "task", "id": "rewrite"},
                     "max_iterations": 3,
                     "until": {"path": "steps.rewrite.output.done", "op": "eq", "value": True}},
                    {"kind": "subworkflow", "id": "escalate", "definition": "escalation", "version": 1,
                     "input": {"ticket": "${input.ticket}"},
                     "when": {"not": {"path": "steps.checks.fraud.output.clean", "op": "exists"}}},
                ],
            },
        })

    def test_what_was_not_said_is_left_out_so_the_daemon_applies_its_defaults(self):
        doc = encode(workflow("plain").step("one", body).document())
        self.assertEqual(doc, {"id": "plain", "kind": "workflow", "body": {"steps": [{"kind": "task", "id": "one"}]}})

    def test_the_decorator_form_registers_the_same_step(self):
        wf = workflow("plain")

        @wf.step("classify", effect="at_most_once")
        def classify(ctx, input, prior):
            return "x"

        self.assertIs(wf.handler_for("classify"), classify)
        self.assertEqual(encode(wf.document())["body"]["steps"],
                         [{"kind": "task", "id": "classify", "effect": "at_most_once"}])

    def test_predicates_combine_the_way_the_contract_spells_them(self):
        self.assertEqual(
            all_of(when("a", "exists"), any_of(when("b", "gt", 1), not_(when("c", "in", [1, 2])))),
            {"all": [{"path": "a", "op": "exists"},
                     {"any": [{"path": "b", "op": "gt", "value": 1},
                              {"not": {"path": "c", "op": "in", "value": [1, 2]}}]}]},
        )

    def test_an_id_the_daemon_would_refuse_is_refused_here(self):
        with self.assertRaisesRegex(ValueError, "not a step id"):
            workflow("Support")
        with self.assertRaisesRegex(ValueError, "not a step id"):
            workflow("ok").step("9lives", body)
        with self.assertRaisesRegex(ValueError, "not a step id"):
            workflow("ok").sleep("has-dash", 1)

    def test_a_foreach_or_loop_body_is_one_claimable_step(self):
        with self.assertRaisesRegex(ValueError, "exactly one body step"):
            workflow("ok").foreach("each", lambda b: b.step("a", body).step("b", body), over="input.items",
                                   max_items=1)
        with self.assertRaisesRegex(ValueError, "exactly one body step"):
            workflow("ok").loop("again", lambda b: None, max_iterations=1)


class HandlerForTest(unittest.TestCase):
    def setUp(self):
        self.classify = lambda ctx, i, p: "classify"
        self.fraud = lambda ctx, i, p: "fraud"
        self.send = lambda ctx, i, p: "send"
        self.rewrite = lambda ctx, i, p: "rewrite"
        self.wf = (
            workflow("support")
            .step("classify", self.classify)
            .parallel("checks", lambda b: b.step("fraud", self.fraud))
            .foreach("notify", lambda b: b.step("send", self.send), over="input.to", max_items=5)
            .loop("refine", lambda b: b.step("rewrite", self.rewrite), max_iterations=2)
        )

    def test_a_plain_step_a_branch_an_item_and_an_iteration_resolve(self):
        self.assertIs(self.wf.handler_for("classify"), self.classify)
        self.assertIs(self.wf.handler_for("checks/fraud"), self.fraud)
        self.assertIs(self.wf.handler_for("notify[3]"), self.send)
        self.assertIs(self.wf.handler_for("notify[3]/send"), self.send)
        self.assertIs(self.wf.handler_for("refine[1]"), self.rewrite)
        self.assertIs(self.wf.handler_for("refine[1]/rewrite"), self.rewrite)

    def test_a_key_never_registered_has_nothing(self):
        self.assertIsNone(self.wf.handler_for("checks"))
        self.assertIsNone(self.wf.handler_for("unknown"))
        self.assertIsNone(self.wf.handler_for("checks/policy"))
