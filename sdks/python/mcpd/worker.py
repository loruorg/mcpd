"""The worker: the user's own process, serving step bodies to the daemon.

It registers the workflows it holds, long-polls for leases, and for each lease
posts `started`, heartbeats while the body runs, and posts `complete` or
`fail`. Every 409 the daemon can answer is handled by its reason, because a
worker that collapses them into one branch abandons steps it has already
finished or reports a failure for a success.

Nothing here runs a model or reads a stored prompt. The daemon hands the worker
a lease with the rendered input and the prior steps; the body does the rest
with the imports, the pool and the credentials this process holds.
"""

from __future__ import annotations

import concurrent.futures
import contextvars
import dataclasses
import logging
import os
import secrets
import threading
from typing import Any, Callable, Dict, List, Optional, Set, TypeVar, Union

from ._generated import SpanKind, StepErrorClass, StepLease, StepUsage
from .client import Mcpd
from .context import TraceContext
from .queue import Throttle
from .runs import StepConflictError
from .span import Span
from .transport import McpdError
from .workflow import StepHandler, Workflow

log = logging.getLogger("mcpd.worker")


class StepFailure(Exception):
    """A failure with a class, raised from a step body to tell the daemon how to treat it."""

    def __init__(self, message: str, *, class_: Union[StepErrorClass, str] = StepErrorClass.FATAL,
                 retry_after_ms: Optional[int] = None, details: Any = None) -> None:
        super().__init__(message)
        self.class_ = StepErrorClass(class_) if not isinstance(class_, StepErrorClass) else class_
        self.retry_after_ms = retry_after_ms
        self.details = details


class RetryableError(StepFailure):
    """A transient failure: the daemon retries it while the step's budget lasts."""

    def __init__(self, message: str, *, retry_after_ms: Optional[int] = None, details: Any = None) -> None:
        super().__init__(message, class_=StepErrorClass.RETRYABLE, retry_after_ms=retry_after_ms,
                         details=details)


# The run's own fields a body span carries; project, environment and the
# service names come from this client. The same five in every SDK.
_LEASE_CORRELATION = ("session_id", "thread_id", "user_id", "request_id", "release")

#: The fields `set_usage` may set, and the order they are merged in.
_USAGE_FIELDS = ("input_tokens", "output_tokens", "total_tokens", "input_details",
                 "output_details", "model", "provider")


@dataclasses.dataclass(frozen=True)
class StepOutcome:
    """How a body finished, handed to `on_step_end`."""

    #: `completed` or `failed`.
    status: str
    #: The class the failure was reported with, when it failed.
    error_class: Optional[StepErrorClass] = None
    #: The message that went to the daemon, when it failed.
    message: Optional[str] = None


@dataclasses.dataclass(frozen=True)
class WorkerStats:
    """What a worker has done since it was built (contract section 7.2)."""

    #: Leases taken from `claim`.
    claimed: int
    #: Bodies running right now.
    inflight: int
    #: Bodies that returned and were reported complete.
    completed: int
    #: Bodies that failed, by error class.
    failed: Dict[str, int]
    #: 409s the worker applied, by the daemon's reason.
    conflicts: Dict[str, int]
    #: Leases refused before any body ran.
    unservable: int


class StepContext:
    """What a step body is handed besides its input and the prior steps.

    `cancelled` is set when the daemon fences this lease or ends the run, so a
    body that checks it stops when it should. `span` records a span under the
    step the daemon opened, so a model call inside the body lands in the run's
    trace at the right depth.
    """

    def __init__(self, mcpd: Mcpd, lease: StepLease) -> None:
        self._mcpd = mcpd
        #: The lease, in full.
        self.lease = lease
        #: Which run.
        self.run_id = lease.run_id
        #: Which step, with any loop or item index folded in.
        self.step_key = lease.step_key
        #: Which attempt, from zero.
        self.attempt = lease.attempt
        #: A key for a third party that accepts one. Stable across retries of this step.
        self.idempotency_key = lease.idempotency_key
        #: Set when the lease is lost or the run ends.
        self.cancelled = threading.Event()
        #: Why, once `cancelled` is set.
        self.cancel_reason: Optional[str] = None
        #: The step's span, which the daemon opened; record your own spans under it.
        self.parent = TraceContext(trace_id=lease.trace_id, span_id=lease.span_id, sampled=True)
        self._state: Optional[Dict[str, Any]] = None
        self._usage: Optional[StepUsage] = None
        # What the body's spans spent, summed as each one ends (section 7.3).
        self._usage_lock = threading.Lock()
        self._accumulated: Optional[StepUsage] = None

    @property
    def state(self) -> Any:
        """The run's accumulated state as of this lease."""
        return self.lease.state

    def input(self, decode: Callable[[Any], T]) -> T:
        """The step's rendered input, checked by `decode`.

        A decoder that raises fails the step `invalid_input`, never `fatal`:
        the input was rendered by the daemon from a template, so a shape the
        body did not expect is a template mistake, and one attempt is the
        right thing to charge for it. `decode` is any callable: a dataclass,
        a TypedDict-checking function, `Model.model_validate` from pydantic.
        """
        return _decode_or(self.lease.input, decode, "input")

    def prior(self, decode: Callable[[Any], T]) -> T:
        """The prior steps, keyed by id, checked by `decode`. Same rule as `input`."""
        return _decode_or(self.lease.prior if self.lease.prior is not None else {}, decode, "prior")

    def state_as(self, decode: Callable[[Any], T]) -> T:
        """The run's state, checked by `decode`. Same rule as `input`."""
        return _decode_or(self.lease.state, decode, "state")

    def set_state(self, patch: Optional[Dict[str, Any]] = None, **keys: Any) -> None:
        """Merge keys into the run's state when the step completes. Top-level keys replace."""
        merged = dict(self._state or {})
        merged.update(patch or {})
        merged.update(keys)
        self._state = merged

    def set_usage(self, usage: Optional[StepUsage] = None, **fields: Any) -> None:
        """Report the tokens this step spent, so the daemon prices and charges them.

        An explicit override: the fields set here replace what the body's
        spans added up. A body that records its model calls as spans through
        `span` never needs this.
        """
        current = self._usage or StepUsage()
        if usage is not None:
            for name in _USAGE_FIELDS:
                value = getattr(usage, name)
                if value is not None:
                    setattr(current, name, value)
        for name, value in fields.items():
            setattr(current, name, value)
        self._usage = current

    def span(self, name: str, kind: Union[SpanKind, str] = SpanKind.GENERIC, **fields: Any) -> Span:
        """A span nested under this step in the run's trace: `with ctx.span("model_call") as s:`.

        What the span spends, and what every span nested under it spends, is
        added to the step's usage when each one ends, so the daemon prices
        the step from what the trace shows.
        """
        correlation = self.lease.correlation
        for key in _LEASE_CORRELATION:
            value = getattr(correlation, key, None)
            # A key named with no value is not an instruction to erase it, so
            # the test is on the value and not on `key not in fields`. A step
            # span that lost its session cannot be found by session, by the
            # trace filters or by a subject erasure, and neither can any span
            # under it.
            if value is not None and fields.get(key) is None:
                fields[key] = value
        span = self._mcpd.span(name, kind, traceparent=self.parent.traceparent(), **fields)
        span._usage_sink = self._accumulate
        return span

    def _accumulate(self, span: Span) -> None:
        """Add one ended span's usage to the step's. Called by the span, once."""
        usage = span._usage
        if usage is None:
            return
        with self._usage_lock:
            total = self._accumulated or StepUsage()
            for name in ("input_tokens", "output_tokens", "total_tokens"):
                value = getattr(usage, name)
                if value is not None:
                    setattr(total, name, (getattr(total, name) or 0) + int(value))
            if span._model:
                total.model = span._model
            if span._provider:
                total.provider = span._provider
            self._accumulated = total

    def collected(self) -> "tuple[Optional[Dict[str, Any]], Optional[StepUsage]]":
        """What the body accumulated, read by the worker when it completes.

        The usage is what the spans added up, with anything `set_usage` set
        replacing it field by field.
        """
        with self._usage_lock:
            summed = self._accumulated
        if summed is None:
            return self._state, self._usage
        if self._usage is None:
            return self._state, summed
        merged = dataclasses.replace(summed)
        for name in _USAGE_FIELDS:
            value = getattr(self._usage, name)
            if value is not None:
                setattr(merged, name, value)
        return self._state, merged

    def _cancel(self, reason: str) -> None:
        self.cancel_reason = reason
        self.cancelled.set()


OnError = Callable[[str, BaseException], None]
OnLease = Callable[[StepLease], Any]
OnStepStart = Callable[[StepContext], Any]
OnStepEnd = Callable[[StepContext, StepOutcome], Any]
OnConflict = Callable[[StepLease, str], Any]

T = TypeVar("T")


def _decode_or(value: Any, decode: Callable[[Any], T], what: str) -> T:
    try:
        return decode(value)
    except Exception as exc:  # noqa: BLE001 - every decoder failure is the same class
        raise StepFailure(f"the step's {what} could not be read: {exc}", class_=StepErrorClass.INVALID_INPUT) from exc


def _default_worker_id() -> str:
    host = os.environ.get("HOSTNAME") or "worker"
    return f"{host}-{secrets.token_hex(4)}"


class Worker:
    """Serves step bodies from this process. See the module comment for the protocol it drives."""

    def __init__(self, mcpd: Mcpd, *, worker_id: Optional[str] = None, concurrency: int = 4,
                 project: Optional[str] = None, wait_ms: int = 20_000,
                 on_error: Optional[OnError] = None, on_lease: Optional[OnLease] = None,
                 on_step_start: Optional[OnStepStart] = None, on_step_end: Optional[OnStepEnd] = None,
                 on_conflict: Optional[OnConflict] = None) -> None:
        self._mcpd = mcpd
        self._worker_id = worker_id or _default_worker_id()
        self._concurrency = max(1, int(concurrency))
        self._project = project
        self._wait_ms = min(60_000, max(0, int(wait_ms)))
        self._user_on_error = on_error
        # One throttle per kind of failure, for the default reporter only.
        self._throttles: Dict[str, Throttle] = {}
        # The four hooks of section 7.2. Each is the user's code, wrapped: one
        # that raises is reported through `on_error` and the loop goes on.
        self._on_lease = on_lease
        self._on_step_start = on_step_start
        self._on_step_end = on_step_end
        self._on_conflict = on_conflict
        self._claimed = 0
        self._completed = 0
        self._failed: Dict[str, int] = {}
        self._conflicts: Dict[str, int] = {}
        self._unservable = 0
        self._workflows: List[Workflow] = []
        self._heartbeat_s = 10.0
        self._lock = threading.RLock()
        self._inflight: Set[concurrent.futures.Future] = set()
        self._pool: Optional[concurrent.futures.ThreadPoolExecutor] = None
        self._stopping = threading.Event()
        self._wake = threading.Event()
        self._slot = threading.Event()

    def register(self, *workflows: Workflow) -> "Worker":
        """The workflows this worker serves. Their definitions are posted by `serve`."""
        self._workflows.extend(workflows)
        return self

    @property
    def worker_id(self) -> str:
        """The name every lease records as its owner."""
        return self._worker_id

    @property
    def inflight(self) -> int:
        """Steps running right now."""
        with self._lock:
            return len(self._inflight)

    def stats(self) -> WorkerStats:
        """Leases claimed, bodies in flight, completed, failed by class, 409s by reason."""
        with self._lock:
            return WorkerStats(
                claimed=self._claimed,
                inflight=len(self._inflight),
                completed=self._completed,
                failed=dict(self._failed),
                conflicts=dict(self._conflicts),
                unservable=self._unservable,
            )

    def serve(self) -> None:
        """Register, then claim and serve until `stop()` is called.

        Blocks the calling thread. Returns once every step in flight has reported.
        """
        self._stopping.clear()
        self._wake.clear()
        self.register_definitions()
        while not self._stopping.is_set():
            free = self._concurrency - self.inflight
            if free <= 0:
                self._slot.wait(1.0)
                self._slot.clear()
                continue
            try:
                taken = self.claim(free)
            except Exception as exc:  # noqa: BLE001 - the loop must outlive one bad claim
                self._on_error("claim", f"mcpd worker {self._worker_id}: claim failed; retrying", exc)
                self._wake.wait(1.0)
                continue
            if taken == 0:
                self._wake.wait(0.01)
        self.drain()

    def stop(self) -> None:
        """Stop claiming and wait for the steps in flight."""
        self._stopping.set()
        self._wake.set()
        self._slot.set()
        self.drain()

    def register_definitions(self) -> None:
        """Post every registered definition and read back the lease timings."""
        registered = self._mcpd.steps.register(self._worker_id, [w.document() for w in self._workflows],
                                               project=self._project)
        self._heartbeat_s = max(1.0, registered.heartbeat_ms / 1000.0)

    def claim(self, max_leases: Optional[int] = None, run_id: Optional[str] = None) -> int:
        """One claim, serving what it granted. Returns how many leases were taken.

        `serve` calls this in a loop; a script that drives its own run calls it
        with that run's id until the run ends.
        """
        wanted = self._concurrency if max_leases is None else max_leases
        response = self._mcpd.steps.claim(self._worker_id, max=min(100, max(1, wanted)), wait_ms=self._wait_ms,
                                          run_id=run_id, project=self._project)
        for lease in response.leases:
            with self._lock:
                self._claimed += 1
            self._hook("on_lease", self._on_lease, lease)
            with self._lock:
                future = self._executor().submit(self._serve, lease)
                self._inflight.add(future)
                future.add_done_callback(self._done)
        return len(response.leases)

    def drain(self) -> None:
        """Wait for every step in flight."""
        while True:
            with self._lock:
                pending = list(self._inflight)
            if not pending:
                return
            concurrent.futures.wait(pending)

    def _executor(self) -> concurrent.futures.ThreadPoolExecutor:
        if self._pool is None:
            self._pool = concurrent.futures.ThreadPoolExecutor(
                max_workers=self._concurrency, thread_name_prefix=f"mcpd-worker-{self._worker_id}")
        return self._pool

    def _done(self, future: concurrent.futures.Future) -> None:
        with self._lock:
            self._inflight.discard(future)
        self._slot.set()

    def _handler_for(self, lease: StepLease) -> Optional[StepHandler]:
        for w in self._workflows:
            if w.id != lease.definition_id:
                continue
            if w.version is not None and w.version != lease.definition_version:
                continue
            handler = w.handler_for(lease.step_key)
            if handler is not None:
                return handler
        return None

    def _on_error(self, kind: str, message: str, exc: BaseException) -> None:
        """Report one of the worker's own failures.

        A caller's `on_error` sees every one. Without one, each kind of failure
        is logged at most once per interval. The kind is the key, never the
        message: a message names its step, and a key that did would let every
        step of a failing run through the limit.
        """
        if self._user_on_error is not None:
            self._user_on_error(message, exc)
            return
        with self._lock:
            throttle = self._throttles.setdefault(kind, Throttle())
        suppressed = throttle.check()
        if suppressed is not None:
            more = f" ({suppressed} more like it were not logged)" if suppressed else ""
            log.warning("%s: %s%s", message, exc, more)

    def _hook(self, name: str, hook: Optional[Callable[..., Any]], *args: Any) -> None:
        """Call one user hook, and never let it into the serve loop."""
        if hook is None:
            return
        try:
            hook(*args)
        except Exception as exc:  # noqa: BLE001 - the user's code, not the worker's
            self._on_error("hook", f"mcpd worker {self._worker_id}: the {name} hook raised", exc)

    def _count_failure(self, class_: Any) -> None:
        key = str(_plain(class_))
        with self._lock:
            self._failed[key] = self._failed.get(key, 0) + 1

    def _serve(self, lease: StepLease) -> None:
        steps = self._mcpd.steps
        handler = self._handler_for(lease)
        if handler is None:
            with self._lock:
                self._unservable += 1
            self._report(lease, lambda: steps.fail(
                lease.lease_id, class_=StepErrorClass.UNSERVABLE,
                message=(f"worker {self._worker_id} does not serve {lease.definition_id}"
                         f"@{lease.definition_version} step {lease.step_key}")))
            return

        # Before the body. Always. This is what tells the daemon the difference
        # between "offered" and "running", and on an at_most_once step it
        # refuses a complete without it.
        if self._report(lease, lambda: steps.started(lease.lease_id)) == "abandon":
            return

        ctx = StepContext(self._mcpd, lease)
        self._hook("on_step_start", self._on_step_start, ctx)
        stop_beat = threading.Event()
        # Set when the body returns or raises, or when the lease is given up at
        # its deadline or by a fence. Whichever comes first frees the slot.
        settled = threading.Event()

        def beat() -> None:
            while not stop_beat.wait(self._heartbeat_s):
                try:
                    steps.heartbeat(lease.lease_id)
                except StepConflictError as exc:
                    # The daemon has moved on without us; the body learns through `cancelled`.
                    ctx._cancel(f"{_plain(exc.conflict.reason)}: {exc.conflict.message}")
                    settled.set()
                    return
                except McpdError as exc:
                    # A transport failure is not evidence of anything; the next beat may land.
                    self._on_error("heartbeat", f"mcpd worker {self._worker_id}: heartbeat failed for {lease.step_key}", exc)

        beater = threading.Thread(target=beat, name=f"mcpd-heartbeat-{lease.lease_id}", daemon=True)
        beater.start()

        # The step's own ceiling, enforced here as well as by the daemon: a body
        # that runs past `deadline_ms` is cancelled through `cancelled` and the
        # attempt is failed retryable, so the pool's slot is freed and the
        # journal says why, instead of the daemon reclaiming a lease still
        # heartbeating from inside a body that will never return.
        deadline: Optional[threading.Timer] = None
        if lease.deadline_ms is not None and lease.deadline_ms > 0:
            reason = f"step deadline of {lease.deadline_ms}ms passed"

            def expire() -> None:
                if ctx.cancelled.is_set():
                    return
                ctx._cancel(reason)
                stop_beat.set()
                try:
                    self._report(lease, lambda: steps.fail(lease.lease_id, class_=StepErrorClass.RETRYABLE, message=reason))
                    self._count_failure(StepErrorClass.RETRYABLE)
                    self._hook("on_step_end", self._on_step_end, ctx,
                               StepOutcome("failed", StepErrorClass.RETRYABLE, reason))
                finally:
                    # The slot is held only until the daemon has heard why.
                    settled.set()

            deadline = threading.Timer(lease.deadline_ms / 1000.0, expire)
            deadline.daemon = True
            deadline.start()
        # The body runs on a thread of its own and the pool's thread waits for it
        # or for the lease to be given up. A body that ignores `cancelled`
        # cannot be stopped, which nothing in Python can do to a thread, but it
        # no longer holds a slot past its deadline as contract section 7.2
        # requires. What it returns or raises after that is ignored.
        outcome: Dict[str, Any] = {}
        prior = lease.prior if lease.prior is not None else {}
        context = contextvars.copy_context()

        def run_body() -> None:
            try:
                outcome["output"] = context.run(handler, ctx, lease.input, prior)
            except BaseException as exc:  # noqa: BLE001 - handed to the pool's thread below
                outcome["error"] = exc
            finally:
                settled.set()

        threading.Thread(target=run_body, name=f"mcpd-step-{lease.lease_id}", daemon=True).start()
        try:
            settled.wait()
            if ctx.cancelled.is_set():
                return
            try:
                if "error" in outcome:
                    raise outcome["error"]
                output = outcome.get("output")
            except Exception as exc:  # noqa: BLE001 - every failure of a body is reported, none is hidden
                if ctx.cancelled.is_set():
                    return
                failure = _to_failure(exc)
                message = str(failure) or "the step body raised"
                self._report(lease, lambda: steps.fail(
                    lease.lease_id, class_=failure.class_, message=message,
                    details=failure.details, retry_after_ms=failure.retry_after_ms))
                self._count_failure(failure.class_)
                self._hook("on_step_end", self._on_step_end, ctx,
                           StepOutcome("failed", failure.class_, message))
                return
            if ctx.cancelled.is_set():
                return
            state, usage = ctx.collected()
            self._report(lease, lambda: steps.complete(lease.lease_id, output=output, state=state, usage=usage))
            with self._lock:
                self._completed += 1
            # After the post, never before it: a hook must not sit on the
            # completion path.
            self._hook("on_step_end", self._on_step_end, ctx, StepOutcome("completed"))
        finally:
            stop_beat.set()
            if deadline is not None:
                deadline.cancel()

    def _report(self, lease: StepLease, call: Callable[[], Any]) -> Any:
        """Post one worker call and apply the 409 table.

        `already_applied` is a success, and every other reason means abandon
        this lease.
        """
        try:
            return call()
        except StepConflictError as exc:
            reason = str(_plain(exc.conflict.reason))
            with self._lock:
                self._conflicts[reason] = self._conflicts.get(reason, 0) + 1
            self._hook("on_conflict", self._on_conflict, lease, reason)
            if reason == "already_applied":
                return "done"
            if reason == "not_started":
                self._on_error(
                    "not_started",
                    f"mcpd worker {self._worker_id}: a bug in this worker: {lease.step_key} was reported before started",
                    exc)
            return "abandon"
        except McpdError as exc:
            self._on_error("call", f"mcpd worker {self._worker_id}: call failed for {lease.step_key}", exc)
            return "abandon"


def _plain(value: Any) -> Any:
    return getattr(value, "value", value)


def _to_failure(exc: BaseException) -> StepFailure:
    if isinstance(exc, StepFailure):
        return exc
    return StepFailure(str(exc) or exc.__class__.__name__)
