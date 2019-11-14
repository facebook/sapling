# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# manifest.py - manifest revision class for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import heapq
import itertools
import os
import struct

from . import error, mdiff, policy, revlog, util
from .i18n import _
from .node import bin, hex


parsers = policy.importmod(r"parsers")
propertycache = util.propertycache


def _parse(data):
    """Generates (path, node, flags) tuples from a manifest text"""
    # This method does a little bit of excessive-looking
    # precondition checking. This is so that the behavior of this
    # class exactly matches its C counterpart to try and help
    # prevent surprise breakage for anyone that develops against
    # the pure version.
    if data and data[-1:] != "\n":
        raise ValueError("Manifest did not end in a newline.")
    prev = None
    for l in data.splitlines():
        if prev is not None and prev > l:
            raise ValueError("Manifest lines not in sorted order.")
        prev = l
        f, n = l.split("\0")
        if len(n) > 40:
            yield f, bin(n[:40]), n[40:]
        else:
            yield f, bin(n), ""


def _text(it):
    """Given an iterator over (path, node, flags) tuples, returns a manifest
    text"""
    files = []
    lines = []
    _hex = revlog.hex
    for f, n, fl in it:
        files.append(f)
        # if this is changed to support newlines in filenames,
        # be sure to check the templates/ dir again (especially *-raw.tmpl)
        lines.append("%s\0%s%s\n" % (f, _hex(n), fl))

    _checkforbidden(files)
    return "".join(lines)


class lazymanifestiter(object):
    def __init__(self, lm):
        self.pos = 0
        self.lm = lm

    def __iter__(self):
        return self

    def next(self):
        try:
            data, pos = self.lm._get(self.pos)
        except IndexError:
            raise StopIteration
        if pos == -1:
            self.pos += 1
            return data[0]
        self.pos += 1
        zeropos = data.find("\x00", pos)
        return data[pos:zeropos]

    __next__ = next


class lazymanifestiterentries(object):
    def __init__(self, lm):
        self.lm = lm
        self.pos = 0

    def __iter__(self):
        return self

    def next(self):
        try:
            data, pos = self.lm._get(self.pos)
        except IndexError:
            raise StopIteration
        if pos == -1:
            self.pos += 1
            return data
        zeropos = data.find("\x00", pos)
        hashval = unhexlify(data, self.lm.extrainfo[self.pos], zeropos + 1, 40)
        flags = self.lm._getflags(data, self.pos, zeropos)
        self.pos += 1
        return (data[pos:zeropos], hashval, flags)

    __next__ = next


def unhexlify(data, extra, pos, length):
    s = bin(data[pos : pos + length])
    if extra:
        s += chr(extra & 0xFF)
    return s


def _cmp(a, b):
    return (a > b) - (a < b)


class purelazymanifest(object):
    def __init__(self, data, positions=None, extrainfo=None, extradata=None):
        if positions is None:
            self.positions = self.findlines(data)
            self.extrainfo = [0] * len(self.positions)
            self.data = data
            self.extradata = []
        else:
            self.positions = positions[:]
            self.extrainfo = extrainfo[:]
            self.extradata = extradata[:]
            self.data = data

    def findlines(self, data):
        if not data:
            return []
        pos = data.find("\n")
        if pos == -1 or data[-1:] != "\n":
            raise ValueError("Manifest did not end in a newline.")
        positions = [0]
        prev = data[: data.find("\x00")]
        while pos < len(data) - 1 and pos != -1:
            positions.append(pos + 1)
            nexts = data[pos + 1 : data.find("\x00", pos + 1)]
            if nexts < prev:
                raise ValueError("Manifest lines not in sorted order.")
            prev = nexts
            pos = data.find("\n", pos + 1)
        return positions

    def _get(self, index):
        # get the position encoded in pos:
        #   positive number is an index in 'data'
        #   negative number is in extrapieces
        pos = self.positions[index]
        if pos >= 0:
            return self.data, pos
        return self.extradata[-pos - 1], -1

    def _getkey(self, pos):
        if pos >= 0:
            return self.data[pos : self.data.find("\x00", pos + 1)]
        return self.extradata[-pos - 1][0]

    def bsearch(self, key):
        first = 0
        last = len(self.positions) - 1

        while first <= last:
            midpoint = (first + last) // 2
            nextpos = self.positions[midpoint]
            candidate = self._getkey(nextpos)
            r = _cmp(key, candidate)
            if r == 0:
                return midpoint
            else:
                if r < 0:
                    last = midpoint - 1
                else:
                    first = midpoint + 1
        return -1

    def bsearch2(self, key):
        # same as the above, but will always return the position
        # done for performance reasons
        first = 0
        last = len(self.positions) - 1

        while first <= last:
            midpoint = (first + last) // 2
            nextpos = self.positions[midpoint]
            candidate = self._getkey(nextpos)
            r = _cmp(key, candidate)
            if r == 0:
                return (midpoint, True)
            else:
                if r < 0:
                    last = midpoint - 1
                else:
                    first = midpoint + 1
        return (first, False)

    def __contains__(self, key):
        return self.bsearch(key) != -1

    def _getflags(self, data, needle, pos):
        start = pos + 41
        end = data.find("\n", start)
        if end == -1:
            end = len(data) - 1
        if start == end:
            return ""
        return self.data[start:end]

    def __getitem__(self, key):
        if not isinstance(key, bytes):
            raise TypeError("getitem: manifest keys must be a bytes.")
        needle = self.bsearch(key)
        if needle == -1:
            raise KeyError
        data, pos = self._get(needle)
        if pos == -1:
            return (data[1], data[2])
        zeropos = data.find("\x00", pos)
        assert 0 <= needle <= len(self.positions)
        assert len(self.extrainfo) == len(self.positions)
        hashval = unhexlify(data, self.extrainfo[needle], zeropos + 1, 40)
        flags = self._getflags(data, needle, zeropos)
        return (hashval, flags)

    def __delitem__(self, key):
        needle, found = self.bsearch2(key)
        if not found:
            raise KeyError
        cur = self.positions[needle]
        self.positions = self.positions[:needle] + self.positions[needle + 1 :]
        self.extrainfo = self.extrainfo[:needle] + self.extrainfo[needle + 1 :]
        if cur >= 0:
            self.data = self.data[:cur] + "\x00" + self.data[cur + 1 :]

    def __setitem__(self, key, value):
        if not isinstance(key, bytes):
            raise TypeError("setitem: manifest keys must be a byte string.")
        if not isinstance(value, tuple) or len(value) != 2:
            raise TypeError("Manifest values must be a tuple of (node, flags).")
        hashval = value[0]
        if not isinstance(hashval, bytes) or not 20 <= len(hashval) <= 22:
            raise TypeError("node must be a 20-byte byte string")
        flags = value[1]
        if len(hashval) == 22:
            hashval = hashval[:-1]
        if not isinstance(flags, bytes) or len(flags) > 1:
            raise TypeError("flags must a 0 or 1 byte string, got %r", flags)
        needle, found = self.bsearch2(key)
        if found:
            # put the item
            pos = self.positions[needle]
            if pos < 0:
                self.extradata[-pos - 1] = (key, hashval, value[1])
            else:
                # just don't bother
                self.extradata.append((key, hashval, value[1]))
                self.positions[needle] = -len(self.extradata)
        else:
            # not found, put it in with extra positions
            self.extradata.append((key, hashval, value[1]))
            self.positions = (
                self.positions[:needle]
                + [-len(self.extradata)]
                + self.positions[needle:]
            )
            self.extrainfo = self.extrainfo[:needle] + [0] + self.extrainfo[needle:]

    def copy(self):
        # XXX call _compact like in C?
        return _lazymanifest(self.data, self.positions, self.extrainfo, self.extradata)

    def _compact(self):
        # hopefully not called TOO often
        if len(self.extradata) == 0:
            return
        l = []
        last_cut = 0
        i = 0
        offset = 0
        self.extrainfo = [0] * len(self.positions)
        while i < len(self.positions):
            if self.positions[i] >= 0:
                cur = self.positions[i]
                last_cut = cur
                while True:
                    self.positions[i] = offset
                    i += 1
                    if i == len(self.positions) or self.positions[i] < 0:
                        break
                    offset += self.positions[i] - cur
                    cur = self.positions[i]
                end_cut = self.data.find("\n", cur)
                if end_cut != -1:
                    end_cut += 1
                offset += end_cut - cur
                l.append(self.data[last_cut:end_cut])
            else:
                while i < len(self.positions) and self.positions[i] < 0:
                    cur = self.positions[i]
                    t = self.extradata[-cur - 1]
                    l.append(self._pack(t))
                    self.positions[i] = offset
                    if len(t[1]) > 20:
                        self.extrainfo[i] = ord(t[1][21])
                    offset += len(l[-1])
                    i += 1
        self.data = "".join(l)
        self.extradata = []

    def _pack(self, d):
        return d[0] + "\x00" + hex(d[1][:20]) + d[2] + "\n"

    def text(self):
        self._compact()
        return self.data

    def diff(self, m2):
        """Finds changes between the current manifest and m2."""
        # XXX think whether efficiency matters here
        diff = {}

        for fn, e1, flags in self.iterentries():
            if fn not in m2:
                diff[fn] = (e1, flags), (None, "")
            else:
                e2 = m2[fn]
                if (e1, flags) != e2:
                    diff[fn] = (e1, flags), e2

        for fn, e2, flags in m2.iterentries():
            if fn not in self:
                diff[fn] = (None, ""), (e2, flags)

        return diff

    def iterentries(self):
        return lazymanifestiterentries(self)

    def iterkeys(self):
        return lazymanifestiter(self)

    def __iter__(self):
        return lazymanifestiter(self)

    def __len__(self):
        return len(self.positions)

    def filtercopy(self, filterfn):
        # XXX should be optimized
        c = _lazymanifest("")
        for f, n, fl in self.iterentries():
            if filterfn(f):
                c[f] = n, fl
        return c


try:
    _lazymanifest = parsers.lazymanifest
except AttributeError:
    _lazymanifest = purelazymanifest


class manifestdict(object):
    def __init__(self, data=""):
        self._lm = _lazymanifest(data)

    def __getitem__(self, key):
        return self._lm[key][0]

    def find(self, key):
        return self._lm[key]

    def __len__(self):
        return len(self._lm)

    def __nonzero__(self):
        # nonzero is covered by the __len__ function, but implementing it here
        # makes it easier for extensions to override.
        return len(self._lm) != 0

    __bool__ = __nonzero__

    def __setitem__(self, key, node):
        self._lm[key] = node, self.flags(key, "")

    def __contains__(self, key):
        if key is None:
            return False
        return key in self._lm

    def __delitem__(self, key):
        del self._lm[key]

    def __iter__(self):
        return self._lm.__iter__()

    def iterkeys(self):
        return self._lm.iterkeys()

    def keys(self):
        return list(self.iterkeys())

    def filesnotin(self, m2, matcher=None):
        """Set of files in this manifest that are not in the other"""
        if matcher:
            m1 = self.matches(matcher)
            m2 = m2.matches(matcher)
            return m1.filesnotin(m2)
        diff = self.diff(m2)
        files = set(
            filepath
            for filepath, hashflags in diff.iteritems()
            if hashflags[1][0] is None
        )
        return files

    @propertycache
    def _dirs(self):
        return util.dirs(self)

    def dirs(self):
        return self._dirs

    def hasdir(self, dir):
        return dir in self._dirs

    def _filesfastpath(self, match):
        """Checks whether we can correctly and quickly iterate over matcher
        files instead of over manifest files."""
        files = match.files()
        return len(files) < 100 and (
            match.isexact() or (match.prefix() and all(fn in self for fn in files))
        )

    def walk(self, match):
        """Generates matching file names.

        Equivalent to manifest.matches(match).iterkeys(), but without creating
        an entirely new manifest.

        It also reports nonexistent files by marking them bad with match.bad().
        """
        if match.always():
            for f in iter(self):
                yield f
            return

        fset = set(match.files())

        # avoid the entire walk if we're only looking for specific files
        if self._filesfastpath(match):
            for fn in sorted(fset):
                yield fn
            return

        for fn in self:
            if fn in fset:
                # specified pattern is the exact name
                fset.remove(fn)
            if match(fn):
                yield fn

        # for dirstate.walk, files=[''] means "walk the whole tree".
        # follow that here, too
        fset.discard("")

        for fn in sorted(fset):
            if not self.hasdir(fn):
                match.bad(fn, None)

    def matches(self, match):
        """generate a new manifest filtered by the match argument"""
        if match.always():
            return self.copy()

        if self._filesfastpath(match):
            m = manifestdict()
            lm = self._lm
            for fn in match.files():
                if fn in lm:
                    m._lm[fn] = lm[fn]
            return m

        m = manifestdict()
        m._lm = self._lm.filtercopy(match)
        return m

    def diff(self, m2, matcher=None):
        """Finds changes between the current manifest and m2.

        Args:
          m2: the manifest to which this manifest should be compared.

        The result is returned as a dict with filename as key and
        values of the form ((n1,fl1),(n2,fl2)), where n1/n2 is the
        nodeid in the current/other manifest and fl1/fl2 is the flag
        in the current/other manifest. Where the file does not exist,
        the nodeid will be None and the flags will be the empty
        string.
        """
        if matcher:
            m1 = self.matches(matcher)
            m2 = m2.matches(matcher)
            return m1.diff(m2)
        return self._lm.diff(m2._lm)

    def setflag(self, key, flag):
        self._lm[key] = self[key], flag

    def get(self, key, default=None):
        try:
            return self._lm[key][0]
        except KeyError:
            return default

    def flags(self, key, default=""):
        try:
            return self._lm[key][1]
        except KeyError:
            return default

    def copy(self):
        c = manifestdict()
        c._lm = self._lm.copy()
        return c

    def items(self):
        return (x[:2] for x in self._lm.iterentries())

    iteritems = items

    def iterentries(self):
        return self._lm.iterentries()

    def text(self):
        # use (probably) native version for v1
        return self._lm.text()

    def fastdelta(self, base, changes):
        """Given a base manifest text as a bytearray and a list of changes
        relative to that text, compute a delta that can be used by revlog.
        """
        delta = []
        dstart = None
        dend = None
        dline = [""]
        start = 0
        # zero copy representation of base as a buffer
        addbuf = util.buffer(base)

        changes = list(changes)
        if len(changes) < 1000:
            # start with a readonly loop that finds the offset of
            # each line and creates the deltas
            for f, todelete in changes:
                # bs will either be the index of the item or the insert point
                start, end = _msearch(addbuf, f, start)
                if not todelete:
                    h, fl = self._lm[f]
                    l = "%s\0%s%s\n" % (f, revlog.hex(h), fl)
                else:
                    if start == end:
                        # item we want to delete was not found, error out
                        raise AssertionError(_("failed to remove %s from manifest") % f)
                    l = ""
                if dstart is not None and dstart <= start and dend >= start:
                    if dend < end:
                        dend = end
                    if l:
                        dline.append(l)
                else:
                    if dstart is not None:
                        delta.append([dstart, dend, "".join(dline)])
                    dstart = start
                    dend = end
                    dline = [l]

            if dstart is not None:
                delta.append([dstart, dend, "".join(dline)])
            # apply the delta to the base, and get a delta for addrevision
            deltatext, arraytext = _addlistdelta(base, delta)
        else:
            # For large changes, it's much cheaper to just build the text and
            # diff it.
            arraytext = bytearray(self.text())
            deltatext = mdiff.textdiff(util.buffer(base), util.buffer(arraytext))

        return arraytext, deltatext


def _msearch(m, s, lo=0, hi=None):
    """return a tuple (start, end) that says where to find s within m.

    If the string is found m[start:end] are the line containing
    that string.  If start == end the string was not found and
    they indicate the proper sorted insertion point.

    m should be a buffer, a memoryview or a byte string.
    s is a byte string"""

    def advance(i, c):
        while i < lenm and m[i : i + 1] != c:
            i += 1
        return i

    if not s:
        return (lo, lo)
    lenm = len(m)
    if not hi:
        hi = lenm
    while lo < hi:
        mid = (lo + hi) // 2
        start = mid
        while start > 0 and m[start - 1 : start] != "\n":
            start -= 1
        end = advance(start, "\0")
        if bytes(m[start:end]) < s:
            # we know that after the null there are 40 bytes of sha1
            # this translates to the bisect lo = mid + 1
            lo = advance(end + 40, "\n") + 1
        else:
            # this translates to the bisect hi = mid
            hi = start
    end = advance(lo, "\0")
    found = m[lo:end]
    if s == found:
        # we know that after the null there are 40 bytes of sha1
        end = advance(end + 40, "\n")
        return (lo, end + 1)
    else:
        return (lo, lo)


def _checkforbidden(l):
    """Check filenames for illegal characters."""
    for f in l:
        if "\n" in f or "\r" in f:
            raise error.RevlogError(
                _("'\\n' and '\\r' disallowed in filenames: %r") % f
            )


# apply the changes collected during the bisect loop to our addlist
# return a delta suitable for addrevision
def _addlistdelta(addlist, x):
    # for large addlist arrays, building a new array is cheaper
    # than repeatedly modifying the existing one
    currentposition = 0
    newaddlist = bytearray()

    for start, end, content in x:
        newaddlist += addlist[currentposition:start]
        if content:
            newaddlist += bytearray(content)

        currentposition = end

    newaddlist += addlist[currentposition:]

    deltatext = "".join(
        struct.pack(">lll", start, end, len(content)) + content
        for start, end, content in x
    )
    return deltatext, newaddlist


def _splittopdir(f):
    if "/" in f:
        dir, subpath = f.split("/", 1)
        return dir + "/", subpath
    else:
        return "", f


_noop = lambda s: None


class treemanifest(object):
    def __init__(self, dir="", text=""):
        self._dir = dir
        self._node = revlog.nullid
        self._loadfunc = _noop
        self._copyfunc = _noop
        self._dirty = False
        self._dirs = {}
        # Using _lazymanifest here is a little slower than plain old dicts
        self._files = {}
        self._flags = {}
        if text:

            def readsubtree(subdir, subm):
                raise AssertionError(
                    "treemanifest constructor only accepts " "flat manifests"
                )

            self.parse(text, readsubtree)
            self._dirty = True  # Mark flat manifest dirty after parsing

    def _subpath(self, path):
        return self._dir + path

    def __len__(self):
        self._load()
        size = len(self._files)
        for m in self._dirs.values():
            size += m.__len__()
        return size

    def _isempty(self):
        self._load()  # for consistency; already loaded by all callers
        return not self._files and (
            not self._dirs or all(m._isempty() for m in self._dirs.values())
        )

    def __repr__(self):
        return "<treemanifest dir=%s, node=%s, loaded=%s, dirty=%s at 0x%x>" % (
            self._dir,
            revlog.hex(self._node),
            bool(self._loadfunc is _noop),
            self._dirty,
            id(self),
        )

    def dir(self):
        """The directory that this tree manifest represents, including a
        trailing '/'. Empty string for the repo root directory."""
        return self._dir

    def node(self):
        """This node of this instance. nullid for unsaved instances. Should
        be updated when the instance is read or written from a revlog.
        """
        assert not self._dirty
        return self._node

    def setnode(self, node):
        self._node = node
        self._dirty = False

    def iterentries(self):
        self._load()
        for p, n in sorted(itertools.chain(self._dirs.items(), self._files.items())):
            if p in self._files:
                yield self._subpath(p), n, self._flags.get(p, "")
            else:
                for x in n.iterentries():
                    yield x

    def items(self):
        self._load()
        for p, n in sorted(itertools.chain(self._dirs.items(), self._files.items())):
            if p in self._files:
                yield self._subpath(p), n
            else:
                for f, sn in n.iteritems():
                    yield f, sn

    iteritems = items

    def iterkeys(self):
        self._load()
        for p in sorted(itertools.chain(self._dirs, self._files)):
            if p in self._files:
                yield self._subpath(p)
            else:
                for f in self._dirs[p].iterkeys():
                    yield f

    def keys(self):
        return list(self.iterkeys())

    def __iter__(self):
        return self.iterkeys()

    def __contains__(self, f):
        if f is None:
            return False
        self._load()
        dir, subpath = _splittopdir(f)
        if dir:
            if dir not in self._dirs:
                return False
            return self._dirs[dir].__contains__(subpath)
        else:
            return f in self._files

    def get(self, f, default=None):
        self._load()
        dir, subpath = _splittopdir(f)
        if dir:
            if dir not in self._dirs:
                return default
            return self._dirs[dir].get(subpath, default)
        else:
            return self._files.get(f, default)

    def __getitem__(self, f):
        self._load()
        dir, subpath = _splittopdir(f)
        if dir:
            return self._dirs[dir].__getitem__(subpath)
        else:
            return self._files[f]

    def flags(self, f):
        self._load()
        dir, subpath = _splittopdir(f)
        if dir:
            if dir not in self._dirs:
                return ""
            return self._dirs[dir].flags(subpath)
        else:
            if f in self._dirs:
                return ""
            return self._flags.get(f, "")

    def find(self, f):
        self._load()
        dir, subpath = _splittopdir(f)
        if dir:
            return self._dirs[dir].find(subpath)
        else:
            return self._files[f], self._flags.get(f, "")

    def __delitem__(self, f):
        self._load()
        dir, subpath = _splittopdir(f)
        if dir:
            self._dirs[dir].__delitem__(subpath)
            # If the directory is now empty, remove it
            if self._dirs[dir]._isempty():
                del self._dirs[dir]
        else:
            del self._files[f]
            if f in self._flags:
                del self._flags[f]
        self._dirty = True

    def __setitem__(self, f, n):
        assert n is not None
        self._load()
        dir, subpath = _splittopdir(f)
        if dir:
            if dir not in self._dirs:
                self._dirs[dir] = treemanifest(self._subpath(dir))
            self._dirs[dir].__setitem__(subpath, n)
        else:
            self._files[f] = n[:21]  # to match manifestdict's behavior
        self._dirty = True

    def _load(self):
        if self._loadfunc is not _noop:
            lf, self._loadfunc = self._loadfunc, _noop
            lf(self)
        elif self._copyfunc is not _noop:
            cf, self._copyfunc = self._copyfunc, _noop
            cf(self)

    def setflag(self, f, flags):
        """Set the flags (symlink, executable) for path f."""
        self._load()
        dir, subpath = _splittopdir(f)
        if dir:
            if dir not in self._dirs:
                self._dirs[dir] = treemanifest(self._subpath(dir))
            self._dirs[dir].setflag(subpath, flags)
        else:
            self._flags[f] = flags
        self._dirty = True

    def copy(self):
        copy = treemanifest(self._dir)
        copy._node = self._node
        copy._dirty = self._dirty
        if self._copyfunc is _noop:

            def _copyfunc(s):
                self._load()
                for d in self._dirs:
                    s._dirs[d] = self._dirs[d].copy()
                s._files = dict.copy(self._files)
                s._flags = dict.copy(self._flags)

            if self._loadfunc is _noop:
                _copyfunc(copy)
            else:
                copy._copyfunc = _copyfunc
        else:
            copy._copyfunc = self._copyfunc
        return copy

    def filesnotin(self, m2, matcher=None):
        """Set of files in this manifest that are not in the other"""
        if matcher:
            m1 = self.matches(matcher)
            m2 = m2.matches(matcher)
            return m1.filesnotin(m2)

        files = set()

        def _filesnotin(t1, t2):
            if t1._node == t2._node and not t1._dirty and not t2._dirty:
                return
            t1._load()
            t2._load()
            for d, m1 in t1._dirs.iteritems():
                if d in t2._dirs:
                    m2 = t2._dirs[d]
                    _filesnotin(m1, m2)
                else:
                    files.update(m1.iterkeys())

            for fn in t1._files.iterkeys():
                if fn not in t2._files:
                    files.add(t1._subpath(fn))

        _filesnotin(self, m2)
        return files

    @propertycache
    def _alldirs(self):
        return util.dirs(self)

    def dirs(self):
        return self._alldirs

    def hasdir(self, dir):
        self._load()
        topdir, subdir = _splittopdir(dir)
        if topdir:
            if topdir in self._dirs:
                return self._dirs[topdir].hasdir(subdir)
            return False
        return (dir + "/") in self._dirs

    def walk(self, match):
        """Generates matching file names.

        Equivalent to manifest.matches(match).iterkeys(), but without creating
        an entirely new manifest.

        It also reports nonexistent files by marking them bad with match.bad().
        """
        if match.always():
            for f in iter(self):
                yield f
            return

        fset = set(match.files())

        for fn in self._walk(match):
            if fn in fset:
                # specified pattern is the exact name
                fset.remove(fn)
            yield fn

        # for dirstate.walk, files=[''] means "walk the whole tree".
        # follow that here, too
        fset.discard("")

        for fn in sorted(fset):
            if not self.hasdir(fn):
                match.bad(fn, None)

    def _walk(self, match):
        """Recursively generates matching file names for walk()."""
        if not match.visitdir(self._dir[:-1]):
            return

        # yield this dir's files and walk its submanifests
        self._load()
        for p in sorted(self._dirs.keys() + self._files.keys()):
            if p in self._files:
                fullp = self._subpath(p)
                if match(fullp):
                    yield fullp
            else:
                for f in self._dirs[p]._walk(match):
                    yield f

    def matches(self, match):
        """generate a new manifest filtered by the match argument"""
        if match.always():
            return self.copy()

        return self._matches(match)

    def _matches(self, match):
        """recursively generate a new manifest filtered by the match argument.
        """

        visit = match.visitdir(self._dir[:-1])
        if visit == "all":
            return self.copy()
        ret = treemanifest(self._dir)
        if not visit:
            return ret

        self._load()
        for fn in self._files:
            fullp = self._subpath(fn)
            if not match(fullp):
                continue
            ret._files[fn] = self._files[fn]
            if fn in self._flags:
                ret._flags[fn] = self._flags[fn]

        for dir, subm in self._dirs.iteritems():
            m = subm._matches(match)
            if not m._isempty():
                ret._dirs[dir] = m

        if not ret._isempty():
            ret._dirty = True
        return ret

    def diff(self, m2, matcher=None):
        """Finds changes between the current manifest and m2.

        Args:
          m2: the manifest to which this manifest should be compared.

        The result is returned as a dict with filename as key and
        values of the form ((n1,fl1),(n2,fl2)), where n1/n2 is the
        nodeid in the current/other manifest and fl1/fl2 is the flag
        in the current/other manifest. Where the file does not exist,
        the nodeid will be None and the flags will be the empty
        string.
        """
        if matcher:
            m1 = self.matches(matcher)
            m2 = m2.matches(matcher)
            return m1.diff(m2)
        result = {}
        emptytree = treemanifest()

        def _diff(t1, t2):
            if t1._node == t2._node and not t1._dirty and not t2._dirty:
                return
            t1._load()
            t2._load()
            for d, m1 in t1._dirs.iteritems():
                m2 = t2._dirs.get(d, emptytree)
                _diff(m1, m2)

            for d, m2 in t2._dirs.iteritems():
                if d not in t1._dirs:
                    _diff(emptytree, m2)

            for fn, n1 in t1._files.iteritems():
                fl1 = t1._flags.get(fn, "")
                n2 = t2._files.get(fn, None)
                fl2 = t2._flags.get(fn, "")
                if n1 != n2 or fl1 != fl2:
                    result[t1._subpath(fn)] = ((n1, fl1), (n2, fl2))

            for fn, n2 in t2._files.iteritems():
                if fn not in t1._files:
                    fl2 = t2._flags.get(fn, "")
                    result[t2._subpath(fn)] = ((None, ""), (n2, fl2))

        _diff(self, m2)
        return result

    def unmodifiedsince(self, m2):
        return not self._dirty and not m2._dirty and self._node == m2._node

    def parse(self, text, readsubtree):
        for f, n, fl in _parse(text):
            if fl == "t":
                f = f + "/"
                self._dirs[f] = readsubtree(self._subpath(f), n)
            elif "/" in f:
                # This is a flat manifest, so use __setitem__ and setflag rather
                # than assigning directly to _files and _flags, so we can
                # assign a path in a subdirectory, and to mark dirty (compared
                # to nullid).
                self[f] = n
                if fl:
                    self.setflag(f, fl)
            else:
                # Assigning to _files and _flags avoids marking as dirty,
                # and should be a little faster.
                self._files[f] = n
                if fl:
                    self._flags[f] = fl

    def text(self):
        """Get the full data of this manifest as a bytestring."""
        self._load()
        return _text(self.iterentries())

    def dirtext(self):
        """Get the full data of this directory as a bytestring. Make sure that
        any submanifests have been written first, so their nodeids are correct.
        """
        self._load()
        flags = self.flags
        dirs = [(d[:-1], self._dirs[d]._node, "t") for d in self._dirs]
        files = [(f, self._files[f], flags(f)) for f in self._files]
        return _text(sorted(dirs + files))

    def read(self, gettext, readsubtree):
        def _load_for_read(s):
            s.parse(gettext(), readsubtree)
            s._dirty = False

        self._loadfunc = _load_for_read

    def writesubtrees(self, m1, m2, writesubtree):
        self._load()  # for consistency; should never have any effect here
        m1._load()
        m2._load()
        emptytree = treemanifest()
        for d, subm in self._dirs.iteritems():
            subp1 = m1._dirs.get(d, emptytree)._node
            subp2 = m2._dirs.get(d, emptytree)._node
            if subp1 == revlog.nullid:
                subp1, subp2 = subp2, subp1
            writesubtree(subm, subp1, subp2)

    def walksubtrees(self, matcher=None):
        """Returns an iterator of the subtrees of this manifest, including this
        manifest itself.

        If `matcher` is provided, it only returns subtrees that match.
        """
        if matcher and not matcher.visitdir(self._dir[:-1]):
            return
        if not matcher or matcher(self._dir[:-1]):
            yield self

        self._load()
        for d, subm in self._dirs.iteritems():
            for subtree in subm.walksubtrees(matcher=matcher):
                yield subtree


class manifestrevlog(revlog.revlog):
    """A revlog that stores manifest texts. This is responsible for caching the
    full-text manifest contents.
    """

    def __init__(
        self, opener, dir="", dirlogcache=None, indexfile=None, treemanifest=False
    ):
        """Constructs a new manifest revlog

        `indexfile` - used by extensions to have two manifests at once, like
        when transitioning between flatmanifeset and treemanifests.

        `treemanifest` - used to indicate this is a tree manifest revlog. Opener
        options can also be used to make this a tree manifest revlog. The opener
        option takes precedence, so if it is set to True, we ignore whatever
        value is passed in to the constructor.
        """
        # During normal operations, we expect to deal with not more than four
        # revs at a time (such as during commit --amend). When rebasing large
        # stacks of commits, the number can go up, hence the config knob below.
        cachesize = 4
        optiontreemanifest = False
        opts = getattr(opener, "options", None)
        if opts is not None:
            cachesize = opts.get("manifestcachesize", cachesize)
            optiontreemanifest = opts.get("treemanifest", False)

        self._treeondisk = optiontreemanifest or treemanifest

        self._fulltextcache = util.lrucachedict(cachesize)

        if dir:
            assert self._treeondisk, "opts is %r" % opts
            if not dir.endswith("/"):
                dir = dir + "/"

        if indexfile is None:
            indexfile = "00manifest.i"
            if dir:
                indexfile = "meta/" + dir + indexfile

        self._dir = dir
        # The dirlogcache is kept on the root manifest log
        if dir:
            self._dirlogcache = dirlogcache
        else:
            self._dirlogcache = {"": self}

        super(manifestrevlog, self).__init__(
            opener,
            indexfile,
            # only root indexfile is cached
            checkambig=not bool(dir),
            mmaplargeindex=True,
        )

    @property
    def fulltextcache(self):
        return self._fulltextcache

    def clearcaches(self):
        super(manifestrevlog, self).clearcaches()
        self._fulltextcache.clear()
        self._dirlogcache = {"": self}

    def dirlog(self, dir):
        if dir:
            assert self._treeondisk
        if dir not in self._dirlogcache:
            mfrevlog = manifestrevlog(
                self.opener, dir, self._dirlogcache, treemanifest=self._treeondisk
            )
            self._dirlogcache[dir] = mfrevlog
        return self._dirlogcache[dir]

    def add(self, m, transaction, link, p1, p2, added, removed, readtree=None):
        if p1 in self.fulltextcache and util.safehasattr(m, "fastdelta"):
            # If our first parent is in the manifest cache, we can
            # compute a delta here using properties we know about the
            # manifest up-front, which may save time later for the
            # revlog layer.

            _checkforbidden(added)
            # combine the changed lists into one sorted iterator
            work = heapq.merge(
                [(x, False) for x in added], [(x, True) for x in removed]
            )

            arraytext, deltatext = m.fastdelta(self.fulltextcache[p1], work)
            cachedelta = self.rev(p1), deltatext
            text = util.buffer(arraytext)
            n = self.addrevision(text, transaction, link, p1, p2, cachedelta)
        else:
            # The first parent manifest isn't already loaded, so we'll
            # just encode a fulltext of the manifest and pass that
            # through to the revlog layer, and let it handle the delta
            # process.
            if self._treeondisk:
                assert readtree, "readtree must be set for treemanifest writes"
                m1 = readtree(self._dir, p1)
                m2 = readtree(self._dir, p2)
                n = self._addtree(m, transaction, link, m1, m2, readtree)
                arraytext = None
            else:
                text = m.text()
                n = self.addrevision(text, transaction, link, p1, p2)
                arraytext = bytearray(text)

        if arraytext is not None:
            self.fulltextcache[n] = arraytext

        return n

    def _addtree(self, m, transaction, link, m1, m2, readtree):
        # If the manifest is unchanged compared to one parent,
        # don't write a new revision
        if self._dir != "" and (m.unmodifiedsince(m1) or m.unmodifiedsince(m2)):
            return m.node()

        def writesubtree(subm, subp1, subp2):
            sublog = self.dirlog(subm.dir())
            sublog.add(
                subm, transaction, link, subp1, subp2, None, None, readtree=readtree
            )

        m.writesubtrees(m1, m2, writesubtree)
        text = m.dirtext()
        n = None
        if self._dir != "":
            # Double-check whether contents are unchanged to one parent
            if text == m1.dirtext():
                n = m1.node()
            elif text == m2.dirtext():
                n = m2.node()

        if not n:
            n = self.addrevision(text, transaction, link, m1.node(), m2.node())

        # Save nodeid so parent manifest can calculate its nodeid
        m.setnode(n)
        return n


class manifestlog(object):
    """A collection class representing the collection of manifest snapshots
    referenced by commits in the repository.

    In this situation, 'manifest' refers to the abstract concept of a snapshot
    of the list of files in the given commit. Consumers of the output of this
    class do not care about the implementation details of the actual manifests
    they receive (i.e. tree or flat or lazily loaded, etc)."""

    def __init__(self, opener, repo):
        usetreemanifest = False
        cachesize = 4

        self.ui = repo.ui

        opts = getattr(opener, "options", None)
        if opts is not None:
            usetreemanifest = opts.get("treemanifest", usetreemanifest)
            cachesize = opts.get("manifestcachesize", cachesize)
        self._treeinmem = usetreemanifest

        self._opener = opener
        self._revlog = repo._constructmanifest()
        self._repo = repo.unfiltered()

        # A cache of the manifestctx or treemanifestctx for each directory
        self._dirmancache = {}
        self._dirmancache[""] = util.lrucachedict(cachesize)

        self.cachesize = cachesize
        self.recentlinknode = None

    def __nonzero__(self):
        return bool(self._revlog)

    def _maplinknode(self, linknode):
        """Turns a linknode into a linkrev. Only needed for revlog backed
        manifestlogs."""
        linkrev = self._repo.changelog.rev(linknode)
        return linkrev

    def _maplinkrev(self, linkrev):
        """Turns a linkrev into a linknode. Only needed for revlog backed
        manifestlogs."""
        if linkrev >= len(self._repo.changelog):
            raise LookupError(_("linkrev %s not in changelog") % linkrev)
        return self._repo.changelog.node(linkrev)

    def __getitem__(self, node):
        """Retrieves the manifest instance for the given node. Throws a
        LookupError if not found.
        """
        return self.get("", node)

    def get(self, dir, node, verify=True):
        """Retrieves the manifest instance for the given node. Throws a
        LookupError if not found.

        `verify` - if True an exception will be thrown if the node is not in
                   the revlog
        """
        if node in self._dirmancache.get(dir, ()):
            return self._dirmancache[dir][node]

        if dir:
            if self._revlog._treeondisk:
                if verify:
                    dirlog = self._revlog.dirlog(dir)
                    if node not in dirlog.nodemap:
                        raise LookupError(node, dirlog.indexfile, _("no node"))
                m = treemanifestctx(self, dir, node)
            else:
                raise error.Abort(
                    _("cannot ask for manifest directory '%s' in a flat " "manifest")
                    % dir
                )
        else:
            if verify:
                if node not in self._revlog.nodemap:
                    raise LookupError(node, self._revlog.indexfile, _("no node"))
            if self._treeinmem:
                m = treemanifestctx(self, "", node)
            else:
                m = manifestctx(self, node)

        if node != revlog.nullid:
            mancache = self._dirmancache.get(dir)
            if not mancache:
                mancache = util.lrucachedict(self.cachesize)
                self._dirmancache[dir] = mancache
            mancache[node] = m
        return m

    def clearcaches(self):
        self._dirmancache.clear()
        self._revlog.clearcaches()

    def commitpending(self):
        """Used in alternative manifestlog implementations to flush additions to
        disk."""

    def abortpending(self):
        """Used in alternative manifestlog implementations to throw out pending
        additions."""


class memmanifestctx(object):
    def __init__(self, manifestlog):
        self._manifestlog = manifestlog
        self._manifestdict = manifestdict()
        self._node = None
        self._parents = None
        self._linkrev = None

    def _revlog(self):
        return self._manifestlog._revlog

    def new(self):
        return memmanifestctx(self._manifestlog)

    def copy(self):
        memmf = memmanifestctx(self._manifestlog)
        memmf._manifestdict = self.read().copy()
        return memmf

    def read(self):
        return self._manifestdict

    def write(self, transaction, link, p1, p2, added, removed):
        if self._node is not None:
            raise error.ProgrammingError("calling memmanifestctx.write() twice")

        node = self._revlog().add(
            self._manifestdict, transaction, link, p1, p2, added, removed
        )
        self._node = node
        self._parents = (p1, p2)
        self._linkrev = link
        return node

    @property
    def parents(self):
        if self._parents is None:
            raise error.ProgrammingError(
                "accessing memmanifestctx.parents " "before write()"
            )
        return self._parents

    def node(self):
        if self._node is None:
            raise error.ProgrammingError(
                "accessing memmanifestctx.node() " "before write()"
            )
        return self._node

    @property
    def linknode(self):
        if self._linkrev is None:
            raise error.ProgrammingError(
                _("accessing memmanifestctx.linknode " "before write()")
            )
        return self._manifestlog._maplinkrev(self._linkrev)


class manifestctx(object):
    """A class representing a single revision of a manifest, including its
    contents, its parent revs, and its linkrev.
    """

    def __init__(self, manifestlog, node):
        self._manifestlog = manifestlog
        self._data = None

        self._node = node

        # TODO: We eventually want p1, p2, and linkrev exposed on this class,
        # but let's add it later when something needs it and we can load it
        # lazily.
        # self.p1, self.p2 = revlog.parents(node)
        # rev = revlog.rev(node)
        # self.linkrev = revlog.linkrev(rev)

    def _revlog(self):
        return self._manifestlog._revlog

    def node(self):
        return self._node

    def new(self):
        return memmanifestctx(self._manifestlog)

    def copy(self):
        memmf = memmanifestctx(self._manifestlog)
        memmf._manifestdict = self.read().copy()
        return memmf

    @propertycache
    def parents(self):
        return self._revlog().parents(self._node)

    @propertycache
    def linknode(self):
        revlog = self._revlog()
        linkrev = revlog.linkrev(revlog.rev(self._node))
        return self._manifestlog._maplinkrev(linkrev)

    def read(self):
        if self._data is None:
            if self._node == revlog.nullid:
                self._data = manifestdict()
            else:
                rl = self._revlog()
                text = rl.revision(self._node)
                arraytext = bytearray(text)
                rl._fulltextcache[self._node] = arraytext
                self._data = manifestdict(text)
        return self._data

    def readnew(self, shallow=False):
        """Returns the entries that were introduced by this manifest revision.

        If `shallow` is True, it returns only the immediate children in a tree.
        """
        revlog = self._revlog()
        r = revlog.rev(self._node)
        d = mdiff.patchtext(revlog.revdiff(revlog.parentrevs(r)[0], r))
        return manifestdict(d)

    def find(self, key):
        return self.read().find(key)


class memtreemanifestctx(object):
    def __init__(self, manifestlog, dir=""):
        self._manifestlog = manifestlog
        self._dir = dir
        self._treemanifest = treemanifest()

    def _revlog(self):
        return self._manifestlog._revlog

    def new(self, dir=""):
        return memtreemanifestctx(self._manifestlog, dir=dir)

    def copy(self):
        memmf = memtreemanifestctx(self._manifestlog, dir=self._dir)
        memmf._treemanifest = self._treemanifest.copy()
        return memmf

    def read(self):
        return self._treemanifest

    def write(self, transaction, link, p1, p2, added, removed):
        def readtree(dir, node):
            return self._manifestlog.get(dir, node).read()

        return self._revlog().add(
            self._treemanifest,
            transaction,
            link,
            p1,
            p2,
            added,
            removed,
            readtree=readtree,
        )


class treemanifestctx(object):
    def __init__(self, manifestlog, dir, node):
        self._manifestlog = manifestlog
        self._dir = dir
        self._data = None

        self._node = node

        # TODO: Load p1/p2/linkrev lazily. They need to be lazily loaded so that
        # we can instantiate treemanifestctx objects for directories we don't
        # have on disk.
        # self.p1, self.p2 = revlog.parents(node)
        # rev = revlog.rev(node)
        # self.linkrev = revlog.linkrev(rev)

    def _revlog(self):
        return self._manifestlog._revlog.dirlog(self._dir)

    def read(self):
        if self._data is None:
            rl = self._revlog()
            if self._node == revlog.nullid:
                self._data = treemanifest()
            elif rl._treeondisk:
                m = treemanifest(dir=self._dir)

                def gettext():
                    return rl.revision(self._node)

                def readsubtree(dir, subm):
                    # Set verify to False since we need to be able to create
                    # subtrees for trees that don't exist on disk.
                    return self._manifestlog.get(dir, subm, verify=False).read()

                m.read(gettext, readsubtree)
                m.setnode(self._node)
                self._data = m
            else:
                text = rl.revision(self._node)
                arraytext = bytearray(text)
                rl.fulltextcache[self._node] = arraytext
                self._data = treemanifest(dir=self._dir, text=text)

        return self._data

    def node(self):
        return self._node

    def new(self, dir=""):
        return memtreemanifestctx(self._manifestlog, dir=dir)

    def copy(self):
        memmf = memtreemanifestctx(self._manifestlog, dir=self._dir)
        memmf._treemanifest = self.read().copy()
        return memmf

    @propertycache
    def parents(self):
        return self._revlog().parents(self._node)

    def readnew(self, shallow=False):
        """Returns the entries that were introduced by this manifest revision.

        If `shallow` is True, it returns only the immediate children in a tree.
        """
        revlog = self._revlog()
        if shallow:
            r = revlog.rev(self._node)
            d = mdiff.patchtext(revlog.revdiff(revlog.parentrevs(r)[0], r))
            return manifestdict(d)
        else:
            # Need to perform a slow delta
            r0 = revlog.parentrevs(revlog.rev(self._node))[0]
            m0 = self._manifestlog.get(self._dir, revlog.node(r0)).read()
            m1 = self.read()
            md = treemanifest(dir=self._dir)
            for f, ((n0, fl0), (n1, fl1)) in m0.diff(m1).iteritems():
                if n1:
                    md[f] = n1
                    if fl1:
                        md.setflag(f, fl1)
            return md

    def find(self, key):
        return self.read().find(key)
