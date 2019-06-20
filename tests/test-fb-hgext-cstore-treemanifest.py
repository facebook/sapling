#!/usr/bin/env python2.7
from __future__ import absolute_import

import random
import unittest

import silenttestrunner
from edenscm.mercurial import manifest, match as matchmod
from edenscm.mercurial.node import hex, nullid
from edenscmnative import cstore


class FakeDataStore(object):
    def __init__(self):
        self._data = {}

    def get(self, path, node):
        return self._data[(path, node)]

    def add(self, path, node, deltabase, value):
        self._data[(path, node)] = value


class FakeHistoryStore(object):
    def __init__(self):
        self._data = {}

    def getancestors(self, queryname, querynode):
        results = {}
        queue = [(queryname, querynode)]
        while queue:
            name, node = queue.pop()
            p1, p2, linknode, copyfrom = self._data[(name, node)]
            results[node] = (p1, p2, linknode, copyfrom)
            if p1 != nullid:
                queue.append((copyfrom or name, p1))
            if p2 != nullid:
                queue.append((name, p2))

        return results

    def add(self, path, node, p1, p2, linknode, copyfrom):
        self._data[(path, node)] = (p1, p2, linknode, copyfrom)


def getvalidflag():
    # t is reserved as a directory entry, so don't go around setting that as the
    # flag.
    while True:
        r = random.randint(0, 255)
        if r != ord("t"):
            return chr(r)


def hashflags(requireflag=False):
    h = "".join([chr(random.randint(0, 255)) for x in range(20)])
    if random.randint(0, 1) == 0 and requireflag is False:
        f = ""
    else:
        f = getvalidflag()
    return h, f


class ctreemanifesttests(unittest.TestCase):
    def setUp(self):
        random.seed(0)

    def testInitialization(self):
        cstore.treemanifest(FakeDataStore())

    def testEmptyFlag(self):
        a = cstore.treemanifest(FakeDataStore())
        h, f = hashflags()[0], ""
        a.set("abc", h, f)
        out = a.find("abc")
        self.assertEquals((h, f), out)

    def testNullFlag(self):
        a = cstore.treemanifest(FakeDataStore())
        h, f = hashflags()[0], "\0"
        a.set("abc", h, f)
        out = a.find("abc")
        self.assertEquals((h, f), out)

    def testSetGet(self):
        a = cstore.treemanifest(FakeDataStore())
        h, f = hashflags()
        a.set("abc", h, f)
        out = a.find("abc")
        self.assertEquals((h, f), out)

    def testUpdate(self):
        a = cstore.treemanifest(FakeDataStore())
        h, f = hashflags()
        a.set("abc", h, f)
        out = a.find("abc")
        self.assertEquals((h, f), out)

        h, f = hashflags()
        a.set("abc", h, f)
        out = a.find("abc")
        self.assertEquals((h, f), out)

    def testDirAfterFile(self):
        a = cstore.treemanifest(FakeDataStore())
        file_h, file_f = hashflags()
        a.set("abc", file_h, file_f)
        out = a.find("abc")
        self.assertEquals((file_h, file_f), out)

        dir_h, dir_f = hashflags()
        a.set("abc/def", dir_h, dir_f)
        out = a.find("abc/def")
        self.assertEquals((dir_h, dir_f), out)

        out = a.find("abc")
        self.assertEquals((file_h, file_f), out)

    def testFileAfterDir(self):
        a = cstore.treemanifest(FakeDataStore())
        dir_h, dir_f = hashflags()
        a.set("abc/def", dir_h, dir_f)
        out = a.find("abc/def")
        self.assertEquals((dir_h, dir_f), out)

        file_h, file_f = hashflags()
        a.set("abc", file_h, file_f)
        out = a.find("abc")
        self.assertEquals((file_h, file_f), out)

        out = a.find("abc/def")
        self.assertEquals((dir_h, dir_f), out)

    def testDeeplyNested(self):
        a = cstore.treemanifest(FakeDataStore())
        h, f = hashflags()
        a.set("abc/def/ghi/jkl", h, f)
        out = a.find("abc/def/ghi/jkl")
        self.assertEquals((h, f), out)

        h, f = hashflags()
        a.set("abc/def/ghi/jkl2", h, f)
        out = a.find("abc/def/ghi/jkl2")
        self.assertEquals((h, f), out)

    def testBushyTrees(self):
        a = cstore.treemanifest(FakeDataStore())
        nodes = {}
        for ix in range(111):
            h, f = hashflags()
            nodes["abc/def/ghi/jkl%d" % ix] = (h, f)

        for fp, (h, f) in nodes.items():
            a.set(fp, h, f)

        for fp, (h, f) in nodes.items():
            out = a.find(fp)
            self.assertEquals((h, f), out)

    def testFlagChanges(self):
        a = cstore.treemanifest(FakeDataStore())

        # go from no flags to with flags, back to no flags.
        h, f = hashflags(requireflag=True)
        self.assertEquals(len(f), 1)

        a.set("abc", h, "")
        out = a.find("abc")
        self.assertEquals(h, out[0])
        self.assertEquals("", out[1])

        a.set("abc", h, f)
        out = a.find("abc")
        self.assertEquals(h, out[0])
        self.assertEquals(f, out[1])

        a.set("abc", h, "")
        out = a.find("abc")
        self.assertEquals(h, out[0])
        self.assertEquals("", out[1])

    def testSetRemove(self):
        a = cstore.treemanifest(FakeDataStore())
        h, f = hashflags()
        a.set("abc", h, f)
        out = a.find("abc")
        self.assertEquals((h, f), out)

        a.set("abc", None, None)
        try:
            out = a.find("abc")
            raise RuntimeError("set should've removed file abc")
        except KeyError:
            pass

    def testCleanupAfterRemove(self):
        a = cstore.treemanifest(FakeDataStore())
        h, f = hashflags()
        a.set("abc/def/ghi", h, f)
        out = a.find("abc/def/ghi")
        self.assertEquals((h, f), out)

        a.set("abc/def/ghi", None, None)

        h, f = hashflags()
        a.set("abc", h, f)
        out = a.find("abc")
        self.assertEquals((h, f), out)

    def testIterOrder(self):
        a = cstore.treemanifest(FakeDataStore())
        h, f = hashflags()
        a.set("abc/def/ghi", h, f)
        a.set("abc/def.ghi", h, f)

        results = [fp for fp in a]
        self.assertEquals(results[0], "abc/def.ghi")
        self.assertEquals(results[1], "abc/def/ghi")

    def testIterOrderSigned(self):
        a = cstore.treemanifest(FakeDataStore())
        h, f = hashflags()
        a.set("abc/def/\xe6\xe9", h, f)
        a.set("abc/def/gh", h, f)

        results = [fp for fp in a]
        self.assertEquals(results[0], "abc/def/gh")
        self.assertEquals(results[1], "abc/def/\xe6\xe9")

    def testWrite(self):
        a = cstore.treemanifest(FakeDataStore())
        a.set("abc/def/x", *hashflags())
        a.set("abc/def/y", *hashflags())
        a.set("abc/z", *hashflags())
        alinknode = hashflags()[0]

        dstore = FakeDataStore()
        hstore = FakeHistoryStore()
        for name, node, text, p1text, p1, p2 in a.finalize():
            dstore.add(name, node, nullid, text)
            hstore.add(name, node, p1, p2, alinknode, "")
            if not name:
                anode = node

        a2 = cstore.treemanifest(dstore, anode)
        self.assertEquals(list(a.iterentries()), list(a2.iterentries()))
        self.assertEquals(
            hstore.getancestors("", anode), {anode: (nullid, nullid, alinknode, "")}
        )

        b = a2.copy()
        b.set("lmn/v", *hashflags())
        b.set("abc/z", *hashflags())
        blinknode = hashflags()[0]

        for name, node, text, p1text, p1, p2 in b.finalize(a2):
            dstore.add(name, node, nullid, text)
            hstore.add(name, node, p1, p2, blinknode, "")
            if not name:
                bnode = node

        b2 = cstore.treemanifest(dstore, bnode)
        self.assertEquals(list(b.iterentries()), list(b2.iterentries()))
        self.assertEquals(
            hstore.getancestors("", bnode),
            {
                bnode: (anode, nullid, blinknode, ""),
                anode: (nullid, nullid, alinknode, ""),
            },
        )

    def testWriteNoChange(self):
        """Tests that making a change to a tree, then making a second change
        such that the result is a no-op, doesn't serialize that subtree. It
        should only serialize the root node, because we're giving the root node
        a new parent.
        """
        a = cstore.treemanifest(FakeDataStore())
        xhashflags = hashflags()
        a.set("abc/def/x", *xhashflags)
        a.set("abc/z", *hashflags())
        alinknode = hashflags()[0]

        dstore = FakeDataStore()
        hstore = FakeHistoryStore()
        for name, node, text, p1text, p1, p2 in a.finalize():
            dstore.add(name, node, nullid, text)
            hstore.add(name, node, p1, p2, alinknode, "")
            if not name:
                anode = node

        a2 = cstore.treemanifest(dstore, anode)

        b = a2.copy()
        b.set("abc/def/x", *hashflags())
        b.set("abc/def/x", *xhashflags)
        blinknode = hashflags()[0]

        newtrees = set()
        for name, node, text, p1text, p1, p2 in b.finalize(a2):
            newtrees.add((name, node))
            dstore.add(name, node, nullid, text)
            hstore.add(name, node, p1, p2, blinknode, "")

        self.assertEquals(newtrees, set([("", node)]))

    def testWriteReplaceFile(self):
        """Tests writing a manifest which replaces a file with a directory."""
        a = cstore.treemanifest(FakeDataStore())
        a.set("abc/a", *hashflags())
        a.set("abc/z", *hashflags())
        alinknode = hashflags()[0]

        dstore = FakeDataStore()
        hstore = FakeHistoryStore()
        for name, node, text, p1text, p1, p2 in a.finalize():
            dstore.add(name, node, nullid, text)
            hstore.add(name, node, p1, p2, alinknode, "")
            if not name:
                anode = node

        b = a.copy()
        b.set("abc/a", None, None)
        b.set("abc/a/foo", *hashflags())
        blinknode = hashflags()[0]

        for name, node, text, p1text, p1, p2 in b.finalize(a):
            dstore.add(name, node, nullid, text)
            hstore.add(name, node, p1, p2, blinknode, "")
            if not name:
                bnode = node

        b2 = cstore.treemanifest(dstore, bnode)
        self.assertEquals(list(b.iterentries()), list(b2.iterentries()))
        self.assertEquals(
            hstore.getancestors("", bnode),
            {
                bnode: (anode, nullid, blinknode, ""),
                anode: (nullid, nullid, alinknode, ""),
            },
        )

    def testGet(self):
        a = cstore.treemanifest(FakeDataStore())
        zflags = hashflags()
        a.set("abc/z", *zflags)

        self.assertEquals(a.get("abc/z"), zflags[0])
        self.assertEquals(a.get("abc/x"), None)
        self.assertEquals(a.get("abc"), None)

    def testFind(self):
        a = cstore.treemanifest(FakeDataStore())
        zflags = hashflags()
        a.set("abc/z", *zflags)

        self.assertEquals(a.find("abc/z"), zflags)
        try:
            a.find("abc/x")
            raise RuntimeError("find for non-existent file should throw")
        except KeyError:
            pass

    def testSetFlag(self):
        a = cstore.treemanifest(FakeDataStore())
        zflags = hashflags()
        a.set("abc/z", *zflags)
        a.setflag("abc/z", "")
        self.assertEquals(a.flags("abc/z"), "")

        a.setflag("abc/z", "d")
        self.assertEquals(a.flags("abc/z"), "d")

        try:
            a.setflag("foo", "d")
            raise RuntimeError("setflag should throw")
        except KeyError:
            pass

    def testSetItem(self):
        a = cstore.treemanifest(FakeDataStore())
        zflags = hashflags(requireflag=True)
        a.set("abc/z", *zflags)

        fooflags = hashflags()
        a["foo"] = fooflags[0]
        self.assertEquals(a.find("foo"), (fooflags[0], ""))

        newnode = hashflags()[0]
        a["abc/z"] = newnode
        self.assertEquals(a.find("abc/z"), (newnode, zflags[1]))

    def testText(self):
        a = cstore.treemanifest(FakeDataStore())
        zflags = hashflags(requireflag=True)
        a.set("abc/z", *zflags)

        treetext = a.text()
        treetextv2 = a.text()

        b = manifest.manifestdict()
        b["abc/z"] = zflags[0]
        b.setflag("abc/z", zflags[1])
        fulltext = b.text()
        fulltextv2 = b.text()

        self.assertEquals(treetext, fulltext)
        self.assertEquals(treetextv2, fulltextv2)

    def testDiff(self):
        a = cstore.treemanifest(FakeDataStore())
        zflags = hashflags()
        mflags = hashflags()
        a.set("abc/z", *zflags)
        a.set("xyz/m", *mflags)
        alinknode = hashflags()[0]

        b = cstore.treemanifest(FakeDataStore())
        b.set("abc/z", *zflags)
        b.set("xyz/m", *mflags)
        blinknode = hashflags()[0]

        # Diff matching trees
        # - uncommitted trees
        diff = a.diff(b)
        self.assertEquals(diff, {})

        # - committed trees
        dstore = FakeDataStore()
        hstore = FakeHistoryStore()
        for name, node, text, p1text, p1, p2 in a.finalize():
            dstore.add(name, node, nullid, text)
            hstore.add(name, node, p1, p2, alinknode, "")
        for name, node, text, p1text, p1, p2 in b.finalize():
            dstore.add(name, node, nullid, text)
            hstore.add(name, node, p1, p2, blinknode, "")
        diff = a.diff(b)
        self.assertEquals(diff, {})

        b2 = b.copy()

        # Diff with modifications
        newfileflags = hashflags()
        newzflags = hashflags()
        b2.set("newfile", *newfileflags)
        b2.set("abc/z", *newzflags)

        # - uncommitted trees
        diff = a.diff(b2)
        self.assertEquals(
            diff, {"newfile": ((None, ""), newfileflags), "abc/z": (zflags, newzflags)}
        )

        # - uncommitted trees with matcher
        match = matchmod.match("/", "/", ["abc/*"])
        diff = a.diff(b2, match=match)
        self.assertEquals(diff, {"abc/z": (zflags, newzflags)})

        match = matchmod.match("/", "/", ["newfile"])
        diff = a.diff(b2, match=match)
        self.assertEquals(diff, {"newfile": ((None, ""), newfileflags)})

        # - committed trees
        for name, node, text, p1text, p1, p2 in b2.finalize(a):
            dstore.add(name, node, nullid, text)
            hstore.add(name, node, p1, p2, blinknode, "")

        diff = a.diff(b2)
        self.assertEquals(
            diff, {"newfile": ((None, ""), newfileflags), "abc/z": (zflags, newzflags)}
        )

        # Diff with clean
        diff = a.diff(b2, clean=True)
        self.assertEquals(
            diff,
            {
                "newfile": ((None, ""), newfileflags),
                "abc/z": (zflags, newzflags),
                "xyz/m": None,
            },
        )

    def testFilesNotIn(self):
        a = cstore.treemanifest(FakeDataStore())
        zflags = hashflags()
        mflags = hashflags()
        a.set("abc/z", *zflags)
        a.set("xyz/m", *mflags)
        alinknode = hashflags()[0]

        b = cstore.treemanifest(FakeDataStore())
        b.set("abc/z", *zflags)
        b.set("xyz/m", *mflags)
        blinknode = hashflags()[0]

        # filesnotin matching trees
        # - uncommitted trees
        diff = a.filesnotin(b)
        self.assertEquals(diff, set())

        # - committed trees
        dstore = FakeDataStore()
        hstore = FakeHistoryStore()
        for name, node, text, p1text, p1, p2 in a.finalize():
            dstore.add(name, node, nullid, text)
            hstore.add(name, node, p1, p2, alinknode, "")
        for name, node, text, p1text, p1, p2 in b.finalize():
            dstore.add(name, node, nullid, text)
            hstore.add(name, node, p1, p2, blinknode, "")
        diff = a.filesnotin(b)
        self.assertEquals(diff, set())

        # filesnotin with modifications
        newfileflags = hashflags()
        newzflags = hashflags()
        b.set("newfile", *newfileflags)
        b.set("abc/z", *newzflags)

        # In 'a' and not in 'b'
        files = a.filesnotin(b)
        self.assertEquals(files, set())

        # In 'b' and not in 'a'
        files = b.filesnotin(a)
        self.assertEquals(files, set(["newfile"]))

        # With dir matcher
        match = matchmod.match("/", "/", ["abc/*"])
        files = b.filesnotin(a, match=match)
        self.assertEquals(files, set())

        # With file matcher
        match = matchmod.match("/", "/", ["newfile"])
        files = b.filesnotin(a, match=match)
        self.assertEquals(files, set(["newfile"]))

        # With no matches
        match = matchmod.match("/", "/", ["xxx"])
        files = b.filesnotin(a, match=match)
        self.assertEquals(files, set([]))

    def testHasDir(self):
        a = cstore.treemanifest(FakeDataStore())
        zflags = hashflags()
        a.set("abc/z", *zflags)

        self.assertFalse(a.hasdir("abc/z"))
        self.assertTrue(a.hasdir("abc"))
        self.assertFalse(a.hasdir("xyz"))

    def testListDir(self):
        a = cstore.treemanifest(FakeDataStore())
        zflags = hashflags()
        a.set("a/b/x", *zflags)
        a.set("a/b/y/_", *zflags)
        a.set("a/c/z", *zflags)
        a.set("1", *zflags)
        self.assertEquals(a.listdir(""), ["1", "a"])
        self.assertEquals(a.listdir("a"), ["b", "c"])
        self.assertEquals(a.listdir("a/b"), ["x", "y"])
        self.assertEquals(a.listdir("a/b/"), ["x", "y"])
        self.assertEquals(a.listdir("a/b/y"), ["_"])
        self.assertEquals(a.listdir("a/c"), ["z"])
        self.assertEquals(a.listdir("foo"), None)
        self.assertEquals(a.listdir("1"), None)
        self.assertEquals(a.listdir("1/"), None)
        self.assertEquals(a.listdir("a/b/y/_"), None)

    def testContains(self):
        a = cstore.treemanifest(FakeDataStore())
        zflags = hashflags()
        a.set("abc/z", *zflags)

        self.assertTrue("abc/z" in a)
        self.assertFalse("abc" in a)
        self.assertFalse(None in a)

    def testDirs(self):
        a = cstore.treemanifest(FakeDataStore())
        zflags = hashflags()
        a.set("abc/z", *zflags)

        dirs = a.dirs()
        self.assertTrue("abc" in dirs)
        self.assertFalse("abc/z" in dirs)

    def testNonZero(self):
        a = cstore.treemanifest(FakeDataStore())
        self.assertFalse(bool(a))
        zflags = hashflags()
        a.set("abc/z", *zflags)
        self.assertTrue(bool(a))

    def testFlags(self):
        a = cstore.treemanifest(FakeDataStore())
        zflags = hashflags(requireflag=True)
        a.set("abc/z", *zflags)

        self.assertEquals(a.flags("x"), "")
        self.assertEquals(a.flags("x", default="z"), "z")
        self.assertEquals(a.flags("abc/z"), zflags[1])

    def testMatches(self):
        a = cstore.treemanifest(FakeDataStore())
        zflags = hashflags()
        a.set("abc/z", *zflags)
        a.set("foo", *hashflags())

        match = matchmod.match("/", "/", ["abc/z"])

        result = a.matches(match)
        self.assertEquals(list(result.iterentries()), [("abc/z", zflags[0], zflags[1])])

        match = matchmod.match("/", "/", ["x"])
        result = a.matches(match)
        self.assertEquals(list(result.iterentries()), [])

    def testKeys(self):
        a = cstore.treemanifest(FakeDataStore())
        self.assertEquals(a.keys(), [])

        zflags = hashflags()
        a.set("abc/z", *zflags)
        a.set("foo", *hashflags())

        self.assertEquals(a.keys(), ["abc/z", "foo"])

    def testIterItems(self):
        a = cstore.treemanifest(FakeDataStore())
        self.assertEquals(list(a.iteritems()), [])

        zflags = hashflags()
        fooflags = hashflags()
        a.set("abc/z", *zflags)
        a.set("foo", *fooflags)

        self.assertEquals(
            list(a.iteritems()), [("abc/z", zflags[0]), ("foo", fooflags[0])]
        )

    def testWalkSubtrees(self):
        a = cstore.treemanifest(FakeDataStore())

        zflags = hashflags()
        fooflags = hashflags()
        a.set("abc/z", *zflags)
        a.set("foo", *fooflags)

        # Walk over inmemory tree
        subtrees = list(a.walksubtrees())
        self.assertEquals(
            subtrees,
            [
                (
                    "abc",
                    nullid,
                    "z\0%s%s\n" % (hex(zflags[0]), zflags[1]),
                    "",
                    nullid,
                    nullid,
                ),
                (
                    "",
                    nullid,
                    "abc\0%st\nfoo\0%s%s\n"
                    % (hex(nullid), hex(fooflags[0]), fooflags[1]),
                    "",
                    nullid,
                    nullid,
                ),
            ],
        )

        # Walk over finalized tree
        dstore = FakeDataStore()
        hstore = FakeHistoryStore()
        for name, node, text, p1text, p1, p2 in a.finalize():
            dstore.add(name, node, nullid, text)
            hstore.add(name, node, p1, p2, nullid, "")
            if name == "":
                rootnode = node
            if name == "abc":
                abcnode = node

        subtrees = list(a.walksubtrees())
        self.assertEquals(
            subtrees,
            [
                (
                    "abc",
                    abcnode,
                    "z\0%s%s\n" % (hex(zflags[0]), zflags[1]),
                    "",
                    nullid,
                    nullid,
                ),
                (
                    "",
                    rootnode,
                    "abc\0%st\nfoo\0%s%s\n"
                    % (hex(abcnode), hex(fooflags[0]), fooflags[1]),
                    "",
                    nullid,
                    nullid,
                ),
            ],
        )

    def testWalkSubdirtrees(self):
        a = cstore.treemanifest(FakeDataStore())

        zflags = hashflags()
        qflags = hashflags()
        fooflags = hashflags()
        a.set("abc/def/z", *zflags)
        a.set("abc/xyz/q", *qflags)
        a.set("mno/foo", *fooflags)

        # Walk over finalized tree
        dstore = FakeDataStore()
        hstore = FakeHistoryStore()
        for name, node, text, p1text, p1, p2 in a.finalize():
            dstore.add(name, node, nullid, text)
            hstore.add(name, node, p1, p2, nullid, "")
            if name == "abc":
                abcnode = node
            if name == "abc/xyz":
                abcxyznode = node
            if name == "abc/def":
                abcdefnode = node

        subtrees = list(
            cstore.treemanifest.walksubdirtrees(("abc/def", abcdefnode), dstore)
        )
        self.assertEquals(
            subtrees,
            [
                (
                    "abc/def",
                    abcdefnode,
                    "z\0%s%s\n" % (hex(zflags[0]), zflags[1]),
                    "",
                    nullid,
                    nullid,
                )
            ],
        )

        subtrees = list(cstore.treemanifest.walksubdirtrees(("abc", abcnode), dstore))
        self.assertEquals(
            subtrees,
            [
                (
                    "abc/def",
                    abcdefnode,
                    "z\0%s%s\n" % (hex(zflags[0]), zflags[1]),
                    "",
                    nullid,
                    nullid,
                ),
                (
                    "abc/xyz",
                    abcxyznode,
                    "q\0%s%s\n" % (hex(qflags[0]), qflags[1]),
                    "",
                    nullid,
                    nullid,
                ),
                (
                    "abc",
                    abcnode,
                    "def\0%st\n" % (hex(abcdefnode),)
                    + "xyz\0%st\n" % (hex(abcxyznode),),
                    "",
                    nullid,
                    nullid,
                ),
            ],
        )


if __name__ == "__main__":
    silenttestrunner.main(__name__)
