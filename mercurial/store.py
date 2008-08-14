# store.py - repository store handling for Mercurial
#
# Copyright 2008 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from i18n import _
import os, stat, osutil, util

def _buildencodefun():
    e = '_'
    win_reserved = [ord(x) for x in '\\:*?"<>|']
    cmap = dict([ (chr(x), chr(x)) for x in xrange(127) ])
    for x in (range(32) + range(126, 256) + win_reserved):
        cmap[chr(x)] = "~%02x" % x
    for x in range(ord("A"), ord("Z")+1) + [ord(e)]:
        cmap[chr(x)] = e + chr(x).lower()
    dmap = {}
    for k, v in cmap.iteritems():
        dmap[v] = k
    def decode(s):
        i = 0
        while i < len(s):
            for l in xrange(1, 4):
                try:
                    yield dmap[s[i:i+l]]
                    i += l
                    break
                except KeyError:
                    pass
            else:
                raise KeyError
    return (lambda s: "".join([cmap[c] for c in s]),
            lambda s: "".join(list(decode(s))))

encodefilename, decodefilename = _buildencodefun()

def _dirwalk(path, recurse):
    '''yields (filename, size)'''
    for e, kind, st in osutil.listdir(path, stat=True):
        pe = os.path.join(path, e)
        if kind == stat.S_IFDIR:
            if recurse:
                for x in _dirwalk(pe, True):
                    yield x
        elif kind == stat.S_IFREG:
            yield pe, st.st_size

def _calcmode(path):
    try:
        # files in .hg/ will be created using this mode
        mode = os.stat(path).st_mode
            # avoid some useless chmods
        if (0777 & ~util._umask) == (0777 & mode):
            mode = None
    except OSError:
        mode = None
    return mode

class basicstore:
    '''base class for local repository stores'''
    def __init__(self, path, opener):
        self.path = path
        self.createmode = _calcmode(path)
        self.opener = opener(self.path)
        self.opener.createmode = self.createmode

    def join(self, f):
        return os.path.join(self.path, f)

    def _revlogfiles(self, relpath='', recurse=False):
        '''yields (filename, size)'''
        if relpath:
            path = os.path.join(self.path, relpath)
        else:
            path = self.path
        if not os.path.isdir(path):
            return
        striplen = len(self.path) + len(os.sep)
        filetypes = ('.d', '.i')
        for f, size in _dirwalk(path, recurse):
            if (len(f) > 2) and f[-2:] in filetypes:
                yield util.pconvert(f[striplen:]), size

    def datafiles(self, reporterror=None):
        for x in self._revlogfiles('data', True):
            yield x

    def walk(self):
        '''yields (direncoded filename, size)'''
        # yield data files first
        for x in self.datafiles():
            yield x
        # yield manifest before changelog
        meta = util.sort(self._revlogfiles())
        meta.reverse()
        for x in meta:
            yield x

class encodedstore(basicstore):
    def __init__(self, path, opener):
        self.path = os.path.join(path, 'store')
        self.createmode = _calcmode(self.path)
        self.encodefn = encodefilename
        op = opener(self.path)
        op.createmode = self.createmode
        self.opener = lambda f, *args, **kw: op(self.encodefn(f), *args, **kw)

    def datafiles(self, reporterror=None):
        for f, size in self._revlogfiles('data', True):
            try:
                yield decodefilename(f), size
            except KeyError:
                if not reporterror:
                    raise
                reporterror(_("cannot decode filename '%s'") % f)

    def join(self, f):
        return os.path.join(self.path, self.encodefn(f))

def store(requirements, path, opener):
    if 'store' in requirements:
        return encodedstore(path, opener)
    return basicstore(path, opener)
