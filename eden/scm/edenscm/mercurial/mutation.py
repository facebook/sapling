# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# mutation.py - commit mutation tracking

from __future__ import absolute_import

from collections import defaultdict

from bindings import mutationstore

from . import error, node as nodemod, perftrace, phases, repoview, util
from .i18n import _
from .node import nullid


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
        if splitting:
            extra["mutsplit"] = ",".join(identfromnode(n) for n in splitting)


def recording(repo):
    return repo.ui.configbool("mutation", "record")


def enabled(repo):
    return repo.ui.configbool("mutation", "enabled")


def makemutationstore(repo):
    return mutationstore.mutationstore(repo.svfs.join("mutation"))


class bundlemutationstore(object):
    def __init__(self, bundlerepo):
        self._entries = {}
        self._splitheads = {}
        self._successorssets = {}
        self._mutationstore = makemutationstore(bundlerepo)

    def addbundleentries(self, entries):
        for entry in entries:
            self._entries[entry.succ()] = entry
            succs = []
            for split in entry.split() or ():
                self._splitheads[split] = entry.succ()
                succs.append(split)
            succs.append(entry.succ())
            for pred in entry.preds():
                self._successorssets.setdefault(pred, []).append(succs)

    def get(self, node):
        return self._mutationstore.get(node) or self._entries.get(node)

    def getsplithead(self, node):
        return self._mutationstore.getsplithead(node) or self._splitheads.get(node)

    def getsuccessorssets(self, node):
        storesets = self._mutationstore.getsuccessorssets(node)
        bundlesets = self._successorssets.get(node, [])
        if bundlesets and storesets:
            return util.removeduplicates(storesets + bundlesets)
        else:
            return storesets or bundlesets

    def has(self, node):
        return self._mutationstore.has(node) or node in self._entries

    def add(self, entry):
        # bundlerepo mutation stores are immutable.  This should never be called.
        raise NotImplementedError

    def flush(self):
        pass


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
    count = 0
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
            count += 1
    return count


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
    succsets = sorted(repo._mutationstore.getsuccessorssets(node))
    return util.removesortedduplicates(succsets)


def allpredecessors(repo, nodes, startdepth=None, stopdepth=None):
    """Yields all the nodes that are predecessors of the given nodes.

    Some predecessors may not be known locally."""
    depth = 0
    thislevel = set(nodes)
    nextlevel = set()
    seen = {nullid}
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


class obsoletecache(object):
    def __init__(self):
        # Set of commits that are known to be obsolete for each filter level.
        self.obsolete = defaultdict(set)

        # Set of commits that are known to be not obsolete for each filter level.
        self.notobsolete = defaultdict(set)

        # If true, then the full set of obsolete commits is known for this
        # filter level, and is stored in ``self.obsolete``.
        self.complete = defaultdict(bool)

    def isobsolete(self, repo, node):
        """Returns true if the node is obsolete in the repository."""
        if node not in repo:
            return False
        obsolete = self.obsolete[repo.filtername]
        if node in obsolete:
            return True
        if self.complete[repo.filtername] or node in self.notobsolete[repo.filtername]:
            return False
        unfi = repo.unfiltered()
        clhasnode = getisvisiblefunc(repo)
        clrev = unfi.changelog.rev
        hiddenrevs = repoview.filterrevs(repo, "visible")

        for succ in allsuccessors(repo, [node], startdepth=1):
            # If any successor is already known to be obsolete, we can
            # assume that the current node is obsolete without checking further.
            if succ in obsolete:
                obsolete.add(node)
                return True
            # The node is obsolete if any successor is visible in the normal
            # filtered repo.
            if clhasnode(succ) and clrev(succ) not in hiddenrevs:
                obsolete.add(node)
                return True
        self.notobsolete[repo.filtername].add(node)
        return False

    def obsoletenodes(self, repo):
        if self.complete[repo.filtername]:
            return self.obsolete[repo.filtername]

        with perftrace.trace("Compute Obsolete Nodes"):
            perftrace.traceflag("mutation")

            # Testing each node separately will result in lots of repeated tests.
            # Instead, we can do the following:
            # - Compute all nodes that are obsolete because one of their closest
            #   successors is visible.
            # - Work back from these commits marking all of their predecessors as
            #   obsolete.
            # Note that "visible" here means "visible in a normal filtered repo",
            # even if the filter for this repo includes other commits.
            clhasnode = getisvisiblefunc(repo)
            clrev = repo.changelog.rev
            obsolete = self.obsolete[repo.filtername]
            hiddenrevs = repoview.filterrevs(repo, "visible")
            for node in repo.nodes("not public()"):
                succsets = successorssets(repo, node, closest=True)
                if succsets != [[node]]:
                    if any(
                        clrev(succ) not in hiddenrevs
                        for succset in succsets
                        for succ in succset
                    ):
                        obsolete.add(node)
            candidates = set(obsolete)
            seen = set(obsolete)
            while candidates:
                candidate = candidates.pop()
                entry = lookupsplit(repo, candidate)
                if entry:
                    for pred in entry.preds():
                        if pred not in obsolete and pred not in seen:
                            candidates.add(pred)
                            seen.add(pred)
                            if clhasnode(pred) and pred != nullid:
                                obsolete.add(pred)
            self.obsolete[repo.filtername] = frozenset(obsolete)
            self.complete[repo.filtername] = True
            # Since we know all obsolete commits, no need to remember which ones
            # are not obsolete.
            if repo.filtername in self.notobsolete:
                del self.notobsolete[repo.filtername]
            return self.obsolete[repo.filtername]


def isobsolete(repo, node):
    if not util.safehasattr(repo, "_mutationobsolete"):
        repo._mutationobsolete = obsoletecache()
    return repo._mutationobsolete.isobsolete(repo, node)


def obsoletenodes(repo):
    if not util.safehasattr(repo, "_mutationobsolete"):
        repo._mutationobsolete = obsoletecache()
    return repo._mutationobsolete.obsoletenodes(repo)


def clearobsoletecache(repo):
    if util.safehasattr(repo, "_mutationobsolete"):
        del repo._mutationobsolete


def fate(repo, node):
    """Returns the fate of a node.

    This returns a list of ([nodes], operation) pairs, indicating mutations that
    happened to this node that resulted in one or more visible commits.
    """
    clrev = repo.changelog.rev
    phase = repo._phasecache.phase
    fate = []
    if isobsolete(repo, node):
        for succset in successorssets(repo, node, closest=True):
            if succset == [node]:
                pass
            elif len(succset) > 1:
                fate.append((succset, "split"))
            else:
                succ = succset[0]
                # Base the default operation name on the successor's phase
                if succ in repo and phase(repo, clrev(succ)) == phases.public:
                    op = "land"
                else:
                    op = "rewrite"
                # Try to find the real operation name.
                entry = lookup(repo, succ)
                if entry is not None:
                    preds = entry.preds()
                    if preds is not None and node in preds:
                        op = entry.op() or op
                fate.append((succset, op))
    return fate


def predecessorsset(repo, startnode, closest=False):
    """Return a list of the commits that were replaced by the startnode.

    If there are no such commits, returns a list containing the startnode.

    If ``closest`` is True, returns a list of the visible commits that are the
    closest previous version of the start node.

    If ``closest`` is False, returns a list of the earliest original versions of
    the start node.
    """
    seen = {startnode}

    def get(node):
        """Get immediate predecessors

        Returns a list of immediate predecessors of the node, omitting
        any predecessors which have already been seen, to prevent loops.

        If the node has no predecessors, returns a list containing the node
        itself.
        """
        entry = lookupsplit(repo, node)
        if entry is not None:
            preds = entry.preds()
            if preds is not None:
                return [pred for pred in preds if pred not in seen] or [node]
        return [node]

    preds = [startnode]
    nextpreds = sum((get(p) for p in preds), [])
    expanded = nextpreds != preds
    while expanded:
        seen.update(nextpreds)
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
        # Remove duplicates from the list of predecessors.  Splits and folds
        # can lead to the same commits via multiple routes, but we don't need
        # to process them multiple times.
        nextpreds = util.removeduplicates(nextpreds)
    return preds


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
    seen = {startnode}

    def getsets(node):
        """Get immediate successors sets

        Returns a list of immediate successors sets of the node, omitting
        any sets which contain nodes already seen, to prevent loops.

        If the node has no successors, returns a list containing a single
        successors set which contains the node itself.
        """
        succsets = [
            succset
            for succset in lookupsuccessors(repo, node)
            if not any(succ in seen for succ in succset)
        ]
        return succsets or [[node]]

    clhasnode = getisvisiblefunc(repo)

    succsets = [[startnode]]
    nextsuccsets = getsets(startnode)
    expanded = nextsuccsets != succsets
    while expanded:
        seen.update(node for succset in nextsuccsets for node in succset)
        if all(clhasnode(s) for succset in nextsuccsets for s in succset):
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
                            [[succ]] if clhasnode(succ) else getsets(succ)
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
                [s for s in succset if clhasnode(s)] for succset in nextsuccsets
            ]
            # Remove sets that are now empty.
            newnextsuccsets = [succset for succset in newnextsuccsets if succset]
            if newnextsuccsets:
                expanded = newnextsuccsets != nextsuccsets
                nextsuccsets = newnextsuccsets
        # Remove duplicates from the list of successors sets.  Splits and folds
        # can lead to the same commits via multiple routes, but we don't need
        # to process them multiple times.
        nextsuccsets = util.removeduplicates(nextsuccsets, key=frozenset)
    return succsets


def foreground(repo, nodes):
    """Returns all nodes in the "foreground" of the given nodes.

    The foreground of a commit is the transitive closure of all descendants
    and successors of the commit.
    """
    unfi = repo.unfiltered()
    nm = unfi.changelog.nodemap
    foreground = set(nodes)
    newnodes = set(nodes)
    while newnodes:
        newnodes.update(n for n in allsuccessors(repo, newnodes) if n in nm)
        newnodes.update(unfi.nodes("%ln::", newnodes))
        newnodes.difference_update(foreground)
        foreground.update(newnodes)
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
    including the nodes themselves.
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
            entries.append(entry.tostoreentry())
            for nextnode in entry.preds():
                if nextnode not in seen:
                    remaining.add(nextnode)

    return mutationstore.bundle(entries)


def convertfromobsmarkers(repo):
    """Convert obsmarkers into mutation records."""
    obsmarkers = repo.obsstore._all
    # Sort obsmarkers by date.  Applying them in probable date order gives us
    # a better chance of resolving cycles in the right way.
    obsmarkers.sort(key=lambda x: x[4])
    newmut = {}

    def checkloopfree(pred, succs):
        candidates = {pred}
        while candidates:
            candidate = candidates.pop()
            if candidate in newmut:
                mutpreds, mutsplit, _markers = newmut[candidate]
                for succ in succs:
                    if succ in mutpreds:
                        repo.ui.debug(
                            "ignoring loop: %s -> %s\n  history loops at %s -> %s\n"
                            % (
                                nodemod.hex(pred),
                                ", ".join([nodemod.hex(s) for s in succs]),
                                nodemod.hex(succ),
                                nodemod.hex(candidate),
                            )
                        )
                        return False
                candidates.update(mutpreds)
        return True

    def checkduplicate(marker, markers):
        return any(marker[:2] == m[:2] for m in markers)

    dropprune = 0
    droprevive = 0
    dropundo = 0
    droploop = 0
    dropinvalid = 0

    for obsmarker in obsmarkers:
        obspred, obssuccs, obsflag, obsmeta, obsdate, obsparents = obsmarker
        if not obssuccs:
            # Skip prune markers
            dropprune += 1
            continue
        if obssuccs == (obspred,):
            # Skip revive markers
            droprevive += 1
            continue
        obsmeta = dict(obsmeta)
        if obsmeta.get("operation") in ("undo", "uncommit", "unamend"):
            # Skip undo-style markers
            dropundo += 1
            continue
        if not checkloopfree(obspred, obssuccs):
            # Skip markers that introduce loops
            droploop += 1
            continue
        if len(obssuccs) > 1:
            # Split marker
            succ = obssuccs[-1]
            if succ in newmut:
                preds, split, markers = newmut[succ]
                if checkduplicate(obsmarker, markers):
                    # duplicate
                    continue
                repo.ui.debug(
                    "invalid obsmarker found: %s -> %s is both split and folded\n"
                    % (nodemod.hex(obspred), nodemod.hex(succ))
                )
                dropinvalid += 1
                continue
            newmut[succ] = ([obspred], obssuccs[:-1], [obsmarker])
        elif obssuccs[0] in newmut:
            preds, split, markers = newmut[obssuccs[0]]
            if checkduplicate(obsmarker, markers):
                # duplicate
                continue
            # Fold marker
            preds.append(obspred)
            markers.append(obsmarker)
        else:
            # Normal marker
            newmut[obssuccs[0]] = ([obspred], None, [obsmarker])

    repo.ui.debug(
        "processed %s markers for %s commits\n" % (len(obsmarkers), len(newmut))
    )
    repo.ui.debug(
        "dropped markers: prune: %s, revive: %s, undo: %s, loop: %s, invalid: %s\n"
        % (dropprune, droprevive, dropundo, droploop, dropinvalid)
    )

    entries = []

    for succ, (preds, split, obsmarkers) in newmut.items():
        if lookup(repo, succ) is not None:
            # Have already converted this successor, or already know about it
            continue
        mutop = ""
        mutuser = ""
        mutdate = None
        for obsmarker in obsmarkers:
            obspred, obssuccs, obsflag, obsmeta, obsdate, obsparents = obsmarker
            obsmeta = dict(obsmeta)
            obsop = obsmeta.get("operation", "")
            if not mutop or obsop not in ("", "copy"):
                mutop = obsop
            obsuser = obsmeta.get("user", "")
            if not mutuser and obsuser:
                mutuser = obsuser
            if not mutdate:
                mutdate = obsdate

        entries.append(
            createsyntheticentry(
                repo, ORIGIN_OBSMARKER, preds, succ, mutop, split, mutuser, mutdate
            )
        )

    with repo.lock():
        count = recordentries(repo, entries, skipexisting=False)

    return (len(entries), len(newmut), count)


def automigrate(repo):
    if recording(repo) and repo.ui.configbool("mutation", "automigrate"):
        msg = _(
            "checking for new obsmarkers to migrate to the mutation store (https://fburl.com/hgmutation)\n"
        )
        repo.ui.status(msg)
        convertfromobsmarkers(repo)


def getisvisiblefunc(repo):
    """get a (node) -> bool function to test visibility

    If narrow-heads is set, visible commits are defined as "::head()".

    visibility here is used to test where a "successor" exists or not.
    """
    if repo.ui.configbool("experimental", "narrow-heads"):
        # Use phase to test.
        # secret phase means "not reachable from public or draft heads", aka. "hidden"
        nodemap = repo.changelog.nodemap
        getphase = repo._phasecache.phase

        def isvisible(
            node, nodemap=nodemap, getphase=getphase, repo=repo, secret=phases.secret
        ):
            try:
                rev = nodemap[node]
            except error.RevlogError:
                return False
            return getphase(repo, rev) != secret

        return isvisible
    else:
        # Use cl.hasnode to test
        return repo.changelog.hasnode
