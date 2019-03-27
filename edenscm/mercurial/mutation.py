# mutation.py - commit mutation tracking
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from . import error, node as nodemod, phases, util
from .rust.bindings import mutationstore


ORIGIN_COMMIT = mutationstore.ORIGIN_COMMIT
ORIGIN_OBSMARKER = mutationstore.ORIGIN_OBSMARKER
ORIGIN_SYNTHETIC = mutationstore.ORIGIN_SYNTHETIC


def identfromnode(node):
    return "hg/%s" % nodemod.hex(node)


def nodefromident(ident):
    if ident.startswith("hg/"):
        return nodemod.bin(ident[3:])
    raise error.Abort("Unrecognised commit identifier: %s" % ident)


def record(repo, extra, prednodes, op=None, splitting=None):
    for key in "mutpred", "mutuser", "mutdate", "mutop", "mutsplit":
        if key in extra:
            del extra[key]
    if recording(repo):
        extra["mutpred"] = ",".join(identfromnode(p) for p in prednodes)
        extra["mutuser"] = repo.ui.config("mutation", "user") or repo.ui.username()
        date = repo.ui.config("mutation", "date")
        if date is None:
            date = util.makedate()
        else:
            date = util.parsedate(date)
        extra["mutdate"] = "%d %d" % date
        if op is not None:
            extra["mutop"] = op
        if splitting is not None:
            extra["mutsplit"] = ",".join(identfromnode(n) for n in splitting)


def recording(repo):
    return repo.ui.configbool("mutation", "record")


def enabled(repo):
    return repo.ui.configbool("mutation", "enabled")


class mutationentry(object):
    def __init__(self, node, extra):
        self.extra = extra
        self.node = node

    def origin(self):
        return None

    def succ(self):
        return self.node

    def preds(self):
        if "mutpred" in self.extra:
            return [nodefromident(x) for x in self.extra["mutpred"].split(",")]

    def split(self):
        if "mutsplit" in self.extra:
            return [nodefromident(x) for x in self.extra["mutsplit"].split(",")]

    def op(self):
        return self.extra.get("mutop")

    def user(self):
        return self.extra.get("mutuser")

    def time(self):
        if "mutdate" in self.extra:
            return float(self.extra.get("mutdate").split()[0])

    def tz(self):
        if "mutdate" in self.extra:
            return int(self.extra.get("mutdate").split()[1])

    def tostoreentry(self, origin=ORIGIN_COMMIT):
        if "mutpred" in self.extra:
            return mutationstore.mutationentry(
                origin,
                self.node,
                self.preds(),
                self.split(),
                self.op() or "",
                self.user() or "",
                self.time() or 0,
                self.tz() or 0,
                None,
            )


def createsyntheticentry(
    repo, origin, preds, succ, op, splitting=None, user=None, date=None
):
    user = user or repo.ui.config("mutation", "user") or repo.ui.username()
    date = date or repo.ui.config("mutation", "date")
    if date is None:
        date = util.makedate()
    else:
        date = util.parsedate(date)
    return mutationstore.mutationentry(
        origin, succ, preds, splitting, op, user, date[0], date[1], None
    )


def recordentries(repo, entries, skipexisting=True):
    unfi = repo.unfiltered()
    mc = repo._mutationcache
    for entry in entries:
        if skipexisting:
            succ = entry.succ()
            if succ in unfi or mc.store.has(succ):
                continue
        repo._mutationcache.store.add(entry)
    # TODO(mbthomas): take part in transactions
    repo._mutationcache.store.flush()


class mutationcache(object):
    """Cache of derived mutation information for a local repo."""

    def __init__(self, repo):
        self.store = mutationstore.mutationstore(repo.svfs.join("mutation"))
        self._precomputesuccessorssets(repo)
        self._precomputeobsolete(repo)

    def _precomputesuccessorssets(self, repo):
        """"""
        unfi = repo.unfiltered()
        clrevision = unfi.changelog.changelogrevision
        unfimutable = set(unfi.nodes("not public()"))

        # successorssets maps mutated commits to the sets of successors.  This
        # is a map from commit node to lists of successors sets.  In the cache
        # these are the immediate successors, whether or not they are obsolete.
        successorssets = {}

        # splitheads maps split destinations to the top of the stack that they
        # were split into.  The top of the stack contains the split metadata
        # and is the real successor of the commit that was split.
        splitheads = {}

        def addsuccs(pred, succs):
            succsets = successorssets.setdefault(pred, [])
            if succs not in succsets:
                succsets.append(succs)

        # Compute successor relationships
        for current in unfimutable:
            entry = mutationentry(current, clrevision(current).extra)

            # Compute the full set of successors, this is the current commit,
            # plus any commits mentioned in `mutsplit`.
            succs = [current]
            split = entry.split()
            if split is not None:
                for splitnode in split:
                    # Record that this split successor was a result of this
                    # split operation by linking it to the current commit.
                    splitheads[splitnode] = current
                succs = split + succs

            # Now add `succs` as a successor set for all predecessors.
            preds = entry.preds()
            if preds is not None:
                for pred in preds:
                    addsuccs(pred, succs)

        # ``successorssets`` is a map from a mutated commit to the sets of
        # commits that immediately replace it.
        self._successorssets = successorssets

        # ``splitheads`` is a map of commits that were created by splitting
        # another commit to the top of the stack that they were split into.
        # The top-of-stack commit contains the mutation record.
        self._splitheads = splitheads

    def _precomputeobsolete(self, repo):
        successorssets = self._successorssets

        # Compute obsolete commits by traversing the commit graph looking for
        # commits that have a visible or obsolete successor.
        obsolete = set()
        for current in repo.nodes("not public()"):
            thislevel = {current}
            nextlevel = set()
            seen = set()
            while thislevel:
                for node in thislevel:
                    if node in seen:
                        continue
                    seen.add(node)
                    # Get successors from both this cache and the store.  We
                    # can't use lookupsuccessors as we're still building the
                    # cache.
                    for succset in successorssets.get(node, ()):
                        nextlevel.update(succset)
                    for succset in self.store.getsuccessorssets(node):
                        nextlevel.update(succset)
                # This node is obsolete if any successor is visible in the repo.
                # If any successor is already known to be obsolete, we can also
                # assume that the current node is obsolete without checking
                # further.
                if any(
                    nextnode in obsolete or nextnode in repo for nextnode in nextlevel
                ):
                    obsolete.add(current)
                    break
                thislevel = nextlevel
                nextlevel = set()

        # ``obsolete`` is the set of all visible commits that have been mutated
        # (i.e., have a visible successor).
        self._obsolete = obsolete


def lookup(repo, node, extra=None):
    """Look up mutation information for the given node

    For the fastpath case where the commit extras are already known, these
    can optionally be passed in through the ``extra`` parameter.
    """
    unfi = repo.unfiltered()
    if extra is None and node in unfi:
        extra = unfi.changelog.changelogrevision(node).extra
    if extra is not None and "mutpred" in extra:
        return mutationentry(node, extra)
    else:
        return repo._mutationcache.store.get(node)


def lookupsplit(repo, node):
    """Look up mutation information for the given node, or the main split node
    if this node is the result of a split.
    """
    unfi = repo.unfiltered()
    mc = repo._mutationcache
    extra = None
    mainnode = mc._splitheads.get(node) or mc.store.getsplithead(node) or node
    if mainnode in unfi:
        extra = unfi.changelog.changelogrevision(mainnode).extra
    if extra is not None and "mutpred" in extra:
        return mutationentry(mainnode, extra)
    else:
        return mc.store.get(mainnode)


def lookupsuccessors(repo, node):
    """Look up the immediate successors sets for the given node"""
    mc = repo._mutationcache
    cachesuccsets = sorted(mc._successorssets.get(node, []))
    storesuccsets = sorted(mc.store.getsuccessorssets(node))
    return util.mergelists(cachesuccsets, storesuccsets) or None


def allpredecessors(repo, nodes, startdepth=None, stopdepth=None):
    """Yields all the nodes that are predecessors of the given nodes.

    Some predecessors may not be known locally."""
    depth = 0
    thislevel = set(nodes)
    nextlevel = set()
    seen = set()
    while thislevel and (stopdepth is None or depth < stopdepth):
        for current in thislevel:
            if current in seen:
                continue
            seen.add(current)
            if startdepth is None or depth >= startdepth:
                yield current
            pred = None
            entry = lookupsplit(repo, current)
            if entry is not None:
                pred = entry.preds()
            if pred is not None:
                for nextnode in pred:
                    if nextnode not in seen:
                        nextlevel.add(nextnode)
        depth += 1
        thislevel = nextlevel
        nextlevel = set()


def allsuccessors(repo, nodes, startdepth=None, stopdepth=None):
    """Yields all the nodes that are successors of the given nodes.

    Successors that are not known locally may be omitted."""
    depth = 0
    thislevel = set(nodes)
    nextlevel = set()
    seen = set()
    while thislevel and (stopdepth is None or depth < stopdepth):
        for current in thislevel:
            if current in seen:
                continue
            seen.add(current)
            if startdepth is None or depth >= startdepth:
                yield current
            succsets = lookupsuccessors(repo, current)
            if succsets:
                nextlevel = nextlevel.union(*succsets)
        depth += 1
        thislevel = nextlevel
        nextlevel = set()


def fate(repo, node):
    """Returns the fate of a node.

    This returns a list of ([nodes], operation) pairs, indicating mutations that
    happened to this node that resulted in one or more visible commits.
    """
    clrev = repo.changelog.rev
    phasecache = repo._phasecache
    fate = []
    for succset in successorssets(repo, node, closest=True):
        if succset == [node]:
            pass
        elif len(succset) > 1:
            fate.append((succset, "split"))
        else:
            succ = succset[0]
            preds = None
            entry = lookup(repo, succ)
            if entry is not None:
                preds = entry.preds()
                op = entry.op()
            if preds is not None and node in preds:
                fate.append((succset, op))
            elif succ in repo and phasecache.phase(repo, clrev(succ)) == phases.public:
                fate.append((succset, "land"))
            else:
                fate.append((succset, "rewrite"))
    return fate


def obsoletenodes(repo):
    return repo._mutationcache._obsolete


def predecessorsset(repo, startnode, closest=False):
    """Return a list of the commits that were replaced by the startnode.

    If there are no such commits, returns a list containing the startnode.

    If ``closest`` is True, returns a list of the visible commits that are the
    closest previous version of the start node.

    If ``closest`` is False, returns a list of the earliest original versions of
    the start node.
    """

    def get(node):
        entry = lookupsplit(repo, node)
        if entry is not None:
            preds = entry.preds()
            if preds is not None:
                return preds
        return [node]

    preds = [startnode]
    nextpreds = sum((get(p) for p in preds), [])
    expanded = nextpreds != preds
    while expanded:
        if all(p in repo for p in nextpreds):
            # We have found a set of predecessors that are all visible - this is
            # a valid set to return.
            preds = nextpreds
            if closest:
                break
            # Now look at the next predecessors of each commit.
            newnextpreds = sum((get(p) for p in nextpreds), [])
        else:
            # Expand out to the predecessors of the commits until we find visible
            # ones.
            newnextpreds = sum(([p] if p in repo else get(p) for p in nextpreds), [])
        expanded = newnextpreds != nextpreds
        nextpreds = newnextpreds
        if not expanded:
            # We've reached a stable state and some of the commits might not be
            # visible.  Remove the invisible commits, and continue with what's
            # left.
            newnextpreds = [p for p in nextpreds if p in repo]
            if newnextpreds:
                expanded = newnextpreds != nextpreds
                nextpreds = newnextpreds
    return util.removeduplicates(preds)


def _succproduct(succsetlist):
    """Takes a list of successorsset lists and returns a single successorsset
    list representing the cartesian product of those successorsset lists.

    The ordering of elements within the lists must be preserved.

    >>> _succproduct([[[1]], [[2]]])
    [[1, 2]]
    >>> _succproduct([[[1, 2]], [[3, 4]]])
    [[1, 2, 3, 4]]
    >>> _succproduct([[[1, 2], [3, 4]], [[5, 6]]])
    [[1, 2, 5, 6], [3, 4, 5, 6]]
    >>> _succproduct([[[1, 2], [3, 4]], [[5, 6], [7, 8]]])
    [[1, 2, 5, 6], [3, 4, 5, 6], [1, 2, 7, 8], [3, 4, 7, 8]]
    >>> _succproduct([[[1, 2], [3, 4]], [[2, 3], [7, 8]]])
    [[1, 2, 3], [3, 4, 2], [1, 2, 7, 8], [3, 4, 7, 8]]
    >>> _succproduct([[[1, 2], [3, 4]], [[1, 2], [7, 8]]])
    [[1, 2], [3, 4, 1, 2], [1, 2, 7, 8], [3, 4, 7, 8]]
    >>> _succproduct([[[1], [2]], [[3], [4]]])
    [[1, 3], [2, 3], [1, 4], [2, 4]]
    >>> _succproduct([[[5]], [[4]], [[3]], [[2]], [[1]]])
    [[5, 4, 3, 2, 1]]
    """
    # Start with the first successorsset.
    product = succsetlist[0]
    for succsets in succsetlist[1:]:
        # For each of the remaining successorssets, compute the product with
        # the successorsset so far.
        newproduct = []
        for succset in succsets:
            for p in product:
                newproduct.append(p + [s for s in succset if s not in p])
        product = newproduct
    return product


def successorssets(repo, startnode, closest=False, cache=None):
    """Return a list of lists of commits that replace the startnode.

    If there are no such commits, returns a list containing a single list
    containing the startnode.

    If ``closest`` is True, the lists contain the visible commits that are the
    closest next version of the start node.

    If ``closest`` is False, the lists contain the latest versions of the start
    node.

    The ``cache`` parameter is unused.  It is provided to make this function
    signature-compatible with ``obsutil.successorssets``.
    """

    def getsets(node):
        return lookupsuccessors(repo, node) or [[node]]

    succsets = [[startnode]]
    nextsuccsets = getsets(startnode)
    expanded = nextsuccsets != succsets
    while expanded:
        if all(s in repo for succset in nextsuccsets for s in succset):
            # We have found a set of successor sets that all contain visible
            # commits - this is a valid set to return.
            succsets = nextsuccsets
            if closest:
                break

            # Now look at the next successors of each successors set.  When
            # commits are modified in different ways (i.e. they have been
            # diverged), we need to find all possible permutations that replace
            # the original nodes.  To do this, we compute the cartesian product
            # of the successors sets of each successor in the original
            # successors set.
            #
            # For example, if A is split into B and C, B is diverged to B1 and
            # B2, and C is also diverged to C1 and C2, then the successors sets
            # of A are: [B1, C1], [B1, C2], [B2, C1], [B2, C2], which is the
            # cartesian product: [B1, B2] x [C1, C2].
            newnextsuccsets = sum(
                [
                    _succproduct([getsets(succ) for succ in succset])
                    for succset in nextsuccsets
                ],
                [],
            )
        else:
            # Expand each successors set out to its successors until we find
            # visible commit.  Again, use the cartesian product to find all
            # permutations.
            newnextsuccsets = sum(
                [
                    _succproduct(
                        [
                            [[succ]] if succ in repo else getsets(succ)
                            for succ in succset
                        ]
                    )
                    for succset in nextsuccsets
                ],
                [],
            )
        expanded = newnextsuccsets != nextsuccsets
        nextsuccsets = newnextsuccsets
        if not expanded:
            # We've reached a stable state and some of the commits might not be
            # visible.  Remove the invisible commits, and continue with what's
            # left.
            newnextsuccsets = [
                [s for s in succset if s in repo] for succset in nextsuccsets
            ]
            # Remove sets that are now empty.
            newnextsuccsets = [succset for succset in newnextsuccsets if succset]
            if newnextsuccsets:
                expanded = newnextsuccsets != nextsuccsets
                nextsuccsets = newnextsuccsets
    return util.removeduplicates(succsets, key=frozenset)


def foreground(repo, nodes):
    """Returns all nodes in the "foreground" of the given nodes.

    The foreground of a commit is the transitive closure of all descendants
    and successors of the commit.
    """
    unfi = repo.unfiltered()
    foreground = set()
    newctxs = set(unfi.set("%ln::", nodes))
    while newctxs:
        newnodes = set(c.node() for c in newctxs) - foreground
        newnodes.update(allsuccessors(repo, newnodes))
        foreground = foreground | newnodes
        newctxs = set(unfi.set("(%ln::) - (%ln)", newnodes, newnodes))
    return foreground


def toposortrevs(repo, revs, predmap):
    """topologically sort revs according to the given predecessor map"""
    dag = {}
    valid = set(revs)
    heads = set(revs)
    clparentrevs = repo.changelog.parentrevs
    for rev in revs:
        prev = [p for p in clparentrevs(rev) if p in valid]
        prev.extend(predmap[rev])
        heads.difference_update(prev)
        dag[rev] = prev
    if not heads:
        raise error.Abort("commit predecessors and ancestors contain a cycle")
    seen = set()
    sortedrevs = []
    revstack = list(reversed(sorted(heads)))
    while revstack:
        rev = revstack[-1]
        if rev not in seen:
            seen.add(rev)
            for next in reversed(dag[rev]):
                if next not in seen:
                    revstack.append(next)
        else:
            sortedrevs.append(rev)
            revstack.pop()
    return sortedrevs


def toposort(repo, items, nodefn=None):
    """topologically sort nodes according to the given predecessor map

    items can either be nodes, or something convertible to nodes by a provided
    node function.
    """
    if nodefn is None:
        nodefn = lambda item: item
    clrev = repo.changelog.rev
    revmap = {clrev(nodefn(x)): i for i, x in enumerate(items)}
    predmap = {}
    for item in items:
        node = nodefn(item)
        rev = clrev(node)
        predmap[rev] = [
            r
            for r in map(clrev, predecessorsset(repo, node, closest=True))
            if r != rev and r in revmap
        ]
    sortedrevs = toposortrevs(repo, revmap.keys(), predmap)
    return [items[revmap[r]] for r in sortedrevs]


def unbundle(repo, bundledata):
    if recording(repo):
        entries = mutationstore.unbundle(bundledata)
        recordentries(repo, entries, skipexisting=True)


def bundle(repo, nodes):
    """Generate bundled mutation data for bundling alongside the given nodes.

    This consists of mutation entries for all predecessors of the given nodes,
    excluding the nodes themselves, as they are expected to have the mutation
    information embedded in the commit extras.
    """
    nodes = set(nodes)
    remaining = set(nodes)
    seen = set()
    entries = []
    while remaining:
        current = remaining.pop()
        if current in seen:
            continue
        seen.add(current)

        entry = lookupsplit(repo, current)
        if entry is not None:
            if entry.succ() not in nodes:
                entries.append(entry.tostoreentry())
            for nextnode in entry.preds():
                if nextnode not in seen:
                    remaining.add(nextnode)

    return mutationstore.bundle(entries)
