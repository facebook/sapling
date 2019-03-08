# mutation.py - commit mutation tracking
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from . import error, node as nodemod, util


def record(repo, extra, prednodes, op=None, splitting=None):
    for key in "mutpred", "mutuser", "mutdate", "mutop", "mutsplit":
        if key in extra:
            del extra[key]
    if recording(repo):
        extra["mutpred"] = ",".join(nodemod.hex(p) for p in prednodes)
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
            extra["mutsplit"] = ",".join(nodemod.hex(n) for n in splitting)


def recording(repo):
    return repo.ui.configbool("mutation", "record")


def enabled(repo):
    return repo.ui.configbool("mutation", "enabled")


class mutationcache(object):
    """Cache of derived mutation information for a local repo."""

    def __init__(self, repo):
        self._precomputesuccessorssets(repo)
        self._precomputeobsolete(repo)
        self._precomputeorphans(repo)

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

        # contentdivergenceroots is the set of draft commits for which there
        # are multiple visible successors sets.  The visibile mutable successors
        # are "content-divergent".
        contentdivergenceroots = set()

        # phasedivergenceroots is the set of public commits for which there
        # are visible mutable successors.  The visible mutable successors are
        # "phase-divergent".
        phasedivergenceroots = set()

        def addsuccs(pred, succs):
            succsets = successorssets.setdefault(pred, [])
            if succs not in succsets:
                succsets.append(succs)
            if len(succsets) > 1:
                contentdivergenceroots.add(pred)

        # Compute successor relationships
        for current in unfimutable:
            extra = clrevision(current).extra
            preds = None
            if "mutpred" in extra:
                preds = [nodemod.bin(x) for x in extra["mutpred"].split(",")]
            split = None
            if "mutsplit" in extra:
                split = [nodemod.bin(x) for x in extra["mutsplit"].split(",")]

            # Compute the full set of successors, this is the current commit,
            # plus any commits mentioned in `mutsplit`.
            succs = [current]
            if split is not None:
                for splitnode in split:
                    # Record that this split successor was a result of this
                    # split operation by linking it to the current commit.
                    splitheads[splitnode] = current
                succs = split + succs

            # Now add `succs` as a successor set for all predecessors.
            if preds is not None:
                for pred in preds:
                    addsuccs(pred, succs)
                    if pred in unfi and pred not in unfimutable:
                        # We have traversed back to a public (immutable) commit.
                        # This means its successors might be phase divergent, so
                        # mark the public commit as a phase divergence root.
                        phasedivergenceroots.add(pred)

        # ``successorssets`` is a map from a mutated commit to the sets of
        # commits that immediately replace it.
        self._successorssets = successorssets

        # ``splitheads`` is a map of commits that were created by splitting
        # another commit to the top of the stack that they were split into.
        # The top-of-stack commit contains the mutation record.
        self._splitheads = splitheads

        # ``phasedivergenceroots`` is a set of public commits that have visible
        # draft successors.  The successors are all "phase divergent".
        self._phasedivergenceroots = phasedivergenceroots

        # ``contentdivergenceroots`` is a set of draft commits that have
        # multiple visible eventual successors sets.  These eventual successors
        # sets are all "content divergent".
        self._contentdivergenceroots = contentdivergenceroots

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
                    for succset in successorssets.get(node, ()):
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

    def _precomputeorphans(self, repo):
        obsolete = self._obsolete
        unfi = repo.unfiltered()
        clparents = unfi.changelog.parents
        mutable = set(repo.nodes("not public()"))

        # Compute orphaned and extinct commits by traversing the commit graph looking for
        # obsolete commits.
        #
        # Orphaned commits are equivalent to `obsolete():: - obsolete()`,  and
        # extinct commits are equivalent to `obsolete() - ::orphan()`,
        # except that these won't perform well until we have a fast child
        # look-up.
        orphan = set()
        extinct = set(obsolete)
        for head in repo.nodes("heads(not public())"):
            stack = [head]
            visited = [0]
            # True if all commits up to this point are obsolete.
            allobsolete = [head in obsolete]
            # Stack index of the most recent obsolete commit, or -1 if none are.
            lastobsolete = [0 if head in obsolete else -1]
            while stack:
                current = stack[-1]
                isobsolete = current in obsolete
                if visited[-1] == 0:
                    if isobsolete:
                        orphan.update(stack[lastobsolete[-1] + 1 : -1])
                        if not allobsolete[-1]:
                            extinct.discard(current)
                if visited[-1] < 2:
                    parent = clparents(current)[visited[-1]]
                    visited[-1] += 1
                    if parent != nodemod.nullid and parent in mutable:
                        lastobsolete.append(
                            len(stack) - 1 if isobsolete else lastobsolete[-1]
                        )
                        stack.append(parent)
                        allobsolete.append(allobsolete[-1] and isobsolete)
                        visited.append(0)
                else:
                    stack.pop()
                    allobsolete.pop()
                    lastobsolete.pop()
                    visited.pop()

        # ``orphan`` is the set of all visible but not obsolete commits that
        # have an obsolete ancestor.
        self._orphan = orphan

        # ``extinct`` is the set of all obsolete commits that do not have any
        # orphaned descendants.
        self._extinct = extinct


def allpredecessors(repo, nodes, startdepth=None, stopdepth=None):
    """Yields all the nodes that are predecessors of the given nodes.

    Some predecessors may not be known locally."""
    unfi = repo.unfiltered()
    mc = repo._mutationcache
    cl = unfi.changelog
    clrev = cl.changelogrevision
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
            mainnode = mc._splitheads.get(current, current)
            if mainnode in unfi:
                extra = clrev(mainnode).extra
                pred = None
                if "mutpred" in extra:
                    pred = [nodemod.bin(x) for x in extra["mutpred"].split(",")]
            else:
                continue
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
    mc = repo._mutationcache
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
            for succset in mc._successorssets.get(current, ()):
                nextlevel.update(succset)
        depth += 1
        thislevel = nextlevel
        nextlevel = set()


def obsoletenodes(repo):
    return repo._mutationcache._obsolete


def extinctnodes(repo):
    return repo._mutationcache._extinct


def orphannodes(repo):
    return repo._mutationcache._orphan


def phasedivergentnodes(repo):
    return (
        n
        for n in allsuccessors(
            repo, repo._mutationcache._phasedivergenceroots, startdepth=1
        )
        if n in repo
    )


def contentdivergentnodes(repo):
    return (
        n
        for n in allsuccessors(
            repo, repo._mutationcache._contentdivergenceroots, startdepth=1
        )
        if n in repo
    )


def predecessorsset(repo, startnode, closest=False):
    """Return a list of the commits that were replaced by the startnode.

    If there are no such commits, returns a list containing the startnode.

    If ``closest`` is True, returns a list of the visible commits that are the
    closest previous version of the start node.

    If ``closest`` is False, returns a list of the earliest original versions of
    the start node.
    """
    unfi = repo.unfiltered()
    cl = unfi.changelog
    clrevision = cl.changelogrevision

    def get(node):
        if node in unfi:
            extra = clrevision(node).extra
            if "mutpred" in extra:
                return [nodemod.bin(x) for x in extra["mutpred"].split(",")]
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
