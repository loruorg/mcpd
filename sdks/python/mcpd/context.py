"""Identifiers, timestamps and the ambient span.

Three things that every other module needs and that must not disagree with the
server: ids are shaped exactly as OpenTelemetry shapes them, timestamps are UTC
and carry as much precision as the machine can report, and "the span I am
inside" is a `contextvars.ContextVar`, so it follows an `await` into a task and
does not leak between threads.
"""

from __future__ import annotations

import contextvars
import os
import re
from dataclasses import dataclass
from datetime import datetime, timezone
from time import time_ns
from typing import Any, Optional

__all__ = [
    "EPOCH",
    "NanoTime",
    "TraceContext",
    "current_span",
    "format_traceparent",
    "is_valid_span_id",
    "is_valid_trace_id",
    "nanotime",
    "new_span_id",
    "new_trace_id",
    "now_ns",
    "parse_traceparent",
]

#: Characters in a hex-encoded trace id.
TRACE_ID_LEN = 32
#: Characters in a hex-encoded span id.
SPAN_ID_LEN = 16

_HEX = re.compile(r"\A[0-9a-f]+\Z")


def _random_id(length: int) -> str:
    """A lowercase hex id of `length` characters that is never all zero.

    OpenTelemetry reserves the all-zero id to mean "no trace". Drawing again is
    cheaper than emitting an id every downstream system will discard.
    """
    n = length // 2
    while True:
        raw = os.urandom(n)
        if any(raw):
            return raw.hex()


def new_trace_id() -> str:
    """A fresh 128-bit trace id, 32 lowercase hex characters."""
    return _random_id(TRACE_ID_LEN)


def new_span_id() -> str:
    """A fresh 64-bit span id, 16 lowercase hex characters."""
    return _random_id(SPAN_ID_LEN)


def is_valid_trace_id(value: Any) -> bool:
    """True when `value` is a well-formed, non-zero trace id."""
    return _valid(value, TRACE_ID_LEN)


def is_valid_span_id(value: Any) -> bool:
    """True when `value` is a well-formed, non-zero span id."""
    return _valid(value, SPAN_ID_LEN)


def _valid(value: Any, length: int) -> bool:
    return (
        isinstance(value, str)
        and len(value) == length
        and bool(_HEX.match(value))
        and value.strip("0") != ""
    )


class NanoTime(datetime):
    """A UTC datetime that also remembers the nanoseconds datetime cannot hold.

    `datetime` stops at microseconds, but the wire format and every other mcpd
    SDK carry nanoseconds. Subclassing keeps the generated dataclasses' declared
    type honest while the codec reads `.ns` and writes all nine digits.
    """

    __slots__ = ("ns",)

    @classmethod
    def from_ns(cls, ns: int) -> "NanoTime":
        """Build from a Unix epoch nanosecond count, as `time.time_ns()` returns."""
        secs, rem = divmod(int(ns), 1_000_000_000)
        base = datetime.fromtimestamp(secs, tz=timezone.utc)
        self = cls(
            base.year, base.month, base.day, base.hour, base.minute, base.second,
            rem // 1000, tzinfo=timezone.utc,
        )
        object.__setattr__(self, "ns", int(ns))
        return self

    def isoformat_ns(self) -> str:
        """RFC 3339 with nanosecond precision and a `Z` offset."""
        secs, rem = divmod(self.ns, 1_000_000_000)
        base = datetime.fromtimestamp(secs, tz=timezone.utc)
        return f"{base.strftime('%Y-%m-%dT%H:%M:%S')}.{rem:09d}Z"


def now_ns() -> int:
    """The current wall-clock time in Unix epoch nanoseconds."""
    return time_ns()


def nanotime(ns: int) -> NanoTime:
    """Shorthand for `NanoTime.from_ns`."""
    return NanoTime.from_ns(ns)


#: The sentinel a producer sends for `received_at`.
#:
#: `received_at` is the server's own clock, not ours, and the server overwrites
#: any value equal to the epoch. The field is required by the wire format, so
#: sending the sentinel is how a client says "you stamp it".
EPOCH = NanoTime.from_ns(0)


@dataclass(frozen=True)
class TraceContext:
    """The part of a span that crosses a process boundary."""

    trace_id: str
    span_id: str
    sampled: bool = True

    def traceparent(self) -> str:
        """This context as a W3C `traceparent` header value."""
        return format_traceparent(self)


_TRACEPARENT = re.compile(
    r"\A(?P<version>[0-9a-f]{2})-(?P<trace>[0-9a-f]{32})-(?P<span>[0-9a-f]{16})-(?P<flags>[0-9a-f]{2})\Z"
)


def parse_traceparent(value: Optional[str]) -> Optional[TraceContext]:
    """Read a W3C `traceparent`, or return None when it is not one.

    Version `ff` is invalid by the specification; a higher version is not, so it
    is parsed on the strength of the four fields every future version must keep.
    """
    if not isinstance(value, str):
        return None
    m = _TRACEPARENT.match(value.strip().lower())
    if m is None:
        return None
    if m.group("version") == "ff":
        return None
    trace_id, span_id = m.group("trace"), m.group("span")
    if not is_valid_trace_id(trace_id) or not is_valid_span_id(span_id):
        return None
    return TraceContext(trace_id, span_id, sampled=bool(int(m.group("flags"), 16) & 0x01))


def format_traceparent(ctx: TraceContext) -> str:
    """Render a context as a version-00 `traceparent`."""
    return f"00-{ctx.trace_id}-{ctx.span_id}-{'01' if ctx.sampled else '00'}"


#: The span the calling code is inside, if any.
#:
#: A `ContextVar` is the only mechanism that behaves correctly for threads,
#: `asyncio` tasks and generators at once, which is why the contract names it.
current_span: "contextvars.ContextVar[Optional[Any]]" = contextvars.ContextVar(
    "mcpd_current_span", default=None
)
