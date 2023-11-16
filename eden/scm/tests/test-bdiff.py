from __future__ import absolute_import, print_function

import collections
import struct
import unittest

from sapling import mdiff


class diffreplace(collections.namedtuple("diffreplace", "start end from_ to")):
    def __repr__(self):
        return "diffreplace(%r, %r, %r, %r)" % self


class BdiffTests(unittest.TestCase):
    def assert_bdiff_applies(self, a, b):
        d = mdiff.textdiff(a, b)
        c = a
        if d:
            c = mdiff.patches(a, [d])
        self.assertEqual(
            c,
            b,
            (
                "bad diff+patch result from\n  %r to\n  "
                "%r: \nbdiff: %r\npatched: %r" % (a, b, d, c[:200])
            ),
        )

    def assert_bdiff(self, a, b):
        self.assert_bdiff_applies(a, b)
        self.assert_bdiff_applies(b, a)

    def test_bdiff_basic(self):
        cases = [
            (b"a\nc\n\n\n\n", b"a\nb\n\n\n"),
            (b"a\nb\nc\n", b"a\nc\n"),
            (b"", b""),
            (b"a\nb\nc", b"a\nb\nc"),
            (b"a\nb\nc\nd\n", b"a\nd\n"),
            (b"a\nb\nc\nd\n", b"a\nc\ne\n"),
            (b"a\nb\nc\n", b"a\nc\n"),
            (b"a\n", b"c\na\nb\n"),
            (b"a\n", b""),
            (b"a\n", b"b\nc\n"),
            (b"a\n", b"c\na\n"),
            (b"", b"adjfkjdjksdhfksj"),
            (b"", b"ab"),
            (b"", b"abc"),
            (b"a", b"a"),
            (b"ab", b"ab"),
            (b"abc", b"abc"),
            (b"a\n", b"a\n"),
            (b"a\nb", b"a\nb"),
        ]
        for a, b in cases:
            self.assert_bdiff(a, b)

    def showdiff(self, a, b):
        bin = mdiff.textdiff(a, b)
        pos = 0
        q = 0
        actions = []
        while pos < len(bin):
            p1, p2, l = struct.unpack(">lll", bin[pos : pos + 12])
            pos += 12
            if p1:
                actions.append(a[q:p1])
            actions.append(diffreplace(p1, p2, a[p1:p2], bin[pos : pos + l]))
            pos += l
            q = p2
        if q < len(a):
            actions.append(a[q:])
        return actions

    def test_issue1295(self):
        cases = [
            (
                b"x\n\nx\n\nx\n\nx\n\nz\n",
                b"x\n\nx\n\ny\n\nx\n\nx\n\nz\n",
                [b"x\n\nx\n\n", diffreplace(6, 6, b"", b"y\n\n"), b"x\n\nx\n\nz\n"],
            ),
            (
                b"x\n\nx\n\nx\n\nx\n\nz\n",
                b"x\n\nx\n\ny\n\nx\n\ny\n\nx\n\nz\n",
                [
                    b"x\n\nx\n\n",
                    diffreplace(6, 6, b"", b"y\n\n"),
                    b"x\n\n",
                    diffreplace(9, 9, b"", b"y\n\n"),
                    b"x\n\nz\n",
                ],
            ),
        ]
        for old, new, want in cases:
            self.assertEqual(self.showdiff(old, new), want)

    def test_issue1295_varies_on_pure(self):
        # we should pick up abbbc. rather than bc.de as the longest match
        got = self.showdiff(
            b"a\nb\nb\nb\nc\n.\nd\ne\n.\nf\n",
            b"a\nb\nb\na\nb\nb\nb\nc\n.\nb\nc\n.\nd\ne\nf\n",
        )
        want_c = [
            b"a\nb\nb\n",
            diffreplace(6, 6, b"", b"a\nb\nb\nb\nc\n.\n"),
            b"b\nc\n.\nd\ne\n",
            diffreplace(16, 18, b".\n", b""),
            b"f\n",
        ]
        want_pure = [
            diffreplace(0, 0, b"", b"a\nb\nb\n"),
            b"a\nb\nb\nb\nc\n.\n",
            diffreplace(12, 12, b"", b"b\nc\n.\n"),
            b"d\ne\n",
            diffreplace(16, 18, b".\n", b""),
            b"f\n",
        ]
        self.assertTrue(
            got in (want_c, want_pure),
            "got: %r, wanted either %r or %r" % (got, want_c, want_pure),
        )

    def test_fixws(self):
        cases = [
            (b" \ta\r b\t\n", b"ab\n", 1),
            (b" \ta\r b\t\n", b" a b\n", 0),
            (b"", b"", 1),
            (b"", b"", 0),
        ]
        for a, b, allws in cases:
            c = mdiff.fixws(a, allws)
            self.assertEqual(
                c, b, "fixws(%r) want %r got %r (allws=%r)" % (a, b, c, allws)
            )

    def test_nice_diff_for_trivial_change(self):
        self.assertEqual(
            self.showdiff(
                b"".join(b"<%d\n-\n" % i for i in range(5)),
                b"".join(b">%d\n-\n" % i for i in range(5)),
            ),
            [
                diffreplace(0, 3, b"<0\n", b">0\n"),
                b"-\n",
                diffreplace(5, 8, b"<1\n", b">1\n"),
                b"-\n",
                diffreplace(10, 13, b"<2\n", b">2\n"),
                b"-\n",
                diffreplace(15, 18, b"<3\n", b">3\n"),
                b"-\n",
                diffreplace(20, 23, b"<4\n", b">4\n"),
                b"-\n",
            ],
        )

    def test_prefer_appending(self):
        # 1 line to 3 lines
        self.assertEqual(
            self.showdiff(b"a\n", b"a\n" * 3),
            [b"a\n", diffreplace(2, 2, b"", b"a\na\n")],
        )
        # 1 line to 5 lines
        self.assertEqual(
            self.showdiff(b"a\n", b"a\n" * 5),
            [b"a\n", diffreplace(2, 2, b"", b"a\na\na\na\n")],
        )

    def test_prefer_removing_trailing(self):
        # 3 lines to 1 line
        self.assertEqual(
            self.showdiff(b"a\n" * 3, b"a\n"),
            [b"a\n", diffreplace(2, 6, b"a\na\n", b"")],
        )
        # 5 lines to 1 line
        self.assertEqual(
            self.showdiff(b"a\n" * 5, b"a\n"),
            [b"a\n", diffreplace(2, 10, b"a\na\na\na\n", b"")],
        )


if __name__ == "__main__":
    import silenttestrunner

    silenttestrunner.main(__name__)
