# implementation.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import

import collections
import heapq
import os
import time

from edenscm.mercurial import error, manifest, mdiff, revlog, util
from edenscmnative import cfastmanifest, cstore

from .constants import CACHE_SUBDIR, DEFAULT_MAX_MEMORY_ENTRIES
from .metrics import metricscollector


supportsctree = True

propertycache = util.propertycache


class hybridmanifest(object):
    """
    Hybrid manifest that behaves like a lazy manifest.

    Initialized with one of the three:
    - flat      an existing flat manifest
    - fast      an existing fast manifest
    - loadflat  a function to load a flat manifest from disk
    """

    def __init__(
        self,
        ui,
        opener,
        manifestlog,
        flat=None,
        fast=None,
        loadflat=None,
        tree=None,
        node=None,
    ):
        self.__flatmanifest = flat
        self.loadflat = loadflat

        if supportsctree and ui.configbool("fastmanifest", "usetree"):
            self.__treemanifest = tree
        else:
            self.__treemanifest = False

        if ui.configbool("fastmanifest", "usecache"):
            self.__cachedmanifest = fast
        else:
            self.__cachedmanifest = False

        assert (
            self.__flatmanifest is not None
            or self.__cachedmanifest not in (None, False)
            or self.__treemanifest not in (None, False)
            or self.loadflat is not None
        )

        self.ui = ui
        self.opener = opener
        self.manifestlog = manifestlog
        self.node = node
        self.basemanifest = None

        self.cachekey = revlog.hex(self.node) if self.node is not None else None

        self.fastcache = fastmanifestcache.getinstance(opener, self.ui)
        self.treecache = treemanifestcache.getinstance(opener, self.ui)
        self.debugfastmanifest = self.ui.configbool(
            "fastmanifest", "debugfastmanifest", False
        )

        self.incache = True if self.__cachedmanifest not in (None, False) else None

        if self.ui.configbool("fastmanifest", "silent"):
            self.debug = _silent_debug
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
            elif self.__cachedmanifest not in (None, False):
                # build a flat manifest from the text of the fastmanifest.
                self.__flatmanifest = manifest.manifestdict(
                    self.__cachedmanifest.text()
                )
            elif self.__treemanifest not in (None, False):
                # build a flat manifest from the text of the fastmanifest.
                self.__flatmanifest = manifest.manifestdict(self.__treemanifest.text())

            assert isinstance(self.__flatmanifest, manifest.manifestdict)
        return self.__flatmanifest

    def _cachedmanifest(self):
        if self.__cachedmanifest is False:
            return None

        if self.incache is None:
            # Cache lookup
            if self.cachekey is not None and self.cachekey in self.fastcache:
                self.__cachedmanifest = self.fastcache[self.cachekey]
            elif self.node == revlog.nullid:
                fm = cfastmanifest.fastmanifest()
                self.__cachedmanifest = fastmanifestdict(fm)
            elif self.debugfastmanifest:
                # in debug mode, we always convert into a fastmanifest.
                r = self._flatmanifest()
                fm = cfastmanifest.fastmanifest(r.text())
                self.__cachedmanifest = fastmanifestdict(fm)

            self.incache = self.__cachedmanifest is not None
            metricscollector.get().recordsample(
                "cachehit", hit=self.incache, node=self.cachekey
            )
            self.debug(
                "[FM] cache %s for fastmanifest %s\n"
                % ("hit" if self.incache else "miss", self.cachekey)
            )

        return self.__cachedmanifest

    def _treemanifest(self):
        if self.__treemanifest is False:
            return None
        assert supportsctree
        if self.__treemanifest is None:
            if self.node is None:
                return None
            if self.node in self.treecache:
                self.__treemanifest = self.treecache[self.node]
            elif self.node == revlog.nullid:
                store = self.manifestlog.datastore
                self.__treemanifest = cstore.treemanifest(store)
            else:
                store = self.manifestlog.datastore
                self.ui.pushbuffer()
                try:
                    store.get("", self.node)
                    self.__treemanifest = cstore.treemanifest(store, self.node)
                    # The buffer is only to eat certain errors, so show
                    # non-error messages.
                    output = self.ui.popbuffer()
                    if output:
                        self.ui.status(output)
                except (KeyError, error.Abort):
                    # Record that it doesn't exist, so we don't keep checking
                    # the store.
                    self.ui.popbuffer()
                    # Eat the buffer so we don't print a remote: warning
                    self.__treemanifest = False
                    return None
                except Exception:
                    # Other errors should be printed
                    output = self.ui.popbuffer()
                    if output:
                        self.ui.status(output)
                    raise

        return self.__treemanifest

    def _incache(self):
        if self.incache or self.debugfastmanifest:
            return True
        elif self.cachekey:
            return self.cachekey in self.fastcache
        return False

    def _manifest(self, operation):
        # Get the manifest most suited for the operations (flat or cached)
        # TODO: return fastmanifest when suitable
        c = self._cachedmanifest()
        if c is not None:
            return c

        t = self._treemanifest()
        if t is not None:
            return t

        r = self._flatmanifest()
        return r

    # Proxy all the manifest methods to the flatmanifest except magic methods
    def __getattr__(self, name):
        return getattr(self._manifest(name), name)

    # Magic methods should be proxied differently than __getattr__
    # For the moment all methods they all use the _flatmanifest
    def __iter__(self):
        return self._manifest("__iter__").__iter__()

    def __contains__(self, key):
        return self._manifest("__contains__").__contains__(key)

    def __getitem__(self, key):
        return self._manifest("__getitem__").__getitem__(key)

    def __setitem__(self, key, val):
        return self._manifest("__setitem__").__setitem__(key, val)

    def __delitem__(self, key):
        return self._manifest("__delitem__").__delitem__(key)

    def __nonzero__(self):
        return bool(self._manifest("__nonzero__"))

    __bool__ = __nonzero__

    def __len__(self):
        return len(self._manifest("__len__"))

    def text(self, *args, **kwargs):
        # Normally we would prefer treemanifest instead of flat, but for text()
        # flat is actually faster for now.
        m = self._cachedmanifest()
        if m is None:
            m = self._flatmanifest()
        if m is None:
            m = self._treemanifest()
        return m.text(*args, **kwargs)

    def fastdelta(self, base, changes):
        m = self._manifest("fastdelta")
        if isinstance(m, cstore.treemanifest):
            return fastdelta(m, m.find, base, changes)
        return m.fastdelta(base, changes)

    def _converttohybridmanifest(self, m):
        if isinstance(m, hybridmanifest):
            return m
        elif isinstance(m, fastmanifestdict):
            return hybridmanifest(self.ui, self.opener, self.manifestlog, fast=m)
        elif isinstance(m, manifest.manifestdict):
            return hybridmanifest(self.ui, self.opener, self.manifestlog, flat=m)
        elif supportsctree and isinstance(m, cstore.treemanifest):
            return hybridmanifest(self.ui, self.opener, self.manifestlog, tree=m)
        else:
            raise ValueError("unknown manifest type {0}".format(type(m)))

    def copy(self):
        copy = self._manifest("copy").copy()
        hybridmf = self._converttohybridmanifest(copy)
        hybridmf.basemanifest = self.basemanifest or self
        return hybridmf

    def matches(self, *args, **kwargs):
        matches = self._manifest("matches").matches(*args, **kwargs)
        hybridmf = self._converttohybridmanifest(matches)
        hybridmf.basemanifest = self.basemanifest or self
        return hybridmf

    def _getmatchingtypemanifest(self, m2, operation):
        # Find _m1 and _m2 of the same type, to provide the fastest computation
        if isinstance(m2, hybridmanifest):
            self.debug("[FM] %s: other side is hybrid manifest\n" % operation)

            # Best case: both are in the cache
            if self._incache() and m2._incache():
                cachedmf1 = self._cachedmanifest()
                cachedmf2 = m2._cachedmanifest()
                if cachedmf1 is not None and cachedmf2 is not None:
                    return cachedmf1, cachedmf2, True

            # Second best: both are trees
            treemf1 = self._treemanifest()
            treemf2 = m2._treemanifest()
            if treemf1 is not None and treemf2 is not None:
                self.debug(("[FM] %s: loaded matching tree " "manifests\n") % operation)
                return treemf1, treemf2, False

            # Third best: one tree, one computed tree
            def canbuildtree(m):
                return (
                    m._cachedmanifest() is not None
                    and m.basemanifest is not None
                    and m.basemanifest._cachedmanifest() is not None
                    and m.basemanifest._treemanifest() is not None
                )

            def buildtree(m):
                diff = m.basemanifest._cachedmanifest().diff(m._cachedmanifest())
                temptreemf = m.basemanifest._treemanifest().copy()
                for f, ((an, af), (bn, bf)) in diff.iteritems():
                    temptreemf.set(f, bn, bf)
                return temptreemf

            if treemf1 is not None and canbuildtree(m2):
                self.debug(
                    ("[FM] %s: computed matching tree " "manifests\n") % operation
                )
                return treemf1, buildtree(m2), False
            elif treemf2 is not None and canbuildtree(self):
                self.debug(
                    ("[FM] %s: computed matching tree " "manifests\n") % operation
                )
                return buildtree(self), treemf2, False

            # Worst: both flat
            self.debug("[FM] %s: cache and tree miss\n" % operation)
            return self._flatmanifest(), m2._flatmanifest(), False
        else:
            # This happens when diffing against a new manifest (like rev -1)
            self.debug("[FM] %s: other side not hybrid manifest\n" % operation)
            return self._flatmanifest(), m2, False

    def diff(self, m2, *args, **kwargs):
        self.debug("[FM] performing diff\n")
        _m1, _m2, hit = self._getmatchingtypemanifest(m2, "diff")
        metricscollector.get().recordsample("diffcachehit", hit=hit)
        return _m1.diff(_m2, *args, **kwargs)

    def filesnotin(self, m2, *args, **kwargs):
        self.debug("[FM] performing filesnotin\n")
        _m1, _m2, hit = self._getmatchingtypemanifest(m2, "filesnotin")
        metricscollector.get().recordsample("filesnotincachehit", hit=hit)
        return _m1.filesnotin(_m2, *args, **kwargs)


class fastmanifestdict(object):
    def __init__(self, fm):
        self._fm = fm

    def __getitem__(self, key):
        return self._fm[key][0]

    def find(self, key):
        return self._fm[key]

    def __nonzero__(self):
        for x in self:
            return True
        return False

    __bool__ = __nonzero__

    def __len__(self):
        return len(self._fm)

    def __setitem__(self, key, node):
        if len(node) == 22:
            # sometimes we set the 22nd byte.  this is not preserved by
            # lazymanifest or manifest::_lazymanifest.
            node = node[:21]
        self._fm[key] = node, self.flags(key, "")

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
        # TODO: we can improve the speed of this by making it return the
        # right thing from the native code
        return (x[:2] for x in self._fm.iterentries())

    def keys(self):
        return list(self.iterkeys())

    def filesnotin(self, m2, matcher=None):
        """Set of files in this manifest that are not in the other"""
        diff = self.diff(m2, matcher=matcher)
        files = set(
            filepath
            for filepath, hashflags in diff.iteritems()
            if hashflags[1][0] is None
        )
        return files

    @util.propertycache
    def _dirs(self):
        return util.dirs(self)

    def dirs(self):
        return self._dirs

    def hasdir(self, dir):
        return dir in self._dirs

    def _filesfastpath(self, match):
        """Checks whether we can correctly and quickly iterate over matcher
        files instead of over manifest files."""
        files = match.files()
        return len(files) < 100 and (
            match.isexact() or (match.prefix() and all(fn in self for fn in files))
        )

    def walk(self, match):
        """Generates matching file names.

        Equivalent to manifest.matches(match).iterkeys(), but without creating
        an entirely new manifest.

        It also reports nonexistent files by marking them bad with match.bad().
        """
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
        fset.discard(".")

        for fn in sorted(fset):
            if not self.hasdir(fn):
                match.bad(fn, None)

    def matches(self, match):
        """generate a new manifest filtered by the match argument"""
        if match.always():
            return self.copy()

        if self._filesfastpath(match):
            nfm = cfastmanifest.fastmanifest()
            for fn in match.files():
                if fn in self._fm:
                    nfm[fn] = self._fm[fn]
            m = fastmanifestdict(nfm)
            return m

        nfm = self._fm.filtercopy(match)
        m = fastmanifestdict(nfm)
        return m

    def diff(self, m2, matcher=None):
        """Finds changes between the current manifest and m2.

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
        """
        if matcher:
            mf1 = self.matches(matcher)
            mf2 = m2.matches(matcher)
            return mf1.diff(mf2)
        return self._fm.diff(m2._fm)

    def setflag(self, key, flag):
        self._fm[key] = self[key], flag

    def get(self, key, default=None):
        try:
            return self._fm[key][0]
        except KeyError:
            return default

    def flags(self, key, default=""):
        try:
            return self._fm[key][1]
        except KeyError:
            return default

    def copy(self):
        c = fastmanifestdict(self._fm.copy())
        return c

    def text(self):
        # use (probably) native version for v1
        return self._fm.text()

    def fastdelta(self, base, changes):
        """Given a base manifest text as an array.array and a list of changes
        relative to that text, compute a delta that can be used by revlog.
        """
        return fastdelta(self, self._fm.__getitem__, base, changes)


def fastdelta(mf, mfgetter, base, changes):
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
                h, fl = mfgetter(f)
                l = "%s\0%s%s\n" % (f, revlog.hex(h), fl)
            else:
                if start == end:
                    # item we want to delete was not found, error out
                    raise AssertionError((("failed to remove %s from manifest") % f))
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
        arraytext = bytearray(mf.text())
        deltatext = mdiff.textdiff(util.buffer(base), util.buffer(arraytext))

    return arraytext, deltatext


class ondiskcache(object):
    def __init__(self, debugf, opener, ui):
        self.debugf = debugf
        self.opener = opener
        self.ui = ui
        self.pathprefix = "fast"
        base = opener.join(None)
        self.cachepath = os.path.join(base, CACHE_SUBDIR)
        if not os.path.exists(self.cachepath):
            try:
                os.makedirs(self.cachepath)
            except EnvironmentError:
                # Likely permission issues, in that case, we won't be able to
                # access the cache afterwards
                pass

    def _pathfromnode(self, hexnode):
        return os.path.join(self.cachepath, self.pathprefix + hexnode)

    def touch(self, hexnode, delay=0):
        filetime = time.time() - delay
        path = self._pathfromnode(hexnode)
        try:
            self.debugf("[FM] refreshing %s with delay %d\n" % (hexnode, delay))
            os.utime(path, (filetime, filetime))
        except EnvironmentError:
            pass

    def __contains__(self, hexnode):
        path = self._pathfromnode(hexnode)
        return os.path.exists(path)

    def items(self):
        """Return the entries in the cache, sorted from most relevant to least
        relevant"""
        entries = []
        for entry in os.listdir(self.cachepath):
            try:
                if entry.startswith(self.pathprefix):
                    path = os.path.join(self.cachepath, entry)
                    entries.append(
                        (entry, os.path.getmtime(path), os.path.getsize(path))
                    )
            except EnvironmentError:
                pass
        entries.sort(key=lambda x: (-x[1], x[0]))
        return [x[0].replace(self.pathprefix, "") for x in entries]

    def __iter__(self):
        return iter(self.items())

    def setwithlimit(self, hexnode, manifest, limit=-1):
        """Writes a manifest to the cache.  Returns True if the cache already
        contains the item or if the write is successful.  Returns False if the
        write fails.  Raises CacheFullException if writing the cache entry would
        cause us to pass the limit.
        """
        if hexnode in self:
            return True
        path = self._pathfromnode(hexnode)
        if isinstance(manifest, cfastmanifest.fastmanifest) or isinstance(
            manifest, fastmanifestdict
        ):
            fm = manifest
        else:
            fm = cfastmanifest.fastmanifest(manifest.text())
        tmpfpath = util.mktempcopy(path, True)
        entrysize = fm.bytes()
        if limit != -1 and self.totalsize()[0] + entrysize > limit:
            raise CacheFullException()
        try:
            fm._save(tmpfpath)
            util.rename(tmpfpath, path)
            return True
        except EnvironmentError:
            return False
        finally:
            try:
                os.unlink(tmpfpath)
            except OSError:
                pass

    def __setitem__(self, hexnode, manifest):
        self.setwithlimit(hexnode, manifest)

    def __delitem__(self, hexnode):
        path = self._pathfromnode(hexnode)
        try:
            os.unlink(path)
        except EnvironmentError:
            pass

    def __getitem__(self, hexnode):
        path = self._pathfromnode(hexnode)
        try:
            fm = cfastmanifest.fastmanifest.load(path)
            # touch on access to make this cache a LRU cache
            os.utime(path, None)
        except EnvironmentError:
            return None
        else:
            return fastmanifestdict(fm)

    def entrysize(self, hexnode):
        try:
            return os.path.getsize(self._pathfromnode(hexnode))
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
                msg = "%s (size %s)\n" % (
                    self.pathprefix + entry,
                    util.bytecount(entrysize),
                )
                self.ui.status(msg)
        return totalsize, numentries


class CacheFullException(Exception):
    pass


class treemanifestcache(object):
    @staticmethod
    def getinstance(opener, ui):
        if not util.safehasattr(opener, "treemanifestcache"):
            opener.treemanifestcache = treemanifestcache(opener, ui)
        return opener.treemanifestcache

    def __init__(self, opener, ui):
        self.ui = ui
        self._cache = {}

    def clear(self):
        self._cache.clear()

    def __contains__(self, node):
        return node in self._cache

    def get(self, node, default=None):
        return self._cache.get(node, default=default)

    def __getitem__(self, node):
        return self._cache[node]

    def __setitem__(self, node, value):
        self._cache[node] = value


class fastmanifestcache(object):
    @staticmethod
    def getinstance(opener, ui):
        if not util.safehasattr(opener, "fastmanifestcache"):
            # Avoid circular imports
            from . import cachemanager

            limit = cachemanager._systemawarecachelimit(opener=opener, ui=ui)
            opener.fastmanifestcache = fastmanifestcache(opener, ui, limit)
        return opener.fastmanifestcache

    def __init__(self, opener, ui, limit):
        self.ui = ui
        if self.ui.configbool("fastmanifest", "silent"):
            self.debug = _silent_debug
        else:
            self.debug = self.ui.debug
        self.ondiskcache = ondiskcache(self.debug, opener, ui)
        maxinmemoryentries = self.ui.config(
            "fastmanifest", "maxinmemoryentries", DEFAULT_MAX_MEMORY_ENTRIES
        )
        self.inmemorycache = util.lrucachedict(maxinmemoryentries)
        self.limit = limit

    def overridelimit(self, limiter):
        self.limit = limiter

    def touch(self, hexnode, delay=0):
        self.ondiskcache.touch(hexnode, delay)

    def __getitem__(self, hexnode):
        if hexnode in self.inmemorycache:
            return self.inmemorycache[hexnode]

        r = self.ondiskcache[hexnode]
        if r:
            self.inmemorycache[hexnode] = r
        return r

    def __contains__(self, hexnode):
        return hexnode in self.inmemorycache or hexnode in self.ondiskcache

    def __setitem__(self, hexnode, manifest):
        if hexnode in self.ondiskcache and hexnode in self.inmemorycache:
            self.debug("[FM] skipped %s, already cached\n" % hexnode)
            return

        if self.limit:
            if self.ondiskcache.totalsize()[0] > self.limit.bytes():
                self.debug("[FM] skipped %s, cache full\n" % hexnode)
            else:
                self.debug("[FM] caching revision %s\n" % hexnode)
                self.ondiskcache.setwithlimit(hexnode, manifest, self.limit.bytes())
        else:
            self.debug("[FM] caching revision %s\n" % hexnode)
            self.ondiskcache[hexnode] = manifest
        self.put_inmemory(hexnode, manifest)

    def put_inmemory(self, hexnode, fmdict):
        if hexnode not in self.inmemorycache:
            self.inmemorycache[hexnode] = fmdict.copy()

    def __iter__(self):
        return self.ondiskcache.__iter__()

    def prune(self):
        return self.makeroomfor(0, set())

    def pruneall(self):
        for entry in reversed(list(self.ondiskcache)):
            self.debug("[FM] removing cached manifest fast%s\n" % entry)
            del self.ondiskcache[entry]

    def makeroomfor(self, needed, excluded):
        """Make room on disk for a cache entry of size `needed`.  Cache entries
        in `excluded` are not subjected to removal.
        """
        cacheentries = collections.deque(self.ondiskcache.items())
        maxtotal = self.limit.bytes() - needed

        while len(cacheentries) > 0 and self.ondiskcache.totalsize()[0] > maxtotal:
            candidate = cacheentries.pop()

            if candidate in excluded:
                # it's immune, so skip it.
                continue

            self.debug("[FM] removing cached manifest fast%s\n" % (candidate,))
            del self.ondiskcache[candidate]


class hybridmanifestctx(object):
    """A class representing a single revision of a manifest, including its
    contents, its parent revs, and its linkrev.
    """

    def __init__(self, ui, manifestlog, revlog, node):
        self._ui = ui
        self._manifestlog = manifestlog
        self._opener = manifestlog._opener
        self._revlog = revlog
        self._node = node
        self._hybridmanifest = None

    @propertycache
    def revlog(self):
        if self._revlog is None:
            raise error.ProgrammingError(
                "cannot access flat manifest revlog " "for treeonly repository"
            )
        return self._revlog

    def copy(self):
        memmf = manifest.memmanifestctx(self._manifestlog)
        memmf._manifestdict = self.read().copy()
        return memmf

    @propertycache
    def parents(self):
        if util.safehasattr(self._manifestlog, "historystore"):
            store = self._manifestlog.historystore
            try:
                p1, p2, linknode, copyfrom = store.getnodeinfo("", self._node)
                return p1, p2
            except KeyError:
                pass
        return self.revlog.parents(self._node)

    def read(self):
        if self._hybridmanifest is None:

            def loadflat():
                # This should eventually be made lazy loaded, so consumers can
                # access the node/p1/linkrev data without having to parse the
                # whole manifest.
                data = self.revlog.revision(self._node)
                arraytext = bytearray(data)
                self.revlog._fulltextcache[self._node] = arraytext
                return manifest.manifestdict(data)

            self._hybridmanifest = hybridmanifest(
                self._ui,
                self._opener,
                self._manifestlog,
                loadflat=loadflat,
                node=self._node,
            )
        return self._hybridmanifest

    def readnew(self, shallow=False):
        """Returns the entries that were introduced by this manifest revision.

        If `shallow` is True, it returns only the immediate children in a tree.
        """
        p1, p2 = self.parents
        mf = self.read()
        parentmf = self._manifestlog[p1].read()

        treemf = mf._treemanifest()
        ptreemf = parentmf._treemanifest()
        if treemf is not None and ptreemf is not None:
            diff = ptreemf.diff(treemf)
            result = manifest.manifestdict()
            for path, ((oldn, oldf), (newn, newf)) in diff.iteritems():
                if newn is not None:
                    result[path] = newn
                    if newf:
                        result.setflag(path, newf)
            return mf._converttohybridmanifest(result)

        rl = self.revlog
        r = rl.rev(self._node)
        d = mdiff.patchtext(rl.revdiff(rl.parentrevs(r)[0], r))
        return manifest.manifestdict(d)

    def node(self):
        return self._node

    def find(self, path):
        return self.read().find(path)


class manifestfactory(object):
    def __init__(self, ui):
        self.ui = ui

    def newgetitem(self, orig, *args, **kwargs):
        mfl = args[0]
        node = args[1]
        dir = ""

        if node in mfl._dirmancache.get(dir, ()):
            return mfl._dirmancache[dir][node]

        m = hybridmanifestctx(mfl.ui, mfl, mfl._revlog, node)

        if node != revlog.nullid:
            mancache = mfl._dirmancache.get(dir)
            if mancache is None:
                mancache = util.lrucachedict(mfl.cachesize)
                mfl._dirmancache[dir] = mancache
            mancache[node] = m

        return m

    def newgetdirmanifestctx(self, orig, mfl, dir, node, *args):
        if dir != "":
            raise NotImplemented("fastmanifest doesn't support trees")

        return self.newgetitem(None, mfl, node)

    def add(self, orig, *args, **kwargs):
        origself, m, transaction, link, p1, p2, added, removed = args[:8]
        fastcache = fastmanifestcache.getinstance(origself.opener, self.ui)

        p1text = None

        p1hexnode = revlog.hex(p1)
        cacheenabled = self.ui.configbool("fastmanifest", "usecache")
        treeenabled = self.ui.configbool("fastmanifest", "usetree")

        if (
            cacheenabled
            and p1hexnode in fastcache
            and isinstance(m, hybridmanifest)
            and m._incache()
        ):
            p1text = fastcache[p1hexnode].text()
        elif treeenabled:
            tree = m._treemanifest()
            if tree is not None:
                p1text = origself.revision(p1)

        if p1text:
            manifest._checkforbidden(added)
            # combine the changed lists into one sorted iterator
            work = heapq.merge(
                [(x, False) for x in added], [(x, True) for x in removed]
            )

            # TODO: potential for optimization: avoid this silly conversion to a
            # python array.
            manifestarray = bytearray(p1text)

            arraytext, deltatext = m.fastdelta(manifestarray, work)
            cachedelta = origself.rev(p1), deltatext
            text = util.buffer(arraytext)
            node = origself.addrevision(text, transaction, link, p1, p2, cachedelta)
            hexnode = revlog.hex(node)

            # Even though we may have checked 'm._incache()' above, it may have
            # since disappeared, since background processes could be modifying
            # the cache.
            cachedmf = m._cachedmanifest()
            if cachedmf:
                fastcache.put_inmemory(hexnode, cachedmf)
                self.ui.debug("[FM] wrote manifest %s\n" % (hexnode,))
        else:
            # If neither cache could help, fallback to the normal add
            node = orig(*args, **kwargs)

        return node

    def ctxwrite(self, orig, mfctx, transaction, link, p1, p2, added, removed):
        mfl = mfctx._manifestlog
        treeenabled = mfl.ui.configbool("fastmanifest", "usetree")
        if (
            supportsctree
            and treeenabled
            and p1 not in mfl._revlog.nodemap
            and not mfl.datastore.getmissing([("", p1)])
        ):
            # If p1 is not in the flat manifest but is in the tree store, then
            # this is a commit on top of a tree only commit and we should then
            # produce a treeonly commit.
            node = None
        else:
            node = orig(mfctx, transaction, link, p1, p2, added, removed)

        if supportsctree and treeenabled:
            opener = mfctx._revlog().opener

            m = mfctx._manifestdict

            # hybridmanifest requires you provide either a value or loadflat.
            # Let's give it a dummy value, since we know we'll only be calling
            # _treemanifest()
            def loadflat():
                raise RuntimeError("no-op loadflat should never be hit")

            tree = hybridmanifest(
                mfl.ui, opener, mfl, loadflat=loadflat, node=p1
            )._treemanifest()

            if tree is not None:
                newtree = tree.copy()
                for filename in removed:
                    del newtree[filename]

                for filename in added:
                    fnode = m[filename]
                    fflag = m.flags(filename)
                    newtree.set(filename, fnode, fflag)

                tmfl = mfl.treemanifestlog

                # If the manifest was already committed as a flat manifest, use
                # its node.
                overridenode = None
                overridep1node = None
                if node is not None:
                    overridenode = node
                    overridep1node = p1

                # linknode=None because linkrev is provided
                node = tmfl.add(
                    mfl.ui,
                    newtree,
                    p1,
                    p2,
                    None,
                    overridenode=overridenode,
                    overridep1node=overridep1node,
                    tr=transaction,
                    linkrev=link,
                )

                treemanifestcache.getinstance(opener, mfl.ui)[node] = newtree

                def finalize(tr):
                    treemanifestcache.getinstance(opener, mfl.ui).clear()

                transaction.addfinalize("fastmanifesttreecache", finalize)

        return node


def _silent_debug(*args, **kwargs):
    """Replacement for ui.debug that silently swallows the arguments.
    Typically enabled when running the mercurial test suite by setting:
    --extra-config-opt=fastmanifest.silent=True"""
