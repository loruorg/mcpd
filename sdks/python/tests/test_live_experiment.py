"""The experiment runner against a real mcpd, with every empty shape it sends.

Skipped unless `MCPD_TEST_ENDPOINT` names a running server, like test_live.py.
"""

import json
import os
import time
import unittest
import urllib.request

from mcpd import Experiment, Mcpd, ScorerConfig

ENDPOINT = os.environ.get("MCPD_TEST_ENDPOINT")


def seed_dataset(project, cases):
    """A dataset holding `cases`, written over plain HTTP so only the runner is under test."""
    dataset_id = f"py-live-{time.time_ns()}"

    def post(path, body):
        request = urllib.request.Request(
            ENDPOINT.rstrip("/") + path, data=json.dumps(body).encode(),
            headers={"content-type": "application/json"}, method="POST")
        with urllib.request.urlopen(request, timeout=10) as answer:
            answer.read()

    post("/v1/api/datasets", {"dataset_id": dataset_id, "name": dataset_id, "item_count": 0,
                              "created_at": "2026-01-01T00:00:00Z",
                              "correlation": {"project": project}})
    post(f"/v1/api/datasets/{dataset_id}/items",
         [{"item_id": f"{dataset_id}-{i}", "dataset_id": dataset_id, **case}
          for i, case in enumerate(cases)])
    return dataset_id


@unittest.skipUnless(ENDPOINT, "set MCPD_TEST_ENDPOINT to run the live tests")
class LiveExperimentTest(unittest.TestCase):
    def test_an_experiment_runs_over_nulls_and_empty_lists(self):
        project = "sdk-python-experiment"
        mcpd = Mcpd(ENDPOINT, project=project, environment="test", flush_interval=0.05)
        self.addCleanup(mcpd.shutdown)
        nested = {"q": "hi", "n": None, "l": []}
        dataset_id = seed_dataset(project, [{"input": nested, "expected": "hi"},
                                            {"input": None, "expected": "x"}])
        seen = []
        results = mcpd.run_experiment(dataset_id, lambda case: seen.append(case) or "hi",
                                      config={"needle": "hi"})
        self.assertCountEqual(seen, [nested, None])
        self.assertEqual((results.runs, results.failed, len(results.items)), (2, 0, 2))
        recorded = {}
        for item in results.items:
            root = next(s for s in mcpd.get_trace(item.trace_id).spans if s.is_root)
            recorded[item.item_id] = root.input
        self.assertEqual(recorded[f"{dataset_id}-0"], nested)
        self.assertIsNone(recorded[f"{dataset_id}-1"])

        experiment = mcpd.create_experiment(Experiment(
            experiment_id="", name="py-live-empty", dataset_id=dataset_id,
            status="running", item_count=0, correlation=None))
        mcpd.score_experiment(experiment.experiment_id)
        mcpd.score_experiment(experiment.experiment_id, [], ScorerConfig())


if __name__ == "__main__":
    unittest.main()
