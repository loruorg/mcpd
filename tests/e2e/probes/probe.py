"""The canonical run, in Python. Prints its trace id on the last line."""
import os
from mcpd import Mcpd, SpanKind, FeedbackKind, LogLevel, MetricKind

mcpd = Mcpd(endpoint=os.environ["MCPD_ENDPOINT"], project=os.environ["MCPD_PROJECT"])

with mcpd.trace(
    "support_agent",
    kind=SpanKind.AGENT_RUN,
    input="where is my order 41?",
    session_id="sess-conformance",
    thread_id="thread-7",
    user_id="user-41",
    request_id="req-9001",
    release="2026.08.1",
) as run:
    with run.child("plan_answer", kind=SpanKind.MODEL_GENERATION) as gen:
        gen.set_model("gpt-4o", provider="openai")
        gen.set_usage(input_tokens=1180, output_tokens=96)

    with run.child("lookup_order", kind=SpanKind.TOOL_CALL):
        pass

    try:
        with run.child("estimate_delivery", kind=SpanKind.TOOL_CALL):
            raise RuntimeError("shipping service returned 503")
    except RuntimeError:
        pass

    # Recorded inside the run, so the log carries the trace and the metric
    # carries an exemplar back to it.
    mcpd.log(
        LogLevel.WARN,
        "estimate_delivery failed, answering from the cached estimate",
        {"step": "estimate_delivery"},
    )
    mcpd.metric(
        "conformance.run_steps",
        3,
        MetricKind.COUNTER,
        labels={"outcome": "degraded", "lang": "python"},
        unit="{step}",
    )

    trace_id = run.trace_id

mcpd.score("answer_relevance", 0.86, min=0, max=1, trace_id=trace_id, source="live")
# No source: the suite compares what each SDK defaults to.
mcpd.score("scale_check", 3, min=1, max=5, trace_id=trace_id)
mcpd.feedback(FeedbackKind.THUMBS, "up", trace_id=trace_id)
mcpd.shutdown()
print(trace_id)
