#!/usr/bin/env python3
"""A support agent, instrumented with mcpd.

The agent is fake: it answers one question, calls one tool, and the tool fails.
Everything around it is exactly what you would write for a real one.

Run it against a local daemon:

    mcpd demo --data ./mcpd.db          # in another terminal
    python3 quickstart.py

Then open http://localhost:4317 and read the run.
"""

import os
import random
import time

# #region init
from mcpd import FeedbackKind, Mcpd, SpanKind

# Everything here has an MCPD_* environment variable behind it, so the same code
# runs in development and in production with no edit. `Mcpd()` with no arguments
# talks to a local daemon.
mcpd = Mcpd(
    endpoint=os.environ.get("MCPD_ENDPOINT", "http://localhost:4317"),
    project="support",
    environment="development",
    service_name="support-agent",
    service_version="1.4.2",
)
# #endregion init


def lookup_order(order_id: str) -> dict:
    """The tool the agent calls. In this example it always times out."""
    raise TimeoutError("orders API did not answer in 2s")


def answer(question: str, user_id: str) -> "tuple[str, str]":
    """Answer one support question, recording every step of it."""

    # #region trace
    # The root span is the run. Everything opened inside it becomes a child,
    # because the active span lives in a `contextvars.ContextVar`.
    with mcpd.trace("support_agent", session_id="sess-8121", user_id=user_id,
                    tags=["support", "orders"]) as run:
        run.set_input({"question": question})

        # A model call. `record_response` reads the model, the token counts
        # mcpd prices the run from, the id and the finish reason off the reply.
        with mcpd.span("plan", kind=SpanKind.MODEL_GENERATION) as generation:
            generation.set_input([
                {"role": "system", "content": "You are a support agent."},
                {"role": "user", "content": question},
            ])
            time.sleep(0.12)  # stand-in for the provider round trip
            generation.set_output("I should look the order up first.")
            generation.record_response({  # the shape OpenAI answers in
                "id": "chatcmpl-1", "model": "gpt-4o-mini",
                "choices": [{"finish_reason": "stop"}],
                "usage": {"prompt_tokens": 812, "completion_tokens": 96,
                          "prompt_tokens_details": {"cached_tokens": 128}},
            })

        # A tool call that fails. The `with` block records the exception on the
        # span, marks it as an error, and lets the exception through unchanged.
        order = None
        try:
            with mcpd.span("lookup_order", kind=SpanKind.TOOL_CALL) as tool:
                tool.set_entity("tool", entity_id="orders.lookup", name="Order lookup")
                tool.set_input({"order_id": "A-7741"})
                order = lookup_order("A-7741")
                tool.set_output(order)
        except TimeoutError as exc:
            # The span already carries the error. This is the agent deciding
            # what to do about it, which is a different question.
            mcpd.log("warn", "order lookup failed, answering from context", {"error": str(exc)})
            mcpd.metric("support.tool_failures", 1, "counter", labels={"tool": "orders.lookup"})

        reply = ("I could not reach the order system just now. Your order A-7741 "
                 "shipped on Tuesday and tracking should update within the hour.")
        run.set_output(reply)
        run.set_metadata({"used_fallback": order is None})
        return reply, run.trace_id
    # #endregion trace


def main() -> None:
    reply, trace_id = answer("where is my order?", user_id="u-3312")
    print(reply)

    # #region score
    # A machine evaluation. `min` and `max` are required: a bare number with no
    # declared scale cannot be compared to anything.
    mcpd.score(
        "answer_relevance",
        value=round(random.uniform(0.6, 0.95), 2),
        min=0.0,
        max=1.0,
        trace_id=trace_id,
        reason="named the order and gave a delivery window, without the live lookup",
    )

    # A human evaluation. Kept separate from a score, because a person's
    # judgement and a model's must never be averaged together.
    mcpd.feedback(
        FeedbackKind.THUMBS,
        "up",
        trace_id=trace_id,
        comment="fine given the outage",
        author_kind="end_user",
    )
    # #endregion score

    # #region query
    # Nothing has been sent yet: recording appends to an in-memory queue and a
    # background worker does the I/O. Flush before you read your own writes.
    mcpd.flush()

    detail = mcpd.get_trace(trace_id, include=["logs", "scores", "feedback"])
    print(f"\n{detail.trace.name}  {detail.trace.duration_ns / 1e6:.0f} ms  "
          f"{detail.trace.span_count} spans  {detail.trace.error_count} errors")
    for span in detail.spans:
        cost = f"  ${span.cost.total_cost:.6f}" if span.cost else ""
        print(f"  {span.kind.value:<18} {span.name:<14} {span.status.value}{cost}")

    summary = mcpd.analytics_summary("-1h", "-0s")
    print(f"\nlast hour  traces={summary.trace_count}  errors={summary.error_count}  "
          f"p95={summary.latency.p95:.0f}ms  cost=${summary.total_cost:.4f}")
    # #endregion query

    # Explicit, idempotent, and the reason a short-lived process does not lose
    # its last batch.
    mcpd.shutdown()


if __name__ == "__main__":
    main()
