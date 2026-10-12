"""The wire: codec, gzip, retry and the idempotency key (contract section 6)."""

import gzip
import json
import sys
import types
import unittest

from mcpd import records
from mcpd.context import EPOCH, nanotime
from mcpd.transport import (
    GZIP_THRESHOLD,
    default_sender,
    MAX_ATTEMPTS,
    Response,
    Transport,
    TransportError,
    decode,
    encode,
    encode_params,
    parse_timestamp,
)


def a_span(name="probe", **kwargs):
    return records.Span(
        trace_id="0af7651916cd43dd8448eb211c80319c",
        span_id="b7ad6b7169203331",
        name=name,
        kind=records.SpanKind.AGENT_RUN,
        status=records.SpanStatus.OK,
        start_time=nanotime(1_700_000_000_123_456_789),
        is_root=True,
        is_event=False,
        correlation=records.Correlation(project="default"),
        received_at=EPOCH,
        **kwargs,
    )


class FakeSender:
    """A sender that answers from a script and remembers what it was asked."""

    name = "fake"

    def __init__(self, script):
        self.script = list(script)
        self.calls = []

    def send(self, method, url, headers, body, timeout):
        self.calls.append({"method": method, "url": url, "headers": dict(headers), "body": body})
        answer = self.script.pop(0) if self.script else Response(200, {}, b"{}")
        if isinstance(answer, Exception):
            raise answer
        return answer

    def close(self):
        pass


def ok_body(accepted=1):
    return json.dumps({"accepted": accepted, "rejected": 0, "duplicate": 0}).encode()


def transport(script, **kwargs):
    t = Transport("http://mcpd.test", sender=FakeSender(script), **kwargs)
    t.slept = []
    t.sleep = t.slept.append  # never actually wait during a test
    return t


class CodecTest(unittest.TestCase):
    def test_encode_omits_unset_fields_and_renders_enums(self):
        wire = encode(a_span())
        self.assertEqual(wire["kind"], "agent_run")
        self.assertEqual(wire["status"], "ok")
        self.assertNotIn("end_time", wire, "None fields are omitted, as the server expects")
        self.assertNotIn("model", wire)

    def test_encode_keeps_nanosecond_timestamps_in_utc(self):
        wire = encode(a_span())
        self.assertEqual(wire["start_time"], "2023-11-14T22:13:20.123456789Z")
        self.assertEqual(wire["received_at"], "1970-01-01T00:00:00.000000000Z")

    def test_a_python_keyword_field_keeps_its_wire_name(self):
        summary = records.AnalyticsSummary(
            from_=nanotime(0), to=nanotime(0), trace_count=1, span_count=1, error_count=0,
            error_rate=0.0, latency=records.LatencyStats(), input_tokens=0, output_tokens=0,
            total_cost=0.0,
        )
        wire = encode(summary)
        self.assertIn("from", wire)
        self.assertNotIn("from_", wire)
        self.assertEqual(decode(records.AnalyticsSummary, wire).from_, nanotime(0))

    def test_decode_reads_a_real_response_shape(self):
        payload = {
            "traces": [{
                "trace_id": "0af7651916cd43dd8448eb211c80319c",
                "name": "support_agent",
                "kind": "agent_run",
                "status": "error",
                "start_time": "2026-08-27T22:34:29.405355239Z",
                "span_count": 7, "error_count": 1, "model_count": 3, "tool_count": 3,
                "correlation": {"project": "default", "environment": "production"},
                "is_complete": True,
                "received_at": "2026-08-27T22:36:29.405511235Z",
                "usage": {"input_tokens": 3300, "output_tokens": 84},
                "tags": ["demo", "support"],
            }],
            "page": {"has_more": False},
        }
        parsed = decode(records.TraceListResponse, payload)
        trace = parsed.traces[0]
        self.assertEqual(trace.name, "support_agent")
        self.assertIs(trace.status, records.SpanStatus.ERROR)
        self.assertEqual(trace.correlation.environment, "production")
        self.assertEqual(trace.usage.input_tokens, 3300)
        self.assertEqual(trace.tags, ["demo", "support"])
        self.assertEqual(trace.start_time.ns, 1787870069405355239)
        self.assertFalse(parsed.page.has_more)

    def test_an_unknown_enum_value_survives_as_its_string(self):
        # A server one version ahead must not be able to crash a client.
        span = decode(records.Span, dict(encode(a_span()), kind="teleportation"))
        self.assertEqual(span.kind, "teleportation")

    def test_decode_names_the_field_the_server_left_out(self):
        with self.assertRaises(TransportError) as caught:
            decode(records.HealthStatus, {"status": "ok"})
        self.assertIn("HealthStatus", str(caught.exception))

    def test_timestamps_parse_at_every_precision(self):
        self.assertEqual(parse_timestamp("2026-08-27T22:30:00Z").ns, 1787869800000000000)
        self.assertEqual(parse_timestamp("2026-08-27T22:30:00.5Z").ns, 1787869800500000000)
        self.assertEqual(parse_timestamp("2026-08-27T23:30:00+01:00").ns, 1787869800000000000)
        with self.assertRaises(TransportError):
            parse_timestamp("yesterday")

    def test_query_parameters_drop_the_unset_and_join_lists_with_commas(self):
        # The daemon keeps the last of a repeated key, so `tags=a&tags=b` would
        # ask for traces tagged `b` alone.
        rendered = encode_params({"project": "default", "from_": "-1h", "tags": ["a", "b"],
                                  "limit": 10, "kind": records.SpanKind.TOOL_CALL, "q": None})
        self.assertEqual(rendered, "project=default&from=-1h&tags=a%2Cb&limit=10&kind=tool_call")
        self.assertEqual(encode_params({"tags": []}), "")


class IngestTest(unittest.TestCase):
    def test_a_small_batch_is_sent_uncompressed(self):
        t = transport([Response(200, {}, ok_body())])
        t.ingest(records.IngestBatch(spans=[a_span()], ingest_key="k"))
        call = t._sender.calls[0]
        self.assertNotIn("content-encoding", call["headers"])
        self.assertLess(len(call["body"]), GZIP_THRESHOLD)

    def test_a_large_batch_is_gzipped(self):
        t = transport([Response(200, {}, ok_body())])
        big = a_span(input="x" * (GZIP_THRESHOLD * 4))
        t.ingest(records.IngestBatch(spans=[big], ingest_key="k"))
        call = t._sender.calls[0]
        self.assertEqual(call["headers"]["content-encoding"], "gzip")
        self.assertEqual(json.loads(gzip.decompress(call["body"]))["ingest_key"], "k")

    def test_the_api_key_is_sent_as_a_bearer_token(self):
        t = transport([Response(200, {}, ok_body())], api_key="secret")
        t.ingest(records.IngestBatch(spans=[a_span()], ingest_key="k"))
        self.assertEqual(t._sender.calls[0]["headers"]["authorization"], "Bearer secret")

    def test_the_result_comes_back_decoded(self):
        t = transport([Response(200, {}, json.dumps(
            {"accepted": 2, "rejected": 1, "duplicate": 0,
             "errors": [{"signal": "span", "index": 1, "message": "bad id"}]}).encode())])
        result = t.ingest(records.IngestBatch(spans=[a_span()], ingest_key="k"))
        self.assertEqual((result.accepted, result.rejected), (2, 1))
        self.assertEqual(result.errors[0].signal, records.SignalType.SPAN)


class RetryTest(unittest.TestCase):
    def test_a_503_is_retried_and_the_body_never_changes(self):
        t = transport([Response(503, {}, b"nope"), Response(200, {}, ok_body())])
        t.ingest(records.IngestBatch(spans=[a_span()], ingest_key="stable-key"))
        self.assertEqual(len(t._sender.calls), 2)
        first, second = (c["body"] for c in t._sender.calls)
        self.assertEqual(first, second, "a retry must replay the same idempotent batch")

    def test_each_gateway_status_the_contract_names_is_retried(self):
        for status in (429, 502, 504):
            t = transport([Response(status, {}, b"nope"), Response(status, {}, b"nope"),
                           Response(200, {}, ok_body())])
            t.ingest(records.IngestBatch(spans=[a_span()], ingest_key="k"))
            self.assertEqual(len(t._sender.calls), 3, f"{status} was not retried twice")
            self.assertEqual(len(t.slept), 2, f"{status} was retried without backing off")

    def test_a_500_is_not_retried(self):
        # Not a load-shedding status, so a second identical batch fares no better.
        t = transport([Response(500, {}, b"{}")])
        with self.assertRaises(TransportError):
            t.ingest(records.IngestBatch(spans=[a_span()], ingest_key="k"))
        self.assertEqual(len(t._sender.calls), 1)

    def test_it_gives_up_after_three_attempts(self):
        t = transport([Response(503, {}, b"")] * 5)
        with self.assertRaises(TransportError):
            t.ingest(records.IngestBatch(spans=[a_span()], ingest_key="k"))
        self.assertEqual(len(t._sender.calls), MAX_ATTEMPTS)
        self.assertEqual(len(t.slept), MAX_ATTEMPTS - 1, "no sleep after the final attempt")

    def test_a_connection_failure_is_retried(self):
        t = transport([TransportError("refused", retryable=True),
                       TransportError("refused", retryable=True),
                       Response(200, {}, ok_body())])
        t.ingest(records.IngestBatch(spans=[a_span()], ingest_key="k"))
        self.assertEqual(len(t._sender.calls), 3)

    def test_a_400_is_not_retried(self):
        t = transport([Response(400, {}, json.dumps({"message": "malformed"}).encode())])
        with self.assertRaises(TransportError) as caught:
            t.ingest(records.IngestBatch(spans=[a_span()], ingest_key="k"))
        self.assertEqual(len(t._sender.calls), 1, "a malformed batch stays malformed")
        self.assertEqual(caught.exception.status, 400)
        self.assertIn("malformed", str(caught.exception))

    def test_a_401_is_not_retried(self):
        t = transport([Response(401, {}, b"{}")])
        with self.assertRaises(TransportError):
            t.ingest(records.IngestBatch(spans=[a_span()], ingest_key="k"))
        self.assertEqual(len(t._sender.calls), 1)

    def test_retry_after_is_honoured(self):
        t = transport([Response(429, {"Retry-After": "2"}, b""), Response(200, {}, ok_body())])
        t.ingest(records.IngestBatch(spans=[a_span()], ingest_key="k"))
        self.assertEqual(t.slept, [2.0])

    def test_a_hostile_retry_after_is_capped(self):
        t = transport([Response(429, {"retry-after": "86400"}, b""), Response(200, {}, ok_body())])
        t.ingest(records.IngestBatch(spans=[a_span()], ingest_key="k"))
        self.assertLessEqual(t.slept[0], 30.0, "a shutdown must not wait a day")

    def test_backoff_grows_and_is_jittered(self):
        t = transport([Response(503, {}, b"")] * 3)
        with self.assertRaises(TransportError):
            t.ingest(records.IngestBatch(spans=[a_span()], ingest_key="k"))
        self.assertTrue(all(0.0 <= s <= 0.5 for s in t.slept), t.slept)


class RealSenderTest(unittest.TestCase):
    """The default sender, against a real socket.

    The fake sender above proves the retry policy; this proves the thing that
    actually opens the connection, because `urllib` is what every install of
    this package ships with.
    """

    @classmethod
    def setUpClass(cls):
        import threading
        from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

        cls.seen = []
        cls.script = []

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                body = self.rfile.read(int(self.headers.get("content-length", 0)))
                # HTTP header names are case-insensitive and `urllib` title-cases
                # what it sends, so compare on a lowered copy.
                headers = {k.lower(): v for k, v in self.headers.items()}
                cls.seen.append({"path": self.path, "headers": headers, "body": body})
                status = cls.script.pop(0) if cls.script else 200
                payload = ok_body() if status == 200 else b'{"message": "later"}'
                self.send_response(status)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            def log_message(self, *args):
                pass

        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()
        cls.endpoint = "http://127.0.0.1:%d" % cls.server.server_address[1]

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()

    def setUp(self):
        type(self).seen.clear()
        type(self).script.clear()

    def test_a_batch_arrives_gzipped_and_the_result_comes_back(self):
        t = Transport(self.endpoint, api_key="k1")
        self.addCleanup(t.close)
        big = a_span(input="x" * (GZIP_THRESHOLD * 4))
        result = t.ingest(records.IngestBatch(spans=[big], ingest_key="real-1"))
        self.assertEqual(result.accepted, 1)
        sent = self.seen[0]
        self.assertEqual(sent["path"], "/v1/ingest")
        self.assertEqual(sent["headers"]["content-encoding"], "gzip")
        self.assertEqual(sent["headers"]["authorization"], "Bearer k1")
        self.assertEqual(json.loads(gzip.decompress(sent["body"]))["ingest_key"], "real-1")

    def test_a_503_from_a_real_server_is_retried(self):
        type(self).script.extend([503, 200])
        t = Transport(self.endpoint)
        t.sleep = lambda seconds: None
        self.addCleanup(t.close)
        t.ingest(records.IngestBatch(spans=[a_span()], ingest_key="real-2"))
        self.assertEqual(len(self.seen), 2)
        self.assertEqual(self.seen[0]["body"], self.seen[1]["body"])

    def test_an_unreachable_server_raises_rather_than_hanging(self):
        t = Transport("http://127.0.0.1:9", timeout=0.2)
        t.sleep = lambda seconds: None
        self.addCleanup(t.close)
        with self.assertRaises(TransportError) as caught:
            t.ingest(records.IngestBatch(spans=[a_span()], ingest_key="real-3"))
        self.assertTrue(caught.exception.retryable)


class SenderSelectionTest(unittest.TestCase):
    def test_urllib_is_the_fallback_when_httpx_is_absent(self):
        self.assertEqual(default_sender(1.0, prefer_httpx=False).name, "urllib")
        if "httpx" not in sys.modules:
            try:
                import httpx  # noqa: F401
            except ImportError:
                self.assertEqual(default_sender(1.0).name, "urllib")

    def test_httpx_is_used_when_it_is_installed(self):
        stub = types.ModuleType("httpx")

        class HTTPError(Exception):
            pass

        class Reply:
            status_code = 200
            headers = {"content-type": "application/json"}
            content = ok_body()

        class Client:
            def __init__(self, timeout=None):
                self.timeout = timeout
                self.calls = []
                self.closed = False

            def request(self, method, url, headers=None, content=None, timeout=None):
                self.calls.append((method, url))
                return Reply()

            def close(self):
                self.closed = True

        stub.HTTPError = HTTPError
        stub.Client = Client
        previous = sys.modules.get("httpx")
        sys.modules["httpx"] = stub
        try:
            sender = default_sender(2.5)
            self.assertEqual(sender.name, "httpx")
            response = sender.send("POST", "http://mcpd.test/v1/ingest", {}, b"{}", 1.0)
            self.assertEqual(response.status, 200)
            sender.close()
        finally:
            if previous is None:
                del sys.modules["httpx"]
            else:
                sys.modules["httpx"] = previous


if __name__ == "__main__":
    unittest.main()
