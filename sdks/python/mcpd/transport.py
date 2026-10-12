"""The wire: JSON codec, HTTP, gzip and retry.

Everything that knows what mcpd's API looks like lives here, so the tracing code
above it never touches a socket. Two rules from the contract shape this module:

* Section 6: batches are gzipped above 1 KiB, carry a stable `ingest_key` so a
  retry cannot double-count, and retry only on 429, 502, 503, 504 and a
  connection failure. A 4xx that is not 429 is malformed and will stay malformed.
* B1: nothing here is allowed to reach the caller from the export path. Errors
  are raised as `TransportError` and swallowed by the client's worker. Query
  methods are the exception: a read that quietly returned nothing would be worse
  than one that failed.
"""

from __future__ import annotations

import gzip
import json
import random
import re
import socket
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import fields as dataclass_fields, is_dataclass
from datetime import datetime, timezone
from enum import Enum
from time import sleep
from typing import Any, Dict, Iterable, List, Mapping, NamedTuple, Optional, Type, TypeVar

from . import _generated as records
from .context import NanoTime

__all__ = ["McpdError", "TransportError", "Transport", "decode", "encode"]

T = TypeVar("T")

#: Bodies larger than this are gzipped. Below it, compression costs more CPU
#: than it saves bytes.
GZIP_THRESHOLD = 1024

#: Statuses worth trying again. Everything else 4xx is the sender's fault.
RETRY_STATUS = frozenset({429, 502, 503, 504})

#: Total attempts, including the first.
MAX_ATTEMPTS = 3

#: First backoff step. Doubles per attempt, with full jitter.
BACKOFF_BASE = 0.25

#: However long a server asks us to wait, we stop waiting here. An honest
#: `Retry-After: 3600` must not hold a process open through shutdown.
BACKOFF_CAP = 30.0


class McpdError(Exception):
    """A call the caller asked for could not be completed."""


class TransportError(McpdError):
    """An HTTP or network failure, with the status when the server answered."""

    def __init__(self, message: str, status: Optional[int] = None, retryable: bool = False,
                 body: Optional[str] = None) -> None:
        super().__init__(message)
        self.status = status
        self.retryable = retryable
        #: The response body, when the server answered. A 409 from the run
        #: surface carries its reason here.
        self.body = body


# --------------------------------------------------------------------------- #
# Codec
# --------------------------------------------------------------------------- #

# The generated module records the fields whose spec name is a Python keyword.
# Reversing it here means the codec never has to guess that `from_` is `from`.
_ATTR_TO_WIRE = {attr: wire for wire, attr in records.RESERVED_FIELD_NAMES.items()}
_WIRE_TO_ATTR = dict(records.RESERVED_FIELD_NAMES)

_TIMESTAMP = re.compile(
    r"\A(\d{4})-(\d{2})-(\d{2})[Tt ](\d{2}):(\d{2}):(\d{2})(\.\d+)?(?:[Zz]|([+-])(\d{2}):?(\d{2}))\Z"
)


def _iso(value: datetime) -> str:
    """RFC 3339 in UTC, at the best precision the value carries.

    B9: never local time. A naive datetime is read as UTC rather than guessed
    at, because guessing is how a distributed trace becomes unreadable.
    """
    if isinstance(value, NanoTime):
        return value.isoformat_ns()
    if value.tzinfo is None:
        value = value.replace(tzinfo=timezone.utc)
    return value.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%f") + "Z"


def encode(value: Any) -> Any:
    """Turn generated dataclasses into the JSON shape the server accepts.

    `None` fields are omitted, matching the server's `skip_serializing_if`, so a
    batch carries only what the producer actually set.
    """
    if is_dataclass(value) and not isinstance(value, type):
        out: Dict[str, Any] = {}
        for f in dataclass_fields(value):
            v = getattr(value, f.name)
            if v is None:
                continue
            out[_ATTR_TO_WIRE.get(f.name, f.name)] = encode(v)
        return out
    if isinstance(value, Enum):
        return value.value
    if isinstance(value, datetime):
        return _iso(value)
    if isinstance(value, (list, tuple)):
        return [encode(v) for v in value]
    if isinstance(value, dict):
        return {str(k): encode(v) for k, v in value.items()}
    return value


def parse_timestamp(text: str) -> NanoTime:
    """Read an RFC 3339 timestamp without losing sub-microsecond digits."""
    m = _TIMESTAMP.match(text.strip())
    if m is None:
        raise TransportError(f"{text!r} is not an RFC 3339 timestamp")
    y, mo, d, h, mi, s = (int(m.group(i)) for i in range(1, 7))
    frac = m.group(7) or ""
    nanos = int((frac[1:] + "000000000")[:9]) if frac else 0
    base = datetime(y, mo, d, h, mi, s, tzinfo=timezone.utc)
    offset = 0
    if m.group(8):
        offset = (int(m.group(9)) * 3600 + int(m.group(10)) * 60) * (-1 if m.group(8) == "+" else 1)
    return NanoTime.from_ns(int(base.timestamp() + offset) * 1_000_000_000 + nanos)


def _annotation(text: str) -> "tuple[str, bool]":
    """Split a generated annotation into (base type name, is a list).

    The annotations are strings, because the generated module uses
    `from __future__ import annotations`. Reading them as text rather than
    evaluating them is what keeps this decoder working on Python 3.9, where
    `X | None` is not valid at runtime.
    """
    t = text.strip()
    if t.endswith("| None"):
        t = t[: -len("| None")].strip()
    if t.startswith("list[") and t.endswith("]"):
        return t[5:-1].strip(), True
    return t, False


def _decode_value(kind: str, value: Any) -> Any:
    if value is None:
        return None
    target = getattr(records, kind, None)
    if target is not None and isinstance(target, type):
        if issubclass(target, Enum):
            # A server one version ahead may know a variant this build does not.
            # Keeping the raw string beats raising on a value we simply have not
            # heard of; every enum here is a `str` enum, so comparisons still work.
            try:
                return target(value)
            except ValueError:
                return value
        if is_dataclass(target):
            return decode(target, value)
    if kind == "datetime":
        return parse_timestamp(value) if isinstance(value, str) else value
    return value


def decode(cls: Type[T], data: Any) -> T:
    """Build a generated dataclass from a decoded JSON object."""
    if not isinstance(data, dict):
        raise TransportError(f"expected a JSON object for {cls.__name__}, got {type(data).__name__}")
    kwargs: Dict[str, Any] = {}
    for f in dataclass_fields(cls):
        wire = _ATTR_TO_WIRE.get(f.name, f.name)
        if wire not in data:
            continue
        kind, is_list = _annotation(str(f.type))
        raw = data[wire]
        if is_list:
            kwargs[f.name] = [_decode_value(kind, v) for v in (raw or [])]
        else:
            kwargs[f.name] = _decode_value(kind, raw)
    try:
        return cls(**kwargs)  # type: ignore[call-arg]
    except TypeError as exc:
        raise TransportError(f"{cls.__name__} is missing a field the server did not send: {exc}") from exc


def encode_params(params: Mapping[str, Any]) -> str:
    """Render a query string, dropping anything the caller left unset."""
    pairs: List[tuple] = []
    for key, value in params.items():
        if value is None:
            continue
        wire = _ATTR_TO_WIRE.get(key, key)
        if isinstance(value, (list, tuple, set)):
            # One comma-joined value, never a repeated key. The daemon reads the
            # query as a map, so `tags=a&tags=b` keeps only `b` and answers for
            # one tag instead of both.
            items: Iterable[Any] = value
            joined = ",".join(_param(item) for item in items if item is not None)
            if joined:
                pairs.append((wire, joined))
        else:
            pairs.append((wire, _param(value)))
    return urllib.parse.urlencode(pairs)


def _param(value: Any) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, Enum):
        return str(value.value)
    if isinstance(value, datetime):
        return _iso(value)
    if isinstance(value, (dict, list)):
        return json.dumps(encode(value), separators=(",", ":"))
    return str(value)


# --------------------------------------------------------------------------- #
# HTTP
# --------------------------------------------------------------------------- #


class Response(NamedTuple):
    """One HTTP answer, however it was fetched."""

    status: int
    headers: Mapping[str, str]
    body: bytes


class UrllibSender:
    """The default sender: `urllib.request`, and therefore no dependencies.

    A user who installs mcpd gets a working exporter with nothing else in their
    lockfile, which is the whole point of the zero-dependency rule.
    """

    name = "urllib"

    def send(self, method: str, url: str, headers: Mapping[str, str], body: Optional[bytes],
             timeout: float) -> Response:
        request = urllib.request.Request(url, data=body, method=method)
        for key, value in headers.items():
            request.add_header(key, value)
        try:
            with urllib.request.urlopen(request, timeout=timeout) as resp:
                return Response(resp.status, dict(resp.headers.items()), resp.read())
        except urllib.error.HTTPError as exc:
            # The server answered, so this is a status to be judged, not a
            # connection failure. Read the body: mcpd puts the reason in it.
            return Response(exc.code, dict(exc.headers.items() if exc.headers else {}), exc.read())
        except (urllib.error.URLError, socket.timeout, OSError) as exc:
            raise TransportError(f"{method} {url} did not complete: {exc}", retryable=True) from exc

    def close(self) -> None:
        """Nothing to release. Present so both senders have one interface."""


class HttpxSender:
    """An optional sender backed by `httpx`, for connection reuse.

    Installed as the `httpx` extra. It is a performance choice only: the SDK
    behaves identically without it.
    """

    name = "httpx"

    def __init__(self, timeout: float) -> None:
        import httpx  # imported lazily so the dependency stays genuinely optional

        self._httpx = httpx
        self._client = httpx.Client(timeout=timeout)

    def send(self, method: str, url: str, headers: Mapping[str, str], body: Optional[bytes],
             timeout: float) -> Response:
        try:
            resp = self._client.request(method, url, headers=dict(headers), content=body,
                                        timeout=timeout)
        except self._httpx.HTTPError as exc:
            raise TransportError(f"{method} {url} did not complete: {exc}", retryable=True) from exc
        return Response(resp.status_code, dict(resp.headers), resp.content)

    def close(self) -> None:
        self._client.close()


def default_sender(timeout: float, prefer_httpx: bool = True) -> Any:
    """Pick a sender: `httpx` when it is installed, `urllib` otherwise."""
    if prefer_httpx:
        try:
            return HttpxSender(timeout)
        except Exception:  # noqa: BLE001 - any import or construction failure falls back
            pass
    return UrllibSender()


class Transport:
    """Signed, compressed, retrying HTTP against one mcpd endpoint."""

    def __init__(self, endpoint: str, api_key: Optional[str] = None, timeout: float = 10.0,
                 sender: Any = None, user_agent: str = "mcpd-python") -> None:
        self.endpoint = endpoint.rstrip("/")
        self.api_key = api_key
        self.timeout = timeout
        self.user_agent = user_agent
        self._sender = sender if sender is not None else default_sender(timeout)
        #: Sleep function, so tests can assert on backoff without waiting for it.
        self.sleep = sleep

    def close(self) -> None:
        """Release the sender's resources. Safe to call more than once."""
        try:
            self._sender.close()
        except Exception:  # noqa: BLE001 - closing must not be the thing that fails
            pass

    def _headers(self, extra: Optional[Mapping[str, str]] = None) -> Dict[str, str]:
        headers = {"user-agent": self.user_agent, "accept": "application/json"}
        if self.api_key:
            headers["authorization"] = f"Bearer {self.api_key}"
        if extra:
            headers.update(extra)
        return headers

    def export(self, batch: records.IngestBatch, ingest_key: str) -> records.IngestResult:
        """The sink protocol (contract section 2.2): the default sink is this transport."""
        batch.ingest_key = ingest_key
        return self.ingest(batch)

    def ingest(self, batch: records.IngestBatch) -> records.IngestResult:
        """POST one batch to `/v1/ingest`.

        The batch's `ingest_key` is set by the caller and never changed here, so
        every retry of this body is the same idempotent write.
        """
        payload = json.dumps(encode(batch), separators=(",", ":")).encode("utf-8")
        headers = {"content-type": "application/json"}
        if len(payload) > GZIP_THRESHOLD:
            payload = gzip.compress(payload, compresslevel=6)
            headers["content-encoding"] = "gzip"
        resp = self.request("POST", "/v1/ingest", body=payload, headers=headers)
        return decode(records.IngestResult, resp)

    def get(self, path: str, params: Optional[Mapping[str, Any]] = None) -> Any:
        """GET a query endpoint and return the decoded JSON."""
        query = encode_params(params or {})
        return self.request("GET", f"{path}?{query}" if query else path)

    def post(self, path: str, value: Any, hold: float = 0.0) -> Any:
        """POST a single record and return the decoded JSON.

        `hold` is how long the daemon may hold the request open before it
        answers, in seconds, as it does for a long poll. The request's timeout
        is that plus the configured timeout; otherwise the client would cancel
        a poll longer than its own timeout, which is every idle claim.
        """
        body = json.dumps(encode(value), separators=(",", ":")).encode("utf-8")
        return self.request("POST", path, body=body, headers={"content-type": "application/json"}, hold=hold)

    def request(self, method: str, path: str, body: Optional[bytes] = None,
                headers: Optional[Mapping[str, str]] = None, hold: float = 0.0) -> Any:
        """One request, retried per the contract, returning decoded JSON."""
        url = f"{self.endpoint}{path}"
        last: Optional[TransportError] = None
        for attempt in range(MAX_ATTEMPTS):
            try:
                resp = self._sender.send(method, url, self._headers(headers), body, self.timeout + hold)
            except TransportError as exc:
                last = exc
            else:
                if 200 <= resp.status < 300:
                    if not resp.body:
                        return None
                    try:
                        return json.loads(resp.body.decode("utf-8"))
                    except ValueError as exc:
                        raise TransportError(f"{method} {path} returned a body that is not JSON: {exc}",
                                             status=resp.status) from exc
                last = TransportError(
                    f"{method} {path} failed: {resp.status} {_reason(resp.body)}",
                    status=resp.status,
                    retryable=resp.status in RETRY_STATUS,
                    body=resp.body.decode("utf-8", "replace")[:4096],
                )
                if not last.retryable:
                    raise last
                wait = _retry_after(resp.headers)
                if attempt + 1 < MAX_ATTEMPTS:
                    self.sleep(wait if wait is not None else _backoff(attempt))
                continue
            if attempt + 1 < MAX_ATTEMPTS:
                self.sleep(_backoff(attempt))
        raise last if last is not None else TransportError(f"{method} {path} failed")


def _backoff(attempt: int) -> float:
    """Exponential backoff with full jitter.

    Jitter matters more than the curve: a fleet that all retry at 250 ms
    reproduces the load spike that caused the failure.
    """
    return min(BACKOFF_CAP, random.uniform(0.0, BACKOFF_BASE * (2 ** attempt)))


def _retry_after(headers: Mapping[str, str]) -> Optional[float]:
    """Honour `Retry-After` in seconds, capped so it cannot stall a shutdown."""
    for key, value in headers.items():
        if key.lower() != "retry-after":
            continue
        try:
            return max(0.0, min(BACKOFF_CAP, float(value.strip())))
        except (TypeError, ValueError):
            return None
    return None


def _reason(body: bytes) -> str:
    """Pull mcpd's own error message out of a response body, when it has one."""
    try:
        parsed = json.loads(body.decode("utf-8"))
    except Exception:  # noqa: BLE001 - a non-JSON body is still worth showing
        return body.decode("utf-8", "replace")[:200]
    if isinstance(parsed, dict) and isinstance(parsed.get("message"), str):
        return parsed["message"]
    return json.dumps(parsed)[:200]
