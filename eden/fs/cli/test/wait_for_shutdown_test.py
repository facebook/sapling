#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import contextlib
import signal
import subprocess
import sys
import threading
import time
import typing
import unittest

from eden.fs.cli.daemon import wait_for_shutdown


class WaitForShutdownTest(unittest.TestCase):
    def test_waiting_for_exited_process_finishes_immediately(self) -> None:
        process = AutoReapingChildProcess(["python3", "-c", "0"])
        process.wait()

        stop_watch = StopWatch()
        with stop_watch.measure():
            wait_for_shutdown(process.pid, timeout=5)
        self.assertLessEqual(stop_watch.elapsed, 3)

    def test_waiting_for_exiting_process_finishes_without_sigkill(self) -> None:
        process = AutoReapingChildProcess(
            ["python3", "-c", "import time; time.sleep(1)"]
        )
        wait_for_shutdown(process.pid, timeout=5)
        returncode = process.wait()
        self.assertEqual(returncode, 0, "Process should have exited cleanly")

    def test_waiting_for_alive_process_kills_with_sigkill(self) -> None:
        process = AutoReapingChildProcess(
            ["python3", "-c", "import time; time.sleep(30)"]
        )
        wait_for_shutdown(process.pid, timeout=1)
        returncode = process.wait()
        self.assertEqual(
            returncode,
            -signal.SIGKILL if sys.platform != "win32" else 1,
            "Process should have exited with SIGKILL",
        )


class AutoReapingChildProcess:
    """A child process (subprocess.Popen) which is promptly reaped."""

    def __init__(self, args: typing.List[str]) -> None:
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
            returncode = self.__returncode
            assert returncode is not None
            return returncode

    def __wait_for_process_start(self) -> None:
        with self.__condition:
            while self.__pid is None and self.__error is None:
                self.__condition.wait()
            error = self.__error
            if error is not None:
                raise error
            assert self.__pid is not None

    def __start_thread(self, *args: typing.List[str]) -> None:
        thread = threading.Thread(target=self.__run_thread, args=(args), daemon=True)
        thread.start()

    def __run_thread(self, popen_args: typing.List[str]) -> None:
        try:
            with subprocess.Popen(popen_args) as process:
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
