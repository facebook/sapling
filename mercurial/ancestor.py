# ancestor.py - generic DAG ancestor algorithm for mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import heapq

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
