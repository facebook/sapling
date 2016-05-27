# cachemanager.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import os
import random

from mercurial import extensions, revlog, scmutil, util

from extutil import wrapfilecache

import concurrency
import constants
from implementation import fastmanifestcache

def _relevantremonamesrevs(repo):
    revs = set()
    remotenames = None
    try:
        remotenames = extensions.find('remotenames')
    except KeyError: # remotenames not loaded
        pass
    if remotenames is not None:
        # interesting remotenames to fetch
        relevantnames = set(repo.ui.configlist("fastmanifest",
                                               "relevantremotenames",
                                               ["master"]))
        names = remotenames.readremotenames(repo)
        for rev, kind, prefix, name in names:
            if name in relevantnames and kind == "bookmarks":
                revs.add(repo[rev].rev())
    return revs

def fastmanifesttocache(repo, subset, x):
    """Revset of the interesting revisions to cache. This returns:
    - Drafts
    - Revisions with a bookmarks
    - Revisions with some selected remote bookmarks (master, stable ...)
    - Their parents (to make diff -c faster)
    - TODO The base of potential rebase operations
    - Filtering all of the above to only include recent changes
    """

    # Add relevant remotenames to the list of interesting revs
    revs = _relevantremonamesrevs(repo)

    # Add all the other relevant revs
    query = "(not public() & not hidden()) + bookmark()"
    cutoff = repo.ui.configint("fastmanifest", "cachecutoffdays", 60)
    if cutoff == -1: # no cutoff
        datelimit = ""
    else:
        datelimit = "and date(-%d)" % cutoff

    revs.update(scmutil.revrange(repo,["(%s + parents(%s)) %s"
                %(query, query, datelimit)]))

    return subset & revs

GB = 1024**3
MB = 1024**2

class _systemawarecachelimit(object):
    """A limit that will be tighter as the free disk space reduces"""
    def parseconfig(self, repo):
        configs = {
            'lowgrowthslope': constants.DEFAULT_LOWGROWTH_SLOPE,
            'lowgrowththresholdgb': constants.DEFAULT_LOWGROWTH_TRESHOLDGB,
            'maxcachesizegb': constants.DEFAULT_MAXCACHESIZEGB,
            'highgrowthslope': constants.DEFAULT_HIGHGROWTHSLOPE
        }
        for configkey, default in configs.items():
            strconfig = repo.ui.config("fastmanifest", configkey, default)
            try:
                configs[configkey] = float(strconfig)
            except ValueError:
                # Keep default value and print a warning when config is invalid
                msg = ("Invalid config for fastmanifest.%s, expected a number")
                repo.ui.warn((msg % strconfig))
        return configs

    def __init__(self, repo):
        # Probe the system root partition to know what is available
        st = os.statvfs(repo.root)
        self.free = st.f_bavail * st.f_frsize
        self.total = st.f_blocks * st.f_frsize
        # Read parameters from config
        self.config = self.parseconfig(repo)

    def bytes(self):
        return _systemawarecachelimit.cacheallocation(self.free, **self.config)

    @staticmethod
    def cacheallocation(
            freespace,
            lowgrowththresholdgb=constants.DEFAULT_LOWGROWTH_TRESHOLDGB,
            lowgrowthslope=constants.DEFAULT_LOWGROWTH_SLOPE,
            maxcachesizegb=constants.DEFAULT_MAXCACHESIZEGB,
            highgrowthslope=constants.DEFAULT_HIGHGROWTHSLOPE):
        """Given the free space available in bytes, return the size of the cache

        When disk space is limited (less than lowgrowththreshold), we increase
        the cache size linearly: lowgrowthslope * freespace. Over
        lowgrowththreshold, we increase the cache size linearly but faster:
        highgrowthslope * freespace until we hit maxcachesize.

        These values are configurable, default values are:

        [fastmanifest]
        lowgrowththresholdgb = 20
        lowgrowthslope = 0.1
        highgrowthslope = 0.2
        maxcachesizegb = 6

        ^ Cache Size
        |
        |      /-------------------  <- maxcachesize
        |     |
        |    /  <- slope is highgrowthslope
        |   | <- lowgrowththreshold
        |  /
        | /   <- slope is lowgrowslope
        |/
        -------------------------> Free Space
        """

        if freespace < lowgrowththresholdgb * GB:
            return min(maxcachesizegb * GB, lowgrowthslope * freespace)
        else:
            return min(maxcachesizegb * GB, highgrowthslope * freespace)

def cachemanifestpruneall(ui, repo):
    cache = fastmanifestcache.getinstance(repo.store.opener, ui)
    cache.pruneall()

def cachemanifestlist(ui, repo):
    cache = fastmanifestcache.getinstance(repo.store.opener, ui)
    total, numentries = cache.totalsize(silent=False)
    ui.status(("cache size is: %s\n" % util.bytecount(total)))
    ui.status(("number of entries is: %s\n" % numentries))

def shufflebybatch(it, batchsize):
    """Shuffle by batches to avoid caching process stepping on each other
    while maintaining an ordering between batches:

    Before:
    [ BATCH 1 | BATCH 2 | BATCH 3 ...]
    Where rev # in BATCH 1 > rev # in BATCH 2, etc.

    After:
    [ SHUFFLED BATCH 1 | SHUFFLED BATCH 2 | SHUFFLED BATCH 3 ...]
    Where rev # in SHUFFLED BATCH 1 > rev # in SHUFFLED BATCH 2, etc."""
    for batchstart in range(0, len(it), batchsize):
        batchend = min(len(it), batchstart + batchsize)
        batch = it[batchstart:batchend]
        random.shuffle(batch)
        it[batchstart:batchend] = batch

def cachemanifestfillandtrim(ui, repo, revset, limit, background):
    if background:
        if concurrency.fork_worker(ui, repo):
            return
    cache = fastmanifestcache.getinstance(repo.store.opener, ui)

    computedrevs = scmutil.revrange(repo, revset)
    sortedrevs = sorted(computedrevs, key=lambda x:-x)
    if ui.configbool("fastmanifest", "randomorder", True):
        # Make a copy because we want to keep the ordering to assign mtime
        # below
        revs = sortedrevs[:]
        batchsize = ui.configint("fastmanifest", "shufflebatchsize", 5)
        shufflebybatch(revs, batchsize)
    else:
        revs = sortedrevs

    revstomannodes = {}
    for rev in revs:
        mannode = revlog.hex(repo.changelog.changelogrevision(rev).manifest)
        revstomannodes[rev] = mannode
        if cache.containsnode(mannode):
            ui.debug("skipped %s, already cached (fast path)\n" % mannode)
            # Account for the fact that we access this manifest
            cache.refresh(mannode)
            continue
        manifest = repo[rev].manifest()
        if not cache.put(mannode, manifest, limit):
            # Insertion failed because cache is full
            del revstomannodes[rev]
            break

    # Make the least relevant entries have an artificially older mtime
    # than the more relevant ones. We use a resolution of 2 for time to work
    # accross all platforms and ensure that the order is marked.
    # Note that we use sortedrevs and not revs because here we don't care about
    # the shuffling, we just want the most relevant revisions to have more
    # recent mtime.
    mtimemultiplier = 2
    for offset, rev in enumerate(sortedrevs):
        if rev in revstomannodes:
            hexnode = revstomannodes[rev]
            cache.refresh(hexnode, delay=offset * mtimemultiplier)
        else:
            pass # We didn't have enough space for that rev

    if limit is not None:
        cache.prune(limit)

    if background:
        os._exit(0)

def _cacheonchangeconfig(repo):
    """return revs, bg, limit suitable for caching fastmanifest on change"""
    revset = ["fastmanifesttocache()"]
    bg = repo.ui.configbool("fastmanifest",
                            "cacheonchangebackground",
                            True)
    return revset, bg, _systemawarecachelimit(repo)

def triggercacheonbookmarkchange(orig, self, *args, **kwargs):
    repo = self._repo
    revset, bg, limit = _cacheonchangeconfig(repo)
    cachemanifestfillandtrim(repo.ui, repo, revset, limit, bg)
    return orig(self, *args, **kwargs)

def triggercacheondirstatechange(orig, self, *args, **kwargs):
    if util.safehasattr(self, "_fastmanifestrepo"):
        repo = self._fastmanifestrepo
        revset, bg, limit = _cacheonchangeconfig(repo)
        cachemanifestfillandtrim(repo.ui, repo, revset, limit, bg)
    return orig(self, *args, **kwargs)

def triggercacheonremotenameschange(orig, repo, *args, **kwargs):
    revset, bg, limit = _cacheonchangeconfig(repo)
    cachemanifestfillandtrim(repo.ui, repo, revset, limit, bg)
    return orig(repo, *args, **kwargs)
