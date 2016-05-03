import binascii
import itertools
import random
import shutil
import struct
import tempfile
import time
import unittest

import silenttestrunner

from remotefilelog.datapack import datapack, mutabledatapack
from remotefilelog.datapack import datapackstore

from mercurial import util
from mercurial.node import hex, bin, nullid

class datapacktests(unittest.TestCase):
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
        return util.sha1(content).digest()

    def getFakeHash(self):
        return bin('1' * 40)

    def createPack(self, revisions=None):
        if revisions is None:
            revisions = [("filename", self.getFakeHash(), nullid, "content")]

        packdir = self.makeTempDir()
        packer = mutabledatapack(packdir)

        for filename, node, base, content in revisions:
            packer.add(filename, node, base, content)

        path = packer.close()
        return datapack(path)

    def testAddSingle(self):
        """Test putting a simple blob into a pack and reading it out.
        """
        filename = "foo"
        content = "abcdef"
        node = self.getHash(content)

        revisions = [(filename, node, nullid, content)]
        pack = self.createPack(revisions)

        chain = pack.getdeltachain(filename, node)
        self.assertEquals(content, chain[0][4])

    def testAddMultiple(self):
        """Test putting multiple unrelated blobs into a pack and reading them
        out.
        """
        revisions = []
        for i in range(10):
            filename = "foo%s" % i
            content = "abcdef%s" % i
            node = self.getHash(content)
            revisions.append((filename, node, nullid, content))

        pack = self.createPack(revisions)

        for filename, node, base, content in revisions:
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
                content = "content-%s" % j
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

        missing = pack.getmissing([("foo", revisions[0][1]),
                                   ("foo", revisions[1][1])])
        self.assertFalse(missing)

        fakenode = self.getFakeHash()
        missing = pack.getmissing([("foo", revisions[0][1]), ("foo", fakenode)])
        self.assertEquals(missing, [("foo", fakenode)])

    def testAddThrows(self):
        pack = self.createPack()

        try:
            pack.add('filename', nullid, 'contents')
            self.assertTrue(False, "datapack.add should throw")
        except RuntimeError:
            pass

    def testBadVersionThrows(self):
        pack = self.createPack()
        path = pack.path + '.datapack'
        with open(path) as f:
            raw = f.read()
        raw = struct.pack('!B', 1) + raw[1:]
        with open(path, 'w+') as f:
            f.write(raw)

        try:
            pack = datapack(pack.path)
            self.assertTrue(False, "bad version number should have thrown")
        except RuntimeError:
            pass

    # perf test off by default since it's slow
    def _testIndexPerf(self):
        random.seed(0)
        print "Multi-get perf test"
        packsizes = [
            100,
            10000,
            100000,
            500000,
            1000000,
            3000000,
        ]
        lookupsizes = [
            10,
            100,
            1000,
            10000,
            100000,
            1000000,
        ]
        for packsize in packsizes:
            revisions = []
            for i in xrange(packsize):
                filename = "filename-%s" % i
                content = "content-%s" % i
                node = self.getHash(content)
                revisions.append((filename, node, nullid, content))

            path = self.createPack(revisions).path

            # Perf of large multi-get
            import gc
            gc.disable()
            pack = datapack(path)
            for lookupsize in lookupsizes:
                if lookupsize > packsize:
                    continue
                random.shuffle(revisions)
                findnodes = [(rev[0], rev[1]) for rev in revisions]

                start = time.time()
                result = pack.getmissing(findnodes[:lookupsize])
                elapsed = time.time() - start
                print ("%s pack %s lookups = %0.04f" %
                       (('%s' % packsize).rjust(7),
                        ('%s' % lookupsize).rjust(7),
                        elapsed))

            print ""
            gc.enable()

        # The perf test is meant to produce output, so we always fail the test
        # so the user sees the output.
        raise RuntimeError("perf test always fails")

# TODO:
# datapack store:
# - getmissing
# - GC two packs into one

if __name__ == '__main__':
    silenttestrunner.main(__name__)
