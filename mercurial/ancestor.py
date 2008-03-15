# ancestor.py - generic DAG ancestor algorithm for mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import heapq

def ancestor(a, b, pfunc):
    """
    return the least common ancestor of nodes a and b or None if there
    is no such ancestor.

    pfunc must return a list of parent vertices
    """

    if a == b:
        return a

    # find depth from root of all ancestors
    visit = [a, b]
    depth = {}
    while visit:
        vertex = visit[-1]
        pl = pfunc(vertex)
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
                depth[vertex] = min([depth[p] for p in pl]) - 1
                visit.pop()

    # traverse ancestors in order of decreasing distance from root
    def ancestors(vertex):
        h = [(depth[vertex], vertex)]
        seen = {}
        while h:
            d, n = heapq.heappop(h)
            if n not in seen:
                seen[n] = 1
                yield (d, n)
                for p in pfunc(n):
                    heapq.heappush(h, (depth[p], p))

    def generations(vertex):
        sg, s = None, {}
        for g, v in ancestors(vertex):
            if g != sg:
                if sg:
                    yield sg, s
                sg, s = g, {v:1}
            else:
                s[v] = 1
        yield sg, s

    x = generations(a)
    y = generations(b)
    gx = x.next()
    gy = y.next()

    # increment each ancestor list until it is closer to root than
    # the other, or they match
    try:
        while 1:
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

def symmetricdifference(a, b, pfunc):
    """symmetric difference of the sets of ancestors of a and b

    I.e. revisions that are ancestors of a or b, but not both.
    """
    # basic idea:
    # - mark a and b with different colors
    # - walk the graph in topological order with the help of a heap;
    #   for each revision r:
    #     - if r has only one color, we want to return it
    #     - add colors[r] to its parents
    #
    # We keep track of the number of revisions in the heap that
    # we may be interested in.  We stop walking the graph as soon
    # as this number reaches 0.
    WHITE = 1
    BLACK = 2
    ALLCOLORS = WHITE | BLACK
    colors = {a: WHITE, b: BLACK}

    visit = [-a, -b]
    heapq.heapify(visit)
    n_wanted = len(visit)
    ret = []

    while n_wanted:
        r = -heapq.heappop(visit)
        wanted = colors[r] != ALLCOLORS
        n_wanted -= wanted
        if wanted:
            ret.append(r)

        for p in pfunc(r):
            if p not in colors:
                # first time we see p; add it to visit
                n_wanted += wanted
                colors[p] = colors[r]
                heapq.heappush(visit, -p)
            elif colors[p] != ALLCOLORS and colors[p] != colors[r]:
                # at first we thought we wanted p, but now
                # we know we don't really want it
                n_wanted -= 1
                colors[p] |= colors[r]

        del colors[r]

    return ret
