# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import dataclasses
import errno
import io
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from collections.abc import Callable, Iterator
from contextlib import contextmanager, redirect_stderr, redirect_stdout
from dataclasses import replace
from typing import Any
from unittest import mock

from eden.integration.correctness.exploratory.cli import _main as cli_main
from eden.integration.correctness.exploratory.core import containment
from eden.integration.correctness.exploratory.core.checker import (
    BLOCKED,
    decode_verdict,
    encode_verdict,
    NOT_EVALUATED,
    REJECT,
)
from eden.integration.correctness.exploratory.core.history import (
    CLEANUP,
    Event,
    event_to_json,
    INVOKE,
    RUN_CLOSED,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Budgets,
    from_json as _from_json,
    RunSpec,
    to_json,
)
from eden.integration.correctness.exploratory.core.target import RUN_DIR_ENV
from eden.integration.correctness.exploratory.core.test.containment_testing import (
    deadline_after,
    private_temp_root,
)
from eden.integration.correctness.exploratory.core.test.testing import (
    HANG_GUARD,
    minimal_spec,
)
from eden.integration.correctness.exploratory.edenfs.target import live_runner_argv_for
from eden.integration.correctness.exploratory.edenfs.test.testing import WORKED_EXAMPLE
from eden.integration.correctness.exploratory.fake.target import FAKE
from eden.integration.correctness.exploratory.targets import target_entries

# The CLI's EdenFS entry: these tests exercise containment, not its properties.
EDENFS_TARGET = target_entries()["edenfs"].target


def from_json(text: str) -> RunSpec:
    return _from_json(text, EDENFS_TARGET)


def _tiny_spec(runner_deadline: float = 120.0) -> RunSpec:
    """WORKED_EXAMPLE fixture with an overridden runner deadline."""
    spec = from_json(WORKED_EXAMPLE)
    cleanup = min(spec.budgets.cleanup_deadline_seconds, runner_deadline / 4)
    budgets = replace(
        spec.budgets,
        runner_deadline_seconds=runner_deadline,
        cleanup_deadline_seconds=cleanup,
        operation_deadline_seconds=min(
            spec.budgets.operation_deadline_seconds, runner_deadline - cleanup
        ),
    )
    return replace(spec, budgets=budgets)


def _event_line() -> str:
    event = Event(type=INVOKE, time=1.0, data={"action": "w1", "actor": "a"})
    return json.dumps(event_to_json(event), sort_keys=True)


def _fake_observation_spec() -> RunSpec:
    return _from_json(
        minimal_spec(
            {"g": {"kind": "getattr", "deadline": "operation", "path": "p"}},
            [{"id": "a", "actions": ["g"]}],
            target_name="fake",
            environment_target={},
        ),
        FAKE,
    )


def _fake_write_read_spec() -> RunSpec:
    return _from_json(
        minimal_spec(
            {
                "w": {
                    "kind": "write",
                    "deadline": "operation",
                    "path": "register",
                    "value_id": "written",
                },
                "r": {"kind": "read", "deadline": "operation", "path": "register"},
            },
            [{"id": "a", "actions": ["w", "r"]}],
            target_name="fake",
            environment_target={},
        ),
        FAKE,
    )


# Budgets only a hang could exhaust.
_LONG_BUDGETS = Budgets(
    operation_deadline_seconds=HANG_GUARD,
    cleanup_deadline_seconds=HANG_GUARD,
    runner_deadline_seconds=4 * HANG_GUARD,
)


def _read_json(path: str) -> Any:
    with open(path) as handle:
        return json.load(handle)


_MANIFESTS = (
    "run-spec.json",
    "history.jsonl",
    "execution.json",
    "verdict.json",
    "replay.json",
)


@contextmanager
def _captured_spawns(
    delay: float = 0.0,
) -> Iterator[list[tuple[float, dict[str, str], subprocess.Popen[Any]]]]:
    """Record each runner the supervisor spawns: entry time, environment, process.

    A positive ``delay`` holds every spawn that long after it is requested.
    """
    spawns: list[tuple[float, dict[str, str], subprocess.Popen[Any]]] = []
    original_spawn = subprocess.Popen

    def spawn(*args: Any, **kwargs: Any) -> subprocess.Popen[Any]:
        entered = time.monotonic()
        time.sleep(delay)
        process = original_spawn(*args, **kwargs)
        spawns.append((entered, kwargs["env"], process))
        return process

    with mock.patch.object(containment.subprocess, "Popen", side_effect=spawn):
        yield spawns


def _lines(count: int) -> Callable[[bytes], bool]:
    """Return a predicate accepting output with at least ``count`` complete lines."""
    return lambda output: output.count(b"\n") >= count


class ContainmentTest(unittest.TestCase):
    def setUp(self) -> None:
        private_temp_root(self)

    def test_supervisor_deadline_includes_spawn_latency(self) -> None:
        """Supervisor and runner share spawn's absolute deadline, including startup.

        The spawn is held after the supervisor fixes the deadline. The runner
        echoes the deadline it read, which must equal both the value the
        supervisor passed and the value the supervisor enforced, and must predate
        the held spawn. The runner then blocks forever; the deadline expires
        once its echo is read, and the supervisor SIGKILLs it.
        """
        runner_deadline = 2 * HANG_GUARD
        supervised: list[float] = []
        original_supervise = containment._supervise

        def supervise(*args: Any) -> Any:
            supervised.append(args[-1])
            return original_supervise(*args)

        script = f"""
import json, os, signal
deadline = os.environ[{containment.RUNNER_DEADLINE_ENV!r}]
print(json.dumps({{'type': 'cleanup', 'time': 0.0, 'deadline': deadline}}), flush=True)
signal.pause()
"""
        with (
            tempfile.TemporaryDirectory() as tmp,
            _captured_spawns(delay=0.2) as spawns,
            mock.patch.object(containment, "_supervise", side_effect=supervise),
            deadline_after(_lines(1)),
        ):
            result = containment.run_exploration(
                _tiny_spec(runner_deadline=runner_deadline),
                tmp,
                target=EDENFS_TARGET,
                runner_argv=[sys.executable, "-c", script],
            )
            events = containment.load_history(
                os.path.join(tmp, containment.HISTORY_NAME)
            )
        ((entered, env, process),) = spawns
        passed = env[containment.RUNNER_DEADLINE_ENV]
        self.assertEqual(
            [passed, None], [event.data.get("deadline") for event in events]
        )
        self.assertEqual([float(passed)], supervised)
        # Fixed before the spawn began, so the held spawn counts against it.
        self.assertLessEqual(float(passed), entered + runner_deadline)
        self.assertEqual(result.execution.status, containment.CONTAINMENT_TIMEOUT)
        self.assertEqual(-signal.SIGKILL, result.execution.returncode)
        self.assertEqual(-signal.SIGKILL, process.wait(timeout=HANG_GUARD))

    def test_setup_mapping_reaches_execution_manifest(self) -> None:
        """A neutral cleanup setup mapping is copied into execution.json."""
        cleanup = Event(
            type=CLEANUP,
            time=1.0,
            data={
                "unwind": {
                    "leaks": 0,
                    "misses": 0,
                    "unwound": 0,
                    "setup": {"neutral_ms": 17},
                },
            },
        )
        line = json.dumps(event_to_json(cleanup), sort_keys=True)
        script = f"import sys; sys.stdout.write({line!r} + chr(10))"
        with tempfile.TemporaryDirectory() as tmp:
            result = containment.run_exploration(
                _tiny_spec(),
                tmp,
                target=EDENFS_TARGET,
                runner_argv=[sys.executable, "-c", script],
            )
            execution = _read_json(os.path.join(tmp, "execution.json"))
        expected = {"neutral_ms": 17, "run_dir": mock.ANY}
        self.assertEqual(expected, result.execution.setup)
        self.assertEqual(expected, execution["setup"])

    def test_absent_cleanup_leaves_only_the_supervisor_run_dir_fact(self) -> None:
        """A run without cleanup metadata records only the supervisor's run dir."""
        with tempfile.TemporaryDirectory() as tmp:
            result = containment.run_exploration(
                _tiny_spec(),
                tmp,
                target=EDENFS_TARGET,
                runner_argv=[sys.executable, "-c", "pass"],
            )
            execution = _read_json(os.path.join(tmp, "execution.json"))
        self.assertEqual(["run_dir"], list(result.execution.setup or {}))
        self.assertEqual(["run_dir"], list(execution["setup"]))

    def test_runner_env_restores_runtime_lib_dir(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            bin_dir = os.path.join(tmp, "runtime", "bin")
            lib_dir = os.path.join(tmp, "runtime", "lib")
            os.makedirs(bin_dir)
            os.makedirs(lib_dir)
            with mock.patch.object(sys, "executable", os.path.join(bin_dir, "python")):
                env = containment._runner_env()
        self.assertEqual(env["LD_LIBRARY_PATH"].split(os.pathsep)[0], lib_dir)

    def test_live_runner_argv_shape(self) -> None:
        base = containment.runner_argv_for(
            "/tmp/seed-0/run-spec.json", EDENFS_TARGET.name
        )
        self.assertEqual(live_runner_argv_for("/tmp/seed-0/run-spec.json"), base)
        self.assertEqual(
            live_runner_argv_for("/tmp/seed-0/run-spec.json", "torn-write"),
            base + ["--red-control", "torn-write"],
        )

    def test_live_runner_rejects_unknown_red_control(self) -> None:
        spec = _tiny_spec()
        with tempfile.TemporaryDirectory() as tmp:
            spec_path = os.path.join(tmp, "run-spec.json")
            with open(spec_path, "w") as handle:
                handle.write(to_json(spec))
            out, err = io.StringIO(), io.StringIO()
            with redirect_stdout(out), redirect_stderr(err):
                code = cli_main(
                    [
                        "--runner",
                        "--target",
                        "edenfs",
                        "--spec",
                        spec_path,
                        "--red-control",
                        "bogus",
                    ]
                )
            self.assertEqual(code, 2)
            # Validation happens before any daemon setup; stdout stays pure.
            self.assertEqual(out.getvalue(), "")
            self.assertIn("unknown red_control", err.getvalue())

    def test_live_shaped_runner_sigkill_preserves_prefix(self) -> None:
        lines = [_event_line(), _event_line(), _event_line()]
        blob = "\n".join(lines) + "\n"
        script = (
            f"import signal, sys; sys.stdout.write({blob!r}); "
            "sys.stdout.flush(); signal.pause()"
        )
        spec = _tiny_spec()
        argv = [sys.executable, "-c", script]
        with tempfile.TemporaryDirectory() as tmp, deadline_after(_lines(3)):
            result = containment.run_exploration(
                spec, tmp, target=EDENFS_TARGET, runner_argv=argv
            )
            self.assertEqual(result.execution.status, containment.CONTAINMENT_TIMEOUT)
            self.assertEqual(-signal.SIGKILL, result.execution.returncode)
            events = containment.load_history(os.path.join(tmp, "history.jsonl"))
            self.assertEqual([INVOKE] * 3 + [RUN_CLOSED], [e.type for e in events])
            replay_spec, replay_events = containment.load_replay(
                os.path.join(tmp, "replay.json"), EDENFS_TARGET
            )
            self.assertEqual(to_json(replay_spec), to_json(spec))
            self.assertEqual(replay_events, events)

    def test_runner_harness_reason_reaches_execution(self) -> None:
        info = {
            "type": "info",
            "time": 1.0,
            "action": "w1",
            "actor": "a",
            "reason": "operation-timeout",
        }
        line = json.dumps(info, sort_keys=True)
        script = (
            "import sys; sys.stdout.write(%r + chr(10)); "
            "sys.stdout.flush(); sys.exit(1)" % line
        )
        spec = _tiny_spec()
        argv = [sys.executable, "-c", script]
        with tempfile.TemporaryDirectory() as tmp:
            result = containment.run_exploration(
                spec, tmp, target=EDENFS_TARGET, runner_argv=argv
            )
            self.assertEqual(result.execution.status, containment.HARNESS_FAILURE)
            self.assertIn("runner exited 1", result.execution.reason)
            self.assertIn("operation-timeout", result.execution.reason)
            self.assertEqual(
                _read_json(os.path.join(tmp, "execution.json"))["reason"],
                result.execution.reason,
            )

    def test_execution_records_daemon_exit(self) -> None:
        """A daemon-exit info event is copied into execution.json as daemon_exit."""
        info = {
            "type": "info",
            "time": 2.0,
            "reason": "daemon-exit",
            "returncode": -11,
            "signal": "SIGSEGV",
            "log_tail": ["Segmentation fault"],
            "core_path": None,
        }
        line = json.dumps(info, sort_keys=True)
        script = f"import sys; sys.stdout.write({line!r} + chr(10))"
        with tempfile.TemporaryDirectory() as tmp:
            result = containment.run_exploration(
                _tiny_spec(),
                tmp,
                target=EDENFS_TARGET,
                runner_argv=[sys.executable, "-c", script],
            )
            execution = _read_json(os.path.join(tmp, "execution.json"))
        expected = {
            "returncode": -11,
            "signal": "SIGSEGV",
            "time": 2.0,
            "log_tail": ["Segmentation fault"],
            "core_path": None,
        }
        self.assertEqual(expected, result.execution.daemon_exit)
        self.assertEqual(expected, execution["daemon_exit"])

    def _run_runner(self, script: str, status: str) -> tuple[Any, str]:
        """Run a fake-target runner script; return (execution.json, run_dir)."""
        with tempfile.TemporaryDirectory() as tmp:
            containment.run_exploration(
                _fake_observation_spec(),
                tmp,
                target=FAKE,
                runner_argv=[sys.executable, "-c", script],
            )
            execution = _read_json(os.path.join(tmp, "execution.json"))
        self.assertEqual(status, execution["status"])
        return execution, execution["setup"]["run_dir"]

    def test_supervisor_closes_run_dir_when_runner_dies_before_cleanup(self) -> None:
        """A dead runner's live daemon is SIGKILLed, its mount unmounted, dir removed."""
        daemon = subprocess.Popen(
            [sys.executable, "-c", "import signal; signal.pause()"]
        )
        self.addCleanup(daemon.kill)
        order: list[str] = []

        def fusermount(argv: list[str], **_kwargs: Any) -> Any:
            order.append(argv[2])
            return mock.Mock(returncode=0, stderr="")

        def pids_naming(_run_dir: str) -> list[int]:
            order.append("kill")
            return [daemon.pid] if len(order) == 1 else []

        with (
            mock.patch.object(containment, "_mounts_under", lambda d: [d + "/m"]),
            mock.patch.object(containment, "_pids_owning", pids_naming),
            mock.patch.object(containment.subprocess, "run", fusermount),
        ):
            execution, run_dir = self._run_runner(
                f"import os; open(os.environ[{RUN_DIR_ENV!r}] + '/lock', 'w'); exit(1)",
                containment.HARNESS_FAILURE,
            )
        self.assertEqual(-signal.SIGKILL, daemon.wait(timeout=HANG_GUARD))
        self.assertEqual(["kill", "kill", run_dir + "/m"], order)
        self.assertFalse(os.path.exists(run_dir))
        self.assertEqual("supervisor: unmounted 1, removed", execution["cleanup"])

    def test_spawn_failure_writes_manifests_when_the_run_dir_resists_removal(
        self,
    ) -> None:
        """A failed spawn still writes execution.json; a run dir it cannot remove is recorded."""
        real_rmtree = shutil.rmtree
        kept: list[str] = []

        def resisting(path: Any, *args: Any, **kwargs: Any) -> None:
            if os.path.basename(str(path)).startswith(containment.RUN_DIR_PREFIX):
                kept.append(str(path))
                raise OSError(errno.EBUSY, "busy")
            real_rmtree(path, *args, **kwargs)

        with tempfile.TemporaryDirectory() as tmp:
            try:
                with (
                    mock.patch.object(
                        containment.subprocess, "Popen", side_effect=OSError("no exec")
                    ),
                    mock.patch.object(containment.shutil, "rmtree", resisting),
                ):
                    result = containment.run_exploration(
                        _tiny_spec(),
                        tmp,
                        target=EDENFS_TARGET,
                        runner_argv=[sys.executable, "-c", "pass"],
                    )
            finally:
                for path in kept:
                    real_rmtree(path, ignore_errors=True)
            execution = _read_json(os.path.join(tmp, "execution.json"))
        self.assertEqual(containment.HARNESS_FAILURE, result.execution.status)
        self.assertTrue(result.execution.reason.startswith("spawn-failed: "))
        self.assertEqual(1, len(kept))
        self.assertIn("remove failed", execution["cleanup"])

    def test_supervisor_removes_the_run_dir_a_failed_runner_left(self) -> None:
        execution, run_dir = self._run_runner("exit(1)", containment.HARNESS_FAILURE)
        self.assertFalse(os.path.exists(run_dir))
        self.assertEqual("supervisor: unmounted 0, removed", execution["cleanup"])

    def test_degradation_comes_only_from_the_final_record(self) -> None:
        degraded = Event(CLEANUP, 1.0, {"environment_degraded": "mount failed"})
        self.assertEqual(
            (containment.ENVIRONMENT_DEGRADED, "mount failed"),
            containment._final_status(degraded),
        )
        status, reason = containment._runner_exit_status(
            [], containment.ENVIRONMENT_DEGRADED_EXIT
        )
        self.assertEqual(containment.HARNESS_FAILURE, status)
        self.assertIn("runner exited 75", reason)

    def test_final_record_ends_supervision_and_decides_status(self) -> None:
        """The runner's final record ends supervision before EOF and decides the status.

        Each runner writes its final record and one more line, then holds its
        stdout open until killed, as a runner thread blocked in a call would.
        The supervisor kills it at the final record (returncode None), never
        records the later line, closes the run dir, and appends `run-closed`.
        """
        completed = {"status": "completed", "reason": None}
        cases = (
            ({"scheduler": completed, "unwind": {"leaks": 0}}, "completed", "ok"),
            (
                {
                    "scheduler": {
                        "status": "harness-failure",
                        "reason": "operation-timeout",
                    }
                },
                "harness-failure",
                "operation-timeout",
            ),
            (
                {"scheduler": completed, "unwind": {"leaks": 2}},
                "harness-failure",
                "cleanup-failed: unwind reported 2 leak(s)",
            ),
            (
                {"unwind": {"setup": {}}, "environment_degraded": "oom"},
                "environment-degraded",
                "oom",
            ),
        )
        for data, status, reason in cases:
            final = json.dumps(event_to_json(Event(CLEANUP, 1.0, data)))
            later = json.dumps({"type": "info", "time": 2.0, "reason": "late"})
            script = (
                f"import signal, sys; sys.stdout.write({final!r} + chr(10) + "
                f"{later!r} + chr(10)); sys.stdout.flush(); signal.pause()"
            )
            with self.subTest(reason=reason), tempfile.TemporaryDirectory() as tmp:
                started = time.monotonic()
                with _captured_spawns() as spawns:
                    result = containment.run_exploration(
                        replace(_fake_observation_spec(), budgets=_LONG_BUDGETS),
                        tmp,
                        target=FAKE,
                        runner_argv=[sys.executable, "-c", script],
                    )
                self.assertLess(time.monotonic() - started, HANG_GUARD)
                self.assertEqual(
                    (status, reason, None),
                    (
                        result.execution.status,
                        result.execution.reason,
                        result.execution.returncode,
                    ),
                )
                ((_entered, _env, runner),) = spawns
                self.assertEqual(-signal.SIGKILL, runner.wait(timeout=HANG_GUARD))
                events = containment.load_history(os.path.join(tmp, "history.jsonl"))
                self.assertEqual([CLEANUP, RUN_CLOSED], [e.type for e in events])
                self.assertEqual(2, result.execution.events)
                self.assertEqual(
                    "supervisor: unmounted 0, removed", result.execution.cleanup
                )
                self.assertFalse(
                    os.path.exists((result.execution.setup or {})["run_dir"])
                )

    def test_close_run_dir_kills_only_processes_carrying_the_run_dir_env(self) -> None:
        """A process carrying the run's RUN_DIR_ENV dies; one that only names the dir lives."""
        pause = [sys.executable, "-c", "import signal; signal.pause()"]
        with tempfile.TemporaryDirectory() as tmp:
            run_dir = os.path.join(tmp, "run")
            os.mkdir(run_dir)
            inspector_env = {k: v for k, v in os.environ.items() if k != RUN_DIR_ENV}
            inspector = subprocess.Popen(
                [*pause, run_dir + "/stderr.log"], env=inspector_env
            )
            owned = subprocess.Popen(pause, env={**os.environ, RUN_DIR_ENV: run_dir})
            for process in (inspector, owned):
                self.addCleanup(process.kill)
            with mock.patch.object(containment, "_mounts_under", lambda _d: []):
                cleanup = containment.close_run_dir(run_dir)
        self.assertEqual(-signal.SIGKILL, owned.wait(timeout=HANG_GUARD))
        self.assertIsNone(inspector.poll())
        self.assertEqual(
            containment.RunDirClose("supervisor: unmounted 0, removed", True, False),
            cleanup,
        )

    def test_leaked_run_dir_reported_even_without_cleanup_event(self) -> None:
        """A run dir the supervisor cannot remove stays recorded as setup.run_dir."""
        busy = mock.Mock(rmtree=mock.Mock(side_effect=OSError("busy")))
        with mock.patch.object(containment, "shutil", busy):
            execution, run_dir = self._run_runner(
                "exit(1)", containment.HARNESS_FAILURE
            )
        self.addCleanup(shutil.rmtree, run_dir)
        self.assertTrue(os.path.isdir(run_dir))
        self.assertEqual(
            "supervisor: unmounted 0, remove failed: busy", execution["cleanup"]
        )

    def test_a_run_dir_close_leak_fails_a_completed_run(self) -> None:
        """A completed run whose run dir outlives the close is a cleanup failure."""
        final = Event(CLEANUP, 1.0, {"scheduler": {"status": "completed"}})
        line = json.dumps(event_to_json(final))
        script = f"import sys; sys.stdout.write({line!r} + chr(10))"
        busy = mock.Mock(rmtree=mock.Mock(side_effect=OSError("busy")))
        with tempfile.TemporaryDirectory() as tmp:
            with mock.patch.object(containment, "shutil", busy):
                result = containment.run_exploration(
                    _fake_observation_spec(),
                    tmp,
                    target=FAKE,
                    runner_argv=[sys.executable, "-c", script],
                )
            events = containment.load_history(os.path.join(tmp, "history.jsonl"))
        run_dir = (result.execution.setup or {})["run_dir"]
        self.addCleanup(shutil.rmtree, run_dir)
        self.assertEqual(
            (
                containment.HARNESS_FAILURE,
                "cleanup-failed: supervisor: unmounted 0, remove failed: busy",
            ),
            (result.execution.status, result.execution.reason),
        )
        # Every process died, so the run's effects are resolved.
        self.assertEqual([CLEANUP, RUN_CLOSED], [e.type for e in events])

    def test_a_run_dir_already_gone_still_gets_the_kill_scan(self) -> None:
        """When the run dir is already gone and no process is left, the
        supervisor's kill scan still runs and it records "runner"."""
        cleanup = Event(type=CLEANUP, time=2.0, data={"unwind": {}})
        line = json.dumps(event_to_json(cleanup), sort_keys=True)
        script = (
            f"import os, shutil, sys; shutil.rmtree(os.environ[{RUN_DIR_ENV!r}]); "
            f"sys.stdout.write({line!r} + chr(10))"
        )
        with (
            mock.patch.object(containment, "_pids_owning", return_value=[]) as pids,
            mock.patch.object(containment.subprocess, "run") as run,
        ):
            execution, _run_dir = self._run_runner(script, containment.COMPLETED)
        pids.assert_called()
        run.assert_not_called()
        self.assertEqual("runner", execution["cleanup"])

    def test_cleanup_disabled_keeps_the_run_dir_and_kills_nothing(self) -> None:
        """EDEN_TEST_CLEANUP=no leaves the run dir, its mounts, and pids alone."""
        with (
            mock.patch.dict(os.environ, {"EDEN_TEST_CLEANUP": "no"}),
            mock.patch.object(containment, "_mounts_under") as mounts,
            mock.patch.object(containment, "_pids_owning") as pids,
            mock.patch.object(containment.subprocess, "run") as run,
        ):
            execution, run_dir = self._run_runner(
                "exit(1)", containment.HARNESS_FAILURE
            )
        self.addCleanup(shutil.rmtree, run_dir)
        self.assertTrue(os.path.isdir(run_dir))
        mounts.assert_not_called()
        pids.assert_not_called()
        run.assert_not_called()
        self.assertEqual(
            "supervisor: kept (EDEN_TEST_CLEANUP=no)", execution["cleanup"]
        )

    def test_sigsegv_records_setup_phase_and_faulthandler_traceback(self) -> None:
        """A runner signal leaves its last phase and Python fatal-signal trace."""
        script_template = """
import os
import signal
import sys
from dataclasses import replace

from eden.integration.correctness.exploratory.core.containment import runner_main
from eden.integration.correctness.exploratory.fake.target import FAKE


def make_backend(_environment, _spec, _red_control):
    os.kill(os.getpid(), signal.SIGSEGV)

target = replace(FAKE, make_backend=make_backend)
raise SystemExit(runner_main({spec_path!r}, target))
"""
        with tempfile.TemporaryDirectory() as tmp:
            script = script_template.format(
                spec_path=os.path.join(tmp, "run-spec.json")
            )
            result = containment.run_exploration(
                _fake_observation_spec(),
                tmp,
                target=FAKE,
                runner_argv=[sys.executable, "-c", script],
            )
            execution = _read_json(os.path.join(tmp, "execution.json"))
            with open(os.path.join(tmp, "stderr.log")) as stderr_file:
                stderr = stderr_file.read()
        self.assertEqual(containment.HARNESS_FAILURE, result.execution.status)
        self.assertEqual(-signal.SIGSEGV, result.execution.returncode)
        self.assertEqual("setup-done", result.execution.runner_phase)
        self.assertEqual(-signal.SIGSEGV, execution["returncode"])
        self.assertEqual("setup-done", execution["runner_phase"])
        self.assertIn("Fatal Python error: Segmentation fault", stderr)
        self.assertIn("Current thread", stderr)
        self.assertIn("make_backend", stderr)

    def test_runner_keeps_stdout_pure_and_never_closes_after_backend_failure(
        self,
    ) -> None:
        """make_environment's stdout goes to stderr.log; a failed make_backend leaves the environment to the supervisor."""
        script_template = """
import sys
from dataclasses import replace

from eden.integration.correctness.exploratory.core.containment import runner_main
from eden.integration.correctness.exploratory.fake.target import FAKE, FakeEnvironment


class ClosingEnvironment(FakeEnvironment):
    def close(self):
        print("environment closed", file=sys.stderr, flush=True)
        return None


def make_environment(_spec, _sink):
    print("environment noise")
    return ClosingEnvironment()


def make_backend(_environment, _spec, _red_control):
    raise RuntimeError("backend offline")

target = replace(FAKE, make_environment=make_environment, make_backend=make_backend)
raise SystemExit(runner_main({spec_path!r}, target))
"""
        with tempfile.TemporaryDirectory() as tmp:
            script = script_template.format(
                spec_path=os.path.join(tmp, "run-spec.json")
            )
            result = containment.run_exploration(
                _fake_observation_spec(),
                tmp,
                target=FAKE,
                runner_argv=[sys.executable, "-c", script],
            )
            with open(os.path.join(tmp, "stderr.log")) as stderr_file:
                stderr = stderr_file.read()
        self.assertEqual(containment.HARNESS_FAILURE, result.execution.status)
        self.assertEqual(1, result.execution.returncode)
        self.assertIn("environment noise", stderr)
        self.assertIn("runner: backend-failed: RuntimeError: backend offline", stderr)
        self.assertNotIn("environment closed", stderr)

    def test_backend_exception_text_reaches_the_execution_manifest(self) -> None:
        """An unexpected backend exception leaves its bounded `Type: message` in execution.json."""
        script_template = """
from dataclasses import replace

from eden.integration.correctness.exploratory.core.containment import runner_main
from eden.integration.correctness.exploratory.fake.target import FAKE, FakeBackend


class RaisingBackend(FakeBackend):
    def __call__(self, action):
        raise RuntimeError("backend exploded")


target = replace(FAKE, make_backend=RaisingBackend)
raise SystemExit(runner_main({spec_path!r}, target))
"""
        with tempfile.TemporaryDirectory() as tmp:
            script = script_template.format(
                spec_path=os.path.join(tmp, "run-spec.json")
            )
            result = containment.run_exploration(
                _fake_observation_spec(),
                tmp,
                target=FAKE,
                runner_argv=[sys.executable, "-c", script],
            )
            execution = _read_json(os.path.join(tmp, "execution.json"))
        self.assertEqual(containment.HARNESS_FAILURE, result.execution.status)
        self.assertEqual("invalid-backend-response", result.execution.reason)
        self.assertEqual(
            "RuntimeError: backend exploded", result.execution.backend_error
        )
        self.assertEqual("RuntimeError: backend exploded", execution["backend_error"])

    def test_supervisor_closes_run_dir_when_supervision_raises(self) -> None:
        """An exception out of supervision still removes the run dir."""
        closed: list[str] = []

        def close_run_dir(run_dir: str) -> str:
            closed.append(run_dir)
            shutil.rmtree(run_dir)
            return "supervisor: removed"

        with (
            tempfile.TemporaryDirectory() as tmp,
            mock.patch.object(
                containment, "_supervise", side_effect=OSError("fsync failed")
            ),
            mock.patch.object(containment, "close_run_dir", close_run_dir),
        ):
            with self.assertRaisesRegex(OSError, "fsync failed"):
                containment.run_exploration(
                    _fake_observation_spec(),
                    tmp,
                    target=FAKE,
                    runner_argv=[sys.executable, "-c", "pass"],
                )
        self.assertEqual(1, len(closed))
        self.assertFalse(os.path.exists(closed[0]))

    def test_setup_failure_still_manifests(self) -> None:
        def _boom() -> None:
            raise RuntimeError("fixture offline")

        spec = _fake_write_read_spec()
        with tempfile.TemporaryDirectory() as tmp:
            result = containment.run_exploration(spec, tmp, target=FAKE, setup=_boom)
            self.assertEqual(result.execution.status, containment.HARNESS_FAILURE)
            self.assertIn("fixture offline", result.execution.reason)
            for name in _MANIFESTS:
                self.assertTrue(os.path.isfile(os.path.join(tmp, name)))
            self.assertEqual(os.path.getsize(os.path.join(tmp, "history.jsonl")), 0)
            # Fail-closed: the empty history cannot accept; it never reached
            # the target, so core.activity blocks instead of rejecting.
            self.assertEqual(result.verdict.kind, NOT_EVALUATED)
            self.assertEqual(result.verdict.not_evaluated_cause, BLOCKED)

    def test_check_cli_exits_nonzero_on_reject(self) -> None:
        spec = _fake_write_read_spec()
        torn = "54" * 64  # b"T" * 64: full-length bytes no write produced
        history = [
            {"type": "invoke", "time": 1.0, "action": "w", "actor": "a"},
            {"type": "ok", "time": 2.0, "action": "w", "actor": "a"},
            {"type": "invoke", "time": 3.0, "action": "r", "actor": "a"},
            {"type": "ok", "time": 4.0, "action": "r", "actor": "a", "bytes": torn},
        ]
        with tempfile.TemporaryDirectory() as tmp:
            spec_path = os.path.join(tmp, "spec.json")
            history_path = os.path.join(tmp, "history.jsonl")
            with open(spec_path, "w") as handle:
                handle.write(to_json(spec))
            with open(history_path, "w") as handle:
                for line in history:
                    handle.write(json.dumps(line, sort_keys=True) + "\n")
            buf = io.StringIO()
            with redirect_stdout(buf):
                code = cli_main(
                    [
                        "check",
                        "--target",
                        "fake",
                        "--spec",
                        spec_path,
                        "--history",
                        history_path,
                    ]
                )
            self.assertEqual(code, 1)
            printed = json.loads(buf.getvalue())
            self.assertEqual((REJECT, 1), (printed["kind"], printed["schema_version"]))
            self.assertEqual(
                buf.getvalue().encode("ascii"),
                encode_verdict(decode_verdict(buf.getvalue().encode("ascii"))),
            )

    def test_offline_walkthrough_runs_on_the_fake_target(self) -> None:
        """The root README's offline steps, each verb given `--target fake`."""
        with tempfile.TemporaryDirectory() as tmp:
            spec = os.path.join(tmp, "seed-8.json")
            run = os.path.join(tmp, "seed-8-run")
            steps = (
                ["generate", "--target", "fake", "--seed", "8", "--files", "32"],
                ["validate", "--target", "fake", "--spec", spec],
                ["run", "--target", "fake", "--spec", spec, "--artifacts", run],
                [
                    "check",
                    "--target",
                    "fake",
                    "--spec",
                    spec,
                    "--history",
                    f"{run}/history.jsonl",
                ],
                ["check", "--target", "fake", "--replay", f"{run}/replay.json"],
            )
            for argv in steps:
                with self.subTest(verb=argv[0]):
                    out = io.StringIO()
                    with redirect_stdout(out):
                        self.assertEqual(0, cli_main(argv))
                    if argv[0] == "generate":
                        with open(spec, "w") as handle:
                            handle.write(out.getvalue())
            for verb in (["validate", "--spec", spec], ["check", "--spec", spec]):
                with self.subTest(missing_target=verb[0]):
                    with redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                        cli_main(verb)

    def test_run_cli_exits_nonzero_on_meaningless_prefix(self) -> None:
        spec = _fake_observation_spec()
        with tempfile.TemporaryDirectory() as tmp:
            spec_path = os.path.join(tmp, "spec.json")
            with open(spec_path, "w") as handle:
                handle.write(to_json(spec))
            artifacts = os.path.join(tmp, "artifacts")
            buf = io.StringIO()
            with redirect_stdout(buf):
                code = cli_main(
                    [
                        "run",
                        "--target",
                        "fake",
                        "--spec",
                        spec_path,
                        "--artifacts",
                        artifacts,
                    ]
                )
            self.assertEqual(code, 1)
            # The run itself completed; only the verdict refuses it.
            self.assertEqual(
                _read_json(os.path.join(artifacts, "execution.json"))["status"],
                containment.COMPLETED,
            )
            self.assertEqual(
                _read_json(os.path.join(artifacts, "verdict.json"))["kind"], REJECT
            )
            admission = _read_json(os.path.join(artifacts, "admission.json"))
            self.assertEqual("manual", admission["run_purpose"])
            self.assertEqual(
                {field.name for field in dataclasses.fields(containment.Execution)},
                set(_read_json(os.path.join(artifacts, "execution.json"))),
            )
