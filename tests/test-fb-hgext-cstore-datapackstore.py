#!/usr/bin/env python2.7
from __future__ import absolute_import

import hashlib
import random
import shutil
import tempfile
import time
import unittest

import edenscm.mercurial.ui as uimod
import silenttestrunner
from edenscm.hgext.remotefilelog.datapack import fastdatapack, mutabledatapack
from edenscm.mercurial.node import bin, nullid
from edenscmnative.cstore import datapackstore


class datapackstoretests(unittest.TestCase):
    def setUp(self):
        random.seed(0)
        self.tempdirs = []

    def tearDown(self):
        for d in self.tempdirs:
            shutil.rmtree(d)

    def makeTempDir(self):
        tempdir = tempfile.mkdtemp()
        self.tempdirs.append(tempdir)
        return tempdir

    def getHash(self, content):
        return hashlib.sha1(content).digest()

    def getFakeHash(self):
        return "".join(chr(random.randint(0, 255)) for _ in range(20))

    def createPack(self, packdir, revisions=None):
        if revisions is None:
            revisions = [("filename", self.getFakeHash(), nullid, "content")]

        packer = mutabledatapack(uimod.ui(), packdir)

        for filename, node, base, content in revisions:
            packer.add(filename, node, base, content)

        path = packer.close()
        return fastdatapack(path)

    def testGetDeltaChainSingleRev(self):
        """Test getting a 1-length delta chain."""
        packdir = self.makeTempDir()

        revisions = [("foo", self.getFakeHash(), nullid, "content")]
        self.createPack(packdir, revisions=revisions)

        store = datapackstore(packdir)

        chain = store.getdeltachain(revisions[0][0], revisions[0][1])
        self.assertEquals(1, len(chain))
        self.assertEquals("content", chain[0][4])

    def testPackWithSameNodePrefixes(self):
        """
        Test a pack with several nodes that all start with the same prefix.

        Previously the cdatapack code had a bug reading packs where all nodes
        started with the same byte, causing it to fail to find most nodes in
        the pack file.
        """
        packdir = self.makeTempDir()

        node1 = bin("c4beedc1071590f5a0869a72efd80ce182bb1146")
        node2 = bin("c4beede6a252041e1d8c0e8410c5c37eb6568c49")
        node3 = bin("c4beed4045e49bf0c18e6aa3a4bdd00ff72ed99e")

        packer = mutabledatapack(uimod.ui(), packdir)
        packer.add("foo.c", node1, nullid, "stuff")
        packer.add("bar.c", node2, nullid, "other stuff")
        packer.add("test", node3, nullid, "things")
        path = packer.close()

        # We use fastdatapack.getmissing() to exercise the cdatapack find()
        # function
        pack = fastdatapack(path)
        self.assertEquals(pack.getmissing([("foo.c", node1)]), [])
        self.assertEquals(pack.getmissing([("bar.c", node2)]), [])
        self.assertEquals(pack.getmissing([("test", node3)]), [])

        # Confirm that getmissing() does return a node that is actually missing
        node4 = bin("4e4a47e84ced76e1d30da10a59ad9e95c9d621d7")
        self.assertEquals(pack.getmissing([("other.c", node4)]), [("other.c", node4)])

    def testGetDeltaChainMultiRev(self):
        """Test getting a 2-length delta chain."""
        packdir = self.makeTempDir()

        firsthash = self.getFakeHash()
        revisions = [
            ("foo", firsthash, nullid, "content"),
            ("foo", self.getFakeHash(), firsthash, "content2"),
        ]
        self.createPack(packdir, revisions=revisions)

        store = datapackstore(packdir)

        chain = store.getdeltachain(revisions[1][0], revisions[1][1])
        self.assertEquals(2, len(chain))
        self.assertEquals("content2", chain[0][4])
        self.assertEquals("content", chain[1][4])

    def testGetDeltaChainMultiPack(self):
        """Test getting chains from multiple packs."""
        packdir = self.makeTempDir()

        revisions1 = [("foo", self.getFakeHash(), nullid, "content")]
        self.createPack(packdir, revisions=revisions1)

        revisions2 = [("foo", self.getFakeHash(), revisions1[0][1], "content2")]
        self.createPack(packdir, revisions=revisions2)

        store = datapackstore(packdir)

        chain1 = store.getdeltachain(revisions2[0][0], revisions2[0][1])
        self.assertEquals(1, len(chain1))
        self.assertEquals("content2", chain1[0][4])

        chain2 = store.getdeltachain(chain1[0][2], chain1[0][3])
        self.assertEquals(1, len(chain2))
        self.assertEquals("content", chain2[0][4])

    def testGetMissing(self):
        packdir = self.makeTempDir()

        revisions = [("foo", self.getFakeHash(), nullid, "content")]
        self.createPack(packdir, revisions=revisions)

        store = datapackstore(packdir)

        missinghash1 = self.getFakeHash()
        missinghash2 = self.getFakeHash()
        missing = store.getmissing(
            [
                (revisions[0][0], revisions[0][1]),
                ("foo", missinghash1),
                ("foo2", missinghash2),
            ]
        )
        self.assertEquals(2, len(missing))
        self.assertEquals(
            set([("foo", missinghash1), ("foo2", missinghash2)]), set(missing)
        )

    def testRefreshPacks(self):
        packdir = self.makeTempDir()

        revisions = [("foo", self.getFakeHash(), nullid, "content")]
        self.createPack(packdir, revisions=revisions)
        store = datapackstore(packdir)

        missing = store.getmissing([(revisions[0][0], revisions[0][1])])
        self.assertEquals(0, len(missing))

        revisions2 = [("foo2", self.getFakeHash(), nullid, "content")]
        self.createPack(packdir, revisions=revisions2)

        # First miss should guarantee a refresh
        missing = store.getmissing([(revisions2[0][0], revisions2[0][1])])
        self.assertEquals(0, len(missing))

        revisions3 = [("foo3", self.getFakeHash(), nullid, "content")]
        self.createPack(packdir, revisions=revisions3)

        # Second miss should guarantee a refresh after 100ms.
        # Use a busy loop since we listen to the clock timer internally.
        now = time.time()
        while time.time() - now < 0.2:
            continue
        missing = store.getmissing([(revisions3[0][0], revisions3[0][1])])
        self.assertEquals(0, len(missing))


if __name__ == "__main__":
    silenttestrunner.main(__name__)
