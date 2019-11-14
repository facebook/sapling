from __future__ import absolute_import

import unittest

import silenttestrunner
from edenscm.mercurial import match as matchmod


class NeverMatcherTests(unittest.TestCase):
    def testVisitdir(self):
        m = matchmod.nevermatcher("", "")
        self.assertFalse(m.visitdir(""))
        self.assertFalse(m.visitdir("dir"))

    def testManyGlobRaises(self):
        n = 10000
        rules = ["a/b/*/c/d/e/f/g/%s/**" % i for i in range(n)]
        with self.assertRaises(ValueError):
            # "Compiled regex exceeds size limit of 10485760 bytes."
            matchmod.treematcher("", "", rules=rules)

    def testManyPrefixes(self):
        n = 10000
        rules = ["a/b/c/d/e/f/g/%s/**" % i for i in range(n)]
        m = matchmod.treematcher("", "", rules=rules)
        self.assertTrue(m.visitdir("a"))
        self.assertTrue(m.visitdir("a/b"))
        self.assertEqual(m.visitdir("a/b/c/d/e/f/g/1"), "all")
        self.assertFalse(m.visitdir("b"))
        self.assertTrue(m("a/b/c/d/e/f/g/99/x"))


if __name__ == "__main__":
    silenttestrunner.main(__name__)
