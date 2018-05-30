# fastmanifest.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""a treemanifest disk cache for speeding up manifest comparison

This extension adds fastmanifest, a treemanifest disk cache for speeding up
manifest comparison. It also contains utilities to investigate manifest access
patterns.

Configuration options and default value:

[fastmanifest]

# If true, disable all logging, used for running the mercurial test suite
# without changing the output.
silent = False

# If true, suppress all logging from worker processes.
silentworker = True

# If true, materializes every manifest as a fastmanifest. Used to test that
# fastmanifest passes the mercurial test suite. This happens in memory only and
# the on-disk fileformat is still revlog of flat manifest.
debugcachemanifest = False

# Filename, is not empty will log access to any manifest.
logfile = ""

# Cache fastmanifest if remotenames or bookmarks change, or on a commit.
cacheonchange = False

# Make cacheonchange(see above) work in the background.
cacheonchangebackground = True

# Maximum number of fastmanifest kept in volatile memory
maxinmemoryentries = 10

# Dump metrics after each command, see metrics.py
debugmetrics = False

# If False, cache entries in a deterministic order, otherwise use a randomorder
# by batches.
randomorder = True

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

# Enables the creation and use of fast cache manifests (defaults to True)
usecache=False

# Enables the use of treemanifests (defaults to False)
usetree=True

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
from __future__ import absolute_import

import sys

from mercurial import (
    bookmarks,
    dispatch,
    error,
    extensions,
    localrepo,
    manifest,
    registrar,
    revset as revsetmod,
)
from mercurial.i18n import _

from . import cachemanager, debug, implementation, metrics


metricscollector = metrics.metricscollector
manifestfactory = implementation.manifestfactory
fastmanifestcache = implementation.fastmanifestcache

cmdtable = {}
command = registrar.command(cmdtable)

configtable = {}
configitem = registrar.configitem(configtable)

configitem("fastmanifest", "logfile", default="")
configitem("fastmanifest", "debugmetrics", default=False)
configitem("fastmanifest", "usecache", default=True)
configitem("fastmanifest", "usetree", default=False)


@command(
    "^debugcachemanifest",
    [
        ("r", "rev", [], "cache the manifest for revs", "REV"),
        ("a", "all", False, "cache all relevant revisions", ""),
        (
            "l",
            "limit",
            0,
            "limit size of total rev in bytes (<0: unlimited; 0: default policy)",
            "BYTES",
        ),
        ("p", "pruneall", False, "prune all the entries"),
        ("e", "list", False, "list the content of the cache and its size", ""),
    ],
    "hg debugcachemanifest",
)
def debugcachemanifest(ui, repo, *pats, **opts):
    pruneall = opts["pruneall"]
    displaylist = opts["list"]
    if opts["all"]:
        revset = ["fastmanifesttocache()"]
    elif opts["rev"]:
        revset = opts["rev"]
    else:
        revset = []

    ui.debug(
        ("[FM] caching revset: %s, pruneall(%s), list(%s)\n")
        % (revset, pruneall, displaylist)
    )

    if displaylist and pruneall:
        raise error.Abort("can only use --pruneall or --list not both")

    if pruneall:
        cachemanager.cachemanifestpruneall(ui, repo)
        return

    if displaylist:
        cachemanager.cachemanifestlist(ui, repo)
        return

    if opts["limit"] != 0:
        if opts["limit"] < 0:
            limitbytes = sys.maxint
        else:
            limitbytes = opts["limit"]

        cache = fastmanifestcache.getinstance(repo.store.opener, ui)
        cache.overridelimit(debug.fixedcachelimit(limitbytes))

    cachemanager.cachemanifestfillandtrim(ui, repo, revset)


@command("^cachemanifest", [], "hg cachemanifest")
def cachemanifest(ui, repo, *pats, **opts):
    cachemanager.cacher.cachemanifest(repo)


class uiproxy(object):
    """This is a proxy object that forwards all requests to a real ui object."""

    def __init__(self, ui):
        self.ui = ui

    def _updateui(self, ui):
        self.ui = ui

    def __getattr__(self, name):
        return getattr(self.ui, name)


class FastManifestExtension(object):
    initialized = False
    uiproxy = uiproxy(None)

    @staticmethod
    def _logonexit(orig, ui, repo, cmd, fullargs, *args):
        r = orig(ui, repo, cmd, fullargs, *args)
        metricscollector.get().logsamples(ui)
        return r

    @staticmethod
    def get_ui():
        return FastManifestExtension.uiproxy

    @staticmethod
    def set_ui(ui):
        FastManifestExtension.uiproxy._updateui(ui)

    @staticmethod
    def setup():
        ui = FastManifestExtension.get_ui()
        logger = debug.manifestaccesslogger(ui)
        extensions.wrapfunction(manifest.manifestrevlog, "rev", logger.revwrap)

        factory = manifestfactory(ui)

        extensions.wrapfunction(manifest.manifestlog, "__getitem__", factory.newgetitem)
        extensions.wrapfunction(
            manifest.manifestlog, "get", factory.newgetdirmanifestctx
        )
        extensions.wrapfunction(manifest.memmanifestctx, "write", factory.ctxwrite)
        extensions.wrapfunction(manifest.manifestrevlog, "add", factory.add)

        if ui.configbool("fastmanifest", "usecache"):
            revsetmod.symbols["fastmanifesttocache"] = cachemanager.fastmanifesttocache
            revsetmod.safesymbols.add("fastmanifesttocache")
            revsetmod.symbols["fastmanifestcached"] = cachemanager.fastmanifestcached
            revsetmod.safesymbols.add("fastmanifestcached")

            # Trigger to enable caching of relevant manifests
            extensions.wrapfunction(
                bookmarks.bmstore, "_write", cachemanager.triggers.onbookmarkchange
            )
            extensions.wrapfunction(
                localrepo.localrepository, "commitctx", cachemanager.triggers.oncommit
            )
            try:
                remotenames = extensions.find("remotenames")
            except KeyError:
                pass
            else:
                if remotenames:
                    extensions.wrapfunction(
                        remotenames,
                        "saveremotenames",
                        cachemanager.triggers.onremotenameschange,
                    )

            extensions.wrapfunction(
                dispatch, "runcommand", cachemanager.triggers.runcommandtrigger
            )

        extensions.wrapfunction(
            dispatch, "runcommand", FastManifestExtension._logonexit
        )


def extsetup(ui):
    # always update the ui object.  this is probably a bogus ui object, but we
    # don't want to have a backing ui object of None.
    FastManifestExtension.set_ui(ui)

    FastManifestExtension.setup()


def reposetup(ui, repo):
    # Don't update the ui for remote peer repos, since they won't have the local
    # configs.
    if repo.local() is None:
        return

    # always update the ui object.
    FastManifestExtension.set_ui(ui)

    if ui.configbool("fastmanifest", "usetree"):
        try:
            extensions.find("treemanifest")
        except KeyError:
            raise error.Abort(
                _(
                    "fastmanifest.usetree cannot be enabled without"
                    " enabling treemanifest"
                )
            )
