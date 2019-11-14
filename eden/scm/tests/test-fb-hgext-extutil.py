# Copyright 2004-present Facebook. All Rights Reserved.

from __future__ import absolute_import

import errno
import os
import time
import unittest

import silenttestrunner
from edenscm.hgext import extutil
from edenscm.mercurial import error, vfs, worker


locktimeout = 25
locksuccess = 24


class ExtutilTests(unittest.TestCase):
    def testbgcommandnoblock(self):
        """runbgcommand() should return without waiting for the process to
        finish."""
        env = os.environ.copy()
        start = time.time()
        extutil.runbgcommand(["sleep", "5"], env)
        end = time.time()
        if end - start >= 1.0:
            self.fail(
                "runbgcommand() took took %s seconds, should have "
                "returned immediately" % (end - start)
            )

    def testbgcommandfailure1(self):
        """runbgcommand() should throw if executing the process fails."""
        env = os.environ.copy()
        try:
            extutil.runbgcommand(["no_such_program", "arg1", "arg2"], env)
            self.fail("expected runbgcommand to fail with ENOENT")
        except OSError as ex:
            self.assertEqual(ex.errno, errno.ENOENT)

    def testbgcommandfailure2(self):
        """runbgcommand() should throw if executing the process fails."""
        env = os.environ.copy()
        try:
            extutil.runbgcommand([os.devnull, "arg1", "arg2"], env)
            self.fail("expected runbgcommand to fail with EACCES")
        except OSError as ex:
            self.assertEqual(ex.errno, errno.EACCES)

    def testflock(self):
        testtmp = os.environ["TESTTMP"]
        opener = vfs.vfs(testtmp)
        name = "testlock"

        with extutil.flock(opener.join(name), "testing a lock", timeout=0):
            otherlock = self.otherprocesslock(opener, name)
            self.assertEquals(
                otherlock, locktimeout, "other process should not have taken the lock"
            )

        otherlock = self.otherprocesslock(opener, name)
        self.assertEquals(
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
            p, st = os.waitpid(pid, 0)
            st = worker._exitstatus(st)  # Convert back to an int
            return st


if __name__ == "__main__":
    silenttestrunner.main(__name__)
