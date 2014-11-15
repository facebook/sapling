from mercurial import ancestor, commands, hg, ui, util
from mercurial.node import nullrev
import binascii, getopt, math, os, random, sys, time

def buildgraph(rng, nodes=100, rootprob=0.05, mergeprob=0.2, prevprob=0.7):
    '''nodes: total number of nodes in the graph
    rootprob: probability that a new node (not 0) will be a root
    mergeprob: probability that, excluding a root a node will be a merge
    prevprob: probability that p1 will be the previous node

    return value is a graph represented as an adjacency list.
    '''
    graph = [None] * nodes
    for i in xrange(nodes):
        if i == 0 or rng.random() < rootprob:
            graph[i] = [nullrev]
        elif i == 1:
            graph[i] = [0]
        elif rng.random() < mergeprob:
            if i == 2 or rng.random() < prevprob:
                # p1 is prev
                p1 = i - 1
            else:
                p1 = rng.randrange(i - 1)
            p2 = rng.choice(range(0, p1) + range(p1 + 1, i))
            graph[i] = [p1, p2]
        elif rng.random() < prevprob:
            graph[i] = [i - 1]
        else:
            graph[i] = [rng.randrange(i - 1)]

    return graph

def buildancestorsets(graph):
    ancs = [None] * len(graph)
    for i in xrange(len(graph)):
        ancs[i] = set([i])
        if graph[i] == [nullrev]:
            continue
        for p in graph[i]:
            ancs[i].update(ancs[p])
    return ancs

class naiveincrementalmissingancestors(object):
    def __init__(self, ancs, bases):
        self.ancs = ancs
        self.bases = set(bases)
    def addbases(self, newbases):
        self.bases.update(newbases)
    def removeancestorsfrom(self, revs):
        for base in self.bases:
            if base != nullrev:
                revs.difference_update(self.ancs[base])
        revs.discard(nullrev)
    def missingancestors(self, revs):
        res = set()
        for rev in revs:
            if rev != nullrev:
                res.update(self.ancs[rev])
        for base in self.bases:
            if base != nullrev:
                res.difference_update(self.ancs[base])
        return sorted(res)

def test_missingancestors(seed, rng):
    # empirically observed to take around 1 second
    graphcount = 100
    testcount = 10
    inccount = 10
    nerrs = [0]
    # the default mu and sigma give us a nice distribution of mostly
    # single-digit counts (including 0) with some higher ones
    def lognormrandom(mu, sigma):
        return int(math.floor(rng.lognormvariate(mu, sigma)))

    def samplerevs(nodes, mu=1.1, sigma=0.8):
        count = min(lognormrandom(mu, sigma), len(nodes))
        return rng.sample(nodes, count)

    def err(seed, graph, bases, seq, output, expected):
        if nerrs[0] == 0:
            print >> sys.stderr, 'seed:', hex(seed)[:-1]
        if gerrs[0] == 0:
            print >> sys.stderr, 'graph:', graph
        print >> sys.stderr, '* bases:', bases
        print >> sys.stderr, '* seq: ', seq
        print >> sys.stderr, '*  output:  ', output
        print >> sys.stderr, '*  expected:', expected
        nerrs[0] += 1
        gerrs[0] += 1

    for g in xrange(graphcount):
        graph = buildgraph(rng)
        ancs = buildancestorsets(graph)
        gerrs = [0]
        for _ in xrange(testcount):
            # start from nullrev to include it as a possibility
            graphnodes = range(nullrev, len(graph))
            bases = samplerevs(graphnodes)

            # fast algorithm
            inc = ancestor.incrementalmissingancestors(graph.__getitem__, bases)
            # reference slow algorithm
            naiveinc = naiveincrementalmissingancestors(ancs, bases)
            seq = []
            revs = []
            for _ in xrange(inccount):
                if rng.random() < 0.2:
                    newbases = samplerevs(graphnodes)
                    seq.append(('addbases', newbases))
                    inc.addbases(newbases)
                    naiveinc.addbases(newbases)
                if rng.random() < 0.4:
                    # larger set so that there are more revs to remove from
                    revs = samplerevs(graphnodes, mu=1.5)
                    seq.append(('removeancestorsfrom', revs))
                    hrevs = set(revs)
                    rrevs = set(revs)
                    inc.removeancestorsfrom(hrevs)
                    naiveinc.removeancestorsfrom(rrevs)
                    if hrevs != rrevs:
                        err(seed, graph, bases, seq, sorted(hrevs),
                            sorted(rrevs))
                else:
                    revs = samplerevs(graphnodes)
                    seq.append(('missingancestors', revs))
                    h = inc.missingancestors(revs)
                    r = naiveinc.missingancestors(revs)
                    if h != r:
                        err(seed, graph, bases, seq, h, r)

# graph is a dict of child->parent adjacency lists for this graph:
# o  13
# |
# | o  12
# | |
# | | o    11
# | | |\
# | | | | o  10
# | | | | |
# | o---+ |  9
# | | | | |
# o | | | |  8
#  / / / /
# | | o |  7
# | | | |
# o---+ |  6
#  / / /
# | | o  5
# | |/
# | o  4
# | |
# o |  3
# | |
# | o  2
# |/
# o  1
# |
# o  0

graph = {0: [-1], 1: [0], 2: [1], 3: [1], 4: [2], 5: [4], 6: [4],
         7: [4], 8: [-1], 9: [6, 7], 10: [5], 11: [3, 7], 12: [9],
         13: [8]}

def genlazyancestors(revs, stoprev=0, inclusive=False):
    print ("%% lazy ancestor set for %s, stoprev = %s, inclusive = %s" %
           (revs, stoprev, inclusive))
    return ancestor.lazyancestors(graph.get, revs, stoprev=stoprev,
                                  inclusive=inclusive)

def printlazyancestors(s, l):
    print 'membership: %r' % [n for n in l if n in s]
    print 'iteration:  %r' % list(s)

def test_lazyancestors():
    # Empty revs
    s = genlazyancestors([])
    printlazyancestors(s, [3, 0, -1])

    # Standard example
    s = genlazyancestors([11, 13])
    printlazyancestors(s, [11, 13, 7, 9, 8, 3, 6, 4, 1, -1, 0])

    # Standard with ancestry in the initial set (1 is ancestor of 3)
    s = genlazyancestors([1, 3])
    printlazyancestors(s, [1, -1, 0])

    # Including revs
    s = genlazyancestors([11, 13], inclusive=True)
    printlazyancestors(s, [11, 13, 7, 9, 8, 3, 6, 4, 1, -1, 0])

    # Test with stoprev
    s = genlazyancestors([11, 13], stoprev=6)
    printlazyancestors(s, [11, 13, 7, 9, 8, 3, 6, 4, 1, -1, 0])
    s = genlazyancestors([11, 13], stoprev=6, inclusive=True)
    printlazyancestors(s, [11, 13, 7, 9, 8, 3, 6, 4, 1, -1, 0])


# The C gca algorithm requires a real repo. These are textual descriptions of
# DAGs that have been known to be problematic.
dagtests = [
    '+2*2*2/*3/2',
    '+3*3/*2*2/*4*4/*4/2*4/2*2',
]
def test_gca():
    u = ui.ui()
    for i, dag in enumerate(dagtests):
        repo = hg.repository(u, 'gca%d' % i, create=1)
        cl = repo.changelog
        if not util.safehasattr(cl.index, 'ancestors'):
            # C version not available
            return

        commands.debugbuilddag(u, repo, dag)
        # Compare the results of the Python and C versions. This does not
        # include choosing a winner when more than one gca exists -- we make
        # sure both return exactly the same set of gcas.
        for a in cl:
            for b in cl:
                cgcas = sorted(cl.index.ancestors(a, b))
                pygcas = sorted(ancestor.ancestors(cl.parentrevs, a, b))
                if cgcas != pygcas:
                    print "test_gca: for dag %s, gcas for %d, %d:" % (dag, a, b)
                    print "  C returned:      %s" % cgcas
                    print "  Python returned: %s" % pygcas

def main():
    seed = None
    opts, args = getopt.getopt(sys.argv[1:], 's:', ['seed='])
    for o, a in opts:
        if o in ('-s', '--seed'):
            seed = long(a, base=0) # accepts base 10 or 16 strings

    if seed is None:
        try:
            seed = long(binascii.hexlify(os.urandom(16)), 16)
        except AttributeError:
            seed = long(time.time() * 1000)

    rng = random.Random(seed)
    test_missingancestors(seed, rng)
    test_lazyancestors()
    test_gca()

if __name__ == '__main__':
    main()
