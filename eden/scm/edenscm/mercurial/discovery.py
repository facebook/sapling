# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# discovery.py - protocol changeset discovery functions
#
# Copyright 2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import functools

from . import bookmarks, branchmap, phases, setdiscovery, treediscovery, util
from .node import hex, nullid


def findcommonincoming(
    repo, remote, heads=None, force=False, ancestorsof=None, needlargestcommonset=True
):
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
    "needlargestcommonset" if set to True then it will return the largest set of common nodes.
    Otherwise heuristics can be used to speed up discovery but return a smaller
    common set.

    If you pass heads and they are all known locally, the response lists just
    these heads in "common" and in "heads".

    Please use findcommonoutgoing to compute the set of outgoing nodes to give
    extensions a good hook into outgoing.
    """

    if not remote.capable("getbundle"):
        return treediscovery.findcommonincoming(repo, remote, heads, force)

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
        needlargestcommonset=needlargestcommonset,
    )
    common, anyinc, srvheads = res
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
        commoninc = findcommonincoming(
            repo, other, force=force, ancestorsof=onlyheads, needlargestcommonset=True
        )
    og.commonheads, _any, _hds = commoninc

    # compute outgoing
    if repo.ui.configbool("experimental", "narrow-heads"):
        mayexclude = None
    else:
        mayexclude = repo._phasecache.phaseroots[phases.secret] or repo.obsstore
    if not mayexclude:
        og.missingheads = onlyheads or repo.heads()
    elif onlyheads is None:
        # use visible heads as it should be cached
        og.missingheads = repo.filtered("served").heads()
        og.excluded = [ctx.node() for ctx in repo.set("secret()")]
    else:
        # compute common, missing and exclude secret stuff
        sets = repo.changelog.findcommonmissing(og.commonheads, onlyheads)
        og._common, allmissing = sets
        og._missing = missing = []
        og.excluded = excluded = []
        for node in allmissing:
            ctx = repo[node]
            if ctx.phase() >= phases.secret:
                excluded.append(node)
            else:
                missing.append(node)
        if len(missing) == len(allmissing):
            missingheads = onlyheads
        else:  # update missing heads
            missingheads = phases.newheads(repo, onlyheads, excluded)
        og.missingheads = missingheads
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


def _headssummary(pushop):
    """compute a summary of branch and heads status before and after push

    return {'branch': ([remoteheads], [newheads],
                       [unsyncedheads], [discardedheads])} mapping

    - branch: the branch name,
    - remoteheads: the list of remote heads known locally
                   None if the branch is new,
    - newheads: the new remote heads (known locally) with outgoing pushed,
    - unsyncedheads: the list of remote heads unknown locally,
    - discardedheads: the list of heads made obsolete by the push.
    """
    repo = pushop.repo.unfiltered()
    remote = pushop.remote
    outgoing = pushop.outgoing
    cl = repo.changelog
    headssum = {}
    # A. Create set of branches involved in the push.
    branches = set(repo[n].branch() for n in outgoing.missing)
    remotemap = remote.branchmap()
    newbranches = branches - set(remotemap)
    branches.difference_update(newbranches)

    # A. register remote heads
    remotebranches = set()
    for branch, heads in remote.branchmap().iteritems():
        remotebranches.add(branch)
        known = []
        unsynced = []
        knownnode = cl.hasnode  # do not use nodemap until it is filtered
        for h in heads:
            if knownnode(h):
                known.append(h)
            else:
                unsynced.append(h)
        headssum[branch] = (known, list(known), unsynced)
    # B. add new branch data
    missingctx = list(repo[n] for n in outgoing.missing)
    touchedbranches = set()
    for ctx in missingctx:
        branch = ctx.branch()
        touchedbranches.add(branch)
        if branch not in headssum:
            headssum[branch] = (None, [], [])

    # C drop data about untouched branches:
    for branch in remotebranches - touchedbranches:
        del headssum[branch]

    # D. Update newmap with outgoing changes.
    # This will possibly add new heads and remove existing ones.
    newmap = branchmap.branchcache(
        (branch, heads[1])
        for branch, heads in headssum.iteritems()
        if heads[0] is not None
    )
    newmap.update(repo, (ctx.rev() for ctx in missingctx))
    for branch, newheads in newmap.iteritems():
        headssum[branch][1][:] = newheads
    for branch, items in headssum.iteritems():
        for l in items:
            if l is not None:
                l.sort()
        headssum[branch] = items + ([],)

    # If there are no obsstore, no post processing are needed.
    if repo.obsstore:
        torev = repo.changelog.rev
        futureheads = set(torev(h) for h in outgoing.missingheads)
        futureheads |= set(torev(h) for h in outgoing.commonheads)
        allfuturecommon = repo.changelog.ancestors(futureheads, inclusive=True)
        for branch, heads in sorted(headssum.iteritems()):
            remoteheads, newheads, unsyncedheads, placeholder = heads
            result = _postprocessobsolete(pushop, allfuturecommon, newheads)
            headssum[branch] = (
                remoteheads,
                sorted(result[0]),
                unsyncedheads,
                sorted(result[1]),
            )
    return headssum


def _oldheadssummary(repo, remoteheads, outgoing, inc=False):
    """Compute branchmapsummary for repo without branchmap support"""

    # 1-4b. old servers: Check for new topological heads.
    # Construct {old,new}map with branch = None (topological branch).
    # (code based on update)
    knownnode = repo.changelog.hasnode  # no nodemap until it is filtered
    oldheads = sorted(h for h in remoteheads if knownnode(h))
    # all nodes in outgoing.missing are children of either:
    # - an element of oldheads
    # - another element of outgoing.missing
    # - nullrev
    # This explains why the new head are very simple to compute.
    r = repo.set("heads(%ln + %ln)", oldheads, outgoing.missing)
    newheads = sorted(c.node() for c in r)
    # set some unsynced head to issue the "unsynced changes" warning
    if inc:
        unsynced = [None]
    else:
        unsynced = []
    return {None: (oldheads, newheads, unsynced, [])}


def _nowarnheads(pushop):
    # Compute newly pushed bookmarks. We don't warn about bookmarked heads.
    repo = pushop.repo.unfiltered()
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


def _postprocessobsolete(pushop, futurecommon, candidate_newhs):
    """post process the list of new heads with obsolescence information

    Exists as a sub-function to contain the complexity and allow extensions to
    experiment with smarter logic.

    Returns (newheads, discarded_heads) tuple
    """
    # known issue
    #
    # * We "silently" skip processing on all changeset unknown locally
    #
    # * if <nh> is public on the remote, it won't be affected by obsolete
    #     marker and a new is created

    # define various utilities and containers
    repo = pushop.repo
    unfi = repo.unfiltered()
    tonode = unfi.changelog.node
    torev = unfi.changelog.nodemap.get
    public = phases.public
    getphase = unfi._phasecache.phase
    ispublic = lambda r: getphase(unfi, r) == public
    ispushed = lambda n: torev(n) in futurecommon
    hasoutmarker = functools.partial(pushingmarkerfor, unfi.obsstore, ispushed)
    successorsmarkers = unfi.obsstore.successors
    newhs = set()  # final set of new heads
    discarded = set()  # new head of fully replaced branch

    localcandidate = set()  # candidate heads known locally
    unknownheads = set()  # candidate heads unknown locally
    for h in candidate_newhs:
        if h in unfi:
            localcandidate.add(h)
        else:
            if successorsmarkers.get(h) is not None:
                msg = (
                    "checkheads: remote head unknown locally has" " local marker: %s\n"
                )
                repo.ui.debug(msg % hex(h))
            unknownheads.add(h)

    # fast path the simple case
    if len(localcandidate) == 1:
        return unknownheads | set(candidate_newhs), set()

    # actually process branch replacement
    while localcandidate:
        nh = localcandidate.pop()
        # run this check early to skip the evaluation of the whole branch
        if torev(nh) in futurecommon or ispublic(torev(nh)):
            newhs.add(nh)
            continue

        # Get all revs/nodes on the branch exclusive to this head
        # (already filtered heads are "ignored"))
        branchrevs = unfi.revs("only(%n, (%ln+%ln))", nh, localcandidate, newhs)
        branchnodes = [tonode(r) for r in branchrevs]

        # The branch won't be hidden on the remote if
        # * any part of it is public,
        # * any part of it is considered part of the result by previous logic,
        # * if we have no markers to push to obsolete it.
        if (
            any(ispublic(r) for r in branchrevs)
            or any(torev(n) in futurecommon for n in branchnodes)
            or any(not hasoutmarker(n) for n in branchnodes)
        ):
            newhs.add(nh)
        else:
            # note: there is a corner case if there is a merge in the branch.
            # we might end up with -more- heads.  However, these heads are not
            # "added" by the push, but more by the "removal" on the remote so I
            # think is a okay to ignore them,
            discarded.add(nh)
    newhs |= unknownheads
    return newhs, discarded


def pushingmarkerfor(obsstore, ispushed, node):
    """true if some markers are to be pushed for node

    We cannot just look in to the pushed obsmarkers from the pushop because
    discovery might have filtered relevant markers. In addition listing all
    markers relevant to all changesets in the pushed set would be too expensive
    (O(len(repo)))

    (note: There are cache opportunity in this function. but it would requires
    a two dimensional stack.)
    """
    successorsmarkers = obsstore.successors
    stack = [node]
    seen = set(stack)
    while stack:
        current = stack.pop()
        if ispushed(current):
            return True
        markers = successorsmarkers.get(current, ())
        # markers fields = ('prec', 'succs', 'flag', 'meta', 'date', 'parents')
        for m in markers:
            nexts = m[1]  # successors
            if not nexts:  # this is a prune marker
                nexts = m[5] or ()  # parents
            for n in nexts:
                if n not in seen:
                    seen.add(n)
                    stack.append(n)
    return False
