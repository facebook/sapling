# ancestor.py - generic DAG ancestor algorithm for mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import collections
import heapq

from .node import nullrev

def commonancestorsheads(pfunc, *nodes):
    """Returns a set with the heads of all common ancestors of all nodes,
    heads(::nodes[0] and ::nodes[1] and ...) .

    pfunc must return a list of parent vertices for a given vertex.
    """
    if not isinstance(nodes, set):
        nodes = set(nodes)
    if nullrev in nodes:
        return set()
    if len(nodes) <= 1:
        return nodes

    allseen = (1 << len(nodes)) - 1
    seen = [0] * (max(nodes) + 1)
    for i, n in enumerate(nodes):
        seen[n] = 1 << i
    poison = 1 << (i + 1)

    gca = set()
    interesting = len(nodes)
    nv = len(seen) - 1
    while nv >= 0 and interesting:
        v = nv
        nv -= 1
        if not seen[v]:
            continue
        sv = seen[v]
        if sv < poison:
            interesting -= 1
            if sv == allseen:
                gca.add(v)
                sv |= poison
                if v in nodes:
                    # history is linear
                    return {v}
        if sv < poison:
            for p in pfunc(v):
                sp = seen[p]
                if p == nullrev:
                    continue
                if sp == 0:
                    seen[p] = sv
                    interesting += 1
                elif sp != sv:
                    seen[p] |= sv
        else:
            for p in pfunc(v):
                if p == nullrev:
                    continue
                sp = seen[p]
                if sp and sp < poison:
                    interesting -= 1
                seen[p] = sv
    return gca

def ancestors(pfunc, *orignodes):
    """
    Returns the common ancestors of a and b that are furthest from a
    root (as measured by longest path).

    pfunc must return a list of parent vertices for a given vertex.
    """
    def deepest(nodes):
        interesting = {}
        count = max(nodes) + 1
        depth = [0] * count
        seen = [0] * count
        mapping = []
        for (i, n) in enumerate(sorted(nodes)):
            depth[n] = 1
            b = 1 << i
            seen[n] = b
            interesting[b] = 1
            mapping.append((b, n))
        nv = count - 1
        while nv >= 0 and len(interesting) > 1:
            v = nv
            nv -= 1
            dv = depth[v]
            if dv == 0:
                continue
            sv = seen[v]
            for p in pfunc(v):
                if p == nullrev:
                    continue
                dp = depth[p]
                nsp = sp = seen[p]
                if dp <= dv:
                    depth[p] = dv + 1
                    if sp != sv:
                        interesting[sv] += 1
                        nsp = seen[p] = sv
                        if sp:
                            interesting[sp] -= 1
                            if interesting[sp] == 0:
                                del interesting[sp]
                elif dv == dp - 1:
                    nsp = sp | sv
                    if nsp == sp:
                        continue
                    seen[p] = nsp
                    interesting.setdefault(nsp, 0)
                    interesting[nsp] += 1
                    interesting[sp] -= 1
                    if interesting[sp] == 0:
                        del interesting[sp]
            interesting[sv] -= 1
            if interesting[sv] == 0:
                del interesting[sv]

        if len(interesting) != 1:
            return []

        k = 0
        for i in interesting:
            k |= i
        return set(n for (i, n) in mapping if k & i)

    gca = commonancestorsheads(pfunc, *orignodes)

    if len(gca) <= 1:
        return gca
    return deepest(gca)

class incrementalmissingancestors(object):
    '''persistent state used to calculate missing ancestors incrementally

    Although similar in spirit to lazyancestors below, this is a separate class
    because trying to support contains and missingancestors operations with the
    same internal data structures adds needless complexity.'''
    def __init__(self, pfunc, bases):
        self.bases = set(bases)
        if not self.bases:
            self.bases.add(nullrev)
        self.pfunc = pfunc

    def hasbases(self):
        '''whether the common set has any non-trivial bases'''
        return self.bases and self.bases != {nullrev}

    def addbases(self, newbases):
        '''grow the ancestor set by adding new bases'''
        self.bases.update(newbases)

    def removeancestorsfrom(self, revs):
        '''remove all ancestors of bases from the set revs (in place)'''
        bases = self.bases
        pfunc = self.pfunc
        revs.difference_update(bases)
        # nullrev is always an ancestor
        revs.discard(nullrev)
        if not revs:
            return
        # anything in revs > start is definitely not an ancestor of bases
        # revs <= start needs to be investigated
        start = max(bases)
        keepcount = sum(1 for r in revs if r > start)
        if len(revs) == keepcount:
            # no revs to consider
            return

        for curr in xrange(start, min(revs) - 1, -1):
            if curr not in bases:
                continue
            revs.discard(curr)
            bases.update(pfunc(curr))
            if len(revs) == keepcount:
                # no more potential revs to discard
                break

    def missingancestors(self, revs):
        '''return all the ancestors of revs that are not ancestors of self.bases

        This may include elements from revs.

        Equivalent to the revset (::revs - ::self.bases). Revs are returned in
        revision number order, which is a topological order.'''
        revsvisit = set(revs)
        basesvisit = self.bases
        pfunc = self.pfunc
        bothvisit = revsvisit.intersection(basesvisit)
        revsvisit.difference_update(bothvisit)
        if not revsvisit:
            return []

        start = max(max(revsvisit), max(basesvisit))
        # At this point, we hold the invariants that:
        # - revsvisit is the set of nodes we know are an ancestor of at least
        #   one of the nodes in revs
        # - basesvisit is the same for bases
        # - bothvisit is the set of nodes we know are ancestors of at least one
        #   of the nodes in revs and one of the nodes in bases. bothvisit and
        #   revsvisit are mutually exclusive, but bothvisit is a subset of
        #   basesvisit.
        # Now we walk down in reverse topo order, adding parents of nodes
        # already visited to the sets while maintaining the invariants. When a
        # node is found in both revsvisit and basesvisit, it is removed from
        # revsvisit and added to bothvisit. When revsvisit becomes empty, there
        # are no more ancestors of revs that aren't also ancestors of bases, so
        # exit.

        missing = []
        for curr in xrange(start, nullrev, -1):
            if not revsvisit:
                break

            if curr in bothvisit:
                bothvisit.remove(curr)
                # curr's parents might have made it into revsvisit through
                # another path
                for p in pfunc(curr):
                    revsvisit.discard(p)
                    basesvisit.add(p)
                    bothvisit.add(p)
                continue

            if curr in revsvisit:
                missing.append(curr)
                revsvisit.remove(curr)
                thisvisit = revsvisit
                othervisit = basesvisit
            elif curr in basesvisit:
                thisvisit = basesvisit
                othervisit = revsvisit
            else:
                # not an ancestor of revs or bases: ignore
                continue

            for p in pfunc(curr):
                if p == nullrev:
                    pass
                elif p in othervisit or p in bothvisit:
                    # p is implicitly in thisvisit. This means p is or should be
                    # in bothvisit
                    revsvisit.discard(p)
                    basesvisit.add(p)
                    bothvisit.add(p)
                else:
                    # visit later
                    thisvisit.add(p)

        missing.reverse()
        return missing

class lazyancestors(object):
    def __init__(self, pfunc, revs, stoprev=0, inclusive=False):
        """Create a new object generating ancestors for the given revs. Does
        not generate revs lower than stoprev.

        This is computed lazily starting from revs. The object supports
        iteration and membership.

        cl should be a changelog and revs should be an iterable. inclusive is
        a boolean that indicates whether revs should be included. Revs lower
        than stoprev will not be generated.

        Result does not include the null revision."""
        self._parentrevs = pfunc
        self._initrevs = revs
        self._stoprev = stoprev
        self._inclusive = inclusive

        # Initialize data structures for __contains__.
        # For __contains__, we use a heap rather than a deque because
        # (a) it minimizes the number of parentrevs calls made
        # (b) it makes the loop termination condition obvious
        # Python's heap is a min-heap. Multiply all values by -1 to convert it
        # into a max-heap.
        self._containsvisit = [-rev for rev in revs]
        heapq.heapify(self._containsvisit)
        if inclusive:
            self._containsseen = set(revs)
        else:
            self._containsseen = set()

    def __nonzero__(self):
        """False if the set is empty, True otherwise."""
        try:
            next(iter(self))
            return True
        except StopIteration:
            return False

    __bool__ = __nonzero__

    def __iter__(self):
        """Generate the ancestors of _initrevs in reverse topological order.

        If inclusive is False, yield a sequence of revision numbers starting
        with the parents of each revision in revs, i.e., each revision is *not*
        considered an ancestor of itself.  Results are in breadth-first order:
        parents of each rev in revs, then parents of those, etc.

        If inclusive is True, yield all the revs first (ignoring stoprev),
        then yield all the ancestors of revs as when inclusive is False.
        If an element in revs is an ancestor of a different rev it is not
        yielded again."""
        seen = set()
        revs = self._initrevs
        if self._inclusive:
            for rev in revs:
                yield rev
            seen.update(revs)

        parentrevs = self._parentrevs
        stoprev = self._stoprev
        visit = collections.deque(revs)

        see = seen.add
        schedule = visit.append

        while visit:
            for parent in parentrevs(visit.popleft()):
                if parent >= stoprev and parent not in seen:
                    schedule(parent)
                    see(parent)
                    yield parent

    def __contains__(self, target):
        """Test whether target is an ancestor of self._initrevs."""
        # Trying to do both __iter__ and __contains__ using the same visit
        # heap and seen set is complex enough that it slows down both. Keep
        # them separate.
        seen = self._containsseen
        if target in seen:
            return True

        parentrevs = self._parentrevs
        visit = self._containsvisit
        stoprev = self._stoprev
        heappop = heapq.heappop
        heappush = heapq.heappush
        see = seen.add

        targetseen = False

        while visit and -visit[0] > target and not targetseen:
            for parent in parentrevs(-heappop(visit)):
                if parent < stoprev or parent in seen:
                    continue
                # We need to make sure we push all parents into the heap so
                # that we leave it in a consistent state for future calls.
                heappush(visit, -parent)
                see(parent)
                if parent == target:
                    targetseen = True

        return targetseen
