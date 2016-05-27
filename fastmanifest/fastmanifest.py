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

class fixedcachelimit(object):
    """A fix cache limit expressed as a number of bytes"""
    def __init__(self, bytes):
        self._bytes = bytes

    def bytes(self):
        return self._bytes
