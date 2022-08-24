from __future__ import absolute_import

import unittest

import silenttestrunner
from edenscm import match as matchmod
from hghave import require


class NeverMatcherTests(unittest.TestCase):
    def testVisitdir(self):
        m = matchmod.nevermatcher("", "")
        self.assertFalse(m.visitdir(""))
        self.assertFalse(m.visitdir("dir"))

    def testManyGlobRaises(self):
        n = 10000
        rules = ["a/b/*/c/d/e/f/g/%s/**" % i for i in range(n)]
        with self.assertRaises(Exception):
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


class ExplainTreeMatcherTests(unittest.TestCase):
    def testExplain(self):
        m = matchmod.treematcher("/", "", rules=["foo/bar", "!baz", "qux", "!qux"])
        self.assertEqual(m.explain("blah"), None)
        self.assertEqual(m.explain("baz"), "!baz")
        self.assertEqual(m.explain("qux"), "!qux")
        self.assertEqual(m.explain("foo/bar"), "foo/bar")

        m = matchmod.treematcher(
            "/", "", rules=["foo/bar", "!baz"], ruledetails=["a", "b"]
        )
        self.assertEqual(m.explain("blah"), None)
        self.assertEqual(m.explain("baz"), "!baz (b)")
        self.assertEqual(m.explain("foo/bar"), "foo/bar (a)")

    # Works for unions of tree matchers as well.
    def testExplainUnion(self):
        m = matchmod.unionmatcher(
            [
                matchmod.treematcher("/", "", rules=["f*"]),
                matchmod.treematcher("/", "", rules=["bar"]),
            ]
        )
        self.assertEqual(m.explain("foo"), "f*")


if __name__ == "__main__":
    silenttestrunner.main(__name__)
