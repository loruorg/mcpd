"""The run surface of the daemon, as a client: definitions, runs, events, and
the worker protocol `Worker` drives.

These calls raise on failure, like the read API, because a run you asked for
and did not get is news. A worker call refused with a 409 raises a
`StepConflictError` carrying the daemon's closed reason, which is what lets a
worker tell a fence from its own landed call.
"""

from __future__ import annotations

import json
import time
from dataclasses import dataclass
from typing import Any, Callable, Iterator, List, Mapping, Optional, Sequence, TypeVar, Union

from ._generated import (
    ClaimRequest,
    ClaimResponse,
    Correlation,
    Definition,
    DefinitionDocument,
    DefinitionVersion,
    EventReceipt,
    LeaseState,
    PublishEventRequest,
    Run,
    RunDetail,
    RunEvent,
    RunEventPage,
    RunListResponse,
    RunStatus,
    StartRunRequest,
    StepAck,
    StepConflict,
    StepError,
    StepErrorClass,
    StepFailure as StepFailureBody,
    StepResult,
    StepUsage,
    WorkerRegistered,
    WorkerRegistration,
)
from .transport import McpdError, Transport, TransportError, decode, encode_params
from .workflow import Workflow

T = TypeVar("T")


class RunFailedError(McpdError):
    """A run that ended without succeeding, raised by `wait` with the run attached."""

    def __init__(self, run: Run) -> None:
        detail = f": {run.error}" if run.error else ""
        super().__init__(f"run {run.run_id} {_plain(run.status)}{detail}")
        #: `failed`, `timed_out` or `cancelled`.
        self.run_status = run.status
        #: The run as it ended.
        self.run = run


@dataclass
class RunResult:
    """What `wait` answers: the run as it ended, with its output to hand."""

    #: Always `succeeded`; any other ending raises.
    status: RunStatus
    #: What the definition's `output` template rendered.
    output: Any
    #: The priced usage the steps reported, in USD.
    cost_usd: float
    #: How many attempts the run spent.
    steps_used: int
    #: The trace every step of the run landed in.
    trace_id: str
    #: The run in full.
    run: Run


class StepConflictError(McpdError):
    """A worker call the daemon refused with one of the four closed reasons."""

    def __init__(self, conflict: StepConflict, cause: TransportError) -> None:
        super().__init__(f"{_plain(conflict.reason)}: {conflict.message}")
        #: The daemon's answer in full.
        self.conflict = conflict
        self.status = 409
        self.body = cause.body


def _plain(value: Any) -> Any:
    return getattr(value, "value", value)


def _typed(call: Callable[[], T]) -> T:
    """Run a worker call, raising a 409 as a typed conflict and anything else unchanged."""
    try:
        return call()
    except TransportError as exc:
        if exc.status == 409 and exc.body:
            try:
                parsed = json.loads(exc.body)
            except ValueError:
                raise exc
            if isinstance(parsed, dict) and "reason" in parsed:
                raise StepConflictError(decode(StepConflict, parsed), exc) from exc
        raise


def _definition_id(definition: Union[str, Workflow]) -> str:
    return definition.id if isinstance(definition, Workflow) else definition


class DefinitionsApi:
    """Definitions: the immutable, versioned documents the daemon walks."""

    def __init__(self, transport: Transport, project: str) -> None:
        self._transport = transport
        self._project = project

    def put(self, doc: Union[DefinitionDocument, Workflow], *, project: Optional[str] = None) -> DefinitionVersion:
        """Store one version of a definition, and read the version back."""
        if isinstance(doc, Workflow):
            doc = doc.document()
        query = encode_params({"project": project or self._project})
        return decode(DefinitionVersion, self._transport.post(f"/v1/api/definitions?{query}", doc))

    def get(self, id: str, *, version: Optional[int] = None, project: Optional[str] = None) -> Definition:
        """One definition: the latest version, unless `version` names one."""
        return decode(Definition, self._transport.get(
            f"/v1/api/definitions/{id}", {"project": project or self._project, "version": version}))

    def list(self, *, id: Optional[str] = None, project: Optional[str] = None) -> List[Definition]:
        """Every stored version in the project, or every version of one id."""
        raw = self._transport.get("/v1/api/definitions", {"project": project or self._project, "id": id})
        return [decode(Definition, item) for item in (raw or [])]


class RunsApi:
    """Runs: start, read, watch, cancel, signal."""

    def __init__(self, transport: Transport, project: str) -> None:
        self._transport = transport
        self._project = project

    def start(self, definition: Union[str, Workflow], *, version: Optional[int] = None, input: Any = None,
              correlation: Optional[Mapping[str, Any]] = None, idempotency_key: Optional[str] = None,
              project: Optional[str] = None) -> Run:
        """Start a run. The response is the run as stored, with its trace id.

        `idempotency_key` is unique per project while held: a retry of this
        call returns the same run.
        """
        project = project or self._project
        request = StartRunRequest(
            definition_id=_definition_id(definition),
            project=project,
            version=version,
            input=input,
            correlation=Correlation(**{"project": project, **dict(correlation)}) if correlation is not None else None,
            idempotency_key=idempotency_key,
        )
        return decode(Run, self._transport.post("/v1/api/runs", request))

    def get(self, run_id: str) -> RunDetail:
        """One run and everything durable about it: its journal, deadlines and waits."""
        return decode(RunDetail, self._transport.get(f"/v1/api/runs/{run_id}"))

    def list(self, *, status: Optional[Union[RunStatus, str]] = None, definition_id: Optional[str] = None,
             limit: Optional[int] = None, cursor: Optional[str] = None,
             project: Optional[str] = None) -> RunListResponse:
        """Runs in the project, most recently updated first."""
        return decode(RunListResponse, self._transport.get("/v1/api/runs", {
            "project": project or self._project, "status": status, "definition_id": definition_id,
            "limit": limit, "cursor": cursor,
        }))

    def cancel(self, run_id: str) -> Run:
        """End a run and every child run under it. Cancelling an ended run changes nothing."""
        return decode(Run, self._transport.post(f"/v1/api/runs/{run_id}/cancel", {}))

    def events(self, run_id: str, *, after: Optional[int] = None, limit: Optional[int] = None) -> RunEventPage:
        """A page of the run's journal, from `after` exclusive."""
        return decode(RunEventPage, self._transport.get(
            f"/v1/api/runs/{run_id}/events", {"after": after, "limit": limit}))

    def watch(self, run_id: str, *, after: int = -1, poll: float = 0.5) -> Iterator[RunEvent]:
        """Every event of a run as it happens, from `after` exclusive, until the run ends.

        A client that stops and starts again passes the last `seq` it saw and
        misses nothing that was committed.
        """
        while True:
            page = self.events(run_id, after=after, limit=500)
            ended = False
            for event in page.events:
                after = max(after, int(event.seq))
                yield event
                if _plain(event.kind) == "run_ended":
                    ended = True
            if ended:
                return
            if page.has_more:
                continue
            time.sleep(poll)

    def wait(self, run_id: str, *, after: int = -1, timeout: Optional[float] = None,
             poll: float = 0.5) -> RunResult:
        """Wait for a run to end and return it with its output.

        A run that ended `failed`, `timed_out` or `cancelled` raises
        `RunFailedError` carrying the run; a run still going after `timeout`
        seconds raises `TimeoutError`.
        """
        deadline = None if timeout is None else time.monotonic() + timeout
        while True:
            page = self.events(run_id, after=after, limit=500)
            ended = False
            for event in page.events:
                after = max(after, int(event.seq))
                if _plain(event.kind) == "run_ended":
                    ended = True
            if ended:
                break
            if page.has_more:
                continue
            if deadline is not None and time.monotonic() >= deadline:
                raise TimeoutError(f"run {run_id} is still running after {timeout}s")
            time.sleep(poll if deadline is None else max(0.0, min(poll, deadline - time.monotonic())))
        run = self.get(run_id).run
        if _plain(run.status) != "succeeded":
            raise RunFailedError(run)
        return RunResult(status=run.status, output=run.output, cost_usd=run.cost_usd,
                         steps_used=run.steps_used, trace_id=run.trace_id, run=run)

    def start_and_wait(self, definition: Union[str, Workflow], *, timeout: Optional[float] = None,
                       poll: float = 0.5, **start: Any) -> RunResult:
        """`start`, then `wait`. Keyword arguments other than `timeout` and `poll` go to `start`."""
        run = self.start(definition, **start)
        return self.wait(run.run_id, timeout=timeout, poll=poll)

    def signal(self, run_id: str, event: str, *, payload: Any = None,
               event_id: Optional[str] = None) -> EventReceipt:
        """Deliver an event to one run through its own open wait."""
        body = {"event": event}
        if payload is not None:
            body["payload"] = payload
        if event_id is not None:
            body["event_id"] = event_id
        return decode(EventReceipt, self._transport.post(f"/v1/api/runs/{run_id}/signal", body))


class EventsApi:
    """Events: the door a webhook or an approval console posts to."""

    def __init__(self, transport: Transport, project: str) -> None:
        self._transport = transport
        self._project = project

    def publish(self, event: str, key: str, *, payload: Any = None, event_id: Optional[str] = None,
                project: Optional[str] = None) -> EventReceipt:
        """Route an event by name and key to the run waiting for it.

        Send your own `event_id` from any handler that can be retried, such as
        a webhook: a repeat of a supplied id is answered `duplicate` and writes
        nothing.
        """
        request = PublishEventRequest(event=event, key=key, project=project or self._project,
                                      payload=payload, event_id=event_id)
        return decode(EventReceipt, self._transport.post("/v1/api/events", request))


class StepsApi:
    """The worker protocol, call for call. `Worker` drives it; a bare script can too."""

    def __init__(self, transport: Transport, project: str) -> None:
        self._transport = transport
        self._project = project

    def register(self, worker_id: str, definitions: Sequence[Union[DefinitionDocument, Workflow]], *,
                 project: Optional[str] = None) -> WorkerRegistered:
        """Announce a worker and store what it serves."""
        docs = [d.document() if isinstance(d, Workflow) else d for d in definitions]
        request = WorkerRegistration(worker_id=worker_id, definitions=docs, project=project or self._project)
        return decode(WorkerRegistered, self._transport.post("/v1/api/workers/register", request))

    def claim(self, worker_id: str, *, max: int, wait_ms: int, run_id: Optional[str] = None,
              project: Optional[str] = None) -> ClaimResponse:
        """Offer up to `max` runnable steps, waiting up to `wait_ms` for one."""
        request = ClaimRequest(worker_id=worker_id, max=max, wait_ms=wait_ms, run_id=run_id,
                               project=project or self._project)
        return decode(ClaimResponse, self._transport.post("/v1/api/steps/claim", request, hold=wait_ms / 1000.0))

    def started(self, lease_id: str) -> LeaseState:
        """Declare that the body is running. Required under `at_most_once`."""
        return _typed(lambda: decode(LeaseState, self._transport.post(f"/v1/api/steps/{lease_id}/started", {})))

    def heartbeat(self, lease_id: str) -> LeaseState:
        """Extend the lease. The only evidence the daemon has that a worker is alive."""
        return _typed(lambda: decode(LeaseState, self._transport.post(f"/v1/api/steps/{lease_id}/heartbeat", {})))

    def complete(self, lease_id: str, *, output: Any = None, state: Any = None,
                 usage: Optional[StepUsage] = None) -> StepAck:
        """Finish the step."""
        result = StepResult(output=output, state=state, usage=usage)
        return _typed(lambda: decode(StepAck, self._transport.post(f"/v1/api/steps/{lease_id}/complete", result)))

    def fail(self, lease_id: str, *, class_: Union[StepErrorClass, str], message: str, details: Any = None,
             retry_after_ms: Optional[int] = None) -> StepAck:
        """Fail the step, with the class that decides whether it is retried."""
        error = StepError(class_=StepErrorClass(class_) if not isinstance(class_, StepErrorClass) else class_,
                          message=message, details=details, retry_after_ms=retry_after_ms)
        body = StepFailureBody(error=error)
        return _typed(lambda: decode(StepAck, self._transport.post(f"/v1/api/steps/{lease_id}/fail", body)))
