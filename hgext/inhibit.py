# inhibit.py - redefine obsolete(), bumped(), divergent() revsets
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""redefine obsolete(), bumped(), divergent() revsets"""

from __future__ import absolute_import

from mercurial import error, extensions, obsolete, util


def _obsoletedrevs(repo):
    """Redefine "obsolete()" revset. Previously, X is obsoleted if X appears as
    a predecessor in a marker. Now, X is obsoleted if X is a predecessor in
    marker M1, *and* is not a successor in marker M2 where M2.date >= M1.date.

    This allows undo to return to old hashes, and is correct as long as
    obsmarker is not exchanged.
    """
    getnode = repo.changelog.node
    markersbysuccessor = repo.obsstore.predecessors.get
    markersbypredecessor = repo.obsstore.successors.get
    result = set()
    for r in obsolete._mutablerevs(repo):
        n = getnode(r)
        m1s = markersbypredecessor(n)
        m2s = markersbysuccessor(n)
        if m1s:
            if m2s:
                # marker: (prec, [succ], flag, meta, (date, timezone), parent)
                d1 = max(m[4][0] for m in m1s)
                d2 = max(m[4][0] for m in m2s)
                if d2 < d1:
                    result.add(r)
            else:
                result.add(r)
    return result


def _obsstorecreate(
    orig,
    self,
    tr,
    prec,
    succs=(),
    flag=0,
    parents=None,
    date=None,
    metadata=None,
    ui=None,
):
    # we need to resolve default date
    if date is None:
        if ui is not None:
            date = ui.configdate("devel", "default-date")
        if date is None:
            date = util.makedate()
    # if prec is a successor of an existing marker, make default date bigger so
    # the old marker won't revive the predecessor accidentally. This helps tests
    # where date are always (0, 0)
    markers = self.predecessors.get(prec)
    if markers:
        maxdate = max(m[4] for m in markers)
        maxdate = (maxdate[0] + 1, maxdate[1])
        if maxdate > date:
            date = maxdate
    return orig(self, tr, prec, succs, flag, parents, date, metadata, ui)


def _createmarkers(orig, repo, rels, *args, **kwargs):
    # make predecessor context unfiltered so parents() won't raise
    unfi = repo.unfiltered()
    rels = [list(r) for r in rels]  # make mutable
    for r in rels:
        try:
            r[0] = unfi[r[0].node()]
        except error.RepoLookupError:
            # node could be unknown in current repo
            pass
    return orig(unfi, rels, *args, **kwargs)


def revive(ctxlist, operation="revive"):
    """un-obsolete revisions (public API used by other extensions)"""
    rels = [(ctx, (ctx,)) for ctx in ctxlist if ctx.obsolete()]
    if not rels:
        return
    # revive it by creating a self cycle marker
    repo = rels[0][0].repo()
    with repo.lock():
        obsolete.createmarkers(repo, rels, operation=operation)


def uisetup(ui):
    revsets = obsolete.cachefuncs

    # redefine obsolete(): handle cycles and make nodes visible
    revsets["obsolete"] = _obsoletedrevs

    # make divergent() and bumped() empty
    # NOTE: we should avoid doing this but just change templates to only show a
    # subset of troubles we care about.
    revsets["divergent"] = revsets["bumped"] = lambda repo: frozenset()

    # make obsstore.create not complain about in-marker cycles, since we want
    # to write X -> X to revive X.
    extensions.wrapfunction(obsolete.obsstore, "create", _obsstorecreate)

    # make createmarkers use unfiltered predecessor ctx, workarounds an issue
    # that prec.parents() may raise FilteredIndexError.
    # NOTE: should be fixed upstream once hash-preserving obsstore is a thing.
    extensions.wrapfunction(obsolete, "createmarkers", _createmarkers)
