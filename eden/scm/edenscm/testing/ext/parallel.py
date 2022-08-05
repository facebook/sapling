# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""building blocks for testing parallel operations"""

import os
import threading
import time


def testsetup(t):
    t.pyenv.update(
        {
            "parallel": parallel,
            "waitevent": waitevent,
            "notifyevent": notifyevent,
        }
    )

    @t.command
    def _waitevent(args, stderr):
        for name in args:
            waitevent(name)

    @t.command
    def _notifyevent(args, stderr):
        for name in args:
            notifyevent(name)


def parallel(*targets, timeout=60):
    """run target functions in threads and wait them"""
    threads = []
    for target in targets:
        t = threading.Thread(target=target, daemon=True)
        t.start()
        threads.append(t)
    for thread in threads:
        thread.join(timeout)
        if thread.is_alive():
            raise TimeoutError(f"waiting for thread {thread.name}")


def waitevent(name: str, timeout=60.0):
    """wait for notify(name) to happen"""
    start = time.time()
    path = _eventfilepath(name)
    while True:
        try:
            os.unlink(path)
        except FileNotFoundError:
            if timeout > 0 and time.time() - start > timeout:
                raise TimeoutError(f"waiting for {name}")
            time.sleep(0.1)
        else:
            break


def notifyevent(name: str):
    """unblock current or next wait(name) once

    It's an error if a previous notify(name) hasn't been wait(name)-ed.
    """
    path = _eventfilepath(name)
    with open(path, "x"):
        pass


def _eventfilepath(name: str):
    testtmp = os.getenv("TESTTMP")
    return os.path.join(testtmp, f"event-{name}")
