"""
changegroup.py - Mercurial changegroup manipulation functions

 Copyright 2006 Matt Mackall <mpm@selenic.com>

This software may be used and distributed according to the terms
of the GNU General Public License, incorporated herein by reference.
"""
import struct
from i18n import gettext as _
from demandload import *
demandload(globals(), "util")

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

