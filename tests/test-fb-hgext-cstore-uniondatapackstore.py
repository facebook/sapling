#!/usr/bin/env python2.7
from __future__ import absolute_import

import hashlib
import random
import shutil
import tempfile
import unittest

import edenscm.mercurial.ui as uimod
import silenttestrunner
from edenscm.hgext.remotefilelog.datapack import datapack, mutabledatapack
from edenscm.mercurial import mdiff
from edenscm.mercurial.node import nullid
from edenscmnative.cstore import datapackstore, uniondatapackstore


class uniondatapackstoretests(unittest.TestCase):
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

    def createPackStore(self, packdir, revisions=None):
        if revisions is None:
            revisions = [("filename", self.getFakeHash(), nullid, "content")]

        packer = mutabledatapack(uimod.ui(), packdir)

        for filename, node, base, content in revisions:
            packer.add(filename, node, base, content)

        packer.close()
        return datapackstore(packdir)

    def testGetFromSingleDelta(self):
        packdir = self.makeTempDir()

        revisions = [("foo", self.getFakeHash(), nullid, "content")]
        store = self.createPackStore(packdir, revisions=revisions)

        unionstore = uniondatapackstore([store])

        text = unionstore.get(revisions[0][0], revisions[0][1])
        self.assertEquals("content", text)

    def testGetFromChainDeltas(self):
        packdir = self.makeTempDir()

        rev1 = "content"
        rev2 = "content2"
        firsthash = self.getFakeHash()
        revisions = [
            ("foo", firsthash, nullid, rev1),
            ("foo", self.getFakeHash(), firsthash, mdiff.textdiff(rev1, rev2)),
        ]
        store = self.createPackStore(packdir, revisions=revisions)

        unionstore = uniondatapackstore([store])

        text = unionstore.get(revisions[1][0], revisions[1][1])
        self.assertEquals(rev2, text)

    def testGetDeltaChainSingleRev(self):
        """Test getting a 1-length delta chain."""
        packdir = self.makeTempDir()

        revisions = [("foo", self.getFakeHash(), nullid, "content")]
        store = self.createPackStore(packdir, revisions=revisions)

        unionstore = uniondatapackstore([store])

        chain = unionstore.getdeltachain(revisions[0][0], revisions[0][1])
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
        store = self.createPackStore(packdir, revisions=revisions)

        unionstore = uniondatapackstore([store])

        chain = unionstore.getdeltachain(revisions[1][0], revisions[1][1])
        self.assertEquals(2, len(chain))
        self.assertEquals("content2", chain[0][4])
        self.assertEquals("content", chain[1][4])

    def testGetDeltaChainMultiPack(self):
        """Test getting chains from multiple packs."""
        packdir = self.makeTempDir()

        revisions1 = [("foo", self.getFakeHash(), nullid, "content")]
        store = self.createPackStore(packdir, revisions=revisions1)

        revisions2 = [("foo", self.getFakeHash(), revisions1[0][1], "content2")]
        store = self.createPackStore(packdir, revisions=revisions2)

        unionstore = uniondatapackstore([store])

        chain = unionstore.getdeltachain(revisions2[0][0], revisions2[0][1])
        self.assertEquals(2, len(chain))
        self.assertEquals("content2", chain[0][4])
        self.assertEquals("content", chain[1][4])

    def testGetMissing(self):
        packdir = self.makeTempDir()

        revisions = [("foo", self.getFakeHash(), nullid, "content")]
        store = self.createPackStore(packdir, revisions=revisions)

        unionstore = uniondatapackstore([store])

        missinghash1 = self.getFakeHash()
        missinghash2 = self.getFakeHash()
        missing = unionstore.getmissing(
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

    def testAddRemoveStore(self):
        packdir = self.makeTempDir()
        revisions = [("foo", self.getFakeHash(), nullid, "content")]
        store = self.createPackStore(packdir, revisions=revisions)

        packdir2 = self.makeTempDir()
        revisions2 = [("foo2", self.getFakeHash(), nullid, "content2")]
        store2 = self.createPackStore(packdir2, revisions=revisions2)

        unionstore = uniondatapackstore([store])
        unionstore.addstore(store2)

        # Fetch from store2
        result = unionstore.get("foo2", revisions2[0][1])
        self.assertEquals(result, revisions2[0][3])

        # Drop the store
        unionstore.removestore(store2)

        # Fetch from store1
        result = unionstore.get("foo", revisions[0][1])
        self.assertEquals(result, revisions[0][3])

        # Fetch from missing store2
        try:
            unionstore.get("foo2", revisions2[0][1])
            self.asserFalse(True, "get should've thrown")
        except KeyError:
            pass


class uniondatastorepythontests(uniondatapackstoretests):
    def createPackStore(self, packdir, revisions=None):
        if revisions is None:
            revisions = [("filename", self.getFakeHash(), nullid, "content")]

        packer = mutabledatapack(uimod.ui(), packdir)

        for filename, node, base, content in revisions:
            packer.add(filename, node, base, content)

        path = packer.close()
        return datapack(path)

    def testGetDeltaChainMultiPack(self):
        """Test getting chains from multiple packs."""
        packdir = self.makeTempDir()

        revisions1 = [("foo", self.getFakeHash(), nullid, "content")]
        pack1 = self.createPackStore(packdir, revisions=revisions1)

        revisions2 = [("foo", self.getFakeHash(), revisions1[0][1], "content2")]
        pack2 = self.createPackStore(packdir, revisions=revisions2)

        unionstore = uniondatapackstore([pack1, pack2])

        chain = unionstore.getdeltachain(revisions2[0][0], revisions2[0][1])
        self.assertEquals(2, len(chain))
        self.assertEquals("content2", chain[0][4])
        self.assertEquals("content", chain[1][4])

    def testGetDeltaChainMultiPackPyAndC(self):
        """Test getting chains from multiple packs."""
        packdir1 = self.makeTempDir()
        packdir2 = self.makeTempDir()

        revisions1 = [("foo", self.getFakeHash(), nullid, "content")]
        store = super(uniondatastorepythontests, self).createPackStore(
            packdir1, revisions=revisions1
        )

        revisions2 = [("foo", self.getFakeHash(), revisions1[0][1], "content2")]
        pack = self.createPackStore(packdir2, revisions=revisions2)

        unionstore = uniondatapackstore([pack, store])

        chain = unionstore.getdeltachain(revisions2[0][0], revisions2[0][1])
        self.assertEquals(2, len(chain))
        self.assertEquals("content2", chain[0][4])
        self.assertEquals("content", chain[1][4])


if __name__ == "__main__":
    silenttestrunner.main(__name__)
