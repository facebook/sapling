# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The EdenFS environment fixture without a daemon: the daemon-exit monitor, log-tail
and signal parsing, the fatal-bug flag, setup load order, overlay aging,
setup-profile schema bounds, and setup out-of-memory classification.
"""

from __future__ import annotations

import io
import json
import os
import shlex
import signal
import subprocess
import sys
import threading
import time
import unittest
from contextlib import redirect_stderr, redirect_stdout
from dataclasses import replace
from typing import Any, cast
from unittest import mock

from eden.integration.correctness.exploratory.core import (
    containment,
    target as target_module,
)
from eden.integration.correctness.exploratory.core.history import (
    event_from_json,
    INFO,
    InMemorySink,
)
from eden.integration.correctness.exploratory.core.runspec import (
    from_json,
    SpecError,
    to_json,
)
from eden.integration.correctness.exploratory.edenfs import (
    environment as environment_module,
)
from eden.integration.correctness.exploratory.edenfs.environment import Environment
from eden.integration.correctness.exploratory.edenfs.target import RECORDING_TARGET
from eden.integration.correctness.exploratory.edenfs.test.environment_fixtures import (
    environment_target,
    getattr_action,
    HANG_GUARD,
    single_action_spec,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import WORKED_EXAMPLE
from eden.integration.correctness.repo import AuthoredCommit
from eden.test_support.temporary_directory import TempFileManager


def _monitored_launcher(env: Environment) -> subprocess.Popen[str]:
    """Attach a monitor to a stand-in launcher that sleeps until killed."""
    launcher = subprocess.Popen(
        [sys.executable, "-c", "import signal; signal.pause()"], text=True
    )
    env._launcher = launcher
    env._start_exit_monitor()
    return launcher


def _first_info_event(sink: InMemorySink, timeout: float = HANG_GUARD) -> Any:
    """Return the sink's single info event once it arrives within ``timeout``."""
    deadline = time.monotonic() + timeout
    while not sink.of_type(INFO) and time.monotonic() < deadline:
        time.sleep(0.02)
    [event] = sink.of_type(INFO)
    return event


def _tail_of_stderr(lines: list[str]) -> list[str]:
    """Run ``_log_tail`` with ``lines`` as the file behind fd 2."""
    with TempFileManager() as manager:
        log = manager.make_temp_file("stderr.log")
        log.write("\n".join(lines) + "\n")
        log.flush()
        saved_stderr = os.dup(2)
        try:
            os.dup2(log.fileno(), 2)
            return environment_module._log_tail()
        finally:
            os.dup2(saved_stderr, 2)
            os.close(saved_stderr)


class _RecordingRepo:
    """A mount repo stand-in that records `sl goto` calls."""

    def __init__(self, fail_at: int | None = None) -> None:
        self.calls: list[tuple[str, str]] = []
        self.fail_at = fail_at

    def hg(self, command: str, node: str) -> str:
        self.calls.append((command, node))
        if self.fail_at == len(self.calls):
            raise RuntimeError("update failed")
        return ""

    def get_head_hash(self) -> str:
        return "tip"


class EnvironmentUnitTest(unittest.TestCase):
    """Environment behavior that needs no daemon."""

    def _with_fatal_eden_bug(self, supplied: str | None) -> tuple[Environment, list]:
        env = object.__new__(Environment)
        env._setup_facts = {}
        with mock.patch.dict(os.environ, clear=False):
            os.environ.pop("EDENFS_DAEMON_FATAL_EDEN_BUG", None)
            if supplied is not None:
                os.environ["EDENFS_DAEMON_FATAL_EDEN_BUG"] = supplied
            args = env._daemon_extra_args()
        return env, args

    def test_setup_profile_schema(self) -> None:
        """Setup keys are exact and scatter stays within the repository."""
        raw = json.loads(WORKED_EXAMPLE)
        profile = raw["environment"]["target"]["setup_profile"]
        profile["unknown"] = 1
        with self.assertRaisesRegex(SpecError, "unknown fields"):
            from_json(json.dumps(raw), RECORDING_TARGET)

        del profile["unknown"]
        file_count = raw["environment"]["target"]["repository"]["file_count"]
        for scatter_dirty in (-1, file_count + 1):
            with self.subTest(scatter_dirty=scatter_dirty):
                profile["scatter_dirty"] = scatter_dirty
                with self.assertRaisesRegex(SpecError, "scatter_dirty"):
                    from_json(json.dumps(raw), RECORDING_TARGET)

        profile["scatter_dirty"] = file_count
        spec = from_json(json.dumps(raw), RECORDING_TARGET)
        actual = spec.environment.target["setup_profile"]["scatter_dirty"]
        self.assertEqual(file_count, actual)

    def test_load_setup_schema_defaults_and_bounds(self) -> None:
        """Load setup knobs are optional and bounded by distinct revisions."""
        raw = json.loads(WORKED_EXAMPLE)
        profile = raw["environment"]["target"]["setup_profile"]
        spec = from_json(json.dumps(raw), RECORDING_TARGET)
        self.assertNotIn("warm_readdir", spec.environment.target["setup_profile"])
        self.assertNotIn("aged_overlay", spec.environment.target["setup_profile"])

        profile.update(warm_readdir=True, aged_overlay=3)
        spec = from_json(json.dumps(raw), RECORDING_TARGET)
        self.assertTrue(spec.environment.target["setup_profile"]["warm_readdir"])
        self.assertEqual(3, spec.environment.target["setup_profile"]["aged_overlay"])

        for field, invalid in (
            ("warm_readdir", 1),
            ("reference_clone", 1),
            ("aged_overlay", -1),
            ("aged_overlay", True),
            ("aged_overlay", 4),
        ):
            with self.subTest(field=field, invalid=invalid):
                profile.update(warm_readdir=True, aged_overlay=0, reference_clone=False)
                profile[field] = invalid
                with self.assertRaisesRegex(SpecError, field):
                    from_json(json.dumps(raw), RECORDING_TARGET)

        raw["environment"]["target"]["repository"]["commit_count"] = 1
        profile.update(warm_readdir=False, aged_overlay=1)
        with self.assertRaisesRegex(SpecError, "aged_overlay"):
            from_json(json.dumps(raw), RECORDING_TARGET)

    def test_memory_max_schema(self) -> None:
        """The optional memory limit accepts positive integers only."""
        raw = json.loads(WORKED_EXAMPLE)
        profile = raw["environment"]["target"]["setup_profile"]
        spec = from_json(json.dumps(raw), RECORDING_TARGET)
        self.assertNotIn("memory_max_mb", spec.environment.target["setup_profile"])

        profile["memory_max_mb"] = 4096
        spec = from_json(json.dumps(raw), RECORDING_TARGET)
        self.assertEqual(
            4096, spec.environment.target["setup_profile"]["memory_max_mb"]
        )
        for invalid in (0, -1, True, "4096"):
            with self.subTest(invalid=invalid):
                profile["memory_max_mb"] = invalid
                with self.assertRaisesRegex(SpecError, "memory_max_mb"):
                    from_json(json.dumps(raw), RECORDING_TARGET)

    def test_case_sensitive_false_is_valid(self) -> None:
        """A case-insensitive target configuration is a valid RunSpec."""
        raw = json.loads(WORKED_EXAMPLE)
        raw["environment"]["target"]["case_sensitive"] = False
        spec = from_json(json.dumps(raw), RECORDING_TARGET)
        self.assertFalse(spec.environment.target["case_sensitive"])

    def test_memory_limit_scopes_the_daemon_without_swap(self) -> None:
        """A memory limit becomes the scope's MemoryMax with swap disabled."""
        env = object.__new__(Environment)
        env._memory_max_mb = 4096
        prefix = env._daemon_command_prefix()
        self.assertEqual(
            [
                "systemd-run",
                "--user",
                "--scope",
                "--quiet",
                "-p",
                "MemoryMax=4096M",
                "-p",
                "MemorySwapMax=0",
            ],
            prefix[prefix.index("systemd-run") :],
        )
        env._memory_max_mb = None
        prefix = env._daemon_command_prefix()
        self.assertEqual(
            ["systemd-run", "--user", "--scope", "--quiet"],
            prefix[prefix.index("systemd-run") :],
        )

    def test_the_daemon_dies_with_the_process_that_spawned_it(self) -> None:
        """The launcher prefix SIGKILLs the daemon when the process that spawned it dies.

        Twin: a prefix naming a parent that is not the spawner runs nothing.
        """
        env = object.__new__(Environment)
        env._memory_max_mb = None
        prefix = env._daemon_command_prefix()
        parent = prefix.index("systemd-run") - 1
        self.assertEqual(str(os.getpid()), prefix[parent])
        guard = [shlex.quote(arg) for arg in prefix[:parent]]
        spawner = subprocess.Popen(
            ["sh", "-c", " ".join([*guard, "$$", "sleep 300 & echo $!; wait"])],
            stdout=subprocess.PIPE,
            text=True,
        )
        self.addCleanup(spawner.kill)
        stdout = spawner.stdout
        assert stdout is not None
        daemon = int(stdout.readline())

        def command() -> bytes:
            try:
                with open(f"/proc/{daemon}/cmdline", "rb") as handle:
                    return handle.read()
            except OSError:
                return b""

        deadline = time.monotonic() + HANG_GUARD
        while not command().startswith(b"sleep\0") and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertTrue(command().startswith(b"sleep\0"))
        spawner.kill()
        spawner.wait(timeout=HANG_GUARD)

        def running() -> bool:
            try:
                with open(f"/proc/{daemon}/stat") as handle:
                    return handle.read().rpartition(")")[2].split()[0] != "Z"
            except OSError:
                return False

        while running() and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertFalse(running())
        orphan = [*prefix[:parent], "1", "echo", "ran"]
        result = subprocess.run(orphan, capture_output=True, text=True)
        self.assertEqual((1, ""), (result.returncode, result.stdout))

    def _stopping_env(self, kill: Any, launcher: subprocess.Popen[str]) -> Environment:
        env = object.__new__(Environment)
        env._owned_exit = threading.Event()
        env._eden = cast(Any, mock.Mock(kill=kill))
        env._launcher = launcher
        env._daemon_pid = launcher.pid
        env._run_dir = None
        return env

    def test_close_sigkills_the_launcher_only_after_a_failed_normal_stop(self) -> None:
        """A failed normal shutdown is a close error, then SIGKILL.

        Twin: a shutdown that stops the launcher sends no SIGKILL.
        """
        launcher = subprocess.Popen(
            [sys.executable, "-c", "import signal; signal.pause()"], text=True
        )
        self.addCleanup(launcher.kill)

        def refuse(retry: bool) -> None:
            raise RuntimeError("edenfsctl stop failed")

        env = self._stopping_env(refuse, launcher)
        with self.assertRaisesRegex(RuntimeError, "normal EdenFS shutdown failed"):
            env._stop_eden()
        self.assertEqual(-signal.SIGKILL, launcher.returncode)

        stopped = subprocess.Popen([sys.executable, "-c", ""], text=True)
        env = self._stopping_env(lambda retry: stopped.wait(HANG_GUARD), stopped)
        with mock.patch.object(environment_module, "_sigkill_owned_processes") as kill:
            env._stop_eden()
        kill.assert_not_called()

    def test_sigkill_never_signals_a_launcher_pid_already_reaped(self) -> None:
        """The launcher is signaled through its Popen, so a reaped pid is never reused."""
        launcher = subprocess.Popen([sys.executable, "-c", ""], text=True)
        launcher.wait(timeout=HANG_GUARD)
        with mock.patch.object(environment_module.os, "kill") as kill:
            environment_module._sigkill_owned_processes(launcher.pid, launcher)
        kill.assert_not_called()

    def test_abort_kills_the_launcher_without_an_exit_report(self) -> None:
        """An owned kill marks the exit owned, so no daemon-exit event is emitted."""
        env = object.__new__(Environment)
        env._sink = InMemorySink()
        launcher = _monitored_launcher(env)
        self.addCleanup(launcher.kill)
        env._daemon_pid = launcher.pid
        env.abort()
        self.assertEqual(-signal.SIGKILL, launcher.returncode)
        time.sleep(0.1)
        self.assertEqual([], env._sink.of_type(INFO))

    def test_a_vanished_scope_reports_its_oom_kill_result(self) -> None:
        """A setup OOM that removed the scope's cgroup is read from the unit's Result.

        The failed unit is reset afterwards. Twin: any other Result is not degraded.
        """
        cgroup = environment_module.Path("/sys/fs/cgroup/absent/run-r42.scope")
        for result, degraded in (("oom-kill", True), ("exit-code", False)):
            with self.subTest(result=result):
                env = object.__new__(Environment)
                env._setup_facts = {}
                env._memory_cgroup = cgroup
                shown = subprocess.CompletedProcess([], 0, f"{result}\n", "")
                error = RuntimeError("daemon went away")
                with mock.patch.object(
                    environment_module.subprocess, "run", return_value=shown
                ) as run:
                    if degraded:
                        with self.assertRaises(target_module.EnvironmentDegraded):
                            env._raise_if_setup_oom(error)
                    else:
                        env._raise_if_setup_oom(error)
                self.assertEqual(
                    [
                        [
                            "systemctl",
                            "--user",
                            "show",
                            "run-r42.scope",
                            "-p",
                            "Result",
                            "--value",
                        ],
                        ["systemctl", "--user", "reset-failed", "run-r42.scope"],
                    ],
                    [call.args[0] for call in run.call_args_list],
                )

    def test_aged_overlay_uses_distinct_revisions_and_counts_completed_pairs(
        self,
    ) -> None:
        """Aging counts only successful away-and-back updates to distinct nodes."""
        env = object.__new__(Environment)
        env._aged_overlay = 3
        env._commits = tuple(
            AuthoredCommit(node, revision)
            for revision, node in enumerate(("old-0", "old-1", "old-2", "tip"))
        )
        env._setup_facts = {}
        repo = _RecordingRepo()
        env._mount_repo = cast(Any, repo)
        env._age_overlay()
        self.assertEqual(
            [
                ("goto", "old-0"),
                ("goto", "tip"),
                ("goto", "old-1"),
                ("goto", "tip"),
                ("goto", "old-2"),
                ("goto", "tip"),
            ],
            repo.calls,
        )
        self.assertEqual(3, env._setup_facts["aged_overlay_round_trips"])

        failing = object.__new__(Environment)
        failing._aged_overlay = 3
        failing._commits = env._commits
        failing._setup_facts = {}
        failing._mount_repo = cast(Any, _RecordingRepo(fail_at=4))
        with self.assertRaisesRegex(RuntimeError, "update failed"):
            failing._age_overlay()
        self.assertEqual(1, failing._setup_facts["aged_overlay_round_trips"])

    def test_setup_load_order_is_age_materialize_scatter_then_warm(self) -> None:
        """Aging precedes working-copy setup and directory warming runs last."""
        env = object.__new__(Environment)
        env._aged_overlay = 1
        env._materialize = True
        env._scatter_dirty = 1
        env._warm_readdir_enabled = True
        env._setup_facts = {}
        calls: list[str] = []
        with (
            mock.patch.object(
                env, "_age_overlay", side_effect=lambda: calls.append("age")
            ),
            mock.patch.object(
                env,
                "_materialize_working_copy",
                side_effect=lambda: calls.append("materialize"),
            ),
            mock.patch.object(
                env,
                "_scatter_working_copy",
                side_effect=lambda: calls.append("scatter"),
            ),
            mock.patch.object(
                env, "_warm_readdir", side_effect=lambda: calls.append("warm")
            ),
            mock.patch.object(time, "monotonic", side_effect=(1.0, 1.5, 2.0, 2.25)),
        ):
            env._prepare_working_copy()
        self.assertEqual(["age", "materialize", "scatter", "warm"], calls)
        self.assertEqual(500, env._setup_facts["materialize_ms"])
        self.assertEqual(250, env._setup_facts["warm_readdir_ms"])

    def test_daemon_exit_recorded(self) -> None:
        """An exit the environment did not request reaches the sink; an owned one does not."""

        def monitored(owned: bool) -> tuple[subprocess.Popen[str], InMemorySink]:
            env = object.__new__(Environment)
            env._sink = InMemorySink()
            launcher = _monitored_launcher(env)
            if owned:
                env._owned_exit.set()
            os.kill(launcher.pid, signal.SIGKILL)
            return launcher, env._sink

        _launcher, sink = monitored(owned=False)
        event = _first_info_event(sink)
        self.assertEqual(
            {"reason": "daemon-exit", "returncode": -9, "signal": "SIGKILL"},
            {key: event.data[key] for key in ("reason", "returncode", "signal")},
        )
        self.assertIsInstance(event.time, float)

        started: list[threading.Thread] = []
        real_thread = threading.Thread

        def capture(*args: Any, **kwargs: Any) -> threading.Thread:
            started.append(real_thread(*args, **kwargs))
            return started[-1]

        with mock.patch.object(threading, "Thread", side_effect=capture):
            owned_launcher, owned_sink = monitored(owned=True)
        (monitor,) = started
        monitor.join(timeout=HANG_GUARD)
        self.assertFalse(monitor.is_alive())
        self.assertEqual(-signal.SIGKILL, owned_launcher.wait(timeout=HANG_GUARD))
        self.assertEqual([], owned_sink.events)

    def test_daemon_exit_reported_when_a_probe_fails(self) -> None:
        """A failing core-path probe still yields the exit event, error recorded."""
        env = object.__new__(Environment)
        env._sink = InMemorySink()
        with mock.patch.object(
            environment_module, "_core_path", side_effect=OSError("core_pattern")
        ):
            launcher = _monitored_launcher(env)
            os.kill(launcher.pid, signal.SIGKILL)
            event = _first_info_event(env._sink)
        self.assertEqual("daemon-exit", event.data["reason"])
        self.assertEqual(
            (-9, "SIGKILL"), (event.data["returncode"], event.data["signal"])
        )
        self.assertIsNone(event.data["core_path"])
        self.assertEqual("OSError: core_pattern", event.data["error"])

    def test_exit_report_reads_the_signal_from_the_tail_on_a_clean_exit_code(
        self,
    ) -> None:
        """A launcher exiting 1 names the signal its edenfs child's banner reports."""
        banner = "*** Signal 6 (SIGABRT) (0x0) received by PID 4242, stack trace: ***"
        with (
            mock.patch.object(environment_module, "_log_tail", return_value=[banner]),
            mock.patch.object(environment_module, "_core_path", return_value=None),
        ):
            report = environment_module._exit_report(1)
        self.assertEqual(
            ("daemon-exit", 1, "SIGABRT", [banner]),
            (
                report["reason"],
                report["returncode"],
                report["signal"],
                report["log_tail"],
            ),
        )
        self.assertNotIn("error", report)

    def test_log_tail_anchors_on_folly_signal_banner(self) -> None:
        """A fatal-signal banner deeper than 50 lines from EOF heads the tail."""
        banner = (
            "*** Signal 11 (SIGSEGV) (0x0) received by PID 4242 (pthread TID 0x1)"
            " (linux TID 4242) (code: sent by kill, sigsend or raise), stack trace: ***"
        )
        lines = [f"I0101 00:00:00 boot line {i}" for i in range(10)]
        lines += ["*** Aborted at 1790101846 (Unix time) ***", banner]
        lines += [f"    @ {i:016x} frame_{i}" for i in range(70)]
        tail = _tail_of_stderr(lines)
        self.assertEqual(banner, tail[0])
        self.assertEqual(50, len(tail))

    def test_log_tail_anchors_on_the_last_crash_signature(self) -> None:
        """A non-fatal EDEN_BUG line far above the fatal banner does not hide it."""
        banner = "*** Signal 11 (SIGSEGV) (0x0) received by PID 4242, stack trace: ***"
        lines = ["I0101 00:00:00 boot", "E0101 00:00:01 EDEN_BUG: non-fatal check"]
        lines += [f"I0101 00:01:{i:02d} steady state {i}" for i in range(60)]
        lines += [banner] + [f"    @ {i:016x} frame_{i}" for i in range(20)]
        tail = _tail_of_stderr(lines)
        self.assertEqual(banner, tail[0])
        self.assertEqual(21, len(tail))

    def test_log_tail_without_a_crash_is_the_last_50_lines(self) -> None:
        """With no crash signature the tail is the end of the log."""
        lines = [f"I0101 00:00:00 line {i}" for i in range(80)]
        self.assertEqual(lines[30:], _tail_of_stderr(lines))

    def test_daemon_signal_read_from_tail_banner(self) -> None:
        """The folly banner in an already-captured tail names the daemon's signal."""
        with_banner = [
            "*** Aborted at 1790101846 (Unix time) ***",
            "*** Signal 11 (SIGSEGV) (0x0) received by PID 4242 (pthread TID 0x1),"
            " stack trace: ***",
            "    @ 000000000aac0d60 _start",
        ]
        self.assertEqual("SIGSEGV", environment_module._signal_from_tail(with_banner))
        self.assertIsNone(
            environment_module._signal_from_tail(["I0101 quiet exit", "goodbye"])
        )
        self.assertIsNone(environment_module._signal_from_tail([]))

    def test_fatal_eden_bug_flag_follows_supplied_fact(self) -> None:
        """The daemon flag is passed only when the supplied fact is true."""
        for supplied, expected in ((None, False), ("false", False), ("true", True)):
            with self.subTest(supplied=supplied):
                _env, args = self._with_fatal_eden_bug(supplied)
                self.assertIn("--enable_fault_injection", args)
                self.assertEqual(expected, "--fatal_eden_bug" in args)

    def test_setup_records_fatal_eden_bug(self) -> None:
        """The recorded setup fact equals the supplied value, false when absent."""
        for supplied, expected in ((None, False), ("false", False), ("true", True)):
            with self.subTest(supplied=supplied):
                env, _args = self._with_fatal_eden_bug(supplied)
                self.assertIs(expected, env._setup_facts["fatal_eden_bug"])

    def test_fatal_eden_bug_rejects_non_boolean_text(self) -> None:
        """A supplied fact that is not JSON true or false refuses setup."""
        with self.assertRaisesRegex(RuntimeError, "EDENFS_DAEMON_FATAL_EDEN_BUG"):
            self._with_fatal_eden_bug("1")

    def test_setup_oom_is_environment_degraded(self) -> None:
        """A typed setup OOM preserves facts and never starts actions."""

        class SetupDegradedEnvironment:
            def setup(self) -> None:
                raise target_module.EnvironmentDegraded(
                    "daemon-oom-killed",
                    {
                        "memory": {
                            "max_bytes": 1048576,
                            "current_bytes": 0,
                            "peak_bytes": 1048576,
                            "events": {"oom_kill": 1},
                        }
                    },
                )

        spec = single_action_spec(getattr_action(), environment_target())
        environment = SetupDegradedEnvironment()
        backend_created = False

        def make_backend(*_args: Any) -> Any:
            nonlocal backend_created
            backend_created = True
            raise AssertionError("backend created after setup degradation")

        target = replace(
            RECORDING_TARGET,
            make_environment=lambda _spec, _sink: environment,
            make_backend=make_backend,
        )
        with TempFileManager() as manager:
            spec_file = manager.make_temp_file("spec.json")
            spec_file.write(to_json(spec))
            spec_file.flush()
            stdout = io.StringIO()
            with redirect_stdout(stdout), redirect_stderr(io.StringIO()):
                returncode = containment.runner_main(spec_file.name, target)

        events = [
            event_from_json(json.loads(line)) for line in stdout.getvalue().splitlines()
        ]
        self.assertEqual(containment.ENVIRONMENT_DEGRADED_EXIT, returncode)
        self.assertEqual(
            (containment.ENVIRONMENT_DEGRADED, "daemon-oom-killed"),
            containment._final_status(events[-1]),
        )
        self.assertFalse(backend_created)
        self.assertEqual(
            1, events[-1].data["unwind"]["setup"]["memory"]["events"]["oom_kill"]
        )

    def test_a_cgroup_oom_kill_during_setup_raises_environment_degraded(self) -> None:
        """Setup errors become EnvironmentDegraded only when the cgroup counted an OOM kill."""
        with TempFileManager() as manager:
            cgroup = manager.make_temp_dir(prefix="cgroup.")
            for name, text in (
                ("memory.max", "1048576\n"),
                ("memory.current", "0\n"),
                ("memory.peak", "1048576\n"),
            ):
                (cgroup / name).write_text(text)
            for oom_kills, degraded in ((0, False), (1, True)):
                with self.subTest(oom_kills=oom_kills):
                    (cgroup / "memory.events").write_text(
                        f"low 0\nhigh 0\nmax 3\noom 1\noom_kill {oom_kills}\n"
                    )
                    env = object.__new__(Environment)
                    env._setup_facts = {}
                    env._memory_cgroup = cgroup
                    error = RuntimeError("daemon went away")
                    if degraded:
                        with self.assertRaises(
                            target_module.EnvironmentDegraded
                        ) as raised:
                            env._raise_if_setup_oom(error)
                        self.assertIs(error, raised.exception.__cause__)
                    else:
                        env._raise_if_setup_oom(error)
                    self.assertEqual(
                        (1048576, oom_kills),
                        (
                            env._setup_facts["memory"]["max_bytes"],
                            env._setup_facts["memory"]["events"]["oom_kill"],
                        ),
                    )
