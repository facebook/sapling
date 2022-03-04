# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# fmt: off
from __future__ import absolute_import

import os
import sys

from hghave import require
from testutil import argspans


try:
    import parso

    parso.parse
except ImportError:
    sys.stderr.write("skipped: missing feature: parso\n")
    sys.exit(80)


def testfunc():
    def foo(x, y):
        filepath, lineno, indent, spans = argspans.argspans()
        assert os.path.basename(filepath) == "test-argspans.py"
        assert lineno == 35
        assert indent == 12
        assert spans == [((35, 16), (35, 17)), ((35, 19), (35, 24))]

    if True:
        if True:
            foo(1, "abc")

    def nested(x, y):
        def inner1(x, y):
            inner2(x, y)

        def inner2(x, y):
            filepath, lineno, indent, spans = argspans.argspans(nested=2)
            assert lineno == 49
            assert indent == 4
            assert spans == [((49, 11), (49, 13)), ((49, 15), (49, 20))]

        inner1(x, y)

    nested(42, "def")


def testoperator():
    class A(object):
        def __eq__(self, rhs):
            filepath, lineno, indent, spans = argspans.argspans()
            assert indent == 4
            assert spans == [((59, 11), (61, 7))]

    A() == """multi
    line
    """


testfunc()
testoperator()

# fmt: on
