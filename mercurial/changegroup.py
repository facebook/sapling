# changegroup.py - Mercurial changegroup manipulation functions
#
#  Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
import util
import struct, os, bz2, zlib, tempfile

def readexactly(stream, n):
    '''read n bytes from stream.read and abort if less was available'''
    s = stream.read(n)
    if len(s) < n:
        raise util.Abort(_("stream ended unexpectedly"
                           " (got %d bytes, expected %d)")
                          % (len(s), n))
    return s

def getchunk(stream):
    """return the next chunk from stream as a string"""
    d = readexactly(stream, 4)
    l = struct.unpack(">l", d)[0]
    if l <= 4:
        if l:
            raise util.Abort(_("invalid chunk length %d") % l)
        return ""
    return readexactly(stream, l - 4)

def chunkheader(length):
    """return a changegroup chunk header (string)"""
    return struct.pack(">l", length + 4)

def closechunk():
    """return a changegroup chunk header (string) for a zero-length chunk"""
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

def collector(cl, mmfs, files):
    # Gather information about changeset nodes going out in a bundle.
    # We want to gather manifests needed and filelogs affected.
    def collect(node):
        c = cl.read(node)
        files.update(c[3])
        mmfs.setdefault(c[0], node)
    return collect

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

        # an empty chunkgroup is the end of the changegroup
        # a changegroup has at least 2 chunkgroups (changelog and manifest).
        # after that, an empty chunkgroup is the end of the changegroup
        empty = False
        count = 0
        while not empty or count <= 2:
            empty = True
            count += 1
            while 1:
                chunk = getchunk(cg)
                if not chunk:
                    break
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

def decompressor(fh, alg):
    if alg == 'UN':
        return fh
    elif alg == 'GZ':
        def generator(f):
            zd = zlib.decompressobj()
            for chunk in f:
                yield zd.decompress(chunk)
    elif alg == 'BZ':
        def generator(f):
            zd = bz2.BZ2Decompressor()
            zd.decompress("BZ")
            for chunk in util.filechunkiter(f, 4096):
                yield zd.decompress(chunk)
    else:
        raise util.Abort("unknown bundle compression '%s'" % alg)
    return util.chunkbuffer(generator(fh))

class unbundle10(object):
    def __init__(self, fh, alg):
        self._stream = decompressor(fh, alg)
        self._type = alg
        self.callback = None
    def compressed(self):
        return self._type != 'UN'
    def read(self, l):
        return self._stream.read(l)
    def seek(self, pos):
        return self._stream.seek(pos)
    def tell(self):
        return self._stream.tell()
    def close(self):
        return self._stream.close()

    def chunklength(self):
        d = readexactly(self._stream, 4)
        l = struct.unpack(">l", d)[0]
        if l <= 4:
            if l:
                raise util.Abort(_("invalid chunk length %d") % l)
            return 0
        if self.callback:
            self.callback()
        return l - 4

    def chunk(self):
        """return the next chunk from changegroup 'source' as a string"""
        l = self.chunklength()
        return readexactly(self._stream, l)

    def parsechunk(self):
        l = self.chunklength()
        if not l:
            return {}
        h = readexactly(self._stream, 80)
        node, p1, p2, cs = struct.unpack("20s20s20s20s", h)
        data = readexactly(self._stream, l - 80)
        return dict(node=node, p1=p1, p2=p2, cs=cs, data=data)

class headerlessfixup(object):
    def __init__(self, fh, h):
        self._h = h
        self._fh = fh
    def read(self, n):
        if self._h:
            d, self._h = self._h[:n], self._h[n:]
            if len(d) < n:
                d += readexactly(self._fh, n - len(d))
            return d
        return readexactly(self._fh, n)

def readbundle(fh, fname):
    header = readexactly(fh, 6)

    if not fname:
        fname = "stream"
        if not header.startswith('HG') and header.startswith('\0'):
            fh = headerlessfixup(fh, header)
            header = "HG10UN"

    magic, version, alg = header[0:2], header[2:4], header[4:6]

    if magic != 'HG':
        raise util.Abort(_('%s: not a Mercurial bundle') % fname)
    if version != '10':
        raise util.Abort(_('%s: unknown bundle version %s') % (fname, version))
    return unbundle10(fh, alg)
