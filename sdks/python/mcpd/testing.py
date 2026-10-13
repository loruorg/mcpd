"""A client for your own unit tests (SDK contract 2.5).

    from mcpd.testing import TestClient

    def test_answer():
        mcpd = TestClient()
        answer_question(mcpd)
        call = mcpd.recorded_span("model_call")
        assert call.usage.output_tokens == 37

It is a real `Mcpd` whose sink keeps every batch in memory, so what you read
back went through sampling, payload capture, redaction and the queue exactly as
it would on the way to a daemon. Nothing leaves the process.
"""

from __future__ import annotations

import dataclasses
from typing import Any, Dict, List, Optional, Sequence, Union

from . import _generated as records
from ._generated import SpanKind
from .client import Mcpd
from .sinks import RecordingSink

__all__ = ["SpanTree", "TestClient"]


@dataclasses.dataclass
class SpanTree:
    """One finished span and the finished spans under it, by start time."""

    span: records.Span
    children: List["SpanTree"] = dataclasses.field(default_factory=list)


class TestClient(Mcpd):
    """An `Mcpd` that records into memory.

    Pass it wherever your code takes a client, then read back what it recorded.
    It takes every `Mcpd` option except `sink`. Every accessor flushes first,
    so you never call `flush()` yourself.
    """

    # Its name starts with "Test", and pytest must not try to collect it.
    __test__ = False

    def __init__(self, endpoint: Optional[str] = None, *, enabled: Optional[bool] = None,
                 sample: Optional[float] = None, **options: Any) -> None:
        if "sink" in options:
            raise TypeError("TestClient records into its own sink; pass no sink")
        #: The sink underneath, for a test that wants the raw batches.
        self.recorder = RecordingSink()
        # `enabled` and `sample` default to on whatever the environment says: a
        # CI job that turns mcpd off must not turn the test double off with it.
        super().__init__(
            endpoint,
            enabled=True if enabled is None else enabled,
            sample=1.0 if sample is None else sample,
            sink=self.recorder,
            **options,
        )

    def spans(self, name: Optional[str] = None,
              kind: Optional[Union[SpanKind, str]] = None) -> List[records.Span]:
        """The finished spans, one per span id, in the order each id first arrived."""
        self.flush()
        want = None if kind is None else SpanKind(kind)
        return [s for s in _finished(self.recorder.spans)
                if (name is None or s.name == name) and (want is None or s.kind == want)]

    def recorded_span(self, name: str) -> records.Span:
        """The one finished span called `name`.

        Raises `AssertionError` when there is none or more than one, because a
        failed assertion is what a test wants from it. Not `span()`, because
        the client already has a `span` that starts one.
        """
        found = self.spans(name)
        if len(found) != 1:
            seen = ", ".join(dict.fromkeys(s.name for s in self.spans())) or "none"
            raise AssertionError(f"expected one finished span named {name!r}, found "
                                 f"{len(found)}; spans recorded: {seen}")
        return found[0]

    def logs(self) -> List[records.LogRecord]:
        """Every log line, in arrival order."""
        self.flush()
        return list(self.recorder.logs)

    def metrics(self) -> List[records.MetricPoint]:
        """Every metric point, in arrival order."""
        self.flush()
        return list(self.recorder.metrics)

    def scores(self) -> List[records.Score]:
        """Every score, in arrival order."""
        self.flush()
        return list(self.recorder.scores)

    def recorded_feedback(self) -> List[records.Feedback]:
        """Every piece of feedback, in arrival order.

        Not `feedback()`, because the client already has a `feedback` that
        records one.
        """
        self.flush()
        return list(self.recorder.feedback)

    def trees(self) -> List[SpanTree]:
        """Every trace as a tree. A span whose parent was not recorded is a root."""
        nodes = {s.span_id: SpanTree(s) for s in self.spans()}
        roots: List[SpanTree] = []
        for node in nodes.values():
            parent = nodes.get(node.span.parent_span_id or "")
            if parent is not None and parent is not node:
                parent.children.append(node)
            else:
                roots.append(node)
        _order(roots)
        return roots

    def reset(self) -> None:
        """Forget everything recorded so far."""
        self.flush()
        self.recorder.clear()


def _finished(found: Sequence[records.Span]) -> List[records.Span]:
    """The last record per span id, in first-arrival order, ended spans only."""
    # Assigning to a key a dict already holds keeps its position, which is the
    # order the contract asks for.
    last: Dict[str, records.Span] = {}
    for s in found:
        last[s.span_id] = s
    return [s for s in last.values() if s.end_time is not None]


def _start(node: SpanTree) -> Any:
    t = node.span.start_time
    return getattr(t, "ns", None) or int(t.timestamp() * 1e9)


def _order(nodes: List[SpanTree]) -> None:
    nodes.sort(key=_start)
    for node in nodes:
        _order(node.children)
