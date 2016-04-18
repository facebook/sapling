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


Configuration options:

[fastmanifest]
logfile = "" # Filename, is not empty will log access to any manifest


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
TODO handle asynchronous save
TODO size limit handling
"""
import os
import fastmanifest_wrapper

from mercurial import cmdutil
from mercurial import extensions
from mercurial import manifest
from mercurial import revset
from mercurial import revlog
from mercurial import scmutil
from mercurial import util

CACHE_SUBDIR = "manifestcache"
cmdtable = {}
command = cmdutil.command(cmdtable)


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


def fastmanifesttocache(repo, subset, x):
    """Revset of the interesting revisions to cache"""
    return scmutil.revrange(repo, ["not public() + bookmark()"])


class hybridmanifest(object):
    """
    Hybrid manifest that behaves like a lazy manifest.

    Initialized with:
    - loadflat a function to load a flat manifest from disk
    - cache an object with mapping method to work with fast manifest from disk

    For the moment, behaves like a lazymanifest since cachedmanifest is not
    yet available.
    """
    def __init__(self, loadflat, ui, flatcache=None, fastcache=None,
                 node=None):
        self.loadflat = loadflat
        self.__flatmanifest = None
        self.flatcache = flatcache
        self.__cachedmanifest = None
        self.fastcache = fastcache
        self.node = node
        self.ui = ui
        if self.ui:
            self.debugfastmanifest = self.ui.configbool("fastmanifest",
                                                        "debugfastmanifest")
        else:
            self.debugfastmanifest = False
        if self.node:
            self.node = revlog.hex(self.node)

    def _flatmanifest(self):
        if not self.__flatmanifest:
            # Cache lookup
            if (self.node and self.flatcache
               and self.flatcache.contains(self.node)):
                self.__flatmanifest = self.flatcache.get(self.node)
                if self.__flatmanifest:
                    self.ui.debug("cache hit for flatmanifest %s\n"
                                  % self.node)
                    return self.__flatmanifest
            if self.node:
                self.ui.debug("cache miss for flatmanifest %s\n" % self.node)

            # Disk lookup
            self.__flatmanifest = self.loadflat()
            if isinstance(self.__flatmanifest, hybridmanifest):
                # See comment in extsetup to see why we have to do that
                self.__flatmanifest = self.__flatmanifest._flatmanifest()
            assert isinstance(self.__flatmanifest, manifest.manifestdict)
        return self.__flatmanifest

    def _cachedmanifest(self):
        if not self.__cachedmanifest:
            # Cache lookup
            if (self.node and self.fastcache
               and self.fastcache.contains(self.node)):
                self.__cachedmanifest = self.fastcache.get(self.node)
                if self.__cachedmanifest:
                    self.ui.debug("cache hit for fastmanifest %s\n"
                                  % self.node)
                    return self.__cachedmanifest
        return None

    def _incache(self):
        if self.flatcache and self.node:
            return self.flatcache.contains(self.node)
        return False

    def _manifest(self, operation):
        # Get the manifest most suited for the operations (flat or cached)
        # TODO return fastmanifest when suitable
        if self.debugfastmanifest:
            return fastmanifest_wrapper(self._flatmanifest().text())
        return self._flatmanifest()

    # Proxy all the manifest methods to the flatmanifest except magic methods
    def __getattr__(self, name):
        return getattr(self._manifest(name), name)

    # Magic methods should be proxied differently than __getattr__
    # For the moment all methods they all use the _flatmanifest
    def __iter__(self):
        return self._manifest('__iter__').__iter__()

    def __contains__(self, key):
        return self._manifest('__contains__').__contains__(key)

    def __getitem__(self, key):
        return self._manifest('__getitem__').__getitem__(key)

    def __setitem__(self, key, val):
        return self._manifest('__setitem__').__setitem__(key, val)

    def __delitem__(self, key):
        return self._manifest('__delitem__').__delitem__(key)

    def __len__(self):
        return self._manifest('__len__').__len__()

    def copy(self):
        return hybridmanifest(loadflat=lambda: self._flatmanifest().copy(),
                              flatcache=self.flatcache,
                              fastcache=self.fastcache,
                              node=self.node,
                              ui=self.ui)

    def matches(self, *args, **kwargs):
        newload = lambda: self._flatmanifest().matches(*args, **kwargs)
        return hybridmanifest(loadflat=newload,
                              flatcache=self.flatcache,
                              fastcache=self.fastcache,
                              ui=self.ui)

    def diff(self, m2, *args, **kwargs):
        self.ui.debug("performing diff\n")
        # Find _m1 and _m2 of the same type, to provide the fastest computation
        _m1, _m2 = None, None

        if isinstance(m2, hybridmanifest):
            self.ui.debug("other side is hybrid manifest\n")
            # CACHE HIT
            if self._incache() and m2._incache():
                _m1, _m2 = self._cachedmanifest(), m2._cachedmanifest()
                # _m1 or _m2 can be None if _incache was True if the cache
                # got garbage collected in the meantime or entry is corrupted
                if not _m1 or not _m2:
                    self.ui.debug("fallback to regular diff\n")
                    _m1, _m2 = self._flatmanifest(), m2._flatmanifest()
                else:
                    self.ui.debug("fastmanifest diff\n")

            # CACHE MISS
            else:
                self.ui.debug("fallback to regular diff\n")
                _m1, _m2 = self._flatmanifest(), m2._flatmanifest()
        else:
            # This happens when diffing against a new manifest (like rev -1)
            self.ui.debug("fallback to regular diff\n")
            _m1, _m2 = self._flatmanifest(), m2

        assert type(_m1) == type(_m2)
        return _m1.diff(_m2, *args, **kwargs)


class manifestcache(object):
    def __init__(self, opener, ui):
        self.opener = opener
        self.ui = ui
        self.inmemorycache = {}
        base = opener.join(None)
        self.cachepath = os.path.join(base, CACHE_SUBDIR)
        if not os.path.exists(self.cachepath):
            os.makedirs(self.cachepath)

    def keyprefix(self):
        raise NotImplementedError("abstract method, should be overriden")

    def load(self, data):
        raise NotImplementedError("abstract method, should be overriden")

    def dump(self, manifest):
        raise NotImplementedError("abstract method, should be overriden")

    def inmemorycachekey(self, key):
        return (self.keyprefix(), key)

    def filecachepath(self, key):
        return os.path.join(self.cachepath, self.keyprefix() + key)

    def get(self, key):
        # In memory cache lookup
        ident = self.inmemorycachekey(key)
        r = self.inmemorycache.get(ident, None)
        if r:
            return r

        # On disk cache lookup
        try:
            with open(self.filecachepath(key)) as f:
                r = self.load(f.read())
        except EnvironmentError:
            return None

        # In memory cache update
        if r:
            self.inmemorycache[ident] = r
        return r

    def contains(self, key):
        if self.inmemorycachekey(key) in self.inmemorycache:
            return True
        return os.path.exists(self.filecachepath(key))

    def put(self, key, manifest):
        if self.contains(key):
            self.ui.debug("skipped %s, already cached\n" % key)
        else:
            self.ui.debug("caching revision %s\n" % key)
            fh = util.atomictempfile(self.filecachepath(key), mode="w+")
            try:
                fh.write(self.dump(manifest))
            finally:
                fh.close()

    def prune(self, limit):
        # TODO logic to prune old entries
        pass

# flatmanifestcache and fastmanifestcache are singletons
# Implementation inspired from:
# https://stackoverflow.com/questions/31875/
# is-there-a-simple-elegant-way-to-define-singletons-in-python


class flatmanifestcache(manifestcache):
    _instance = None

    def __new__(cls, *args, **kwargs):
        if not cls._instance:
            cls._instance = super(flatmanifestcache, cls).__new__(cls, *args,
                                                                  **kwargs)
        return cls._instance

    def keyprefix(self):
        return "flat"

    def load(self, data):
        return manifest.manifestdict(data)

    def dump(self, manifest):
        return manifest.text()


class fastmanifestcache(manifestcache):
    _instance = None

    def __new__(cls, *args, **kwargs):
        if not cls._instance:
            cls._instance = super(fastmanifestcache, cls).__new__(cls, *args,
                                                                  **kwargs)
        return cls._instance

    def keyprefix(self):
        return "fast"

    def load(self, data):
        raise NotImplementedError("TODO integrate with @ttung's code")

    def dump(self, manifest):
        raise NotImplementedError("TODO integrate with @ttung's code")


class manifestfactory(object):
    def __init__(self, ui):
        self.ui = ui

    def newmanifest(self, orig, *args, **kwargs):
        loadfn = lambda: orig(*args, **kwargs)
        fastcache = fastmanifestcache(args[0].opener, self.ui)
        flatcache = flatmanifestcache(args[0].opener, self.ui)
        return hybridmanifest(loadflat=loadfn,
                              ui=self.ui,
                              flatcache=flatcache,
                              fastcache=fastcache)

    def read(self, orig, *args, **kwargs):
        loadfn = lambda: orig(*args, **kwargs)
        fastcache = fastmanifestcache(args[0].opener, self.ui)
        flatcache = flatmanifestcache(args[0].opener, self.ui)
        return hybridmanifest(loadflat=loadfn,
                              ui=self.ui,
                              flatcache=flatcache,
                              fastcache=fastcache,
                              node=args[1])


def _cachemanifest(ui, repo, revs, flat, sync, limit):
    ui.debug(("caching rev: %s , synchronous(%s), flat(%s)\n")
             % (revs, sync, flat))
    if flat:
        cache = flatmanifestcache(repo.store.opener, ui)
    else:
        cache = fastmanifestcache(repo.store.opener, ui)

    for rev in revs:
        manifest = repo[rev].manifest()
        nodehex = manifest.node
        cache.put(nodehex, manifest)

    if limit:
        cache.prune(limit)


@command('^debugcachemanifest', [
    ('r', 'rev', [], 'cache the manifest for revs', 'REV'),
    ('f', 'flat', False, 'cache flat manifests instead of fast manifests', ''),
    ('a', 'all', False, 'cache all relevant revisions', ''),
    ('l', 'limit', False, 'limit size of total rev in bytes', 'BYTES'),
    ('s', 'synchronous', False, 'wait for completion to return', '')],
    'hg debugcachemanifest')
def debugcachemanifest(ui, repo, *pats, **opts):
    flat = opts["flat"]
    sync = opts["synchronous"]
    limit = opts["limit"]
    if opts["all"]:
        revs = scmutil.revrange(repo, ["fastmanifesttocache()"])
    elif opts["rev"]:
        revs = scmutil.revrange(repo, opts["rev"])
    else:
        revs = []
    _cachemanifest(ui, repo, revs, flat, sync, limit)


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

    revset.symbols['fastmanifesttocache'] = fastmanifesttocache
    revset.safesymbols.add('fastmanifesttocache')

