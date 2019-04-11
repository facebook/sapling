# visibility.py - tracking visibility through visible heads
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno

from edenscm.mercurial import error, node


def _convertfromobsolete(repo):
    """convert obsolete markers into a set of visible heads"""
    with repo.ui.configoverride(
        {("mutation", "enabled"): False, ("visibility", "enabled"): False},
        "convertfromobsolete",
    ):
        return set(repo.unfiltered().nodes("heads((not public()) - hidden())"))


def starttracking(repo):
    if "visibleheads" not in repo.storerequirements:
        with repo.lock():
            repo.storerequirements.add("visibleheads")
            repo._writestorerequirements()


def stoptracking(repo):
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

    def __init__(self, ui, repo):
        self.ui = ui
        self.vfs = repo.svfs
        self._invisiblerevs = None
        try:
            lines = self.vfs("visibleheads").readlines()
            if not lines or lines[0].strip() != FORMAT_VERSION:
                raise error.Abort("invalid visibleheads file format")
            self.heads = set(node.bin(head.strip()) for head in lines[1:])
            self.dirty = False
        except IOError as err:
            if err.errno != errno.ENOENT:
                raise
            self.heads = _convertfromobsolete(repo)
            self.dirty = True

    def _write(self, fp):
        fp.write("%s\n" % FORMAT_VERSION)
        for h in self.heads:
            fp.write("%s\n" % (node.hex(h),))
        self.dirty = False

    def _updateheads(self, repo, newheads, tr):
        newheads = set(newheads)
        if self.heads != newheads:
            self.heads = newheads
            self.dirty = True
            self._invisiblerevs = None
            repo.invalidatevolatilesets()
        if self.dirty:
            tr.addfilegenerator("visibility", ("visibleheads",), self._write)

    def add(self, repo, newnodes, tr):
        unfi = repo.unfiltered()
        newheads = self.heads.union(newnodes)
        newheads = unfi.nodes("heads(%ln::%ln)", newheads, newheads)
        self._updateheads(repo, newheads, tr)

    def remove(self, repo, oldnodes, tr):
        unfi = repo.unfiltered()
        clrev = unfi.changelog.rev
        clparents = unfi.changelog.parents
        phasecache = unfi._phasecache
        newheads = set()
        candidates = self.heads.copy()
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
        newheads = unfi.nodes("heads(%ln::%ln)", newheads, newheads)
        self._updateheads(repo, newheads, tr)

    def phaseadjust(self, repo, tr, newdraft=None, newpublic=None):
        """update visibility following a phase adjustment.

        The newdraft commits should remain visible.  The newpublic commits
        can be removed, as public commits are always visible.
        """
        unfi = repo.unfiltered()
        newheads = self.heads.copy()
        if newpublic:
            newheads.difference_update(newpublic)
        if newdraft:
            newheads.update(newdraft)
        newheads = unfi.nodes("heads(%ln::%ln)", newheads, newheads)
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


def add(repo, newnodes):
    if tracking(repo):
        with repo.lock(), repo.transaction("update-visibility") as tr:
            repo._visibleheads.add(repo, newnodes, tr)


def remove(repo, oldnodes):
    if tracking(repo):
        with repo.lock(), repo.transaction("update-visibility") as tr:
            repo._visibleheads.remove(repo, oldnodes, tr)


def phaseadjust(repo, tr, newdraft=None, newpublic=None):
    if tracking(repo):
        repo._visibleheads.phaseadjust(repo, tr, newdraft, newpublic)


def heads(repo):
    if tracking(repo):
        return repo._visibleheads.heads


def invisiblerevs(repo):
    """Returns the invisible mutable revs in this repo"""
    if tracking(repo):
        return repo._visibleheads.invisiblerevs(repo)


def tracking(repo):
    return "visibleheads" in repo.storerequirements


def enabled(repo):
    # TODO(mbthomas): support bundlerepo
    from . import bundlerepo  # avoid import cycle

    if isinstance(repo, bundlerepo.bundlerepository):
        return False
    return tracking(repo) and repo.ui.configbool("visibility", "enabled")
