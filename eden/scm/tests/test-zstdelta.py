from __future__ import absolute_import

import os
import unittest

import silenttestrunner
from bindings import zstd


class testzstd(unittest.TestCase):
    def testdelta(self):
        base = os.urandom(100000)
        data = base[:1000] + "x" + base[1000:90000] + base[90500:]
        delta = zstd.diff(base, data)

        # The delta is tiny
        self.assertLess(len(delta), 100)

        # The delta can be applied
        self.assertEqual(zstd.apply(base, delta), data)


if __name__ == "__main__":
    silenttestrunner.main(__name__)
