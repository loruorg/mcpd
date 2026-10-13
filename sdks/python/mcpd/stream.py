"""Streamed model calls (SDK contract 3.2).

A streamed response spreads the facts `record_response` reads across its
chunks: the id and model in the first, the text in the middle, the finish
reason and the usage at the end. This collects them, one chunk at a time, and
builds the response the provider would have sent without streaming, so the span
records exactly what it would have recorded for the same call made the other
way.

Chunks are read the way `record_response` reads a response: a mapping, a
provider SDK's pydantic model (dumped first) or plain attributes. Nothing here
raises on a chunk it does not understand, and nothing grows without bound: text
and tool-call arguments share one budget, and tool calls are capped by count.
"""

from __future__ import annotations

from typing import Any, Dict, List, Mapping, Optional

from .model_call import _field, _obj

__all__ = ["STREAM_MAX_TOOL_CALLS", "STREAM_TEXT_BUDGET", "StreamAccumulator"]

#: Code points of text and tool-call arguments kept, together.
STREAM_TEXT_BUDGET = 32_768
#: Tool calls kept. Later ones are ignored.
STREAM_MAX_TOOL_CALLS = 64

_ANTHROPIC_EVENTS = frozenset(
    ("message_start", "content_block_start", "content_block_delta", "message_delta")
)


def _str(value: Any) -> Optional[str]:
    return value if isinstance(value, str) and value else None


def _index(value: Any, fallback: int) -> int:
    # A bool is an int in Python, and `True` is not a position.
    if isinstance(value, int) and not isinstance(value, bool) and value >= 0:
        return value
    return fallback


def _list(value: Any) -> Optional[List[Any]]:
    return list(value) if isinstance(value, (list, tuple)) else None


def _has(obj: Any, key: str) -> bool:
    return key in obj if isinstance(obj, Mapping) else hasattr(obj, key)


def _dict(value: Any) -> Optional[Dict[str, Any]]:
    """A plain dict of the value's fields, so usage from two events can be merged."""
    obj = _obj(value)
    if obj is None:
        return None
    if isinstance(obj, Mapping):
        return dict(obj)
    try:
        return {k: v for k, v in vars(obj).items() if not k.startswith("_")}
    except TypeError:
        return None


class _ToolCall:
    __slots__ = ("id", "name", "arguments")

    def __init__(self) -> None:
        self.id: Optional[str] = None
        self.name: Optional[str] = None
        self.arguments = ""


class StreamAccumulator:
    """What one stream has said so far."""

    def __init__(self) -> None:
        self.shape: Optional[str] = None
        self.truncated = False
        self._id: Optional[str] = None
        self._model: Optional[str] = None
        self._tier: Optional[str] = None
        self._text: List[str] = []
        self._budget = STREAM_TEXT_BUDGET
        self._tools: Dict[int, _ToolCall] = {}
        self._finish: Dict[int, str] = {}
        self._stop: Optional[str] = None
        self._usage: Any = None

    def add(self, chunk: Any) -> bool:
        """Fold one chunk in. Answers whether it carried text or a tool-call delta."""
        c = _obj(chunk)
        if c is None:
            return False
        raw = _field(c, "choices")
        choices = _list(raw)
        # A typed chunk with no choices can serialise them as null.
        if choices is None and raw is None and _has(c, "choices"):
            choices = []
        if choices is not None:
            return self._openai(c, choices)
        kind = _field(c, "type")
        if isinstance(kind, str) and kind in _ANTHROPIC_EVENTS:
            return self._anthropic(c, kind)
        return False

    def _openai(self, c: Any, choices: List[Any]) -> bool:
        if self.shape is None:
            self.shape = "openai"
        self._id = self._id or _str(_field(c, "id"))
        self._model = self._model or _str(_field(c, "model"))
        self._tier = self._tier or _str(_field(c, "service_tier"))
        usage = _obj(_field(c, "usage"))
        if usage is not None:
            self._usage = usage
        content = False
        for position, raw in enumerate(choices):
            choice = _obj(raw)
            if choice is None:
                continue
            at = _index(_field(choice, "index"), position)
            reason = _str(_field(choice, "finish_reason"))
            if reason is not None:
                self._finish[at] = reason
            if at != 0:
                continue
            delta = _obj(_field(choice, "delta"))
            if delta is None:
                continue
            text = _str(_field(delta, "content"))
            if text is not None:
                content = True
                self._text.append(self._take(text))
            calls = _list(_field(delta, "tool_calls"))
            for i, raw_call in enumerate(calls or ()):
                call = _obj(raw_call)
                if call is None:
                    continue
                content = True
                tool = self._tool(_index(_field(call, "index"), i))
                if tool is None:
                    continue
                fn = _obj(_field(call, "function"))
                tool.id = tool.id or _str(_field(call, "id"))
                tool.name = tool.name or _str(_field(fn, "name"))
                args = _str(_field(fn, "arguments"))
                if args is not None:
                    tool.arguments += self._take(args)
        return content

    def _anthropic(self, c: Any, kind: str) -> bool:
        if self.shape is None:
            self.shape = "anthropic"
        if kind == "message_start":
            message = _obj(_field(c, "message"))
            self._id = self._id or _str(_field(message, "id"))
            self._model = self._model or _str(_field(message, "model"))
            usage = _dict(_field(message, "usage"))
            if usage is not None:
                self._usage = usage
            return False
        if kind == "content_block_start":
            block = _obj(_field(c, "content_block"))
            if _field(block, "type") == "tool_use":
                tool = self._tool(_index(_field(c, "index"), len(self._tools)))
                if tool is not None:
                    tool.id = tool.id or _str(_field(block, "id"))
                    tool.name = tool.name or _str(_field(block, "name"))
                return True
            text = _str(_field(block, "text"))
            if text is None:
                return False
            self._text.append(self._take(text))
            return True
        if kind == "content_block_delta":
            delta = _obj(_field(c, "delta"))
            delta_type = _field(delta, "type")
            if delta_type == "text_delta":
                text = _str(_field(delta, "text"))
                if text is None:
                    return False
                self._text.append(self._take(text))
                return True
            if delta_type == "input_json_delta":
                args = _str(_field(delta, "partial_json"))
                if args is None:
                    return False
                tool = self._tools.get(_index(_field(c, "index"), -1))
                if tool is not None:
                    tool.arguments += self._take(args)
                return True
            return False
        # message_delta: the stop reason and the final usage, whose fields
        # replace the ones `message_start` reported.
        self._stop = _str(_field(_obj(_field(c, "delta")), "stop_reason")) or self._stop
        usage = _dict(_field(c, "usage"))
        if usage is not None:
            merged = dict(self._usage) if isinstance(self._usage, Mapping) else {}
            merged.update({k: v for k, v in usage.items() if v is not None})
            self._usage = merged
        return False

    def _tool(self, at: int) -> Optional[_ToolCall]:
        """The tool call at `at`, made on first sight, or None past the cap."""
        known = self._tools.get(at)
        if known is not None:
            return known
        if len(self._tools) >= STREAM_MAX_TOOL_CALLS:
            self.truncated = True
            return None
        tool = _ToolCall()
        self._tools[at] = tool
        return tool

    def _take(self, s: str) -> str:
        """As much of `s` as the budget still allows. A Python str counts code points."""
        if self._budget <= 0:
            self.truncated = True
            return ""
        if len(s) > self._budget:
            kept = s[: self._budget]
            self._budget = 0
            self.truncated = True
            return kept
        self._budget -= len(s)
        return s

    def response(self) -> Optional[Dict[str, Any]]:
        """The response the provider would have sent, or None when no chunk was recognised."""
        if self.shape == "openai":
            out: Dict[str, Any] = {"object": "chat.completion"}
            _put(out, "id", self._id)
            _put(out, "model", self._model)
            _put(out, "service_tier", self._tier)
            out["choices"] = [{"index": i, "finish_reason": r} for i, r in sorted(self._finish.items())]
            _put(out, "usage", self._usage)
            return out
        if self.shape == "anthropic":
            out = {"type": "message"}
            _put(out, "id", self._id)
            _put(out, "model", self._model)
            _put(out, "stop_reason", self._stop)
            _put(out, "usage", self._usage)
            return out
        return None

    def output(self) -> Any:
        """The text, or the text and the tool calls, or None when neither arrived."""
        text = "".join(self._text)
        if not self._tools:
            return text or None
        calls = []
        for _, tool in sorted(self._tools.items()):
            call: Dict[str, Any] = {}
            _put(call, "id", tool.id)
            _put(call, "name", tool.name)
            call["arguments"] = tool.arguments
            calls.append(call)
        return {"content": text, "tool_calls": calls}


def _put(out: Dict[str, Any], key: str, value: Any) -> None:
    if value is not None:
        out[key] = value
