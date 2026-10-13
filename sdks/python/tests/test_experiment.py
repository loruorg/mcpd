"""Experiments (contract section 5.2): the reads and writes, and the runner."""

import asyncio
import threading
import time
import unittest

from mcpd import (
    DATASET_ITEM_ATTRIBUTE,
    Correlation,
    Experiment,
    ExperimentResults,
    ExperimentStatus,
    McpdError,
    ScorerConfig,
    ScoreRunResult,
    SpanKind,
    SpanStatus,
    Trajectory,
)
from mcpd.testing import TestClient
from mcpd.transport import TransportError, encode, encode_params

from tests.test_client import FakeTransport

TRACE = "0af7651916cd43dd8448eb211c80319c"


def item(n, value=None):
    return {"item_id": f"case-{n}", "dataset_id": "qs", "input": {"q": value or f"q{n}"},
            "expected": f"a{n}", "metadata": {"Mixed_Key": n}}


class Daemon(FakeTransport):
    """Answers the experiment endpoints, and remembers every request in order."""

    def __init__(self, items=(), items_error=None, fail_failed_post=False):
        super().__init__()
        self.items = list(items)
        self.items_error = items_error
        self.fail_failed_post = fail_failed_post
        self.requests = []
        self.lock_requests = threading.Lock()

    def note(self, method, path, query, body):
        with self.lock_requests:
            self.requests.append((method, path, query, body))

    def get(self, path, params=None):
        self.note("GET", path, encode_params(params or {}), None)
        if path.endswith("/items"):
            if self.items_error is not None:
                raise self.items_error
            return self.items
        if path.endswith("/trajectory"):
            return {"trace_id": TRACE, "step_count": 2, "tool_count": 1, "error_count": 0,
                    "goal_reached": True, "tool_sequence": ["search"],
                    "steps": [{"index": 0, "span_id": "b7ad6b7169203331", "kind": "tool_call",
                               "name": "search", "status": "ok", "start_offset_ms": 1.5}]}
        if path.endswith("/results"):
            return {"experiment_id": "exp-1", "runs": 3, "failed": 1,
                    "items": [{"trace_id": TRACE, "status": "ok", "item_id": "case-1",
                               "scores": {"exact_match": 1.0, "llm_judge:Tone": 0.5},
                               "expected": {"Answer_Text": "a1"}}],
                    "scorers": [{"scorer_id": "exact_match", "n": 3, "mean": 0.5, "min": 0.0,
                                 "max": 1.0, "direction": "higher_is_better"}]}
        raise AssertionError(f"unexpected GET {path}")

    def post(self, path, value, hold=0.0):
        path, _, query = path.partition("?")
        body = encode(value)
        self.note("POST", path, query, body)
        if path == "/v1/api/experiments":
            if body["status"] == "failed" and self.fail_failed_post:
                raise TransportError("the daemon went away", status=503)
            return {**body, "experiment_id": body["experiment_id"] or "exp-1"}
        if path.endswith("/score"):
            return {"traces_scored": 3, "scores_written": 3}
        raise AssertionError(f"unexpected POST {path}")

    def experiment_posts(self):
        return [r[3] for r in self.requests if r[1] == "/v1/api/experiments"]


def client(daemon, **options):
    return TestClient("http://mcpd.test", project="tests", transport=daemon,
                      flush_interval=0.02, **options)


class ReadWriteTest(unittest.TestCase):
    def test_get_trajectory_reads_its_path_with_the_project(self):
        d = Daemon()
        c = client(d)
        t = c.get_trajectory("trace/1")
        self.assertIsInstance(t, Trajectory)
        self.assertEqual(t.tool_sequence, ["search"])
        self.assertIs(t.steps[0].kind, SpanKind.TOOL_CALL)
        self.assertEqual(d.requests, [("GET", "/v1/api/traces/trace%2F1/trajectory",
                                       "project=tests", None)])
        c.shutdown()

    def test_list_dataset_items_sends_limit_and_nothing_else(self):
        d = Daemon(items=[item(1)])
        c = client(d)
        found = c.list_dataset_items("qs/1", limit=10)
        self.assertEqual(found[0].item_id, "case-1")
        self.assertEqual(found[0].input, {"q": "q1"})
        self.assertEqual(found[0].metadata, {"Mixed_Key": 1}, "free-form JSON comes back as sent")
        c.list_dataset_items("qs")
        self.assertEqual([r[1:3] for r in d.requests], [
            ("/v1/api/datasets/qs%2F1/items", "limit=10"),
            ("/v1/api/datasets/qs/items", ""),
        ])
        c.shutdown()

    def test_create_experiment_fills_the_project_and_keeps_free_form_json(self):
        d = Daemon()
        c = client(d)
        made = c.create_experiment(Experiment(
            experiment_id="", name="v2", dataset_id="qs", status=ExperimentStatus.RUNNING,
            item_count=0, correlation=Correlation(project=""), metadata={"Prompt_Version": "answer@4"}))
        self.assertEqual(made.experiment_id, "exp-1")
        self.assertIs(made.status, ExperimentStatus.RUNNING)
        body = d.requests[0][3]
        self.assertEqual(body["correlation"], {"project": "tests"})
        self.assertEqual(body["metadata"], {"Prompt_Version": "answer@4"})
        c.create_experiment(Experiment(
            experiment_id="exp-1", name="v2", dataset_id="qs", status=ExperimentStatus.RUNNING,
            item_count=0, correlation=Correlation(project="other")))
        self.assertEqual(d.requests[1][3]["correlation"], {"project": "other"},
                         "a project the caller named is kept")
        c.shutdown()

    def test_score_experiment_posts_the_scorers_and_config(self):
        d = Daemon()
        c = client(d)
        result = c.score_experiment("exp/1", ["exact_match"],
                                    {"json_schema": {"type": "object", "minLength": 2}})
        self.assertIsInstance(result, ScoreRunResult)
        self.assertEqual(result.scores_written, 3)
        c.score_experiment("exp-1", [])
        c.score_experiment("exp-1", ["contains"], ScorerConfig(needle="Refund"))
        method, path, query, body = d.requests[0]
        self.assertEqual((method, path, query), ("POST", "/v1/api/experiments/exp%2F1/score",
                                                 "project=tests"))
        self.assertEqual(body, {"scorers": ["exact_match"],
                                "config": {"json_schema": {"type": "object", "minLength": 2}}})
        self.assertEqual(d.requests[1][3], {"scorers": []}, "no config is no config key")
        self.assertEqual(d.requests[2][3]["config"]["needle"], "Refund")
        c.shutdown()

    def test_experiment_results_reads_its_path_and_keeps_the_scores_map(self):
        d = Daemon()
        c = client(d)
        results = c.experiment_results("exp-1")
        self.assertIsInstance(results, ExperimentResults)
        self.assertEqual(results.items[0].scores, {"exact_match": 1.0, "llm_judge:Tone": 0.5})
        self.assertEqual(results.items[0].expected, {"Answer_Text": "a1"})
        self.assertEqual(results.scorers[0].mean, 0.5)
        self.assertEqual(d.requests, [("GET", "/v1/api/experiments/exp-1/results",
                                       "project=tests", None)])
        c.shutdown()


class RunnerTest(unittest.TestCase):
    def test_a_run_records_one_root_span_per_case_and_carries_on_past_a_failure(self):
        d = Daemon(items=[item(1), item(2, "boom"), item(3)])
        c = client(d)
        seen = []

        def target(input):
            seen.append(input["q"])
            with c.span("model_call", kind="model_generation"):
                if input["q"] == "boom":
                    raise RuntimeError("the model refused")
                return f"answer to {input['q']}"

        results = c.run_experiment("qs", target, name="answers-v2", scorers=["exact_match"],
                                   concurrency=1, metadata={"prompt": "answer@4"})
        self.assertEqual(results.experiment_id, "exp-1")
        self.assertEqual(seen, ["q1", "boom", "q3"], "the case after a failure still runs")

        roots = {s.attributes[DATASET_ITEM_ATTRIBUTE]: s for s in c.spans("answers-v2")}
        self.assertEqual(sorted(roots), ["case-1", "case-2", "case-3"])
        for case, root in roots.items():
            self.assertTrue(root.is_root)
            self.assertIs(root.kind, SpanKind.AGENT_RUN)
            self.assertEqual(root.correlation.experiment_id, "exp-1")
            self.assertEqual(root.correlation.project, "tests")
        self.assertEqual(roots["case-1"].input, {"q": "q1"})
        self.assertEqual(roots["case-1"].output, "answer to q1")
        self.assertIs(roots["case-1"].status, SpanStatus.OK)
        self.assertEqual(roots["case-3"].output, "answer to q3")
        self.assertIs(roots["case-2"].status, SpanStatus.ERROR)
        self.assertEqual(roots["case-2"].error.message, "the model refused")
        children = c.spans("model_call")
        self.assertEqual(len(children), 3)
        parents = {s.span_id for s in roots.values()}
        for child in children:
            self.assertIn(child.parent_span_id, parents, "the target's spans nest under the case")

        posts = d.experiment_posts()
        self.assertEqual(len(posts), 2)
        self.assertEqual(posts[0]["status"], "running")
        self.assertEqual(posts[0]["experiment_id"], "")
        self.assertEqual(posts[0]["name"], "answers-v2")
        self.assertEqual(posts[0]["dataset_id"], "qs")
        self.assertEqual(posts[0]["item_count"], 0)
        self.assertEqual(posts[0]["metadata"], {"prompt": "answer@4"})
        self.assertTrue(posts[0]["started_at"].endswith("Z"))
        self.assertEqual(posts[1]["status"], "succeeded")
        self.assertEqual(posts[1]["experiment_id"], "exp-1")
        self.assertEqual(posts[1]["item_count"], 3)
        self.assertEqual(posts[1]["summary"], {"exact_match": 0.5})
        self.assertIn("finished_at", posts[1])
        score = [r for r in d.requests if r[1].endswith("/score")]
        self.assertEqual(score, [("POST", "/v1/api/experiments/exp-1/score", "project=tests",
                                  {"scorers": ["exact_match"]})])
        items_read = [r for r in d.requests if r[1].endswith("/items")]
        self.assertEqual(items_read[0][2], "limit=5000")
        c.shutdown()

    def test_every_case_is_kept_whatever_the_sample_rate(self):
        d = Daemon(items=[item(1), item(2)])
        c = client(d, sample=0.0)
        c.run_experiment("qs", lambda input: "ok")
        self.assertEqual(len(c.spans("qs")), 2, "a sampled-out case is a case with no answer")
        c.shutdown()

    def assert_bounded(self, run):
        d = Daemon(items=[item(n) for n in range(12)])
        c = client(d)
        state = {"now": 0, "max": 0}
        lock = threading.Lock()
        run(c, state, lock)
        self.assertEqual(state["max"], 3, "never more targets in flight than concurrency")
        self.assertEqual(len(c.spans("qs")), 12)
        c.shutdown()

    def test_no_more_than_concurrency_plain_targets_run_at_once(self):
        def run(c, state, lock):
            def target(input):
                with lock:
                    state["now"] += 1
                    state["max"] = max(state["max"], state["now"])
                time.sleep(0.02)
                with lock:
                    state["now"] -= 1
                return "ok"

            c.run_experiment("qs", target, concurrency=3)

        self.assert_bounded(run)

    def test_no_more_than_concurrency_async_targets_run_at_once(self):
        def run(c, state, lock):
            async def target(input):
                state["now"] += 1
                state["max"] = max(state["max"], state["now"])
                await asyncio.sleep(0.02)
                state["now"] -= 1
                return "ok"

            c.run_experiment("qs", target, concurrency=3)

        self.assert_bounded(run)

    def test_an_async_target_nests_its_spans_and_records_its_error(self):
        d = Daemon(items=[item(1), item(2, "boom")])
        c = client(d)

        async def target(input):
            # A task the target starts copies the context, so it nests too.
            async def call():
                with c.span("model_call", kind="model_generation"):
                    await asyncio.sleep(0)
                    if input["q"] == "boom":
                        raise ValueError("bad answer")
                    return "fine"

            return await asyncio.ensure_future(call())

        async def main():
            return await c.arun_experiment("qs", target)

        asyncio.run(main())
        roots = {s.attributes[DATASET_ITEM_ATTRIBUTE]: s for s in c.spans("qs")}
        self.assertEqual(roots["case-1"].output, "fine")
        self.assertEqual(roots["case-2"].error.message, "bad answer")
        parents = {s.span_id for s in roots.values()}
        self.assertEqual({s.parent_span_id for s in c.spans("model_call")}, parents)
        self.assertEqual(d.experiment_posts()[-1]["status"], "succeeded")
        c.shutdown()

    def test_an_async_target_from_inside_a_loop_must_use_arun(self):
        d = Daemon(items=[item(1)])
        c = client(d)

        async def target(input):
            return "ok"

        async def main():
            with self.assertRaises(McpdError) as caught:
                c.run_experiment("qs", target)
            self.assertIn("arun_experiment", str(caught.exception))

        asyncio.run(main())
        self.assertEqual(d.requests, [], "refused before the first request")
        c.shutdown()

    def test_a_failed_read_marks_the_experiment_failed_and_raises(self):
        d = Daemon(items_error=TransportError("GET items failed: 403 forbidden", status=403))
        c = client(d)
        called = []
        with self.assertRaises(TransportError) as caught:
            c.run_experiment("qs", called.append)
        self.assertEqual(caught.exception.status, 403)
        self.assertEqual(called, [], "the target never runs")
        posts = d.experiment_posts()
        self.assertEqual([p["status"] for p in posts], ["running", "failed"])
        self.assertEqual(posts[1]["experiment_id"], "exp-1")
        self.assertIn("403", posts[1]["error"])
        self.assertIn("finished_at", posts[1])
        c.shutdown()

    def test_a_failure_to_record_failed_never_hides_the_original_error(self):
        d = Daemon(items_error=TransportError("GET items failed: 403", status=403),
                   fail_failed_post=True)
        c = client(d)
        with self.assertLogs("mcpd", "WARNING") as logged:
            with self.assertRaises(TransportError) as caught:
                c.run_experiment("qs", lambda input: None)
        self.assertEqual(caught.exception.status, 403)
        self.assertIn("could not record the failed status", "\n".join(logged.output))
        c.shutdown()

    def test_an_undelivered_flush_fails_the_run(self):
        d = Daemon(items=[item(1)])
        c = client(d)
        c.flush = lambda timeout=10.0: False
        with self.assertRaises(McpdError) as caught:
            c.run_experiment("qs", lambda input: "ok")
        self.assertIn("flush deadline", str(caught.exception))
        self.assertEqual(d.experiment_posts()[-1]["status"], "failed")
        self.assertFalse([r for r in d.requests if r[1].endswith("/score")], "nothing scored")
        c.shutdown()

    def test_a_bad_concurrency_or_a_disabled_client_is_refused_with_no_request(self):
        d = Daemon(items=[item(1)])
        c = client(d)
        for bad in (0, -1, 1.5, True):
            with self.assertRaises(ValueError, msg=repr(bad)) as caught:
                c.run_experiment("qs", lambda input: "ok", concurrency=bad)
            self.assertIn("concurrency", str(caught.exception))
        c.shutdown()
        off = client(d, enabled=False)
        with self.assertRaises(McpdError) as caught:
            off.run_experiment("qs", lambda input: "ok")
        self.assertIn("enabled", str(caught.exception))
        self.assertEqual(d.requests, [], "refused before the first request")


if __name__ == "__main__":
    unittest.main()
