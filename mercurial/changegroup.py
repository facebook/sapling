"""
changegroup.py - Mercurial changegroup manipulation functions

 Copyright 2006 Matt Mackall <mpm@selenic.com>

This software may be used and distributed according to the terms
of the GNU General Public License, incorporated herein by reference.
"""
from i18n import gettext as _
from demandload import *
demandload(globals(), "struct os bz2 zlib util tempfile")

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

def genchunk(data):
    """build a changegroup chunk"""
    header = struct.pack(">l", len(data)+ 4)
    return "%s%s" % (header, data)

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
            if os.path.exists(filename):
                raise util.Abort(_("file '%s' already exists") % filename)
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
        empty = False
        while not empty:
            empty = True
            for chunk in chunkiter(cg):
                empty = False
                fh.write(z.compress(genchunk(chunk)))
            fh.write(z.compress(closechunk()))
        fh.write(z.flush())
        cleanup = None
        return filename
    finally:
        if fh is not None:
            fh.close()
        if cleanup is not None:
            os.unlink(cleanup)

def readbundle(fh, fname):
    header = fh.read(6)
    if not header.startswith("HG"):
        raise util.Abort(_("%s: not a Mercurial bundle file") % fname)
    elif not header.startswith("HG10"):
        raise util.Abort(_("%s: unknown bundle version") % fname)

    if header == "HG10BZ":
        def generator(f):
            zd = bz2.BZ2Decompressor()
            zd.decompress("BZ")
            for chunk in util.filechunkiter(f, 4096):
                yield zd.decompress(chunk)
        return util.chunkbuffer(generator(fh))
    elif header == "HG10UN":
        return fh

    raise util.Abort(_("%s: unknown bundle compression type")
                     % fname)
