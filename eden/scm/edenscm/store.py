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

import errno
import hashlib
import os
import stat
from typing import Optional, Sized

import bindings

from edenscmnative import parsers

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


# pyre-fixme[16]: Module `parsers` has no attribute `encodedir`.
encodedir = getattr(parsers, "encodedir", _encodedir)


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


def _buildencodefun(forfncache):
    """
    >>> enc, dec = _buildencodefun(False)

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

    if not forfncache:
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

    if forfncache:

        def encode(s):
            assert isinstance(s, str), "encode accepts str paths"
            s = encodeutf8(s)
            return decodeutf8(b"".join([cmap[s[c : c + 1]] for c in range(len(s))]))

        return (encode, lambda s: decodeutf8(b"".join(list(decode(s)))))
    else:
        return (
            encodemaybelong,
            lambda s: decodeutf8(b"".join(list(decode(encodeutf8(s))))),
        )


_encodefname, _decodefname = _buildencodefun(True)

# Special version that works with long upper-case file names
_encodefnamelong, _decodefnamelong = _buildencodefun(False)


def encodefilename(s):
    """
    >>> encodefilename('foo.i/bar.d/bla.hg/hi:world?/HELLO')
    'foo.i.hg/bar.d.hg/bla.hg.hg/hi~3aworld~3f/_h_e_l_l_o'
    """
    return _encodefnamelong(encodedir(s))


def decodefilename(s):
    """
    >>> decodefilename('foo.i.hg/bar.d.hg/bla.hg.hg/hi~3aworld~3f/_h_e_l_l_o')
    'foo.i/bar.d/bla.hg/hi:world?/HELLO'
    """
    return decodedir(_decodefnamelong(s))


def _buildlowerencodefun():
    xchr = pycompat.bytechr
    cmap = dict([(inttobyte(x), inttobyte(x)) for x in range(127)])
    for x in _reserved():
        cmap[inttobyte(x)] = encodeutf8("~%02x" % x)
    for x in range(ord("A"), ord("Z") + 1):
        cmap[inttobyte(x)] = encodeutf8(xchr(x).lower())

    def lowerencode(s):
        s = encodeutf8(s)
        return decodeutf8(b"".join([cmap[c] for c in iter(s)]))

    return lowerencode


# pyre-fixme[16]: Module `parsers` has no attribute `lowerencode`.
lowerencode = getattr(parsers, "lowerencode", None) or _buildlowerencodefun()

# Windows reserved names: con, prn, aux, nul, com1..com9, lpt1..lpt9
_winres3 = ("aux", "con", "prn", "nul")  # length 3
_winres4 = ("com", "lpt")  # length 4 (with trailing 1..9)


def _auxencode(path, dotencode):
    """
    Encodes filenames containing names reserved by Windows or which end in
    period or space. Does not touch other single reserved characters c.
    Specifically, c in '\\:*?"<>|' or ord(c) <= 31 are *not* encoded here.
    Additionally encodes space or period at the beginning, if dotencode is
    True. Parameter path is assumed to be all lowercase.
    A segment only needs encoding if a reserved name appears as a
    basename (e.g. "aux", "aux.foo"). A directory or file named "foo.aux"
    doesn't need encoding.

    >>> s = '.foo/aux.txt/txt.aux/con/prn/nul/foo.'
    >>> _auxencode(s.split('/'), True)
    ['~2efoo', 'au~78.txt', 'txt.aux', 'co~6e', 'pr~6e', 'nu~6c', 'foo~2e']
    >>> s = '.com1com2/lpt9.lpt4.lpt1/conprn/com0/lpt0/foo.'
    >>> _auxencode(s.split('/'), False)
    ['.com1com2', 'lp~749.lpt4.lpt1', 'conprn', 'com0', 'lpt0', 'foo~2e']
    >>> _auxencode(['foo. '], True)
    ['foo.~20']
    >>> _auxencode([' .foo'], True)
    ['~20.foo']
    """
    for i, n in enumerate(path):
        if not n:
            continue
        if dotencode and n[0] in ". ":
            n = "~%02x" % ord(n[0:1]) + n[1:]
            path[i] = n
        else:
            l = n.find(".")
            if l == -1:
                l = len(n)
            if (l == 3 and n[:3] in _winres3) or (
                l == 4 and n[3:4] <= "9" and n[3:4] >= "1" and n[:3] in _winres4
            ):
                # encode third letter ('aux' -> 'au~78')
                ec = "~%02x" % ord(n[2:3])
                n = n[0:2] + ec + n[3:]
                path[i] = n
        if n[-1] in ". ":
            # encode last period or space ('foo...' -> 'foo..~2e')
            path[i] = n[:-1] + "~%02x" % ord(n[-1:])
    return path


_maxstorepathlen = 120
_dirprefixlen = 8
_maxshortdirslen: int = 8 * (_dirprefixlen + 1) - 4


def _hashencode(path, dotencode) -> str:
    digest = hashlib.sha1(encodeutf8(path)).hexdigest()
    le = lowerencode(path[5:]).split("/")  # skips prefix 'data/' or 'meta/'
    parts = _auxencode(le, dotencode)
    basename = parts[-1]
    _root, ext = os.path.splitext(basename)
    sdirs = []
    sdirslen = 0
    for p in parts[:-1]:
        d = p[:_dirprefixlen]
        if d[-1] in ". ":
            # Windows can't access dirs ending in period or space
            d = d[:-1] + "_"
        if sdirslen == 0:
            t = len(d)
        else:
            t = sdirslen + 1 + len(d)
            if t > _maxshortdirslen:
                break
        sdirs.append(d)
        sdirslen = t
    dirs = "/".join(sdirs)
    if len(dirs) > 0:
        dirs += "/"
    res = "dh/" + dirs + digest + ext
    spaceleft = _maxstorepathlen - len(res)
    if spaceleft > 0:
        filler = basename[:spaceleft]
        res = "dh/" + dirs + filler + digest + ext
    return res


def _hybridencode(path, dotencode) -> str:
    """encodes path with a length limit

    Encodes all paths that begin with 'data/', according to the following.

    Default encoding (reversible):

    Encodes all uppercase letters 'X' as '_x'. All reserved or illegal
    characters are encoded as '~xx', where xx is the two digit hex code
    of the character (see encodefilename).
    Relevant path components consisting of Windows reserved filenames are
    masked by encoding the third character ('aux' -> 'au~78', see _auxencode).

    Hashed encoding (not reversible):

    If the default-encoded path is longer than _maxstorepathlen, a
    non-reversible hybrid hashing of the path is done instead.
    This encoding uses up to _dirprefixlen characters of all directory
    levels of the lowerencoded path, but not more levels than can fit into
    _maxshortdirslen.
    Then follows the filler followed by the sha digest of the full path.
    The filler is the beginning of the basename of the lowerencoded path
    (the basename is everything after the last path separator). The filler
    is as long as possible, filling in characters from the basename until
    the encoded path has _maxstorepathlen characters (or all chars of the
    basename have been taken).
    The extension (e.g. '.i' or '.d') is preserved.

    The string 'data/' at the beginning is replaced with 'dh/', if the hashed
    encoding was used.
    """
    path = encodedir(path)
    ef = _encodefname(path).split("/")
    res = "/".join(_auxencode(ef, dotencode))
    if len(res) > _maxstorepathlen:
        res = _hashencode(path, dotencode)
    return res


def _pathencode(path: Sized) -> str:
    de = encodedir(path)
    if len(path) > _maxstorepathlen:
        return _hashencode(de, True)
    ef = _encodefname(de).split("/")
    res = "/".join(_auxencode(ef, True))
    if len(res) > _maxstorepathlen:
        return _hashencode(de, True)
    return res


# pyre-fixme[16]: Module `parsers` has no attribute `pathencode`.
_pathencode = getattr(parsers, "pathencode", _pathencode)


def _plainhybridencode(f):
    return _hybridencode(f, False)


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


_data = (
    "data meta 00manifest.d 00manifest.i 00changelog.d 00changelog.i"
    " phaseroots visibleheads"
    " segments hgcommits mutation metalog"
)


def setvfsmode(vfs) -> None:
    vfs.createmode = _calcmode(vfs)


class basicstore(object):
    """base class for local repository stores"""

    def __init__(self, path, vfstype):
        vfs = vfstype(path)
        setvfsmode(vfs)
        self.path = vfs.base
        self.createmode = vfs.createmode
        self.rawvfs = vfs
        self.vfs = vfsmod.filtervfs(vfs, encodedir)
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

    def datafiles(self):
        return self._walk("data", True) + self._walk("meta", True)

    def topfiles(self):
        # yield manifest before changelog
        return reversed(self._walk("", False))

    def walk(self):
        """yields (unencoded, encoded, size)"""
        # yield data files first
        for x in self.datafiles():
            yield x
        for x in self.topfiles():
            yield x

    def copylist(self):
        return ["requires"] + _data.split()

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


class fncache(object):
    # the filename used to be partially encoded
    # hence the encodedir/decodedir dance
    def __init__(self, vfs):
        self.vfs = vfs
        self.entries = None
        self._dirty = False

    def _load(self):
        """fill the entries from the fncache file"""
        self._dirty = False
        try:
            fp = self.vfs("fncache", mode="rb")
        except IOError:
            # skip nonexistent file
            self.entries = set()
            return
        self.entries = set(decodedir(decodeutf8(fp.read())).splitlines())
        if "" in self.entries:
            fp.seek(0)
            for n, line in enumerate(util.iterfile(fp)):
                line = decodeutf8(line)
                if not line.rstrip("\n"):
                    t = _("invalid entry in fncache, line %d") % (n + 1)
                    raise error.Abort(t)
        fp.close()

    def write(self, tr):
        if self._dirty:
            tr.addbackup("fncache")
            fp = self.vfs("fncache", mode="wb", atomictemp=True)
            if self.entries:
                fp.write(encodeutf8(encodedir("\n".join(self.entries) + "\n")))
            fp.close()
            self._dirty = False

    def add(self, fn):
        if self.entries is None:
            self._load()
        if fn not in self.entries:
            self._dirty = True
            self.entries.add(fn)

    def remove(self, fn):
        if self.entries is None:
            self._load()
        try:
            self.entries.remove(fn)
            self._dirty = True
        except KeyError:
            pass

    def __contains__(self, fn):
        if self.entries is None:
            self._load()
        return fn in self.entries

    def __iter__(self):
        if self.entries is None:
            self._load()
        return iter(self.entries)


class metavfs(object):
    """Wrapper vfs that writes data to metalog"""

    metapaths = {}

    def __init__(self):
        super().__init__()
        self._rsrepo = None

    @util.propertycache
    def metalog(self):
        vfs = self.vfs
        metalog = self._rsrepo.metalog()

        # Keys that are previously tracked in metalog.
        tracked = set(pycompat.decodeutf8((metalog.get("tracked") or b"")).split())
        # Keys that should be tracked (specified by config).
        desired = set(self.metapaths)

        # Migrate up (from svfs plain files to metalog).
        for name in desired.difference(tracked):
            data = vfs.tryread(name)
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


class readablestream(object):
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


class writablestream(object):
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


class _fncachevfs(vfsmod.abstractvfs, vfsmod.proxyvfs, metavfs):
    def __init__(self, vfs, fnc, encode):
        vfsmod.proxyvfs.__init__(self, vfs)
        self.fncache = fnc
        self.encode = encode

    def __call__(self, path, mode="r", *args, **kw):
        if path in self.metapaths:
            return self.metaopen(path, mode)
        if mode not in ("r", "rb") and (
            path.startswith("data/") or path.startswith("meta/")
        ):
            self.fncache.add(path)
        return self.vfs(self.encode(path), mode, *args, **kw)

    def join(self, path: "Optional[str]", *insidef: str) -> str:
        if path:
            return self.vfs.join(self.encode(os.path.join(path, *insidef)))
        else:
            return self.vfs.join(path)


class fncachestore(basicstore):
    def __init__(self, path, vfstype, dotencode):
        if dotencode:
            encode = _pathencode
        else:
            encode = _plainhybridencode
        self.encode = encode
        vfs = vfstype(path + "/store")
        self.path = vfs.base
        self.pathsep = self.path + "/"
        self.createmode = _calcmode(vfs)
        vfs.createmode = self.createmode
        self.rawvfs = vfs
        fnc = fncache(vfs)
        self.fncache = fnc
        self.vfs = _fncachevfs(vfs, fnc, encode)
        self.opener = self.vfs

    def join(self, f):
        return self.pathsep + self.encode(f)

    def getsize(self, path):
        return self.rawvfs.stat(path).st_size

    def datafiles(self):
        for f in sorted(self.fncache):
            ef = self.encode(f)
            try:
                yield f, ef, self.getsize(ef)
            except OSError as err:
                if err.errno != errno.ENOENT:
                    raise

    def copylist(self):
        d = (
            "data meta dh fncache phaseroots visibleheads"
            " 00manifest.d 00manifest.i 00changelog.d 00changelog.i"
            " segments hgcommits mutation metalog"
        )
        return ["requires", "00changelog.i", "store/requires"] + [
            "store/" + f for f in d.split()
        ]

    def write(self, tr):
        self.fncache.write(tr)

    def invalidatecaches(self):
        self.fncache.entries = None

    def markremoved(self, fn):
        self.fncache.remove(fn)

    def _exists(self, f):
        ef = self.encode(f)
        try:
            self.getsize(ef)
            return True
        except OSError as err:
            if err.errno != errno.ENOENT:
                raise
            # nonexistent entry
            return False

    def __contains__(self, path):
        """Checks if the store contains path"""
        path = "/".join(("data", path))
        # check for files (exact match)
        e = path + ".i"
        if e in self.fncache and self._exists(e):
            return True
        # now check for directories (prefix match)
        if not path.endswith("/"):
            path += "/"
        for e in self.fncache:
            if e.startswith(path) and self._exists(e):
                return True
        return False


def store(requirements, path, vfstype, uiconfig=None) -> fncachestore:
    store = fncachestore(path, vfstype, "dotencode" in requirements)
    # Change remotenames and visibleheads to be backed by metalog,
    # so they can be atomically read or written.
    store.vfs.metapaths = set(bindings.metalog.tracked())
    return store
