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

from mercurial import bookmarks, cmdutil, dispatch, error, extensions
from mercurial import localrepo, manifest
from mercurial import revset as revsetmod

import cachemanager
from metrics import metricscollector
import debug
from implementation import manifestfactory

cmdtable = {}
command = cmdutil.command(cmdtable)

@command('^debugcachemanifest', [
    ('r', 'rev', [], 'cache the manifest for revs', 'REV'),
    ('a', 'all', False, 'cache all relevant revisions', ''),
    ('l', 'limit', -1, 'limit size of total rev in bytes', 'BYTES'),
    ('p', 'pruneall', False, 'prune all the entries'),
    ('e', 'list', False, 'list the content of the cache and its size','')],
    'hg debugcachemanifest')
def debugcachemanifest(ui, repo, *pats, **opts):
    if opts["limit"] == -1 :
        limit = None
    else:
        limit = debug.fixedcachelimit(opts["limit"])

    pruneall = opts["pruneall"]
    displaylist = opts['list']
    if opts["all"]:
        revset = ["fastmanifesttocache()"]
    elif opts["rev"]:
        revset = opts["rev"]
    else:
        revset = []

    ui.debug(("[FM] caching revset: %s, pruneall(%s), list(%s)\n")
             % (revset, pruneall, displaylist))

    if displaylist and pruneall:
        raise error.Abort("can only use --pruneall or --list not both")

    if pruneall:
        cachemanager.cachemanifestpruneall(ui, repo)
        return

    if displaylist:
        cachemanager.cachemanifestlist(ui, repo)
        return

    if revset or limit:
        cachemanager.cachemanifestfillandtrim(
            ui, repo, revset, limit)

@command('^cachemanifest', [],
    'hg cachemanifest')
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
        logger = debug.manifestaccesslogger(FastManifestExtension.get_ui())
        extensions.wrapfunction(manifest.manifest, 'rev', logger.revwrap)

        factory = manifestfactory(FastManifestExtension.get_ui())
        # Wraps all the function creating a manifestdict
        # We have to do that because the logic to create manifest can take
        # 7 different codepaths and we want to retain the node information
        # that comes at the top level:
        #
        # read -> _newmanifest ---------------------------> manifestdict
        #
        # readshallowfast -> readshallow -----------------> manifestdict
        #    \                    \------> _newmanifest --> manifestdict
        #    --> readshallowdelta ------------------------> manifestdict
        #         \->readdelta    -------> _newmanifest --> manifestdict
        #             \->slowreaddelta --> _newmanifest --> manifestdict
        #
        # othermethods -----------------------------------> manifestdict
        #
        # We can have hybridmanifest that wraps one hybridmanifest in some
        # codepath. We resolve to the correct flatmanifest when asked in
        # the_flatmanifest method
        #
        # The recursion level is at most 2 because we wrap the two top
        # level functions and _newmanifest
        # (wrapped only for the case of -1)
        extensions.wrapfunction(dispatch, 'runcommand',
                                FastManifestExtension._logonexit)
        extensions.wrapfunction(manifest.manifest, '_newmanifest',
                                factory.newmanifest)
        extensions.wrapfunction(manifest.manifest, 'read', factory.read)
        try:
            extensions.wrapfunction(manifest.manifest, 'readshallowfast',
                                    factory.read)
        except AttributeError:
            # The function didn't use to be defined in previous versions
            # of hg
            pass
        extensions.wrapfunction(manifest.manifest, 'add', factory.add)

        revsetmod.symbols['fastmanifesttocache'] = (
                cachemanager.fastmanifesttocache
        )
        revsetmod.safesymbols.add('fastmanifesttocache')
        revsetmod.symbols['fastmanifestcached'] = (
                cachemanager.fastmanifestcached
        )
        revsetmod.safesymbols.add('fastmanifestcached')

        # Trigger to enable caching of relevant manifests
        extensions.wrapfunction(bookmarks.bmstore, '_write',
                                cachemanager.triggers.onbookmarkchange)
        extensions.wrapfunction(localrepo.localrepository, 'commitctx',
                                cachemanager.triggers.oncommit)
        try:
            remotenames = extensions.find('remotenames')
        except KeyError:
            pass
        else:
            if remotenames:
                extensions.wrapfunction(
                    remotenames,
                    'saveremotenames',
                    cachemanager.triggers.onremotenameschange)

        extensions.wrapfunction(dispatch, 'runcommand',
                        cachemanager.triggers.runcommandtrigger)

def extsetup(ui):
    # always update the ui object.  this is probably a bogus ui object, but we
    # don't want to have a backing ui object of None.
    FastManifestExtension.set_ui(ui)

    FastManifestExtension.setup()

def reposetup(ui, repo):
    # always update the ui object.
    FastManifestExtension.set_ui(ui)
