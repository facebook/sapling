# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import, print_function

import unittest

import silenttestrunner

from sapling import util


class testlrucachefunc(unittest.TestCase):
    def test_kwargs(self):
        calledcount = 0

        @util.lrucachefunc
        def foo(a, b, c=4, d=5):
            nonlocal calledcount
            calledcount += 1
            return (a, b, c, d)

        self.assertEqual(foo(1, 2, 3, 4), (1, 2, 3, 4))
        self.assertEqual(foo(1, 2, 3, 4), (1, 2, 3, 4))
        self.assertEqual(calledcount, 1)

        self.assertEqual(foo(1, 2, d=4), (1, 2, 4, 4))
        self.assertEqual(foo(1, 2, d=4), (1, 2, 4, 4))
        self.assertEqual(calledcount, 2)

        self.assertEqual(foo(1, 2, d=5, c=4), (1, 2, 4, 5))
        self.assertEqual(foo(1, 2, c=4, d=5), (1, 2, 4, 5))
        self.assertEqual(calledcount, 3)

        calledcount = 0
        for i in range(30):
            self.assertEqual(foo(1, i), (1, i, 4, 5))
        self.assertEqual(calledcount, 30)

        calledcount = 0
        for i in reversed(range(30)):
            self.assertEqual(foo(1, i), (1, i, 4, 5))
        self.assertEqual(calledcount, 9)

        @util.lrucachefunc
        def bar(*args, **kwargs):
            nonlocal calledcount
            calledcount += 1
            return (args, kwargs)

        # Make sure we differentiate args and kwargs in cache:
        calledcount = 0
        self.assertEqual(bar(1, a=2), ((1,), {"a": 2}))
        self.assertEqual(bar(1, ("a", 2)), ((1, ("a", 2)), {}))
        self.assertEqual(calledcount, 2)


if __name__ == "__main__":
    silenttestrunner.main(__name__)
