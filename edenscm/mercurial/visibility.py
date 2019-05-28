# visibility.py - tracking visibility through visible heads
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno

from edenscm.mercurial import error, node, util
from edenscm.mercurial.i18n import _


def _convertfromobsolete(repo):
    """convert obsolete markers into a set of visible heads"""
    with repo.ui.configoverride(
        {("mutation", "enabled"): False, ("visibility", "enabled"): False},
        "convertfromobsolete",
    ):
        return list(repo.unfiltered().nodes("heads((not public()) - hidden())"))


def starttracking(repo):
    """start tracking visibility information through visible mutable heads"""
    if "visibleheads" not in repo.storerequirements:
        with repo.lock():
            repo.storerequirements.add("visibleheads")
            setvisibleheads(repo, _convertfromobsolete(repo))
            repo._writestorerequirements()


def stoptracking(repo):
    """stop tracking visibility information and revert to using obsmarkers"""
    if "visibleheads" in repo.storerequirements:
        with repo.lock():
            repo.storerequirements.discard("visibleheads")
            repo._writestorerequirements()
    if repo.svfs.lexists("visibleheads"):
        repo.svfs.tryunlink("visibleheads")


# Supported file format version.
# Version 1 is:
#  * A single line containing "v1"
#  * A list of node hashes for each visible head, one per line.
FORMAT_VERSION = "v1"


class visibleheads(object):
    """tracks visible non-public heads in the repostory

    Track visibility of non-public commits through a set of heads.  This only
    covers non-public (draft and secret) commits - public commits are always
    visible.

    This class is responsible for tracking the set of heads, and persisting
    them to the store.
    """

    LOGHEADLIMIT = 4

    def __init__(self, vfs):
        self.vfs = vfs
        self._invisiblerevs = None
        try:
            lines = self.vfs("visibleheads").readlines()
            if not lines or lines[0].strip() != FORMAT_VERSION:
                raise error.Abort("invalid visibleheads file format")
            self.heads = [node.bin(head.strip()) for head in lines[1:]]
            self.dirty = False
            self._logheads("read", visibility_headcount=len(self.heads))
        except IOError as err:
            if err.errno != errno.ENOENT:
                raise
            self.heads = []
            self.dirty = True

    def _write(self, fp):
        fp.write("%s\n" % FORMAT_VERSION)
        for h in self.heads:
            fp.write("%s\n" % (node.hex(h),))
        self.dirty = False
        self._logheads("wrote", visibility_newheadcount=len(self.heads))

    def _logheads(self, op, **opts):
        util.log(
            "visibility",
            "%s %d heads: %s%s\n",
            op,
            len(self.heads),
            ", ".join(
                node.short(h) for h in reversed(self.heads[-self.LOGHEADLIMIT :])
            ),
            ", ..." if len(self.heads) > self.LOGHEADLIMIT else "",
            **opts
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
        unfi = repo.unfiltered()
        realnewheads = list(unfi.nodes("heads(%ln::%ln)", newheads, newheads))
        realnewheadsset = set(realnewheads)
        newheads = util.removeduplicates(
            [head for head in self.heads if head in realnewheadsset] + realnewheads
        )
        if self.heads != newheads:
            self._logchange(self.heads, newheads)
            self.heads = newheads
            self.dirty = True
            self._invisiblerevs = None
            repo.invalidatevolatilesets()
        if self.dirty:
            tr.addfilegenerator("visibility", ("visibleheads",), self._write)

    def setvisibleheads(self, repo, newheads, tr):
        self._updateheads(repo, newheads, tr)

    def add(self, repo, newnodes, tr):
        newheads = set(self.heads).union(newnodes)
        self._updateheads(repo, newheads, tr)

    def remove(self, repo, oldnodes, tr):
        unfi = repo.unfiltered()
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

        from . import phases  # avoid circular import

        hidden = set(repo._phasecache.getrevset(repo, (phases.draft, phases.secret)))
        rfunc = repo.changelog.rev
        pfunc = repo.changelog.parentrevs
        visible = [rfunc(n) for n in self.heads]
        hidden.difference_update(visible)
        while visible:
            for p in pfunc(visible.pop()):
                if p != node.nullrev and p in hidden:
                    hidden.remove(p)
                    visible.append(p)
        self._invisiblerevs = hidden
        return hidden


def setvisibleheads(repo, newheads):
    """set the visible heads

    Updates the set of visible mutable heads to be exactly those specified.
    """
    if tracking(repo):
        with repo.lock(), repo.transaction("update-visibility") as tr:
            repo.changelog._visibleheads.setvisibleheads(repo, newheads, tr)


def add(repo, newnodes):
    """add nodes to the visible set

    Adds the given nodes to the set of visible nodes.  This includes any
    ancestors of the commits that are not currently visible.
    """
    if tracking(repo):
        with repo.lock(), repo.transaction("update-visibility") as tr:
            repo.changelog._visibleheads.add(repo, newnodes, tr)


def remove(repo, oldnodes):
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


def phaseadjust(repo, tr, newdraft=None, newpublic=None):
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
    # TODO(mbthomas): support bundlerepo
    from . import bundlerepo  # avoid import cycle

    if isinstance(repo, bundlerepo.bundlerepository):
        return False
    return tracking(repo) and repo.ui.configbool("visibility", "enabled")


def automigrate(repo):
    mode = repo.ui.config("visibility", "automigrate")
    if mode == "start" and "visibleheads" not in repo.storerequirements:
        repo.ui.status(_("switching to explicit tracking of visible commits\n"))
        starttracking(repo)
    if mode == "stop" and "visibleheads" in repo.storerequirements:
        repo.ui.status(_("reverting to tracking visibility through obsmarkers\n"))
        stoptracking(repo)
