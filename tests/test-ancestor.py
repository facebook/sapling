from mercurial import ancestor, commands, hg, ui, util

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
pfunc = graph.get

class mockchangelog(object):
    parentrevs = graph.get

def runmissingancestors(revs, bases):
    print "%% ancestors of %s and not of %s" % (revs, bases)
    print ancestor.missingancestors(revs, bases, pfunc)

def test_missingancestors():
    # Empty revs
    runmissingancestors([], [1])
    runmissingancestors([], [])

    # If bases is empty, it's the same as if it were [nullrev]
    runmissingancestors([12], [])

    # Trivial case: revs == bases
    runmissingancestors([0], [0])
    runmissingancestors([4, 5, 6], [6, 5, 4])

    # With nullrev
    runmissingancestors([-1], [12])
    runmissingancestors([12], [-1])

    # 9 is a parent of 12. 7 is a parent of 9, so an ancestor of 12. 6 is an
    # ancestor of 12 but not of 7.
    runmissingancestors([12], [9])
    runmissingancestors([9], [12])
    runmissingancestors([12, 9], [7])
    runmissingancestors([7, 6], [12])

    # More complex cases
    runmissingancestors([10], [11, 12])
    runmissingancestors([11], [10])
    runmissingancestors([11], [10, 12])
    runmissingancestors([12], [10])
    runmissingancestors([12], [11])
    runmissingancestors([10, 11, 12], [13])
    runmissingancestors([13], [10, 11, 12])

def genlazyancestors(revs, stoprev=0, inclusive=False):
    print ("%% lazy ancestor set for %s, stoprev = %s, inclusive = %s" %
           (revs, stoprev, inclusive))
    return ancestor.lazyancestors(mockchangelog, revs, stoprev=stoprev,
                                  inclusive=inclusive)

def printlazyancestors(s, l):
    print [n for n in l if n in s]

def test_lazyancestors():
    # Empty revs
    s = genlazyancestors([])
    printlazyancestors(s, [3, 0, -1])

    # Standard example
    s = genlazyancestors([11, 13])
    printlazyancestors(s, [11, 13, 7, 9, 8, 3, 6, 4, 1, -1, 0])

    # Including revs
    s = genlazyancestors([11, 13], inclusive=True)
    printlazyancestors(s, [11, 13, 7, 9, 8, 3, 6, 4, 1, -1, 0])

    # Test with stoprev
    s = genlazyancestors([11, 13], stoprev=6)
    printlazyancestors(s, [11, 13, 7, 9, 8, 3, 6, 4, 1, -1, 0])
    s = genlazyancestors([11, 13], stoprev=6, inclusive=True)
    printlazyancestors(s, [11, 13, 7, 9, 8, 3, 6, 4, 1, -1, 0])


# The C gca algorithm requires a real repo. These are textual descriptions of
# dags that have been known to be problematic.
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

if __name__ == '__main__':
    test_missingancestors()
    test_lazyancestors()
    test_gca()
