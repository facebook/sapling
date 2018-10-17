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
    # developer config: extensions.clindex
    if ui.config("extensions", "clindex") == "":
        # disable cachenoderevs if clindex is enabled.
        ui.setconfig("perftweaks", "cachenoderevs", "false", "perftweaks")
    wrapfunction(branchmap.branchcache, "update", _branchmapupdate)
    wrapfunction(branchmap, "read", _branchmapread)
    wrapfunction(branchmap, "replacecache", _branchmapreplacecache)
    wrapfunction(branchmap, "updatecache", _branchmapupdatecache)

    try:
        rebase = extensions.find("rebase")
        wrapfunction(rebase.rebaseruntime, "_preparenewrebase", _trackrebasesize)
    except KeyError:
        pass

    # noderev cache creation
    # The node rev cache is a cache of rev numbers that we are likely to do a
    # node->rev lookup for. Since looking up rev->node is cheaper than
    # node->rev, we use this cache to prefill the changelog radix tree with
    # mappings.
    wrapfunction(branchmap.branchcache, "write", _branchmapwrite)
    wrapfunction(phases.phasecache, "advanceboundary", _editphases)
    wrapfunction(phases.phasecache, "retractboundary", _editphases)
    try:
        remotenames = extensions.find("remotenames")
        wrapfunction(remotenames, "saveremotenames", _saveremotenames)
    except KeyError:
        pass

    wrapfunction(namespaces.namespaces, "singlenode", _singlenode)


def reposetup(ui, repo):
    if repo.local() is not None:
        _preloadrevs(repo)

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


def _branchmapread(orig, repo):
    # developer config: perftweaks.disablebranchcache2
    if not repo.ui.configbool("perftweaks", "disablebranchcache2"):
        return orig(repo)
    # Don't bother reading branchmap since branchcache.update() will be called
    # anyway and that is O(changelog)


def _branchmapreplacecache(orig, repo, bm):
    if not repo.ui.configbool("perftweaks", "disablebranchcache2"):
        return orig(repo, bm)
    # Don't bother writing branchmap since we don't read it


def _branchmapupdatecache(orig, repo):
    if not repo.ui.configbool("perftweaks", "disablebranchcache2"):
        return orig(repo)

    # The original logic has unnecessary steps, ex. it calculates the "served"
    # repoview as an attempt to build branchcache for "visible". And then
    # calculates "immutable" for calculating "served", recursively.
    #
    # Just use a shortcut path that construct the branchcache directly.
    partial = repo._branchcaches.get(repo.filtername)
    if partial is None:
        partial = branchmap.branchcache()
    partial.update(repo, None)
    repo._branchcaches[repo.filtername] = partial


def _branchmapwrite(orig, self, repo):
    if repo.ui.configbool("perftweaks", "disablebranchcache2"):
        # Since we don't read the branchcache, don't bother writing it.
        result = None
    else:
        result = orig(self, repo)
    if repo.ui.configbool("perftweaks", "cachenoderevs", True):
        revs = set()
        nodemap = repo.changelog.nodemap
        for branch, heads in self.iteritems():
            revs.update(nodemap[n] for n in heads)
        name = "branchheads-%s" % repo.filtername
        _savepreloadrevs(repo, name, revs)
    return result


def _saveremotenames(orig, repo, remotepath, branches=None, bookmarks=None):
    result = orig(repo, remotepath, branches=branches, bookmarks=bookmarks)
    if repo.ui.configbool("perftweaks", "cachenoderevs", True):
        revs = set()
        nodemap = repo.changelog.nodemap
        if bookmarks:
            for b, n in bookmarks.iteritems():
                n = bin(n)
                # remotenames can pass bookmarks that don't exist in the
                # changelog yet. It filters them internally, but we need to as
                # well.
                if n in nodemap:
                    revs.add(nodemap[n])
        if branches:
            for branch, nodes in branches.iteritems():
                for n in nodes:
                    if n in nodemap:
                        revs.add(nodemap[n])

        name = "remotenames-%s" % remotepath
        _savepreloadrevs(repo, name, revs)

    return result


def _editphases(orig, self, repo, tr, *args):
    result = orig(self, repo, tr, *args)

    def _write(fp):
        revs = set()
        nodemap = repo.changelog.nodemap
        for phase, roots in enumerate(self.phaseroots):
            for n in roots:
                if n in nodemap:
                    revs.add(nodemap[n])
        _savepreloadrevs(repo, "phaseroots", revs)

    # We don't actually use the transaction file generator. It's just a hook so
    # we can write out at the same time as phases.
    if tr:
        tr.addfilegenerator("noderev-phaseroot", ("phaseroots-fake",), _write)
    else:
        # fp is not used anyway
        _write(fp=None)

    return result


def _cachefilename(name):
    return "noderevs/%s" % name


def _preloadrevs(repo):
    # Preloading the node-rev map for likely to be used revs saves 100ms on
    # every command. This is because normally to look up a node, hg has to scan
    # the changelog.i file backwards, potentially reading through hundreds of
    # thousands of entries and building a cache of them.  Looking up a rev
    # however is fast, because we know exactly what offset in the file to read.
    # Reading old commits is common, since the branchmap needs to to convert old
    # branch heads from node to rev.

    if repo.ui.configbool("perftweaks", "cachenoderevs", True):
        repo = repo.unfiltered()
        revs = set()
        cachedir = repo.localvfs.join("cache", "noderevs")
        try:
            for cachefile in os.listdir(cachedir):
                filename = _cachefilename(cachefile)
                revs.update(int(r) for r in repo.cachevfs(filename))

            getnode = repo.changelog.node
            nodemap = repo.changelog.nodemap
            for r in revs:
                try:
                    node = getnode(r)
                    nodemap[node] = r
                except (IndexError, ValueError):
                    # Rev no longer exists or rev is out of range
                    pass
        except EnvironmentError:
            # No permission to read? No big deal
            pass


def _savepreloadrevs(repo, name, revs):
    if repo.ui.configbool("perftweaks", "cachenoderevs", True):
        cachedir = repo.localvfs.join("cache", "noderevs")
        try:
            repo.localvfs.mkdir(cachedir)
        except OSError as ex:
            # If we failed because the directory already exists,
            # continue.  In all other cases (e.g., no permission to create the
            # directory), just silently return without doing anything.
            if ex.errno != errno.EEXIST:
                return

        try:
            filename = _cachefilename(name)
            f = repo.cachevfs.open(filename, mode="w+", atomictemp=True)
            f.write("\n".join(str(r) for r in revs))
            f.close()
        except EnvironmentError:
            # No permission to write? No big deal
            pass


def _trackrebasesize(orig, self, destmap):
    result = orig(self, destmap)
    if not destmap:
        return result

    # The code assumes the rebase source is roughly a linear stack within a
    # single feature branch, and there is only one destination. If that is not
    # the case, the distance might be not accurate.
    repo = self.repo
    destrev = max(destmap.values())
    rebaseset = destmap.keys()
    commitcount = len(rebaseset)
    distance = len(
        repo.revs("(%ld %% %d) + (%d %% %ld)", rebaseset, destrev, destrev, rebaseset)
    )
    # 'distance' includes the commits being rebased, so subtract them to get the
    # actual distance being traveled. Even though we log update_distance above,
    # a rebase may run multiple updates, so that value might be not be accurate.
    repo.ui.log(
        "rebase_size",
        "",
        rebase_commitcount=commitcount,
        rebase_distance=distance - commitcount,
    )

    return result
