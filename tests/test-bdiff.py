from __future__ import absolute_import, print_function
import collections
import struct
import unittest

from mercurial import (
    mdiff,
)

class diffreplace(
    collections.namedtuple('diffreplace', 'start end from_ to')):
    def __repr__(self):
        return 'diffreplace(%r, %r, %r, %r)' % self

class BdiffTests(unittest.TestCase):

    def assert_bdiff_applies(self, a, b):
        d = mdiff.textdiff(a, b)
        c = a
        if d:
            c = mdiff.patches(a, [d])
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
        bin = mdiff.textdiff(a, b)
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
        ]
        for old, new, want in cases:
            self.assertEqual(self.showdiff(old, new), want)

    def test_issue1295_varies_on_pure(self):
            # we should pick up abbbc. rather than bc.de as the longest match
        got = self.showdiff("a\nb\nb\nb\nc\n.\nd\ne\n.\nf\n",
                            "a\nb\nb\na\nb\nb\nb\nc\n.\nb\nc\n.\nd\ne\nf\n")
        want_c = ['a\nb\nb\n',
                  diffreplace(6, 6, '', 'a\nb\nb\nb\nc\n.\n'),
                  'b\nc\n.\nd\ne\n',
                  diffreplace(16, 18, '.\n', ''),
                  'f\n']
        want_pure = [diffreplace(0, 0, '', 'a\nb\nb\n'),
                     'a\nb\nb\nb\nc\n.\n',
                     diffreplace(12, 12, '', 'b\nc\n.\n'),
                     'd\ne\n',
                     diffreplace(16, 18, '.\n', ''), 'f\n']
        self.assert_(got in (want_c, want_pure),
                     'got: %r, wanted either %r or %r' % (
                         got, want_c, want_pure))

    def test_fixws(self):
        cases = [
            (" \ta\r b\t\n", "ab\n", 1),
            (" \ta\r b\t\n", " a b\n", 0),
            ("", "", 1),
            ("", "", 0),
        ]
        for a, b, allws in cases:
            c = mdiff.fixws(a, allws)
            self.assertEqual(
                c, b, 'fixws(%r) want %r got %r (allws=%r)' % (a, b, c, allws))

    def test_nice_diff_for_trivial_change(self):
        self.assertEqual(self.showdiff(
            ''.join('<%s\n-\n' % i for i in range(5)),
            ''.join('>%s\n-\n' % i for i in range(5))),
                         [diffreplace(0, 3, '<0\n', '>0\n'),
                          '-\n',
                          diffreplace(5, 8, '<1\n', '>1\n'),
                          '-\n',
                          diffreplace(10, 13, '<2\n', '>2\n'),
                          '-\n',
                          diffreplace(15, 18, '<3\n', '>3\n'),
                          '-\n',
                          diffreplace(20, 23, '<4\n', '>4\n'),
                          '-\n'])

    def test_prefer_appending(self):
        # 1 line to 3 lines
        self.assertEqual(self.showdiff('a\n', 'a\n' * 3),
                         ['a\n', diffreplace(2, 2, '', 'a\na\n')])
        # 1 line to 5 lines
        self.assertEqual(self.showdiff('a\n', 'a\n' * 5),
                         ['a\n', diffreplace(2, 2, '', 'a\na\na\na\n')])

    def test_prefer_removing_trailing(self):
        # 3 lines to 1 line
        self.assertEqual(self.showdiff('a\n' * 3, 'a\n'),
                         ['a\n', diffreplace(2, 6, 'a\na\n', '')])
        # 5 lines to 1 line
        self.assertEqual(self.showdiff('a\n' * 5, 'a\n'),
                         ['a\n', diffreplace(2, 10, 'a\na\na\na\n', '')])

if __name__ == '__main__':
    import silenttestrunner
    silenttestrunner.main(__name__)
