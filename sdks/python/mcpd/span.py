"""The span handle.

A `Span` is what user code holds. It is deliberately dumb: it accumulates
fields and hands its current state to the client's queue twice, once when it
starts and once when it ends (B8). Nothing here blocks and nothing here raises.

The same object serves `with` and `async with`. There is no work to await when a
span ends, so the async protocol is the sync one with an `a` in front of it, and
a user who moves a function from sync to async changes one keyword.
"""

from __future__ import annotations

import asyncio
import functools
import traceback
from time import perf_counter_ns
from typing import (Any, AsyncIterator, Callable, Dict, Iterator, List, Mapping, Optional,
                    Sequence, Union)

from . import _generated as records
from ._generated import EntityKind, SpanKind, SpanStatus
from .context import (
    EPOCH,
    TraceContext,
    current_span,
    format_traceparent,
    nanotime,
    now_ns,
)
from .model_call import response_facts
from .stream import StreamAccumulator

__all__ = ["Span"]


def _closer(it: Any, name: str) -> Any:
    """The iterator's `close` or `aclose`, so stopping early releases the connection."""
    method = getattr(it, name, None)
    return method if callable(method) else None


def _record_sync(stream: Any, observe: Callable[[Any], None],
                 finish: Callable[..., None]) -> Iterator[Any]:
    it = iter(stream)
    try:
        for chunk in it:
            observe(chunk)
            yield chunk
    except GeneratorExit:
        # The caller stopped early: a `break`, `close()` or garbage collection.
        close = _closer(it, "close")
        if close is not None:
            try:
                close()
            except Exception:  # noqa: BLE001 - the provider's cleanup is not ours to raise
                pass
        raise
    except Exception as exc:
        finish(exc)
        raise
    finally:
        finish()


async def _record_async(stream: Any, observe: Callable[[Any], None],
                        finish: Callable[..., None]) -> AsyncIterator[Any]:
    it = stream.__aiter__()
    try:
        while True:
            try:
                chunk = await it.__anext__()
            except StopAsyncIteration:
                break
            observe(chunk)
            yield chunk
    except (GeneratorExit, asyncio.CancelledError):
        # The caller stopped early: a `break`, `aclose()`, a cancelled task.
        close = _closer(it, "aclose")
        if close is not None:
            try:
                await close()
            except Exception:  # noqa: BLE001 - the provider's cleanup is not ours to raise
                pass
        raise
    except Exception as exc:
        finish(exc)
        raise
    finally:
        finish()


def _guard(method):
    """Swallow anything a recording call raises (B1) and keep the chain going.

    Observability must never take down the thing it observes. Every setter
    returns `self` so a failed call is invisible to the caller's expression.
    """

    @functools.wraps(method)
    def wrapper(self, *args, **kwargs):
        try:
            return method(self, *args, **kwargs)
        except Exception as exc:  # noqa: BLE001 - that is the entire point
            self._client._instrumentation_failed(f"Span.{method.__name__}", exc)
            return self

    return wrapper


class Span:
    """One step of a run: an agent invocation, a model call, a tool call."""

    __slots__ = (
        "_client", "_trace_id", "_span_id", "_parent_span_id", "_external_parent_span_id",
        "_name", "_kind", "_status", "_start_ns", "_start_mono", "_end_ns", "_is_root",
        "_is_event", "_sampled", "_recording", "_ended", "_token", "_correlation",
        "_input", "_output", "_attributes", "_metadata", "_usage", "_cost", "_error",
        "_links", "_tags", "_model", "_provider", "_entity_kind", "_entity_id",
        "_entity_name", "_capture", "_usage_sink", "_usage_reported",
        "_model_parameters",
    )

    def __init__(
        self,
        client: Any,
        *,
        trace_id: str,
        span_id: str,
        name: str,
        kind: SpanKind,
        correlation: records.Correlation,
        parent_span_id: Optional[str] = None,
        external_parent_span_id: Optional[str] = None,
        is_root: bool = False,
        is_event: bool = False,
        sampled: bool = True,
        recording: bool = True,
        capture: bool = True,
    ) -> None:
        self._client = client
        self._trace_id = trace_id
        self._span_id = span_id
        self._parent_span_id = parent_span_id
        self._external_parent_span_id = external_parent_span_id
        self._name = name
        self._kind = kind
        self._status = SpanStatus.UNSET
        self._start_ns = now_ns()
        # Wall time says when the span started; the monotonic clock says how
        # long it took. Reading duration off the wall clock would let an NTP
        # step in the middle of a run turn into a negative or absurd latency.
        self._start_mono = perf_counter_ns()
        self._end_ns: Optional[int] = None
        self._is_root = is_root
        self._is_event = is_event
        self._sampled = sampled
        self._recording = recording
        self._ended = False
        self._token = None
        self._correlation = correlation
        self._input: Any = None
        self._output: Any = None
        self._attributes: Optional[Dict[str, Any]] = None
        self._metadata: Optional[Dict[str, Any]] = None
        self._usage: Optional[records.TokenUsage] = None
        self._model_parameters: Optional[records.ModelParameters] = None
        self._cost: Optional[records.Cost] = None
        self._error: Optional[records.SpanError] = None
        self._links: Optional[List[records.SpanLink]] = None
        self._tags: Optional[List[str]] = None
        self._model: Optional[str] = None
        self._provider: Optional[str] = None
        self._entity_kind: Optional[EntityKind] = None
        self._entity_id: Optional[str] = None
        self._entity_name: Optional[str] = None
        # Whether `input` and `output` may leave the process (section 2.1).
        # Decided when the span starts, from the client or this span's own
        # override, so a payload set later is dropped here and never queued.
        self._capture = capture
        # Installed by a step context on the span it opens and inherited by
        # every span nested under it (section 7.3). Called once, at the end,
        # with this span, when it carries usage.
        self._usage_sink: Optional[Callable[["Span"], None]] = None
        self._usage_reported = False

    # ----------------------------------------------------------------- identity

    @property
    def trace_id(self) -> str:
        """The run this span belongs to."""
        return self._trace_id

    @property
    def span_id(self) -> str:
        """This span's id, unique within the trace."""
        return self._span_id

    @property
    def parent_span_id(self) -> Optional[str]:
        """The span above this one, or None on a root."""
        return self._parent_span_id

    @property
    def name(self) -> str:
        """What this operation is called."""
        return self._name

    @property
    def kind(self) -> SpanKind:
        """What kind of operation this is."""
        return self._kind

    @property
    def status(self) -> SpanStatus:
        """The outcome so far."""
        return self._status

    @property
    def sampled(self) -> bool:
        """Whether this trace was kept. Decided at the root and inherited (B4)."""
        return self._sampled

    @property
    def recording(self) -> bool:
        """Whether anything set on this span will be exported."""
        return self._recording and not self._client.closed

    @property
    def ended(self) -> bool:
        """Whether `end` has already run."""
        return self._ended

    @property
    def captures_payloads(self) -> bool:
        """Whether `input` and `output` set on this span are kept in the process."""
        return self._capture

    @property
    def context(self) -> TraceContext:
        """The part of this span that can cross a process boundary."""
        return TraceContext(self._trace_id, self._span_id, self._sampled)

    @property
    def traceparent(self) -> str:
        """This span as a W3C `traceparent`, ready to put on an outbound request (B7)."""
        return format_traceparent(self.context)

    # ------------------------------------------------------------------ children

    def child(self, name: str, kind: Union[SpanKind, str] = SpanKind.GENERIC, **fields: Any) -> "Span":
        """Start a span underneath this one, inheriting the trace decision."""
        try:
            return self._client._start(name=name, kind=kind, parent=self, **fields)
        except Exception as exc:  # noqa: BLE001 - B1 applies to starting a span too
            self._client._instrumentation_failed("Span.child", exc)
            return self._client._dropped_span(name, kind, parent=self)

    def event(self, name: str, **fields: Any) -> "Span":
        """Record a point in time inside this span.

        Events have no duration and no end, so they are exported once and are
        finished the moment they are created.
        """
        try:
            span = self._client._start(name=name, kind=fields.pop("kind", SpanKind.GENERIC),
                                       parent=self, is_event=True, **fields)
            span._ended = True
            # An event never ends, so what it spent is handed up here.
            span._report_usage()
            return span
        except Exception as exc:  # noqa: BLE001
            self._client._instrumentation_failed("Span.event", exc)
            return self._client._dropped_span(name, SpanKind.GENERIC, parent=self)

    # ------------------------------------------------------------------- setters

    @_guard
    def set_input(self, value: Any) -> "Span":
        """What went in. On a model call this is the prompt, and it is the point.

        Dropped here, before it is queued, when the client or this span keeps
        payloads in the process (`capture_payloads=False`).
        """
        if self._capture:
            self._input = value
        return self

    @_guard
    def set_output(self, value: Any) -> "Span":
        """What came out. On a model call this is the completion. Same rule as `set_input`."""
        if self._capture:
            self._output = value
        return self

    @_guard
    def set_usage(
        self,
        input_tokens: Optional[int] = None,
        output_tokens: Optional[int] = None,
        total_tokens: Optional[int] = None,
        *,
        text: Optional[int] = None,
        cache_read: Optional[int] = None,
        cache_write: Optional[int] = None,
        reasoning: Optional[int] = None,
        input_details: Optional[records.InputTokenDetails] = None,
        output_details: Optional[records.OutputTokenDetails] = None,
    ) -> "Span":
        """Token counts for one model call.

        The breakdowns are what cost is computed from, so a cached prompt is
        priced as a cached prompt rather than as fresh input.

        Pass `text` when your provider counts `input_tokens` net of the cached
        share, which is what the Anthropic API does: its `input_tokens` is
        already the uncached share, so `text` is that same number. Without it
        mcpd has to read `input_tokens` as a total that already contains the
        cache, and it bills the difference, which is less than you were
        charged. Providers that report a gross total, OpenAI among them, need
        nothing: leave it unset and the cached share is subtracted, as it
        should be.
        """
        if input_details is None and (
            text is not None or cache_read is not None or cache_write is not None
        ):
            input_details = records.InputTokenDetails(
                text=text, cache_read=cache_read, cache_write=cache_write
            )
        if output_details is None and reasoning is not None:
            output_details = records.OutputTokenDetails(reasoning=reasoning)
        self._usage = records.TokenUsage(
            input_tokens=input_tokens,
            output_tokens=output_tokens,
            total_tokens=total_tokens,
            input_details=input_details,
            output_details=output_details,
        )
        return self

    @_guard
    def set_model_parameters(
        self,
        *,
        temperature: Optional[float] = None,
        top_p: Optional[float] = None,
        top_k: Optional[int] = None,
        max_tokens: Optional[int] = None,
        seed: Optional[int] = None,
        stop_sequences: Optional[Sequence[str]] = None,
        has_response_format: Optional[bool] = None,
    ) -> "Span":
        """How the model was called: the sampling settings you chose.

        These are settings, not payload. Nothing here carries a prompt or a
        completion, so they are recorded whether or not payload capture is on.
        They are what a run has to be reproduced from, and dropping them with
        the prompt would leave a run nobody can repeat.

        One call describes one model call, so calling this twice replaces
        rather than merges: settings half from one request and half from
        another would describe a call nobody made.

        An argument you pass is kept whatever it is, so `seed=0` is a seed and
        `temperature=0` is a temperature. Only `None` means "not given".
        """
        self._model_parameters = records.ModelParameters(
            has_response_format=bool(has_response_format) if has_response_format is not None else False,
            temperature=None if temperature is None else float(temperature),
            top_p=None if top_p is None else float(top_p),
            top_k=None if top_k is None else int(top_k),
            max_tokens=None if max_tokens is None else int(max_tokens),
            seed=None if seed is None else int(seed),
            stop_sequences=None if stop_sequences is None else [str(v) for v in stop_sequences],
        )
        return self

    @_guard
    def set_cost(self, total_cost: float, *, currency: str = "USD",
                 input_cost: Optional[float] = None, output_cost: Optional[float] = None) -> "Span":
        """Report a cost you already know.

        Normally you do not: mcpd prices the span at ingest from its price book.
        Set this only when the provider billed you a figure you would rather keep.
        """
        self._cost = records.Cost(currency=currency, is_estimated=False, input_cost=input_cost,
                                  output_cost=output_cost, total_cost=total_cost)
        return self

    @_guard
    def set_model(self, model: Optional[str] = None, provider: Optional[str] = None) -> "Span":
        """Name the model behind this span, so cost and latency can group by it.

        Both parts are optional so a provider can be recorded on its own, but a
        model with no provider is the common case and is what mcpd prices from.
        """
        if model is not None:
            self._model = str(model)
        if provider is not None:
            self._provider = str(provider)
        return self

    @_guard
    def set_entity(self, kind: Optional[Union[EntityKind, str]] = None,
                   entity_id: Optional[str] = None, name: Optional[str] = None) -> "Span":
        """Name the agent, tool or workflow that produced this span."""
        if kind is not None:
            self._entity_kind = kind if isinstance(kind, EntityKind) else EntityKind(kind)
        if entity_id is not None:
            self._entity_id = str(entity_id)
        if name is not None:
            self._entity_name = str(name)
        return self

    @_guard
    def set_status(self, status: Union[SpanStatus, str]) -> "Span":
        """Set the outcome without ending the span."""
        self._status = status if isinstance(status, SpanStatus) else SpanStatus(status)
        return self

    @_guard
    def set_usage_record(self, usage: records.TokenUsage) -> "Span":
        """Attach a `TokenUsage` you built yourself, breakdowns and all."""
        self._usage = usage
        return self

    @_guard
    def set_error(self, error: Union[str, BaseException], *, kind: Optional[str] = None,
                  stack: Optional[str] = None, retryable: Optional[bool] = None,
                  domain: Optional[str] = None, details: Any = None) -> "Span":
        """Record why this span failed, and mark it failed (B10).

        Accepts an exception directly, because that is what the caller has: the
        message, the class and the traceback are all read off it.
        """
        if isinstance(error, BaseException):
            message = str(error) or error.__class__.__name__
            kind = kind or error.__class__.__name__
            if stack is None:
                stack = "".join(traceback.format_exception(type(error), error, error.__traceback__))
        else:
            message = str(error or "").strip()
        if not message:
            # "Something failed" is not actionable, and the wire type demands a
            # message. Naming the class is the most honest thing left to say.
            message = kind or "error recorded without a message"
            self._client._instrumentation_warning(
                "set_error was called without a message; record what actually failed"
            )
        self._error = records.SpanError(message=message, kind=kind, stack=stack,
                                        domain=domain, retryable=retryable, details=details)
        self._status = SpanStatus.ERROR
        return self

    @_guard
    def set_metadata(self, metadata: Mapping[str, Any]) -> "Span":
        """Attach anything you want. mcpd never interprets it."""
        self._metadata = dict(self._metadata or {})
        self._metadata.update(metadata)
        return self

    @_guard
    def set_attributes(self, attributes: Mapping[str, Any]) -> "Span":
        """Kind-specific structured detail, as described in the attribute reference."""
        self._attributes = dict(self._attributes or {})
        self._attributes.update(attributes)
        return self

    @_guard
    def record_response(self, response: Any, provider: Optional[str] = None) -> "Span":
        """Record what an OpenAI- or Anthropic-shaped response says about its call.

        Sets the model, the provider, the token usage with its cache and
        reasoning meters, and the response id, finish reasons and service tier
        as `gen_ai.response.*` attributes. The daemon prices a batch or flex
        call from the tier.

        It never records the text: set it with `set_output` if you want it
        kept. An unknown shape records what it can and raises nothing.
        """
        facts = response_facts(response, provider)
        if facts.model is not None:
            self._model = facts.model
        if facts.provider is not None:
            self._provider = str(facts.provider)
        if facts.usage is not None:
            self._usage = facts.usage
        if facts.attributes:
            self._attributes = {**(self._attributes or {}), **facts.attributes}
        return self

    @_guard
    def mark_first_token(self) -> "Span":
        """Record the time to first token, from this span's start to now.

        Kept as `mcpd.time_to_first_token_ms`. Only the first call counts, so
        it can sit inside a streaming loop.
        """
        attrs = self._attributes or {}
        if attrs.get("mcpd.time_to_first_token_ms") is not None:
            return self
        # The monotonic clock, as for duration: a wall-clock step mid-stream
        # must not turn into a negative latency.
        ms = (perf_counter_ns() - self._start_mono) / 1e6
        self._attributes = {**attrs, "mcpd.time_to_first_token_ms": ms}
        return self

    def record_stream(self, stream: Any, provider: Optional[str] = None) -> Any:
        """Read a streamed model call through this span, and record it.

        Pass the provider's stream and read the one this returns in its place:
        a generator for an iterator, an async generator for an async iterator.
        Every chunk comes through unchanged. The first chunk with text or a
        tool-call delta marks the first token. When the stream ends, raises, or
        you stop reading early, the span records what `record_response` would
        have read from the whole response, sets the text as its output, and
        ends. An error from the stream is recorded and reaches you unchanged.
        """
        acc = StreamAccumulator()
        state = {"done": False}

        def observe(chunk: Any) -> None:
            try:
                if acc.add(chunk):
                    self.mark_first_token()
            except Exception as exc:  # noqa: BLE001 - B1: a chunk never breaks the stream
                self._client._instrumentation_failed("Span.record_stream", exc)

        def finish(error: Optional[BaseException] = None) -> None:
            if state["done"]:
                return
            state["done"] = True
            try:
                response = acc.response()
                if response is not None:
                    self.record_response(response, provider)
                output = acc.output()
                if output is not None:
                    self.set_output(output)
                if acc.truncated:
                    self.set_attributes({"mcpd.stream.truncated": True})
                if error is not None:
                    self.set_error(error)
            except Exception as exc:  # noqa: BLE001 - B1
                self._client._instrumentation_failed("Span.record_stream", exc)
            self.end()

        if hasattr(stream, "__aiter__"):
            return _record_async(stream, observe, finish)
        return _record_sync(stream, observe, finish)

    @_guard
    def set_guardrail(self, tripped: bool, reason: Optional[str] = None) -> "Span":
        """Record a guardrail's outcome as `mcpd.guardrail.tripped` and `mcpd.guardrail.reason`.

        Tripping is not an error by itself; call `set_error` as well if it
        should fail the span.
        """
        add: Dict[str, Any] = {"mcpd.guardrail.tripped": bool(tripped)}
        if reason is not None:
            add["mcpd.guardrail.reason"] = str(reason)
        self._attributes = {**(self._attributes or {}), **add}
        return self

    @_guard
    def set_prompt(self, prompt: Any) -> "Span":
        """Link the prompt this step used, so `list_spans(prompt=...)` finds it.

        Sets `mcpd.prompt.name` and, unless it is a fallback (version 0),
        `mcpd.prompt.version`. A missing or malformed prompt sets nothing.
        """
        if prompt is None:
            return self
        if isinstance(prompt, Mapping):
            name, version = prompt.get("name"), prompt.get("version")
        else:
            name, version = getattr(prompt, "name", None), getattr(prompt, "version", None)
        # `bool` is an `int`, and `True` as a version would be a lie on the span.
        if not isinstance(name, str) or not name:
            return self
        if not isinstance(version, int) or isinstance(version, bool) or version < 0:
            return self
        add: Dict[str, Any] = {"mcpd.prompt.name": name}
        if version > 0:
            add["mcpd.prompt.version"] = version
        self._attributes = {**(self._attributes or {}), **add}
        return self

    @_guard
    def set_tags(self, tags: Sequence[str]) -> "Span":
        """Label the run. mcpd reads tags from the root span, where they label the trace."""
        self._tags = [str(t) for t in tags]
        return self

    @_guard
    def add_link(self, trace_id: str, span_id: Optional[str] = None, kind: Optional[str] = None,
                 attributes: Any = None) -> "Span":
        """Point at another span or trace this one follows from."""
        self._links = list(self._links or [])
        self._links.append(records.SpanLink(trace_id=trace_id, span_id=span_id, kind=kind,
                                            attributes=attributes))
        return self

    # -------------------------------------------------------- correlated signals

    def log(self, level: Any, message: str, fields: Optional[Mapping[str, Any]] = None,
            **kwargs: Any) -> None:
        """Write a log line correlated to this span."""
        self._client.log(level, message, fields, trace_id=self._trace_id, span_id=self._span_id,
                         **kwargs)

    def score(self, scorer_id: str, value: float, *, min: float, max: float, **kwargs: Any) -> None:
        """Attach a machine evaluation of this span."""
        self._client.score(scorer_id, value, min=min, max=max, trace_id=self._trace_id,
                           span_id=self._span_id, **kwargs)

    def feedback(self, kind: Any, value: Any, **kwargs: Any) -> None:
        """Attach a human evaluation of this span."""
        self._client.feedback(kind, value, trace_id=self._trace_id, span_id=self._span_id, **kwargs)

    # ----------------------------------------------------------------- lifecycle

    def end(self, status: Optional[Union[SpanStatus, str]] = None) -> None:
        """Finish the span and queue its final state. Safe to call twice."""
        try:
            if self._ended:
                return
            self._ended = True
            self._end_ns = self._start_ns + (perf_counter_ns() - self._start_mono)
            if status is not None:
                self._status = SpanStatus(status) if not isinstance(status, SpanStatus) else status
            elif self._status is SpanStatus.UNSET:
                self._status = SpanStatus.ERROR if self._error else SpanStatus.OK
            self._client._enqueue_span(self)
            self._report_usage()
        except Exception as exc:  # noqa: BLE001 - B1: ending a span cannot fail loudly
            self._client._instrumentation_failed("Span.end", exc)

    def _report_usage(self) -> None:
        """Hand this span's usage to the step that opened it, once (section 7.3)."""
        if self._usage_sink is None or self._usage is None or self._usage_reported:
            return
        self._usage_reported = True
        try:
            self._usage_sink(self)
        except Exception as exc:  # noqa: BLE001 - B1
            self._client._instrumentation_failed("Span.usage", exc)

    def _attach(self) -> "Span":
        """Make this the ambient span for the current context."""
        try:
            self._token = current_span.set(self)
        except Exception as exc:  # noqa: BLE001
            self._client._instrumentation_failed("Span.attach", exc)
        return self

    def _detach(self) -> None:
        try:
            if self._token is not None:
                current_span.reset(self._token)
                self._token = None
        except (ValueError, RuntimeError):
            # Reset across a different context, which happens when a span is
            # entered in one task and left in another. The variable will be
            # collected with its context; there is nothing to repair.
            self._token = None

    def _finish(self, exc: Optional[BaseException]) -> None:
        """Shared exit path for both context-manager protocols."""
        if exc is not None and self._error is None:
            self.set_error(exc)
        self.end()
        self._detach()

    def __enter__(self) -> "Span":
        return self._attach()

    def __exit__(self, exc_type, exc, tb) -> bool:
        self._finish(exc)
        return False  # never swallow the caller's exception

    async def __aenter__(self) -> "Span":
        return self._attach()

    async def __aexit__(self, exc_type, exc, tb) -> bool:
        self._finish(exc)
        return False

    # ------------------------------------------------------------------- export

    def to_record(self) -> records.Span:
        """The generated wire record for this span, as it stands right now."""
        duration = None if self._end_ns is None else max(0, self._end_ns - self._start_ns)
        return records.Span(
            trace_id=self._trace_id,
            span_id=self._span_id,
            parent_span_id=self._parent_span_id,
            external_parent_span_id=self._external_parent_span_id,
            name=self._name,
            kind=self._kind,
            status=self._status,
            start_time=nanotime(self._start_ns),
            end_time=None if self._end_ns is None else nanotime(self._end_ns),
            duration_ns=duration,
            is_root=self._is_root,
            is_event=self._is_event,
            entity_kind=self._entity_kind,
            entity_id=self._entity_id,
            entity_name=self._entity_name,
            input=self._input,
            output=self._output,
            attributes=self._attributes,
            metadata=self._metadata,
            usage=self._usage,
            model_parameters=self._model_parameters,
            cost=self._cost,
            error=self._error,
            links=self._links,
            tags=self._tags,
            correlation=self._correlation,
            model=self._model,
            provider=self._provider,
            # The server stamps `received_at` from its own clock; the epoch is
            # how a producer says so, and the field is not optional on the wire.
            received_at=EPOCH,
        )

    def __repr__(self) -> str:
        return (f"Span(name={self._name!r}, kind={self._kind.value}, trace_id={self._trace_id!r}, "
                f"span_id={self._span_id!r}, status={self._status.value})")
