"""A durable workflow in Python: the graph goes to the daemon as a definition,
the step bodies stay in this process for a Worker to serve.

The regions are read into the documentation at build time.
"""

import threading

from mcpd import Mcpd, RetryPolicy, StepFailure, Worker, when, workflow

mcpd = Mcpd(project="support")


def deliver(text: str, idempotency_key: str) -> bool:
    return bool(text) and bool(idempotency_key)


# #region workflow
support = workflow("support_ticket")


@support.step("classify")
def classify(ctx, input, prior):
    return {"kind": "question" if input["ticket"].startswith("Q") else "bug"}


@support.agent("draft", input={"kind": "${steps.classify.output.kind}"})
def draft(ctx, input, prior):
    # Your own model call, recorded under this step in the run's trace. What
    # the span spends is the step's usage: the daemon prices it from here.
    with ctx.span("model_call", kind="model_generation") as span:
        span.set_model("claude-sonnet-4", "anthropic")
        span.set_usage(input_tokens=812, output_tokens=37)
        text = f"an answer to a {input['kind']}"
    return text


support.wait_for("approval", event="approved", key="${input.ticket}",
                 timeout_ms=7 * 24 * 3_600_000,
                 when=when("steps.classify.output.kind", "eq", "bug"))


@support.step("send", input={"text": "${steps.draft.output}"}, effect="at_most_once",
              retry=RetryPolicy(max_attempts=3, backoff="exponential", initial_ms=500))
def send(ctx, input, prior):
    if not deliver(input["text"], ctx.idempotency_key):
        raise StepFailure("mail relay busy", class_="retryable", retry_after_ms=2_000)
    return {"sent": True}


support.output({"sent": "${steps.send.output.sent}"})
# #endregion workflow

# #region worker
# Serve step bodies for as long as this process lives. serve() blocks, so it
# runs on a thread of its own when the process does other work too.
worker = Worker(mcpd, concurrency=8).register(support)
threading.Thread(target=worker.serve, daemon=True).start()
# #endregion worker

# #region run
# Start a run from anywhere that holds an execute key, answer the wait from a
# webhook, then follow the journal to the end and read the run.
run = mcpd.runs.start(support, input={"ticket": "Q-42"})

# A repeat of this event id is a duplicate, not a second event.
mcpd.events.publish("approved", "Q-42", payload={"by": "ops"}, event_id="hook-1")

for event in mcpd.runs.watch(run.run_id):
    print(event.seq, event.kind)
detail = mcpd.runs.get(run.run_id)
print(detail.run.status, detail.run.output)
# #endregion run

worker.stop()
mcpd.shutdown()
