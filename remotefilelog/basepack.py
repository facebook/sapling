import errno, hashlib, mmap, os, struct, time
from mercurial import osutil

import constants

# The pack version supported by this implementation. This will need to be
# rev'd whenever the byte format changes. Ex: changing the fanout prefix,
# changing any of the int sizes, changing the delta algorithm, etc.
VERSION = 0
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
SMALLFANOUTCUTOFF = 2**16 / 8

# The amount of time to wait between checking for new packs. This prevents an
# exception when data is moved to a new pack after the process has already
# loaded the pack list.
REFRESHRATE = 0.1

class basepackstore(object):
    def __init__(self, path):
        self.path = path
        self.packs = []
        # lastrefesh is 0 so we'll immediately check for new packs on the first
        # failure.
        self.lastrefresh = 0

        for filepath in self._getavailablepackfiles():
            self.packs.append(self.getpack(filepath))

    def _getavailablepackfiles(self):
        suffixlen = len(self.INDEXSUFFIX)

        files = []
        filenames = set()
        try:
            for filename, size, stat in osutil.listdir(self.path, stat=True):
                files.append((stat.st_mtime, filename))
                filenames.add(filename)
        except OSError as ex:
            if ex.errno != errno.ENOENT:
                raise

        # Put most recent pack files first since they contain the most recent
        # info.
        files = sorted(files, reverse=True)
        for mtime, filename in files:
            packfilename = '%s%s' % (filename[:-suffixlen], self.PACKSUFFIX)
            if (filename[-suffixlen:] == self.INDEXSUFFIX
                and packfilename in filenames):
                yield os.path.join(self.path, filename)[:-suffixlen]

    def getpack(self, path):
        raise NotImplemented()

    def getmissing(self, keys):
        missing = keys
        for pack in self.packs:
            missing = pack.getmissing(missing)

        if missing:
            for pack in self.refresh():
                missing = pack.getmissing(missing)

        return missing

    def markledger(self, ledger):
        for pack in self.packs:
            pack.markledger(ledger)

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
            new = set(self._getavailablepackfiles()) - previous

            for filepath in new:
                newpacks.append(self.getpack(filepath))
            self.packs.extend(newpacks)

        return newpacks

class basepack(object):
    # The maximum amount we should read via mmap before remmaping so the old
    # pages can be released (100MB)
    MAXPAGEDIN = 100 * 1024**2

    def __init__(self, path):
        self.path = path
        self.packpath = path + self.PACKSUFFIX
        self.indexpath = path + self.INDEXSUFFIX
        # TODO: use an opener/vfs to access these paths
        self.indexfp = open(self.indexpath, 'rb')
        self.datafp = open(self.packpath, 'rb')

        self.indexsize = os.stat(self.indexpath).st_size
        self.datasize = os.stat(self.packpath).st_size

        self._index = None
        self._data = None
        self.freememory() # initialize the mmap

        version = struct.unpack('!B', self._data[:PACKVERSIONSIZE])[0]
        if version != VERSION:
            raise RuntimeError("unsupported pack version '%s'" %
                               version)

        version, config = struct.unpack('!BB',
                                    self._index[:INDEXVERSIONSIZE])
        if version != VERSION:
            raise RuntimeError("unsupported pack index version '%s'" %
                               version)

        if 0b10000000 & config:
            self.params = indexparams(LARGEFANOUTPREFIX)
        else:
            self.params = indexparams(SMALLFANOUTPREFIX)

        params = self.params
        rawfanout = self._index[FANOUTSTART:FANOUTSTART + params.fanoutsize]
        self._fanouttable = []
        for i in xrange(0, params.fanoutcount):
            loc = i * 4
            fanoutentry = struct.unpack('!I', rawfanout[loc:loc + 4])[0]
            self._fanouttable.append(fanoutentry)

    def freememory(self):
        """Unmap and remap the memory to free it up after known expensive
        operations."""
        if self._index:
            if self._pagedin < self.MAXPAGEDIN:
                return

            self._index.close()
            self._data.close()

        # memory-map the file, size 0 means whole file
        self._index = mmap.mmap(self.indexfp.fileno(), 0,
                                access=mmap.ACCESS_READ)
        self._data = mmap.mmap(self.datafp.fileno(), 0,
                               access=mmap.ACCESS_READ)
        self._pagedin = 0

    def getmissing(self, keys):
        raise NotImplemented()

    def markledger(self, ledger):
        raise NotImplemented()

    def cleanup(self, ledger):
        raise NotImplemented()

    def __iter__(self):
        raise NotImplemented()

    def iterentries(self):
        raise NotImplemented()

class mutablebasepack(object):
    def __init__(self, opener):
        self.opener = opener
        self.entries = {}
        self.packfp, self.packpath = opener.mkstemp(
            suffix=self.PACKSUFFIX + '-tmp')
        self.idxfp, self.idxpath = opener.mkstemp(
            suffix=self.INDEXSUFFIX + '-tmp')
        self.packfp = os.fdopen(self.packfp, 'w+')
        self.idxfp = os.fdopen(self.idxfp, 'w+')
        self.sha = hashlib.sha1()
        self._closed = False

        # The opener provides no way of doing permission fixup on files created
        # via mkstemp, so we must fix it ourselves. We can probably fix this
        # upstream in vfs.mkstemp so we don't need to use the private method.
        opener._fixfilemode(opener.join(self.packpath))
        opener._fixfilemode(opener.join(self.idxpath))

        # Write header
        # TODO: make it extensible (ex: allow specifying compression algorithm,
        # a flexible key/value header, delta algorithm, fanout size, etc)
        version = struct.pack('!B', VERSION) # unsigned 1 byte int
        self.writeraw(version)

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        if exc_type is None:
            if not self._closed:
                self.close()
        else:
            # Unclean exit
            try:
                self.opener.unlink(self.packpath)
                self.opener.unlink(self.idxpath)
            except Exception:
                pass

    def writeraw(self, data):
        self.packfp.write(data)
        self.sha.update(data)

    def close(self, ledger=None):
        sha = self.sha.hexdigest()
        self.packfp.close()
        self.writeindex()

        if len(self.entries) == 0:
            # Empty pack
            self.opener.unlink(self.packpath)
            self.opener.unlink(self.idxpath)
            self._closed = True
            return None

        self.opener.rename(self.packpath, sha + self.PACKSUFFIX)
        self.opener.rename(self.idxpath, sha + self.INDEXSUFFIX)

        self._closed = True
        result = self.opener.join(sha)
        if ledger:
            ledger.addcreated(result)
        return result

    def writeindex(self):
        rawindex = ''

        largefanout = len(self.entries) > SMALLFANOUTCUTOFF
        if largefanout:
            params = indexparams(LARGEFANOUTPREFIX)
        else:
            params = indexparams(SMALLFANOUTPREFIX)

        fanouttable = [EMPTYFANOUT] * params.fanoutcount

        # Precompute the location of each entry
        locations = {}
        count = 0
        for node in sorted(self.entries.iterkeys()):
            location = count * self.INDEXENTRYLENGTH
            locations[node] = location
            count += 1

            # Must use [0] on the unpack result since it's always a tuple.
            fanoutkey = struct.unpack(params.fanoutstruct,
                                      node[:params.fanoutprefix])[0]
            if fanouttable[fanoutkey] == EMPTYFANOUT:
                fanouttable[fanoutkey] = location

        rawfanouttable = ''
        last = 0
        for offset in fanouttable:
            offset = offset if offset != EMPTYFANOUT else last
            last = offset
            rawfanouttable += struct.pack('!I', offset)

        rawindex = self.createindex(locations)

        self._writeheader(params)
        self.idxfp.write(rawfanouttable)
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
        self.idxfp.write(struct.pack('!BB', VERSION, config))

class indexparams(object):
    __slots__ = ('fanoutprefix', 'fanoutstruct', 'fanoutcount', 'fanoutsize',
                 'indexstart')

    def __init__(self, prefixsize):
        self.fanoutprefix = prefixsize

        # The struct pack format for fanout table location (i.e. the format that
        # converts the node prefix into an integer location in the fanout
        # table).
        if prefixsize == SMALLFANOUTPREFIX:
            self.fanoutstruct = '!B'
        elif prefixsize == LARGEFANOUTPREFIX:
            self.fanoutstruct = '!H'
        else:
            raise ValueError("invalid fanout prefix size: %s" % prefixsize)

        # The number of fanout table entries
        self.fanoutcount = 2**(prefixsize * 8)

        # The total bytes used by the fanout table
        self.fanoutsize = self.fanoutcount * 4

        self.indexstart = FANOUTSTART + self.fanoutsize
