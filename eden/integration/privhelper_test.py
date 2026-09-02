#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import signal
import subprocess
import sys
from pathlib import Path
from typing import Dict, List, Optional

from eden.fs.cli import util

from .lib import testcase


@testcase.eden_repo_test
class PrivHelperMemoryPriorityTest(testcase.EdenRepoTest):
    """Verify the privhelper process is protected from the OOM killer."""

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        result = super().edenfs_extra_config() or {}
        # The privhelper in this test runs unprivileged, so it can only
        # *raise* oom_score_adj values; use a positive value.
        result.setdefault("core", []).append(
            'priv-helper-target-memory-priority = "300"'
        )
        return result

    async def test_privhelper_oom_score_adj_is_set(self) -> None:
        if sys.platform != "linux":
            self.skipTest("oom_score_adj is a Linux concept")

        async with self.get_async_thrift_client() as client:
            info = await client.checkPrivHelper()
        self.assertTrue(info.connected, "privhelper should be connected")
        self.assertGreater(info.pid, 0, "privhelper pid should be known")

        score = int(Path(f"/proc/{info.pid}/oom_score_adj").read_text().strip())
        self.assertEqual(
            300,
            score,
            "privhelper oom_score_adj should match "
            "core:priv-helper-target-memory-priority",
        )


@testcase.eden_repo_test
class PrivHelperDeathTest(testcase.EdenRepoTest):
    """Exercise EdenFS behavior when the privhelper process dies."""

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    def privhelper_connected(self) -> bool:
        with self.eden.get_thrift_client() as client:
            return client.checkPrivHelper().connected

    def kill_privhelper(self) -> None:
        """Kill the privhelper and wait until the daemon has noticed."""
        with self.eden.get_thrift_client() as client:
            info = client.checkPrivHelper()
        self.assertTrue(info.connected, "privhelper should start out connected")
        self.assertGreater(info.pid, 0)
        os.kill(info.pid, signal.SIGKILL)
        # From here on the daemon cannot shut down cleanly; register the
        # workaround shutdown now (cleanups run in LIFO order, so this runs
        # before the harness's own cleanup) so it also covers the case
        # where an assertion below fails.
        self.addCleanup(self.shutdown_with_dead_privhelper)
        util.poll_until(
            lambda: True if not self.privhelper_connected() else None,
            timeout=15,
        )

    def test_doctor_reports_dead_privhelper(self) -> None:
        self.kill_privhelper()

        proc = self.eden.run_unchecked(
            "doctor",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            encoding="utf-8",
        )
        self.assertNotEqual(0, proc.returncode, "doctor should report a problem")
        output = proc.stdout + proc.stderr
        self.assertIn("PrivHelper process is not accessible", output)
        self.assertIn("eden restart", output)

    def shutdown_with_dead_privhelper(self) -> None:
        # Shutting down a daemon whose privhelper is dead reports failure:
        # EdenFS cannot unmount its checkouts, so the daemon exits non-zero.
        # Absorb that here (retry=True) instead of letting the harness's
        # cleanup treat the non-zero exit as a test error. This documents
        # today's expected behavior; a daemon that can recover from
        # privhelper death would shut down cleanly instead.
        self.eden.shutdown(retry=True)
