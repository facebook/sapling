#!/usr/bin/env python2.7

import hashlib
import os
import random
import shutil
import sys
import tempfile
import unittest

import silenttestrunner

# Load the local cstore, not the system one
fullpath = os.path.join(os.getcwd(), __file__)
sys.path.insert(0, os.path.dirname(os.path.dirname(fullpath)))

from cstore import (
    datapackstore,
    uniondatapackstore,
)

from remotefilelog.datapack import (
    fastdatapack,
    mutabledatapack,
)

from mercurial import mdiff
from mercurial.node import nullid
import mercurial.ui

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
        return ''.join(chr(random.randint(0, 255)) for _ in range(20))

    def createPack(self, packdir, revisions=None):
        if revisions is None:
            revisions = [("filename", self.getFakeHash(), nullid, "content")]

        packer = mutabledatapack(mercurial.ui.ui(), packdir)

        for filename, node, base, content in revisions:
            packer.add(filename, node, base, content)

        path = packer.close()
        return fastdatapack(path)

    def testGetFromSingleDelta(self):
        packdir = self.makeTempDir()

        revisions = [("foo", self.getFakeHash(), nullid, "content")]
        self.createPack(packdir, revisions=revisions)

        unionstore = uniondatapackstore([datapackstore(packdir)])

        text = unionstore.get(revisions[0][0], revisions[0][1])
        self.assertEquals("content", text)

    def testGetFromChainDeltas(self):
        packdir = self.makeTempDir()

        rev1 = "content"
        rev2 = "content2"
        firsthash = self.getFakeHash()
        revisions = [
            ("foo", firsthash, nullid, rev1),
            ("foo", self.getFakeHash(), firsthash,
             mdiff.textdiff(rev1, rev2)),
        ]
        self.createPack(packdir, revisions=revisions)

        unionstore = uniondatapackstore([datapackstore(packdir)])

        text = unionstore.get(revisions[1][0], revisions[1][1])
        self.assertEquals(rev2, text)

    def testGetDeltaChainSingleRev(self):
        """Test getting a 1-length delta chain."""
        packdir = self.makeTempDir()

        revisions = [("foo", self.getFakeHash(), nullid, "content")]
        self.createPack(packdir, revisions=revisions)

        unionstore = uniondatapackstore([datapackstore(packdir)])

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
        self.createPack(packdir, revisions=revisions)

        unionstore = uniondatapackstore([datapackstore(packdir)])

        chain = unionstore.getdeltachain(revisions[1][0], revisions[1][1])
        self.assertEquals(2, len(chain))
        self.assertEquals("content2", chain[0][4])
        self.assertEquals("content", chain[1][4])

    def testGetDeltaChainMultiPack(self):
        """Test getting chains from multiple packs."""
        packdir = self.makeTempDir()

        revisions1 = [
            ("foo", self.getFakeHash(), nullid, "content"),
        ]
        self.createPack(packdir, revisions=revisions1)

        revisions2 = [
            ("foo", self.getFakeHash(), revisions1[0][1], "content2"),
        ]
        self.createPack(packdir, revisions=revisions2)

        unionstore = uniondatapackstore([datapackstore(packdir)])

        chain = unionstore.getdeltachain(revisions2[0][0], revisions2[0][1])
        self.assertEquals(2, len(chain))
        self.assertEquals("content2", chain[0][4])
        self.assertEquals("content", chain[1][4])

    def testGetMissing(self):
        packdir = self.makeTempDir()

        revisions = [("foo", self.getFakeHash(), nullid, "content")]
        self.createPack(packdir, revisions=revisions)

        unionstore = uniondatapackstore([datapackstore(packdir)])

        missinghash1 = self.getFakeHash()
        missinghash2 = self.getFakeHash()
        missing = unionstore.getmissing([
            (revisions[0][0], revisions[0][1]),
            ("foo", missinghash1),
            ("foo2", missinghash2),
        ])
        self.assertEquals(2, len(missing))
        self.assertEquals(set([("foo", missinghash1), ("foo2", missinghash2)]),
                          set(missing))

if __name__ == '__main__':
    silenttestrunner.main(__name__)
