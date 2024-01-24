# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import os
import tempfile
import unittest

import silenttestrunner
from sapling import lock, pycompat, vfs as vfsmod


testlockname = "testlock"


class lockwrapper(lock.lock):
    def __init__(self, pidoffset, *args, **kwargs):
        # lock.lock.__init__() calls lock(), so the pidoffset assignment needs
        # to be earlier
        self._pidoffset = pidoffset
        super(lockwrapper, self).__init__(*args, **kwargs)

    def _getpid(self):
        pid = super(lockwrapper, self)._getpid()
        return "%s/%s" % (pid, self._pidoffset)


class teststate:
    def __init__(self, testcase, dir, pidoffset=0):
        self._testcase = testcase
        self._acquirecalled = False
        self._releasecalled = False
        self._postreleasecalled = False
        self.vfs = vfsmod.vfs(dir, audit=False)
        self._pidoffset = pidoffset

    def makelock(self, name=testlockname, **kwargs):
        if "releasefn" not in kwargs:
            kwargs["releasefn"] = self.releasefn
        if "acquirefn" not in kwargs:
            kwargs["acquirefn"] = self.acquirefn

        l = lockwrapper(
            self._pidoffset,
            self.vfs,
            name,
            **kwargs,
        )
        l.postrelease.append(self.postreleasefn)
        return l

    def acquirefn(self):
        self._acquirecalled = True

    def releasefn(self):
        self._releasecalled = True

    def postreleasefn(self):
        self._postreleasecalled = True

    def assertacquirecalled(self, called):
        self._testcase.assertEqual(
            self._acquirecalled,
            called,
            "expected acquire to be %s but was actually %s"
            % (self._tocalled(called), self._tocalled(self._acquirecalled)),
        )

    def resetacquirefn(self):
        self._acquirecalled = False

    def assertreleasecalled(self, called):
        self._testcase.assertEqual(
            self._releasecalled,
            called,
            "expected release to be %s but was actually %s"
            % (self._tocalled(called), self._tocalled(self._releasecalled)),
        )

    def assertpostreleasecalled(self, called):
        self._testcase.assertEqual(
            self._postreleasecalled,
            called,
            "expected postrelease to be %s but was actually %s"
            % (self._tocalled(called), self._tocalled(self._postreleasecalled)),
        )

    def assertlockexists(self, exists):
        actual = self.vfs.lexists(testlockname)
        self._testcase.assertEqual(
            actual,
            exists,
            "expected lock to %s but actually did %s"
            % (self._toexists(exists), self._toexists(actual)),
        )

    def _tocalled(self, called):
        if called:
            return "called"
        else:
            return "not called"

    def _toexists(self, exists):
        if exists:
            return "exist"
        else:
            return "not exist"


class testlock(unittest.TestCase):
    def testlock(self):
        state = teststate(self, tempfile.mkdtemp(dir=os.getcwd()))
        lock = state.makelock()
        state.assertacquirecalled(True)
        lock.release()
        state.assertreleasecalled(True)
        state.assertpostreleasecalled(True)
        state.assertlockexists(False)

    def testrecursivelock(self):
        state = teststate(self, tempfile.mkdtemp(dir=os.getcwd()))
        lock = state.makelock()
        state.assertacquirecalled(True)

        state.resetacquirefn()
        lock.lock()
        # recursive lock should not call acquirefn again
        state.assertacquirecalled(False)

        lock.release()  # brings lock refcount down from 2 to 1
        state.assertreleasecalled(False)
        state.assertpostreleasecalled(False)
        state.assertlockexists(True)

        lock.release()  # releases the lock
        state.assertreleasecalled(True)
        state.assertpostreleasecalled(True)
        state.assertlockexists(False)

    def testunlockordering(self):
        state = teststate(self, tempfile.mkdtemp(dir=os.getcwd()))

        def raiseexception():
            raise Exception("oops")

        unlocks = []

        def recordunlock(name):
            def _record():
                unlocks.append(name)

            return _record

        with self.assertRaisesRegex(Exception, "oops"):
            with state.makelock("one", releasefn=recordunlock("one")), state.makelock(
                "two", releasefn=recordunlock("two")
            ), state.makelock(
                "three", releasefn=recordunlock("three"), acquirefn=raiseexception
            ):
                pass

        # Make sure we release in reverse order.
        self.assertEqual(unlocks, ["three", "two", "one"])

    if not pycompat.iswindows:

        def testlockfork(self):
            state = teststate(self, tempfile.mkdtemp(dir=os.getcwd()))
            lock = state.makelock()
            state.assertacquirecalled(True)

            pid = os.fork()
            if pid == 0:
                lock._pidoffset = os.getpid()
                lock.release()
                state.assertreleasecalled(False)
                state.assertpostreleasecalled(False)
                state.assertlockexists(True)
                os._exit(0)

            _, status = os.waitpid(pid, 0)
            self.assertTrue(((status >> 8) & 0x7F) == 0)

            # release the actual lock
            lock.release()
            state.assertreleasecalled(True)
            state.assertpostreleasecalled(True)
            state.assertlockexists(False)

    def testislocked(self):
        d = tempfile.mkdtemp(dir=os.getcwd())
        state = teststate(self, d)

        self.assertFalse(lock.islocked(state.vfs, testlockname))

        with state.makelock():
            self.assertTrue(lock.islocked(state.vfs, testlockname))

        self.assertFalse(lock.islocked(state.vfs, testlockname))


if __name__ == "__main__":
    silenttestrunner.main(__name__)
