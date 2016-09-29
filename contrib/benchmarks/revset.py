# revset.py - asv revset benchmarks
#
# Copyright 2016 Logilab SA <contact@logilab.fr>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''ASV revset benchmarks generated from contrib/base-revsets.txt

Each revset benchmark is parameterized with variants (first, last, sort, ...)
'''

from __future__ import absolute_import

import os
import string
import sys

from . import basedir, perfbench

def createrevsetbenchmark(baseset, variants=None):
    if variants is None:
        # Default variants
        variants = ["plain", "first", "last", "sort", "sort+first",
                    "sort+last"]
    fname = "track_" + "_".join("".join([
        c if c in string.digits + string.letters else " "
        for c in baseset
    ]).split())

    def wrap(fname, baseset):
        @perfbench(name=baseset, params=[("variant", variants)])
        def f(perf, variant):
            revset = baseset
            if variant != "plain":
                for var in variant.split("+"):
                    revset = "%s(%s)" % (var, revset)
            return perf("perfrevset", revset)
        f.__name__ = fname
        return f
    return wrap(fname, baseset)

def initializerevsetbenchmarks():
    mod = sys.modules[__name__]
    with open(os.path.join(basedir, 'contrib', 'base-revsets.txt'),
              'rb') as fh:
        for line in fh:
            baseset = line.strip()
            if baseset and not baseset.startswith('#'):
                func = createrevsetbenchmark(baseset)
                setattr(mod, func.__name__, func)

initializerevsetbenchmarks()
