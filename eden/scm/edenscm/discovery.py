# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# discovery.py - protocol changeset discovery functions
#
# Copyright 2010 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from . import bookmarks, setdiscovery, util
from .node import nullid


def findcommonincoming(repo, remote, heads=None, force=False, ancestorsof=None):
    """Return a tuple (common, anyincoming, heads) used to identify the common
    subset of nodes between repo and remote.

    "common" is a list of (at least) the heads of the common subset.
    "anyincoming" is testable as a boolean indicating if any nodes are missing
      locally. If remote does not support getbundle, this actually is a list of
      roots of the nodes that would be incoming, to be supplied to
      changegroupsubset. No code except for pull should be relying on this fact
      any longer.
    "heads" is either the supplied heads, or else the remote's heads.
    "ancestorsof" if not None, restrict the discovery to a subset defined by
      these nodes. Changeset outside of this set won't be considered (and
      won't appears in "common")

    If you pass heads and they are all known locally, the response lists just
    these heads in "common" and in "heads".

    Please use findcommonoutgoing to compute the set of outgoing nodes to give
    extensions a good hook into outgoing.
    """

    if heads:
        allknown = True
        knownnode = repo.changelog.hasnode  # no nodemap until it is filtered
        for h in heads:
            if not knownnode(h):
                allknown = False
                break
        if allknown:
            return (heads, False, heads)

    res = setdiscovery.findcommonheads(
        repo.ui,
        repo,
        remote,
        abortwhenunrelated=not force,
        ancestorsof=ancestorsof,
        explicitremoteheads=heads,
    )
    common, anyinc, srvheads = res
    unfi = repo
    # anyinc = True prints "no changes found". However that is not always
    # true if heads is provided. Do a double check.
    if anyinc is False and heads and any(head not in unfi for head in heads):
        anyinc = True
    return (list(common), anyinc, heads or list(srvheads))


class outgoing(object):
    """Represents the set of nodes present in a local repo but not in a
    (possibly) remote one.

    Members:

      missing is a list of all nodes present in local but not in remote.
      common is a list of all nodes shared between the two repos.
      excluded is the list of missing changeset that shouldn't be sent remotely.
      missingheads is the list of heads of missing.
      commonheads is the list of heads of common.

    The sets are computed on demand from the heads, unless provided upfront
    by discovery."""

    def __init__(self, repo, commonheads=None, missingheads=None, missingroots=None):
        # at least one of them must not be set
        assert None in (commonheads, missingroots)
        cl = repo.changelog
        if missingheads is None:
            missingheads = repo.heads()
        if missingroots:
            discbases = []
            for n in missingroots:
                discbases.extend([p for p in cl.parents(n) if p != nullid])
            # TODO remove call to nodesbetween.
            # TODO populate attributes on outgoing instance instead of setting
            # discbases.
            csets, roots, heads = cl.nodesbetween(missingroots, missingheads)
            included = set(csets)
            missingheads = heads
            commonheads = [n for n in discbases if n not in included]
        elif not commonheads:
            commonheads = [nullid]
        self.commonheads = commonheads
        self.missingheads = missingheads
        self._revlog = cl
        self._common = None
        self._missing = None
        self.excluded = []

    def _computecommonmissing(self):
        sets = self._revlog.findcommonmissing(self.commonheads, self.missingheads)
        self._common, self._missing = sets

    @util.propertycache
    def common(self):
        if self._common is None:
            self._computecommonmissing()
        return self._common

    @util.propertycache
    def missing(self):
        if self._missing is None:
            self._computecommonmissing()
        return self._missing


def findcommonoutgoing(
    repo, other, onlyheads=None, force=False, commoninc=None, portable=False
):
    """Return an outgoing instance to identify the nodes present in repo but
    not in other.

    If onlyheads is given, only nodes ancestral to nodes in onlyheads
    (inclusive) are included. If you already know the local repo's heads,
    passing them in onlyheads is faster than letting them be recomputed here.

    If commoninc is given, it must be the result of a prior call to
    findcommonincoming(repo, other, force) to avoid recomputing it here.

    If portable is given, compute more conservative common and missingheads,
    to make bundles created from the instance more portable."""
    # declare an empty outgoing object to be filled later
    og = outgoing(repo, None, None)

    # get common set if not provided
    if commoninc is None:
        commoninc = findcommonincoming(repo, other, force=force, ancestorsof=onlyheads)
    og.commonheads, _any, _hds = commoninc

    # compute outgoing
    og.missingheads = onlyheads or repo.heads()
    if portable:
        # recompute common and missingheads as if -r<rev> had been given for
        # each head of missing, and --base <rev> for each head of the proper
        # ancestors of missing
        og._computecommonmissing()
        cl = repo.changelog
        missingrevs = set(cl.rev(n) for n in og._missing)
        og._common = set(cl.ancestors(missingrevs)) - missingrevs
        commonheads = set(og.commonheads)
        og.missingheads = [h for h in og.missingheads if h not in commonheads]

    return og


def _nowarnheads(pushop):
    # Compute newly pushed bookmarks. We don't warn about bookmarked heads.
    repo = pushop.repo
    remote = pushop.remote
    localbookmarks = repo._bookmarks
    remotebookmarks = remote.listkeys("bookmarks")
    bookmarkedheads = set()

    # internal config: bookmarks.pushing
    newbookmarks = [
        localbookmarks.expandname(b)
        for b in pushop.ui.configlist("bookmarks", "pushing")
    ]

    for bm in localbookmarks:
        rnode = remotebookmarks.get(bm)
        if rnode and rnode in repo:
            lctx, rctx = repo[bm], repo[rnode]
            if bookmarks.validdest(repo, rctx, lctx):
                bookmarkedheads.add(lctx.node())
        else:
            if bm in newbookmarks and bm not in remotebookmarks:
                bookmarkedheads.add(repo[bm].node())

    return bookmarkedheads
