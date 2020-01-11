# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""alternative changelog index

This extension replaces certain parts of changelog index algorithms to make it
more efficient when changelog is large.

Config::

    [clindex]
    # Use Rust nodemap
    nodemap = True

    # Verify operations against other implementations.
    verify = False

    # Incrementally build Rust nodemap once it misses 20k revisions
    lagthreshold = 20000

    # Path to write logs (default: $repo/.hg/cache/clindex.log)
    logpath = /tmp/a.log
"""

from __future__ import absolute_import

import datetime
import errno
import os

from edenscm.mercurial import (
    changelog,
    error,
    extensions,
    localrepo,
    policy,
    registrar,
    revlog,
    util,
    vfs as vfsmod,
)

from edenscm.mercurial.node import (
    hex,
    nullhex,
    nullid,
)

from . import parsers
import bindings

indexes = bindings.indexes
indexes.nodemap.emptyindexbuffer() # force demandimport to load indexes

RustError = bindings.error.RustError

configtable = {}
configitem = registrar.configitem(configtable)

configitem(b'clindex', b'nodemap', default=True)
configitem(b'clindex', b'verify', default=False)

# Inserting 20k nodes takes about 2ms. See https://phab.mercurial-scm.org/D1291
# for the table of node count and performance.
configitem(b'clindex', b'lagthreshold', default=20000)

# Path to write logs.
configitem(b'clindex', b'logpath', default=None)

origindextype = parsers.index

# cdef is important for performance because it avoids dict lookups:
# - `self._origindex` becomes `some_c_struct_pointer->_origindex`
# - `__getitem__`, `__len__` will be using `PyMappingMethods` APIs

cdef class clindex(object):
    cdef readonly _changelog
    cdef readonly localconfig _config
    cdef readonly nodemap _nodemap
    cdef _origindex
    cdef _vfs

    def __init__(self, data, inlined, vfs, config):
        assert not inlined
        assert vfs
        self._origindex = origindextype(data, inlined)
        self._changelog = data
        # Copy the config so it can be changed just for this clindex object.
        # For example, disabling Rust nodemap temporarily if strip happens.
        self._config = config.copy()
        self._nodemap = nodemap(self._origindex, data, vfs, config)
        self._vfs = vfs

    def ancestors(self, *revs):
        return self._origindex.ancestors(*revs)

    def commonancestorsheads(self, *revs):
        return self._origindex.commonancestorsheads(*revs)

    def __getitem__(self, int rev):
        return self._origindex[rev]

    def computephasesmapsets(self, roots):
        return self._origindex.computephasesmapsets(roots)

    def reachableroots2(self, int minroot, heads, roots, includepath):
        return self._origindex.reachableroots2(minroot, heads, roots,
                                               includepath)

    def headrevs(self):
        return self._origindex.headrevs()

    def headrevsfiltered(self, filtered):
        return self._origindex.headrevsfiltered(filtered)

    def deltachain(self, rev, stoprev, generaldelta):
        return self._origindex.deltachain(rev, stoprev, generaldelta)

    def insert(self, int rev, entry):
        if rev < 0:
            rev = len(self._origindex) + rev
        self._origindex.insert(rev, entry)
        self._nodemap[entry[-1]] = rev

    def partialmatch(self, hexnode):
        return self._nodemap.partialmatch(hexnode)

    def __len__(self):
        return len(self._origindex)

    def __delitem__(self, x):
        # This one is tricky: it's called by strip. The Rust nodemap cannot
        # really handle it easily so let's just disable it for now.
        # repo.destroyed() will reconstruct a clindex object, which will
        # re-enable and re-build the cache.
        del self._origindex[x]
        self._config.nodemap = False

    @property
    def nodemap(self):
        return self._nodemap

    def destroying(self):
        _log(self._vfs, b'clindex: destroying')
        self._nodemap.destroying()

    def updatecaches(self):
        self._nodemap.updatecache()

cdef class nodemap(object):
    """mutable nodemap

    Backed by an immutable nodemap implemented by Rust and a simple override
    dict. The Rust nodemap only follows changelog index data while the nodemap
    has to support __setitem__ to be compatible with the current Mercurial
    APIs.
    """
    cdef localconfig _config
    cdef _origindex
    cdef readonly _overrides # {node: rev | None}
    cdef readonly _rustnodemap
    cdef _vfs
    cdef readonly bint _updated

    emptyindex = indexes.nodemap.emptyindexbuffer()

    def __init__(self, origindex, changelog, vfs, config):
        self._config = config
        self._origindex = origindex
        self._overrides = {}
        self._vfs = vfs
        try:
            index = util.buffer(util.mmapread(vfs(b'nodemap', b'rb')))
            if len(index) < len(self.emptyindex):
                index = self.emptyindex
        except IOError as ex:
            if ex.errno != errno.ENOENT:
                raise
            _log(self._vfs, b'nodemap: is empty')
            index = self.emptyindex
        if config.nodemap:
            try:
                rustnodemap = indexes.nodemap(changelog, index)
            except Exception as ex:
                _log(self._vfs, b'nodemap: corrupted: %r' % ex)
                rustnodemap = indexes.nodemap(changelog, self.emptyindex)
            self._rustnodemap = rustnodemap
        self._updated = False

    def updatecache(self):
        # updatecache may get called for *many* times. That is, an "outdated"
        # changelog object being used across multiple transactions. This test
        # avoids unnecessary re-updates.
        if self._updated:
            return
        # nodemap was disabled (ex. by destroying()). The changelog is now
        # outdated. Do not rely on it building index.
        if not self._config.nodemap:
            return
        # Writing nodemap has a cost. Do not update it if not lagging too much.
        lag = self._rustnodemap.lag()
        if lag == 0 or lag < self._config.lagthreshold:
            return
        _log(self._vfs, b'nodemap: updating (lag=%s)' % lag)
        with self._vfs(b'nodemap', b'w', atomictemp=True) as f:
            f.write(self._rustnodemap.build())
        self._updated = True

    def __getitem__(self, node):
        if not self._config.nodemap:
            return self._origindex[node]

        if node == nullid:
            # special case for hg: b'\0' * 20 => -1
            return -1
        if node in self._overrides:
            rev = self._overrides[node]
        elif self._config.verify:
            try:
                revorig = self._origindex[node]
            except error.RevlogError:
                revorig = None # convert "not found" to None
            rev = _logifraise(self._vfs,
                              lambda: self._rustnodemap[node],
                              lambda: {'nodemap.getitem': hex(node),
                                       b'revorig': revorig})
            if rev != revorig:
                _logandraise(self._vfs,
                             b'nodemap: inconsistent getitem(%s): %r vs %r'
                             % (hex(node), rev, revorig))
        else:
            rev = self._rustnodemap[node]

        if rev is None:
            raise error.RevlogError
        else:
            return rev

    def __setitem__(self, node, rev):
        self._overrides[node] = rev
        self._origindex[node] = rev

    def __delitem__(self, node):
        self._overrides[node] = None

    def __contains__(self, node):
        if not self._config.nodemap:
            return node in self._origindex

        if self._overrides.get(node) or node == nullid:
            return True

        if self._config.verify:
            resorig = node in self._origindex
            res = _logifraise(self._vfs,
                              lambda: node in self._rustnodemap,
                              lambda: {'nodemap.contains': hex(node),
                                       b'resorig': resorig})
            if res != resorig:
                _logandraise(self._vfs,
                             b'nodemap: inconsistent contains(%s): %r vs %r'
                             % (hex(node), res, resorig))
        else:
            res = node in self._rustnodemap
        return res

    def get(self, node, default=None):
        if self.__contains__(node):
            return self.__getitem__(node)
        else:
            return default

    def partialmatch(self, hexprefix):
        if not self._config.nodemap:
            return self._origindex.partialmatch(hexprefix)

        if self._config.verify:
            resorig = self._origindex.partialmatch(hexprefix)
            res = _logifraise(
                self._vfs,
                lambda: self._rustpartialmatch(hexprefix),
                lambda: {'partialmatch': hexprefix, b'resorig': resorig})
            if res != resorig:
                _logandraise(
                    self._vfs,
                    b'nodemap: inconsistent partialmatch(%s): %r vs %r'
                    % (hexprefix, res, resorig))
        else:
            res = self._rustpartialmatch(hexprefix)
        return res

    cdef _rustpartialmatch(self, hexprefix):
        candidates = set()
        # Special case: nullid
        if nullhex.startswith(hexprefix):
            candidates.add(nullid)
        try:
            node = self._rustnodemap.partialmatch(hexprefix)
            if node is not None:
                candidates.add(node)
        except (RuntimeError, RustError) as ex:
            # Convert b'ambiguous prefix' to RevlogError. This is because the
            # rust code cannot access RevlogError cleanly. So we do the
            # conversion here.
            if b'ambiguous prefix' in str(ex):
                raise error.RevlogError
            raise

        # Search nodes in overrides. This is needed because overrides could
        # live outside the changelog snapshot and are unknown to the rust
        # index.  Ideally we can keep changelog always up-to-date with the
        # index. But that requires more changes (ex. removing index.insert API
        # and index takes care of data writes).
        candidates.update(k for k in self._overrides.iterkeys()
                          if hex(k).startswith(hexprefix))
        if len(candidates) == 1:
            return list(candidates)[0]
        elif len(candidates) > 1:
            raise error.RevlogError
        else:
            return None

    @property
    def lag(self):
        if self._config.nodemap:
            return self._rustnodemap.lag()
        else:
            return 0

    def destroying(self):
        self._vfs.tryunlink(b'nodemap')
        self._config.nodemap = False

# These are unfortunate. But we need vfs access inside index.__init__. Doing
# that properly requires API changes in revlog.__init__ and
# revlogio.parseindex that might make things uglier, or break the (potential)
# intention of keeping revlog low-level, de-coupled from high-level objects
# including vfs and ui. So let's use a temporary global state to pass the
# vfs object and config options down to parseindex.
_cachevfs = None
_config = None

# Lightweight config state that is dedicated for this extensions and is
# decoupled from heavy-weight ui object.
cdef class localconfig:
    cdef public bint nodemap
    cdef public bint verify
    cdef public int lagthreshold

    def copy(self):
        rhs = localconfig()
        rhs.nodemap = self.nodemap
        rhs.verify = self.verify
        rhs.lagthreshold = self.lagthreshold
        return rhs

    @classmethod
    def fromui(cls, ui):
        self = cls()
        self.nodemap = ui.configbool(b'clindex', b'nodemap')
        self.verify = ui.configbool(b'clindex', b'verify')
        self.lagthreshold = ui.configint(b'clindex', b'lagthreshold')
        return self

def _parseindex(orig, self, data, inline):
    if inline:
        # clindex does not support inline. fallback to original index
        return orig(self, data, inline)
    index = clindex(data, inline, _cachevfs, _config)
    return index, index.nodemap, None

# Simple utilities to log debug messages
def _logandraise(vfs, message):
    _log(vfs, message)
    _recover(vfs)
    raise RuntimeError(message)

def _logifraise(vfs, func, infofunc):
    try:
        return func()
    except RuntimeError as ex:
        _log(vfs, b'exception: %r %r' % (ex, infofunc()))
        _recover(vfs)
        raise

def _recover(vfs):
    vfs.tryunlink(b'nodemap')
    vfs.tryunlink(b'childmap')

_logpath = None

def _log(vfs, message):
    try:
        if _logpath:
            f = open(_logpath, b'ab')
        else:
            f = vfs(b'clindex.log', b'ab')
        with f:
            timestamp = datetime.datetime.now().strftime(b'%Y-%m-%d %H:%M:%S.%f')
            pid = os.getpid()
            f.write(b'%s [%d] %s\n' % (timestamp, pid, message))
    except IOError:
        # The log is not important. IOError like "Permission denied" should not
        # be fatal.
        pass

def _wrapchangelog(orig, repo):
    # need to pass vfs to _parseindex so it can read the cache directory
    global _cachevfs
    _cachevfs = repo.cachevfs

    # pass a subset of config interesting to this extension
    global _config
    _config = localconfig.fromui(repo.ui)

    try:
        with extensions.wrappedfunction(revlog.revlogio,
                                        b'parseindex', _parseindex):
            return orig(repo)
    finally:
        # do not leak them outside parseindex
        _config = None
        _cachevfs = None

def reposetup(ui, repo):
    if not repo.local():
        return

    try:
        # Record nodemap lag.
        ui.log("nodemap_lag", nodemap_lag=repo.changelog.nodemap.lag)
    except AttributeError:
        pass

    unfilteredmethod = localrepo.unfilteredmethod

    class clindexrepo(repo.__class__):
        @unfilteredmethod
        def updatecaches(self, tr=None):
            try:
                self.changelog.index.updatecaches()
            except AttributeError as ex: # pure, or clindex is not used
                pass
            super(clindexrepo, self).updatecaches(tr)

        @unfilteredmethod
        def destroying(self):
            # Tell clindex to prepare for the strip. clindex will unlink
            # nodemap and other caches.
            try:
                self.changelog.index.destroying()
            except AttributeError as ex:
                pass
            super(clindexrepo, self).destroying()

        @unfilteredmethod
        def destroyed(self):
            # Force a reload of changelog. The current "self.changelog" object
            # has an outdated snapshot of changelog.i. We need to read the new
            # version before updatecaches().
            if b'changelog' in self.__dict__:
                del self.__dict__[b'changelog']
            if b'changelog' in self._filecache:
                del self._filecache[b'changelog']
            # This calls "updatecachess" and will pick up the new changelog.i.
            super(clindexrepo, self).destroyed()

    repo.__class__ = clindexrepo

def uisetup(ui):
    # global logpath config
    global _logpath
    _logpath = ui.config(b'clindex', b'logpath')

    # filecache method has to be wrapped using wrapfilecache
    extensions.wrapfilecache(localrepo.localrepository, b'changelog',
                             _wrapchangelog)
