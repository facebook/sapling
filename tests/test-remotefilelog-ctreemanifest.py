#!/usr/bin/env python2.7

import random
import unittest

import silenttestrunner

import ctreemanifest

class FakeStore(object):
    def get(self, xyz):
        return "abcabcabc"

def getvalidflag():
    # t is reserved as a directory entry, so don't go around setting that as the
    # flag.
    while True:
        r = random.randint(0, 255)
        if r != ord('t'):
            return chr(r)

def hashflags(requireflag=False):
    h = ''.join([chr(random.randint(0, 255)) for x in range(20)])
    if random.randint(0, 1) == 0 and requireflag is False:
        f = ''
    else:
        f = getvalidflag()
    return h, f

class ctreemanifesttests(unittest.TestCase):
    def testInitialization(self):
        a = ctreemanifest.treemanifest(FakeStore())

    def testEmptyFlag(self):
        a = ctreemanifest.treemanifest(FakeStore())
        h, f = hashflags()[0], ''
        a.set("abc", h, f)
        out = a.find("abc")
        self.assertEquals((h, f), out)

    def testNullFlag(self):
        a = ctreemanifest.treemanifest(FakeStore())
        h, f = hashflags()[0], '\0'
        a.set("abc", h, f)
        out = a.find("abc")
        self.assertEquals((h, f), out)

    def testSetGet(self):
        a = ctreemanifest.treemanifest(FakeStore())
        h, f = hashflags()
        a.set("abc", h, f)
        out = a.find("abc")
        self.assertEquals((h, f), out)

    def testUpdate(self):
        a = ctreemanifest.treemanifest(FakeStore())
        h, f = hashflags()
        a.set("abc", h, f)
        out = a.find("abc")
        self.assertEquals((h, f), out)

        h, f = hashflags()
        a.set("abc", h, f)
        out = a.find("abc")
        self.assertEquals((h, f), out)

    def testDirAfterFile(self):
        a = ctreemanifest.treemanifest(FakeStore())
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
        a = ctreemanifest.treemanifest(FakeStore())
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
        a = ctreemanifest.treemanifest(FakeStore())
        h, f = hashflags()
        a.set("abc/def/ghi/jkl", h, f)
        out = a.find("abc/def/ghi/jkl")
        self.assertEquals((h, f), out)

        h, f = hashflags()
        a.set("abc/def/ghi/jkl2", h, f)
        out = a.find("abc/def/ghi/jkl2")
        self.assertEquals((h, f), out)

    def testDeeplyNested(self):
        a = ctreemanifest.treemanifest(FakeStore())
        h, f = hashflags()
        a.set("abc/def/ghi/jkl", h, f)
        out = a.find("abc/def/ghi/jkl")
        self.assertEquals((h, f), out)

        h, f = hashflags()
        a.set("abc/def/ghi/jkl2", h, f)
        out = a.find("abc/def/ghi/jkl2")
        self.assertEquals((h, f), out)

    def testBushyTrees(self):
        a = ctreemanifest.treemanifest(FakeStore())
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
        a = ctreemanifest.treemanifest(FakeStore())

        # go from no flags to with flags, back to no flags.
        h, f = hashflags(requireflag=True)
        self.assertEquals(len(f), 1)

        a.set("abc", h, '')
        out = a.find("abc")
        self.assertEquals(h, out[0])
        self.assertEquals('', out[1])

        a.set("abc", h, f)
        out = a.find("abc")
        self.assertEquals(h, out[0])
        self.assertEquals(f, out[1])

        a.set("abc", h, '')
        out = a.find("abc")
        self.assertEquals(h, out[0])
        self.assertEquals('', out[1])

    def testSetRemove(self):
        a = ctreemanifest.treemanifest(FakeStore())
        h, f = hashflags()
        a.set("abc", h, f)
        out = a.find("abc")
        self.assertEquals((h, f), out)

        a.set("abc", None, None)
        out = a.find("abc")
        self.assertEquals((None, None), out)

    def testCleanupAfterRemove(self):
        a = ctreemanifest.treemanifest(FakeStore())
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
        a = ctreemanifest.treemanifest(FakeStore())
        h, f = hashflags()
        a.set("abc/def/ghi", h, f)
        a.set("abc/def.ghi", h, f)

        results = [fp for fp in a]
        self.assertEquals(results[0], "abc/def.ghi")
        self.assertEquals(results[1], "abc/def/ghi")

if __name__ == '__main__':
    silenttestrunner.main(__name__)
