#!/usr/bin/env python
from __future__ import absolute_import, print_function

import hashlib
import os
import random
import shutil
import stat
import struct
import tempfile
import time
import unittest

import edenscm.mercurial.ui as uimod
import silenttestrunner
from bindings import revisionstore
from edenscm.hgext.remotefilelog import constants
from edenscm.hgext.remotefilelog.datapack import datapackstore
from edenscm.mercurial import error
from edenscm.mercurial.node import nullid


SMALLFANOUTCUTOFF = 2 ** 16 / 8

try:
    xrange(0)
except NameError:
    xrange = range


class datapacktestsbase(object):
    def __init__(self, datapackreader):
        self.datapackreader = datapackreader

    def setUp(self):
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

    def createPack(self, revisions=None, packdir=None, version=0):
        if revisions is None:
            revisions = [("filename", self.getFakeHash(), nullid, "content")]

        if packdir is None:
            packdir = self.makeTempDir()

        packer = revisionstore.mutabledeltastore(packfilepath=packdir)

        for args in revisions:
            filename, node, base, content = args[0:4]
            # meta is optional
            meta = None
            if len(args) > 4:
                meta = args[4]
            packer.add(filename, node, base, content, metadata=meta)

        path = packer.flush()
        return self.datapackreader(path)

    def _testAddSingle(self, content):
        """Test putting a simple blob into a pack and reading it out.
        """
        filename = "foo"
        node = self.getHash(content)

        revisions = [(filename, node, nullid, content)]
        pack = self.createPack(revisions)

        chain = pack.getdeltachain(filename, node)
        self.assertEquals(content, chain[0][4])

    def testAddSingle(self):
        self._testAddSingle("")

    def testAddSingleEmpty(self):
        self._testAddSingle("abcdef")

    def testAddMultiple(self):
        """Test putting multiple unrelated blobs into a pack and reading them
        out.
        """
        revisions = []
        for i in range(10):
            filename = "foo%s" % i
            content = "abcdef%s" % i
            node = self.getHash(content)
            revisions.append((filename, node, self.getFakeHash(), content))

        pack = self.createPack(revisions)

        for filename, node, base, content in revisions:
            entry = pack.getdelta(filename, node)
            self.assertEquals((content, filename, base, {}), entry)

            chain = pack.getdeltachain(filename, node)
            self.assertEquals(content, chain[0][4])

    def testAddDeltas(self):
        """Test putting multiple delta blobs into a pack and read the chain.
        """
        revisions = []
        filename = "foo"
        lastnode = nullid
        for i in range(10):
            content = "abcdef%s" % i
            node = self.getHash(content)
            revisions.append((filename, node, lastnode, content))
            lastnode = node

        pack = self.createPack(revisions)

        entry = pack.getdelta(filename, revisions[0][1])
        realvalue = (revisions[0][3], filename, revisions[0][2], {})
        self.assertEquals(entry, realvalue)

        # Test that the chain for the final entry has all the others
        chain = pack.getdeltachain(filename, node)
        for i in range(10):
            content = "abcdef%s" % i
            self.assertEquals(content, chain[-i - 1][4])

    def testPackMany(self):
        """Pack many related and unrelated objects.
        """
        # Build a random pack file
        revisions = []
        blobs = {}
        random.seed(0)
        for i in range(100):
            filename = "filename-%s" % i
            filerevs = []
            for j in range(random.randint(1, 100)):
                content = "content-%s-%s" % (i, j)
                node = self.getHash(content)
                lastnode = nullid
                if len(filerevs) > 0:
                    lastnode = filerevs[random.randint(0, len(filerevs) - 1)]
                filerevs.append(node)
                blobs[(filename, node, lastnode)] = content
                revisions.append((filename, node, lastnode, content))

        pack = self.createPack(revisions)

        # Verify the pack contents
        for (filename, node, lastnode), content in sorted(blobs.iteritems()):
            chain = pack.getdeltachain(filename, node)
            for entry in chain:
                expectedcontent = blobs[(entry[0], entry[1], entry[3])]
                self.assertEquals(entry[4], expectedcontent)

    def testPackMetadata(self):
        revisions = []
        for i in range(100):
            filename = "%s.txt" % i
            content = "put-something-here \n" * i
            node = self.getHash(content)
            meta = {constants.METAKEYFLAG: i ** 4, constants.METAKEYSIZE: len(content)}
            revisions.append((filename, node, nullid, content, meta))
        pack = self.createPack(revisions, version=1)
        for name, node, x, content, origmeta in revisions:
            parsedmeta = pack.getmeta(name, node)
            # flag == 0 should be optimized out
            if origmeta[constants.METAKEYFLAG] == 0:
                del origmeta[constants.METAKEYFLAG]
            self.assertEquals(parsedmeta, origmeta)

    def testGetMissing(self):
        """Test the getmissing() api.
        """
        revisions = []
        filename = "foo"
        lastnode = nullid
        for i in range(10):
            content = "abcdef%s" % i
            node = self.getHash(content)
            revisions.append((filename, node, lastnode, content))
            lastnode = node

        pack = self.createPack(revisions)

        missing = pack.getmissing([("foo", revisions[0][1])])
        self.assertFalse(missing)

        missing = pack.getmissing([("foo", revisions[0][1]), ("foo", revisions[1][1])])
        self.assertFalse(missing)

        fakenode = self.getFakeHash()
        missing = pack.getmissing([("foo", revisions[0][1]), ("foo", fakenode)])
        self.assertEquals(missing, [("foo", fakenode)])

    def testAddThrows(self):
        pack = self.createPack()

        try:
            pack.add("filename", nullid, "contents")
            self.assertTrue(False, "datapack.add should throw")
        except (AttributeError, RuntimeError):
            pass

    def testBadVersionThrows(self):
        pack = self.createPack()
        path = pack.path() + ".datapack"
        with open(path) as f:
            raw = f.read()
        raw = struct.pack("!B", 255) + raw[1:]
        os.chmod(path, os.stat(path).st_mode | stat.S_IWRITE)
        with open(path, "w+") as f:
            f.write(raw)

        try:
            pack = self.datapackreader(pack.path())
            self.assertTrue(False, "bad version number should have thrown")
        except error.RustError:
            pass

    def testMissingDeltabase(self):
        fakenode = self.getFakeHash()
        revisions = [("filename", fakenode, self.getFakeHash(), "content")]
        pack = self.createPack(revisions)
        chain = pack.getdeltachain("filename", fakenode)
        self.assertEquals(len(chain), 1)

    def testLargePack(self):
        """Test creating and reading from a large pack with over X entries.
        This causes it to use a 2^16 fanout table instead."""
        revisions = []
        blobs = {}
        total = SMALLFANOUTCUTOFF + 1
        for i in xrange(total):
            filename = "filename-%s" % i
            content = filename
            node = self.getHash(content)
            blobs[(filename, node)] = content
            revisions.append((filename, node, nullid, content))

        pack = self.createPack(revisions)

        for (filename, node), content in blobs.iteritems():
            actualcontent = pack.getdeltachain(filename, node)[0][4]
            self.assertEquals(actualcontent, content)

    def testPacksCache(self):
        """Test that we remember the most recent packs while fetching the delta
        chain."""

        packdir = self.makeTempDir()
        deltachains = []

        numpacks = 200
        revisionsperpack = 100

        for i in range(numpacks):
            chain = []
            revision = (str(i), self.getFakeHash(), nullid, "content")

            for _ in range(revisionsperpack):
                chain.append(revision)
                revision = (str(i), self.getFakeHash(), revision[1], self.getFakeHash())

            self.createPack(chain, packdir)
            deltachains.append(chain)

        class testdatapackstore(datapackstore):
            # Ensures that we are not keeping everything in the cache.
            DEFAULTCACHESIZE = numpacks / 2

        store = testdatapackstore(uimod.ui(), packdir)

        random.shuffle(deltachains)
        for randomchain in deltachains:
            revision = random.choice(randomchain)
            chain = store.getdeltachain(revision[0], revision[1])

            mostrecentpack = next(iter(store.packs), None)
            self.assertEquals(
                mostrecentpack.getdeltachain(revision[0], revision[1]), chain
            )

            self.assertEquals(randomchain.index(revision) + 1, len(chain))

    def testInlineRepack(self):
        """Verify that when fetchpacks is enabled, and the number of packfiles
        is over DEFAULTCACHESIZE, the refresh operation will trigger a repack,
        reducing the number of packfiles in the store.
        """
        packdir = self.makeTempDir()

        numpacks = 20
        revisionsperpack = 100

        for i in range(numpacks):
            chain = []
            revision = (str(i), self.getFakeHash(), nullid, "content")

            for _ in range(revisionsperpack):
                chain.append(revision)
                revision = (str(i), self.getFakeHash(), revision[1], self.getFakeHash())

            self.createPack(chain, packdir)

        packreader = self.datapackreader

        class testdatapackstore(datapackstore):
            DEFAULTCACHESIZE = numpacks / 2

            def getpack(self, path):
                return packreader(path)

        store = testdatapackstore(uimod.ui(), packdir)

        # The first refresh should populate all the packfiles.
        store.refresh()
        self.assertEquals(len(store.packs), testdatapackstore.DEFAULTCACHESIZE)

        # Each packfile is made up of 2 files: the data, and the index
        self.assertEquals(len(os.listdir(packdir)), numpacks * 2)

        store.markforrefresh()

        # The second one should repack all the packfiles into one.
        store.fetchpacksenabled = True
        store.refresh()
        self.assertEquals(len(store.packs), 1)

        # There should only be 2 files: the packfile, and the index
        self.assertEquals(len(os.listdir(packdir)), 2)

    def testCorruptPackHandling(self):
        """Test that the pack store deletes corrupt packs."""

        packdir = self.makeTempDir()
        deltachains = []

        numpacks = 5
        revisionsperpack = 100

        firstpack = None
        secondindex = None
        for i in range(numpacks):
            chain = []
            revision = (str(i), self.getFakeHash(), nullid, "content")

            for _ in range(revisionsperpack):
                chain.append(revision)
                revision = (str(i), self.getFakeHash(), revision[1], self.getFakeHash())

            pack = self.createPack(chain, packdir)
            if firstpack is None:
                firstpack = pack.packpath()
            elif secondindex is None:
                secondindex = pack.indexpath()

            deltachains.append(chain)

        ui = uimod.ui()
        store = datapackstore(ui, packdir, deletecorruptpacks=True)

        key = (deltachains[0][0][0], deltachains[0][0][1])
        # Count packs
        origpackcount = len(os.listdir(packdir))

        # Read key
        store.getdelta(*key)

        # Corrupt the pack
        os.chmod(firstpack, 0o644)
        f = open(firstpack, "w")
        f.truncate(1)
        f.close()

        # Re-create the store. Otherwise the behavior is kind of "undefined"
        # because the size of mmap-ed memory isn't truncated automatically,
        # and is filled by 0.
        store = datapackstore(ui, packdir, deletecorruptpacks=True)

        # Look for key again
        try:
            ui.pushbuffer(error=True)
            delta = store.getdelta(*key)
            raise RuntimeError("getdelta on corrupt key should fail %s" % repr(delta))
        except KeyError:
            pass
        ui.popbuffer()

        # Count packs
        newpackcount = len(os.listdir(packdir))

        # Assert the corrupt pack was removed
        self.assertEquals(origpackcount - 2, newpackcount)

        # Corrupt the index
        os.chmod(secondindex, 0o644)
        f = open(secondindex, "w")
        f.truncate(1)
        f.close()

        # Load the packs
        origpackcount = len(os.listdir(packdir))
        ui.pushbuffer(error=True)
        store = datapackstore(ui, packdir, deletecorruptpacks=True)
        # Constructing the store doesn't load the packfiles, these are loaded
        # on demand, and thus the detection of bad packfiles only happen then.
        # Let's force a refresh to make sure the bad pack files are deleted.
        store.refresh()
        ui.popbuffer()
        newpackcount = len(os.listdir(packdir))

        # Assert the corrupt pack was removed
        self.assertEquals(origpackcount - 2, newpackcount)

    def testReadingMutablePack(self):
        """Tests that the data written into a mutabledatapack can be read out
        before it has been finalized."""
        packdir = self.makeTempDir()
        packer = revisionstore.mutabledeltastore(packfilepath=packdir)

        # Add some unused first revision for noise
        packer.add("qwert", self.getFakeHash(), self.getFakeHash(), "qwertcontent")

        filename = "filename1"
        node = self.getFakeHash()
        base = self.getFakeHash()
        content = "asdf"
        meta = {constants.METAKEYFLAG: 1, constants.METAKEYSIZE: len(content)}
        packer.add(filename, node, base, content, metadata=meta)

        # Add some unused third revision for noise
        packer.add("zxcv", self.getFakeHash(), self.getFakeHash(), "zcxvcontent")

        # Test getmissing
        missing = ("", self.getFakeHash())
        value = packer.getmissing([missing, (filename, node)])
        self.assertEquals(value, [missing])

        # Test getmeta
        value = packer.getmeta(filename, node)
        self.assertEquals(value, meta)

        # Test getdelta
        value = packer.getdelta(filename, node)
        self.assertEquals(value, (content, filename, base, meta))

        # Test getdeltachain
        value = packer.getdeltachain(filename, node)
        self.assertEquals(value, [(filename, node, filename, base, content)])

    # perf test off by default since it's slow
    def _testIndexPerf(self):
        random.seed(0)
        print("Multi-get perf test")
        packsizes = [100, 10000, 100000, 500000, 1000000, 3000000]
        lookupsizes = [10, 100, 1000, 10000, 100000, 1000000]
        for packsize in packsizes:
            revisions = []
            for i in xrange(packsize):
                filename = "filename-%s" % i
                content = "content-%s" % i
                node = self.getHash(content)
                revisions.append((filename, node, nullid, content))

            path = self.createPack(revisions).path()

            # Perf of large multi-get
            import gc

            gc.disable()
            pack = self.datapackreader(path)
            for lookupsize in lookupsizes:
                if lookupsize > packsize:
                    continue
                random.shuffle(revisions)
                findnodes = [(rev[0], rev[1]) for rev in revisions]

                start = time.time()
                pack.getmissing(findnodes[:lookupsize])
                elapsed = time.time() - start
                print(
                    "%s pack %s lookups = %0.04f"
                    % (
                        ("%s" % packsize).rjust(7),
                        ("%s" % lookupsize).rjust(7),
                        elapsed,
                    )
                )

            print("")
            gc.enable()

        # The perf test is meant to produce output, so we always fail the test
        # so the user sees the output.
        raise RuntimeError("perf test always fails")


class rustdatapacktests(datapacktestsbase, unittest.TestCase):
    def __init__(self, *args, **kwargs):
        datapacktestsbase.__init__(self, revisionstore.datapack)
        unittest.TestCase.__init__(self, *args, **kwargs)


# TODO:
# datapack store:
# - getmissing
# - GC two packs into one

if __name__ == "__main__":
    silenttestrunner.main(__name__)
