# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
# fmt: off

from __future__ import absolute_import

import os
import sys

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
        assert lineno == 34
        assert indent == 12
        assert spans == [((34, 16), (34, 17)), ((34, 19), (34, 24))]

    if True:
        if True:
            foo(1, "abc")

    def nested(x, y):
        def inner1(x, y):
            inner2(x, y)

        def inner2(x, y):
            filepath, lineno, indent, spans = argspans.argspans(nested=2)
            assert lineno == 48
            assert indent == 4
            assert spans == [((48, 11), (48, 13)), ((48, 15), (48, 20))]

        inner1(x, y)

    nested(42, "def")


def testoperator():
    class A(object):
        def __eq__(self, rhs):
            filepath, lineno, indent, spans = argspans.argspans()
            assert indent == 4
            assert spans == [((58, 11), (60, 7))]

    A() == """multi
    line
    """


testfunc()
testoperator()

# fmt: on
