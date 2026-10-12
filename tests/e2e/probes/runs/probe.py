"""The canonical run, served in Python. Prints one JSON line at the end."""
import json
import os
import time

from mcpd import Mcpd, StepFailure, Worker, workflow

with open(os.path.join(os.path.dirname(__file__), "canonical.json")) as f:
    canonical = json.load(f)

mcpd = Mcpd(endpoint=os.environ["MCPD_ENDPOINT"], project=os.environ["MCPD_PROJECT"])
wf = workflow(canonical["definition_id"])


@wf.step("classify")
def classify(ctx, input, prior):
    return {"kind": "question" if input["ticket"].startswith("Q") else "bug"}


@wf.agent("draft", input={"kind": "${steps.classify.output.kind}"})
def draft(ctx, input, prior):
    ctx.set_state(drafted=True)
    with ctx.span("model_call", kind="model_generation") as span:
        span.set_model("claude-sonnet-4", "anthropic")
        span.set_usage(input_tokens=120, output_tokens=40)
    return f"an answer to a {input['kind']}"


wf.wait_for("approval", event="approved", key="${input.ticket}", timeout_ms=60_000, accept_parked=True)


@wf.step(
    "send",
    input={"text": "${steps.draft.output}", "by": "${steps.approval.output.by}"},
    effect="at_most_once",
    retry={"max_attempts": 2, "backoff": "fixed", "initial_ms": 100},
)
def send(ctx, input, prior):
    if ctx.attempt == 0:
        raise StepFailure("relay busy", class_="retryable", retry_after_ms=100)
    return {"sent": True, "by": input["by"]}


wf.output({"answer": "${steps.draft.output}", "sent": "${steps.send.output.sent}", "by": "${steps.send.output.by}"})


def loud(message, error):
    raise RuntimeError(f"{message}: {error}")


worker = Worker(mcpd, worker_id="probe-python", wait_ms=1_000, on_error=loud).register(wf)
worker.register_definitions()

run = mcpd.runs.start(canonical["definition_id"], input=canonical["input"], correlation=canonical["correlation"])
event = canonical["event"]
first = mcpd.events.publish(event["name"], event["key"], payload=event["payload"], event_id=event["event_id"])
second = mcpd.events.publish(event["name"], event["key"], payload=event["payload"], event_id=event["event_id"])

detail = mcpd.runs.get(run.run_id)
for _ in range(60):
    if detail.run.status != "running":
        break
    worker.claim(4, run.run_id)
    worker.drain()
    detail = mcpd.runs.get(run.run_id)
    if detail.run.status == "running":
        time.sleep(0.1)
mcpd.shutdown()
print(json.dumps({"run_id": run.run_id, "trace_id": run.trace_id, "status": str(getattr(detail.run.status, "value", detail.run.status)),
                  "publish": [str(getattr(r.outcome, "value", r.outcome)) for r in (first, second)]}))
