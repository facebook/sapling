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

# Make cacheonchange use systemawarecachelimit.
cacheonchangesystemlimit = True

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
import array
import os
import random
import sys
import time

from mercurial import bookmarks, cmdutil, dirstate, error, extensions
from mercurial import localrepo, manifest, mdiff, revlog, revset
from mercurial import scmutil, util

from extutil import wrapfilecache

import fastmanifest_wrapper

CACHE_SUBDIR = "manifestcache"
cmdtable = {}
command = cmdutil.command(cmdtable)

def silent_debug(*args, **kwargs):
    """Replacement for ui.debug that silently swallows the arguments.
    Typically enabled when running the mercurial test suite by setting:
    --extra-config-opt=fastmanifest.silent=True"""
    pass

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

    #Add relevant remotenames to the list of interesting revs
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

class hybridmanifest(object):
    """
    Hybrid manifest that behaves like a lazy manifest.

    Initialized with one of the three:
    - flat      an existing flat manifest
    - fast      an existing fast manifest
    - loadflat  a function to load a flat manifest from disk
    """
    def __init__(self, ui, opener,
                 flat=None, fast=None, loadflat=None, node=None):
        self.__flatmanifest = flat
        self.__cachedmanifest = fast
        self.loadflat = loadflat

        assert (self.__flatmanifest is not None or
                self.__cachedmanifest is None or
                self.loadflat is None)

        self.ui = ui
        self.opener = opener
        self.node = node

        self.cachekey = revlog.hex(self.node) if self.node is not None else None

        self.fastcache = fastmanifestcache.getinstance(opener, self.ui)
        self.debugfastmanifest = (self.ui.configbool("fastmanifest",
                                                     "debugfastmanifest")
                                  if self.ui is not None
                                  else False)

        self.incache = True if self.__cachedmanifest is not None else None

        if self.ui is None or self.ui.configbool("fastmanifest", "silent"):
            self.debug = silent_debug
        else:
            self.debug = self.ui.debug

    def _flatmanifest(self):
        if self.__flatmanifest is None:
            if self.loadflat is not None:
                # Load the manifest and cache it.
                self.__flatmanifest = self.loadflat()

                if isinstance(self.__flatmanifest, hybridmanifest):
                    # See comment in extsetup to see why we have to do that
                    self.__flatmanifest = self.__flatmanifest._flatmanifest()
            elif self.__cachedmanifest is not None:
                # build a flat manifest from the text of the fastmanifest.
                self.__flatmanifest = manifest.manifestdict(
                    self.__cachedmanifest.text())

            assert isinstance(self.__flatmanifest, manifest.manifestdict)
        return self.__flatmanifest

    def _cachedmanifest(self):
        if self.incache is None:
            # Cache lookup
            if (self.cachekey is not None and
                self.fastcache.containsnode(self.cachekey)):
                self.__cachedmanifest = self.fastcache.get(self.cachekey)
            elif self.node == revlog.nullid:
                fm = fastmanifest_wrapper.fastManifest()
                self.__cachedmanifest = fastmanifestdict(fm)
            elif self.debugfastmanifest:
                # in debug mode, we always convert into a fastmanifest.
                r = self._flatmanifest()
                fm = fastmanifest_wrapper.fastManifest(r.text())
                self.__cachedmanifest = fastmanifestdict(fm)

            self.incache = self.__cachedmanifest is not None

            self.debug("cache %s for fastmanifest %s\n"
                       % ("hit" if self.incache else "miss", self.cachekey))

        return self.__cachedmanifest

    def _incache(self):
        if self.incache or self.debugfastmanifest:
            return True
        elif self.cachekey:
            return self.fastcache.containsnode(self.cachekey)
        return False

    def _manifest(self, operation):
        # Get the manifest most suited for the operations (flat or cached)
        # TODO: return fastmanifest when suitable
        c = self._cachedmanifest()
        if c is not None:
            return c

        r = self._flatmanifest()

        return r

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
        copy = self._manifest('copy').copy()
        if isinstance(copy, hybridmanifest):
            return copy
        elif isinstance(copy, fastmanifestdict):
            return hybridmanifest(self.ui, self.opener, fast=copy,
                                  node=self.node)
        elif isinstance(copy, manifest.manifestdict):
            return hybridmanifest(self.ui, self.opener, flat=copy,
                                  node=self.node)
        else:
            raise ValueError("unknown manifest type {0}".format(type(copy)))

    def matches(self, *args, **kwargs):
        matches = self._manifest('matches').matches(*args, **kwargs)
        if isinstance(matches, hybridmanifest):
            return matches
        elif isinstance(matches, fastmanifestdict):
            return hybridmanifest(self.ui, self.opener, fast=matches)
        elif isinstance(matches, manifest.manifestdict):
            return hybridmanifest(self.ui, self.opener, flat=matches)
        else:
            raise ValueError("unknown manifest type {0}".format(type(matches)))

    def diff(self, m2, *args, **kwargs):
        self.debug("performing diff\n")
        # Find _m1 and _m2 of the same type, to provide the fastest computation
        _m1, _m2 = None, None

        if isinstance(m2, hybridmanifest):
            self.debug("diff: other side is hybrid manifest\n")
            # CACHE HIT
            if self._incache() and m2._incache():
                _m1, _m2 = self._cachedmanifest(), m2._cachedmanifest()
                # _m1 or _m2 can be None if _incache was True if the cache
                # got garbage collected in the meantime or entry is corrupted
                if _m1 is None or _m2 is None:
                    self.debug("diff: unable to load one or "
                               "more manifests\n")
                    _m1, _m2 = self._flatmanifest(), m2._flatmanifest()
            # CACHE MISS
            else:
                self.debug("diff: cache miss\n")
                _m1, _m2 = self._flatmanifest(), m2._flatmanifest()
        else:
            # This happens when diffing against a new manifest (like rev -1)
            self.debug("diff: other side not hybrid manifest\n")
            _m1, _m2 = self._flatmanifest(), m2

        assert type(_m1) == type(_m2)
        return _m1.diff(_m2, *args, **kwargs)


class fastmanifestcache(object):
    _instance = None
    @classmethod
    def getinstance(cls, opener, ui):
        if not cls._instance:
            cls._instance = fastmanifestcache(opener, ui)
        return cls._instance

    def __init__(self, opener, ui):
        self.opener = opener
        self.ui = ui
        self.inmemorycache = {}
        base = opener.join(None)
        self.cachepath = os.path.join(base, CACHE_SUBDIR)
        if not os.path.exists(self.cachepath):
            os.makedirs(self.cachepath)
        if self.ui is None or self.ui.configbool("fastmanifest", "silent"):
            self.debug = silent_debug
        else:
            self.debug = self.ui.debug

    def keyprefix(self):
        return "fast"

    def load(self, fpath):
        try:
            fm = fastmanifest_wrapper.fastManifest.load(fpath)
            # touch on access to make this cache a LRU cache
            os.utime(fpath, None)
        except EnvironmentError:
            return None
        else:
            return fastmanifestdict(fm)

    def dump(self, fpath, manifest):
        # TODO: is this already a hybridmanifest/fastmanifest?  if so, we may be
        # able to skip a frivolous conversion step.
        fm = fastmanifest_wrapper.fastManifest(manifest.text())
        fm.save(fpath)

    def inmemorycachekey(self, hexnode):
        return (self.keyprefix(), hexnode)

    def filecachepath(self, hexnode):
        return os.path.join(self.cachepath, self.keyprefix() + hexnode)

    def refresh(self, hexnode, delay=0):
        filetime = time.time() - delay
        path = self.filecachepath(hexnode)
        try:
            os.utime(path, (filetime, filetime))
        except EnvironmentError:
            pass

    def get(self, hexnode):
        # In memory cache lookup
        ident = self.inmemorycachekey(hexnode)
        r = self.inmemorycache.get(ident, None)
        if r:
            return r

        # On disk cache lookup
        realfpath = self.filecachepath(hexnode)
        r = self.load(realfpath)

        # In memory cache update
        if r:
            self.inmemorycache[ident] = r
        return r

    def containsnode(self, hexnode):
        if self.inmemorycachekey(hexnode) in self.inmemorycache:
            return True
        return os.path.exists(self.filecachepath(hexnode))

    def put(self, hexnode, manifest, limit=None):
        # Is there no more space already?
        if limit is not None:
            cachesize = self.totalsize()[0]
            allowedspace = limit.bytes() - cachesize
            if allowedspace < 0:
                return False

        if self.containsnode(hexnode):
            self.debug("skipped %s, already cached\n" % hexnode)
        else:
            self.debug("caching revision %s\n" % hexnode)

            realfpath = self.filecachepath(hexnode)
            tmpfpath = util.mktempcopy(realfpath, True)
            try:
                self.dump(tmpfpath, manifest)
                newsize = os.path.getsize(tmpfpath)

                # Inserting the entry would make the cache overflow
                if limit is not None and newsize + cachesize > limit.bytes():
                    return False

                util.rename(tmpfpath, realfpath)
                return True
            finally:
                try:
                    os.unlink(tmpfpath)
                except OSError:
                    pass

    def __iter__(self):
        for f in sorted(os.listdir(self.cachepath)):
            if f.startswith(self.keyprefix()):
                yield f

    def entrysize(self, f):
        try:
            return os.path.getsize(os.path.join(self.cachepath, f))
        except EnvironmentError:
            return None

    def totalsize(self, silent=True):
        totalsize = 0
        numentries = 0
        for entry in self:
            entrysize = self.entrysize(entry)
            if entrysize == -1:
                # Entry was deleted by another process
                continue
            totalsize += entrysize
            numentries += 1
            if not silent:
                msg = "%s (size %s)\n" % (entry, util.bytecount(entrysize))
                self.ui.status((msg))
        return totalsize, numentries

    def prune(self, limit):
        # Get the list of entries and mtime first to avoid race condition
        entries = []
        for entry in self:
            try:
                path = os.path.join(self.cachepath, entry)
                entries.append((entry, os.path.getmtime(path),
                                      os.path.getsize(path)))
            except EnvironmentError:
                pass
        # Do nothing, we don't exceed the limit
        if limit.bytes() > sum([e[2] for e in entries]):
            self.debug("nothing to do, cache size < limit\n")
            return
        # [most recently accessed, second most recently accessed ...]
        entriesbyage = sorted(entries, key=lambda x:(-x[1],x[0]))

        # We traverse the list of entries from the newest to the oldest
        # and once we hit the limit of what we can keep, we stop and
        # trim what is above the limit
        sizetokeep = 0
        startindextodiscard = 0
        for i, entry in enumerate(entriesbyage):
            if sizetokeep + entry[2] > limit.bytes():
                startindextodiscard = i
                break
            sizetokeep += entry[2]

        for entry in entriesbyage[startindextodiscard:]:
            self.pruneentrybyfname(entry[0])

    def pruneentrybyfname(self, fname):
        self.debug("removing cached manifest %s\n" % fname)
        try:
            os.unlink(os.path.join(self.cachepath, fname))
        except EnvironmentError:
            pass

    def pruneentry(self, hexnode):
        self.pruneentrybyfname(self.filecachepath(hexnode))

    def pruneall(self):
        for f in self:
            self.pruneentrybyfname(f)

class manifestfactory(object):
    def __init__(self, ui):
        self.ui = ui

    def newmanifest(self, orig, *args, **kwargs):
        loadfn = lambda: orig(*args, **kwargs)
        return hybridmanifest(self.ui,
                              args[0].opener,
                              loadflat=loadfn)

    def read(self, orig, *args, **kwargs):
        loadfn = lambda: orig(*args, **kwargs)
        return hybridmanifest(self.ui,
                              args[0].opener,
                              loadflat=loadfn,
                              node=args[1])

# Returns true if we're the original process, returns false if we're the child
# process.
def fork_worker(ui, repo):
    pid = os.fork()
    if pid > 0:
        return True

    # we're not closing the prior descriptors because that would cause a flush,
    # and we don't want that.
    ui.fin = sys.stdin = open(os.devnull, "r")
    ui.fout = ui.ferr = sys.stdout = sys.stderr = open(os.devnull, "w")
    repo.ui = ui
    os.setsid()
    pid = os.fork()
    if pid > 0:
        os._exit(0)

    return False

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
    def __init__(self, repo):
        # Probe the system root partition to know what is available
        st = os.statvfs(repo.root)
        self.free = st.f_bavail * st.f_frsize
        self.total = st.f_blocks * st.f_frsize
        # Read parameters from config
        self.lowgrowththresholdgb = repo.ui.config("fastmanifest",
                                                   "lowgrowththresholdgb",
                                                   DEFAULT_LOWGROWTH_TRESHOLDGB)
        self.lowgrowthslope = repo.ui.config("fastmanifest",
                                           "lowgrowthslope",
                                           DEFAULT_LOWGROWTH_SLOPE)
        self.maxcachesizegb = repo.ui.config("fastmanifest",
                                             "maxcachesizegb",
                                             DEFAULT_MAXCACHESIZEGB)
        self.highgrowthslope = repo.ui.config("fastmanifest",
                                              "highgrowthslope",
                                              DEFAULT_HIGHGROWTHSLOPE)
        try:
            self.lowgrowththresholdgb = float(self.lowgrowththresholdgb)
            self.lowgrowthslope = float(self.lowgrowthslope)
            self.maxcachesizegb = float(self.maxcachesizegb)
            self.highgrowthslope = float(self.highgrowthslope)
        except ValueError:
            self.lowgrowththresholdgb = DEFAULT_LOWGROWTH_TRESHOLDGB
            self.lowgrowthslope = DEFAULT_LOWGROWTH_SLOPE
            self.maxcachesizegb = DEFAULT_MAXCACHESIZEGB
            self.highgrowthslope = DEFAULT_HIGHGROWTHSLOPE

    def bytes(self):
        return systemawarecachelimit.cacheallocation(
                               self.free,
                               lowgrowththresholdgb=self.lowgrowththresholdgb,
                               highgrowthslope=self.highgrowthslope,
                               maxcachesizegb=self.maxcachesizegb,
                               lowgrowthslope=self.lowgrowthslope)

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


def _cachemanifestpruneall(ui, repo):
    cache = fastmanifestcache.getinstance(repo.store.opener, ui)
    cache.pruneall()

def _cachemanifestlist(ui, repo):
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

def _cachemanifestfillandtrim(ui, repo, revset, limit, background):
    if background:
        if fork_worker(ui, repo):
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

    mannodes = {}
    for rev in revs:
        mannode = revlog.hex(repo.changelog.changelogrevision(rev).manifest)
        mannodes[rev] = mannode
        if cache.containsnode(mannode):
            ui.debug("skipped %s, already cached (fast path)\n" % mannode)
            # Account for the fact that we access this manifest
            cache.refresh(mannode)
            continue
        manifest = repo[rev].manifest()
        if not cache.put(mannode, manifest, limit):
            # Insertion failed because cache is full
            del mannodes[rev]
            break

    # Make the least relevant entries have an artificially older mtime
    # than the more relevant ones. We use a resolution of 2 for time to work
    # accross all platforms and ensure that the order is marked.
    # Note that we use sortedrevs and not revs because here we don't care about
    # the shuffling, we just want the most relevant revisions to have more
    # recent mtime.
    mtimemultiplier = 2
    for offset, rev in enumerate(sortedrevs):
        if rev in mannodes:
            hexnode = mannodes[rev]
            cache.refresh(hexnode, delay=offset * mtimemultiplier)
        else:
            pass # We didn't have enough space for that rev

    if limit is not None:
        cache.prune(limit)

    if background:
        os._exit(0)

class fastmanifestdict(object):
    def __init__(self, fm):
        self._fm = fm

    def __getitem__(self, key):
        return self._fm[key][0]

    def find(self, key):
        return self._fm[key]

    def __len__(self):
        return len(self._fm)

    def __setitem__(self, key, node):
        if len(node) == 22:
            # sometimes we set the 22nd byte.  this is not preserved by
            # lazymanifest or manifest::_lazymanifest.
            node = node[:21]
        self._fm[key] = node, self.flags(key, '')

    def __contains__(self, key):
        return key in self._fm

    def __delitem__(self, key):
        del self._fm[key]

    def __iter__(self):
        return self._fm.__iter__()

    def iterkeys(self):
        return self._fm.iterkeys()

    def iterentries(self):
        return self._fm.iterentries()

    def iteritems(self):
        return (x[:2] for x in self._fm.iterentries())

    def keys(self):
        return list(self.iterkeys())

    def filesnotin(self, m2):
        '''Set of files in this manifest that are not in the other'''
        diff = self.diff(m2)
        files = set(filepath
                    for filepath, hashflags in diff.iteritems()
                    if hashflags[1][0] is None)
        return files

    @util.propertycache
    def _dirs(self):
        return util.dirs(self)

    def dirs(self):
        return self._dirs

    def hasdir(self, dir):
        return dir in self._dirs

    def _filesfastpath(self, match):
        '''Checks whether we can correctly and quickly iterate over matcher
        files instead of over manifest files.'''
        files = match.files()
        return (len(files) < 100 and (match.isexact() or
            (match.prefix() and all(fn in self for fn in files))))

    def walk(self, match):
        '''Generates matching file names.

        Equivalent to manifest.matches(match).iterkeys(), but without creating
        an entirely new manifest.

        It also reports nonexistent files by marking them bad with match.bad().
        '''
        if match.always():
            for f in iter(self):
                yield f
            return

        fset = set(match.files())

        # avoid the entire walk if we're only looking for specific files
        if self._filesfastpath(match):
            for fn in sorted(fset):
                yield fn
            return

        for fn in self:
            if fn in fset:
                # specified pattern is the exact name
                fset.remove(fn)
            if match(fn):
                yield fn

        # for dirstate.walk, files=['.'] means "walk the whole tree".
        # follow that here, too
        fset.discard('.')

        for fn in sorted(fset):
            if not self.hasdir(fn):
                match.bad(fn, None)

    def matches(self, match):
        '''generate a new manifest filtered by the match argument'''
        if match.always():
            return self.copy()

        if self._filesfastpath(match):
            nfm = fastmanifest_wrapper.fastManifest()
            for fn in match.files():
                if fn in self._fm:
                    nfm[fn] = self._fm[fn]
            m = fastmanifestdict(nfm)
            return m

        nfm = self._fm.filtercopy(match)
        m = fastmanifestdict(nfm)
        return m

    def diff(self, m2, clean=False):
        '''Finds changes between the current manifest and m2.

        Args:
          m2: the manifest to which this manifest should be compared.
          clean: if true, include files unchanged between these manifests
                 with a None value in the returned dictionary.

        The result is returned as a dict with filename as key and
        values of the form ((n1,fl1),(n2,fl2)), where n1/n2 is the
        nodeid in the current/other manifest and fl1/fl2 is the flag
        in the current/other manifest. Where the file does not exist,
        the nodeid will be None and the flags will be the empty
        string.
        '''
        return self._fm.diff(m2._fm, clean)

    def setflag(self, key, flag):
        self._fm[key] = self[key], flag

    def get(self, key, default=None):
        try:
            return self._fm[key][0]
        except KeyError:
            return default

    def flags(self, key, default=''):
        try:
            return self._fm[key][1]
        except KeyError:
            return default

    def copy(self):
        c = fastmanifestdict(self._fm.copy())
        return c

    def text(self, usemanifestv2=False):
        if usemanifestv2:
            return manifest._textv2(self._fm.iterentries())
        else:
            # use (probably) native version for v1
            return self._fm.text()

    def fastdelta(self, base, changes):
        """Given a base manifest text as an array.array and a list of changes
        relative to that text, compute a delta that can be used by revlog.
        """
        delta = []
        dstart = None
        dend = None
        dline = [""]
        start = 0
        # zero copy representation of base as a buffer
        addbuf = util.buffer(base)

        changes = list(changes)
        if len(changes) < 1000:
            # start with a readonly loop that finds the offset of
            # each line and creates the deltas
            for f, todelete in changes:
                # bs will either be the index of the item or the insert point
                start, end = manifest._msearch(addbuf, f, start)
                if not todelete:
                    h, fl = self._fm[f]
                    l = "%s\0%s%s\n" % (f, revlog.hex(h), fl)
                else:
                    if start == end:
                        # item we want to delete was not found, error out
                        raise AssertionError(
                                (("failed to remove %s from manifest") % f))
                    l = ""
                if dstart is not None and dstart <= start and dend >= start:
                    if dend < end:
                        dend = end
                    if l:
                        dline.append(l)
                else:
                    if dstart is not None:
                        delta.append([dstart, dend, "".join(dline)])
                    dstart = start
                    dend = end
                    dline = [l]

            if dstart is not None:
                delta.append([dstart, dend, "".join(dline)])
            # apply the delta to the base, and get a delta for addrevision
            deltatext, arraytext = manifest._addlistdelta(base, delta)
        else:
            # For large changes, it's much cheaper to just build the text and
            # diff it.
            arraytext = array.array('c', self.text())
            deltatext = mdiff.textdiff(base, arraytext)

        return arraytext, deltatext

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
        _cachemanifestpruneall(ui, repo)
        return

    if displaylist:
        _cachemanifestlist(ui, repo)
        return

    if revset or limit:
        _cachemanifestfillandtrim(ui, repo, revset, limit, background)

def _cacheonchangeconfig(repo):
    """return revs, bg, limit suitable for caching fastmanifest on change"""
    revset = ["fastmanifesttocache()"]
    bg = repo.ui.configbool("fastmanifest",
                            "cacheonchangebackground",
                            True)
    systemlimit = repo.ui.configbool("fastmanifest",
                                     "cacheonchangesystemlimit",
                                     True)
    limit = None
    if systemlimit:
        limit = systemawarecachelimit(repo)
    return revset, bg, limit

def triggercacheonbookmarkchange(orig, self, *args, **kwargs):
    repo = self._repo
    revset, bg, limit = _cacheonchangeconfig(repo)
    _cachemanifestfillandtrim(repo.ui, repo, revset, limit, bg)
    return orig(self, *args, **kwargs)

def triggercacheondirstatechange(orig, self, *args, **kwargs):
    if util.safehasattr(self, "_fastmanifestrepo"):
        repo = self._fastmanifestrepo
        revset, bg, limit = _cacheonchangeconfig(repo)
        _cachemanifestfillandtrim(repo.ui, repo, revset, limit, bg)
    return orig(self, *args, **kwargs)

def triggercacheonremotenameschange(orig, repo, *args, **kwargs):
    revset, bg, limit = _cacheonchangeconfig(repo)
    _cachemanifestfillandtrim(repo.ui, repo, revset, limit, bg)
    return orig(repo, *args, **kwargs)

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

    if ui.configbool("fastmanifest", "cacheonchange", False):
        # Trigger to enable caching of relevant manifests
        extensions.wrapfunction(bookmarks.bmstore, '_write',
                                triggercacheonbookmarkchange)
        extensions.wrapfunction(dirstate.dirstate, 'write',
                                 triggercacheondirstatechange)
        try:
            remotenames = extensions.find('remotenames')
        except KeyError:
            pass
        else:
            if remotenames:
                extensions.wrapfunction(remotenames, 'saveremotenames',
                                        triggercacheonremotenameschange)

        def wrapdirstate(orig, self):
            dirstate = orig(self)
            dirstate._fastmanifestrepo = self
            return dirstate
        wrapfilecache(localrepo.localrepository, 'dirstate',
                                 wrapdirstate)
