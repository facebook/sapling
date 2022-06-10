from __future__ import absolute_import

import errno
import os
import tempfile
import unittest

import silenttestrunner
from edenscm.mercurial import (
    encoding,
    error,
    extensions,
    lock,
    pycompat,
    ui,
    util,
    vfs as vfsmod,
)
from hghave import require


testlockname = "testlock"


class lockwrapper(lock.pythonlock):
    def __init__(self, pidoffset, *args, **kwargs):
        # lock.lock.__init__() calls lock(), so the pidoffset assignment needs
        # to be earlier
        self._pidoffset = pidoffset
        super(lockwrapper, self).__init__(*args, **kwargs)

    def _getpid(self):
        pid = super(lockwrapper, self)._getpid()
        return "%s/%s" % (pid, self._pidoffset)


class teststate(object):
    def __init__(self, testcase, dir, pidoffset=0):
        self._testcase = testcase
        self._acquirecalled = False
        self._releasecalled = False
        self._postreleasecalled = False
        self.vfs = vfsmod.vfs(dir, audit=False)
        self._pidoffset = pidoffset

    def makelock(self, *args, **kwargs):
        l = lockwrapper(
            self._pidoffset,
            self.vfs,
            testlockname,
            releasefn=self.releasefn,
            acquirefn=self.acquirefn,
            *args,
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

    def testfrequentlockunlock(self):
        """This tests whether lock acquisition fails as expected, even if
        (1) lock can't be acquired (makelock fails by EEXIST), and
        (2) lockinfo can't be read in (readlock fails by ENOENT) while
        retrying 5 times.
        """

        d = tempfile.mkdtemp(dir=os.getcwd())
        state = teststate(self, d)

        def emulatefrequentlock(*args, **kwargs):
            raise OSError(errno.EEXIST, "File exists")

        def emulatefrequentunlock(*args, **kwargs):
            raise OSError(errno.ENOENT, "No such file or directory")

        state.vfs.makelock = emulatefrequentlock
        state.vfs.readlock = emulatefrequentunlock

        try:
            state.makelock(timeout=0)
            self.fail("unexpected lock acquisition")
        except error.LockHeld as why:
            self.assertTrue(why.errno == errno.ETIMEDOUT)
            self.assertTrue(why.lockinfo == lock.emptylockinfo)
            state.assertlockexists(False)

    def testislocked(self):
        d = tempfile.mkdtemp(dir=os.getcwd())
        state = teststate(self, d)

        self.assertFalse(lock.islocked(state.vfs, testlockname))

        with state.makelock():
            self.assertTrue(lock.islocked(state.vfs, testlockname))

        self.assertFalse(lock.islocked(state.vfs, testlockname))


if not pycompat.iswindows:

    class testposixmakelock(unittest.TestCase):
        def testremovesymlinkplaceholder(self):
            class SpecificError(Exception):
                pass

            # Rename is the last step of makelock. Make it fail.
            def _failrename(orig, src, dst):
                raise SpecificError()

            testtmp = encoding.environ.get("TESTTMP")
            lockpath = os.path.join(testtmp, "testlock")
            with extensions.wrappedfunction(
                os, "rename", _failrename
            ), self.assertRaises(SpecificError):
                util.makelock("foo:%s" % os.getpid(), lockpath)

            # The placeholder lock should be removed.
            self.assertFalse(os.path.lexists(lockpath))


if __name__ == "__main__":
    silenttestrunner.main(__name__)
