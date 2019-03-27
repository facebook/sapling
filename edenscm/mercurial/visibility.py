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
        {("mutation", "enabled"): False}, "convertfromobsolete"
    ):
        return set(repo.unfiltered().nodes("heads((not public()) - obsolete())"))


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
            repo.filteredrevcache.clear()
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


def makevisibleheads(ui, repo):
    tracking = ui.config("visibility", "tracking", "auto")
    if tracking != "auto":
        trackingbool = util.parsebool(tracking)
        if trackingbool is None:
            raise error.ConfigError(
                _("visibility.tracking not valid ('%s' is not 'auto' or boolean)")
                % tracking
            )
        if trackingbool:
            if "visibleheads" not in repo.storerequirements:
                repo.storerequirements.add("visibleheads")
                with repo.lock():
                    repo._writestorerequirements()
        else:
            if "visibleheads" in repo.storerequirements:
                repo.storerequirements.remove("visibleheads")
                with repo.lock():
                    repo._writestorerequirements()
            if repo.svfs.lexists("visibleheads"):
                repo.svfs.tryunlink("visibleheads")
    if "visibleheads" in repo.storerequirements:
        return visibleheads(ui, repo)
    else:
        return None


def add(repo, newnodes):
    vh = repo._visibleheads
    if vh is not None:
        with repo.lock(), repo.transaction("update-visibility") as tr:
            vh.add(repo, newnodes, tr)


def remove(repo, oldnodes):
    vh = repo._visibleheads
    if vh is not None:
        with repo.lock(), repo.transaction("update-visibility") as tr:
            vh.remove(repo, oldnodes, tr)


def phaseadjust(repo, tr, newdraft=None, newpublic=None):
    vh = repo._visibleheads
    if vh is not None:
        vh.phaseadjust(repo, tr, newdraft, newpublic)


def heads(repo):
    vh = repo._visibleheads
    if vh is not None:
        return vh.heads
    return None


def invisiblerevs(repo):
    """Returns the invisible mutable revs in this repo"""
    vh = repo._visibleheads
    if vh is not None:
        return vh.invisiblerevs(repo)
    return None


def tracking(repo):
    return repo._visibleheads is not None


def enabled(repo):
    return tracking(repo) and not repo.ui.configbool("visibility", "forceobsolete")
