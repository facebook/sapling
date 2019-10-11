#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import contextlib
import signal
import subprocess
import threading
import time
import typing
import unittest

from eden.cli.daemon import is_zombie_process, wait_for_shutdown
from eden.cli.util import poll_until


class WaitForShutdownTest(unittest.TestCase):
    def test_waiting_for_exited_process_finishes_immediately(self) -> None:
        process = AutoReapingChildProcess(["true"])
        process.wait()

        stop_watch = StopWatch()
        with stop_watch.measure():
            wait_for_shutdown(process.pid, timeout=5)
        self.assertLessEqual(stop_watch.elapsed, 3)

    def test_waiting_for_zombie_process_finishes_immediately(self) -> None:
        with subprocess.Popen(["true"]) as process:
            wait_until_process_is_zombie(process)

            stop_watch = StopWatch()
            with stop_watch.measure():
                wait_for_shutdown(process.pid, timeout=5)
            self.assertLessEqual(stop_watch.elapsed, 3)

    def test_waiting_for_exiting_process_finishes_without_sigkill(self) -> None:
        process = AutoReapingChildProcess(["sleep", "1"])
        wait_for_shutdown(process.pid, timeout=5)
        returncode = process.wait()
        self.assertEqual(returncode, 0, "Process should have exited cleanly")

    def test_waiting_for_alive_process_kills_with_sigkill(self) -> None:
        process = AutoReapingChildProcess(["sleep", "30"])
        wait_for_shutdown(process.pid, timeout=1)
        returncode = process.wait()
        self.assertEqual(
            returncode, -signal.SIGKILL, "Process should have exited with SIGKILL"
        )

    def test_waiting_for_alive_unreaped_child_process_kills_with_sigkill(self) -> None:
        process = subprocess.Popen(["sleep", "30"])
        # Don't reap the process yet.
        wait_for_shutdown(process.pid, timeout=1)
        returncode = process.wait()
        self.assertEqual(
            returncode, -signal.SIGKILL, "Process should have exited with SIGKILL"
        )


class IsZombieProcessTest(unittest.TestCase):
    def test_init_process_is_not_a_zombie(self) -> None:
        self.assertFalse(is_zombie_process(1))

    def test_running_child_of_current_process_is_not_a_zombie(self) -> None:
        process = subprocess.Popen(["sleep", "3"])
        self.assertFalse(is_zombie_process(process.pid))

    def test_dead_process_is_not_a_zombie(self) -> None:
        with subprocess.Popen(["true"]) as process:
            process.wait()
            self.assertFalse(is_zombie_process(process.pid))

    def test_exited_unreaped_child_of_current_process_is_a_zombie(self) -> None:
        with subprocess.Popen(["true"]) as process:
            # Wait for the process to finish, but don't reap it yet.
            time.sleep(1)

            self.assertTrue(is_zombie_process(process.pid))


def wait_until_process_is_zombie(process: subprocess.Popen) -> None:
    def is_zombie() -> typing.Optional[bool]:
        return True if is_zombie_process(process.pid) else None

    poll_until(is_zombie, timeout=3)


class AutoReapingChildProcess:
    """A child process (subprocess.Popen) which is promptly reaped."""

    def __init__(self, args) -> None:
        super().__init__()

        self.__condition = threading.Condition()
        self.__error: typing.Optional[BaseException] = None
        self.__pid: typing.Optional[int] = None
        self.__returncode: typing.Optional[int] = None

        self.__start_thread(args)
        self.__wait_for_process_start()

    @property
    def pid(self) -> int:
        with self.__condition:
            pid = self.__pid
            assert pid is not None
            return pid

    def wait(self) -> int:
        with self.__condition:
            while self.__returncode is None:
                self.__condition.wait()
            assert self.__returncode is not None
            # pyre-fixme[7]: Expected `int` but got `Optional[int]`.
            return self.__returncode

    def __wait_for_process_start(self) -> None:
        with self.__condition:
            while self.__pid is None and self.__error is None:
                self.__condition.wait()
            if self.__error is not None:
                # pyre-fixme[48]: Expression `self.__error` has type
                #  `Optional[BaseException]` but must extend BaseException.
                raise self.__error
            assert self.__pid is not None

    def __start_thread(self, *args, **kwargs) -> None:
        thread = threading.Thread(
            target=self.__run_thread, args=(args, kwargs), daemon=True
        )
        thread.start()

    def __run_thread(self, popen_args, popen_kwargs) -> None:
        try:
            with subprocess.Popen(*popen_args, **popen_kwargs) as process:
                with self.__condition:
                    self.__pid = process.pid
                    self.__condition.notify_all()
                process.wait()
                with self.__condition:
                    self.__returncode = process.returncode
                    self.__condition.notify_all()
        except BaseException as e:
            with self.__condition:
                self.__error = e
                self.__condition.notify_all()


class StopWatch:
    __elapsed: float = 0

    @property
    def elapsed(self) -> float:
        return self.__elapsed

    @contextlib.contextmanager
    def measure(self) -> typing.Iterator[None]:
        start_time = self.__time()
        try:
            yield
        finally:
            end_time = self.__time()
            self.__elapsed += end_time - start_time

    def __time(self) -> float:
        return time.monotonic()
