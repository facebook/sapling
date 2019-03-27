# mutation.py - commit mutation tracking
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from . import error, node as nodemod, phases, repoview, util
from .rust.bindings import mutationstore


ORIGIN_COMMIT = mutationstore.ORIGIN_COMMIT
ORIGIN_OBSMARKER = mutationstore.ORIGIN_OBSMARKER
ORIGIN_SYNTHETIC = mutationstore.ORIGIN_SYNTHETIC
ORIGIN_LOCAL = mutationstore.ORIGIN_LOCAL


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


def makemutationstore(repo):
    return mutationstore.mutationstore(repo.svfs.join("mutation"))


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


def createcommitentry(repo, node):
    extra = repo.changelog.changelogrevision(node).extra
    if "mutpred" in extra:
        return mutationentry(node, extra)


def recordentries(repo, entries, skipexisting=True):
    with repo.transaction("record-mutation") as tr:
        unfi = repo.unfiltered()
        ms = repo._mutationstore
        tr.addfinalize("mutation", lambda _tr: ms.flush())
        for entry in entries:
            if skipexisting:
                succ = entry.succ()
                if succ in unfi or ms.has(succ):
                    continue
            ms.add(entry)


def lookup(repo, node, extra=None):
    """Look up mutation information for the given node

    For the fastpath case where the commit extras are already known, these
    can optionally be passed in through the ``extra`` parameter.
    """
    return repo._mutationstore.get(node)


def lookupsplit(repo, node):
    """Look up mutation information for the given node, or the main split node
    if this node is the result of a split.
    """
    ms = repo._mutationstore
    mainnode = ms.getsplithead(node) or node
    return ms.get(mainnode)


def lookupsuccessors(repo, node):
    """Look up the immediate successors sets for the given node"""
    return sorted(repo._mutationstore.getsuccessorssets(node))


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


def isobsolete(repo, node):
    """Returns true if the node is obsolete in the repository."""
    if node not in repo:
        return False
    if not util.safehasattr(repo, "_mutationobsolete"):
        repo._mutationobsolete = set()
    obsolete = repo._mutationobsolete
    if node in obsolete:
        return True
    unfi = repo.unfiltered()
    clrev = unfi.changelog.rev
    hiddenrevs = repoview.filterrevs(repo, "visible")

    for succ in allsuccessors(repo, [node], startdepth=1):
        # If any successor is already known to be obsolete, we can
        # assume that the current node is obsolete without checking further.
        if succ in obsolete:
            return True
        # The node is obsolete if any successor is visible in the repo.
        if succ in unfi:
            if clrev(succ) not in hiddenrevs:
                obsolete.add(node)
                return True
    return False


def obsoletenodes(repo):
    return {node for node in repo.nodes("not public()") if isobsolete(repo, node)}


def clearobsoletecache(repo):
    if util.safehasattr(repo, "_mutationobsolete"):
        del repo._mutationobsolete


def fate(repo, node):
    """Returns the fate of a node.

    This returns a list of ([nodes], operation) pairs, indicating mutations that
    happened to this node that resulted in one or more visible commits.
    """
    clrev = repo.changelog.rev
    phasecache = repo._phasecache
    fate = []
    if isobsolete(repo, node):
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
                elif (
                    succ in repo
                    and phasecache.phase(repo, clrev(succ)) == phases.public
                ):
                    fate.append((succset, "land"))
                else:
                    fate.append((succset, "rewrite"))
    return fate


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
