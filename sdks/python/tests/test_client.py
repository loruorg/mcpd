"""Client behaviour: the non-negotiables B1 to B10, against a fake transport."""

import asyncio
import logging
import threading
import time
import unittest

from mcpd import Mcpd, SpanKind, SpanStatus, new_trace_id, records
from mcpd.transport import TransportError, encode_params


class FakeTransport:
    """Collects batches instead of sending them, and can be told to fail."""

    def __init__(self, fail=False, block=None):
        self.batches = []
        self.fail = fail
        self.block = block
        self.closed = False
        self.lock = threading.Lock()

    def ingest(self, batch):
        if self.block is not None:
            self.block.wait(5)
        if self.fail:
            raise TransportError("the server is not there", retryable=True)
        with self.lock:
            self.batches.append(batch)
        signals = sum(len(getattr(batch, f) or []) for f in
                      ("spans", "logs", "metrics", "scores", "feedback"))
        return records.IngestResult(accepted=signals, rejected=0, duplicate=0)

    def close(self):
        self.closed = True

    def records_of(self, field="spans"):
        with self.lock:
            return [r for b in self.batches for r in (getattr(b, field) or [])]


def client(**kwargs):
    kwargs.setdefault("transport", FakeTransport())
    kwargs.setdefault("flush_interval", 0.02)
    kwargs.setdefault("project", "tests")
    return Mcpd("http://mcpd.test", **kwargs)


class TracingTest(unittest.TestCase):
    def test_a_scoped_span_ends_itself_and_reports_ok(self):
        c = client()
        with c.trace("run") as span:
            span.set_input({"q": "hi"})
        self.assertTrue(c.flush(2))
        final = [s for s in c._transport.records_of() if s.end_time is not None]
        self.assertEqual(len(final), 1)
        self.assertIs(final[0].status, SpanStatus.OK)
        self.assertEqual(final[0].input, {"q": "hi"})
        self.assertTrue(final[0].is_root)
        c.shutdown()

    def test_a_child_inherits_the_trace_and_names_its_parent(self):
        c = client()
        with c.trace("run") as root:
            with c.span("step", kind=SpanKind.TOOL_CALL) as step:
                self.assertEqual(step.trace_id, root.trace_id)
                self.assertEqual(step.parent_span_id, root.span_id)
                self.assertIs(c.current_span(), step)
            self.assertIs(c.current_span(), root, "the context is restored on the way out")
        self.assertIsNone(c.current_span())
        c.shutdown()

    def test_an_unfinished_span_is_still_sent(self):
        # B8: a run that crashes mid-flight is exactly the run you need to see.
        c = client()
        span = c.trace("long_running")
        self.assertTrue(c.flush(2))
        sent = c._transport.records_of()
        self.assertEqual(len(sent), 1)
        self.assertIsNone(sent[0].end_time)
        self.assertIs(sent[0].status, SpanStatus.UNSET)
        span.end()
        self.assertTrue(c.flush(2))
        self.assertEqual(len(c._transport.records_of()), 2, "the end updates the same span")
        self.assertIsNotNone(c._transport.records_of()[1].end_time)
        c.shutdown()

    def test_a_span_that_starts_and_ends_between_flushes_costs_one_record(self):
        c = client(flush_interval=60)
        with c.trace("quick"):
            pass
        self.assertTrue(c.flush(2))
        self.assertEqual(len(c._transport.records_of()), 1)
        c.shutdown()

    def test_an_exception_is_recorded_and_re_raised(self):
        c = client()
        with self.assertRaises(ValueError):
            with c.trace("run") as span:
                raise ValueError("no such order")
        c.flush(2)
        sent = c._transport.records_of()[-1]
        self.assertIs(sent.status, SpanStatus.ERROR)
        self.assertEqual(sent.error.message, "no such order")
        self.assertEqual(sent.error.kind, "ValueError")
        self.assertIn("ValueError", sent.error.stack)
        c.shutdown()

    def test_an_error_always_carries_a_message(self):
        # B10: "something failed" is not actionable, so an empty message is
        # replaced rather than sent.
        c = client()
        with c.trace("run") as span:
            span.set_error("", kind="Timeout")
            self.assertIs(span.status, SpanStatus.ERROR)
        c.flush(2)
        self.assertEqual(c._transport.records_of()[-1].error.message, "Timeout")
        c.shutdown()

    def test_end_is_idempotent(self):
        c = client()
        span = c.trace("run")
        span.end()
        first = span.to_record().end_time
        span.end(SpanStatus.ERROR)
        self.assertIs(span.status, SpanStatus.OK)
        self.assertEqual(span.to_record().end_time, first)
        c.shutdown()

    def test_an_event_is_a_point_in_time(self):
        c = client()
        with c.trace("run") as span:
            span.event("cache_miss", metadata={"key": "orders"})
        c.flush(2)
        events = [s for s in c._transport.records_of() if s.is_event]
        self.assertEqual(len(events), 1)
        self.assertIsNone(events[0].end_time)
        self.assertEqual(events[0].metadata, {"key": "orders"})
        c.shutdown()

    def test_correlation_flows_from_the_client_and_the_root(self):
        c = client(environment="staging", service_name="support")
        with c.trace("run", session_id="sess-1", user_id="u-9") as root:
            with c.span("step") as step:
                pass
        c.flush(2)
        for record in c._transport.records_of():
            self.assertEqual(record.correlation.project, "tests")
            self.assertEqual(record.correlation.environment, "staging")
            self.assertEqual(record.correlation.service_name, "support")
            self.assertEqual(record.correlation.session_id, "sess-1")
            self.assertEqual(record.correlation.user_id, "u-9")
        c.shutdown()

    def test_unknown_keyword_fields_land_in_metadata(self):
        # A typo must cost a misplaced key, never an exception in the caller.
        c = client()
        with c.trace("run", stuff="value"):
            pass
        c.flush(2)
        self.assertEqual(c._transport.records_of()[-1].metadata, {"stuff": "value"})
        c.shutdown()

    def test_usage_and_model_reach_the_wire(self):
        c = client()
        with c.span("generate", kind="model_generation", model="gpt-4o-mini",
                    provider="openai") as gen:
            gen.set_usage(input_tokens=812, output_tokens=96, cache_read=128)
        c.flush(2)
        sent = c._transport.records_of()[-1]
        self.assertEqual((sent.model, sent.provider), ("gpt-4o-mini", "openai"))
        self.assertEqual(sent.usage.input_tokens, 812)
        self.assertEqual(sent.usage.input_details.cache_read, 128)
        c.shutdown()

    def test_model_parameters_reach_the_wire(self):
        c = client()
        with c.span("generate", kind="model_generation") as gen:
            gen.set_model_parameters(
                temperature=0.2, top_p=0.95, top_k=40, max_tokens=1024,
                seed=7, stop_sequences=["\n\nHuman:"], has_response_format=True,
            )
        c.flush(2)
        sent = c._transport.records_of()[-1].model_parameters
        self.assertEqual(sent.temperature, 0.2)
        self.assertEqual(sent.top_p, 0.95)
        self.assertEqual(sent.top_k, 40)
        self.assertEqual(sent.max_tokens, 1024)
        self.assertEqual(sent.seed, 7)
        self.assertEqual(sent.stop_sequences, ["\n\nHuman:"])
        self.assertTrue(sent.has_response_format)
        c.shutdown()

    def test_a_zero_seed_is_a_seed(self):
        # The classic bug: reading 0 as "not given". A zero seed is what a
        # reproducible run is pinned to, and a zero temperature is the setting
        # most worth recording, so neither may be dropped as falsy.
        c = client()
        with c.span("generate") as gen:
            gen.set_model_parameters(temperature=0, seed=0, top_p=0)
        c.flush(2)
        sent = c._transport.records_of()[-1].model_parameters
        self.assertEqual(sent.seed, 0)
        self.assertEqual(sent.temperature, 0.0)
        self.assertEqual(sent.top_p, 0.0)
        self.assertFalse(sent.has_response_format)
        c.shutdown()

    def test_model_parameters_replace_so_the_record_describes_one_call(self):
        # Half of one request and half of another would describe a call nobody
        # made, so the second call wins outright.
        c = client()
        with c.span("generate") as gen:
            gen.set_model_parameters(temperature=0.1, seed=99)
            gen.set_model_parameters(max_tokens=256)
        c.flush(2)
        sent = c._transport.records_of()[-1].model_parameters
        self.assertEqual((sent.temperature, sent.seed, sent.max_tokens), (None, None, 256))
        c.shutdown()

    def test_timestamps_are_utc_with_nanosecond_resolution(self):
        c = client()
        with c.trace("run"):
            time.sleep(0.001)
        c.flush(2)
        sent = c._transport.records_of()[-1]
        self.assertEqual(sent.start_time.utcoffset().total_seconds(), 0.0)
        self.assertGreater(sent.duration_ns, 0)
        self.assertRegex(sent.start_time.isoformat_ns(), r"\.\d{9}Z\Z")
        c.shutdown()


class ContextPropagationTest(unittest.TestCase):
    def test_an_inbound_traceparent_is_continued(self):
        c = client()
        header = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"
        with c.trace("handler", traceparent=header) as span:
            self.assertEqual(span.trace_id, "0af7651916cd43dd8448eb211c80319c")
            self.assertEqual(span.parent_span_id, "b7ad6b7169203331")
            self.assertTrue(span.sampled)
            self.assertTrue(span.traceparent.startswith(
                "00-0af7651916cd43dd8448eb211c80319c-"))
            self.assertTrue(span.traceparent.endswith("-01"))
            self.assertEqual(c.traceparent(), span.traceparent)
        c.shutdown()

    def test_an_upstream_decision_to_drop_is_obeyed(self):
        c = client(sample=1.0)
        header = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-00"
        with c.trace("handler", traceparent=header) as span:
            self.assertFalse(span.sampled, "a trace some services drop is not a trace")
        c.flush(2)
        self.assertEqual(c._transport.records_of(), [])
        c.shutdown()

    def test_a_malformed_traceparent_starts_a_new_trace(self):
        c = client()
        with c.trace("handler", traceparent="garbage") as span:
            self.assertRegex(span.trace_id, r"\A[0-9a-f]{32}\Z")
            self.assertIsNone(span.parent_span_id)
        c.shutdown()

    def test_context_follows_an_await_into_a_task(self):
        async def run():
            c = client()
            async with c.trace("run") as root:
                async def step(i):
                    async with c.span("worker") as w:
                        await asyncio.sleep(0)
                        return w.parent_span_id, w.trace_id
                results = await asyncio.gather(*(step(i) for i in range(4)))
            await c.aflush(2)
            await c.ashutdown()
            return root, results

        root, results = asyncio.run(run())
        for parent_span_id, trace_id in results:
            self.assertEqual(parent_span_id, root.span_id)
            self.assertEqual(trace_id, root.trace_id)


class SamplingTest(unittest.TestCase):
    def test_the_decision_is_made_once_and_inherited(self):
        # B4: a half-recorded trace reads as a bug in the user's system.
        c = client(sample=1.0)
        with c.trace("run") as root:
            c.sample = 0.0  # a change mid-trace must not split it
            with c.span("step") as step:
                self.assertTrue(step.sampled)
                with step.child("deeper") as deep:
                    self.assertTrue(deep.sampled)
        c.flush(2)
        self.assertEqual(len(c._transport.records_of()), 3)
        c.shutdown()

    def test_sample_zero_records_nothing_but_still_returns_a_usable_span(self):
        c = client(sample=0.0)
        with c.trace("run") as span:
            span.set_input("x").set_output("y")
            self.assertFalse(span.sampled)
            self.assertRegex(span.trace_id, r"\A[0-9a-f]{32}\Z")
            self.assertTrue(span.traceparent.endswith("-00"))
        c.flush(2)
        self.assertEqual(c._transport.records_of(), [])
        c.shutdown()

    def test_the_decision_is_deterministic_for_a_trace_id(self):
        c = client(sample=0.5)
        trace_id = "0af7651916cd43dd8448eb211c80319c"
        self.assertEqual(c._decide(trace_id), c._decide(trace_id))
        kept = sum(c._decide(new_trace_id()) for _ in range(2000))
        self.assertTrue(800 < kept < 1200, f"a half sample kept {kept} of 2000")
        c.shutdown()


class SignalTest(unittest.TestCase):
    def test_logs_metrics_scores_and_feedback_correlate_to_the_active_span(self):
        c = client()
        with c.trace("run") as span:
            c.log("warn", "slow tool", {"tool": "orders"})
            c.metric("orders.lookup", 1, "counter", {"tool": "orders"}, unit="{call}")
            span.score("relevance", 0.82, min=0.0, max=1.0, reason="cited the order")
            span.feedback("thumbs", "up", comment="fixed it")
        c.flush(2)
        t = c._transport
        self.assertEqual(t.records_of("logs")[0].trace_id, span.trace_id)
        self.assertEqual(t.records_of("logs")[0].span_id, span.span_id)
        self.assertEqual(t.records_of("metrics")[0].trace_id, span.trace_id)
        self.assertEqual(t.records_of("scores")[0].value, 0.82)
        self.assertEqual(t.records_of("scores")[0].min, 0.0)
        self.assertEqual(t.records_of("feedback")[0].value, "up")
        c.shutdown()

    def test_a_score_with_no_source_is_live(self):
        # The same call once filtered differently depending on the language
        # that made it, because one SDK defaulted to offline.
        c = client()
        with c.trace("run") as span:
            span.score("relevance", 0.82, min=0.0, max=1.0)
        c.flush(2)
        self.assertIs(c._transport.records_of("scores")[0].source, records.ScoreSource.LIVE)
        c.shutdown()

    def test_a_score_must_declare_its_scale(self):
        c = client()
        with self.assertRaises(TypeError):
            c.score("relevance", 0.82)  # min and max are keyword-only and required
        c.shutdown()

    def test_a_score_with_no_target_is_dropped_not_raised(self):
        c = client()
        with self.assertLogs("mcpd", level=logging.WARNING) as logged:
            c.score("relevance", 1.0, min=0.0, max=1.0)
        self.assertIn("dropped", logged.output[0])
        c.flush(2)
        self.assertEqual(c._transport.records_of("scores"), [])
        c.shutdown()

    def test_every_signal_travels_in_one_batch(self):
        c = client(flush_interval=60)
        with c.trace("run"):
            c.log("info", "hello")
            c.metric("m", 1)
            c.score("s", 1, min=0, max=1)
            c.feedback("thumbs", "up")
        c.flush(2)
        batch = c._transport.batches[0]
        self.assertTrue(all(getattr(batch, f) for f in
                            ("spans", "logs", "metrics", "scores", "feedback")))
        self.assertIsNotNone(batch.ingest_key)
        c.shutdown()


class QueueBoundTest(unittest.TestCase):
    def test_the_queue_never_grows_past_its_bound(self):
        # B3: an unbounded buffer turns a network outage into an out-of-memory kill.
        held = threading.Event()
        transport = FakeTransport(block=held)
        c = Mcpd("http://mcpd.test", transport=transport, max_queue=16, flush_interval=60,
                 batch_size=4)
        try:
            with self.assertLogs("mcpd", level=logging.WARNING) as logged:
                spans = [c.trace(f"run-{i}") for i in range(200)]
            self.assertLessEqual(c.queue_size, 16)
            self.assertGreaterEqual(c.dropped, 180)
            self.assertIn("queue is full", logged.output[0])
            self.assertEqual(len(logged.output), 1, "the warning is throttled to one per interval")
        finally:
            held.set()
            c.shutdown(1)

    def test_recording_never_waits_for_a_blocked_transport(self):
        # B2: the worker is stuck inside the transport for as long as the
        # event is held. Everything recorded meanwhile has to return at once.
        held = threading.Event()
        c = Mcpd("http://mcpd.test", transport=FakeTransport(block=held), max_queue=10_000,
                 flush_interval=60, batch_size=1)
        try:
            with c.trace("the one that gets stuck"):
                pass
            c._ensure_worker()
            c._wake.set()
            time.sleep(0.05)
            started = time.monotonic()
            for i in range(200):
                with c.trace(f"run-{i}") as span:
                    span.set_input(i)
                    c.log("info", "inside")
            elapsed = time.monotonic() - started
            self.assertLess(elapsed, 0.5, f"recording 200 runs took {elapsed:.3f}s behind a "
                            "transport that was not answering")
            self.assertFalse(c.flush(0.1), "a flush with the transport held reports the timeout")
        finally:
            held.set()
            c.shutdown(1)

    def test_dropping_never_reaches_the_caller(self):
        c = Mcpd("http://mcpd.test", transport=FakeTransport(block=threading.Event()),
                 max_queue=1, flush_interval=60)
        for i in range(50):
            with c.trace(f"run-{i}") as span:
                span.set_input(i)
        self.assertGreater(c.dropped, 0)
        c.shutdown(0.2)


class BatchingTest(unittest.TestCase):
    def test_signals_are_sent_in_batches_of_at_most_batch_size(self):
        c = client(batch_size=32, flush_interval=60)
        for i in range(100):
            with c.trace(f"run-{i}"):
                pass
        self.assertTrue(c.flush(5))
        batches = c._transport.batches
        self.assertGreaterEqual(len(batches), 4)
        self.assertTrue(all(len(b.spans) <= 32 for b in batches))
        self.assertEqual(sum(len(b.spans) for b in batches), 100)
        self.assertEqual(len({b.ingest_key for b in batches}), len(batches),
                         "each batch carries its own idempotency key")
        c.shutdown()


class DisabledTest(unittest.TestCase):
    def test_disabled_is_a_no_op_that_still_returns_valid_objects(self):
        c = Mcpd("http://mcpd.test", enabled=False, transport=FakeTransport())
        with c.trace("run") as span:
            span.set_input("x").set_output("y").set_model("m", "p")
            span.set_usage(input_tokens=1)
            c.log("info", "hello")
            c.metric("m", 1)
            span.score("s", 1, min=0, max=1)
            span.feedback("thumbs", "up")
            self.assertRegex(span.trace_id, r"\A[0-9a-f]{32}\Z")
            self.assertRegex(span.span_id, r"\A[0-9a-f]{16}\Z")
            self.assertFalse(span.recording)
        self.assertTrue(c.flush(1))
        self.assertEqual(c.queue_size, 0)
        self.assertEqual(c._transport.batches, [])
        self.assertIsNone(c._worker, "a disabled client starts no thread")
        c.shutdown()

    def test_the_environment_can_disable_it(self):
        import os

        os.environ["MCPD_ENABLED"] = "false"
        try:
            self.assertFalse(Mcpd(transport=FakeTransport()).enabled)
        finally:
            del os.environ["MCPD_ENABLED"]


class EnvironmentTest(unittest.TestCase):
    """Every field reads its variable, in the unit the name says, and code wins."""

    ENV = {
        "MCPD_ENDPOINT": "http://from-env:4317/",
        "MCPD_PROJECT": "from-env",
        "MCPD_SAMPLE": "0.25",
        "MCPD_BATCH_SIZE": "7",
        "MCPD_FLUSH_INTERVAL_MS": "150",
        "MCPD_MAX_QUEUE": "33",
        "MCPD_TIMEOUT_MS": "900",
    }

    def setUp(self):
        import os

        self.saved = {name: os.environ.get(name) for name in self.ENV}
        os.environ.update(self.ENV)

    def tearDown(self):
        import os

        for name, value in self.saved.items():
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value

    def test_the_environment_configures_the_client(self):
        c = Mcpd(transport=FakeTransport())
        try:
            self.assertEqual(c.endpoint, "http://from-env:4317")
            self.assertEqual(c.project, "from-env")
            self.assertEqual(c.sample, 0.25)
            self.assertEqual(c.batch_size, 7)
            # Milliseconds in the environment, seconds in code.
            self.assertEqual(c.flush_interval, 0.15)
            self.assertEqual(c.max_queue, 33)
            self.assertEqual(c._queue.max_size, 33)
            self.assertEqual(c.timeout, 0.9)
        finally:
            c.shutdown()

    def test_code_wins_over_the_environment(self):
        c = Mcpd("http://from-code:4317", project="from-code", sample=0.5, batch_size=8,
                 flush_interval=0.16, max_queue=34, timeout=0.91, transport=FakeTransport())
        try:
            self.assertEqual(c.endpoint, "http://from-code:4317")
            self.assertEqual(c.project, "from-code")
            self.assertEqual(c.sample, 0.5)
            self.assertEqual(c.batch_size, 8)
            self.assertEqual(c.flush_interval, 0.16)
            self.assertEqual(c.max_queue, 34)
            self.assertEqual(c.timeout, 0.91)
        finally:
            c.shutdown()

    def test_a_tuning_variable_that_is_not_a_whole_positive_number_is_ignored(self):
        import os

        os.environ["MCPD_BATCH_SIZE"] = "lots"
        os.environ["MCPD_MAX_QUEUE"] = "0"
        os.environ["MCPD_TIMEOUT_MS"] = "2.5"
        with self.assertLogs("mcpd", level=logging.WARNING):
            c = Mcpd(transport=FakeTransport())
        try:
            self.assertEqual(c.batch_size, 128)
            self.assertEqual(c.max_queue, 8192)
            self.assertEqual(c.timeout, 10.0)
        finally:
            c.shutdown()


class NeverThrowsTest(unittest.TestCase):
    """B1, the single most important rule in the contract."""

    def test_an_export_failure_is_swallowed(self):
        c = client(transport=FakeTransport(fail=True))
        with self.assertLogs("mcpd", level=logging.WARNING):
            with c.trace("run") as span:
                span.set_input("x")
            self.assertTrue(c.flush(2))
        self.assertGreater(c.failures, 0)
        c.shutdown()

    def test_a_dead_server_never_raises(self):
        # A real socket, to a port nothing is listening on.
        c = Mcpd("http://127.0.0.1:9", timeout=0.2, flush_interval=0.05)
        with c.trace("run") as span:
            span.set_input({"q": "hi"})
            c.log("info", "hello")
            c.metric("m", 1)
            span.score("s", 1, min=0, max=1)
            span.feedback("thumbs", "up")
        c.flush(5)
        c.shutdown(5)
        self.assertGreater(c.failures, 0, "it failed, quietly, exactly as it should")

    def test_a_rejected_signal_is_reported_not_raised(self):
        class Rejecting(FakeTransport):
            def ingest(self, batch):
                return records.IngestResult(
                    accepted=0, rejected=1, duplicate=0,
                    errors=[records.IngestError(signal=records.SignalType.SPAN, index=0,
                                                message="trace id is not hexadecimal")])

        c = client(transport=Rejecting())
        with self.assertLogs("mcpd", level=logging.WARNING) as logged:
            with c.trace("run"):
                pass
            c.flush(2)
        self.assertIn("not hexadecimal", logged.output[0])
        c.shutdown()

    def test_a_worker_that_hits_a_bug_keeps_exporting(self):
        class Flaky(FakeTransport):
            def __init__(self):
                super().__init__()
                self.first = True

            def ingest(self, batch):
                if self.first:
                    self.first = False
                    raise RuntimeError("a bug, not a network failure")
                return super().ingest(batch)

        c = client(transport=Flaky(), flush_interval=0.02)
        with self.assertLogs("mcpd", level=logging.WARNING):
            with c.trace("first"):
                pass
            c.flush(2)
        with c.trace("second"):
            pass
        self.assertTrue(c.flush(2))
        self.assertEqual([s.name for s in c._transport.records_of()], ["second"])
        c.shutdown()

    def test_a_setter_given_something_broken_does_not_raise(self):
        class Hostile:
            """A mapping that explodes the moment anything reads it."""

            def keys(self):
                raise RuntimeError("no")

            def __getitem__(self, key):
                raise RuntimeError("no")

        c = client()
        with self.assertLogs("mcpd", level=logging.WARNING):
            with c.trace("run") as span:
                returned = span.set_metadata(Hostile())
                self.assertIs(returned, span, "a failed setter still returns the span")
        c.shutdown()

    def test_recording_after_shutdown_is_inert(self):
        c = client()
        c.shutdown()
        with c.trace("late") as span:
            span.set_input("x")
            c.log("info", "late")
        self.assertEqual(c.queue_size, 0)
        self.assertFalse(span.recording)


class ReadingTransport(FakeTransport):
    """Answers reads from a table keyed by path, and remembers what was asked."""

    BODIES = {
        "/v1/api/metrics": {
            "name": "orders.looked_up", "bucket": "1h", "unit": "{order}",
            "series": [{"labels": {"outcome": "ok"},
                        "points": [{"timestamp": "2026-08-01T12:00:00Z", "value": 1.5, "count": 1}]}],
        },
        "/v1/api/feedback": {
            "feedback": [{"feedback_id": "f", "kind": "thumbs", "value": "up",
                          "timestamp": "2026-08-01T12:00:00Z", "correlation": {"project": "tests"},
                          "trace_id": "0af7651916cd43dd8448eb211c80319c", "comment": "helpful"}],
            "page": {"has_more": False},
        },
    }

    def __init__(self):
        super().__init__()
        self.reads = []

    def get(self, path, params=None):
        self.reads.append((path, dict(params or {})))
        return self.BODIES[path]


class ReadTest(unittest.TestCase):
    """The read surface builds the daemon's request and returns the generated types."""

    def test_query_metrics_sends_the_window_and_decodes_the_series(self):
        from datetime import datetime, timezone

        from mcpd import MetricSeriesResponse

        c = client(transport=ReadingTransport())
        from_ = datetime(2026, 8, 1, 11, tzinfo=timezone.utc)
        series = c.query_metrics("orders.looked_up", from_, "-0s", bucket="1h", agg="sum",
                                 group_by=["outcome"], filter={"model": "gpt-4o", "status": "ok"})
        self.assertIsInstance(series, MetricSeriesResponse)
        self.assertEqual(series.name, "orders.looked_up")
        self.assertEqual(series.series[0].labels, {"outcome": "ok"})
        self.assertEqual(series.series[0].points[0].value, 1.5)
        self.assertEqual(series.series[0].points[0].timestamp.year, 2026)

        path, params = c._transport.reads[0]
        self.assertEqual(path, "/v1/api/metrics")
        self.assertEqual(params["project"], "tests", "a read without the project reads the wrong tenant")
        self.assertEqual(params["name"], "orders.looked_up")
        self.assertEqual(params["from_"], from_)
        self.assertEqual(params["to"], "-0s")
        self.assertEqual(params["bucket"], "1h")
        self.assertEqual(params["agg"], "sum")
        self.assertEqual(params["group_by"], ["outcome"])
        # The daemon reads key:value pairs. JSON here matched no point at all.
        self.assertEqual(params["filter"], "model:gpt-4o,status:ok")
        self.assertIn("filter=model%3Agpt-4o%2Cstatus%3Aok", encode_params(params))
        for bad in ({"a:b": "x"}, {"a,b": "x"}, {"": "x"}, {"model": "a,b"}):
            with self.assertRaises(ValueError, msg=f"{bad} cannot be written as key:value pairs"):
                c.query_metrics("orders.looked_up", from_, "-0s", filter=bad)
        self.assertEqual(len(c._transport.reads), 1, "a refused filter never reaches the wire")
        c.shutdown()

    def test_list_feedback_sends_only_the_documented_filters_and_decodes_the_page(self):
        from mcpd import FeedbackListResponse

        c = client(transport=ReadingTransport())
        page = c.list_feedback("0af7651916cd43dd8448eb211c80319c", limit=5)
        self.assertIsInstance(page, FeedbackListResponse)
        self.assertEqual(len(page.feedback), 1)
        self.assertIs(page.feedback[0].kind, records.FeedbackKind.THUMBS)
        self.assertEqual(page.feedback[0].value, "up")
        self.assertEqual(page.feedback[0].comment, "helpful")
        self.assertFalse(page.page.has_more)

        path, params = c._transport.reads[0]
        self.assertEqual(path, "/v1/api/feedback")
        # The daemon answers 400 to any other key, so these three are all.
        self.assertEqual(params, {
            "project": "tests", "trace_id": "0af7651916cd43dd8448eb211c80319c", "limit": 5,
        })
        with self.assertRaises(TypeError):
            c.list_feedback("0af7651916cd43dd8448eb211c80319c", kind="thumbs")  # type: ignore[call-arg]
        with self.assertRaises(ValueError):
            c.list_feedback("")
        self.assertEqual(len(c._transport.reads), 1, "a refused call never reaches the wire")
        c.shutdown()


class LifecycleTest(unittest.TestCase):
    def test_flush_and_shutdown_are_idempotent(self):
        c = client()
        with c.trace("run"):
            pass
        self.assertTrue(c.flush(2))
        self.assertTrue(c.flush(2), "flushing an empty queue is a no-op that succeeds")
        c.shutdown()
        c.shutdown()
        self.assertTrue(c.closed)
        self.assertTrue(c._transport.closed)

    def test_shutdown_flushes_what_is_queued(self):
        c = client(flush_interval=60)
        with c.trace("run"):
            pass
        c.shutdown(5)
        self.assertEqual(len(c._transport.records_of()), 1)

    def test_the_client_works_as_a_context_manager(self):
        with client() as c:
            with c.trace("run"):
                pass
        self.assertTrue(c.closed)
        self.assertEqual(len(c._transport.records_of()), 1)

    def test_the_async_client_works_as_a_context_manager(self):
        async def run():
            c = client()
            async with c:
                async with c.trace("run"):
                    pass
            return c

        c = asyncio.run(run())
        self.assertTrue(c.closed)
        self.assertEqual(len(c._transport.records_of()), 1)


if __name__ == "__main__":
    unittest.main()
