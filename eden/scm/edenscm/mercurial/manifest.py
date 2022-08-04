# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# manifest.py - manifest revision class for mercurial
#
# Copyright 2005-2007 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import heapq
import struct

from edenscmnative import parsers

from . import error, mdiff, pycompat, revlog, util
from .i18n import _
from .node import bin
from .pycompat import encodeutf8, unicode


propertycache = util.propertycache


def _parse(data):
    """Generates (path, node, flags) tuples from a manifest text"""
    # This method does a little bit of excessive-looking
    # precondition checking. This is so that the behavior of this
    # class exactly matches its C counterpart to try and help
    # prevent surprise breakage for anyone that develops against
    # the pure version.
    if data and data[-1:] != b"\n":
        raise ValueError("Manifest did not end in a newline.")
    prev = None
    for l in data.splitlines():
        if prev is not None and prev > l:
            raise ValueError("Manifest lines not in sorted order.")
        prev = l
        f, n = pycompat.decodeutf8(l).split("\0")
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
    return pycompat.encodeutf8("".join(lines))


def unhexlify(data, extra, pos, length):
    s = bin(data[pos : pos + length])
    if extra:
        s += chr(extra & 0xFF)
    return s


def _cmp(a, b):
    return (a > b) - (a < b)


_lazymanifest = parsers.lazymanifest


class manifestdict(object):
    def __init__(self, data=b""):
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
        return pycompat.iterkeys(self._lm)

    def keys(self):
        return list(self._lm.keys())

    def filesnotin(self, m2, matcher=None):
        """Set of files in this manifest that are not in the other"""
        if matcher:
            m1 = self.matches(matcher)
            m2 = m2.matches(matcher)
            return m1.filesnotin(m2)
        diff = self.diff(m2)
        files = set(
            filepath
            for filepath, hashflags in pycompat.iteritems(diff)
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
        dline = [b""]
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
                    l = encodeutf8("%s\0%s%s\n" % (f, revlog.hex(h), fl))
                else:
                    if start == end:
                        # item we want to delete was not found, error out
                        raise AssertionError(_("failed to remove %s from manifest") % f)
                    l = b""
                if dstart is not None and dstart <= start and dend >= start:
                    if dend < end:
                        dend = end
                    if l:
                        dline.append(l)
                else:
                    if dstart is not None:
                        delta.append([dstart, dend, b"".join(dline)])
                    dstart = start
                    dend = end
                    dline = [l]

            if dstart is not None:
                delta.append([dstart, dend, b"".join(dline)])
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
    s is str"""
    assert not isinstance(m, unicode)
    assert isinstance(s, str)
    s = encodeutf8(s)

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
        while start > 0 and m[start - 1 : start] != b"\n":
            start -= 1
        end = advance(start, b"\0")
        if bytes(m[start:end]) < s:
            # we know that after the null there are 40 bytes of sha1
            # this translates to the bisect lo = mid + 1
            lo = advance(end + 40, b"\n") + 1
        else:
            # this translates to the bisect hi = mid
            hi = start
    end = advance(lo, b"\0")
    found = m[lo:end]
    if s == found:
        # we know that after the null there are 40 bytes of sha1
        end = advance(end + 40, b"\n")
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

    deltatext = b"".join(
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
        self._repo = repo

        # A cache of the manifestctx or treemanifestctx for each directory
        self._dirmancache = {}
        self._dirmancache[""] = util.lrucachedict(cachesize)

        self.cachesize = cachesize
        self.recentlinknode = None

    def __nonzero__(self):
        return bool(self._revlog)

    __bool__ = __nonzero__

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
            raise error.Abort(
                _("cannot ask for manifest directory '%s' in a flat manifest") % dir
            )
        else:
            if verify:
                if node not in self._revlog.nodemap:
                    raise LookupError(node, self._revlog.indexfile, _("no node"))
            if self._treeinmem:
                raise error.Abort("legacy upstream treemanifest no longer supported")
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
