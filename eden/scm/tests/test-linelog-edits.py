#!/usr/bin/env python
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import random
import sys

import bindings

IntLineLog = bindings.linelog.IntLineLog


randint = random.randint

vecratio = 3  # number of edit_chunk
maxb1 = 0xFFFFFF
maxdeltaa = 10  # max(a2 - b1)
maxdeltab = 10  # max(b2 - b1)


def generator(seed=None, endrev=None):  # generate test cases
    lines = []
    random.seed(seed)
    rev = 0
    while rev != endrev:
        rev += 1
        n = len(lines)
        a1 = randint(0, n)
        a2 = randint(a1, min(n, a1 + maxdeltaa))
        b1 = randint(0, maxb1)
        b2 = randint(b1, b1 + maxdeltab)
        blines = [(rev, bidx) for bidx in range(b1, b2)]
        lines[a1:a2] = blines
        yield lines, rev, a1, a2, b1, b2


def assert_eq(lhs, rhs):
    if lhs != rhs:
        raise RuntimeError(f"assert_eq failed: {lhs} != {rhs}")


# init
seed = random.random()
log = IntLineLog()

# how many random revisions we generate
endrev = 2000
try:
    endrev = int(sys.argv[1])
except Exception:
    pass

# populate linelog
for lines, b_rev, a1, a2, b1, b2 in generator(seed, endrev):
    a_rev = log.max_rev()
    log = log.edit_chunk(a_rev, a1, a2, b_rev, b1, b2)

# verify we can get back these states by annotating each revision
for lines, rev, a1, a2, b1, b2 in generator(seed, endrev):
    checkout_lines = [(l[0], l[1]) for l in log.checkout_lines(rev)[:-1]]
    assert_eq(lines, checkout_lines)
