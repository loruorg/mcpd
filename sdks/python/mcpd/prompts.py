"""Prompts: the read cache, the fallback, and rendering (contract section 5.1).

The client owns the network calls. This module holds what has no I/O in it, so
rendering works on a prompt that came from anywhere, including a fallback built
while the daemon was down.
"""

from __future__ import annotations

import copy
import threading
import time
from collections import OrderedDict
from datetime import datetime, timezone
from typing import Any, Callable, Dict, Hashable, List, Mapping, Optional, Sequence, Tuple, Union

from ._generated import Prompt, PromptKind

__all__ = ["PromptRenderError", "render_prompt", "variables_in"]

#: B3: the cache is bounded like everything else the SDK holds in memory.
CACHE_SIZE = 256


class PromptRenderError(ValueError):
    """A template names variables the caller did not give.

    `missing` lists every one of them, in order of first use, so one failed
    render tells the caller everything to fix rather than one name per run.
    """

    def __init__(self, missing: Sequence[str]) -> None:
        self.missing: List[str] = list(missing)
        names = ", ".join(self.missing)
        noun = "variable" if len(self.missing) == 1 else "variables"
        super().__init__(f"the prompt needs the {noun} {names}, which were not given")


def _is_name(text: str) -> bool:
    # The daemon's rule, character for character: an ASCII letter or `_`
    # first, then ASCII letters, digits, `_` and `.`. `str.isalpha` would
    # accept non-ASCII letters the daemon does not.
    if not text:
        return False
    first = text[0]
    if not (first == "_" or (first.isascii() and first.isalpha())):
        return False
    return all(c in "_." or (c.isascii() and c.isalnum()) for c in text)


def _scan(template: str, on_name: Callable[[str, str], str]) -> str:
    """Walk the placeholders of `template` once, as the daemon's `variables_in` does.

    `on_name` gets each valid name and the placeholder as written, and returns
    what to put in its place. Anything that is not a placeholder is copied.
    """
    out: List[str] = []
    rest = template
    while True:
        open_at = rest.find("{{")
        if open_at < 0:
            break
        after = rest[open_at + 2:]
        close = after.find("}}")
        if close < 0:
            break
        raw = rest[open_at:open_at + 2 + close + 2]
        name = after[:close].strip()
        out.append(rest[:open_at])
        out.append(on_name(name, raw) if _is_name(name) else raw)
        rest = after[close + 2:]
    out.append(rest)
    return "".join(out)


def variables_in(template: str, out: Optional[List[str]] = None) -> List[str]:
    """Every variable `template` names, in order of first use, without duplicates."""
    found: List[str] = [] if out is None else out

    def note(name: str, raw: str) -> str:
        if name not in found:
            found.append(name)
        return raw

    _scan(template, note)
    return found


def _variables_in_value(value: Any, out: List[str]) -> None:
    # The daemon reads every string in a chat body, not only `content`, so a
    # fallback has to as well or its `variables` would disagree with a fetch.
    if isinstance(value, str):
        variables_in(value, out)
    elif isinstance(value, (list, tuple)):
        for item in value:
            _variables_in_value(item, out)
    elif isinstance(value, Mapping):
        for item in value.values():
            _variables_in_value(item, out)


def _render(template: str, variables: Mapping[str, Any], missing: List[str]) -> str:
    def substitute(name: str, raw: str) -> str:
        if name in variables:
            value = variables[name]
            return value if isinstance(value, str) else str(value)
        if name not in missing:
            missing.append(name)
        return raw

    return _scan(template, substitute)


def render_prompt(prompt: Prompt, variables: Optional[Mapping[str, Any]] = None
                  ) -> Union[str, List[Any]]:
    """Fill a prompt's `{{variables}}`.

    A `text` prompt becomes a string. A `chat` prompt becomes a copy of its
    messages in which every string `content` is filled and everything else is
    kept as it was. A variable the template names and `variables` lacks raises
    `PromptRenderError`; extra variables are ignored.
    """
    values: Mapping[str, Any] = variables if variables is not None else {}
    missing: List[str] = []
    if prompt.kind == PromptKind.CHAT:
        rendered: List[Any] = []
        for message in prompt.messages or []:
            # A deep copy, so a caller who edits the result does not edit the
            # cached prompt every later call is served from.
            message = copy.deepcopy(message)
            if isinstance(message, dict) and isinstance(message.get("content"), str):
                message["content"] = _render(message["content"], values, missing)
            rendered.append(message)
        if missing:
            raise PromptRenderError(missing)
        return rendered
    text = _render(prompt.template or "", values, missing)
    if missing:
        raise PromptRenderError(missing)
    return text


def fallback_prompt(name: str, project: str, fallback: Union[str, Sequence[Any]]) -> Prompt:
    """The `Prompt` a fallback stands for: version 0, no labels, variables computed."""
    variables: List[str] = []
    if isinstance(fallback, str):
        variables_in(fallback, variables)
        return Prompt(project=project, name=name, version=0, kind=PromptKind.TEXT,
                      created_at=datetime.now(timezone.utc), variables=variables, labels=[],
                      template=fallback)
    messages = copy.deepcopy(list(fallback))
    _variables_in_value(messages, variables)
    return Prompt(project=project, name=name, version=0, kind=PromptKind.CHAT,
                  created_at=datetime.now(timezone.utc), variables=variables, labels=[],
                  messages=messages)


class PromptCache:
    """Fetched prompts by (project, name, selector), oldest evicted first.

    Thread-safe because one client is shared by every thread of a service. The
    lock covers the map only; a fetch happens outside it, so a slow daemon
    holds up the caller who asked and nobody else.
    """

    def __init__(self, size: int = CACHE_SIZE, clock: Callable[[], float] = time.monotonic) -> None:
        self.size = size
        #: Monotonic, so a wall-clock step cannot make a copy fresh for ever.
        self.clock = clock
        self._lock = threading.Lock()
        self._entries: "OrderedDict[Hashable, Tuple[float, Prompt]]" = OrderedDict()
        # Keys whose last fetch failed and has been reported, so an outage
        # costs one warning per prompt rather than one per call.
        self._reported: Dict[Hashable, bool] = {}

    def get(self, key: Hashable) -> Optional[Tuple[float, Prompt]]:
        with self._lock:
            return self._entries.get(key)

    def put(self, key: Hashable, prompt: Prompt) -> None:
        with self._lock:
            self._entries.pop(key, None)
            self._entries[key] = (self.clock(), prompt)
            while len(self._entries) > self.size:
                old, _ = self._entries.popitem(last=False)
                self._reported.pop(old, None)
            self._reported.pop(key, None)

    def first_failure(self, key: Hashable) -> bool:
        """True the first time a key's fetch fails since its last success."""
        with self._lock:
            if self._reported.get(key):
                return False
            self._reported[key] = True
            return True

    def __len__(self) -> int:
        with self._lock:
            return len(self._entries)
