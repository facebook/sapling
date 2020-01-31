# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
# fmt: off

from __future__ import absolute_import

import os
import sys

from hghave import require
from testutil import argspans


require(["py2"])


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
        assert lineno == 38
        assert indent == 12
        assert spans == [((38, 16), (38, 17)), ((38, 19), (38, 24))]

    if True:
        if True:
            foo(1, "abc")

    def nested(x, y):
        def inner1(x, y):
            inner2(x, y)

        def inner2(x, y):
            filepath, lineno, indent, spans = argspans.argspans(nested=2)
            assert lineno == 52
            assert indent == 4
            assert spans == [((52, 11), (52, 13)), ((52, 15), (52, 20))]

        inner1(x, y)

    nested(42, "def")


def testoperator():
    class A(object):
        def __eq__(self, rhs):
            filepath, lineno, indent, spans = argspans.argspans()
            assert indent == 4
            assert spans == [((62, 11), (64, 7))]

    A() == """multi
    line
    """


testfunc()
testoperator()

# fmt: on
