from __future__ import absolute_import

import collections
import errno
import hashlib
import os
import struct
import time

from mercurial.i18n import _
from mercurial import error, policy, pycompat, util, vfs as vfsmod

from ..extlib import litemmap
from . import constants, shallowutil

osutil = policy.importmod(r"osutil")

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

        cachedpacks = set(pack for pack in self._lrucache)
        # Yield for paths not in the cache.
        for pack in self._packs - cachedpacks:
            self._lastpack = pack
            yield pack

        # Data not found in any pack.
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

        packs = []
        for filepath, __, __ in self._getavailablepackfilessorted():
            try:
                pack = self.getpack(filepath)
            except Exception as ex:
                # An exception may be thrown if the pack file is corrupted
                # somehow.  Log a warning but keep going in this case, just
                # skipping this pack file.
                #
                # If this is an ENOENT error then don't even bother logging.
                # Someone could have removed the file since we retrieved the
                # list of paths.
                if getattr(ex, "errno", None) != errno.ENOENT:
                    ui.warn(_("unable to load pack %s: %s\n") % (filepath, ex))
                continue
            packs.append(pack)

        self.packs = _cachebackedpacks(packs, self.DEFAULTCACHESIZE)

    def _getavailablepackfiles(self):
        """For each pack file (a index/data file combo), yields:
          (full path without extension, mtime, size)

        mtime will be the mtime of the index/data file (whichever is newer)
        size is the combined size of index/data file
        """
        indexsuffixlen = len(self.INDEXSUFFIX)
        packsuffixlen = len(self.PACKSUFFIX)

        ids = set()
        sizes = collections.defaultdict(lambda: 0)
        mtimes = collections.defaultdict(lambda: [])
        try:
            for filename, type, stat in osutil.listdir(self.path, stat=True):
                id = None
                if filename[-indexsuffixlen:] == self.INDEXSUFFIX:
                    id = filename[:-indexsuffixlen]
                elif filename[-packsuffixlen:] == self.PACKSUFFIX:
                    id = filename[:-packsuffixlen]

                # Since we expect to have two files corresponding to each ID
                # (the index file and the pack file), we can yield once we see
                # it twice.
                if id:
                    sizes[id] += stat.st_size  # Sum both files' sizes together
                    mtimes[id].append(stat.st_mtime)
                    if id in ids:
                        yield (os.path.join(self.path, id), max(mtimes[id]), sizes[id])
                    else:
                        ids.add(id)
        except OSError as ex:
            if ex.errno != errno.ENOENT:
                raise

    def _getavailablepackfilessorted(self):
        """Like `_getavailablepackfiles`, but also sorts the files by mtime,
        yielding newest files first.

        This is desirable, since it is more likely newer packfiles have more
        desirable data.
        """
        files = []
        for path, mtime, size in self._getavailablepackfiles():
            files.append((mtime, size, path))
        files = sorted(files, reverse=True)
        for mtime, size, path in files:
            yield path, mtime, size

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

        for pack in self.packs:
            pack.markledger(ledger, options)

    def markforrefresh(self):
        """Tells the store that there may be new pack files, so the next time it
        has a lookup miss it should check for new files."""
        self.lastrefresh = 0

    def refresh(self):
        """Checks for any new packs on disk, adds them to the main pack list,
        and returns a list of just the new packs."""
        now = time.time()

        # If we experience a lot of misses (like in the case of getmissing() on
        # new objects), let's only actually check disk for new stuff every once
        # in a while. Generally this code path should only ever matter when a
        # repack is going on in the background, and that should be pretty rare
        # to have that happen twice in quick succession.
        newpacks = []
        if now > self.lastrefresh + REFRESHRATE:
            self.lastrefresh = now
            previous = set(p.path for p in self.packs)
            for filepath, __, __ in self._getavailablepackfilessorted():
                if filepath not in previous:
                    newpack = self.getpack(filepath)
                    newpacks.append(newpack)
                    self.packs.add(newpack)

        return newpacks

    def runonpacks(self, func):
        badpacks = []
        for pack in self.packs:
            try:
                yield func(pack)
            except KeyError:
                pass
            except Exception:
                # Other exceptions indicate an issue with the pack file, so
                # remove it.
                badpacks.append(pack)

        for pack in self.refresh():
            try:
                yield func(pack)
            except KeyError:
                pass
            except Exception:
                # Other exceptions indicate an issue with the pack file, so
                # remove it.
                badpacks.append(pack)

        if badpacks:
            if self.deletecorruptpacks:
                for pack in badpacks:
                    self.ui.warn(_("deleting corrupt pack '%s'\n") % pack.path)
                    self.packs.remove(pack)
                    util.tryunlink(pack.packpath)
                    util.tryunlink(pack.indexpath)
            else:
                for pack in badpacks:
                    self.ui.warn(
                        _("detected corrupt pack '%s' - ignoring it\n") % pack.path
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
        self.path = path
        self.packpath = path + self.PACKSUFFIX
        self.indexpath = path + self.INDEXSUFFIX

        self.indexsize = os.stat(self.indexpath).st_size
        self.datasize = os.stat(self.packpath).st_size

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
        with util.posixfile(self.indexpath, PACKOPENMODE) as indexfp:
            # memory-map the file, size 0 means whole file
            self._index = litemmap.mmap(
                indexfp.fileno(), 0, access=litemmap.pymmap.ACCESS_READ
            )
        with util.posixfile(self.packpath, PACKOPENMODE) as datafp:
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


class mutablebasepack(versionmixin):
    def __init__(self, ui, packdir, version=0):
        self._checkversion(version)

        opener = vfsmod.vfs(packdir)
        opener.createmode = 0o444
        self.opener = opener

        self.entries = {}

        shallowutil.mkstickygroupdir(ui, packdir)
        self.packfp, self.packpath = opener.mkstemp(suffix=self.PACKSUFFIX + "-tmp")
        self.idxfp, self.idxpath = opener.mkstemp(suffix=self.INDEXSUFFIX + "-tmp")
        self.packfp = os.fdopen(self.packfp, "w+")
        self.idxfp = os.fdopen(self.idxfp, "w+")
        self.sha = hashlib.sha1()
        self._closed = False

        # Where the pack was written to, if closed successfully.
        self._destpath = None

        # The opener provides no way of doing permission fixup on files created
        # via mkstemp, so we must fix it ourselves. We can probably fix this
        # upstream in vfs.mkstemp so we don't need to use the private method.
        opener._fixfilemode(opener.join(self.packpath))
        opener._fixfilemode(opener.join(self.idxpath))

        # Write header
        # TODO: make it extensible (ex: allow specifying compression algorithm,
        # a flexible key/value header, delta algorithm, fanout size, etc)
        versionbuf = struct.pack("!B", self.VERSION)  # unsigned 1 byte int
        self.writeraw(versionbuf)

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        if exc_type is None:
            self._destpath = self.close()
        else:
            self.abort()

    @property
    def destpath(self):
        if self._destpath is None:
            raise error.ProgrammingError(
                _("attempted to read mutabledatapack path before it was closed")
            )
        return self._destpath

    def abort(self):
        # Unclean exit
        self._cleantemppacks()

    def writeraw(self, data):
        self.packfp.write(data)
        self.sha.update(data)

    def close(self, ledger=None):
        if self._closed:
            return

        try:
            sha = self.sha.hexdigest()
            self.packfp.close()
            self.writeindex()

            if len(self.entries) == 0:
                # Empty pack
                self._cleantemppacks()
                self._closed = True
                return None

            self.opener.rename(self.packpath, sha + self.PACKSUFFIX)
            try:
                self.opener.rename(self.idxpath, sha + self.INDEXSUFFIX)
            except Exception as ex:
                try:
                    self.opener.unlink(sha + self.PACKSUFFIX)
                except Exception:
                    pass
                # Throw exception 'ex' explicitly since a normal 'raise' would
                # potentially throw an exception from the unlink cleanup.
                raise ex
        except Exception:
            # Clean up temp packs in all exception cases
            self._cleantemppacks()
            raise

        self._closed = True
        result = self.opener.join(sha)
        if ledger:
            ledger.addcreated(result)
        return result

    def _cleantemppacks(self):
        try:
            self.opener.unlink(self.packpath)
        except Exception:
            pass
        try:
            self.opener.unlink(self.idxpath)
        except Exception:
            pass

    def writeindex(self):
        rawindex = ""

        largefanout = len(self.entries) > SMALLFANOUTCUTOFF
        if largefanout:
            params = indexparams(LARGEFANOUTPREFIX, self.VERSION)
        else:
            params = indexparams(SMALLFANOUTPREFIX, self.VERSION)

        fanouttable = [EMPTYFANOUT] * params.fanoutcount

        # Precompute the location of each entry
        locations = {}
        count = 0
        for node in sorted(self.entries.iterkeys()):
            location = count * self.INDEXENTRYLENGTH
            locations[node] = location
            count += 1

            # Must use [0] on the unpack result since it's always a tuple.
            fanoutkey = struct.unpack(params.fanoutstruct, node[: params.fanoutprefix])[
                0
            ]
            if fanouttable[fanoutkey] == EMPTYFANOUT:
                fanouttable[fanoutkey] = location

        rawfanouttable = ""
        last = 0
        for offset in fanouttable:
            offset = offset if offset != EMPTYFANOUT else last
            last = offset
            rawfanouttable += struct.pack("!I", offset)

        rawentrieslength = struct.pack("!Q", len(self.entries))

        # The index offset is the it's location in the file. So after the 2 byte
        # header and the fanouttable.
        rawindex = self.createindex(locations, 2 + len(rawfanouttable))

        self._writeheader(params)
        self.idxfp.write(rawfanouttable)
        if self.VERSION == 1:
            self.idxfp.write(rawentrieslength)
        self.idxfp.write(rawindex)
        self.idxfp.close()

    def createindex(self, nodelocations):
        raise NotImplemented()

    def _writeheader(self, indexparams):
        # Index header
        #    <version: 1 byte>
        #    <large fanout: 1 bit> # 1 means 2^16, 0 means 2^8
        #    <unused: 7 bit> # future use (compression, delta format, etc)
        config = 0
        if indexparams.fanoutprefix == LARGEFANOUTPREFIX:
            config = 0b10000000
        self.idxfp.write(struct.pack("!BB", self.VERSION, config))


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
