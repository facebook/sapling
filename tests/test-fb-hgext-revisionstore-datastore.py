#!/usr/bin/env python2.7
from __future__ import absolute_import

import hashlib
import random
import shutil
import tempfile
import unittest

import edenscm.mercurial.ui as uimod
import silenttestrunner
from edenscm.hgext.remotefilelog import constants
from edenscm.hgext.remotefilelog.contentstore import unioncontentstore
from edenscm.hgext.remotefilelog.datapack import datapackstore, mutabledatapack
from edenscm.mercurial.node import nullid
from edenscmnative.bindings import revisionstore


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
        return "".join(chr(random.randint(0, 255)) for _ in range(20))

    def createPack(self, packdir, revisions=None):
        if revisions is None:
            revisions = [("filename", self.getFakeHash(), nullid, "content")]

        packer = mutabledatapack(uimod.ui(), packdir, version=1)

        for filename, node, base, content, meta in revisions:
            packer.add(filename, node, base, content, metadata=meta)

        path = packer.close()
        return revisionstore.datapack(path)

    def testGet(self):
        packdir = self.makeTempDir()
        revisions = [("foo", self.getFakeHash(), nullid, "content", None)]
        self.createPack(packdir, revisions=revisions)

        pystore = unioncontentstore(datapackstore(uimod.ui(), packdir))

        ruststore = revisionstore.datastore(pystore)

        rustcontent = ruststore.get(revisions[0][0], revisions[0][1])
        pycontent = ruststore.get(revisions[0][0], revisions[0][1])
        self.assertEquals(pycontent, rustcontent)

    def testGetDeltaChain(self):
        packdir = self.makeTempDir()
        hash1 = self.getFakeHash()
        revisions = [
            ("foo", hash1, nullid, "content1", None),
            ("foo", self.getFakeHash(), hash1, "content2", None),
        ]
        self.createPack(packdir, revisions=revisions)

        pystore = unioncontentstore(datapackstore(uimod.ui(), packdir))

        ruststore = revisionstore.datastore(pystore)

        rustchain = ruststore.getdeltachain(revisions[1][0], revisions[1][1])
        pychain = pystore.getdeltachain(revisions[1][0], revisions[1][1])
        self.assertEquals(pychain, rustchain)

    def testGetMeta(self):
        packdir = self.makeTempDir()
        hash1 = self.getFakeHash()
        meta = {constants.METAKEYFLAG: 1, constants.METAKEYSIZE: len("content1")}
        revisions = [
            ("foo", hash1, nullid, "content1", meta),
            ("foo", self.getFakeHash(), hash1, "content2", None),
        ]
        self.createPack(packdir, revisions=revisions)

        pystore = unioncontentstore(datapackstore(uimod.ui(), packdir))

        ruststore = revisionstore.datastore(pystore)

        rustmeta = ruststore.getmeta(revisions[0][0], revisions[0][1])
        pymeta = pystore.getmeta(revisions[0][0], revisions[0][1])
        self.assertEquals(pymeta, rustmeta)

        rustmeta = ruststore.getmeta(revisions[1][0], revisions[1][1])
        pymeta = pystore.getmeta(revisions[1][0], revisions[1][1])
        self.assertEquals(pymeta, rustmeta)

    def testGetMissing(self):
        packdir = self.makeTempDir()
        revisions = [("foo", self.getFakeHash(), nullid, "content", None)]
        self.createPack(packdir, revisions=revisions)

        pystore = unioncontentstore(datapackstore(uimod.ui(), packdir))

        ruststore = revisionstore.datastore(pystore)

        missing = [(revisions[0][0], revisions[0][1])]
        rustcontent = ruststore.getmissing(missing)
        pycontent = pystore.getmissing(missing)
        self.assertEquals(pycontent, rustcontent)

        missing = [(revisions[0][0], revisions[0][1]), ("bar", self.getFakeHash())]
        rustcontent = ruststore.getmissing(missing)
        pycontent = pystore.getmissing(missing)
        self.assertEquals(pycontent, rustcontent)


if __name__ == "__main__":
    silenttestrunner.main(__name__)
