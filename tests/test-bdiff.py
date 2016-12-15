from __future__ import absolute_import, print_function
import collections
import struct
import unittest

import silenttestrunner

from mercurial import (
    bdiff,
    mpatch,
)

class diffreplace(
    collections.namedtuple('diffreplace', 'start end from_ to')):
    def __repr__(self):
        return 'diffreplace(%r, %r, %r, %r)' % self

class BdiffTests(unittest.TestCase):

    def assert_bdiff_applies(self, a, b):
        d = bdiff.bdiff(a, b)
        c = a
        if d:
            c = mpatch.patches(a, [d])
        self.assertEqual(
            c, b, ("bad diff+patch result from\n  %r to\n  "
                   "%r: \nbdiff: %r\npatched: %r" % (a, b, d, c[:200])))

    def assert_bdiff(self, a, b):
        self.assert_bdiff_applies(a, b)
        self.assert_bdiff_applies(b, a)

    def test_bdiff_basic(self):
        cases = [
            ("a\nc\n\n\n\n", "a\nb\n\n\n"),
            ("a\nb\nc\n", "a\nc\n"),
            ("", ""),
            ("a\nb\nc", "a\nb\nc"),
            ("a\nb\nc\nd\n", "a\nd\n"),
            ("a\nb\nc\nd\n", "a\nc\ne\n"),
            ("a\nb\nc\n", "a\nc\n"),
            ("a\n", "c\na\nb\n"),
            ("a\n", ""),
            ("a\n", "b\nc\n"),
            ("a\n", "c\na\n"),
            ("", "adjfkjdjksdhfksj"),
            ("", "ab"),
            ("", "abc"),
            ("a", "a"),
            ("ab", "ab"),
            ("abc", "abc"),
            ("a\n", "a\n"),
            ("a\nb", "a\nb"),
        ]
        for a, b in cases:
            self.assert_bdiff(a, b)

    def showdiff(self, a, b):
        bin = bdiff.bdiff(a, b)
        pos = 0
        q = 0
        actions = []
        while pos < len(bin):
            p1, p2, l = struct.unpack(">lll", bin[pos:pos + 12])
            pos += 12
            if p1:
                actions.append(a[q:p1])
            actions.append(diffreplace(p1, p2, a[p1:p2], bin[pos:pos + l]))
            pos += l
            q = p2
        if q < len(a):
            actions.append(a[q:])
        return actions

    def test_issue1295(self):
        cases = [
            ("x\n\nx\n\nx\n\nx\n\nz\n", "x\n\nx\n\ny\n\nx\n\nx\n\nz\n",
             ['x\n\nx\n\n', diffreplace(6, 6, '', 'y\n\n'), 'x\n\nx\n\nz\n']),
            ("x\n\nx\n\nx\n\nx\n\nz\n", "x\n\nx\n\ny\n\nx\n\ny\n\nx\n\nz\n",
             ['x\n\nx\n\n',
              diffreplace(6, 6, '', 'y\n\n'),
              'x\n\n',
              diffreplace(9, 9, '', 'y\n\n'),
              'x\n\nz\n']),
            # we should pick up abbbc. rather than bc.de as the longest match
            ("a\nb\nb\nb\nc\n.\nd\ne\n.\nf\n",
             "a\nb\nb\na\nb\nb\nb\nc\n.\nb\nc\n.\nd\ne\nf\n",
             ['a\nb\nb\n',
              diffreplace(6, 6, '', 'a\nb\nb\nb\nc\n.\n'),
              'b\nc\n.\nd\ne\n',
              diffreplace(16, 18, '.\n', ''),
              'f\n']),
        ]
        for old, new, want in cases:
            self.assertEqual(self.showdiff(old, new), want)

def showdiff(a, b):
    print('showdiff(\n  %r,\n  %r):' % (a, b))
    bin = bdiff.bdiff(a, b)
    pos = 0
    q = 0
    while pos < len(bin):
        p1, p2, l = struct.unpack(">lll", bin[pos:pos + 12])
        pos += 12
        if p1:
            print('', repr(a[q:p1]))
        print('', p1, p2, repr(a[p1:p2]), '->', repr(bin[pos:pos + l]))
        pos += l
        q = p2
    if q < len(a):
        print('', repr(a[q:]))

def testfixws(a, b, allws):
    c = bdiff.fixws(a, allws)
    if c != b:
        print("*** fixws", repr(a), repr(b), allws)
        print("got:")
        print(repr(c))

testfixws(" \ta\r b\t\n", "ab\n", 1)
testfixws(" \ta\r b\t\n", " a b\n", 0)
testfixws("", "", 1)
testfixws("", "", 0)

print("done")

print("Nice diff for a trivial change:")
showdiff(
    ''.join('<%s\n-\n' % i for i in range(5)),
    ''.join('>%s\n-\n' % i for i in range(5)))

print("Diff 1 to 3 lines - preference for appending:")
showdiff('a\n', 'a\n' * 3)
print("Diff 1 to 5 lines - preference for appending:")
showdiff('a\n', 'a\n' * 5)
print("Diff 3 to 1 lines - preference for removing trailing lines:")
showdiff('a\n' * 3, 'a\n')
print("Diff 5 to 1 lines - preference for removing trailing lines:")
showdiff('a\n' * 5, 'a\n')

if __name__ == '__main__':
    silenttestrunner.main(__name__)
