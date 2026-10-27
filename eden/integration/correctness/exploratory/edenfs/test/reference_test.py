# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The reference side of a differential run: preparing the reference checkout (modeled
dirt, the initial state digest), its processes and their cleanup (stragglers found
and killed, group signals, unkillable sides kept for a retry), parity of reference
file and descriptor actions with the live backend, the environment's initial state,
and the close event's final failures.
"""

from __future__ import annotations

import errno
import os
import signal
import stat
import subprocess
import tempfile
import threading
import time
import unittest
import uuid
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
from types import SimpleNamespace
from typing import Any, cast
from unittest import mock

from eden.integration.correctness.exploratory.core.differential import SideResult
from eden.integration.correctness.exploratory.core.differential_schedule import observe
from eden.integration.correctness.exploratory.core.history import FAIL, InMemorySink, OK
from eden.integration.correctness.exploratory.core.scheduler import BackendResult
from eden.integration.correctness.exploratory.core.target import ProcessRegistry
from eden.integration.correctness.exploratory.edenfs import (
    differential_backend,
    environment as edenfs_environment,
    sapling_processes,
)
from eden.integration.correctness.exploratory.edenfs.actions import (
    ACTION_DEFS,
    DIFFERENTIAL_OBSERVATIONS,
)
from eden.integration.correctness.exploratory.edenfs.backends import LiveBackend
from eden.integration.correctness.exploratory.edenfs.encoding import (
    encode_value,
    initial_content,
)
from eden.integration.correctness.exploratory.edenfs.environment import Environment
from eden.integration.correctness.exploratory.edenfs.executor import (
    serves,
    SharedExecutor,
)
from eden.integration.correctness.exploratory.edenfs.reference import (
    InitialState,
    PLAIN_FILESYSTEM,
    ReferenceBackend,
    ReferenceCheckout,
    ReferenceFailure,
    SETUP,
    UNSUPPORTED,
)
from eden.integration.correctness.exploratory.edenfs.sapling_processes import (
    _Live,
    CLOSED,
    GROUP_LEAK,
    LEFT_RUNNING,
    REFERENCE_TIMEOUT,
    SPAWN,
    TOKEN_ENV,
)
from eden.integration.correctness.exploratory.edenfs.test.backends_fixtures import (
    _FakeEnv,
    REGISTERS,
)
from eden.integration.correctness.exploratory.edenfs.test.reference_fixtures import (
    FAKE_SL,
    HANG_GUARD,
    reference_spec,
    REGISTER,
    SEED,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import make_action

_FAST = 60.0  # kill_wait


def setUpModule() -> None:
    """Make every paired call's start barrier a hang guard.

    Production bounds the barrier by min(10 s, 0.1 x the operation deadline);
    here both sides start on their own, so only a stuck side reaches it.
    """
    for name in ("_BARRIER_SECONDS", "_BARRIER_FRACTION"):
        patcher = mock.patch.object(differential_backend, name, HANG_GUARD)
        patcher.start()
        unittest.addModuleCleanup(patcher.stop)


def _pidfd_pid(fd: int) -> int:
    """Return the PID a pidfd refers to."""
    with open(f"/proc/self/fdinfo/{fd}") as handle:
        for line in handle:
            if line.startswith("Pid:"):
                return int(line.split()[1])
    raise AssertionError(f"fd {fd} is not a pidfd")


@contextmanager
def _signals_sent() -> Iterator[list[int]]:
    """Record the PID or process group of every signal sent in the block."""
    sent: list[int] = []
    real_kill, real_killpg = os.kill, os.killpg
    real_pidfd_send = signal.pidfd_send_signal

    def kill(pid: int, signum: int) -> None:
        sent.append(pid)
        real_kill(pid, signum)

    def killpg(pgid: int, signum: int) -> None:
        sent.append(pgid)
        real_killpg(pgid, signum)

    def pidfd_send(fd: int, signum: int, *args: Any) -> None:
        sent.append(_pidfd_pid(fd))
        real_pidfd_send(fd, signum, *args)

    with (
        mock.patch.object(os, "kill", kill),
        mock.patch.object(os, "killpg", killpg),
        mock.patch.object(signal, "pidfd_send_signal", pidfd_send),
    ):
        yield sent


class _Checkouts(unittest.TestCase):
    """A source fixture, a fake Sapling, and a reference checkout under a temp dir."""

    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.tmp = Path(directory.name)
        self.sl = self.tmp / "sl"
        self.sl.write_text(FAKE_SL)
        self.sl.chmod(0o755)
        self.source = self.tmp / "source"
        for index in range(4):
            path = self.source / "tree" / "a" / f"f{index}"
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(f"committed {index}\n".encode())
        (self.source / ".node").write_text("n1")
        (self.source / ".origin").write_text(str(self.source))
        (self.tmp / "run").mkdir()

    def checkout(self, **environment: str) -> ReferenceCheckout:
        reference = ReferenceCheckout(
            self.tmp / "run" / "reference",
            str(self.sl),
            {**os.environ, **environment},
            ("n0", "n1"),
        )
        self.addCleanup(reference.close_resources)
        return reference


_DIRT = (("tree/a/f0", b"q" * 64), ("tree/a/f2", b"q" * 64))


class ReferenceCheckoutTest(_Checkouts):
    def test_prepare_replays_the_modeled_dirt_and_verifies_it(self) -> None:
        reference = self.checkout()
        state = InitialState("n1", _DIRT)
        facts = reference.prepare(str(self.source), state)
        self.assertEqual(
            {
                "path": str(reference.root),
                "node": "n1",
                "dirty_paths": 2,
                "initial_state": state.digest(),
                "umask": "0022",
            },
            facts,
        )
        for path, data in _DIRT:
            self.assertEqual(data, (reference.root / path).read_bytes())
        self.assertEqual(b"committed 1\n", (reference.root / "tree/a/f1").read_bytes())

    def test_prepare_refuses_a_checkout_off_the_modeled_state(self) -> None:
        with self.assertRaises(ReferenceFailure) as caught:
            self.checkout().prepare(str(self.source), InitialState("n0", _DIRT))
        self.assertEqual(SETUP, caught.exception.kind)

    def test_initial_state_digest_names_commit_and_dirt(self) -> None:
        base = InitialState("n1", _DIRT)
        self.assertEqual(base.digest(), InitialState("n1", _DIRT).digest())
        self.assertNotEqual(base.digest(), InitialState("n0", _DIRT).digest())
        self.assertNotEqual(base.digest(), InitialState("n1", _DIRT[:1]).digest())

    def test_status_runs_registered_processes_under_reference_keys(self) -> None:
        reference = self.checkout()
        reference.prepare(str(self.source), InitialState("n1", _DIRT))
        spec = reference_spec([make_action("s1", "status", channel="sl")])
        published: list[str] = []
        registry = reference.processes
        real_publish = registry.publish

        def publish(name: str, process: Any, spawned_at: float) -> None:
            published.append(name)
            real_publish(name, process, spawned_at)

        with mock.patch.object(registry, "publish", side_effect=publish):
            result = ReferenceBackend(reference, spec)(spec.action("s1"))
        self.assertEqual(1, published.count("reference/s1/status"))
        self.assertIsNone(registry.lookup("reference/s1/status"))
        self.assertEqual(OK, result.outcome)
        self.assertEqual(
            [{"path": "tree/a/f0", "code": "M"}, {"path": "tree/a/f2", "code": "M"}],
            result.facts["status"],
        )
        self.assertEqual(["tree/a/f0", "tree/a/f2"], result.facts["diff_paths"])
        self.assertIsNone(reference.processes.lookup("s1"))

    def test_process_failure_keeps_its_exit_status(self) -> None:
        reference = self.checkout()
        reference.root.mkdir()
        result = reference.run("x/fail", ["fail"], _FAST)
        self.assertEqual(4, result.returncode)
        self.assertEqual("refused\n", result.stderr)

    def test_straggling_group_is_killed_as_left_running(self) -> None:
        reference = self.checkout()
        reference.root.mkdir()
        with self.assertRaises(ReferenceFailure) as caught:
            reference.run("x/straggle", ["straggle"], _FAST)
        self.assertEqual(LEFT_RUNNING, caught.exception.kind)
        self.assertIn("sleep 3600", caught.exception.detail)

    def test_pipe_holding_straggler_is_killed_without_blocking_on_output(self) -> None:
        reference = self.checkout()
        reference.root.mkdir()
        # The straggler is still alive when it is named left-running, so the
        # run did not wait on its output.
        with self.assertRaises(ReferenceFailure) as caught:
            reference.run("x/pipe", ["straggle-pipe"], _FAST)
        self.assertEqual(LEFT_RUNNING, caught.exception.kind)
        self.assertIn("sleep 3600", caught.exception.detail)
        self.assertEqual([], reference.leftovers())

    def test_own_session_straggler_is_found_by_token_and_killed(self) -> None:
        reference = self.checkout()
        reference.root.mkdir()
        with self.assertRaises(ReferenceFailure) as caught:
            reference.run("x/session", ["straggle-session"], _FAST)
        self.assertEqual(LEFT_RUNNING, caught.exception.kind)
        self.assertIn("sleep 3600", caught.exception.detail)
        self.assertEqual([], reference.leftovers())

    def test_overrunning_leader_is_killed_as_a_typed_timeout(self) -> None:
        reference = self.checkout()
        reference.root.mkdir()
        # A 30 s leader can only end in reference-timeout through the 0.3 s bound.
        with self.assertRaises(ReferenceFailure) as caught:
            reference.run(
                "x/hang",
                ["hang"],
                60.0,
                until=time.monotonic() + 0.3,
            )
        self.assertEqual(REFERENCE_TIMEOUT, caught.exception.kind)
        self.assertEqual([], reference.leftovers())
        self.assertIsNone(reference.processes.lookup("reference/x/hang"))

    def test_every_reference_process_pins_a_plain_filesystem(self) -> None:
        reference = self.checkout()
        reference.root.mkdir()
        result = reference.run("x/args", ["args"], _FAST)
        self.assertEqual(["args", *PLAIN_FILESYSTEM], result.stdout.splitlines())

    def test_reference_processes_run_with_umask_022(self) -> None:
        reference = self.checkout()
        reference.root.mkdir()
        previous = os.umask(0o077)
        try:
            result = reference.run("x/umask", ["umask"], _FAST)
        finally:
            os.umask(previous)
        self.assertEqual("0022", result.stdout.strip())

    def test_publish_failure_is_a_spawn_failure_even_after_the_group_exits(
        self,
    ) -> None:
        reference = self.checkout()
        reference.root.mkdir()
        with (
            mock.patch.object(
                reference.processes, "publish", side_effect=RuntimeError("twice")
            ),
            mock.patch.object(os, "killpg", side_effect=ProcessLookupError),
        ):
            with self.assertRaises(ReferenceFailure) as caught:
                reference.run("x/publish", ["log"], _FAST)
        self.assertEqual(SPAWN, caught.exception.kind)

    def test_group_leak_stays_live_until_cleanup_reaps_it(self) -> None:
        reference = self.checkout()
        reference.root.mkdir()
        with mock.patch.object(_Live, "signal", lambda _self: None):
            with self.assertRaises(ReferenceFailure) as caught:
                reference.run(
                    "x/leak",
                    ["straggle"],
                    0.3,
                    until=time.monotonic() + 60.0,
                )
        self.assertEqual(GROUP_LEAK, caught.exception.kind)
        self.assertEqual(("reference/x/leak",), reference.live())
        self.assertIsNotNone(reference.processes.lookup("reference/x/leak"))
        self.assertTrue(reference.leftovers())
        self.assertEqual(0, reference.close_resources(time.monotonic() + HANG_GUARD))
        self.assertEqual((), reference.live())
        self.assertIsNone(reference.processes.lookup("reference/x/leak"))
        self.assertEqual([], reference.leftovers())

    def test_unkillable_side_is_counted_and_kept_for_a_retry(self) -> None:
        reference = self.checkout()
        reference.root.mkdir()
        with mock.patch.object(_Live, "signal", lambda _self: None):
            with self.assertRaises(ReferenceFailure):
                reference.run(
                    "x/leak",
                    ["straggle"],
                    0.3,
                    until=time.monotonic() + 60.0,
                )
            self.assertEqual(1, reference.close_resources(time.monotonic() + 0.3))
        self.assertEqual(("reference/x/leak",), reference.live())
        self.assertEqual(0, reference.close_resources(time.monotonic() + HANG_GUARD))
        self.assertEqual([], reference.leftovers())

    def test_group_signals_only_reach_an_unreaped_leader(self) -> None:
        """Every killpg names a leader PID that is still held, so it cannot be reused."""
        reference = self.checkout()
        reference.root.mkdir()
        signaled: list[tuple[int, bool]] = []
        real_killpg = os.killpg

        def recording_killpg(pgid: int, signum: int) -> None:
            signaled.append((pgid, os.path.exists(f"/proc/{pgid}")))
            real_killpg(pgid, signum)

        with mock.patch.object(os, "killpg", side_effect=recording_killpg):
            for key, command in (
                ("x/straggle", "straggle"),
                ("x/session", "straggle-session"),
            ):
                with self.assertRaises(ReferenceFailure):
                    reference.run(key, [command], _FAST)
            with self.assertRaises(ReferenceFailure):
                reference.run(
                    "x/hang",
                    ["hang"],
                    60.0,
                    until=time.monotonic() + 0.3,
                )
            result = reference.run("x/fail", ["fail"], _FAST)
            reference.close_resources(time.monotonic() + HANG_GUARD)
        self.assertEqual(4, result.returncode)
        self.assertTrue(signaled)
        self.assertEqual([], [pgid for pgid, held in signaled if not held])

    def test_a_reused_pid_listed_for_a_token_is_never_signaled(self) -> None:
        """A PID listed as a token carrier, then reused, fails the post-pidfd recheck."""
        unrelated = subprocess.Popen(
            ["sleep", "3600"], start_new_session=True, env={"PATH": os.environ["PATH"]}
        )
        self.addCleanup(unrelated.wait)
        self.addCleanup(unrelated.kill)
        with (
            mock.patch.object(
                sapling_processes, "_token_pids", return_value=[unrelated.pid]
            ),
            _signals_sent() as sent,
        ):
            sapling_processes.signal_tokens("stale-token")
        self.assertEqual([], sent)
        self.assertIsNone(unrelated.poll())

    def _carrier(self, token: str) -> subprocess.Popen[bytes]:
        """Start a process carrying ``token`` that no checkout here started."""
        process = subprocess.Popen(
            ["sleep", "3600"],
            start_new_session=True,
            env={"PATH": os.environ["PATH"], TOKEN_ENV: token},
        )
        self.addCleanup(process.wait)
        self.addCleanup(process.kill)
        return process

    def test_concurrent_runs_processes_are_neither_counted_nor_killed(self) -> None:
        """Another run's token, or one sharing this run's token as a prefix, is foreign."""
        reference = self.checkout()
        reference.root.mkdir()
        other = self.checkout()
        foreign = self._carrier("foreign-test")
        prefixed = self._carrier(f"{reference.token}x.{uuid.uuid4().hex}")
        concurrent = self._carrier(f"{other.token}.{uuid.uuid4().hex}")
        carriers = [foreign, prefixed, concurrent]
        any_token = f"\0{TOKEN_ENV}=".encode()
        self.assertLessEqual(
            {p.pid for p in carriers}, set(sapling_processes._carrier_pids(any_token))
        )
        with _signals_sent() as sent:
            with self.assertRaises(ReferenceFailure) as caught:
                reference.run("x/session", ["straggle-session"], _FAST)
            self.assertEqual(LEFT_RUNNING, caught.exception.kind)
            self.assertEqual([], reference.leftovers())
            self.assertEqual(
                0, reference.close_resources(time.monotonic() + HANG_GUARD)
            )
        self.assertEqual([], [p.pid for p in carriers if p.pid in sent])
        self.assertEqual([None, None, None], [p.poll() for p in carriers])
        self.assertEqual([concurrent.pid], other.leftovers())
        self.assertEqual(0, other.close_resources(time.monotonic() + HANG_GUARD))
        self.assertEqual(-signal.SIGKILL, concurrent.wait(HANG_GUARD))
        self.assertEqual([None, None], [foreign.poll(), prefixed.poll()])

    def test_this_runs_stray_carrier_is_detected_and_cleaned(self) -> None:
        reference = self.checkout()
        reference.root.mkdir()
        stray = self._carrier(f"{reference.token}.{uuid.uuid4().hex}")
        self.assertEqual([stray.pid], reference.leftovers())
        self.assertEqual(0, reference.close_resources(time.monotonic() + HANG_GUARD))
        self.assertEqual(-signal.SIGKILL, stray.wait(HANG_GUARD))
        self.assertEqual([], reference.leftovers())

    def test_a_stale_pid_is_never_counted_or_signaled(self) -> None:
        """A PID that carried this run's token, then exited or was reused, is not ours."""
        reference = self.checkout()
        reference.root.mkdir()
        mine = self._carrier(f"{reference.token}.{uuid.uuid4().hex}")
        self.assertEqual([mine.pid], reference.leftovers())
        mine.kill()
        mine.wait()
        unrelated = self._carrier("")
        with mock.patch.object(
            sapling_processes, "_proc_entries", return_value=[mine.pid, unrelated.pid]
        ):
            self.assertEqual([], reference.leftovers())
        with (
            mock.patch.object(
                sapling_processes, "_carrier_pids", return_value=[unrelated.pid]
            ),
            _signals_sent() as sent,
        ):
            self.assertEqual(0, reference.close_resources(time.monotonic() + 0.2))
        self.assertNotIn(unrelated.pid, sent)
        self.assertIsNone(unrelated.poll())

    def test_exited_never_waits_on_a_pidfd_a_concurrent_reap_closed(self) -> None:
        """``reap`` may close the pidfd between ``exited`` taking it and waiting on it."""

        class _ReapOnRelease:
            def __init__(self, live: _Live) -> None:
                self.live = live
                self.inner = threading.Lock()
                self.fired = False

            def __enter__(self) -> None:
                self.inner.acquire()

            def __exit__(self, *_exc: object) -> None:
                self.inner.release()
                if threading.current_thread().name == "waiter" and not self.fired:
                    self.fired = True
                    self.live.lock = self.inner
                    self.live.reap()

        for _ in range(50):
            process = subprocess.Popen(["/bin/true"], start_new_session=True)
            live = _Live(process, "token", os.pidfd_open(process.pid))
            live.lock = cast(Any, _ReapOnRelease(live))
            outcome: list[object] = []

            def wait(live: _Live = live, outcome: list[object] = outcome) -> None:
                try:
                    outcome.append(live.exited(1.0))
                except OSError as error:
                    outcome.append(error)

            waiter = threading.Thread(target=wait, name="waiter")
            waiter.start()
            waiter.join(timeout=HANG_GUARD)
            self.assertEqual([True], outcome)
            self.assertTrue(live.reaped)

    def test_spawn_failure_is_typed(self) -> None:
        reference = ReferenceCheckout(self.tmp, str(self.tmp / "missing-sl"), {}, ())
        with self.assertRaises(ReferenceFailure) as caught:
            reference.run("x/spawn", ["log"], _FAST)
        self.assertEqual(SPAWN, caught.exception.kind)

    def test_close_resources_kills_and_reaps_a_hung_group_by_the_deadline(self) -> None:
        reference = self.checkout()
        reference.root.mkdir()
        errors: list[Exception] = []

        def hang() -> None:
            try:
                reference.run(
                    "h/hang",
                    ["hang"],
                    60.0,
                    until=time.monotonic() + 60.0,
                )
            except Exception as error:  # noqa: BLE001 - asserted below
                errors.append(error)

        worker = threading.Thread(target=hang)
        worker.start()
        deadline = time.monotonic() + HANG_GUARD
        pgid = None
        while pgid is None and time.monotonic() < deadline:
            pgid = reference.processes.lookup("reference/h/hang")
            time.sleep(0.01)
        assert pgid is not None
        self.assertEqual(0, reference.close_resources(time.monotonic() + HANG_GUARD))
        worker.join(timeout=HANG_GUARD)
        self.assertFalse(worker.is_alive())
        (error,) = errors
        assert isinstance(error, ReferenceFailure)
        self.assertEqual(CLOSED, error.kind)
        with self.assertRaises(ProcessLookupError):
            os.killpg(pgid, signal.SIGKILL)
        with self.assertRaises(ReferenceFailure):
            reference.run("h/after", ["log"], _FAST)


class ReferenceActionParityTest(_Checkouts):
    """The reference reports each file action exactly as LiveBackend does on its root."""

    def test_file_actions_observe_like_the_live_backend(self) -> None:
        actions = [
            make_action("w1", "write", path=REGISTER, value_id="v1"),
            make_action("q", "read", path=REGISTER, channel="fuse"),
            make_action("g", "getattr", path=REGISTER),
            make_action("o", "open", path=REGISTER),
            make_action("f", "read_fd", fd="o"),
            make_action("c", "close", fd="o"),
            make_action("u", "unlink", path="tree/registers/missing.reg"),
            make_action("m", "mkdir", path="tree/registers/made"),
            make_action("h", "chmod", path=REGISTER, mode=0o755),
            make_action(
                "n",
                "rename",
                source=REGISTERS[1],
                destination="tree/registers/moved.reg",
            ),
            make_action("d", "readdir", path="tree/registers", channel="fuse"),
        ]
        spec = reference_spec(actions)
        roots = {}
        for side in ("eden", "reference"):
            root = self.tmp / side
            for register in REGISTERS:
                path = root / register
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(initial_content(SEED, register))
            roots[side] = root
        env = SimpleNamespace(
            mount_path=roots["eden"], processes=ProcessRegistry(), setup_facts={}
        )
        live = LiveBackend(cast(Any, env), spec)
        reference = ReferenceBackend(
            ReferenceCheckout(roots["reference"], str(self.sl), {}, ()), spec
        )
        for action in actions:
            with self.subTest(action=action.id):
                primary, mirrored = live(action), reference(action)
                definition = DIFFERENTIAL_OBSERVATIONS[action.kind]
                self.assertEqual(
                    observe(
                        SideResult(primary.outcome, primary.facts, 0, 1), definition
                    ),
                    observe(
                        SideResult(mirrored.outcome, mirrored.facts, 0, 1), definition
                    ),
                )
        self.assertEqual(
            encode_value(SEED, "a", "v1"),
            (roots["reference"] / REGISTER).read_bytes(),
        )
        self.assertEqual(FAIL, reference(actions[6]).outcome)
        self.assertEqual(
            0o755, stat.S_IMODE((roots["reference"] / REGISTER).stat().st_mode)
        )
        live.close_resources()
        reference.close_resources()

    def test_every_shared_kind_runs_through_the_shared_executor(self) -> None:
        """Each side hands every action both can run to the one shared executor.

        One action per differential kind and shared channel; Thrift channels
        stay with the live side and the reference refuses them.
        """
        moved = "tree/registers/moved.reg"
        shared = [
            make_action("w", "write", path=REGISTER, value_id="v1"),
            make_action("q", "read", path=REGISTER, channel="fuse"),
            make_action("g", "getattr", path=REGISTER),
            make_action("o", "open", path=REGISTER),
            make_action("f", "read_fd", fd="o"),
            make_action("c", "close", fd="o"),
            make_action("n", "rename", source=REGISTER, destination=moved),
            make_action("u", "unlink", path=REGISTER),
            make_action("m", "mkdir", path="tree/made"),
            make_action("h", "chmod", path=REGISTER, mode=0o755),
            make_action("d", "readdir", path="tree", channel="fuse"),
            make_action("s", "status", channel="sl"),
            make_action("k", "checkout", revision=1, via="goto", mode="normal"),
            make_action("y", "checkout", revision=1, via="goto", mode="continue"),
            make_action("r", "sl_rm", path=REGISTER),
            make_action("v", "sl_mv", source=REGISTER, destination=moved),
        ]
        thrift = [
            make_action(
                "t1", "read", path=REGISTER, channel="thrift", sync_timeout_seconds=0
            ),
            make_action("t2", "readdir", path="tree", channel="thrift"),
            make_action("t3", "status", channel="thrift"),
            make_action("t4", "checkout", revision=1, via="thrift", mode="normal"),
        ]
        self.assertEqual(
            set(DIFFERENTIAL_OBSERVATIONS), {action.kind for action in shared}
        )
        spec = reference_spec(shared + thrift)
        env = _FakeEnv({})
        cast(Any, env).mount_path = self.tmp
        live = LiveBackend(cast(Any, env), spec)
        reference = ReferenceBackend(
            ReferenceCheckout(self.tmp, str(self.sl), {}, ("n0", "n1")), spec
        )
        served: list[tuple[str, str]] = []
        sides = {id(live._executor): "live", id(reference._executor): "reference"}

        def run(executor: SharedExecutor, action: Any) -> BackendResult:
            served.append((sides[id(executor)], action.id))
            return BackendResult(OK, {"status": [], "diff_paths": []})

        with mock.patch.object(SharedExecutor, "__call__", autospec=True) as call:
            call.side_effect = run
            for action in shared:
                self.assertTrue(serves(spec, action), action.id)
                live(action)
                reference(action)
            for action in thrift:
                self.assertFalse(serves(spec, action), action.id)
                with self.assertRaises(ReferenceFailure) as caught:
                    reference(action)
                self.assertEqual(UNSUPPORTED, caught.exception.kind)
        self.assertEqual(
            [(side, a.id) for a in shared for side in ("live", "reference")], served
        )

    def test_the_shared_executor_refuses_every_other_kind(self) -> None:
        executor = SharedExecutor(
            str, lambda _action: b"", str, mock.Mock(), RuntimeError
        )
        for kind in sorted(set(ACTION_DEFS) - set(DIFFERENTIAL_OBSERVATIONS)):
            with self.subTest(kind=kind):
                with self.assertRaisesRegex(ValueError, f"does not run '{kind}'"):
                    executor(make_action("x", kind))

    def test_descriptor_actions_after_a_failed_reference_open_report_ebadf(
        self,
    ) -> None:
        spec = reference_spec(
            [
                make_action("o", "open", path="tree/missing"),
                make_action("f", "read_fd", fd="o"),
                make_action("c", "close", fd="o"),
            ]
        )
        reference = ReferenceBackend(
            ReferenceCheckout(self.tmp, str(self.sl), {}, ()), spec
        )
        self.assertEqual(errno.ENOENT, reference(spec.action("o")).facts["errno"])
        for action_id in ("f", "c"):
            result = reference(spec.action(action_id))
            self.assertEqual(
                (FAIL, errno.EBADF), (result.outcome, result.facts["errno"])
            )

    def test_actions_without_a_reference_side_are_refused(self) -> None:
        spec = reference_spec(
            [
                make_action(
                    "t",
                    "read",
                    path=REGISTER,
                    channel="thrift",
                    sync_timeout_seconds=0,
                )
            ]
        )
        reference = ReferenceBackend(
            ReferenceCheckout(self.tmp, str(self.sl), {}, ()), spec
        )
        with self.assertRaises(ReferenceFailure):
            reference(spec.action("t"))
        with self.assertRaises(ReferenceFailure):
            reference(make_action("k", "kill_process", target_action="t", delay_ms=0))


class EnvironmentInitialStateTest(unittest.TestCase):
    def test_scatter_writes_exactly_the_modeled_dirt(self) -> None:
        env = Environment(
            seed=5,
            file_count=8,
            commit_count=2,
            registers=REGISTERS,
            scatter_dirty=3,
            sink=InMemorySink(),
        )
        with tempfile.TemporaryDirectory() as mount:
            env._mount_path = Path(mount)
            env._commits = cast(Any, (mock.Mock(node="n0"), mock.Mock(node="n1")))
            tip = len(env.commits) - 1
            for index in range(8):
                path = Path(mount) / env._layout.file_at(index, tip).path
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"committed")
            dirt = env.initial_dirt()
            self.assertEqual(3, len(dirt))
            env._scatter_working_copy()
            dirty = {
                str(path.relative_to(mount))
                for path in Path(mount).rglob("*")
                if path.is_file() and path.read_bytes() != b"committed"
            }
            self.assertEqual({path for path, _ in dirt}, dirty)
            for path, data in dirt:
                self.assertEqual(data, (Path(mount) / path).read_bytes())

    def test_reference_is_prepared_after_every_other_setup_step(self) -> None:
        source = Path(edenfs_environment.__file__).read_text()
        setup = source[
            source.index("    def setup(self)") : source.index("    def initial_dirt")
        ]
        self.assertLess(
            setup.index("self._prepare_working_copy()"),
            setup.index("self._prepare_reference("),
        )


class EnvironmentCloseTest(unittest.TestCase):
    def _environment(
        self, cls: type[Environment] = Environment
    ) -> tuple[Environment, ReferenceCheckout]:
        env = cls(
            seed=3,
            file_count=4,
            commit_count=2,
            registers=REGISTERS,
            sink=InMemorySink(),
        )
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        checkout = ReferenceCheckout(Path(tmp.name), "/bin/false", {}, ())
        env._reference = checkout
        env._reference_runtime = checkout.runtime
        return env, checkout

    def test_lifecycle_style_stop_override_still_closes_the_reference(self) -> None:
        """Restack note: lifecycle's `_stop_eden` clears the old `_reference_repo` field.

        That assignment is now an unused attribute; the reference closes in
        its own cleanup step before `_stop_eden` runs, whatever a subclass does.
        """

        class _LifecycleStyle(Environment):
            def _stop_eden(self) -> None:
                self._reference_repo = None

        env, checkout = self._environment(_LifecycleStyle)
        self.assertIsNone(env.close())
        with self.assertRaises(ReferenceFailure) as caught:
            checkout.run("after/close", ["log"], _FAST)
        self.assertEqual(CLOSED, caught.exception.kind)
