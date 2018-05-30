#!/usr/bin/env python2.7
from __future__ import absolute_import

import hashlib
import random
import shutil
import tempfile
import time
import unittest

import mercurial.ui
import silenttestrunner
from hgext.extlib.cstore import datapackstore
from hgext.remotefilelog.datapack import fastdatapack, mutabledatapack
from mercurial.node import nullid


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

        packer = mutabledatapack(mercurial.ui.ui(), packdir)

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
