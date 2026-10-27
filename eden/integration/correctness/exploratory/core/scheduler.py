# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Thread-per-actor scheduler for exploratory RunSpecs.

Each action emits exactly one completion, plus one invoke when it is
invoked; never-invoked actions complete as skipped. Actors share the runner
deadline minus the cleanup reservation, measured from the supervisor spawn
deadline. Sweeping atomically stops further invocation and marks in-flight
actions info with a harness failure; fault unwind then always runs. Backend
calls still running are left to the supervisor, which kills the run.
"""

from __future__ import annotations

import sys
import threading
import time
import traceback
from collections.abc import Callable
from dataclasses import dataclass, field
from typing import Any

from eden.integration.correctness.exploratory.core.history import (
    Event,
    event_line,
    FAIL,
    HistorySink,
    INFO,
    INVOKE,
    OK,
    SKIPPED,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    DEADLINE_RUN,
    FINISHED,
    RunSpec,
    STARTED,
)


OUTCOMES = frozenset({OK, FAIL, INFO})

COMPLETED = "completed"
HARNESS_FAILURE = "harness-failure"

BROKEN_HISTORY_TRANSPORT = "broken-history-transport"
INVALID_BACKEND_RESPONSE = "invalid-backend-response"
FAULT_WITNESS_FAILED = "fault-witness-failed"
SCHEDULER_INVARIANT = "scheduler-invariant"
OPERATION_TIMEOUT = "operation-timeout"
RUNNER_DEADLINE = "runner-deadline"

HARNESS_REASONS = frozenset(
    {
        BROKEN_HISTORY_TRANSPORT,
        INVALID_BACKEND_RESPONSE,
        FAULT_WITNESS_FAILED,
        SCHEDULER_INVARIANT,
        OPERATION_TIMEOUT,
        RUNNER_DEADLINE,
    }
)

_GATE_POLL_SECONDS = 0.05
# Event fields the scheduler owns; backend facts may not carry them.
RESERVED_FACT_KEYS = frozenset({"action", "actor", "type", "time"})
# Bound on the `Type: message` text an unexpected backend exception leaves in
# the execution manifest; the full traceback goes to stderr.log.
MAX_BACKEND_ERROR_BYTES = 512


@dataclass(frozen=True)
class BackendResult:
    """One backend response: a closed outcome plus observation facts."""

    outcome: str
    facts: dict[str, Any] = field(default_factory=dict)


@dataclass(frozen=True)
class ExecutionStatus:
    """Terminal scheduler state: completed or stopped with a reason."""

    status: str
    reason: str | None = None
    # `Type: message` of the first backend exception that is not a declared
    # HarnessFailure, at most MAX_BACKEND_ERROR_BYTES of UTF-8.
    error: str | None = None


def bounded_error(text: str) -> str:
    """Return `text` cut to at most MAX_BACKEND_ERROR_BYTES of UTF-8."""
    encoded = text.encode("utf-8", errors="replace")[:MAX_BACKEND_ERROR_BYTES]
    return encoded.decode("utf-8", errors="ignore")


@dataclass(frozen=True)
class HarnessFailure(Exception):
    """A backend hook to stop admission with a harness-failure reason."""

    reason: str


class _Swept(Exception):
    """The sweep began before the backend call started; the sweep completes it."""


Backend = Callable[[Action], BackendResult]
Clock = Callable[[], float]
Unwind = Callable[[], None]


class _GateTable:
    """started/finished gate table with stop-aware waits."""

    def __init__(self) -> None:
        self._cond = threading.Condition()
        self._seen: dict[str, set[str]] = {}

    def mark(self, action_id: str, event: str) -> None:
        with self._cond:
            self._seen.setdefault(action_id, set()).add(event)
            self._cond.notify_all()

    def wait(self, action_id: str, event: str, stop: threading.Event) -> bool:
        """Block until (action_id, event) is marked; False if stop fires."""
        with self._cond:
            while event not in self._seen.get(action_id, ()):
                if stop.is_set():
                    return False
                self._cond.wait(timeout=_GATE_POLL_SECONDS)
            return True


class _Run:
    """Shared state for one run_actions execution across actor threads."""

    def __init__(
        self,
        spec: RunSpec,
        backend: Backend,
        sink: HistorySink,
        clock: Clock,
        actor_deadline: float,
    ) -> None:
        self._spec = spec
        self._backend = backend
        self._sink = sink
        self._clock = clock
        self._actor_deadline = actor_deadline
        self._stop = threading.Event()
        self._state_lock = threading.Lock()
        self._reason: str | None = None
        self._error: str | None = None
        self._table = _GateTable()
        self._sink_lock = threading.Lock()
        self._sink_broken = False
        # Guards _completed, _invoked, and _sweeping together: an invoke and
        # the sweep decide under it, so no action is invoked once swept.
        self._completed: set[str] = set()
        self._completed_lock = threading.Lock()
        self._invoked: set[str] = set()
        self._sweeping = False
        self._owners = {
            action_id: actor.id for actor in spec.actors for action_id in actor.actions
        }
        self._by_id = {action.id: action for action in spec.actions}

    def stop_with(self, reason: str) -> None:
        """Set the stop flag, keeping only the first failure reason."""
        with self._state_lock:
            if self._reason is None:
                self._reason = reason
            self._stop.set()

    def emit(self, event: Event) -> None:
        """Append to the sink; the first sink failure stops the run and drops later events."""
        with self._sink_lock:
            if self._sink_broken:
                return
            try:
                self._sink.emit(event)
            except Exception:  # noqa: BLE001 - any sink failure breaks the transport
                self._sink_broken = True
                traceback.print_exc(file=sys.stderr)
                self.stop_with(BROKEN_HISTORY_TRANSPORT)

    def complete(self, action_id: str, outcome: str, facts: dict[str, Any]) -> None:
        with self._completed_lock:
            if action_id in self._completed:
                return
            self._completed.add(action_id)
        # Emit the history record before publishing FINISHED: a dependent
        # gated on finished must never invoke before this completion lands
        # in history, or the durable prefix would misorder the edge.
        self.emit(
            Event(
                type=outcome,
                time=self._clock(),
                data={
                    **facts,
                    "action": action_id,
                    "actor": self._owners.get(action_id, ""),
                },
            )
        )
        self._table.mark(action_id, FINISHED)

    def sweep(self) -> None:
        """Stop admission, then complete every action that has no completion.

        Invocation stops atomically. Deadline-driven skips choose RUNNER_DEADLINE
        before the sweep creates any timeout; an actual earlier failure wins.
        An in-flight run-deadline action, which no watchdog bounds, makes the
        reason OPERATION_TIMEOUT; in-flight operation-deadline actions alone
        are inside their own budgets, so RUNNER_DEADLINE. In-flight actions
        complete as info with that reason. Never-invoked actions complete as
        skipped.
        """
        with self._completed_lock:
            self._sweeping = True
            self._stop.set()
            in_flight = self._invoked - self._completed
        if any(action.id not in self._invoked for action in self._spec.actions):
            if time.monotonic() >= self._actor_deadline:
                self.stop_with(RUNNER_DEADLINE)
        if in_flight:
            # An operation-deadline action still in flight is inside its
            # watchdog's budget; only a run-deadline action has used its own.
            hung = any(self._by_id[a].deadline == DEADLINE_RUN for a in in_flight)
            self.stop_with(OPERATION_TIMEOUT if hung else RUNNER_DEADLINE)
        with self._state_lock:
            reason = self._reason
        for action in self._spec.actions:
            if action.id in in_flight:
                # A hung run-deadline op has no watchdog, so fail closed
                # instead of skipping an action the backend may still run.
                self.complete(action.id, INFO, {"reason": reason})
            elif action.id not in self._invoked:
                self.complete(action.id, SKIPPED, {})

    def run_watchdog(
        self, action: Action, deadline: float | None
    ) -> BackendResult | None:
        """Run the backend; None means the watchdog fired. Raises _Swept if the sweep had begun."""
        box: list[BackendResult] = []
        raised: list[Exception] = []

        def target() -> None:
            try:
                box.append(self._backend(action))
            except Exception as error:  # noqa: BLE001 - classified below
                raised.append(error)

        worker = threading.Thread(target=target, daemon=True, name=f"op-{action.id}")
        with self._completed_lock:
            if self._sweeping:
                raise _Swept()
            worker.start()
        worker.join(timeout=deadline)
        if worker.is_alive():
            return None
        if raised:
            raise raised[0]
        if len(box) != 1 or not isinstance(box[0], BackendResult):
            raise HarnessFailure(INVALID_BACKEND_RESPONSE)
        result = box[0]
        if result.outcome not in OUTCOMES or type(result.facts) is not dict:
            raise HarnessFailure(INVALID_BACKEND_RESPONSE)
        if not RESERVED_FACT_KEYS.isdisjoint(result.facts):
            raise HarnessFailure(INVALID_BACKEND_RESPONSE)
        try:
            event_line(Event(type=result.outcome, time=0, data=dict(result.facts)))
        except (TypeError, ValueError) as error:
            raise HarnessFailure(INVALID_BACKEND_RESPONSE) from error
        return result

    def unwind(self, unwind: Unwind) -> None:
        """Run fault unwind; an exception it raises is an invalid backend response."""
        try:
            unwind()
        except Exception as error:  # noqa: BLE001 - recorded below
            self.record_error(error)
            self.stop_with(INVALID_BACKEND_RESPONSE)

    def record_error(self, error: Exception) -> None:
        """Print an undeclared backend error and keep the first one, bounded."""
        traceback.print_exception(error, file=sys.stderr)
        with self._state_lock:
            if self._error is None:
                self._error = bounded_error(f"{type(error).__name__}: {error}")

    def classify_harness_error(self, action_id: str, error: Exception) -> str:
        """Record a backend error as an info completion; return its reason."""
        if isinstance(error, HarnessFailure) and error.reason in HARNESS_REASONS:
            reason = error.reason
        else:
            reason = INVALID_BACKEND_RESPONSE
            self.record_error(error)
        # Stop admission before publishing FINISHED: the backend may still
        # be running, so no finished-gated dependent may be invoked.
        self.stop_with(reason)
        self.complete(action_id, INFO, {"reason": reason})
        return reason

    def execute_one(self, actor_id: str, action: Action) -> bool:
        """Invoke one gated action; False means the actor must stop."""
        gated = all(
            self._table.wait(dep_id, dep_event, self._stop)
            for dep_id, dep_event in sorted(action.requires)
        )
        if not gated:
            return False
        with self._completed_lock:
            if self._sweeping or self._stop.is_set() or action.id in self._completed:
                return False
            if time.monotonic() >= self._actor_deadline:
                self.stop_with(RUNNER_DEADLINE)
                return False
            self._invoked.add(action.id)
            self.emit(
                Event(
                    type=INVOKE,
                    time=self._clock(),
                    data={"action": action.id, "actor": actor_id},
                )
            )
        self._table.mark(action.id, STARTED)
        watched = action.deadline != DEADLINE_RUN
        deadline = self._spec.budgets.operation_deadline_seconds if watched else None
        try:
            result = self.run_watchdog(action, deadline)
        except _Swept:
            return False
        except Exception as error:  # noqa: BLE001 - classified in helper
            self.classify_harness_error(action.id, error)
            return False
        if result is None:
            # The timed-out operation may still run daemon-side: stop
            # admission before FINISHED wakes a finished-gated dependent.
            self.stop_with(OPERATION_TIMEOUT)
            self.complete(action.id, INFO, {"reason": OPERATION_TIMEOUT})
            return False
        self.complete(action.id, result.outcome, dict(result.facts))
        return True

    def run_actor(self, actor_id: str, action_ids: tuple[str, ...]) -> None:
        """Execute an actor's actions in order until admission stops."""
        for action_id in action_ids:
            if not self.execute_one(actor_id, self._by_id[action_id]):
                break


def run_actions(
    spec: RunSpec,
    backend: Backend,
    sink: HistorySink,
    clock: Clock,
    *,
    unwind: Unwind | None = None,
    runner_deadline: float | None = None,
) -> ExecutionStatus:
    """Execute every action to exactly one completion event, then unwind faults.

    Actors stop at D-C. The sweep and `unwind` run even if starting an
    actor raises; neither waits for a backend call still running. Returns
    COMPLETED or HARNESS_FAILURE with its first reason; deadline-driven
    skips are runner-deadline.
    """
    if runner_deadline is None:
        runner_deadline = time.monotonic() + spec.budgets.runner_deadline_seconds
    give_up = runner_deadline - spec.budgets.cleanup_deadline_seconds
    run = _Run(spec, backend, sink, clock, give_up)
    try:
        threads = [
            threading.Thread(
                target=run.run_actor,
                args=(actor.id, actor.actions),
                name=f"actor-{actor.id}",
                daemon=True,
            )
            for actor in spec.actors
        ]
        for thread in threads:
            if time.monotonic() >= give_up:
                run.stop_with(RUNNER_DEADLINE)
                break
            thread.start()
        for thread in threads:
            if thread.ident is not None:
                thread.join(timeout=max(0.0, give_up - time.monotonic()))
    finally:
        try:
            run.sweep()
        finally:
            if unwind is not None:
                run.unwind(unwind)
    with run._state_lock:
        reason, error = run._reason, run._error
    if reason is None:
        return ExecutionStatus(COMPLETED, None)
    return ExecutionStatus(HARNESS_FAILURE, reason, error)
