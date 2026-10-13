"""`mcpd.testing.TestClient` (contract 2.5)."""

import os
import re
import unittest
from unittest import mock

from mcpd import Mcpd, SpanKind, SpanRecord, SpanStatus, records
from mcpd.context import EPOCH, nanotime
from mcpd.testing import TestClient

TRACE = "0af7651916cd43dd8448eb211c80319c"
BASE = 1_700_000_000_000_000_000


def answer(mcpd):
    with mcpd.span("answer", kind="agent_run") as run:
        with mcpd.span("lookup", kind="tool_call"):
            pass
        with mcpd.span("generate", kind="model_generation") as gen:
            gen.set_output("it ships tomorrow")
        mcpd.log("info", "answered", {"order": "A-1"})
        mcpd.metric("answers", 1)
        mcpd.score("relevance", 0.9, min=0, max=1)
        mcpd.feedback("thumbs", 1, trace_id=run.trace_id)
        run.set_output("done")


class TestClientTest(unittest.TestCase):
    def test_is_an_mcpd_and_reads_every_signal_with_no_flush(self):
        mcpd = TestClient(flush_interval=10.0)
        self.assertIsInstance(mcpd, Mcpd)
        answer(mcpd)
        self.assertEqual([s.name for s in mcpd.spans()], ["answer", "lookup", "generate"])
        self.assertEqual([entry.message for entry in mcpd.logs()], ["answered"])
        self.assertEqual([m.name for m in mcpd.metrics()], ["answers"])
        self.assertEqual(mcpd.scores()[0].scorer_id, "relevance")
        self.assertEqual(mcpd.recorded_feedback()[0].kind.value, "thumbs")

    def test_one_record_per_span_id_finished_only(self):
        mcpd = TestClient(flush_interval=10.0)
        still_open = mcpd.trace("still_running")
        with mcpd.span("answer") as run:
            # The start record leaves before the end record, so both arrive.
            mcpd.flush()
            run.set_output("done")
        spans = mcpd.spans()
        self.assertEqual(len(mcpd.recorder.spans), 3)
        self.assertEqual([s.name for s in spans], ["answer"])
        self.assertEqual(spans[0].output, "done")
        still_open.end()
        self.assertEqual([s.name for s in mcpd.spans()], ["still_running", "answer"])

    def test_filters_by_name_and_kind(self):
        mcpd = TestClient()
        answer(mcpd)
        self.assertEqual([s.name for s in mcpd.spans(kind="tool_call")], ["lookup"])
        self.assertEqual([s.name for s in mcpd.spans(kind=SpanKind.TOOL_CALL)], ["lookup"])
        self.assertEqual(mcpd.spans(name="generate")[0].output, "it ships tomorrow")

    def test_recorded_span_fails_on_none_and_on_two(self):
        mcpd = TestClient()
        answer(mcpd)
        with self.assertRaisesRegex(AssertionError, "found 0"):
            mcpd.recorded_span("missing")
        answer(mcpd)
        with self.assertRaisesRegex(AssertionError, re.escape("found 2; spans recorded: answer")):
            mcpd.recorded_span("lookup")

    def test_tree(self):
        mcpd = TestClient()
        answer(mcpd)
        trees = mcpd.trees()
        self.assertEqual(len(trees), 1)
        self.assertEqual(trees[0].span.name, "answer")
        self.assertEqual([c.span.name for c in trees[0].children], ["lookup", "generate"])

    def test_tree_orders_children_by_start_time_and_orphans_are_roots(self):
        mcpd = TestClient()

        def record(name, span_id, parent, start_ms):
            mcpd.record(SpanRecord(
                trace_id=TRACE, span_id=span_id, parent_span_id=parent, name=name,
                kind=SpanKind.GENERIC, status=SpanStatus.OK,
                start_time=nanotime(BASE + start_ms * 1_000_000),
                end_time=nanotime(BASE + (start_ms + 1) * 1_000_000),
                is_root=parent is None, is_event=False, received_at=EPOCH,
                correlation=records.Correlation(project="tests")))

        # Arrival order differs from start order at every level.
        record("second", "b" * 16, "a" * 16, 20)
        record("first", "c" * 16, "a" * 16, 10)
        record("orphan", "d" * 16, "f" * 16, 5)
        record("root", "a" * 16, None, 0)
        trees = mcpd.trees()
        self.assertEqual([t.span.name for t in trees], ["root", "orphan"])
        self.assertEqual([c.span.name for c in trees[0].children], ["first", "second"])

    def test_reset_forgets_everything(self):
        mcpd = TestClient()
        answer(mcpd)
        mcpd.reset()
        self.assertEqual(mcpd.spans(), [])
        self.assertEqual(mcpd.logs(), [])
        self.assertEqual(mcpd.scores(), [])
        self.assertEqual(mcpd.recorded_feedback(), [])

    def test_redaction_applies(self):
        def redact(value):
            return value.replace("tomorrow", "[date]") if isinstance(value, str) else value

        mcpd = TestClient(redact=redact)
        answer(mcpd)
        self.assertEqual(mcpd.recorded_span("generate").output, "it ships [date]")

    def test_capture_payloads_applies(self):
        mcpd = TestClient(capture_payloads=False)
        answer(mcpd)
        self.assertIsNone(mcpd.recorded_span("generate").output)

    def test_stays_on_when_the_environment_turns_mcpd_off(self):
        with mock.patch.dict(os.environ, {"MCPD_ENABLED": "false", "MCPD_SAMPLE": "0"}):
            mcpd = TestClient()
            answer(mcpd)
            self.assertEqual(len(mcpd.spans()), 3)
            self.assertTrue(mcpd.enabled)
            self.assertEqual(mcpd.sample, 1.0)

    def test_code_still_wins(self):
        mcpd = TestClient(enabled=False)
        answer(mcpd)
        self.assertEqual(mcpd.spans(), [])

    def test_refuses_a_sink(self):
        with self.assertRaises(TypeError):
            TestClient(sink=object())


if __name__ == "__main__":
    unittest.main()
