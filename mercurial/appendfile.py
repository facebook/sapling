# appendfile.py - special classes to make repo updates atomic
#
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from demandload import *
demandload(globals(), "cStringIO changelog errno manifest os tempfile util")

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

    def __init__(self, fp, tmpname):
        if tmpname:
            self.tmpname = tmpname
            self.tmpfp = util.posixfile(self.tmpname, 'ab+')
        else:
            fd, self.tmpname = tempfile.mkstemp(prefix="hg-appendfile-")
            os.close(fd)
            self.tmpfp = util.posixfile(self.tmpname, 'ab+')
        self.realfp = fp
        self.offset = fp.tell()
        # real file is not written by anyone else. cache its size so
        # seek and read can be fast.
        self.realsize = util.fstat(fp).st_size
        self.name = fp.name

    def end(self):
        self.tmpfp.flush() # make sure the stat is correct
        return self.realsize + util.fstat(self.tmpfp).st_size

    def tell(self):
        return self.offset

    def flush(self):
        self.tmpfp.flush()

    def close(self):
        self.realfp.close()
        self.tmpfp.close()

    def seek(self, offset, whence=0):
        '''virtual file offset spans real file and temp file.'''
        if whence == 0:
            self.offset = offset
        elif whence == 1:
            self.offset += offset
        elif whence == 2:
            self.offset = self.end() + offset

        if self.offset < self.realsize:
            self.realfp.seek(self.offset)
        else:
            self.tmpfp.seek(self.offset - self.realsize)

    def read(self, count=-1):
        '''only trick here is reads that span real file and temp file.'''
        fp = cStringIO.StringIO()
        old_offset = self.offset
        if self.offset < self.realsize:
            s = self.realfp.read(count)
            fp.write(s)
            self.offset += len(s)
            if count > 0:
                count -= len(s)
        if count != 0:
            if old_offset != self.offset:
                self.tmpfp.seek(self.offset - self.realsize)
            s = self.tmpfp.read(count)
            fp.write(s)
            self.offset += len(s)
        return fp.getvalue()

    def write(self, s):
        '''append to temp file.'''
        self.tmpfp.seek(0, 2)
        self.tmpfp.write(s)
        # all writes are appends, so offset must go to end of file.
        self.offset = self.realsize + self.tmpfp.tell()

class appendopener(object):
    '''special opener for files that only read or append.'''

    def __init__(self, opener):
        self.realopener = opener
        # key: file name, value: appendfile name
        self.tmpnames = {}

    def __call__(self, name, mode='r'):
        '''open file.'''

        assert mode in 'ra+'
        try:
            realfp = self.realopener(name, 'r')
        except IOError, err:
            if err.errno != errno.ENOENT: raise
            realfp = self.realopener(name, 'w+')
        tmpname = self.tmpnames.get(name)
        fp = appendfile(realfp, tmpname)
        if tmpname is None:
            self.tmpnames[name] = fp.tmpname
        return fp

    def writedata(self):
        '''copy data from temp files to real files.'''
        # write .d file before .i file.
        tmpnames = self.tmpnames.items()
        tmpnames.sort()
        for name, tmpname in tmpnames:
            fp = open(tmpname, 'rb')
            s = fp.read()
            fp.close()
            os.unlink(tmpname)
            fp = self.realopener(name, 'a')
            fp.write(s)
            fp.close()

# files for changelog and manifest are in different appendopeners, so
# not mixed up together.

class appendchangelog(changelog.changelog, appendopener):
    def __init__(self, opener, version):
        appendopener.__init__(self, opener)
        changelog.changelog.__init__(self, self, version)
    def checkinlinesize(self, fp, tr):
        return

class appendmanifest(manifest.manifest, appendopener):
    def __init__(self, opener, version):
        appendopener.__init__(self, opener)
        manifest.manifest.__init__(self, self, version)
    def checkinlinesize(self, fp, tr):
        return
