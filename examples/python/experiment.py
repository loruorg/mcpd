"""An experiment in Python: run every case of a dataset through your own code,
score each answer against its case, and read the results.

The regions are read into the documentation at build time.
"""

from mcpd import Mcpd

mcpd = Mcpd(project="support")


# The code under test. Anything it records nests under the case's run.
def answer(input):
    with mcpd.span("model_call", kind="model_generation") as span:
        span.set_model("claude-sonnet-4", "anthropic")
        if "refund" in input["question"]:
            return "Refunds take five days."
        return "It ships tomorrow."


# #region experiment
# Each case runs in its own root span, four at a time. A case that raises is
# recorded as a failed run, and the others still run. An `async def` target
# works too: from async code, call `await mcpd.arun_experiment(...)`.
results = mcpd.run_experiment(
    "support-questions",
    answer,
    name="answers-v2",
    scorers=["exact_match", "similarity"],
    metadata={"prompt": "answer@4"},
)

for scorer in results.scorers:
    print(f"{scorer.scorer_id}: mean {scorer.mean:.2f} over {scorer.n}")
for item in results.items:
    if item.status == "error":
        print(f"case {item.item_id} failed: {item.trace_id}")

# Read one run back as the steps its agent took.
if results.items:
    trajectory = mcpd.get_trajectory(results.items[0].trace_id)
    print(f"{trajectory.step_count} steps, tools: {' > '.join(trajectory.tool_sequence)}")
# #endregion experiment

mcpd.shutdown()
