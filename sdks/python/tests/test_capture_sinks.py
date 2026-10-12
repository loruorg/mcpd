"""Payload capture, `record`, the external parent, sinks and `stats` (contract sections 2.1 to 3)."""

import json
import os
import unittest

from mcpd import (
    ConsoleSink,
    Mcpd,
    RecordingSink,
    SpanKind,
    SpanStatus,
    SpanRecord,
    records,
)
from mcpd.context import EPOCH, nanotime
from mcpd.transport import Transport
from tests.test_client import FakeTransport
from tests.test_transport import FakeSender

TRACE = "0af7651916cd43dd8448eb211c80319c"
SPAN = "b7ad6b7169203331"
PARENT = "c3" * 8


def recorder(**kwargs):
    """A client whose batches land in a `RecordingSink`, flushed by hand."""
    sink = RecordingSink()
    kwargs.setdefault("flush_interval", 10.0)
    kwargs.setdefault("project", "tests")
    return Mcpd("http://mcpd.test", sink=sink, transport=FakeTransport(), **kwargs), sink


def finished(name="imported", **overrides):
    """A span record as an adapter for another tracing system would hold it."""
    fields = dict(
        trace_id=TRACE,
        span_id=SPAN,
        parent_span_id=PARENT,
        name=name,
        kind=SpanKind.TOOL_CALL,
        status=SpanStatus.OK,
        start_time=nanotime(1_700_000_000_000_000_000),
        end_time=nanotime(1_700_000_000_250_000_000),
        duration_ns=250_000_000,
        is_root=False,
        is_event=False,
        input={"q": "secret"},
        output={"a": "also secret"},
        correlation=records.Correlation(project="theirs", session_id="s-9"),
        received_at=EPOCH,
    )
    fields.update(overrides)
    return SpanRecord(**fields)


class CapturePayloadsTest(unittest.TestCase):
    def test_off_in_code_keeps_input_and_output_off_every_record(self):
        c, sink = recorder(capture_payloads=False)
        with c.trace("run", input={"prompt": "from the start"}) as run:
            run.set_input({"prompt": "set later"}).set_output("answer")
            run.set_model("m", "p").set_usage(input_tokens=3, output_tokens=1)
            run.set_model_parameters(temperature=0, seed=0, stop_sequences=["STOP"])
            run.event("chunk", input="a token", output="another")
            with c.span("child", output="child output") as child:
                child.set_input("child input")
        self.assertTrue(c.flush(2))
        # A start and an end that meet in the queue share one record (B8).
        self.assertEqual(sorted(s.name for s in sink.spans), ["child", "chunk", "run"])
        for span in sink.spans:
            self.assertIsNone(span.input, span.name)
            self.assertIsNone(span.output, span.name)
        final = [s for s in sink.spans if s.name == "run" and s.end_time is not None][0]
        # The rest of the record is untouched.
        self.assertIs(final.status, SpanStatus.OK)
        self.assertEqual(final.model, "m")
        self.assertEqual(final.usage.input_tokens, 3)
        # Sampling settings are metadata, not payload: no prompt is in them,
        # and they are what the run has to be reproduced from. Capture off
        # must not take them, and a zero seed must survive.
        self.assertEqual(final.model_parameters.seed, 0)
        self.assertEqual(final.model_parameters.temperature, 0.0)
        self.assertEqual(final.model_parameters.stop_sequences, ["STOP"])
        self.assertFalse(final.model_parameters.has_response_format)
        self.assertEqual(final.correlation.project, "tests")
        self.assertFalse(run.captures_payloads)
        c.shutdown()

    def test_off_in_the_environment(self):
        os.environ["MCPD_CAPTURE_PAYLOADS"] = "off"
        try:
            c, sink = recorder()
        finally:
            del os.environ["MCPD_CAPTURE_PAYLOADS"]
        self.assertFalse(c.capture_payloads)
        with c.trace("run") as run:
            run.set_input("secret")
        c.flush(2)
        self.assertTrue(all(s.input is None for s in sink.spans))
        c.shutdown()

    def test_a_span_overrides_the_client_in_either_direction(self):
        off, sink = recorder(capture_payloads=False)
        with off.trace("kept", capture_payloads=True) as kept:
            kept.set_input("visible")
            with off.span("dropped") as dropped:
                dropped.set_input("not visible")  # the override is for one span only
        off.flush(2)
        by_name = {s.name: s for s in sink.spans if s.end_time is not None}
        self.assertEqual(by_name["kept"].input, "visible")
        self.assertIsNone(by_name["dropped"].input)
        off.shutdown()

        on, sink = recorder()
        with on.trace("kept") as kept:
            kept.set_input("visible")
            with on.span("dropped", capture_payloads=False, input="never queued") as dropped:
                dropped.set_output("nor this")
        on.flush(2)
        by_name = {s.name: s for s in sink.spans if s.end_time is not None}
        self.assertEqual(by_name["kept"].input, "visible")
        self.assertIsNone(by_name["dropped"].input)
        self.assertIsNone(by_name["dropped"].output)
        on.shutdown()

    def test_a_recorded_span_is_stripped_like_any_other(self):
        c, sink = recorder(capture_payloads=False)
        self.assertTrue(c.record(finished()))
        c.flush(2)
        self.assertEqual(len(sink.spans), 1)
        self.assertIsNone(sink.spans[0].input)
        self.assertIsNone(sink.spans[0].output)
        self.assertEqual(sink.spans[0].name, "imported")
        c.shutdown()


class ExternalParentTest(unittest.TestCase):
    def test_the_external_parent_reaches_the_wire_and_not_the_metadata(self):
        c, sink = recorder()
        with c.trace("run", external_parent_span_id="ab" * 8) as run:
            run.set_output("linked")
        c.flush(2)
        span = [s for s in sink.spans if s.end_time is not None][0]
        self.assertEqual(span.external_parent_span_id, "ab" * 8)
        self.assertIsNone(span.parent_span_id, "an external parent never parents inside mcpd's tree")
        self.assertTrue(span.is_root)
        self.assertIsNone(span.metadata)
        c.shutdown()

    def test_it_is_spelled_snake_case_on_the_wire(self):
        sender = FakeSender([])
        transport = Transport("http://mcpd.test", sender=sender)
        c = Mcpd("http://mcpd.test", project="tests", transport=transport, flush_interval=10.0)
        c.span("child", external_parent_span_id="ab" * 8).end()
        c.flush(2)
        body = json.loads(sender.calls[0]["body"])
        self.assertEqual(body["spans"][0]["external_parent_span_id"], "ab" * 8)
        c.shutdown()


class RecordTest(unittest.TestCase):
    def test_a_well_formed_span_is_queued_as_given(self):
        c, sink = recorder(environment="staging", sample=0.0)  # never re-sampled: sample 0 still queues
        self.assertTrue(c.record(finished()))
        c.flush(2)
        self.assertEqual(len(sink.spans), 1)
        got = sink.spans[0]
        self.assertEqual((got.trace_id, got.span_id, got.parent_span_id), (TRACE, SPAN, PARENT))
        self.assertEqual(got.start_time, nanotime(1_700_000_000_000_000_000))
        self.assertEqual(got.end_time, nanotime(1_700_000_000_250_000_000))
        self.assertEqual(got.duration_ns, 250_000_000)
        self.assertEqual(got.input, {"q": "secret"})
        # Set fields are kept; empty ones are filled from the client.
        self.assertEqual(got.correlation.project, "theirs")
        self.assertEqual(got.correlation.session_id, "s-9")
        self.assertEqual(got.correlation.environment, "staging")
        self.assertEqual(c.stats().rejected, 0)
        c.shutdown()

    def test_each_malformed_shape_is_counted_not_raised(self):
        c, sink = recorder()
        bad = [
            finished(trace_id="0AF7651916CD43DD8448EB211C80319C"),
            finished(trace_id="0af7651916cd43dd"),
            finished(trace_id="0" * 32),
            finished(span_id="B7AD6B7169203331"),
            finished(span_id="0" * 16),
            finished(span_id="b7ad6b71692033311"),
            finished(parent_span_id="zz" * 8),
            finished(parent_span_id="0" * 16),
            finished(name=""),
            finished(name="   "),
            finished(start_time=None),
        ]
        for record in bad:
            self.assertFalse(c.record(record))
        self.assertFalse(c.record("not a span"))
        c.flush(2)
        self.assertEqual(sink.spans, [])
        self.assertEqual(c.stats().rejected, len(bad) + 1)
        self.assertEqual(c.stats().exported, 0)
        c.shutdown()

    def test_a_disabled_client_drops_it_without_counting_a_rejection(self):
        sink = RecordingSink()
        c = Mcpd("http://mcpd.test", enabled=False, sink=sink, transport=FakeTransport())
        self.assertFalse(c.record(finished()))
        c.flush(1)
        self.assertEqual(sink.spans, [])
        self.assertEqual(c.stats().rejected, 0)
        c.shutdown()


class SinkTest(unittest.TestCase):
    def test_a_recording_sink_sees_exactly_what_was_recorded(self):
        c, sink = recorder(batch_size=100)
        with c.trace("run") as run:
            run.set_usage(input_tokens=1)
            c.log("info", "hello")
            c.metric("latency", 3.5, "gauge")
            run.score("judge", 0.5, min=0, max=1)
            run.feedback("thumbs", "up")
        self.assertTrue(c.flush(2))
        self.assertEqual(sink.batches, 1)
        self.assertEqual([s.name for s in sink.spans], ["run"])
        self.assertEqual(sink.spans[0].usage.input_tokens, 1)
        self.assertEqual([l.message for l in sink.logs], ["hello"])
        self.assertEqual([m.name for m in sink.metrics], ["latency"])
        self.assertEqual([s.scorer_id for s in sink.scores], ["judge"])
        self.assertEqual([f.value for f in sink.feedback], ["up"])
        self.assertEqual(c.stats().exported, 5)
        sink.clear()
        self.assertEqual((sink.batches, sink.spans, sink.logs), (0, [], []))
        c.shutdown()

    def test_a_sink_that_raises_is_a_failed_batch_the_caller_never_sees(self):
        class Broken:
            def export(self, batch, ingest_key):
                raise RuntimeError("no")

        c = Mcpd("http://mcpd.test", project="tests", sink=Broken(), transport=FakeTransport(),
                 flush_interval=10.0)
        with c.trace("run"):
            pass
        self.assertTrue(c.flush(2))  # flush completes; nothing raised
        stats = c.stats()
        self.assertEqual(stats.failed, 1)
        self.assertEqual(stats.exported, 0)
        self.assertEqual(stats.queued, 0)
        c.shutdown()

    def test_flush_and_shutdown_return_by_their_deadline_when_the_sink_never_answers(self):
        # Against a black-holed daemon every batch costs three attempts of the
        # full request timeout. A bound on batches is no bound on time.
        import time

        calls = []

        class Slow:
            def export(self, batch, ingest_key):
                calls.append(ingest_key)
                time.sleep(0.4)

        c = Mcpd("http://mcpd.test", project="tests", sink=Slow(), transport=FakeTransport(),
                 batch_size=1, flush_interval=10.0)
        for i in range(5):
            c.log("info", f"line {i}")

        self.assertFalse(c.flush(0.05), "a flush that ran out of time says so")
        started = time.monotonic()
        c.shutdown(0.2)
        took = time.monotonic() - started
        self.assertLess(took, 0.35, "one deadline covers flushing and stopping together")

        # The batch in flight finishes and is counted; no further batch starts.
        time.sleep(1.0)
        self.assertEqual(len(calls), 1)
        self.assertEqual(c.stats().exported, 1)
        self.assertEqual(c.stats().queued, 4, "what was given up stays visible as queued")

    def test_a_console_sink_writes_one_wire_shaped_json_line_per_signal(self):
        lines = []
        c = Mcpd("http://mcpd.test", project="tests", sink=ConsoleSink(write=lines.append),
                 transport=FakeTransport(), flush_interval=10.0)
        with c.trace("run") as run:
            run.set_input({"q": 1})
            c.log("warn", "careful")
        c.flush(2)
        parsed = [json.loads(line) for line in lines]
        self.assertTrue(all(line.endswith("\n") for line in lines))
        self.assertEqual([p["signal"] for p in parsed], ["span", "log"])
        self.assertEqual(parsed[0]["trace_id"], run.trace_id)
        self.assertEqual(parsed[0]["input"], {"q": 1})
        self.assertNotIn("traceId", parsed[0])
        self.assertEqual(parsed[1]["message"], "careful")
        c.shutdown()

    def test_the_transport_is_the_default_sink_and_carries_the_key(self):
        sender = FakeSender([])
        transport = Transport("http://mcpd.test", sender=sender)
        c = Mcpd("http://mcpd.test", project="tests", transport=transport, flush_interval=10.0)
        c.trace("run").end()
        c.flush(2)
        body = json.loads(sender.calls[0]["body"])
        self.assertRegex(body["ingest_key"], r"\A[0-9a-f]{32}\Z")
        c.shutdown()


class StatsTest(unittest.TestCase):
    def test_the_six_counters(self):
        c, sink = recorder(max_queue=2)
        self.assertEqual(c.stats().capacity, 2)
        c.trace("a").end()
        c.trace("b").end()
        c.trace("c").end()  # evicts the oldest
        self.assertEqual(c.stats().queued, 2)
        self.assertEqual(c.stats().dropped, 1)
        self.assertFalse(c.record(finished(name="")))
        self.assertEqual(c.stats().rejected, 1)
        c.flush(2)
        self.assertEqual(c.stats().queued, 0)
        self.assertEqual(c.stats().exported, 2)
        self.assertEqual(c.stats().failed, 0)
        c.shutdown()
