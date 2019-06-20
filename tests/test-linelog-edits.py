#!/usr/bin/env python
from __future__ import absolute_import

import random
import sys

from edenscmnative import linelog


randint = random.randint

vecratio = 3  # number of replacelines / number of replacelines_vec
maxlinenum = 0xFFFFFF
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
        usevec = not bool(randint(0, vecratio))
        if usevec:
            blines = [(randint(0, rev), randint(0, maxlinenum)) for _ in range(b1, b2)]
        else:
            blines = [(rev, bidx) for bidx in range(b1, b2)]
        lines[a1:a2] = blines
        yield lines, rev, a1, a2, b1, b2, blines, usevec


def ensure(condition):
    if not condition:
        raise RuntimeError("Unexpected")


# init
seed = random.random()
log = linelog.linelog()
log.clear()
log.annotate(0)

# how many random revisions we generate
endrev = 2000
try:
    endrev = int(sys.argv[1])
except Exception:
    pass

# populate linelog
for lines, rev, a1, a2, b1, b2, blines, usevec in generator(seed, endrev):
    if usevec:
        log.replacelines_vec(rev, a1, a2, blines)
    else:
        log.replacelines(rev, a1, a2, b1, b2)
    ensure(lines == log.annotateresult)

# verify we can get back these states by annotating each revision
for lines, rev, a1, a2, b1, b2, blines, usevec in generator(seed, endrev):
    log.annotate(rev)
    ensure(lines == log.annotateresult)
