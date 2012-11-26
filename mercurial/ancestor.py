# ancestor.py - generic DAG ancestor algorithm for mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import heapq
from node import nullrev

def ancestor(a, b, pfunc):
    """
    Returns the common ancestor of a and b that is furthest from a
    root (as measured by longest path) or None if no ancestor is
    found. If there are multiple common ancestors at the same
    distance, the first one found is returned.

    pfunc must return a list of parent vertices for a given vertex
    """

    if a == b:
        return a

    a, b = sorted([a, b])

    # find depth from root of all ancestors
    # depth is stored as a negative for heapq
    parentcache = {}
    visit = [a, b]
    depth = {}
    while visit:
        vertex = visit[-1]
        pl = pfunc(vertex)
        parentcache[vertex] = pl
        if not pl:
            depth[vertex] = 0
            visit.pop()
        else:
            for p in pl:
                if p == a or p == b: # did we find a or b as a parent?
                    return p # we're done
                if p not in depth:
                    visit.append(p)
            if visit[-1] == vertex:
                # -(maximum distance of parents + 1)
                depth[vertex] = min([depth[p] for p in pl]) - 1
                visit.pop()

    # traverse ancestors in order of decreasing distance from root
    def ancestors(vertex):
        h = [(depth[vertex], vertex)]
        seen = set()
        while h:
            d, n = heapq.heappop(h)
            if n not in seen:
                seen.add(n)
                yield (d, n)
                for p in parentcache[n]:
                    heapq.heappush(h, (depth[p], p))

    def generations(vertex):
        sg, s = None, set()
        for g, v in ancestors(vertex):
            if g != sg:
                if sg:
                    yield sg, s
                sg, s = g, set((v,))
            else:
                s.add(v)
        yield sg, s

    x = generations(a)
    y = generations(b)
    gx = x.next()
    gy = y.next()

    # increment each ancestor list until it is closer to root than
    # the other, or they match
    try:
        while True:
            if gx[0] == gy[0]:
                for v in gx[1]:
                    if v in gy[1]:
                        return v
                gy = y.next()
                gx = x.next()
            elif gx[0] > gy[0]:
                gy = y.next()
            else:
                gx = x.next()
    except StopIteration:
        return None

def missingancestors(revs, bases, pfunc):
    """Return all the ancestors of revs that are not ancestors of bases.

    This may include elements from revs.

    Equivalent to the revset (::revs - ::bases). Revs are returned in
    revision number order, which is a topological order.

    revs and bases should both be iterables. pfunc must return a list of
    parent revs for a given revs.

    graph is a dict of child->parent adjacency lists for this graph:
    o  13
    |
    | o  12
    | |
    | | o    11
    | | |\
    | | | | o  10
    | | | | |
    | o---+ |  9
    | | | | |
    o | | | |  8
     / / / /
    | | o |  7
    | | | |
    o---+ |  6
     / / /
    | | o  5
    | |/
    | o  4
    | |
    o |  3
    | |
    | o  2
    |/
    o  1
    |
    o  0
    >>> graph = {0: [-1], 1: [0], 2: [1], 3: [1], 4: [2], 5: [4], 6: [4],
    ...          7: [4], 8: [-1], 9: [6, 7], 10: [5], 11: [3, 7], 12: [9],
    ...          13: [8]}
    >>> pfunc = graph.get

    Empty revs
    >>> missingancestors([], [1], pfunc)
    []
    >>> missingancestors([], [], pfunc)
    []

    If bases is empty, it's the same as if it were [nullrev]
    >>> missingancestors([12], [], pfunc)
    [0, 1, 2, 4, 6, 7, 9, 12]

    Trivial case: revs == bases
    >>> missingancestors([0], [0], pfunc)
    []
    >>> missingancestors([4, 5, 6], [6, 5, 4], pfunc)
    []

    With nullrev
    >>> missingancestors([-1], [12], pfunc)
    []
    >>> missingancestors([12], [-1], pfunc)
    [0, 1, 2, 4, 6, 7, 9, 12]

    9 is a parent of 12. 7 is a parent of 9, so an ancestor of 12. 6 is an
    ancestor of 12 but not of 7.
    >>> missingancestors([12], [9], pfunc)
    [12]
    >>> missingancestors([9], [12], pfunc)
    []
    >>> missingancestors([12, 9], [7], pfunc)
    [6, 9, 12]
    >>> missingancestors([7, 6], [12], pfunc)
    []

    More complex cases
    >>> missingancestors([10], [11, 12], pfunc)
    [5, 10]
    >>> missingancestors([11], [10], pfunc)
    [3, 7, 11]
    >>> missingancestors([11], [10, 12], pfunc)
    [3, 11]
    >>> missingancestors([12], [10], pfunc)
    [6, 7, 9, 12]
    >>> missingancestors([12], [11], pfunc)
    [6, 9, 12]
    >>> missingancestors([10, 11, 12], [13], pfunc)
    [0, 1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12]
    >>> missingancestors([13], [10, 11, 12], pfunc)
    [8, 13]
    """

    revsvisit = set(revs)
    basesvisit = set(bases)
    if not revsvisit:
        return []
    if not basesvisit:
        basesvisit.add(nullrev)
    start = max(max(revsvisit), max(basesvisit))
    bothvisit = revsvisit.intersection(basesvisit)
    revsvisit.difference_update(bothvisit)
    basesvisit.difference_update(bothvisit)
    # At this point, we hold the invariants that:
    # - revsvisit is the set of nodes we know are an ancestor of at least one
    #   of the nodes in revs
    # - basesvisit is the same for bases
    # - bothvisit is the set of nodes we know are ancestors of at least one of
    #   the nodes in revs and one of the nodes in bases
    # - a node may be in none or one, but not more, of revsvisit, basesvisit
    #   and bothvisit at any given time
    # Now we walk down in reverse topo order, adding parents of nodes already
    # visited to the sets while maintaining the invariants. When a node is
    # found in both revsvisit and basesvisit, it is removed from them and
    # added to bothvisit instead. When revsvisit becomes empty, there are no
    # more ancestors of revs that aren't also ancestors of bases, so exit.

    missing = []
    for curr in xrange(start, nullrev, -1):
        if not revsvisit:
            break

        if curr in bothvisit:
            bothvisit.remove(curr)
            # curr's parents might have made it into revsvisit or basesvisit
            # through another path
            for p in pfunc(curr):
                revsvisit.discard(p)
                basesvisit.discard(p)
                bothvisit.add(p)
            continue

        # curr will never be in both revsvisit and basesvisit, since if it
        # were it'd have been pushed to bothvisit
        if curr in revsvisit:
            missing.append(curr)
            thisvisit = revsvisit
            othervisit = basesvisit
        elif curr in basesvisit:
            thisvisit = basesvisit
            othervisit = revsvisit
        else:
            # not an ancestor of a or b: ignore
            continue

        thisvisit.remove(curr)
        for p in pfunc(curr):
            if p == nullrev:
                pass
            elif p in othervisit or p in bothvisit:
                # p is implicitly in thisvisit. This means p is or should be
                # in bothvisit
                revsvisit.discard(p)
                basesvisit.discard(p)
                bothvisit.add(p)
            else:
                # visit later
                thisvisit.add(p)

    missing.reverse()
    return missing
