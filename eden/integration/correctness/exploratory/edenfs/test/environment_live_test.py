# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The EdenFS environment fixture with a real daemon: close (including after SIGKILL),
fixture bytes, case-insensitive mounts, scatter dirt, materialization, overlay aging,
directory warming, the reference clone, and the memory limit. Each run is judged by
`EDENFS_TARGET`.
"""

from __future__ import annotations

import json
import os
import re
import threading
import unittest
from typing import Any, cast

from eden.fs.service.eden.thrift_types import (
    FaultDefinition,
    GetBlockedFaultsRequest,
    RemoveFaultArg,
    UnblockFaultArg,
)
from eden.integration.correctness.exploratory.core.checker import ACCEPT
from eden.integration.correctness.exploratory.core.containment import (
    COMPLETED,
    load_history,
)
from eden.integration.correctness.exploratory.core.history import History, InMemorySink
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    DEADLINE_OPERATION,
    RunSpec,
)
from eden.integration.correctness.exploratory.edenfs.encoding import (
    encode_committed,
    initial_content,
)
from eden.integration.correctness.exploratory.edenfs.environment import (
    CHURN,
    Environment,
)
from eden.integration.correctness.exploratory.edenfs.target import RECORDING_TARGET
from eden.integration.correctness.exploratory.edenfs.test.backends_fixtures import (
    _artifact_directory,
    _fail_on_setup_failure,
    _require_live,
)
from eden.integration.correctness.exploratory.edenfs.test.environment_fixtures import (
    environment_target,
    getattr_action,
    HANG_GUARD,
    REGISTER,
    SEED,
    single_action_spec,
)
from eden.integration.correctness.exploratory.edenfs.test.live import run_contained


_REGISTERS = (REGISTER, "tree/registers/r1.reg")


def _sl_status() -> Action:
    return Action(
        id="s1", kind="status", deadline=DEADLINE_OPERATION, payload={"channel": "sl"}
    )


def _pid_is_dead(pid: int) -> bool:
    """Return True when ``pid`` no longer names a live process."""
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return True
    except PermissionError:
        return False
    return False


class EnvironmentLiveTest(unittest.TestCase):
    """Environment behavior against a real daemon."""

    def _make_env(self, **kwargs: Any) -> Environment:
        _require_live(self)
        env = Environment(
            seed=SEED, registers=_REGISTERS, sink=InMemorySink(), **kwargs
        )
        self.addCleanup(env.close)
        env.setup()
        return env

    def _run(self, spec: RunSpec, prefix: str) -> tuple[dict[str, Any], History, str]:
        """Run a spec; return its execution manifest, history, and stderr."""
        _require_live(self)
        with _artifact_directory(prefix) as artifact_dir:
            result = run_contained(spec, artifact_dir)
            _fail_on_setup_failure(result, artifact_dir)
            self.assertEqual(COMPLETED, result.execution.status, result.execution)
            self.assertEqual(ACCEPT, result.verdict.kind, result.verdict)
            with open(os.path.join(artifact_dir, "execution.json")) as handle:
                execution = json.load(handle)
            with open(os.path.join(artifact_dir, "stderr.log")) as handle:
                stderr = handle.read()
            history = History(
                spec,
                load_history(os.path.join(artifact_dir, "history.jsonl")),
                RECORDING_TARGET.actions,
            )
        return execution, history, stderr

    def test_close_leaves_no_daemon_behind(self) -> None:
        env = self._make_env()
        pid = env.daemon_pid
        assert pid is not None
        self.assertIsNone(env.close())
        self.assertTrue(_pid_is_dead(pid), f"daemon pid {pid} survived close")

    def test_close_after_kill_leaves_no_daemon_behind(self) -> None:
        env = self._make_env()
        pid = env.daemon_pid
        assert pid is not None
        env.abort()
        self.assertIsNone(env.close())
        self.assertTrue(_pid_is_dead(pid), f"daemon pid {pid} survived kill+close")

    def test_fixture_bytes_every_revision_and_through_fuse(self) -> None:
        """Registers hold each revision's image, churn is revision-stable, both 64 bytes via FUSE."""
        env = self._make_env()
        for commit in env.commits:
            for path in (*_REGISTERS, *CHURN):
                with self.subTest(revision=commit.revision, path=path):
                    expected = (
                        encode_committed(SEED, path, commit.revision)
                        if path in _REGISTERS
                        else initial_content(SEED, path)
                    )
                    output = env.backing_repo.hg("cat", "-r", commit.node, path)
                    self.assertEqual(expected, output.encode())
        head = env.mount_repo.get_head_hash()
        [revision] = [c.revision for c in env.commits if c.node == head]
        for path in (*_REGISTERS, *CHURN):
            with self.subTest(fuse=path):
                data = (env.mount_path / path).read_bytes()
                self.assertEqual(64, len(data))
                expected = (
                    encode_committed(SEED, path, revision)
                    if path in _REGISTERS
                    else initial_content(SEED, path)
                )
                self.assertEqual(expected, data)

    def test_thrift_client_answers(self) -> None:
        env = self._make_env()
        with env.get_thrift_client() as client:
            info = cast(Any, client).getDaemonInfo()
            self.assertEqual(env.daemon_pid, info.pid)

    def test_case_insensitive_mount(self) -> None:
        """An uppercase register path resolves on a case-insensitive mount."""
        register = REGISTER.upper()
        spec = single_action_spec(
            getattr_action(register),
            environment_target(case_sensitive=False, register=register),
        )
        _execution, history, _stderr = self._run(spec, "case-insensitive.")
        self.assertEqual(64, history.facts("g1")["size"])

    def test_scatter_dirty_visible_to_status(self) -> None:
        """Setup dirt is visible to status and never touches registers or churn."""
        spec = single_action_spec(_sl_status(), environment_target(scatter_dirty=20))
        _execution, history, _stderr = self._run(spec, "scatter-dirty.")
        status = [entry["path"] for entry in history.facts("s1")["status"]]
        self.assertEqual(20, len(status))
        self.assertFalse(set(status) & {REGISTER, *CHURN})

    def test_materialize_completes(self) -> None:
        """A materializing run records its completed setup duration."""
        spec = single_action_spec(
            getattr_action(), environment_target(materialize=True)
        )
        execution, _history, _stderr = self._run(spec, "materialize.")
        self.assertGreaterEqual(execution["setup"]["materialize_ms"], 0)

    def test_aged_overlay_round_trips_restore_tip_before_other_setup(self) -> None:
        """Three successful distinct-revision round trips restore the original tip."""
        spec = single_action_spec(
            Action(
                id="s1",
                kind="status",
                deadline=DEADLINE_OPERATION,
                payload={"channel": "thrift"},
            ),
            environment_target(
                file_count=4,
                materialize=True,
                scatter_dirty=1,
                warm_readdir=True,
                aged_overlay=3,
            ),
        )
        execution, history, stderr = self._run(spec, "aged-overlay.")
        self.assertEqual(3, execution["setup"]["aged_overlay_round_trips"])
        self.assertGreaterEqual(execution["setup"]["materialize_ms"], 0)
        self.assertGreaterEqual(execution["setup"]["warm_readdir_ms"], 0)
        tip = next(
            line.split()[-1]
            for line in stderr.splitlines()
            if line.startswith("live-ready commits=")
        )
        self.assertEqual(tip, history.facts("s1")["parent"])

    def test_reference_clone_created(self) -> None:
        """`reference_clone` adds a plain sl clone at the mount's commit under the run dir."""
        env = self._make_env(reference_clone=True)
        facts = env.setup_facts["reference"]
        reference = facts["path"]
        self.assertTrue(reference.startswith(f"{env.run_dir}/"), reference)
        self.assertEqual(reference, str(env.reference.root))
        self.assertTrue(os.path.isdir(os.path.join(reference, ".hg")))
        self.assertFalse(os.path.ismount(reference))
        self.assertEqual(env.mount_repo.get_head_hash(), facts["node"])
        self.assertEqual("0022", facts["umask"])

    def test_warm_readdir_avoids_a_later_lookup_fault(self) -> None:
        """A setup-warmed dentry completes without hitting an armed lookup block."""
        spec = single_action_spec(
            getattr_action(), environment_target(file_count=4, warm_readdir=True)
        )
        execution, _history, _stderr = self._run(spec, "warm-readdir.")
        self.assertGreaterEqual(execution["setup"]["warm_readdir_ms"], 0)

        env = Environment(
            seed=SEED,
            file_count=4,
            registers=_REGISTERS,
            warm_readdir=True,
            sink=InMemorySink(),
        )
        self.addCleanup(env.close, True)
        env.setup()
        key_class = "FuseDispatcherImpl::lookup"
        selector = re.escape(REGISTER.rsplit("/", 1)[-1])
        reader_done = threading.Event()
        read_result: dict[str, bytes | Exception] = {}

        def read_warm_path() -> None:
            try:
                read_result["data"] = (env.mount_path / REGISTER).read_bytes()
            except OSError as error:
                read_result["error"] = error
            finally:
                reader_done.set()

        with env.get_thrift_client() as raw_client:
            cast(Any, raw_client).injectFault(
                FaultDefinition(
                    keyClass=key_class, keyValueRegex=selector, count=1, block=True
                )
            )
        worker = threading.Thread(target=read_warm_path, daemon=True)
        worker.start()
        blocked: list[str] = []
        try:
            self.assertTrue(
                reader_done.wait(HANG_GUARD), "warm read blocked on dispatcher lookup"
            )
            self.assertNotIn("error", read_result)
            with env.get_thrift_client() as raw_client:
                blocked = list(
                    cast(Any, raw_client)
                    .getBlockedFaults(GetBlockedFaultsRequest(keyclass=key_class))
                    .keyValues
                )
        finally:
            with env.get_thrift_client() as raw_client:
                client = cast(Any, raw_client)
                removed = client.removeFault(
                    RemoveFaultArg(keyClass=key_class, keyValueRegex=selector)
                )
                client.unblockFault(
                    UnblockFaultArg(keyClass=key_class, keyValueRegex=selector)
                )
                after = list(
                    client.getBlockedFaults(
                        GetBlockedFaultsRequest(keyclass=key_class)
                    ).keyValues
                )
            worker.join(HANG_GUARD)
        self.assertFalse(worker.is_alive())
        self.assertEqual([], blocked)
        self.assertTrue(removed, "the exact injected registration was not removed")
        self.assertEqual([], after)
        self.assertEqual(64, len(cast(bytes, read_result["data"])))

    def test_memory_max_applied(self) -> None:
        """The daemon cgroup reports the requested 4096 MiB limit."""
        spec = single_action_spec(
            getattr_action(), environment_target(memory_max_mb=4096)
        )
        execution, _history, _stderr = self._run(spec, "memory-max.")
        self.assertEqual(4294967296, execution["setup"]["memory"]["max_bytes"])

    def test_no_limit_when_absent(self) -> None:
        """An omitted memory limit leaves the daemon cgroup unlimited."""
        spec = single_action_spec(getattr_action(), environment_target())
        execution, _history, _stderr = self._run(spec, "memory-unlimited.")
        self.assertEqual("max", execution["setup"]["memory"]["max_bytes"])

    def test_memory_peak_recorded(self) -> None:
        """A 1,000-file materialization records positive peak cgroup usage."""
        spec = single_action_spec(
            getattr_action(), environment_target(file_count=1000, materialize=True)
        )
        execution, _history, _stderr = self._run(spec, "memory-peak.")
        peak_bytes = execution["setup"]["memory"]["peak_bytes"]
        self.assertIs(type(peak_bytes), int)
        self.assertGreater(peak_bytes, 0)
