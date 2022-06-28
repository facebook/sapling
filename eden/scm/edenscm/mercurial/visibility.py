# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# visibility.py - tracking visibility through visible heads

from __future__ import absolute_import

import bindings
from edenscm.mercurial import node, util
from edenscm.mercurial.i18n import _


def _convertfromobsolete(repo):
    """convert obsolete markers into a set of visible heads"""
    with repo.ui.configoverride(
        {("mutation", "enabled"): False, ("visibility", "enabled"): False},
        "convertfromobsolete",
    ):
        return list(repo.nodes("heads((not public()) - hidden())"))


def starttracking(repo) -> None:
    """start tracking visibility information through visible mutable heads"""
    if "visibleheads" not in repo.storerequirements:
        with repo.lock():
            repo.storerequirements.add("visibleheads")
            setvisibleheads(repo, _convertfromobsolete(repo))
            repo._writestorerequirements()


def stoptracking(repo) -> None:
    """stop tracking visibility information and revert to using obsmarkers"""
    with repo.lock(), repo.transaction("disable-visibility"):
        if "visibleheads" in repo.storerequirements:
            repo.storerequirements.discard("visibleheads")
            repo._writestorerequirements()
        repo.svfs.write("visibleheads", b"")


encodeheads = bindings.refencode.encodevisibleheads
decodeheads = bindings.refencode.decodevisibleheads


class visibleheads(object):
    """tracks visible non-public heads in the repostory

    Track visibility of non-public commits through a set of heads.  This only
    covers non-public (draft and secret) commits - public commits are always
    visible.

    This class is responsible for tracking the set of heads, and persisting
    them to the store.
    """

    LOGHEADLIMIT = 4

    # Count of changes. Useful for cache invalidation.
    _changecount = 0

    def __init__(self, vfs):
        self.vfs = vfs
        self._invisiblerevs = None
        self._allheads = bindings.nodemap.nodeset(vfs.join("allheads"))

        heads = self.heads
        self._logheads("read", visibility_headcount=len(heads))
        if heads:
            # Populate allheads with heads
            add = self._allheads.add
            for head in heads:
                add(head)

    @property
    def changecount(self):
        return self._changecount

    @property
    def heads(self):
        return decodeheads(self.vfs.metalog["visibleheads"])

    @heads.setter
    def heads(self, heads):
        self.vfs.metalog["visibleheads"] = encodeheads(heads)

    def _logposttransaction(self, fp):
        self._logheads("wrote", visibility_newheadcount=len(self.heads))

    def _logheads(self, op, **opts):
        heads = self.heads
        util.log(
            "visibility",
            "%s %d heads: %s%s\n",
            op,
            len(heads),
            ", ".join(node.short(h) for h in reversed(heads[-self.LOGHEADLIMIT :])),
            ", ..." if len(heads) > self.LOGHEADLIMIT else "",
            **opts,
        )

    def _logchange(self, oldheads, newheads):
        newheads = set(newheads)
        oldheads = set(oldheads)
        addedheads = newheads - oldheads
        removedheads = oldheads - newheads
        util.log(
            "visibility",
            "removed %s heads [%s]; added %s heads [%s]\n",
            len(removedheads),
            ", ".join(node.short(n) for n in removedheads),
            len(addedheads),
            ", ".join(node.short(n) for n in addedheads),
        )

    def _updateheads(self, repo, newheads, tr):
        newheads = list(newheads)
        # Remove heads that are not actually heads, and preserve the ordering
        # in self.heads for heads that have not changed.
        unfi = repo
        if len(newheads) > 1:
            cl = unfi.changelog
            hasnode = cl.hasnode
            realnewheads = list(
                cl.dag.headsancestors(
                    [h for h in newheads if hasnode(h) and h != node.nullid]
                )
            )
        else:
            realnewheads = list(newheads)
        realnewheadsset = set(realnewheads)
        oldheads = self.heads
        newheads = util.removeduplicates(
            [head for head in oldheads if head in realnewheadsset] + realnewheads
        )
        if oldheads != newheads:
            self._changecount += 1
            self._logchange(oldheads, newheads)
            self.heads = newheads
            self._invisiblerevs = None
            add = self._allheads.add
            for head in newheads:
                add(head)
            repo.invalidatevolatilesets()
            tr.addpostclose("log-visibility", self._logposttransaction)
            tr.addpostclose("allheads", lambda _tr: self._allheads.flush())

    def setvisibleheads(self, repo, newheads, tr):
        self._updateheads(repo, newheads, tr)

    def add(self, repo, newnodes, tr):
        newheads = set(self.heads).union(newnodes)
        self._updateheads(repo, newheads, tr)

    def remove(self, repo, oldnodes, tr):
        unfi = repo
        clrev = unfi.changelog.rev
        clparents = unfi.changelog.parents
        phasecache = unfi._phasecache
        newheads = set()
        candidates = set(self.heads)
        obsolete = set(repo.nodes("obsolete()"))
        oldnodes = set(oldnodes)

        from . import phases  # avoid circular import

        seen = set()
        while candidates:
            n = candidates.pop()
            if n in seen:
                continue
            seen.add(n)
            if n not in unfi or phasecache.phase(unfi, clrev(n)) == phases.public:
                pass
            elif n in oldnodes:
                for p in clparents(n):
                    if n != node.nullid:
                        candidates.add(p)
                        # If the parent node is already obsolete, also remove
                        # it from the visible set.
                        if p in obsolete:
                            oldnodes.add(p)
            else:
                newheads.add(n)
        self._updateheads(repo, newheads, tr)

    def allheads(self):
        """get all heads ever tracked as "visibleheads".

        Extra filtering is needed to make sure they are actually heads.
        """
        return self._allheads.items()

    def phaseadjust(self, repo, tr, newdraft=None, newpublic=None):
        """update visibility following a phase adjustment.

        The newdraft commits should remain visible.  The newpublic commits
        can be removed, as public commits are always visible.
        """
        newheads = set(self.heads)
        if newpublic:
            newheads.difference_update(newpublic)
        if newdraft:
            newheads.update(newdraft)
        self._updateheads(repo, newheads, tr)

    def invisiblerevs(self, repo):
        if self._invisiblerevs is not None:
            return self._invisiblerevs

        if repo.ui.configbool("experimental", "narrow-heads"):
            # With narrow-heads, "draft()" lists all visible draft commits,
            # nothing needs to be filtered out.
            self._invisiblerevs = frozenset()
            return self._invisiblerevs

        from . import phases  # avoid circular import

        hidden = set(repo._phasecache.getrevset(repo, (phases.draft, phases.secret)))
        rfunc = repo.changelog.rev
        pfunc = repo.changelog.parentrevs
        hasnode = repo.changelog.nodemap.__contains__
        visible = [rfunc(n) for n in self.heads if hasnode(n)]
        hidden.difference_update(visible)
        while visible:
            for p in pfunc(visible.pop()):
                if p != node.nullrev and p in hidden:
                    hidden.remove(p)
                    visible.append(p)

        self._invisiblerevs = hidden
        return hidden


class bundlevisibleheads(visibleheads):
    """specialization of visibleheads for bundlerepos

    In bundlerepos, all the bundle commits are visible, so the bundleheads
    are added to the set of visible heads.
    """

    def addbundleheads(self, bundleheads):
        # Some of the bundleheads may be descendants of the visible heads.
        # There shouldn't be too much overlap, so we ignore this and just add
        # the bundle heads as additional visible heads.
        self.heads = util.removeduplicates(self.heads + bundleheads)

    def _updateheads(self, repo, newheads, tr):
        # Accept the new heads as-is.
        self.heads = newheads


def setvisibleheads(repo, newheads) -> None:
    """set the visible heads

    Updates the set of visible mutable heads to be exactly those specified.
    """
    if tracking(repo):
        with repo.lock(), repo.transaction("update-visibility") as tr:
            repo.changelog._visibleheads.setvisibleheads(repo, newheads, tr)


def add(repo, newnodes) -> None:
    """add nodes to the visible set

    Adds the given nodes to the set of visible nodes.  This includes any
    ancestors of the commits that are not currently visible.
    """
    if tracking(repo):
        with repo.lock(), repo.transaction("update-visibility") as tr:
            repo.changelog._visibleheads.add(repo, newnodes, tr)


def remove(repo, oldnodes) -> None:
    """remove nodes from the visible set

    Removes the given nodes from the set of visible nodes.  If any of the nodes
    have any visible descendents, then those nodes are *not* removed.  That is,
    the removed nodes must be head nodes, or ancestors of other nodes that are
    being removed together.

    If removal of the nodes causes any obsolete ancestors to become head nodes,
    those obsolete ancestors are also removed.  This means given a situation
    like the following:

       o D'
       |
       o B
       |
       | o D
       | |
       | x C
       |/
       o A

    If D is being rebased to D', `visibility.remove(D)` will cause both D and C
    to be removed from the visible set.
    """
    if tracking(repo):
        with repo.lock(), repo.transaction("update-visibility") as tr:
            repo.changelog._visibleheads.remove(repo, oldnodes, tr)


def phaseadjust(repo, tr, newdraft=None, newpublic=None) -> None:
    """adjust the phase of visible nodes

    Visibility tracking only cares about non public commits.  If a commit
    transisitions between draft and public, this function must be called to
    update the accounting.

    Nodes that were draft and are now public must be provided in the
    ``newpublic`` list.  Nodes that were public and are now draft must be
    provided in the ``newdraft`` list.
    """
    if tracking(repo):
        repo.changelog._visibleheads.phaseadjust(repo, tr, newdraft, newpublic)


def heads(repo):
    """returns the current set of visible mutable heads"""
    if tracking(repo):
        return repo.changelog._visibleheads.heads


def invisiblerevs(repo):
    """returns the invisible mutable revs in this repo"""
    if tracking(repo):
        return repo.changelog._visibleheads.invisiblerevs(repo)


def tracking(repo):
    """returns true if this repo is explicitly tracking visible mutable heads"""
    return "visibleheads" in repo.storerequirements


def enabled(repo):
    """returns true if this repo is using visibleheads to determine visibility"""
    return tracking(repo) and repo.ui.configbool("visibility", "enabled")


def automigrate(repo) -> None:
    desired = repo.ui.configbool("visibility", "enabled")
    actual = tracking(repo)
    if desired and not actual:
        if repo.ui.configbool("visibility", "verbose"):
            repo.ui.status(_("switching to explicit tracking of visible commits\n"))
        starttracking(repo)
    if not desired and actual:
        if repo.ui.configbool("visibility", "verbose"):
            repo.ui.status(_("reverting to tracking visibility through obsmarkers\n"))
        stoptracking(repo)
