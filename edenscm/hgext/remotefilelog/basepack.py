# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import collections
import errno
import hashlib
import os
import stat as statmod
import struct
import time

from edenscm.mercurial import error, policy, pycompat, util, vfs as vfsmod
from edenscm.mercurial.i18n import _
from edenscmnative import litemmap

from . import constants, shallowutil


# The pack version supported by this implementation. This will need to be
# rev'd whenever the byte format changes. Ex: changing the fanout prefix,
# changing any of the int sizes, changing the delta algorithm, etc.
PACKVERSIONSIZE = 1
INDEXVERSIONSIZE = 2

FANOUTSTART = INDEXVERSIONSIZE

# Constant that indicates a fanout table entry hasn't been filled in. (This does
# not get serialized)
EMPTYFANOUT = -1

# The fanout prefix is the number of bytes that can be addressed by the fanout
# table. Example: a fanout prefix of 1 means we use the first byte of a hash to
# look in the fanout table (which will be 2^8 entries long).
SMALLFANOUTPREFIX = 1
LARGEFANOUTPREFIX = 2

# The number of entries in the index at which point we switch to a large fanout.
# It is chosen to balance the linear scan through a sparse fanout, with the
# size of the bisect in actual index.
# 2^16 / 8 was chosen because it trades off (1 step fanout scan + 5 step
# bisect) with (8 step fanout scan + 1 step bisect)
# 5 step bisect = log(2^16 / 8 / 255)  # fanout
# 10 step fanout scan = 2^16 / (2^16 / 8)  # fanout space divided by entries
SMALLFANOUTCUTOFF = 2 ** 16 / 8

# The amount of time to wait between checking for new packs. This prevents an
# exception when data is moved to a new pack after the process has already
# loaded the pack list.
REFRESHRATE = 0.1

try:
    xrange(0)
except NameError:
    xrange = range

if pycompat.isposix:
    # With glibc 2.7+ the 'e' flag uses O_CLOEXEC when opening.
    # The 'e' flag will be ignored on older versions of glibc.
    PACKOPENMODE = "rbe"
else:
    PACKOPENMODE = "rb"


class _cachebackedpacks(object):
    def __init__(self, packs, cachesize):
        self._packs = set(packs)
        self._lrucache = util.lrucachedict(cachesize)
        self._lastpack = None

        # Avoid cold start of the cache by populating the most recent packs
        # in the cache.
        for i in reversed(range(min(cachesize, len(packs)))):
            self._movetofront(packs[i])

    def __len__(self):
        return len(self._lrucache)

    def _movetofront(self, pack):
        # This effectively makes pack the first entry in the cache.
        self._lrucache[pack] = True

    def _registerlastpackusage(self):
        if self._lastpack is not None:
            self._movetofront(self._lastpack)
            self._lastpack = None

    def add(self, pack):
        self._registerlastpackusage()

        # This method will mostly be called when packs are not in cache.
        # Therefore, adding pack to the cache.
        self._movetofront(pack)
        self._packs.add(pack)

    def remove(self, pack):
        self._packs.remove(pack)
        del self._lrucache[pack]

    def __iter__(self):
        self._registerlastpackusage()

        # Cache iteration is based on LRU.
        for pack in self._lrucache:
            self._lastpack = pack
            yield pack

        if len(self._packs) != len(self._lrucache):
            cachedpacks = set(pack for pack in self._lrucache)
            # Yield for paths not in the cache.
            for pack in self._packs - cachedpacks:
                self._lastpack = pack
                yield pack

        # Data not found in any pack.
        self._lastpack = None

    def clear(self):
        self._packs.clear()
        self._lrucache.clear()
        self._lastpack = None


class basepackstore(object):
    # Default cache size limit for the pack files.
    DEFAULTCACHESIZE = 100

    def __init__(self, ui, path, deletecorruptpacks=False):
        self.ui = ui
        self.path = path
        self.deletecorruptpacks = deletecorruptpacks

        # lastrefesh is 0 so we'll immediately check for new packs on the first
        # failure.
        self.lastrefresh = 0

        self.packs = _cachebackedpacks([], self.DEFAULTCACHESIZE)
        self.packspath = set()

        self.fetchpacksenabled = self.ui.configbool("remotefilelog", "fetchpacks")

    def _getavailablepackfiles(self, currentpacks=None):
        """For each pack file (a index/data file combo), yields:
          (full path without extension, mtime, size)

        mtime will be the mtime of the index/data file (whichever is newer)
        size is the combined size of index/data file
        """
        if currentpacks is None:
            currentpacks = set()

        ids = set()
        sizes = collections.defaultdict(lambda: 0)
        mtimes = collections.defaultdict(lambda: [])
        try:
            for filename in os.listdir(self.path):
                filename = os.path.join(self.path, filename)
                id, ext = os.path.splitext(filename)

                if id not in currentpacks:
                    # Since we expect to have two files corresponding to each ID
                    # (the index file and the pack file), we can yield once we see
                    # it twice.
                    if ext == self.INDEXSUFFIX or ext == self.PACKSUFFIX:
                        st = os.lstat(filename)
                        if statmod.S_ISDIR(st.st_mode):
                            continue

                        sizes[id] += st.st_size  # Sum both files' sizes together
                        mtimes[id].append(st.st_mtime)
                        if id in ids:
                            yield (
                                os.path.join(self.path, id),
                                max(mtimes[id]),
                                sizes[id],
                            )
                        else:
                            ids.add(id)
        except OSError as ex:
            if ex.errno != errno.ENOENT:
                raise

    def _getavailablepackfilessorted(self, currentpacks):
        """Like `_getavailablepackfiles`, but also sorts the files by mtime,
        yielding newest files first.

        This is desirable, since it is more likely newer packfiles have more
        desirable data.
        """
        files = []
        for path, mtime, size in self._getavailablepackfiles(currentpacks):
            files.append((mtime, size, path))
        files = sorted(files, reverse=True)
        for __, __, path in files:
            yield path

    def gettotalsizeandcount(self):
        """Returns the total disk size (in bytes) of all the pack files in
        this store, and the count of pack files.

        (This might be smaller than the total size of the ``self.path``
        directory, since this only considers fuly-writen pack files, and not
        temporary files or other detritus on the directory.)
        """
        totalsize = 0
        count = 0
        for __, __, size in self._getavailablepackfiles():
            totalsize += size
            count += 1
        return totalsize, count

    def getmetrics(self):
        """Returns metrics on the state of this store."""
        size, count = self.gettotalsizeandcount()
        return {"numpacks": count, "totalpacksize": size}

    def getpack(self, path):
        raise NotImplemented()

    def getmissing(self, keys):
        missing = keys

        def func(pack):
            return pack.getmissing(missing)

        for newmissing in self.runonpacks(func):
            missing = newmissing
            if not missing:
                break

        return missing

    def markledger(self, ledger, options=None):
        if options and options.get(constants.OPTION_LOOSEONLY):
            return

        # Since the packs are initialized lazily, self.packs might be empty. To
        # be sure, let's manually refresh.
        self.refresh()

        # Stop deleting packs if they are corrupt.  If we discover corruption
        # then we'll handle it via the ledger.
        deletecorruptpacks = self.deletecorruptpacks
        if deletecorruptpacks:
            self.deletecorruptpacks = False

            def cleanup(ui):
                self.deletecorruptpacks = True

            ledger.addcleanup(cleanup)

        def makecleanupcorruption(path, packpath, indexpath):
            def cleanup(ui):
                ui.warn(_("cleaning up corrupt pack '%s'\n") % path)
                if deletecorruptpacks:
                    util.tryunlink(packpath)
                    util.tryunlink(indexpath)
                else:
                    util.rename(packpath, packpath + ".corrupt")
                    util.rename(indexpath, indexpath + ".corrupt")

            return cleanup

        with ledger.location(self.path):
            for pack in self.packs:
                try:
                    pack.markledger(ledger, options)
                except Exception:
                    self.ui.warn(_("detected corrupt pack '%s'\n") % pack.path())
                    # Mark this pack as corrupt, which prevents cleanup being
                    # called.  Add a separate cleanup function to handle the
                    # corruption at the end of repack by either deleting or
                    # renaming the file.
                    ledger.markcorruptsource(pack)
                    ledger.addcleanup(
                        makecleanupcorruption(
                            pack.path(), pack.packpath(), pack.indexpath()
                        )
                    )

    def markforrefresh(self):
        """Tells the store that there may be new pack files, so the next time it
        has a lookup miss it should check for new files."""
        self.lastrefresh = 0

    def refresh(self):
        """Checks for any new packs on disk, adds them to the main pack list,
        and returns a list of just the new packs."""
        now = time.time()

        # When remotefilelog.fetchpacks is enabled, some commands will trigger
        # many packfiles to be written to disk. This has the negative effect to
        # really slow down the refresh function, to the point where 90+% of the
        # time would be spent in it. A simple (but effective) solution is to
        # run repack when we detect that the number of packfiles is too big. A
        # better solution is to use a file format that isn't immutable, like
        # IndexedLog. Running repack is the short-time solution until
        # IndexedLog is more widely deployed.
        if self.fetchpacksenabled and len(self.packs) == self.DEFAULTCACHESIZE:
            self.packs.clear()
            self.packspath.clear()
            try:
                self.repackstore()
            except Exception:
                # Failures can happen due to concurrent repacks, which should
                # be rare. Let's just ignore these, the next refresh will
                # re-issue the repack and succeed.
                pass

        # If we experience a lot of misses (like in the case of getmissing() on
        # new objects), let's only actually check disk for new stuff every once
        # in a while. Generally this code path should only ever matter when a
        # repack is going on in the background, and that should be pretty rare
        # to have that happen twice in quick succession.
        newpacks = []
        if now > self.lastrefresh + REFRESHRATE:
            previous = self.packspath
            for filepath in self._getavailablepackfilessorted(previous):
                try:
                    newpack = self.getpack(filepath)
                    newpacks.append(newpack)
                except Exception as ex:
                    # An exception may be thrown if the pack file is corrupted
                    # somehow.  Log a warning but keep going in this case, just
                    # skipping this pack file.
                    #
                    # If this is an ENOENT error then don't even bother logging.
                    # Someone could have removed the file since we retrieved the
                    # list of paths.
                    if getattr(ex, "errno", None) != errno.ENOENT:
                        if self.deletecorruptpacks:
                            self.ui.warn(_("deleting corrupt pack '%s'\n") % filepath)
                            util.tryunlink(filepath + self.PACKSUFFIX)
                            util.tryunlink(filepath + self.INDEXSUFFIX)
                        else:
                            self.ui.warn(
                                _("detected corrupt pack '%s' - ignoring it\n")
                                % filepath
                            )

            self.lastrefresh = time.time()

        for pack in reversed(newpacks):
            self.packs.add(pack)
            self.packspath.add(pack.path())

        return newpacks

    def runonpacks(self, func):
        badpacks = []

        for pack in self.packs:
            try:
                yield func(pack)
            except KeyError:
                pass
            except Exception as ex:
                # Other exceptions indicate an issue with the pack file, so
                # remove it.
                badpacks.append((pack, getattr(ex, "errno", None)))

        newpacks = self.refresh()
        if newpacks != []:
            newpacks = set(newpacks)
            for pack in self.packs:
                if pack in newpacks:
                    try:
                        yield func(pack)
                    except KeyError:
                        pass
                    except Exception as ex:
                        # Other exceptions indicate an issue with the pack file, so
                        # remove it.
                        badpacks.append((pack, getattr(ex, "errno", None)))

        if badpacks:
            if self.deletecorruptpacks:
                for pack, err in badpacks:
                    self.packs.remove(pack)
                    self.packspath.remove(pack.path())

                    if err != errno.ENOENT:
                        self.ui.warn(_("deleting corrupt pack '%s'\n") % pack.path())
                        util.tryunlink(pack.packpath())
                        util.tryunlink(pack.indexpath())
            else:
                for pack, err in badpacks:
                    if err != errno.ENOENT:
                        self.ui.warn(
                            _("detected corrupt pack '%s' - ignoring it\n")
                            % pack.path()
                        )


class versionmixin(object):
    # Mix-in for classes with multiple supported versions
    VERSION = None
    SUPPORTED_VERSIONS = [0]

    def _checkversion(self, version):
        if version in self.SUPPORTED_VERSIONS:
            if self.VERSION is None:
                # only affect this instance
                self.VERSION = version
            elif self.VERSION != version:
                raise RuntimeError("inconsistent version: %s" % version)
        else:
            raise RuntimeError("unsupported version: %s" % version)


class basepack(versionmixin):
    # The maximum amount we should read via mmap before remmaping so the old
    # pages can be released (100MB)
    MAXPAGEDIN = 100 * 1024 ** 2

    SUPPORTED_VERSIONS = [0]

    def __init__(self, path):
        self._path = path
        self._packpath = path + self.PACKSUFFIX
        self._indexpath = path + self.INDEXSUFFIX

        self.indexsize = os.stat(self._indexpath).st_size
        self.datasize = os.stat(self._packpath).st_size

        self._index = None
        self._data = None
        self.freememory()  # initialize the mmap

        version = struct.unpack("!B", self._data[:PACKVERSIONSIZE])[0]
        self._checkversion(version)

        version, config = struct.unpack("!BB", self._index[:INDEXVERSIONSIZE])
        self._checkversion(version)

        if 0b10000000 & config:
            self.params = indexparams(LARGEFANOUTPREFIX, version)
        else:
            self.params = indexparams(SMALLFANOUTPREFIX, version)

    def path(self):
        return self._path

    def packpath(self):
        return self._packpath

    def indexpath(self):
        return self._indexpath

    @util.propertycache
    def _fanouttable(self):
        params = self.params
        rawfanout = self._index[FANOUTSTART : FANOUTSTART + params.fanoutsize]
        fanouttable = []
        for i in xrange(0, params.fanoutcount):
            loc = i * 4
            fanoutentry = struct.unpack("!I", rawfanout[loc : loc + 4])[0]
            fanouttable.append(fanoutentry)
        return fanouttable

    @util.propertycache
    def _indexend(self):
        if self.VERSION == 0:
            return self.indexsize
        else:
            offset = self.params.indexstart - 8
            nodecount = struct.unpack_from("!Q", self._index[offset : offset + 8])[0]
            return self.params.indexstart + nodecount * self.INDEXENTRYLENGTH

    def freememory(self):
        """Unmap and remap the memory to free it up after known expensive
        operations. Return True if self._data and self._index were reloaded.
        """
        if self._index:
            if self._pagedin < self.MAXPAGEDIN:
                return False

            self._index.close()
            self._data.close()

        # TODO: use an opener/vfs to access these paths
        with util.posixfile(self.indexpath(), PACKOPENMODE) as indexfp:
            # memory-map the file, size 0 means whole file
            self._index = litemmap.mmap(
                indexfp.fileno(), 0, access=litemmap.pymmap.ACCESS_READ
            )
        with util.posixfile(self.packpath(), PACKOPENMODE) as datafp:
            self._data = litemmap.mmap(
                datafp.fileno(), 0, access=litemmap.pymmap.ACCESS_READ
            )

        self._pagedin = 0
        return True

    def getmissing(self, keys):
        raise NotImplemented()

    def markledger(self, ledger, options=None):
        raise NotImplemented()

    def cleanup(self, ledger):
        raise NotImplemented()

    def __iter__(self):
        raise NotImplemented()

    def iterentries(self):
        raise NotImplemented()


class indexparams(object):
    __slots__ = (
        "fanoutprefix",
        "fanoutstruct",
        "fanoutcount",
        "fanoutsize",
        "indexstart",
    )

    def __init__(self, prefixsize, version):
        self.fanoutprefix = prefixsize

        # The struct pack format for fanout table location (i.e. the format that
        # converts the node prefix into an integer location in the fanout
        # table).
        if prefixsize == SMALLFANOUTPREFIX:
            self.fanoutstruct = "!B"
        elif prefixsize == LARGEFANOUTPREFIX:
            self.fanoutstruct = "!H"
        else:
            raise ValueError("invalid fanout prefix size: %s" % prefixsize)

        # The number of fanout table entries
        self.fanoutcount = 2 ** (prefixsize * 8)

        # The total bytes used by the fanout table
        self.fanoutsize = self.fanoutcount * 4

        self.indexstart = FANOUTSTART + self.fanoutsize
        if version == 1:
            # Skip the index length
            self.indexstart += 8
