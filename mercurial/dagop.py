# dagop.py - graph ancestry and topology algorithm for revset
#
# Copyright 2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import heapq

from . import (
    error,
    mdiff,
    node,
    patch,
    smartset,
)

baseset = smartset.baseset
generatorset = smartset.generatorset

# possible maximum depth between null and wdir()
_maxlogdepth = 0x80000000

def _walkrevtree(pfunc, revs, startdepth, stopdepth, reverse):
    """Walk DAG using 'pfunc' from the given 'revs' nodes

    'pfunc(rev)' should return the parent/child revisions of the given 'rev'
    if 'reverse' is True/False respectively.

    Scan ends at the stopdepth (exlusive) if specified. Revisions found
    earlier than the startdepth are omitted.
    """
    if startdepth is None:
        startdepth = 0
    if stopdepth is None:
        stopdepth = _maxlogdepth
    if stopdepth == 0:
        return
    if stopdepth < 0:
        raise error.ProgrammingError('negative stopdepth')
    if reverse:
        heapsign = -1  # max heap
    else:
        heapsign = +1  # min heap

    # load input revs lazily to heap so earlier revisions can be yielded
    # without fully computing the input revs
    revs.sort(reverse)
    irevs = iter(revs)
    pendingheap = []  # [(heapsign * rev, depth), ...] (i.e. lower depth first)

    inputrev = next(irevs, None)
    if inputrev is not None:
        heapq.heappush(pendingheap, (heapsign * inputrev, 0))

    lastrev = None
    while pendingheap:
        currev, curdepth = heapq.heappop(pendingheap)
        currev = heapsign * currev
        if currev == inputrev:
            inputrev = next(irevs, None)
            if inputrev is not None:
                heapq.heappush(pendingheap, (heapsign * inputrev, 0))
        # rescan parents until curdepth >= startdepth because queued entries
        # of the same revision are iterated from the lowest depth
        foundnew = (currev != lastrev)
        if foundnew and curdepth >= startdepth:
            lastrev = currev
            yield currev
        pdepth = curdepth + 1
        if foundnew and pdepth < stopdepth:
            for prev in pfunc(currev):
                if prev != node.nullrev:
                    heapq.heappush(pendingheap, (heapsign * prev, pdepth))

def filectxancestors(fctxs, followfirst=False):
    """Like filectx.ancestors(), but can walk from multiple files/revisions,
    and includes the given fctxs themselves

    Yields (rev, {fctx, ...}) pairs in descending order.
    """
    visit = {}
    visitheap = []
    def addvisit(fctx):
        rev = fctx.rev()
        if rev not in visit:
            visit[rev] = set()
            heapq.heappush(visitheap, -rev)  # max heap
        visit[rev].add(fctx)

    if followfirst:
        cut = 1
    else:
        cut = None

    for c in fctxs:
        addvisit(c)
    while visit:
        currev = -heapq.heappop(visitheap)
        curfctxs = visit.pop(currev)
        yield currev, curfctxs
        for c in curfctxs:
            for parent in c.parents()[:cut]:
                addvisit(parent)
    assert not visitheap

def filerevancestors(fctxs, followfirst=False):
    """Like filectx.ancestors(), but can walk from multiple files/revisions,
    and includes the given fctxs themselves

    Returns a smartset.
    """
    gen = (rev for rev, _cs in filectxancestors(fctxs, followfirst))
    return generatorset(gen, iterasc=False)

def _genrevancestors(repo, revs, followfirst, startdepth, stopdepth, cutfunc):
    if followfirst:
        cut = 1
    else:
        cut = None
    cl = repo.changelog
    def plainpfunc(rev):
        try:
            return cl.parentrevs(rev)[:cut]
        except error.WdirUnsupported:
            return (pctx.rev() for pctx in repo[rev].parents()[:cut])
    if cutfunc is None:
        pfunc = plainpfunc
    else:
        pfunc = lambda rev: [r for r in plainpfunc(rev) if not cutfunc(r)]
        revs = revs.filter(lambda rev: not cutfunc(rev))
    return _walkrevtree(pfunc, revs, startdepth, stopdepth, reverse=True)

def revancestors(repo, revs, followfirst=False, startdepth=None,
                 stopdepth=None, cutfunc=None):
    """Like revlog.ancestors(), but supports additional options, includes
    the given revs themselves, and returns a smartset

    Scan ends at the stopdepth (exlusive) if specified. Revisions found
    earlier than the startdepth are omitted.

    If cutfunc is provided, it will be used to cut the traversal of the DAG.
    When cutfunc(X) returns True, the DAG traversal stops - revision X and
    X's ancestors in the traversal path will be skipped. This could be an
    optimization sometimes.

    Note: if Y is an ancestor of X, cutfunc(X) returning True does not
    necessarily mean Y will also be cut. Usually cutfunc(Y) also wants to
    return True in this case. For example,

        D     # revancestors(repo, D, cutfunc=lambda rev: rev == B)
        |\    # will include "A", because the path D -> C -> A was not cut.
        B C   # If "B" gets cut, "A" might want to be cut too.
        |/
        A
    """
    gen = _genrevancestors(repo, revs, followfirst, startdepth, stopdepth,
                           cutfunc)
    return generatorset(gen, iterasc=False)

def _genrevdescendants(repo, revs, followfirst):
    if followfirst:
        cut = 1
    else:
        cut = None

    cl = repo.changelog
    first = revs.min()
    nullrev = node.nullrev
    if first == nullrev:
        # Are there nodes with a null first parent and a non-null
        # second one? Maybe. Do we care? Probably not.
        yield first
        for i in cl:
            yield i
    else:
        seen = set(revs)
        for i in cl.revs(first):
            if i in seen:
                yield i
                continue
            for x in cl.parentrevs(i)[:cut]:
                if x != nullrev and x in seen:
                    seen.add(i)
                    yield i
                    break

def _builddescendantsmap(repo, startrev, followfirst):
    """Build map of 'rev -> child revs', offset from startrev"""
    cl = repo.changelog
    nullrev = node.nullrev
    descmap = [[] for _rev in xrange(startrev, len(cl))]
    for currev in cl.revs(startrev + 1):
        p1rev, p2rev = cl.parentrevs(currev)
        if p1rev >= startrev:
            descmap[p1rev - startrev].append(currev)
        if not followfirst and p2rev != nullrev and p2rev >= startrev:
            descmap[p2rev - startrev].append(currev)
    return descmap

def _genrevdescendantsofdepth(repo, revs, followfirst, startdepth, stopdepth):
    startrev = revs.min()
    descmap = _builddescendantsmap(repo, startrev, followfirst)
    def pfunc(rev):
        return descmap[rev - startrev]
    return _walkrevtree(pfunc, revs, startdepth, stopdepth, reverse=False)

def revdescendants(repo, revs, followfirst, startdepth=None, stopdepth=None):
    """Like revlog.descendants() but supports additional options, includes
    the given revs themselves, and returns a smartset

    Scan ends at the stopdepth (exlusive) if specified. Revisions found
    earlier than the startdepth are omitted.
    """
    if startdepth is None and stopdepth is None:
        gen = _genrevdescendants(repo, revs, followfirst)
    else:
        gen = _genrevdescendantsofdepth(repo, revs, followfirst,
                                        startdepth, stopdepth)
    return generatorset(gen, iterasc=True)

def _reachablerootspure(repo, minroot, roots, heads, includepath):
    """return (heads(::<roots> and ::<heads>))

    If includepath is True, return (<roots>::<heads>)."""
    if not roots:
        return []
    parentrevs = repo.changelog.parentrevs
    roots = set(roots)
    visit = list(heads)
    reachable = set()
    seen = {}
    # prefetch all the things! (because python is slow)
    reached = reachable.add
    dovisit = visit.append
    nextvisit = visit.pop
    # open-code the post-order traversal due to the tiny size of
    # sys.getrecursionlimit()
    while visit:
        rev = nextvisit()
        if rev in roots:
            reached(rev)
            if not includepath:
                continue
        parents = parentrevs(rev)
        seen[rev] = parents
        for parent in parents:
            if parent >= minroot and parent not in seen:
                dovisit(parent)
    if not reachable:
        return baseset()
    if not includepath:
        return reachable
    for rev in sorted(seen):
        for parent in seen[rev]:
            if parent in reachable:
                reached(rev)
    return reachable

def reachableroots(repo, roots, heads, includepath=False):
    """return (heads(::<roots> and ::<heads>))

    If includepath is True, return (<roots>::<heads>)."""
    if not roots:
        return baseset()
    minroot = roots.min()
    roots = list(roots)
    heads = list(heads)
    try:
        revs = repo.changelog.reachableroots(minroot, heads, roots, includepath)
    except AttributeError:
        revs = _reachablerootspure(repo, minroot, roots, heads, includepath)
    revs = baseset(revs)
    revs.sort()
    return revs

def _changesrange(fctx1, fctx2, linerange2, diffopts):
    """Return `(diffinrange, linerange1)` where `diffinrange` is True
    if diff from fctx2 to fctx1 has changes in linerange2 and
    `linerange1` is the new line range for fctx1.
    """
    blocks = mdiff.allblocks(fctx1.data(), fctx2.data(), diffopts)
    filteredblocks, linerange1 = mdiff.blocksinrange(blocks, linerange2)
    diffinrange = any(stype == '!' for _, stype in filteredblocks)
    return diffinrange, linerange1

def blockancestors(fctx, fromline, toline, followfirst=False):
    """Yield ancestors of `fctx` with respect to the block of lines within
    `fromline`-`toline` range.
    """
    diffopts = patch.diffopts(fctx._repo.ui)
    fctx = fctx.introfilectx()
    visit = {(fctx.linkrev(), fctx.filenode()): (fctx, (fromline, toline))}
    while visit:
        c, linerange2 = visit.pop(max(visit))
        pl = c.parents()
        if followfirst:
            pl = pl[:1]
        if not pl:
            # The block originates from the initial revision.
            yield c, linerange2
            continue
        inrange = False
        for p in pl:
            inrangep, linerange1 = _changesrange(p, c, linerange2, diffopts)
            inrange = inrange or inrangep
            if linerange1[0] == linerange1[1]:
                # Parent's linerange is empty, meaning that the block got
                # introduced in this revision; no need to go futher in this
                # branch.
                continue
            # Set _descendantrev with 'c' (a known descendant) so that, when
            # _adjustlinkrev is called for 'p', it receives this descendant
            # (as srcrev) instead possibly topmost introrev.
            p._descendantrev = c.rev()
            visit[p.linkrev(), p.filenode()] = p, linerange1
        if inrange:
            yield c, linerange2

def blockdescendants(fctx, fromline, toline):
    """Yield descendants of `fctx` with respect to the block of lines within
    `fromline`-`toline` range.
    """
    # First possibly yield 'fctx' if it has changes in range with respect to
    # its parents.
    try:
        c, linerange1 = next(blockancestors(fctx, fromline, toline))
    except StopIteration:
        pass
    else:
        if c == fctx:
            yield c, linerange1

    diffopts = patch.diffopts(fctx._repo.ui)
    fl = fctx.filelog()
    seen = {fctx.filerev(): (fctx, (fromline, toline))}
    for i in fl.descendants([fctx.filerev()]):
        c = fctx.filectx(i)
        inrange = False
        for x in fl.parentrevs(i):
            try:
                p, linerange2 = seen[x]
            except KeyError:
                # nullrev or other branch
                continue
            inrangep, linerange1 = _changesrange(c, p, linerange2, diffopts)
            inrange = inrange or inrangep
            # If revision 'i' has been seen (it's a merge) and the line range
            # previously computed differs from the one we just got, we take the
            # surrounding interval. This is conservative but avoids loosing
            # information.
            if i in seen and seen[i][1] != linerange1:
                lbs, ubs = zip(linerange1, seen[i][1])
                linerange1 = min(lbs), max(ubs)
            seen[i] = c, linerange1
        if inrange:
            yield c, linerange1

def toposort(revs, parentsfunc, firstbranch=()):
    """Yield revisions from heads to roots one (topo) branch at a time.

    This function aims to be used by a graph generator that wishes to minimize
    the number of parallel branches and their interleaving.

    Example iteration order (numbers show the "true" order in a changelog):

      o  4
      |
      o  1
      |
      | o  3
      | |
      | o  2
      |/
      o  0

    Note that the ancestors of merges are understood by the current
    algorithm to be on the same branch. This means no reordering will
    occur behind a merge.
    """

    ### Quick summary of the algorithm
    #
    # This function is based around a "retention" principle. We keep revisions
    # in memory until we are ready to emit a whole branch that immediately
    # "merges" into an existing one. This reduces the number of parallel
    # branches with interleaved revisions.
    #
    # During iteration revs are split into two groups:
    # A) revision already emitted
    # B) revision in "retention". They are stored as different subgroups.
    #
    # for each REV, we do the following logic:
    #
    #   1) if REV is a parent of (A), we will emit it. If there is a
    #   retention group ((B) above) that is blocked on REV being
    #   available, we emit all the revisions out of that retention
    #   group first.
    #
    #   2) else, we'll search for a subgroup in (B) awaiting for REV to be
    #   available, if such subgroup exist, we add REV to it and the subgroup is
    #   now awaiting for REV.parents() to be available.
    #
    #   3) finally if no such group existed in (B), we create a new subgroup.
    #
    #
    # To bootstrap the algorithm, we emit the tipmost revision (which
    # puts it in group (A) from above).

    revs.sort(reverse=True)

    # Set of parents of revision that have been emitted. They can be considered
    # unblocked as the graph generator is already aware of them so there is no
    # need to delay the revisions that reference them.
    #
    # If someone wants to prioritize a branch over the others, pre-filling this
    # set will force all other branches to wait until this branch is ready to be
    # emitted.
    unblocked = set(firstbranch)

    # list of groups waiting to be displayed, each group is defined by:
    #
    #   (revs:    lists of revs waiting to be displayed,
    #    blocked: set of that cannot be displayed before those in 'revs')
    #
    # The second value ('blocked') correspond to parents of any revision in the
    # group ('revs') that is not itself contained in the group. The main idea
    # of this algorithm is to delay as much as possible the emission of any
    # revision.  This means waiting for the moment we are about to display
    # these parents to display the revs in a group.
    #
    # This first implementation is smart until it encounters a merge: it will
    # emit revs as soon as any parent is about to be emitted and can grow an
    # arbitrary number of revs in 'blocked'. In practice this mean we properly
    # retains new branches but gives up on any special ordering for ancestors
    # of merges. The implementation can be improved to handle this better.
    #
    # The first subgroup is special. It corresponds to all the revision that
    # were already emitted. The 'revs' lists is expected to be empty and the
    # 'blocked' set contains the parents revisions of already emitted revision.
    #
    # You could pre-seed the <parents> set of groups[0] to a specific
    # changesets to select what the first emitted branch should be.
    groups = [([], unblocked)]
    pendingheap = []
    pendingset = set()

    heapq.heapify(pendingheap)
    heappop = heapq.heappop
    heappush = heapq.heappush
    for currentrev in revs:
        # Heap works with smallest element, we want highest so we invert
        if currentrev not in pendingset:
            heappush(pendingheap, -currentrev)
            pendingset.add(currentrev)
        # iterates on pending rev until after the current rev have been
        # processed.
        rev = None
        while rev != currentrev:
            rev = -heappop(pendingheap)
            pendingset.remove(rev)

            # Seek for a subgroup blocked, waiting for the current revision.
            matching = [i for i, g in enumerate(groups) if rev in g[1]]

            if matching:
                # The main idea is to gather together all sets that are blocked
                # on the same revision.
                #
                # Groups are merged when a common blocking ancestor is
                # observed. For example, given two groups:
                #
                # revs [5, 4] waiting for 1
                # revs [3, 2] waiting for 1
                #
                # These two groups will be merged when we process
                # 1. In theory, we could have merged the groups when
                # we added 2 to the group it is now in (we could have
                # noticed the groups were both blocked on 1 then), but
                # the way it works now makes the algorithm simpler.
                #
                # We also always keep the oldest subgroup first. We can
                # probably improve the behavior by having the longest set
                # first. That way, graph algorithms could minimise the length
                # of parallel lines their drawing. This is currently not done.
                targetidx = matching.pop(0)
                trevs, tparents = groups[targetidx]
                for i in matching:
                    gr = groups[i]
                    trevs.extend(gr[0])
                    tparents |= gr[1]
                # delete all merged subgroups (except the one we kept)
                # (starting from the last subgroup for performance and
                # sanity reasons)
                for i in reversed(matching):
                    del groups[i]
            else:
                # This is a new head. We create a new subgroup for it.
                targetidx = len(groups)
                groups.append(([], {rev}))

            gr = groups[targetidx]

            # We now add the current nodes to this subgroups. This is done
            # after the subgroup merging because all elements from a subgroup
            # that relied on this rev must precede it.
            #
            # we also update the <parents> set to include the parents of the
            # new nodes.
            if rev == currentrev: # only display stuff in rev
                gr[0].append(rev)
            gr[1].remove(rev)
            parents = [p for p in parentsfunc(rev) if p > node.nullrev]
            gr[1].update(parents)
            for p in parents:
                if p not in pendingset:
                    pendingset.add(p)
                    heappush(pendingheap, -p)

            # Look for a subgroup to display
            #
            # When unblocked is empty (if clause), we were not waiting for any
            # revisions during the first iteration (if no priority was given) or
            # if we emitted a whole disconnected set of the graph (reached a
            # root).  In that case we arbitrarily take the oldest known
            # subgroup. The heuristic could probably be better.
            #
            # Otherwise (elif clause) if the subgroup is blocked on
            # a revision we just emitted, we can safely emit it as
            # well.
            if not unblocked:
                if len(groups) > 1:  # display other subset
                    targetidx = 1
                    gr = groups[1]
            elif not gr[1] & unblocked:
                gr = None

            if gr is not None:
                # update the set of awaited revisions with the one from the
                # subgroup
                unblocked |= gr[1]
                # output all revisions in the subgroup
                for r in gr[0]:
                    yield r
                # delete the subgroup that you just output
                # unless it is groups[0] in which case you just empty it.
                if targetidx:
                    del groups[targetidx]
                else:
                    gr[0][:] = []
    # Check if we have some subgroup waiting for revisions we are not going to
    # iterate over
    for g in groups:
        for r in g[0]:
            yield r
