from mercurial import ancestor

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

if __name__ == '__main__':
    test_missingancestors()
