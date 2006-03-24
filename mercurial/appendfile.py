# appendfile.py - special classes to make repo updates atomic
#
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from demandload import *
demandload(globals(), "cStringIO changelog manifest os tempfile")

# writes to metadata files are ordered.  reads: changelog, manifest,
# normal files.  writes: normal files, manifest, changelog.

# manifest contains pointers to offsets in normal files.  changelog
# contains pointers to offsets in manifest.  if reader reads old
# changelog while manifest or normal files are written, it has no
# pointers into new parts of those files that are maybe not consistent
# yet, so will not read them.

# localrepo.addchangegroup thinks it writes changelog first, then
# manifest, then normal files (this is order they are available, and
# needed for computing linkrev fields), but uses appendfile to hide
# updates from readers.  data not written to manifest or changelog
# until all normal files updated.  write manifest first, then
# changelog.

# with this write ordering, readers cannot see inconsistent view of
# repo during update.

class appendfile(object):
    '''implement enough of file protocol to append to revlog file.
    appended data is written to temp file.  reads and seeks span real
    file and temp file.  readers cannot see appended data until
    writedata called.'''

    def __init__(self, fp):
        fd, self.tmpname = tempfile.mkstemp()
        self.tmpfp = os.fdopen(fd, 'ab+')
        self.realfp = fp
        self.offset = 0
        # real file is not written by anyone else. cache its size so
        # seek and read can be fast.
        self.fpsize = os.fstat(fp.fileno()).st_size

    def seek(self, offset):
        '''virtual file offset spans real file and temp file.'''
        self.offset = offset
        if self.offset < self.fpsize:
            self.realfp.seek(self.offset)
        else:
            self.tmpfp.seek(self.offset - self.fpsize)

    def read(self, count=-1):
        '''only trick here is reads that span real file and temp file.'''
        fp = cStringIO.StringIO()
        old_offset = self.offset
        if self.offset < self.fpsize:
            s = self.realfp.read(count)
            fp.write(s)
            self.offset += len(s)
            if count > 0:
                count -= len(s)
        if count != 0:
            if old_offset != self.offset:
                self.tmpfp.seek(self.offset - self.fpsize)
            s = self.tmpfp.read(count)
            fp.write(s)
            self.offset += len(s)
        return fp.getvalue()

    def write(self, s):
        '''append to temp file.'''
        self.tmpfp.write(s)
        # all writes are appends, so offset must go to end of file.
        self.offset = self.fpsize + self.tmpfp.tell()

    def writedata(self):
        '''copy data from temp file to real file.'''
        self.tmpfp.seek(0)
        s = self.tmpfp.read()
        self.tmpfp.close()
        self.realfp.seek(0, 2)
        # small race here.  we write all new data in one call, but
        # reader can see partial update due to python or os. file
        # locking no help: slow, not portable, not reliable over nfs.
        # only safe thing is write to temp file every time and rename,
        # but performance bad when manifest or changelog gets big.
        self.realfp.write(s)
        self.realfp.close()

    def __del__(self):
        '''delete temp file even if exception raised.'''
        try: os.unlink(self.tmpname)
        except: pass

class sharedfile(object):
    '''let file objects share a single appendfile safely.  each
    sharedfile has own offset, syncs up with appendfile offset before
    read and after read and write.'''

    def __init__(self, fp):
        self.fp = fp
        self.offset = 0

    def seek(self, offset):
        self.offset = offset

    def read(self, count=-1):
        try:
            if self.offset != self.fp.offset:
                self.fp.seek(self.offset)
            return self.fp.read(count)
        finally:
            self.offset = self.fp.offset

    def write(self, s):
        try:
            return self.fp.write(s)
        finally:
            self.offset = self.fp.offset

    def close(self):
        # revlog wants this.
        pass

    def flush(self):
        # revlog wants this.
        pass

    def writedata(self):
        self.fp.writedata()

class appendopener(object):
    '''special opener for files that only read or append.'''

    def __init__(self, opener):
        self.realopener = opener
        # key: file name, value: appendfile object
        self.fps = {}

    def __call__(self, name, mode='r'):
        '''open file.  return same cached appendfile object for every
        later call.'''

        assert mode in 'ra'
        fp = self.fps.get(name)
        if fp is None:
            fp = appendfile(self.realopener(name, 'a+'))
            self.fps[name] = fp
        return sharedfile(fp)

    def writedata(self):
        '''copy data from temp files to real files.'''
        # write .d file before .i file.
        fps = self.fps.items()
        fps.sort()
        for name, fp in fps:
            fp.writedata()

# files for changelog and manifest are in different appendopeners, so
# not mixed up together.

class appendchangelog(changelog.changelog, appendopener):
    def __init__(self, opener):
        appendopener.__init__(self, opener)
        changelog.changelog.__init__(self, self)

class appendmanifest(manifest.manifest, appendopener):
    def __init__(self, opener):
        appendopener.__init__(self, opener)
        manifest.manifest.__init__(self, self)
