"""The authoring surface for durable workflows.

A workflow is data the daemon holds and bodies the daemon never sees. The
builder produces both from one file: `document()` is the definition the daemon
walks, in the shape `spec/definition.schema.json` describes, and the handlers
stay in this process for a `Worker` to serve. There is no YAML to keep in step
with the code, because the code is where the graph is written.

Every construct of the run contract has a method here. A branch, a foreach body
and a loop body are one claimable step each; a multi-step body is a
`subworkflow`.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from enum import Enum
from typing import TYPE_CHECKING, Any, Callable, Dict, List, Mapping, Optional, Union

from ._generated import Backoff, DefinitionDocument, DefinitionKind, Effect, OnError, WaitTimeout

if TYPE_CHECKING:  # pragma: no cover - typing only
    from .worker import StepContext

# --------------------------------------------------------------------------- #
# Predicates and templates
# --------------------------------------------------------------------------- #

#: A guard the daemon evaluates: a leaf, or `all`, `any`, `not` over leaves.
Predicate = Dict[str, Any]

#: A step body. Returns the step's output, or raises. A raised `StepFailure`
#: carries its own class; any other exception is `fatal`.
StepHandler = Callable[["StepContext", Any, Any], Any]

_MISSING = object()


def when(path: str, op: str, value: Any = _MISSING) -> Predicate:
    """A leaf predicate: `when("steps.order.output.amount_usd", "gt", 100)`.

    A path reads `input`, `steps.<id>`, `run`, and `item`, `index` or
    `iter.index` where they apply. `op` is one of eq, ne, lt, lte, gt, gte,
    in, exists, matches.
    """
    if value is _MISSING:
        return {"path": path, "op": op}
    return {"path": path, "op": op, "value": value}


def all_of(*predicates: Predicate) -> Predicate:
    """Every predicate must hold."""
    return {"all": list(predicates)}


def any_of(*predicates: Predicate) -> Predicate:
    """At least one predicate must hold."""
    return {"any": list(predicates)}


def not_(predicate: Predicate) -> Predicate:
    """The predicate must not hold."""
    return {"not": predicate}


@dataclass
class RetryPolicy:
    """How failures classed `retryable` are retried.

    `max_attempts` counts every attempt including the first, so the default of
    one means no retry.
    """

    max_attempts: Optional[int] = None
    backoff: Optional[Union[Backoff, str]] = None
    initial_ms: Optional[int] = None
    max_ms: Optional[int] = None

    def to_wire(self) -> Dict[str, Any]:
        out: Dict[str, Any] = {}
        if self.max_attempts is not None:
            out["max_attempts"] = self.max_attempts
        if self.backoff is not None:
            out["backoff"] = _plain(self.backoff)
        if self.initial_ms is not None:
            out["initial_ms"] = self.initial_ms
        if self.max_ms is not None:
            out["max_ms"] = self.max_ms
        return out


@dataclass
class Limits:
    """The ceilings a workflow declares."""

    max_steps: Optional[int] = None
    run_timeout_ms: Optional[int] = None
    max_cost_usd: Optional[float] = None

    def to_wire(self) -> Dict[str, Any]:
        out: Dict[str, Any] = {}
        if self.max_steps is not None:
            out["max_steps"] = self.max_steps
        if self.run_timeout_ms is not None:
            out["run_timeout_ms"] = self.run_timeout_ms
        if self.max_cost_usd is not None:
            out["max_cost_usd"] = self.max_cost_usd
        return out


# --------------------------------------------------------------------------- #
# The document
# --------------------------------------------------------------------------- #

_ID = re.compile(r"^[a-z][a-z0-9_]*$")


def _check_id(value: str) -> str:
    if not isinstance(value, str) or not _ID.match(value):
        raise ValueError(f'"{value}" is not a step id; an id matches ^[a-z][a-z0-9_]*$')
    return value


def _plain(value: Any) -> Any:
    return value.value if isinstance(value, Enum) else value


def _common(node: Dict[str, Any], when: Optional[Predicate], on_error: Optional[Union[OnError, str]]) -> Dict[str, Any]:
    if when is not None:
        node["when"] = when
    if on_error is not None:
        node["on_error"] = _plain(on_error)
    return node


def _claimable(kind: str, id: str, *, when: Optional[Predicate], input: Any, effect: Optional[Union[Effect, str]],
               step_timeout_ms: Optional[int], retry: Optional[Union[RetryPolicy, Mapping[str, Any]]],
               on_error: Optional[Union[OnError, str]]) -> Dict[str, Any]:
    node: Dict[str, Any] = _common({"kind": kind, "id": _check_id(id)}, when, on_error)
    if input is not None:
        node["input"] = input
    if effect is not None:
        node["effect"] = _plain(effect)
    if step_timeout_ms is not None:
        node["step_timeout_ms"] = step_timeout_ms
    if retry is not None:
        node["retry"] = retry.to_wire() if isinstance(retry, RetryPolicy) else dict(retry)
    return node


class BranchBuilder:
    """What a parallel branch, a foreach body or a loop body may hold: one claimable step."""

    def __init__(self) -> None:
        self.nodes: List[Dict[str, Any]] = []
        self.handlers: Dict[str, StepHandler] = {}

    def step(self, id: str, handler: Optional[StepHandler] = None, *, when: Optional[Predicate] = None,
             input: Any = None, effect: Optional[Union[Effect, str]] = None,
             step_timeout_ms: Optional[int] = None, retry: Optional[Union[RetryPolicy, Mapping[str, Any]]] = None,
             on_error: Optional[Union[OnError, str]] = None) -> Any:
        """A branch or body that a worker serves. Works as a decorator when no handler is given."""
        return self._add("task", id, handler, when=when, input=input, effect=effect,
                         step_timeout_ms=step_timeout_ms, retry=retry, on_error=on_error)

    def agent(self, id: str, handler: Optional[StepHandler] = None, *, when: Optional[Predicate] = None,
              input: Any = None, effect: Optional[Union[Effect, str]] = None,
              step_timeout_ms: Optional[int] = None, retry: Optional[Union[RetryPolicy, Mapping[str, Any]]] = None,
              on_error: Optional[Union[OnError, str]] = None) -> Any:
        """The same, recorded as an agent turn."""
        return self._add("agent", id, handler, when=when, input=input, effect=effect,
                         step_timeout_ms=step_timeout_ms, retry=retry, on_error=on_error)

    def _add(self, kind: str, id: str, handler: Optional[StepHandler], **options: Any) -> Any:
        if handler is None:
            def decorate(fn: StepHandler) -> StepHandler:
                self._add(kind, id, fn, **options)
                return fn
            return decorate
        self.nodes.append(_claimable(kind, id, **options))
        self.handlers[id] = handler
        return self


class Workflow:
    """A workflow under construction. Steps run in the order they are added."""

    def __init__(self, id: str, *, version: Optional[int] = None, input_schema: Any = None,
                 limits: Optional[Union[Limits, Mapping[str, Any]]] = None) -> None:
        self.id = _check_id(id)
        self.version = version
        self._input_schema = input_schema
        self._limits = limits
        self._nodes: List[Dict[str, Any]] = []
        self._handlers: Dict[str, StepHandler] = {}
        # The body id a foreach or loop root maps to, so a lease's key finds its handler.
        self._body_of: Dict[str, str] = {}
        self._output: Any = _MISSING

    def step(self, id: str, handler: Optional[StepHandler] = None, *, when: Optional[Predicate] = None,
             input: Any = None, effect: Optional[Union[Effect, str]] = None,
             step_timeout_ms: Optional[int] = None, retry: Optional[Union[RetryPolicy, Mapping[str, Any]]] = None,
             on_error: Optional[Union[OnError, str]] = None) -> Any:
        """A claimable step a worker serves.

        `handler(ctx, input, prior)` returns the step's output. Given no
        handler, this is a decorator: `@wf.step("classify")`.
        """
        return self._add("task", id, handler, when=when, input=input, effect=effect,
                         step_timeout_ms=step_timeout_ms, retry=retry, on_error=on_error)

    def agent(self, id: str, handler: Optional[StepHandler] = None, *, when: Optional[Predicate] = None,
              input: Any = None, effect: Optional[Union[Effect, str]] = None,
              step_timeout_ms: Optional[int] = None, retry: Optional[Union[RetryPolicy, Mapping[str, Any]]] = None,
              on_error: Optional[Union[OnError, str]] = None) -> Any:
        """A claimable step recorded as an agent turn: your own loop, however many turns, as one durable step."""
        return self._add("agent", id, handler, when=when, input=input, effect=effect,
                         step_timeout_ms=step_timeout_ms, retry=retry, on_error=on_error)

    def _add(self, kind: str, id: str, handler: Optional[StepHandler], **options: Any) -> Any:
        if handler is None:
            def decorate(fn: StepHandler) -> StepHandler:
                self._add(kind, id, fn, **options)
                return fn
            return decorate
        self._nodes.append(_claimable(kind, id, **options))
        self._handlers[id] = handler
        return self

    def sleep(self, id: str, ms: int, *, when: Optional[Predicate] = None,
              on_error: Optional[Union[OnError, str]] = None) -> "Workflow":
        """A delay. No process is alive during it."""
        self._nodes.append(_common({"kind": "sleep", "id": _check_id(id), "ms": ms}, when, on_error))
        return self

    def sleep_until(self, id: str, at: str, *, when: Optional[Predicate] = None,
                    on_error: Optional[Union[OnError, str]] = None) -> "Workflow":
        """A delay until an instant, rendered from a template such as `${input.billing_date}`."""
        self._nodes.append(_common({"kind": "sleep_until", "id": _check_id(id), "at": at}, when, on_error))
        return self

    def wait_for(self, id: str, *, event: str, key: str, timeout_ms: int, accept_parked: Optional[bool] = None,
                 on_timeout: Optional[Union[WaitTimeout, str]] = None, when: Optional[Predicate] = None,
                 on_error: Optional[Union[OnError, str]] = None) -> "Workflow":
        """A pause until an event arrives.

        The daemon holds the deadline and the mailbox; the key is rendered per
        run, so `${input.order_id}` addresses this run and no other.
        """
        node = _common({"kind": "wait_event", "id": _check_id(id), "event": event, "key": key,
                        "timeout_ms": timeout_ms}, when, on_error)
        if accept_parked is not None:
            node["accept_parked"] = accept_parked
        if on_timeout is not None:
            node["on_timeout"] = _plain(on_timeout)
        self._nodes.append(node)
        return self

    def parallel(self, id: str, build: Callable[[BranchBuilder], Any], *, when: Optional[Predicate] = None,
                 on_error: Optional[Union[OnError, str]] = None) -> "Workflow":
        """Named branches, each one claimable step, all on disk before any is offered."""
        b = BranchBuilder()
        build(b)
        self._nodes.append(_common({"kind": "parallel", "id": _check_id(id), "branches": b.nodes}, when, on_error))
        self._handlers.update(b.handlers)
        return self

    def foreach(self, id: str, build: Callable[[BranchBuilder], Any], *, over: str, max_items: int,
                concurrency: Optional[int] = None, when: Optional[Predicate] = None,
                on_error: Optional[Union[OnError, str]] = None) -> "Workflow":
        """One claimable step per element of the array at `over`.

        The body's input template can read `item` and `index`; afterwards
        `steps.<id>.outputs` is the array of outputs in item order.
        """
        body, handlers = self._one_body(id, build, "foreach")
        node = _common({"kind": "foreach", "id": _check_id(id), "over": over, "body": body,
                        "max_items": max_items}, when, on_error)
        if concurrency is not None:
            node["concurrency"] = concurrency
        self._nodes.append(node)
        self._adopt(id, handlers)
        return self

    def loop(self, id: str, build: Callable[[BranchBuilder], Any], *, max_iterations: int,
             until: Optional[Predicate] = None, when: Optional[Predicate] = None,
             on_error: Optional[Union[OnError, str]] = None) -> "Workflow":
        """A bounded loop.

        `until` is evaluated after each iteration and reads `steps.<body id>`
        for that iteration; `iter.index` is the iteration number.
        """
        body, handlers = self._one_body(id, build, "loop")
        node = _common({"kind": "loop", "id": _check_id(id), "body": body,
                        "max_iterations": max_iterations}, when, on_error)
        if until is not None:
            node["until"] = until
        self._nodes.append(node)
        self._adopt(id, handlers)
        return self

    def subworkflow(self, id: str, *, definition: str, version: Optional[int] = None, input: Any = None,
                    when: Optional[Predicate] = None,
                    on_error: Optional[Union[OnError, str]] = None) -> "Workflow":
        """A child run of another definition. Its output becomes `steps.<id>.output`."""
        node = _common({"kind": "subworkflow", "id": _check_id(id), "definition": definition}, when, on_error)
        if version is not None:
            node["version"] = version
        if input is not None:
            node["input"] = input
        self._nodes.append(node)
        return self

    def output(self, template: Any) -> "Workflow":
        """The template rendered when the run reaches the end of the graph."""
        self._output = template
        return self

    def document(self) -> DefinitionDocument:
        """The definition document the daemon stores and walks."""
        body: Dict[str, Any] = {"steps": list(self._nodes)}
        if self._input_schema is not None:
            body["input_schema"] = self._input_schema
        if self._limits is not None:
            body["limits"] = (self._limits.to_wire() if isinstance(self._limits, Limits)
                              else dict(self._limits))
        if self._output is not _MISSING:
            body["output"] = self._output
        return DefinitionDocument(id=self.id, body=body, kind=DefinitionKind.WORKFLOW, version=self.version)

    def handler_for(self, step_key: str) -> Optional[StepHandler]:
        """The handler a lease's step key addresses.

        `order`, `checks/fraud`, `notify[3]` and `refine[2]/rewrite` each
        resolve to the lexical id that registered the body.
        """
        head, slash, sub = step_key.partition("/")
        root = head.split("[", 1)[0]
        if slash:
            return self._handlers.get(sub)
        if root != head:
            return self._handlers.get(self._body_of.get(root, root))
        return self._handlers.get(root)

    def _one_body(self, id: str, build: Callable[[BranchBuilder], Any], what: str):
        b = BranchBuilder()
        build(b)
        if len(b.nodes) != 1:
            raise ValueError(f'{what} "{id}" needs exactly one body step; use a subworkflow for more')
        return b.nodes[0], b.handlers

    def _adopt(self, id: str, handlers: Dict[str, StepHandler]) -> None:
        for body_id, handler in handlers.items():
            self._handlers[body_id] = handler
            self._body_of[id] = body_id


def workflow(id: str, *, version: Optional[int] = None, input_schema: Any = None,
             limits: Optional[Union[Limits, Mapping[str, Any]]] = None) -> Workflow:
    """Start building a workflow."""
    return Workflow(id, version=version, input_schema=input_schema, limits=limits)
