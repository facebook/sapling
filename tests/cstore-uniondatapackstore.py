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
