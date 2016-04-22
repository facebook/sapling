# Randomized torture test generation for bdiff

from __future__ import absolute_import, print_function
import random, sys
from mercurial import (
    bdiff,
    mpatch,
)

def reducetest(a, b):
    tries = 0
    reductions = 0
    print("reducing...")
    while tries < 1000:
        a2 = "\n".join(l for l in a.splitlines()
                       if random.randint(0, 100) > 0) + "\n"
        b2 = "\n".join(l for l in b.splitlines()
                       if random.randint(0, 100) > 0) + "\n"
        if a2 == a and b2 == b:
            continue
        if a2 == b2:
            continue
        tries += 1

        try:
            test1(a, b)
        except Exception as inst:
            reductions += 1
            tries = 0
            a = a2
            b = b2

    print("reduced:", reductions, len(a) + len(b),
          repr(a), repr(b))
    try:
        test1(a, b)
    except Exception as inst:
        print("failed:", inst)

    sys.exit(0)

def test1(a, b):
    d = bdiff.bdiff(a, b)
    if not d:
        raise ValueError("empty")
    c = mpatch.patches(a, [d])
    if c != b:
        raise ValueError("bad")

def testwrap(a, b):
    try:
        test1(a, b)
        return
    except Exception as inst:
        pass
    print("exception:", inst)
    reducetest(a, b)

def test(a, b):
    testwrap(a, b)
    testwrap(b, a)

def rndtest(size, noise):
    a = []
    src = "                aaaaaaaabbbbccd"
    for x in xrange(size):
        a.append(src[random.randint(0, len(src) - 1)])

    while True:
        b = [c for c in a if random.randint(0, 99) > noise]
        b2 = []
        for c in b:
            b2.append(c)
            while random.randint(0, 99) < noise:
                b2.append(src[random.randint(0, len(src) - 1)])
        if b2 != a:
            break

    a = "\n".join(a) + "\n"
    b = "\n".join(b2) + "\n"

    test(a, b)

maxvol = 10000
startsize = 2
while True:
    size = startsize
    count = 0
    while size < maxvol:
        print(size)
        volume = 0
        while volume < maxvol:
            rndtest(size, 2)
            volume += size
            count += 2
        size *= 2
    maxvol *= 4
    startsize *= 4
