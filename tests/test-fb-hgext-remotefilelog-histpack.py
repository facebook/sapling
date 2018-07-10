#!/usr/bin/env python
from __future__ import absolute_import

import hashlib
import os
import random
import shutil
import stat
import struct
import tempfile
import unittest

import silenttestrunner
from hgext.remotefilelog.basepack import LARGEFANOUTPREFIX, SMALLFANOUTCUTOFF
from hgext.remotefilelog.historypack import historypack, mutablehistorypack
from mercurial import error, ui as uimod
from mercurial.node import nullid


try:
    xrange(0)
except NameError:
    xrange = range


class histpacktests(unittest.TestCase):
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

    def createPack(self, revisions=None):
        """Creates and returns a historypack containing the specified revisions.

        `revisions` is a list of tuples, where each tuple contains a filanem,
        node, p1node, p2node, and linknode.
        """
        if revisions is None:
            revisions = [
                (
                    "filename",
                    self.getFakeHash(),
                    nullid,
                    nullid,
                    self.getFakeHash(),
                    None,
                )
            ]

        packdir = self.makeTempDir()
        packer = mutablehistorypack(uimod.ui(), packdir, version=1)

        for filename, node, p1, p2, linknode, copyfrom in revisions:
            packer.add(filename, node, p1, p2, linknode, copyfrom)

        path = packer.close()
        return historypack(path)

    def testAddSingle(self):
        """Test putting a single entry into a pack and reading it out.
        """
        filename = "foo"
        node = self.getFakeHash()
        p1 = self.getFakeHash()
        p2 = self.getFakeHash()
        linknode = self.getFakeHash()

        revisions = [(filename, node, p1, p2, linknode, None)]
        pack = self.createPack(revisions)

        actual = pack.getancestors(filename, node)[node]
        self.assertEquals(p1, actual[0])
        self.assertEquals(p2, actual[1])
        self.assertEquals(linknode, actual[2])

    def testAddMultiple(self):
        """Test putting multiple unrelated revisions into a pack and reading
        them out.
        """
        revisions = []
        for i in range(10):
            filename = "foo-%s" % i
            node = self.getFakeHash()
            p1 = self.getFakeHash()
            p2 = self.getFakeHash()
            linknode = self.getFakeHash()
            revisions.append((filename, node, p1, p2, linknode, None))

        pack = self.createPack(revisions)

        for filename, node, p1, p2, linknode, copyfrom in revisions:
            actual = pack.getancestors(filename, node)[node]
            self.assertEquals(p1, actual[0])
            self.assertEquals(p2, actual[1])
            self.assertEquals(linknode, actual[2])
            self.assertEquals(copyfrom, actual[3])

    def testAddAncestorChain(self):
        """Test putting multiple revisions in into a pack and read the ancestor
        chain.
        """
        revisions = []
        filename = "foo"
        lastnode = nullid
        for i in range(10):
            node = self.getFakeHash()
            revisions.append((filename, node, lastnode, nullid, nullid, None))
            lastnode = node

        # revisions must be added in topological order, newest first
        revisions = list(reversed(revisions))
        pack = self.createPack(revisions)

        # Test that the chain has all the entries
        ancestors = pack.getancestors(revisions[0][0], revisions[0][1])
        for filename, node, p1, p2, linknode, copyfrom in revisions:
            ap1, ap2, alinknode, acopyfrom = ancestors[node]
            self.assertEquals(ap1, p1)
            self.assertEquals(ap2, p2)
            self.assertEquals(alinknode, linknode)
            self.assertEquals(acopyfrom, copyfrom)

    def testPackMany(self):
        """Pack many related and unrelated ancestors.
        """
        # Build a random pack file
        allentries = {}
        ancestorcounts = {}
        revisions = []
        random.seed(0)
        for i in range(100):
            filename = "filename-%s" % i
            entries = []
            p2 = nullid
            linknode = nullid
            for j in range(random.randint(1, 100)):
                node = self.getFakeHash()
                p1 = nullid
                if len(entries) > 0:
                    p1 = entries[random.randint(0, len(entries) - 1)]
                entries.append(node)
                revisions.append((filename, node, p1, p2, linknode, None))
                allentries[(filename, node)] = (p1, p2, linknode)
                if p1 == nullid:
                    ancestorcounts[(filename, node)] = 1
                else:
                    newcount = ancestorcounts[(filename, p1)] + 1
                    ancestorcounts[(filename, node)] = newcount

        # Must add file entries in reverse topological order
        revisions = list(reversed(revisions))
        pack = self.createPack(revisions)

        # Verify the pack contents
        for (filename, node), (p1, p2, lastnode) in allentries.iteritems():
            ancestors = pack.getancestors(filename, node)
            self.assertEquals(ancestorcounts[(filename, node)], len(ancestors))
            for anode, (ap1, ap2, alinknode, copyfrom) in ancestors.iteritems():
                ep1, ep2, elinknode = allentries[(filename, anode)]
                self.assertEquals(ap1, ep1)
                self.assertEquals(ap2, ep2)
                self.assertEquals(alinknode, elinknode)
                self.assertEquals(copyfrom, None)

    def testGetNodeInfo(self):
        revisions = []
        filename = "foo"
        lastnode = nullid
        for i in range(10):
            node = self.getFakeHash()
            revisions.append((filename, node, lastnode, nullid, nullid, None))
            lastnode = node

        pack = self.createPack(revisions)

        # Test that getnodeinfo returns the expected results
        for filename, node, p1, p2, linknode, copyfrom in revisions:
            ap1, ap2, alinknode, acopyfrom = pack.getnodeinfo(filename, node)
            self.assertEquals(ap1, p1)
            self.assertEquals(ap2, p2)
            self.assertEquals(alinknode, linknode)
            self.assertEquals(acopyfrom, copyfrom)

    def testGetMissing(self):
        """Test the getmissing() api.
        """
        revisions = []
        filename = "foo"
        for i in range(10):
            node = self.getFakeHash()
            p1 = self.getFakeHash()
            p2 = self.getFakeHash()
            linknode = self.getFakeHash()
            revisions.append((filename, node, p1, p2, linknode, None))

        pack = self.createPack(revisions)

        missing = pack.getmissing([(filename, revisions[0][1])])
        self.assertFalse(missing)

        missing = pack.getmissing(
            [(filename, revisions[0][1]), (filename, revisions[1][1])]
        )
        self.assertFalse(missing)

        fakenode = self.getFakeHash()
        missing = pack.getmissing([(filename, revisions[0][1]), (filename, fakenode)])
        self.assertEquals(missing, [(filename, fakenode)])

        # Test getmissing on a non-existant filename
        missing = pack.getmissing([("bar", fakenode)])
        self.assertEquals(missing, [("bar", fakenode)])

    def testAddThrows(self):
        pack = self.createPack()

        try:
            pack.add("filename", nullid, nullid, nullid, nullid, None)
            self.assertTrue(False, "historypack.add should throw")
        except RuntimeError:
            pass

    def testBadVersionThrows(self):
        pack = self.createPack()
        path = pack.path() + ".histpack"
        with open(path) as f:
            raw = f.read()
        raw = struct.pack("!B", 255) + raw[1:]
        os.chmod(path, os.stat(path).st_mode | stat.S_IWRITE)
        with open(path, "w+") as f:
            f.write(raw)

        try:
            pack = historypack(pack.path())
            self.assertTrue(False, "bad version number should have thrown")
        except RuntimeError:
            pass

    def testLargePack(self):
        """Test creating and reading from a large pack with over X entries.
        This causes it to use a 2^16 fanout table instead."""
        total = SMALLFANOUTCUTOFF + 1
        revisions = []
        for i in xrange(total):
            filename = "foo-%s" % i
            node = self.getFakeHash()
            p1 = self.getFakeHash()
            p2 = self.getFakeHash()
            linknode = self.getFakeHash()
            revisions.append((filename, node, p1, p2, linknode, None))

        pack = self.createPack(revisions)
        self.assertEquals(pack.params.fanoutprefix, LARGEFANOUTPREFIX)

        for filename, node, p1, p2, linknode, copyfrom in revisions:
            actual = pack.getancestors(filename, node)[node]
            self.assertEquals(p1, actual[0])
            self.assertEquals(p2, actual[1])
            self.assertEquals(linknode, actual[2])
            self.assertEquals(copyfrom, actual[3])

    def testReadingMutablePack(self):
        """Tests that the data written into a mutablehistorypack can be read out
        before it has been finalized."""
        packdir = self.makeTempDir()
        packer = mutablehistorypack(uimod.ui(), packdir, version=1)

        revisions = []

        filename = "foo"
        lastnode = nullid
        for i in range(5):
            node = self.getFakeHash()
            revisions.append((filename, node, lastnode, nullid, nullid, ""))
            lastnode = node

        filename = "bar"
        lastnode = nullid
        for i in range(5):
            node = self.getFakeHash()
            revisions.append((filename, node, lastnode, nullid, nullid, ""))
            lastnode = node

        for filename, node, p1, p2, linknode, copyfrom in revisions:
            packer.add(filename, node, p1, p2, linknode, copyfrom)

        # Test getancestors()
        for filename, node, p1, p2, linknode, copyfrom in revisions:
            entry = packer.getancestors(filename, node)
            self.assertEquals(entry, {node: (p1, p2, linknode, copyfrom)})

        # Test getmissing()
        missingcheck = [(revisions[0][0], revisions[0][1]), ("foo", self.getFakeHash())]
        missing = packer.getmissing(missingcheck)
        self.assertEquals(missing, missingcheck[1:])

    def testWritingLinkRevs(self):
        """Tests that we can add linkrevs and have them written as linknodes.
        """

        class fakerepo(object):
            def __init__(self):
                self.changelog = fakechangelog()

        class fakechangelog(object):
            def __init__(self):
                self.commits = []

            def __len__(self):
                return len(self.commits)

            def rev(self, node):
                try:
                    return self.commits.index(node)
                except Exception:
                    raise error.LookupError(hex(node), "x", "x")

            def node(self, rev):
                if rev >= len(self.commits):
                    raise error.LookupError(rev, "x", "x")
                return self.commits[rev]

        repo = fakerepo()
        packdir = self.makeTempDir()
        packer = mutablehistorypack(uimod.ui(), packdir, version=1, repo=repo)

        revisions = []
        commits = []

        filename = "foo"
        lastnode = nullid
        for i in range(5):
            node = self.getFakeHash()
            linknode = self.getFakeHash()
            commits.append(linknode)
            revisions.append((filename, node, lastnode, nullid, linknode, ""))
            lastnode = node

        for filename, node, p1, p2, linknode, copyfrom in revisions:
            packer.add(
                filename, node, p1, p2, None, copyfrom, linkrev=commits.index(linknode)
            )

        # Test adding linknode and linkrev
        try:
            packer.add(
                "",
                self.getFakeHash(),
                self.getFakeHash(),
                nullid,
                self.getFakeHash(),
                "",
                5,
            )
            self.assertFalse(True, "Adding linknode and linkrev should've " "thrown")
        except error.ProgrammingError:
            pass

        # Test getancestors before finalizing
        try:
            filename, node = revisions[0][:2]
            packer.getancestors(filename, node)
            self.assertFalse(True, "Reading data before finalizing should've " "thrown")
        except error.ProgrammingError:
            pass

        # "Commit" the commits to the changelog
        repo.changelog.commits = commits

        # Verify reading from the mutable store
        for filename, node, p1, p2, linknode, copyfrom in revisions:
            entry = packer.getnodeinfo(filename, node)
            self.assertEquals(entry, (p1, p2, linknode, copyfrom))

        path = packer.close()
        pack = historypack(path)

        # Verify reading from the on disk pack
        for filename, node, p1, p2, linknode, copyfrom in revisions:
            entry = pack.getnodeinfo(filename, node)
            copyfrom = None if not copyfrom else copyfrom
            self.assertEquals(entry, (p1, p2, linknode, copyfrom))


# TODO:
# histpack store:
# - repack two packs into one

if __name__ == "__main__":
    silenttestrunner.main(__name__)
