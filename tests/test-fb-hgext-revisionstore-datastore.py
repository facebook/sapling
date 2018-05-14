#!/usr/bin/env python2.7
from __future__ import absolute_import

import hashlib
import random
import shutil
import tempfile
import unittest

import silenttestrunner

from hgext.extlib.pyrevisionstore import (
    datastore,
)

from hgext.remotefilelog.datapack import (
    datapackstore,
    fastdatapack,
    mutabledatapack,
)

from hgext.remotefilelog.contentstore import (
    unioncontentstore,
)

from mercurial.node import nullid
import mercurial.ui

class datastoretests(unittest.TestCase):
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

        packer = mutabledatapack(mercurial.ui.ui(), packdir, version=1)

        for filename, node, base, content, meta in revisions:
            packer.add(filename, node, base, content, metadata=meta)

        path = packer.close()
        return fastdatapack(path)

    def testGet(self):
        packdir = self.makeTempDir()
        revisions = [("foo", self.getFakeHash(), nullid, "content", None)]
        self.createPack(packdir, revisions=revisions)

        pystore = unioncontentstore(datapackstore(mercurial.ui.ui(), packdir))

        ruststore = datastore(pystore)

        rustcontent = ruststore.get(revisions[0][0], revisions[0][1])
        pycontent = ruststore.get(revisions[0][0], revisions[0][1])
        self.assertEquals(pycontent, rustcontent)

if __name__ == '__main__':
    silenttestrunner.main(__name__)
