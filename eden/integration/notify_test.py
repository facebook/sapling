#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import asyncio
import json
import platform
import subprocess
import sys
import time
import typing as t

from .lib import testcase


@testcase.eden_repo_test
class NotifyTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    def setUp(self) -> None:
        super().setUp()
        if platform.system() == "Windows" and not getattr(self, "loop", None):
            # This is required on Windows
            # pyre-ignore[16]: Windows only
            self.loop = asyncio.ProactorEventLoop()
            asyncio.set_event_loop(self.loop)

    def tearDown(self) -> None:
        if getattr(self, "loop", None):
            self.loop.close()
        super().tearDown()

    async def subscribe(self) -> asyncio.subprocess.Process:
        cmd, env = self.eden.get_edenfsctl_cmd_env(
            "notify",
            "changes-since",
            "--subscribe",
            "--throttle",
            "5",
            "--json",
            self.mount,
        )
        env["EDENFS_LOG"] = "edenfs=trace"

        return await asyncio.create_subprocess_exec(
            *cmd,
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            cwd=self.mount,
        )

    async def next_event(
        self, sub: asyncio.subprocess.Process, timeout: int = 5
    ) -> t.Optional[t.Dict[str, t.Any]]:
        """Wait for the next event generated from the subscription.

        We need asyncio here so we can set a timeout. Otherwise we will need to
        manually select.
        """
        stdout = sub.stdout
        if not stdout:
            raise RuntimeError("no stdout captured")

        try:
            line = await asyncio.wait_for(stdout.readline(), timeout=timeout)
        except asyncio.TimeoutError:
            return None
        try:
            # Newline separates each json event
            await asyncio.wait_for(stdout.readline(), timeout=timeout)
        except asyncio.TimeoutError:
            return None
        if not line or line == b"\n":
            return None
        try:
            return json.loads(line.decode())
        except json.JSONDecodeError:
            print(f"failed to decode: {line}", file=sys.stderr)
            raise

    async def wait_for_next_event(
        self,
        sub: asyncio.subprocess.Process,
        action: t.Callable[[], None] = lambda: None,
        attempt: int = 10,
    ) -> t.Optional[t.Dict[str, t.Any]]:
        """Pull until the next event generated from the subscription

        When the system is under heavy load or when we are testing dev mode
        binary, the subscription may finish initializing after the write event
        is already done by the test. So we retry a few times to make sure the
        subscription sees it.
        """
        for _ in range(attempt):
            time.sleep(0.1)
            action()
            event = await self.next_event(sub)
            if event:
                return event
        return None

    async def test_debug_subscribe(self) -> None:
        subscription = await self.subscribe()

        event = await self.wait_for_next_event(subscription)

        self.assertIsNotNone(event)
        self.assertIsNotNone(event["to_position"])
        self.assertIsNotNone(event["to_position"]["mount_generation"])
        self.assertIsNotNone(event["to_position"]["sequence_number"])
        self.assertIsNotNone(event["to_position"]["snapshot_hash"])
        self.assertListEqual(event["changes"], [])

        event = await self.wait_for_next_event(
            subscription, lambda: self.write_file("hello", "test")
        )

        self.assertIsNotNone(event)
        self.assertIsNotNone(event["to_position"])
        self.assertIsNotNone(event["to_position"]["mount_generation"])
        self.assertIsNotNone(event["to_position"]["sequence_number"])
        self.assertIsNotNone(event["to_position"]["snapshot_hash"])
        self.assertListEqual(
            event["changes"],
            [
                {
                    "SmallChange": {
                        "Modified": {
                            "file_type": "Regular",
                            "path": [104, 101, 108, 108, 111],
                        }
                    }
                }
            ],
        )
