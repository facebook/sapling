import silenttestrunner
import unittest
import os
import sys
import time

from mercurial import error
from mercurial import manifest
from mercurial import scmutil
from mercurial import util

class HybridManifest(unittest.TestCase):

    def test_wrap(self):
        """If a new magic method is added to manifestdict, we want to make sure
        that hybridmanifest implement it, this test validates that all magic
        methods of manifestdict are implemented by hybridmanifest to avoid
        breakage in prod

        """
        vfs = scmutil.vfs('')
        hd = fastmanifest.implementation.hybridmanifest(None, vfs)
        ismagic = lambda x: x.startswith("__") and x.endswith("__")
        magicmethods = [k
                        for k, v in manifest.manifestdict.__dict__.items()
                        if util.safehasattr(v, '__call__') and ismagic(k)]
        for method in magicmethods:
                assert util.safehasattr(hd, method),\
                        "%s missing in hybrid manifest" % method

    def test_cachelimit(self):
        from fastmanifest.cachemanager import _systemawarecachelimit
        cachealloc = _systemawarecachelimit.cacheallocation
        GB = fastmanifest.cachemanager.GB
        MB = fastmanifest.cachemanager.MB
        assert cachealloc(0) == 0
        assert cachealloc(120 * GB) == 6 * GB
        assert abs(cachealloc(28 * GB) - 5.6 * GB) < 5 * MB

    def test_shufflebybatch(self):
        data = range(10000)
        fastmanifest.cachemanager.shufflebybatch(data, 5)
        assert len(data) == 10000
        assert data != range(10000)

    def test_looselock_basic(self):
        """Attempt to secure two locks.  The second one should fail."""
        vfs = scmutil.vfs('')
        with fastmanifest.concurrency.looselock(vfs, "lock") as l1:
            assert l1.held()

            vfs2 = scmutil.vfs('')
            try:
                with fastmanifest.concurrency.looselock(vfs2, "lock") as l2:
                    assert l2.held()

            except error.LockHeld:
                pass
            else:
                self.fail("two locks both succeeded.")

        self.assertRaises(OSError,
                          lambda: vfs.lstat("lock"))

    def test_looselock_stealing(self):
        """Attempt to secure three locks.  The second lock should succeed
        through a steal.  The third lock should fail because the second lock
        should have refreshed the lock.

        Finally, verify that the locks are properly cleaned up.
        """
        vfs = scmutil.vfs('')
        with fastmanifest.concurrency.looselock(vfs, "lock") as l1:
            assert l1.held()

            # locks are implemented as symlinks, and we can't utime those, so we
            # have to wait.........
            time.sleep(2)

            vfs2 = scmutil.vfs('')
            with fastmanifest.concurrency.looselock(vfs2, "lock", 0.2) as l2:
                assert l2.held()

                vfs3 = scmutil.vfs('')
                try:
                    with fastmanifest.concurrency.looselock(vfs3, "lock") as l3:
                        assert l3.held()
                except error.LockHeld:
                    pass
                else:
                    self.fail("third lock shouldn't be able to steal.")

            self.assertRaises(OSError,
                              lambda: vfs.lstat("lock"))

if __name__ == "__main__":
    sys.path.insert(0, os.path.join(os.environ["TESTDIR"], ".."))
    import fastmanifest
    silenttestrunner.main(__name__)
