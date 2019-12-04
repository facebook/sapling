# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# branchmap.py - logic to computes, maintain and stores branchmap for local repo
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from . import scmutil
from .node import nullid, nullrev


def updatecache(repo):
    # Don't write the branchmap if it's disabled.
    # The original logic has unnecessary steps, ex. it calculates the "served"
    # repoview as an attempt to build branchcache for "visible". And then
    # calculates "immutable" for calculating "served", recursively.
    #
    # Just use a shortcut path that construct the branchcache directly.
    partial = repo._branchcaches.get(repo.filtername)
    if partial is None:
        partial = branchcache()
    partial.update(repo, None)
    repo._branchcaches[repo.filtername] = partial


class branchcache(dict):
    """A dict like object that hold branches heads cache.

    This cache is used to avoid costly computations to determine all the
    branch heads of a repo.

    The cache is serialized on disk in the following format:

    <tip hex node> <tip rev number> [optional filtered repo hex hash]
    <branch head hex node> <open/closed state> <branch name>
    <branch head hex node> <open/closed state> <branch name>
    ...

    The first line is used to check if the cache is still valid. If the
    branch cache is for a filtered repo view, an optional third hash is
    included that hashes the hashes of all filtered revisions.

    The open/closed state is represented by a single letter 'o' or 'c'.
    This field can be used to avoid changelog reads when determining if a
    branch head closes a branch or not.
    """

    def __init__(
        self,
        entries=(),
        tipnode=nullid,
        tiprev=nullrev,
        filteredhash=None,
        closednodes=None,
    ):
        super(branchcache, self).__init__(entries)
        self.tipnode = tipnode
        self.tiprev = tiprev
        self.filteredhash = filteredhash
        # closednodes is a set of nodes that close their branch. If the branch
        # cache has been updated, it may contain nodes that are no longer
        # heads.
        if closednodes is None:
            self._closednodes = set()
        else:
            self._closednodes = closednodes

    def validfor(self, repo):
        """Is the cache content valid regarding a repo

        - False when cached tipnode is unknown or if we detect a strip.
        - True when cache is up to date or a subset of current repo."""
        try:
            return (self.tipnode == repo.changelog.node(self.tiprev)) and (
                self.filteredhash == scmutil.filteredhash(repo, self.tiprev)
            )
        except IndexError:
            return False

    def _branchtip(self, heads):
        """Return tuple with last open head in heads and false,
        otherwise return last closed head and true."""
        tip = heads[-1]
        closed = True
        for h in reversed(heads):
            if h not in self._closednodes:
                tip = h
                closed = False
                break
        return tip, closed

    def branchtip(self, branch):
        """Return the tipmost open head on branch head, otherwise return the
        tipmost closed head on branch.
        Raise KeyError for unknown branch."""
        return self._branchtip(self[branch])[0]

    def iteropen(self, nodes):
        return (n for n in nodes if n not in self._closednodes)

    def branchheads(self, branch, closed=False):
        heads = self[branch]
        if not closed:
            heads = list(self.iteropen(heads))
        return heads

    def iterbranches(self):
        for bn, heads in self.iteritems():
            yield (bn, heads) + self._branchtip(heads)

    # pyre-fixme[15]: `copy` overrides method defined in `dict` inconsistently.
    def copy(self):
        """return an deep copy of the branchcache object"""
        return branchcache(
            self, self.tipnode, self.tiprev, self.filteredhash, self._closednodes
        )

    def update(self, repo, revgen):
        """Given a branchhead cache, self, that may have extra nodes or be
        missing heads, and a generator of nodes that are strictly a superset of
        heads missing, this function updates self to be correct.
        """
        # Behave differently if the cache is disabled.
        cl = repo.changelog
        tonode = cl.node

        if self.tiprev == len(cl) - 1 and self.validfor(repo):
            return

        # Since we have no branches, the default branch heads are equal to
        # repo.headrevs(). Note: repo.headrevs() is already sorted and it may
        # return -1.
        branchheads = [i for i in repo.headrevs(reverse=False) if i >= 0]

        if not branchheads:
            if "default" in self:
                del self["default"]
            tiprev = -1
        else:
            self["default"] = [tonode(rev) for rev in branchheads]
            tiprev = branchheads[-1]
        self.tipnode = cl.node(tiprev)
        self.tiprev = tiprev
        self.filteredhash = scmutil.filteredhash(repo, self.tiprev)
        repo.ui.log(
            "branchcache", "perftweaks updated %s branch cache\n", repo.filtername
        )
