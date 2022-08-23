# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import errno
import os
import time
import unittest

import silenttestrunner
from edenscm.ext import extutil
from edenscm.mercurial import error, util, vfs
from hghave import require


locktimeout = 25
locksuccess = 24


def _exitstatus(code):
    """convert a posix exit status into the same form returned by
    os.spawnv

    returns None if the process was stopped instead of exiting"""
    if os.WIFEXITED(code):
        return os.WEXITSTATUS(code)
    elif os.WIFSIGNALED(code):
        return -(os.WTERMSIG(code))


class ExtutilTests(unittest.TestCase):
    def testspawndetachednoblock(self):
        """spawndetached() should return without waiting for the process to
        finish."""
        start = time.time()
        util.spawndetached(["sleep", "5"])
        end = time.time()
        if end - start >= 1.0:
            self.fail(
                "spawndetached() took took %s seconds, should have "
                "returned immediately" % (end - start)
            )

    def testspawndetachedfailure1(self):
        """spawndetached() should throw if executing the process fails."""
        try:
            util.spawndetached(["no_such_program", "arg1", "arg2"])
            self.fail("expected spawndetached to fail with ENOENT")
        except (OSError, IOError) as ex:
            self.assertEqual(ex.errno, errno.ENOENT)

    def testspawndetachedfailure2(self):
        """spawndetached() should throw if executing the process fails."""
        try:
            util.spawndetached([os.devnull, "arg1", "arg2"])
            self.fail("expected spawndetached to fail with EACCES")
        except (OSError, IOError) as ex:
            self.assertEqual(ex.errno, errno.EPERM)

    def testflock(self):
        testtmp = os.environ["TESTTMP"]
        opener = vfs.vfs(testtmp)
        name = "testlock"

        with extutil.flock(opener.join(name), "testing a lock", timeout=0):
            otherlock = self.otherprocesslock(opener, name)
            self.assertEqual(
                otherlock, locktimeout, "other process should not have taken the lock"
            )

        otherlock = self.otherprocesslock(opener, name)
        self.assertEqual(
            otherlock, locksuccess, "other process should have taken the lock"
        )

    def otherprocesslock(self, opener, name):
        pid = os.fork()
        if pid == 0:
            try:
                with extutil.flock(opener.join(name), "other process lock", timeout=0):
                    os._exit(locksuccess)
            except error.LockHeld:
                os._exit(locktimeout)
        else:
            time.sleep(0.1)  # Avoids a crash on OSX
            p, st = os.waitpid(pid, 0)
            st = _exitstatus(st)  # Convert back to an int
            return st


if __name__ == "__main__":
    silenttestrunner.main(__name__)
