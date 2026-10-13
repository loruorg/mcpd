"""Model calls: what an OpenAI- or Anthropic-shaped response says about the
call that produced it, mapped onto mcpd's usage record (SDK contract 3.1).

Nothing here imports a provider SDK. A response is read through mapping access
or plain attributes, and a provider SDK's pydantic model is dumped to a dict
first, so the provider's own object, a parsed JSON body and a hand-built test
fixture all work. A field that is missing or of the wrong type is left absent
rather than raised on.
"""

from __future__ import annotations

import dataclasses
import math
from typing import Any, Dict, List, Mapping, Optional

from . import _generated as records

__all__ = [
    "ResponseFacts",
    "response_facts",
    "response_shape",
    "usage_from_anthropic",
    "usage_from_openai",
]

_SCALARS = (str, bytes, bytearray, int, float, bool, list, tuple, set, frozenset)


def _obj(value: Any) -> Any:
    """The value as something with named fields, or None when it has none.

    A pydantic model is dumped rather than read by attribute, because its
    `model_dump` is the one view that carries every field the provider sent,
    including the ones the installed SDK version does not declare.
    """
    if value is None or isinstance(value, _SCALARS):
        return None
    if isinstance(value, Mapping):
        return value
    dump = getattr(value, "model_dump", None)
    if callable(dump):
        try:
            dumped = dump()
        except Exception:  # noqa: BLE001 - an odd model is read by attribute instead
            dumped = None
        if isinstance(dumped, Mapping):
            return dumped
    return value


def _field(obj: Any, key: str) -> Any:
    if obj is None:
        return None
    if isinstance(obj, Mapping):
        return obj.get(key)
    try:
        return getattr(obj, key, None)
    except Exception:  # noqa: BLE001 - a property that raises is a field that is absent
        return None


def _has(obj: Any, key: str) -> bool:
    if obj is None:
        return False
    if isinstance(obj, Mapping):
        return key in obj
    try:
        return hasattr(obj, key)
    except Exception:  # noqa: BLE001
        return False


def _int(value: Any) -> Optional[int]:
    # A bool is an int in Python, and `True` tokens is not a count.
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return None
    if isinstance(value, float) and not math.isfinite(value):
        return None
    return int(math.floor(value)) if value >= 0 else None


def _str(value: Any) -> Optional[str]:
    return value if isinstance(value, str) and value else None


def _compact(cls: Any, **fields: Any) -> Any:
    """`cls(**fields)` without the absent ones, or None when nothing is left."""
    present = {k: v for k, v in fields.items() if v is not None}
    return cls(**present) if present else None


def usage_from_openai(response: Any) -> Optional[records.TokenUsage]:
    """Token usage from an OpenAI chat-completions or responses-API response.

    The prompt total is gross, so the plain text share is the total less the
    cached and audio tokens.
    """
    usage = _obj(_field(_obj(response), "usage"))
    if usage is None:
        return None
    input_tokens = _int(_field(usage, "prompt_tokens"))
    if input_tokens is None:
        input_tokens = _int(_field(usage, "input_tokens"))
    output_tokens = _int(_field(usage, "completion_tokens"))
    if output_tokens is None:
        output_tokens = _int(_field(usage, "output_tokens"))
    in_detail = _obj(_field(usage, "prompt_tokens_details"))
    if in_detail is None:
        in_detail = _obj(_field(usage, "input_tokens_details"))
    out_detail = _obj(_field(usage, "completion_tokens_details"))
    if out_detail is None:
        out_detail = _obj(_field(usage, "output_tokens_details"))
    cached = _int(_field(in_detail, "cached_tokens"))
    audio_in = _int(_field(in_detail, "audio_tokens"))
    text = None
    if input_tokens is not None and (cached is not None or audio_in is not None):
        text = max(0, input_tokens - (cached or 0) - (audio_in or 0))
    total = _int(_field(usage, "total_tokens"))
    if total is None and input_tokens is not None and output_tokens is not None:
        total = input_tokens + output_tokens
    return _compact(
        records.TokenUsage,
        input_tokens=input_tokens,
        output_tokens=output_tokens,
        total_tokens=total,
        input_details=_compact(records.InputTokenDetails, text=text, cache_read=cached,
                               audio=audio_in),
        output_details=_compact(records.OutputTokenDetails,
                                reasoning=_int(_field(out_detail, "reasoning_tokens")),
                                audio=_int(_field(out_detail, "audio_tokens"))),
    )


def usage_from_anthropic(response: Any) -> Optional[records.TokenUsage]:
    """Token usage from an Anthropic messages response.

    `input_tokens` there is net of the cache, so it is the text share, and the
    prompt's size is the sum of it and both cache meters.
    """
    usage = _obj(_field(_obj(response), "usage"))
    if usage is None:
        return None
    text = _int(_field(usage, "input_tokens"))
    read = _int(_field(usage, "cache_read_input_tokens"))
    write = _int(_field(usage, "cache_creation_input_tokens"))
    split = _obj(_field(usage, "cache_creation"))
    output_tokens = _int(_field(usage, "output_tokens"))
    input_tokens = None
    if text is not None or read is not None or write is not None:
        input_tokens = (text or 0) + (read or 0) + (write or 0)
    total = None
    if input_tokens is not None and output_tokens is not None:
        total = input_tokens + output_tokens
    return _compact(
        records.TokenUsage,
        input_tokens=input_tokens,
        output_tokens=output_tokens,
        total_tokens=total,
        input_details=_compact(records.InputTokenDetails, text=text, cache_read=read,
                               cache_write=write,
                               cache_write_5m=_int(_field(split, "ephemeral_5m_input_tokens")),
                               cache_write_1h=_int(_field(split, "ephemeral_1h_input_tokens"))),
    )


def response_shape(response: Any) -> Optional[str]:
    """Which provider shaped this response: `openai`, `anthropic`, or None."""
    r = _obj(response)
    if r is None:
        return None
    usage = _obj(_field(r, "usage"))
    if _field(r, "type") == "message" or _has(usage, "cache_read_input_tokens"):
        return "anthropic"
    if _has(usage, "prompt_tokens") or _has(usage, "input_tokens_details"):
        return "openai"
    if isinstance(_field(r, "choices"), (list, tuple)) or _field(r, "object") == "response":
        return "openai"
    if _has(usage, "input_tokens"):
        return "anthropic"
    return None


@dataclasses.dataclass
class ResponseFacts:
    """What `Span.record_response` sets on a span."""

    model: Optional[str] = None
    provider: Optional[str] = None
    usage: Optional[records.TokenUsage] = None
    attributes: Dict[str, Any] = dataclasses.field(default_factory=dict)


def response_facts(response: Any, provider: Optional[str] = None) -> ResponseFacts:
    """Everything a response says about its call: the model, the provider, the
    usage, and the id, finish reasons and service tier as `gen_ai.response.*`
    attributes. Never the text: that stays the caller's to record or not.
    """
    r = _obj(response)
    shape = response_shape(response)
    attributes: Dict[str, Any] = {}
    response_id = _str(_field(r, "id"))
    if response_id is not None:
        attributes["gen_ai.response.id"] = response_id
    reasons: List[str] = []
    choices = _field(r, "choices")
    if isinstance(choices, (list, tuple)):
        for choice in choices:
            reason = _str(_field(_obj(choice), "finish_reason"))
            if reason is not None:
                reasons.append(reason)
    stop = _str(_field(r, "stop_reason"))
    if stop is not None:
        reasons.append(stop)
    if not reasons and _field(r, "object") == "response":
        status = _str(_field(r, "status"))
        if status is not None:
            reasons.append(status)
    if reasons:
        attributes["gen_ai.response.finish_reasons"] = reasons
    tier = _str(_field(r, "service_tier")) or _str(_field(_obj(_field(r, "usage")), "service_tier"))
    if tier is not None:
        attributes["gen_ai.response.service_tier"] = tier
    if shape == "anthropic":
        usage = usage_from_anthropic(response)
    elif shape == "openai":
        usage = usage_from_openai(response)
    else:
        usage = None
    return ResponseFacts(
        model=_str(_field(r, "model")),
        provider=provider if provider is not None else shape,
        usage=usage,
        attributes=attributes,
    )
