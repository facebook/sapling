# fastmanifest.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""
This extension adds fastmanifest, a treemanifest disk cache for speeding up
manifest comparison. It also contains utilities to investigate manifest access
patterns.


Configuration options and default value:

[fastmanifest]

# If true, disable all logging, used for running the mercurial test suite
# without changing the output.
silent = False

# If true, materializes every manifest as a fastmanifest. Used to test that
# fastmanifest passes the mercurial test suite. This happens in memory only and
# the on-disk fileformat is still revlog of flat manifest.
debugcachemanifest = False

# Filename, is not empty will log access to any manifest.
logfile = ""

# Cache fastmanifest if dirstate, remotenames or bookmarks change.
cacheonchange = False

# Make cacheonchange(see above) work in the background.
cacheonchangebackground = True

# If False, cache entries in a deterministic order, otherwise use a randomorder
# by batches.
randomorder = True

# Batch size for the random ordering.
shufflebatchsize = 5

# Cache properties, see systemawarecachelimit.
lowgrowththresholdgb = 20
lowgrowthslope = 0.1
highgrowthslope = 0.2
maxcachesizegb = 6

# Cut off date, revisions older than the cutoff won't be cached, default is
# 60 days. -1 means no limit.
cachecutoffdays = 60

# List of relevant remotenames whose manifest is to be included in the cache.
# The list is comma or space separated
relevantremotenames = master

Description:

`manifestaccesslogger` logs manifest accessed to a logfile specified with
the option fastmanifest.logfile

`fastmanifesttocache` is a revset of relevant manifests to cache

`hybridmanifest` is a proxy class for flat and cached manifest that loads
manifest from cache or from disk.
It chooses what kind of manifest is relevant to create based on the operation,
ideally the fastest.
TODO instantiate fastmanifest when they are more suitable

`manifestcache` is the class handling the interface with the cache, it supports
caching flat and fast manifest and retrieving them.
TODO logic for loading fastmanifest
TODO logic for saving fastmanifest
TODO garbage collection

`manifestfactory` is a class whose method wraps manifest creating method of
manifest.manifest. It intercepts the calls to build hybridmanifest instead of
regularmanifests. We use a class for that to allow sharing the ui object that
is not normally accessible to manifests.

`debugcachemanifest` is a command calling `_cachemanifest`, a function to add
manifests to the cache and manipulate what is cached. It allows caching fast
and flat manifest, asynchronously and synchronously.
"""
import os
import random

from mercurial import extensions, revlog, scmutil, util

from extutil import wrapfilecache

import concurrency
import constants
from implementation import fastmanifestcache

class manifestaccesslogger(object):
    """Class to log manifest access and confirm our assumptions"""
    def __init__(self, logfile):
        self._logfile = logfile

    def revwrap(self, orig, *args, **kwargs):
        """Wraps manifest.rev and log access"""
        r = orig(*args, **kwargs)
        try:
            with open(self._logfile, "a") as f:
                f.write("%s\n" % r)
        except EnvironmentError:
            pass
        return r

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

class fixedcachelimit(object):
    """A fix cache limit expressed as a number of bytes"""
    def __init__(self, bytes):
        self._bytes = bytes

    def bytes(self):
        return self._bytes


GB = 1024**3
MB = 1024**2
DEFAULT_LOWGROWTH_TRESHOLDGB = 20
DEFAULT_MAXCACHESIZEGB = 6
DEFAULT_LOWGROWTH_SLOPE = 0.1
DEFAULT_HIGHGROWTHSLOPE = 0.2

class systemawarecachelimit(object):
    """A limit that will be tighter as the free disk space reduces"""
    def parseconfig(self, repo):
        configs = {
            'lowgrowthslope': DEFAULT_LOWGROWTH_SLOPE,
            'lowgrowththresholdgb': DEFAULT_LOWGROWTH_TRESHOLDGB,
            'maxcachesizegb': DEFAULT_MAXCACHESIZEGB,
            'highgrowthslope': DEFAULT_HIGHGROWTHSLOPE
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
        return systemawarecachelimit.cacheallocation(self.free, **self.config)

    @staticmethod
    def cacheallocation(freespace,
                        lowgrowththresholdgb=DEFAULT_LOWGROWTH_TRESHOLDGB,
                        lowgrowthslope=DEFAULT_LOWGROWTH_SLOPE,
                        maxcachesizegb=DEFAULT_MAXCACHESIZEGB,
                        highgrowthslope=DEFAULT_HIGHGROWTHSLOPE):
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
    return revset, bg, systemawarecachelimit(repo)

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
