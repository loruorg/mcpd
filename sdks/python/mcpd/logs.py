"""The logging signal: what a field map may hold, and how a level relates to an
OpenTelemetry severity number.

Nothing here raises. A field the caller handed us is data, and data that cannot
be encoded is worth less than the batch it travels in.
"""

from __future__ import annotations

import datetime as _datetime
import math
import traceback
from enum import Enum
from typing import Any, Dict, List, Optional, Union

from ._generated import LogLevel

__all__ = [
    "describe_error",
    "encode_fields",
    "log_level_from_severity",
    "meets_level",
    "parse_log_level",
    "severity_number",
]

#: How deep a nested field is followed before it is named rather than walked.
_MAX_DEPTH = 12


def severity_number(level: Union[LogLevel, str]) -> int:
    """The OpenTelemetry severity number the spec gives a level."""
    return _as_level(level).severity


def log_level_from_severity(severity: int) -> LogLevel:
    """The level an OpenTelemetry severity number lands on.

    By range, the way the daemon reads an inbound OTLP `LogRecord`: 1 to 4 is
    trace, 5 to 8 debug, 9 to 12 info, 13 to 16 warn, 17 to 20 error, and 21 and
    above fatal. A number outside the range is clamped rather than refused,
    because a sender that invents a 40 still meant something serious.
    """
    ordered = sorted(LogLevel, key=lambda level: level.severity)
    found = ordered[0]
    for candidate in ordered:
        if severity >= candidate.severity:
            found = candidate
    return found


def meets_level(level: Union[LogLevel, str], floor: Union[LogLevel, str]) -> bool:
    """True when `level` is at or above the floor."""
    return severity_number(level) >= severity_number(floor)


def parse_log_level(name: Optional[str]) -> Optional[LogLevel]:
    """Read a level by name, case-insensitively. Anything else is None."""
    if name is None:
        return None
    try:
        return LogLevel(name.strip().lower())
    except ValueError:
        return None


def describe_error(value: Any) -> Optional[Dict[str, str]]:
    """An exception as its type, message and traceback.

    `json.dumps` refuses an exception outright, so a batch carrying one was lost
    whole: every span, metric, score and piece of feedback that shared it went
    with the log line. The traceback is present only when the exception was
    actually raised, because one that was constructed and never raised has none.
    """
    if not isinstance(value, BaseException):
        return None
    described = {
        "type": type(value).__name__,
        "message": str(value) or type(value).__name__,
    }
    if value.__traceback__ is not None:
        described["stack"] = "".join(
            traceback.format_exception(type(value), value, value.__traceback__)
        ).strip()
    return described


def encode_fields(value: Any) -> Any:
    """A caller's field map, in a shape `json.dumps` is certain to accept.

    One value nothing can encode costs its own key and nothing else. Losing a
    field is a small thing; losing the batch it travelled in is B1 broken in the
    quietest possible way, because the process survives and the data does not.
    """
    return _encode(value, set(), 0)


def _as_level(level: Union[LogLevel, str]) -> LogLevel:
    if isinstance(level, LogLevel):
        return level
    return parse_log_level(str(level)) or LogLevel.INFO


def _encode(value: Any, seen: set, depth: int) -> Any:
    if isinstance(value, float) and not math.isfinite(value):
        # `json.dumps` writes NaN and the infinities as bare words, which are
        # not JSON, and the daemon refuses the whole batch over one. Named as a
        # string, it costs its own key and nothing else (contract section 4.1).
        return str(value)
    if value is None or isinstance(value, (bool, int, float, str)):
        # bool before int on purpose: bool is a subclass of int and both encode.
        return value
    described = describe_error(value)
    if described is not None:
        return described
    if isinstance(value, Enum):
        return _encode(value.value, seen, depth)
    if isinstance(value, (_datetime.datetime, _datetime.date, _datetime.time)):
        return value.isoformat()
    if isinstance(value, (bytes, bytearray)):
        return value.decode("utf-8", "replace")
    if isinstance(value, (dict, list, tuple, set, frozenset)):
        marker = id(value)
        # A cycle is the common way a caller's object cannot be encoded: a
        # request holding its own response, a node holding its parent. Naming it
        # is more use to the reader than dropping the key.
        if marker in seen:
            return "[circular]"
        if depth >= _MAX_DEPTH:
            return "[too deep]"
        seen.add(marker)
        try:
            if isinstance(value, dict):
                out: Dict[str, Any] = {}
                for key, item in value.items():
                    out[_key(key)] = _encode(item, seen, depth + 1)
                return out
            items: List[Any] = [_encode(item, seen, depth + 1) for item in value]
            return items
        except Exception:  # noqa: BLE001 - B1: a field is never worth the batch
            return _text(value)
        finally:
            seen.discard(marker)
    return _text(value)


def _key(key: Any) -> str:
    try:
        return key if isinstance(key, str) else str(key)
    except Exception:  # noqa: BLE001 - a __str__ of the caller's own
        return "[unreadable]"


def _text(value: Any) -> str:
    try:
        return str(value)
    except Exception:  # noqa: BLE001 - a __str__ that raises is still not our problem
        return "[unencodable]"
