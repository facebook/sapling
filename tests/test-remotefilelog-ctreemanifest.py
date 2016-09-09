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
def hashflags():
    h = ''.join([chr(random.randint(0, 255)) for x in range(20)])
    if random.randint(0, 1) == 0:
        f = ''
    else:
        f = chr(random.randint(0, 255))
    yield (h, f)

class ctreemanifesttests(unittest.TestCase):
    def testInitialization(self):
        a = ctreemanifest.treemanifest(FakeStore())

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

if __name__ == '__main__':
    silenttestrunner.main(__name__)
