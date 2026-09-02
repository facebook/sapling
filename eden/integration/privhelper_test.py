#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import sys
from pathlib import Path
from typing import Dict, List, Optional

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
