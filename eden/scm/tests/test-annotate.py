# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright Mercurial Contributors
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import, print_function

import unittest

from sapling import mdiff
from sapling.annotate import _annotatepair


try:
    xrange(0)
except NameError:
    xrange = range


class AnnotateTests(unittest.TestCase):
    """Unit tests for annotate code."""

    def testannotatepair(self):
        self.maxDiff = None  # camelcase-required

        oldfctx = "old"
        p1fctx, p2fctx, childfctx = "p1", "p2", "c"
        olddata = b"a\nb\n"
        p1data = b"a\nb\nc\n"
        p2data = b"a\nc\nd\n"
        childdata = b"a\nb2\nc\nc2\nd\n"
        diffopts = mdiff.diffopts()

        def decorate(text, rev):
            return (
                [(rev, i) for i in xrange(1, text.count(b"\n") + 1)],
                text,
            )

        # Basic usage

        oldann = decorate(olddata, oldfctx)
        p1ann = decorate(p1data, p1fctx)
        p1ann = _annotatepair([oldann], p1ann, diffopts)
        self.assertEqual(
            p1ann[0],
            [("old", 1), ("old", 2), ("p1", 3)],
        )

        p2ann = decorate(p2data, p2fctx)
        p2ann = _annotatepair([oldann], p2ann, diffopts)
        self.assertEqual(
            p2ann[0],
            [("old", 1), ("p2", 2), ("p2", 3)],
        )

        # Test with multiple parents (note the difference caused by ordering)

        childann = decorate(childdata, childfctx)
        childann = _annotatepair([p1ann, p2ann], childann, diffopts)
        self.assertEqual(
            childann[0],
            [
                ("old", 1),
                ("c", 2),
                ("p2", 2),
                ("c", 4),
                ("p2", 3),
            ],
        )

        childann = decorate(childdata, childfctx)
        childann = _annotatepair([p2ann, p1ann], childann, diffopts)
        self.assertEqual(
            childann[0],
            [
                ("old", 1),
                ("c", 2),
                ("p1", 3),
                ("c", 4),
                ("p2", 3),
            ],
        )


if __name__ == "__main__":
    import silenttestrunner

    silenttestrunner.main(__name__)
