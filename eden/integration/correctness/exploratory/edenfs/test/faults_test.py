# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Tests for the fault tracker's cleanup accounting over a fake Thrift client."""

import errno
import os
import subprocess
import unittest
from collections.abc import Callable, Iterator
from contextlib import contextmanager
from types import SimpleNamespace
from typing import Any, cast
from unittest import mock

from eden.integration.correctness.exploratory.core.history import (
    CLEANUP_REMOVED,
    InMemorySink,
)
from eden.integration.correctness.exploratory.core.scheduler import BackendResult
from eden.integration.correctness.exploratory.edenfs import faults
from eden.integration.correctness.exploratory.edenfs.faults import (
    _Installed,
    FaultTracker,
)
from eden.thrift.client import EdenNotRunningError
from thrift.python.exceptions import TransportError, TransportErrorType

KEY_CLASS = "tree-checkout"


class _FakeFaultClient:
    def __init__(self, blocked: list[list[str]]) -> None:
        self.blocked = blocked
        self.error: Exception | None = None
        self.remove_errors: list[Exception] = []
        self.remove_calls = 0
        self.on_inject: Callable[[Any], None] | None = None

    def injectFault(self, definition: Any) -> None:
        if self.on_inject is not None:
            self.on_inject(definition)

    def removeFault(self, arg: Any) -> bool:
        self.remove_calls += 1
        if self.remove_errors:
            raise self.remove_errors.pop(0)
        return False

    def unblockFault(self, arg: Any) -> None:
        pass

    def getBlockedFaults(self, request: Any) -> Any:
        if self.error is not None:
            raise self.error
        values = self.blocked.pop(0) if len(self.blocked) > 1 else self.blocked[0]
        return SimpleNamespace(keyValues=values)


class _FakeFaultEnv:
    def __init__(self, client: _FakeFaultClient, daemon_pid: int) -> None:
        self.client = client
        self.daemon_pid = daemon_pid
        self.exited = False
        self.hit = False
        self.mount_path = "/mount"

    @contextmanager
    def get_thrift_client(self) -> Iterator[_FakeFaultClient]:
        yield self.client


def _stopped_socket_error() -> EdenNotRunningError:
    """The error the Eden client raises when a cleanup call's connection fails."""
    return EdenNotRunningError(
        "/eden/socket",
        TransportError(
            TransportErrorType.NOT_OPEN, "connection refused", errno.ECONNREFUSED, 0
        ),
    )


def _exited_pid() -> int:
    """Return the pid of a child that has already exited and been reaped."""
    child = subprocess.Popen(["true"])
    child.wait()
    return child.pid


def _unwind(
    client: _FakeFaultClient, daemon_pid: int | None = None
) -> tuple[dict[str, int], dict[str, Any]]:
    """Unwind one never-hit block install and return the summary and cleanup facts."""
    spec: Any = SimpleNamespace(budgets=SimpleNamespace(cleanup_deadline_seconds=1.0))
    pid = os.getpid() if daemon_pid is None else daemon_pid
    tracker = FaultTracker(_FakeFaultEnv(client, pid), spec)
    tracker._installed["F"] = _Installed(
        fault_id="F",
        key_class=KEY_CLASS,
        selector="tree/a1/.*",
        witness=None,
        behavior="block",
        target_action="c1",
    )
    sink = InMemorySink()
    with mock.patch.object(faults, "DRAIN_TIMEOUT_SECONDS", 0.05):
        summary = tracker.unwind(sink, lambda: 0.0)
    (cleanup,) = sink.of_type(CLEANUP_REMOVED)
    return summary, cleanup.data


class UnwindTest(unittest.TestCase):
    def test_drained_cleanup_records_no_blocked_keys(self) -> None:
        summary, facts = _unwind(_FakeFaultClient([["tree/a1/b"], []]))
        self.assertEqual(summary["leaks"], 0)
        self.assertFalse(facts["leak"])
        self.assertEqual(facts["blocked_keys"], [])

    def test_leak_records_the_keys_still_blocked(self) -> None:
        summary, facts = _unwind(_FakeFaultClient([["tree/a1/b", "tree/a2/c"]]))
        self.assertEqual(summary["leaks"], 1)
        self.assertTrue(facts["leak"])
        self.assertEqual(facts["blocked_keys"], ["tree/a1/b", "tree/a2/c"])

    def test_cleanup_error_is_a_leak_with_unknown_blocked_keys(self) -> None:
        client = _FakeFaultClient([[]])
        client.error = RuntimeError("rpc failed")
        summary, facts = _unwind(client)
        self.assertEqual(summary["leaks"], 1)
        self.assertTrue(facts["leak"])
        self.assertEqual(facts["error"], "RuntimeError: rpc failed")
        self.assertIsNone(facts["blocked_keys"])

    def test_transport_error_with_the_daemon_alive_is_retried_once(self) -> None:
        client = _FakeFaultClient([[]])
        client.remove_errors = [_stopped_socket_error()]
        summary, facts = _unwind(client)
        self.assertEqual(summary["leaks"], 0)
        self.assertFalse(facts["leak"], facts)
        self.assertEqual(facts["cleanup_retries"], 1)
        self.assertEqual(facts["blocked_keys"], [])
        self.assertNotIn("error", facts)
        self.assertEqual(client.remove_calls, 2)

    def test_transport_error_with_the_daemon_gone_is_not_retried(self) -> None:
        client = _FakeFaultClient([[]])
        client.remove_errors = [_stopped_socket_error()]
        summary, facts = _unwind(client, daemon_pid=_exited_pid())
        self.assertEqual(summary["leaks"], 1)
        self.assertTrue(facts["leak"])
        self.assertEqual(facts["cleanup_retries"], 0)
        self.assertIn("EdenNotRunningError", facts["error"])
        self.assertEqual(client.remove_calls, 1)

    def test_a_second_consecutive_failure_is_a_leak(self) -> None:
        client = _FakeFaultClient([[]])
        client.remove_errors = [_stopped_socket_error(), _stopped_socket_error()]
        summary, facts = _unwind(client)
        self.assertEqual(summary["leaks"], 1)
        self.assertTrue(facts["leak"])
        self.assertEqual(facts["cleanup_retries"], 1)
        self.assertIn("EdenNotRunningError", facts["error"])
        self.assertIsNone(facts["blocked_keys"])
        self.assertEqual(client.remove_calls, 2)


def _tree_checkout_spec() -> Any:
    """A spec whose `c1` is a Thrift checkout an ordinal tree-checkout install targets."""
    target = SimpleNamespace(
        id="c1", kind="checkout", payload={"revision": 1, "via": "thrift"}
    )
    return SimpleNamespace(
        action=lambda action_id: target if action_id == "c1" else None,
        budgets=SimpleNamespace(cleanup_deadline_seconds=1.0),
    )


def _install(behavior: str, ordinal: int = 1) -> Any:
    payload: dict[str, Any] = {
        "fault_id": "F",
        "site": "tree-checkout",
        "target_action": "c1",
        "behavior": behavior,
        "key": "tree/a1",
        "ordinal": ordinal,
    }
    return SimpleNamespace(id="f1", kind="fault_install", payload=payload)


class InstallAndAwaitTest(unittest.TestCase):
    def test_a_failed_prefix_registration_is_still_unwound(self) -> None:
        """An ordinal install that raises after any registration is tracked for unwind."""
        client = _FakeFaultClient([[]])

        def refuse(_definition: Any) -> None:
            raise RuntimeError("inject failed")

        client.on_inject = refuse
        tracker = FaultTracker(
            _FakeFaultEnv(client, os.getpid()), _tree_checkout_spec()
        )
        with self.assertRaises(RuntimeError):
            tracker.install(_install("block", ordinal=3))
        sink = InMemorySink()
        with mock.patch.object(faults, "DRAIN_TIMEOUT_SECONDS", 0.05):
            summary = tracker.unwind(sink, lambda: 0.0)
        self.assertEqual(1, summary["unwound"])
        (cleanup,) = sink.of_type(CLEANUP_REMOVED)
        self.assertEqual("F", cleanup.data["fault_id"])

    def test_a_raising_main_registration_is_unwound_but_never_witnessed(
        self,
    ) -> None:
        """The ordinal prefix registers, the one-shot raises: unwound, a miss.

        Control: the same install whose registrations all succeed and whose
        removal finds nothing left is consumed, not a miss.
        """
        for behavior in ("block", "error", "delay"):
            for main_raises in (True, False):
                with self.subTest(behavior=behavior, main_raises=main_raises):
                    client = _FakeFaultClient([[]])
                    injected: list[Any] = []

                    def inject(
                        definition: Any,
                        raises: bool = main_raises,
                        seen: list[Any] = injected,
                    ) -> None:
                        seen.append(definition)
                        if raises and len(seen) == 2:
                            raise RuntimeError("inject failed")

                    client.on_inject = inject
                    tracker = FaultTracker(
                        _FakeFaultEnv(client, os.getpid()), _tree_checkout_spec()
                    )
                    install = _install(behavior, ordinal=3)
                    install.payload["delay_ms"] = 10
                    if main_raises:
                        with self.assertRaisesRegex(RuntimeError, "inject failed"):
                            tracker.install(install)
                    else:
                        tracker.install(install)
                    self.assertEqual(2, injected[0].count)
                    removals = client.remove_calls
                    sink = InMemorySink()
                    with mock.patch.object(faults, "DRAIN_TIMEOUT_SECONDS", 0.05):
                        summary = tracker.unwind(sink, lambda: 0.0)
                    (cleanup,) = sink.of_type(CLEANUP_REMOVED)
                    self.assertGreater(client.remove_calls, removals)
                    self.assertEqual(1, summary["unwound"])
                    blocking = behavior == "block"
                    self.assertEqual(
                        (
                            main_raises or blocking,
                            None if main_raises or blocking else True,
                        ),
                        (cleanup.data["miss"], cleanup.data["consumed"]),
                    )

    def test_a_non_blocking_await_reports_only_the_fixed_fields(self) -> None:
        """The await never merges the target's completion facts into its own."""
        tracker = FaultTracker(
            _FakeFaultEnv(_FakeFaultClient([[]]), os.getpid()), _tree_checkout_spec()
        )
        tracker.install(_install("error"))
        target = cast(Any, SimpleNamespace(id="c1"))
        tracker.record_completion(
            target,
            BackendResult("fail", {"reason": "checkout: x", "source": "thrift"}),
        )
        awaited = tracker.wait_for_hit(
            cast(Any, SimpleNamespace(payload={"fault_id": "F"}))
        )
        self.assertEqual(
            {"fault_id": "F", "target_completed": True, "target_outcome": "fail"},
            dict(awaited.facts),
        )
