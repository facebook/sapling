from __future__ import absolute_import

import unittest
from mercurial import error, mdiff

# for readability, line numbers are 0-origin
text1 = '''
           00 at OLD
           01 at OLD
           02 at OLD
02 at NEW, 03 at OLD
03 at NEW, 04 at OLD
04 at NEW, 05 at OLD
05 at NEW, 06 at OLD
           07 at OLD
           08 at OLD
           09 at OLD
           10 at OLD
           11 at OLD
'''[1:] # strip initial LF

text2 = '''
00 at NEW
01 at NEW
02 at NEW, 03 at OLD
03 at NEW, 04 at OLD
04 at NEW, 05 at OLD
05 at NEW, 06 at OLD
06 at NEW
07 at NEW
08 at NEW
09 at NEW
10 at NEW
11 at NEW
'''[1:] # strip initial LF

def filteredblocks(blocks, rangeb):
    """return `rangea` extracted from `blocks` coming from
    `mdiff.blocksinrange` along with the mask of blocks within rangeb.
    """
    filtered, rangea = mdiff.blocksinrange(blocks, rangeb)
    skipped = [b not in filtered for b in blocks]
    return rangea, skipped

class blocksinrangetests(unittest.TestCase):

    def setUp(self):
        self.blocks = list(mdiff.allblocks(text1, text2))
        assert self.blocks == [
            ([0, 3, 0, 2], '!'),
            ((3, 7, 2, 6), '='),
            ([7, 12, 6, 12], '!'),
            ((12, 12, 12, 12), '='),
        ], self.blocks

    def testWithinEqual(self):
        """linerange within an "=" block"""
        # IDX 0         1
        #     012345678901
        # SRC NNOOOONNNNNN (New/Old)
        #        ^^
        linerange2 = (3, 5)
        linerange1, skipped = filteredblocks(self.blocks, linerange2)
        self.assertEqual(linerange1, (4, 6))
        self.assertEqual(skipped, [True, False, True, True])

    def testWithinEqualStrictly(self):
        """linerange matching exactly an "=" block"""
        # IDX 0         1
        #     012345678901
        # SRC NNOOOONNNNNN (New/Old)
        #       ^^^^
        linerange2 = (2, 6)
        linerange1, skipped = filteredblocks(self.blocks, linerange2)
        self.assertEqual(linerange1, (3, 7))
        self.assertEqual(skipped, [True, False, True, True])

    def testWithinEqualLowerbound(self):
        """linerange at beginning of an "=" block"""
        # IDX 0         1
        #     012345678901
        # SRC NNOOOONNNNNN (New/Old)
        #       ^^
        linerange2 = (2, 4)
        linerange1, skipped = filteredblocks(self.blocks, linerange2)
        self.assertEqual(linerange1, (3, 5))
        self.assertEqual(skipped, [True, False, True, True])

    def testWithinEqualLowerboundOneline(self):
        """oneline-linerange at beginning of an "=" block"""
        # IDX 0         1
        #     012345678901
        # SRC NNOOOONNNNNN (New/Old)
        #       ^
        linerange2 = (2, 3)
        linerange1, skipped = filteredblocks(self.blocks, linerange2)
        self.assertEqual(linerange1, (3, 4))
        self.assertEqual(skipped, [True, False, True, True])

    def testWithinEqualUpperbound(self):
        """linerange at end of an "=" block"""
        # IDX 0         1
        #     012345678901
        # SRC NNOOOONNNNNN (New/Old)
        #        ^^^
        linerange2 = (3, 6)
        linerange1, skipped = filteredblocks(self.blocks, linerange2)
        self.assertEqual(linerange1, (4, 7))
        self.assertEqual(skipped, [True, False, True, True])

    def testWithinEqualUpperboundOneLine(self):
        """oneline-linerange at end of an "=" block"""
        # IDX 0         1
        #     012345678901
        # SRC NNOOOONNNNNN (New/Old)
        #          ^
        linerange2 = (5, 6)
        linerange1, skipped = filteredblocks(self.blocks, linerange2)
        self.assertEqual(linerange1, (6, 7))
        self.assertEqual(skipped, [True, False, True, True])

    def testWithinFirstBlockNeq(self):
        """linerange within the first "!" block"""
        # IDX 0         1
        #     012345678901
        # SRC NNOOOONNNNNN (New/Old)
        #     ^
        #      |           (empty)
        #      ^
        #     ^^
        for linerange2 in [
            (0, 1),
            (1, 1),
            (1, 2),
            (0, 2),
        ]:
            linerange1, skipped = filteredblocks(self.blocks, linerange2)
            self.assertEqual(linerange1, (0, 3))
            self.assertEqual(skipped, [False, True, True, True])

    def testWithinLastBlockNeq(self):
        """linerange within the last "!" block"""
        # IDX 0         1
        #     012345678901
        # SRC NNOOOONNNNNN (New/Old)
        #           ^
        #            ^
        #           |      (empty)
        #           ^^^^^^
        #                ^
        for linerange2 in [
            (6, 7),
            (7, 8),
            (7, 7),
            (6, 12),
            (11, 12),
        ]:
            linerange1, skipped = filteredblocks(self.blocks, linerange2)
            self.assertEqual(linerange1, (7, 12))
            self.assertEqual(skipped, [True, True, False, True])

    def testAccrossTwoBlocks(self):
        """linerange accross two blocks"""
        # IDX 0         1
        #     012345678901
        # SRC NNOOOONNNNNN (New/Old)
        #      ^^^^
        linerange2 = (1, 5)
        linerange1, skipped = filteredblocks(self.blocks, linerange2)
        self.assertEqual(linerange1, (0, 6))
        self.assertEqual(skipped, [False, False, True, True])

    def testCrossingSeveralBlocks(self):
        """linerange accross three blocks"""
        # IDX 0         1
        #     012345678901
        # SRC NNOOOONNNNNN (New/Old)
        #      ^^^^^^^
        linerange2 = (1, 8)
        linerange1, skipped = filteredblocks(self.blocks, linerange2)
        self.assertEqual(linerange1, (0, 12))
        self.assertEqual(skipped, [False, False, False, True])

    def testStartInEqBlock(self):
        """linerange starting in an "=" block"""
        # IDX 0         1
        #     012345678901
        # SRC NNOOOONNNNNN (New/Old)
        #          ^^^^
        #         ^^^^^^^
        for linerange2, expectedlinerange1 in [
            ((5, 9), (6, 12)),
            ((4, 11), (5, 12)),
        ]:
            linerange1, skipped = filteredblocks(self.blocks, linerange2)
            self.assertEqual(linerange1, expectedlinerange1)
            self.assertEqual(skipped, [True, False, False, True])

    def testEndInEqBlock(self):
        """linerange ending in an "=" block"""
        # IDX 0         1
        #     012345678901
        # SRC NNOOOONNNNNN (New/Old)
        #      ^^
        #     ^^^^^
        for linerange2, expectedlinerange1 in [
            ((1, 3), (0, 4)),
            ((0, 4), (0, 5)),
        ]:
            linerange1, skipped = filteredblocks(self.blocks, linerange2)
            self.assertEqual(linerange1, expectedlinerange1)
            self.assertEqual(skipped, [False, False, True, True])

    def testOutOfRange(self):
        """linerange exceeding file size"""
        exctype = error.Abort
        for linerange2 in [
            (0, 34),
            (15, 12),
        ]:
            # Could be `with self.assertRaises(error.Abort)` but python2.6
            # does not have assertRaises context manager.
            try:
                mdiff.blocksinrange(self.blocks, linerange2)
            except exctype as exc:
                self.assertTrue('line range exceeds file size' in str(exc))
            else:
                self.fail('%s not raised' % exctype.__name__)

if __name__ == '__main__':
    import silenttestrunner
    silenttestrunner.main(__name__)
