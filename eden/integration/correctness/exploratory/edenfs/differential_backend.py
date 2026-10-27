# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Paired dispatch of shared actions to the EdenFS backend and the plain reference.

``DifferentialBackend`` wraps any primary backend, LiveBackend or a subclass
such as a lifecycle backend, without changing it. The scheduler invokes it
once per action. An action that ``expects_reference`` starts on both sides
from that one invocation: each side waits at one start barrier, the
primary runs in the scheduler's worker and the reference beside it, and the
action completes only after both return. The reference side becomes the
``reference_result`` fact of the primary's single terminal event, keyed by
the same action ID. Every other action, faults and nemeses included, runs
on the primary alone.

A reference side without a trustworthy observation (a spawn failure, a
timeout, a process that outlives its side, a broken barrier, or a malformed
reference result) is a reference fact, never a product one: the primary
side's result is kept as the action's terminal, and its ``reference_result``
is an unresolved (``info``) side whose ``reason`` names the typed failure.
The failure is also recorded in the run's ``ReferenceRuntime``. From then
on the reference checkout's state is unknown, so every later shared action
runs on the primary alone and carries an ``info`` reference side naming the
first failure; nothing compares against a diverged reference. A primary
side that raises still raises. The paired call is bounded at three quarters
of the operation deadline, so a stuck reference is never reported as the
primary's operation timeout.
"""

from __future__ import annotations

import sys
import threading
import time
from collections.abc import Callable
from typing import Any, cast

from eden.integration.correctness.exploratory.core.differential import (
    REFERENCE_RESULT,
    SideResult,
)
from eden.integration.correctness.exploratory.core.differential_schedule import (
    reference_facts,
)
from eden.integration.correctness.exploratory.core.history import HistorySink, INFO
from eden.integration.correctness.exploratory.core.runspec import Action, RunSpec
from eden.integration.correctness.exploratory.core.scheduler import (
    Backend,
    BackendResult,
    Clock,
    HarnessFailure,
    INVALID_BACKEND_RESPONSE,
    OUTCOMES,
)
from eden.integration.correctness.exploratory.edenfs.actions import expects_reference
from eden.integration.correctness.exploratory.edenfs.reference import (
    ReferenceBackend,
    ReferenceFailure,
    ReferenceRuntime,
)
from eden.integration.correctness.exploratory.edenfs.sapling_processes import (
    REFERENCE_TIMEOUT,
)

BARRIER = "barrier"
EXCEPTION = "exception"
MALFORMED = "malformed"
DIVERGED = "reference-diverged"
_MAX_REASON_CHARS = 1024
_BARRIER_SECONDS = 10.0
# Fractions of the operation deadline: the start barrier and the whole
# paired call stay inside it, so a stuck reference side is a typed reference
# failure, never the primary's operation timeout.
_BARRIER_FRACTION = 0.1
_JOIN_FRACTION = 0.75


class _ReferenceCall:
    """One reference side running beside its primary, started at the shared barrier."""

    def __init__(
        self,
        reference: Callable[[Action], BackendResult],
        action: Action,
        barrier: threading.Barrier,
        clock: Clock,
    ) -> None:
        self.result: BackendResult | None = None
        self.started: float | None = None
        self.completed: float | None = None
        self.error: Exception | None = None
        self._thread = threading.Thread(
            target=self._run,
            args=(reference, action, barrier, clock),
            daemon=True,
            name=f"reference-{action.id}",
        )
        self._thread.start()

    def _run(
        self,
        reference: Callable[[Action], BackendResult],
        action: Action,
        barrier: threading.Barrier,
        clock: Clock,
    ) -> None:
        try:
            barrier.wait()
            self.started = clock()
            self.result = reference(action)
            self.completed = clock()
        except Exception as error:  # noqa: BLE001 - classified by the caller
            self.error = error

    def join(self, until: float) -> bool:
        """Wait for the reference side until ``until``; return whether it finished."""
        self._thread.join(timeout=max(0.0, until - time.monotonic()))
        return not self._thread.is_alive()


def _side_error(error: Exception) -> tuple[str, str]:
    """Return the typed kind and detail of a reference side's error."""
    if isinstance(error, ReferenceFailure):
        return error.kind, error.detail
    return EXCEPTION, f"{type(error).__name__}: {error}"


def _returned(primary: object) -> dict[str, Any]:
    """Return what the primary side returned, whole: its outcome and facts."""
    if isinstance(primary, BackendResult):
        return {"outcome": primary.outcome, "facts": dict(primary.facts)}
    return {"returned": type(primary).__name__}


def _raised(error: Exception) -> dict[str, Any]:
    """Return what the primary side raised."""
    return {"error": f"{type(error).__name__}: {error}"}


class DifferentialBackend:
    """Run shared actions on the primary and the reference as one paired action."""

    def __init__(
        self,
        primary: Backend,
        reference: ReferenceBackend,
        spec: RunSpec,
        *,
        runtime: ReferenceRuntime | None = None,
        clock: Clock = time.monotonic,
    ) -> None:
        operation = spec.budgets.operation_deadline_seconds
        self._primary = primary
        self._reference = reference
        self._spec = spec
        self._runtime = ReferenceRuntime() if runtime is None else runtime
        self._clock = clock
        self._barrier_seconds = min(_BARRIER_SECONDS, operation * _BARRIER_FRACTION)
        self._join_seconds = operation * _JOIN_FRACTION
        self._published: list[dict[str, Any]] = []
        self._lock = threading.Lock()
        self._diverged: str | None = None

    @property
    def primary(self) -> Backend:
        """Return the wrapped primary backend."""
        return self._primary

    @property
    def reference(self) -> ReferenceBackend:
        """Return the reference backend."""
        return self._reference

    @property
    def diverged(self) -> str | None:
        """Return the first reference failure, once the reference stopped pairing."""
        with self._lock:
            return self._diverged

    def __call__(self, action: Action) -> BackendResult:
        """Run a shared action on both sides, anything else on the primary alone."""
        if not expects_reference(self._spec, action):
            return self._primary(action)
        diverged = self.diverged
        if diverged is not None:
            primary = self._checked_primary(action, self._primary(action))
            now = self._clock()
            return self._unresolved(primary, f"{DIVERGED}: {diverged}", now, now)
        return self._paired_call(action)

    def _paired_call(self, action: Action) -> BackendResult:
        """Start both sides at one barrier and wait for the reference within the join bound."""
        until = time.monotonic() + self._join_seconds
        barrier = threading.Barrier(2, timeout=self._barrier_seconds)
        side = _ReferenceCall(self._reference, action, barrier, self._clock)
        try:
            barrier.wait()
        except threading.BrokenBarrierError:
            side.join(until)
            primary = self._checked_primary(action, self._primary(action))
            reason = self._failure(action, BARRIER, "start barrier broke")
            now = self._clock()
            return self._unresolved(primary, reason, now, now)
        try:
            primary = self._primary(action)
        except Exception as error:
            finished = side.join(until)
            self._record_side(action, side, finished, _raised(error))
            raise
        finished = side.join(until)
        primary = self._checked_primary(action, primary)
        if not finished:
            reason = self._failure(
                action, REFERENCE_TIMEOUT, self._overrun(), _returned(primary)
            )
            return self._side_unresolved(primary, side, reason)
        return self._pair(action, primary, side)

    def _checked_primary(self, action: Action, primary: object) -> BackendResult:
        """Return the primary's result, or raise: a malformed primary is the harness's."""
        if not isinstance(primary, BackendResult):
            raise self._harness(action, "the primary returned no backend result")
        if REFERENCE_RESULT in primary.facts or "action" in primary.facts:
            raise self._harness(action, "primary facts shadow the pair")
        return primary

    def _harness(self, action: Action, detail: str) -> HarnessFailure:
        print(f"differential: {detail} on {action.id}", file=sys.stderr, flush=True)
        return HarnessFailure(INVALID_BACKEND_RESPONSE)

    def _unresolved(
        self, primary: BackendResult, reason: str, started: float, completed: float
    ) -> BackendResult:
        """Return the primary's result carrying an unresolved reference side."""
        side = SideResult(
            INFO,
            {"reason": reason[:_MAX_REASON_CHARS]},
            started,
            max(started, completed),
        )
        return BackendResult(
            primary.outcome, {**primary.facts, **reference_facts(side)}
        )

    def _side_unresolved(
        self, primary: BackendResult, side: _ReferenceCall, reason: str
    ) -> BackendResult:
        now = self._clock()
        started = now if side.started is None else side.started
        return self._unresolved(primary, reason, started, now)

    def _overrun(self) -> str:
        return f"reference side still running after {self._join_seconds:.1f}s"

    def _record_side(
        self,
        action: Action,
        side: _ReferenceCall,
        finished: bool,
        evidence: dict[str, Any],
    ) -> None:
        """Record a failed reference side beside a primary that raised; never raises."""
        if not finished:
            self._failure(action, REFERENCE_TIMEOUT, self._overrun(), evidence)
        elif side.error is not None:
            self._failure(action, *_side_error(side.error), evidence)

    def _pair(
        self, action: Action, primary: BackendResult, side: _ReferenceCall
    ) -> BackendResult:
        """Merge the reference side into the primary's facts, or leave it unresolved."""
        evidence = _returned(primary)
        if side.error is not None:
            kind, detail = _side_error(side.error)
            reason = self._failure(action, kind, detail, evidence)
            return self._side_unresolved(primary, side, reason)
        reference = side.result
        if (
            not isinstance(reference, BackendResult)
            or reference.outcome not in OUTCOMES
            or side.started is None
            or side.completed is None
        ):
            reason = self._failure(
                action, MALFORMED, "the reference returned no backend result", evidence
            )
            return self._side_unresolved(primary, side, reason)
        try:
            paired = reference_facts(
                SideResult(
                    reference.outcome,
                    dict(reference.facts),
                    side.started,
                    side.completed,
                )
            )
        except ValueError as error:
            reason = self._failure(action, MALFORMED, str(error), evidence)
            return self._side_unresolved(primary, side, reason)
        self._runtime.paired()
        return BackendResult(primary.outcome, {**primary.facts, **paired})

    def _failure(
        self,
        action: Action,
        kind: str,
        detail: str,
        primary: dict[str, Any] | None = None,
    ) -> str:
        """Record one typed reference failure, stop pairing, and return its reason."""
        self._runtime.failed(action.id, kind, detail, primary)
        reason = f"{kind}: {detail}"
        with self._lock:
            if self._diverged is None:
                self._diverged = f"{action.id} {reason}"
        print(
            f"differential: reference {kind} on {action.id}: {detail}",
            file=sys.stderr,
            flush=True,
        )
        return reason

    def runtime_facts(self) -> dict[str, Any]:
        """Return the paired count and every typed reference failure so far."""
        return self._runtime.snapshot()

    def unwind_faults(self, sink: HistorySink, clock: Clock) -> dict[str, Any]:
        """Unwind the primary's faults and add the reference runtime to its setup facts.

        Workers may still be running here; ``close_resources`` refreshes the
        same runtime record after they join.
        """
        primary = cast(Any, self._primary)
        unwind = getattr(primary, "unwind_faults", None) or primary.unwind
        result: dict[str, Any] = dict(unwind(sink, clock))
        setup = dict(result.get("setup") or {})
        runtime = self.runtime_facts()
        setup["reference"] = {**dict(setup.get("reference") or {}), "runtime": runtime}
        result["setup"] = setup
        self._published.append(runtime)
        return result

    def close_resources(self, deadline: float | None = None) -> int:
        """Close the primary's resources, reap every reference process, then refresh the record."""
        leaks = 0
        try:
            close = getattr(self._primary, "close_resources", None)
            if close is not None:
                leaks += close(deadline)
        finally:
            try:
                leaks += self._reference.close_resources(deadline)
            finally:
                final = self.runtime_facts()
                for published in self._published:
                    published.clear()
                    published.update(final)
        return leaks

    def unwind(self, sink: HistorySink, clock: Clock) -> dict[str, Any]:
        """Unwind faults and close resources for callers without staged cleanup."""
        result = self.unwind_faults(sink, clock)
        result["leaks"] = result.get("leaks", 0) + self.close_resources()
        return result
