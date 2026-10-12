"""Ids and W3C context propagation (contract B6 and B7)."""

import unittest

from mcpd.context import (
    TraceContext,
    format_traceparent,
    is_valid_span_id,
    is_valid_trace_id,
    nanotime,
    new_span_id,
    new_trace_id,
    now_ns,
    parse_traceparent,
)


class IdTest(unittest.TestCase):
    def test_ids_are_opentelemetry_shaped(self):
        for _ in range(200):
            trace_id, span_id = new_trace_id(), new_span_id()
            self.assertRegex(trace_id, r"\A[0-9a-f]{32}\Z")
            self.assertRegex(span_id, r"\A[0-9a-f]{16}\Z")
            self.assertNotEqual(trace_id.strip("0"), "", "the all-zero id is reserved")
            self.assertNotEqual(span_id.strip("0"), "")

    def test_ids_are_unique(self):
        self.assertEqual(len({new_trace_id() for _ in range(1000)}), 1000)

    def test_validation_rejects_the_wrong_shapes(self):
        self.assertTrue(is_valid_trace_id("0af7651916cd43dd8448eb211c80319c"))
        self.assertFalse(is_valid_trace_id("0" * 32), "all zero")
        self.assertFalse(is_valid_trace_id("0AF7651916CD43DD8448EB211C80319C"), "uppercase")
        self.assertFalse(is_valid_trace_id("0af765"), "too short")
        self.assertFalse(is_valid_trace_id(None))
        self.assertTrue(is_valid_span_id("b7ad6b7169203331"))
        self.assertFalse(is_valid_span_id("b7ad6b7169203331b7ad6b7169203331"))


class TraceparentTest(unittest.TestCase):
    HEADER = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"

    def test_round_trip(self):
        ctx = parse_traceparent(self.HEADER)
        self.assertEqual(ctx.trace_id, "0af7651916cd43dd8448eb211c80319c")
        self.assertEqual(ctx.span_id, "b7ad6b7169203331")
        self.assertTrue(ctx.sampled)
        self.assertEqual(format_traceparent(ctx), self.HEADER)
        self.assertEqual(ctx.traceparent(), self.HEADER)

    def test_the_sampled_flag_survives_both_ways(self):
        unsampled = self.HEADER[:-2] + "00"
        self.assertFalse(parse_traceparent(unsampled).sampled)
        self.assertTrue(format_traceparent(TraceContext("a" * 32, "b" * 16, True)).endswith("-01"))
        self.assertTrue(format_traceparent(TraceContext("a" * 32, "b" * 16, False)).endswith("-00"))

    def test_a_future_version_is_still_readable(self):
        # The specification requires a parser to accept a higher version on the
        # strength of the four fields every version keeps.
        self.assertIsNotNone(parse_traceparent("cc-" + self.HEADER[3:]))

    def test_malformed_headers_are_refused(self):
        for bad in [None, "", "not-a-header", self.HEADER[:-1],
                    "ff-" + self.HEADER[3:],                       # version ff is invalid
                    "00-" + "0" * 32 + "-b7ad6b7169203331-01",     # all-zero trace id
                    "00-0af7651916cd43dd8448eb211c80319c-" + "0" * 16 + "-01",
                    "00-0af7651916cd43dd8448eb211c80319g-b7ad6b7169203331-01"]:
            self.assertIsNone(parse_traceparent(bad), bad)

    def test_case_and_whitespace_are_tolerated(self):
        self.assertIsNotNone(parse_traceparent("  " + self.HEADER.upper() + "  "))


class TimestampTest(unittest.TestCase):
    def test_timestamps_are_utc_with_nanoseconds(self):
        ts = nanotime(now_ns())
        rendered = ts.isoformat_ns()
        self.assertRegex(rendered, r"\A\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{9}Z\Z")
        self.assertEqual(ts.utcoffset().total_seconds(), 0.0)

    def test_the_nanosecond_remainder_is_not_rounded_away(self):
        ts = nanotime(1_700_000_000_123_456_789)
        self.assertEqual(ts.isoformat_ns(), "2023-11-14T22:13:20.123456789Z")
        self.assertEqual(ts.microsecond, 123456)


if __name__ == "__main__":
    unittest.main()
