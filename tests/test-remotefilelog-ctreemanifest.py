#!/usr/bin/env python2.7

import random
import unittest
from contextlib import contextmanager

import silenttestrunner

import ctreemanifest

class FakeStore(object):
    def get(self, xyz):
        return "abcabcabc"

@contextmanager
def hashflags(requireflag=False):
    h = ''.join([chr(random.randint(0, 255)) for x in range(20)])
    if random.randint(0, 1) == 0 and requireflag is False:
        f = ''
    else:
        f = chr(random.randint(0, 255))
    yield (h, f)

class ctreemanifesttests(unittest.TestCase):
    def testInitialization(self):
        a = ctreemanifest.treemanifest(FakeStore())

    def testEmptyFlag(self):
        a = ctreemanifest.treemanifest(FakeStore())
        with hashflags() as (h, _):
            f = ''
            a.set("abc", h, f)
            out = a.find("abc")
            self.assertEquals((h, f), out)

    def testNullFlag(self):
        a = ctreemanifest.treemanifest(FakeStore())
        with hashflags() as (h, _):
            f = '\0'
            a.set("abc", h, f)
            out = a.find("abc")
            self.assertEquals((h, f), out)

    def testSetGet(self):
        a = ctreemanifest.treemanifest(FakeStore())
        with hashflags() as (h, f):
            a.set("abc", h, f)
            out = a.find("abc")
            self.assertEquals((h, f), out)

    def testUpdate(self):
        a = ctreemanifest.treemanifest(FakeStore())
        with hashflags() as (h, f):
            a.set("abc", h, f)
            out = a.find("abc")
            self.assertEquals((h, f), out)

        with hashflags() as (h, f):
            a.set("abc", h, f)
            out = a.find("abc")
            self.assertEquals((h, f), out)

    def testConflict(self):
        a = ctreemanifest.treemanifest(FakeStore())
        with hashflags() as (h, f):
            a.set("abc", h, f)
            out = a.find("abc")
            self.assertEquals((h, f), out)

        with hashflags() as (h, f):
            self.assertRaises(
                TypeError,
                lambda: a.set("abc/def", h, f)
            )

    def testDeeplyNested(self):
        a = ctreemanifest.treemanifest(FakeStore())
        with hashflags() as (h, f):
            a.set("abc/def/ghi/jkl", h, f)
            out = a.find("abc/def/ghi/jkl")
            self.assertEquals((h, f), out)

        with hashflags() as (h, f):
            a.set("abc/def/ghi/jkl2", h, f)
            out = a.find("abc/def/ghi/jkl2")
            self.assertEquals((h, f), out)

    def testDeeplyNested(self):
        a = ctreemanifest.treemanifest(FakeStore())
        with hashflags() as (h, f):
            a.set("abc/def/ghi/jkl", h, f)
            out = a.find("abc/def/ghi/jkl")
            self.assertEquals((h, f), out)

        with hashflags() as (h, f):
            a.set("abc/def/ghi/jkl2", h, f)
            out = a.find("abc/def/ghi/jkl2")
            self.assertEquals((h, f), out)

    def testBushyTrees(self):
        a = ctreemanifest.treemanifest(FakeStore())
        nodes = {}
        for ix in range(111):
            with hashflags() as (h, f):
                nodes["abc/def/ghi/jkl%d" % ix] = (h, f)

        for fp, (h, f) in nodes.items():
            a.set(fp, h, f)

        for fp, (h, f) in nodes.items():
            out = a.find(fp)
            self.assertEquals((h, f), out)

    def testFlagChanges(self):
        a = ctreemanifest.treemanifest(FakeStore())

        # go from no flags to with flags, back to no flags.
        with hashflags(requireflag=True) as (h, f):
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
        with hashflags() as (h, f):
            a.set("abc", h, f)
            out = a.find("abc")
            self.assertEquals((h, f), out)

            a.set("abc", None, None)
            out = a.find("abc")
            self.assertEquals((None, None), out)

    def testCleanupAfterRemove(self):
        a = ctreemanifest.treemanifest(FakeStore())
        with hashflags() as (h, f):
            a.set("abc/def/ghi", h, f)
            out = a.find("abc/def/ghi")
            self.assertEquals((h, f), out)

        a.set("abc/def/ghi", None, None)

        with hashflags() as (h, f):
            a.set("abc", h, f)
            out = a.find("abc")
            self.assertEquals((h, f), out)

if __name__ == '__main__':
    silenttestrunner.main(__name__)
