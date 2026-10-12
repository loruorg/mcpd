"""The bounded export queue.

B3 of the SDK contract: an unbounded buffer turns a network outage into an
out-of-memory kill. This queue has a hard ceiling, drops the oldest entry when
it is reached, counts what it dropped, and says so at most once per interval.

It also keys entries, which is what makes B8 affordable. A span is exported when
it starts and again when it ends; if the end arrives before the worker has sent
the start, the second write replaces the first in place and the span costs one
record instead of two.
"""

from __future__ import annotations

import threading
from collections import deque
from time import monotonic
from typing import Any, Deque, Dict, Hashable, List, Optional, Tuple

__all__ = ["BoundedQueue", "Throttle"]


class Throttle:
    """Allows one caller through per interval, and counts the rest.

    Used for both noisy contract rules: B1's "logged at most once per interval"
    and B3's dropped-span warning. A failing exporter must not be able to fill
    the logs of the application it is observing.
    """

    def __init__(self, interval: float = 60.0) -> None:
        self._interval = interval
        self._lock = threading.Lock()
        self._last = 0.0
        self._suppressed = 0

    def check(self) -> Optional[int]:
        """Return the number of suppressed calls when allowed through, else None.

        The count is included so a single line can say "and 412 more".
        """
        with self._lock:
            now = monotonic()
            if self._last and now - self._last < self._interval:
                self._suppressed += 1
                return None
            suppressed, self._suppressed, self._last = self._suppressed, 0, now
            return suppressed


class _Entry:
    """A queued signal. Mutable so a keyed entry can be updated in place."""

    __slots__ = ("key", "signal", "payload")

    def __init__(self, key: Optional[Hashable], signal: str, payload: Any) -> None:
        self.key = key
        self.signal = signal
        self.payload = payload


class BoundedQueue:
    """A thread-safe FIFO with a hard ceiling and drop-oldest overflow."""

    def __init__(self, max_size: int) -> None:
        if max_size < 1:
            raise ValueError("max_queue must be at least 1")
        self._max = max_size
        self._lock = threading.Lock()
        self._items: Deque[_Entry] = deque()
        self._index: Dict[Hashable, _Entry] = {}
        self._dropped = 0

    @property
    def max_size(self) -> int:
        """The ceiling this queue was built with."""
        return self._max

    @property
    def dropped(self) -> int:
        """How many signals have been discarded for want of room, ever."""
        with self._lock:
            return self._dropped

    def __len__(self) -> int:
        with self._lock:
            return len(self._items)

    def put(self, signal: str, payload: Any, key: Optional[Hashable] = None) -> bool:
        """Queue one signal. Returns False when something had to be dropped.

        Never raises and never blocks: the calling code is an agent doing real
        work, and it must not wait on us.
        """
        with self._lock:
            if key is not None:
                existing = self._index.get(key)
                if existing is not None:
                    # The same span, updated. Replacing keeps its queue position,
                    # so ordering still reflects when the work started.
                    existing.payload = payload
                    return True
            dropped = False
            while len(self._items) >= self._max:
                oldest = self._items.popleft()
                if oldest.key is not None:
                    self._index.pop(oldest.key, None)
                self._dropped += 1
                dropped = True
            entry = _Entry(key, signal, payload)
            self._items.append(entry)
            if key is not None:
                self._index[key] = entry
            return not dropped

    def drain(self, limit: int) -> List[Tuple[str, Any]]:
        """Remove and return up to `limit` signals, oldest first."""
        out: List[Tuple[str, Any]] = []
        with self._lock:
            while self._items and len(out) < limit:
                entry = self._items.popleft()
                if entry.key is not None:
                    self._index.pop(entry.key, None)
                out.append((entry.signal, entry.payload))
        return out

    def clear(self) -> None:
        """Discard everything queued, without counting it as an overflow drop."""
        with self._lock:
            self._items.clear()
            self._index.clear()
