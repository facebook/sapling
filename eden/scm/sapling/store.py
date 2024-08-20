# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# store.py - repository store handling for Mercurial
#
# Copyright 2008 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import stat
from typing import Optional

import bindings

parsers = bindings.cext.parsers

from . import error, pycompat, util, vfs as vfsmod
from .i18n import _
from .pycompat import decodeutf8, encodeutf8, inttobyte, range


# This avoids a collision between a file named foo and a dir named
# foo.i or foo.d
def _encodedir(path):
    """
    >>> _encodedir('data/foo.i')
    'data/foo.i'
    >>> _encodedir('data/foo.i/bla.i')
    'data/foo.i.hg/bla.i'
    >>> _encodedir('data/foo.i.hg/bla.i')
    'data/foo.i.hg.hg/bla.i'
    >>> _encodedir('data/foo.i\\ndata/foo.i/bla.i\\ndata/foo.i.hg/bla.i\\n')
    'data/foo.i\\ndata/foo.i.hg/bla.i\\ndata/foo.i.hg.hg/bla.i\\n'
    """
    return (
        path.replace(".hg/", ".hg.hg/")
        .replace(".i/", ".i.hg/")
        .replace(".d/", ".d.hg/")
    )


encodedir = _encodedir


def decodedir(path):
    """
    >>> decodedir('data/foo.i')
    'data/foo.i'
    >>> decodedir('data/foo.i.hg/bla.i')
    'data/foo.i/bla.i'
    >>> decodedir('data/foo.i.hg.hg/bla.i')
    'data/foo.i.hg/bla.i'
    """
    if ".hg/" not in path:
        return path
    return (
        path.replace(".d.hg/", ".d/")
        .replace(".i.hg/", ".i/")
        .replace(".hg.hg/", ".hg/")
    )


def _reserved():
    """characters that are problematic for filesystems

    * ascii escapes (0..31)
    * ascii hi (126..255)
    * windows specials

    these characters will be escaped by encodefunctions
    """
    winreserved = [ord(x) for x in '\\:*?"<>|']
    for x in range(32):
        yield x
    for x in range(126, 256):
        yield x
    for x in winreserved:
        yield x


def _buildencodefun():
    """
    >>> enc, dec = _buildencodefun()

    >>> enc('nothing/special.txt')
    'nothing/special.txt'
    >>> dec('nothing/special.txt')
    'nothing/special.txt'

    >>> enc('HELLO')
    '_h_e_l_l_o'
    >>> dec('_h_e_l_l_o')
    'HELLO'

    >>> enc('hello:world?')
    'hello~3aworld~3f'
    >>> dec('hello~3aworld~3f')
    'hello:world?'

    >>> enc('X' * 128)
    'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX'
    >>> enc('X' * 127)
    '_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x_x'
    >>> path = '/'.join(['Z', 'X' * 128, 'Y' * 127])
    >>> enc(path)
    '_z/XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX/_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y_y'
    >>> dec(enc(path)) == path
    True
    >>> dec(enc('X' * 128)) == 'X' * 128
    True
    >>> dec(enc('X' * 127)) == 'X' * 127
    True
    >>> enc('/')
    '/'
    >>> dec('/')
    '/'

    >>> enc('X' * 253 + '_')
    'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX__'
    >>> enc('X' * 254 + '_')
    'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX:'
    >>> path = '/'.join(['Z', 'X_' * 85, 'Y_' * 86])
    >>> enc(path)
    '_z/X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__X__/Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:Y:'
    >>> dec(enc(path)) == path
    True
    >>> dec(enc('_' * 128)) == '_' * 128
    True
    >>> dec(enc('_' * 127)) == '_' * 127
    True
    """
    e = "_"
    xchr = pycompat.bytechr
    asciistr = list(map(inttobyte, range(127)))
    capitals = list(range(ord("A"), ord("Z") + 1))

    cmap = dict((x, x) for x in asciistr)
    for x in _reserved():
        cmap[inttobyte(x)] = encodeutf8("~%02x" % x)
    for x in capitals + [ord(e)]:
        cmap[inttobyte(x)] = encodeutf8(e + xchr(x).lower())

    dmap = {}
    for k, v in cmap.items():
        dmap[v] = k

    cmaplong = cmap.copy()

    for i in capitals:
        c = inttobyte(i)
        cmaplong[c] = c
        assert c not in dmap
        dmap[c] = c

    cmapverylong = cmaplong.copy()
    cmapverylong[b"_"] = b":"
    assert b":" not in dmap
    dmap[b":"] = b"_"

    def encodecomp(comp):
        assert isinstance(comp, str), "encodecomp accepts str paths"
        comp = encodeutf8(comp)
        comp = [comp[i : i + 1] for i in range(len(comp))]
        encoded = b"".join(cmap[c] for c in comp)
        if len(encoded) > 255:
            encoded = b"".join(cmaplong[c] for c in comp)
        if len(encoded) > 255:
            encoded = b"".join(cmapverylong[c] for c in comp)
        return decodeutf8(encoded)

    def encodemaybelong(path):
        assert isinstance(path, str), "encodemaybelong accepts str paths"
        return "/".join(map(encodecomp, path.split("/")))

    def decode(s):
        assert isinstance(s, bytes), "decode accepts bytes paths"
        i = 0
        while i < len(s):
            for l in range(1, 4):
                try:
                    yield dmap[s[i : i + l]]
                    i += l
                    break
                except KeyError:
                    pass
            else:
                raise KeyError

    return (
        encodemaybelong,
        lambda s: decodeutf8(b"".join(list(decode(encodeutf8(s))))),
    )


# Special version that works with long upper-case file names
_encodefnamelong = _buildencodefun()[0]


def encodefilename(s):
    """
    >>> encodefilename('foo.i/bar.d/bla.hg/hi:world?/HELLO')
    'foo.i.hg/bar.d.hg/bla.hg.hg/hi~3aworld~3f/_h_e_l_l_o'
    """
    return _encodefnamelong(encodedir(s))


def _calcmode(vfs):
    try:
        # files in .hg/ will be created using this mode
        mode = vfs.stat().st_mode
        # avoid some useless chmods
        if (0o777 & ~util.umask) == (0o777 & mode):
            mode = None
    except OSError:
        mode = None
    return mode


def setvfsmode(vfs) -> None:
    vfs.createmode = _calcmode(vfs)


class basicstore:
    """base class for local repository stores"""

    def __init__(self, path, vfstype):
        path = path + "/store"
        vfs = vfstype(path)
        setvfsmode(vfs)
        self.path = vfs.base
        self.createmode = vfs.createmode
        self.rawvfs = vfs
        self.vfs = metavfs(vfs)
        self.opener = self.vfs

    def join(self, f):
        return self.path + "/" + encodedir(f)

    def _walk(self, relpath, recurse):
        """yields (unencoded, encoded, size)"""
        path = self.path
        if relpath:
            path += "/" + relpath
        striplen = len(self.path) + 1
        l = []
        if self.rawvfs.isdir(path):
            visit = [path]
            readdir = self.rawvfs.readdir
            while visit:
                p = visit.pop()
                for f, kind, st in readdir(p, stat=True):
                    fp = p + "/" + f
                    if kind == stat.S_IFREG and f[-2:] in (".d", ".i"):
                        n = util.pconvert(fp[striplen:])
                        l.append((decodedir(n), n, st.st_size))
                    elif kind == stat.S_IFDIR and recurse:
                        visit.append(fp)
        l.sort()
        return l

    def topfiles(self):
        # yield manifest before changelog
        return reversed(self._walk("", False))

    def walk(self):
        """yields (unencoded, encoded, size)"""
        # yield data files first
        for x in self.topfiles():
            yield x

    def copylist(self):
        d = (
            "data meta dh fncache indexedlogdatastore indexedloghistorystore phaseroots visibleheads"
            " 00manifest.d 00manifest.i 00changelog.d 00changelog.i"
            " segments hgcommits lfs manifests mutation metalog"
            " revlogmeta"
        )
        return ["requires", "00changelog.i", "store/requires"] + [
            "store/" + f for f in d.split()
        ]

    def write(self, tr):
        pass

    def invalidatecaches(self):
        pass

    def markremoved(self, fn):
        pass

    def __contains__(self, path):
        """Checks if the store contains path"""
        path = "/".join(("data", path))
        # file?
        if self.vfs.exists(path + ".i"):
            return True
        # dir?
        if not path.endswith("/"):
            path = path + "/"
        return self.vfs.exists(path)


class metavfs(util.proxy_wrapper, vfsmod.abstractvfs):
    """Wrapper vfs that writes data to metalog"""

    def __init__(self, vfs):
        super().__init__(vfs, _rsrepo=None, metapaths=set(bindings.metalog.tracked()))

    @util.propertycache
    def metalog(self):
        metalog = self._rsrepo.metalog()

        # Keys that are previously tracked in metalog.
        tracked = set(pycompat.decodeutf8((metalog.get("tracked") or b"")).split())
        # Keys that should be tracked (specified by config).
        desired = set(self.metapaths)

        # Migrate up (from svfs plain files to metalog).
        for name in desired.difference(tracked):
            data = self.inner.tryread(name)
            if data is not None:
                metalog[name] = data

        # Migrating down is a no-op, since we double-write to svfs too.

        metalog["tracked"] = "\n".join(sorted(desired)).encode("utf-8")

        try:
            # XXX: This is racy.
            metalog.commit("migrate from vfs", int(util.timer()))
        except Exception:
            pass

        return metalog

    def invalidatemetalog(self):
        self._rsrepo.invalidatemetalog()
        return self.__dict__.pop("metalog", None)

    def metaopen(self, path, mode="r"):
        assert path in self.metapaths
        # Return a virtual file that is backed by self.metalog
        if mode in {"r", "rb"}:
            return readablestream(self.metalog[path] or b"")
        elif mode in {"w", "wb"}:

            def write(content, path=path, self=self):
                self.metalog.set(path, content)
                # Also write to disk for compatibility (ex. shell completion
                # script might read them).
                legacypath = self.join(path)
                util.replacefile(legacypath, content)

            return writablestream(write)
        else:
            raise error.ProgrammingError("mode %s is unsupported for %s" % (mode, path))

    def __call__(self, path, mode="r", *args, **kw):
        if path in self.metapaths:
            return self.metaopen(path, mode)
        return self.inner(path, mode, *args, **kw)

    def join(self, path: "Optional[str]", *insidef: str) -> str:
        return self.inner.join(path)


class readablestream:
    """Similar to stringio, but also works in a with context"""

    def __init__(self, data):
        self.stream = util.stringio(data)

    def __enter__(self):
        return self.stream

    def __exit__(self, exctype, excval, exctb):
        pass

    def __getattr__(self, name):
        return getattr(self.stream, name)

    def close(self):
        pass


class writablestream:
    """Writable stringio that writes to specified place on close"""

    def __init__(self, writefunc):
        self.writefunc = writefunc
        self.stream = util.stringio()
        self.closed = False

    def __enter__(self):
        assert not self.closed
        return self.stream

    def __exit__(self, exctype, excval, exctb):
        self.close()

    def __getattr__(self, name):
        assert not self.closed
        return getattr(self.stream, name)

    def __del__(self):
        self.close()

    def close(self):
        if self.closed:
            return
        self.closed = True
        value = self.stream.getvalue()
        self.writefunc(value)


def store(requirements, path, vfstype, uiconfig=None) -> basicstore:
    store = basicstore(path, vfstype)
    # Change remotenames and visibleheads to be backed by metalog,
    # so they can be atomically read or written.
    return store
