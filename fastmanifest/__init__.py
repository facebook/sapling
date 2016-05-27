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

from mercurial import bookmarks, cmdutil, dirstate, error, extensions
from mercurial import localrepo, manifest, revset

from extutil import wrapfilecache

import cachemanager
import concurrency
from fastmanifest import *
from implementation import manifestfactory

cmdtable = {}
command = cmdutil.command(cmdtable)

@command('^debugcachemanifest', [
    ('r', 'rev', [], 'cache the manifest for revs', 'REV'),
    ('a', 'all', False, 'cache all relevant revisions', ''),
    ('l', 'limit', -1, 'limit size of total rev in bytes', 'BYTES'),
    ('p', 'pruneall', False, 'prune all the entries'),
    ('b', 'background', False,
     'return imediately and process in the background', ''),
    ('e', 'list', False, 'list the content of the cache and its size','')],
    'hg debugcachemanifest')
def debugcachemanifest(ui, repo, *pats, **opts):
    background = opts["background"]
    if opts["limit"] == -1 :
        limit = None
    else:
        limit = fixedcachelimit(opts["limit"])

    pruneall = opts["pruneall"]
    displaylist = opts['list']
    if opts["all"]:
        revset = ["fastmanifesttocache()"]
    elif opts["rev"]:
        revset = opts["rev"]
    else:
        revset = []

    ui.debug(("caching revset: %s, background(%s), pruneall(%s), list(%s)\n")
             % (revset, background, pruneall, displaylist))

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
            ui, repo, revset, limit, background)

def extsetup(ui):
    logfile = ui.config("fastmanifest", "logfile", "")
    factory = manifestfactory(ui)
    if logfile:
        logger = manifestaccesslogger(logfile)
        extensions.wrapfunction(manifest.manifest, 'rev', logger.revwrap)
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
    # codepath. We resolve to the correct flatmanifest when asked in the
    # _flatmanifest method
    #
    # The recursion level is at most 2 because we wrap the two top level
    # functions and _newmanifest (wrapped only for the case of -1)

    extensions.wrapfunction(manifest.manifest, '_newmanifest',
                            factory.newmanifest)
    extensions.wrapfunction(manifest.manifest, 'read', factory.read)
    try:
        extensions.wrapfunction(manifest.manifest, 'readshallowfast',
                                factory.read)
    except AttributeError:
        # The function didn't use to be defined in previous versions of hg
        pass

    revset.symbols['fastmanifesttocache'] = cachemanager.fastmanifesttocache
    revset.safesymbols.add('fastmanifesttocache')

    if ui.configbool("fastmanifest", "cacheonchange", False):
        # Trigger to enable caching of relevant manifests
        extensions.wrapfunction(bookmarks.bmstore, '_write',
                                cachemanager.triggercacheonbookmarkchange)
        extensions.wrapfunction(dirstate.dirstate, 'write',
                                cachemanager.triggercacheondirstatechange)
        try:
            remotenames = extensions.find('remotenames')
        except KeyError:
            pass
        else:
            if remotenames:
                extensions.wrapfunction(
                    remotenames,
                    'saveremotenames',
                    cachemanager.triggercacheonremotenameschange)

        def wrapdirstate(orig, self):
            dirstate = orig(self)
            dirstate._fastmanifestrepo = self
            return dirstate
        wrapfilecache(localrepo.localrepository, 'dirstate',
                                 wrapdirstate)
