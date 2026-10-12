"""The worker loop and the run API against a scripted sender (contract sections 7.2 to 7.4)."""

import json
import threading
import time
import unittest

from mcpd import (
    Mcpd,
    McpdError,
    RecordingSink,
    RetryableError,
    RunFailedError,
    StepConflictError,
    StepFailure,
    StepOutcome,
    Worker,
    workflow,
)
from mcpd.transport import Response, Transport
from tests.test_transport import FakeSender

TRACE = "a1" * 16
STEP_SPAN = "b2" * 8


def wire_lease(**overrides):
    """A lease as the daemon spells it on the wire."""
    lease = {
        "lease_id": "lease-1", "run_id": "run-1", "project": "test", "definition_id": "support",
        "definition_version": 1, "step_key": "classify", "kind": "task", "attempt": 0,
        "effect": "at_least_once", "expires_at": "2026-09-01T10:00:00Z", "idempotency_key": "run-1:classify:0",
        "input": {"ticket_id": "T-1"},
        "prior": {"triage": {"status": "succeeded", "output": {"queue_name": "billing"}}},
        "state": {"seen_count": 1},
        "correlation": {"project": "test", "session_id": "s-1"},
        "trace_id": TRACE, "span_id": STEP_SPAN, "parent_span_id": "c3" * 8,
    }
    lease.update(overrides)
    return lease


def ok(body):
    return Response(200, {}, json.dumps(body).encode("utf-8"))


def claimed(*leases):
    return ok({"leases": list(leases)})


IDLE = ok({"leases": []})
STARTED = ok({"expires_at": "2026-09-01T10:00:30Z", "reclaimed": False})
ACKED = ok({"run_status": "running", "next": "claim"})
REGISTERED = ok({"worker_id": "w-1", "registered": [{"id": "support", "version": 1, "assigned": True}],
                 "lease_ttl_ms": 3_000, "heartbeat_ms": 1_000})


def conflict(reason, **extra):
    body = {"error": "conflict", "reason": reason, "message": f"the daemon says {reason}", "attempt": 0,
            "run_status": "running"}
    body.update(extra)
    return Response(409, {}, json.dumps(body).encode("utf-8"))


class RoutedSender(FakeSender):
    """A scripted sender that answers some paths by suffix, whatever the order the threads arrive in."""

    def __init__(self, script, routes):
        super().__init__(script)
        self.routes = routes

    def send(self, method, url, headers, body, timeout):
        for suffix, answer in self.routes.items():
            if url.split("?")[0].endswith(suffix):
                self.calls.append({"method": method, "url": url, "headers": dict(headers), "body": body})
                return answer
        return super().send(method, url, headers, body, timeout)


class Harness:
    """A client on a scripted sender, and a worker that reports into a list."""

    def __init__(self, script, routes=None, sink=None, **worker):
        self.sender = RoutedSender(script, routes or {})
        transport = Transport("http://mcpd.test", sender=self.sender)
        transport.sleep = lambda seconds: None
        self.errors = []
        # Spans the bodies record land in the sink when one is given, so a test
        # can read them without an ingest post reaching the scripted sender.
        self.sink = sink
        self.mcpd = Mcpd("http://mcpd.test", project="test", transport=transport, flush_interval=10.0,
                         sink=sink)
        worker.setdefault("worker_id", "w-1")
        worker.setdefault("wait_ms", 0)
        worker.setdefault("on_error", lambda message, error: self.errors.append((message, error)))
        self.worker = Worker(self.mcpd, **worker)

    def paths(self):
        return [f"{c['method']} {c['url'].replace('http://mcpd.test', '')}" for c in self.sender.calls]

    def body(self, index):
        raw = self.sender.calls[index]["body"]
        return json.loads(raw.decode("utf-8")) if raw else None

    def close(self):
        self.mcpd.shutdown()


class ClaimLongPollTest(unittest.TestCase):
    """The claim request outlives the client timeout by `wait_ms` (contract section 7.2)."""

    def test_an_idle_claim_answered_after_the_timeout_but_within_the_wait_is_a_response(self):
        from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                self.rfile.read(int(self.headers.get("content-length", 0)))
                # The daemon holds an idle claim open for the whole of wait_ms,
                # so the answer lands after the client's own timeout. A claim cut
                # off by that timeout is what made a healthy idle worker report a
                # failure every ten seconds.
                time.sleep(0.3)
                payload = b'{"leases": []}'
                self.send_response(200)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            def log_message(self, *args):
                pass

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        transport = Transport("http://127.0.0.1:%d" % server.server_address[1], timeout=0.1)
        transport.sleep = lambda seconds: None
        mcpd = Mcpd(transport.endpoint, project="test", transport=transport, flush_interval=10.0)
        self.addCleanup(mcpd.shutdown)

        response = mcpd.steps.claim("w-1", max=1, wait_ms=2_000)

        self.assertEqual(response.leases, [])


class ServingOneLeaseTest(unittest.TestCase):
    def test_started_then_the_body_with_input_prior_and_state_then_complete(self):
        h = Harness([claimed(wire_lease()), STARTED, ACKED])
        seen = []

        def classify(ctx, input, prior):
            seen.extend([input, prior, ctx.state, ctx.idempotency_key, ctx.attempt, ctx.run_id, ctx.step_key])
            ctx.set_state({"seen_count": 2}, last_ticket="T-1")
            ctx.set_usage(input_tokens=10, output_tokens=5)
            return {"kind": "question"}

        h.worker.register(workflow("support").step("classify", classify))
        self.assertEqual(h.worker.claim(1), 1)
        h.worker.drain()

        self.assertEqual(h.paths(), ["POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started",
                                     "POST /v1/api/steps/lease-1/complete"])
        self.assertEqual(h.body(0), {"project": "test", "worker_id": "w-1", "max": 1, "wait_ms": 0})
        # Opaque objects reach the body spelled as the daemon sent them.
        self.assertEqual(seen, [
            {"ticket_id": "T-1"},
            {"triage": {"status": "succeeded", "output": {"queue_name": "billing"}}},
            {"seen_count": 1}, "run-1:classify:0", 0, "run-1", "classify",
        ])
        self.assertEqual(h.body(2), {"output": {"kind": "question"}, "state": {"seen_count": 2, "last_ticket": "T-1"},
                                     "usage": {"input_tokens": 10, "output_tokens": 5}})
        self.assertEqual(h.errors, [])
        self.assertEqual(h.worker.inflight, 0)
        h.close()

    def test_a_body_that_returns_nothing_completes_with_no_output(self):
        h = Harness([claimed(wire_lease()), STARTED, ACKED])
        h.worker.register(workflow("support").step("classify", lambda ctx, i, p: None))
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(h.body(2), {})
        h.close()

    def test_a_span_from_the_body_lands_under_the_step_span_the_daemon_opened(self):
        h = Harness([claimed(wire_lease()), STARTED, ACKED, ok({"accepted": 1, "rejected": 0, "duplicate": 0})])

        def draft(ctx, input, prior):
            with ctx.span("model_call") as span:
                span.set_model("claude-sonnet-4", "anthropic")
            return "ok"

        h.worker.register(workflow("support").agent("classify", draft))
        h.worker.claim(1)
        h.worker.drain()
        self.assertTrue(h.mcpd.flush(5))

        ingest = [c for c in h.sender.calls if c["url"].endswith("/v1/ingest")]
        self.assertEqual(len(ingest), 1)
        spans = json.loads(ingest[0]["body"].decode("utf-8"))["spans"]
        self.assertEqual(len(spans), 1)
        self.assertEqual(spans[0]["name"], "model_call")
        self.assertEqual(spans[0]["trace_id"], TRACE)
        self.assertEqual(spans[0]["parent_span_id"], STEP_SPAN)
        self.assertEqual(spans[0]["correlation"]["session_id"], "s-1")
        h.close()

    def test_a_span_option_naming_a_correlation_field_with_none_keeps_the_run_s_value(self):
        h = Harness([claimed(wire_lease(correlation={
            "project": "test", "session_id": "s-1", "thread_id": "t-1", "user_id": "u-1",
            "request_id": "r-1", "release": "2026.09.1"})),
            STARTED, ACKED, ok({"accepted": 1, "rejected": 0, "duplicate": 0})])

        def draft(ctx, input, prior):
            # `session_id` is named with no value, which is not the same as
            # being told to erase it: the run's session reaches the span
            # anyway. `thread_id` carries a value, so that one wins.
            with ctx.span("model_call", session_id=None, thread_id="t-2"):
                pass
            return "ok"

        h.worker.register(workflow("support").agent("classify", draft))
        h.worker.claim(1)
        h.worker.drain()
        self.assertTrue(h.mcpd.flush(5))

        ingest = [c for c in h.sender.calls if c["url"].endswith("/v1/ingest")]
        correlation = json.loads(ingest[0]["body"].decode("utf-8"))["spans"][0]["correlation"]
        self.assertEqual(
            {k: correlation.get(k) for k in
             ("session_id", "thread_id", "user_id", "request_id", "release")},
            {"session_id": "s-1", "thread_id": "t-2", "user_id": "u-1",
             "request_id": "r-1", "release": "2026.09.1"})
        h.close()

    def test_a_branch_an_item_and_an_iteration_key_find_their_handlers(self):
        # Three bodies run on three threads, so the answers are routed by path
        # rather than scripted in an order the threads would not keep.
        h = Harness([
            claimed(wire_lease(lease_id="l-a", step_key="checks/fraud"),
                    wire_lease(lease_id="l-b", step_key="notify[2]"),
                    wire_lease(lease_id="l-c", step_key="refine[1]")),
        ], routes={"/started": STARTED, "/complete": ACKED}, concurrency=3)
        ran = []

        def record(name):
            def handler(ctx, input, prior):
                ran.append(name)
                return name
            return handler

        h.worker.register(
            workflow("support")
            .parallel("checks", lambda b: b.step("fraud", record("fraud")))
            .foreach("notify", lambda b: b.step("send", record("send")), over="input.to", max_items=3)
            .loop("refine", lambda b: b.step("rewrite", record("rewrite")), max_iterations=2)
        )
        self.assertEqual(h.worker.claim(3), 3)
        h.worker.drain()
        self.assertEqual(sorted(ran), ["fraud", "rewrite", "send"])
        self.assertEqual(len([p for p in h.paths() if p.endswith("/complete")]), 3)
        h.close()


class FailingALeaseTest(unittest.TestCase):
    def test_a_retryable_error_is_reported_with_its_class_and_delay(self):
        h = Harness([claimed(wire_lease()), STARTED, ok({"run_status": "running", "next": "idle", "attempts_left": 2})])

        def classify(ctx, input, prior):
            raise RetryableError("rate limited", retry_after_ms=250, details={"status_code": 429})

        h.worker.register(workflow("support").step("classify", classify))
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(h.paths()[2], "POST /v1/api/steps/lease-1/fail")
        self.assertEqual(h.body(2), {"error": {"class": "retryable", "message": "rate limited",
                                               "details": {"status_code": 429}, "retry_after_ms": 250}})
        h.close()

    def test_any_other_raise_is_fatal_and_a_step_failure_names_its_class(self):
        h = Harness([claimed(wire_lease(), wire_lease(lease_id="lease-2", step_key="draft"))],
                    routes={"/started": STARTED, "/fail": ACKED}, concurrency=2)

        def classify(ctx, input, prior):
            raise RuntimeError("the model returned nothing")

        def draft(ctx, input, prior):
            raise StepFailure("no such ticket", class_="invalid_input")

        h.worker.register(workflow("support").step("classify", classify).step("draft", draft))
        h.worker.claim(2)
        h.worker.drain()
        fails = sorted((json.loads(c["body"])["error"] for c in h.sender.calls if c["url"].endswith("/fail")),
                       key=lambda e: e["class"])
        self.assertEqual(fails, [{"class": "fatal", "message": "the model returned nothing"},
                                 {"class": "invalid_input", "message": "no such ticket"}])
        h.close()

    def test_a_lease_it_cannot_serve_is_handed_back_without_running_anything(self):
        h = Harness([claimed(wire_lease(definition_id="billing"), wire_lease(lease_id="lease-2", definition_version=7))],
                    routes={"/fail": ACKED}, concurrency=2)
        ran = []
        h.worker.register(workflow("support", version=1).step("classify", lambda ctx, i, p: ran.append(1)))
        h.worker.claim(2)
        h.worker.drain()
        self.assertEqual(ran, [])
        self.assertEqual(sorted(h.paths()[1:]), ["POST /v1/api/steps/lease-1/fail", "POST /v1/api/steps/lease-2/fail"])
        for call in h.sender.calls[1:]:
            error = json.loads(call["body"])["error"]
            self.assertEqual(error["class"], "unservable")
            self.assertIn("w-1 does not serve", error["message"])
        h.close()

    def test_without_on_error_one_line_per_kind_of_failure_per_interval_whatever_the_step(self):
        # Three leases this worker cannot serve, each with its own step key, and
        # a daemon that refuses every fail. Every report used to be written, so
        # a failing run logged one line per step.
        refused = Response(400, {}, b'{"error":"bad_request","message":"no"}')
        leases = [wire_lease(lease_id=f"lease-{n}", step_key=f"unknown_{n}") for n in (1, 2, 3)]
        h = Harness([claimed(*leases)], routes={"/fail": refused}, concurrency=3, on_error=None)
        h.worker.register(workflow("support"))
        with self.assertLogs("mcpd", level="WARNING") as logs:
            h.worker.claim(3)
            h.worker.drain()
        self.assertEqual(len([p for p in h.paths() if p.endswith("/fail")]), 3)
        self.assertEqual(len([line for line in logs.output if "call failed for" in line]), 1, logs.output)
        h.close()


class ConflictTableTest(unittest.TestCase):
    def test_already_applied_is_done_and_says_nothing(self):
        h = Harness([claimed(wire_lease()), STARTED, conflict("already_applied", attempt_status="succeeded")])
        h.worker.register(workflow("support").step("classify", lambda ctx, i, p: 1))
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(len(h.paths()), 3)
        self.assertEqual(h.errors, [])
        h.close()

    def test_a_body_whose_started_was_fenced_never_runs(self):
        h = Harness([claimed(wire_lease()), conflict("fenced", attempt_status="orphaned")])
        ran = []
        h.worker.register(workflow("support").step("classify", lambda ctx, i, p: ran.append(1)))
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(ran, [])
        self.assertEqual(h.paths(), ["POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started"])
        self.assertEqual(h.errors, [])
        h.close()

    def test_a_lease_whose_run_ended_is_abandoned_with_nothing_more_posted(self):
        h = Harness([claimed(wire_lease()), STARTED, conflict("run_terminal", run_status="cancelled")])
        h.worker.register(workflow("support").step("classify", lambda ctx, i, p: 1))
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(len(h.paths()), 3)
        self.assertEqual(h.errors, [])
        h.close()

    def test_not_started_is_logged_loudly_because_it_is_a_bug_in_the_worker(self):
        h = Harness([claimed(wire_lease(effect="at_most_once")), STARTED, conflict("not_started")])
        h.worker.register(workflow("support").step("classify", lambda ctx, i, p: 1))
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(len(h.errors), 1)
        self.assertIn("a bug in this worker", h.errors[0][0])
        self.assertIsInstance(h.errors[0][1], StepConflictError)
        h.close()

    def test_a_fenced_heartbeat_cancels_the_body_and_posts_no_complete(self):
        h = Harness([REGISTERED, claimed(wire_lease()), STARTED, conflict("fenced", attempt_status="orphaned")])
        reasons = []
        # The worker frees the slot at the fence without waiting for the body,
        # so the body's own record is waited for here rather than assumed.
        recorded = threading.Event()

        def classify(ctx, input, prior):
            ctx.cancelled.wait(5)
            reasons.append(ctx.cancel_reason)
            recorded.set()
            return "too late"

        h.worker.register(workflow("support").step("classify", classify))
        h.worker.register_definitions()
        h.worker.claim(1)
        h.worker.drain()
        self.assertTrue(recorded.wait(5), "the body never saw the cancellation")
        self.assertEqual(len(reasons), 1)
        self.assertIn("fenced", reasons[0])
        self.assertEqual(h.paths(), ["POST /v1/api/workers/register", "POST /v1/api/steps/claim",
                                     "POST /v1/api/steps/lease-1/started", "POST /v1/api/steps/lease-1/heartbeat"])
        self.assertEqual(h.errors, [])
        h.close()

    def test_a_heartbeat_that_fails_for_any_other_reason_is_reported_and_the_body_goes_on(self):
        # A 400 is not retried by the transport; a 5xx would be, and would land.
        h = Harness([REGISTERED, claimed(wire_lease()), STARTED,
                     Response(400, {}, b'{"error":"bad_request","message":"no"}'), ACKED])

        def classify(ctx, input, prior):
            time.sleep(1.15)
            return "done"

        h.worker.register(workflow("support").step("classify", classify))
        h.worker.register_definitions()
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(h.paths()[-1], "POST /v1/api/steps/lease-1/complete")
        self.assertTrue(any("heartbeat failed" in m for m, _ in h.errors))
        h.close()


class ServeLoopTest(unittest.TestCase):
    def test_it_registers_claims_until_stopped_and_returns_once_stopped(self):
        h = Harness([REGISTERED, IDLE], concurrency=2)
        h.worker.register(workflow("support").step("classify", lambda ctx, i, p: 1))
        thread = threading.Thread(target=h.worker.serve)
        thread.start()
        time.sleep(0.05)
        h.worker.stop()
        thread.join(5)
        self.assertFalse(thread.is_alive())

        self.assertEqual(h.paths()[0], "POST /v1/api/workers/register")
        self.assertEqual(h.body(0), {"project": "test", "worker_id": "w-1", "definitions": [
            {"id": "support", "kind": "workflow", "body": {"steps": [{"kind": "task", "id": "classify"}]}}]})
        self.assertTrue(all(p == "POST /v1/api/steps/claim" for p in h.paths()[1:]))
        self.assertGreater(len(h.sender.calls), 2)
        self.assertEqual(h.body(1), {"project": "test", "worker_id": "w-1", "max": 2, "wait_ms": 0})
        self.assertEqual(h.errors, [])
        h.close()

    def test_a_claim_that_failed_is_reported_and_tried_again(self):
        h = Harness([REGISTERED, Response(400, {}, b'{"error":"bad_request","message":"no"}'), IDLE])
        h.worker.register(workflow("support").step("classify", lambda ctx, i, p: 1))
        thread = threading.Thread(target=h.worker.serve)
        thread.start()
        time.sleep(0.05)
        h.worker.stop()
        thread.join(5)
        self.assertIn("claim failed", h.errors[0][0])
        h.close()


class RunApiTest(unittest.TestCase):
    def test_a_run_starts_with_the_fields_the_daemon_expects(self):
        h = Harness([ok({"run_id": "run-1", "project": "test", "definition_id": "support", "definition_version": 2,
                         "status": "running", "cursor": {"index": 0, "construct": "task"}, "correlation": {"project": "test"},
                         "trace_id": TRACE, "steps_used": 0, "cost_usd": 0, "root_run_id": "run-1", "depth": 0,
                         "created_at": "2026-09-01T10:00:00Z", "updated_at": "2026-09-01T10:00:00Z"})])
        run = h.mcpd.runs.start("support", version=2, input={"ticket_id": "T-1"}, idempotency_key="ticket:T-1",
                                correlation={"session_id": "s-1"})
        self.assertEqual(run.run_id, "run-1")
        self.assertEqual(run.status, "running")
        self.assertEqual(h.sender.calls[0]["url"], "http://mcpd.test/v1/api/runs")
        self.assertEqual(h.body(0), {"project": "test", "definition_id": "support", "version": 2,
                                     "input": {"ticket_id": "T-1"}, "idempotency_key": "ticket:T-1",
                                     "correlation": {"project": "test", "session_id": "s-1"}})
        h.close()

    def test_a_definition_is_stored_with_its_body_untouched(self):
        h = Harness([ok({"id": "support", "version": 1, "assigned": True})])
        wf = workflow("support").step("classify", lambda ctx, i, p: 1, step_timeout_ms=100)
        stored = h.mcpd.definitions.put(wf)
        self.assertEqual((stored.id, stored.version, stored.assigned), ("support", 1, True))
        self.assertEqual(h.sender.calls[0]["url"], "http://mcpd.test/v1/api/definitions?project=test")
        self.assertEqual(h.body(0), {"id": "support", "kind": "workflow",
                                     "body": {"steps": [{"kind": "task", "id": "classify", "step_timeout_ms": 100}]}})
        h.close()

    def test_watch_walks_the_pages_until_the_run_ends_passing_the_last_seq_it_saw(self):
        h = Harness([
            ok({"events": [
                {"run_id": "run-1", "seq": 0, "kind": "run_started", "payload": {"definition_id": "support"},
                 "at": "2026-09-01T10:00:00Z"},
                {"run_id": "run-1", "seq": 1, "kind": "step_leased", "payload": {"step_key": "classify"},
                 "at": "2026-09-01T10:00:01Z"}], "has_more": True}),
            ok({"events": [], "has_more": False}),
            ok({"events": [{"run_id": "run-1", "seq": 2, "kind": "run_ended", "payload": {"status": "succeeded"},
                            "at": "2026-09-01T10:00:02Z"}], "has_more": False}),
        ])
        kinds = [event.kind for event in h.mcpd.runs.watch("run-1", poll=0.001)]
        self.assertEqual(kinds, ["run_started", "step_leased", "run_ended"])
        self.assertEqual([c["url"] for c in h.sender.calls], [
            "http://mcpd.test/v1/api/runs/run-1/events?after=-1&limit=500",
            "http://mcpd.test/v1/api/runs/run-1/events?after=1&limit=500",
            "http://mcpd.test/v1/api/runs/run-1/events?after=1&limit=500",
        ])
        h.close()

    def test_publish_and_signal_carry_the_ids_the_caller_supplied(self):
        receipt = ok({"outcome": "delivered", "event_id": "evt-1"})
        h = Harness([receipt, receipt])
        h.mcpd.events.publish("approved", "T-1", payload={"by": "ops"}, event_id="evt-1")
        h.mcpd.runs.signal("run-1", "approved", payload={"by": "ops"}, event_id="evt-2")
        self.assertEqual(h.sender.calls[0]["url"], "http://mcpd.test/v1/api/events")
        self.assertEqual(h.body(0), {"project": "test", "event": "approved", "key": "T-1", "payload": {"by": "ops"},
                                     "event_id": "evt-1"})
        self.assertEqual(h.sender.calls[1]["url"], "http://mcpd.test/v1/api/runs/run-1/signal")
        self.assertEqual(h.body(1), {"event": "approved", "payload": {"by": "ops"}, "event_id": "evt-2"})
        h.close()

    def test_a_409_from_a_worker_call_is_a_typed_conflict_and_any_other_409_is_itself(self):
        h = Harness([conflict("fenced", attempt_status="orphaned"),
                     Response(409, {}, b'{"error":"version_taken","message":"support@1 exists with a different body"}')])
        with self.assertRaises(StepConflictError) as caught:
            h.mcpd.steps.complete("lease-1", output=1)
        self.assertEqual(caught.exception.conflict.reason, "fenced")
        self.assertEqual(caught.exception.conflict.attempt_status, "orphaned")
        self.assertEqual(caught.exception.status, 409)

        with self.assertRaises(McpdError) as plain:
            h.mcpd.definitions.put(workflow("support", version=1))
        self.assertNotIsInstance(plain.exception, StepConflictError)
        self.assertIn("version_taken", plain.exception.body)
        h.close()



class DeadlineTest(unittest.TestCase):
    def test_a_body_that_runs_past_the_lease_deadline_is_cancelled_and_failed_retryable(self):
        h = Harness([claimed(wire_lease(deadline_ms=150)), STARTED, ACKED])
        reasons = []
        # The worker frees the slot at the deadline without waiting for the
        # body, so the body's own record is waited for rather than assumed.
        recorded = threading.Event()

        def classify(ctx, input, prior):
            ctx.cancelled.wait(5)
            reasons.append(ctx.cancel_reason)
            recorded.set()
            return "too late"

        h.worker.register(workflow("support").step("classify", classify))
        h.worker.claim(1)
        h.worker.drain()
        self.assertTrue(recorded.wait(5), "the body never saw the cancellation")
        self.assertEqual(reasons, ["step deadline of 150ms passed"])
        self.assertEqual(h.paths(), ["POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started",
                                     "POST /v1/api/steps/lease-1/fail"])
        self.assertEqual(h.body(2), {"error": {"class": "retryable", "message": "step deadline of 150ms passed"}})
        self.assertEqual(h.errors, [])
        h.close()

    def test_the_slot_is_freed_at_the_deadline_even_when_the_body_ignores_cancelled(self):
        import time

        h = Harness([claimed(wire_lease(deadline_ms=100)), STARTED, ACKED])
        # A body that never looks at ctx.cancelled.
        h.worker.register(workflow("support").step("classify", lambda ctx, i, p: time.sleep(3)))
        h.worker.claim(1)
        began = time.monotonic()
        h.worker.drain()
        self.assertLess(time.monotonic() - began, 1.0, "the slot was held past the deadline")
        self.assertEqual(h.worker.stats().inflight, 0)
        self.assertEqual(h.paths()[-1], "POST /v1/api/steps/lease-1/fail")
        h.close()

    def test_a_body_that_finishes_in_time_is_left_alone(self):
        h = Harness([claimed(wire_lease(deadline_ms=5_000)), STARTED, ACKED])
        h.worker.register(workflow("support").step("classify", lambda ctx, i, p: "quick"))
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(h.paths()[-1], "POST /v1/api/steps/lease-1/complete")
        h.close()


class DecodingTest(unittest.TestCase):
    def test_a_decoder_that_rejects_the_input_fails_the_step_invalid_input(self):
        h = Harness([claimed(wire_lease()), STARTED, ACKED])

        def classify(ctx, input, prior):
            self.assertEqual(list(ctx.prior(dict)), ["triage"])
            self.assertEqual(ctx.state_as(dict)["seen_count"], 1)

            def ticket(value):
                if not isinstance(value.get("ticket_id"), int):
                    raise ValueError("ticket_id must be a number")
                return value

            return ctx.input(ticket)

        h.worker.register(workflow("support").step("classify", classify))
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(h.paths()[2], "POST /v1/api/steps/lease-1/fail")
        self.assertEqual(h.body(2), {"error": {"class": "invalid_input",
                                               "message": "the step's input could not be read: ticket_id must be a number"}})
        h.close()


class WaitTest(unittest.TestCase):
    @staticmethod
    def run_body(status, **extra):
        run = {"run_id": "run-1", "project": "test", "definition_id": "support", "definition_version": 1,
               "status": status, "cursor": {"index": 2, "construct": "end"}, "correlation": {"project": "test"},
               "trace_id": TRACE, "steps_used": 2, "cost_usd": 0.25, "root_run_id": "run-1", "depth": 0,
               "created_at": "2026-09-01T10:00:00Z", "updated_at": "2026-09-01T10:00:05Z"}
        run.update(extra)
        return {"run": run, "attempts": [], "timers": [], "waits": []}

    @staticmethod
    def ended():
        return ok({"events": [{"run_id": "run-1", "seq": 0, "kind": "run_ended", "at": "2026-09-01T10:00:05Z"}],
                   "has_more": False})

    def test_returns_the_output_once_the_run_ends(self):
        h = Harness([self.ended(), ok(self.run_body("succeeded", output={"answer": 42}))])
        result = h.mcpd.runs.wait("run-1", poll=0.001)
        self.assertEqual(result.status, "succeeded")
        self.assertEqual(result.output, {"answer": 42})
        self.assertEqual(result.cost_usd, 0.25)
        self.assertEqual(result.trace_id, TRACE)
        h.close()

    def test_raises_a_typed_error_carrying_the_run_when_it_did_not_succeed(self):
        h = Harness([self.ended(), ok(self.run_body("failed", error="send: relay down"))])
        with self.assertRaises(RunFailedError) as caught:
            h.mcpd.runs.wait("run-1", poll=0.001)
        self.assertEqual(caught.exception.run_status, "failed")
        self.assertEqual(caught.exception.run.error, "send: relay down")
        self.assertEqual(str(caught.exception), "run run-1 failed: send: relay down")
        h.close()

    def test_gives_up_after_the_timeout_while_the_run_is_still_going(self):
        h = Harness([], {"/events": ok({"events": [], "has_more": False})})
        with self.assertRaises(TimeoutError):
            h.mcpd.runs.wait("run-1", poll=0.005, timeout=0.04)
        h.close()

    def test_starts_and_waits_in_one_call(self):
        h = Harness([ok(self.run_body("running")["run"]), self.ended(), ok(self.run_body("succeeded", output="done"))])
        result = h.mcpd.runs.start_and_wait("support", input={"a": 1}, poll=0.001)
        self.assertEqual(result.output, "done")
        self.assertEqual(h.body(0), {"project": "test", "definition_id": "support", "input": {"a": 1}})
        h.close()


class UsageAccumulationTest(unittest.TestCase):
    """Section 7.3: what the body's spans spend is the step's usage."""

    def test_spans_under_the_context_sum_into_the_step_and_the_last_model_wins(self):
        h = Harness([claimed(wire_lease()), STARTED, ACKED], sink=RecordingSink())

        def draft(ctx, input, prior):
            with ctx.span("first_call", kind="model_generation") as first:
                first.set_model("claude-sonnet-4", "anthropic")
                first.set_usage(input_tokens=100, output_tokens=10, total_tokens=110)
                # Ambient nesting: a span opened through the client under the
                # active one inherits the step, and so does an explicit child.
                with h.mcpd.span("nested") as nested:
                    nested.set_usage(input_tokens=20, output_tokens=5)
                first.child("grandchild", usage={"input_tokens": 1}).end()
            with ctx.span("second_call", kind="model_generation") as second:
                second.set_model("gpt-4o", "openai")
                second.set_usage(input_tokens=7, output_tokens=3)
            with ctx.span("no_usage"):
                pass
            return "text"

        h.worker.register(workflow("support").step("classify", draft))
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(h.paths()[2], "POST /v1/api/steps/lease-1/complete")
        self.assertEqual(h.body(2)["usage"], {
            "input_tokens": 128, "output_tokens": 18, "total_tokens": 110,
            "model": "gpt-4o", "provider": "openai",
        })
        h.mcpd.flush(2)
        # The spans themselves still carry their own usage for the trace.
        ended = {s.name: s for s in h.sink.spans if s.end_time is not None}
        self.assertEqual(ended["first_call"].usage.input_tokens, 100)
        self.assertEqual(ended["first_call"].parent_span_id, STEP_SPAN)
        self.assertEqual(ended["nested"].parent_span_id, ended["first_call"].span_id)
        h.close()

    def test_set_usage_is_an_explicit_override_field_by_field(self):
        h = Harness([claimed(wire_lease()), STARTED, ACKED], sink=RecordingSink())

        def draft(ctx, input, prior):
            with ctx.span("call") as span:
                span.set_model("m", "p")
                span.set_usage(input_tokens=100, output_tokens=10)
            ctx.set_usage(input_tokens=1, model="hand-set")
            return "text"

        h.worker.register(workflow("support").step("classify", draft))
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(h.body(2)["usage"], {"input_tokens": 1, "output_tokens": 10, "model": "hand-set",
                                              "provider": "p"})
        h.close()

    def test_a_body_that_records_no_spans_reports_what_it_sets(self):
        h = Harness([claimed(wire_lease()), STARTED, ACKED], sink=RecordingSink())
        h.worker.register(workflow("support").step("classify", lambda ctx, i, p: ctx.set_usage(input_tokens=4)))
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(h.body(2)["usage"], {"input_tokens": 4})
        h.close()

    def test_a_span_ended_twice_counts_once(self):
        h = Harness([claimed(wire_lease()), STARTED, ACKED], sink=RecordingSink())

        def draft(ctx, input, prior):
            span = ctx.span("call")
            span.set_usage(input_tokens=5)
            span.end()
            span.end()
            return None

        h.worker.register(workflow("support").step("classify", draft))
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(h.body(2)["usage"], {"input_tokens": 5})
        h.close()


class HooksAndStatsTest(unittest.TestCase):
    """Section 7.2: the four hooks and `stats()`."""

    def hooked(self, script, **kwargs):
        calls = []
        h = Harness(script, sink=RecordingSink(),
                    on_lease=lambda lease: calls.append(("lease", lease.lease_id)),
                    on_step_start=lambda ctx: calls.append(("start", ctx.step_key)),
                    on_step_end=lambda ctx, outcome: calls.append(("end", ctx.step_key, outcome)),
                    on_conflict=lambda lease, reason: calls.append(("conflict", lease.lease_id, reason)),
                    **kwargs)
        return h, calls

    def test_a_completed_step_calls_three_hooks_once_each(self):
        h, calls = self.hooked([claimed(wire_lease()), STARTED, ACKED])
        h.worker.register(workflow("support").step("classify", lambda ctx, i, p: 1))
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(calls, [("lease", "lease-1"), ("start", "classify"),
                                 ("end", "classify", StepOutcome("completed"))])
        stats = h.worker.stats()
        self.assertEqual((stats.claimed, stats.inflight, stats.completed, stats.unservable), (1, 0, 1, 0))
        self.assertEqual((stats.failed, stats.conflicts), ({}, {}))
        h.close()

    def test_a_fatal_failure_reaches_on_step_end_with_its_class_and_message(self):
        h, calls = self.hooked([claimed(wire_lease()), STARTED, ACKED])

        def classify(ctx, input, prior):
            raise RuntimeError("the model returned nothing")

        h.worker.register(workflow("support").step("classify", classify))
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(calls[-1], ("end", "classify", StepOutcome("failed", "fatal", "the model returned nothing")))
        self.assertEqual(calls[-1][2].error_class, "fatal")
        self.assertEqual(h.worker.stats().failed, {"fatal": 1})
        self.assertEqual(h.worker.stats().completed, 0)
        h.close()

    def test_a_fenced_started_reaches_on_conflict_and_no_body_hook(self):
        h, calls = self.hooked([claimed(wire_lease()), conflict("fenced", attempt_status="orphaned")])
        h.worker.register(workflow("support").step("classify", lambda ctx, i, p: 1))
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(calls, [("lease", "lease-1"), ("conflict", "lease-1", "fenced")])
        self.assertEqual(h.worker.stats().conflicts, {"fenced": 1})
        self.assertEqual(h.errors, [])
        h.close()

    def test_an_unservable_lease_is_counted_and_runs_no_body_hook(self):
        h, calls = self.hooked([claimed(wire_lease(definition_id="billing")), ACKED])
        h.worker.register(workflow("support").step("classify", lambda ctx, i, p: 1))
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(calls, [("lease", "lease-1")])
        self.assertEqual(h.worker.stats().unservable, 1)
        self.assertEqual(h.worker.stats().failed, {})
        h.close()

    def test_a_hook_that_raises_is_reported_and_the_lease_still_completes(self):
        def explode(ctx):
            raise ValueError("hook bug")

        h = Harness([claimed(wire_lease()), STARTED, ACKED], sink=RecordingSink(), on_step_start=explode)
        h.worker.register(workflow("support").step("classify", lambda ctx, i, p: "done"))
        h.worker.claim(1)
        h.worker.drain()
        self.assertEqual(h.paths()[2], "POST /v1/api/steps/lease-1/complete")
        self.assertEqual(h.body(2), {"output": "done"})
        self.assertEqual(len(h.errors), 1)
        self.assertIn("on_step_start hook raised", h.errors[0][0])
        self.assertIsInstance(h.errors[0][1], ValueError)
        self.assertEqual(h.worker.stats().completed, 1)
        h.close()
