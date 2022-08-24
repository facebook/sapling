# Copyright (C) 2004, 2005 Canonical Ltd
#
# This program is free software; you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation; either version 2 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program; if not, see <http://www.gnu.org/licenses/>.

from __future__ import absolute_import

import unittest

from edenscm import error, simplemerge, util
from edenscm.pycompat import decodeutf8, encodeutf8
from hghave import require


TestCase = unittest.TestCase
# bzr compatible interface, for the tests
class Merge3(simplemerge.Merge3Text):
    """3-way merge of texts.

    Given BASE, OTHER, THIS, tries to produce a combined text
    incorporating the changes from both BASE->OTHER and BASE->THIS.
    All three will typically be sequences of lines."""

    def __init__(self, base, a, b):
        basetext = b"\n".join([i.strip(b"\n") for i in base] + [b""])
        atext = b"\n".join([i.strip(b"\n") for i in a] + [b""])
        btext = b"\n".join([i.strip(b"\n") for i in b] + [b""])
        if util.binary(basetext) or util.binary(atext) or util.binary(btext):
            raise error.Abort("don't know how to merge binary files")
        simplemerge.Merge3Text.__init__(self, basetext, atext, btext)
        self.base = base
        self.a = a
        self.b = b


CantReprocessAndShowBase = simplemerge.CantReprocessAndShowBase


def split_lines(t):
    return util.stringio(t).readlines()


############################################################
# test case data from the gnu diffutils manual
# common base
TZU = split_lines(
    b"""     The Nameless is the origin of Heaven and Earth;
     The named is the mother of all things.

     Therefore let there always be non-being,
       so we may see their subtlety,
     And let there always be being,
       so we may see their outcome.
     The two are the same,
     But after they are produced,
       they have different names.
     They both may be called deep and profound.
     Deeper and more profound,
     The door of all subtleties!
"""
)

LAO = split_lines(
    b"""     The Way that can be told of is not the eternal Way;
     The name that can be named is not the eternal name.
     The Nameless is the origin of Heaven and Earth;
     The Named is the mother of all things.
     Therefore let there always be non-being,
       so we may see their subtlety,
     And let there always be being,
       so we may see their outcome.
     The two are the same,
     But after they are produced,
       they have different names.
"""
)


TAO = split_lines(
    b"""     The Way that can be told of is not the eternal Way;
     The name that can be named is not the eternal name.
     The Nameless is the origin of Heaven and Earth;
     The named is the mother of all things.

     Therefore let there always be non-being,
       so we may see their subtlety,
     And let there always be being,
       so we may see their result.
     The two are the same,
     But after they are produced,
       they have different names.

       -- The Way of Lao-Tzu, tr. Wing-tsit Chan

"""
)

MERGED_RESULT = split_lines(
    b"""\
     The Way that can be told of is not the eternal Way;
     The name that can be named is not the eternal name.
     The Nameless is the origin of Heaven and Earth;
     The Named is the mother of all things.
     Therefore let there always be non-being,
       so we may see their subtlety,
     And let there always be being,
       so we may see their result.
     The two are the same,
     But after they are produced,
       they have different names.
"""
    b"""<<<<<<< LAO
=======

       -- The Way of Lao-Tzu, tr. Wing-tsit Chan

"""
    b""">>>>>>> TAO
"""
)


class TestMerge3(TestCase):
    def log(self, msg):
        pass

    def test_no_changes(self):
        """No conflicts because nothing changed"""
        m3 = Merge3([b"aaa", b"bbb"], [b"aaa", b"bbb"], [b"aaa", b"bbb"])

        self.assertEqual(m3.find_unconflicted(), [(0, 2)])

        self.assertEqual(
            list(m3.find_sync_regions()), [(0, 2, 0, 2, 0, 2), (2, 2, 2, 2, 2, 2)]
        )

        self.assertEqual(list(m3.merge_regions()), [("unchanged", 0, 2)])

        self.assertEqual(list(m3.merge_groups()), [("unchanged", [b"aaa", b"bbb"])])

    def test_front_insert(self):
        m3 = Merge3([b"zz"], [b"aaa", b"bbb", b"zz"], [b"zz"])

        # todo: should use a sentinel at end as from get_matching_blocks
        # to match without zz
        self.assertEqual(
            list(m3.find_sync_regions()), [(0, 1, 2, 3, 0, 1), (1, 1, 3, 3, 1, 1)]
        )

        self.assertEqual(list(m3.merge_regions()), [("a", 0, 2), ("unchanged", 0, 1)])

        self.assertEqual(
            list(m3.merge_groups()), [("a", [b"aaa", b"bbb"]), ("unchanged", [b"zz"])]
        )

    def test_null_insert(self):
        m3 = Merge3([], [b"aaa", b"bbb"], [])
        # todo: should use a sentinel at end as from get_matching_blocks
        # to match without zz
        self.assertEqual(list(m3.find_sync_regions()), [(0, 0, 2, 2, 0, 0)])

        self.assertEqual(list(m3.merge_regions()), [("a", 0, 2)])

        self.assertEqual(list(m3.merge_lines()), [b"aaa", b"bbb"])

    def test_no_conflicts(self):
        """No conflicts because only one side changed"""
        m3 = Merge3([b"aaa", b"bbb"], [b"aaa", b"111", b"bbb"], [b"aaa", b"bbb"])

        self.assertEqual(m3.find_unconflicted(), [(0, 1), (1, 2)])

        self.assertEqual(
            list(m3.find_sync_regions()),
            [(0, 1, 0, 1, 0, 1), (1, 2, 2, 3, 1, 2), (2, 2, 3, 3, 2, 2)],
        )

        self.assertEqual(
            list(m3.merge_regions()),
            [("unchanged", 0, 1), ("a", 1, 2), ("unchanged", 1, 2)],
        )

    def test_append_a(self):
        m3 = Merge3(
            [b"aaa\n", b"bbb\n"], [b"aaa\n", b"bbb\n", b"222\n"], [b"aaa\n", b"bbb\n"]
        )

        self.assertEqual(b"".join(m3.merge_lines()), b"aaa\nbbb\n222\n")

    def test_append_b(self):
        m3 = Merge3(
            [b"aaa\n", b"bbb\n"], [b"aaa\n", b"bbb\n"], [b"aaa\n", b"bbb\n", b"222\n"]
        )

        self.assertEqual(b"".join(m3.merge_lines()), b"aaa\nbbb\n222\n")

    def test_append_agreement(self):
        m3 = Merge3(
            [b"aaa\n", b"bbb\n"],
            [b"aaa\n", b"bbb\n", b"222\n"],
            [b"aaa\n", b"bbb\n", b"222\n"],
        )

        self.assertEqual(b"".join(m3.merge_lines()), b"aaa\nbbb\n222\n")

    def test_append_clash(self):
        m3 = Merge3(
            [b"aaa\n", b"bbb\n"],
            [b"aaa\n", b"bbb\n", b"222\n"],
            [b"aaa\n", b"bbb\n", b"333\n"],
        )

        ml = m3.merge_lines(
            name_a=b"a",
            name_b=b"b",
            start_marker=b"<<",
            mid_marker=b"--",
            end_marker=b">>",
        )
        self.assertEqual(
            b"".join(ml),
            b"aaa\n" b"bbb\n" b"<< a\n" b"222\n" b"--\n" b"333\n" b">> b\n",
        )

    def test_insert_agreement(self):
        m3 = Merge3(
            [b"aaa\n", b"bbb\n"],
            [b"aaa\n", b"222\n", b"bbb\n"],
            [b"aaa\n", b"222\n", b"bbb\n"],
        )

        ml = m3.merge_lines(
            name_a=b"a",
            name_b=b"b",
            start_marker=b"<<",
            mid_marker=b"--",
            end_marker=b">>",
        )
        self.assertEqual(b"".join(ml), b"aaa\n222\nbbb\n")

    def test_insert_clash(self):
        """Both try to insert lines in the same place."""
        m3 = Merge3(
            [b"aaa\n", b"bbb\n"],
            [b"aaa\n", b"111\n", b"bbb\n"],
            [b"aaa\n", b"222\n", b"bbb\n"],
        )

        self.assertEqual(m3.find_unconflicted(), [(0, 1), (1, 2)])

        self.assertEqual(
            list(m3.find_sync_regions()),
            [(0, 1, 0, 1, 0, 1), (1, 2, 2, 3, 2, 3), (2, 2, 3, 3, 3, 3)],
        )

        self.assertEqual(
            list(m3.merge_regions()),
            [("unchanged", 0, 1), ("conflict", 1, 1, 1, 2, 1, 2), ("unchanged", 1, 2)],
        )

        self.assertEqual(
            list(m3.merge_groups()),
            [
                ("unchanged", [b"aaa\n"]),
                ("conflict", [], [b"111\n"], [b"222\n"]),
                ("unchanged", [b"bbb\n"]),
            ],
        )

        ml = m3.merge_lines(
            name_a=b"a",
            name_b=b"b",
            start_marker=b"<<",
            mid_marker=b"--",
            end_marker=b">>",
        )
        self.assertEqual(
            b"".join(ml),
            b"""aaa
<< a
111
--
222
>> b
bbb
""",
        )

    def test_replace_clash(self):
        """Both try to insert lines in the same place."""
        m3 = Merge3(
            [b"aaa", b"000", b"bbb"], [b"aaa", b"111", b"bbb"], [b"aaa", b"222", b"bbb"]
        )

        self.assertEqual(m3.find_unconflicted(), [(0, 1), (2, 3)])

        self.assertEqual(
            list(m3.find_sync_regions()),
            [(0, 1, 0, 1, 0, 1), (2, 3, 2, 3, 2, 3), (3, 3, 3, 3, 3, 3)],
        )

    def test_replace_multi(self):
        """Replacement with regions of different size."""
        m3 = Merge3(
            [b"aaa", b"000", b"000", b"bbb"],
            [b"aaa", b"111", b"111", b"111", b"bbb"],
            [b"aaa", b"222", b"222", b"222", b"222", b"bbb"],
        )

        self.assertEqual(m3.find_unconflicted(), [(0, 1), (3, 4)])

        self.assertEqual(
            list(m3.find_sync_regions()),
            [(0, 1, 0, 1, 0, 1), (3, 4, 4, 5, 5, 6), (4, 4, 5, 5, 6, 6)],
        )

    def test_merge_poem(self):
        """Test case from diff3 manual"""
        m3 = Merge3(TZU, LAO, TAO)
        ml = list(m3.merge_lines(b"LAO", b"TAO"))
        self.log("merge result:")
        self.log(decodeutf8(b"".join(ml)))
        self.assertEqual(ml, MERGED_RESULT)

    def test_binary(self):
        with self.assertRaises(error.Abort):
            Merge3([b"\x00"], [b"a"], [b"b"])

    def test_dos_text(self):
        base_text = b"a\r\n"
        this_text = b"b\r\n"
        other_text = b"c\r\n"
        m3 = Merge3(
            base_text.splitlines(True),
            other_text.splitlines(True),
            this_text.splitlines(True),
        )
        m_lines = m3.merge_lines(b"OTHER", b"THIS")
        self.assertEqual(
            b"<<<<<<< OTHER\r\nc\r\n=======\r\nb\r\n"
            b">>>>>>> THIS\r\n".splitlines(True),
            list(m_lines),
        )

    def test_mac_text(self):
        base_text = b"a\r"
        this_text = b"b\r"
        other_text = b"c\r"
        m3 = Merge3(
            base_text.splitlines(True),
            other_text.splitlines(True),
            this_text.splitlines(True),
        )
        m_lines = m3.merge_lines(b"OTHER", b"THIS")
        self.assertEqual(
            b"<<<<<<< OTHER\rc\r=======\rb\r" b">>>>>>> THIS\r".splitlines(True),
            list(m_lines),
        )


if __name__ == "__main__":
    # hide the timer
    import time

    orig = time.time
    try:
        time.time = lambda: 0
        time.perf_counter = lambda: 0
        unittest.main()
    finally:
        time.time = orig
