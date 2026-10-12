"""End-to-end against a real mcpd.

Skipped unless `MCPD_TEST_ENDPOINT` names a running server, so the suite still
runs offline:

    mcpd demo --data /tmp/mcpd-test.db --listen 127.0.0.1:4317
    MCPD_TEST_ENDPOINT=http://127.0.0.1:4317 python3 -m unittest discover

`MCPD_REQUIRE_LIVE=1` turns a missing endpoint into a failure rather than a
skip. CI sets it in the job that starts a daemon, so if the variable is ever
renamed or stops reaching the tests the job goes red instead of quietly
skipping. The Rust SDK's live suite spent a long time reporting passes having
contacted no server at all, and this is the guard against a repeat.
"""

import os
import time
import unittest

from mcpd import FeedbackKind, Mcpd, McpdError, SpanKind, SpanStatus, Worker, new_trace_id, workflow

ENDPOINT = os.environ.get("MCPD_TEST_ENDPOINT")
REQUIRE_LIVE = os.environ.get("MCPD_REQUIRE_LIVE")


class LiveIsRequiredTest(unittest.TestCase):
    """The guard that stops a skipped live suite from reading as a pass."""

    @unittest.skipIf(
        ENDPOINT or not REQUIRE_LIVE,
        "the guard only bites when MCPD_REQUIRE_LIVE is set without an endpoint",
    )
    def test_the_live_suite_was_required_to_reach_a_daemon(self):
        self.fail(
            "MCPD_REQUIRE_LIVE is set but MCPD_TEST_ENDPOINT is not, so the live "
            "suite would have skipped without reaching a server. That is the "
            "exact silence this variable exists to break."
        )


@unittest.skipUnless(ENDPOINT, "set MCPD_TEST_ENDPOINT to run the live tests")
class LiveTest(unittest.TestCase):
    def setUp(self):
        self.project = "sdk-python-test"
        self.mcpd = Mcpd(ENDPOINT, project=self.project, environment="test",
                         service_name="sdk-test", flush_interval=0.05)
        self.addCleanup(self.mcpd.shutdown)

    def test_the_server_is_the_version_this_sdk_was_generated_from(self):
        health = self.mcpd.health()
        self.assertEqual(health.status, "ok")
        self.assertEqual(health.spec_version, "1.0.0")

    def test_a_whole_run_round_trips(self):
        mcpd = self.mcpd
        with mcpd.trace("support_agent", tags=["live-test"], session_id="live-1") as run:
            run.set_input({"question": "where is my order?"})
            with mcpd.span("answer", kind=SpanKind.MODEL_GENERATION) as gen:
                gen.set_model("gpt-4o-mini", "openai")
                gen.set_usage(input_tokens=812, output_tokens=96)
                gen.set_output("It ships tomorrow.")
            with mcpd.span("lookup_order", kind=SpanKind.TOOL_CALL) as tool:
                tool.set_error(TimeoutError("the orders API did not answer"))
            mcpd.log("info", "answered", {"tool_calls": 1})
            mcpd.metric("support.answers", 1, "counter", {"outcome": "ok"})
            run.set_output("It ships tomorrow.")
            trace_id = run.trace_id
        mcpd.score("relevance", 0.82, min=0.0, max=1.0, trace_id=trace_id, reason="cited the order")
        mcpd.feedback(FeedbackKind.THUMBS, "up", trace_id=trace_id, comment="fixed it")
        self.assertTrue(mcpd.flush(10))
        self.assertEqual(mcpd.dropped, 0)
        self.assertEqual(mcpd.failures, 0)

        detail = mcpd.get_trace(trace_id, include=["logs", "scores", "feedback"])
        self.assertEqual(detail.trace.name, "support_agent")
        self.assertEqual(detail.trace.tags, ["live-test"])
        names = {span.name: span for span in detail.spans}
        self.assertEqual(set(names), {"support_agent", "answer", "lookup_order"})
        self.assertIs(names["lookup_order"].status, SpanStatus.ERROR)
        self.assertIn("orders API", names["lookup_order"].error.message)
        self.assertEqual(names["answer"].parent_span_id, detail.trace.root_span_id)
        # The server priced the model call from its own price book.
        self.assertIsNotNone(detail.trace.cost)
        self.assertGreater(detail.trace.cost.total_cost, 0)
        self.assertEqual(detail.trace.usage.input_tokens, 812)
        self.assertEqual(len(detail.logs), 1)
        self.assertEqual(detail.scores[0].scorer_id, "relevance")
        self.assertEqual(detail.feedback[0].value, "up")

        listed = mcpd.list_traces(limit=5, from_="-1h")
        self.assertTrue(any(t.trace_id == trace_id for t in listed.traces))
        summary = mcpd.analytics_summary("-1h", "-0s")
        self.assertGreaterEqual(summary.trace_count, 1)
        cost = mcpd.analytics_cost("-1h", "-0s", group_by="model")
        self.assertGreaterEqual(cost.total_cost, 0)
        spans = mcpd.list_spans(trace_id=trace_id, limit=10)
        self.assertEqual(len(spans.spans), 3)
        logs = mcpd.list_logs(trace_id=trace_id)
        self.assertEqual(len(logs.logs), 1)
        scores = mcpd.list_scores(trace_id=trace_id)
        self.assertEqual(len(scores.scores), 1)

    def test_an_unfinished_run_is_visible_before_it_ends(self):
        mcpd = self.mcpd
        span = mcpd.trace("long_running")
        self.assertTrue(mcpd.flush(10))
        detail = mcpd.get_trace(span.trace_id)
        self.assertIsNone(detail.spans[0].end_time)
        self.assertFalse(detail.trace.is_complete)
        span.end()
        self.assertTrue(mcpd.flush(10))
        detail = mcpd.get_trace(span.trace_id)
        self.assertIsNotNone(detail.spans[0].end_time)
        self.assertTrue(detail.trace.is_complete)

    def test_a_batch_replayed_with_the_same_key_is_not_counted_twice(self):
        from mcpd import IngestBatch
        from mcpd.transport import Transport

        transport = Transport(ENDPOINT)
        self.addCleanup(transport.close)
        with self.mcpd.trace("idempotent") as span:
            pass
        # A fresh key per run: the server remembers the ones it has seen, which
        # is exactly the behaviour under test.
        batch = IngestBatch(spans=[span.to_record()], ingest_key=new_trace_id())
        first = transport.ingest(batch)
        second = transport.ingest(batch)
        self.assertEqual(first.accepted, 1)
        self.assertEqual(second.duplicate, 1)
        self.assertEqual(second.accepted, 0)

    def test_a_bad_id_is_reported_rather_than_stored(self):
        from mcpd import IngestBatch
        from mcpd.transport import Transport

        transport = Transport(ENDPOINT)
        self.addCleanup(transport.close)
        with self.mcpd.trace("bad") as span:
            pass
        record = span.to_record()
        record.span_id = "not-hex"
        result = transport.ingest(IngestBatch(spans=[record]))
        self.assertEqual(result.rejected, 1)
        self.assertEqual(result.errors[0].index, 0)


if __name__ == "__main__":
    unittest.main()


@unittest.skipUnless(ENDPOINT, "set MCPD_TEST_ENDPOINT to run the live tests")
class LiveRunTest(unittest.TestCase):
    """The run surface against a real daemon: register, start, serve, read back."""

    def setUp(self):
        self.project = f"sdk-python-runs-{new_trace_id()[:8]}"
        self.mcpd = Mcpd(ENDPOINT, project=self.project, environment="test",
                         service_name="sdk-test", flush_interval=0.05)
        self.addCleanup(self.mcpd.shutdown)

    def worker(self, wf, worker_id):
        def loud(message, error):
            raise AssertionError(f"{message}: {error}")
        worker = Worker(self.mcpd, worker_id=worker_id, wait_ms=500, on_error=loud).register(wf)
        worker.register_definitions()
        return worker

    def drive(self, worker, run_id):
        detail = self.mcpd.runs.get(run_id)
        for _ in range(10):
            if detail.run.status != "running":
                break
            worker.claim(4, run_id)
            worker.drain()
            detail = self.mcpd.runs.get(run_id)
        return detail

    def test_a_workflow_is_served_and_the_run_and_its_trace_read_back(self):
        wf = workflow("live_support")

        @wf.step("classify")
        def classify(ctx, input, prior):
            return {"kind": "question" if input["ticket"].startswith("Q") else "bug"}

        @wf.agent("draft", input={"kind": "${steps.classify.output.kind}"})
        def draft(ctx, input, prior):
            ctx.set_usage(input_tokens=120, output_tokens=40)
            ctx.set_state(drafted=True)
            with ctx.span("live_model") as span:
                span.set_model("claude-sonnet-4", "anthropic")
                span.set_usage(input_tokens=120, output_tokens=40)
            return f"an answer to a {input['kind']}"

        wf.output({"answer": "${steps.draft.output}"})
        worker = self.worker(wf, "live-worker")

        run = self.mcpd.runs.start(wf, input={"ticket": "Q-42"})
        self.assertEqual(run.status, "running")
        self.assertEqual(run.definition_id, "live_support")

        detail = self.drive(worker, run.run_id)
        self.assertEqual(detail.run.status, "succeeded")
        self.assertEqual(detail.run.output, {"answer": "an answer to a question"})
        self.assertEqual(detail.run.state, {"drafted": True})
        by_key = {a.step_key: a for a in detail.attempts}
        self.assertEqual(by_key["classify"].status, "succeeded")
        self.assertEqual(by_key["draft"].status, "succeeded")
        self.assertEqual(by_key["draft"].kind, "agent")
        self.assertEqual(by_key["draft"].input, {"kind": "question"})
        self.assertEqual(by_key["draft"].usage.input_tokens, 120)

        kinds = [event.kind for event in self.mcpd.runs.watch(run.run_id, poll=0.05)]
        self.assertEqual(kinds[0], "run_started")
        self.assertIn("step_finished", kinds)
        self.assertEqual(kinds[-1], "run_ended")

        self.assertTrue(self.mcpd.flush(10))
        # The daemon writes the run's own spans through an outbox drained on
        # its own clock, so give it a moment before reading the trace.
        names = []
        for _ in range(40):
            try:
                trace = self.mcpd.get_trace(run.trace_id)
            except McpdError:
                trace = None
            names = [s.name for s in trace.spans] if trace else []
            if "live_model" in names and len(names) >= 4:
                break
            time.sleep(0.25)
        self.assertIn("live_model", names)
        trace = self.mcpd.get_trace(run.trace_id)
        model = next(s for s in trace.spans if s.name == "live_model")
        self.assertEqual(model.parent_span_id, by_key["draft"].span_id)

    def test_a_wait_resumes_on_publish_and_another_run_ends_on_cancel(self):
        wf = (
            workflow("live_approval")
            .wait_for("approval", event="approved", key="${input.ticket}", timeout_ms=60_000)
            .step("close", lambda ctx, input, prior: {"closed_by": input["by"]},
                  input={"by": "${steps.approval.output.by}"})
            .output("${steps.close.output}")
        )
        worker = self.worker(wf, "live-approver")

        waiting = self.mcpd.runs.start("live_approval", input={"ticket": "T-7"})
        detail = self.mcpd.runs.get(waiting.run_id)
        for _ in range(10):
            if detail.waits:
                break
            time.sleep(0.1)
            detail = self.mcpd.runs.get(waiting.run_id)
        self.assertEqual([(w.event, w.key) for w in detail.waits], [("approved", "T-7")])

        receipt = self.mcpd.events.publish("approved", "T-7", payload={"by": "ops"}, event_id="live-approval-1")
        self.assertEqual(receipt.outcome, "delivered")
        again = self.mcpd.events.publish("approved", "T-7", event_id="live-approval-1")
        self.assertEqual(again.outcome, "duplicate")

        detail = self.drive(worker, waiting.run_id)
        self.assertEqual(detail.run.status, "succeeded")
        self.assertEqual(detail.run.output, {"closed_by": "ops"})

        doomed = self.mcpd.runs.start("live_approval", input={"ticket": "T-8"})
        cancelled = self.mcpd.runs.cancel(doomed.run_id)
        self.assertEqual(cancelled.status, "cancelled")
        kinds = [event.kind for event in self.mcpd.runs.watch(doomed.run_id, poll=0.05)]
        self.assertEqual(kinds[-1], "run_ended")

        listed = self.mcpd.runs.list(definition_id="live_approval")
        self.assertEqual(sorted(r.run_id for r in listed.runs), sorted([doomed.run_id, waiting.run_id]))
