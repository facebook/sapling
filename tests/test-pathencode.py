# This is a randomized test that generates different pathnames every
# time it is invoked, and tests the encoding of those pathnames.
#
# It uses a simple probabilistic model to generate valid pathnames
# that have proven likely to expose bugs and divergent behavior in
# different encoding implementations.

from __future__ import absolute_import, print_function

import binascii
import collections
import itertools
import math
import os
import random
import sys
import time
from mercurial import (
    store,
)

try:
    xrange
except NameError:
    xrange = range

validchars = set(map(chr, range(0, 256)))
alphanum = range(ord('A'), ord('Z'))

for c in '\0/':
    validchars.remove(c)

winreserved = ('aux con prn nul'.split() +
               ['com%d' % i for i in xrange(1, 10)] +
               ['lpt%d' % i for i in xrange(1, 10)])

def casecombinations(names):
    '''Build all case-diddled combinations of names.'''

    combos = set()

    for r in names:
        for i in xrange(len(r) + 1):
            for c in itertools.combinations(xrange(len(r)), i):
                d = r
                for j in c:
                    d = ''.join((d[:j], d[j].upper(), d[j + 1:]))
                combos.add(d)
    return sorted(combos)

def buildprobtable(fp, cmd='hg manifest tip'):
    '''Construct and print a table of probabilities for path name
    components.  The numbers are percentages.'''

    counts = collections.defaultdict(lambda: 0)
    for line in os.popen(cmd).read().splitlines():
        if line[-2:] in ('.i', '.d'):
            line = line[:-2]
        if line.startswith('data/'):
            line = line[5:]
        for c in line:
            counts[c] += 1
    for c in '\r/\n':
        counts.pop(c, None)
    t = sum(counts.itervalues()) / 100.0
    fp.write('probtable = (')
    for i, (k, v) in enumerate(sorted(counts.iteritems(), key=lambda x: x[1],
                                      reverse=True)):
        if (i % 5) == 0:
            fp.write('\n    ')
        vt = v / t
        if vt < 0.0005:
            break
        fp.write('(%r, %.03f), ' % (k, vt))
    fp.write('\n    )\n')

# A table of character frequencies (as percentages), gleaned by
# looking at filelog names from a real-world, very large repo.

probtable = (
    ('t', 9.828), ('e', 9.042), ('s', 8.011), ('a', 6.801), ('i', 6.618),
    ('g', 5.053), ('r', 5.030), ('o', 4.887), ('p', 4.363), ('n', 4.258),
    ('l', 3.830), ('h', 3.693), ('_', 3.659), ('.', 3.377), ('m', 3.194),
    ('u', 2.364), ('d', 2.296), ('c', 2.163), ('b', 1.739), ('f', 1.625),
    ('6', 0.666), ('j', 0.610), ('y', 0.554), ('x', 0.487), ('w', 0.477),
    ('k', 0.476), ('v', 0.473), ('3', 0.336), ('1', 0.335), ('2', 0.326),
    ('4', 0.310), ('5', 0.305), ('9', 0.302), ('8', 0.300), ('7', 0.299),
    ('q', 0.298), ('0', 0.250), ('z', 0.223), ('-', 0.118), ('C', 0.095),
    ('T', 0.087), ('F', 0.085), ('B', 0.077), ('S', 0.076), ('P', 0.076),
    ('L', 0.059), ('A', 0.058), ('N', 0.051), ('D', 0.049), ('M', 0.046),
    ('E', 0.039), ('I', 0.035), ('R', 0.035), ('G', 0.028), ('U', 0.026),
    ('W', 0.025), ('O', 0.017), ('V', 0.015), ('H', 0.013), ('Q', 0.011),
    ('J', 0.007), ('K', 0.005), ('+', 0.004), ('X', 0.003), ('Y', 0.001),
    )

for c, _ in probtable:
    validchars.remove(c)
validchars = list(validchars)

def pickfrom(rng, table):
    c = 0
    r = rng.random() * sum(i[1] for i in table)
    for i, p in table:
        c += p
        if c >= r:
            return i

reservedcombos = casecombinations(winreserved)

# The first component of a name following a slash.

firsttable = (
    (lambda rng: pickfrom(rng, probtable), 90),
    (lambda rng: rng.choice(validchars), 5),
    (lambda rng: rng.choice(reservedcombos), 5),
    )

# Components of a name following the first.

resttable = firsttable[:-1]

# Special suffixes.

internalsuffixcombos = casecombinations('.hg .i .d'.split())

# The last component of a path, before a slash or at the end of a name.

lasttable = resttable + (
    (lambda rng: '', 95),
    (lambda rng: rng.choice(internalsuffixcombos), 5),
    )

def makepart(rng, k):
    '''Construct a part of a pathname, without slashes.'''

    p = pickfrom(rng, firsttable)(rng)
    l = len(p)
    ps = [p]
    maxl = rng.randint(1, k)
    while l < maxl:
        p = pickfrom(rng, resttable)(rng)
        l += len(p)
        ps.append(p)
    ps.append(pickfrom(rng, lasttable)(rng))
    return ''.join(ps)

def makepath(rng, j, k):
    '''Construct a complete pathname.'''

    return ('data/' + '/'.join(makepart(rng, k) for _ in xrange(j)) +
            rng.choice(['.d', '.i']))

def genpath(rng, count):
    '''Generate random pathnames with gradually increasing lengths.'''

    mink, maxk = 1, 4096
    def steps():
        for i in xrange(count):
            yield mink + int(round(math.sqrt((maxk - mink) * float(i) / count)))
    for k in steps():
        x = rng.randint(1, k)
        y = rng.randint(1, k)
        yield makepath(rng, x, y)

def runtests(rng, seed, count):
    nerrs = 0
    for p in genpath(rng, count):
        h = store._pathencode(p)    # uses C implementation, if available
        r = store._hybridencode(p, True) # reference implementation in Python
        if h != r:
            if nerrs == 0:
                print('seed:', hex(seed)[:-1], file=sys.stderr)
            print("\np: '%s'" % p.encode("string_escape"), file=sys.stderr)
            print("h: '%s'" % h.encode("string_escape"), file=sys.stderr)
            print("r: '%s'" % r.encode("string_escape"), file=sys.stderr)
            nerrs += 1
    return nerrs

def main():
    import getopt

    # Empirically observed to take about a second to run
    count = 100
    seed = None
    opts, args = getopt.getopt(sys.argv[1:], 'c:s:',
                               ['build', 'count=', 'seed='])
    for o, a in opts:
        if o in ('-c', '--count'):
            count = int(a)
        elif o in ('-s', '--seed'):
            seed = int(a, base=0) # accepts base 10 or 16 strings
        elif o == '--build':
            buildprobtable(sys.stdout,
                           'find .hg/store/data -type f && '
                           'cat .hg/store/fncache 2>/dev/null')
            sys.exit(0)

    if seed is None:
        try:
            seed = int(binascii.hexlify(os.urandom(16)), 16)
        except AttributeError:
            seed = int(time.time() * 1000)

    rng = random.Random(seed)
    if runtests(rng, seed, count):
        sys.exit(1)

if __name__ == '__main__':
    main()
