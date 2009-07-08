# changegroup.py - Mercurial changegroup manipulation functions
#
#  Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

from i18n import _
import util
import struct, os, bz2, zlib, tempfile

def getchunk(source):
    """get a chunk from a changegroup"""
    d = source.read(4)
    if not d:
        return ""
    l = struct.unpack(">l", d)[0]
    if l <= 4:
        return ""
    d = source.read(l - 4)
    if len(d) < l - 4:
        raise util.Abort(_("premature EOF reading chunk"
                           " (got %d bytes, expected %d)")
                          % (len(d), l - 4))
    return d

def chunkiter(source):
    """iterate through the chunks in source"""
    while 1:
        c = getchunk(source)
        if not c:
            break
        yield c

def chunkheader(length):
    """build a changegroup chunk header"""
    return struct.pack(">l", length + 4)

def closechunk():
    return struct.pack(">l", 0)

class nocompress(object):
    def compress(self, x):
        return x
    def flush(self):
        return ""

bundletypes = {
    "": ("", nocompress),
    "HG10UN": ("HG10UN", nocompress),
    "HG10BZ": ("HG10", lambda: bz2.BZ2Compressor()),
    "HG10GZ": ("HG10GZ", lambda: zlib.compressobj()),
}

# hgweb uses this list to communicate its preferred type
bundlepriority = ['HG10GZ', 'HG10BZ', 'HG10UN']

def writebundle(cg, filename, bundletype):
    """Write a bundle file and return its filename.

    Existing files will not be overwritten.
    If no filename is specified, a temporary file is created.
    bz2 compression can be turned off.
    The bundle file will be deleted in case of errors.
    """

    fh = None
    cleanup = None
    try:
        if filename:
            fh = open(filename, "wb")
        else:
            fd, filename = tempfile.mkstemp(prefix="hg-bundle-", suffix=".hg")
            fh = os.fdopen(fd, "wb")
        cleanup = filename

        header, compressor = bundletypes[bundletype]
        fh.write(header)
        z = compressor()

        # parse the changegroup data, otherwise we will block
        # in case of sshrepo because we don't know the end of the stream

        # an empty chunkiter is the end of the changegroup
        # a changegroup has at least 2 chunkiters (changelog and manifest).
        # after that, an empty chunkiter is the end of the changegroup
        empty = False
        count = 0
        while not empty or count <= 2:
            empty = True
            count += 1
            for chunk in chunkiter(cg):
                empty = False
                fh.write(z.compress(chunkheader(len(chunk))))
                pos = 0
                while pos < len(chunk):
                    next = pos + 2**20
                    fh.write(z.compress(chunk[pos:next]))
                    pos = next
            fh.write(z.compress(closechunk()))
        fh.write(z.flush())
        cleanup = None
        return filename
    finally:
        if fh is not None:
            fh.close()
        if cleanup is not None:
            os.unlink(cleanup)

def unbundle(header, fh):
    if header == 'HG10UN':
        return fh
    elif not header.startswith('HG'):
        # old client with uncompressed bundle
        def generator(f):
            yield header
            for chunk in f:
                yield chunk
    elif header == 'HG10GZ':
        def generator(f):
            zd = zlib.decompressobj()
            for chunk in f:
                yield zd.decompress(chunk)
    elif header == 'HG10BZ':
        def generator(f):
            zd = bz2.BZ2Decompressor()
            zd.decompress("BZ")
            for chunk in util.filechunkiter(f, 4096):
                yield zd.decompress(chunk)
    return util.chunkbuffer(generator(fh))

def readbundle(fh, fname):
    header = fh.read(6)
    if not header.startswith('HG'):
        raise util.Abort(_('%s: not a Mercurial bundle file') % fname)
    if not header.startswith('HG10'):
        raise util.Abort(_('%s: unknown bundle version') % fname)
    elif header not in bundletypes:
        raise util.Abort(_('%s: unknown bundle compression type') % fname)
    return unbundle(header, fh)
