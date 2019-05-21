from __future__ import absolute_import

import os
import time
import unittest

import silenttestrunner
from edenscm.mercurial import error, manifest, ui, util, vfs as vfsmod


class mockmanifest(object):
    def __init__(self, text):
        self.text = text

    def copy(self):
        return mockmanifest(self.text)


class mockondiskcache(object):
    def __init__(self):
        self.data = {}

    def _pathfromnode(self, hexnode):
        return hexnode

    def touch(self, hexnode, delay=0):
        pass

    def __contains__(self, hexnode):
        return hexnode in self.data

    def items(self):
        return self.data.keys()

    def __iter__(self):
        return iter(self.items())

    def __setitem__(self, hexnode, manifest):
        self.data[hexnode] = manifest

    def __delitem__(self, hexnode):
        if hexnode in self.data:
            del self.data[hexnode]

    def __getitem__(self, hexnode):
        return self.data.get(hexnode, None)

    def entrysize(self, hexnode):
        return len(self.data[hexnode]) if hexnode in self.data else None

    def totalsize(self, silent=True):
        return (sum(self.entrysize(hexnode) for hexnode in self), len(self.items()))


class HybridManifest(unittest.TestCase):
    def test_wrap(self):
        """If a new magic method is added to manifestdict, we want to make sure
        that hybridmanifest implement it, this test validates that all magic
        methods of manifestdict are implemented by hybridmanifest to avoid
        breakage in prod

        """
        vfs = vfsmod.vfs(os.getcwd())
        tempui = ui.ui()
        tempui.setconfig("fastmanifest", "usecache", True)
        hd = fastmanifest.implementation.hybridmanifest(tempui, vfs, None, fast=True)
        ismagic = lambda x: x.startswith("__") and x.endswith("__")
        magicmethods = [
            k
            for k, v in manifest.manifestdict.__dict__.items()
            if util.safehasattr(v, "__call__") and ismagic(k)
        ]
        for method in magicmethods:
            assert util.safehasattr(hd, method), (
                "%s missing in hybrid manifest" % method
            )

    def test_cachelimit(self):
        from edenscm.hgext.fastmanifest.cachemanager import _systemawarecachelimit

        cachealloc = _systemawarecachelimit.cacheallocation
        GB = fastmanifest.cachemanager.GB
        MB = fastmanifest.cachemanager.MB
        assert cachealloc(0) == 0
        assert cachealloc(120 * GB) == 6 * GB
        assert abs(cachealloc(28 * GB) - 5.6 * GB) < 5 * MB

    def test_looselock_basic(self):
        """Attempt to secure two locks.  The second one should fail."""
        vfs = vfsmod.vfs("")
        with fastmanifest.concurrency.looselock(vfs, "lock") as l1:
            assert l1.held()

            vfs2 = vfsmod.vfs("")
            try:
                with fastmanifest.concurrency.looselock(vfs2, "lock") as l2:
                    assert l2.held()

            except error.LockHeld:
                pass
            else:
                self.fail("two locks both succeeded.")

        self.assertRaises(OSError, lambda: vfs.lstat("lock"))

    def test_cachehierarchy(self):
        """We mock the ondisk cache and test that the two layers of cache
        work properly"""
        vfs = vfsmod.vfs(os.getcwd())
        tempui = ui.ui()
        tempui.setconfig("fastmanifest", "usecache", True)
        cache = fastmanifest.implementation.fastmanifestcache(vfs, tempui, None)
        ondiskcache = mockondiskcache()
        cache.ondiskcache = ondiskcache
        # Test 1) Put one manifest in the cache, check that retrieving it does
        # not hit the disk
        cache["abc"] = mockmanifest("abcnode")
        # remove the ondiskcache to make sure we don't hit it
        cache.ondiskcache = None
        assert cache["abc"].text == "abcnode"
        assert ondiskcache.data["abc"].text == "abcnode"
        cache.ondiskcache = ondiskcache

        # Test 2) Put an entry in the cache that is already in memory but not
        # on disk, should write it on disk
        ondiskcache.data.clear()
        cache["abc"] = mockmanifest("abcnode")
        assert ondiskcache.data["abc"].text == "abcnode"

        # Test 3) Put an entry in the cache that is already on disk, not in
        # memory, it should be added to the inmemorycache
        cache.inmemorycache.clear()
        cache["abc"] = mockmanifest("abcnode")
        assert cache.inmemorycache["abc"].text == "abcnode"

        # Test 4) We have at most 10 entries in the in memorycache by
        # default
        for a in range(20):
            cache[chr(a + ord("a"))] = mockmanifest(chr(a + ord("a")) + "node")

        assert len(cache.ondiskcache.items()) == 21
        assert len(cache.inmemorycache) == 10

    def test_looselock_stealing(self):
        """Attempt to secure three locks.  The second lock should succeed
        through a steal.  The third lock should fail because the second lock
        should have refreshed the lock.

        Finally, verify that the locks are properly cleaned up.
        """
        vfs = vfsmod.vfs("")
        with fastmanifest.concurrency.looselock(vfs, "lock") as l1:
            assert l1.held()

            # locks are implemented as symlinks, and we can't utime those, so we
            # have to wait.........
            time.sleep(2)

            vfs2 = vfsmod.vfs("")
            with fastmanifest.concurrency.looselock(vfs2, "lock", 0.2) as l2:
                assert l2.held()

                vfs3 = vfsmod.vfs("")
                try:
                    with fastmanifest.concurrency.looselock(vfs3, "lock") as l3:
                        assert l3.held()
                except error.LockHeld:
                    pass
                else:
                    self.fail("third lock shouldn't be able to steal.")

            self.assertRaises(OSError, lambda: vfs.lstat("lock"))


if __name__ == "__main__":
    from edenscm.hgext import fastmanifest

    silenttestrunner.main(__name__)
