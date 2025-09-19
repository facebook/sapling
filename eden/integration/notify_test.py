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
    git_test_supported = False

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
        self.mkdir(".edenfs-notifications-state")
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

    async def subscribe_with_states(self, states) -> asyncio.subprocess.Process:
        self.mkdir(".edenfs-notifications-state")
        for state in states:
            self.mkdir(f".edenfs-notifications-state/{state}")
        args = [
            "notify",
            "changes-since",
            "--subscribe",
            "--throttle",
            "5",
            "--json",
            self.mount,
        ]
        for state in states:
            args.append("--states")
            args.append(state)
        cmd, env = self.eden.get_edenfsctl_cmd_env(*args)
        env["EDENFS_LOG"] = "edenfs=trace"

        return await asyncio.create_subprocess_exec(
            *cmd,
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            cwd=self.mount,
        )

    async def enter_state(self, state, duration=30) -> asyncio.subprocess.Process:
        # set default duration to prevent accidental spinlocks
        args = [
            "notify",
            "enter-state",
            state,
            self.mount,
        ]
        if duration:
            args.append("--duration")
            args.append(str(duration))
        cmd, env = self.eden.get_edenfsctl_cmd_env(*args)
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

    async def test_debug_subscribe_with_states(self) -> None:
        subscription = await self.subscribe_with_states(["hello"])
        event = await self.wait_for_next_event(subscription)
        self.assertIsNotNone(event)
        self.assertListEqual(event["changes"], [])

        self.write_file("hello2", "test")
        time.sleep(1)
        event = await self.wait_for_next_event(subscription)
        self.assertIsNotNone(event)
        self.assertTrue(
            {
                "SmallChange": {
                    "Added": {
                        "file_type": "Regular",
                        "path": [104, 101, 108, 108, 111, 50],
                    }
                }
            }
            in event["changes"],
            msg=f"changes: {event['changes']}",
        )

        state_process = await self.enter_state("hello")
        time.sleep(1)
        event = await self.wait_for_next_event(subscription)
        if event and "event_type" not in event:
            # Sometimes the Modified change from the previous change gets chunked
            # into a separate event from Added. Read it and bypass.
            event = await self.wait_for_next_event(subscription)
        self.assertIsNotNone(event)
        self.assertEqual(event["event_type"], "Entered", msg=f"event: {event}")
        self.assertEqual(event["state"], "hello", msg=f"event: {event}")

        # expect this to return None since the state is asserted
        self.write_file("hello3", "test")
        time.sleep(1)
        self.rename("hello3", "hello4")
        time.sleep(1)
        if sys.platform == "linux":
            self.rm("hello4")
        time.sleep(1)
        event = await self.wait_for_next_event(subscription, attempt=1)
        self.assertIsNone(event)

        # exit the state
        state_process.terminate()
        event = await self.wait_for_next_event(subscription)
        self.assertIsNotNone(event)
        self.assertEqual(event["event_type"], "Left", msg=f"event: {event}")
        self.assertEqual(event["state"], "hello", msg=f"event: {event}")

        # Get deferred changes
        changes = []
        while event is not None:
            event = await self.wait_for_next_event(subscription, attempt=1)
            if event is None:
                break
            changes.extend(event["changes"])
        if sys.platform != "win32":
            self.assertTrue(
                {
                    "SmallChange": {
                        "Added": {
                            "file_type": "Regular",
                            "path": [104, 101, 108, 108, 111, 51],
                        }
                    }
                }
                in changes,
                msg=f"changes: {changes}",
            )
            self.assertTrue(
                {
                    "SmallChange": {
                        "Renamed": {
                            "file_type": "Regular",
                            "from": [104, 101, 108, 108, 111, 51],
                            "to": [104, 101, 108, 108, 111, 52],
                        }
                    }
                }
                in changes,
                msg=f"changes: {changes}",
            )
            # Rm behavior is different on Linux and Mac
            # Mac will sometimes rename the file to a nfs.date.xxxx and then delete it
            if sys.platform == "linux":
                self.assertTrue(
                    {
                        "SmallChange": {
                            "Removed": {
                                "file_type": "Regular",
                                "path": [104, 101, 108, 108, 111, 52],
                            }
                        }
                    }
                    in changes,
                    msg=f"changes: {changes}",
                )
        else:
            # Windows has no rename, only add/remove
            self.assertTrue(
                {
                    "SmallChange": {
                        "Added": {
                            "file_type": "Regular",
                            "path": [104, 101, 108, 108, 111, 51],
                        }
                    }
                }
                in changes,
                msg=f"changes: {changes}",
            )
            self.assertTrue(
                {
                    "SmallChange": {
                        "Removed": {
                            "file_type": "Regular",
                            "path": [104, 101, 108, 108, 111, 51],
                        }
                    }
                }
                in changes,
                msg=f"changes: {changes}",
            )
            self.assertTrue(
                {
                    "SmallChange": {
                        "Added": {
                            "file_type": "Regular",
                            "path": [104, 101, 108, 108, 111, 52],
                        }
                    }
                }
                in changes,
                msg=f"changes: {changes}",
            )

    async def test_debug_subscribe_with_multiple_states(self) -> None:
        subscription = await self.subscribe_with_states(["hello", "goodbye"])
        event = await self.wait_for_next_event(subscription)
        self.assertIsNotNone(event)
        self.assertListEqual(event["changes"], [])

        self.write_file("hello2", "test")
        time.sleep(1)
        event = await self.wait_for_next_event(subscription)
        self.assertIsNotNone(event)
        self.assertTrue(
            {
                "SmallChange": {
                    "Added": {
                        "file_type": "Regular",
                        "path": [104, 101, 108, 108, 111, 50],
                    }
                }
            }
            in event["changes"],
            msg=f"changes: {event['changes']}",
        )

        hello_process = await self.enter_state("hello")
        time.sleep(1)
        event = await self.wait_for_next_event(subscription)
        if event and "event_type" not in event:
            # Sometimes the Modified change from the previous change gets chunked
            # into a separate event from Added. Read it and bypass.
            event = await self.wait_for_next_event(subscription)
        self.assertIsNotNone(event)
        self.assertEqual(event["event_type"], "Entered", msg=f"event: {event}")
        self.assertEqual(event["state"], "hello", msg=f"event: {event}")

        # expect this to return None since the state is asserted
        self.write_file("hello3", "test")
        time.sleep(1)
        self.rename("hello3", "hello4")
        time.sleep(1)
        if sys.platform == "linux":
            self.rm("hello4")
        time.sleep(1)
        event = await self.wait_for_next_event(subscription, attempt=1)
        self.assertIsNone(event)

        goodbye_process = await self.enter_state("goodbye")
        time.sleep(1)
        event = await self.wait_for_next_event(subscription)
        self.assertIsNotNone(event)
        self.assertEqual(event["event_type"], "Entered", msg=f"event: {event}")
        self.assertEqual(event["state"], "goodbye", msg=f"event: {event}")

        # exit the first state
        hello_process.terminate()
        event = await self.wait_for_next_event(subscription)
        self.assertIsNotNone(event)
        self.assertEqual(event["event_type"], "Left", msg=f"event: {event}")
        self.assertEqual(event["state"], "hello", msg=f"event: {event}")

        # expect this to return None since the second state is asserted
        event = await self.wait_for_next_event(subscription, attempt=1)
        self.assertIsNone(event)

        # exit the second state
        goodbye_process.terminate()
        event = await self.wait_for_next_event(subscription)
        self.assertIsNotNone(event)
        self.assertEqual(event["event_type"], "Left", msg=f"event: {event}")
        self.assertEqual(event["state"], "goodbye", msg=f"event: {event}")

        # Get deferred changes
        changes = []
        while event is not None:
            event = await self.wait_for_next_event(subscription, attempt=1)
            if event is None:
                break
            changes.extend(event["changes"])
        if sys.platform != "win32":
            self.assertTrue(
                {
                    "SmallChange": {
                        "Added": {
                            "file_type": "Regular",
                            "path": [104, 101, 108, 108, 111, 51],
                        }
                    }
                }
                in changes,
                msg=f"changes: {changes}",
            )
            self.assertTrue(
                {
                    "SmallChange": {
                        "Renamed": {
                            "file_type": "Regular",
                            "from": [104, 101, 108, 108, 111, 51],
                            "to": [104, 101, 108, 108, 111, 52],
                        }
                    }
                }
                in changes,
                msg=f"changes: {changes}",
            )
            # Rm behavior is different on Linux and Mac
            # Mac will sometimes rename the file to a nfs.date.xxxx and then delete it
            if sys.platform == "linux":
                self.assertTrue(
                    {
                        "SmallChange": {
                            "Removed": {
                                "file_type": "Regular",
                                "path": [104, 101, 108, 108, 111, 52],
                            }
                        }
                    }
                    in changes,
                    msg=f"changes: {changes}",
                )
        else:
            # Windows has no rename, only add/remove
            self.assertTrue(
                {
                    "SmallChange": {
                        "Added": {
                            "file_type": "Regular",
                            "path": [104, 101, 108, 108, 111, 51],
                        }
                    }
                }
                in changes,
                msg=f"changes: {changes}",
            )
            self.assertTrue(
                {
                    "SmallChange": {
                        "Removed": {
                            "file_type": "Regular",
                            "path": [104, 101, 108, 108, 111, 51],
                        }
                    }
                }
                in changes,
                msg=f"changes: {changes}",
            )
            self.assertTrue(
                {
                    "SmallChange": {
                        "Added": {
                            "file_type": "Regular",
                            "path": [104, 101, 108, 108, 111, 52],
                        }
                    }
                }
                in changes,
                msg=f"changes: {changes}",
            )
