# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import tempfile
import unittest

import silenttestrunner
from edenscm import error, lock, pycompat, ui, vfs


class testrustlock(unittest.TestCase):
    def setUp(self):
        self.vfs = vfs.vfs(tempfile.mkdtemp(dir=os.getcwd()), audit=False)
        self.ui = ui.ui()

    def testcallbacks(self):
        acquired, prereleased, postreleased = (0, 0, 0)

        def acquire():
            nonlocal acquired
            acquired += 1

        def release():
            nonlocal prereleased
            prereleased += 1

        def postrelease():
            nonlocal postreleased
            postreleased += 1

        l = lock.rustlock(
            self.vfs,
            "foo",
            acquirefn=acquire,
            releasefn=release,
        )
        l.postrelease.append(postrelease)

        self.assertLocked("foo")
        self.assertEqual(acquired, 1)
        self.assertEqual(prereleased, 0)
        self.assertEqual(postreleased, 0)

        # recursive lock call - don't call callbacks again
        l.lock()
        l.release()
        self.assertLocked("foo")
        self.assertEqual(acquired, 1)
        self.assertEqual(prereleased, 0)
        self.assertEqual(postreleased, 0)

        l.release()
        self.assertNotLocked("foo")
        self.assertEqual(acquired, 1)
        self.assertEqual(prereleased, 1)
        self.assertEqual(postreleased, 1)

    def testsubdirlock(self):
        self.vfs.mkdir("some_dir")

        l = lock.rustlock(
            self.vfs,
            "some_dir/foo",
        )

        self.assertLocked("some_dir/foo")

        l.release()
        self.assertNotLocked("some_dir/foo")

    if not pycompat.iswindows:

        def testpermissionerror(self):
            os.chmod(self.vfs.base, 0)
            with self.assertRaises(error.LockUnavailable):
                lock.rustlock(self.vfs, "foo")

        # Test that we don't drop locks in forked child.
        def testfork(self):
            l = lock.rustlock(self.vfs, "foo")

            pid = os.fork()
            if pid == 0:
                l.release()
                self.assertLocked("foo")
                os._exit(0)

            os.waitpid(pid, 0)

            self.assertLocked("foo")

            l.release()

            self.assertNotLocked("foo")

    # Make sure the devel.lockmode=rust_only flag works.
    def testrustonlymode(self):
        with self.ui.configoverride({("devel", "lockmode"): "rust_only"}):
            with lock.lock(self.vfs, "foo", ui=self.ui):
                self.assertLocked("foo")
                self.assertLegacyLock("foo", True)

            self.assertNotLocked("foo")
            self.assertLegacyLock("foo", False)

    def assertLegacyLock(self, name, exists):
        self.assertEqual(self.vfs.lexists(name), exists)

    def assertLocked(self, name):
        with self.assertRaises(error.LockHeld):
            lock.rustlock(self.vfs, name, timeout=0)

    def assertNotLocked(self, name):
        try:
            lock.rustlock(self.vfs, name, timeout=0).release()
        except Exception as err:
            self.assertTrue(False, str(err))


if __name__ == "__main__":
    silenttestrunner.main(__name__)
