import errno, os, struct
from mercurial import osutil, util

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

class basepackstore(object):
    def __init__(self, path):
        self.packs = []
        suffixlen = len(self.INDEXSUFFIX)

        files = []
        filenames = set()
        try:
            for filename, size, stat in osutil.listdir(path, stat=True):
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
                packpath = os.path.join(path, filename)
                self.packs.append(self.getpack(packpath[:-suffixlen]))

    def getpack(self, path):
        raise NotImplemented()

    def getmissing(self, keys):
        missing = keys
        for pack in self.packs:
            missing = pack.getmissing(missing)

        return missing

    def markledger(self, ledger):
        for pack in self.packs:
            pack.markledger(ledger)

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
        self.sha = util.sha1()
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
