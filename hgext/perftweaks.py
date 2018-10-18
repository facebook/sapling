# perftweaks.py
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""extension for tweaking Mercurial features to improve performance.

::
    [perftweaks]
    # Whether to use faster hidden cache. It has faster cache hash calculation
    # which only check stat of a few files inside store/ directory.
    fasthiddencache = False
"""

import errno
import os

from mercurial import (
    branchmap,
    dispatch,
    extensions,
    localrepo,
    merge,
    namespaces,
    phases,
    scmutil,
    tags,
    util,
)
from mercurial.extensions import wrapfunction
from mercurial.i18n import _
from mercurial.node import bin


testedwith = "ships-with-fb-hgext"


def extsetup(ui):
    wrapfunction(branchmap.branchcache, "update", _branchmapupdate)
    wrapfunction(namespaces.namespaces, "singlenode", _singlenode)


def reposetup(ui, repo):
    if repo.local() is not None:
        # developer config: perftweaks.disableupdatebranchcacheoncommit
        if repo.ui.configbool("perftweaks", "disableupdatebranchcacheoncommit"):

            class perftweaksrepo(repo.__class__):
                @localrepo.unfilteredmethod
                def updatecaches(self, tr=None):
                    # Disable "branchmap.updatecache(self.filtered('served'))"
                    # code path guarded by "if tr.changes['revs']". First, we
                    # don't have on-disk branchmap. Second, accessing
                    # "repo.filtered('served')" alone is not very cheap.
                    bakrevs = None
                    if tr and "revs" in tr.changes:
                        bakrevs = tr.changes["revs"]
                        tr.changes["revs"] = frozenset()
                    try:
                        super(perftweaksrepo, self).updatecaches(tr)
                    finally:
                        if bakrevs:
                            tr.changes["revs"] = bakrevs

            repo.__class__ = perftweaksrepo

        # record nodemap lag
        try:
            lag = repo.changelog.nodemap.lag
            ui.log("nodemap_lag", "", nodemap_lag=lag)
        except AttributeError:
            pass


def _singlenode(orig, self, repo, name):
    """Skips reading branches namespace if unnecessary"""
    # developer config: perftweaks.disableresolvingbranches
    if not repo.ui.configbool("perftweaks", "disableresolvingbranches"):
        return orig(self, repo, name)

    # If branches are disabled, only resolve the 'default' branch. Loading
    # 'branches' is O(len(changelog)) time complexity because it calls
    # headrevs() which scans the entire changelog.
    names = self._names
    namesbak = names.copy()
    if name != "default" and "branches" in names:
        del names["branches"]
    try:
        return orig(self, repo, name)
    finally:
        self.names = namesbak


def _branchmapupdate(orig, self, repo, revgen):
    if not repo.ui.configbool("perftweaks", "disablebranchcache"):
        return orig(self, repo, revgen)

    cl = repo.changelog
    tonode = cl.node

    if self.tiprev == len(cl) - 1 and self.validfor(repo):
        return

    # Since we have no branches, the default branch heads are equal to
    # cl.headrevs(). Note: cl.headrevs() is already sorted and it may return
    # -1.
    branchheads = [i for i in cl.headrevs() if i >= 0]

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
    repo.ui.log("branchcache", "perftweaks updated %s branch cache\n", repo.filtername)
