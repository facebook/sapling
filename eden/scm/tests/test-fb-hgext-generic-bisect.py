from __future__ import absolute_import

import unittest

import silenttestrunner


class BisectTests(unittest.TestCase):
    def testSimple(self):
        self._assertBisect([1], 1, 0)
        self._assertBisect([1, 2], 1, 0)
        self._assertBisect([1, 2], 2, 1)

    def testEmptyArray(self):
        self._assertBisect([], 1, None)

    def testTwoEqual(self):
        self._assertBisect([], 1, None)
        self._assertBisect([1, 1], 1, 0)
        self._assertBisect([1, 1, 1], 1, 0)

    def testAllBigger(self):
        self._assertBisect([2], 1, None)
        self._assertBisect([2, 3], 1, None)
        self._assertBisect([2, 3, 4], 1, None)

    def testBig(self):
        array = range(0, 10)
        for i in array:
            self._assertBisect(array, i, i)
        array = range(0, 11)
        for i in array:
            self._assertBisect(array, i, i)

    def testNotFound(self):
        array = range(0, 10)
        self._assertBisect(array, 10, None)
        array = range(0, 11)
        self._assertBisect(array, 11, None)
        array = range(0, 10, 2)
        self._assertBisect(array, 1, None)

    def _assertBisect(self, array, value, result):
        def comp(index, value):
            if array[index] < value:
                return -1
            elif array[index] == value:
                return 0
            else:
                return 1

        self.assertEqual(bisect(0, len(array) - 1, comp, value), result)


if __name__ == "__main__":
    from edenscm.hgext.generic_bisect import bisect

    silenttestrunner.main(__name__)
