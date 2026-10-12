"""The logging signal: serialised errors, the level floor and severity numbers
(contract section 4).
"""

import gzip
import json
import os
import unittest

from mcpd import (
    LogLevel,
    Mcpd,
    RecordingSink,
    describe_error,
    encode_fields,
    log_level_from_severity,
    severity_number,
)
from mcpd.transport import Response, Transport
from tests.test_client import FakeTransport
from tests.test_transport import FakeSender


def recorder(**kwargs):
    sink = RecordingSink()
    kwargs.setdefault("flush_interval", 10.0)
    kwargs.setdefault("project", "tests")
    return Mcpd("http://mcpd.test", sink=sink, transport=FakeTransport(), **kwargs), sink


def raised(exc):
    """The same exception, with the traceback it only gets from being raised."""
    try:
        raise exc
    except type(exc) as caught:
        return caught


class CorrelationWithNoValue(unittest.TestCase):
    """P301: a key present with no value never erases an inherited one."""

    def test_none_on_a_log_leaves_the_span_correlation_standing(self):
        mcpd, sink = recorder()
        with mcpd.trace("run", session_id="session-7", user_id="u-1"):
            mcpd.log("info", "inside", session_id=None)
            mcpd.metric("latency_ms", 4.0, session_id=None)
        mcpd.flush()

        self.assertEqual(sink.logs[0].correlation.session_id, "session-7")
        self.assertEqual(sink.logs[0].correlation.user_id, "u-1")
        self.assertEqual(sink.metrics[0].correlation.session_id, "session-7")
        mcpd.shutdown()

    def test_a_value_still_wins_over_the_run(self):
        mcpd, sink = recorder()
        with mcpd.trace("run", session_id="run"):
            mcpd.log("info", "inside", session_id="line")
        mcpd.flush()

        self.assertEqual(sink.logs[0].correlation.session_id, "line")
        mcpd.shutdown()


class ErrorsInFields(unittest.TestCase):
    """P300: an exception under a field key, and a value nothing can encode."""

    def test_an_exception_arrives_as_type_message_and_stack(self):
        mcpd, sink = recorder()
        mcpd.log("error", "the call failed", {"error": raised(ValueError("boom")), "attempt": 2})
        mcpd.flush()

        fields = sink.logs[0].fields
        self.assertEqual(fields["attempt"], 2)
        self.assertEqual(fields["error"]["type"], "ValueError")
        self.assertEqual(fields["error"]["message"], "boom")
        self.assertIn("ValueError: boom", fields["error"]["stack"])
        mcpd.shutdown()

    def test_an_exception_nested_in_a_list_or_a_label(self):
        mcpd, sink = recorder()
        mcpd.log("warn", "retries exhausted", {"attempts": [ValueError("first")]})
        mcpd.metric("failures", 1, labels={"cause": ValueError("label")})
        mcpd.flush()

        self.assertEqual(
            sink.logs[0].fields["attempts"][0], {"type": "ValueError", "message": "first"}
        )
        self.assertEqual(
            sink.metrics[0].labels["cause"], {"type": "ValueError", "message": "label"}
        )
        mcpd.shutdown()

    def test_it_costs_one_field_and_not_the_batch_it_travelled_in(self):
        """The defect: `json.dumps` refuses an exception, and the batch is lost."""
        sender = FakeSender([Response(200, {}, b'{"accepted":2,"rejected":0,"duplicate":0}')])
        transport = Transport("http://mcpd.test", sender=sender)
        mcpd = Mcpd("http://mcpd.test", transport=transport, project="tests", flush_interval=10.0)
        cycle = {"name": "request"}
        cycle["self"] = cycle
        with mcpd.trace("run"):
            pass
        mcpd.log("error", "a request went wrong", {"request": cycle, "why": ValueError("boom")})
        mcpd.flush()

        self.assertEqual(mcpd.stats().failed, 0)
        self.assertEqual(mcpd.stats().exported, 2)
        body = sender.calls[0]["body"]
        if sender.calls[0]["headers"].get("content-encoding") == "gzip":
            body = gzip.decompress(body)
        sent = json.loads(body)
        self.assertEqual(len(sent["spans"]), 1)
        self.assertEqual(
            sent["logs"][0]["fields"],
            {
                "request": {"name": "request", "self": "[circular]"},
                "why": {"type": "ValueError", "message": "boom"},
            },
        )
        mcpd.shutdown()

    def test_a_span_score_or_feedback_value_costs_itself_and_never_the_batch(self):
        """A set, a Decimal, bytes or a cycle in a payload made `json.dumps` raise in
        the transport, and a NaN went out as a bare word the daemon refuses."""
        from decimal import Decimal

        sender = FakeSender([Response(200, {}, b'{"accepted":5,"rejected":0,"duplicate":0}')])
        transport = Transport("http://mcpd.test", sender=sender)
        mcpd = Mcpd("http://mcpd.test", transport=transport, project="tests", flush_interval=10.0)
        cycle = {"name": "request"}
        cycle["self"] = cycle
        with mcpd.trace("run", input={"tags": {"a"}, "raw": b"hi"}) as run:
            run.set_output({"total": Decimal("1.50"), "ratio": float("nan"), "loop": cycle})
            run.set_metadata({"limit": float("inf")})
            run.set_model_parameters(temperature=float("nan"), top_p=0.9)
            run.score("judge", 0.5, min=0, max=1, metadata={"seen": {1}})
            run.feedback("thumbs", {"votes": {2}})
            # A number JSON cannot carry is the signal itself here, so only it is lost.
            mcpd.metric("ratio", float("nan"))
            run.score("broken", float("inf"), min=0, max=1)
            mcpd.log("info", "beside them")
        self.assertTrue(mcpd.flush(2))

        self.assertEqual(mcpd.stats().failed, 0)
        body = sender.calls[0]["body"]
        if sender.calls[0]["headers"].get("content-encoding") == "gzip":
            body = gzip.decompress(body)

        def strict(word):
            raise AssertionError(f"{word} is not JSON, and the daemon refuses the batch")

        sent = json.loads(body, parse_constant=strict)
        span = [s for s in sent["spans"] if "end_time" in s][0]
        self.assertEqual(span["input"], {"tags": ["a"], "raw": "hi"})
        self.assertEqual(
            span["output"],
            {"total": "1.50", "ratio": "nan", "loop": {"name": "request", "self": "[circular]"}},
        )
        self.assertEqual(span["metadata"], {"limit": "inf"})
        self.assertNotIn("temperature", span["model_parameters"])
        self.assertEqual(span["model_parameters"]["top_p"], 0.9)
        self.assertEqual([s["scorer_id"] for s in sent["scores"]], ["judge"])
        self.assertEqual(sent["scores"][0]["metadata"], {"seen": [1]})
        self.assertEqual(sent["feedback"][0]["value"], {"votes": [2]})
        self.assertNotIn("metrics", sent)
        self.assertEqual(len(sent["logs"]), 1)
        mcpd.shutdown()

    def test_describe_error_answers_only_for_an_exception(self):
        self.assertEqual(
            describe_error(KeyError("k")), {"type": "KeyError", "message": "'k'"}
        )
        self.assertIsNone(describe_error("not an exception"))

    def test_encode_fields_keeps_what_json_already_accepts(self):
        given = {"a": 1, "b": [True, None, "x"], "c": {"d": 1.5}}
        self.assertEqual(encode_fields(given), given)


class TheLevelFloor(unittest.TestCase):
    """P085: the lowest level the client records."""

    def setUp(self):
        os.environ.pop("MCPD_LOG_LEVEL", None)

    def tearDown(self):
        os.environ.pop("MCPD_LOG_LEVEL", None)

    def test_the_default_drops_trace_and_keeps_debug(self):
        mcpd, sink = recorder()
        self.assertEqual(mcpd.log_level, LogLevel.DEBUG)
        mcpd.log("trace", "finer than debug")
        mcpd.log("debug", "kept")
        mcpd.flush()

        self.assertEqual([r.message for r in sink.logs], ["kept"])
        mcpd.shutdown()

    def test_a_floor_set_in_code_drops_everything_below_it(self):
        mcpd, sink = recorder(log_level="warn")
        for level in LogLevel:
            mcpd.log(level, level.value)
        mcpd.flush()

        self.assertEqual([r.message for r in sink.logs], ["warn", "error", "fatal"])
        self.assertEqual(mcpd.stats().dropped, 0)
        mcpd.shutdown()

    def test_it_reads_the_environment_and_code_wins(self):
        os.environ["MCPD_LOG_LEVEL"] = "ERROR"
        from_env, sink = recorder()
        self.assertEqual(from_env.log_level, LogLevel.ERROR)
        from_env.log("warn", "below the floor")
        from_env.flush()
        self.assertEqual(sink.logs, [])
        from_env.shutdown()

        from_code, _ = recorder(log_level=LogLevel.TRACE)
        self.assertEqual(from_code.log_level, LogLevel.TRACE)
        from_code.shutdown()

    def test_a_name_that_is_not_a_level_is_ignored(self):
        os.environ["MCPD_LOG_LEVEL"] = "verbose"
        mcpd, _ = recorder()
        self.assertEqual(mcpd.log_level, LogLevel.DEBUG)
        mcpd.shutdown()

    def test_it_never_filters_the_other_four_signals(self):
        mcpd, sink = recorder(log_level="fatal")
        with mcpd.trace("run") as run:
            trace_id = run.trace_id
        mcpd.metric("latency_ms", 3.0)
        mcpd.score("judge", 1.0, min=0.0, max=1.0, trace_id=trace_id)
        mcpd.feedback("thumbs", "up", trace_id=trace_id)
        mcpd.flush()

        self.assertEqual(len(sink.spans), 1)
        self.assertEqual(len(sink.metrics), 1)
        self.assertEqual(len(sink.scores), 1)
        self.assertEqual(len(sink.feedback), 1)
        mcpd.shutdown()


class SeverityNumbers(unittest.TestCase):
    """P090: the OpenTelemetry mapping, both directions."""

    def test_every_level_carries_the_number_the_spec_gives_it(self):
        self.assertEqual([severity_number(l) for l in LogLevel], [1, 5, 9, 13, 17, 21])

    def test_every_number_maps_back_by_range(self):
        ranges = [
            (1, 4, LogLevel.TRACE),
            (5, 8, LogLevel.DEBUG),
            (9, 12, LogLevel.INFO),
            (13, 16, LogLevel.WARN),
            (17, 20, LogLevel.ERROR),
            (21, 24, LogLevel.FATAL),
        ]
        for low, high, level in ranges:
            for number in range(low, high + 1):
                self.assertEqual(log_level_from_severity(number), level)

    def test_a_number_outside_the_range_is_clamped(self):
        self.assertEqual(log_level_from_severity(0), LogLevel.TRACE)
        self.assertEqual(log_level_from_severity(-3), LogLevel.TRACE)
        self.assertEqual(log_level_from_severity(40), LogLevel.FATAL)

    def test_every_level_round_trips(self):
        for level in LogLevel:
            self.assertEqual(log_level_from_severity(severity_number(level)), level)


if __name__ == "__main__":
    unittest.main()
