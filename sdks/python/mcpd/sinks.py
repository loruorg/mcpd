"""Where a drained batch goes (contract section 2.2).

The exporter turns the queue into an `IngestBatch` and hands it to one sink
together with the batch's `ingest_key`. The HTTP transport is the default sink
and the only one that talks to a daemon. The two here never touch a socket:
`RecordingSink` keeps what it saw so a test can assert on it, and
`ConsoleSink` prints one JSON line per signal for a local dump.

A sink runs on the exporter's thread inside the same guard as the transport.
One that raises is a failed batch, counted and reported, never a failure the
caller sees (B1).
"""

from __future__ import annotations

import json
import sys
import threading
from typing import Any, Callable, List, Optional, Protocol

from . import _generated as records
from .transport import encode

__all__ = ["Sink", "RecordingSink", "ConsoleSink", "TransportSink"]

#: The five buckets of an `IngestBatch`, paired with the `signal` name each
#: record carries on a console line.
_BUCKETS = (("spans", "span"), ("logs", "log"), ("metrics", "metric"), ("scores", "score"),
            ("feedback", "feedback"))


class Sink(Protocol):
    """One call per drained batch. Return value is ignored unless it is an `IngestResult`."""

    def export(self, batch: records.IngestBatch, ingest_key: str) -> Optional[records.IngestResult]:
        ...


class RecordingSink:
    """Keeps every batch in memory: what a test reads to check what was recorded.

    The lists hold the generated records in arrival order, so an assertion can
    read `sink.spans[0].usage` without decoding anything.
    """

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self.spans: List[records.Span] = []
        self.logs: List[records.LogRecord] = []
        self.metrics: List[records.MetricPoint] = []
        self.scores: List[records.Score] = []
        self.feedback: List[records.Feedback] = []
        self.batches = 0

    def export(self, batch: records.IngestBatch, ingest_key: str) -> None:
        with self._lock:
            self.batches += 1
            for field, _ in _BUCKETS:
                getattr(self, field).extend(getattr(batch, field) or [])

    def clear(self) -> None:
        """Forget everything seen so far, including the batch count."""
        with self._lock:
            for field, _ in _BUCKETS:
                getattr(self, field).clear()
            self.batches = 0

    def __repr__(self) -> str:
        return (f"RecordingSink(batches={self.batches}, spans={len(self.spans)}, logs={len(self.logs)}, "
                f"metrics={len(self.metrics)}, scores={len(self.scores)}, feedback={len(self.feedback)})")


class ConsoleSink:
    """One JSON line per signal, shaped `{"signal": "span", ...record}`, to stderr.

    The record is spelled as the wire spells it, so a line here is what the
    daemon would have received. Pass `write` to send the lines elsewhere.
    """

    def __init__(self, write: Optional[Callable[[str], Any]] = None) -> None:
        self._write = write if write is not None else sys.stderr.write

    def export(self, batch: records.IngestBatch, ingest_key: str) -> None:
        for field, signal in _BUCKETS:
            for record in getattr(batch, field) or []:
                line = {"signal": signal}
                line.update(encode(record))
                self._write(json.dumps(line, separators=(",", ":"), default=str) + "\n")


class TransportSink:
    """The default: hand the batch to the HTTP transport, key and all."""

    def __init__(self, transport: Any) -> None:
        self.transport = transport

    def export(self, batch: records.IngestBatch, ingest_key: str) -> Optional[records.IngestResult]:
        batch.ingest_key = ingest_key
        return self.transport.ingest(batch)
