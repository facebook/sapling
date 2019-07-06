# cachemanager.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import

import errno
import os

from edenscm.mercurial import encoding, error, extensions, revlog, scmutil, util
from edenscmnative import cfastmanifest

from . import concurrency, constants
from .implementation import CacheFullException, fastmanifestcache
from .metrics import metricscollector


def _relevantremonamesrevs(repo):
    revs = set()
    remotenames = None
    try:
        remotenames = extensions.find("remotenames")
    except KeyError:  # remotenames not loaded
        pass
    if remotenames is not None:
        # interesting remotenames to fetch
        relevantnames = set(
            repo.ui.configlist("fastmanifest", "relevantremotenames", ["master"])
        )
        names = remotenames.readremotenames(repo)
        for rev, kind, prefix, name in names:
            if name in relevantnames and kind == "bookmarks":
                revs.add(repo[rev].rev())
    return revs


def fastmanifestcached(repo, subset, x):
    """Revset encompassing all revisions whose manifests are cached"""
    # At the high level, we look at what is cached, and go from manifest nodes
    # to changelog revs.
    #
    # 1) We look at all the cached manifest, from each of them we find the first
    # changelog rev that introduced each cached manifest thanks to linkrevs.
    # 2) We compute the minimum of those changelog revs. It is guaranteed that
    # all the changelog revs whose manifest are cached are above that minimum
    # rev in the changelog
    # 3) From this minimum, we inspect all the more recent and visible changelog
    # revisions and keep track of the one whose manifest is cached.
    cache = fastmanifestcache.getinstance(repo.store.opener, repo.ui)
    manifestsbinnodes = set(
        [revlog.bin(u.replace("fast", "")) for u in cache.ondiskcache]
    )
    mfrevlog = repo.manifestlog._revlog
    manifestslinkrevs = [mfrevlog.linkrev(mfrevlog.rev(k)) for k in manifestsbinnodes]
    cachedrevs = set()
    if manifestslinkrevs:
        for u in repo.changelog.revs(min(manifestslinkrevs)):
            revmanifestbin = repo.changelog.changelogrevision(u).manifest
            if revmanifestbin in manifestsbinnodes:
                cachedrevs.add(u)
    return subset & cachedrevs


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
    if cutoff == -1:  # no cutoff
        datelimit = ""
    else:
        datelimit = "and date(-%d)" % cutoff

    revs.update(
        scmutil.revrange(repo, ["(%s + parents(%s)) %s" % (query, query, datelimit)])
    )

    metricscollector.get().recordsample("revsetsize", size=len(revs))
    return subset & revs


GB = 1024 ** 3
MB = 1024 ** 2


class _systemawarecachelimit(object):
    """A limit that will be tighter as the free disk space reduces"""

    def parseconfig(self, ui):
        configkeys = set(
            [
                "lowgrowthslope",
                "lowgrowththresholdgb",
                "maxcachesizegb",
                "highgrowthslope",
            ]
        )
        configs = {}
        for configkey in configkeys:
            strconfig = ui.config("fastmanifest", configkey)
            if strconfig is None:
                continue
            try:
                configs[configkey] = float(strconfig)
            except ValueError:
                # Keep default value and print a warning when config is invalid
                msg = "Invalid config for fastmanifest.%s, expected a number"
                ui.warn((msg % strconfig))
        return configs

    def __init__(self, repo=None, opener=None, ui=None):
        # Probe the system root partition to know what is available
        try:
            if repo is None and (opener is None or ui is None):
                raise error.Abort("Need to specify repo or (opener and ui)")
            st = None
            if not util.safehasattr(os, "statvfs"):
                self.free = 0
                self.total = 0
            elif repo is not None:
                st = os.statvfs(repo.root)
            else:
                st = os.statvfs(opener.join(None))
            if st is not None:
                self.free = st.f_bavail * st.f_frsize
                self.total = st.f_blocks * st.f_frsize
        except (OSError, IOError) as ex:
            if ex.errno == errno.EACCES:
                self.free = 0
                self.total = 0
                return
            raise

        # Read parameters from config
        if repo is not None:
            self.config = self.parseconfig(repo.ui)
        else:
            self.config = self.parseconfig(ui)

    def bytes(self):
        return _systemawarecachelimit.cacheallocation(self.free, **self.config)

    @staticmethod
    def cacheallocation(
        freespace,
        lowgrowththresholdgb=constants.DEFAULT_LOWGROWTH_TRESHOLDGB,
        lowgrowthslope=constants.DEFAULT_LOWGROWTH_SLOPE,
        maxcachesizegb=constants.DEFAULT_MAXCACHESIZEGB,
        highgrowthslope=constants.DEFAULT_HIGHGROWTHSLOPE,
    ):
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
    total, numentries = cache.ondiskcache.totalsize(silent=False)
    ui.status(("cache size is: %s\n" % util.bytecount(total)))
    ui.status(("number of entries is: %s\n" % numentries))

    if ui.debug:
        revs = set(repo.revs("fastmanifestcached()"))
        import collections

        revstoman = collections.defaultdict(list)
        for r in revs:
            mannode = revlog.hex(repo.changelog.changelogrevision(r).manifest)
            revstoman[mannode].append(str(r))
        if revs:
            ui.status(("Most relevant cache entries appear first\n"))
            ui.status(("=" * 80))
            ui.status(("\nmanifest node                           |revs\n"))
            for h in cache.ondiskcache:
                l = h.replace("fast", "")
                ui.status("%s|%s\n" % (l, ",".join(revstoman.get(l, []))))


def cachemanifestfillandtrim(ui, repo, revset):
    """Cache the manifests described by `revset`.  This priming is subject to
    limits imposed by the cache, and thus not all the entries may be written.
    """
    try:
        with concurrency.looselock(
            repo.localvfs, "fastmanifest", constants.WORKER_SPAWN_LOCK_STEAL_TIMEOUT
        ):
            cache = fastmanifestcache.getinstance(repo.store.opener, ui)

            computedrevs = scmutil.revrange(repo, revset)
            sortedrevs = sorted(computedrevs, key=lambda x: -x)

            if len(sortedrevs) == 0:
                # normally, we prune as we make space for new revisions to add
                # to the cache.  however, if we're not adding any new elements,
                # we'll never check the disk cache size.  this is an explicit
                # check for that particular scenario.
                cache.prune()
            else:
                revstomannodes = {}
                mannodesprocessed = set()
                for rev in sortedrevs:
                    mannode = revlog.hex(repo.changelog.changelogrevision(rev).manifest)
                    revstomannodes[rev] = mannode
                    mannodesprocessed.add(mannode)

                    if mannode in cache.ondiskcache:
                        ui.debug(
                            "[FM] skipped %s, already cached "
                            "(fast path)\n" % (mannode,)
                        )

                        # Account for the fact that we access this manifest
                        cache.ondiskcache.touch(mannode)
                        continue
                    manifest = repo[rev].manifest()
                    fastmanifest = cfastmanifest.fastmanifest(manifest.text())

                    cache.makeroomfor(fastmanifest.bytes(), mannodesprocessed)

                    try:
                        cache[mannode] = fastmanifest
                    except CacheFullException:
                        break

                # Make the least relevant entries have an artificially older
                # mtime than the more relevant ones. We use a resolution of 2
                # for time to work accross all platforms and ensure that the
                # order is marked.
                #
                # Note that we use sortedrevs and not revs because here we
                # don't care about the shuffling, we just want the most relevant
                # revisions to have more recent mtime.
                mtimemultiplier = 2
                for offset, rev in enumerate(sortedrevs):
                    if rev in revstomannodes:
                        hexnode = revstomannodes[rev]
                        cache.ondiskcache.touch(hexnode, delay=offset * mtimemultiplier)
                    else:
                        metricscollector.get().recordsample("cacheoverflow", hit=True)
                        # We didn't have enough space for that rev
    except error.LockHeld:
        return
    except (OSError, IOError) as ex:
        if ex.errno == errno.EACCES:
            # permission issue
            ui.warn(("warning: not using fastmanifest\n"))
            ui.warn(("(make sure that .hg/store is writeable)\n"))
            return
        raise

    total, numentries = cache.ondiskcache.totalsize()
    if isinstance(cache.limit, _systemawarecachelimit):
        free = cache.limit.free / 1024 ** 2
    else:
        free = -1
    metricscollector.get().recordsample(
        "ondiskcachestats",
        bytes=total,
        numentries=numentries,
        limit=(cache.limit.bytes() / 1024 ** 2),
        freespace=free,
    )


class cacher(object):
    @staticmethod
    def cachemanifest(repo):
        revset = ["fastmanifesttocache()"]
        cachemanifestfillandtrim(repo.ui, repo, revset)


class triggers(object):
    repos_to_update = set()

    @staticmethod
    def runcommandtrigger(orig, *args, **kwargs):
        result = orig(*args, **kwargs)

        for repo in triggers.repos_to_update:
            bg = repo.ui.configbool("fastmanifest", "cacheonchangebackground", True)

            if bg:
                silent_worker = repo.ui.configbool("fastmanifest", "silentworker", True)

                # see if the user wants us to invoke a specific instance of
                # mercurial.
                workerexe = encoding.environ.get("SCM_WORKER_EXE")

                cmd = util.hgcmd()[:]
                if workerexe is not None:
                    cmd[0] = workerexe

                cmd.extend(["--repository", repo.root, "cachemanifest"])

                concurrency.runshellcommand(cmd, silent_worker=silent_worker)
            else:
                cacher.cachemanifest(repo)

        return result

    @staticmethod
    def onbookmarkchange(orig, self, *args, **kwargs):
        repo = self._repo
        ui = repo.ui

        if ui.configbool("fastmanifest", "cacheonchange", False):
            triggers.repos_to_update.add(repo)
            metricscollector.get().recordsample("trigger", source="bookmark")

        return orig(self, *args, **kwargs)

    @staticmethod
    def oncommit(orig, self, *args, **kwargs):
        repo = self
        ui = repo.ui

        if ui.configbool("fastmanifest", "cacheonchange", False):
            triggers.repos_to_update.add(repo)
            metricscollector.get().recordsample("trigger", source="commit")

        return orig(self, *args, **kwargs)

    @staticmethod
    def onremotenameschange(orig, repo, *args, **kwargs):
        ui = repo.ui

        if ui.configbool("fastmanifest", "cacheonchange", False):
            triggers.repos_to_update.add(repo)
            metricscollector.get().recordsample("trigger", source="remotenames")

        return orig(repo, *args, **kwargs)
