"""The experiment runner (SDK contract 5.2).

The daemon never calls the agent under test. This does, in your process, once
per dataset case inside a root span that names the experiment and the case,
and then asks the daemon to score what was recorded.

A plain function runs on a fixed set of threads and a coroutine function on a
fixed set of asyncio workers. Either way at most `concurrency` cases are in
flight, and each case runs in a copy of the caller's context with its span made
current, so the spans the target opens are children of the case's run.
"""

from __future__ import annotations

import asyncio
import contextvars
import dataclasses
import inspect
import threading
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
from typing import TYPE_CHECKING, Any, Awaitable, Callable, Dict, List, Optional, Sequence, Union

from ._generated import (
    Correlation,
    DatasetItem,
    Experiment,
    ExperimentResults,
    ExperimentStatus,
    ScorerConfig,
    SpanKind,
)
from .transport import McpdError

if TYPE_CHECKING:
    from .client import Mcpd

__all__ = ["DATASET_ITEM_ATTRIBUTE", "ExperimentTarget"]

#: The attribute the daemon reads to find the case a run answered.
DATASET_ITEM_ATTRIBUTE = "mcpd.dataset_item_id"

#: The most cases the daemon answers in one page.
MAX_CASES = 5000

#: The code under test: called with each case's input, and what it returns is the answer.
ExperimentTarget = Callable[[Any], Union[Any, Awaitable[Any]]]

Config = Optional[Union[ScorerConfig, Dict[str, Any]]]


@dataclasses.dataclass
class _Plan:
    dataset_id: str
    target: ExperimentTarget
    name: str
    scorers: List[str]
    config: Config
    concurrency: int
    metadata: Any


def is_async_target(target: Any) -> bool:
    """Whether calling `target` gives a coroutine, judged before it is called."""
    if inspect.iscoroutinefunction(target):
        return True
    call = getattr(target, "__call__", None)
    return call is not None and inspect.iscoroutinefunction(call)


def plan(client: "Mcpd", dataset_id: str, target: ExperimentTarget, name: Optional[str],
         scorers: Optional[Sequence[str]], config: Config, concurrency: int,
         metadata: Any) -> _Plan:
    """The checks that refuse a run before its first request."""
    if not client.enabled:
        raise McpdError("run_experiment needs an enabled client: no run would reach the daemon "
                        "to be scored. Build the client with enabled=True, or unset "
                        "MCPD_ENABLED=false.")
    if isinstance(concurrency, bool) or not isinstance(concurrency, int) or concurrency < 1:
        raise ValueError(f"run_experiment concurrency must be a whole number of at least 1, "
                         f"got {concurrency!r}")
    if not callable(target):
        raise TypeError(f"run_experiment target must be a function, got {type(target).__name__}")
    return _Plan(dataset_id=dataset_id, target=target, name=name or dataset_id,
                 scorers=list(scorers or []), config=config, concurrency=concurrency,
                 metadata=metadata)


def _now() -> datetime:
    return datetime.now(timezone.utc)


def _draft(client: "Mcpd", p: _Plan) -> Experiment:
    return Experiment(
        experiment_id="", name=p.name, dataset_id=p.dataset_id,
        status=ExperimentStatus.RUNNING, item_count=0,
        correlation=Correlation(project=client.project), started_at=_now(),
        metadata=p.metadata,
    )


def _finish(client: "Mcpd", p: _Plan, experiment: Experiment, cases: int) -> ExperimentResults:
    """Flush, score, read the results and mark the experiment succeeded."""
    if not client.flush():
        raise McpdError("the runs did not reach the daemon before the flush deadline, so none "
                        "was scored. Check that the daemon is reachable, then run the "
                        "experiment again.")
    client.score_experiment(experiment.experiment_id, p.scorers, p.config)
    results = client.experiment_results(experiment.experiment_id)
    summary = {scorer.scorer_id: scorer.mean for scorer in results.scorers}
    client.create_experiment(dataclasses.replace(
        experiment, status=ExperimentStatus.SUCCEEDED, finished_at=_now(), item_count=cases,
        summary=summary,
    ))
    return results


def _record_failure(client: "Mcpd", experiment: Optional[Experiment],
                    error: BaseException) -> None:
    """Mark the experiment failed. A failure here must never hide the original error."""
    if experiment is None:
        return
    try:
        client.create_experiment(dataclasses.replace(
            experiment, status=ExperimentStatus.FAILED, finished_at=_now(),
            error=str(error) or type(error).__name__,
        ))
    except Exception as exc:  # noqa: BLE001 - reported, then the original error is raised
        client._instrumentation_failed("run_experiment could not record the failed status", exc)


def _open_case(client: "Mcpd", experiment: Experiment, item: DatasetItem) -> Any:
    return client._start(
        name=experiment.name, kind=SpanKind.AGENT_RUN, root=True,
        # A sampled-out case would be a case with no answer.
        always_sample=True,
        input=item.input,
        attributes={DATASET_ITEM_ATTRIBUTE: item.item_id},
        experiment_id=experiment.experiment_id,
    )


def _settle(result: Any) -> Any:
    """A plain function that returned an awaitable is driven to its answer on this thread."""
    if not inspect.isawaitable(result):
        return result

    async def wait() -> Any:
        return await result

    return asyncio.run(wait())


def run_sync(client: "Mcpd", p: _Plan) -> ExperimentResults:
    experiment: Optional[Experiment] = None
    try:
        experiment = client.create_experiment(_draft(client, p))
        items = client.list_dataset_items(p.dataset_id, limit=MAX_CASES)
        running = experiment
        lock = threading.Lock()
        position = [0]

        def take() -> Optional[DatasetItem]:
            with lock:
                if position[0] >= len(items):
                    return None
                position[0] += 1
                return items[position[0] - 1]

        def worker() -> None:
            # One worker per thread, each pulling the next case, so at most
            # `concurrency` targets are in flight however long the dataset is.
            item = take()
            while item is not None:
                span = _open_case(client, running, item)
                try:
                    with span:
                        output = _settle(p.target(item.input))
                        if output is not None:
                            span.set_output(output)
                except Exception:  # noqa: BLE001 - recorded on the span; the next case runs
                    pass
                item = take()

        workers = min(p.concurrency, len(items))
        if workers:
            with ThreadPoolExecutor(max_workers=workers,
                                    thread_name_prefix="mcpd-experiment") as pool:
                # A context per thread: one context cannot be entered by two
                # threads at once, and each must see the caller's variables.
                futures = [pool.submit(contextvars.copy_context().run, worker)
                           for _ in range(workers)]
                for future in futures:
                    future.result()
        return _finish(client, p, running, len(items))
    except BaseException as exc:
        _record_failure(client, experiment, exc)
        raise


async def run_async(client: "Mcpd", p: _Plan) -> ExperimentResults:
    loop = asyncio.get_running_loop()

    async def blocking(fn: Callable[..., Any], *args: Any) -> Any:
        # The reads and writes block on HTTP, and the event loop must not.
        return await loop.run_in_executor(None, lambda: fn(*args))

    experiment: Optional[Experiment] = None
    try:
        experiment = await blocking(client.create_experiment, _draft(client, p))
        items: List[DatasetItem] = await blocking(
            lambda: client.list_dataset_items(p.dataset_id, limit=MAX_CASES))
        running = experiment
        position = 0
        target_is_async = is_async_target(p.target)

        async def call(item: DatasetItem) -> Any:
            if target_is_async:
                return await p.target(item.input)
            # A plain function goes to a thread, in a copy of this context so
            # the case's span is still the current one there.
            result = await loop.run_in_executor(
                None, contextvars.copy_context().run, p.target, item.input)
            if inspect.isawaitable(result):
                result = await result
            return result

        async def worker() -> None:
            nonlocal position
            while position < len(items):
                item = items[position]
                position += 1
                span = _open_case(client, running, item)
                try:
                    with span:
                        output = await call(item)
                        if output is not None:
                            span.set_output(output)
                except Exception:  # noqa: BLE001 - recorded on the span; the next case runs
                    pass

        workers = min(p.concurrency, len(items))
        if workers:
            await asyncio.gather(*(worker() for _ in range(workers)))
        return await blocking(_finish, client, p, running, len(items))
    except BaseException as exc:
        await blocking(_record_failure, client, experiment, exc)
        raise
