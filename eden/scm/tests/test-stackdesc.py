# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
# isort:skip_file

import os
import sys

if os.name == "nt":
    sys.exit(80)

from edenscm.mercurial.util import describe, renderstack


@describe(lambda b: "plus1 b = %s" % b)
def plus1(a, b):
    return plus2(a, b=b)


@describe(lambda b, a: "plus2 a = %s, b = %s" % (a, b))
def plus2(a, b):
    return plus3(a=a, b=b)


@describe(lambda b, a=0, c=0: "plus3 a = %s, b = %s, c = %s" % (a, b, c))
def plus3(**kwargs):
    return plus4(**kwargs)


@describe(lambda: 1 / 0)
def plus4(a, b):
    return plus5(a, b)


@describe(lambda x: "plus5 x = %s" % (x,))
def plus5(a, b):
    for line in renderstack():
        print(line)
    return a + b


print("result = %s" % plus1(3, 5))
