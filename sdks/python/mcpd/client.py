"""The client: configuration, the background worker, and every call you make.

Shape of the thing: recording a signal appends to a bounded queue and returns
(B2, B3). A daemon thread drains that queue on an interval, or when someone
flushes, and posts batches over HTTP. Nothing on the recording path can raise
(B1), nothing on it can block, and `shutdown` is what a short-lived process
calls so its last batch is not lost (B5).

Reads are the deliberate exception. `list_traces` and its neighbours are
synchronous and they raise, because a query that silently returned nothing would
be a worse bug than one that failed.
"""

from __future__ import annotations

import asyncio
import atexit
import dataclasses
import logging
import math
import os
import threading
import weakref
from enum import Enum
from typing import Any, Callable, Dict, List, Mapping, Optional, Sequence, Union

from . import _generated as records
from ._generated import (
    AnalyticsSummary,
    CostBreakdown,
    DeleteResult,
    FeedbackKind,
    FeedbackListResponse,
    HealthStatus,
    LogLevel,
    LogListResponse,
    MetricKind,
    MetricSeriesResponse,
    ScoreListResponse,
    ScoreSource,
    SpanKind,
    SpanListResponse,
    SpanStatus,
    TraceDetail,
    TraceListResponse,
)
from .context import (
    EPOCH,
    TraceContext,
    current_span,
    is_valid_span_id,
    is_valid_trace_id,
    nanotime,
    new_span_id,
    new_trace_id,
    now_ns,
    parse_traceparent,
)
from .logs import encode_fields, meets_level, parse_log_level
from .queue import BoundedQueue, Throttle
from .runs import DefinitionsApi, EventsApi, RunsApi, StepsApi
from .sinks import Sink, TransportSink
from .span import Span
from .transport import Transport, TransportError, decode, encode_params

__all__ = ["Mcpd", "Stats"]

log = logging.getLogger("mcpd")

DEFAULT_ENDPOINT = "http://localhost:4317"

#: Correlation fields a caller may set per span rather than per client.
_CORRELATION_FIELDS = (
    "project", "environment", "service_name", "service_version", "session_id", "thread_id",
    "user_id", "resource_id", "run_id", "request_id", "experiment_id", "release",
)

#: Span fields a caller may pass as keyword arguments when starting a span.
_SPAN_FIELDS = (
    "input", "output", "attributes", "metadata", "tags", "model", "provider", "usage",
    "entity_kind", "entity_id", "entity_name", "links", "status",
)

#: Start options that shape the span itself rather than fill a field on it.
_START_OPTIONS = ("external_parent_span_id", "capture_payloads")


@dataclasses.dataclass(frozen=True)
class Stats:
    """What the client has done since it was built (contract section 2.3)."""

    #: Signals waiting to be sent.
    queued: int
    #: Signals the bounded queue evicted (B3).
    dropped: int
    #: How many signals the queue can hold.
    capacity: int
    #: Signals handed to the sink.
    exported: int
    #: Batches the sink refused.
    failed: int
    #: Spans `record` refused.
    rejected: int
    #: Values `redact` raised on or could not use, dropped rather than sent.
    redaction_failures: int = 0


#: Queue signal name -> the payload fields `redact` sees (section 2.1.1).
_REDACTED = {"span": ("input", "output", "attributes", "metadata"), "log": ("fields",)}

#: Queue signal name -> the field of `IngestBatch` it belongs in.
_BUCKET = {"span": "spans", "log": "logs", "metric": "metrics", "score": "scores",
           "feedback": "feedback"}


def _apply_fields(span: Span, fields: Mapping[str, Any]) -> None:
    """Apply the keyword fields a caller passed when starting a span.

    Written out rather than dispatched by name: the pairs that belong together
    (a model and its provider, an entity's kind and its id) have to be applied
    in one call, and a clever loop got that wrong.
    """
    if "input" in fields:
        span.set_input(fields["input"])
    if "output" in fields:
        span.set_output(fields["output"])
    if "attributes" in fields:
        span.set_attributes(fields["attributes"] or {})
    if "metadata" in fields:
        span.set_metadata(fields["metadata"] or {})
    if "tags" in fields:
        span.set_tags(fields["tags"] or [])
    if "model" in fields or "provider" in fields:
        span.set_model(fields.get("model"), fields.get("provider"))
    if "usage" in fields:
        usage = fields["usage"]
        if isinstance(usage, Mapping):
            span.set_usage(**usage)
        else:
            span.set_usage_record(usage)
    if "entity_kind" in fields or "entity_id" in fields or "entity_name" in fields:
        span.set_entity(fields.get("entity_kind"), fields.get("entity_id"),
                        fields.get("entity_name"))
    for link in fields.get("links") or []:
        if isinstance(link, Mapping):
            span.add_link(**link)
        else:
            span.add_link(link.trace_id, link.span_id, link.kind, link.attributes)
    if fields.get("status") is not None:
        span.set_status(fields["status"])


def _env(name: str) -> Optional[str]:
    value = os.environ.get(name)
    return value if value not in (None, "") else None


def _env_float(name: str, default: float) -> float:
    raw = _env(name)
    if raw is None:
        return default
    try:
        return float(raw)
    except ValueError:
        log.warning("%s=%r is not a number; using %s", name, raw, default)
        return default


def _env_int(name: str, default: int) -> int:
    """A whole positive number, or the default.

    The four tuning variables carry their unit in their name, so a value is a
    plain integer and never a duration string. Zero and below are refused along
    with anything unparseable: a batch of zero signals or a queue of zero
    entries would stop the exporter, and a typo in a deployment variable must
    not.
    """
    raw = _env(name)
    if raw is None:
        return default
    try:
        value = int(raw.strip())
    except ValueError:
        log.warning("%s=%r is not a whole number; using %s", name, raw, default)
        return default
    if value <= 0:
        log.warning("%s=%r must be positive; using %s", name, raw, default)
        return default
    return value


def _env_bool(name: str, default: bool) -> bool:
    raw = _env(name)
    if raw is None:
        return default
    return raw.strip().lower() in ("1", "true", "yes", "on")


class Mcpd:
    """A handle on one mcpd endpoint.

    Constructed with no arguments it talks to a local mcpd, which is the whole
    first-run story. Every field is settable in code and by environment
    variable, and code wins.
    """

    def __init__(
        self,
        endpoint: Optional[str] = None,
        *,
        api_key: Optional[str] = None,
        project: Optional[str] = None,
        environment: Optional[str] = None,
        service_name: Optional[str] = None,
        service_version: Optional[str] = None,
        sample: Optional[float] = None,
        batch_size: Optional[int] = None,
        flush_interval: Optional[float] = None,
        max_queue: Optional[int] = None,
        timeout: Optional[float] = None,
        enabled: Optional[bool] = None,
        capture_payloads: Optional[bool] = None,
        log_level: Optional[Union[LogLevel, str]] = None,
        transport: Optional[Transport] = None,
        sink: Optional[Sink] = None,
        redact: Optional[Callable[[Any], Any]] = None,
    ) -> None:
        self.endpoint = (endpoint or _env("MCPD_ENDPOINT") or DEFAULT_ENDPOINT).rstrip("/")
        self.api_key = api_key or _env("MCPD_API_KEY")
        self.project = project or _env("MCPD_PROJECT") or "default"
        self.environment = environment or _env("MCPD_ENVIRONMENT")
        self.service_name = service_name or _env("MCPD_SERVICE_NAME")
        self.service_version = service_version or _env("MCPD_SERVICE_VERSION")
        self.sample = _env_float("MCPD_SAMPLE", 1.0) if sample is None else float(sample)
        # The environment carries the two durations in whole milliseconds, so
        # a value copied from a service in another language keeps working; in
        # code they stay seconds, which is what `time.sleep` and `Event.wait`
        # take.
        if batch_size is None:
            batch_size = _env_int("MCPD_BATCH_SIZE", 128)
        if flush_interval is None:
            flush_interval = _env_int("MCPD_FLUSH_INTERVAL_MS", 2000) / 1000.0
        if max_queue is None:
            max_queue = _env_int("MCPD_MAX_QUEUE", 8192)
        if timeout is None:
            timeout = _env_int("MCPD_TIMEOUT_MS", 10_000) / 1000.0
        self.batch_size = max(1, int(batch_size))
        self.flush_interval = max(0.01, float(flush_interval))
        self.max_queue = max(1, int(max_queue))
        self.timeout = float(timeout)
        self.enabled = _env_bool("MCPD_ENABLED", True) if enabled is None else bool(enabled)
        #: Whether `input` and `output` may leave the process (section 2.1). A
        #: span can say otherwise for itself with `capture_payloads=`.
        self.capture_payloads = (_env_bool("MCPD_CAPTURE_PAYLOADS", True)
                                 if capture_payloads is None else bool(capture_payloads))
        #: The lowest level `log` records (section 4.2). A line below it is
        #: dropped at the call, before the record is built. An unreadable name
        #: is ignored rather than refused: a typo in a deployment variable must
        #: not silence a service's logs.
        self.log_level = (parse_log_level(log_level if isinstance(log_level, str) else None)
                          or (log_level if isinstance(log_level, LogLevel) else None)
                          or parse_log_level(_env("MCPD_LOG_LEVEL"))
                          or LogLevel.DEBUG)

        self._queue = BoundedQueue(self.max_queue)
        self._transport = transport if transport is not None else Transport(
            self.endpoint, self.api_key, self.timeout, user_agent="mcpd-python/0.1.0"
        )
        #: Where a drained batch goes. The transport, unless the caller chose
        #: a recorder or a console dump (section 2.2).
        self._sink: Sink = sink if sink is not None else TransportSink(self._transport)
        #: Applied to every span's input, output, attributes and metadata, and
        #: to every log line's fields, before the record is queued (section
        #: 2.1.1). Code only: no environment variable can carry a function.
        self._redact: Optional[Callable[[Any], Any]] = redact if callable(redact) else None
        self._redaction_failures = 0
        self._overflow = Throttle()
        self._failure = Throttle()
        self._warning = Throttle()
        self._failures = 0
        self._exported = 0
        self._failed_batches = 0
        self._rejected = 0

        #: The run surface: definitions, runs, events and the worker protocol.
        self.definitions = DefinitionsApi(self._transport, self.project)
        self.runs = RunsApi(self._transport, self.project)
        self.events = EventsApi(self._transport, self.project)
        self.steps = StepsApi(self._transport, self.project)

        self._lock = threading.Lock()
        self._cv = threading.Condition(self._lock)
        self._wake = threading.Event()
        self._stopping = threading.Event()
        self._worker: Optional[threading.Thread] = None
        self._sending = False
        self._closed = False
        # Set when a shutdown's deadline passed: no further batch starts.
        self._abandoned = False

        if self.enabled:
            # A process that exits without flushing loses its last batch, and
            # short-lived processes are exactly where that hurts.
            atexit.register(_shutdown_at_exit, weakref.ref(self))

    # ------------------------------------------------------------------- tracing

    def trace(self, name: str, kind: Union[SpanKind, str] = SpanKind.AGENT_RUN,
              **fields: Any) -> Span:
        """Start a root span: one agent run, one workflow, one job.

        Pass `traceparent=` to continue a trace that started in another service
        (B7); pass `parent=` to nest under a span you are holding.
        """
        try:
            return self._start(name=name, kind=kind, root=True, **fields)
        except Exception as exc:  # noqa: BLE001 - B1
            self._instrumentation_failed("Mcpd.trace", exc)
            return self._dropped_span(name, kind)

    def span(self, name: str, kind: Union[SpanKind, str] = SpanKind.GENERIC,
             **fields: Any) -> Span:
        """Start a span under whatever is currently active, or a root if nothing is.

        This is the scoped form: `with mcpd.span("search_docs") as span:` ends
        the span on the way out and records an error if the block raised. The
        same object works with `async with`.
        """
        try:
            return self._start(name=name, kind=kind, **fields)
        except Exception as exc:  # noqa: BLE001 - B1
            self._instrumentation_failed("Mcpd.span", exc)
            return self._dropped_span(name, kind)

    def current_span(self) -> Optional[Span]:
        """The span the calling code is inside, if any."""
        return current_span.get()

    def traceparent(self) -> Optional[str]:
        """The active span as a W3C `traceparent`, to put on an outbound request."""
        span = current_span.get()
        return span.traceparent if span is not None else None

    def record(self, span: records.Span) -> bool:
        """Queue a span that already ended, ids, parent and times as the caller has them.

        For an adapter that holds another tracing system's spans, a batch
        import or a replay. The record is never re-parented, re-timed or
        re-sampled. Correlation fields it leaves empty are filled from this
        client's; fields it sets are kept. A malformed record (an id that is
        not lowercase hex of the right length, an all-zero id, an empty name,
        no start time) is dropped and counted in `stats().rejected`, never
        raised. Returns whether the span was queued.
        """
        try:
            if not self._recording():
                return False
            problem = _invalid_record(span)
            if problem is not None:
                self._rejected += 1
                self._instrumentation_warning(f"a recorded span was refused: {problem}")
                return False
            captured = self.capture_payloads
            queued = dataclasses.replace(
                span,
                correlation=self._fill_correlation(span.correlation),
                input=span.input if captured else None,
                output=span.output if captured else None,
                received_at=span.received_at if span.received_at is not None else EPOCH,
            )
            self._put("span", queued, key=(queued.trace_id, queued.span_id))
            return True
        except Exception as exc:  # noqa: BLE001 - B1
            self._instrumentation_failed("Mcpd.record", exc)
            return False

    def stats(self) -> Stats:
        """Queue depth, drops, exports, failed batches and refused records."""
        return Stats(
            queued=len(self._queue),
            dropped=self._queue.dropped,
            capacity=self._queue.max_size,
            exported=self._exported,
            failed=self._failed_batches,
            rejected=self._rejected,
            redaction_failures=self._redaction_failures,
        )

    # --------------------------------------------------------- the other signals

    def log(self, level: Union[LogLevel, str], message: str,
            fields: Optional[Mapping[str, Any]] = None, *, logger: Optional[str] = None,
            trace_id: Optional[str] = None, span_id: Optional[str] = None,
            **correlation: Any) -> None:
        """Record a log line, correlated to the active span unless told otherwise."""
        try:
            if not self._recording():
                return
            level = LogLevel(level) if not isinstance(level, LogLevel) else level
            # Below the floor costs one comparison: no record, no queue, no
            # correlation lookup.
            if not meets_level(level, self.log_level):
                return
            span = current_span.get()
            record = records.LogRecord(
                log_id=new_trace_id(),
                timestamp=nanotime(now_ns()),
                level=level,
                message=str(message),
                correlation=self._correlation(correlation, span),
                received_at=EPOCH,
                trace_id=trace_id or (span.trace_id if span else None),
                span_id=span_id or (span.span_id if span else None),
                fields=encode_fields(dict(fields)) if fields else None,
                logger=logger,
            )
            self._put("log", record)
        except Exception as exc:  # noqa: BLE001 - B1
            self._instrumentation_failed("Mcpd.log", exc)

    def metric(self, name: str, value: float, kind: Union[MetricKind, str] = MetricKind.COUNTER,
               labels: Optional[Mapping[str, Any]] = None, unit: Optional[str] = None,
               **correlation: Any) -> None:
        """Record one numeric observation, with the active span as its exemplar.

        The exemplar is what turns a spike on a chart into a run you can open.
        """
        try:
            if not self._recording():
                return
            value = float(value)
            if not math.isfinite(value):
                # The point would go out as a bare NaN, which is not JSON, and
                # the daemon would refuse the whole batch it travelled in. The
                # point is what cannot be sent, so it alone is dropped
                # (contract section 4.1).
                self._instrumentation_warning(
                    f"metric {name!r} has the value {value}, which JSON cannot carry; "
                    "the point was dropped"
                )
                return
            span = current_span.get()
            record = records.MetricPoint(
                metric_id=new_trace_id(),
                name=name,
                kind=MetricKind(kind) if not isinstance(kind, MetricKind) else kind,
                value=value,
                timestamp=nanotime(now_ns()),
                correlation=self._correlation(correlation, span),
                received_at=EPOCH,
                unit=unit,
                labels=encode_fields(dict(labels)) if labels else None,
                trace_id=span.trace_id if span else None,
                span_id=span.span_id if span else None,
            )
            self._put("metric", record)
        except Exception as exc:  # noqa: BLE001 - B1
            self._instrumentation_failed("Mcpd.metric", exc)

    def score(self, scorer_id: str, value: float, *, min: float, max: float,
              trace_id: Optional[str] = None, span_id: Optional[str] = None,
              reason: Optional[str] = None, label: Optional[str] = None,
              source: Union[ScoreSource, str] = ScoreSource.LIVE,
              scorer_version: Optional[str] = None,
              metadata: Optional[Mapping[str, Any]] = None, **correlation: Any) -> None:
        """Attach a machine evaluation to a trace or a span.

        `min` and `max` are keyword-only and have no defaults on purpose: a bare
        number with no declared scale cannot be compared to anything, so the
        scale is enforced here, at the call site, rather than guessed later.
        """
        try:
            if not self._recording():
                return
            if not all(math.isfinite(float(n)) for n in (value, min, max)):
                self._instrumentation_warning(
                    f"score {scorer_id!r} has value {value} on [{min}, {max}], which JSON "
                    "cannot carry; the score was dropped"
                )
                return
            span = current_span.get()
            trace_id = trace_id or (span.trace_id if span else None)
            if trace_id is None and span_id is None:
                self._instrumentation_warning(
                    f"score {scorer_id!r} was dropped: it names no trace_id or span_id, "
                    "and no span is active"
                )
                return
            record = records.Score(
                score_id=new_trace_id(),
                scorer_id=scorer_id,
                source=ScoreSource(source) if not isinstance(source, ScoreSource) else source,
                value=float(value),
                timestamp=nanotime(now_ns()),
                correlation=self._correlation(correlation, span),
                trace_id=trace_id,
                span_id=span_id,
                scorer_version=scorer_version,
                min=float(min),
                max=float(max),
                label=label,
                reason=reason,
                metadata=dict(metadata) if metadata else None,
            )
            self._put("score", record)
        except Exception as exc:  # noqa: BLE001 - B1
            self._instrumentation_failed("Mcpd.score", exc)

    def feedback(self, kind: Union[FeedbackKind, str], value: Any, *,
                 trace_id: Optional[str] = None, span_id: Optional[str] = None,
                 comment: Optional[str] = None, author_id: Optional[str] = None,
                 author_kind: Optional[str] = None,
                 metadata: Optional[Mapping[str, Any]] = None, **correlation: Any) -> None:
        """Attach a human evaluation to a trace or a span.

        Kept separate from `score` because the two answer different questions
        and must never be averaged together.
        """
        try:
            if not self._recording():
                return
            span = current_span.get()
            trace_id = trace_id or (span.trace_id if span else None)
            if trace_id is None and span_id is None:
                self._instrumentation_warning(
                    "feedback was dropped: it names no trace_id or span_id, and no span is active"
                )
                return
            record = records.Feedback(
                feedback_id=new_trace_id(),
                kind=FeedbackKind(kind) if not isinstance(kind, FeedbackKind) else kind,
                value=value,
                timestamp=nanotime(now_ns()),
                correlation=self._correlation(correlation, span),
                trace_id=trace_id,
                span_id=span_id,
                comment=comment,
                author_id=author_id,
                author_kind=author_kind,
                metadata=dict(metadata) if metadata else None,
            )
            self._put("feedback", record)
        except Exception as exc:  # noqa: BLE001 - B1
            self._instrumentation_failed("Mcpd.feedback", exc)

    # ----------------------------------------------------------- span plumbing

    def _start(self, *, name: str, kind: Union[SpanKind, str], parent: Optional[Span] = None,
               traceparent: Optional[str] = None, root: bool = False, is_event: bool = False,
               **fields: Any) -> Span:
        """Build a span, decide whether it is recorded, and export its start."""
        upstream: Optional[TraceContext] = parse_traceparent(traceparent)
        if parent is None and not root:
            parent = current_span.get()
        if upstream is not None:
            parent = None  # a remote parent wins: this span continues that trace

        span_fields = {k: fields.pop(k) for k in _SPAN_FIELDS if k in fields}
        corr_fields = {k: fields.pop(k) for k in _CORRELATION_FIELDS if k in fields}
        options = {k: fields.pop(k) for k in _START_OPTIONS if k in fields}
        # Anything left is not a field we know. Folding it into metadata means a
        # typo costs a misplaced key, never an exception in the caller's process.
        if fields:
            span_fields["metadata"] = {**(span_fields.get("metadata") or {}), **fields}

        if upstream is not None:
            trace_id, parent_span_id, sampled = upstream.trace_id, upstream.span_id, upstream.sampled
        elif parent is not None:
            # B4: the decision was made at the root; a child never re-decides.
            trace_id, parent_span_id, sampled = parent.trace_id, parent.span_id, parent.sampled
        else:
            trace_id = new_trace_id()
            parent_span_id = None
            sampled = self._decide(trace_id)

        correlation = self._correlation(corr_fields, parent)
        capture = options.get("capture_payloads")
        external = options.get("external_parent_span_id")
        span = Span(
            self,
            trace_id=trace_id,
            span_id=new_span_id(),
            name=str(name),
            kind=SpanKind(kind) if not isinstance(kind, SpanKind) else kind,
            correlation=correlation,
            parent_span_id=parent_span_id,
            external_parent_span_id=str(external) if external is not None else None,
            is_root=parent_span_id is None,
            is_event=is_event,
            sampled=sampled,
            recording=self.enabled and sampled,
            capture=self.capture_payloads if capture is None else bool(capture),
        )
        # A step context installs a usage sink on the span it opens; every span
        # nested under it inherits the sink, so the step's usage is the sum of
        # what its spans spent (section 7.3).
        if parent is not None:
            span._usage_sink = parent._usage_sink
        _apply_fields(span, span_fields)

        # B8, and the whole of an event's life: the span goes on the wire before
        # it finishes, because a run that crashes mid-flight is exactly the run
        # you need to see. Ending it queues the same key again and replaces this
        # record if the worker has not sent it yet.
        self._enqueue_span(span)
        return span

    def _dropped_span(self, name: str, kind: Union[SpanKind, str],
                      parent: Optional[Span] = None) -> Span:
        """A valid, inert span, returned when starting a real one failed (B1)."""
        try:
            span_kind = SpanKind(kind) if not isinstance(kind, SpanKind) else kind
        except ValueError:
            span_kind = SpanKind.GENERIC
        return Span(
            self,
            trace_id=parent.trace_id if parent is not None else new_trace_id(),
            span_id=new_span_id(),
            name=str(name),
            kind=span_kind,
            correlation=records.Correlation(project=self.project),
            parent_span_id=parent.span_id if parent is not None else None,
            is_root=parent is None,
            sampled=False,
            recording=False,
        )

    def _decide(self, trace_id: str) -> bool:
        """Head sampling, decided once per trace (B4).

        The trace id's low 64 bits are compared against the ratio, exactly as
        the server's deterministic sampler does it, so a client and a fleet of
        collectors keep and drop the same traces without coordinating.
        """
        if self.sample >= 1.0:
            return True
        if self.sample <= 0.0:
            return False
        try:
            value = int(trace_id[-16:], 16)
        except ValueError:
            return True
        return value < int(self.sample * float(1 << 64))

    def _correlation(self, overrides: Mapping[str, Any],
                     parent: Optional[Span] = None) -> records.Correlation:
        """Client defaults, then the parent's context, then this call's overrides."""
        base: Dict[str, Any] = {
            "project": self.project,
            "environment": self.environment,
            "service_name": self.service_name,
            "service_version": self.service_version,
        }
        if parent is not None:
            for field in _CORRELATION_FIELDS:
                value = getattr(parent._correlation, field, None)
                if value is not None:
                    base[field] = value
        for key, value in overrides.items():
            if key in _CORRELATION_FIELDS and value is not None:
                base[key] = value
        return records.Correlation(**base)

    def _fill_correlation(self, given: Optional[records.Correlation]) -> records.Correlation:
        """A recorded span's correlation, with the client's underneath what it set."""
        filled = self._correlation({})
        if given is None:
            return filled
        for field in _CORRELATION_FIELDS:
            value = getattr(given, field, None)
            if value is not None:
                setattr(filled, field, value)
        return filled

    def _enqueue_span(self, span: Span) -> None:
        """Queue a span's current state, replacing an unsent earlier state."""
        if not span._recording or not self._recording():
            return
        self._put("span", span.to_record(), key=(span.trace_id, span.span_id))

    # -------------------------------------------------------------- the worker

    def _recording(self) -> bool:
        return self.enabled and not self._closed

    def _put(self, signal: str, record: Any, key: Any = None) -> None:
        if not self._recording():
            return
        if self._redact is not None:
            record = self._redacted(signal, record)
        if not self._queue.put(signal, _encodable(signal, record), key):
            suppressed = self._overflow.check()
            if suppressed is not None:
                # B3: say it once per interval, with the running total, so a
                # network outage cannot also flood the application's logs.
                log.warning(
                    "mcpd queue is full at %d signals; dropped the oldest (%d dropped in total, "
                    "%d further warnings suppressed)",
                    self._queue.max_size, self._queue.dropped, suppressed,
                )
        self._ensure_worker()

    def _redacted(self, signal: str, record: Any) -> Any:
        """The record with `redact` applied to its payload fields (section 2.1.1).

        Works on a copy, so the caller's span keeps what it was given, and
        fails closed field by field: a redactor that raises, or hands back a
        non-mapping where a mapping belongs, costs that field and is counted.
        Sending the original instead would be the worst outcome, because a
        broken redactor is exactly when the original is most likely to leak.
        """
        names = _REDACTED.get(signal)
        if not names:
            return record
        changes: Dict[str, Any] = {}
        for name in names:
            value = getattr(record, name)
            if value is None:
                continue
            try:
                kept = self._redact(value)  # type: ignore[misc]
                if kept is not None and name not in ("input", "output") \
                        and not isinstance(kept, Mapping):
                    raise TypeError(f"redact returned {type(kept).__name__} for {name}, "
                                    "where a mapping belongs")
            except Exception as exc:  # noqa: BLE001 - B1, and fail closed
                kept = None
                self._redaction_failures += 1
                self._instrumentation_failed("redact", exc)
            if kept is not None and name == "fields":
                # Log fields were made JSON-safe before this; what the
                # redactor returned has not been.
                kept = encode_fields(dict(kept))
            elif kept is not None and name in ("attributes", "metadata"):
                kept = dict(kept)
            changes[name] = kept
        return dataclasses.replace(record, **changes) if changes else record

    def _ensure_worker(self) -> None:
        """Start the exporter thread on first use, not on construction.

        A client that is built and never used costs nothing, which matters in
        tests and in processes that fork before they trace.
        """
        if self._worker is not None and self._worker.is_alive():
            return
        with self._lock:
            if self._closed or (self._worker is not None and self._worker.is_alive()):
                return
            self._worker = threading.Thread(target=self._run, name="mcpd-exporter", daemon=True)
            self._worker.start()

    def _run(self) -> None:
        while not self._stopping.is_set():
            self._wake.wait(self.flush_interval)
            self._wake.clear()
            self._drain_guarded()
        self._drain_guarded()

    def _drain_guarded(self) -> None:
        """`_drain`, wrapped so nothing can kill the exporter thread.

        A worker that died would stop every export for the life of the process
        and say nothing, which is the quiet failure B1 exists to prevent.
        """
        try:
            self._drain()
        except Exception as exc:  # noqa: BLE001
            self._instrumentation_failed("Mcpd worker", exc)

    def _drain(self) -> None:
        """Send everything queued, one batch at a time."""
        while True:
            with self._cv:
                # A shutdown that ran out of time gave up what is queued: the
                # batch in flight finishes, and no other starts.
                if self._abandoned:
                    self._cv.notify_all()
                    return
                items = self._queue.drain(self.batch_size)
                self._sending = bool(items)
                if not items:
                    self._cv.notify_all()
                    return
            try:
                self._send(items)
            finally:
                with self._cv:
                    self._sending = False
                    self._cv.notify_all()

    def _send(self, items: Sequence) -> None:
        buckets: Dict[str, List[Any]] = {"spans": [], "logs": [], "metrics": [], "scores": [],
                                         "feedback": []}
        for signal, record in items:
            buckets[_BUCKET[signal]].append(record)
        batch = records.IngestBatch(
            spans=buckets["spans"] or None,
            logs=buckets["logs"] or None,
            metrics=buckets["metrics"] or None,
            scores=buckets["scores"] or None,
            feedback=buckets["feedback"] or None,
            # Stable per batch, so a retry after a timeout cannot double-count.
            ingest_key=new_trace_id(),
        )
        try:
            result = self._sink.export(batch, batch.ingest_key)
        except TransportError as exc:
            # B1: the batch is lost, the process is not. Retries already
            # happened inside the transport; holding the data would grow the
            # queue we just bounded.
            self._export_failed(len(items), exc)
            return
        except Exception as exc:  # noqa: BLE001 - a codec bug, or a sink, must not escape either
            self._export_failed(len(items), exc)
            return
        self._exported += len(items)
        if not isinstance(result, records.IngestResult):
            return
        if result.rejected and result.errors:
            first = result.errors[0]
            # `signal` decodes to a raw string when the server knows a kind this
            # build does not, so read the value defensively.
            signal = getattr(first.signal, "value", first.signal)
            self._instrumentation_warning(
                f"mcpd rejected {result.rejected} of {len(items)} signals; "
                f"first was {signal}[{first.index}]: {first.message}"
            )

    def _export_failed(self, count: int, exc: BaseException) -> None:
        self._failures += 1
        self._failed_batches += 1
        suppressed = self._failure.check()
        if suppressed is not None:
            log.warning("mcpd export of %d signals failed: %s (%d failures so far, %d warnings "
                        "suppressed)", count, exc, self._failures, suppressed)

    def _instrumentation_failed(self, where: str, exc: BaseException) -> None:
        """B1: an instrumentation bug is logged, at most once per interval, never raised."""
        self._failures += 1
        suppressed = self._failure.check()
        if suppressed is not None:
            log.warning("mcpd instrumentation error in %s: %r (%d suppressed)", where, exc,
                        suppressed)

    def _instrumentation_warning(self, message: str) -> None:
        if self._warning.check() is not None:
            log.warning("mcpd: %s", message)

    # ----------------------------------------------------------------- lifecycle

    @property
    def closed(self) -> bool:
        """True once `shutdown` has run."""
        return self._closed

    @property
    def queue_size(self) -> int:
        """How many signals are waiting to be sent."""
        return len(self._queue)

    @property
    def dropped(self) -> int:
        """How many signals the bounded queue has discarded, ever (B3)."""
        return self._queue.dropped

    @property
    def failures(self) -> int:
        """How many exports or instrumentation calls have failed, ever."""
        return self._failures

    def flush(self, timeout: float = 10.0) -> bool:
        """Send everything queued and wait for it. Returns False on timeout.

        Idempotent, and safe to call from anywhere. In async code call
        `aflush`, which does the same work without blocking the event loop.
        """
        try:
            if not self.enabled or self._worker is None:
                return True
            self._ensure_worker()
            deadline = _deadline(timeout)
            with self._cv:
                self._wake.set()
                while len(self._queue) or self._sending:
                    remaining = deadline()
                    if remaining <= 0:
                        return False
                    self._cv.wait(remaining)
                return True
        except Exception as exc:  # noqa: BLE001 - B1 covers flush too
            self._instrumentation_failed("Mcpd.flush", exc)
            return False

    def shutdown(self, timeout: float = 10.0) -> None:
        """Flush, stop the worker, and release the transport. Safe to call twice.

        One deadline covers the whole call, flushing and stopping together.
        When it passes, the worker starts no further batch and what is still
        queued is given up; `stats().queued` still counts it.
        """
        try:
            with self._lock:
                if self._closed:
                    return
            remaining = _deadline(timeout)
            finished = self.flush(timeout)
            with self._lock:
                self._closed = True
                if not finished:
                    self._abandoned = True
            self._stopping.set()
            self._wake.set()
            worker = self._worker
            if worker is not None and worker.is_alive():
                worker.join(max(0.0, remaining()))
            self._transport.close()
        except Exception as exc:  # noqa: BLE001 - B1
            self._instrumentation_failed("Mcpd.shutdown", exc)

    async def aflush(self, timeout: float = 10.0) -> bool:
        """`flush`, off the event loop."""
        loop = asyncio.get_running_loop()
        return await loop.run_in_executor(None, lambda: self.flush(timeout))

    async def ashutdown(self, timeout: float = 10.0) -> None:
        """`shutdown`, off the event loop."""
        loop = asyncio.get_running_loop()
        await loop.run_in_executor(None, lambda: self.shutdown(timeout))

    def __enter__(self) -> "Mcpd":
        return self

    def __exit__(self, exc_type, exc, tb) -> bool:
        self.shutdown()
        return False

    async def __aenter__(self) -> "Mcpd":
        return self

    async def __aexit__(self, exc_type, exc, tb) -> bool:
        await self.ashutdown()
        return False

    # --------------------------------------------------------------------- reads

    def list_traces(self, *, project: Optional[str] = None, q: Optional[str] = None,
                    status: Optional[Union[SpanStatus, str]] = None,
                    kind: Optional[Union[SpanKind, str]] = None, name: Optional[str] = None,
                    tags: Optional[Sequence[str]] = None, session_id: Optional[str] = None,
                    thread_id: Optional[str] = None, user_id: Optional[str] = None,
                    environment: Optional[str] = None, model: Optional[str] = None,
                    from_: Optional[Any] = None, to: Optional[Any] = None,
                    min_duration_ms: Optional[int] = None, min_cost: Optional[float] = None,
                    has_score: Optional[str] = None, score_below: Optional[float] = None,
                    sort: Optional[str] = None, order: Optional[str] = None,
                    limit: Optional[int] = None, cursor: Optional[str] = None) -> TraceListResponse:
        """A page of runs. `from_`/`to` take a datetime or a shorthand like `-1h`."""
        return decode(TraceListResponse, self._transport.get("/v1/api/traces", {
            "project": project or self.project, "q": q, "status": status, "kind": kind,
            "name": name, "tags": tags, "session_id": session_id, "thread_id": thread_id,
            "user_id": user_id, "environment": environment, "model": model, "from_": from_,
            "to": to, "min_duration_ms": min_duration_ms, "min_cost": min_cost,
            "has_score": has_score, "score_below": score_below, "sort": sort, "order": order,
            "limit": limit, "cursor": cursor,
        }))

    def get_trace(self, trace_id: str, include: Optional[Union[str, Sequence[str]]] = None,
                  *, project: Optional[str] = None) -> TraceDetail:
        """One run and every span in it. `include` adds logs, scores or feedback."""
        if isinstance(include, (list, tuple, set)):
            include = ",".join(include)
        # A trace id is only unique within its project, which is the tenancy key.
        return decode(TraceDetail, self._transport.get(
            f"/v1/api/traces/{trace_id}",
            {"include": include, "project": project or self.project},
        ))

    def delete_trace(self, trace_id: str, *, project: Optional[str] = None) -> DeleteResult:
        """Delete a run and everything attached to it."""
        return decode(DeleteResult, self._transport.request(
            "DELETE",
            f"/v1/api/traces/{trace_id}?" + encode_params({"project": project or self.project}),
        ))

    def list_spans(self, *, project: Optional[str] = None, q: Optional[str] = None,
                   kind: Optional[Union[SpanKind, str]] = None,
                   status: Optional[Union[SpanStatus, str]] = None, name: Optional[str] = None,
                   model: Optional[str] = None, trace_id: Optional[str] = None,
                   from_: Optional[Any] = None, to: Optional[Any] = None,
                   min_duration_ms: Optional[int] = None, sort: Optional[str] = None,
                   order: Optional[str] = None, limit: Optional[int] = None,
                   cursor: Optional[str] = None) -> SpanListResponse:
        """A page of steps, across runs."""
        return decode(SpanListResponse, self._transport.get("/v1/api/spans", {
            "project": project or self.project, "q": q, "kind": kind, "status": status,
            "name": name, "model": model, "trace_id": trace_id, "from_": from_, "to": to,
            "min_duration_ms": min_duration_ms, "sort": sort, "order": order, "limit": limit,
            "cursor": cursor,
        }))

    def list_logs(self, *, project: Optional[str] = None, q: Optional[str] = None,
                  level: Optional[Union[LogLevel, str]] = None, trace_id: Optional[str] = None,
                  span_id: Optional[str] = None, from_: Optional[Any] = None,
                  to: Optional[Any] = None, limit: Optional[int] = None,
                  cursor: Optional[str] = None) -> LogListResponse:
        """A page of log records. Filtering by level is always `>=`."""
        return decode(LogListResponse, self._transport.get("/v1/api/logs", {
            "project": project or self.project, "q": q, "level": level, "trace_id": trace_id,
            "span_id": span_id, "from_": from_, "to": to, "limit": limit, "cursor": cursor,
        }))

    def query_metrics(self, name: str, from_: Any, to: Any, *, project: Optional[str] = None,
                      bucket: Optional[str] = None, agg: Optional[str] = None,
                      group_by: Optional[Sequence[str]] = None,
                      filter: Optional[Mapping[str, Any]] = None) -> MetricSeriesResponse:
        """An aggregated metric series, one line per label combination.

        `filter` keeps points whose labels have these values. It is sent as
        `key:value` pairs joined by commas, so a key cannot contain `:` or `,`
        and a value cannot contain `,`; either raises `ValueError`.
        """
        return decode(MetricSeriesResponse, self._transport.get("/v1/api/metrics", {
            "project": project or self.project, "name": name, "from_": from_, "to": to,
            "bucket": bucket, "agg": agg, "group_by": group_by,
            "filter": _metric_filter(filter),
        }))

    def list_scores(self, *, project: Optional[str] = None, scorer_id: Optional[str] = None,
                    source: Optional[Union[ScoreSource, str]] = None,
                    trace_id: Optional[str] = None, min_value: Optional[float] = None,
                    max_value: Optional[float] = None, from_: Optional[Any] = None,
                    to: Optional[Any] = None, limit: Optional[int] = None,
                    cursor: Optional[str] = None) -> ScoreListResponse:
        """A page of machine evaluations."""
        return decode(ScoreListResponse, self._transport.get("/v1/api/scores", {
            "project": project or self.project, "scorer_id": scorer_id, "source": source,
            "trace_id": trace_id, "min_value": min_value, "max_value": max_value,
            "from_": from_, "to": to, "limit": limit, "cursor": cursor,
        }))

    def list_feedback(self, trace_id: str, *, project: Optional[str] = None,
                      limit: Optional[int] = None) -> FeedbackListResponse:
        """The human evaluations attached to one run.

        The daemon reads feedback one run at a time and refuses any other filter
        with a 400, so the trace id is required and nothing else is sent.
        """
        if not trace_id:
            raise ValueError("list_feedback needs a trace_id: the daemon reads feedback one run at a time")
        return decode(FeedbackListResponse, self._transport.get("/v1/api/feedback", {
            "project": project or self.project, "trace_id": trace_id, "limit": limit,
        }))

    def analytics_summary(self, from_: Any, to: Any, *, project: Optional[str] = None,
                          group_by: Optional[str] = None) -> AnalyticsSummary:
        """Headline numbers for a window: runs, errors, latency, tokens, spend."""
        return decode(AnalyticsSummary, self._transport.get("/v1/api/analytics/summary", {
            "project": project or self.project, "from_": from_, "to": to, "group_by": group_by,
        }))

    def analytics_cost(self, from_: Any, to: Any, *, project: Optional[str] = None,
                       group_by: Optional[str] = None,
                       bucket: Optional[str] = None) -> CostBreakdown:
        """Where the money went, optionally bucketed into a time series."""
        return decode(CostBreakdown, self._transport.get("/v1/api/analytics/cost", {
            "project": project or self.project, "from_": from_, "to": to, "group_by": group_by,
            "bucket": bucket,
        }))

    def health(self) -> HealthStatus:
        """Whether the server is alive, and which spec version it implements."""
        return decode(HealthStatus, self._transport.get("/healthz"))

    def __repr__(self) -> str:
        return (f"Mcpd(endpoint={self.endpoint!r}, project={self.project!r}, "
                f"enabled={self.enabled}, queued={self.queue_size})")


def _encodable(signal: str, record: Any) -> Any:
    """A record whose caller-supplied values `json.dumps` is certain to accept.

    A span's input, output, attributes and metadata, a score's metadata and a
    piece of feedback's value can be anything the caller had to hand. A set, a
    `Decimal`, bytes or a cycle among them made `json.dumps` raise in the
    transport, after the batch was assembled, and every signal that shared the
    batch was lost with it. Encoded here, as a log line's fields already are,
    one value with no JSON form costs its own key and nothing else (B1,
    contract section 4.1).
    """
    if signal == "span":
        changes: Dict[str, Any] = {}
        for name in ("input", "output", "attributes", "metadata"):
            value = getattr(record, name)
            if value is not None:
                changes[name] = encode_fields(value)
        params = record.model_parameters
        if params is not None:
            # A setting JSON cannot carry is left out, as though not given.
            for name in ("temperature", "top_p"):
                value = getattr(params, name)
                if isinstance(value, float) and not math.isfinite(value):
                    params = dataclasses.replace(params, **{name: None})
            changes["model_parameters"] = params
        return dataclasses.replace(record, **changes) if changes else record
    if signal == "score" and record.metadata is not None:
        return dataclasses.replace(record, metadata=encode_fields(record.metadata))
    if signal == "feedback":
        changes = {"value": encode_fields(record.value)}
        if record.metadata is not None:
            changes["metadata"] = encode_fields(record.metadata)
        return dataclasses.replace(record, **changes)
    return record


def _metric_filter(filter: Optional[Mapping[str, Any]]) -> Optional[str]:
    """Label filters the way the daemon reads them: `key:value` joined by commas.

    JSON in this place matched no point, so a filtered query came back as an
    empty series with a 200. A key or value the format cannot carry is refused
    rather than sent, because the daemon would split it into a different filter
    and answer a different question.
    """
    if not filter:
        return None
    pairs = []
    for key, value in filter.items():
        text = value.value if isinstance(value, Enum) else value
        text = ("true" if text else "false") if isinstance(text, bool) else str(text)
        if not key or ":" in key or "," in key:
            raise ValueError(f"metric filter key {key!r} cannot be empty or contain ':' or ','")
        if "," in text:
            raise ValueError(f"metric filter value {text!r} for {key!r} cannot contain ','")
        pairs.append(f"{key}:{text}")
    return ",".join(pairs)


def _invalid_record(span: Any) -> Optional[str]:
    """Why a record handed to `record` cannot be queued, or None when it can."""
    if not isinstance(span, records.Span):
        return f"expected a Span record, got {type(span).__name__}"
    if not is_valid_trace_id(span.trace_id):
        return f"trace_id {span.trace_id!r} is not 32 lowercase hex, or is zero"
    if not is_valid_span_id(span.span_id):
        return f"span_id {span.span_id!r} is not 16 lowercase hex, or is zero"
    if span.parent_span_id is not None and not is_valid_span_id(span.parent_span_id):
        return f"parent_span_id {span.parent_span_id!r} is not 16 lowercase hex, or is zero"
    if not isinstance(span.name, str) or not span.name.strip():
        return "name is empty"
    if span.start_time is None:
        return "start_time is missing"
    return None


def _deadline(timeout: float):
    """A closure returning the seconds left, so a wait loop cannot drift."""
    from time import monotonic

    end = monotonic() + max(0.0, timeout)
    return lambda: end - monotonic()


def _shutdown_at_exit(ref: "weakref.ReferenceType[Mcpd]") -> None:
    """Flush a client that is still alive when the interpreter exits."""
    client = ref()
    if client is not None:
        client.shutdown(timeout=5.0)
